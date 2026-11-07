use pulsar_protocol::*;
fn receipt() -> MotionRetimeReceipt {
    let spec = SourcePlacementSpec {
        source_version: SourceVersionId::new("source").unwrap(),
        source_start: SourceTimestamp::new(0, 1).unwrap(),
        source_end: SourceTimestamp::new(10, 1).unwrap(),
        source_anchor: SourceTimestamp::new(0, 1).unwrap(),
        project_anchor: ProjectTime::ZERO,
        rate_numerator: 1,
        rate_denominator: 1,
    };
    let from = SourcePlacement {
        placement_id: SourcePlacementId::new("old").unwrap(),
        created_revision: RevisionId::new(1),
        spec: spec.clone(),
    };
    let mut to = from.clone();
    to.placement_id = SourcePlacementId::new("new").unwrap();
    to.spec.rate_numerator = 2;
    let identity = ArtifactIdentity {
        artifact_id: ArtifactId::new(format!("motion.{}", "a".repeat(64))).unwrap(),
        sha256: "a".repeat(64),
        byte_len: 100,
    };
    MotionRetimeReceipt {
        schema_version: 1,
        project_id: ProjectId::new("project").unwrap(),
        base_revision: RevisionId::new(3),
        revision: RevisionId::new(4),
        session_id: SessionId::new("session").unwrap(),
        request_id: RequestId::new("request").unwrap(),
        actor: "actor".into(),
        operation_sha256: "b".repeat(64),
        from,
        to,
        axes: vec![Axis::Stroke],
        before: identity.clone(),
        after: identity,
        recorded_at_unix_ms: 1,
    }
}
#[test]
fn receipt_requires_exact_revision_source_identity_and_explicit_axes() {
    let r = receipt();
    r.validate().unwrap();
    let mut bad = r.clone();
    bad.revision = RevisionId::new(5);
    assert!(bad.validate().is_err());
    bad = r.clone();
    bad.to.spec.source_version = SourceVersionId::new("foreign").unwrap();
    assert!(bad.validate().is_err());
    bad = r.clone();
    bad.axes.push(Axis::Stroke);
    assert!(bad.validate().is_err());
    bad = r.clone();
    bad.before.artifact_id = ArtifactId::new("arbitrary").unwrap();
    assert!(bad.validate().is_err());
    bad = r;
    bad.to.created_revision = RevisionId::new(5);
    assert!(bad.validate().is_err());
}
#[test]
fn receipt_contract_is_strict_and_status_is_scoped_read_only() {
    let r = receipt();
    let mut json = serde_json::to_value(r).unwrap();
    json["qualified"] = true.into();
    assert!(serde_json::from_value::<MotionRetimeReceipt>(json).is_err());
    let command = Command::MotionRetimeStatus {
        revision: RevisionId::new(4),
    };
    assert!(command.requires_project());
    assert!(!command.requires_revision());
    assert!(!command.is_mutating());
}

#[test]
fn archival_correspondence_never_claims_local_edit_attribution() {
    let original = receipt();
    let mut from = original.from.clone();
    let mut to = original.to.clone();
    from.placement_id = SourcePlacementId::new("clone-old").unwrap();
    to.placement_id = SourcePlacementId::new("clone-new").unwrap();
    let archived = ArchivedMotionRetimeReceipt {
        schema_version: 1,
        project_id: ProjectId::new("clone").unwrap(),
        base_revision: original.base_revision,
        revision: original.revision,
        from,
        to,
        local_actor: "archive:namespace:actor".into(),
        origin_manifest: PackageObjectRef {
            sha256: "c".repeat(64),
            byte_len: 100,
        },
        origin_receipt: original,
        historical_attribution_only: true,
    };
    archived.validate().unwrap();
    let mut wrong = archived.clone();
    wrong.historical_attribution_only = false;
    assert!(wrong.validate().is_err());
    wrong = archived.clone();
    wrong.project_id = wrong.origin_receipt.project_id.clone();
    assert!(wrong.validate().is_err());
    wrong = archived.clone();
    wrong.from = wrong.origin_receipt.from.clone();
    assert!(wrong.validate().is_err());
    let mut json = serde_json::to_value(&archived).unwrap();
    json["qualified"] = true.into();
    assert!(serde_json::from_value::<ArchivedMotionRetimeReceipt>(json).is_err());
    let command = Command::ArchivedMotionRetimeStatus {
        revision: RevisionId::new(4),
    };
    assert!(command.requires_project());
    assert!(!command.requires_revision());
    assert!(!command.is_mutating());
}
