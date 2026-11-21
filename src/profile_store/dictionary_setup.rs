//! Dictionary registration preparation and locked publication; no runtime application.
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, Value};

pub const EMPTY_GLOSSARY: &str = "reading\tsurface\tpart_of_speech\tcomment\tpriority\tis_locked\tenabled\tby\tcreated_at\texpires_at\ttags\n";
pub enum DictionarySource { New, Existing(PathBuf) }
pub struct RegistrationInput {
    pub expected_revision: String,
    pub key: String,
    pub label: String,
    pub source: DictionarySource,
}
pub struct RegistrationPlan {
    pub conf_path: PathBuf,
    pub source_revision: String,
    pub updated_conf: String,
    pub table_path: PathBuf,
    /// Some only for a new file. Publication must never overwrite existing data.
    pub new_file_contents: Option<&'static str>,
}

/// A created dictionary is retained if configuration publication fails. The
/// caller can offer the same file for explicit inspection and registration;
/// silently deleting it would race writers outside the configuration lock.
#[derive(Debug, thiserror::Error)]
#[error("設定保存に失敗しました。辞書は保持しています: {dictionary_path}。内容を確認して既存辞書として登録できます。")]
pub struct IncompleteRegistration {
    pub dictionary_path: PathBuf,
    #[source]
    pub source: anyhow::Error,
}

/// This is not an installation: publication must recheck identity/revision under
/// the profile lock, validate existing TSVs and reject path aliases/collisions.
/// No graph is implicitly edited and no existing file is replaced here.
pub fn prepare_registration(conf_path: &Path, original: &str, input: &RegistrationInput) -> Result<RegistrationPlan> {
    if !conf_path.is_absolute() { bail!("設定ファイルの絶対パスが必要です。"); }
    let revision = blake3::hash(original.as_bytes()).to_hex().to_string();
    if input.expected_revision != revision { bail!("設定が変更されています。入力を保持して読み直してください。"); }
    let key = input.key.trim();
    if key.is_empty() || key.len() > 64 || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) || key == "." || key == ".." {
        bail!("辞書IDには英数字と . _ - を使用してください。");
    }
    // These names remain Windows devices even with an extension (CON.tsv,
    // LPT1.custom.tsv). Reject before creating directories or staging files.
    // Keep this portable so a configuration prepared elsewhere works on Windows.
    let stem = key.split('.').next().unwrap_or_default().to_ascii_uppercase();
    let numbered_device = stem.strip_prefix("COM").or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'));
    if matches!(input.source, DictionarySource::New)
        && (matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || numbered_device) {
        bail!("この辞書IDはWindowsの予約名です。別のIDを指定してください。");
    }
    let label = input.label.trim();
    if label.is_empty() || label.chars().count() > 120 || label.chars().any(char::is_control) { bail!("辞書名を入力してください。"); }
    let conf: crate::Conf = toml::from_str(original).context("設定を読み取れません。")?;
    if conf.control_api.as_ref().is_some_and(|c| c.tables.iter().any(|t| t.key.eq_ignore_ascii_case(key))) { bail!("同じIDの辞書が登録されています。"); }
    let (table_path, new_file_contents) = match &input.source {
        DictionarySource::New => (conf_path.parent().context("設定フォルダーがありません。")?.join("dictionaries").join(format!("{key}.tsv")), Some(EMPTY_GLOSSARY)),
        DictionarySource::Existing(path) => {
            if !path.is_absolute() || path.file_name().is_none() { bail!("既存辞書の絶対パスが必要です。"); }
            (path.clone(), None)
        }
    };
    let path_text = table_path.to_str().context("辞書のパスを文字列として扱えません。")?;
    if path_text.chars().any(char::is_control) { bail!("辞書パスに制御文字は使えません。"); }
    let mut doc: DocumentMut = original.parse()?;
    let control = doc.entry("control_api").or_insert(Item::Table(Table::new())).as_table_like_mut().context("control_apiの形式が不正です。")?;
    let mut entry = Table::new();
    entry["key"] = toml_edit::value(key);
    entry["path"] = toml_edit::value(path_text);
    entry["label"] = toml_edit::value(label);
    entry["role"] = toml_edit::value("glossary");
    entry["editable"] = toml_edit::value(true);
    if !control.contains_key("tables") { control.insert("tables", Item::ArrayOfTables(ArrayOfTables::new())); }
    match control.get_mut("tables").context("辞書一覧がありません。")? {
        Item::ArrayOfTables(tables) => tables.push(entry),
        Item::Value(Value::Array(tables)) => tables.push(Value::InlineTable(entry.into_inline_table())),
        _ => bail!("辞書一覧の形式が不正です。"),
    }
    let updated_conf = doc.to_string();
    let _: crate::Conf = toml::from_str(&updated_conf).context("登録後の設定を検証できません。")?;
    Ok(RegistrationPlan { conf_path: conf_path.to_path_buf(), source_revision: revision, updated_conf, table_path, new_file_contents })
}

/// Inspect a selected file without modifying or normalizing its bytes.
pub fn inspect_existing(path: &Path) -> Result<(PathBuf, String, usize)> {
    let canonical = std::fs::canonicalize(path).context("既存辞書を確認できません。")?;
    if !canonical.is_file() { bail!("辞書ファイルを選んでください。"); }
    let bytes = std::fs::read(&canonical)?;
    let text = std::str::from_utf8(&bytes).context("辞書はUTF-8のTSVを指定してください。")?;
    let table = crate::flowgraph::nodes::table_ops::parse_tsv_with_mode(text, "auto")?;
    if table.schema().column_index("reading").is_none() || table.schema().column_index("surface").is_none() {
        bail!("読みと単語を扱える辞書ではありません。");
    }
    Ok((canonical, blake3::hash(&bytes).to_hex().to_string(), table.len()))
}

pub fn save_registration(conf_path: &Path, input: &RegistrationInput, existing_revision: Option<&str>) -> Result<RegistrationPlan> {
    save_registration_with(conf_path, input, existing_revision, super::write_config_transaction)
}

fn save_registration_with(conf_path: &Path, input: &RegistrationInput, existing_revision: Option<&str>, publish: impl FnOnce(&Path, &[u8]) -> Result<()>) -> Result<RegistrationPlan> {
    use std::io::Write;
    let _guard = super::lifecycle::lock_config_write(conf_path)?;
    let original = std::fs::read_to_string(conf_path)?;
    let plan = prepare_registration(conf_path, &original, input)?;
    if plan.new_file_contents.is_none() {
        let (canonical, revision, _) = inspect_existing(&plan.table_path)?;
        if existing_revision != Some(revision.as_str()) { bail!("辞書の内容が変更されています。確認し直してください。"); }
        // Store the inspected destination, not a path alias whose target may
        // change between registration and the next application startup.
        let resolved = RegistrationInput {
            expected_revision: input.expected_revision.clone(),
            key: input.key.clone(),
            label: input.label.clone(),
            source: DictionarySource::Existing(canonical),
        };
        let plan = prepare_registration(conf_path, &original, &resolved)?;
        publish(&plan.conf_path, plan.updated_conf.as_bytes())?;
        return Ok(plan);
    }
    let contents = plan.new_file_contents.unwrap();
    let directory = plan.table_path.parent().context("保存先がありません。")?;
    std::fs::create_dir_all(directory)?;
    let profile = std::fs::canonicalize(conf_path.parent().context("設定フォルダーがありません。")?)?;
    if std::fs::canonicalize(directory)? != profile.join("dictionaries") {
        bail!("辞書の保存先がプロファイルの外へ変更されています。");
    }
    // Write a private staging file fully before publishing a new dictionary.
    // hard_link fails if the destination exists, unlike rename on Unix.
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    let staged_path = directory.join(format!(".dictionary-{}-{nonce}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&staged_path)?;
    let staged = std::mem::DropGuard::new(staged_path, |path| {
        let _ = std::fs::remove_file(path);
    });
    let staged_write = file.write_all(contents.as_bytes()).and_then(|_| file.sync_all());
    drop(file);
    staged_write?;
    std::fs::hard_link(&*staged, &plan.table_path)
        .context("辞書を作成できません。同じ保存先がある場合、その内容は変更していません。")?;
    drop(staged);
    if let Err(error) = publish(&plan.conf_path, plan.updated_conf.as_bytes()) {
        return Err(IncompleteRegistration { dictionary_path: plan.table_path, source: error }.into());
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let nonce=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let path=std::env::temp_dir().join(format!("vac-dictionary-test-{}-{nonce}",std::process::id()));
            std::fs::create_dir(&path).unwrap();Self(path)
        }
        fn path(&self)->&Path { &self.0 }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            if let (Ok(path),Ok(root))=(std::fs::canonicalize(&self.0),std::fs::canonicalize(std::env::temp_dir())) {
                if path.starts_with(&root) && path.file_name().unwrap().to_string_lossy().starts_with("vac-dictionary-test-") {
                    let _=std::fs::remove_dir_all(path);
                }
            }
        }
    }
    fn input(s: &str) -> RegistrationInput { RegistrationInput { expected_revision: blake3::hash(s.as_bytes()).to_hex().to_string(), key:"stream".into(), label:"配信の読み方".into(), source:DictionarySource::New } }
    fn location() -> PathBuf { std::env::temp_dir().join("unvac-dictionary-preparation-only").join("conf.toml") }
    #[test]
    fn dictionary_registration_preserves_existing_config_and_comments() {
        let s="# keep comment\nworkers=2\n[control_api]\nenabled=true\n[[control_api.tables]]\nkey='other'\npath='existing.tsv'\neditable=false\n";
        let plan=prepare_registration(&location(),s,&input(s)).unwrap();
        assert_eq!(plan.source_revision,blake3::hash(s.as_bytes()).to_hex().to_string());
        assert!(plan.updated_conf.contains("# keep comment"));
        let conf:crate::Conf=toml::from_str(&plan.updated_conf).unwrap();
        assert_eq!(conf.control_api.as_ref().unwrap().tables.len(),2);assert!(!conf.control_api.as_ref().unwrap().tables[0].editable);
        assert_eq!(conf.control_api.as_ref().unwrap().tables[0].path,PathBuf::from("existing.tsv"));
        assert_eq!(conf.control_api.as_ref().unwrap().tables[1].path,plan.table_path);
        assert!(conf.control_api.as_ref().unwrap().tables[1].quick_add.is_none());
        assert_eq!(plan.new_file_contents,Some(EMPTY_GLOSSARY));
    }
    #[test]
    fn dictionary_registration_accepts_inline_array_without_replacing_entries() {
        let s="[control_api]\ntables=[{key='old',path='old.tsv',editable=false}]\n";
        let plan=prepare_registration(&location(),s,&input(s)).unwrap();
        let conf:crate::Conf=toml::from_str(&plan.updated_conf).unwrap();
        assert_eq!(conf.control_api.as_ref().unwrap().tables[0].key,"old");assert_eq!(conf.control_api.as_ref().unwrap().tables.len(),2);
    }
    #[test]
    fn dictionary_registration_rejects_stale_duplicate_and_invalid_inputs() {
        let s="";let mut i=input(s);i.expected_revision="stale".into();assert!(prepare_registration(&location(),s,&i).is_err());
        for key in ["../escape","", ".", "..", "name/path"] {let mut i=input(s);i.key=key.into();assert!(prepare_registration(&location(),s,&i).is_err());}
        let s="[control_api]\ntables=[{key='STREAM',path='old.tsv'}]";assert!(prepare_registration(&location(),s,&input(s)).is_err());
    }
    #[test]
    fn dictionary_registration_existing_is_reference_only_and_profile_paths_differ() {
        let s="";let a=prepare_registration(&location(),s,&input(s)).unwrap();
        let b=prepare_registration(&location().parent().unwrap().join("other/conf.toml"),s,&input(s)).unwrap();assert_ne!(a.table_path,b.table_path);
        let mut i=input(s);i.source=DictionarySource::Existing(location().with_file_name("shared.tsv"));
        let p=prepare_registration(&location(),s,&i).unwrap();assert!(p.new_file_contents.is_none());
        i.source=DictionarySource::Existing(PathBuf::from("relative.tsv"));assert!(prepare_registration(&location(),s,&i).is_err());
    }
    #[test]
    fn dictionary_registration_save_and_rollback_preserve_existing_files() {
        let dir=TestDirectory::new();let conf=dir.path().join("conf.toml");std::fs::write(&conf, "# original\n").unwrap();
        let i=input("# original\n");
        assert!(save_registration_with(&conf,&i,None,|_,_|bail!("injected write failure")).is_err());
        assert_eq!(std::fs::read_to_string(&conf).unwrap(),"# original\n");
        assert_eq!(std::fs::read_to_string(dir.path().join("dictionaries/stream.tsv")).unwrap(),EMPTY_GLOSSARY);
        std::fs::write(dir.path().join("dictionaries/stream.tsv"),"owned by user").unwrap();
        assert!(save_registration(&conf,&i,None).is_err());
        assert_eq!(std::fs::read_to_string(dir.path().join("dictionaries/stream.tsv")).unwrap(),"owned by user");
        let mut other=input("# original\n");other.key="second".into();
        let saved=save_registration(&conf,&other,None).unwrap();
        assert_eq!(std::fs::read_to_string(&saved.table_path).unwrap(),EMPTY_GLOSSARY);
        assert_eq!(std::fs::read_to_string(&conf).unwrap(),saved.updated_conf);
    }
    #[test]
    fn dictionary_registration_existing_changed_and_shared_lock_rejected() {
        let dir=TestDirectory::new();let conf=dir.path().join("conf.toml");std::fs::write(&conf, "").unwrap();
        let dictionary=dir.path().join("external.tsv");std::fs::write(&dictionary,EMPTY_GLOSSARY).unwrap();
        let (_,revision,rows)=inspect_existing(&dictionary).unwrap();assert_eq!(rows,0);
        let mut i=input("");i.source=DictionarySource::Existing(dictionary.clone());
        assert!(save_registration(&conf,&i,Some("stale")).is_err());
        let held=super::super::lifecycle::lock_config_write(&conf).unwrap();
        assert!(save_registration(&conf,&i,Some(&revision)).is_err());drop(held);
        save_registration(&conf,&i,Some(&revision)).unwrap();
        assert_eq!(std::fs::read_to_string(dictionary).unwrap(),EMPTY_GLOSSARY);
    }

    #[test]
    fn dictionary_registration_failed_publication_retains_changed_dictionary() {
        let dir=TestDirectory::new();
        let conf=dir.path().join("conf.toml");
        std::fs::write(&conf, "").unwrap();
        let dictionary=dir.path().join("dictionaries/stream.tsv");
        let result=save_registration_with(&conf,&input(""),None,|_,_| {
            std::fs::write(&dictionary,"external change")?;
            bail!("injected publication failure")
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&conf).unwrap(),"");
        assert_eq!(std::fs::read_to_string(&dictionary).unwrap(),"external change");
        assert_eq!(std::fs::read_dir(dictionary.parent().unwrap()).unwrap().count(),1);
    }

    #[test]
    fn dictionary_registration_device_names_fail_before_file_changes() {
        let dir = TestDirectory::new();
        let conf = dir.path().join("conf.toml");
        let original = "# preserve this configuration\n";
        std::fs::write(&conf, original).unwrap();
        for key in ["CON", "con.extra", "PRN", "aux", "NUL", "COM1", "com9.custom", "LPT1", "lpt9"] {
            let mut request = input(original);
            request.key = key.into();
            let error = save_registration(&conf, &request, None).err().expect("reserved name accepted");
            assert!(error.to_string().contains("予約名"), "{key}: {error}");
            assert_eq!(std::fs::read_to_string(&conf).unwrap(), original);
            assert!(!dir.path().join("dictionaries").exists());
        }
        for key in ["console", "auxiliary", "com10", "lpt0", "stream.1"] {
            let mut request = input(original);
            request.key = key.into();
            assert!(prepare_registration(&conf, original, &request).is_ok(), "{key}");
        }
        // An ID referencing an existing ordinary file is not a filename.
        let mut request = input(original);
        request.key = "CON".into();
        request.source = DictionarySource::Existing(dir.path().join("ordinary.tsv"));
        assert!(prepare_registration(&conf, original, &request).is_ok());
    }

    #[test]
    fn dictionary_registration_existing_stores_inspected_path_without_rewriting_file() {
        let dir = TestDirectory::new();
        let conf = dir.path().join("conf.toml");
        std::fs::write(&conf, "").unwrap();
        let dictionary = dir.path().join("shared.tsv");
        std::fs::write(&dictionary, EMPTY_GLOSSARY).unwrap();
        let (canonical, revision, _) = inspect_existing(&dictionary).unwrap();
        let mut request = input("");
        request.source = DictionarySource::Existing(dir.path().join(".").join("shared.tsv"));
        let plan = save_registration(&conf, &request, Some(&revision)).unwrap();
        assert_eq!(plan.table_path, canonical);
        let saved: crate::Conf = toml::from_str(&std::fs::read_to_string(&conf).unwrap()).unwrap();
        assert_eq!(saved.control_api.unwrap().tables[0].path, canonical);
        assert_eq!(std::fs::read_to_string(&dictionary).unwrap(), EMPTY_GLOSSARY);
    }

    #[test]
    fn dictionary_registration_invalid_existing_file_preserves_both_files() {
        let dir = TestDirectory::new();
        let conf = dir.path().join("conf.toml");
        std::fs::write(&conf, "# retain\n").unwrap();
        let dictionary = dir.path().join("not-a-dictionary.tsv");
        let bytes = b"reading\tsurface\n\xff\tbroken\n";
        std::fs::write(&dictionary, bytes).unwrap();
        let mut request = input("# retain\n");
        request.source = DictionarySource::Existing(dictionary.clone());
        let revision = blake3::hash(bytes).to_hex().to_string();
        assert!(save_registration(&conf, &request, Some(&revision)).is_err());
        assert_eq!(std::fs::read_to_string(conf).unwrap(), "# retain\n");
        assert_eq!(std::fs::read(dictionary).unwrap(), bytes);
    }

    #[test]
    fn dictionary_registration_keeps_headerless_ime_support() {
        let dir = TestDirectory::new();
        let conf = dir.path().join("conf.toml");
        std::fs::write(&conf, "").unwrap();
        let dictionary = dir.path().join("ime.tsv");
        let original = "うさぎ\tUSAGI\n";
        std::fs::write(&dictionary, original).unwrap();
        let (_, revision, rows) = inspect_existing(&dictionary).unwrap();
        assert_eq!(rows, 1);
        let mut request = input("");
        request.source = DictionarySource::Existing(dictionary.clone());
        save_registration(&conf, &request, Some(&revision)).unwrap();
        assert_eq!(std::fs::read_to_string(dictionary).unwrap(), original);
    }

    #[test]
    fn dictionary_registration_failed_new_file_can_be_reviewed_and_registered_without_rewrite() {
        let dir=TestDirectory::new();let conf=dir.path().join("conf.toml");
        std::fs::write(&conf,"# original\n").unwrap();
        let failed=save_registration_with(&conf,&input("# original\n"),None,|_,_|bail!("injected failure")).err().unwrap();
        let retained=failed.downcast_ref::<IncompleteRegistration>().unwrap().dictionary_path.clone();
        assert_eq!(std::fs::read_to_string(&conf).unwrap(),"# original\n");
        let contents=format!("{}うさぎ\tUSAGI\t名詞\t\t0\tfalse\ttrue\tuser\t\t\t\n",EMPTY_GLOSSARY);
        std::fs::write(&retained,&contents).unwrap();
        let (_,revision,rows)=inspect_existing(&retained).unwrap();assert_eq!(rows,1);
        let mut reviewed=input("# original\n");reviewed.source=DictionarySource::Existing(retained.clone());
        save_registration(&conf,&reviewed,Some(&revision)).unwrap();
        assert_eq!(std::fs::read_to_string(&retained).unwrap(),contents);
        let saved:crate::Conf=toml::from_str(&std::fs::read_to_string(conf).unwrap()).unwrap();
        assert_eq!(saved.control_api.unwrap().tables[0].path,retained.canonicalize().unwrap());
    }

}
