use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::flowgraph::loader::{parse_flowgraph_file, FlowgraphFile, NodeEntry};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractStatus {
	Ready,
	Attention,
	Blocked,
	Inactive,
}

#[derive(Clone, Debug, Serialize)]
pub struct ContractCheck {
	pub id: &'static str,
	pub status: ContractStatus,
	pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct StreamingContractAudit {
	pub status: ContractStatus,
	pub checks: Vec<ContractCheck>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ChatOutputRoute {
	pub subscription_node_id: String,
	pub tts_node_id: String,
}

struct GraphFile {
	path: PathBuf,
	graph: FlowgraphFile,
}

pub fn audit_streaming_contract(root: &Path) -> Result<StreamingContractAudit> {
	let files = load_graph_files(root)?;
	let voice_count = feature_count(&files, "flowgraph.ingress.voice");
	let has_ai_output =
		feature_count(&files, "flowgraph.ingress.ai_response") > 0 || feature_count(&files, "flowgraph.ingress.ai_speech") > 0;
	// This audit describes the AI cohost speech/subtitle contract, not a
	// requirement for every Twitch automation or microphone-only graph.
	if !has_ai_output {
		return Ok(StreamingContractAudit {
			status: ContractStatus::Inactive,
			checks: Vec::new(),
		});
	}

	let mut checks = vec![match voice_count {
		1 => ready("voice_ingress", "Voice Ingressは1経路。"),
		0 => blocked("voice_ingress", "Voice Ingressがない。"),
		count => attention(
			"voice_ingress",
			format!("Voice Ingressが{count}経路ある。比較試験でなければ1経路へ絞る。"),
		),
	}];

	let assistant_full = matching_feature_nodes(&files, "flowgraph.ingress.ai_response");
	let assistant_full_subtitle = assistant_full
		.iter()
		.filter(|(file, node)| reaches_bos_destination(file, &node.id, "ai-synth"))
		.count();
	let assistant_full_tts = assistant_full
		.iter()
		.filter(|(file, node)| !reachable_nodes(file, &node.id, "flowgraph.tts.speak").is_empty())
		.count();
	checks.push(if assistant_full_subtitle == 1 && assistant_full_tts == 0 {
		ready("assistant_subtitle", "ai全文はai-synth字幕だけへ送られる。")
	} else {
		blocked(
			"assistant_subtitle",
			format!("ai全文の字幕経路={assistant_full_subtitle}、TTS直結={assistant_full_tts}。字幕1・TTS直結0が必要。"),
		)
	});

	let assistant_speech = matching_feature_nodes(&files, "flowgraph.ingress.ai_speech");
	let assistant_tts = matching_tts_routes(&assistant_speech, "assistant");
	checks.push(match assistant_tts {
		1 => ready(
			"assistant_tts",
			"speech_segment付きai.speechはassistant laneへnon-blocking投入される。",
		),
		0 => blocked("assistant_tts", "ai.speechからassistant laneへのnon-blocking TTS経路がない。"),
		count => attention(
			"assistant_tts",
			format!("assistant TTS経路が{count}本ある。二重読み上げを確認する。"),
		),
	});

	let twitch_routes = files
		.iter()
		.flat_map(|file| {
			file.graph
				.nodes
				.iter()
				.filter(|node| node.feature == "flowgraph.ingress.twitch" && property_str(node, "mode") == Some("irc"))
				.map(move |node| (file, node))
		})
		.filter(|(file, node)| reaches_chat_bos_destination(file, &node.id))
		.count();
	checks.push(match twitch_routes {
		1 => ready("twitch_chat", "Twitch IRCはchat/chat-synth BOSへ明示出力される。"),
		0 => blocked("twitch_chat", "Twitch IRCからchat/chat-synth BOSへの明示経路がない。"),
		count => attention(
			"twitch_chat",
			format!("Twitch IRCからchatへの経路が{count}本ある。重複受信を確認する。"),
		),
	});

	let chat = matching_feature_nodes(&files, "flowgraph.ingress.twitch");
	let chat_subtitle = chat
		.iter()
		.filter(|(file, node)| reaches_chat_bos_destination(file, &node.id))
		.count();
	let chat_tts = matching_tts_routes(&chat, "chat");
	checks.push(if chat_subtitle == 1 && chat_tts == 1 {
		ready("chat_output", "Twitch chatはBOS字幕とchat lane TTSへ送られる。")
	} else {
		blocked(
			"chat_output",
			format!("BOS chat字幕経路={chat_subtitle}、chat lane TTS経路={chat_tts}。各1本が必要。"),
		)
	});

	let status = if checks.iter().any(|check| check.status == ContractStatus::Blocked) {
		ContractStatus::Blocked
	} else if checks.iter().any(|check| check.status == ContractStatus::Attention) {
		ContractStatus::Attention
	} else {
		ContractStatus::Ready
	};
	Ok(StreamingContractAudit { status, checks })
}

/// Twitch ingressからBOS chatとchat lane TTSの双方へ到達する実行対象を返す。
pub fn chat_output_routes(root: &Path) -> Result<Vec<ChatOutputRoute>> {
	let files = load_graph_files(root)?;
	let mut routes = Vec::new();
	for (file, subscription) in matching_feature_nodes(&files, "flowgraph.ingress.twitch") {
		if !reaches_chat_bos_destination(file, &subscription.id) {
			continue;
		}
		for tts_node_id in reachable_nodes(file, &subscription.id, "flowgraph.tts.speak") {
			if resolved_input(file, &tts_node_id, "lane").and_then(toml::Value::as_str) != Some("chat")
				|| resolved_input(file, &tts_node_id, "non_blocking").and_then(toml::Value::as_bool) != Some(true)
			{
				continue;
			}
			routes.push(ChatOutputRoute {
				subscription_node_id: fully_qualified_node(root, &file.path, &subscription.id)?,
				tts_node_id: fully_qualified_node(root, &file.path, &tts_node_id)?,
			});
		}
	}
	routes.sort_by(|left, right| {
		left.subscription_node_id
			.cmp(&right.subscription_node_id)
			.then_with(|| left.tts_node_id.cmp(&right.tts_node_id))
	});
	Ok(routes)
}

fn fully_qualified_node(root: &Path, path: &Path, node_id: &str) -> Result<String> {
	let relative = path
		.strip_prefix(root)
		.with_context(|| format!("Flowgraph pathがroot外です: {}", path.display()))?;
	let relative = relative.to_string_lossy().replace('\\', "/");
	let graph_id = relative
		.strip_suffix(".flowgraph.toml")
		.with_context(|| format!("Flowgraph拡張子が不正です: {}", path.display()))?;
	Ok(format!("{graph_id}::{node_id}"))
}

fn ready(id: &'static str, detail: impl Into<String>) -> ContractCheck {
	ContractCheck {
		id,
		status: ContractStatus::Ready,
		detail: detail.into(),
	}
}

fn attention(id: &'static str, detail: impl Into<String>) -> ContractCheck {
	ContractCheck {
		id,
		status: ContractStatus::Attention,
		detail: detail.into(),
	}
}

fn blocked(id: &'static str, detail: impl Into<String>) -> ContractCheck {
	ContractCheck {
		id,
		status: ContractStatus::Blocked,
		detail: detail.into(),
	}
}

fn load_graph_files(root: &Path) -> Result<Vec<GraphFile>> {
	let mut files = Vec::new();
	let mut directories = vec![root.to_path_buf()];
	while let Some(directory) = directories.pop() {
		for entry in std::fs::read_dir(&directory).with_context(|| format!("Flowgraph directoryを読めません: {}", directory.display()))?
		{
			let entry = entry?;
			let kind = entry.file_type()?;
			let path = entry.path();
			if kind.is_dir() {
				directories.push(path);
				continue;
			}
			let name = path.file_name().and_then(|value| value.to_str()).unwrap_or_default();
			if !kind.is_file() || !name.ends_with(".flowgraph.toml") {
				continue;
			}
			let source = std::fs::read_to_string(&path).with_context(|| format!("Flowgraphを読めません: {}", path.display()))?;
			let graph = parse_flowgraph_file(&source, Some(&path)).map_err(|error| anyhow::anyhow!(error.to_string()))?;
			files.push(GraphFile { path, graph });
		}
	}
	files.sort_by(|left, right| left.path.cmp(&right.path));
	Ok(files)
}

fn feature_count(files: &[GraphFile], feature: &str) -> usize {
	files
		.iter()
		.flat_map(|file| file.graph.nodes.iter())
		.filter(|node| node.feature == feature)
		.count()
}

fn property_str<'a>(node: &'a NodeEntry, key: &str) -> Option<&'a str> {
	node.properties.get(key).and_then(toml::Value::as_str)
}

fn matching_feature_nodes<'a>(files: &'a [GraphFile], feature: &str) -> Vec<(&'a GraphFile, &'a NodeEntry)> {
	files
		.iter()
		.flat_map(|file| {
			file.graph
				.nodes
				.iter()
				.filter(|node| node.feature == feature)
				.map(move |node| (file, node))
		})
		.collect()
}

fn matching_tts_routes(subscriptions: &[(&GraphFile, &NodeEntry)], lane: &str) -> usize {
	subscriptions
		.iter()
		.flat_map(|(file, node)| {
			reachable_nodes(file, &node.id, "flowgraph.tts.speak")
				.into_iter()
				.map(move |id| (*file, id))
		})
		.filter(|(file, node_id)| {
			resolved_input(file, node_id, "lane").and_then(toml::Value::as_str) == Some(lane)
				&& resolved_input(file, node_id, "non_blocking").and_then(toml::Value::as_bool) == Some(true)
		})
		.count()
}

fn reaches_bos_destination(file: &GraphFile, start: &str, destination: &str) -> bool {
	reachable_nodes(file, start, "flowgraph.bos.output")
		.iter()
		.any(|node_id| resolved_input(file, node_id, "destination").and_then(toml::Value::as_str) == Some(destination))
}

pub(crate) const CHAT_BOS_CHANNELS: [&str; 2] = ["chat", "chat-synth"];

fn reaches_chat_bos_destination(file: &GraphFile, start: &str) -> bool {
	CHAT_BOS_CHANNELS
		.into_iter()
		.any(|destination| reaches_bos_destination(file, start, destination))
}

fn reachable_nodes(file: &GraphFile, start: &str, feature: &str) -> Vec<String> {
	let mut queue = VecDeque::from([start.to_string()]);
	let mut seen = HashSet::new();
	let mut found = Vec::new();
	while let Some(node_id) = queue.pop_front() {
		if !seen.insert(node_id.clone()) {
			continue;
		}
		if node_id != start && file.graph.nodes.iter().any(|node| node.id == node_id && node.feature == feature) {
			found.push(node_id.clone());
		}
		for edge in &file.graph.edges {
			let Some((from_node, _)) = split_port(&edge.from) else {
				continue;
			};
			let Some((to_node, to_port)) = split_port(&edge.to) else {
				continue;
			};
			if from_node == node_id && to_port == "exec_in" {
				queue.push_back(to_node.to_string());
			}
		}
	}
	found
}

fn resolved_input<'a>(file: &'a GraphFile, target_node: &str, target_port: &str) -> Option<&'a toml::Value> {
	let target = file.graph.nodes.iter().find(|node| node.id == target_node)?;
	if let Some(value) = target.properties.get(target_port) {
		return Some(value);
	}
	let edge = file
		.graph
		.edges
		.iter()
		.find(|edge| split_port(&edge.to).is_some_and(|(node, port)| node == target_node && port == target_port))?;
	let (source_node, _) = split_port(&edge.from)?;
	file.graph
		.nodes
		.iter()
		.find(|node| node.id == source_node && node.feature.starts_with("flowgraph.literal."))?
		.properties
		.get("value")
}

fn split_port(value: &str) -> Option<(&str, &str)> {
	value.rsplit_once(':')
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::{SystemTime, UNIX_EPOCH};

	fn temp_root(name: &str) -> PathBuf {
		let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
		let root = std::env::temp_dir().join(format!("unvac-stream-contract-{name}-{}-{nonce}", std::process::id()));
		std::fs::create_dir_all(&root).unwrap();
		root
	}

	#[test]
	fn non_ai_clip_automation_does_not_require_cohost_contract() {
		let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("flowgraph.example/twitch-random-clip");
		let audit = audit_streaming_contract(&root).unwrap();
		assert_eq!(audit.status, ContractStatus::Inactive);
		assert!(audit.checks.is_empty());
	}

	#[test]
	fn canonical_streaming_graph_is_ready() {
		let root = temp_root("ready");
		std::fs::write(
			root.join("main.flowgraph.toml"),
			include_str!("../tests/fixtures/streaming-contract-ready.flowgraph.toml"),
		)
		.unwrap();
		let audit = audit_streaming_contract(&root).unwrap();
		assert_eq!(audit.status, ContractStatus::Ready);
		assert!(audit.checks.iter().all(|check| check.status == ContractStatus::Ready));
		assert_eq!(
			chat_output_routes(&root).unwrap(),
			[ChatOutputRoute {
				subscription_node_id: "main::twitch".into(),
				tts_node_id: "main::chat_tts".into(),
			}]
		);
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn migrated_chat_synth_destination_is_ready() {
		let root = temp_root("chat-synth");
		let source = include_str!("../tests/fixtures/streaming-contract-ready.flowgraph.toml")
			.replace("properties.value = \"chat\"", "properties.value = \"chat-synth\"");
		std::fs::write(root.join("main.flowgraph.toml"), source).unwrap();
		let audit = audit_streaming_contract(&root).unwrap();
		assert_eq!(audit.status, ContractStatus::Ready);
		assert_eq!(chat_output_routes(&root).unwrap().len(), 1);
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn full_ai_text_connected_to_tts_is_blocked() {
		let root = temp_root("double-tts");
		std::fs::write(
			root.join("main.flowgraph.toml"),
			r#"
[[nodes]]
id="ai"
feature="flowgraph.ingress.ai_response"
[[nodes]]
id="speak"
feature="flowgraph.tts.speak"
[[edges]]
from="ai:exec_out"
to="speak:exec_in"
"#,
		)
		.unwrap();
		let audit = audit_streaming_contract(&root).unwrap();
		assert_eq!(audit.status, ContractStatus::Blocked);
		let subtitle = audit.checks.iter().find(|check| check.id == "assistant_subtitle").unwrap();
		assert!(subtitle.detail.contains("TTS直結=1"));
		std::fs::remove_dir_all(root).unwrap();
	}
}
