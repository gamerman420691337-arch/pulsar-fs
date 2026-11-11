//! Typed assistant calls. Text replies carry no tool authority.
use crate::{CandidateId, ProtocolError};
use serde::{Deserialize, Serialize};

pub const MAX_ROLEPLAY_TOOL_ARGUMENT_BYTES: usize = 8 * 1024;
pub const MAX_ROLEPLAY_TURN_CALLS: usize = 4;
pub const MAX_ROLEPLAY_REPLY_BYTES: usize = 8 * 1024;
pub const MAX_ROLEPLAY_WORKER_RESPONSE_BYTES: u64 = 48 * 1024;

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

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleplayReply {
    pub text: String,
    pub calls: Vec<RoleplayToolCall>,
}

pub const MAX_ROLEPLAY_HISTORY_MESSAGES: usize = 15;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleplaySpeaker {
    User,
    Assistant,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleplayMessage {
    pub speaker: RoleplaySpeaker,
    pub text: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleplayTurnInput {
    pub profile: crate::AttemptId,
    pub consent: std::path::PathBuf,
    pub messages: Vec<RoleplayMessage>,
}

impl RoleplayTurnInput {
    pub fn encode_messages(&self) -> Result<String, ProtocolError> {
        if !self.consent.is_absolute() || self.consent.as_os_str().len() > 4096 {
            return Err(ProtocolError::invalid(
                "roleplay needs an absolute bounded consent path",
            ));
        }
        if self.messages.is_empty()
            || self.messages.len() > MAX_ROLEPLAY_HISTORY_MESSAGES
            || self.messages.len() % 2 == 0
            || self.messages.iter().enumerate().any(|(index, message)| {
                message.speaker
                    != if index % 2 == 0 {
                        RoleplaySpeaker::User
                    } else {
                        RoleplaySpeaker::Assistant
                    }
                    || message.text.trim().is_empty()
                    || message.text.contains('\0')
                    || message.text.len() > MAX_ROLEPLAY_REPLY_BYTES
            })
        {
            return Err(ProtocolError::invalid(
                "roleplay history requires bounded alternating messages ending with the user",
            ));
        }
        let encoded = serde_json::to_string(&self.messages)
            .map_err(|_| ProtocolError::invalid("roleplay history encoding failed"))?;
        if encoded.len() > crate::MAX_ASSISTANT_USER_REQUEST_BYTES {
            return Err(ProtocolError::invalid(
                "roleplay history exceeds the encoded byte bound",
            ));
        }
        Ok(encoded)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleplayWorkerResponse {
    pub version: u16,
    pub attempt_id: crate::AttemptId,
    pub runtime: crate::ArtifactIdentity,
    pub model: crate::ArtifactIdentity,
    pub reply: RoleplayReply,
}

pub fn validate_roleplay_worker_input(
    request: &crate::AssistantWorkerRequest,
) -> Result<(), ProtocolError> {
    request.validate()?;
    let scope = &request.scope;
    if request.vision.is_some()
        || scope.creative_patterns.is_some()
        || !scope.target_ids.is_empty()
        || !scope.review_ids.is_empty()
        || !scope.edit_recipe_ids.is_empty()
        || !scope.allowed_sample_strides.is_empty()
    {
        return Err(ProtocolError::invalid(
            "roleplay worker accepts text without proposal or vision inputs",
        ));
    }
    Ok(())
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
    fn turn_history_is_alternating_bounded_and_debug_redacted() {
        let mut input = RoleplayTurnInput {
            profile: crate::AttemptId::new("fixture").unwrap(),
            consent: std::env::temp_dir().join("consent.json"),
            messages: vec![RoleplayMessage {
                speaker: RoleplaySpeaker::User,
                text: "private fixture text".into(),
            }],
        };
        assert!(input
            .encode_messages()
            .unwrap()
            .contains("private fixture text"));
        assert!(!format!(
            "{:?}",
            crate::Command::RunRoleplayTurn {
                input: input.clone(),
                owner_proof: "fixture".into(),
            }
        )
        .contains("private fixture text"));
        assert!(!format!(
            "{:?}",
            crate::ResponseBody::RoleplayReply(Box::new(RoleplayReply {
                text: "private fixture text".into(),
                calls: vec![],
            }))
        )
        .contains("private fixture text"));
        input.messages.push(RoleplayMessage {
            speaker: RoleplaySpeaker::Assistant,
            text: "reply".into(),
        });
        assert!(input.encode_messages().is_err());
        input.messages.push(RoleplayMessage {
            speaker: RoleplaySpeaker::User,
            text: "x".repeat(MAX_ROLEPLAY_REPLY_BYTES + 1),
        });
        assert!(input.encode_messages().is_err());
    }

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
