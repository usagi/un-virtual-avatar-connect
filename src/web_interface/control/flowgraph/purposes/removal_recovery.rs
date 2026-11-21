//! Durable original bytes before grouped removal. No runtime application.
use super::*;
use std::{collections::BTreeMap, fs, path::PathBuf};
const PREFIX: &str = ".flowgraph-purpose-removal-";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema_version: u32,
    root: String,
    before: Document,
    after: Document,
    files: BTreeMap<String, Vec<u8>>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemovalRecovery { pub id: String, pub revision: String }
#[derive(Debug, Serialize)]
pub struct HistoryEntry {pub id:String, pub recovery:Option<RemovalRecovery>, pub purpose:Option<Purpose>,pub files:Vec<String>,pub error:Option<String>}

/// Discover durable records after restart, including damaged records explicitly.
pub fn removal_recovery_history(root:&Path)->io::Result<Vec<HistoryEntry>> {
    let _guard=super::super::write_lock::acquire(root)?;
    let mut entries=vec![];
    for entry in fs::read_dir(root)? {
        let entry=entry?;let name=entry.file_name().to_string_lossy().into_owned();
        let Some(id)=name.strip_prefix(PREFIX).and_then(|s|s.strip_suffix(".json")) else {continue};
        let result=(||->io::Result<(RemovalRecovery,Record)>{
            if !entry.file_type()?.is_file(){return Err(invalid("invalid recovery record file"));}
            let recovery=RemovalRecovery{id:id.into(),revision:blake3::hash(&fs::read(entry.path())?).to_hex().to_string()};
            let record=read_record(root,&recovery)?;Ok((recovery,record))
        })();
        entries.push(match result {Ok((recovery,record))=>HistoryEntry{id:id.into(),recovery:Some(recovery),purpose:record.before.purposes.iter().find(|p|!record.after.purposes.iter().any(|after|after.id==p.id)).cloned(),files:record.files.keys().cloned().collect(),error:None},Err(e)=>HistoryEntry{id:id.into(),recovery:None,purpose:None,files:vec![],error:Some(e.to_string())}});
    }
    entries.sort_by(|a,b|a.id.cmp(&b.id));Ok(entries)
}

fn record_path(root: &Path, id: &str) -> io::Result<PathBuf> {
    if id.len()!=32 || !id.bytes().all(|b|b.is_ascii_hexdigit()) {return Err(invalid("invalid removal recovery id"));}
    Ok(root.join(format!("{PREFIX}{id}.json")))
}
fn graph_path(root: &Path, fq: &str) -> io::Result<PathBuf> {
    Document{schema_version:1,purposes:vec![Purpose{id:"path".into(),name:"Path".into(),roots:vec![fq.into()]}]}.validate()?;
    let mut path=root.to_path_buf();
    let relative=format!("{fq}.flowgraph.toml");
    for part in relative.split('/') {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink()=>return Err(invalid("recovery path must not use links")),
            Ok(_)=>{},
            Err(e) if e.kind()==io::ErrorKind::NotFound=>{},
            Err(e)=>return Err(e),
        }
    }
    Ok(path)
}
fn read_record(root: &Path, expected: &RemovalRecovery) -> io::Result<Record> {
    let path=record_path(root,&expected.id)?;
    let meta=fs::symlink_metadata(&path)?;
    if !meta.is_file()||meta.file_type().is_symlink(){return Err(invalid("invalid recovery record file"));}
    let bytes=fs::read(path)?;
    if blake3::hash(&bytes).to_hex().as_str()!=expected.revision{return Err(invalid("removal recovery record changed"));}
    let record:Record=serde_json::from_slice(&bytes).map_err(|e|invalid(&e.to_string()))?;
    if record.schema_version!=1||record.root!=super::super::graph_root_identity(root)||record.files.is_empty(){return Err(invalid("invalid recovery record identity"));}
    record.before.validate()?; record.after.validate()?;
    let removed=record.before.purposes.iter().filter(|p|!record.after.purposes.iter().any(|after|after.id==p.id)).collect::<Vec<_>>();
    if removed.len()!=1{return Err(invalid("recovery must describe one removed purpose"));}
    let mut retained=record.before.clone();retained.purposes.retain(|p|p.id!=removed[0].id);
    if retained!=record.after{return Err(invalid("recovery changes unrelated registrations"));}
    for fq in record.files.keys(){graph_path(root,fq)?;}
    Ok(record)
}

/// Publish recovery bytes before a future caller performs any removal.
/// `reviewed` is the complete native removal-plan response, not client ownership.
pub fn prepare_removal_recovery(root: &Path, expected: &Snapshot, id: &str, files: Vec<String>, reviewed: &serde_json::Value) -> io::Result<RemovalRecovery> {
    let _guard=super::super::write_lock::acquire(root)?;
    prepare_unlocked(root,expected,id,files,reviewed)
}
fn prepare_unlocked(root: &Path, expected: &Snapshot, id: &str, files: Vec<String>, reviewed: &serde_json::Value) -> io::Result<RemovalRecovery> {
    let actual=super::removal::removal_plan_unlocked(root,expected,id,files.clone())?;
    if &actual!=reviewed{return Err(invalid("removal review changed"));}
    if actual["graph_validation_ok"]!=true || actual["other_purpose_uses"].as_array().is_none_or(|v|!v.is_empty()) || actual["purpose_reference_errors"].as_array().is_none_or(|v|!v.is_empty()) {
        return Err(invalid("removal has unresolved dependencies"));
    }
    let before=read_unlocked(root)?.document;
    let mut after=before.clone(); after.purposes.retain(|p|p.id!=id);
    let mut originals=BTreeMap::new();
    for fq in files {
        let bytes=fs::read(graph_path(root,&fq)?)?;
        if actual["graph_sources"][&fq].as_str()!=Some(blake3::hash(&bytes).to_hex().as_str()){return Err(invalid("removal source changed"));}
        originals.insert(fq,bytes);
    }
    let record=Record{schema_version:1,root:expected.root.clone(),before,after,files:originals};
    let bytes=serde_json::to_vec(&record).map_err(|e|invalid(&e.to_string()))?;
    let recovery=RemovalRecovery{id:format!("{:032x}",rand::random::<u128>()),revision:blake3::hash(&bytes).to_hex().to_string()};
    super::super::file_replace::create(&record_path(root,&recovery.id)?,&bytes)?;
    read_record(root,&recovery)?;
    Ok(recovery)
}

#[derive(Debug, Serialize)]
pub struct RemovalOutcome {
    pub recovery: RemovalRecovery,
    pub removed_files: Vec<String>,
    pub registration: Option<Snapshot>,
    pub error: Option<String>,
}

/// File/storage operation only. Does not reload or imply safe runtime application.
/// Errors after journal publication carry the recovery identity and actual progress.
pub fn remove_purpose_files(root:&Path,expected:&Snapshot,id:&str,files:Vec<String>,reviewed:&serde_json::Value)->io::Result<RemovalOutcome>{
    remove_with(root,expected,id,files,reviewed,|_|Ok(()))
}
fn remove_with(root:&Path,expected:&Snapshot,id:&str,files:Vec<String>,reviewed:&serde_json::Value,mut checkpoint:impl FnMut(usize)->io::Result<()>)->io::Result<RemovalOutcome>{
    let _guard=super::super::write_lock::acquire(root)?;
    let recovery=prepare_unlocked(root,expected,id,files,reviewed)?;
    let mut outcome=RemovalOutcome{recovery,removed_files:vec![],registration:None,error:None};
    let operation=(||->io::Result<Snapshot>{
        let record=read_record(root,&outcome.recovery)?;
        for (index,(fq,bytes)) in record.files.iter().enumerate(){
            checkpoint(index)?;
            let path=graph_path(root,fq)?;
            if fs::read(&path)?!=*bytes{return Err(invalid("removal source changed after recovery preparation"));}
            fs::remove_file(path)?;
            outcome.removed_files.push(fq.clone());
        }
        checkpoint(record.files.len())?;
        super::save_unlocked(root,expected,record.after)
    })();
    match operation{Ok(snapshot)=>outcome.registration=Some(snapshot),Err(error)=>outcome.error=Some(error.to_string())}
    Ok(outcome)
}

/// Restore original bytes without replacing any changed file or registration.
/// A partial I/O failure retains the record; retry skips already restored bytes.
pub fn restore_removal(root: &Path, expected: &RemovalRecovery) -> io::Result<Snapshot> {
    let _guard=super::super::write_lock::acquire(root)?;
    let record=read_record(root,expected)?;
    let current=read_unlocked(root)?;
    if current.document!=record.before && current.document!=record.after{return Err(invalid("purpose registrations changed since removal"));}
    let mut missing=vec![];
    for (fq,bytes) in &record.files {
        let path=graph_path(root,fq)?;
        match fs::read(&path) {
            Ok(now) if now==*bytes=>{},
            Ok(_)=>return Err(invalid("recovery destination was edited")),
            Err(e) if e.kind()==io::ErrorKind::NotFound=>missing.push((path,bytes)),
            Err(e)=>return Err(e),
        }
    }
    for (path,bytes) in missing {
        // Empty parent directories are retained by grouped file removal.
        super::super::file_replace::create(&path,bytes)?;
    }
    super::save_unlocked(root,&current,record.before)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removing_package_manifest_cannot_orphan_unregistered_module() {
        let root=std::env::temp_dir().join(format!("vac-removal-package-{}",rand::random::<u64>()));
        fs::create_dir_all(root.join("feature")).unwrap();
        let manifest=b"[package]\nid='test.feature'\nversion='1.0.0'\n";
        let module=b"[graph]\nkind='module'\n";
        fs::write(root.join("feature/package.flowgraph.toml"),manifest).unwrap();
        fs::write(root.join("feature/module.flowgraph.toml"),module).unwrap();
        let before=save(&root,&read(&root).unwrap(),Document{schema_version:1,purposes:vec![
            Purpose{id:"first".into(),name:"First".into(),roots:vec!["feature/package".into()]},
        ]}).unwrap();
        let selected=vec!["feature/package".into()];
        let blocked=super::super::removal_plan(&root,&before,"first",selected.clone()).unwrap();
        // The remaining module still loads, but losing its package changes its boundary.
        assert_eq!(blocked["graph_validation_ok"],true);
        assert!(remove_purpose_files(&root,&before,"first",selected,&blocked).is_err());
        assert!(removal_recovery_history(&root).unwrap().is_empty());
        assert_eq!(fs::read(root.join("feature/package.flowgraph.toml")).unwrap(),manifest);
        assert_eq!(fs::read(root.join("feature/module.flowgraph.toml")).unwrap(),module);
        assert_eq!(read(&root).unwrap(),before);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn shared_dependency_rejection_and_root_only_roundtrip() {
        let root=std::env::temp_dir().join(format!("vac-removal-shared-{}",rand::random::<u64>()));
        fs::create_dir(&root).unwrap();
        let module=b"[graph]\nkind='module'\n[[nodes]]\nid='counter'\nfeature='flowgraph.state.int_counter'\n";
        let parent=b"[[instances]]\nid='child'\ngraph='module'\n";
        fs::write(root.join("module.flowgraph.toml"),module).unwrap();
        for name in ["main","outside"] {fs::write(root.join(format!("{name}.flowgraph.toml")),parent).unwrap();}
        let before=save(&root,&read(&root).unwrap(),Document{schema_version:1,purposes:vec![
            Purpose{id:"first".into(),name:"First".into(),roots:vec!["main".into()]},
            Purpose{id:"second".into(),name:"Second".into(),roots:vec!["outside".into()]},
        ]}).unwrap();
        let all=vec!["main".into(),"module".into()];
        let blocked=super::super::removal_plan(&root,&before,"first",all.clone()).unwrap();
        assert!(remove_purpose_files(&root,&before,"first",all,&blocked).is_err());
        assert!(removal_recovery_history(&root).unwrap().is_empty());
        assert_eq!(read(&root).unwrap(),before);
        assert_eq!(fs::read(root.join("main.flowgraph.toml")).unwrap(),parent);
        assert_eq!(fs::read(root.join("module.flowgraph.toml")).unwrap(),module);
        let selected=vec!["main".into()];
        let allowed=super::super::removal_plan(&root,&before,"first",selected.clone()).unwrap();
        let outcome=remove_purpose_files(&root,&before,"first",selected,&allowed).unwrap();
        assert!(outcome.error.is_none());
        assert_eq!(outcome.removed_files,vec!["main"]);
        assert_eq!(outcome.registration.unwrap().document.purposes,vec![before.document.purposes[1].clone()]);
        assert_eq!(fs::read(root.join("module.flowgraph.toml")).unwrap(),module);
        assert_eq!(fs::read(root.join("outside.flowgraph.toml")).unwrap(),parent);
        let history=removal_recovery_history(&root).unwrap();
        assert_eq!(history.len(),1);
        assert_eq!(restore_removal(&root,history[0].recovery.as_ref().unwrap()).unwrap().document,before.document);
        assert_eq!(fs::read(root.join("main.flowgraph.toml")).unwrap(),parent);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn child_removal_process() {
        let Some(root)=std::env::var_os("UNVAC_TEST_REMOVAL_PROCESS_ROOT") else{return};
        let root=PathBuf::from(root);
        let mode=std::env::var("UNVAC_TEST_REMOVAL_PROCESS_MODE").unwrap();
        if mode=="restore" {
            let history=removal_recovery_history(&root).unwrap();assert_eq!(history.len(),1);
            restore_removal(&root,history[0].recovery.as_ref().unwrap()).unwrap();
            return;
        }
        let stop:usize=mode.parse().unwrap();
        let before=read(&root).unwrap();let files=vec!["a".into(),"b".into()];
        let plan=super::super::removal_plan(&root,&before,"pair",files.clone()).unwrap();
        let outcome=remove_with(&root,&before,"pair",files,&plan,|index|{
            if index==stop {std::process::exit(73);} // No unwinding or lock Drop.
            Ok(())
        }).unwrap();
        assert!(outcome.error.is_none());
        std::process::exit(73); // Committed storage; response never delivered.
    }
    fn child(root:&Path,mode:&str)->std::process::Output {
        let name=format!("{}::child_removal_process",module_path!().split_once("::").unwrap().1);
        let mut command=std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact",&name]).env("UNVAC_TEST_REMOVAL_PROCESS_ROOT",root).env("UNVAC_TEST_REMOVAL_PROCESS_MODE",mode);
        #[cfg(windows)] {use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
        command.output().unwrap()
    }
    #[test]
    fn removal_process_exit_recovers_in_fresh_process() {
        for stop in 0..=3 {
            let root=std::env::temp_dir().join(format!("vac-removal-process-{}",rand::random::<u64>()));fs::create_dir(&root).unwrap();
            let bytes=b"[meta]\ntitle='Original'\n";
            for fq in ["a","b","unrelated"]{fs::write(root.join(format!("{fq}.flowgraph.toml")),bytes).unwrap();}
            let before=save(&root,&read(&root).unwrap(),Document{schema_version:1,purposes:vec![Purpose{id:"pair".into(),name:"Pair".into(),roots:vec!["a".into(),"b".into()]}]}).unwrap();
            let stopped=child(&root,&stop.to_string());
            assert_eq!(stopped.status.code(),Some(73),"{}",String::from_utf8_lossy(&stopped.stderr));
            assert_eq!(root.join("a.flowgraph.toml").exists(),stop==0);
            assert_eq!(root.join("b.flowgraph.toml").exists(),stop<2);
            assert_eq!(read(&root).unwrap().document.purposes.is_empty(),stop==3);
            // No receipt passed through memory/response. Fresh processes discover it.
            for _ in 0..2 {
                let restored=child(&root,"restore");
                assert!(restored.status.success(),"{}{}",String::from_utf8_lossy(&restored.stdout),String::from_utf8_lossy(&restored.stderr));
                assert!(String::from_utf8_lossy(&restored.stdout).contains("1 passed"));
                assert_eq!(read(&root).unwrap().document,before.document);
                for fq in ["a","b","unrelated"]{assert_eq!(fs::read(root.join(format!("{fq}.flowgraph.toml"))).unwrap(),bytes);}
            }
            fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn grouped_removal_failures_preserve_recovery_and_actual_progress() {
        for fail_at in 0..=3 {
            let root=std::env::temp_dir().join(format!("vac-removal-failure-{}",rand::random::<u64>()));fs::create_dir(&root).unwrap();
            let bytes=b"[meta]\ntitle='Original'\n";
            for fq in ["a","b","unrelated"] {fs::write(root.join(format!("{fq}.flowgraph.toml")),bytes).unwrap();}
            let before=save(&root,&read(&root).unwrap(),Document{schema_version:1,purposes:vec![Purpose{id:"pair".into(),name:"Pair".into(),roots:vec!["a".into(),"b".into()]}]}).unwrap();
            let files=vec!["a".into(),"b".into()];
            let plan=super::super::removal_plan(&root,&before,"pair",files.clone()).unwrap();
            let outcome=remove_with(&root,&before,"pair",files,&plan,|index|if index==fail_at{Err(io::Error::other("injected interruption"))}else{Ok(())}).unwrap();
            assert_eq!(outcome.removed_files.len(),fail_at.min(2));
            assert_eq!(outcome.error.is_some(),fail_at<3);
            assert_eq!(outcome.registration.is_some(),fail_at==3);
            for fq in &outcome.removed_files {assert!(!root.join(format!("{fq}.flowgraph.toml")).exists());}
            assert_eq!(fs::read(root.join("unrelated.flowgraph.toml")).unwrap(),bytes);
            // Recover using only rediscovered disk state, not the operation result.
            let history=removal_recovery_history(&root).unwrap();
            let recovered=restore_removal(&root,history[0].recovery.as_ref().unwrap()).unwrap();
            assert_eq!(recovered.document,before.document);
            for fq in ["a","b"]{assert_eq!(fs::read(root.join(format!("{fq}.flowgraph.toml"))).unwrap(),bytes);}
            // A reviewed source change must reject before another journal/deletion.
            let files=vec!["a".into(),"b".into()];
            let plan=super::super::removal_plan(&root,&recovered,"pair",files.clone()).unwrap();
            fs::write(root.join("a.flowgraph.toml"),"[meta]\ntitle='User edit'\n").unwrap();
            assert!(remove_purpose_files(&root,&recovered,"pair",files,&plan).is_err());
            assert_eq!(removal_recovery_history(&root).unwrap().len(),1);
            assert!(root.join("b.flowgraph.toml").exists());
            fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn removal_record_restores_partial_removal_and_rejects_conflicts() {
        let root=std::env::temp_dir().join(format!("vac-removal-record-{}",rand::random::<u64>()));
        fs::create_dir(&root).unwrap();
        for fq in ["a","b"]{fs::write(root.join(format!("{fq}.flowgraph.toml")),"[meta]\ntitle='Original'\n").unwrap();}
        let before=save(&root,&read(&root).unwrap(),Document{schema_version:1,purposes:vec![Purpose{id:"pair".into(),name:"Pair".into(),roots:vec!["a".into(),"b".into()]}]}).unwrap();
        let files=vec!["a".into(),"b".into()];
        let plan=super::super::removal_plan(&root,&before,"pair",files.clone()).unwrap();
        let record=prepare_removal_recovery(&root,&before,"pair",files,&plan).unwrap();
        assert_eq!(removal_recovery_history(&root).unwrap()[0].recovery.as_ref().unwrap().revision,record.revision);
        // Simulate interruption after only one file was removed.
        fs::remove_file(root.join("a.flowgraph.toml")).unwrap();
        assert_eq!(restore_removal(&root,&record).unwrap().document,before.document);
        // Then simulate all files and the registration removed.
        for fq in ["a","b"]{fs::remove_file(root.join(format!("{fq}.flowgraph.toml"))).unwrap();}
        save(&root,&read(&root).unwrap(),Document{schema_version:1,purposes:vec![]}).unwrap();
        fs::write(root.join("b.flowgraph.toml"),"Edited").unwrap();
        assert!(restore_removal(&root,&record).is_err());
        assert!(!root.join("a.flowgraph.toml").exists()); // preflight before any write
        fs::remove_file(root.join("b.flowgraph.toml")).unwrap();
        assert_eq!(restore_removal(&root,&record).unwrap().document,before.document);
        assert_eq!(restore_removal(&root,&record).unwrap().document,before.document);
        let mut renamed=before.document.clone();renamed.purposes[0].name="User edit".into();
        save(&root,&read(&root).unwrap(),renamed.clone()).unwrap();
        assert!(restore_removal(&root,&record).is_err());
        assert_eq!(read(&root).unwrap().document,renamed);
        let foreign=root.with_extension("foreign"); fs::create_dir(&foreign).unwrap();
        fs::copy(record_path(&root,&record.id).unwrap(),record_path(&foreign,&record.id).unwrap()).unwrap();
        assert!(restore_removal(&foreign,&record).is_err());
        fs::write(record_path(&root,&record.id).unwrap(),b"broken").unwrap();
        assert!(restore_removal(&root,&record).is_err());
        assert!(removal_recovery_history(&root).unwrap()[0].error.is_some());
        fs::remove_dir_all(root).unwrap(); fs::remove_dir_all(foreign).unwrap();
    }
}
