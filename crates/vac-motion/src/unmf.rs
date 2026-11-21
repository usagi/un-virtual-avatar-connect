//! UNMotionFrame adapter layer.
//!
//! UNVAC keeps VMC as a compatibility wire format. The internal motion data
//! shape for new graph work is UNMotionFrame, and UNMF/Z is the transport
//! envelope for exchanging it with UNA / UNM and other U.N. applications.

use crate::vmc::{self, VMC_EXT_BLEND_VAL, VMC_EXT_BONE_POS, VMC_EXT_ROOT_POS};
use crate::{MotionFrame, OscMessageWire};
use serde_json::{json, Value as JsonValue};
use un_motion_frame::{
	BodyMotion, BoneSample, CoordinateSpace, ExpressionSample, FaceMotion, Handedness, HumanoidBone, HumanoidPose, LengthUnit,
	MotionExtension, MotionHeader, MotionMetadata, MotionSourceInfo, MotionSourceKind, Quatf, SampleState, TrackingState, TransformSample,
	UNMotionFrame, Vec3f,
};

pub use un_motion_frame::UNMotionFrame as UnMotionFrame;

pub const UNVAC_UNMF_ADAPTER_NAMESPACE: &str = "network.usagi.unvac.motion";

pub fn vmc_motion_frame_to_unmf(frame: &MotionFrame, sequence: u64) -> UNMotionFrame {
	let mut out = UNMotionFrame::new(sequence);
	out.header = MotionHeader::new(sequence);
	out.header.coordinate_space = CoordinateSpace::Vmc;
	out.header.handedness = Handedness::Unknown;
	out.header.length_unit = LengthUnit::Meter;
	out.sources.push(MotionSourceInfo {
		source_id: "vmc".to_string(),
		source_kind: MotionSourceKind::VmcInput,
		display_name: Some("VMC OSC".to_string()),
		confidence: 1.0,
		latency_ns: None,
		state: TrackingState::Valid,
	});
	out.metadata = MotionMetadata {
		producer: Some("un-virtual-avatar-connect/vmc-adapter".to_string()),
		extensions: vec![MotionExtension {
			namespace: UNVAC_UNMF_ADAPTER_NAMESPACE.to_string(),
			name: "source_format".to_string(),
			value: "vmc_osc_motion_frame".to_string(),
		}],
		..MotionMetadata::default()
	};

	let mut humanoid = HumanoidPose {
		root: None,
		bones: Vec::new(),
	};
	let mut expressions = Vec::new();
	let mut skipped_bones = Vec::new();

	for msg in &frame.osc_messages {
		if let Some(root) = vmc::try_parse_ext_pos_message(msg, VMC_EXT_ROOT_POS) {
			humanoid.root = Some(transform_from_vmc(&root.position, &root.rotation));
			continue;
		}
		if let Some(sample) = vmc::try_parse_ext_pos_message(msg, VMC_EXT_BONE_POS) {
			if let Some(bone) = vmc_bone_name_to_unmf(&sample.bone) {
				humanoid.bones.push(BoneSample {
					bone,
					transform: transform_from_vmc(&sample.position, &sample.rotation),
					confidence: 1.0,
					source_index: Some(0),
					state: SampleState::Valid,
				});
			} else {
				skipped_bones.push(sample.bone);
			}
			continue;
		}
		if let Some(sample) = vmc::try_parse_blend_val_message(msg) {
			expressions.push(ExpressionSample {
				name: vmc_expression_name_to_unmf(&sample.name),
				value: sample.value as f32,
				confidence: 1.0,
				source_index: Some(0),
				state: SampleState::Valid,
			});
		}
	}

	if humanoid.root.is_some() || !humanoid.bones.is_empty() {
		out.body = Some(BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(humanoid),
		});
	}
	if !expressions.is_empty() {
		out.face = Some(FaceMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			head: None,
			expressions,
		});
	}
	if !skipped_bones.is_empty() {
		out.metadata
			.notes
			.push(format!("Skipped unknown VMC bones: {}", skipped_bones.join(", ")));
	}
	out
}

pub fn unmf_to_vmc_motion_frame(frame: &UNMotionFrame) -> MotionFrame {
	let mut osc_messages = Vec::new();
	if let Some(body) = &frame.body {
		if let Some(humanoid) = &body.humanoid {
			if let Some(root) = &humanoid.root {
				if let Some(msg) = transform_to_vmc_message(VMC_EXT_ROOT_POS, "root", root) {
					osc_messages.push(msg);
				}
			}
			for bone in &humanoid.bones {
				if let Some(name) = unmf_bone_to_vmc_name(bone.bone) {
					if let Some(msg) = transform_to_vmc_message(VMC_EXT_BONE_POS, name, &bone.transform) {
						osc_messages.push(msg);
					}
				}
			}
		}
	}
	if let Some(face) = &frame.face {
		for expr in &face.expressions {
			osc_messages.push(OscMessageWire {
				address: VMC_EXT_BLEND_VAL.to_string(),
				args: vec![JsonValue::String(unmf_expression_name_to_vmc(&expr.name)), json!(expr.value)],
			});
		}
	}
	MotionFrame { byte_len: 0, osc_messages }
}

pub fn unmf_to_json_value(frame: &UNMotionFrame) -> JsonValue {
	serde_json::to_value(frame).unwrap_or(JsonValue::Null)
}

pub fn unmf_from_json_value(value: &JsonValue) -> Result<UNMotionFrame, String> {
	serde_json::from_value(value.clone()).map_err(|e| format!("UNMF JSON decode: {e}"))
}

pub fn encode_unmfz_payload(frame: &UNMotionFrame) -> Result<Vec<u8>, String> {
	un_motion_frame_zenoh::encode_frame(frame).map_err(|e| format!("UNMF/Z encode: {e}"))
}

pub fn decode_unmfz_payload(payload: &[u8]) -> Result<UNMotionFrame, String> {
	un_motion_frame_zenoh::decode_frame(payload).map_err(|e| format!("UNMF/Z decode: {e}"))
}

pub fn filter_unmf(frame: &UNMotionFrame, bone_names_csv: &str, expression_name_contains: &str) -> UNMotionFrame {
	let mut out = frame.clone();
	let bone_filter = parse_bone_filter(bone_names_csv);
	if let Some(body) = out.body.as_mut() {
		if let Some(humanoid) = body.humanoid.as_mut() {
			if let Some(filter) = bone_filter.as_ref() {
				humanoid.bones.retain(|sample| filter.contains(&sample.bone));
			}
			if humanoid.root.is_none() && humanoid.bones.is_empty() {
				body.humanoid = None;
			}
		}
		if body.humanoid.is_none() {
			out.body = None;
		}
	}
	let expr_filter = expression_name_contains.trim();
	if let Some(face) = out.face.as_mut() {
		if !expr_filter.is_empty() {
			face.expressions.retain(|sample| sample.name.contains(expr_filter));
		}
		if face.head.is_none() && face.expressions.is_empty() {
			out.face = None;
		}
	}
	out
}

pub fn map_unmf(
	frame: &UNMotionFrame,
	translation_scale: f64,
	expression_scale: f64,
	clamp_expressions: bool,
) -> Result<UNMotionFrame, String> {
	if !translation_scale.is_finite() {
		return Err("translation_scale must be finite".to_string());
	}
	if !expression_scale.is_finite() {
		return Err("expression_scale must be finite".to_string());
	}
	let mut out = frame.clone();
	if let Some(body) = out.body.as_mut() {
		if let Some(humanoid) = body.humanoid.as_mut() {
			if let Some(root) = humanoid.root.as_mut() {
				scale_transform_translation(root, translation_scale as f32);
			}
			for bone in &mut humanoid.bones {
				scale_transform_translation(&mut bone.transform, translation_scale as f32);
			}
		}
	}
	if let Some(face) = out.face.as_mut() {
		if let Some(head) = face.head.as_mut() {
			scale_transform_translation(head, translation_scale as f32);
		}
		for expression in &mut face.expressions {
			let mapped = expression.value * expression_scale as f32;
			expression.value = if clamp_expressions { mapped.clamp(0.0, 1.0) } else { mapped };
		}
	}
	Ok(out)
}

pub fn extract_bone<'a>(frame: &'a UNMotionFrame, bone_name: &str) -> Option<(HumanoidBone, &'a BoneSample)> {
	let want = vmc_bone_name_to_unmf(bone_name.trim())?;
	let humanoid = frame.body.as_ref()?.humanoid.as_ref()?;
	humanoid
		.bones
		.iter()
		.find(|sample| sample.bone == want)
		.map(|sample| (want, sample))
}

pub fn extract_expression<'a>(frame: &'a UNMotionFrame, name: &str) -> Option<&'a ExpressionSample> {
	let want = unmf_expression_name_to_vmc(name.trim());
	let face = frame.face.as_ref()?;
	if want.is_empty() {
		return face.expressions.first();
	}
	face.expressions
		.iter()
		.find(|sample| unmf_expression_name_to_vmc(&sample.name) == want)
}

pub fn vmc_bone_name_to_unmf(name: &str) -> Option<HumanoidBone> {
	match normalize_bone_name(name).as_str() {
		"hips" => Some(HumanoidBone::Hips),
		"spine" => Some(HumanoidBone::Spine),
		"chest" => Some(HumanoidBone::Chest),
		"upperchest" => Some(HumanoidBone::UpperChest),
		"neck" => Some(HumanoidBone::Neck),
		"head" => Some(HumanoidBone::Head),
		"leftshoulder" => Some(HumanoidBone::LeftShoulder),
		"leftupperarm" => Some(HumanoidBone::LeftUpperArm),
		"leftlowerarm" => Some(HumanoidBone::LeftLowerArm),
		"lefthand" => Some(HumanoidBone::LeftHand),
		"rightshoulder" => Some(HumanoidBone::RightShoulder),
		"rightupperarm" => Some(HumanoidBone::RightUpperArm),
		"rightlowerarm" => Some(HumanoidBone::RightLowerArm),
		"righthand" => Some(HumanoidBone::RightHand),
		"leftupperleg" => Some(HumanoidBone::LeftUpperLeg),
		"leftlowerleg" => Some(HumanoidBone::LeftLowerLeg),
		"leftfoot" => Some(HumanoidBone::LeftFoot),
		"lefttoes" => Some(HumanoidBone::LeftToes),
		"rightupperleg" => Some(HumanoidBone::RightUpperLeg),
		"rightlowerleg" => Some(HumanoidBone::RightLowerLeg),
		"rightfoot" => Some(HumanoidBone::RightFoot),
		"righttoes" => Some(HumanoidBone::RightToes),
		"lefteye" => Some(HumanoidBone::LeftEye),
		"righteye" => Some(HumanoidBone::RightEye),
		"jaw" => Some(HumanoidBone::Jaw),
		_ => None,
	}
}

pub fn unmf_bone_to_vmc_name(bone: HumanoidBone) -> Option<&'static str> {
	match bone {
		HumanoidBone::Hips => Some("Hips"),
		HumanoidBone::Spine => Some("Spine"),
		HumanoidBone::Chest => Some("Chest"),
		HumanoidBone::UpperChest => Some("UpperChest"),
		HumanoidBone::Neck => Some("Neck"),
		HumanoidBone::Head => Some("Head"),
		HumanoidBone::LeftShoulder => Some("LeftShoulder"),
		HumanoidBone::LeftUpperArm => Some("LeftUpperArm"),
		HumanoidBone::LeftLowerArm => Some("LeftLowerArm"),
		HumanoidBone::LeftHand => Some("LeftHand"),
		HumanoidBone::RightShoulder => Some("RightShoulder"),
		HumanoidBone::RightUpperArm => Some("RightUpperArm"),
		HumanoidBone::RightLowerArm => Some("RightLowerArm"),
		HumanoidBone::RightHand => Some("RightHand"),
		HumanoidBone::LeftUpperLeg => Some("LeftUpperLeg"),
		HumanoidBone::LeftLowerLeg => Some("LeftLowerLeg"),
		HumanoidBone::LeftFoot => Some("LeftFoot"),
		HumanoidBone::LeftToes => Some("LeftToes"),
		HumanoidBone::RightUpperLeg => Some("RightUpperLeg"),
		HumanoidBone::RightLowerLeg => Some("RightLowerLeg"),
		HumanoidBone::RightFoot => Some("RightFoot"),
		HumanoidBone::RightToes => Some("RightToes"),
		HumanoidBone::LeftEye => Some("LeftEye"),
		HumanoidBone::RightEye => Some("RightEye"),
		HumanoidBone::Jaw => Some("Jaw"),
	}
}

fn normalize_bone_name(name: &str) -> String {
	name.chars()
		.filter(|ch| ch.is_ascii_alphanumeric())
		.flat_map(char::to_lowercase)
		.collect()
}

fn parse_bone_filter(input: &str) -> Option<Vec<HumanoidBone>> {
	let mut bones = Vec::new();
	for part in input.split(',') {
		let name = part.trim();
		if name.is_empty() {
			continue;
		}
		if let Some(bone) = vmc_bone_name_to_unmf(name) {
			if !bones.contains(&bone) {
				bones.push(bone);
			}
		}
	}
	if bones.is_empty() {
		None
	} else {
		Some(bones)
	}
}

fn scale_transform_translation(transform: &mut TransformSample, scale: f32) {
	if let Some(p) = transform.translation.as_mut() {
		p.x *= scale;
		p.y *= scale;
		p.z *= scale;
	}
}

fn transform_from_vmc(position: &[f64; 3], rotation: &[f64; 4]) -> TransformSample {
	TransformSample {
		translation: Some(Vec3f {
			x: position[0] as f32,
			y: position[1] as f32,
			z: position[2] as f32,
		}),
		rotation: Some(Quatf {
			x: rotation[0] as f32,
			y: rotation[1] as f32,
			z: rotation[2] as f32,
			w: rotation[3] as f32,
		}),
		scale: None,
		linear_velocity: None,
		angular_velocity: None,
	}
}

fn transform_to_vmc_message(address: &str, name: &str, transform: &TransformSample) -> Option<OscMessageWire> {
	let p = transform.translation?;
	let r = transform.rotation?;
	Some(OscMessageWire {
		address: address.to_string(),
		args: vec![
			JsonValue::String(name.to_string()),
			json!(p.x),
			json!(p.y),
			json!(p.z),
			json!(r.x),
			json!(r.y),
			json!(r.z),
			json!(r.w),
		],
	})
}

fn vmc_expression_name_to_unmf(name: &str) -> String {
	name.to_string()
}

fn unmf_expression_name_to_vmc(name: &str) -> String {
	name.trim_prefix("vmc:").to_string()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn vmc_frame_converts_to_unmf_body_and_face() {
		let frame = MotionFrame {
			byte_len: 0,
			osc_messages: vec![
				OscMessageWire {
					address: VMC_EXT_BONE_POS.into(),
					args: vec![
						json!("Head"),
						json!(1.0),
						json!(2.0),
						json!(3.0),
						json!(0.0),
						json!(0.0),
						json!(0.0),
						json!(1.0),
					],
				},
				OscMessageWire {
					address: VMC_EXT_BLEND_VAL.into(),
					args: vec![json!("Joy"), json!(0.75)],
				},
			],
		};
		let unmf = vmc_motion_frame_to_unmf(&frame, 7);
		assert_eq!(unmf.header.magic, *b"UNMF");
		assert_eq!(unmf.header.sequence, 7);
		let bone = extract_bone(&unmf, "Head").expect("head").1;
		assert_eq!(bone.bone, HumanoidBone::Head);
		assert_eq!(bone.transform.translation.unwrap().x, 1.0);
		let expr = extract_expression(&unmf, "Joy").expect("joy");
		assert_eq!(expr.name, "Joy");
		assert_eq!(expr.value, 0.75);
	}

	#[test]
	fn unmf_roundtrips_through_unmfz_payload() {
		let frame = UNMotionFrame::new(42);
		let bytes = encode_unmfz_payload(&frame).expect("encode");
		let decoded = decode_unmfz_payload(&bytes).expect("decode");
		assert_eq!(decoded.header.sequence, 42);
	}

	#[test]
	fn unmf_converts_back_to_vmc_motion_frame() {
		let mut frame = UNMotionFrame::new(1);
		frame.body = Some(BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: None,
				bones: vec![BoneSample {
					bone: HumanoidBone::Head,
					transform: transform_from_vmc(&[1.0, 2.0, 3.0], &[0.0, 0.0, 0.0, 1.0]),
					confidence: 1.0,
					source_index: None,
					state: SampleState::Valid,
				}],
			}),
		});
		frame.face = Some(FaceMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			head: None,
			expressions: vec![ExpressionSample {
				name: "vmc:Joy".into(),
				value: 0.5,
				confidence: 1.0,
				source_index: None,
				state: SampleState::Valid,
			}],
		});
		let vmc = unmf_to_vmc_motion_frame(&frame);
		assert_eq!(vmc.osc_messages.len(), 2);
		assert_eq!(vmc.osc_messages[0].address, VMC_EXT_BONE_POS);
		assert_eq!(vmc.osc_messages[0].args[0], json!("Head"));
		assert_eq!(vmc.osc_messages[1].args[0], json!("Joy"));
	}

	#[test]
	fn filter_and_map_unmf_motion() {
		let source = MotionFrame {
			byte_len: 0,
			osc_messages: vec![
				OscMessageWire {
					address: VMC_EXT_BONE_POS.into(),
					args: vec![
						json!("Head"),
						json!(1.0),
						json!(2.0),
						json!(3.0),
						json!(0.0),
						json!(0.0),
						json!(0.0),
						json!(1.0),
					],
				},
				OscMessageWire {
					address: VMC_EXT_BONE_POS.into(),
					args: vec![
						json!("Hips"),
						json!(4.0),
						json!(5.0),
						json!(6.0),
						json!(0.0),
						json!(0.0),
						json!(0.0),
						json!(1.0),
					],
				},
				OscMessageWire {
					address: VMC_EXT_BLEND_VAL.into(),
					args: vec![json!("Joy"), json!(0.75)],
				},
			],
		};
		let frame = vmc_motion_frame_to_unmf(&source, 1);
		let filtered = filter_unmf(&frame, "Head", "Joy");
		assert!(extract_bone(&filtered, "Head").is_some());
		assert!(extract_bone(&filtered, "Hips").is_none());
		let mapped = map_unmf(&filtered, 2.0, 2.0, true).expect("map");
		let head = extract_bone(&mapped, "Head").expect("head").1;
		assert_eq!(head.transform.translation.unwrap().x, 2.0);
		assert_eq!(extract_expression(&mapped, "Joy").expect("joy").value, 1.0);
	}
}
