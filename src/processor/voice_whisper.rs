//! VADで発話区間を切り出し、単一のWhisper workerへ確定認識を依頼する。

use crate::bridges::voice::FlowgraphVoiceIngress;
use crate::flowgraph::node::TriggerHandle;
use crate::processor::voice::{
	record_voice_worker_error, record_voice_worker_final, record_voice_worker_partial, set_voice_worker_backend, voice_worker_status,
	VoiceIngress, VoiceSink,
};
use crate::processor::voice_partial_preview::VoicePartialPreview;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SAMPLE_RATE: u32 = 16_000;
const FRAME_MS: u64 = 20;
const FRAME_SAMPLES: usize = (SAMPLE_RATE as usize * FRAME_MS as usize) / 1_000;
const MAX_UTTERANCE_MS: u64 = 30_000;

pub(crate) fn spawn_from_flowgraph(
	ingress: &FlowgraphVoiceIngress,
	tokio_handle: tokio::runtime::Handle,
	trigger: TriggerHandle,
	control_event_tx: tokio::sync::broadcast::Sender<crate::control_events::ControlEvent>,
	speech_floor: Arc<crate::state::SpeechFloorManager>,
	voice_input: Arc<crate::state::VoiceInputControl>,
	state: std::sync::Weak<tokio::sync::RwLock<crate::state::State>>,
) -> Option<VoiceIngress> {
	if ingress.model_path.trim().is_empty() {
		log::error!(
			"《Flowgraph/Voice》: ノード {} の `model_path` が空です。Whisper GGMLモデルを指定してください。",
			ingress.node_id
		);
		return None;
	}

	let partial_preview = VoicePartialPreview::spawn(&tokio_handle, state, ingress.partial_channel.clone());
	let sink = VoiceSink::new(
		trigger,
		ingress.node_id.clone(),
		ingress.fixed_channel.clone(),
		control_event_tx,
		speech_floor.clone(),
		voice_input,
		partial_preview,
		ingress.speech_floor_key.clone(),
		ingress.speech_floor_release_delay_ms,
	);
	let config = WhisperConfig {
		model_spec: ingress.model_path.clone(),
		source: ingress.source.clone(),
		language: whisper_lang_primary(&ingress.language_code),
		backend: ingress.whisper_backend.clone(),
		gpu_device: ingress.whisper_gpu_device.clone(),
		partial_interval_ms: ingress.partial_interval_ms,
		partial_min_audio_ms: ingress.partial_min_audio_ms,
		vad: VadConfig {
			threshold_db: ingress.vad_threshold_db,
			endpoint_ms: ingress.vad_endpoint_ms,
			pre_roll_ms: ingress.vad_pre_roll_ms,
			min_speech_ms: ingress.vad_min_speech_ms,
			max_utterance_ms: MAX_UTTERANCE_MS,
		},
		source_label: format!("V2/Flowgraph/Voice/Whisper node={}", ingress.node_id),
	};

	let stop_flag = Arc::new(AtomicBool::new(false));
	let stop_flag_worker = stop_flag.clone();
	let status = voice_worker_status(ingress);
	let thread_status = status.clone();
	let join_handle = std::thread::Builder::new()
		.name("vac-voice-whisper-fg".into())
		.spawn(move || {
			match run_whisper_loop(tokio_handle, sink, config, stop_flag_worker, thread_status.clone()) { Err(error) => {
				set_voice_worker_backend(&thread_status, "failed");
				record_voice_worker_error(&thread_status, error.to_string());
				log::error!("《Flowgraph/Voice》: Whisperスレッドが終了しました: {error}");
			} _ => {
				set_voice_worker_backend(&thread_status, "stopped");
			}}
		})
		.ok()?;

	Some(VoiceIngress {
		join_handle,
		stop_flag,
		speech_floor,
		speech_floor_key: ingress.speech_floor_key.clone(),
		status,
	})
}

struct WhisperConfig {
	model_spec: String,
	source: String,
	language: String,
	backend: String,
	gpu_device: String,
	partial_interval_ms: u64,
	partial_min_audio_ms: u64,
	vad: VadConfig,
	source_label: String,
}

#[derive(Clone, Copy, Debug)]
struct VadConfig {
	threshold_db: f32,
	endpoint_ms: u64,
	pre_roll_ms: u64,
	min_speech_ms: u64,
	max_utterance_ms: u64,
}

pub(crate) struct VoiceActivitySegmenter {
	config: VadConfig,
	pre_roll: VecDeque<f32>,
	utterance: Vec<f32>,
	onset_frames: usize,
	silence_frames: usize,
	active: bool,
}

#[cfg(feature = "tools")]
pub(crate) fn voice_activity_segmenter_for_probe(
	threshold_db: f32,
	endpoint_ms: u64,
	pre_roll_ms: u64,
	min_speech_ms: u64,
) -> VoiceActivitySegmenter {
	VoiceActivitySegmenter::new(VadConfig {
		threshold_db,
		endpoint_ms,
		pre_roll_ms,
		min_speech_ms,
		max_utterance_ms: MAX_UTTERANCE_MS,
	})
}

#[cfg(feature = "tools")]
pub(crate) const fn voice_frame_ms() -> u64 {
	FRAME_MS
}

#[cfg(feature = "tools")]
pub(crate) const fn voice_frame_samples() -> usize {
	FRAME_SAMPLES
}

impl VoiceActivitySegmenter {
	fn new(config: VadConfig) -> Self {
		Self {
			config,
			pre_roll: VecDeque::new(),
			utterance: Vec::new(),
			onset_frames: 0,
			silence_frames: 0,
			active: false,
		}
	}

	pub(crate) fn is_active(&self) -> bool {
		self.active
	}

	pub(crate) fn active_audio(&self, min_audio_ms: u64) -> Option<Vec<f32>> {
		(self.active && self.utterance.len() >= ms_to_samples(min_audio_ms)).then(|| self.utterance.clone())
	}

	pub(crate) fn push(&mut self, frame: &[f32]) -> Option<Vec<f32>> {
		debug_assert_eq!(frame.len(), FRAME_SAMPLES);
		let speech = rms_db(frame) >= self.config.threshold_db;
		if !self.active {
			self.pre_roll.extend(frame.iter().copied());
			let max_pre_roll = ms_to_samples(self.config.pre_roll_ms.max(self.config.min_speech_ms));
			while self.pre_roll.len() > max_pre_roll {
				self.pre_roll.pop_front();
			}
			self.onset_frames = if speech { self.onset_frames + 1 } else { 0 };
			if self.onset_frames * FRAME_MS as usize >= self.config.min_speech_ms as usize {
				self.active = true;
				self.silence_frames = 0;
				self.utterance.extend(self.pre_roll.drain(..));
			}
			return None;
		}

		self.utterance.extend_from_slice(frame);
		self.silence_frames = if speech { 0 } else { self.silence_frames + 1 };
		let reached_endpoint = self.silence_frames * FRAME_MS as usize >= self.config.endpoint_ms as usize;
		let reached_limit = self.utterance.len() >= ms_to_samples(self.config.max_utterance_ms);
		if reached_endpoint || reached_limit {
			self.active = false;
			self.onset_frames = 0;
			self.silence_frames = 0;
			return Some(std::mem::take(&mut self.utterance));
		}
		None
	}
}

fn run_whisper_loop(
	rt: tokio::runtime::Handle,
	mut sink: VoiceSink,
	config: WhisperConfig,
	stop_flag: Arc<AtomicBool>,
	status: crate::processor::voice::SharedVoiceWorkerStatus,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	let model_path = super::voice_whisper_model::resolve_whisper_model(&config.model_spec)
		.map_err(|error| -> Box<dyn std::error::Error + Send + Sync> { error.into() })?;
	let mut whisper =
		super::whisper_worker::WhisperWorkerClient::start(&model_path, &config.language, &config.backend, &config.gpu_device)?;
	set_voice_worker_backend(&status, whisper.backend());

	let host = cpal::default_host();
	let device = select_input_device(&host, &config.source)?;
	let device_name = device
		.description()
		.map(|description| description.name().to_string())
		.unwrap_or_else(|_| "<unknown>".into());
	let supported = device.default_input_config()?;
	let sample_format = supported.sample_format();
	let input_rate = supported.sample_rate();
	let channels = supported.channels() as usize;
	let stream_config: StreamConfig = supported.into();
	let native_buffer = Arc::new(Mutex::new(Vec::<f32>::new()));
	let stream = build_input_stream(&device, &stream_config, sample_format, channels, input_rate, native_buffer.clone())?;
	stream.play()?;
	let _keep_stream_alive = stream;

	log::info!(
		"《Voice》: Whisper開始 input={device_name:?} {}Hz {}ch VAD={}dB endpoint={}ms pre_roll={}ms backend={} ({})",
		input_rate,
		channels,
		config.vad.threshold_db,
		config.vad.endpoint_ms,
		config.vad.pre_roll_ms,
		whisper.backend(),
		config.source_label
	);

	let mut segmenter = VoiceActivitySegmenter::new(config.vad);
	let partial_interval = (config.partial_interval_ms > 0).then(|| Duration::from_millis(config.partial_interval_ms));
	let mut next_partial_at: Option<Instant> = None;
	let mut last_partial = String::new();
	let mut pending = Vec::<f32>::with_capacity(FRAME_SAMPLES * 4);
	while !stop_flag.load(Ordering::Relaxed) {
		std::thread::sleep(Duration::from_millis(FRAME_MS));
		let native = drain_native_buffer(&native_buffer, 4_096);
		if native.is_empty() {
			continue;
		}
		pending.extend(linear_resample_mono(&native, input_rate, SAMPLE_RATE));
		while pending.len() >= FRAME_SAMPLES {
			let frame: Vec<f32> = pending.drain(..FRAME_SAMPLES).collect();
			let was_active = segmenter.is_active();
			let completed = segmenter.push(&frame);
			if !was_active && segmenter.is_active() {
				sink.on_speech_started();
				next_partial_at = partial_interval.map(|interval| Instant::now() + interval);
				last_partial.clear();
			}
			let Some(utterance) = completed else {
				if let (Some(interval), Some(deadline)) = (partial_interval, next_partial_at) {
					if Instant::now() >= deadline {
						if let Some(audio) = segmenter.active_audio(config.partial_min_audio_ms) {
							match whisper.transcribe(audio) {
								Ok((text, inference_ms)) => {
									set_voice_worker_backend(&status, whisper.backend());
									let text = crate::utility::normalize_interstitial_japanese_spaces(text.trim());
									log::trace!("《Voice》: Whisper途中 inference={}ms backend={}", inference_ms, whisper.backend());
									let emitted = !text.is_empty() && text != last_partial;
									record_voice_worker_partial(&status, Some(inference_ms), emitted);
									if emitted {
										last_partial = text.clone();
										sink.on_partial(text);
									}
								}
								Err(error) => {
									record_voice_worker_error(&status, error.clone());
									log::warn!("《Voice》: Whisper途中認識に失敗: {error}");
								}
							}
							next_partial_at = Some(Instant::now() + interval);
						}
					}
				}
				continue;
			};
			next_partial_at = None;
			last_partial.clear();
			let final_text = match whisper.transcribe(utterance) {
				Ok((text, inference_ms)) => {
					set_voice_worker_backend(&status, whisper.backend());
					record_voice_worker_final(&status, Some(inference_ms));
					log::debug!("《Voice》: Whisper確定 inference={}ms backend={}", inference_ms, whisper.backend());
					crate::utility::normalize_interstitial_japanese_spaces(text.trim())
				}
				Err(error) => {
					record_voice_worker_error(&status, error.clone());
					log::warn!("《Voice》: Whisper worker失敗: {error}");
					String::new()
				}
			};
			sink.on_final(&rt, final_text);
		}
	}
	log::info!("《Voice》: Whisper shutdown ({})", config.source_label);
	Ok(())
}

fn select_input_device(host: &cpal::Host, source: &str) -> Result<Device, Box<dyn std::error::Error + Send + Sync>> {
	let source = source.trim();
	if source.is_empty() {
		return host
			.default_input_device()
			.ok_or_else(|| "既定の入力オーディオデバイスがありません".into());
	}
	let source_lower = source.to_lowercase();
	for device in host.input_devices()? {
		let name = device
			.description()
			.map(|description| description.name().to_string())
			.unwrap_or_default();
		if name.to_lowercase().contains(&source_lower) {
			return Ok(device);
		}
	}
	Err(format!("入力オーディオデバイス {source:?} が見つかりません").into())
}

fn build_input_stream(
	device: &Device,
	config: &StreamConfig,
	format: SampleFormat,
	channels: usize,
	input_rate: u32,
	buffer: Arc<Mutex<Vec<f32>>>,
) -> Result<Stream, Box<dyn std::error::Error + Send + Sync>> {
	let error_callback = |error| log::error!("《Voice》: cpalストリームエラー: {error}");
	let stream = match format {
		SampleFormat::F32 => {
			let buffer = buffer.clone();
			device.build_input_stream(
				config,
				move |data: &[f32], _| append_interleaved(&buffer, data, channels, input_rate),
				error_callback,
				None,
			)?
		}
		SampleFormat::I16 => {
			let buffer = buffer.clone();
			device.build_input_stream(
				config,
				move |data: &[i16], _| {
					let samples = data.iter().map(|sample| *sample as f32 / 32_768.0).collect::<Vec<_>>();
					append_interleaved(&buffer, &samples, channels, input_rate);
				},
				error_callback,
				None,
			)?
		}
		SampleFormat::U16 => device.build_input_stream(
			config,
			move |data: &[u16], _| {
				let samples = data
					.iter()
					.map(|sample| (*sample as i32 - 32_768) as f32 / 32_768.0)
					.collect::<Vec<_>>();
				append_interleaved(&buffer, &samples, channels, input_rate);
			},
			error_callback,
			None,
		)?,
		other => return Err(format!("未対応のサンプル形式: {other:?}").into()),
	};
	Ok(stream)
}

fn append_interleaved(buffer: &Arc<Mutex<Vec<f32>>>, data: &[f32], channels: usize, input_rate: u32) {
	if channels == 0 {
		return;
	}
	if let Ok(mut samples) = buffer.lock() {
		for frame in data.chunks_exact(channels) {
			samples.push(frame.iter().sum::<f32>() / channels as f32);
		}
		let overflow = samples.len().saturating_sub(input_rate as usize * 30);
		if overflow > 0 {
			samples.drain(..overflow);
		}
	}
}

fn drain_native_buffer(buffer: &Arc<Mutex<Vec<f32>>>, max_samples: usize) -> Vec<f32> {
	let Ok(mut samples) = buffer.lock() else {
		return Vec::new();
	};
	let take = samples.len().min(max_samples);
	samples.drain(..take).collect()
}

fn linear_resample_mono(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
	if from_hz == to_hz || input.is_empty() {
		return input.to_vec();
	}
	let ratio = from_hz as f64 / to_hz as f64;
	let output_len = (input.len() as f64 / ratio).floor() as usize;
	(0..output_len)
		.map(|index| {
			let source = index as f64 * ratio;
			let base = source.floor() as usize;
			let fraction = source - base as f64;
			let first = input.get(base).copied().unwrap_or(0.0);
			let second = input.get(base + 1).copied().unwrap_or(first);
			(first as f64 * (1.0 - fraction) + second as f64 * fraction) as f32
		})
		.collect()
}

fn rms_db(frame: &[f32]) -> f32 {
	let rms = (frame.iter().map(|sample| sample * sample).sum::<f32>() / frame.len().max(1) as f32).sqrt();
	20.0 * rms.max(1.0e-9).log10()
}

fn ms_to_samples(milliseconds: u64) -> usize {
	(SAMPLE_RATE as u64 * milliseconds / 1_000) as usize
}

fn whisper_lang_primary(language: &str) -> String {
	let language = language.trim();
	if language.is_empty() {
		return "ja".into();
	}
	language.split(['-', '_']).next().unwrap_or("ja").to_string()
}

#[cfg(test)]
mod tests {
	use super::*;

	fn config() -> VadConfig {
		VadConfig {
			threshold_db: -45.0,
			endpoint_ms: 600,
			pre_roll_ms: 400,
			min_speech_ms: 100,
			max_utterance_ms: 30_000,
		}
	}

	fn frame(level: f32) -> Vec<f32> {
		vec![level; FRAME_SAMPLES]
	}

	#[test]
	fn vad_keeps_pre_roll_and_finishes_after_endpoint() {
		let mut vad = VoiceActivitySegmenter::new(config());
		for _ in 0..25 {
			assert!(vad.push(&frame(0.0)).is_none());
		}
		for _ in 0..5 {
			assert!(vad.push(&frame(0.1)).is_none());
		}
		assert!(vad.is_active());
		for _ in 0..29 {
			assert!(vad.push(&frame(0.0)).is_none());
		}
		let utterance = vad.push(&frame(0.0)).expect("endpoint");
		assert!(!vad.is_active());
		assert_eq!(utterance.len(), ms_to_samples(1_000));
	}

	#[test]
	fn vad_rejects_short_noise() {
		let mut vad = VoiceActivitySegmenter::new(config());
		for _ in 0..4 {
			assert!(vad.push(&frame(0.1)).is_none());
		}
		assert!(vad.push(&frame(0.0)).is_none());
		assert!(!vad.is_active());
	}

	#[test]
	fn active_audio_is_available_only_during_a_long_enough_utterance() {
		let mut vad = VoiceActivitySegmenter::new(config());
		for _ in 0..5 {
			assert!(vad.push(&frame(0.1)).is_none());
		}
		assert!(vad.is_active());
		assert!(vad.active_audio(700).is_none());
		for _ in 0..30 {
			assert!(vad.push(&frame(0.1)).is_none());
		}
		let snapshot = vad.active_audio(700).expect("partial audio");
		assert_eq!(snapshot.len(), ms_to_samples(700));
		for _ in 0..30 {
			let _ = vad.push(&frame(0.0));
		}
		assert!(vad.active_audio(700).is_none());
	}
}
