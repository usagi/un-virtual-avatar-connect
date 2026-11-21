use rodio::Source;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use un_motion_frame::{
	BodyMotion, BoneSample, CoordinateSpace, ExpressionSample, FaceMotion, Handedness, HumanoidBone, HumanoidPose, LengthUnit,
	MotionExtension, MotionHeader, MotionMetadata, MotionSourceInfo, MotionSourceKind, Quatf, SampleState, TrackingState, TransformSample,
	UNMotionFrame,
};
use un_motion_frame_zenoh::{Publisher, TopicMode, ZenohSessionBackend, ZenohTopicStrategy};

const FRAME_RATE: u32 = 30;
const FRAME_INTERVAL: Duration = Duration::from_millis(1_000 / FRAME_RATE as u64);

#[derive(Debug, Clone)]
pub(crate) struct AvatarMotionConfig {
	pub base_key_expr: String,
	pub expression: String,
	pub target_link_id: String,
	pub gain: f32,
}

#[derive(Debug, Clone)]
pub(crate) struct AvatarMotionTarget {
	pub target_link_id: String,
	pub base_key_expr: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AvatarSemanticOwnership {
	/// `None`は従来のAvatarIntent用mask。`Some`はpose assetが所有するboneの厳密な集合。
	pub owned_bones: Option<Vec<HumanoidBone>>,
	pub owns_root: bool,
	pub owned_fingers: Vec<crate::avatar_hand_pose::OwnedFinger>,
}

impl AvatarMotionConfig {
	pub fn enabled(&self) -> bool {
		!self.base_key_expr.trim().is_empty()
	}
}

enum MotionCommand {
	Prepare(AvatarMotionConfig),
	Play {
		config: AvatarMotionConfig,
		levels: Vec<f32>,
	},
	BeginSemantic {
		base_key_expr: String,
		target_link_id: String,
		ownership: AvatarSemanticOwnership,
		respond_to: oneshot::Sender<Result<u64, String>>,
	},
	SetSemantic {
		target_link_id: String,
		token: u64,
		frame: UNMotionFrame,
	},
	EndSemantic {
		target_link_id: String,
		token: u64,
	},
}

#[derive(Debug, Clone)]
pub(crate) struct AvatarMotionController {
	tx: mpsc::UnboundedSender<MotionCommand>,
	targets: Arc<RwLock<HashMap<String, AvatarMotionTarget>>>,
}

/// Restores only this playback's layer on normal return, error, or task cancellation.
/// The compositor ignores obsolete tokens, so an old guard cannot reset a newer pose.
pub(crate) struct SemanticPlaybackGuard {
	controller: AvatarMotionController,
	target: String,
	token: u64,
	neutral: UNMotionFrame,
}

impl SemanticPlaybackGuard {
	pub fn new(controller: AvatarMotionController, target: String, token: u64, neutral: UNMotionFrame) -> Self {
		Self {
			controller,
			target,
			token,
			neutral,
		}
	}
}

impl Drop for SemanticPlaybackGuard {
	fn drop(&mut self) {
		let _ = self
			.controller
			.set_semantic_frame(self.target.clone(), self.token, self.neutral.clone());
		let _ = self.controller.end_semantic(self.target.clone(), self.token);
	}
}

impl AvatarMotionController {
	#[cfg(test)]
	pub fn spawn() -> Self {
		Self::spawn_with_transport(crate::unmfz_transport::UnmfzTransportRuntime::default())
	}

	pub fn spawn_with_transport(transport: crate::unmfz_transport::UnmfzTransportRuntime) -> Self {
		Self::spawn_with_blink_policy(transport, HashMap::new())
	}

	pub fn spawn_with_blink_policy(
		transport: crate::unmfz_transport::UnmfzTransportRuntime,
		blink_policy: HashMap<String, Vec<String>>,
	) -> Self {
		let (tx, rx) = mpsc::unbounded_channel();
		tokio::spawn(run_motion_loop(rx, transport, blink_policy));
		Self {
			tx,
			targets: Arc::new(RwLock::new(HashMap::new())),
		}
	}

	pub fn prepare(&self, config: AvatarMotionConfig) -> Result<(), String> {
		if !config.enabled() {
			return Ok(());
		}
		let target = config.clone();
		self.tx
			.send(MotionCommand::Prepare(config))
			.map_err(|_| "avatar motion controller has stopped".to_string())?;
		self.record_target(&target);
		Ok(())
	}

	pub fn start_lip_sync(&self, wav: &[u8], config: AvatarMotionConfig) -> Result<(), String> {
		if !config.enabled() {
			return Ok(());
		}
		let levels = wav_envelope(wav, FRAME_RATE, config.gain)?;
		let target = config.clone();
		self.tx
			.send(MotionCommand::Play { config, levels })
			.map_err(|_| "avatar motion controller has stopped".to_string())?;
		self.record_target(&target);
		Ok(())
	}

	pub async fn begin_semantic(&self, base_key_expr: String, target_link_id: String) -> Result<u64, String> {
		self.begin_semantic_with_ownership(base_key_expr, target_link_id, AvatarSemanticOwnership::default())
			.await
	}

	pub async fn begin_semantic_with_ownership(
		&self,
		base_key_expr: String,
		target_link_id: String,
		ownership: AvatarSemanticOwnership,
	) -> Result<u64, String> {
		if base_key_expr.trim().is_empty() {
			return Err("avatar motion base key is empty".into());
		}
		if target_link_id.trim().is_empty() {
			return Err("AvatarIntent target_link_id is required for motion composition".into());
		}
		let (respond_to, response) = oneshot::channel();
		let target = AvatarMotionConfig {
			base_key_expr: base_key_expr.clone(),
			expression: "JawOpen".into(),
			target_link_id: target_link_id.clone(),
			gain: 1.0,
		};
		self.tx
			.send(MotionCommand::BeginSemantic {
				base_key_expr,
				target_link_id,
				ownership,
				respond_to,
			})
			.map_err(|_| "avatar motion controller has stopped".to_string())?;
		self.record_target(&target);
		response
			.await
			.map_err(|_| "avatar motion controller did not start semantic motion".to_string())?
	}

	pub fn set_semantic_frame(&self, target_link_id: String, token: u64, frame: UNMotionFrame) -> Result<(), String> {
		self.tx
			.send(MotionCommand::SetSemantic {
				target_link_id,
				token,
				frame,
			})
			.map_err(|_| "avatar motion controller has stopped".to_string())
	}

	pub fn end_semantic(&self, target_link_id: String, token: u64) -> Result<(), String> {
		self.tx
			.send(MotionCommand::EndSemantic { target_link_id, token })
			.map_err(|_| "avatar motion controller has stopped".to_string())
	}

	pub fn configured_targets(&self) -> Vec<AvatarMotionTarget> {
		let mut targets = self
			.targets
			.read()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
			.values()
			.cloned()
			.collect::<Vec<_>>();
		targets.sort_by(|a, b| a.target_link_id.cmp(&b.target_link_id));
		targets
	}

	fn record_target(&self, config: &AvatarMotionConfig) {
		let target = AvatarMotionTarget {
			target_link_id: config.target_link_id.trim().to_string(),
			base_key_expr: config.base_key_expr.trim().trim_end_matches('/').to_string(),
		};
		self.targets
			.write()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
			.insert(target.target_link_id.clone(), target);
	}
}

struct AvatarMotionPublisher {
	publisher: Publisher<ZenohSessionBackend>,
	base_key_expr: String,
	next_sequence: u64,
}

impl AvatarMotionPublisher {
	fn open(base_key_expr: &str, transport: &crate::unmfz_transport::UnmfzTransportRuntime) -> Result<Self, String> {
		let base_key_expr = base_key_expr.trim().trim_end_matches('/');
		let backend = transport.publisher_backend()?;
		let strategy = ZenohTopicStrategy::new(base_key_expr, TopicMode::Frame).with_schema_major(1);
		Ok(Self {
			publisher: Publisher::new(backend).with_strategy(strategy),
			base_key_expr: base_key_expr.to_string(),
			next_sequence: 1,
		})
	}

	fn matches_base_key(&self, base_key_expr: &str) -> bool {
		self.base_key_expr == base_key_expr.trim().trim_end_matches('/')
	}

	fn publish(
		&mut self,
		config: &AvatarMotionConfig,
		mouth: f32,
		blink: f32,
		semantic_layers: &[(&UNMotionFrame, &AvatarSemanticOwnership)],
		blink_suppressed: &[String],
	) -> Result<(), String> {
		let mut frame = compose_avatar_frame(
			self.next_sequence,
			&config.target_link_id,
			&config.expression,
			mouth.clamp(0.0, 1.0),
			blink.clamp(0.0, 1.0),
			semantic_layers,
		);
		attenuate_blink_conflicts(&mut frame, blink, blink_suppressed);
		self.next_sequence = self.next_sequence.wrapping_add(1);
		self.publisher.send(&frame).map_err(|error| error.to_string())
	}
}

struct MotionTargetState {
	config: AvatarMotionConfig,
	publisher: AvatarMotionPublisher,
	levels: Vec<f32>,
	level_index: usize,
	blink: BlinkClock,
	semantic_layers: Vec<SemanticLayer>,
}

struct SemanticLayer {
	token: u64,
	frame: Option<UNMotionFrame>,
	ownership: AvatarSemanticOwnership,
	clear_after: Option<Instant>,
}

impl SemanticLayer {
	fn retain_after_publish(&self, now: Instant, published: bool) -> bool {
		!published || !self.clear_after.is_some_and(|deadline| now >= deadline)
	}

	fn priority(&self) -> u8 {
		if self.ownership.owned_bones.is_some() {
			10
		} else {
			20
		}
	}

	fn conflicts_with(&self, ownership: &AvatarSemanticOwnership) -> bool {
		self.ownership.owned_bones.is_some() == ownership.owned_bones.is_some()
	}
}

fn motion_target_key(config: &AvatarMotionConfig) -> String {
	config.target_link_id.trim().to_string()
}

fn blink_seed(config: &AvatarMotionConfig) -> u64 {
	let mut hash = 0xcbf2_9ce4_8422_2325u64;
	for byte in format!(
		"{}\0{}",
		config.base_key_expr.trim().trim_end_matches('/'),
		motion_target_key(config)
	)
	.bytes()
	{
		hash ^= u64::from(byte);
		hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
	}
	hash
}

impl MotionTargetState {
	fn open(config: AvatarMotionConfig, transport: &crate::unmfz_transport::UnmfzTransportRuntime) -> Result<Self, String> {
		let publisher = AvatarMotionPublisher::open(&config.base_key_expr, transport)?;
		let blink = BlinkClock::new_with_seed(Instant::now(), blink_seed(&config));
		Ok(Self {
			config,
			publisher,
			levels: Vec::new(),
			level_index: 0,
			blink,
			semantic_layers: Vec::new(),
		})
	}

	fn prepare(&mut self, config: AvatarMotionConfig) {
		self.config = config;
		self.levels.clear();
		self.level_index = 0;
	}

	fn play(&mut self, config: AvatarMotionConfig, levels: Vec<f32>) {
		self.config = config;
		self.levels = levels;
		self.level_index = 0;
	}
}

fn ensure_target<'a>(
	targets: &'a mut HashMap<String, MotionTargetState>,
	config: &AvatarMotionConfig,
	transport: &crate::unmfz_transport::UnmfzTransportRuntime,
) -> Result<&'a mut MotionTargetState, String> {
	let key = motion_target_key(config);
	let replace_target = targets
		.get(&key)
		.is_none_or(|target| !target.publisher.matches_base_key(&config.base_key_expr));
	if replace_target {
		targets.insert(key.clone(), MotionTargetState::open(config.clone(), transport)?);
	}
	Ok(targets.get_mut(&key).expect("motion target inserted"))
}

struct BlinkClock {
	next_blink: Instant,
	started: Option<Instant>,
	rng: u64,
}

impl BlinkClock {
	fn new_with_seed(now: Instant, seed: u64) -> Self {
		let mut clock = Self {
			next_blink: now,
			started: None,
			rng: seed,
		};
		clock.schedule_regular(now);
		clock
	}

	fn value(&mut self, now: Instant) -> f32 {
		if self.started.is_none() && now >= self.next_blink {
			self.started = Some(now);
		}
		let Some(started) = self.started else {
			return 0.0;
		};
		let elapsed_ms = now.saturating_duration_since(started).as_secs_f32() * 1_000.0;
		let value = if elapsed_ms < 45.0 {
			smoothstep(elapsed_ms / 45.0)
		} else if elapsed_ms < 70.0 {
			1.0
		} else if elapsed_ms < 170.0 {
			1.0 - smoothstep((elapsed_ms - 70.0) / 100.0)
		} else {
			self.started = None;
			if self.next_random() % 8 == 0 {
				self.next_blink = now + Duration::from_millis(180);
			} else {
				self.schedule_regular(now);
			}
			0.0
		};
		value
	}

	fn schedule_regular(&mut self, now: Instant) {
		let delay_ms = 2_400 + self.next_random() % 4_101;
		self.next_blink = now + Duration::from_millis(delay_ms);
	}

	fn next_random(&mut self) -> u64 {
		self.rng = self
			.rng
			.wrapping_mul(6_364_136_223_846_793_005)
			.wrapping_add(1_442_695_040_888_963_407);
		self.rng >> 32
	}
}

async fn run_motion_loop(
	mut rx: mpsc::UnboundedReceiver<MotionCommand>,
	transport: crate::unmfz_transport::UnmfzTransportRuntime,
	blink_policy: HashMap<String, Vec<String>>,
) {
	let mut targets = HashMap::<String, MotionTargetState>::new();
	let mut next_semantic_token = 1u64;
	let mut interval = tokio::time::interval(FRAME_INTERVAL);
	interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

	loop {
		tokio::select! {
			command = rx.recv() => {
				let Some(command) = command else { return; };
				match command {
					MotionCommand::Prepare(config) => {
						match ensure_target(&mut targets, &config, &transport) {
							Ok(target) => target.prepare(config),
							Err(error) => log::warn!("《Avatar Motion Compositor》 {error}"),
						}
					}
					MotionCommand::Play { config, levels } => {
						match ensure_target(&mut targets, &config, &transport) {
							Ok(target) => target.play(config, levels),
							Err(error) => log::warn!("《Avatar Motion Compositor》 {error}"),
						}
					}
					MotionCommand::BeginSemantic { base_key_expr, target_link_id, ownership, respond_to } => {
						let config = AvatarMotionConfig {
							base_key_expr,
							expression: "JawOpen".into(),
							target_link_id: target_link_id.trim().to_string(),
							gain: 1.0,
						};
						let token = next_semantic_token;
						next_semantic_token = next_semantic_token.wrapping_add(1).max(1);
						let target = match ensure_target(&mut targets, &config, &transport) {
							Ok(target) => target,
							Err(error) => {
								let _ = respond_to.send(Err(error));
								continue;
							}
						};
					target.semantic_layers.retain(|layer| !layer.conflicts_with(&ownership));
					target.semantic_layers.push(SemanticLayer {
						token,
						frame: None,
						ownership,
						clear_after: None,
					});
						let _ = respond_to.send(Ok(token));
					}
					MotionCommand::SetSemantic { target_link_id, token, frame } => {
						if let Some(layer) = targets
							.get_mut(target_link_id.trim())
							.and_then(|target| target.semantic_layers.iter_mut().find(|layer| layer.token == token))
						{
							layer.frame = Some(frame);
							layer.clear_after = None;
						}
					}
					MotionCommand::EndSemantic { target_link_id, token } => {
						if let Some(layer) = targets
							.get_mut(target_link_id.trim())
							.and_then(|target| target.semantic_layers.iter_mut().find(|layer| layer.token == token))
						{
							layer.clear_after = Some(Instant::now() + FRAME_INTERVAL);
						}
					}
				}
			}
			_ = interval.tick() => {
				let now = Instant::now();
				for target in targets.values_mut() {
					let mouth = target.levels.get(target.level_index).copied().unwrap_or(0.0);
					if target.level_index < target.levels.len() {
						target.level_index += 1;
					}
					let blink_value = target.blink.value(now);
					let mut semantic_layers = target
						.semantic_layers
						.iter()
						.filter_map(|layer| layer.frame.as_ref().map(|frame| (layer.priority(), frame, &layer.ownership)))
						.collect::<Vec<_>>();
					semantic_layers.sort_by_key(|(priority, _, _)| *priority);
					let semantic_layers = semantic_layers
						.iter()
						.map(|(_, frame, ownership)| (*frame, *ownership))
						.collect::<Vec<_>>();
					let published = match target
						.publisher
						.publish(&target.config, mouth, blink_value, &semantic_layers,
							blink_policy.get(&target.config.target_link_id).map(Vec::as_slice).unwrap_or(&[]))
					{
						Ok(()) => true,
						Err(error) => {
							log::warn!("《Avatar Motion Compositor》 {error}");
							false
						}
					};
					// Keep the final neutral frame available for retry after a send failure.
					target
						.semantic_layers
						.retain(|layer| layer.retain_after_publish(now, published));
				}
			}
		}
	}
}

fn smoothstep(value: f32) -> f32 {
	let t = value.clamp(0.0, 1.0);
	t * t * (3.0 - 2.0 * t)
}

fn wav_envelope(wav: &[u8], fps: u32, gain: f32) -> Result<Vec<f32>, String> {
	let decoder = rodio::Decoder::new(Cursor::new(wav.to_vec())).map_err(|error| format!("lip-sync WAV decode: {error}"))?;
	let sample_rate = decoder.sample_rate().get() as usize;
	let channels = decoder.channels().get() as usize;
	let samples_per_window = ((sample_rate * channels) / fps.clamp(1, 120) as usize).max(channels);
	let samples = decoder.collect::<Vec<_>>();
	if samples.is_empty() {
		return Ok(vec![0.0]);
	}

	let mut rms = samples
		.chunks(samples_per_window)
		.map(|window| {
			let sum_sq = window.iter().map(|sample| sample * sample).sum::<f32>();
			(sum_sq / window.len() as f32).sqrt()
		})
		.collect::<Vec<_>>();
	let peak = rms.iter().copied().fold(0.0f32, f32::max);
	if peak <= 1.0e-6 {
		return Ok(vec![0.0; rms.len()]);
	}

	let gain = gain.clamp(0.1, 4.0);
	let gate = peak * 0.035;
	let mut smoothed = 0.0f32;
	for value in &mut rms {
		let target = if *value <= gate {
			0.0
		} else {
			((*value - gate) / (peak - gate).max(1.0e-6)).sqrt() * 0.9 * gain
		}
		.clamp(0.0, 1.0);
		let blend = if target >= smoothed { 0.65 } else { 0.35 };
		smoothed += (target - smoothed) * blend;
		*value = smoothed;
	}
	Ok(rms)
}

fn compose_avatar_frame(
	sequence: u64,
	target_link_id: &str,
	expression: &str,
	mouth: f32,
	blink: f32,
	semantic_layers: &[(&UNMotionFrame, &AvatarSemanticOwnership)],
) -> UNMotionFrame {
	let mut frame = UNMotionFrame::new(sequence);
	frame.header = MotionHeader::new(sequence);
	frame.header.coordinate_space = CoordinateSpace::UNMotion;
	frame.header.handedness = Handedness::RightHanded;
	frame.header.length_unit = LengthUnit::Meter;
	frame.sources.push(MotionSourceInfo {
		source_id: target_link_id.to_string(),
		source_kind: MotionSourceKind::Manual,
		display_name: Some("UNVAC avatar motion compositor".into()),
		confidence: 1.0,
		latency_ns: None,
		state: TrackingState::Valid,
	});
	frame.face = Some(FaceMotion {
		tracking_state: TrackingState::Valid,
		confidence: 1.0,
		head: None,
		expressions: [(expression, mouth), ("EyeBlinkLeft", blink), ("EyeBlinkRight", blink)]
			.into_iter()
			.map(|(name, value)| ExpressionSample {
				name: name.to_string(),
				value,
				confidence: 1.0,
				source_index: Some(0),
				state: SampleState::Valid,
			})
			.collect(),
	});
	frame.body = Some(BodyMotion {
		tracking_state: TrackingState::Valid,
		confidence: 1.0,
		humanoid: Some(HumanoidPose {
			root: None,
			bones: vec![
				identity_bone(HumanoidBone::Head),
				arm_bone(HumanoidBone::LeftUpperArm, 1.0),
				identity_bone(HumanoidBone::LeftLowerArm),
				identity_bone(HumanoidBone::LeftHand),
				arm_bone(HumanoidBone::RightUpperArm, -1.0),
				identity_bone(HumanoidBone::RightLowerArm),
				identity_bone(HumanoidBone::RightHand),
			],
		}),
	});
	frame.metadata = MotionMetadata {
		producer: Some("un-virtual-avatar-connect/avatar-motion".into()),
		extensions: vec![MotionExtension {
			namespace: "network.usagi.unvac.avatar_motion".into(),
			name: "composed_avatar_motion".into(),
			value: "v1".into(),
		}],
		..MotionMetadata::default()
	};
	for (semantic, ownership) in semantic_layers {
		merge_semantic_layer(&mut frame, semantic, expression, ownership);
	}
	frame
}

fn attenuate_blink_conflicts(frame: &mut UNMotionFrame, blink: f32, presets: &[String]) {
	if let Some(face) = frame.face.as_mut() {
		for sample in &mut face.expressions {
			if presets.iter().any(|preset| vac_motion::avatar_intent::same_preset_name(preset, &sample.name)) {
				sample.value *= 1.0 - blink.clamp(0.0, 1.0);
			}
		}
	}
}

fn merge_semantic_layer(frame: &mut UNMotionFrame, semantic: &UNMotionFrame, mouth_expression: &str, ownership: &AvatarSemanticOwnership) {
	if let Some(semantic_face) = semantic.face.as_ref() {
		let face = frame.face.get_or_insert_with(|| FaceMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			head: None,
			expressions: Vec::new(),
		});
		if semantic_face.head.is_some() {
			face.head = semantic_face.head.clone();
		}
		for expression in &semantic_face.expressions {
			if vac_motion::avatar_intent::same_preset_name(&expression.name, mouth_expression)
				|| vac_motion::avatar_intent::same_preset_name(&expression.name, "EyeBlinkLeft")
				|| vac_motion::avatar_intent::same_preset_name(&expression.name, "EyeBlinkRight")
			{
				continue;
			}
			if let Some(current) = face
				.expressions
				.iter_mut()
				.find(|current| vac_motion::avatar_intent::same_preset_name(&current.name, &expression.name))
			{
				*current = expression.clone();
			} else {
				face.expressions.push(expression.clone());
			}
		}
	}

	crate::avatar_hand_pose::merge(frame, semantic, &ownership.owned_fingers);

	let Some(semantic_pose) = semantic.body.as_ref().and_then(|body| body.humanoid.as_ref()) else {
		return;
	};
	let body = frame.body.get_or_insert_with(|| BodyMotion {
		tracking_state: TrackingState::Valid,
		confidence: 1.0,
		humanoid: Some(HumanoidPose {
			root: None,
			bones: Vec::new(),
		}),
	});
	let pose = body.humanoid.get_or_insert_with(|| HumanoidPose {
		root: None,
		bones: Vec::new(),
	});
	if ownership.owns_root && semantic_pose.root.is_some() {
		pose.root = semantic_pose.root.clone();
	}
	for bone in &semantic_pose.bones {
		let owned = ownership.owned_bones.as_ref().map_or_else(
			|| !matches!(bone.bone, HumanoidBone::LeftUpperArm | HumanoidBone::RightUpperArm),
			|bones| bones.contains(&bone.bone),
		);
		if !owned {
			continue;
		}
		if let Some(current) = pose.bones.iter_mut().find(|current| current.bone == bone.bone) {
			*current = bone.clone();
		} else {
			pose.bones.push(bone.clone());
		}
	}
}

fn arm_bone(bone: HumanoidBone, z_sign: f32) -> BoneSample {
	const HALF_SQRT_2: f32 = std::f32::consts::FRAC_1_SQRT_2;
	BoneSample {
		bone,
		transform: TransformSample {
			translation: None,
			rotation: Some(Quatf {
				x: 0.0,
				y: 0.0,
				z: HALF_SQRT_2 * z_sign,
				w: HALF_SQRT_2,
			}),
			scale: None,
			linear_velocity: None,
			angular_velocity: None,
		},
		confidence: 1.0,
		source_index: Some(0),
		state: SampleState::Valid,
	}
}

fn identity_bone(bone: HumanoidBone) -> BoneSample {
	BoneSample {
		bone,
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
	}
}

#[cfg(test)]
mod tests {
	#[test]
	fn explicit_fingers_merge_without_body_and_preserve_other_channels() {
		let mut frame = crate::avatar_hand_pose::tests::frame();
		let before = frame.clone();
		let mut source = frame.clone();
		source.body = None;
		source.left_hand.as_mut().unwrap().fingers[0].joints[0].rotation = Some(Quatf{x:0.,y:0.,z:1.,w:0.});
		let ownership = AvatarSemanticOwnership { owned_bones:Some(Vec::new()), owns_root:false,
			owned_fingers:crate::avatar_hand_pose::parse("left.index").unwrap() };
		merge_semantic_layer(&mut frame,&source,"A",&ownership);
		assert_eq!(frame.left_hand.as_ref().unwrap().fingers[0],source.left_hand.as_ref().unwrap().fingers[0]);
		assert_eq!(frame.left_hand.as_ref().unwrap().fingers[1],before.left_hand.as_ref().unwrap().fingers[1]);
		assert_eq!(frame.right_hand,before.right_hand);
		assert_eq!(frame.body,before.body);
	}

	#[test]
	fn failed_neutral_publish_retains_release_until_success_without_removing_active_pose() {
		use vac_motion::avatar_intent::{avatar_intent_neutral_unmf, AvatarIntentRequest};
		let request: AvatarIntentRequest = serde_json::from_value(serde_json::json!({"intents":[
			{"kind":"expression","name":"happy","intensity":0.6,"duration_ms":200,"curve":"hold"}
		]})).unwrap();
		let now = Instant::now();
		let mut layers = vec![
			SemanticLayer {
				token: 1,
				frame: Some(avatar_intent_neutral_unmf(&request, 1).unwrap()),
				ownership: AvatarSemanticOwnership::default(),
				clear_after: Some(now + FRAME_INTERVAL),
			},
			SemanticLayer {
				token: 2,
				frame: None,
				ownership: AvatarSemanticOwnership {
					owned_bones: Some(vec![HumanoidBone::LeftUpperArm]),
					owns_root: false,
 owned_fingers: Vec::new(),
				},
				clear_after: None,
			},
		];
		layers.retain(|layer| layer.retain_after_publish(now, true));
		assert_eq!(layers.len(), 2, "early success must not shorten the release window");
		for delay_ms in [33, 100, 1000] {
			layers.retain(|layer| layer.retain_after_publish(now + Duration::from_millis(delay_ms), false));
			assert_eq!(layers.len(), 2, "send failure must preserve neutral for retry");
			let neutral = layers[0].frame.as_ref().unwrap();
			let composed = compose_avatar_frame(2, "test", "JawOpen", 0.4, 0.7,
				&[(neutral, &layers[0].ownership)]);
			let expressions = composed.face.unwrap().expressions;
			for (name, expected) in [("Joy", 0.0), ("JawOpen", 0.4), ("EyeBlinkLeft", 0.7)] {
				assert_eq!(expressions.iter().find(|sample| sample.name == name).unwrap().value, expected);
			}
		}
		layers.retain(|layer| layer.retain_after_publish(now + Duration::from_secs(2), true));
		assert_eq!(layers.len(), 1);
		assert_eq!(layers[0].token, 2, "successful release must keep the independent active pose");
	}

	#[tokio::test]
	async fn cancelled_playback_submits_neutral_before_ending_its_own_layer() {
		let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
		let controller = super::AvatarMotionController {
			tx,
			targets: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
		};
		let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
		let neutral = crate::avatar_hand_pose::tests::frame();
		let expected_neutral = neutral.clone();
		let task = tokio::spawn(async move {
			let _cleanup = super::SemanticPlaybackGuard::new(controller, "test-avatar".into(), 42, neutral);
			ready_tx.send(()).unwrap();
			std::future::pending::<()>().await;
		});
		ready_rx.await.unwrap();
		task.abort();
		assert!(task.await.unwrap_err().is_cancelled());
		match rx.recv().await.unwrap() {
			super::MotionCommand::SetSemantic {
				target_link_id,
				token,
				frame,
			} => {
				assert_eq!(target_link_id, "test-avatar");
				assert_eq!(token, 42);
				assert_eq!(frame, expected_neutral);
			}
			_ => panic!("neutral must precede layer removal"),
		}
		assert!(
			matches!(rx.recv().await.unwrap(), super::MotionCommand::EndSemantic { target_link_id, token: 42 } if target_link_id == "test-avatar")
		);
	}
	use super::*;

	fn wav_with_samples(samples: &[i16], sample_rate: u32) -> Vec<u8> {
		let data_len = (samples.len() * 2) as u32;
		let mut wav = Vec::with_capacity(44 + data_len as usize);
		wav.extend_from_slice(b"RIFF");
		wav.extend_from_slice(&(36 + data_len).to_le_bytes());
		wav.extend_from_slice(b"WAVEfmt ");
		wav.extend_from_slice(&16u32.to_le_bytes());
		wav.extend_from_slice(&1u16.to_le_bytes());
		wav.extend_from_slice(&1u16.to_le_bytes());
		wav.extend_from_slice(&sample_rate.to_le_bytes());
		wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
		wav.extend_from_slice(&2u16.to_le_bytes());
		wav.extend_from_slice(&16u16.to_le_bytes());
		wav.extend_from_slice(b"data");
		wav.extend_from_slice(&data_len.to_le_bytes());
		for sample in samples {
			wav.extend_from_slice(&sample.to_le_bytes());
		}
		wav
	}

	#[test]
	fn envelope_tracks_quiet_and_loud_windows() {
		let samples = [vec![0i16; 100], vec![3_000i16; 100], vec![20_000i16; 100]].concat();
		let envelope = wav_envelope(&wav_with_samples(&samples, 1_000), 10, 1.0).unwrap();
		assert_eq!(envelope.len(), 3);
		assert_eq!(envelope[0], 0.0);
		assert!(envelope[2] > envelope[1]);
	}

	#[test]
	fn ambient_frame_contains_mouth_blink_and_i_pose() {
		let frame = compose_avatar_frame(7, "kaltsit_ai", "JawOpen", 0.42, 0.75, &[]);
		let expressions = &frame.face.unwrap().expressions;
		assert_eq!(expressions.len(), 3);
		assert_eq!(expressions[0].name, "JawOpen");
		assert!((expressions[0].value - 0.42).abs() < f32::EPSILON);
		assert_eq!(expressions[1].name, "EyeBlinkLeft");
		assert!((expressions[1].value - 0.75).abs() < f32::EPSILON);
		assert_eq!(expressions[2].name, "EyeBlinkRight");
		let bones = &frame.body.unwrap().humanoid.unwrap().bones;
		assert_eq!(bones.len(), 7);
		assert_eq!(bones[0].bone, HumanoidBone::Head);
		assert_eq!(bones[0].transform.rotation.as_ref().unwrap().w, 1.0);
		assert_eq!(bones[1].bone, HumanoidBone::LeftUpperArm);
		assert!(bones[1].transform.rotation.as_ref().unwrap().z > 0.0);
		assert_eq!(bones[4].bone, HumanoidBone::RightUpperArm);
		assert!(bones[4].transform.rotation.as_ref().unwrap().z < 0.0);
		assert!(bones[2..4]
			.iter()
			.chain(&bones[5..7])
			.all(|bone| bone.transform.rotation.as_ref().unwrap().w == 1.0));
	}

	#[test]
	fn semantic_layer_merges_without_overriding_articulation_or_i_pose() {
		let mut semantic = UNMotionFrame::new(99);
		semantic.face = Some(FaceMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			head: None,
			expressions: vec![
				ExpressionSample {
					name: "Joy".into(),
					value: 0.6,
					confidence: 1.0,
					source_index: Some(0),
					state: SampleState::Valid,
				},
				ExpressionSample {
					name: "JawOpen".into(),
					value: 1.0,
					confidence: 1.0,
					source_index: Some(0),
					state: SampleState::Valid,
				},
			],
		});
		semantic.body = Some(BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: None,
				bones: vec![arm_bone(HumanoidBone::LeftUpperArm, -1.0), arm_bone(HumanoidBone::Head, 1.0)],
			}),
		});

		let ownership = AvatarSemanticOwnership::default();
		let frame = compose_avatar_frame(7, "kaltsit_ai", "JawOpen", 0.42, 0.75, &[(&semantic, &ownership)]);
		let expressions = &frame.face.unwrap().expressions;
		assert_eq!(expressions.iter().find(|value| value.name == "JawOpen").unwrap().value, 0.42);
		assert_eq!(expressions.iter().find(|value| value.name == "Joy").unwrap().value, 0.6);
		let bones = &frame.body.unwrap().humanoid.unwrap().bones;
		assert_eq!(
			bones
				.iter()
				.find(|value| value.bone == HumanoidBone::LeftUpperArm)
				.unwrap()
				.transform
				.rotation
				.as_ref()
				.unwrap()
				.z
				.signum(),
			1.0
		);
		assert!(bones.iter().any(|value| value.bone == HumanoidBone::Head));
	}

	#[test]
	fn pose_asset_ownership_can_override_ambient_arms_without_overriding_articulation() {
		let mut semantic = UNMotionFrame::new(99);
		semantic.face = Some(FaceMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			head: None,
			expressions: vec![ExpressionSample {
				name: "JawOpen".into(),
				value: 1.0,
				confidence: 1.0,
				source_index: Some(0),
				state: SampleState::Valid,
			}],
		});
		semantic.body = Some(BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: None,
				bones: vec![arm_bone(HumanoidBone::LeftUpperArm, -1.0), arm_bone(HumanoidBone::Head, 1.0)],
			}),
		});
		let ownership = AvatarSemanticOwnership {
			owned_bones: Some(vec![HumanoidBone::LeftUpperArm]),
			owns_root: false,
 owned_fingers: Vec::new(),
		};

		let frame = compose_avatar_frame(7, "kaltsit_ai", "JawOpen", 0.42, 0.75, &[(&semantic, &ownership)]);
		let expressions = &frame.face.unwrap().expressions;
		assert_eq!(expressions.iter().find(|value| value.name == "JawOpen").unwrap().value, 0.42);
		let bones = &frame.body.unwrap().humanoid.unwrap().bones;
		assert_eq!(
			bones
				.iter()
				.find(|value| value.bone == HumanoidBone::LeftUpperArm)
				.unwrap()
				.transform
				.rotation
				.as_ref()
				.unwrap()
				.z
				.signum(),
			-1.0
		);
		let head = bones.iter().find(|value| value.bone == HumanoidBone::Head).unwrap();
		assert_eq!(head.transform.rotation.as_ref().unwrap().z, 0.0);
		assert_eq!(head.transform.rotation.as_ref().unwrap().w, 1.0);
	}

	#[test]
	fn intent_layer_can_override_pose_head_while_pose_keeps_arm_ownership() {
		let mut pose = UNMotionFrame::new(1);
		pose.body = Some(BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: None,
				bones: vec![arm_bone(HumanoidBone::Head, -1.0), arm_bone(HumanoidBone::LeftUpperArm, -1.0)],
			}),
		});
		let mut intent = UNMotionFrame::new(2);
		intent.body = Some(BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: None,
				bones: vec![arm_bone(HumanoidBone::Head, 1.0)],
			}),
		});
		let pose_ownership = AvatarSemanticOwnership {
			owned_bones: Some(vec![HumanoidBone::Head, HumanoidBone::LeftUpperArm]),
			owns_root: false,
 owned_fingers: Vec::new(),
		};
		let intent_ownership = AvatarSemanticOwnership::default();

		let frame = compose_avatar_frame(
			7,
			"kaltsit_ai",
			"JawOpen",
			0.42,
			0.75,
			&[(&pose, &pose_ownership), (&intent, &intent_ownership)],
		);
		let bones = &frame.body.unwrap().humanoid.unwrap().bones;
		let head = bones.iter().find(|value| value.bone == HumanoidBone::Head).unwrap();
		let arm = bones.iter().find(|value| value.bone == HumanoidBone::LeftUpperArm).unwrap();
		assert!(head.transform.rotation.as_ref().unwrap().z > 0.0);
		assert!(arm.transform.rotation.as_ref().unwrap().z < 0.0);
	}

	#[test]
	fn blink_clock_closes_and_reopens_smoothly() {
		let start = Instant::now();
		let mut clock = BlinkClock {
			next_blink: start,
			started: None,
			rng: 1,
		};
		assert_eq!(clock.value(start), 0.0);
		assert!(clock.value(start + Duration::from_millis(30)) > 0.0);
		assert_eq!(clock.value(start + Duration::from_millis(55)), 1.0);
		assert!(clock.value(start + Duration::from_millis(120)) < 1.0);
		assert_eq!(clock.value(start + Duration::from_millis(180)), 0.0);
	}

	#[test]
	fn blink_suppression_reopens_without_mutating_intent_and_keeps_other_channels() {
		use vac_motion::avatar_intent::{avatar_intent_to_unmf_with_bindings, AvatarExpressionBindings, AvatarIntentMapperOptions, AvatarIntentRequest};
		let bindings: AvatarExpressionBindings = serde_json::from_value(serde_json::json!({"smile":{"eye_smile1":1.0,"cheek":1.0}})).unwrap();
		let request: AvatarIntentRequest = serde_json::from_value(serde_json::json!({"intents":[
			{"kind":"expression","name":"smile","intensity":0.55,"duration_ms":200,"curve":"hold"}
		]})).unwrap();
		for elapsed_ms in [0, 200] {
			let intent = avatar_intent_to_unmf_with_bindings(&request,
				&AvatarIntentMapperOptions { elapsed_ms, ..Default::default() }, &bindings).unwrap();
			let ownership = AvatarSemanticOwnership::default();
			let strength = if elapsed_ms == 0 { 0.55 } else { 0.0 };
			for blink in [0.0, 0.5, 1.0, 0.5, 0.0] {
				let mut frame = compose_avatar_frame(1, "test", "JawOpen", 0.7, blink, &[(&intent, &ownership)]);
				attenuate_blink_conflicts(&mut frame, blink, &["eye_smile1".into()]);
				let face = frame.face.unwrap();
				for (name, expected) in [("eye_smile1", strength * (1.0 - blink)), ("cheek", strength),
					("JawOpen", 0.7), ("EyeBlinkLeft", blink), ("EyeBlinkRight", blink)] {
					assert!((face.expressions.iter().find(|e| e.name == name).unwrap().value - expected).abs() < 1e-6);
				}
			}
		}
	}

	#[test]
	fn renderer_name_aliases_cannot_override_mouth_or_blink_and_share_attenuation() {
		let mut semantic = UNMotionFrame::new(1);
		semantic.face = Some(FaceMotion { tracking_state: TrackingState::Valid, confidence: 1.0, head: None,
			expressions: [("CustomMouth", 0.0), ("eye_blink_left", 0.0), ("EYE-BLINK-RIGHT", 0.0), ("EyeSmile", 0.8)]
				.into_iter().map(|(name, value)| ExpressionSample {name: name.into(), value, confidence: 1.0, source_index: None, state: SampleState::Valid}).collect() });
		let ownership = AvatarSemanticOwnership::default();
		let mut frame = compose_avatar_frame(1, "test", "custom_mouth", 0.7, 0.5, &[(&semantic, &ownership)]);
		attenuate_blink_conflicts(&mut frame, 0.5, &["eye_smile".into()]);
		let face = frame.face.unwrap();
		for (name, expected) in [("custom_mouth", 0.7), ("EyeBlinkLeft", 0.5), ("EyeBlinkRight", 0.5), ("EyeSmile", 0.4)] {
			let matching: Vec<_> = face.expressions.iter().filter(|e| vac_motion::avatar_intent::same_preset_name(&e.name, name)).collect();
			assert_eq!(matching.len(), 1, "one sample per renderer target: {name}");
			assert_eq!(matching[0].value, expected, "{name}");
		}
	}

	#[test]
	fn bound_expressions_preserve_compositor_mouth_and_blink_during_release() {
		use vac_motion::avatar_intent::{avatar_intent_to_unmf_with_bindings, AvatarExpressionBindings, AvatarIntentMapperOptions, AvatarIntentRequest};
		let bindings: AvatarExpressionBindings = serde_json::from_value(serde_json::json!({"smile":{"modelHappy":1.0}})).unwrap();
		let request: AvatarIntentRequest = serde_json::from_value(serde_json::json!({"intents":[
			{"kind":"expression","name":"smile","intensity":0.55,"duration_ms":200,"curve":"hold"}
		]})).unwrap();
		for elapsed_ms in [0, 200] {
			let intent = avatar_intent_to_unmf_with_bindings(&request,
				&AvatarIntentMapperOptions { elapsed_ms, ..Default::default() }, &bindings).unwrap();
			let ownership = AvatarSemanticOwnership::default();
			let frame = compose_avatar_frame(1, "test", "JawOpen", 0.7, 1.0, &[(&intent, &ownership)]);
			let face = frame.face.unwrap();
			for (name, expected) in [("JawOpen", 0.7), ("EyeBlinkLeft", 1.0), ("EyeBlinkRight", 1.0),
				("modelHappy", if elapsed_ms == 0 { 0.55 } else { 0.0 })] {
				assert_eq!(face.expressions.iter().find(|e| e.name == name).unwrap().value, expected);
			}
		}
	}

	#[test]
	fn timed_expression_release_preserves_pose_mouth_blink_and_head_motion() {
		use vac_motion::avatar_intent::{avatar_intent_to_unmf, AvatarIntentMapperOptions, AvatarIntentRequest};
		let request: AvatarIntentRequest = serde_json::from_value(serde_json::json!({"intents":[
			{"kind":"expression","name":"happy","intensity":0.6,"duration_ms":200,"curve":"hold"},
			{"kind":"head_nod","intensity":0.5,"duration_ms":1000,"curve":"linear"}
		]}))
		.unwrap();
		let arm_ids = vec![
			HumanoidBone::LeftUpperArm,
			HumanoidBone::LeftLowerArm,
			HumanoidBone::LeftHand,
			HumanoidBone::RightUpperArm,
			HumanoidBone::RightLowerArm,
			HumanoidBone::RightHand,
		];
		let mut pose = UNMotionFrame::new(1);
		pose.body = Some(BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: None,
				bones: arm_ids.iter().map(|id| arm_bone(*id, -1.0)).collect(),
			}),
		});
		let ownership = AvatarSemanticOwnership {
			owned_bones: Some(arm_ids.clone()),
			owns_root: false,
 owned_fingers: Vec::new(),
		};
		let intent_ownership = AvatarSemanticOwnership::default();
		for (elapsed_ms, mouth, blink) in [(0, 0.2, 0.0), (500, 0.7, 1.0), (1000, 0.0, 0.0)] {
			let intent = avatar_intent_to_unmf(
				&request,
				&AvatarIntentMapperOptions {
					elapsed_ms,
					..Default::default()
				},
			)
			.unwrap();
			let frame = compose_avatar_frame(
				1,
				"test",
				"JawOpen",
				mouth,
				blink,
				&[(&pose, &ownership), (&intent, &intent_ownership)],
			);
			let expressions = &frame.face.unwrap().expressions;
			for (name, expected) in [
				("JawOpen", mouth),
				("EyeBlinkLeft", blink),
				("EyeBlinkRight", blink),
				("Joy", if elapsed_ms == 0 { 0.6 } else { 0.0 }),
			] {
				assert_eq!(expressions.iter().find(|e| e.name == name).unwrap().value, expected);
			}
			let bones = frame.body.unwrap().humanoid.unwrap().bones;
			for id in &arm_ids {
				assert_eq!(
					bones.iter().find(|b| b.bone == *id).unwrap().transform.rotation,
					Some(arm_bone(*id, -1.0).transform.rotation.unwrap())
				);
			}
			let head = bones
				.iter()
				.find(|b| b.bone == HumanoidBone::Head)
				.unwrap()
				.transform
				.rotation
				.unwrap();
			if elapsed_ms == 500 {
				assert!(head.x.abs() > 0.01);
			} else {
				assert_eq!(head.w, 1.0);
			}
		}
	}

	#[test]
	fn motion_targets_are_isolated_by_base_key_and_link_id() {
		let config = AvatarMotionConfig {
			base_key_expr: "un-motion/frame/ai-a/".into(),
			expression: "JawOpen".into(),
			target_link_id: "ai-a".into(),
			gain: 1.0,
		};
		let mut other_link = config.clone();
		other_link.target_link_id = "ai-b".into();
		let mut other_key = config.clone();
		other_key.base_key_expr = "un-motion/frame/ai-b".into();

		assert_ne!(motion_target_key(&config), motion_target_key(&other_link));
		assert_eq!(motion_target_key(&config), motion_target_key(&other_key));
		assert_ne!(blink_seed(&config), blink_seed(&other_link));
		assert_ne!(blink_seed(&config), blink_seed(&other_key));
		assert_eq!(motion_target_key(&config), "ai-a");
	}
}
