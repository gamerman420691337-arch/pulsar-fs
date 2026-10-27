use pulsar_protocol::*;

#[test]
fn selection_and_catalog_mutations_refuse_exhausted_revisions() {
    for revision in [i64::MAX as u64 - 1, i64::MAX as u64] {
        let owner_proof = "a".repeat(64);
        let commands = [
            Command::SelectModelPack {
                manifest_sha256: None,
                expected_selection_revision: revision,
                approve_unqualified_native_execution: false,
                owner_proof: owner_proof.clone(),
            },
            Command::SelectCatalogModelPack {
                manifest_sha256: "b".repeat(64),
                binding: ModelCatalogBinding {
                    bootstrap_sha256: "c".repeat(64),
                    catalog_revision: 1,
                },
                expected_selection_revision: revision,
                approve_unqualified_native_execution: true,
                owner_proof: owner_proof.clone(),
            },
            Command::WithdrawModelPackApproval {
                choice: ModelPackChoice {
                    manifest_sha256: "b".repeat(64),
                    selection_revision: 1,
                },
                expected_selection_revision: revision,
                include_unclassified_generation: false,
                owner_proof: owner_proof.clone(),
            },
            Command::RefreshModelCatalog {
                source: std::env::temp_dir().join("catalog"),
                bootstrap: std::env::temp_dir().join("bootstrap"),
                bootstrap_sha256: "c".repeat(64),
                expected_catalog_revision: revision,
                approve_bootstrap: true,
                owner_proof,
            },
        ];
        for command in commands {
            let request = Request::new(RequestId::new("revision-boundary").unwrap(), command);
            assert_eq!(request.validate().is_ok(), revision < i64::MAX as u64);
        }
    }
    // Reading an already exhausted catalog revision does not increment it.
    Request::new(
        RequestId::new("read-boundary").unwrap(),
        Command::CheckModelPackCatalog {
            bootstrap_sha256: "a".repeat(64),
            manifest_sha256: "b".repeat(64),
            expected_catalog_revision: i64::MAX as u64,
            owner_proof: "c".repeat(64),
        },
    )
    .validate()
    .unwrap();
}
