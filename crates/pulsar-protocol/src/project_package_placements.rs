//! Versioned, inert placement closure. Import must assign fresh local IDs.
use crate::{ErrorCode, PortableProjectManifest, ProtocolError, RevisionId, SourceKind};
use pulsar_core::{SourcePlacement, SourcePlacementId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRevisionPlacements {
    pub revision: RevisionId,
    pub placements: Vec<SourcePlacementId>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageHistoryPlacements {
    pub position: u64,
    pub placements: Vec<SourcePlacementId>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackagePlacementClosure {
    pub definitions: Vec<SourcePlacement>,
    pub active: Vec<SourcePlacementId>,
    /// Missing legacy membership rows remain absent, meaning empty membership.
    pub revisions: Vec<PackageRevisionPlacements>,
    pub history: Vec<PackageHistoryPlacements>,
}
fn invalid(message: &str) -> ProtocolError {
    ProtocolError::invalid(message)
}
fn ordered<T: Ord>(items: impl Iterator<Item = T>) -> bool {
    let mut prior = None;
    for item in items {
        if prior.as_ref().is_some_and(|p| p >= &item) {
            return false;
        }
        prior = Some(item);
    }
    true
}
impl PackagePlacementClosure {
    pub fn entry_count(&self) -> Result<usize, ProtocolError> {
        [
            1,
            self.definitions.len(),
            self.active.len(),
            self.revisions.len(),
            self.history.len(),
        ]
        .into_iter()
        .chain(self.revisions.iter().map(|r| r.placements.len()))
        .chain(self.history.iter().map(|r| r.placements.len()))
        .try_fold(0usize, |n, c| n.checked_add(c))
        .ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "placement closure count overflow",
            )
        })
    }
    pub fn validate(&self, manifest: &PortableProjectManifest) -> Result<(), ProtocolError> {
        if self.definitions.is_empty()
            || self.definitions.len() > 4096
            || !ordered(self.definitions.iter().map(|d| &d.placement_id))
            || !ordered(self.revisions.iter().map(|r| r.revision))
            || !ordered(self.history.iter().map(|r| r.position))
        {
            return Err(invalid(
                "placement definitions/membership must be bounded, unique and ordered",
            ));
        }
        let definitions: BTreeMap<_, _> = self
            .definitions
            .iter()
            .map(|d| (&d.placement_id, d))
            .collect();
        let sources: BTreeMap<_, _> = manifest
            .sources
            .iter()
            .map(|s| (&s.source_version, s))
            .collect();
        let revisions: BTreeSet<_> = manifest.revisions.iter().map(|r| r.revision).collect();
        let history: BTreeMap<_, _> = manifest
            .edit_states
            .iter()
            .map(|r| (r.position, r))
            .collect();
        for d in &self.definitions {
            d.spec.validate().map_err(|e| invalid(&e.to_string()))?;
            if !revisions.contains(&d.created_revision)
                || sources
                    .get(&d.spec.source_version)
                    .is_none_or(|s| s.kind != SourceKind::Media || s.evicted)
            {
                return Err(invalid(
                    "placement definition requires its retained media and creation revision",
                ));
            }
        }
        let check =
            |ids: &[SourcePlacementId], revision: RevisionId| -> Result<(), ProtocolError> {
                if ids.len() > 128
                    || !ordered(ids.iter())
                    || ids.iter().any(|id| {
                        definitions
                            .get(id)
                            .is_none_or(|d| d.created_revision > revision)
                    })
                {
                    return Err(invalid(
                        "placement membership is unordered, missing or precedes creation",
                    ));
                }
                Ok(())
            };
        check(&self.active, manifest.captured_revision)?;
        for r in &self.revisions {
            if !revisions.contains(&r.revision) {
                return Err(invalid("placement revision membership has no revision"));
            }
            check(&r.placements, r.revision)?;
        }
        for r in &self.history {
            let state = history
                .get(&r.position)
                .ok_or_else(|| invalid("placement history edit state is absent"))?;
            check(
                &r.placements,
                state.source_revision.unwrap_or(manifest.captured_revision),
            )?;
        }
        let head = self
            .revisions
            .iter()
            .find(|r| r.revision == manifest.captured_revision)
            .map(|r| r.placements.as_slice())
            .unwrap_or(&[]);
        let current = self
            .history
            .iter()
            .find(|r| r.position == manifest.head.history_cursor)
            .map(|r| r.placements.as_slice())
            .unwrap_or(&[]);
        if self.active != head || self.active != current {
            return Err(invalid(
                "active placement membership differs from captured head/history",
            ));
        }
        Ok(())
    }
}
