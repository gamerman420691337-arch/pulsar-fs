//! Exported authored journal fixtures, never production or physical acceptance.
use super::*;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn native_fixture_hash(path: &Path) -> String {
    let mut file = std::fs::File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut bytes = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut bytes).unwrap();
        if count == 0 {
            break;
        }
        hash.update(&bytes[..count]);
    }
    format!("{:x}", hash.finalize())
}

fn fresh_native_fixture_directory() -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os("PULSAR_BROWSER_JOURNAL_FIXTURE_DIR")
            .expect("set PULSAR_BROWSER_JOURNAL_FIXTURE_DIR to a fresh absolute output directory"),
    );
    assert!(path.is_absolute());
    assert!(path.components().all(|part| matches!(
        part,
        std::path::Component::RootDir | std::path::Component::Normal(_)
    )));
    let parent = path.parent().unwrap();
    assert_eq!(parent.canonicalize().unwrap(), parent);
    assert!(
        path.starts_with("/mnt/4tb/tmp") || path.starts_with("/mnt/4tb/artifacts"),
        "fixture output must be in the owned scratch or artifact area"
    );
    std::fs::create_dir(&path).expect("fixture output must not already exist");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    path
}

fn write_native_fixture_file(path: &Path, bytes: &[u8]) {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path).unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

#[test]
#[ignore = "exports an explicitly authored journal for a separately supervised v8/v10 reader campaign"]
fn export_populated_browser_journal_for_native_reader_campaign() {
    let output = fresh_native_fixture_directory();
    let mut fixture = Fixture::new().browser();
    let prepared = fixture.prepare();
    let claim = fixture.journal.claim_play(prepared).unwrap();
    fixture
        .journal
        .complete_play_operation(&claim, DispatchOutcome::Acknowledged)
        .unwrap();
    let history = fixture.history();
    assert_eq!(history.state, PlayOperationState::Acknowledged);
    assert!(history.record.browser_start.is_some());
    let record: String = fixture
        .journal
        .db
        .query_row(
            "SELECT record FROM play_operations WHERE id=?1",
            [&history.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        record.as_bytes(),
        serde_json::to_vec(&fixture.record).unwrap()
    );
    let (decision_sha256, membership_sha256): (String, String) = fixture
        .journal
        .db
        .query_row(
            "SELECT decision_sha256,membership_sha256 FROM play_reference_members WHERE operation_id=?1",
            [&history.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let decision = serde_json::to_vec(fixture.record.browser_start.as_ref().unwrap()).unwrap();
    assert_eq!(decision_sha256, digest(&decision));
    let journal = output.join("journal.sqlite3");
    fixture
        .journal
        .db
        .execute("VACUUM INTO ?1", [journal.to_str().unwrap()])
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&journal, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    std::fs::File::open(&journal).unwrap().sync_all().unwrap();
    let snapshot =
        Connection::open_with_flags(&journal, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let journal_schema_version = DeviceEffectJournal::validate_existing(&snapshot).unwrap();
    assert_eq!(journal_schema_version, 10);
    let copied: String = snapshot
        .query_row(
            "SELECT record FROM play_operations WHERE id=?1",
            [&history.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(copied, record);
    drop(snapshot);
    write_native_fixture_file(&output.join("original-record.json"), record.as_bytes());
    write_native_fixture_file(&output.join("original-decision.json"), &decision);
    let image = std::env::current_exe().unwrap();
    let metadata = serde_json::json!({
        "schema_version": 1,
        "synthetic_fixture": true,
        "qualified": false,
        "production_acceptance_created": false,
        "real_device_contacted": false,
        "physical_effect_observed": false,
        "limitation": "Fixture::new().browser() contains authored Physical and LocalHumanAcceptance labels solely to exercise journal invariants. They are not measurements, production acceptance, or human authorization. Acknowledged is an injected fixture outcome; no transport ran.",
        "creation_path": "Fixture::new().browser -> prepare_play -> claim_play -> complete_play_operation(Acknowledged) -> VACUUM INTO",
        "journal_schema_version": journal_schema_version,
        "record_id": history.id,
        "record_sha256": digest(record.as_bytes()),
        "record_bytes": record.len(),
        "decision_sha256": decision_sha256,
        "membership_sha256": membership_sha256,
        "membership_source": "read from the transaction produced by DeviceEffectJournal; not synthesized by the exporter",
        "journal_sha256": native_fixture_hash(&journal),
        "journal_bytes": journal.metadata().unwrap().len(),
        "source_test_image": image,
        "source_test_image_sha256": native_fixture_hash(&image),
    });
    write_native_fixture_file(
        &output.join("fixture.json"),
        &serde_json::to_vec_pretty(&metadata).unwrap(),
    );
    println!(
        "authored browser journal fixture exported to {}",
        output.display()
    );
}
