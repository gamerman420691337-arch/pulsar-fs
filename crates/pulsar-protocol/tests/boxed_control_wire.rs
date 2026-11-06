//! Frozen wire fixtures guard tagged serialization and inline message storage.
use pulsar_protocol::*;
use serde::Deserialize;
#[derive(Deserialize)]
struct Fixture {
    kind: String,
    json: String,
}
#[test]
fn previous_wire_bytes_decode_and_reencode_without_pointer_wrappers() {
    let fixtures: Vec<Fixture> =
        serde_json::from_str(include_str!("fixtures/boxed-control-wire.json")).unwrap();
    assert_eq!(fixtures.len(), 3);
    for fixture in fixtures {
        let actual = match fixture.kind.as_str() {
            "generate" => serde_json::to_string(
                &serde_json::from_str::<WorkerOperation>(&fixture.json).unwrap(),
            )
            .unwrap(),
            "download" => {
                serde_json::to_string(&serde_json::from_str::<ResponseBody>(&fixture.json).unwrap())
                    .unwrap()
            }
            "ready" => serde_json::to_string(
                &serde_json::from_str::<PackageBulkReply>(&fixture.json).unwrap(),
            )
            .unwrap(),
            _ => panic!("unknown frozen contract"),
        };
        assert_eq!(
            actual, fixture.json,
            "wire contract {} changed",
            fixture.kind
        );
    }
}
#[test]
fn control_and_worker_enum_inline_storage_stays_bounded() {
    assert!(std::mem::size_of::<RequestFrame>() <= 128);
    assert!(std::mem::size_of::<ResponseBody>() <= 256);
    assert!(std::mem::size_of::<WorkerOperation>() <= 512);
    assert!(std::mem::size_of::<WorkerOutput>() <= 256);
}
