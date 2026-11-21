//! `flowgraph.web.*`: web feed / polling utility nodes.

use crate::datetime::DateTime;
use crate::flowgraph::node::{
	get_optional_bool, get_optional_int, get_optional_string, get_required_string, EffectfulNode, ExecCtx, ExecFireSet, InputMap,
	NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec, RecordedEffect,
};
use crate::flowgraph::socket::{FlowResult, SocketType, SocketValue};
use crate::flowgraph::table::{ColumnSpec, Row, Table, TableSchema};
use async_trait::async_trait;
use feed_rs::model::{Entry, Feed, Text};
use serde_json::{json, Value as JsonValue};
use std::time::Duration;

pub struct WebRssFetchNode;

impl NodeDescriptor for WebRssFetchNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.web.rss_fetch".into(),
			title: "Web RSS Fetch".into(),
			category: "web".into(),
			description: Some("RSS / Atom / JSON Feed を取得して normalized item JSON と Table に変換する。Effectful".into()),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("url", "URL", SocketType::String),
				PortSpec::input("limit", "Limit", SocketType::Int).with_default(SocketValue::Int(20)),
				PortSpec::input("since", "Since", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("include_content", "Include Content", SocketType::Bool).with_default(SocketValue::Bool(false)),
				PortSpec::input("timeout_ms", "Timeout ms", SocketType::Int).with_default(SocketValue::Int(5000)),
			],
			outputs: vec![
				PortSpec::exec_output("on_success", "On Success"),
				PortSpec::exec_output("on_error", "On Error"),
				PortSpec::output("items_json", "Items JSON", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::output("items_table", "Items Table", SocketType::Table),
				PortSpec::output("latest_updated_at", "Latest Updated At", SocketType::String),
				PortSpec::output("error", "Error", SocketType::String),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Json))),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl EffectfulNode for WebRssFetchNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired.contains("exec_in") {
			return Ok(NodeOutput::new());
		}
		let url = get_required_string(inputs, "url")?;
		let limit = get_optional_int(inputs, "limit", 20)?.clamp(0, 500) as usize;
		let since = get_optional_string(inputs, "since", "")?;
		let include_content = get_optional_bool(inputs, "include_content", false)?;
		let timeout_ms = get_optional_int(inputs, "timeout_ms", 5000)?.clamp(100, 120_000) as u64;

		let body = if let Some(mock) = ctx
			.effect_mocks
			.as_ref()
			.and_then(|mocks| mocks.http_response(&ctx.node_id))
			.cloned()
		{
			ctx.log(format!("web.rss_fetch mock: GET {url} -> node {}", ctx.node_id));
			if let Some(error) = mock.error {
				ctx.record_effect(RecordedEffect::http_request(
					ctx.node_id.clone(),
					"GET",
					url.clone(),
					JsonValue::Null,
					None,
					None,
					Some(error.clone()),
				));
				return Ok(rss_error_output(error));
			}
			ctx.record_effect(RecordedEffect::http_request(
				ctx.node_id.clone(),
				"GET",
				url.clone(),
				JsonValue::Null,
				Some(mock.status),
				Some(mock.body_text.clone()),
				None,
			));
			if !(200..=299).contains(&mock.status) {
				return Ok(rss_error_output(format!("HTTP {}", mock.status)));
			}
			mock.body_text
		} else {
			match fetch_feed_body(&url, timeout_ms).await {
				Ok(body) => body,
				Err(error) => return Ok(rss_error_output(error)),
			}
		};

		match parse_feed_items(&body, &url, limit, &since, include_content) {
			Ok(parsed) => Ok(rss_success_output(parsed)),
			Err(error) => Ok(rss_error_output(error)),
		}
	}
}

#[derive(Debug, Clone)]
struct ParsedFeedItems {
	items: Vec<JsonValue>,
	table: Table,
	latest_updated_at: String,
}

async fn fetch_feed_body(url: &str, timeout_ms: u64) -> Result<String, String> {
	let url = url.trim();
	if url.is_empty() {
		return Err("url is empty".into());
	}
	let client = reqwest::Client::builder()
		.timeout(Duration::from_millis(timeout_ms))
		.build()
		.map_err(|e| format!("reqwest client: {e}"))?;
	let response = client.get(url).send().await.map_err(|e| format!("send failed: {e}"))?;
	let status = response.status();
	let text = response.text().await.map_err(|e| format!("read body failed: {e}"))?;
	if !status.is_success() {
		return Err(format!("HTTP {}: {}", status.as_u16(), text.chars().take(160).collect::<String>()));
	}
	Ok(text)
}

fn parse_feed_items(body: &str, source_url: &str, limit: usize, since: &str, include_content: bool) -> Result<ParsedFeedItems, String> {
	let feed = feed_rs::parser::parse(body.as_bytes()).map_err(|e| format!("parse feed: {e}"))?;
	let since = parse_since(since)?;
	let feed_title = text_content(feed.title.as_ref());
	let mut items = Vec::new();
	let mut latest: Option<DateTime> = None;

	for entry in feed.entries.iter() {
		if items.len() >= limit {
			break;
		}
		let updated_at = entry_updated_at(entry);
		let updated_dt = updated_at.as_deref().and_then(|value| DateTime::from_rfc3339(value).ok());
		if let (Some(since), Some(updated)) = (&since, &updated_dt) {
			if updated <= since {
				continue;
			}
		}
		if let Some(updated) = updated_dt {
			if latest.as_ref().map(|latest| updated > *latest).unwrap_or(true) {
				latest = Some(updated);
			}
		}
		items.push(entry_to_json(entry, &feed, &feed_title, source_url, include_content));
	}

	let table = feed_items_table(&items);
	Ok(ParsedFeedItems {
		items,
		table,
		latest_updated_at: latest.map(|dt| dt.to_rfc3339()).unwrap_or_default(),
	})
}

fn parse_since(value: &str) -> Result<Option<DateTime>, String> {
	let trimmed = value.trim();
	if trimmed.is_empty() {
		Ok(None)
	} else {
		DateTime::from_rfc3339(trimmed)
			.map(Some)
			.map_err(|e| format!("since must be RFC3339: {e}"))
	}
}

fn entry_to_json(entry: &Entry, feed: &Feed, feed_title: &str, source_url: &str, include_content: bool) -> JsonValue {
	let content = if include_content {
		entry.content.as_ref().and_then(|content| content.body.clone()).unwrap_or_default()
	} else {
		String::new()
	};
	json!({
		"id": entry.id,
		"title": text_content(entry.title.as_ref()),
		"link": primary_link(entry),
		"updated_at": entry_updated_at(entry).unwrap_or_default(),
		"published_at": entry.published.map(|dt| dt.to_rfc3339()).unwrap_or_default(),
		"summary": text_content(entry.summary.as_ref()),
		"content": content,
		"authors": entry.authors.iter().map(|author| author.name.clone()).collect::<Vec<_>>(),
		"categories": entry.categories.iter().map(|category| category.term.clone()).collect::<Vec<_>>(),
		"feed_id": feed.id,
		"feed_title": feed_title,
		"source_url": source_url,
	})
}

fn text_content(value: Option<&Text>) -> String {
	value.map(|text| text.content.clone()).unwrap_or_default()
}

fn primary_link(entry: &Entry) -> String {
	entry
		.links
		.iter()
		.find(|link| link.rel.as_deref().unwrap_or("alternate") == "alternate")
		.or_else(|| entry.links.first())
		.map(|link| link.href.clone())
		.unwrap_or_default()
}

fn entry_updated_at(entry: &Entry) -> Option<String> {
	entry.updated.or(entry.published).map(|dt| dt.to_rfc3339())
}

fn feed_items_table(items: &[JsonValue]) -> Table {
	let schema = TableSchema::new(vec![
		ColumnSpec::new("id", SocketType::String),
		ColumnSpec::new("title", SocketType::String),
		ColumnSpec::new("link", SocketType::String),
		ColumnSpec::new("updated_at", SocketType::String),
		ColumnSpec::new("published_at", SocketType::String),
		ColumnSpec::new("summary", SocketType::String),
		ColumnSpec::new("content", SocketType::String),
		ColumnSpec::new("authors", SocketType::Json),
		ColumnSpec::new("categories", SocketType::Json),
		ColumnSpec::new("feed_title", SocketType::String),
		ColumnSpec::new("source_url", SocketType::String),
	]);
	let rows = items
		.iter()
		.map(|item| {
			Row::new(vec![
				item["id"].clone(),
				item["title"].clone(),
				item["link"].clone(),
				item["updated_at"].clone(),
				item["published_at"].clone(),
				item["summary"].clone(),
				item["content"].clone(),
				item["authors"].clone(),
				item["categories"].clone(),
				item["feed_title"].clone(),
				item["source_url"].clone(),
			])
		})
		.collect();
	Table::new(schema, rows)
}

fn rss_success_output(parsed: ParsedFeedItems) -> NodeOutput {
	let list = parsed.items.iter().cloned().map(SocketValue::Json).collect::<Vec<_>>();
	NodeOutput::new()
		.set_data("items_json", SocketValue::List(list))
		.set_data("items_table", SocketValue::Table(parsed.table))
		.set_data("latest_updated_at", SocketValue::String(parsed.latest_updated_at))
		.set_data("error", SocketValue::String(String::new()))
		.set_data(
			"result",
			SocketValue::Result(FlowResult::ok(SocketValue::Json(JsonValue::Array(parsed.items)))),
		)
		.fire_exec("on_success")
}

fn rss_error_output(error: impl Into<String>) -> NodeOutput {
	let error = error.into();
	NodeOutput::new()
		.set_data("items_json", SocketValue::List(Vec::new()))
		.set_data("items_table", SocketValue::Table(Table::empty()))
		.set_data("latest_updated_at", SocketValue::String(String::new()))
		.set_data("error", SocketValue::String(error.clone()))
		.set_data("result", SocketValue::Result(FlowResult::err(error).with_code("web.rss_fetch")))
		.fire_exec("on_error")
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::node::{EffectMocks, HttpMockResponse};
	use std::sync::Arc;

	fn fired() -> ExecFireSet {
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		fired
	}

	#[test]
	fn parse_rss_feed_normalizes_items_and_table() {
		let body = r#"<?xml version="1.0" encoding="UTF-8" ?>
<rss version="2.0">
<channel>
<title>VAC News</title>
<link>https://example.test/</link>
<description>News</description>
<item>
<guid>1</guid>
<title>First</title>
<link>https://example.test/1</link>
<pubDate>Fri, 24 Apr 2026 12:00:00 GMT</pubDate>
<description>Hello</description>
</item>
</channel>
</rss>"#;
		let parsed = parse_feed_items(body, "https://example.test/rss.xml", 20, "", false).unwrap();
		assert_eq!(parsed.items.len(), 1);
		assert_eq!(parsed.items[0]["title"].as_str(), Some("First"));
		assert_eq!(parsed.items[0]["feed_title"].as_str(), Some("VAC News"));
		assert_eq!(parsed.items[0]["content"].as_str(), Some(""));
		assert_eq!(parsed.table.len(), 1);
		assert_eq!(parsed.latest_updated_at, "2026-04-24T12:00:00Z");
	}

	#[test]
	fn parse_atom_feed_respects_since_limit_and_content_flag() {
		let body = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
<title>Atom Feed</title>
<id>urn:feed</id>
<updated>2026-04-24T12:00:00Z</updated>
<entry>
<title>Old</title><id>urn:old</id><updated>2026-04-24T11:00:00Z</updated><content>old body</content>
</entry>
<entry>
<title>New</title><id>urn:new</id><updated>2026-04-24T13:00:00Z</updated><content>new body</content>
</entry>
</feed>"#;
		let parsed = parse_feed_items(body, "https://example.test/atom.xml", 1, "2026-04-24T12:00:00Z", true).unwrap();
		assert_eq!(parsed.items.len(), 1);
		assert_eq!(parsed.items[0]["title"].as_str(), Some("New"));
		assert_eq!(parsed.items[0]["content"].as_str(), Some("new body"));
		assert_eq!(parsed.latest_updated_at, "2026-04-24T13:00:00Z");
	}

	#[tokio::test]
	async fn rss_fetch_mock_returns_success_contract_and_records_http_effect() {
		let mut mocks = EffectMocks::default();
		mocks.http.insert(
			"rss".into(),
			HttpMockResponse {
				status: 200,
				body_text: r#"<?xml version="1.0" encoding="UTF-8" ?>
<rss version="2.0"><channel><title>VAC</title><item><guid>1</guid><title>One</title><pubDate>Fri, 24 Apr 2026 12:00:00 GMT</pubDate></item></channel></rss>"#
					.into(),
				body_json: JsonValue::Null,
				error: None,
			},
		);
		let mut ctx = ExecCtx {
			node_id: "rss".into(),
			effect_mocks: Some(Arc::new(mocks)),
			..Default::default()
		};
		let inputs: InputMap = [("url".into(), SocketValue::String("https://example.test/rss.xml".into()))]
			.into_iter()
			.collect();

		let out = WebRssFetchNode
			.execute(&mut ctx, &InputMap::new(), &inputs, &fired())
			.await
			.unwrap();
		assert!(out.fired_exec.contains("on_success"));
		assert_eq!(
			out.data.get("latest_updated_at"),
			Some(&SocketValue::String("2026-04-24T12:00:00Z".into()))
		);
		assert_eq!(out.data.get("items_table").unwrap().as_table().unwrap().len(), 1);
		assert_eq!(ctx.recorded_effects.len(), 1);
		assert_eq!(ctx.recorded_effects[0].kind, "http");
	}
}
