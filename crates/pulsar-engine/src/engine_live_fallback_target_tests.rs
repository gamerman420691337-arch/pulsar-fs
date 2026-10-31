// These are authored cancellation handles on real Engine capture/stop commands.
// They establish no native provider absence or target execution qualification.
#[test]
fn stop_retired_fallback_ancestor_still_cancels_registered_target_workers() {
    for already_retired in [false, true] {
        let f = fixture();
        let capture = capture_report(f.engine.handle(capture_start(&f)).result.unwrap());
        let stop = || request(&f.session, Some(&f.project), Some(0),
            Command::StopLiveCapture { epoch: capture.identity.epoch.clone() });
        if already_retired {
            assert!(!capture_report(f.engine.handle(stop()).result.unwrap()).active);
        }
        let (original, target) = f.engine.live_captures.lock().unwrap()
            .fallback_worker_flags_for_test(&capture.identity.epoch);
        assert!(!target.load(Ordering::Acquire));
        let command = stop();
        assert!(!capture_report(f.engine.handle(command.clone()).result.unwrap()).active);
        assert!(original.load(Ordering::Acquire));
        assert!(target.load(Ordering::Acquire), "retired={already_retired}");
        assert!(!capture_report(f.engine.handle(command).result.unwrap()).active);
    }
}
