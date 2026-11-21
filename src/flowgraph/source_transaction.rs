//! Recoverable multi-file publication, not an all-files atomic rename.
//! Callers must hold the graph-root write lock for prepare/publish/recover and
//! keep runtime reload disabled until the pending record has been cleared.
use std::{collections::BTreeMap,io,path::{Path,PathBuf}};
use serde::{Serialize,Deserialize};
use super::speech_route_sources::RedirectedSource;
pub const PENDING:&str=".flowgraph-source-transaction.pending.json";
const GENERATION:&str=".flowgraph-source-transaction.generation";

pub fn generation(root:&Path)->io::Result<Option<String>> {
    match std::fs::read_to_string(root.join(GENERATION)) {
        Ok(value) if value.len()==32 && value.bytes().all(|c|c.is_ascii_hexdigit())=>Ok(Some(value)),
        Ok(_)=>Err(invalid("invalid graph transaction generation")),
        Err(e) if e.kind()==io::ErrorKind::NotFound=>Ok(None),
        Err(e)=>Err(e),
    }
}

#[derive(Serialize,Deserialize)]
struct Change {before:String,after:String}
#[derive(Serialize,Deserialize)]
struct Journal {version:u32,id:String,root:PathBuf,changes:BTreeMap<String,Change>}

#[derive(Debug, Serialize)]
pub struct PendingStatus {
    pub transaction_id: String,
    pub changed_files: Vec<String>,
    pub conflicting_files: Vec<String>,
    pub restorable: bool,
}

/// Inspect under the root lock. Missing or externally edited source files are
/// conflicts, not permission to discard the durable recovery record.
pub fn pending_status(root: &Path) -> io::Result<Option<PendingStatus>> {
    let root = root.canonicalize()?;
    let id = match std::fs::read_to_string(root.join(PENDING)) {
        Ok(id) => id,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let journal = read_pending(&root, &id)?;
    let mut conflicting_files = Vec::new();
    for (fq, change) in &journal.changes {
        match target(&root, fq).and_then(std::fs::read_to_string) {
            Ok(current) if current == change.before || current == change.after => {},
            _ => conflicting_files.push(fq.clone()),
        }
    }
    Ok(Some(PendingStatus {
        transaction_id: id, changed_files: journal.changes.keys().cloned().collect(),
        restorable: conflicting_files.is_empty(), conflicting_files,
    }))
}

fn invalid(reason:&str)->io::Error {io::Error::new(io::ErrorKind::InvalidData,reason)}
fn target(root:&Path,fq:&str)->io::Result<PathBuf> {
    if fq.is_empty() || fq.split('/').any(|part|part.is_empty()||part=="."||part==".."||part.contains(['\\',':'])) {return Err(invalid("invalid graph path"));}
    let path=root.join(format!("{fq}.flowgraph.toml"));
    if !path.canonicalize()?.starts_with(root) {return Err(invalid("graph resolves outside transaction root"));}
    Ok(path)
}

pub fn ensure_no_pending(root:&Path)->io::Result<()> {
    match std::fs::symlink_metadata(root.join(PENDING)) {
        Ok(_)=>Err(invalid("Flowgraphの保存が未完了です。保存前の内容を復元してから読み込んでください。")),
        Err(e) if e.kind()==io::ErrorKind::NotFound=>Ok(()),
        Err(e)=>Err(e),
    }
}

/// Durable originals and complete plan precede the first graph replacement.
/// Unchanged sources must already have been checked under the caller's lock.
pub fn prepare(root:&Path,changes:&BTreeMap<String,RedirectedSource>)->io::Result<String> {
    let root=root.canonicalize()?;ensure_no_pending(&root)?;
    if changes.is_empty(){return Err(invalid("empty transaction"));}
    let mut entries=BTreeMap::new();
    for(fq,change) in changes {
        let path=target(&root,fq)?;
        if std::fs::read_to_string(path)?!=change.original {return Err(invalid("source changed before transaction preparation"));}
        entries.insert(fq.clone(),Change{before:change.original.clone(),after:change.proposed.clone()});
    }
    let id=format!("{:032x}",rand::random::<u128>());
    let journal=Journal{version:1,id:id.clone(),root:root.clone(),changes:entries};
    let bytes=serde_json::to_vec(&journal).map_err(io::Error::other)?;
    // This immutable record is retained as the recovery/history preimage.
    crate::file_publish::create(&root.join(format!(".flowgraph-source-transaction-{id}.json")),&bytes)?;
    crate::file_publish::create(&root.join(PENDING),id.as_bytes())?;
    // Remains after completion/rollback: a loader spanning the complete
    // transaction must detect it even if it never observes the pending marker.
    crate::file_publish::replace(&root.join(GENERATION),id.as_bytes())?;
    Ok(id)
}

fn read_pending(root:&Path,id:&str)->io::Result<Journal> {
    if id.len()!=32 || !id.bytes().all(|c|c.is_ascii_hexdigit()){return Err(invalid("invalid transaction identity"));}
    if std::fs::read_to_string(root.join(PENDING))?!=id {return Err(invalid("pending transaction changed"));}
    let journal:Journal=serde_json::from_slice(&std::fs::read(root.join(format!(".flowgraph-source-transaction-{id}.json")))?).map_err(io::Error::other)?;
    if journal.version!=1 || journal.id!=id || journal.root!=root || journal.changes.is_empty(){return Err(invalid("transaction record does not match root"));}
    Ok(journal)
}

pub fn publish(root:&Path,id:&str)->io::Result<()> {
    publish_with(root,id,|path,bytes|crate::file_publish::replace(path,bytes))
}

fn publish_with(root:&Path,id:&str,mut replace:impl FnMut(&Path,&[u8])->io::Result<()>)->io::Result<()> {
    let root=root.canonicalize()?;let journal=read_pending(&root,id)?;
    // Validate all preimages first, then recheck each immediately before write.
    for(fq,change) in &journal.changes {
        if std::fs::read_to_string(target(&root,fq)?)?!=change.before {return Err(invalid("source changed before publication"));}
    }
    for(fq,change) in &journal.changes {
        let path=target(&root,fq)?;
        if std::fs::read_to_string(&path)?!=change.before {return Err(invalid("source changed during publication"));}
        replace(&path,change.after.as_bytes())?;
    }
    for(fq,change) in &journal.changes {
        if std::fs::read_to_string(target(&root,fq)?)?!=change.after {return Err(invalid("published content changed"));}
    }
    read_pending(&root,id)?;
    std::fs::remove_file(root.join(PENDING))
}

/// Explicit rollback accepts only the exact before/after bytes from this plan.
/// A subsequent user edit rejects the entire recovery before its first write.
pub fn restore(root:&Path,id:&str)->io::Result<()> {
    let root=root.canonicalize()?;let journal=read_pending(&root,id)?;
    for(fq,change) in &journal.changes {
        let current=std::fs::read_to_string(target(&root,fq)?)?;
        if current!=change.before && current!=change.after {return Err(invalid("later edit requires manual recovery"));}
    }
    for(fq,change) in &journal.changes {
        let path=target(&root,fq)?;let current=std::fs::read_to_string(&path)?;
        if current==change.before {continue;}
        if current!=change.after {return Err(invalid("content changed during recovery"));}
        crate::file_publish::replace(&path,change.before.as_bytes())?;
    }
    for(fq,change) in &journal.changes {
        if std::fs::read_to_string(target(&root,fq)?)?!=change.before {return Err(invalid("restored content changed"));}
    }
    read_pending(&root,id)?;std::fs::remove_file(root.join(PENDING))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_transaction_child() {
        let Some(root)=std::env::var_os("UNVAC_TRANSACTION_INTERRUPT_ROOT") else { return; };
        let root=PathBuf::from(root).canonicalize().unwrap();
        assert!(root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("vac-interrupted-transaction-"));
        let _guard=crate::flowgraph::write_lock::acquire(&root).unwrap();
        let changes:BTreeMap<_,_>=["a","b"].into_iter().map(|name|(name.to_string(),RedirectedSource{
            original:format!("# {name} before\n"),proposed:format!("# {name} after\n"),edge_inputs:1,boundary_inputs:0,
        })).collect();
        let id=prepare(&root,&changes).unwrap();
        let mut count=0;
        let _=publish_with(&root,&id,|path,bytes|{
            count+=1;
            if count==2 { std::process::exit(73); }
            crate::file_publish::replace(path,bytes)
        });
        panic!("child did not interrupt publication");
    }

    #[test]
    fn process_exit_preserves_recovery_and_releases_write_lock() {
        let root=std::env::temp_dir().join(format!("vac-interrupted-transaction-{}",rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        for name in ["a","b"] {std::fs::write(root.join(format!("{name}.flowgraph.toml")),format!("# {name} before\n")).unwrap();}
        let test_path=format!("{}::interrupted_transaction_child",module_path!().split_once("::").unwrap().1);
        let mut command=std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact",&test_path]).env("UNVAC_TRANSACTION_INTERRUPT_ROOT",&root)
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        #[cfg(windows)] {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child=command.spawn().unwrap();
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(15);
        let status=loop {
            if let Some(status)=child.try_wait().unwrap(){break status;}
            if std::time::Instant::now()>=deadline {
                let _=child.kill();let _=child.wait();panic!("owned transaction child timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        };
        assert_eq!(status.code(),Some(73));
        let guard=crate::flowgraph::write_lock::acquire(&root).unwrap();
        let pending=pending_status(&root).unwrap().unwrap();
        assert!(pending.restorable);
        assert_eq!(std::fs::read_to_string(root.join("a.flowgraph.toml")).unwrap(),"# a after\n");
        assert_eq!(std::fs::read_to_string(root.join("b.flowgraph.toml")).unwrap(),"# b before\n");
        assert!(crate::flowgraph::load_flowgraph_dir(&root).is_err());
        restore(&root,&pending.transaction_id).unwrap();
        for name in ["a","b"] {assert_eq!(std::fs::read_to_string(root.join(format!("{name}.flowgraph.toml"))).unwrap(),format!("# {name} before\n"));}
        assert!(pending_status(&root).unwrap().is_none());
        assert!(crate::flowgraph::load_flowgraph_dir(&root).is_ok());
        drop(guard);
        let resolved=root.canonicalize().unwrap();
        assert!(resolved.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert!(resolved.file_name().unwrap().to_string_lossy().starts_with("vac-interrupted-transaction-"));
        std::fs::remove_dir_all(resolved).unwrap();
    }
    #[test]
    fn source_transaction_recovers_partial_write_and_preserves_later_edits() {
        for scenario in ["complete","partial","later-edit"] {
            let root=std::env::temp_dir().join(format!("vac-source-transaction-{}",rand::random::<u64>()));
            std::fs::create_dir(&root).unwrap();
            let changes:BTreeMap<_,_>=["a","b"].into_iter().map(|name|(name.to_string(),RedirectedSource{original:format!("# {name} before\n"),proposed:format!("# {name} after\n"),edge_inputs:1,boundary_inputs:0})).collect();
            for(fq,change) in &changes {std::fs::write(root.join(format!("{fq}.flowgraph.toml")),&change.original).unwrap();}
            let guard=crate::flowgraph::write_lock::acquire(&root).unwrap();
            assert_eq!(generation(&root).unwrap(),None);
            let id=prepare(&root,&changes).unwrap();
            assert!(ensure_no_pending(&root).is_err());
            assert_eq!(generation(&root).unwrap(),Some(id.clone()));
            assert!(crate::flowgraph::load_flowgraph_dir(&root).is_err());
            if scenario=="complete" {
                publish(&root,&id).unwrap();ensure_no_pending(&root).unwrap();
                assert!(crate::flowgraph::load_flowgraph_dir(&root).is_ok());
                for(fq,change) in &changes {assert_eq!(std::fs::read_to_string(root.join(format!("{fq}.flowgraph.toml"))).unwrap(),change.proposed);}
            } else {
                let mut count=0;
                let result=publish_with(&root,&id,|path,bytes|{count+=1;if count==2{return Err(io::Error::other("injected second-file failure"));}crate::file_publish::replace(path,bytes)});
                assert!(result.is_err());assert!(ensure_no_pending(&root).is_err());
                assert_eq!(std::fs::read_to_string(root.join("a.flowgraph.toml")).unwrap(),changes["a"].proposed);
                assert_eq!(std::fs::read_to_string(root.join("b.flowgraph.toml")).unwrap(),changes["b"].original);
                if scenario=="later-edit" {
                    std::fs::write(root.join("b.flowgraph.toml"),"# later user edit\n").unwrap();
                    assert!(restore(&root,&id).is_err());
                    assert_eq!(std::fs::read_to_string(root.join("a.flowgraph.toml")).unwrap(),changes["a"].proposed);
                    assert_eq!(std::fs::read_to_string(root.join("b.flowgraph.toml")).unwrap(),"# later user edit\n");
                } else {
                    restore(&root,&id).unwrap();ensure_no_pending(&root).unwrap();
                    assert!(crate::flowgraph::load_flowgraph_dir(&root).is_ok());
                    for(fq,change) in &changes {assert_eq!(std::fs::read_to_string(root.join(format!("{fq}.flowgraph.toml"))).unwrap(),change.original);}
                }
            }
            drop(guard);
            let resolved=root.canonicalize().unwrap();
            assert!(resolved.starts_with(std::env::temp_dir().canonicalize().unwrap()));
            assert!(resolved.file_name().unwrap().to_string_lossy().starts_with("vac-source-transaction-"));
            std::fs::remove_dir_all(resolved).unwrap();
        }
    }
}
