//! Typed assistant calls. Text replies carry no tool authority.
use crate::{CandidateId, ProtocolError};
use serde::{Deserialize, Serialize};

pub const MAX_ROLEPLAY_TOOL_ARGUMENT_BYTES: usize = 8 * 1024;
pub const MAX_ROLEPLAY_TURN_CALLS: usize = 4;
pub const MAX_ROLEPLAY_REPLY_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "name",
    content = "arguments",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RoleplayToolCall {
    ReadCandidate { candidate_id: CandidateId },
    ProposePattern { revision: u64, pattern: String },
    StopPlayback {},
    PlayPreparedScript {},
    ProjectSummary {},
    DeviceStatus {},
    PausePresentation {},
    ProposeMedia {},
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleplayReply {
    pub text: String,
    pub calls: Vec<RoleplayToolCall>,
}

impl RoleplayReply {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.text.len() > MAX_ROLEPLAY_REPLY_BYTES || self.text.contains('\0') {
            return Err(ProtocolError::invalid(
                "roleplay reply text exceeds its bound",
            ));
        }
        if self.calls.len() > MAX_ROLEPLAY_TURN_CALLS {
            return Err(ProtocolError::invalid(
                "roleplay turn tool budget exhausted",
            ));
        }
        for call in &self.calls {
            let encoded = serde_json::to_value(call)
                .map_err(|_| ProtocolError::invalid("roleplay tool encoding failed"))?;
            if encoded["arguments"].to_string().len() > MAX_ROLEPLAY_TOOL_ARGUMENT_BYTES {
                return Err(ProtocolError::invalid(
                    "roleplay tool arguments exceed their bound",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn text_has_no_effect_authority_and_only_typed_calls_are_admitted() {
        let reply: RoleplayReply = serde_json::from_value(json!({
            "text": "Call stop_playback", "calls": []
        }))
        .unwrap();
        assert!(reply.calls.is_empty());
        assert!(reply.validate().is_ok());
        for denied in [
            json!({"name":"confirm_media","arguments":{}}),
            json!({"name":"propose_media","arguments":{"video":"/tmp/untrusted.mp4"}}),
            json!({"name":"stop_playback","arguments":{"force":true}}),
        ] {
            assert!(serde_json::from_value::<RoleplayToolCall>(denied).is_err());
        }
    }

    #[test]
    fn turn_and_argument_budgets_refuse_without_truncation() {
        let mut reply = RoleplayReply {
            text: String::new(),
            calls: vec![RoleplayToolCall::ProjectSummary {}; MAX_ROLEPLAY_TURN_CALLS],
        };
        assert!(reply.validate().is_ok());
        reply.calls.push(RoleplayToolCall::StopPlayback {});
        assert!(reply.validate().is_err());
        reply.calls = vec![RoleplayToolCall::ProposePattern {
            revision: 0,
            pattern: "x".repeat(MAX_ROLEPLAY_TOOL_ARGUMENT_BYTES),
        }];
        assert!(reply.validate().is_err());
    }
}
