//! Read-only composition dependency discovery for a future share preview.
//! This is not an export approval: node-specific types, dynamic
//! references and assets still require their own analysis and user selection.
use super::{load_all_files, normalize_fq_str, CopyError, LoadedOnDisk};
use crate::flowgraph::loader::dir::normalize_manifest_fq;
use crate::flowgraph::loader::dir::{package_id_is_valid, package_version_is_valid, package_version_requirement_is_valid};
use crate::flowgraph::loader::{file::GraphKind, parse_port_ref};
use crate::flowgraph::loader::{resolve_fq_ref, PortRefStr, ResolveContext};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct CompositionSharePlan {
	pub files: Vec<String>,
	pub references: Vec<CompositionReference>,
	pub package_references: Vec<PackageReference>,
	pub static_references: Vec<StaticReference>,
	pub type_references: Vec<TypeReference>,
	pub asset_candidates: Vec<StaticReference>,
	// Always false: composition closure alone does not prove a portable feature.
	pub portable_feature_verified: bool,
	pub unverified: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct CompositionReference {
	pub from_file: String,
	pub instance_id: String,
	pub graph: String,
	pub target: Option<String>,
	pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PackageReference {
	pub from_file: String,
	pub package_id: String,
	pub requirement: String,
	pub candidates: Vec<String>,
	pub target: Option<String>,
	pub error: Option<String>,
}

fn package_reference(loaded: &BTreeMap<String, LoadedOnDisk>, from: &str, id: &str, requirement: &str) -> PackageReference {
	let candidates: Vec<String> = loaded
		.iter()
		.filter(|(_, f)| f.doc.package.as_ref().and_then(|p| p.id.as_deref()) == Some(id))
		.map(|(fq, _)| fq.clone())
		.collect();
	let error = if !package_id_is_valid(id) {
		Some("Invalid package ID".into())
	} else if !package_version_requirement_is_valid(requirement) {
		Some("Unsupported version requirement".into())
	} else if loaded[from].doc.package.as_ref().and_then(|p| p.id.as_deref()) == Some(id) {
		Some("Package depends on itself".into())
	} else if candidates.is_empty() {
		Some("Package is missing".into())
	} else if candidates.len() != 1 {
		Some("Package ID has multiple definitions".into())
	} else {
		let version = loaded[&candidates[0]].doc.package.as_ref().and_then(|p| p.version.as_deref());
		if version.is_some_and(|v| !package_version_is_valid(v)) {
			Some("Invalid package version".into())
		} else if requirement != "*" && version != Some(requirement) {
			Some(format!(
				"Version mismatch: required {requirement}, found {}",
				version.unwrap_or("<none>")
			))
		} else {
			None
		}
	};
	let target = if error.is_none() { candidates.first().cloned() } else { None };
	PackageReference {
		from_file: from.into(),
		package_id: id.into(),
		requirement: requirement.into(),
		candidates,
		target,
		error,
	}
}

#[derive(Debug, Serialize)]
pub struct StaticReference {
	pub from_file: String,
	pub field: String,
	pub raw: String,
	pub target: Option<String>,
	pub error: Option<String>,
}

fn static_references(loaded: &BTreeMap<String, LoadedOnDisk>, fq: &str, context: &ResolveContext) -> Vec<StaticReference> {
	let doc = &loaded[fq].doc;
	let mut rows = Vec::new();
	let mut record = |field: String, raw: &str, resolved: Result<String, String>| {
		let resolved = resolved.and_then(|target| {
			if loaded.contains_key(&target) {
				Ok(target)
			} else {
				Err(format!("Graph file not found: {target}"))
			}
		});
		let (target, error) = match resolved {
			Ok(target) => (Some(target), None),
			Err(error) => (None, Some(error)),
		};
		rows.push(StaticReference {
			from_file: fq.into(),
			field,
			raw: raw.into(),
			target,
			error,
		});
	};
	for (index, edge) in doc.edges.iter().enumerate() {
		for (side, raw) in [("from", &edge.from), ("to", &edge.to)] {
			record(
				format!("edges[{index}].{side}"),
				raw,
				parse_port_ref(raw).and_then(|p| resolve_fq_ref(&p, context)),
			);
		}
	}
	for (index, input) in doc.graph.inputs.iter().enumerate() {
		record(
			format!("graph.inputs[{index}].to"),
			&input.to,
			parse_port_ref(&input.to).and_then(|p| resolve_fq_ref(&p, context)),
		);
	}
	for (index, output) in doc.graph.outputs.iter().enumerate() {
		record(
			format!("graph.outputs[{index}].from"),
			&output.from,
			parse_port_ref(&output.from).and_then(|p| resolve_fq_ref(&p, context)),
		);
	}
	if let Some(package) = &doc.package {
		let scope = fq.rsplit_once('/').map_or("", |(scope, _)| scope);
		for (index, raw) in package.exports.iter().enumerate() {
			let target = normalize_manifest_fq(raw);
			let resolved = if target.is_empty() {
				Err("Empty package export".into())
			} else if !scope.is_empty() && target != scope && !target.starts_with(&format!("{scope}/")) {
				Err("Package export is outside package scope".into())
			} else if loaded.get(&target).is_some_and(|f| f.doc.graph.kind != GraphKind::Module) {
				Err("Package export is not a module graph".into())
			} else {
				Ok(target)
			};
			record(format!("package.exports[{index}]"), raw, resolved);
		}
	}
	rows
}

#[derive(Debug, Serialize)]
pub struct TypeReference {
	pub from_file: String,
	pub field: String,
	pub expression: String,
	pub schema_id: Option<String>,
	pub candidates: Vec<String>,
	pub target: Option<String>,
	pub error: Option<String>,
}

fn type_references(loaded: &BTreeMap<String, LoadedOnDisk>, fq: &str) -> Vec<TypeReference> {
	let doc = &loaded[fq].doc;
	let mut expressions = Vec::new();
	for (index, ty) in doc.types.iter().enumerate() {
		for (field, expression) in &ty.fields {
			expressions.push((format!("types[{index}].fields.{field}"), expression));
		}
	}
	for (index, port) in doc.graph.inputs.iter().enumerate() {
		expressions.push((format!("graph.inputs[{index}].type"), &port.ty));
	}
	for (index, port) in doc.graph.outputs.iter().enumerate() {
		expressions.push((format!("graph.outputs[{index}].type"), &port.ty));
	}
	let mut rows = Vec::new();
	for (field, expression) in expressions {
		let parsed = match crate::flowgraph::SocketType::parse(expression) {
			Ok(parsed) => parsed,
			Err(error) => {
				rows.push(TypeReference {
					from_file: fq.into(),
					field,
					expression: expression.clone(),
					schema_id: None,
					candidates: vec![],
					target: None,
					error: Some(error.to_string()),
				});
				continue;
			}
		};
		let mut ids = BTreeSet::new();
		crate::flowgraph::loader::file::collect_record_schema_refs(&parsed, &mut ids);
		for id in ids {
			// Keep duplicate declarations even within one file; never select one silently.
			let candidates: Vec<String> = loaded
				.iter()
				.flat_map(|(file, data)| data.doc.types.iter().filter(|ty| ty.id == id).map(move |_| file.clone()))
				.collect();
			let error = match candidates.len() {
				0 => Some("Record schema is missing".into()),
				1 => None,
				_ => Some("Record schema has multiple definitions".into()),
			};
			let target = if error.is_none() { candidates.first().cloned() } else { None };
			rows.push(TypeReference {
				from_file: fq.into(),
				field: field.clone(),
				expression: expression.clone(),
				schema_id: Some(id),
				candidates,
				target,
				error,
			});
		}
	}
	rows
}

/// Package declarations selected for deletion that still own retained files.
/// Prospective loading alone cannot detect losing a package boundary.
pub fn retained_package_owners(root: &Path, excluded: &std::collections::HashSet<String>) -> Result<Vec<StaticReference>, CopyError> {
	let files = super::load_files_for_removal(root, excluded)?;
	let retained = files.keys().filter(|fq| !excluded.contains(*fq)).cloned().collect::<Vec<_>>();
	if retained.is_empty() { return Ok(Vec::new()); }
	Ok(plan(&files, &retained)?.static_references.into_iter().filter(|reference|
		reference.field == "package.owner" && !excluded.contains(&reference.from_file)
			&& reference.target.as_ref().is_some_and(|owner| excluded.contains(owner))
	).collect())
}

pub fn plan_composition_share(root: &Path, seeds: &[String]) -> Result<CompositionSharePlan, CopyError> {
	if seeds.is_empty() {
		return Err(CopyError::NoTargets);
	}
	let files = load_all_files(root)?;
	let mut result = plan(&files, seeds)?;
	for fq in &result.files {
		let ctx = crate::flowgraph::node::ExecCtx {
			graph_root: Some(root.to_path_buf()), node_id: format!("{fq}::__share"), ..Default::default()
		};
		for node in &files[fq].doc.nodes {
			for (key, value) in &node.properties {
				collect_asset_candidates(&ctx, fq, &format!("{}.properties.{key}", node.id), value, &mut result.asset_candidates);
			}
		}
	}
	Ok(result)
}

// Literal occurrences are candidates, not proof of runtime use. Never evaluate
// arbitrary nodes or include files automatically while preparing a share plan.
fn collect_asset_candidates(ctx: &crate::flowgraph::node::ExecCtx, fq: &str, field: &str, value: &toml::Value, out: &mut Vec<StaticReference>) {
	match value {
		toml::Value::String(raw) if raw.starts_with("graph-file:") => {
			let resolved = ctx.resolve_read_path(raw).and_then(|path| {
				if !path.is_file() { return Err("Asset is not a regular file".into()); }
				let root = ctx.graph_root.as_ref().unwrap().canonicalize().map_err(|e| e.to_string())?;
				path.strip_prefix(root).map(|p| p.to_string_lossy().replace('\\', "/")).map_err(|e| e.to_string())
			});
			let (target, error) = match resolved { Ok(p) => (Some(p), None), Err(e) => (None, Some(e)) };
			out.push(StaticReference { from_file: fq.into(), field: field.into(), raw: raw.clone(), target, error });
		}
		toml::Value::Array(values) => for (i, value) in values.iter().enumerate() { collect_asset_candidates(ctx, fq, &format!("{field}[{i}]"), value, out); },
		toml::Value::Table(values) => for (key, value) in values { collect_asset_candidates(ctx, fq, &format!("{field}.{key}"), value, out); },
		_ => {}
	}
}

fn plan(loaded: &BTreeMap<String, LoadedOnDisk>, seeds: &[String]) -> Result<CompositionSharePlan, CopyError> {
	let mut pending = BTreeSet::new();
	for seed in seeds {
		let fq = normalize_fq_str(seed)?;
		if !loaded.contains_key(&fq) {
			return Err(CopyError::FileNotFound(fq));
		}
		pending.insert(fq);
	}
	let mut visited = BTreeSet::new();
	let mut references = Vec::new();
	let mut package_references = Vec::new();
	let mut static_refs = Vec::new();
	let mut type_refs = Vec::new();
	while let Some(fq) = pending.pop_first() {
		if !visited.insert(fq.clone()) {
			continue;
		}
		// A module's package boundary is declared in another file. Preserve the
		// nearest enclosing manifest, even when the seed is only that module.
		// Match loader/composition.rs ownership (longest containing scope).
		let owners: Vec<_> = loaded.iter().filter_map(|(owner, file)| {
			file.doc.package.as_ref()?;
			let scope = owner.rsplit_once('/').map_or("", |(scope, _)| scope);
			(scope.is_empty() || fq == scope || fq.starts_with(&format!("{scope}/")))
				.then_some((owner, scope.len()))
		}).collect();
		if let Some(depth) = owners.iter().map(|(_, depth)| *depth).max() {
			for (owner, _) in owners.iter().filter(|(_, length)| *length == depth) {
				if *owner == &fq { continue; }
				if !visited.contains(*owner) { pending.insert((*owner).clone()); }
				static_refs.push(StaticReference {
					from_file: fq.clone(), field: "package.owner".into(),
					raw: (*owner).clone(), target: Some((*owner).clone()), error: None,
				});
			}
		}
		if let Some(package) = &loaded[&fq].doc.package {
			for (id, requirement) in &package.dependencies {
				let reference = package_reference(loaded, &fq, id, requirement);
				if let Some(target) = &reference.target {
					if !visited.contains(target) {
						pending.insert(target.clone());
					}
				}
				package_references.push(reference);
			}
		}
		let context = ResolveContext {
			current_file_fq: fq.clone(),
			known_file_fqs: loaded.keys().cloned().collect(),
		};
		let types = type_references(loaded, &fq);
		for reference in &types {
			if let Some(target) = &reference.target {
				if !visited.contains(target) {
					pending.insert(target.clone());
				}
			}
		}
		type_refs.extend(types);
		let refs = static_references(loaded, &fq, &context);
		for reference in &refs {
			if let Some(target) = &reference.target {
				if !visited.contains(target) {
					pending.insert(target.clone());
				}
			}
		}
		static_refs.extend(refs);
		for instance in &loaded[&fq].doc.instances {
			// Exactly the resolver input used by loader/composition.rs.
			let parsed = PortRefStr {
				fq_path: Some(instance.graph.clone()),
				node_id: "_".into(),
				port: "_".into(),
			};
			let resolved = resolve_fq_ref(&parsed, &context).and_then(|target| {
				if loaded.contains_key(&target) {
					Ok(target)
				} else {
					Err(format!("Graph file not found: {target}"))
				}
			});
			let (target, error) = match resolved {
				Ok(target) => {
					if !visited.contains(&target) {
						pending.insert(target.clone());
					}
					(Some(target), None)
				}
				Err(error) => (None, Some(error)),
			};
			references.push(CompositionReference {
				from_file: fq.clone(),
				instance_id: instance.id.clone(),
				graph: instance.graph.clone(),
				target,
				error,
			});
		}
	}
	Ok(CompositionSharePlan {
		files: visited.into_iter().collect(),
		references,
		package_references,
		static_references: static_refs,
		type_references: type_refs,
		asset_candidates: Vec::new(),
		portable_feature_verified: false,
		unverified: vec![
			"node_specific_types_and_enums",
			"assets_and_dynamic_references",
			"credentials_and_private_content",
			"full_graph_validation",
			"immutable_export_snapshot",
			"import_and_runtime_verification",
		],
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::loader::file::{FlowgraphFile, GraphInstance};

	fn fixtures() -> BTreeMap<String, LoadedOnDisk> {
		[
			("feature/main", vec![("first", "../shared"), ("again", "/shared/main")]),
			("shared/main", vec![("back", "/feature/main")]),
			("unrelated/main", vec![]),
		]
		.into_iter()
		.map(|(name, instances)| {
			let mut doc: FlowgraphFile = toml::from_str("").unwrap();
			doc.instances = instances
				.into_iter()
				.map(|(id, graph)| GraphInstance {
					id: id.into(),
					graph: graph.into(),
					position: None,
				})
				.collect();
			(name.into(), LoadedOnDisk { path: name.into(), doc })
		})
		.collect()
	}

	#[test]
	fn module_seed_preserves_nearest_package_manifest() {
		use crate::flowgraph::loader::file::FlowgraphPackageManifest;
		let mut files = BTreeMap::new();
		for name in ["root-package", "lib/package", "lib/nested/package", "lib/nested/module", "unrelated/main"] {
			let mut doc: FlowgraphFile = toml::from_str("").unwrap();
			if name.ends_with("package") {
				doc.package = Some(FlowgraphPackageManifest { id: Some(name.replace('/', ".")), ..Default::default() });
			} else { doc.graph.kind = GraphKind::Module; }
			files.insert(name.into(), LoadedOnDisk { path: name.into(), doc });
		}
		let result = plan(&files, &["lib/nested/module".into()]).unwrap();
		assert_eq!(result.files, ["lib/nested/module", "lib/nested/package"]);
		let owner = result.static_references.iter().find(|row| row.field == "package.owner").unwrap();
		assert_eq!(owner.target.as_deref(), Some("lib/nested/package"));
		let root_owned = plan(&files, &["unrelated/main".into()]).unwrap();
		assert_eq!(root_owned.files, ["root-package", "unrelated/main"]);
	}

	#[test]
	fn closure_deduplicates_cycles_and_preserves_reference_reasons() {
		let plan = plan(&fixtures(), &["feature/main".into()]).unwrap();
		assert_eq!(plan.files, ["feature/main", "shared/main"]);
		assert_eq!(plan.references.len(), 3);
		assert!(plan.references.iter().all(|r| r.error.is_none()));
		assert!(!plan.portable_feature_verified);
	}

	#[test]
	fn unresolved_graph_is_reported_instead_of_silently_omitted() {
		let mut files = fixtures();
		files.get_mut("shared/main").unwrap().doc.instances[0].graph = "/missing/main".into();
		let plan = plan(&files, &["feature/main".into()]).unwrap();
		assert_eq!(plan.files, ["feature/main", "shared/main"]);
		let missing = plan.references.iter().find(|r| r.instance_id == "back").unwrap();
		assert_eq!(missing.from_file, "shared/main");
		assert!(missing.target.is_none());
		assert!(missing.error.as_deref().unwrap().contains("missing/main"));
		assert!(super::plan(&files, &["absent".into()]).is_err());
	}
	#[test]
	fn package_dependency_candidates_reject_ambiguity_and_version_mismatch() {
		use crate::flowgraph::loader::file::FlowgraphPackageManifest;
		let mut files = fixtures();
		files.get_mut("feature/main").unwrap().doc.package = Some(FlowgraphPackageManifest {
			id: Some("test.feature".into()),
			dependencies: BTreeMap::from([("test.shared".into(), "1.0.0".into())]),
			..Default::default()
		});
		files.get_mut("shared/main").unwrap().doc.package = Some(FlowgraphPackageManifest {
			id: Some("test.shared".into()),
			version: Some("1.0.0".into()),
			..Default::default()
		});
		// Remove graph references: package dependency alone must reach shared/main.
		files.get_mut("feature/main").unwrap().doc.instances.clear();
		files.get_mut("shared/main").unwrap().doc.instances.clear();
		let good = plan(&files, &["feature/main".into()]).unwrap();
		assert_eq!(good.files, ["feature/main", "shared/main"]);
		assert_eq!(good.package_references[0].target.as_deref(), Some("shared/main"));
		files.get_mut("shared/main").unwrap().doc.package.as_mut().unwrap().version = Some("2.0.0".into());
		let mismatch = plan(&files, &["feature/main".into()]).unwrap();
		assert_eq!(mismatch.files, ["feature/main"]);
		assert!(mismatch.package_references[0]
			.error
			.as_deref()
			.unwrap()
			.contains("Version mismatch"));
		let duplicate = files["shared/main"].doc.package.clone();
		files.get_mut("unrelated/main").unwrap().doc.package = duplicate;
		let ambiguous = plan(&files, &["feature/main".into()]).unwrap();
		assert_eq!(ambiguous.package_references[0].candidates.len(), 2);
		assert!(ambiguous.package_references[0].target.is_none());
	}
	#[test]
	fn edges_and_package_exports_keep_reasons_and_reject_outside_scope() {
		use crate::flowgraph::loader::file::{EdgeEntry, FlowgraphPackageManifest};
		let mut files = fixtures();
		for file in files.values_mut() {
			file.doc.instances.clear();
		}
		files.get_mut("feature/main").unwrap().doc.edges.push(EdgeEntry {
			from: "../shared::source:value".into(),
			to: "sink:value".into(),
		});
		files.get_mut("shared/main").unwrap().doc.package = Some(FlowgraphPackageManifest {
			id: Some("test.shared".into()),
			exports: vec!["./shared/module.flowgraph.toml".into(), "unrelated/main".into()],
			..Default::default()
		});
		let mut module: FlowgraphFile = toml::from_str("").unwrap();
		module.graph.kind = GraphKind::Module;
		files.insert(
			"shared/module".into(),
			LoadedOnDisk {
				path: "shared/module".into(),
				doc: module,
			},
		);
		let result = plan(&files, &["feature/main".into()]).unwrap();
		assert_eq!(result.files, ["feature/main", "shared/main", "shared/module"]);
		assert!(result
			.static_references
			.iter()
			.any(|r| r.field == "edges[0].from" && r.target.as_deref() == Some("shared/main")));
		let rejected = result.static_references.iter().find(|r| r.raw == "unrelated/main").unwrap();
		assert!(rejected.target.is_none());
		assert!(rejected.error.as_deref().unwrap().contains("outside"));
	}
	#[test]
	fn nested_record_types_follow_dependencies_and_report_duplicate_definitions() {
		use crate::flowgraph::loader::file::FlowgraphTypeDef;
		let mut files = fixtures();
		for file in files.values_mut() {
			file.doc.instances.clear();
		}
		files.get_mut("feature/main").unwrap().doc.types.push(FlowgraphTypeDef {
			id: "root".into(),
			fields: BTreeMap::from([("child".into(), "option<list<record<shared>>>".into())]),
			..Default::default()
		});
		files.get_mut("shared/main").unwrap().doc.types.push(FlowgraphTypeDef {
			id: "shared".into(),
			fields: BTreeMap::from([("missing".into(), "record<absent>".into())]),
			..Default::default()
		});
		let result = plan(&files, &["feature/main".into()]).unwrap();
		assert_eq!(result.files, ["feature/main", "shared/main"]);
		assert!(result
			.type_references
			.iter()
			.any(|r| r.schema_id.as_deref() == Some("absent") && r.error.is_some()));
		let duplicate = files["shared/main"].doc.types[0].clone();
		files.get_mut("shared/main").unwrap().doc.types.push(duplicate);
		let result = plan(&files, &["feature/main".into()]).unwrap();
		assert_eq!(result.files, ["feature/main"]);
		assert_eq!(result.type_references[0].candidates.len(), 2);
		assert!(result.type_references[0].target.is_none());
	}
	#[test]
	fn saved_files_plan_export_import_preserves_references_and_original_bytes() {
		use super::super::zip_codec::{export_zip, import_zip, ZipImportOptions, ZipImportOutcome};
		use super::super::{CopyRequest, CopyTarget, FragmentScope};
		use std::fs;
		let temp = std::env::temp_dir().canonicalize().unwrap();
		let root = temp.join(format!(
			"vac-share-roundtrip-{}-{}",
			std::process::id(),
			std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_nanos()
		));
		fs::create_dir(&root).unwrap();
		struct Cleanup(std::path::PathBuf, std::path::PathBuf);
		impl Drop for Cleanup {
			fn drop(&mut self) {
				if let Ok(path) = self.0.canonicalize() {
					if path.parent() == Some(self.1.as_path()) {
						let _ = std::fs::remove_dir_all(path);
					}
				}
			}
		}
		let _cleanup = Cleanup(root.clone(), temp);
		let src = root.join("source");
		let dst = root.join("destination");
		fs::create_dir(&src).unwrap();
		fs::create_dir(&dst).unwrap();
		let sources = [
			(
				"feature/main",
				r#"
[package]
id = "test.feature"
[package.dependencies]
"test.module" = "1.0.0"
[[instances]]
id = "worker"
graph = "/shared/module"
[[types]]
id = "root"
[types.fields]
child = "option<record<shared>>"
"#,
			),
			(
				"shared/module",
				r#"
[graph]
kind = "module"
[[types]]
id = "shared"
[types.fields]
value = "string"
"#,
			),
			("shared/package", "[package]\nid = \"test.module\"\nversion = \"1.0.0\"\nexports = [\"shared/module\"]\n"),
			("unrelated/main", "# unrelated private configuration\n"),
		];
		for (fq, source) in sources {
			let path = src.join(format!("{fq}.flowgraph.toml"));
			fs::create_dir_all(path.parent().unwrap()).unwrap();
			fs::write(path, source).unwrap();
		}
		let result = plan_composition_share(&src, &["feature/main".into()]).unwrap();
		assert_eq!(result.files, ["feature/main", "shared/module", "shared/package"]);
		let bytes = export_zip(
			&src,
			&CopyRequest {
				scope: FragmentScope::Mixed,
				targets: result.files.iter().map(|fq| CopyTarget::File { fq: fq.clone() }).collect(),
				origin: None,
			},
		)
		.unwrap();
		let options = ZipImportOptions {
			dry_run: true,
			target_prefix: "imported".into(),
			paste: Default::default(),
		};
		let preview = import_zip(&dst, &bytes, &options).unwrap();
		assert!(matches!(preview, ZipImportOutcome::Preview(_)));
		assert_eq!(fs::read_dir(&dst).unwrap().count(), 0);
		let options = ZipImportOptions { dry_run: false, ..options };
		let imported = import_zip(&dst, &bytes, &options).unwrap();
		assert!(matches!(imported, ZipImportOutcome::Report(_)));
		let restored = plan_composition_share(&dst, &["imported/feature/main".into()]).unwrap();
		assert_eq!(restored.files, ["imported/feature/main", "imported/shared/module", "imported/shared/package"]);
		assert!(restored.references.iter().all(|r| r.error.is_none()));
		assert!(restored.package_references.iter().all(|r| r.error.is_none()));
		assert!(restored.static_references.iter().all(|r| r.error.is_none()));
		assert!(restored.type_references.iter().all(|r| r.error.is_none()));
		let loaded = crate::flowgraph::loader::load_flowgraph_dir(&dst).unwrap();
		assert!(loaded.composition.instances.iter().any(|instance|
			instance.owner_fq == "imported/feature/main" && instance.graph_fq == "imported/shared/module"));
		let loaded_package = loaded.package_manifests.iter().find(|package|
			package.id.as_deref() == Some("test.module")).unwrap();
		assert_eq!(loaded_package.source_fq, "imported/shared/package");
		assert_eq!(loaded_package.exports, ["imported/shared/module"]);

		// Exporting only the module must also preserve its separate owner declaration.
		let module_plan = plan_composition_share(&src, &["shared/module".into()]).unwrap();
		assert_eq!(module_plan.files, ["shared/module", "shared/package"]);
		let module_zip = export_zip(&src, &CopyRequest {
			scope: FragmentScope::Mixed,
			targets: module_plan.files.iter().map(|fq| CopyTarget::File { fq: fq.clone() }).collect(),
			origin: None,
		}).unwrap();
		import_zip(&dst, &module_zip, &ZipImportOptions {
			dry_run: false, target_prefix: "module-only".into(), paste: Default::default(),
		}).unwrap();
		let module_restored = plan_composition_share(&dst, &["module-only/shared/module".into()]).unwrap();
		assert_eq!(module_restored.files, ["module-only/shared/module", "module-only/shared/package"]);
		assert!(module_restored.static_references.iter().all(|r| r.error.is_none()));
		let manifest: FlowgraphFile = toml::from_str(&fs::read_to_string(
			dst.join("module-only/shared/package.flowgraph.toml")).unwrap()).unwrap();
		let package = manifest.package.unwrap();
		assert_eq!(package.id.as_deref(), Some("test.module"));
		assert_eq!(package.version.as_deref(), Some("1.0.0"));
		assert_eq!(package.exports, ["module-only/shared/module"]);

		for (fq, source) in sources {
			assert_eq!(fs::read(src.join(format!("{fq}.flowgraph.toml"))).unwrap(), source.as_bytes());
		}
		assert!(!dst.join("imported/unrelated/main.flowgraph.toml").exists());
	}
}
