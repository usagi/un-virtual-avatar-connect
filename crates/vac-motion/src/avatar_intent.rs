//! AI avatar intent to UNMotionFrame mapper.
//!
//! UNVAC v2.0 keeps AI body control at an intent layer. The AI emits a small,
//! bounded command such as "concerned expression" or "nod once"; this module
//! turns that intent into one deterministic UNMF frame.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::f32::consts::TAU;
use un_motion_frame::{
	BodyMotion, BoneSample, CoordinateSpace, ExpressionSample, FaceMotion, Handedness, HumanoidBone, HumanoidPose, LengthUnit,
	MotionExtension, MotionHeader, MotionMetadata, MotionSourceInfo, MotionSourceKind, Quatf, SampleState, TrackingState, TransformSample,
	UNMotionFrame,
};

const DEFAULT_DURATION_MS: u64 = 700;
const MIN_DURATION_MS: u64 = 100;
const MAX_DURATION_MS: u64 = 5_000;
const MAX_INTENTS: usize = 3;
const MAX_HEAD_DEG: f32 = 20.0;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct AvatarIntentRequest {
	#[serde(default)]
	pub target_link_id: Option<String>,
	#[serde(default)]
	pub intents: Vec<AvatarIntent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AvatarIntent {
	Expression {
		name: String,
		#[serde(default)]
		intensity: Option<f32>,
		#[serde(default)]
		duration_ms: Option<u64>,
		#[serde(default)]
		curve: Option<AvatarCurve>,
	},
	HeadNod {
		#[serde(default)]
		count: Option<u8>,
		#[serde(default)]
		intensity: Option<f32>,
		#[serde(default)]
		duration_ms: Option<u64>,
		#[serde(default)]
		curve: Option<AvatarCurve>,
	},
	HeadShake {
		#[serde(default)]
		count: Option<u8>,
		#[serde(default)]
		intensity: Option<f32>,
		#[serde(default)]
		duration_ms: Option<u64>,
		#[serde(default)]
		curve: Option<AvatarCurve>,
	},
	LookAt {
		#[serde(default)]
		yaw_deg: Option<f32>,
		#[serde(default)]
		pitch_deg: Option<f32>,
		#[serde(default)]
		duration_ms: Option<u64>,
	},
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AvatarCurve {
	#[default]
	Linear,
	EaseOut,
	Hold,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AvatarIntentMapperOptions {
	#[serde(default)]
	pub sequence: u64,
	#[serde(default)]
	pub elapsed_ms: u64,
	#[serde(default = "default_producer")]
	pub producer: String,
}

impl Default for AvatarIntentMapperOptions {
	fn default() -> Self {
		Self {
			sequence: 0,
			elapsed_ms: 0,
			producer: default_producer(),
		}
	}
}

fn default_producer() -> String {
	"un-virtual-avatar-connect/avatar-intent".to_string()
}

pub fn avatar_intent_to_unmf(request: &AvatarIntentRequest, options: &AvatarIntentMapperOptions) -> Result<UNMotionFrame, String> {
	avatar_intent_to_unmf_with_bindings(request, options, &AvatarExpressionBindings::new())
}

/// Explicit model presets per semantic emotion. Weights multiply the requested intensity.
pub type AvatarExpressionBindings = BTreeMap<String, BTreeMap<String, f32>>;

/// Match the renderer's case-insensitive, punctuation-insensitive preset lookup.
pub fn same_preset_name(left: &str, right: &str) -> bool {
	let key = |name: &str| name.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect::<String>();
	let left_key = key(left);
	left.eq_ignore_ascii_case(right) || (!left_key.is_empty() && left_key == key(right))
}

pub fn validate_expression_bindings(bindings: &AvatarExpressionBindings) -> Result<(), String> {
	for (emotion, presets) in bindings {
		if !matches!(emotion.as_str(), "smile" | "laugh" | "annoyed" | "concerned" | "surprised") {
			return Err(format!("unknown expression binding emotion: {emotion}"));
		}
		if presets.is_empty() || presets.len() > 16 {
			return Err(format!("expression binding {emotion} requires 1..=16 presets"));
		}
		for (name, weight) in presets {
			if presets.keys().any(|other| other != name && same_preset_name(name, other)) {
				return Err(format!("expression binding {emotion} has ambiguous duplicate preset {name}"));
			}
			if name.trim().is_empty() || name != name.trim() || name.len() > 128 || name.chars().any(char::is_control) {
				return Err(format!("invalid expression preset name in {emotion}"));
			}
			if matches!(normalize_name(name).replace('_', "").as_str(), "jawopen" | "blink" | "blinkl" | "blinkr" | "eyeblinkleft" | "eyeblinkright" | "a" | "i" | "u" | "e" | "o") {
				return Err(format!("expression binding {emotion} must not own mouth/blink preset {name}"));
			}
			if !weight.is_finite() || !(0.0..=1.0).contains(weight) {
				return Err(format!("expression binding {emotion}/{name} weight must be finite and within 0..=1"));
			}
		}
	}
	Ok(())
}

fn expression_binding_key(name: &str) -> Option<&'static str> {
	if name.contains(':') { return None; }
	match normalize_name(name).as_str() {
		"joy" | "happy" | "smile" | "agree" | "pleased" => Some("smile"),
		"fun" | "laugh" | "playful" => Some("laugh"),
		"angry" | "stern" | "annoyed" => Some("annoyed"),
		"sorrow" | "sad" | "concerned" | "worried" => Some("concerned"),
		"surprised" | "surprise" | "startled" => Some("surprised"),
		_ => None,
	}
}

pub fn avatar_intent_to_unmf_with_bindings(
	request: &AvatarIntentRequest,
	options: &AvatarIntentMapperOptions,
	bindings: &AvatarExpressionBindings,
) -> Result<UNMotionFrame, String> {
	validate_request(request)?;
	validate_expression_bindings(bindings)?;

	let mut frame = UNMotionFrame::new(options.sequence);
	frame.header = MotionHeader::new(options.sequence);
	frame.header.coordinate_space = CoordinateSpace::UNMotion;
	frame.header.handedness = Handedness::RightHanded;
	frame.header.length_unit = LengthUnit::Meter;
	frame.sources.push(MotionSourceInfo {
		source_id: request.target_link_id.clone().unwrap_or_else(|| "avatar_intent".to_string()),
		source_kind: MotionSourceKind::Manual,
		display_name: request.target_link_id.clone(),
		confidence: 1.0,
		latency_ns: None,
		state: TrackingState::Valid,
	});
	frame.metadata = MotionMetadata {
		producer: Some(options.producer.clone()),
		extensions: vec![MotionExtension {
			namespace: "network.usagi.unvac.avatar_intent".to_string(),
			name: "intent_mapper".to_string(),
			value: "v2.0-first-slice".to_string(),
		}],
		..MotionMetadata::default()
	};

	let mut expressions = Vec::new();
	let mut pitch_deg = 0.0f32;
	let mut yaw_deg = 0.0f32;

	for intent in &request.intents {
		match intent {
			AvatarIntent::Expression {
				name,
				intensity,
				duration_ms,
				curve,
			} => {
				if is_neutral_expression(name) {
					expressions.extend(neutral_expression_samples());
					for preset in bindings.values().flat_map(|presets| presets.keys()) {
						if !expressions.iter().any(|sample| same_preset_name(&sample.name, preset)) {
							expressions.push(ExpressionSample {
								name: preset.clone(), value: 0.0, confidence: 1.0,
								source_index: Some(0), state: SampleState::Valid,
							});
						}
					}
					continue;
				}
				let Some(expr_names) = map_expression_names(name) else {
					return Err(format!("unknown avatar expression: {name}"));
				};
				let duration = clamp_duration(duration_ms.unwrap_or(DEFAULT_DURATION_MS));
				let progress = progress(options.elapsed_ms, duration);
				let value = if options.elapsed_ms < duration {
					clamp01(intensity.unwrap_or(0.5)) * curve.unwrap_or_default().sample_decay(progress)
				} else {
					0.0
				};
				// 疎な表情更新では省略した値がRendererに残るため、終了時も明示的に0を送る。
				let presets = expression_binding_key(name).and_then(|key| bindings.get(key));
				let weighted_names: Vec<_> = match presets {
					Some(presets) => presets.iter().map(|(name, weight)| (name.clone(), *weight)).collect(),
					None => expr_names.into_iter().map(|name| (name, 1.0)).collect(),
				};
				expressions.extend(weighted_names.into_iter().map(|(name, weight)| ExpressionSample {
					name,
					value: value * weight,
					confidence: 1.0,
					source_index: Some(0),
					state: SampleState::Valid,
				}));
			}
			AvatarIntent::HeadNod {
				count,
				intensity,
				duration_ms,
				curve,
			} => {
				let duration = clamp_duration(duration_ms.unwrap_or(DEFAULT_DURATION_MS));
				let progress = progress(options.elapsed_ms, duration);
				if progress <= 1.0 {
					let count = count.unwrap_or(1).clamp(1, 3) as f32;
					let phase = curve.unwrap_or(AvatarCurve::EaseOut).sample_motion_phase(progress);
					let amp = clamp01(intensity.unwrap_or(0.35)) * MAX_HEAD_DEG;
					// UNMotion is right-handed, Y-up, +Z-forward: positive X pitch lowers the face.
					pitch_deg += 0.5 * (1.0 - (TAU * count * phase).cos()) * amp;
				}
			}
			AvatarIntent::HeadShake {
				count,
				intensity,
				duration_ms,
				curve,
			} => {
				let duration = clamp_duration(duration_ms.unwrap_or(DEFAULT_DURATION_MS));
				let progress = progress(options.elapsed_ms, duration);
				if progress <= 1.0 {
					let count = count.unwrap_or(1).clamp(1, 3) as f32;
					let phase = curve.unwrap_or(AvatarCurve::EaseOut).sample_motion_phase(progress);
					let amp = clamp01(intensity.unwrap_or(0.35)) * MAX_HEAD_DEG;
					yaw_deg += (TAU * count * phase).sin() * amp;
				}
			}
			AvatarIntent::LookAt {
				yaw_deg: yaw,
				pitch_deg: pitch,
				duration_ms,
			} => {
				let duration = clamp_duration(duration_ms.unwrap_or(DEFAULT_DURATION_MS));
				let progress = progress(options.elapsed_ms, duration);
				if progress <= 1.0 {
					let envelope = pose_hold_envelope(progress);
					yaw_deg += yaw.unwrap_or(0.0).clamp(-MAX_HEAD_DEG, MAX_HEAD_DEG) * envelope;
					pitch_deg += pitch.unwrap_or(0.0).clamp(-MAX_HEAD_DEG, MAX_HEAD_DEG) * envelope;
				}
			}
		}
	}

	// A new semantic layer replaces the old one. Reset omitted presets in the same
	// frame so a sparse Renderer does not retain the previous emotion until release.
	let mut reset = neutral_expression_samples();
	reset.extend(bindings.values().flat_map(|presets| presets.keys()).map(|name| ExpressionSample {
		name: name.clone(), value: 0.0, confidence: 1.0, source_index: Some(0), state: SampleState::Valid,
	}));
	for sample in reset {
		if !expressions.iter().any(|current| same_preset_name(&current.name, &sample.name)) {
			expressions.push(sample);
		}
	}
	if !expressions.is_empty() {
		frame.face = Some(FaceMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			head: None,
			expressions,
		});
	}

	pitch_deg = pitch_deg.clamp(-MAX_HEAD_DEG, MAX_HEAD_DEG);
	yaw_deg = yaw_deg.clamp(-MAX_HEAD_DEG, MAX_HEAD_DEG);
	if pitch_deg != 0.0 || yaw_deg != 0.0 {
		frame.body = Some(BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: None,
				bones: vec![BoneSample {
					bone: HumanoidBone::Head,
					transform: TransformSample {
						translation: None,
						rotation: Some(quat_from_pitch_yaw(pitch_deg.to_radians(), yaw_deg.to_radians())),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					confidence: 1.0,
					source_index: Some(0),
					state: SampleState::Valid,
				}],
			}),
		});
	}

	Ok(frame)
}

/// Intent群を再生する必要がある最長時間。
pub fn avatar_intent_duration_ms(request: &AvatarIntentRequest) -> Result<u64, String> {
	validate_request(request)?;
	Ok(request
		.intents
		.iter()
		.map(|intent| match intent {
			AvatarIntent::Expression { duration_ms, .. }
			| AvatarIntent::HeadNod { duration_ms, .. }
			| AvatarIntent::HeadShake { duration_ms, .. }
			| AvatarIntent::LookAt { duration_ms, .. } => clamp_duration(duration_ms.unwrap_or(DEFAULT_DURATION_MS)),
		})
		.max()
		.unwrap_or(0))
}

/// Intent再生後に表情と頭部姿勢を確実に戻す終端フレーム。
pub fn avatar_intent_neutral_unmf(request: &AvatarIntentRequest, sequence: u64) -> Result<UNMotionFrame, String> {
	avatar_intent_neutral_unmf_with_bindings(request, sequence, &AvatarExpressionBindings::new())
}

pub fn avatar_intent_neutral_unmf_with_bindings(
	request: &AvatarIntentRequest,
	sequence: u64,
	bindings: &AvatarExpressionBindings,
) -> Result<UNMotionFrame, String> {
	validate_request(request)?;
	let neutral = AvatarIntentRequest {
		target_link_id: request.target_link_id.clone(),
		intents: vec![AvatarIntent::Expression {
			name: "neutral".into(),
			intensity: Some(1.0),
			duration_ms: Some(MIN_DURATION_MS),
			curve: Some(AvatarCurve::Linear),
		}],
	};
	let mut frame = avatar_intent_to_unmf_with_bindings(
		&neutral,
		&AvatarIntentMapperOptions {
			sequence,
			..AvatarIntentMapperOptions::default()
		},
		bindings,
	)?;
	if request.intents.iter().any(|intent| {
		matches!(
			intent,
			AvatarIntent::HeadNod { .. } | AvatarIntent::HeadShake { .. } | AvatarIntent::LookAt { .. }
		)
	}) {
		frame.body = Some(BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: None,
				bones: vec![BoneSample {
					bone: HumanoidBone::Head,
					transform: TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: 0.0,
							y: 0.0,
							z: 0.0,
							w: 1.0,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					confidence: 1.0,
					source_index: Some(0),
					state: SampleState::Valid,
				}],
			}),
		});
	}
	Ok(frame)
}

fn validate_request(request: &AvatarIntentRequest) -> Result<(), String> {
	if request.intents.len() > MAX_INTENTS {
		return Err(format!("too many avatar intents: {} > {MAX_INTENTS}", request.intents.len()));
	}
	for intent in &request.intents {
		match intent {
			AvatarIntent::Expression {
				intensity, duration_ms, ..
			}
			| AvatarIntent::HeadNod {
				intensity, duration_ms, ..
			}
			| AvatarIntent::HeadShake {
				intensity, duration_ms, ..
			} => {
				if let Some(v) = intensity {
					if !v.is_finite() || !(0.0..=1.0).contains(v) {
						return Err("avatar intent intensity must be finite and within 0..1".to_string());
					}
				}
				validate_duration(*duration_ms)?;
			}
			AvatarIntent::LookAt {
				yaw_deg,
				pitch_deg,
				duration_ms,
			} => {
				for v in [yaw_deg, pitch_deg].into_iter().flatten() {
					if !v.is_finite() {
						return Err("look_at degrees must be finite".to_string());
					}
				}
				validate_duration(*duration_ms)?;
			}
		}
	}
	Ok(())
}

fn validate_duration(duration_ms: Option<u64>) -> Result<(), String> {
	if let Some(ms) = duration_ms {
		if !(MIN_DURATION_MS..=MAX_DURATION_MS).contains(&ms) {
			return Err(format!("avatar intent duration_ms must be {MIN_DURATION_MS}..{MAX_DURATION_MS}"));
		}
	}
	Ok(())
}

fn clamp_duration(ms: u64) -> u64 {
	ms.clamp(MIN_DURATION_MS, MAX_DURATION_MS)
}

fn clamp01(v: f32) -> f32 {
	if v.is_finite() {
		v.clamp(0.0, 1.0)
	} else {
		0.0
	}
}

fn progress(elapsed_ms: u64, duration_ms: u64) -> f32 {
	elapsed_ms as f32 / duration_ms.max(1) as f32
}

impl AvatarCurve {
	fn sample_decay(self, progress: f32) -> f32 {
		let t = progress.clamp(0.0, 1.0);
		match self {
			AvatarCurve::Linear => 1.0 - t,
			AvatarCurve::EaseOut => {
				let inv = 1.0 - t;
				inv * inv
			}
			AvatarCurve::Hold => 1.0,
		}
	}

	fn sample_motion_phase(self, progress: f32) -> f32 {
		let t = progress.clamp(0.0, 1.0);
		match self {
			AvatarCurve::EaseOut => smoothstep(t),
			AvatarCurve::Linear | AvatarCurve::Hold => t,
		}
	}
}

fn smoothstep(value: f32) -> f32 {
	let t = value.clamp(0.0, 1.0);
	t * t * (3.0 - 2.0 * t)
}

fn pose_hold_envelope(progress: f32) -> f32 {
	const ATTACK_END: f32 = 0.15;
	const RELEASE_START: f32 = 0.80;
	let t = progress.clamp(0.0, 1.0);
	if t < ATTACK_END {
		smoothstep(t / ATTACK_END)
	} else if t > RELEASE_START {
		1.0 - smoothstep((t - RELEASE_START) / (1.0 - RELEASE_START))
	} else {
		1.0
	}
}

fn map_expression_names(name: &str) -> Option<Vec<String>> {
	let n = name.trim();
	if n.is_empty() {
		return None;
	}
	if n.contains(':') {
		return Some(vec![n.to_string()]);
	}
	let mapped: &[&str] = match normalize_name(n).as_str() {
		"joy" | "happy" | "smile" | "agree" | "pleased" => &["Joy", "MouthSmileLeft", "MouthSmileRight"],
		"fun" | "laugh" | "playful" => &["Fun", "MouthSmileLeft", "MouthSmileRight", "JawOpen"],
		"angry" | "stern" | "annoyed" => &["Angry", "BrowDownLeft", "BrowDownRight"],
		"sorrow" | "sad" | "concerned" | "worried" => &["Sorrow", "BrowInnerUp"],
		"surprised" | "surprise" | "startled" => &["Surprised", "BrowInnerUp", "EyeWideLeft", "EyeWideRight", "JawOpen"],
		"blink" => &["Blink", "EyeBlinkLeft", "EyeBlinkRight"],
		"blinkleft" | "blink_left" => &["Blink_L", "EyeBlinkLeft"],
		"blinkright" | "blink_right" => &["Blink_R", "EyeBlinkRight"],
		"a" | "aa" => &["A", "JawOpen"],
		"i" | "ih" => &["I", "MouthSmileLeft", "MouthSmileRight"],
		"u" | "ou" => &["U", "MouthPucker"],
		"e" | "eh" => &["E", "MouthStretchLeft", "MouthStretchRight"],
		"o" | "oh" => &["O", "MouthFunnel"],
		_ => return None,
	};
	Some(mapped.iter().map(|name| (*name).to_string()).collect())
}

fn is_neutral_expression(name: &str) -> bool {
	!name.contains(':') && normalize_name(name) == "neutral"
}

fn neutral_expression_samples() -> Vec<ExpressionSample> {
	[
		"Joy",
		"Fun",
		"Angry",
		"Sorrow",
		"Surprised",
		"Blink",
		"Blink_L",
		"Blink_R",
		"A",
		"I",
		"U",
		"E",
		"O",
		"MouthSmileLeft",
		"MouthSmileRight",
		"JawOpen",
		"BrowDownLeft",
		"BrowDownRight",
		"BrowInnerUp",
		"EyeWideLeft",
		"EyeWideRight",
		"EyeBlinkLeft",
		"EyeBlinkRight",
		"MouthPucker",
		"MouthStretchLeft",
		"MouthStretchRight",
		"MouthFunnel",
	]
	.into_iter()
	.map(|name| ExpressionSample {
		name: name.to_string(),
		value: 0.0,
		confidence: 1.0,
		source_index: Some(0),
		state: SampleState::Valid,
	})
	.collect()
}

fn normalize_name(name: &str) -> String {
	name.chars()
		.filter(|c| c.is_ascii_alphanumeric() || *c == '_')
		.flat_map(char::to_lowercase)
		.collect()
}

fn quat_from_pitch_yaw(pitch: f32, yaw: f32) -> Quatf {
	let (sx, cx) = (pitch * 0.5).sin_cos();
	let (sy, cy) = (yaw * 0.5).sin_cos();
	let (sz, cz) = (0.0f32, 1.0f32);
	Quatf {
		x: sx * cy * cz + cx * sy * sz,
		y: cx * sy * cz - sx * cy * sz,
		z: cx * cy * sz + sx * sy * cz,
		w: cx * cy * cz - sx * sy * sz,
	}
}

#[cfg(test)]
mod tests {
	#[test]
	fn native_speech_review_fixture_is_valid() {
		let request: super::AvatarIntentRequest = serde_json::from_str(include_str!("../tests/fixtures/speech-review-smile.json")).unwrap();
		assert_eq!(super::avatar_intent_duration_ms(&request).unwrap(), 4_500);
		super::avatar_intent_to_unmf(&request, &super::AvatarIntentMapperOptions::default()).unwrap();
		super::avatar_intent_neutral_unmf(&request, 1).unwrap();
	}
	use super::*;

	#[test]
	fn default_expressions_reset_when_replaced_by_another_emotion_or_head_only() {
		// Model-specific bindings are optional. The receiver retains omitted values.
		let mut receiver = BTreeMap::new();
		for (intent, expected_joy, expected_sorrow) in [
			(serde_json::json!({"kind":"expression","name":"happy","intensity":0.6,"curve":"hold"}), 0.6, 0.0),
			(serde_json::json!({"kind":"expression","name":"concerned","intensity":0.4,"curve":"hold"}), 0.0, 0.4),
			(serde_json::json!({"kind":"head_nod","intensity":0.5}), 0.0, 0.0),
		] {
			let request: AvatarIntentRequest = serde_json::from_value(serde_json::json!({"intents":[intent]})).unwrap();
			let frame = avatar_intent_to_unmf(&request, &AvatarIntentMapperOptions::default()).unwrap();
			if let Some(face) = frame.face {
				for sample in face.expressions {
					receiver.insert(sample.name, sample.value);
				}
			}
			assert_eq!(receiver.get("Joy").copied().unwrap_or(0.0), expected_joy, "previous happiness must not survive replacement");
			assert_eq!(receiver.get("Sorrow").copied().unwrap_or(0.0), expected_sorrow, "previous concern must not survive replacement");
		}
	}

	#[test]
	fn expression_binding_reset_does_not_shadow_a_renderer_name_alias() {
		let bindings = AvatarExpressionBindings::from([("smile".into(), BTreeMap::from([("mouth_smile_left".into(), 1.0)]))]);
		let request: AvatarIntentRequest = serde_json::from_value(serde_json::json!({"intents":[
			{"kind":"expression","name":"smile","intensity":0.6,"duration_ms":800,"curve":"hold"}
		]})).unwrap();
		let frame = avatar_intent_to_unmf_with_bindings(&request, &AvatarIntentMapperOptions::default(), &bindings).unwrap();
		let face = frame.face.unwrap();
		let samples: Vec<_> = face.expressions.iter().filter(|sample| same_preset_name(&sample.name, "MouthSmileLeft")).collect();
		assert_eq!(samples.len(), 1);
		assert_eq!(samples[0].value, 0.6);
		let ambiguous = AvatarExpressionBindings::from([("smile".into(), BTreeMap::from([
			("happy".into(), 1.0), ("Happy".into(), 0.5),
		]))]);
		assert!(validate_expression_bindings(&ambiguous).is_err());
	}

	#[test]
	fn model_bindings_weight_aliases_and_reset_on_switch_and_release() {
		let bindings = AvatarExpressionBindings::from([
			("smile".into(), BTreeMap::from([("modelHappy".into(), 0.5)])),
			("concerned".into(), BTreeMap::from([("modelSad".into(), 1.0)])),
		]);
		let request = |name: &str| AvatarIntentRequest {
			target_link_id: Some("model-one".into()),
			intents: vec![AvatarIntent::Expression {
				name: name.into(), intensity: Some(0.6), duration_ms: Some(800), curve: Some(AvatarCurve::Hold),
			}],
		};
		let weights = |frame: UNMotionFrame| frame.face.unwrap().expressions.into_iter()
			.map(|sample| (sample.name, sample.value)).collect::<BTreeMap<_, _>>();
		let options = AvatarIntentMapperOptions::default();
		let happy = weights(avatar_intent_to_unmf_with_bindings(&request("pleased"), &options, &bindings).unwrap());
		assert!((happy["modelHappy"] - 0.3).abs() < 1e-6);
		assert_eq!(happy["modelSad"], 0.0);
		assert_eq!(happy["MouthSmileLeft"], 0.0);
		let sad = weights(avatar_intent_to_unmf_with_bindings(&request("worried"), &options, &bindings).unwrap());
		assert_eq!(sad["modelHappy"], 0.0);
		assert!((sad["modelSad"] - 0.6).abs() < 1e-6);
		let expired = weights(avatar_intent_to_unmf_with_bindings(&request("smile"),
			&AvatarIntentMapperOptions { elapsed_ms: 800, ..Default::default() }, &bindings).unwrap());
		assert_eq!(expired["modelHappy"], 0.0);
		let neutral = weights(avatar_intent_neutral_unmf_with_bindings(&request("smile"), 42, &bindings).unwrap());
		assert!(neutral.values().all(|value| *value == 0.0));
		assert!(neutral.contains_key("modelHappy") && neutral.contains_key("modelSad"));
		let default = weights(avatar_intent_to_unmf(&request("smile"), &options).unwrap());
		assert!(!default.contains_key("modelHappy"));
		assert!((default["MouthSmileLeft"] - 0.6).abs() < 1e-6);
	}

	#[test]
	fn model_bindings_reject_protected_names_and_invalid_weights() {
		for (name, weight) in [("JawOpen", 1.0), ("EyeBlinkLeft", 1.0), ("Blink_L", 1.0),
			("happy", f32::NAN), ("happy", 1.1), ("happy", -0.1), (" happy", 0.5)] {
			let bindings = AvatarExpressionBindings::from([("smile".into(), BTreeMap::from([(name.into(), weight)]))]);
			assert!(validate_expression_bindings(&bindings).is_err(), "{name} {weight}");
		}
		assert!(validate_expression_bindings(&AvatarExpressionBindings::from([("neutral".into(),
			BTreeMap::from([("happy".into(), 1.0)]))])).is_err());
	}

	#[test]
	fn maps_expression_alias_to_vmc_blendshape() {
		let request = AvatarIntentRequest {
			target_link_id: Some("kaltsit_ai".into()),
			intents: vec![AvatarIntent::Expression {
				name: "concerned".into(),
				intensity: Some(0.45),
				duration_ms: Some(1200),
				curve: Some(AvatarCurve::Hold),
			}],
		};
		let frame = avatar_intent_to_unmf(&request, &AvatarIntentMapperOptions::default()).unwrap();
		let expressions = frame.face.unwrap().expressions;
		assert!(expressions.iter().any(|expr| expr.name == "Sorrow" && expr.value == 0.45));
		assert!(expressions.iter().any(|expr| expr.name == "BrowInnerUp" && expr.value == 0.45));
	}

	#[test]
	fn maps_neutral_to_zero_vmc_blendshapes() {
		let request = AvatarIntentRequest {
			target_link_id: Some("kaltsit_ai".into()),
			intents: vec![AvatarIntent::Expression {
				name: "neutral".into(),
				intensity: Some(1.0),
				duration_ms: Some(300),
				curve: Some(AvatarCurve::Linear),
			}],
		};
		let frame = avatar_intent_to_unmf(&request, &AvatarIntentMapperOptions::default()).unwrap();
		let expressions = frame.face.unwrap().expressions;
		assert!(expressions.iter().any(|expr| expr.name == "Joy" && expr.value == 0.0));
		assert!(expressions.iter().any(|expr| expr.name == "Sorrow" && expr.value == 0.0));
		assert!(expressions.iter().any(|expr| expr.name == "Surprised" && expr.value == 0.0));
		assert!(expressions.iter().any(|expr| expr.name == "MouthSmileLeft" && expr.value == 0.0));
	}

	#[test]
	fn maps_head_nod_to_head_bone_rotation() {
		let request = AvatarIntentRequest {
			target_link_id: None,
			intents: vec![AvatarIntent::HeadNod {
				count: Some(1),
				intensity: Some(0.5),
				duration_ms: Some(1000),
				curve: Some(AvatarCurve::Hold),
			}],
		};
		let frame = avatar_intent_to_unmf(
			&request,
			&AvatarIntentMapperOptions {
				elapsed_ms: 500,
				..AvatarIntentMapperOptions::default()
			},
		)
		.unwrap();
		assert_eq!(frame.header.coordinate_space, CoordinateSpace::UNMotion);
		assert_eq!(frame.header.handedness, Handedness::RightHanded);
		let bone = &frame.body.unwrap().humanoid.unwrap().bones[0];
		assert_eq!(bone.bone, HumanoidBone::Head);
		let rotation = bone.transform.rotation.unwrap();
		// Rotate the subject's +Z forward vector. A nod must point below the horizon.
		let forward_y = 2.0 * (rotation.y * rotation.z - rotation.w * rotation.x);
		assert!(forward_y < -0.1, "nod must lower the face, got forward.y={forward_y}");
	}

	#[test]
	fn short_expression_resets_while_longer_head_motion_continues() {
		let request: AvatarIntentRequest = serde_json::from_value(serde_json::json!({
			"intents": [
				{"kind":"expression", "name":"happy", "intensity":0.6, "duration_ms":200, "curve":"hold"},
				{"kind":"head_nod", "intensity":0.5, "duration_ms":1000, "curve":"linear"}
			]
		}))
		.unwrap();
		// Rendererは受信した表情だけを更新する。省略では以前の値が残る。
		let mut joy = 0.0;
		for elapsed_ms in [0, 200, 201, 500] {
			let frame = avatar_intent_to_unmf(
				&request,
				&AvatarIntentMapperOptions {
					elapsed_ms,
					..Default::default()
				},
			)
			.unwrap();
			if let Some(sample) = frame
				.face
				.as_ref()
				.and_then(|face| face.expressions.iter().find(|sample| sample.name == "Joy"))
			{
				joy = sample.value;
			}
			if elapsed_ms == 0 {
				assert_eq!(joy, 0.6);
			} else {
				assert_eq!(joy, 0.0, "expired expression remained at {elapsed_ms} ms");
			}
			if elapsed_ms == 500 {
				let head = &frame.body.unwrap().humanoid.unwrap().bones[0];
				assert!(head.transform.rotation.unwrap().x.abs() > 0.01);
			}
		}
	}

	#[test]
	fn nod_count_repeats_down_and_return_cycles() {
		let request = AvatarIntentRequest {
			target_link_id: None,
			intents: vec![AvatarIntent::HeadNod {
				count: Some(2),
				intensity: Some(0.5),
				duration_ms: Some(1000),
				curve: Some(AvatarCurve::Linear),
			}],
		};
		for elapsed_ms in [250, 750] {
			let frame = avatar_intent_to_unmf(
				&request,
				&AvatarIntentMapperOptions {
					elapsed_ms,
					..AvatarIntentMapperOptions::default()
				},
			)
			.unwrap();
			let rotation = frame.body.unwrap().humanoid.unwrap().bones[0].transform.rotation.unwrap();
			assert!(rotation.x > 0.08, "elapsed_ms={elapsed_ms} rotation={rotation:?}");
		}
		let center = avatar_intent_to_unmf(
			&request,
			&AvatarIntentMapperOptions {
				elapsed_ms: 500,
				..AvatarIntentMapperOptions::default()
			},
		)
		.unwrap();
		assert!(center.body.is_none());
	}

	#[test]
	fn eased_nod_keeps_full_requested_amplitude() {
		let request = AvatarIntentRequest {
			target_link_id: None,
			intents: vec![AvatarIntent::HeadNod {
				count: Some(1),
				intensity: Some(0.5),
				duration_ms: Some(1000),
				curve: Some(AvatarCurve::EaseOut),
			}],
		};
		let frame = avatar_intent_to_unmf(
			&request,
			&AvatarIntentMapperOptions {
				elapsed_ms: 500,
				..AvatarIntentMapperOptions::default()
			},
		)
		.unwrap();
		let rotation = frame.body.unwrap().humanoid.unwrap().bones[0].transform.rotation.unwrap();
		assert!(rotation.x > 0.08, "rotation={rotation:?}");
	}

	#[test]
	fn look_at_smoothly_enters_holds_and_releases() {
		let request = AvatarIntentRequest {
			target_link_id: None,
			intents: vec![AvatarIntent::LookAt {
				yaw_deg: Some(20.0),
				pitch_deg: Some(0.0),
				duration_ms: Some(2000),
			}],
		};
		let yaw_rotation = |elapsed_ms| {
			avatar_intent_to_unmf(
				&request,
				&AvatarIntentMapperOptions {
					elapsed_ms,
					..AvatarIntentMapperOptions::default()
				},
			)
			.unwrap()
			.body
			.and_then(|body| body.humanoid)
			.and_then(|pose| pose.bones[0].transform.rotation)
		};
		assert!(yaw_rotation(0).is_none());
		let entering = yaw_rotation(150).unwrap();
		let held = yaw_rotation(1000).unwrap();
		let releasing = yaw_rotation(1800).unwrap();
		assert!(entering.y > 0.07 && entering.y < held.y);
		assert!(held.y > 0.17);
		assert!(releasing.y > 0.07 && releasing.y < held.y);
		assert!(yaw_rotation(2000).is_none());
	}

	#[test]
	fn reports_longest_intent_duration_and_builds_head_neutral_frame() {
		let request = AvatarIntentRequest {
			target_link_id: Some("kaltsit_ai".into()),
			intents: vec![
				AvatarIntent::Expression {
					name: "concerned".into(),
					intensity: Some(0.4),
					duration_ms: Some(1200),
					curve: Some(AvatarCurve::Hold),
				},
				AvatarIntent::HeadShake {
					count: Some(1),
					intensity: Some(0.25),
					duration_ms: Some(800),
					curve: Some(AvatarCurve::EaseOut),
				},
			],
		};
		assert_eq!(avatar_intent_duration_ms(&request).unwrap(), 1200);
		let frame = avatar_intent_neutral_unmf(&request, 42).unwrap();
		assert_eq!(frame.header.sequence, 42);
		let head = &frame.body.unwrap().humanoid.unwrap().bones[0];
		assert_eq!(head.bone, HumanoidBone::Head);
		assert_eq!(head.transform.rotation.unwrap().w, 1.0);
		assert!(frame.face.unwrap().expressions.iter().all(|sample| sample.value == 0.0));
	}

	#[test]
	fn rejects_unsafe_intents() {
		let request = AvatarIntentRequest {
			target_link_id: None,
			intents: vec![AvatarIntent::Expression {
				name: "concerned".into(),
				intensity: Some(1.5),
				duration_ms: Some(1200),
				curve: None,
			}],
		};
		assert!(avatar_intent_to_unmf(&request, &AvatarIntentMapperOptions::default()).is_err());
	}
}
