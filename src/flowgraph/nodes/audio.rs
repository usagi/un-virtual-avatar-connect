//! Audio analysis utility nodes for Phase omega.

use crate::flowgraph::node::{
	get_optional_float, get_optional_int, get_optional_string, get_required_bytes, EffectfulNode, ExecCtx, ExecFireSet, InputMap,
	NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec, PureNode,
};
use crate::flowgraph::socket::{FlowResult, SocketType, SocketValue};
use async_trait::async_trait;
use rodio::Source as _;
use serde_json::json;

const SILENCE_DBFS: f64 = -120.0;

pub struct AudioPlayNode;
pub struct AudioEnvelopeNode;
pub struct AudioPitchNode;

#[derive(Debug, Clone, Copy)]
enum PcmFormat {
	S16Le,
	F32Le,
}

impl PcmFormat {
	fn parse(input: &str) -> Result<Self, NodeExecError> {
		let normalized = input.trim().to_ascii_lowercase().replace('-', "_");
		match normalized.as_str() {
			"" | "pcm_s16le" | "s16le" | "i16le" => Ok(Self::S16Le),
			"pcm_f32le" | "f32le" | "float32le" => Ok(Self::F32Le),
			other => Err(NodeExecError::Generic(anyhow::anyhow!(
				"audio.envelope: unsupported format `{other}`"
			))),
		}
	}

	fn sample_size(self) -> usize {
		match self {
			Self::S16Le => 2,
			Self::F32Le => 4,
		}
	}
}

#[derive(Debug, Clone, Copy)]
struct Envelope {
	rms: f64,
	peak: f64,
	sample_count: usize,
}

fn audio_play_error(message: impl Into<String>) -> NodeOutput {
	let message = message.into();
	NodeOutput::new()
		.set_data("played", SocketValue::Bool(false))
		.set_data("source", SocketValue::String(String::new()))
		.set_data("bytes_len", SocketValue::Int(0))
		.set_data("error", SocketValue::String(message.clone()))
		.set_data("result", SocketValue::Result(FlowResult::err(message).with_code("audio.play")))
		.fire_exec("on_error")
}

fn audio_play_success(played: bool, source: String, bytes_len: usize) -> NodeOutput {
	let result = json!({
		"played": played,
		"source": source,
		"bytes_len": bytes_len,
	});
	NodeOutput::new()
		.set_data("played", SocketValue::Bool(played))
		.set_data("source", SocketValue::String(source))
		.set_data("bytes_len", SocketValue::Int(bytes_len as i64))
		.set_data("error", SocketValue::String(String::new()))
		.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Json(result))))
		.fire_exec("on_success")
}

fn decode_pcm(samples: &[u8], format: PcmFormat) -> Result<Vec<f64>, NodeExecError> {
	let sample_size = format.sample_size();
	let sample_count = samples.len() / sample_size;
	let mut decoded = Vec::with_capacity(sample_count);
	for chunk in samples.chunks_exact(sample_size) {
		let sample = match format {
			PcmFormat::S16Le => i16::from_le_bytes([chunk[0], chunk[1]]) as f64 / 32768.0,
			PcmFormat::F32Le => {
				let value = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64;
				if !value.is_finite() {
					return Err(NodeExecError::Generic(anyhow::anyhow!(
						"audio.envelope: pcm_f32le sample must be finite"
					)));
				}
				value
			}
		};
		decoded.push(sample);
	}
	Ok(decoded)
}

fn analyze_pcm(samples: &[u8], format: PcmFormat) -> Result<Envelope, NodeExecError> {
	let decoded = decode_pcm(samples, format)?;
	if decoded.is_empty() {
		return Ok(Envelope {
			rms: 0.0,
			peak: 0.0,
			sample_count: 0,
		});
	}

	let mut sum_sq = 0.0;
	let mut peak = 0.0;
	for sample in &decoded {
		let abs = sample.abs();
		sum_sq += sample * sample;
		if abs > peak {
			peak = abs;
		}
	}

	Ok(Envelope {
		rms: (sum_sq / decoded.len() as f64).sqrt(),
		peak,
		sample_count: decoded.len(),
	})
}

fn estimate_pitch(samples: &[f64], sample_rate: f64, min_hz: f64, max_hz: f64) -> (f64, f64) {
	if samples.len() < 4 || sample_rate <= 0.0 || min_hz <= 0.0 || max_hz <= min_hz {
		return (0.0, 0.0);
	}
	let max_lag = (sample_rate / min_hz).round() as usize;
	let min_lag = (sample_rate / max_hz).round().max(1.0) as usize;
	if min_lag >= samples.len() || min_lag > max_lag {
		return (0.0, 0.0);
	}

	let mean = samples.iter().sum::<f64>() / samples.len() as f64;
	let centered: Vec<f64> = samples.iter().map(|sample| sample - mean).collect();
	let energy = centered.iter().map(|sample| sample * sample).sum::<f64>();
	if energy <= f64::EPSILON {
		return (0.0, 0.0);
	}

	let bounded_max_lag = max_lag.min(centered.len() - 1);
	let mut best_lag = 0usize;
	let mut best_corr = 0.0;
	let mut prev_corr = 0.0;
	let mut rising = false;
	for lag in min_lag..=bounded_max_lag {
		let len = centered.len() - lag;
		let mut dot = 0.0;
		let mut a_energy = 0.0;
		let mut b_energy = 0.0;
		for i in 0..len {
			let a = centered[i];
			let b = centered[i + lag];
			dot += a * b;
			a_energy += a * a;
			b_energy += b * b;
		}
		let denom = (a_energy * b_energy).sqrt();
		if denom <= f64::EPSILON {
			continue;
		}
		let corr = dot / denom;
		if corr > best_corr {
			best_corr = corr;
			best_lag = lag;
		}
		if rising && corr < prev_corr && prev_corr >= 0.9 {
			let lag = lag - 1;
			return (sample_rate / lag as f64, prev_corr.clamp(0.0, 1.0));
		}
		rising = corr > prev_corr;
		prev_corr = corr;
	}

	if best_lag == 0 {
		(0.0, 0.0)
	} else {
		(sample_rate / best_lag as f64, best_corr.clamp(0.0, 1.0))
	}
}

impl NodeDescriptor for AudioPlayNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.audio.play".into(),
			title: "Audio Play".into(),
			category: "audio".into(),
			description: Some(
				"Effectful audio playback. Plays encoded audio bytes or a file path through the shared VAC audio sink. Use graph-file:assets/sound.wav for an asset relative to the defining graph file.".into(),
			),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("bytes", "Bytes", SocketType::Bytes).with_default(SocketValue::Bytes(Vec::new())),
				PortSpec::input("path", "Path", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("volume", "Volume", SocketType::Float).with_default(SocketValue::Float(1.0)),
				PortSpec::input("speed", "Speed", SocketType::Float).with_default(SocketValue::Float(1.0)),
			],
			outputs: vec![
				PortSpec::exec_output("on_success", "On Success"),
				PortSpec::exec_output("on_error", "On Error"),
				PortSpec::output("played", "Played", SocketType::Bool),
				PortSpec::output("source", "Source", SocketType::String),
				PortSpec::output("bytes_len", "Bytes Length", SocketType::Int),
				PortSpec::output("error", "Error", SocketType::String),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Json))),
			],
			properties: vec![],
		}
	}
}

impl NodeDescriptor for AudioEnvelopeNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.audio.envelope".into(),
			title: "Audio Envelope".into(),
			category: "audio".into(),
			description: Some("Pure PCM envelope analyzer. Computes RMS, peak, dBFS, sample count, frame count, and duration.".into()),
			inputs: vec![
				PortSpec::input("samples", "Samples", SocketType::Bytes).with_default(SocketValue::Bytes(Vec::new())),
				PortSpec::input("format", "Format", SocketType::String)
					.with_default(SocketValue::String("pcm_s16le".into()))
					.with_closed_string_variants(["pcm_s16le", "pcm_f32le"]),
				PortSpec::input("channels", "Channels", SocketType::Int).with_default(SocketValue::Int(1)),
				PortSpec::input("sample_rate", "Sample Rate", SocketType::Int).with_default(SocketValue::Int(16000)),
			],
			outputs: vec![
				PortSpec::output("rms", "RMS", SocketType::Float),
				PortSpec::output("peak", "Peak", SocketType::Float),
				PortSpec::output("dbfs", "dBFS", SocketType::Float),
				PortSpec::output("sample_count", "Sample Count", SocketType::Int),
				PortSpec::output("frame_count", "Frame Count", SocketType::Int),
				PortSpec::output("duration_ms", "Duration ms", SocketType::Float),
			],
			properties: vec![],
		}
	}
}

impl NodeDescriptor for AudioPitchNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.audio.pitch".into(),
			title: "Audio Pitch".into(),
			category: "audio".into(),
			description: Some("Pure PCM pitch estimator using normalized autocorrelation.".into()),
			inputs: vec![
				PortSpec::input("samples", "Samples", SocketType::Bytes).with_default(SocketValue::Bytes(Vec::new())),
				PortSpec::input("format", "Format", SocketType::String)
					.with_default(SocketValue::String("pcm_s16le".into()))
					.with_closed_string_variants(["pcm_s16le", "pcm_f32le"]),
				PortSpec::input("sample_rate", "Sample Rate", SocketType::Int).with_default(SocketValue::Int(16000)),
				PortSpec::input("min_hz", "Min Hz", SocketType::Float).with_default(SocketValue::Float(70.0)),
				PortSpec::input("max_hz", "Max Hz", SocketType::Float).with_default(SocketValue::Float(1000.0)),
				PortSpec::input("voiced_threshold", "Voiced Threshold", SocketType::Float).with_default(SocketValue::Float(0.35)),
			],
			outputs: vec![
				PortSpec::output("frequency_hz", "Frequency Hz", SocketType::Float),
				PortSpec::output("confidence", "Confidence", SocketType::Float),
				PortSpec::output("voiced", "Voiced", SocketType::Bool),
				PortSpec::output("sample_count", "Sample Count", SocketType::Int),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl EffectfulNode for AudioPlayNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired_exec.contains("exec_in") {
			return Ok(NodeOutput::new());
		}

		let path = get_optional_string(inputs, "path", "")?;
		let mut bytes = if inputs.contains_key("bytes") {
			get_required_bytes(inputs, "bytes")?.to_vec()
		} else {
			Vec::new()
		};
		let source = if !bytes.is_empty() {
			"bytes".to_string()
		} else if !path.trim().is_empty() {
			let resolved = match ctx.resolve_read_path(path.trim()) {
				Ok(path) => path,
				Err(error) => return Ok(audio_play_error(error)),
			};
			match tokio::fs::read(resolved).await {
				Ok(read) => {
					bytes = read;
					path.trim().to_string()
				}
				Err(error) => return Ok(audio_play_error(format!("read audio file `{}`: {error}", path.trim()))),
			}
		} else {
			return Ok(audio_play_error("audio bytes or path is required"));
		};

		let bytes_len = bytes.len();
		let Some(sink) = ctx.audio_sink.as_ref() else {
			return Ok(audio_play_success(false, source, bytes_len));
		};

		let volume = get_optional_float(inputs, "volume", 1.0)?.clamp(0.0, 4.0) as f32;
		let speed = get_optional_float(inputs, "speed", 1.0)?.clamp(0.25, 4.0) as f32;
		let cursor = std::io::Cursor::new(bytes);
		let decoded = rodio::Decoder::new(cursor).map_err(|error| NodeExecError::Generic(anyhow::anyhow!("audio.play decode: {error}")))?;
		let mut sink = sink.lock().await;
		sink.ensure_device().map_err(|error| NodeExecError::Generic(anyhow::anyhow!("audio.play output: {error}")))?;
		sink.player.append(decoded.speed(speed).amplify(volume));
		Ok(audio_play_success(true, source, bytes_len))
	}
}

#[async_trait]
impl PureNode for AudioEnvelopeNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let samples = get_required_bytes(inputs, "samples")?;
		let format = PcmFormat::parse(&get_optional_string(inputs, "format", "pcm_s16le")?)?;
		let channels = get_optional_int(inputs, "channels", 1)?.max(1) as usize;
		let sample_rate = get_optional_int(inputs, "sample_rate", 16000)?.max(1) as f64;
		let envelope = analyze_pcm(samples, format)?;
		let frame_count = envelope.sample_count / channels;
		let duration_ms = frame_count as f64 / sample_rate * 1000.0;
		let dbfs = if envelope.rms > 0.0 {
			(20.0 * envelope.rms.log10()).max(SILENCE_DBFS)
		} else {
			SILENCE_DBFS
		};

		Ok(NodeOutput::new()
			.set_data("rms", SocketValue::Float(envelope.rms))
			.set_data("peak", SocketValue::Float(envelope.peak))
			.set_data("dbfs", SocketValue::Float(dbfs))
			.set_data("sample_count", SocketValue::Int(envelope.sample_count as i64))
			.set_data("frame_count", SocketValue::Int(frame_count as i64))
			.set_data("duration_ms", SocketValue::Float(duration_ms)))
	}
}

#[async_trait]
impl PureNode for AudioPitchNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let bytes = get_required_bytes(inputs, "samples")?;
		let format = PcmFormat::parse(&get_optional_string(inputs, "format", "pcm_s16le")?)?;
		let sample_rate = get_optional_int(inputs, "sample_rate", 16000)?.max(1) as f64;
		let min_hz = crate::flowgraph::node::get_optional_float(inputs, "min_hz", 70.0)?.max(1.0);
		let max_hz = crate::flowgraph::node::get_optional_float(inputs, "max_hz", 1000.0)?.max(min_hz + 1.0);
		let threshold = crate::flowgraph::node::get_optional_float(inputs, "voiced_threshold", 0.35)?.clamp(0.0, 1.0);
		let samples = decode_pcm(bytes, format)?;
		let (frequency_hz, confidence) = estimate_pitch(&samples, sample_rate, min_hz, max_hz);

		Ok(NodeOutput::new()
			.set_data("frequency_hz", SocketValue::Float(frequency_hz))
			.set_data("confidence", SocketValue::Float(confidence))
			.set_data("voiced", SocketValue::Bool(confidence >= threshold && frequency_hz > 0.0))
			.set_data("sample_count", SocketValue::Int(samples.len() as i64)))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn float_output(output: &NodeOutput, key: &str) -> f64 {
		match output.data.get(key) {
			Some(SocketValue::Float(value)) => *value,
			other => panic!("expected float output {key}, got {other:?}"),
		}
	}

	async fn run(samples: Vec<u8>, format: &str, channels: i64, sample_rate: i64) -> NodeOutput {
		let inputs: InputMap = [
			("samples".into(), SocketValue::Bytes(samples)),
			("format".into(), SocketValue::String(format.into())),
			("channels".into(), SocketValue::Int(channels)),
			("sample_rate".into(), SocketValue::Int(sample_rate)),
		]
		.into_iter()
		.collect();
		AudioEnvelopeNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap()
	}

	#[tokio::test]
	async fn audio_play_no_fire_is_noop() {
		let mut ctx = ExecCtx::default();
		let output = AudioPlayNode
			.execute(&mut ctx, &InputMap::new(), &InputMap::new(), &ExecFireSet::new())
			.await
			.unwrap();
		assert!(output.data.is_empty());
		assert!(output.fired_exec.is_empty());
	}

	#[tokio::test]
	async fn audio_play_requires_bytes_or_path() {
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		let mut ctx = ExecCtx::default();
		let output = AudioPlayNode
			.execute(&mut ctx, &InputMap::new(), &InputMap::new(), &fired)
			.await
			.unwrap();
		assert!(output.fired_exec.contains("on_error"));
		assert_eq!(output.data.get("played"), Some(&SocketValue::Bool(false)));
		match output.data.get("result").unwrap() {
			SocketValue::Result(result) => assert_eq!(result.code.as_deref(), Some("audio.play")),
			other => panic!("expected result, got {other:?}"),
		}
	}

	#[tokio::test]
	async fn audio_play_headless_bytes_reports_not_played_success() {
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		let inputs: InputMap = [("bytes".into(), SocketValue::Bytes(vec![0, 1, 2, 3]))].into_iter().collect();
		let mut ctx = ExecCtx::default();
		let output = AudioPlayNode.execute(&mut ctx, &InputMap::new(), &inputs, &fired).await.unwrap();
		assert!(output.fired_exec.contains("on_success"));
		assert_eq!(output.data.get("played"), Some(&SocketValue::Bool(false)));
		assert_eq!(output.data.get("source"), Some(&SocketValue::String("bytes".into())));
		assert_eq!(output.data.get("bytes_len"), Some(&SocketValue::Int(4)));
	}

	#[tokio::test]
	async fn audio_envelope_analyzes_s16le_pcm() {
		let mut bytes = Vec::new();
		for sample in [0i16, 32767, -32768] {
			bytes.extend_from_slice(&sample.to_le_bytes());
		}
		let output = run(bytes, "pcm_s16le", 1, 16000).await;
		let expected_rms = ((0.0f64.powi(2) + (32767.0 / 32768.0f64).powi(2) + 1.0) / 3.0).sqrt();
		assert!((float_output(&output, "rms") - expected_rms).abs() < 1e-12);
		assert_eq!(float_output(&output, "peak"), 1.0);
		assert_eq!(output.data.get("sample_count"), Some(&SocketValue::Int(3)));
		assert_eq!(output.data.get("frame_count"), Some(&SocketValue::Int(3)));
	}

	#[tokio::test]
	async fn audio_envelope_analyzes_f32le_pcm() {
		let mut bytes = Vec::new();
		for sample in [0.5f32, -0.25] {
			bytes.extend_from_slice(&sample.to_le_bytes());
		}
		let output = run(bytes, "pcm_f32le", 2, 48000).await;
		assert!((float_output(&output, "rms") - ((0.25f64 + 0.0625) / 2.0).sqrt()).abs() < 1e-12);
		assert_eq!(float_output(&output, "peak"), 0.5);
		assert_eq!(output.data.get("sample_count"), Some(&SocketValue::Int(2)));
		assert_eq!(output.data.get("frame_count"), Some(&SocketValue::Int(1)));
		assert!((float_output(&output, "duration_ms") - (1.0 / 48000.0 * 1000.0)).abs() < 1e-12);
	}

	#[tokio::test]
	async fn audio_envelope_empty_is_silence() {
		let output = run(Vec::new(), "pcm_s16le", 1, 16000).await;
		assert_eq!(float_output(&output, "rms"), 0.0);
		assert_eq!(float_output(&output, "peak"), 0.0);
		assert_eq!(float_output(&output, "dbfs"), SILENCE_DBFS);
		assert_eq!(output.data.get("sample_count"), Some(&SocketValue::Int(0)));
		assert_eq!(output.data.get("frame_count"), Some(&SocketValue::Int(0)));
		assert_eq!(float_output(&output, "duration_ms"), 0.0);
	}

	#[tokio::test]
	async fn audio_pitch_estimates_sine_frequency() {
		let sample_rate = 16_000.0;
		let frequency = 440.0;
		let mut bytes = Vec::new();
		for n in 0..1600 {
			let phase = 2.0 * std::f64::consts::PI * frequency * n as f64 / sample_rate;
			let sample = (phase.sin() * 0.7 * i16::MAX as f64) as i16;
			bytes.extend_from_slice(&sample.to_le_bytes());
		}
		let inputs: InputMap = [
			("samples".into(), SocketValue::Bytes(bytes)),
			("format".into(), SocketValue::String("pcm_s16le".into())),
			("sample_rate".into(), SocketValue::Int(sample_rate as i64)),
			("min_hz".into(), SocketValue::Float(100.0)),
			("max_hz".into(), SocketValue::Float(800.0)),
		]
		.into_iter()
		.collect();
		let output = AudioPitchNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		let observed = float_output(&output, "frequency_hz");
		let confidence = float_output(&output, "confidence");
		assert!(
			(observed - frequency).abs() < 10.0,
			"observed={observed}, expected={frequency}, confidence={confidence}"
		);
		assert!(confidence > 0.8);
		assert_eq!(output.data.get("voiced"), Some(&SocketValue::Bool(true)));
	}
}
