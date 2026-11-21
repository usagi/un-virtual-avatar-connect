use anyhow::{Context, Result};
use clap::Parser;
#[cfg(feature = "vulkan")]
use std::ffi::CString;
use std::io::{self, BufReader, BufWriter};
use std::time::Instant;
use vac_stt_protocol::{read_frame, write_frame, WorkerRequest, WorkerResponse, PROTOCOL_VERSION};
use whisper_rs::{install_logging_hooks, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

#[derive(Debug, Parser)]
struct Args {
	#[arg(long)]
	model: String,
	#[arg(long, default_value = "ja")]
	language: String,
	#[arg(long)]
	threads: Option<usize>,
	/// GPU index from the backend's physical-device enumeration, or `auto`.
	#[arg(long, default_value = "auto")]
	gpu_device: String,
}

fn main() {
	if let Err(error) = run() {
		eprintln!("vac-stt-worker: {error:#}");
		std::process::exit(1);
	}
}

fn run() -> Result<()> {
	let args = Args::parse();
	install_logging_hooks();
	configure_gpu_device(&args.gpu_device)?;
	let context = WhisperContext::new_with_params(&args.model, WhisperContextParameters::default())
		.with_context(|| format!("failed to load Whisper model {}", args.model))?;
	let mut state = context.create_state().context("failed to create Whisper state")?;
	let threads = args.threads.unwrap_or_else(|| num_cpus::get().clamp(1, 8)).clamp(1, 64) as i32;
	if cfg!(feature = "vulkan") {
		transcribe(&mut state, &vec![0.0; 32_000], &args.language, threads).context("GPU backend warm-up failed")?;
	}

	let stdin = io::stdin();
	let stdout = io::stdout();
	let mut reader = BufReader::new(stdin.lock());
	let mut writer = BufWriter::new(stdout.lock());
	write_frame(
		&mut writer,
		&WorkerResponse::Ready {
			protocol_version: PROTOCOL_VERSION,
			backend: backend_name().into(),
		},
	)?;

	loop {
		match read_frame::<_, WorkerRequest>(&mut reader)? {
			WorkerRequest::Transcribe { request_id, pcm_16khz } => {
				let started = Instant::now();
				let response = match transcribe(&mut state, &pcm_16khz, &args.language, threads) {
					Ok(text) => WorkerResponse::Transcript {
						request_id,
						text,
						inference_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
					},
					Err(error) => WorkerResponse::Error {
						request_id: Some(request_id),
						message: error.to_string(),
					},
				};
				write_frame(&mut writer, &response)?;
			}
			WorkerRequest::Shutdown => break,
		}
	}
	Ok(())
}

#[cfg(feature = "vulkan")]
fn configure_gpu_device(requested: &str) -> Result<()> {
	if !requested.eq_ignore_ascii_case("auto") {
		let index = requested
			.parse::<usize>()
			.context("Vulkan GPU device must be `auto` or a numeric index")?;
		apply_gpu_device(index)?;
		eprintln!("vac-stt-worker: Vulkan device {index} selected explicitly");
		return Ok(());
	}
	if std::env::var_os("GGML_VK_VISIBLE_DEVICES").is_some() {
		return Ok(());
	}

	use ash::vk;
	let entry = unsafe { ash::Entry::load() }.context("failed to load the Vulkan loader")?;
	let application_name = CString::new("vac-stt-worker")?;
	let application_info = vk::ApplicationInfo::default()
		.application_name(&application_name)
		.api_version(vk::make_api_version(0, 1, 1, 0));
	let create_info = vk::InstanceCreateInfo::default().application_info(&application_info);
	let instance = unsafe { entry.create_instance(&create_info, None) }.context("failed to create a Vulkan instance")?;
	let devices = unsafe { instance.enumerate_physical_devices() }.context("failed to enumerate Vulkan devices")?;
	let properties = devices
		.iter()
		.map(|device| unsafe { instance.get_physical_device_properties(*device) })
		.collect::<Vec<_>>();
	let selected = properties
		.iter()
		.position(|property| property.device_type == vk::PhysicalDeviceType::DISCRETE_GPU)
		.or_else(|| {
			properties
				.iter()
				.position(|property| property.device_type == vk::PhysicalDeviceType::INTEGRATED_GPU)
		})
		.or_else(|| {
			properties
				.iter()
				.position(|property| property.device_type != vk::PhysicalDeviceType::CPU)
		});
	for (index, property) in properties.iter().enumerate() {
		let name = unsafe { std::ffi::CStr::from_ptr(property.device_name.as_ptr()) }.to_string_lossy();
		eprintln!("vac-stt-worker: Vulkan device {index}: {name} ({:?})", property.device_type);
	}
	unsafe { instance.destroy_instance(None) };
	let selected = selected.context("no usable Vulkan GPU was enumerated")?;
	apply_gpu_device(selected)?;
	eprintln!("vac-stt-worker: Vulkan device {selected} selected automatically");
	Ok(())
}

// Windows explicitly guarantees thread-safe environment mutation. On Unix,
// use exec with a prepared child environment: Vulkan drivers may start threads.
#[cfg(feature = "vulkan")]
fn apply_gpu_device(index: usize) -> Result<()> {
	let value = index.to_string();
	if std::env::var("GGML_VK_VISIBLE_DEVICES").as_deref() == Ok(value.as_str()) {
		return Ok(());
	}
	#[cfg(windows)]
	{
		// SAFETY: std::env::set_var is always sound on Windows, including with threads.
		unsafe { std::env::set_var("GGML_VK_VISIBLE_DEVICES", value) };
		Ok(())
	}
	#[cfg(unix)]
	{
		use std::os::unix::process::CommandExt;
		let error = std::process::Command::new(std::env::current_exe()?)
			.args(std::env::args_os().skip(1))
			.env("GGML_VK_VISIBLE_DEVICES", value)
			.exec();
		Err(error).context("restart worker with selected Vulkan device")
	}
}

#[cfg(not(feature = "vulkan"))]
fn configure_gpu_device(_requested: &str) -> Result<()> {
	Ok(())
}

fn backend_name() -> &'static str {
	if cfg!(feature = "vulkan") {
		"vulkan"
	} else {
		"cpu"
	}
}

fn transcribe(state: &mut WhisperState, pcm: &[f32], language: &str, threads: i32) -> Result<String> {
	if pcm.len() < 1_600 {
		anyhow::bail!("audio is shorter than 100 ms");
	}
	let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 5 });
	params.set_n_threads(threads);
	params.set_language(Some(language));
	params.set_translate(false);
	params.set_print_special(false);
	params.set_print_progress(false);
	params.set_print_realtime(false);
	params.set_print_timestamps(false);
	state.full(params, pcm).context("Whisper inference failed")?;

	let mut text = String::new();
	for segment in state.as_iter() {
		let segment = segment.to_string();
		if !segment.trim().is_empty() {
			if !text.is_empty() {
				text.push(' ');
			}
			text.push_str(segment.trim());
		}
	}
	Ok(text)
}
