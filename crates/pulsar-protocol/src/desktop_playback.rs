//! Ephemeral, host-owned Desktop player controls; no project mutation or model actions.
use crate::*;

pub const DESKTOP_SYNC_OFFSET_LIMIT_MS: i16 = 2_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum DesktopPlaybackAction {
    Open {
        video: PathBuf,
        script: PathBuf,
    },
    OpenMedia {
        video: PathBuf,
    },
    /// Host-owned playback of the exact bytes identified during media review.
    OpenReviewedMedia {
        video: PathBuf,
        sha256: String,
    },
    PrepareMediaReview {
        video: PathBuf,
        consent: PathBuf,
    },
    ConfirmMediaReview {
        review: EffectReview,
    },
    Arm {
        enabled: bool,
        max_speed: u16,
        min_pct: u8,
        max_pct: u8,
    },
    ArmAssistant {
        project: ProjectId,
        consent: PathBuf,
        max_speed: u16,
        min_pct: u8,
        max_pct: u8,
        sync_offset_ms: i16,
    },
    AssistantPlay {
        grant: SessionId,
        project: ProjectId,
        consent: PathBuf,
    },
    Play {
        /// Negative advances the Handy; positive delays it.
        #[serde(default, skip_serializing_if = "is_zero_offset")]
        sync_offset_ms: i16,
    },
    Pause,
    Seek {
        position_ms: u64,
    },
    Stop,
    Status,
    Close,
}

fn is_zero_offset(offset: &i16) -> bool {
    *offset == 0
}

fn validate_local_path(path: &std::path::Path) -> Result<(), ProtocolError> {
    if !path.is_absolute() || path.as_os_str().len() > 4096 {
        return Err(ProtocolError::invalid(
            "player needs bounded absolute local paths",
        ));
    }
    Ok(())
}

impl DesktopPlaybackAction {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::Open { video, script } => {
                validate_local_path(video)?;
                validate_local_path(script)?;
            }
            Self::OpenMedia { video } => validate_local_path(video)?,
            Self::OpenReviewedMedia { video, sha256 } => {
                validate_local_path(video)?;
                if sha256.len() != 64
                    || !sha256
                        .bytes()
                        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                {
                    return Err(ProtocolError::invalid(
                        "reviewed video needs a lowercase SHA-256 digest",
                    ));
                }
            }
            Self::PrepareMediaReview { video, consent } => {
                validate_local_path(video)?;
                validate_local_path(consent)?;
            }
            Self::AssistantPlay { consent, .. } => validate_local_path(consent)?,
            Self::ArmAssistant {
                consent,
                max_speed,
                min_pct,
                max_pct,
                sync_offset_ms,
                ..
            } => {
                validate_local_path(consent)?;
                Self::Arm {
                    enabled: true,
                    max_speed: *max_speed,
                    min_pct: *min_pct,
                    max_pct: *max_pct,
                }
                .validate()?;
                Self::Play {
                    sync_offset_ms: *sync_offset_ms,
                }
                .validate()?;
            }
            Self::Arm {
                enabled: true,
                max_speed,
                min_pct,
                max_pct,
            } => {
                if !(1..=300).contains(max_speed)
                    || min_pct >= max_pct
                    || *max_pct > 100
                    || max_pct - min_pct > 70
                {
                    return Err(ProtocolError::invalid(
                        "player limits require speed 1–300 and a range no wider than 70%",
                    ));
                }
            }
            Self::Seek { position_ms } if *position_ms > 86_400_000 => {
                return Err(ProtocolError::invalid("player seek exceeds 24 hours"));
            }
            Self::Play { sync_offset_ms }
                if sync_offset_ms.unsigned_abs() > DESKTOP_SYNC_OFFSET_LIMIT_MS as u16 =>
            {
                return Err(ProtocolError::invalid(
                    "player sync offset must be within ±2000 ms",
                ));
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopMediaReview {
    pub review: EffectReview,
    pub consent_path: PathBuf,
    pub video_name: String,
    pub sha256: String,
    pub byte_len: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopPlaybackStatus {
    pub loaded: bool,
    pub armed: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub limits_applied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_grant: Option<SessionId>,
    pub playing: bool,
    pub connected: bool,
    pub busy: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub expires_in_ms: u64,
    pub message: String,
    pub video_name: String,
    pub script_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_review: Option<DesktopMediaReview>,
}

pub(crate) fn validate_request(request: &Request) -> Result<(), ProtocolError> {
    if let Command::DesktopPlayback {
        action,
        owner_proof,
    } = &request.command
    {
        if request.project.is_some() || request.expected_revision.is_some() {
            return Err(ProtocolError::invalid(
                "Desktop player is not project-scoped",
            ));
        }
        bounded_text(owner_proof, 64, "host-local player owner capability")?;
        action.validate()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn video_only_open_rejects_urls_and_script_arguments() {
        let url = DesktopPlaybackAction::OpenMedia {
            video: "https://example.test/video.mp4".into(),
        };
        assert!(url.validate().is_err());
        assert!(
            serde_json::from_value::<DesktopPlaybackAction>(serde_json::json!({
                "action": "open_media", "video": "video.mp4", "script": "video.funscript"
            }))
            .is_err()
        );
    }

    #[test]
    fn play_offset_is_signed_bounded_and_defaults_to_zero_on_the_wire() {
        let old_play = serde_json::json!({"action": "play"});
        let decoded: DesktopPlaybackAction = serde_json::from_value(old_play.clone()).unwrap();
        assert!(matches!(
            decoded,
            DesktopPlaybackAction::Play { sync_offset_ms: 0 }
        ));
        assert_eq!(serde_json::to_value(decoded).unwrap(), old_play);
        for sync_offset_ms in [-2_000, 2_000] {
            assert!(DesktopPlaybackAction::Play { sync_offset_ms }
                .validate()
                .is_ok());
        }
        for sync_offset_ms in [-2_001, 2_001, i16::MIN] {
            assert!(DesktopPlaybackAction::Play { sync_offset_ms }
                .validate()
                .is_err());
        }
    }

    #[test]
    fn limits_and_paths_are_checked_without_effects() {
        assert!(DesktopPlaybackAction::Arm {
            enabled: true,
            max_speed: 150,
            min_pct: 35,
            max_pct: 65
        }
        .validate()
        .is_ok());
        assert!(DesktopPlaybackAction::Arm {
            enabled: true,
            max_speed: 301,
            min_pct: 0,
            max_pct: 100
        }
        .validate()
        .is_err());
        assert!(DesktopPlaybackAction::Open {
            video: "relative.mp4".into(),
            script: "relative.funscript".into()
        }
        .validate()
        .is_err());
        assert!(DesktopPlaybackAction::Seek {
            position_ms: 86_400_001
        }
        .validate()
        .is_err());
    }
}
