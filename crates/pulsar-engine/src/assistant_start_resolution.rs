//! Atomic accepted-start discovery or original-ID withdrawal; never dispatches inference.
use super::*;
impl Engine {
    pub(super) fn resolve_assistant_start(
        &self,
        request: &Request,
        fingerprint: &str,
        owner: &SessionId,
    ) -> PResult<ResponseBody> {
        let Command::ResolveAssistantStart { reference } = &request.command else {
            return Err(ProtocolError::invalid("not an assistant resolution"));
        };
        reference.validate()?;
        if reference.original_request_id == request.request_id {
            return Err(ProtocolError::invalid(
                "original and resolution IDs must differ",
            ));
        }
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(internal)?;
        self.authorize(&tx, request, owner)?;
        if let Some(body) = replay(&tx, owner.as_str(), request, fingerprint)? {
            return Ok(body);
        }
        let present: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM revisions WHERE project=?1 AND revision=?2)",
                params![
                    project(request)?.as_str(),
                    rev_sql(reference.base_revision)?
                ],
                |r| r.get(0),
            )
            .map_err(internal)?;
        if !present {
            return Err(ProtocolError::new(
                ErrorCode::NotFound,
                "referenced project revision is absent",
            ));
        }
        let id: Option<String> = tx
            .query_row(
                "SELECT id FROM assistant_attempts WHERE session=?1 AND request=?2 AND project=?3",
                params![
                    owner.as_str(),
                    reference.original_request_id.as_str(),
                    project(request)?.as_str()
                ],
                |r| r.get(0),
            )
            .optional()
            .map_err(internal)?;
        let outcome = if let Some(id) = id {
            let mut original = request.clone();
            original.request_id = reference.original_request_id.clone();
            original.expected_revision = Some(reference.base_revision);
            original.command = Command::StartAssistant {
                spec: reference.spec.clone(),
            };
            let original_fingerprint = format!(
                "{:x}",
                Sha256::digest(request_fingerprint(&original).map_err(internal)?)
            );
            let stored: Option<String> = tx
                .query_row(
                    "SELECT fingerprint FROM request_identities WHERE session=?1 AND request=?2",
                    params![owner.as_str(), reference.original_request_id.as_str()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(internal)?;
            if stored.as_deref() != Some(original_fingerprint.as_str()) {
                return Err(ProtocolError::new(
                    ErrorCode::RequestConflict,
                    "accepted assistant invocation differs from retained request",
                ));
            }
            let report = assistant_operations::read(&tx, &AttemptId::new(id).map_err(internal)?)?;
            AssistantStartOutcome::Accepted {
                report: Box::new(report),
            }
        } else {
            let reserved = format!(
                "{:x}",
                Sha256::digest(
                    serde_json::to_vec(&(
                        "pulsar-unpublished-assistant-start-withdrawal-v1",
                        project(request)?,
                        reference
                    ))
                    .map_err(internal)?
                )
            );
            let old: Option<String> = tx
                .query_row(
                    "SELECT fingerprint FROM request_identities WHERE session=?1 AND request=?2",
                    params![owner.as_str(), reference.original_request_id.as_str()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(internal)?;
            if old.as_ref().is_some_and(|f| f != &reserved) {
                return Err(ProtocolError::new(
                    ErrorCode::RequestConflict,
                    "original request namespace is occupied; absence cannot establish withdrawal",
                ));
            }
            tx.execute(
                "INSERT OR IGNORE INTO request_identities VALUES(?1,?2,?3)",
                params![
                    owner.as_str(),
                    reference.original_request_id.as_str(),
                    reserved
                ],
            )
            .map_err(internal)?;
            AssistantStartOutcome::Withdrawn
        };
        let value = AssistantStartResolution {
            project_id: project(request)?.clone(),
            session_id: owner.clone(),
            reference: reference.clone(),
            outcome,
        };
        value.validate_for(request)?;
        let body = ResponseBody::AssistantStartResolution(Box::new(value));
        remember(&tx, owner.as_str(), request, fingerprint, &body)?;
        tx.commit().map_err(internal)?;
        Ok(body)
    }
}
