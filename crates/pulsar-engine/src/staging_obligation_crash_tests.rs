//! File-journal fixture for actual process-death cuts. This adapter is not the
//! production fence format, database barrier or portable representation.
use super::*;
use std::{
    io::Write,
    os::unix::process::ExitStatusExt,
    process::Command,
    time::{Duration, Instant},
};

const ROOT: &str = "PULSAR_OBLIGATION_CRASH_ROOT";
const CUT: &str = "PULSAR_OBLIGATION_CRASH_CUT";
const CASE: &str = "staging_obligation::crash_tests::native_creation_and_cleanup_crash_cuts";

fn persist(root: &Path, record: Option<&Record>) -> Result<()> {
    let bytes = serde_json::to_vec(&record)?;
    ensure!(bytes.len() <= 4096, "oversized fixture record");
    let mut next = tempfile::NamedTempFile::new_in(root)?;
    next.write_all(&bytes)?;
    next.as_file().sync_all()?;
    next.persist(root.join("fixture-journal.json"))?;
    directory(root)?.sync_all()?;
    Ok(())
}
fn parked(root: &Path, path: &Path, cut: &str) {
    fs::write(
        root.join("ready.json"),
        serde_json::to_vec(&serde_json::json!({"path":path,"cut":cut})).unwrap(),
    )
    .unwrap();
    std::thread::sleep(Duration::from_secs(60));
    panic!("parent did not perform the crash cut");
}
fn owner(root: &Path, cut: &str) {
    persist(root, None).unwrap();
    let mut last = PathBuf::new();
    let mut publish = |record: Option<&Record>| -> Result<()> {
        if let Some(record) = record {
            last = record.parent.join(&record.name);
        }
        let phase = match record.map(|r| &r.phase) {
            Some(Phase::Intent) => "intent",
            Some(Phase::Bound(_)) => "identity",
            Some(Phase::Retired(_)) => "retirement",
            None => "clear",
        };
        if cut == format!("{phase}_pending") {
            parked(root, &last, cut);
        }
        persist(root, record)?;
        if cut == format!("{phase}_acked") {
            parked(root, &last, cut);
        }
        Ok(())
    };
    let mut owned = create(root, &mut publish).unwrap();
    fs::write(owned.path().join("bytes"), b"authored fixture payload").unwrap();
    owned.finish_after_use_retired(&mut publish).unwrap();
    panic!("requested cut was not reached");
}
struct Process(std::process::Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "actual owned-process termination; run explicitly serially"]
fn native_creation_and_cleanup_crash_cuts() {
    if let Some(root) = std::env::var_os(ROOT) {
        owner(Path::new(&root), &std::env::var(CUT).unwrap());
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    for cut in [
        "intent_pending",
        "intent_acked",
        "identity_pending",
        "identity_acked",
        "retirement_pending",
        "retirement_acked",
        "clear_pending",
        "clear_acked",
    ] {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let mut process = Process(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    CASE,
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(ROOT, root.path())
                .env(CUT, cut)
                .spawn()
                .unwrap(),
        );
        let end = Instant::now() + Duration::from_secs(10);
        let ready: serde_json::Value = loop {
            if let Ok(bytes) = fs::read(root.path().join("ready.json")) {
                if let Ok(ready) = serde_json::from_slice(&bytes) {
                    break ready;
                }
            }
            assert!(
                process.0.try_wait().unwrap().is_none(),
                "owner exited before {cut}"
            );
            assert!(Instant::now() < end, "owner did not reach {cut}");
            std::thread::sleep(Duration::from_millis(10));
        };
        let path = PathBuf::from(ready["path"].as_str().unwrap());
        assert!(path.starts_with(root.path()));
        assert_eq!(ready["cut"], cut);
        process.0.kill().unwrap();
        assert_eq!(process.0.wait().unwrap().signal(), Some(libc::SIGKILL));
        let raw = fs::read(root.path().join("fixture-journal.json")).unwrap();
        assert!(raw.len() <= 4096);
        let record: Option<Record> = serde_json::from_slice(&raw).unwrap();
        assert_eq!(serde_json::to_vec(&record).unwrap(), raw);
        let expected_phase = match cut {
            "intent_acked" | "identity_pending" => "intent",
            "identity_acked" | "retirement_pending" => "identity",
            "retirement_acked" | "clear_pending" => "retirement",
            "intent_pending" | "clear_acked" => "clear",
            _ => unreachable!(),
        };
        match (&record, expected_phase) {
            (
                Some(Record {
                    phase: Phase::Intent,
                    ..
                }),
                "intent",
            )
            | (
                Some(Record {
                    phase: Phase::Bound(_),
                    ..
                }),
                "identity",
            )
            | (
                Some(Record {
                    phase: Phase::Retired(_),
                    ..
                }),
                "retirement",
            )
            | (None, "clear") => {}
            other => panic!("unexpected retained state for {cut}: {other:?}"),
        }
        if matches!(cut, "retirement_pending" | "retirement_acked") {
            assert_eq!(
                fs::read(path.join("bytes")).unwrap(),
                b"authored fixture payload"
            );
        }
        if let Some(record) = &record {
            let result = reconcile_after_use_retired(record, &mut |r| persist(root.path(), r));
            if cut == "identity_pending" {
                assert!(result.is_err());
                assert!(path.exists());
                assert_eq!(
                    fs::read(root.path().join("fixture-journal.json")).unwrap(),
                    raw
                );
                eprintln!("{cut}: SIGKILL confirmed, ambiguous created root preserved, admission reconciliation refused");
                continue;
            }
            result.unwrap();
        }
        assert!(!path.exists(), "owned root was not reconciled for {cut}");
        assert_eq!(
            fs::read(root.path().join("fixture-journal.json")).unwrap(),
            b"null"
        );
        eprintln!(
            "{cut}: SIGKILL confirmed, owned root absent, durable fixture obligation cleared"
        );
    }
}
