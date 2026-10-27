//! Provisional report metadata and provider honesty, not GPU execution.
use pulsar_protocol::*;
use sha2::{Digest, Sha256};
fn cpu() -> LiveCaptureReport {
    let mut report: LiveCaptureReport =
        serde_json::from_str(include_str!("fixtures/live-cpu-capture-report.json")).unwrap();
    report.options.inference.as_mut().unwrap().model_path =
        std::env::temp_dir().join("pulsar-fixture-model");
    report
}
fn completed() -> LiveCaptureReport {
    let mut report = cuda();
    report.active = false;
    report.retirement = Some(LiveCaptureRetirement::Completed);
    report.queue = None;
    report.admitted_pixel_bytes = 0;
    let last = report.last_frame.as_ref().unwrap();
    let info = report.inference.as_mut().unwrap();
    info.state = LiveCaptureInferenceState::Retired;
    info.latest = None;
    let artifact = |hash: char, size: u64| {
        let sha256 = hash.to_string().repeat(64);
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: size,
        }
    };
    info.cuda_evidence = Some(LiveCudaEvidenceRecord {
        schema_version: 1,
        qualified: false,
        manifest: artifact('b', 100),
        summary: LiveCudaStreamSummary {
            schema_version: 1,
            manifest_sha256: "b".repeat(64),
            successful_runs: info.completed,
            last_sequence: info.completed,
            last_frame: last.frame,
            last_source_time: last.source_time,
            transcript_sha256: "c".repeat(64),
        },
        receipt: artifact('d', 100),
        trace: artifact('e', 100),
    });
    report
}
#[test]
fn cuda_finishing_freezes_provisional_publication_and_requires_ready_work() {
    let mut report = cuda();
    report.inference.as_mut().unwrap().state = LiveCaptureInferenceState::Finishing;
    assert!(report.validate().is_err());
    report.inference.as_mut().unwrap().latest = None;
    report.validate().unwrap();
    for change in ["binding", "work", "cpu", "inflight"] {
        let mut bad = report.clone();
        match change {
            "binding" => bad.inference.as_mut().unwrap().cuda_manifest_sha256 = None,
            "work" => {
                let i = bad.inference.as_mut().unwrap();
                i.completed = 0;
                i.accepted = 0;
                i.stale = 0;
            }
            "cpu" => bad = cpu(),
            "inflight" => {
                bad.queue = Some(LiveQueueStatus {
                    connected: true,
                    received: bad.received_frames,
                    in_flight_frames: 1,
                    ..Default::default()
                });
            }
            _ => unreachable!(),
        }
        bad.inference.as_mut().unwrap().state = LiveCaptureInferenceState::Finishing;
        assert!(bad.validate().is_err(), "{change}");
    }
}
#[test]
fn completed_cuda_report_requires_matching_unqualified_retained_evidence() {
    let report = completed();
    report.validate().unwrap();
    for change in [
        "missing",
        "qualified",
        "manifest",
        "artifact",
        "bytes",
        "runs",
        "sequence",
        "frame",
        "source",
        "retirement",
        "phase",
        "elapsed",
    ] {
        let mut bad = report.clone();
        let info = bad.inference.as_mut().unwrap();
        match change {
            "missing" => info.cuda_evidence = None,
            "qualified" => info.cuda_evidence.as_mut().unwrap().qualified = true,
            "manifest" => {
                info.cuda_evidence.as_mut().unwrap().summary.manifest_sha256 = "f".repeat(64)
            }
            "artifact" => {
                info.cuda_evidence.as_mut().unwrap().receipt.artifact_id =
                    ArtifactId::new("other").unwrap()
            }
            "bytes" => info.cuda_evidence.as_mut().unwrap().trace.byte_len = 8 * 1024 * 1024 + 1,
            "runs" => info.cuda_evidence.as_mut().unwrap().summary.successful_runs = 0,
            "sequence" => info.cuda_evidence.as_mut().unwrap().summary.last_sequence += 1,
            "frame" => {
                info.cuda_evidence.as_mut().unwrap().summary.last_frame = FrameId::new(u64::MAX)
            }
            "source" => {
                info.cuda_evidence
                    .as_mut()
                    .unwrap()
                    .summary
                    .last_source_time = SourceTimestamp::new(i64::MAX, 1).unwrap()
            }
            "retirement" => bad.retirement = Some(LiveCaptureRetirement::Stopped),
            "phase" => info.state = LiveCaptureInferenceState::Finishing,
            "elapsed" => bad.server_elapsed = Some(LiveClockTick(u64::MAX)),
            _ => unreachable!(),
        }
        assert!(bad.validate().is_err(), "{change}");
    }
}
#[test]
fn completed_is_unavailable_to_cpu_or_capture_without_inference() {
    let mut report = cpu();
    report.active = false;
    report.queue = None;
    report.admitted_pixel_bytes = 0;
    report.retirement = Some(LiveCaptureRetirement::Completed);
    let i = report.inference.as_mut().unwrap();
    i.latest = None;
    i.state = LiveCaptureInferenceState::Retired;
    assert!(report.validate().is_err());
    report.inference = None;
    report.options.inference = None;
    assert!(report.validate().is_err());
}
fn cuda() -> LiveCaptureReport {
    let mut report = cpu();
    let profile = &mut report.options.inference.as_mut().unwrap().profile;
    profile.provider = InferenceProvider::Cuda;
    profile.schema_version = 2;
    profile.cuda_resources = Some(CudaResourceLimits {
        device_ordinal: 0,
        device_uuid: "GPU-12345678-1234-1234-1234-123456789abc".into(),
        address_space_bytes: 8 << 30,
        device_reservation_bytes: 96 << 20,
        arena_limit_bytes: 64 << 20,
    });
    let profile_sha256 = format!("{:x}", Sha256::digest(serde_json::to_vec(profile).unwrap()));
    let info = report.inference.as_mut().unwrap();
    info.profile_sha256 = profile_sha256.clone();
    info.cuda_manifest_sha256 = Some("b".repeat(64));
    let result = &mut info.latest.as_mut().unwrap().result;
    let LiveInferenceReply::Estimated {
        attempt_id,
        identity,
        input,
        position,
        pair_analysis_elapsed_ns,
        ..
    } = result.clone()
    else {
        panic!("CPU fixture");
    };
    *result = LiveInferenceReply::CudaProvisional {
        attempt_id,
        identity,
        profile_sha256,
        manifest_sha256: "b".repeat(64),
        input,
        position,
        pair_analysis_elapsed_ns,
    };
    report
}
#[test]
fn cpu_capture_bytes_remain_identical_and_omit_cuda_binding() {
    let report = cpu();
    report.validate().unwrap();
    let original: LiveCaptureReport =
        serde_json::from_slice(include_bytes!("fixtures/live-cpu-capture-report.json")).unwrap();
    assert_eq!(
        serde_json::to_vec(&original).unwrap(),
        include_bytes!("fixtures/live-cpu-capture-report.json")
    );
    assert!(!String::from_utf8(serde_json::to_vec(&report).unwrap())
        .unwrap()
        .contains("cuda"));
}
#[test]
fn cuda_provisional_report_requires_matching_manifest_and_provider() {
    let report = cuda();
    report.validate().unwrap();
    for change in [
        "missing",
        "changed",
        "malformed",
        "cpu-result",
        "qualified",
        "duration",
        "age",
        "profile",
    ] {
        let mut broken = report.clone();
        let info = broken.inference.as_mut().unwrap();
        match change {
            "missing" => info.cuda_manifest_sha256 = None,
            "changed" => info.cuda_manifest_sha256 = Some("c".repeat(64)),
            "malformed" => info.cuda_manifest_sha256 = Some("B".repeat(64)),
            "cpu-result" => {
                let LiveInferenceReply::CudaProvisional {
                    attempt_id,
                    identity,
                    profile_sha256,
                    input,
                    position,
                    pair_analysis_elapsed_ns,
                    ..
                } = info.latest.as_ref().unwrap().result.clone()
                else {
                    unreachable!()
                };
                info.latest.as_mut().unwrap().result = LiveInferenceReply::Estimated {
                    attempt_id,
                    identity,
                    profile_sha256,
                    input,
                    position,
                    pair_analysis_elapsed_ns,
                };
            }
            "qualified" => info.latest.as_mut().unwrap().qualified = true,
            "duration" => {
                if let LiveInferenceReply::CudaProvisional {
                    pair_analysis_elapsed_ns,
                    ..
                } = &mut info.latest.as_mut().unwrap().result
                {
                    *pair_analysis_elapsed_ns = 0;
                }
            }
            "age" => info.latest.as_mut().unwrap().capture_age_ns += 1,
            "profile" => {
                if let LiveInferenceReply::CudaProvisional { profile_sha256, .. } =
                    &mut info.latest.as_mut().unwrap().result
                {
                    *profile_sha256 = "c".repeat(64);
                }
            }
            _ => unreachable!(),
        }
        assert!(broken.validate().is_err(), "{change}");
    }
}
#[test]
fn cpu_report_cannot_claim_cuda_binding_or_provisional_result() {
    let mut report = cpu();
    report.inference.as_mut().unwrap().cuda_manifest_sha256 = Some("b".repeat(64));
    assert!(report.validate().is_err());
    let mut report = cpu();
    report
        .inference
        .as_mut()
        .unwrap()
        .latest
        .as_mut()
        .unwrap()
        .result = cuda().inference.unwrap().latest.unwrap().result;
    assert!(report.validate().is_err());
}
#[test]
fn cuda_startup_can_be_unbound_but_running_cannot() {
    let mut report = cuda();
    let info = report.inference.as_mut().unwrap();
    info.state = LiveCaptureInferenceState::Starting;
    info.cuda_manifest_sha256 = None;
    info.completed = 0;
    info.accepted = 0;
    info.latest = None;
    report.validate().unwrap();
    report.inference.as_mut().unwrap().state = LiveCaptureInferenceState::Running;
    assert!(report.validate().is_err());
}
#[test]
fn retired_cuda_metadata_retains_binding_but_cannot_keep_latest() {
    let mut report = cuda();
    report.active = false;
    report.retirement = Some(LiveCaptureRetirement::Stopped);
    report.admitted_pixel_bytes = 0;
    report.inference.as_mut().unwrap().state = LiveCaptureInferenceState::Retired;
    assert!(report.validate().is_err());
    report.inference.as_mut().unwrap().latest = None;
    report.validate().unwrap();
}

#[test]
fn finish_requested_is_cuda_only_and_requires_ready_work() {
    let mut report = cuda();
    let info = report.inference.as_mut().unwrap();
    info.state = LiveCaptureInferenceState::FinishRequested;
    info.latest = None;
    report.validate().unwrap();
    let mut empty = report.clone();
    let i = empty.inference.as_mut().unwrap();
    i.completed = 0;
    i.accepted = 0;
    i.stale = 0;
    assert!(empty.validate().is_err());
    empty.queue = Some(LiveQueueStatus {
        connected: true,
        received: empty.received_frames,
        in_flight_frames: 1,
        ..Default::default()
    });
    empty.validate().unwrap();
    let mut cpu = cpu();
    let i = cpu.inference.as_mut().unwrap();
    i.state = LiveCaptureInferenceState::FinishRequested;
    i.latest = None;
    assert!(cpu.validate().is_err());
    let mut latest = report.clone();
    latest.inference.as_mut().unwrap().latest = cuda().inference.unwrap().latest;
    assert!(latest.validate().is_err());
}
#[test]
fn public_finish_is_revision_scoped_and_gpu_metadata_does_not_enable_cpu_worker() {
    let command = Command::FinishLiveCapture {
        epoch: AttemptId::new("epoch").unwrap(),
    };
    assert_eq!(
        serde_json::to_value(&command).unwrap()["operation"],
        "finish_live_capture"
    );
    assert!(
        Request::new(RequestId::new("finish").unwrap(), command.clone())
            .validate()
            .is_err()
    );
    Request::new(RequestId::new("finish").unwrap(), command)
        .in_project(ProjectId::new("project").unwrap(), Some(RevisionId::new(0)))
        .validate()
        .unwrap();
    let mut manifest: LiveInferenceManifest =
        serde_json::from_str(include_str!("fixtures/live-cpu-manifest.json")).unwrap();
    manifest.model.path = std::env::temp_dir().join("pulsar-fixture-model");
    manifest.runtime.path = std::env::temp_dir().join("pulsar-fixture-runtime");
    assert!(manifest.validate_cuda_metadata().is_err());
    manifest.profile = cuda().options.inference.unwrap().profile;
    manifest.validate_cuda_metadata().unwrap();
    assert!(manifest.validate().is_err());
}
