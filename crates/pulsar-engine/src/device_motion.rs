//! Bounded serialization of a device-space proposal. This is not a driver.
use pulsar_protocol::*;
use serde::Serialize;
use std::io::{self, Write};

#[derive(Serialize)]
struct Proposal<'a> {
    format: &'static str,
    project: &'a ProjectId,
    revision: RevisionId,
    master_sha256: &'a str,
    profile_sha256: &'a str,
    profile: &'a DynamicsProfile,
    qualification: &'static str,
    admitted: bool,
    motion: &'a AdaptedMotion,
}

struct BoundedBytes {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other(
                "device-motion export exceeds output limit",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(crate) fn proposal_bytes(
    project: &ProjectId,
    revision: RevisionId,
    master_sha256: &str,
    profile_sha256: &str,
    profile: &DynamicsProfile,
    motion: &AdaptedMotion,
    limit: usize,
) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = BoundedBytes {
        bytes: Vec::new(),
        limit,
    };
    let proposal = Proposal {
        format: "pulsar-device-motion-v1",
        project,
        revision,
        master_sha256,
        profile_sha256,
        profile,
        qualification: "unqualified_proposal",
        admitted: false,
        motion,
    };
    serde_json::to_writer(&mut writer, &proposal)
        .map_err(|error| ProtocolError::new(ErrorCode::ResourceExhausted, error.to_string()))?;
    Ok(writer.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serialized_proposal_is_bounded_before_allocation() {
        let profile = DynamicsProfile {
            axes: vec![AxisDynamics {
                axis: Axis::Stroke,
                unit: "mm".into(),
                minimum: 0.0,
                maximum: 100.0,
                max_velocity: 1.0,
                max_acceleration: 1.0,
                max_jerk: 1.0,
            }],
            coupled: vec![],
            minimum_gain: 0.0,
        };
        let motion = MotionProgram::new(vec![MotionTrack::new(
            Axis::Stroke,
            vec![MotionAction::new(
                ProjectTime::ZERO,
                NormalizedPosition::new(0.5).unwrap(),
                EvidenceKind::Observed,
            )
            .unwrap()],
        )
        .unwrap()])
        .unwrap();
        let adapted = adapt_device_motion(&motion, &profile).unwrap();
        let id = ProjectId::new("p").unwrap();
        assert_eq!(
            proposal_bytes(
                &id,
                RevisionId::new(1),
                "hash",
                "hash",
                &profile,
                &adapted,
                10
            )
            .unwrap_err()
            .code,
            ErrorCode::ResourceExhausted
        );
    }
}
