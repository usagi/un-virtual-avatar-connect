use actix_web::web::{self, Data, Json};
use actix_web::{get, put, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::control_events::ControlEvent;
use crate::SharedState;

#[derive(Debug, Clone, Serialize)]
pub struct VoiceInputState {
	pub muted: bool,
	pub worker_count: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub persistence_error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PutVoiceInputState {
	pub muted: bool,
}

async fn current_state(state: &SharedState) -> VoiceInputState {
	let (voice_input, bridge_handles) = {
		let state = state.read().await;
		(state.voice_input.clone(), state.bridge_handles.clone())
	};
	let worker_count = bridge_handles.lock().await.voice_worker_count();
	VoiceInputState {
		muted: voice_input.is_muted(),
		worker_count,
		persistence_error: None,
	}
}

#[get("/voice-input")]
pub async fn get_voice_input(state: Data<SharedState>) -> impl Responder {
	HttpResponse::Ok().json(current_state(state.get_ref()).await)
}

#[put("/voice-input")]
pub async fn put_voice_input(state: Data<SharedState>, body: Json<PutVoiceInputState>) -> impl Responder {
	let (voice_input, bridge_handles, control_event_tx, conf_source_path) = {
		let state = state.read().await;
		(
			state.voice_input.clone(),
			state.bridge_handles.clone(),
			state.control_event_tx.clone(),
			state.conf_source_path.clone(),
		)
	};
	let (changed, persistence_error) = apply_voice_input_setting(&voice_input, body.muted, |muted| {
		crate::profile_store::set_voice_input_muted_for_conf(conf_source_path.as_deref(), muted)
	});
	let handles = bridge_handles.lock().await;
	if body.muted {
		handles.release_voice_speech_floors();
	}
	let response = VoiceInputState {
		muted: voice_input.is_muted(),
		worker_count: handles.voice_worker_count(),
		persistence_error,
	};
	drop(handles);

	if changed {
		let _ = control_event_tx.send(ControlEvent::VoiceInputState {
			muted: response.muted,
			worker_count: response.worker_count,
		});
	}
	HttpResponse::Ok().json(response)
}

pub fn configure(cfg: &mut web::ServiceConfig) {
	cfg.service(get_voice_input).service(put_voice_input);
}

// Apply the live gate immediately, but report persistence separately. Retrying
// the same value must retry the write even when the live gate already matches.
fn apply_voice_input_setting(
    control: &crate::state::VoiceInputControl,
    muted: bool,
    persist: impl FnOnce(bool) -> anyhow::Result<bool>,
) -> (bool, Option<String>) {
    let changed = control.set_muted(muted);
    let error = persist(muted).err().map(|error| format!("{error:#}"));
    (changed, error)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_mute_persistence_keeps_live_gate_and_same_value_retry_recovers() {
        let control = crate::state::VoiceInputControl::new(false);
        let (changed, error) = apply_voice_input_setting(&control, true, |_| anyhow::bail!("disk unavailable"));
        assert!(changed && control.is_muted());
        assert_eq!(error.as_deref(), Some("disk unavailable"));
        let mut stored = false;
        let (changed, error) = apply_voice_input_setting(&control, true, |value| { stored = value; Ok(true) });
        assert!(!changed && control.is_muted() && stored);
        assert!(error.is_none());
    }
}
