//! Explicit input for callers authorized by the existing Control API policy.
//! Sender and flags are caller-supplied context, not authentication credentials.
use crate::flowgraph::node::{
	ExecFireSet, InputMap, NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec, PureEvalHost, PureNode,
};
use crate::flowgraph::socket::{SocketType, SocketValue};
use async_trait::async_trait;

pub struct ControlInputNode;

impl NodeDescriptor for ControlInputNode {
	fn describe(&self) -> NodeSpec {
		let strings = SocketType::List(Box::new(SocketType::String));
		NodeSpec {
			feature: "flowgraph.ingress.control".into(),
			title: "Control Input".into(),
			category: "ingress".into(),
			description: Some("Control APIでこのノードを明示指定して投入。本文・送信者・フラグを後続へ渡す。送信者とフラグはAPI呼出者が提供する情報であり、このノードは本人確認を行わない。".into()),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Receive"),
				PortSpec::input("content", "Content", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("source_actor", "Sender", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("flags", "Flags", strings.clone()).with_default(SocketValue::List(vec![])),
				PortSpec::input("meta", "Metadata", SocketType::Json).with_default(SocketValue::Json(serde_json::json!({}))),
			],
			outputs: vec![
				PortSpec::exec_output("exec_out", "Received"),
				PortSpec::output("content", "Content", SocketType::String),
				PortSpec::output("source_actor", "Sender", SocketType::String),
				PortSpec::output("flags", "Flags", strings),
				PortSpec::output("meta", "Metadata", SocketType::Json),
			],
			properties: vec![],
		}
	}

	fn control_triggerable(&self) -> bool { true }
}

#[async_trait]
impl PureNode for ControlInputNode {
	async fn compute(&self, _: &PureEvalHost, _: &InputMap, inputs: &InputMap, fired: &ExecFireSet) -> Result<NodeOutput, NodeExecError> {
		if !fired.contains("exec_in") { return Ok(NodeOutput::new()); }
		let mut output = NodeOutput::new().fire_exec("exec_out");
		for name in ["content", "source_actor", "flags", "meta"] {
			let value = inputs.get(name).cloned().ok_or_else(|| NodeExecError::MissingRequiredInput(name.into()))?;
			output = output.set_data(name, value);
		}
		Ok(output)
	}
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn dictionary_control_template_preserves_rows_across_reloads() {
        let root = std::env::temp_dir().join(format!("vac-dictionary-control-{}", rand::random::<u64>()));
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("main.flowgraph.toml"), include_str!("../../../flowgraph.example/dictionary-control-update/main.flowgraph.toml")).unwrap();
        let dictionary = root.join("assets/user.tsv");
        std::fs::write(&dictionary, include_str!("../../../flowgraph.example/dictionary-control-update/assets/user.tsv")).unwrap();
        for (action, word, reading, rows) in [("learn", "alpha", "あるふぁ", 1), ("learn", "beta", "べーた", 2), ("learn", "beta", "べーた", 2), ("forget", "alpha", "あるふぁ", 1)] {
            std::fs::write(root.join("main.flowgraph.test.toml"), format!(r#"
[[triggers]]
node="main::{action}_request"
exec=["exec_in"]
[[triggers.overrides]]
port="meta"
ty="json"
value={{surface="{word}",reading="{reading}",part_of_speech="名詞"}}
[[tests]]
name="Dictionary control persistence"
[tests.expect]
trigger_count=1
"#)).unwrap();
            let report = crate::flowgraph::fixture_runner::run_fixture_once_report(&root).await.unwrap();
            assert!(report.ok, "{:?}", report.tests);
            let text = std::fs::read_to_string(&dictionary).unwrap();
            let table = crate::flowgraph::nodes::table_ops::parse_tsv_with_mode(&text, "auto").unwrap();
            assert_eq!(table.len(), rows, "{action}: {text}");
        }
        let text = std::fs::read_to_string(&dictionary).unwrap();
        assert!(text.contains("べーた\tbeta"));
        assert!(!text.contains("あるふぁ\talpha"));
        let resolved = std::fs::canonicalize(&root).unwrap();
        assert!(resolved.starts_with(std::fs::canonicalize(std::env::temp_dir()).unwrap()));
        assert!(resolved.file_name().unwrap().to_string_lossy().starts_with("vac-dictionary-control-"));
        std::fs::remove_dir_all(resolved).unwrap();
    }

    #[tokio::test]
    async fn explicit_control_flags_gate_real_dictionary_updates() {
        let root = std::env::temp_dir().join(format!("vac-control-flags-{}", rand::random::<u64>()));
        std::fs::create_dir_all(root.join("assets")).unwrap();
        let graph = include_str!("../../../flowgraph.example/dictionary-managed-command/main.flowgraph.toml")
            .replace("flowgraph.ingress.web_input", "flowgraph.ingress.control")
            .replace("properties.value=[]", "properties.value=[\"dictionary.write\"]")
            .replace("properties.value=[\"example_moderator\"]", "properties.value=[]");
        std::fs::write(root.join("main.flowgraph.toml"), format!("{graph}\n[[edges]]\nfrom=\"in:flags\"\nto=\"gate:flags\"\n")).unwrap();
        std::fs::write(root.join("assets/commands.tsv"), include_str!("../../../flowgraph.example/dictionary-managed-command/assets/commands.tsv")).unwrap();
        let dictionary = root.join("assets/user.tsv");
        std::fs::write(&dictionary, include_str!("../../../flowgraph.example/dictionary-managed-command/assets/user.tsv")).unwrap();
        for (flags, command, changed) in [("[]", "learn(X=dictionary.write)", false), ("[\"dictionary.write\"]", "learn(X=正)", true), ("[]", "learn(Y=偽)", false)] {
            let before = std::fs::read(&dictionary).unwrap();
            std::fs::write(root.join("main.flowgraph.test.toml"), format!(r#"
[[triggers]]
node="main::in"
exec=["exec_in"]
[[triggers.overrides]]
port="content"
ty="string"
value="{command}"
[[triggers.overrides]]
port="flags"
ty="list<string>"
value={flags}
[[tests]]
name="Explicit control flags"
[tests.expect]
trigger_count=1
"#)).unwrap();
            let result = crate::flowgraph::fixture_runner::run_fixture_once_report(&root).await.unwrap();
            assert!(result.ok, "{:?}", result.tests);
            assert_eq!(std::fs::read(&dictionary).unwrap() != before, changed);
        }
        let text = std::fs::read_to_string(dictionary).unwrap();
        assert!(text.contains("正\tX"));
        assert!(!text.contains("偽"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
