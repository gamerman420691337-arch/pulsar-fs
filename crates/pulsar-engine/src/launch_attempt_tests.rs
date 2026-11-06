//! Decisions at the actual post-retirement boundary. These unit tests do not
//! assert that a synthetic response proves a native provider or tree retirement.
use super::*;

fn fixture(root: &std::path::Path) -> WorkerRequest {
    let mut request: WorkerRequest =
        serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json")).unwrap();
    request.output_dir = root.canonicalize().unwrap();
    request
}
fn response(request: &WorkerRequest, classified: bool) -> WorkerResult {
    let WorkerOperation::Generate { settings, .. } = &request.operation else {
        panic!("finite-generation fixture required")
    };
    let profile = settings.inference_profile.as_ref().unwrap();
    WorkerResult {
        provider_unavailable: classified.then(|| InferenceProviderUnavailable {
            schema_version: 1,
            provider: profile.provider,
            profile_sha256: crate::inference_profiles::profile_identity(profile)
                .unwrap()
                .sha256,
            check: InferenceProviderAvailabilityCheck::NotListedByRuntime,
        }),
        cuda_artifacts: None,
        version: PROTOCOL_VERSION,
        attempt_id: request.attempt_id.clone(),
        job_id: request.job_id.clone(),
        result: Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "authored exact original worker error",
        )),
    }
}
fn finalize(
    request: &WorkerRequest,
    response: WorkerResult,
    cancelled: bool,
    deadline: Instant,
) -> AttemptReport {
    let mut settled_failure = None;
    let result = complete_retired_response(
        request,
        response,
        &AtomicBool::new(cancelled),
        deadline,
        false,
        None,
        &mut settled_failure,
    );
    AttemptReport {
        result,
        settled_failure,
    }
}
fn pending_deadline() -> Instant {
    Instant::now() + Duration::from_secs(3)
}

#[test]
fn settled_worker_failure_retains_exact_original_error_and_bound_provider_metadata() {
    let root = tempfile::tempdir().unwrap();
    let request = fixture(root.path());
    let response = response(&request, true);
    let original = response.result.as_ref().err().unwrap().clone();
    let evidence = serde_json::to_value(&response.provider_unavailable).unwrap();
    let (result, settled) = finalize(&request, response, false, pending_deadline()).into_parts();
    assert_eq!(result.err().unwrap(), original);
    let settled = settled.expect("validated worker failure was settled");
    assert_eq!(settled.error(), &original);
    assert_eq!(
        serde_json::to_value(settled.provider_unavailable()).unwrap(),
        evidence
    );
}

#[test]
fn older_unclassified_worker_error_is_settled_without_inference_reclassification() {
    let root = tempfile::tempdir().unwrap();
    let mut request = fixture(root.path());
    // Missing metadata must not introduce profile admission into the legacy
    // error path. This request represents an operation without a model.
    request.cuda_execution = None;
    request.tools.onnx_runtime = None;
    let WorkerOperation::Generate {
        settings, model, ..
    } = &mut request.operation
    else {
        unreachable!()
    };
    settings.inference_profile = None;
    settings.model_input = None;
    *model = None;
    let original = ProtocolError {
        code: ErrorCode::InvalidRequest,
        message: "exact older worker contract refusal".into(),
        retryable: false,
    };
    let response = WorkerResult {
        provider_unavailable: None,
        cuda_artifacts: None,
        version: PROTOCOL_VERSION,
        attempt_id: request.attempt_id.clone(),
        job_id: request.job_id.clone(),
        result: Err(original.clone()),
    };
    let (result, settled) = finalize(&request, response, false, pending_deadline()).into_parts();
    assert_eq!(result.err().unwrap(), original);
    let settled = settled.unwrap();
    assert_eq!(settled.error(), &original);
    assert!(settled.provider_unavailable().is_none());
}

#[test]
fn host_or_unconfirmed_retirement_error_cannot_manufacture_a_settled_worker_failure() {
    for code in [
        ErrorCode::Unavailable,
        ErrorCode::Cancelled,
        ErrorCode::DependencyMismatch,
    ] {
        let original = ProtocolError::new(code, "original host/transport/retirement failure");
        let (result, settled) = AttemptReport::host_failure(original.clone()).into_parts();
        assert_eq!(result.err().unwrap(), original);
        assert!(settled.is_none());
    }
}

#[test]
fn already_cancelled_real_launch_path_has_no_settled_marker_or_filesystem_effect() {
    let root = tempfile::tempdir().unwrap();
    let config = EngineConfig::new(
        root.path().join("state"),
        root.path().join("missing-worker"),
    );
    let mut request = fixture(root.path());
    request.output_dir = root.path().join("must-not-exist");
    let mut settled = None;
    let result = execute_inner(
        &config,
        &request,
        &AtomicBool::new(true),
        None,
        None,
        &mut settled,
    );
    assert_eq!(result.err().unwrap().code, ErrorCode::Cancelled);
    assert!(settled.is_none());
    assert!(!request.output_dir.exists());
    assert!(!config.state_dir.exists());
}

#[test]
fn final_cancel_or_original_deadline_refuses_even_valid_provider_metadata() {
    for cancelled in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let request = fixture(root.path());
        let deadline = if cancelled {
            pending_deadline()
        } else {
            Instant::now()
        };
        let (result, settled) =
            finalize(&request, response(&request, true), cancelled, deadline).into_parts();
        let error = result.err().unwrap();
        assert_eq!(
            error.code,
            if cancelled {
                ErrorCode::Cancelled
            } else {
                ErrorCode::Unavailable
            }
        );
        assert_eq!(
            error.message,
            if cancelled {
                "worker attempt cancelled"
            } else {
                "worker wall deadline exceeded"
            }
        );
        assert!(settled.is_none());
    }
}

#[test]
fn original_response_version_job_and_attempt_must_match_before_settlement() {
    for case in 0..3 {
        let root = tempfile::tempdir().unwrap();
        let request = fixture(root.path());
        let mut reply = response(&request, true);
        match case {
            0 => reply.version = PROTOCOL_VERSION.saturating_add(1),
            1 => reply.job_id = JobId::new("different-job").unwrap(),
            2 => reply.attempt_id = AttemptId::new("different-attempt").unwrap(),
            _ => unreachable!(),
        }
        let (result, settled) = finalize(&request, reply, false, pending_deadline()).into_parts();
        assert_eq!(result.err().unwrap().code, ErrorCode::DependencyMismatch);
        assert!(settled.is_none());
    }
}

#[test]
fn malformed_or_unbound_provider_metadata_cannot_acquire_settled_marker() {
    for case in 0..7 {
        let root = tempfile::tempdir().unwrap();
        let mut request = fixture(root.path());
        let mut reply = response(&request, true);
        match case {
            0 => reply.provider_unavailable.as_mut().unwrap().schema_version = 2,
            1 => reply.provider_unavailable.as_mut().unwrap().provider = InferenceProvider::Cpu,
            2 => reply.provider_unavailable.as_mut().unwrap().profile_sha256 = "0".repeat(64),
            3 => reply.result.as_mut().err().unwrap().code = ErrorCode::InvalidRequest,
            4 => reply.result.as_mut().err().unwrap().retryable = true,
            5 => {
                let WorkerOperation::Generate { settings, .. } = &mut request.operation else {
                    unreachable!()
                };
                settings.inference_profile = None;
            }
            6 => {
                let artifact = ArtifactReference {
                    identity: request.source.identity.clone(),
                    path: request.source.path.clone(),
                };
                reply.cuda_artifacts = Some(WorkerCudaArtifacts {
                    receipt: artifact.clone(),
                    trace: artifact,
                });
            }
            _ => unreachable!(),
        }
        let (result, settled) = finalize(&request, reply, false, pending_deadline()).into_parts();
        assert!(result.is_err(), "case {case}");
        assert!(settled.is_none(), "case {case}");
    }
}

#[test]
fn success_cannot_be_relabeled_as_provider_unavailable() {
    let root = tempfile::tempdir().unwrap();
    let request = fixture(root.path());
    let mut reply = response(&request, true);
    let artifact = ArtifactReference {
        identity: request.source.identity.clone(),
        path: request.source.path.clone(),
    };
    reply.result = Ok(WorkerOutput::Generated {
        program: artifact.clone(),
        receipt: artifact,
        lineage: vec![],
        review: vec![],
    });
    let (result, settled) = finalize(&request, reply, false, pending_deadline()).into_parts();
    assert!(result.is_err());
    assert!(settled.is_none());
}

#[test]
fn artifact_accounting_and_nonregular_output_fail_before_settled_marker() {
    for nonregular in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut request = fixture(root.path());
        let reply = response(&request, false);
        request.budget.output_bytes = 2;
        let path = root.path().join("unexpected-output");
        if nonregular {
            std::fs::create_dir(path).unwrap();
        } else {
            std::fs::write(path, b"123").unwrap();
        }
        let (result, settled) = finalize(&request, reply, false, pending_deadline()).into_parts();
        assert_eq!(
            result.err().unwrap().code,
            if nonregular {
                ErrorCode::Forbidden
            } else {
                ErrorCode::ResourceExhausted
            }
        );
        assert!(settled.is_none());
    }
}

#[test]
fn ordinary_failed_worker_cannot_carry_cuda_artifacts_into_settlement() {
    let root = tempfile::tempdir().unwrap();
    let request = fixture(root.path());
    let mut reply = response(&request, false);
    let artifact = ArtifactReference {
        identity: request.source.identity.clone(),
        path: request.source.path.clone(),
    };
    reply.cuda_artifacts = Some(WorkerCudaArtifacts {
        receipt: artifact.clone(),
        trace: artifact,
    });
    let (result, settled) = finalize(&request, reply, false, pending_deadline()).into_parts();
    assert_eq!(result.err().unwrap().code, ErrorCode::InvalidRequest);
    assert!(settled.is_none());
}

#[test]
fn result_only_adapter_preserves_the_original_worker_error() {
    let root = tempfile::tempdir().unwrap();
    let request = fixture(root.path());
    let reply = response(&request, true);
    let original = reply.result.as_ref().err().unwrap().clone();
    let error = finalize(&request, reply, false, pending_deadline())
        .into_result()
        .err()
        .unwrap();
    assert_eq!(error, original);
}
