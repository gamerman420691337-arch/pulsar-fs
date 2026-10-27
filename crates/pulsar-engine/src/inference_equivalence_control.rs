//! Host-owner review of retained inference evidence; no scheduling or execution.
use super::inference_equivalence_store as store;
use super::*;
use std::{
    io::Read,
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) fn is_command(command: &Command) -> bool {
    matches!(
        command,
        Command::ImportInferenceEquivalence { .. }
            | Command::InferenceEquivalenceStatus { .. }
            | Command::ListInferenceEquivalences { .. }
            | Command::AcceptInferenceEquivalence { .. }
            | Command::WithdrawInferenceEquivalence { .. }
    )
}

fn invalid(message: &str) -> ProtocolError {
    ProtocolError::invalid(message)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn observed_time() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .filter(|n| *n > 0 && *n <= i64::MAX as u64)
}

fn read_exact_evidence(
    path: &Path,
    expected: &str,
    limit: u64,
    length: Option<u64>,
) -> PResult<Vec<u8>> {
    let mut bytes = Vec::new();
    artifacts::open_regular(path)
        .map_err(internal)?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(internal)?;
    if bytes.is_empty()
        || bytes.len() as u64 > limit
        || length.is_some_and(|n| n != bytes.len() as u64)
        || digest(&bytes) != expected
    {
        return Err(invalid(
            "inference equivalence input changed or exceeds its byte bound",
        ));
    }
    Ok(bytes)
}

fn bounded_replay(
    db: &Connection,
    actor: &SessionId,
    request: &Request,
    fingerprint: &str,
) -> PResult<Option<ResponseBody>> {
    let row: Option<(String, String)> = db.query_row(
        "SELECT fingerprint,CASE WHEN length(CAST(response AS BLOB))<=262144 THEN response ELSE NULL END FROM requests WHERE session=?1 AND request=?2",
        params![actor.as_str(), request.request_id.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().map_err(internal)?;
    match row {
        Some((old, body)) if old == fingerprint => decode(&body).map(Some),
        Some(_) => Err(ProtocolError::new(
            ErrorCode::RequestConflict,
            "request identity reused with different content",
        )),
        None => Ok(None),
    }
}

fn check_replay(body: &ResponseBody, request: &Request, actor: &SessionId) -> PResult<()> {
    let ResponseBody::InferenceEquivalenceSaved(receipt) = body else {
        return Err(invalid("inference equivalence receipt type differs"));
    };
    receipt.validate()?;
    if receipt.request_id != request.request_id {
        return Err(invalid("inference equivalence receipt request differs"));
    }
    let matches = match &request.command {
        Command::ImportInferenceEquivalence {
            manifest_sha256, ..
        } => receipt.input_sha256.as_ref() == Some(manifest_sha256),
        Command::AcceptInferenceEquivalence {
            dossier_id,
            expected_record_revision,
            operator,
            ..
        } => {
            receipt.input_sha256.is_none()
                && receipt.report.dossier_id == *dossier_id
                && Some(receipt.report.revision) == expected_record_revision.checked_add(1)
                && receipt.report.state == InferenceEquivalenceState::Accepted
                && receipt.report.decision.as_ref().is_some_and(|d| {
                    d.request_id == request.request_id
                        && d.actor_session == *actor
                        && d.operator.as_ref() == Some(operator)
                        && d.note.is_empty()
                })
        }
        Command::WithdrawInferenceEquivalence {
            dossier_id,
            expected_record_revision,
            reason,
            ..
        } => {
            receipt.input_sha256.is_none()
                && receipt.report.dossier_id == *dossier_id
                && Some(receipt.report.revision) == expected_record_revision.checked_add(1)
                && receipt.report.state == InferenceEquivalenceState::Withdrawn
                && receipt.report.decision.as_ref().is_some_and(|d| {
                    d.request_id == request.request_id
                        && d.actor_session == *actor
                        && d.operator.is_none()
                        && d.note == *reason
                })
        }
        _ => false,
    };
    if !matches {
        return Err(invalid(
            "inference equivalence receipt does not match original intent",
        ));
    }
    Ok(())
}

fn saved(
    report: InferenceEquivalenceReport,
    request: &Request,
    input_sha256: Option<String>,
) -> PResult<ResponseBody> {
    let receipt = InferenceEquivalenceReceipt {
        input_sha256,
        request_id: request.request_id.clone(),
        report,
    };
    receipt.validate()?;
    Ok(ResponseBody::InferenceEquivalenceSaved(Box::new(receipt)))
}

impl Engine {
    pub(super) fn inference_equivalence_control(
        &self,
        request: &Request,
        actor: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        request.validate()?;
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, actor)?;
        check_request_identity(&db, actor.as_str(), request, fingerprint)?;
        match &request.command {
            Command::InferenceEquivalenceStatus { dossier_id, .. } => {
                let _memory = self.pool.reserve_memory(128 * 1024 * 1024)?;
                return Ok(ResponseBody::InferenceEquivalenceStatus(Box::new(
                    store::status(&db, dossier_id)?,
                )));
            }
            Command::ListInferenceEquivalences { after, limit, .. } => {
                let _memory = self.pool.reserve_memory(128 * 1024 * 1024)?;
                return Ok(ResponseBody::InferenceEquivalenceList(Box::new(
                    store::list(&db, after.as_ref(), *limit)?,
                )));
            }
            _ => {}
        }
        if let Some(body) = bounded_replay(&db, actor, request, fingerprint)? {
            check_replay(&body, request, actor)?;
            // Replaying a historical receipt does not restore current acceptance,
            // refresh expiry or issue a runtime handle.
            return Ok(body);
        }
        match &request.command {
            Command::ImportInferenceEquivalence {
                path,
                manifest_sha256,
                ..
            } => {
                // Raw documents and their decoded structures coexist here.
                let _memory = self.pool.reserve_memory(128 * 1024 * 1024)?;
                let bytes = read_exact_evidence(
                    Path::new(path),
                    manifest_sha256,
                    INFERENCE_EQUIVALENCE_MANIFEST_LIMIT,
                    None,
                )?;
                let input: InferenceEquivalenceInput = serde_json::from_slice(&bytes)
                    .map_err(|_| invalid("invalid inference equivalence input document"))?;
                input.validate()?;
                let mut evidence = Vec::new();
                let mut payloads = Vec::new();
                let mut total = 0u64;
                for entry in input.evidence {
                    let bytes = read_exact_evidence(
                        Path::new(&entry.path),
                        &entry.sha256,
                        INFERENCE_EQUIVALENCE_EVIDENCE_LIMIT,
                        Some(entry.byte_len),
                    )?;
                    total = total
                        .checked_add(entry.byte_len)
                        .filter(|n| *n <= INFERENCE_EQUIVALENCE_EVIDENCE_TOTAL_LIMIT)
                        .ok_or_else(|| invalid("inference equivalence aggregate evidence bound"))?;
                    evidence.push(InferenceEquivalenceEvidence {
                        kind: entry.kind,
                        identity: ArtifactIdentity {
                            artifact_id: ArtifactId::new(entry.sha256.clone()).map_err(internal)?,
                            sha256: entry.sha256,
                            byte_len: entry.byte_len,
                        },
                    });
                    payloads.push((entry.kind, bytes));
                }
                evidence.sort_by_key(|entry| entry.kind);
                let manifest = InferenceEquivalenceManifest {
                    subject: input.subject,
                    evidence,
                };
                manifest.validate()?;
                if fs2::available_space(&self.config.state_dir).map_err(internal)?
                    < total
                        .saturating_mul(3)
                        .saturating_add(self.pool.reserved_storage())
                        .saturating_add(16 * 1024 * 1024)
                {
                    return Err(ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "insufficient inference equivalence evidence storage",
                    ));
                }
                let tx = db.transaction().map_err(internal)?;
                let report = store::import_pending(&tx, &manifest, &payloads)?;
                let body = saved(report, request, Some(manifest_sha256.clone()))?;
                remember(&tx, actor.as_str(), request, fingerprint, &body)?;
                tx.commit().map_err(internal)?;
                Ok(body)
            }
            Command::AcceptInferenceEquivalence {
                dossier_id,
                expected_record_revision,
                operator,
                ..
            } => {
                let _memory = self.pool.reserve_memory(128 * 1024 * 1024)?;
                let now = observed_time().ok_or_else(|| {
                    ProtocolError::new(
                        ErrorCode::Unavailable,
                        "wall clock unavailable for inference equivalence acceptance",
                    )
                })?;
                let tx = db.transaction().map_err(internal)?;
                let report = store::accept(
                    &tx,
                    dossier_id,
                    *expected_record_revision,
                    actor,
                    &request.request_id,
                    operator,
                    now,
                )?;
                let body = saved(report, request, None)?;
                remember(&tx, actor.as_str(), request, fingerprint, &body)?;
                tx.commit().map_err(internal)?;
                Ok(body)
            }
            Command::WithdrawInferenceEquivalence {
                dossier_id,
                expected_record_revision,
                reason,
                ..
            } => {
                // The first transaction keeps withdrawal pending across a crash or
                // a later receipt-write failure. Its global request reservation
                // prevents any other command from reusing the original ID.
                let tx = db.transaction().map_err(internal)?;
                let intent = store::begin_withdrawal(
                    &tx,
                    dossier_id,
                    *expected_record_revision,
                    actor,
                    request,
                    fingerprint,
                    reason,
                )?;
                tx.commit().map_err(internal)?;
                self.fallback_lifetimes.invalidate_dossier(dossier_id);
                let tx = db.transaction().map_err(internal)?;
                let report = store::finish_withdrawal(&tx, &intent, observed_time())?;
                let body = saved(report, request, None)?;
                remember(&tx, actor.as_str(), request, fingerprint, &body)?;
                tx.commit().map_err(internal)?;
                Ok(body)
            }
            _ => Err(ProtocolError::unsupported(
                "inference equivalence operation",
            )),
        }
    }
}
