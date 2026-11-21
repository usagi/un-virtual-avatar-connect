//! Development adapter: run the production ZIP importer against a disposable
//! snapshot, without bootstrapping VAC, loading user settings or executing nodes.
use std::{collections::BTreeMap, io::{Read, Write}, path::{Component, Path, PathBuf}};
use serde::Deserialize;
use virtual_avatar_connect::flowgraph::fragment::zip_codec::{import_zip, ZipImportOptions};

#[derive(Deserialize)]
struct Request {
    files: BTreeMap<String, Vec<u8>>,
    #[serde(default)]
    archive: Vec<u8>,
    #[serde(default)]
    options: ZipImportOptions,
    #[serde(default)]
    read: Option<ReadRequest>,
}

#[derive(Deserialize)]
struct ReadRequest {
    operation: String,
    request: serde_json::Value,
}

fn recovery_error(response: actix_web::HttpResponse) -> anyhow::Error {
    use actix_web::body::MessageBody;
    let status=response.status();
    let detail=response.into_body().try_into_bytes().ok().map(|b|String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    anyhow::anyhow!("{status}: {detail}")
}

struct Workspace(PathBuf);
impl Workspace {
    fn create() -> std::io::Result<Self> {
        let root = std::env::temp_dir().canonicalize()?.join(format!("unvac-isolated-import-{}-{}", std::process::id(), rand::random::<u64>()));
        std::fs::create_dir(&root)?;
        Ok(Self(root))
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        // Only this successfully created, absolute temporary root is removed.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn relative_path(name: &str) -> anyhow::Result<&Path> {
    let path = Path::new(name);
    anyhow::ensure!(!name.is_empty() && !name.contains('\\') && path.components().all(|part| matches!(part, Component::Normal(_))), "invalid snapshot path");
    Ok(path)
}

fn collect(root: &Path, directory: &Path, files: &mut BTreeMap<String, Vec<u8>>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        anyhow::ensure!(!kind.is_symlink(), "unexpected link in isolated workspace");
        if kind.is_dir() { collect(root, &entry.path(), files)?; }
        else if kind.is_file() {
            files.insert(entry.path().strip_prefix(root)?.to_string_lossy().replace('\\', "/"), std::fs::read(entry.path())?);
        }
    }
    Ok(())
}

fn run(request: Request) -> anyhow::Result<serde_json::Value> {
    anyhow::ensure!(request.archive.len() <= 1_000_000, "preview archive too large");
    anyhow::ensure!(request.files.len() <= 2048 && request.files.values().map(Vec::len).sum::<usize>() <= 32_000_000, "preview snapshot too large");
    if let Ok(mut archive) = zip::ZipArchive::new(std::io::Cursor::new(&request.archive)) {
        anyhow::ensure!(archive.len() <= 2048, "too many preview archive entries");
        let mut expanded = 0u64;
        for index in 0..archive.len() {
            expanded = expanded.checked_add(archive.by_index(index)?.size()).ok_or_else(|| anyhow::anyhow!("archive size overflow"))?;
            anyhow::ensure!(expanded <= 32_000_000, "preview expanded archive too large");
        }
    }
    // Validate all names before any writes, even though the root is disposable.
    for name in request.files.keys() { relative_path(name)?; }
    let workspace = Workspace::create()?;
    for (name, bytes) in request.files {
        let path = workspace.0.join(relative_path(&name)?);
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, bytes)?;
    }
    if let Some(read) = request.read {
        use virtual_avatar_connect::flowgraph::fragment::{copy_targets, serialize_fragment, CopyRequest};
        let result = (|| -> anyhow::Result<serde_json::Value> {
            match read.operation.as_str() {
                "purpose-read" => {
                    Ok(serde_json::to_value(virtual_avatar_connect::web_interface::control::flowgraph::purposes::read(&workspace.0)?)?)
                }
                "purpose-history" => {
                    Ok(serde_json::json!({"root":workspace.0.canonicalize()?.to_string_lossy().replace('\\',"/"),"entries":virtual_avatar_connect::web_interface::control::flowgraph::purposes::history(&workspace.0)?}))
                }
                "purpose-save" => {
                    use virtual_avatar_connect::web_interface::control::flowgraph::purposes;
                    let mut expected: purposes::Snapshot = serde_json::from_value(read.request["expected"].clone())?;
                    anyhow::ensure!(expected.root == "__isolated_root__", "invalid isolated purpose root");
                    expected.root = workspace.0.canonicalize()?.to_string_lossy().replace('\\', "/");
                    let document: purposes::Document = serde_json::from_value(read.request["document"].clone())?;
                    Ok(serde_json::to_value(purposes::save(&workspace.0, &expected, document)?)?)
                }
                "removal-plan" => {
                    let fq=read.request["fq"].as_str().ok_or_else(||anyhow::anyhow!("missing fq"))?;
                    virtual_avatar_connect::web_interface::control::flowgraph::removal_plan_snapshot(&workspace.0,fq).map_err(recovery_error)
                }
                "delete-file" => {
                    let fq=read.request["fq"].as_str().ok_or_else(||anyhow::anyhow!("missing fq"))?;
                    let expected=read.request["expected_content"].as_str().ok_or_else(||anyhow::anyhow!("missing expected content"))?;
                    let backup=virtual_avatar_connect::web_interface::control::flowgraph::remove_file_without_reload(&workspace.0,fq,Some(expected),Some(&workspace.0.to_string_lossy().replace('\\',"/"))).map_err(recovery_error)?;
                    Ok(serde_json::json!({"fq":fq,"path":format!("{fq}.flowgraph.toml"),"backup":backup,"ok":false,
                        "diagnostics":[{"severity":"warning","message":"隔離プレビューのファイルを削除。実行への適用はしていません。"}]}))
                }
                "recovery-history" => {
                    let entries=virtual_avatar_connect::web_interface::control::flowgraph::file_recovery::history(&workspace.0).map_err(recovery_error)?;
                    Ok(serde_json::json!({"root_dir":workspace.0.to_string_lossy().replace('\\',"/"),"entries":entries}))
                }
                "recovery-plan" => {
                    let fq=read.request["fq"].as_str().ok_or_else(||anyhow::anyhow!("missing fq"))?;
                    let backup=read.request["backup"].as_str().ok_or_else(||anyhow::anyhow!("missing backup"))?;
                    Ok(serde_json::to_value(virtual_avatar_connect::web_interface::control::flowgraph::file_recovery::plan(&workspace.0,fq,backup).map_err(recovery_error)?)?)
                }
                "restore-file" => {
                    let mut expected: virtual_avatar_connect::web_interface::control::flowgraph::file_recovery::RecoveryPlan=serde_json::from_value(read.request)?;
                    anyhow::ensure!(expected.root_dir=="__isolated_root__","invalid isolated root token");
                    expected.root_dir=workspace.0.to_string_lossy().replace('\\',"/");
                    let backup=virtual_avatar_connect::web_interface::control::flowgraph::file_recovery::restore(&workspace.0,&expected.fq,&expected).map_err(recovery_error)?;
                    Ok(serde_json::json!({"fq":expected.fq,"path":format!("{}.flowgraph.toml",expected.fq),"backup":backup,"ok":false,
                        "diagnostics":[{"severity":"warning","message":"隔離プレビューのファイルを復元。実行への適用はしていません。"}]}))
                }
                "companions" => Ok(serde_json::to_value(virtual_avatar_connect::flowgraph::fragment::zip_codec::list_companion_candidates(&workspace.0)?)?),
                "export" => {
                    let request: virtual_avatar_connect::flowgraph::fragment::zip_codec::ZipExportRequest = serde_json::from_value(read.request)?;
                    Ok(serde_json::json!({"archive": virtual_avatar_connect::web_interface::control::flowgraph::export_snapshot(&workspace.0, &request).map_err(recovery_error)?}))
                }
                "share-plan" => {
                    let files: Vec<String> = serde_json::from_value(read.request.get("files").cloned().ok_or_else(|| anyhow::anyhow!("missing files"))?)?;
                    virtual_avatar_connect::web_interface::control::flowgraph::share_plan_snapshot(&workspace.0, &files).map_err(recovery_error)
                }
                "copy" => {
                    let copy: CopyRequest = serde_json::from_value(read.request)?;
                    let fragment = copy_targets(&workspace.0, &copy)?;
                    Ok(serde_json::json!({"fragment_toml":serialize_fragment(&fragment).map_err(anyhow::Error::msg)?,"file_count":fragment.files.len(),"dangling_count":fragment.danglings.len(),"fragment":fragment}))
                }
                _ => anyhow::bail!("unknown read operation"),
            }
        })();
        let mut files=BTreeMap::new();
        collect(&workspace.0,&workspace.0,&mut files)?;
        return Ok(match result {
            Ok(value) => serde_json::json!({"ok":true,"result":value,"scratch_root":workspace.0,"files":files}),
            Err(error) => serde_json::json!({"ok":false,"error":error.to_string(),"scratch_root":workspace.0,"files":files}),
        });
    }
    let outcome = import_zip(&workspace.0, &request.archive, &request.options);
    let mut files = BTreeMap::new();
    collect(&workspace.0, &workspace.0, &mut files)?;
    // Error snapshots deliberately retain partial progress; callers must not
    // mistake an importer error for proof that no files changed.
    Ok(match outcome {
        Ok(outcome) => serde_json::json!({"ok":true,"outcome":outcome,"files":files,"scratch_root":workspace.0}),
        Err(error) => serde_json::json!({"ok":false,"error":error.to_string(),"files":files,"scratch_root":workspace.0}),
    })
}

fn main() -> anyhow::Result<()> {
    let mut input = Vec::new();
    std::io::stdin().take(160_000_001).read_to_end(&mut input)?;
    anyhow::ensure!(input.len() <= 160_000_000, "request too large");
    let result = run(serde_json::from_slice(&input)?)?;
    serde_json::to_writer(std::io::stdout().lock(), &result)?;
    std::io::stdout().write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arbitrary_two_graph_archive_dry_run_and_import_use_native_codec() {
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, text) in [
            ("manifest.flowgraph.toml", "[fragment]\nschema='vac-flowgraph-fragment/v2'\nexported_at='2026-10-18T00:00:00Z'\nscope='folder'\n"),
            ("flowgraph/one.flowgraph.toml", "[meta]\ntitle='Custom one'\n"),
            ("flowgraph/two.flowgraph.toml", "[meta]\ntitle='Custom two'\n"),
        ] {
            archive.start_file(name, options).unwrap();
            archive.write_all(text.as_bytes()).unwrap();
        }
        let bytes = archive.finish().unwrap().into_inner();
        for dry_run in [true, false] {
            let result = run(Request {read:None, files:BTreeMap::new(), archive:bytes.clone(), options:ZipImportOptions {
                dry_run, target_prefix:"my-work".into(), ..Default::default()
            }}).unwrap();
            assert_eq!(result["ok"], true, "{result}");
            let files = result["files"].as_object().unwrap();
            assert_eq!(files.len(), if dry_run {0} else {2});
            if !dry_run {
                assert!(files.contains_key("my-work/one.flowgraph.toml"));
                assert!(files.contains_key("my-work/two.flowgraph.toml"));
            }
        }
    }
    #[test]
    fn traversal_rejected_and_invalid_archive_preserves_snapshot() {
        for path in ["../outside", "/outside", "a/../outside", "C:/outside", "a\\outside"] {
            assert!(relative_path(path).is_err(), "{path}");
        }
        let result = run(Request {read:None, files:BTreeMap::from([("existing.flowgraph.toml".into(), b"# unchanged".to_vec())]),archive:vec![],options:ZipImportOptions::default()}).unwrap();
        assert_eq!(result["ok"], false);
        assert_eq!(result["files"]["existing.flowgraph.toml"], serde_json::json!(b"# unchanged".to_vec()));
    }
}
