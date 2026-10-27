//! Local, exact unclaimed cancellation. It performs no device or hosting I/O.
use super::*;
impl Engine {
    pub(super) fn handy_setup_cancel(
        &self,
        r: &Request,
        session: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        r.validate()?;
        let Command::CancelHandySetup {
            setup_request_id,
            record_id,
            ..
        } = &r.command
        else {
            return Err(ProtocolError::unsupported("setup cancellation"));
        };
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, r, session)?;
        check_request_identity(&db, session.as_str(), r, fingerprint)?;
        let prior = replay(&db, session.as_str(), r, fingerprint)?;
        if let Some(body) = &prior {
            let ResponseBody::HandySetupCancellation(receipt) = body else {
                return Err(internal("setup cancellation replay type differs"));
            };
            receipt.validate()?;
            if receipt.request_id != r.request_id
                || receipt.status.project_id != *project(r)?
                || receipt.status.setup_request_id != *setup_request_id
                || receipt.status.history.as_ref().map(|h| &h.record_id) != Some(record_id)
            {
                return Err(internal(
                    "setup cancellation replay differs from captured intent",
                ));
            }
            if receipt.settled {
                return Ok(body.clone());
            }
        }
        // Keep the established authority-before-journal lock order. No external I/O occurs.
        let mut journal = self._device_effect_journal.lock().map_err(internal)?;
        let observed = journal
            .setup_request_history(session, setup_request_id)
            .map_err(internal)?
            .ok_or_else(|| {
                ProtocolError::new(ErrorCode::NotFound, "original setup history not found")
            })?;
        if observed.id != record_id.as_str() || observed.record.project_id != *project(r)? {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "setup cancellation selection differs",
            ));
        }
        use crate::device_effect_journal::SetupJournalState as J;
        if !matches!(observed.state, J::Prepared | J::Cancelled) {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "setup was claimed or attempted; local cancellation cannot stop or retract it",
            ));
        }
        if prior.is_none() {
            let status = handy_setup::status_from_observation(
                project(r)?.clone(),
                setup_request_id.clone(),
                Some(observed),
            )?;
            let pending = HandySetupCancellation {
                request_id: r.request_id.clone(),
                status,
                settled: false,
            };
            pending.validate()?;
            let tx = db.transaction().map_err(internal)?;
            remember(
                &tx,
                session.as_str(),
                r,
                fingerprint,
                &ResponseBody::HandySetupCancellation(Box::new(pending)),
            )?;
            tx.commit().map_err(internal)?;
        }
        journal
            .cancel_setup_unclaimed(record_id.as_str())
            .map_err(|_| {
                ProtocolError::new(
                    ErrorCode::UnknownOutcome,
                    "setup cancellation could not be confirmed; inspect original history",
                )
            })?;
        let observed = journal
            .setup_request_history(session, setup_request_id)
            .map_err(internal)?
            .ok_or_else(|| internal("cancelled setup history disappeared"))?;
        let status = handy_setup::status_from_observation(
            project(r)?.clone(),
            setup_request_id.clone(),
            Some(observed),
        )?;
        let receipt = HandySetupCancellation {
            request_id: r.request_id.clone(),
            status,
            settled: true,
        };
        receipt.validate()?;
        let body = ResponseBody::HandySetupCancellation(Box::new(receipt));
        let tx = db.transaction().map_err(internal)?;
        let count=tx.execute("UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",params![encode(&body)?,session.as_str(),r.request_id.as_str()]).map_err(|_|ProtocolError::new(ErrorCode::UnknownOutcome,"cancellation outcome could not be saved; retry only the original cancellation request"))?;
        if count != 1 {
            return Err(internal("setup cancellation reservation disappeared"));
        }
        tx.commit().map_err(|_| {
            ProtocolError::new(
                ErrorCode::UnknownOutcome,
                "cancellation receipt commit is uncertain; retry the original request",
            )
        })?;
        Ok(body)
    }
}
