//! Authored schema-3 clocks and full portable clone lifecycle, no active generation grant.
use super::*;
use std::collections::BTreeMap;
pub(super) fn verify_native_portable(
    request: &WorkerRequest,
    program: &ArtifactReference,
    receipt: &ArtifactReference,
    lineage: &[ArtifactIdentity],
    review: &[ReviewFlag],
) {
    let raw_program = std::fs::read(&program.path).unwrap();
    let motion: MotionProgram = serde_json::from_slice(&raw_program).unwrap();
    let canonical = serde_json::to_vec(&motion).unwrap();
    let motion = crate::motion_artifacts::ProgramStore::validate_canonical(&canonical).unwrap();
    let motion_identity = ArtifactIdentity {
        artifact_id: motion.artifact_id.clone(),
        sha256: motion.sha256.clone(),
        byte_len: motion.byte_len,
    };
    let raw_receipt = std::fs::read(&receipt.path).unwrap();
    let mut retained_lineage = lineage.to_vec();
    retained_lineage.push(receipt.identity.clone());
    let clocks = crate::source_clock_receipts::load_verified_readers(
        request,
        &retained_lineage,
        &motion_identity,
        std::io::Cursor::new(&raw_receipt),
        raw_receipt.len() as u64,
        |identity, _| {
            assert_eq!(identity, &program.identity);
            Ok(Box::new(std::io::Cursor::new(raw_program.clone())))
        },
    )
    .unwrap();
    assert_eq!(clocks.plan.samples.len(), 28);
    assert_eq!(clocks.identity, receipt.identity);
    for (i, sample) in clocks.plan.samples.iter().enumerate() {
        assert_eq!(sample.source_frame_index, i as u64);
    }
    let mut bad: serde_json::Value = serde_json::from_slice(&raw_receipt).unwrap();
    bad["schema_version"] = 2.into();
    let bad = serde_json::to_vec(&bad).unwrap();
    let digest = format!("{:x}", Sha256::digest(&bad));
    let mut bad_lineage = lineage.to_vec();
    bad_lineage.push(ArtifactIdentity {
        artifact_id: ArtifactId::new(digest.clone()).unwrap(),
        sha256: digest,
        byte_len: bad.len() as u64,
    });
    assert!(crate::source_clock_receipts::load_verified_readers(
        request,
        &bad_lineage,
        &motion_identity,
        std::io::Cursor::new(&bad),
        bad.len() as u64,
        |_, _| panic!("downgrade must refuse before program")
    )
    .is_err());
    let WorkerOperation::Generate {
        settings,
        model: Some(model),
        ..
    } = &request.operation
    else {
        panic!()
    };
    let placement = settings.source_placement.clone().unwrap();
    let empty = b"{\"tracks\":[]}".to_vec();
    let empty_motion = crate::motion_artifacts::ProgramStore::validate_canonical(&empty).unwrap();
    let mut objects: BTreeMap<String, (PackageObjectDescriptor, Vec<u8>)> = BTreeMap::new();
    fn object(
        objects: &mut BTreeMap<String, (PackageObjectDescriptor, Vec<u8>)>,
        bytes: Vec<u8>,
        role: PackageObjectRole,
    ) -> PackageObjectRef {
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let byte_len = bytes.len() as u64;
        let record = objects.entry(sha256.clone()).or_insert_with(|| {
            (
                PackageObjectDescriptor {
                    sha256: sha256.clone(),
                    byte_len,
                    roles: vec![],
                },
                bytes.clone(),
            )
        });
        assert_eq!(record.1, bytes);
        if !record.0.roles.contains(&role) {
            record.0.roles.push(role);
            record.0.roles.sort();
        }
        PackageObjectRef { sha256, byte_len }
    }
    object(&mut objects, empty, PackageObjectRole::MotionProgram);
    object(&mut objects, canonical, PackageObjectRole::MotionProgram);
    object(
        &mut objects,
        raw_program,
        PackageObjectRole::DependencyEvidence,
    );
    object(&mut objects, raw_receipt, PackageObjectRole::WorkerReceipt);
    object(
        &mut objects,
        std::fs::read(&request.source.path).unwrap(),
        PackageObjectRole::SourceSnapshot,
    );
    object(
        &mut objects,
        std::fs::read(&model.path).unwrap(),
        PackageObjectRole::AnnotatedModel,
    );
    let manifest_ref = object(
        &mut objects,
        serde_json::to_vec(request).unwrap(),
        PackageObjectRole::WorkerManifest,
    );
    let candidate = PackageCandidate {
        rebased_from: None,
        project_interval: settings.project_interval,
        candidate_id: CandidateId::new("authored-candidate").unwrap(),
        base_revision: request.base_revision,
        motion: motion.clone(),
        job_origin: Some(request.job_id.clone()),
        lineage: retained_lineage,
        committed_revision: None,
        review: review.to_vec(),
    };
    let mut manifest = PortableProjectManifest {
        format_version: 11,
        origin_project_id: request.project_id.clone(),
        captured_revision: RevisionId::new(0),
        captured_event_cursor: 0,
        completeness: PackageCompleteness::default(),
        counts: PackageRecordCounts::default(),
        head: PackageProjectHead {
            name: "Authored annotated portable".into(),
            revision: RevisionId::new(0),
            history_cursor: 0,
            motion: empty_motion.clone(),
            protected: vec![],
        },
        revisions: vec![PackageRevision {
            revision: RevisionId::new(0),
            actor: "authored".into(),
            kind: "create".into(),
            label: "".into(),
            motion: empty_motion.clone(),
            protected: vec![],
        }],
        edit_states: vec![PackageEditState {
            position: 0,
            motion: empty_motion,
            protected: vec![],
            source_revision: None,
        }],
        revision_lineage: vec![],
        candidates: vec![candidate],
        authored_lineage: vec![],
        sources: vec![PackageSource {
            browser_fallback_history: None,
            source_version: request.source.source_version.clone(),
            kind: SourceKind::Media,
            label: "authored media".into(),
            identity: request.source.identity.clone(),
            pinned: true,
            evicted: false,
            stash_associations: vec![],
        }],
        legacy_cells: vec![],
        generated_origins: vec![PackageGeneratedOrigin {
            fallback_history: None,
            cpu_runtime: None,
            image_origin: None,
            annotated_model: Some(model.identity.clone()),
            cuda_evidence: None,
            job_id: request.job_id.clone(),
            attempt_id: request.attempt_id.clone(),
            base_revision: request.base_revision,
            source_version: request.source.source_version.clone(),
            manifest: manifest_ref,
            receipt: Some(receipt.identity.clone()),
            program: Some(program.identity.clone()),
            state: "completed".into(),
        }],
        export_receipts: vec![],
        objects: objects.values().map(|r| r.0.clone()).collect(),
        imported_origins: vec![],
        placements: Some(PackagePlacementClosure {
            definitions: vec![placement.clone()],
            active: vec![placement.placement_id.clone()],
            revisions: vec![PackageRevisionPlacements {
                revision: RevisionId::new(0),
                placements: vec![placement.placement_id.clone()],
            }],
            history: vec![PackageHistoryPlacements {
                position: 0,
                placements: vec![placement.placement_id.clone()],
            }],
        }),
        retiming_receipts: vec![],
    };
    manifest.counts = manifest.record_counts().unwrap();
    manifest.validate().unwrap();
    let pool = crate::resources::ResourcePool::new(4 * 1024 * 1024 * 1024, 2);
    let mut missing = manifest.clone();
    missing.generated_origins[0].annotated_model = None;
    missing
        .objects
        .retain(|o| !o.roles.contains(&PackageObjectRole::AnnotatedModel));
    missing.format_version = missing.required_format_version();
    missing.counts = missing.record_counts().unwrap();
    missing.validate().unwrap();
    assert!(
        crate::authority::project_package_imports::validate_evidence(
            &missing,
            &pool,
            |r| Ok(objects[&r.sha256].1.clone()),
            &mut || Ok(())
        )
        .is_err()
    );
    for mutation in ["trim", "order", "boundaries", "quantization"] {
        let mut plan = clocks.plan.clone();
        match mutation {
            "trim" => {
                plan.project_trim =
                    TimeRange::new(ProjectTime::from_nanos(0), ProjectTime::from_nanos(1)).unwrap()
            }
            "order" => plan.project_order.reverse(),
            "boundaries" => plan.uncovered_boundaries.clear(),
            _ => plan.samples[0].quantization.time = ProjectTime::from_nanos(1),
        }
        let mut value: serde_json::Value =
            serde_json::from_slice(&objects[&receipt.identity.sha256].1).unwrap();
        value["placed_generation_plan"] = serde_json::to_value(plan).unwrap();
        let data = serde_json::to_vec(&value).unwrap();
        let sha256 = format!("{:x}", Sha256::digest(&data));
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256: sha256.clone(),
            byte_len: data.len() as u64,
        };
        let mut bad = manifest.clone();
        bad.generated_origins[0].receipt = Some(identity.clone());
        *bad.candidates[0].lineage.last_mut().unwrap() = identity;
        let object = bad
            .objects
            .iter_mut()
            .find(|o| o.sha256 == receipt.identity.sha256)
            .unwrap();
        object.sha256 = sha256.clone();
        object.byte_len = data.len() as u64;
        bad.objects.sort_by(|a, b| a.sha256.cmp(&b.sha256));
        bad.validate().unwrap();
        assert!(
            crate::authority::project_package_imports::validate_evidence(
                &bad,
                &pool,
                |r| Ok(if r.sha256 == sha256 {
                    data.clone()
                } else {
                    objects[&r.sha256].1.clone()
                }),
                &mut || Ok(())
            )
            .is_err(),
            "{mutation}"
        );
    }
    let mut bytes = vec![];
    crate::project_package_format::write_package(
        &manifest,
        &mut bytes,
        |o| Ok(std::io::Cursor::new(objects[&o.sha256].1.clone())),
        Default::default(),
    )
    .unwrap();
    // Original current-attempt locators become unavailable before import/restart.
    std::fs::rename(
        &request.source.path,
        request.source.path.with_extension("unavailable"),
    )
    .unwrap();
    std::fs::rename(&model.path, model.path.with_extension("unavailable")).unwrap();
    let old = std::env::var_os("PULSAR_ANNOTATED_OLD_READER").map(std::path::PathBuf::from);
    crate::authority::project_package_imports::exercise_authored_annotated_portable_lifecycle(
        &bytes,
        &model.identity,
        old.as_deref(),
    );
}
