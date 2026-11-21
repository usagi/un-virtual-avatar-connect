//! Avatar playback completes through the trigger bus without blocking other ingress.
use crate::flowgraph::node::{InputMap, NodeExecError, NodeOutput, TriggerEvent, TriggerHandle};
use crate::flowgraph::socket::SocketValue;
use std::{
	collections::HashMap,
	future::Future,
	sync::{
		atomic::{AtomicU64, Ordering},
		Mutex,
	},
};

pub(super) const RESUME: &str = "__avatar_complete__";
pub(super) const JOB_ID: &str = "__avatar_job_id__";
type PlaybackResult = Result<NodeOutput, NodeExecError>;

struct Job {
	node_id: String,
	bus: TriggerHandle,
	task: tokio::task::JoinHandle<PlaybackResult>,
}
impl Drop for Job {
	fn drop(&mut self) {
		self.task.abort();
	}
}

#[derive(Default)]
pub(super) struct AvatarJobs {
	next: AtomicU64,
	jobs: Mutex<HashMap<u64, Job>>,
}
impl AvatarJobs {
	#[cfg(test)]
	pub fn start(
		&self,
		node_id: String,
		bus: TriggerHandle,
		work: impl Future<Output = PlaybackResult> + Send + 'static,
	) -> Result<(), String> {
		self.start_with_inputs(node_id, bus, InputMap::new(), work)
	}
	pub fn start_with_inputs(
		&self,
		node_id: String,
		bus: TriggerHandle,
		inputs: InputMap,
		work: impl Future<Output = PlaybackResult> + Send + 'static,
	) -> Result<(), String> {
		let mut jobs = self.jobs.lock().unwrap();
		jobs.retain(|_, job| !job.bus.is_closed());
		if jobs.len() >= 32 {
			return Err("too many pending avatar playbacks".into());
		}
		let id = self.next.fetch_add(1, Ordering::Relaxed);
		let task_bus = bus.clone();
		let task_node = node_id.clone();
		let task = tokio::spawn(async move {
			let result = tokio::select! {
				biased;
				_ = task_bus.closed() => return Err(NodeExecError::Generic(anyhow::anyhow!("avatar graph closed"))),
				result = work => result,
			};
			let mut event = TriggerEvent::new(task_node).with_exec(RESUME);
			event.data_overrides = inputs;
			let _ = task_bus.send(event.with_override(JOB_ID, SocketValue::Int(id as i64)));
			result
		});
		jobs.insert(id, Job { node_id, bus, task });
		Ok(())
	}
	pub async fn finish(&self, node_id: &str, id: u64) -> PlaybackResult {
		let mut job = {
			let mut jobs = self.jobs.lock().unwrap();
			if !jobs.get(&id).is_some_and(|job| job.node_id == node_id) {
				return Err(NodeExecError::Generic(anyhow::anyhow!("unknown avatar completion")));
			}
			jobs.remove(&id).unwrap()
		};
		(&mut job.task)
			.await
			.map_err(|error| NodeExecError::Generic(anyhow::anyhow!("avatar task: {error}")))?
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::Duration;
	#[tokio::test]
	async fn completion_in_new_generation_keeps_original_ingress_inputs() {
		use crate::flowgraph::nodes::{
			avatar::AvatarIntentPlayUnmfzNode, ingress::AiAvatarIntentIngressNode, json_ops::JsonParseNode, log::LogNode,
		};
		use crate::flowgraph::{
			engine::{create_trigger_bus, FlowgraphBuilder, PortRef},
			node::{ExecCtx, NodeImpl},
		};
		use std::sync::Arc;
		let mut builder = FlowgraphBuilder::new();
		builder.add_node("ingress", NodeImpl::pure(Arc::new(AiAvatarIntentIngressNode)), InputMap::new());
		builder.add_node("parse", NodeImpl::pure(Arc::new(JsonParseNode)), InputMap::new());
		builder.add_node(
			"play",
			NodeImpl::effectful(Arc::new(AvatarIntentPlayUnmfzNode::default())),
			InputMap::new(),
		);
		builder.add_node("log", NodeImpl::effectful(Arc::new(LogNode)), InputMap::new());
		builder.connect_exec(PortRef::new("ingress", "exec_out"), PortRef::new("play", "exec_in"));
		builder.connect(PortRef::new("ingress", "content"), PortRef::new("parse", "text"));
		builder.connect(PortRef::new("parse", "value"), PortRef::new("play", "intent"));
		builder.connect_exec(PortRef::new("play", "on_error"), PortRef::new("log", "exec_in"));
		builder.connect(PortRef::new("play", "error"), PortRef::new("log", "value"));
		let mut program = builder.build().unwrap();
		let (bus, rx) = create_trigger_bus();
		bus.send(
			TriggerEvent::new("ingress")
				.with_exec("__trigger__")
				.with_override("__content__", SocketValue::String(r#"{"intents":"not-an-array"}"#.into())),
		)
		.unwrap();
		let mut ctx = ExecCtx::default();
		program
			.run_forever_with_bus(&mut ctx, bus, rx, tokio::time::sleep(Duration::from_millis(100)), None)
			.await
			.unwrap();
		assert_eq!(ctx.trace.len(), 1);
		assert!(ctx.trace[0].contains("AvatarIntent JSON decode"));
	}
	#[tokio::test]
	async fn avatar_node_delivers_failure_only_on_completion_trigger() {
		use crate::flowgraph::node::{EffectfulNode, ExecCtx, ExecFireSet, InputMap};
		let node = super::super::avatar::AvatarIntentPlayUnmfzNode::default();
		let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
		let mut ctx = ExecCtx {
			node_id: "intent".into(),
			trigger: Some(TriggerHandle::new(tx)),
			..Default::default()
		};
		let mut inputs = InputMap::new();
		inputs.insert("intent".into(), SocketValue::Json(serde_json::json!({})));
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		let accepted = node.execute(&mut ctx, &InputMap::new(), &inputs, &fired).await.unwrap();
		assert!(accepted.fired_exec.is_empty());
		let completed = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await.unwrap().unwrap();
		let mut resumed = ExecFireSet::new();
		resumed.insert(RESUME);
		let output = node
			.execute(&mut ctx, &InputMap::new(), &completed.data_overrides, &resumed)
			.await
			.unwrap();
		assert!(output.fired_exec.contains("on_error"));
		assert!(!output.fired_exec.contains("on_success"));
	}
	#[tokio::test]
	async fn independent_playbacks_start_before_either_completes() {
		let jobs = AvatarJobs::default();
		let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
		let bus = TriggerHandle::new(tx);
		let (start_tx, mut start_rx) = tokio::sync::mpsc::unbounded_channel();
		let (finish_a, gate_a) = tokio::sync::oneshot::channel();
		let (finish_b, gate_b) = tokio::sync::oneshot::channel();
		for (name, gate) in [("expression", gate_a), ("pose", gate_b)] {
			let start = start_tx.clone();
			jobs.start(name.into(), bus.clone(), async move {
				start.send(name).unwrap();
				gate.await.unwrap();
				Ok(NodeOutput::new().fire_exec("on_success"))
			})
			.unwrap();
		}
		for _ in 0..2 {
			tokio::time::timeout(Duration::from_secs(1), start_rx.recv())
				.await
				.unwrap()
				.unwrap();
		}
		assert!(rx.try_recv().is_err(), "acceptance is not playback completion");
		finish_b.send(()).unwrap();
		let event = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await.unwrap().unwrap();
		assert_eq!(event.node_id, "pose");
		let SocketValue::Int(id) = event.data_overrides[JOB_ID] else {
			panic!("missing job ID")
		};
		assert!(jobs.finish("expression", id as u64).await.is_err());
		jobs.finish("pose", id as u64).await.unwrap();
		finish_a.send(()).unwrap();
		let event = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await.unwrap().unwrap();
		assert_eq!(event.node_id, "expression");
	}
	#[tokio::test]
	async fn graph_close_drops_running_future_for_neutral_cleanup() {
		struct Cleanup(Option<tokio::sync::oneshot::Sender<()>>);
		impl Drop for Cleanup {
			fn drop(&mut self) {
				let _ = self.0.take().unwrap().send(());
			}
		}
		let jobs = AvatarJobs::default();
		let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
		let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
		let (clean_tx, clean_rx) = tokio::sync::oneshot::channel();
		jobs.start("pose".into(), TriggerHandle::new(tx), async move {
			let _guard = Cleanup(Some(clean_tx));
			ready_tx.send(()).unwrap();
			std::future::pending::<()>().await;
			Ok(NodeOutput::new())
		})
		.unwrap();
		ready_rx.await.unwrap();
		drop(rx);
		tokio::time::timeout(Duration::from_secs(1), clean_rx).await.unwrap().unwrap();
	}
	#[tokio::test]
	async fn pending_work_is_bounded() {
		let jobs = AvatarJobs::default();
		let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
		let bus = TriggerHandle::new(tx);
		for _ in 0..32 {
			jobs.start("pose".into(), bus.clone(), std::future::pending()).unwrap();
		}
		assert!(jobs.start("pose".into(), bus, std::future::pending()).is_err());
	}
}
