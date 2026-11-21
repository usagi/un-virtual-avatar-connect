//! v2 Flowgraph composition: typed graph interface と graph-as-node の load-time 展開。

use super::diagnostic::{Diagnostic, DiagnosticCode};
use super::file::{EdgeEntry, FlowgraphFile, GraphKind};
use super::reference::{parse_port_ref, resolve_fq_ref, PortRefStr, ResolveContext};
use crate::flowgraph::registry::NodeRegistry;
use crate::flowgraph::SocketType;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct GraphCompositionSummary {
	pub graphs: Vec<GraphDefinitionSummary>,
	pub instances: Vec<GraphInstanceSummary>,
	pub portals: Vec<CrossFilePortalSummary>,
}

impl GraphCompositionSummary {
	pub fn runtime_provenance(&self) -> HashMap<String, (String, String)> {
		let mut out = self
			.graphs
			.iter()
			.filter(|graph| graph.kind == GraphKind::Entry)
			.map(|graph| (graph.fq.clone(), (graph.fq.clone(), String::new())))
			.collect::<HashMap<_, _>>();
		for instance in &self.instances {
			let path = instance
				.runtime_fq
				.split_once("/@")
				.map_or(String::new(), |(_, path)| path.replace("/@", "/"));
			out.insert(instance.runtime_fq.clone(), (instance.graph_fq.clone(), path));
		}
		out
	}
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphDefinitionSummary {
	pub fq: String,
	pub kind: GraphKind,
	pub inputs: Vec<GraphInterfacePortSummary>,
	pub outputs: Vec<GraphInterfacePortSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphInterfacePortSummary {
	pub name: String,
	pub ty: String,
	pub mapping: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphInstanceSummary {
	pub owner_fq: String,
	pub id: String,
	pub graph_fq: String,
	pub runtime_fq: String,
	pub position: Option<[f64; 2]>,
	#[serde(default = "crate::utility::bool_true")]
	pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrossFilePortalSummary {
	pub owner_fq: String,
	pub from: String,
	pub to: String,
	pub from_fq: String,
	pub to_fq: String,
	#[serde(default = "crate::utility::bool_true")]
	pub from_active: bool,
	#[serde(default = "crate::utility::bool_true")]
	pub to_active: bool,
	#[serde(default = "crate::utility::bool_true")]
	pub active: bool,
}

type FileTriple = (String, PathBuf, FlowgraphFile);

fn normalize_manifest_fq(value: &str) -> String {
	let mut value = value.trim().replace('\\', "/");
	while let Some(stripped) = value.strip_prefix("./") {
		value = stripped.to_string();
	}
	if let Some(stripped) = value.strip_suffix(".flowgraph.toml") {
		value = stripped.to_string();
	}
	value.trim_matches('/').to_string()
}

struct Expander<'a> {
	definitions: HashMap<&'a str, (&'a PathBuf, &'a FlowgraphFile)>,
	packages: Vec<PackageBoundary>,
	known: HashSet<String>,
	reg: &'a NodeRegistry,
	diagnostics: Vec<Diagnostic>,
	expanded: Vec<FileTriple>,
	summary: GraphCompositionSummary,
}

#[derive(Debug, Clone)]
struct PackageBoundary {
	id: String,
	scope: String,
	exports: HashSet<String>,
	dependencies: HashSet<String>,
}

pub(crate) fn expand_composition(
	files: &[FileTriple],
	known: &HashSet<String>,
	reg: &NodeRegistry,
) -> Result<(Vec<FileTriple>, GraphCompositionSummary, Vec<Diagnostic>), Vec<Diagnostic>> {
	let definitions = files
		.iter()
		.map(|(fq, path, file)| (fq.as_str(), (path, file)))
		.collect::<HashMap<_, _>>();
	let packages = files
		.iter()
		.filter_map(|(fq, _, file)| {
			let package = file.package.as_ref()?;
			let id = package.id.clone()?;
			Some(PackageBoundary {
				id,
				scope: fq.rsplit_once('/').map_or(String::new(), |(scope, _)| scope.to_string()),
				exports: package.exports.iter().map(|value| normalize_manifest_fq(value)).collect(),
				dependencies: package.dependencies.keys().cloned().collect(),
			})
		})
		.collect();
	let mut expander = Expander {
		definitions,
		packages,
		known: known.clone(),
		reg,
		diagnostics: Vec::new(),
		expanded: Vec::new(),
		summary: GraphCompositionSummary::default(),
	};

	for (fq, _, file) in files {
		expander.summary.graphs.push(GraphDefinitionSummary {
			fq: fq.clone(),
			kind: file.graph.kind,
			inputs: file
				.graph
				.inputs
				.iter()
				.map(|port| GraphInterfacePortSummary {
					name: port.name.clone(),
					ty: port.ty.clone(),
					mapping: port.to.clone(),
				})
				.collect(),
			outputs: file
				.graph
				.outputs
				.iter()
				.map(|port| GraphInterfacePortSummary {
					name: port.name.clone(),
					ty: port.ty.clone(),
					mapping: port.from.clone(),
				})
				.collect(),
		});
	}

	for (fq, path, file) in files {
		if file.graph.kind == GraphKind::Entry {
			expander.expand_placement(fq, fq, path, file, Vec::new(), &mut Vec::new());
		}
	}
	let mut instance_counts: HashMap<String, usize> = HashMap::new();
	for instance in &expander.summary.instances {
		*instance_counts.entry(instance.graph_fq.clone()).or_default() += 1;
	}
	for (graph_fq, count) in instance_counts {
		if count < 2 {
			continue;
		}
		let Some((path, graph)) = expander.definitions.get(graph_fq.as_str()).copied() else {
			continue;
		};
		let autonomous = graph
			.nodes
			.iter()
			.find(|node| node.feature.starts_with("flowgraph.ingress.") || node.feature == "flowgraph.util.timer_interval");
		if let Some(node) = autonomous {
			expander.diagnostics.push(
				Diagnostic::warning(
					DiagnosticCode::MultipleAutonomousSourceInstances,
					format!(
						"自律source '{}' を含むmodule '{}' が {} instance配置されている。外部購読・timer・bindの重複を確認すること",
						node.feature, graph_fq, count
					),
				)
				.with_file(path.clone())
				.with_node(node.id.clone()),
			);
		}
	}
	expander.summary.graphs.sort_by(|a, b| a.fq.cmp(&b.fq));
	expander.summary.instances.sort_by(|a, b| a.runtime_fq.cmp(&b.runtime_fq));
	expander
		.summary
		.portals
		.sort_by(|a, b| (&a.owner_fq, &a.from, &a.to).cmp(&(&b.owner_fq, &b.from, &b.to)));
	if expander.diagnostics.iter().any(|diag| diag.severity == super::Severity::Error) {
		Err(expander.diagnostics)
	} else {
		Ok((expander.expanded, expander.summary, expander.diagnostics))
	}
}

impl Expander<'_> {
	fn expand_placement(
		&mut self,
		source_fq: &str,
		runtime_fq: &str,
		path: &PathBuf,
		source: &FlowgraphFile,
		inherited_activation: Vec<super::diagnostic::FlowgraphFileActivationMeta>,
		stack: &mut Vec<String>,
	) {
		if let Some(pos) = stack.iter().position(|item| item == source_fq) {
			let mut cycle = stack[pos..].to_vec();
			cycle.push(source_fq.to_string());
			self.diagnostics.push(
				Diagnostic::error(
					DiagnosticCode::GraphInstanceCycle,
					format!("graph instance に閉路: {}", cycle.join(" -> ")),
				)
				.with_file(path.clone()),
			);
			return;
		}
		stack.push(source_fq.to_string());
		let mut activation_chain = inherited_activation;
		if let Some(meta) = source.meta.as_ref() {
			activation_chain.push(meta.activation_meta());
		}

		self.validate_interface(source_fq, path, source);
		let mut instance_targets: HashMap<String, (String, String)> = HashMap::new();
		let mut ids: HashSet<String> = source.nodes.iter().map(|node| node.id.clone()).collect();
		for instance in &source.instances {
			if instance.id.trim().is_empty() || instance.id != instance.id.trim() || !ids.insert(instance.id.clone()) {
				self.diagnostics.push(
					Diagnostic::error(
						DiagnosticCode::InvalidGraphInstance,
						format!("instance id '{}' が空、不正、またはnode/instanceと重複", instance.id),
					)
					.with_file(path.clone())
					.with_node(instance.id.clone()),
				);
				continue;
			}
			let Some(target_fq) = self.resolve_graph_ref(source_fq, &instance.graph, path) else {
				continue;
			};
			let Some((target_path, target)) = self.definitions.get(target_fq.as_str()).copied() else {
				self.diagnostics.push(
					Diagnostic::error(
						DiagnosticCode::InvalidGraphInstance,
						format!("instance '{}' のgraph '{}'が解決不能", instance.id, target_fq),
					)
					.with_file(path.clone())
					.with_node(instance.id.clone()),
				);
				continue;
			};
			if !self.validate_package_access(source_fq, &target_fq, path, &instance.id) {
				continue;
			}
			if target.graph.kind != GraphKind::Module {
				self.diagnostics.push(
					Diagnostic::error(
						DiagnosticCode::InvalidGraphInstance,
						format!("entry graph '{}' はinstance化できない", target_fq),
					)
					.with_file(path.clone())
					.with_node(instance.id.clone()),
				);
				continue;
			}
			let child_runtime_fq = format!("{runtime_fq}/@{}", instance.id);
			instance_targets.insert(instance.id.clone(), (target_fq.clone(), child_runtime_fq.clone()));
			self.summary.instances.push(GraphInstanceSummary {
				owner_fq: runtime_fq.to_string(),
				id: instance.id.clone(),
				graph_fq: target_fq.clone(),
				runtime_fq: child_runtime_fq.clone(),
				position: instance.position,
				active: true,
			});
			self.expand_placement(&target_fq, &child_runtime_fq, target_path, target, activation_chain.clone(), stack);
		}

		let mut placed = source.clone();
		placed.graph = Default::default();
		placed.instances.clear();
		placed.groups.clear();
		placed.package = None;
		placed.activation_chain = activation_chain.clone();
		if source.graph.kind == GraphKind::Module {
			placed.meta = combined_activation_meta(&activation_chain);
		}
		placed.edges = source
			.edges
			.iter()
			.filter_map(|edge| {
				let from = self.rewrite_endpoint(source_fq, runtime_fq, path, source, &instance_targets, &edge.from, false)?;
				let to = self.rewrite_endpoint(source_fq, runtime_fq, path, source, &instance_targets, &edge.to, true)?;
				let from_fq = endpoint_fq(&from, runtime_fq);
				let to_fq = endpoint_fq(&to, runtime_fq);
				if from_fq != to_fq {
					self.summary.portals.push(CrossFilePortalSummary {
						owner_fq: runtime_fq.to_string(),
						from: from.clone(),
						to: to.clone(),
						from_fq,
						to_fq,
						from_active: true,
						to_active: true,
						active: true,
					});
				}
				Some(EdgeEntry { from, to })
			})
			.collect();
		self.expanded.push((runtime_fq.to_string(), path.clone(), placed));
		stack.pop();
	}

	fn package_for(&self, fq: &str) -> Option<&PackageBoundary> {
		self.packages
			.iter()
			.filter(|package| package.scope.is_empty() || fq == package.scope || fq.starts_with(&format!("{}/", package.scope)))
			.max_by_key(|package| package.scope.len())
	}

	fn validate_package_access(&mut self, source_fq: &str, target_fq: &str, path: &PathBuf, instance_id: &str) -> bool {
		let source_package = self.package_for(source_fq).cloned();
		let target_package = self.package_for(target_fq).cloned();
		let Some(target_package) = target_package else {
			return true;
		};
		if source_package.as_ref().is_some_and(|package| package.id == target_package.id) {
			return true;
		}
		if !target_package.exports.contains(target_fq) {
			self.diagnostics.push(
				Diagnostic::error(
					DiagnosticCode::GraphEncapsulation,
					format!(
						"package '{}' の非export module '{}' はpackage外からinstance化できない",
						target_package.id, target_fq
					),
				)
				.with_file(path.clone())
				.with_node(instance_id.to_string())
				.with_hint(format!("[package].exports に '{target_fq}' を追加")),
			);
			return false;
		}
		if let Some(source_package) = source_package {
			if !source_package.dependencies.contains(&target_package.id) {
				self.diagnostics.push(
					Diagnostic::error(
						DiagnosticCode::InvalidPackageManifest,
						format!(
							"package '{}' から '{}' を参照するには [package].dependencies の宣言が必要",
							source_package.id, target_package.id
						),
					)
					.with_file(path.clone())
					.with_node(instance_id.to_string())
					.with_hint(format!("[package].dependencies.{}", target_package.id)),
				);
				return false;
			}
		}
		true
	}

	fn resolve_graph_ref(&mut self, current_fq: &str, raw: &str, path: &PathBuf) -> Option<String> {
		let parsed = PortRefStr {
			fq_path: Some(raw.to_string()),
			node_id: "_".into(),
			port: "_".into(),
		};
		let ctx = ResolveContext {
			current_file_fq: current_fq.to_string(),
			known_file_fqs: self.known.clone(),
		};
		match resolve_fq_ref(&parsed, &ctx) {
			Ok(fq) => Some(fq),
			Err(error) => {
				self.diagnostics.push(
					Diagnostic::error(DiagnosticCode::InvalidGraphInstance, format!("graph参照 '{raw}' が不正: {error}"))
						.with_file(path.clone()),
				);
				None
			}
		}
	}

	#[allow(clippy::too_many_arguments)]
	fn rewrite_endpoint(
		&mut self,
		source_fq: &str,
		_runtime_fq: &str,
		path: &PathBuf,
		_source: &FlowgraphFile,
		instances: &HashMap<String, (String, String)>,
		raw: &str,
		input_side: bool,
	) -> Option<String> {
		let parsed = match parse_port_ref(raw) {
			Ok(parsed) => parsed,
			Err(_) => return Some(raw.to_string()), // 通常loaderが詳細診断する。
		};
		if parsed.fq_path.is_none() {
			if let Some((target_fq, child_runtime_fq)) = instances.get(&parsed.node_id) {
				let (_, target) = self.definitions.get(target_fq.as_str()).copied()?;
				let mapping = if input_side {
					target
						.graph
						.inputs
						.iter()
						.find(|port| port.name == parsed.port)
						.map(|port| port.to.as_str())
				} else {
					target
						.graph
						.outputs
						.iter()
						.find(|port| port.name == parsed.port)
						.map(|port| port.from.as_str())
				};
				let Some(mapping) = mapping else {
					self.diagnostics.push(
						Diagnostic::error(
							DiagnosticCode::InvalidGraphInterface,
							format!(
								"instance '{}': interface {} '{}' が存在しない",
								parsed.node_id,
								if input_side { "input" } else { "output" },
								parsed.port
							),
						)
						.with_file(path.clone())
						.with_hint(raw.to_string()),
					);
					return None;
				};
				let mapped = match parse_port_ref(mapping) {
					Ok(mapped) if mapped.fq_path.is_none() => mapped,
					_ => return None,
				};
				return Some(format!("{child_runtime_fq}::{}:{}", mapped.node_id, mapped.port));
			}
			return Some(raw.to_string());
		}

		let ctx = ResolveContext {
			current_file_fq: source_fq.to_string(),
			known_file_fqs: self.known.clone(),
		};
		let target_fq = match resolve_fq_ref(&parsed, &ctx) {
			Ok(fq) => fq,
			Err(_) => return Some(raw.to_string()),
		};
		if self
			.definitions
			.get(target_fq.as_str())
			.is_some_and(|(_, file)| file.graph.kind == GraphKind::Module)
		{
			self.diagnostics.push(
				Diagnostic::error(
					DiagnosticCode::GraphEncapsulation,
					format!("module '{}' の内部nodeはinstance interface外から参照できない", target_fq),
				)
				.with_file(path.clone())
				.with_hint(raw.to_string()),
			);
			return None;
		}
		Some(format!("{target_fq}::{}:{}", parsed.node_id, parsed.port))
	}

	fn validate_interface(&mut self, fq: &str, path: &PathBuf, file: &FlowgraphFile) {
		let nodes = file.nodes.iter().map(|node| (node.id.as_str(), node)).collect::<HashMap<_, _>>();
		let mut input_names = HashSet::new();
		for port in &file.graph.inputs {
			if !input_names.insert(port.name.as_str()) {
				self.interface_error(path, format!("graph input '{}' が重複", port.name), &port.name);
				continue;
			}
			self.validate_interface_mapping(fq, path, &nodes, &port.name, &port.ty, &port.to, true);
		}
		let mut output_names = HashSet::new();
		for port in &file.graph.outputs {
			if !output_names.insert(port.name.as_str()) {
				self.interface_error(path, format!("graph output '{}' が重複", port.name), &port.name);
				continue;
			}
			self.validate_interface_mapping(fq, path, &nodes, &port.name, &port.ty, &port.from, false);
		}
	}

	#[allow(clippy::too_many_arguments)]
	fn validate_interface_mapping(
		&mut self,
		_fq: &str,
		path: &PathBuf,
		nodes: &HashMap<&str, &super::file::NodeEntry>,
		name: &str,
		declared_type: &str,
		mapping: &str,
		input: bool,
	) {
		let declared = if declared_type == "exec" {
			None
		} else {
			match SocketType::parse(declared_type) {
				Ok(ty) => Some(ty),
				Err(error) => {
					self.interface_error(path, format!("interface '{name}' の型 '{declared_type}' が不正: {error}"), name);
					return;
				}
			}
		};
		let parsed = match parse_port_ref(mapping) {
			Ok(parsed) if parsed.fq_path.is_none() => parsed,
			_ => {
				self.interface_error(
					path,
					format!("interface '{name}' のmappingは同一file内node:portでなければならない: '{mapping}'"),
					name,
				);
				return;
			}
		};
		let Some(node) = nodes.get(parsed.node_id.as_str()) else {
			self.interface_error(path, format!("interface '{name}' のnode '{}' が存在しない", parsed.node_id), name);
			return;
		};
		let Some(spec) = self.reg.spec(&node.feature) else {
			return; // 通常loaderの unknown feature 診断へ委譲。
		};
		let actual = if input {
			spec.find_input(&parsed.port)
		} else {
			spec.find_output(&parsed.port)
		};
		let Some(actual) = actual else {
			self.interface_error(
				path,
				format!("interface '{name}' のmapping port '{}' が存在しない", parsed.port),
				name,
			);
			return;
		};
		let declared_exec = declared_type == "exec";
		if declared_exec != actual.is_exec
			|| (!declared_exec
				&& !declared
					.as_ref()
					.is_some_and(|ty| ty.compatible_with(&actual.ty) && actual.ty.compatible_with(ty)))
		{
			self.interface_error(
				path,
				format!(
					"interface '{name}' の宣言型 '{declared_type}' とmapping port型 '{}' が一致しない",
					actual.ty
				),
				name,
			);
		}
	}

	fn interface_error(&mut self, path: &PathBuf, message: String, hint: &str) {
		self.diagnostics.push(
			Diagnostic::error(DiagnosticCode::InvalidGraphInterface, message)
				.with_file(path.clone())
				.with_hint(hint.to_string()),
		);
	}
}

fn endpoint_fq(raw: &str, local_fq: &str) -> String {
	parse_port_ref(raw)
		.ok()
		.and_then(|parsed| parsed.fq_path)
		.unwrap_or_else(|| local_fq.to_string())
}

fn combined_activation_meta(chain: &[super::diagnostic::FlowgraphFileActivationMeta]) -> Option<super::file::FileMeta> {
	if chain.is_empty() {
		return None;
	}
	let mut mode_groups = Vec::new();
	for meta in chain {
		for group in &meta.mode_groups {
			if !mode_groups.contains(group) {
				mode_groups.push(group.clone());
			}
		}
	}
	Some(super::file::FileMeta {
		mode_groups,
		default_enabled: chain.iter().all(|meta| meta.default_enabled),
		..Default::default()
	})
}
