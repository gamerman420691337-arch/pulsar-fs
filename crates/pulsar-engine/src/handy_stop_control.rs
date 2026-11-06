//! Incomplete stop history and boot-local activity; never physical stopping proof.
use super::*;

impl Engine {
    pub(in crate::authority) fn handy_stop_status(
        &self,
        r: &Request,
        actor: &SessionId,
    ) -> PResult<ResponseBody> {
        r.validate()?;
        let Command::HandyStopStatus {
            acquisition_request_id,
            ..
        } = &r.command
        else {
            return Err(ProtocolError::invalid("expected stop status request"));
        };
        let db = self.db.lock().map_err(internal)?;
        self.authorize(&db, r, actor)?;
        let saved = load(&db, actor, project(r)?, acquisition_request_id)?;
        let journal = self._device_effect_journal.lock().map_err(internal)?;
        let observations = journal.history(&saved.device_id, 128).map_err(internal)?;
        let mut history = Vec::new();
        for observation in observations {
            let effect = &observation.record;
            if !effect.stop
                || effect.boot != saved.engine_epoch
                || effect.device_session != saved.device_session
                || effect.admission.project != saved.project_id
                || effect.admission.revision != saved.revision
            {
                continue;
            }
            let emergency = journal
                .emergency_stop_recorded(&observation.id)
                .map_err(internal)?;
            history.push(HandyStopHistory {
                record_id: ArtifactId::new(observation.id).map_err(internal)?,
                sequence: effect.sequence,
                outcome: observation.outcome.map(|outcome| match outcome {
                    crate::playback_runtime::DispatchOutcome::Acknowledged => {
                        HandyStopOutcome::Acknowledged
                    }
                    crate::playback_runtime::DispatchOutcome::Unknown => HandyStopOutcome::Unknown,
                }),
                emergency,
            });
        }
        drop(journal);
        let native = {
            let registry = self.handy_preparations.lock().map_err(internal)?;
            if let Some(entry) = registry.entries.get(&saved.preparation_id).filter(|e| {
                e.owner == *actor
                    && e.controller.as_ref().is_some_and(|c| {
                        c.report.controller_id == saved.controller_id
                            && c.report.acquisition_request_id == *acquisition_request_id
                    })
            }) {
                entry.stop_activity.lock().map_err(internal)?.clone()
            } else {
                None
            }
        };
        let status = HandyStopStatus {
            project_id: saved.project_id,
            acquisition_request_id: acquisition_request_id.clone(),
            controller_id: saved.controller_id,
            engine_epoch: saved.engine_epoch,
            device_session: saved.device_session,
            history,
            native,
            history_complete: false,
        };
        status.validate()?;
        Ok(ResponseBody::HandyStop(Box::new(status)))
    }
}
