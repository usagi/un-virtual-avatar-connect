//! OBS Browser Source templates served by the built-in web server.
//!
//! First slice: self-contained HTML pages that poll the existing `/output` endpoint.

use crate::Result;
use actix_web::{get, web, HttpResponse, Responder};
use serde::Deserialize;

const DEFAULT_CHANNEL: &str = "effect";
const DEFAULT_LIMIT: usize = 20;
const MIN_POLL_MS: u64 = 100;
const MAX_POLL_MS: u64 = 60_000;
const MAX_CHANNELS: usize = 8;
const MIN_TTL_MS: u64 = 1_000;
const MAX_TTL_MS: u64 = 300_000;
const MIN_FONT_SCALE: u16 = 50;
const MAX_FONT_SCALE: u16 = 200;
const MIN_WIDTH_PCT: u8 = 20;
const MAX_WIDTH_PCT: u8 = 100;
const MAX_LABEL_CHARS: usize = 40;

#[derive(Debug, Deserialize)]
pub struct ObsTemplateQuery {
	pub channel: Option<String>,
	pub channels: Option<String>,
	pub poll_ms: Option<u64>,
	pub limit: Option<usize>,
	pub theme: Option<String>,
	pub ttl_ms: Option<u64>,
	pub align: Option<String>,
	pub label: Option<String>,
	pub font_scale: Option<u16>,
	pub width_pct: Option<u8>,
}

#[get("/obs-template/{kind}")]
pub async fn get(kind: web::Path<String>, query: web::Query<ObsTemplateQuery>) -> Result<impl Responder> {
	let kind = TemplateKind::parse(&kind);
	let config = TemplateConfig::from_query(kind, &query);
	Ok(HttpResponse::Ok()
		.content_type("text/html; charset=utf-8")
		.body(render_template_page(&config)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TemplateKind {
	Subtitle,
	Conversation,
	Chat,
	Raid,
	Effect,
}

impl TemplateKind {
	fn parse(value: &str) -> Self {
		match value.trim().to_ascii_lowercase().as_str() {
			"conversation" | "dialog" | "dialogue" => Self::Conversation,
			"chat" | "twitch-chat" | "twitch_chat" => Self::Chat,
			"raid" | "raid-alert" | "raid_alert" => Self::Raid,
			"effect" | "effects" | "overlay" => Self::Effect,
			_ => Self::Subtitle,
		}
	}

	fn as_str(self) -> &'static str {
		match self {
			Self::Subtitle => "subtitle",
			Self::Conversation => "conversation",
			Self::Chat => "chat",
			Self::Raid => "raid",
			Self::Effect => "effect",
		}
	}

	fn title(self) -> &'static str {
		match self {
			Self::Subtitle => "UNVAC Subtitle",
			Self::Conversation => "UNVAC Conversation",
			Self::Chat => "UNVAC Chat",
			Self::Raid => "UNVAC Raid Alert",
			Self::Effect => "UNVAC Effects",
		}
	}
}

#[derive(Debug, Clone)]
struct TemplateConfig {
	kind: TemplateKind,
	channels: Vec<String>,
	poll_ms: u64,
	limit: usize,
	theme: String,
	ttl_ms: u64,
	align: &'static str,
	label: String,
	font_scale: u16,
	width_pct: u8,
}

impl TemplateConfig {
	fn from_query(kind: TemplateKind, query: &ObsTemplateQuery) -> Self {
		let channels = parse_channels(query.channels.as_deref().or(query.channel.as_deref()));
		Self {
			kind,
			channels,
			poll_ms: query.poll_ms.unwrap_or(500).clamp(MIN_POLL_MS, MAX_POLL_MS),
			limit: query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 100),
			theme: query
				.theme
				.as_deref()
				.map(str::trim)
				.filter(|value| !value.is_empty())
				.unwrap_or("default")
				.to_string(),
			ttl_ms: query.ttl_ms.unwrap_or_else(|| default_ttl_ms(kind)).clamp(MIN_TTL_MS, MAX_TTL_MS),
			align: normalize_align(query.align.as_deref()),
			label: query
				.label
				.as_deref()
				.map(str::trim)
				.unwrap_or_default()
				.chars()
				.take(MAX_LABEL_CHARS)
				.collect(),
			font_scale: query.font_scale.unwrap_or(100).clamp(MIN_FONT_SCALE, MAX_FONT_SCALE),
			width_pct: query.width_pct.unwrap_or(100).clamp(MIN_WIDTH_PCT, MAX_WIDTH_PCT),
		}
	}
}

fn normalize_align(value: Option<&str>) -> &'static str {
	match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
		Some("left") => "left",
		Some("right") => "right",
		_ => "center",
	}
}

fn parse_channels(raw: Option<&str>) -> Vec<String> {
	let mut channels = Vec::new();
	for value in raw
		.unwrap_or(DEFAULT_CHANNEL)
		.split(',')
		.map(str::trim)
		.filter(|value| !value.is_empty())
	{
		if !channels.iter().any(|channel| channel == value) {
			channels.push(value.to_string());
		}
		if channels.len() == MAX_CHANNELS {
			break;
		}
	}
	if channels.is_empty() {
		channels.push(DEFAULT_CHANNEL.to_string());
	}
	channels
}

fn default_ttl_ms(kind: TemplateKind) -> u64 {
	match kind {
		TemplateKind::Subtitle => 30_000,
		TemplateKind::Conversation | TemplateKind::Chat => 60_000,
		TemplateKind::Raid | TemplateKind::Effect => 15_000,
	}
}

fn render_template_page(config: &TemplateConfig) -> String {
	let channels = serde_json::to_string(&config.channels).unwrap_or_else(|_| "[]".into());
	let kind = json_string(config.kind.as_str());
	let title = html_escape(config.kind.title());
	let theme = attr_escape(&config.theme);
	let label = json_string(&config.label);
	let align = config.align;
	let justify = match config.align {
		"left" => "start",
		"right" => "end",
		_ => "center",
	};
	format!(
		r#"<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
:root {{
  color-scheme: dark;
  --fg: #f8fafc;
  --muted: rgba(248,250,252,.72);
  --panel: rgba(4,12,20,.68);
  --accent: #22d3ee;
  --shadow: 0 18px 70px rgba(0,0,0,.42);
  --font-scale: {font_scale};
  --content-width: {width_pct}%;
  font-family: "Segoe UI", "Noto Sans JP", sans-serif;
}}
* {{ box-sizing: border-box; }}
html, body {{ width: 100%; height: 100%; margin: 0; overflow: hidden; background: transparent; }}
body {{ display: grid; place-items: end center; padding: clamp(18px, 3vw, 48px); color: var(--fg); }}
#root {{ width: min(1480px, 100%); }}
.subtitle {{ display: grid; justify-items: {justify}; gap: 10px; }}
.line {{
  max-width: min(var(--content-width), 100%);
  padding: .52em .82em;
  border-radius: 8px;
  background: var(--panel);
  box-shadow: var(--shadow);
  font-size: clamp(calc(30px * var(--font-scale)), calc(4.5vh * var(--font-scale)), calc(76px * var(--font-scale)));
  line-height: 1.18;
  text-align: {align};
  overflow-wrap: anywhere;
  text-wrap: balance;
}}
.meta {{ color: var(--muted); font-size: 18px; letter-spacing: 0; }}
.conversation {{ display: grid; gap: 12px; align-content: end; }}
.bubble {{
  width: fit-content;
  max-width: min(960px, 86%);
  padding: 18px 22px;
  border-left: 5px solid var(--accent);
  border-radius: 8px;
  background: var(--panel);
  box-shadow: var(--shadow);
  font-size: clamp(24px, 3.2vh, 48px);
  line-height: 1.24;
  overflow-wrap: anywhere;
}}
.bubble:nth-last-child(odd) {{ justify-self: end; border-left: 0; border-right: 5px solid var(--accent); }}
.chat {{ display: grid; gap: 8px; align-content: end; justify-items: start; }}
.chat-row {{
  max-width: min(760px, 82%);
  padding: 10px 14px;
  border-radius: 8px;
  background: rgba(4,12,20,.58);
  font-size: clamp(18px, 2.2vh, 30px);
  line-height: 1.25;
  overflow-wrap: anywhere;
}}
.chat-actor {{ margin-right: .55em; color: var(--accent); font-weight: 700; }}
.raid {{
  display: grid;
  justify-items: center;
  gap: 12px;
  padding: 28px 36px;
  border-radius: 8px;
  background: var(--panel);
  box-shadow: var(--shadow);
  text-align: center;
}}
.raid-title {{ font-size: clamp(36px, 6vh, 88px); font-weight: 800; }}
.raid-body {{ font-size: clamp(22px, 3vh, 42px); color: var(--muted); overflow-wrap: anywhere; }}
.effect-layer {{
  position: fixed;
  inset: 0;
  width: 100%;
  height: 100%;
  display: grid;
  place-items: center;
  pointer-events: none;
}}
#root.effect-layer {{ width: 100%; max-width: none; }}
.effect-card {{
  width: min(1120px, 82vw);
  padding: clamp(24px, 4vw, 64px);
  border: 2px solid rgba(34,211,238,.78);
  border-radius: 8px;
  background: rgba(4,12,20,.84);
  box-shadow: var(--shadow);
  text-align: center;
  animation: effect-enter 420ms cubic-bezier(.2,.8,.2,1) both;
}}
.effect-title {{ color: var(--accent); font-size: clamp(24px, 3.2vh, 44px); font-weight: 700; }}
.effect-content {{ margin-top: .25em; font-size: clamp(34px, 5.5vh, 78px); font-weight: 800; line-height: 1.14; overflow-wrap: anywhere; }}
.effect-chat {{ place-items: end center; padding-bottom: 7vh; }}
.effect-chat .effect-card {{ text-align: left; }}
.effect-chat .effect-content {{ font-size: clamp(28px, 4vh, 58px); }}
.effect-clip .effect-card {{ width: min(1280px, 90vw); padding: 16px; }}
.effect-clip iframe {{ display: block; width: 100%; aspect-ratio: 16/9; min-height: 300px; border: 0; }}
.effect-clip .effect-content {{ font-size: clamp(20px, 2.5vh, 36px); margin: 12px 0; }}
.effect-flash {{
  background: rgba(125,211,252,.52);
  animation: effect-flash 820ms ease-out both;
}}
@keyframes effect-enter {{
  from {{ opacity: 0; transform: translateY(28px) scale(.97); }}
  to {{ opacity: 1; transform: translateY(0) scale(1); }}
}}
@keyframes effect-flash {{
  0% {{ opacity: 0; }}
  18% {{ opacity: 1; }}
  100% {{ opacity: 0; }}
}}
@media (prefers-reduced-motion: reduce) {{
  .effect-card, .effect-flash {{ animation-duration: 1ms; }}
}}
.hidden {{ opacity: 0; transform: translateY(12px); }}
[data-theme="minimal"] .line, [data-theme="minimal"] .bubble, [data-theme="minimal"] .chat-row, [data-theme="minimal"] .raid, [data-theme="minimal"] .effect-card {{ background: rgba(0,0,0,.36); box-shadow: none; }}
</style>
</head>
<body data-theme="{theme}">
<main id="root" aria-live="polite"></main>
<script>
const CONFIG = {{ kind: {kind}, channels: {channels}, pollMs: {poll_ms}, limit: {limit}, ttlMs: {ttl_ms}, label: {label} }};
let items = [];
let lastEffectKey = "";
const noticePageOpenedAt = Date.now();
const noticeSeen = new Set();
const noticeQueue = [];
let activeNotice = null;
const root = document.getElementById("root");

function escapeHtml(value) {{
  return String(value ?? "").replace(/[&<>"']/g, ch => ({{ "&": "&amp;", "<": "&lt;", ">": "&gt;", "\"": "&quot;", "'": "&#39;" }}[ch]));
}}

function visibleItems() {{
  const now = Date.now();
  const sorted = items
    .filter(item => item.is_preflight_probe !== true)
    .filter(item => String(item.content ?? "").trim().length > 0)
    .filter(item => {{
      const at = Date.parse(item.datetime ?? "");
      return !Number.isFinite(at) || now - at <= CONFIG.ttlMs;
    }})
    .sort((a, b) => String(a.datetime ?? "").localeCompare(String(b.datetime ?? "")) || Number(a.id ?? 0) - Number(b.id ?? 0));
  const deduped = [];
  for (const item of sorted) {{
    const previous = deduped[deduped.length - 1];
    const previousAt = Date.parse(previous?.datetime ?? "");
    const itemAt = Date.parse(item.datetime ?? "");
    const sameRecentText = previous && previous.channel === item.channel && previous.content === item.content &&
      Number.isFinite(previousAt) && Number.isFinite(itemAt) && itemAt - previousAt <= 3000;
    if (sameRecentText) deduped[deduped.length - 1] = item;
    else deduped.push(item);
  }}
  return deduped.slice(-CONFIG.limit);
}}

function displayActor(item) {{
  const actor = String(item?.source_actor ?? "").trim();
  if (!actor || actor === "flowgraph" || actor.startsWith("flowgraph:") || actor.startsWith("control.")) return "";
  return actor;
}}

function itemFlags(item) {{
  return Array.isArray(item?.flags) ? item.flags.map(flag => String(flag)) : [];
}}

function effectDurationMs(item) {{
  if (itemFlags(item).includes("obs.clip")) return Math.min(Math.max(Number(item?.clip_duration_ms) || 30000, 1000), 180000) + 5000;
  const requested = Number(item?.quick_effect_duration_ms);
  if (!Number.isFinite(requested) || requested <= 0) return CONFIG.ttlMs;
  return Math.min(Math.max(requested, 250), CONFIG.ttlMs);
}}

function visibleEffects() {{
  const now = Date.now();
  return items
    .filter(item => item.is_preflight_probe !== true)
    .filter(item => itemFlags(item).some(flag => flag.startsWith("obs.")))
    .filter(item => {{
      const at = Date.parse(item.datetime ?? "");
      return !Number.isFinite(at) || now - at <= effectDurationMs(item);
    }})
    .sort((a, b) => String(a.datetime ?? "").localeCompare(String(b.datetime ?? "")) || Number(a.id ?? 0) - Number(b.id ?? 0));
}}

function renderEffect() {{
  // Notice events are queued by identity, not collapsed by equal text.
  for (const item of items.filter(i => itemFlags(i).some(f => f === "obs.event.notice" || f === "obs.clip")).sort((a,b) => Number(a.id)-Number(b.id))) {{
    const at = Date.parse(item.datetime ?? "");
    const key = `${{item.id}}:${{item.datetime}}`;
    if (!Number.isFinite(at) || at < noticePageOpenedAt || item.is_preflight_probe === true || noticeSeen.has(key)) continue;
    noticeSeen.add(key);
    if (noticeSeen.size > 4096) noticeSeen.delete(noticeSeen.values().next().value);
    if (noticeQueue.length < 100) noticeQueue.push(item);
    else console.warn("UNVAC event notice queue full; notification retained in event/chat logs");
  }}
  if (activeNotice && Date.now() >= activeNotice.until) {{ activeNotice = null; lastEffectKey = ""; root.innerHTML = ""; }}
  if (!activeNotice && noticeQueue.length) {{
    const item = noticeQueue.shift();
    activeNotice = {{ item, until: Date.now() + effectDurationMs(item) }};
  }}
  const latest = activeNotice?.item ?? visibleEffects().filter(i => !itemFlags(i).some(f => f === "obs.event.notice" || f === "obs.clip")).at(-1);
  if (!latest) {{
    if (lastEffectKey) {{ root.className = "effect-layer"; root.innerHTML = ""; lastEffectKey = ""; }}
    return;
  }}
  const flags = itemFlags(latest);
  const kind = flags.includes("obs.effect.flash") ? "flash" : flags.includes("obs.raid") ? "raid" : flags.includes("obs.chat.highlight") ? "chat" : "generic";
  const key = `${{latest.id ?? ""}}:${{latest.datetime ?? ""}}:${{kind}}`;
  if (key === lastEffectKey) return;
  lastEffectKey = key;
  if (flags.includes("obs.clip")) {{
    const slug = String(latest.clip_id ?? "");
    if (!/^[A-Za-z0-9_-]{{1,200}}$/.test(slug)) {{ root.innerHTML = ""; return; }}
    const url = new URL("https://clips.twitch.tv/embed");
    url.searchParams.set("clip", slug);
    url.searchParams.set("parent", location.hostname);
    url.searchParams.set("autoplay", "true");
    url.searchParams.set("muted", "false");
    root.className = "effect-layer effect-clip";
    root.innerHTML = `<section class="effect-card"><div class="effect-title">CLIP</div><div class="effect-content">${{escapeHtml(latest.content)}}</div><iframe title="Twitch clip" src="${{escapeHtml(url.href)}}" allow="autoplay; fullscreen" allowfullscreen></iframe></section>`;
    return;
  }}
  if (kind === "flash") {{
    root.className = "effect-layer effect-flash";
    root.innerHTML = "";
    return;
  }}
  const label = String(latest?.quick_effect_label ?? (kind === "raid" ? "RAID" : kind === "chat" ? "CHAT" : "EVENT"));
  root.className = `effect-layer effect-${{kind}}`;
  root.innerHTML = `<section class="effect-card"><div class="effect-title">${{escapeHtml(label)}}</div><div class="effect-content">${{escapeHtml(latest.content)}}</div></section>`;
}}

function render() {{
	if (CONFIG.kind === "effect") {{
	  renderEffect();
	  return;
	}}
  const visible = visibleItems();
  if (CONFIG.kind === "conversation") {{
    root.className = "conversation";
    root.innerHTML = visible.slice(-4).map(item => {{
      const actor = displayActor(item);
      const actorHtml = actor ? `<div class="meta">${{escapeHtml(actor)}}</div>` : "";
      return `<section class="bubble">${{actorHtml}}${{escapeHtml(item.content)}}</section>`;
    }}).join("");
    return;
  }}
  if (CONFIG.kind === "chat") {{
    root.className = "chat";
    root.innerHTML = visible.map(item => {{
      const actor = displayActor(item);
      const actorHtml = actor ? `<span class="chat-actor">${{escapeHtml(actor)}}</span>` : "";
      return `<div class="chat-row">${{actorHtml}}<span>${{escapeHtml(item.content)}}</span></div>`;
    }}).join("");
    return;
  }}
  if (CONFIG.kind === "raid") {{
    const latest = visible[visible.length - 1];
    root.className = "raid";
    root.innerHTML = latest ? `<div class="raid-title">RAID</div><div class="raid-body">${{escapeHtml(latest.content)}}</div>` : "";
    return;
  }}
  const latest = visible[visible.length - 1];
  root.className = "subtitle";
  const hasContent = latest && String(latest.content ?? "").trim().length > 0;
  const actor = CONFIG.label || displayActor(latest);
  const actorHtml = actor ? `<div class="meta">${{escapeHtml(actor)}}</div>` : "";
  root.innerHTML = hasContent ? `<div class="line">${{escapeHtml(latest.content)}}</div>${{actorHtml}}` : "";
}}

async function poll() {{
  try {{
    const res = await fetch("/output", {{
      method: "POST",
      headers: {{ "content-type": "application/json" }},
      body: JSON.stringify({{ channels: CONFIG.channels.map(name => ({{ name, count: CONFIG.limit }})) }})
    }});
    if (!res.ok) return;
    const json = await res.json();
    const channelData = json.channel_data || {{}};
    items = CONFIG.channels.flatMap(name => channelData[name] || []);
    render();
  }} catch (_) {{}}
}}

render();
poll();
setInterval(poll, CONFIG.pollMs);
setInterval(() => {{ if (CONFIG.kind === "effect") renderEffect(); }}, 100);
</script>
</body>
</html>
"#,
		title = title,
		theme = theme,
		kind = kind,
		channels = channels,
		poll_ms = config.poll_ms,
		limit = config.limit,
		ttl_ms = config.ttl_ms,
		label = label,
		align = align,
		justify = justify,
		font_scale = f32::from(config.font_scale) / 100.0,
		width_pct = config.width_pct,
	)
}

fn json_string(value: &str) -> String {
	serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}

fn html_escape(value: &str) -> String {
	value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn attr_escape(value: &str) -> String {
	html_escape(value).replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn template_kind_aliases_are_stable() {
		assert_eq!(TemplateKind::parse("dialog"), TemplateKind::Conversation);
		assert_eq!(TemplateKind::parse("twitch_chat"), TemplateKind::Chat);
		assert_eq!(TemplateKind::parse("raid-alert"), TemplateKind::Raid);
		assert_eq!(TemplateKind::parse("effects"), TemplateKind::Effect);
		assert_eq!(TemplateKind::parse("unknown"), TemplateKind::Subtitle);
	}

	#[test]
	fn template_config_clamps_poll_and_limit() {
		let query = ObsTemplateQuery {
			channel: Some(" ai ".into()),
			channels: None,
			poll_ms: Some(1),
			limit: Some(1000),
			theme: Some("minimal".into()),
			ttl_ms: Some(999_999),
			align: Some("RIGHT".into()),
			label: Some("  Kal'tsit Pseudo  ".into()),
			font_scale: Some(999),
			width_pct: Some(1),
		};
		let config = TemplateConfig::from_query(TemplateKind::Chat, &query);
		assert_eq!(config.channels, vec!["ai"]);
		assert_eq!(config.poll_ms, MIN_POLL_MS);
		assert_eq!(config.limit, 100);
		assert_eq!(config.theme, "minimal");
		assert_eq!(config.ttl_ms, MAX_TTL_MS);
		assert_eq!(config.align, "right");
		assert_eq!(config.label, "Kal'tsit Pseudo");
		assert_eq!(config.font_scale, MAX_FONT_SCALE);
		assert_eq!(config.width_pct, MIN_WIDTH_PCT);
	}

	#[test]
	fn template_config_accepts_multiple_unique_channels() {
		let query = ObsTemplateQuery {
			channel: Some("ignored".into()),
			channels: Some(" user, ai-synth, user ".into()),
			poll_ms: None,
			limit: None,
			theme: None,
			ttl_ms: None,
			align: Some("invalid".into()),
			label: None,
			font_scale: None,
			width_pct: None,
		};
		let config = TemplateConfig::from_query(TemplateKind::Conversation, &query);
		assert_eq!(config.channels, vec!["user", "ai-synth"]);
		assert_eq!(config.ttl_ms, 60_000);
		assert_eq!(config.align, "center");
	}

	#[test]
	fn rendered_template_contains_output_poll_contract() {
		let config = TemplateConfig {
			kind: TemplateKind::Subtitle,
			channels: vec!["user".into(), "ai-synth".into()],
			poll_ms: 500,
			limit: 20,
			theme: "default".into(),
			ttl_ms: 30_000,
			align: "left",
			label: "Dr.USAGI".into(),
			font_scale: 110,
			width_pct: 70,
		};
		let html = render_template_page(&config);
		assert!(html.contains("fetch(\"/output\""));
		assert!(html.contains("count: CONFIG.limit"));
		assert!(html.contains("CONFIG.channels.flatMap"));
		assert!(html.contains("item.is_preflight_probe !== true"));
		assert!(html.contains("CONFIG.ttlMs"));
		assert!(html.contains("const hasContent = latest &&"));
		assert!(html.contains("root.innerHTML = hasContent ?"));
		assert!(!html.contains("retrieved_id"));
		assert!(!html.contains("lastId"));
		assert!(html.contains("[\"user\",\"ai-synth\"]"));
		assert!(html.contains("aria-live=\"polite\""));
		assert!(html.contains("justify-items: start"));
		assert!(html.contains("text-align: left"));
		assert!(html.contains("--font-scale: 1.1"));
		assert!(html.contains("--content-width: 70%"));
		assert!(html.contains(r#"label: "Dr.USAGI""#));
	}

	#[test]
	fn effect_template_filters_flags_and_uses_event_duration() {
		let config = TemplateConfig {
			kind: TemplateKind::Effect,
			channels: vec!["effect".into(), "chat".into()],
			poll_ms: 100,
			limit: 20,
			theme: "default".into(),
			ttl_ms: 15_000,
			align: "center",
			label: String::new(),
			font_scale: 100,
			width_pct: 100,
		};
		let html = render_template_page(&config);
		assert!(html.contains("flag.startsWith(\"obs.\")"));
		assert!(html.contains("quick_effect_duration_ms"));
		assert!(html.contains("obs.effect.flash"));
		assert!(html.contains("obs.chat.highlight"));
		assert!(html.contains("lastEffectKey"));
	}
}
