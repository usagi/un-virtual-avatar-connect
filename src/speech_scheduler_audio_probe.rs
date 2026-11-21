//! Opt-in device probe. Uses saved synthesized WAVs, with the player muted.
use super::*;
use async_trait::async_trait;

struct SavedWav(Vec<u8>);

#[async_trait]
impl TtsDriver for SavedWav {
	fn name(&self) -> &'static str {
		"saved-wav-probe"
	}
	async fn synthesize(&self, _: &TtsRequest) -> Result<TtsSynthesis, TtsError> {
		Ok(TtsSynthesis::Wav(self.0.clone()))
	}
}

#[tokio::test]
#[ignore = "requires VAC_SEGMENT_AUDIO_DIR, saved A/B WAVs, and an audio device; output is muted"]
async fn saved_segments_yield_at_boundary_on_same_human_onset() {
	let root = std::path::PathBuf::from(std::env::var_os("VAC_SEGMENT_AUDIO_DIR").expect("fixture directory"));
	let release_offset: f64 = std::env::var("VAC_PROBE_RELEASE_SECONDS").unwrap_or_else(|_| "6.0".into()).parse().unwrap();
	assert!(release_offset > 0.5 && release_offset <= 6.0);
	let mut reports = Vec::new();
	for variant in ["A", "B"] {
		let sink = crate::AudioSink::open_default().expect("audio device required for this opt-in probe");
		sink.player.set_volume(0.0);
		let sink = Arc::new(tokio::sync::Mutex::new(sink));
		let scheduler = SpeechScheduler::spawn(Some(sink.clone()), AvatarMotionController::spawn());
		let floor = Arc::new(crate::state::SpeechFloorManager::new());
		let (tx, mut rx) = tokio::sync::broadcast::channel(64);
		let mut tasks = Vec::new();
		let origin = Instant::now();
		let mut events = Vec::new();
		for index in 0..2 {
			let wav = std::fs::read(root.join(format!("agreement-{variant}-{index}.wav"))).unwrap();
			let observer = SpeechLifecycleObserver::new(
				tx.clone(),
				index.to_string(),
				"probe".into(),
				"saved".into(),
				"saved".into(),
				SpeechLane::Assistant,
				1,
			);
			let scheduler = scheduler.clone();
			let floor = floor.clone();
			tasks.push(tokio::spawn(async move {
				scheduler
					.speak_with_floor(
						SpeechLane::Assistant,
						"probe".into(),
						index,
						SpeechQueuePolicy::for_lane(SpeechLane::Assistant),
						Arc::new(SavedWav(wav)),
						TtsRequest {
							text: "fixture".into(),
							voice: String::new(),
							speed: 1.0,
							pitch: 0.0,
							volume: 1.0,
							endpoint: String::new(),
							save_path: String::new(),
							extra: Default::default(),
						},
						None,
						Some(observer),
						Some((floor, "human".into())),
					)
					.await
			}));
			if index == 0 {
				loop {
					let event = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.unwrap().unwrap();
					let first = matches!(
						&event,
						ControlEvent::TtsLifecycle {
							phase: TtsLifecyclePhase::PlaybackStarted,
							..
						}
					);
					events.push((origin.elapsed().as_secs_f64(), event));
					if first {
						break;
					}
				}
			}
		}
		// Both variants use the same offset from the first playback notification.
		let first_start = events.last().unwrap().0;
		tokio::time::sleep_until(tokio::time::Instant::from_std(origin + Duration::from_secs_f64(first_start + 0.5))).await;
		floor.set_held("human", true);
		let human_start = origin.elapsed().as_secs_f64();
		tokio::time::sleep_until(tokio::time::Instant::from_std(origin + Duration::from_secs_f64(first_start + release_offset))).await;
		while let Ok(event) = rx.try_recv() {
			events.push((origin.elapsed().as_secs_f64(), event));
		}
		assert!(
			!events.iter().any(
				|(_, event)| matches!(event, ControlEvent::TtsLifecycle { node_id, phase: TtsLifecyclePhase::PlaybackStarted, .. } if node_id == "1")
			),
			"tail started while human held the floor"
		);
		if release_offset > 5.0 {
			assert!(sink.lock().await.player.empty(), "first segment must finish while the floor remains held");
		}
		let release = origin.elapsed().as_secs_f64();
		floor.set_held("human", false);
		for task in tasks {
			assert!(
				tokio::time::timeout(Duration::from_secs(10), task)
					.await
					.unwrap()
					.unwrap()
					.unwrap()
					.played
			);
		}
		// speak returns after submission; lifecycle Completed is emitted after the player drains.
		loop {
			let event = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await.unwrap().unwrap();
			let done = matches!(&event, ControlEvent::TtsLifecycle { node_id, phase: TtsLifecyclePhase::Completed, .. } if node_id == "1");
			events.push((origin.elapsed().as_secs_f64(), event));
			if done {
				break;
			}
		}
		reports.push(
			serde_json::json!({"variant": variant, "human_start_seconds": human_start, "release_seconds": release, "release_offset_seconds": release_offset, "events": events}),
		);
	}
	std::fs::write(
		root.join(format!("scheduler-yield-probe-{}ms.json", (release_offset * 1000.0).round() as u64)),
		serde_json::to_vec_pretty(&reports).unwrap(),
	)
	.unwrap();
}
