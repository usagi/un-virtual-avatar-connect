use std::{
	collections::HashMap,
	env, fs,
	io::{self, BufRead, Write},
	path::{Path, PathBuf},
	sync::LazyLock,
};

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tract_onnx::prelude::*;

mod arpakana;

const MAX_ENGLISH: usize = 29;
const MAX_KANA: usize = 36;
const KANA_CLASSES: usize = 89;
const START_ID: usize = 1;
const END_ID: usize = 0;

static ENGLISH_SPAN: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"[A-Za-z]+(?:_[A-Za-z]+)*_[0-9]+|[A-Za-z]+(?:['’-][A-Za-z]+)*").expect("valid English or username span regex")
});
static USERNAME_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z]+(?:_[A-Za-z]+)*_[0-9]+$").expect("valid username id regex"));
static DOMAIN_LIKE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)[a-z0-9_-]+\.[a-z0-9]{1,12}").expect("valid domain or file regex"));

type Model = std::sync::Arc<RunnableModel<TypedFact, Box<dyn TypedOp>>>;

#[derive(Debug, Deserialize)]
struct Request {
	id: u64,
	content: String,
	min_confidence: f32,
}

#[derive(Debug, Serialize)]
struct Response {
	id: u64,
	ok: bool,
	result: Option<ConversionResult>,
	error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ConversionTrace {
	source: String,
	candidate: Option<String>,
	confidence: Option<f32>,
	accepted: bool,
	reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ConversionResult {
	text: String,
	converted_count: usize,
	spans: Vec<ConversionTrace>,
}

struct English2Kana {
	model: Model,
	english_word_index: HashMap<char, usize>,
	kana_index_word: HashMap<usize, String>,
	known_readings: HashMap<String, KnownReading>,
	jmdict_readings: HashMap<String, Vec<JmdictReading>>,
	cmudict: HashMap<String, String>,
}

#[derive(Debug)]
struct KnownReading {
	text: String,
	votes: usize,
}

#[derive(Debug)]
struct JmdictReading {
	text: String,
	common: bool,
}

#[derive(Debug)]
struct TranslationCandidate {
	text: String,
	confidence: f32,
	reason: String,
}

impl English2Kana {
	fn load(model_dir: &Path) -> Result<Self> {
		let model_path = model_dir.join("english2kana-v1.onnx");
		let model = tract_onnx::onnx()
			.model_for_path(&model_path)
			.with_context(|| format!("ONNXモデルを開けない: {}", model_path.display()))?
			.with_input_fact(0, f32::fact([1, MAX_ENGLISH]).into())?
			.with_input_fact(1, f32::fact([1, MAX_KANA]).into())?
			.into_optimized()?
			.into_runnable()?;
		let english_word_index = load_word_index(&model_dir.join("tokenizer_english.json"))?
			.into_iter()
			.filter_map(|(token, id)| token.chars().next().filter(|_| token.chars().count() == 1).map(|ch| (ch, id)))
			.collect();
		let kana_index_word = load_index_word(&model_dir.join("tokenizer_kana.json"))?;
		let known_readings = load_known_readings(&model_dir.join("known-readings.tsv"))?;
		let jmdict_readings = load_jmdict_readings(&model_dir.join("jmdict-readings.tsv"))?;
		let cmudict = load_cmudict(&model_dir.join("cmudict.dict"))?;
		Ok(Self {
			model,
			english_word_index,
			kana_index_word,
			known_readings,
			jmdict_readings,
			cmudict,
		})
	}

	fn translate_onnx(&self, word: &str) -> Result<(String, f32)> {
		if word.is_empty() || word.len() > MAX_ENGLISH || !word.bytes().all(|byte| byte.is_ascii_alphabetic()) {
			return Err(anyhow!("入力は1..={MAX_ENGLISH}文字のASCII英字でなければならない"));
		}
		let mut encoder = vec![0.0_f32; MAX_ENGLISH];
		for (index, ch) in word.to_ascii_lowercase().chars().enumerate() {
			encoder[index] = *self
				.english_word_index
				.get(&ch)
				.ok_or_else(|| anyhow!("英字Tokenizerに {ch:?} がない"))? as f32;
		}
		let mut ids = vec![START_ID];
		let mut probabilities: Vec<f32> = Vec::new();
		for _ in 0..(MAX_KANA - 1) {
			let mut decoder = vec![0.0_f32; MAX_KANA];
			for (index, id) in ids.iter().enumerate() {
				decoder[index] = *id as f32;
			}
			let encoder_tensor = Tensor::from_shape(&[1, MAX_ENGLISH], &encoder)?;
			let decoder_tensor = Tensor::from_shape(&[1, MAX_KANA], &decoder)?;
			let outputs = self.model.run(tvec!(encoder_tensor.into(), decoder_tensor.into()))?;
			let prediction = outputs[0].to_plain_array_view::<f32>()?;
			let position = ids.len() - 1;
			let (predicted_id, probability) = (0..KANA_CLASSES)
				.map(|id| (id, prediction[[0, position, id]]))
				.max_by(|left, right| left.1.total_cmp(&right.1))
				.ok_or_else(|| anyhow!("ONNX出力が空"))?;
			probabilities.push(probability);
			if predicted_id == END_ID {
				break;
			}
			ids.push(predicted_id);
		}
		let mut decoded = String::new();
		for id in ids {
			if let Some(token) = self.kana_index_word.get(&id) {
				decoded.push_str(token);
			}
		}
		let decoded = decoded
			.trim_start_matches(|ch| "<s>".contains(ch))
			.trim_end_matches(|ch| "<e>".contains(ch))
			.replace(['<', '>'], "");
		if decoded.is_empty() {
			return Err(anyhow!("ONNX出力が空文字"));
		}
		let confidence = if probabilities.is_empty() {
			0.0
		} else {
			(probabilities.iter().map(|value| value.max(f32::MIN_POSITIVE).ln()).sum::<f32>() / probabilities.len() as f32).exp()
		};
		Ok((decoded, confidence))
	}

	fn translate_atomic(&self, word: &str) -> Result<TranslationCandidate> {
		let normalized = word.to_ascii_lowercase();
		// These ordinary English function words collide with abbreviation entries
		// in both loanword dictionaries. Capitalization also occurs in game UI and
		// chat, so it must not turn TO / IS into letter names. Keep this small:
		// genuine initialisms and informal contractions retain the existing policy.
		let function_word = match normalized.as_str() {
			"to" => Some("トゥー"),
			"is" => Some("イズ"),
			_ => None,
		};
		if let Some(text) = function_word {
			return Ok(TranslationCandidate {
				text: text.into(),
				confidence: 1.0,
				reason: "english_function_word".into(),
			});
		}
		let known = self.known_readings.get(&normalized);
		let (onnx_text, onnx_confidence) = self.translate_onnx(word)?;
		let cmu_text = self
			.cmudict
			.get(&normalized)
			.and_then(|phonemes| arpakana::arpabet_to_kana(phonemes));
		if let Some(candidates) = self.jmdict_readings.get(&normalized) {
			if let Some(text) = select_jmdict_reading(candidates, &onnx_text, cmu_text.as_deref(), known) {
				return Ok(TranslationCandidate {
					text,
					confidence: 1.0,
					reason: "jmdict_loanword".into(),
				});
			}
		}
		if let Some(known) = known {
			return Ok(TranslationCandidate {
				text: known.text.clone(),
				confidence: 1.0,
				reason: if known.votes >= 2 {
					"english2kana_corpus_consensus".into()
				} else {
					"english2kana_corpus_singleton".into()
				},
			});
		}
		if word.len() >= 2 && word.bytes().all(|byte| byte.is_ascii_uppercase()) {
			return Ok(TranslationCandidate {
				text: spell_initialism(word),
				confidence: 1.0,
				reason: "initialism".into(),
			});
		}
		if word.bytes().all(|byte| byte.is_ascii_lowercase()) {
			if let Some(base) = inflection_bases(&normalized)
				.into_iter()
				.find(|base| self.known_readings.contains_key(base.as_str()) || self.jmdict_readings.contains_key(base.as_str()))
			{
				let mut candidate = self.translate_atomic(&base)?;
				candidate.reason = format!("inflected_base:{base}:{}", candidate.reason);
				return Ok(candidate);
			}
		}
		if word.bytes().next().is_some_and(|byte| byte.is_ascii_uppercase()) {
			if let Some(text) = cmu_text {
				return Ok(TranslationCandidate {
					text,
					confidence: 1.0,
					reason: "cmudict_arpabet".into(),
				});
			}
		}
		Ok(TranslationCandidate {
			text: onnx_text,
			confidence: onnx_confidence,
			reason: "onnx_model".into(),
		})
	}

	fn translate_token(&self, token: &str) -> Result<TranslationCandidate> {
		if USERNAME_ID.is_match(token) {
			return self.translate_username_id(token);
		}
		let apostrophe_compact = token.replace(['\'', '’'], "");
		if apostrophe_compact != token && !token.contains('-') {
			let mut candidate = self.translate_token(&apostrophe_compact)?;
			candidate.reason = format!("apostrophe_compact:{}", candidate.reason);
			return Ok(candidate);
		}
		if token.contains('-') {
			let mut converted = Vec::new();
			let mut reasons = Vec::new();
			let mut confidence = 1.0_f32;
			for part in token.split('-') {
				let candidate = self.translate_token(part)?;
				confidence = confidence.min(candidate.confidence);
				reasons.push(candidate.reason);
				converted.push(candidate.text);
			}
			return Ok(TranslationCandidate {
				text: converted.join("-"),
				confidence,
				reason: format!("hyphenated:{}", reasons.join("+")),
			});
		}
		let parts = split_camel_case(token);
		if parts.len() > 1 {
			let mut text = String::new();
			let mut reasons = Vec::new();
			let mut confidence = 1.0_f32;
			for part in parts {
				let candidate = self.translate_atomic(part)?;
				text.push_str(&candidate.text);
				confidence = confidence.min(candidate.confidence);
				reasons.push(candidate.reason);
			}
			return Ok(TranslationCandidate {
				text,
				confidence,
				reason: format!("camel_case:{}", reasons.join("+")),
			});
		}
		self.translate_atomic(token)
	}

	fn translate_username_id(&self, token: &str) -> Result<TranslationCandidate> {
		let mut converted = Vec::new();
		let mut reasons = Vec::new();
		let mut confidence = 1.0_f32;
		for segment in token.split('_') {
			if segment.bytes().all(|byte| byte.is_ascii_digit()) {
				converted.extend(segment.bytes().map(spell_digit).map(str::to_string));
				reasons.push("digits".to_string());
				continue;
			}
			for run in alphanumeric_runs(segment) {
				if run.bytes().all(|byte| byte.is_ascii_digit()) {
					converted.extend(run.bytes().map(spell_digit).map(str::to_string));
					reasons.push("digits".to_string());
				} else {
					let candidate = self.translate_token(run)?;
					confidence = confidence.min(candidate.confidence);
					reasons.push(candidate.reason);
					converted.push(candidate.text);
				}
			}
		}
		Ok(TranslationCandidate {
			text: converted.join("・"),
			confidence,
			reason: format!("username_id:{}", reasons.join("+")),
		})
	}

	fn convert_text(&self, content: &str, min_confidence: f32) -> ConversionResult {
		convert_text_with(content, min_confidence, |word| self.translate_token(word))
	}
}

fn tokenizer_config(path: &Path) -> Result<serde_json::Value> {
	let outer: serde_json::Value =
		serde_json::from_slice(&fs::read(path).with_context(|| format!("Tokenizerを開けない: {}", path.display()))?)?;
	outer
		.get("config")
		.cloned()
		.ok_or_else(|| anyhow!("Tokenizer config がない: {}", path.display()))
}

fn load_word_index(path: &Path) -> Result<HashMap<String, usize>> {
	let config = tokenizer_config(path)?;
	let encoded = config
		.get("word_index")
		.and_then(serde_json::Value::as_str)
		.ok_or_else(|| anyhow!("word_index がない"))?;
	Ok(serde_json::from_str(encoded)?)
}

fn load_index_word(path: &Path) -> Result<HashMap<usize, String>> {
	let config = tokenizer_config(path)?;
	let encoded = config
		.get("index_word")
		.and_then(serde_json::Value::as_str)
		.ok_or_else(|| anyhow!("index_word がない"))?;
	let string_keys: HashMap<String, String> = serde_json::from_str(encoded)?;
	string_keys
		.into_iter()
		.map(|(key, value)| Ok((key.parse::<usize>()?, value)))
		.collect()
}

fn load_known_readings(path: &Path) -> Result<HashMap<String, KnownReading>> {
	let mut readings = HashMap::new();
	for line in fs::read_to_string(path)
		.with_context(|| format!("既知語読みを開けない: {}", path.display()))?
		.lines()
	{
		if line.starts_with('#') || line.trim().is_empty() {
			continue;
		}
		let mut fields = line.split('\t');
		let Some(word) = fields.next() else { continue };
		let Some(kana) = fields.next() else { continue };
		let Some(votes) = fields.next().and_then(|field| field.parse::<usize>().ok()) else {
			continue;
		};
		if word.bytes().all(|byte| byte.is_ascii_lowercase()) && !kana.is_empty() {
			readings.insert(
				word.to_string(),
				KnownReading {
					text: kana.to_string(),
					votes,
				},
			);
		}
	}
	Ok(readings)
}

fn load_jmdict_readings(path: &Path) -> Result<HashMap<String, Vec<JmdictReading>>> {
	let mut readings = HashMap::new();
	for line in fs::read_to_string(path)
		.with_context(|| format!("JMdict読みを開けない: {}", path.display()))?
		.lines()
	{
		if line.starts_with('#') || line.trim().is_empty() {
			continue;
		}
		let mut fields = line.split('\t');
		let Some(word) = fields.next() else { continue };
		let Some(kana) = fields.next() else { continue };
		let Some(common) = fields.next() else { continue };
		if word.bytes().all(|byte| byte.is_ascii_lowercase()) && !kana.is_empty() {
			readings.entry(word.to_string()).or_insert_with(Vec::new).push(JmdictReading {
				text: kana.to_string(),
				common: common == "1",
			});
		}
	}
	Ok(readings)
}

fn select_jmdict_reading(candidates: &[JmdictReading], onnx: &str, cmu: Option<&str>, known: Option<&KnownReading>) -> Option<String> {
	let mut ranked = candidates
		.iter()
		.filter_map(|candidate| {
			let known_distance = known.map_or(0, |reading| edit_distance(&candidate.text, &reading.text));
			if known.is_some_and(|reading| reading.votes >= 2 && known_distance > 2) {
				return None;
			}
			let onnx_distance = edit_distance(&candidate.text, onnx);
			let cmu_distance = cmu.map_or(onnx_distance, |reading| edit_distance(&candidate.text, reading));
			let uncommon_penalty = usize::from(!candidate.common) * 2;
			Some((
				candidate,
				onnx_distance + cmu_distance + known_distance * 3 + uncommon_penalty,
				onnx_distance.min(cmu_distance),
			))
		})
		.collect::<Vec<_>>();
	ranked.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.text.cmp(&right.0.text)));
	let (candidate, _, nearest_distance) = ranked.first()?;
	let acceptance_distance = 3.max(candidate.text.chars().count() / 2);
	(*nearest_distance <= acceptance_distance).then(|| candidate.text.clone())
}

fn inflection_bases(word: &str) -> Vec<String> {
	let mut bases = Vec::new();
	if let Some(base) = word.strip_suffix("ies") {
		bases.push(format!("{base}y"));
	}
	if let Some(base) = word.strip_suffix("es") {
		bases.push(base.to_string());
	}
	if let Some(base) = word.strip_suffix('s') {
		bases.push(base.to_string());
	}
	if word.ends_with("ed") {
		bases.push(word[..word.len() - 1].to_string());
	}
	if let Some(base) = word.strip_suffix("ed") {
		bases.push(base.to_string());
	}
	if let Some(base) = word.strip_suffix("ing") {
		bases.push(base.to_string());
		bases.push(format!("{base}e"));
		if base.len() > 1 && base.as_bytes()[base.len() - 1] == base.as_bytes()[base.len() - 2] {
			bases.push(base[..base.len() - 1].to_string());
		}
	}
	bases
}

fn edit_distance(left: &str, right: &str) -> usize {
	let right = right.chars().collect::<Vec<_>>();
	let mut previous = (0..=right.len()).collect::<Vec<_>>();
	for (left_index, left_char) in left.chars().enumerate() {
		let mut current = vec![left_index + 1];
		for (right_index, right_char) in right.iter().enumerate() {
			current.push(
				(previous[right_index + 1] + 1)
					.min(current[right_index] + 1)
					.min(previous[right_index] + usize::from(left_char != *right_char)),
			);
		}
		previous = current;
	}
	previous[right.len()]
}

fn load_cmudict(path: &Path) -> Result<HashMap<String, String>> {
	let mut pronunciations = HashMap::new();
	for line in fs::read_to_string(path)
		.with_context(|| format!("CMUdictを開けない: {}", path.display()))?
		.lines()
	{
		if line.starts_with(";;;") || line.trim().is_empty() {
			continue;
		}
		let Some((raw_word, phonemes)) = line.split_once(' ') else {
			continue;
		};
		let word = raw_word.split_once('(').map_or(raw_word, |(base, _)| base);
		if !word.is_empty() && !phonemes.trim().is_empty() {
			pronunciations
				.entry(word.to_string())
				.or_insert_with(|| phonemes.trim().to_string());
		}
	}
	Ok(pronunciations)
}

fn split_camel_case(token: &str) -> Vec<&str> {
	if !token.is_ascii() {
		return vec![token];
	}
	let bytes = token.as_bytes();
	let mut starts = vec![0];
	for index in 1..bytes.len() {
		let previous = bytes[index - 1];
		let current = bytes[index];
		let next = bytes.get(index + 1).copied();
		let lower_to_upper = previous.is_ascii_lowercase() && current.is_ascii_uppercase();
		let acronym_to_word =
			previous.is_ascii_uppercase() && current.is_ascii_uppercase() && next.is_some_and(|value| value.is_ascii_lowercase());
		if lower_to_upper || acronym_to_word {
			starts.push(index);
		}
	}
	starts.push(bytes.len());
	starts.windows(2).map(|range| &token[range[0]..range[1]]).collect()
}

fn spell_initialism(word: &str) -> String {
	word.bytes()
		.filter_map(|byte| {
			Some(match byte.to_ascii_uppercase() {
				b'A' => "エー",
				b'B' => "ビー",
				b'C' => "シー",
				b'D' => "ディー",
				b'E' => "イー",
				b'F' => "エフ",
				b'G' => "ジー",
				b'H' => "エイチ",
				b'I' => "アイ",
				b'J' => "ジェー",
				b'K' => "ケー",
				b'L' => "エル",
				b'M' => "エム",
				b'N' => "エヌ",
				b'O' => "オー",
				b'P' => "ピー",
				b'Q' => "キュー",
				b'R' => "アール",
				b'S' => "エス",
				b'T' => "ティー",
				b'U' => "ユー",
				b'V' => "ブイ",
				b'W' => "ダブリュー",
				b'X' => "エックス",
				b'Y' => "ワイ",
				b'Z' => "ゼット",
				_ => return None,
			})
		})
		.collect()
}

fn spell_digit(byte: u8) -> &'static str {
	match byte {
		b'0' => "ゼロ",
		b'1' => "イチ",
		b'2' => "ニ",
		b'3' => "サン",
		b'4' => "ヨン",
		b'5' => "ゴ",
		b'6' => "ロク",
		b'7' => "ナナ",
		b'8' => "ハチ",
		b'9' => "キュウ",
		_ => "",
	}
}

fn alphanumeric_runs(segment: &str) -> Vec<&str> {
	let mut starts = vec![0];
	let bytes = segment.as_bytes();
	for index in 1..bytes.len() {
		if bytes[index - 1].is_ascii_digit() != bytes[index].is_ascii_digit() {
			starts.push(index);
		}
	}
	starts.push(bytes.len());
	starts.windows(2).map(|range| &segment[range[0]..range[1]]).collect()
}

fn is_extended_latin(ch: char) -> bool {
	matches!(ch as u32, 0x00c0..=0x024f | 0x1e00..=0x1eff)
}

fn token_looks_unsafe(content: &str, start: usize, end: usize) -> bool {
	if content[..start].bytes().filter(|byte| *byte == b'`').count() % 2 == 1 {
		return true;
	}
	let token_start = content[..start].rfind(char::is_whitespace).map_or(0, |index| index + 1);
	let token_end = content[end..].find(char::is_whitespace).map_or(content.len(), |index| end + index);
	let token = &content[token_start..token_end];
	let previous = content[..start].chars().next_back();
	let next = content[end..].chars().next();
	previous.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
		|| next.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
		|| token.contains("://")
		|| token.contains('@')
		|| token.contains('/')
		|| token.contains('\\')
		|| token.contains('`')
		|| token.chars().any(is_extended_latin)
		|| DOMAIN_LIKE.is_match(token)
}

fn rejected(source: &str, reason: impl Into<String>) -> ConversionTrace {
	ConversionTrace {
		source: source.into(),
		candidate: None,
		confidence: None,
		accepted: false,
		reason: reason.into(),
	}
}

fn convert_text_with<F>(content: &str, min_confidence: f32, mut translate: F) -> ConversionResult
where
	F: FnMut(&str) -> Result<TranslationCandidate>,
{
	let mut text = String::with_capacity(content.len());
	let mut cursor = 0;
	let mut converted_count = 0;
	let mut spans = Vec::new();
	for found in ENGLISH_SPAN.find_iter(content) {
		text.push_str(&content[cursor..found.start()]);
		let source = found.as_str();
		let (replacement, trace) = if source.len() > MAX_ENGLISH {
			(source.to_string(), rejected(source, "too_long"))
		} else if token_looks_unsafe(content, found.start(), found.end()) {
			(source.to_string(), rejected(source, "url_email_path_or_mixed_token"))
		} else {
			match translate(source) {
				Ok(candidate) if candidate.confidence >= min_confidence && !candidate.text.is_empty() => {
					converted_count += 1;
					(
						candidate.text.clone(),
						ConversionTrace {
							source: source.into(),
							candidate: Some(candidate.text),
							confidence: Some(candidate.confidence),
							accepted: true,
							reason: candidate.reason,
						},
					)
				}
				Ok(candidate) => (
					source.to_string(),
					ConversionTrace {
						source: source.into(),
						candidate: Some(candidate.text),
						confidence: Some(candidate.confidence),
						accepted: false,
						reason: "low_confidence".into(),
					},
				),
				Err(error) => (source.to_string(), rejected(source, format!("inference_error: {error}"))),
			}
		};
		text.push_str(&replacement);
		spans.push(trace);
		cursor = found.end();
	}
	text.push_str(&content[cursor..]);
	ConversionResult {
		text,
		converted_count,
		spans,
	}
}

fn parse_model_dir() -> Result<PathBuf> {
	let mut args = env::args_os().skip(1);
	match (args.next().and_then(|value| value.into_string().ok()), args.next()) {
		(Some(flag), Some(path)) if flag == "--model-dir" => Ok(PathBuf::from(path)),
		_ => Err(anyhow!("usage: vac-english2kana-worker --model-dir <directory>")),
	}
}

fn main() {
	if let Err(error) = run() {
		eprintln!("vac-english2kana-worker: {error:#}");
		std::process::exit(1);
	}
}

fn run() -> Result<()> {
	let model = English2Kana::load(&parse_model_dir()?)?;
	let stdin = io::stdin();
	let mut stdout = io::BufWriter::new(io::stdout().lock());
	for line in stdin.lock().lines() {
		let response = match serde_json::from_str::<Request>(&line?) {
			Ok(request) => Response {
				id: request.id,
				ok: true,
				result: Some(model.convert_text(&request.content, request.min_confidence.clamp(0.0, 1.0))),
				error: None,
			},
			Err(error) => Response {
				id: 0,
				ok: false,
				result: None,
				error: Some(format!("invalid request: {error}")),
			},
		};
		serde_json::to_writer(&mut stdout, &response)?;
		stdout.write_all(b"\n")?;
		stdout.flush()?;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn conservative_token_boundaries_and_confidence() {
		let content = "hello https://example.com a@b.com foo42 final_model.onnx `voice-test --model model.onnx` Wiš'adel world";
		let result = convert_text_with(content, 0.5, |word| {
			Ok(TranslationCandidate {
				text: format!("[{word}]"),
				confidence: if word == "world" { 0.2 } else { 0.9 },
				reason: "test".into(),
			})
		});
		assert_eq!(
			result.text,
			"[hello] https://example.com a@b.com foo42 final_model.onnx `voice-test --model model.onnx` Wiš'adel world"
		);
		assert_eq!(result.converted_count, 1);
	}

	#[test]
	fn camel_case_boundaries_keep_initialisms_together() {
		assert_eq!(split_camel_case("DirectML"), vec!["Direct", "ML"]);
		assert_eq!(split_camel_case("CamelCase"), vec!["Camel", "Case"]);
		assert_eq!(split_camel_case("CamelCASE"), vec!["Camel", "CASE"]);
		assert_eq!(split_camel_case("ONNX"), vec!["ONNX"]);
	}

	#[test]
	fn username_id_keeps_numeric_identity_as_individual_digits() {
		assert!(USERNAME_ID.is_match("foxxx_47"));
		assert!(USERNAME_ID.is_match("fox_alt_007"));
		assert!(!USERNAME_ID.is_match("x86_64"));
		assert!(!USERNAME_ID.is_match("final_model_v2"));
		assert_eq!(alphanumeric_runs("fox2"), vec!["fox", "2"]);
		assert_eq!("007".bytes().map(spell_digit).collect::<Vec<_>>(), vec!["ゼロ", "ゼロ", "ナナ"]);
	}

	#[test]
	fn jmdict_selection_uses_both_model_and_pronunciation() {
		let reading = |text: &str, common| JmdictReading { text: text.into(), common };
		let known_case = KnownReading {
			text: "ケース".into(),
			votes: 22,
		};
		assert_eq!(
			select_jmdict_reading(&[reading("キャッシュ", true)], "カシェ", Some("カッシュ"), None).as_deref(),
			Some("キャッシュ")
		);
		assert_eq!(
			select_jmdict_reading(
				&[
					reading("アキテクチャ", false),
					reading("アーキテクチャー", true),
					reading("アーキテクチュア", false),
				],
				"アーキテクチャー",
				Some("アーカテクチャー"),
				None,
			)
			.as_deref(),
			Some("アーキテクチャー")
		);
		assert_eq!(
			select_jmdict_reading(&[reading("ダラダラ", true)], "ストリーミング", Some("ストリーミング"), None),
			None
		);
		assert_eq!(
			select_jmdict_reading(
				&[reading("ケース", true), reading("ケイス", false)],
				"ケース",
				Some("ケイス"),
				Some(&known_case),
			)
			.as_deref(),
			Some("ケース")
		);
	}

	#[test]
	fn inflection_candidates_reduce_to_lexical_bases() {
		assert!(inflection_bases("checks").iter().any(|base| base == "check"));
		assert!(inflection_bases("completed").iter().any(|base| base == "complete"));
		assert!(inflection_bases("sampling").iter().any(|base| base == "sample"));
	}

	#[test]
	fn generated_model_matches_golden_when_available() {
		let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
		let model_dir = root.join(".vendors/english2kana/onnx");
		if !model_dir.join("english2kana-v1.onnx").is_file() {
			return;
		}
		let model = English2Kana::load(&model_dir).expect("load generated model");
		for (input, expected) in [("arknights", "アークナイツ"), ("mvp", "エムブイピー")] {
			assert_eq!(model.translate_onnx(input).unwrap().0, expected);
		}
		assert_eq!(model.translate_onnx("inference").unwrap().0, "インフェレンス");
		assert_eq!(
			model
				.convert_text(
					"High menu Japanese manual operator cache architecture algorithm inference samples completed raw DirectML CamelCase CamelCASE Kal'tsit",
					0.0,
				)
				.text,
			"ハイ メニュー ジャパニーズ マニュアル オペレーター キャッシュ アーキテクチャ アルゴリズム インフェレンス サンプル コンプリート ロー ダイレクトエムエル キャメルケース キャメルケース カルトシット"
		);
		assert_eq!(
			model.convert_text("alvecim, foxxx_47", 0.0).text,
			"アルベシム, フォックス・ヨン・ナナ"
		);
	}
}
