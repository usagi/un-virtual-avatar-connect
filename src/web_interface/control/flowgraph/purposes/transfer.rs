//! Purpose-aware ZIP transfer. Import is storage only; install explicitly applies after registration.
use actix_web::{post, web, HttpResponse, http::StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use crate::SharedState;
use super::Snapshot;
use super::super::{parse_graph_body, util};
use crate::flowgraph::fragment::zip_codec::{self, ZipImportOptions};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportRequest { expected: Snapshot, id: String, #[serde(default)] companions: Vec<String> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportRequest { expected: Snapshot, #[serde(default)] archive: Vec<u8>, options: ZipImportOptions }

fn graph_sources(root:&Path)->anyhow::Result<std::collections::BTreeMap<std::path::PathBuf,String>> {
    crate::flowgraph::loader::walk_flowgraph_dir(root)?.into_iter().map(|path| {
        let digest=blake3::hash(&std::fs::read(&path)?).to_hex().to_string();
        Ok((path,digest))
    }).collect()
}

fn export_at(root:&Path, request:ExportRequest)->anyhow::Result<Value> {
    let (archive,mut metadata)=export_parts(root,request)?;
    metadata["archive"]=json!(archive);Ok(metadata)
}
fn export_parts(root:&Path, request:ExportRequest)->anyhow::Result<(Vec<u8>,Value)> {
    let _guard=super::super::write_lock::acquire(root)?;
    let current=super::read_unlocked(root)?;
    anyhow::ensure!(current==request.expected,"purpose selection changed");
    let purpose=current.document.purposes.iter().find(|p|p.id==request.id)
        .ok_or_else(||anyhow::anyhow!("purpose is missing"))?;
    let sources=graph_sources(root)?;
    let plan=crate::flowgraph::fragment::composition_share::plan_composition_share(root,&purpose.roots)?;
    use crate::flowgraph::fragment::{CopyRequest,CopyTarget,FragmentScope};
    let copy=CopyRequest{scope:FragmentScope::Mixed,targets:plan.files.iter().cloned().map(|fq|CopyTarget::File{fq}).collect(),origin:None};
    let portable=zip_codec::PortablePurpose{schema_version:1,name:purpose.name.clone(),roots:purpose.roots.clone()};
    let archive=zip_codec::export_zip_with_purpose(root,&copy,&request.companions,Some(&portable))?;
    anyhow::ensure!(graph_sources(root)?==sources,"purpose graph sources changed during export");
    Ok((archive,json!({"files":plan.files,"purpose":portable,"plan":plan})))
}

pub(super) fn validate_archive(bytes:&[u8])->anyhow::Result<()> {
    anyhow::ensure!(bytes.len()<=64*1024*1024,"archive exceeds 64 MiB");
    let mut archive=zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    anyhow::ensure!(archive.len()<=4096,"archive has too many entries");
    let mut expanded=0u64;
    for index in 0..archive.len() {
        expanded=expanded.checked_add(archive.by_index(index)?.size()).ok_or_else(||anyhow::anyhow!("archive size overflow"))?;
        anyhow::ensure!(expanded<=256*1024*1024,"expanded archive exceeds 256 MiB");
    }
    Ok(())
}

fn import_at(root:&Path, mut request:ImportRequest, is_preview:bool)->anyhow::Result<Value> {
    validate_archive(&request.archive)?;
    request.options.dry_run=is_preview;
    if is_preview {
        let _guard=super::super::write_lock::acquire(root)?;
        let current=super::read_unlocked(root)?;
        anyhow::ensure!(current.root==request.expected.root,"purpose root changed");
        anyhow::ensure!(current.revision==request.expected.revision,"purpose revision changed");
        Ok(serde_json::to_value(zip_codec::import_zip(root,&request.archive,&request.options)?)?)
    } else {
        let attempt = super::attempt::begin(root, &request.expected, &request.archive, &request.options)?;
        let observer = super::attempt::Observer::new(root, &attempt);
        let mut result = super::import_registered_observed(root,&request.expected,&request.archive,&request.options,&observer)
            .map(|value| serde_json::to_value(value).expect("serializable import report"));
        let outcome = match &result {
            Ok(value) => json!({"status":"returned","report":value}),
            Err(error) => {
                let progress = error.get_ref().and_then(|error| error.downcast_ref::<zip_codec::ZipImportError>())
                    .and_then(super::super::fragment_zip::zip_partial_details);
                json!({"status":"failed","error":error.to_string(),"progress":progress})
            },
        };
        let journal_error = super::attempt::finish(root, &attempt, &outcome).err().map(|error| error.to_string());
        if let Ok(value) = &mut result {
            value["attempt_id"] = json!(attempt);
            value["journal_error"] = json!(journal_error);
        }
        Ok(result?)
    }
}

#[post("/flowgraph/purposes/export")]
async fn export(state:web::Data<SharedState>,http:actix_web::HttpRequest,body:web::Bytes)->HttpResponse {
    let request=match parse_graph_body::<ExportRequest>(&body){Ok(v)=>v,Err(e)=>return e};
    let (root,_)=match util::flowgraph_dir(&state).await{Ok(v)=>v,Err(e)=>return e};
    if http.headers().get("Accept").and_then(|value|value.to_str().ok())==Some("application/vnd.unvac.purpose-zip") {
        match export_parts(&root,request).and_then(|(archive,metadata)|super::transfer_wire::encode(&metadata,&archive)) {
            Ok(body)=>HttpResponse::Ok().content_type("application/vnd.unvac.purpose-zip").body(body),
            Err(error)=>respond(Err(error)),
        }
    }else{respond(export_at(&root,request))}
}
async fn receive(state:web::Data<SharedState>,body:web::Bytes,is_preview:bool)->HttpResponse {
    let (metadata,archive)=match super::transfer_wire::decode(&body){Ok(v)=>v,Err(e)=>return e};
    let mut request=match parse_graph_body::<ImportRequest>(metadata){Ok(v)=>v,Err(e)=>return e};
    if let Some(archive)=archive {request.archive=archive.to_vec();}
    let (root,_)=match util::flowgraph_dir(&state).await{Ok(v)=>v,Err(e)=>return e};
    respond(import_at(&root,request,is_preview))
}
#[post("/flowgraph/purposes/preview")]
async fn preview(state:web::Data<SharedState>,body:web::Bytes)->HttpResponse {receive(state,body,true).await}
#[post("/flowgraph/purposes/import")]
async fn import(state:web::Data<SharedState>,body:web::Bytes)->HttpResponse {receive(state,body,false).await}
/// Separate, explicit apply operation for feature installation. Ordinary imports
/// and recovery remain storage-only, including after an interrupted install.
#[post("/flowgraph/purposes/install")]
async fn install(state:web::Data<SharedState>,body:web::Bytes)->HttpResponse {
    let (metadata,archive)=match super::transfer_wire::decode(&body){Ok(v)=>v,Err(e)=>return e};
    let mut request=match parse_graph_body::<ImportRequest>(metadata){Ok(v)=>v,Err(e)=>return e};
    if let Some(archive)=archive {request.archive=archive.to_vec();}
    let (root,_)=match util::flowgraph_dir(&state).await{Ok(v)=>v,Err(e)=>return e};
    let mut report=match import_at(&root,request,false){Ok(v)=>v,Err(e)=>return respond(Err(e))};
    report["reload_attempted"]=json!(false);
    report["reload_ok"]=Value::Null;
    // A partially saved or unregistered feature must not be announced as active.
    if report["registration"].is_null() || !report["registration_error"].is_null() {
        return HttpResponse::Ok().json(report);
    }
    let Some(_apply_guard) = lock_install_application(&root, &mut report) else {
        return HttpResponse::Ok().json(report);
    };
    let current=util::flowgraph_dir(&state).await;
    if !matches!(current,Ok((ref current,_)) if super::super::graph_root_identity(current)==super::super::graph_root_identity(&root)) {
        report["apply_error"]=json!("The active graph root changed after saving; installation was not applied");
        return HttpResponse::Ok().json(report);
    }
    report["reload_attempted"]=json!(true);
    let (ok,diagnostics,restart_required)=super::super::reload::reload_runtime_with_restart_status(&state,&root).await;
    report["reload_ok"]=json!(ok);
    report["restart_required"]=json!(restart_required);
    report["diagnostics"]=json!(diagnostics);
    HttpResponse::Ok().json(report)
}
// Import/registration has committed already. A busy apply stage must preserve its report.
fn lock_install_application(root: &Path, report: &mut Value) -> Option<std::fs::File> {
    match super::super::write_lock::acquire(root) {
        Ok(guard) => Some(guard),
        Err(error) => {
            report["apply_error"] = json!(if error.kind() == std::io::ErrorKind::WouldBlock {
                "用途は保存・登録済みですが、別の操作が同じ保存先を処理中のため適用していません。完了後に再読込してください。".to_owned()
            } else {
                format!("用途は保存・登録済みですが、適用前の排他制御に失敗しました: {error}")
            });
            None
        }
    }
}

fn respond(result:anyhow::Result<Value>)->HttpResponse {
    match result {
        Ok(value)=>HttpResponse::Ok().json(value),
        Err(error)=>{
            // Registered imports preserve the typed ZIP failure inside io::Error.
            // Keep actual progress instead of reducing a partial save to a generic conflict.
            let zip_error=error.downcast_ref::<std::io::Error>()
                .and_then(|error|error.get_ref())
                .and_then(|error|error.downcast_ref::<zip_codec::ZipImportError>())
                .or_else(||error.downcast_ref::<zip_codec::ZipImportError>());
            if let Some(zip_error)=zip_error {
                if let Some(response)=super::super::fragment_zip::zip_partial_response(zip_error){return response;}
            }
            util::err_json(StatusCode::CONFLICT,"purpose_transfer_failed",error)
        }
    }
}
#[post("/flowgraph/purposes/transfer-history")]
async fn transfer_history(state:web::Data<SharedState>)->HttpResponse {
    let (root,_)=match util::flowgraph_dir(&state).await{Ok(value)=>value,Err(error)=>return error};
    respond(super::attempt::history(&root).map(|entries| json!({"root":super::super::graph_root_identity(&root),"entries":entries})).map_err(Into::into))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResumePreviewRequest { expected:Snapshot, id:String, #[serde(default)] archive:Vec<u8> }
#[post("/flowgraph/purposes/resume-preview")]
async fn resume_preview(state:web::Data<SharedState>,body:web::Bytes)->HttpResponse {
    let (root,_)=match util::flowgraph_dir(&state).await{Ok(value)=>value,Err(error)=>return error};
    resume_preview_response(&root,&body)
}
// Keep transport decoding, validation and response mapping together so the
// production path can be checked without starting audio or application runtime.
fn resume_preview_response(root:&Path,body:&[u8])->HttpResponse {
    let (metadata,archive)=match super::transfer_wire::decode(&body){Ok(v)=>v,Err(e)=>return e};
    let mut request=match parse_graph_body::<ResumePreviewRequest>(metadata){Ok(v)=>v,Err(e)=>return e};
    if let Some(archive)=archive {request.archive=archive.to_vec();}
    respond(super::resume::preview(root,&request.expected,&request.id,&request.archive))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResumeExecuteRequest { review:Value, #[serde(default)] archive:Vec<u8> }
#[post("/flowgraph/purposes/resume-execute")]
async fn resume_execute(state:web::Data<SharedState>,body:web::Bytes)->HttpResponse {
    let (root,_)=match util::flowgraph_dir(&state).await{Ok(value)=>value,Err(error)=>return error};
    resume_execute_response(&root,&body)
}
fn resume_execute_response(root:&Path,body:&[u8])->HttpResponse {
    let (metadata,archive)=match super::transfer_wire::decode(&body){Ok(v)=>v,Err(e)=>return e};
    let mut request=match parse_graph_body::<ResumeExecuteRequest>(metadata){Ok(v)=>v,Err(e)=>return e};
    if let Some(archive)=archive {request.archive=archive.to_vec();}
    respond(super::resume::execute(root,&request.review,&request.archive))
}
pub fn configure(config:&mut web::ServiceConfig){config.service(export).service(preview).service(import).service(install).service(transfer_history).service(resume_preview).service(resume_execute);}

#[cfg(test)]
mod tests {
    use super::*;
    #[actix_web::test]
    async fn replacement_resume_binary_transport_conflict_and_registration() {
        use crate::flowgraph::fragment::{CopyRequest,CopyTarget,FragmentScope,paste::WriteObserver};
        use actix_web::{test,App};
        let base=std::env::temp_dir().join(format!("vac-resume-wire-{:032x}",rand::random::<u128>()));
        let root=base.join("dest");let source=base.join("source");
        std::fs::create_dir_all(&root).unwrap();std::fs::create_dir(&source).unwrap();
        std::fs::write(root.join("main.flowgraph.toml"),"[meta]\ntitle='original'\n").unwrap();
        std::fs::write(source.join("main.flowgraph.toml"),"[meta]\ntitle='incoming'\n").unwrap();
        let original=std::fs::read(root.join("main.flowgraph.toml")).unwrap();
        let purpose=zip_codec::PortablePurpose{schema_version:1,name:"Transport recovery".into(),roots:vec!["main".into()]};
        let archive=zip_codec::export_zip_with_purpose(&source,&CopyRequest{scope:FragmentScope::File,targets:vec![CopyTarget::File{fq:"main".into()}],origin:None},&[],Some(&purpose)).unwrap();
        let expected=super::super::read(&root).unwrap();
        let options=ZipImportOptions{target_prefix:String::new(),..Default::default()};
        let id=super::super::attempt::begin(&root,&expected,&archive,&options).unwrap();
        struct Stop<'a>(super::super::attempt::Observer<'a>);
        impl WriteObserver for Stop<'_> {
            fn graphs_planned(&self,files:&[crate::flowgraph::fragment::paste::PlannedGraph],report:&crate::flowgraph::fragment::paste::PasteReport)->std::io::Result<()>{self.0.graphs_planned(files,report)}
            fn before_write(&self,path:&Path,before:Option<&[u8]>,after:&[u8],source:Option<&str>)->std::io::Result<()>{
                self.0.before_write(path,before,after,source)?;Err(std::io::Error::other("injected stop before graph publication"))
            }
        }
        {
            let _guard=super::super::super::write_lock::acquire(&root).unwrap();
            assert!(zip_codec::import_zip_observed(&root,&archive,&options,Some(&Stop(super::super::attempt::Observer::new(&root,&id)))).is_err());
        }
        async fn preview_at(root:web::Data<std::path::PathBuf>,body:web::Bytes)->HttpResponse{resume_preview_response(root.get_ref(),&body)}
        async fn execute_at(root:web::Data<std::path::PathBuf>,body:web::Bytes)->HttpResponse{resume_execute_response(root.get_ref(),&body)}
        // In-process Actix transport; the production decoder/response functions
        // are identical. No sockets, SharedState, audio, or runtime initialization.
        let app=test::init_service(App::new().app_data(web::Data::new(root.clone()))
            .route("/resume-preview",web::post().to(preview_at)).route("/resume-execute",web::post().to(execute_at))).await;
        let preview_body=super::super::transfer_wire::encode(&json!({"expected":expected,"id":id}),&archive).unwrap();
        let review:Value=test::call_and_read_body_json(&app,test::TestRequest::post().uri("/resume-preview").set_payload(preview_body.clone()).to_request()).await;
        assert_eq!(review["execution_available"],true);assert_eq!(review["graphs"][0]["action"],"replace");
        assert_eq!(std::fs::read(root.join("main.flowgraph.toml")).unwrap(),original);
        let execute_body=super::super::transfer_wire::encode(&json!({"review":review}),&archive).unwrap();
        std::fs::write(root.join("main.flowgraph.toml"),b"# user edit").unwrap();
        let rejected=test::call_service(&app,test::TestRequest::post().uri("/resume-execute").set_payload(execute_body.clone()).to_request()).await;
        assert_eq!(rejected.status(),StatusCode::CONFLICT);
        let error:Value=test::read_body_json(rejected).await;assert_eq!(error["error"],"purpose_transfer_failed");
        assert_eq!(std::fs::read(root.join("main.flowgraph.toml")).unwrap(),b"# user edit");
        std::fs::write(root.join("main.flowgraph.toml"),&original).unwrap();
        let malformed=test::call_service(&app,test::TestRequest::post().uri("/resume-execute").set_payload(b"UNVACZIP".as_slice()).to_request()).await;
        assert_eq!(malformed.status(),StatusCode::BAD_REQUEST);
        assert_eq!(std::fs::read(root.join("main.flowgraph.toml")).unwrap(),original);
        let saved:Value=test::call_and_read_body_json(&app,test::TestRequest::post().uri("/resume-execute").set_payload(execute_body.clone()).to_request()).await;
        assert_eq!(saved["graphs_completed"],true);assert_eq!(saved["requires_reinspection"],true);assert_eq!(saved["reload_attempted"],false);
        assert_eq!(super::super::read(&root).unwrap(),expected);
        let stale=test::call_service(&app,test::TestRequest::post().uri("/resume-execute").set_payload(execute_body).to_request()).await;
        assert_eq!(stale.status(),StatusCode::CONFLICT);
        let next:Value=test::call_and_read_body_json(&app,test::TestRequest::post().uri("/resume-preview").set_payload(preview_body).to_request()).await;
        assert_eq!(next["graphs"][0]["action"],"keep");assert!(next["stage"].is_null());
        // Legacy JSON request stays interoperable with the binary review path.
        let registered:Value=test::call_and_read_body_json(&app,test::TestRequest::post().uri("/resume-execute").set_json(json!({"review":next,"archive":archive})).to_request()).await;
        assert_eq!(registered["result"]["reload_attempted"],false);
        assert_eq!(registered["result"]["registration"]["name"],"Transport recovery");
        assert_eq!(super::super::read(&root).unwrap().document.purposes.len(),1);
        assert!(base.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn child_resume_exits_before_registration() {
        let Some(base)=std::env::var_os("UNVAC_TEST_RESUME_CRASH_BASE") else{return};
        let base=std::path::PathBuf::from(base);
        let review:Value=serde_json::from_slice(&std::fs::read(base.join("resume-review.json")).unwrap()).unwrap();
        let archive=std::fs::read(base.join("resume-archive.zip")).unwrap();
        super::super::resume::execute_with_hook(&base.join("dest"),&review,&archive,||std::process::exit(31)).unwrap();
        panic!("expected interrupted resume");
    }
    #[actix_web::test]
    async fn actual_companion_io_failure_reports_saved_graphs_without_registration() {
        let base=std::env::temp_dir().join(format!("vac-transfer-io-{}-{}",std::process::id(),rand::random::<u64>()));
        std::fs::create_dir(&base).unwrap();let base=base.canonicalize().unwrap();
        let source=base.join("source");let dest=base.join("dest");
        std::fs::create_dir_all(source.join("z")).unwrap();
        std::fs::create_dir_all(dest.join("received")).unwrap();
        std::fs::write(source.join("main.flowgraph.toml"),"[meta]\ntitle='IO failure review'\n").unwrap();
        std::fs::write(source.join("a.txt"),b"first companion").unwrap();
        std::fs::write(source.join("z/second.txt"),b"second companion").unwrap();
        // A real filesystem failure: a file prevents creation of a required directory.
        std::fs::write(dest.join("received/z"),b"existing user file").unwrap();
        let selection=super::super::save(&source,&super::super::read(&source).unwrap(),super::super::Document{schema_version:1,purposes:vec![super::super::Purpose{id:"test".into(),name:"IO review".into(),roots:vec!["main".into()]}]}).unwrap();
        let exported=export_at(&source,ExportRequest{expected:selection,id:"test".into(),companions:vec!["a.txt".into(),"z/second.txt".into()]}).unwrap();
        let empty=super::super::read(&dest).unwrap();
        let request=ImportRequest{expected:empty.clone(),archive:serde_json::from_value(exported["archive"].clone()).unwrap(),options:ZipImportOptions{target_prefix:"received".into(),..Default::default()}};
        let response=respond(import_at(&dest,request,false));
        assert_eq!(response.status(),500);
        let bytes=actix_web::body::to_bytes(response.into_body()).await.unwrap();
        let body:Value=serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"],"partial_write");
        let graphs=body["written_files"].as_array().unwrap();
        assert_eq!(graphs.len(),1);
        assert!(graphs[0].as_str().unwrap().replace('\\',"/").ends_with("received/main.flowgraph.toml"));
        let companions=body["written_companions"].as_array().unwrap();
        assert_eq!(companions.len(),1);
        assert!(companions[0].as_str().unwrap().ends_with("received/a.txt"));
        assert_eq!(body["reload_attempted"],false);
        assert!(dest.join("received/main.flowgraph.toml").is_file());
        assert_eq!(std::fs::read(dest.join("received/a.txt")).unwrap(),b"first companion");
        assert_eq!(std::fs::read(dest.join("received/z")).unwrap(),b"existing user file");
        assert_eq!(super::super::read(&dest).unwrap(),empty);
        assert!(super::super::registration_recovery(&dest).unwrap().is_empty());
        let history = super::super::attempt::history(&dest).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0]["unfinished"], false);
        assert_eq!(history[0]["result"]["outcome"]["progress"]["written_files"], body["written_files"]);
        assert_eq!(history[0]["result"]["outcome"]["progress"]["written_companions"], body["written_companions"]);
        let material = &body["companion_mappings"][0];
        let material_bytes = std::fs::read(material["path"].as_str().unwrap()).unwrap();
        assert_eq!(material["content_digest"], blake3::hash(&material_bytes).to_hex().to_string());
        assert_eq!(history[0]["result"]["outcome"]["progress"]["companion_mappings"], body["companion_mappings"]);
        assert_eq!(body["file_mappings"][0]["original_fq"], "main");
        assert_eq!(body["file_mappings"][0]["fq"], "received/main");
        assert_eq!(history[0]["result"]["outcome"]["progress"]["file_mappings"], body["file_mappings"]);
        let archive:Vec<u8>=serde_json::from_value(exported["archive"].clone()).unwrap();
        let attempt_id=history[0]["id"].as_str().unwrap();
        // Emulate interruption before completion checkpoint, retaining the full plan.
        let checkpoint=dest.join(format!(".flowgraph-purpose-transfer-{attempt_id}.graphs.json"));
        let saved_checkpoint=std::fs::read(&checkpoint).unwrap();std::fs::remove_file(&checkpoint).unwrap();
        let graph_path=dest.join("received/main.flowgraph.toml");
        let saved_graph=std::fs::read(&graph_path).unwrap();std::fs::remove_file(&graph_path).unwrap();
        let partial=super::super::resume::preview(&dest,&empty,attempt_id,&archive).unwrap();
        assert_eq!(partial["stage"],"graphs_incomplete");assert_eq!(partial["graphs"][0]["action"],"create");
        assert_eq!(partial["purpose"]["roots"][0],"received/main");assert_eq!(partial["execution_available"],true);
        std::fs::write(base.join("resume-review.json"),serde_json::to_vec(&partial).unwrap()).unwrap();
        std::fs::write(base.join("resume-archive.zip"),&archive).unwrap();
        let graph_test=format!("{}::child_resume_exits_before_registration",module_path!().split_once("::").unwrap().1);
        let graph_child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact",&graph_test]).env("UNVAC_TEST_RESUME_CRASH_BASE",&base).output().unwrap();
        assert_eq!(graph_child.status.code(),Some(31),"{}{}",String::from_utf8_lossy(&graph_child.stdout),String::from_utf8_lossy(&graph_child.stderr));
        assert_eq!(std::fs::read(&graph_path).unwrap(),saved_graph);
        assert!(!checkpoint.exists());assert_eq!(super::super::read(&dest).unwrap(),empty);
        assert!(super::super::resume::execute(&dest,&partial,&archive).is_err());
        let continued=super::super::resume::preview(&dest,&empty,attempt_id,&archive).unwrap();
        assert_eq!(continued["graphs"][0]["action"],"keep");
        let result=super::super::resume::execute(&dest,&continued,&archive).unwrap();
        assert_eq!(result["requires_reinspection"],true);assert_eq!(result["reload_attempted"],false);
        assert_eq!(std::fs::read(&graph_path).unwrap(),saved_graph);
        assert_eq!(std::fs::read(&checkpoint).unwrap(),saved_checkpoint);
        assert_eq!(super::super::read(&dest).unwrap(),empty);
        assert!(super::super::resume::execute(&dest,&partial,&archive).is_err());
        let plan=super::super::resume::preview(&dest,&empty,attempt_id,&archive).unwrap();
        assert_eq!(plan["graphs"][0]["action"],"keep");
        assert_eq!(plan["materials"][0]["action"],"keep");
        assert_eq!(plan["materials"][1]["action"],"conflict");
        assert_eq!(plan["purpose"]["roots"],json!(["received/main"]));
        let mut other_archive=archive.clone();other_archive.push(0);
        assert!(super::super::resume::preview(&dest,&empty,attempt_id,&other_archive).unwrap_err().to_string().contains("archive does not match"));
        std::fs::remove_file(dest.join("received/z")).unwrap();
        let ready_plan=super::super::resume::preview(&dest,&empty,attempt_id,&archive).unwrap();
        assert_eq!(ready_plan["materials"][1]["action"],"create");
        assert!(!dest.join("received/z/second.txt").exists());
        let graph_before=std::fs::read(dest.join("received/main.flowgraph.toml")).unwrap();
        std::fs::write(base.join("resume-review.json"),serde_json::to_vec(&ready_plan).unwrap()).unwrap();
        std::fs::write(base.join("resume-archive.zip"),&archive).unwrap();
        let test=format!("{}::child_resume_exits_before_registration",module_path!().split_once("::").unwrap().1);
        let child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact",&test]).env("UNVAC_TEST_RESUME_CRASH_BASE",&base).output().unwrap();
        assert_eq!(child.status.code(),Some(31),"{}{}",String::from_utf8_lossy(&child.stdout),String::from_utf8_lossy(&child.stderr));
        assert_eq!(super::super::read(&dest).unwrap(),empty);
        assert!(super::super::resume::execute(&dest,&ready_plan,&archive).unwrap_err().to_string().contains("review changed"));
        let after_crash=super::super::resume::preview(&dest,&empty,attempt_id,&archive).unwrap();
        assert_eq!(after_crash["materials"][1]["action"],"keep");
        let result=super::super::resume::execute(&dest,&after_crash,&archive).unwrap();
        assert_eq!(result["result"]["registration"]["id"],format!("imported-{attempt_id}"));
        assert_eq!(std::fs::read(dest.join("received/z/second.txt")).unwrap(),b"second companion");
        assert_eq!(std::fs::read(dest.join("received/main.flowgraph.toml")).unwrap(),graph_before);
        let resumed=super::super::read(&dest).unwrap();assert_eq!(resumed.document.purposes.len(),1);
        assert!(super::super::resume::execute(&dest,&ready_plan,&archive).is_err());
        let done_plan=super::super::resume::preview(&dest,&resumed,attempt_id,&archive).unwrap();
        assert_eq!(done_plan["already_registered"],true);
        super::super::resume::execute(&dest,&done_plan,&archive).unwrap();
        assert_eq!(super::super::read(&dest).unwrap(),resumed);
        // Recovery must retain the actual renamed destination, not infer it from
        // the ZIP name or overwrite an older graph when the import is resumed.
        let renamed_dest = base.join("renamed");
        std::fs::create_dir_all(renamed_dest.join("received")).unwrap();
        std::fs::write(renamed_dest.join("received/main.flowgraph.toml"), "[meta]\ntitle='existing graph'\n").unwrap();
        std::fs::write(renamed_dest.join("received/z"), b"existing blocker").unwrap();
        let request = ImportRequest { expected:super::super::read(&renamed_dest).unwrap(),
            archive:serde_json::from_value(exported["archive"].clone()).unwrap(),
            options:ZipImportOptions{target_prefix:"received".into(),paste:crate::flowgraph::fragment::paste::PasteOptions {
                on_conflict_file:crate::flowgraph::fragment::paste::OnConflictFile::Rename,..Default::default()
            },..Default::default()} };
        let response = respond(import_at(&renamed_dest,request,false));
        assert_eq!(response.status(),500);
        let bytes = actix_web::body::to_bytes(response.into_body()).await.unwrap();
        let renamed:Value = serde_json::from_slice(&bytes).unwrap();
        let mapping = &renamed["file_mappings"][0];
        assert_eq!(mapping["original_fq"],"main");
        assert_ne!(mapping["fq"],"received/main");
        assert!(mapping["fq"].as_str().unwrap().starts_with("received/main"));
        let written:toml::Value = toml::from_str(&std::fs::read_to_string(mapping["path"].as_str().unwrap()).unwrap()).unwrap();
        assert_eq!(written["meta"]["title"].as_str(),Some("IO failure review"));
        assert_eq!(std::fs::read_to_string(renamed_dest.join("received/main.flowgraph.toml")).unwrap(),"[meta]\ntitle='existing graph'\n");
        let history = super::super::attempt::history(&renamed_dest).unwrap();
        assert_eq!(history[0]["result"]["outcome"]["progress"]["file_mappings"][0],*mapping);
        assert_eq!(history[0]["started"]["options"]["paste"]["on_conflict_file"], "rename");
        let renamed_plan=super::super::resume::preview(&renamed_dest,&super::super::read(&renamed_dest).unwrap(),history[0]["id"].as_str().unwrap(),&archive).unwrap();
        assert_eq!(renamed_plan["purpose"]["roots"][0],mapping["fq"]);
        let published = std::fs::read(mapping["path"].as_str().unwrap()).unwrap();
        assert_eq!(mapping["content_digest"], blake3::hash(&published).to_hex().to_string());
        std::fs::write(mapping["path"].as_str().unwrap(), b"# later user edit").unwrap();
        assert!(super::super::resume::preview(&renamed_dest,&super::super::read(&renamed_dest).unwrap(),history[0]["id"].as_str().unwrap(),&archive).unwrap_err().to_string().contains("saved graph changed"));
        let retained = super::super::attempt::history(&renamed_dest).unwrap();
        assert_eq!(retained[0]["result"]["outcome"]["progress"]["file_mappings"][0]["content_digest"], mapping["content_digest"]);
        assert!(base.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert!(base.file_name().unwrap().to_string_lossy().starts_with("vac-transfer-io-"));
        std::fs::remove_dir_all(base).unwrap();
    }
    #[actix_web::test]
    async fn registered_import_response_retains_partial_companion_progress() {
        let failure=zip_codec::ZipImportError::CompanionWrite {
            source:std::io::Error::new(std::io::ErrorKind::PermissionDenied,"injected denial"),
            failed_path:"received/second.png".into(),written_files:vec!["received/main.flowgraph.toml".into()],
            file_mappings:vec![], companion_mappings:vec![],
            written_companions:vec!["received/first.png".into()],backups:vec!["before-main.bak".into()],
        };
        let response=respond(Err(std::io::Error::other(failure).into()));
        assert_eq!(response.status(),500);
        let bytes=actix_web::body::to_bytes(response.into_body()).await.unwrap();
        let body:Value=serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"],"partial_write");
        assert_eq!(body["written_files"],json!(["received/main.flowgraph.toml"]));
        assert_eq!(body["written_companions"],json!(["received/first.png"]));
        assert_eq!(body["backups"],json!(["before-main.bak"]));
        assert_eq!(body["reload_attempted"],false);
        assert_eq!(respond(Err(anyhow::anyhow!("purpose revision changed"))).status(),409);
    }
    #[test]
    fn ai_free_twitch_examples_transfer_to_fresh_profile() {
        for name in ["twitch-event-notices", "twitch-random-clip"] {
            let base=std::env::temp_dir().join(format!("vac-example-transfer-{}-{}",std::process::id(),rand::random::<u64>()));
            std::fs::create_dir(&base).unwrap(); let base=base.canonicalize().unwrap();
            let source=base.join("source"); let dest=base.join("dest");
            std::fs::create_dir(&source).unwrap(); std::fs::create_dir(&dest).unwrap();
            let example=Path::new(env!("CARGO_MANIFEST_DIR")).join("flowgraph.example").join(name);
            let text=std::fs::read_to_string(example.join("main.flowgraph.toml")).unwrap();
            let instructions=std::fs::read(example.join("README.md")).unwrap();
            std::fs::write(source.join("main.flowgraph.toml"),&text).unwrap();
            std::fs::write(source.join("README.md"),&instructions).unwrap();
            let selection=super::super::save(&source,&super::super::read(&source).unwrap(),super::super::Document{schema_version:1,purposes:vec![super::super::Purpose{id:name.into(),name:name.into(),roots:vec!["main".into()]}]}).unwrap();
            let exported=export_at(&source,ExportRequest{expected:selection,id:name.into(),companions:vec!["README.md".into()]}).unwrap();
            let imported=import_at(&dest,ImportRequest{expected:super::super::read(&dest).unwrap(),archive:serde_json::from_value(exported["archive"].clone()).unwrap(),options:ZipImportOptions{target_prefix:"received".into(),..Default::default()}},false).unwrap();
            assert!(imported["registration_error"].is_null(),"{name}: {imported}");
            crate::flowgraph::loader::load_flowgraph_dir(&dest).unwrap();
            let original:toml::Value=toml::from_str(&text).unwrap();
            let restored:toml::Value=toml::from_str(&std::fs::read_to_string(dest.join("received/main.flowgraph.toml")).unwrap()).unwrap();
            let source_nodes=original["nodes"].as_array().unwrap();
            let dest_nodes=restored["nodes"].as_array().unwrap();
            assert_eq!(source_nodes.len(),dest_nodes.len());
            for node in source_nodes {
                let found=dest_nodes.iter().find(|candidate| candidate.get("id")==node.get("id")).unwrap();
                assert_eq!(found.get("feature"),node.get("feature"));
                assert_eq!(found.get("properties").and_then(toml::Value::as_table).cloned().unwrap_or_default(),node.get("properties").and_then(toml::Value::as_table).cloned().unwrap_or_default(),"{name}: external token keys and output channels must retain their meaning");
                assert!(!node["feature"].as_str().unwrap().contains(".ai."));
            }
            assert_eq!(std::fs::read(dest.join("received/README.md")).unwrap(),instructions);
            let registered=super::super::read(&dest).unwrap();
            assert_eq!(registered.document.purposes.len(),1);
            assert_eq!(registered.document.purposes[0].roots,vec!["received/main"]);
            assert_eq!(std::fs::read_to_string(source.join("main.flowgraph.toml")).unwrap(),text);
            std::fs::remove_dir_all(base).unwrap();
        }
    }

    #[test]
    fn purpose_transfer_keeps_dependencies_companions_and_target_identity() {
        let base=std::env::temp_dir().join(format!("vac-transfer-{}-{}",std::process::id(),rand::random::<u64>()));
        std::fs::create_dir(&base).unwrap();let base=base.canonicalize().unwrap();
        let source=base.join("source");let dest=base.join("dest");
        std::fs::create_dir(&source).unwrap();std::fs::create_dir(&dest).unwrap();
        std::fs::write(source.join("module.flowgraph.toml"),"[graph]\nkind='module'\n[[nodes]]\nid='counter'\nfeature='flowgraph.state.int_counter'\n").unwrap();
        std::fs::write(source.join("main.flowgraph.toml"),"[[instances]]\nid='child'\ngraph='module'\n").unwrap();
        std::fs::write(source.join("README.md"),"Purpose instructions").unwrap();
        let selection=super::super::save(&source,&super::super::read(&source).unwrap(),super::super::Document{schema_version:1,purposes:vec![super::super::Purpose{id:"test".into(),name:"Reusable function".into(),roots:vec!["main".into()]}]}).unwrap();
        let exported=export_at(&source,ExportRequest{expected:selection.clone(),id:"test".into(),companions:vec!["README.md".into()]}).unwrap();
        let files=exported["files"].as_array().unwrap();assert!(files.contains(&json!("main")));assert!(files.contains(&json!("module")));
        let bytes:Vec<u8>=serde_json::from_value(exported["archive"].clone()).unwrap();
        let empty=super::super::read(&dest).unwrap();
        let make_request=|expected:Snapshot| ImportRequest{expected,archive:bytes.clone(),options:ZipImportOptions{target_prefix:"received".into(),..Default::default()}};
        let checked=import_at(&dest,make_request(empty.clone()),true).unwrap();
        assert_eq!(checked["purpose"]["name"],"Reusable function");
        assert!(!dest.join("received").exists());
        let foreign=import_at(&dest,make_request(selection.clone()),true).unwrap_err();assert!(foreign.to_string().contains("root changed"));
        assert!(export_at(&dest,ExportRequest{expected:selection,id:"test".into(),companions:vec![]}).is_err());
        let imported=import_at(&dest,make_request(empty.clone()),false).unwrap();
        assert!(imported["registration_error"].is_null());
        assert_eq!(imported["registration"]["name"],"Reusable function");
        assert!(dest.join("received/main.flowgraph.toml").exists());
        assert!(dest.join("received/module.flowgraph.toml").exists());
        assert_eq!(std::fs::read_to_string(dest.join("received/README.md")).unwrap(),"Purpose instructions");
        let snapshot=super::super::read(&dest).unwrap();
        assert_eq!(snapshot.document.purposes.len(),1);
        assert_eq!(snapshot.document.purposes[0].roots,vec!["received/main"]);
        assert!(import_at(&dest,make_request(empty),false).unwrap_err().to_string().contains("revision changed"));
        assert_eq!(super::super::read(&dest).unwrap(),snapshot);
        assert!(validate_archive(b"not a zip").is_err());
        assert!(base.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert!(base.file_name().unwrap().to_string_lossy().starts_with("vac-transfer-"));
        std::fs::remove_dir_all(base).unwrap();
    }
}

#[cfg(test)]
mod install_apply_lock_tests {
    use super::*;

    #[test]
    fn busy_apply_preserves_saved_registration_and_can_retry_after_release() {
        let root = std::env::temp_dir().join(format!("unvac-purpose-apply-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        let guard = super::super::super::write_lock::acquire(&root).unwrap();
        let mut report = json!({"registration":{"id":"saved"},"written_files":["main"],"reload_attempted":false,"reload_ok":null});
        assert!(lock_install_application(&root, &mut report).is_none());
        assert_eq!(report["registration"]["id"], "saved");
        assert_eq!(report["written_files"], json!(["main"]));
        assert_eq!(report["reload_attempted"], false);
        assert!(report["reload_ok"].is_null());
        assert!(report["apply_error"].as_str().unwrap().contains("保存・登録済み"));
        drop(guard);
        let retry_guard = lock_install_application(&root, &mut json!({})).unwrap();
        assert!(super::super::super::write_lock::acquire(&root).is_err());
        drop(retry_guard);
        std::fs::remove_file(root.join(".flowgraph-write.lock")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
