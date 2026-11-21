//! Preserve supported legacy endpoint settings rather than silently changing its URL.
use super::MigrationReport;
use toml_edit::{DocumentMut, Table, value};

pub(super) fn preserve_endpoint(source: &str, old: &Table, report: &mut MigrationReport) -> Result<String, String> {
    let field = |key: &str, default: &str| -> Result<String, String> {
        match old.get(key) {
            None => Ok(default.into()),
            Some(item) => item.as_str().map(str::to_owned).ok_or_else(||format!("{key} は文字列が必要です")),
        }
    };
    let path = field("web_input_path", "/input")?;
    // An explicitly empty legacy path means root, whereas an empty Flowgraph
    // path requests automatic naming. Resolve the old meaning before writing.
    let path = if path.trim().starts_with('/') { path.trim().to_owned() } else { format!("/{}", path.trim()) };
    let path = path.trim_end_matches('/');
    let path = if path.is_empty() { "/" } else { path };
    // Match the legacy parser: only GET selects GET, everything else is POST.
    let method = if field("web_input_method", "POST")?.trim().eq_ignore_ascii_case("get") { "GET" } else { "POST" };
    let format = field("web_input_body_format", "json")?.trim().to_ascii_lowercase();
    let format = match format.as_str() {
        "toml" => "toml",
        "msgpack" | "messagepack" => "msgpack",
        _ => "json", // Same fallback as the legacy registry.
    };
    let channel = field("channel_to", "")?;
    if channel.trim().is_empty() {
        report.warn(Some("web-input".into()), "本文のchannelによる旧動的振り分けは移行していません。生成した固定のFlowgraph接続を確認してください。");
    }
    let mut graph: DocumentMut = source.parse().map_err(|e|format!("{e}"))?;
    let node = graph["nodes"].as_array_of_tables_mut().and_then(|nodes|nodes.iter_mut().find(|n|n.get("id").and_then(|v|v.as_str())==Some("in")))
        .ok_or("Web入力ノードがありません")?;
    node["properties"]["path"] = value(path);
    node["properties"]["method"] = value(method);
    node["properties"]["body_format"] = value(format);
    node["properties"]["fixed_channel"] = value(channel.trim());
    Ok(graph.to_string())
}
