//! String-only adapter for the real Twitch identity editor. No VAC startup.
use std::io::{Read, Write};
use serde::Deserialize;
use virtual_avatar_connect::web_interface::control::twitch_setup::{isolated_preview, Input};
#[derive(Deserialize)]
struct Request { content: String, runtime_content: String, input: Option<Input> }
fn main() -> anyhow::Result<()> {
    let mut bytes=Vec::new();std::io::stdin().take(4_000_001).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len()<=4_000_000,"request too large");
    let request:Request=serde_json::from_slice(&bytes)?;
    let reply=match isolated_preview(&request.content,&request.runtime_content,request.input) {
        Ok(result)=>serde_json::json!({"ok":true,"result":result}),
        Err(error)=>serde_json::json!({"ok":false,"error":error}),
    };
    std::io::stdout().write_all(&serde_json::to_vec(&reply)?)?;Ok(())
}
