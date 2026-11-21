//! `flowgraph.table.*`: 汎用 Table 型の I/O と JSON 相互変換（η-2）。
//!
//! 辞書以外の汎用ユースケース（scene registry / Twitch user list / moderation log 等）にも効く
//! primitive 操作ノード群。辞書セマンティクスを知らない純粋な表操作。
//!
//! ## 含まれるノード
//! - `flowgraph.table.from_json` (Pure): `List<Json>` → `Table`（スキーマは先頭 object から推論）
//! - `flowgraph.table.to_json` (Pure): `Table` → `List<Json>`
//! - `flowgraph.table.load_tsv` (Effectful): path → Table（auto / google_ime / headerful / legacy_loose モード）
//! - `flowgraph.table.write_tsv` (Effectful): Table + path → exec（atomic rename）

use crate::flowgraph::node::{
	get_optional_bool, get_optional_int, get_optional_string, get_required_list, get_required_string, EffectfulNode, ExecCtx, ExecFireSet,
	InputMap, NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec, PureNode, RecordedEffect,
};
use crate::flowgraph::socket::{FlowResult, SocketType, SocketValue};
use crate::flowgraph::table::{ColumnSpec, Row, Table, TableSchema};
use async_trait::async_trait;
use base64::Engine as _;
use calamine::{Data, Reader};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------
// 共通: SocketValue → JsonValue ヘルパ（ここでも必要）
// ---------------------------------------------------------------------

fn sv_to_json(v: &SocketValue) -> JsonValue {
	match v {
		SocketValue::Bool(b) => JsonValue::Bool(*b),
		SocketValue::Int(i) => JsonValue::Number((*i).into()),
		SocketValue::Float(f) => serde_json::Number::from_f64(*f).map(JsonValue::Number).unwrap_or(JsonValue::Null),
		SocketValue::String(s) => JsonValue::String(s.clone()),
		SocketValue::Bytes(bytes) => JsonValue::String(base64::engine::general_purpose::STANDARD.encode(bytes)),
		SocketValue::Json(j) => j.clone(),
		SocketValue::List(xs) => JsonValue::Array(xs.iter().map(sv_to_json).collect()),
		SocketValue::Map(m) => JsonValue::Object(m.iter().map(|(k, v)| (k.clone(), sv_to_json(v))).collect()),
		SocketValue::Option(value) => value.as_ref().map(|value| sv_to_json(value)).unwrap_or(JsonValue::Null),
		SocketValue::Result(result) => {
			let mut obj = serde_json::Map::new();
			obj.insert("ok".into(), JsonValue::Bool(result.ok));
			if let Some(value) = result.value.as_ref() {
				obj.insert("value".into(), sv_to_json(value));
			}
			if let Some(error) = result.error.as_ref() {
				obj.insert("error".into(), JsonValue::String(error.clone()));
			}
			if let Some(code) = result.code.as_ref() {
				obj.insert("code".into(), JsonValue::String(code.clone()));
			}
			JsonValue::Object(obj)
		}
		SocketValue::Table(t) => t.to_json_array(),
		// Phase ξ §6.4: table 化境界は value のみ（pass-through）。
		SocketValue::Quantity(q) => serde_json::Number::from_f64(q.value)
			.map(JsonValue::Number)
			.unwrap_or(JsonValue::Null),
		// Phase π: Table セル内 DateTime は RFC3339 (Z suffix) 文字列として格納。
		SocketValue::DateTime(dt) => JsonValue::String(dt.to_rfc3339()),
		SocketValue::MotionFrame(m) => m.to_json_value(),
		SocketValue::Unmf(frame) => vac_motion::unmf::unmf_to_json_value(frame),
	}
}

// ---------------------------------------------------------------------
// flowgraph.table.from_json
// ---------------------------------------------------------------------

pub struct TableFromJsonNode;

impl NodeDescriptor for TableFromJsonNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.table.from_json".into(),
			title: "Table From JSON".into(),
			category: "table".into(),
			description: Some("List<Json> (object の配列) を Table に変換。スキーマは先頭 object から推論".into()),
			inputs: vec![
				PortSpec::input("json", "JSON", SocketType::List(Box::new(SocketType::Json))).with_default(SocketValue::List(vec![]))
			],
			outputs: vec![
				PortSpec::output("table", "Table", SocketType::Table),
				PortSpec::output("row_count", "Row Count", SocketType::Int),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for TableFromJsonNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let list = get_required_list(inputs, "json")?;
		let arr: Vec<JsonValue> = list.iter().map(sv_to_json).collect();
		let table = Table::from_json_array(&arr, None).unwrap_or_else(|_| Table::empty());
		let count = table.len() as i64;
		Ok(NodeOutput::new()
			.set_data("table", SocketValue::Table(table))
			.set_data("row_count", SocketValue::Int(count)))
	}
}

// ---------------------------------------------------------------------
// flowgraph.table.to_json
// ---------------------------------------------------------------------

pub struct TableToJsonNode;

/// Ordered concatenation without dictionary-specific interpretation.
pub struct TableConcatNode;

impl NodeDescriptor for TableConcatNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.table.concat".into(),
			title: "Table Concat".into(),
			category: "table".into(),
			description: Some("同じ列定義の表を先頭・末尾の順に連結。重複行と列の型を保持し、異なる列定義は拒否する。".into()),
			inputs: vec![PortSpec::input("first", "First", SocketType::Table), PortSpec::input("second", "Second", SocketType::Table)],
			outputs: vec![PortSpec::output("table", "Table", SocketType::Table)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for TableConcatNode {
	async fn compute(&self, _host: &crate::flowgraph::node::PureEvalHost, _props: &InputMap,
		inputs: &InputMap, _fired: &ExecFireSet) -> Result<NodeOutput, NodeExecError> {
		let first = get_required_table(inputs, "first")?;
		let second = get_required_table(inputs, "second")?;
		if first.schema() != second.schema() {
			return Err(NodeExecError::Generic(anyhow::anyhow!("table.concat requires identical column definitions")));
		}
		let rows = first.rows().iter().chain(second.rows()).cloned().collect();
		Ok(NodeOutput::new().set_data("table", SocketValue::Table(Table::new(first.schema().clone(), rows))))
	}
}

impl NodeDescriptor for TableToJsonNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.table.to_json".into(),
			title: "Table To JSON".into(),
			category: "table".into(),
			description: Some("Table を List<Json> (object の配列) に変換".into()),
			inputs: vec![PortSpec::input("table", "Table", SocketType::Table).with_default(SocketValue::Table(Table::empty()))],
			outputs: vec![
				PortSpec::output("json", "JSON", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::output("row_count", "Row Count", SocketType::Int),
			],
			properties: vec![],
		}
	}
}

fn get_required_table<'a>(inputs: &'a InputMap, key: &str) -> Result<&'a Table, NodeExecError> {
	let v = inputs.get(key).ok_or_else(|| NodeExecError::MissingRequiredInput(key.into()))?;
	v.as_table().map_err(|_| NodeExecError::TypeMismatch {
		port: key.into(),
		expected: SocketType::Table,
		actual: v.type_of(),
	})
}

#[async_trait]
impl PureNode for TableToJsonNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let table = get_required_table(inputs, "table")?;
		let arr = match table.to_json_array() {
			JsonValue::Array(a) => a,
			_ => Vec::new(),
		};
		let list: Vec<SocketValue> = arr.into_iter().map(SocketValue::Json).collect();
		let count = list.len() as i64;
		Ok(NodeOutput::new()
			.set_data("json", SocketValue::List(list))
			.set_data("row_count", SocketValue::Int(count)))
	}
}

// ---------------------------------------------------------------------
// flowgraph.table.draw
// ---------------------------------------------------------------------

pub struct TableDrawNode;

impl NodeDescriptor for TableDrawNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.table.draw".into(),
			title: "Table Draw".into(),
			category: "table".into(),
			description: Some("Table から行を抽選。重み列、seed、重複あり / なしを扱う".into()),
			inputs: vec![
				PortSpec::input("table", "Table", SocketType::Table).with_default(SocketValue::Table(Table::empty())),
				PortSpec::input("count", "Count", SocketType::Int).with_default(SocketValue::Int(1)),
				PortSpec::input("weight_column", "Weight Column", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("seed", "Seed", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("replacement", "Replacement", SocketType::Bool).with_default(SocketValue::Bool(false)),
			],
			outputs: vec![
				PortSpec::output("rows", "Rows", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::output("row", "Row", SocketType::Json),
				PortSpec::output("indices", "Indices", SocketType::List(Box::new(SocketType::Int))),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for TableDrawNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let table = get_required_table(inputs, "table")?;
		let requested_count = get_optional_int(inputs, "count", 1)?.max(0) as usize;
		let weight_column = get_optional_string(inputs, "weight_column", "")?;
		let seed = get_optional_string(inputs, "seed", "")?;
		let replacement = get_optional_bool(inputs, "replacement", false)?;

		let selected = draw_table_indices(table, requested_count, &weight_column, &seed, replacement);
		let rows: Vec<SocketValue> = selected
			.iter()
			.filter_map(|idx| table.rows().get(*idx))
			.map(|row| SocketValue::Json(JsonValue::Object(row.as_map(table.schema()))))
			.collect();
		let first_row = rows
			.first()
			.and_then(|value| match value {
				SocketValue::Json(value) => Some(value.clone()),
				_ => None,
			})
			.unwrap_or(JsonValue::Null);
		let indices = selected.into_iter().map(|idx| SocketValue::Int(idx as i64)).collect();

		Ok(NodeOutput::new()
			.set_data("rows", SocketValue::List(rows))
			.set_data("row", SocketValue::Json(first_row))
			.set_data("indices", SocketValue::List(indices)))
	}
}

fn draw_table_indices(table: &Table, requested_count: usize, weight_column: &str, seed: &str, replacement: bool) -> Vec<usize> {
	if requested_count == 0 || table.is_empty() {
		return Vec::new();
	}
	let count = if replacement {
		requested_count.min(10_000)
	} else {
		requested_count.min(table.len())
	};
	let mut rng = SplitMix64::new(seed_u64(seed));
	let weight_idx = (!weight_column.trim().is_empty())
		.then(|| table.schema().column_index(weight_column.trim()))
		.flatten();
	let weights = weight_idx.map(|idx| {
		table
			.rows()
			.iter()
			.map(|row| row.get(idx).map(json_to_nonnegative_weight).unwrap_or(0.0))
			.collect::<Vec<_>>()
	});
	let mut candidates: Vec<usize> = (0..table.len()).collect();
	let mut selected = Vec::with_capacity(count);

	for _ in 0..count {
		if candidates.is_empty() {
			break;
		}
		let candidate_pos = match weights.as_ref() {
			Some(weights) => choose_weighted_candidate(&mut rng, &candidates, weights),
			None => rng.next_index(candidates.len()),
		};
		let idx = candidates[candidate_pos];
		selected.push(idx);
		if !replacement {
			candidates.swap_remove(candidate_pos);
		}
	}

	selected
}

fn choose_weighted_candidate(rng: &mut SplitMix64, candidates: &[usize], weights: &[f64]) -> usize {
	let total: f64 = candidates
		.iter()
		.map(|idx| weights.get(*idx).copied().unwrap_or(0.0))
		.filter(|weight| *weight > 0.0 && weight.is_finite())
		.sum();
	if total <= 0.0 {
		return rng.next_index(candidates.len());
	}
	let mut needle = rng.next_unit_f64() * total;
	for (candidate_pos, idx) in candidates.iter().enumerate() {
		let weight = weights.get(*idx).copied().unwrap_or(0.0);
		if weight <= 0.0 || !weight.is_finite() {
			continue;
		}
		if needle < weight {
			return candidate_pos;
		}
		needle -= weight;
	}
	candidates.len().saturating_sub(1)
}

fn json_to_nonnegative_weight(value: &JsonValue) -> f64 {
	let weight = match value {
		JsonValue::Number(number) => number.as_f64().unwrap_or(0.0),
		JsonValue::String(value) => value.trim().parse::<f64>().unwrap_or(0.0),
		JsonValue::Bool(true) => 1.0,
		JsonValue::Bool(false) | JsonValue::Null | JsonValue::Array(_) | JsonValue::Object(_) => 0.0,
	};
	if weight.is_finite() && weight > 0.0 {
		weight
	} else {
		0.0
	}
}

fn seed_u64(seed: &str) -> u64 {
	if seed.is_empty() {
		let nanos = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map(|duration| duration.as_nanos() as u64)
			.unwrap_or(0);
		return nanos ^ 0x9e37_79b9_7f4a_7c15;
	}
	let hash = blake3::hash(seed.as_bytes());
	let mut bytes = [0u8; 8];
	bytes.copy_from_slice(&hash.as_bytes()[..8]);
	u64::from_le_bytes(bytes)
}

struct SplitMix64 {
	state: u64,
}

impl SplitMix64 {
	fn new(seed: u64) -> Self {
		Self { state: seed }
	}

	fn next_u64(&mut self) -> u64 {
		self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
		let mut z = self.state;
		z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
		z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
		z ^ (z >> 31)
	}

	fn next_unit_f64(&mut self) -> f64 {
		((self.next_u64() >> 11) as f64) / ((1u64 << 53) as f64)
	}

	fn next_index(&mut self, len: usize) -> usize {
		if len <= 1 {
			return 0;
		}
		(self.next_u64() as usize) % len
	}
}

// ---------------------------------------------------------------------
// flowgraph.table.load_tsv
// ---------------------------------------------------------------------

pub struct TableLoadTsvNode;

impl NodeDescriptor for TableLoadTsvNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.table.load_tsv".into(),
			title: "Table Load TSV".into(),
			category: "table".into(),
			description: Some("TSV ファイルを Table に読み込む（auto / google_ime / headerful / legacy_loose）。Effectful".into()),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("path", "Path", SocketType::String),
				PortSpec::input("mode", "Mode", SocketType::String).with_default(SocketValue::String("auto".into())),
			],
			outputs: vec![
				PortSpec::exec_output("on_success", "On Success"),
				PortSpec::exec_output("on_error", "On Error"),
				PortSpec::output("table", "Table", SocketType::Table),
				PortSpec::output("row_count", "Row Count", SocketType::Int),
				PortSpec::output("revision", "File Revision", SocketType::String),
				PortSpec::output("error", "Error", SocketType::String),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Table))),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl EffectfulNode for TableLoadTsvNode {
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
		let path = get_required_string(inputs, "path")?;
		let mode = get_optional_string(inputs, "mode", "auto")?;

		let contents = if let Some(mock) = ctx
			.effect_mocks
			.as_ref()
			.and_then(|mocks| mocks.file_read_response(&ctx.node_id))
			.cloned()
		{
			ctx.log(format!("file.read mock: {path} -> node {}", ctx.node_id));
			if let Some(error) = mock.error {
				ctx.record_effect(RecordedEffect::file_read(
					ctx.node_id.clone(),
					path.clone(),
					None,
					Some(error.clone()),
				));
				return Ok(err_output_load(format!("read_to_string {path}: {error}")));
			}
			ctx.record_effect(RecordedEffect::file_read(
				ctx.node_id.clone(),
				path.clone(),
				Some(mock.contents.clone()),
				None,
			));
			mock.contents
		} else {
			let resolved = match ctx.resolve_read_path(&path) {
				Ok(path) => path,
				Err(error) => return Ok(err_output_load(error)),
			};
			match tokio::fs::read_to_string(&resolved).await {
				Ok(s) => s,
				Err(e) => {
					return Ok(err_output_load(format!("read_to_string {path}: {e}")));
				}
			}
		};
		match parse_tsv_with_mode(&contents, &mode) {
			Ok(table) => Ok(load_success_output(table).set_data("revision", SocketValue::String(crate::file_publish::revision(contents.as_bytes())))),
			Err(e) => Ok(err_output_load(format!("parse: {e}"))),
		}
	}
}

fn err_output_load(msg: impl Into<String>) -> NodeOutput {
	let msg = msg.into();
	NodeOutput::new()
		.set_data("table", SocketValue::Table(Table::empty()))
		.set_data("row_count", SocketValue::Int(0))
		.set_data("error", SocketValue::String(msg.clone()))
		.set_data("result", SocketValue::Result(FlowResult::err(msg).with_code("table.load_tsv")))
		.fire_exec("on_error")
}

fn load_success_output(table: Table) -> NodeOutput {
	let count = table.len() as i64;
	NodeOutput::new()
		.set_data("table", SocketValue::Table(table.clone()))
		.set_data("row_count", SocketValue::Int(count))
		.set_data("error", SocketValue::String(String::new()))
		.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Table(table))))
		.fire_exec("on_success")
}

// ---------------------------------------------------------------------
// flowgraph.table.load_xlsx
// ---------------------------------------------------------------------

pub struct TableLoadXlsxNode;
pub struct TableLoadGoogleSheetPublicNode;
pub struct TableLoadGoogleSheetAuthNode;

impl NodeDescriptor for TableLoadXlsxNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.table.load_xlsx".into(),
			title: "Table Load XLSX".into(),
			category: "table".into(),
			description: Some("XLSX ファイルの sheet / range を Table に読み込む。Effectful".into()),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("path", "Path", SocketType::String),
				PortSpec::input("sheet", "Sheet", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("range", "Range", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("header_mode", "Header Mode", SocketType::String).with_default(SocketValue::String("auto".into())),
			],
			outputs: vec![
				PortSpec::exec_output("on_success", "On Success"),
				PortSpec::exec_output("on_error", "On Error"),
				PortSpec::output("table", "Table", SocketType::Table),
				PortSpec::output("sheet_name", "Sheet Name", SocketType::String),
				PortSpec::output("row_count", "Row Count", SocketType::Int),
				PortSpec::output("column_count", "Column Count", SocketType::Int),
				PortSpec::output("error", "Error", SocketType::String),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Table))),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl EffectfulNode for TableLoadXlsxNode {
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
		let path = get_required_string(inputs, "path")?;
		let sheet = get_optional_string(inputs, "sheet", "")?;
		let range = get_optional_string(inputs, "range", "")?;
		let header_mode = get_optional_string(inputs, "header_mode", "auto")?;

		if let Some(mock) = ctx
			.effect_mocks
			.as_ref()
			.and_then(|mocks| mocks.file_read_response(&ctx.node_id))
			.cloned()
		{
			ctx.log(format!("file.read mock: {path} -> node {}", ctx.node_id));
			if let Some(error) = mock.error {
				ctx.record_effect(RecordedEffect::file_read(
					ctx.node_id.clone(),
					path.clone(),
					None,
					Some(error.clone()),
				));
				return Ok(err_output_xlsx(format!("read {path}: {error}")));
			}
			ctx.record_effect(RecordedEffect::file_read(
				ctx.node_id.clone(),
				path.clone(),
				Some(mock.contents.clone()),
				None,
			));
			return match parse_xlsx_csv_fixture(&mock.contents, &header_mode) {
				Ok(table) => Ok(xlsx_success_output(table, sheet_name_or_default(&sheet, "mock"))),
				Err(e) => Ok(err_output_xlsx(format!("parse mock CSV fixture: {e}"))),
			};
		}

		let path_for_task = path.clone();
		let result = tokio::task::spawn_blocking(move || load_xlsx_table(&path_for_task, &sheet, &range, &header_mode))
			.await
			.map_err(|e| NodeExecError::Generic(anyhow::anyhow!("xlsx load task failed: {e}")))?;
		match result {
			Ok((table, sheet_name)) => Ok(xlsx_success_output(table, sheet_name)),
			Err(e) => Ok(err_output_xlsx(format!("load_xlsx {path}: {e}"))),
		}
	}
}

fn xlsx_success_output(table: Table, sheet_name: String) -> NodeOutput {
	let row_count = table.len() as i64;
	let column_count = table.schema().len() as i64;
	NodeOutput::new()
		.set_data("table", SocketValue::Table(table.clone()))
		.set_data("sheet_name", SocketValue::String(sheet_name))
		.set_data("row_count", SocketValue::Int(row_count))
		.set_data("column_count", SocketValue::Int(column_count))
		.set_data("error", SocketValue::String(String::new()))
		.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Table(table))))
		.fire_exec("on_success")
}

fn err_output_xlsx(msg: impl Into<String>) -> NodeOutput {
	let msg = msg.into();
	NodeOutput::new()
		.set_data("table", SocketValue::Table(Table::empty()))
		.set_data("sheet_name", SocketValue::String(String::new()))
		.set_data("row_count", SocketValue::Int(0))
		.set_data("column_count", SocketValue::Int(0))
		.set_data("error", SocketValue::String(msg.clone()))
		.set_data("result", SocketValue::Result(FlowResult::err(msg).with_code("table.load_xlsx")))
		.fire_exec("on_error")
}

fn load_xlsx_table(path: &str, sheet: &str, range_spec: &str, header_mode: &str) -> Result<(Table, String), String> {
	let mut workbook = calamine::open_workbook_auto(path).map_err(|e| e.to_string())?;
	let sheet_name = if sheet.trim().is_empty() {
		workbook
			.sheet_names()
			.first()
			.cloned()
			.ok_or_else(|| "workbook has no sheets".to_string())?
	} else {
		sheet.trim().to_string()
	};
	let range = workbook
		.worksheet_range(&sheet_name)
		.map_err(|e| format!("worksheet {sheet_name:?}: {e}"))?;
	let cells = cells_from_calamine_range(&range);
	let sliced = slice_cells(&cells, range_spec)?;
	let table = table_from_grid(&sliced, header_mode)?;
	Ok((table, sheet_name))
}

fn cells_from_calamine_range(range: &calamine::Range<Data>) -> Vec<Vec<JsonValue>> {
	range
		.rows()
		.map(|row| row.iter().map(calamine_data_to_json).collect::<Vec<_>>())
		.collect()
}

fn calamine_data_to_json(cell: &Data) -> JsonValue {
	match cell {
		Data::Empty => JsonValue::Null,
		Data::String(s) => JsonValue::String(s.clone()),
		Data::Float(f) => serde_json::Number::from_f64(*f).map(JsonValue::Number).unwrap_or(JsonValue::Null),
		Data::Int(i) => JsonValue::Number((*i).into()),
		Data::Bool(b) => JsonValue::Bool(*b),
		Data::Error(e) => JsonValue::String(format!("{e:?}")),
		Data::DateTime(dt) => JsonValue::String(dt.to_string()),
		Data::DateTimeIso(s) | Data::DurationIso(s) => JsonValue::String(s.clone()),
	}
}

fn slice_cells(cells: &[Vec<JsonValue>], range_spec: &str) -> Result<Vec<Vec<JsonValue>>, String> {
	let Some((start_row, start_col, end_row, end_col)) = parse_a1_range(range_spec)? else {
		return Ok(trim_grid(cells.to_vec()));
	};
	let mut out = Vec::new();
	for row_idx in start_row..=end_row {
		let mut row = Vec::new();
		for col_idx in start_col..=end_col {
			row.push(cells.get(row_idx).and_then(|r| r.get(col_idx)).cloned().unwrap_or(JsonValue::Null));
		}
		out.push(row);
	}
	Ok(trim_grid(out))
}

fn table_from_grid(cells: &[Vec<JsonValue>], header_mode: &str) -> Result<Table, String> {
	let cells = trim_grid(cells.to_vec());
	if cells.is_empty() {
		return Ok(Table::empty());
	}
	let header_mode = match header_mode.trim().to_ascii_lowercase().as_str() {
		"none" | "no_header" | "false" => "none",
		"first_row" | "header" | "headerful" | "true" => "first_row",
		_ => "auto",
	};
	let use_header = header_mode == "first_row" || (header_mode == "auto" && row_looks_like_header(&cells[0]));
	let width = cells.iter().map(Vec::len).max().unwrap_or(0);
	let (names, data_start) = if use_header {
		(
			normalize_column_names(cells.first().into_iter().flatten().map(json_cell_to_name).collect()),
			1,
		)
	} else {
		((0..width).map(|idx| format!("column{}", idx + 1)).collect(), 0)
	};
	let schema = TableSchema::new(
		names
			.into_iter()
			.map(|name| ColumnSpec::new(name, SocketType::Json).nullable())
			.collect(),
	);
	let mut rows = Vec::new();
	for row in cells.iter().skip(data_start) {
		let mut values = Vec::with_capacity(width);
		for idx in 0..width {
			values.push(row.get(idx).cloned().unwrap_or(JsonValue::Null));
		}
		rows.push(Row::new(values));
	}
	Ok(Table::new(schema, rows))
}

fn row_looks_like_header(row: &[JsonValue]) -> bool {
	!row.is_empty()
		&& row
			.iter()
			.all(|value| matches!(value, JsonValue::String(s) if !s.trim().is_empty()))
		&& {
			let non_empty = row.iter().filter(|value| json_cell_to_name(value).is_some()).count();
			non_empty > 0
		}
}

fn json_cell_to_name(value: &JsonValue) -> Option<String> {
	match value {
		JsonValue::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
		JsonValue::Number(n) => Some(n.to_string()),
		JsonValue::Bool(b) => Some(b.to_string()),
		_ => None,
	}
}

fn normalize_column_names(raw: Vec<Option<String>>) -> Vec<String> {
	let mut seen: HashMap<String, usize> = HashMap::new();
	raw.into_iter()
		.enumerate()
		.map(|(idx, name)| {
			let base = name
				.map(|s| sanitize_column_name(&s))
				.filter(|s| !s.is_empty())
				.unwrap_or_else(|| format!("column{}", idx + 1));
			let count = seen.entry(base.clone()).or_insert(0);
			*count += 1;
			if *count == 1 {
				base
			} else {
				format!("{base}_{}", *count)
			}
		})
		.collect()
}

fn sanitize_column_name(value: &str) -> String {
	let mut out = String::new();
	for ch in value.trim().chars() {
		if ch.is_ascii_alphanumeric() || ch == '_' {
			out.push(ch);
		} else if ch.is_whitespace() || ch == '-' || ch == '.' {
			out.push('_');
		}
	}
	let out = out.trim_matches('_').to_string();
	if out.is_empty() {
		String::new()
	} else if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
		format!("col_{out}")
	} else {
		out
	}
}

fn trim_grid(mut cells: Vec<Vec<JsonValue>>) -> Vec<Vec<JsonValue>> {
	while cells.last().is_some_and(|row| row.iter().all(json_cell_is_empty)) {
		cells.pop();
	}
	let width = cells
		.iter()
		.map(|row| {
			row.iter()
				.rposition(|cell| !json_cell_is_empty(cell))
				.map(|idx| idx + 1)
				.unwrap_or(0)
		})
		.max()
		.unwrap_or(0);
	for row in &mut cells {
		row.truncate(width);
	}
	cells
}

fn json_cell_is_empty(value: &JsonValue) -> bool {
	matches!(value, JsonValue::Null) || matches!(value, JsonValue::String(s) if s.is_empty())
}

fn parse_a1_range(input: &str) -> Result<Option<(usize, usize, usize, usize)>, String> {
	let trimmed = input.trim();
	if trimmed.is_empty() {
		return Ok(None);
	}
	let mut parts = trimmed.split(':');
	let start = parts.next().unwrap_or_default();
	let end = parts.next().unwrap_or(start);
	if parts.next().is_some() {
		return Err(format!("invalid A1 range {trimmed:?}"));
	}
	let (start_row, start_col) = parse_a1_cell(start)?;
	let (end_row, end_col) = parse_a1_cell(end)?;
	if end_row < start_row || end_col < start_col {
		return Err(format!("invalid A1 range order {trimmed:?}"));
	}
	Ok(Some((start_row, start_col, end_row, end_col)))
}

fn parse_a1_cell(input: &str) -> Result<(usize, usize), String> {
	let input = input.trim();
	let mut col = 0usize;
	let mut row = String::new();
	for ch in input.chars() {
		if ch.is_ascii_alphabetic() {
			if !row.is_empty() {
				return Err(format!("invalid A1 cell {input:?}"));
			}
			col = col * 26 + ((ch.to_ascii_uppercase() as u8 - b'A') as usize + 1);
		} else if ch.is_ascii_digit() {
			row.push(ch);
		} else {
			return Err(format!("invalid A1 cell {input:?}"));
		}
	}
	if col == 0 || row.is_empty() {
		return Err(format!("invalid A1 cell {input:?}"));
	}
	let row = row.parse::<usize>().map_err(|e| format!("invalid A1 row {input:?}: {e}"))?;
	if row == 0 {
		return Err(format!("invalid A1 row {input:?}"));
	}
	Ok((row - 1, col - 1))
}

fn sheet_name_or_default(sheet: &str, default: &str) -> String {
	if sheet.trim().is_empty() {
		default.into()
	} else {
		sheet.trim().into()
	}
}

fn parse_xlsx_csv_fixture(contents: &str, header_mode: &str) -> Result<Table, String> {
	parse_csv_table(contents, header_mode)
}

// ---------------------------------------------------------------------
// flowgraph.table.load_google_sheet_public
// ---------------------------------------------------------------------

impl NodeDescriptor for TableLoadGoogleSheetPublicNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.table.load_google_sheet_public".into(),
			title: "Table Load Google Sheet Public".into(),
			category: "table".into(),
			description: Some("公開 Google Sheets を CSV export 経由で Table に読み込む。OAuth は使わない。Effectful".into()),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("spreadsheet", "Spreadsheet", SocketType::String),
				PortSpec::input("sheet", "Sheet", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("range", "Range", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("header_mode", "Header Mode", SocketType::String).with_default(SocketValue::String("auto".into())),
				PortSpec::input("timeout_ms", "Timeout ms", SocketType::Int).with_default(SocketValue::Int(8000)),
			],
			outputs: vec![
				PortSpec::exec_output("on_success", "On Success"),
				PortSpec::exec_output("on_error", "On Error"),
				PortSpec::output("table", "Table", SocketType::Table),
				PortSpec::output("sheet_name", "Sheet Name", SocketType::String),
				PortSpec::output("row_count", "Row Count", SocketType::Int),
				PortSpec::output("column_count", "Column Count", SocketType::Int),
				PortSpec::output("source_url", "Source URL", SocketType::String),
				PortSpec::output("error", "Error", SocketType::String),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Table))),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl EffectfulNode for TableLoadGoogleSheetPublicNode {
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
		let spreadsheet = get_required_string(inputs, "spreadsheet")?;
		let sheet = get_optional_string(inputs, "sheet", "")?;
		let range = get_optional_string(inputs, "range", "")?;
		let header_mode = get_optional_string(inputs, "header_mode", "auto")?;
		let timeout_ms = get_optional_int(inputs, "timeout_ms", 8000)?.clamp(100, 120_000) as u64;
		let source_url = match google_sheet_csv_url(&spreadsheet, &sheet, &range) {
			Ok(url) => url,
			Err(e) => return Ok(err_output_google_sheet(e, String::new())),
		};

		let csv_text = if let Some(mock) = ctx
			.effect_mocks
			.as_ref()
			.and_then(|mocks| mocks.http_response(&ctx.node_id))
			.cloned()
		{
			ctx.log(format!("http.request mock: GET {source_url} -> node {}", ctx.node_id));
			if let Some(error) = mock.error {
				ctx.record_effect(RecordedEffect::http_request(
					ctx.node_id.clone(),
					"GET",
					source_url.clone(),
					JsonValue::Null,
					None,
					None,
					Some(error.clone()),
				));
				return Ok(err_output_google_sheet(format!("request {source_url}: {error}"), source_url));
			}
			ctx.record_effect(RecordedEffect::http_request(
				ctx.node_id.clone(),
				"GET",
				source_url.clone(),
				JsonValue::Null,
				Some(mock.status),
				Some(mock.body_text.clone()),
				None,
			));
			if !(200..=299).contains(&mock.status) {
				return Ok(err_output_google_sheet(format!("HTTP {}", mock.status), source_url));
			}
			mock.body_text
		} else {
			match fetch_text(&source_url, timeout_ms).await {
				Ok(text) => text,
				Err(e) => return Ok(err_output_google_sheet(format!("request {source_url}: {e}"), source_url)),
			}
		};

		match parse_csv_table(&csv_text, &header_mode) {
			Ok(table) => Ok(google_sheet_success_output(
				table,
				sheet_name_or_default(&sheet, "public"),
				source_url,
			)),
			Err(e) => Ok(err_output_google_sheet(format!("parse CSV: {e}"), source_url)),
		}
	}
}

async fn fetch_text(url: &str, timeout_ms: u64) -> Result<String, String> {
	let client = reqwest::Client::builder()
		.timeout(std::time::Duration::from_millis(timeout_ms))
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

fn google_sheet_success_output(table: Table, sheet_name: String, source_url: String) -> NodeOutput {
	let row_count = table.len() as i64;
	let column_count = table.schema().len() as i64;
	NodeOutput::new()
		.set_data("table", SocketValue::Table(table.clone()))
		.set_data("sheet_name", SocketValue::String(sheet_name))
		.set_data("row_count", SocketValue::Int(row_count))
		.set_data("column_count", SocketValue::Int(column_count))
		.set_data("source_url", SocketValue::String(source_url))
		.set_data("error", SocketValue::String(String::new()))
		.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Table(table))))
		.fire_exec("on_success")
}

fn err_output_google_sheet(msg: impl Into<String>, source_url: String) -> NodeOutput {
	let msg = msg.into();
	NodeOutput::new()
		.set_data("table", SocketValue::Table(Table::empty()))
		.set_data("sheet_name", SocketValue::String(String::new()))
		.set_data("row_count", SocketValue::Int(0))
		.set_data("column_count", SocketValue::Int(0))
		.set_data("source_url", SocketValue::String(source_url))
		.set_data("error", SocketValue::String(msg.clone()))
		.set_data(
			"result",
			SocketValue::Result(FlowResult::err(msg).with_code("table.load_google_sheet_public")),
		)
		.fire_exec("on_error")
}

// ---------------------------------------------------------------------
// flowgraph.table.load_google_sheet_auth
// ---------------------------------------------------------------------

impl NodeDescriptor for TableLoadGoogleSheetAuthNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.table.load_google_sheet_auth".into(),
			title: "Table Load Google Sheet Auth".into(),
			category: "table".into(),
			description: Some(
				"Google Sheets API v4 values.get を OAuth access_token 付きで呼び、private Sheet を Table に読み込む。Effectful".into(),
			),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("spreadsheet", "Spreadsheet", SocketType::String),
				PortSpec::input("access_token", "Access Token", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("token_env", "Token Env", SocketType::String)
					.with_default(SocketValue::String("UNVAC_GOOGLE_SHEETS_ACCESS_TOKEN".into())),
				PortSpec::input("token_file", "Token File", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("sheet", "Sheet", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("range", "Range", SocketType::String).with_default(SocketValue::String("A:ZZ".into())),
				PortSpec::input("header_mode", "Header Mode", SocketType::String).with_default(SocketValue::String("auto".into())),
				PortSpec::input("timeout_ms", "Timeout ms", SocketType::Int).with_default(SocketValue::Int(8000)),
			],
			outputs: vec![
				PortSpec::exec_output("on_success", "On Success"),
				PortSpec::exec_output("on_error", "On Error"),
				PortSpec::output("table", "Table", SocketType::Table),
				PortSpec::output("sheet_name", "Sheet Name", SocketType::String),
				PortSpec::output("row_count", "Row Count", SocketType::Int),
				PortSpec::output("column_count", "Column Count", SocketType::Int),
				PortSpec::output("source_url", "Source URL", SocketType::String),
				PortSpec::output("error", "Error", SocketType::String),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Table))),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl EffectfulNode for TableLoadGoogleSheetAuthNode {
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
		let spreadsheet = get_required_string(inputs, "spreadsheet")?;
		let access_token = get_optional_string(inputs, "access_token", "")?;
		let token_env = get_optional_string(inputs, "token_env", "UNVAC_GOOGLE_SHEETS_ACCESS_TOKEN")?;
		let token_file = get_optional_string(inputs, "token_file", "")?;
		let sheet = get_optional_string(inputs, "sheet", "")?;
		let range = get_optional_string(inputs, "range", "A:ZZ")?;
		let header_mode = get_optional_string(inputs, "header_mode", "auto")?;
		let timeout_ms = get_optional_int(inputs, "timeout_ms", 8000)?.clamp(100, 120_000) as u64;
		let access_token = match resolve_google_access_token(ctx, &access_token, &token_env, &token_file).await {
			Ok(token) => token,
			Err(e) => return Ok(err_output_google_sheet_auth(e, String::new())),
		};
		let source_url = match google_sheet_values_url(&spreadsheet, &sheet, &range) {
			Ok(url) => url,
			Err(e) => return Ok(err_output_google_sheet_auth(e, String::new())),
		};

		let body = if let Some(mock) = ctx
			.effect_mocks
			.as_ref()
			.and_then(|mocks| mocks.http_response(&ctx.node_id))
			.cloned()
		{
			ctx.log(format!("http.request mock: GET {source_url} -> node {}", ctx.node_id));
			let request = serde_json::json!({"authenticated": true, "auth": "bearer", "access_token": "<redacted>"});
			if let Some(error) = mock.error {
				ctx.record_effect(RecordedEffect::http_request(
					ctx.node_id.clone(),
					"GET",
					source_url.clone(),
					request,
					None,
					None,
					Some(error.clone()),
				));
				return Ok(err_output_google_sheet_auth(format!("request {source_url}: {error}"), source_url));
			}
			ctx.record_effect(RecordedEffect::http_request(
				ctx.node_id.clone(),
				"GET",
				source_url.clone(),
				request,
				Some(mock.status),
				Some(mock.body_text.clone()),
				None,
			));
			if !(200..=299).contains(&mock.status) {
				return Ok(err_output_google_sheet_auth(format!("HTTP {}", mock.status), source_url));
			}
			mock.body_text
		} else {
			match fetch_text_bearer(&source_url, &access_token, timeout_ms).await {
				Ok(text) => text,
				Err(e) => return Ok(err_output_google_sheet_auth(format!("request {source_url}: {e}"), source_url)),
			}
		};

		match parse_google_sheet_values_table(&body, &header_mode) {
			Ok(table) => Ok(google_sheet_success_output(
				table,
				sheet_name_or_default(&sheet, "authenticated"),
				source_url,
			)),
			Err(e) => Ok(err_output_google_sheet_auth(format!("parse values JSON: {e}"), source_url)),
		}
	}
}

async fn resolve_google_access_token(ctx: &mut ExecCtx, access_token: &str, token_env: &str, token_file: &str) -> Result<String, String> {
	let access_token = access_token.trim();
	if !access_token.is_empty() {
		return Ok(access_token.to_string());
	}
	let token_env = token_env.trim();
	if !token_env.is_empty() {
		if let Ok(value) = std::env::var(token_env) {
			let value = value.trim();
			if !value.is_empty() {
				return Ok(value.to_string());
			}
		}
	}
	let token_file = token_file.trim();
	if !token_file.is_empty() {
		match tokio::fs::read_to_string(token_file).await {
			Ok(contents) => {
				ctx.record_effect(RecordedEffect::file_read(ctx.node_id.clone(), token_file.to_string(), None, None));
				return parse_google_token_file_contents(&contents);
			}
			Err(e) => {
				ctx.record_effect(RecordedEffect::file_read(
					ctx.node_id.clone(),
					token_file.to_string(),
					None,
					Some(e.to_string()),
				));
				return Err(format!("read token_file {token_file}: {e}"));
			}
		}
	}
	Err("access_token is empty; set access_token, token_env, or token_file".into())
}

fn parse_google_token_file_contents(contents: &str) -> Result<String, String> {
	let trimmed = contents.trim();
	if trimmed.is_empty() {
		return Err("token_file is empty".into());
	}
	if trimmed.starts_with('{') {
		let json: JsonValue = serde_json::from_str(trimmed).map_err(|e| format!("parse token_file JSON: {e}"))?;
		for key in ["access_token", "accessToken", "token"] {
			if let Some(value) = json
				.get(key)
				.and_then(JsonValue::as_str)
				.map(str::trim)
				.filter(|value| !value.is_empty())
			{
				return Ok(value.to_string());
			}
		}
		return Err("token_file JSON must contain access_token".into());
	}
	Ok(trimmed.to_string())
}

fn err_output_google_sheet_auth(msg: impl Into<String>, source_url: String) -> NodeOutput {
	let msg = msg.into();
	let mut out = err_output_google_sheet(msg.clone(), source_url);
	out.data.insert(
		"result".into(),
		SocketValue::Result(FlowResult::err(msg).with_code("table.load_google_sheet_auth")),
	);
	out
}

fn google_sheet_csv_url(spreadsheet: &str, sheet: &str, range: &str) -> Result<String, String> {
	let (id, url_gid) = extract_google_sheet_id_and_gid(spreadsheet)?;
	let sheet = sheet.trim();
	let mut url = format!("https://docs.google.com/spreadsheets/d/{id}/gviz/tq?tqx=out:csv");
	if !sheet.is_empty() {
		if sheet.chars().all(|ch| ch.is_ascii_digit()) {
			url.push_str("&gid=");
			url.push_str(sheet);
		} else {
			url.push_str("&sheet=");
			url.push_str(&urlencoding::encode(sheet));
		}
	} else if let Some(gid) = url_gid {
		url.push_str("&gid=");
		url.push_str(&gid);
	}
	let range = range.trim();
	if !range.is_empty() {
		parse_a1_range(range)?;
		url.push_str("&range=");
		url.push_str(&urlencoding::encode(range));
	}
	Ok(url)
}

fn google_sheet_values_url(spreadsheet: &str, sheet: &str, range: &str) -> Result<String, String> {
	let (id, _) = extract_google_sheet_id_and_gid(spreadsheet)?;
	let a1 = google_sheet_values_a1(sheet, range)?;
	Ok(format!(
		"https://sheets.googleapis.com/v4/spreadsheets/{}/values/{}?majorDimension=ROWS&valueRenderOption=UNFORMATTED_VALUE",
		id,
		urlencoding::encode(&a1)
	))
}

fn google_sheet_values_a1(sheet: &str, range: &str) -> Result<String, String> {
	let sheet = sheet.trim();
	let range = range.trim();
	if range.is_empty() {
		if sheet.is_empty() {
			return Ok("A:ZZ".into());
		}
		return Ok(quote_google_sheet_name(sheet));
	}
	parse_a1_range_or_open(range)?;
	if sheet.is_empty() {
		Ok(range.to_string())
	} else {
		Ok(format!("{}!{}", quote_google_sheet_name(sheet), range))
	}
}

fn quote_google_sheet_name(sheet: &str) -> String {
	format!("'{}'", sheet.replace('\'', "''"))
}

fn parse_a1_range_or_open(input: &str) -> Result<(), String> {
	if parse_a1_range(input).is_ok() {
		return Ok(());
	}
	let trimmed = input.trim();
	let mut parts = trimmed.split(':');
	let Some(start) = parts.next() else {
		return Err(format!("invalid A1 range {input:?}"));
	};
	let Some(end) = parts.next() else {
		return Err(format!("invalid A1 range {input:?}"));
	};
	if parts.next().is_some() || start.is_empty() || end.is_empty() {
		return Err(format!("invalid A1 range {input:?}"));
	}
	if !start.chars().all(|ch| ch.is_ascii_alphabetic()) || !end.chars().all(|ch| ch.is_ascii_alphabetic()) {
		return Err(format!("invalid A1 range {input:?}"));
	}
	Ok(())
}

async fn fetch_text_bearer(url: &str, access_token: &str, timeout_ms: u64) -> Result<String, String> {
	let client = reqwest::Client::builder()
		.timeout(std::time::Duration::from_millis(timeout_ms))
		.build()
		.map_err(|e| format!("reqwest client: {e}"))?;
	let response = client
		.get(url)
		.bearer_auth(access_token)
		.send()
		.await
		.map_err(|e| format!("send failed: {e}"))?;
	let status = response.status();
	let text = response.text().await.map_err(|e| format!("read body failed: {e}"))?;
	if !status.is_success() {
		return Err(format!("HTTP {}: {}", status.as_u16(), text.chars().take(160).collect::<String>()));
	}
	Ok(text)
}

fn parse_google_sheet_values_table(body: &str, header_mode: &str) -> Result<Table, String> {
	let json: JsonValue = serde_json::from_str(body).map_err(|e| e.to_string())?;
	let values = json
		.get("values")
		.and_then(JsonValue::as_array)
		.ok_or_else(|| "missing values array".to_string())?;
	let rows = values
		.iter()
		.map(|row| {
			row.as_array()
				.ok_or_else(|| "values row is not an array".to_string())
				.map(|cells| cells.iter().map(google_value_cell_to_json).collect::<Vec<_>>())
		})
		.collect::<Result<Vec<_>, _>>()?;
	table_from_grid(&rows, header_mode)
}

fn google_value_cell_to_json(value: &JsonValue) -> JsonValue {
	match value {
		JsonValue::String(s) => csv_cell_to_json(s),
		JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::Null => value.clone(),
		other => JsonValue::String(other.to_string()),
	}
}

fn extract_google_sheet_id_and_gid(input: &str) -> Result<(String, Option<String>), String> {
	let trimmed = input.trim();
	if trimmed.is_empty() {
		return Err("spreadsheet is empty".into());
	}
	let gid = extract_gid(trimmed);
	if let Some(pos) = trimmed.find("/spreadsheets/d/") {
		let rest = &trimmed[pos + "/spreadsheets/d/".len()..];
		let id = rest.split(|ch| matches!(ch, '/' | '?' | '#')).next().unwrap_or_default().trim();
		if id.is_empty() {
			return Err("spreadsheet URL does not contain an id".into());
		}
		return Ok((id.to_string(), gid));
	}
	let id = trimmed.split(|ch| matches!(ch, '/' | '?' | '#')).next().unwrap_or_default().trim();
	if id.is_empty() || id.contains("docs.google.com") {
		return Err("spreadsheet must be a Google Sheets URL or spreadsheet id".into());
	}
	Ok((id.to_string(), gid))
}

fn extract_gid(input: &str) -> Option<String> {
	for marker in ["gid=", "gid%3D"] {
		if let Some(pos) = input.find(marker) {
			let rest = &input[pos + marker.len()..];
			let gid = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect::<String>();
			if !gid.is_empty() {
				return Some(gid);
			}
		}
	}
	None
}

fn parse_csv_table(contents: &str, header_mode: &str) -> Result<Table, String> {
	let mut reader = csv::ReaderBuilder::new()
		.has_headers(false)
		.flexible(true)
		.from_reader(contents.as_bytes());
	let mut rows = Vec::new();
	for record in reader.records() {
		let record = record.map_err(|e| e.to_string())?;
		rows.push(record.iter().map(csv_cell_to_json).collect::<Vec<_>>());
	}
	table_from_grid(&rows, header_mode)
}

fn csv_cell_to_json(cell: &str) -> JsonValue {
	let trimmed = cell.trim();
	if trimmed.is_empty() {
		JsonValue::Null
	} else if trimmed.eq_ignore_ascii_case("true") {
		JsonValue::Bool(true)
	} else if trimmed.eq_ignore_ascii_case("false") {
		JsonValue::Bool(false)
	} else if let Ok(i) = trimmed.parse::<i64>() {
		JsonValue::Number(i.into())
	} else if let Ok(f) = trimmed.parse::<f64>() {
		serde_json::Number::from_f64(f)
			.map(JsonValue::Number)
			.unwrap_or(JsonValue::String(cell.to_string()))
	} else {
		JsonValue::String(cell.to_string())
	}
}

// ---------------------------------------------------------------------
// flowgraph.table.write_tsv
// ---------------------------------------------------------------------

pub struct TableWriteTsvNode;

impl NodeDescriptor for TableWriteTsvNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.table.write_tsv".into(),
			title: "Table Write TSV".into(),
			category: "table".into(),
			description: Some("Table を TSV ファイルに書き出す（atomic rename）。Effectful".into()),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("table", "Table", SocketType::Table),
				PortSpec::input("path", "Path", SocketType::String),
				PortSpec::input("expected_revision", "Expected File Revision", SocketType::String).with_default(SocketValue::String(String::new())),
			],
			outputs: vec![
				PortSpec::exec_output("on_success", "On Success"),
				PortSpec::exec_output("on_error", "On Error"),
				PortSpec::exec_output("on_conflict", "On Conflict"),
				PortSpec::output("bytes_written", "Bytes Written", SocketType::Int),
				PortSpec::output("error", "Error", SocketType::String),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Int))),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl EffectfulNode for TableWriteTsvNode {
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
		let table = get_required_table(inputs, "table")?.clone();
		let path = get_required_string(inputs, "path")?;
		let expected = get_optional_string(inputs, "expected_revision", "")?;
		let serialized = write_tsv_string(&table);
		let bytes = serialized.as_bytes().to_vec();
		let len = bytes.len() as i64;
		if let Some(mock) = ctx
			.effect_mocks
			.as_ref()
			.and_then(|mocks| mocks.file_write_response(&ctx.node_id))
			.cloned()
		{
			ctx.log(format!("file.write mock: {path} ({len} bytes) -> node {}", ctx.node_id));
			if let Some(error) = mock.error {
				ctx.record_effect(RecordedEffect::file_write(
					ctx.node_id.clone(),
					path.clone(),
					len,
					serialized,
					Some(error.clone()),
				));
				return Ok(err_output_write(format!("write {path}: {error}")));
			}
			ctx.record_effect(RecordedEffect::file_write(ctx.node_id.clone(), path.clone(), len, serialized, None));
			return Ok(write_success_output(len));
		}
		let resolved = match ctx.resolve_write_path(&path) {
			Ok(path) => path,
			Err(error) => return Ok(err_output_write(error)),
		};
		if let Err(e) = crate::file_publish::replace_checked_async(resolved, bytes, (!expected.is_empty()).then_some(expected)).await {
			let out = err_output_write(format!("write {path}: {e}"));
			return Ok(if matches!(e, crate::file_publish::CheckedWriteError::Conflict) { out.fire_exec("on_conflict") } else { out });
		}
		Ok(write_success_output(len))
	}
}

fn err_output_write(msg: impl Into<String>) -> NodeOutput {
	let msg = msg.into();
	NodeOutput::new()
		.set_data("bytes_written", SocketValue::Int(0))
		.set_data("error", SocketValue::String(msg.clone()))
		.set_data("result", SocketValue::Result(FlowResult::err(msg).with_code("table.write_tsv")))
		.fire_exec("on_error")
}

fn write_success_output(bytes_written: i64) -> NodeOutput {
	NodeOutput::new()
		.set_data("bytes_written", SocketValue::Int(bytes_written))
		.set_data("error", SocketValue::String(String::new()))
		.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Int(bytes_written))))
		.fire_exec("on_success")
}

// ---------------------------------------------------------------------
// TSV パース / 書き出し（11 カラム辞書スキーマ前提の default 補完を含む）
// ---------------------------------------------------------------------

/// 11 カラム辞書スキーマ（phase-eta-dictionary-unification.md §3.1）。
/// Dictionary 以外の汎用 Table でも、header 行から自前スキーマを作ればよい。
pub const DICTIONARY_COLUMNS: &[&str] = &[
	"reading",
	"surface",
	"part_of_speech",
	"comment",
	"priority",
	"is_locked",
	"enabled",
	"by",
	"created_at",
	"expires_at",
	"tags",
];

const LEGACY_V2_DICTIONARY_COLUMNS: &[&str] = &[
	"source",
	"replacement",
	"kind",
	"priority",
	"is_locked",
	"enabled",
	"by",
	"created_at",
	"expires_at",
	"tags",
	"note",
];

/// 辞書スキーマのデフォルト `ColumnSpec` 配列を作る。
pub fn dictionary_schema() -> TableSchema {
	TableSchema::new(vec![
		ColumnSpec::new("reading", SocketType::String),
		ColumnSpec::new("surface", SocketType::String),
		ColumnSpec::new("part_of_speech", SocketType::String),
		ColumnSpec::new("comment", SocketType::String),
		ColumnSpec::new("priority", SocketType::Int),
		ColumnSpec::new("is_locked", SocketType::Bool),
		ColumnSpec::new("enabled", SocketType::Bool),
		ColumnSpec::new("by", SocketType::String),
		ColumnSpec::new("created_at", SocketType::String),
		ColumnSpec::new("expires_at", SocketType::String).nullable(),
		ColumnSpec::new("tags", SocketType::String),
	])
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum TsvParseError {
	#[error("TSV パース失敗: {0}")]
	Parse(String),
}

/// TSV 文字列 → Table。
///
/// - `mode = "headerful"`: 1 行目をヘッダとして列名に採用、以降は値の TAB 分割
/// - `mode = "google_ime"`: Google 日本語入力互換の 2〜4 列
///   `(reading, surface, part_of_speech?, comment?)` を読み、残りをデフォルト補完
/// - `mode = "legacy_loose"`: V1 互換として空白区切り先頭 2 トークンを
///   `(reading, surface)` に採用し、残りをデフォルト補完
/// - `mode = "auto"`: 新旧 11 列ヘッダ、Google IME、V1 loose を自動判別
pub fn parse_tsv_with_mode(contents: &str, mode: &str) -> Result<Table, TsvParseError> {
	let mode = match mode {
		"headerful" => "headerful",
		"google_ime" => "google_ime",
		"legacy_loose" => "legacy_loose",
		_ => detect_tsv_mode(contents),
	};
	match mode {
		"headerful" => parse_headerful_tsv(contents),
		"legacy_v2_glossary" => parse_legacy_v2_glossary(contents),
		"google_ime" => parse_google_ime(contents),
		_ => parse_legacy_loose(contents),
	}
}

fn detect_tsv_mode(contents: &str) -> &'static str {
	for line in contents.lines() {
		let trimmed = line.trim_start();
		if trimmed.is_empty() || trimmed.starts_with('#') {
			continue;
		}
		let columns: Vec<&str> = trimmed.trim_end_matches('\r').split('\t').collect();
		if columns == LEGACY_V2_DICTIONARY_COLUMNS {
			return "legacy_v2_glossary";
		}
		if columns == DICTIONARY_COLUMNS || trimmed.starts_with("reading\t") || trimmed == "reading" {
			return "headerful";
		}
		// 汎用 Table の従来 auto 判定を維持する。旧 Glossary 11 列は上で先に移行する。
		if trimmed.starts_with("source\t") || trimmed == "source" {
			return "headerful";
		}
		return if columns.len() >= 2 { "google_ime" } else { "legacy_loose" };
	}
	"legacy_loose"
}

fn glossary_row_defaults(reading: String, surface: String, part_of_speech: String, comment: String, by: &str) -> Row {
	let now = jiff::Timestamp::now().strftime("%Y-%m-%dT%H:%M:%SZ").to_string();
	Row::new(vec![
		JsonValue::String(reading),
		JsonValue::String(surface),
		JsonValue::String(part_of_speech),
		JsonValue::String(comment),
		JsonValue::Number(0.into()),
		JsonValue::Bool(true),
		JsonValue::Bool(true),
		JsonValue::String(by.into()),
		JsonValue::String(now),
		JsonValue::Null,
		JsonValue::String(String::new()),
	])
}

fn parse_google_ime(contents: &str) -> Result<Table, TsvParseError> {
	let schema = dictionary_schema();
	let mut rows = Vec::new();
	for (lineno, line) in contents.lines().enumerate() {
		let trimmed = line.trim_end_matches('\r');
		if trimmed.trim().is_empty() || trimmed.trim_start().starts_with('#') {
			continue;
		}
		let fields: Vec<&str> = trimmed.split('\t').collect();
		if !(2..=4).contains(&fields.len()) {
			return Err(TsvParseError::Parse(format!(
				"行 {} は Google IME 互換の 2〜4 列ではありません（{} 列）",
				lineno + 1,
				fields.len()
			)));
		}
		let reading = fields[0].trim();
		let surface = fields[1].trim();
		if reading.is_empty() || surface.is_empty() {
			return Err(TsvParseError::Parse(format!("行 {} の読みまたは単語が空です", lineno + 1)));
		}
		let part_of_speech = fields.get(2).map(|v| v.trim()).filter(|v| !v.is_empty()).unwrap_or("名詞");
		let comment = fields.get(3).copied().unwrap_or("");
		rows.push(glossary_row_defaults(
			reading.into(),
			surface.into(),
			part_of_speech.into(),
			comment.into(),
			"google_ime",
		));
	}
	Ok(Table::new(schema, rows))
}

fn parse_legacy_v2_glossary(contents: &str) -> Result<Table, TsvParseError> {
	let legacy = parse_headerful_tsv(contents)?;
	let schema = dictionary_schema();
	let mut rows = Vec::with_capacity(legacy.len());
	for row in legacy.rows() {
		let get = |name: &str| {
			legacy
				.schema()
				.column_index(name)
				.and_then(|index| row.get(index))
				.cloned()
				.unwrap_or(JsonValue::Null)
		};
		let kind = get("kind").as_str().unwrap_or("literal").to_string();
		let part_of_speech = if kind == "regex" { "正規表現" } else { "名詞" };
		let source = get("source");
		let replacement = get("replacement");
		let note = get("note");
		// 旧CLIは Google IME の reading/surface を、誤って source/replacement の
		// ヘッダで出力していた。CLI生成行だけは値順を維持し、それ以外の旧V2行は
		// ヘッダ本来の source -> replacement 意味に従って反転する。
		let generated_literal = kind != "regex" && note.as_str().is_some_and(|value| value.starts_with("migrated from "));
		let (reading, surface) = if generated_literal {
			(source, replacement)
		} else {
			(replacement, source)
		};
		rows.push(Row::new(vec![
			reading,
			surface,
			JsonValue::String(part_of_speech.into()),
			note,
			get("priority"),
			get("is_locked"),
			get("enabled"),
			get("by"),
			get("created_at"),
			get("expires_at"),
			get("tags"),
		]));
	}
	Ok(Table::new(schema, rows))
}

fn parse_headerful_tsv(contents: &str) -> Result<Table, TsvParseError> {
	let mut lines = contents
		.lines()
		.filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'));
	let header = lines.next().ok_or_else(|| TsvParseError::Parse("空のファイル".into()))?;
	let col_names: Vec<String> = header.split('\t').map(unescape_tsv_field).collect();
	// 既知のカラム名は型を付与、未知カラムは json 扱い
	let columns: Vec<ColumnSpec> = col_names
		.iter()
		.map(|n| ColumnSpec::new(n.clone(), infer_type_for_column(n)).nullable())
		.collect();
	let schema = TableSchema::new(columns);

	let mut rows = Vec::new();
	for (lineno, line) in lines.enumerate() {
		let fields: Vec<String> = line.split('\t').map(unescape_tsv_field).collect();
		if fields.len() > col_names.len() {
			return Err(TsvParseError::Parse(format!(
				"行 {} のカラム数 {} がヘッダ {} を超過",
				lineno + 2,
				fields.len(),
				col_names.len()
			)));
		}
		let mut values = Vec::with_capacity(col_names.len());
		for (i, col) in col_names.iter().enumerate() {
			let raw = fields.get(i).cloned().unwrap_or_default();
			values.push(string_to_json_for_column(col, &raw));
		}
		rows.push(Row::new(values));
	}
	Ok(Table::new(schema, rows))
}

fn parse_legacy_loose(contents: &str) -> Result<Table, TsvParseError> {
	let schema = dictionary_schema();
	let mut rows = Vec::new();
	for line in contents.lines() {
		let trimmed = line.trim();
		if trimmed.is_empty() || trimmed.starts_with('#') {
			continue;
		}
		// 先頭 2 トークン（空白 or TAB）を採用
		let mut iter = trimmed.split_whitespace();
		let reading = match iter.next() {
			Some(s) => s.to_string(),
			None => continue,
		};
		let surface = match iter.next() {
			Some(s) => s.to_string(),
			None => continue,
		};
		rows.push(glossary_row_defaults(reading, surface, "名詞".into(), String::new(), "legacy"));
	}
	Ok(Table::new(schema, rows))
}

fn infer_type_for_column(name: &str) -> SocketType {
	match name {
		"priority" => SocketType::Int,
		"is_locked" | "enabled" => SocketType::Bool,
		_ => SocketType::String,
	}
}

fn string_to_json_for_column(col: &str, raw: &str) -> JsonValue {
	match col {
		"priority" => raw
			.parse::<i64>()
			.map(|n| JsonValue::Number(n.into()))
			.unwrap_or(JsonValue::Number(0.into())),
		"is_locked" | "enabled" => match raw.to_ascii_lowercase().as_str() {
			"true" | "1" | "yes" | "y" => JsonValue::Bool(true),
			"false" | "0" | "no" | "n" => JsonValue::Bool(false),
			_ => JsonValue::Bool(col == "enabled"), // 空文字は enabled だけ default true
		},
		"expires_at" => {
			if raw.is_empty() {
				JsonValue::Null
			} else {
				JsonValue::String(raw.to_string())
			}
		}
		_ => JsonValue::String(raw.to_string()),
	}
}

/// Table → TSV 文字列。ヘッダありで出力。値内タブ/改行/バックスラッシュをエスケープ。
pub fn write_tsv_string(table: &Table) -> String {
	let mut out = String::new();
	let schema = table.schema();
	let names: Vec<&str> = schema.column_names().collect();
	out.push_str(&names.iter().map(|n| escape_tsv_field(n)).collect::<Vec<_>>().join("\t"));
	out.push('\n');
	for row in table.rows() {
		let mut cells = Vec::with_capacity(names.len());
		for (i, _col) in names.iter().enumerate() {
			let v = row.get(i).cloned().unwrap_or(JsonValue::Null);
			cells.push(escape_tsv_field(&json_to_tsv_cell(&v)));
		}
		out.push_str(&cells.join("\t"));
		out.push('\n');
	}
	out
}

fn json_to_tsv_cell(v: &JsonValue) -> String {
	match v {
		JsonValue::Null => String::new(),
		JsonValue::Bool(b) => b.to_string(),
		JsonValue::Number(n) => n.to_string(),
		JsonValue::String(s) => s.clone(),
		_ => serde_json::to_string(v).unwrap_or_default(),
	}
}

fn escape_tsv_field(s: &str) -> String {
	let mut out = String::with_capacity(s.len());
	for c in s.chars() {
		match c {
			'\\' => out.push_str(r"\\"),
			'\t' => out.push_str(r"\t"),
			'\n' => out.push_str(r"\n"),
			'\r' => out.push_str(r"\r"),
			c => out.push(c),
		}
	}
	out
}

fn unescape_tsv_field(s: &str) -> String {
	let mut out = String::with_capacity(s.len());
	let mut chars = s.chars().peekable();
	while let Some(c) = chars.next() {
		if c == '\\' {
			match chars.next() {
				Some('\\') => out.push('\\'),
				Some('t') => out.push('\t'),
				Some('n') => out.push('\n'),
				Some('r') => out.push('\r'),
				Some(other) => {
					out.push('\\');
					out.push(other);
				}
				None => out.push('\\'),
			}
		} else {
			out.push(c);
		}
	}
	out
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
	#[tokio::test]
	async fn concat_preserves_duplicates_and_rejects_different_column_types() {
		use super::*;
		let first = parse_tsv_with_mode("value\nsame\nsame\n", "headerful").unwrap();
		let second = parse_tsv_with_mode("value\nlast\n", "headerful").unwrap();
		let mut inputs = InputMap::from([
			("first".into(), SocketValue::Table(first.clone())),
			("second".into(), SocketValue::Table(second.clone())),
		]);
		let host = crate::flowgraph::node::PureEvalHost::default();
		let result = TableConcatNode.compute(&host, &InputMap::new(), &inputs, &ExecFireSet::new()).await.unwrap();
		let table = result.data["table"].as_table().unwrap();
		assert_eq!(table.schema(), first.schema());
		assert_eq!(table.rows(), [first.rows(), second.rows()].concat());
		let other = Table::new(TableSchema::new(vec![ColumnSpec::new("value", SocketType::Int)]), vec![]);
		inputs.insert("second".into(), SocketValue::Table(other));
		assert!(TableConcatNode.compute(&host, &InputMap::new(), &inputs, &ExecFireSet::new()).await.is_err());
	}
	use super::*;
	use crate::flowgraph::node::{EffectMocks, FileReadMockResponse, HttpMockResponse, InputMap};
	use std::sync::Arc;
	use std::time::{SystemTime, UNIX_EPOCH};

	fn fired() -> ExecFireSet {
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		fired
	}

	fn temp_token_file(contents: &str) -> std::path::PathBuf {
		let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
		let path = std::env::temp_dir().join(format!("unvac-google-token-{nanos}.json"));
		std::fs::write(&path, contents).unwrap();
		path
	}

	#[tokio::test]
	async fn table_from_json_roundtrip() {
		let node_to = TableFromJsonNode;
		let node_from = TableToJsonNode;
		let src = SocketValue::List(vec![
			SocketValue::Json(serde_json::json!({"a": "x", "b": 1})),
			SocketValue::Json(serde_json::json!({"a": "y", "b": 2})),
		]);
		let mut inputs = InputMap::new();
		inputs.insert("json".into(), src);
		let out = node_to
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		let table_val = out.data.get("table").unwrap().clone();
		assert_eq!(out.data.get("row_count"), Some(&SocketValue::Int(2)));

		let mut inputs2 = InputMap::new();
		inputs2.insert("table".into(), table_val);
		let out2 = node_from
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs2,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		if let Some(SocketValue::List(xs)) = out2.data.get("json") {
			assert_eq!(xs.len(), 2);
		} else {
			panic!("json not a list");
		}
	}

	#[test]
	fn parse_headerful_tsv_basic() {
		let tsv = "reading\tsurface\tpart_of_speech\tcomment\tpriority\tis_locked\tenabled\tby\tcreated_at\texpires_at\ttags\n\
		          ぐりーてぃんぐ\thello\t名詞\t挨拶\t10\tfalse\ttrue\tuser:a\t2026-04-23T00:00:00Z\t\tcasual\n";
		let table = parse_tsv_with_mode(tsv, "headerful").unwrap();
		assert_eq!(table.len(), 1);
		assert_eq!(table.schema().len(), 11);
		let row = &table.rows()[0];
		assert_eq!(row.get(0).and_then(|v| v.as_str()), Some("ぐりーてぃんぐ"));
		assert_eq!(row.get(4).and_then(|v| v.as_i64()), Some(10));
		assert_eq!(row.get(5).and_then(|v| v.as_bool()), Some(false));
		assert_eq!(row.get(6).and_then(|v| v.as_bool()), Some(true));
		assert!(row.get(9).unwrap().is_null());
	}

	#[test]
	fn parse_legacy_loose_space_and_tab() {
		// 空白区切り + TAB 区切り混在の legacy ファイル
		let contents = "にんげん 人間\nいかく\t異格\t名詞\t将進酒\n# comment line\n\n";
		let table = parse_tsv_with_mode(contents, "legacy_loose").unwrap();
		assert_eq!(table.len(), 2);
		assert_eq!(table.schema().len(), 11);
		let r0 = &table.rows()[0];
		assert_eq!(r0.get(0).and_then(|v| v.as_str()), Some("にんげん"));
		assert_eq!(r0.get(1).and_then(|v| v.as_str()), Some("人間"));
		assert_eq!(r0.get(5).and_then(|v| v.as_bool()), Some(true)); // is_locked
		let r1 = &table.rows()[1];
		assert_eq!(r1.get(0).and_then(|v| v.as_str()), Some("いかく"));
		assert_eq!(r1.get(1).and_then(|v| v.as_str()), Some("異格"));
	}

	#[test]
	fn parse_google_ime_preserves_two_three_and_four_column_prefix() {
		let contents = "にんげん\t人間\nいかく\t異格\t名詞\nけるしー\tケルシー\t固有名詞\tアークナイツ\n";
		let table = parse_tsv_with_mode(contents, "google_ime").unwrap();
		assert_eq!(table.len(), 3);
		assert_eq!(table.schema().column_names().collect::<Vec<_>>(), DICTIONARY_COLUMNS);
		assert_eq!(table.rows()[0].get(2).and_then(|v| v.as_str()), Some("名詞"));
		assert_eq!(table.rows()[1].get(2).and_then(|v| v.as_str()), Some("名詞"));
		assert_eq!(table.rows()[2].get(3).and_then(|v| v.as_str()), Some("アークナイツ"));
	}

	#[test]
	fn auto_detect_chooses_legacy_for_loose_file() {
		let contents = "にんげん 人間\n";
		let table = parse_tsv_with_mode(contents, "auto").unwrap();
		assert_eq!(table.len(), 1);
		assert_eq!(table.schema().len(), 11);
	}

	#[test]
	fn auto_detect_migrates_legacy_v2_glossary_header() {
		let tsv = "source\treplacement\tkind\tpriority\tis_locked\tenabled\tby\tcreated_at\texpires_at\ttags\tnote\n\
		          hi\thello\tliteral\t0\tfalse\ttrue\tme\t2026-04-23T00:00:00Z\t\t\t\n\
		          いかく\t異格\tliteral\t0\ttrue\ttrue\tmigrator\t2026-04-23T00:00:00Z\t\t\tmigrated from dictionary.txt\n\
		          ^foo$\tふー\tregex\t0\tfalse\ttrue\tme\t2026-04-23T00:00:00Z\t\t\t\n";
		let table = parse_tsv_with_mode(tsv, "auto").unwrap();
		assert_eq!(table.len(), 3);
		assert_eq!(table.schema().columns[0].name, "reading");
		assert_eq!(table.rows()[0].get(0).and_then(|v| v.as_str()), Some("hello"));
		assert_eq!(table.rows()[0].get(1).and_then(|v| v.as_str()), Some("hi"));
		assert_eq!(table.rows()[0].get(2).and_then(|v| v.as_str()), Some("名詞"));
		assert_eq!(table.rows()[1].get(0).and_then(|v| v.as_str()), Some("いかく"));
		assert_eq!(table.rows()[1].get(1).and_then(|v| v.as_str()), Some("異格"));
		assert_eq!(table.rows()[2].get(0).and_then(|v| v.as_str()), Some("ふー"));
		assert_eq!(table.rows()[2].get(1).and_then(|v| v.as_str()), Some("^foo$"));
		assert_eq!(table.rows()[2].get(2).and_then(|v| v.as_str()), Some("正規表現"));
	}

	#[test]
	fn write_and_reparse_tsv_roundtrip() {
		let tsv = "reading\tsurface\tpart_of_speech\tcomment\tpriority\tis_locked\tenabled\tby\tcreated_at\texpires_at\ttags\n\
		          はい\thi\t名詞\tメモ\t5\tfalse\ttrue\tme\t2026-04-23T00:00:00Z\t\tx,y\n";
		let table = parse_tsv_with_mode(tsv, "headerful").unwrap();
		let serialized = write_tsv_string(&table);
		let reparsed = parse_tsv_with_mode(&serialized, "headerful").unwrap();
		assert_eq!(reparsed.rows(), table.rows());
	}

	#[test]
	fn xlsx_a1_range_parser_uses_zero_based_inclusive_bounds() {
		assert_eq!(parse_a1_range("").unwrap(), None);
		assert_eq!(parse_a1_range("A1:C3").unwrap(), Some((0, 0, 2, 2)));
		assert_eq!(parse_a1_range("b2:d4").unwrap(), Some((1, 1, 3, 3)));
		assert!(parse_a1_range("C3:A1").is_err());
		assert!(parse_a1_range("1A").is_err());
	}

	#[test]
	fn xlsx_table_from_grid_normalizes_headers_and_range() {
		let grid = vec![
			vec![
				JsonValue::String("Scene Name".into()),
				JsonValue::String("Scene Name".into()),
				JsonValue::String("1 Count".into()),
			],
			vec![
				JsonValue::String("opening".into()),
				JsonValue::String("duplicate".into()),
				JsonValue::Number(2.into()),
			],
		];
		let table = table_from_grid(&grid, "first_row").unwrap();
		let names = table.schema().column_names().collect::<Vec<_>>();
		assert_eq!(names, vec!["Scene_Name", "Scene_Name_2", "col_1_Count"]);
		assert_eq!(table.len(), 1);
		assert_eq!(table.rows()[0].get(2).and_then(JsonValue::as_i64), Some(2));
	}

	#[tokio::test]
	async fn load_xlsx_mock_fixture_returns_table_contract() {
		let mut mocks = EffectMocks::default();
		mocks.file_read.insert(
			"xlsx".into(),
			FileReadMockResponse {
				contents: "scene,count\nopening,2\nending,3\n".into(),
				error: None,
			},
		);
		let mut ctx = ExecCtx {
			node_id: "xlsx".into(),
			effect_mocks: Some(Arc::new(mocks)),
			..Default::default()
		};
		let inputs: InputMap = [
			("path".into(), SocketValue::String("profile.xlsx".into())),
			("header_mode".into(), SocketValue::String("first_row".into())),
		]
		.into_iter()
		.collect();

		let out = TableLoadXlsxNode
			.execute(&mut ctx, &InputMap::new(), &inputs, &fired())
			.await
			.unwrap();
		assert!(out.fired_exec.contains("on_success"));
		assert_eq!(out.data.get("row_count"), Some(&SocketValue::Int(2)));
		assert_eq!(out.data.get("column_count"), Some(&SocketValue::Int(2)));
		assert_eq!(out.data.get("sheet_name"), Some(&SocketValue::String("mock".into())));
		let table = out.data.get("table").unwrap().as_table().unwrap();
		assert_eq!(table.schema().column_names().collect::<Vec<_>>(), vec!["scene", "count"]);
	}

	#[test]
	fn google_sheet_url_accepts_id_url_gid_sheet_and_range() {
		let url = google_sheet_csv_url("sheet123", "", "").unwrap();
		assert_eq!(url, "https://docs.google.com/spreadsheets/d/sheet123/gviz/tq?tqx=out:csv");

		let url = google_sheet_csv_url("https://docs.google.com/spreadsheets/d/sheet123/edit#gid=456", "", "A1:B2").unwrap();
		assert_eq!(
			url,
			"https://docs.google.com/spreadsheets/d/sheet123/gviz/tq?tqx=out:csv&gid=456&range=A1%3AB2"
		);

		let url = google_sheet_csv_url("sheet123", "Scene List", "").unwrap();
		assert_eq!(
			url,
			"https://docs.google.com/spreadsheets/d/sheet123/gviz/tq?tqx=out:csv&sheet=Scene%20List"
		);

		let url = google_sheet_csv_url("sheet123", "789", "").unwrap();
		assert_eq!(url, "https://docs.google.com/spreadsheets/d/sheet123/gviz/tq?tqx=out:csv&gid=789");
	}

	#[test]
	fn google_sheet_values_url_builds_api_range() {
		let url = google_sheet_values_url("sheet123", "Scene List", "A1:B3").unwrap();
		assert_eq!(
			url,
			"https://sheets.googleapis.com/v4/spreadsheets/sheet123/values/%27Scene%20List%27%21A1%3AB3?majorDimension=ROWS&valueRenderOption=UNFORMATTED_VALUE"
		);

		let url = google_sheet_values_url("https://docs.google.com/spreadsheets/d/sheet123/edit#gid=456", "", "A:ZZ").unwrap();
		assert_eq!(
			url,
			"https://sheets.googleapis.com/v4/spreadsheets/sheet123/values/A%3AZZ?majorDimension=ROWS&valueRenderOption=UNFORMATTED_VALUE"
		);

		assert!(google_sheet_values_url("sheet123", "", "1A").is_err());
	}

	#[test]
	fn parse_csv_table_uses_header_mode_and_typed_cells() {
		let table = parse_csv_table("scene,count,enabled\nopening,2,true\nending,3,false\n", "first_row").unwrap();
		assert_eq!(table.schema().column_names().collect::<Vec<_>>(), vec!["scene", "count", "enabled"]);
		assert_eq!(table.len(), 2);
		assert_eq!(table.rows()[0].get(1).and_then(JsonValue::as_i64), Some(2));
		assert_eq!(table.rows()[0].get(2).and_then(JsonValue::as_bool), Some(true));
	}

	#[tokio::test]
	async fn table_draw_count_one_emits_row_and_index() {
		let table = parse_csv_table("scene,count\nopening,2\nending,3\n", "first_row").unwrap();
		let inputs: InputMap = [
			("table".into(), SocketValue::Table(table)),
			("count".into(), SocketValue::Int(1)),
			("seed".into(), SocketValue::String("fixed".into())),
		]
		.into_iter()
		.collect();

		let out = TableDrawNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();

		assert!(matches!(out.data.get("row"), Some(SocketValue::Json(JsonValue::Object(_)))));
		assert_eq!(out.data.get("rows").unwrap().as_list().unwrap().len(), 1);
		assert_eq!(out.data.get("indices").unwrap().as_list().unwrap().len(), 1);
	}

	#[tokio::test]
	async fn table_draw_without_replacement_keeps_indices_unique() {
		let table = parse_csv_table("scene\nopening\nmain\nending\n", "first_row").unwrap();
		let inputs: InputMap = [
			("table".into(), SocketValue::Table(table)),
			("count".into(), SocketValue::Int(3)),
			("seed".into(), SocketValue::String("unique".into())),
			("replacement".into(), SocketValue::Bool(false)),
		]
		.into_iter()
		.collect();

		let out = TableDrawNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		let mut indices: Vec<i64> = out
			.data
			.get("indices")
			.unwrap()
			.as_list()
			.unwrap()
			.iter()
			.map(|value| value.as_i64().unwrap())
			.collect();
		indices.sort_unstable();
		indices.dedup();
		assert_eq!(indices.len(), 3);
	}

	#[tokio::test]
	async fn table_draw_uses_positive_weight_column_and_ignores_zero_values() {
		let table = parse_csv_table("scene,weight\nnever,0\nalways,10\n", "first_row").unwrap();
		let inputs: InputMap = [
			("table".into(), SocketValue::Table(table)),
			("count".into(), SocketValue::Int(1)),
			("weight_column".into(), SocketValue::String("weight".into())),
			("seed".into(), SocketValue::String("weighted".into())),
		]
		.into_iter()
		.collect();

		let out = TableDrawNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();

		let row = out.data.get("row").unwrap().as_json().unwrap();
		assert_eq!(row.get("scene").and_then(JsonValue::as_str), Some("always"));
		assert_eq!(out.data.get("indices"), Some(&SocketValue::List(vec![SocketValue::Int(1)])));
	}

	#[tokio::test]
	async fn load_google_sheet_public_mock_fixture_returns_table_contract() {
		let mut mocks = EffectMocks::default();
		mocks.http.insert(
			"sheet".into(),
			HttpMockResponse {
				status: 200,
				body_text: "scene,count\nopening,2\nending,3\n".into(),
				body_json: JsonValue::Null,
				error: None,
			},
		);
		let mut ctx = ExecCtx {
			node_id: "sheet".into(),
			effect_mocks: Some(Arc::new(mocks)),
			..Default::default()
		};
		let inputs: InputMap = [
			("spreadsheet".into(), SocketValue::String("sheet123".into())),
			("sheet".into(), SocketValue::String("Scenes".into())),
			("range".into(), SocketValue::String("A1:B3".into())),
			("header_mode".into(), SocketValue::String("first_row".into())),
		]
		.into_iter()
		.collect();

		let out = TableLoadGoogleSheetPublicNode
			.execute(&mut ctx, &InputMap::new(), &inputs, &fired())
			.await
			.unwrap();
		assert!(out.fired_exec.contains("on_success"));
		assert_eq!(out.data.get("row_count"), Some(&SocketValue::Int(2)));
		assert_eq!(out.data.get("column_count"), Some(&SocketValue::Int(2)));
		assert_eq!(out.data.get("sheet_name"), Some(&SocketValue::String("Scenes".into())));
		let source_url = out.data.get("source_url").unwrap().as_str().unwrap();
		assert!(source_url.contains("sheet=Scenes"));
		assert!(source_url.contains("range=A1%3AB3"));
		assert_eq!(ctx.recorded_effects.len(), 1);
		assert_eq!(ctx.recorded_effects[0].kind, "http");
	}

	#[tokio::test]
	async fn load_google_sheet_auth_mock_fixture_returns_table_contract() {
		let mut mocks = EffectMocks::default();
		mocks.http.insert(
			"sheet_auth".into(),
			HttpMockResponse {
				status: 200,
				body_text: r#"{"range":"Scenes!A1:B3","majorDimension":"ROWS","values":[["scene","count"],["opening",2],["ending",3]]}"#
					.into(),
				body_json: JsonValue::Null,
				error: None,
			},
		);
		let mut ctx = ExecCtx {
			node_id: "sheet_auth".into(),
			effect_mocks: Some(Arc::new(mocks)),
			..Default::default()
		};
		let inputs: InputMap = [
			("spreadsheet".into(), SocketValue::String("sheet123".into())),
			("access_token".into(), SocketValue::String("token".into())),
			("sheet".into(), SocketValue::String("Scenes".into())),
			("range".into(), SocketValue::String("A1:B3".into())),
			("header_mode".into(), SocketValue::String("first_row".into())),
		]
		.into_iter()
		.collect();

		let out = TableLoadGoogleSheetAuthNode
			.execute(&mut ctx, &InputMap::new(), &inputs, &fired())
			.await
			.unwrap();
		assert!(out.fired_exec.contains("on_success"));
		assert_eq!(out.data.get("row_count"), Some(&SocketValue::Int(2)));
		assert_eq!(out.data.get("column_count"), Some(&SocketValue::Int(2)));
		assert_eq!(out.data.get("sheet_name"), Some(&SocketValue::String("Scenes".into())));
		let table = out.data.get("table").unwrap().as_table().unwrap();
		assert_eq!(table.schema().column_names().collect::<Vec<_>>(), vec!["scene", "count"]);
		assert_eq!(table.rows()[0].get(1).and_then(JsonValue::as_i64), Some(2));
		assert_eq!(ctx.recorded_effects.len(), 1);
		assert_eq!(ctx.recorded_effects[0].request_body.as_ref().unwrap()["access_token"], "<redacted>");
	}

	#[tokio::test]
	async fn load_google_sheet_auth_reads_token_file_without_recording_token_contents() {
		let token_file = temp_token_file(r#"{"access_token":"file-token"}"#);
		let mut mocks = EffectMocks::default();
		mocks.http.insert(
			"sheet_auth".into(),
			HttpMockResponse {
				status: 200,
				body_text: r#"{"values":[["scene"],["opening"]]}"#.into(),
				body_json: JsonValue::Null,
				error: None,
			},
		);
		let mut ctx = ExecCtx {
			node_id: "sheet_auth".into(),
			effect_mocks: Some(Arc::new(mocks)),
			..Default::default()
		};
		let inputs: InputMap = [
			("spreadsheet".into(), SocketValue::String("sheet123".into())),
			("token_env".into(), SocketValue::String(String::new())),
			("token_file".into(), SocketValue::String(token_file.to_string_lossy().to_string())),
			("header_mode".into(), SocketValue::String("first_row".into())),
		]
		.into_iter()
		.collect();

		let out = TableLoadGoogleSheetAuthNode
			.execute(&mut ctx, &InputMap::new(), &inputs, &fired())
			.await
			.unwrap();
		assert!(out.fired_exec.contains("on_success"));
		assert_eq!(out.data.get("row_count"), Some(&SocketValue::Int(1)));
		assert_eq!(ctx.recorded_effects.len(), 2);
		assert_eq!(ctx.recorded_effects[0].kind, "file_read");
		assert_eq!(ctx.recorded_effects[0].contents, None);
		assert_eq!(ctx.recorded_effects[1].kind, "http");
		assert_eq!(ctx.recorded_effects[1].request_body.as_ref().unwrap()["access_token"], "<redacted>");
		let _ = std::fs::remove_file(token_file);
	}

	#[tokio::test]
	async fn load_google_sheet_auth_empty_token_is_recoverable_result() {
		let mut ctx = ExecCtx {
			node_id: "sheet_auth".into(),
			..Default::default()
		};
		let inputs: InputMap = [
			("spreadsheet".into(), SocketValue::String("sheet123".into())),
			("access_token".into(), SocketValue::String(" ".into())),
			("token_env".into(), SocketValue::String(String::new())),
		]
		.into_iter()
		.collect();

		let out = TableLoadGoogleSheetAuthNode
			.execute(&mut ctx, &InputMap::new(), &inputs, &fired())
			.await
			.unwrap();
		assert!(out.fired_exec.contains("on_error"));
		let result = out.data.get("result").unwrap().as_result().unwrap();
		assert!(!result.ok);
		assert_eq!(result.code.as_deref(), Some("table.load_google_sheet_auth"));
	}

	#[tokio::test]
	async fn load_google_sheet_public_http_error_is_recoverable_result() {
		let mut mocks = EffectMocks::default();
		mocks.http.insert(
			"sheet".into(),
			HttpMockResponse {
				status: 403,
				body_text: "private".into(),
				body_json: JsonValue::Null,
				error: None,
			},
		);
		let mut ctx = ExecCtx {
			node_id: "sheet".into(),
			effect_mocks: Some(Arc::new(mocks)),
			..Default::default()
		};
		let inputs: InputMap = [("spreadsheet".into(), SocketValue::String("sheet123".into()))]
			.into_iter()
			.collect();

		let out = TableLoadGoogleSheetPublicNode
			.execute(&mut ctx, &InputMap::new(), &inputs, &fired())
			.await
			.unwrap();
		assert!(out.fired_exec.contains("on_error"));
		assert_eq!(out.data.get("row_count"), Some(&SocketValue::Int(0)));
		let result = out.data.get("result").unwrap().as_result().unwrap();
		assert!(!result.ok);
		assert_eq!(result.code.as_deref(), Some("table.load_google_sheet_public"));
	}

	#[test]
	fn escape_unescape_tsv_field_roundtrip() {
		let orig = "a\tb\nc\\d";
		let esc = escape_tsv_field(orig);
		assert!(!esc.contains('\t'));
		assert!(!esc.contains('\n'));
		assert_eq!(unescape_tsv_field(&esc), orig);
	}

	#[test]
	fn load_success_output_includes_result() {
		let table = parse_tsv_with_mode("source\treplacement\nhello\tworld\n", "headerful").unwrap();
		let out = load_success_output(table);
		assert!(out.fired_exec.contains("on_success"));
		match out.data.get("result").unwrap() {
			SocketValue::Result(result) => {
				assert!(result.ok);
				assert!(matches!(result.value.as_deref(), Some(SocketValue::Table(table)) if table.len() == 1));
			}
			other => panic!("expected result, got {other:?}"),
		}
	}

	#[test]
	fn table_error_outputs_include_result() {
		let load = err_output_load("load failed");
		match load.data.get("result").unwrap() {
			SocketValue::Result(result) => {
				assert!(!result.ok);
				assert_eq!(result.code.as_deref(), Some("table.load_tsv"));
			}
			other => panic!("expected result, got {other:?}"),
		}

		let write = err_output_write("write failed");
		match write.data.get("result").unwrap() {
			SocketValue::Result(result) => {
				assert!(!result.ok);
				assert_eq!(result.code.as_deref(), Some("table.write_tsv"));
			}
			other => panic!("expected result, got {other:?}"),
		}
	}

	#[test]
	fn write_success_output_includes_result() {
		let out = write_success_output(42);
		assert!(out.fired_exec.contains("on_success"));
		match out.data.get("result").unwrap() {
			SocketValue::Result(result) => {
				assert!(result.ok);
				assert_eq!(result.value.as_deref(), Some(&SocketValue::Int(42)));
			}
			other => panic!("expected result, got {other:?}"),
		}
	}

	#[tokio::test]
	async fn graph_asset_write_roundtrip_preserves_peer_and_unowned_tmp() {
		let root = std::env::temp_dir().join(format!("vac-table-write-{}", rand::random::<u64>()));
		let assets = root.join("pack/assets");
		std::fs::create_dir_all(&assets).unwrap();
		let path = assets.join("dictionary.tsv");
		std::fs::write(&path, "old").unwrap();
		std::fs::write(assets.join("dictionary.tsv.tmp"), "not ours").unwrap();
		std::fs::write(assets.join("peer.tsv"), "peer").unwrap();
		let mut ctx = ExecCtx {
			node_id: "main/@speaker::write".into(),
			graph_root: Some(root.clone()),
			runtime_provenance: std::collections::HashMap::from([
				("main/@speaker".into(), ("pack/module".into(), "speaker".into()))
			]),
			..Default::default()
		};
		let table = parse_tsv_with_mode("reading\tsurface\nうさぎ\tUSAGI\n", "headerful").unwrap();
		let inputs = InputMap::from([
			("path".into(), SocketValue::String("graph-file:assets/dictionary.tsv".into())),
			("table".into(), SocketValue::Table(table.clone())),
		]);
		let result = TableWriteTsvNode.execute(&mut ctx, &InputMap::new(), &inputs, &fired()).await.unwrap();
		assert!(result.fired_exec.contains("on_success"), "{result:?}");
		let loaded = TableLoadTsvNode.execute(&mut ctx, &InputMap::new(), &inputs, &fired()).await.unwrap();
		assert_eq!(loaded.data.get("table"), Some(&SocketValue::Table(table)));
		let mut guarded = inputs.clone();
		guarded.insert("expected_revision".into(), loaded.data["revision"].clone());
		let newer = "reading\tsurface\n別の更新\tUSAGI\n";
		std::fs::write(&path, newer).unwrap();
		let conflict = TableWriteTsvNode.execute(&mut ctx, &InputMap::new(), &guarded, &fired()).await.unwrap();
		assert!(conflict.fired_exec.contains("on_conflict"));
		assert!(conflict.fired_exec.contains("on_error"));
		assert!(!conflict.fired_exec.contains("on_success"));
		assert_eq!(std::fs::read_to_string(&path).unwrap(), newer);
		let refreshed = TableLoadTsvNode.execute(&mut ctx, &InputMap::new(), &inputs, &fired()).await.unwrap();
		guarded.insert("expected_revision".into(), refreshed.data["revision"].clone());
		let saved = TableWriteTsvNode.execute(&mut ctx, &InputMap::new(), &guarded, &fired()).await.unwrap();
		assert!(saved.fired_exec.contains("on_success"));
		assert_eq!(std::fs::read_to_string(assets.join("dictionary.tsv.tmp")).unwrap(), "not ours");
		assert_eq!(std::fs::read_to_string(assets.join("peer.tsv")).unwrap(), "peer");
		let mut failed = inputs.clone();
		failed.insert("path".into(), SocketValue::String("graph-file:assets".into()));
		let result = TableWriteTsvNode.execute(&mut ctx, &InputMap::new(), &failed, &fired()).await.unwrap();
		assert!(result.fired_exec.contains("on_error"));
		assert_eq!(std::fs::read_dir(root.join("pack")).unwrap().count(), 2);
		assert_eq!(std::fs::read_dir(&assets).unwrap().count(), 4);
		std::fs::remove_dir_all(root).unwrap();
	}
}
