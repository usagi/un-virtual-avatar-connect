//! Explicit finger-chain ownership for authored poses. No wrist ownership here.
use un_motion_frame::{Finger, HandMotion, TrackingState, TransformSample, UNMotionFrame};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OwnedFinger {
	pub left: bool,
	pub finger: Finger,
}

pub(crate) fn parse(input: &str) -> Result<Vec<OwnedFinger>, String> {
	let mut result = Vec::new();
	for token in input.split(',').map(str::trim).filter(|s| !s.is_empty()) {
		let (side, name) = token
			.split_once('.')
			.ok_or_else(|| format!("invalid owned_fingers entry: {token}"))?;
		let left = match side {
			"left" => true,
			"right" => false,
			_ => return Err(format!("invalid finger side: {side}")),
		};
		let finger = match name {
			"thumb" => Finger::Thumb,
			"index" => Finger::Index,
			"middle" => Finger::Middle,
			"ring" => Finger::Ring,
			"little" => Finger::Little,
			_ => return Err(format!("invalid finger name: {name}; use a complete three-joint finger chain")),
		};
		let owned = OwnedFinger { left, finger };
		if result.contains(&owned) {
			return Err(format!("duplicate owned_fingers entry: {token}"));
		}
		result.push(owned);
	}
	Ok(result)
}

fn hand(frame: &UNMotionFrame, left: bool) -> Option<&HandMotion> {
	if left {
		frame.left_hand.as_ref()
	} else {
		frame.right_hand.as_ref()
	}
}

pub(crate) fn validate(frame: &UNMotionFrame, mask: &[OwnedFinger], label: &str) -> Result<(), String> {
	for owned in mask {
		let h = hand(frame, owned.left).ok_or_else(|| format!("{label}: missing hand for {owned:?}"))?;
		if h.tracking_state == TrackingState::Lost {
			return Err(format!("{label}: lost hand for {owned:?}"));
		}
		let fingers: Vec<_> = h.fingers.iter().filter(|f| f.finger == owned.finger).collect();
		if fingers.len() != 1 || fingers[0].joints.len() != 3 {
			return Err(format!(
				"{label}: {owned:?} requires exactly one proximal/intermediate/distal chain"
			));
		}
		for joint in &fingers[0].joints {
			let q = joint.rotation.ok_or_else(|| format!("{label}: missing rotation for {owned:?}"))?;
			let norm = q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
			if !norm.is_finite() || norm < 1e-12 {
				return Err(format!("{label}: invalid rotation for {owned:?}"));
			}
			// Authored finger poses rotate joints, never change bone lengths.
			if joint.translation.is_some() || joint.scale.is_some() {
				return Err(format!("{label}: finger translation/scale is unsupported for {owned:?}"));
			}
		}
	}
	Ok(())
}

pub(crate) fn blend(
	from: &UNMotionFrame,
	to: &UNMotionFrame,
	output: &mut UNMotionFrame,
	mask: &[OwnedFinger],
	mut transform: impl FnMut(&TransformSample, &TransformSample) -> Result<TransformSample, String>,
) -> Result<(), String> {
	if !mask.is_empty() && from.header.coordinate_space != to.header.coordinate_space {
		return Err("pose/release finger coordinate spaces differ".into());
	}
	validate(from, mask, "release")?;
	validate(to, mask, "pose")?;
	output.left_hand = None;
	output.right_hand = None;
	for left in [true, false] {
		let selected: Vec<_> = mask.iter().filter(|m| m.left == left).collect();
		if selected.is_empty() {
			continue;
		}
		let a = hand(from, left).unwrap();
		let b = hand(to, left).unwrap();
		let mut result = b.clone();
		result.wrist = None;
		result.fingers.clear();
		for owned in selected {
			let af = a.fingers.iter().find(|f| f.finger == owned.finger).unwrap();
			let mut bf = b.fingers.iter().find(|f| f.finger == owned.finger).unwrap().clone();
			for (index, joint) in bf.joints.iter_mut().enumerate() {
				*joint = transform(&af.joints[index], joint)?;
			}
			result.fingers.push(bf);
		}
		if left {
			output.left_hand = Some(result);
		} else {
			output.right_hand = Some(result);
		}
	}
	Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
	use super::*;
	use un_motion_frame::{FingerPose, Quatf};

	pub(crate) fn frame() -> UNMotionFrame {
		let mut frame: UNMotionFrame =
			serde_json::from_str(include_str!("../assets/avatar-poses/compact-greeting/greeting-hand.unmf.json")).unwrap();
		let transform = TransformSample {
			translation: None,
			rotation: Some(Quatf {
				x: 0.,
				y: 0.,
				z: 0.,
				w: 1.,
			}),
			scale: None,
			linear_velocity: None,
			angular_velocity: None,
		};
		frame.left_hand = Some(HandMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.,
			wrist: Some(transform.clone()),
			fingers: vec![
				FingerPose {
					finger: Finger::Index,
					joints: vec![transform.clone(); 3],
					confidence: 1.,
				},
				FingerPose {
					finger: Finger::Thumb,
					joints: vec![transform; 3],
					confidence: 1.,
				},
			],
		});
		frame.right_hand = frame.left_hand.clone();
		frame
	}

	#[test]
	fn rejects_partial_ambiguous_or_invalid_finger_assets() {
		let mask = parse("left.index").unwrap();
		for count in [0, 1, 2, 4, 5] {
			let mut f = frame();
			let finger = &mut f.left_hand.as_mut().unwrap().fingers[0];
			finger.joints.resize(count, finger.joints[0].clone());
			assert!(validate(&f, &mask, "pose").is_err());
		}
		let mut f = frame();
		f.left_hand.as_mut().unwrap().fingers[0].joints[0].rotation.as_mut().unwrap().w = f32::NAN;
		assert!(validate(&f, &mask, "pose").is_err());
		assert!(parse("left.index,left.index").is_err());
		assert!(parse("left.index.distal").is_err());
		let mut missing = frame();
		missing.left_hand = None;
		assert!(validate(&missing, &mask, "release").is_err());
	}

	#[test]
	fn merge_preserves_wrist_other_fingers_and_opposite_hand() {
		let mut target = frame();
		let before = target.clone();
		let mut source = frame();
		source.left_hand.as_mut().unwrap().fingers[0].joints[1].rotation = Some(Quatf {
			x: 0.,
			y: 0.,
			z: 1.,
			w: 0.,
		});
		source.left_hand.as_mut().unwrap().wrist = None;
		merge(&mut target, &source, &parse("left.index").unwrap());
		assert_eq!(
			target.left_hand.as_ref().unwrap().fingers[0],
			source.left_hand.as_ref().unwrap().fingers[0]
		);
		assert_eq!(
			target.left_hand.as_ref().unwrap().fingers[1],
			before.left_hand.as_ref().unwrap().fingers[1]
		);
		assert_eq!(target.left_hand.as_ref().unwrap().wrist, before.left_hand.as_ref().unwrap().wrist);
		assert_eq!(target.right_hand, before.right_hand);
	}

	#[test]
	fn empty_mask_drops_all_fingers_from_asset_output() {
		let f = frame();
		let mut output = f.clone();
		blend(&f, &f, &mut output, &[], |a, _| Ok(a.clone())).unwrap();
		assert!(output.left_hand.is_none() && output.right_hand.is_none());
	}
}

pub(crate) fn merge(output: &mut UNMotionFrame, source: &UNMotionFrame, mask: &[OwnedFinger]) {
	for owned in mask {
		let Some(src) = hand(source, owned.left) else {
			continue;
		};
		let Some(finger) = src.fingers.iter().find(|f| f.finger == owned.finger) else {
			continue;
		};
		let target = if owned.left {
			&mut output.left_hand
		} else {
			&mut output.right_hand
		};
		let target = target.get_or_insert_with(|| HandMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			wrist: None,
			fingers: Vec::new(),
		});
		if let Some(existing) = target.fingers.iter_mut().find(|f| f.finger == owned.finger) {
			*existing = finger.clone();
		} else {
			target.fingers.push(finger.clone());
		}
	}
}
