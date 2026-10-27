//! Process-local absolute lifetime shared by source and model workers.
#![cfg(target_os = "linux")]
use pulsar_protocol::ProtocolError;
use std::time::{Duration, Instant};
pub(crate) fn deadline(
    origin: Instant,
    lease_end: Instant,
    budget_ms: u64,
    now: Instant,
) -> Result<Instant, ProtocolError> {
    if origin > now
        || lease_end <= now
        || lease_end
            .checked_duration_since(origin)
            .is_none_or(|d| d > Duration::from_secs(30))
        || budget_ms == 0
        || budget_ms > 30000
    {
        return Err(ProtocolError::invalid(
            "live worker parent lifetime is invalid or already expired",
        ));
    }
    Ok(lease_end.min(
        now.checked_add(Duration::from_millis(budget_ms))
            .ok_or_else(|| ProtocolError::invalid("live worker deadline overflow"))?,
    ))
}
