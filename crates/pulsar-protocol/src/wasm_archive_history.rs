//! Bounded historical observations; discovery grants no execution or commit authority.
use crate::*;
use serde::{Deserialize, Serialize};
pub const WASM_ARCHIVE_PAGE_LIMIT: u16 = 32;
pub fn validate_wasm_archive_limit(limit: u16) -> Result<(), ProtocolError> {
    if !(1..=WASM_ARCHIVE_PAGE_LIMIT).contains(&limit) {
        return Err(ProtocolError::invalid(
            "WASM archive page limit must be 1..=32",
        ));
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmArchivePage {
    pub project_id: ProjectId,
    pub after: Option<AttemptId>,
    pub limit: u16,
    pub operations: Vec<WasmEditReport>,
    pub next: Option<AttemptId>,
}
impl WasmArchivePage {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_wasm_archive_limit(self.limit)?;
        if self.operations.len() > usize::from(self.limit) {
            return Err(ProtocolError::invalid(
                "WASM archive page exceeds requested limit",
            ));
        }
        WasmEditCatalog {
            project_id: self.project_id.clone(),
            operations: self.operations.clone(),
        }
        .validate()?;
        let mut previous = self.after.as_ref();
        for report in &self.operations {
            if report.state != WasmEditState::Completed
                || previous.is_some_and(|p| p.as_str() >= report.attempt_id.as_str())
            {
                return Err(ProtocolError::invalid(
                    "WASM archive page is not completed ordered history",
                ));
            }
            previous = Some(&report.attempt_id);
        }
        if let Some(next) = &self.next {
            if self.operations.len() != usize::from(self.limit)
                || self.operations.last().map(|r| &r.attempt_id) != Some(next)
            {
                return Err(ProtocolError::invalid(
                    "WASM archive continuation differs from the full page",
                ));
            }
        }
        if serde_json::to_vec(self)
            .map_err(|e| ProtocolError::invalid(e.to_string()))?
            .len()
            > MAX_CONTROL_BYTES - 16 * 1024
        {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "WASM archive page exceeds control admission",
            ));
        }
        Ok(())
    }
    pub fn validate_for(&self, request: &Request) -> Result<(), ProtocolError> {
        self.validate()?;
        match &request.command {
            Command::ListArchivedWasmEdits { after, limit }
                if request.project.as_ref() == Some(&self.project_id)
                    && after == &self.after
                    && limit == &self.limit =>
            {
                Ok(())
            }
            _ => Err(ProtocolError::invalid(
                "WASM archive page request correspondence differs",
            )),
        }
    }
}
