//! Avatar intent nodes.

use crate::flowgraph::node::{
	get_optional_bool, get_optional_int, get_optional_string, get_required_json, get_required_unmf, EffectfulNode, ExecCtx, ExecFireSet,
	InputMap, NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec, PureNode,
};
use crate::flowgraph::socket::{FlowResult, SocketType, SocketValue};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use un_motion_frame::{HumanoidBone, Quatf, TransformSample, Vec3f};
use vac_motion::avatar_intent::{
	avatar_intent_duration_ms, avatar_intent_neutral_unmf_with_bindings,
	avatar_intent_to_unmf_with_bindings, AvatarExpressionBindings, AvatarIntentMapperOptions, AvatarIntentRequest,
};
use vac_motion::unmf::UnMotionFrame;

use crate::avatar_motion::{AvatarSemanticOwnership, SemanticPlaybackGuard};

pub struct AvatarIntentToUnmfNode;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AvatarPoseCatalog {
	schema: String,
	presets: BTreeMap<String, AvatarPoseCatalogEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AvatarPoseCatalogEntry {
	pose: String,
	release: String,
	owned_bones: Vec<String>,
	#[serde(default)]
	owned_fingers: Vec<String>,
	attack_ms: i64,
	hold_ms: i64,
	release_ms: i64,
}

pub struct AvatarPoseCatalogLookupNode;

impl NodeDescriptor for AvatarPoseCatalogLookupNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.avatar.pose_catalog_lookup".into(),
			title: "Avatar: Lookup Pose Catalog".into(),
			category: "avatar".into(),
			description: Some("許可済みpreset名から保存UNMF poseと安全な再生設定を解決する。".into()),
			inputs: vec![
				PortSpec::input("catalog", "Catalog", SocketType::Json),
				PortSpec::input("preset", "Preset", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("asset_root", "Asset Root", SocketType::String).with_default(SocketValue::String(String::new())),
			],
			outputs: vec![
				PortSpec::output("ok", "OK", SocketType::Bool),
				PortSpec::output("pose_path", "Pose Path", SocketType::String),
				PortSpec::output("release_path", "Release Path", SocketType::String),
				PortSpec::output("owned_bones", "Owned Bones", SocketType::String),
 PortSpec::output("owned_fingers", "Owned Fingers", SocketType::String),
				PortSpec::output("attack_ms", "Attack ms", SocketType::Int),
				PortSpec::output("hold_ms", "Hold ms", SocketType::Int),
				PortSpec::output("release_ms", "Release ms", SocketType::Int),
				PortSpec::output("error", "Error", SocketType::String),
			],
			properties: vec![],
		}
	}
}

fn pose_catalog_error(error: impl Into<String>) -> NodeOutput {
	NodeOutput::new()
		.set_data("ok", SocketValue::Bool(false))
		.set_data("pose_path", SocketValue::String(String::new()))
		.set_data("release_path", SocketValue::String(String::new()))
		.set_data("owned_bones", SocketValue::String(String::new()))
 .set_data("owned_fingers", SocketValue::String(String::new()))
		.set_data("attack_ms", SocketValue::Int(0))
		.set_data("hold_ms", SocketValue::Int(0))
		.set_data("release_ms", SocketValue::Int(0))
		.set_data("error", SocketValue::String(error.into()))
}

fn resolve_pose_asset_path(asset_root: &str, relative: &str) -> Result<String, String> {
	let asset_root = asset_root.trim();
	if asset_root.is_empty() {
		return Err("asset_root is required".into());
	}
	let relative = Path::new(relative.trim());
	if relative.as_os_str().is_empty()
		|| relative.is_absolute()
		|| relative.components().any(|component| !matches!(component, Component::Normal(_)))
	{
		return Err(format!(
			"pose catalog asset path must be a normalized relative path: {}",
			relative.display()
		));
	}
	Ok(Path::new(asset_root).join(relative).to_string_lossy().into_owned())
}

fn validate_pose_timing(value: i64, maximum: i64, label: &str) -> Result<i64, String> {
	if !(0..=maximum).contains(&value) {
		return Err(format!("pose catalog {label} must be between 0 and {maximum}"));
	}
	Ok(value)
}

#[async_trait]
impl PureNode for AvatarPoseCatalogLookupNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let catalog = match serde_json::from_value::<AvatarPoseCatalog>(get_required_json(inputs, "catalog")?.clone()) {
			Ok(catalog) => catalog,
			Err(error) => return Ok(pose_catalog_error(format!("invalid avatar pose catalog: {error}"))),
		};
		if catalog.schema != "unvac.avatar_pose_catalog.v1" {
			return Ok(pose_catalog_error(format!(
				"unsupported avatar pose catalog schema: {}",
				catalog.schema
			)));
		}
		let preset = get_optional_string(inputs, "preset", "")?;
		let preset = preset.trim();
		if preset.is_empty()
			|| preset.len() > 64
			|| !preset
				.bytes()
				.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-'))
		{
			return Ok(pose_catalog_error("preset must be a lowercase ASCII identifier"));
		}
		let Some(entry) = catalog.presets.get(preset) else {
			return Ok(pose_catalog_error(format!("unknown avatar pose preset: {preset}")));
		};
		let owned_bones = entry.owned_bones.join(",");
 let owned_fingers = entry.owned_fingers.join(",");
 if let Err(error) = crate::avatar_hand_pose::parse(&owned_fingers) { return Ok(pose_catalog_error(error)); }
		if let Err(error) = parse_owned_bones(&owned_bones) {
			return Ok(pose_catalog_error(error));
		}
		let asset_root = get_optional_string(inputs, "asset_root", "")?;
		let values = (|| {
			Ok::<_, String>((
				resolve_pose_asset_path(&asset_root, &entry.pose)?,
				resolve_pose_asset_path(&asset_root, &entry.release)?,
				validate_pose_timing(entry.attack_ms, 5_000, "attack_ms")?,
				validate_pose_timing(entry.hold_ms, 60_000, "hold_ms")?,
				validate_pose_timing(entry.release_ms, 5_000, "release_ms")?,
			))
		})();
		let (pose_path, release_path, attack_ms, hold_ms, release_ms) = match values {
			Ok(values) => values,
			Err(error) => return Ok(pose_catalog_error(error)),
		};
		Ok(NodeOutput::new()
			.set_data("ok", SocketValue::Bool(true))
			.set_data("pose_path", SocketValue::String(pose_path))
			.set_data("release_path", SocketValue::String(release_path))
			.set_data("owned_bones", SocketValue::String(owned_bones))
 .set_data("owned_fingers", SocketValue::String(owned_fingers))
			.set_data("attack_ms", SocketValue::Int(attack_ms))
			.set_data("hold_ms", SocketValue::Int(hold_ms))
			.set_data("release_ms", SocketValue::Int(release_ms))
			.set_data("error", SocketValue::String(String::new())))
	}
}

impl NodeDescriptor for AvatarIntentToUnmfNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.avatar.intent_to_unmf".into(),
			title: "Avatar: Intent to UNMF".into(),
			category: "avatar".into(),
			description: Some("AI 用 AvatarIntent JSON を安全な UNMotionFrame に変換する。".into()),
			inputs: vec![
				PortSpec::input("intent", "Intent", SocketType::Json),
				PortSpec::input("sequence", "Sequence", SocketType::Int).with_default(SocketValue::Int(0)),
				PortSpec::input("elapsed_ms", "Elapsed ms", SocketType::Int).with_default(SocketValue::Int(0)),
				PortSpec::input("expression_bindings", "Expression bindings", SocketType::Json)
					.with_default(SocketValue::Json(serde_json::json!({}))),
			],
			outputs: vec![
				PortSpec::output("ok", "OK", SocketType::Bool),
				PortSpec::output("unmf", "UNMF", SocketType::Unmf),
				PortSpec::output("target_link_id", "Target Link ID", SocketType::String),
				PortSpec::output("error", "Error", SocketType::String),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Unmf))),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for AvatarIntentToUnmfNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let value = get_required_json(inputs, "intent")?;
		let sequence = get_optional_int(inputs, "sequence", 0)?.max(0) as u64;
		let elapsed_ms = get_optional_int(inputs, "elapsed_ms", 0)?.max(0) as u64;
		let bindings = match inputs.get("expression_bindings") {
			None => Ok(AvatarExpressionBindings::new()),
			Some(SocketValue::Json(value)) => serde_json::from_value(value.clone()).map_err(|error| error.to_string()),
			Some(_) => Err("expression_bindings must be JSON".to_string()),
		};
		match bindings.and_then(|bindings| intent_json_to_unmf_with_bindings(value, sequence, elapsed_ms, &bindings)) {
			Ok((request, frame)) => Ok(NodeOutput::new()
				.set_data("ok", SocketValue::Bool(true))
				.set_data("unmf", SocketValue::Unmf(frame.clone()))
				.set_data("target_link_id", SocketValue::String(request.target_link_id.unwrap_or_default()))
				.set_data("error", SocketValue::String(String::new()))
				.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Unmf(frame))))),
			Err(error) => {
				let frame = UnMotionFrame::new(sequence);
				Ok(NodeOutput::new()
					.set_data("ok", SocketValue::Bool(false))
					.set_data("unmf", SocketValue::Unmf(frame))
					.set_data("target_link_id", SocketValue::String(String::new()))
					.set_data("error", SocketValue::String(error.clone()))
					.set_data("result", SocketValue::Result(FlowResult::err(error).with_code("avatar.intent"))))
			}
		}
	}
}

fn intent_json_to_unmf_with_bindings(
	value: &JsonValue, sequence: u64, elapsed_ms: u64, bindings: &AvatarExpressionBindings,
) -> Result<(AvatarIntentRequest, UnMotionFrame), String> {
	let request: AvatarIntentRequest = serde_json::from_value(value.clone()).map_err(|e| format!("AvatarIntent JSON decode: {e}"))?;
	let frame = avatar_intent_to_unmf_with_bindings(
		&request,
		&AvatarIntentMapperOptions {
			sequence,
			elapsed_ms,
			..AvatarIntentMapperOptions::default()
		},
		bindings,
	)?;
	Ok((request, frame))
}

/// AvatarIntentを時間サンプル列へ展開し、AvatarLink compositorへ投入する。
#[derive(Default)]
pub struct AvatarIntentPlayUnmfzNode {
	next_sequence: std::sync::Arc<AtomicU64>,
	jobs: super::avatar_jobs::AvatarJobs,
}

impl NodeDescriptor for AvatarIntentPlayUnmfzNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.avatar.intent_play_unmfz".into(),
			title: "Avatar: Play Intent to UNMF/Z".into(),
			category: "avatar".into(),
			description: Some("AvatarIntentを時系列UNMotionFrameへ展開し、終了時のneutral復帰までAvatarLink compositorへ投入する。".into()),
			inputs: vec![
				PortSpec::exec_input(super::avatar_jobs::RESUME, "(internal)"),
				PortSpec::input(super::avatar_jobs::JOB_ID, "(internal)", SocketType::Int).with_default(SocketValue::Int(0)),
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("intent", "Intent", SocketType::Json),
				PortSpec::input("base_key_expr", "Base Key", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("topic_mode", "Topic Mode", SocketType::String).with_default(SocketValue::String("frame".into())),
				PortSpec::input("schema_major", "Schema Major", SocketType::Int).with_default(SocketValue::Int(1)),
				PortSpec::input("endpoint", "UNAA/Z Endpoint", SocketType::Json).with_default(SocketValue::Json(JsonValue::Null)),
				PortSpec::input("fps", "Frames per second", SocketType::Int).with_default(SocketValue::Int(30)),
				PortSpec::input("release_ms", "Neutral release ms", SocketType::Int).with_default(SocketValue::Int(200)),
				PortSpec::input("dry_run", "Dry Run", SocketType::Bool).with_default(SocketValue::Bool(false)),
			],
			outputs: vec![
				PortSpec::exec_output("on_success", "On Success"),
				PortSpec::exec_output("on_error", "On Error"),
				PortSpec::output("frames_sent", "Samples Submitted", SocketType::Int),
				PortSpec::output("key_expr", "Key Expr", SocketType::String),
				PortSpec::output("error", "Error", SocketType::String),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Int))),
			],
			properties: vec![],
		}
	}
}

fn avatar_play_error(error: impl Into<String>) -> NodeOutput {
	let error = error.into();
	NodeOutput::new()
		.set_data("frames_sent", SocketValue::Int(0))
		.set_data("key_expr", SocketValue::String(String::new()))
		.set_data("error", SocketValue::String(error.clone()))
		.set_data(
			"result",
			SocketValue::Result(FlowResult::err(error).with_code("avatar.intent_play_unmfz")),
		)
		.fire_exec("on_error")
}

impl AvatarIntentPlayUnmfzNode {
	async fn run_inline(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired_exec.contains("exec_in") {
			return Ok(NodeOutput::new());
		}
		let request = match serde_json::from_value::<AvatarIntentRequest>(get_required_json(inputs, "intent")?.clone()) {
			Ok(request) => request,
			Err(error) => return Ok(avatar_play_error(format!("AvatarIntent JSON decode: {error}"))),
		};
		let duration_ms = match avatar_intent_duration_ms(&request) {
			Ok(duration) => duration,
			Err(error) => return Ok(avatar_play_error(error)),
		};
		let resolved_strategy = super::unmf::resolved_strategy_from_inputs(inputs)?;
		let strategy = super::unmf::strategy_from_inputs(inputs)?;
		let fps = get_optional_int(inputs, "fps", 30)?.clamp(10, 60) as u64;
		let frame_interval_ms = 1000 / fps;
		let release_ms = get_optional_int(inputs, "release_ms", 200)?.clamp(0, 1_000) as u64;
		let dry_run = get_optional_bool(inputs, "dry_run", false)?;
		let target_link_id = request.target_link_id.clone().unwrap_or_default();
		let bindings = match ctx.state_handle.as_ref().and_then(|weak| weak.upgrade()) { Some(state) => {
			state.read().await.avatar_links.get(&target_link_id)
				.map(|link| link.expression_bindings.clone()).unwrap_or_default()
		} _ => {
			AvatarExpressionBindings::new()
		}};
		let compositor = if dry_run {
			None
		} else {
			if !resolved_strategy.topic_mode.eq_ignore_ascii_case("frame") || resolved_strategy.schema_major != 1 {
				return Ok(avatar_play_error(
					"Avatar motion compositor supports only UNMF/Z topic_mode=frame and schema_major=1",
				));
			}
			let Some(state) = ctx.state_handle.as_ref().and_then(|weak| weak.upgrade()) else {
				return Ok(avatar_play_error(
					"Avatar motion compositor is unavailable without VAC runtime state",
				));
			};
			let avatar_motion = state.read().await.avatar_motion.clone();
			let token = match avatar_motion
				.begin_semantic(resolved_strategy.base_key_expr.clone(), target_link_id.clone())
				.await
			{
				Ok(token) => token,
				Err(error) => return Ok(avatar_play_error(error)),
			};
			Some((avatar_motion, token))
		};
		let _cleanup = compositor
			.as_ref()
			.map(|(controller, token)| {
				avatar_intent_neutral_unmf_with_bindings(&request, self.next_sequence.fetch_add(1, Ordering::Relaxed), &bindings)
					.map(|neutral| SemanticPlaybackGuard::new(controller.clone(), target_link_id.clone(), *token, neutral))
			})
			.transpose()
			.map_err(|error| NodeExecError::Generic(anyhow::anyhow!("{error}")))?;
		let mut elapsed_ms = 0;
		let mut frames_sent = 0i64;
		let mut playback_started = None;

		loop {
			let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
			let frame = match avatar_intent_to_unmf_with_bindings(
				&request,
				&AvatarIntentMapperOptions {
					sequence,
					elapsed_ms,
					..AvatarIntentMapperOptions::default()
				},
				&bindings,
			) {
				Ok(frame) => frame,
				Err(error) => {
					if let Some((avatar_motion, token)) = compositor.as_ref() {
						let _ = avatar_motion.end_semantic(target_link_id.clone(), *token);
					}
					return Ok(avatar_play_error(error));
				}
			};
			if let Some((avatar_motion, token)) = compositor.as_ref() {
				if let Err(error) = avatar_motion.set_semantic_frame(target_link_id.clone(), *token, frame.clone()) {
					let _ = avatar_motion.end_semantic(target_link_id.clone(), *token);
					return Ok(avatar_play_error(error));
				}
			}
			frames_sent += 1;
			let playback_started = *playback_started.get_or_insert_with(tokio::time::Instant::now);
			if elapsed_ms >= duration_ms {
				break;
			}
			let next_elapsed = (elapsed_ms + frame_interval_ms).min(duration_ms);
			tokio::time::sleep_until(playback_started + Duration::from_millis(next_elapsed)).await;
			elapsed_ms = next_elapsed;
		}

		let mut release_elapsed_ms = 0;
		let mut release_started = None;
		let last_key = loop {
			let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
			let neutral = match avatar_intent_neutral_unmf_with_bindings(&request, sequence, &bindings) {
				Ok(frame) => frame,
				Err(error) => {
					if let Some((avatar_motion, token)) = compositor.as_ref() {
						let _ = avatar_motion.end_semantic(target_link_id.clone(), *token);
					}
					return Ok(avatar_play_error(error));
				}
			};
			let key_expr = strategy.key_expr_for_frame(&neutral);
			if let Some((avatar_motion, token)) = compositor.as_ref() {
				if let Err(error) = avatar_motion.set_semantic_frame(target_link_id.clone(), *token, neutral) {
					let _ = avatar_motion.end_semantic(target_link_id.clone(), *token);
					return Ok(avatar_play_error(error));
				}
			}
			frames_sent += 1;
			let release_started = *release_started.get_or_insert_with(tokio::time::Instant::now);
			if release_elapsed_ms >= release_ms {
				break key_expr;
			}
			let next_elapsed = (release_elapsed_ms + frame_interval_ms).min(release_ms);
			tokio::time::sleep_until(release_started + Duration::from_millis(next_elapsed)).await;
			release_elapsed_ms = next_elapsed;
		};
		if let Some((avatar_motion, token)) = compositor.as_ref() {
			if let Err(error) = avatar_motion.end_semantic(target_link_id, *token) {
				return Ok(avatar_play_error(error));
			}
		}
		ctx.log(format!(
			"avatar intent playback: {last_key} samples={frames_sent} duration_ms={duration_ms} release_ms={release_ms} dry_run={dry_run}"
		));
		Ok(NodeOutput::new()
			.set_data("frames_sent", SocketValue::Int(frames_sent))
			.set_data("key_expr", SocketValue::String(last_key))
			.set_data("error", SocketValue::String(String::new()))
			.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Int(frames_sent))))
			.fire_exec("on_success"))
	}
}

/// 保存済みUNMF poseと解除用baselineをAvatarLink compositor上で再生する。
#[derive(Default)]
pub struct AvatarPoseAssetPlayUnmfzNode {
	jobs: super::avatar_jobs::AvatarJobs,
}

impl NodeDescriptor for AvatarPoseAssetPlayUnmfzNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.avatar.pose_asset_play_unmfz".into(),
			title: "Avatar: Play Pose Asset to UNMF/Z".into(),
			category: "avatar".into(),
			description: Some(
				"保存済みUNMF poseを所有boneだけ適用し、baselineで解除する。口パクと瞬きはAvatarLink compositorが保持する。".into(),
			),
			inputs: vec![
				PortSpec::exec_input(super::avatar_jobs::RESUME, "(internal)"),
				PortSpec::input(super::avatar_jobs::JOB_ID, "(internal)", SocketType::Int).with_default(SocketValue::Int(0)),
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("pose", "Pose UNMF", SocketType::Unmf),
				PortSpec::input("release", "Release UNMF", SocketType::Unmf),
				PortSpec::input("owned_bones", "Owned Bones", SocketType::String).with_default(SocketValue::String(String::new())),
 PortSpec::input("owned_fingers", "Owned Fingers", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("target_link_id", "Target Link ID", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("base_key_expr", "Base Key", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("topic_mode", "Topic Mode", SocketType::String).with_default(SocketValue::String("frame".into())),
				PortSpec::input("schema_major", "Schema Major", SocketType::Int).with_default(SocketValue::Int(1)),
				PortSpec::input("endpoint", "UNAA/Z Endpoint", SocketType::Json).with_default(SocketValue::Json(JsonValue::Null)),
				PortSpec::input("attack_ms", "Attack ms", SocketType::Int).with_default(SocketValue::Int(250)),
				PortSpec::input("hold_ms", "Hold ms", SocketType::Int).with_default(SocketValue::Int(1_000)),
				PortSpec::input("release_ms", "Release ms", SocketType::Int).with_default(SocketValue::Int(200)),
				PortSpec::input("fps", "Frames per second", SocketType::Int).with_default(SocketValue::Int(30)),
				PortSpec::input("dry_run", "Dry Run", SocketType::Bool).with_default(SocketValue::Bool(false)),
			],
			outputs: vec![
				PortSpec::exec_output("on_success", "On Success"),
				PortSpec::exec_output("on_error", "On Error"),
				PortSpec::output("frames_sent", "Samples Submitted", SocketType::Int),
				PortSpec::output("key_expr", "Key Expr", SocketType::String),
				PortSpec::output("error", "Error", SocketType::String),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Int))),
			],
			properties: vec![],
		}
	}
}

fn pose_asset_play_error(error: impl Into<String>) -> NodeOutput {
	let error = error.into();
	NodeOutput::new()
		.set_data("frames_sent", SocketValue::Int(0))
		.set_data("key_expr", SocketValue::String(String::new()))
		.set_data("error", SocketValue::String(error.clone()))
		.set_data(
			"result",
			SocketValue::Result(FlowResult::err(error).with_code("avatar.pose_asset_play_unmfz")),
		)
		.fire_exec("on_error")
}

fn parse_owned_bones(input: &str) -> Result<Vec<un_motion_frame::HumanoidBone>, String> {
	let mut bones = Vec::new();
	for name in input.split(',').map(str::trim).filter(|name| !name.is_empty()) {
		let Some(bone) = vac_motion::unmf::vmc_bone_name_to_unmf(name) else {
			return Err(format!("unknown owned bone: {name}"));
		};
		if !bones.contains(&bone) {
			bones.push(bone);
		}
	}
	if bones.is_empty() {
		return Err("owned_bones must contain at least one humanoid bone".into());
	}
	Ok(bones)
}

fn validate_owned_bones(frame: &UnMotionFrame, bones: &[un_motion_frame::HumanoidBone], label: &str) -> Result<(), String> {
	let available = frame
		.body
		.as_ref()
		.and_then(|body| body.humanoid.as_ref())
		.map(|pose| pose.bones.as_slice())
		.unwrap_or_default();
	for bone in bones {
		if !available.iter().any(|sample| sample.bone == *bone) {
			let name = vac_motion::unmf::unmf_bone_to_vmc_name(*bone).unwrap_or("Unknown");
			return Err(format!("{label} UNMF does not contain owned bone {name}"));
		}
	}
	Ok(())
}

fn lerp_vec3(from: Vec3f, to: Vec3f, amount: f32) -> Vec3f {
	Vec3f {
		x: from.x + (to.x - from.x) * amount,
		y: from.y + (to.y - from.y) * amount,
		z: from.z + (to.z - from.z) * amount,
	}
}

fn normalize_quat(value: Quatf) -> Quatf {
	let length = (value.x * value.x + value.y * value.y + value.z * value.z + value.w * value.w).sqrt();
	if length <= f32::EPSILON {
		return Quatf {
			x: 0.0,
			y: 0.0,
			z: 0.0,
			w: 1.0,
		};
	}
	Quatf {
		x: value.x / length,
		y: value.y / length,
		z: value.z / length,
		w: value.w / length,
	}
}

fn slerp_quat(from: Quatf, mut to: Quatf, amount: f32) -> Quatf {
	let from = normalize_quat(from);
	to = normalize_quat(to);
	let mut dot = from.x * to.x + from.y * to.y + from.z * to.z + from.w * to.w;
	if dot < 0.0 {
		dot = -dot;
		to = Quatf {
			x: -to.x,
			y: -to.y,
			z: -to.z,
			w: -to.w,
		};
	}
	if dot > 0.9995 {
		return normalize_quat(Quatf {
			x: from.x + (to.x - from.x) * amount,
			y: from.y + (to.y - from.y) * amount,
			z: from.z + (to.z - from.z) * amount,
			w: from.w + (to.w - from.w) * amount,
		});
	}
	let theta = dot.clamp(-1.0, 1.0).acos();
	let sin_theta = theta.sin();
	let from_weight = ((1.0 - amount) * theta).sin() / sin_theta;
	let to_weight = (amount * theta).sin() / sin_theta;
	normalize_quat(Quatf {
		x: from.x * from_weight + to.x * to_weight,
		y: from.y * from_weight + to.y * to_weight,
		z: from.z * from_weight + to.z * to_weight,
		w: from.w * from_weight + to.w * to_weight,
	})
}

fn blend_optional_vec3(
	from: Option<Vec3f>,
	to: Option<Vec3f>,
	amount: f32,
	bone: HumanoidBone,
	field: &str,
) -> Result<Option<Vec3f>, String> {
	match (from, to) {
		(Some(from), Some(to)) => Ok(Some(lerp_vec3(from, to, amount))),
		(None, None) => Ok(None),
		_ => Err(format!("pose/release transform mismatch for {bone:?}.{field}")),
	}
}

fn blend_optional_quat(from: Option<Quatf>, to: Option<Quatf>, amount: f32, bone: HumanoidBone) -> Result<Option<Quatf>, String> {
	match (from, to) {
		(Some(from), Some(to)) => Ok(Some(slerp_quat(from, to, amount))),
		(None, None) => Ok(None),
		_ => Err(format!("pose/release transform mismatch for {bone:?}.rotation")),
	}
}

fn blend_transform(from: &TransformSample, to: &TransformSample, amount: f32, bone: HumanoidBone) -> Result<TransformSample, String> {
	Ok(TransformSample {
		translation: blend_optional_vec3(from.translation, to.translation, amount, bone, "translation")?,
		rotation: blend_optional_quat(from.rotation, to.rotation, amount, bone)?,
		scale: blend_optional_vec3(from.scale, to.scale, amount, bone, "scale")?,
		linear_velocity: None,
		angular_velocity: None,
	})
}

fn blend_owned_pose(from: &UnMotionFrame, to: &UnMotionFrame, bones: &[HumanoidBone], amount: f32) -> Result<UnMotionFrame, String> {
	let from_pose = from
		.body
		.as_ref()
		.and_then(|body| body.humanoid.as_ref())
		.ok_or_else(|| "release UNMF does not contain a humanoid pose".to_string())?;
	let to_pose = to
		.body
		.as_ref()
		.and_then(|body| body.humanoid.as_ref())
		.ok_or_else(|| "pose UNMF does not contain a humanoid pose".to_string())?;
	let mut output = to.clone();
	output.face = None;
	output.eyes = None;
	output.left_hand = None;
	output.right_hand = None;
	output.signals.clear();
	let output_pose = output
		.body
		.as_mut()
		.and_then(|body| body.humanoid.as_mut())
		.expect("validated humanoid pose");
	output_pose.root = None;
	output_pose.bones.retain(|sample| bones.contains(&sample.bone));
	for output_bone in &mut output_pose.bones {
		let from_bone = from_pose
			.bones
			.iter()
			.find(|sample| sample.bone == output_bone.bone)
			.expect("validated release bone");
		let to_bone = to_pose
			.bones
			.iter()
			.find(|sample| sample.bone == output_bone.bone)
			.expect("validated pose bone");
		output_bone.transform = blend_transform(&from_bone.transform, &to_bone.transform, amount, output_bone.bone)?;
	}
	Ok(output)
}

fn blend_owned_pose_with_fingers(from: &UnMotionFrame, to: &UnMotionFrame, bones: &[HumanoidBone], fingers: &[crate::avatar_hand_pose::OwnedFinger], amount: f32) -> Result<UnMotionFrame, String> {
    let mut output = blend_owned_pose(from, to, bones, amount)?;
    crate::avatar_hand_pose::blend(from, to, &mut output, fingers, |a,b| blend_transform(a,b,amount,HumanoidBone::LeftHand))?;
    Ok(output)
}

fn eased_progress(elapsed_ms: u64, duration_ms: u64) -> f32 {
	if duration_ms == 0 {
		return 1.0;
	}
	let value = (elapsed_ms as f32 / duration_ms as f32).clamp(0.0, 1.0);
	value * value * (3.0 - 2.0 * value)
}

async fn submit_pose_phase(
	compositor: Option<&(crate::avatar_motion::AvatarMotionController, u64)>,
	target_link_id: &str,
	from: &UnMotionFrame,
	to: &UnMotionFrame,
	owned_bones: &[HumanoidBone],
 owned_fingers: &[crate::avatar_hand_pose::OwnedFinger],
	duration_ms: u64,
	frame_interval_ms: u64,
	dry_run: bool,
) -> Result<i64, String> {
	let mut samples = 0;
	let mut elapsed_ms = 0;
	let mut phase_started = None;
	loop {
		let frame = blend_owned_pose_with_fingers(from, to, owned_bones, owned_fingers, eased_progress(elapsed_ms, duration_ms))?;
		if let Some((avatar_motion, token)) = compositor {
			avatar_motion.set_semantic_frame(target_link_id.to_string(), *token, frame)?;
		}
		samples += 1;
		if elapsed_ms >= duration_ms {
			break;
		}
		let next_elapsed = (elapsed_ms + frame_interval_ms).min(duration_ms);
		if !dry_run {
			let phase_started = *phase_started.get_or_insert_with(tokio::time::Instant::now);
			tokio::time::sleep_until(phase_started + Duration::from_millis(next_elapsed)).await;
		}
		elapsed_ms = next_elapsed;
	}
	Ok(samples)
}

impl AvatarPoseAssetPlayUnmfzNode {
	async fn run_inline(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired_exec.contains("exec_in") {
			return Ok(NodeOutput::new());
		}
		let pose = get_required_unmf(inputs, "pose")?.clone();
		let release = get_required_unmf(inputs, "release")?.clone();
		let owned_bones = match parse_owned_bones(&get_optional_string(inputs, "owned_bones", "")?) {
			Ok(bones) => bones,
			Err(error) => return Ok(pose_asset_play_error(error)),
		};
		if let Err(error) =
			validate_owned_bones(&pose, &owned_bones, "pose").and_then(|()| validate_owned_bones(&release, &owned_bones, "release"))
		{
			return Ok(pose_asset_play_error(error));
		}

        let owned_fingers = match crate::avatar_hand_pose::parse(&get_optional_string(inputs, "owned_fingers", "")?) {
            Ok(mask) => mask, Err(error) => return Ok(pose_asset_play_error(error)),
        };
		let resolved_strategy = super::unmf::resolved_strategy_from_inputs(inputs)?;
		let strategy = super::unmf::strategy_from_inputs(inputs)?;
		let target_link_id = get_optional_string(inputs, "target_link_id", "")?;
		if target_link_id.trim().is_empty() {
			return Ok(pose_asset_play_error("target_link_id is required"));
		}
		let fps = get_optional_int(inputs, "fps", 30)?.clamp(10, 60) as u64;
		let frame_interval_ms = 1_000 / fps;
		let attack_ms = get_optional_int(inputs, "attack_ms", 250)?.clamp(0, 5_000) as u64;
		let hold_ms = get_optional_int(inputs, "hold_ms", 1_000)?.clamp(0, 60_000) as u64;
		let release_ms = get_optional_int(inputs, "release_ms", 200)?.clamp(0, 5_000) as u64;
		let dry_run = get_optional_bool(inputs, "dry_run", false)?;
		if let Err(error) = blend_owned_pose_with_fingers(&release, &pose, &owned_bones, &owned_fingers, 0.5) {
			return Ok(pose_asset_play_error(error));
		}

		let compositor = if dry_run {
			None
		} else {
			if !resolved_strategy.topic_mode.eq_ignore_ascii_case("frame") || resolved_strategy.schema_major != 1 {
				return Ok(pose_asset_play_error(
					"Avatar motion compositor supports only UNMF/Z topic_mode=frame and schema_major=1",
				));
			}
			let Some(state) = ctx.state_handle.as_ref().and_then(|weak| weak.upgrade()) else {
				return Ok(pose_asset_play_error(
					"Avatar motion compositor is unavailable without VAC runtime state",
				));
			};
			let avatar_motion = state.read().await.avatar_motion.clone();
			let ownership = AvatarSemanticOwnership {
				owned_bones: Some(owned_bones.clone()),
 owned_fingers: owned_fingers.clone(),
				owns_root: false,
			};
			let token = match avatar_motion
				.begin_semantic_with_ownership(resolved_strategy.base_key_expr.clone(), target_link_id.clone(), ownership)
				.await
			{
				Ok(token) => token,
				Err(error) => return Ok(pose_asset_play_error(error)),
			};
			Some((avatar_motion, token))
		};

		let _cleanup = compositor
			.as_ref()
			.map(|(controller, token)| SemanticPlaybackGuard::new(controller.clone(), target_link_id.clone(), *token, release.clone()));
		let playback = async {
			let mut frames_sent = 0;
			frames_sent += submit_pose_phase(
				compositor.as_ref(),
				&target_link_id,
				&release,
				&pose,
				&owned_bones,
 &owned_fingers,
				attack_ms,
				frame_interval_ms,
				dry_run,
			)
			.await?;
			frames_sent += submit_pose_phase(
				compositor.as_ref(),
				&target_link_id,
				&pose,
				&pose,
				&owned_bones,
 &owned_fingers,
				hold_ms,
				frame_interval_ms,
				dry_run,
			)
			.await?;
			frames_sent += submit_pose_phase(
				compositor.as_ref(),
				&target_link_id,
				&pose,
				&release,
				&owned_bones,
 &owned_fingers,
				release_ms,
				frame_interval_ms,
				dry_run,
			)
			.await?;
			Ok::<i64, String>(frames_sent)
		}
		.await;
		let frames_sent = match playback {
			Ok(frames_sent) => frames_sent,
			Err(error) => {
				if let Some((avatar_motion, token)) = compositor.as_ref() {
					let _ = avatar_motion.end_semantic(target_link_id.clone(), *token);
				}
				log::warn!("Avatar pose asset playback failed: target_link_id={target_link_id} error={error}");
				return Ok(pose_asset_play_error(error));
			}
		};
		if let Some((avatar_motion, token)) = compositor.as_ref() {
			if let Err(error) = avatar_motion.end_semantic(target_link_id.clone(), *token) {
				log::warn!("Avatar pose asset release failed: target_link_id={target_link_id} error={error}");
				return Ok(pose_asset_play_error(error));
			}
		}

		let key_expr = strategy.key_expr_for_frame(&release);
		ctx.log(format!(
			"avatar pose asset playback: {key_expr} samples={frames_sent} attack_ms={attack_ms} hold_ms={hold_ms} release_ms={release_ms} owned_bones={} dry_run={dry_run}",
			owned_bones.len()
		));
		log::info!(
			"Avatar pose asset playback completed: target_link_id={target_link_id} key={key_expr} samples={frames_sent} attack_ms={attack_ms} hold_ms={hold_ms} release_ms={release_ms} owned_bones={} dry_run={dry_run}",
			owned_bones.len()
		);
		Ok(NodeOutput::new()
			.set_data("frames_sent", SocketValue::Int(frames_sent))
			.set_data("key_expr", SocketValue::String(key_expr))
			.set_data("error", SocketValue::String(String::new()))
			.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Int(frames_sent))))
			.fire_exec("on_success"))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::node::PureEvalHost;
	use serde_json::json;
	use std::collections::HashMap;
	use un_motion_frame::{BodyMotion, BoneSample, HumanoidBone, HumanoidPose, SampleState, TrackingState, TransformSample};

	#[tokio::test]
	async fn intent_node_accepts_explicit_model_bindings_and_reports_invalid_ones() {
		let mut inputs = HashMap::from([
			("intent".into(), SocketValue::Json(json!({"intents":[
				{"kind":"expression","name":"smile","intensity":0.4,"duration_ms":800,"curve":"hold"}
			]}))),
			("expression_bindings".into(), SocketValue::Json(json!({"smile":{"modelHappy":0.5}}))),
		]);
		let output = AvatarIntentToUnmfNode.compute(&PureEvalHost::default(), &HashMap::new(), &inputs, &ExecFireSet::new()).await.unwrap();
		assert_eq!(output.data["ok"], SocketValue::Bool(true));
		let SocketValue::Unmf(frame) = &output.data["unmf"] else { panic!("missing frame") };
		assert_eq!(frame.face.as_ref().unwrap().expressions.iter().find(|e| e.name == "modelHappy").unwrap().value, 0.2);
		inputs.insert("expression_bindings".into(), SocketValue::Json(json!({"smile":{"JawOpen":1.0}})));
		let output = AvatarIntentToUnmfNode.compute(&PureEvalHost::default(), &HashMap::new(), &inputs, &ExecFireSet::new()).await.unwrap();
		assert_eq!(output.data["ok"], SocketValue::Bool(false));
		assert!(matches!(&output.data["error"], SocketValue::String(error) if error.contains("JawOpen")));
	}

	fn pose_frame(bones: &[HumanoidBone]) -> UnMotionFrame {
		let mut frame = UnMotionFrame::new(1);
		frame.body = Some(BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: None,
				bones: bones
					.iter()
					.copied()
					.map(|bone| BoneSample {
						bone,
						transform: TransformSample {
							translation: None,
							rotation: None,
							scale: None,
							linear_velocity: None,
							angular_velocity: None,
						},
						confidence: 1.0,
						source_index: None,
						state: SampleState::Valid,
					})
					.collect(),
			}),
		});
		frame
	}

	fn pose_catalog(pose: &str) -> JsonValue {
		json!({
			"schema": "unvac.avatar_pose_catalog.v1",
			"presets": {
				"open_hands": {
					"pose": pose,
					"release": "neutral.unmf.json",
					"owned_bones": ["Head", "LeftUpperArm", "RightUpperArm"],
					"attack_ms": 250,
					"hold_ms": 1200,
					"release_ms": 300
				}
			}
		})
	}

	#[tokio::test]
	async fn pose_catalog_lookup_resolves_only_named_relative_assets() {
		let inputs = HashMap::from([
			("catalog".into(), SocketValue::Json(pose_catalog("pose.unmf.json"))),
			("preset".into(), SocketValue::String("open_hands".into())),
			("asset_root".into(), SocketValue::String("assets/avatar".into())),
		]);
		let output = AvatarPoseCatalogLookupNode
			.compute(&PureEvalHost::default(), &HashMap::new(), &inputs, &ExecFireSet::new())
			.await
			.unwrap();
		assert_eq!(output.data["ok"], SocketValue::Bool(true));
		assert_eq!(
			output.data["pose_path"],
			SocketValue::String(Path::new("assets/avatar").join("pose.unmf.json").to_string_lossy().into_owned())
		);
		assert_eq!(output.data["attack_ms"], SocketValue::Int(250));
	}

	#[tokio::test]
	async fn pose_catalog_lookup_rejects_parent_path() {
		let inputs = HashMap::from([
			("catalog".into(), SocketValue::Json(pose_catalog("../pose.unmf.json"))),
			("preset".into(), SocketValue::String("open_hands".into())),
			("asset_root".into(), SocketValue::String("assets/avatar".into())),
		]);
		let output = AvatarPoseCatalogLookupNode
			.compute(&PureEvalHost::default(), &HashMap::new(), &inputs, &ExecFireSet::new())
			.await
			.unwrap();
		assert_eq!(output.data["ok"], SocketValue::Bool(false));
		assert!(output.data["error"].as_str().unwrap().contains("normalized relative path"));
	}

	#[tokio::test]
	async fn intent_to_unmf_node_maps_expression() {
		let mut inputs = HashMap::new();
		inputs.insert(
			"intent".into(),
			SocketValue::Json(json!({
				"target_link_id": "kaltsit_ai",
				"intents": [
					{"kind": "expression", "name": "concerned", "intensity": 0.45, "duration_ms": 1200, "curve": "hold"}
				]
			})),
		);
		let out = AvatarIntentToUnmfNode
			.compute(&PureEvalHost::default(), &HashMap::new(), &inputs, &ExecFireSet::default())
			.await
			.unwrap();
		assert_eq!(out.data.get("ok"), Some(&SocketValue::Bool(true)));
		assert_eq!(out.data.get("target_link_id"), Some(&SocketValue::String("kaltsit_ai".into())));
		assert!(out.data.get("unmf").unwrap().as_unmf().is_ok());
	}

	#[tokio::test]
	async fn intent_to_unmf_node_returns_result_error_for_invalid_intent() {
		let mut inputs = HashMap::new();
		inputs.insert(
			"intent".into(),
			SocketValue::Json(json!({
				"intents": [
					{"kind": "expression", "name": "unknown_expression", "intensity": 0.5}
				]
			})),
		);
		let out = AvatarIntentToUnmfNode
			.compute(&PureEvalHost::default(), &HashMap::new(), &inputs, &ExecFireSet::default())
			.await
			.unwrap();
		assert_eq!(out.data.get("ok"), Some(&SocketValue::Bool(false)));
		assert!(out
			.data
			.get("error")
			.unwrap()
			.as_str()
			.unwrap()
			.contains("unknown avatar expression"));
	}

	#[tokio::test]
	async fn intent_player_emits_timed_frames_and_neutral_in_dry_run() {
		let mut inputs = HashMap::new();
		inputs.insert(
			"intent".into(),
			SocketValue::Json(json!({
				"target_link_id": "kaltsit_ai",
				"intents": [
					{"kind": "head_nod", "count": 1, "intensity": 0.35, "duration_ms": 100, "curve": "ease_out"}
				]
			})),
		);
		inputs.insert("base_key_expr".into(), SocketValue::String("un-motion/frame/kaltsit-ai".into()));
		inputs.insert("topic_mode".into(), SocketValue::String("frame".into()));
		inputs.insert("schema_major".into(), SocketValue::Int(1));
		inputs.insert("fps".into(), SocketValue::Int(20));
		inputs.insert("dry_run".into(), SocketValue::Bool(true));
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");

		let output = AvatarIntentPlayUnmfzNode::default()
			.execute(&mut ExecCtx::default(), &HashMap::new(), &inputs, &fired)
			.await
			.unwrap();

		assert_eq!(output.data.get("frames_sent"), Some(&SocketValue::Int(8)));
		assert_eq!(
			output.data.get("key_expr"),
			Some(&SocketValue::String("un-motion/frame/kaltsit-ai/v1".into()))
		);
		assert!(output.fired_exec.contains("on_success"));
	}

	#[tokio::test]
	async fn finger_pose_player_requires_release_before_dry_run() {
		let pose=crate::avatar_hand_pose::tests::frame();
		let mut release=pose.clone();release.left_hand=None;
		let mut inputs=HashMap::new();
		inputs.insert("pose".into(),SocketValue::Unmf(pose));
		inputs.insert("release".into(),SocketValue::Unmf(release));
		inputs.insert("owned_bones".into(),SocketValue::String("LeftUpperArm".into()));
		inputs.insert("owned_fingers".into(),SocketValue::String("left.index".into()));
		inputs.insert("target_link_id".into(),SocketValue::String("fixture".into()));
		inputs.insert("dry_run".into(),SocketValue::Bool(true));
		let mut fired=ExecFireSet::new();fired.insert("exec_in");
		let output=AvatarPoseAssetPlayUnmfzNode::default().execute(&mut ExecCtx::default(),&HashMap::new(),&inputs,&fired).await.unwrap();
		assert!(output.fired_exec.contains("on_error"));
		assert!(output.data["error"].as_str().unwrap().contains("release: missing hand"));
		inputs.insert("release".into(),SocketValue::Unmf(crate::avatar_hand_pose::tests::frame()));
		let output=AvatarPoseAssetPlayUnmfzNode::default().execute(&mut ExecCtx::default(),&HashMap::new(),&inputs,&fired).await.unwrap();
		assert!(output.fired_exec.contains("on_success"));
	}

	#[tokio::test]
	async fn pose_asset_player_requires_release_for_every_owned_bone() {
		let mut inputs = HashMap::new();
		inputs.insert(
			"pose".into(),
			SocketValue::Unmf(pose_frame(&[HumanoidBone::Head, HumanoidBone::LeftUpperArm])),
		);
		inputs.insert("release".into(), SocketValue::Unmf(pose_frame(&[HumanoidBone::Head])));
		inputs.insert("owned_bones".into(), SocketValue::String("Head,LeftUpperArm".into()));
		inputs.insert("target_link_id".into(), SocketValue::String("kaltsit_ai".into()));
		inputs.insert("dry_run".into(), SocketValue::Bool(true));
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");

		let output = AvatarPoseAssetPlayUnmfzNode::default()
			.execute(&mut ExecCtx::default(), &HashMap::new(), &inputs, &fired)
			.await
			.unwrap();

		assert!(output.fired_exec.contains("on_error"));
		assert!(output.data["error"]
			.as_str()
			.unwrap()
			.contains("release UNMF does not contain owned bone LeftUpperArm"));
	}

	#[tokio::test]
	async fn pose_asset_player_counts_pose_and_release_samples_in_dry_run() {
		let frame = pose_frame(&[HumanoidBone::Head, HumanoidBone::LeftUpperArm]);
		let mut inputs = HashMap::new();
		inputs.insert("pose".into(), SocketValue::Unmf(frame.clone()));
		inputs.insert("release".into(), SocketValue::Unmf(frame));
		inputs.insert("owned_bones".into(), SocketValue::String("Head,LeftUpperArm".into()));
		inputs.insert("target_link_id".into(), SocketValue::String("kaltsit_ai".into()));
		inputs.insert("base_key_expr".into(), SocketValue::String("un-motion/frame/kaltsit-ai".into()));
		inputs.insert("topic_mode".into(), SocketValue::String("frame".into()));
		inputs.insert("schema_major".into(), SocketValue::Int(1));
		inputs.insert("fps".into(), SocketValue::Int(20));
		inputs.insert("hold_ms".into(), SocketValue::Int(100));
		inputs.insert("release_ms".into(), SocketValue::Int(100));
		inputs.insert("dry_run".into(), SocketValue::Bool(true));
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");

		let mut ctx = ExecCtx::default();
		let output = AvatarPoseAssetPlayUnmfzNode::default()
			.execute(&mut ctx, &HashMap::new(), &inputs, &fired)
			.await
			.unwrap();

		assert_eq!(output.data.get("frames_sent"), Some(&SocketValue::Int(12)));
		assert_eq!(
			output.data.get("key_expr"),
			Some(&SocketValue::String("un-motion/frame/kaltsit-ai/v1".into()))
		);
		assert!(output.fired_exec.contains("on_success"));
		assert!(ctx.trace.iter().any(|line| line.contains("owned_bones=2")));
	}

	#[test]
	#[ignore = "offline research export requires locally authored A pose/release assets"]
	fn export_greeting_transition_for_offline_retarget() {
		let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/greeting-study");
		let read = |name: &str| -> UnMotionFrame {
			serde_json::from_slice(&std::fs::read(root.join(name)).unwrap()).unwrap()
		};
		let pose = read("candidate-a-unmf-offline.json");
		let release = read("candidate-a-release-offline.json");
		let bones = [HumanoidBone::LeftShoulder, HumanoidBone::LeftUpperArm, HumanoidBone::LeftLowerArm, HumanoidBone::LeftHand];
		let fingers = crate::avatar_hand_pose::parse("left.thumb,left.index,left.middle,left.ring,left.little").unwrap();
		let mut frames = Vec::new();
		for (phase, from, to) in [("attack", &release, &pose), ("release", &pose, &release)] {
			for elapsed_ms in (0..=1000).step_by(50) {
				let frame = blend_owned_pose_with_fingers(from, to, &bones, &fingers, eased_progress(elapsed_ms, 1000)).unwrap();
				crate::avatar_hand_pose::validate(&frame, &fingers, phase).unwrap();
				assert!(frame.face.is_none() && frame.eyes.is_none() && frame.right_hand.is_none());
				assert!(frame.left_hand.as_ref().unwrap().wrist.is_none());
				frames.push(serde_json::json!({"phase":phase,"elapsed_ms":elapsed_ms,"frame":frame}));
			}
		}
		std::fs::write(root.join("vac-transition-frames.json"), serde_json::to_vec_pretty(&frames).unwrap()).unwrap();
	}

	#[test]
	fn finger_pose_interpolates_and_returns_only_explicit_chains() {
		let release=crate::avatar_hand_pose::tests::frame();
		let mut pose=release.clone();
		for joint in &mut pose.left_hand.as_mut().unwrap().fingers[0].joints {
			joint.rotation=Some(Quatf{x:0.,y:0.,z:1.,w:0.});
		}
		let mask=crate::avatar_hand_pose::parse("left.index").unwrap();
		let bones=[HumanoidBone::LeftUpperArm];
		let mid=blend_owned_pose_with_fingers(&release,&pose,&bones,&mask,0.5).unwrap();
		let hand=mid.left_hand.as_ref().unwrap();
		assert_eq!(hand.fingers.len(),1);
		assert!(hand.wrist.is_none() && mid.right_hand.is_none());
		let q=hand.fingers[0].joints[0].rotation.unwrap();
		assert!((q.z-std::f32::consts::FRAC_1_SQRT_2).abs()<1e-5);
		assert!((q.w-std::f32::consts::FRAC_1_SQRT_2).abs()<1e-5);
		let end=blend_owned_pose_with_fingers(&pose,&release,&bones,&mask,1.).unwrap();
		assert_eq!(end.left_hand.unwrap().fingers[0],release.left_hand.unwrap().fingers[0]);
	}

	#[test]
	fn pose_asset_blend_slerps_owned_bone_and_drops_unowned_bones() {
		let mut release = pose_frame(&[HumanoidBone::Head, HumanoidBone::LeftUpperArm]);
		let mut pose = release.clone();
		let release_bones = &mut release.body.as_mut().unwrap().humanoid.as_mut().unwrap().bones;
		release_bones[0].transform.rotation = Some(Quatf {
			x: 0.0,
			y: 0.0,
			z: 0.0,
			w: 1.0,
		});
		let pose_bones = &mut pose.body.as_mut().unwrap().humanoid.as_mut().unwrap().bones;
		pose_bones[0].transform.rotation = Some(Quatf {
			x: 0.0,
			y: 1.0,
			z: 0.0,
			w: 0.0,
		});

		let blended = blend_owned_pose(&release, &pose, &[HumanoidBone::Head], 0.5).unwrap();
		let bones = &blended.body.unwrap().humanoid.unwrap().bones;
		assert_eq!(bones.len(), 1);
		assert_eq!(bones[0].bone, HumanoidBone::Head);
		let rotation = bones[0].transform.rotation.unwrap();
		assert!((rotation.y - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.0001);
		assert!((rotation.w - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.0001);
	}
}

#[async_trait]
impl EffectfulNode for AvatarIntentPlayUnmfzNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if fired_exec.contains(super::avatar_jobs::RESUME) {
			let id = get_optional_int(inputs, super::avatar_jobs::JOB_ID, -1)?;
			return self.jobs.finish(&ctx.node_id, id as u64).await;
		}
		if fired_exec.contains("exec_in") && !get_optional_bool(inputs, "dry_run", false)? {
			if let Some(bus) = ctx.trigger.clone() {
				let runner = Self {
					next_sequence: self.next_sequence.clone(),
					..Self::default()
				};
				let mut task_ctx = ExecCtx {
					state_handle: ctx.state_handle.clone(),
					node_id: ctx.node_id.clone(),
					..Default::default()
				};
				let inputs = inputs.clone();
				let resume_inputs = inputs.clone();
				let props = props.clone();
				let fired = fired_exec.clone();
				let work = async move { runner.run_inline(&mut task_ctx, &props, &inputs, &fired).await };
				return Ok(match self.jobs.start_with_inputs(ctx.node_id.clone(), bus, resume_inputs, work) {
					Ok(()) => NodeOutput::new(),
					Err(error) => avatar_play_error(error),
				});
			}
		}
		self.run_inline(ctx, props, inputs, fired_exec).await
	}
}

#[async_trait]
impl EffectfulNode for AvatarPoseAssetPlayUnmfzNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if fired_exec.contains(super::avatar_jobs::RESUME) {
			let id = get_optional_int(inputs, super::avatar_jobs::JOB_ID, -1)?;
			return self.jobs.finish(&ctx.node_id, id as u64).await;
		}
		if fired_exec.contains("exec_in") && !get_optional_bool(inputs, "dry_run", false)? {
			if let Some(bus) = ctx.trigger.clone() {
				let runner = Self::default();
				let mut task_ctx = ExecCtx {
					state_handle: ctx.state_handle.clone(),
					node_id: ctx.node_id.clone(),
					..Default::default()
				};
				let inputs = inputs.clone();
				let resume_inputs = inputs.clone();
				let props = props.clone();
				let fired = fired_exec.clone();
				let work = async move { runner.run_inline(&mut task_ctx, &props, &inputs, &fired).await };
				return Ok(match self.jobs.start_with_inputs(ctx.node_id.clone(), bus, resume_inputs, work) {
					Ok(()) => NodeOutput::new(),
					Err(error) => pose_asset_play_error(error),
				});
			}
		}
		self.run_inline(ctx, props, inputs, fired_exec).await
	}
}
