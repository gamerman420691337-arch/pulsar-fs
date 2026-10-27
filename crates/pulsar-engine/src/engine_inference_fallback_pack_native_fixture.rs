// Include only inside inference_fallback_native_tests. SOFTWARE SIMULATION:
// public pack import/selection/withdrawal exercise a disposable test owner, never
// genuine human acceptance, rights approval, useful-model quality or equivalence.
// The parent test retains its explicit eligibility/provider-absence simulation.

struct NativeSelectedPack {
    choice: ModelPackChoice,
    operation: WorkerOperation,
    dependencies: Vec<ArtifactIdentity>,
    metadata: Value,
    supplier: tempfile::TempDir,
}
impl NativeSelectedPack {
    fn generation_command(&self, input: &Inputs) -> Result<Command> {
        let WorkerOperation::Generate {
            preset, settings, ..
        } = &self.operation
        else {
            anyhow::bail!("selected pack fixture has no generation operation");
        };
        self.choice.validate()?;
        input.selection.validate()?;
        Ok(Command::GenerateWithInferenceFallback {
            source_version: input.source.source_version.clone(),
            preset: preset.clone(),
            settings: settings.clone(),
            model: InferenceFallbackModel::ModelPack {
                choice: self.choice.clone(),
            },
            selection: Box::new(input.selection.clone()),
        })
    }

    /// The supplier is no longer a worker input after import/snapshotting. Call
    /// after the case's custody joins so cleanup remains explicit in its receipt.
    fn close(self) -> Result<()> {
        self.supplier
            .close()
            .context("remove simulated selected-pack supplier")
    }
}

fn native_pack_identity(bytes: &[u8]) -> Result<ArtifactIdentity> {
    let sha256 = fixture_hash(bytes);
    Ok(ArtifactIdentity {
        artifact_id: ArtifactId::new(sha256.clone())?,
        sha256,
        byte_len: bytes.len() as u64,
    })
}
fn native_pack_write(path: &Path, bytes: &[u8]) -> Result<ArtifactIdentity> {
    use std::os::unix::fs::MetadataExt;
    ensure!(
        !bytes.is_empty() && bytes.len() <= PACK_MANIFEST_LIMIT,
        "native pack declaration exceeds bound"
    );
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    ensure!(
        file.metadata()?.is_file() && file.metadata()?.nlink() == 1,
        "native pack declaration is not single-link regular storage"
    );
    native_pack_identity(bytes)
}

/// Copy only the exact pinned neutral model/runtime into a new private file.
/// This never hardlinks or changes supplier permissions, and never loads bytes.
fn native_pack_copy(source: &Path, destination: &Path, expected: &ArtifactIdentity) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    expected.validate()?;
    ensure!(
        expected.byte_len > 0 && expected.byte_len <= 256 * 1024 * 1024,
        "native pack asset exceeds bound"
    );
    let mut input = artifacts::open_regular(source)?;
    let before = input.metadata()?;
    ensure!(
        before.is_file() && before.len() == expected.byte_len,
        "native pack source length differs"
    );
    let stamp = |metadata: &fs::Metadata| {
        (
            metadata.dev(),
            metadata.ino(),
            metadata.len(),
            metadata.mode(),
            metadata.mtime(),
            metadata.mtime_nsec(),
            metadata.ctime(),
            metadata.ctime_nsec(),
        )
    };
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)?;
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        count = count
            .checked_add(read as u64)
            .context("native pack copy length overflow")?;
        ensure!(
            count <= expected.byte_len,
            "native pack asset grew during capture"
        );
        hash.update(&buffer[..read]);
        output.write_all(&buffer[..read])?;
    }
    ensure!(
        count == expected.byte_len && format!("{:x}", hash.finalize()) == expected.sha256,
        "native pack asset content differs"
    );
    let named = fs::symlink_metadata(source)?;
    ensure!(
        named.is_file()
            && !named.file_type().is_symlink()
            && stamp(&before) == stamp(&input.metadata()?)
            && stamp(&before) == stamp(&named),
        "native pack source changed during capture"
    );
    output.sync_all()?;
    let copied = output.metadata()?;
    ensure!(
        copied.is_file() && copied.len() == expected.byte_len && copied.nlink() == 1,
        "native pack copy is not exact single-link regular storage"
    );
    Ok(())
}

fn native_pack_require_dependencies(
    actual: &[ArtifactIdentity],
    expected: &[ArtifactIdentity],
) -> Result<()> {
    ensure!(
        expected.len() == 3 && actual.len() <= 256,
        "native pack dependency inventory differs"
    );
    let mut ids = std::collections::BTreeSet::new();
    for identity in expected {
        identity.validate()?;
        ensure!(
            identity.byte_len > 0
                && identity.artifact_id.as_str() == identity.sha256
                && ids.insert(identity.artifact_id.as_str()),
            "native pack expected identity differs"
        );
        let mut matching = actual
            .iter()
            .filter(|value| value.artifact_id == identity.artifact_id);
        ensure!(
            matching.next() == Some(identity) && matching.next().is_none(),
            "native pack original snapshot was lost, changed or duplicated"
        );
    }
    Ok(())
}

fn prepare_native_selected_pack(
    engine: &Arc<Engine>,
    input: &Inputs,
    label: &str,
) -> Result<NativeSelectedPack> {
    ensure!(
        !label.is_empty()
            && label.len() <= 48
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
        "native selected-pack case label differs"
    );
    input.from.validate()?;
    ensure!(
        input.source.identity.sha256 == MEDIA_SHA
            && input.model.identity.sha256 == MODEL_SHA
            && input.from.model == input.model.identity
            && input.from.runtime.sha256 == ORT_SHA,
        "selected-pack fixture requires the existing pinned neutral inputs"
    );
    let owner = engine
        .cpu_owner_policy
        .as_ref()
        .context("native pack fixture requires CPU owner policy")?;
    let runtime = owner.runtime_reference()?;
    ensure!(
        runtime.identity == input.from.runtime,
        "selected pack runtime differs from CPU owner image"
    );
    let ResponseBody::ModelPackSelection(before) = call(
        engine,
        &input.actor,
        None,
        None,
        Command::ModelPackSelectionStatus {
            owner_proof: engine.model_pack_owner_token.clone(),
        },
    )?
    else {
        anyhow::bail!("native pack selection status response missing");
    };
    before.validate()?;
    ensure!(
        before.choice.is_none() && before.selection_revision == 0,
        "native pack fixture requires fresh disposable owner selection"
    );

    let fixture_parent = engine
        .config
        .state_dir
        .parent()
        .context("native fixture parent missing")?
        .canonicalize()?;
    let supplier = tempfile::Builder::new()
        .prefix("simulated-native-pack-")
        .tempdir_in(&fixture_parent)?;
    let model_identity = input.model.identity.clone();
    let runtime_identity = runtime.identity.clone();
    let profile_bytes = serde_json::to_vec(&input.from)?;
    let license_bytes = b"SOFTWARE SIMULATION fixture notice. No redistribution, rights, model-quality, reviewer or human-acceptance claim is asserted by this test.";
    let profile_identity = native_pack_identity(&profile_bytes)?;
    let license_identity = native_pack_identity(license_bytes)?;
    let total = [
        model_identity.byte_len,
        runtime_identity.byte_len,
        profile_identity.byte_len,
        license_identity.byte_len,
    ]
    .into_iter()
    .try_fold(0u64, |sum, len| sum.checked_add(len))
    .context("native pack total overflow")?;
    ensure!(
        total <= PACK_TOTAL_LIMIT,
        "native pack total exceeds intake bound"
    );
    native_pack_copy(
        Path::new(MODEL),
        &supplier.path().join("model.onnx"),
        &model_identity,
    )?;
    native_pack_copy(
        &runtime.path,
        &supplier.path().join("runtime.so"),
        &runtime_identity,
    )?;
    native_pack_write(&supplier.path().join("profile.json"), &profile_bytes)?;
    native_pack_write(&supplier.path().join("LICENSE"), license_bytes)?;
    let assets = [
        ("model.onnx", PackAssetRole::Model, &model_identity),
        ("runtime.so", PackAssetRole::Runtime, &runtime_identity),
        ("profile.json", PackAssetRole::Profile, &profile_identity),
        ("LICENSE", PackAssetRole::License, &license_identity),
    ]
    .into_iter()
    .map(|(path, role, identity)| PackAsset {
        path: path.into(),
        role,
        byte_len: identity.byte_len,
        sha256: identity.sha256.clone(),
    })
    .collect();
    let manifest = ModelPackManifest {
        schema_version:1,
        pack_id:ArtifactId::new(format!("simulated-native-pack-{label}-{}",Uuid::new_v4()))?,
        target_os:"linux".into(),target_arch:"x86_64".into(),ort_api_version:24,
        labels:vec!["position".into()],
        assets,license_path:"LICENSE".into(),
        source_description:"SOFTWARE SIMULATION: pinned neutral model/runtime; no genuine owner acceptance, rights verification, scientific equivalence or quality qualification.".into(),
    };
    manifest.validate()?;
    let manifest_bytes = serde_json::to_vec(&manifest)?;
    let manifest_identity =
        native_pack_write(&supplier.path().join("manifest.json"), &manifest_bytes)?;
    fs::File::open(supplier.path())?.sync_all()?;

    let ResponseBody::ModelPack(imported) = call(
        engine,
        &input.actor,
        None,
        None,
        Command::ImportModelPack {
            source: supplier.path().to_owned(),
            owner_proof: engine.model_pack_owner_token.clone(),
        },
    )?
    else {
        anyhow::bail!("native pack import response missing");
    };
    imported.validate()?;
    ensure!(
        imported.manifest_sha256 == manifest_identity.sha256
            && imported.asset_count == 4
            && imported.total_bytes == total
            && !imported.active
            && !imported.native_execution_approved,
        "native pack integrity intake differs"
    );
    let selection_request = request(
        &input.actor,
        None,
        None,
        Command::SelectModelPack {
            manifest_sha256: Some(manifest_identity.sha256.clone()),
            expected_selection_revision: before.selection_revision,
            approve_unqualified_native_execution: true,
            owner_proof: engine.model_pack_owner_token.clone(),
        },
    );
    let ResponseBody::ModelPackSelection(selected) =
        engine.handle(selection_request.clone()).result?
    else {
        anyhow::bail!("native pack selection response missing");
    };
    selected.validate()?;
    let choice = selected
        .choice
        .clone()
        .context("native selected pack choice absent")?;
    ensure!(
        choice.manifest_sha256 == manifest_identity.sha256
            && choice.selection_revision == 1
            && selected.selected_by.as_ref() == Some(&input.actor)
            && selected.selection_request.as_ref() == Some(&selection_request.request_id)
            && selected
                .profile
                .as_ref()
                .is_some_and(|profile| fixture_bytes(profile) == profile_bytes)
            && selected.native_execution_approved
            && !selected.rights_verified
            && !selected.compatible
            && !selected.qualified,
        "native selected pack reply differs or promotes qualification"
    );
    let approval_identity = native_pack_identity(&serde_json::to_vec(selected.as_ref())?)?;
    let expected = vec![
        manifest_identity.clone(),
        profile_identity.clone(),
        approval_identity.clone(),
    ];
    let prepared = engine.prepare_selected_pack(&choice, settings(&input.from))?;
    ensure!(
        prepared.model.identity == input.model.identity
            && encode(&prepared.settings)? == encode(&settings(&input.from))?
            && prepared.dependencies == expected,
        "real selected-pack preparation changed model/settings or original three snapshots"
    );
    native_pack_require_dependencies(&prepared.dependencies, &expected)?;
    let metadata = json!({
        "software_simulation":true,"model_pack_owner_approval_simulated":true,
        "real_owner_acceptance_created":false,"public_equivalence_acceptance_called":false,
        "real_measured_equivalence":false,"qualified":false,"rights_verified":false,"physical_device":false,
        "pack_manifest":manifest_identity,"profile_document":profile_identity,"approval_document":approval_identity,
        "model":model_identity,"runtime":runtime_identity,"license_notice":license_identity,
        "import_report":imported,"selection_report":selected,"catalog_bound":false,
        "captured_pack_dependencies":expected,
        "scope":"Disposable test-owner pack import/selection and exact document capture only; no genuine human, rights, quality or equivalence acceptance."
    });
    Ok(NativeSelectedPack {
        choice,
        operation: WorkerOperation::Generate {
            preset: "default".into(),
            settings: Box::new(prepared.settings),
            model: Some(prepared.model),
        },
        dependencies: prepared.dependencies,
        metadata,
        supplier,
    })
}

fn verify_native_selected_pack_admission(
    engine: &Arc<Engine>,
    input: &Inputs,
    pack: &NativeSelectedPack,
    initial: &WorkerRequest,
) -> Result<Value> {
    initial.validate()?;
    ensure!(
        initial.project_id == input.project && initial.source.identity == input.source.identity,
        "selected pack initial source/project differs"
    );
    ensure!(
        encode(&initial.operation)? == encode(&pack.operation)?,
        "selected pack initial operation differs"
    );
    native_pack_require_dependencies(&initial.dependencies, &pack.dependencies)?;
    let observed = {
        let db = engine.db.lock().map_err(|_| anyhow::anyhow!("DB lock"))?;
        engine.fallback_pack_dependencies(&db, initial)?
    };
    ensure!(
        observed == pack.dependencies,
        "initial fallback lost its private selected-pack origin"
    );
    Ok(
        json!({"initial_attempt":initial.attempt_id,"private_pack_origin_verified":true,
        "original_pack_dependencies":observed,"model_pack_owner_approval_simulated":true,
        "qualified":false,"public_equivalence_acceptance_called":false}),
    )
}

fn withdraw_native_selected_pack(
    engine: &Arc<Engine>,
    input: &Inputs,
    pack: &NativeSelectedPack,
) -> Result<Value> {
    let ResponseBody::ModelPackSelection(current) = call(
        engine,
        &input.actor,
        None,
        None,
        Command::ModelPackSelectionStatus {
            owner_proof: engine.model_pack_owner_token.clone(),
        },
    )?
    else {
        anyhow::bail!("pack withdrawal selection status missing");
    };
    current.validate()?;
    ensure!(
        current.choice.as_ref() == Some(&pack.choice)
            && current.selection_revision == pack.choice.selection_revision,
        "native pack withdrawal must name the exact still-current choice"
    );
    let withdrawal = request(
        &input.actor,
        None,
        None,
        Command::WithdrawModelPackApproval {
            choice: pack.choice.clone(),
            expected_selection_revision: current.selection_revision,
            include_unclassified_generation: false,
            owner_proof: engine.model_pack_owner_token.clone(),
        },
    );
    let ResponseBody::ModelPackWithdrawal(report) = engine.handle(withdrawal.clone()).result?
    else {
        anyhow::bail!("pack withdrawal report missing");
    };
    report.validate_reply(&withdrawal)?;
    ensure!(
        report.cancelled_approved_jobs == 1
            && report.cancelled_unclassified_jobs == 0
            && report.selection.choice.is_none()
            && !report.selection.native_execution_approved,
        "native pack withdrawal did not cancel exactly its classified job"
    );
    Ok(
        json!({"software_simulation":true,"model_pack_owner_approval_simulated":true,
        "real_owner_acceptance_created":false,"public_equivalence_acceptance_called":false,
        "qualified":false,"physical_device":false,"withdrawal":report}),
    )
}

fn verify_native_selected_pack_success(
    engine: &Arc<Engine>,
    input: &Inputs,
    pack: &NativeSelectedPack,
    initial: &WorkerRequest,
    successor: &WorkerRequest,
    native_receipt: &Value,
    terminal: &JobSnapshot,
) -> Result<Value> {
    initial.validate()?;
    successor.validate()?;
    ensure!(
        initial.job_id == successor.job_id
            && initial.attempt_id != successor.attempt_id
            && initial.project_id == successor.project_id
            && initial.source.identity == successor.source.identity
            && initial.source.source_version == successor.source.source_version
            && initial.base_revision == successor.base_revision
            && terminal.job_id == successor.job_id
            && terminal.attempt_id == successor.attempt_id
            && terminal.state == "completed"
            && terminal.error.is_none(),
        "selected-pack successor identity/state differs"
    );
    native_pack_require_dependencies(&initial.dependencies, &pack.dependencies)?;
    native_pack_require_dependencies(&successor.dependencies, &pack.dependencies)?;
    let WorkerOperation::Generate {
        settings,
        model: Some(model),
        ..
    } = &successor.operation
    else {
        anyhow::bail!("selected-pack successor has no model generation");
    };
    ensure!(
        model.identity == input.model.identity
            && settings
                .inference_profile
                .as_ref()
                .is_some_and(|profile| fixture_bytes(profile) == fixture_bytes(&input.to)),
        "selected-pack successor changed model or exact target profile"
    );
    ensure!(
        native_receipt["qualification"] == "unqualified"
            && native_receipt["job_id"] == json!(successor.job_id)
            && native_receipt["attempt_id"] == json!(successor.attempt_id)
            && native_receipt["inference_execution"]["provider"] == "cpu"
            && native_receipt["inference_execution"]["successful_runs"]
                .as_u64()
                .is_some_and(|runs| runs > 0)
            && native_receipt["generation_progress"]["reused_analysed_samples"].as_u64() == Some(0),
        "selected-pack native successor receipt or fresh inference evidence differs"
    );
    let receipt_dependencies: Vec<ArtifactIdentity> = serde_json::from_value(
        native_receipt
            .get("dependencies")
            .context("selected-pack receipt dependencies missing")?
            .clone(),
    )?;
    native_pack_require_dependencies(&receipt_dependencies, &pack.dependencies)?;
    let candidate_id = terminal
        .candidate_id
        .as_ref()
        .context("selected-pack candidate missing")?;
    let (candidate_lineage, retained_pack) = {
        let db = engine.db.lock().map_err(|_| anyhow::anyhow!("DB lock"))?;
        let raw:String=db.query_row("SELECT CASE WHEN typeof(lineage)='text' AND length(CAST(lineage AS BLOB)) BETWEEN 1 AND 1048576 THEN lineage ELSE NULL END FROM candidates WHERE id=?1 AND job=?2 AND project=?3 AND base_revision=?4",params![candidate_id.as_str(),successor.job_id.as_str(),successor.project_id.as_str(),successor.base_revision.value()],|row|row.get(0))?;
        let lineage: Vec<ArtifactIdentity> = serde_json::from_str(&raw)?;
        (lineage, engine.fallback_pack_dependencies(&db, successor)?)
    };
    native_pack_require_dependencies(&candidate_lineage, &pack.dependencies)?;
    ensure!(
        retained_pack == pack.dependencies,
        "published successor lost original pack admission"
    );
    let receipt_path = successor.output_dir.join("receipt.json");
    ensure!(
        json_file(&receipt_path)? == *native_receipt,
        "selected-pack receipt changed after reading"
    );
    let receipt_identity = file_identity(&receipt_path)?;
    ensure!(
        candidate_lineage
            .iter()
            .filter(|identity| *identity == &receipt_identity)
            .count()
            == 1,
        "candidate lineage does not retain the exact native receipt"
    );
    let ResponseBody::Candidate(candidate) = call(
        engine,
        &input.actor,
        Some(&input.project),
        None,
        Command::GetCandidate {
            candidate_id: candidate_id.clone(),
        },
    )?
    else {
        anyhow::bail!("selected-pack public candidate missing");
    };
    ensure!(
        candidate.job_id.as_ref() == Some(&successor.job_id)
            && candidate.project_id == input.project
            && candidate.base_revision == successor.base_revision,
        "selected-pack public candidate scope differs"
    );
    let master = snapshot(engine, &input.actor, &input.project)?;
    ensure!(
        master.revision == input.original.revision
            && encode(&master.motion)? == encode(&input.original.motion)?,
        "selected-pack native generation altered master"
    );
    Ok(
        json!({"original_pack_dependencies":pack.dependencies,"initial_attempt":initial.attempt_id,
        "successor_attempt":successor.attempt_id,"receipt_identity":receipt_identity,
        "candidate_id":candidate_id,"receipt_dependency_count":receipt_dependencies.len(),
        "candidate_lineage_count":candidate_lineage.len(),"pack_provenance_preserved":true,
        "actual_cpu_successful_runs":native_receipt["inference_execution"]["successful_runs"],
        "master_preserved":true,"model_pack_owner_approval_simulated":true,
        "real_owner_acceptance_created":false,"public_equivalence_acceptance_called":false,"qualified":false}),
    )
}

/// Terminal metadata checks after the active barrier (when used), custody joins,
/// and public withdrawal. The barrier separately proves live native progress.
fn verify_native_selected_pack_withdrawal(
    engine: &Arc<Engine>,
    input: &Inputs,
    pack: &NativeSelectedPack,
    initial: &WorkerRequest,
    successor: Option<&WorkerRequest>,
    terminal: &JobSnapshot,
) -> Result<Value> {
    native_pack_require_dependencies(&initial.dependencies, &pack.dependencies)?;
    ensure!(
        terminal.job_id == initial.job_id
            && terminal.project_id == input.project
            && terminal.state == "cancelled"
            && terminal.candidate_id.is_none()
            && terminal.error.as_deref() == Some("model-pack approval withdrawn by host owner"),
        "selected-pack withdrawal terminal diagnostic differs"
    );
    if let Some(next) = successor {
        next.validate()?;
        native_pack_require_dependencies(&next.dependencies, &pack.dependencies)?;
        ensure!(
            next.job_id == initial.job_id
                && next.attempt_id != initial.attempt_id
                && terminal.attempt_id == next.attempt_id,
            "withdrawn successor identity differs"
        );
    } else {
        ensure!(
            terminal.attempt_id == initial.attempt_id,
            "pending pack withdrawal created a successor"
        );
    }
    let (attempts, candidates, pending, initial_state, current_state) = {
        let db = engine.db.lock().map_err(|_| anyhow::anyhow!("DB lock"))?;
        ensure!(
            engine.fallback_pack_dependencies(&db, initial).is_err(),
            "withdrawn pack still admits dependency inheritance"
        );
        (
            db.query_row(
                "SELECT COUNT(*) FROM attempts WHERE job=?1",
                [initial.job_id.as_str()],
                |row| row.get::<_, i64>(0),
            )?,
            db.query_row(
                "SELECT COUNT(*) FROM candidates WHERE job=?1",
                [initial.job_id.as_str()],
                |row| row.get::<_, i64>(0),
            )?,
            db.query_row(
                "SELECT COUNT(*) FROM inference_fallback_pending WHERE job=?1",
                [initial.job_id.as_str()],
                |row| row.get::<_, i64>(0),
            )?,
            db.query_row(
                "SELECT state FROM attempts WHERE id=?1 AND job=?2",
                params![initial.attempt_id.as_str(), initial.job_id.as_str()],
                |row| row.get::<_, String>(0),
            )?,
            db.query_row(
                "SELECT state FROM attempts WHERE id=?1 AND job=?2",
                params![terminal.attempt_id.as_str(), initial.job_id.as_str()],
                |row| row.get::<_, String>(0),
            )?,
        )
    };
    ensure!(
        attempts == if successor.is_some() { 2 } else { 1 }
            && candidates == 0
            && pending == 0
            && initial_state == "failed"
            && current_state
                == if successor.is_some() {
                    "cancelled"
                } else {
                    "failed"
                },
        "selected-pack withdrawal changed failed predecessor or published late work"
    );
    let ResponseBody::ModelPackSelection(current) = call(
        engine,
        &input.actor,
        None,
        None,
        Command::ModelPackSelectionStatus {
            owner_proof: engine.model_pack_owner_token.clone(),
        },
    )?
    else {
        anyhow::bail!("post-withdrawal pack selection response missing");
    };
    current.validate()?;
    ensure!(
        current.choice.is_none()
            && current.selection_revision == pack.choice.selection_revision + 1,
        "withdrawn pack selection was restored or advanced unexpectedly"
    );
    let master = snapshot(engine, &input.actor, &input.project)?;
    ensure!(
        master.revision == input.original.revision
            && encode(&master.motion)? == encode(&input.original.motion)?,
        "selected-pack withdrawal altered master"
    );
    Ok(
        json!({"attempt_count":attempts,"candidate_count":candidates,"pending_rows":pending,
        "failed_predecessor_preserved":true,"successor_record_present":successor.is_some(),
        "original_pack_dependencies":pack.dependencies,"terminal":terminal,"selection":current,
        "master_preserved":true,"model_pack_owner_approval_simulated":true,
        "real_owner_acceptance_created":false,"public_equivalence_acceptance_called":false,"qualified":false}),
    )
}

#[test]
fn native_pack_copy_helper_refuses_replaced_content_and_existing_destination() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::write(&source, b"neutral authored copy fixture").unwrap();
    let expected = native_pack_identity(b"neutral authored copy fixture").unwrap();
    native_pack_copy(&source, &target, &expected).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"neutral authored copy fixture");
    assert!(native_pack_copy(&source, &target, &expected).is_err());
    fs::write(&source, b"different authored bytes").unwrap();
    assert!(native_pack_copy(&source, &root.path().join("changed"), &expected).is_err());
}

#[test]
fn native_pack_membership_helper_requires_exact_unique_original_trio() {
    let expected = [
        b"manifest".as_slice(),
        b"profile".as_slice(),
        b"approval".as_slice(),
    ]
    .into_iter()
    .map(|bytes| native_pack_identity(bytes).unwrap())
    .collect::<Vec<_>>();
    let mut actual = expected.clone();
    actual.push(native_pack_identity(b"unrelated runtime").unwrap());
    native_pack_require_dependencies(&actual, &expected).unwrap();
    actual.push(expected[1].clone());
    assert!(native_pack_require_dependencies(&actual, &expected).is_err());
    assert!(native_pack_require_dependencies(&expected[..2], &expected).is_err());
    let mut changed = expected.clone();
    changed[1].byte_len += 1;
    assert!(native_pack_require_dependencies(&changed, &expected).is_err());
}
