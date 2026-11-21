//! Copy legacy dictionaries into a portable, explicitly loaded glossary.
use super::{dictionary_file::{convert_input, header_line, InputSpec, KindHint}, MigrationReport};
use anyhow::{bail, Context, Result};
use std::path::Path;
use toml_edit::Table;

pub(super) fn render(
    processor: &Table,
    source_base: Option<&Path>,
    topic_dir: &Path,
    report: &mut MigrationReport,
) -> Result<String> {
    let mut output = format!("{}\n", header_line());
    let mut separate = Vec::new();
    let mut writable = Vec::new();
    for (field, list) in [("writable_dictionary_file", "dictionary_files"), ("writable_regex_file", "regex_files")] {
        if let Some(value) = processor.get(field) {
            let name = value.as_str().with_context(|| format!("{field} must be a path string"))?;
            if name.is_empty() || !processor.get(list).and_then(|v| v.as_array())
                .is_some_and(|paths| paths.iter().any(|p| p.as_str() == Some(name))) {
                bail!("{field} must name a member of {list}");
            }
            writable.push((list, name.to_owned()));
        }
    }
    // Keep file and entry order. Do not deduplicate entries belonging to the user.
    for (field, kind) in [("dictionary_files", KindHint::Literal), ("regex_files", KindHint::Regex)] {
        let Some(value) = processor.get(field) else { continue };
        let paths = value.as_array().with_context(|| format!("{field} must be an array"))?;
        for value in paths {
            let name = value.as_str().with_context(|| format!("{field} must contain paths"))?;
            if name.is_empty() { bail!("{field} contains an empty path"); }
            let path = Path::new(name);
            let path = if path.is_absolute() {
                path.to_owned()
            } else {
                source_base.with_context(|| format!("relative dictionary path {name:?} needs the original conf.toml location"))?.join(path)
            };
            let spec = InputSpec { path, kind_hint: kind, tag: None };
            let mut one = format!("{}\n", header_line());
            for row in convert_input(&spec, "migration", false)? {
                let line = format!("{}\n", row.to_tsv_line());
                output.push_str(&line);
                one.push_str(&line);
            }
            separate.push((field, name.to_owned(), one));
        }
    }
    if !writable.is_empty() {
        return render_separate(separate, &writable, topic_dir, report);
    }
    let assets = topic_dir.join("assets");
    std::fs::create_dir_all(&assets)?;
    let dictionary = assets.join("dictionary.tsv");
    std::fs::write(&dictionary, output)?;
    report.written_files.push(dictionary);
    report.warn(Some("modify".into()), "辞書は同梱TSVへコピーしました。旧channel_from/channel_toの接続と、学習・忘却の書込先は手動移行が必要です。元ファイルは変更していません。");
    Ok(r#"[meta]
title = "Migrated: modify"
description = "Copied glossary with explicit load-before-replace; reconnect legacy channels before use."
tags = ["migrated", "modify", "glossary"]

[[nodes]]
id = "in"
feature = "flowgraph.ingress.web_input"
position = [80, 300]
[[nodes]]
id = "dict_path"
feature = "flowgraph.literal.string"
position = [80, 100]
properties.value = "graph-file:assets/dictionary.tsv"
[[nodes]]
id = "load"
feature = "flowgraph.table.load_tsv"
position = [300, 100]
[[nodes]]
id = "dict"
feature = "flowgraph.glossary.replace"
position = [550, 300]
[[nodes]]
id = "log"
feature = "flowgraph.util.log"
position = [800, 300]
[[nodes]]
id = "load_error"
feature = "flowgraph.util.log"
position = [550, 100]
[[edges]]
from = "in:exec_out"
to = "load:exec_in"
[[edges]]
from = "dict_path:value"
to = "load:path"
[[edges]]
from = "in:content"
to = "dict:content"
[[edges]]
from = "load:table"
to = "dict:dictionary"
[[edges]]
from = "load:on_success"
to = "log:exec_in"
[[edges]]
from = "dict:result"
to = "log:value"
[[edges]]
from = "load:on_error"
to = "load_error:exec_in"
[[edges]]
from = "load:error"
to = "load_error:value"
"#.into())
}

fn render_separate(
    inputs: Vec<(&str, String, String)>, writable: &[(&str, String)],
    topic_dir: &Path, report: &mut MigrationReport,
) -> Result<String> {
    let assets = topic_dir.join("assets");
    std::fs::create_dir_all(&assets)?;
    let mut files: std::collections::BTreeMap<(String, String), String> = std::collections::BTreeMap::new();
    let mut manifest = Vec::new();
    let mut graph = String::from("[meta]\ntitle = \"Migrated: modify\"\ndescription = \"Separate dictionary assets; ordered loading and concatenation.\"\ntags = [\"migrated\", \"modify\", \"glossary\"]\n");
    let mut edges = String::new();
    fn node(graph: &mut String, id: &str, feature: &str) {
        graph.push_str(&format!("\n[[nodes]]\nid = {}\nfeature = {}\n", super::toml_str(id), super::toml_str(feature)));
    }
    fn edge(edges: &mut String, from: &str, to: &str) {
        edges.push_str(&format!("\n[[edges]]\nfrom = {}\nto = {}\n", super::toml_str(from), super::toml_str(to)));
    }
    node(&mut graph, "in", "flowgraph.ingress.web_input");
    let mut previous_exec = "in:exec_out".to_owned();
    let mut combined = String::new();
    for (i, (kind, original, contents)) in inputs.iter().enumerate() {
        let key = (kind.to_string(), original.clone());
        let asset = if let Some(asset) = files.get(&key) { asset.clone() } else {
            let asset = format!("assets/dictionary-{}.tsv", files.len());
            let path = topic_dir.join(&asset);
            std::fs::write(&path, contents)?;
            report.written_files.push(path);
            files.insert(key, asset.clone());
            asset
        };
        manifest.push(serde_json::json!({
            "kind": kind, "original": original, "asset": asset,
            "writable": writable.iter().any(|(list, name)| list == kind && name == original)
        }));
        // Preserve the terminal "load" node used by cross-processor topology.
        let load = if i + 1 == inputs.len() { "load".into() } else { format!("load_{i}") };
        let path = format!("path_{i}");
        node(&mut graph, &path, "flowgraph.literal.string");
        graph.push_str(&format!("properties.value = {}\n", super::toml_str(&format!("graph-file:{asset}"))));
        node(&mut graph, &load, "flowgraph.table.load_tsv");
        edge(&mut edges, &format!("{path}:value"), &format!("{load}:path"));
        edge(&mut edges, &previous_exec, &format!("{load}:exec_in"));
        let error = format!("load_error_{i}");
        node(&mut graph, &error, "flowgraph.util.log");
        edge(&mut edges, &format!("{load}:on_error"), &format!("{error}:exec_in"));
        edge(&mut edges, &format!("{load}:error"), &format!("{error}:value"));
        if combined.is_empty() { combined = format!("{load}:table"); } else {
            let concat = format!("concat_{i}");
            node(&mut graph, &concat, "flowgraph.table.concat");
            edge(&mut edges, &combined, &format!("{concat}:first"));
            edge(&mut edges, &format!("{load}:table"), &format!("{concat}:second"));
            combined = format!("{concat}:table");
        }
        previous_exec = format!("{load}:on_success");
    }
    node(&mut graph, "dict", "flowgraph.glossary.replace");
    node(&mut graph, "log", "flowgraph.util.log");
    edge(&mut edges, "in:content", "dict:content");
    edge(&mut edges, &combined, "dict:dictionary");
    edge(&mut edges, &previous_exec, "log:exec_in");
    edge(&mut edges, "dict:result", "log:value");
    graph.push_str(&edges);
    let manifest_path = topic_dir.join("dictionary-assets.json");
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    report.written_files.push(manifest_path);
    report.warn(Some("modify".into()), "書込対象の区別を維持して辞書を個別TSVへコピーしました。dictionary-assets.json に対応を記録しています。学習・忘却コマンドと競合制御の配線は未完了です。元ファイルは変更していません。");
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::super::{migrate_conf_file, migrate_conf_str, EntryKind};

    #[tokio::test]
    async fn writable_dictionary_stays_separate_and_reloads_without_changing_fixed_entries() {
        let root = std::env::temp_dir().join(format!("vac-writable-migration-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("fixed.tsv"), "固定\tFIXED\t名詞\t\n").unwrap();
        std::fs::write(root.join("user.tsv"), "旧\tUSER\t名詞\t\n").unwrap();
        std::fs::write(root.join("conf.toml"), r#"
[[processors]]
feature="modify"
dictionary_files=["fixed.tsv","user.tsv"]
writable_dictionary_file="user.tsv"
"#).unwrap();
        let out = root.join("migrated");
        let report = migrate_conf_file(&root.join("conf.toml"), &out, false).unwrap();
        assert!(!report.has_errors(), "{:?}", report.entries);
        let topic = out.join("flowgraph/chat-filters");
        let mapping: serde_json::Value = serde_json::from_slice(&std::fs::read(topic.join("dictionary-assets.json")).unwrap()).unwrap();
        assert_eq!(mapping[0]["writable"], false);
        assert_eq!(mapping[1]["writable"], true);
        let fixed = topic.join(mapping[0]["asset"].as_str().unwrap());
        let editable = topic.join(mapping[1]["asset"].as_str().unwrap());
        let fixed_bytes = std::fs::read(&fixed).unwrap();
        let graph_path = topic.join("main.flowgraph.toml");
        let graph = std::fs::read_to_string(&graph_path).unwrap()
            .replace("feature = \"flowgraph.ingress.web_input\"", "feature = \"flowgraph.literal.string\"\nproperties.value = \"FIXED USER\"")
            .replace("from = \"in:content\"", "from = \"in:value\"")
            .replace("[[edges]]\nfrom = \"in:exec_out\"\nto = \"load_0:exec_in\"\n", "");
        std::fs::write(graph_path, graph).unwrap();
        for expected in ["固定 旧", "固定 新"] {
            if expected.ends_with('新') {
                let current = std::fs::read_to_string(&editable).unwrap();
                std::fs::write(&editable, current.replace("旧\tUSER", "新\tUSER")).unwrap();
            }
            std::fs::write(out.join("flowgraph/verify.flowgraph.test.toml"), format!(r#"
[[triggers]]
node="chat-filters/main::load_0"
exec=["exec_in"]
[[tests]]
name="Separate writable dictionary"
[[tests.expect.stored_values]]
node="chat-filters/main::dict"
port="result"
value="{expected}"
"#)).unwrap();
            let result = crate::flowgraph::fixture_runner::run_fixture_once_report(&out.join("flowgraph")).await.unwrap();
            assert!(result.ok, "{:?}", result.tests);
        }
        assert_eq!(std::fs::read(&fixed).unwrap(), fixed_bytes);
        assert_eq!(std::fs::read_to_string(root.join("user.tsv")).unwrap(), "旧\tUSER\t名詞\t\n");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn writable_dictionary_outside_loaded_list_is_rejected() {
        let out = std::env::temp_dir().join(format!("vac-writable-invalid-{}", rand::random::<u64>()));
        let report = migrate_conf_str("[[processors]]\nfeature='modify'\ndictionary_files=[]\nwritable_dictionary_file='other.tsv'", &out, false).unwrap();
        assert!(report.has_errors());
        assert!(report.entries.iter().any(|entry| entry.message.contains("must name a member")));
        assert!(!out.join("flowgraph/chat-filters/main.flowgraph.toml").exists());
        std::fs::remove_dir_all(out).unwrap();
    }

    #[tokio::test]
    async fn copied_literal_and_regex_dictionary_executes_without_original_files() {
        let root = std::env::temp_dir().join(format!("vac-migrate-dict-{}", rand::random::<u64>()));
        let source = root.join("元設定");
        let out = root.join("migrated");
        std::fs::create_dir_all(&source).unwrap();
        let dictionary = "うさぎ\tUSAGI\t名詞\t既存の注記\n";
        let regex = "得点$1 SCORE(\\d+)\n";
        std::fs::write(source.join("辞書.tsv"), dictionary).unwrap();
        std::fs::write(source.join("regex.txt"), regex).unwrap();
        std::fs::write(source.join("conf.toml"), r#"
[[processors]]
feature="modify"
dictionary_files=["辞書.tsv"]
regex_files=["regex.txt"]
"#).unwrap();
        let report = migrate_conf_file(&source.join("conf.toml"), &out, false).unwrap();
        assert!(!report.has_errors(), "{:?}", report.entries);
        assert_eq!(std::fs::read_to_string(source.join("辞書.tsv")).unwrap(), dictionary);
        assert_eq!(std::fs::read_to_string(source.join("regex.txt")).unwrap(), regex);
        std::fs::remove_dir_all(&source).unwrap();
        let graph_path = out.join("flowgraph/chat-filters/main.flowgraph.toml");
        // Supply deterministic text instead of starting a real ingress service.
        // The generated load, replacement and success/error execution paths stay intact.
        let graph = std::fs::read_to_string(&graph_path).unwrap()
            .replace("feature = \"flowgraph.ingress.web_input\"", "feature = \"flowgraph.literal.string\"\nproperties.value = \"USAGI SCORE42\"")
            .replace("from = \"in:content\"", "from = \"in:value\"")
            .replace("[[edges]]\nfrom = \"in:exec_out\"\nto = \"load:exec_in\"\n", "");
        std::fs::write(&graph_path, graph).unwrap();
        std::fs::write(out.join("flowgraph/verify.flowgraph.test.toml"), r#"
[[triggers]]
node="chat-filters/main::load"
exec=["exec_in"]
[[tests]]
name="Copied glossary produces literal and regex replacements"
[[tests.expect.stored_values]]
node="chat-filters/main::dict"
port="result"
value="うさぎ 得点42"
"#).unwrap();
        let result = crate::flowgraph::fixture_runner::run_fixture_once_report(&out.join("flowgraph")).await.unwrap();
        assert!(result.ok, "{:?}", result.tests);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn relative_dictionary_without_source_location_fails_instead_of_becoming_empty() {
        let out = std::env::temp_dir().join(format!("vac-migrate-no-base-{}", rand::random::<u64>()));
        let report = migrate_conf_str("[[processors]]\nfeature='modify'\ndictionary_files=['missing.tsv']\n", &out, false).unwrap();
        assert!(report.has_errors());
        assert!(report.entries.iter().any(|entry| entry.message.contains("original conf.toml location")));
        assert!(!report.entries.iter().any(|entry| entry.kind == EntryKind::Converted));
        assert!(!out.join("flowgraph/chat-filters/main.flowgraph.toml").exists());
        std::fs::remove_dir_all(out).unwrap();
    }
}
