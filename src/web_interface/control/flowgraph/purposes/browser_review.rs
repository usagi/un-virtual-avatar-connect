//! Explicit opt-in leaf API review. No State, audio, OAuth or runtime startup.
use super::*;
use actix_web::{web, HttpResponse};
use serde_json::{json, Value};
use std::path::PathBuf;

fn command(root: &Path, action: &str, body: Value) -> anyhow::Result<Value> {
    let expected = || -> anyhow::Result<Snapshot> { Ok(serde_json::from_value(body["expected"].clone())?) };
    Ok(match action {
        "inventory" => {
            let mut files = vec![];
            for path in crate::flowgraph::loader::walk_flowgraph_dir(root)? {
                let text = std::fs::read_to_string(&path)?;
                let file = crate::flowgraph::loader::file::parse_flowgraph_file(&text,Some(&path))?;
                files.push(json!({"fq":crate::flowgraph::loader::fq_path_of_file(root,&path).map_err(anyhow::Error::msg)?,"path":path.strip_prefix(root)?.to_string_lossy().replace('\\',"/"),"title":file.meta.as_ref().and_then(|m|m.title.clone()),"node_count":file.nodes.len(),"edge_count":file.edges.len(),"parse_error":null,"source_digest":crate::flowgraph::loader::file::package_source_digest(&text)}));
            }
            json!({"root_dir":super::super::graph_root_identity(root),"exists":true,"files":files})
        },
        "read" => serde_json::to_value(read(root)?)?,
        "save" => serde_json::to_value(save(root, &expected()?, serde_json::from_value(body["document"].clone())?)?)?,
        "removal-plan" => removal_plan(root,&expected()?,body["id"].as_str().ok_or_else(||anyhow::anyhow!("missing purpose id"))?,serde_json::from_value(body["files"].clone())?)?,
        "remove-files" => serde_json::to_value(remove_purpose_files(root,&expected()?,body["id"].as_str().ok_or_else(||anyhow::anyhow!("missing purpose id"))?,serde_json::from_value(body["files"].clone())?,&body["reviewed"])?)?,
        "removal-history" => json!({"root":super::super::graph_root_identity(root),"entries":removal_recovery_history(root)?}),
        "restore-removal" => serde_json::to_value(restore_removal(root,&serde_json::from_value(body["recovery"].clone())?)?)?,
        "history" => json!({"root":super::super::graph_root_identity(root),"entries":history(root)?}),
        "recovery" => json!({"root":super::super::graph_root_identity(root),"entries":registration_recovery(root)?}),
        "retry" => serde_json::to_value(retry_registration(root,&expected()?,body["id"].as_str().ok_or_else(||anyhow::anyhow!("missing recovery id"))?,body["revision"].as_str().ok_or_else(||anyhow::anyhow!("missing recovery revision"))?)?)?,
        "preview" | "import" => {
            let archive: Vec<u8> = serde_json::from_value(body["archive"].clone())?;
            anyhow::ensure!(archive.len() <= 1_000_000,"review archive too large");
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&archive))?;
            anyhow::ensure!(zip.len() <= 2048,"too many archive entries");
            let mut expanded = 0u64;
            for index in 0..zip.len() { expanded = expanded.checked_add(zip.by_index(index)?.size()).ok_or_else(||anyhow::anyhow!("archive overflow"))?; }
            anyhow::ensure!(expanded <= 32_000_000,"expanded review archive too large");
            let mut options: crate::flowgraph::fragment::zip_codec::ZipImportOptions = serde_json::from_value(body["options"].clone())?;
            if action == "preview" {
                options.dry_run = true;
                serde_json::to_value(crate::flowgraph::fragment::zip_codec::import_zip(root,&archive,&options)?)?
            } else {
                options.dry_run = false;
                serde_json::to_value(import_registered(root,&expected()?,&archive,&options)?)?
            }
        },
        "export" => {
            let _guard = super::super::write_lock::acquire(root)?;
            let current = read_unlocked(root)?;
            anyhow::ensure!(current == expected()?, "purpose selection changed");
            let id = body["id"].as_str().ok_or_else(||anyhow::anyhow!("missing purpose id"))?;
            let purpose = current.document.purposes.iter().find(|p|p.id==id).ok_or_else(||anyhow::anyhow!("purpose is missing"))?;
            let plan = crate::flowgraph::fragment::composition_share::plan_composition_share(root,&purpose.roots)?;
            use crate::flowgraph::fragment::{CopyRequest,CopyTarget,FragmentScope};
            use crate::flowgraph::fragment::zip_codec::{export_zip_with_purpose,PortablePurpose};
            let copy = CopyRequest {scope:FragmentScope::Mixed,targets:plan.files.iter().cloned().map(|fq|CopyTarget::File{fq}).collect(),origin:None};
            let portable = PortablePurpose {schema_version:1,name:purpose.name.clone(),roots:purpose.roots.clone()};
            json!({"archive":export_zip_with_purpose(root,&copy,&[],Some(&portable))?,"files":plan.files,"purpose":portable,"plan":plan})
        },
        _ => anyhow::bail!("unsupported isolated purpose operation"),
    })
}
async fn handle(root: web::Data<PathBuf>, action: web::Path<String>, body: web::Json<Value>) -> HttpResponse {
    match command(root.get_ref(),&action,body.into_inner()) {
        Ok(value) => HttpResponse::Ok().json(value),
        Err(error) => HttpResponse::Conflict().json(json!({"error":"purpose_review_failed","detail":error.to_string()})),
    }
}

#[actix_web::test]
#[ignore = "explicit isolated purpose browser API review, no runtime or devices"]
async fn isolated_purpose_browser_api() {
    let run = std::env::var("UNVAC_PURPOSE_REVIEW_RUN").expect("explicit run name required");
    assert!(!run.is_empty() && run.len()<=64 && run.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'));
    let seconds: u64 = std::env::var("UNVAC_PURPOSE_REVIEW_SECONDS").unwrap_or_else(|_|"300".into()).parse().unwrap();
    assert!((1..=600).contains(&seconds));
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/purpose-api-review");
    std::fs::create_dir_all(&parent).unwrap(); let parent = parent.canonicalize().unwrap();
    let root = parent.join(run); let marker = root.join("isolated-review.marker");
    if root.exists() {
        assert_eq!(root.canonicalize().unwrap(),root);
        assert_eq!(std::fs::read_to_string(&marker).unwrap(),"UNVAC isolated purpose review v1");
    } else {
        std::fs::create_dir(&root).unwrap();
        std::fs::write(&marker,"UNVAC isolated purpose review v1").unwrap();
        std::fs::create_dir(root.join("graphs")).unwrap();
        std::fs::write(root.join("graphs/main.flowgraph.toml"),"[meta]\ntitle='Purpose review'\n").unwrap();
    }
    let graphs = root.join("graphs"); assert_eq!(graphs.canonicalize().unwrap(),graphs);
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST,0)).unwrap();
    let port = listener.local_addr().unwrap().port(); let server_root = graphs.clone();
    let server = actix_web::HttpServer::new(move || actix_web::App::new()
        .app_data(web::Data::new(server_root.clone()))
        .app_data(web::JsonConfig::default().limit(8_000_000))
        .route("/api/v1/control/flowgraph/purposes/{action}", web::post().to(handle)))
        .workers(1).listen(listener).unwrap().run();
    let handle = server.handle(); let task = actix_web::rt::spawn(server);
    let ready = root.join("ready.json");
    std::fs::write(&ready,serde_json::to_vec_pretty(&json!({"kind":"unvac-isolated-purpose-review","pid":std::process::id(),"port":port,"graph_root":super::super::graph_root_identity(&graphs),"lifetime_seconds":seconds})).unwrap()).unwrap();
    println!("Purpose API ready: {}",ready.display());
    tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;
    handle.stop(true).await; task.await.unwrap().unwrap(); std::fs::remove_file(ready).unwrap();
}

#[test]
fn native_commands_keep_identity_and_export_named_selection() {
    let root = std::env::temp_dir().join(format!("vac-purpose-http-{}",rand::random::<u64>()));
    std::fs::create_dir(&root).unwrap(); std::fs::write(root.join("main.flowgraph.toml"),"[meta]\ntitle='Review'\n").unwrap();
    let initial = command(&root,"read",json!({})).unwrap();
    let saved = command(&root,"save",json!({"expected":initial,"document":{"schema_version":1,"purposes":[{"id":"one","name":"配信支援","roots":["main"]}]}})).unwrap();
    let exported = command(&root,"export",json!({"expected":saved,"id":"one"})).unwrap();
    let preview = command(&root,"preview",json!({"archive":exported["archive"],"options":{"target_prefix":"received"}})).unwrap();
    assert_eq!(preview["purpose"]["name"],"配信支援"); assert!(!root.join("received").exists());
    let imported = command(&root,"import",json!({"expected":saved,"archive":exported["archive"],"options":{"target_prefix":"received"}})).unwrap();
    assert_eq!(imported["registration"]["roots"][0],"received/main");
    assert!(command(&root,"export",json!({"expected":saved,"id":"one"})).is_err());
    assert!(command(&root,"unknown",json!({})).is_err());
    std::fs::write(root.join("main.flowgraph.toml"),"[[instances]]\nid='absent'\ngraph='missing'\n").unwrap();
    let current = command(&root,"read",json!({})).unwrap();
    let incomplete = command(&root,"export",json!({"expected":current,"id":"one"})).unwrap();
    assert!(incomplete["plan"]["references"].as_array().unwrap().iter().any(|reference|reference["error"].is_string()));
    assert_eq!(incomplete["plan"]["portable_feature_verified"],false);
    assert_eq!(incomplete["plan"]["files"],incomplete["files"]);
    std::fs::remove_dir_all(root).unwrap();
}
