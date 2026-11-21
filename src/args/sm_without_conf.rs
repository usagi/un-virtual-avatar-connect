use super::Args;
use crate::SharedAudioSink;
use anyhow::Result;

impl Args {
	/// conf 不要の特殊モード処理群の実行
	pub async fn execute_special_modes_without_conf(&self, audio_sink: SharedAudioSink) -> Result<()> {
		// debug が true ならログレベルを Trace に設定
		match self.debug {
			true => {
				log::set_max_level(log::LevelFilter::Trace);
				log::warn!("コマンドライン引数で -D/--debug が指定されたためログレベルが Trace に設定されます。");
				log::trace!("コマンドライン引数のパース結果: {:?}", self);
			}
			false => log::set_max_level(log::LevelFilter::Info),
		}

		if let (Some(instance_id), Some(args_json)) = (
			self.unaa_z_camera_effect_probe_instance.as_deref(),
			self.unaa_z_camera_effect_args_json.as_deref(),
		) {
			let result = execute_unaa_z_camera_effect_probe(instance_id, args_json).await;
			match result {
				Ok(value) => {
					println!("{}", serde_json::to_string_pretty(&value)?);
					std::process::exit(0);
				}
				Err(error) => {
					log::error!("UNAA/Z camera.effect probe failed: {error}");
					std::process::exit(1);
				}
			}
		}

		if self.coeiroink_speakers || self.aivisspeech_speakers || self.voicevox_speakers {
			let mut failed = false;
			for (selected, name, endpoint) in [
				(self.coeiroink_speakers, "COEIROINK", "http://127.0.0.1:50032"),
				(self.aivisspeech_speakers, "AivisSpeech", "http://127.0.0.1:10101"),
				(self.voicevox_speakers, "VOICEVOX", "http://127.0.0.1:50021"),
			] {
				if !selected {
					continue;
				}
				match super::speaker_catalog::fetch(endpoint).await {
					Ok(speakers) => println!(
						"{}",
						serde_json::to_string_pretty(&serde_json::json!({"engine": name, "speakers": speakers}))?
					),
					Err(error) => {
						log::error!("{name}: 話者一覧の取得に失敗: {error:#}");
						failed = true;
					}
				}
			}
			std::process::exit(if failed { 1 } else { 0 });
		}

		if self.test_os_tts {
			if let Err(error) = super::os_tts_test::run(&audio_sink).await {
				log::error!("OS-TTS のテストに失敗しました: {error:#}");
				std::process::exit(1);
			}
			std::process::exit(0);
		}

		if self.migrate {
			let out_dir = std::path::PathBuf::from(&self.migrate_out_dir);
			match crate::migrate::migrate_conf_file(std::path::Path::new(&self.conf), &out_dir, self.migrate_strict) {
				Ok(report) => {
					println!("{}", report.summary());
					if report.has_errors() {
						std::process::exit(1);
					}
					std::process::exit(0);
				}
				Err(e) => {
					log::error!("migrate 失敗: {e}");
					std::process::exit(1);
				}
			}
		}

		if self.validate_conf {
			let conf_path = std::path::PathBuf::from(&self.conf);
			match crate::Conf::new_noop_probe(&conf_path) {
				Ok(conf) => {
					let profile_dir = conf_path.parent().unwrap_or_else(|| std::path::Path::new("."));
					if let Err(error) = crate::ai::validate_profile_references(&conf, profile_dir) {
						log::error!("profile reference validation failed: {error}");
						std::process::exit(1);
					}
					let mut flowgraph_summary = None;
					if let Some(root) = conf.resolved_flowgraph_dir() {
						match crate::flowgraph::loader::load_flowgraph_dir(&root) {
							Ok(report) => {
								let errors = report
									.diagnostics
									.iter()
									.filter(|item| item.severity == crate::flowgraph::loader::Severity::Error)
									.count();
								if errors > 0 {
									log::error!("Flowgraph検証で{errors}件のerror: {}", root.display());
									std::process::exit(1);
								}
								flowgraph_summary = Some((root, report.graph_signature.file_count, report.capability_summary.node_count));
							}
							Err(error) => {
								log::error!("Flowgraph検証失敗: {} ({error})", root.display());
								std::process::exit(1);
							}
						}
					}
					println!("conf validation OK: {}", conf_path.display());
					if let Some((root, files, nodes)) = flowgraph_summary {
						println!("Flowgraph load OK: {} ({files} files, {nodes} nodes)", root.display());
					}
					std::process::exit(0);
				}
				Err(error) => {
					log::error!("conf validation failed: {} ({error})", conf_path.display());
					std::process::exit(1);
				}
			}
		}

		let flowgraph_mode_count =
			self.flowgraph_test_dir.is_some() as u8 + self.flowgraph_test_root.is_some() as u8 + self.flowgraph_load_dir.is_some() as u8;
		if flowgraph_mode_count > 1 {
			let message = "--flowgraph-test-dir / --flowgraph-test-root / --flowgraph-load-dir は同時に指定できません。";
			if self.flowgraph_test_json {
				println!(
					"{}",
					serde_json::to_string_pretty(&serde_json::json!({
						"ok": false,
						"error": message,
					}))?
				);
			} else {
				log::error!("{message}");
			}
			std::process::exit(1);
		}

		if let Some(dir) = &self.flowgraph_load_dir {
			let root = std::path::PathBuf::from(dir);
			match crate::flowgraph::loader::load_flowgraph_dir(&root) {
				Ok(report) => {
					let streaming_contract = crate::streaming_contract::audit_streaming_contract(&root)?;
					let error_count = report
						.diagnostics
						.iter()
						.filter(|d| d.severity == crate::flowgraph::loader::Severity::Error)
						.count();
					let warning_count = report
						.diagnostics
						.iter()
						.filter(|d| d.severity == crate::flowgraph::loader::Severity::Warning)
						.count();
					let ok = error_count == 0
						&& (!self.flowgraph_require_streaming_contract
							|| streaming_contract.status != crate::streaming_contract::ContractStatus::Blocked);
					if self.flowgraph_test_json {
						println!(
							"{}",
							serde_json::to_string_pretty(&serde_json::json!({
								"ok": ok,
								"root": root.display().to_string(),
								"file_count": report.graph_signature.file_count,
								"node_count": report.capability_summary.node_count,
								"effectful_node_count": report.capability_summary.effectful_node_count,
								"stateful_node_count": report.capability_summary.stateful_node_count,
								"required_capabilities": report.capability_summary.capabilities,
								"diagnostic_count": report.diagnostics.len(),
								"error_count": error_count,
								"warning_count": warning_count,
								"diagnostics": report.diagnostics,
								"streaming_contract": streaming_contract,
							}))?
						);
					} else {
						let status = if ok { "OK" } else { "FAILED" };
						println!("Flowgraph load {status}: {}", root.display());
						println!("  files: {}", report.graph_signature.file_count);
						println!("  nodes: {}", report.capability_summary.node_count);
						println!("  effectful_nodes: {}", report.capability_summary.effectful_node_count);
						println!("  stateful_nodes: {}", report.capability_summary.stateful_node_count);
						println!(
							"  capabilities: {}",
							format_capability_counts(&report.capability_summary.capability_counts)
						);
						println!("  diagnostics: {} error(s), {} warning(s)", error_count, warning_count);
						println!("  streaming_contract: {:?}", streaming_contract.status);
						for check in &streaming_contract.checks {
							println!("    {:?} {}: {}", check.status, check.id, check.detail);
						}
						for diagnostic in &report.diagnostics {
							println!("    {:?}: {}", diagnostic.severity, diagnostic.message);
						}
					}
					std::process::exit(if ok { 0 } else { 1 });
				}
				Err(e) => {
					if self.flowgraph_test_json {
						println!(
							"{}",
							serde_json::to_string_pretty(&serde_json::json!({
								"ok": false,
								"root": root.display().to_string(),
								"error": e.to_string(),
								"diagnostics": e.diagnostics,
							}))?
						);
					} else {
						log::error!("Flowgraph load 失敗: {e}");
					}
					std::process::exit(1);
				}
			}
		}

		if let Some(dir) = &self.flowgraph_test_dir {
			let root = std::path::PathBuf::from(dir);
			match crate::flowgraph::fixture_runner::run_fixture_once_report(&root).await {
				Ok(report) => {
					if self.flowgraph_test_json {
						println!("{}", serde_json::to_string_pretty(&report)?);
					} else {
						let status = if report.ok { "OK" } else { "FAILED" };
						println!("Flowgraph fixture {status}: {}", report.root);
						println!("  nodes: {}", report.node_count);
						println!("  stateful_nodes: {}", report.capability_summary.stateful_node_count);
						println!("  generation: {}", report.generation);
						println!(
							"  capabilities: {}",
							format_capability_counts(&report.capability_summary.capability_counts)
						);
						println!("  mocks: {}", report.mock_count);
						println!("  trace: {} line(s)", report.trace_count);
						println!("  effects: {}", report.effect_count);
						println!("  stored_values: {}", report.stored_values.len());
						println!("  state_versions: {}", report.state_versions.len());
						println!("  state_restores: {}", report.state_restore.restored_node_count);
						println!("  state_snapshots: {}", report.state_snapshots.len());
						println!("  cache: {} hit(s), {} miss(es)", report.cache_hits, report.cache_misses);
						if !report.tests.is_empty() {
							println!(
								"  tests: {} passed / {} failed",
								report.tests.len() - report.failed_tests,
								report.failed_tests
							);
							for test in &report.tests {
								if !test.ok {
									println!("    FAIL {} :: {}", test.file, test.name);
									for failure in &test.failures {
										println!("      - {failure}");
									}
								}
							}
						}
					}
					std::process::exit(if report.ok { 0 } else { 1 });
				}
				Err(e) => {
					if self.flowgraph_test_json {
						println!(
							"{}",
							serde_json::to_string_pretty(&serde_json::json!({
								"ok": false,
								"root": root.display().to_string(),
								"error": e.to_string(),
							}))?
						);
					} else {
						log::error!("Flowgraph fixture 失敗: {e}");
					}
					std::process::exit(1);
				}
			}
		}

		if let Some(root_dir) = &self.flowgraph_test_root {
			let root = std::path::PathBuf::from(root_dir);
			match crate::flowgraph::fixture_runner::run_fixture_suite_report(&root).await {
				Ok(report) => {
					if self.flowgraph_test_json {
						println!("{}", serde_json::to_string_pretty(&report)?);
					} else {
						let status = if report.ok { "OK" } else { "FAILED" };
						println!("Flowgraph fixture suite {status}: {}", report.root);
						println!("  fixtures: {} total / {} failed", report.fixture_count, report.failed_fixtures);
						println!("  tests: {} total / {} failed", report.test_count, report.failed_tests);
						println!("  triggers: {}", report.trigger_count);
						println!("  effects: {}", report.effect_count);
						println!("  state_restores: {}", report.state_restore_count);
						println!("  state_snapshots: {}", report.state_snapshot_count);
						println!("  capabilities: {}", format_suite_capability_counts(&report.reports));
						for fixture in &report.reports {
							if !fixture.ok {
								println!("    FAIL {}", fixture.root);
								for test in &fixture.tests {
									if !test.ok {
										println!("      {} :: {}", test.file, test.name);
										for failure in &test.failures {
											println!("        - {failure}");
										}
									}
								}
							}
						}
						for error in &report.errors {
							println!("    ERROR {}: {}", error.root, error.error);
						}
					}
					std::process::exit(if report.ok { 0 } else { 1 });
				}
				Err(e) => {
					if self.flowgraph_test_json {
						println!(
							"{}",
							serde_json::to_string_pretty(&serde_json::json!({
								"ok": false,
								"root": root.display().to_string(),
								"error": e.to_string(),
							}))?
						);
					} else {
						log::error!("Flowgraph fixture suite 失敗: {e}");
					}
					std::process::exit(1);
				}
			}
		}

		Ok(())
	}
}

async fn execute_unaa_z_camera_effect_probe(instance_id: &str, args_json: &str) -> Result<serde_json::Value> {
	let snapshot = crate::un_app_api::zenoh_discovery_snapshot_only().await;
	let (session_id, dispatch) =
		crate::un_app_api::prepare_camera_effect_probe_dispatch(&snapshot, instance_id, args_json).map_err(anyhow::Error::msg)?;
	let response = crate::un_app_api::publish_command_with_response_wait(&dispatch, 2_000)
		.await
		.map_err(anyhow::Error::msg)?
		.ok_or_else(|| anyhow::anyhow!("Renderer did not return a camera.effect response within 2000 ms"))?;
	if response.payload.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
		anyhow::bail!("Renderer rejected camera.effect: {}", response.payload);
	}
	Ok(serde_json::json!({
		"ok": true,
		"app_id": "un-avatar",
		"instance_id": dispatch.instance_id,
		"session_id": session_id,
		"request_id": dispatch.request_id,
		"response": response,
	}))
}

fn format_suite_capability_counts(reports: &[crate::flowgraph::fixture_runner::FixtureRunReport]) -> String {
	let mut counts = std::collections::BTreeMap::new();
	for report in reports {
		for (capability, count) in &report.capability_summary.capability_counts {
			*counts.entry(capability.clone()).or_insert(0) += count;
		}
	}
	format_capability_counts(&counts)
}

fn format_capability_counts(counts: &std::collections::BTreeMap<String, usize>) -> String {
	if counts.is_empty() {
		return "none".into();
	}
	counts
		.iter()
		.map(|(capability, count)| format!("{capability}={count}"))
		.collect::<Vec<_>>()
		.join(", ")
}
