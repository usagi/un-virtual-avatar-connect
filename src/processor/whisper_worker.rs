use std::collections::{HashSet, VecDeque};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use vac_stt_protocol::{read_frame, write_frame, WorkerRequest, WorkerResponse, PROTOCOL_VERSION};

pub(crate) struct WhisperWorkerClient {
	model_path: String,
	language: String,
	gpu_device: String,
	candidates: VecDeque<PathBuf>,
	active: Option<WorkerProcess>,
	next_request_id: u64,
}

impl WhisperWorkerClient {
	pub(crate) fn start(model_path: &Path, language: &str, backend: &str, gpu_device: &str) -> Result<Self, String> {
		let mut client = Self {
			model_path: model_path.to_string_lossy().into_owned(),
			language: language.into(),
			gpu_device: gpu_device.trim().into(),
			candidates: worker_candidates(backend),
			active: None,
			next_request_id: 1,
		};
		client.start_next_worker()?;
		Ok(client)
	}

	pub(crate) fn backend(&self) -> &str {
		self.active.as_ref().map(|worker| worker.backend.as_str()).unwrap_or("unavailable")
	}

	pub(crate) fn transcribe(&mut self, pcm_16khz: Vec<f32>) -> Result<(String, u64), String> {
		let request_id = self.next_request_id;
		self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
		let mut last_error = None;
		loop {
			if self.active.is_none() {
				self.start_next_worker().map_err(|start_error| match last_error {
					Some(error) => format!("{error}; fallback worker startup failed: {start_error}"),
					None => start_error,
				})?;
			}
			let result = self
				.active
				.as_mut()
				.expect("worker exists after startup")
				.transcribe(request_id, &pcm_16khz);
			match result {
				Ok(transcript) => return Ok(transcript),
				Err(error) => {
					let backend = self.backend().to_string();
					log::warn!("《Voice》: Whisper {backend} worker failed; trying fallback: {error}");
					last_error = Some(format!("{backend} worker failed: {error}"));
					self.active.take();
				}
			}
		}
	}

	fn start_next_worker(&mut self) -> Result<(), String> {
		let mut errors = Vec::new();
		while let Some(path) = self.candidates.pop_front() {
			match WorkerProcess::start(&path, &self.model_path, &self.language, &self.gpu_device) {
				Ok(worker) => {
					log::info!(
						"《Voice》: Whisper worker started backend={} path={}",
						worker.backend,
						path.display()
					);
					self.active = Some(worker);
					return Ok(());
				}
				Err(error) => {
					log::warn!("《Voice》: Whisper worker startup failed path={}: {error}", path.display());
					errors.push(format!("{}: {error}", path.display()));
				}
			}
		}
		if errors.is_empty() {
			Err("Whisper worker was not found beside the UNVAC executable; reinstall the portable package".into())
		} else {
			Err(format!("no Whisper worker could start ({})", errors.join("; ")))
		}
	}
}

/// STT評価器から配布workerを常駐状態のまま反復利用するための薄いhandle。
#[cfg(feature = "tools")]
pub(crate) struct WhisperWorkerProbe {
	process: WorkerProcess,
}

#[cfg(feature = "tools")]
impl WhisperWorkerProbe {
	pub(crate) fn start(path: &Path, model: &Path, language: &str, gpu_device: &str) -> Result<Self, String> {
		WorkerProcess::start(path, &model.to_string_lossy(), language, gpu_device).map(|process| Self { process })
	}

	pub(crate) fn backend(&self) -> &str {
		&self.process.backend
	}

	pub(crate) fn transcribe(&mut self, request_id: u64, pcm_16khz: &[f32]) -> Result<(String, u64), String> {
		self.process.transcribe(request_id, pcm_16khz)
	}
}

struct WorkerProcess {
	child: Child,
	stdin: ChildStdin,
	stdout: BufReader<ChildStdout>,
	backend: String,
}

impl WorkerProcess {
	fn start(path: &Path, model: &str, language: &str, gpu_device: &str) -> Result<Self, String> {
		let mut command = Command::new(path);
		configure_worker_process(&mut command, is_cpu_worker_path(path));
		command.arg("--model").arg(model).arg("--language").arg(language);
		if is_gpu_worker_path(path) && !gpu_device.is_empty() {
			command.arg("--gpu-device").arg(gpu_device);
		}
		let mut child = command
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::inherit())
			.spawn()
			.map_err(|error| error.to_string())?;
		let stdin = child.stdin.take().ok_or("worker stdin is unavailable")?;
		let stdout = child.stdout.take().ok_or("worker stdout is unavailable")?;
		let mut stdout = BufReader::new(stdout);
		match read_frame::<_, WorkerResponse>(&mut stdout).map_err(|error| error.to_string())? {
			WorkerResponse::Ready { protocol_version, backend } if protocol_version == PROTOCOL_VERSION => Ok(Self {
				child,
				stdin,
				stdout,
				backend,
			}),
			WorkerResponse::Ready { protocol_version, .. } => Err(format!(
				"protocol version mismatch: main={PROTOCOL_VERSION}, worker={protocol_version}"
			)),
			other => Err(format!("unexpected worker startup response: {other:?}")),
		}
	}

	fn transcribe(&mut self, request_id: u64, pcm_16khz: &[f32]) -> Result<(String, u64), String> {
		write_frame(
			&mut self.stdin,
			&WorkerRequest::Transcribe {
				request_id,
				pcm_16khz: pcm_16khz.to_vec(),
			},
		)
		.map_err(|error| error.to_string())?;
		match read_frame::<_, WorkerResponse>(&mut self.stdout).map_err(|error| error.to_string())? {
			WorkerResponse::Transcript {
				request_id: response_id,
				text,
				inference_ms,
			} if response_id == request_id => Ok((text, inference_ms)),
			WorkerResponse::Error {
				request_id: Some(response_id),
				message,
			} if response_id == request_id => Err(message),
			other => Err(format!("unexpected worker response for request {request_id}: {other:?}")),
		}
	}
}

impl Drop for WorkerProcess {
	fn drop(&mut self) {
		let _ = write_frame(&mut self.stdin, &WorkerRequest::Shutdown);
		if self.child.try_wait().ok().flatten().is_none() {
			let _ = self.child.kill();
		}
		let _ = self.child.wait();
	}
}

fn worker_candidates(backend: &str) -> VecDeque<PathBuf> {
	let mut candidates = Vec::new();
	if let Some(path) = std::env::var_os("UNVAC_STT_WORKER").map(PathBuf::from) {
		candidates.push(path);
	}
	if let Ok(exe) = std::env::current_exe() {
		if let Some(dir) = exe.parent() {
			append_bundled_worker_candidates(dir, backend, &mut candidates);
		}
	}

	let mut seen = HashSet::new();
	candidates
		.into_iter()
		.filter(|path| path.is_file())
		.filter(|path| seen.insert(path.clone()))
		.collect()
}

fn append_bundled_worker_candidates(dir: &Path, backend: &str, candidates: &mut Vec<PathBuf>) {
	append_bundled_worker_candidates_for_cpu(dir, backend, candidates, cpu_supports_avx2_worker());
}

fn cpu_supports_avx2_worker() -> bool {
	#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
	return std::is_x86_feature_detected!("sse4.2") && std::is_x86_feature_detected!("avx2")
		&& std::is_x86_feature_detected!("fma") && std::is_x86_feature_detected!("f16c");
	#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
	false
}

fn append_bundled_worker_candidates_for_cpu(dir: &Path, backend: &str, candidates: &mut Vec<PathBuf>, avx2: bool) {
	let backend = backend.trim().to_ascii_lowercase();
	if matches!(backend.as_str(), "" | "auto" | "vulkan") && vulkan_worker_is_usable(dir) {
		candidates.push(dir.join(worker_file_names()[0]));
	}
	if matches!(backend.as_str(), "" | "auto" | "vulkan" | "cpu") {
		let optimized = dir.join(if cfg!(windows) { "vac-stt-worker-cpu-avx2.exe" } else { "vac-stt-worker-cpu-avx2" });
		if avx2 && optimized.is_file() {
			candidates.push(optimized);
		}
		for name in &worker_file_names()[1..] {
			candidates.push(dir.join(name));
		}
	}
}

#[cfg(windows)]
fn worker_file_names() -> [&'static str; 3] {
	["vac-stt-worker-vulkan.exe", "vac-stt-worker-cpu.exe", "vac-stt-worker.exe"]
}

#[cfg(not(windows))]
fn worker_file_names() -> [&'static str; 3] {
	["vac-stt-worker-vulkan", "vac-stt-worker-cpu", "vac-stt-worker"]
}

#[cfg(windows)]
fn configure_worker_process(command: &mut Command, cpu_worker: bool) {
	use std::os::windows::process::CommandExt;
	const CREATE_NO_WINDOW: u32 = 0x0800_0000;
	const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
	command.creation_flags(CREATE_NO_WINDOW | if cpu_worker { BELOW_NORMAL_PRIORITY_CLASS } else { 0 });
}

#[cfg(not(windows))]
fn configure_worker_process(_command: &mut Command, _cpu_worker: bool) {}

fn is_cpu_worker_path(path: &Path) -> bool {
	path.file_stem()
		.and_then(|name| name.to_str())
		.is_some_and(|name| name == "vac-stt-worker" || name.ends_with("-cpu") || name.ends_with("-cpu-avx2"))
}

fn is_gpu_worker_path(path: &Path) -> bool {
	path.file_stem()
		.and_then(|name| name.to_str())
		.is_some_and(|name| name.ends_with("-vulkan"))
}

#[cfg(windows)]
fn vulkan_worker_is_usable(dir: &Path) -> bool {
	let worker = dir.join(worker_file_names()[0]);
	if !worker.is_file() {
		return false;
	}
	std::env::var_os("WINDIR")
		.map(PathBuf::from)
		.is_some_and(|windows| windows.join("System32").join("vulkan-1.dll").is_file())
}

#[cfg(not(windows))]
fn vulkan_worker_is_usable(dir: &Path) -> bool {
	dir.join(worker_file_names()[0]).is_file()
}

#[cfg(all(test, windows))]
mod tests {
	use super::*;
	use std::fs;

	#[test]
	fn optimized_cpu_requires_supported_isa_and_keeps_baseline_fallback() {
		let dir = std::env::temp_dir().join(format!("unvac-stt-isa-test-{}", std::process::id()));
		fs::create_dir_all(&dir).unwrap();
		let optimized = dir.join("vac-stt-worker-cpu-avx2.exe");
		fs::write(&optimized, []).unwrap();
		for supported in [false, true] {
			let mut candidates = Vec::new();
			append_bundled_worker_candidates_for_cpu(&dir, "cpu", &mut candidates, supported);
			assert_eq!(candidates.contains(&optimized), supported);
			assert_eq!(candidates[candidates.len() - 2], dir.join("vac-stt-worker-cpu.exe"));
			if supported { assert_eq!(candidates[0], optimized); }
		}
		assert!(is_cpu_worker_path(&optimized));
		fs::remove_file(&optimized).unwrap();
		let mut candidates = Vec::new();
		append_bundled_worker_candidates_for_cpu(&dir, "cpu", &mut candidates, true);
		assert_eq!(candidates[0], dir.join("vac-stt-worker-cpu.exe"));
		fs::remove_dir(dir).unwrap();
	}

	#[test]
	fn vulkan_worker_precedes_cpu_when_loader_is_available() {
		let dir = std::env::temp_dir().join(format!("unvac-stt-vulkan-worker-test-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).unwrap();
		fs::write(dir.join("vac-stt-worker-vulkan.exe"), []).unwrap();
		fs::write(dir.join("vac-stt-worker-cpu.exe"), []).unwrap();

		let mut candidates = Vec::new();
		append_bundled_worker_candidates(&dir, "auto", &mut candidates);
		assert_eq!(candidates[0], dir.join("vac-stt-worker-vulkan.exe"));
		assert_eq!(candidates[1], dir.join("vac-stt-worker-cpu.exe"));

		fs::remove_dir_all(dir).unwrap();
	}

	#[test]
	fn explicit_cpu_backend_skips_gpu_workers() {
		let dir = std::env::temp_dir().join(format!("unvac-stt-cpu-worker-test-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).unwrap();
		for name in worker_file_names() {
			fs::write(dir.join(name), []).unwrap();
		}

		let mut candidates = Vec::new();
		append_bundled_worker_candidates(&dir, "cpu", &mut candidates);
		assert_eq!(candidates, [dir.join("vac-stt-worker-cpu.exe"), dir.join("vac-stt-worker.exe")]);

		fs::remove_dir_all(dir).unwrap();
	}
}
