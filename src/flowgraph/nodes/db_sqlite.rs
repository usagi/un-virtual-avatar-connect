//! SQLite read-only query node.

use crate::flowgraph::node::{
	get_optional_int, get_required_string, EffectfulNode, ExecCtx, ExecFireSet, InputMap, NodeDescriptor, NodeExecError, NodeOutput,
	NodeSpec, PortSpec, RecordedEffect,
};
use crate::flowgraph::socket::{FlowResult, SocketType, SocketValue};
use crate::flowgraph::table::{ColumnSpec, Row, Table, TableSchema};
use async_trait::async_trait;
use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{params_from_iter, Connection, OpenFlags};
use serde_json::Value as JsonValue;
use std::time::Duration;

pub struct SqliteQueryNode;

impl NodeDescriptor for SqliteQueryNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.db.sqlite.query".into(),
			title: "SQLite Query".into(),
			category: "db".into(),
			description: Some("SQLite database を read-only 接続で query し、結果を Table として返す。".into()),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("path", "Path", SocketType::String),
				PortSpec::input("sql", "SQL", SocketType::String),
				PortSpec::input("params", "Params", SocketType::Json).with_default(SocketValue::Json(JsonValue::Array(Vec::new()))),
				PortSpec::input("timeout_ms", "Timeout ms", SocketType::Int).with_default(SocketValue::Int(8000)),
			],
			outputs: vec![
				PortSpec::exec_output("on_success", "On Success"),
				PortSpec::exec_output("on_error", "On Error"),
				PortSpec::output("table", "Table", SocketType::Table),
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
impl EffectfulNode for SqliteQueryNode {
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
		let sql = get_required_string(inputs, "sql")?;
		let params = get_optional_json_input(inputs, "params", JsonValue::Array(Vec::new()))?;
		let timeout_ms = get_optional_int(inputs, "timeout_ms", 8000)?.clamp(100, 120_000) as u64;
		let request_body = serde_json::json!({
			"sql": sql,
			"params": params,
			"read_only": true,
		});

		let result = match validate_readonly_query(&sql).and_then(|_| json_params_to_sql(params)) {
			Ok(sql_params) => {
				let path_for_query = path.clone();
				let sql_for_query = sql.clone();
				tokio::task::spawn_blocking(move || query_sqlite_readonly(&path_for_query, &sql_for_query, sql_params, timeout_ms))
					.await
					.map_err(|e| format!("sqlite worker join failed: {e}"))
					.and_then(|inner| inner)
			}
			Err(e) => Err(e),
		};

		match result {
			Ok(table) => {
				let response_body = serde_json::json!({
					"row_count": table.len(),
					"column_count": table.schema().len(),
				})
				.to_string();
				ctx.record_effect(RecordedEffect::sqlite_query(
					ctx.node_id.clone(),
					path,
					request_body,
					Some(response_body),
					None,
				));
				Ok(sqlite_success_output(table))
			}
			Err(error) => {
				ctx.record_effect(RecordedEffect::sqlite_query(
					ctx.node_id.clone(),
					path,
					request_body,
					None,
					Some(error.clone()),
				));
				Ok(sqlite_error_output(error))
			}
		}
	}
}

fn validate_readonly_query(sql: &str) -> Result<(), String> {
	let trimmed = sql.trim();
	if trimmed.is_empty() {
		return Err("sql is empty".into());
	}
	let lower = trimmed.to_ascii_lowercase();
	if !(lower.starts_with("select") || lower.starts_with("with") || lower.starts_with("pragma")) {
		return Err("only SELECT / WITH / PRAGMA read-only queries are allowed".into());
	}
	if trimmed.trim_end_matches(';').contains(';') {
		return Err("multiple SQL statements are not allowed".into());
	}
	Ok(())
}

fn get_optional_json_input(inputs: &InputMap, key: &str, default: JsonValue) -> Result<JsonValue, NodeExecError> {
	match inputs.get(key) {
		None => Ok(default),
		Some(value) => value.as_json().cloned().map_err(|_| NodeExecError::TypeMismatch {
			port: key.into(),
			expected: SocketType::Json,
			actual: value.type_of(),
		}),
	}
}

fn json_params_to_sql(params: JsonValue) -> Result<Vec<SqlValue>, String> {
	let array = params.as_array().ok_or_else(|| "params must be a JSON array".to_string())?;
	array.iter().map(json_value_to_sql).collect()
}

fn json_value_to_sql(value: &JsonValue) -> Result<SqlValue, String> {
	Ok(match value {
		JsonValue::Null => SqlValue::Null,
		JsonValue::Bool(value) => SqlValue::Integer(i64::from(*value)),
		JsonValue::Number(value) => {
			if let Some(i) = value.as_i64() {
				SqlValue::Integer(i)
			} else if let Some(f) = value.as_f64() {
				SqlValue::Real(f)
			} else {
				return Err(format!("unsupported JSON number parameter: {value}"));
			}
		}
		JsonValue::String(value) => SqlValue::Text(value.clone()),
		JsonValue::Array(_) | JsonValue::Object(_) => SqlValue::Text(value.to_string()),
	})
}

fn query_sqlite_readonly(path: &str, sql: &str, params: Vec<SqlValue>, timeout_ms: u64) -> Result<Table, String> {
	let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
		.map_err(|e| format!("open read-only {path}: {e}"))?;
	conn.busy_timeout(Duration::from_millis(timeout_ms))
		.map_err(|e| format!("set busy timeout: {e}"))?;
	let mut stmt = conn.prepare(sql).map_err(|e| format!("prepare: {e}"))?;
	let column_names = stmt
		.column_names()
		.into_iter()
		.enumerate()
		.map(|(idx, name)| {
			if name.trim().is_empty() {
				format!("column{}", idx + 1)
			} else {
				name.to_string()
			}
		})
		.collect::<Vec<_>>();
	let schema = TableSchema::new(
		column_names
			.into_iter()
			.map(|name| ColumnSpec::new(name, SocketType::Json).nullable())
			.collect(),
	);
	let column_count = schema.len();
	let mut rows = stmt.query(params_from_iter(params.iter())).map_err(|e| format!("query: {e}"))?;
	let mut out = Vec::new();
	while let Some(row) = rows.next().map_err(|e| format!("read row: {e}"))? {
		let mut values = Vec::with_capacity(column_count);
		for idx in 0..column_count {
			values.push(sql_value_ref_to_json(
				row.get_ref(idx).map_err(|e| format!("read column {idx}: {e}"))?,
			));
		}
		out.push(Row::new(values));
	}
	Ok(Table::new(schema, out))
}

fn sql_value_ref_to_json(value: ValueRef<'_>) -> JsonValue {
	match value {
		ValueRef::Null => JsonValue::Null,
		ValueRef::Integer(value) => JsonValue::Number(value.into()),
		ValueRef::Real(value) => serde_json::Number::from_f64(value)
			.map(JsonValue::Number)
			.unwrap_or(JsonValue::Null),
		ValueRef::Text(value) => JsonValue::String(String::from_utf8_lossy(value).to_string()),
		ValueRef::Blob(value) => JsonValue::String(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, value)),
	}
}

fn sqlite_success_output(table: Table) -> NodeOutput {
	let row_count = table.len() as i64;
	let column_count = table.schema().len() as i64;
	NodeOutput::new()
		.set_data("table", SocketValue::Table(table.clone()))
		.set_data("row_count", SocketValue::Int(row_count))
		.set_data("column_count", SocketValue::Int(column_count))
		.set_data("error", SocketValue::String(String::new()))
		.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Table(table))))
		.fire_exec("on_success")
}

fn sqlite_error_output(error: impl Into<String>) -> NodeOutput {
	let error = error.into();
	NodeOutput::new()
		.set_data("table", SocketValue::Table(Table::empty()))
		.set_data("row_count", SocketValue::Int(0))
		.set_data("column_count", SocketValue::Int(0))
		.set_data("error", SocketValue::String(error.clone()))
		.set_data("result", SocketValue::Result(FlowResult::err(error).with_code("db.sqlite.query")))
		.fire_exec("on_error")
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::node::EffectMocks;
	use std::sync::Arc;
	use std::time::{SystemTime, UNIX_EPOCH};

	fn fired() -> ExecFireSet {
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		fired
	}

	fn temp_db_path(name: &str) -> std::path::PathBuf {
		let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
		std::env::temp_dir().join(format!("unvac-{name}-{nanos}.sqlite3"))
	}

	fn create_sample_db() -> std::path::PathBuf {
		let path = temp_db_path("query");
		let conn = Connection::open(&path).unwrap();
		conn.execute(
			"create table scenes (name text not null, weight integer not null, enabled integer not null)",
			[],
		)
		.unwrap();
		conn.execute("insert into scenes values (?1, ?2, ?3)", ("opening", 2, 1)).unwrap();
		conn.execute("insert into scenes values (?1, ?2, ?3)", ("ending", 3, 0)).unwrap();
		path
	}

	#[tokio::test]
	async fn sqlite_query_returns_table_with_bound_params() {
		let path = create_sample_db();
		let mut ctx = ExecCtx {
			node_id: "sqlite".into(),
			effect_mocks: Some(Arc::new(EffectMocks::default())),
			..Default::default()
		};
		let inputs: InputMap = [
			("path".into(), SocketValue::String(path.to_string_lossy().to_string())),
			(
				"sql".into(),
				SocketValue::String("select name, weight from scenes where weight > ?1 order by weight".into()),
			),
			("params".into(), SocketValue::Json(serde_json::json!([1]))),
		]
		.into_iter()
		.collect();

		let out = SqliteQueryNode
			.execute(&mut ctx, &InputMap::new(), &inputs, &fired())
			.await
			.unwrap();
		assert!(out.fired_exec.contains("on_success"));
		assert_eq!(out.data.get("row_count"), Some(&SocketValue::Int(2)));
		assert_eq!(out.data.get("column_count"), Some(&SocketValue::Int(2)));
		let table = out.data.get("table").unwrap().as_table().unwrap();
		assert_eq!(table.schema().column_names().collect::<Vec<_>>(), vec!["name", "weight"]);
		assert_eq!(table.rows()[0].get(0).and_then(JsonValue::as_str), Some("opening"));
		assert_eq!(ctx.recorded_effects.len(), 1);
		assert_eq!(ctx.recorded_effects[0].kind, "sqlite_query");
		let _ = std::fs::remove_file(path);
	}

	#[tokio::test]
	async fn sqlite_query_rejects_write_statement_before_opening_db() {
		let mut ctx = ExecCtx {
			node_id: "sqlite".into(),
			..Default::default()
		};
		let inputs: InputMap = [
			("path".into(), SocketValue::String("missing.sqlite3".into())),
			("sql".into(), SocketValue::String("delete from scenes".into())),
		]
		.into_iter()
		.collect();

		let out = SqliteQueryNode
			.execute(&mut ctx, &InputMap::new(), &inputs, &fired())
			.await
			.unwrap();
		assert!(out.fired_exec.contains("on_error"));
		let result = out.data.get("result").unwrap().as_result().unwrap();
		assert!(!result.ok);
		assert_eq!(result.code.as_deref(), Some("db.sqlite.query"));
		assert!(out.data.get("error").unwrap().as_str().unwrap().contains("only SELECT"));
		assert_eq!(ctx.recorded_effects.len(), 1);
	}
}
