//! Reviewed connection operations under the existing Control API authorization.
use actix_web::{post, web, HttpResponse, http::StatusCode};
use serde::Deserialize;
use crate::flowgraph::speech_route_sources::DictionaryFailurePolicy;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    expected_source:String,
    revision:String,
    dictionary_key:String,
    destination:String,
    failure_policy:DictionaryFailurePolicy,
}

fn failure(status:StatusCode,reason:impl ToString)->HttpResponse {
    HttpResponse::build(status).json(serde_json::json!({"error":"dictionary_connection_preview_failed","reason":reason.to_string()}))
}

#[post("/dictionary/connection/preview")]
async fn preview(state:web::Data<crate::SharedState>,body:web::Json<Input>)->HttpResponse {
    let Some(path)=state.read().await.conf_source_path.clone() else {
        return failure(StatusCode::CONFLICT,"操作対象の保存先を特定できません。");
    };
    let input=body.into_inner();
    if input.expected_source!=path.to_string_lossy() {
        return failure(StatusCode::CONFLICT,"操作対象が変わりました。入力を保持して確認し直してください。");
    }
    let requested_path=path.clone();
    let result=web::block(move ||crate::profile_store::dictionary_connection::prepare(
        &path,&input.revision,&input.dictionary_key,&input.destination,input.failure_policy)).await;
    if state.read().await.conf_source_path.as_ref()!=Some(&requested_path) {
        return failure(StatusCode::CONFLICT,"確認中に操作対象が変わりました。");
    }
    match result {
        Ok(Ok(plan))=>HttpResponse::Ok().json(serde_json::json!({
            "context":plan.reviewed.context(),"review_revision":plan.reviewed.revision(),
            "changes":plan.reviewed.changes(),"affected_destinations":plan.affected_destinations,
            "dictionary_revision":plan.dictionary_revision,
            "basis":"saved_configuration","graph_validated":true,"saved":false,"runtime_applied":false,
        })),
        Ok(Err(error))=>failure(StatusCode::UNPROCESSABLE_ENTITY,error),
        Err(_)=>failure(StatusCode::INTERNAL_SERVER_ERROR,"変更案の確認処理を完了できませんでした。"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveInput {
    expected_source: String, revision: String, dictionary_key: String,
    destination: String, failure_policy: DictionaryFailurePolicy,
    review_revision: String, dictionary_revision: String,
    confirm_shared_definition: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryInput { expected_source: String, revision: String }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreInput { expected_source: String, revision: String, transaction_id: String }

async fn target(state: &crate::SharedState, expected: &str) -> Result<std::path::PathBuf, HttpResponse> {
    let path=state.read().await.conf_source_path.clone();
    path.filter(|p|p.to_string_lossy()==expected)
        .ok_or_else(||failure(StatusCode::CONFLICT,"操作対象が変わりました。入力を保持して確認し直してください。"))
}

fn operation_result<T:serde::Serialize>(result:Result<anyhow::Result<T>,actix_web::error::BlockingError>,source:&str)->HttpResponse {
    match result {
        Ok(Ok(receipt))=>HttpResponse::Ok().json(receipt),
        Ok(Err(error))=>HttpResponse::UnprocessableEntity().json(serde_json::json!({
            "error":"dictionary_connection_operation_failed","reason":format!("{error:#}"),
            "profile_source":source,"check_recovery":true,"runtime_applied":false,
        })),
        Err(_)=>HttpResponse::InternalServerError().json(serde_json::json!({
            "error":"dictionary_connection_outcome_unknown",
            "reason":"操作結果を取得できませんでした。再実行せず保存状態を確認してください。",
            "profile_source":source,"check_recovery":true,"runtime_applied":false,
        })),
    }
}

#[post("/dictionary/connection/save")]
async fn save(state:web::Data<crate::SharedState>,body:web::Json<SaveInput>)->HttpResponse {
    let input=body.into_inner();
    let path=match target(state.get_ref(),&input.expected_source).await {Ok(p)=>p,Err(r)=>return r};
    let source=path.to_string_lossy().into_owned();
    let result=web::block(move ||crate::profile_store::dictionary_connection::save(
        &path,&input.revision,&input.dictionary_key,&input.destination,input.failure_policy,
        &input.review_revision,&input.dictionary_revision,input.confirm_shared_definition)).await;
    // A later selection change cannot turn a completed write into a failure.
    // The receipt always belongs to the originally captured server profile.
    operation_result(result,&source)
}

#[post("/dictionary/connection/recovery")]
async fn recovery(state:web::Data<crate::SharedState>,body:web::Json<RecoveryInput>)->HttpResponse {
    let input=body.into_inner();
    let path=match target(state.get_ref(),&input.expected_source).await {Ok(p)=>p,Err(r)=>return r};
    let source=path.to_string_lossy().into_owned();
    operation_result(web::block(move ||crate::profile_store::dictionary_connection::recovery(
        &path,&input.revision,None)).await,&source)
}

#[post("/dictionary/connection/restore")]
async fn restore(state:web::Data<crate::SharedState>,body:web::Json<RestoreInput>)->HttpResponse {
    let input=body.into_inner();
    let path=match target(state.get_ref(),&input.expected_source).await {Ok(p)=>p,Err(r)=>return r};
    let source=path.to_string_lossy().into_owned();
    operation_result(web::block(move ||crate::profile_store::dictionary_connection::recovery(
        &path,&input.revision,Some(&input.transaction_id))).await,&source)
}

pub fn configure(cfg:&mut web::ServiceConfig) {cfg.service(preview).service(save).service(recovery).service(restore);}

#[cfg(test)]
mod tests {
    use super::*;
    #[actix_web::test]
    async fn dictionary_connection_http_returns_validated_plan_without_writes() {
        use actix_web::{test as http,App};
        let root=std::env::temp_dir().join(format!("vac-dictionary-http-plan-{}",rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();std::fs::create_dir(root.join("graphs")).unwrap();
        let main=include_str!("../../../flowgraph.example/chat-echo/main.flowgraph.toml");
        let tts=include_str!("../../../flowgraph.example/chat-echo/tts.flowgraph.toml");
        std::fs::write(root.join("graphs/main.flowgraph.toml"),main).unwrap();
        std::fs::write(root.join("graphs/tts.flowgraph.toml"),tts).unwrap();
        let path=root.join("conf.toml");let dictionary=root.join("reading.tsv");
        std::fs::write(&dictionary,"reading\tsurface\nゆーえぬ\tUN\n").unwrap();
        let source=format!("flowgraph_dir='graphs'\n[[control_api.tables]]\nkey='reading'\npath={}\n",toml::Value::String(dictionary.to_string_lossy().into_owned()));
        std::fs::write(&path,&source).unwrap();
        let mut conf:crate::Conf=toml::from_str("").unwrap();conf.source_path=Some(path.clone());conf.runtime_dir=Some(root.join("runtime"));
        let audio=std::sync::Arc::new(tokio::sync::Mutex::new(crate::AudioSink::disconnected_for_test()));
        let state=crate::State::new(&conf,audio,crate::shutdown::ShutdownBroker::new()).await.unwrap();
        let app=http::init_service(App::new().app_data(web::Data::new(state.clone())).configure(configure)).await;
        let mut body=serde_json::json!({"expected_source":path,"revision":blake3::hash(source.as_bytes()).to_hex().to_string(),"dictionary_key":"reading","destination":"tts::speaker","failure_policy":"skip_speech"});
        let request=|body:&serde_json::Value|http::TestRequest::post().uri("/dictionary/connection/preview").set_json(body).to_request();
        let response=http::call_service(&app,request(&body)).await;assert_eq!(response.status(),StatusCode::OK);
        let plan:serde_json::Value=http::read_body_json(response).await;
        assert_eq!(plan["basis"],"saved_configuration");assert_eq!(plan["graph_validated"],true);
        assert_eq!(plan["saved"],false);assert_eq!(plan["runtime_applied"],false);
        assert_eq!(plan["changes"].as_object().unwrap().len(),2);
        assert_eq!(plan["affected_destinations"],serde_json::json!(["tts::speaker"]));
        body["expected_source"]=serde_json::json!(root.join("other.toml"));
        assert_eq!(http::call_service(&app,request(&body)).await.status(),StatusCode::CONFLICT);
        assert_eq!(std::fs::read_to_string(&path).unwrap(),source);
        assert_eq!(std::fs::read_to_string(root.join("graphs/main.flowgraph.toml")).unwrap(),main);
        assert_eq!(std::fs::read_to_string(root.join("graphs/tts.flowgraph.toml")).unwrap(),tts);
        body["expected_source"]=serde_json::json!(path);
        let revision=body["revision"].as_str().unwrap();
        let prepared=crate::profile_store::dictionary_connection::prepare(&path,revision,"reading","tts::speaker",DictionaryFailurePolicy::SkipSpeech).unwrap();
        let graphs=root.join("graphs");
        let id={
            let _guard=crate::flowgraph::write_lock::acquire(&graphs).unwrap();
            let id=crate::flowgraph::source_transaction::prepare(&graphs,prepared.reviewed.changes()).unwrap();
            let (fq,change)=prepared.reviewed.changes().iter().next().unwrap();
            std::fs::write(graphs.join(format!("{fq}.flowgraph.toml")),&change.proposed).unwrap();
            id
        };
        let status_body=serde_json::json!({"expected_source":path,"revision":revision});
        let response=http::call_service(&app,http::TestRequest::post().uri("/dictionary/connection/recovery").set_json(&status_body).to_request()).await;
        assert_eq!(response.status(),StatusCode::OK);
        let status:serde_json::Value=http::read_body_json(response).await;
        assert_eq!(status["pending"]["transaction_id"],id);assert_eq!(status["pending"]["restorable"],true);
        let restore_body=serde_json::json!({"expected_source":path,"revision":revision,"transaction_id":id});
        let response=http::call_service(&app,http::TestRequest::post().uri("/dictionary/connection/restore").set_json(&restore_body).to_request()).await;
        assert_eq!(response.status(),StatusCode::OK);
        let receipt:serde_json::Value=http::read_body_json(response).await;
        assert_eq!(receipt["restored"],true);assert!(receipt["pending"].is_null());
        assert_eq!(receipt["runtime_applied"],false);
        let mut save_body=body.clone();
        save_body["review_revision"]=plan["review_revision"].clone();
        save_body["dictionary_revision"]=plan["dictionary_revision"].clone();
        save_body["confirm_shared_definition"]=serde_json::json!(false);
        let save_request=|value:&serde_json::Value|http::TestRequest::post().uri("/dictionary/connection/save").set_json(value).to_request();
        save_body["expected_source"]=serde_json::json!(root.join("other.toml"));
        assert_eq!(http::call_service(&app,save_request(&save_body)).await.status(),StatusCode::CONFLICT);
        save_body["expected_source"]=serde_json::json!(path);
        let response=http::call_service(&app,save_request(&save_body)).await;
        assert_eq!(response.status(),StatusCode::OK);
        let receipt:serde_json::Value=http::read_body_json(response).await;
        assert_eq!(receipt["saved"],true);assert_eq!(receipt["runtime_applied"],false);
        assert_eq!(receipt["profile_source"],serde_json::json!(path));
        for (fq,change) in prepared.reviewed.changes() {
            assert_eq!(std::fs::read_to_string(graphs.join(format!("{fq}.flowgraph.toml"))).unwrap(),change.proposed);
        }
        assert_eq!(http::call_service(&app,save_request(&save_body)).await.status(),StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(std::fs::read_to_string(&path).unwrap(),source);
        drop(app);drop(state);
        let resolved=root.canonicalize().unwrap();
        assert!(resolved.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert!(resolved.file_name().unwrap().to_string_lossy().starts_with("vac-dictionary-http-plan-"));
        std::fs::remove_dir_all(resolved).unwrap();
    }
}
