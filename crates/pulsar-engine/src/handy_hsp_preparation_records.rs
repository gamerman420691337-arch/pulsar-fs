//! Original HSP synchronization intent. These bounded records never restore a
//! native clock, captured credentials, or permission to repeat the effectful GET.
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const HSP_PREPARATION_RECORD_LIMIT: usize = 16 * 1024;
pub const HSP_SYNC_METHOD: &str = "GET";
pub const HSP_SYNC_PATH: &str = "/hstp/clocksync?s=true";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HspPreparationRecord {
    pub boot: DeviceSessionId,
    pub actor_session: SessionId,
    pub request_id: RequestId,
    pub request_fingerprint: String,
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub device_session: DeviceSessionId,
    pub registration: HandyRegistration,
    pub device: HandyIdentity,
    pub mode: HandyDeviceMode,
    /// Exact canonical, validated settings preserve finite values without
    /// pretending an unvalidated floating-point DTO implements Eq.
    pub operative_settings_json: String,
    pub qualification: HandyQualificationBinding,
    pub subject_sha256: String,
    pub profile_sha256: String,
    pub route_sha256: String,
    pub route_namespace_sha256: String,
    pub user_offset_before: HandyUserTimeOffset,
    pub started_boottime_ns: u64,
    pub deadline_boottime_ns: u64,
}
impl HspPreparationRecord {
    pub fn settings(&self) -> Result<HandyOperativeSettings> {
        ensure!(
            self.operative_settings_json.len() <= 8192,
            "HSP settings record exceeds bound"
        );
        let settings: HandyOperativeSettings = serde_json::from_str(&self.operative_settings_json)?;
        settings.validate()?;
        ensure!(
            serde_json::to_string(&settings)? == self.operative_settings_json,
            "HSP settings record is not canonical"
        );
        Ok(settings)
    }
    pub fn validate(&self) -> Result<()> {
        self.registration.validate()?;
        self.device.validate()?;
        self.mode.validate()?;
        self.qualification.validate()?;
        self.user_offset_before.validate()?;
        self.settings()?;
        ensure!(
            !self.registration.disabled && self.boot != self.device_session && self.mode.mode == 4,
            "invalid HSP preparation subject"
        );
        ensure!(
            self.started_boottime_ns > 0
                && self
                    .deadline_boottime_ns
                    .checked_sub(self.started_boottime_ns)
                    .is_some_and(|n| n > 0 && n <= 60_000_000_000),
            "invalid original HSP preparation deadline"
        );
        for value in [
            &self.request_fingerprint,
            &self.subject_sha256,
            &self.profile_sha256,
            &self.route_sha256,
            &self.route_namespace_sha256,
        ] {
            validate_sha256(value)?;
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= HSP_PREPARATION_RECORD_LIMIT,
            "HSP preparation record exceeds bound"
        );
        Ok(())
    }
    pub fn id(&self) -> Result<String> {
        self.validate()?;
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(
                "pulsar.hsp.preparation.v1",
                HSP_SYNC_METHOD,
                HSP_SYNC_PATH,
                self
            ))?)
        ))
    }
    pub fn intent(&self) -> Result<String> {
        self.validate()?;
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(
                "pulsar.hsp.preparation.intent.v1",
                &self.actor_session,
                &self.request_id
            ))?)
        ))
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.validate()?;
        Ok(serde_json::to_vec(self)?)
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= HSP_PREPARATION_RECORD_LIMIT,
            "HSP preparation record exceeds bound"
        );
        let record: Self = serde_json::from_slice(bytes)?;
        record.validate()?;
        Ok(record)
    }
}
