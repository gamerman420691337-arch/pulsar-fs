use anyhow::{ensure, Result};
use pulsar_protocol::{ProjectTime, TimeRange, MAX_MOTION_BYTES};
use std::io::Read;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 6,
        "usage: benchmark-motion PROGRAM SHA256 SUBJECT TARGET START_NS END_NS"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(&args[0])?
        .take(MAX_MOTION_BYTES + 1)
        .read_to_end(&mut bytes)?;
    let window = TimeRange::new(
        ProjectTime::from_nanos(args[4].parse()?),
        ProjectTime::from_nanos(args[5].parse()?),
    )?;
    let result =
        pulsar_engine::benchmark::motion_candidate(&bytes, &args[1], &args[2], &args[3], window)?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
