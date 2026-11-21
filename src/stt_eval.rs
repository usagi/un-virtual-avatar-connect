use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use vosk::{DecodingState, Model, Recognizer};
#[cfg(feature = "stt-eval-whisper")]
use whisper_rs::{install_logging_hooks, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::processor::voice_vosk::{complete_result_text, VOSK_CHUNK_SAMPLES, VOSK_SAMPLE_RATE};

#[derive(Parser)]
#[command(name = "vac-stt-eval", about = "UNVAC STT comparison harness")]
struct Cli {
	#[command(subcommand)]
	command: Command,
}

#[derive(Subcommand)]
enum Command {
	/// List available input devices.
	Devices,
	/// List available output devices for deterministic loopback playback.
	OutputDevices,
	/// Record the default microphone as 16 kHz mono PCM WAV.
	Record {
		#[arg(long)]
		output: PathBuf,
		#[arg(long, default_value_t = 8)]
		seconds: u64,
		#[arg(long, default_value_t = 3)]
		countdown_seconds: u64,
		#[arg(long)]
		device: Option<String>,
		#[arg(long, conflicts_with = "device")]
		device_index: Option<usize>,
	},
	/// Play a WAV through a selected output device such as VB-CABLE Input.
	Play {
		#[arg(long)]
		input: PathBuf,
		#[arg(long)]
		device: Option<String>,
		#[arg(long, conflicts_with = "device")]
		device_index: Option<usize>,
	},
	/// Inspect WAV signal levels without loading an STT model.
	Inspect {
		#[arg(long)]
		input: PathBuf,
	},
	/// Replay a WAV through the current Vosk baseline.
	Vosk {
		#[arg(long)]
		input: PathBuf,
		#[arg(long, default_value = "vosk-model-small-ja-0.22")]
		model: String,
		#[arg(long)]
		reference: Option<String>,
		#[arg(long)]
		json_out: Option<PathBuf>,
		#[arg(long, default_value_t = 1_000)]
		flush_ms: u64,
	},
	/// Transcribe the estimated utterance region with Whisper.
	#[cfg(feature = "stt-eval-whisper")]
	Whisper {
		#[arg(long)]
		input: PathBuf,
		#[arg(long, default_value = "small")]
		model: String,
		#[arg(long)]
		reference: Option<String>,
		#[arg(long)]
		json_out: Option<PathBuf>,
		#[arg(long, default_value = "ja")]
		language: String,
		#[arg(long, default_value_t = 500)]
		padding_ms: u64,
	},
	/// Probe a packaged Whisper worker with incremental audio prefixes and a final pass.
	#[cfg(feature = "voice-whisper")]
	Worker {
		#[arg(long)]
		input: PathBuf,
		#[arg(long)]
		worker: PathBuf,
		#[arg(long)]
		model: String,
		#[arg(long)]
		reference: Option<String>,
		#[arg(long)]
		json_out: Option<PathBuf>,
		#[arg(long, default_value = "ja")]
		language: String,
		#[arg(long, default_value_t = 650)]
		partial_interval_ms: u64,
		#[arg(long, default_value_t = 1_300)]
		partial_min_audio_ms: u64,
		#[arg(long, default_value_t = -45.0)]
		vad_threshold_db: f32,
		#[arg(long, default_value_t = 600)]
		vad_endpoint_ms: u64,
		#[arg(long, default_value_t = 400)]
		vad_pre_roll_ms: u64,
		#[arg(long, default_value_t = 100)]
		vad_min_speech_ms: u64,
		#[arg(long, default_value = "")]
		gpu_device: String,
	},
}

#[derive(Serialize)]
struct EvalReport {
	engine: &'static str,
	model: String,
	input: PathBuf,
	input_duration_ms: u64,
	chunk_ms: u64,
	flush_ms: u64,
	first_partial_audio_ms: Option<u64>,
	first_final_audio_ms: Option<u64>,
	last_final_audio_ms: Option<u64>,
	final_count: usize,
	first_partial_latency_ms: Option<u64>,
	final_latency_ms: Option<u64>,
	processing_ms: u64,
	realtime_factor: f64,
	reference: Option<String>,
	recognized: String,
	character_error_rate: Option<f64>,
	audio: AudioStats,
	vad: VadEstimate,
	events: Vec<EvalEvent>,
}

#[derive(Serialize)]
struct VadEstimate {
	threshold_dbfs: f32,
	frame_ms: u64,
	minimum_voiced_ms: u64,
	speech_start_ms: Option<u64>,
	speech_end_ms: Option<u64>,
}

#[derive(Serialize)]
struct AudioStats {
	peak_dbfs: Option<f32>,
	rms_dbfs: Option<f32>,
	max_frame_rms_dbfs: Option<f32>,
	frames_above_vad_threshold: usize,
	frame_count: usize,
}

#[derive(Serialize)]
struct InspectReport {
	input: PathBuf,
	duration_ms: u64,
	audio: AudioStats,
	vad: VadEstimate,
}

#[cfg(feature = "stt-eval-whisper")]
#[derive(Serialize)]
struct WhisperEvalReport {
	engine: &'static str,
	backend: &'static str,
	model: String,
	input: PathBuf,
	input_duration_ms: u64,
	inference_audio_start_ms: u64,
	inference_audio_end_ms: u64,
	inference_audio_duration_ms: u64,
	padding_ms: u64,
	processing_ms: u64,
	realtime_factor: f64,
	reference: Option<String>,
	recognized: String,
	character_error_rate: Option<f64>,
	audio: AudioStats,
	vad: VadEstimate,
}

#[cfg(feature = "voice-whisper")]
#[derive(Serialize)]
struct WorkerEvalReport {
	engine: &'static str,
	backend: String,
	worker: PathBuf,
	model: String,
	input: PathBuf,
	input_duration_ms: u64,
	inference_audio_start_ms: u64,
	inference_audio_end_ms: u64,
	inference_audio_duration_ms: u64,
	partial_interval_ms: u64,
	partial_min_audio_ms: u64,
	vad_threshold_db: f32,
	vad_endpoint_ms: u64,
	vad_pre_roll_ms: u64,
	vad_min_speech_ms: u64,
	first_partial_audio_ms: Option<u64>,
	first_partial_inference_ms: Option<u64>,
	max_partial_inference_ms: Option<u64>,
	partial_deadline_miss_count: usize,
	final_inference_ms: u64,
	realtime_factor: f64,
	reference: Option<String>,
	recognized: String,
	character_error_rate: Option<f64>,
	audio: AudioStats,
	vad: VadEstimate,
	events: Vec<WorkerEvalEvent>,
}

#[cfg(feature = "voice-whisper")]
#[derive(Serialize)]
struct WorkerEvalEvent {
	kind: &'static str,
	audio_ms: u64,
	inference_audio_ms: u64,
	inference_ms: u64,
	emitted: bool,
	text: String,
}

#[derive(Serialize)]
struct InputDeviceInfo {
	index: usize,
	name: String,
	description: String,
	manufacturer: Option<String>,
	driver: Option<String>,
	address: Option<String>,
	is_default: bool,
	sample_rate: Option<u32>,
	channels: Option<u16>,
	sample_format: Option<String>,
}

#[derive(Serialize)]
struct EvalEvent {
	kind: &'static str,
	audio_ms: u64,
	processing_ms: u64,
	text: String,
}

pub fn run() -> Result<()> {
	let mut logger = env_logger::Builder::from_env(env_logger::Env::default());
	if std::env::var("RUST_LOG").map_or(true, |value| value.trim().is_empty()) {
		logger.filter_module("whisper_rs", log::LevelFilter::Info);
	}
	let _ = logger.try_init();
	match Cli::parse().command {
		Command::Devices => list_devices(),
		Command::OutputDevices => list_output_devices(),
		Command::Record {
			output,
			seconds,
			countdown_seconds,
			device,
			device_index,
		} => record(&output, seconds, countdown_seconds, device.as_deref(), device_index),
		Command::Play {
			input,
			device,
			device_index,
		} => play(&input, device.as_deref(), device_index),
		Command::Inspect { input } => inspect(&input),
		Command::Vosk {
			input,
			model,
			reference,
			json_out,
			flush_ms,
		} => evaluate_vosk(&input, &model, reference, json_out.as_deref(), flush_ms),
		#[cfg(feature = "stt-eval-whisper")]
		Command::Whisper {
			input,
			model,
			reference,
			json_out,
			language,
			padding_ms,
		} => evaluate_whisper(&input, &model, reference, json_out.as_deref(), &language, padding_ms),
		#[cfg(feature = "voice-whisper")]
		Command::Worker {
			input,
			worker,
			model,
			reference,
			json_out,
			language,
			partial_interval_ms,
			partial_min_audio_ms,
			vad_threshold_db,
			vad_endpoint_ms,
			vad_pre_roll_ms,
			vad_min_speech_ms,
			gpu_device,
		} => evaluate_worker(
			&input,
			&worker,
			&model,
			reference,
			json_out.as_deref(),
			&language,
			partial_interval_ms,
			partial_min_audio_ms,
			vad_threshold_db,
			vad_endpoint_ms,
			vad_pre_roll_ms,
			vad_min_speech_ms,
			&gpu_device,
		),
	}
}

fn list_devices() -> Result<()> {
	let host = cpal::default_host();
	let default_name = host.default_input_device().and_then(|device| device_name(&device).ok());
	let mut devices = Vec::new();
	for (index, device) in host.input_devices()?.enumerate() {
		let description = device.description()?;
		let name = description.name().to_owned();
		let config = device.default_input_config().ok();
		devices.push(InputDeviceInfo {
			index,
			is_default: default_name.as_deref() == Some(name.as_str()),
			name,
			description: description.to_string(),
			manufacturer: description.manufacturer().map(str::to_owned),
			driver: description.driver().map(str::to_owned),
			address: description.address().map(str::to_owned),
			sample_rate: config.as_ref().map(|value| value.sample_rate()),
			channels: config.as_ref().map(|value| value.channels()),
			sample_format: config.as_ref().map(|value| format!("{:?}", value.sample_format())),
		});
	}
	println!("{}", serde_json::to_string_pretty(&devices)?);
	Ok(())
}

fn list_output_devices() -> Result<()> {
	let host = cpal::default_host();
	let default_name = host.default_output_device().and_then(|device| device_name(&device).ok());
	let mut devices = Vec::new();
	for (index, device) in host.output_devices()?.enumerate() {
		let description = device.description()?;
		let name = description.name().to_owned();
		let config = device.default_output_config().ok();
		devices.push(InputDeviceInfo {
			index,
			is_default: default_name.as_deref() == Some(name.as_str()),
			name,
			description: description.to_string(),
			manufacturer: description.manufacturer().map(str::to_owned),
			driver: description.driver().map(str::to_owned),
			address: description.address().map(str::to_owned),
			sample_rate: config.as_ref().map(|value| value.sample_rate()),
			channels: config.as_ref().map(|value| value.channels()),
			sample_format: config.as_ref().map(|value| format!("{:?}", value.sample_format())),
		});
	}
	println!("{}", serde_json::to_string_pretty(&devices)?);
	Ok(())
}

fn inspect(input: &Path) -> Result<()> {
	let samples = read_wav_16k_mono(input)?;
	if samples.is_empty() {
		bail!("input WAV contains no samples: {}", input.display());
	}
	let report = InspectReport {
		input: input.to_path_buf(),
		duration_ms: samples_to_ms(samples.len()),
		audio: analyze_audio(&samples),
		vad: estimate_speech_region(&samples),
	};
	println!("{}", serde_json::to_string_pretty(&report)?);
	Ok(())
}

fn record(output: &Path, seconds: u64, countdown_seconds: u64, device_query: Option<&str>, device_index: Option<usize>) -> Result<()> {
	if seconds == 0 {
		bail!("--seconds must be greater than zero");
	}

	let host = cpal::default_host();
	let device = select_input_device(&host, device_query, device_index)?;
	let device_name = device_name(&device).unwrap_or_else(|_| "<unknown>".into());
	let supported = device.default_input_config()?;
	let sample_format = supported.sample_format();
	let sample_rate = supported.sample_rate();
	let channels = supported.channels() as usize;
	let stream_config: StreamConfig = supported.into();
	let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
	let callback_samples = samples.clone();
	let error_slot = Arc::new(Mutex::new(None::<String>));
	let callback_error = error_slot.clone();
	let error_fn = move |error: cpal::StreamError| {
		if let Ok(mut slot) = callback_error.lock() {
			*slot = Some(error.to_string());
		}
	};

	let stream: Stream = match sample_format {
		SampleFormat::F32 => device.build_input_stream(
			&stream_config,
			move |data: &[f32], _| append_mono(&callback_samples, data, channels),
			error_fn,
			None,
		)?,
		SampleFormat::I16 => device.build_input_stream(
			&stream_config,
			move |data: &[i16], _| {
				let converted = data.iter().map(|&sample| sample as f32 / 32_768.0).collect::<Vec<_>>();
				append_mono(&callback_samples, &converted, channels);
			},
			error_fn,
			None,
		)?,
		SampleFormat::U16 => device.build_input_stream(
			&stream_config,
			move |data: &[u16], _| {
				let converted = data.iter().map(|&sample| (sample as f32 - 32_768.0) / 32_768.0).collect::<Vec<_>>();
				append_mono(&callback_samples, &converted, channels);
			},
			error_fn,
			None,
		)?,
		format => bail!("unsupported input sample format: {format:?}"),
	};

	println!(
		"recording: device={device_name:?}, input={} Hz/{} ch/{sample_format:?}, duration={seconds}s",
		sample_rate, channels
	);
	// Capture the countdown too so UI delivery delay cannot clip an early utterance.
	stream.play()?;
	for remaining in (1..=countdown_seconds).rev() {
		println!("recording starts in {remaining}...");
		std::io::stdout().flush()?;
		std::thread::sleep(Duration::from_secs(1));
	}
	println!("recording started; keep about one second of silence before and after speech");
	std::io::stdout().flush()?;
	std::thread::sleep(Duration::from_secs(seconds));
	drop(stream);

	if let Some(error) = error_slot.lock().map_err(|_| anyhow!("audio error lock poisoned"))?.take() {
		bail!("audio stream failed: {error}");
	}

	let native = samples.lock().map_err(|_| anyhow!("audio sample lock poisoned"))?.clone();
	if native.is_empty() {
		bail!("microphone returned no samples");
	}
	let mono_16k = linear_resample(&native, sample_rate, VOSK_SAMPLE_RATE);
	write_wav(output, &mono_16k)?;
	println!(
		"recorded: {} ({} samples, {:.3}s)",
		output.display(),
		mono_16k.len(),
		mono_16k.len() as f64 / VOSK_SAMPLE_RATE as f64
	);
	Ok(())
}

fn play(input: &Path, device_query: Option<&str>, device_index: Option<usize>) -> Result<()> {
	let source_i16 = read_wav_16k_mono(input)?;
	if source_i16.is_empty() {
		bail!("input WAV contains no samples: {}", input.display());
	}
	let source = source_i16.iter().map(|sample| *sample as f32 / 32_768.0).collect::<Vec<_>>();
	let host = cpal::default_host();
	let device = select_output_device(&host, device_query, device_index)?;
	let device_name = device_name(&device).unwrap_or_else(|_| "<unknown>".into());
	let supported = device.default_output_config()?;
	let sample_format = supported.sample_format();
	let sample_rate = supported.sample_rate();
	let channels = supported.channels() as usize;
	let stream_config: StreamConfig = supported.into();
	let samples = Arc::new(linear_resample(&source, VOSK_SAMPLE_RATE, sample_rate));
	let cursor = Arc::new(AtomicUsize::new(0));
	let error_slot = Arc::new(Mutex::new(None::<String>));
	let callback_error = error_slot.clone();
	let error_fn = move |error: cpal::StreamError| {
		if let Ok(mut slot) = callback_error.lock() {
			*slot = Some(error.to_string());
		}
	};
	let stream: Stream = match sample_format {
		SampleFormat::F32 => {
			let samples = samples.clone();
			let cursor = cursor.clone();
			device.build_output_stream(
				&stream_config,
				move |data: &mut [f32], _| fill_output_f32(data, channels, &samples, &cursor),
				error_fn,
				None,
			)?
		}
		SampleFormat::I16 => {
			let samples = samples.clone();
			let cursor = cursor.clone();
			device.build_output_stream(
				&stream_config,
				move |data: &mut [i16], _| fill_output_i16(data, channels, &samples, &cursor),
				error_fn,
				None,
			)?
		}
		SampleFormat::U16 => {
			let samples = samples.clone();
			let cursor = cursor.clone();
			device.build_output_stream(
				&stream_config,
				move |data: &mut [u16], _| fill_output_u16(data, channels, &samples, &cursor),
				error_fn,
				None,
			)?
		}
		format => bail!("unsupported output sample format: {format:?}"),
	};

	println!(
		"playing: input={} device={device_name:?}, output={} Hz/{} ch/{sample_format:?}",
		input.display(),
		sample_rate,
		channels
	);
	stream.play()?;
	let timeout = Duration::from_secs_f64(samples.len() as f64 / sample_rate as f64 + 5.0);
	let started = Instant::now();
	while cursor.load(Ordering::Acquire) < samples.len() && started.elapsed() < timeout {
		if let Some(error) = error_slot.lock().map_err(|_| anyhow!("audio error lock poisoned"))?.take() {
			bail!("audio stream failed: {error}");
		}
		std::thread::sleep(Duration::from_millis(10));
	}
	drop(stream);
	if cursor.load(Ordering::Acquire) < samples.len() {
		bail!("audio playback timed out");
	}
	println!(
		"played: {} ({:.3}s)",
		input.display(),
		source.len() as f64 / VOSK_SAMPLE_RATE as f64
	);
	Ok(())
}

fn fill_output_f32(output: &mut [f32], channels: usize, samples: &[f32], cursor: &AtomicUsize) {
	fill_output(output, channels, samples, cursor, |sample| sample);
}

fn fill_output_i16(output: &mut [i16], channels: usize, samples: &[f32], cursor: &AtomicUsize) {
	fill_output(output, channels, samples, cursor, |sample| {
		(sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
	});
}

fn fill_output_u16(output: &mut [u16], channels: usize, samples: &[f32], cursor: &AtomicUsize) {
	fill_output(output, channels, samples, cursor, |sample| {
		((sample.clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32) as u16
	});
}

fn fill_output<T: Copy>(output: &mut [T], channels: usize, samples: &[f32], cursor: &AtomicUsize, convert: impl Fn(f32) -> T) {
	if channels == 0 {
		return;
	}
	let mut index = cursor.load(Ordering::Relaxed);
	for frame in output.chunks_mut(channels) {
		let sample = samples.get(index).copied().unwrap_or(0.0);
		frame.fill(convert(sample));
		if index < samples.len() {
			index += 1;
		}
	}
	cursor.store(index, Ordering::Release);
}

fn select_input_device(host: &cpal::Host, query: Option<&str>, index: Option<usize>) -> Result<cpal::Device> {
	if let Some(index) = index {
		return host
			.input_devices()?
			.nth(index)
			.ok_or_else(|| anyhow!("input device index is out of range: {index}; run `cargo xtask stt-eval devices`"));
	}
	let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
		return host.default_input_device().context("default input device is not available");
	};
	let query_lower = query.to_lowercase();
	let mut exact = Vec::new();
	let mut partial = Vec::new();
	for device in host.input_devices()? {
		let name = device_name(&device)?;
		if name.eq_ignore_ascii_case(query) {
			exact.push((name, device));
		} else if name.to_lowercase().contains(&query_lower) {
			partial.push((name, device));
		}
	}
	let mut matches = if exact.is_empty() { partial } else { exact };
	match matches.len() {
		0 => bail!("input device not found: {query:?}; run `cargo xtask stt-eval devices`"),
		1 => Ok(matches.pop().expect("one input device").1),
		_ => {
			let names = matches.into_iter().map(|(name, _)| name).collect::<Vec<_>>().join(", ");
			bail!("input device query is ambiguous: {query:?}; matches: {names}")
		}
	}
}

fn select_output_device(host: &cpal::Host, query: Option<&str>, index: Option<usize>) -> Result<cpal::Device> {
	if let Some(index) = index {
		return host
			.output_devices()?
			.nth(index)
			.ok_or_else(|| anyhow!("output device index is out of range: {index}; run `cargo xtask stt-eval output-devices`"));
	}
	let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
		return host.default_output_device().context("default output device is not available");
	};
	let query_lower = query.to_lowercase();
	let mut exact = Vec::new();
	let mut partial = Vec::new();
	for device in host.output_devices()? {
		let name = device_name(&device)?;
		if name.eq_ignore_ascii_case(query) {
			exact.push((name, device));
		} else if name.to_lowercase().contains(&query_lower) {
			partial.push((name, device));
		}
	}
	let mut matches = if exact.is_empty() { partial } else { exact };
	match matches.len() {
		0 => bail!("output device not found: {query:?}; run `cargo xtask stt-eval output-devices`"),
		1 => Ok(matches.pop().expect("one output device").1),
		_ => {
			let names = matches.into_iter().map(|(name, _)| name).collect::<Vec<_>>().join(", ");
			bail!("output device query is ambiguous: {query:?}; matches: {names}")
		}
	}
}

fn device_name(device: &cpal::Device) -> Result<String> {
	Ok(device.description()?.name().to_owned())
}

fn evaluate_vosk(input: &Path, model_spec: &str, reference: Option<String>, json_out: Option<&Path>, flush_ms: u64) -> Result<()> {
	let input_samples = read_wav_16k_mono(input)?;
	if input_samples.is_empty() {
		bail!("input WAV contains no samples: {}", input.display());
	}
	let input_duration_ms = samples_to_ms(input_samples.len());
	let audio = analyze_audio(&input_samples);
	let vad = estimate_speech_region(&input_samples);
	let model_dir = crate::processor::voice_vosk_model::resolve_vosk_model_dir(model_spec).map_err(anyhow::Error::msg)?;
	let model_path = model_dir
		.to_str()
		.ok_or_else(|| anyhow!("Vosk model path is not UTF-8: {}", model_dir.display()))?;
	let model = Model::new(model_path).ok_or_else(|| anyhow!("failed to load Vosk model: {}", model_dir.display()))?;
	let mut recognizer = Recognizer::new(&model, VOSK_SAMPLE_RATE as f32).context("failed to create Vosk recognizer")?;
	recognizer.set_partial_words(false);

	let flush_samples = (flush_ms as usize).saturating_mul(VOSK_SAMPLE_RATE as usize).saturating_div(1_000);
	let mut replay = input_samples;
	replay.resize(replay.len().saturating_add(flush_samples), 0);
	let started = Instant::now();
	let mut events = Vec::new();
	let mut finals = Vec::new();
	let mut consumed = 0usize;
	let mut last_partial = String::new();

	for chunk in replay.chunks(VOSK_CHUNK_SAMPLES) {
		consumed += chunk.len();
		match recognizer.accept_waveform(chunk)? {
			DecodingState::Running => {
				let text = crate::utility::normalize_interstitial_japanese_spaces(recognizer.partial_result().partial.trim());
				if !text.is_empty() && text != last_partial {
					last_partial = text.clone();
					events.push(EvalEvent {
						kind: "partial",
						audio_ms: samples_to_ms(consumed),
						processing_ms: elapsed_ms(started),
						text,
					});
				}
			}
			DecodingState::Finalized => {
				let text = complete_result_text(recognizer.result());
				last_partial.clear();
				if !text.is_empty() {
					finals.push(text.clone());
					events.push(EvalEvent {
						kind: "final",
						audio_ms: samples_to_ms(consumed),
						processing_ms: elapsed_ms(started),
						text,
					});
				}
				recognizer.reset();
			}
			DecodingState::Failed => {}
		}
	}

	let tail = complete_result_text(recognizer.final_result());
	if !tail.is_empty() {
		finals.push(tail.clone());
		events.push(EvalEvent {
			kind: "final",
			audio_ms: samples_to_ms(consumed),
			processing_ms: elapsed_ms(started),
			text: tail,
		});
	}

	let processing_ms = elapsed_ms(started);
	let recognized = finals.join("");
	let first_partial_audio_ms = events.iter().find(|event| event.kind == "partial").map(|event| event.audio_ms);
	let first_final_audio_ms = events.iter().find(|event| event.kind == "final").map(|event| event.audio_ms);
	let last_final_audio_ms = events.iter().rev().find(|event| event.kind == "final").map(|event| event.audio_ms);
	let final_count = events.iter().filter(|event| event.kind == "final").count();
	let character_error_rate = reference.as_deref().map(|expected| character_error_rate(expected, &recognized));
	let report = EvalReport {
		engine: "vosk",
		model: model_dir.display().to_string(),
		input: input.to_path_buf(),
		input_duration_ms,
		chunk_ms: samples_to_ms(VOSK_CHUNK_SAMPLES),
		flush_ms,
		first_partial_audio_ms,
		first_final_audio_ms,
		last_final_audio_ms,
		final_count,
		first_partial_latency_ms: first_partial_audio_ms
			.zip(vad.speech_start_ms)
			.map(|(partial, speech_start)| partial.saturating_sub(speech_start)),
		final_latency_ms: last_final_audio_ms
			.zip(vad.speech_end_ms)
			.map(|(final_time, speech_end)| final_time.saturating_sub(speech_end)),
		processing_ms,
		realtime_factor: processing_ms as f64 / input_duration_ms.max(1) as f64,
		reference,
		recognized,
		character_error_rate,
		audio,
		vad,
		events,
	};
	let json = serde_json::to_string_pretty(&report)?;
	println!("{json}");
	if let Some(path) = json_out {
		if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
			std::fs::create_dir_all(parent)?;
		}
		std::fs::write(path, format!("{json}\n"))?;
		println!("report: {}", path.display());
	}
	Ok(())
}

#[cfg(feature = "stt-eval-whisper")]
fn evaluate_whisper(
	input: &Path,
	model_spec: &str,
	reference: Option<String>,
	json_out: Option<&Path>,
	language: &str,
	padding_ms: u64,
) -> Result<()> {
	let input_samples = read_wav_16k_mono(input)?;
	if input_samples.is_empty() {
		bail!("input WAV contains no samples: {}", input.display());
	}
	let input_duration_ms = samples_to_ms(input_samples.len());
	let audio = analyze_audio(&input_samples);
	let vad = estimate_speech_region(&input_samples);
	let speech_start_ms = vad
		.speech_start_ms
		.ok_or_else(|| anyhow!("VAD found no speech in {}", input.display()))?;
	let speech_end_ms = vad
		.speech_end_ms
		.ok_or_else(|| anyhow!("VAD found no speech in {}", input.display()))?;
	let inference_audio_start_ms = speech_start_ms.saturating_sub(padding_ms);
	let inference_audio_end_ms = speech_end_ms.saturating_add(padding_ms).min(input_duration_ms);
	let start_sample = ms_to_samples(inference_audio_start_ms).min(input_samples.len());
	let end_sample = ms_to_samples(inference_audio_end_ms).min(input_samples.len());
	let pcm = input_samples[start_sample..end_sample]
		.iter()
		.map(|sample| *sample as f32 / 32_768.0)
		.collect::<Vec<_>>();
	if pcm.len() < (VOSK_SAMPLE_RATE / 10) as usize {
		bail!("Whisper inference region is too short");
	}

	let model_path = crate::processor::voice_whisper_model::resolve_whisper_model(model_spec).map_err(anyhow::Error::msg)?;
	install_logging_hooks();
	let context =
		WhisperContext::new_with_params(&model_path, WhisperContextParameters::default()).context("failed to load Whisper model")?;
	let mut state = context.create_state().context("failed to create Whisper state")?;
	let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 5 });
	params.set_n_threads(num_cpus::get().clamp(1, 8) as i32);
	params.set_language(Some(language));
	params.set_translate(false);
	params.set_print_special(false);
	params.set_print_progress(false);
	params.set_print_realtime(false);
	params.set_print_timestamps(false);

	let started = Instant::now();
	state.full(params, &pcm).context("Whisper inference failed")?;
	let processing_ms = elapsed_ms(started);
	let mut recognized = String::new();
	for segment in state.as_iter() {
		let text = segment.to_string();
		if !text.trim().is_empty() {
			if !recognized.is_empty() {
				recognized.push(' ');
			}
			recognized.push_str(text.trim());
		}
	}
	let recognized = crate::utility::normalize_interstitial_japanese_spaces(recognized.trim());
	let inference_audio_duration_ms = inference_audio_end_ms.saturating_sub(inference_audio_start_ms);
	let character_error_rate = reference.as_deref().map(|expected| character_error_rate(expected, &recognized));
	let report = WhisperEvalReport {
		engine: "whisper",
		backend: if cfg!(feature = "stt-eval-whisper-cuda") { "cuda" } else { "cpu" },
		model: model_path.display().to_string(),
		input: input.to_path_buf(),
		input_duration_ms,
		inference_audio_start_ms,
		inference_audio_end_ms,
		inference_audio_duration_ms,
		padding_ms,
		processing_ms,
		realtime_factor: processing_ms as f64 / inference_audio_duration_ms.max(1) as f64,
		reference,
		recognized,
		character_error_rate,
		audio,
		vad,
	};
	write_report(&report, json_out)
}

#[cfg(feature = "voice-whisper")]
#[allow(clippy::too_many_arguments)]
fn evaluate_worker(
	input: &Path,
	worker: &Path,
	model_spec: &str,
	reference: Option<String>,
	json_out: Option<&Path>,
	language: &str,
	partial_interval_ms: u64,
	partial_min_audio_ms: u64,
	vad_threshold_db: f32,
	vad_endpoint_ms: u64,
	vad_pre_roll_ms: u64,
	vad_min_speech_ms: u64,
	gpu_device: &str,
) -> Result<()> {
	if partial_interval_ms == 0 {
		bail!("partial_interval_ms must be greater than zero");
	}
	if !worker.is_file() {
		bail!("Whisper worker was not found: {}", worker.display());
	}
	let input_samples = read_wav_16k_mono(input)?;
	if input_samples.is_empty() {
		bail!("input WAV contains no samples: {}", input.display());
	}
	let input_duration_ms = samples_to_ms(input_samples.len());
	let audio = analyze_audio(&input_samples);
	let vad = estimate_speech_region(&input_samples);
	let mut pcm = input_samples.iter().map(|sample| *sample as f32 / 32_768.0).collect::<Vec<_>>();
	pcm.resize(pcm.len().saturating_add(ms_to_samples(vad_endpoint_ms.saturating_add(20))), 0.0);

	let model_path = crate::processor::voice_whisper_model::resolve_whisper_model(model_spec).map_err(anyhow::Error::msg)?;
	let mut probe = crate::processor::whisper_worker::WhisperWorkerProbe::start(worker, &model_path, language, gpu_device)
		.map_err(anyhow::Error::msg)?;
	let backend = probe.backend().to_string();
	let mut request_id = 1_u64;
	let mut events = Vec::new();
	let mut last_partial = String::new();
	let frame_ms = crate::processor::voice_whisper::voice_frame_ms();
	let frame_samples = crate::processor::voice_whisper::voice_frame_samples();
	let mut segmenter = crate::processor::voice_whisper::voice_activity_segmenter_for_probe(
		vad_threshold_db,
		vad_endpoint_ms,
		vad_pre_roll_ms,
		vad_min_speech_ms,
	);
	let mut capture_ms = 0_u64;
	let mut wall_ms = 0_u64;
	let mut next_partial_at = None;
	let mut completed = None;
	for frame in pcm.chunks_exact(frame_samples) {
		capture_ms = capture_ms.saturating_add(frame_ms);
		wall_ms = wall_ms.max(capture_ms);
		let was_active = segmenter.is_active();
		let utterance = segmenter.push(frame);
		if !was_active && segmenter.is_active() {
			next_partial_at = Some(wall_ms.saturating_add(partial_interval_ms));
			last_partial.clear();
		}
		if let Some(utterance) = utterance {
			completed = Some((capture_ms, utterance));
			break;
		}
		if next_partial_at.is_some_and(|deadline| wall_ms >= deadline) {
			if let Some(partial_audio) = segmenter.active_audio(partial_min_audio_ms) {
				let inference_audio_ms = samples_to_ms(partial_audio.len());
				let (text, inference_ms) = probe.transcribe(request_id, &partial_audio).map_err(anyhow::Error::msg)?;
				request_id = request_id.wrapping_add(1).max(1);
				let text = crate::utility::normalize_interstitial_japanese_spaces(text.trim());
				let emitted = !text.is_empty() && text != last_partial;
				if emitted {
					last_partial = text.clone();
				}
				events.push(WorkerEvalEvent {
					kind: "partial",
					audio_ms: capture_ms,
					inference_audio_ms,
					inference_ms,
					emitted,
					text,
				});
				wall_ms = wall_ms.saturating_add(inference_ms);
				next_partial_at = Some(wall_ms.saturating_add(partial_interval_ms));
			}
		}
	}
	let (inference_audio_end_ms, final_audio) =
		completed.ok_or_else(|| anyhow!("runtime VAD found no complete utterance in {}", input.display()))?;
	let inference_audio_duration_ms = samples_to_ms(final_audio.len());
	let inference_audio_start_ms = inference_audio_end_ms.saturating_sub(inference_audio_duration_ms);
	let (recognized, final_inference_ms) = probe.transcribe(request_id, &final_audio).map_err(anyhow::Error::msg)?;
	let recognized = crate::utility::normalize_interstitial_japanese_spaces(recognized.trim());
	events.push(WorkerEvalEvent {
		kind: "final",
		audio_ms: inference_audio_end_ms,
		inference_audio_ms: inference_audio_duration_ms,
		inference_ms: final_inference_ms,
		emitted: true,
		text: recognized.clone(),
	});
	let partial_events = events.iter().filter(|event| event.kind == "partial").collect::<Vec<_>>();
	let first_partial = partial_events.iter().find(|event| event.emitted);
	let first_partial_audio_ms = first_partial.map(|event| event.audio_ms);
	let first_partial_inference_ms = first_partial.map(|event| event.inference_ms);
	let max_partial_inference_ms = partial_events.iter().map(|event| event.inference_ms).max();
	let partial_deadline_miss_count = partial_events
		.iter()
		.filter(|event| event.inference_ms > partial_interval_ms)
		.count();
	let character_error_rate = reference.as_deref().map(|expected| character_error_rate(expected, &recognized));
	let report = WorkerEvalReport {
		engine: "whisper-worker",
		backend,
		worker: worker.to_path_buf(),
		model: model_path.display().to_string(),
		input: input.to_path_buf(),
		input_duration_ms,
		inference_audio_start_ms,
		inference_audio_end_ms,
		inference_audio_duration_ms,
		partial_interval_ms,
		partial_min_audio_ms,
		vad_threshold_db,
		vad_endpoint_ms,
		vad_pre_roll_ms,
		vad_min_speech_ms,
		first_partial_audio_ms,
		first_partial_inference_ms,
		max_partial_inference_ms,
		partial_deadline_miss_count,
		final_inference_ms,
		realtime_factor: final_inference_ms as f64 / inference_audio_duration_ms.max(1) as f64,
		reference,
		recognized,
		character_error_rate,
		audio,
		vad,
		events,
	};
	write_report(&report, json_out)
}

fn write_report<T: Serialize>(report: &T, json_out: Option<&Path>) -> Result<()> {
	let json = serde_json::to_string_pretty(report)?;
	println!("{json}");
	if let Some(path) = json_out {
		if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
			std::fs::create_dir_all(parent)?;
		}
		std::fs::write(path, format!("{json}\n"))?;
		println!("report: {}", path.display());
	}
	Ok(())
}

fn append_mono(target: &Arc<Mutex<Vec<f32>>>, interleaved: &[f32], channels: usize) {
	if channels == 0 {
		return;
	}
	if let Ok(mut samples) = target.lock() {
		for frame in interleaved.chunks_exact(channels) {
			samples.push(frame.iter().sum::<f32>() / channels as f32);
		}
	}
}

fn linear_resample(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
	if from_hz == to_hz || input.is_empty() {
		return input.to_vec();
	}
	let ratio = from_hz as f64 / to_hz as f64;
	let output_len = (input.len() as f64 / ratio).floor() as usize;
	(0..output_len)
		.map(|index| {
			let source = index as f64 * ratio;
			let lower = source.floor() as usize;
			let fraction = source - lower as f64;
			let a = input.get(lower).copied().unwrap_or(0.0);
			let b = input.get(lower + 1).copied().unwrap_or(a);
			(a as f64 * (1.0 - fraction) + b as f64 * fraction) as f32
		})
		.collect()
}

fn write_wav(path: &Path, samples: &[f32]) -> Result<()> {
	if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
		std::fs::create_dir_all(parent)?;
	}
	let spec = hound::WavSpec {
		channels: 1,
		sample_rate: VOSK_SAMPLE_RATE,
		bits_per_sample: 16,
		sample_format: hound::SampleFormat::Int,
	};
	let mut writer = hound::WavWriter::create(path, spec)?;
	for sample in samples {
		writer.write_sample((sample * 32_768.0).clamp(-32_768.0, 32_767.0) as i16)?;
	}
	writer.finalize()?;
	Ok(())
}

fn read_wav_16k_mono(path: &Path) -> Result<Vec<i16>> {
	let mut reader = hound::WavReader::open(path).with_context(|| format!("open WAV: {}", path.display()))?;
	let spec = reader.spec();
	if spec.channels == 0 {
		bail!("WAV has zero channels");
	}
	let interleaved = match spec.sample_format {
		hound::SampleFormat::Int => {
			let scale = 2_f32.powi(spec.bits_per_sample.saturating_sub(1) as i32);
			reader
				.samples::<i32>()
				.map(|sample| sample.map(|value| value as f32 / scale))
				.collect::<std::result::Result<Vec<_>, _>>()?
		}
		hound::SampleFormat::Float => reader.samples::<f32>().collect::<std::result::Result<Vec<_>, _>>()?,
	};
	let mono = interleaved
		.chunks_exact(spec.channels as usize)
		.map(|frame| frame.iter().sum::<f32>() / spec.channels as f32)
		.collect::<Vec<_>>();
	Ok(linear_resample(&mono, spec.sample_rate, VOSK_SAMPLE_RATE)
		.into_iter()
		.map(|sample| (sample * 32_768.0).clamp(-32_768.0, 32_767.0) as i16)
		.collect())
}

fn samples_to_ms(samples: usize) -> u64 {
	(samples as u64).saturating_mul(1_000) / VOSK_SAMPLE_RATE as u64
}

#[cfg(any(feature = "stt-eval-whisper", feature = "voice-whisper"))]
fn ms_to_samples(milliseconds: u64) -> usize {
	milliseconds.saturating_mul(VOSK_SAMPLE_RATE as u64).saturating_div(1_000) as usize
}

fn elapsed_ms(started: Instant) -> u64 {
	started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn analyze_audio(samples: &[i16]) -> AudioStats {
	const FRAME_MS: u64 = 20;
	const VAD_THRESHOLD_DBFS: f32 = -45.0;
	let frame_samples = (VOSK_SAMPLE_RATE as u64 * FRAME_MS / 1_000) as usize;
	let peak = samples
		.iter()
		.map(|sample| sample.unsigned_abs() as f32 / 32_768.0)
		.fold(0.0_f32, f32::max);
	let overall_rms = rms(samples);
	let frame_rms = samples.chunks(frame_samples).map(rms).collect::<Vec<_>>();
	let max_frame_rms = frame_rms.iter().copied().fold(0.0_f32, f32::max);
	let threshold = 10_f32.powf(VAD_THRESHOLD_DBFS / 20.0);
	AudioStats {
		peak_dbfs: amplitude_to_dbfs(peak),
		rms_dbfs: amplitude_to_dbfs(overall_rms),
		max_frame_rms_dbfs: amplitude_to_dbfs(max_frame_rms),
		frames_above_vad_threshold: frame_rms.iter().filter(|value| **value >= threshold).count(),
		frame_count: frame_rms.len(),
	}
}

fn rms(samples: &[i16]) -> f32 {
	if samples.is_empty() {
		return 0.0;
	}
	let mean_square = samples
		.iter()
		.map(|sample| {
			let normalized = *sample as f32 / 32_768.0;
			normalized * normalized
		})
		.sum::<f32>()
		/ samples.len() as f32;
	mean_square.sqrt()
}

fn amplitude_to_dbfs(amplitude: f32) -> Option<f32> {
	(amplitude > 0.0).then(|| 20.0 * amplitude.log10())
}

fn estimate_speech_region(samples: &[i16]) -> VadEstimate {
	const FRAME_MS: u64 = 20;
	const MINIMUM_VOICED_MS: u64 = 100;
	const THRESHOLD_DBFS: f32 = -45.0;
	let frame_samples = (VOSK_SAMPLE_RATE as u64 * FRAME_MS / 1_000) as usize;
	let minimum_frames = (MINIMUM_VOICED_MS / FRAME_MS) as usize;
	let threshold = 10_f32.powf(THRESHOLD_DBFS / 20.0);
	let voiced = samples
		.chunks(frame_samples)
		.map(|frame| rms(frame) >= threshold)
		.collect::<Vec<_>>();

	let speech_start_frame = voiced
		.windows(minimum_frames)
		.position(|window| window.iter().all(|is_voiced| *is_voiced));
	let speech_end_frame = speech_start_frame.and_then(|start| {
		voiced
			.iter()
			.enumerate()
			.skip(start)
			.filter_map(|(index, is_voiced)| is_voiced.then_some(index))
			.last()
	});
	VadEstimate {
		threshold_dbfs: THRESHOLD_DBFS,
		frame_ms: FRAME_MS,
		minimum_voiced_ms: MINIMUM_VOICED_MS,
		speech_start_ms: speech_start_frame.map(|frame| frame as u64 * FRAME_MS),
		speech_end_ms: speech_end_frame.map(|frame| (frame as u64 + 1) * FRAME_MS),
	}
}

fn character_error_rate(expected: &str, actual: &str) -> f64 {
	let expected = normalize_cer(expected);
	let actual = normalize_cer(actual);
	if expected.is_empty() {
		return if actual.is_empty() { 0.0 } else { 1.0 };
	}
	levenshtein(&expected, &actual) as f64 / expected.len() as f64
}

fn normalize_cer(text: &str) -> Vec<char> {
	text.chars()
		.filter(|character| !character.is_whitespace() && !character.is_ascii_punctuation())
		.filter(|character| !matches!(character, '。' | '、' | '！' | '？' | '「' | '」' | '（' | '）'))
		.flat_map(char::to_lowercase)
		.collect()
}

fn levenshtein(left: &[char], right: &[char]) -> usize {
	let mut previous = (0..=right.len()).collect::<Vec<_>>();
	let mut current = vec![0; right.len() + 1];
	for (left_index, left_char) in left.iter().enumerate() {
		current[0] = left_index + 1;
		for (right_index, right_char) in right.iter().enumerate() {
			current[right_index + 1] = (previous[right_index + 1] + 1)
				.min(current[right_index] + 1)
				.min(previous[right_index] + usize::from(left_char != right_char));
		}
		std::mem::swap(&mut previous, &mut current);
	}
	previous[right.len()]
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn cer_ignores_spacing_and_japanese_punctuation() {
		assert_eq!(character_error_rate("そうだな、それで行こう。", "そうだなそれで行こう"), 0.0);
	}

	#[test]
	fn levenshtein_counts_substitution() {
		assert_eq!(levenshtein(&['a', 'b', 'c'], &['a', 'x', 'c']), 1);
	}

	#[test]
	fn vad_estimates_a_single_speech_region() {
		let mut samples = vec![0; VOSK_SAMPLE_RATE as usize];
		samples.extend(vec![3_000; (VOSK_SAMPLE_RATE / 2) as usize]);
		samples.extend(vec![0; VOSK_SAMPLE_RATE as usize]);
		let estimate = estimate_speech_region(&samples);
		assert_eq!(estimate.speech_start_ms, Some(1_000));
		assert_eq!(estimate.speech_end_ms, Some(1_500));
	}

	#[test]
	fn audio_stats_report_full_scale_peak() {
		let stats = analyze_audio(&[i16::MIN, 0, i16::MAX]);
		assert_eq!(stats.peak_dbfs, Some(0.0));
		assert_eq!(stats.frame_count, 1);
	}

	#[test]
	fn output_filler_duplicates_mono_and_advances_by_frames() {
		let cursor = AtomicUsize::new(0);
		let mut output = [0.0; 6];
		fill_output_f32(&mut output, 2, &[0.25, -0.5], &cursor);
		assert_eq!(output, [0.25, 0.25, -0.5, -0.5, 0.0, 0.0]);
		assert_eq!(cursor.load(Ordering::Acquire), 2);
	}

	#[cfg(feature = "voice-whisper")]
	#[test]
	fn worker_probe_uses_runtime_vad_segmenter() {
		let mut vad = crate::processor::voice_whisper::voice_activity_segmenter_for_probe(-45.0, 600, 400, 100);
		let frame_samples = crate::processor::voice_whisper::voice_frame_samples();
		for _ in 0..15 {
			assert!(vad.push(&vec![0.0; frame_samples]).is_none());
		}
		for _ in 0..5 {
			assert!(vad.push(&vec![0.1; frame_samples]).is_none());
		}
		assert!(vad.is_active());
		assert_eq!(vad.active_audio(0).unwrap().len(), frame_samples * 20);
	}
}
