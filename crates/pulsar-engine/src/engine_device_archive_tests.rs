fn device_archive_profile() -> DynamicsProfile {
    DynamicsProfile {
        axes: vec![
            AxisDynamics {
                axis: Axis::Stroke,
                unit: "mm".into(),
                minimum: 0.0,
                maximum: 100.0,
                max_velocity: 1e6,
                max_acceleration: 1e6,
                max_jerk: 1e6,
            },
            AxisDynamics {
                axis: Axis::Roll,
                unit: "rad".into(),
                minimum: -1.0,
                maximum: 1.0,
                max_velocity: 1e6,
                max_acceleration: 1e6,
                max_jerk: 1e6,
            },
        ],
        coupled: vec![],
        minimum_gain: 0.0,
    }
}
#[test]
fn device_archive_capture_retains_composite_proposal_without_resolving_external_output() {
    let f = fixture();
    let actions = vec![
        MotionAction::new(
            ProjectTime::ZERO,
            NormalizedPosition::new(0.2).unwrap(),
            EvidenceKind::Synthesized,
        )
        .unwrap(),
        MotionAction::new(
            ProjectTime::from_nanos(1_000_000_000),
            NormalizedPosition::new(0.8).unwrap(),
            EvidenceKind::Synthesized,
        )
        .unwrap(),
    ];
    let motion = MotionProgram::new(vec![
        MotionTrack::new(Axis::Stroke, actions.clone()).unwrap(),
        MotionTrack::new(Axis::Roll, actions).unwrap(),
    ])
    .unwrap();
    f.engine
        .handle(request(
            &f.session,
            Some(&f.project),
            Some(0),
            commit_program(&f, 0, motion),
        ))
        .result
        .unwrap();
    let path = f.dir.path().join("device-proposal.json");
    let command = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::ExportDeviceMotion {
            path: path.clone(),
            profile: device_archive_profile(),
        },
    );
    let token = command.auth_token.clone().unwrap();
    f.engine.handle(command).result.unwrap();
    let bytes = fs::read(&path).unwrap();
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["motion"]["tracks"].as_array().unwrap().len(), 2);
    assert_eq!(value["admitted"], false);
    fs::remove_file(&path).unwrap();
    let result = project_packages::capture(
        &f.engine,
        &f.session,
        &token,
        &f.project,
        RevisionId::new(1),
    );
    assert!(
        result.is_ok(),
        "device-motion export must remain packagable: {:?}",
        result.err()
    );
    let plan = project_packages::capture(
        &f.engine,
        &f.session,
        &token,
        &f.project,
        RevisionId::new(1),
    )
    .unwrap();
    plan.manifest.validate().unwrap();
    assert_eq!(plan.manifest.export_receipts.len(), 1);
    let receipt = &plan.manifest.export_receipts[0];
    assert_eq!(
        serde_json::to_value(receipt).unwrap()["axis"],
        "device-motion"
    );
    assert_eq!(receipt.sha256, hash);
    assert_eq!(receipt.revision, RevisionId::new(1));
    assert!(matches!(receipt.state, PackageExportState::Complete));
    assert!(!path.exists());
}
