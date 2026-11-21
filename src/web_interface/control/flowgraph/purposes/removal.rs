//! Prospective grouped removal only. A named selection does not own dependencies.
use super::*;
use std::collections::{BTreeMap,HashSet};
use serde_json::{json,Value};
fn unresolved(plan: &crate::flowgraph::fragment::composition_share::CompositionSharePlan) -> Vec<String> {
    plan.references.iter().filter_map(|r|r.error.as_ref().map(|e|format!("{}: {}: {}",r.from_file,r.graph,e)))
        .chain(plan.package_references.iter().filter_map(|r|r.error.as_ref().map(|e|format!("{}: {}: {}",r.from_file,r.package_id,e))))
        .chain(plan.static_references.iter().filter_map(|r|r.error.as_ref().map(|e|format!("{}: {}: {}",r.from_file,r.field,e))))
        .chain(plan.type_references.iter().filter_map(|r|r.error.as_ref().map(|e|format!("{}: {}: {}",r.from_file,r.field,e))))
        .collect()
}
fn sources(root: &Path) -> io::Result<BTreeMap<String,String>> {
    let mut result=BTreeMap::new();
    for path in crate::flowgraph::loader::walk_flowgraph_dir(root)? {
        let fq=crate::flowgraph::loader::fq_path_of_file(root,&path).map_err(|e|invalid(&e))?;
        result.insert(fq,blake3::hash(&std::fs::read(path)?).to_hex().to_string());
    }
    Ok(result)
}
pub fn removal_plan(root: &Path, expected: &Snapshot, id: &str, files: Vec<String>) -> io::Result<Value> {
    let _guard=super::super::write_lock::acquire(root)?;
    removal_plan_unlocked(root,expected,id,files)
}
pub(super) fn removal_plan_unlocked(root: &Path, expected: &Snapshot, id: &str, files: Vec<String>) -> io::Result<Value> {
    if super::super::graph_root_identity(root)!=expected.root {return Err(invalid("purpose root changed"));}
    Document {schema_version:1,purposes:vec![Purpose{id:"selection".into(),name:"Removal selection".into(),roots:files.clone()}]}.validate()?;
    let current=read_unlocked(root)?;
    if current.revision!=expected.revision {return Err(invalid("purpose revision changed"));}
    let purpose=current.document.purposes.iter().find(|p|p.id==id).ok_or_else(||invalid("purpose is missing"))?;
    let before=sources(root)?;
    let plan=crate::flowgraph::fragment::composition_share::plan_composition_share(root,&purpose.roots).map_err(|e|invalid(&e.to_string()))?;
    if files.iter().any(|f|!plan.files.contains(f)) {return Err(invalid("removal includes files outside this purpose"));}
    let selected=files.iter().cloned().collect::<HashSet<_>>();
    // Removing a manifest can silently change the boundary of a retained module.
    // The prospective loader alone cannot detect that: the module remains valid
    // without the former manifest. Include unregistered files in this check.
    let mut reference_errors=unresolved(&plan);
    let remaining=crate::flowgraph::fragment::composition_share::retained_package_owners(root,&selected)
        .map_err(|e|invalid(&e.to_string()))?;
    for reference in remaining {
        reference_errors.push(format!("{} が所属定義 {} を使用しています。定義を残すか、所属するファイルも含めて撤去範囲を見直してください。",reference.from_file,reference.target.unwrap_or_default()));
    }
    let mut other_uses=vec![];
    for other in current.document.purposes.iter().filter(|p|p.id!=id) {
        match crate::flowgraph::fragment::composition_share::plan_composition_share(root,&other.roots) {
            Ok(other_plan)=>{
                let used=other_plan.files.iter().filter(|f|selected.contains(*f)).cloned().collect::<Vec<_>>();
                let errors=unresolved(&other_plan);
                let error=if errors.is_empty(){None}else{Some(errors.join("\n"))};
                if !used.is_empty()||error.is_some(){other_uses.push(json!({"id":other.id,"name":other.name,"files":used,"error":error}));}
            },
            Err(error)=>other_uses.push(json!({"id":other.id,"name":other.name,"files":[],"error":error.to_string()})),
        }
    }
    let (valid,diagnostics)=match crate::flowgraph::loader::dir::load_flowgraph_dir_excluding_files(root,&selected) {
        Ok(report)=>(!report.diagnostics.iter().any(|d|d.severity==crate::flowgraph::loader::Severity::Error),report.diagnostics),
        Err(error)=>(false,error.diagnostics),
    };
    if sources(root)?!=before || read_unlocked(root)?.revision!=current.revision {return Err(invalid("purpose removal sources changed during review"));}
    Ok(json!({"root":current.root,"purpose":purpose,"purpose_revision":current.revision,"graph_sources":before,"requested_files":files,
        "retained_files":plan.files.iter().filter(|f|!selected.contains(*f)).collect::<Vec<_>>(),"other_purpose_uses":other_uses,
        "graph_validation_ok":valid,"diagnostics":diagnostics,"purpose_reference_errors":reference_errors,"delete_ready":false,
        "unverified":["dynamic_references","transaction_and_recovery","runtime_application"]}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grouped_removal_keeps_shared_parts_and_validates_remaining_graphs() {
        let root=std::env::temp_dir().join(format!("vac-purpose-removal-{}",rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("module.flowgraph.toml"),"[graph]\nkind='module'\n[[nodes]]\nid='counter'\nfeature='flowgraph.state.int_counter'\n").unwrap();
        for name in ["main","outside"] {std::fs::write(root.join(format!("{name}.flowgraph.toml")),"[[instances]]\nid='child'\ngraph='module'\n").unwrap();}
        let empty=read(&root).unwrap();
        let first=Purpose{id:"first".into(),name:"First".into(),roots:vec!["main".into()]};
        let second=Purpose{id:"second".into(),name:"Second".into(),roots:vec!["module".into()]};
        let current=save(&root,&empty,Document{schema_version:1,purposes:vec![first.clone(),second]}).unwrap();
        let before=sources(&root).unwrap();
        let roots_only=removal_plan(&root,&current,"first",vec!["main".into()]).unwrap();
        assert_eq!(roots_only["retained_files"],json!(["module"]));
        assert_eq!(roots_only["graph_validation_ok"],true);
        let whole=removal_plan(&root,&current,"first",vec!["main".into(),"module".into()]).unwrap();
        assert_eq!(whole["other_purpose_uses"][0]["id"],"second");
        assert_eq!(whole["graph_validation_ok"],false); // outside still references module
        assert_eq!(whole["delete_ready"],false);
        assert!(removal_plan(&root,&current,"first",vec!["outside".into()]).is_err());
        assert!(removal_plan(&root,&empty,"first",vec!["main".into()]).is_err());
        assert_eq!(sources(&root).unwrap(),before);
        assert_eq!(read(&root).unwrap(),current);
        let current=save(&root,&current,Document{schema_version:1,purposes:vec![first.clone()]}).unwrap();
        std::fs::write(root.join("outside.flowgraph.toml"),"[meta]\ntitle='Independent'\n").unwrap();
        let before=sources(&root).unwrap();
        let whole=removal_plan(&root,&current,"first",vec!["main".into(),"module".into()]).unwrap();
        assert_eq!(whole["graph_validation_ok"],true);
        assert!(whole["other_purpose_uses"].as_array().unwrap().is_empty());
        assert_eq!(whole["delete_ready"],false); // no mutation API or recovery transaction yet
        assert_eq!(sources(&root).unwrap(),before);
        // Discovery can succeed while reporting unresolved references. Such a
        // purpose must not disappear from the impact report as "unrelated".
        std::fs::write(root.join("outside.flowgraph.toml"),"[[instances]]\nid='lost'\ngraph='missing'\n").unwrap();
        let current=save(&root,&current,Document{schema_version:1,purposes:vec![first, Purpose{id:"broken".into(),name:"Broken".into(),roots:vec!["outside".into()]}]}).unwrap();
        let review=removal_plan(&root,&current,"first",vec!["main".into()]).unwrap();
        assert_eq!(review["other_purpose_uses"][0]["id"],"broken");
        assert!(review["other_purpose_uses"][0]["error"].as_str().unwrap().contains("missing"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
