use pulsar_core::{ArtifactId, AttemptId, ProjectId, RequestId, RevisionId};
use pulsar_protocol::*;

fn spec() -> WasmEditSpec {
    let identity = ArtifactIdentity {
        artifact_id: ArtifactId::new("module").unwrap(),
        sha256: "a".repeat(64),
        byte_len: 8,
    };
    WasmEditSpec {
        manifest: ExtensionManifest {
            schema_version: PROTOCOL_VERSION,
            name: "fixture".into(),
            role: ExtensionRole::Editor,
            artifact: identity.clone(),
            abi: WASM_PROCESSING_ABI.into(),
            requested_scopes: vec![],
            confinement: ConfinementRequirement::Wasm,
            budget: ResourceBudget {
                memory_bytes: 256 * 1024 * 1024,
                output_bytes: WASM_BUFFER_MAX_BYTES as u64,
                wall_time_ms: 5000,
                cpu_threads: 2,
            },
        },
        module: vec![0, 97, 115, 109, 1, 0, 0, 0],
        guest_memory_bytes: 65536,
        expected_worker: identity,
        approve_execution: true,
        label: "proposal".into(),
    }
}
#[test]
fn execution_intent_requires_editor_approval_revision_and_no_effect_scopes() {
    let good = spec();
    good.validate().unwrap();
    let mut bad = good.clone();
    bad.approve_execution = false;
    assert!(bad.validate().is_err());
    bad = good.clone();
    bad.manifest.role = ExtensionRole::Processor;
    assert!(bad.validate().is_err());
    bad = good.clone();
    bad.manifest.requested_scopes = vec![Scope::Read];
    assert!(bad.validate().is_err());
    let mut request = Request::new(
        RequestId::new("invocation").unwrap(),
        Command::StartWasmEdit { spec: good },
    );
    request.project = Some(ProjectId::new("project").unwrap());
    assert!(request.validate().is_err());
    request.expected_revision = Some(RevisionId::new(0));
    request.validate().unwrap();
    let debug = format!("{:?}", request.command);
    assert!(!debug.contains("proposal"));
    assert!(!debug.contains("module:"));
}
#[test]
fn edit_status_is_bound_to_project_attempt_and_terminal_shape() {
    let mut request = Request::new(
        RequestId::new("poll").unwrap(),
        Command::WasmEditStatus {
            attempt_id: AttemptId::new("attempt").unwrap(),
        },
    );
    request.project = Some(ProjectId::new("project").unwrap());
    let mut report = WasmEditReport {
        attempt_id: AttemptId::new("attempt").unwrap(),
        request_id: RequestId::new("start").unwrap(),
        project_id: request.project.clone().unwrap(),
        base_revision: RevisionId::new(0),
        engine_epoch: "epoch".into(),
        state: WasmEditState::Running,
        candidate_id: None,
        receipt: None,
        error: None,
    };
    report.validate_for(&request).unwrap();
    report.attempt_id = AttemptId::new("other").unwrap();
    assert!(report.validate_for(&request).is_err());
    report.attempt_id = AttemptId::new("attempt").unwrap();
    report.project_id = ProjectId::new("other").unwrap();
    assert!(report.validate_for(&request).is_err());
    report.project_id = request.project.clone().unwrap();
    report.state = WasmEditState::Cancelled;
    assert!(report.validate().is_err());
    report.error = Some(ProtocolError::new(ErrorCode::Cancelled, "cancelled"));
    report.validate().unwrap();
    report.state = WasmEditState::Completed;
    assert!(report.validate().is_err());
}

#[test]
fn catalog_and_removal_bind_observed_operation_without_candidate_deletion() {
    let project = ProjectId::new("project").unwrap();
    let report = WasmEditReport {
        attempt_id: AttemptId::new("attempt").unwrap(),
        request_id: RequestId::new("original").unwrap(),
        project_id: project.clone(),
        base_revision: RevisionId::new(0),
        engine_epoch: "epoch".into(),
        state: WasmEditState::Failed,
        candidate_id: None,
        receipt: None,
        error: Some(ProtocolError::new(
            ErrorCode::InvalidRequest,
            "guest refused",
        )),
    };
    let mut request = Request::new(
        RequestId::new("remove").unwrap(),
        Command::RemoveUnsuccessfulWasmEdit {
            attempt_id: report.attempt_id.clone(),
            original_request_id: report.request_id.clone(),
            expected_state: report.state,
        },
    );
    assert!(request.validate().is_err());
    request.project = Some(project.clone());
    request.validate().unwrap();
    report.validate_removal_for(&request).unwrap();
    let mut changed = report.clone();
    changed.request_id = RequestId::new("other").unwrap();
    assert!(changed.validate_removal_for(&request).is_err());
    changed = report.clone();
    changed.state = WasmEditState::Interrupted;
    assert!(changed.validate_removal_for(&request).is_err());
    request.command = Command::ListWasmEdits;
    let mut catalog = WasmEditCatalog {
        project_id: project,
        operations: vec![report.clone()],
    };
    catalog.validate_for(&request).unwrap();
    catalog.operations.push(report.clone());
    assert!(catalog.validate_for(&request).is_err());
    catalog.operations = vec![report.clone(); 65];
    assert!(catalog.validate().is_err());
    catalog.operations = vec![report];
    catalog.project_id = ProjectId::new("other").unwrap();
    assert!(catalog.validate().is_err());
    for state in [WasmEditState::Running, WasmEditState::Completed] {
        request.command = Command::RemoveUnsuccessfulWasmEdit {
            attempt_id: AttemptId::new("attempt").unwrap(),
            original_request_id: RequestId::new("original").unwrap(),
            expected_state: state,
        };
        assert!(request.validate().is_err());
    }
    assert!(!Command::ListWasmEdits.is_mutating());
    assert!(Command::ListWasmEdits.requires_project());
}

#[test]
fn catalog_response_bytes_are_bounded_without_truncation() {
    let project = ProjectId::new("project").unwrap();
    let report = WasmEditReport {
        attempt_id: AttemptId::new("attempt").unwrap(),
        request_id: RequestId::new("start").unwrap(),
        project_id: project.clone(),
        base_revision: RevisionId::new(0),
        engine_epoch: "epoch".into(),
        state: WasmEditState::Failed,
        candidate_id: None,
        receipt: None,
        error: Some(ProtocolError::new(
            ErrorCode::Unavailable,
            "x".repeat(MAX_CONTROL_BYTES),
        )),
    };
    let catalog = WasmEditCatalog {
        project_id: project,
        operations: vec![report],
    };
    assert_eq!(
        catalog.validate().unwrap_err().code,
        ErrorCode::ResourceExhausted
    );
    assert_eq!(catalog.operations.len(), 1);
}

#[test]
fn archival_preserves_completed_shape_and_checks_original_scope_and_attempt() {
    let mut report = completed_report();
    let request = Request::new(
        RequestId::new("archive").unwrap(),
        Command::ArchiveCompletedWasmEdit {
            attempt_id: report.attempt_id.clone(),
            original_request_id: report.request_id.clone(),
        },
    )
    .in_project(report.project_id.clone(), None);
    request.validate().unwrap();
    assert!(request.command.is_mutating());
    report.validate_archive_for(&request).unwrap();
    let mut wrong = request.clone();
    wrong.project = Some(ProjectId::new("other").unwrap());
    assert!(report.validate_archive_for(&wrong).is_err());
    let mut wrong = request.clone();
    wrong.command = Command::ArchiveCompletedWasmEdit {
        attempt_id: report.attempt_id.clone(),
        original_request_id: RequestId::new("other").unwrap(),
    };
    assert!(report.validate_archive_for(&wrong).is_err());
    let body = ResponseBody::WasmEditArchived((report.clone()).into());
    let round: ResponseBody = serde_json::from_slice(&serde_json::to_vec(&body).unwrap()).unwrap();
    assert!(matches!(round, ResponseBody::WasmEditArchived(_)));
    report.state = WasmEditState::Failed;
    report.candidate_id = None;
    report.receipt = None;
    report.error = Some(ProtocolError::new(ErrorCode::Unavailable, "failed"));
    report.validate().unwrap();
    assert!(report.validate_archive_for(&request).is_err());
}

fn completed_report() -> WasmEditReport {
    let identity = spec().manifest.artifact;
    let program = ProgramDescriptor {
        artifact_id: identity.artifact_id.clone(),
        sha256: identity.sha256.clone(),
        byte_len: 8,
        codec: MotionCodec::MotionProgramJsonV1,
        axes: vec![],
    };

    WasmEditReport {
        attempt_id: AttemptId::new("attempt").unwrap(),
        request_id: RequestId::new("start").unwrap(),
        project_id: ProjectId::new("project").unwrap(),
        base_revision: RevisionId::new(0),
        engine_epoch: "epoch".into(),
        state: WasmEditState::Completed,
        candidate_id: Some(CandidateId::new("candidate").unwrap()),
        error: None,
        receipt: Some(WasmEditReceipt {
            schema_version: PROTOCOL_VERSION,
            request_sha256: "a".repeat(64),
            module: identity.clone(),
            worker: identity.clone(),
            base_program: program.clone(),
            input_sha256: "a".repeat(64),
            input_byte_len: 8,
            output_sha256: "a".repeat(64),
            output_byte_len: 8,
            result_program: program,
            edit_receipt: identity,
            fuel_consumed: 1,
            worker_peak_memory_bytes: 65536,
        }),
    }
}

#[test]
fn archive_pages_bind_limit_cursor_order_completion_and_continuation() {
    let a = completed_report();
    let mut b = a.clone();
    b.attempt_id = AttemptId::new("b-attempt").unwrap();
    b.request_id = RequestId::new("b-start").unwrap();
    let request = Request::new(
        RequestId::new("history").unwrap(),
        Command::ListArchivedWasmEdits {
            after: None,
            limit: 2,
        },
    )
    .in_project(a.project_id.clone(), None);
    request.validate().unwrap();
    assert!(!request.command.is_mutating());
    assert!(!request.command.requires_revision());
    let page = WasmArchivePage {
        project_id: a.project_id.clone(),
        after: None,
        limit: 2,
        operations: vec![a, b.clone()],
        next: Some(b.attempt_id.clone()),
    };
    page.validate_for(&request).unwrap();
    let round: ResponseBody = serde_json::from_slice(
        &serde_json::to_vec(&ResponseBody::WasmArchivePage(page.clone())).unwrap(),
    )
    .unwrap();
    assert!(matches!(round, ResponseBody::WasmArchivePage(_)));
    for limit in [0, 33, u16::MAX] {
        let mut bad = request.clone();
        bad.command = Command::ListArchivedWasmEdits { after: None, limit };
        assert!(bad.validate().is_err());
    }
    let mut bad = page.clone();
    bad.operations.reverse();
    assert!(bad.validate().is_err());
    bad = page.clone();
    bad.after = Some(b.attempt_id.clone());
    assert!(bad.validate().is_err());
    bad = page.clone();
    bad.next = Some(AttemptId::new("wrong").unwrap());
    assert!(bad.validate().is_err());
    bad = page.clone();
    bad.operations.pop();
    assert!(bad.validate().is_err());
    bad = page.clone();
    bad.operations[1] = bad.operations[0].clone();
    assert!(bad.validate().is_err());
    bad = page.clone();
    bad.operations[0].project_id = ProjectId::new("foreign").unwrap();
    assert!(bad.validate().is_err());
    bad = page.clone();
    bad.limit = 3;
    assert!(bad.validate().is_err());
    bad = page.clone();
    bad.next = None;
    bad.limit = 3;
    bad.validate().unwrap();
    assert!(bad.validate_for(&request).is_err());
    bad = page;
    bad.operations[0].state = WasmEditState::Running;
    bad.operations[0].candidate_id = None;
    bad.operations[0].receipt = None;
    assert!(bad.validate().is_err());
}

fn start_reference() -> WasmStartReference {
    let spec = spec();
    WasmStartReference {
        original_request_id: RequestId::new("original").unwrap(),
        base_revision: RevisionId::new(0),
        manifest: spec.manifest,
        worker: spec.expected_worker,
        guest_memory_bytes: spec.guest_memory_bytes,
        label: spec.label,
    }
}
#[test]
fn start_resolution_requires_bounded_metadata_project_and_distinct_effect_identity() {
    let reference = start_reference();
    reference.validate().unwrap();
    let project = ProjectId::new("project").unwrap();
    let id = reference.resolution_request_id(&project).unwrap();
    assert_ne!(id, reference.original_request_id);
    assert_eq!(id, reference.resolution_request_id(&project).unwrap());
    let request = Request::new(
        id,
        Command::ResolveWasmStart {
            reference: reference.clone(),
        },
    )
    .in_project(project, None);
    request.validate().unwrap();
    assert!(request.command.is_mutating());
    assert!(!request.command.requires_revision());
    let mut bad = request.clone();
    bad.project = None;
    assert!(bad.validate().is_err());
    bad = request.clone();
    bad.request_id = reference.original_request_id.clone();
    assert!(bad.validate().is_err());
    let mut bad = reference.clone();
    bad.manifest.role = ExtensionRole::Processor;
    assert!(bad.validate().is_err());
    bad = reference.clone();
    bad.manifest.requested_scopes = vec![Scope::Edit];
    assert!(bad.validate().is_err());
    bad = reference.clone();
    bad.manifest.artifact.byte_len = WASM_MODULE_MAX_BYTES as u64 + 1;
    assert!(bad.validate().is_err());
    bad = reference;
    bad.label = "changed".into();
    assert_ne!(
        request.request_id,
        bad.resolution_request_id(request.project.as_ref().unwrap())
            .unwrap()
    );
}
#[test]
fn resolution_outcomes_bind_original_reference_and_accepted_receipt_without_execution_approval() {
    let mut reference = start_reference();
    reference.original_request_id = RequestId::new("start").unwrap();
    let project = ProjectId::new("project").unwrap();
    let request = Request::new(
        reference.resolution_request_id(&project).unwrap(),
        Command::ResolveWasmStart {
            reference: reference.clone(),
        },
    )
    .in_project(project.clone(), None);
    let mut resolution = WasmStartResolution {
        project_id: project,
        reference: reference.clone(),
        outcome: WasmStartOutcome::Withdrawn,
    };
    resolution.validate_for(&request).unwrap();
    let body = ResponseBody::WasmStartResolution((resolution.clone()).into());
    let round: ResponseBody = serde_json::from_slice(&serde_json::to_vec(&body).unwrap()).unwrap();
    assert!(matches!(round, ResponseBody::WasmStartResolution(_)));
    resolution.reference.label = "foreign".into();
    assert!(resolution.validate_for(&request).is_err());
    resolution.reference = reference;
    resolution.outcome = WasmStartOutcome::Accepted {
        report: (completed_report()).into(),
    };
    resolution.validate_for(&request).unwrap();
    if let WasmStartOutcome::Accepted { report } = &mut resolution.outcome {
        report.receipt.as_mut().unwrap().worker.sha256 = "b".repeat(64);
    }
    assert!(resolution.validate_for(&request).is_err());
}
