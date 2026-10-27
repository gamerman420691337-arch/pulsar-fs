use super::*;
use crate::handy_online::DeviceTimingRead;
const BASE: u64 = 1_000_000_000;
const SERVER: u64 = 1_800_000_000_000;

fn fixture() -> (
    HandyContinuousTimeReference,
    ContinuousTimingSamples,
    QualifiedContinuousTiming,
) {
    let value: HandyProgramCorrelation = serde_json::from_str(include_str!(
        "../../pulsar-protocol/tests/fixtures/handy-program-correlation.json"
    ))
    .unwrap();
    let mut before = value.inspection.device;
    let hssp = before.remote_hssp.as_mut().unwrap();
    hssp.play_state = HandyRemotePlayState::Playing;
    hssp.current_time_ms = 5004;
    let mut after = before.clone();
    after.remote_hssp.as_mut().unwrap().current_time_ms = 5010;
    let sample = |a, b, uptime| HandyDeviceTimingSample {
        info: HandyDeviceTimeInfo {
            device_uptime_ms: uptime,
            reported_clock_offset_ms: 100,
            last_sync_round_trip_ms: 5,
        },
        user_offset: HandyUserTimeOffset { offset_ms: -50 },
        read_started_boottime_ns: BASE + a,
        read_finished_boottime_ns: BASE + b,
    };
    (
        HandyContinuousTimeReference {
            start_time_ms: 5000,
            server_time_ms: SERVER,
            uncertainty_ns: 1_000_000,
        },
        ContinuousTimingSamples {
            device: PlayingTimingRead {
                device: DeviceTimingRead {
                    before,
                    first: sample(5_000_000, 6_000_000, 100),
                    second: sample(7_000_000, 8_000_000, 102),
                    after,
                },
                started_boottime_ns: BASE + 3_000_000,
                finished_boottime_ns: BASE + 12_000_000,
            },
            server_before: Sample {
                sent_ns: BASE,
                received_ns: BASE + 2_000_000,
                server_ms: SERVER + 1,
            },
            server_after: Sample {
                sent_ns: BASE + 13_000_000,
                received_ns: BASE + 15_000_000,
                server_ms: SERVER + 14,
            },
        },
        QualifiedContinuousTiming {
            contract: HandyContinuousTimingQualification {
                model: HandyContinuousTimingModel::HsspPlaytimeRequestBracket,
                timestamp_error_ns: 1_000_000,
                maximum_observation_age_ns: 1_000_000_000,
                maximum_sample_span_ns: 30_000_000,
                drift_ppm: 0,
            },
            maximum_total_error_ns: 20_000_000,
            server: QualifiedServerClock {
                contract: HandyServerClockQualification {
                    model: HandyServerClockModel::RequestBracketMonotonic,
                    timestamp_error_ns: 1_000_000,
                },
                max_age_ns: 2_000_000_000,
                max_uncertainty_ns: 20_000_000,
                drift_ppm: 0,
            },
        },
    )
}
fn create(
    r: HandyContinuousTimeReference,
    s: ContinuousTimingSamples,
    q: QualifiedContinuousTiming,
) -> Result<NativeContinuousTiming> {
    NativeContinuousTiming::new(
        ArtifactId::new("native-continuous-fixture").unwrap(),
        r,
        s,
        q,
    )
}
fn shift(s: &mut ContinuousTimingSamples, milliseconds: u64) {
    let ns = milliseconds * 1_000_000;
    for server in [&mut s.server_before, &mut s.server_after] {
        server.sent_ns += ns;
        server.received_ns += ns;
        server.server_ms += milliseconds;
    }
    let read = &mut s.device;
    read.started_boottime_ns += ns;
    read.finished_boottime_ns += ns;
    for sample in [&mut read.device.first, &mut read.device.second] {
        sample.read_started_boottime_ns += ns;
        sample.read_finished_boottime_ns += ns;
        sample.info.device_uptime_ms += milliseconds;
    }
    for sample in [&mut read.device.before, &mut read.device.after] {
        sample.remote_hssp.as_mut().unwrap().current_time_ms += milliseconds;
    }
}

#[test]
fn continuous_phase_uses_full_asymmetric_brackets_and_preserves_initial_error() {
    let (reference, samples, qualification) = fixture();
    let clock = create(reference, samples, qualification).unwrap();
    let at = clock.at(BASE + 15_000_000).unwrap();
    // Phase at the last receipt spans5012..5018ms. The independent server
    // interval spans epoch+13..17ms; the original reference contributes1ms.
    assert_eq!(at.observed_phase_ms, 5015);
    assert_eq!(at.phase_uncertainty_ns, 3_000_000);
    assert_eq!(at.estimated_server_ms, SERVER + 15);
    assert_eq!(at.server_uncertainty_ns, 2_000_000);
    assert_eq!(at.alignment_error_ns, 6_000_000);
    assert_eq!(at.valid_until_boottime_ns, BASE + 1_003_000_000);
    assert_eq!(
        clock.at(BASE + 15_500_000).unwrap().alignment_error_ns,
        8_000_000
    );
    assert_eq!(
        clock.at(BASE + 16_000_000).unwrap().alignment_error_ns,
        6_000_000
    );
    assert!(clock.at(BASE + 14_999_999).is_err());
    assert!(clock.at(at.valid_until_boottime_ns).is_err());
}

#[test]
fn continuous_phase_renews_only_from_new_overlapping_live_observations() {
    let (reference, samples, qualification) = fixture();
    let original = create(reference.clone(), samples.clone(), qualification.clone()).unwrap();
    let mut later = samples.clone();
    shift(&mut later, 200);
    let next = create(reference.clone(), later.clone(), qualification.clone()).unwrap();
    next.agrees_with(&original).unwrap();
    assert!(original.at(BASE + 1_100_000_000).is_err());
    assert_eq!(
        next.at(BASE + 1_100_000_000).unwrap().alignment_error_ns,
        6_000_000
    );
    assert_eq!(next.reference.uncertainty_ns, 1_000_000);
    assert!(original.agrees_with(&next).is_err());
    assert!(original.agrees_with(&original).is_err());
    let mut gap = samples;
    shift(&mut gap, 1100);
    assert!(create(reference.clone(), gap, qualification.clone())
        .unwrap()
        .agrees_with(&original)
        .is_err());
    let mut rewritten = reference;
    rewritten.uncertainty_ns = 0;
    assert!(create(rewritten, later, qualification)
        .unwrap()
        .agrees_with(&original)
        .is_err());
}

#[test]
fn continuous_phase_rejects_phase_jumps_even_inside_the_total_alignment_budget() {
    let (reference, samples, qualification) = fixture();
    let original = create(reference.clone(), samples.clone(), qualification.clone()).unwrap();
    let mut later = samples;
    shift(&mut later, 200);
    for sample in [
        &mut later.device.device.before,
        &mut later.device.device.after,
    ] {
        sample.remote_hssp.as_mut().unwrap().current_time_ms += 10;
    }
    let biased = create(reference, later, qualification).unwrap();
    assert_eq!(
        biased.at(biased.observed_at()).unwrap().alignment_error_ns,
        16_000_000
    );
    assert!(biased.agrees_with(&original).is_err());
}

#[test]
fn continuous_phase_rejects_context_clock_order_bounds_and_elapsed_source_age() {
    let (reference, samples, qualification) = fixture();
    for case in 0..13 {
        let mut bad = samples.clone();
        let mut q = qualification.clone();
        let mut r = reference.clone();
        match case {
            0 => bad.server_before.received_ns = bad.device.started_boottime_ns + 1,
            1 => bad.device.finished_boottime_ns = bad.device.started_boottime_ns - 1,
            2 => {
                bad.device.device.second.read_started_boottime_ns =
                    bad.device.device.first.read_finished_boottime_ns - 1
            }
            3 => bad.server_after.sent_ns = bad.device.finished_boottime_ns - 1,
            4 => bad.server_after.received_ns = bad.server_after.sent_ns - 1,
            5 => bad.device.device.second.info.reported_clock_offset_ms += 1,
            6 => bad.device.device.second.info.device_uptime_ms = 99,
            7 => bad.device.device.second.user_offset.offset_ms += 1,
            8 => bad.device.device.after.mode.mode_session_id += 1,
            9 => {
                bad.device
                    .device
                    .after
                    .remote_hssp
                    .as_mut()
                    .unwrap()
                    .play_state = HandyRemotePlayState::Stopped
            }
            10 => q.maximum_total_error_ns = 5_999_999,
            11 => {
                q.contract.maximum_observation_age_ns = 12_000_000;
                q.contract.maximum_sample_span_ns = 12_000_000;
            }
            _ => r.uncertainty_ns = u64::MAX,
        }
        assert!(create(r, bad, q).is_err(), "case {case}");
    }
}

#[test]
fn continuous_phase_drift_can_expire_error_budget_before_source_age() {
    let (reference, samples, mut qualification) = fixture();
    qualification.contract.drift_ppm = 1000;
    qualification.maximum_total_error_ns = 6_100_000;
    let clock = create(reference, samples, qualification).unwrap();
    assert!(clock.at(BASE + 16_000_000).is_ok());
    assert!(clock.at(BASE + 600_000_000).is_err());
    assert!(BASE + 600_000_000 < clock.valid_until_ns);
}

#[test]
fn continuous_phase_zero_and_extreme_inputs_do_not_wrap() {
    let (_, _, q) = fixture();
    let phase = Phase::new(0, 10, 10, &q.contract).unwrap();
    assert_eq!(phase.interval(10).unwrap(), (0, 1_000_000));
    assert!(phase.interval(9).is_err());
    assert!(Phase::new(MAX_MS + 1, 10, 10, &q.contract).is_err());
    let (mut reference, mut samples, qualification) = fixture();
    reference.server_time_ms = MAX_MS;
    assert!(create(reference, samples.clone(), qualification.clone()).is_err());
    samples.server_after.server_ms = MAX_MS + 1;
    assert!(create(fixture().0, samples, qualification).is_err());
}
