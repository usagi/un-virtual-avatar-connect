//! Explicit moderator command gate and reusable clip lookup. No implicit chat/OBS output.
use crate::flowgraph::node::*;
use crate::flowgraph::socket::{SocketType, SocketValue};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::{
	any::Any,
	collections::{HashMap, VecDeque},
	time::{Duration, Instant},
};

pub struct ClipCommandNode;
pub struct RandomClipNode;
#[derive(Default)]
struct GateState {
	seen: VecDeque<String>,
	channels: HashMap<String, Instant>,
}
fn valid_login(s: &str) -> bool {
	!s.is_empty() && s.len() <= 25 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
fn command(meta: &Value) -> Option<String> {
	if meta["transport"] != "twitch.irc" {
		return None;
	}
	let sender = meta["sender_id"].as_str()?;
	let channel = meta["broadcaster_id"].as_str()?;
	if sender.is_empty() || channel.is_empty() || (sender != channel && meta["is_moderator"].as_bool() != Some(true)) {
		return None;
	}
	let mut words = meta["message_text"].as_str()?.split_whitespace();
	if !matches!(words.next()?, "!rc" | "!randomclip") {
		return None;
	}
	let login = words.next().unwrap_or("").trim_start_matches('@').to_ascii_lowercase();
	// Empty login deliberately routes an authorized syntax error to the error branch.
	Some(if valid_login(&login) && words.next().is_none() {
		login
	} else {
		String::new()
	})
}
impl NodeDescriptor for ClipCommandNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
 feature:"flowgraph.twitch.clip_command".into(), title:"Twitch: Moderator Clip Command".into(), category:"twitch".into(),
 description:Some("Twitch IRCの配信者/モデレーターによる !rc / !randomclip を受付。本文から権限を推定しない。ノード内でID重複とチャンネル毎5秒の連打を抑止。".into()),
 inputs:vec![PortSpec::exec_input("exec_in","Exec"),PortSpec::input("meta","IRC Meta",SocketType::Json)],
 outputs:vec![PortSpec::exec_output("on_command","On Command"),PortSpec::exec_output("on_skip","On Skip"),PortSpec::output("login","Target Login",SocketType::String),PortSpec::output("broadcaster_id","Chat Channel ID",SocketType::String)], properties:vec![] }
	}
}
#[async_trait]
impl StatefulNode for ClipCommandNode {
	fn init_state(&self) -> Box<dyn Any + Send> {
		Box::<GateState>::default()
	}
	async fn compute(
		&self,
		state: &mut (dyn Any + Send),
		_props: &InputMap,
		inputs: &InputMap,
		fired: &ExecFireSet,
		_ctx: &StatefulCtx<'_>,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired.contains("exec_in") {
			return Ok(NodeOutput::new());
		}
		let meta = get_required_json(inputs, "meta")?;
		let Some(login) = command(&meta) else {
			return Ok(NodeOutput::new().fire_exec("on_skip"));
		};
		let id = meta["message_id"].as_str().unwrap_or("");
		let channel = meta["broadcaster_id"].as_str().unwrap_or("");
		let s = state.downcast_mut::<GateState>().unwrap();
		let now = Instant::now();
		if id.is_empty() || s.seen.iter().any(|x| x == id) {
			return Ok(NodeOutput::new().fire_exec("on_skip"));
		}
		if s.seen.len() >= 4096 {
			s.seen.pop_front();
		}
		s.seen.push_back(id.into());
		s.channels.retain(|_, t| now.duration_since(*t) < Duration::from_secs(5));
		if s.channels.contains_key(channel) {
			return Ok(NodeOutput::new().fire_exec("on_skip"));
		}
		s.channels.insert(channel.into(), now);
		Ok(NodeOutput::new()
			.set_data("login", SocketValue::String(login))
			.set_data("broadcaster_id", SocketValue::String(channel.into()))
			.fire_exec("on_command"))
	}
}
impl NodeDescriptor for RandomClipNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
 feature:"flowgraph.twitch.random_clip".into(),title:"Twitch: Random Clip".into(),category:"twitch".into(),
 description:Some("指定配信者のクリップを取得し候補内から均等抽選。既定は視聴数順の最大100件、max_pagesで最大1000件。送信・再生は下流へ明示配線する。".into()),
 inputs:vec![PortSpec::exec_input("exec_in","Exec"),PortSpec::input("login","Login",SocketType::String),PortSpec::input("access_token","Access Token",SocketType::String),PortSpec::input("client_id","Client ID",SocketType::String),PortSpec::input("endpoint","Endpoint",SocketType::String).with_default(SocketValue::String(String::new()))],
 outputs:vec![PortSpec::exec_output("on_success","On Success"),PortSpec::exec_output("on_error","On Error"),PortSpec::output("content","Overlay Title",SocketType::String),PortSpec::output("chat_text","Chat Notice",SocketType::String),PortSpec::output("error","Error",SocketType::String),PortSpec::output("clip","Clip",SocketType::Json),PortSpec::output("meta","Overlay Meta",SocketType::Json),PortSpec::output("flags","Overlay Flags",SocketType::Json)],
 properties:vec![PropertySpec::new("max_pages","Candidate Pages (100 each)",SocketType::Int,SocketValue::Int(1))] }
	}
}
fn failure(reason: &str) -> NodeOutput {
	NodeOutput::new()
		.set_data("error", SocketValue::String(reason.into()))
		.set_data("chat_text", SocketValue::String(reason.into()))
		.fire_exec("on_error")
}
fn eligible(v: &Value, uid: &str) -> bool {
	let id = v["id"].as_str().unwrap_or("");
	let duration = v["duration"].as_f64().unwrap_or(0.0);
	v["broadcaster_id"].as_str() == Some(uid)
		&& !id.is_empty()
		&& id.len() <= 200
		&& id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
		&& duration.is_finite()
		&& (1.0..=180.0).contains(&duration)
}
async fn fetch(client: &reqwest::Client, base: &str, path: &str, token: &str, cid: &str) -> Result<Value, String> {
	let r = client
		.get(format!("{base}{path}"))
		.bearer_auth(token)
		.header("Client-Id", cid)
		.send()
		.await
		.map_err(|_| "クリップ情報の取得に失敗した。少し待って再試行してほしい。".to_string())?;
	if !r.status().is_success() {
		return Err(format!("クリップ情報を取得できなかった（HTTP {}）。", r.status().as_u16()));
	}
	r.json().await.map_err(|_| "クリップ情報の応答を読み取れなかった。".into())
}
async fn lookup(inputs: &InputMap, props: &InputMap) -> Result<NodeOutput, String> {
	let login = get_required_string(inputs, "login").map_err(|e| e.to_string())?;
	if !valid_login(&login) {
		return Err("使い方: !rc Twitchユーザー名 / !randomclip Twitchユーザー名".into());
	}
	let token = get_required_string(inputs, "access_token").map_err(|e| e.to_string())?;
	let cid = get_required_string(inputs, "client_id").map_err(|e| e.to_string())?;
	let endpoint = get_optional_string(inputs, "endpoint", "").map_err(|e| e.to_string())?;
	let base = if endpoint.is_empty() {
		"https://api.twitch.tv/helix"
	} else {
		endpoint.trim_end_matches('/')
	};
	let client = reqwest::Client::builder()
		.timeout(Duration::from_secs(10))
		.build()
		.map_err(|_| "通信を準備できなかった。".to_string())?;
	let users = fetch(&client, base, &format!("/users?login={login}"), &token, &cid).await?;
	let uid = users["data"][0]["id"]
		.as_str()
		.filter(|s| !s.is_empty())
		.ok_or_else(|| format!("{login}さんが見つからなかった。"))?;
	let pages = get_optional_int(props, "max_pages", 1).unwrap_or(1).clamp(1, 10);
	let mut clips = Vec::new();
	let mut cursor = String::new();
	let mut seen = std::collections::HashSet::new();
	for _ in 0..pages {
		let data = fetch(
			&client,
			base,
			&format!(
				"/clips?broadcaster_id={}&first=100&after={}",
				urlencoding::encode(uid),
				urlencoding::encode(&cursor)
			),
			&token,
			&cid,
		)
		.await?;
		if let Some(rows) = data["data"].as_array() {
			for row in rows {
				if eligible(row, uid) && seen.insert(row["id"].as_str().unwrap().to_string()) {
					clips.push(row.clone());
				}
			}
		}
		let next = data["pagination"]["cursor"].as_str().unwrap_or("");
		if next.is_empty() || next == cursor {
			break;
		}
		cursor = next.into();
	}
	if clips.is_empty() {
		return Err(format!("{login}さんの再生できるクリップが見つからなかった。"));
	}
	let clip = clips.swap_remove(rand::random_range(0..clips.len()));
	let id = clip["id"].as_str().unwrap();
	let title = clip["title"]
		.as_str()
		.unwrap_or("Clip")
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ");
	let title: String = title.chars().take(180).collect();
	let duration = (clip["duration"].as_f64().unwrap() * 1000.0).ceil() as u64;
	Ok(NodeOutput::new()
		.set_data("content", SocketValue::String(format!("{login} — {title}")))
		.set_data(
			"chat_text",
			SocketValue::String(format!("{login}さんのクリップ紹介: {title} https://clips.twitch.tv/{id}")),
		)
		.set_data("error", SocketValue::String(String::new()))
		.set_data(
			"meta",
			SocketValue::Json(json!({"clip_id":id,"clip_duration_ms":duration,"quick_effect_label":"CLIP"})),
		)
		.set_data("flags", SocketValue::Json(json!(["obs.clip"])))
		.set_data("clip", SocketValue::Json(clip))
		.fire_exec("on_success"))
}
#[async_trait]
impl EffectfulNode for RandomClipNode {
	async fn execute(
		&self,
		_ctx: &mut ExecCtx,
		props: &InputMap,
		inputs: &InputMap,
		fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired.contains("exec_in") {
			return Ok(NodeOutput::new());
		}
		Ok(match tokio::time::timeout(Duration::from_secs(45), lookup(inputs, props)).await {
			Ok(Ok(out)) => out,
			Ok(Err(e)) => failure(&e),
			Err(_) => failure("クリップ取得が時間切れになった。少し待って再試行してほしい。"),
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[tokio::test]
	async fn duplicate_and_channel_cooldown_do_not_refire() {
		let node=ClipCommandNode; let mut state=node.init_state(); let ctx=StatefulCtx{node_id:"gate",trigger:None};
		let mut inputs=InputMap::new(); let mut fired=ExecFireSet::new(); fired.insert("exec_in");
		let mut meta=json!({"transport":"twitch.irc","sender_id":"1","broadcaster_id":"1","message_id":"a","message_text":"!rc example"});
		for (id,channel,expected) in [("a","1",true),("a","1",false),("b","1",false),("c","2",true)] {
			meta["message_id"]=json!(id);meta["broadcaster_id"]=json!(channel);meta["sender_id"]=json!(channel);
			inputs.insert("meta".into(),SocketValue::Json(meta.clone()));
			let out=node.compute(state.as_mut(),&InputMap::new(),&inputs,&fired,&ctx).await.unwrap();
			assert_eq!(out.fired_exec.contains("on_command"),expected);
		}
	}
	#[test]
	fn command_requires_actual_role_and_exact_syntax() {
		let mut m =
			json!({"transport":"twitch.irc","sender_id":"2","broadcaster_id":"1","is_moderator":false,"message_text":"!rc example"});
		assert_eq!(command(&m), None);
		m["is_moderator"] = json!(true);
		assert_eq!(command(&m), Some("example".into()));
		m["message_text"] = json!("!randomclip @EXAMPLE");
		assert_eq!(command(&m), Some("example".into()));
		m["is_moderator"] = json!(false);
		m["sender_id"] = json!("1");
		assert_eq!(command(&m), Some("example".into()));
		for s in ["!rc", "!rc https://evil.test", "!rc a b"] {
			m["message_text"] = json!(s);
			assert_eq!(command(&m), Some(String::new()));
		}
		m["message_text"] = json!("name:!rc example");
		assert_eq!(command(&m), None);
		m["message_text"] = json!("!rc example");
		m["transport"] = json!("web");
		assert_eq!(command(&m), None);
	}
	#[test]
	fn clips_require_matching_owner_valid_slug_and_duration() {
		let mut v = json!({"id":"Some_Clip-1","broadcaster_id":"1","duration":30.5});
		assert!(eligible(&v, "1"));
		assert!(!eligible(&v, "2"));
		v["id"] = json!("\" onload=evil");
		assert!(!eligible(&v, "1"));
		v["id"] = json!("Clip");
		v["duration"] = json!(0);
		assert!(!eligible(&v, "1"));
	}
}
