use std::{
	fs,
	io::Read,
	path::{Path, PathBuf},
	thread,
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{repo_root, Result, DEFAULT_DESKTOP_BASE_URL as DEFAULT_BASE_URL};
const DEFAULT_FIXTURE: &str = "tests/fixtures/ai-turn/streaming-partner.ja.json";

pub(crate) fn run(args: Vec<String>) -> Result<()> {
	let args = CliArgs::new(&args);
	let root = repo_root()?;
	let fixture_path = resolve_path(&root, args.value_or("--fixture", DEFAULT_FIXTURE));
	let fixture: EvaluationFixture = serde_json::from_str(&fs::read_to_string(&fixture_path)?)?;
	fixture.validate()?;

	if args.flag("--dry-run") {
		println!("AI turn fixture OK: {} ({} cases)", fixture_path.display(), fixture.cases.len());
		return Ok(());
	}

	let base_url = args.value_or("--base-url", DEFAULT_BASE_URL).trim_end_matches('/');
	let token = args.value_or("--token", "");
	let timeout = Duration::from_secs(args.parse_u64("--timeout-secs", 90)?);
	let persona_id = args.value("--persona-id").unwrap_or(&fixture.persona_id);
	let output_path = args
		.value("--output")
		.map(|value| resolve_path(&root, value))
		.unwrap_or_else(|| default_output_path(&root));
	let cases = match args.value("--through") {
		Some(id) => {
			let index = fixture
				.cases
				.iter()
				.position(|case| case.id == id)
				.ok_or_else(|| format!("fixture has no case `{id}`"))?;
			&fixture.cases[..=index]
		}
		None => fixture.cases.as_slice(),
	};

	let ping = get_json(&format!("{base_url}/api/v1/control/ping"), token)?;
	let before = persona_usage(&get_json(&format!("{base_url}/api/v1/control/snapshot"), token)?, persona_id)?;
	let started_at = jiff::Timestamp::now().to_string();
	let mut results = Vec::with_capacity(cases.len());
	let mut previous_episode_id: Option<String> = None;

	for (index, case) in cases.iter().enumerate() {
		if case.delay_before_ms > 0 {
			println!("==> {}: waiting {} ms", case.id, case.delay_before_ms);
			thread::sleep(Duration::from_millis(case.delay_before_ms));
		}
		println!("==> {}/{} {}: {}", index + 1, cases.len(), case.id, case.content);
		let request_started = Instant::now();
		let ingress = post_json(
			&format!("{base_url}/api/v1/control/ingress"),
			token,
			&json!({
				"channel": case.channel.as_deref().unwrap_or(&fixture.channel),
				"content": case.content,
				"is_final": true,
				"source": {"kind": "xtask.ai_turn_eval", "actor": case.actor.as_deref().unwrap_or("local")},
				"meta": {"fixture": fixture.name, "case": case.id}
			}),
		)?;
		let observation_id = ingress
			.get("id")
			.and_then(Value::as_u64)
			.ok_or("Control ingress response has no numeric id")?;
		let event = wait_for_decision(base_url, token, persona_id, observation_id, timeout)?;
		let elapsed_ms = request_started.elapsed().as_millis() as u64;
		let failures = evaluate_case(case, &fixture.speech_policy, &event, previous_episode_id.as_deref());
		println!(
			"    action={} speech={} episode={} phase={} elapsed={} ms{}",
			event.action,
			event.speak.as_ref().map(|_| "yes").unwrap_or("no"),
			event.episode_id.as_deref().unwrap_or("-"),
			event.episode_phase.as_deref().unwrap_or("-"),
			elapsed_ms,
			if failures.is_empty() { "" } else { " FAILED" },
		);
		previous_episode_id = event.episode_id.clone();
		results.push(CaseResult {
			id: case.id.clone(),
			observation_id,
			elapsed_ms,
			event,
			failures,
		});
	}

	let after = persona_usage(&get_json(&format!("{base_url}/api/v1/control/snapshot"), token)?, persona_id)?;
	let usage_delta = after.delta_from(&before);
	let failed_cases = results.iter().filter(|result| !result.failures.is_empty()).count();
	let spoken_cases = results.iter().filter(|result| result.event.speak.is_some()).count();
	let avatar_intents_requested = results.iter().map(|result| result.event.avatar_intent_requested_count).sum();
	let avatar_intents_emitted = results.iter().map(|result| result.event.avatar_intent_emitted_count).sum();
	let avatar_intent_errors = results.iter().filter(|result| result.event.avatar_intent_error.is_some()).count();
	let average_elapsed_ms = results.iter().map(|result| result.elapsed_ms).sum::<u64>() / results.len() as u64;
	let p95_elapsed_ms = percentile_nearest_rank(results.iter().map(|result| result.elapsed_ms), 95);
	let report = EvaluationReport {
		schema: "unvac.ai_turn_eval.report.v1",
		fixture: fixture.name,
		fixture_path: fixture_path.display().to_string(),
		base_url: base_url.to_string(),
		persona_id: persona_id.to_string(),
		app_version: ping.get("version").and_then(Value::as_str).unwrap_or("unknown").to_string(),
		started_at,
		completed_at: jiff::Timestamp::now().to_string(),
		case_count: results.len(),
		failed_cases,
		spoken_cases,
		avatar_intents_requested,
		avatar_intents_emitted,
		avatar_intent_errors,
		avatar_poses_requested: results.iter().filter(|r| r.event.avatar_pose_requested).count(),
		avatar_poses_emitted: results.iter().filter(|r| r.event.avatar_pose_emitted).count(),
		avatar_pose_errors: results.iter().filter(|r| r.event.avatar_pose_error.is_some()).count(),
		average_elapsed_ms,
		p95_elapsed_ms,
		usage_before: before,
		usage_after: after,
		usage_delta,
		results,
	};
	if let Some(parent) = output_path.parent() {
		fs::create_dir_all(parent)?;
	}
	fs::write(&output_path, serde_json::to_string_pretty(&report)?)?;
	println!("Report: {}", output_path.display());
	println!(
		"Poses: requested={} emitted={} errors={}",
		report.avatar_poses_requested, report.avatar_poses_emitted, report.avatar_pose_errors
	);
	println!(
		"Summary: cases={} failed={} spoken={} avatar_requested={} avatar_emitted={} avatar_errors={} avg={} ms p95={} ms requests={} input={} cached={} cache_write={} output={}",
		report.case_count,
		report.failed_cases,
		report.spoken_cases,
		report.avatar_intents_requested,
		report.avatar_intents_emitted,
		report.avatar_intent_errors,
		report.average_elapsed_ms,
		report.p95_elapsed_ms,
		report.usage_delta.api_requests,
		report.usage_delta.input_tokens,
		report.usage_delta.cached_tokens,
		report.usage_delta.cache_write_tokens,
		report.usage_delta.output_tokens,
	);
	if failed_cases > 0 && !args.flag("--no-fail") {
		return Err(format!("{failed_cases} AI turn evaluation case(s) failed").into());
	}
	Ok(())
}

#[derive(Debug)]
struct CliArgs<'a> {
	items: &'a [String],
}

impl<'a> CliArgs<'a> {
	fn new(items: &'a [String]) -> Self {
		Self { items }
	}

	fn flag(&self, name: &str) -> bool {
		self.items.iter().any(|item| item == name)
	}

	fn value(&self, name: &str) -> Option<&str> {
		self.items
			.iter()
			.position(|item| item == name)
			.and_then(|index| self.items.get(index + 1))
			.map(String::as_str)
	}

	fn value_or<'b>(&'b self, name: &str, default: &'b str) -> &'b str {
		self.value(name).unwrap_or(default)
	}

	fn parse_u64(&self, name: &str, default: u64) -> Result<u64> {
		match self.value(name) {
			Some(value) => Ok(value.parse().map_err(|_| format!("{name} must be an unsigned integer: {value}"))?),
			None => Ok(default),
		}
	}
}

#[derive(Debug, Deserialize)]
struct EvaluationFixture {
	schema: String,
	name: String,
	persona_id: String,
	channel: String,
	#[serde(default)]
	speech_policy: SpeechPolicy,
	cases: Vec<EvaluationCase>,
}

impl EvaluationFixture {
	fn validate(&self) -> Result<()> {
		if self.schema != "unvac.ai_turn_eval.fixture.v1" {
			return Err(format!("unsupported AI turn fixture schema: {}", self.schema).into());
		}
		if self.name.trim().is_empty() || self.persona_id.trim().is_empty() || self.channel.trim().is_empty() {
			return Err("fixture name, persona_id, and channel must not be empty".into());
		}
		if self.cases.is_empty() {
			return Err("fixture must contain at least one case".into());
		}
		self.speech_policy.validate()?;
		for case in &self.cases {
			case.validate()?;
		}
		Ok(())
	}
}

#[derive(Debug, Default, Deserialize)]
struct SpeechPolicy {
	max_chars: Option<usize>,
	max_sentences: Option<usize>,
	#[serde(default)]
	forbid_question_ending: bool,
}

impl SpeechPolicy {
	fn validate(&self) -> Result<()> {
		if self.max_chars == Some(0) || self.max_sentences == Some(0) {
			return Err("speech policy limits must be greater than zero".into());
		}
		Ok(())
	}
}

#[derive(Debug, Deserialize)]
struct EvaluationCase {
	id: String,
	#[serde(default)]
	actor: Option<String>,
	#[serde(default)]
	channel: Option<String>,
	content: String,
	#[serde(default)]
	delay_before_ms: u64,
	#[serde(default)]
	allowed_actions: Vec<String>,
	#[serde(default = "default_any")]
	speech: String,
	#[serde(default = "default_any")]
	episode: String,
	#[serde(default)]
	allowed_episode_phases: Vec<String>,
	#[serde(default)]
	episode_memory_includes: Vec<String>,
	#[serde(default)]
	speech_forbids: Vec<String>,
	#[serde(default)]
	speech_includes: Vec<String>,
	#[serde(default = "default_any")]
	avatar_intent: String,
	#[serde(default = "default_any")]
	avatar_pose: String,
	#[serde(default = "default_any")]
	episode_relation: String,
}

fn default_any() -> String {
	"any".to_string()
}

impl EvaluationCase {
	fn validate(&self) -> Result<()> {
		if self.id.trim().is_empty() || self.content.trim().is_empty() {
			return Err("fixture case id and content must not be empty".into());
		}
		for action in &self.allowed_actions {
			if !matches!(action.as_str(), "observe" | "respond" | "react" | "close") {
				return Err(format!("case {} has invalid action: {action}", self.id).into());
			}
		}
		if !matches!(self.speech.as_str(), "any" | "required" | "forbidden") {
			return Err(format!("case {} has invalid speech expectation: {}", self.id, self.speech).into());
		}
		if !matches!(self.episode.as_str(), "any" | "active" | "none") {
			return Err(format!("case {} has invalid episode expectation: {}", self.id, self.episode).into());
		}
		if !matches!(self.avatar_intent.as_str(), "any" | "required" | "forbidden") {
			return Err(format!("case {} has invalid avatar_intent expectation: {}", self.id, self.avatar_intent).into());
		}
		if !matches!(self.avatar_pose.as_str(), "any" | "required" | "forbidden") {
			return Err(format!("case {} has invalid avatar_pose expectation: {}", self.id, self.avatar_pose).into());
		}
		if !matches!(self.episode_relation.as_str(), "any" | "same" | "different" | "none") {
			return Err(format!(
				"case {} has invalid episode_relation expectation: {}",
				self.id, self.episode_relation
			)
			.into());
		}
		Ok(())
	}
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AiTurnDecisionEvent {
	persona_id: String,
	observation_channel: String,
	observation_datum_id: u64,
	action: String,
	speak: Option<String>,
	#[serde(default)]
	episode_id: Option<String>,
	#[serde(default)]
	episode_phase: Option<String>,
	#[serde(default)]
	episode_memory: Option<String>,
	#[serde(default)]
	episode_turn_count: Option<u32>,
	#[serde(default)]
	episode_phase_remaining_secs: Option<u64>,
	#[serde(default)]
	avatar_intent_requested_count: usize,
	#[serde(default)]
	avatar_intent_emitted_count: usize,
	#[serde(default)]
	avatar_intent_target_link_id: Option<String>,
	#[serde(default)]
	avatar_intent_error: Option<String>,
	#[serde(default)]
	avatar_pose_requested: bool,
	#[serde(default)]
	avatar_pose_emitted: bool,
	#[serde(default)]
	avatar_pose_target_link_id: Option<String>,
	#[serde(default)]
	avatar_pose_preset: Option<String>,
	#[serde(default)]
	avatar_pose_error: Option<String>,
	decided_at: String,
}

#[derive(Debug, Deserialize)]
struct EventHistoryResponse {
	events: Vec<TimestampedEvent>,
}

#[derive(Debug, Deserialize)]
struct TimestampedEvent {
	event: Value,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct Usage {
	completed_reactions: u64,
	failed_reactions: u64,
	api_requests: u64,
	tool_calls: u64,
	input_tokens: u64,
	cached_tokens: u64,
	cache_write_tokens: u64,
	output_tokens: u64,
	reasoning_tokens: u64,
	last_latency_ms: Option<u64>,
}

impl Usage {
	fn delta_from(&self, before: &Self) -> Self {
		Self {
			completed_reactions: self.completed_reactions.saturating_sub(before.completed_reactions),
			failed_reactions: self.failed_reactions.saturating_sub(before.failed_reactions),
			api_requests: self.api_requests.saturating_sub(before.api_requests),
			tool_calls: self.tool_calls.saturating_sub(before.tool_calls),
			input_tokens: self.input_tokens.saturating_sub(before.input_tokens),
			cached_tokens: self.cached_tokens.saturating_sub(before.cached_tokens),
			cache_write_tokens: self.cache_write_tokens.saturating_sub(before.cache_write_tokens),
			output_tokens: self.output_tokens.saturating_sub(before.output_tokens),
			reasoning_tokens: self.reasoning_tokens.saturating_sub(before.reasoning_tokens),
			last_latency_ms: self.last_latency_ms,
		}
	}
}

#[derive(Debug, Serialize)]
struct CaseResult {
	id: String,
	observation_id: u64,
	elapsed_ms: u64,
	event: AiTurnDecisionEvent,
	failures: Vec<String>,
}

#[derive(Debug, Serialize)]
struct EvaluationReport {
	schema: &'static str,
	fixture: String,
	fixture_path: String,
	base_url: String,
	persona_id: String,
	app_version: String,
	started_at: String,
	completed_at: String,
	case_count: usize,
	failed_cases: usize,
	spoken_cases: usize,
	avatar_intents_requested: usize,
	avatar_intents_emitted: usize,
	avatar_intent_errors: usize,
	avatar_poses_requested: usize,
	avatar_poses_emitted: usize,
	avatar_pose_errors: usize,
	average_elapsed_ms: u64,
	p95_elapsed_ms: u64,
	usage_before: Usage,
	usage_after: Usage,
	usage_delta: Usage,
	results: Vec<CaseResult>,
}

fn evaluate_case(
	case: &EvaluationCase,
	speech_policy: &SpeechPolicy,
	event: &AiTurnDecisionEvent,
	previous_episode_id: Option<&str>,
) -> Vec<String> {
	let mut failures = Vec::new();
	match case.avatar_pose.as_str() {
		"required" if !event.avatar_pose_requested || !event.avatar_pose_emitted => {
			failures.push("avatar pose was required but not emitted".into())
		}
		"forbidden" if event.avatar_pose_requested || event.avatar_pose_emitted => failures.push("avatar pose was forbidden".into()),
		_ => {}
	}
	if let Some(error) = &event.avatar_pose_error {
		failures.push(format!("avatar pose failed: {error}"));
	}
	if event.avatar_pose_emitted && !event.avatar_pose_requested {
		failures.push("avatar pose emitted without request".into());
	}
	if !case.allowed_actions.is_empty() && !case.allowed_actions.iter().any(|action| action == &event.action) {
		failures.push(format!("action `{}` not in {:?}", event.action, case.allowed_actions));
	}
	match case.speech.as_str() {
		"required" if event.speak.is_none() => failures.push("speech was required".to_string()),
		"forbidden" if event.speak.is_some() => failures.push("speech was forbidden".to_string()),
		_ => {}
	}
	match case.avatar_intent.as_str() {
		"required" if event.avatar_intent_requested_count == 0 => failures.push("avatar intent was required".to_string()),
		"forbidden" if event.avatar_intent_requested_count > 0 => failures.push(format!(
			"avatar intent was forbidden but {} intent(s) were requested",
			event.avatar_intent_requested_count
		)),
		_ => {}
	}
	if event.avatar_intent_emitted_count > event.avatar_intent_requested_count {
		failures.push(format!(
			"emitted avatar intent count {} exceeds requested count {}",
			event.avatar_intent_emitted_count, event.avatar_intent_requested_count
		));
	}
	if event.avatar_intent_error.is_some() && event.avatar_intent_emitted_count > 0 {
		failures.push("avatar intent has both an error and emitted intents".to_string());
	}
	match case.episode.as_str() {
		"active" if event.episode_id.is_none() => failures.push("active episode was required".to_string()),
		"none" if event.episode_id.is_some() => failures.push("episode must be absent".to_string()),
		_ => {}
	}
	if !case.allowed_episode_phases.is_empty()
		&& !event
			.episode_phase
			.as_ref()
			.is_some_and(|phase| case.allowed_episode_phases.iter().any(|allowed| allowed == phase))
	{
		failures.push(format!(
			"episode phase {:?} not in {:?}",
			event.episode_phase, case.allowed_episode_phases
		));
	}
	if !case.episode_memory_includes.is_empty() {
		let memory = event.episode_memory.as_deref().unwrap_or("");
		for expected in &case.episode_memory_includes {
			if !memory.contains(expected) {
				failures.push(format!("episode memory does not include `{expected}`: {memory:?}"));
			}
		}
	}
	for expected in &case.speech_includes {
		if !event.speak.as_deref().unwrap_or("").contains(expected) {
			failures.push(format!("speech missing expected text `{expected}`: {:?}", event.speak));
		}
	}
	if let Some(speech) = event.speak.as_deref() {
		let char_count = speech.chars().count();
		if speech_policy.max_chars.is_some_and(|max| char_count > max) {
			failures.push(format!(
				"speech has {char_count} characters, above limit {:?}: {speech:?}",
				speech_policy.max_chars
			));
		}
		let sentence_count = count_sentences(speech);
		if speech_policy.max_sentences.is_some_and(|max| sentence_count > max) {
			failures.push(format!(
				"speech has {sentence_count} sentences, above limit {:?}: {speech:?}",
				speech_policy.max_sentences
			));
		}
		if speech_policy.forbid_question_ending && ends_with_question(speech) {
			failures.push(format!("speech ends with a follow-up question: {speech:?}"));
		}
		for forbidden in &case.speech_forbids {
			if speech.contains(forbidden) {
				failures.push(format!("speech contains forbidden unsupported claim `{forbidden}`: {speech:?}"));
			}
		}
	}
	match case.episode_relation.as_str() {
		"same" if event.episode_id.as_deref() != previous_episode_id => failures.push(format!(
			"episode {:?} differs from previous {:?}",
			event.episode_id, previous_episode_id
		)),
		"different"
			if event.episode_id.is_none() || previous_episode_id.is_none() || event.episode_id.as_deref() == previous_episode_id =>
		{
			failures.push(format!("episode {:?} was not new from {:?}", event.episode_id, previous_episode_id))
		}
		"none" if event.episode_id.is_some() => failures.push("episode relation required no episode".to_string()),
		_ => {}
	}
	failures
}

fn count_sentences(speech: &str) -> usize {
	let terminal_count = speech.chars().filter(|ch| matches!(ch, '。' | '！' | '？' | '!' | '?')).count();
	if terminal_count == 0 && !speech.trim().is_empty() {
		1
	} else {
		terminal_count
	}
}

fn ends_with_question(speech: &str) -> bool {
	speech
		.trim_end_matches(|ch: char| ch.is_whitespace() || matches!(ch, '」' | '』' | '）' | ')' | ']' | '】'))
		.ends_with(['?', '？'])
}

fn percentile_nearest_rank(values: impl Iterator<Item = u64>, percentile: usize) -> u64 {
	let mut values = values.collect::<Vec<_>>();
	values.sort_unstable();
	let rank = (percentile.clamp(1, 100) * values.len()).div_ceil(100);
	values[rank.saturating_sub(1)]
}

fn wait_for_decision(base_url: &str, token: &str, persona_id: &str, observation_id: u64, timeout: Duration) -> Result<AiTurnDecisionEvent> {
	let started = Instant::now();
	loop {
		let value = get_json(
			&format!("{base_url}/api/v1/control/events/history?kind=ai_turn_decision&limit=500"),
			token,
		)?;
		let history: EventHistoryResponse = serde_json::from_value(value)?;
		for item in history.events.into_iter().rev() {
			if item.event.get("kind").and_then(Value::as_str) != Some("ai_turn_decision") {
				continue;
			}
			let event: AiTurnDecisionEvent = serde_json::from_value(item.event)?;
			if event.persona_id == persona_id && event.observation_datum_id == observation_id {
				return Ok(event);
			}
		}
		if started.elapsed() >= timeout {
			return Err(format!("timed out waiting for ai_turn_decision: persona={persona_id} observation_id={observation_id}").into());
		}
		thread::sleep(Duration::from_millis(250));
	}
}

fn persona_usage(snapshot: &Value, persona_id: &str) -> Result<Usage> {
	let personas = snapshot
		.get("ai_personas")
		.and_then(Value::as_array)
		.ok_or("snapshot has no ai_personas array")?;
	let persona = personas
		.iter()
		.find(|item| item.get("id").and_then(Value::as_str) == Some(persona_id))
		.ok_or_else(|| format!("AI persona `{persona_id}` is not running"))?;
	Ok(serde_json::from_value(
		persona.get("usage").cloned().ok_or("AI persona has no usage")?,
	)?)
}

fn get_json(uri: &str, token: &str) -> Result<Value> {
	let mut request = ureq::get(uri);
	if !token.trim().is_empty() {
		request = request.header("Authorization", &format!("Bearer {token}"));
	}
	let response = request.call().map_err(|error| format!("GET {uri}: {error}"))?;
	read_json(response)
}

fn post_json(uri: &str, token: &str, body: &Value) -> Result<Value> {
	let mut request = ureq::post(uri).header("Content-Type", "application/json; charset=utf-8");
	if !token.trim().is_empty() {
		request = request.header("Authorization", &format!("Bearer {token}"));
	}
	let response = request
		.send(serde_json::to_vec(body)?)
		.map_err(|error| format!("POST {uri}: {error}"))?;
	read_json(response)
}

fn read_json(response: ureq::http::Response<ureq::Body>) -> Result<Value> {
	let mut reader = response.into_body().into_reader();
	let mut text = String::new();
	reader.read_to_string(&mut text)?;
	Ok(serde_json::from_str(&text)?)
}

fn resolve_path(root: &Path, value: &str) -> PathBuf {
	let path = PathBuf::from(value);
	if path.is_absolute() {
		path
	} else {
		root.join(path)
	}
}

fn default_output_path(root: &Path) -> PathBuf {
	let epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
	root.join("target").join("ai-turn-eval").join(format!("report-{epoch}.json"))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn event(action: &str, speak: Option<&str>, episode_id: Option<&str>, phase: Option<&str>) -> AiTurnDecisionEvent {
		AiTurnDecisionEvent {
			persona_id: "kaltsit".to_string(),
			observation_channel: "user".to_string(),
			observation_datum_id: 1,
			action: action.to_string(),
			speak: speak.map(str::to_string),
			episode_id: episode_id.map(str::to_string),
			episode_phase: phase.map(str::to_string),
			episode_memory: None,
			episode_turn_count: None,
			episode_phase_remaining_secs: None,
			avatar_intent_requested_count: 0,
			avatar_intent_emitted_count: 0,
			avatar_intent_target_link_id: None,
			avatar_intent_error: None,
			avatar_pose_requested: false,
			avatar_pose_emitted: false,
			avatar_pose_target_link_id: None,
			avatar_pose_preset: None,
			avatar_pose_error: None,
			decided_at: "2026-07-25T00:00:00Z".to_string(),
		}
	}

	#[test]
	fn expectation_checks_speech_and_episode_relation() {
		let speech_policy = SpeechPolicy::default();
		let case = EvaluationCase {
			id: "continue".to_string(),
			actor: None,
			channel: None,
			content: "追加情報".to_string(),
			delay_before_ms: 0,
			allowed_actions: vec!["respond".to_string(), "observe".to_string()],
			speech: "any".to_string(),
			episode: "active".to_string(),
			allowed_episode_phases: vec!["yielding".to_string()],
			episode_memory_includes: vec!["追加".to_string()],
			speech_forbids: vec!["安全".to_string()],
			speech_includes: Vec::new(),
			avatar_intent: "any".to_string(),
			avatar_pose: "any".to_string(),
			episode_relation: "same".to_string(),
		};
		let mut matching = event("observe", None, Some("k:1"), Some("yielding"));
		matching.episode_memory = Some("追加情報を受領".to_string());
		assert!(evaluate_case(&case, &speech_policy, &matching, Some("k:1")).is_empty());
		assert!(!evaluate_case(
			&case,
			&speech_policy,
			&event("react", Some("x"), Some("k:2"), Some("yielding")),
			Some("k:1")
		)
		.is_empty());
	}

	#[test]
	fn speech_policy_checks_length_sentences_and_follow_up_question() {
		let policy = SpeechPolicy {
			max_chars: Some(12),
			max_sentences: Some(2),
			forbid_question_ending: true,
		};
		let case = EvaluationCase {
			id: "speech-policy".to_string(),
			actor: None,
			channel: None,
			content: "応答".to_string(),
			delay_before_ms: 0,
			allowed_actions: vec!["respond".to_string()],
			speech: "required".to_string(),
			episode: "any".to_string(),
			allowed_episode_phases: Vec::new(),
			episode_memory_includes: Vec::new(),
			speech_forbids: Vec::new(),
			speech_includes: vec!["応答".to_string()],
			avatar_intent: "any".to_string(),
			avatar_pose: "any".to_string(),
			episode_relation: "any".to_string(),
		};

		assert!(evaluate_case(&case, &policy, &event("respond", Some("短い応答だ。"), None, None), None).is_empty());
		assert!(evaluate_case(&case, &policy, &event("respond", None, None, None), None)
			.iter().any(|failure| failure.contains("missing expected text")));
		assert!(evaluate_case(&case, &policy, &event("respond", Some("別の内容。"), None, None), None)
			.iter().any(|failure| failure.contains("missing expected text")));
		let failures = evaluate_case(
			&case,
			&policy,
			&event("respond", Some("これは長すぎる応答だ。さらに続ける。どう思う？"), None, None),
			None,
		);
		assert_eq!(failures.len(), 3);
	}

	#[test]
	fn avatar_intent_expectation_checks_requested_and_emitted_counts() {
		let policy = SpeechPolicy::default();
		let case = EvaluationCase {
			id: "quiet-observation".to_string(),
			actor: None,
			channel: None,
			content: "次へ進む".to_string(),
			delay_before_ms: 0,
			allowed_actions: vec!["observe".to_string()],
			speech: "forbidden".to_string(),
			episode: "none".to_string(),
			allowed_episode_phases: Vec::new(),
			episode_memory_includes: Vec::new(),
			speech_forbids: Vec::new(),
			speech_includes: Vec::new(),
			avatar_intent: "forbidden".to_string(),
			avatar_pose: "any".to_string(),
			episode_relation: "none".to_string(),
		};
		let quiet = event("observe", None, None, None);
		assert!(evaluate_case(&case, &policy, &quiet, None).is_empty());

		let mut overactive = quiet.clone();
		overactive.avatar_intent_requested_count = 1;
		overactive.avatar_intent_target_link_id = Some("kaltsit_ai".to_string());
		assert!(evaluate_case(&case, &policy, &overactive, None)
			.iter()
			.any(|failure| failure.contains("avatar intent was forbidden")));
	}

	#[test]
	fn percentile_uses_nearest_rank() {
		assert_eq!(percentile_nearest_rank([100, 400, 200, 300].into_iter(), 95), 400);
		assert_eq!(percentile_nearest_rank([100, 400, 200, 300].into_iter(), 50), 200);
	}

	#[test]
	fn pose_expectation_requires_delivery_and_reports_errors() {
		let mut case: EvaluationCase = serde_json::from_value(serde_json::json!({
			"id": "pose", "content": "両手を広げて", "avatar_pose": "required"
		}))
		.unwrap();
		let policy = SpeechPolicy::default();
		let mut result = event("respond", Some("楽しみだ。"), None, None);
		assert!(!evaluate_case(&case, &policy, &result, None).is_empty());
		result.avatar_pose_requested = true;
		assert!(!evaluate_case(&case, &policy, &result, None).is_empty());
		result.avatar_pose_emitted = true;
		result.avatar_pose_preset = Some("open_hands".into());
		result.avatar_pose_target_link_id = Some("kaltsit_ai".into());
		let result: AiTurnDecisionEvent = serde_json::from_value(serde_json::to_value(result).unwrap()).unwrap();
		assert!(evaluate_case(&case, &policy, &result, None).is_empty());
		assert_eq!(result.avatar_pose_preset.as_deref(), Some("open_hands"));
		case.avatar_pose = "forbidden".into();
		assert!(!evaluate_case(&case, &policy, &result, None).is_empty());
		case.avatar_pose = "any".into();
		let mut failed = result;
		failed.avatar_pose_error = Some("invalid target".into());
		failed.avatar_pose_emitted = false;
		assert!(evaluate_case(&case, &policy, &failed, None)
			.iter()
			.any(|f| f.contains("invalid target")));
		case.avatar_pose = "sometimes".into();
		assert!(case.validate().is_err());
	}

	#[test]
	fn older_events_without_pose_fields_remain_readable() {
		let mut value = serde_json::to_value(event("observe", None, None, None)).unwrap();
		value.as_object_mut().unwrap().retain(|key, _| !key.starts_with("avatar_pose"));
		let parsed: AiTurnDecisionEvent = serde_json::from_value(value).unwrap();
		assert!(!parsed.avatar_pose_requested);
		assert!(!parsed.avatar_pose_emitted);
		assert!(parsed.avatar_pose_error.is_none());
	}
}
