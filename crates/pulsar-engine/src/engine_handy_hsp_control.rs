//! Owner-authenticated observations and the Engine's private HSP lifetime.
use super::*;
use crate::handy_hsp_execution::{HspExecutionError, HspExecutionHandle, HspExecutionPump};

fn hsp_error(error: HspExecutionError) -> ProtocolError {
    match error {
        HspExecutionError::NotFound => ProtocolError::new(
            ErrorCode::NotFound,
            "no native HSP session is retained for this original actor request",
        ),
        HspExecutionError::InvalidAdmission => {
            ProtocolError::invalid("invalid native HSP admission")
        }
        _ => ProtocolError::new(ErrorCode::Unavailable, "native HSP owner is unavailable"),
    }
}

impl Engine {
    /// Live admission remains private pending native preparation and a producer.
    /// Explicit recovery may start an idle owner to fence historical-only routes.
    /// Merely observing status never calls this method or starts a device owner.
    pub(in crate::authority) fn ensure_handy_hsp_execution(&self) -> PResult<HspExecutionHandle> {
        if self.is_shutting_down() {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "Engine is shutting down",
            ));
        }
        let mut slot = self.handy_hsp_execution.lock().map_err(internal)?;
        if let Some(pump) = slot.as_ref() {
            if !pump.is_running() {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "native HSP owner stopped",
                ));
            }
        } else {
            *slot = Some(
                HspExecutionPump::start(
                    self._device_effect_journal.clone(),
                    self.shutdown_requested.clone(),
                )
                .map_err(hsp_error)?,
            );
        }
        Ok(slot.as_ref().expect("initialized HSP owner").handle())
    }

    pub(in crate::authority) fn handy_hsp_status(
        &self,
        request: &Request,
        actor: &SessionId,
    ) -> PResult<ResponseBody> {
        request.validate()?;
        let Command::HandyHspStatus {
            stream_request_id, ..
        } = &request.command
        else {
            return Err(ProtocolError::invalid("expected HSP status request"));
        };
        let db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, actor)?;
        let project = project(request)?;
        let handle = self
            .handy_hsp_execution
            .lock()
            .map_err(internal)?
            .as_ref()
            .map(HspExecutionPump::handle)
            .ok_or_else(|| hsp_error(HspExecutionError::NotFound))?;
        let snapshot = handle
            .snapshot_for_session(actor, project, stream_request_id)
            .map_err(hsp_error)?;
        let status = snapshot.into_status().map_err(hsp_error)?;
        status.validate()?;
        Ok(ResponseBody::HandyHsp(Box::new(status)))
    }
}
