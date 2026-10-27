//! Authored mode-binding checks only; no HSP, device or native source is created.
use super::*;

fn reply(manifest: Option<&str>) -> LiveInferenceReply {
    let identity = LiveIdentity {
        epoch: AttemptId::new("authored-epoch").unwrap(),
        project: ProjectId::new("authored-project").unwrap(),
        revision: RevisionId::new(1),
        source: SourceVersionId::new("authored-source").unwrap(),
    };
    let pixels = ArtifactIdentity {
        artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
        sha256: "a".repeat(64),
        byte_len: LIVE_FRAME_BYTES as u64,
    };
    let input = LiveInferenceInput {
        sequence: 1,
        pair: RawFramePair {
            source_epoch: identity.epoch.clone(),
            previous_frame: FrameId::new(0),
            current_frame: FrameId::new(1),
            previous_time: SourceTimestamp::new(0, 1).unwrap(),
            current_time: SourceTimestamp::new(1, 10).unwrap(),
            presentation_clock: Some(LivePresentationClock {
                epoch: identity.epoch.clone(),
                source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                project_anchor: ProjectTime::ZERO,
                exact_project_anchor: None,
                presentation_anchor: LiveClockTick(0),
                added_buffer_ns: 0,
            }),
        },
        previous: pixels.clone(),
        current: pixels,
    };
    let attempt_id = AttemptId::new("authored-attempt").unwrap();
    let profile_sha256 = "b".repeat(64);
    let position = NormalizedPosition::new(0.5).unwrap();
    match manifest {
        None => LiveInferenceReply::Estimated {
            attempt_id,
            identity,
            profile_sha256,
            input,
            position,
            pair_analysis_elapsed_ns: 1,
        },
        Some(manifest_sha256) => LiveInferenceReply::CapturedCpuEstimated {
            attempt_id,
            identity,
            profile_sha256,
            input,
            position,
            manifest_sha256: manifest_sha256.into(),
            pair_analysis_elapsed_ns: 1,
        },
    }
}

#[test]
fn hsp_host_and_captured_cpu_modes_are_not_interchangeable() {
    let hash = "c".repeat(64);
    let host = reply(None);
    let captured = reply(Some(&hash));
    assert!(selected_cpu_estimate(&host, None).is_ok());
    assert!(selected_cpu_estimate(&captured, Some(&hash)).is_ok());
    assert!(selected_cpu_estimate(&host, Some(&hash)).is_err());
    assert!(selected_cpu_estimate(&captured, None).is_err());
}

#[test]
fn hsp_captured_cpu_rejects_runtime_manifest_substitution_with_equal_profile() {
    let original = "c".repeat(64);
    let replacement = "d".repeat(64);
    let first = reply(Some(&original));
    let changed = reply(Some(&replacement));
    assert_eq!(
        selected_cpu_estimate(&first, Some(&original))
            .unwrap()
            .profile_sha256,
        selected_cpu_estimate(&changed, Some(&replacement))
            .unwrap()
            .profile_sha256,
    );
    assert!(selected_cpu_estimate(&changed, Some(&original)).is_err());
    assert!(selected_cpu_estimate(&first, Some(&replacement)).is_err());
}

#[test]
fn hsp_extraction_borrows_original_scope_and_input_without_rewriting_variant() {
    let hash = "c".repeat(64);
    for expected in [None, Some(hash.as_str())] {
        let value = reply(expected);
        let bytes = serde_json::to_vec(&value).unwrap();
        let selected = selected_cpu_estimate(&value, expected).unwrap();
        let (attempt, identity, profile, input, position) = match &value {
            LiveInferenceReply::Estimated {
                attempt_id,
                identity,
                profile_sha256,
                input,
                position,
                ..
            }
            | LiveInferenceReply::CapturedCpuEstimated {
                attempt_id,
                identity,
                profile_sha256,
                input,
                position,
                ..
            } => (attempt_id, identity, profile_sha256, input, position),
            _ => unreachable!(),
        };
        assert!(std::ptr::eq(selected.attempt_id, attempt));
        assert!(std::ptr::eq(selected.identity, identity));
        assert!(std::ptr::eq(selected.profile_sha256, profile));
        assert!(std::ptr::eq(selected.input, input));
        assert!(std::ptr::eq(selected.position, position));
        assert_eq!(bytes, serde_json::to_vec(&value).unwrap());
    }
}

#[test]
fn hsp_cannot_use_ready_error_or_cuda_as_an_accepted_cpu_estimate() {
    let hash = "c".repeat(64);
    let LiveInferenceReply::CapturedCpuEstimated {
        attempt_id,
        identity,
        profile_sha256,
        manifest_sha256,
        input,
        position,
        pair_analysis_elapsed_ns,
    } = reply(Some(&hash))
    else {
        unreachable!()
    };
    let values = [
        LiveInferenceReply::Ready {
            attempt_id: attempt_id.clone(),
            identity: identity.clone(),
            profile_sha256: profile_sha256.clone(),
        },
        LiveInferenceReply::CapturedCpuReady {
            attempt_id: attempt_id.clone(),
            identity: identity.clone(),
            profile_sha256: profile_sha256.clone(),
            manifest_sha256: manifest_sha256.clone(),
        },
        LiveInferenceReply::CudaProvisional {
            attempt_id,
            identity,
            profile_sha256,
            manifest_sha256,
            input,
            position,
            pair_analysis_elapsed_ns,
        },
        LiveInferenceReply::Error(ProtocolError::invalid("authored failure")),
    ];
    for value in values {
        for expected in [None, Some(hash.as_str())] {
            assert!(selected_cpu_estimate(&value, expected).is_err());
        }
    }
}
