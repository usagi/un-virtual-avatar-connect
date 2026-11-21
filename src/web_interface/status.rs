use crate::{resource::CONTENT_TYPE_TEXT_HTML, Result};
use actix_web::{get, HttpResponse, Responder};

const CONTENT_HEAD: &str = r#"<!DOCTYPE html>
<meta charset="utf-8">
<title>VAC/status</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Kiwi+Maru:wght@500&display=swap" rel="stylesheet">
<link rel="stylesheet" href="/resources/css/common/dark-mode.css">
"#;

#[get("/status")]
async fn get() -> Result<impl Responder> {
	log::trace!("/status");

	let mut content = CONTENT_HEAD.to_string();

	{
		content.push_str("<h1>Virtual Avatar Connect</h1>\n<ul>\n");
		let vac_version = env!("CARGO_PKG_VERSION");
		content.push_str(&format!("<li>VAC Version: {}</li>\n", vac_version));
		// 実行環境のOS情報を取得
		let os_info = os_info::get();
		content.push_str(&format!("<li>OS: {}</li>\n", os_info));
		content.push_str("</ul>\n<hr>\n");
	}

	match make_os_tts_section().await {
		Ok(section) => content.push_str(&section),
		Err(e) => {
			log::error!("OS-TTS の情報を取得できませんでした。: {}", e);
			content.push_str("<section><h2>OS-TTS</h2><p>OS-TTS の情報を取得できませんでした。</p></section>");
		}
	}

	// External TTS engines are now configured and observed through FlowGraph nodes.
	content.push_str(
		"<section><h2>TTS Engines</h2>\
   <p>外部 TTS engine の接続と実行は FlowGraph の TTS ノードで扱います。\
   現在の構成は FlowGraph 画面、設定ファイル、各 TTS tutorial を確認してください。</p></section><hr>\n",
	);

	Ok(HttpResponse::Ok().content_type(CONTENT_TYPE_TEXT_HTML).body(content))
}

fn make_section(title: &str, content: &str) -> String {
	let mut section = String::new();
	section.push_str("<section>\n");
	section.push_str(&format!("<h2>{}</h2>\n", title));
	section.push_str(content);
	section.push_str("</section>\n<hr>\n");
	section
}

fn make_tr_td(vs: Vec<String>) -> String {
	let mut tr = String::new();
	tr.push_str("<tr>");
	for v in vs {
		tr.push_str(&format!("<td>{}</td>", v));
	}
	tr.push_str("</tr>");
	tr
}

fn make_tr_th(vs: Vec<String>) -> String {
	let mut tr = String::new();
	tr.push_str("<tr>");
	for v in vs {
		tr.push_str(&format!("<th style=\"padding-right: 1em; text-align: left\">{}</th>", v));
	}
	tr.push_str("</tr>");
	tr
}

async fn make_os_tts_section() -> Result<String> {
	let mut section_content = "".to_string();

	let tts = tts::Tts::default()?;
	let voices = tts.voices()?;
	let trs = voices
		.into_iter()
		.map(|v| {
			let lang = v.language().to_string();
			let name = v.name().to_string();
			let id = v.id().to_string();
			make_tr_td(vec![lang, name, id])
		})
		.collect::<Vec<_>>()
		.join("\n");

	section_content.push_str("<table>\n");
	section_content.push_str(make_tr_th(vec!["Lang".to_string(), "Name".to_string(), "ID".to_string()]).as_str());
	section_content.push_str(trs.as_str());
	section_content.push_str("</table>\n");

	Ok(make_section("《OS-TTS》", section_content.as_str()))
}
