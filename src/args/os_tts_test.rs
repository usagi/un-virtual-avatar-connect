//! Restore the explicit CLI voice audition without requiring a profile or V1 processors.
use crate::SharedAudioSink;
use anyhow::{Context, Result};

pub(super) async fn run(audio_sink: &SharedAudioSink) -> Result<()> {
	let mut tts = tts::Tts::default().context("OS音声の初期化に失敗")?;
	let voices = tts.voices().context("OS音声一覧の取得に失敗")?;
	anyhow::ensure!(!voices.is_empty(), "利用できるOS音声がありません");
	println!("--- OS-TTS Voices ---");
	println!("rate: min={} normal={} max={}", tts.min_rate(), tts.normal_rate(), tts.max_rate());
	println!(
		"pitch: min={} normal={} max={}",
		tts.min_pitch(),
		tts.normal_pitch(),
		tts.max_pitch()
	);
	println!(
		"volume: min={} normal={} max={}",
		tts.min_volume(),
		tts.normal_volume(),
		tts.max_volume()
	);
	for voice in &voices {
		println!("Lang: {}  Name: {}  ID: {}", voice.language(), voice.name(), voice.id());
	}
	for voice in voices {
		let text = sample_text(&voice.language().to_string(), &voice.name());
		tts.set_voice(&voice).context("試聴するOS音声の選択に失敗")?;
		#[cfg(target_os = "windows")]
		{
			let wav = tts.synthesize(text).context("OS音声の合成に失敗")?;
			crate::flowgraph::tts::driver::AudioContext { sink: Some(audio_sink) }
				.play_wav(wav)
				.await?;
			while !audio_sink.lock().await.player.empty() {
				tokio::time::sleep(std::time::Duration::from_millis(50)).await;
			}
		}
		#[cfg(not(target_os = "windows"))]
		{
			let _ = audio_sink;
			tts.speak(text, false).context("OS音声の再生に失敗")?;
			while tts.is_speaking()? {
				tokio::time::sleep(std::time::Duration::from_millis(50)).await;
			}
		}
	}
	println!("OS-TTS のテストが完了しました。");
	Ok(())
}

// Retain the language-specific audition phrases from the former OsTts::test.
fn sample_text(language: &str, name: &str) -> String {
	match language.to_lowercase().as_str() {
		// ChatGPTに使用人口の多そうな順に30種類作ってもらった。これ以上は要望があってから考える。
		"zh-cn" => format!("我是{}，如果您想要这样的语音，请选择我。你想吃点心吗？", name),
		"zh-tw" => format!("我是{}，如果您想要這樣的語音，請選擇我。你想吃鳳梨酥嗎？", name),
		es if es.starts_with("es") => format!("Soy {} y si deseas esta voz, elige la mía. ¿Quieres comer tortillas?", name),
		en if en.starts_with("en") => format!("I am {}, if you want this voice, choose me. Do you want to eat meat pie?", name),
		"hi-in" => format!("मैं हूं {} और अगर आपको यह आवाज़ चाहिए तो मेरा चयन करें। क्या आप मसाला चाय पीते हैं?", name),
		"ar-sa" => format!("أنا {}، إذا كنت ترغب في هذا الصوت، اخترني. هل تريد أن تأكل الحمص؟", name),
		"bn-bd" => format!("আমি {} এবং আপনি যদি এই ভয়েসটি চান তবে আমাকে চয়ন করুন।", name),
		"ru-ru" => format!("Я {} и если вам нужен такой голос, выберите меня. Вы пьете водку?", name),
		"pt-br" => format!("Eu sou {} e se você quiser esta voz, escolha-me.", name),
		"ja-jp" => format!("私は{}、このような音声を使いたい場合は選んでくれ。お寿司食べる？", name),
		"pa-in" => format!("ਮੈਂ {} ਹਾਂ ਅਤੇ ਜੇਕਰ ਤੁਸੀਂ ਇਸ ਆਵਾਜ਼ ਦੀ ਇਚਛਾ ਕਰਦੇ ਹੋ ਤਾਂ ਮੈਨੂੰ ਚੁਣੋ।", name),
		"de-de" => format!(
			"Ich bin {} und wenn du diese Stimme möchtest, wähle mich. Sollte Ihr Bier Schwartz sein?",
			name
		),
		"fr-fr" => format!(
			"Je suis {} et si vous voulez cette voix, choisissez-moi. Tu veux manger de la galette ?",
			name
		),
		"ko-kr" => format!("나는 {} 이며, 이 음성을 원하면 나를 선택하십시오. 순두부 먹어?", name),
		"tr-tr" => format!("Ben {}'yim ve bu sesi istersen beni seç.", name),
		"it-it" => format!("Sono {} e se vuoi questa voce, scegli me. Che tipo di pasta utilizzare?", name),
		"uk-ua" => format!("Я {} і якщо вам потрібен саме такий голос, оберіть мене. Хочеш їсти борщ?", name),
		"vi-vn" => format!("Tôi là {} và nếu bạn muốn giọng nói này, hãy chọn tôi.", name),
		"fa-ir" => format!("من {} هستم و اگر می‌خواهید این صدا را داشته باشید، من را انتخاب کنید.", name),
		"th-th" => format!("ฉันคือ {} และหากคุณต้องการเสียงนี้ โปรดเลือกฉัน. อยากกินต้มยำกุ้งมั้ย?", name),
		"pl-pl" => format!("Jestem {} i jeśli chcesz ten głos, wybierz mnie.", name),
		"ro-ro" => format!("Sunt {} și dacă vrei această voce, alege-mă.", name),
		"nl-nl" => format!("Ik ben {} en als je deze stem wilt, kies dan voor mij.", name),
		"el-gr" => format!("Είμαι {} και αν θέλετε αυτήν τη φωνή, επιλέξτε με.", name),
		"hu-hu" => format!("Én vagyok {} és ha ezt a hangot szeretnéd, válassz engem.", name),
		"cs-cz" => format!("Jsem {} a pokud chcete tento hlas, vyberte si mě.", name),
		"sv-se" => format!("Jag är {} och om du vill ha den här rösten, välj mig.", name),
		"pt-pt" => format!("Sou {} e se quiseres esta voz, escolhe-me.", name),
		"id-id" => format!("Saya {} dan jika Anda ingin suara ini, pilih saya.", name),
		"ms-my" => format!("Saya {} dan jika anda mahu suara ini, pilih saya.", name),
		lang => {
			log::warn!("未知の言語コードです: {}", lang);
			name.to_string()
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn audition_has_text_for_known_and_unlisted_languages() {
		for language in ["ja-JP", "en-US", "zh-CN", "fr-FR", "unknown"] {
			let text = sample_text(language, "Test Voice");
			assert!(text.contains("Test Voice"));
			assert!(!text.is_empty());
		}
	}
}
