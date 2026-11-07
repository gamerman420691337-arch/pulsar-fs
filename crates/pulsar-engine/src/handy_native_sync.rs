//! Pending and completed current-process sync facts, independent of the retired controller.
use super::*;
use crate::{
    device_effect_journal::ClockSyncEffectRecord,
    handy_playback_clock::{NativeDeviceClock, QualifiedDeviceClock},
};
pub(super) struct Context {
    pub(super) id: ArtifactId,
    pub(super) record: ClockSyncEffectRecord,
    pub(super) preparation: HandyPlaybackPreparation,
    pub(super) captured: handy_setup::Captured,
    pub(super) qualified: QualifiedDeviceClock,
    started_ns: u64,
}
pub(super) struct Slot {
    pub(super) context: Arc<Context>,
    pub(super) native: Option<NativeDeviceClock>,
}
impl Registry {
    pub(super) fn retain_syncs(&mut self, mut keep: impl FnMut(&Slot) -> bool) {
        self.syncs.retain(|_, v| keep(v));
        let syncs = &self.syncs;
        for e in self.entries.values_mut() {
            if e.playback_clock.as_ref().is_some_and(|c| {
                !syncs
                    .get(&c.sync.id)
                    .is_some_and(|s| Arc::ptr_eq(&s.context, &c.sync))
            }) {
                e.playback_clock = None;
            }
            if e.continuous_timing.as_ref().is_some_and(|c| {
                !syncs
                    .get(&c.pins.sync.id)
                    .is_some_and(|s| s.native.is_some() && Arc::ptr_eq(&s.context, &c.pins.sync))
            }) {
                e.continuous_timing = None;
            }
        }
    }
    pub(super) fn retire_syncs(&mut self, reason: Retirement<'_>) {
        self.retain_syncs(|s| {
            let c = &s.context;
            let r = &c.record;
            !match reason {
                Retirement::Id(id) => c.preparation.preparation_id == *id,
                Retirement::Device(id) => r.registration.device_id == *id,
                Retirement::Connection(id) => r.original_controller.device_session == *id,
                Retirement::Qualification(id) => r.preparation_qualification.dossier_id == *id,
                Retirement::Hosting(actor, id) => {
                    r.actor_session == *actor && c.captured.hosting.request_id == *id
                }
            }
        });
    }
    pub(super) fn prune_syncs(&mut self, now: u64) {
        let retained = self
            .entries
            .values()
            .filter_map(|entry| {
                entry.continuous_timing.as_ref().and_then(|timing| {
                    timing
                        .current(entry, now, &self.syncs)
                        .then(|| timing.pins.sync.clone())
                })
            })
            .collect::<Vec<_>>();
        self.retain_syncs(|s| {
            retained
                .iter()
                .any(|context| Arc::ptr_eq(context, &s.context))
                || s.native.as_ref().map_or_else(
                    || {
                        now.checked_sub(s.context.started_ns).is_some_and(|age| {
                            age <= s.context.qualified.contract.maximum_sync_age_ns
                        })
                    },
                    |n| n.error_at(now).is_ok(),
                )
        });
    }
    pub(super) fn begin_sync_context(
        &mut self,
        id: ArtifactId,
        record: ClockSyncEffectRecord,
        preparation: HandyPlaybackPreparation,
        captured: handy_setup::Captured,
        qualified: Option<QualifiedDeviceClock>,
        now: u64,
    ) -> PResult<Option<Arc<Context>>> {
        self.prune_syncs(now);
        self.retain_syncs(|s| {
            s.context.record.route_sha256 != record.route_sha256
                && s.context.record.registration.device_id != record.registration.device_id
        });
        let Some(qualified) = qualified else {
            return Ok(None);
        };
        if self.syncs.len() >= 64 {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "native synchronization capacity reached",
            ));
        }
        let context = Arc::new(Context {
            id: id.clone(),
            record,
            preparation,
            captured,
            qualified,
            started_ns: now,
        });
        self.syncs.insert(
            id,
            Slot {
                context: context.clone(),
                native: None,
            },
        );
        Ok(Some(context))
    }
    pub(super) fn finish_sync_context(
        &mut self,
        context: &Arc<Context>,
        native: Option<NativeDeviceClock>,
        now: u64,
    ) {
        self.prune_syncs(now);
        let current = self
            .syncs
            .get(&context.id)
            .is_some_and(|s| s.native.is_none() && Arc::ptr_eq(&s.context, context));
        if !current {
            return;
        }
        if let Some(native) = native.filter(|n| {
            n.acknowledgment.sent_boottime_ns >= context.started_ns
                && n.acknowledgment.received_boottime_ns
                    < context.record.original_controller.deadline_boottime_ns
                && n.error_at(now).is_ok()
        }) {
            self.syncs
                .get_mut(&context.id)
                .expect("checked native context")
                .native = Some(native);
        } else {
            self.retain_syncs(|s| !Arc::ptr_eq(&s.context, context));
        }
    }
}
