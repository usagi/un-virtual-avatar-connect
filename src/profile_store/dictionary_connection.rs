//! Review and publish saved-profile dictionary connections without runtime reload.
use std::path::Path;
use anyhow::{bail, Context, Result};
use crate::flowgraph::speech_route_sources::{DictionaryFailurePolicy, InsertionContext, ReviewedInsertion, read_complete_sources};

pub struct ConnectionPreview {
    pub reviewed: ReviewedInsertion,
    pub affected_destinations: Vec<String>,
    pub dictionary_revision: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ConnectionSaved {
    pub profile_source: String,
    pub graph_root: String,
    pub transaction_id: String,
    pub changed_files: Vec<String>,
    pub affected_destinations: Vec<String>,
    pub saved: bool,
    pub runtime_applied: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct RecoveryStatus {
    pub profile_source: String,
    pub profile_revision: String,
    pub graph_root: String,
    pub pending: Option<crate::flowgraph::source_transaction::PendingStatus>,
    pub restored: bool,
    pub runtime_applied: bool,
}

/// Derive the recovery root from this server's saved profile, never from a
/// browser-supplied directory. The same revision binds inspection and restore.
pub fn recovery(conf_path: &Path, expected_revision: &str, restore_id: Option<&str>) -> Result<RecoveryStatus> {
    let _config_lock = super::lifecycle::lock_config_write(conf_path)?;
    let source = std::fs::read_to_string(conf_path)?;
    let revision = blake3::hash(source.as_bytes()).to_hex().to_string();
    if revision != expected_revision { bail!("設定が変わりました。復元対象を確認し直してください。"); }
    let mut conf: crate::Conf = toml::from_str(&source)?;
    conf.source_path = Some(conf_path.to_path_buf());
    let root = conf.resolved_flowgraph_dir().context("Flowgraphの保存先が未設定です。")?.canonicalize()?;
    let _root_lock = crate::flowgraph::write_lock::acquire(&root)?;
    let mut pending = crate::flowgraph::source_transaction::pending_status(&root)?;
    if let Some(id) = restore_id {
        let status = pending.as_ref().context("復元待ちの保存はありません。状態を確認し直してください。")?;
        if status.transaction_id != id { bail!("復元対象が変わりました。状態を確認し直してください。"); }
        if !status.restorable { bail!("保存後の編集または読込不能なファイルがあります。自動復元せず内容を確認してください。"); }
        crate::flowgraph::source_transaction::restore(&root, id)?;
        pending = crate::flowgraph::source_transaction::pending_status(&root)?;
    }
    Ok(RecoveryStatus {
        profile_source: conf_path.to_string_lossy().into_owned(), profile_revision: revision,
        graph_root: root.to_string_lossy().into_owned(), pending,
        restored: restore_id.is_some(), runtime_applied: false,
    })
}

/// Server-owned profile path; the caller supplies only the reviewed expectations.
/// A failed publication retains its journal for explicit recovery, never retry.
pub fn save(
    conf_path: &Path, expected_revision: &str, dictionary_key: &str,
    destination: &str, policy: DictionaryFailurePolicy, review_revision: &str,
    dictionary_revision: &str, confirm_shared_definition: bool,
) -> Result<ConnectionSaved> {
    let preview = prepare(conf_path, expected_revision, dictionary_key, destination, policy)?;
    if preview.reviewed.revision() != review_revision || preview.dictionary_revision != dictionary_revision {
        bail!("確認後に構成または辞書が変わりました。接続案を確認し直してください。");
    }
    if preview.affected_destinations.len() > 1 && !confirm_shared_definition {
        bail!("共有定義を使う全読み上げ先への変更を確認してください。");
    }
    let _config_lock = super::lifecycle::lock_config_write(conf_path)?;
    let current = std::fs::read_to_string(conf_path)?;
    if blake3::hash(current.as_bytes()).to_hex().as_str() != expected_revision {
        bail!("設定が変わりました。対象を確認し直してください。");
    }
    let context = preview.reviewed.context();
    let root = Path::new(&context.graph_root);
    let _root_lock = crate::flowgraph::write_lock::acquire(root)?;
    crate::flowgraph::source_transaction::ensure_no_pending(root)?;
    let (_, current_dictionary_revision, _) = super::dictionary_setup::inspect_existing(Path::new(&context.dictionary_path))?;
    if current_dictionary_revision != dictionary_revision {
        bail!("確認後に辞書が変わりました。接続案を確認し直してください。");
    }
    let sources = read_complete_sources(root).map_err(anyhow::Error::msg)?;
    preview.reviewed.verify_current(review_revision, context, &sources).map_err(anyhow::Error::msg)?;
    let id = crate::flowgraph::source_transaction::prepare(root, preview.reviewed.changes())?;
    crate::flowgraph::source_transaction::publish(root, &id)
        .with_context(|| format!("接続の保存が未完了です。復元用記録: {id}"))?;
    Ok(ConnectionSaved {
        profile_source: context.profile_source.clone(), graph_root: context.graph_root.clone(),
        transaction_id: id, changed_files: preview.reviewed.changes().keys().cloned().collect(),
        affected_destinations: preview.affected_destinations, saved: true, runtime_applied: false,
    })
}

/// conf_path is supplied by the active server profile, not by request routing.
/// The revision and selected runtime node are optimistic expectations only.
pub fn prepare(
    conf_path:&Path, expected_revision:&str, dictionary_key:&str,
    destination:&str, policy:DictionaryFailurePolicy,
)->Result<ConnectionPreview> {
    let _config_lock=super::lifecycle::lock_config_write(conf_path)?;
    let original=std::fs::read_to_string(conf_path)?;
    let revision=blake3::hash(original.as_bytes()).to_hex().to_string();
    if revision!=expected_revision {bail!("設定が変わりました。対象を確認し直してください。");}
    let mut conf:crate::Conf=toml::from_str(&original)?;
    conf.source_path=Some(conf_path.to_path_buf());
    let tables=&conf.control_api.as_ref().context("辞書一覧が設定されていません。")?.tables;
    let matches:Vec<_>=tables.iter().filter(|table|table.key==dictionary_key).collect();
    if matches.len()!=1 {bail!("辞書IDを一つに特定できません。");}
    // Existing Control tables intentionally resolve relative paths against cwd.
    // Do not reinterpret old profiles relative to conf.toml here.
    let (dictionary_path,dictionary_revision,_)=super::dictionary_setup::inspect_existing(&matches[0].path)?;
    let root=conf.resolved_flowgraph_dir().context("Flowgraphの保存先が未設定です。")?.canonicalize()?;
    let _root_lock=crate::flowgraph::write_lock::acquire(&root)?;
    crate::flowgraph::source_transaction::ensure_no_pending(&root)?;
    let sources=read_complete_sources(&root).map_err(anyhow::Error::msg)?;
    let loaded=crate::flowgraph::loader::dir::load_flowgraph_source_snapshot(&root,&sources).map_err(|e|anyhow::anyhow!("構成を読み込めません: {e:?}"))?;
    if read_complete_sources(&root).map_err(anyhow::Error::msg)?!=sources {
        bail!("構成が確認中に変わりました。もう一度確認してください。");
    }
    let routes=crate::flowgraph::speech_routes::inspect(&loaded);
    let selected=routes.iter().find(|route|route.runtime_node==destination).context("選んだ読み上げ先が保存済み構成にありません。")?;
    let (_,node)=selected.runtime_node.split_once("::").context("読み上げ先の識別子が不正です。")?;
    let graph=crate::flowgraph::loader::fq_path_of_file(&root,&selected.source_file).map_err(anyhow::Error::msg)?;
    let affected_destinations=routes.iter().filter(|route|route.source_file==selected.source_file
        && route.runtime_node.split_once("::").is_some_and(|(_,id)|id==node))
        .map(|route|route.runtime_node.clone()).collect();
    let context=InsertionContext{
        profile_source:conf_path.to_string_lossy().into_owned(),profile_revision:revision,
        graph_root:root.to_string_lossy().into_owned(),dictionary_key:dictionary_key.into(),
        dictionary_path:dictionary_path.to_str().context("辞書パスを文字列として扱えません。")?.into(),
        target_graph:graph,target_node:node.into(),failure:policy,
    };
    let reviewed=ReviewedInsertion::prepare(context,sources.clone()).map_err(anyhow::Error::msg)?;
    let mut proposed=sources;
    for(fq,change) in reviewed.changes() {proposed.insert(fq.clone(),change.proposed.clone());}
    let checked=crate::flowgraph::loader::dir::load_flowgraph_source_snapshot(&root,&proposed)
        .map_err(|e|anyhow::anyhow!("変更後の構成を検証できません: {e:?}"))?;
    if checked.diagnostics.iter().any(|d|d.severity==crate::flowgraph::loader::Severity::Error) {
        bail!("変更後の構成に診断エラーがあります: {:?}",checked.diagnostics);
    }
    Ok(ConnectionPreview{reviewed,affected_destinations,dictionary_revision})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn saved_connections_execute_with_each_profiles_registered_dictionary() {
        let temp = std::env::temp_dir().join(format!("vac-saved-dictionary-route-{}", rand::random::<u64>()));
        std::fs::create_dir(&temp).unwrap();
        let mut profiles = Vec::new();
        for (name, reading) in [("first", "ゆーえぬ"), ("second", "別の読み")] {
            let root = temp.join(name);
            let graphs = root.join("graphs");
            std::fs::create_dir_all(&graphs).unwrap();
            let main = include_str!("../../flowgraph.example/chat-echo/main.flowgraph.toml")
                .replace("flowgraph.ingress.twitch", "flowgraph.ingress.web_input");
            // Evaluate the real route, but stop before synthesis/driver lookup.
            let tts = format!("{}\n[[nodes]]\nid='test_floor'\nfeature='flowgraph.literal.string'\nproperties.value='isolated_test'\n[[edges]]\nfrom='test_floor:value'\nto='speaker:respect_speech_floor'\n",
                include_str!("../../flowgraph.example/chat-echo/tts.flowgraph.toml"));
            std::fs::write(graphs.join("main.flowgraph.toml"), main).unwrap();
            std::fs::write(graphs.join("tts.flowgraph.toml"), tts).unwrap();
            let dictionary = root.join("reading.tsv");
            std::fs::write(&dictionary, format!("reading\tsurface\n{reading}\tUN\n")).unwrap();
            let conf_path = root.join("conf.toml");
            let conf = format!("flowgraph_dir='graphs'\n[[control_api.tables]]\nkey='reading'\npath={}\n",
                toml::Value::String(dictionary.to_string_lossy().into_owned()));
            std::fs::write(&conf_path, &conf).unwrap();
            let revision = blake3::hash(conf.as_bytes()).to_hex().to_string();
            let preview = prepare(&conf_path, &revision, "reading", "tts::speaker", DictionaryFailurePolicy::SkipSpeech).unwrap();
            let receipt = save(&conf_path, &revision, "reading", "tts::speaker", DictionaryFailurePolicy::SkipSpeech,
                preview.reviewed.revision(), &preview.dictionary_revision, false).unwrap();
            assert!(receipt.saved);
            assert!(!receipt.runtime_applied);
            std::fs::write(graphs.join("main.flowgraph.test.toml"), r#"
[[triggers]]
node='main::in'
exec=['__trigger__']
[[triggers.overrides]]
port='__content__'
ty='string'
value='UN stream'
[[tests]]
name='saved profile route'
[tests.expect]
trigger_count=1
"#).unwrap();
            profiles.push((graphs, conf_path, conf, dictionary, reading));
        }
        // Load both saved routes after both saves; repeated A/B/A catches
        // accidental use of a global dictionary key or another profile's path.
        for index in [0, 1, 0] {
            let (graphs, conf_path, conf, dictionary, reading) = &profiles[index];
            let report = crate::flowgraph::fixture_runner::run_fixture_once_report(graphs).await.unwrap();
            assert!(report.ok, "{:?}", report.tests);
            let value = report.stored_values.iter().find(|v| v.node == "tts::reading_replace" && v.port == "result").unwrap();
            assert_eq!(value.value, serde_json::json!(format!("{reading} stream")));
            let error = report.stored_values.iter().find(|v| v.node == "tts::speaker" && v.port == "error").unwrap();
            assert_eq!(error.value, serde_json::json!("respect_speech_floor requires shared runtime state"));
            assert_eq!(std::fs::read_to_string(conf_path).unwrap(), *conf);
            assert_eq!(std::fs::read_to_string(dictionary).unwrap(), format!("reading\tsurface\n{reading}\tUN\n"));
        }
        let resolved = temp.canonicalize().unwrap();
        assert!(resolved.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert!(resolved.file_name().unwrap().to_string_lossy().starts_with("vac-saved-dictionary-route-"));
        std::fs::remove_dir_all(resolved).unwrap();
    }

    #[test]
    fn dictionary_connection_uses_profile_targets_without_publishing() {
        let root=std::env::temp_dir().join(format!("vac-dictionary-profile-plan-{}",rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();std::fs::create_dir(root.join("graphs")).unwrap();
        let main=include_str!("../../flowgraph.example/chat-echo/main.flowgraph.toml");
        let tts=include_str!("../../flowgraph.example/chat-echo/tts.flowgraph.toml");
        std::fs::write(root.join("graphs/main.flowgraph.toml"),main).unwrap();
        std::fs::write(root.join("graphs/tts.flowgraph.toml"),tts).unwrap();
        let dictionary=root.join("reading.tsv");
        std::fs::write(&dictionary,"reading\tsurface\nゆーえぬ\tUN\n").unwrap();
        let conf_path=root.join("conf.toml");
        let conf=format!("flowgraph_dir='graphs'\n[[control_api.tables]]\nkey='reading'\npath={}\n",toml::Value::String(dictionary.to_string_lossy().into_owned()));
        std::fs::write(&conf_path,&conf).unwrap();
        let revision=blake3::hash(conf.as_bytes()).to_hex().to_string();
        let preview=prepare(&conf_path,&revision,"reading","tts::speaker",DictionaryFailurePolicy::SkipSpeech).unwrap();
        assert_eq!(preview.affected_destinations,vec!["tts::speaker"]);
        assert_eq!(preview.reviewed.changes().len(),2);
        assert!(!preview.dictionary_revision.is_empty());
        assert!(prepare(&conf_path,"old","reading","tts::speaker",DictionaryFailurePolicy::SkipSpeech).is_err());
        assert!(prepare(&conf_path,&revision,"missing","tts::speaker",DictionaryFailurePolicy::SkipSpeech).is_err());
        assert!(prepare(&conf_path,&revision,"reading","missing::speaker",DictionaryFailurePolicy::SkipSpeech).is_err());
        assert_eq!(std::fs::read_to_string(&conf_path).unwrap(),conf);
        assert_eq!(std::fs::read_to_string(root.join("graphs/main.flowgraph.toml")).unwrap(),main);
        assert_eq!(std::fs::read_to_string(root.join("graphs/tts.flowgraph.toml")).unwrap(),tts);
        // Simulate interruption after one durable replacement, then inspect and
        // recover through the saved profile, including later-edit protection.
        let graph_root=root.join("graphs");
        let transaction_id={
            let _guard=crate::flowgraph::write_lock::acquire(&graph_root).unwrap();
            let id=crate::flowgraph::source_transaction::prepare(&graph_root,preview.reviewed.changes()).unwrap();
            let (fq,change)=preview.reviewed.changes().iter().next().unwrap();
            std::fs::write(graph_root.join(format!("{fq}.flowgraph.toml")),&change.proposed).unwrap();
            id
        };
        let status=recovery(&conf_path,&revision,None).unwrap();
        assert!(status.pending.unwrap().restorable);
        assert!(recovery(&conf_path,"stale",Some(&transaction_id)).is_err());
        assert!(recovery(&conf_path,&revision,Some("wrong-id")).is_err());
        let (fq,change)=preview.reviewed.changes().iter().next().unwrap();
        let edited=graph_root.join(format!("{fq}.flowgraph.toml"));
        std::fs::write(&edited,"# later edit\n").unwrap();
        let status=recovery(&conf_path,&revision,None).unwrap().pending.unwrap();
        assert!(!status.restorable);assert_eq!(status.conflicting_files,vec![fq.clone()]);
        assert!(recovery(&conf_path,&revision,Some(&transaction_id)).is_err());
        assert_eq!(std::fs::read_to_string(&edited).unwrap(),"# later edit\n");
        std::fs::write(&edited,&change.proposed).unwrap();
        let status=recovery(&conf_path,&revision,Some(&transaction_id)).unwrap();
        assert!(status.restored);assert!(!status.runtime_applied);assert!(status.pending.is_none());
        for (fq,change) in preview.reviewed.changes() {
            assert_eq!(std::fs::read_to_string(graph_root.join(format!("{fq}.flowgraph.toml"))).unwrap(),change.original);
        }
        // No stale review may write. A successful save is durable but does not
        // claim that the separately running Flowgraph has been reloaded.
        assert!(save(&conf_path,&revision,"reading","tts::speaker",DictionaryFailurePolicy::SkipSpeech,
            "stale",&preview.dictionary_revision,false).is_err());
        assert_eq!(std::fs::read_to_string(root.join("graphs/main.flowgraph.toml")).unwrap(),main);
        std::fs::write(&dictionary,"reading\tsurface\n変更\tUN\n").unwrap();
        assert!(save(&conf_path,&revision,"reading","tts::speaker",DictionaryFailurePolicy::SkipSpeech,
            preview.reviewed.revision(),&preview.dictionary_revision,false).is_err());
        std::fs::write(&dictionary,"reading\tsurface\nゆーえぬ\tUN\n").unwrap();
        let saved=save(&conf_path,&revision,"reading","tts::speaker",DictionaryFailurePolicy::SkipSpeech,
            preview.reviewed.revision(),&preview.dictionary_revision,false).unwrap();
        assert!(saved.saved);assert!(!saved.runtime_applied);
        assert_eq!(saved.changed_files.len(),2);
        for (fq,change) in preview.reviewed.changes() {
            assert_eq!(std::fs::read_to_string(root.join(format!("graphs/{fq}.flowgraph.toml"))).unwrap(),change.proposed);
        }
        assert!(crate::flowgraph::load_flowgraph_dir(&root.join("graphs")).is_ok());
        assert_eq!(std::fs::read_to_string(&conf_path).unwrap(),conf);
        let resolved=root.canonicalize().unwrap();
        assert!(resolved.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert!(resolved.file_name().unwrap().to_string_lossy().starts_with("vac-dictionary-profile-plan-"));
        std::fs::remove_dir_all(resolved).unwrap();
    }
}
