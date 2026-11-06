use pulsar_protocol::*;
fn manifest() -> ModelPackManifest {
    ModelPackManifest {
        schema_version: 1,
        pack_id: ArtifactId::new("authored-fixture").unwrap(),
        target_os: "linux".into(),
        target_arch: "x86_64".into(),
        ort_api_version: 24,
        labels: vec!["fixture".into()],
        license_path: "LICENSE".into(),
        source_description: "Authored inert test bytes; no native runtime or trained model".into(),
        assets: [
            ("model", PackAssetRole::Model),
            ("runtime", PackAssetRole::Runtime),
            ("profile", PackAssetRole::Profile),
            ("LICENSE", PackAssetRole::License),
        ]
        .into_iter()
        .map(|(path, role)| PackAsset {
            path: path.into(),
            role,
            byte_len: 12,
            sha256: "a".repeat(64),
        })
        .collect(),
    }
}
#[test]
fn pack_paths_reject_traversal_aliases_and_reserved_names() {
    for path in [
        "",
        "/model",
        "../model",
        "x/../model",
        "C:/model",
        "x\\model",
        "CON.onnx",
        "x/LPT1.so",
        "x/model.",
        "MANIFEST.JSON",
        "Receipt.json/x",
    ] {
        assert!(validate_pack_path(path).is_err(), "{path}");
    }
    validate_pack_path("assets/model.onnx").unwrap();
    let mut m = manifest();
    m.assets[0].path = "a".into();
    m.assets[1].path = "a/runtime".into();
    assert!(m.validate().is_err());
    m = manifest();
    m.assets[0].path = "A/model".into();
    m.assets[1].path = "a/runtime".into();
    assert!(m.validate().is_err());
}
#[test]
fn pack_manifest_is_bounded_and_strict_without_execution_authority() {
    let m = manifest();
    m.validate().unwrap();
    let mut value = serde_json::to_value(&m).unwrap();
    value["activate"] = serde_json::Value::Bool(true);
    assert!(serde_json::from_value::<ModelPackManifest>(value).is_err());
    let text = serde_json::to_string(&m).unwrap().replacen(
        "\"schema_version\":1",
        "\"schema_version\":1,\"schema_version\":1",
        1,
    );
    assert!(serde_json::from_str::<ModelPackManifest>(&text).is_err());
    let mut m = m;
    m.assets[0].byte_len = 257 * 1024 * 1024;
    assert!(m.validate().is_err());
    m = manifest();
    m.labels.push("fixture".into());
    assert!(m.validate().is_err());
    let mut r = ModelPackReport {
        manifest_sha256: "a".repeat(64),
        pack_id: ArtifactId::new("fixture").unwrap(),
        asset_count: 4,
        total_bytes: 48,
        declared_target_matches_host: true,
        verified_at_unix_ms: 1,
        integrity_verified: true,
        native_execution_approved: false,
        rights_verified: false,
        compatible: false,
        qualified: false,
        active: false,
    };
    r.validate().unwrap();
    r.active = true;
    assert!(r.validate().is_err());
}
#[test]
fn pack_commands_are_global_with_only_import_mutating() {
    let import = Command::ImportModelPack {
        source: "/fixture".into(),
        owner_proof: "owner".into(),
    };
    let status = Command::ModelPackStatus {
        manifest_sha256: "a".repeat(64),
        owner_proof: "owner".into(),
    };
    assert!(!import.requires_project() && !import.requires_revision() && import.is_mutating());
    assert!(!status.requires_project() && !status.requires_revision() && !status.is_mutating());
    let discard = Command::DiscardModelPackStage {
        basename: ".pack-stage-authored".into(),
        manifest_sha256: "a".repeat(64),
        owner_proof: "owner".into(),
    };
    assert!(!discard.requires_project() && !discard.requires_revision() && discard.is_mutating());
    let staging = Command::ModelPackStagingStatus {
        owner_proof: "owner".into(),
    };
    assert!(!staging.requires_project() && !staging.requires_revision() && !staging.is_mutating());
}

#[test]
fn staging_observations_cannot_claim_unverified_identity_or_cleanup_authority() {
    let mut report = ModelPackStagingReport {
        schema_version: 1,
        observed_at_unix_ms: 1,
        stages: vec![ModelPackStageObservation {
            basename: ".pack-stage-authored".into(),
            state: ModelPackStageState::UnlockedRecognized,
            manifest_sha256: Some("a".repeat(64)),
        }],
        observation_only: true,
    };
    report.validate().unwrap();
    report.stages[0].manifest_sha256 = None;
    assert!(report.validate().is_err());
    report.stages[0].state = ModelPackStageState::Locked;
    report.validate().unwrap();
    report.observation_only = false;
    assert!(report.validate().is_err());
    report.observation_only = true;
    report.stages[0].basename = "../outside".into();
    assert!(report.validate().is_err());
    assert!(validate_pack_path(".PULSAR-STAGE.JSON").is_err());
}
