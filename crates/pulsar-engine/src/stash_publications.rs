//! Query-only local publication observations. Never dispatches or promotes state.
use super::*;
#[derive(Deserialize)]
struct Header {
    schema_version: u32,
    context: Context,
    reused_sample_clocks: u64,
    release_qualified: bool,
    #[serde(deserialize_with = "crate::source_clock_receipts::bounded::<_, _, 1000000>")]
    mappings: Vec<SourceClockActionMapping>,
}
#[derive(Deserialize)]
struct Context {
    #[serde(default)]
    archived_context: Option<ArchivedPublicationContext>,
    #[serde(default)]
    action_ancestry: Option<stash_action_ancestry::NativeActionAncestry>,
    #[serde(default)]
    original_candidate: Option<CandidateId>,
    #[serde(default)]
    original_receipt: Option<ArtifactIdentity>,
    #[serde(default)]
    original_program: Option<ArtifactIdentity>,
    #[serde(default)]
    placement: Option<SourcePlacement>,
    #[serde(
        default,
        deserialize_with = "crate::source_clock_receipts::bounded::<_, _, 2048>"
    )]
    retimings: Vec<MotionRetimeReceipt>,
    #[serde(
        default,
        deserialize_with = "crate::source_clock_receipts::bounded::<_, _, 128>"
    )]
    native_retimings: Vec<MotionRetimeReceipt>,
    #[serde(
        default,
        deserialize_with = "crate::source_clock_receipts::bounded::<_, _, 128>"
    )]
    archived_retimings: Vec<ArchivedMotionRetimeReceipt>,
    publication_request: RequestId,
    source: StashSourceCapture,
    project: ProjectId,
    revision: RevisionId,
    candidate: CandidateId,
    job: JobId,
    master_sha256: String,
    script: ArtifactIdentity,
}
type Row = (i64, String, String, String, Option<String>, Option<String>);
fn row(db: &Connection, session: &SessionId, project: &ProjectId, id: &RequestId) -> PResult<Row> {
    db.query_row("SELECT e.revision,e.path,e.sha256,e.state,p.proof,r.response FROM exports e LEFT JOIN stash_clock_proofs p ON p.session=e.session AND p.request=e.request LEFT JOIN requests r ON r.session=e.session AND r.request=e.request WHERE e.session=?1 AND e.project=?2 AND e.request=?3",params![session.as_str(),project.as_str(),id.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(internal)?.ok_or_else(||ProtocolError::new(ErrorCode::NotFound,"publication dispatch is absent for this session/project"))
}
impl Engine {
    pub(super) fn stash_publication_observation(
        &self,
        request: &Request,
        id: &RequestId,
    ) -> PResult<ResponseBody> {
        let _admission = self.pool.reserve_memory(transfers::VALIDATION_MEMORY)?;
        let session = request.session.as_ref().expect("authenticated");
        let project = project(request)?;
        let saved = {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, session)?;
            row(&db, session, project, id)?
        };
        let (revision, path, hash, state, raw_proof, raw_receipt) = &saved;
        let revision = RevisionId::new(u64::try_from(*revision).map_err(internal)?);
        let dispatch = match state.as_str() {
            "pending" => StashPublicationDispatch::Pending,
            "failed_or_unknown" => StashPublicationDispatch::FailedOrUnknown,
            "complete" => StashPublicationDispatch::Complete,
            _ => return Err(internal("unknown publication dispatch state")),
        };
        let completed = (if *state == "complete" {
            raw_receipt.as_ref()
        } else {
            None
        })
        .map(|r| decode::<ResponseBody>(r))
        .transpose()?
        .and_then(|r| {
            if let ResponseBody::StashScriptExported(r) = r {
                Some(r)
            } else {
                None
            }
        });
        let proof: Option<StashTimingProof> =
            raw_proof.as_ref().map(|raw| decode(raw)).transpose()?;
        if let Some(receipt) = &completed {
            if encode(&receipt.timing_proof)? != encode(&proof)? || receipt.release_qualified {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "stored publication proof reference changed",
                ));
            }
            if let Some(context) = proof.as_ref().and_then(|p| p.archived_context.as_ref()) {
                context.validate()?;
                if context.project_id != receipt.project_id
                    || context.master.revision != receipt.revision
                    || context.master.anchor_candidate != receipt.candidate_id
                    || context.origin.job_id != receipt.job_id
                    || context.origin.source_version != receipt.source.source_version
                    || context.origin.source_identity != receipt.source.identity
                {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "stored archival publication correspondence changed",
                    ));
                }
            }
        }
        if *state == "complete" && raw_receipt.is_some() && completed.is_none() {
            return Err(ProtocolError::invalid(
                "recorded export is not a Stash publication",
            ));
        }
        if let Some(p) = &completed {
            if p.project_id != *project
                || p.revision != revision
                || p.publication_request_id.as_ref() != Some(id)
                || p.path != Path::new(path)
                || p.identity.sha256 != *hash
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "stored Stash publication identity is inconsistent",
                ));
            }
        }
        let mut proof_observation = StashArtifactObservation::NotRecorded;
        let mut publication = completed.clone();
        if let Some(proof) = &proof {
            proof_observation = StashArtifactObservation::Unverified;
            let expected = self
                .config
                .state_dir
                .join("source-clock-proofs")
                .join(format!("{}.json", proof.identity.sha256));
            if proof.path == expected {
                if let Ok(header) = crate::source_clock_receipts::read_verified::<Header>(
                    &expected,
                    &proof.identity,
                ) {
                    let c = header.context;
                    let origin = match (
                        header.schema_version,
                        &c.archived_context,
                        &proof.archived_context,
                    ) {
                        (1 | 2 | 4 | 6, None, None) => {
                            let checked = (|| -> PResult<bool> {
                                if (matches!(header.schema_version, 4 | 6))
                                    != c.action_ancestry.is_some()
                                {
                                    return Ok(false);
                                }
                                let db = self.db.lock().map_err(internal)?;
                                let source_bound:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sources WHERE project=?1 AND version=?2 AND sha256=?3 AND bytes=?4)",params![c.project.as_str(),c.source.source_version.as_str(),c.source.identity.sha256,i64::try_from(c.source.identity.byte_len).map_err(internal)?],|r|r.get(0)).map_err(internal)?;
                                if !source_bound {
                                    return Ok(false);
                                }
                                let descriptor =
                                    motion_state::revision_descriptor(&db, &c.project, c.revision)?;
                                let _motion_memory = self.pool.reserve_memory(
                                    descriptor
                                        .byte_len
                                        .checked_mul(8)
                                        .and_then(|n| n.checked_add(16 * 1024 * 1024))
                                        .ok_or_else(|| {
                                            ProtocolError::new(
                                                ErrorCode::ResourceExhausted,
                                                "proof motion admission overflow",
                                            )
                                        })?,
                                )?;
                                if descriptor.sha256 != c.master_sha256 {
                                    return Ok(false);
                                }
                                let program = motion_state::store_for(&db)?.read(&descriptor)?;
                                let track = program
                                    .track(Axis::Stroke)
                                    .ok_or_else(|| ProtocolError::invalid("proof stroke absent"))?;
                                let derived = stash_action_ancestry::map_native(
                                    &db,
                                    &c.project,
                                    &c.source.source_version,
                                    c.revision,
                                    track,
                                    &self.pool,
                                )?;
                                if header.schema_version == 6 {
                                    if derived.ancestry.carriers.len() < 2 {
                                        return Ok(false);
                                    }
                                    let manifest = source_clock_origins::manifest(
                                        &db,
                                        &c.project,
                                        &c.source.source_version,
                                        &c.job,
                                        &self.pool,
                                    )?;
                                    let selected = source_clock_origins::clocks(
                                        &db,
                                        &manifest,
                                        &c.candidate,
                                        &self.pool,
                                    )?;
                                    if c.original_candidate.as_ref()
                                        != Some(&selected.original_candidate)
                                        || c.original_receipt.as_ref()
                                            != Some(&selected.verified.identity)
                                        || c.original_program.as_ref() != Some(&selected.program)
                                        || c.placement.as_ref()
                                            != Some(&selected.verified.plan.placement)
                                    {
                                        return Ok(false);
                                    }
                                    let revisions =
                                        stash_action_ancestry::selected_native_retimings(
                                            &derived.ancestry,
                                            &c.job,
                                            &selected,
                                        )?;
                                    let receipts = revisions
                                        .into_iter()
                                        .map(|revision| {
                                            motion_retime_receipts::load_verified(
                                                &db, &c.project, revision, &self.pool,
                                            )
                                        })
                                        .collect::<PResult<Vec<_>>>()?;
                                    if encode(&receipts)? != encode(&c.retimings)? {
                                        return Ok(false);
                                    }
                                }
                                Ok(c.action_ancestry
                                    .as_ref()
                                    .is_none_or(|ancestry| derived.ancestry == *ancestry)
                                    && derived.motion.mappings == header.mappings
                                    && derived.motion.reused_sample_clocks
                                        == header.reused_sample_clocks
                                    && derived.motion.mappings.len() as u64 == proof.mapped_actions)
                            })();
                            checked.unwrap_or(false)
                        }
                        (3 | 5 | 7, Some(context), Some(saved)) => {
                            let checked = (|| -> PResult<bool> {
                                context.validate()?;
                                if encode(context)? != encode(saved)?
                                    || context.project_id != c.project
                                    || context.master.revision != c.revision
                                    || context.master.anchor_candidate != c.candidate
                                    || context.origin.job_id != c.job
                                    || context.master.master.sha256 != c.master_sha256
                                    || context.origin.source_version != c.source.source_version
                                    || context.origin.source_identity != c.source.identity
                                    || matches!(header.schema_version, 5 | 7)
                                        != c.action_ancestry.is_some()
                                {
                                    return Ok(false);
                                }
                                if !context.sampled_clocks_verified {
                                    return Ok(header.schema_version == 3
                                        && header.mappings.is_empty()
                                        && header.reused_sample_clocks == 0);
                                }
                                let db = self.db.lock().map_err(internal)?;
                                let descriptor =
                                    motion_state::revision_descriptor(&db, &c.project, c.revision)?;
                                let _motion_memory = self.pool.reserve_memory(
                                    descriptor
                                        .byte_len
                                        .checked_mul(8)
                                        .and_then(|n| n.checked_add(16 * 1024 * 1024))
                                        .ok_or_else(|| {
                                            ProtocolError::new(
                                                ErrorCode::ResourceExhausted,
                                                "archival proof motion admission overflow",
                                            )
                                        })?,
                                )?;
                                if descriptor.sha256 != c.master_sha256 {
                                    return Ok(false);
                                }
                                let program = motion_state::store_for(&db)?.read(&descriptor)?;
                                let track = program.track(Axis::Stroke).ok_or_else(|| {
                                    ProtocolError::invalid("archival proof stroke absent")
                                })?;
                                let derived = stash_action_ancestry::map_archived(
                                    &db,
                                    &c.project,
                                    c.revision,
                                    &context.origin,
                                    track,
                                    &self.pool,
                                    &self.import_store,
                                )?;
                                if (header.schema_version == 7)
                                    != (derived.ancestry.carriers.len() > 1)
                                {
                                    return Ok(false);
                                }
                                let revisions = stash_action_ancestry::selected_archived_retimings(
                                    &derived.ancestry,
                                    &context.origin,
                                )?;
                                if revisions != context.master.retiming_revisions {
                                    return Ok(false);
                                }
                                let mut native = Vec::new();
                                let mut archived = Vec::new();
                                for revision in revisions {
                                    let local:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM motion_retime_receipts WHERE project=?1 AND revision=?2)",params![c.project.as_str(),rev_sql(revision)?],|r|r.get(0)).map_err(internal)?;
                                    if local {
                                        native.push(motion_retime_receipts::load_verified(
                                            &db, &c.project, revision, &self.pool,
                                        )?);
                                    } else {
                                        archived.push(archived_motion_retimes::load_verified(
                                            &db,
                                            &c.project,
                                            revision,
                                            &self.pool,
                                            &self.import_store,
                                        )?);
                                    }
                                }
                                if encode(&native)? != encode(&c.native_retimings)?
                                    || encode(&archived)? != encode(&c.archived_retimings)?
                                {
                                    return Ok(false);
                                }
                                Ok(c.action_ancestry
                                    .as_ref()
                                    .is_none_or(|ancestry| derived.ancestry == *ancestry)
                                    && derived.motion.mappings == header.mappings
                                    && derived.motion.reused_sample_clocks
                                        == header.reused_sample_clocks
                                    && derived.motion.mappings.len() as u64 == proof.mapped_actions)
                            })();
                            checked.unwrap_or(false)
                        }
                        _ => false,
                    };
                    if origin
                        && !header.release_qualified
                        && c.project == *project
                        && c.revision == revision
                        && c.publication_request == *id
                        && c.source.script_destination == Path::new(path)
                        && c.script.sha256 == *hash
                        && c.master_sha256 == proof.master_sha256
                        && header.reused_sample_clocks == proof.reused_sample_clocks
                    {
                        if let Some(receipt) = &completed {
                            if receipt.candidate_id != c.candidate
                                || receipt.job_id != c.job
                                || encode(&receipt.source)? != encode(&c.source)?
                            {
                                return Err(ProtocolError::new(
                                    ErrorCode::DependencyMismatch,
                                    "completed publication differs from retained proof context",
                                ));
                            }
                        }
                        proof_observation = StashArtifactObservation::Verified;
                        publication = Some(Box::new(StashScriptExport {
                            timing_proof: Some(proof.clone()),
                            publication_request_id: Some(id.clone()),
                            project_id: project.clone(),
                            revision,
                            candidate_id: c.candidate,
                            job_id: c.job,
                            source: c.source,
                            path: PathBuf::from(path),
                            identity: c.script,
                            unresolved_review_count: 0,
                            review_state_unknown: true,
                            release_qualified: false,
                        }));
                    }
                }
            }
        }
        let pair = match &publication {
            Some(p)
                if p.project_id == *project
                    && p.revision == revision
                    && p.path == Path::new(path)
                    && p.identity.sha256 == *hash =>
            {
                if crate::stash_files::verify_published(p).is_ok() {
                    StashArtifactObservation::Verified
                } else {
                    StashArtifactObservation::Unverified
                }
            }
            None if proof.is_some() => StashArtifactObservation::Unverified,
            _ => StashArtifactObservation::NotRecorded,
        };
        {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, session)?;
            if row(&db, session, project, id)? != saved {
                return Err(ProtocolError::new(
                    ErrorCode::RequestConflict,
                    "publication record changed during observation; repeat this query",
                ));
            }
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(internal)?
            .as_millis();
        Ok(ResponseBody::StashPublication(
            (StashPublicationObservation {
                publication_request_id: id.clone(),
                project_id: project.clone(),
                revision,
                dispatch,
                path: PathBuf::from(path),
                expected_script_sha256: hash.clone(),
                timing_proof: proof,
                proof_observation,
                source_and_script_observation: pair,
                observed_at_unix_ms: u64::try_from(now).map_err(internal)?,
                completed_receipt: completed.map(|value| *value),
                release_qualified: false,
            })
            .into(),
        ))
    }
}
