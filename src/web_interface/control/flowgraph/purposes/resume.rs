//! Read-only review for resuming material publication after completed graph import.
use std::{collections::BTreeSet, io::Read, path::Path};
use serde_json::{json, Value};
use crate::flowgraph::fragment::{paste::PasteReport, zip_codec::{self, ZipImportOptions, ZipImportOutcome}};

pub(super) fn preview(root: &Path, expected: &super::Snapshot, id: &str, archive: &[u8]) -> anyhow::Result<Value> {
    anyhow::ensure!(id.len()==32 && id.bytes().all(|b| b.is_ascii_hexdigit()), "invalid transfer id");
    super::transfer::validate_archive(archive)?;
    let _guard=super::super::write_lock::acquire(root)?;
    preview_unlocked(root,expected,id,archive)
}
fn preview_unlocked(root:&Path,expected:&super::Snapshot,id:&str,archive:&[u8])->anyhow::Result<Value>{
    let current=super::read_unlocked(root)?;
    anyhow::ensure!(&current==expected,"purpose selection changed");
    let prefix=format!(".flowgraph-purpose-transfer-{id}");
    let start=super::attempt::read_record(root,&root.join(format!("{prefix}.json")),id)?;
    anyhow::ensure!(start["archive_digest"]==blake3::hash(archive).to_hex().to_string(),"original archive does not match");
    let options:ZipImportOptions=serde_json::from_value(start.get("options").cloned().ok_or_else(||anyhow::anyhow!("legacy transfer has no replay options"))?)?;
    let checkpoint_path=root.join(format!("{prefix}.graphs.json"));
    let meta=match std::fs::symlink_metadata(&checkpoint_path) {
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=>return super::graph_plan::preview(root,id,&start,expected,archive),
        result=>result?,
    };
    anyhow::ensure!(meta.is_file() && !meta.file_type().is_symlink() && meta.len()<=16*1024*1024,"invalid graph checkpoint");
    let checkpoint_bytes=std::fs::read(checkpoint_path)?;
    let checkpoint:Value=serde_json::from_slice(&checkpoint_bytes)?;
    anyhow::ensure!(checkpoint["schema_version"]==1 && checkpoint["id"]==id && checkpoint["root"]==current.root && checkpoint["state"]=="graphs_completed","graph checkpoint identity changed");
    let report:PasteReport=serde_json::from_value(checkpoint["report"].clone())?;
    let ZipImportOutcome::Preview(incoming)=zip_codec::import_zip(root,archive,&ZipImportOptions{dry_run:true,..options.clone()})? else {unreachable!()};
    let portable=incoming.purpose.ok_or_else(||anyhow::anyhow!("archive has no portable purpose"))?;
    let mut seen=BTreeSet::new(); let mut destinations=BTreeSet::new(); let mut graphs=vec![];
    for file in &report.written_files {
        let original=file.original_fq.as_deref().ok_or_else(||anyhow::anyhow!("missing original graph mapping"))?;
        anyhow::ensure!(seen.insert(original) && destinations.insert(&file.fq),"duplicate graph mapping");
        anyhow::ensure!(incoming.entries.iter().any(|entry|entry.zip_path==format!("flowgraph/{original}.flowgraph.toml")),"graph mapping absent from archive");
        super::Document{schema_version:1,purposes:vec![super::Purpose{id:"check".into(),name:"check".into(),roots:vec![file.fq.clone()]}]}.validate()?;
        let relative=format!("{}.flowgraph.toml",file.fq);
        let expected_path=root.join(&relative).to_string_lossy().replace('\\',"/");
        anyhow::ensure!(file.path==expected_path,"graph mapping destination changed");
        let digest=super::attempt_inspection::current_digest(root,&relative)?;
        anyhow::ensure!(digest.as_deref()==Some(&file.content_digest),"saved graph changed or missing: {relative}");
        graphs.push(json!({"path":relative,"content_digest":digest,"action":"keep"}));
    }
    for entry in &incoming.entries {
        let original=entry.zip_path.strip_prefix("flowgraph/").unwrap().strip_suffix(".flowgraph.toml").unwrap();
        let destination=entry.dest_path.strip_suffix(".flowgraph.toml").unwrap();
        anyhow::ensure!(seen.contains(original) || report.skipped_files.iter().any(|skip|skip==destination),"incomplete graph checkpoint");
    }
    let mut roots=vec![]; let mut missing_roots=vec![];
    for original in portable.roots {
        match report.written_files.iter().find(|file|file.original_fq.as_ref()==Some(&original)) {
            Some(file)=>roots.push(file.fq.clone()), None=>missing_roots.push(original),
        }
    }
    let mut zip=zip::ZipArchive::new(std::io::Cursor::new(archive))?;
    let mut materials=vec![]; let mut conflicts=vec![];
    for entry in incoming.companions {
        let mut bytes=vec![]; zip.by_name(&entry.zip_path)?.read_to_end(&mut bytes)?;
        let digest=blake3::hash(&bytes).to_hex().to_string();
        let observed=super::attempt_inspection::current_digest(root,&entry.dest_path);
        let action=match &observed {
            Ok(Some(existing)) if *existing==digest=>"keep",
            Ok(None)=>"create",
            _=>{conflicts.push(entry.dest_path.clone());"conflict"},
        };
        materials.push(json!({"path":entry.dest_path,"source":entry.zip_path,"content_digest":digest,"action":action}));
    }
    let purpose_id=start["purpose_id"].as_str().filter(|value|*value==format!("imported-{id}"));
    let registered=purpose_id.and_then(|id|current.document.purposes.iter().find(|purpose|purpose.id==id));
    if let Some(purpose)=registered {anyhow::ensure!(purpose.name==portable.name && purpose.roots==roots,"registration identity is already used");}
    let available=purpose_id.is_some() && conflicts.is_empty() && missing_roots.is_empty() && !roots.is_empty() && registered.is_none();
    let plan=json!({"root":current.root,"attempt_id":id,"expected":current,"archive_digest":start["archive_digest"],
        "start_revision":blake3::hash(&serde_json::to_vec(&start)?).to_hex().to_string(),"options":options,
        "checkpoint_revision":blake3::hash(&checkpoint_bytes).to_hex().to_string(),"graphs":graphs,"materials":materials,
        "purpose":{"id":purpose_id,"name":portable.name,"roots":roots,"missing_roots":missing_roots},"conflicts":conflicts,
        "skipped_files":report.skipped_files,"unresolved_danglings":report.unresolved_danglings,
        "already_registered":registered.is_some(),"execution_available":available});
    Ok(plan)
}

pub(super) fn execute(root:&Path,review:&Value,archive:&[u8])->anyhow::Result<Value>{
    execute_with_hook(root,review,archive,||Ok(()))
}
pub(super) fn execute_with_hook(root:&Path,review:&Value,archive:&[u8],after_material:impl Fn()->anyhow::Result<()>)->anyhow::Result<Value>{
    super::transfer::validate_archive(archive)?;
    let id=review["attempt_id"].as_str().ok_or_else(||anyhow::anyhow!("missing attempt id"))?;
    anyhow::ensure!(id.len()==32 && id.bytes().all(|b|b.is_ascii_hexdigit()),"invalid transfer id");
    let expected:super::Snapshot=serde_json::from_value(review["expected"].clone())?;
    let _guard=super::super::write_lock::acquire(root)?;
    let plan=preview_unlocked(root,&expected,id,archive)?;
    anyhow::ensure!(&plan==review,"resume review changed; inspect again");
    if plan["already_registered"]==true {return Ok(json!({"snapshot":expected,"already_registered":true,"reload_attempted":false}));}
    anyhow::ensure!(plan["execution_available"]==true,"resume has unresolved conflicts or incomplete identity");
    if plan["stage"]=="graphs_incomplete" {return super::graph_plan::execute(root,id,&plan,after_material);}
    let purpose=super::Purpose{id:plan["purpose"]["id"].as_str().unwrap().into(),name:plan["purpose"]["name"].as_str().unwrap().into(),roots:serde_json::from_value(plan["purpose"]["roots"].clone())?};
    let operation=format!(".flowgraph-purpose-transfer-{id}.resume-{:032x}",rand::random::<u128>());
    // The complete remaining write plan is durable before any resumed write.
    super::super::file_replace::create(&root.join(format!("{operation}.json")),&serde_json::to_vec_pretty(&json!({"schema_version":1,"root":expected.root,"attempt_id":id,"plan":plan,"state":"planned"}))?)?;
    let mut zip=zip::ZipArchive::new(std::io::Cursor::new(archive))?;
    for material in plan["materials"].as_array().unwrap() {
        if material["action"]!="create" {continue;}
        let relative=material["path"].as_str().unwrap();
        anyhow::ensure!(super::attempt_inspection::current_digest(root,relative)?.is_none(),"material destination appeared after review");
        let mut bytes=vec![];zip.by_name(material["source"].as_str().unwrap())?.read_to_end(&mut bytes)?;
        anyhow::ensure!(material["content_digest"]==blake3::hash(&bytes).to_hex().to_string(),"material archive content changed");
        let path=root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap())?;
        anyhow::ensure!(super::attempt_inspection::current_digest(root,relative)?.is_none(),"material destination appeared before publication");
        super::super::file_replace::create(&path,&bytes)?;
        after_material()?;
    }
    let finished=preview_unlocked(root,&expected,id,archive)?;
    anyhow::ensure!(finished["checkpoint_revision"]==plan["checkpoint_revision"] && finished["start_revision"]==plan["start_revision"],"resume records changed");
    anyhow::ensure!(finished["materials"].as_array().unwrap().iter().all(|file|file["action"]=="keep"),"materials changed during resume");
    let checkpoint_bytes=std::fs::read(root.join(format!(".flowgraph-purpose-transfer-{id}.graphs.json")))?;
    anyhow::ensure!(plan["checkpoint_revision"]==blake3::hash(&checkpoint_bytes).to_hex().to_string(),"graph checkpoint changed");
    let checkpoint:Value=serde_json::from_slice(&checkpoint_bytes)?;
    let report:PasteReport=serde_json::from_value(checkpoint["report"].clone())?;
    let options:ZipImportOptions=serde_json::from_value(plan["options"].clone())?;
    let ZipImportOutcome::Preview(preview)=zip_codec::import_zip(root,archive,&ZipImportOptions{dry_run:true,..options})? else {unreachable!()};
    let companion_mappings=plan["materials"].as_array().unwrap().iter().map(|file|zip_codec::WrittenCompanion{
        source_path:file["source"].as_str().unwrap().strip_prefix("flowgraph/").unwrap().into(),
        path:root.join(file["path"].as_str().unwrap()).to_string_lossy().replace('\\',"/"),content_digest:file["content_digest"].as_str().unwrap().into(),
    }).collect::<Vec<_>>();
    let imported=zip_codec::ZipImportReport{purpose:Some(zip_codec::ImportedPurpose{name:purpose.name.clone(),roots:purpose.roots.clone(),missing_roots:vec![]}),
        manifest:preview.manifest,written_files:report.written_files.iter().map(|file|file.path.clone()).collect(),
        written_companions:companion_mappings.iter().map(|file|file.path.clone()).collect(),companion_mappings,file_mappings:report.written_files,
        danglings_unresolved:report.unresolved_danglings,target_prefix:preview.target_prefix,reload_ok:None,restart_required:None,diagnostics_count:None};
    let receipt=super::recovery::prepare_idempotent(root,&purpose,&imported)?;
    let mut document=expected.document.clone();document.purposes.push(purpose.clone());
    let snapshot=super::save_unlocked(root,&expected,document)?;
    let outcome=json!({"snapshot":snapshot,"registration":purpose,"recovery":receipt,"reload_attempted":false});
    // Registration is already durable; failure to record this result must not suggest rollback.
    let journal_error=super::super::file_replace::create(&root.join(format!("{operation}.result.json")),&serde_json::to_vec_pretty(&outcome)?).err().map(|error|error.to_string());
    Ok(json!({"result":outcome,"journal_error":journal_error}))
}
