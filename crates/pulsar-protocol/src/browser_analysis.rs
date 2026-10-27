//! Declared two-reader browser capture selection. These values carry no native
//! player, source, device or physical timing authority.
use crate::{ArtifactId, HandyPlayerIdentity, LiveSourceTimeBasis, ProtocolError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserAnalysisLookaheadPolicy {
    pub target_lead_us: u64,
    pub minimum_lead_us: u64,
    pub maximum_lead_us: u64,
}
impl BrowserAnalysisLookaheadPolicy {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.minimum_lead_us == 0
            || self.minimum_lead_us > self.target_lead_us
            || self.target_lead_us > self.maximum_lead_us
            || self.maximum_lead_us > 30_000_000
        {
            return Err(ProtocolError::invalid(
                "invalid browser analysis lookahead limits",
            ));
        }
        Ok(())
    }
}

/// A native client declares which visible player an analysis feed accompanies.
/// The Engine must resolve the timing ID to its current native player/source
/// lifetime; copying this DTO cannot reconstruct that lifetime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserAnalysisCaptureDeclaration {
    pub feed_id: ArtifactId,
    pub player: HandyPlayerIdentity,
    pub timing_binding_id: ArtifactId,
    pub policy: BrowserAnalysisLookaheadPolicy,
}
impl BrowserAnalysisCaptureDeclaration {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.player.validate()?;
        self.policy.validate()?;
        if self.player.time_basis != LiveSourceTimeBasis::SourceMediaPresentation {
            return Err(ProtocolError::invalid(
                "browser analysis requires the selected source-media player",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lookahead_refuses_zero_reversed_and_unbounded_lead() {
        let valid = BrowserAnalysisLookaheadPolicy {
            minimum_lead_us: 1,
            target_lead_us: 1_000_000,
            maximum_lead_us: 30_000_000,
        };
        valid.validate().unwrap();
        for invalid in [
            BrowserAnalysisLookaheadPolicy {
                minimum_lead_us: 0,
                ..valid
            },
            BrowserAnalysisLookaheadPolicy {
                minimum_lead_us: 1_000_001,
                ..valid
            },
            BrowserAnalysisLookaheadPolicy {
                maximum_lead_us: 999_999,
                ..valid
            },
            BrowserAnalysisLookaheadPolicy {
                maximum_lead_us: 30_000_001,
                ..valid
            },
        ] {
            assert!(invalid.validate().is_err());
        }
    }
    #[test]
    fn analysis_declaration_refuses_other_capture_roles_and_unknown_authority_fields() {
        let mut value = serde_json::json!({
            "feed_id":"feed", "timing_binding_id":"timing",
            "player":{"launch_id":"launch","page_id":"page","player_id":"visible",
                "source_id":"source","generation":1,"time_basis":"source_media_presentation"},
            "policy":{"minimum_lead_us":100,"target_lead_us":1000,"maximum_lead_us":2000}
        });
        let original: BrowserAnalysisCaptureDeclaration =
            serde_json::from_value(value.clone()).unwrap();
        original.validate().unwrap();
        for role in [
            "producer_declared",
            "display_capture_presentation",
            "camera_capture_presentation",
        ] {
            value["player"]["time_basis"] = role.into();
            let declaration: BrowserAnalysisCaptureDeclaration =
                serde_json::from_value(value.clone()).unwrap();
            assert!(declaration.validate().is_err());
        }
        value["player"]["time_basis"] = "source_media_presentation".into();
        value["qualified"] = true.into();
        assert!(serde_json::from_value::<BrowserAnalysisCaptureDeclaration>(value).is_err());
    }
}
