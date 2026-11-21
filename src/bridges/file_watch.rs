//! `flowgraph.ingress.file_watch` 用 bridge。
//!
//! first slice は OS watcher crate に依存せず、定期 polling で file / directory の
//! create / modify / delete を検出して TriggerEvent を投入する。

use crate::flowgraph::loader::LoadedNodeMeta;
use crate::flowgraph::node::{TriggerEvent, TriggerHandle};
use crate::flowgraph::socket::SocketValue;
use crate::shutdown::ShutdownBroker;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub struct FlowgraphFileWatchIngress {
	pub node_id: String,
	pub path: PathBuf,
	pub recursive: bool,
	pub poll_ms: u64,
	pub fixed_channel: String,
}

impl FlowgraphFileWatchIngress {
	pub fn from_meta(fq: &str, meta: &LoadedNodeMeta) -> Option<Self> {
		let p = &meta.properties;
		let get_str = |k: &str| -> String {
			p.get(k)
				.and_then(|v| v.as_str().ok())
				.map(str::trim)
				.map(str::to_string)
				.unwrap_or_default()
		};
		let get_bool = |k: &str, default: bool| -> bool { p.get(k).and_then(|v| v.as_bool().ok()).unwrap_or(default) };
		let get_int = |k: &str, default: i64| -> i64 { p.get(k).and_then(|v| v.as_i64().ok()).unwrap_or(default) };
		let path = get_str("path");
		if path.is_empty() {
			log::warn!(
				"《Flowgraph/FileWatch》 ingress node_id={} は property `path` が空のためブリッジを開始しません。",
				fq
			);
			return None;
		}
		Some(Self {
			node_id: fq.to_string(),
			path: PathBuf::from(path),
			recursive: get_bool("recursive", false),
			poll_ms: get_int("poll_ms", 500).max(100) as u64,
			fixed_channel: get_str("fixed_channel"),
		})
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileStamp {
	len: u64,
	modified_ms: Option<u128>,
}

type Snapshot = BTreeMap<PathBuf, FileStamp>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileWatchEvent {
	path: PathBuf,
	kind: &'static str,
	stamp: Option<FileStamp>,
}

fn spawn_one(entry: FlowgraphFileWatchIngress, trigger: TriggerHandle, shutdown: Arc<ShutdownBroker>) -> JoinHandle<()> {
	let node_id = entry.node_id.clone();
	let path = entry.path.clone();
	let recursive = entry.recursive;
	let poll_ms = entry.poll_ms;
	let kind = if entry.fixed_channel.trim().is_empty() {
		"file_watch".to_string()
	} else {
		entry.fixed_channel.trim().to_string()
	};
	tokio::spawn(async move {
		log::info!(
			"《Flowgraph/FileWatch》 ingress 起動 node_id={} path={} recursive={} poll_ms={}",
			node_id,
			path.display(),
			recursive,
			poll_ms
		);
		let mut snapshot = match scan_path_blocking(path.clone(), recursive).await {
			Ok(snapshot) => snapshot,
			Err(e) => {
				log::warn!(
					"《Flowgraph/FileWatch》 初期 scan 失敗 node_id={} path={}: {}",
					node_id,
					path.display(),
					e
				);
				Snapshot::new()
			}
		};
		let mut interval = tokio::time::interval(Duration::from_millis(poll_ms));
		loop {
			tokio::select! {
				biased;
				_ = shutdown.wait() => {
					log::info!("《Flowgraph/FileWatch》 ingress 終了 node_id={} path={}", node_id, path.display());
					break;
				}
				_ = interval.tick() => {
					let next = match scan_path_blocking(path.clone(), recursive).await {
						Ok(snapshot) => snapshot,
						Err(e) => {
							log::warn!("《Flowgraph/FileWatch》 scan 失敗 node_id={} path={}: {}", node_id, path.display(), e);
							Snapshot::new()
						}
					};
					for event in diff_snapshots(&snapshot, &next) {
						let path_text = event.path.display().to_string();
						let meta = json!({
							"path": path_text,
							"event_kind": event.kind,
							"root": path.display().to_string(),
							"recursive": recursive,
							"len": event.stamp.as_ref().map(|s| s.len),
							"modified_ms": event.stamp.as_ref().and_then(|s| s.modified_ms),
						});
						let ev = TriggerEvent::new(&node_id)
							.with_exec("__trigger__")
							.with_override("__path__", SocketValue::String(path_text))
							.with_override("__event_kind__", SocketValue::String(event.kind.to_string()))
							.with_override("__source_actor__", SocketValue::String(path.display().to_string()))
							.with_override("__source_kind__", SocketValue::String(kind.clone()))
							.with_override("__meta__", SocketValue::Json(meta));
						if let Err(e) = trigger.send(ev) {
							log::warn!("《Flowgraph/FileWatch》 TriggerEvent 送信失敗 node_id={}: {}", node_id, e);
						}
					}
					snapshot = next;
				}
			}
		}
	})
}

pub fn spawn(entries: &[FlowgraphFileWatchIngress], trigger: Option<TriggerHandle>, shutdown: Arc<ShutdownBroker>) -> Vec<JoinHandle<()>> {
	if entries.is_empty() {
		return Vec::new();
	}
	let Some(trigger) = trigger else {
		log::warn!(
			"《Flowgraph/FileWatch》 ingress ノード {} 件を検出しましたが、Flowgraph ランタイムが無いため file_watch ブリッジを開始できません。",
			entries.len()
		);
		return Vec::new();
	};
	entries
		.iter()
		.cloned()
		.map(|entry| spawn_one(entry, trigger.clone(), shutdown.clone()))
		.collect()
}

async fn scan_path_blocking(path: PathBuf, recursive: bool) -> Result<Snapshot, String> {
	tokio::task::spawn_blocking(move || scan_path(&path, recursive))
		.await
		.map_err(|e| format!("scan task failed: {e}"))?
}

fn scan_path(path: &Path, recursive: bool) -> Result<Snapshot, String> {
	let mut out = Snapshot::new();
	if path.is_file() {
		insert_file_stamp(&mut out, path)?;
		return Ok(out);
	}
	if path.is_dir() {
		scan_dir(path, recursive, &mut out)?;
		return Ok(out);
	}
	Ok(out)
}

fn scan_dir(path: &Path, recursive: bool, out: &mut Snapshot) -> Result<(), String> {
	let entries = std::fs::read_dir(path).map_err(|e| format!("read_dir {}: {e}", path.display()))?;
	for entry in entries {
		let entry = entry.map_err(|e| format!("read_dir entry {}: {e}", path.display()))?;
		let child = entry.path();
		if child.is_file() {
			insert_file_stamp(out, &child)?;
		} else if recursive && child.is_dir() {
			scan_dir(&child, recursive, out)?;
		}
	}
	Ok(())
}

fn insert_file_stamp(out: &mut Snapshot, path: &Path) -> Result<(), String> {
	let meta = std::fs::metadata(path).map_err(|e| format!("metadata {}: {e}", path.display()))?;
	let modified_ms = meta
		.modified()
		.ok()
		.and_then(|mtime| mtime.duration_since(SystemTime::UNIX_EPOCH).ok())
		.map(|duration| duration.as_millis());
	out.insert(
		path.to_path_buf(),
		FileStamp {
			len: meta.len(),
			modified_ms,
		},
	);
	Ok(())
}

fn diff_snapshots(old: &Snapshot, new: &Snapshot) -> Vec<FileWatchEvent> {
	let keys = old.keys().chain(new.keys()).cloned().collect::<BTreeSet<_>>();
	let mut events = Vec::new();
	for path in keys {
		match (old.get(&path), new.get(&path)) {
			(None, Some(stamp)) => events.push(FileWatchEvent {
				path,
				kind: "created",
				stamp: Some(stamp.clone()),
			}),
			(Some(_), None) => events.push(FileWatchEvent {
				path,
				kind: "deleted",
				stamp: None,
			}),
			(Some(a), Some(b)) if a != b => events.push(FileWatchEvent {
				path,
				kind: "modified",
				stamp: Some(b.clone()),
			}),
			_ => {}
		}
	}
	events
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::loader::LoadedNodeMeta;
	use crate::flowgraph::node::InputMap;
	use std::path::PathBuf;

	fn meta(props: Vec<(&str, SocketValue)>) -> LoadedNodeMeta {
		let mut m = InputMap::new();
		for (k, v) in props {
			m.insert(k.to_string(), v);
		}
		LoadedNodeMeta {
			feature: "flowgraph.ingress.file_watch".into(),
			file: PathBuf::from("t.flowgraph.toml"),
			position: None,
			activation_chain: Vec::new(),
			properties: m,
		}
	}

	#[test]
	fn from_meta_requires_non_empty_path_and_clamps_poll_ms() {
		assert!(FlowgraphFileWatchIngress::from_meta("n1", &meta(vec![])).is_none());
		let entry = FlowgraphFileWatchIngress::from_meta(
			"n1",
			&meta(vec![
				("path", SocketValue::String("profiles".into())),
				("recursive", SocketValue::Bool(true)),
				("poll_ms", SocketValue::Int(1)),
				("fixed_channel", SocketValue::String("profile_watch".into())),
			]),
		)
		.expect("entry");
		assert_eq!(entry.node_id, "n1");
		assert_eq!(entry.path, PathBuf::from("profiles"));
		assert!(entry.recursive);
		assert_eq!(entry.poll_ms, 100);
		assert_eq!(entry.fixed_channel, "profile_watch");
	}

	#[test]
	fn diff_snapshots_reports_create_modify_delete() {
		let a = PathBuf::from("a.txt");
		let b = PathBuf::from("b.txt");
		let c = PathBuf::from("c.txt");
		let mut old = Snapshot::new();
		old.insert(
			a.clone(),
			FileStamp {
				len: 1,
				modified_ms: Some(1),
			},
		);
		old.insert(
			b.clone(),
			FileStamp {
				len: 1,
				modified_ms: Some(1),
			},
		);
		let mut new = Snapshot::new();
		new.insert(
			a.clone(),
			FileStamp {
				len: 2,
				modified_ms: Some(2),
			},
		);
		new.insert(
			c.clone(),
			FileStamp {
				len: 1,
				modified_ms: Some(1),
			},
		);

		let events = diff_snapshots(&old, &new);
		assert_eq!(events.len(), 3);
		assert!(events.iter().any(|e| e.path == a && e.kind == "modified"));
		assert!(events.iter().any(|e| e.path == b && e.kind == "deleted"));
		assert!(events.iter().any(|e| e.path == c && e.kind == "created"));
	}
}
