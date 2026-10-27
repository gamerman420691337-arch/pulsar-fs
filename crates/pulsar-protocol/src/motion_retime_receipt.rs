//! An authored clock-change receipt, never media or physical qualification.
use crate::*;
/// Verified historical facts and their destination correspondence. Original
/// sessions/requests/actors remain archival and acquire no local authority.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchivedMotionRetimeReceipt {
    pub schema_version: u16,
    pub project_id: ProjectId,
    pub base_revision: RevisionId,
    pub revision: RevisionId,
    pub from: SourcePlacement,
    pub to: SourcePlacement,
    pub local_actor: String,
    pub origin_manifest: PackageObjectRef,
    pub origin_receipt: MotionRetimeReceipt,
    pub historical_attribution_only: bool,
}
impl ArchivedMotionRetimeReceipt {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.origin_receipt.validate()?;
        self.origin_manifest.validate()?;
        if self.schema_version != 1
            || !self.historical_attribution_only
            || self.project_id == self.origin_receipt.project_id
            || self.base_revision.checked_next().ok() != Some(self.revision)
            || self.from.spec != self.origin_receipt.from.spec
            || self.to.spec != self.origin_receipt.to.spec
            || self.from.placement_id == self.to.placement_id
            || self.from.placement_id == self.origin_receipt.from.placement_id
            || self.to.placement_id == self.origin_receipt.to.placement_id
            || self.from.created_revision > self.base_revision
            || self.to.created_revision > self.base_revision
            || !self.local_actor.starts_with("archive:")
            || self.local_actor.len() > 256
            || self.origin_manifest.byte_len > MAX_PROJECT_PACKAGE_MANIFEST_BYTES as u64
            || !self
                .local_actor
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
        {
            return Err(ProtocolError::invalid(
                "invalid archival retiming correspondence",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionRetimeReceipt {
    pub schema_version: u16,
    pub project_id: ProjectId,
    pub base_revision: RevisionId,
    pub revision: RevisionId,
    pub session_id: SessionId,
    pub request_id: RequestId,
    pub actor: String,
    pub operation_sha256: String,
    pub from: SourcePlacement,
    pub to: SourcePlacement,
    pub axes: Vec<Axis>,
    pub before: ArtifactIdentity,
    pub after: ArtifactIdentity,
    pub recorded_at_unix_ms: u64,
}
impl MotionRetimeReceipt {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.from
            .spec
            .validate()
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        self.to
            .spec
            .validate()
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        self.before.validate()?;
        self.after.validate()?;
        crate::validate_sha256(&self.operation_sha256)?;
        if self.schema_version != 1
            || self.base_revision.checked_next().ok() != Some(self.revision)
            || self.from.created_revision > self.base_revision
            || self.to.created_revision > self.base_revision
            || self.from.placement_id == self.to.placement_id
            || self.from.spec.source_version != self.to.spec.source_version
            || self.axes.is_empty()
            || self.axes.len() > 6
            || self
                .axes
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.axes.len()
            || self.recorded_at_unix_ms == 0
            || self.actor.is_empty()
            || self.actor.len() > 128
            || !self
                .actor
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
            || self.before.artifact_id.as_str() != format!("motion.{}", self.before.sha256)
            || self.after.artifact_id.as_str() != format!("motion.{}", self.after.sha256)
        {
            return Err(ProtocolError::invalid(
                "invalid authored motion-retiming receipt",
            ));
        }
        Ok(())
    }
}
