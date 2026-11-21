//! Export the production node-catalog response without starting VAC or a listener.
//! Usage: cargo run --example export_ui_catalog -- <output.json>
#[actix_web::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os().nth(1).ok_or("expected output path")?;
    let app = actix_web::test::init_service(
        actix_web::App::new().service(
            virtual_avatar_connect::web_interface::control::flowgraph::get_node_catalog,
        ),
    )
    .await;
    let request = actix_web::test::TestRequest::get()
        .uri("/flowgraph/node-catalog")
        .to_request();
    let response = actix_web::test::call_service(&app, request).await;
    if !response.status().is_success() {
        return Err(format!("catalog response: {}", response.status()).into());
    }
    let body = actix_web::test::read_body(response).await;
    let catalog: serde_json::Value = serde_json::from_slice(&body)?;
    std::fs::write(output, serde_json::to_vec_pretty(&catalog)?)?;
    println!("Exported {} production node specifications; no VAC startup or network listener.", catalog["count"]);
    Ok(())
}
