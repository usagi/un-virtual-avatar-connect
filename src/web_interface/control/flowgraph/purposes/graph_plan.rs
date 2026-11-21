//! Read-only inspection. A matching file does not authorize replay or overwrite.
use std::{collections::BTreeSet, io, path::Path};
use serde_json::{json, Value};

fn invalid(message:&str)->io::Error {io::Error::new(io::ErrorKind::InvalidData,message)}
fn valid_fq(value:&str)->io::Result<()> {
    super::Document{schema_version:1,purposes:vec![super::Purpose{id:"inspect".into(),name:"inspect".into(),roots:vec![value.into()]}]}.validate()
}

// A verified file alone is not enough: bind it to the exact original import
// mapping and both versions. Ambiguous/unreadable intent sets cannot authorize
// replacement. Legacy attempts remain inspectable without this evidence.
fn matching_preimage(intents:&[Value],path:&str,source:&str,before:&str,after:&str)->Option<Value> {
    if intents.iter().any(|row|row["observation"]=="unreadable") {return None;}
    let matching=intents.iter().filter(|row|row["path"]==path).collect::<Vec<_>>();
    if matching.len()!=1 {return None;}
    let row=matching[0];
    if row["source"]!=source || row["before_digest"]!=before || row["content_digest"]!=after
        || row["preimage"]["observation"]!="verified" || row["preimage"]["content_digest"]!=before {return None;}
    Some(json!({"path":row["preimage"]["path"],"content_digest":before,
        "size":row["preimage"]["size"],"intent_revision":row["intent_revision"],"observation":"verified"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_evidence_requires_exact_unambiguous_mapping_and_versions() {
        let row=json!({"path":"a.flowgraph.toml","source":"source/a","before_digest":"old","content_digest":"new",
            "observation":"matches_previous","intent_revision":"intent","preimage":{"path":"scoped.bin","content_digest":"old","size":3,"observation":"verified"}});
        let check=|rows:&[Value]|matching_preimage(rows,"a.flowgraph.toml","source/a","old","new");
        assert!(check(&[row.clone()]).is_some());
        assert!(check(&[row.clone(),row.clone()]).is_none());
        for (key,value) in [("path","b.flowgraph.toml"),("source","other/a"),("before_digest","other"),("content_digest","later")] {
            let mut changed=row.clone();changed[key]=json!(value);assert!(check(&[changed]).is_none(),"accepted {key}");
        }
        for state in ["missing","changed","unreadable"] {
            let mut changed=row.clone();changed["preimage"]["observation"]=json!(state);assert!(check(&[changed]).is_none());
        }
        let mut legacy=row.clone();legacy.as_object_mut().unwrap().remove("preimage");assert!(check(&[legacy]).is_none());
        assert!(check(&[row,json!({"observation":"unreadable"})]).is_none());
    }
    #[test]
    fn incomplete_graph_plan_inspects_all_destinations_without_mutation() {
        let directory=std::env::temp_dir().join(format!("vac-graph-plan-{:032x}",rand::random::<u128>()));
        std::fs::create_dir(&directory).unwrap();let root=directory.as_path();let id="12345678901234567890123456789012";
        let old=blake3::hash(b"old").to_hex().to_string();
        let mut plan=json!({"schema_version":1,"id":id,"root":super::super::super::graph_root_identity(root),"state":"graphs_planned",
            "report":{"written_files":[],"imported_nodes":[],"skipped_nodes":[],"skipped_files":[],"unresolved_danglings":[],"backups":[]},
            "files":[
                {"original_fq":"a","fq":"a","content":"planned","before_digest":null},
                {"original_fq":"b","fq":"b","content":"planned","before_digest":null},
                {"original_fq":"c","fq":"c","content":"planned","before_digest":old},
                {"original_fq":"d","fq":"d","content":"planned","before_digest":old},
                {"original_fq":"e","fq":"e","content":"planned","before_digest":null}
            ]});
        let path=root.join(format!(".flowgraph-purpose-transfer-{id}.graph-plan.json"));
        std::fs::write(&path,serde_json::to_vec(&plan).unwrap()).unwrap();
        std::fs::write(root.join("a.flowgraph.toml"),b"planned").unwrap();
        std::fs::write(root.join("c.flowgraph.toml"),b"old").unwrap();
        std::fs::write(root.join("e.flowgraph.toml"),b"user edit").unwrap();
        let result=inspect(root,id).unwrap().unwrap();
        let states=result["files"].as_array().unwrap().iter().map(|file|file["observation"].as_str().unwrap()).collect::<Vec<_>>();
        assert_eq!(states,vec!["matches_intended","not_created","matches_previous","missing","changed"]);
        assert_eq!(result["execution_available"],false);
        assert!(!root.join("b.flowgraph.toml").exists());
        assert_eq!(std::fs::read(root.join("e.flowgraph.toml")).unwrap(),b"user edit");
        for invalid_fq in ["../outside","a", "A", "x:y", "x\\y"] {
            plan["files"][1]["fq"]=json!(invalid_fq);
            std::fs::write(&path,serde_json::to_vec(&plan).unwrap()).unwrap();
            assert!(inspect(root,id).is_err(),"accepted {invalid_fq}");
        }
        plan["files"][1]["fq"]=json!("b");plan["root"]=json!("another-profile");
        std::fs::write(&path,serde_json::to_vec(&plan).unwrap()).unwrap();
        assert!(inspect(root,id).is_err());
        assert!(root.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(root).unwrap();
    }
}

pub(super) fn inspect(root:&Path,id:&str)->io::Result<Option<Value>> {
    if id.len()!=32 || !id.bytes().all(|b|b.is_ascii_hexdigit()) {return Err(invalid("invalid graph plan id"));}
    let path=root.join(format!(".flowgraph-purpose-transfer-{id}.graph-plan.json"));
    let metadata=match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind()==io::ErrorKind::NotFound=>return Ok(None),
        result=>result?,
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len()>256*1024*1024 {return Err(invalid("invalid graph plan file"));}
    let bytes=std::fs::read(&path)?;
    let plan:Value=serde_json::from_slice(&bytes)?;
    if plan["schema_version"]!=1 || plan["id"]!=id || plan["root"]!=super::super::graph_root_identity(root) || plan["state"]!="graphs_planned" {
        return Err(invalid("graph plan identity changed"));
    }
    let report:crate::flowgraph::fragment::paste::PasteReport=serde_json::from_value(plan["report"].clone())?;
    if !report.written_files.is_empty() || !report.backups.is_empty() {return Err(invalid("graph plan is not prepublication"));}
    let files=plan["files"].as_array().ok_or_else(||invalid("missing planned graphs"))?;
    if files.len()>4096 {return Err(invalid("too many planned graphs"));}
    let mut sources=BTreeSet::new();let mut destinations=BTreeSet::new();let mut validated=vec![];
    // Validate every path before reading any destination.
    for file in files {
        let source=file["original_fq"].as_str().ok_or_else(||invalid("missing graph source"))?;
        let fq=file["fq"].as_str().ok_or_else(||invalid("missing graph destination"))?;
        valid_fq(source)?;valid_fq(fq)?;
        if !sources.insert(source.to_lowercase()) || !destinations.insert(fq.to_lowercase()) {return Err(invalid("duplicate graph plan mapping"));}
        let content=file["content"].as_str().ok_or_else(||invalid("missing graph content"))?;
        let previous=file.get("before_digest").ok_or_else(||invalid("missing previous digest"))?;
        if !previous.is_null() && !previous.as_str().is_some_and(|digest|digest.len()==64&&digest.bytes().all(|b|b.is_ascii_hexdigit())) {return Err(invalid("invalid previous digest"));}
        validated.push((source,format!("{fq}.flowgraph.toml"),blake3::hash(content.as_bytes()).to_hex().to_string(),previous.as_str()));
    }
    let intents=super::attempt_inspection::inspect(root,id)?;
    let mut observations=vec![];
    for (source,path,digest,previous) in validated {
        let preimage=previous.and_then(|before|matching_preimage(&intents,&path,source,before,&digest));
        let observation=match super::attempt_inspection::current_digest(root,&path) {
            Ok(current)=>{
                let state=match current.as_deref(){Some(value) if value==digest=>"matches_intended",Some(value) if Some(value)==previous=>"matches_previous",Some(_)=>"changed",None if previous.is_none()=>"not_created",None=>"missing"};
                json!({"path":path,"source":source,"observation":state,"current_digest":current,"preimage":preimage})
            },
            Err(error)=>json!({"path":path,"source":source,"observation":"unreadable","error":error.to_string()}),
        };
        observations.push(observation);
    }
    Ok(Some(json!({"revision":blake3::hash(&bytes).to_hex().to_string(),"files":observations,"execution_available":false})))
}

/// Review against the original ZIP and immutable original replacement evidence.
pub(super) fn preview(root:&Path,id:&str,start:&Value,expected:&super::Snapshot,archive:&[u8])->anyhow::Result<Value>{
    use crate::flowgraph::fragment::zip_codec::{self,ZipImportOptions,ZipImportOutcome};
    let observed=inspect(root,id)?.ok_or_else(||invalid("graph plan is unavailable"))?;
    let bytes=std::fs::read(root.join(format!(".flowgraph-purpose-transfer-{id}.graph-plan.json")))?;
    anyhow::ensure!(observed["revision"]==blake3::hash(&bytes).to_hex().to_string(),"graph plan changed during inspection");
    let plan:Value=serde_json::from_slice(&bytes)?;
    anyhow::ensure!(start["archive_digest"]==blake3::hash(archive).to_hex().to_string(),"original archive does not match");
    let options:ZipImportOptions=serde_json::from_value(start["options"].clone())?;
    let ZipImportOutcome::Preview(incoming)=zip_codec::import_zip(root,archive,&ZipImportOptions{dry_run:true,..options})? else {unreachable!()};
    let portable=incoming.purpose.ok_or_else(||invalid("archive has no portable purpose"))?;
    let report:crate::flowgraph::fragment::paste::PasteReport=serde_json::from_value(plan["report"].clone())?;
    let files=plan["files"].as_array().unwrap();
    for file in files {
        let source=file["original_fq"].as_str().unwrap();
        anyhow::ensure!(incoming.entries.iter().any(|entry|entry.zip_path==format!("flowgraph/{source}.flowgraph.toml")),"planned graph absent from archive");
        crate::flowgraph::loader::file::parse_flowgraph_file(file["content"].as_str().unwrap(),None).map_err(|error|invalid(&error.to_string()))?;
    }
    for entry in &incoming.entries {
        let source=entry.zip_path.strip_prefix("flowgraph/").unwrap().strip_suffix(".flowgraph.toml").unwrap();
        let destination=entry.dest_path.strip_suffix(".flowgraph.toml").unwrap();
        anyhow::ensure!(files.iter().any(|file|file["original_fq"]==source)||report.skipped_files.iter().any(|file|file==destination),"archive graph absent from plan");
    }
    let mut roots=vec![];let mut missing=vec![];
    for source in portable.roots {
        match files.iter().find(|file|file["original_fq"]==source) {
            Some(file)=>roots.push(file["fq"].as_str().unwrap().to_owned()),None=>missing.push(source),
        }
    }
    let mut conflicts=vec![];
    let graphs=observed["files"].as_array().unwrap().iter().zip(files).map(|(file,planned)|{
        let recovery_required=!planned["before_digest"].is_null();
        let recovery_valid=!recovery_required || file["preimage"]["observation"]=="verified";
        let action=match (file["observation"].as_str(),recovery_valid){
            (Some("matches_intended"),true)=>"keep",(Some("not_created"),true)=>"create",
            (Some("matches_previous"),true)=>"replace",
            _=>{conflicts.push(file["path"].clone());"conflict"}};
        json!({"path":file["path"],"action":action,"preimage":file["preimage"]})
    }).collect::<Vec<_>>();
    let available=conflicts.is_empty()&&missing.is_empty()&&!roots.is_empty()
        &&start["purpose_id"]==format!("imported-{id}");
    Ok(json!({"root":expected.root,"attempt_id":id,"expected":expected,"graphs":graphs,"materials":[],
        "purpose":{"name":portable.name,"roots":roots,"missing_roots":missing},"conflicts":conflicts,
        "skipped_files":report.skipped_files,"unresolved_danglings":report.unresolved_danglings,"execution_available":available,"already_registered":false,
        "stage":"graphs_incomplete","plan_revision":observed["revision"]}))
}

/// Caller holds the root lock and has recomputed the exact reviewed plan.
pub(super) fn execute(root:&Path,id:&str,review:&Value,after_graph:impl Fn()->anyhow::Result<()>)->anyhow::Result<Value>{
    use crate::flowgraph::fragment::paste::{PasteReport,WrittenFile,WriteObserver};
    let bytes=std::fs::read(root.join(format!(".flowgraph-purpose-transfer-{id}.graph-plan.json")))?;
    anyhow::ensure!(review["plan_revision"]==blake3::hash(&bytes).to_hex().to_string(),"graph plan changed");
    let plan:Value=serde_json::from_slice(&bytes)?;
    let files=plan["files"].as_array().unwrap();
    let operation=root.join(format!(".flowgraph-purpose-transfer-{id}.graph-resume-{:032x}.json",rand::random::<u128>()));
    super::super::file_replace::create(&operation,&serde_json::to_vec_pretty(review)?)?;
    let mut report:PasteReport=serde_json::from_value(plan["report"].clone())?;
    for file in files {
        let fq=file["fq"].as_str().unwrap();let relative=format!("{fq}.flowgraph.toml");
        let content=file["content"].as_str().unwrap().as_bytes();let digest=blake3::hash(content).to_hex().to_string();
        let reviewed=review["graphs"].as_array().unwrap().iter().find(|row|row["path"]==relative)
            .ok_or_else(||invalid("graph absent from review"))?;
        if let Some(before)=file["before_digest"].as_str() {
            let intents=super::attempt_inspection::inspect(root,id)?;
            let preimage=matching_preimage(&intents,&relative,file["original_fq"].as_str().unwrap(),before,&digest)
                .ok_or_else(||invalid("replacement recovery evidence changed"))?;
            anyhow::ensure!(reviewed["preimage"]==preimage,"replacement evidence differs from review");
            report.backups.push(root.join(preimage["path"].as_str().unwrap()).to_string_lossy().replace('\\',"/"));
        }
        match super::attempt_inspection::current_digest(root,&relative)? {
            Some(current) if current==digest=>{},
            Some(current)=>{
                anyhow::ensure!(reviewed["action"]=="replace" && file["before_digest"]==current,"saved graph changed during resume");
                super::super::file_replace::replace(&root.join(&relative),content)?;
                after_graph()?;
            }
            None=>{
                anyhow::ensure!(file["before_digest"].is_null() && reviewed["action"]=="create","previous graph is missing during resume");
                let path=root.join(&relative);std::fs::create_dir_all(path.parent().unwrap())?;
                anyhow::ensure!(super::attempt_inspection::current_digest(root,&relative)?.is_none(),"graph appeared during resume");
                super::super::file_replace::create(&path,content)?;
                after_graph()?;
            }
        }
        report.written_files.push(WrittenFile{original_fq:Some(file["original_fq"].as_str().unwrap().into()),fq:fq.into(),
            path:root.join(&relative).to_string_lossy().replace('\\',"/"),content_digest:digest});
    }
    for file in &report.written_files {
        anyhow::ensure!(super::attempt_inspection::current_digest(root,&format!("{}.flowgraph.toml",file.fq))?.as_deref()==Some(&file.content_digest),"graph changed before checkpoint");
    }
    super::attempt::Observer::new(root,id).graphs_completed(&report)?;
    Ok(json!({"graphs_completed":true,"requires_reinspection":true,"reload_attempted":false}))
}
