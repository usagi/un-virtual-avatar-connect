//! 常駐 AI サービス本体。
//!
//! 旧 `src/processor/openai_chat/mod.rs` の `OpenAiChat` プロセッサーを Phase I で「1 ペルソナ = 1 常駐タスク」の
//! [`AiService`] に昇格させた。プロセッサーの `is_channel_from` + `process` による線形パイプライン駆動ではなく、
//! [`crate::state::State`] から `broadcast` 経由で流れてくる [`Observation`] を event loop で受け取り、
//! `observe.triggers` にマッチしたときだけ応答を評価する。
//!
//! Phase II 以降で加える Decision Engine・Heartbeat・Action 実行はこの event loop を拡張する形で入れる。

use super::completion;
use super::config::{AiPersonaConf, OpenAiChatFinetuning, OpenAiReasoningEffortConf};
use super::context;
use super::decision::{uniform_jitter, Decision, DecisionInput, DecisionSpec};
use super::episode::EpisodeMemory;
use super::initiative::{self, InitiativeCandidateKind, InitiativePolicyInput, InitiativePolicyOutcome};
use super::model_policy;
use super::observe::{Observation, ObserveSet};
use super::reload::make_responses_request_template;
use super::streaming_turn::{SpeechSegmenter, TurnStreamDecoder};
use super::tools::{self, ToolContext};
use super::{ENV_OPENAI_API_KEY, ENV_OPENAI_MAX_OUTPUT_TOKENS};

use crate::ai::openai_responses::types::input::InputItem;
use crate::ai::openai_responses::types::request::{
	CreateResponseRequest, PromptCacheMode, PromptCacheOptions, PromptCacheTtl, ReasoningEffort, TextConfig, TextFormat, ToolChoice,
	ToolChoiceMode,
};
use crate::ai::openai_responses::types::response::OutputItem;
use crate::ai::openai_responses::types::stream::StreamEvent;
use crate::ai::openai_responses::util::extract_output_text;
use crate::ai::openai_responses::ResponsesClient;
#[cfg(any(test, not(feature="desktop-native-review")))]
use crate::ai::openai_responses::ResponsesClientConfig;
use crate::conf::{TwitchEventSubConfig, TwitchModeratorConfig};
use crate::state::{AiRuntime, AiUsageTelemetry};
use crate::{ChannelDatum, SharedChannelData, SharedState};

use anyhow::{anyhow, bail, Context, Result};
use futures::StreamExt;
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, watch, Mutex, RwLock, Semaphore};
use vac_motion::avatar_intent::{AvatarIntent, AvatarIntentRequest};

const DEFAULT_MEMORY_CAPACITY: usize = 4;
const DEFAULT_REMOVE_CHARS: &str = "\n\r\t";
const DEFAULT_OPENAI_MAX_IN_FLIGHT: usize = 2;
const DEFAULT_OPENAI_STREAM: bool = true;
const DEFAULT_OVERFLOW_SUMMARY_MIN_CHARS: usize = 64;
const INITIATIVE_SOURCE_KIND: &str = "ai.initiative";
const INITIATIVE_INSTRUCTIONS: &str = r#"これは人間の発話への応答ではなく、配信中の自発参加候補の評価だ。
直前の「AI自発参加候補」developer messageと、Twitch宣言状態、画像観測、直近の会話だけを根拠にする。
新しい配信価値を短く加えられる場合だけreactを選ぶ。相槌、状況の言い換え、未観測の攻略助言、会話を続けるための質問はobserveとする。
respondとcloseは選ばない。observeではspeakをnullにする。"#;
const ONGOING_CONVERSATION_INITIATIVE_INSTRUCTIONS: &str = r#"これは継続中の配信へ、独立した頻度上限を通過して得た自発参加枠だ。
直近の会話または明示された観測に根拠を持つ、重複しない短い一言をreactとして発話する。実況の単なる復唱、未観測の攻略助言、会話を引き延ばす質問は作らない。
ただし、直近の会話で配信者が返答不要・静かに見守ることを明示し、その後に新しい呼びかけや応答を求める発言がない場合はobserve、speak=nullとする。この要求は自発参加枠より優先する。「黙って見ている」などの沈黙の宣言も発話しない。
それ以外はreactを選び、respondとcloseは選ばない。"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReactionOrigin {
	Observation,
	Initiative(InitiativeCandidateKind),
}

#[derive(Debug, Default)]
struct InitiativeRuntime {
	online_since_ms: Option<u64>,
	attempt_times_ms: VecDeque<u64>,
	speech_times_ms: VecDeque<u64>,
}
const TURN_MANAGER_INSTRUCTIONS: &str = r#"配信AI共演者として、最新の観測に対する行動を選べ。
判定対象は、この指示に続く実際の会話履歴のうち最新の会話発話だ。user roleで渡される画像観測は発話ではなく参考情報であり、判定対象を置き換えない。過去の会話や引用例を判定対象と取り違えるな。AI自発参加候補の場合は、別途指定される自発参加の指示に従う。
次の優先順位で判定し、上位に該当したら下位を選ぶな。
1. AIの名前を呼ぶ明示的な呼びかけ、またはAIへ意見、判断、回答、作業を求める明示的な質問や依頼 -> respond。ゲーム中の発言でも observe にしない。
   名前を呼び直していなくても、直前のAIの発言に新しい感想や考えを返して対話を続けている発言はrespondの対象とする。明示的な質問だけに限定しない。ただし単なる相槌、終了承認、自己解決、ゲーム操作へ戻る発言は含めない。会話の継続だけを理由に新しい質問や助言を付け足さない。
2. 通常進行ではない重要な出来事や、共有された状況への明確で強い感情 -> react。
3. AIとの会話に対する合意や終了承認 -> close または observe。
4. 実況、操作宣言、予定、独り言、通常進行 -> observe。

直前に「Twitchチャネルポイント交換による高優先度依頼」developer messageがある場合は必ずrespondを選ぶ。視聴者がポイントを支払った依頼なので、通常の短い相槌ではなく、現在日時・会話文脈・利用可能なツールを使って実質的に答える。システム時刻で解決できる「今年」や「今日」を聞き返さない。

上記に該当しない場合の既定は observe、つまり沈黙だ。
- observe: 配信者の実況、操作宣言、状況の独り言、会話終了後の進行。speak は null。
- respond: AIへ返答を求める明示的な質問、感想の共有、相談、依頼。speak は必要十分な短文。respondを選ぶことは、問題解決や助言を始めることではない。雑談に自然な軽い提案は残してよいが、相談や作業の依頼がないのに必要情報を要求したり、手順整理・課題解決の受付へ会話を切り替えない。明示的な相談や依頼には具体的に応じる。
- react: 通常進行ではない重要な出来事や強い感情へ、配信価値のある短い反応を返す。求められていない助言、原因、次の行動は足さない。
- close: AIとの会話に対する合意や終了承認を短く返し、会話を閉じる。新しい質問を作らない。

対比例:
- 「まずは右側を確認します」-> observe
- 「ではゲームに戻ります」-> observe
- 「この進め方で大丈夫だと思う？」-> respond。明示的な質問なので observe は誤り。
- 「ケルシー先生、聞こえている？」-> respond。名指しの呼びかけを沈黙で無視するな。
- 「さっきの戦闘、右から回った方がよかったと思う？」-> respond。明示的な意見要求なので observe は誤り。
- 「先生、仕組みがまだ分からないからこそ、あれこれ想像してしまうんだよな。どう思う？」-> respond。考察の面白さを共有する誘いだ。未確定なことは未確定のまま、手探りや着眼点への興味を返す。真偽判定や検証手順の依頼へ変えない。
- 「敵は左側に固まり、右側に配置場所が二つ残っていた」-> 現在の相談への追加根拠。返すなら「その情報なら、右回りを検討する根拠はある」までとし、未観測の安全性や有利不利を足さない。
- 「今のはさすがにひどいな」-> react。共有状況への強い評価なので、短い反応には配信価値がある。
- 「次は装備を見直してから出るつもり」-> observe。予定の共有だけなので、助言を求められていない。
- 「少し考える。まだ決めなくていい」-> observe。待機や思考の宣言に相槌を返して会話を延長しない。
- 「そうだな、それで行こう」-> close または observe

単なる実況を依頼と解釈するな。配信者へ報告や追加説明を要求するな。会話を続けることだけを目的とした質問を生成するな。考えた内容をすべて発話する必要はない。
観測にない理由、条件、利点、危険、次の手順をもっともらしく補うな。「たぶん」「可能性がある」と弱めても、根拠のない内容を作ることに変わりはない。
事実、原因、優劣の判定を求められて判断材料が足りない場合は、どちらかを推測せず、足りない範囲だけを短く示せ。
感想、興奮、謎や考察の面白さを共有する誘いには、述べられた出来事・気持ち・仮説の範囲で一緒に反応しろ。画面や確定した答えがなくても、その共有まで「判断できない」で終える必要はない。
「かもしれない」という仮説を確定したゲームの仕組みへ変えるな。相槌の理由づけでも、観測にない勝因やプレイ内容を足すな。原因分析、改善案、検証手順は明示的に求められた場合に返せ。
現在の会話エピソードがrefractoryでも、新しい明示的な質問や依頼にはrespondできる。その場合は完了済みの話題を混ぜず、別の会話として扱え。

episode_memory は現在の会話エピソードを更新する短い記憶だ。
- respondでは、現在の話題と観測済みの根拠を一つの短文へ圧縮する。遊びや考察を共有する会話では気持ちや未確定の仮説を保ち、解決すべき課題へ変えない。相談なら未解決点も残す。
- closeでは、解決または合意した内容を一つの短文にする。
- observeまたはreactでは、現在の会話エピソードへ直接関係する新情報がある場合だけ更新後の短文を返す。
- 最新の観測が現在の話題へ新しい根拠を加えた場合は、発話しなくても、その根拠を取り込んだepisode_memoryを必ず返す。
- 新しい根拠があるのに、直前のepisode_memoryを変更せずそのまま返すな。
- 通常実況、操作宣言、予定、単発の感情反応、現在の話題と無関係な観測ではnull。
観測にない内容を足さない。会話を続けるための新しい問いを記憶へ作らない。

avatar_intent は、発話や重要な反応へ自然な身体表現を添える場合だけ返す。不要ならnull。
- 1回につき1〜2個までの小さな表情、頷き、首振り、視線に限る。毎回は使わない。
- observeでは必ずnull。沈黙を身体反応で埋めるな。
- 中立的・分析的なrespondは原則null。明確な肯定、懸念、制止を発話する場合だけ、その意味に合う小さな表情または頭部動作を1個添えてよい。
- 明示的な合意をcloseする場合は、小さなhead_nodを1個添えてよい。それ以外のcloseはnullでよい。
- 強い感情をreactとして発話する場合は、その感情に合う小さなexpressionを1個返す。失敗への強い否定ならannoyedまたはconcerned、成功への喜びならsmileを使う。
- look_atは観測中の明確な注視対象がある場合だけ使う。単なる会話の装飾には使わない。
- intensityは0.6以下、duration_msは2000以下、頷きと首振りは1回、視線角度は15度以内。
- expressionのcurveは、holdならduration_msの間その強度を保ち、linear（省略時）とease_outなら開始直後から弱まる。ease_outは特に早く弱まる。発話に添えて感情を伝える表情はholdで900〜1400msを目安にし、一瞬の驚きや短い反応を消したい場合は減衰を選ぶ。強度を上げる代わりに必要な時間だけ保ち、発話の長さ全体には引き延ばさない。この使い分けのために表情の回数を増やさない。
- target_link_idは追加のsystem instructionで指定されたAvatarLink IDをそのまま使う。
- avatar_poseは、追加のsystem instructionで許可された名前付きポーズを発話へ添える場合だけ返す。不要ならnull。
- presetは許可された名前をそのまま使い、ファイルパスや未提示の名前を作らない。
- observeでは必ずnull。respondまたはreactでも、説明や強調に全身・腕のポーズが明確に有用な場合だけ使い、毎回は使わない。
- avatar_intentとavatar_poseは併用できる。avatar_poseが腕や身体、avatar_intentが表情や小さな頭部動作を担当する。
"#;

// ============================================================
// χ-6: conf 値を Responses API の request 値へ resolve するヘルパ。
//
// 優先順位は `env > 新 conf キー > legacy conf キー > None（= API 既定）`。
// 新旧両方指定されても warn は出さない（un-discord 方針に合わせ "warn 無しで fallback"）。
// ============================================================

/// `openai_max_output_tokens` を env / 新 conf / legacy `max_tokens` の順に解決する。
///
/// - env `VAC_OPENAI_MAX_OUTPUT_TOKENS`: 数値パース失敗時は warn ログを出して無視
/// - 新キー `openai_max_output_tokens` (u32) → そのまま
/// - legacy `max_tokens` (u16) → u32 にキャスト
fn resolve_openai_max_output_tokens(persona: &AiPersonaConf) -> Option<u32> {
	resolve_openai_max_output_tokens_with_env(persona, std::env::var(ENV_OPENAI_MAX_OUTPUT_TOKENS).ok().as_deref())
}

fn resolve_openai_max_output_tokens_with_env(persona: &AiPersonaConf, env_value: Option<&str>) -> Option<u32> {
	if let Some(raw) = env_value {
		match raw.trim().parse::<u32>() {
			Ok(v) => return Some(v),
			Err(e) => log::warn!(
				"環境変数 {} = {:?} は u32 に解釈できませんでした（{}）。conf のフォールバックを使います。",
				ENV_OPENAI_MAX_OUTPUT_TOKENS,
				raw,
				e
			),
		}
	}
	if let Some(v) = persona.openai_max_output_tokens {
		return Some(v);
	}
	persona.max_tokens.map(u32::from)
}

/// tool loop round 間 reasoning pass-through を有効にするかを解決する。
///
/// - conf `openai_reasoning_encrypted_passthrough = true/false` が明示されていればそれを優先
/// - 未指定なら既定true。実際の対応モデルはmodel_policyで判定する。
fn resolve_reasoning_encrypted_passthrough(persona: &AiPersonaConf) -> bool {
	persona.openai_reasoning_encrypted_passthrough.unwrap_or(true)
}

/// ψ-α: `include: ["reasoning.encrypted_content"]` の付与をモデル判定と enabled flag に基づいて適用する。
///
/// - 対応モデルかつenabledのときだけincludeに追加（重複しない）
/// - 未対応モデルまたはdisabledではrequestを変更しない
fn apply_reasoning_passthrough_include(request: &mut CreateResponseRequest, model: Option<&str>, enabled: bool) {
	if !(enabled && model_policy::supports_reasoning_history(model)) {
		return;
	}
	let entry = "reasoning.encrypted_content".to_string();
	let list = request.include.get_or_insert_with(Vec::new);
	if !list.iter().any(|s| s == &entry) {
		list.push(entry);
	}
}

/// ψ-α: `response.output` 中の `OutputItem::Reasoning` を次ラウンド input 用の
/// `InputItem::Reasoning` に変換して返す。順序は出力順を維持する。
///
/// 返り値の 2 番目は `encrypted_content` が欠落していた件数。`include` を指定した
/// request に対して API 側が blob を返さないケースを観測したい目的で使う（warn で露出）。
fn collect_reasoning_input_items(output: &[OutputItem]) -> (Vec<InputItem>, usize) {
	let mut items = Vec::new();
	let mut missing = 0usize;
	for item in output {
		if let OutputItem::Reasoning {
			id,
			encrypted_content,
			summary,
			..
		} = item
		{
			if encrypted_content.is_none() {
				missing += 1;
			}
			items.push(InputItem::Reasoning {
				id: id.clone(),
				encrypted_content: encrypted_content.clone(),
				summary: summary.clone(),
			});
		}
	}
	(items, missing)
}

/// `openai_reasoning_effort` の conf enum を Responses API の `ReasoningEffort` に写す。
fn resolve_openai_reasoning_effort(persona: &AiPersonaConf) -> Option<ReasoningEffort> {
	persona.openai_reasoning_effort.map(|e| match e {
		OpenAiReasoningEffortConf::None => ReasoningEffort::None,
		OpenAiReasoningEffortConf::Minimal => ReasoningEffort::Minimal,
		OpenAiReasoningEffortConf::Low => ReasoningEffort::Low,
		OpenAiReasoningEffortConf::Medium => ReasoningEffort::Medium,
		OpenAiReasoningEffortConf::High => ReasoningEffort::High,
		OpenAiReasoningEffortConf::Xhigh => ReasoningEffort::Xhigh,
		OpenAiReasoningEffortConf::Max => ReasoningEffort::Max,
	})
}

/// オーバーフロー要約で使う `max_output_tokens` を新キー > legacy の順に解決する。
fn resolve_overflow_summary_max_output_tokens(persona: &AiPersonaConf) -> Option<u32> {
	if let Some(v) = persona.memory_overflow_summary_max_output_tokens {
		return Some(v);
	}
	persona.memory_overflow_summary_max_completion_tokens.map(u32::from)
}

/// 1 ペルソナ分の AI サービス。`Clone` で同一リソースを共有する（Arc + broadcast）。
///
/// Phase VI-α-4 以降は `persona` / `request_template` / `decision` が
/// **Control API からの hot-reload で atomic に差し替えうる**。そのため外部からこれらを参照する呼び出しは
/// すべて `read().await` を通過させる必要がある。書き換えは [`super::reload::AiReloadHandle`] 経由で行う。
#[derive(Clone)]
pub struct AiService {
	/// ペルソナ設定。hot-reload で `instructions` / `heartbeat.enabled` / `decision.threshold` が変わりうる。
	persona: Arc<RwLock<Arc<AiPersonaConf>>>,
	/// 起動時に固定される表示用ラベル（id ベース）。reload 対象ではないため同期 API に使える。
	persona_label: String,
	channel_utterance: String,
	observe: Arc<ObserveSet>,
	state: SharedState,
	channel_data: SharedChannelData,
	/// χ-5: OpenAI Responses API `/v1/responses` 用の自前 client。
	client: ResponsesClient,
	/// χ-5: Responses API リクエストテンプレート。
	/// `reload::make_responses_request_template()` の結果を atomic swap する。
	request_template: Arc<RwLock<Arc<CreateResponseRequest>>>,
	last_activated: Arc<Mutex<SystemTime>>,
	last_claimed_observation_id: Arc<AtomicU64>,
	/// Phase II: 旧 `force_activate_regex` / `ignore_regex` / `min_interval` を包含するスコアリング仕様。
	/// Phase VI-α-4 で reload 対象になったので `RwLock<Arc<_>>`。
	decision: Arc<RwLock<Arc<DecisionSpec>>>,
	episode_memory: Arc<Mutex<EpisodeMemory>>,
	visual_history: Arc<Mutex<super::visual_history::VisualHistory>>,
	past_screen: Arc<Mutex<super::past_screen::PastScreen>>,
	in_flight: Arc<Semaphore>,
	in_flight_capacity: usize,
	overflow_summary_last_at: Arc<Mutex<Option<Instant>>>,
	/// Phase IV: Actions。Twitch 系 Helix ツールに渡す資格情報の所在。未指定時はその系統のツール呼び出しは失敗する。
	twitch_eventsub: Option<Arc<TwitchEventSubConfig>>,
	/// Phase V-c: モデレーション系ツール用のボット/モデレーターアカウント設定。
	twitch_moderator: Option<Arc<TwitchModeratorConfig>>,
	/// `vac_twitch_chat_say` 等で `broadcaster_login` 引数が省略されたときのフォールバック。
	twitch_default_broadcaster_login: Option<String>,
	/// Phase VI-α-2: Control API からの soft pause。`true` の間は新規の `react()` / heartbeat 評価をスキップする。
	/// 既に走っている応答は中断しない（soft suspend）。hard abort はタスクハンドル側から行う別系統。
	paused: Arc<AtomicBool>,
	usage: Arc<RwLock<AiUsageTelemetry>>,
	initiative_runtime: Arc<Mutex<InitiativeRuntime>>,
}

impl AiService {
	pub async fn new(
		persona: AiPersonaConf,
		state: SharedState,
		twitch_eventsub: Option<Arc<TwitchEventSubConfig>>,
		twitch_moderator: Option<Arc<TwitchModeratorConfig>>,
		twitch_default_broadcaster_login: Option<String>,
		paused: Arc<AtomicBool>,
		usage: Arc<RwLock<AiUsageTelemetry>>,
	) -> Result<Self> {
		model_policy::validate_model_policy(&persona)?;
		validate_tool_policy_at_startup(&persona).await?;
		let channel_utterance = persona
			.channel_utterance
			.clone()
			.ok_or_else(|| anyhow!("AI ペルソナに channel_utterance が設定されていません: id={:?}", persona.id))?;

		if persona.observe.triggers.is_empty() {
			let heartbeat_enabled = persona.heartbeat.as_ref().map(|h| h.enabled).unwrap_or(false);
			if !heartbeat_enabled {
				log::warn!(
					"AI ペルソナ id={:?} は observe.triggers が空で heartbeat も無効です。応答は発生しません。",
					persona.id
				);
			} else {
				log::info!(
					"AI ペルソナ id={:?} は observe.triggers が空ですが heartbeat が有効なので自発評価は走ります。",
					persona.id
				);
			}
		}

		let observe = ObserveSet::from_conf(&channel_utterance, &persona.observe);

		let client = make_client(&persona)?;
		// χ-6: 新 conf キー（`openai_max_output_tokens` / `openai_reasoning_effort` / `openai_store`）を
		// 優先しつつ env / legacy にフォールバックして request_template に詰める。
		let resolved_max_output_tokens = resolve_openai_max_output_tokens(&persona);
		let resolved_reasoning_effort = resolve_openai_reasoning_effort(&persona);
		let resolved_store = Some(persona.openai_store.unwrap_or(false));
		let request_template =
			make_responses_request_template(&persona, resolved_max_output_tokens, resolved_reasoning_effort, resolved_store)?;

		let decision = Arc::new(DecisionSpec::from_persona(&persona)?);
		log::debug!(
			"《AI[{}]》 Decision engine: {:?} (legacy_compat={})",
			persona.id.as_deref().unwrap_or("-"),
			decision,
			persona.decision.is_none(),
		);

		let max_in_flight = persona.openai_max_in_flight.unwrap_or(DEFAULT_OPENAI_MAX_IN_FLIGHT).max(1);

		if persona.api_key.is_some() {
			log::warn!("================================================================");
			log::warn!(
    "api_key が設定ファイルで直接設定されています。設定ファイルを共有したり一般に公開する際は不慮の漏出に十分に注意して下さい。または環境変数 {} での設定も検討して下さい。",
    ENV_OPENAI_API_KEY
   );
			log::warn!("================================================================");
		}

		let channel_data = state.read().await.channel_data.clone();

		log::info!(
   "《AI[{}]》 サービスを初期化しました: triggers={:?} channel_utterance={:?} model={:?} include_all={} additional={:?} exclude={:?} max_in_flight={} stream={}",
   persona.id.clone().unwrap_or_else(|| "-".to_string()),
   persona.observe.triggers,
   channel_utterance,
   persona.model,
   persona.observe.include_all,
   persona.observe.include_additional,
   persona.observe.exclude,
   max_in_flight,
   persona.openai_stream.unwrap_or(DEFAULT_OPENAI_STREAM),
  );

		let persona_arc = Arc::new(persona);
		let persona_label = persona_arc.id.clone().unwrap_or_else(|| "-".to_string());

		Ok(Self {
			persona: Arc::new(RwLock::new(persona_arc)),
			persona_label,
			channel_utterance,
			observe: Arc::new(observe),
			state,
			channel_data,
			client,
			request_template: Arc::new(RwLock::new(Arc::new(request_template))),
			last_activated: Arc::new(Mutex::new(SystemTime::UNIX_EPOCH)),
			last_claimed_observation_id: Arc::new(AtomicU64::new(0)),
			decision: Arc::new(RwLock::new(decision)),
			episode_memory: Arc::new(Mutex::new(EpisodeMemory::default())),
			visual_history: Arc::new(Mutex::new(super::visual_history::VisualHistory::default())),
			past_screen: Arc::new(Mutex::new(super::past_screen::PastScreen::default())),
			in_flight: Arc::new(Semaphore::new(max_in_flight)),
			in_flight_capacity: max_in_flight,
			overflow_summary_last_at: Arc::new(Mutex::new(None)),
			twitch_eventsub,
			twitch_moderator,
			twitch_default_broadcaster_login,
			paused,
			usage,
			initiative_runtime: Arc::new(Mutex::new(InitiativeRuntime::default())),
		})
	}

	/// Phase VI-α-2: このサービスが現在 paused かどうか。
	pub fn is_paused(&self) -> bool {
		self.paused.load(Ordering::SeqCst)
	}

	/// Phase IV: function-call 型ツール群に渡す実行コンテキストを生成する。
	///
	/// `persona` が reload されるため、tool_context も毎回 read lock を通って最新の `channel_effect` を見る。
	async fn tool_context(&self) -> ToolContext {
		let persona = self.persona.read().await.clone();
		ToolContext {
			state: self.state.clone(),
			persona_label: self.persona_label(),
			effect_channel: persona.channel_effect.clone().unwrap_or_else(|| "effect".to_string()),
			effect_allowed_channels: persona.openai_tool_policy.as_ref().and_then(|policy| policy.effect_allowed_channels.clone()),
			twitch_eventsub: self.twitch_eventsub.clone(),
			twitch_moderator: self.twitch_moderator.clone(),
			twitch_default_broadcaster_login: self.twitch_default_broadcaster_login.clone(),
		}
	}

	pub fn persona_label(&self) -> String {
		// Phase VI-α-4: reload 対象でないため、`new()` 時点でキャッシュしたラベルをそのまま返す。
		self.persona_label.clone()
	}

	/// Phase VI-α-4: Control API `/reload` 用の hot-reload ハンドル。
	/// 同じ `Arc<RwLock<Arc<_>>>` を共有しているので、ここで書き換えれば AiService 側にも即座に反映される。
	pub fn reload_handle(&self) -> super::reload::AiReloadHandle {
		super::reload::AiReloadHandle {
			persona_id: self.persona_label_id_opt(),
			persona: self.persona.clone(),
			request_template: self.request_template.clone(),
			decision: self.decision.clone(),
		}
	}

	/// 起動時にキャッシュしたラベルは `"-"` を代入してあるため、id の有無を復元したいときに使う。
	fn persona_label_id_opt(&self) -> Option<String> {
		if self.persona_label == "-" {
			None
		} else {
			Some(self.persona_label.clone())
		}
	}

	/// event loop。観測の受信を止めず、最新のトリガー観測を単一ワーカーへ渡す。
	///
	/// ワーカー側は短い静穏時間で近接観測をまとめ、`min_interval` 中も最新観測を保持する。
	/// channel data がイベントログなので、最後の観測だけを通知しても、その観測までの文脈は `react()` が収集できる。
	pub async fn run(self, mut rx: broadcast::Receiver<Observation>) {
		let label = self.persona_label();
		log::info!("《AI[{}]》 event loop を起動しました。", label);
		let (pending_tx, pending_rx) = watch::channel::<Option<Observation>>(None);
		let worker = self.clone();
		let worker_handle = tokio::spawn(async move {
			worker.run_observation_worker(pending_rx).await;
		});

		loop {
			match rx.recv().await {
				Ok(obs) => {
					if self.is_paused() {
						log::trace!("《AI[{}]》 paused のため観測 {:?} をスキップします。", label, obs);
						continue;
					}
					let explicit_target = obs.channel.strip_prefix(crate::flowgraph::nodes::ai::FLOWGRAPH_AI_INPUT_PREFIX);
					if explicit_target.is_some_and(|target| target != "*" && target != label) {
						continue;
					}
					if explicit_target.is_none() && !self.observe.is_trigger(&obs.channel) {
						continue;
					}
					self.emit_activation_decision(&obs, crate::control_events::AiActivationOutcome::Accepted, None, None, None, 0)
						.await;
					pending_tx.send_replace(Some(obs));
				}
				Err(broadcast::error::RecvError::Lagged(n)) => {
					log::warn!("《AI[{}]》 観測が {} 件遅延しました（追いつけなかった分は欠落）。", label, n);
				}
				Err(broadcast::error::RecvError::Closed) => {
					log::info!("《AI[{}]》 event loop を終了します（broadcast が閉じられました）。", label);
					break;
				}
			}
		}
		drop(pending_tx);
		let _ = worker_handle.await;
	}

	async fn emit_activation_decision(
		&self,
		obs: &Observation,
		outcome: crate::control_events::AiActivationOutcome,
		score: Option<f64>,
		threshold: Option<f64>,
		remaining_interval_secs: Option<u64>,
		speech_floor_wait_ms: u64,
	) {
		let tx = self.state.read().await.control_event_tx.clone();
		let _ = tx.send(crate::control_events::ControlEvent::AiActivationDecision {
			persona_id: self.persona_label(),
			observation_channel: obs.channel.clone(),
			observation_datum_id: obs.datum_id,
			outcome,
			score,
			threshold,
			remaining_interval_secs,
			speech_floor_wait_ms,
			observed_at: jiff::Timestamp::now().to_string(),
		});
	}

	async fn run_observation_worker(self, mut pending_rx: watch::Receiver<Option<Observation>>) {
		let label = self.persona_label();
		while pending_rx.changed().await.is_ok() {
			let Some(mut latest) = pending_rx.borrow_and_update().clone() else {
				continue;
			};

			loop {
				latest = self.coalesce_observations(&mut pending_rx, latest).await;
				let remaining = {
					let decision = self.decision.read().await;
					let last_activated = *self.last_activated.lock().await;
					decision.remaining_min_interval(last_activated)
				};
				let Some(remaining) = remaining else {
					break;
				};

				log::trace!(
					"《AI[{}]》 observation buffer: 評価可能になるまで {:?} 保留します。",
					label,
					remaining
				);
				tokio::select! {
					changed = pending_rx.changed() => {
						if changed.is_err() {
							break;
						}
						if let Some(newer) = pending_rx.borrow_and_update().clone() {
							latest = newer;
							continue;
						}
					}
					_ = tokio::time::sleep(remaining) => {}
				}
				break;
			}

			if self.is_paused() {
				log::trace!("《AI[{}]》 paused のため保留観測 {:?} を破棄します。", label, latest);
				continue;
			}
			if let Err(e) = self.react(latest).await {
				log::error!("《AI[{}]》 応答中にエラー: {:?}", label, e);
			}
		}
		log::info!("《AI[{}]》 observation worker を終了します。", label);
	}

	async fn coalesce_observations(&self, pending_rx: &mut watch::Receiver<Option<Observation>>, latest: Observation) -> Observation {
		let conf = self.persona.read().await.observation_buffer.clone().unwrap_or_default();
		let settle = std::time::Duration::from_millis(conf.settle_ms);
		let max_wait = std::time::Duration::from_millis(conf.max_wait_ms.max(conf.settle_ms));
		coalesce_pending_observations(pending_rx, latest, settle, max_wait).await
	}

	/// Twitch配信状態と会話の静穏時間から、通常の人間発話とは独立した自発参加候補を作る。
	pub async fn run_initiative(self, interval_secs: u64) {
		let label = self.persona_label();
		let mut tick = tokio::time::interval(Duration::from_secs(interval_secs.max(1)));
		tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
		tick.tick().await;
		loop {
			tick.tick().await;
			if self.is_paused() {
				continue;
			}
			let Some(conf) = self.persona.read().await.initiative.clone().filter(|value| value.enabled) else {
				continue;
			};
			let now_ms = unix_ms();
			let (registry, eventsub_last_message) = {
				let state = self.state.read().await;
				(
					state.twitch_stream_state.clone(),
					state.twitch_eventsub_last_message_unix_ms.clone(),
				)
			};
			let stream = registry.snapshot(crate::stream_state::DEFAULT_FRESHNESS_SECS).await;
			let online_fresh = initiative::stream_online_available(
				stream.online.as_ref().map(|online| online.value),
				stream.online.as_ref().is_some_and(|online| online.fresh),
				eventsub_last_message.load(Ordering::Acquire),
				now_ms,
			);
			let last_activity_ms = {
				let channel_data = self.channel_data.read().await;
				channel_data
					.iter()
					.rev()
					.find(|datum| {
						datum.has_flag(ChannelDatum::FLAG_IS_FINAL)
							&& (datum.channel == self.channel_utterance || self.observe.is_trigger(&datum.channel))
							&& !datum.source.as_ref().is_some_and(|source| source.kind == INITIATIVE_SOURCE_KIND)
					})
					.and_then(|datum| u64::try_from(datum.get_datetime().as_millisecond()).ok())
			};

			let (outcome, idle_secs, attempts_last_hour, speeches_last_hour) = {
				let mut runtime = self.initiative_runtime.lock().await;
				if online_fresh {
					if runtime.online_since_ms.is_none() {
						runtime.online_since_ms = Some(now_ms);
					}
				} else {
					runtime.online_since_ms = None;
				}
				let window_start = now_ms.saturating_sub(60 * 60 * 1_000);
				runtime.attempt_times_ms.retain(|at| *at >= window_start);
				runtime.speech_times_ms.retain(|at| *at >= window_start);
				let online_since_ms = runtime.online_since_ms;
				let idle_since_ms = last_activity_ms
					.filter(|at| online_since_ms.is_some_and(|online| *at > online))
					.or(online_since_ms);
				let idle_secs = idle_since_ms.map(|at| now_ms.saturating_sub(at) / 1_000).unwrap_or(0);
				let attempts: Vec<u64> = runtime.attempt_times_ms.iter().copied().collect();
				let speeches: Vec<u64> = runtime.speech_times_ms.iter().copied().collect();
				let outcome = initiative::evaluate(
					&conf,
					InitiativePolicyInput {
						now_ms,
						stream_online_fresh: online_fresh,
						online_since_ms,
						last_activity_ms,
						attempt_times_ms: &attempts,
						speech_times_ms: &speeches,
					},
				);
				(outcome, idle_secs, attempts.len(), speeches.len())
			};
			let InitiativePolicyOutcome::Candidate(kind) = outcome else {
				self.emit_initiative_decision(outcome, idle_secs, attempts_last_hour, speeches_last_hour).await;
				continue;
			};
			// A timer must not generate a second answer while an observation is
			// already being answered. Reserve without waiting or consuming the
			// initiative attempt budget; ordinary observations keep their cadence.
			let Some(initiative_permit) = try_reserve_initiative(&self.in_flight, self.in_flight_capacity) else {
				self.emit_initiative_decision(InitiativePolicyOutcome::SuppressedActiveConversation, idle_secs, attempts_last_hour, speeches_last_hour).await;
				continue;
			};
			self.emit_initiative_decision(outcome, idle_secs, attempts_last_hour, speeches_last_hour).await;

			let kind_label = match kind {
				InitiativeCandidateKind::StreamStart => "配信開始後の最初の参加機会",
				InitiativeCandidateKind::OngoingConversation => "継続中の配信への定期参加機会",
				InitiativeCandidateKind::LongSilence => "長い沈黙後の参加機会",
			};
			let title = stream.title.as_ref().map(|value| value.value.as_str()).unwrap_or("");
			let category = stream.category_name.as_ref().map(|value| value.value.as_str()).unwrap_or("");
			let channel = format!("{}{}", crate::flowgraph::nodes::ai::FLOWGRAPH_AI_INPUT_PREFIX, label);
			let datum = ChannelDatum::new(
				channel.clone(),
				format!("{kind_label}。静穏時間は{idle_secs}秒。Twitch宣言カテゴリー={category:?}、タイトル={title:?}。発話する価値がなければ沈黙すること。"),
			)
			.with_flag(ChannelDatum::FLAG_IS_FINAL)
			.with_source(crate::state::DataSource::new(INITIATIVE_SOURCE_KIND).with_subtype(match kind {
				InitiativeCandidateKind::StreamStart => "stream_start",
				InitiativeCandidateKind::OngoingConversation => "ongoing_conversation",
				InitiativeCandidateKind::LongSilence => "long_silence",
			}));
			let datum_id = datum.get_id();
			self.state.read().await.push_channel_datum_ai_internal(datum).await;
			let obs = Observation {
				datum_id,
				channel: channel.clone(),
			};
			if let Err(error) = self.react_with_origin(obs, ReactionOrigin::Initiative(kind), Some(initiative_permit)).await {
				log::error!("《AI[{}]》 Initiative応答中にエラー: {error:#}", label);
				let (attempts, speeches) = {
					let runtime = self.initiative_runtime.lock().await;
					(runtime.attempt_times_ms.len(), runtime.speech_times_ms.len())
				};
				self.send_initiative_event(crate::control_events::AiInitiativeOutcome::Failed, idle_secs, attempts, speeches)
					.await;
				continue;
			}
			let marker = format!("ai[{}]({}:{})", label, channel, datum_id);
			let spoke = self.channel_data.read().await.iter().any(|datum| datum.flags.contains(&marker));
			if spoke {
				self.initiative_runtime.lock().await.speech_times_ms.push_back(unix_ms());
			}
			let (attempts, speeches) = {
				let runtime = self.initiative_runtime.lock().await;
				(runtime.attempt_times_ms.len(), runtime.speech_times_ms.len())
			};
			// This scheduler is sequential; no attempt timestamps are pruned while
			// react_with_origin runs. Only its first model dispatch appends one.
			let dispatched = attempts > attempts_last_hour;
			self.emit_initiative_completion(spoke, dispatched, idle_secs, attempts, speeches).await;
		}
	}

	async fn emit_initiative_decision(
		&self,
		outcome: InitiativePolicyOutcome,
		idle_secs: u64,
		attempts_last_hour: usize,
		speeches_last_hour: usize,
	) {
		use crate::control_events::AiInitiativeOutcome as EventOutcome;
		let outcome = match outcome {
			InitiativePolicyOutcome::SuppressedStreamUnavailable => EventOutcome::SuppressedStreamUnavailable,
			InitiativePolicyOutcome::SuppressedStartupGrace => EventOutcome::SuppressedStartupGrace,
			InitiativePolicyOutcome::SuppressedActiveConversation => EventOutcome::SuppressedActiveConversation,
			InitiativePolicyOutcome::SuppressedMinInterval => EventOutcome::SuppressedMinInterval,
			InitiativePolicyOutcome::SuppressedAttemptBudget => EventOutcome::SuppressedAttemptBudget,
			InitiativePolicyOutcome::SuppressedSpeechBudget => EventOutcome::SuppressedSpeechBudget,
			InitiativePolicyOutcome::Candidate(InitiativeCandidateKind::StreamStart) => EventOutcome::CandidateStreamStart,
			InitiativePolicyOutcome::Candidate(InitiativeCandidateKind::OngoingConversation) => EventOutcome::CandidateOngoingConversation,
			InitiativePolicyOutcome::Candidate(InitiativeCandidateKind::LongSilence) => EventOutcome::CandidateLongSilence,
		};
		self.send_initiative_event(outcome, idle_secs, attempts_last_hour, speeches_last_hour)
			.await;
	}

	async fn emit_initiative_completion(&self, spoke: bool, dispatched: bool, idle_secs: u64, attempts: usize, speeches: usize) {
		let outcome = if spoke {
			crate::control_events::AiInitiativeOutcome::Spoke
		} else if !dispatched {
			crate::control_events::AiInitiativeOutcome::SkippedBeforeRequest
		} else {
			crate::control_events::AiInitiativeOutcome::CompletedWithoutSpeech
		};
		self.send_initiative_event(outcome, idle_secs, attempts, speeches).await;
	}

	async fn clear_reaction_failure(&self, stage: &str) {
        let health = self.state.read().await.runtime_health.clone();
        health.clear(&format!("ai_reaction:{}:{stage}", self.persona_label())).await;
    }

	async fn emit_reaction_failure(&self, datum_id: u64, stage: &str, error: &anyhow::Error) {
		let (category, http_status) = super::failure_diagnostic::classify(error);
		let health = self.state.read().await.runtime_health.clone();
		health.report(super::failure_diagnostic::issue(&self.persona_label(), stage, error)).await;
		let _ = self.state.read().await.control_event_tx.send(crate::control_events::ControlEvent::AiReactionFailure {
			persona_id: self.persona_label(),
			observation_datum_id: datum_id,
			stage: stage.into(),
			category: category.into(),
			http_status,
			observed_at: jiff::Timestamp::now().to_string(),
		});
	}

	async fn send_initiative_event(
		&self,
		outcome: crate::control_events::AiInitiativeOutcome,
		idle_secs: u64,
		attempts_last_hour: usize,
		speeches_last_hour: usize,
	) {
		let tx = self.state.read().await.control_event_tx.clone();
		let _ = tx.send(crate::control_events::ControlEvent::AiInitiativeDecision {
			persona_id: self.persona_label(),
			outcome,
			idle_secs,
			attempts_last_hour,
			speeches_last_hour,
			observed_at: jiff::Timestamp::now().to_string(),
		});
	}

	/// Phase III: heartbeat タスク本体。
	///
	/// `interval_secs` ごとに直近のトリガー発話を確認する観測欠落の回収監視。
	/// 通常event loopが処理済みのdatumは`react()`入口のclaimで除外され、同じ内容を時間経過だけで再評価しない。
	/// 自発発話は将来のInitiative Schedulerで、明示的な候補と新規性を持つ別経路として扱う。
	pub async fn run_heartbeat(self, interval_secs: u64) {
		let label = self.persona_label();
		let mut tick = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
		tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
		tick.tick().await; // 1 回目の即発を捨てる（起動直後の不要な評価を避ける）
		loop {
			tick.tick().await;

			if self.is_paused() {
				log::trace!("《AI[{}]》 heartbeat: paused のためティックをスキップします。", label);
				continue;
			}

			// Phase VI-α-4: hot-reload で enabled が切り替わりうるため、毎ティック現在値を確認する。
			// 当初は spawn 時に enabled=true だったが reload で false になった、のケースを吸収。
			let hb_enabled_now = self.persona.read().await.heartbeat.as_ref().map(|h| h.enabled).unwrap_or(false);
			if !hb_enabled_now {
				log::trace!(
					"《AI[{}]》 heartbeat: enabled=false のためティックをスキップします（reload 反映）。",
					label
				);
				continue;
			}

			let latest = {
				let channel_data = self.channel_data.read().await;
				channel_data
					.iter()
					.rev()
					.find(|cd| cd.has_flag(ChannelDatum::FLAG_IS_FINAL) && self.observe.is_trigger(&cd.channel))
					.map(|cd| (cd.get_id(), cd.channel.clone()))
			};
			let Some((datum_id, channel)) = latest else {
				log::trace!("《AI[{}]》 heartbeat: 評価対象のトリガー発話が見つかりません。スキップ。", label);
				continue;
			};
			if self.last_claimed_observation_id.load(Ordering::SeqCst) >= datum_id {
				log::trace!("《AI[{}]》 heartbeat: datum {} は処理済みです。", label, datum_id);
				continue;
			}
			let min_interval_remaining = {
				let decision = self.decision.read().await;
				let last_activated = *self.last_activated.lock().await;
				decision.remaining_min_interval(last_activated)
			};
			if min_interval_remaining.is_some() {
				log::trace!(
					"《AI[{}]》 heartbeat: observation workerの保留を優先し、min_interval中の回収をスキップします。",
					label
				);
				continue;
			}

			let obs = Observation { datum_id, channel };
			log::trace!("《AI[{}]》 heartbeat: Observation {:?} を再評価します。", label, obs);
			let me = self.clone();
			tokio::spawn(async move {
				if let Err(e) = me.react(obs).await {
					log::error!("《AI[{}]》 heartbeat 応答中にエラー: {:?}", me.persona_label(), e);
				}
			});
		}
	}

	/// 1 回の観測に対する応答生成本体。旧 `OpenAiChat::process` の中身に相当。
	async fn react(&self, obs: Observation) -> Result<()> {
		self.react_with_origin(obs, ReactionOrigin::Observation, None).await
	}

	async fn react_with_origin(&self, obs: Observation, origin: ReactionOrigin, reserved_permit: Option<tokio::sync::OwnedSemaphorePermit>) -> Result<()> {
		if !claim_reaction_observation(&self.last_claimed_observation_id, obs.datum_id, origin) {
			log::trace!(
				"《AI[{}]》 Observation {:?} は処理済みまたは新しい観測に置き換わったためスキップします。",
				self.persona_label(),
				obs
			);
			return Ok(());
		}

		// Phase VI-α-4: persona / request_template / decision は hot-reload で差し替わるため、
		// react() の開始時にまとめて read lock を取ってスナップショットを作る。
		let persona = self.persona.read().await.clone();
		let mut observe = (*self.observe).clone();
		if obs.channel.starts_with(crate::flowgraph::nodes::ai::FLOWGRAPH_AI_INPUT_PREFIX) {
			observe.triggers.push(obs.channel.clone());
		}

		let memory_capacity = persona.memory_capacity.unwrap_or(DEFAULT_MEMORY_CAPACITY);
		let custom_instructions = persona.custom_instructions.clone();
		let system_instructions_extra = persona.system_instructions_extra.clone();
		let memory_summary_path = persona.memory_summary_path.clone();
		let openai_few_shot = persona.openai_few_shot.clone();
		let persona_anchor = persona.persona_anchor.clone();
		let assistant_strip_substrings = persona.assistant_strip_substrings.clone();
		let memory_budget_approx_tokens = persona.memory_budget_approx_tokens;
		let memory_budget_chars_per_approx_token = persona.memory_budget_chars_per_approx_token.unwrap_or(4).max(1);
		let memory_overflow_summary_enabled = persona.memory_overflow_summary_enabled.unwrap_or(false);
		let memory_overflow_summary_model = persona.memory_overflow_summary_model.clone();
		let memory_overflow_summary_max_output_tokens = resolve_overflow_summary_max_output_tokens(&persona);
		let memory_overflow_summary_max_input_chars = persona.memory_overflow_summary_max_input_chars.unwrap_or(16_000);
		let memory_overflow_summary_min_chars = persona
			.memory_overflow_summary_min_chars
			.unwrap_or(DEFAULT_OVERFLOW_SUMMARY_MIN_CHARS);
		let memory_overflow_summary_cooldown_secs = persona.memory_overflow_summary_cooldown_secs.filter(|&s| s > 0);
		let routine_model = persona.model.clone();
		let routine_max_output_tokens = resolve_openai_max_output_tokens(&persona);
		let remove_chars = persona
			.remove_chars
			.as_ref()
			.cloned()
			.unwrap_or_else(|| DEFAULT_REMOVE_CHARS.to_string());
		let fine_tuning = persona.fine_tuning.clone();
		let memory_summary = persona.memory_summary.clone();
		let memory_max_chars = persona.memory_max_chars;
		let assistant_max_chars = persona.assistant_max_chars;
		let assistant_max_sentences = persona.assistant_max_sentences;
		let openai_stream = persona.openai_stream.unwrap_or(DEFAULT_OPENAI_STREAM);
		let turn_management_enabled = persona.turn_management.as_ref().is_some_and(|conf| conf.enabled);
		let visual_history_enabled = turn_management_enabled && persona.turn_management.as_ref().is_some_and(|conf| conf.visual_history_enabled);
		let past_screen_enabled = visual_history_enabled && persona.turn_management.as_ref().is_some_and(|conf| conf.past_screen_image_enabled);
		let avatar_sync_with_speech = persona.turn_management.as_ref().is_some_and(|conf| conf.avatar_sync_with_speech);
		let continuation_gap = persona
			.turn_management
			.as_ref()
			.and_then(|conf| conf.continuation_gap_ms)
			.filter(|ms| *ms > 0)
			.map(std::time::Duration::from_millis);
		let openai_tools_json_path = persona.openai_tools_json_path.clone();
		let openai_tool_choice = persona.openai_tool_choice.clone();
		let openai_parallel_tool_calls = persona.openai_parallel_tool_calls;
		let openai_tool_policy = persona.openai_tool_policy.clone();
		let reasoning_encrypted_passthrough = resolve_reasoning_encrypted_passthrough(&persona);
		let respect_speech_floor = persona.respect_speech_floor.clone();

		let channel_utterance = self.channel_utterance.clone();
		let state = self.state.clone();
		let channel_data = self.channel_data.clone();
		let client = self.client.clone();
		// χ-5: Arc<CreateResponseRequest> として取得し、後続で `(*request_template).clone()` で深い複製を作る。
		let request_template: Arc<CreateResponseRequest> = self.request_template.read().await.clone();
		let last_activated = self.last_activated.clone();
		let episode_memory = self.episode_memory.clone();
		let in_flight = self.in_flight.clone();
		let overflow_summary_last_at = self.overflow_summary_last_at.clone();
		let decision: Arc<DecisionSpec> = self.decision.read().await.clone();

		let persona_label = self.persona_label();
		let marker_flag = format!("ai[{}]({}:{})", persona_label, obs.channel, obs.datum_id);

		let priority_observation = {
			let channel_data = channel_data.read().await;
			channel_data
				.iter()
				.find(|datum| datum.get_id() == obs.datum_id)
				.is_some_and(|datum| {
					decision.is_priority_observation(&DecisionInput {
						channel: &obs.channel,
						latest_trigger_content: datum.content.trim(),
					})
				})
		};

		let speech_floor_wait_started = Instant::now();
		crate::state::wait_respect_speech_floor_key(&state, respect_speech_floor.as_deref()).await;
		let speech_floor_wait_ms = if respect_speech_floor.as_deref().is_some_and(|key| !key.trim().is_empty()) {
			speech_floor_wait_started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
		} else {
			0
		};
		if respect_speech_floor.as_deref().is_some_and(|key| !key.trim().is_empty()) {
			let superseded = {
				let channel_data = channel_data.read().await;
				if matches!(origin, ReactionOrigin::Initiative(_)) {
					initiative_is_superseded(&channel_data, &obs, &observe)
				} else {
					observation_is_superseded(&channel_data, &obs)
				}
			};
			if should_discard_superseded_observation(superseded, priority_observation) {
				self.emit_activation_decision(
					&obs,
					crate::control_events::AiActivationOutcome::DiscardedSuperseded,
					None,
					None,
					None,
					speech_floor_wait_ms,
				)
				.await;
				log::debug!(
					"《AI[{}]》 発話権の解放待ち中に新しい確定発話が届いたため、古い観測 {:?} を破棄します。",
					persona_label,
					obs
				);
				return Ok(());
			} else if superseded {
				log::debug!(
					"《AI[{}]》 発話権待ち中に新しい確定発話が届きましたが、高優先度の観測 {:?} を保持します。",
					persona_label,
					obs
				);
			}
		}

		let _in_flight_permit = match reserved_permit {
			Some(permit) => permit,
			None => match in_flight.acquire_owned().await {
				Ok(permit) => permit,
				Err(_) => {
					log::error!("《AI[{}]》 同時実行セマフォが閉じられました。処理を中断します。", persona_label);
					return Ok(());
				}
			},
		};

		// メモリ窓収集
		let (mut reversed_sources, dropped_for_overflow) = {
			let channel_data = channel_data.read().await;
			match context::collect_memory_window(&channel_data, obs.datum_id, memory_capacity, &observe) {
				Ok(mut v) => {
					let dropped = if memory_budget_approx_tokens.is_some() {
						context::shrink_memory_window_by_approx_tokens(
							&mut v,
							memory_budget_approx_tokens,
							memory_budget_chars_per_approx_token,
						)
					} else {
						context::shrink_memory_window_by_chars(&mut v, memory_max_chars)
					};
					(v, dropped)
				}
				Err(context::MemoryWindowError::TriggerNotFound) => {
					log::error!(
      "《AI[{}]》 処理対象の入力は既にありませんでした。必要に応じて設定の state_data_capacity を増やすと解決するかもしれません。",
      persona_label
     );
					return Ok(());
				}
				Err(context::MemoryWindowError::TriggerNotFinal) => {
					log::trace!("《AI[{}]》 未確定の入力なので、処理をスキップします。", persona_label);
					return Ok(());
				}
			}
		};

		let overflow_summary_text: Option<String> = if memory_overflow_summary_enabled && !dropped_for_overflow.is_empty() {
			let raw = context::format_dropped_turns_for_overflow_summary(&dropped_for_overflow);
			let truncated = context::truncate_overflow_summary_input(&raw, memory_overflow_summary_max_input_chars);
			let input_chars = truncated.trim().chars().count();
			if input_chars < memory_overflow_summary_min_chars {
				log::debug!(
					"《AI[{}]》 メモリ窓オーバーフロー要約: 入力が短いためスキップ（{} < {} 文字）。",
					persona_label,
					input_chars,
					memory_overflow_summary_min_chars
				);
				None
			} else if let Some(cool_secs) = memory_overflow_summary_cooldown_secs {
				let guard = overflow_summary_last_at.lock().await;
				let skip_for_cooldown = match *guard {
					Some(prev) => prev.elapsed().as_secs() < cool_secs,
					None => false,
				};
				if skip_for_cooldown {
					log::debug!(
						"《AI[{}]》 メモリ窓オーバーフロー要約: クールダウン中のためスキップ（要約 API の最短間隔 {}s）。",
						persona_label,
						cool_secs
					);
					None
				} else {
					drop(guard);
					run_overflow_summary(
						&client,
						&memory_overflow_summary_model,
						routine_model.as_deref(),
						memory_overflow_summary_max_output_tokens,
						&truncated,
						dropped_for_overflow.len(),
						&overflow_summary_last_at,
					)
					.await
				}
			} else {
				run_overflow_summary(
					&client,
					&memory_overflow_summary_model,
					routine_model.as_deref(),
					memory_overflow_summary_max_output_tokens,
					&truncated,
					dropped_for_overflow.len(),
					&overflow_summary_last_at,
				)
				.await
			}
		} else {
			None
		};

		let Some(latest_user_content) = reversed_sources
			.iter()
			.find(|cd| observe.is_trigger(&cd.channel))
			.map(|v| v.content.clone())
		else {
			log::warn!(
				"《AI[{}]》 メモリ窓にトリガー（{:?}）の発話がありません。応答をスキップします。",
				persona_label,
				persona.observe.triggers
			);
			return Ok(());
		};

		let request_class = reversed_sources
			.iter()
			.find(|datum| datum.get_id() == obs.datum_id)
			.and_then(context::request_class)
			.map(str::to_string);
		let selected_model = model_policy::select_model(
			&persona,
			&obs.channel,
			&latest_user_content,
			request_class.as_deref(),
			routine_max_output_tokens,
		)?;
		let model_for_runtime = Some(selected_model.model.to_string());
		if selected_model.escalated {
			log::info!(
				"《AI[{}]》 model escalation: channel={:?} model={}",
				persona_label,
				obs.channel,
				selected_model.model
			);
		}

		log::trace!("《AI[{}]》 reversed_sources = {:?}", persona_label, reversed_sources);

		// Phase II: Decision Engine による応答判定。legacy の force/ignore/min_interval も内部で同じ数式に畳んでいる。
		let newest_trimmed = reversed_sources.iter().next().map(|cd| cd.content.trim()).unwrap_or("");
		let last_activated_snapshot = *last_activated.lock().await;
		let decision_result = decision.evaluate(
			&DecisionInput {
				channel: &obs.channel,
				latest_trigger_content: newest_trimmed,
			},
			last_activated_snapshot,
			uniform_jitter,
		);
		if matches!(origin, ReactionOrigin::Initiative(_)) {
			log::debug!("《AI[{}]》 Initiative policy通過済み候補をモデル評価へ渡します。", persona_label);
		} else {
			match decision_result {
				Decision::Activate { score, threshold } => {
					self.emit_activation_decision(
						&obs,
						crate::control_events::AiActivationOutcome::Activated,
						Some(score),
						Some(threshold),
						None,
						speech_floor_wait_ms,
					)
					.await;
					log::debug!(
						"《AI[{}]》 Decision: Activate (score={:.2} >= threshold={:.2}) target={:?}",
						persona_label,
						score,
						threshold,
						newest_trimmed
					);
					if decision.min_interval_in_secs().is_some() {
						*last_activated.lock().await = SystemTime::now();
					}
				}
				Decision::SkipByScore { score, threshold } => {
					self.emit_activation_decision(
						&obs,
						crate::control_events::AiActivationOutcome::SkippedByScore,
						Some(score),
						Some(threshold),
						None,
						speech_floor_wait_ms,
					)
					.await;
					log::trace!(
						"《AI[{}]》 Decision: SkipByScore (score={:.2} < threshold={:.2}) target={:?}",
						persona_label,
						score,
						threshold,
						newest_trimmed
					);
					return Ok(());
				}
				Decision::SkipByMinInterval { remaining_secs } => {
					self.emit_activation_decision(
						&obs,
						crate::control_events::AiActivationOutcome::SkippedByMinInterval,
						None,
						None,
						Some(remaining_secs),
						speech_floor_wait_ms,
					)
					.await;
					log::trace!(
						"《AI[{}]》 Decision: SkipByMinInterval（残り {}s）。",
						persona_label,
						remaining_secs
					);
					return Ok(());
				}
			}
		}

		let memory_summary_combined: Option<String> = {
			let inline = memory_summary.clone().unwrap_or_default();
			let mut combined = inline;
			if let Some(ref path) = memory_summary_path {
				match tokio::fs::read_to_string(path).await {
					Ok(file_txt) => {
						let ft = file_txt.trim();
						if !ft.is_empty() {
							if combined.trim().is_empty() {
								combined = file_txt;
							} else {
								combined = format!("{}\n\n---\n\n{}", ft, combined.trim());
							}
						}
					}
					Err(e) => log::warn!(
						"《AI[{}]》 memory_summary_path を読めませんでした: {:?} ({})",
						persona_label,
						path,
						e
					),
				}
			}
			if let Some(ov) = overflow_summary_text {
				let block = format!("【メモリ窓から落ちた発話の要約】\n{}", ov.trim());
				combined = if combined.trim().is_empty() {
					block
				} else {
					format!("{}\n\n---\n\n{}", block, combined.trim())
				};
			}
			let t = combined.trim();
			if t.is_empty() {
				None
			} else {
				Some(combined)
			}
		};

		// χ-5: Responses API 用の request を template から深くコピー。
		let mut request = (*request_template).clone();
		request.model = selected_model.model.to_string();
		request.reasoning = None;
		model_policy::apply_model_responses_options(
			&mut request,
			selected_model.model,
			selected_model.max_output_tokens,
			selected_model.reasoning_effort,
		);
		let explicit_prompt_cache = model_policy::supports_explicit_prompt_cache(model_for_runtime.as_deref());
		request.prompt_cache_options = explicit_prompt_cache.then_some(PromptCacheOptions {
			mode: PromptCacheMode::Explicit,
			ttl: Some(PromptCacheTtl::ThirtyMinutes),
		});
		request.input = context::assemble_openai_responses_static_input(
			model_for_runtime.as_deref(),
			custom_instructions.as_deref(),
			system_instructions_extra.as_deref(),
			&openai_few_shot,
			persona_anchor.as_deref(),
			turn_management_enabled.then_some(TURN_MANAGER_INSTRUCTIONS),
			explicit_prompt_cache,
		);
		if turn_management_enabled {
			if let Some(episode_context) = episode_memory.lock().await.render_context(SystemTime::now()) {
				request.input.push(InputItem::message("developer", episode_context));
			}
		}
		request.input.push(context::current_time_input_item());
		if let Some(max) = assistant_max_sentences.filter(|max| *max > 0) {
			request.input.push(InputItem::message(
				"developer",
				format!(
					"【返答本文の上限】音声・字幕へ出す返答本文（Turn Managerではspeak）は最大{max}文。句点と感嘆符・疑問符も文の区切りになる。相槌だけで文数を使い切らず、会話への反応や回答の要点をこの上限内に収める。後から切り落とされる文へ肝心の内容を置かない。"
				),
			));
		}
		let perception = state.read().await.stream_perception.clone();
		let mut refresh = perception.refresh_for_reply(&state, &observe).await;
		if refresh.attempted && refresh.failed {
			request.input.push(InputItem::message("developer",
				"今回の応答直前に更新できなかったOBS sourceがある。そのsourceの以前の画像・観測は今回の入力から除外した。取得できていない画面の現在の状態を断定しない。他に添付された観測があれば、そのsourceと取得時刻、会話の根拠を区別する。"));
		}
		{
			let data = channel_data.read().await;
			context::refresh_state_observations(&mut reversed_sources, &data, &observe);
		}
		let (assembly_mode, assembly_mode_generation) = {
			let s = state.read().await;
			let mode = s.runtime_mode_id.read().unwrap_or_else(|error| error.into_inner());
			(mode.clone(), s.runtime_mode_generation.load(std::sync::atomic::Ordering::SeqCst))
		};
		perception.restrict_reply_to_mode(&mut refresh, assembly_mode.as_deref());
		refresh.exclude_unrefreshed(&mut reversed_sources);
		let stream_registry = state.read().await.twitch_stream_state.clone();
		context::refresh_twitch_declaration(&mut reversed_sources, &stream_registry, &observe).await;
		let context_collected_at = jiff::Timestamp::now().to_string();
		let (dynamic_input, mut context_observations, mut input_datums) =
			context::assemble_dynamic_input_with_observations(memory_summary_combined.as_deref(), &reversed_sources, &observe);

        let mut visual_history_json = None;
        let mut visual_request_scope = None;
        let mut past_input_datum = None;
        if visual_history_enabled {
            let mut scope: Vec<String> = reversed_sources.iter().filter_map(|d| {
                match d.source.as_ref()?.kind.as_str() {
                    "obs.scene_source" => Some(format!("{}:{:?}:{:?}", d.channel, d.meta_get("scene_name"), d.meta_get("source_name"))),
                    "twitch.stream_state" => Some(format!("twitch:{:?}:{:?}:{:?}", d.meta_get("state").and_then(|v| v.pointer("/category_id/value")), d.meta_get("state").and_then(|v| v.pointer("/online/value")), d.meta_get("state").and_then(|v| v.pointer("/started_at/value")))),
                    _ => None,
                }
            }).collect();
            // Same source and same mode name after a pause are a new visual lifetime.
            // This also expires the optional previous image across mode round trips.
            scope.push(format!("mode-generation:{assembly_mode_generation}"));
            scope.sort();
            if past_screen_enabled {
                use super::openai_responses::types::input::{InputContent, InputContentPart};
                let images: Vec<_> = dynamic_input.iter().filter_map(|item| match item {
                    InputItem::Message { content: InputContent::Parts(parts), .. } => Some(parts), _ => None,
                }).flatten().filter(|part| matches!(part, InputContentPart::InputImage { .. })).collect();
                let current_sources: Vec<_> = context_observations.iter().filter(|source| source.source_kind == "obs.scene_source" && source.input_image_count == 1).collect();
                let source_is_named = current_sources.first().is_some_and(|source| reversed_sources.iter().any(|datum| {
                    datum.get_id() == source.datum_id && ["scene_name", "source_name"].iter().all(|key| datum.meta_get(key).and_then(serde_json::Value::as_str).is_some_and(|value| !value.is_empty()))
                }));
                let current = if images.len() == 1 && current_sources.len() == 1 && source_is_named {
                    Some(super::past_screen::Frame { observation: current_sources[0].clone(), image: images[0].clone() })
                } else { None };
                let prompt = matches!(origin, ReactionOrigin::Observation).then(|| reversed_sources.iter().find(|datum| datum.get_id() == obs.datum_id && !datum.source.as_ref().is_some_and(|source| source.kind == "ai")).map(|datum| datum.content.as_str())).flatten();
                if let Some(previous) = self.past_screen.lock().await.select(scope.join("|"), current, prompt, jiff::Timestamp::now()) {
                    past_input_datum = Some(crate::control_events::AiContextDatum { datum_id: previous.observation.datum_id, input_index: request.input.len(), role: "user".into(), source_kind: Some(previous.observation.source_kind.clone()) });
                    request.input.push(previous.input());
                    context_observations.push(previous.observation);
                }
            } else { *self.past_screen.lock().await = Default::default(); }
            request.input.push(InputItem::message("developer", super::visual_history::INSTRUCTIONS));
            let mut visual_history = self.visual_history.lock().await;
            let prepared_history = visual_history.prepare(scope.join("|"), &context_observations, jiff::Timestamp::now());
            visual_request_scope = Some(visual_history.request_scope());
            if let Some(history) = prepared_history {
                request.input.push(InputItem::message("user", format!("[過去画像の読取メモ・非信頼データ・現在状態ではない] {history}")));
                visual_history_json = Some(history);
            }
        } else {
            *self.visual_history.lock().await = Default::default();
            *self.past_screen.lock().await = Default::default();
        }

		for datum in &mut input_datums { datum.input_index += request.input.len(); }
        if let Some(datum) = past_input_datum { input_datums.push(datum); }
		request.input.extend(dynamic_input);
		if let ReactionOrigin::Initiative(kind) = origin {
			let instructions = if kind == InitiativeCandidateKind::OngoingConversation {
				ONGOING_CONVERSATION_INITIATIVE_INSTRUCTIONS
			} else {
				INITIATIVE_INSTRUCTIONS
			};
			request.input.push(InputItem::message("developer", instructions));
		}
		if turn_management_enabled {
			request.text = Some(turn_manager_text_config_with_history(visual_history_enabled));
		}

		// ψ-α: gpt-5 系 + passthrough 有効時のみ `include: ["reasoning.encrypted_content"]` を付与。
		// 非 gpt-5 モデルでは no-op（API 側で意味を持たない keyword 指定を避ける）。
		let include_before = request.include.as_ref().map(|v| v.len()).unwrap_or(0);
		apply_reasoning_passthrough_include(&mut request, model_for_runtime.as_deref(), reasoning_encrypted_passthrough);
		if request.include.as_ref().map(|v| v.len()).unwrap_or(0) > include_before {
			log::debug!(
				"《AI[{}]》 ψ-α: include=reasoning.encrypted_content を付与（model={:?}, passthrough={}）。",
				persona_label,
				model_for_runtime,
				reasoning_encrypted_passthrough
			);
		}

		// χ-5: tools は Responses API の `Vec<Tool>` を直接 request.tools に入れる。
		// hosted tools（web_search / file_search / code_interpreter）も passthrough。
		let mut tools_declared_local: std::collections::HashSet<String> = std::collections::HashSet::new();
		let mut has_tools = false;
		if let Some(ref p) = openai_tools_json_path {
			match tokio::fs::read_to_string(p).await {
				Ok(s) => match tools::parse_tools_json(&s) {
					Ok(mut v) if !v.is_empty() => {
						tools::constrain_effect_channels(&mut v, openai_tool_policy.as_ref().and_then(|policy| policy.effect_allowed_channels.as_deref()), persona.channel_effect.as_deref().unwrap_or("effect"))?;
						let require_strict = openai_tool_policy.as_ref().is_some_and(|policy| policy.require_strict);
						if let Err(e) = tools::validate_function_tool_schemas(&v, require_strict) {
							bail!("openai_tools_json schema検証に失敗しました: {e}");
						}
						if turn_management_enabled {
							v.retain(|tool| tool.name() != tools::AVATAR_INTENT_TOOL_NAME);
						}
						if v.is_empty() {
							log::info!(
								"《AI[{}]》 Turn Manager構造化出力が身体表現を担うため、OpenAI toolsは空になりました。",
								persona_label
							);
						} else {
							tools_declared_local = tools::locally_dispatched_tool_names(&v);
							let declared_count = v.len();
							let hosted_count = v.iter().filter(|t| !t.is_locally_dispatched()).count();
							if let Some(policy) = openai_tool_policy.as_ref() {
								let allowed = allowed_tools_for_request(policy, &obs.channel, request_class.as_deref());
								if let Some(allowed) = allowed {
									let allowed = turn_manager_allowed_tools(allowed, turn_management_enabled);
									if allowed.is_empty() {
										request.tool_choice = Some(ToolChoice::Mode(ToolChoiceMode::None));
										tools_declared_local.clear();
									} else {
										let (choice, local_allowed) = tools::apply_allowed_tools(&v, &allowed)?;
										request.tool_choice = Some(choice);
										tools_declared_local = local_allowed;
									}
								}
							}
							request.tools = Some(v);
							request.tool_choice = match request.tool_choice.take() {
								Some(policy_choice) => Some(policy_choice),
								None => match openai_tool_choice.as_deref() {
									None => None,
									Some(tc) => match tools::parse_tool_choice(tc) {
										Ok(x) => Some(x),
										Err(e) => {
											log::warn!("openai_tool_choice を解釈できません: {} — 未指定にします。", e);
											None
										}
									},
								},
							};
							if let Some(ptc) = openai_parallel_tool_calls {
								request.parallel_tool_calls = Some(ptc);
							}
							has_tools = true;
							log::info!(
								"《AI[{}]》 OpenAI tools を読み込みました: {:?} (declared={}, local_dispatchable={}, hosted={})",
								persona_label,
								p,
								declared_count,
								tools_declared_local.len(),
								hosted_count,
							);
						}
					}
					Ok(_) => log::warn!("《AI[{}]》 openai_tools_json_path の tools が空です: {:?}", persona_label, p),
					Err(e) => log::warn!("《AI[{}]》 openai_tools_json をパースできません: {:?} ({})", persona_label, p, e),
				},
				Err(e) => log::warn!(
					"《AI[{}]》 openai_tools_json_path を読めませんでした: {:?} ({})",
					persona_label,
					p,
					e
				),
			}
		}

		let effective_stream = openai_stream;

		log::debug!(
			"《AI[{}]》 応答をリクエストします（stream={}, tools={}）。",
			persona_label,
			effective_stream,
			has_tools
		);

		// χ-5: Responses API の stream / non-stream を同じ tool-loop で駆動する。
		// streaming 経路でも OutputItem::FunctionCall が返れば次ラウンドの input[] に積んで再 request、
		// ツール無し応答のラウンドで break。
		let tool_ctx_opt: Option<ToolContext> = if has_tools { Some(self.tool_context().await) } else { None };
		let drive_started = Instant::now();
        let prepared_at = jiff::Timestamp::now().to_string();
        let archive = {
            let s = state.read().await;
            let streaming = s.runtime_mode_id.read().ok().is_some_and(|mode| mode.as_deref() == Some("streaming"));
            (streaming && s.diagnostic_images.enabled()).then(|| s.diagnostic_images.clone())
        };
        if let Some(archive) = archive {
            context::archive_selected_images(&archive, &reversed_sources, &observe, &request.input, serde_json::json!({
                "persona_id": persona_label, "observation_datum_id": obs.datum_id,
                "model": request.model, "context_collected_at": context_collected_at,
                "prepared_at": prepared_at, "stage": "first_request_prepared"
            }));
        }

		let _ = state
			.read()
			.await
			.control_event_tx
			.send(crate::control_events::ControlEvent::AiRequestContext {
				persona_id: persona_label.clone(),
				observation_datum_id: obs.datum_id,
				model: request.model.clone(),
				request_class: request_class.clone(),
				context_collected_at,
				prepared_at,
				observations: context_observations.clone(),
                visual_history_json,
                input_datums,
			});
		// A candidate can yield to newer human speech before any model request.
		// Only actual model evaluation spends the independent attempt budget.
		if matches!(origin, ReactionOrigin::Initiative(_)) {
			self.initiative_runtime.lock().await.attempt_times_ms.push_back(unix_ms());
		}
		let drive_result = drive_responses_tool_loop(
			&client,
			request,
			effective_stream,
			has_tools,
			&tools_declared_local,
			tool_ctx_opt.as_ref(),
			&state,
			&channel_utterance,
			&persona_label,
			turn_management_enabled,
			should_track_speech_freshness(
				effective_stream,
				turn_management_enabled,
				priority_observation,
				request_class.as_deref(),
				respect_speech_floor.as_deref(),
			)
			.then(|| (obs.channel.clone(), obs.datum_id)),
			continuation_gap,
		)
		.await;

		let (mut content, stream_datum_id, stream_nonempty_delta_chunks, speech_utterance_id) = match drive_result {
			Ok(out) => {
				self.clear_reaction_failure("request_or_tool_loop").await;
				let mut usage = self.usage.write().await;
				usage.completed_reactions += 1;
				usage.api_requests += out.usage.api_requests;
				usage.tool_calls += out.usage.tool_calls;
				usage.input_tokens += out.usage.input_tokens;
				usage.cached_tokens += out.usage.cached_tokens;
				usage.cache_write_tokens += out.usage.cache_write_tokens;
				usage.output_tokens += out.usage.output_tokens;
				usage.reasoning_tokens += out.usage.reasoning_tokens;
				usage.last_model = Some(selected_model.model.to_string());
				usage.last_latency_ms = Some(drive_started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
				usage.last_updated_at = Some(jiff::Timestamp::now().to_string());
				(
					out.content,
					out.stream_datum_id,
					out.stream_nonempty_delta_chunks,
					out.speech_utterance_id,
				)
			}
			Err(e) => {
				self.emit_reaction_failure(obs.datum_id, "request_or_tool_loop", &e).await;
				let mut usage = self.usage.write().await;
				usage.failed_reactions += 1;
				usage.last_model = Some(selected_model.model.to_string());
				usage.last_latency_ms = Some(drive_started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
				usage.last_updated_at = Some(jiff::Timestamp::now().to_string());
				log::error!("《AI[{}]》 OpenAI Responses API 呼び出しに失敗しました: {:?}", persona_label, e);
				eprint_openai_usage_hint_if_needed(&format!("{e:?}"));
				bail!("{e:?}");
			}
		};

		if turn_management_enabled {
			let turn = match parse_turn_manager_output(&content) {
				Ok(turn) => turn,
				Err(error) => {
					self.emit_reaction_failure(obs.datum_id, "turn_output_parse", &error).await;
					return Err(error);
				}
			};
			self.clear_reaction_failure("turn_output_parse").await;
			let playback = state.read().await.speech_scheduler.playback.clone();
			if playback
				.freshness(&speech_utterance_id)
				.is_some_and(|guard| guard.is_cancelled() && !guard.is_committed())
			{
				let outcome = if playback.freshness(&speech_utterance_id).is_some_and(|guard| guard.was_superseded()) {
					crate::control_events::AiActivationOutcome::DiscardedSuperseded
				} else {
					crate::control_events::AiActivationOutcome::DiscardedUndelivered
				};
				if let Some(id) = stream_datum_id {
					state.read().await.update_channel_datum_content_by_id(id, String::new()).await;
				}
				self.emit_activation_decision(
					&obs,
					outcome,
					None,
					None,
					None,
					speech_floor_wait_ms,
				)
				.await;
				return Ok(());
			}
			let output_ready_at = jiff::Timestamp::now().to_string();
			let action = turn.action.as_str().to_string();
			let speak = turn.speak.filter(|text| !text.trim().is_empty());
			let resolved_speech = resolve_turn_speech(turn.action, speak.clone())?;
			if let (Some(guard), Some(text)) = (playback.freshness(&speech_utterance_id), resolved_speech.as_ref()) {
				guard.expect_text(text.clone());
			}
			if resolved_speech.is_some() {
				if let Some(guard) = playback.freshness(&speech_utterance_id) {
					// All spoken ordinary turns, including those without a gesture,
					// stay provisional until delivery or cancellation wins.
					guard.wait_for_delivery(std::time::Duration::from_secs(30)).await;
				}
			}
			let avatar_intent_requested_count = turn.avatar_intent.as_ref().map_or(0, |request| request.intents.len());
			let avatar_intent_target_link_id = turn.avatar_intent.as_ref().and_then(|request| request.target_link_id.clone());
			let avatar_pose_requested = turn.avatar_pose.is_some();
			let avatar_pose_preset = turn.avatar_pose.as_ref().map(|request| request.preset.clone());
			let avatar_pose_target_link_id = turn.avatar_pose.as_ref().map(|request| request.target_link_id.clone());
			let mut avatar_intent_emitted_count = 0;
			let mut avatar_intent_error = None;
			let mut avatar_pose_emitted = false;
			let mut avatar_pose_error = None;
			let sync_result = if playback.freshness(&speech_utterance_id).is_some_and(|guard| guard.is_cancelled()) {
				Err("speech was cancelled before delivery".to_string())
			} else if avatar_sync_with_speech && resolved_speech.is_some() && (avatar_intent_requested_count > 0 || avatar_pose_requested) {
				if effective_stream {
					let playback = state.read().await.speech_scheduler.playback.clone();
					playback.wait(&speech_utterance_id, std::time::Duration::from_secs(30)).await
				} else {
					Err("avatar speech synchronization requires streaming speech output".to_string())
				}
			} else {
				Ok(())
			};
			if playback
				.freshness(&speech_utterance_id)
				.is_some_and(|guard| guard.is_cancelled() && !guard.is_committed())
			{
				if let Some(id) = stream_datum_id {
					state.read().await.update_channel_datum_content_by_id(id, String::new()).await;
				}
				self.emit_activation_decision(
					&obs,
					if playback.freshness(&speech_utterance_id).is_some_and(|guard| guard.was_superseded()) {
						crate::control_events::AiActivationOutcome::DiscardedSuperseded
					} else {
						crate::control_events::AiActivationOutcome::DiscardedUndelivered
					},
					None,
					None,
					None,
					speech_floor_wait_ms,
				)
				.await;
				return Ok(());
			}
			if let Err(error) = sync_result {
				log::warn!("《AI[{persona_label}]》 speech-bound avatar output discarded: {error}");
				if avatar_intent_requested_count > 0 {
					avatar_intent_error = Some(error.clone());
				}
				if avatar_pose_requested {
					avatar_pose_error = Some(error);
				}
			} else if turn.action != TurnAction::Observe {
				if let Some(avatar_intent) = turn.avatar_intent.as_ref() {
					match push_turn_avatar_intent(&state, &persona_label, avatar_intent).await {
						Ok(()) => avatar_intent_emitted_count = avatar_intent.intents.len(),
						Err(error) => {
							let message = format!("{error:#}");
							log::warn!("《AI[{}]》 Turn Manager AvatarIntentを破棄しました: {message}", persona_label);
							avatar_intent_error = Some(message);
						}
					}
				}
				if let Some(avatar_pose) = turn.avatar_pose.as_ref() {
					match push_turn_avatar_pose(&state, &persona_label, avatar_pose).await {
						Ok(()) => avatar_pose_emitted = true,
						Err(error) => {
							let message = format!("{error:#}");
							log::warn!("《AI[{}]》 Turn Manager AvatarPoseを破棄しました: {message}", persona_label);
							avatar_pose_error = Some(message);
						}
					}
				}
			} else if turn.avatar_pose.is_some() {
				avatar_pose_error = Some("observeではavatar_poseを発行できません".to_string());
			}
            commit_visual_notes(&self.visual_history, visual_request_scope.as_ref(), &turn.visual_notes, &context_observations, jiff::Timestamp::now()).await;
            let episode_memory_update = turn.episode_memory.as_deref().filter(|value| !value.trim().is_empty());
			let episode = {
				let now = SystemTime::now();
				let mut memory = episode_memory.lock().await;
				memory.apply(
					&persona_label,
					obs.datum_id,
					&latest_user_content,
					&action,
					speak.as_deref(),
					episode_memory_update,
					now,
				);
				if resolved_speech.is_some() {
					if let Some(guard) = playback.freshness(&speech_utterance_id) {
						memory.bind_delivery(obs.datum_id, guard);
					}
				}
				memory.snapshot(now)
			};
			let _ = state
				.read()
				.await
				.control_event_tx
				.send(crate::control_events::ControlEvent::AiTurnDecision {
					persona_id: persona_label.clone(),
					observation_channel: obs.channel.clone(),
					observation_datum_id: obs.datum_id,
					action: action.clone(),
					speak: speak.clone(),
					episode_id: episode.as_ref().map(|value| value.id.clone()),
					episode_phase: episode.as_ref().map(|value| value.phase.to_string()),
					episode_memory: episode.as_ref().map(|value| value.memory.clone()),
					episode_turn_count: episode.as_ref().map(|value| value.turn_count),
					episode_phase_remaining_secs: episode.as_ref().map(|value| value.phase_remaining_secs),
					avatar_intent_requested_count,
					avatar_intent_parameters: avatar_intent_diagnostic_parameters(turn.avatar_intent.as_ref()),
					avatar_intent_emitted_count,
					avatar_intent_target_link_id,
					avatar_intent_error,
					avatar_pose_requested,
					avatar_pose_emitted,
					avatar_pose_preset,
					avatar_pose_target_link_id,
					avatar_pose_error,
					output_ready_at: Some(output_ready_at),
					speech_utterance_id: resolved_speech.as_ref().map(|_| speech_utterance_id.clone()),
					decided_at: jiff::Timestamp::now().to_string(),
				});
			log::info!(
				"《AI[{}]》 Turn Manager: action={} episode_update={} target={:?}",
				persona_label,
				action,
				episode_memory_update.is_some(),
				latest_user_content
			);
			let Some(resolved_speech) = resolved_speech else {
				return Ok(());
			};
			content = resolved_speech;
		}
		content = apply_openai_output_filters(
			content,
			&remove_chars,
			assistant_max_chars,
			assistant_max_sentences,
			&assistant_strip_substrings,
		);

		if content.trim().is_empty() {
			log::warn!(
				"《AI[{}]》 最終応答が空です（下流へ渡ります）。model={:?} stream={} sse_nonempty_deltas={}",
				persona_label,
				model_for_runtime.as_deref(),
				effective_stream,
				stream_nonempty_delta_chunks,
			);
		}

		let delivery = state.read().await.speech_scheduler.playback.freshness(&speech_utterance_id);
		if let Some(delivery) = delivery {
			// The ordinary speech path has already submitted its pieces to TTS.
			// Keep the history current without blocking the next observation, and
			// dispatch ai_response exactly once, with only the delivered text.
			let id = if let Some(id) = stream_datum_id {
				id
			} else {
				let datum = ai_response_datum(channel_utterance, String::new(), &persona_label);
				let id = datum.get_id();
				state.read().await.push_channel_datum_quiet(datum).await;
				id
			};
			let mut updates = delivery.delivery_updates();
			let prefix = delivery.delivered_text().0;
			state
				.read()
				.await
				.update_delivered_channel_datum(id, prefix.clone(), &marker_flag)
				.await;
			let weak_state = Arc::downgrade(&state);
			tokio::spawn(async move {
				let mut previous = prefix;
				loop {
					updates.borrow_and_update();
					let finished = delivery.delivery_finished();
					let text = delivery.delivered_text().0;
					let Some(state) = weak_state.upgrade() else { break };
					if text != previous {
						if !state
							.read()
							.await
							.update_delivered_channel_datum(id, text.clone(), &marker_flag)
							.await
						{
							break;
						}
						previous = text.clone();
					}
					if finished {
						if !text.is_empty() {
							if let Some(ft) = fine_tuning.as_ref() {
								if let Err(error) = append_fine_tuning_record(ft, &custom_instructions, &latest_user_content, &text).await {
									log::warn!("《AI[{persona_label}]》 delivered fine-tuning record failed: {error:?}");
								}
							}
							state.read().await.finalize_channel_datum_and_dispatch(id, text, &marker_flag).await;
						}
						break;
					}
					drop(state);
					tokio::select! {
						_ = updates.changed() => {},
						_ = delivery.cancelled() => {},
						// Check runtime ownership even if an external graph never
						// produces a terminal speech event. No strong State cycle.
						_ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {},
					}
				}
			});
			return Ok(());
		}

		if let Some(ft) = fine_tuning {
			if let Err(e) = append_fine_tuning_record(&ft, &custom_instructions, &latest_user_content, &content).await {
				log::warn!("《AI[{}]》 fine-tuning ログ追記に失敗しました: {:?}", persona_label, e);
			}
		}

		if let Some(sid) = stream_datum_id {
			state
				.read()
				.await
				.finalize_channel_datum_and_dispatch(sid, content, &marker_flag)
				.await;
		} else {
			let datum = ai_response_datum(channel_utterance, content, &persona_label)
				.with_flag(ChannelDatum::FLAG_IS_FINAL)
				.with_flag(&marker_flag);
			state.read().await.push_channel_datum(datum).await;
		}

		Ok(())
	}
}

async fn validate_tool_policy_at_startup(persona: &AiPersonaConf) -> Result<()> {
	let Some(policy) = persona.openai_tool_policy.as_ref() else {
		return Ok(());
	};
	let path = persona
		.openai_tools_json_path
		.as_ref()
		.ok_or_else(|| anyhow!("openai_tool_policy には openai_tools_json_path が必要です"))?;
	let source = tokio::fs::read_to_string(path)
		.await
		.with_context(|| format!("openai_tools_json_path を読み込めません: {}", path.display()))?;
	let mut declared =
		tools::parse_tools_json(&source).with_context(|| format!("openai_tools_json_path のJSONを解釈できません: {}", path.display()))?;
	tools::constrain_effect_channels(&mut declared, policy.effect_allowed_channels.as_deref(), persona.channel_effect.as_deref().unwrap_or("effect"))?;
	tools::validate_function_tool_schemas(&declared, policy.require_strict)?;
	if let Some(allowed) = policy.default_allowed.as_ref() {
		tools::apply_allowed_tools(&declared, allowed)?;
	}
	for (channel, allowed) in &policy.by_channel {
		tools::apply_allowed_tools(&declared, allowed).with_context(|| format!("openai_tool_policy.by_channel[{channel:?}] が不正です"))?;
	}
	for (request_class, allowed) in &policy.by_request_class {
		tools::apply_allowed_tools(&declared, allowed)
			.with_context(|| format!("openai_tool_policy.by_request_class[{request_class:?}] が不正です"))?;
	}
	Ok(())
}

/// `[[ai.personas]]` を走査し、有効なペルソナの [`AiService`] を立ち上げる。
/// 各ペルソナについて:
/// - 1 本は `broadcast` を受ける通常の event loop（`AiService::run`）
/// - `heartbeat.enabled = true` ならもう 1 本は heartbeat タスク（`AiService::run_heartbeat`）
pub async fn spawn_all(
	conf: &super::config::AiConf,
	state: SharedState,
	tx: broadcast::Sender<Observation>,
	twitch_eventsub: Option<Arc<TwitchEventSubConfig>>,
	twitch_moderator: Option<Arc<TwitchModeratorConfig>>,
	twitch_default_broadcaster_login: Option<String>,
) -> Result<Vec<tokio::task::JoinHandle<()>>> {
	let mut handles = Vec::new();
	let mut runtimes: Vec<AiRuntime> = Vec::new();
	for p in &conf.personas {
		if !p.is_enabled {
			log::info!(
				"《AI[{}]》 は is_enabled = false のためスキップします。",
				p.id.clone().unwrap_or_else(|| "-".to_string())
			);
			continue;
		}
		// Phase VI-α-2: AiService の paused フラグと State 側の AiRuntime を同じ Arc<AtomicBool> で結ぶ。
		// 以降、Control API の /pause・/resume は AiRuntime 側を書き換えるだけで AiService 側にも即座に伝わる。
		let mut runtime = AiRuntime::new(p.id.clone());
		runtime.display_name = p.display_name.clone();
		let paused_flag = runtime.paused.clone();
		let usage = runtime.usage.clone();
		let svc = AiService::new(
			p.clone(),
			state.clone(),
			twitch_eventsub.clone(),
			twitch_moderator.clone(),
			twitch_default_broadcaster_login.clone(),
			paused_flag,
			usage,
		)
		.await?;
		// Phase VI-α-4: hot-reload ハンドルを AiRuntime に登録する。以降 Control API の /reload からアクセス可能。
		runtime.reload_handle = Some(svc.reload_handle());

		// Phase VI-α-4: heartbeat はブロック自体が定義されていれば常に spawn しておき、`enabled` は
		// 毎ティック参照するようにした。これにより Control API から reload で enabled を切り替えられる。
		if let Some(ref hb) = p.heartbeat {
			let interval = hb.interval_secs.max(1);
			if hb.enabled {
				log::info!(
					"《AI[{}]》 heartbeat を有効化しました（{} 秒周期）。",
					svc.persona_label(),
					interval
				);
			} else {
				log::info!(
					"《AI[{}]》 heartbeat は enabled=false で待機しています（{} 秒周期、reload で有効化可）。",
					svc.persona_label(),
					interval
				);
			}
			let svc_hb = svc.clone();
			handles.push(tokio::spawn(async move { svc_hb.run_heartbeat(interval).await }));
		}
		if let Some(ref initiative) = p.initiative {
			let interval = initiative.interval_secs.max(1);
			log::info!(
				"《AI[{}]》 Initiative Schedulerは enabled={}、{}秒周期です。",
				svc.persona_label(),
				initiative.enabled,
				interval
			);
			let svc_initiative = svc.clone();
			handles.push(tokio::spawn(async move { svc_initiative.run_initiative(interval).await }));
		}

		let rx = tx.subscribe();
		handles.push(tokio::spawn(async move { svc.run(rx).await }));
		runtimes.push(runtime);
	}
	// 登録済みペルソナの一覧を State に反映（Control API の snapshot / pause / resume が参照する）。
	// State 外側ロックは Arc を取り出すだけで即座に解放し、以降は ai_runtimes 自身の RwLock だけを握る。
	let ar = state.read().await.ai_runtimes.clone();
	ar.write().await.extend(runtimes);
	Ok(handles)
}

async fn run_overflow_summary(
	client: &ResponsesClient,
	overflow_model: &Option<String>,
	main_model: Option<&str>,
	max_output_tokens: Option<u32>,
	truncated: &str,
	dropped_count: usize,
	last_at: &Arc<Mutex<Option<Instant>>>,
) -> Option<String> {
	let model_ov = match overflow_model.as_deref().or(main_model) {
		Some(m) => m,
		None => {
			log::warn!("AI: `model` が未設定のためオーバーフロー要約をスキップします。");
			return None;
		}
	};
	log::debug!(
		"メモリ窓オーバーフロー要約を実行します（落とした発話 {} 件、要約 model={}）。",
		dropped_count,
		model_ov
	);
	{
		let mut g = last_at.lock().await;
		*g = Some(Instant::now());
	}
	match completion::summarize_overflow_turns(client, model_ov, max_output_tokens, truncated).await {
		Ok(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
		Ok(_) => {
			log::warn!("メモリ窓オーバーフロー要約の結果が空でした。長期メモには載せません。");
			None
		}
		Err(e) => {
			log::warn!("メモリ窓オーバーフロー要約に失敗しました（本応答は続行）: {:?}", e);
			None
		}
	}
}

async fn append_fine_tuning_record(
	fine_tuning: &OpenAiChatFinetuning,
	custom_instructions: &Option<String>,
	latest_user_content: &str,
	assistant_content: &str,
) -> Result<()> {
	use serde::Serialize;
	use tokio::io::AsyncWriteExt;

	#[derive(Serialize)]
	struct Line {
		messages: Vec<Message>,
	}
	#[derive(Serialize)]
	struct Message {
		role: String,
		content: String,
	}

	let train_path = fine_tuning.train_path();
	if let Some(parent) = std::path::Path::new(&train_path)
		.parent()
		.filter(|path| !path.as_os_str().is_empty())
	{
		tokio::fs::create_dir_all(parent).await?;
	}

	if train_path.to_lowercase().ends_with(".csv") {
		let f = tokio::fs::OpenOptions::new().create(true).append(true).open(&train_path).await?;
		let is_new_file = f.metadata().await?.len() == 0;
		let mut w = csv_async::AsyncWriter::from_writer(f);
		if is_new_file {
			w.write_record(vec!["user", "assistant"]).await?;
		}
		w.write_record(vec![latest_user_content, assistant_content]).await?;
	} else {
		let mut line = Line { messages: vec![] };
		if let Some(ci) = custom_instructions.as_ref() {
			line.messages.push(Message {
				role: "system".to_string(),
				content: ci.clone(),
			});
		}
		line.messages.push(Message {
			role: "user".to_string(),
			content: latest_user_content.to_string(),
		});
		line.messages.push(Message {
			role: "assistant".to_string(),
			content: assistant_content.to_string(),
		});
		let line = serde_json::to_string(&line)?.replace('\n', "");
		tokio::fs::OpenOptions::new()
			.create(true)
			.append(true)
			.open(&train_path)
			.await?
			.write_all(format!("{}\n", line).as_bytes())
			.await?;
	};

	log::trace!("fine-tuning 用のファイルに追記しました: {:?}", train_path);
	Ok(())
}

fn apply_openai_output_filters(
	mut content: String,
	remove_chars: &str,
	assistant_max_chars: Option<usize>,
	assistant_max_sentences: Option<usize>,
	strip_substrings: &[String],
) -> String {
	for sub in strip_substrings {
		if !sub.is_empty() {
			content = content.replace(sub, "");
		}
	}
	for remove_char in remove_chars.chars() {
		content = content.replace(remove_char, "");
	}
	if let Some(max) = assistant_max_sentences.filter(|max| *max > 0) {
		let mut sentence_count = 0usize;
		let mut end_byte = None;
		for (byte_index, ch) in content.char_indices() {
			if matches!(ch, '。' | '！' | '？' | '!' | '?') {
				sentence_count += 1;
				if sentence_count >= max {
					end_byte = Some(byte_index + ch.len_utf8());
					break;
				}
			}
		}
		if let Some(end_byte) = end_byte {
			content.truncate(end_byte);
		}
	}
	if let Some(max) = assistant_max_chars {
		let n = content.chars().count();
		if n > max {
			content = content.chars().take(max).collect();
		}
	}
	content
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum TurnAction {
	Observe,
	Respond,
	React,
	Close,
}

impl TurnAction {
	fn as_str(self) -> &'static str {
		match self {
			Self::Observe => "observe",
			Self::Respond => "respond",
			Self::React => "react",
			Self::Close => "close",
		}
	}
}

#[derive(Debug, Deserialize)]
struct TurnManagerOutput {
	#[serde(default)]
	visual_notes: Vec<super::visual_history::VisualNote>,
	action: TurnAction,
	speak: Option<String>,
	episode_memory: Option<String>,
	avatar_intent: Option<AvatarIntentRequest>,
	avatar_pose: Option<TurnAvatarPoseRequest>,
}

/// Keep completion-side history publication on one path, including observe turns.
/// The request's captured scope, not the source selected at completion, owns notes.
async fn commit_visual_notes(
    history: &Mutex<super::visual_history::VisualHistory>,
    request_scope: Option<&super::visual_history::VisualRequestScope>,
    notes: &[super::visual_history::VisualNote],
    observations: &[crate::control_events::AiContextObservation],
    now: jiff::Timestamp,
) -> bool {
    let Some(scope) = request_scope else { return false; };
    history.lock().await.record_for_request(scope, notes, observations, now)
}

#[cfg(test)]
mod visual_completion_tests {
    use super::*;

    #[tokio::test]
    async fn delayed_parsed_response_cannot_publish_under_new_scope() {
        assert_delayed_completion_is_isolated(false).await;
    }

    #[tokio::test]
    async fn delayed_parsed_response_cannot_publish_after_scope_roundtrip() {
        assert_delayed_completion_is_isolated(true).await;
    }

    async fn assert_delayed_completion_is_isolated(roundtrip: bool) {
        let now: jiff::Timestamp="2026-10-18T00:00:10Z".parse().unwrap();
        let source=|id| crate::control_events::AiContextObservation {
            datum_id:id,channel:"obs:game".into(),source_kind:"obs.scene_source".into(),source_subtype:None,
            observed_at:Some("2026-10-18T00:00:09Z".into()),attachment_count:1,input_image_count:1,
        };
        let history=Arc::new(Mutex::new(super::super::visual_history::VisualHistory::default()));
        history.lock().await.prepare("scene-a/source-a".into(),&[source(1)],now);
        let old_scope = history.lock().await.request_scope();
        let (release,wait)=tokio::sync::oneshot::channel();
        let pending_history=history.clone();
        let old_source=source(1);
        let pending=tokio::spawn(async move {
            wait.await.unwrap();
            let output=parse_turn_manager_output(r#"{"action":"observe","speak":null,"episode_memory":null,"avatar_intent":null,"avatar_pose":null,"visual_notes":[{"datum_id":1,"kind":"state","readouts":[],"evidence":"OLD SCREEN"}]}"#).unwrap();
            commit_visual_notes(&pending_history,Some(&old_scope),&output.visual_notes,&[old_source],now).await
        });
        history.lock().await.prepare("scene-b/source-b".into(),&[source(2)],now);
        let active_scope = if roundtrip { "scene-a/source-a" } else { "scene-b/source-b" };
        if roundtrip {
            history.lock().await.prepare(active_scope.into(), &[source(2)], now);
        }
        let current_scope = history.lock().await.request_scope();
        let current=parse_turn_manager_output(r#"{"action":"observe","speak":null,"episode_memory":null,"avatar_intent":null,"avatar_pose":null,"visual_notes":[{"datum_id":2,"kind":"state","readouts":[{"label":"HP","value":"80"}],"evidence":"CURRENT SCREEN"}]}"#).unwrap();
        assert!(commit_visual_notes(&history,Some(&current_scope),&current.visual_notes,&[source(2)],now).await);
        release.send(()).unwrap();
        assert!(!pending.await.unwrap());
        let retained=history.lock().await.prepare(active_scope.into(),&[source(2)],now).unwrap();
        assert!(retained.contains("CURRENT SCREEN") && retained.contains("HP: 80"));
        assert!(!retained.contains("OLD SCREEN"));
        *history.lock().await=Default::default(); // Feature disabled while a response is pending.
        assert!(!commit_visual_notes(&history,Some(&current_scope),&current.visual_notes,&[source(2)],now).await);
        assert!(!commit_visual_notes(&history,None,&current.visual_notes,&[source(2)],now).await);
    }
}

#[derive(Debug, Deserialize)]
struct TurnAvatarPoseRequest {
	target_link_id: String,
	preset: String,
}

fn turn_manager_text_config() -> TextConfig {
	TextConfig {
		format: Some(TextFormat::JsonSchema {
			name: "vac_ai_turn_decision".to_string(),
			schema: serde_json::json!({
				"type": "object",
				"properties": {
					"action": {
						"type": "string",
						"enum": ["observe", "respond", "react", "close"]
					},
					"speak": {
						"type": ["string", "null"]
					},
					"episode_memory": {
						"type": ["string", "null"]
					},
					"avatar_intent": {
						"type": ["object", "null"],
						"properties": {
							"target_link_id": { "type": "string", "minLength": 1, "maxLength": 64 },
							"intents": {
								"type": "array",
								"minItems": 1,
								"maxItems": 2,
								"items": {
									"type": "object",
									"properties": {
										"kind": { "type": "string", "enum": ["expression", "head_nod", "head_shake", "look_at"] },
										"name": { "type": ["string", "null"], "enum": ["smile", "happy", "concerned", "worried", "stern", "annoyed", "surprised", null] },
										"intensity": { "type": ["number", "null"], "minimum": 0.0, "maximum": 0.6 },
										"duration_ms": { "type": ["integer", "null"], "minimum": 100, "maximum": 2000 },
										"curve": { "type": ["string", "null"], "enum": ["linear", "ease_out", "hold", null] },
										"count": { "type": ["integer", "null"], "minimum": 1, "maximum": 1 },
										"yaw_deg": { "type": ["number", "null"], "minimum": -15.0, "maximum": 15.0 },
										"pitch_deg": { "type": ["number", "null"], "minimum": -15.0, "maximum": 15.0 }
									},
									"required": ["kind", "name", "intensity", "duration_ms", "curve", "count", "yaw_deg", "pitch_deg"],
									"additionalProperties": false
								}
							}
						},
						"required": ["target_link_id", "intents"],
						"additionalProperties": false
					},
					"avatar_pose": {
						"type": ["object", "null"],
						"properties": {
							"target_link_id": { "type": "string", "minLength": 1, "maxLength": 64 },
							"preset": { "type": "string", "minLength": 1, "maxLength": 64 }
						},
						"required": ["target_link_id", "preset"],
						"additionalProperties": false
					}
				},
				"required": ["action", "speak", "episode_memory", "avatar_intent", "avatar_pose"],
				"additionalProperties": false
			}),
			strict: Some(true),
			description: Some("配信AI共演者の発話要否、発話する短文、会話エピソードの差分".to_string()),
		}),
	}
}

fn turn_manager_text_config_with_history(enabled: bool) -> TextConfig {
    let mut config = turn_manager_text_config();
    if enabled {
        if let Some(TextFormat::JsonSchema { schema, .. }) = config.format.as_mut() {
            schema["properties"]["visual_notes"] = serde_json::json!({
                "type":"array", "maxItems":2,
                "items":{"type":"object","properties":{
                    "datum_id":{"type":"integer","minimum":0},
                    "kind":{"type":"string","enum":["state","event"]},
                    "readouts":{"type":"array","maxItems":6,"description":"state: directly readable HUD labels and values, independently of the conversation topic. event: empty array.","items":{
                        "type":"object","properties":{
                            "label":{"type":"string","minLength":1,"maxLength":32,"description":"Copy a label actually written in the image and belonging to this value in the same UI element. Do not pair sorting headers, tabs or menu buttons with unrelated values or nearby symbols. If the label or pairing is unclear, omit this readout and put the visible value with its screen position in evidence; do not invent a semantic label from an icon."},
                            "value":{"type":"string","minLength":1,"maxLength":40}
                        },"required":["label","value"],"additionalProperties":false
                    }},
                    "evidence":{"type":"string","maxLength":160}
                },"required":["datum_id","kind","readouts","evidence"],"additionalProperties":false}
            });
            schema["required"].as_array_mut().unwrap().push(serde_json::json!("visual_notes"));
        }
    }
    config
}

fn turn_manager_allowed_tools(allowed: &[String], enabled: bool) -> Vec<String> {
	allowed
		.iter()
		.filter(|name| !enabled || name.as_str() != tools::AVATAR_INTENT_TOOL_NAME)
		.cloned()
		.collect()
}

fn allowed_tools_for_request<'a>(
	policy: &'a crate::ai::config::OpenAiToolPolicyConf,
	channel: &str,
	request_class: Option<&str>,
) -> Option<&'a Vec<String>> {
	request_class
		.and_then(|value| policy.by_request_class.get(value))
		.or_else(|| policy.by_channel.get(channel))
		.or(policy.default_allowed.as_ref())
}

async fn push_turn_avatar_intent(state: &SharedState, persona_label: &str, request: &AvatarIntentRequest) -> Result<()> {
	validate_turn_avatar_intent(request)?;
	let arguments = serde_json::to_string(request).context("Turn Manager AvatarIntentをJSON化できません")?;
	let (channel, target_link_id, intent_count, datum) =
		tools::build_avatar_intent_datum(&arguments, persona_label).map_err(|error| anyhow!(error))?;
	let datum_id = datum.get_id();
	log::info!(
		"《AI[{}]》 Turn Manager AvatarIntent -> channel={} target_link_id={} intents={} (datum_id={})",
		persona_label,
		channel,
		target_link_id,
		intent_count,
		datum_id
	);
	state.read().await.push_channel_datum(datum).await;
	Ok(())
}

async fn push_turn_avatar_pose(state: &SharedState, persona_label: &str, request: &TurnAvatarPoseRequest) -> Result<()> {
	let datum = build_turn_avatar_pose_datum(persona_label, request)?;
	let datum_id = datum.get_id();
	log::info!(
		"《AI[{}]》 Turn Manager AvatarPose -> channel=avatar_pose target_link_id={} preset={} (datum_id={})",
		persona_label,
		request.target_link_id,
		request.preset,
		datum_id
	);
	state.read().await.push_channel_datum(datum).await;
	Ok(())
}

fn build_turn_avatar_pose_datum(persona_label: &str, request: &TurnAvatarPoseRequest) -> Result<ChannelDatum> {
	validate_turn_avatar_pose(request)?;
	Ok(ChannelDatum::new("avatar_pose".to_string(), request.preset.clone())
		.with_flag(ChannelDatum::FLAG_IS_FINAL)
		.with_source(crate::DataSource::new("ai.turn_manager").with_subtype("action.emit_avatar_pose"))
		.with_meta("source_actor", persona_label.to_string())
		.with_meta("target_link_id", request.target_link_id.clone())
		.with_meta("preset", request.preset.clone())
		.with_meta("schema", "unvac.avatar_pose_preset.v1"))
}

fn validate_turn_avatar_pose(request: &TurnAvatarPoseRequest) -> Result<()> {
	let target = request.target_link_id.trim();
	if target.is_empty()
		|| target.len() > 64
		|| !target
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
	{
		bail!("Turn Manager AvatarPoseのtarget_link_idが不正です");
	}
	let preset = request.preset.trim();
	if preset.is_empty()
		|| preset.len() > 64
		|| !preset
			.bytes()
			.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-'))
	{
		bail!("Turn Manager AvatarPoseのpresetが不正です");
	}
	Ok(())
}

fn avatar_intent_diagnostic_parameters(request: Option<&AvatarIntentRequest>) -> Vec<serde_json::Value> {
	let Some(request) = request else {
		return Vec::new();
	};
	if validate_turn_avatar_intent(request).is_err() {
		return Vec::new();
	}
	request
		.intents
		.iter()
		.filter_map(|intent| {
			let mut value = serde_json::to_value(intent).ok()?;
			// Validation accepts whitespace/case aliases; persist only the bounded
			// canonical expression name, never the original model-supplied string.
			if let Some(name) = value.get_mut("name") {
				*name = serde_json::Value::String(name.as_str()?.trim().to_ascii_lowercase());
			}
			Some(value)
		})
		.collect()
}

fn validate_turn_avatar_intent(request: &AvatarIntentRequest) -> Result<()> {
	let target = request.target_link_id.as_deref().map(str::trim).unwrap_or_default();
	if target.is_empty()
		|| target.len() > 64
		|| !target
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
	{
		bail!("Turn Manager AvatarIntentのtarget_link_idが不正です");
	}
	if request.intents.is_empty() || request.intents.len() > 2 {
		bail!("Turn Manager AvatarIntentは1〜2個に制限されています");
	}
	for intent in &request.intents {
		let (intensity, duration_ms) = match intent {
			AvatarIntent::Expression {
				name,
				intensity,
				duration_ms,
				..
			} => {
				if !matches!(
					name.trim().to_ascii_lowercase().as_str(),
					"smile" | "happy" | "concerned" | "worried" | "stern" | "annoyed" | "surprised"
				) {
					bail!("Turn Manager AvatarIntentのexpression名が不正です: {name}");
				}
				(*intensity, *duration_ms)
			}
			AvatarIntent::HeadNod {
				count,
				intensity,
				duration_ms,
				..
			}
			| AvatarIntent::HeadShake {
				count,
				intensity,
				duration_ms,
				..
			} => {
				if count.is_some_and(|value| value != 1) {
					bail!("Turn Manager AvatarIntentの頷き・首振りは1回に制限されています");
				}
				(*intensity, *duration_ms)
			}
			AvatarIntent::LookAt {
				yaw_deg,
				pitch_deg,
				duration_ms,
			} => {
				if yaw_deg.is_some_and(|value| !(-15.0..=15.0).contains(&value))
					|| pitch_deg.is_some_and(|value| !(-15.0..=15.0).contains(&value))
				{
					bail!("Turn Manager AvatarIntentの視線角度は±15度に制限されています");
				}
				(None, *duration_ms)
			}
		};
		if intensity.is_some_and(|value| !(0.0..=0.6).contains(&value)) {
			bail!("Turn Manager AvatarIntentの強度は0.0〜0.6に制限されています");
		}
		if duration_ms.is_some_and(|value| !(100..=2000).contains(&value)) {
			bail!("Turn Manager AvatarIntentの時間は100〜2000msに制限されています");
		}
	}
	Ok(())
}

fn parse_turn_manager_output(content: &str) -> Result<TurnManagerOutput> {
	serde_json::from_str(content.trim()).with_context(|| {
		format!(
			"Turn Manager の構造化出力を解釈できません: {}",
			content.chars().take(300).collect::<String>()
		)
	})
}

fn resolve_turn_speech(action: TurnAction, speak: Option<String>) -> Result<Option<String>> {
	match action {
		TurnAction::Observe => Ok(None),
		TurnAction::Close => Ok(speak),
		TurnAction::Respond | TurnAction::React => speak
			.map(Some)
			.ok_or_else(|| anyhow!("Turn Manager action={} だが speak が空です", action.as_str())),
	}
}

fn eprint_openai_usage_hint_if_needed(err: &str) {
	let es = err.to_lowercase();
	if es.contains("billing") || es.contains("quota") || es.contains("limit") || es.contains("exceeded") {
		static MSG: &str = r#"
=================================================================
=================================================================
 OpenAI へのリクエストの失敗理由に
  Billing Exceeded Limit Quota
 などのキーワードが含まれています。使用状況やプランを確認して下さい。
 慌てず落ち着いて Usage ページを確認して計画的に人生を楽しみましょう。
 Usage: https://platform.openai.com/account/usage
=================================================================
=================================================================
"#;
		eprint!("{}", MSG);
	}
}

/// χ-5: Responses API tool-loop driver の戻り値。
struct ToolLoopOutput {
	speech_utterance_id: String,
	content: String,
	stream_datum_id: Option<u64>,
	stream_nonempty_delta_chunks: usize,
	usage: ToolLoopUsage,
}

#[derive(Debug, Default)]
struct ToolLoopUsage {
	api_requests: u64,
	tool_calls: u64,
	input_tokens: u64,
	cached_tokens: u64,
	cache_write_tokens: u64,
	output_tokens: u64,
	reasoning_tokens: u64,
}

fn streaming_speech_segment_datum(
	channel_utterance: &str,
	persona_label: &str,
	utterance_id: &str,
	segment_index: u32,
	segment: String,
	utterance_content: String,
) -> ChannelDatum {
	ChannelDatum::new(format!("{channel_utterance}.speech"), segment)
		.with_flag(ChannelDatum::FLAG_IS_FINAL)
		.with_flag(ChannelDatum::FLAG_SPEECH_SEGMENT)
		.with_source(crate::DataSource::new(ChannelDatum::SOURCE_AI_SPEECH).with_actor(persona_label))
		.with_meta("speech_lane", "assistant")
		.with_meta("utterance_id", utterance_id)
		.with_meta("segment_index", i64::from(segment_index))
		.with_meta("utterance_content", utterance_content)
		.with_meta("persona_id", persona_label)
}

fn ai_response_datum(channel_utterance: String, content: String, persona_label: &str) -> ChannelDatum {
	ChannelDatum::new(channel_utterance, content)
		.with_source(crate::DataSource::new(ChannelDatum::SOURCE_AI_RESPONSE).with_actor(persona_label))
		.with_meta("persona_id", persona_label)
}

/// χ-5: Responses API の streaming / non-stream を共通 tool-loop で駆動する。
///
/// - streaming: `create_stream` で SSE を読み、`OutputTextDelta` で live 表示を更新、
///   `Completed` / `Incomplete` で `response.output[]` を確定取得。
///   非 streaming: `create` を呼んで `Response` を直接取得。
/// - `OutputItem::FunctionCall` が 1 件でもあれば、それら（と対応 output）を `request.input[]`
///   に積み直して次ラウンドに入る。無ければ assistant text を抽出して break。
///
/// 既定のラウンド上限は 8（旧 Chat Completions 経路と同じ）。
#[allow(clippy::too_many_arguments)]
async fn drive_responses_tool_loop(
	client: &ResponsesClient,
	mut request: CreateResponseRequest,
	streaming: bool,
	has_tools: bool,
	declared_local: &std::collections::HashSet<String>,
	tool_ctx: Option<&ToolContext>,
	state: &SharedState,
	channel_utterance: &str,
	persona_label: &str,
	stream_turn_manager: bool,
	freshness_origin: Option<(String, u64)>,
	continuation_gap: Option<std::time::Duration>,
) -> Result<ToolLoopOutput> {
	const MAX_TOOL_ROUNDS: usize = 8;

	let mut content = String::new();
	let mut stream_datum_id: Option<u64> = None;
	let mut stream_nonempty_delta_chunks: usize = 0;
	let mut stream_visible_content = String::new();
	let mut usage = ToolLoopUsage::default();
	let speech_utterance_id = format!("{persona_label}:{}", jiff::Timestamp::now());
	if let Some((channel, observation_id)) = freshness_origin {
		let shared = state.read().await;
		let data = shared.channel_data.read().await;
		let guard = shared
			.speech_scheduler
			.playback
			.register_freshness(&speech_utterance_id, channel.clone(), observation_id);
		guard.set_continuation_gap(continuation_gap);
		// Registration and the already-arrived-input check share the channel lock
		// with publication, so an input cannot fall between those two operations.
		if let Some(newer) = data.iter().rev().find(|datum| {
			datum.channel == channel
				&& datum.get_id() > observation_id
				&& datum.has_flag(ChannelDatum::FLAG_IS_FINAL)
				&& !datum.is_preflight_probe()
		}) {
			shared.speech_scheduler.playback.observe_final(&channel, newer.get_id());
		}
		if guard.is_cancelled() {
			log::debug!("《AI[{persona_label}]》 speech already superseded before generation");
		}
	}
	let speech_freshness = state.read().await.speech_scheduler.playback.freshness(&speech_utterance_id);
	let mut speech_segment_index = 0u32;
	let mut speech_utterance_content = String::new();

	// ψ-α: caller が `include: ["reasoning.encrypted_content"]` を付けた場合のみ
	// tool loop round 間で Reasoning item を pass-through する。caller は
	// is_gpt5_family(model) && openai_reasoning_encrypted_passthrough チェックを
	// 済ませた上で include を設定するので、ここでは include の内容だけ見る。
	let reasoning_passthrough = request
		.include
		.as_ref()
		.is_some_and(|v| v.iter().any(|s| s == "reasoning.encrypted_content"));

	let mut round = 0;
	let mut retried_empty = false;
	while round < MAX_TOOL_ROUNDS {
		let response: crate::ai::openai_responses::types::response::Response = if streaming {
			log::debug!("《AI[{}]》 create_stream 開始 (round={})", persona_label, round);
			let stream = client
				.create_stream(request.clone())
				.await
				.context("create_stream 失敗")?;
			log::debug!("《AI[{}]》 create_stream accepted, SSE 受信待ち (round={})", persona_label, round);
			futures::pin_mut!(stream);

			// FunctionCallArgumentsDelta を item_id 単位で蓄積する（Completed 前に最終 response
			// が届けばそちらを優先して使うが、届かないサーバ実装への保険として持っておく）。
			let mut fc_args_accum: HashMap<String, String> = HashMap::new();
			let mut finalized: Option<crate::ai::openai_responses::types::response::Response> = None;
			let mut failure: Option<String> = None;
			let mut ev_count: usize = 0;
			let mut turn_decoder = stream_turn_manager.then(TurnStreamDecoder::default);
			let mut speech_segmenter = stream_turn_manager.then(SpeechSegmenter::default);

			while let Some(ev) = stream.next().await {
				ev_count += 1;
				match &ev {
					Ok(StreamEvent::Created { .. }) => log::trace!("《AI[{}]》 SSE #{}: Created", persona_label, ev_count),
					Ok(StreamEvent::InProgress { .. }) => log::trace!("《AI[{}]》 SSE #{}: InProgress", persona_label, ev_count),
					Ok(StreamEvent::Completed { .. }) => log::debug!("《AI[{}]》 SSE #{}: Completed", persona_label, ev_count),
					Ok(StreamEvent::Incomplete { .. }) => log::debug!("《AI[{}]》 SSE #{}: Incomplete", persona_label, ev_count),
					Ok(StreamEvent::Failed { .. }) => log::warn!("《AI[{}]》 SSE #{}: Failed", persona_label, ev_count),
					Ok(StreamEvent::Error { error }) => log::warn!(
						"《AI[{}]》 SSE #{}: Error code={:?} msg={:?}",
						persona_label,
						ev_count,
						error.code,
						error.message
					),
					_ => {}
				}
				match ev {
					Ok(StreamEvent::OutputTextDelta { delta, .. }) => {
						if delta.is_empty() {
							continue;
						}
						stream_nonempty_delta_chunks += 1;
						content.push_str(&delta);
						if speech_freshness.as_ref().is_some_and(|guard| guard.is_cancelled()) {
							continue;
						}
						let visible_content = if let Some(decoder) = turn_decoder.as_mut() {
							let update = match decoder.push(&delta) {
								Ok(update) => update,
								Err(error) => {
									failure = Some(format!("Turn Manager stream decode error: {error}"));
									break;
								}
							};
							let speech_allowed = matches!(update.action.as_deref(), Some("respond" | "react" | "close"));
							if speech_allowed {
								if !update.speak.starts_with(&stream_visible_content) {
									failure = Some("Turn Manager の visible speak が既存の字幕接頭辞を書き換えました".to_string());
									break;
								}
								let newly_visible = update.speak[stream_visible_content.len()..].to_string();
								stream_visible_content = update.speak;
								if let Some(segmenter) = speech_segmenter.as_mut() {
									for segment in segmenter.push(&newly_visible, update.speak_complete) {
										speech_utterance_content.push_str(&segment);
										let datum = streaming_speech_segment_datum(
											channel_utterance,
											persona_label,
											&speech_utterance_id,
											speech_segment_index,
											segment.clone(),
											speech_utterance_content.clone(),
										);
										state.read().await.push_channel_datum(datum).await;
										log::trace!(
											"《AI[{}]》 streaming speech segment ready (index={} chars={}): {:?}",
											persona_label,
											speech_segment_index,
											segment.chars().count(),
											segment
										);
										speech_segment_index = speech_segment_index.saturating_add(1);
									}
								}
							}
							stream_visible_content.as_str()
						} else {
							content.as_str()
						};
						if visible_content.is_empty() {
							continue;
						}
						match stream_datum_id {
							None => {
								let cd = ai_response_datum(channel_utterance.to_string(), visible_content.to_string(), persona_label);
								let sid = cd.get_id();
								state.read().await.push_channel_datum_quiet(cd).await;
								stream_datum_id = Some(sid);
							}
							Some(sid) => {
								state
									.read()
									.await
									.update_channel_datum_content_by_id(sid, visible_content.to_string())
									.await;
							}
						}
					}
					Ok(StreamEvent::FunctionCallArgumentsDelta { item_id, delta, .. }) => {
						fc_args_accum.entry(item_id).or_default().push_str(&delta);
					}
					Ok(StreamEvent::FunctionCallArgumentsDone { item_id, arguments, .. }) => {
						fc_args_accum.insert(item_id, arguments);
					}
					Ok(StreamEvent::Completed { response }) | Ok(StreamEvent::Incomplete { response }) => {
						finalized = Some(response);
						// The terminal event contains the final response and usage.
						// HTTP EOF may arrive later; it is not a generation boundary.
						break;
					}
					Ok(StreamEvent::Failed { response }) => {
						let err = response
							.error
							.as_ref()
							.and_then(|e| e.message.clone())
							.unwrap_or_else(|| "unknown".to_string());
						failure = Some(format!("response.failed: {err}"));
					}
					Ok(StreamEvent::Error { error }) => {
						failure = Some(format!("stream error: code={:?} message={:?}", error.code, error.message));
					}
					Ok(_) => {}
					Err(e) => {
						failure = Some(format!("stream decode error: {e}"));
						break;
					}
				}
				if failure.is_some() {
					break;
				}
			}
			log::debug!(
				"《AI[{}]》 SSE ループ終了 (events={}, finalized={}, failure={:?})",
				persona_label,
				ev_count,
				finalized.is_some(),
				failure
			);
			if let Some(msg) = failure {
				bail!("《AI[{persona_label}]》 Responses SSE で失敗: {msg}");
			}
			let Some(mut resp) = finalized else {
				bail!("《AI[{persona_label}]》 Responses SSE が Completed/Incomplete を受信しないまま終了しました。（events={ev_count}）");
			};

			// FunctionCall item の arguments が stream 終了時に未充填のケース（サーバ実装差異）用フォールバック。
			for item in resp.output.iter_mut() {
				if let OutputItem::FunctionCall { id, arguments, .. } = item {
					if arguments.is_empty() {
						if let Some(acc) = fc_args_accum.remove(id.as_str()) {
							*arguments = acc;
						}
					}
				}
			}
			resp
		} else {
			client.create(request.clone()).await.context("create 失敗")?
		};

		usage.api_requests += 1;
		if let Some(response_usage) = response.usage.as_ref() {
			usage.input_tokens += u64::from(response_usage.input_tokens);
			usage.output_tokens += u64::from(response_usage.output_tokens);
			if let Some(details) = response_usage.input_tokens_details.as_ref() {
				usage.cached_tokens += u64::from(details.cached_tokens);
				usage.cache_write_tokens += u64::from(details.cache_write_tokens);
			}
			if let Some(details) = response_usage.output_tokens_details.as_ref() {
				usage.reasoning_tokens += u64::from(details.reasoning_tokens);
			}
		}

		// function_call 抽出
		let function_calls: Vec<OutputItem> = response
			.output
			.iter()
			.filter(|item| matches!(item, OutputItem::FunctionCall { .. }))
			.cloned()
			.collect();

		if function_calls.is_empty() {
			// final assistant text を確定する。streaming で live に積んできた text と、response の
			// 確定 output_text を比較し、確定側が長ければ（サーバが補った最終テキストを使って）置き換える。
			if !streaming {
				if let Some(t) = extract_output_text(&response) {
					content = t;
				}
			} else if let Some(final_text) = extract_output_text(&response) {
				if final_text.chars().count() > content.chars().count() {
					content = final_text;
				}
			}
			// Retry the current request, including images, persona, JSON schema and
			// tool results. Never replay tool calls or replace context with a generic prompt.
			if content.trim().is_empty()
				&& !retried_empty
				&& model_policy::should_retry_on_empty_assistant(Some(&request.model))
				&& !speech_freshness.as_ref().is_some_and(|guard| guard.is_cancelled())
			{
				retried_empty = true;
				content.clear();
				log::warn!("《AI[{}]》 空応答のため現在の入力・返答形式を保って1回再試行します。", persona_label);
				continue;
			}
			if let Some(guard) = speech_freshness.as_ref() {
				guard.finish_generation(speech_segment_index as usize);
			}
			return Ok(ToolLoopOutput {
				speech_utterance_id,
				content,
				stream_datum_id,
				stream_nonempty_delta_chunks,
				usage,
			});
		}

		if !has_tools {
			bail!("《AI[{persona_label}]》 tools を宣言していないのに OpenAI が function_call を返しました（想定外）。");
		}
		let ctx = tool_ctx.ok_or_else(|| anyhow!("tool_ctx が未初期化（バグ）"))?;
		usage.tool_calls += function_calls.len() as u64;

		// ψ-α: passthrough 有効時、response.output に含まれる Reasoning item を
		// 次ラウンドの input[] に FunctionCall より先に積み直す。OpenAI 公式の推奨形
		// （"all items between the last user message and your function call output are
		//   passed into the next response untouched"）に従う。
		// 壊れた / 欠落 blob は warn + そのまま透過（item 自体は落とさない）。
		if reasoning_passthrough {
			let (reasoning_items, missing_blob) = collect_reasoning_input_items(&response.output);
			let pushed = reasoning_items.len();
			for item in reasoning_items {
				request.input.push(item);
			}
			if missing_blob > 0 {
				log::warn!(
     "《AI[{}]》 ψ-α: include=reasoning.encrypted_content を指定したが Reasoning item {}/{} 件で encrypted_content が欠落（blob 無しで transit）。API 側の応答形状変更を疑う余地あり。",
     persona_label,
     missing_blob,
     pushed
    );
			} else if pushed > 0 {
				log::trace!(
					"《AI[{}]》 ψ-α: Reasoning item {} 件を次ラウンド input に pass-through (round={}).",
					persona_label,
					pushed,
					round
				);
			}
		}

		// 次ラウンド input[] に FunctionCall item をそのまま積み、続けて FunctionCallOutput を積む。
		for fc in &function_calls {
			if let OutputItem::FunctionCall {
				call_id, name, arguments, ..
			} = fc
			{
				request.input.push(InputItem::FunctionCall {
					call_id: call_id.clone(),
					name: name.clone(),
					arguments: arguments.clone(),
				});
			}
		}
		for fc in &function_calls {
			if let Some(view) = fc.as_function_call() {
				let out_item = tools::dispatch_tool_call(view, declared_local, ctx).await;
				request.input.push(out_item);
			}
		}

		log::debug!(
			"《AI[{}]》 tool-loop round={} function_calls={} 次ラウンドへ。",
			persona_label,
			round + 1,
			function_calls.len()
		);
		round += 1;
	}

	bail!(
		"《AI[{persona_label}]》 OpenAI ツール呼び出しのラウンド上限（{}）に達しました。",
		MAX_TOOL_ROUNDS
	);
}

fn unix_ms() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.unwrap_or_default()
		.as_millis()
		.try_into()
		.unwrap_or(u64::MAX)
}

fn observation_is_superseded(channel_data: &crate::state::ChannelData, obs: &Observation) -> bool {
	channel_data
		.iter()
		.rev()
		.any(|cd| cd.get_id() > obs.datum_id && cd.channel == obs.channel && cd.has_flag(ChannelDatum::FLAG_IS_FINAL))
}

fn initiative_is_superseded(channel_data: &crate::state::ChannelData, obs: &Observation, observe: &ObserveSet) -> bool {
	channel_data.iter().rev().any(|datum| {
		datum.get_id() > obs.datum_id
			&& datum.has_flag(ChannelDatum::FLAG_IS_FINAL)
			&& (datum.channel == observe.channel_utterance || observe.is_trigger(&datum.channel))
			&& !datum.source.as_ref().is_some_and(|source| source.kind == INITIATIVE_SOURCE_KIND)
	})
}

fn should_track_speech_freshness(
	streaming: bool,
	turn_manager: bool,
	priority: bool,
	request_class: Option<&str>,
	floor: Option<&str>,
) -> bool {
	streaming && turn_manager && !priority && request_class.is_none() && floor.is_some_and(|key| !key.trim().is_empty())
}

fn should_discard_superseded_observation(superseded: bool, priority_observation: bool) -> bool {
	superseded && !priority_observation
}

fn try_reserve_initiative(in_flight: &Arc<Semaphore>, capacity: usize) -> Option<tokio::sync::OwnedSemaphorePermit> {
	let capacity = u32::try_from(capacity).ok().filter(|value| *value > 0)?;
	// Claim the whole idle capacity atomically, then immediately return all
	// but one permit, preserving the configured parallel capacity for new input.
	let mut idle = in_flight.clone().try_acquire_many_owned(capacity).ok()?;
	idle.split(1)
}

fn claim_reaction_observation(last_claimed: &AtomicU64, datum_id: u64, origin: ReactionOrigin) -> bool {
	// Timer datums are not human observations and cannot supersede one that
	// is still queued in the observation worker.
	match origin {
		ReactionOrigin::Observation => claim_observation(last_claimed, datum_id),
		ReactionOrigin::Initiative(_) => true,
	}
}

fn claim_observation(last_claimed: &AtomicU64, datum_id: u64) -> bool {
	last_claimed
		.try_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
			(datum_id > current).then_some(datum_id)
		})
		.is_ok()
}

async fn coalesce_pending_observations(
	pending_rx: &mut watch::Receiver<Option<Observation>>,
	mut latest: Observation,
	settle: std::time::Duration,
	max_wait: std::time::Duration,
) -> Observation {
	let started = Instant::now();
	loop {
		let max_remaining = max_wait.saturating_sub(started.elapsed());
		if max_remaining.is_zero() {
			break;
		}
		tokio::select! {
			changed = pending_rx.changed() => {
				if changed.is_err() {
					break;
				}
				if let Some(newer) = pending_rx.borrow_and_update().clone() {
					latest = newer;
				}
			}
			_ = tokio::time::sleep(settle.min(max_remaining)) => break,
		}
	}
	latest
}

fn make_client(conf: &AiPersonaConf) -> Result<ResponsesClient> {
 #[cfg(feature="desktop-native-review")]
 {
  let _ = conf;
  return ResponsesClient::new(super::review_provider::from_environment()?).map_err(|e| anyhow!("Review provider: {e}"));
 }
 #[cfg(not(feature="desktop-native-review"))]
 { make_production_client(conf) }
}

#[cfg(not(feature="desktop-native-review"))]
fn make_production_client(conf: &AiPersonaConf) -> Result<ResponsesClient> {
	let api_key = super::resolve_openai_api_key(conf)?;
	let Some(api_key) = api_key else {
		bail!(
   "OpenAI の API KEY が設定されていません。環境変数 VAC_OPENAI_API_KEY、api_key_file、または `[[ai.personas]]` の api_key を指定して下さい。"
  );
	};
	let cfg = ResponsesClientConfig {
		api_key,
		..Default::default()
	};
	ResponsesClient::new(cfg).map_err(|e| anyhow!("ResponsesClient の構築に失敗しました: {e}"))
}

#[cfg(test)]
#[path = "empty_response_tests.rs"]
mod empty_response_tests;

#[cfg(test)]
#[path = "request_capture_tests.rs"]
mod request_capture_tests;

#[cfg(test)]
mod chi6_resolver_tests {
	use super::*;

	#[test]
	fn initiative_reservation_defers_to_an_existing_response_without_queueing() {
		let slots = Arc::new(Semaphore::new(2));
		let ordinary = slots.clone().try_acquire_owned().unwrap();
		assert!(try_reserve_initiative(&slots, 2).is_none());
		assert_eq!(slots.available_permits(), 1);
		drop(ordinary);
		let initiative = try_reserve_initiative(&slots, 2).unwrap();
		assert_eq!(slots.available_permits(), 1);
		// A later ordinary response retains the existing parallel slot.
		let ordinary = slots.clone().try_acquire_owned().unwrap();
		drop(initiative);
		drop(ordinary);
		assert_eq!(slots.available_permits(), 2);
	}

	#[tokio::test]
	async fn initiative_reservation_is_released_on_cancel_at_capacity_one() {
		let slots = Arc::new(Semaphore::new(1));
		let permit = try_reserve_initiative(&slots, 1).unwrap();
		let task = tokio::spawn(async move {
			let _permit = permit;
			std::future::pending::<()>().await;
		});
		assert_eq!(slots.available_permits(), 0);
		task.abort();
		assert!(task.await.unwrap_err().is_cancelled());
		assert!(try_reserve_initiative(&slots, 1).is_some());
	}

	#[test]
	fn initiative_marker_does_not_discard_a_queued_human_observation() {
		let claimed = AtomicU64::new(10);
		assert!(claim_reaction_observation(&claimed, 20, ReactionOrigin::Initiative(InitiativeCandidateKind::OngoingConversation)));
		assert_eq!(claimed.load(Ordering::SeqCst), 10);
		assert!(claim_reaction_observation(&claimed, 15, ReactionOrigin::Observation));
		assert!(!claim_reaction_observation(&claimed, 15, ReactionOrigin::Observation));
		assert!(claim_reaction_observation(&claimed, 21, ReactionOrigin::Observation));
	}

	#[tokio::test]
	async fn failure_diagnostic_initiative_dispatched_request_spends_budget_even_when_api_fails() {
		let mut conf: crate::Conf = toml::from_str("").unwrap();
		conf.runtime_dir = Some(std::env::temp_dir().join(format!("vac_initiative_request_{}", rand::random::<u64>())));
		let audio = Arc::new(Mutex::new(crate::AudioSink::disconnected_for_test()));
		let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
		state.read().await.twitch_stream_state.apply_eventsub("stream.online", &serde_json::json!({"broadcaster_user_login":"fixture"})).await;
		let mut persona = persona_minimal();
		persona.api_key = Some("local-test".into());
		persona.initiative = Some(super::super::config::AiInitiativeConf {
			enabled: true, interval_secs: 1, startup_grace_secs: 0, idle_secs: 0,
			min_interval_secs: 0, max_attempts_per_hour: 1, max_speeches_per_hour: 2,
		});
		let mut service = AiService::new(persona, state.clone(), None, None, None,
			Arc::new(AtomicBool::new(false)), Arc::new(RwLock::new(AiUsageTelemetry::default()))).await.unwrap();
		service.client = ResponsesClient::new(ResponsesClientConfig { api_key: "local-test".into(), base_url: "http://127.0.0.1:9".into(), ..Default::default() }).unwrap();
		let mut events = state.read().await.control_event_tx.subscribe();
		let task = tokio::spawn(service.clone().run_initiative(1));
		let mut requests = 0;
		let mut failures = 0;
		let mut diagnostic_failures = 0;
		let outcome = tokio::time::timeout(Duration::from_secs(15), async {
			loop {
				match events.recv().await.unwrap() {
					crate::control_events::ControlEvent::AiRequestContext { .. } => requests += 1,
					crate::control_events::ControlEvent::AiReactionFailure { stage, category, http_status, .. } => {
						assert_eq!(stage, "request_or_tool_loop");
						assert_eq!(category, "transport");
						assert_eq!(http_status, None);
						diagnostic_failures += 1;
					},
					crate::control_events::ControlEvent::AiInitiativeDecision { outcome: crate::control_events::AiInitiativeOutcome::Failed, .. } => failures += 1,
					crate::control_events::ControlEvent::AiInitiativeDecision { outcome: crate::control_events::AiInitiativeOutcome::SuppressedAttemptBudget, .. } => break,
					_ => {}
				}
			}
		}).await;
		task.abort();
		let _ = task.await;
		assert!(outcome.is_ok(), "attempt limit was never reached");
		assert_eq!(requests, 1);
		assert_eq!(failures, 1);
		assert_eq!(diagnostic_failures, 1);
		let issues = state.read().await.runtime_health.snapshot().await;
		assert!(issues.iter().any(|issue| issue.component == "ai_reaction" && issue.code == "transport"));
        let health = state.read().await.runtime_health.clone();
        let error = anyhow::anyhow!("fixture failure");
        health.report(super::super::failure_diagnostic::issue("other-partner", "request_or_tool_loop", &error)).await;
        service.emit_reaction_failure(1, "turn_output_parse", &error).await;
        service.clear_reaction_failure("request_or_tool_loop").await;
        let remaining = health.snapshot().await;
        assert_eq!(remaining.len(), 2);
        assert!(remaining.iter().any(|i| i.key == "ai_reaction:other-partner:request_or_tool_loop"));
        assert!(remaining.iter().any(|i| i.key == format!("ai_reaction:{}:turn_output_parse", service.persona_label())));
        service.clear_reaction_failure("turn_output_parse").await;
        let remaining = health.snapshot().await;
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].key, "ai_reaction:other-partner:request_or_tool_loop");
		assert_eq!(service.initiative_runtime.lock().await.attempt_times_ms.len(), 1);
	}

	#[tokio::test]
	async fn initiative_cancelled_for_new_human_speech_preserves_attempt_budget() {
		let mut conf: crate::Conf = toml::from_str("").unwrap();
		conf.runtime_dir = Some(std::env::temp_dir().join(format!("vac_initiative_cancel_{}", rand::random::<u64>())));
		let audio = Arc::new(Mutex::new(crate::AudioSink::disconnected_for_test()));
		let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
		state.read().await.twitch_stream_state.apply_eventsub("stream.online", &serde_json::json!({"broadcaster_user_login":"fixture"})).await;
		let mut persona = persona_minimal();
		persona.api_key = Some("local-test".into());
		persona.observe.triggers = vec!["user".into()];
		persona.respect_speech_floor = Some("fixture".into());
		persona.initiative = Some(super::super::config::AiInitiativeConf {
			enabled: true, interval_secs: 1, startup_grace_secs: 0, idle_secs: 0,
			min_interval_secs: 600, max_attempts_per_hour: 1, max_speeches_per_hour: 2,
		});
		let mut service = AiService::new(persona, state.clone(), None, None, None,
			Arc::new(AtomicBool::new(false)), Arc::new(RwLock::new(AiUsageTelemetry::default()))).await.unwrap();
		service.client = ResponsesClient::new(ResponsesClientConfig { api_key: "local-test".into(), base_url: "http://127.0.0.1:9".into(), ..Default::default() }).unwrap();
		state.read().await.speech_floor.set_held("fixture", true);
		let mut events = state.read().await.control_event_tx.subscribe();
		let task = tokio::spawn(service.clone().run_initiative(1));
		tokio::time::timeout(Duration::from_secs(5), async {
			loop {
				if service.channel_data.read().await.iter().any(|d| d.source.as_ref().is_some_and(|s| s.kind == INITIATIVE_SOURCE_KIND)) { break; }
				tokio::task::yield_now().await;
			}
		}).await.unwrap();
		service.channel_data.write().await.push_back(ChannelDatum::new("user".into(), "new human speech".into()).with_flag(ChannelDatum::FLAG_IS_FINAL));
		state.read().await.speech_floor.set_held("fixture", false);
		let mut requests = 0;
		tokio::time::timeout(Duration::from_secs(5), async {
			loop {
				match events.recv().await.unwrap() {
					crate::control_events::ControlEvent::AiRequestContext { .. } => requests += 1,
					crate::control_events::ControlEvent::AiInitiativeDecision { outcome: crate::control_events::AiInitiativeOutcome::SkippedBeforeRequest, .. } => break,
					_ => {}
				}
			}
		}).await.unwrap();
		assert_eq!(requests, 0);
		assert!(service.initiative_runtime.lock().await.attempt_times_ms.is_empty());
		// Keep the same scheduler alive: cancellation must not leave admission,
		// cooldown or attempt accounting stuck until restart. The next candidate
		// must reach transport and only then consume the single allowed attempt.
		let mut failures = 0;
		let resumed = tokio::time::timeout(Duration::from_secs(8), async {
			loop {
				match events.recv().await.unwrap() {
					crate::control_events::ControlEvent::AiRequestContext { .. } => requests += 1,
					crate::control_events::ControlEvent::AiInitiativeDecision { outcome: crate::control_events::AiInitiativeOutcome::Failed, .. } => { failures += 1; break; },
					_ => {}
				}
			}
		}).await;
		task.abort();
		let _ = task.await;
		assert!(resumed.is_ok(), "cancelled initiative prevented the next candidate from dispatching");
		assert_eq!(requests, 1);
		assert_eq!(failures, 1);
		assert_eq!(service.initiative_runtime.lock().await.attempt_times_ms.len(), 1);
	}

	#[tokio::test]
	async fn initiative_scheduler_defers_busy_tick_without_spending_budget() {
		let mut conf: crate::Conf = toml::from_str("").unwrap();
		conf.runtime_dir = Some(std::env::temp_dir().join(format!("vac_initiative_admission_{}", rand::random::<u64>())));
		let audio = Arc::new(Mutex::new(crate::AudioSink::open_default().unwrap()));
		let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
		state.read().await.twitch_stream_state.apply_eventsub("stream.online", &serde_json::json!({"broadcaster_user_login":"fixture"})).await;
		let mut persona = persona_minimal();
		persona.api_key = Some("local-test".into());
		persona.initiative = Some(super::super::config::AiInitiativeConf {
			enabled: true, interval_secs: 1, startup_grace_secs: 0, idle_secs: 0,
			min_interval_secs: 0, max_attempts_per_hour: 4, max_speeches_per_hour: 2,
		});
		let mut service = AiService::new(persona, state.clone(), None, None, None,
			Arc::new(AtomicBool::new(false)), Arc::new(RwLock::new(AiUsageTelemetry::default()))).await.unwrap();
		// Even if admission regresses, this test cannot contact the real API.
		service.client = ResponsesClient::new(ResponsesClientConfig { api_key: "local-test".into(), base_url: "http://127.0.0.1:9".into(), ..Default::default() }).unwrap();
		let ordinary = service.in_flight.clone().try_acquire_owned().unwrap();
		let mut events = state.read().await.control_event_tx.subscribe();
		let task = tokio::spawn(service.clone().run_initiative(1));
		let suppressed = tokio::time::timeout(Duration::from_secs(5), async {
			loop {
				if let Ok(crate::control_events::ControlEvent::AiInitiativeDecision { outcome, .. }) = events.recv().await {
					break outcome;
				}
			}
		}).await;
		assert!(matches!(suppressed, Ok(crate::control_events::AiInitiativeOutcome::SuppressedActiveConversation)));
		assert!(service.initiative_runtime.lock().await.attempt_times_ms.is_empty());
		assert_eq!(service.last_claimed_observation_id.load(Ordering::SeqCst), 0);
		drop(ordinary);
		let resumed = tokio::time::timeout(Duration::from_secs(5), async {
			loop {
				if let Ok(crate::control_events::ControlEvent::AiInitiativeDecision { outcome, .. }) = events.recv().await {
					break outcome;
				}
			}
		}).await;
		task.abort();
		let _ = task.await;
		assert!(matches!(resumed, Ok(crate::control_events::AiInitiativeOutcome::CandidateStreamStart)));
	}

	pub(super) fn persona_minimal() -> AiPersonaConf {
		AiPersonaConf {
			id: Some("test".to_string()),
			display_name: None,
			is_enabled: true,
			channel_utterance: Some("to".to_string()),
			channel_effect: None,
			observe: Default::default(),
			decision: None,
			heartbeat: None,
			initiative: None,
			turn_management: None,
			observation_buffer: None,
			api_key: None,
			api_key_file: None,
			model: Some("gpt-5-mini".to_string()),
			openai_model_policy: None,
			custom_instructions: None,
			system_instructions_extra: None,
			openai_max_output_tokens: None,
			max_tokens: None,
			openai_reasoning_effort: None,
			openai_store: None,
			openai_reasoning_encrypted_passthrough: None,
			openai_prompt_cache_key: None,
			temperature: None,
			top_p: None,
			n: None,
			presence_penalty: None,
			frequency_penalty: None,
			user: None,
			memory_capacity: None,
			memory_max_chars: None,
			memory_budget_approx_tokens: None,
			memory_budget_chars_per_approx_token: None,
			memory_overflow_summary_enabled: None,
			memory_overflow_summary_model: None,
			memory_overflow_summary_max_output_tokens: None,
			memory_overflow_summary_max_completion_tokens: None,
			memory_overflow_summary_max_input_chars: None,
			memory_overflow_summary_min_chars: None,
			memory_overflow_summary_cooldown_secs: None,
			memory_summary: None,
			memory_summary_path: None,
			openai_few_shot: vec![],
			persona_anchor: None,
			force_activate_regex_pattern: None,
			ignore_regex_pattern: None,
			min_interval_in_secs: None,
			remove_chars: None,
			assistant_max_chars: None,
			assistant_max_sentences: None,
			assistant_strip_substrings: vec![],
			openai_stream: None,
			openai_tools_json_path: None,
			openai_tool_choice: None,
			openai_parallel_tool_calls: None,
			openai_tool_policy: None,
			openai_max_in_flight: None,
			fine_tuning: None,
			respect_speech_floor: None,
		}
	}

	#[test]
	fn latest_final_observation_supersedes_older_turn_on_same_channel() {
		let older = ChannelDatum::new("user".into(), "敵が来ました".into()).with_flag(ChannelDatum::FLAG_IS_FINAL);
		let obs = Observation {
			datum_id: older.get_id(),
			channel: older.channel.clone(),
		};
		let unrelated = ChannelDatum::new("chat".into(), "hello".into()).with_flag(ChannelDatum::FLAG_IS_FINAL);
		let newer = ChannelDatum::new("user".into(), "いったん戻ります".into()).with_flag(ChannelDatum::FLAG_IS_FINAL);
		let channel_data = std::collections::VecDeque::from([older, unrelated, newer]);
		assert!(observation_is_superseded(&channel_data, &obs));
	}

	#[test]
	fn unrelated_or_unfinalized_observation_does_not_supersede_turn() {
		let older = ChannelDatum::new("user".into(), "考えます".into()).with_flag(ChannelDatum::FLAG_IS_FINAL);
		let obs = Observation {
			datum_id: older.get_id(),
			channel: older.channel.clone(),
		};
		let partial = ChannelDatum::new("user".into(), "考えてから".into());
		let unrelated = ChannelDatum::new("chat".into(), "hello".into()).with_flag(ChannelDatum::FLAG_IS_FINAL);
		let channel_data = std::collections::VecDeque::from([older, partial, unrelated]);
		assert!(!observation_is_superseded(&channel_data, &obs));
	}

	#[test]
	fn human_activity_supersedes_initiative_waiting_for_speech_floor() {
		let initiative = ChannelDatum::new("__flowgraph.ai.input/kaltsit".into(), "candidate".into())
			.with_flag(ChannelDatum::FLAG_IS_FINAL)
			.with_source(crate::state::DataSource::new(INITIATIVE_SOURCE_KIND));
		let obs = Observation {
			datum_id: initiative.get_id(),
			channel: initiative.channel.clone(),
		};
		let newer_user = ChannelDatum::new("user".into(), "new human speech".into()).with_flag(ChannelDatum::FLAG_IS_FINAL);
		let observe = ObserveSet {
			triggers: vec!["user".into()],
			channel_utterance: "ai".into(),
			include_all: false,
			include_additional: Vec::new(),
			exclude: Vec::new(),
		};
		let channel_data = std::collections::VecDeque::from([initiative, newer_user]);
		assert!(initiative_is_superseded(&channel_data, &obs, &observe));
	}

	#[test]
	fn force_activation_regex_is_priority_during_speech_floor_wait() {
		let persona = persona_minimal();
		let mut persona = persona;
		persona.force_activate_regex_pattern = Some("ケルシー".into());
		let decision = DecisionSpec::from_persona(&persona).unwrap();
		assert!(decision.is_priority_observation(&DecisionInput {
			channel: "user",
			latest_trigger_content: "ケルシー先生、沈黙していない？",
		}));
	}

	#[test]
	fn avatar_intent_diagnostics_keep_bounded_parameters_without_rejected_text() {
		let mut request: AvatarIntentRequest = serde_json::from_value(serde_json::json!({
			"target_link_id":"kaltsit_ai", "intents":[
				{"kind":"expression","name":"  SMILE  ","intensity":0.4,"duration_ms":900},
				{"kind":"head_nod","count":1,"intensity":0.3,"duration_ms":700}
			]
		}))
		.unwrap();
		let parameters = avatar_intent_diagnostic_parameters(Some(&request));
		assert_eq!(parameters.len(), 2);
		assert_eq!(parameters[0]["name"], "smile");
		assert_eq!(parameters[0]["duration_ms"], 900);
		assert_eq!(parameters[1]["kind"], "head_nod");
		if let AvatarIntent::Expression { name, .. } = &mut request.intents[0] {
			*name = "untrusted dialogue text".into();
		}
		assert!(avatar_intent_diagnostic_parameters(Some(&request)).is_empty());
		assert!(avatar_intent_diagnostic_parameters(None).is_empty());
	}

	#[test]
	fn superseded_priority_observation_is_preserved() {
		assert!(should_track_speech_freshness(true, true, false, None, Some("human")));
		assert!(!should_track_speech_freshness(true, true, true, None, Some("human")));
		assert!(!should_track_speech_freshness(
			true,
			true,
			false,
			Some("channel_points"),
			Some("human")
		));
		assert!(!should_track_speech_freshness(false, true, false, None, Some("human")));
		assert!(!should_track_speech_freshness(true, false, false, None, Some("human")));
		assert!(!should_track_speech_freshness(true, true, false, None, Some(" ")));
		assert!(should_discard_superseded_observation(true, false));
		assert!(!should_discard_superseded_observation(true, true));
		assert!(!should_discard_superseded_observation(false, false));
	}

	#[test]
	fn observation_claim_allows_each_datum_only_once_and_prefers_newer_ids() {
		let claimed = AtomicU64::new(0);
		assert!(claim_observation(&claimed, 10));
		assert!(!claim_observation(&claimed, 10));
		assert!(!claim_observation(&claimed, 9));
		assert!(claim_observation(&claimed, 11));
	}

	#[tokio::test]
	async fn observation_buffer_coalesces_to_latest_observation() {
		let first = Observation {
			datum_id: 1,
			channel: "user".into(),
		};
		let (tx, mut rx) = watch::channel(Some(first.clone()));
		rx.borrow_and_update();
		let sender = tokio::spawn(async move {
			tokio::time::sleep(std::time::Duration::from_millis(5)).await;
			tx.send_replace(Some(Observation {
				datum_id: 2,
				channel: "user".into(),
			}));
			tokio::time::sleep(std::time::Duration::from_millis(5)).await;
			tx.send_replace(Some(Observation {
				datum_id: 3,
				channel: "user".into(),
			}));
		});

		let latest = coalesce_pending_observations(
			&mut rx,
			first,
			std::time::Duration::from_millis(20),
			std::time::Duration::from_millis(100),
		)
		.await;
		sender.await.unwrap();
		assert_eq!(latest.datum_id, 3);
	}

	#[tokio::test]
	async fn observation_buffer_respects_max_wait_under_continuous_updates() {
		let first = Observation {
			datum_id: 1,
			channel: "user".into(),
		};
		let (tx, mut rx) = watch::channel(Some(first.clone()));
		rx.borrow_and_update();
		let sender = tokio::spawn(async move {
			for datum_id in 2..=20 {
				tokio::time::sleep(std::time::Duration::from_millis(5)).await;
				tx.send_replace(Some(Observation {
					datum_id,
					channel: "user".into(),
				}));
			}
		});

		let started = Instant::now();
		let latest = coalesce_pending_observations(
			&mut rx,
			first,
			std::time::Duration::from_millis(20),
			std::time::Duration::from_millis(40),
		)
		.await;
		assert!(started.elapsed() < std::time::Duration::from_millis(90));
		assert!(latest.datum_id > 1);
		sender.abort();
	}

	#[test]
	fn output_filter_truncates_at_sentence_boundary() {
		let filtered = apply_openai_output_filters(
			"了解だ。無理に回さない。戻ったら状況を教えてくれ。今どこまで進んだ？".into(),
			"",
			None,
			Some(2),
			&[],
		);
		assert_eq!(filtered, "了解だ。無理に回さない。");
	}

	#[test]
	fn output_filter_keeps_shorter_response_unchanged() {
		let filtered = apply_openai_output_filters("了解だ。".into(), "", None, Some(2), &[]);
		assert_eq!(filtered, "了解だ。");
	}

	#[test]
	fn streaming_speech_segment_uses_dedicated_channel_and_metadata() {
		let datum = streaming_speech_segment_datum(
			"ai",
			"kaltsit",
			"turn-7",
			2,
			"無理はするな。".into(),
			"焦る必要はない。無理はするな。".into(),
		);
		assert_eq!(datum.channel, "ai.speech");
		assert_eq!(datum.content, "無理はするな。");
		assert!(datum.has_flag(ChannelDatum::FLAG_IS_FINAL));
		assert!(datum.has_flag(ChannelDatum::FLAG_SPEECH_SEGMENT));
		assert_eq!(datum.meta["speech_lane"], "assistant");
		assert_eq!(datum.meta["utterance_id"], "turn-7");
		assert_eq!(datum.meta["segment_index"], 2);
		assert_eq!(datum.meta["utterance_content"], "焦る必要はない。無理はするな。");
		assert_eq!(datum.meta["persona_id"], "kaltsit");
		assert_eq!(
			datum.source().map(|source| source.kind.as_str()),
			Some(ChannelDatum::SOURCE_AI_SPEECH)
		);
	}

	#[test]
	fn ai_response_uses_canonical_source_independent_of_channel_name() {
		let datum = ai_response_datum("custom-output".into(), "了解した。".into(), "kaltsit");
		assert_eq!(datum.channel, "custom-output");
		assert_eq!(datum.meta["persona_id"], "kaltsit");
		assert_eq!(
			datum.source().map(|source| source.kind.as_str()),
			Some(ChannelDatum::SOURCE_AI_RESPONSE)
		);
	}

	#[test]
	fn turn_manager_output_accepts_observe_without_speech() {
		let output =
			parse_turn_manager_output(r#"{"action":"observe","speak":null,"episode_memory":null,"avatar_intent":null,"avatar_pose":null}"#)
				.unwrap();
		assert_eq!(output.action, TurnAction::Observe);
		assert_eq!(output.speak, None);
		assert!(output.episode_memory.is_none());
		assert!(output.avatar_intent.is_none());
		assert!(output.avatar_pose.is_none());
	}

	#[test]
	fn turn_manager_schema_is_strict_and_nullable() {
		let value = serde_json::to_value(turn_manager_text_config()).unwrap();
		assert_eq!(value["format"]["type"], "json_schema");
		assert_eq!(value["format"]["strict"], true);
		assert_eq!(value["format"]["schema"]["additionalProperties"], false);
		assert_eq!(value["format"]["schema"]["properties"]["speak"]["type"][1], "null");
		assert_eq!(value["format"]["schema"]["properties"]["episode_memory"]["type"][1], "null");
		assert_eq!(value["format"]["schema"]["properties"]["avatar_intent"]["type"][1], "null");
		assert_eq!(value["format"]["schema"]["properties"]["avatar_pose"]["type"][1], "null");
		assert_eq!(
			value["format"]["schema"]["properties"]["avatar_intent"]["properties"]["intents"]["maxItems"],
			2
		);
		assert_eq!(
			value["format"]["schema"]["properties"]["avatar_intent"]["properties"]["intents"]["items"]["properties"]["intensity"]
				["maximum"],
			0.6
		);
	}

	#[test]
	fn turn_manager_instructions_bound_avatar_intent_cadence() {
		assert!(TURN_MANAGER_INSTRUCTIONS.contains("observeでは必ずnull"));
		assert!(TURN_MANAGER_INSTRUCTIONS.contains("強い感情をreactとして発話する場合"));
		assert!(TURN_MANAGER_INSTRUCTIONS.contains("小さなexpressionを1個返す"));
		assert!(TURN_MANAGER_INSTRUCTIONS.contains("中立的・分析的なrespondは原則null"));
		assert!(TURN_MANAGER_INSTRUCTIONS.contains("ファイルパスや未提示の名前を作らない"));
		assert!(TURN_MANAGER_INSTRUCTIONS.contains("avatar_intentとavatar_poseは併用できる"));
	}

	#[test]
	fn turn_manager_output_accepts_bounded_avatar_intent() {
		let output = parse_turn_manager_output(
			r#"{"action":"react","speak":"よかった。","episode_memory":null,"avatar_intent":{"target_link_id":"kaltsit_ai","intents":[{"kind":"expression","name":"smile","intensity":0.45,"duration_ms":700,"curve":"ease_out","count":null,"yaw_deg":null,"pitch_deg":null},{"kind":"head_nod","name":null,"intensity":0.35,"duration_ms":900,"curve":"ease_out","count":1,"yaw_deg":null,"pitch_deg":null}]},"avatar_pose":null}"#,
		)
		.unwrap();
		let request = output.avatar_intent.unwrap();
		assert_eq!(request.intents.len(), 2);
		validate_turn_avatar_intent(&request).unwrap();
	}

	#[test]
	fn turn_manager_avatar_intent_rejects_excessive_motion() {
		let request = AvatarIntentRequest {
			target_link_id: Some("kaltsit_ai".into()),
			intents: vec![AvatarIntent::HeadShake {
				count: Some(2),
				intensity: Some(0.8),
				duration_ms: Some(2500),
				curve: None,
			}],
		};
		assert!(validate_turn_avatar_intent(&request).is_err());
	}

	#[test]
	fn turn_manager_output_accepts_named_avatar_pose() {
		let output = parse_turn_manager_output(
			r#"{"action":"respond","speak":"ここを見ろ。","episode_memory":null,"avatar_intent":null,"avatar_pose":{"target_link_id":"kaltsit_ai","preset":"open_hands"}}"#,
		)
		.unwrap();
		let request = output.avatar_pose.unwrap();
		assert_eq!(request.target_link_id, "kaltsit_ai");
		assert_eq!(request.preset, "open_hands");
		validate_turn_avatar_pose(&request).unwrap();
		let datum = build_turn_avatar_pose_datum("Kaltsit", &request).unwrap();
		assert_eq!(datum.channel, "avatar_pose");
		assert_eq!(datum.content, "open_hands");
		assert!(datum.has_flag(ChannelDatum::FLAG_IS_FINAL));
		assert_eq!(
			datum.source.as_ref().and_then(|source| source.subtype.as_deref()),
			Some("action.emit_avatar_pose")
		);
		assert_eq!(datum.meta_get("source_actor").and_then(serde_json::Value::as_str), Some("Kaltsit"));
		assert_eq!(
			datum.meta_get("target_link_id").and_then(serde_json::Value::as_str),
			Some("kaltsit_ai")
		);
		assert_eq!(
			datum.meta_get("schema").and_then(serde_json::Value::as_str),
			Some("unvac.avatar_pose_preset.v1")
		);
	}

	#[test]
	fn turn_manager_avatar_pose_rejects_paths_and_unknown_shape() {
		for preset in ["../pose", "poses/open_hands", "OpenHands"] {
			let request = TurnAvatarPoseRequest {
				target_link_id: "kaltsit_ai".into(),
				preset: preset.into(),
			};
			assert!(validate_turn_avatar_pose(&request).is_err(), "accepted preset: {preset}");
		}
	}

	#[test]
	fn turn_manager_removes_avatar_function_tool_from_allowed_policy() {
		let allowed = vec!["vac_ping".into(), tools::AVATAR_INTENT_TOOL_NAME.into(), "vac_emit_effect".into()];
		assert_eq!(
			turn_manager_allowed_tools(&allowed, true),
			vec!["vac_ping".to_string(), "vac_emit_effect".to_string()]
		);
		assert_eq!(turn_manager_allowed_tools(&allowed, false), allowed);
	}

	#[test]
	fn trusted_request_class_tool_policy_precedes_channel_and_default() {
		let policy = crate::ai::config::OpenAiToolPolicyConf {
			effect_allowed_channels: None,
			require_strict: true,
			default_allowed: Some(vec!["default".into()]),
			by_channel: [("user".into(), vec!["channel".into()])].into(),
			by_request_class: [("twitch_channel_points_reward".into(), vec!["web_search".into()])].into(),
		};
		assert_eq!(
			allowed_tools_for_request(&policy, "user", Some("twitch_channel_points_reward")),
			policy.by_request_class.get("twitch_channel_points_reward")
		);
		assert_eq!(allowed_tools_for_request(&policy, "user", None), policy.by_channel.get("user"));
		assert_eq!(allowed_tools_for_request(&policy, "other", None), policy.default_allowed.as_ref());
	}

	#[test]
	fn turn_manager_close_may_end_silently() {
		assert_eq!(resolve_turn_speech(TurnAction::Close, None).unwrap(), None);
		assert!(resolve_turn_speech(TurnAction::Respond, None).is_err());
	}

	// Supply the environment value directly; tests never mutate process-wide state.
	#[test]
	fn max_output_tokens_precedence_env_new_legacy() {
		for (env_value, modern, legacy, expected) in [
			(None, None, None, None),
			(None, None, Some(256), Some(256)),
			(None, Some(1024), Some(256), Some(1024)),
			(Some("2048"), Some(1024), Some(256), Some(2048)),
			(Some("not-a-number"), Some(555), None, Some(555)),
			(Some("not-a-number"), None, Some(111), Some(111)),
			(Some(""), Some(999), None, Some(999)),
			(Some(" 2048 "), None, None, Some(2048)),
		] {
			let mut persona = persona_minimal();
			persona.openai_max_output_tokens = modern;
			persona.max_tokens = legacy;
			assert_eq!(resolve_openai_max_output_tokens_with_env(&persona, env_value), expected);
		}
	}

	#[test]
	fn reasoning_effort_maps_each_variant() {
		let mut p = persona_minimal();
		p.openai_reasoning_effort = Some(OpenAiReasoningEffortConf::Low);
		assert_eq!(resolve_openai_reasoning_effort(&p), Some(ReasoningEffort::Low));
		p.openai_reasoning_effort = Some(OpenAiReasoningEffortConf::Medium);
		assert_eq!(resolve_openai_reasoning_effort(&p), Some(ReasoningEffort::Medium));
		p.openai_reasoning_effort = Some(OpenAiReasoningEffortConf::High);
		assert_eq!(resolve_openai_reasoning_effort(&p), Some(ReasoningEffort::High));
		p.openai_reasoning_effort = None;
		assert_eq!(resolve_openai_reasoning_effort(&p), None);
	}

	#[test]
	fn overflow_summary_max_prefers_new_key_over_legacy() {
		let mut p = persona_minimal();
		p.memory_overflow_summary_max_output_tokens = Some(800);
		p.memory_overflow_summary_max_completion_tokens = Some(200);
		assert_eq!(resolve_overflow_summary_max_output_tokens(&p), Some(800));
	}

	#[test]
	fn overflow_summary_max_falls_back_to_legacy() {
		let mut p = persona_minimal();
		p.memory_overflow_summary_max_completion_tokens = Some(200);
		assert_eq!(resolve_overflow_summary_max_output_tokens(&p), Some(200));
	}

	#[test]
	fn reasoning_effort_conf_serde_lowercase() {
		// toml で書く小文字表記 (`"low"` / `"medium"` / `"high"`) を受理できること。
		let low: OpenAiReasoningEffortConf = serde_json::from_str("\"low\"").unwrap();
		let medium: OpenAiReasoningEffortConf = serde_json::from_str("\"medium\"").unwrap();
		let high: OpenAiReasoningEffortConf = serde_json::from_str("\"high\"").unwrap();
		assert_eq!(low, OpenAiReasoningEffortConf::Low);
		assert_eq!(medium, OpenAiReasoningEffortConf::Medium);
		assert_eq!(high, OpenAiReasoningEffortConf::High);

		// 大文字表記は受理しない。
		let err = serde_json::from_str::<OpenAiReasoningEffortConf>("\"HIGH\"");
		assert!(err.is_err());
	}

	// ============================================================
	// ψ-α: encrypted reasoning passthrough
	// ============================================================

	#[test]
	fn terra_reasoning_history_include_respects_opt_out() {
		for enabled in [true, false] {
			let mut req = CreateResponseRequest::default();
			apply_reasoning_passthrough_include(&mut req, Some("gpt-6.1-terra"), enabled);
			assert_eq!(req.include.is_some(), enabled);
			if enabled { assert_eq!(req.include.unwrap(), vec!["reasoning.encrypted_content".to_string()]); }
		}
	}

	#[test]
	fn reasoning_encrypted_passthrough_defaults_to_true() {
		let p = persona_minimal();
		assert!(
			resolve_reasoning_encrypted_passthrough(&p),
			"未指定時は既定 true（gpt-5 系で実際に有効、非 gpt-5 は caller 側で no-op）"
		);
	}

	#[test]
	fn reasoning_encrypted_passthrough_respects_opt_out() {
		let mut p = persona_minimal();
		p.openai_reasoning_encrypted_passthrough = Some(false);
		assert!(!resolve_reasoning_encrypted_passthrough(&p), "明示的 false は尊重");
	}

	#[test]
	fn apply_reasoning_passthrough_include_noop_when_disabled() {
		let mut req = CreateResponseRequest::default();
		apply_reasoning_passthrough_include(&mut req, Some("gpt-5-mini"), false);
		assert!(req.include.is_none(), "disabled なら include を触らない");
	}

	#[test]
	fn apply_reasoning_passthrough_include_noop_for_non_gpt5() {
		let mut req = CreateResponseRequest::default();
		apply_reasoning_passthrough_include(&mut req, Some("gpt-4o-mini"), true);
		assert!(req.include.is_none(), "非 gpt-5 モデルでは no-op");
	}

	#[test]
	fn apply_reasoning_passthrough_include_sets_for_gpt5() {
		let mut req = CreateResponseRequest::default();
		apply_reasoning_passthrough_include(&mut req, Some("gpt-5-mini"), true);
		assert_eq!(
			req.include.as_ref().map(|v| v.as_slice()),
			Some(&["reasoning.encrypted_content".to_string()][..])
		);
	}

	#[test]
	fn apply_reasoning_passthrough_include_dedupes() {
		let mut req = CreateResponseRequest {
			include: Some(vec!["reasoning.encrypted_content".to_string()]),
			..Default::default()
		};
		apply_reasoning_passthrough_include(&mut req, Some("gpt-5-mini"), true);
		assert_eq!(req.include.as_ref().unwrap().len(), 1, "既に入っていれば重複 append しない");
	}

	#[test]
	fn apply_reasoning_passthrough_include_handles_unknown_model() {
		let mut req = CreateResponseRequest::default();
		apply_reasoning_passthrough_include(&mut req, None, true);
		assert!(req.include.is_none(), "model 未指定でも no-op（safe default）");
	}

	#[test]
	fn collect_reasoning_input_items_preserves_order_and_flags_missing_blobs() {
		use crate::ai::openai_responses::types::{MessageContent, OutputItem};
		let output = vec![
			OutputItem::Reasoning {
				id: "rs_a".into(),
				status: None,
				summary: Some(serde_json::json!([])),
				encrypted_content: Some("blob_a".into()),
			},
			// 間に Message が挟まっていても Reasoning だけを抽出する。
			OutputItem::Message {
				id: "msg_a".into(),
				status: None,
				role: "assistant".into(),
				content: vec![MessageContent::OutputText {
					text: "hi".into(),
					annotations: None,
				}],
			},
			OutputItem::Reasoning {
				id: "rs_b".into(),
				status: None,
				summary: None,
				encrypted_content: None, // blob 欠落
			},
		];
		let (items, missing) = collect_reasoning_input_items(&output);
		assert_eq!(items.len(), 2);
		assert_eq!(missing, 1, "blob 欠落は 1 件");
		match &items[0] {
			InputItem::Reasoning { id, encrypted_content, .. } => {
				assert_eq!(id, "rs_a");
				assert_eq!(encrypted_content.as_deref(), Some("blob_a"));
			}
			_ => panic!("[0] must be Reasoning"),
		}
		match &items[1] {
			InputItem::Reasoning { id, encrypted_content, .. } => {
				assert_eq!(id, "rs_b");
				assert!(encrypted_content.is_none(), "blob は None で保持");
			}
			_ => panic!("[1] must be Reasoning"),
		}
	}

	#[test]
	fn collect_reasoning_input_items_returns_empty_when_no_reasoning() {
		use crate::ai::openai_responses::types::{MessageContent, OutputItem};
		let output = vec![OutputItem::Message {
			id: "msg_only".into(),
			status: None,
			role: "assistant".into(),
			content: vec![MessageContent::OutputText {
				text: "hi".into(),
				annotations: None,
			}],
		}];
		let (items, missing) = collect_reasoning_input_items(&output);
		assert!(items.is_empty());
		assert_eq!(missing, 0);
	}
}
