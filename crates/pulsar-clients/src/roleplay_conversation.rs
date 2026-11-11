//! Ephemeral roleplay transport shared by Desktop and CLI.
use crate::{motion::ResponseBody, EngineApi, SessionClient};
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use std::{
    io::{self, BufRead, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(crate) fn profiles(
    client: &mut SessionClient,
    project: &ProjectId,
) -> Result<Vec<AssistantProfile>> {
    client.set_operation_deadline(Some(Instant::now() + Duration::from_secs(5)));
    match client.execute(Command::AssistantProfiles, Some(project.clone()), None)? {
        ResponseBody::AssistantProfiles(profiles) => Ok(profiles),
        _ => anyhow::bail!("Engine returned an unexpected model catalog"),
    }
}

pub(crate) fn infer(
    client: &mut SessionClient,
    bootstrap: &Path,
    project: &ProjectId,
    input: RoleplayTurnInput,
    deadline_ms: u64,
) -> Result<Box<RoleplayReply>> {
    let directory = bootstrap
        .parent()
        .context("Engine state directory is missing")?;
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
    client.set_operation_deadline(Some(
        Instant::now() + Duration::from_millis(deadline_ms.saturating_add(5000)),
    ));
    match client.execute(
        Command::RunRoleplayTurn { input, owner_proof },
        Some(project.clone()),
        None,
    )? {
        ResponseBody::RoleplayReply(reply) => Ok(reply),
        _ => anyhow::bail!("Engine returned an unexpected roleplay reply"),
    }
}

fn read_message(input: &mut impl BufRead) -> Result<Option<String>> {
    let mut bytes = Vec::new();
    if input
        .take((MAX_ROLEPLAY_REPLY_BYTES + 2) as u64)
        .read_until(b'\n', &mut bytes)?
        == 0
    {
        return Ok(None);
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    ensure!(
        bytes.len() <= MAX_ROLEPLAY_REPLY_BYTES,
        "roleplay input line exceeds its byte bound"
    );
    Ok(Some(
        String::from_utf8(bytes).context("roleplay input must be UTF-8")?,
    ))
}

pub(crate) fn run_cli(
    endpoint: &Path,
    bootstrap: &Path,
    project: ProjectId,
    consent: PathBuf,
    profile_id: Option<AttemptId>,
    media: Option<PathBuf>,
) -> Result<()> {
    let mut client = SessionClient::connect(endpoint, bootstrap)?;
    let registered = profiles(&mut client, &project)?;
    let profile = match profile_id {
        Some(id) => registered
            .iter()
            .find(|profile| profile.profile_id == id)
            .context("requested local model profile is not registered")?,
        None => match registered.as_slice() {
            [profile] => profile,
            [] => anyhow::bail!("No local model profile is registered"),
            _ => anyhow::bail!("Select one of the registered local models with --profile"),
        },
    };
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let mut messages = Vec::new();
    let mut work_calls = 0;
    while let Some(text) = read_message(&mut stdin)? {
        if text.trim().is_empty() {
            continue;
        }
        messages.push(RoleplayMessage {
            speaker: RoleplaySpeaker::User,
            text,
            tool_receipts: vec![],
        });
        let input = RoleplayTurnInput {
            profile: profile.profile_id.clone(),
            consent: consent.clone(),
            messages: messages.clone(),
        };
        input.encode_messages()?;
        let RoleplayReply { text, calls } = *infer(
            &mut client,
            bootstrap,
            &project,
            input,
            profile.wall_time_ms,
        )?;
        let tool_receipts = crate::mcp_stdio::dispatch_roleplay_calls(
            endpoint,
            bootstrap,
            project.clone(),
            &consent,
            media.as_deref(),
            calls,
            &mut work_calls,
            || true,
        );
        let reply = RoleplayMessage {
            speaker: RoleplaySpeaker::Assistant,
            text,
            tool_receipts,
        };
        serde_json::to_writer(&mut stdout, &reply)?;
        writeln!(stdout)?;
        stdout.flush()?;
        messages.push(reply);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_message_transport_refuses_oversize_and_preserves_turn_boundaries() {
        let mut input = &b"first\r\nsecond"[..];
        assert_eq!(read_message(&mut input).unwrap().as_deref(), Some("first"));
        assert_eq!(read_message(&mut input).unwrap().as_deref(), Some("second"));
        assert!(read_message(&mut input).unwrap().is_none());
        let bytes = vec![b'x'; MAX_ROLEPLAY_REPLY_BYTES + 1];
        assert!(read_message(&mut bytes.as_slice()).is_err());
        assert!(read_message(&mut &b"\xff\n"[..]).is_err());
    }
}
