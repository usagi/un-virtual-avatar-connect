//! `flowgraph.ingress.web_input` 用の actix-web 組み込みブリッジ（δ-9 Part B）。
//!
//! ## 機能
//!
//! - ingress ノードの property `path` / `method` / `body_format` / `fixed_channel` を読み、
//!   actix-web の `ServiceConfig` にハンドラを登録する。
//! - リクエスト受信時、本文を decode して `TriggerEvent::new(node_id)` に
//!   `__content__` / `__source_actor__` / `__source_kind__` の overrides を載せ、
//!   `TriggerHandle::send` で Flowgraph ワーカーに投げ込む。
//! - `path` が空の場合は `/input/flowgraph/<fq_node_id>` を自動採番。
//! - 同一 `(path, method)` の重複登録はログ警告のうえ先勝ち。

use crate::flowgraph::loader::LoadedNodeMeta;
use crate::flowgraph::node::{TriggerEvent, TriggerHandle};
use crate::flowgraph::socket::SocketValue;
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;
use std::sync::Arc;

/// ingress ノード 1 件分の解決済みメタ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowgraphWebInputEndpoint {
	pub node_id: String,
	pub path: String,
	pub method: Method,
	pub body_format: BodyFormat,
	pub fixed_channel: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Method {
	Get,
	Post,
	Put,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyFormat {
	Plain,
	Json,
	Form,
	Toml,
	MsgPack,
}

impl FlowgraphWebInputEndpoint {
	pub fn from_meta(fq: &str, meta: &LoadedNodeMeta) -> Option<Self> {
		let props = &meta.properties;
		let path_raw = props
			.get("path")
			.and_then(|v| v.as_str().ok())
			.map(str::trim)
			.map(str::to_string)
			.unwrap_or_default();
		let path = if path_raw.is_empty() {
			format!("/input/flowgraph/{}", sanitize_path_segment(fq))
		} else {
			normalize_path(&path_raw)
		};
		let method = props
			.get("method")
			.and_then(|v| v.as_str().ok())
			.map(|s| s.trim().to_ascii_uppercase())
			.and_then(|s| match s.as_str() {
				"GET" => Some(Method::Get),
				"POST" => Some(Method::Post),
				"PUT" => Some(Method::Put),
				other => {
					log::warn!("《Flowgraph/WebInput》 node={fq} 不明な method='{other}'、POST にフォールバック");
					None
				}
			})
			.unwrap_or(Method::Post);
		let body_format = props
			.get("body_format")
			.and_then(|v| v.as_str().ok())
			.map(|s| s.trim().to_ascii_lowercase())
			.and_then(|s| match s.as_str() {
				"plain" | "" => Some(BodyFormat::Plain),
				"json" => Some(BodyFormat::Json),
				"form" => Some(BodyFormat::Form),
                "toml" => Some(BodyFormat::Toml),
                "msgpack" | "messagepack" => Some(BodyFormat::MsgPack),
				other => {
					log::warn!("《Flowgraph/WebInput》 node={fq} 不明な body_format='{other}'、plain にフォールバック");
					None
				}
			})
			.unwrap_or(BodyFormat::Plain);
		let fixed_channel = props
			.get("fixed_channel")
			.and_then(|v| v.as_str().ok())
			.map(str::trim)
			.map(str::to_string)
			.unwrap_or_default();
		Some(Self {
			node_id: fq.to_string(),
			path,
			method,
			body_format,
			fixed_channel,
		})
	}
}

fn normalize_path(p: &str) -> String {
	let p = p.trim();
	let p = if p.starts_with('/') {
		p.to_string()
	} else {
		format!("/{}", p.trim_start_matches('/'))
	};
	let p = p.trim_end_matches('/');
	if p.is_empty() {
		"/".into()
	} else {
		p.to_string()
	}
}

fn sanitize_path_segment(fq: &str) -> String {
	fq.chars()
		.map(|c| {
			if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
				c
			} else {
				'_'
			}
		})
		.collect()
}

// ---------------------------------------------------------------------
// actix-web 統合
// ---------------------------------------------------------------------

/// Flowgraph web_input エンドポイントを actix-web `ServiceConfig` に一括登録する。
///
/// Trigger は `Arc<Option<TriggerHandle>>` 相当を `Data` で渡す想定。
/// Flowgraph ワーカー未起動時は全ハンドラが 503 を返す。
pub fn register_routes(cfg: &mut web::ServiceConfig, endpoints: &[FlowgraphWebInputEndpoint]) {
	use std::collections::HashSet;
	let mut seen: HashSet<(String, Method)> = HashSet::new();
	for ep in endpoints {
		if !seen.insert((ep.path.clone(), ep.method)) {
			log::warn!(
				"《Flowgraph/WebInput》 path+method 重複: {} {:?} (node={})",
				ep.path,
				ep.method,
				ep.node_id
			);
			continue;
		}
		let ep_data = web::Data::new(ep.clone());
		// Resource selection must include the method: a route-only guard makes
		// the first resource return 405 before another method at this path is tried.
		let resource = web::resource(&ep.path);
		let resource = match ep.method {
			Method::Get => resource.guard(actix_web::guard::Get()).route(web::get().to(handle_get)),
			Method::Post => resource.guard(actix_web::guard::Post()).route(web::post().to(handle_post)),
			Method::Put => resource.guard(actix_web::guard::Put()).route(web::put().to(handle_post)), // PUT も POST と同じ decode
		};
		cfg.service(resource.app_data(ep_data));
	}
}

#[derive(Deserialize)]
struct GetQuery {
	#[serde(default)]
	channel: Option<String>,
	#[serde(default)]
	is_final: Option<bool>,
	#[serde(default)]
	content: Option<String>,
	#[serde(default)]
	source_actor: Option<String>,
}

async fn handle_get(
	_req: HttpRequest,
	query: web::Query<GetQuery>,
	ep: web::Data<FlowgraphWebInputEndpoint>,
	trigger: web::Data<Arc<Option<TriggerHandle>>>,
    state: Option<web::Data<crate::SharedState>>,
) -> impl Responder {
	let content = query.content.clone().unwrap_or_default();
	let actor = query.source_actor.clone().unwrap_or_default();
	forward_current(&ep, &trigger, state.as_ref().map(|value| value.get_ref()), content, actor, input_metadata(query.channel.as_deref(), query.is_final)).await
}

// Keep the legacy MessagePack sequence order as well as named-map encoding.
#[derive(Deserialize)]
struct LegacyStructuredBody {
    #[serde(default)]
    channel: Option<String>,
    content: String,
    #[serde(default)]
    is_final: Option<bool>,
    #[serde(default)]
    source_actor: Option<String>,
}

async fn handle_post(
	_req: HttpRequest,
	body: web::Bytes,
	ep: web::Data<FlowgraphWebInputEndpoint>,
	trigger: web::Data<Arc<Option<TriggerHandle>>>,
    state: Option<web::Data<crate::SharedState>>,
) -> impl Responder {
	let (content, actor, meta) = match ep.body_format {
		BodyFormat::Plain => (String::from_utf8_lossy(&body).into_owned(), String::new(), serde_json::Value::Null),
		BodyFormat::Json => match serde_json::from_slice::<LegacyStructuredBody>(&body) {
			Ok(b) => { let meta = input_metadata(b.channel.as_deref(), b.is_final); (b.content, b.source_actor.unwrap_or_default(), meta) },
			Err(e) => {
				return HttpResponse::BadRequest().body(format!("invalid json: {e}"));
			}
		},
        BodyFormat::Toml => match std::str::from_utf8(&body).map_err(|e|e.to_string())
            .and_then(|text|toml::from_str::<LegacyStructuredBody>(text).map_err(|e|e.to_string())) {
            Ok(b) => { let meta = input_metadata(b.channel.as_deref(), b.is_final); (b.content, b.source_actor.unwrap_or_default(), meta) },
            Err(e) => return HttpResponse::BadRequest().body(format!("invalid toml: {e}")),
        },
        BodyFormat::MsgPack => match rmp_serde::from_slice::<LegacyStructuredBody>(&body) {
            Ok(b) => { let meta = input_metadata(b.channel.as_deref(), b.is_final); (b.content, b.source_actor.unwrap_or_default(), meta) },
            Err(e) => return HttpResponse::BadRequest().body(format!("invalid msgpack: {e}")),
        },
		BodyFormat::Form => match serde_urlencoded::from_bytes::<LegacyStructuredBody>(&body) {
			Ok(b) => { let meta = input_metadata(b.channel.as_deref(), b.is_final); (b.content, b.source_actor.unwrap_or_default(), meta) },
			Err(e) => {
				return HttpResponse::BadRequest().body(format!("invalid form: {e}"));
			}
		},
	};
	forward_current(&ep, &trigger, state.as_ref().map(|value| value.get_ref()), content, actor, meta).await
}

// Preserve caller-supplied context as data, never as authorization or routing.
fn input_metadata(channel: Option<&str>, is_final: Option<bool>) -> serde_json::Value {
    let mut fields = serde_json::Map::new();
    if let Some(channel) = channel { fields.insert("channel".into(), channel.into()); }
    if let Some(is_final) = is_final { fields.insert("is_final".into(), is_final.into()); }
    if fields.is_empty() { serde_json::Value::Null }
    else { serde_json::json!({"web_input": fields}) }
}

// Production HTTP services keep their routes across graph reloads, but the
// worker behind those routes is replaced. Never retain its startup sender.
async fn forward_current(ep: &FlowgraphWebInputEndpoint, startup: &Arc<Option<TriggerHandle>>,
    state: Option<&crate::SharedState>, content: String, actor: String, meta: serde_json::Value) -> HttpResponse {
    let Some(state) = state else { return forward(ep, startup, content, actor, meta).await; };
    let graph = state.read().await.flowgraph.clone();
    let runtime = graph.read().await;
    let Some(runtime) = runtime.as_ref() else {
        return HttpResponse::ServiceUnavailable().body("flowgraph runtime not active");
    };
    // Changed method/body/path must not be interpreted using stale HTTP metadata.
    // Those changes still require the existing explicit restart workflow.
    if runtime.node_meta.get(&ep.node_id).and_then(|meta| FlowgraphWebInputEndpoint::from_meta(&ep.node_id, meta)).as_ref() != Some(ep) {
        return HttpResponse::ServiceUnavailable().body("web input changed; restart VAC to apply its routes");
    }
    let trigger = Arc::new(runtime.trigger());
    forward(ep, &trigger, content, actor, meta).await
}

async fn forward(ep: &FlowgraphWebInputEndpoint, trigger: &Arc<Option<TriggerHandle>>, content: String, actor: String, meta: serde_json::Value) -> HttpResponse {
	let Some(handle) = trigger.as_ref().as_ref() else {
		return HttpResponse::ServiceUnavailable().body("flowgraph runtime not active");
	};
	let source_kind = if ep.fixed_channel.is_empty() {
		"web_input".to_string()
	} else {
		ep.fixed_channel.clone()
	};
	let event = TriggerEvent::new(&ep.node_id)
		.with_exec("__trigger__")
		.with_override("__content__", SocketValue::String(content))
		.with_override("__source_actor__", SocketValue::String(actor))
		.with_override("__source_kind__", SocketValue::String(source_kind))
        .with_override("__meta__", SocketValue::Json(meta));
	match handle.send(event) {
		Ok(()) => HttpResponse::Ok().body("ok"),
		Err(e) => {
			log::warn!("《Flowgraph/WebInput》 trigger 送信失敗 node={} err={e}", ep.node_id);
			HttpResponse::ServiceUnavailable().body(format!("trigger send failed: {e}"))
		}
	}
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::engine::create_trigger_bus;
	use crate::flowgraph::node::InputMap;
	use std::path::PathBuf;

    #[actix_web::test]
    async fn existing_http_route_follows_replaced_worker_and_rejects_changed_endpoint() {
        let root = std::env::temp_dir().join(format!("vac-http-reload-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&root).unwrap();
        let source = include_str!("../../gui/src/lib/flowgraph/templates/counter.flowgraph.toml");
        std::fs::write(root.join("main.flowgraph.toml"), source).unwrap();
        let mut conf: crate::Conf = toml::from_str("").unwrap();
        conf.runtime_dir = Some(root.join("runtime"));
        let audio = Arc::new(tokio::sync::Mutex::new(crate::AudioSink::disconnected_for_test()));
        let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
        let graph = state.read().await.flowgraph.clone();
        let initial = crate::flowgraph::FlowgraphRuntime::load_and_spawn(&root, std::sync::Weak::new(), None, None, None, None);
        assert!(initial.ok);
        let startup = initial.trigger();
        let endpoints = crate::bridges::collect_all(&initial.node_meta).web_input;
        let uri = endpoints[0].path.clone();
        *graph.write().await = Some(initial);
        let app = actix_web::test::init_service(actix_web::App::new()
            .app_data(web::Data::new(state.clone()))
            .app_data(web::Data::new(Arc::new(startup.clone())))
            .configure(|cfg| register_routes(cfg, &endpoints))).await;
        for generation in 0..3 {
            if generation > 0 {
                graph.read().await.as_ref().unwrap().handle.as_ref().unwrap().shutdown().await;
                let next = crate::flowgraph::FlowgraphRuntime::load_and_spawn(&root, std::sync::Weak::new(), None, None, None, None);
                assert!(next.ok);
                *graph.write().await = Some(next);
                assert!(startup.as_ref().unwrap().send_exec("main::in", "__trigger__").is_err());
            }
            let response = actix_web::test::call_service(&app, actix_web::test::TestRequest::post().uri(&uri).set_payload("input").to_request()).await;
            assert_eq!(response.status(), 200);
            let runtime = graph.read().await;
            let snapshot = runtime.as_ref().unwrap().export_live_state_snapshot().await.unwrap().unwrap();
            assert!(snapshot.nodes.iter().any(|n| n.value == serde_json::json!({"value":1})));
        }
        // The existing HTTP route must not accept a newly configured method.
        graph.read().await.as_ref().unwrap().handle.as_ref().unwrap().shutdown().await;
        std::fs::write(root.join("main.flowgraph.toml"), source.replace("feature = \"flowgraph.ingress.web_input\"", "feature = \"flowgraph.ingress.web_input\"\nproperties.method = \"PUT\"")).unwrap();
        let next = crate::flowgraph::FlowgraphRuntime::load_and_spawn(&root, std::sync::Weak::new(), None, None, None, None);
        assert!(next.ok);
        *graph.write().await = Some(next);
        let response = actix_web::test::call_service(&app, actix_web::test::TestRequest::post().uri(&uri).set_payload("stale method").to_request()).await;
        assert_eq!(response.status(), 503);
        graph.read().await.as_ref().unwrap().handle.as_ref().unwrap().shutdown().await;
        let resolved = root.canonicalize().unwrap();
        assert!(resolved.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert!(resolved.file_name().unwrap().to_string_lossy().starts_with("vac-http-reload-"));
        std::fs::remove_dir_all(resolved).unwrap();
    }

	#[actix_web::test]
	async fn bundled_counter_accepts_http_after_runtime_recreation() {
		let root = std::env::temp_dir().join(format!("vac-counter-http-{}-{}",std::process::id(),rand::random::<u64>()));
		std::fs::create_dir_all(root.join("features/counter")).unwrap();
		std::fs::write(root.join("features/counter/main.flowgraph.toml"),include_str!("../../gui/src/lib/flowgraph/templates/counter.flowgraph.toml")).unwrap();
		for boot in 0..2 {
			let rt=crate::flowgraph::runtime::FlowgraphRuntime::load_and_spawn(&root,std::sync::Weak::new(),None,None,None,None);
			assert!(rt.ok,"{:?}",rt.diagnostics);
			let handle=rt.handle.as_ref().unwrap();
			let endpoints=crate::bridges::collect_all(&rt.node_meta).web_input;
			assert_eq!(endpoints.len(),1);
			let path=endpoints[0].path.clone();
			let app=actix_web::test::init_service(actix_web::App::new()
				.app_data(web::Data::new(Arc::new(Some(handle.trigger.clone()))))
				.configure(|cfg|register_routes(cfg,&endpoints))).await;
			for expected in 1..=2 {
				let response=actix_web::test::call_service(&app,actix_web::test::TestRequest::post().uri(&path).set_payload("counter input").to_request()).await;
				assert_eq!(response.status(),200);
				tokio::time::timeout(std::time::Duration::from_secs(3),async {
					loop {
						let snapshot=handle.command.export_state_snapshot().await.unwrap();
						if snapshot.nodes.iter().any(|n|n.feature=="flowgraph.state.int_counter" && n.value==serde_json::json!({"value":expected})){break;}
						tokio::time::sleep(std::time::Duration::from_millis(10)).await;
					}
				}).await.expect("HTTP input must increment the real counter state");
			}
			println!("counter boot {boot}: HTTP input reached states 1 and 2");
			let messages:Vec<_>=handle.runtime_trace.read().unwrap().iter()
				.filter(|event|event.phase==crate::flowgraph::RuntimeTracePhase::Log)
				.map(|event|(event.runtime_node_id.clone(),event.value_summary.clone())).collect();
			assert_eq!(messages,vec![("features/counter/main::count_log".into(),"1".into()),("features/counter/main::count_log".into(),"2".into())]);
			handle.shutdown().await;
		}
		assert!(root.starts_with(std::env::temp_dir()));
		std::fs::remove_dir_all(root).unwrap();
	}


    #[actix_web::test]
    async fn web_input_metadata_preserves_false_and_missing_without_rerouting() {
        use actix_web::{test, App};
        for method in [Method::Get, Method::Post] {
            for format in [BodyFormat::Json, BodyFormat::Form] {
                let (trigger, mut rx) = create_trigger_bus();
                let ep = FlowgraphWebInputEndpoint { node_id: "fixed::in".into(), path: "/metadata".into(), method, body_format: format, fixed_channel: "configured".into() };
                let app = test::init_service(App::new().app_data(web::Data::new(Arc::new(Some(trigger))))
                    .configure(|cfg|register_routes(cfg, std::slice::from_ref(&ep)))).await;
                for supplied in [true, false] {
                    let query = if supplied { "content=hello&channel=another&is_final=false&source_actor=alice" } else { "content=hello" };
                    let request = if method == Method::Get {
                        test::TestRequest::get().uri(&format!("/metadata?{query}"))
                    } else if format == BodyFormat::Form {
                        test::TestRequest::post().uri("/metadata").set_payload(query)
                    } else {
                        test::TestRequest::post().uri("/metadata").set_json(if supplied {
                            serde_json::json!({"content":"hello","channel":"another","is_final":false,"source_actor":"alice","flags":["moderator"]})
                        } else { serde_json::json!({"content":"hello"}) })
                    };
                    assert_eq!(test::call_service(&app, request.to_request()).await.status(), 200);
                    let event = rx.try_recv().unwrap();
                    assert_eq!(event.node_id, "fixed::in");
                    assert_eq!(event.data_overrides["__source_kind__"].as_str().unwrap(), "configured");
                    assert_eq!(event.data_overrides["__source_actor__"].as_str().unwrap(), if supplied {"alice"} else {""});
                    assert_eq!(event.data_overrides["__meta__"], SocketValue::Json(if supplied {
                        serde_json::json!({"web_input":{"channel":"another","is_final":false}})
                    } else { serde_json::Value::Null }));
                    assert!(!event.data_overrides.contains_key("flags"));
                }
            }
        }
    }

    #[actix_web::test]
    async fn legacy_structured_bodies_reach_web_input() {
        use actix_web::{test, App};
        let old = crate::web_interface::web_input::InputPayload { channel: Some("old".into()), content: "日本語の本文".into(), is_final: true };
        let cases = [
            ("toml", toml::to_string(&old).unwrap().into_bytes()),
            ("msgpack", rmp_serde::to_vec(&old).unwrap()),
            ("messagepack", rmp_serde::to_vec_named(&old).unwrap()),
        ];
        for (format, body) in cases {
            let (trigger, mut rx) = create_trigger_bus();
            let root = std::env::temp_dir().join(format!("vac-web-migration-{}-{}", std::process::id(), rand::random::<u64>()));
            let old_conf = format!("[[processors]]\nfeature='webinput'\nchannel_to='chat'\nweb_input_path='/legacy-message'\nweb_input_body_format='{format}'\n");
            let report = crate::migrate::migrate_conf_str(&old_conf, &root, false).unwrap();
            assert!(!report.has_errors(), "{}", report.summary());
            let loaded = crate::flowgraph::loader::load_file(&root.join("flowgraph/web-input/main.flowgraph.toml"), Some("migrated")).unwrap();
            let endpoints = crate::bridges::collect_all(&loaded.node_meta).web_input;
            assert_eq!(endpoints.len(), 1);
            let ep = &endpoints[0];
            assert_eq!(ep.path, "/legacy-message");
            let app = test::init_service(App::new().app_data(web::Data::new(Arc::new(Some(trigger))))
                .configure(|cfg|register_routes(cfg, std::slice::from_ref(ep)))).await;
            let reply = test::call_service(&app, test::TestRequest::post().uri(&ep.path).set_payload(body).to_request()).await;
            assert_eq!(reply.status(), 200, "{format}");
            let event = rx.try_recv().unwrap();
            assert_eq!(event.node_id, "migrated::in");
            assert_eq!(event.data_overrides["__meta__"], SocketValue::Json(serde_json::json!({"web_input":{"channel":"old","is_final":true}})));
            assert_eq!(event.data_overrides["__content__"].as_str().unwrap(), "日本語の本文");
            assert_eq!(event.data_overrides["__source_kind__"].as_str().unwrap(), "chat");
            let bad = test::call_service(&app, test::TestRequest::post().uri(&ep.path).set_payload(vec![0xc1, 0xff]).to_request()).await;
            assert_eq!(bad.status(), 400, "{format}");
            assert!(rx.try_recv().is_err(), "Malformed body must not trigger {format}");
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[actix_web::test]
    async fn same_path_methods_reach_their_own_nodes() {
        use actix_web::{test, App, http::Method as HttpMethod};
        let (trigger, mut rx) = create_trigger_bus();
        let endpoints: Vec<_> = [Method::Get, Method::Post, Method::Put].into_iter().enumerate()
            .map(|(i, method)| FlowgraphWebInputEndpoint {
                node_id: format!("input_{i}"), path: "/shared-input".into(), method,
                body_format: BodyFormat::Plain, fixed_channel: format!("channel_{i}"),
            }).collect();
        let app = test::init_service(App::new()
            .app_data(web::Data::new(Arc::new(Some(trigger))))
            .configure(|cfg| register_routes(cfg, &endpoints))).await;
        for (i, method) in [HttpMethod::GET, HttpMethod::POST, HttpMethod::PUT].into_iter().enumerate() {
            let request = test::TestRequest::default().uri("/shared-input?content=hello")
                .method(method).set_payload("hello").to_request();
            let response = test::call_service(&app, request).await;
            assert!(response.status().is_success(), "method index {i}: {}", response.status());
            let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await.unwrap().unwrap();
            assert_eq!(event.node_id, format!("input_{i}"));
            assert_eq!(event.data_overrides.get("__source_kind__").and_then(|v|v.as_str().ok()), Some(format!("channel_{i}").as_str()));
        }
    }

	fn meta_with(props: Vec<(&str, SocketValue)>) -> LoadedNodeMeta {
		let mut m = InputMap::new();
		for (k, v) in props {
			m.insert(k.to_string(), v);
		}
		LoadedNodeMeta {
			feature: "flowgraph.ingress.web_input".into(),
			file: PathBuf::from("t.flowgraph.toml"),
			position: None,
			activation_chain: Vec::new(),
			properties: m,
		}
	}

	#[test]
	fn endpoint_defaults_are_applied() {
		let meta = meta_with(vec![]);
		let ep = FlowgraphWebInputEndpoint::from_meta("ingress/web::in", &meta).unwrap();
		assert_eq!(ep.path, "/input/flowgraph/ingress_web__in");
		assert_eq!(ep.method, Method::Post);
		assert_eq!(ep.body_format, BodyFormat::Plain);
		assert_eq!(ep.fixed_channel, "");
	}

	#[test]
	fn endpoint_from_properties() {
		let meta = meta_with(vec![
			("path", SocketValue::String("/input/chat".into())),
			("method", SocketValue::String("post".into())),
			("body_format", SocketValue::String("json".into())),
			("fixed_channel", SocketValue::String("my_channel".into())),
		]);
		let ep = FlowgraphWebInputEndpoint::from_meta("in", &meta).unwrap();
		assert_eq!(ep.path, "/input/chat");
		assert_eq!(ep.method, Method::Post);
		assert_eq!(ep.body_format, BodyFormat::Json);
		assert_eq!(ep.fixed_channel, "my_channel");
	}

	#[test]
	fn unknown_method_falls_back_to_post() {
		let meta = meta_with(vec![("method", SocketValue::String("HEAD".into()))]);
		let ep = FlowgraphWebInputEndpoint::from_meta("in", &meta).unwrap();
		assert_eq!(ep.method, Method::Post);
	}

	#[tokio::test]
	async fn forward_sends_trigger_event_with_overrides() {
		let (handle, mut rx) = create_trigger_bus();
		let arc_handle: Arc<Option<TriggerHandle>> = Arc::new(Some(handle));
		let ep = FlowgraphWebInputEndpoint {
			node_id: "in".into(),
			path: "/x".into(),
			method: Method::Post,
			body_format: BodyFormat::Plain,
			fixed_channel: "chan1".into(),
		};
		let _resp = super::forward(&ep, &arc_handle, "hello".into(), "alice".into(), serde_json::Value::Null).await;
		let ev = rx.recv().await.expect("event received");
		assert_eq!(ev.node_id, "in");
		assert!(ev.fired_exec.contains(&"__trigger__".to_string()));
		assert_eq!(ev.data_overrides.get("__content__").and_then(|v| v.as_str().ok()), Some("hello"));
		assert_eq!(
			ev.data_overrides.get("__source_actor__").and_then(|v| v.as_str().ok()),
			Some("alice")
		);
		assert_eq!(
			ev.data_overrides.get("__source_kind__").and_then(|v| v.as_str().ok()),
			Some("chan1")
		);
	}
}
