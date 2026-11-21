//! Bounded, opt-in archive of OBS images selected for an AI request. Never captures images.
use anyhow::{Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
	io::{Cursor, Write},
	path::{Path, PathBuf},
	sync::mpsc,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImageArchiveConf {
	pub directory: PathBuf,
	#[serde(default = "default_limit")]
	pub max_bytes: u64,
}
fn default_limit() -> u64 {
	1_000_000_000
}
impl ImageArchiveConf {
	pub fn validate(&self) -> Result<()> {
		if !self.directory.is_absolute() {
			bail!("perception.image_archive.directory must be absolute");
		}
		if !(1_000_000..=10_000_000_000).contains(&self.max_bytes) {
			bail!("image_archive.max_bytes must be 1MB..10GB");
		}
		Ok(())
	}
}

#[derive(Debug)]
enum ArchiveCommand {
	Image(String, serde_json::Value),
	Flush(mpsc::Sender<()>),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ImageArchive {
	sender: Option<mpsc::SyncSender<ArchiveCommand>>,
	sources: Vec<(String, String, String)>,
}
impl ImageArchive {
	pub fn new(conf: Option<&ImageArchiveConf>, sources: &[crate::stream_perception::ObsPerceptionSourceConf]) -> Self {
		let Some(conf) = conf.cloned() else {
			return Self::default();
		};
		let (sender, receiver) = mpsc::sync_channel::<ArchiveCommand>(4);
		std::thread::spawn(move || {
			for command in receiver {
				match command {
					ArchiveCommand::Image(url, metadata) => {
						if let Err(error) = save(&conf, &url, metadata) {
							log::warn!("《診断画像》保存失敗: {error}");
						}
					}
					ArchiveCommand::Flush(done) => {
						let _ = done.send(());
					}
				}
			}
		});
		Self {
			sender: Some(sender),
			sources: sources
				.iter()
				.filter(|s| s.enabled)
				.map(|s| (s.channel.clone(), s.scene_name.clone(), s.source_name.clone()))
				.collect(),
		}
	}
	pub async fn flush(&self) {
		let Some(sender) = self.sender.clone() else {
			return;
		};
		let work = tokio::task::spawn_blocking(move || {
			let (done, receiver) = mpsc::channel();
			let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
			let mut command = ArchiveCommand::Flush(done);
			loop {
				match sender.try_send(command) {
					Ok(()) => break,
					Err(mpsc::TrySendError::Full(returned)) if std::time::Instant::now() < deadline => {
						command = returned;
						std::thread::sleep(std::time::Duration::from_millis(10));
					}
					Err(_) => return false,
				}
			}
			receiver
				.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
				.is_ok()
		});
		if !matches!(tokio::time::timeout(std::time::Duration::from_secs(6), work).await, Ok(Ok(true))) {
			log::warn!("《診断画像》終了時の保存待機が完了しませんでした");
		}
	}
	pub fn accepts(&self, cd: &crate::ChannelDatum) -> bool {
		cd.source.as_ref().is_some_and(|s| s.kind == "obs.scene_source")
			&& self.sources.iter().any(|(channel, scene, source)| {
				&cd.channel == channel
					&& cd.meta_get("scene_name").and_then(serde_json::Value::as_str) == Some(scene.as_str())
					&& cd.meta_get("source_name").and_then(serde_json::Value::as_str) == Some(source.as_str())
			})
	}
	pub fn enabled(&self) -> bool {
		self.sender.is_some()
	}
	pub fn submit(&self, url: String, metadata: serde_json::Value) {
		if let Some(sender) = &self.sender {
			if sender.try_send(ArchiveCommand::Image(url, metadata)).is_err() {
				log::warn!("《診断画像》保存キュー満杯または停止中。画像を破棄");
			}
		}
	}
}

fn digest(bytes: &[u8]) -> String {
	Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn owned_file(path: &Path) -> bool {
	let Some(stem) = path.file_stem().and_then(|v| v.to_str()).and_then(|v| v.strip_prefix("vac-image-")) else {
		return false;
	};
	stem.len() >= 16
		&& stem.bytes().all(|v| v.is_ascii_digit())
		&& matches!(path.extension().and_then(|v| v.to_str()), Some("jpg" | "png" | "webp" | "json"))
}

fn reserve(conf: &ImageArchiveConf, additional: u64) -> Result<()> {
	if additional > conf.max_bytes {
		bail!("single image exceeds archive budget");
	}
	let mut files = std::collections::BTreeMap::<String, Vec<(PathBuf, u64)>>::new();
	let mut total = 0;
	for entry in std::fs::read_dir(&conf.directory)? {
		let entry = entry?;
		if !entry.file_type()?.is_file() || !owned_file(&entry.path()) {
			continue;
		}
		let size = entry.metadata()?.len();
		total += size;
		files
			.entry(entry.path().file_stem().unwrap().to_string_lossy().into_owned())
			.or_default()
			.push((entry.path(), size));
	}
	for (_, group) in files {
		if total + additional <= conf.max_bytes {
			break;
		}
		for (path, size) in group {
			std::fs::remove_file(path)?;
			total -= size;
		}
	}
	Ok(())
}

fn encode(url: &str) -> Result<(Vec<u8>, &'static str, String)> {
	let (prefix, data) = url.split_once(',').ok_or_else(|| anyhow::anyhow!("not a data URL"))?;
	let extension = match prefix {
		"data:image/jpeg;base64" | "data:image/jpg;base64" => "jpg",
		"data:image/png;base64" => "png",
		_ => bail!("unsupported diagnostic image format"),
	};
	if data.len() > 24 * 1024 * 1024 {
		bail!("image too large");
	}
	let original = STANDARD.decode(data)?;
	let hash = digest(&original);
	let mut reader = image::ImageReader::new(Cursor::new(&original)).with_guessed_format()?;
	let mut limits = image::Limits::default();
	limits.max_image_width = Some(4096);
	limits.max_image_height = Some(4096);
	limits.max_alloc = Some(128 * 1024 * 1024);
	reader.limits(limits);
	let pixels = reader.decode()?;
	let mut webp = Cursor::new(Vec::new());
	pixels.write_to(&mut webp, image::ImageFormat::WebP)?;
	let webp = webp.into_inner();
	if webp.len() < original.len() {
		Ok((webp, "webp", hash))
	} else {
		Ok((original, extension, hash))
	}
}

fn save(conf: &ImageArchiveConf, url: &str, mut metadata: serde_json::Value) -> Result<()> {
	let (bytes, extension, original_hash) = encode(url)?;
	std::fs::create_dir_all(&conf.directory)?;
	// Do not follow a redirected archive root or delete unrelated entries.
	if std::fs::symlink_metadata(&conf.directory)?.file_type().is_symlink() {
		bail!("archive root must not be a symlink");
	}
	let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
	let stem = format!("vac-image-{stamp:020}");
	let filename = format!("{stem}.{extension}");
	metadata["image_file"] = filename.clone().into();
	metadata["input_sha256"] = original_hash.into();
	metadata["stored_sha256"] = digest(&bytes).into();
	metadata["encoding"] = extension.into();
	metadata["pixel_preserving"] = true.into();
	metadata["saved_at"] = jiff::Timestamp::now().to_string().into();
	let json = serde_json::to_vec_pretty(&metadata)?;
	reserve(conf, (bytes.len() + json.len()) as u64)?;
	let image_path = conf.directory.join(filename);
	let json_path = conf.directory.join(format!("{stem}.json"));
	let mut image_file = std::fs::OpenOptions::new().write(true).create_new(true).open(&image_path)?;
	if let Err(error) = image_file.write_all(&bytes) {
		drop(image_file);
		let _ = std::fs::remove_file(&image_path);
		return Err(error.into());
	}
	drop(image_file);
	let result = (|| -> Result<()> {
		let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&json_path)?;
		if let Err(error) = file.write_all(&json) {
			drop(file);
			let _ = std::fs::remove_file(&json_path);
			return Err(error.into());
		}
		Ok(())
	})();
	if result.is_err() {
		let _ = std::fs::remove_file(image_path);
	}
	result
}
#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn diagnostic_archive_preserves_pixels_and_rotates_pairs() {
		let directory = std::env::temp_dir().join(format!(
			"vac-archive-test-{}",
			std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_nanos()
		));
		std::fs::create_dir(&directory).unwrap();
		let conf = ImageArchiveConf {
			directory: directory.clone(),
			max_bytes: 1_000_000,
		};
		let pixels = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(64, 64, image::Rgb([32, 72, 100])));
		let mut original = Cursor::new(Vec::new());
		pixels.write_to(&mut original, image::ImageFormat::Png).unwrap();
		let url = format!("data:image/png;base64,{}", STANDARD.encode(original.get_ref()));
		let (encoded, _, hash) = encode(&url).unwrap();
		assert_eq!(image::load_from_memory(&encoded).unwrap().to_rgb8(), pixels.to_rgb8());
		assert_eq!(hash, digest(original.get_ref()));
		assert!(encode("https://example.com/image.png").is_err());
		assert!(encode("data:image/png;base64,bad").is_err());
		let old = "vac-image-00000000000000000001";
		std::fs::write(directory.join(format!("{old}.jpg")), vec![0; 999_000]).unwrap();
		std::fs::write(directory.join(format!("{old}.json")), vec![0; 999]).unwrap();
		std::fs::write(directory.join("user.jpg"), b"keep").unwrap();
		save(&conf, &url, serde_json::json!({"observation_datum_id":42})).unwrap();
		assert!(!directory.join(format!("{old}.jpg")).exists());
		assert!(!directory.join(format!("{old}.json")).exists());
		assert_eq!(std::fs::read(directory.join("user.jpg")).unwrap(), b"keep");
		let total: u64 = std::fs::read_dir(&directory)
			.unwrap()
			.map(|e| e.unwrap().metadata().unwrap().len())
			.sum();
		assert!(total <= conf.max_bytes);
		assert!(reserve(&conf, conf.max_bytes + 1).is_err());
		for entry in std::fs::read_dir(&directory).unwrap() {
			std::fs::remove_file(entry.unwrap().path()).unwrap();
		}
		std::fs::remove_dir(directory).unwrap();
	}
}
