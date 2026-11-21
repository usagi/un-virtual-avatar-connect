//! Read-only speech destinations from a successfully loaded program.
//! No inference of event pairing, current mode activation, or safe rewrites.
use super::engine::ResolvedInputConnection;
use super::loader::LoadReport;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct SpeechRoute {
    pub runtime_node: String,
    pub source_file: PathBuf,
    pub source_graph: Option<String>,
    pub instance_path: Option<String>,
    /// Editing one module definition may affect each of these runtime copies.
    pub definition_instances: usize,
    pub incoming: Vec<ResolvedInputConnection>,
}

pub fn inspect(report: &LoadReport) -> Vec<SpeechRoute> {
    let provenance=report.composition.runtime_provenance();
    let mut routes=Vec::new();
    for (node,meta) in &report.node_meta {
        if meta.feature!="flowgraph.tts.speak" {continue;}
        let Some((runtime_graph,local_node))=node.split_once("::") else {continue;};
        let origin=provenance.get(runtime_graph);
        let definition_instances=report.node_meta.iter().filter(|(other,other_meta)| {
            other_meta.feature==meta.feature && other_meta.file==meta.file
                && other.split_once("::").is_some_and(|(_,id)|id==local_node)
        }).count();
        routes.push(SpeechRoute {
            runtime_node:node.clone(),source_file:meta.file.clone(),
            source_graph:origin.map(|(source,_)|source.clone()),
            instance_path:origin.map(|(_,instance)|instance.clone()),
            definition_instances,
            incoming:report.program.incoming_connections(node).expect("loaded metadata node belongs to program"),
        });
    }
    routes.sort_by(|a,b|a.runtime_node.cmp(&b.runtime_node));
    routes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_routes_preserve_shared_module_instances() {
        let root=std::env::temp_dir().join(format!("vac-speech-route-inspection-{}",rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("voice.flowgraph.toml"),r#"
[graph]
kind="module"
[[graph.inputs]]
name="run"
type="exec"
to="speaker:exec_in"
[[graph.inputs]]
name="text"
type="string"
to="speaker:text"
[[nodes]]
id="speaker"
feature="flowgraph.tts.speak"
[[nodes]]
id="engine"
feature="flowgraph.literal.string"
properties.value="voicevox"
[[edges]]
from="engine:value"
to="speaker:engine"
"#).unwrap();
        std::fs::write(root.join("main.flowgraph.toml"),r#"
[[nodes]]
id="input"
feature="flowgraph.ingress.web_input"
[[instances]]
id="left"
graph="voice"
[[instances]]
id="right"
graph="voice"
[[edges]]
from="input:exec_out"
to="left:run"
[[edges]]
from="input:content"
to="left:text"
[[edges]]
from="input:exec_out"
to="right:run"
[[edges]]
from="input:content"
to="right:text"
"#).unwrap();
        let loaded=super::super::load_flowgraph_dir(&root).unwrap();
        let routes=inspect(&loaded);
        assert_eq!(routes.len(),2);
        for (route,name) in routes.iter().zip(["left","right"]) {
            assert_eq!(route.runtime_node,format!("main/@{name}::speaker"));
            assert_eq!(route.source_graph.as_deref(),Some("voice"));
            assert_eq!(route.instance_path.as_deref(),Some(name));
            assert_eq!(route.definition_instances,2);
            assert_eq!(route.incoming.iter().filter(|p|p.to_port=="text" && p.from_node=="main::input").count(),1);
            assert_eq!(route.incoming.iter().filter(|p|p.exec && p.from_node=="main::input").count(),1);
            assert!(route.incoming.iter().any(|p|p.to_port=="engine" && p.from_node==format!("main/@{name}::engine")));
        }
        let (runtime,program)=super::super::runtime::FlowgraphRuntime::load_program(&root);
        assert!(runtime.ok);assert!(program.is_some());
        assert_eq!(runtime.speech_routes.len(),2);
        let loaded_digest=runtime.source_digests["main"].clone();
        std::fs::write(root.join("main.flowgraph.toml"),"# saved edit, not applied to existing runtime\n").unwrap();
        assert_eq!(runtime.clone().speech_routes.len(),2);
        assert_eq!(runtime.source_digests["main"],loaded_digest);
        let (fresh,_)=super::super::runtime::FlowgraphRuntime::load_program(&root);
        assert!(fresh.ok);assert!(fresh.speech_routes.is_empty());
        assert_ne!(fresh.source_digests["main"],loaded_digest);
        let resolved=root.canonicalize().unwrap();
        assert!(resolved.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert!(resolved.file_name().unwrap().to_string_lossy().starts_with("vac-speech-route-inspection-"));
        std::fs::remove_dir_all(resolved).unwrap();
    }
}
