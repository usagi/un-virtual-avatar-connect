//! Debug desktop-only transport fixture; normal products cannot select this endpoint.
use std::path::Path;
use super::openai_responses::ResponsesClientConfig;

pub(super) fn config(url: &str, root: &Path, store: &Path) -> anyhow::Result<ResponsesClientConfig> {
 let parsed = reqwest::Url::parse(url)?;
 anyhow::ensure!(parsed.scheme() == "http" && parsed.host_str() == Some("127.0.0.1")
  && parsed.port().is_some_and(|port| port != 0) && parsed.username().is_empty()
  && parsed.password().is_none() && parsed.query().is_none() && parsed.fragment().is_none()
  && parsed.path() == "/", "Review AI endpoint must be http://127.0.0.1:<port>/ with no credentials or extra path");
 let root = root.canonicalize()?;
 anyhow::ensure!(std::fs::read_to_string(root.join(".vac-native-review"))?.trim() == "isolated-desktop-close-v1", "Invalid review marker");
 anyhow::ensure!(store.canonicalize()? == root.join("store").canonicalize()?, "Review store mismatch");
 Ok(ResponsesClientConfig { api_key: "isolated-review-no-secret".into(), base_url: parsed.to_string(), ..Default::default() })
}

#[cfg(feature="desktop-native-review")]
pub(super) fn from_environment() -> anyhow::Result<ResponsesClientConfig> {
 #[cfg(not(debug_assertions))]
 compile_error!("Review AI endpoint must not be available in release builds");
 let url=std::env::var("UN_VAC_REVIEW_PROVIDER_URL")?;
 let root=std::env::var_os("UN_VAC_DESKTOP_REVIEW_ROOT").ok_or_else(||anyhow::anyhow!("Explicit review root required"))?;
 let store=std::env::var_os("UN_VAC_CONFIG_DIR").ok_or_else(||anyhow::anyhow!("Explicit review store required"))?;
 config(&url, Path::new(&root), Path::new(&store))
}

#[cfg(test)]
mod tests {
 use super::*;
 #[test]
 fn review_provider_requires_loopback_marked_store_and_uses_no_real_key() {
  let root=std::env::temp_dir().join(format!("vac-review-provider-{}",rand::random::<u64>()));
  let store=root.join("store");std::fs::create_dir_all(&store).unwrap();
  assert!(config("http://127.0.0.1:12345",&root,&store).is_err());
  std::fs::write(root.join(".vac-native-review"),"isolated-desktop-close-v1").unwrap();
  let good=config("http://127.0.0.1:12345",&root,&store).unwrap();
  assert_eq!(good.api_key,"isolated-review-no-secret");
  for bad in ["https://127.0.0.1:12345", "http://localhost:12345", "http://127.0.0.2:12345", "http://example.com:12345", "http://user:secret@127.0.0.1:12345", "http://127.0.0.1", "http://127.0.0.1:0", "http://127.0.0.1:12345/responses", "http://127.0.0.1:12345/?key=x", "http://127.0.0.1:12345/#x"] {
   assert!(config(bad,&root,&store).is_err(),"{bad}");
  }
  assert!(config("http://127.0.0.1:12345",&root,&root).is_err());
  std::fs::remove_dir_all(root).unwrap();
 }
}
