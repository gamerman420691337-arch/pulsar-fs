//! Host-owned approval withdrawal; private admission facts are not client capabilities.
use super::*;

// Deliberately outside the checked SessionId alphabet. Ordinary replay cannot reach it.
const ADMISSION_NAMESPACE: &str = "pulsar/generation-admission/v1";
const LIMIT: usize = 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Admission {
    schema_version: u16,
    job: JobId,
    project: ProjectId,
    origin: Origin,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Origin {
    Direct,
    Pack {
        choice: ModelPackChoice,
        approval: ArtifactIdentity,
        model: ArtifactIdentity,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dependencies: Option<Vec<ArtifactIdentity>>,
    },
}
fn mismatch(error: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        format!("generation admission: {error}"),
    )
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn check(admission: &Admission, manifest: &WorkerRequest) -> PResult<()> {
    if admission.schema_version != 1
        || admission.job != manifest.job_id
        || admission.project != manifest.project_id
    {
        return Err(mismatch("stored job scope changed"));
    }
    if let Origin::Pack {
        choice,
        approval,
        model,
        dependencies,
    } = &admission.origin
    {
        choice.validate()?;
        approval.validate()?;
        model.validate()?;
        if let Some(dependencies) = dependencies {
            check_captured_pack_dependencies(manifest, choice, approval, dependencies)?;
        }
        if !manifest.dependencies.contains(approval)
            || !matches!(&manifest.operation,WorkerOperation::Generate {model:Some(actual),..} if &actual.identity==model)
        {
            return Err(mismatch("pack admission differs from retained inputs"));
        }
    }
    Ok(())
}
fn check_captured_pack_dependencies(
    manifest: &WorkerRequest,
    choice: &ModelPackChoice,
    approval: &ArtifactIdentity,
    dependencies: &[ArtifactIdentity],
) -> PResult<()> {
    if dependencies.len() != 3
        || dependencies.last() != Some(approval)
        || dependencies[0].sha256 != choice.manifest_sha256
    {
        return Err(mismatch(
            "fallback pack requires its original manifest, profile and approval snapshots",
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for dependency in dependencies {
        dependency.validate()?;
        if dependency.artifact_id.as_str() != dependency.sha256
            || dependency.byte_len == 0
            || dependency.byte_len > 2 * 1024 * 1024
            || !seen.insert(&dependency.artifact_id)
            || manifest
                .dependencies
                .iter()
                .filter(|value| value.artifact_id == dependency.artifact_id)
                .count()
                != 1
            || !manifest.dependencies.contains(dependency)
        {
            return Err(mismatch("fallback pack snapshot membership differs"));
        }
    }
    Ok(())
}
fn require_fallback_reader(db: &Connection) -> PResult<()> {
    crate::inference_fallback_reader::version(db).map_err(mismatch)?;
    let raw = crate::live_cpu_reader::version(db).map_err(mismatch)?;
    if raw != 37 {
        return Err(mismatch("fallback pack provenance requires reader37"));
    }
    Ok(())
}
fn selection_snapshot(current: &ModelPackSelectionReport) -> PResult<ArtifactIdentity> {
    let bytes = serde_json::to_vec(current).map_err(internal)?;
    let sha256 = digest(&bytes);
    Ok(ArtifactIdentity {
        artifact_id: ArtifactId::new(sha256.clone()).map_err(internal)?,
        sha256,
        byte_len: bytes.len() as u64,
    })
}

pub(super) fn record_admission(
    tx: &Transaction<'_>,
    manifest: &WorkerRequest,
    request: &Request,
    extra: &[ArtifactIdentity],
) -> PResult<()> {
    if !matches!(manifest.operation, WorkerOperation::Generate { .. }) {
        return Ok(());
    }
    let selected = match &request.command {
        Command::GenerateFromModelPack { choice, .. } => Some((choice, None)),
        Command::GenerateWithInferenceFallback {
            model: InferenceFallbackModel::ModelPack { choice },
            selection,
            ..
        } => Some((choice, Some(selection.as_ref()))),
        _ => None,
    };
    let origin = if let Some((choice, fallback)) = selected {
        let WorkerOperation::Generate {
            model: Some(model), ..
        } = &manifest.operation
        else {
            return Err(mismatch("pack operation lacks model"));
        };
        let current = crate::model_pack_selection::status(tx).map_err(mismatch)?;
        if current.choice.as_ref() != Some(choice)
            || current.profile.as_ref().map(|profile| &profile.model) != Some(&model.identity)
        {
            return Err(mismatch("pack admission lacks exact current approval"));
        }
        let approval = extra
            .last()
            .ok_or_else(|| mismatch("pack approval snapshot absent"))?
            .clone();
        let dependencies = if let Some(selection) = fallback {
            require_fallback_reader(tx)?;
            let retained = super::inference_fallback_store::retained(tx, manifest)?
                .ok_or_else(|| mismatch("fallback pack admission lacks retained initial intent"))?;
            if retained.initial_attempt != manifest.attempt_id
                || retained.admission.kind
                    != super::inference_fallback_store::FallbackAdmissionKind::Initial
                || encode(&retained.selection)? != encode(selection)?
            {
                return Err(mismatch(
                    "fallback pack admission differs from initial selection",
                ));
            }
            check_captured_pack_dependencies(manifest, choice, &approval, extra)?;
            if approval != selection_snapshot(&current)? {
                return Err(mismatch(
                    "fallback pack approval snapshot differs from current selection",
                ));
            }
            Some(extra.to_vec())
        } else {
            None
        };
        Origin::Pack {
            choice: choice.clone(),
            approval,
            model: model.identity.clone(),
            dependencies,
        }
    } else {
        Origin::Direct
    };
    let admission = Admission {
        schema_version: 1,
        job: manifest.job_id.clone(),
        project: manifest.project_id.clone(),
        origin,
    };
    check(&admission, manifest)?;
    let raw = encode(&admission)?;
    tx.execute(
        "INSERT INTO requests(session,request,fingerprint,response) VALUES(?1,?2,?3,?4)",
        params![
            ADMISSION_NAMESPACE,
            manifest.job_id.as_str(),
            digest(raw.as_bytes()),
            raw
        ],
    )
    .map_err(internal)?;
    Ok(())
}
fn read_admission(db: &Connection, manifest: &WorkerRequest) -> PResult<Option<Admission>> {
    let row:Option<(String,String)>=db.query_row("SELECT CASE WHEN typeof(response)='text' AND length(CAST(response AS BLOB)) BETWEEN 1 AND 16384 THEN response ELSE NULL END,CASE WHEN typeof(fingerprint)='text' AND length(fingerprint)=64 THEN fingerprint ELSE NULL END FROM requests WHERE session=?1 AND request=?2",params![ADMISSION_NAMESPACE,manifest.job_id.as_str()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(mismatch)?;
    row.map(|(raw, hash)| {
        if digest(raw.as_bytes()) != hash {
            return Err(mismatch("receipt identity changed"));
        }
        let admission: Admission = decode(&raw).map_err(mismatch)?;
        if matches!(
            &admission.origin,
            Origin::Pack {
                dependencies: Some(_),
                ..
            }
        ) {
            require_fallback_reader(db)?;
            if encode(&admission)? != raw {
                return Err(mismatch("fallback pack admission is not canonical"));
            }
        }
        check(&admission, manifest)?;
        Ok(admission)
    })
    .transpose()
}
/// Return only original pack-document identities. Never inherit a predecessor's
/// runtime, GPU image, environment or attempt-specific dependencies. The caller
/// separately rechecks catalog approval through require_current_pack_choice.
pub(super) fn fallback_dependencies(
    db: &Connection,
    previous: &WorkerRequest,
) -> PResult<Vec<ArtifactIdentity>> {
    let admission = read_admission(db, previous)?
        .ok_or_else(|| mismatch("fallback job lacks private admission classification"))?;
    let Origin::Pack {
        choice,
        approval,
        model,
        dependencies,
    } = admission.origin
    else {
        return Ok(Vec::new());
    };
    let dependencies = dependencies
        .ok_or_else(|| mismatch("pack admission has no retained fallback snapshot set"))?;
    require_fallback_reader(db)?;
    let retained = super::inference_fallback_store::retained(db, previous)?
        .ok_or_else(|| mismatch("pack fallback ledger absent"))?;
    let initial_raw:String=db.query_row("SELECT CASE WHEN typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND 1048576 THEN manifest ELSE NULL END FROM main.attempts WHERE id=?1 AND job=?2",params![retained.initial_attempt.as_str(),previous.job_id.as_str()],|row|row.get(0)).map_err(mismatch)?;
    let initial: WorkerRequest = decode(&initial_raw).map_err(mismatch)?;
    initial.validate()?;
    if encode(&initial)? != initial_raw
        || initial.attempt_id != retained.initial_attempt
        || initial.job_id != previous.job_id
        || initial.project_id != previous.project_id
    {
        return Err(mismatch("fallback initial pack attempt identity differs"));
    }
    let original = read_admission(db, &initial)?
        .ok_or_else(|| mismatch("initial pack admission disappeared"))?;
    if !matches!(&original.origin,Origin::Pack {choice:original_choice,approval:original_approval,model:original_model,dependencies:Some(original_dependencies)} if original_choice==&choice && original_approval==&approval && original_model==&model && original_dependencies==&dependencies)
    {
        return Err(mismatch(
            "fallback pack snapshot set differs from initial admission",
        ));
    }
    let current = crate::model_pack_selection::status(db).map_err(mismatch)?;
    if current.choice.as_ref() != Some(&choice)
        || current.profile.as_ref().map(|profile| &profile.model) != Some(&model)
        || selection_snapshot(&current)? != approval
    {
        return Err(mismatch(
            "fallback pack approval is no longer current and exact",
        ));
    }
    Ok(dependencies)
}

fn cancel_job_for_withdrawal(tx: &Transaction<'_>, id: &str) -> PResult<()> {
    let job = JobId::new(id).map_err(mismatch)?;
    tx.execute("UPDATE jobs SET state='cancelled',error='model-pack approval withdrawn by host owner' WHERE id=?1",[id]).map_err(internal)?;
    // A failed predecessor is immutable history, not an active worker to cancel.
    tx.execute("UPDATE attempts SET state='cancelled' WHERE job=?1 AND state IN ('queued','running','interrupted','assistant_paused')",[id]).map_err(internal)?;
    super::inference_fallback_store::clear_pending(tx, &job)?;
    Ok(())
}

fn was_issued(
    db: &Connection,
    choice: &ModelPackChoice,
    current: &ModelPackSelectionReport,
) -> PResult<bool> {
    if current.choice.as_ref() == Some(choice) {
        return Ok(true);
    }
    // Only the bounded selection-response envelope is decoded, never a bearer request.
    let mut stmt=db.prepare("SELECT session,request,response FROM requests WHERE CASE WHEN typeof(response)='text' AND length(CAST(response AS BLOB))<=1048576 AND json_valid(response) THEN json_extract(response,'$.kind')='model_pack_selection' AND json_extract(response,'$.value.choice.manifest_sha256')=?1 AND json_extract(response,'$.value.choice.selection_revision')=?2 ELSE 0 END LIMIT 2").map_err(internal)?;
    let rows = stmt
        .query_map(
            params![choice.manifest_sha256, choice.selection_revision as i64],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .map_err(internal)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(internal)?;
    if rows.len() > 1 {
        return Err(mismatch("duplicate issued approval"));
    }
    let Some((session, request, raw)) = rows.first() else {
        return Ok(false);
    };
    let ResponseBody::ModelPackSelection(report) = decode(raw)? else {
        return Err(mismatch("approval envelope changed"));
    };
    report.validate()?;
    if report.choice.as_ref() != Some(choice)
        || report.selected_by.as_ref().map(SessionId::as_str) != Some(session.as_str())
        || report.selection_request.as_ref().map(RequestId::as_str) != Some(request.as_str())
    {
        return Err(mismatch("issued approval identity changed"));
    }
    Ok(true)
}
impl Engine {
    /// Recheck current host catalog approval as well as the stored provenance.
    /// Call under the caller's admission transaction/artifact protection.
    pub(super) fn fallback_pack_dependencies(
        &self,
        db: &Connection,
        previous: &WorkerRequest,
    ) -> PResult<Vec<ArtifactIdentity>> {
        let dependencies = fallback_dependencies(db, previous)?;
        let admission = read_admission(db, previous)?
            .ok_or_else(|| mismatch("fallback private admission disappeared"))?;
        match admission.origin {
            Origin::Direct if dependencies.is_empty() => {}
            Origin::Pack {
                choice,
                dependencies: Some(retained),
                ..
            } if retained == dependencies => {
                self.require_current_pack_choice(db, &choice)?;
            }
            _ => {
                return Err(mismatch(
                    "fallback private admission changed during catalog recheck",
                ))
            }
        }
        Ok(dependencies)
    }

    pub(super) fn model_pack_withdrawal_command(
        &self,
        request: &Request,
        session: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        let Command::WithdrawModelPackApproval {
            choice,
            expected_selection_revision,
            include_unclassified_generation,
            ..
        } = &request.command
        else {
            return Err(ProtocolError::invalid("not a pack withdrawal"));
        };
        let _gate = self.artifact_gate.lock().map_err(internal)?;
        let _memory = self.pool.reserve_memory(64 * 1024 * 1024)?;
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(internal)?;
        self.authorize(&tx, request, session)?;
        if let Some(reply) = replay(&tx, session.as_str(), request, fingerprint)? {
            return Ok(reply);
        }
        let current = crate::model_pack_selection::status(&tx).map_err(mismatch)?;
        if current.selection_revision != *expected_selection_revision {
            return Err(mismatch(
                "current selection changed; refresh before withdrawal",
            ));
        }
        if !was_issued(&tx, choice, &current)? {
            return Err(mismatch("approval was not issued by this engine"));
        }
        let mut approved = Vec::new();
        let mut unknown = Vec::new();
        {
            let mut stmt=tx.prepare("SELECT CASE WHEN typeof(id)='text' AND length(id) BETWEEN 1 AND 128 THEN id ELSE NULL END,CASE WHEN typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND ?1 THEN manifest ELSE NULL END FROM jobs WHERE state IN ('queued','running','interrupted','fallback_pending') ORDER BY id LIMIT 1025").map_err(internal)?;
            let mut rows = stmt.query([MAX_CONTROL_BYTES as i64]).map_err(internal)?;
            let mut scanned = 0;
            while let Some(row) = rows.next().map_err(mismatch)? {
                scanned += 1;
                if scanned > LIMIT {
                    return Err(ProtocolError::new(ErrorCode::ResourceExhausted,"withdrawal pending-job inventory exceeds 1024; reduce it explicitly before retry"));
                }
                let id: String = row.get(0).map_err(mismatch)?;
                let raw: String = row.get(1).map_err(mismatch)?;
                let manifest: WorkerRequest = decode(&raw).map_err(mismatch)?;
                manifest.validate()?;
                if manifest.job_id.as_str() != id {
                    return Err(mismatch("job manifest identity changed"));
                }
                if !matches!(manifest.operation, WorkerOperation::Generate { .. }) {
                    continue;
                }
                match read_admission(&tx, &manifest)? {
                    Some(Admission {
                        origin:
                            Origin::Pack {
                                choice: original, ..
                            },
                        ..
                    }) if original == *choice => approved.push(id),
                    Some(_) => {}
                    None => unknown.push(id),
                }
            }
        }
        if !include_unclassified_generation && !unknown.is_empty() {
            return Err(ProtocolError::new(ErrorCode::Unavailable,format!("{} unfinished generation jobs lack admission classification; cancel them individually or explicitly include unclassified generation",unknown.len())));
        }
        if !include_unclassified_generation {
            unknown.clear();
        }
        let selection = if current.choice.as_ref() == Some(choice) {
            crate::model_pack_selection::select_bound(
                &tx,
                *expected_selection_revision,
                None,
                None,
                session,
                &request.request_id,
            )?
        } else {
            current
        };
        let mut flags = Vec::new();
        {
            let workers = self.workers.lock().map_err(internal)?;
            for id in approved.iter().chain(unknown.iter()) {
                cancel_job_for_withdrawal(&tx, id)?;
                if let Some(flag) = workers.get(id) {
                    flags.push(flag.clone());
                }
            }
        }
        let report = ModelPackWithdrawalReport {
            schema_version: 1,
            choice: choice.clone(),
            selection,
            cancelled_approved_jobs: approved.len() as u32,
            cancelled_unclassified_jobs: unknown.len() as u32,
        };
        report.validate_reply(request)?;
        let body = ResponseBody::ModelPackWithdrawal(Box::new(report));
        remember(&tx, session.as_str(), request, fingerprint, &body)?;
        tx.commit().map_err(internal)?;
        for flag in flags {
            flag.store(true, Ordering::Release);
        }
        Ok(body)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod fallback_pack_tests {
    use super::super::inference_fallback_store::{FallbackAdmission, FallbackAdmissionKind};
    use super::*;

    // Authored metadata only. No model, native runtime, owner API, measured
    // equivalence or device is executed or accepted by these fixtures.
    fn fixture_identity(n: u8) -> ArtifactIdentity {
        let sha256 = format!("{n:064x}");
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: 1,
        }
    }

    fn fixture_reference(n: u8) -> InferenceFallbackReference {
        InferenceFallbackReference {
            dossier_id: ArtifactId::new(format!("{n:064x}")).unwrap(),
            expected_revision: 1,
            expected_report_sha256: format!("{n:064x}"),
        }
    }

    fn fixture_selection() -> InferenceFallbackSelection {
        InferenceFallbackSelection {
            schema_version: 1,
            equivalents: vec![fixture_reference(1), fixture_reference(2)],
            maximum_fallbacks: 2,
            source_policy: InferenceFallbackSourcePolicy {
                schema_version: 1,
                kind: InferenceFallbackSourceKind::DecodedMedia,
                sources: None,
                independent_audio: InferenceFallbackIndependentAudio::Any,
            },
            view_policy: InferenceFallbackViewPolicy {
                schema_version: 1,
                applicability: InferenceFallbackViewApplicability::PreserveRequestedSemantics,
            },
        }
    }

    fn fixture_profile(runtime: u8) -> DeclaredInferenceProfile {
        DeclaredInferenceProfile {
            schema_version: 1,
            model: fixture_identity(2),
            runtime: fixture_identity(runtime),
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 3,
                    layout: TensorLayout::Nchw,
                    class_count: 1,
                    decoder: DetectorDecoder::YoloChannelMajor,
                },
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 1,
            minimum_output_bytes: 1,
            cpu_threads: 1,
            cuda_resources: None,
        }
    }

    fn fixture_request(attempt: &str, runtime: u8) -> WorkerRequest {
        let profile = fixture_profile(runtime);
        let request = WorkerRequest {
            cuda_execution: None,
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new(attempt).unwrap(),
            job_id: JobId::new("fixture-job").unwrap(),
            project_id: ProjectId::new("fixture-project").unwrap(),
            base_revision: RevisionId::new(1),
            source: SourceArtifact {
                source_version: SourceVersionId::new("fixture-source").unwrap(),
                path: "/nonexistent/fallback-fixture/source".into(),
                identity: fixture_identity(1),
            },
            independent_audio: None,
            output_dir: format!("/nonexistent/fallback-fixture/{attempt}").into(),
            operation: WorkerOperation::Generate {
                preset: "default".into(),
                settings: Box::new(GenerationSettings {
                    model_input: Some(profile.preprocessing.input.clone()),
                    inference_profile: Some(profile),
                    ..GenerationSettings::default()
                }),
                model: Some(SourceArtifact {
                    source_version: SourceVersionId::new("fixture-model").unwrap(),
                    path: "/nonexistent/fallback-fixture/model".into(),
                    identity: fixture_identity(2),
                }),
            },
            budget: ResourceBudget {
                memory_bytes: 128,
                output_bytes: 128,
                wall_time_ms: 1000,
                cpu_threads: 1,
            },
            dependencies: vec![
                fixture_identity(1),
                fixture_identity(2),
                fixture_identity(runtime),
                fixture_identity(12),
                fixture_selection().identity().unwrap(),
            ],
            tools: WorkerTools {
                ffmpeg: ArtifactReference {
                    identity: fixture_identity(4),
                    path: "/nonexistent/fallback-fixture/ffmpeg".into(),
                },
                ffprobe: ArtifactReference {
                    identity: fixture_identity(5),
                    path: "/nonexistent/fallback-fixture/ffprobe".into(),
                },
                onnx_runtime: Some(ArtifactReference {
                    identity: fixture_identity(runtime),
                    path: "/nonexistent/fallback-fixture/runtime".into(),
                }),
            },
        };
        request.validate().unwrap();
        assert!(fixture_selection()
            .matches_worker_request(&request)
            .unwrap());
        request
    }

    fn fixture_environment(runtime: u8) -> InferenceEquivalenceEnvironment {
        InferenceEquivalenceEnvironment {
            engine: fixture_identity(11),
            worker: fixture_identity(12),
            os: "linux".into(),
            architecture: "x86_64".into(),
            platform_build: fixture_identity(13),
            hardware: fixture_identity(14),
            dependencies: vec![fixture_identity(runtime)],
        }
    }

    fn fixture_admission(
        kind: FallbackAdmissionKind,
        ordinal: u8,
        predecessor: Option<AttemptId>,
        runtime: u8,
    ) -> FallbackAdmission {
        FallbackAdmission {
            schema_version: 1,
            kind,
            ordinal,
            predecessor,
            reference: fixture_reference(1),
            observed_environment: fixture_environment(runtime),
            admitted_at_unix_ms: 10,
            expires_at_unix_ms: 100,
        }
    }

    struct Fixture {
        db: Connection,
        manifest: WorkerRequest,
        request: Request,
        extra: Vec<ArtifactIdentity>,
        choice: ModelPackChoice,
    }
    fn snapshot(value: &impl Serialize) -> ArtifactIdentity {
        let bytes = serde_json::to_vec(value).unwrap();
        let sha256 = digest(&bytes);
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: bytes.len() as u64,
        }
    }
    fn fixture(fallback: bool, declare: bool) -> Fixture {
        let mut manifest = fixture_request("initial", 3);
        if !fallback {
            manifest.dependencies.retain(|identity| {
                !identity
                    .artifact_id
                    .as_str()
                    .starts_with(INFERENCE_FALLBACK_SELECTION_ID_PREFIX)
            });
        }
        // An unrelated old runtime/device-style identity must never be copied
        // into a successor by the pack-document helper.
        manifest.dependencies.push(fixture_identity(15));
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON;PRAGMA user_version=20;CREATE TABLE requests(session TEXT NOT NULL,request TEXT NOT NULL,fingerprint TEXT NOT NULL,response TEXT NOT NULL,PRIMARY KEY(session,request));CREATE TABLE jobs(id TEXT PRIMARY KEY,attempt TEXT NOT NULL,project TEXT NOT NULL,base_revision INTEGER NOT NULL,session TEXT NOT NULL,source TEXT NOT NULL,state TEXT NOT NULL,manifest TEXT NOT NULL,candidate TEXT,error TEXT,attempts INTEGER NOT NULL DEFAULT 1);CREATE TABLE attempts(id TEXT PRIMARY KEY,job TEXT NOT NULL REFERENCES jobs(id),manifest TEXT NOT NULL,state TEXT NOT NULL);").unwrap();
        crate::model_pack_selection::initialize(&db).unwrap();
        let actor = SessionId::new("fixture-owner").unwrap();
        let tx = db.transaction().unwrap();
        let selected = crate::model_pack_selection::select(
            &tx,
            0,
            Some((fixture_identity(20).sha256, fixture_profile(3))),
            &actor,
            &RequestId::new("fixture-selection").unwrap(),
        )
        .unwrap();
        let choice = selected.choice.clone().unwrap();
        let extra = vec![
            fixture_identity(20),
            snapshot(&fixture_profile(3)),
            selection_snapshot(&selected).unwrap(),
        ];
        manifest.dependencies.extend(extra.iter().cloned());
        let raw = encode(&manifest).unwrap();
        tx.execute("INSERT INTO jobs(id,attempt,project,base_revision,session,source,state,manifest) VALUES(?1,?2,?3,?4,'fixture-owner',?5,'queued',?6)",params![manifest.job_id.as_str(),manifest.attempt_id.as_str(),manifest.project_id.as_str(),manifest.base_revision.value(),manifest.source.source_version.as_str(),raw]).unwrap();
        tx.execute(
            "INSERT INTO attempts VALUES(?1,?2,?3,'queued')",
            params![manifest.attempt_id.as_str(), manifest.job_id.as_str(), raw],
        )
        .unwrap();
        crate::job_attempt_history::initialize(&tx).unwrap();
        if declare {
            super::super::inference_fallback_store::declare_initial(
                &tx,
                &manifest,
                &fixture_selection(),
                &fixture_admission(FallbackAdmissionKind::Initial, 0, None, 3),
            )
            .unwrap();
        }
        tx.commit().unwrap();
        let WorkerOperation::Generate { settings, .. } = &manifest.operation else {
            unreachable!()
        };
        let command = if fallback {
            Command::GenerateWithInferenceFallback {
                source_version: manifest.source.source_version.clone(),
                preset: "default".into(),
                settings: settings.clone(),
                model: InferenceFallbackModel::ModelPack {
                    choice: choice.clone(),
                },
                selection: Box::new(fixture_selection()),
            }
        } else {
            Command::GenerateFromModelPack {
                source_version: manifest.source.source_version.clone(),
                preset: "default".into(),
                settings: Some(settings.as_ref().clone()),
                choice: choice.clone(),
            }
        };
        let request = Request::new(RequestId::new("fixture-generation").unwrap(), command)
            .with_session(actor)
            .with_auth_token("fixture-token".into())
            .in_project(manifest.project_id.clone(), Some(manifest.base_revision));
        Fixture {
            db,
            manifest,
            request,
            extra,
            choice,
        }
    }
    fn record(fixture: &mut Fixture) {
        let tx = fixture.db.transaction().unwrap();
        record_admission(&tx, &fixture.manifest, &fixture.request, &fixture.extra).unwrap();
        tx.commit().unwrap();
    }
    fn saved(fixture: &Fixture) -> String {
        fixture
            .db
            .query_row(
                "SELECT response FROM requests WHERE session=?1 AND request=?2",
                params![ADMISSION_NAMESPACE, fixture.manifest.job_id.as_str()],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn new_fallback_pack_origin_retains_only_original_three_document_snapshots() {
        let mut fixture = fixture(true, true);
        record(&mut fixture);
        let admission: Admission = decode(&saved(&fixture)).unwrap();
        assert!(
            matches!(&admission.origin,Origin::Pack {choice,dependencies:Some(dependencies),..} if choice==&fixture.choice && dependencies==&fixture.extra)
        );
        let retained = fallback_dependencies(&fixture.db, &fixture.manifest).unwrap();
        assert_eq!(retained, fixture.extra);
        assert_eq!(retained.len(), 3);
        for old in &fixture.manifest.dependencies {
            if !fixture.extra.contains(old) {
                assert!(!retained.contains(old));
            }
        }
    }

    #[test]
    fn strict_legacy_pack_serialization_stays_optional_and_direct_returns_no_pack_dependencies() {
        let mut strict = fixture(false, false);
        // Legacy callers historically passed only the approval snapshot.
        let tx = strict.db.transaction().unwrap();
        record_admission(&tx, &strict.manifest, &strict.request, &strict.extra[2..]).unwrap();
        tx.commit().unwrap();
        let raw: serde_json::Value = serde_json::from_str(&saved(&strict)).unwrap();
        assert!(raw["origin"].get("dependencies").is_none());
        assert!(fallback_dependencies(&strict.db, &strict.manifest).is_err());
        let mut direct = fixture(false, false);
        direct.request.command = Command::Generate {
            source_version: direct.manifest.source.source_version.clone(),
            preset: "default".into(),
            settings: None,
            model_path: None,
        };
        record(&mut direct);
        assert!(matches!(
            read_admission(&direct.db, &direct.manifest)
                .unwrap()
                .unwrap()
                .origin,
            Origin::Direct
        ));
        assert!(fallback_dependencies(&direct.db, &direct.manifest)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn fallback_pack_requires_committed_initial_mode_before_private_admission() {
        let mut fixture = fixture(true, false);
        let tx = fixture.db.transaction().unwrap();
        assert!(
            record_admission(&tx, &fixture.manifest, &fixture.request, &fixture.extra).is_err()
        );
        assert_eq!(
            tx.query_row("SELECT COUNT(*) FROM requests", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            tx.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            20
        );
    }

    #[test]
    fn missing_changed_duplicated_or_nonmatching_pack_snapshots_refuse() {
        for change in 0..4 {
            let mut fixture = fixture(true, true);
            let mut extra = fixture.extra.clone();
            match change {
                0 => {
                    extra.remove(1);
                }
                1 => {
                    extra[0] = fixture_identity(21);
                }
                2 => {
                    extra[1] = extra[0].clone();
                }
                _ => {
                    extra[2] = fixture_identity(22);
                }
            }
            let tx = fixture.db.transaction().unwrap();
            assert!(record_admission(&tx, &fixture.manifest, &fixture.request, &extra).is_err());
            assert_eq!(
                tx.query_row("SELECT COUNT(*) FROM requests", [], |row| row
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
    }

    #[test]
    fn successor_pack_dependencies_refuse_changed_choice_model_or_initial_manifest() {
        for change in 0..3 {
            let mut fixture = fixture(true, true);
            record(&mut fixture);
            if change == 0 {
                let tx = fixture.db.transaction().unwrap();
                crate::model_pack_selection::select(
                    &tx,
                    fixture.choice.selection_revision,
                    None,
                    &SessionId::new("fixture-owner").unwrap(),
                    &RequestId::new("clear-selection").unwrap(),
                )
                .unwrap();
                tx.commit().unwrap();
            } else if change == 1 {
                let mut selected = crate::model_pack_selection::status(&fixture.db).unwrap();
                selected.profile.as_mut().unwrap().model = fixture_identity(99);
                fixture
                    .db
                    .execute(
                        "UPDATE model_pack_selection SET report=?1",
                        [encode(&selected).unwrap()],
                    )
                    .unwrap();
            } else {
                fixture
                    .db
                    .execute("UPDATE attempts SET manifest=' '||manifest", [])
                    .unwrap();
            }
            assert!(fallback_dependencies(&fixture.db, &fixture.manifest).is_err());
        }
    }

    #[test]
    fn fallback_pack_cannot_drop_its_new_field_and_reinterpret_as_legacy() {
        let mut fixture = fixture(true, true);
        record(&mut fixture);
        let mut admission: Admission = decode(&saved(&fixture)).unwrap();
        let Origin::Pack { dependencies, .. } = &mut admission.origin else {
            panic!("pack origin required")
        };
        *dependencies = None;
        let raw = encode(&admission).unwrap();
        fixture
            .db
            .execute(
                "UPDATE requests SET fingerprint=?1,response=?2 WHERE session=?3",
                params![digest(raw.as_bytes()), raw, ADMISSION_NAMESPACE],
            )
            .unwrap();
        assert!(fallback_dependencies(&fixture.db, &fixture.manifest).is_err());
    }

    #[test]
    fn withdrawal_pending_clear_and_job_cancel_are_atomic_and_preserve_failed_predecessor() {
        let mut fixture = fixture(true, true);
        record(&mut fixture);
        fixture
            .db
            .execute("UPDATE jobs SET state='fallback_pending'", [])
            .unwrap();
        fixture
            .db
            .execute("UPDATE attempts SET state='failed'", [])
            .unwrap();
        // Authored pending row only: no settled native execution is claimed.
        fixture
            .db
            .execute(
                "INSERT INTO inference_fallback_pending VALUES(?1,?2,'fixture-epoch',?3)",
                params![
                    fixture.manifest.job_id.as_str(),
                    fixture.manifest.attempt_id.as_str(),
                    "a".repeat(64)
                ],
            )
            .unwrap();
        {
            let tx = fixture.db.transaction().unwrap();
            cancel_job_for_withdrawal(&tx, fixture.manifest.job_id.as_str()).unwrap();
            assert_eq!(
                tx.query_row(
                    "SELECT COUNT(*) FROM inference_fallback_pending",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            assert_eq!(
                tx.query_row("SELECT state FROM attempts", [], |row| row
                    .get::<_, String>(0))
                    .unwrap(),
                "failed"
            );
        }
        assert_eq!(
            fixture
                .db
                .query_row("SELECT state FROM jobs", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "fallback_pending"
        );
        assert_eq!(
            fixture
                .db
                .query_row(
                    "SELECT COUNT(*) FROM inference_fallback_pending",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        let tx = fixture.db.transaction().unwrap();
        cancel_job_for_withdrawal(&tx, fixture.manifest.job_id.as_str()).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            fixture
                .db
                .query_row("SELECT state FROM jobs", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "cancelled"
        );
        assert_eq!(
            fixture
                .db
                .query_row(
                    "SELECT COUNT(*) FROM inference_fallback_pending",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(
            fixture
                .db
                .query_row("SELECT state FROM attempts", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "failed"
        );
    }
}
