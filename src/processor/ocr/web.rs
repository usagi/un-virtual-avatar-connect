use super::PathOrTempFileWithMime;
use anyhow::{Context, Result};
use async_tempfile::TempFile;
use tokio::io::AsyncWriteExt;

pub async fn get(url: &str) -> Result<PathOrTempFileWithMime> {
	log::debug!("画像をダウンロードします。 url = {:?}", url);
	let res = reqwest::get(url)
		.await
		.with_context(|| format!("画像のダウンロードに失敗しました: {url}"))?
		.error_for_status()
		.with_context(|| format!("画像URLがエラーを返しました: {url}"))?;

	// res -> mime
	let mime = res
		.headers()
		.get("content-type")
		.map(|h| h.to_str().unwrap_or_default().to_string())
		.unwrap_or_default();

	let bytes = res.bytes().await.context("画像responseの読み取りに失敗しました")?;
	let bytes = bytes.as_ref();
	let mut tempfile = TempFile::new().await.context("OCR画像用一時ファイルの作成に失敗しました")?;
	// tempfile に書き込み
	{
		tempfile
			.write_all(bytes)
			.await
			.context("OCR画像用一時ファイルへの書き込みに失敗しました")?;
	}
	log::trace!("一時ファイルを作成しました: {:?}", tempfile.file_path());
	Ok(PathOrTempFileWithMime::TempFile(tempfile, mime))
}
