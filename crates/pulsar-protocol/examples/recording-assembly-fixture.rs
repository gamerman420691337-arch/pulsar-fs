//! Read-only diagnostic harness. Archive bytes never grant Engine authority.
use pulsar_protocol::*;
use std::io::Read;
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let end: i64 = args.next().ok_or("range end required")?.parse()?;
    let maximum: u64 = args
        .next()
        .ok_or("interpolation policy required")?
        .parse()?;
    let mut pages = Vec::new();
    let mut files = 0;
    for path in args {
        files += 1;
        if files > 64 {
            return Err("at most 64 archives".into());
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(32 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("archive exceeds 32 MiB".into());
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        let incoming: Vec<LiveRecordingPage> =
            serde_json::from_value(value.get("pages").ok_or("pages missing")?.clone())?;
        if pages.len() + incoming.len() > 256 {
            return Err("too many pages".into());
        }
        pages.extend(incoming);
    }
    let assembly = assemble_live_recordings(
        &pages,
        TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(end))?,
        maximum,
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "schema":1,"qualified":false,"kind":"diagnostic-live-recording-assembly",
            "program":assembly.program,
            "contains_cuda_provisional":assembly.contains_cuda_provisional,
            "recording_requests":assembly.recording_requests,
            "engine_authority":false,"project_effects":false
        }))?
    );
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("recording assembly: {error}");
        std::process::exit(1);
    }
}
