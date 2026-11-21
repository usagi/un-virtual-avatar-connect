//! Shared legacy dictionary/regex conversion into the current eleven-column TSV.
use std::{fs, path::PathBuf};
use anyhow::{anyhow, bail, Context, Result};

#[derive(Debug, Clone)]
pub(super) struct InputSpec {
	pub(super) path: PathBuf,
	pub(super) kind_hint: KindHint,
	pub(super) tag: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KindHint {
	Auto,
	Literal,
	Regex,
}

impl KindHint {
	fn parse(s: &str) -> Result<Self> {
		match s {
			"auto" => Ok(Self::Auto),
			"literal" => Ok(Self::Literal),
			"regex" => Ok(Self::Regex),
			other => Err(anyhow!("unknown kind '{other}' (expected auto/literal/regex)")),
		}
	}
}

pub(super) fn parse_input_arg(s: &str, default_kind: &str) -> Result<InputSpec> {
	// A Windows drive separator is part of the path, including C:relative and
	// verbatim \\?\C:\ paths. Only a later ':' starts the optional kind/tag.
	let drive_start = if s.starts_with(r"\\?\") { 4 } else { 0 };
	let bytes = s.as_bytes();
	let drive_colon = (bytes.get(drive_start).is_some_and(u8::is_ascii_alphabetic) && bytes.get(drive_start + 1) == Some(&b':'))
		.then_some(drive_start + 1);
	let separator = s
		.char_indices()
		.find(|(index, c)| *c == ':' && Some(*index) != drive_colon)
		.map(|(index, _)| index);
	let (path_text, suffix) = match separator {
		Some(index) => (&s[..index], Some(&s[index + 1..])),
		None => (s, None),
	};
	if path_text.is_empty() {
		bail!("input path must not be empty");
	}
	let (kind, tag) = suffix.map(|s| s.split_once(':').unwrap_or((s, ""))).unwrap_or(("", ""));
	let path = PathBuf::from(path_text);
	let kind_hint = KindHint::parse(if kind.is_empty() { default_kind } else { kind })?;
	let tag = (!tag.is_empty()).then(|| tag.to_string());
	Ok(InputSpec { path, kind_hint, tag })
}

pub(super) fn detect_kind(path: &PathBuf, hint: KindHint) -> &'static str {
	if matches!(hint, KindHint::Literal) {
		return "literal";
	}
	if matches!(hint, KindHint::Regex) {
		return "regex";
	}
	// auto: 拡張子と接頭辞から判定
	let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
	let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
	if name.starts_with("regex.") || ext == "csv" {
		"regex"
	} else if name.starts_with("dictionary.") {
		"literal"
	} else {
		"literal"
	}
}

#[derive(Debug, Clone)]
pub(super) struct Row {
	reading: String,
	surface: String,
	part_of_speech: String,
	comment: String,
	priority: i64,
	is_locked: bool,
	enabled: bool,
	by: String,
	created_at: String,
	expires_at: Option<String>,
	tags: String,
}

impl Row {
	pub(super) fn to_tsv_line(&self) -> String {
		let cells = [
			escape(&self.reading),
			escape(&self.surface),
			escape(&self.part_of_speech),
			escape(&self.comment),
			self.priority.to_string(),
			self.is_locked.to_string(),
			self.enabled.to_string(),
			escape(&self.by),
			escape(&self.created_at),
			self.expires_at.as_deref().map(escape).unwrap_or_default(),
			escape(&self.tags),
		];
		cells.join("\t")
	}
}

fn escape(s: &str) -> String {
	let mut out = String::with_capacity(s.len());
	for c in s.chars() {
		match c {
			'\\' => out.push_str(r"\\"),
			'\t' => out.push_str(r"\t"),
			'\n' => out.push_str(r"\n"),
			'\r' => out.push_str(r"\r"),
			c => out.push(c),
		}
	}
	out
}

fn now_rfc3339() -> String {
	jiff::Timestamp::now().strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn parse_dict_loose(contents: &str) -> Vec<(String, String, String, String)> {
	let mut rows = Vec::new();
	for line in contents.lines() {
		let trimmed = line.trim_end_matches('\r');
		if trimmed.is_empty() || trimmed.starts_with('#') {
			continue;
		}
		if trimmed.contains('\t') {
			let fields: Vec<&str> = trimmed.split('\t').collect();
			if fields.len() < 2 || fields.len() > 4 {
				continue;
			}
			rows.push((
				fields[0].to_string(),
				fields[1].to_string(),
				fields.get(2).copied().filter(|v| !v.is_empty()).unwrap_or("名詞").to_string(),
				fields.get(3).copied().unwrap_or("").to_string(),
			));
		} else {
			let mut it = trimmed.split_whitespace();
			let Some(reading) = it.next() else { continue };
			let Some(surface) = it.next() else { continue };
			rows.push((reading.to_string(), surface.to_string(), "名詞".into(), String::new()));
		}
	}
	rows
}

fn parse_regex_space(contents: &str) -> Vec<(String, String)> {
	// V1 形式: `replacement<SPACES>pattern` 1 行 1 エントリ。
	// replacement が複数単語になりうるため、「`^` から始まるトークン」を pattern 開始点として探す。
	// 見つからない場合は先頭単語 = replacement, 残り = pattern にフォールバック。
	let mut rows = Vec::new();
	for line in contents.lines() {
		let trimmed = line.trim_end_matches(['\r', '\n']);
		if trimmed.trim().is_empty() || trimmed.trim_start().starts_with('#') {
			continue;
		}
		let t = trimmed.trim_start();

		// "^" 始まりの regex パターンを探す: (空白) + "^"
		let anchor_split = find_regex_anchor_split(t);
		let (replacement, pattern) = match anchor_split {
			Some(idx) => {
				let r = t[..idx].trim_end().to_string();
				let p = t[idx..].to_string();
				(r, p)
			}
			None => {
				// 先頭単語が replacement
				let end = t.char_indices().find(|(_, c)| c.is_whitespace()).map(|(i, _)| i).unwrap_or(t.len());
				let r = t[..end].to_string();
				let p = t[end..].trim_start().to_string();
				(r, p)
			}
		};
		if pattern.is_empty() {
			continue;
		}
		rows.push((pattern, replacement));
	}
	rows
}

/// `t` の中で "空白 + '^'" というパターンの '^' 位置を返す。
fn find_regex_anchor_split(t: &str) -> Option<usize> {
	let bytes = t.as_bytes();
	for i in 1..bytes.len() {
		if bytes[i] == b'^' {
			let prev = bytes[i - 1];
			if prev == b' ' || prev == b'\t' {
				return Some(i);
			}
		}
	}
	None
}

fn parse_regex_csv(contents: &str) -> Result<Vec<(String, String)>> {
	let mut reader = csv::ReaderBuilder::new()
		.has_headers(false)
		.flexible(true)
		.from_reader(contents.as_bytes());
	let mut rows = Vec::new();
	for rec in reader.records() {
		let rec = rec.context("CSV parse")?;
		if rec.is_empty() {
			continue;
		}
		let replacement = rec.get(0).unwrap_or("").to_string();
		let pattern = rec.get(1).unwrap_or("").to_string();
		if pattern.is_empty() {
			continue;
		}
		rows.push((pattern, replacement));
	}
	Ok(rows)
}

pub(super) fn convert_input(spec: &InputSpec, by_default: &str, locked: bool) -> Result<Vec<Row>> {
	let contents = fs::read_to_string(&spec.path).with_context(|| format!("read {}", spec.path.display()))?;
	let kind = detect_kind(&spec.path, spec.kind_hint);
	let now = now_rfc3339();

	let entries: Vec<(String, String, String, String)> = match kind {
		"literal" => parse_dict_loose(&contents),
		"regex" => {
			let ext = spec.path.extension().and_then(|s| s.to_str()).unwrap_or("");
			let pairs = if ext.eq_ignore_ascii_case("csv") {
				parse_regex_csv(&contents)?
			} else {
				parse_regex_space(&contents)
			};
			pairs
				.into_iter()
				.map(|(surface, reading)| (reading, surface, "正規表現".into(), String::new()))
				.collect()
		}
		_ => unreachable!(),
	};

	let by = spec.tag.as_deref().unwrap_or(by_default).to_string();
	// ソース由来の tag も tags 列に詰める（by とは別軸のメタ）
	let tag_string = spec.path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();

	let mut rows = Vec::with_capacity(entries.len());
	for (reading, surface, part_of_speech, source_comment) in entries {
		rows.push(Row {
			reading,
			surface,
			part_of_speech,
			comment: if source_comment.is_empty() {
				format!("migrated from {}", spec.path.display())
			} else {
				source_comment
			},
			priority: 0,
			is_locked: locked,
			enabled: true,
			by: by.clone(),
			created_at: now.clone(),
			expires_at: None,
			tags: tag_string.clone(),
		});
	}
	Ok(rows)
}

pub(super) fn header_line() -> &'static str {
	"reading\tsurface\tpart_of_speech\tcomment\tpriority\tis_locked\tenabled\tby\tcreated_at\texpires_at\ttags"
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parse_dict_loose_handles_space_and_tab() {
		let input = "にんげん 人間\nいかく\t異格\n# comment\n\n";
		let pairs = parse_dict_loose(input);
		assert_eq!(pairs.len(), 2);
		assert_eq!(pairs[0], ("にんげん".into(), "人間".into(), "名詞".into(), "".into()));
		assert_eq!(pairs[1], ("いかく".into(), "異格".into(), "名詞".into(), "".into()));
	}

	#[test]
	fn parse_google_ime_four_columns_preserves_pos_and_comment() {
		let rows = parse_dict_loose("けるしー\tケルシー\t固有名詞\tアークナイツ\n");
		assert_eq!(
			rows[0],
			("けるしー".into(), "ケルシー".into(), "固有名詞".into(), "アークナイツ".into())
		);
	}

	#[test]
	fn parse_regex_space_replacement_and_rest_as_pattern() {
		let input = "/quit ^\\s*システムコマンド\\s*シャットダウン.*\nUSAGI.NETWORK うさぎ\\s*ネットワーク\n";
		let pairs = parse_regex_space(input);
		assert_eq!(pairs.len(), 2);
		assert_eq!(pairs[0].0, "^\\s*システムコマンド\\s*シャットダウン.*");
		assert_eq!(pairs[0].1, "/quit");
		assert_eq!(pairs[1].1, "USAGI.NETWORK");
	}

	#[test]
	fn parse_regex_space_multi_word_replacement_split_at_anchor() {
		// V1 `regex.pre-command.txt` の "/set init ^..." パターン（replacement に空白あり）
		let input = "/set init ^\\s*システムコマンド\\s*セット\\s*(イニット|イニシャライズ).*\n";
		let pairs = parse_regex_space(input);
		assert_eq!(pairs.len(), 1);
		assert_eq!(pairs[0].1, "/set init");
		assert!(pairs[0].0.starts_with('^'));
	}

	#[test]
	fn parse_regex_space_no_anchor_falls_back_to_first_token() {
		let input = "prefix .*\n";
		let pairs = parse_regex_space(input);
		assert_eq!(pairs.len(), 1);
		assert_eq!(pairs[0].1, "prefix");
		assert_eq!(pairs[0].0, ".*");
	}

	#[test]
	fn parse_regex_csv_replacement_pattern_order() {
		let input = "パチパチパチ,\"[8８]{3,}\"\n,\"([!?,.]+)\"\n$1 (以下略),\"^(.{32}).*\"\n";
		let pairs = parse_regex_csv(input).unwrap();
		assert_eq!(pairs.len(), 3);
		assert_eq!(pairs[0].0, "[8８]{3,}");
		assert_eq!(pairs[0].1, "パチパチパチ");
		assert_eq!(pairs[1].1, ""); // empty replacement OK
		assert_eq!(pairs[2].1, "$1 (以下略)");
	}

	#[test]
	fn detect_kind_from_filename() {
		assert_eq!(detect_kind(&PathBuf::from("dictionary.foo.txt"), KindHint::Auto), "literal");
		assert_eq!(detect_kind(&PathBuf::from("regex.foo.txt"), KindHint::Auto), "regex");
		assert_eq!(detect_kind(&PathBuf::from("regex.foo.csv"), KindHint::Auto), "regex");
		assert_eq!(detect_kind(&PathBuf::from("foo.csv"), KindHint::Auto), "regex");
		assert_eq!(detect_kind(&PathBuf::from("random.txt"), KindHint::Literal), "literal");
	}

	#[test]
	fn tsv_row_round_trips_escapes() {
		let r = Row {
			reading: "a\tb".into(),
			surface: "c\nd".into(),
			part_of_speech: "名詞".into(),
			comment: String::new(),
			priority: 5,
			is_locked: true,
			enabled: true,
			by: "legacy".into(),
			created_at: "2026-04-23T00:00:00Z".into(),
			expires_at: None,
			tags: String::new(),
		};
		let line = r.to_tsv_line();
		assert!(!line.contains('\n'));
		assert!(line.contains(r"\t"));
		assert!(line.contains(r"\n"));
	}

	#[test]
	fn parse_input_arg_forms() {
		let a = parse_input_arg("foo.txt", "auto").unwrap();
		assert!(a.tag.is_none());
		assert!(matches!(a.kind_hint, KindHint::Auto));
		let b = parse_input_arg("foo.txt:regex", "auto").unwrap();
		assert!(matches!(b.kind_hint, KindHint::Regex));
		let c = parse_input_arg("foo.txt:literal:chat", "auto").unwrap();
		assert!(matches!(c.kind_hint, KindHint::Literal));
		assert_eq!(c.tag.as_deref(), Some("chat"));
	}

	#[test]
	fn windows_paths_keep_drive_colons_and_optional_metadata() {
		for path in [
			r"C:\配信 辞書\dictionary.txt",
			"D:/dict/regex.csv",
			r"C:dictionary.txt",
			r"\\?\C:\dict\a.txt",
			r"\\server\share\a.txt",
		] {
			let plain = parse_input_arg(path, "literal").unwrap();
			assert_eq!(plain.path, PathBuf::from(path));
			assert_eq!(plain.kind_hint, KindHint::Literal);
			assert_eq!(plain.tag, None);
			let annotated = parse_input_arg(&format!("{path}:regex:chat:moderator"), "auto").unwrap();
			assert_eq!(annotated.path, PathBuf::from(path));
			assert_eq!(annotated.kind_hint, KindHint::Regex);
			assert_eq!(annotated.tag.as_deref(), Some("chat:moderator"));
		}
	}

	#[test]
	fn invalid_input_metadata_is_not_silently_treated_as_a_path() {
		assert!(parse_input_arg("", "auto").is_err());
		assert!(parse_input_arg(":literal", "auto").is_err());
		assert!(parse_input_arg(r"C:\dict.txt:not-a-kind", "auto").is_err());
		let defaulted = parse_input_arg(r"C:\dict.txt::tag", "literal").unwrap();
		assert_eq!(defaulted.kind_hint, KindHint::Literal);
		assert_eq!(defaulted.tag.as_deref(), Some("tag"));
	}

	#[test]
	fn absolute_unicode_path_converts_real_file_without_modifying_source() {
		let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
		let root = std::env::temp_dir().join(format!("vac-dict-{}-{unique}-配信 辞書", std::process::id()));
		fs::create_dir(&root).unwrap();
		let path = root.join("dictionary.user.txt");
		let original = "うさぎ\tUSAGI\t名詞\t配信の呼び名\n";
		fs::write(&path, original).unwrap();
		let spec = parse_input_arg(&format!("{}:literal:review", path.display()), "auto").unwrap();
		let rows = convert_input(&spec, "legacy", false).unwrap();
		assert_eq!(rows.len(), 1);
		assert_eq!(rows[0].reading, "うさぎ");
		assert_eq!(rows[0].surface, "USAGI");
		assert_eq!(rows[0].comment, "配信の呼び名");
		assert_eq!(rows[0].by, "review");
		assert!(!rows[0].is_locked);
		assert_eq!(rows[0].to_tsv_line().split('\t').count(), 11);
		assert_eq!(fs::read_to_string(path).unwrap(), original);
		fs::remove_dir_all(root).unwrap();
	}

}
