//! Bind the verified composition to the exact migrated owner and ingress.
use std::{collections::BTreeMap, path::PathBuf};
use anyhow::{bail, Context, Result};
use toml_edit::{DocumentMut, Table, value};
use super::MigrationReport;

pub(super) fn finish(processors: &[Table], files: &BTreeMap<usize, (String, PathBuf)>, report: &mut MigrationReport) {
    for (&index, (_, path)) in files {
        let processor = &processors[index];
        if processor.get("feature").and_then(|v| v.as_str()) != Some("dictionary-command") { continue; }
        let has_flags = processor.get("allowed_flags").and_then(|v| v.as_array()).is_some_and(|v| !v.is_empty());
        if has_flags {
            let draft = path.with_file_name("main.control-input.toml");
            match bind(processor, processors, files, &draft, report, true) {
                Ok(()) => {
                    let (fq, _) = &files[&index];
                    let guide = path.with_file_name("control-input-migration.json");
                    let plan = serde_json::json!({
                        "status":"client_update_required_not_active",
                        "old_channel":processor.get("channel_from").and_then(|v|v.as_str()),
                        "draft":"main.control-input.toml", "install_as":"main.flowgraph.toml",
                        "target_node":format!("{fq}::in"),
                        "endpoint_template":"POST /api/v1/control/flowgraph/default/trigger/{url_encoded_target_node}",
                        "request_example":{"exec_port":"exec_in","inputs":{"content":"learn(X=Y)","source_actor":"","flags":[]}},
                        "requirements":["Update the trusted caller to address this node under the existing Control API authentication policy.","Send parsed command content without the old display-name prefix; send the actual sender and authorized context flags separately.","Review this draft and replace main.flowgraph.toml only after updating the caller. The draft extension is deliberately not loaded.","202 Accepted means queued, not successful authorization or persistence. Check the graph result."],
                        "automatic_activation":false
                    });
                    if let Err(error) = std::fs::write(&guide, serde_json::to_vec_pretty(&plan).unwrap()) {
                        report.error(Some("dictionary-command".into()), format!("client migration guide failed: {error}"));
                    } else { report.written_files.push(guide); report.written_files.push(draft); }
                    report.error(Some("dictionary-command".into()), format!("processors[{index}] requires a trusted flag adapter: prepared main.control-input.toml and control-input-migration.json; client update and explicit installation required"));
                },
                Err(error) => report.error(Some("dictionary-command".into()), format!("processors[{index}] の管理入力案作成失敗: {error:#}")),
            }
        } else { match bind(processor, processors, files, path, report, false) { Err(error) => {
            report.error(Some("dictionary-command".into()), format!("processors[{index}] の接続失敗: {error:#}"));
        } _ => {}}}
    }
}

fn list(processor: &Table, name: &str) -> Result<toml_edit::Array> {
    let mut result = toml_edit::Array::new();
    if let Some(item) = processor.get(name) {
        for entry in item.as_array().with_context(|| format!("{name} must be an array"))? {
            result.push(entry.as_str().with_context(|| format!("{name} must contain strings"))?);
        }
    }
    Ok(result)
}

pub(super) fn identity_source(index: usize, processors: &[Table], files: &BTreeMap<usize, (String, PathBuf)>) -> Result<String> {
    fn resolve(index: usize, processors: &[Table], files: &BTreeMap<usize, (String, PathBuf)>, visited: &mut std::collections::HashSet<usize>) -> Result<String> {
        if !visited.insert(index) { bail!("sender identity chain contains a cycle"); }
        let p = &processors[index];
        let (fq, _) = files.get(&index).context("identity producer was not migrated")?;
        let feature = p.get("feature").and_then(|v| v.as_str()).unwrap_or("");
        let result = match feature {
            "twitch" | "web-input" | "webinput" => format!("/{fq}::in:source_actor"),
            "modify" | "gas-translation" | "libre-translation" => {
                let channel = p.get("channel_from").and_then(|v| v.as_str()).context("identity transform has no channel_from")?;
                let sources: Vec<_> = processors.iter().enumerate().filter(|(_, p)| p.get("channel_to").and_then(|v| v.as_str()) == Some(channel)).collect();
                if sources.is_empty() { bail!("identity channel {channel:?} has no producer"); }
                let mut actors = Vec::new();
                for (source, _) in sources { actors.push(resolve(source, processors, files, visited)?); }
                if actors.len() == 1 { actors.remove(0) } else { format!("/{fq}::input_actor:string") }
            },
            _ => bail!("sender identity forwarding from {feature:?} is not implemented"),
        };
        visited.remove(&index);
        Ok(result)
    }
    resolve(index, processors, files, &mut std::collections::HashSet::new())
}

pub(super) fn connect_inputs(graph: &mut DocumentMut, inputs: &[(String, String, String)]) -> Result<(String, String, String)> {
    if inputs.len() == 1 { return Ok(inputs[0].clone()); }
    fn node(graph: &mut DocumentMut, id: &str, feature: &str, literal: Option<&str>) {
        let mut n = Table::new(); n["id"] = value(id); n["feature"] = value(feature);
        if let Some(text) = literal { let mut p = Table::new(); p["value"] = value(text); n["properties"] = toml_edit::Item::Table(p); }
        graph["nodes"].as_array_of_tables_mut().unwrap().push(n);
    }
    fn edge(graph: &mut DocumentMut, from: &str, to: &str) {
        let mut e = Table::new(); e["from"] = value(from); e["to"] = value(to);
        graph["edges"].as_array_of_tables_mut().unwrap().push(e);
    }
    node(graph, "input_content_key", "flowgraph.literal.string", Some("content"));
    node(graph, "input_actor_key", "flowgraph.literal.string", Some("source_actor"));
    let mut previous: Option<(String, String)> = None;
    for (i, (exec, content, actor)) in inputs.iter().enumerate() {
        let text_node = format!("input_{i}_text"); let actor_node = format!("input_{i}_actor");
        node(graph, &text_node, "flowgraph.json.set_string", None);
        node(graph, &actor_node, "flowgraph.json.set_string", None);
        edge(graph, "input_content_key:value", &format!("{text_node}:key"));
        edge(graph, content, &format!("{text_node}:value"));
        edge(graph, &format!("{text_node}:result"), &format!("{actor_node}:object"));
        edge(graph, "input_actor_key:value", &format!("{actor_node}:key"));
        edge(graph, actor, &format!("{actor_node}:value"));
        let packed = format!("{actor_node}:result");
        previous = Some(match previous {
            None => (exec.clone(), packed),
            Some((previous_exec, previous_value)) => {
                let merge = format!("input_merge_{i}");
                node(graph, &merge, "flowgraph.flow.merge_json", None);
                edge(graph, &previous_exec, &format!("{merge}:exec_a")); edge(graph, &previous_value, &format!("{merge}:value_a"));
                edge(graph, exec, &format!("{merge}:exec_b")); edge(graph, &packed, &format!("{merge}:value_b"));
                (format!("{merge}:exec_out"), format!("{merge}:value"))
            }
        });
    }
    let (exec, packed) = previous.context("no input to merge")?;
    for (id, key) in [("input_content", "input_content_key"), ("input_actor", "input_actor_key")] {
        node(graph, id, "flowgraph.json.get", None);
        edge(graph, &packed, &format!("{id}:value")); edge(graph, &format!("{key}:value"), &format!("{id}:path"));
    }
    Ok((exec, "input_content:string".into(), "input_actor:string".into()))
}

fn bind(processor: &Table, processors: &[Table], files: &BTreeMap<usize, (String, PathBuf)>,
    path: &std::path::Path, report: &mut MigrationReport, control_input: bool) -> Result<()> {
    let target = processor.get("target_modify_id").and_then(|v| v.as_str()).context("target_modify_id is required")?;
    let owners: Vec<_> = processors.iter().enumerate().filter(|(_, p)| p.get("id").and_then(|v| v.as_str()) == Some(target)).collect();
    if owners.len() != 1 { bail!("target_modify_id={target:?} must resolve to exactly one processor"); }
    let (owner_index, owner) = owners[0];
    if owner.get("feature").and_then(|v| v.as_str()) != Some("modify") { bail!("target is not a modify processor"); }
    let (_, owner_graph) = files.get(&owner_index).context("target dictionary was not migrated")?;
    let writable = owner.get("writable_dictionary_file").and_then(|v| v.as_str()).context("target has no writable_dictionary_file")?;
    let owner_dir = owner_graph.parent().context("owner directory missing")?;
    let mapping: serde_json::Value = serde_json::from_slice(&std::fs::read(owner_dir.join("dictionary-assets.json"))?)?;
    let asset = mapping.as_array().context("invalid dictionary asset mapping")?.iter().find(|entry|
        entry["kind"] == "dictionary_files" && entry["original"] == writable && entry["writable"] == true)
        .and_then(|entry| entry["asset"].as_str()).context("writable dictionary asset not found")?;
    let mut inputs = Vec::new();
    if !control_input {
    let channel = processor.get("channel_from").and_then(|v| v.as_str()).context("channel_from is required")?;
    let sources: Vec<_> = processors.iter().enumerate().filter(|(_, p)| p.get("channel_to").and_then(|v| v.as_str()) == Some(channel)).collect();
    if sources.is_empty() { bail!("channel_from={channel:?} has no producer"); }
    let strip = match processor.get("strip_chat_prefix") {
        Some(v) => v.as_bool().context("strip_chat_prefix must be boolean")?,
        None => true,
    };

    for (source_index, source) in sources {
        let source_feature = source.get("feature").and_then(|v| v.as_str()).unwrap_or("");
        let identity_fq = identity_source(source_index, processors, files)?;
        let (source_exec, source_content) = super::topology::output(source_feature).context("input producer has no supported text output")?;
        let (source_fq, _) = files.get(&source_index).context("input producer was not migrated")?;
        if strip && source_feature != "twitch" { bail!("prefix removal for non-Twitch input needs an explicit adapter"); }
        inputs.push((format!("/{source_fq}::{source_exec}"), format!("/{source_fq}::{}", if strip {"in:message_text"} else {source_content}), identity_fq));
    }
    }
    let flags = list(processor, "allowed_flags")?;
    if !control_input && !flags.is_empty() { bail!("legacy flags require an explicit trusted flag adapter; login-only substitution is not permitted"); }
    let output = processor.get("channel_to").map(|item| {
        item.as_str().filter(|v| !v.trim().is_empty()).context("channel_to must be a non-empty string")
    }).transpose()?;
    let logins = list(processor, "allowed_logins")?;
    let mut graph: DocumentMut = include_str!("../../flowgraph.example/dictionary-managed-command/main.flowgraph.toml").parse()?;
    let owner_topic = owner_dir.file_name().and_then(|v| v.to_str()).context("owner topic missing")?;
    let dictionary_path = format!("graph-file:../{owner_topic}/{asset}");
    let nodes = graph["nodes"].as_array_of_tables_mut().context("composition nodes missing")?;
    if !control_input { nodes.retain(|n| n.get("id").and_then(|v| v.as_str()) != Some("in")); }
    for node in nodes.iter_mut() {
        match node.get("id").and_then(|v| v.as_str()).unwrap_or("") {
            "in" if control_input => node["feature"] = value("flowgraph.ingress.control"),
            "dictionary_path" => node["properties"]["value"] = value(dictionary_path.clone()),
            "logins" => node["properties"]["value"] = value(logins.clone()),
            "flags" => node["properties"]["value"] = value(flags.clone()),
            // Legacy forget removed all matching rows; the new standalone sample remains one-entry undo.
            "undo_mode" | "exact_mode" => node["properties"]["value"] = value("all"),
            _ => {}
        }
    }
    let (input_exec, input_text, input_actor) = if control_input {
        let mut edge = Table::new(); edge["from"] = value("in:flags"); edge["to"] = value("gate:flags");
        graph["edges"].as_array_of_tables_mut().unwrap().push(edge);
        ("in:exec_out".into(), "in:content".into(), "in:source_actor".into())
    } else { connect_inputs(&mut graph, &inputs)? };
    for edge in graph["edges"].as_array_of_tables_mut().context("composition edges missing")?.iter_mut() {
        let replacement = match edge.get("from").and_then(|v| v.as_str()) {
            Some("in:exec_out") => Some(&input_exec),
            Some("in:source_actor") => Some(&input_actor),
            Some("in:content") => Some(&input_text),
            _ => None,
        };
        if let Some(port) = replacement { edge["from"] = value(port.as_str()); }
    }
    if let Some(output) = output {
        let mut publish = Table::new();
        publish["id"] = value("publish_feedback");
        publish["feature"] = value("flowgraph.bos.output");
        let mut properties = Table::new();
        properties["default_destination"] = value(output);
        publish["properties"] = toml_edit::Item::Table(properties);
        graph["nodes"].as_array_of_tables_mut().unwrap().push(publish);
        for (from, to) in [("feedback:exec_out", "publish_feedback:exec_in"), ("feedback:value", "publish_feedback:content")] {
            let mut edge = Table::new(); edge["from"] = value(from); edge["to"] = value(to);
            graph["edges"].as_array_of_tables_mut().unwrap().push(edge);
        }
    }
    let graph = super::preserve_activation(&graph.to_string(), processor).map_err(anyhow::Error::msg)?;
    let assets = path.parent().context("command directory missing")?.join("assets");
    std::fs::create_dir_all(&assets)?;
    let grammar = assets.join("commands.tsv");
    std::fs::write(&grammar, include_str!("../../flowgraph.example/dictionary-managed-command/assets/commands.tsv"))?;
    std::fs::write(path, graph)?;
    report.written_files.push(grammar);
    let destination = if control_input {
        "未適用の管理入力下書きに権限・対象辞書・入力を接続しました。呼出し側の更新と明示的な適用が必要です。"
    } else {
        "権限・対象辞書・入力を接続しました。"
    };
    report.warn(Some("dictionary-command".into()), format!("{destination}旧忘却は一致行の全削除として保持し、ロック行は保護します。新規サンプルの1件UNDOとは異なります。本文の名前を権限判定には使いません。"));
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn unsupported_identity_and_feedback_routes_are_not_silently_dropped() {
        for (extra, expected) in [
            ("allowed_flags=['moderator']", "trusted flag adapter"),
            ("channel_to=''", "non-empty string"),
            ("allowed_logins=[123]", "must contain strings"),
            ("strip_chat_prefix='yes'", "must be boolean"),
        ] {
            let root = std::env::temp_dir().join(format!("vac-command-reject-{}", rand::random::<u64>()));
            std::fs::create_dir(&root).unwrap();
            std::fs::write(root.join("dictionary.tsv"), "元\tORIGINAL\t名詞\t\n").unwrap();
            std::fs::write(root.join("conf.toml"), format!(r#"
[[processors]]
feature="twitch"
channel_to="chat"
[[processors]]
feature="modify"
id="owner"
dictionary_files=["dictionary.tsv"]
writable_dictionary_file="dictionary.tsv"
[[processors]]
feature="dictionary-command"
target_modify_id="owner"
channel_from="chat"
{extra}
"#)).unwrap();
            let out = root.join("out");
            let report = super::super::migrate_conf_file(&root.join("conf.toml"), &out, false).unwrap();
            assert!(report.has_errors());
            assert!(report.entries.iter().any(|entry| entry.message.contains(expected)), "{:?}", report.entries);
            assert_eq!(std::fs::read_to_string(root.join("dictionary.tsv")).unwrap(), "元\tORIGINAL\t名詞\t\n");
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[tokio::test]
    async fn migrated_command_uses_verified_twitch_text_and_only_the_selected_owner() {
        let root = std::env::temp_dir().join(format!("vac-bound-command-{}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("user.tsv"), "旧\tOLD\t名詞\t\n").unwrap();
        std::fs::write(root.join("peer.tsv"), "別\tPEER\t名詞\t\n").unwrap();
        std::fs::write(root.join("conf.toml"), r#"
[[processors]]
feature="twitch"
channel_to="chat"
[[processors]]
feature="dictionary-command"
target_modify_id="chosen"
channel_from="chat"
channel_to="reply"
allowed_logins=["alice"]
[[processors]]
id="peer"
feature="modify"
dictionary_files=["peer.tsv"]
writable_dictionary_file="peer.tsv"
[[processors]]
id="chosen"
feature="modify"
dictionary_files=["user.tsv"]
writable_dictionary_file="user.tsv"
[[processors]]
feature="modify"
channel_from="reply"
dictionary_files=["peer.tsv"]
"#).unwrap();
        let out = root.join("out");
        let report = super::super::migrate_conf_file(&root.join("conf.toml"), &out, false).unwrap();
        assert!(!report.has_errors(), "{:?}", report.entries);
        let peer = out.join("flowgraph/chat-filters/assets/dictionary-0.tsv");
        let peer_before = std::fs::read(&peer).unwrap();
        let chosen = out.join("flowgraph/chat-filters-1/assets/dictionary-0.tsv");
        for (actor, command, expected_change) in [("ALICE", "learn(X=正)", true), ("alice", "learn(X=正)", false), ("stranger", "learn(X=偽)", false)] {
            let before = std::fs::read(&chosen).unwrap();
            std::fs::write(out.join("flowgraph/test.flowgraph.test.toml"), format!(r#"
[[triggers]]
node="twitch-echo/main::in"
exec=["__trigger__"]
[[triggers.overrides]]
port="__source_actor__"
ty="string"
value="{actor}"
[[triggers.overrides]]
port="__content__"
ty="string"
value="alice:learn(X=本文偽装)"
[[triggers.overrides]]
port="__meta__"
ty="json"
value={{message_text="{command}"}}
[[tests]]
name="Migrated selected owner"
[tests.expect]
trigger_count=1
"#)).unwrap();
            let result = crate::flowgraph::fixture_runner::run_fixture_once_report(&out.join("flowgraph")).await.unwrap();
            assert!(result.ok, "{:?}", result.tests);
            assert_eq!(before != std::fs::read(&chosen).unwrap(), expected_change);
            let replies = result.exec_count.iter().find(|(id, _)| id == "chat-filters-2/main::load").map(|(_, count)| *count).unwrap_or(0);
            assert_eq!(replies, usize::from(actor != "stranger"));
            if actor != "stranger" {
                let feedback = result.stored_values.iter().find(|v| v.node.ends_with("::feedback") && v.port == "value").unwrap();
                let downstream = result.stored_values.iter().find(|v| v.node == "chat-filters-2/main::dict" && v.port == "result").unwrap();
                assert_eq!(feedback.value, downstream.value);
                assert!(feedback.value.as_str().unwrap().contains(if expected_change {"学習:"} else {"登録済み"}));
            }
        }
        let fixture_path = out.join("flowgraph/test.flowgraph.test.toml");
        let fixture = std::fs::read_to_string(&fixture_path).unwrap().replace("stranger", "alice").replace("learn(X=偽)", "learn(Y=保存失敗)");
        std::fs::write(&fixture_path, fixture).unwrap();
        let before_failure = std::fs::read(&chosen).unwrap();
        let lock = std::fs::OpenOptions::new().read(true).write(true).open(chosen.parent().unwrap().join(".vac-data-write.lock")).unwrap();
        lock.try_lock().unwrap();
        let result = crate::flowgraph::fixture_runner::run_fixture_once_report(&out.join("flowgraph")).await.unwrap();
        assert!(result.ok, "{:?}", result.tests);
        assert_eq!(std::fs::read(&chosen).unwrap(), before_failure);
        assert!(!result.exec_count.iter().any(|(id, count)| id == "chat-filters-2/main::load" && *count > 0));
        assert!(result.exec_count.iter().any(|(id, count)| id.ends_with("::learn_save_error") && *count == 1));
        drop(lock);
        let contents = std::fs::read_to_string(&chosen).unwrap();
        assert!(contents.contains("正\tX"));
        assert!(!contents.contains("偽"));
        assert_eq!(std::fs::read(peer).unwrap(), peer_before);
        assert_eq!(std::fs::read_to_string(root.join("user.tsv")).unwrap(), "旧\tOLD\t名詞\t\n");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn browser_only_feedback_reaches_real_channel_store_and_broadcast() {
        use std::sync::Arc;
        use crate::flowgraph::{node::TriggerEvent, socket::SocketValue};
        let root = std::env::temp_dir().join(format!("vac-bos-command-{}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("user.tsv"), "旧\tOLD\t名詞\t\n").unwrap();
        std::fs::write(root.join("conf.toml"), r#"
[[processors]]
feature="web-input"
channel_to="chat"
[[processors]]
feature="modify"
id="owner"
dictionary_files=["user.tsv"]
writable_dictionary_file="user.tsv"
[[processors]]
feature="dictionary-command"
channel_from="chat"
channel_to="browser-reply"
target_modify_id="owner"
strip_chat_prefix=false
allowed_logins=["alice"]
"#).unwrap();
        let out = root.join("out");
        let report = super::super::migrate_conf_file(&root.join("conf.toml"), &out, false).unwrap();
        assert!(!report.has_errors(), "{:?}", report.entries);
        let mut conf: crate::Conf = toml::from_str("").unwrap();
        conf.runtime_dir = Some(root.join("runtime"));
        let state = crate::State::new(&conf,
            Arc::new(tokio::sync::Mutex::new(crate::AudioSink::disconnected_for_test())),
            crate::shutdown::ShutdownBroker::new()).await.unwrap();
        let data = state.read().await.channel_data.clone();
        let mut receiver = state.read().await.channel_datum_tx.subscribe();
        let dictionary = out.join("flowgraph/chat-filters/assets/dictionary-0.tsv");
        for (actor, command, expect, busy) in [
            ("alice", "learn(X=読)", Some("学習:"), false),
            ("alice", "learn(X=読)", Some("登録済み"), false),
            ("stranger", "learn(X=偽)", None, false),
            ("alice", "learn(Y=失敗)", None, true),
            ("alice", "forget(X)", Some("忘却:"), false),
        ] {
            let before = data.read().await.len();
            let before_file = std::fs::read(&dictionary).unwrap();
            let lock = if busy {
                let lock = std::fs::OpenOptions::new().read(true).write(true).open(dictionary.parent().unwrap().join(".vac-data-write.lock")).unwrap();
                lock.try_lock().unwrap(); Some(lock)
            } else { None };
            let mut fixture = crate::flowgraph::fixture_runner::load_fixture_program(&out.join("flowgraph")).unwrap();
            fixture.execution_context.state_handle = Some(Arc::downgrade(&state));
            let (tx, rx) = crate::flowgraph::engine::create_trigger_bus();
            tx.send(TriggerEvent::new("web-input/main::in").with_exec("__trigger__")
                .with_override("__source_actor__", SocketValue::String(actor.into()))
                .with_override("__content__", SocketValue::String(command.into()))).unwrap();
            fixture.program.run_forever_with_bus(&mut fixture.execution_context, tx, rx,
                tokio::time::sleep(std::time::Duration::from_millis(100)), None).await.unwrap();
            drop(lock);
            if let Some(expected) = expect {
                let rows = data.read().await;
                assert_eq!(rows.len(), before + 1, "{command}");
                let row = rows.back().unwrap();
                assert_eq!(row.channel, "browser-reply");
                assert!(row.content.contains(expected), "{}", row.content);
                let broadcast = receiver.try_recv().unwrap();
                assert_eq!(broadcast.channel, row.channel);
                assert_eq!(broadcast.content, row.content);
                assert!(receiver.try_recv().is_err(), "duplicate output");
            } else {
                assert_eq!(data.read().await.len(), before);
                assert!(receiver.try_recv().is_err());
                assert_eq!(std::fs::read(&dictionary).unwrap(), before_file);
            }
        }
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn processed_command_keeps_original_actor_and_transformed_text() {
        let root = std::env::temp_dir().join(format!("vac-command-identity-{}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        for (name, text) in [("first.tsv", "NEXT\talice:GO\t名詞\t\n"), ("second.tsv", "learn(X=正)\tNEXT\t名詞\t\n"), ("user.tsv", "旧\tOLD\t名詞\t\n")] {
            std::fs::write(root.join(name), text).unwrap();
        }
        std::fs::write(root.join("conf.toml"), r#"
[[processors]]
feature="twitch"
channel_to="raw"
[[processors]]
feature="modify"
channel_from="raw"
channel_to="stage"
dictionary_files=["first.tsv"]
[[processors]]
feature="modify"
channel_from="stage"
channel_to="processed"
dictionary_files=["second.tsv"]
[[processors]]
feature="modify"
id="owner"
dictionary_files=["user.tsv"]
writable_dictionary_file="user.tsv"
[[processors]]
feature="dictionary-command"
channel_from="processed"
target_modify_id="owner"
strip_chat_prefix=false
allowed_logins=["alice"]
"#).unwrap();
        let out = root.join("out");
        let report = super::super::migrate_conf_file(&root.join("conf.toml"), &out, false).unwrap();
        assert!(!report.has_errors(), "{:?}", report.entries);
        let dictionary = out.join("flowgraph/chat-filters-2/assets/dictionary-0.tsv");
        for actor in ["stranger", "ALICE"] {
            let before = std::fs::read(&dictionary).unwrap();
            std::fs::write(out.join("flowgraph/test.flowgraph.test.toml"), format!(r#"
[[triggers]]
node="twitch-echo/main::in"
exec=["__trigger__"]
[[triggers.overrides]]
port="__content__"
ty="string"
value="alice:GO"
[[triggers.overrides]]
port="__source_actor__"
ty="string"
value="{actor}"
[[tests]]
name="Processed text, original sender"
[tests.expect]
trigger_count=1
"#)).unwrap();
            let result = crate::flowgraph::fixture_runner::run_fixture_once_report(&out.join("flowgraph")).await.unwrap();
            assert!(result.ok, "{:?}", result.tests);
            assert_eq!(before != std::fs::read(&dictionary).unwrap(), actor == "ALICE");
        }
        assert!(std::fs::read_to_string(dictionary).unwrap().contains("正\tX"));
        assert_eq!(std::fs::read_to_string(root.join("user.tsv")).unwrap(), "旧\tOLD\t名詞\t\n");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn merged_processed_commands_keep_each_event_actor() {
        let root = std::env::temp_dir().join(format!("vac-command-merged-identity-{}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        for (name, text) in [("first.tsv", "NX\talice:X\t名詞\t\nNY\talice:Y\t名詞\t\nNZ\talice:Z\t名詞\t\nNW\talice:W\t名詞\t\n"), ("second.tsv", "learn(X=一)\tNX\t名詞\t\nlearn(Y=二)\tNY\t名詞\t\nlearn(Z=偽)\tNZ\t名詞\t\nlearn(W=偽)\tNW\t名詞\t\n"), ("user.tsv", "旧\tOLD\t名詞\t\n")] {
            std::fs::write(root.join(name), text).unwrap();
        }
        std::fs::write(root.join("conf.toml"), r#"
[[processors]]
feature="twitch"
channel_to="raw"
[[processors]]
feature="twitch"
channel_to="raw"
[[processors]]
feature="modify"
channel_from="raw"
channel_to="stage"
dictionary_files=["first.tsv"]
[[processors]]
feature="modify"
channel_from="stage"
channel_to="processed"
dictionary_files=["second.tsv"]
[[processors]]
feature="modify"
id="owner"
dictionary_files=["user.tsv"]
writable_dictionary_file="user.tsv"
[[processors]]
feature="dictionary-command"
channel_from="processed"
target_modify_id="owner"
strip_chat_prefix=false
allowed_logins=["alice"]
"#).unwrap();
        let out = root.join("out");
        let report = super::super::migrate_conf_file(&root.join("conf.toml"), &out, false).unwrap();
        assert!(!report.has_errors(), "{:?}", report.entries);
        let dictionary = out.join("flowgraph/chat-filters-2/assets/dictionary-0.tsv");
        let mut fixture = String::new();
        for (source, actor, text) in [("twitch-echo", "alice", "X"), ("twitch-echo-1", "stranger", "Z"), ("twitch-echo-1", "ALICE", "Y"), ("twitch-echo", "stranger", "W")] {
            fixture.push_str(&format!(r#"
[[triggers]]
node="{source}/main::in"
exec=["__trigger__"]
[[triggers.overrides]]
port="__content__"
ty="string"
value="alice:{text}"
[[triggers.overrides]]
port="__source_actor__"
ty="string"
value="{actor}"
"#));
        }
        fixture.push_str("\n[[tests]]\nname=\"Merged transformed commands preserve actor\"\n[tests.expect]\ntrigger_count=4\n");
        std::fs::write(out.join("flowgraph/test.flowgraph.test.toml"), fixture).unwrap();
        let result = crate::flowgraph::fixture_runner::run_fixture_once_report(&out.join("flowgraph")).await.unwrap();
        assert!(result.ok, "{:?}", result.tests);
        let saved = std::fs::read_to_string(dictionary).unwrap();
        assert!(saved.contains("一\tX") && saved.contains("二\tY"), "{saved}");
        assert!(!saved.contains("偽"), "{saved}");
        assert_eq!(result.exec_count.iter().find(|(id, _)| id == "chat-dictionary-command/main::learn_save").unwrap().1, 2);
        assert_eq!(std::fs::read_to_string(root.join("user.tsv")).unwrap(), "旧\tOLD\t名詞\t\n");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn identity_resolution_rejects_cycles_in_every_branch() {
        let mut a = toml_edit::Table::new();
        a["feature"] = toml_edit::value("modify");
        a["channel_from"] = toml_edit::value("same");
        a["channel_to"] = toml_edit::value("same");
        let files = [(0, ("a".into(), std::path::PathBuf::from("a")))].into_iter().collect();
        assert!(super::identity_source(0, &[a.clone()], &files).unwrap_err().to_string().contains("cycle"));
        assert!(super::identity_source(0, &[a.clone(), a], &files).unwrap_err().to_string().contains("cycle"));
    }

    #[tokio::test]
    async fn legacy_forget_removes_all_matches_without_changing_new_undo_default() {
        let root = std::env::temp_dir().join(format!("vac-legacy-forget-{}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        let original = "A\tX\t名詞\t\nB\tX\t名詞\t\n";
        std::fs::write(root.join("user.tsv"), original).unwrap();
        std::fs::write(root.join("conf.toml"), r#"
[[processors]]
feature="web-input"
channel_to="commands"
[[processors]]
feature="modify"
id="owner"
dictionary_files=["user.tsv"]
writable_dictionary_file="user.tsv"
[[processors]]
feature="dictionary-command"
channel_from="commands"
target_modify_id="owner"
strip_chat_prefix=false
"#).unwrap();
        let out = root.join("out");
        let report = super::super::migrate_conf_file(&root.join("conf.toml"), &out, false).unwrap();
        assert!(!report.has_errors(), "{:?}", report.entries);
        let dictionary = out.join("flowgraph/chat-filters/assets/dictionary-0.tsv");
        // Include repeated exact matches, another reading, a different surface and a protected row.
        let header = include_str!("../../flowgraph.example/dictionary-managed-command/assets/user.tsv");
        let rows = "A\tX\t名詞\t\t0\tfalse\ttrue\tuser\t\t\t\nA\tX\t名詞\t\t0\tfalse\ttrue\tuser\t\t\t\nB\tX\t名詞\t\t0\tfalse\ttrue\tuser\t\t\t\nOther\tY\t名詞\t\t0\tfalse\ttrue\tuser\t\t\t\nA\tX\t名詞\t\t0\ttrue\ttrue\tsystem\t\t\t\n";
        std::fs::write(&dictionary, format!("{header}{rows}")).unwrap();
        for (command, remaining, removed) in [("forget(X=A)", 3, 2), ("forget(X)", 2, 1)] {
            std::fs::write(out.join("flowgraph/test.flowgraph.test.toml"), format!(r#"
[[triggers]]
node="web-input/main::in"
exec=["__trigger__"]
[[triggers.overrides]]
port="__content__"
ty="string"
value="{command}"
[[tests]]
name="Legacy all matching removal"
[tests.expect]
trigger_count=1
"#)).unwrap();
            let result = crate::flowgraph::fixture_runner::run_fixture_once_report(&out.join("flowgraph")).await.unwrap();
            assert!(result.ok, "{:?}", result.tests);
            let text = std::fs::read_to_string(&dictionary).unwrap();
            let table = crate::flowgraph::nodes::table_ops::parse_tsv_with_mode(&text, "headerful").unwrap();
            assert_eq!(table.len(), remaining);
            assert!(text.contains("Other\tY"));
            assert!(text.contains("true\ttrue\tsystem"));
            assert!(result.stored_values.iter().any(|v| v.port == "removed_count" && v.value == serde_json::json!(removed)));
        }
        let sample: toml_edit::DocumentMut = include_str!("../../flowgraph.example/dictionary-managed-command/main.flowgraph.toml").parse().unwrap();
        let modes: Vec<_> = sample["nodes"].as_array_of_tables().unwrap().iter().filter(|n| n["id"].as_str() == Some("undo_mode")).collect();
        assert_eq!(modes[0]["properties"]["value"].as_str(), Some("latest"));
        assert_eq!(std::fs::read_to_string(root.join("user.tsv")).unwrap(), original);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn multiple_command_sources_keep_sender_and_text_together() {
        let root = std::env::temp_dir().join(format!("vac-multi-command-{}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("user.tsv"), "旧\tOLD\t名詞\t\n").unwrap();
        std::fs::write(root.join("conf.toml"), r#"
[[processors]]
feature="twitch"
channel_to="chat"
[[processors]]
feature="twitch"
channel_to="chat"
[[processors]]
feature="twitch"
channel_to="chat"
[[processors]]
feature="modify"
id="owner"
dictionary_files=["user.tsv"]
writable_dictionary_file="user.tsv"
[[processors]]
feature="dictionary-command"
channel_from="chat"
target_modify_id="owner"
allowed_logins=["alice"]
"#).unwrap();
        let out = root.join("out");
        let report = super::super::migrate_conf_file(&root.join("conf.toml"), &out, false).unwrap();
        assert!(!report.has_errors(), "{:?}", report.entries);
        let mut fixture = String::new();
        for (node, actor, text) in [("twitch-echo", "alice", "learn(X=一)"), ("twitch-echo-1", "stranger", "learn(BAD=偽)"),
            ("twitch-echo-2", "ALICE", "learn(Y=二)"), ("twitch-echo", "stranger", "learn(Z=偽)")] {
            fixture.push_str(&format!(r#"
[[triggers]]
node="{node}/main::in"
exec=["__trigger__"]
[[triggers.overrides]]
port="__source_actor__"
ty="string"
value="{actor}"
[[triggers.overrides]]
port="__content__"
ty="string"
value="alice:spoofed-display"
[[triggers.overrides]]
port="__meta__"
ty="json"
value={{message_text="{text}"}}
"#));
        }
        fixture.push_str("\n[[tests]]\nname=\"Multiple inputs keep their own identity\"\n[tests.expect]\ntrigger_count=4\n");
        std::fs::write(out.join("flowgraph/test.flowgraph.test.toml"), fixture).unwrap();
        let result = crate::flowgraph::fixture_runner::run_fixture_once_report(&out.join("flowgraph")).await.unwrap();
        assert!(result.ok, "{:?}", result.tests);
        let saved = std::fs::read_to_string(out.join("flowgraph/chat-filters/assets/dictionary-0.tsv")).unwrap();
        assert!(saved.contains("一\tX") && saved.contains("二\tY"), "{saved}");
        assert!(!saved.contains("偽") && !saved.contains("BAD"), "{saved}");
        assert_eq!(result.exec_count.iter().find(|(id, _)| id == "chat-dictionary-command/main::learn_save").unwrap().1, 2);
        assert_eq!(std::fs::read_to_string(root.join("user.tsv")).unwrap(), "旧\tOLD\t名詞\t\n");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn flagged_client_migration_draft_preserves_policy_and_stays_inactive() {
        let root = std::env::temp_dir().join(format!("vac-flags-draft-{}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("user.tsv"), "旧\tOLD\t名詞\t\n").unwrap();
        std::fs::write(root.join("conf.toml"), r#"
[[processors]]
feature="modify"
id="owner"
dictionary_files=["user.tsv"]
writable_dictionary_file="user.tsv"
[[processors]]
feature="dictionary-command"
channel_from="old-trusted-channel"
target_modify_id="owner"
allowed_flags=["dictionary.write"]
allowed_logins=["operator"]
"#).unwrap();
        let out = root.join("out");
        let report = super::super::migrate_conf_file(&root.join("conf.toml"), &out, false).unwrap();
        assert!(report.has_errors(), "must require client migration");
        let folder = out.join("flowgraph/chat-dictionary-command");
        let main = folder.join("main.flowgraph.toml");
        assert!(std::fs::read_to_string(&main).unwrap().contains("flowgraph.dictionary.command"), "draft must not replace runtime graph automatically");
        let plan: serde_json::Value = serde_json::from_slice(&std::fs::read(folder.join("control-input-migration.json")).unwrap()).unwrap();
        assert_eq!(plan["automatic_activation"], false);
        assert_eq!(plan["target_node"], "chat-dictionary-command/main::in");
        assert_eq!(plan["request_example"]["inputs"]["flags"], serde_json::json!([]));
        let draft = std::fs::read_to_string(folder.join("main.control-input.toml")).unwrap();
        assert!(draft.contains("dictionary.write") && draft.contains("operator"));
        // Simulate explicit installation only inside this disposable migrated tree.
        std::fs::write(&main, draft).unwrap();
        let dictionary = out.join("flowgraph/chat-filters/assets/dictionary-0.tsv");
        for (actor, flags, command, changed) in [("stranger", "[]", "learn(X=dictionary.write)", false),
            ("stranger", "[\"dictionary.write\"]", "learn(X=正)", true), ("OPERATOR", "[]", "learn(Y=別)", true)] {
            let before = std::fs::read(&dictionary).unwrap();
            std::fs::write(out.join("flowgraph/test.flowgraph.test.toml"), format!(r#"
[[triggers]]
node="chat-dictionary-command/main::in"
exec=["exec_in"]
[[triggers.overrides]]
port="content"
ty="string"
value="{command}"
[[triggers.overrides]]
port="source_actor"
ty="string"
value="{actor}"
[[triggers.overrides]]
port="flags"
ty="list<string>"
value={flags}
[[tests]]
name="Explicit client migration preserves OR policy"
[tests.expect]
trigger_count=1
"#)).unwrap();
            let result = crate::flowgraph::fixture_runner::run_fixture_once_report(&out.join("flowgraph")).await.unwrap();
            assert!(result.ok, "{:?}", result.tests);
            assert_eq!(std::fs::read(&dictionary).unwrap() != before, changed);
        }
        assert_eq!(std::fs::read_to_string(root.join("user.tsv")).unwrap(), "旧\tOLD\t名詞\t\n");
        std::fs::remove_dir_all(root).unwrap();
    }

}
