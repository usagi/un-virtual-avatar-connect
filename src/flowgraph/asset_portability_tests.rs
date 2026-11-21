use super::*;

#[tokio::test]
async fn imported_module_reads_its_own_assets_in_spawned_worker() {
    use crate::flowgraph::fragment::{CopyRequest, zip_codec::{export_zip_with_companions, import_zip, ZipImportOptions}};
    let outer = std::env::temp_dir().join(format!("vac-imported-assets-{}", rand::random::<u64>()));
    let source = outer.join("source");
    let target = outer.join("target");
    std::fs::create_dir_all(source.join("pack/assets")).unwrap();
    std::fs::create_dir(&target).unwrap();
    std::fs::write(source.join("main.flowgraph.toml"), "[[instances]]\nid='asset'\ngraph='./pack/module'\n").unwrap();
    std::fs::write(source.join("pack/module.flowgraph.toml"), r#"
[graph]
kind='module'
[[nodes]]
id='text_path'
feature='flowgraph.literal.string'
[nodes.properties]
value='graph-file:assets/readme.txt'
[[nodes]]
id='read'
feature='flowgraph.file.read_string'
[[nodes]]
id='table_path'
feature='flowgraph.literal.string'
[nodes.properties]
value='graph-file:assets/dictionary.tsv'
[[nodes]]
id='table'
feature='flowgraph.table.load_tsv'
[[nodes]]
id='audio_path'
feature='flowgraph.literal.string'
[nodes.properties]
value='graph-file:assets/silence.wav'
[[nodes]]
id='audio'
feature='flowgraph.audio.play'
[[edges]]
from='text_path:value'
to='read:path'
[[edges]]
from='table_path:value'
to='table:path'
[[edges]]
from='audio_path:value'
to='audio:path'
"#).unwrap();
    std::fs::write(source.join("pack/assets/readme.txt"), "portable text").unwrap();
    std::fs::write(source.join("pack/assets/dictionary.tsv"), "reading\tsurface\tpart_of_speech\nうさぎ\tUSAGI\t名詞\n").unwrap();
    // One silent PCM sample. Decode validation is offline; no output device opens.
    let mut wav = Vec::new();
    wav.extend(b"RIFF"); wav.extend(38u32.to_le_bytes()); wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes()); wav.extend(1u16.to_le_bytes()); wav.extend(1u16.to_le_bytes());
    wav.extend(8000u32.to_le_bytes()); wav.extend(16000u32.to_le_bytes());
    wav.extend(2u16.to_le_bytes()); wav.extend(16u16.to_le_bytes()); wav.extend(b"data");
    wav.extend(2u32.to_le_bytes()); wav.extend(0i16.to_le_bytes());
    std::fs::write(source.join("pack/assets/silence.wav"), &wav).unwrap();
    let copy: CopyRequest = serde_json::from_value(serde_json::json!({"scope":"mixed","targets":[{"kind":"file","fq":"main"},{"kind":"file","fq":"pack/module"}]})).unwrap();
    let archive = export_zip_with_companions(&source, &copy, &["pack/assets/readme.txt".into(), "pack/assets/silence.wav".into(), "pack/assets/dictionary.tsv".into()]).unwrap();
    import_zip(&target, &archive, &ZipImportOptions { target_prefix:"received".into(), ..Default::default() }).unwrap();
    std::fs::remove_dir_all(&source).unwrap();
    let share = fragment::composition_share::plan_composition_share(&target, &["received/main".into()]).unwrap();
    assert_eq!(share.asset_candidates.len(), 3);
    assert!(share.asset_candidates.iter().all(|a| a.error.is_none()));
    assert!(share.asset_candidates.iter().any(|a| a.target.as_deref() == Some("received/pack/assets/readme.txt")));
    assert!(share.asset_candidates.iter().any(|a| a.target.as_deref() == Some("received/pack/assets/silence.wav")));
    let imported_wav = std::fs::read(target.join("received/pack/assets/silence.wav")).unwrap();
    assert_eq!(imported_wav, wav);
    assert_eq!(rodio::Decoder::new(std::io::Cursor::new(imported_wav)).unwrap().count(), 1);
    std::fs::write(target.join("portable.flowgraph.test.toml"), r#"
[[triggers]]
node='received/main/@asset::read'
exec=['exec_in']
[[triggers]]
node='received/main/@asset::table'
exec=['exec_in']
[[tests]]
name='Imported module reads its defining-source companion'
[[tests.expect.stored_values]]
node='received/main/@asset::read'
port='text'
value='portable text'
[[tests.expect.stored_values]]
node='received/main/@asset::table'
port='row_count'
value=1
"#).unwrap();
    let fixture = fixture_runner::run_fixture_once_report(&target).await.unwrap();
    assert!(fixture.ok, "tests={:?}, values={:?}", fixture.tests, fixture.stored_values);
    assert_eq!(fixture.tests.len(), 1);
    let rt = FlowgraphRuntime::load_and_spawn(&target, std::sync::Weak::new(), None, None, None, None);
    assert!(rt.ok, "{:?}", rt.diagnostics);
    let handle = rt.handle.as_ref().unwrap();
    for node in ["received/main/@asset::read", "received/main/@asset::audio", "received/main/@asset::table"] {
        handle.trigger.send_exec(node, "exec_in").unwrap();
    }
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let complete = {
                let events = handle.runtime_trace.read().unwrap();
                ["received/main/@asset::read", "received/main/@asset::audio", "received/main/@asset::table"].iter().all(|node|
                    events.iter().any(|event| event.runtime_node_id == *node && event.port == "on_success" && event.phase == RuntimeTracePhase::ExecOutput))
            };
            if complete { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await;
    let trace = handle.runtime_trace.read().unwrap().clone();
    handle.shutdown().await;
    std::fs::remove_dir_all(outer).unwrap();
    assert!(outcome.is_ok(), "imported asset reads did not succeed: {trace:?}");
}
