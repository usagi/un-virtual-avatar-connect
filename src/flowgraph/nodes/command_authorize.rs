//! Reusable allow-list gate. Identity must come from a trusted ingress, not
//! from text supplied by the sender. This node does not authenticate callers.
use crate::flowgraph::node::{
    get_optional_string, get_required_list, ExecFireSet, InputMap, NodeDescriptor,
    NodeExecError, NodeOutput, NodeSpec, PortSpec, PureEvalHost, PureNode,
};
use crate::flowgraph::socket::{SocketType, SocketValue};
use async_trait::async_trait;

pub struct CommandAuthorizeNode;

impl NodeDescriptor for CommandAuthorizeNode {
    fn describe(&self) -> NodeSpec {
        let strings = SocketType::List(Box::new(SocketType::String));
        NodeSpec {
            feature: "flowgraph.command.authorize".into(),
            title: "Command Authorize".into(),
            category: "command".into(),
            description: Some("信頼できる入力元のlogin/flagsを許可一覧とOR照合。loginは大小文字を区別しない。許可一覧は両方を明示接続し、両方空なら全許可。本文中の名前を認証情報として使わない。".into()),
            inputs: vec![
                PortSpec::exec_input("exec_in", "Exec"),
                PortSpec::input("login", "Sender Login", SocketType::String).with_default(SocketValue::String(String::new())),
                PortSpec::input("flags", "Sender Flags", strings.clone()).with_default(SocketValue::List(vec![])),
                PortSpec::input("allowed_logins", "Allowed Logins", strings.clone()),
                PortSpec::input("allowed_flags", "Allowed Flags", strings),
            ],
            outputs: vec![
                PortSpec::exec_output("on_allowed", "On Allowed"),
                PortSpec::exec_output("on_denied", "On Denied"),
                PortSpec::output("allowed", "Allowed", SocketType::Bool),
            ],
            properties: vec![],
        }
    }
}

fn strings<'a>(inputs: &'a InputMap, key: &str) -> Result<Vec<&'a str>, NodeExecError> {
    get_required_list(inputs, key)?.iter().map(|value| match value {
        SocketValue::String(value) => Ok(value.as_str()),
        _ => Err(NodeExecError::Generic(anyhow::anyhow!("{key} must contain only strings"))),
    }).collect()
}

#[async_trait]
impl PureNode for CommandAuthorizeNode {
    async fn compute(&self, _host: &PureEvalHost, _props: &InputMap, inputs: &InputMap,
        fired: &ExecFireSet) -> Result<NodeOutput, NodeExecError> {
        if !fired.contains("exec_in") { return Ok(NodeOutput::new()); }
        let login = get_optional_string(inputs, "login", "")?.to_lowercase();
        let flags = if inputs.contains_key("flags") { strings(inputs, "flags")? } else { vec![] };
        let logins = strings(inputs, "allowed_logins")?;
        let allowed_flags = strings(inputs, "allowed_flags")?;
        let allowed = (logins.is_empty() && allowed_flags.is_empty())
            || (!login.is_empty() && logins.iter().any(|candidate| candidate.to_lowercase() == login))
            || flags.iter().any(|flag| !flag.is_empty() && allowed_flags.contains(flag));
        Ok(NodeOutput::new().set_data("allowed", SocketValue::Bool(allowed))
            .fire_exec(if allowed { "on_allowed" } else { "on_denied" }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn list(values: &[&str]) -> SocketValue {
        SocketValue::List(values.iter().map(|v| SocketValue::String((*v).into())).collect())
    }
    fn fired() -> ExecFireSet {
        let mut value = ExecFireSet::new();
        value.insert("exec_in");
        value
    }
    #[tokio::test]
    async fn denied_sender_never_reaches_downstream_file_write() {
        let root = std::env::temp_dir().join(format!("vac-authorize-{}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("main.flowgraph.toml"), r#"
[[nodes]]
id="in"
feature="flowgraph.ingress.web_input"
[[nodes]]
id="gate"
feature="flowgraph.command.authorize"
[[nodes]]
id="logins"
feature="flowgraph.literal.strings"
properties.value=["ALICE"]
[[nodes]]
id="flags"
feature="flowgraph.literal.strings"
properties.value=[]
[[nodes]]
id="path"
feature="flowgraph.literal.string"
properties.value="not-actually-written.txt"
[[nodes]]
id="write"
feature="flowgraph.file.write_string"
[[edges]]
from="in:exec_out"
to="gate:exec_in"
[[edges]]
from="in:source_actor"
to="gate:login"
[[edges]]
from="logins:value"
to="gate:allowed_logins"
[[edges]]
from="flags:value"
to="gate:allowed_flags"
[[edges]]
from="gate:on_allowed"
to="write:exec_in"
[[edges]]
from="path:value"
to="write:path"
[[edges]]
from="in:content"
to="write:text"
"#).unwrap();
        for (actor, effects) in [("alice", 1), ("mallory", 0)] {
            std::fs::write(root.join("main.flowgraph.test.toml"), format!(r#"
[[triggers]]
node="main::in"
exec=["__trigger__"]
[[triggers.overrides]]
port="__source_actor__"
ty="string"
value="{actor}"
[[triggers.overrides]]
port="__content__"
ty="string"
value="alice: learn(X=Y)"
[[mocks.file_write]]
node="main::write"
[[tests]]
name="Only trusted identity grants write branch"
[tests.expect]
effect_count={effects}
"#)).unwrap();
            let result = crate::flowgraph::fixture_runner::run_fixture_once_report(&root).await.unwrap();
            assert!(result.ok, "{:?}", result.tests);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn login_or_flag_gate_preserves_policy_without_treating_missing_identity_as_allowed() {
        for (login, flags, logins, allowed_flags, expected) in [
            ("alice", vec![], vec!["ALICE"], vec![], true),
            ("mallory", vec!["moderator"], vec!["alice"], vec!["moderator"], true),
            ("mallory", vec!["Moderator"], vec!["alice"], vec!["moderator"], false),
            ("", vec![], vec!["alice"], vec![], false),
            ("", vec![], vec![""], vec![], false),
            ("", vec![""], vec![], vec![""], false),
            ("", vec![], vec![], vec![], true),
        ] {
            let inputs = InputMap::from([
                ("login".into(), SocketValue::String(login.into())),
                ("flags".into(), list(&flags)),
                ("allowed_logins".into(), list(&logins)),
                ("allowed_flags".into(), list(&allowed_flags)),
            ]);
            let output = CommandAuthorizeNode.compute(&PureEvalHost::default(), &InputMap::new(),
                &inputs, &fired()).await.unwrap();
            assert_eq!(output.data["allowed"], SocketValue::Bool(expected));
            assert!(output.fired_exec.contains(if expected { "on_allowed" } else { "on_denied" }));
            assert_eq!(output.fired_exec.len(), 1);
        }
    }
    #[tokio::test]
    async fn missing_or_malformed_policy_does_not_become_unrestricted() {
        let fired = fired();
        for inputs in [
            InputMap::new(),
            InputMap::from([("allowed_logins".into(), list(&[])),
                ("allowed_flags".into(), SocketValue::List(vec![SocketValue::Int(1)]))]),
        ] {
            assert!(CommandAuthorizeNode.compute(&PureEvalHost::default(), &InputMap::new(), &inputs, &fired).await.is_err());
        }
        let output = CommandAuthorizeNode.compute(&PureEvalHost::default(), &InputMap::new(),
            &InputMap::new(), &ExecFireSet::new()).await.unwrap();
        assert!(output.fired_exec.is_empty());
    }
}
