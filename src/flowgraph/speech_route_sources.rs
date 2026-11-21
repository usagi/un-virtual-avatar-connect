//! Source-level input redirection for a reviewed dictionary insertion.
//! This prepares text only; publication and insertion of the destination nodes
//! belong to the caller's revision-checked, validated graph transaction.
use super::loader::file::parse_flowgraph_file;
use super::loader::reference::{parse_port_ref, resolve_fq_ref, ResolveContext};
use std::collections::{BTreeMap, HashSet};

/// Unlike the tolerant runtime directory walker, a write preview must fail if
/// any included directory cannot be read. Otherwise it could certify a partial
/// source set and miss connections in that directory. Caller holds root lock.
pub fn read_complete_sources(root:&std::path::Path)->Result<BTreeMap<String,String>,String> {
    let root=root.canonicalize().map_err(|e|e.to_string())?;
    let mut pending=vec![root.clone()];let mut visited=HashSet::new();let mut sources=BTreeMap::new();
    while let Some(directory)=pending.pop() {
        let canonical=directory.canonicalize().map_err(|e|e.to_string())?;
        if !canonical.starts_with(&root) {return Err("graph directory resolves outside selected root".into());}
        if !visited.insert(canonical) {return Err("graph directory alias or cycle requires explicit editing".into());}
        for entry in std::fs::read_dir(&directory).map_err(|e|e.to_string())? {
            let entry=entry.map_err(|e|e.to_string())?;let path=entry.path();
            let name=entry.file_name().into_string().map_err(|_|"graph entry name is not UTF-8")?;
            let kind=entry.file_type().map_err(|e|e.to_string())?;
            if kind.is_dir() {
                if !name.starts_with('.')&&!name.starts_with('_') {pending.push(path);}
            } else if super::loader::is_flowgraph_file(&path) {
                let canonical=path.canonicalize().map_err(|e|e.to_string())?;
                if !canonical.starts_with(&root) {return Err("graph file resolves outside selected root".into());}
                let fq=super::loader::fq_path_of_file(&root,&path)?;
                let content=std::fs::read_to_string(&path).map_err(|e|e.to_string())?;
                sources.insert(fq,content);
            }
        }
    }
    Ok(sources)
}

#[derive(Debug, serde::Serialize)]
pub struct RedirectedSource {
    pub original: String,
    pub proposed: String,
    pub edge_inputs: usize,
    pub boundary_inputs: usize,
}

fn replace_endpoint(slot: &mut toml_edit::Item, next: String) {
    let mut value = toml_edit::Value::from(next);
    if let Some(previous) = slot.as_value() {
        *value.decor_mut() = previous.decor().clone();
    }
    *slot = toml_edit::Item::Value(value);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all="snake_case")]
pub enum DictionaryFailurePolicy { SkipSpeech, SpeakOriginal }

/// Values must be obtained from the selected server-side profile and canonical
/// graph root, not accepted as authority from client-supplied paths.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct InsertionContext {
    pub profile_source: String,
    pub profile_revision: String,
    pub graph_root: String,
    pub dictionary_key: String,
    pub dictionary_path: String,
    pub target_graph: String,
    pub target_node: String,
    pub failure: DictionaryFailurePolicy,
}

pub struct ReviewedInsertion {
    context: InsertionContext,
    sources: BTreeMap<String,String>,
    changes: BTreeMap<String,RedirectedSource>,
    revision: String,
}

impl ReviewedInsertion {
    pub fn prepare(context:InsertionContext,sources:BTreeMap<String,String>)->Result<Self,String> {
        let changes=prepare_dictionary_insertion(&sources,&context.target_graph,&context.target_node,&context.dictionary_path,context.failure)?;
        let encoded=serde_json::to_vec(&(&context,&sources,&changes)).map_err(|e|e.to_string())?;
        let revision=blake3::hash(&encoded).to_hex().to_string();
        Ok(Self{context,sources,changes,revision})
    }
    pub fn revision(&self)->&str {&self.revision}
    pub fn context(&self)->&InsertionContext {&self.context}
    pub fn changes(&self)->&BTreeMap<String,RedirectedSource> {&self.changes}

    /// Call under the shared graph-root write lock after rereading the selected
    /// profile and complete source set. This does not acquire locks or publish.
    pub fn verify_current(&self,review_revision:&str,context:&InsertionContext,sources:&BTreeMap<String,String>)->Result<(),String> {
        if review_revision!=self.revision {return Err("reviewed insertion changed".into());}
        if context!=&self.context {return Err("profile, dictionary, destination or failure policy changed".into());}
        if sources!=&self.sources {return Err("graph source set changed after review".into());}
        Ok(())
    }
}

/// Prepare an entire insertion, preserving the existing speech node identity
/// and its voice/engine/output connections. No files or running state change.
pub fn prepare_dictionary_insertion(
    sources: &BTreeMap<String, String>, target_graph: &str, target_node: &str,
    dictionary_path: &str, failure: DictionaryFailurePolicy,
) -> Result<BTreeMap<String, RedirectedSource>, String> {
    if dictionary_path.trim().is_empty() { return Err("dictionary path is empty".into()); }
    let original = sources.get(target_graph).ok_or("target source missing")?;
    let parsed = parse_flowgraph_file(original,None).map_err(|e|format!("{e:?}"))?;
    let mut used: HashSet<String> = parsed.nodes.iter().map(|n|n.id.clone())
        .chain(parsed.instances.iter().map(|n|n.id.clone())).collect();
    let mut allocate = |base: &str| {
        let mut name=base.to_string();let mut index=1;
        while !used.insert(name.clone()) {name=format!("{base}_{index}");index+=1;}
        name
    };
    let load=allocate("reading_load");let replace=allocate("reading_replace");
    let path=allocate("reading_path");let error=allocate("reading_error");
    let replacements=BTreeMap::from([
        ("text".into(),(replace.clone(),"content".into())),
        ("exec_in".into(),(load.clone(),"exec_in".into())),
    ]);
    for port in ["text","exec_in"] {
        let one=BTreeMap::from([(port.to_string(),replacements[port].clone())]);
        if redirect_inputs(sources,target_graph,target_node,&one)?.is_empty() {
            return Err(format!("speech {port} has no source connection; direct invocation requires a separate plan"));
        }
    }
    let mut plan=redirect_inputs(sources,target_graph,target_node,&replacements)?;
    let source=plan.get(target_graph).map(|p|p.proposed.as_str()).unwrap_or(original);
    let mut doc=source.parse::<toml_edit::DocumentMut>().map_err(|e|e.to_string())?;
    fn append(doc:&mut toml_edit::DocumentMut,key:&str,table:toml_edit::Table)->Result<(),String>{
        if !doc.contains_key(key) {doc[key]=toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());}
        doc[key].as_array_of_tables_mut().ok_or_else(||format!("{key} must use array-of-tables syntax"))?.push(table);Ok(())
    }
    for (id,feature) in [(&load,"flowgraph.table.load_tsv"),(&replace,"flowgraph.glossary.replace"),(&path,"flowgraph.literal.string"),(&error,"flowgraph.util.log")] {
        let mut node=toml_edit::Table::new();node["id"]=toml_edit::value(id);node["feature"]=toml_edit::value(feature);
        if id==&path {node["properties"]=toml_edit::Item::Table(toml_edit::Table::new());node["properties"]["value"]=toml_edit::value(dictionary_path);}
        append(&mut doc,"nodes",node)?;
    }
    let mut edges=vec![
        (format!("{path}:value"),format!("{load}:path")),
        (format!("{load}:table"),format!("{replace}:dictionary")),
        (format!("{replace}:result"),format!("{target_node}:text")),
        (format!("{load}:on_success"),format!("{target_node}:exec_in")),
        (format!("{load}:on_error"),format!("{error}:exec_in")),
        (format!("{load}:error"),format!("{error}:value")),
    ];
    // load_tsv emits an empty table on failure; glossary.replace then retains
    // the input text. Both policies report the error through the log node.
    if matches!(failure,DictionaryFailurePolicy::SpeakOriginal) {
        edges.push((format!("{load}:on_error"),format!("{target_node}:exec_in")));
    }
    for (from,to) in edges {let mut edge=toml_edit::Table::new();edge["from"]=toml_edit::value(from);edge["to"]=toml_edit::value(to);append(&mut doc,"edges",edge)?;}
    let entry=plan.entry(target_graph.into()).or_insert_with(||RedirectedSource{original:original.clone(),proposed:String::new(),edge_inputs:0,boundary_inputs:0});
    entry.proposed=doc.to_string();
    Ok(plan)
}

pub fn redirect_inputs(
    sources: &BTreeMap<String, String>,
    target_graph: &str,
    target_node: &str,
    replacements: &BTreeMap<String, (String, String)>,
) -> Result<BTreeMap<String, RedirectedSource>, String> {
    let target = sources.get(target_graph).ok_or("target source missing")?;
    let parsed = parse_flowgraph_file(target, None).map_err(|e| format!("{e:?}"))?;
    if !parsed.nodes.iter().any(|n| n.id == target_node && n.feature == "flowgraph.tts.speak") {
        return Err("target is not a speech node in the selected source".into());
    }
    let known: HashSet<String> = sources.keys().cloned().collect();
    let mut result = BTreeMap::new();
    for (fq, original) in sources {
        let parsed = parse_flowgraph_file(original, None).map_err(|e| format!("{e:?}"))?;
        let context = ResolveContext { current_file_fq: fq.clone(), known_file_fqs: known.clone() };
        let redirect = |raw: &str| -> Result<Option<String>, String> {
            let endpoint = parse_port_ref(raw)?;
            if endpoint.node_id != target_node || resolve_fq_ref(&endpoint, &context)? != target_graph {
                return Ok(None);
            }
            let Some((node, port)) = replacements.get(&endpoint.port) else { return Ok(None); };
            // Preserve the original file-reference spelling, including relative
            // paths and directory/main aliases; only the endpoint changes.
            let prefix = endpoint.fq_path.map(|p| format!("{p}::")).unwrap_or_default();
            Ok(Some(format!("{prefix}{node}:{port}")))
        };
        let mut document = original.parse::<toml_edit::DocumentMut>().map_err(|e| e.to_string())?;
        let mut edge_inputs = 0;
        let mut boundary_inputs = 0;
        for (index, edge) in parsed.edges.iter().enumerate() {
            if let Some(next) = redirect(&edge.to)? {
                replace_endpoint(&mut document["edges"][index]["to"], next);
                edge_inputs += 1;
            }
        }
        for (index, input) in parsed.graph.inputs.iter().enumerate() {
            if let Some(next) = redirect(&input.to)? {
                replace_endpoint(&mut document["graph"]["inputs"][index]["to"], next);
                boundary_inputs += 1;
            }
        }
        if edge_inputs + boundary_inputs > 0 {
            result.insert(fq.clone(), RedirectedSource { original: original.clone(), proposed: document.to_string(), edge_inputs, boundary_inputs });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn insertion_source_snapshot_rejects_unreadable_content_instead_of_skipping_it() {
        let root=std::env::temp_dir().join(format!("vac-insertion-sources-{}",rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("nested")).unwrap();
        std::fs::create_dir(root.join("_excluded")).unwrap();
        std::fs::write(root.join("main.flowgraph.toml"),"# main\r\n").unwrap();
        std::fs::write(root.join("nested/voice.flowgraph.toml"),"# voice\n").unwrap();
        std::fs::write(root.join("_excluded/bad.flowgraph.toml"),[0xff]).unwrap();
        let snapshot=read_complete_sources(&root).unwrap();
        assert_eq!(snapshot.len(),2);assert_eq!(snapshot["main"],"# main\r\n");
        assert_eq!(snapshot["nested/voice"],"# voice\n");
        std::fs::write(root.join("nested/voice.flowgraph.toml"),[0xff]).unwrap();
        assert!(read_complete_sources(&root).is_err());
        let resolved=root.canonicalize().unwrap();
        assert!(resolved.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert!(resolved.file_name().unwrap().to_string_lossy().starts_with("vac-insertion-sources-"));
        std::fs::remove_dir_all(resolved).unwrap();
    }
    #[test]
    fn reviewed_dictionary_insertion_binds_target_and_complete_source_set() {
        let sources=BTreeMap::from([
            ("main".into(),include_str!("../../flowgraph.example/chat-echo/main.flowgraph.toml").to_string()),
            ("tts".into(),include_str!("../../flowgraph.example/chat-echo/tts.flowgraph.toml").to_string()),
            ("unrelated".into(),"# another purpose\n".into()),
        ]);
        let context=InsertionContext{profile_source:"C:/isolated/a/conf.toml".into(),profile_revision:"v1".into(),graph_root:"C:/isolated/a/graphs".into(),dictionary_key:"reading".into(),dictionary_path:"C:/isolated/a/reading.tsv".into(),target_graph:"tts".into(),target_node:"speaker".into(),failure:DictionaryFailurePolicy::SkipSpeech};
        let plan=ReviewedInsertion::prepare(context.clone(),sources.clone()).unwrap();
        assert_eq!(plan.changes().len(),2);
        plan.verify_current(plan.revision(),&context,&sources).unwrap();
        assert!(plan.verify_current("obsolete",&context,&sources).is_err());
        for change in 0..4 {
            let mut changed=context.clone();
            match change {0=>changed.profile_source="C:/isolated/b/conf.toml".into(),1=>changed.graph_root="C:/isolated/b/graphs".into(),2=>changed.dictionary_path="C:/shared/reading.tsv".into(),_=>changed.failure=DictionaryFailurePolicy::SpeakOriginal}
            assert!(plan.verify_current(plan.revision(),&changed,&sources).is_err());
        }
        for change in 0..3 {
            let mut changed=sources.clone();
            match change {0=>{changed.insert("unrelated".into(),"# edited elsewhere\n".into());},1=>{changed.remove("unrelated");},_=>{changed.insert("added".into(),"# newly added\n".into());}}
            assert!(plan.verify_current(plan.revision(),&context,&changed).is_err());
        }
        let again=ReviewedInsertion::prepare(context.clone(),sources.clone()).unwrap();
        assert_eq!(again.revision(),plan.revision());
        let mut other=context;other.failure=DictionaryFailurePolicy::SpeakOriginal;
        assert_ne!(ReviewedInsertion::prepare(other,sources).unwrap().revision(),plan.revision());
    }
    #[tokio::test]
    async fn dictionary_insertion_executes_success_and_explicit_failure_policies() {
        // Requiring a speech floor without supplying shared runtime state stops
        // before driver lookup. Text evaluation is real; synthesis is not.
        let sources=BTreeMap::from([
            ("main".into(),include_str!("../../flowgraph.example/chat-echo/main.flowgraph.toml").replace("flowgraph.ingress.twitch","flowgraph.ingress.web_input")),
            ("tts".into(),format!("{}\n[[nodes]]\nid='test_floor'\nfeature='flowgraph.literal.string'\nproperties.value='isolated_test'\n[[edges]]\nfrom='test_floor:value'\nto='speaker:respect_speech_floor'\n",include_str!("../../flowgraph.example/chat-echo/tts.flowgraph.toml"))),
        ]);
        for exists in [true,false] {
            for policy in [DictionaryFailurePolicy::SkipSpeech,DictionaryFailurePolicy::SpeakOriginal] {
                let root=std::env::temp_dir().join(format!("vac-dictionary-execution-{}",rand::random::<u64>()));
                std::fs::create_dir(&root).unwrap();
                if exists {std::fs::write(root.join("reading.tsv"),"reading\tsurface\nゆーえぬ\tUN\n").unwrap();}
                let plan=prepare_dictionary_insertion(&sources,"tts","speaker","graph-file:reading.tsv",policy).unwrap();
                for(fq,change)in &plan {std::fs::write(root.join(format!("{fq}.flowgraph.toml")),&change.proposed).unwrap();}
                let mut fixture=String::new();
                for text in ["UN first","UN second"] {
                    fixture.push_str(&format!("[[triggers]]\nnode='main::in'\nexec=['__trigger__']\n[[triggers.overrides]]\nport='__content__'\nty='string'\nvalue='{text}'\n"));
                }
                fixture.push_str("[[tests]]\nname='dictionary execution'\n[tests.expect]\ntrigger_count=2\n");
                std::fs::write(root.join("main.flowgraph.test.toml"),fixture).unwrap();
                let report=super::super::fixture_runner::run_fixture_once_report(&root).await.unwrap();
                assert!(report.ok);
                let count=|node:&str|report.exec_count.iter().find(|(id,_)|id==node).map(|(_,count)|*count).unwrap_or(0);
                let should_speak=exists || matches!(policy,DictionaryFailurePolicy::SpeakOriginal);
                assert_eq!(count("tts::speaker"),if should_speak{2}else{0});
                assert_eq!(count("tts::reading_error"),if exists{0}else{2});
                if should_speak {
                    let text=report.stored_values.iter().find(|value|value.node=="tts::reading_replace" && value.port=="result").unwrap();
                    assert_eq!(text.value,serde_json::json!(if exists{"ゆーえぬ second"}else{"UN second"}));
                    let error=report.stored_values.iter().find(|value|value.node=="tts::speaker" && value.port=="error").unwrap();
                    assert_eq!(error.value.as_str().unwrap(),"respect_speech_floor requires shared runtime state");
                }
                let resolved=root.canonicalize().unwrap();
                assert!(resolved.starts_with(std::env::temp_dir().canonicalize().unwrap()));
                assert!(resolved.file_name().unwrap().to_string_lossy().starts_with("vac-dictionary-execution-"));
                std::fs::remove_dir_all(resolved).unwrap();
            }
        }
    }
    #[test]
    fn dictionary_insertion_keeps_existing_speaker_and_validates_both_policies() {
        let sources=BTreeMap::from([
            ("main".into(),include_str!("../../flowgraph.example/chat-echo/main.flowgraph.toml").to_string()),
            ("tts".into(),include_str!("../../flowgraph.example/chat-echo/tts.flowgraph.toml").to_string()),
        ]);
        for policy in [DictionaryFailurePolicy::SkipSpeech,DictionaryFailurePolicy::SpeakOriginal] {
            let plan=prepare_dictionary_insertion(&sources,"tts","speaker","C:/isolated/reading.tsv",policy).unwrap();
            assert_eq!(plan.len(),2);
            let root=std::env::temp_dir().join(format!("vac-dictionary-insertion-{}",rand::random::<u64>()));
            std::fs::create_dir(&root).unwrap();
            for (fq,change) in &plan {std::fs::write(root.join(format!("{fq}.flowgraph.toml")),&change.proposed).unwrap();}
            let report=super::super::load_flowgraph_dir(&root).unwrap();
            let speech=report.program.incoming_connections("tts::speaker").unwrap();
            assert!(speech.iter().any(|e|e.to_port=="engine" && e.from_node=="tts::engine_literal"));
            assert!(speech.iter().any(|e|e.to_port=="text" && e.from_node=="tts::reading_replace"));
            assert_eq!(speech.iter().filter(|e|e.exec).count(),if matches!(policy,DictionaryFailurePolicy::SpeakOriginal){2}else{1});
            let resolved=root.canonicalize().unwrap();
            assert!(resolved.starts_with(std::env::temp_dir().canonicalize().unwrap()));
            assert!(resolved.file_name().unwrap().to_string_lossy().starts_with("vac-dictionary-insertion-"));
            std::fs::remove_dir_all(resolved).unwrap();
        }
    }
    #[test]
    fn speech_source_redirection_preserves_boundaries_and_unrelated_inputs() {
        let voice = "# keep this explanation\n[graph]\nkind='module'\n[[graph.inputs]]\nname='run'\ntype='exec'\nto='speaker:exec_in'\n[[nodes]]\nid='speaker'\nfeature='flowgraph.tts.speak'\n[[edges]]\nfrom='engine:value'\nto='speaker:engine'\n";
        let caller = "[[edges]]\nfrom='in:text'\nto='../voice::speaker:text'\n[[edges]]\nfrom='in:exec'\nto='../voice::speaker:exec_in'\n[[edges]]\nfrom='speaker:exec_out'\nto='other:exec_in'\n";
        let unrelated = "# same local ID in a different graph\n[[edges]]\nfrom='in:text'\nto='speaker:text'\n";
        let sources = BTreeMap::from([("voice/main".into(),voice.into()),("inputs/main".into(),caller.into()),("other".into(),unrelated.into())]);
        let replacements = BTreeMap::from([("text".into(),("dictionary".into(),"text".into())),("exec_in".into(),("dictionary".into(),"run".into()))]);
        let plan = redirect_inputs(&sources,"voice/main","speaker",&replacements).unwrap();
        assert_eq!(plan.len(),2);
        assert_eq!(plan["voice/main"].boundary_inputs,1);
        assert_eq!(plan["inputs/main"].edge_inputs,2);
        assert!(plan["voice/main"].proposed.contains("# keep this explanation"));
        assert!(plan["voice/main"].proposed.contains("to='speaker:engine'"));
        assert!(plan["inputs/main"].proposed.contains("../voice::dictionary:text"));
        assert!(plan["inputs/main"].proposed.contains("from='speaker:exec_out'"));
        assert_eq!(sources["other"],unrelated);
        assert_eq!(plan["voice/main"].original,voice);
        assert!(redirect_inputs(&sources,"other","speaker",&replacements).is_err());
    }
}
