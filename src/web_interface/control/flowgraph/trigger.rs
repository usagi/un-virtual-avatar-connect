//! `POST /flowgraph/{instance_id}/trigger/{node_id}` — Control からの安全なノード 1-shot 発火（Phase φ-2）。

use actix_web::web::{self, Data, Json};
use actix_web::{post, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::flowgraph::node::{json_to_socket_value, InputMap, PortDirection, PortSpec, TriggerEvent};
use crate::flowgraph::registry;
use crate::SharedState;

use super::util::err_json;

/// リクエスト: `{"inputs": {"source": ..., ...}, "exec_port": "exec_in"?}`
///
/// `inputs` の value は以下いずれかを受け付ける:
///   - 生 JSON: `"ドクターウサギ"` / `42` / `true` / `["a","b"]`
///   - 型注釈版: `{"type":"string","value":"ドクターウサギ"}` — `type` は診断用で実際には
///     socket 側の `PortSpec.ty` が正とする。`value` だけを抽出して生 JSON 版と同じ経路に流す。
#[derive(Debug, Deserialize)]
pub struct TriggerNodeRequest {
	/// Selected exec outputs to observe for this request only (maximum 16).
	#[serde(default)]
	pub observe: Vec<(String, String)>,
	#[serde(default)]
	pub wait_ms: u64,
	#[serde(default)]
	pub expected_root: Option<String>,
	#[serde(default)]
	pub expected_source_digest: Option<String>,
	#[serde(default)]
	pub inputs: serde_json::Map<String, serde_json::Value>,
	/// 複数の exec 入力を持つノードで「どのポートを発火したか」を明示する場合に指定。
	/// 省略時は最初の exec 入力ポートを選ぶ（通常 `exec_in`）。
	#[serde(default)]
	pub exec_port: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TriggerNodeResponse {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub receipt: Option<crate::flowgraph::trigger_receipt::ReceiptSnapshot>,
	/// 処理受理（engine への送信まで）を表す。実行完了ではない点に注意。
	pub accepted: bool,
	pub instance_id: String,
	pub node_id: String,
	pub feature: String,
	pub exec_port: String,
	/// 実際に coerce して送った data override のキー名一覧。debug 用。
	pub overridden_inputs: Vec<String>,
}

/// `{"type":"T","value":V}` 形式の薄皮を剥がす。見つからなければそのまま返す。
fn unwrap_typed_value(v: &serde_json::Value) -> &serde_json::Value {
	if let serde_json::Value::Object(obj) = v {
		if obj.len() == 2 && obj.contains_key("type") && obj.contains_key("value") {
			if let Some(inner) = obj.get("value") {
				return inner;
			}
		}
	}
	v
}

/// 検証フェーズのエラー。HTTP status / error code / detail のタプルに落として返す。
///
/// 純粋関数部だけを切り出して `#[cfg(test)]` からも直接叩けるようにしている。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TriggerValidationError {
	NoExecInput,
	ExecPortNotFound(String),
	ExecPortNotExec(String),
	UnknownPort(String),
	ExecPortInData(String),
	InputTypeMismatch { port: String, ty: String },
}

impl TriggerValidationError {
	pub(crate) fn status(&self) -> actix_web::http::StatusCode {
		use actix_web::http::StatusCode;
		match self {
			Self::NoExecInput | Self::ExecPortNotExec(_) | Self::UnknownPort(_) | Self::ExecPortInData(_) => StatusCode::BAD_REQUEST,
			Self::ExecPortNotFound(_) => StatusCode::NOT_FOUND,
			Self::InputTypeMismatch { .. } => StatusCode::UNPROCESSABLE_ENTITY,
		}
	}

	pub(crate) fn code(&self) -> &'static str {
		match self {
			Self::NoExecInput => "no_exec_input",
			Self::ExecPortNotFound(_) => "exec_port_not_found",
			Self::ExecPortNotExec(_) => "invalid_port",
			Self::UnknownPort(_) => "unknown_port",
			Self::ExecPortInData(_) => "invalid_port",
			Self::InputTypeMismatch { .. } => "input_type_mismatch",
		}
	}

	fn detail(&self) -> String {
		match self {
			Self::NoExecInput => "ノードに exec 入力ポートがありません".into(),
			Self::ExecPortNotFound(n) => format!("ノードに exec_port '{n}' がありません"),
			Self::ExecPortNotExec(n) => {
				format!("指定された exec_port '{n}' は exec 入力ではありません")
			}
			Self::UnknownPort(n) => format!("ノードに入力ポート '{n}' が存在しません"),
			Self::ExecPortInData(n) => {
				format!("exec 入力 '{n}' に data 値は送れません（exec_port で指定）")
			}
			Self::InputTypeMismatch { port, ty } => {
				format!("入力 '{port}' の値を socket 型 {ty} に変換できません")
			}
		}
	}

	fn to_response(&self) -> HttpResponse {
		err_json(self.status(), self.code(), self.detail())
	}
}

fn select_exec_port<'a>(
	spec: &'a crate::flowgraph::node::NodeSpec,
	override_name: Option<&str>,
) -> Result<&'a PortSpec, TriggerValidationError> {
	if let Some(name) = override_name {
		match spec.inputs.iter().find(|p| p.name == name) {
			Some(p) if p.direction == PortDirection::Input && p.is_exec => Ok(p),
			Some(_) => Err(TriggerValidationError::ExecPortNotExec(name.to_string())),
			None => Err(TriggerValidationError::ExecPortNotFound(name.to_string())),
		}
	} else {
		spec.inputs
			.iter()
			.find(|p| p.direction == PortDirection::Input && p.is_exec)
			.ok_or(TriggerValidationError::NoExecInput)
	}
}

/// body.inputs を NodeSpec.inputs に沿って coerce する。exec ポートへの data 送信は拒否。
///
/// 純粋関数。HTTP / SharedState に依存しない。
pub(crate) fn coerce_input_overrides(
	spec: &crate::flowgraph::node::NodeSpec,
	inputs: &serde_json::Map<String, serde_json::Value>,
) -> Result<(InputMap, Vec<String>), TriggerValidationError> {
	let mut overrides = InputMap::new();
	let mut names: Vec<String> = Vec::with_capacity(inputs.len());
	for (name, raw_value) in inputs.iter() {
		let Some(port) = spec.inputs.iter().find(|p| &p.name == name) else {
			return Err(TriggerValidationError::UnknownPort(name.clone()));
		};
		if port.is_exec {
			return Err(TriggerValidationError::ExecPortInData(name.clone()));
		}
		let unwrapped = unwrap_typed_value(raw_value);
		let Some(sv) = json_to_socket_value(&port.ty, unwrapped) else {
			return Err(TriggerValidationError::InputTypeMismatch {
				port: name.clone(),
				ty: port.ty.to_string(),
			});
		};
		overrides.insert(name.clone(), sv);
		names.push(name.clone());
	}
	Ok((overrides, names))
}

#[post("/flowgraph/{instance_id}/trigger/{node_id}")]
pub async fn post_trigger_node(
	state: Data<SharedState>,
	path: web::Path<(String, String)>,
	body: Json<TriggerNodeRequest>,
) -> impl Responder {
	let (instance_id, node_id) = path.into_inner();

	if instance_id != "default" {
		return err_json(
			actix_web::http::StatusCode::NOT_FOUND,
			"instance_not_found",
			format!("instance_id '{instance_id}' は存在しません（V2 は 'default' のみ対応）"),
		);
	}

	let fg = state.read().await.flowgraph.clone();
	let rt_guard = fg.read().await;
	let Some(rt) = rt_guard.as_ref() else {
		return err_json(
			actix_web::http::StatusCode::NOT_FOUND,
			"instance_not_found",
			"flowgraph_dir が未設定または未ロードです",
		);
	};

	if let Some(expected) = &body.expected_root {
		if *expected != super::graph_root_identity(&rt.root_dir) {
			return err_json(actix_web::http::StatusCode::CONFLICT,"trigger_root_changed","実行対象の保存先が変わりました");
		}
	}
	if let Some(expected) = &body.expected_source_digest {
		let fq=node_id.split_once("::").map(|(fq,_)|fq).unwrap_or("");
		if rt.source_digests.get(fq) != Some(expected) {
			return err_json(actix_web::http::StatusCode::CONFLICT,"trigger_source_changed","確認した構成と実行内容が異なります");
		}
	}
	let Some(meta) = rt.node_meta.get(&node_id) else {
		return err_json(
			actix_web::http::StatusCode::NOT_FOUND,
			"node_not_found",
			format!("node_id '{node_id}' は存在しません"),
		);
	};
	let feature = meta.feature.clone();

	let reg = registry::registry();
	let Some(spec) = reg.spec(&feature) else {
		return err_json(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"feature_not_registered",
			format!("feature '{feature}' が registry に見つかりません（loader と不整合）"),
		);
	};

	if !reg.is_control_triggerable(&feature) {
		return err_json(
			actix_web::http::StatusCode::FORBIDDEN,
			"feature_not_triggerable",
			format!("feature '{feature}' は control API からの外部トリガに opt-in していません"),
		);
	}

	let exec_port = match select_exec_port(&spec, body.exec_port.as_deref()) {
		Ok(p) => p.name.clone(),
		Err(e) => return e.to_response(),
	};

	let (overrides, overridden_names) = match coerce_input_overrides(&spec, &body.inputs) {
		Ok(v) => v,
		Err(e) => return e.to_response(),
	};

	let Some(handle) = rt.trigger() else {
		return err_json(
			actix_web::http::StatusCode::SERVICE_UNAVAILABLE,
			"worker_not_running",
			"Flowgraph worker が停止中です（ロードエラー等）",
		);
	};

	let mut event = TriggerEvent::new(node_id.clone()).with_exec(exec_port.clone());
	event.data_overrides = overrides;
	if body.wait_ms > 2000 {
		return err_json(actix_web::http::StatusCode::BAD_REQUEST,"invalid_receipt_wait","結果の待機は最大2000msです");
	}
	let receipt = if body.observe.is_empty() { None } else {
		let receipt=match crate::flowgraph::trigger_receipt::TriggerReceipt::new(body.observe.clone()) {
			Ok(value)=>std::sync::Arc::new(value),
			Err(error)=>return err_json(actix_web::http::StatusCode::BAD_REQUEST,"invalid_receipt_outputs",error),
		};
		for (node,port) in &body.observe {
			let valid=rt.node_meta.get(node).and_then(|meta| reg.spec(&meta.feature))
				.is_some_and(|spec| spec.outputs.iter().any(|p| p.name==*port && p.is_exec));
			if !valid { return err_json(actix_web::http::StatusCode::BAD_REQUEST,"invalid_receipt_outputs","観測対象は現在の構成にあるexec出力に限ります"); }
		}
		Some(receipt)
	};
	event.receipt=receipt.clone();

	if let Err(e) = handle.send(event) {
		return err_json(
			actix_web::http::StatusCode::SERVICE_UNAVAILABLE,
			"trigger_send_failed",
			format!("trigger 送信失敗: {e}"),
		);
	}
	// Never hold the runtime read lock while waiting; reload may retire this
	// worker. The receipt remains tied to the exact event sent above.
	drop(rt_guard);
	let receipt=match receipt {
		Some(receipt)=>receipt.wait_snapshot(std::time::Duration::from_millis(body.wait_ms)).await,
		None=>None,
	};

	log::info!(
		"《Flowgraph/Trigger》 {} :: {} (feature={}, exec={}, overrides={})",
		instance_id,
		node_id,
		feature,
		exec_port,
		overridden_names.len()
	);

	HttpResponse::Accepted().json(TriggerNodeResponse {
		receipt,
		accepted: true,
		instance_id,
		node_id,
		feature,
		exec_port,
		overridden_inputs: overridden_names,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::socket::SocketValue;
	use serde_json::json;

	fn learn_spec() -> crate::flowgraph::node::NodeSpec {
		registry::registry()
			.spec("flowgraph.glossary.learn")
			.expect("learn が registry に必要")
	}

	#[test]
	fn unwrap_typed_value_strips_wrapper() {
		let wrapped = json!({ "type": "string", "value": "DR.USAGI" });
		assert_eq!(unwrap_typed_value(&wrapped), &json!("DR.USAGI"));

		let bare = json!("DR.USAGI");
		assert_eq!(unwrap_typed_value(&bare), &bare);

		// 余計なキーが付いていたらそのまま（Json ポート向け）。
		let extra = json!({ "type": "string", "value": "x", "extra": 1 });
		assert_eq!(unwrap_typed_value(&extra), &extra);
	}

	#[test]
	fn select_exec_port_picks_first_by_default() {
		let spec = learn_spec();
		let p = select_exec_port(&spec, None).expect("learn は exec_in を持つ");
		assert_eq!(p.name, "exec_in");
		assert!(p.is_exec);
	}

	#[test]
	fn select_exec_port_accepts_matching_override() {
		let spec = learn_spec();
		let p = select_exec_port(&spec, Some("exec_in")).unwrap();
		assert_eq!(p.name, "exec_in");
	}

	#[test]
	fn select_exec_port_rejects_non_exec_override() {
		let spec = learn_spec();
		// `surface` は String 入力であって exec ではない。
		let err = select_exec_port(&spec, Some("surface")).unwrap_err();
		assert!(matches!(err, TriggerValidationError::ExecPortNotExec(ref n) if n == "surface"));
		assert_eq!(err.status(), actix_web::http::StatusCode::BAD_REQUEST);
		assert_eq!(err.code(), "invalid_port");
	}

	#[test]
	fn select_exec_port_rejects_missing_override() {
		let spec = learn_spec();
		let err = select_exec_port(&spec, Some("ghost")).unwrap_err();
		assert!(matches!(err, TriggerValidationError::ExecPortNotFound(_)));
		assert_eq!(err.status(), actix_web::http::StatusCode::NOT_FOUND);
	}

	#[test]
	fn coerce_input_overrides_happy_path() {
		let spec = learn_spec();
		let inputs = serde_json::Map::from_iter([
			("reading".into(), json!("ドクターウサギ")),
			("surface".into(), json!({"type":"string","value":"Dr.USAGI"})),
			("priority".into(), json!(3)),
		]);
		let (map, names) = coerce_input_overrides(&spec, &inputs).unwrap();
		assert_eq!(names.len(), 3);
		assert!(matches!(map.get("reading"), Some(SocketValue::String(s)) if s == "ドクターウサギ"));
		assert!(matches!(map.get("surface"), Some(SocketValue::String(s)) if s == "Dr.USAGI"));
		assert!(matches!(map.get("priority"), Some(SocketValue::Int(3))));
	}

	#[test]
	fn coerce_input_overrides_unknown_port() {
		let spec = learn_spec();
		let inputs = serde_json::Map::from_iter([("ghost".into(), json!("x"))]);
		let err = coerce_input_overrides(&spec, &inputs).unwrap_err();
		assert!(matches!(err, TriggerValidationError::UnknownPort(ref n) if n == "ghost"));
		assert_eq!(err.status(), actix_web::http::StatusCode::BAD_REQUEST);
		assert_eq!(err.code(), "unknown_port");
	}

	#[test]
	fn coerce_input_overrides_exec_port_rejected() {
		let spec = learn_spec();
		let inputs = serde_json::Map::from_iter([("exec_in".into(), json!(true))]);
		let err = coerce_input_overrides(&spec, &inputs).unwrap_err();
		assert!(matches!(err, TriggerValidationError::ExecPortInData(_)));
	}

	#[test]
	fn coerce_input_overrides_type_mismatch() {
		let spec = learn_spec();
		// priority は int 要求だが string を投げる → 422。
		let inputs = serde_json::Map::from_iter([("priority".into(), json!("not-a-number"))]);
		let err = coerce_input_overrides(&spec, &inputs).unwrap_err();
		assert!(matches!(err, TriggerValidationError::InputTypeMismatch { .. }));
		assert_eq!(err.status(), actix_web::http::StatusCode::UNPROCESSABLE_ENTITY);
		assert_eq!(err.code(), "input_type_mismatch");
	}

	#[test]
	fn coerce_input_overrides_empty_is_ok() {
		let spec = learn_spec();
		let (map, names) = coerce_input_overrides(&spec, &serde_json::Map::new()).unwrap();
		assert!(map.is_empty());
		assert!(names.is_empty());
	}
    #[test]
    fn control_input_accepts_typed_flags_and_rejects_invalid_shape() {
        let registry = crate::flowgraph::registry::registry();
        assert!(registry.is_control_triggerable("flowgraph.ingress.control"));
        let spec = registry.spec("flowgraph.ingress.control").unwrap();
        assert_eq!(select_exec_port(&spec, None).unwrap().name, "exec_in");
        let (values, _) = coerce_input_overrides(&spec, &serde_json::from_value(json!({"content":"learn(X=Y)", "flags":["dictionary.write"]})).unwrap()).unwrap();
        assert!(matches!(values.get("flags"), Some(SocketValue::List(items)) if items.len() == 1));
        assert!(coerce_input_overrides(&spec, &serde_json::from_value(json!({"flags":{"dictionary.write":true}})).unwrap()).is_err());
        assert!(coerce_input_overrides(&spec, &serde_json::from_value(json!({"exec_in":true})).unwrap()).is_err());
    }

    #[actix_web::test]
    async fn protected_control_input_persists_only_authorized_targeted_commands() {
        use actix_web::{test as http, App};
        use std::sync::Arc;
        use crate::web_interface::control::auth::{ControlApiRuntime, TokenSource, control_api_auth};
        let root = std::env::temp_dir().join(format!("vac-control-input-http-{}", rand::random::<u64>()));
        let graphs = root.join("graphs");
        std::fs::create_dir_all(graphs.join("assets")).unwrap();
        let graph = include_str!("../../../../flowgraph.example/dictionary-managed-command/main.flowgraph.toml")
            .replace("flowgraph.ingress.web_input", "flowgraph.ingress.control")
            .replace("properties.value=[]", "properties.value=[\"dictionary.write\"]")
            .replace("properties.value=[\"example_moderator\"]", "properties.value=[]");
        std::fs::write(graphs.join("main.flowgraph.toml"), format!("{graph}\n[[edges]]\nfrom=\"in:flags\"\nto=\"gate:flags\"\n")).unwrap();
        std::fs::write(graphs.join("assets/commands.tsv"), include_str!("../../../../flowgraph.example/dictionary-managed-command/assets/commands.tsv")).unwrap();
        let dictionary = graphs.join("assets/user.tsv");
        std::fs::write(&dictionary, include_str!("../../../../flowgraph.example/dictionary-managed-command/assets/user.tsv")).unwrap();
        let peer = root.join("peer.tsv"); std::fs::write(&peer, "untouched peer").unwrap();
        let mut conf: crate::Conf = toml::from_str("").unwrap(); conf.runtime_dir = Some(root.join("runtime"));
        let state = crate::State::new(&conf, Arc::new(tokio::sync::Mutex::new(crate::AudioSink::disconnected_for_test())), crate::shutdown::ShutdownBroker::new()).await.unwrap();
        let runtime = crate::flowgraph::runtime::FlowgraphRuntime::load_and_spawn(&graphs, Arc::downgrade(&state), None, None, None, None);
        assert!(runtime.ok, "{:?}", runtime.diagnostics);
        let worker = runtime.handle.clone().unwrap();
        *state.read().await.flowgraph.write().await = Some(runtime);
        let policy = ControlApiRuntime { token: "isolated-test-token".into(), token_source: TokenSource::Config,
            require_token_for_loopback: true, require_token_for_non_loopback: true, written_token_file: None, tables: vec![] };
        let app = http::init_service(App::new().app_data(Data::new(state.clone())).app_data(Data::new(policy))
            .service(web::scope("/api/v1/control").wrap(actix_web::middleware::from_fn(control_api_auth)).service(post_trigger_node))).await;
        let uri = "/api/v1/control/flowgraph/default/trigger/main%3A%3Ain";
        for (token, body, expected_status, changed) in [
            (None, json!({"inputs":{"content":"learn(X=拒否)","flags":["dictionary.write"]}}), 401, false),
            (Some("wrong"), json!({"inputs":{"content":"learn(X=拒否)","flags":["dictionary.write"]}}), 401, false),
            (Some("isolated-test-token"), json!({"expected_root":"wrong-root", "inputs":{"content":"learn(X=別対象)","flags":["dictionary.write"]}}), 409, false),
            (Some("isolated-test-token"), json!({"inputs":{"content":"learn(X=誤型)","flags":{"dictionary.write":true}}}), 422, false),
            (Some("isolated-test-token"), json!({"inputs":{"content":"learn(X=dictionary.write)","flags":[]}}), 202, false),
            (Some("isolated-test-token"), json!({"inputs":{"content":"learn(X=正)","flags":["dictionary.write"]}}), 202, true),
            (Some("isolated-test-token"), json!({"inputs":{"content":"learn(Y=残留権限)","flags":[]}}), 202, false),
        ] {
            let before = std::fs::read(&dictionary).unwrap();
            let mut request = http::TestRequest::post().uri(uri).peer_addr("127.0.0.1:12345".parse().unwrap()).set_json(body);
            if let Some(token) = token { request = request.insert_header(("Authorization", format!("Bearer {token}"))); }
            let response = http::call_service(&app, request.to_request()).await;
            assert_eq!(response.status().as_u16(), expected_status);
            // HTTP acceptance alone is not execution completion. Wait for the
            // running engine to drain the event before inspecting persisted data.
            tokio::time::timeout(std::time::Duration::from_secs(3), worker.command.export_state_snapshot()).await.unwrap().unwrap();
            assert_eq!(before != std::fs::read(&dictionary).unwrap(), changed);
            assert_eq!(std::fs::read_to_string(&peer).unwrap(), "untouched peer");
        }
        assert!(std::fs::read_to_string(&dictionary).unwrap().contains("正\tX"));
        worker.shutdown().await;
        drop(app); drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }

}
