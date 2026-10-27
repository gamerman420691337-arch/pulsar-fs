//! Bounded stdin ingress. No source acquisition, native launch or device authority.
use crate::SessionClient;
#[cfg(unix)]
use anyhow::ensure;
use anyhow::Result;
use pulsar_protocol::*;
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(not(unix))]
pub(crate) fn run(
    _client: &mut SessionClient,
    _project: ProjectId,
    _revision: RevisionId,
    _epoch: AttemptId,
) -> Result<LiveCaptureReport> {
    anyhow::bail!("piped encoded ingress unsupported on this platform")
}

/// Use the beginning of the status exchange so this client deadline cannot
/// extend the server lease by its reply transit time. Equal-rate local clocks
/// remain an assumption; fresh engine authority still fences every effect.
#[cfg(unix)]
fn lease_deadline(report: &LiveCaptureReport, sent: Instant) -> Result<Instant> {
    report.validate()?;
    ensure!(report.active, "Encoded capture retired");
    let remaining = Duration::from_millis(u64::from(report.options.lease_ms))
        .checked_sub(Duration::from_nanos(
            report.server_elapsed.expect("validated").0,
        ))
        .ok_or_else(|| anyhow::anyhow!("Encoded capture expired"))?;
    let end = sent
        .checked_add(remaining)
        .ok_or_else(|| anyhow::anyhow!("Encoded lease deadline overflow"))?;
    ensure!(
        Instant::now() < end,
        "Encoded capture expired during status exchange"
    );
    Ok(end)
}

#[cfg(unix)]
pub(crate) fn run(
    client: &mut SessionClient,
    project: ProjectId,
    revision: RevisionId,
    epoch: AttemptId,
) -> Result<LiveCaptureReport> {
    use rustix::event::{poll, PollFd, PollFlags, Timespec};
    use std::{fs::File, io::Read, os::unix::fs::FileTypeExt};
    // Duplicate, validate and own the sole input reader without changing
    // shared open-file flags or using buffered stdin that may prefetch bytes.
    let mut input = File::from(rustix::io::fcntl_dupfd_cloexec(std::io::stdin(), 0)?);
    let metadata = input.metadata()?;
    ensure!(
        metadata.is_file() || metadata.file_type().is_fifo(),
        "Encoded stdin must be a pipe or regular file; interactive/device/socket input refused"
    );
    let sent = Instant::now();
    let initial = client.capture_status_until(
        project.clone(),
        revision,
        epoch.clone(),
        sent + Duration::from_secs(10),
    )?;
    let maximum = initial
        .options
        .encoded_source
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Capture did not admit encoded input"))?
        .maximum_input_bytes;
    ensure!(
        !metadata.is_file() || metadata.len() > 0 && metadata.len() <= maximum,
        "Encoded stdin file exceeds admission or is empty"
    );
    let mut deadline = lease_deadline(&initial, sent)?;
    let mut bulk = client.open_live_encoded_until(&initial, deadline)?;
    let mut total = 0u64;
    let mut chunk = [0u8; 65536];
    let mut next_status = Instant::now();
    loop {
        ensure!(
            Instant::now() < deadline,
            "Encoded capture expired while waiting for producer"
        );
        if Instant::now() >= next_status {
            let sent = Instant::now();
            let current = client.capture_status_until(
                project.clone(),
                revision,
                epoch.clone(),
                deadline.min(sent + Duration::from_secs(1)),
            )?;
            ensure!(
                current.identity == initial.identity
                    && current.engine_epoch == initial.engine_epoch,
                "Encoded capture scope changed"
            );
            deadline = deadline.min(lease_deadline(&current, sent)?);
            bulk.limit_deadline(deadline)?;
            next_status = Instant::now() + Duration::from_millis(250);
        }
        let wait = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(10));
        let timeout = Timespec {
            tv_sec: 0,
            tv_nsec: wait.as_nanos() as _,
        };
        let events = {
            let mut fds = [PollFd::new(&input, PollFlags::IN)];
            match poll(&mut fds, Some(&timeout)) {
                Ok(0) => continue,
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(error.into()),
                Ok(_) => {}
            }
            fds[0].revents()
        };
        ensure!(
            !events.intersects(PollFlags::ERR | PollFlags::NVAL),
            "Encoded stdin poll failed"
        );
        // Read at most one byte past the admission to distinguish exact EOF
        // from an oversized stream. Excess bytes are never forwarded.
        let length = usize::try_from((maximum - total + 1).min(chunk.len() as u64))?;
        let n = match input.read(&mut chunk[..length]) {
            Ok(n) => n,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                ) =>
            {
                continue
            }
            Err(e) => return Err(e.into()),
        };
        if n == 0 {
            ensure!(total > 0, "Encoded stdin is empty");
            return Ok(bulk.finish()?);
        }
        ensure!(
            n as u64 <= maximum - total,
            "Encoded stdin exceeds admitted byte limit"
        );
        ensure!(
            Instant::now() < deadline,
            "Encoded capture expired before forwarding producer bytes"
        );
        bulk.push(&chunk[..n])?;
        total += n as u64;
    }
}
