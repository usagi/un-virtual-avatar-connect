use std::{collections::HashMap, fs, io::Write, path::Path};

use flate2::read::GzDecoder;
use serde::Deserialize;

use super::Result;

const MANIFEST_PATH: &str = "models/english2kana/upstream-manifest.json";
const VENDOR_ROOT: &str = ".vendors/english2kana";

#[derive(Debug, Deserialize)]
struct Manifest {
	model_id: String,
	revision: String,
	license: String,
	artifacts: Vec<Artifact>,
	conversion: Conversion,
}

#[derive(Debug, Deserialize)]
struct Artifact {
	name: String,
	url: String,
	sha256: String,
	size: u64,
}

#[derive(Debug, Deserialize)]
struct Conversion {
	python: String,
	packages: Vec<String>,
}

pub(crate) fn run(args: Vec<String>) -> Result<()> {
	if args.first().map(String::as_str) != Some("english2kana") {
		return Err("model requires a model name; supported: english2kana".into());
	}
	match args.get(1).map(String::as_str) {
		Some("prepare") => prepare(),
		Some("convert") => convert(),
		Some("verify") => verify(),
		Some("-h") | Some("--help") | None => {
			print_help();
			Ok(())
		}
		Some(other) => Err(format!("unknown model english2kana subcommand: {other}").into()),
	}
}

fn load_manifest(root: &Path) -> Result<Manifest> {
	let path = root.join(MANIFEST_PATH);
	let manifest: Manifest = serde_json::from_slice(&fs::read(&path)?)?;
	if manifest.model_id != "m7142yosuke/english2kana" || manifest.revision.len() != 40 || manifest.license != "MIT" {
		return Err(format!("invalid english2kana manifest identity: {}", path.display()).into());
	}
	Ok(manifest)
}

fn prepare() -> Result<()> {
	let root = super::repo_root()?;
	let manifest = load_manifest(&root)?;
	let source_dir = root.join(VENDOR_ROOT).join("sources");
	fs::create_dir_all(&source_dir)?;
	for artifact in &manifest.artifacts {
		let path = source_dir.join(&artifact.name);
		if !path.is_file() {
			super::download_file(&artifact.url, &path)?;
		}
		verify_artifact(&path, artifact)?;
		println!("verified {}", path.display());
	}
	write_local_readme(&root, &manifest)?;
	println!("english2kana sources prepared at {}", source_dir.display());
	Ok(())
}

fn convert() -> Result<()> {
	prepare()?;
	let root = super::repo_root()?;
	let manifest = load_manifest(&root)?;
	let vendor = root.join(VENDOR_ROOT);
	let venv = vendor.join(".venv");
	let output_dir = vendor.join("onnx");
	fs::create_dir_all(&output_dir)?;
	super::ensure_python_venv(&venv)?;

	let mut install = super::venv_python(&venv);
	install.args(["-m", "pip", "install", "--disable-pip-version-check"]);
	for package in &manifest.conversion.packages {
		install.arg(package);
	}
	super::run_command(&mut install)?;

	let source_dir = vendor.join("sources");
	let mut export = super::venv_python(&venv);
	export
		.arg(root.join("xtask/scripts/english2kana_export.py"))
		.arg("--model")
		.arg(source_dir.join("english2kana-v1.keras"))
		.arg("--english-tokenizer")
		.arg(source_dir.join("tokenizer_english.json"))
		.arg("--kana-tokenizer")
		.arg(source_dir.join("tokenizer_kana.json"))
		.arg("--output")
		.arg(output_dir.join("english2kana-v1.onnx"))
		.arg("--report")
		.arg(output_dir.join("verification.json"));
	super::run_command(&mut export)?;
	stage_runtime_assets(&root, &source_dir, &output_dir)?;
	verify()
}

fn stage_runtime_assets(root: &Path, source_dir: &Path, output_dir: &Path) -> Result<()> {
	for name in ["tokenizer_english.json", "tokenizer_kana.json"] {
		fs::copy(source_dir.join(name), output_dir.join(name))?;
	}
	fs::copy(
		root.join("models/english2kana/LICENSE.upstream"),
		output_dir.join("LICENSE.english2kana"),
	)?;
	fs::copy(root.join(MANIFEST_PATH), output_dir.join("upstream-manifest.json"))?;
	build_known_readings(source_dir, &output_dir.join("known-readings.tsv"))?;
	build_jmdict_readings(source_dir, &output_dir.join("jmdict-readings.tsv"))?;
	for name in [
		"cmudict.dict",
		"LICENSE.cmudict",
		"LICENSE.arpakana",
		"LICENSE.jmdict",
		"README.english2kana-dataset.md",
	] {
		fs::copy(source_dir.join(name), output_dir.join(name))?;
	}
	Ok(())
}

#[derive(Debug, Deserialize)]
struct ReadingRow {
	name: String,
	name_kana: String,
}

/// 学習元から単独最多の読みを抽出する。票数は実行時の候補優先度に使う。
fn build_known_readings(source_dir: &Path, output: &Path) -> Result<()> {
	let mut counts: HashMap<String, HashMap<String, usize>> = HashMap::new();
	for name in ["english2kana-train.csv", "english2kana-test.csv"] {
		let mut reader = csv::Reader::from_path(source_dir.join(name))?;
		for row in reader.deserialize::<ReadingRow>() {
			let row = row?;
			let word = row.name.trim().to_ascii_lowercase();
			let kana = row.name_kana.trim();
			if word.is_empty() || !word.bytes().all(|byte| byte.is_ascii_alphabetic()) || kana.is_empty() {
				continue;
			}
			*counts.entry(word).or_default().entry(kana.to_string()).or_default() += 1;
		}
	}
	let mut selected = Vec::new();
	for (word, readings) in counts {
		let mut ranked = readings.into_iter().collect::<Vec<_>>();
		ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
		let Some((kana, votes)) = ranked.first() else { continue };
		if ranked.get(1).is_none_or(|(_, runner_up)| votes > runner_up) {
			selected.push((word, kana.clone(), *votes));
		}
	}
	selected.sort_by(|left, right| left.0.cmp(&right.0));
	let mut file = fs::File::create(output)?;
	writeln!(file, "# word\tkana\tvotes")?;
	for (word, kana, votes) in selected {
		writeln!(file, "{word}\t{kana}\t{votes}")?;
	}
	Ok(())
}

#[derive(Debug, Deserialize)]
struct Jmdict {
	words: Vec<JmdictWord>,
}

#[derive(Debug, Deserialize)]
struct JmdictWord {
	kana: Vec<JmdictKana>,
	sense: Vec<JmdictSense>,
}

#[derive(Debug, Deserialize)]
struct JmdictKana {
	common: bool,
	text: String,
}

#[derive(Debug, Deserialize)]
struct JmdictSense {
	gloss: Vec<JmdictGloss>,
}

#[derive(Debug, Deserialize)]
struct JmdictGloss {
	lang: String,
	text: String,
}

/// 英語の完全一致見出しから、カタカナだけで書かれた日本語外来語候補を抽出する。
/// 意味訳や複数語は除外し、候補の最終選択はONNX/CMUdictとの近さで実行時に行う。
fn build_jmdict_readings(source_dir: &Path, output: &Path) -> Result<()> {
	let archive_path = source_dir.join("jmdict-eng-3.6.2+20260720135044.json.tgz");
	let decoder = GzDecoder::new(fs::File::open(&archive_path)?);
	let mut archive = tar::Archive::new(decoder);
	let mut dictionary = None;
	for entry in archive.entries()? {
		let entry = entry?;
		if entry.path()?.extension().is_some_and(|extension| extension == "json") {
			dictionary = Some(serde_json::from_reader::<_, Jmdict>(entry)?);
			break;
		}
	}
	let dictionary = dictionary.ok_or_else(|| format!("JMdict JSON not found in {}", archive_path.display()))?;
	let mut readings: HashMap<String, HashMap<String, bool>> = HashMap::new();
	for entry in dictionary.words {
		let kana = entry
			.kana
			.into_iter()
			.filter(|reading| reading.text.chars().all(is_katakana))
			.collect::<Vec<_>>();
		if kana.is_empty() {
			continue;
		}
		for gloss in entry.sense.into_iter().flat_map(|sense| sense.gloss) {
			let word = gloss.text.trim().to_ascii_lowercase();
			if gloss.lang == "eng" && !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_alphabetic()) {
				let candidates = readings.entry(word).or_default();
				for reading in &kana {
					candidates
						.entry(reading.text.clone())
						.and_modify(|common| *common |= reading.common)
						.or_insert(reading.common);
				}
			}
		}
	}
	let mut rows = readings
		.into_iter()
		.flat_map(|(word, candidates)| candidates.into_iter().map(move |(kana, common)| (word.clone(), kana, common)))
		.collect::<Vec<_>>();
	rows.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
	let mut file = fs::File::create(output)?;
	writeln!(file, "# word\tkatakana\tcommon")?;
	for (word, kana, common) in rows {
		writeln!(file, "{word}\t{kana}\t{}", u8::from(common))?;
	}
	Ok(())
}

fn is_katakana(ch: char) -> bool {
	matches!(ch as u32, 0x30a1..=0x30ff) || ch == 'ー' || ch == '・'
}

fn verify() -> Result<()> {
	let root = super::repo_root()?;
	verified_runtime_dir(&root).map(|_| ())
}

pub(crate) fn verified_runtime_dir(root: &Path) -> Result<std::path::PathBuf> {
	let manifest = load_manifest(&root)?;
	let vendor = root.join(VENDOR_ROOT);
	for artifact in &manifest.artifacts {
		verify_artifact(&vendor.join("sources").join(&artifact.name), artifact)?;
	}
	let model = vendor.join("onnx/english2kana-v1.onnx");
	let report = vendor.join("onnx/verification.json");
	let runtime_assets = [
		"tokenizer_english.json",
		"tokenizer_kana.json",
		"LICENSE.english2kana",
		"known-readings.tsv",
		"jmdict-readings.tsv",
		"cmudict.dict",
		"LICENSE.cmudict",
		"LICENSE.arpakana",
		"LICENSE.jmdict",
		"README.english2kana-dataset.md",
		"upstream-manifest.json",
	];
	if !model.is_file() || !report.is_file() || runtime_assets.iter().any(|name| !vendor.join("onnx").join(name).is_file()) {
		return Err("generated ONNX model or verification report is missing; run `cargo xtask model english2kana convert`".into());
	}
	let report_json: serde_json::Value = serde_json::from_slice(&fs::read(&report)?)?;
	if report_json.get("all_equal").and_then(serde_json::Value::as_bool) != Some(true) {
		return Err(format!("english2kana ONNX verification did not pass: {}", report.display()).into());
	}
	println!("verified ONNX model: {}", model.display());
	println!("onnx_sha256={}", super::sha256_file(&model)?);
	Ok(vendor.join("onnx"))
}

fn verify_artifact(path: &Path, artifact: &Artifact) -> Result<()> {
	let metadata = fs::metadata(path).map_err(|error| format!("missing artifact {}: {error}", path.display()))?;
	if metadata.len() != artifact.size {
		return Err(format!(
			"size mismatch for {}: expected {}, got {}",
			path.display(),
			artifact.size,
			metadata.len()
		)
		.into());
	}
	let actual = super::sha256_file(path)?;
	if actual != artifact.sha256 {
		return Err(format!("sha256 mismatch for {}: expected {}, got {actual}", path.display(), artifact.sha256).into());
	}
	Ok(())
}

fn write_local_readme(root: &Path, manifest: &Manifest) -> Result<()> {
	let vendor = root.join(VENDOR_ROOT);
	fs::write(
		vendor.join("README.local.md"),
		format!(
			"# english2kana ONNX workspace\n\nGenerated and ignored by Git.\n\n- upstream: `{}`\n- immutable revision: `{}`\n- license: `{}`\n- conversion Python: `{}` (isolated under `.venv`)\n- normal UNVAC build/runtime does not use Python or Keras\n",
			manifest.model_id, manifest.revision, manifest.license, manifest.conversion.python
		),
	)?;
	Ok(())
}

fn print_help() {
	println!(
		"model english2kana subcommands:\n\
  prepare    Download immutable upstream model, tokenizers, source and license; verify size/SHA-256.\n\
  convert    Create an isolated conversion venv, export ONNX, and compare Keras/ONNX outputs.\n\
  verify     Recheck pinned sources and the generated verification report."
	);
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn tracked_manifest_has_pinned_identity_and_hashes() {
		let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
		let manifest = load_manifest(root).unwrap();
		assert_eq!(manifest.revision, "c2227321a72cc19f432f23a025cbc66579473e0e");
		assert_eq!(manifest.artifacts.len(), 13);
		assert!(manifest
			.artifacts
			.iter()
			.all(|artifact| artifact.sha256.len() == 64 && artifact.size > 0));
		assert!(manifest.conversion.packages.iter().all(|package| package.contains("==")));
	}
}
