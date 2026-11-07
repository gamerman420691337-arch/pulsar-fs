//! Ephemeral Engine-issued still pixels. No client path, observation or effect authority.
use super::*;
use std::collections::VecDeque;

struct Image {
    context: AssistantImageContext,
    path: PathBuf,
    directory: PathBuf,
    _claim: Reservation,
}
#[derive(Default)]
pub(super) struct Registry {
    images: VecDeque<Image>,
    pending: Vec<(PathBuf, Reservation)>,
}
fn remove_directory(path: &Path) -> PResult<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(unavailable(e)),
    }
}
impl Registry {
    fn prepare_capacity(&mut self) -> PResult<()> {
        while let Some((path, _claim)) = self.pending.last() {
            remove_directory(path)?;
            self.pending.pop();
        }
        while self.images.len() >= 32 {
            remove_directory(&self.images.front().expect("full registry").directory)?;
            self.images.pop_front();
        }
        Ok(())
    }
    fn path(&self, expected: &AssistantImageContext) -> PResult<PathBuf> {
        let image = self
            .images
            .iter()
            .find(|i| i.context.image_id == expected.image_id)
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::Unavailable,
                    "prepared image expired or belongs to another Engine lifetime",
                )
            })?;
        if &image.context != expected {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "prepared image differs from Engine-issued context",
            ));
        }
        Ok(image.path.clone())
    }
}
impl Drop for Registry {
    fn drop(&mut self) {
        for image in &self.images {
            let _ = remove_directory(&image.directory);
        }
        for (path, _) in &self.pending {
            let _ = remove_directory(path);
        }
    }
}
/// Owns the additional storage claim before decoding starts. Failed cleanup
/// transfers both the directory and claim to the registry; new preparation is
/// refused until retry succeeds. Only Engine-created attempt directories enter.
pub(super) struct PendingImage<'a> {
    engine: &'a Engine,
    directory: PathBuf,
    claim: Option<Reservation>,
}
impl<'a> PendingImage<'a> {
    pub(super) fn new(engine: &'a Engine, directory: PathBuf, claim: Reservation) -> Self {
        Self {
            engine,
            directory,
            claim: Some(claim),
        }
    }
}
impl Drop for PendingImage<'_> {
    fn drop(&mut self) {
        if let Some(claim) = self.claim.take() {
            if remove_directory(&self.directory).is_err() {
                let mut registry = self
                    .engine
                    .assistant_images
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                registry.pending.push((self.directory.clone(), claim));
            }
        }
    }
}
impl Engine {
    pub(super) fn prepare_assistant_image(
        &self,
        request: &Request,
        source_version: &SourceVersionId,
    ) -> PResult<ResponseBody> {
        {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(
                &db,
                request,
                request.session.as_ref().ok_or_else(|| {
                    ProtocolError::new(ErrorCode::Unauthorized, "pairing required")
                })?,
            )?;
            check_revision(&db, request)?;
            source_kinds::require_media(&db, project(request)?, source_version)?;
        }
        self.assistant_images
            .lock()
            .map_err(internal)?
            .prepare_capacity()?;
        // Reserve the entire worker output ceiling before it can create files.
        // Retain this charge until eviction or confirmed deletion, including errors.
        let claim = self.pool.reserve_memory(16 * 1024)?.with_storage(
            8 * 1024 * 1024,
            fs2::available_space(&self.config.state_dir).map_err(internal)?,
        )?;
        let context = FrameContext {
            source_version: source_version.clone(),
            source_placement: SourcePlacementId::new("assistant-still").map_err(internal)?,
            frame: FrameId::new(0),
            transform: video_view_transform(None)?,
            seek_generation: 0,
            request_generation: 0,
        };
        self.preview(
            request,
            PreviewIntent {
                assistant_image_claim: Some(claim),
                context: &context,
                source_time: SourceTimestamp::new(0, 1).map_err(internal)?,
                time_basis: PreviewTimeBasis::ElapsedFromFirstFrame,
                model_path: None,
                model_input: None,
                inference_profile: None,
                video_view: None,
                placement_seek: None,
                model_pack: None,
                annotated: None,
            },
        )
    }
    pub(super) fn retain_assistant_image(
        &self,
        db: &Connection,
        request: &Request,
        manifest: &WorkerRequest,
        preview: &PreviewResult,
        pending: &mut PendingImage<'_>,
    ) -> PResult<ResponseBody> {
        check_revision(db, request)?;
        let frame = preview
            .frame
            .as_ref()
            .ok_or_else(|| ProtocolError::invalid("prepared still pixels absent"))?;
        if preview.source_frame_index != 0
            || preview.context.frame != FrameId::new(0)
            || preview.coordinate_map.is_some()
            || preview.placed.is_some()
            || preview.annotated.is_some()
            || frame.format != PixelFormat::Rgb8
            || frame.stride_bytes != u64::from(frame.width) * 3
        {
            return Err(ProtocolError::invalid(
                "prepared image requires unprojected packed first-frame RGB",
            ));
        }
        let current = self.source(db, project(request)?, &manifest.source.source_version)?;
        if current.identity != manifest.source.identity {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "prepared image source changed during decoding",
            ));
        }
        let mut context = AssistantImageContext {
            image_id: String::new(),
            project_id: project(request)?.clone(),
            session_id: request
                .session
                .clone()
                .ok_or_else(|| ProtocolError::new(ErrorCode::Unauthorized, "pairing required"))?,
            revision: manifest.base_revision,
            source_version: current.source_version,
            source: current.identity,
            rgb: frame.artifact.identity.clone(),
            width: frame.width,
            height: frame.height,
            preprocessing_version: 1,
            ffmpeg: manifest.tools.ffmpeg.identity.clone(),
            ffprobe: manifest.tools.ffprobe.identity.clone(),
        };
        context.image_id = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(manifest.attempt_id.clone(), &context)).map_err(internal)?
            )
        );
        context.validate_for(request)?;
        let mut registry = self.assistant_images.lock().map_err(internal)?;
        registry.prepare_capacity()?;
        // Attempt identity makes independently prepared identical pixels distinct.
        registry.images.push_back(Image {
            context: context.clone(),
            path: frame.artifact.path.clone(),
            directory: pending.directory.clone(),
            _claim: pending
                .claim
                .take()
                .expect("prepared image storage custody"),
        });
        Ok(ResponseBody::AssistantImage(Box::new(context)))
    }
    pub(super) fn require_assistant_image(
        &self,
        db: &Connection,
        request: &Request,
        image: &AssistantImageContext,
    ) -> PResult<PathBuf> {
        image.validate()?;
        if request.project.as_ref() != Some(&image.project_id)
            || request.session.as_ref() != Some(&image.session_id)
            || request.expected_revision != Some(image.revision)
        {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "prepared image owner/project/revision differs",
            ));
        }
        source_kinds::require_media(db, &image.project_id, &image.source_version)?;
        let source = self.source(db, &image.project_id, &image.source_version)?;
        if source.identity != image.source {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "prepared image source identity changed",
            ));
        }
        let path = self
            .assistant_images
            .lock()
            .map_err(internal)?
            .path(image)?;
        for (path, identity) in [(&source.path, &image.source), (&path, &image.rgb)] {
            let (sha, bytes) =
                artifacts::digest_file(path, identity.byte_len).map_err(unavailable)?;
            if sha != identity.sha256 || bytes != identity.byte_len {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "prepared image or original source bytes changed",
                ));
            }
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_cleanup_keeps_storage_charged_until_confirmed_retry() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("refused-directory");
        // A file where a directory must be is a deterministic deletion refusal.
        fs::write(&path, b"not a directory").unwrap();
        let pool = crate::resources::ResourcePool::new(1 << 20, 1);
        let claim = pool
            .reserve_memory(16 * 1024)
            .unwrap()
            .with_storage(4096, 32 << 20)
            .unwrap();
        let mut registry = Registry::default();
        registry.pending.push((path.clone(), claim));
        assert!(registry.prepare_capacity().is_err());
        assert_eq!(pool.reserved_storage(), 4096);
        assert_eq!(registry.pending.len(), 1);
        fs::remove_file(path).unwrap();
        registry.prepare_capacity().unwrap();
        assert!(registry.pending.is_empty());
        assert_eq!(pool.reserved_storage(), 0);
    }
    #[test]
    fn absent_or_forged_registry_identity_never_issues_a_path() {
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new("test").unwrap(),
            sha256: "a".repeat(64),
            byte_len: 3,
        };
        let context = AssistantImageContext {
            image_id: "b".repeat(64),
            project_id: ProjectId::new("p").unwrap(),
            session_id: SessionId::new("s").unwrap(),
            revision: RevisionId::new(0),
            source_version: SourceVersionId::new("v").unwrap(),
            source: identity.clone(),
            rgb: identity.clone(),
            width: 1,
            height: 1,
            preprocessing_version: 1,
            ffmpeg: identity.clone(),
            ffprobe: identity,
        };
        let root = tempfile::tempdir().unwrap();
        let pool = crate::resources::ResourcePool::new(1 << 20, 1);
        let mut registry = Registry::default();
        assert!(registry.path(&context).is_err());
        registry.images.push_back(Image {
            context: context.clone(),
            path: root.path().join("frame.rgb"),
            directory: root.path().join("owned-attempt"),
            _claim: pool.reserve_memory(16 * 1024).unwrap(),
        });
        assert_eq!(
            registry.path(&context).unwrap(),
            root.path().join("frame.rgb")
        );
        let mut forged = context.clone();
        forged.source.sha256 = "c".repeat(64);
        assert_eq!(
            registry.path(&forged).unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        forged = context;
        forged.session_id = SessionId::new("other").unwrap();
        assert!(registry.path(&forged).is_err());
    }
}
