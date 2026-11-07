//! Historical generation anchors resolved only through retained namespaces.
use super::*;
use crate::project_package_import_storage::ImportStore;
use crate::source_clock_receipts;
use std::{collections::BTreeSet, io::Cursor};

type ArchivedCandidateRow = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "archived generation correspondence changed",
    )
}
fn absent() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::NotFound,
        "candidate has no retained original generation anchor",
    )
}
pub(super) fn read(
    store: &ImportStore,
    root: &PortableProjectManifest,
    reference: &PackageObjectRef,
    role: Option<PackageObjectRole>,
    pool: &ResourcePool,
) -> PResult<Vec<u8>> {
    let object = root
        .objects
        .iter()
        .find(|o| o.sha256 == reference.sha256 && o.byte_len == reference.byte_len)
        .ok_or_else(mismatch)?;
    let role = role.unwrap_or(
        if object.roles.contains(&PackageObjectRole::MotionProgram) {
            PackageObjectRole::MotionProgram
        } else {
            *object.roles.first().ok_or_else(mismatch)?
        },
    );
    // The caller retains admission for decoded records. This reservation also
    // bounds transient reads; never open an archived worker/source path.
    let _memory = pool.reserve_memory(
        reference
            .byte_len
            .checked_mul(8)
            .and_then(|n| n.checked_add(16 * 1024 * 1024))
            .ok_or_else(mismatch)?,
    )?;
    store.read_retained_object(
        object,
        role,
        if object.roles.contains(&PackageObjectRole::AnnotatedModel) {
            256 * 1024 * 1024
        } else {
            128 * 1024 * 1024
        },
    )
}

/// A CUDA execution receipt and node trace follow the general producer receipt.
/// Use the declared role ordering, never an arbitrary matching lineage entry.
pub(super) fn has_producer_receipt(
    candidate: &PackageCandidate,
    record: &PackageGeneratedOrigin,
) -> bool {
    let Some(receipt) = record.receipt.as_ref() else {
        return false;
    };
    if candidate.job_origin.as_ref() != Some(&record.job_id) {
        return false;
    }
    match &record.cuda_evidence {
        Some(cuda) => cuda
            .general_receipt(&candidate.lineage)
            .is_ok_and(|producer| producer == receipt),
        None => candidate.lineage.last() == Some(receipt),
    }
}

pub(super) fn load(
    db: &Connection,
    project: &ProjectId,
    candidate: &CandidateId,
    pool: &ResourcePool,
    store: &ImportStore,
) -> PResult<ArchivedGenerationReport> {
    let graph = archived_project_origins::load(db, project, None, pool, store)?;
    let anchor = graph
        .root
        .candidates
        .iter()
        .find(|c| c.candidate_id == *candidate)
        .ok_or_else(absent)?;
    let row: Option<ArchivedCandidateRow> = db.query_row(
        "SELECT CASE WHEN length(CAST(c.program AS BLOB))=71 THEN c.program ELSE NULL END,CASE WHEN c.job IS NULL THEN NULL ELSE 'present' END,CASE WHEN length(CAST(c.lineage AS BLOB)) BETWEEN 1 AND 524288 THEN c.lineage ELSE NULL END,CASE WHEN length(CAST(a.imported_candidate AS BLOB)) BETWEEN 1 AND 128 THEN a.imported_candidate ELSE NULL END FROM candidates c JOIN project_import_candidate_aliases a ON a.candidate=c.id AND a.project=c.project WHERE c.project=?1 AND c.id=?2",
        params![project.as_str(), candidate.as_str()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))
        .optional().map_err(internal)?;
    let (program, job, lineage, alias) = row.ok_or_else(absent)?;
    let lineage: Vec<ArtifactIdentity> = decode(&lineage.ok_or_else(mismatch)?)?;
    if job.is_some()
        || alias.as_deref() != Some(candidate.as_str())
        || program.as_deref() != Some(format!("motion:{}", anchor.motion.sha256).as_str())
        || lineage != anchor.lineage
    {
        return Err(mismatch());
    }
    let immediate = graph
        .root
        .imported_origins
        .iter()
        .find(|e| e.manifest.sha256 == graph.immediate_manifest)
        .ok_or_else(mismatch)?;
    if !immediate.candidates.iter().any(|m| m.local == *candidate) {
        return Err(mismatch());
    }
    let mut pending = vec![(&graph.root, candidate.clone())];
    let mut visited = BTreeSet::new();
    let mut origins = Vec::new();
    while let Some((namespace, id)) = pending.pop() {
        let namespace_sha = if std::ptr::eq(namespace, &graph.root) {
            String::new()
        } else {
            graph
                .origins
                .iter()
                .find(|(_, v)| std::ptr::eq(*v, namespace))
                .map(|(k, _)| k.clone())
                .ok_or_else(mismatch)?
        };
        if !visited.insert((namespace_sha.clone(), id.clone())) {
            continue;
        }
        if visited.len() > MAX_PACKAGE_ORIGIN_ENTRIES {
            return Err(mismatch());
        }
        let original = namespace
            .candidates
            .iter()
            .find(|c| c.candidate_id == id)
            .ok_or_else(mismatch)?;
        if let Some(job) = &original.job_origin {
            if namespace_sha.is_empty() || origins.len() >= 16 {
                return Err(mismatch());
            }
            let selected: Vec<_> = namespace
                .generated_origins
                .iter()
                .filter(|o| o.job_id == *job && has_producer_receipt(original, o))
                .collect();
            if selected.len() != 1 {
                return Err(mismatch());
            }
            let origin = selected[0];
            // Retain admission across decoded request, raw program and clocks.
            let evidence_bytes = namespace
                .objects
                .iter()
                .filter(|o| {
                    !o.roles
                        .iter()
                        .all(|role| *role == PackageObjectRole::SourceSnapshot)
                })
                .try_fold(0u64, |n, o| n.checked_add(o.byte_len))
                .ok_or_else(mismatch)?;
            let _memory = pool.reserve_memory(
                evidence_bytes
                    .checked_mul(8)
                    .and_then(|n| n.checked_add(16 * 1024 * 1024))
                    .ok_or_else(mismatch)?,
            )?;
            project_package_imports::validate_evidence(
                namespace,
                pool,
                |r| read(store, &graph.root, r, None, pool),
                &mut || Ok(()),
            )?;
            let raw = read(
                store,
                &graph.root,
                &origin.manifest,
                Some(PackageObjectRole::WorkerManifest),
                pool,
            )?;
            let request: WorkerRequest = serde_json::from_slice(&raw).map_err(|_| mismatch())?;
            project_package_imports::checked_worker_scope(namespace, origin, &request)?;
            let WorkerOperation::Generate {
                preset, settings, ..
            } = &request.operation
            else {
                return Err(absent());
            };
            let receipt = origin.receipt.clone().ok_or_else(mismatch)?;
            let clock = if settings.source_placement.is_some() {
                let clocks = verify_plan(
                    &graph.root,
                    namespace,
                    original,
                    origin,
                    &request,
                    pool,
                    store,
                )?;
                Some(ArchivedGenerationClock {
                    receipt: clocks.receipt.clone(),
                    sample_count: clocks.plan.samples.len() as u64,
                    source_origin: clocks.plan.source_origin,
                    project_trim: clocks.plan.project_trim,
                })
            } else {
                None
            };
            let descriptor = graph
                .root
                .objects
                .iter()
                .find(|o| {
                    o.sha256 == namespace_sha
                        && o.roles.contains(&PackageObjectRole::ImportedManifest)
                })
                .ok_or_else(mismatch)?;
            origins.push(ArchivedGenerationOrigin {
                origin_manifest: PackageObjectRef {
                    sha256: namespace_sha,
                    byte_len: descriptor.byte_len,
                },
                project_id: request.project_id,
                candidate_id: id.clone(),
                job_id: request.job_id,
                attempt_id: request.attempt_id,
                source_version: request.source.source_version,
                source_identity: request.source.identity,
                program: ArtifactIdentity {
                    artifact_id: original.motion.artifact_id.clone(),
                    sha256: original.motion.sha256.clone(),
                    byte_len: original.motion.byte_len,
                },
                worker_manifest: origin.manifest.clone(),
                worker_receipt: receipt,
                preset: preset.clone(),
                settings: *settings.clone(),
                clock,
            });
        }
        for edge in &namespace.imported_origins {
            if let Some(mapping) = edge.candidates.iter().find(|m| m.local == id) {
                pending.push((
                    graph
                        .origins
                        .get(&edge.manifest.sha256)
                        .ok_or_else(mismatch)?,
                    mapping.origin.clone(),
                ));
            }
        }
    }
    if origins.is_empty() {
        return Err(absent());
    }
    origins.sort_by(|a, b| {
        (&a.origin_manifest.sha256, &a.candidate_id)
            .cmp(&(&b.origin_manifest.sha256, &b.candidate_id))
    });
    let report = ArchivedGenerationReport {
        project_id: project.clone(),
        candidate_id: candidate.clone(),
        origins,
        master_derivation: None,
        archival: true,
        active_job: false,
        publication_authority: false,
        release_qualified: false,
    };
    report.validate()?;
    Ok(report)
}

pub(super) struct PublicationClocks {
    pub context: ArchivedPublicationContext,
    pub plan: Option<PlacedGenerationPlan>,
    _memory: Reservation,
}
/// Historical producer selection and the current local publication snapshot.
/// These identities never grant authority to imported jobs.
pub(super) struct PublicationSelection<'a> {
    pub project: &'a ProjectId,
    pub candidate: &'a CandidateId,
    pub job: &'a JobId,
    pub selected: &'a PackageObjectRef,
    pub revision: RevisionId,
    pub source_time: bool,
}
pub(super) fn publication_clocks(
    db: &Connection,
    selection: PublicationSelection<'_>,
    pool: &ResourcePool,
    store: &ImportStore,
) -> PResult<PublicationClocks> {
    let PublicationSelection {
        project,
        candidate,
        job,
        selected,
        revision,
        source_time,
    } = selection;
    let report = load(db, project, candidate, pool, store)?;
    let origin = report
        .origins
        .iter()
        .find(|o| o.origin_manifest == *selected && o.job_id == *job)
        .ok_or_else(mismatch)?
        .clone();
    let master = if source_time {
        stash_action_ancestry::archived_master(
            db, project, revision, candidate, &origin, pool, store,
        )?
    } else {
        stash_lineage::verified_archived_master(db, project, revision, &report, pool, store)?
    };
    let context = ArchivedPublicationContext {
        project_id: project.clone(),
        sampled_clocks_verified: origin.clock.is_some(),
        origin,
        master,
        source_time,
        archival: true,
        active_job: false,
        release_qualified: false,
    };
    context.validate()?;
    let samples = context.origin.clock.as_ref().map_or(0, |c| c.sample_count);
    let memory = pool.reserve_memory(
        samples
            .checked_mul(512)
            .and_then(|n| n.checked_add(16 * 1024 * 1024))
            .ok_or_else(mismatch)?,
    )?;
    let plan = if source_time {
        let verified = retained_plan(db, project, &context.origin, pool, store)?;
        Some(verified.plan)
    } else {
        None
    };
    Ok(PublicationClocks {
        context,
        plan,
        _memory: memory,
    })
}

/// A retained historical sample plan. The admission stays live with the plan;
/// this is correspondence evidence, never an executable imported job.
pub(super) struct RetainedPlan {
    pub plan: PlacedGenerationPlan,
    pub receipt: ArtifactIdentity,
    _memory: Reservation,
}
fn verify_plan(
    root: &PortableProjectManifest,
    namespace: &PortableProjectManifest,
    candidate: &PackageCandidate,
    record: &PackageGeneratedOrigin,
    request: &WorkerRequest,
    pool: &ResourcePool,
    store: &ImportStore,
) -> PResult<RetainedPlan> {
    project_package_imports::checked_worker_scope(namespace, record, request)?;
    if !has_producer_receipt(candidate, record) {
        return Err(mismatch());
    }
    let receipt = record.receipt.as_ref().ok_or_else(mismatch)?;
    let raw = record.program.as_ref().ok_or_else(mismatch)?;
    let needed = record
        .manifest
        .byte_len
        .checked_add(receipt.byte_len)
        .and_then(|n| n.checked_add(raw.byte_len))
        .and_then(|n| n.checked_mul(8))
        .and_then(|n| n.checked_add(16 * 1024 * 1024))
        .ok_or_else(mismatch)?;
    let memory = pool.reserve_memory(needed)?;
    let bytes = read(
        store,
        root,
        &PackageObjectRef {
            sha256: receipt.sha256.clone(),
            byte_len: receipt.byte_len,
        },
        Some(PackageObjectRole::WorkerReceipt),
        pool,
    )?;
    let canonical = ArtifactIdentity {
        artifact_id: candidate.motion.artifact_id.clone(),
        sha256: candidate.motion.sha256.clone(),
        byte_len: candidate.motion.byte_len,
    };
    let verified = source_clock_receipts::load_verified_readers(
        request,
        &candidate.lineage,
        &canonical,
        Cursor::new(bytes),
        receipt.byte_len,
        |identity, _| {
            if identity != raw {
                return Err(mismatch());
            }
            let bytes = read(
                store,
                root,
                &PackageObjectRef {
                    sha256: identity.sha256.clone(),
                    byte_len: identity.byte_len,
                },
                Some(PackageObjectRole::DependencyEvidence),
                pool,
            )?;
            Ok(Box::new(Cursor::new(bytes)) as Box<dyn std::io::Read>)
        },
    )?;
    if verified.identity != *receipt {
        return Err(mismatch());
    }
    SourceClockMapper::new(
        &verified.plan.placement.spec,
        Some(&verified.plan),
        verified.plan.samples.len(),
    )
    .map_err(|_| mismatch())?;
    Ok(RetainedPlan {
        plan: verified.plan,
        receipt: verified.identity,
        _memory: memory,
    })
}

/// Resolve only the explicitly named retained namespace and producer. Historical
/// output paths, local job IDs and matching source bytes cannot select an origin.
pub(super) fn retained_plan(
    db: &Connection,
    project: &ProjectId,
    origin: &ArchivedGenerationOrigin,
    pool: &ResourcePool,
    store: &ImportStore,
) -> PResult<RetainedPlan> {
    let graph = archived_project_origins::load(db, project, None, pool, store)?;
    let object = graph
        .root
        .objects
        .iter()
        .find(|o| {
            o.sha256 == origin.origin_manifest.sha256
                && o.byte_len == origin.origin_manifest.byte_len
                && o.roles.contains(&PackageObjectRole::ImportedManifest)
        })
        .ok_or_else(mismatch)?;
    let namespace = graph.origins.get(&object.sha256).ok_or_else(mismatch)?;
    let candidate = namespace
        .candidates
        .iter()
        .find(|c| c.candidate_id == origin.candidate_id)
        .ok_or_else(mismatch)?;
    let record = namespace
        .generated_origins
        .iter()
        .find(|o| {
            o.job_id == origin.job_id
                && o.manifest == origin.worker_manifest
                && o.receipt.as_ref() == Some(&origin.worker_receipt)
        })
        .ok_or_else(mismatch)?;
    let _request_memory = pool.reserve_memory(
        record
            .manifest
            .byte_len
            .checked_mul(8)
            .and_then(|n| n.checked_add(8192))
            .ok_or_else(mismatch)?,
    )?;
    let bytes = read(
        store,
        &graph.root,
        &record.manifest,
        Some(PackageObjectRole::WorkerManifest),
        pool,
    )?;
    let request: WorkerRequest = serde_json::from_slice(&bytes).map_err(|_| mismatch())?;
    let WorkerOperation::Generate {
        preset, settings, ..
    } = &request.operation
    else {
        return Err(mismatch());
    };
    if request.project_id != origin.project_id
        || request.job_id != origin.job_id
        || request.attempt_id != origin.attempt_id
        || request.source.source_version != origin.source_version
        || encode(&request.source.identity)? != encode(&origin.source_identity)?
        || preset != &origin.preset
        || encode(settings)? != encode(&origin.settings)?
        || candidate.motion.sha256 != origin.program.sha256
        || candidate.motion.byte_len != origin.program.byte_len
        || candidate.motion.artifact_id != origin.program.artifact_id
    {
        return Err(mismatch());
    }
    let verified = verify_plan(
        &graph.root,
        namespace,
        candidate,
        record,
        &request,
        pool,
        store,
    )?;
    let expected = ArchivedGenerationClock {
        receipt: verified.receipt.clone(),
        sample_count: verified.plan.samples.len() as u64,
        source_origin: verified.plan.source_origin,
        project_trim: verified.plan.project_trim,
    };
    if encode(&origin.clock)? != encode(&Some(expected))? {
        return Err(mismatch());
    }
    Ok(verified)
}
