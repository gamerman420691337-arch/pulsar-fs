use pulsar_clients::ResponseBody;
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    kind: String,
    json: String,
}

#[test]
fn client_reply_preserves_frozen_download_encoding() {
    let fixtures: Vec<Fixture> = serde_json::from_str(include_str!(
        "../../pulsar-protocol/tests/fixtures/boxed-control-wire.json"
    ))
    .unwrap();
    let fixture = fixtures.into_iter().find(|f| f.kind == "download").unwrap();
    let wire: pulsar_protocol::ResponseBody = serde_json::from_str(&fixture.json).unwrap();
    let pulsar_protocol::ResponseBody::PackageDownload(lease) = wire else {
        panic!("fixture must be a package download");
    };
    let reply = ResponseBody::PackageDownload(lease);
    assert_eq!(serde_json::to_string(&reply).unwrap(), fixture.json);
}

#[test]
fn client_reply_has_bounded_inline_storage() {
    let bytes = std::mem::size_of::<ResponseBody>();
    assert!(bytes <= 256, "client reply occupies {bytes} bytes");
    println!("client_reply_inline_bytes={bytes}");
}
