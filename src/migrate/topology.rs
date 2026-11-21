//! Reconnect supported legacy text channels using explicit cross-file edges.
use super::MigrationReport;
use std::{collections::BTreeMap, path::PathBuf};
use toml_edit::{DocumentMut, Table};

pub(super) fn output(feature: &str) -> Option<(&'static str, &'static str)> {
    match feature {
        "voice" | "twitch" | "web-input" | "webinput" => Some(("in:exec_out", "in:content")),
        "modify" => Some(("load:on_success", "dict:result")),
        "dictionary-command" => Some(("feedback:exec_out", "feedback:value")),
        "gas-translation" => Some(("gas:on_success", "gas:translated")),
        "libre-translation" => Some(("libre:on_success", "libre:translated")),
        "screenshot" => Some(("cap:on_success", "cap:data_url")),
        "ocr" => Some(("ocr:on_success", "ocr:text")),
        _ => None,
    }
}

fn consumes(feature: &str) -> bool {
    matches!(feature, "modify" | "gas-translation" | "libre-translation" | "screenshot" | "ocr"
        | "os-tts" | "OS-TTS" | "voicevox" | "aivis-speech" | "aivisspeech" | "coeiroink"
        | "bouyomichan" | "BouyomiChan" | "twitch-out")
}

pub(super) fn connect(
    processors: &[Table],
    files: &BTreeMap<usize, (String, PathBuf)>,
    report: &mut MigrationReport,
) -> std::io::Result<()> {
    for (&index, (_, path)) in files {
        let processor = &processors[index];
        let feature = processor.get("feature").and_then(|v| v.as_str()).unwrap_or("");
        // The command adapter validates and binds both text and trusted identity.
        if feature == "dictionary-command" { continue; }
        let Some(channel) = processor.get("channel_from").and_then(|v| v.as_str()) else { continue };
        if !consumes(feature) {
            report.error(Some(feature.into()), format!("processors[{index}] channel_from={channel:?}: この処理の入出力接続は未対応です。"));
            continue;
        }
        // Include unsupported/unconverted producers in ambiguity detection; never
        // select an arbitrary surviving producer and silently drop another.
        let producers: Vec<_> = processors.iter().enumerate().filter(|(_, p)|
            p.get("channel_to").and_then(|v| v.as_str()) == Some(channel)).collect();
        if producers.is_empty() {
            report.error(Some(feature.into()), format!("processors[{index}] channel_from={channel:?}: 送信元が{}件。明示的な合流または入力元の移行が必要です。", producers.len()));
            continue;
        }
        let mut inputs = Vec::new();
        let mut actors = Vec::new();
        let mut unsupported = false;
        for (source_index, source) in producers {
        let source_feature = source.get("feature").and_then(|v| v.as_str()).unwrap_or("");
        let Some((fq, _)) = files.get(&source_index) else {
            report.error(Some(feature.into()), format!("processors[{index}] の送信元 processors[{source_index}] は未変換です。"));
            unsupported = true;
            continue;
        };
        let Some((exec, content)) = output(source_feature) else {
            report.error(Some(feature.into()), format!("processors[{index}] の送信元 {source_feature:?} の出力は未対応です。"));
            unsupported = true;
            continue;
        };
        inputs.push((format!("/{fq}::{exec}"), format!("/{fq}::{content}")));
        actors.push(super::dictionary_command::identity_source(source_index, processors, files));
        }
        if unsupported { continue; }
        let mut graph: DocumentMut = std::fs::read_to_string(path)?.parse()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let (mut exec, mut content) = inputs[0].clone();
        if inputs.len() > 1 && actors.iter().all(Result::is_ok)
            && matches!(feature, "modify" | "gas-translation" | "libre-translation") {
            let paired: Vec<_> = inputs.iter().zip(&actors).map(|((exec, text), actor)|
                (exec.clone(), text.clone(), actor.as_ref().unwrap().clone())).collect();
            let (merged_exec, merged_text, _) = super::dictionary_command::connect_inputs(&mut graph, &paired)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            exec = merged_exec;
            content = merged_text;
        } else {
        for (branch, (next_exec, next_content)) in inputs.iter().enumerate().skip(1) {
            let id = format!("migration_input_merge_{branch}");
            let mut node = Table::new();
            node["id"] = toml_edit::value(&id);
            node["feature"] = toml_edit::value("flowgraph.flow.merge_string");
            graph["nodes"].as_array_of_tables_mut().unwrap().push(node);
            for (from, port) in [(&exec, "exec_a"), (&content, "value_a"), (next_exec, "exec_b"), (next_content, "value_b")] {
                let mut edge = Table::new();
                edge["from"] = toml_edit::value(from.as_str());
                edge["to"] = toml_edit::value(format!("{id}:{port}"));
                graph["edges"].as_array_of_tables_mut().unwrap().push(edge);
            }
            exec = format!("{id}:exec_out");
            content = format!("{id}:value");
        }
        }
        if let Some(nodes) = graph.get_mut("nodes").and_then(|v| v.as_array_of_tables_mut()) {
            nodes.retain(|node| node.get("id").and_then(|v| v.as_str()) != Some("in"));
        }
        if let Some(edges) = graph.get_mut("edges").and_then(|v| v.as_array_of_tables_mut()) {
            for edge in edges.iter_mut() {
                let replacement = match edge.get("from").and_then(|v| v.as_str()) {
                    Some("in:exec_out") => Some(&exec),
                    Some("in:content") => Some(&content),
                    _ => None,
                };
                if let Some(port) = replacement {
                    edge["from"] = toml_edit::value(port.as_str());
                }
            }
        }
        std::fs::write(path, graph.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{migrate_conf_file, migrate_conf_str};

    #[tokio::test]
    async fn merged_inputs_each_reach_shared_transform() {
        let root = std::env::temp_dir().join(format!("vac-migrate-merge-exec-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("dict.tsv"), "一 ONE\n二 TWO\n三 THREE\n").unwrap();
        std::fs::write(root.join("conf.toml"), r#"
[[processors]]
feature="web-input"
channel_to="raw"
[[processors]]
feature="web-input"
channel_to="raw"
[[processors]]
feature="web-input"
channel_to="raw"
[[processors]]
feature="modify"
channel_from="raw"
dictionary_files=["dict.tsv"]
"#).unwrap();
        let out = root.join("out");
        let report = migrate_conf_file(&root.join("conf.toml"), &out, false).unwrap();
        assert!(!report.has_errors(), "{:?}", report.entries);
        let mut fixture = String::new();
        for (source, text) in [("web-input", "ONE"), ("web-input-1", "TWO"), ("web-input-2", "THREE"), ("web-input", "ONE")] {
            fixture.push_str(&format!(r#"
[[triggers]]
node="{source}/main::in"
exec=["__trigger__"]
[[triggers.overrides]]
port="__content__"
ty="string"
value="{text}"
"#));
        }
        fixture.push_str(r#"
[[tests]]
name="All input events execute shared transform"
[tests.expect]
trigger_count=4
[[tests.expect.stored_values]]
node="chat-filters/main::dict"
port="result"
value="一"
"#);
        std::fs::write(out.join("flowgraph/merge.flowgraph.test.toml"), fixture).unwrap();
        let result = crate::flowgraph::fixture_runner::run_fixture_once_report(&out.join("flowgraph")).await.unwrap();
        assert!(result.ok, "{:?}", result.tests);
        assert_eq!(result.exec_count.iter().find(|(id, _)| id == "chat-filters/main::load").unwrap().1, 4);
        let transformed: Vec<_> = result.trace.iter().filter(|line| line.starts_with("log: ")).collect();
        // Generated ingress and modify graphs already log raw and transformed
        // content respectively; assert all events rather than the final cache.
        assert_eq!(transformed, vec!["log: ONE", "log: 一", "log: TWO", "log: 二",
            "log: THREE", "log: 三", "log: ONE", "log: 一"]);
        assert_eq!(std::fs::read_to_string(root.join("dict.tsv")).unwrap(), "一 ONE\n二 TWO\n三 THREE\n");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn web_input_reaches_two_dictionary_stages_through_explicit_edges() {
        let root = std::env::temp_dir().join(format!("vac-migrate-chain-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("one.tsv"), "うさぎ USAGI\n").unwrap();
        std::fs::write(root.join("two.tsv"), "rabbit うさぎ\n").unwrap();
        std::fs::write(root.join("conf.toml"), r#"
[[processors]]
feature="web-input"
channel_to="raw"
[[processors]]
feature="modify"
channel_from="raw"
channel_to="readings"
dictionary_files=["one.tsv"]
[[processors]]
feature="modify"
channel_from="readings"
dictionary_files=["two.tsv"]
"#).unwrap();
        let out = root.join("out");
        let report = migrate_conf_file(&root.join("conf.toml"), &out, false).unwrap();
        assert!(!report.has_errors(), "{:?}", report.entries);
        for name in ["chat-filters", "chat-filters-1"] {
            let graph = std::fs::read_to_string(out.join(format!("flowgraph/{name}/main.flowgraph.toml"))).unwrap();
            assert!(!graph.contains("flowgraph.ingress.web_input"), "{graph}");
        }
        std::fs::write(out.join("flowgraph/chain.flowgraph.test.toml"), r#"
[[triggers]]
node="web-input/main::in"
exec=["__trigger__"]
[[triggers.overrides]]
port="__content__"
ty="string"
value="USAGI"
[[tests]]
name="Original channel chain executes both stages"
[[tests.expect.stored_values]]
node="chat-filters/main::dict"
port="result"
value="うさぎ"
[[tests.expect.stored_values]]
node="chat-filters-1/main::dict"
port="result"
value="rabbit"
"#).unwrap();
        let result = crate::flowgraph::fixture_runner::run_fixture_once_report(&out.join("flowgraph")).await.unwrap();
        assert!(result.ok, "{:?}", result.tests);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsupported_producer_is_not_silently_dropped() {
        let out = std::env::temp_dir().join(format!("vac-migrate-merge-{}", rand::random::<u64>()));
        let report = migrate_conf_str(r#"
[[processors]]
feature="web-input"
channel_to="chat"
[[processors]]
feature="unsupported-input"
channel_to="chat"
[[processors]]
feature="modify"
channel_from="chat"
"#, &out, false).unwrap();
        assert!(report.has_errors());
        assert!(report.entries.iter().any(|e| e.message.contains("未変換")));
        std::fs::remove_dir_all(out).unwrap();
    }
}
