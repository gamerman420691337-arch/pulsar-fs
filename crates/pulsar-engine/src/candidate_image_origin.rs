//! Read-only inspection of accepted image generation, including inert clones.
use super::*;
use std::collections::BTreeSet;

fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "candidate image origin correspondence changed",
    )
}
fn absent() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::NotFound,
        "candidate has no retained image-conditioned generation origin",
    )
}
fn identity(snapshot: &CandidateSnapshot) -> ArtifactIdentity {
    ArtifactIdentity {
        artifact_id: snapshot.motion.program.artifact_id.clone(),
        sha256: snapshot.motion.program.sha256.clone(),
        byte_len: snapshot.motion.program.byte_len,
    }
}
impl Engine {
    pub(super) fn candidate_image_pixels(
        &self,
        db: &Connection,
        project: &ProjectId,
        candidate: &CandidateId,
        record_sha256: &str,
    ) -> PResult<CandidateImageOriginPixels> {
        let _memory = self.pool.reserve_memory(32 * 1024 * 1024)?;
        validate_sha256(record_sha256)?;
        let report = self.candidate_image_origin(db, project, candidate)?;
        let origin = report
            .origins
            .iter()
            .find(|o| o.record.sha256 == record_sha256)
            .ok_or_else(absent)?;
        let image = origin.approval.report.image.as_ref().ok_or_else(mismatch)?;
        // These paths name current engine-owned CAS objects, never historical
        // worker/media suppliers or caller-controlled paths.
        let path = self
            .config
            .state_dir
            .join(if origin.origin_manifest.is_some() {
                "imported-project-evidence"
            } else {
                "snapshots"
            })
            .join(&image.rgb.sha256);
        assistant_image_origin::read_identity(&path, &image.rgb, 512 * 512 * 3)?;
        let frame = FrameArtifact {
            artifact: ArtifactReference {
                path,
                identity: image.rgb.clone(),
            },
            width: image.width,
            height: image.height,
            stride_bytes: u64::from(image.width) * 3,
            format: PixelFormat::Rgb8,
        };
        let record = origin.record.clone();
        let pixels = CandidateImageOriginPixels {
            report,
            record,
            frame,
        };
        pixels.validate()?;
        Ok(pixels)
    }
    pub(super) fn candidate_image_origin(
        &self,
        db: &Connection,
        project: &ProjectId,
        candidate: &CandidateId,
    ) -> PResult<CandidateImageOriginReport> {
        // Retain admission for decoded summaries and their bounded encoding
        // after per-object verification reservations are released.
        let _report_memory = self.pool.reserve_memory(32 * 1024 * 1024)?;
        let bounded: Option<bool> = db.query_row(
            "SELECT typeof(review)='text' AND length(CAST(review AS BLOB)) BETWEEN 2 AND 1048576 FROM candidates WHERE id=?1 AND project=?2",
            params![candidate.as_str(),project.as_str()], |r| r.get(0),
        ).optional().map_err(internal)?;
        if bounded == Some(false) {
            return Err(mismatch());
        }
        let snapshot = candidate_snapshot(db, candidate, project)?;
        let mut origins = if let Some(job) = &snapshot.job_id {
            vec![self.local_image_origin(db, &snapshot, job)?]
        } else {
            self.imported_image_origins(db, &snapshot)?
        };
        origins.sort_by(|a, b| {
            (
                a.origin_manifest.as_ref().map(|m| &m.sha256),
                &a.candidate_id,
            )
                .cmp(&(
                    b.origin_manifest.as_ref().map(|m| &m.sha256),
                    &b.candidate_id,
                ))
        });
        let report = CandidateImageOriginReport {
            project_id: project.clone(),
            candidate_id: candidate.clone(),
            base_revision: snapshot.base_revision,
            motion: snapshot.motion,
            origins,
            observed_temporal_motion: false,
            execution_authority: false,
            qualified: false,
        };
        report.validate()?;
        Ok(report)
    }

    fn local_image_origin(
        &self,
        db: &Connection,
        snapshot: &CandidateSnapshot,
        job: &JobId,
    ) -> PResult<ImageGenerationOrigin> {
        let _memory = self
            .pool
            .reserve_memory(16 * PROGRAM_LIMIT as u64 + 16 * 1024 * 1024)?;
        let row: Option<(String, String, String, Option<String>)> = db.query_row(
            "SELECT state,attempt,CASE WHEN length(CAST(manifest AS BLOB)) BETWEEN 1 AND 524288 THEN manifest ELSE NULL END,candidate FROM jobs WHERE id=?1 AND project=?2",
            params![job.as_str(), snapshot.project_id.as_str()],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
        ).optional().map_err(internal)?;
        let (state, attempt, raw, original_candidate) = row.ok_or_else(absent)?;
        let manifest: WorkerRequest = decode(&raw)?;
        manifest.validate()?;
        if state != "completed"
            || manifest.job_id != *job
            || manifest.attempt_id.as_str() != attempt
            || manifest.project_id != snapshot.project_id
            || manifest.cuda_execution.is_some()
        {
            return Err(mismatch());
        }
        let Some((record, bytes, _)) =
            assistant_image_origin::retained(&self.config.state_dir, &manifest)?
        else {
            return Err(absent());
        };
        let raw_lineage: String = db.query_row(
            "SELECT CASE WHEN length(CAST(lineage AS BLOB)) BETWEEN 1 AND 524288 THEN lineage ELSE NULL END FROM candidates WHERE id=?1 AND project=?2",
            params![snapshot.candidate_id.as_str(), snapshot.project_id.as_str()], |r| r.get(0),
        ).map_err(internal)?;
        let lineage: Vec<ArtifactIdentity> = decode(&raw_lineage)?;
        let receipt = lineage.last().ok_or_else(mismatch)?;
        if !lineage.contains(&record) || manifest.dependencies.iter().any(|d| !lineage.contains(d))
        {
            return Err(mismatch());
        }
        let root = self
            .config
            .state_dir
            .join("attempts")
            .join(manifest.attempt_id.as_str());
        let receipt_bytes =
            assistant_image_origin::read_identity(&root.join("receipt.json"), receipt, 128 * 1024)?;
        let value: serde_json::Value =
            serde_json::from_slice(&receipt_bytes).map_err(|_| mismatch())?;
        let worker_program: ArtifactIdentity =
            serde_json::from_value(value["program"].clone()).map_err(|_| mismatch())?;
        worker_program.validate()?;
        if worker_program.byte_len > MAX_MOTION_BYTES {
            return Err(mismatch());
        }
        assistant_image_origin::verify_receipt(
            &manifest,
            &root.join("receipt.json"),
            receipt,
            &worker_program,
            &lineage[..lineage.len() - 1],
        )?;
        let _motion_memory = self.pool.reserve_memory(
            worker_program
                .byte_len
                .checked_mul(8)
                .and_then(|n| n.checked_add(16 * 1024 * 1024))
                .ok_or_else(mismatch)?,
        )?;
        let motion_bytes = assistant_image_origin::read_identity(
            &root.join("program.json"),
            &worker_program,
            MAX_MOTION_BYTES,
        )?;
        let motion: MotionProgram =
            serde_json::from_slice(&motion_bytes).map_err(|_| mismatch())?;
        if self
            .motion_store
            .read(&snapshot.motion.program)
            .map_err(unavailable)?
            != motion
        {
            return Err(mismatch());
        }
        let original_candidate =
            CandidateId::new(original_candidate.ok_or_else(mismatch)?).map_err(internal)?;
        let anchor = candidate_snapshot(db, &original_candidate, &snapshot.project_id)?;
        if anchor.job_id.as_ref() != Some(job) || anchor.motion.program != snapshot.motion.program {
            return Err(mismatch());
        }
        Ok(ImageGenerationOrigin {
            origin_manifest: None,
            candidate_id: original_candidate,
            job_id: manifest.job_id.clone(),
            attempt_id: manifest.attempt_id.clone(),
            worker_manifest: PackageObjectRef {
                sha256: format!("{:x}", Sha256::digest(raw.as_bytes())),
                byte_len: raw.len() as u64,
            },
            worker_receipt: receipt.clone(),
            worker_program,
            program: identity(snapshot),
            record,
            approval: assistant_image_origin::approval_record(&bytes, &manifest)?,
        })
    }

    fn imported_image_origins(
        &self,
        db: &Connection,
        snapshot: &CandidateSnapshot,
    ) -> PResult<Vec<ImageGenerationOrigin>> {
        let graph = archived_project_origins::load(
            db,
            &snapshot.project_id,
            None,
            &self.pool,
            &self.import_store,
        )?;
        let anchor = graph
            .root
            .candidates
            .iter()
            .find(|c| c.candidate_id == snapshot.candidate_id)
            .ok_or_else(absent)?;
        let raw: String = db.query_row(
            "SELECT CASE WHEN length(CAST(c.lineage AS BLOB)) BETWEEN 1 AND 524288 THEN c.lineage ELSE NULL END FROM candidates c JOIN project_import_candidate_aliases a ON a.candidate=c.id AND a.project=c.project WHERE c.id=?1 AND c.project=?2 AND a.imported_candidate=c.id",
            params![snapshot.candidate_id.as_str(), snapshot.project_id.as_str()], |r| r.get(0),
        ).optional().map_err(internal)?.ok_or_else(absent)?;
        let lineage: Vec<ArtifactIdentity> = decode(&raw)?;
        if anchor.motion != snapshot.motion.program || anchor.lineage != lineage {
            return Err(mismatch());
        }
        let mut pending = vec![(&graph.root, snapshot.candidate_id.clone(), None)];
        let mut visited = BTreeSet::new();
        let mut origins = vec![];
        while let Some((namespace, id, namespace_ref)) = pending.pop() {
            let key = (
                namespace_ref
                    .as_ref()
                    .map(|m: &PackageObjectRef| m.sha256.clone()),
                id.clone(),
            );
            if !visited.insert(key) {
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
                let selected: Vec<_> = namespace
                    .generated_origins
                    .iter()
                    .filter(|o| {
                        o.job_id == *job && archived_generation::has_producer_receipt(original, o)
                    })
                    .collect();
                if selected.len() != 1 || namespace_ref.is_none() {
                    return Err(mismatch());
                }
                let origin = selected[0];
                if origin.image_origin.is_none()
                    && original.lineage.iter().any(|d| {
                        d.artifact_id
                            .as_str()
                            .starts_with("assistant-image-origin-v1:")
                    })
                {
                    return Err(mismatch());
                }
                if let Some(image) = &origin.image_origin {
                    if origins.len() >= 16 {
                        return Err(mismatch());
                    }
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
                    let _memory = self.pool.reserve_memory(
                        evidence_bytes
                            .checked_mul(8)
                            .and_then(|n| n.checked_add(16 * 1024 * 1024))
                            .ok_or_else(mismatch)?,
                    )?;
                    project_package_imports::validate_evidence(
                        namespace,
                        &self.pool,
                        |r| {
                            archived_generation::read(
                                &self.import_store,
                                &graph.root,
                                r,
                                None,
                                &self.pool,
                            )
                        },
                        &mut || Ok(()),
                    )?;
                    let raw = archived_generation::read(
                        &self.import_store,
                        &graph.root,
                        &origin.manifest,
                        Some(PackageObjectRole::WorkerManifest),
                        &self.pool,
                    )?;
                    let request: WorkerRequest =
                        serde_json::from_slice(&raw).map_err(|_| mismatch())?;
                    project_package_imports::checked_worker_scope(namespace, origin, &request)?;
                    let retained_image = assistant_image_origin::archived(&request, |identity| {
                        archived_generation::read(
                            &self.import_store,
                            &graph.root,
                            &PackageObjectRef {
                                sha256: identity.sha256.clone(),
                                byte_len: identity.byte_len,
                            },
                            Some(PackageObjectRole::DependencyEvidence),
                            &self.pool,
                        )
                    })?
                    .ok_or_else(mismatch)?;
                    if retained_image.rgb != image.rgb || original.motion != snapshot.motion.program
                    {
                        return Err(mismatch());
                    }
                    let bytes = archived_generation::read(
                        &self.import_store,
                        &graph.root,
                        &PackageObjectRef {
                            sha256: image.record.sha256.clone(),
                            byte_len: image.record.byte_len,
                        },
                        Some(PackageObjectRole::DependencyEvidence),
                        &self.pool,
                    )?;
                    origins.push(ImageGenerationOrigin {
                        origin_manifest: namespace_ref.clone(),
                        candidate_id: id.clone(),
                        job_id: request.job_id.clone(),
                        attempt_id: request.attempt_id.clone(),
                        worker_manifest: origin.manifest.clone(),
                        worker_receipt: origin.receipt.clone().ok_or_else(mismatch)?,
                        worker_program: origin.program.clone().ok_or_else(mismatch)?,
                        program: identity(snapshot),
                        record: image.record.clone(),
                        approval: assistant_image_origin::approval_record(&bytes, &request)?,
                    });
                }
            }
            for edge in &namespace.imported_origins {
                if let Some(mapping) = edge.candidates.iter().find(|m| m.local == id) {
                    pending.push((
                        graph
                            .origins
                            .get(&edge.manifest.sha256)
                            .ok_or_else(mismatch)?,
                        mapping.origin.clone(),
                        Some(edge.manifest.clone()),
                    ));
                }
            }
        }
        if origins.is_empty() {
            return Err(absent());
        }
        Ok(origins)
    }
}

#[cfg(test)]
pub(super) fn retain_pixel_observation(pixels: &CandidateImageOriginPixels) {
    let Some(directory) = std::env::var_os("PULSAR_IMAGE_PIXEL_OBSERVATIONS").map(PathBuf::from)
    else {
        return;
    };
    assert!(directory.is_absolute() && directory.is_dir());
    pixels.validate().unwrap();
    let bytes = assistant_image_origin::read_identity(
        &pixels.frame.artifact.path,
        &pixels.frame.artifact.identity,
        512 * 512 * 3,
    )
    .unwrap();
    let stem = format!(
        "{}-{}",
        pixels.report.project_id, pixels.report.candidate_id
    );
    fs::write(directory.join(format!("{stem}.rgb")), bytes).unwrap();
    fs::write(
        directory.join(format!("{stem}.json")),
        serde_json::to_vec_pretty(pixels).unwrap(),
    )
    .unwrap();
}
