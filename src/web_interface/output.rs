use crate::{resource::CONTENT_TYPE_APPLICATION_JSON, Result, SharedState};
use actix_web::{post, web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// `browser_source.document_root`（既定: `output`）。`GET /output`・`GET /browser-output/*` が参照する。
#[derive(Clone)]
pub struct OutputPaths {
	pub root: PathBuf,
}

#[derive(Deserialize, Debug)]
struct OutputRequestPayload {
	/// 受信したいチャンネルのリスト
	channels: Vec<ChannelRequest>,
}

#[derive(Deserialize, Debug)]
struct ChannelRequest {
	/// 受信したいチャンネル名
	name: String,
	/// 既に取得済みの ID を与えると、その ID 以降の内容を取得します。
	retrieved_id: Option<u64>,
	/// 既に取得済みの iso8601 日時を与えると、その日時以降の内容を取得します。
	retrieved_timestamp: Option<String>,
	/// 取得する最大件数を指定します。
	count: Option<usize>,
}

#[derive(Serialize, Debug)]
struct OutputResponsePayload {
	/// チャンネル名 -> チャンネルデータ
	channel_data: HashMap<String, Vec<ChannelDatum>>,
}

#[derive(Serialize, Debug)]
struct ChannelDatum {
	/// Datum ID
	id: Option<u64>,
	/// ISO8601 日時
	datetime: Option<String>,
	/// チャンネル名
	channel: Option<String>,
	/// 内容
	content: Option<String>,
	/// 表示用の発話者。Twitch chat のユーザー名や AI persona 名に使う。
	source_actor: Option<String>,
	/// 配信前セルフテスト由来。Browser Sourceは通常字幕から除外する。
	is_preflight_probe: bool,
	/// Quick Effectsの表示名。通常のChannelDatumでは省略する。
	#[serde(skip_serializing_if = "Option::is_none")]
	quick_effect_label: Option<String>,
	/// Quick Effectsの表示時間。通常のChannelDatumでは省略する。
	#[serde(skip_serializing_if = "Option::is_none")]
	quick_effect_duration_ms: Option<u64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	clip_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	clip_duration_ms: Option<u64>,
	/// フラグ
	#[serde(default)]
	flags: HashSet<String>,
}

fn project_channel_datum(cd: &crate::ChannelDatum) -> ChannelDatum {
	ChannelDatum {
		id: Some(cd.get_id()),
		datetime: Some(cd.get_datetime().to_string()),
		channel: Some(cd.channel.clone()),
		content: Some(cd.content.clone()),
		source_actor: cd.meta_get("source_actor").and_then(serde_json::Value::as_str).map(str::to_string),
		is_preflight_probe: cd.is_preflight_probe(),
		quick_effect_label: cd
			.meta_get("quick_effect_label")
			.and_then(serde_json::Value::as_str)
			.map(str::to_string),
		quick_effect_duration_ms: cd.meta_get("quick_effect_duration_ms").and_then(serde_json::Value::as_u64),
		clip_id: cd.meta_get("clip_id").and_then(serde_json::Value::as_str).filter(|s| !s.is_empty() && s.len() <= 200 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')).map(str::to_string),
		clip_duration_ms: cd.meta_get("clip_duration_ms").and_then(serde_json::Value::as_u64).filter(|n| (1000..=180000).contains(n)),
		flags: cd.flags.clone(),
	}
}

#[post("/output")]
async fn post(state: web::Data<SharedState>, request_paylaod: web::Json<OutputRequestPayload>) -> Result<impl Responder> {
	// 出力の構造を作成
	let mut output_response = OutputResponsePayload {
		channel_data: HashMap::new(),
	};

	// state から channel_data を取得する
	let state = state.get_ref().read().await;
	let channel_data = state.channel_data.read().await;

	// request_payload から channel_request を1つずつ取り出して、それぞれに対応する channel_data を取得する
	for channel_request in request_paylaod.channels.iter() {
		let mut channel_data_for_response = Vec::new();

		// channel_data から channel_request に対応する channel_data を取得する
		let channel_data_for_request = channel_data
			.iter()
			.filter(|cd| cd.channel == channel_request.name)
			.collect::<Vec<_>>();

		// count が指定されている場合は、後ろから count 個の channel_datum まで channel_data_for_request を進める
		let channel_data_for_request = match channel_request.count {
			Some(count) => channel_data_for_request.into_iter().rev().take(count).rev().collect::<Vec<_>>(),
			None => channel_data_for_request,
		};

		// retrieved_id が指定されている場合は、それより大きなIDの channel_datum まで channel_data_for_request を進める
		let channel_data_for_request = match channel_request.retrieved_id {
			Some(retrieved_id) => channel_data_for_request
				.into_iter()
				.filter(|cd| cd.get_id() > retrieved_id)
				.collect::<Vec<_>>(),
			None => channel_data_for_request,
		};

		// retrieved_timestamp が指定されている場合は、それより新しい timestamp の channel_datum まで channel_data_for_request を進める
		let channel_data_for_request = match &channel_request.retrieved_timestamp {
			Some(retrieved_timestamp) => channel_data_for_request
				.into_iter()
				.filter(|cd| cd.get_datetime() > retrieved_timestamp.parse::<jiff::Timestamp>().unwrap())
				.collect::<Vec<_>>(),
			None => channel_data_for_request,
		};

		// channel_data_for_request を channel_data_for_response にコピーする
		for cd in channel_data_for_request {
			channel_data_for_response.push(project_channel_datum(cd));
		}

		// 出力の構造に channel_data_for_response を追加する
		output_response
			.channel_data
			.insert(channel_request.name.clone(), channel_data_for_response);
	}

	Ok(HttpResponse::Ok().content_type(CONTENT_TYPE_APPLICATION_JSON).json(output_response))
}

#[actix_web::get("/output")]
pub async fn get_index(paths: web::Data<OutputPaths>) -> Result<impl Responder> {
	let path = paths.root.join("index.html");
	match tokio::fs::read_to_string(&path).await {
		Ok(content) => Ok(HttpResponse::Ok().content_type("text/html").body(content)),
		_ => Ok(HttpResponse::NotFound().finish()),
	}
}

#[actix_web::get("/output/{subindex}")]
pub async fn get_subfile(paths: web::Data<OutputPaths>, subindex: web::Path<String>) -> Result<impl Responder> {
	let path = paths.root.join(subindex.as_str()).with_extension("html");
	match tokio::fs::read_to_string(&path).await {
		Ok(content) => Ok(HttpResponse::Ok().content_type("text/html").body(content)),
		_ => Ok(HttpResponse::NotFound().finish()),
	}
}

#[cfg(test)]
mod tests {
	use super::{project_channel_datum, ChannelDatum};
	use crate::state::DataSource;
	use std::collections::HashSet;

	#[test]
	fn output_only_exposes_explicit_quick_effect_fields() {
		let regular = ChannelDatum {
			id: Some(1),
			datetime: None,
			channel: Some("user".into()),
			content: Some("hello".into()),
			source_actor: None,
			is_preflight_probe: false,
			quick_effect_label: None,
			quick_effect_duration_ms: None,
			clip_id: None,
			clip_duration_ms: None,
			flags: HashSet::new(),
		};
		let regular_json = serde_json::to_value(regular).expect("serialize regular output");
		assert!(regular_json.get("quick_effect_label").is_none());
		assert!(regular_json.get("quick_effect_duration_ms").is_none());

		let effect = ChannelDatum {
			id: Some(2),
			datetime: None,
			channel: Some("effect".into()),
			content: Some("flash".into()),
			source_actor: None,
			is_preflight_probe: false,
			quick_effect_label: Some("画面効果".into()),
			quick_effect_duration_ms: Some(2500),
			clip_id: None,
			clip_duration_ms: None,
			flags: HashSet::from(["obs.effect.flash".into()]),
		};
		let effect_json = serde_json::to_value(effect).expect("serialize effect output");
		assert_eq!(effect_json["quick_effect_label"], "画面効果");
		assert_eq!(effect_json["quick_effect_duration_ms"], 2500);
	}

	#[test]
	fn output_projection_marks_preflight_source_without_exposing_it_as_actor() {
		let source = crate::ChannelDatum::new("ai-synth".into(), "UNVAC 字幕出力テスト".into())
			.with_source(DataSource::new("control.preflight").with_actor("gui"));
		let output = serde_json::to_value(project_channel_datum(&source)).expect("serialize projected output");
		assert_eq!(output["is_preflight_probe"], true);
		assert!(output["source_actor"].is_null());
	}
}
