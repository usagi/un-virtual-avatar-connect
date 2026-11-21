//! Turn Manager の strict JSON stream から、発話開始に必要な情報だけを逐次復号する。
//!
//! 最終的な正当性は従来どおり `serde_json` でJSON全体を検証する。このモジュールは、
//! それより前に確定した `action` と `speak` の接頭辞を安全に取り出すための前段。

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnStreamUpdate {
	pub action: Option<String>,
	pub speak: String,
	pub speak_delta: String,
	pub speak_complete: bool,
	pub speak_is_null: bool,
}

#[derive(Debug, Default)]
pub(crate) struct TurnStreamDecoder {
	raw: String,
	action: Option<String>,
	speak: String,
	speak_complete: bool,
	speak_is_null: bool,
}

impl TurnStreamDecoder {
	pub(crate) fn push(&mut self, delta: &str) -> Result<TurnStreamUpdate, String> {
		self.raw.push_str(delta);

		if self.action.is_none() {
			if let Some(value) = extract_string_property(&self.raw, "action")? {
				if value.complete {
					self.action = Some(value.decoded);
				}
			}
		}

		let mut speak_delta = String::new();
		if !self.speak_complete {
			match extract_nullable_string_property(&self.raw, "speak")? {
				NullableStringPrefix::Pending => {}
				NullableStringPrefix::Null => {
					self.speak_complete = true;
					self.speak_is_null = true;
				}
				NullableStringPrefix::String(value) => {
					if !value.decoded.starts_with(&self.speak) {
						return Err("Turn Manager の speak stream が既に復号した接頭辞を書き換えました".to_string());
					}
					speak_delta.push_str(&value.decoded[self.speak.len()..]);
					self.speak = value.decoded;
					self.speak_complete = value.complete;
				}
			}
		}

		Ok(TurnStreamUpdate {
			action: self.action.clone(),
			speak: self.speak.clone(),
			speak_delta,
			speak_complete: self.speak_complete,
			speak_is_null: self.speak_is_null,
		})
	}

	#[cfg(test)]
	fn speak(&self) -> &str {
		&self.speak
	}
}

#[derive(Debug)]
struct StringPrefix {
	decoded: String,
	complete: bool,
}

enum NullableStringPrefix {
	Pending,
	Null,
	String(StringPrefix),
}

fn extract_string_property(raw: &str, key: &str) -> Result<Option<StringPrefix>, String> {
	let Some(value_start) = property_value_start(raw, key) else {
		return Ok(None);
	};
	let tail = &raw[value_start..];
	if !tail.starts_with('"') {
		return Ok(None);
	}
	decode_string_prefix(tail).map(Some)
}

fn extract_nullable_string_property(raw: &str, key: &str) -> Result<NullableStringPrefix, String> {
	let Some(value_start) = property_value_start(raw, key) else {
		return Ok(NullableStringPrefix::Pending);
	};
	let tail = &raw[value_start..];
	if tail.starts_with("null") {
		return Ok(NullableStringPrefix::Null);
	}
	if tail.starts_with('"') {
		return decode_string_prefix(tail).map(NullableStringPrefix::String);
	}
	Ok(NullableStringPrefix::Pending)
}

fn property_value_start(raw: &str, key: &str) -> Option<usize> {
	let bytes = raw.as_bytes();
	let mut depth = 0usize;
	let mut i = 0usize;
	while i < bytes.len() {
		match bytes[i] {
			b'{' | b'[' => depth += 1,
			b'}' | b']' => depth = depth.saturating_sub(1),
			b'"' => {
				// An incomplete string cannot contain a later top-level key.
				let end = find_unescaped_quote(raw, i + 1)?;
				let after = end + 1;
				let colon = after + raw[after..].len() - raw[after..].trim_start().len();
				if depth == 1 && bytes.get(colon) == Some(&b':')
					&& serde_json::from_str::<String>(&raw[i..after]).ok().as_deref() == Some(key)
				{
					let value = colon + 1;
					return Some(value + raw[value..].len() - raw[value..].trim_start().len());
				}
				i = end;
			}
			_ => {}
		}
		i += 1;
	}
	None
}

fn decode_string_prefix(tail: &str) -> Result<StringPrefix, String> {
	debug_assert!(tail.starts_with('"'));
	let body_start = 1;
	let close = find_unescaped_quote(tail, body_start);
	let body_end = close.unwrap_or(tail.len());
	let body = &tail[body_start..body_end];

	let decoded = decode_longest_valid_json_string_body(body)?;
	Ok(StringPrefix {
		decoded,
		complete: close.is_some(),
	})
}

fn find_unescaped_quote(value: &str, start: usize) -> Option<usize> {
	let bytes = value.as_bytes();
	let mut i = start;
	while i < bytes.len() {
		if bytes[i] == b'"' {
			let mut slashes = 0usize;
			let mut j = i;
			while j > start && bytes[j - 1] == b'\\' {
				slashes += 1;
				j -= 1;
			}
			if slashes % 2 == 0 {
				return Some(i);
			}
		}
		i += 1;
	}
	None
}

fn decode_longest_valid_json_string_body(body: &str) -> Result<String, String> {
	let mut end = body.len();
	loop {
		if body.is_char_boundary(end) {
			let candidate = format!("\"{}\"", &body[..end]);
			if let Ok(decoded) = serde_json::from_str::<String>(&candidate) {
				return Ok(decoded);
			}
		}
		if end == 0 {
			return Ok(String::new());
		}
		end -= 1;
	}
}

#[derive(Debug, Default)]
pub(crate) struct SpeechSegmenter {
	pending: String,
}

impl SpeechSegmenter {
	pub(crate) fn push(&mut self, delta: &str, final_chunk: bool) -> Vec<String> {
		self.pending.push_str(delta);
		let mut segments = Vec::new();

		while let Some(end) = self.next_boundary() {
			let segment = self.pending[..end].trim().to_string();
			self.pending.drain(..end);
			if !segment.is_empty() {
				segments.push(segment);
			}
		}

		if final_chunk {
			let tail = self.pending.trim().to_string();
			self.pending.clear();
			if !tail.is_empty() {
				segments.push(tail);
			}
		}

		segments
	}

	fn next_boundary(&self) -> Option<usize> {
		// Even a short complete reply is a useful point to yield to the human.
		// Do not merge it into an unfinished clause merely to reach a minimum length.
		let min_chars = 2;
		let mut count = 0usize;
		let mut soft_boundary = None;
		let mut hard_boundary = None;

		for (index, ch) in self.pending.char_indices() {
			count += 1;
			let end = index + ch.len_utf8();
			if count >= min_chars && matches!(ch, '。' | '！' | '？' | '!' | '?') {
				return soft_boundary.or(Some(end));
			}
			if count >= 18 && soft_boundary.is_none() && matches!(ch, '、' | '；' | ';') {
				soft_boundary = Some(end);
			}
			if count >= 48 && hard_boundary.is_none() {
				hard_boundary = Some(end);
			}
		}

		soft_boundary.or(hard_boundary)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn metadata_values_and_nested_keys_never_become_speech() {
		for metadata in [
			r#""episode_memory":"speak","avatar_pose":null"#,
			r#""about":{"referent":"speak","world":"game_world"}"#,
			r#""about":{"action":"observe","speak":"誤った発話。"}"#,
			r#""visual_notes":[{"speak":"誤った発話。","evidence":"引用: \"speak\": \"別の値\""}]"#,
		] {
			let prefix = format!("{{{metadata},");
			let mut decoder = TurnStreamDecoder::default();
			for ch in prefix.chars() {
				let update = decoder.push(&ch.to_string()).unwrap();
				assert!(update.action.is_none());
				assert!(update.speak.is_empty());
				assert!(!update.speak_complete);
			}
			let mut speech = String::new();
			for ch in r#""action":"respond","speak":"本当の発話。"}"#.chars() {
				speech.push_str(&decoder.push(&ch.to_string()).unwrap().speak_delta);
			}
			assert_eq!(speech, "本当の発話。");
			assert_eq!(decoder.action.as_deref(), Some("respond"));
		}
	}

	#[test]
	fn decodes_action_and_speak_across_character_deltas() {
		let json = r#"{"action":"respond","speak":"了解だ。次へ進む。","episode_memory":"進行確認"}"#;
		let mut decoder = TurnStreamDecoder::default();
		let mut speech = String::new();
		let mut last = None;

		for ch in json.chars() {
			let update = decoder.push(&ch.to_string()).unwrap();
			speech.push_str(&update.speak_delta);
			last = Some(update);
		}

		assert_eq!(decoder.speak(), "了解だ。次へ進む。");
		assert_eq!(speech, "了解だ。次へ進む。");
		assert_eq!(last.unwrap().action.as_deref(), Some("respond"));
	}

	#[test]
	fn withholds_incomplete_escape_and_decodes_it_once_complete() {
		let mut decoder = TurnStreamDecoder::default();
		let first = decoder.push(r#"{"action":"react","speak":"引用は\u30"#).unwrap();
		assert_eq!(first.speak_delta, "引用は");

		let second = decoder.push(r#"ef\u30fc\u30c9\"だ。","episode_memory":null}"#).unwrap();
		assert_eq!(second.speak_delta, "ワード\"だ。");
		assert!(second.speak_complete);
		assert_eq!(decoder.speak(), "引用はワード\"だ。");
	}

	#[test]
	fn observes_null_speak_without_audio_text() {
		let mut decoder = TurnStreamDecoder::default();
		let update = decoder.push(r#"{"action":"observe","speak":null,"episode_memory":null}"#).unwrap();
		assert_eq!(update.action.as_deref(), Some("observe"));
		assert!(update.speak_is_null);
		assert!(update.speak_complete);
		assert!(update.speak_delta.is_empty());
	}

	#[test]
	fn segments_on_natural_sentence_boundaries() {
		let mut segmenter = SpeechSegmenter::default();
		assert!(segmenter.push("この進め方なら", false).is_empty());
		assert_eq!(segmenter.push("問題ない。", false), vec!["この進め方なら問題ない。"]);
		assert_eq!(segmenter.push("次へ進む。", true), vec!["次へ進む。"]);
	}

	#[test]
	fn uses_long_comma_as_a_soft_boundary() {
		let mut segmenter = SpeechSegmenter::default();
		let segments = segmenter.push("ここまでの情報を整理すると方針は妥当だが、まだ確認が必要だ。", false);
		assert_eq!(segments, vec!["ここまでの情報を整理すると方針は妥当だが、", "まだ確認が必要だ。"]);
	}

	#[test]
	fn flushes_short_tail_only_when_final() {
		let mut segmenter = SpeechSegmenter::default();
		assert!(segmenter.push("了解", false).is_empty());
		assert_eq!(segmenter.push("", true), vec!["了解"]);
	}

	#[test]
	fn short_complete_reply_yields_before_the_following_clause_across_chunk_sizes() {
		let text = "その通りだ。余計な主張は脇に置いて、今はゲームを楽しもう。";
		let chars: Vec<char> = text.chars().collect();
		for chunk_size in [1, 4, chars.len()] {
			let mut segmenter = SpeechSegmenter::default();
			let mut segments = Vec::new();
			for chunk in chars.chunks(chunk_size) {
				segments.extend(segmenter.push(&chunk.iter().collect::<String>(), false));
			}
			segments.extend(segmenter.push("", true));
			assert_eq!(segments, ["その通りだ。", "余計な主張は脇に置いて、今はゲームを楽しもう。"]);
			assert_eq!(segments.concat(), text);
		}
	}

	#[test]
	fn short_question_is_available_before_more_text_arrives() {
		let mut segmenter = SpeechSegmenter::default();
		assert_eq!(segmenter.push("何？", false), ["何？"]);
		assert!(segmenter.push("もう一度", false).is_empty());
		assert_eq!(segmenter.push("聞かせてくれ。", true), ["もう一度聞かせてくれ。"]);
	}
}
