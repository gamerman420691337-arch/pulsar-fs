//! Authored protocol metadata only: no worker, browser, model or device is run.
use super::*;

fn artifact(value: u8) -> ArtifactIdentity {
    let sha256 = format!("{value:064x}");
    ArtifactIdentity {
        artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
        sha256,
        byte_len: 10,
    }
}
fn host_report() -> LiveCaptureReport {
    let report: LiveCaptureReport = serde_json::from_str(include_str!(
        "../../pulsar-protocol/tests/fixtures/live-cpu-capture-report.json"
    ))
    .unwrap();
    report.validate().unwrap();
    report
}
fn bind_captured_report(report: &mut LiveCaptureReport, manifest: LiveCpuInferenceManifest) {
    let inference = report.inference.as_mut().unwrap();
    let result = &mut inference.latest.as_mut().unwrap().result;
    let (input, position, pair_analysis_elapsed_ns) = match result.clone() {
        LiveInferenceReply::Estimated {
            input,
            position,
            pair_analysis_elapsed_ns,
            ..
        }
        | LiveInferenceReply::CapturedCpuEstimated {
            input,
            position,
            pair_analysis_elapsed_ns,
            ..
        } => (input, position, pair_analysis_elapsed_ns),
        _ => panic!("authored CPU estimate expected"),
    };
    let binding = manifest.reply_binding().unwrap();
    *result = LiveInferenceReply::CapturedCpuEstimated {
        attempt_id: manifest.stream.attempt_id.clone(),
        identity: manifest.stream.identity.clone(),
        profile_sha256: binding.profile_sha256().into(),
        manifest_sha256: binding.manifest_sha256().into(),
        input,
        position,
        pair_analysis_elapsed_ns,
    };
    inference.cpu_manifest = Some(Box::new(manifest));
    report.validate().unwrap();
}
fn captured_report() -> LiveCaptureReport {
    let mut report = host_report();
    let mut stream: LiveInferenceManifest = serde_json::from_str(include_str!(
        "../../pulsar-protocol/tests/fixtures/live-cpu-manifest.json"
    ))
    .unwrap();
    stream.model.path = LIVE_CPU_MODEL_PATH.into();
    stream.runtime.path = LIVE_CPU_RUNTIME_PATH.into();
    stream.profile = report.options.inference.as_ref().unwrap().profile.clone();
    let mut libraries: Vec<_> = CPU_RUNTIME_REQUIRED_ALIASES
        .iter()
        .enumerate()
        .map(|(index, name)| CapturedNativeLibrary {
            identity: if *name == "libonnxruntime.so.1" {
                stream.runtime.identity.clone()
            } else {
                artifact(index as u8 + 20)
            },
            names: vec![name.to_string()],
        })
        .collect();
    libraries.sort_by(|left, right| left.identity.sha256.cmp(&right.identity.sha256));
    let manifest = LiveCpuInferenceManifest::new(
        stream,
        CpuRuntimeImage {
            schema_version: 1,
            owner_policy: artifact(250),
            libraries,
        },
    )
    .unwrap();
    bind_captured_report(&mut report, manifest);
    report
}

#[test]
fn browser_cpu_estimate_acceptance_preserves_original_mode_and_input() {
    let host = host_report();
    let captured = captured_report();
    let host_mode = AnalysisCpuMode::from_report(&host).unwrap();
    let captured_mode = AnalysisCpuMode::from_report(&captured).unwrap();
    assert_eq!(
        host_mode.estimated_input(&host).unwrap(),
        captured_mode.estimated_input(&captured).unwrap()
    );
    assert!(host_mode.validate_report(&captured).is_err());
    assert!(host_mode.estimated_input(&captured).is_none());
    assert!(captured_mode.validate_report(&host).is_err());
    assert!(captured_mode.estimated_input(&host).is_none());
    let latest = captured
        .inference
        .as_ref()
        .unwrap()
        .latest
        .as_ref()
        .unwrap();
    assert!(!latest.qualified);
    assert_eq!(latest.evidence, EvidenceKind::Inferred);
}

#[test]
fn browser_cpu_starting_admission_binds_the_later_estimate_without_refreshing_mode() {
    let running = captured_report();
    let mut starting = running.clone();
    let inference = starting.inference.as_mut().unwrap();
    inference.state = LiveCaptureInferenceState::Starting;
    inference.latest = None;
    inference.completed = 0;
    inference.accepted = 0;
    inference.stale = 0;
    starting.validate().unwrap();
    let mode = AnalysisCpuMode::from_report(&starting).unwrap();
    assert!(mode.estimated_input(&starting).is_none());
    assert!(mode.estimated_input(&running).is_some());
    assert!(mode.validate_report(&host_report()).is_err());
}

#[test]
fn browser_cpu_rejects_self_consistent_replacement_manifest_from_fresh_status() {
    let original = captured_report();
    let mode = AnalysisCpuMode::from_report(&original).unwrap();
    let mut replacement = original.clone();
    let manifest = replacement
        .inference
        .as_ref()
        .unwrap()
        .cpu_manifest
        .as_ref()
        .unwrap();
    let mut image = manifest.image.clone();
    image.owner_policy = artifact(252);
    let changed = LiveCpuInferenceManifest::new(manifest.stream.clone(), image).unwrap();
    bind_captured_report(&mut replacement, changed);
    // Internal consistency is insufficient: status must name the original admission.
    replacement.validate().unwrap();
    assert!(mode.validate_report(&replacement).is_err());
    assert!(mode.estimated_input(&replacement).is_none());
    assert!(mode.estimated_input(&original).is_some());
}

#[test]
fn browser_cpu_estimates_require_captured_reply_identity_without_host_coercion() {
    let original = captured_report();
    let mode = AnalysisCpuMode::from_report(&original).unwrap();
    let host = host_report();
    for case in 0..6 {
        let mut changed = original.clone();
        let result = &mut changed
            .inference
            .as_mut()
            .unwrap()
            .latest
            .as_mut()
            .unwrap()
            .result;
        if case == 0 {
            *result = host
                .inference
                .as_ref()
                .unwrap()
                .latest
                .as_ref()
                .unwrap()
                .result
                .clone();
        } else {
            let LiveInferenceReply::CapturedCpuEstimated {
                manifest_sha256,
                profile_sha256,
                identity,
                attempt_id,
                pair_analysis_elapsed_ns,
                ..
            } = result
            else {
                unreachable!()
            };
            match case {
                1 => *manifest_sha256 = "b".repeat(64),
                2 => *profile_sha256 = "c".repeat(64),
                3 => identity.source = SourceVersionId::new("other-source").unwrap(),
                4 => *attempt_id = AttemptId::new("other-attempt").unwrap(),
                _ => *pair_analysis_elapsed_ns = 0,
            }
        }
        assert!(mode.validate_report(&changed).is_ok());
        assert!(mode.estimated_input(&changed).is_none(), "case {case}");
    }
}
