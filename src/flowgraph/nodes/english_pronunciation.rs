//! Rust製english2kana workerを使う、TTS向けの明示的な英字発音前処理。

use std::{
	any::Any,
	io::{BufRead, BufReader, Write},
	path::{Path, PathBuf},
	process::{Child, ChildStdin, ChildStdout, Command, Stdio},
	sync::{Arc, Mutex},
};

use async_trait::async_trait;
use serde::Deserialize;

use crate::flowgraph::{
	node::{
		get_optional_float, get_optional_string, get_required_string, ExecFireSet, InputMap, NodeDescriptor, NodeExecError, NodeOutput,
		NodeSpec, PortSpec, StatefulCtx, StatefulNode,
	},
	socket::{SocketType, SocketValue},
};

pub struct EnglishToKatakanaNode;

impl NodeDescriptor for EnglishToKatakanaNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.string.english_to_katakana".into(),
			title: "English to Katakana (ONNX)".into(),
			category: "string".into(),
			description: Some("Rust製workerのenglish2kana ONNXでASCII英単語をカタカナ化。失敗時は原文を維持".into()),
			inputs: vec![
				PortSpec::input("content", "Content", SocketType::String),
				PortSpec::input("model_dir", "Model Directory", SocketType::String)
					.with_default(SocketValue::String(String::new())),
				PortSpec::input("worker_path", "Worker Executable", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("min_confidence", "Minimum Confidence", SocketType::Float).with_default(SocketValue::Float(0.55)),
			],
			outputs: vec![
				PortSpec::output("result", "Result", SocketType::String),
				PortSpec::output("converted_count", "Converted Count", SocketType::Int),
				PortSpec::output("available", "Model Available", SocketType::Bool),
				PortSpec::output("error", "Error", SocketType::String),
				PortSpec::output("trace", "Trace", SocketType::Json),
			],
			properties: vec![],
		}
	}
}

#[derive(Debug, Deserialize)]
struct WorkerResponse {
	id: u64,
	ok: bool,
	result: Option<WorkerResult>,
	error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WorkerResult {
	text: String,
	converted_count: usize,
	spans: serde_json::Value,
}

struct WorkerClient {
	child: Child,
	stdin: ChildStdin,
	stdout: BufReader<ChildStdout>,
	next_id: u64,
}

impl WorkerClient {
	fn start(worker_path: &Path, model_dir: &str) -> Result<Self, String> {
		let mut command = Command::new(worker_path);
		command
			.arg("--model-dir")
			.arg(model_dir)
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::null());
		#[cfg(windows)]
		{
			use std::os::windows::process::CommandExt;
			command.creation_flags(0x0800_0000);
		}
		let mut child = command
			.spawn()
			.map_err(|error| format!("worker起動失敗 {}: {error}", worker_path.display()))?;
		let stdin = child.stdin.take().ok_or_else(|| "worker stdinを取得できない".to_string())?;
		let stdout = child.stdout.take().ok_or_else(|| "worker stdoutを取得できない".to_string())?;
		Ok(Self {
			child,
			stdin,
			stdout: BufReader::new(stdout),
			next_id: 1,
		})
	}

	fn convert(&mut self, content: &str, min_confidence: f32) -> Result<WorkerResult, String> {
		let id = self.next_id;
		self.next_id = self.next_id.wrapping_add(1).max(1);
		serde_json::to_writer(
			&mut self.stdin,
			&serde_json::json!({ "id": id, "content": content, "min_confidence": min_confidence }),
		)
		.map_err(|error| format!("worker request encode失敗: {error}"))?;
		self.stdin
			.write_all(b"\n")
			.map_err(|error| format!("worker stdin書込失敗: {error}"))?;
		self.stdin.flush().map_err(|error| format!("worker stdin flush失敗: {error}"))?;
		let mut line = String::new();
		self.stdout
			.read_line(&mut line)
			.map_err(|error| format!("worker stdout読込失敗: {error}"))?;
		if line.is_empty() {
			let status = self
				.child
				.try_wait()
				.ok()
				.flatten()
				.map_or_else(|| "running".into(), |status| status.to_string());
			return Err(format!("workerが応答せず終了した: status={status}"));
		}
		let response: WorkerResponse = serde_json::from_str(&line).map_err(|error| format!("worker response decode失敗: {error}"))?;
		if response.id != id {
			return Err(format!("worker response id不一致: expected={id}, actual={}", response.id));
		}
		if !response.ok {
			return Err(response.error.unwrap_or_else(|| "worker inference失敗".into()));
		}
		response.result.ok_or_else(|| "worker responseにresultがない".into())
	}
}

impl Drop for WorkerClient {
	fn drop(&mut self) {
		let _ = self.child.kill();
		let _ = self.child.wait();
	}
}

#[derive(Default)]
struct EnglishToKatakanaState {
	key: Option<(PathBuf, String)>,
	client: Option<Arc<Mutex<WorkerClient>>>,
}

impl EnglishToKatakanaState {
	fn client(&mut self, worker_path: PathBuf, model_dir: &str) -> Result<Arc<Mutex<WorkerClient>>, String> {
		let key = (worker_path.clone(), model_dir.to_string());
		if self.key.as_ref() != Some(&key) || self.client.is_none() {
			self.key = Some(key);
			self.client = Some(Arc::new(Mutex::new(WorkerClient::start(&worker_path, model_dir)?)));
		}
		self.client.clone().ok_or_else(|| "worker clientを利用できない".into())
	}
}

fn resolve_worker_path(configured: &str) -> Result<PathBuf, String> {
	if !configured.trim().is_empty() {
		return Ok(PathBuf::from(configured));
	}
	let executable = std::env::current_exe().map_err(|error| format!("実行ファイル位置を取得できない: {error}"))?;
	let name = if cfg!(windows) {
		"vac-english2kana-worker.exe"
	} else {
		"vac-english2kana-worker"
	};
	Ok(executable.parent().unwrap_or_else(|| Path::new(".")).join(name))
}

fn resolve_model_dir(configured: &str) -> Result<PathBuf, String> {
	if !configured.trim().is_empty() {
		// Explicit relative paths retain their profile/working-directory semantics.
		return Ok(PathBuf::from(configured));
	}
	let executable = std::env::current_exe().map_err(|error| format!("実行ファイル位置を取得できない: {error}"))?;
	Ok(bundled_model_dir(&executable))
}

fn bundled_model_dir(executable: &Path) -> PathBuf {
	executable.parent().unwrap_or_else(|| Path::new(".")).join("models").join("english2kana")
}

fn fail_open(content: String, model_dir: &str, error: String) -> NodeOutput {
	let trace = serde_json::json!({
		"source": "english2kana_onnx_worker",
		"model_dir": model_dir,
		"available": false,
		"error": error,
		"spans": [],
	});
	NodeOutput::new()
		.set_data("result", SocketValue::String(content))
		.set_data("converted_count", SocketValue::Int(0))
		.set_data("available", SocketValue::Bool(false))
		.set_data("error", SocketValue::String(error))
		.set_data("trace", SocketValue::Json(trace))
}

#[async_trait]
impl StatefulNode for EnglishToKatakanaNode {
	fn init_state(&self) -> Box<dyn Any + Send> {
		Box::new(EnglishToKatakanaState::default())
	}

	async fn compute(
		&self,
		state: &mut (dyn Any + Send),
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
		_ctx: &StatefulCtx<'_>,
	) -> Result<NodeOutput, NodeExecError> {
		let state = state.downcast_mut::<EnglishToKatakanaState>().expect("EnglishToKatakanaState");
		let content = get_required_string(inputs, "content")?;
		let configured_model_dir = get_optional_string(inputs, "model_dir", "")?;
		let model_dir = match resolve_model_dir(&configured_model_dir) {
			Ok(path) => path.to_string_lossy().into_owned(),
			Err(error) => return Ok(fail_open(content, &configured_model_dir, error)),
		};
		let worker_path = get_optional_string(inputs, "worker_path", "")?;
		let min_confidence = get_optional_float(inputs, "min_confidence", 0.55)?.clamp(0.0, 1.0) as f32;
		let resolved_worker = match resolve_worker_path(&worker_path) {
			Ok(path) => path,
			Err(error) => return Ok(fail_open(content, &model_dir, error)),
		};
		let client = match state.client(resolved_worker, &model_dir) {
			Ok(client) => client,
			Err(error) => return Ok(fail_open(content, &model_dir, error)),
		};
		let request_content = content.clone();
		let converted = tokio::task::spawn_blocking(move || {
			client
				.lock()
				.map_err(|_| "worker client lockがpoisonされた".to_string())?
				.convert(&request_content, min_confidence)
		})
		.await;
		let converted = match converted {
			Ok(Ok(result)) => result,
			Ok(Err(error)) => {
				state.client = None;
				return Ok(fail_open(content, &model_dir, error));
			}
			Err(error) => {
				state.client = None;
				return Ok(fail_open(content, &model_dir, format!("worker join失敗: {error}")));
			}
		};
		let trace = serde_json::json!({
			"source": "english2kana_onnx_worker",
			"model_dir": model_dir,
			"available": true,
			"min_confidence": min_confidence,
			"spans": converted.spans,
		});
		Ok(NodeOutput::new()
			.set_data("result", SocketValue::String(converted.text))
			.set_data("converted_count", SocketValue::Int(converted.converted_count as i64))
			.set_data("available", SocketValue::Bool(true))
			.set_data("error", SocketValue::String(String::new()))
			.set_data("trace", SocketValue::Json(trace)))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn default_model_is_beside_executable_not_profile_working_directory() {
		let executable = Path::new("package").join("vac.exe");
		assert_eq!(bundled_model_dir(&executable), Path::new("package").join("models").join("english2kana"));
		assert_eq!(resolve_model_dir("").unwrap(), bundled_model_dir(&std::env::current_exe().unwrap()));
		assert_eq!(resolve_model_dir(" ").unwrap(), resolve_model_dir("").unwrap());
	}

	#[test]
	fn explicit_model_paths_are_not_rebased_or_silently_replaced() {
		assert_eq!(resolve_model_dir("models/english2kana").unwrap(), PathBuf::from("models/english2kana"));
		let absolute = std::env::temp_dir().join("custom-kana-model");
		assert_eq!(resolve_model_dir(&absolute.to_string_lossy()).unwrap(), absolute);
	}

	#[tokio::test]
	async fn missing_worker_fails_open_with_diagnostics() {
		let node = EnglishToKatakanaNode;
		let mut state = node.init_state();
		let inputs: InputMap = [
			("content".into(), SocketValue::String("hello Twitch".into())),
			("worker_path".into(), SocketValue::String("definitely/missing/worker.exe".into())),
		]
		.into_iter()
		.collect();
		let output = node
			.compute(
				state.as_mut(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
				&StatefulCtx {
					node_id: "test",
					trigger: None,
				},
			)
			.await
			.unwrap();
		assert_eq!(output.data.get("result"), Some(&SocketValue::String("hello Twitch".into())));
		assert_eq!(output.data.get("available"), Some(&SocketValue::Bool(false)));
	}
}
