//! Reviewed dictionary registration. Runtime wiring remains a separate action.
use std::path::{Path, PathBuf};
use actix_web::{http::StatusCode, get, post, web, HttpResponse};
use serde::{Deserialize, Serialize};
use crate::profile_store::dictionary_setup::{self, DictionarySource, RegistrationInput};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub expected_source: String,
    pub revision: String,
    pub key: String,
    pub label: String,
    /// None plans a new profile-local dictionary. Some only inspects a file.
    pub existing_path: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct Preview {
    source_path: PathBuf,
    revision: String,
    dictionary_path: PathBuf,
    existing_revision: Option<String>,
    existing_rows: Option<usize>,
    creates_file: bool,
    saved: bool,
    runtime_connected: bool,
    review_revision: String,
    storage_scope: &'static str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveInput {
    pub registration: Input,
    pub review_revision: String,
    #[serde(default)]
    pub external_file_confirmed: bool,
}

#[derive(Debug)]
struct Failure(StatusCode, String);
impl Failure {
    fn response(self) -> HttpResponse {
        HttpResponse::build(self.0).json(serde_json::json!({"error":"dictionary_setup_failed","reason":self.1}))
    }
}

fn preview(path: &Path, input: Input) -> Result<Preview, Failure> {
    if input.expected_source != path.to_string_lossy() {
        return Err(Failure(StatusCode::CONFLICT, "操作対象が変わりました。入力を保持して対象を確認してください。".into()));
    }
    let _guard = crate::profile_store::lifecycle::lock_config_write(path)
        .map_err(|_| Failure(StatusCode::CONFLICT, "この設定は別の操作中です。".into()))?;
    let original = std::fs::read_to_string(path)
        .map_err(|_| Failure(StatusCode::INTERNAL_SERVER_ERROR, "設定ファイルを読み取れません。".into()))?;
    if input.revision != blake3::hash(original.as_bytes()).to_hex().as_str() {
        return Err(Failure(StatusCode::CONFLICT, "設定が変更されています。入力を保持して読み直してください。".into()));
    }
    let mut request = RegistrationInput {
        expected_revision: input.revision,
        key: input.key,
        label: input.label,
        source: input.existing_path.map(DictionarySource::Existing).unwrap_or(DictionarySource::New),
    };
    // Validate the requested identity before inspecting any selected file.
    let mut plan = dictionary_setup::prepare_registration(path, &original, &request)
        .map_err(|e| Failure(StatusCode::BAD_REQUEST, e.to_string()))?;
    let (existing_revision, existing_rows) = if plan.new_file_contents.is_none() {
        let (canonical, revision, rows) = dictionary_setup::inspect_existing(&plan.table_path)
            .map_err(|e| Failure(StatusCode::BAD_REQUEST, e.to_string()))?;
        request.source = DictionarySource::Existing(canonical);
        plan = dictionary_setup::prepare_registration(path, &original, &request)
            .map_err(|e| Failure(StatusCode::BAD_REQUEST, e.to_string()))?;
        (Some(revision), Some(rows))
    } else {
        match std::fs::symlink_metadata(&plan.table_path) {
            Ok(_) => return Err(Failure(StatusCode::CONFLICT, "作成先には既にファイルがあります。別のIDを指定するか、既存辞書として登録してください。".into())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => return Err(Failure(StatusCode::BAD_REQUEST, "作成先を確認できません。アクセス権と保存先を確認してください。".into())),
        }
        (None, None)
    };
    let profile_directory = path.parent().and_then(|p| p.canonicalize().ok())
        .ok_or_else(|| Failure(StatusCode::BAD_REQUEST,"設定の保存先を確認できません。".into()))?;
    let storage_scope = if plan.new_file_contents.is_some() || plan.table_path.starts_with(&profile_directory) { "profile_directory" } else { "external_file" };
    let review_bytes = serde_json::to_vec(&(&plan.conf_path,&plan.source_revision,&plan.table_path,&existing_revision,request.key.trim(),request.label.trim(),storage_scope))
        .map_err(|_| Failure(StatusCode::INTERNAL_SERVER_ERROR,"確認結果を作成できません。".into()))?;
    Ok(Preview {
        source_path: plan.conf_path, revision: plan.source_revision,
        dictionary_path: plan.table_path, existing_revision, existing_rows,
        creates_file: plan.new_file_contents.is_some(), saved: false, runtime_connected: false,
        review_revision: blake3::hash(&review_bytes).to_hex().to_string(), storage_scope,
    })
}

fn save(path: &Path, input: SaveInput) -> Result<serde_json::Value, Failure> {
    let reviewed = preview(path, input.registration.clone())?;
    if reviewed.review_revision != input.review_revision {
        return Err(Failure(StatusCode::CONFLICT,"確認後に入力または辞書が変わりました。内容を確認し直してください。".into()));
    }
    if reviewed.storage_scope == "external_file" && !input.external_file_confirmed {
        return Err(Failure(StatusCode::BAD_REQUEST,"外部辞書は別の構成でも使われている可能性があります。同じファイルを編集することを確認してください。".into()));
    }
    let request = RegistrationInput {
        expected_revision: reviewed.revision,
        key: input.registration.key,
        label: input.registration.label,
        source: if reviewed.creates_file { DictionarySource::New } else { DictionarySource::Existing(reviewed.dictionary_path) },
    };
    // Publication rechecks the source and dictionary revisions under its lock;
    // the preview lock is deliberately not held recursively here.
    let plan = dictionary_setup::save_registration(path,&request,reviewed.existing_revision.as_deref())
        .map_err(|error| Failure(if error.downcast_ref::<dictionary_setup::IncompleteRegistration>().is_some() {StatusCode::INTERNAL_SERVER_ERROR} else {StatusCode::CONFLICT},error.to_string()))?;
    Ok(serde_json::json!({
        "source_path":plan.conf_path,"dictionary_path":plan.table_path,
        "revision":blake3::hash(plan.updated_conf.as_bytes()).to_hex().to_string(),
        "saved":true,"restart_required":true,"runtime_connected":false,
        "storage_scope":reviewed.storage_scope,
    }))
}

#[get("/dictionary/setup")]
async fn get_setup(state: web::Data<crate::SharedState>) -> HttpResponse {
    let Some(path) = state.read().await.conf_source_path.clone() else {
        return Failure(StatusCode::CONFLICT,"操作対象の保存先を特定できません。".into()).response();
    };
    match web::block(move || {
        let _guard = crate::profile_store::lifecycle::lock_config_write(&path)
            .map_err(|_| Failure(StatusCode::CONFLICT,"この設定は別の操作中です。".into()))?;
        let content = std::fs::read_to_string(&path)
            .map_err(|_| Failure(StatusCode::INTERNAL_SERVER_ERROR,"設定ファイルを読み取れません。".into()))?;
        // Return identity only, never configuration contents or credentials.
        Ok::<_, Failure>(serde_json::json!({"source_path":path,"revision":blake3::hash(content.as_bytes()).to_hex().to_string()}))
    }).await {
        Ok(Ok(view)) => HttpResponse::Ok().json(view),
        Ok(Err(error)) => error.response(),
        Err(_) => Failure(StatusCode::INTERNAL_SERVER_ERROR,"操作対象の確認に失敗しました。".into()).response(),
    }
}

#[post("/dictionary/setup/preview")]
async fn post_preview(state: web::Data<crate::SharedState>, input: web::Json<Input>) -> HttpResponse {
    let Some(path) = state.read().await.conf_source_path.clone() else {
        return Failure(StatusCode::CONFLICT, "操作対象の保存先を特定できません。".into()).response();
    };
    match web::block(move || preview(&path, input.into_inner())).await {
        Ok(Ok(view)) => HttpResponse::Ok().json(view),
        Ok(Err(error)) => error.response(),
        Err(_) => Failure(StatusCode::INTERNAL_SERVER_ERROR, "辞書の確認処理に失敗しました。".into()).response(),
    }
}

#[post("/dictionary/setup/register")]
async fn post_register(state: web::Data<crate::SharedState>, input: web::Json<SaveInput>) -> HttpResponse {
    let Some(path) = state.read().await.conf_source_path.clone() else {
        return Failure(StatusCode::CONFLICT,"操作対象の保存先を特定できません。".into()).response();
    };
    match web::block(move || save(&path,input.into_inner())).await {
        Ok(Ok(view)) => HttpResponse::Ok().json(view),
        Ok(Err(error)) => error.response(),
        Err(_) => Failure(StatusCode::INTERNAL_SERVER_ERROR,"登録結果を確認できません。設定と辞書を確認してから再操作してください。".into()).response(),
    }
}

pub fn configure(cfg: &mut web::ServiceConfig) { cfg.service(get_setup).service(post_preview).service(post_register); }

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let nonce=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let path=std::env::temp_dir().join(format!("vac-dictionary-preview-{}-{nonce}",std::process::id()));
            std::fs::create_dir(&path).unwrap();
            std::fs::write(path.join("conf.toml"), "# unchanged\n").unwrap();
            Self(path)
        }
        fn input(&self) -> Input {
            Input { expected_source:self.0.join("conf.toml").to_string_lossy().into_owned(),revision:blake3::hash(b"# unchanged\n").to_hex().to_string(),key:"stream".into(),label:"Stream".into(),existing_path:None }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if let (Ok(path),Ok(temp))=(self.0.canonicalize(),std::env::temp_dir().canonicalize()) {
                if path.starts_with(temp) && path.file_name().unwrap().to_string_lossy().starts_with("vac-dictionary-preview-") {let _=std::fs::remove_dir_all(path);}
            }
        }
    }
    #[test]
    fn dictionary_setup_preview_preserves_files_and_rejects_collision_and_stale_revision() {
        let fixture=Fixture::new();let conf=fixture.0.join("conf.toml");
        let proposed=preview(&conf,fixture.input()).unwrap();
        assert!(proposed.creates_file);assert!(!proposed.saved);assert!(!proposed.runtime_connected);
        assert!(!proposed.dictionary_path.exists());assert!(!fixture.0.join("dictionaries").exists());
        let dictionary=fixture.0.join("existing.tsv");let content="うさぎ\tUSAGI\n";
        std::fs::write(&dictionary,content).unwrap();
        let mut input=fixture.input();input.existing_path=Some(dictionary.clone());
        let existing=preview(&conf,input).unwrap();
        assert!(!existing.creates_file);assert_eq!(existing.existing_rows,Some(1));
        assert_eq!(existing.dictionary_path,dictionary.canonicalize().unwrap());
        assert_eq!(std::fs::read_to_string(dictionary).unwrap(),content);
        std::fs::create_dir(fixture.0.join("dictionaries")).unwrap();
        std::fs::write(&proposed.dictionary_path,"keep").unwrap();
        assert_eq!(preview(&conf,fixture.input()).unwrap_err().0,StatusCode::CONFLICT);
        assert_eq!(std::fs::read_to_string(&proposed.dictionary_path).unwrap(),"keep");
        let mut stale=fixture.input();stale.revision="stale".into();
        assert_eq!(preview(&conf,stale).unwrap_err().0,StatusCode::CONFLICT);
        let held=crate::profile_store::lifecycle::lock_config_write(&conf).unwrap();
        assert_eq!(preview(&conf,fixture.input()).unwrap_err().0,StatusCode::CONFLICT);drop(held);
        assert_eq!(std::fs::read_to_string(conf).unwrap(),"# unchanged\n");
    }
    #[test]
    fn dictionary_setup_preview_rejects_wrong_target_before_reading() {
        let path = std::env::temp_dir().join("vac-preview-does-not-exist/conf.toml");
        let input = Input { expected_source: "different-profile".into(), revision: String::new(), key: "stream".into(), label: "Stream".into(), existing_path: None };
        assert_eq!(preview(&path, input).unwrap_err().0, StatusCode::CONFLICT);
    }
    #[test]
    fn dictionary_setup_preview_requires_explicit_target_and_revision() {
        assert!(serde_json::from_value::<Input>(serde_json::json!({"key":"stream","label":"Stream"})).is_err());
        assert!(serde_json::from_value::<Input>(serde_json::json!({"expected_source":"a","revision":"b","key":"stream","label":"Stream","save":true})).is_err());
    }

    #[test]
    fn dictionary_setup_registration_requires_review_and_preserves_external_changes() {
        let fixture=Fixture::new();
        let directory=fixture.0.join("profile");std::fs::create_dir(&directory).unwrap();
        let conf=directory.join("conf.toml");std::fs::write(&conf,"# unchanged\n").unwrap();
        let dictionary=fixture.0.join("shared.tsv");std::fs::write(&dictionary,"うさぎ\tUSAGI\n").unwrap();
        let mut input=fixture.input();input.expected_source=conf.to_string_lossy().into_owned();input.existing_path=Some(dictionary.clone());
        let reviewed=preview(&conf,input.clone()).unwrap();assert_eq!(reviewed.storage_scope,"external_file");
        let request=|input:Input,token:String,confirmed:bool| SaveInput {registration:input,review_revision:token,external_file_confirmed:confirmed};
        assert_eq!(save(&conf,request(input.clone(),reviewed.review_revision.clone(),false)).unwrap_err().0,StatusCode::BAD_REQUEST);
        let edited="うさぎ\tUSAGI\nねこ\tCAT\n";std::fs::write(&dictionary,edited).unwrap();
        assert_eq!(save(&conf,request(input.clone(),reviewed.review_revision,true)).unwrap_err().0,StatusCode::CONFLICT);
        let fresh=preview(&conf,input.clone()).unwrap();
        let mut changed=input.clone();changed.label="Changed after review".into();
        assert_eq!(save(&conf,request(changed,fresh.review_revision.clone(),true)).unwrap_err().0,StatusCode::CONFLICT);
        assert_eq!(std::fs::read_to_string(&conf).unwrap(),"# unchanged\n");
        let saved=save(&conf,request(input,fresh.review_revision,true)).unwrap();
        assert_eq!(saved["saved"],true);assert_eq!(saved["restart_required"],true);assert_eq!(saved["runtime_connected"],false);
        assert_eq!(std::fs::read_to_string(dictionary).unwrap(),edited);
        let parsed:crate::Conf=toml::from_str(&std::fs::read_to_string(conf).unwrap()).unwrap();assert_eq!(parsed.control_api.unwrap().tables.len(),1);
    }

    #[actix_web::test]
    async fn dictionary_setup_http_preview_preserves_files_and_binds_current_profile() {
        use actix_web::{test as http, App};
        use std::sync::Arc;
        let fixture=Fixture::new();
        let conf_path=fixture.0.join("conf.toml");
        let mut conf:crate::Conf=toml::from_str("").unwrap();
        conf.source_path=Some(conf_path.clone());
        conf.runtime_dir=Some(fixture.0.join("runtime"));
        let audio=Arc::new(tokio::sync::Mutex::new(crate::AudioSink::disconnected_for_test()));
        let state=crate::State::new(&conf,audio,crate::shutdown::ShutdownBroker::new()).await.unwrap();
        let app=http::init_service(App::new().app_data(web::Data::new(state.clone())).configure(configure)).await;
        let response=http::call_service(&app,http::TestRequest::get().uri("/dictionary/setup").to_request()).await;
        assert_eq!(response.status(),StatusCode::OK);
        let identity:serde_json::Value=http::read_body_json(response).await;
        assert_eq!(identity.as_object().unwrap().len(),2);
        assert_eq!(identity["source_path"],serde_json::json!(conf_path));
        assert_eq!(identity["revision"],fixture.input().revision);
        let body=serde_json::json!({"expected_source":conf_path,"revision":fixture.input().revision,"key":"stream","label":"Stream"});
        let response=http::call_service(&app,http::TestRequest::post().uri("/dictionary/setup/preview").set_json(&body).to_request()).await;
        assert_eq!(response.status(),StatusCode::OK);
        let view:serde_json::Value=http::read_body_json(response).await;
        assert_eq!(view["saved"],false);assert_eq!(view["runtime_connected"],false);assert_eq!(view["creates_file"],true);
        assert!(!fixture.0.join("dictionaries").exists());
        let dictionary=fixture.0.join("existing.tsv");let contents="うさぎ\tUSAGI\n";
        std::fs::write(&dictionary,contents).unwrap();
        let mut existing=body.clone();existing["existing_path"]=serde_json::json!(dictionary);
        let response=http::call_service(&app,http::TestRequest::post().uri("/dictionary/setup/preview").set_json(&existing).to_request()).await;
        assert_eq!(response.status(),StatusCode::OK);
        let view:serde_json::Value=http::read_body_json(response).await;
        assert_eq!(view["existing_rows"],1);assert_eq!(view["creates_file"],false);
        assert_eq!(view["existing_revision"],blake3::hash(contents.as_bytes()).to_hex().to_string());
        assert!(view.get("content").is_none());
        for (field,value) in [("expected_source","another-profile"),("revision","stale")] {
            let mut invalid=body.clone();invalid[field]=serde_json::json!(value);
            let response=http::call_service(&app,http::TestRequest::post().uri("/dictionary/setup/preview").set_json(&invalid).to_request()).await;
            assert_eq!(response.status(),StatusCode::CONFLICT);
        }
        let mut unknown=body.clone();unknown["save"]=serde_json::json!(true);
        let response=http::call_service(&app,http::TestRequest::post().uri("/dictionary/setup/preview").set_json(&unknown).to_request()).await;
        assert_eq!(response.status(),StatusCode::BAD_REQUEST);
        // Identical bytes in another active profile must not accept the old draft.
        let other=fixture.0.join("other.toml");std::fs::write(&other,"# unchanged\n").unwrap();
        state.write().await.conf_source_path=Some(other.clone());
        let response=http::call_service(&app,http::TestRequest::get().uri("/dictionary/setup").to_request()).await;
        assert_eq!(response.status(),StatusCode::OK);
        let identity:serde_json::Value=http::read_body_json(response).await;
        assert_eq!(identity["source_path"],serde_json::json!(other));
        let response=http::call_service(&app,http::TestRequest::post().uri("/dictionary/setup/preview").set_json(&body).to_request()).await;
        assert_eq!(response.status(),StatusCode::CONFLICT);
        assert_eq!(std::fs::read_to_string(conf_path).unwrap(),"# unchanged\n");
        assert_eq!(std::fs::read_to_string(other).unwrap(),"# unchanged\n");
        assert_eq!(std::fs::read_to_string(dictionary).unwrap(),contents);
        state.write().await.conf_source_path=Some(fixture.0.join("conf.toml"));
        let response=http::call_service(&app,http::TestRequest::post().uri("/dictionary/setup/preview").set_json(&body).to_request()).await;
        assert_eq!(response.status(),StatusCode::OK);
        let reviewed:serde_json::Value=http::read_body_json(response).await;
        let registration=serde_json::json!({"registration":body,"review_revision":reviewed["review_revision"]});
        let response=http::call_service(&app,http::TestRequest::post().uri("/dictionary/setup/register").set_json(&registration).to_request()).await;
        assert_eq!(response.status(),StatusCode::OK);
        let result:serde_json::Value=http::read_body_json(response).await;
        assert_eq!(result["saved"],true);assert_eq!(result["restart_required"],true);assert_eq!(result["runtime_connected"],false);
        let saved=std::fs::read_to_string(fixture.0.join("conf.toml")).unwrap();
        let parsed:crate::Conf=toml::from_str(&saved).unwrap();assert_eq!(parsed.control_api.unwrap().tables.len(),1);
        assert_eq!(std::fs::read_to_string(fixture.0.join("dictionaries/stream.tsv")).unwrap(),dictionary_setup::EMPTY_GLOSSARY);
        let response=http::call_service(&app,http::TestRequest::post().uri("/dictionary/setup/register").set_json(&registration).to_request()).await;
        assert_eq!(response.status(),StatusCode::CONFLICT);
        assert_eq!(std::fs::read_to_string(fixture.0.join("conf.toml")).unwrap(),saved);
        assert_eq!(std::fs::read_to_string(fixture.0.join("other.toml")).unwrap(),"# unchanged\n");

        // Registration does not rewrite the running catalog. Constructing the
        // catalog again from saved configuration is the startup/apply boundary.
        use crate::web_interface::control::{auth::ControlApiRuntime, table};
        let old_runtime=ControlApiRuntime::init(&conf,&state).await.unwrap();
        let old_app=http::init_service(App::new().app_data(web::Data::new(old_runtime)).configure(table::configure)).await;
        let response=http::call_service(&old_app,http::TestRequest::get().uri("/tables").to_request()).await;
        assert_eq!(response.status(),StatusCode::OK);
        let old_catalog:serde_json::Value=http::read_body_json(response).await;assert_eq!(old_catalog["count"],0);
        let saved_conf:crate::Conf=toml::from_str(&saved).unwrap();
        let restored=ControlApiRuntime::init(&saved_conf,&state).await.unwrap();
        let table_app=http::init_service(App::new().app_data(web::Data::new(restored)).configure(table::configure)).await;
        let response=http::call_service(&table_app,http::TestRequest::get().uri("/tables").to_request()).await;
        assert_eq!(response.status(),StatusCode::OK);
        let catalog:serde_json::Value=http::read_body_json(response).await;
        assert_eq!(catalog["count"],1);assert_eq!(catalog["tables"][0]["key"],"stream");assert_eq!(catalog["tables"][0]["exists"],true);
        let response=http::call_service(&table_app,http::TestRequest::get().uri("/table/stream").to_request()).await;
        assert_eq!(response.status(),StatusCode::OK);
        let empty:serde_json::Value=http::read_body_json(response).await;
        assert_eq!(empty["rows"].as_array().unwrap().len(),0);
        let revision=format!("b3:{}",empty["content_hash"].as_str().unwrap());
        let entry=serde_json::json!({"values":{"reading":"うさぎ","surface":"USAGI","part_of_speech":"名詞"}});
        let response=http::call_service(&table_app,http::TestRequest::post().uri("/table/stream/entry").insert_header(("If-Match",revision.clone())).set_json(&entry).to_request()).await;
        assert!(response.status().is_success());
        let response=http::call_service(&table_app,http::TestRequest::post().uri("/table/stream/entry").insert_header(("If-Match",revision)).set_json(&entry).to_request()).await;
        assert_eq!(response.status(),StatusCode::CONFLICT);
        let response=http::call_service(&table_app,http::TestRequest::get().uri("/table/stream").to_request()).await;
        assert_eq!(response.status(),StatusCode::OK);
        let rows:serde_json::Value=http::read_body_json(response).await;
        assert_eq!(rows["rows"].as_array().unwrap().len(),1);
        assert_eq!(rows["rows"][0]["values"]["surface"],"USAGI");
        let saved_dictionary=std::fs::read_to_string(fixture.0.join("dictionaries/stream.tsv")).unwrap();
        assert!(saved_dictionary.contains("うさぎ\tUSAGI"));
        // Verify the saved editor format is usable by the existing processing
        // node, not merely visible in a table. This does not wire a speech graph.
        use crate::flowgraph::node::{StatefulNode,StatefulCtx,InputMap,ExecFireSet};
        use crate::flowgraph::socket::SocketValue;
        let dictionary=crate::flowgraph::nodes::table_ops::parse_tsv_with_mode(&saved_dictionary,"auto").unwrap();
        let replace=crate::flowgraph::nodes::dictionary::DictionaryReplaceNode;
        let mut node_state=replace.init_state();let mut inputs=InputMap::new();
        inputs.insert("content".into(),SocketValue::String("Hello USAGI".into()));
        inputs.insert("dictionary".into(),SocketValue::Table(dictionary));
        let output=replace.compute(node_state.as_mut(),&InputMap::new(),&inputs,&ExecFireSet::new(),&StatefulCtx{node_id:"isolated-dictionary-review",trigger:None}).await.unwrap();
        let Some(SocketValue::String(replaced))=output.data.get("result") else {panic!("dictionary replacement result missing")};
        assert_eq!(replaced,"Hello うさぎ");
        assert_eq!(std::fs::read_to_string(fixture.0.join("other.toml")).unwrap(),"# unchanged\n");
    }
}
