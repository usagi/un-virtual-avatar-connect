//! Configuration-free speaker discovery, independent of the retired V1 processors.
use anyhow::{Context, Result};
use serde_json::Value;

pub(super) async fn fetch(endpoint: &str) -> Result<Value> {
	let url = format!("{}/speakers", endpoint.trim_end_matches('/'));
	let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(10)).build()?;
	let value: Value = client
		.get(&url)
		.send()
		.await
		.with_context(|| format!("{url} に接続できません。音声エンジンを起動してください"))?
		.error_for_status()
		.with_context(|| format!("GET {url}"))?
		.json()
		.await
		.context("話者一覧がJSONではありません")?;
	parse(value)
}

fn parse(value: Value) -> Result<Value> {
	// Preserve engine-specific UUIDs, global/local style IDs, and labels verbatim.
	// VOICEVOX and COEIROINK have different field names and voice-selection syntax.
	let array = value
		.as_array()
		.or_else(|| value.get("speakers").and_then(Value::as_array))
		.context("話者一覧は配列または speakers 配列である必要があります")?;
	for (index, speaker) in array.iter().enumerate() {
		let (name_key, uuid_key, style_name_key, style_id_key) = if speaker.get("speakerUuid").is_some() {
			("speakerName", "speakerUuid", "styleName", "styleId")
		} else {
			("name", "speaker_uuid", "name", "id")
		};
		anyhow::ensure!(
			speaker.get(name_key).and_then(Value::as_str).is_some() && speaker.get(uuid_key).and_then(Value::as_str).is_some(),
			"話者一覧の項目{index}に名前またはUUIDがありません"
		);
		let styles = speaker
			.get("styles")
			.and_then(Value::as_array)
			.with_context(|| format!("話者一覧の項目{index}にstyles配列がありません"))?;
		anyhow::ensure!(
			styles
				.iter()
				.all(|style| style.get(style_name_key).and_then(Value::as_str).is_some()
					&& style.get(style_id_key).and_then(Value::as_u64).is_some()),
			"話者一覧の項目{index}に不正なスタイルがあります"
		);
	}
	Ok(Value::Array(array.clone()))
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn retains_engine_specific_voice_ids() {
		for speaker in [
			serde_json::json!({"name":"voice", "speaker_uuid":"uuid", "styles":[{"id":100,"name":"normal"}]}),
			serde_json::json!({"speakerName":"voice", "speakerUuid":"uuid", "styles":[{"styleId":7,"styleName":"normal"}]}),
		] {
			let expected = serde_json::json!([speaker]);
			assert_eq!(parse(expected.clone()).unwrap(), expected);
			assert_eq!(parse(serde_json::json!({"speakers":expected})).unwrap(), expected);
		}
	}
	#[test]
	fn rejects_error_payloads_but_accepts_empty_inventory() {
		for value in [
			serde_json::json!({"error":"offline"}),
			serde_json::json!([null]),
			serde_json::json!([{"error":"offline"}]),
			serde_json::json!([{"name":"voice","speaker_uuid":"uuid","styles":[{"name":"normal","id":"wrong"}]}]),
			serde_json::json!("error"),
		] {
			assert!(parse(value).is_err());
		}
		assert_eq!(parse(serde_json::json!([])).unwrap(), serde_json::json!([]));
	}
}
