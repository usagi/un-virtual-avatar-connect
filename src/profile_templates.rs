use anyhow::{anyhow, Context, Result};
use toml_edit::DocumentMut;

use crate::profile_store::ProfileSeedFile;

pub struct ProfileTemplate {
	pub id: &'static str,
	pub label: &'static str,
	pub description: &'static str,
	pub recommended: bool,
}

pub struct MaterializedTemplate {
	pub conf: String,
	pub files: Vec<ProfileSeedFile<'static>>,
}

pub const TEMPLATES: &[ProfileTemplate] = &[
	ProfileTemplate {
		id: "streaming-tools",
		label: "Twitch配信支援（AIなし）",
		description: "チャネポなどの通知とクリップ紹介。Twitch認証とOBS出力は作成後に接続",
		recommended: false,
	},
	ProfileTemplate {
		id: "streaming-companion",
		label: "配信AIと始める",
		description: "自然なAI会話、字幕、音声、アバター反応を準備する構成",
		recommended: true,
	},
	ProfileTemplate {
		id: "desktop-assistant",
		label: "デスクトップAI",
		description: "会話と音声出力の経路を準備する構成",
		recommended: false,
	},
	ProfileTemplate {
		id: "automation",
		label: "データ連携・自動化",
		description: "入力、状態、外部処理をFlowgraphで組み立てる構成",
		recommended: false,
	},
	ProfileTemplate {
		id: "blank",
		label: "空から作る",
		description: "構成を追加せず新しいworkspaceだけを作成",
		recommended: false,
	},
];

pub fn materialize(id: &str, default_conf: &str) -> Result<MaterializedTemplate> {
	if !TEMPLATES.iter().any(|template| template.id == id) {
		return Err(anyhow!("unknown profile template: {id}"));
	}
	let mut document = default_conf
		.parse::<DocumentMut>()
		.context("profile templateのdefault confを解釈できません")?;
	let mut files = match id {
		"streaming-tools" => {
			document["flowgraph_dir"] = toml_edit::value("flowgraph");
			vec![
				ProfileSeedFile {
					path: "flowgraph/features/twitch-notices/main.flowgraph.toml",
					contents: include_bytes!("../flowgraph.example/twitch-event-notices/main.flowgraph.toml"),
				},
				ProfileSeedFile {
					path: "flowgraph/features/random-clip/main.flowgraph.toml",
					contents: include_bytes!("../flowgraph.example/twitch-random-clip/main.flowgraph.toml"),
				},
			]
		}
		"streaming-companion" => {
			document["flowgraph_dir"] = toml_edit::value("flowgraph");
			vec![
				ProfileSeedFile {
					path: "flowgraph/assistant-output.flowgraph.toml",
					contents: include_bytes!("../profile.template/streaming-companion/assistant-output.flowgraph.toml"),
				},
				ProfileSeedFile {
					path: "flowgraph/avatar-reaction.flowgraph.toml",
					contents: include_bytes!("../profile.template/streaming-companion/avatar-reaction.flowgraph.toml"),
				},
			]
		}
		"desktop-assistant" => {
			document["flowgraph_dir"] = toml_edit::value("flowgraph");
			vec![ProfileSeedFile {
				path: "flowgraph/assistant-output.flowgraph.toml",
				contents: include_bytes!("../profile.template/streaming-companion/assistant-output.flowgraph.toml"),
			}]
		}
		"automation" => {
			document["flowgraph_dir"] = toml_edit::value("flowgraph");
			vec![ProfileSeedFile {
				path: "flowgraph/main.flowgraph.toml",
				contents: include_bytes!("../gui/src/lib/flowgraph/templates/counter.flowgraph.toml"),
			}]
		}
		"blank" => Vec::new(),
		_ => unreachable!(),
	};
	// Seed names only during creation; existing profiles keep their own catalogue.
	let purposes: Option<&'static [u8]> = match id {
		"streaming-tools" => Some(include_bytes!("../profile.template/streaming-tools/purposes.json")),
		"streaming-companion" => Some(include_bytes!("../profile.template/streaming-companion/purposes.json")),
		"desktop-assistant" => Some(include_bytes!("../profile.template/desktop-assistant/purposes.json")),
		"automation" => Some(include_bytes!("../profile.template/automation/purposes.json")),
		_ => None,
	};
	if let Some(contents) = purposes {
		files.push(ProfileSeedFile { path: "flowgraph/.flowgraph-purposes.json", contents });
	}
	Ok(MaterializedTemplate {
		conf: document.to_string(),
		files,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	#[ignore = "explicit isolated UI fixture generation; writes only the named output file"]
	fn export_isolated_preview_templates() {
		let output = std::env::var_os("UNVAC_PREVIEW_TEMPLATE_OUTPUT").expect("explicit fixture output required");
		let entries: Vec<_> = TEMPLATES.iter().map(|template| {
			// No installed/user configuration enters the preview fixture.
			let materialized = materialize(template.id, "workers = 1\n").unwrap();
			let files: Vec<_> = materialized.files.iter().map(|file| serde_json::json!({
				"path": file.path, "content": std::str::from_utf8(file.contents).unwrap()
			})).collect();
			serde_json::json!({"id":template.id,"label":template.label,"description":template.description,
				"recommended":template.recommended,"conf":materialized.conf,"files":files})
		}).collect();
		let fixture = serde_json::json!({"schema":1,"base":"synthetic workers=1; not installed desktop defaults",
			"generator":"profile_templates::materialize", "templates":entries});
		std::fs::write(output, serde_json::to_vec_pretty(&fixture).unwrap()).unwrap();
	}

	#[test]
	fn every_template_materializes_as_valid_conf() {
		for template in TEMPLATES {
			let materialized = materialize(template.id, "workers = 1\n").unwrap();
			toml::from_str::<crate::Conf>(&materialized.conf).unwrap();
			let root = std::env::temp_dir().join(format!("unvac-profile-template-{}-{}", template.id, std::process::id()));
			let _ = std::fs::remove_dir_all(&root);
			let has_files = !materialized.files.is_empty();
			for file in materialized.files {
				assert!(file.path.starts_with("flowgraph/"));
				assert!(!file.contents.is_empty());
				let relative = file.path.strip_prefix("flowgraph/").unwrap();
				let path = root.join(relative);
				std::fs::create_dir_all(path.parent().unwrap()).unwrap();
				std::fs::write(path, file.contents).unwrap();
			}
			if has_files {
				crate::flowgraph::load_flowgraph_dir(&root).unwrap_or_else(|error| panic!("{} template: {error}", template.id));
				let catalogue = crate::web_interface::control::flowgraph::purposes::read(&root).unwrap();
				assert!(!catalogue.document.purposes.is_empty(), "{} has no named purposes", template.id);
				let mut covered = std::collections::BTreeSet::new();
				for purpose in &catalogue.document.purposes {
					for fq in &purpose.roots {
						assert!(root.join(format!("{fq}.flowgraph.toml")).is_file(), "{}: missing {fq}", template.id);
						covered.insert(fq.clone());
					}
				}
				for path in crate::flowgraph::loader::walk_flowgraph_dir(&root).unwrap() {
					let fq = crate::flowgraph::loader::fq_path_of_file(&root, &path).unwrap();
					assert!(covered.contains(&fq), "{}: unnamed {fq}", template.id);
				}
				std::fs::remove_dir_all(root).unwrap();
			}
		}
	}

	#[test]
	fn automation_uses_the_same_observable_starter_as_feature_library() {
		let template = materialize("automation", "workers = 1\n").unwrap();
		let graph = template.files.iter().find(|file| file.path == "flowgraph/main.flowgraph.toml").unwrap();
		assert_eq!(graph.contents, include_bytes!("../gui/src/lib/flowgraph/templates/counter.flowgraph.toml"));
		let value: toml::Value = toml::from_str(std::str::from_utf8(graph.contents).unwrap()).unwrap();
		let nodes = value["nodes"].as_array().unwrap();
		assert!(nodes.iter().any(|node| node["feature"].as_str() == Some("flowgraph.util.log")));
		let edges = value["edges"].as_array().unwrap();
		assert!(edges.iter().any(|edge| edge["from"].as_str() == Some("counter:changed") && edge["to"].as_str() == Some("count_log:exec_in")));
	}

	#[test]
	fn assistant_output_separates_full_subtitles_from_segmented_speech() {
		let source = include_str!("../profile.template/streaming-companion/assistant-output.flowgraph.toml");
		let root = toml::from_str::<toml::Value>(source).unwrap();
		let nodes = root["nodes"].as_array().unwrap();
		let feature_count = |feature: &str| nodes.iter().filter(|node| node["feature"].as_str() == Some(feature)).count();
		assert_eq!(feature_count("flowgraph.ingress.ai_response"), 1);
		assert_eq!(feature_count("flowgraph.ingress.ai_speech"), 1);
		assert_eq!(feature_count("flowgraph.bos.output"), 2);
		assert_eq!(feature_count("flowgraph.tts.speak"), 1);

		let edges = root["edges"].as_array().unwrap();
		let has_edge = |from: &str, to: &str| {
			edges
				.iter()
				.any(|edge| edge["from"].as_str() == Some(from) && edge["to"].as_str() == Some(to))
		};
		assert!(has_edge("subtitle_in:exec_out", "subtitle_emit:exec_in"));
		assert!(has_edge("speech_in:exec_out", "live_subtitle_emit:exec_in"));
		assert!(has_edge("speech_in:utterance_content", "live_subtitle_emit:content"));
		assert!(!has_edge("subtitle_in:exec_out", "speak:exec_in"));
		assert!(has_edge("speech_in:exec_out", "speak:exec_in"));
		assert!(has_edge("tts_lane:value", "speak:lane"));
		assert!(has_edge("tts_non_blocking:value", "speak:non_blocking"));

		let literal_value = |id: &str| {
			nodes
				.iter()
				.find(|node| node["id"].as_str() == Some(id))
				.map(|node| &node["properties"]["value"])
				.unwrap()
		};
		assert_eq!(literal_value("subtitle_channel").as_str(), Some("ai-synth"));
		assert_eq!(literal_value("tts_lane").as_str(), Some("assistant"));
		assert_eq!(literal_value("tts_non_blocking").as_bool(), Some(true));
	}
}
