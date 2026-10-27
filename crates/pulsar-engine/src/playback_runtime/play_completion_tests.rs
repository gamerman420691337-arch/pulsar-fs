use super::tests::{clock, fixture, journal_path};
use super::*;
use crate::device_effect_journal::{DeviceEffectJournal, PreparedEffectReceipt};

fn begin(
    runtime: &mut HandyPlaybackRuntime,
    admission: &PlaybackAdmission,
    controller: &ClientId,
    journal: &mut DeviceEffectJournal,
) -> (PlaybackEffect, PreparedEffectReceipt) {
    let pending = runtime.prepare_start(controller, clock(), 1234).unwrap();
    runtime
        .dispatch_start_durable(
            pending,
            admission,
            controller,
            &clock().domain,
            clock().monotonic_ns,
            journal,
        )
        .unwrap()
}

fn journal(dir: &std::path::Path) -> DeviceEffectJournal {
    DeviceEffectJournal::open(dir, DeviceSessionId::new("boot").unwrap()).unwrap()
}

#[test]
fn matched_play_completion_records_once_and_unknown_returns_a_stop() {
    for outcome in [DispatchOutcome::Acknowledged, DispatchOutcome::Unknown] {
        let dir = tempfile::tempdir().unwrap();
        let (mut runtime, admission, controller) = fixture();
        let mut journal = journal(dir.path());
        let (play, receipt) = begin(&mut runtime, &admission, &controller, &mut journal);
        let completion = runtime
            .complete_play_with_recovery(&play, &receipt, outcome, &mut journal)
            .unwrap();
        assert_eq!(completion.observation.outcome(), outcome);
        assert!(!completion.observation.journal_pending());
        assert!(!runtime.effect_is_current(&play));
        let stop = completion.stop.unwrap();
        assert_eq!(stop.is_some(), outcome == DispatchOutcome::Unknown);
        if let Some(stop) = stop {
            assert!(runtime.effect_is_current(&stop));
            let prepared = runtime.prepare_stop_durable(stop, &mut journal).unwrap();
            runtime
                .complete_stop_durable(&prepared, DispatchOutcome::Acknowledged, &mut journal)
                .unwrap();
            assert_eq!(runtime.session().state(), DeviceState::StopUnconfirmed);
        } else {
            assert_eq!(runtime.session().state(), DeviceState::Playing);
        }
        assert!(runtime.session().physical_stop_pending());
        assert!(matches!(
            runtime.complete_play_with_recovery(&play, &receipt, outcome, &mut journal),
            Err(PlaybackFailure::RetiredCommand)
        ));
        drop(journal);
        let reopened =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("next-boot").unwrap())
                .unwrap();
        let history = reopened
            .history(&admission.configuration.device, 10)
            .unwrap();
        let recorded = history.iter().find(|entry| !entry.record.stop).unwrap();
        assert_eq!(recorded.outcome, Some(outcome));
        assert_eq!(recorded.record.device_session, *play.session_id());
    }
}

#[test]
fn play_write_failure_retains_observation_and_stop_without_replay() {
    for outcome in [DispatchOutcome::Acknowledged, DispatchOutcome::Unknown] {
        let dir = tempfile::tempdir().unwrap();
        let (mut runtime, admission, controller) = fixture();
        let mut journal = journal(dir.path());
        let (play, receipt) = begin(&mut runtime, &admission, &controller, &mut journal);
        let db = rusqlite::Connection::open(journal_path(dir.path())).unwrap();
        db.execute_batch("CREATE TRIGGER fail_completion BEFORE UPDATE OF outcome ON effects BEGIN SELECT RAISE(ABORT,'synthetic outcome write failure'); END;").unwrap();
        let mut completion = runtime
            .complete_play_with_recovery(&play, &receipt, outcome, &mut journal)
            .unwrap();
        assert!(completion.observation.journal_pending());
        assert!(!runtime.effect_is_current(&play));
        let stop = match completion.stop.unwrap() {
            Some(stop) => runtime.prepare_stop_durable(stop, &mut journal).unwrap(),
            None => runtime
                .request_stop_durable(&controller, &mut journal)
                .unwrap()
                .unwrap(),
        };
        assert_eq!(
            completion.observation.persist(&mut journal),
            Err(PlaybackFailure::JournalUnavailable)
        );
        assert!(matches!(
            runtime.complete_play_with_recovery(&play, &receipt, outcome, &mut journal),
            Err(PlaybackFailure::RetiredCommand)
        ));
        db.execute_batch("DROP TRIGGER fail_completion").unwrap();
        completion.observation.persist(&mut journal).unwrap();
        completion.observation.persist(&mut journal).unwrap();
        assert!(!completion.observation.journal_pending());
        assert!(runtime.prepared_stop_is_current(&stop));
        assert!(runtime.session().physical_stop_pending());
        assert_eq!(runtime.session().state(), DeviceState::StopRequested);
        assert!(runtime
            .request_stop_durable(&controller, &mut journal)
            .unwrap()
            .is_none());
        let history = journal
            .history(&admission.configuration.device, 10)
            .unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(
            history.iter().find(|x| !x.record.stop).unwrap().outcome,
            Some(outcome)
        );
        assert_eq!(
            history.iter().find(|x| x.record.stop).unwrap().outcome,
            None
        );
    }
}

#[test]
fn mismatched_receipt_or_journal_cannot_retire_play() {
    let dir = tempfile::tempdir().unwrap();
    let other_dir = tempfile::tempdir().unwrap();
    let (mut runtime, admission, controller) = fixture();
    let mut journal = journal(dir.path());
    let (play, receipt) = begin(&mut runtime, &admission, &controller, &mut journal);
    let mut other = HandyPlaybackRuntime::new(
        admission.configuration.device.clone(),
        DeviceSessionId::new("other-device-session").unwrap(),
        runtime.timing.clone(),
    )
    .unwrap();
    other
        .arm(controller.clone(), admission.clone(), None)
        .unwrap();
    let (_, substituted) = begin(&mut other, &admission, &controller, &mut journal);
    assert!(matches!(
        runtime.complete_play_with_recovery(
            &play,
            &substituted,
            DispatchOutcome::Acknowledged,
            &mut journal
        ),
        Err(PlaybackFailure::JournalUnavailable)
    ));
    let mut wrong = DeviceEffectJournal::open(
        other_dir.path(),
        DeviceSessionId::new("wrong-boot").unwrap(),
    )
    .unwrap();
    assert!(matches!(
        runtime.complete_play_with_recovery(
            &play,
            &receipt,
            DispatchOutcome::Acknowledged,
            &mut wrong
        ),
        Err(PlaybackFailure::JournalUnavailable)
    ));
    assert!(runtime.effect_is_current(&play));
    assert!(journal
        .history(&admission.configuration.device, 10)
        .unwrap()
        .iter()
        .all(|x| x.outcome.is_none()));
    let stop = runtime.request_stop(&controller).unwrap().unwrap();
    let stop_receipt = journal.prepare(&runtime, &stop).unwrap();
    assert!(matches!(
        runtime.complete_play_with_recovery(
            &stop,
            &stop_receipt,
            DispatchOutcome::Acknowledged,
            &mut journal
        ),
        Err(PlaybackFailure::RetiredCommand)
    ));
    assert!(runtime.effect_is_current(&stop));
}

#[test]
fn accepted_play_observation_cannot_cross_engine_boot() {
    let dir = tempfile::tempdir().unwrap();
    let (mut runtime, admission, controller) = fixture();
    let mut old = journal(dir.path());
    let (play, receipt) = begin(&mut runtime, &admission, &controller, &mut old);
    let mut current =
        DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("new-boot").unwrap()).unwrap();
    let mut completion = runtime
        .complete_play_with_recovery(&play, &receipt, DispatchOutcome::Unknown, &mut old)
        .unwrap();
    assert!(completion.observation.journal_pending());
    assert!(runtime.effect_is_current(&completion.stop.unwrap().unwrap()));
    assert_eq!(
        completion.observation.persist(&mut old),
        Err(PlaybackFailure::JournalUnavailable)
    );
    assert_eq!(
        completion.observation.persist(&mut current),
        Err(PlaybackFailure::JournalUnavailable)
    );
    assert_eq!(
        current
            .history(&admission.configuration.device, 10)
            .unwrap()[0]
            .outcome,
        None
    );
    assert!(!runtime.effect_is_current(&play));
    assert!(runtime.session().physical_stop_pending());
}

#[test]
fn historical_play_metadata_retry_cannot_rearm_after_reconnect() {
    let dir = tempfile::tempdir().unwrap();
    let (mut runtime, admission, controller) = fixture();
    let mut journal = journal(dir.path());
    let (play, receipt) = begin(&mut runtime, &admission, &controller, &mut journal);
    let mut completion = runtime
        .complete_play_with_recovery(&play, &receipt, DispatchOutcome::Acknowledged, &mut journal)
        .unwrap();
    runtime
        .reconnect(DeviceSessionId::new("fresh-session").unwrap())
        .unwrap();
    completion.observation.persist(&mut journal).unwrap();
    assert_eq!(runtime.session().state(), DeviceState::Disarmed);
    assert!(runtime.session().physical_stop_pending());
    assert!(runtime.arm(controller, admission.clone(), None).is_err());
    assert!(matches!(
        runtime.complete_play_with_recovery(
            &play,
            &receipt,
            DispatchOutcome::Unknown,
            &mut journal
        ),
        Err(PlaybackFailure::RetiredCommand)
    ));
    assert_eq!(
        journal
            .history(&admission.configuration.device, 10)
            .unwrap()[0]
            .outcome,
        Some(DispatchOutcome::Acknowledged)
    );
}

#[test]
fn stop_allocation_failure_does_not_discard_a_matched_play_observation() {
    let dir = tempfile::tempdir().unwrap();
    let (mut runtime, admission, controller) = fixture();
    let mut journal = journal(dir.path());
    let (play, receipt) = begin(&mut runtime, &admission, &controller, &mut journal);
    runtime.next_serial = u64::MAX;
    let completion = runtime
        .complete_play_with_recovery(&play, &receipt, DispatchOutcome::Unknown, &mut journal)
        .unwrap();
    assert_eq!(
        completion.stop,
        Err(PlaybackFailure::Domain(CoreError::Overflow))
    );
    assert!(!completion.observation.journal_pending());
    assert_eq!(completion.observation.outcome(), DispatchOutcome::Unknown);
    assert!(!runtime.effect_is_current(&play));
    assert_eq!(runtime.session().state(), DeviceState::StopRequested);
    assert!(runtime.session().physical_stop_pending());
    assert_eq!(
        journal
            .history(&admission.configuration.device, 10)
            .unwrap()[0]
            .outcome,
        Some(DispatchOutcome::Unknown)
    );
}

#[test]
fn conflicting_terminal_history_is_not_rewritten_or_replayed() {
    let dir = tempfile::tempdir().unwrap();
    let (mut runtime, admission, controller) = fixture();
    let mut journal = journal(dir.path());
    let (play, receipt) = begin(&mut runtime, &admission, &controller, &mut journal);
    journal
        .complete(&receipt, DispatchOutcome::Acknowledged)
        .unwrap();
    let mut completion = runtime
        .complete_play_with_recovery(&play, &receipt, DispatchOutcome::Unknown, &mut journal)
        .unwrap();
    assert!(completion.observation.journal_pending());
    assert!(runtime.effect_is_current(&completion.stop.unwrap().unwrap()));
    assert_eq!(
        completion.observation.persist(&mut journal),
        Err(PlaybackFailure::JournalUnavailable)
    );
    assert_eq!(
        journal
            .history(&admission.configuration.device, 10)
            .unwrap()[0]
            .outcome,
        Some(DispatchOutcome::Acknowledged)
    );
    assert!(!runtime.effect_is_current(&play));
}

#[test]
fn play_and_stop_observations_finish_while_a_journal_writer_is_blocked() {
    use std::sync::mpsc;
    use std::time::Duration;

    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<crate::device_effect_journal::EffectReceiptBinding>();
    let dir = tempfile::tempdir().unwrap();
    let (mut runtime, admission, controller) = fixture();
    let mut journal = journal(dir.path());
    let (play, receipt) = begin(&mut runtime, &admission, &controller, &mut journal);
    let binding = journal.receipt_binding();
    let db = rusqlite::Connection::open(journal_path(dir.path())).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    let (send, receive) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let completion =
            runtime.observe_play_completion(&play, &receipt, DispatchOutcome::Unknown, &binding);
        assert!(send.send((runtime, completion)).is_ok());
    });
    let observed = receive.recv_timeout(Duration::from_secs(3));
    // Release the writer before joining even if the regression times out.
    db.execute_batch("ROLLBACK").unwrap();
    worker.join().unwrap();
    let (mut runtime, completion) = observed.expect("play observation waited for journal I/O");
    let mut completion = completion.unwrap();
    assert!(completion.observation.journal_pending());
    assert_eq!(
        journal
            .history(&admission.configuration.device, 10)
            .unwrap()[0]
            .outcome,
        None
    );
    let stop = runtime
        .prepare_stop_durable(completion.stop.unwrap().unwrap(), &mut journal)
        .unwrap();
    let binding = journal.receipt_binding();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    let (send, receive) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let observation =
            runtime.observe_stop_completion(&stop, DispatchOutcome::Acknowledged, &binding);
        assert!(send.send((runtime, observation)).is_ok());
    });
    let observed = receive.recv_timeout(Duration::from_secs(3));
    db.execute_batch("ROLLBACK").unwrap();
    worker.join().unwrap();
    let (runtime, observation) = observed.expect("stop observation waited for journal I/O");
    let mut observation = observation.unwrap();
    assert!(observation.journal_pending());
    assert_eq!(runtime.session().state(), DeviceState::StopUnconfirmed);
    assert!(runtime.session().physical_stop_pending());
    assert!(journal
        .history(&admission.configuration.device, 10)
        .unwrap()
        .iter()
        .all(|x| x.outcome.is_none()));
    completion.observation.persist(&mut journal).unwrap();
    observation.persist(&mut journal).unwrap();
    assert_eq!(runtime.session().state(), DeviceState::StopUnconfirmed);
    let history = journal
        .history(&admission.configuration.device, 10)
        .unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(
        history.iter().find(|x| !x.record.stop).unwrap().outcome,
        Some(DispatchOutcome::Unknown)
    );
    assert_eq!(
        history.iter().find(|x| x.record.stop).unwrap().outcome,
        Some(DispatchOutcome::Acknowledged)
    );
}
