//! Owned sterile recordings from the rendered continuation-boundary refusal.
//! This checks assembly of retained inputs, not model or device qualification.
use pulsar_protocol::{assemble_live_recordings, LiveRecordingPage, ProjectTime, TimeRange};
use sha2::{Digest, Sha256};
use std::io::Read;

#[derive(serde::Deserialize)]
struct Archive {
    pages: Vec<LiveRecordingPage>,
    qualified: bool,
}

fn read_pinned(name: &str, expected: &str) -> Archive {
    let path = std::env::var_os(name).expect("set the owned sterile recording path");
    let file = std::fs::File::open(path).expect("open owned recording");
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .expect("read owned recording");
    assert!(
        bytes.len() <= 1024 * 1024,
        "recording exceeds fixture bound"
    );
    assert_eq!(format!("{:x}", Sha256::digest(&bytes)), expected);
    let archive: Archive = serde_json::from_slice(&bytes).expect("decode recording");
    assert!(!archive.qualified);
    archive
}

#[test]
#[ignore = "requires the two pinned owned sterile recording artifacts"]
fn retained_rendered_duplicate_boundary_assembles_without_bridging() {
    let mut first = read_pinned(
        "PULSAR_TEST_RECORDING_ONE",
        "a2987f784eff1a602af02678a7a05970752e9c304d517d4ab80afbe490d7965e",
    );
    let second = read_pinned(
        "PULSAR_TEST_RECORDING_TWO",
        "f24309fff92fb7aaa9774ce11c8307c2ad74a7b5eea863a0e8a0db44c6119c2a",
    );
    first.pages.extend(second.pages);
    let range = TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(60_000_000_000)).unwrap();
    let result = assemble_live_recordings(&first.pages, range, 200_000_000).unwrap();
    assert_eq!(result.recording_requests.len(), 2);
    let track = &result.program.tracks()[0];
    assert_eq!(track.actions().len(), 171);
    assert!(track.gaps().iter().any(|gap| {
        gap.start() == ProjectTime::from_nanos(22_000_000_001)
            && gap.end() == ProjectTime::from_nanos(22_100_000_000)
    }));
}
