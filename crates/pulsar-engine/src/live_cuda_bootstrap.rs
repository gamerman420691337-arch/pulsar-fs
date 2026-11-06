//! A sealed live-only loader relay. No offline request or renewed lease.
use super::*;
use crate::cuda_mounts::{verify_live_bootstrap, LiveCudaMountWitness};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LiveRole {
    MemoryObservation,
    Inference,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LiveRelay {
    schema_version: u16,
    role: LiveRole,
    pub(crate) witness: LiveCudaMountWitness,
}
impl LiveRelay {
    fn validate(&self, expected: LiveRole) -> Result<()> {
        ensure!(
            self.schema_version == 1 && self.role == expected,
            "CUDA live sealed relay version or role differs"
        );
        self.witness.manifest.validate()?;
        ensure!(
            self.witness.manifest_sha256 == self.witness.manifest.identity_sha256()?,
            "CUDA live relay manifest differs"
        );
        deadline_instant(
            self.witness.deadline_ns,
            self.witness.manifest.stream.budget.wall_time_ms,
        )?;
        Ok(())
    }
}
pub(crate) fn reexec_live_memory(witness: LiveCudaMountWitness) -> Result<()> {
    reexec_live(witness, LiveRole::MemoryObservation)
}
#[cfg(feature = "cuda")]
pub(crate) fn reexec_live_stream(witness: LiveCudaMountWitness) -> Result<()> {
    reexec_live(witness, LiveRole::Inference)
}
fn reexec_live(witness: LiveCudaMountWitness, role: LiveRole) -> Result<()> {
    // The inherited end is checked before and after the full mounted-byte hash;
    // loader transition never creates a fresh budget.
    verify_live_bootstrap(&witness)?;
    let relay = LiveRelay {
        schema_version: 1,
        role,
        witness,
    };
    relay.validate(role)?;
    transition(
        &relay.witness.manifest.execution.library_directory,
        match role {
            LiveRole::MemoryObservation => "cuda-live-memory-observer-exec",
            LiveRole::Inference => "cuda-live-worker-exec",
        },
        &relay,
    )
}
pub(crate) fn read_live_relay() -> Result<LiveRelay> {
    read_live_relay_for(LiveRole::MemoryObservation)
}
#[cfg(feature = "cuda")]
pub(crate) fn read_live_stream_relay() -> Result<LiveRelay> {
    read_live_relay_for(LiveRole::Inference)
}
fn read_live_relay_for(expected: LiveRole) -> Result<LiveRelay> {
    let bytes = read_relay_bytes()?;
    let relay: LiveRelay = serde_json::from_slice(&bytes)?;
    relay.validate(expected)?;
    Ok(relay)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_stream_relay_is_not_a_memory_observation_relay() {
        let relay = LiveRelay {
            schema_version: 1,
            role: LiveRole::Inference,
            witness: crate::cuda_mounts::live_test_witness(),
        };
        relay.validate(LiveRole::Inference).unwrap();
        assert!(relay.validate(LiveRole::MemoryObservation).is_err());
        let bytes = serde_json::to_vec(&relay).unwrap();
        assert!(serde_json::from_slice::<Relay>(&bytes).is_err());
        let relay: LiveRelay = serde_json::from_slice(&bytes).unwrap();
        relay.validate(LiveRole::Inference).unwrap();
    }
    #[test]
    fn sealed_live_relay_has_separate_wire_and_never_renews_deadline() {
        let witness = crate::cuda_mounts::live_test_witness();
        let end = witness.deadline_ns;
        let relay = LiveRelay {
            schema_version: 1,
            role: LiveRole::MemoryObservation,
            witness,
        };
        relay.validate(LiveRole::MemoryObservation).unwrap();
        let bytes = serde_json::to_vec(&relay).unwrap();
        assert!(serde_json::from_slice::<Relay>(&bytes).is_err());
        let decoded: LiveRelay = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.witness.deadline_ns, end);
        decoded.validate(LiveRole::MemoryObservation).unwrap();
        let value = serde_json::to_value(decoded).unwrap();
        for (pointer, replacement) in [
            ("/schema_version", serde_json::json!(2)),
            ("/role", serde_json::json!("inference")),
            ("/witness/deadline_ns", serde_json::json!(0)),
            (
                "/witness/manifest_sha256",
                serde_json::json!("0".repeat(64)),
            ),
        ] {
            let mut bad = value.clone();
            *bad.pointer_mut(pointer).unwrap() = replacement;
            if let Ok(decoded) = serde_json::from_value::<LiveRelay>(bad) {
                assert!(
                    decoded.validate(LiveRole::MemoryObservation).is_err(),
                    "{pointer}"
                );
            }
        }
        let mut extra = value;
        extra["approved"] = serde_json::json!(true);
        assert!(serde_json::from_value::<LiveRelay>(extra).is_err());
    }
}
