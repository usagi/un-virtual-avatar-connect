use std::{
	env,
	ffi::OsStr,
	fs,
	io::{Read, Write},
	path::{Path, PathBuf},
	process::{Command, Stdio},
};

use sha2::{Digest, Sha256};

mod ai_turn_eval;
mod avatar_control;
mod english2kana;
mod release_inventory;
mod stream_check;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub(crate) const DEFAULT_DESKTOP_BASE_URL: &str = "http://127.0.0.1:46000";

const PADDLEOCR_REPO: &str = "https://github.com/PaddlePaddle/PaddleOCR.git";
const DEFAULT_PADDLEOCR_REF: &str = "main";
const VENDOR_ROOT: &str = ".vendors/paddleocr-native";
const NATIVE_WRAPPER_DIR: &str = "native/paddleocr-inproc";
const ONNX_VENDOR_ROOT: &str = ".vendors/paddleocr-onnx";
const PADDLEPADDLE_PADDLE2ONNX_WIN_CP311: &str =
	"https://paddle-whl.bj.bcebos.com/nightly/cpu/paddlepaddle/paddlepaddle-3.0.0.dev20250426-cp311-cp311-win_amd64.whl";

fn main() {
	if let Err(e) = run() {
		eprintln!("xtask: {e}");
		std::process::exit(1);
	}
}

fn run() -> Result<()> {
	let mut args = env::args().skip(1);
	match args.next().as_deref() {
		Some("release-build") => release_build(args.collect()),
		Some("release-worker-build") => {
			let backend = match args.next().as_deref() {
				Some("cpu") => SttWorkerBackend::Cpu,
				Some("cpu-avx2") => SttWorkerBackend::CpuAvx2,
				Some("vulkan") => SttWorkerBackend::Vulkan,
				_ => return Err("release-worker-build requires cpu|cpu-avx2|vulkan and an output directory".into()),
			};
			let destination = PathBuf::from(args.next().ok_or("worker output directory required")?);
			if args.next().is_some() { return Err("unexpected release-worker-build argument".into()); }
			validate_release_environment(env::vars_os())?;
			if destination.join(backend.artifact_name()).exists() { return Err("worker output already exists; choose a new review directory".into()); }
			let root = repo_root()?;
			build_stt_worker(&root, &short_stt_worker_target_dir(&root, backend), &destination, backend)?;
			println!("worker sha256: {}", sha256_file(&destination.join(backend.artifact_name()))?);
			Ok(())
		},
        Some("release-files-check") => {
            let root = args.next().ok_or("release-files-check requires an artifact directory")?;
            if args.next().is_some() { return Err("release-files-check accepts one directory".into()); }
            let count = release_inventory::verify(Path::new(&root), |variant| Ok(release_file_names(ReleaseVariant::parse(variant)?)))?;
            println!("{count} release files match inventory; runtime and release acceptance are separate");
            Ok(())
        },
		Some("release-gui-check") => {
			let path=args.next().ok_or("release-gui-check requires a GUI output directory")?;
			if args.next().is_some() { return Err("release-gui-check accepts one directory".into()); }
			validate_release_gui(Path::new(&path))?;
			println!("normal GUI identity and file hashes verified: {path}");
			Ok(())
		},
		Some("stt-eval") => stt_eval(args.collect()),
		Some("ai-turn-eval") => ai_turn_eval::run(args.collect()),
		Some("avatar-control") => avatar_control::run(args.collect()),
		Some("stream-check") => stream_check::run(args.collect()),
		Some("flowgraph-check") => flowgraph_check(args.collect()),
		Some("model") => english2kana::run(args.collect()),
		Some("docs") => docs(args.collect()),
		Some("paddleocr-native") => paddleocr_native(args.collect()),
		Some("ocr-onnx") => ocr_onnx(args.collect()),
		Some("-h") | Some("--help") | None => {
			print_help();
			Ok(())
		}
		Some(other) => Err(format!("unknown xtask command: {other}").into()),
	}
}

fn flowgraph_check(args: Vec<String>) -> Result<()> {
	let root = repo_root()?;
	let mut flowgraph_root: Option<PathBuf> = None;
	let mut cli_path: Option<PathBuf> = None;
	let mut require_streaming_contract = false;
	let mut index = 0;
	while index < args.len() {
		match args[index].as_str() {
			"--root" => {
				let value = args.get(index + 1).ok_or("--root requires a directory")?;
				flowgraph_root = Some(PathBuf::from(value));
				index += 2;
			}
			"--cli" => {
				let value = args.get(index + 1).ok_or("--cli requires an executable path")?;
				cli_path = Some(PathBuf::from(value));
				index += 2;
			}
			"--require-streaming-contract" => {
				require_streaming_contract = true;
				index += 1;
			}
			other => return Err(format!("unknown flowgraph-check option: {other}").into()),
		}
	}
	let flowgraph_root = flowgraph_root.ok_or("flowgraph-check requires --root <directory>")?;
	let flowgraph_root = if flowgraph_root.is_absolute() {
		flowgraph_root
	} else {
		root.join(flowgraph_root)
	};
	if !flowgraph_root.is_dir() {
		return Err(format!("Flowgraph directory does not exist: {}", flowgraph_root.display()).into());
	}
	let cli_path = resolve_flowgraph_cli(&root, cli_path)?;
	let mut cli = Command::new(&cli_path);
	cli.current_dir(&root)
		.arg("--flowgraph-load-dir")
		.arg(&flowgraph_root)
		.arg("--flowgraph-test-json");
	if require_streaming_contract {
		cli.arg("--flowgraph-require-streaming-contract");
	}
	run_command(&mut cli)
}

fn resolve_flowgraph_cli(root: &Path, explicit: Option<PathBuf>) -> Result<PathBuf> {
	let candidates = if let Some(path) = explicit {
		vec![if path.is_absolute() { path } else { root.join(path) }]
	} else {
		let executable = format!("virtual-avatar-connect-cli{}", env::consts::EXE_SUFFIX);
		vec![
			root.join("target/release").join(&executable),
			root.join("target/debug").join(executable),
		]
	};
	candidates.into_iter().find(|path| path.is_file()).ok_or_else(|| {
		"VAC CLIが見つからない。先に `cargo xtask release-build --variant portable` を実行するか、`--cli <path>` で指定する。".into()
	})
}

fn stt_eval(args: Vec<String>) -> Result<()> {
	let root = repo_root()?;
	if args.first().map(String::as_str) == Some("web-speech") {
		let mut node = Command::new("node");
		node.current_dir(root.join("gui"))
			.arg("scripts/web-speech-probe.mjs")
			.args(args.iter().skip(1));
		return run_command(&mut node);
	}
	let features = match args.first().map(String::as_str) {
		Some("whisper") => "tools,stt-eval-whisper",
		Some("worker") => "tools,voice-whisper",
		_ => "tools",
	};
	let mut cargo = Command::new("cargo");
	cargo
		.current_dir(&root)
		.arg("run")
		.arg("--features")
		.arg(features)
		.arg("--bin")
		.arg("vac-stt-eval")
		.arg("--")
		.args(args);
	run_command(&mut cargo)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReleaseVariant {
	Portable,
	Minimal,
}

impl ReleaseVariant {
	fn parse(value: &str) -> Result<Self> {
		match value {
			"portable" => Ok(Self::Portable),
			"minimal" => Ok(Self::Minimal),
			other => Err(format!("unknown release variant: {other}; expected portable or minimal").into()),
		}
	}

	fn name(self) -> &'static str {
		match self {
			Self::Portable => "portable",
			Self::Minimal => "minimal",
		}
	}

	fn features(self) -> &'static str {
		match self {
			Self::Portable => "voice-vosk,embed-gui,voice-whisper",
			Self::Minimal => "voice-vosk,embed-gui",
		}
	}

	fn uses_vulkan(self) -> bool {
		self == Self::Portable
	}

	fn release_compatibility(self) -> &'static str {
		match self {
			Self::Portable => "general distribution (Vosk + optional Vulkan Whisper + CPU fallback)",
			Self::Minimal => "specialized lightweight build (Vosk only; Whisper unavailable)",
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SttWorkerBackend {
	Cpu,
	CpuAvx2,
	Vulkan,
}

impl SttWorkerBackend {
	fn feature(self) -> Option<&'static str> {
		match self {
			Self::Cpu | Self::CpuAvx2 => None,
			Self::Vulkan => Some("vulkan"),
		}
	}

	fn artifact_name(self) -> &'static str {
		match self {
			Self::Cpu => "vac-stt-worker-cpu.exe",
			Self::CpuAvx2 => "vac-stt-worker-cpu-avx2.exe",
			Self::Vulkan => "vac-stt-worker-vulkan.exe",
		}
	}
}

#[derive(Debug, PartialEq, Eq)]
struct ReleaseBuildOptions {
	variant: ReleaseVariant,
	target_dir: Option<String>,
	skip_npm_ci: bool,
	reuse_gui_dist: bool,
}

fn parse_release_build_options(args: &[String]) -> Result<ReleaseBuildOptions> {
	let mut target_dir = None;
	let mut skip_npm_ci = false;
	let mut reuse_gui_dist = false;
	let mut selected_variant: Option<ReleaseVariant> = None;
	let mut index = 0;
	while index < args.len() {
		let (variant, consumed) = match args[index].as_str() {
			"--variant" => {
				let value = args.get(index + 1).ok_or("--variant requires a value")?;
				(Some(ReleaseVariant::parse(value)?), 2)
			}
			"--voice-whisper" => (Some(ReleaseVariant::Portable), 1),
			"--minimal" => (Some(ReleaseVariant::Minimal), 1),
			"--skip-npm-ci" => {
				skip_npm_ci = true;
				(None, 1)
			}
			"--reuse-gui-dist" => {
				reuse_gui_dist = true;
				(None, 1)
			}
			"--target-dir" => {
				let value = args.get(index + 1).ok_or("--target-dir requires a path")?;
				target_dir = Some(value.clone());
				(None, 2)
			}
			other => return Err(format!("unknown release-build option: {other}").into()),
		};
		if let Some(variant) = variant {
			if selected_variant.is_some_and(|selected| selected != variant) {
				return Err("release build variants are mutually exclusive".into());
			}
			selected_variant = Some(variant);
		}
		index += consumed;
	}

	Ok(ReleaseBuildOptions {
		variant: selected_variant.unwrap_or(ReleaseVariant::Portable),
		target_dir,
		skip_npm_ci,
		reuse_gui_dist,
	})
}

fn validate_release_gui(dist: &Path) -> Result<()> {
	let identity: serde_json::Value = serde_json::from_slice(&fs::read(dist.join("unvac-ui-build.json"))?)?;
	if identity["schema_version"] != 1 || identity["design_preview"] != false {
		return Err("GUI is a design preview or has unknown build identity; rebuild the normal UI".into());
	}
	let files = identity["files"].as_object().ok_or("GUI identity has no file hashes")?;
	if !files.contains_key("index.html") || !files.keys().any(|name| name.ends_with(".js")) {
		return Err("GUI identity lacks index.html or JavaScript".into());
	}
	for (name, expected) in files {
		let path = Path::new(name);
		if path.components().any(|part| !matches!(part, std::path::Component::Normal(_))) {
			return Err("GUI identity contains an invalid relative path".into());
		}
		if expected.as_str() != Some(sha256_file(&dist.join(path))?.as_str()) {
			return Err(format!("GUI file changed after build: {name}").into());
		}
	}
	// The embedded GUI includes the entire directory. A valid subset of hashes
	// must not bless stale HTML/scripts/styles left beside the verified build.
	fn check_inventory(root: &Path, directory: &Path, files: &serde_json::Map<String, serde_json::Value>) -> Result<()> {
		for entry in fs::read_dir(directory)? {
			let entry = entry?;
			let path = entry.path();
			let kind = entry.file_type()?;
			if kind.is_symlink() {
				return Err(format!("GUI build contains a symbolic link: {}", path.display()).into());
			}
			if kind.is_dir() {
				check_inventory(root, &path, files)?;
			} else if matches!(path.extension().and_then(|s| s.to_str()), Some("html" | "js" | "css")) {
				let name = path.strip_prefix(root)?.to_string_lossy().replace('\\', "/");
				if !files.contains_key(&name) {
					return Err(format!("GUI file is absent from build identity: {name}").into());
				}
			}
		}
		Ok(())
	}
	check_inventory(dist, dist, files)?;
	Ok(())
}

// Windows Vosk distribution also needs the MinGW runtime DLLs beside the EXE.
// build.rs copy failures are warnings; release acceptance must fail closed.
fn validate_windows_vosk_runtime(artifact_dir: &Path) -> Result<()> {
	for name in ["libvosk.dll", "libgcc_s_seh-1.dll", "libstdc++-6.dll", "libwinpthread-1.dll"] {
		let path = artifact_dir.join(name);
		let metadata = fs::metadata(&path).map_err(|error| format!("required Vosk runtime missing: {}: {error}", path.display()))?;
		if !metadata.is_file() || metadata.len() == 0 {
			return Err(format!("required Vosk runtime is not a nonempty file: {}", path.display()).into());
		}
		println!("release runtime: {} ({} bytes, sha256={})", path.display(), metadata.len(), sha256_file(&path)?);
	}
	Ok(())
}

const VOSK_RUNTIME_NOTICES: &[&str] = &[
	"MinGW-w64-8.0.0-COPYING.txt",
	"MinGW-w64-8.0.0-runtime.txt",
	"MinGW-w64-8.0.0-notice.txt",
	"Winpthreads-8.0.0.txt",
	"GCC-MinGW-10.2.1-6+24.2-copyright.txt",
	"GPL-3.0.txt",
];

fn validate_vosk_notice(root: &Path, artifact_dir: &Path) -> Result<()> {
	let source: serde_json::Value = serde_json::from_slice(&fs::read(root.join("licenses/Vosk-0.3.45.source.json"))?)?;
	if source["license_sha256"].as_str() != Some(sha256_file(&root.join("licenses/Vosk-0.3.45.txt"))?.as_str()) {
		return Err("Vosk notice differs from its recorded upstream source".into());
	}
	if cfg!(windows) {
		for name in VOSK_RUNTIME_NOTICES {
			if source["runtime_notices"][*name]["sha256"].as_str() != Some(sha256_file(&root.join("licenses").join(name))?.as_str()) {
				return Err(format!("Runtime notice {name} differs from its recorded upstream source").into());
			}
		}
		for name in ["libvosk.dll", "libgcc_s_seh-1.dll", "libstdc++-6.dll", "libwinpthread-1.dll"] {
			if source["dll_sha256"][name].as_str() != Some(sha256_file(&artifact_dir.join(name))?.as_str()) {
				return Err(format!("{name} differs from the reviewed Vosk distribution; verify notices for the selected VOSK_LIB_PATH/VOSK_WIN64_VERSION before release").into());
			}
		}
	}
	Ok(())
}

const ENGLISH_RUNTIME_FILES: &[&str] = &[
		"english2kana-v1.onnx",
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
		"verification.json",
];

fn release_binary_names(variant: ReleaseVariant) -> Vec<&'static str> {
    let mut names = vec!["virtual-avatar-connect-desktop.exe", "virtual-avatar-connect-cli.exe",
        if cfg!(windows) { "vac-english2kana-worker.exe" } else { "vac-english2kana-worker" }];
    if variant != ReleaseVariant::Minimal { names.extend(["vac-stt-worker-cpu.exe", "vac-stt-worker-cpu-avx2.exe"]); }
    if variant.uses_vulkan() { names.push("vac-stt-worker-vulkan.exe"); }
    names
}

fn release_file_names(variant: ReleaseVariant) -> Vec<String> {
    let mut names = release_binary_names(variant);
    names.extend(RELEASE_DOCUMENTS.iter().map(|(_, name)| *name));
    names.push("licenses/THIRD-PARTY-RUST.txt");
    names.push("licenses/Vosk-0.3.45.txt");
    names.push("licenses/Vosk-0.3.45.source.json");
    names.extend(["licenses/THIRD-PARTY-FRONTEND.txt", "licenses/frontend-notices.json"]);
    if cfg!(windows) { names.extend(["libvosk.dll", "libgcc_s_seh-1.dll", "libstdc++-6.dll", "libwinpthread-1.dll"]); }
    let runtime_notices = VOSK_RUNTIME_NOTICES.iter().filter(|_| cfg!(windows)).map(|name| format!("licenses/{name}"));
    names.into_iter().map(str::to_owned).chain(runtime_notices).chain(ENGLISH_RUNTIME_FILES.iter().map(|name| format!("models/english2kana/{name}"))).collect()
}

fn validate_frontend_notices(gui: &Path, notices: &Path) -> Result<()> {
	let record: serde_json::Value = serde_json::from_slice(&fs::read(notices.join("frontend-notices.json"))?)?;
	for (field, file) in [
		("ui_identity_sha256", gui.join("dist/unvac-ui-build.json")),
		("package_lock_sha256", gui.join("package-lock.json")),
		("app_css_sha256", gui.join("src/app.css")),
		("notice_sha256", notices.join("THIRD-PARTY-FRONTEND.txt")),
	] {
		if record[field].as_str() != Some(sha256_file(&file)?.as_str()) {
			return Err(format!("Frontend notice identity mismatch: {field}").into());
		}
	}
	if record["packages"].as_array().is_none_or(|entries| entries.is_empty()) {
		return Err("Frontend notice package list is missing or empty".into());
	}
	Ok(())
}

fn release_build(args: Vec<String>) -> Result<()> {
	let options = parse_release_build_options(&args)?;
	validate_release_environment(env::vars_os())?;
	let root = repo_root()?;
	let english2kana_source = english2kana::verified_runtime_dir(&root)?;
	let target_dir = options
		.target_dir
		.map(PathBuf::from)
		.map(|path| if path.is_absolute() { path } else { root.join(path) })
		.unwrap_or_else(|| root.join("target"));
	let gui = root.join("gui");
	if options.reuse_gui_dist {
		validate_release_gui(&gui.join("dist"))?;
		println!("reusing verified normal GUI dist: {}", gui.join("dist").display());
	} else {
		if !options.skip_npm_ci {
			let mut npm_ci = npm_command();
			npm_ci.current_dir(&gui).arg("ci");
			run_command(&mut npm_ci)?;
		}
		let mut npm_build = npm_command();
		npm_build.current_dir(&gui).env("VITE_UN_DESIGN_PREVIEW", "0").arg("run").arg("build");
		run_command(&mut npm_build)?;
		validate_release_gui(&gui.join("dist"))?;
	}

	println!("release variant: {}", options.variant.name());
	println!("release compatibility: {}", options.variant.release_compatibility());
	let artifact_dir = target_dir.join("release");
	remove_if_exists(&artifact_dir.join(release_inventory::FILE))?;
	fs::create_dir_all(&target_dir)?;
	let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
	let frontend_notices = target_dir.join(format!("frontend-notices-{}-{nonce}", std::process::id()));
	run_command(Command::new("node").current_dir(&root).arg("scripts/collect-frontend-notices.mjs").arg(&frontend_notices))?;
	validate_frontend_notices(&gui, &frontend_notices)?;
	stage_release_guide(&root, &artifact_dir)?;
	let mut cargo = Command::new("cargo");
	cargo
		.current_dir(&root)
		.arg("build")
		.arg("--release")
		.arg("--no-default-features")
		.arg("--target-dir")
		.arg(&target_dir)
		.arg("--features")
		.arg(options.variant.features())
		.arg("--bin")
		.arg("virtual-avatar-connect-desktop")
		.arg("--bin")
		.arg("virtual-avatar-connect-cli");
	run_command(&mut cargo)?;

	if cfg!(windows) {
		validate_windows_vosk_runtime(&artifact_dir)?;
	}
	validate_vosk_notice(&root, &artifact_dir)?;
	fs::create_dir_all(artifact_dir.join("licenses"))?;
	run_command(Command::new("python").current_dir(&root)
		.arg("scripts/collect-release-rust-licenses.py")
		.arg(artifact_dir.join("licenses/THIRD-PARTY-RUST.txt"))
		.arg(options.variant.name()))?;
	validate_release_gui(&gui.join("dist"))?;
	validate_frontend_notices(&gui, &frontend_notices)?;
	fs::create_dir_all(artifact_dir.join("licenses"))?;
	for name in ["THIRD-PARTY-FRONTEND.txt", "frontend-notices.json"] {
		fs::copy(frontend_notices.join(name), artifact_dir.join("licenses").join(name))?;
	}
	fs::copy(root.join("licenses/Vosk-0.3.45.txt"), artifact_dir.join("licenses/Vosk-0.3.45.txt"))?;
	fs::copy(root.join("licenses/Vosk-0.3.45.source.json"), artifact_dir.join("licenses/Vosk-0.3.45.source.json"))?;
	if cfg!(windows) {
		for name in VOSK_RUNTIME_NOTICES {
			fs::copy(root.join("licenses").join(name), artifact_dir.join("licenses").join(name))?;
		}
	}
	build_english2kana_worker(&root, &artifact_dir)?;
	let english2kana_destination = artifact_dir.join("models/english2kana");
	fs::create_dir_all(&english2kana_destination)?;
	for name in ENGLISH_RUNTIME_FILES {
		let destination = english2kana_destination.join(name);
		fs::copy(english2kana_source.join(name), &destination)?;
		println!(
			"release model asset: {} ({} bytes, sha256={})",
			destination.display(),
			destination.metadata()?.len(),
			sha256_file(&destination)?
		);
	}
	if options.variant != ReleaseVariant::Minimal {
		let cpu_target_dir = short_stt_worker_target_dir(&root, SttWorkerBackend::Cpu);
		build_stt_worker(&root, &cpu_target_dir, &artifact_dir, SttWorkerBackend::Cpu)?;
		let avx2_target_dir = short_stt_worker_target_dir(&root, SttWorkerBackend::CpuAvx2);
		build_stt_worker(&root, &avx2_target_dir, &artifact_dir, SttWorkerBackend::CpuAvx2)?;
	} else {
		remove_if_exists(&artifact_dir.join("vac-stt-worker-cpu.exe"))?;
		remove_if_exists(&artifact_dir.join("vac-stt-worker-cpu-avx2.exe"))?;
	}
	remove_if_exists(&artifact_dir.join("vac-stt-worker-cuda.exe"))?;
	if options.variant.uses_vulkan() {
		let vulkan_target_dir = short_stt_worker_target_dir(&root, SttWorkerBackend::Vulkan);
		build_stt_worker(&root, &vulkan_target_dir, &artifact_dir, SttWorkerBackend::Vulkan)?;
	} else {
		remove_if_exists(&artifact_dir.join("vac-stt-worker-vulkan.exe"))?;
	}

	let artifact_names = release_binary_names(options.variant);
	for name in &artifact_names {
		let artifact = artifact_dir.join(name);
		if !artifact.is_file() {
			return Err(format!("release artifact was not produced: {}", artifact.display()).into());
		}
		println!(
			"release artifact: {} ({} bytes, sha256={})",
			artifact.display(),
			artifact.metadata()?.len(),
			sha256_file(&artifact)?
		);
	}
	release_inventory::write(&artifact_dir, options.variant.name(), &release_file_names(options.variant))?;
	Ok(())
}

const RELEASE_DOCUMENTS: &[(&str, &str)] = &[
    ("docs/release/START-HERE.txt", "START-HERE.txt"),
    ("LICENSE", "LICENSE"),
    ("docs/manual/ai-stream-perception.md", "manual/ai-stream-perception.md"),
    ("docs/manual/conf-reference.md", "manual/conf-reference.md"),
    ("docs/manual/datetime-system.md", "manual/datetime-system.md"),
    ("docs/manual/dimensional-quantity-system.md", "manual/dimensional-quantity-system.md"),
    ("docs/manual/flowgraph-composition.md", "manual/flowgraph-composition.md"),
    ("docs/manual/flowgraph-enum-and-library.md", "manual/flowgraph-enum-and-library.md"),
    ("docs/manual/index.md", "manual/index.md"),
    ("docs/manual/update-and-recovery.md", "manual/update-and-recovery.md"),
    ("docs/manual/tutorials/un-suite-vmc-hub.md", "manual/tutorials/un-suite-vmc-hub.md"),
    ("conf.example-un-suite.toml", "conf.example-un-suite.toml"),
    ("flowgraph.example/un-suite-vmc-monitor/main.flowgraph.toml", "flowgraph.example/un-suite-vmc-monitor/main.flowgraph.toml"),
    ("flowgraph.example/vmc-ai-mode-control/main.flowgraph.toml", "flowgraph.example/vmc-ai-mode-control/main.flowgraph.toml"),
    ("flowgraph.example/vmc-blendshape-trigger/main.flowgraph.toml", "flowgraph.example/vmc-blendshape-trigger/main.flowgraph.toml"),
    ("flowgraph.example/vmc-obs-scene-control/main.flowgraph.toml", "flowgraph.example/vmc-obs-scene-control/main.flowgraph.toml"),
    ("flowgraph.example/vmc-udp-ingress/main.flowgraph.toml", "flowgraph.example/vmc-udp-ingress/main.flowgraph.toml"),
    ("docs/manual/node-catalog.md", "manual/node-catalog.md"),
    ("docs/manual/obs-browser-sources.md", "manual/obs-browser-sources.md"),
    ("docs/manual/quickstart.md", "manual/quickstart.md"),
    ("docs/manual/tts-playback-output.md", "manual/tts-playback-output.md"),
    ("docs/manual/tutorials/avatar-ai-control.md", "manual/tutorials/avatar-ai-control.md"),
    ("docs/manual/tutorials/chat-echo.md", "manual/tutorials/chat-echo.md"),
    ("docs/manual/tutorials/chat-filters.md", "manual/tutorials/chat-filters.md"),
    ("docs/manual/tutorials/dictionary-editor-and-quick-add.md", "manual/tutorials/dictionary-editor-and-quick-add.md"),
    ("docs/manual/tutorials/english-pronunciation.md", "manual/tutorials/english-pronunciation.md"),
    ("docs/manual/tutorials/flowgraph-fixtures.md", "manual/tutorials/flowgraph-fixtures.md"),
    ("docs/manual/tutorials/gui-e2e.md", "manual/tutorials/gui-e2e.md"),
    ("docs/manual/tutorials/motion-unmfz.md", "manual/tutorials/motion-unmfz.md"),
    ("docs/manual/tutorials/ocr-screencap.md", "manual/tutorials/ocr-screencap.md"),
    ("docs/manual/tutorials/openai-persona.md", "manual/tutorials/openai-persona.md"),
    ("docs/manual/tutorials/stream-control.md", "manual/tutorials/stream-control.md"),
    ("docs/manual/tutorials/translate-multilang.md", "manual/tutorials/translate-multilang.md"),
    ("docs/manual/tutorials/tts-bouyomichan.md", "manual/tutorials/tts-bouyomichan.md"),
    ("docs/manual/tutorials/tts-coeiroink.md", "manual/tutorials/tts-coeiroink.md"),
    ("docs/manual/tutorials/tts-os.md", "manual/tutorials/tts-os.md"),
    ("docs/manual/tutorials/tts-voicepeak.md", "manual/tutorials/tts-voicepeak.md"),
    ("docs/manual/tutorials/tts-voicevox.md", "manual/tutorials/tts-voicevox.md"),
    ("docs/manual/tutorials/twitch-chat-send.md", "manual/tutorials/twitch-chat-send.md"),
    ("docs/manual/tutorials/twitch-echo.md", "manual/tutorials/twitch-echo.md"),
    ("docs/manual/tutorials/twitch-moderation.md", "manual/tutorials/twitch-moderation.md"),
    ("docs/manual/tutorials/un-app-control.md", "manual/tutorials/un-app-control.md"),
    ("docs/manual/tutorials/voice-input.md", "manual/tutorials/voice-input.md"),
    ("docs/manual/twitch-channel-points.md", "manual/twitch-channel-points.md"),
    ("docs/manual/un-app-api-over-zenoh.md", "manual/un-app-api-over-zenoh.md"),
    ("docs/manual/un-series-release-gate.md", "manual/un-series-release-gate.md"),
    ("docs/manual/v1-to-v2-migration.md", "manual/v1-to-v2-migration.md"),
    ("docs/manual/v2-release-gate.md", "manual/v2-release-gate.md"),
    ("docs/manual/xtask.md", "manual/xtask.md"),
];

fn stage_release_guide(root: &Path, destination: &Path) -> Result<()> {
	for &(source, name) in RELEASE_DOCUMENTS {
		let content = fs::read(root.join(source))?;
		if content.is_empty() { return Err(format!("release document is empty: {source}").into()); }
		let path = destination.join(name);
		fs::create_dir_all(path.parent().ok_or("release document has no parent")?)?;
		fs::write(path, content)?;
	}
	Ok(())
}

// Reject inherited compiler overrides rather than silently discarding a caller's
// settings or producing host-specific binaries under a portable label.
fn validate_release_environment(variables: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>) -> Result<()> {
	let mut unsupported = Vec::new();
	for (name, value) in variables {
		if value.is_empty() { continue; }
		let key = name.to_string_lossy().to_ascii_uppercase();
		if matches!(key.as_str(), "RUSTFLAGS" | "CARGO_ENCODED_RUSTFLAGS" | "CARGO_BUILD_RUSTFLAGS"
			| "CARGO_BUILD_TARGET" | "CARGO_TARGET_DIR" | "CL" | "_CL_" | "CFLAGS" | "CXXFLAGS")
			|| key.starts_with("CMAKE_") || key.starts_with("GGML_") || key.starts_with("WHISPER_")
			|| (key.starts_with("CARGO_TARGET_") && key.ends_with("_RUSTFLAGS"))
		{
			unsupported.push(key);
		}
	}
	unsupported.sort();
	if !unsupported.is_empty() {
		return Err(format!("portable release refuses inherited build overrides: {}. Clear them for this invocation; use --target-dir for artifact placement. Values were not logged.", unsupported.join(", ")).into());
	}
	Ok(())
}

fn build_english2kana_worker(root: &Path, artifact_dir: &Path) -> Result<()> {
	let worker_target_dir = root.join("target/english2kana-worker");
	let mut cargo = Command::new("cargo");
	cargo
		.current_dir(root)
		.arg("build")
		.arg("--release")
		.arg("--target-dir")
		.arg(&worker_target_dir)
		.arg("-p")
		.arg("vac-english2kana-worker");
	run_command(&mut cargo)?;
	fs::create_dir_all(artifact_dir)?;
	let name = if cfg!(windows) {
		"vac-english2kana-worker.exe"
	} else {
		"vac-english2kana-worker"
	};
	fs::copy(worker_target_dir.join("release").join(name), artifact_dir.join(name))?;
	Ok(())
}

fn build_stt_worker(root: &Path, worker_target_dir: &Path, artifact_dir: &Path, backend: SttWorkerBackend) -> Result<Option<PathBuf>> {
	let mut cargo = Command::new("cargo");
	cargo
		.current_dir(root)
		.arg("build")
		.arg("--release")
		.arg("--target-dir")
		.arg(worker_target_dir)
		.arg("-p")
		.arg("vac-stt-whisper-worker");
	if let Some(feature) = backend.feature() {
		cargo.arg("--features").arg(feature);
	}
	cargo
		.env("CMAKE_C_FLAGS_RELEASE", "/O2 /Ob2 /DNDEBUG")
		.env("CMAKE_CXX_FLAGS_RELEASE", "/O2 /Ob2 /DNDEBUG");
	configure_worker_cpu_isa(&mut cargo, backend);
	let dependency_root = match backend {
		SttWorkerBackend::Cpu | SttWorkerBackend::CpuAvx2 => {
			cargo.env("GGML_OPENMP", "OFF").env("GGML_LTO", "OFF");
			None
		}
		SttWorkerBackend::Vulkan => Some(configure_vulkan_environment(&mut cargo)?),
	};
	run_command(&mut cargo)?;
	#[cfg(windows)]
	verify_msvc_worker_isa(worker_target_dir, backend)?;
	#[cfg(windows)]
	if matches!(backend, SttWorkerBackend::Cpu | SttWorkerBackend::CpuAvx2) {
		verify_msvc_cpu_optimization(worker_target_dir)?;
	}
	fs::create_dir_all(artifact_dir)?;
	let source = worker_target_dir.join("release").join("vac-stt-worker.exe");
	let destination = artifact_dir.join(backend.artifact_name());
	if source != destination {
		fs::copy(&source, &destination)?;
	}
	Ok(dependency_root)
}

// Vulkan also compiles CPU code; do not inherit the build host's instruction set.
fn configure_worker_cpu_isa(command: &mut Command, backend: SttWorkerBackend) {
	command.env("GGML_NATIVE", "OFF");
	for flag in ["GGML_SSE42", "GGML_AVX", "GGML_AVX2", "GGML_FMA", "GGML_F16C", "GGML_BMI2",
		"GGML_AVX_VNNI", "GGML_AVX512", "GGML_AVX512_VBMI", "GGML_AVX512_VNNI", "GGML_AVX512_BF16",
		"GGML_AMX_TILE", "GGML_AMX_INT8", "GGML_AMX_BF16"] {
		command.env(flag, "OFF");
	}
	if backend == SttWorkerBackend::CpuAvx2 {
		for flag in ["GGML_SSE42", "GGML_AVX", "GGML_AVX2", "GGML_FMA", "GGML_F16C"] {
			command.env(flag, "ON");
		}
	}
}

fn worker_project_isa_matches(project: &str, backend: SttWorkerBackend) -> bool {
	if project.contains("AdvancedVectorExtensions512") || project.contains("/arch:AVX512") || project.contains("GGML_AVX512") {
		return false;
	}
	let avx2 = project.contains("<EnableEnhancedInstructionSet>AdvancedVectorExtensions2</EnableEnhancedInstructionSet>");
	if backend == SttWorkerBackend::CpuAvx2 {
		avx2 && project.contains("GGML_AVX2") && project.contains("GGML_FMA") && project.contains("GGML_F16C")
	} else {
		!project.contains("<EnableEnhancedInstructionSet>") && !project.contains("/arch:")
			&& !project.contains("GGML_AVX") && !project.contains("GGML_FMA") && !project.contains("GGML_F16C")
	}
}

#[cfg(windows)]
fn verify_msvc_worker_isa(root: &Path, backend: SttWorkerBackend) -> Result<()> {
	fn inspect(root: &Path, backend: SttWorkerBackend, found: &mut usize) -> Result<()> {
		for entry in fs::read_dir(root)? {
			let path = entry?.path();
			if path.is_dir() { inspect(&path, backend, found)?; }
			else if backend == SttWorkerBackend::Vulkan && path.file_name().is_some_and(|name| name == "build.ninja") {
				let project = fs::read_to_string(&path)?;
				if project.contains("ggml-cpu.cpp.obj") {
					*found += 1;
					if !worker_project_isa_matches(&project, backend) {
						return Err(format!("Vulkan worker CPU instruction set differs from baseline: {}", path.display()).into());
					}
				}
			}
			else if path.file_name().is_some_and(|name| name == "ggml-cpu.vcxproj") {
				*found += 1;
				if !worker_project_isa_matches(&fs::read_to_string(&path)?, backend) {
					return Err(format!("worker instruction set differs from {:?}: {}", backend, path.display()).into());
				}
			}
		}
		Ok(())
	}
	let mut found = 0;
	inspect(root, backend, &mut found)?;
	if found == 0 { return Err("worker CPU compiler project was not found; ISA verification unavailable".into()); }
	Ok(())
}

#[cfg(windows)]
fn verify_msvc_cpu_optimization(target_dir: &Path) -> Result<()> {
	let cache_ok = tree_file_contains(target_dir, "CMakeCache.txt", "CMAKE_CXX_FLAGS_RELEASE:STRING=/O2 /Ob2 /DNDEBUG")
		&& tree_file_contains(target_dir, "CMakeCache.txt", "CMAKE_C_FLAGS_RELEASE:STRING=/O2 /Ob2 /DNDEBUG");
	let project_ok = tree_file_contains(target_dir, "ggml-cpu.vcxproj", "<Optimization>MaxSpeed</Optimization>");
	if !cache_ok || !project_ok {
		return Err(format!(
			"CPU Whisper worker was not compiled with MSVC /O2; discard the stale target directory {}",
			target_dir.display()
		)
		.into());
	}
	Ok(())
}

#[cfg(windows)]
fn tree_file_contains(root: &Path, file_name: &str, needle: &str) -> bool {
	let Ok(entries) = fs::read_dir(root) else {
		return false;
	};
	for entry in entries.filter_map(std::result::Result::ok) {
		let path = entry.path();
		if path.is_dir() && tree_file_contains(&path, file_name, needle) {
			return true;
		}
		if path.file_name().is_some_and(|name| name == file_name) && fs::read_to_string(&path).is_ok_and(|content| content.contains(needle))
		{
			return true;
		}
	}
	false
}

#[cfg(windows)]
fn configure_vulkan_environment(command: &mut Command) -> Result<PathBuf> {
	let sdk = env::var_os("VULKAN_SDK")
		.map(PathBuf::from)
		.filter(|path| path.is_dir())
		.or_else(|| {
			let root = PathBuf::from(r"C:\VulkanSDK");
			fs::read_dir(root)
				.ok()?
				.filter_map(std::result::Result::ok)
				.map(|entry| entry.path())
				.filter(|path| path.join("Lib").join("vulkan-1.lib").is_file())
				.max()
		})
		.ok_or("Vulkan SDK was not found; install KhronosGroup.VulkanSDK before building the Vulkan worker")?;
	let ninja = find_ninja().ok_or("Ninja was not found; install Ninja-build.Ninja before building the Vulkan worker")?;
	let msvc_path = apply_msvc_environment(command)?;
	let mut paths = vec![sdk.join("Bin"), ninja.parent().unwrap_or(Path::new(".")).to_path_buf()];
	if let Some(msvc_path) = msvc_path {
		paths.extend(env::split_paths(&msvc_path));
	}
	paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
	command
		.env("VULKAN_SDK", &sdk)
		.env("PATH", env::join_paths(paths)?)
		.env("CMAKE_GENERATOR", "Ninja")
		.env("CMAKE_MAKE_PROGRAM", &ninja);
	println!("Vulkan SDK: {}", sdk.display());
	Ok(sdk)
}

#[cfg(windows)]
fn short_stt_worker_target_dir(root: &Path, backend: SttWorkerBackend) -> PathBuf {
	let digest = Sha256::digest(root.as_os_str().to_string_lossy().as_bytes());
	let key = digest[..6].iter().map(|byte| format!("{byte:02x}")).collect::<String>();
	let drive = env::var_os("SystemDrive").unwrap_or_else(|| "C:".into());
	let base = PathBuf::from(format!("{}\\", drive.to_string_lossy())).join("u").join(key);
	match backend {
		SttWorkerBackend::Cpu => base.join("b1"),
		SttWorkerBackend::CpuAvx2 => base.join("a2"),
		SttWorkerBackend::Vulkan => base.join("v1"),
	}
}

#[cfg(not(windows))]
fn short_stt_worker_target_dir(root: &Path, backend: SttWorkerBackend) -> PathBuf {
	let backend_dir = match backend {
		SttWorkerBackend::Cpu => "stt-worker-cpu-baseline-v1",
		SttWorkerBackend::CpuAvx2 => "stt-worker-cpu-avx2-v1",
		SttWorkerBackend::Vulkan => "stt-worker-vulkan-baseline-v1",
	};
	root.join("target").join(backend_dir)
}

#[cfg(windows)]
fn apply_msvc_environment(command: &mut Command) -> Result<Option<std::ffi::OsString>> {
	let program_files_x86 = env::var_os("ProgramFiles(x86)").ok_or("ProgramFiles(x86) is unavailable")?;
	let vswhere = PathBuf::from(program_files_x86)
		.join("Microsoft Visual Studio")
		.join("Installer")
		.join("vswhere.exe");
	let output = Command::new(vswhere)
		.args([
			"-latest",
			"-products",
			"*",
			"-requires",
			"Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
			"-property",
			"installationPath",
		])
		.output()?;
	if !output.status.success() {
		return Err("Visual Studio C++ toolchain was not found".into());
	}
	let installation = PathBuf::from(String::from_utf8(output.stdout)?.trim());
	let tools_root = installation.join("VC").join("Tools").join("MSVC");
	let tools = latest_child_matching(&tools_root, |path| {
		path.join("bin").join("Hostx64").join("x64").join("cl.exe").is_file()
	})
	.ok_or_else(|| format!("MSVC x64 tools were not found under {}", tools_root.display()))?;
	let sdk_root = PathBuf::from(env::var_os("ProgramFiles(x86)").unwrap())
		.join("Windows Kits")
		.join("10");
	let sdk_include = sdk_root.join("Include");
	let sdk_version = latest_child_matching(&sdk_include, |path| path.join("ucrt").is_dir() && path.join("um").is_dir())
		.ok_or_else(|| format!("Windows SDK headers were not found under {}", sdk_include.display()))?;
	let version = sdk_version.file_name().ok_or("Windows SDK version is invalid")?;
	let sdk_lib = sdk_root.join("Lib").join(version);

	let include = env::join_paths([
		tools.join("include"),
		installation.join("VC").join("Auxiliary").join("VS").join("include"),
		sdk_version.join("ucrt"),
		sdk_version.join("um"),
		sdk_version.join("shared"),
		sdk_version.join("winrt"),
		sdk_version.join("cppwinrt"),
	])?;
	let lib = env::join_paths([
		tools.join("lib").join("x64"),
		sdk_lib.join("ucrt").join("x64"),
		sdk_lib.join("um").join("x64"),
	])?;
	let path = env::join_paths([
		tools.join("bin").join("Hostx64").join("x64"),
		sdk_root.join("bin").join(version).join("x64"),
	])?;
	command.env("INCLUDE", include).env("LIB", &lib).env("LIBPATH", lib);
	Ok(Some(path))
}

#[cfg(windows)]
fn latest_child_matching(root: &Path, predicate: impl Fn(&Path) -> bool) -> Option<PathBuf> {
	fs::read_dir(root)
		.ok()?
		.filter_map(std::result::Result::ok)
		.map(|entry| entry.path())
		.filter(|path| predicate(path))
		.max()
}

#[cfg(windows)]
fn find_ninja() -> Option<PathBuf> {
	env::var_os("PATH")
		.and_then(|paths| {
			env::split_paths(&paths)
				.map(|path| path.join("ninja.exe"))
				.find(|path| path.is_file())
		})
		.or_else(|| {
			let packages = env::var_os("LOCALAPPDATA")
				.map(PathBuf::from)?
				.join("Microsoft")
				.join("WinGet")
				.join("Packages");
			fs::read_dir(packages)
				.ok()?
				.filter_map(std::result::Result::ok)
				.map(|entry| entry.path())
				.find_map(|path| find_file_recursive(&path, "ninja.exe"))
		})
}

#[cfg(windows)]
fn find_file_recursive(dir: &Path, name: &str) -> Option<PathBuf> {
	for entry in fs::read_dir(dir).ok()?.filter_map(std::result::Result::ok) {
		let path = entry.path();
		if path.is_file() && path.file_name().is_some_and(|file_name| file_name == name) {
			return Some(path);
		}
		if path.is_dir() {
			if let Some(found) = find_file_recursive(&path, name) {
				return Some(found);
			}
		}
	}
	None
}

#[cfg(not(windows))]
fn configure_vulkan_environment(_command: &mut Command) -> Result<PathBuf> {
	Ok(PathBuf::new())
}

fn remove_if_exists(path: &Path) -> Result<()> {
	if path.is_file() {
		fs::remove_file(path)?;
		println!("removed stale release artifact: {}", path.display());
	}
	Ok(())
}

fn npm_command() -> Command {
	#[cfg(windows)]
	{
		Command::new("npm.cmd")
	}
	#[cfg(not(windows))]
	{
		Command::new("npm")
	}
}

fn docs(args: Vec<String>) -> Result<()> {
	match args.first().map(String::as_str) {
		Some("bless-node-catalog") => {
			let root = repo_root()?;
			let mut command = Command::new("cargo");
			command
				.current_dir(root)
				.env("BLESS_NODE_CATALOG", "1")
				.arg("test")
				.arg("--lib")
				.arg("node_catalog_md_up_to_date");
			run_command(&mut command)
		}
		Some("-h") | Some("--help") | None => {
			println!("docs subcommands:\n  bless-node-catalog    Regenerate docs/manual/node-catalog.md from the registry.");
			Ok(())
		}
		Some(other) => Err(format!("unknown docs subcommand: {other}").into()),
	}
}

fn ocr_onnx(args: Vec<String>) -> Result<()> {
	match args.first().map(String::as_str) {
		Some("eval") => {
			let mut command = Command::new("cargo");
			command.arg("run").arg("-p").arg("vac-ocr-onnx-eval").arg("--").args(&args[1..]);
			run_command(&mut command)
		}
		Some("prepare-paddle") => prepare_paddle_onnx_models(&args[1..]),
		Some("convert-paddle") => convert_paddle_onnx_models(&args[1..]),
		Some("-h") | Some("--help") | None => {
			print_ocr_onnx_help();
			Ok(())
		}
		Some(other) => Err(format!("unknown ocr-onnx subcommand: {other}").into()),
	}
}

#[derive(Debug, Clone, Copy)]
struct PaddleOcrModelArtifact {
	name: &'static str,
	url: &'static str,
}

#[derive(Debug, Clone, Copy)]
struct PaddleOcrTextArtifact {
	name: &'static str,
	url: &'static str,
}

const PPOCRV3_EN_ARTIFACTS: &[PaddleOcrModelArtifact] = &[
	PaddleOcrModelArtifact {
		name: "en_PP-OCRv3_det_infer",
		url: "https://paddleocr.bj.bcebos.com/PP-OCRv3/english/en_PP-OCRv3_det_infer.tar",
	},
	PaddleOcrModelArtifact {
		name: "en_PP-OCRv3_rec_infer",
		url: "https://paddleocr.bj.bcebos.com/PP-OCRv3/english/en_PP-OCRv3_rec_infer.tar",
	},
	PaddleOcrModelArtifact {
		name: "ch_ppocr_mobile_v2.0_cls_infer",
		url: "https://paddleocr.bj.bcebos.com/dygraph_v2.0/ch/ch_ppocr_mobile_v2.0_cls_infer.tar",
	},
];

const PPOCRV5_SERVER_ARTIFACTS: &[PaddleOcrModelArtifact] = &[
	PaddleOcrModelArtifact {
		name: "PP-OCRv5_server_det_infer",
		url: "https://paddle-model-ecology.bj.bcebos.com/paddlex/official_inference_model/paddle3.0.0/PP-OCRv5_server_det_infer.tar",
	},
	PaddleOcrModelArtifact {
		name: "PP-OCRv5_server_rec_infer",
		url: "https://paddle-model-ecology.bj.bcebos.com/paddlex/official_inference_model/paddle3.0.0/PP-OCRv5_server_rec_infer.tar",
	},
	PaddleOcrModelArtifact {
		name: "ch_ppocr_mobile_v2.0_cls_infer",
		url: "https://paddleocr.bj.bcebos.com/dygraph_v2.0/ch/ch_ppocr_mobile_v2.0_cls_infer.tar",
	},
];

const PPOCRV3_EN_TEXT_ARTIFACTS: &[PaddleOcrTextArtifact] = &[PaddleOcrTextArtifact {
	name: "en_dict.txt",
	url: "https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/ppocr/utils/en_dict.txt",
}];

const PPOCRV5_TEXT_ARTIFACTS: &[PaddleOcrTextArtifact] = &[PaddleOcrTextArtifact {
	name: "ppocrv5_dict.txt",
	url: "https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/ppocr/utils/dict/ppocrv5_dict.txt",
}];

fn prepare_paddle_onnx_models(args: &[String]) -> Result<()> {
	let preset = option_value(args, "--preset").unwrap_or("ppocrv3-en");
	let (artifacts, text_artifacts) = match preset {
		"ppocrv3-en" => (PPOCRV3_EN_ARTIFACTS, PPOCRV3_EN_TEXT_ARTIFACTS),
		"ppocrv5-server" => (PPOCRV5_SERVER_ARTIFACTS, PPOCRV5_TEXT_ARTIFACTS),
		other => return Err(format!("unknown OCR ONNX paddle preset: {other}").into()),
	};

	let root = repo_root()?;
	let vendor = root.join(ONNX_VENDOR_ROOT);
	let downloads = vendor.join("downloads");
	let dicts = vendor.join("dicts").join(preset);
	let paddle_dir = vendor.join("paddle").join(preset);
	let onnx_dir = vendor.join("onnx").join(preset);
	fs::create_dir_all(&downloads)?;
	fs::create_dir_all(&dicts)?;
	fs::create_dir_all(&paddle_dir)?;
	fs::create_dir_all(&onnx_dir)?;

	let mut sums = String::new();
	for artifact in artifacts {
		let tar_path = downloads.join(format!("{}.tar", artifact.name));
		if !tar_path.exists() {
			download_file(artifact.url, &tar_path)?;
		} else {
			println!("already downloaded {}", tar_path.display());
		}
		let sha256 = sha256_file(&tar_path)?;
		sums.push_str(&format!("{}  downloads/{}.tar\n", sha256, artifact.name));
		extract_tar(&tar_path, &paddle_dir)?;
	}

	for artifact in text_artifacts {
		let dict_path = dicts.join(artifact.name);
		if !dict_path.exists() {
			download_file(artifact.url, &dict_path)?;
		} else {
			println!("already downloaded {}", dict_path.display());
		}
		let sha256 = sha256_file(&dict_path)?;
		sums.push_str(&format!("{}  dicts/{}/{}\n", sha256, preset, artifact.name));
	}

	fs::write(vendor.join("SHA256SUMS.txt"), sums)?;
	write_onnx_local_readme(&vendor, preset)?;
	println!("prepared PaddleOCR ONNX source models: {}", paddle_dir.display());
	println!("prepared PaddleOCR recognition dictionaries: {}", dicts.display());
	println!("next: convert Paddle inference models with paddle2onnx into {}", onnx_dir.display());
	Ok(())
}

fn convert_paddle_onnx_models(args: &[String]) -> Result<()> {
	let preset = option_value(args, "--preset").unwrap_or("ppocrv3-en");
	let skip_cls = args.iter().any(|a| a == "--skip-cls");
	let artifacts = match preset {
		"ppocrv3-en" => PPOCRV3_EN_ARTIFACTS,
		"ppocrv5-server" => PPOCRV5_SERVER_ARTIFACTS,
		other => return Err(format!("unknown OCR ONNX paddle preset: {other}").into()),
	};

	let root = repo_root()?;
	let vendor = root.join(ONNX_VENDOR_ROOT);
	let paddle_dir = vendor.join("paddle").join(preset);
	let onnx_dir = vendor.join("onnx").join(preset);
	let venv = vendor.join(".venv");
	fs::create_dir_all(&onnx_dir)?;
	ensure_python_venv(&venv)?;
	run_command(
		venv_python(&venv)
			.arg("-m")
			.arg("pip")
			.arg("install")
			.arg("--upgrade")
			.arg("pip")
			.arg("packaging")
			.arg(PADDLEPADDLE_PADDLE2ONNX_WIN_CP311)
			.arg("paddle2onnx"),
	)?;

	for artifact in artifacts {
		if artifact.name.contains("_cls_") && skip_cls {
			println!("skip classifier {}", artifact.name);
			continue;
		}
		let model_dir = paddle_dir.join(artifact.name);
		let model_filename = if model_dir.join("inference.pdmodel").exists() {
			"inference.pdmodel"
		} else if model_dir.join("inference.json").exists() {
			"inference.json"
		} else {
			return Err(format!("missing Paddle inference model file under {}", model_dir.display()).into());
		};
		if !model_dir.join("inference.pdiparams").exists() {
			return Err(format!("missing Paddle inference model files under {}", model_dir.display()).into());
		}
		let save_file = onnx_dir.join(format!("{}.onnx", onnx_name_for_artifact(artifact.name)));
		run_command(
			venv_script(&venv, "paddle2onnx")
				.arg("--model_dir")
				.arg(&model_dir)
				.arg("--model_filename")
				.arg(model_filename)
				.arg("--params_filename")
				.arg("inference.pdiparams")
				.arg("--save_file")
				.arg(&save_file)
				.arg("--opset_version")
				.arg("11")
				.arg("--enable_onnx_checker")
				.arg("True"),
		)?;
		if artifact.name.contains("_cls_") {
			repair_paddleocr_cls_onnx(&venv, &save_file)?;
		}
		println!("converted {}", save_file.display());
	}
	Ok(())
}

fn repair_paddleocr_cls_onnx(venv: &Path, model: &Path) -> Result<()> {
	let script = r#"
import sys
import onnx
from onnx import helper, shape_inference

path = sys.argv[1]
model = onnx.load(path)
inferred = shape_inference.infer_shapes(model)
rank = {}
for value_info in list(inferred.graph.value_info) + list(inferred.graph.input) + list(inferred.graph.output):
    tensor_type = value_info.type.tensor_type
    if tensor_type.HasField("shape"):
        rank[value_info.name] = len(tensor_type.shape.dim)

nodes = []
patched = 0
for node in model.graph.node:
    if node.op_type == "Concat":
        inputs = []
        changed = False
        for input_name in node.input:
            if rank.get(input_name) == 0:
                output_name = input_name + ".unvac_unsqueeze0"
                nodes.append(helper.make_node("Unsqueeze", [input_name], [output_name], name=input_name + ".UnvacUnsqueeze0", axes=[0]))
                inputs.append(output_name)
                changed = True
            else:
                inputs.append(input_name)
        if changed:
            node.input[:] = inputs
            patched += 1
    nodes.append(node)

if patched:
    del model.graph.node[:]
    model.graph.node.extend(nodes)
    onnx.checker.check_model(model)
    onnx.save(model, path)
print(f"patched_scalar_concat={patched}")
"#;
	run_command(venv_python(venv).arg("-c").arg(script).arg(model))
}

fn onnx_name_for_artifact(name: &str) -> &'static str {
	if name.contains("_det_") {
		"det"
	} else if name.contains("_rec_") {
		"rec"
	} else if name.contains("_cls_") {
		"cls"
	} else {
		"model"
	}
}

fn paddleocr_native(args: Vec<String>) -> Result<()> {
	let Some(subcommand) = args.first().map(String::as_str) else {
		print_paddleocr_help();
		return Ok(());
	};

	match subcommand {
		"plan" => {
			print_paddleocr_plan();
			Ok(())
		}
		"prepare" => prepare_paddleocr(&args[1..]),
		"configure" => configure_paddleocr(&args[1..]),
		"build" => build_paddleocr(&args[1..]),
		"eval" => eval_paddleocr(&args[1..]),
		"license-manifest" => write_license_manifest(),
		"-h" | "--help" => {
			print_paddleocr_help();
			Ok(())
		}
		other => Err(format!("unknown paddleocr-native subcommand: {other}").into()),
	}
}

fn prepare_paddleocr(args: &[String]) -> Result<()> {
	let reference = option_value(args, "--ref").unwrap_or(DEFAULT_PADDLEOCR_REF);
	let root = repo_root()?;
	let vendor = root.join(VENDOR_ROOT);
	let source = vendor.join("src/PaddleOCR");

	create_layout(&vendor)?;
	if source.join(".git").exists() {
		run_command(
			Command::new("git")
				.arg("-C")
				.arg(&source)
				.arg("fetch")
				.arg("--depth")
				.arg("1")
				.arg("origin")
				.arg(reference),
		)?;
		run_command(Command::new("git").arg("-C").arg(&source).arg("checkout").arg("FETCH_HEAD"))?;
	} else {
		if let Some(parent) = source.parent() {
			fs::create_dir_all(parent)?;
		}
		run_command(
			Command::new("git")
				.arg("clone")
				.arg("--depth")
				.arg("1")
				.arg("--branch")
				.arg(reference)
				.arg(PADDLEOCR_REPO)
				.arg(&source),
		)?;
	}

	write_local_readme(&vendor, reference)?;
	copy_if_exists(source.join("LICENSE"), vendor.join("licenses/PaddleOCR-LICENSE"))?;
	write_license_manifest()?;
	println!("prepared PaddleOCR native workspace: {}", vendor.display());
	Ok(())
}

fn configure_paddleocr(args: &[String]) -> Result<()> {
	let root = repo_root()?;
	let vendor = root.join(VENDOR_ROOT);
	create_layout(&vendor)?;

	let build_dir = vendor.join("build");
	fs::create_dir_all(&build_dir)?;

	let mut command = Command::new("cmake");
	command
		.arg("-S")
		.arg(root.join(NATIVE_WRAPPER_DIR))
		.arg("-B")
		.arg(&build_dir)
		.arg("-G")
		.arg("Visual Studio 17 2022")
		.arg("-A")
		.arg("x64")
		.arg(format!("-DCMAKE_INSTALL_PREFIX={}", vendor.join("install").display()))
		.arg(format!("-DUNVAC_PADDLEOCR_VENDOR_ROOT={}", vendor.display()));

	if args.iter().any(|a| a == "--with-upstream") {
		let opencv_dir = option_value(args, "--opencv-dir")
			.map(PathBuf::from)
			.or_else(|| env::var_os("PADDLEOCR_OPENCV_DIR").map(PathBuf::from))
			.ok_or("missing --opencv-dir or PADDLEOCR_OPENCV_DIR")?;
		let paddle_inference_dir = option_value(args, "--paddle-inference-dir")
			.map(PathBuf::from)
			.or_else(|| env::var_os("PADDLEOCR_INFERENCE_DIR").map(PathBuf::from))
			.ok_or("missing --paddle-inference-dir or PADDLEOCR_INFERENCE_DIR")?;
		command
			.arg("-DUNVAC_PADDLEOCR_WITH_UPSTREAM=ON")
			.arg(format!("-DUNVAC_PADDLEOCR_OPENCV_DIR={}", opencv_dir.display()))
			.arg(format!("-DUNVAC_PADDLEOCR_INFERENCE_DIR={}", paddle_inference_dir.display()));
	}

	run_command(&mut command)?;
	Ok(())
}

fn build_paddleocr(args: &[String]) -> Result<()> {
	let config = option_value(args, "--config").unwrap_or("Release");
	let root = repo_root()?;
	let build_dir = root.join(VENDOR_ROOT).join("build");
	if !build_dir.exists() {
		return Err(format!(
			"build dir does not exist: {}. Run `cargo xtask paddleocr-native configure` first.",
			build_dir.display()
		)
		.into());
	}
	run_command(Command::new("cmake").arg("--build").arg(build_dir).arg("--config").arg(config))?;
	let build_dir = root.join(VENDOR_ROOT).join("build");
	run_command(Command::new("cmake").arg("--install").arg(build_dir).arg("--config").arg(config))?;
	Ok(())
}

fn eval_paddleocr(args: &[String]) -> Result<()> {
	let root = repo_root()?;
	let vendor = root.join(VENDOR_ROOT);
	let lib_dir = vendor.join("install/lib");
	let bin_dir = vendor.join("install/bin");
	if !lib_dir.join("unvac_paddleocr.lib").exists() {
		return Err(format!(
			"missing import lib: {}. Run `paddleocr-native build` first.",
			lib_dir.join("unvac_paddleocr.lib").display()
		)
		.into());
	}
	if !bin_dir.join("unvac_paddleocr.dll").exists() {
		return Err(format!(
			"missing dll: {}. Run `paddleocr-native build` first.",
			bin_dir.join("unvac_paddleocr.dll").display()
		)
		.into());
	}

	let old_path = env::var_os("PATH").unwrap_or_default();
	let mut paths = vec![bin_dir.clone()];
	paths.extend(env::split_paths(&old_path));
	let path = env::join_paths(paths)?;

	let mut command = Command::new("cargo");
	command
		.arg("run")
		.arg("-p")
		.arg("vac-ocr-paddle-eval")
		.arg("--features")
		.arg("native-link")
		.arg("--")
		.args(args)
		.env("UNVAC_PADDLEOCR_LIB_DIR", &lib_dir)
		.env("PATH", path);
	run_command(&mut command)
}

fn write_license_manifest() -> Result<()> {
	let root = repo_root()?;
	let vendor = root.join(VENDOR_ROOT);
	create_layout(&vendor)?;
	let manifest = vendor.join("licenses/THIRD_PARTY_MANIFEST.md");
	fs::write(
		&manifest,
		format!(
			"# UNVAC PaddleOCR Native Third-Party Manifest\n\n\
This directory is generated by `cargo xtask paddleocr-native ...` and is not tracked by git.\n\n\
## Intended redistributable artifacts\n\n\
- PaddleOCR native wrapper: `unvac_paddleocr.dll` / import library\n\
- PaddleOCR C++ inference runtime artifacts selected for the release build\n\
- OpenCV runtime artifacts selected for the release build\n\
- OCR model files selected for the release build\n\n\
## Required notices before redistribution\n\n\
- Copy PaddleOCR `LICENSE` into this directory.\n\
- Copy Paddle Inference license notices into this directory.\n\
- Copy OpenCV license notices into this directory.\n\
- Record every model URL, checksum, and license in the release manifest.\n\n\
## Upstream references\n\n\
- PaddleOCR: {PADDLEOCR_REPO}\n\
- Local source cache: `{}`\n",
			vendor.join("src/PaddleOCR").display()
		),
	)?;
	println!("wrote {}", manifest.display());
	Ok(())
}

fn create_layout(vendor: &Path) -> Result<()> {
	for rel in [
		"src",
		"build",
		"install/bin",
		"install/lib",
		"install/include",
		"models",
		"licenses",
	] {
		fs::create_dir_all(vendor.join(rel))?;
	}
	Ok(())
}

fn write_local_readme(vendor: &Path, reference: &str) -> Result<()> {
	fs::write(
		vendor.join("README.local.md"),
		format!(
			"# PaddleOCR Native Workspace\n\n\
This directory is generated and intentionally ignored by git.\n\n\
- upstream: `{PADDLEOCR_REPO}`\n\
- ref: `{reference}`\n\
- wrapper source: `{NATIVE_WRAPPER_DIR}`\n\
- install output: `install/`\n\
- redistribution notices: `licenses/`\n"
		),
	)?;
	Ok(())
}

fn option_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
	args.windows(2).find(|pair| pair[0] == name).map(|pair| pair[1].as_str())
}

fn repo_root() -> Result<PathBuf> {
	let out = Command::new("git")
		.arg("rev-parse")
		.arg("--show-toplevel")
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.output()?;
	if !out.status.success() {
		return Err("git rev-parse --show-toplevel failed".into());
	}
	Ok(PathBuf::from(String::from_utf8(out.stdout)?.trim()))
}

fn copy_if_exists(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<()> {
	let from = from.as_ref();
	if from.exists() {
		if let Some(parent) = to.as_ref().parent() {
			fs::create_dir_all(parent)?;
		}
		fs::copy(from, to)?;
	}
	Ok(())
}

fn download_file(url: &str, dest: &Path) -> Result<()> {
	println!("download {url}");
	if let Some(parent) = dest.parent() {
		fs::create_dir_all(parent)?;
	}
	let response = ureq::get(url)
		.header("User-Agent", "virtual-avatar-connect/xtask PaddleOCR model preparation")
		.call()
		.map_err(|e| format!("GET {url}: {e}"))?;
	if response.status() != 200 {
		return Err(format!("GET {url} returned HTTP {}", response.status()).into());
	}
	let mut reader = response.into_parts().1.into_reader();
	let mut file = fs::File::create(dest)?;
	std::io::copy(&mut reader, &mut file)?;
	Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
	let mut file = fs::File::open(path)?;
	let mut hasher = Sha256::new();
	let mut buf = [0_u8; 1024 * 64];
	loop {
		let n = file.read(&mut buf)?;
		if n == 0 {
			break;
		}
		hasher.update(&buf[..n]);
	}
	let digest = hasher.finalize();
	Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

fn extract_tar(tar_path: &Path, dest: &Path) -> Result<()> {
	fs::create_dir_all(dest)?;
	run_command(Command::new("tar").arg("-xf").arg(tar_path).arg("-C").arg(dest))
}

fn write_onnx_local_readme(vendor: &Path, preset: &str) -> Result<()> {
	let mut f = fs::File::create(vendor.join("README.local.md"))?;
	writeln!(
		f,
		"# PaddleOCR ONNX Workspace\n\n\
This directory is generated and intentionally ignored by git.\n\n\
- preset: `{preset}`\n\
- Paddle inference models: `paddle/{preset}/`\n\
- ONNX output target: `onnx/{preset}/`\n\
- Recognition dictionaries: `dicts/{preset}/`\n\
- checksums: `SHA256SUMS.txt`\n\n\
Convert with `paddle2onnx` after installing it in a Python environment. See `docs/roadmap/ocr-paddle-onnx-models.md`.\n"
	)?;
	Ok(())
}

fn ensure_python_venv(venv: &Path) -> Result<()> {
	let python = venv_python_path(venv);
	if python.exists() {
		return Ok(());
	}
	if let Some(parent) = venv.parent() {
		fs::create_dir_all(parent)?;
	}
	run_command(Command::new("python").arg("-m").arg("venv").arg(venv))
}

fn venv_python(venv: &Path) -> Command {
	Command::new(venv_python_path(venv))
}

fn venv_python_path(venv: &Path) -> PathBuf {
	#[cfg(windows)]
	{
		venv.join("Scripts/python.exe")
	}
	#[cfg(not(windows))]
	{
		venv.join("bin/python")
	}
}

fn venv_script(venv: &Path, name: &str) -> Command {
	#[cfg(windows)]
	{
		Command::new(venv.join(format!("Scripts/{name}.exe")))
	}
	#[cfg(not(windows))]
	{
		Command::new(venv.join(format!("bin/{name}")))
	}
}

fn run_command(command: &mut Command) -> Result<()> {
	print_command(command);
	let status = command.status()?;
	if !status.success() {
		return Err(format!("command failed with status {status}").into());
	}
	Ok(())
}

fn print_command(command: &Command) {
	let program = command.get_program().to_string_lossy();
	let args = command.get_args().map(OsStr::to_string_lossy).collect::<Vec<_>>().join(" ");
	println!("> {program} {args}");
}

fn print_help() {
	println!(
		"xtask commands:\n\
release-files-check <artifact directory>\n\
release-gui-check <GUI output directory>\n\
release-worker-build <cpu|cpu-avx2|vulkan> <output directory>\n\
release-build [--variant portable|minimal] [--skip-npm-ci] [--reuse-gui-dist] [--target-dir <path>]\n\
  stt-eval devices\n\
  stt-eval output-devices\n\
  stt-eval record --output <wav> [--seconds <n>] [--device <name>|--device-index <n>]\n\
  stt-eval play --input <wav> [--device <name>|--device-index <n>]\n\
  stt-eval inspect --input <wav>\n\
  stt-eval vosk --input <wav> --model <path-or-id> [--reference <text>] [--json-out <path>]\n\
  stt-eval whisper --input <wav> --model <path-or-preset> [--reference <text>] [--json-out <path>]\n\
  stt-eval worker --input <wav> --worker <exe> --model <path-or-preset> [--reference <text>] [--json-out <path>]\n\
  stt-eval web-speech [--browser all|chrome|edge|firefox] [--audio <wav> --start-ms <n> --end-ms <n> --reference <text>] [--json-out <path>]\n\
  ai-turn-eval [--fixture <path>] [--base-url <url>] [--persona-id <id>] [--token <token>]\n\
               [--timeout-secs <n>] [--output <path>] [--through <case-id>] [--dry-run] [--no-fail]\n\
  avatar-control <subcommand> [options]\n\
  stream-check [--base-url <url>] [--token <token>] [--channels <csv>] [--expect-live-from <preflight.json>] [--expect-live-minutes <n>] [--expect-tts-nodes <csv>] [--expect-reward-id <id>] [--expect-reward-title <title>] [--expect-reward-channels <csv>] [--allow-non-streaming] [--json] [--evidence-out <path>] [--force]\n\
  flowgraph-check --root <directory> [--cli <executable>] [--require-streaming-contract]\n\
  model english2kana prepare|convert|verify\n\
  docs bless-node-catalog\n\
  paddleocr-native plan\n\
  paddleocr-native prepare [--ref <git-ref>]\n\
  paddleocr-native configure [--with-upstream --opencv-dir <path> --paddle-inference-dir <path>]\n\
  paddleocr-native build [--config Release]\n\
  paddleocr-native eval [--model-dir <path> --loops <n>]\n\
  paddleocr-native license-manifest\n\
  ocr-onnx prepare-paddle [--preset ppocrv3-en|ppocrv5-server]\n\
  ocr-onnx convert-paddle [--preset ppocrv3-en|ppocrv5-server]\n\
  ocr-onnx eval --model <path>"
	);
}

#[cfg(test)]
mod release_build_tests {
	use super::*;

	fn args(values: &[&str]) -> Vec<String> {
		values.iter().map(|value| (*value).to_string()).collect()
	}

	#[test]
	fn release_gui_identity_rejects_preview_missing_and_changed_files() {
		let root=env::temp_dir().join(format!("vac-ui-identity-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
		fs::create_dir(&root).unwrap();
		fs::write(root.join("index.html"),"<html></html>").unwrap();
		fs::write(root.join("app.js"),"normal-ui").unwrap();
		assert!(validate_release_gui(&root).is_err());
		let mut identity=serde_json::json!({"schema_version":1,"design_preview":true,"files":{
			"index.html":sha256_file(&root.join("index.html")).unwrap(),"app.js":sha256_file(&root.join("app.js")).unwrap()
		}});
		let write_identity=|value:&serde_json::Value|fs::write(root.join("unvac-ui-build.json"),serde_json::to_vec(value).unwrap()).unwrap();
		write_identity(&identity);
		assert!(validate_release_gui(&root).is_err());
		identity["design_preview"]=false.into(); write_identity(&identity);
		validate_release_gui(&root).unwrap();
		fs::create_dir(root.join("assets")).unwrap();
		for name in ["stale.js", "stale.css", "stale.html"] {
			let stale = root.join("assets").join(name);
			fs::write(&stale, "untracked preview content").unwrap();
			assert!(validate_release_gui(&root).unwrap_err().to_string().contains("absent from build identity"));
			fs::remove_file(stale).unwrap();
		}
		validate_release_gui(&root).unwrap();
		fs::write(root.join("app.js"),"changed-ui").unwrap();
		assert!(validate_release_gui(&root).is_err());
		fs::remove_file(root.join("app.js")).unwrap();
		assert!(validate_release_gui(&root).is_err());
		fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn release_vosk_runtime_rejects_missing_empty_and_directory_entries() {
		let root = env::temp_dir().join(format!("vac-vosk-release-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
		fs::create_dir(&root).unwrap();
		let names = ["libvosk.dll", "libgcc_s_seh-1.dll", "libstdc++-6.dll", "libwinpthread-1.dll"];
		for name in names { fs::write(root.join(name), b"fixture DLL").unwrap(); }
		validate_windows_vosk_runtime(&root).unwrap();
		for name in names {
			let path = root.join(name);
			fs::remove_file(&path).unwrap();
			assert!(validate_windows_vosk_runtime(&root).unwrap_err().to_string().contains(name));
			fs::write(&path, b"").unwrap();
			assert!(validate_windows_vosk_runtime(&root).is_err());
			fs::remove_file(&path).unwrap();
			fs::create_dir(&path).unwrap();
			assert!(validate_windows_vosk_runtime(&root).is_err());
			fs::remove_dir(&path).unwrap();
			fs::write(&path, b"fixture DLL").unwrap();
		}
		validate_windows_vosk_runtime(&root).unwrap();
		fs::remove_dir_all(root).unwrap();
	}

	#[cfg(windows)]
	#[test]
	fn vosk_notice_rejects_changed_notice_or_dll() {
		let root = env::temp_dir().join(format!("vac-notice-{}", std::process::id()));
		fs::create_dir_all(root.join("licenses")).unwrap();
		let notice = root.join("licenses/Vosk-0.3.45.txt");
		fs::write(&notice, b"notice fixture").unwrap();
		let mut hashes = serde_json::Map::new();
		let mut runtime_notices = serde_json::Map::new();
		for name in VOSK_RUNTIME_NOTICES {
			let path = root.join("licenses").join(name);
			fs::write(&path, b"runtime notice fixture").unwrap();
			runtime_notices.insert((*name).into(), serde_json::json!({"sha256": sha256_file(&path).unwrap()}));
		}
		for name in ["libvosk.dll", "libgcc_s_seh-1.dll", "libstdc++-6.dll", "libwinpthread-1.dll"] {
			fs::write(root.join(name), b"dll fixture").unwrap();
			hashes.insert(name.into(), serde_json::Value::String(sha256_file(&root.join(name)).unwrap()));
		}
		fs::write(root.join("licenses/Vosk-0.3.45.source.json"), serde_json::to_vec(&serde_json::json!({
			"license_sha256": sha256_file(&notice).unwrap(), "dll_sha256":hashes, "runtime_notices":runtime_notices
		})).unwrap()).unwrap();
		validate_vosk_notice(&root, &root).unwrap();
		let runtime_notice = root.join("licenses").join(VOSK_RUNTIME_NOTICES[0]);
		fs::remove_file(&runtime_notice).unwrap();
		assert!(validate_vosk_notice(&root, &root).is_err());
		fs::write(&runtime_notice, b"wrong notice").unwrap();
		assert!(validate_vosk_notice(&root, &root).is_err());
		fs::write(&runtime_notice, b"runtime notice fixture").unwrap();
		fs::write(&notice, b"changed").unwrap();
		assert!(validate_vosk_notice(&root, &root).is_err());
		fs::write(&notice, b"notice fixture").unwrap();
		fs::write(root.join("libvosk.dll"), b"different build").unwrap();
		assert!(validate_vosk_notice(&root, &root).is_err());
		fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn release_build_defaults_to_portable() {
		let options = parse_release_build_options(&[]).unwrap();
		assert_eq!(options.variant, ReleaseVariant::Portable);
		assert_eq!(options.variant.features(), "voice-vosk,embed-gui,voice-whisper");
		assert!(options.variant.release_compatibility().contains("CPU fallback"));
	}

	#[test]
	fn release_guide_is_copied_exactly_and_required_in_inventory() {
		let root = repo_root().unwrap();
		let out = env::temp_dir().join(format!("vac-release-guide-{}", std::process::id()));
		fs::create_dir_all(&out).unwrap();
		stage_release_guide(&root, &out).unwrap();
		for &(source, name) in RELEASE_DOCUMENTS {
			assert_eq!(fs::read(root.join(source)).unwrap(), fs::read(out.join(name)).unwrap());
			for variant in [ReleaseVariant::Portable, ReleaseVariant::Minimal] {
				assert!(release_file_names(variant).contains(&name.to_owned()));
			}
		}
		let names: Vec<String> = RELEASE_DOCUMENTS.iter().map(|(_, name)| (*name).to_owned()).collect();
		release_inventory::write(&out, "portable", &names).unwrap();
		assert_eq!(release_inventory::verify(&out, |_| Ok(names.clone())).unwrap(), RELEASE_DOCUMENTS.len());
		fs::remove_file(out.join("START-HERE.txt")).unwrap();
		assert!(release_inventory::verify(&out, |_| Ok(names.clone())).is_err());
		fs::remove_dir_all(out).unwrap();
	}

	#[test]
	fn frontend_notices_reject_changed_build_or_text() {
		let root = env::temp_dir().join(format!("vac-frontend-notices-{}", std::process::id()));
		fs::create_dir_all(root.join("gui/dist")).unwrap();
		fs::create_dir_all(root.join("gui/src")).unwrap();
		fs::create_dir_all(root.join("notices")).unwrap();
		let gui = root.join("gui");
		let notices = root.join("notices");
		let mut record = serde_json::json!({"packages":[{"name":"fixture"}]});
		for (field, file) in [
			("ui_identity_sha256", gui.join("dist/unvac-ui-build.json")),
			("package_lock_sha256", gui.join("package-lock.json")),
			("app_css_sha256", gui.join("src/app.css")),
			("notice_sha256", notices.join("THIRD-PARTY-FRONTEND.txt")),
		] {
			fs::write(&file, b"fixture").unwrap();
			record[field] = sha256_file(&file).unwrap().into();
		}
		let manifest = notices.join("frontend-notices.json");
		fs::write(&manifest, serde_json::to_vec(&record).unwrap()).unwrap();
		validate_frontend_notices(&gui, &notices).unwrap();
		for file in [gui.join("dist/unvac-ui-build.json"), gui.join("package-lock.json"), gui.join("src/app.css"), notices.join("THIRD-PARTY-FRONTEND.txt")] {
			fs::write(&file, b"changed").unwrap();
			assert!(validate_frontend_notices(&gui, &notices).is_err());
			fs::write(file, b"fixture").unwrap();
		}
		record["packages"] = serde_json::json!([]);
		fs::write(&manifest, serde_json::to_vec(&record).unwrap()).unwrap();
		assert!(validate_frontend_notices(&gui, &notices).is_err());
		fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn release_build_accepts_named_variants_and_legacy_aliases() {
		assert_eq!(
			parse_release_build_options(&args(&["--variant", "minimal"])).unwrap().variant,
			ReleaseVariant::Minimal
		);
	}

	#[test]
	fn release_build_can_explicitly_reuse_validated_gui_dist() {
		let options = parse_release_build_options(&args(&["--skip-npm-ci", "--reuse-gui-dist"])).unwrap();
		assert!(options.skip_npm_ci);
		assert!(options.reuse_gui_dist);
	}

	#[test]
	fn release_build_rejects_conflicting_variants() {
		let error = parse_release_build_options(&args(&["--variant", "minimal", "--voice-whisper"])).unwrap_err();
		assert!(error.to_string().contains("mutually exclusive"));
	}

	#[test]
	fn portable_variant_has_cross_vendor_gpu_and_cpu_fallback() {
		let compatibility = ReleaseVariant::Portable.release_compatibility();
		assert!(compatibility.contains("Vulkan"));
		assert!(compatibility.contains("CPU fallback"));
	}

	#[test]
	fn flowgraph_check_prefers_release_cli_without_recursive_cargo() {
		let root = env::temp_dir().join(format!("unvac-xtask-flowgraph-cli-{}", std::process::id()));
		let executable = format!("virtual-avatar-connect-cli{}", env::consts::EXE_SUFFIX);
		let release = root.join("target/release").join(&executable);
		let debug = root.join("target/debug").join(&executable);
		fs::create_dir_all(release.parent().unwrap()).unwrap();
		fs::create_dir_all(debug.parent().unwrap()).unwrap();
		fs::write(&release, b"release").unwrap();
		fs::write(&debug, b"debug").unwrap();
		assert_eq!(resolve_flowgraph_cli(&root, None).unwrap(), release);
		fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn flowgraph_check_accepts_explicit_relative_cli() {
		let root = env::temp_dir().join(format!("unvac-xtask-flowgraph-explicit-{}", std::process::id()));
		let explicit = PathBuf::from("bin/custom-cli.exe");
		let resolved = root.join(&explicit);
		fs::create_dir_all(resolved.parent().unwrap()).unwrap();
		fs::write(&resolved, b"custom").unwrap();
		assert_eq!(resolve_flowgraph_cli(&root, Some(explicit)).unwrap(), resolved);
		fs::remove_dir_all(root).unwrap();
	}

	#[cfg(windows)]
	#[test]
	fn stt_worker_builds_use_distinct_absolute_short_targets() {
		let root = Path::new(r"C:\workspace\unvac");
		let cpu = short_stt_worker_target_dir(root, SttWorkerBackend::Cpu);
		let vulkan = short_stt_worker_target_dir(root, SttWorkerBackend::Vulkan);
		let avx2 = short_stt_worker_target_dir(root, SttWorkerBackend::CpuAvx2);
		assert_ne!(avx2, cpu);
		assert_ne!(avx2, vulkan);
		assert!(cpu.is_absolute());
		assert!(vulkan.is_absolute());
		assert_ne!(cpu, vulkan);
		assert!(cpu.to_string_lossy().contains(r"\u\"));
		assert!(cpu.as_os_str().len() < 26);
		assert!(vulkan.as_os_str().len() < 28);
	}

	#[test]
	fn distributable_worker_isa_overrides_host_tuning() {
		for backend in [SttWorkerBackend::Cpu, SttWorkerBackend::CpuAvx2, SttWorkerBackend::Vulkan] {
			let mut command = Command::new("cargo");
			command.env("GGML_NATIVE", "ON").env("GGML_AVX512", "ON");
			configure_worker_cpu_isa(&mut command, backend);
			let flags: std::collections::HashMap<_, _> = command.get_envs().map(|(k, v)| (k.to_string_lossy().into_owned(), v.unwrap().to_string_lossy().into_owned())).collect();
			assert_eq!(flags["GGML_NATIVE"], "OFF");
			assert_eq!(flags["GGML_AVX512"], "OFF");
			assert_eq!(flags["GGML_BMI2"], "OFF");
			for flag in ["GGML_AVX2", "GGML_FMA", "GGML_F16C"] {
				assert_eq!(flags[flag], if backend == SttWorkerBackend::CpuAvx2 { "ON" } else { "OFF" });
			}
		}
	}

	#[test]
	fn worker_project_rejects_wrong_or_host_native_isa() {
		let ninja = "build ggml-cpu.cpp.obj: CXX_COMPILER\n  FLAGS = /O2 /Ob2 /DNDEBUG\n  DEFINES = -DGGML_USE_CPU_REPACK\n";
		assert!(worker_project_isa_matches(ninja, SttWorkerBackend::Vulkan));
		assert!(!worker_project_isa_matches(&format!("{ninja}  FLAGS = /arch:AVX2\n"), SttWorkerBackend::Vulkan));
		let avx2 = "<EnableEnhancedInstructionSet>AdvancedVectorExtensions2</EnableEnhancedInstructionSet>GGML_AVX2;GGML_FMA;GGML_F16C";
		assert!(worker_project_isa_matches("<Optimization>MaxSpeed</Optimization>", SttWorkerBackend::Cpu));
		assert!(!worker_project_isa_matches(avx2, SttWorkerBackend::Cpu));
		assert!(worker_project_isa_matches(avx2, SttWorkerBackend::CpuAvx2));
		assert!(!worker_project_isa_matches("", SttWorkerBackend::CpuAvx2));
		for backend in [SttWorkerBackend::Cpu, SttWorkerBackend::CpuAvx2] {
			assert!(!worker_project_isa_matches(&format!("{avx2};GGML_AVX512"), backend));
		}
	}

	#[test]
	fn portable_release_rejects_inherited_compiler_overrides_without_logging_values() {
		for key in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS",
			"CMAKE_CXX_FLAGS", "GGML_NATIVE", "GGML_AVX512", "CL", "_CL_", "CARGO_BUILD_TARGET", "CARGO_TARGET_DIR"] {
			let error = validate_release_environment([(key.into(), "private-override-value".into())]).unwrap_err().to_string();
			assert!(error.contains(key));
			assert!(!error.contains("private-override-value"));
		}
		validate_release_environment([("VULKAN_SDK".into(), "C:/sdk".into()), ("RUSTFLAGS".into(), "".into())]).unwrap();
	}
}

fn print_ocr_onnx_help() {
	println!(
		"ocr-onnx subcommands:\n\
  prepare-paddle [--preset ppocrv3-en|ppocrv5-server]\n\
                       Download and extract official PaddleOCR inference models for ONNX conversion.\n\
  convert-paddle [--preset ppocrv3-en|ppocrv5-server] [--skip-cls]\n\
                       Create a local venv, install latest paddle2onnx, and convert prepared det/rec/cls models.\n\
  eval --model <path>    Load an ONNX model with ort and print input/output metadata.
                       Pass --image / --decode-det / --decode-rec / --decode-cls / --pipeline through to the eval binary."
	);
}

fn print_paddleocr_help() {
	println!(
		"paddleocr-native subcommands:\n\
  plan                Show the experimental native OCR packaging plan.\n\
  prepare             Clone/update PaddleOCR into .vendors and create license layout.\n\
  configure           Configure the UNVAC C ABI wrapper with CMake. Stub build is the default.\n\
  build               Build the configured wrapper.\n\
  eval                Run the development-only Rust FFI probe against the generated DLL.\n\
  license-manifest    Regenerate the local third-party manifest."
	);
}

fn print_paddleocr_plan() {
	println!(
		"UNVAC PaddleOCR native experiment\n\n\
Tracked in repo:\n\
- xtask orchestration\n\
- UNVAC-owned C ABI wrapper source\n\
- Rust sys crate for linking to the wrapper\n\
- docs/roadmap notes\n\n\
Not tracked in repo:\n\
- PaddleOCR checkout\n\
- Paddle Inference binaries\n\
- OpenCV binaries\n\
- model data\n\
- generated license bundle\n\n\
Default layout:\n\
- vendor root: {VENDOR_ROOT}\n\
- wrapper: {NATIVE_WRAPPER_DIR}\n"
	);
}
