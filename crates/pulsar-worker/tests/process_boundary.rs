use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
fn bounded_output(mut command: Command) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "worker failed to exit within ten seconds: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn companion_rejects_user_commands_and_extra_worker_arguments_without_state() {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("state");
    for args in [
        vec!["engine"],
        vec!["gui"],
        vec!["training-input-plan"],
        vec!["worker", "extra"],
        vec!["wasm-worker", "extra"],
        vec![],
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pulsar-worker"));
        command
            .args(args)
            .env("PULSAR_STATE_DIR", &state)
            .stdin(Stdio::null());
        let output = bounded_output(command);
        assert!(!output.status.success());
        assert!(!state.exists());
    }
    for arg in ["--help", "--version"] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pulsar-worker"));
        command
            .arg(arg)
            .env("PULSAR_STATE_DIR", &state)
            .stdin(Stdio::null());
        let output = bounded_output(command);
        assert!(output.status.success());
        assert!(!state.exists());
    }
}

#[test]
fn missing_engine_worker_fails_before_initializing_state() {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("state");
    let config = pulsar_engine::EngineConfig::new(state.clone(), temp.path().join("absent-worker"));
    assert!(pulsar_engine::Engine::open(config).is_err());
    assert!(!state.exists());
}

#[test]
fn worker_role_consumes_stdin_and_rejects_missing_manifest() {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("state");
    let mut command = Command::new(env!("CARGO_BIN_EXE_pulsar-worker"));
    command
        .arg("worker")
        .env("PULSAR_STATE_DIR", &state)
        .stdin(Stdio::null());
    let output = bounded_output(command);
    assert!(!output.status.success());
    assert!(!state.exists());
    assert!(!String::from_utf8_lossy(&output.stderr)
        .contains("expected an engine-launched worker role"));
}
