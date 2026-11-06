//! Inert image-conditioned approval evidence. No stored record grants execution.
use super::*;
use std::io::{Read, Write};

const PREFIX: &str = "assistant-image-origin-v1:";
const LIMIT: u64 = 128 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Origin {
    schema_version: u16,
    approval_request: RequestId,
    approval: AssistantCreativeApproval,
    report: AssistantReport,
    configuration_sha256: String,
    observed_temporal_motion: bool,
    qualified: bool,
}
impl Origin {
    fn validate(&self, manifest: &WorkerRequest) -> PResult<()> {
        self.report.validate()?;
        let image = self
            .report
            .image
            .as_ref()
            .ok_or_else(|| ProtocolError::invalid("image origin lacks prepared context"))?;
        let WorkerOperation::GenerateInput {
            input: GenerationInput::Patterns { patterns },
        } = &manifest.operation
        else {
            return Err(ProtocolError::invalid(
                "image origin requires synthetic patterns",
            ));
        };
        let Some(AssistantProposal::SynthesizePatterns { patterns: proposed }) =
            &self.report.proposal
        else {
            return Err(ProtocolError::invalid(
                "image origin lacks synthetic proposal",
            ));
        };
        let exact = AssistantCreativeApproval::from_report(&self.report)?;
        let configuration = serde_json::to_vec(&GenerationInput::Patterns {
            patterns: patterns.clone(),
        })
        .map_err(internal)?;
        if self.schema_version != 1
            || self.observed_temporal_motion
            || self.qualified
            || self.report.project_id != manifest.project_id
            || self.report.base_revision != manifest.base_revision
            || serde_json::to_vec(&self.approval).map_err(internal)?
                != serde_json::to_vec(&exact).map_err(internal)?
            || serde_json::to_vec(patterns).map_err(internal)?
                != serde_json::to_vec(proposed).map_err(internal)?
            || manifest.source.identity.sha256 != self.configuration_sha256
            || manifest.source.identity.byte_len != configuration.len() as u64
            || self.configuration_sha256 != format!("{:x}", Sha256::digest(configuration))
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "image origin differs from approved patterns or namespace",
            ));
        }
        for identity in [
            &image.source,
            &image.rgb,
            &self.report.profile.worker,
            &image.ffmpeg,
            &image.ffprobe,
            &self.report.profile.model,
            &self.report.profile.runtime,
            self.report
                .profile
                .vision_projector
                .as_ref()
                .ok_or_else(|| ProtocolError::invalid("image origin lacks projector"))?,
        ] {
            if !manifest.dependencies.contains(identity) {
                return Err(ProtocolError::invalid("image origin dependency is absent"));
            }
        }
        Ok(())
    }
}
fn mismatch(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::DependencyMismatch, message)
}
pub(super) fn read_identity(
    path: &Path,
    identity: &ArtifactIdentity,
    limit: u64,
) -> PResult<Vec<u8>> {
    identity.validate()?;
    if identity.byte_len > limit {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "image origin exceeds capture bound",
        ));
    }
    let mut bytes = Vec::new();
    artifacts::open_regular(path)
        .map_err(unavailable)?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(unavailable)?;
    if bytes.len() as u64 != identity.byte_len
        || format!("{:x}", Sha256::digest(&bytes)) != identity.sha256
    {
        return Err(mismatch("image origin bytes changed"));
    }
    Ok(bytes)
}
pub(super) fn approval_record(
    bytes: &[u8],
    manifest: &WorkerRequest,
) -> PResult<ImageGenerationApprovalRecord> {
    let origin: Origin =
        serde_json::from_slice(bytes).map_err(|_| mismatch("invalid image origin record"))?;
    origin.validate(manifest)?;
    let record: ImageGenerationApprovalRecord =
        serde_json::from_slice(bytes).map_err(|_| mismatch("invalid image approval record"))?;
    record.validate()?;
    Ok(record)
}
/// Explicit prefix reserves inert metadata, while existing worker/wire shapes stay unchanged.
pub(super) fn retained(
    root: &Path,
    manifest: &WorkerRequest,
) -> PResult<Option<(ArtifactIdentity, Vec<u8>, AssistantImageContext)>> {
    let matches: Vec<_> = manifest
        .dependencies
        .iter()
        .filter(|d| d.artifact_id.as_str().starts_with(PREFIX))
        .collect();
    if matches.len() > 1 {
        return Err(mismatch("multiple image origins declared"));
    }
    let Some(identity) = matches.first() else {
        return Ok(None);
    };
    if identity.artifact_id.as_str() != format!("{PREFIX}{}", identity.sha256) {
        return Err(mismatch("image origin label differs from hash"));
    }
    let bytes = read_identity(
        &root.join("snapshots").join(&identity.sha256),
        identity,
        LIMIT,
    )?;
    let origin: Origin = serde_json::from_slice(&bytes)
        .map_err(|_| ProtocolError::invalid("invalid image origin record"))?;
    origin.validate(manifest)?;
    let image = origin.report.image.expect("validated image origin");
    read_identity(
        &root.join("snapshots").join(&image.rgb.sha256),
        &image.rgb,
        512 * 512 * 3,
    )?;
    Ok(Some(((*identity).clone(), bytes, image)))
}
pub(super) fn archived(
    manifest: &WorkerRequest,
    mut read: impl FnMut(&ArtifactIdentity) -> PResult<Vec<u8>>,
) -> PResult<Option<AssistantImageContext>> {
    let identities: Vec<_> = manifest
        .dependencies
        .iter()
        .filter(|d| d.artifact_id.as_str().starts_with(PREFIX))
        .collect();
    if identities.len() > 1 {
        return Err(mismatch("multiple archived image origins declared"));
    }
    let Some(identity) = identities.first() else {
        return Ok(None);
    };
    identity.validate()?;
    if identity.byte_len > LIMIT
        || identity.artifact_id.as_str() != format!("{PREFIX}{}", identity.sha256)
    {
        return Err(mismatch("archived image origin declaration differs"));
    }
    let bytes = read(identity)?;
    if bytes.len() as u64 != identity.byte_len
        || format!("{:x}", Sha256::digest(&bytes)) != identity.sha256
    {
        return Err(mismatch("archived image origin bytes changed"));
    }
    let origin: Origin = serde_json::from_slice(&bytes)
        .map_err(|_| ProtocolError::invalid("invalid archived image origin"))?;
    origin.validate(manifest)?;
    let image = origin.report.image.expect("validated origin");
    let rgb = read(&image.rgb)?;
    if rgb.len() as u64 != image.rgb.byte_len
        || format!("{:x}", Sha256::digest(&rgb)) != image.rgb.sha256
    {
        return Err(mismatch("archived image pixels changed"));
    }
    Ok(Some(image))
}
pub(super) fn verify_receipt(
    manifest: &WorkerRequest,
    path: &Path,
    identity: &ArtifactIdentity,
    program: &ArtifactIdentity,
    lineage: &[ArtifactIdentity],
) -> PResult<()> {
    if !manifest
        .dependencies
        .iter()
        .any(|d| d.artifact_id.as_str().starts_with(PREFIX))
    {
        return Ok(());
    }
    let bytes = read_identity(path, identity, LIMIT)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| ProtocolError::invalid("invalid image generation receipt"))?;
    let dependencies: Vec<ArtifactIdentity> = serde_json::from_value(value["dependencies"].clone())
        .map_err(|_| ProtocolError::invalid("invalid image receipt dependencies"))?;
    if value["schema_version"] != 2
        || value["recipe"] != "explicit-six-axis-pattern-v1"
        || value["qualification"] != "unqualified"
        || value["evidence"] != "synthesized"
        || value["details"]["observed_motion"] != false
        || value["project_id"] != serde_json::to_value(&manifest.project_id).map_err(internal)?
        || value["base_revision"]
            != serde_json::to_value(manifest.base_revision).map_err(internal)?
        || value["job_id"] != serde_json::to_value(&manifest.job_id).map_err(internal)?
        || value["attempt_id"] != serde_json::to_value(&manifest.attempt_id).map_err(internal)?
        || value["source_version"]
            != serde_json::to_value(&manifest.source.source_version).map_err(internal)?
        || value["program"] != serde_json::to_value(program).map_err(internal)?
        || value["configuration_sha256"] != manifest.source.identity.sha256
        || dependencies != lineage
    {
        return Err(mismatch(
            "image receipt differs from worker patterns, program or provenance",
        ));
    }
    Ok(())
}

impl Engine {
    pub(super) fn capture_image_origin(
        &self,
        db: &Connection,
        request: &Request,
        input: &GenerationInput,
    ) -> PResult<Vec<ArtifactIdentity>> {
        let Command::GenerateCreativeWithAssistant { approval } = &request.command else {
            return Ok(vec![]);
        };
        let report = assistant_operations::read(db, &approval.attempt_id)?;
        let Some(image) = report.image.as_ref() else {
            return Ok(vec![]);
        };
        let rgb_path = self.require_assistant_image(db, request, image)?;
        let _claim = self
            .pool
            .reserve_memory(LIMIT + 512 * 512 * 3)?
            .with_storage(
                2 * LIMIT + 512 * 512 * 3 + 16 * 1024,
                fs2::available_space(&self.config.state_dir).map_err(unavailable)?,
            )?;
        let (_, sha, size) = artifacts::snapshot_reserved(
            &rgb_path,
            &self.config.state_dir.join("snapshots"),
            image.rgb.byte_len,
            self.pool.reserved_storage(),
        )
        .map_err(unavailable)?;
        if sha != image.rgb.sha256 || size != image.rgb.byte_len {
            return Err(mismatch("image pixels changed during provenance capture"));
        }
        let origin = Origin {
            schema_version: 1,
            approval_request: request.request_id.clone(),
            approval: approval.clone(),
            report: report.clone(),
            configuration_sha256: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(input).map_err(internal)?)
            ),
            observed_temporal_motion: false,
            qualified: false,
        };
        let bytes = serde_json::to_vec(&origin).map_err(internal)?;
        if bytes.len() as u64 > LIMIT {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "image origin exceeds capture bound",
            ));
        }
        let mut temporary =
            tempfile::NamedTempFile::new_in(self.config.state_dir.join("snapshots"))
                .map_err(unavailable)?;
        temporary.write_all(&bytes).map_err(unavailable)?;
        temporary.as_file().sync_all().map_err(unavailable)?;
        let (_, sha256, byte_len) = artifacts::snapshot_reserved(
            temporary.path(),
            &self.config.state_dir.join("snapshots"),
            LIMIT,
            self.pool.reserved_storage(),
        )
        .map_err(unavailable)?;
        let mut captured = ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).map_err(internal)?,
            sha256,
            byte_len,
        };
        captured.artifact_id =
            ArtifactId::new(format!("{PREFIX}{}", captured.sha256)).map_err(internal)?;
        let mut dependencies = vec![
            captured,
            image.source.clone(),
            image.rgb.clone(),
            image.ffmpeg.clone(),
            image.ffprobe.clone(),
            report.profile.worker.clone(),
            report.profile.model.clone(),
            report.profile.runtime.clone(),
            report
                .profile
                .vision_projector
                .clone()
                .expect("validated image report"),
        ];
        dependencies.dedup();
        Ok(dependencies)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Origin, WorkerRequest, Vec<u8>) {
        let identity = |name: &str, bytes: &[u8]| ArtifactIdentity {
            artifact_id: ArtifactId::new(name).unwrap(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
            byte_len: bytes.len() as u64,
        };
        let rgb = vec![11, 22, 33];
        let image = AssistantImageContext {
            image_id: "c".repeat(64),
            project_id: ProjectId::new("project").unwrap(),
            session_id: SessionId::new("owner").unwrap(),
            revision: RevisionId::new(0),
            source_version: SourceVersionId::new("still").unwrap(),
            source: identity("still", b"image"),
            rgb: identity("pixels", &rgb),
            width: 1,
            height: 1,
            preprocessing_version: 1,
            ffmpeg: identity("ffmpeg", b"ffmpeg"),
            ffprobe: identity("ffprobe", b"ffprobe"),
        };
        let profile = AssistantProfile {
            profile_id: AttemptId::new("vision").unwrap(),
            worker: identity("worker", b"worker"),
            runtime: identity("runtime", b"runtime"),
            model: identity("model", b"model"),
            vision_projector: Some(identity("projector", b"projector")),
            cpu_threads: 2,
            wall_time_ms: 120000,
            worker_memory_bytes: 2 * 1024 * 1024 * 1024,
        };
        let patterns = vec![parse_pattern_prompt("pattern=triangle axis=stroke duration=2s frequency=1hz amplitude=0.1 offset=0.5 sample_hz=8").unwrap()];
        let input = GenerationInput::Patterns {
            patterns: patterns.clone(),
        };
        let raw = serde_json::to_vec(&input).unwrap();
        let report = AssistantReport {
            image: Some(image.clone()),
            attempt_id: AttemptId::new("assistant").unwrap(),
            project_id: image.project_id.clone(),
            session_id: image.session_id.clone(),
            request_id: RequestId::new("start").unwrap(),
            base_revision: image.revision,
            engine_epoch: AttemptId::new("epoch").unwrap(),
            profile: profile.clone(),
            context_sha256: "a".repeat(64),
            scope: AssistantProposalScope {
                creative_patterns: Some(CreativePatternScope {
                    axes: vec![Axis::Stroke],
                    duration: ProjectTime::from_nanos(2_000_000_000),
                    sample_hz: 8,
                    max_amplitude: 0.1,
                }),
                ..Default::default()
            },
            state: AssistantState::Completed,
            proposal: Some(AssistantProposal::SynthesizePatterns { patterns }),
            error: None,
            worker_peak_memory_bytes: Some(1024),
        };
        report.validate().unwrap();
        let origin = Origin {
            schema_version: 1,
            approval_request: RequestId::new("generate").unwrap(),
            approval: AssistantCreativeApproval::from_report(&report).unwrap(),
            report,
            configuration_sha256: format!("{:x}", Sha256::digest(&raw)),
            observed_temporal_motion: false,
            qualified: false,
        };
        let tool = ArtifactReference {
            path: PathBuf::from("/tool"),
            identity: identity("tool", b"tool"),
        };
        let mut manifest = WorkerRequest {
            cuda_execution: None,
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("generation").unwrap(),
            job_id: JobId::new("job").unwrap(),
            project_id: image.project_id,
            base_revision: image.revision,
            source: SourceArtifact {
                source_version: SourceVersionId::new("pattern").unwrap(),
                path: PathBuf::from("/source"),
                identity: identity("pattern", &raw),
            },
            independent_audio: None,
            output_dir: PathBuf::from("/output"),
            operation: WorkerOperation::GenerateInput { input },
            budget: ResourceBudget {
                memory_bytes: 2 * 1024 * 1024 * 1024,
                output_bytes: 8 * 1024 * 1024,
                wall_time_ms: 10000,
                cpu_threads: 2,
            },
            dependencies: vec![
                image.source,
                image.rgb,
                image.ffmpeg,
                image.ffprobe,
                profile.worker,
                profile.model,
                profile.runtime,
                profile.vision_projector.unwrap(),
            ],
            tools: WorkerTools {
                ffmpeg: tool.clone(),
                ffprobe: tool,
                onnx_runtime: None,
            },
        };
        let bytes = serde_json::to_vec(&origin).unwrap();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        manifest.dependencies.push(ArtifactIdentity {
            artifact_id: ArtifactId::new(format!("{PREFIX}{hash}")).unwrap(),
            sha256: hash,
            byte_len: bytes.len() as u64,
        });
        manifest.validate().unwrap();
        (origin, manifest, rgb)
    }
    #[test]
    fn origin_binds_patterns_namespace_approval_and_nonqualification() {
        let (origin, manifest, _) = fixture();
        origin.validate(&manifest).unwrap();
        for kind in 0..6 {
            let mut bad = origin.clone();
            match kind {
                0 => bad.qualified = true,
                1 => bad.observed_temporal_motion = true,
                2 => bad.configuration_sha256 = "0".repeat(64),
                3 => bad.approval.context_sha256 = "0".repeat(64),
                4 => bad.report.base_revision = RevisionId::new(1),
                _ => bad.report.proposal = Some(AssistantProposal::NoAction {}),
            }
            assert!(bad.validate(&manifest).is_err());
        }
        let mut bad = manifest.clone();
        bad.project_id = ProjectId::new("foreign").unwrap();
        assert!(origin.validate(&bad).is_err());
        let mut bad = manifest.clone();
        bad.dependencies.remove(0);
        assert!(origin.validate(&bad).is_err());
    }
    #[test]
    fn image_worker_receipt_cannot_omit_provenance_or_change_program_and_qualification() {
        let (_, manifest, _) = fixture();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("receipt.json");
        let program = manifest.source.identity.clone();
        let value = serde_json::json!({"schema_version":2,"recipe":"explicit-six-axis-pattern-v1","qualification":"unqualified","evidence":"synthesized","details":{"observed_motion":false},"project_id":manifest.project_id,"base_revision":manifest.base_revision,"job_id":manifest.job_id,"attempt_id":manifest.attempt_id,"source_version":manifest.source.source_version,"program":program,"configuration_sha256":manifest.source.identity.sha256,"dependencies":manifest.dependencies});
        for kind in 0..4 {
            let mut current = value.clone();
            match kind {
                0 => {}
                1 => current["dependencies"] = serde_json::json!([]),
                2 => current["program"]["sha256"] = serde_json::json!("0".repeat(64)),
                _ => current["qualification"] = serde_json::json!("qualified"),
            }
            let bytes = serde_json::to_vec(&current).unwrap();
            std::fs::write(&path, &bytes).unwrap();
            let identity = ArtifactIdentity {
                artifact_id: ArtifactId::new("receipt").unwrap(),
                sha256: format!("{:x}", Sha256::digest(&bytes)),
                byte_len: bytes.len() as u64,
            };
            assert_eq!(
                verify_receipt(
                    &manifest,
                    &path,
                    &identity,
                    &program,
                    &manifest.dependencies
                )
                .is_ok(),
                kind == 0
            );
        }
    }
    #[test]
    fn required_origin_and_pixels_survive_archive_reads_and_refuse_missing_or_changed_bytes() {
        let (origin, manifest, rgb) = fixture();
        let raw = serde_json::to_vec(&origin).unwrap();
        let read = |identity: &ArtifactIdentity| {
            if identity.artifact_id.as_str().starts_with(PREFIX) {
                Ok(raw.clone())
            } else {
                Ok(rgb.clone())
            }
        };
        assert_eq!(archived(&manifest, read).unwrap(), origin.report.image);
        assert!(archived(&manifest, |_| Err(ProtocolError::invalid("missing"))).is_err());
        assert!(archived(&manifest, |identity| {
            if identity.artifact_id.as_str().starts_with(PREFIX) {
                Ok(raw.clone())
            } else {
                Ok(vec![11, 22, 34])
            }
        })
        .is_err());
        let mut duplicate = manifest.clone();
        duplicate
            .dependencies
            .push(manifest.dependencies.last().unwrap().clone());
        assert!(archived(&duplicate, read).is_err());
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("snapshots")).unwrap();
        let declaration = manifest.dependencies.last().unwrap();
        let image = origin.report.image.as_ref().unwrap();
        let metadata = root.path().join("snapshots").join(&declaration.sha256);
        std::fs::write(&metadata, &raw).unwrap();
        let pixels = root.path().join("snapshots").join(&image.rgb.sha256);
        std::fs::write(&pixels, &rgb).unwrap();
        assert!(retained(root.path(), &manifest).unwrap().is_some());
        std::fs::write(&pixels, [11, 22, 34]).unwrap();
        assert!(retained(root.path(), &manifest).is_err());
        std::fs::write(&pixels, rgb).unwrap();
        std::fs::write(&metadata, b"{}").unwrap();
        assert!(retained(root.path(), &manifest).is_err());
        #[cfg(unix)]
        {
            std::fs::remove_file(&metadata).unwrap();
            std::os::unix::fs::symlink(&pixels, &metadata).unwrap();
            assert!(retained(root.path(), &manifest).is_err());
        }
        let mut legacy = manifest;
        legacy
            .dependencies
            .retain(|d| !d.artifact_id.as_str().starts_with(PREFIX));
        assert!(archived(&legacy, |_| panic!(
            "legacy path must not read image assets"
        ))
        .unwrap()
        .is_none());
    }
}
