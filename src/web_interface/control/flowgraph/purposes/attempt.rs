//! Durable discovery of interrupted transfers. No automatic retry or rollback.
use std::{io, path::Path};
use serde_json::{json, Value};
use crate::flowgraph::fragment::zip_codec::ZipImportOptions;
const PREFIX: &str = ".flowgraph-purpose-transfer-";

pub(super) fn begin(root: &Path, expected: &super::Snapshot, archive: &[u8], options: &ZipImportOptions) -> io::Result<String> {
    if options.dry_run { return Err(super::invalid("preview cannot start a transfer")); }
    let encoded_options = serde_json::to_value(options)?;
    let decoded_options: ZipImportOptions = serde_json::from_value(encoded_options.clone())?;
    if options.paste.position_offset != decoded_options.paste.position_offset {
        return Err(super::invalid("non-finite import position"));
    }
    let _guard = super::super::write_lock::acquire(root)?;
    let current = super::read_unlocked(root)?;
    if current.root != expected.root { return Err(super::invalid("purpose root changed")); }
    if current.revision != expected.revision { return Err(super::invalid("purpose revision changed")); }
    if &current != expected { return Err(super::invalid("purpose selection changed")); }
    let id = format!("{:032x}", rand::random::<u128>());
    let record = json!({"schema_version":1,"id":id,"root":current.root,
        "purpose_id":format!("imported-{id}"),"target_prefix":options.target_prefix,"options":encoded_options,"started_at_ms":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis(),
        "archive_digest":blake3::hash(archive).to_hex().to_string(),
        "before":current,"status":"started","reload_attempted":false});
    super::super::file_replace::create(&root.join(format!("{PREFIX}{id}.json")), &serde_json::to_vec_pretty(&record)?)?;
    Ok(id)
}

/// The caller holds the graph-root write lock for the complete import.
pub(super) struct Observer<'a> {
    root: &'a Path, id: &'a str, sequence: std::cell::Cell<u64>,
    prepared: std::cell::RefCell<std::collections::BTreeMap<String, Value>>,
}
impl<'a> Observer<'a> {
    pub(super) fn new(root: &'a Path, id: &'a str) -> Self {
        Self { root, id, sequence: std::cell::Cell::new(0), prepared:Default::default() }
    }
}
impl crate::flowgraph::fragment::paste::WriteObserver for Observer<'_> {
    fn purpose_id(&self)->Option<String>{Some(format!("imported-{}",self.id))}
    fn graphs_planned(&self,files:&[crate::flowgraph::fragment::paste::PlannedGraph],report:&crate::flowgraph::fragment::paste::PasteReport)->io::Result<()> {
        // Prepare every original before the importer may publish its first graph.
        // If preparation fails, no graph has changed. Publish the complete plan
        // only after all intents are durable; later untouched graphs are then
        // just as recoverable as the graph at the interrupted write boundary.
        for file in files {
            let relative=format!("{}.flowgraph.toml",file.fq);
            let current=super::attempt_inspection::current_digest(self.root,&relative)?;
            if current!=file.before_digest {return Err(super::invalid("graph changed before recovery preparation"));}
            let before=if current.is_some(){Some(std::fs::read(self.root.join(&relative))?)}else{None};
            if before.as_ref().map(|bytes|blake3::hash(bytes).to_hex().to_string())!=file.before_digest {
                return Err(super::invalid("graph changed during recovery preparation"));
            }
            self.before_write(&self.root.join(relative),before.as_deref(),file.content.as_bytes(),Some(&file.original_fq))?;
        }
        let record=json!({"schema_version":1,"id":self.id,"root":super::super::graph_root_identity(self.root),
            "state":"graphs_planned","files":files,"report":report});
        super::super::file_replace::create(&self.root.join(format!("{PREFIX}{}.graph-plan.json",self.id)),&serde_json::to_vec_pretty(&record)?)
    }
    fn graphs_completed(&self, report:&crate::flowgraph::fragment::paste::PasteReport) -> io::Result<()> {
        let record=json!({"schema_version":1,"id":self.id,"root":super::super::graph_root_identity(self.root),
            "state":"graphs_completed","report":report});
        super::super::file_replace::create(&self.root.join(format!("{PREFIX}{}.graphs.json",self.id)),&serde_json::to_vec_pretty(&record)?)
    }

    fn before_write(&self, path: &Path, before: Option<&[u8]>, after: &[u8], source: Option<&str>) -> io::Result<()> {
        let relative = path.strip_prefix(self.root).map_err(|_| super::invalid("write intent outside root"))?;
        let relative = relative.to_string_lossy().replace('\\', "/");
        if relative.split('/').any(|part| matches!(part,""|"."|"..") || part.contains(':')) {
            return Err(super::invalid("invalid write intent path"));
        }
        if let Some(prepared)=self.prepared.borrow().get(&relative) {
            let before_digest=before.map(|bytes|blake3::hash(bytes).to_hex().to_string());
            if prepared["before_digest"]!=json!(before_digest) || prepared["source"]!=json!(source)
                || prepared["content_digest"]!=blake3::hash(after).to_hex().to_string() {
                return Err(super::invalid("prepared graph write changed"));
            }
            let sequence=prepared["sequence"].as_u64().unwrap();
            let disk:Value=serde_json::from_slice(&std::fs::read(self.root.join(format!("{PREFIX}{}.write-{sequence:08}.json",self.id)))?)?;
            if &disk!=prepared {return Err(super::invalid("prepared write intent changed"));}
            if let Some(path)=prepared["preimage"]["path"].as_str() {
                if super::attempt_inspection::current_digest(self.root,path)?!=before_digest {
                    return Err(super::invalid("prepared original changed or missing"));
                }
            }
            return Ok(());
        }
        let sequence = self.sequence.get();
        // Publish the exact preimage before the write intent (and therefore before
        // the importer may replace the destination). Never reuse the mutable .bak.
        // An interrupted intent publication may leave an orphan, but must not
        // overwrite that recovery copy when an observer is recreated.
        let preimage = match before {
            Some(bytes) => {
                let name = format!("{PREFIX}{}.before-{sequence:08}.bin", self.id);
                super::super::file_replace::create(&self.root.join(&name), bytes)?;
                Some(json!({"path":name,"content_digest":blake3::hash(bytes).to_hex().to_string(),"size":bytes.len()}))
            }
            None => None,
        };
        let record = json!({"schema_version":1,"id":self.id,"root":super::super::graph_root_identity(self.root),
            "sequence":sequence,"path":relative,"source":source,
            "before_digest":before.map(|bytes|blake3::hash(bytes).to_hex().to_string()),
            "preimage":preimage,"content_digest":blake3::hash(after).to_hex().to_string(),"state":"planned"});
        super::super::file_replace::create(&self.root.join(format!("{PREFIX}{}.write-{sequence:08}.json",self.id)), &serde_json::to_vec_pretty(&record)?)?;
        self.prepared.borrow_mut().insert(relative,record);
        self.sequence.set(sequence + 1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flowgraph::fragment::paste::WriteObserver as _;

    #[test]
    fn replacement_preimages_are_attempt_scoped_and_never_overwritten() {
        let directory = std::env::temp_dir().join(format!("vac-preimage-{:032x}",rand::random::<u128>()));
        std::fs::create_dir(&directory).unwrap();
        let root = directory.as_path();
        let target = root.join("main.flowgraph.toml");
        std::fs::write(&target, b"old bytes\r\n").unwrap();
        let id = "11111111111111111111111111111111";
        let observer = Observer::new(root, id);
        observer.before_write(&target, Some(b"old bytes\r\n"), b"new", Some("main")).unwrap();
        let intent_path = root.join(format!("{PREFIX}{id}.write-00000000.json"));
        let original_intent = std::fs::read(&intent_path).unwrap();
        let intent: Value = serde_json::from_slice(&original_intent).unwrap();
        let preimage = root.join(intent["preimage"]["path"].as_str().unwrap());
        assert_eq!(std::fs::read(&preimage).unwrap(), b"old bytes\r\n");
        assert_eq!(intent["preimage"]["content_digest"], intent["before_digest"]);
        assert_eq!(intent["preimage"]["size"], 11);
        // Recreating an interrupted observer cannot overwrite earlier evidence.
        assert!(Observer::new(root,id).before_write(&target,Some(b"different"),b"new",Some("main")).is_err());
        assert_eq!(std::fs::read(&preimage).unwrap(), b"old bytes\r\n");
        assert_eq!(std::fs::read(&intent_path).unwrap(), original_intent);
        assert_eq!(std::fs::read(&target).unwrap(), b"old bytes\r\n");
        // A different import keeps its own preimage even for the same target.
        Observer::new(root,"22222222222222222222222222222222")
            .before_write(&target,Some(b"later version"),b"newer",Some("main")).unwrap();
        assert_eq!(std::fs::read(&preimage).unwrap(), b"old bytes\r\n");
        let fresh=root.join("fresh.flowgraph.toml");
        observer.before_write(&fresh,None,b"fresh",Some("fresh")).unwrap();
        let fresh_intent:Value=serde_json::from_slice(&std::fs::read(root.join(format!("{PREFIX}{id}.write-00000001.json"))).unwrap()).unwrap();
        assert!(fresh_intent["preimage"].is_null());
        let rows=super::super::attempt_inspection::inspect(root,id).unwrap();
        assert_eq!(rows[0]["preimage"]["observation"],"verified");
        std::fs::write(&preimage,b"tampered").unwrap();
        assert_eq!(super::super::attempt_inspection::inspect(root,id).unwrap()[0]["preimage"]["observation"],"changed");
        std::fs::remove_file(&preimage).unwrap();
        assert_eq!(super::super::attempt_inspection::inspect(root,id).unwrap()[0]["preimage"]["observation"],"missing");
        let mut forged=intent.clone();forged["preimage"]["path"]=json!("../outside.bin");
        std::fs::write(&intent_path,serde_json::to_vec(&forged).unwrap()).unwrap();
        assert_eq!(super::super::attempt_inspection::inspect(root,id).unwrap()[0]["observation"],"unreadable");
        // Existing schema-1 intents remain readable, without invented recovery proof.
        let mut legacy=intent;legacy.as_object_mut().unwrap().remove("preimage");
        std::fs::write(&intent_path,serde_json::to_vec(&legacy).unwrap()).unwrap();
        let legacy_rows=super::super::attempt_inspection::inspect(root,id).unwrap();
        assert_eq!(legacy_rows[0]["observation"],"matches_previous");
        assert!(legacy_rows[0]["preimage"].is_null());
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn failed_preimage_publication_does_not_publish_intent_or_change_target() {
        let directory=std::env::temp_dir().join(format!("vac-preimage-failure-{:032x}",rand::random::<u128>()));
        std::fs::create_dir(&directory).unwrap();let root=directory.as_path();
        let id="33333333333333333333333333333333";
        let target=root.join("main.flowgraph.toml");std::fs::write(&target,b"original").unwrap();
        let blocked=root.join(format!("{PREFIX}{id}.before-00000000.bin"));
        std::fs::create_dir(&blocked).unwrap();
        assert!(Observer::new(root,id).before_write(&target,Some(b"original"),b"replacement",Some("main")).is_err());
        assert!(!root.join(format!("{PREFIX}{id}.write-00000000.json")).exists());
        assert_eq!(std::fs::read(&target).unwrap(),b"original");
        assert!(blocked.is_dir());
        std::fs::remove_dir_all(&directory).unwrap();
    }
    #[test]
    fn failed_whole_plan_preparation_keeps_all_originals_and_rechecks_prepared_evidence() {
        use crate::flowgraph::fragment::paste::{PlannedGraph,PasteReport};
        let base=std::env::temp_dir().join(format!("vac-prepare-all-{:032x}",rand::random::<u128>()));
        std::fs::create_dir(&base).unwrap();
        let id="44444444444444444444444444444444";
        let files=["a","b"].map(|fq|{
            std::fs::write(base.join(format!("{fq}.flowgraph.toml")),b"original").unwrap();
            PlannedGraph{original_fq:fq.into(),fq:fq.into(),content:"replacement".into(),before_digest:Some(blake3::hash(b"original").to_hex().to_string())}
        });
        let report:PasteReport=serde_json::from_value(json!({"written_files":[],"imported_nodes":[],"skipped_nodes":[],"skipped_files":[],"unresolved_danglings":[],"backups":[]})).unwrap();
        std::fs::create_dir(base.join(format!("{PREFIX}{id}.before-00000001.bin"))).unwrap();
        let observer=Observer::new(&base,id);
        assert!(observer.graphs_planned(&files,&report).is_err());
        assert!(!base.join(format!("{PREFIX}{id}.graph-plan.json")).exists());
        for fq in ["a","b"] {assert_eq!(std::fs::read(base.join(format!("{fq}.flowgraph.toml"))).unwrap(),b"original");}
        // Even an in-memory prepared entry cannot authorize publishing after its
        // immutable evidence is damaged between planning and actual write.
        let original_backup=base.join(format!("{PREFIX}{id}.before-00000000.bin"));
        std::fs::write(&original_backup,b"damaged").unwrap();
        assert!(observer.before_write(&base.join("a.flowgraph.toml"),Some(b"original"),b"replacement",Some("a")).is_err());
        assert_eq!(std::fs::read(base.join("a.flowgraph.toml")).unwrap(),b"original");
        assert!(base.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn child_exits_around_replacement_publication() {
        let Some(base)=std::env::var_os("UNVAC_TEST_PREIMAGE_CRASH_ROOT") else{return};
        let base=std::path::PathBuf::from(base);let root=base.join("dest");
        let archive=std::fs::read(base.join("archive.zip")).unwrap();
        if std::env::var_os("UNVAC_TEST_REPLACEMENT_RESUME_EXIT").is_some() {
            let review:Value=serde_json::from_slice(&std::fs::read(base.join("resume-review.json")).unwrap()).unwrap();
            super::super::resume::execute_with_hook(&root,&review,&archive,||std::process::exit(43)).unwrap();
            panic!("expected exit after resumed replacement");
        }
        let mut options=ZipImportOptions{target_prefix:String::new(),..Default::default()};
        if std::env::var("UNVAC_TEST_PREIMAGE_MODE").unwrap()=="merge" {
            options.paste.on_conflict_file=crate::flowgraph::fragment::paste::OnConflictFile::Merge;
        }
        let phase=std::env::var("UNVAC_TEST_PREIMAGE_PHASE").unwrap();
        let id=begin(&root,&super::super::read(&root).unwrap(),&archive,&options).unwrap();
        let _guard=super::super::super::write_lock::acquire(&root).unwrap();
        struct Crash<'a>{inner:Observer<'a>,phase:String,writes:std::cell::Cell<usize>}
        impl crate::flowgraph::fragment::paste::WriteObserver for Crash<'_> {
            fn graphs_planned(&self,files:&[crate::flowgraph::fragment::paste::PlannedGraph],report:&crate::flowgraph::fragment::paste::PasteReport)->io::Result<()>{self.inner.graphs_planned(files,report)}
            fn before_write(&self,path:&Path,before:Option<&[u8]>,after:&[u8],source:Option<&str>)->io::Result<()>{
                self.inner.before_write(path,before,after,source)?;
                if self.phase=="before"{std::process::exit(41)}
                if self.phase=="mixed" && self.writes.get()==1 {std::process::exit(44)}
                self.writes.set(self.writes.get()+1);
                Ok(())
            }
            fn graphs_completed(&self,_:&crate::flowgraph::fragment::paste::PasteReport)->io::Result<()>{std::process::exit(42)}
        }
        crate::flowgraph::fragment::zip_codec::import_zip_observed(&root,&archive,&options,Some(&Crash{inner:Observer::new(&root,&id),phase,writes:std::cell::Cell::new(0)})).unwrap();
        panic!("expected process exit around replacement");
    }

    #[test]
    fn mixed_graphs_resume_with_unvisited_originals_and_real_nodes() {
        use crate::flowgraph::fragment::{CopyRequest,CopyTarget,FragmentScope};
        for mode in ["overwrite","merge"] {
            let base=std::env::temp_dir().join(format!("vac-mixed-resume-{:032x}",rand::random::<u128>()));
            let source=base.join("source");let root=base.join("dest");
            std::fs::create_dir_all(&source).unwrap();std::fs::create_dir(&root).unwrap();
            let incoming="[[nodes]]\nid='in'\nfeature='flowgraph.ingress.web_input'\n[[nodes]]\nid='flag'\nfeature='flowgraph.state.bool'\n[[edges]]\nfrom='in:exec_out'\nto='flag:toggle'\n";
            let original="[meta]\ntitle='old'\n[[nodes]]\nid='retained'\nfeature='flowgraph.state.bool'\n";
            for fq in ["a","b","c"] {std::fs::write(source.join(format!("{fq}.flowgraph.toml")),incoming).unwrap();}
            for fq in ["a","c"] {std::fs::write(root.join(format!("{fq}.flowgraph.toml")),original).unwrap();}
            let request=CopyRequest{scope:FragmentScope::Mixed,targets:["a","b","c"].into_iter().map(|fq|CopyTarget::File{fq:fq.into()}).collect(),origin:None};
            let purpose=crate::flowgraph::fragment::zip_codec::PortablePurpose{schema_version:1,name:"Mixed".into(),roots:vec!["a".into(),"b".into(),"c".into()]};
            let archive=crate::flowgraph::fragment::zip_codec::export_zip_with_purpose(&source,&request,&[],Some(&purpose)).unwrap();
            std::fs::write(base.join("archive.zip"),&archive).unwrap();
            let test=format!("{}::child_exits_around_replacement_publication",module_path!().split_once("::").unwrap().1);
            let child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact",&test])
                .env("UNVAC_TEST_PREIMAGE_CRASH_ROOT",&base).env("UNVAC_TEST_PREIMAGE_MODE",mode).env("UNVAC_TEST_PREIMAGE_PHASE","mixed").output().unwrap();
            assert_eq!(child.status.code(),Some(44),"{}{}",String::from_utf8_lossy(&child.stdout),String::from_utf8_lossy(&child.stderr));
            let rows=history(&root).unwrap();let id=rows[0]["id"].as_str().unwrap();
            assert_eq!(rows[0]["write_intents"].as_array().unwrap().len(),3);
            let expected=super::super::read(&root).unwrap();
            let review=super::super::resume::preview(&root,&expected,id,&archive).unwrap();
            assert_eq!(review["execution_available"],true);
            let actions=review["graphs"].as_array().unwrap().iter().map(|row|row["action"].as_str().unwrap()).collect::<Vec<_>>();
            assert_eq!(actions,vec!["keep","create","replace"]);
            // A conflict in the last file must prevent creation of the middle one.
            std::fs::write(root.join("c.flowgraph.toml"),"# later edit").unwrap();
            assert!(super::super::resume::execute(&root,&review,&archive).is_err());
            assert!(!root.join("b.flowgraph.toml").exists());
            std::fs::write(root.join("c.flowgraph.toml"),original).unwrap();
            super::super::resume::execute(&root,&review,&archive).unwrap();
            let plan:Value=serde_json::from_slice(&std::fs::read(root.join(format!("{PREFIX}{id}.graph-plan.json"))).unwrap()).unwrap();
            for file in plan["files"].as_array().unwrap() {
                let fq=file["fq"].as_str().unwrap();
                let text=std::fs::read_to_string(root.join(format!("{fq}.flowgraph.toml"))).unwrap();
                assert_eq!(text,file["content"].as_str().unwrap());
                let graph=crate::flowgraph::loader::file::parse_flowgraph_file(&text,None).unwrap();
                assert_eq!(graph.nodes.len(),if mode=="merge" && fq!="b" {3}else{2});
                assert_eq!(graph.edges.len(),1);
            }
            for intent in rows[0]["write_intents"].as_array().unwrap() {
                if let Some(path)=intent["preimage"]["path"].as_str(){assert_eq!(std::fs::read_to_string(root.join(path)).unwrap(),original);}
            }
            assert_eq!(super::super::read(&root).unwrap(),expected);
            let final_review=super::super::resume::preview(&root,&expected,id,&archive).unwrap();
            super::super::resume::execute(&root,&final_review,&archive).unwrap();
            assert_eq!(super::super::read(&root).unwrap().document.purposes[0].roots,vec!["a","b","c"]);
            assert!(base.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
            std::fs::remove_dir_all(base).unwrap();
        }
    }

    #[test]
    fn replacement_preimages_survive_real_import_process_exit() {
        use crate::flowgraph::fragment::{CopyRequest,CopyTarget,FragmentScope};
        for mode in ["overwrite","merge"] {for phase in ["before","after"] {
            let base=std::env::temp_dir().join(format!("vac-preimage-crash-{:032x}",rand::random::<u128>()));
            let source=base.join("source");let root=base.join("dest");
            std::fs::create_dir_all(&source).unwrap();std::fs::create_dir(&root).unwrap();
            let original=b"# preserve exact original bytes\r\n[meta]\r\ntitle='old'\r\n";
            std::fs::write(root.join("main.flowgraph.toml"),original).unwrap();
            std::fs::write(root.join("unrelated.flowgraph.toml"),b"# unrelated").unwrap();
            std::fs::write(source.join("main.flowgraph.toml"),"[meta]\ntitle='incoming'\n").unwrap();
            let request=CopyRequest{scope:FragmentScope::File,targets:vec![CopyTarget::File{fq:"main".into()}],origin:None};
            let portable=crate::flowgraph::fragment::zip_codec::PortablePurpose{schema_version:1,name:"Recovered purpose".into(),roots:vec!["main".into()]};
            let archive=crate::flowgraph::fragment::zip_codec::export_zip_with_purpose(&source,&request,&[],Some(&portable)).unwrap();
            std::fs::write(base.join("archive.zip"),&archive).unwrap();
            let test=format!("{}::child_exits_around_replacement_publication",module_path!().split_once("::").unwrap().1);
            let child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact",&test])
                .env("UNVAC_TEST_PREIMAGE_CRASH_ROOT",&base).env("UNVAC_TEST_PREIMAGE_MODE",mode).env("UNVAC_TEST_PREIMAGE_PHASE",phase).output().unwrap();
            assert_eq!(child.status.code(),Some(if phase=="before"{41}else{42}),"{mode}/{phase}: {}{}",String::from_utf8_lossy(&child.stdout),String::from_utf8_lossy(&child.stderr));
            let rows=history(&root).unwrap();assert_eq!(rows.len(),1);assert_eq!(rows[0]["unfinished"],true);
            let intent=&rows[0]["write_intents"][0];
            assert_eq!(intent["preimage"]["observation"],"verified");
            assert_eq!(rows[0]["graph_plan"]["files"][0]["preimage"]["observation"],"verified");
            assert_eq!(rows[0]["graph_plan"]["files"][0]["preimage"]["path"],intent["preimage"]["path"]);
            assert_eq!(std::fs::read(root.join(intent["preimage"]["path"].as_str().unwrap())).unwrap(),original);
            assert_eq!(intent["observation"],if phase=="before"{"matches_previous"}else{"matches_intended"});
            assert_eq!(std::fs::read(root.join("unrelated.flowgraph.toml")).unwrap(),b"# unrelated");
            assert!(super::super::read(&root).unwrap().document.purposes.is_empty());
            let id=rows[0]["id"].as_str().unwrap();
            let expected=super::super::read(&root).unwrap();
            let review=super::super::resume::preview(&root,&expected,id,&archive).unwrap();
            assert_eq!(review["execution_available"],true);
            assert_eq!(review["graphs"][0]["action"],if phase=="before"{"replace"}else{"keep"});
            let preimage_path=root.join(intent["preimage"]["path"].as_str().unwrap());
            let target=root.join("main.flowgraph.toml");let previous=std::fs::read(&target).unwrap();
            // A changed target or recovery copy invalidates the reviewed action,
            // including a keep action after the original write already completed.
            std::fs::write(&target,b"# later user edit").unwrap();
            assert!(super::super::resume::execute(&root,&review,&archive).is_err());
            assert_eq!(std::fs::read(&target).unwrap(),b"# later user edit");
            std::fs::remove_file(&target).unwrap();
            assert!(super::super::resume::execute(&root,&review,&archive).is_err());
            assert!(!target.exists());std::fs::write(&target,&previous).unwrap();
            std::fs::write(&preimage_path,b"invalid recovery bytes").unwrap();
            assert!(super::super::resume::execute(&root,&review,&archive).is_err());
            assert_eq!(std::fs::read(&target).unwrap(),previous);
            std::fs::write(&preimage_path,original).unwrap();
            let review=if phase=="before" {
                std::fs::write(base.join("resume-review.json"),serde_json::to_vec(&review).unwrap()).unwrap();
                let child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact",&test])
                    .env("UNVAC_TEST_PREIMAGE_CRASH_ROOT",&base).env("UNVAC_TEST_REPLACEMENT_RESUME_EXIT","1").output().unwrap();
                assert_eq!(child.status.code(),Some(43),"{}{}",String::from_utf8_lossy(&child.stdout),String::from_utf8_lossy(&child.stderr));
                assert!(!root.join(format!("{PREFIX}{id}.graphs.json")).exists());
                assert_eq!(std::fs::read(&preimage_path).unwrap(),original);
                assert_eq!(super::super::read(&root).unwrap(),expected);
                assert!(super::super::resume::execute(&root,&review,&archive).is_err());
                let next=super::super::resume::preview(&root,&expected,id,&archive).unwrap();
                assert_eq!(next["graphs"][0]["action"],"keep");next
            }else{review};
            let result=super::super::resume::execute(&root,&review,&archive).unwrap();
            assert_eq!(result["requires_reinspection"],true);
            let plan:Value=serde_json::from_slice(&std::fs::read(root.join(format!("{PREFIX}{id}.graph-plan.json"))).unwrap()).unwrap();
            assert_eq!(std::fs::read(&target).unwrap(),plan["files"][0]["content"].as_str().unwrap().as_bytes());
            assert_eq!(std::fs::read(&preimage_path).unwrap(),original);
            assert_eq!(super::super::read(&root).unwrap(),expected);
            assert!(super::super::resume::execute(&root,&review,&archive).is_err());
            let registration=super::super::resume::preview(&root,&expected,id,&archive).unwrap();
            assert_eq!(registration["execution_available"],true);
            super::super::resume::execute(&root,&registration,&archive).unwrap();
            assert_eq!(super::super::read(&root).unwrap().document.purposes.len(),1);
            assert!(base.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
            std::fs::remove_dir_all(&base).unwrap();
        }}
    }

    #[test]
    fn child_exits_after_material_intent() {
        let Some(base) = std::env::var_os("UNVAC_TEST_WRITE_INTENT_BASE") else { return };
        let base = std::path::PathBuf::from(base);
        let root = base.join("dest");
        let archive = std::fs::read(base.join("archive.zip")).unwrap();
        let options = ZipImportOptions {target_prefix:"received".into(),..Default::default()};
        let id = begin(&root, &super::super::read(&root).unwrap(), &archive, &options).unwrap();
        struct ExitOnMaterial<'a>(Observer<'a>);
        impl crate::flowgraph::fragment::paste::WriteObserver for ExitOnMaterial<'_> {
            fn graphs_planned(&self,files:&[crate::flowgraph::fragment::paste::PlannedGraph],report:&crate::flowgraph::fragment::paste::PasteReport)->io::Result<()>{self.0.graphs_planned(files,report)}
            fn graphs_completed(&self,report:&crate::flowgraph::fragment::paste::PasteReport)->io::Result<()>{self.0.graphs_completed(report)}

            fn before_write(&self, path:&Path, before:Option<&[u8]>, after:&[u8], source:Option<&str>) -> io::Result<()> {
                self.0.before_write(path,before,after,source)?;
                if source == Some("README.md") { std::process::exit(29); }
                Ok(())
            }
        }
        let observer = ExitOnMaterial(Observer::new(&root,&id));
        let _guard = super::super::super::write_lock::acquire(&root).unwrap();
        crate::flowgraph::fragment::zip_codec::import_zip_observed(&root,&archive,&options,Some(&observer)).unwrap();
        panic!("expected process exit before material publication");
    }
    #[test]
    fn write_intents_survive_process_exit_before_terminal_record() {
        use crate::flowgraph::fragment::{CopyRequest,CopyTarget,FragmentScope};
        let base = std::env::temp_dir().join(format!("vac-write-intents-{:032x}", rand::random::<u128>()));
        let source = base.join("source"); let root = base.join("dest");
        std::fs::create_dir_all(&source).unwrap(); std::fs::create_dir(&root).unwrap();
        std::fs::write(source.join("main.flowgraph.toml"),"[meta]\ntitle='crash probe'\n").unwrap();
        std::fs::write(source.join("README.md"),b"material").unwrap();
        let request = CopyRequest {scope:FragmentScope::File, targets:vec![CopyTarget::File{fq:"main".into()}],origin:None};
        let archive = crate::flowgraph::fragment::zip_codec::export_zip_with_companions(&source,&request,&["README.md".into()]).unwrap();
        std::fs::write(base.join("archive.zip"),archive).unwrap();
        let test = format!("{}::child_exits_after_material_intent",module_path!().split_once("::").unwrap().1);
        let child = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact",&test])
            .env("UNVAC_TEST_WRITE_INTENT_BASE",&base).output().unwrap();
        assert_eq!(child.status.code(),Some(29),"{}{}",String::from_utf8_lossy(&child.stdout),String::from_utf8_lossy(&child.stderr));
        let rows = history(&root).unwrap(); assert_eq!(rows.len(),1); assert_eq!(rows[0]["unfinished"],true);
        assert_eq!(rows[0]["write_intents"][0]["observation"],"matches_intended");
        assert_eq!(rows[0]["write_intents"][1]["observation"],"not_created");
        let id = rows[0]["id"].as_str().unwrap();
        let plan:Value=serde_json::from_slice(&std::fs::read(root.join(format!("{PREFIX}{id}.graph-plan.json"))).unwrap()).unwrap();
        assert_eq!(plan["state"],"graphs_planned");
        assert_eq!(plan["files"][0]["original_fq"],"main");
        assert_eq!(plan["files"][0]["fq"],"received/main");
        assert_eq!(plan["files"][0]["content"].as_str().unwrap(),std::fs::read_to_string(root.join("received/main.flowgraph.toml")).unwrap());
        assert!(plan["report"]["written_files"].as_array().unwrap().is_empty());
        let checkpoint:Value=serde_json::from_slice(&std::fs::read(root.join(format!("{PREFIX}{id}.graphs.json"))).unwrap()).unwrap();
        assert_eq!(checkpoint["state"],"graphs_completed");
        assert_eq!(checkpoint["report"]["written_files"][0]["original_fq"],"main");
        assert_eq!(checkpoint["report"]["written_files"][0]["fq"],"received/main");
        assert!(checkpoint["report"]["skipped_files"].is_array());
        assert!(checkpoint["report"]["unresolved_danglings"].is_array());

        let graph:Value = serde_json::from_slice(&std::fs::read(root.join(format!("{PREFIX}{id}.write-00000000.json"))).unwrap()).unwrap();
        let material:Value = serde_json::from_slice(&std::fs::read(root.join(format!("{PREFIX}{id}.write-00000001.json"))).unwrap()).unwrap();
        assert_eq!(graph["path"],"received/main.flowgraph.toml");
        assert_eq!(graph["content_digest"],blake3::hash(&std::fs::read(root.join("received/main.flowgraph.toml")).unwrap()).to_hex().to_string());
        assert_eq!(material["path"],"received/README.md"); assert!(material["before_digest"].is_null());
        assert_eq!(material["content_digest"],blake3::hash(b"material").to_hex().to_string());
        assert!(!root.join("received/README.md").exists());
        assert!(!root.join(format!("{PREFIX}{id}.result.json")).exists());
        std::fs::write(root.join("received/main.flowgraph.toml"),b"later edit").unwrap();
        assert_eq!(history(&root).unwrap()[0]["write_intents"][0]["observation"],"changed");
        let intent_path = root.join(format!("{PREFIX}{id}.write-00000001.json"));
        let mut unsafe_intent = material.clone(); unsafe_intent["path"] = json!("../outside");
        std::fs::write(&intent_path,serde_json::to_vec(&unsafe_intent).unwrap()).unwrap();
        let inspected = history(&root).unwrap();
        assert_eq!(inspected[0]["write_intents"][1]["observation"],"unreadable");
        assert_eq!(std::fs::read(root.join("received/main.flowgraph.toml")).unwrap(),b"later edit");

        assert!(base.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn inspection_distinguishes_previous_missing_and_invalid_destinations() {
        use crate::flowgraph::fragment::paste::WriteObserver;
        let root = std::env::temp_dir().join(format!("vac-inspect-{:032x}",rand::random::<u128>()));
        std::fs::create_dir(&root).unwrap();
        let id = begin(&root,&super::super::read(&root).unwrap(),b"archive",&ZipImportOptions::default()).unwrap();
        let path = root.join("main.flowgraph.toml"); std::fs::write(&path,b"old").unwrap();
        Observer::new(&root,&id).before_write(&path,Some(b"old"),b"new",Some("main")).unwrap();
        assert_eq!(history(&root).unwrap()[0]["write_intents"][0]["observation"],"matches_previous");
        std::fs::write(&path,b"new").unwrap();
        assert_eq!(history(&root).unwrap()[0]["write_intents"][0]["observation"],"matches_intended");
        std::fs::remove_file(&path).unwrap();
        assert_eq!(history(&root).unwrap()[0]["write_intents"][0]["observation"],"missing");
        std::fs::create_dir(&path).unwrap();
        assert_eq!(history(&root).unwrap()[0]["write_intents"][0]["observation"],"unreadable");
        std::fs::write(root.join("blocker"),b"existing file").unwrap();
        let blocked = root.join("blocker/child.txt");
        let next_observer=Observer::new(&root,&id); next_observer.sequence.set(1);
        next_observer.before_write(&blocked,None,b"planned",Some("child.txt")).unwrap();
        let inspected=history(&root).unwrap();
        assert_eq!(inspected[0]["write_intents"][1]["observation"],"unreadable");
        assert_eq!(inspected[0]["write_intents"][1]["path"],"blocker/child.txt");
        let intent_path=root.join(format!("{PREFIX}{id}.write-00000000.json"));
        let mut intent:Value=serde_json::from_slice(&std::fs::read(&intent_path).unwrap()).unwrap();
        intent.as_object_mut().unwrap().remove("before_digest");
        std::fs::write(&intent_path,serde_json::to_vec(&intent).unwrap()).unwrap();
        assert!(history(&root).unwrap()[0]["write_intents"][0]["error"].as_str().unwrap().contains("incomplete"));
        assert!(root.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn child_reads_interrupted_transfer() {
        let Some(root) = std::env::var_os("UNVAC_TEST_TRANSFER_JOURNAL_ROOT") else { return };
        let rows = history(Path::new(&root)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["unfinished"], true);
        assert_eq!(rows[0]["started"]["target_prefix"], "received");
        assert_eq!(rows[0]["started"]["options"]["paste"]["on_conflict_file"], "overwrite");
    }
    #[test]
    fn interrupted_transfer_survives_process_and_rejects_foreign_root() {
        let root = std::env::temp_dir().join(format!("vac-attempt-{:032x}", rand::random::<u128>()));
        std::fs::create_dir(&root).unwrap();
        let id = begin(&root, &super::super::read(&root).unwrap(), b"test archive", &ZipImportOptions { target_prefix:"received".into(), ..Default::default() }).unwrap();
        let test = format!("{}::child_reads_interrupted_transfer", module_path!().split_once("::").unwrap().1);
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &test]).env("UNVAC_TEST_TRANSFER_JOURNAL_ROOT", &root).output().unwrap();
        assert!(child.status.success(), "{}", String::from_utf8_lossy(&child.stdout));
        assert!(String::from_utf8_lossy(&child.stdout).contains("1 passed"));
        finish(&root, &id, &json!({"status":"failed","error":"sample"})).unwrap();
        assert_eq!(history(&root).unwrap()[0]["unfinished"], false);
        assert!(finish(&root, &id, &json!({"status":"overwrite"})).is_err());
        let foreign = root.join("other"); std::fs::create_dir(&foreign).unwrap();
        let name = format!("{PREFIX}{id}.json");
        std::fs::copy(root.join(&name), foreign.join(&name)).unwrap();
        assert!(history(&foreign).unwrap()[0]["error"].as_str().unwrap().contains("identity changed"));
        assert!(root.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn options_survive_reopen_and_reject_corrupt_replay_conditions() {
        let root = std::env::temp_dir().join(format!("vac-attempt-options-{:032x}", rand::random::<u128>()));
        std::fs::create_dir(&root).unwrap();
        let snapshot = super::super::read(&root).unwrap();
        let options: ZipImportOptions = serde_json::from_value(json!({
            "dry_run":false,"target_prefix":"custom",
            "paste":{"on_conflict_file":"merge","on_conflict_node":"skip",
                "position_offset":[12.5,-9.0],"remap":{"old:out":"new:in","discard":null}}
        })).unwrap();
        let id = begin(&root, &snapshot, b"archive", &options).unwrap();
        let row = &history(&root).unwrap()[0];
        assert_eq!(row["started"]["options"], serde_json::to_value(&options).unwrap());
        let path = root.join(format!("{PREFIX}{id}.json"));
        let original: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for corrupt in [json!("rename-unknown"), json!(null)] {
            let mut changed = original.clone();
            changed["options"]["paste"]["on_conflict_file"] = corrupt;
            std::fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
            assert!(history(&root).unwrap()[0]["error"].is_string());
        }
        let mut old = original.clone(); old.as_object_mut().unwrap().remove("options");
        std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
        assert!(history(&root).unwrap()[0].get("error").is_none());
        assert!(begin(&root, &snapshot, b"preview", &ZipImportOptions { dry_run:true,..options.clone() }).is_err());
        let mut invalid = options; invalid.paste.position_offset = Some([f64::NAN,0.0]);
        assert!(begin(&root, &snapshot, b"invalid", &invalid).is_err());
        assert_eq!(history(&root).unwrap().len(),1);
        assert!(root.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn corrupt_result_is_unfinished_and_does_not_hide_other_records() {
        let root = std::env::temp_dir().join(format!("vac-attempt-corrupt-{:032x}", rand::random::<u128>()));
        std::fs::create_dir(&root).unwrap();
        let snapshot = super::super::read(&root).unwrap();
        let good = begin(&root, &snapshot, b"good", &ZipImportOptions { target_prefix:"good".into(), ..Default::default() }).unwrap();
        finish(&root, &good, &json!({"status":"failed","error":"ordinary failure"})).unwrap();
        let bad = begin(&root, &snapshot, b"bad", &ZipImportOptions { target_prefix:"bad".into(), ..Default::default() }).unwrap();
        let path = root.join(format!("{PREFIX}{bad}.result.json"));
        for outcome in [
            json!({"status":"unexpected"}),
            json!({"status":"returned","report":{}}),
            json!({"status":"failed","error":"partial","progress":{"error":"partial_write","reload_attempted":false,"written_files":5,"written_companions":[],"backups":[]}}),
        ] {
            std::fs::write(&path, serde_json::to_vec(&json!({"schema_version":1,"id":bad,"root":snapshot.root,"outcome":outcome})).unwrap()).unwrap();
            let rows = history(&root).unwrap();
            assert_eq!(rows.len(), 2);
            let row = rows.iter().find(|row| row["id"] == bad).unwrap();
            assert_eq!(row["unfinished"], true);
            assert!(row["error"].is_string());
            assert!(row.get("result").is_none());
            assert_eq!(rows.iter().find(|row| row["id"] == good).unwrap()["unfinished"], false);
        }
        std::fs::write(root.join(format!("{PREFIX}{bad}.json")), b"{broken").unwrap();
        let rows = history(&root).unwrap();
        assert_eq!(rows.iter().find(|row| row["id"] == bad).unwrap()["unfinished"], true);
        assert!(root.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(root).unwrap();
    }
}

pub(super) fn finish(root: &Path, id: &str, outcome: &Value) -> io::Result<()> {
    let _guard = super::super::write_lock::acquire(root)?;
    let record = json!({"schema_version":1,"id":id,"root":super::super::graph_root_identity(root),"outcome":outcome});
    super::super::file_replace::create(&root.join(format!("{PREFIX}{id}.result.json")), &serde_json::to_vec_pretty(&record)?)
}

pub(super) fn read_record(root: &Path, path: &Path, id: &str) -> io::Result<Value> {
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 16 * 1024 * 1024 {
        return Err(super::invalid("invalid transfer journal"));
    }
    let value: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    if value["schema_version"] != 1 || value["id"] != id || value["root"] != super::super::graph_root_identity(root) {
        return Err(super::invalid("transfer journal identity changed"));
    }
    if path.file_name().is_some_and(|name| name.to_string_lossy().ends_with(".result.json")) {
        validate_outcome(&value["outcome"])?;
    } else {
        // Older records remain discoverable, but cannot authorize replay.
        if let Some(encoded) = value.get("options") {
            let options: ZipImportOptions = serde_json::from_value(encoded.clone())?;
            if options.dry_run || value["target_prefix"] != options.target_prefix || serde_json::to_value(&options)? != *encoded {
                return Err(super::invalid("invalid transfer options"));
            }
        }
        let before: super::Snapshot = serde_json::from_value(value["before"].clone())?;
        before.document.validate()?;
        let digest = value["archive_digest"].as_str().unwrap_or_default();
        if value["status"] != "started" || value["reload_attempted"] != false ||
            value["target_prefix"].as_str().is_none() || value["started_at_ms"].as_u64().is_none() ||
            before.root != value["root"] || digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(super::invalid("invalid transfer start record"));
        }
    }
    Ok(value)
}

fn validate_outcome(outcome: &Value) -> io::Result<()> {
    match outcome["status"].as_str() {
        Some("failed") => {
            if outcome["error"].as_str().is_none() { return Err(super::invalid("invalid transfer error")); }
            let progress = &outcome["progress"];
            if !progress.is_null() {
                for field in ["written_files", "written_companions", "backups"] {
                    let paths = progress[field].as_array().ok_or_else(|| super::invalid("invalid transfer progress"))?;
                    if !paths.iter().all(|path| path.is_string() || (field == "written_files" && path["path"].is_string())) {
                        return Err(super::invalid("invalid transfer progress path"));
                    }
                }
                if progress["error"] != "partial_write" || progress["reload_attempted"] != false {
                    return Err(super::invalid("invalid partial transfer status"));
                }
            }
        }
        Some("returned") => {
            let report = &outcome["report"];
            if !report.is_object() || !report["imported"].is_object() || report.get("registration").is_none() {
                return Err(super::invalid("invalid transfer result"));
            }
            if !report["registration"].is_null() {
                let purpose: super::Purpose = serde_json::from_value(report["registration"].clone())?;
                super::Document { schema_version: 1, purposes: vec![purpose] }.validate()?;
            }
        }
        _ => return Err(super::invalid("unknown transfer outcome")),
    }
    Ok(())
}

pub(super) fn history(root: &Path) -> io::Result<Vec<Value>> {
    let _guard = super::super::write_lock::acquire(root)?;
    let mut rows = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_prefix(PREFIX).and_then(|name| name.strip_suffix(".json")) else { continue };
        if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) { continue }
        let mut row = match read_record(root, &entry.path(), id) {
            Err(error) => json!({"id":id,"unfinished":true,"error":error.to_string()}),
            Ok(started) => {
                let result_path = root.join(format!("{PREFIX}{id}.result.json"));
                match read_record(root, &result_path, id) {
                    Ok(result) => json!({"id":id,"started":started,"result":result,"unfinished":false}),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => json!({"id":id,"started":started,"unfinished":true}),
                    Err(error) => json!({"id":id,"started":started,"unfinished":true,"error":error.to_string()}),
                }
            }
        };
        if row.get("started").is_some() {
            match super::graph_plan::inspect(root,id) {
                Ok(Some(plan))=>row["graph_plan"]=plan,
                Ok(None)=>(),
                Err(error)=>row["graph_plan_error"]=json!(error.to_string()),
            }
            match super::attempt_inspection::inspect(root,id) {
                Ok(intents) => row["write_intents"] = json!(intents),
                Err(error) => row["inspection_error"] = json!(error.to_string()),
            }
        }
        rows.push(row);
    }
    rows.sort_by_key(|row| row["id"].as_str().unwrap_or_default().to_owned());
    Ok(rows)
}
