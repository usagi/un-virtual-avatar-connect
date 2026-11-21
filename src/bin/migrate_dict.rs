//! η-5: V1 の辞書 / 正規表現ファイルを η の 11 カラム統一 TSV に変換する CLI。
//!
//! # 入力フォーマット（V1）
//!
//! - **Dictionary (Google IME)**: `dictionary.*.txt` — 各行が
//!   `reading<TAB>surface<TAB>part_of_speech?<TAB>comment?`。V1 の空白区切り 2 列も受理。
//! - **Regex (space)**: `regex.*.txt` — 各行が `replacement pattern`（先頭トークンが replacement）。
//!   空白 1 区切り。
//! - **Regex (CSV)**: `regex.*.csv` — `replacement,pattern`。CSV 標準のクォートサポート。
//!   空 replacement（= 削除ルール）も許可、このとき変換後の `replacement` は空文字列。
//!
//! # 出力フォーマット（η）
//!
//! `docs/roadmap/phase-eta-dictionary-unification.md` §3.1 の 11 カラム辞書スキーマ TSV。
//!
//! # 使い方
//!
//! ```sh
//! # 単一ファイル変換
//! virtual-avatar-connect-migrate-dict \
//!     --input dictionary.arknights.txt \
//!     --kind literal \
//!     --output dictionary.arknights.dict.tsv \
//!     --by "arknights:seed" --locked
//!
//! # 複数 input を 1 TSV に merge
//! virtual-avatar-connect-migrate-dict \
//!     --input dictionary.chat.txt:literal:chat \
//!     --input regex.chat.csv:regex:chat \
//!     --output dictionary.chat.dict.tsv
//! ```

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

#[path = "../file_publish.rs"]
mod file_publish;

#[path = "../migrate/dictionary_file.rs"]
mod dictionary_file;
use dictionary_file::{convert_input, detect_kind, header_line, parse_input_arg, InputSpec, Row};

#[derive(Debug, Parser)]
#[command(
	name = "migrate-dict",
	about = "V1 dictionary/regex files -> η unified 11-column TSV",
	long_about = "V1 の dictionary.*.txt / regex.*.txt / regex.*.csv を η の 11 カラム統一 TSV に変換する CLI。\n\
	             --input は \"path\" か \"path:kind\" か \"path:kind:tag\" で複数指定可能。\n\
	             kind: auto / literal / regex (auto は拡張子と接頭辞から推測)"
)]
struct Cli {
	/// 入力ファイル。`path` / `path:kind` / `path:kind:tag` 形式で複数回指定可能。
	#[arg(short, long, required = true, num_args = 1..)]
	input: Vec<String>,

	/// 出力 TSV path。`-` で stdout。
	#[arg(short, long)]
	output: String,

	/// 全エントリ共通の `by` フィールド（個別指定は `--input` の tag が優先）。
	#[arg(long, default_value = "legacy")]
	by: String,

	/// `is_locked=true` で全行を出力する。ファイル由来の "固定辞書" 扱い。
	#[arg(long)]
	locked: bool,

	/// `--locked` 指定時以外のデフォルト kind（path 指定側で上書き可能）。
	#[arg(long, default_value = "auto")]
	default_kind: String,

	/// 上書き確認。指定しないと既存 output を上書きしない。
	#[arg(long)]
	force: bool,
}

fn save_output(path: &std::path::Path, content: &[u8], force: bool) -> Result<()> {
	if force {
		file_publish::replace(path, content)
	} else {
		file_publish::create(path, content)
	}
	.with_context(|| format!("save {} (既存ファイルを上書きする場合は --force)", path.display()))
}

fn main() -> Result<()> {
	let cli = Cli::parse();
	let specs: Vec<InputSpec> = cli
		.input
		.iter()
		.map(|s| parse_input_arg(s, &cli.default_kind))
		.collect::<Result<_>>()?;

	let mut all_rows: Vec<Row> = Vec::new();
	for spec in &specs {
		let rows = convert_input(spec, &cli.by, cli.locked)?;
		eprintln!(
			"[migrate-dict] {} -> {} rows ({}, locked={})",
			spec.path.display(),
			rows.len(),
			detect_kind(&spec.path, spec.kind_hint),
			cli.locked
		);
		all_rows.extend(rows);
	}

	let mut out = String::new();
	out.push_str(header_line());
	out.push('\n');
	for r in &all_rows {
		out.push_str(&r.to_tsv_line());
		out.push('\n');
	}

	if cli.output == "-" {
		print!("{out}");
	} else {
		let dst = PathBuf::from(&cli.output);
		save_output(&dst, out.as_bytes(), cli.force)?;
		eprintln!(
			"[migrate-dict] wrote {} ({} rows, {} bytes)",
			dst.display(),
			all_rows.len(),
			out.len()
		);
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;

	#[test]
	fn output_preserves_existing_destination_and_unrelated_tmp_file() {
		let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
		let root = std::env::temp_dir().join(format!("vac-dict-output-{}-{unique}", std::process::id()));
		fs::create_dir(&root).unwrap();
		let output = root.join("dictionary.tsv");
		let unrelated = root.join("dictionary.tmp");
		fs::write(&unrelated, "unrelated work").unwrap();
		save_output(&output, b"first", false).unwrap();
		assert!(save_output(&output, b"replacement", false).is_err());
		assert_eq!(fs::read(&output).unwrap(), b"first");
		save_output(&output, b"replacement", true).unwrap();
		assert_eq!(fs::read(&output).unwrap(), b"replacement");
		assert_eq!(fs::read_to_string(unrelated).unwrap(), "unrelated work");
		assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
		fs::remove_dir_all(root).unwrap();
	}
}
