//! Explicit disposable IPC qualification host; never reads the user's default state.
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() == 3 && args[1] == "--emit-trap" {
        use std::io::Write;
        let path = std::path::PathBuf::from(&args[2]);
        anyhow::ensure!(path.is_absolute(), "explicit absolute output required");
        let module = wat::parse_str("(module (memory (export \"memory\") 1) (func (export \"pulsar_alloc\") (param i32) (result i32) i32.const 4096) (func (export \"pulsar_process\") (param i32 i32) (result i64) unreachable))")?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(&module)?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--emit-module" {
        use pulsar_core::{
            Axis, EditValueAction, EditValueTrack, EditValuesProgram, NormalizedPosition,
            ProjectTime,
        };
        use std::io::Write;
        let path = std::path::PathBuf::from(&args[2]);
        anyhow::ensure!(path.is_absolute(), "explicit absolute output required");
        let values = EditValuesProgram::new(vec![EditValueTrack::new(
            Axis::Stroke,
            vec![
                EditValueAction::new(ProjectTime::ZERO, NormalizedPosition::new(0.2)?)?,
                EditValueAction::new(
                    ProjectTime::from_nanos(1_000_000_000),
                    NormalizedPosition::new(0.8)?,
                )?,
            ],
            vec![],
        )?])?;
        let output = serde_json::to_vec(&values)?;
        let escaped = output
            .iter()
            .map(|b| format!("\\{b:02x}"))
            .collect::<String>();
        let module=wat::parse_str(format!("(module (memory (export \"memory\") 1) (data (i32.const 0) \"{escaped}\") (func (export \"pulsar_alloc\") (param i32) (result i32) i32.const 4096) (func (export \"pulsar_process\") (param i32 i32) (result i64) i64.const {}))",output.len()))?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(&module)?;
        file.sync_all()?;
        return Ok(());
    }
    anyhow::ensure!(
        args.len() == 3,
        "qualification host requires exact state and worker paths"
    );
    let state = std::path::PathBuf::from(&args[1]);
    let worker = std::path::PathBuf::from(&args[2]);
    anyhow::ensure!(
        state.is_absolute() && worker.is_absolute(),
        "explicit absolute paths required"
    );
    pulsar_engine::run_server(pulsar_engine::EngineConfig::new(state, worker))
}
