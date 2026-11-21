//! ARPAbet発音列を、日本語TTSが読める近似カタカナへ変換する。
//!
//! `jiroshimaya/arpakana` revision
//! `203c274f86ce7808f76ec0cc3fbd758354ee08c1` (MIT) の規則をRust向けに移植。
//! 上流ライセンスはrelease資産 `LICENSE.arpakana` に同梱する。

use std::collections::HashMap;
use std::sync::LazyLock;

static CV: LazyLock<HashMap<&'static str, [&'static str; 5]>> = LazyLock::new(|| {
	HashMap::from([
		("", ["ア", "イ", "ウ", "エ", "オ"]),
		("B", ["バ", "ビ", "ブ", "ベ", "ボ"]),
		("CH", ["チャ", "チ", "チュ", "チェ", "チョ"]),
		("D", ["ダ", "ディ", "ドゥ", "デ", "ド"]),
		("DH", ["ダ", "ディ", "ドゥ", "デ", "ド"]),
		("DX", ["ラ", "リ", "ル", "レ", "ロ"]),
		("F", ["ファ", "フィ", "フ", "フェ", "フォ"]),
		("G", ["ガ", "ギ", "グ", "ゲ", "ゴ"]),
		("HH", ["ハ", "ヒ", "フ", "ヘ", "ホ"]),
		("JH", ["ジャ", "ジ", "ジュ", "ジェ", "ジョ"]),
		("K", ["カ", "キ", "ク", "ケ", "コ"]),
		("L", ["ラ", "リ", "ル", "レ", "ロ"]),
		("M", ["マ", "ミ", "ム", "メ", "モ"]),
		("N", ["ナ", "ニ", "ヌ", "ネ", "ノ"]),
		("NG", ["ンガ", "ンギ", "ング", "ンゲ", "ンゴ"]),
		("NX", ["ナ", "ニ", "ヌ", "ネ", "ノ"]),
		("P", ["パ", "ピ", "プ", "ペ", "ポ"]),
		("R", ["ラ", "リ", "ル", "レ", "ロ"]),
		("S", ["サ", "シ", "ス", "セ", "ソ"]),
		("SH", ["シャ", "シ", "シュ", "シェ", "ショ"]),
		("T", ["タ", "ティ", "トゥ", "テ", "ト"]),
		("TH", ["サ", "シ", "ス", "セ", "ソ"]),
		("V", ["ヴァ", "ヴィ", "ヴ", "ヴェ", "ヴォ"]),
		("W", ["ワ", "ウィ", "ウ", "ウェ", "ウォ"]),
		("Y", ["ヤ", "イ", "ユ", "イェ", "ヨ"]),
		("Z", ["ザ", "ズィ", "ズ", "ゼ", "ゾ"]),
		("ZH", ["ジャ", "ジ", "ジュ", "ジェ", "ジョ"]),
		("K Y", ["キャ", "キィ", "キュ", "キェ", "キョ"]),
		("G Y", ["ギャ", "ギィ", "ギュ", "ギェ", "ギョ"]),
		("S Y", ["シャ", "シィ", "シュ", "シェ", "ショ"]),
		("Z Y", ["ジャ", "ジィ", "ジュ", "ジェ", "ジョ"]),
		("T Y", ["チャ", "チィ", "チュ", "チェ", "チョ"]),
		("D Y", ["ジャ", "ジィ", "ジュ", "ジェ", "ジョ"]),
		("H Y", ["ヒャ", "ヒィ", "ヒュ", "ヒェ", "ヒョ"]),
		("B Y", ["ビャ", "ビィ", "ビュ", "ビェ", "ビョ"]),
		("P Y", ["ピャ", "ピィ", "ピュ", "ピェ", "ピョ"]),
		("M Y", ["ミャ", "ミィ", "ミュ", "ミェ", "ミョ"]),
		("R Y", ["リャ", "リィ", "リュ", "リェ", "リョ"]),
		("L Y", ["リャ", "リィ", "リュ", "リェ", "リョ"]),
		("N Y", ["ニャ", "ニィ", "ニュ", "ニェ", "ニョ"]),
		("F Y", ["フャ", "フィ", "フュ", "フェ", "フョ"]),
		("T S", ["ツァ", "ツィ", "ツ", "ツェ", "ツォ"]),
	])
});

fn vowel_expansion(token: &str) -> Option<&'static [&'static str]> {
	Some(match token {
		"AA" | "AE" | "AH" | "AX" => &["a"],
		"AO" => &["o"],
		"AW" => &["a", "ウ"],
		"AY" => &["a", "イ"],
		"EH" => &["e"],
		"EY" => &["e", "イ"],
		"IH" | "IX" => &["i"],
		"IY" => &["i", "ー"],
		"OW" => &["o", "ウ"],
		"OY" => &["o", "イ"],
		"OH" => &["o", "ー"],
		"UH" | "UX" => &["u"],
		"UW" => &["u", "ー"],
		_ => return None,
	})
}

fn vowel_index(token: &str) -> Option<usize> {
	match token {
		"a" => Some(0),
		"i" => Some(1),
		"u" => Some(2),
		"e" => Some(3),
		"o" => Some(4),
		_ => None,
	}
}

fn is_vowel(token: &str) -> bool {
	vowel_index(token).is_some()
}

fn standalone(token: &str) -> Option<&'static str> {
	Some(match token {
		"B" => "ブ",
		"CH" => "チ",
		"D" => "ド",
		"DH" => "ズ",
		"DX" => "ル",
		"F" => "フ",
		"G" => "グ",
		"JH" => "ジ",
		"K" => "ク",
		"L" => "ル",
		"M" | "N" | "NX" => "ン",
		// 語末 -ing を日本語TTSで脱落させないため、上流の「ン」より明示的にする。
		"NG" => "ング",
		"P" => "プ",
		"S" => "ス",
		"SH" => "シュ",
		"T" => "ト",
		"TH" => "ス",
		"V" => "ヴ",
		"Z" => "ズ",
		"ZH" => "ジュ",
		_ => return None,
	})
}

fn is_kana(token: &str) -> bool {
	!token.is_empty() && token.chars().all(|ch| ('\u{30a0}'..='\u{30ff}').contains(&ch) || ch == 'ー')
}

/// CMUdictの空白区切りARPAbetを近似カタカナへ変換する。
pub(crate) fn arpabet_to_kana(phonemes: &str) -> Option<String> {
	let mut normalized = Vec::new();
	for raw in phonemes.split_ascii_whitespace() {
		let token = raw.trim_end_matches(|ch: char| ch.is_ascii_digit()).to_ascii_uppercase();
		if matches!(token.as_str(), "" | "SIL" | "SP" | "SPN") {
			continue;
		}
		if matches!(token.as_str(), "ER" | "AXR") {
			normalized.extend(["AX".to_string(), "R".to_string()]);
		} else {
			normalized.push(token);
		}
	}
	let mut tokens = Vec::new();
	for token in normalized {
		if let Some(expanded) = vowel_expansion(&token) {
			tokens.extend(expanded.iter().map(|value| (*value).to_string()));
		} else {
			tokens.push(token);
		}
	}
	let mut with_sokuon = Vec::new();
	for index in 0..tokens.len() {
		let cluster = matches!(tokens[index].as_str(), "CH" | "SH" | "JH" | "ZH")
			|| (tokens[index] == "T" && tokens.get(index + 1).is_some_and(|next| next == "S"));
		if cluster && index > 0 && is_vowel(&tokens[index - 1]) {
			with_sokuon.push("ッ".to_string());
		}
		with_sokuon.push(tokens[index].clone());
	}
	let tokens = with_sokuon;
	let mut out = Vec::new();
	let mut index = 0;
	while index < tokens.len() {
		let mut matched = false;
		for consonants in [2_usize, 1, 0] {
			let vowel_at = index + consonants;
			let Some(vowel) = tokens.get(vowel_at).and_then(|token| vowel_index(token)) else {
				continue;
			};
			let key = match consonants {
				0 => "".to_string(),
				1 => tokens[index].clone(),
				2 => format!("{} {}", tokens[index], tokens[index + 1]),
				_ => unreachable!(),
			};
			if let Some(table) = CV.get(key.as_str()) {
				out.push(table[vowel].to_string());
				index = vowel_at + 1;
				matched = true;
				break;
			}
		}
		if matched {
			continue;
		}
		let token = &tokens[index];
		if token == "R" {
			let previous = index.checked_sub(1).and_then(|at| tokens.get(at)).map(String::as_str);
			if matches!(previous, Some("a" | "o" | "ー")) {
				if previous != Some("ー") {
					out.push("ー".to_string());
				}
			} else {
				out.push("ア".to_string());
			}
			index += 1;
			continue;
		}
		if token == "T" && tokens.get(index + 1).is_some_and(|next| next == "S") {
			out.push("ツ".to_string());
			index += 2;
			continue;
		}
		if is_kana(token) {
			out.push(token.clone());
			index += 1;
			continue;
		}
		out.push(standalone(token)?.to_string());
		index += 1;
	}
	let result = out.concat();
	(!result.is_empty()).then_some(result)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn common_arpabet_words_are_recognizable_to_japanese_tts() {
		assert_eq!(arpabet_to_kana("HH AH0 L OW1").as_deref(), Some("ハロウ"));
		assert_eq!(arpabet_to_kana("M EH1 N Y UW0").as_deref(), Some("メニュー"));
		assert_eq!(arpabet_to_kana("HH AY1").as_deref(), Some("ハイ"));
		assert_eq!(arpabet_to_kana("B R IY1 F IH0 NG").as_deref(), Some("ブリーフィング"));
		assert_eq!(arpabet_to_kana("K Y UW1").as_deref(), Some("キュー"));
	}

	#[test]
	fn unknown_phoneme_fails_instead_of_emitting_garbage() {
		assert_eq!(arpabet_to_kana("XX YY"), None);
	}
}
