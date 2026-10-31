//! Follow recorded Engine links; no new Start or source/lease is manufactured.
use super::*;

pub(super) fn discarded_view(preview: &Preview) -> Result<serde_json::Value> {
    let mut value = preview.view()?;
    value["browser"]["analysis_frame_discarded"] = serde_json::Value::Bool(true);
    Ok(value)
}

pub(super) fn notice(preview: &Preview) -> Option<serde_json::Value> {
    let report = preview.report.as_ref()?;
    let info = report.inference.as_ref()?;
    if let Some(transition) = &info.fallback_transition {
        return Some(
            serde_json::json!({"phase":if transition.phase == BrowserInferenceFallbackPhase::Refused {"refused"} else {"waiting"},
            "predecessor_epoch":report.identity.epoch,"target_epoch":transition.next_epoch,"error":transition.error}),
        );
    }
    let admission = info.fallback.as_ref()?;
    if info.state == LiveCaptureInferenceState::Starting {
        return Some(
            serde_json::json!({"phase":"waiting","predecessor_epoch":report.identity.epoch}),
        );
    }
    if admission.history.is_empty() && info.state != LiveCaptureInferenceState::Running {
        return None;
    }
    Some(
        serde_json::json!({"phase":"ready","target_epoch":report.identity.epoch,
        "profile_sha256":info.profile_sha256,"provider":report.options.inference.as_ref()?.profile.provider}),
    )
}

/// True means this operation's already captured frame must be discarded.
pub(super) fn reconcile(preview: &mut Preview) -> Result<bool> {
    let Some(original) = preview.report.clone() else {
        return Ok(false);
    };
    if original
        .inference
        .as_ref()
        .is_none_or(|i| i.fallback_transition.is_none())
    {
        if original.options.inference_fallback.is_some() {
            let state = original
                .inference
                .as_ref()
                .context("selected inference absent")?
                .state;
            if state == LiveCaptureInferenceState::Starting {
                preview.bulk = None;
                return Ok(true);
            }
            if original.active
                && state == LiveCaptureInferenceState::Running
                && preview.bulk.is_none()
            {
                preview.blocked = true;
                preview.begin_recording(
                    &original,
                    preview
                        .playback_epoch
                        .clone()
                        .context("original playback absent")?,
                )?;
                preview.bulk = Some(preview.client.open_live_bulk(&original)?);
                preview.blocked = false;
                return Ok(true);
            }
        }
        return Ok(false);
    }
    ensure!(
        preview.pending.is_none() && preview.analysis_successor.is_none(),
        "fallback cannot reuse a graceful successor reservation"
    );
    let mut previous = original.clone();
    let maximum = original
        .options
        .inference_fallback
        .as_ref()
        .context("fallback selection absent")?
        .maximum_fallbacks;
    for _ in 0..=maximum {
        let transition = previous
            .inference
            .as_ref()
            .and_then(|i| i.fallback_transition.as_ref())
            .context("recorded fallback transition absent")?;
        match transition.phase {
            BrowserInferenceFallbackPhase::Preparing => {
                preview.bulk = None;
                return Ok(true);
            }
            BrowserInferenceFallbackPhase::Refused => bail!(
                "Engine refused original fallback: {}",
                transition
                    .error
                    .as_ref()
                    .context("fallback refusal missing")?
            ),
            BrowserInferenceFallbackPhase::Started => {}
        }
        let next = preview.command(Command::LiveCaptureStatus {
            epoch: transition
                .next_epoch
                .clone()
                .context("target epoch absent")?,
        })?;
        validate_browser_fallback_target(&previous, &next)?;
        let info = next.inference.as_ref().context("target inference absent")?;
        if info.fallback_transition.is_some() {
            previous = next;
            continue;
        }
        ensure!(next.active, "Engine target retired before browser handover");
        if info.state == LiveCaptureInferenceState::Starting {
            preview.bulk = None;
            return Ok(true);
        }
        ensure!(
            info.state == LiveCaptureInferenceState::Running,
            "target is not a running fresh feed"
        );
        let declaration = next
            .options
            .browser_analysis
            .as_ref()
            .context("target feed absent")?
            .clone();
        let mode = AnalysisCpuMode::from_report(&next)?;
        let feed = preview
            .analysis_feed
            .as_ref()
            .context("original native feed absent")?;
        ensure!(
            feed.permit.current()
                && feed.close.is_none()
                && feed.media
                    == *next
                        .options
                        .browser_media
                        .as_ref()
                        .context("target media absent")?
                && feed.browser_timing.binding_id == declaration.timing_binding_id
                && feed.binding.player == declaration.player,
            "original player/media/timing custody retired"
        );
        let deadline = feed
            .capture_deadline
            .context("original browser deadline absent")?;
        ensure!(
            Instant::now() < deadline,
            "original browser capture deadline expired"
        );
        preview.blocked = true;
        preview.bulk = None;
        preview.report = Some(next.clone());
        let feed = preview.analysis_feed.as_mut().expect("checked feed");
        feed.binding.feed_id = declaration.feed_id;
        feed.binding.capture_epoch = next.identity.epoch.clone();
        feed.cpu_mode = mode;
        feed.latest = None;
        feed.last_upload = None;
        feed.measured_lead_us = None;
        // Keep the earlier local bound; the Engine enforces its exact original cap.
        feed.capture_deadline = Some(deadline);
        preview.sequence = 0;
        preview.last_time = None;
        preview.eligible_after = Instant::now();
        preview.begin_recording(
            &next,
            preview
                .playback_epoch
                .clone()
                .context("original playback absent")?,
        )?;
        preview.bulk = Some(preview.client.open_live_bulk(&next)?);
        preview.blocked = false;
        return Ok(true);
    }
    bail!("recorded fallback chain exceeded original allowance")
}
