//! Phase δ-9 D.2/D.3: V1 `Ocr` プロセッサ本体は除去。Flowgraph
//! `flowgraph.ocr.recognize` ノードから参照されるヘルパー（ソース解決・Windows Media OCR 呼び出し）
//! のみをユーティリティモジュールとして残す。

pub(crate) mod data_urls;
pub(crate) mod local;
pub(crate) mod web;

use anyhow::{bail, Context, Result};
use async_tempfile::TempFile;
use std::{
	fs,
	path::{Path, PathBuf},
	sync::atomic::{AtomicU64, Ordering},
};

/// OCR 対象となる画像ソースを表す。`data_urls::get` / `local::get` / `web::get` の戻り値型。
pub enum PathOrTempFileWithMime {
	Path(String, String),
	TempFile(TempFile, String),
}

/// 画像ファイルを Windows Media OCR で認識し、テキストを返す。
///
/// - `path`: 認識対象ファイルのパス
/// - `mime`: MIME タイプ
/// - `lang`: BCP-47 の言語タグ（例: `"ja-JP"`, `"en-US"`）
/// - `lines`: true で認識行を `\n` 区切りで返す、false で空白区切りの 1 行に連結
#[cfg(target_os = "windows")]
pub fn recognize<P: AsRef<Path>, M: AsRef<str>>(path: P, mime: M, lang: &str, lines: bool) -> Result<String> {
	let path = path.as_ref();
	if lang.trim().is_empty() {
		bail!("言語タグが空です");
	}

	let prepared = PreparedPng::open(path, mime.as_ref())?;
	let png_path = prepared.path().to_string_lossy().into_owned();
	let recognized = std::panic::catch_unwind(|| win_ocr::ocr_with_lang(&png_path, lang))
		.map_err(|_| anyhow::anyhow!("Windows Media OCR backend が予期せず停止しました"))?
		.with_context(|| format!("Windows Media OCR の認識に失敗しました (lang={lang})"))?;

	Ok(format_recognized_text(&normalize_cjk_spacing(&recognized), lines))
}

#[cfg(target_os = "windows")]
fn normalize_cjk_spacing(text: &str) -> String {
	let chars = text.chars().collect::<Vec<_>>();
	let mut normalized = String::with_capacity(text.len());
	for (index, ch) in chars.iter().copied().enumerate() {
		if ch == ' ' && index > 0 && index + 1 < chars.len() && is_cjk_text_char(chars[index - 1]) && is_cjk_text_char(chars[index + 1]) {
			continue;
		}
		normalized.push(ch);
	}
	normalized
}

#[cfg(target_os = "windows")]
fn is_cjk_text_char(ch: char) -> bool {
	matches!(ch,
		'\u{3040}'..='\u{30ff}' |
		'\u{3400}'..='\u{4dbf}' |
		'\u{4e00}'..='\u{9fff}' |
		'\u{f900}'..='\u{faff}' |
		'\u{ff66}'..='\u{ff9f}'
	)
}

#[cfg(target_os = "windows")]
fn format_recognized_text(text: &str, lines: bool) -> String {
	let recognized_lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
	if lines {
		recognized_lines.collect::<Vec<_>>().join("\n")
	} else {
		recognized_lines.collect::<Vec<_>>().join(" ")
	}
}

/// `win_ocr 0.1.3` は PNG decoder 固定のため、PNG 以外を一時 PNG へ変換する。
#[cfg(target_os = "windows")]
struct PreparedPng {
	path: PathBuf,
	temporary: bool,
}

#[cfg(target_os = "windows")]
impl PreparedPng {
	fn open(path: &Path, declared_mime: &str) -> Result<Self> {
		let bytes = fs::read(path).with_context(|| format!("OCR 画像を開けません: {}", path.display()))?;
		let format = image::guess_format(&bytes).with_context(|| {
			format!(
				"OCR source は対応画像ではありません: {} (Content-Type={declared_mime})",
				path.display()
			)
		})?;
		if format == image::ImageFormat::Png {
			return Ok(Self {
				path: path.to_path_buf(),
				temporary: false,
			});
		}

		let image = image::load_from_memory_with_format(&bytes, format)
			.with_context(|| format!("OCR source の画像decodeに失敗しました: {}", path.display()))?;
		let temp_path = unique_temp_png_path();
		image
			.save_with_format(&temp_path, image::ImageFormat::Png)
			.with_context(|| format!("OCR用一時PNGの作成に失敗しました: {}", temp_path.display()))?;
		Ok(Self {
			path: temp_path,
			temporary: true,
		})
	}

	fn path(&self) -> &Path {
		&self.path
	}
}

#[cfg(target_os = "windows")]
impl Drop for PreparedPng {
	fn drop(&mut self) {
		if self.temporary {
			if let Err(error) = fs::remove_file(&self.path) {
				log::warn!("OCR用一時PNGを削除できませんでした: {}: {error}", self.path.display());
			}
		}
	}
}

#[cfg(target_os = "windows")]
fn unique_temp_png_path() -> PathBuf {
	static NEXT_ID: AtomicU64 = AtomicU64::new(0);
	let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
	std::env::temp_dir().join(format!("unvac-ocr-{}-{id}.png", std::process::id()))
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
	use super::*;

	#[test]
	fn text_shape_matches_lines_contract() {
		let source = " first line \r\n\r\n second line ";
		assert_eq!(format_recognized_text(source, true), "first line\nsecond line");
		assert_eq!(format_recognized_text(source, false), "first line second line");
	}

	#[test]
	fn cjk_spacing_normalization_preserves_non_cjk_spaces() {
		assert_eq!(normalize_cjk_spacing("サ ン プ ル テ キ ス ト"), "サンプルテキスト");
		assert_eq!(normalize_cjk_spacing("日本語 OCR 結果"), "日本語 OCR 結果");
		assert_eq!(normalize_cjk_spacing("Sample Text"), "Sample Text");
	}

	#[test]
	fn jpeg_is_converted_to_temporary_png() {
		let source = unique_temp_png_path().with_extension("jpg");
		let image = image::RgbImage::from_pixel(8, 8, image::Rgb([255, 255, 255]));
		image.save_with_format(&source, image::ImageFormat::Jpeg).unwrap();
		let prepared = PreparedPng::open(&source, "image/jpeg").unwrap();
		assert_ne!(prepared.path(), source);
		assert_eq!(
			image::guess_format(&fs::read(prepared.path()).unwrap()).unwrap(),
			image::ImageFormat::Png
		);
		fs::remove_file(source).unwrap();
	}

	/// 実画像評価用。`UNVAC_OCR_TEST_IMAGE` と `UNVAC_OCR_TEST_LANG` を設定して明示実行する。
	#[test]
	#[ignore = "Windows OCR言語packと実画像が必要"]
	fn evaluate_real_image_repeatedly() {
		use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
		use windows::Win32::{
			Foundation::FILETIME,
			System::Threading::{GetCurrentProcess, GetProcessTimes},
		};

		fn process_cpu_100ns() -> u64 {
			let mut creation = FILETIME::default();
			let mut exit = FILETIME::default();
			let mut kernel = FILETIME::default();
			let mut user = FILETIME::default();
			unsafe { GetProcessTimes(GetCurrentProcess(), &mut creation, &mut exit, &mut kernel, &mut user) }.unwrap();
			let ticks = |time: FILETIME| u64::from(time.dwLowDateTime) | (u64::from(time.dwHighDateTime) << 32);
			ticks(kernel) + ticks(user)
		}

		let path = std::env::var("UNVAC_OCR_TEST_IMAGE").expect("UNVAC_OCR_TEST_IMAGE is required");
		let lang = std::env::var("UNVAC_OCR_TEST_LANG").unwrap_or_else(|_| "ja-JP".into());
		let pid = sysinfo::get_current_pid().unwrap();
		let mut system = System::new();
		let mut memory = || {
			system.refresh_processes_specifics(
				ProcessesToUpdate::Some(&[pid]),
				true,
				ProcessRefreshKind::everything().without_cpu(),
			);
			system.process(pid).map(|process| process.memory()).unwrap_or_default()
		};
		let memory_before = memory();
		let cpu_before = process_cpu_100ns();
		let started = std::time::Instant::now();
		let first = recognize(&path, "application/octet-stream", &lang, true).unwrap();
		let cold = started.elapsed();
		let warm_started = std::time::Instant::now();
		for _ in 0..20 {
			let current = recognize(&path, "application/octet-stream", &lang, true).unwrap();
			assert_eq!(current, first);
		}
		let warm_elapsed = warm_started.elapsed();
		let cpu_ms = (process_cpu_100ns() - cpu_before) / 10_000;
		let memory_after = memory();
		eprintln!(
			"cold_ms={} warm_avg_ms={} cpu_ms={cpu_ms} memory_before_bytes={memory_before} memory_after_bytes={memory_after} text={first:?}",
			cold.as_millis(),
			warm_elapsed.as_millis() / 20
		);
	}
}
