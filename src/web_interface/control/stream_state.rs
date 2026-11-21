use crate::SharedState;
use actix_web::{get, web, HttpResponse, Responder};

pub fn configure(cfg: &mut web::ServiceConfig) {
	cfg.service(twitch_stream_state);
}

/// Twitchが宣言している配信状態。画像観測による実内容はこのAPIへ混在させない。
#[get("/stream_state/twitch")]
async fn twitch_stream_state(state: web::Data<SharedState>) -> impl Responder {
	let registry = state.read().await.twitch_stream_state.clone();
	HttpResponse::Ok().json(registry.snapshot(crate::stream_state::DEFAULT_FRESHNESS_SECS).await)
}
