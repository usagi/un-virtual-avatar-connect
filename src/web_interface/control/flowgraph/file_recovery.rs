//! Explicit, revision-checked restoration of one graph from its own backup.
use std::{fs, path::{Path, PathBuf}};
use actix_web::{get, post, web::{self, Data, Json}, HttpResponse, Responder, http::StatusCode};
use serde::{Deserialize, Serialize};
use super::{util, graph_root_identity, validate_toml_for_write, file_replace, WriteFileResponse};
use crate::SharedState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag="state", rename_all="snake_case")]
pub enum CurrentFile { Missing, Present { content: String } }

#[derive(Deserialize)]
pub struct PreviewRequest { pub backup: String }

#[derive(Debug, Serialize, Deserialize)]
pub struct RecoveryPlan {
    pub root_dir: String,
    pub fq: String,
    pub backup: String,
    pub backup_content: String,
    pub current: CurrentFile,
}

fn io_error(error: std::io::Error) -> HttpResponse {
    util::err_json(if error.kind()==std::io::ErrorKind::NotFound { StatusCode::NOT_FOUND } else { StatusCode::INTERNAL_SERVER_ERROR }, "recovery_io", error)
}

fn source_path(root: &Path, fq: &str) -> Result<PathBuf,HttpResponse> {
    let path = util::fq_to_file_path(root,fq)?;
    util::ensure_within_root(root,&path)?;
    Ok(path)
}

fn backup_name_matches(path: &Path, name: &str) -> bool {
    let Some(base) = path.file_name().and_then(|s|s.to_str()) else {return false;};
    if name == format!("{base}.bak") {return true;} // Fragment/ZIP importer legacy backup.
    name.strip_prefix(&format!("{base}.bak-")).is_some_and(|suffix|
        !suffix.is_empty() && suffix.bytes().all(|b|b.is_ascii_digit() || b==b'-'))
}

#[derive(Debug, Serialize)]
pub struct HistoryEntry {
    fq: String,
    current_present: bool,
    backups: Vec<String>,
}

pub fn history(root: &Path) -> Result<Vec<HistoryEntry>,HttpResponse> {
    fn visit(root: &Path, directory: &Path, groups: &mut std::collections::BTreeMap<String,Vec<String>>) -> Result<(),HttpResponse> {
        for entry in fs::read_dir(directory).map_err(io_error)? {
            let entry=entry.map_err(io_error)?;
            let kind=entry.file_type().map_err(io_error)?;
            if kind.is_symlink() {continue;}
            if kind.is_dir() {visit(root,&entry.path(),groups)?; continue;}
            if !kind.is_file() {continue;}
            let name=entry.file_name().to_string_lossy().into_owned();
            let Some(index)=name.rfind(".flowgraph.toml.bak") else {continue;};
            let stem=&name[..index];
            if stem.is_empty() {continue;}
            let source=entry.path().with_file_name(format!("{stem}.flowgraph.toml"));
            if !backup_name_matches(&source,&name) {continue;}
            let fq=util::root_relative(root,&source).trim_end_matches(".flowgraph.toml").to_string();
            // Use the same fq/path validation as an eventual restore request.
            source_path(root,&fq)?;
            groups.entry(fq).or_default().push(name);
        }
        Ok(())
    }
    let mut groups=std::collections::BTreeMap::new();
    visit(root,root,&mut groups)?;
    groups.into_iter().map(|(fq,mut backups)| {
        backups.sort();
        let path=source_path(root,&fq)?;
        let current_present=match fs::symlink_metadata(&path) {
            Ok(_) => true,
            Err(error) if error.kind()==std::io::ErrorKind::NotFound => false,
            Err(error)=>return Err(io_error(error)),
        };
        Ok(HistoryEntry {fq,current_present,backups})
    }).collect()
}

#[get("/flowgraph/recovery-history")]
pub async fn recovery_history(state: Data<SharedState>) -> impl Responder {
    let (root,_) = match util::flowgraph_dir(&state).await {Ok(v)=>v,Err(e)=>return e};
    let _guard=match util::lock_graph_writes(&root) {Ok(v)=>v,Err(e)=>return e};
    match history(&root) {
        Ok(entries)=>HttpResponse::Ok().json(serde_json::json!({"root_dir":graph_root_identity(&root),"entries":entries})),
        Err(error)=>error,
    }
}

fn read_plain(path: &Path) -> Result<CurrentFile,HttpResponse> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind()==std::io::ErrorKind::NotFound => Ok(CurrentFile::Missing),
        Err(error) => Err(io_error(error)),
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() =>
            Err(util::err_json(StatusCode::BAD_REQUEST,"invalid_recovery_file","通常のファイルだけを復元できます")),
        Ok(_) => Ok(CurrentFile::Present {content:fs::read_to_string(path).map_err(io_error)?}),
    }
}

pub fn plan(root: &Path, fq: &str, backup: &str) -> Result<RecoveryPlan,HttpResponse> {
    let path = source_path(root,fq)?;
    if !backup_name_matches(&path,backup) {
        return Err(util::err_json(StatusCode::BAD_REQUEST,"invalid_backup","このグラフのバックアップ名を指定してください"));
    }
    let backup_path = path.with_file_name(backup);
    util::ensure_within_root(root,&backup_path)?;
    let CurrentFile::Present {content:backup_content} = read_plain(&backup_path)? else {
        return Err(util::err_json(StatusCode::NOT_FOUND,"backup_missing","バックアップがありません"));
    };
    validate_toml_for_write(&backup_content)?;
    Ok(RecoveryPlan {root_dir:graph_root_identity(root), fq:fq.into(), backup:backup.into(),backup_content,current:read_plain(&path)?})
}

// Caller holds the root lock across this operation and the subsequent reload.
pub fn restore(root: &Path, fq: &str, expected: &RecoveryPlan) -> Result<Option<String>,HttpResponse> {
    if expected.root_dir != graph_root_identity(root) || expected.fq != fq {
        return Err(util::err_json(StatusCode::CONFLICT,"stale_root","復元対象が変わりました。内容を再確認してください"));
    }
    let actual = plan(root,fq,&expected.backup)?;
    if actual.backup_content != expected.backup_content || actual.current != expected.current {
        return Err(util::err_json(StatusCode::CONFLICT,"stale_recovery","確認後に現在のファイルまたはバックアップが変わりました。復元していません"));
    }
    let path = source_path(root,fq)?;
    let backup = match actual.current {
        CurrentFile::Present {..} => {
            let name = util::make_backup(&path).map_err(io_error)?;
            let backup = util::root_relative(root,&path.with_file_name(name));
            file_replace::replace(&path,actual.backup_content.as_bytes()).map_err(io_error)?;
            Some(backup)
        }
        CurrentFile::Missing => {
            file_replace::create(&path,actual.backup_content.as_bytes()).map_err(|error| {
                if error.kind()==std::io::ErrorKind::AlreadyExists {
                    util::err_json(StatusCode::CONFLICT,"stale_recovery","復元先にファイルが作成されました。上書きしていません")
                } else {io_error(error)}
            })?;
            None
        }
    };
    Ok(backup)
}

#[get("/flowgraph/backups/{fq:.*}")]
pub async fn list_backups(state: Data<SharedState>, fq: web::Path<String>) -> impl Responder {
    let (root,_) = match util::flowgraph_dir(&state).await {Ok(v)=>v,Err(e)=>return e};
    let path = match source_path(&root,&fq) {Ok(v)=>v,Err(e)=>return e};
    let _guard = match util::lock_graph_writes(&root) {Ok(v)=>v,Err(e)=>return e};
    let entries = match fs::read_dir(path.parent().unwrap()) {Ok(v)=>v,Err(e)=>return io_error(e)};
    let mut names=Vec::new();
    for entry in entries {
        let entry=match entry {Ok(v)=>v,Err(e)=>return io_error(e)};
        let name=entry.file_name().to_string_lossy().into_owned();
        if backup_name_matches(&path,&name) {
            let kind=match entry.file_type() {Ok(v)=>v,Err(e)=>return io_error(e)};
            if kind.is_file() && !kind.is_symlink() {names.push(name);}
        }
    }
    names.sort();
    HttpResponse::Ok().json(serde_json::json!({"root_dir":graph_root_identity(&root),"fq":fq.as_str(),"backups":names}))
}

#[post("/flowgraph/recovery-plan/{fq:.*}")]
pub async fn preview_restore(state: Data<SharedState>, fq: web::Path<String>, body: Json<PreviewRequest>) -> impl Responder {
    let (root,_) = match util::flowgraph_dir(&state).await {Ok(v)=>v,Err(e)=>return e};
    let _guard = match util::lock_graph_writes(&root) {Ok(v)=>v,Err(e)=>return e};
    match plan(&root,&fq,&body.backup) {Ok(value)=>HttpResponse::Ok().json(value),Err(error)=>error}
}

#[post("/flowgraph/restore-file/{fq:.*}")]
pub async fn restore_file(state: Data<SharedState>, fq: web::Path<String>, body: web::Bytes) -> impl Responder {
    let body=match parse_restore_body(&body) {Ok(v)=>v,Err(e)=>return e};
    let (root,_) = match util::flowgraph_dir(&state).await {Ok(v)=>v,Err(e)=>return e};
    let _guard = match util::lock_graph_writes(&root) {Ok(v)=>v,Err(e)=>return e};
    let backup=match restore(&root,&fq,&body) {Ok(v)=>v,Err(e)=>return e};
    let path=match source_path(&root,&fq) {Ok(v)=>v,Err(e)=>return e};
    let (ok,diagnostics,restart_required)=super::reload::reload_runtime_with_restart_status(&state,&root).await;
    HttpResponse::Ok().json(WriteFileResponse {restart_required,fq:fq.into_inner(),path:util::root_relative(&root,&path),ok,diagnostics,backup})
}

// Uses the Flowgraph scope's 64 MiB Bytes limit. Json's independent default
// limit would reject plans containing two otherwise valid large documents.
fn parse_restore_body(body: &[u8]) -> Result<RecoveryPlan,HttpResponse> {
    super::parse_graph_body(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[actix_web::test]
    async fn large_recovery_plan_uses_flowgraph_payload_limit_and_rejects_missing_expectations() {
        async fn legacy(_body: Json<RecoveryPlan>) -> HttpResponse {HttpResponse::Ok().finish()}
        async fn current(body: web::Bytes) -> HttpResponse {
            match parse_restore_body(&body) {
                Ok(plan)=>HttpResponse::Ok().json(serde_json::json!({"bytes":plan.backup_content.len(),"current":matches!(plan.current,CurrentFile::Present{..})})),
                Err(error)=>error,
            }
        }
        let app=actix_web::test::init_service(actix_web::App::new()
            .app_data(web::PayloadConfig::new(64*1024*1024))
            .route("/old",web::post().to(legacy)).route("/restore",web::post().to(current))).await;
        let content=format!("# {}\n[meta]\ntitle='large'\n","x".repeat(1_100_000));
        let body=serde_json::json!({"root_dir":"root","fq":"main","backup":"main.flowgraph.toml.bak","backup_content":content,"current":{"state":"present","content":content}});
        let old=actix_web::test::call_service(&app,actix_web::test::TestRequest::post().uri("/old").set_json(&body).to_request()).await;
        assert_eq!(old.status(),StatusCode::PAYLOAD_TOO_LARGE,"PayloadConfig alone does not expand Json's limit");
        let current=actix_web::test::call_service(&app,actix_web::test::TestRequest::post().uri("/restore").set_json(&body).to_request()).await;
        assert_eq!(current.status(),StatusCode::OK);
        let actual:serde_json::Value=actix_web::test::read_body_json(current).await;
        assert_eq!(actual["bytes"],content.len());assert_eq!(actual["current"],true);
        for invalid in [serde_json::json!({}),serde_json::json!({"root_dir":"root","fq":"main","backup":"main.flowgraph.toml.bak","backup_content":"old"})] {
            let response=actix_web::test::call_service(&app,actix_web::test::TestRequest::post().uri("/restore").set_json(invalid).to_request()).await;
            assert_eq!(response.status(),StatusCode::BAD_REQUEST);
        }
    }
    #[test]
    fn history_discovers_deleted_nested_and_legacy_backups_only_in_this_root() {
        let root=std::env::temp_dir().join(format!("vac-history-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&root).unwrap();
        for folder in ["profile-a/one","profile-a/two","profile-b/one"] {fs::create_dir_all(root.join(folder)).unwrap();}
        let raw="[meta]\ntitle='retained'\n";
        for file in ["profile-a/one/main.flowgraph.toml.bak-20261018-120000","profile-a/one/main.flowgraph.toml.bak",
                     "profile-a/two/main.flowgraph.toml.bak-20261018-120000","profile-b/one/main.flowgraph.toml.bak-20261018-120000"] {
            fs::write(root.join(file),raw).unwrap();
        }
        fs::write(root.join("profile-a/two/main.flowgraph.toml"),raw).unwrap();
        fs::write(root.join("profile-a/one/not-a-graph.bak-20261018"),raw).unwrap();
        fs::write(root.join("profile-a/one/no.flowgraph.toml.bak-invalid"),raw).unwrap();
        let active=root.join("profile-a");
        let entries=history(&active).unwrap();
        assert_eq!(entries.len(),2);
        assert_eq!(entries[0].fq,"one/main");
        assert!(!entries[0].current_present);
        assert_eq!(entries[0].backups.len(),2);
        assert_eq!(entries[1].fq,"two/main");
        assert!(entries[1].current_present);
        let legacy=plan(&active,"one/main","main.flowgraph.toml.bak").unwrap();
        restore(&active,"one/main",&legacy).unwrap();
        assert_eq!(fs::read_to_string(active.join("one/main.flowgraph.toml")).unwrap(),raw);
        assert_eq!(history(&root.join("profile-b")).unwrap().len(),1);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn restore_checks_root_backup_and_current_and_keeps_both_versions() {
        let root=std::env::temp_dir().join(format!("vac-recovery-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&root).unwrap(); fs::create_dir(root.join("nested")).unwrap();
        let path=root.join("nested/main.flowgraph.toml");
        let old="[meta]\ntitle='old'\n";
        let new="[meta]\ntitle='new'\n";
        fs::write(&path,old).unwrap();
        let backup=util::make_backup(&path).unwrap();
        fs::remove_file(&path).unwrap();
        let missing=plan(&root,"nested/main",&backup).unwrap();
        assert_eq!(missing.current,CurrentFile::Missing);
        fs::write(&path,new).unwrap();
        assert_eq!(restore(&root,"nested/main",&missing).unwrap_err().status(),StatusCode::CONFLICT);
        assert_eq!(fs::read_to_string(&path).unwrap(),new);
        let present=plan(&root,"nested/main",&backup).unwrap();
        let other=root.join("other");fs::create_dir(&other).unwrap();
        assert_eq!(restore(&other,"nested/main",&present).unwrap_err().status(),StatusCode::CONFLICT);
        fs::write(path.with_file_name(&backup),new).unwrap();
        assert_eq!(restore(&root,"nested/main",&present).unwrap_err().status(),StatusCode::CONFLICT);
        fs::write(path.with_file_name(&backup),old).unwrap();
        let preserved=restore(&root,"nested/main",&present).unwrap().unwrap();
        assert!(preserved.starts_with("nested/"));
        assert_eq!(fs::read_to_string(root.join(preserved)).unwrap(),new);
        assert_eq!(fs::read_to_string(&path).unwrap(),old);
        fs::remove_file(&path).unwrap();
        assert!(restore(&root,"nested/main",&missing).unwrap().is_none());
        assert_eq!(fs::read_to_string(&path).unwrap(),old);
        for invalid in ["../main.flowgraph.toml.bak-123","other.flowgraph.toml.bak-123","main.flowgraph.toml.bak-x"] {
            assert!(plan(&root,"nested/main",invalid).is_err());
        }
        fs::write(path.with_file_name(&backup),"not valid [").unwrap();
        assert!(plan(&root,"nested/main",&backup).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(),old);
        fs::remove_dir_all(root).unwrap();
    }
}
