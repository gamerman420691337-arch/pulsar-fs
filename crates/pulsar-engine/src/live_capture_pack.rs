//! Fresh pack admission, preserving the caller's original request fingerprint.
use super::*;

pub(super) fn resolve(
    engine: &Arc<Engine>,
    request: &Request,
    session: &SessionId,
    fingerprint: &str,
) -> PResult<Request> {
    let options = {
        let db = engine.db.lock().map_err(internal)?;
        engine.authorize(&db, request, session)?;
        check_revision(&db, request)?;
        // A historical receipt is observation, never fresh pack/model execution.
        if replay(&db, session.as_str(), request, fingerprint)?.is_some() {
            return Ok(request.clone());
        }
        match &request.command {
            Command::StartLiveCapture { options, .. }
            | Command::ContinueLiveCapture { options, .. } => options.clone(),
            Command::ContinueLiveCaptureAt {
                predecessor,
                source_anchor,
                lease_ms,
                ..
            } => {
                let mut registry = engine.live_captures.lock().map_err(internal)?;
                registry.sweep(&db)?;
                let entry = registry.entries.get(predecessor.as_str()).ok_or_else(|| {
                    ProtocolError::new(ErrorCode::NotFound, "continuation predecessor is absent")
                })?;
                if entry.owner != *session || entry.report.identity.project != *project(request)? {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "continuation predecessor belongs to another actor or project",
                    ));
                }
                if Some(entry.report.identity.revision) != request.expected_revision {
                    return Err(ProtocolError::new(
                        ErrorCode::RevisionConflict,
                        "continuation predecessor revision differs",
                    ));
                }
                report(entry)?.continuation_options(*source_anchor, *lease_ms)?
            }
            _ => return Ok(request.clone()),
        }
    };
    let Some(choice) = &options.model_pack else {
        return Ok(request.clone());
    };
    choice.validate()?;
    let inference = engine.prepare_selected_live_pack(choice)?;
    if let Some(expected) = &options.inference {
        if serde_json::to_value(expected).map_err(internal)?
            != serde_json::to_value(&inference).map_err(internal)?
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "pack capture inference conflicts with freshly admitted model/profile",
            ));
        }
    }
    let mut options = options;
    options.inference = Some(inference);
    options.validate()?;
    let mut resolved = request.clone();
    resolved.command = match &request.command {
        Command::StartLiveCapture { engine_epoch, .. } => Command::StartLiveCapture {
            engine_epoch: engine_epoch.clone(),
            options,
        },
        Command::ContinueLiveCapture {
            engine_epoch,
            predecessor,
            ..
        }
        | Command::ContinueLiveCaptureAt {
            engine_epoch,
            predecessor,
            ..
        } => Command::ContinueLiveCapture {
            engine_epoch: engine_epoch.clone(),
            predecessor: predecessor.clone(),
            options,
        },
        _ => unreachable!("pack admission command checked"),
    };
    Ok(resolved)
}
