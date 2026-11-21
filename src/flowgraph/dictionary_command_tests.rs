//! Real persisted composition, with deterministic ingress and no external service.
#[tokio::test]
async fn managed_dictionary_commands_authorize_learn_undo_and_persist() {
    let root = std::env::temp_dir().join(format!("vac-managed-command-{}", rand::random::<u64>()));
    std::fs::create_dir_all(root.join("assets")).unwrap();
    std::fs::write(root.join("main.flowgraph.toml"),
        include_str!("../../flowgraph.example/dictionary-managed-command/main.flowgraph.toml")).unwrap();
    std::fs::write(root.join("assets/commands.tsv"),
        include_str!("../../flowgraph.example/dictionary-managed-command/assets/commands.tsv")).unwrap();
    let path = root.join("assets/user.tsv");
    std::fs::write(&path, include_str!("../../flowgraph.example/dictionary-managed-command/assets/user.tsv")).unwrap();
    for (actor, text, readings, changed) in [
        ("example_moderator", "学習（X := 古）", vec!["古"], true),
        ("example_moderator", "learn(X=新)", vec!["古", "新"], true),
        ("example_moderator", "learn(X=新)", vec!["古", "新"], false),
        ("stranger", "example_moderator: learn(X=偽)", vec!["古", "新"], false),
        ("stranger", "learn(X=偽)", vec!["古", "新"], false),
        ("example_moderator", "忘却(X)", vec!["古"], true),
        ("EXAMPLE_MODERATOR", "learn(X := 次)", vec!["古", "次"], true),
        ("example_moderator", "forget(X=古)", vec!["次"], true),
        ("example_moderator", "learn(X=)", vec!["次"], false),
        ("example_moderator", "forget(X=)", vec!["次"], false),
        ("example_moderator", "forget(X)", vec![], true),
        ("example_moderator", "forget(X)", vec![], false),
        ("example_moderator", "learn(名称 A := 読み A)", vec!["読み A"], true),
        ("example_moderator", "forget(名称 A=読み A)", vec![], true),
        ("example_moderator", "learn(X=Y:=Z)", vec!["Z"], true),
        ("example_moderator", "forget(X=Y:=Z)", vec![], true),
    ] {
        let before = std::fs::read(&path).unwrap();
        let quote = |s: &str| serde_json::to_string(s).unwrap();
        std::fs::write(root.join("main.flowgraph.test.toml"), format!(r#"
[[triggers]]
node="main::in"
exec=["__trigger__"]
[[triggers.overrides]]
port="__source_actor__"
ty="string"
value={}
[[triggers.overrides]]
port="__content__"
ty="string"
value={}
[[tests]]
name="Persisted command"
[tests.expect]
trigger_count=1
"#, quote(actor), quote(text))).unwrap();
        let result = super::fixture_runner::run_fixture_once_report(&root).await.unwrap();
        assert!(result.ok, "{text}: {:?}", result.tests);
        let contents = std::fs::read_to_string(&path).unwrap();
        let table = super::nodes::table_ops::parse_tsv_with_mode(&contents, "headerful").unwrap();
        let index = table.schema().column_index("reading").unwrap();
        let actual: Vec<_> = table.rows().iter().map(|row| row.0[index].as_str().unwrap()).collect();
        assert_eq!(actual, readings, "{actor}: {text}");
        assert_eq!(before != contents.as_bytes(), changed, "{actor}: {text}");
    }
    let locked = format!("{}固定\tX\t名詞\t\t0\ttrue\ttrue\tsystem\t\t\t\n",
        include_str!("../../flowgraph.example/dictionary-managed-command/assets/user.tsv"));
    std::fs::write(&path, &locked).unwrap();
    let fixture_path = root.join("main.flowgraph.test.toml");
    let fixture = std::fs::read_to_string(&fixture_path).unwrap().replace("forget(X=Y:=Z)", "forget(X)");
    std::fs::write(&fixture_path, &fixture).unwrap();
    let result = super::fixture_runner::run_fixture_once_report(&root).await.unwrap();
    assert!(result.ok, "{:?}", result.tests);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), locked);
    std::fs::write(&fixture_path, fixture.replace("forget(X)", "learn(Y=other)")).unwrap();
    let lock = std::fs::OpenOptions::new().read(true).write(true).open(root.join("assets/.vac-data-write.lock")).unwrap();
    lock.try_lock().unwrap();
    let result = super::fixture_runner::run_fixture_once_report(&root).await.unwrap();
    assert!(result.ok, "{:?}", result.tests);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), locked, "busy writer must not replace the dictionary");
    drop(lock);
    let result = super::fixture_runner::run_fixture_once_report(&root).await.unwrap();
    assert!(result.ok, "{:?}", result.tests);
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("other\tY"));
    assert!(saved.contains("固定\tX"));
    std::fs::remove_dir_all(root).unwrap();
}
