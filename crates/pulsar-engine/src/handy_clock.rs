//! Shared suspend-aware host clock for Handy leases and physical handoff.
use pulsar_protocol::{ErrorCode, ProtocolError};

pub(crate) fn boottime_ns() -> Result<u64, ProtocolError> {
    #[cfg(target_os = "linux")]
    {
        let mut value = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut value) } != 0
            || value.tv_sec < 0
            || !(0..1_000_000_000).contains(&value.tv_nsec)
        {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "suspend-aware controller clock unavailable",
            ));
        }
        (value.tv_sec as u64)
            .checked_mul(1_000_000_000)
            .and_then(|v| v.checked_add(value.tv_nsec as u64))
            .ok_or_else(|| ProtocolError::new(ErrorCode::Internal, "controller clock overflow"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(ProtocolError::unsupported(
            "controller leases require a qualified suspend-aware platform clock",
        ))
    }
}
