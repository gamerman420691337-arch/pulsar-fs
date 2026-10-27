//! Bounded archival CPU-image correspondence. This module never opens a runtime
//! path, adopts an owner policy, or creates execution or acceptance authority.
use pulsar_protocol::*;
use std::collections::{BTreeMap, BTreeSet};

type PResult<T> = Result<T, ProtocolError>;
type ImageKey = (String, String, u64);
const MAX_INDEX_BYTES: usize = 128 * 1024 * 1024;

pub(crate) fn require_no_fallback(dependencies: &[ArtifactIdentity]) -> PResult<()> {
    if dependencies.iter().any(|identity| {
        identity
            .artifact_id
            .as_str()
            .starts_with(INFERENCE_FALLBACK_SELECTION_ID_PREFIX)
    }) {
        return Err(ProtocolError::unsupported(
            "portable inference fallback history is not implemented",
        ));
    }
    Ok(())
}

fn has_cpu(dependencies: &[ArtifactIdentity]) -> bool {
    dependencies.iter().any(|identity| {
        identity
            .artifact_id
            .as_str()
            .starts_with(CPU_RUNTIME_IMAGE_ID_PREFIX)
    })
}

pub(crate) fn check_request(
    origin: &PackageGeneratedOrigin,
    request: &WorkerRequest,
) -> PResult<()> {
    require_no_fallback(&request.dependencies)?;
    match (
        origin.cpu_runtime.as_deref(),
        has_cpu(&request.dependencies),
    ) {
        (None, false) => Ok(()),
        (Some(image), true) => crate::cpu_runtime_db::check_image_declaration(request, image)
            .map_err(|_| {
                ProtocolError::invalid(
                    "archived CPU image differs from its original worker manifest",
                )
            }),
        (None, true) => crate::project_package_import_plan::require_portable_cpu_dependencies(
            &request.dependencies,
        ),
        (Some(_), false) => Err(ProtocolError::invalid(
            "archived CPU image membership is missing or unexpected",
        )),
    }
}

fn image_key(identity: &ArtifactIdentity) -> ImageKey {
    (
        identity.artifact_id.to_string(),
        identity.sha256.clone(),
        identity.byte_len,
    )
}

/// Callers already hold the package's admitted metadata memory. The additional
/// index stays below128MiB and never retains image bodies or worker requests.
#[derive(Default)]
pub(crate) struct CpuArchiveIndex {
    images: BTreeSet<ImageKey>,
    candidate_tags: BTreeSet<ImageKey>,
    declared_workers: BTreeSet<(String, u64)>,
    entries: usize,
    bytes: usize,
}
impl CpuArchiveIndex {
    fn charge(&mut self, bytes: usize) -> PResult<()> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .filter(|n| *n <= MAX_INDEX_BYTES)
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "portable CPU correspondence index exceeds metadata admission",
                )
            })?;
        Ok(())
    }
    pub(crate) fn observe_manifest(&mut self, manifest: &PortableProjectManifest) -> PResult<()> {
        self.entries = self
            .entries
            .checked_add(manifest.candidates.len())
            .and_then(|n| n.checked_add(manifest.generated_origins.len()))
            .filter(|n| *n <= MAX_PACKAGE_ORIGIN_ENTRIES)
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "portable CPU origin graph exceeds entry admission",
                )
            })?;
        for origin in &manifest.generated_origins {
            if let Some(image) = &origin.cpu_runtime {
                let identity = image.identity()?;
                if identity.byte_len > 128 * 1024 || origin.cuda_evidence.is_some() {
                    return Err(ProtocolError::invalid("invalid portable CPU image origin"));
                }
                let key = image_key(&identity);
                if !self.images.contains(&key) {
                    self.charge(384)?;
                    self.images.insert(key);
                }
                let worker = (origin.manifest.sha256.clone(), origin.manifest.byte_len);
                if !self.declared_workers.contains(&worker) {
                    self.charge(128)?;
                    self.declared_workers.insert(worker);
                }
            }
        }
        for candidate in &manifest.candidates {
            require_no_fallback(&candidate.lineage)?;
            for identity in candidate.lineage.iter().filter(|identity| {
                identity
                    .artifact_id
                    .as_str()
                    .starts_with(CPU_RUNTIME_IMAGE_ID_PREFIX)
            }) {
                identity.validate()?;
                let key = image_key(identity);
                if !self.candidate_tags.contains(&key) {
                    self.charge(384)?;
                    self.candidate_tags.insert(key);
                }
            }
        }
        Ok(())
    }
    pub(crate) fn check_worker(
        &self,
        reference: &PackageObjectRef,
        request: &WorkerRequest,
    ) -> PResult<()> {
        require_no_fallback(&request.dependencies)?;
        if has_cpu(&request.dependencies)
            && !self
                .declared_workers
                .contains(&(reference.sha256.clone(), reference.byte_len))
        {
            return Err(ProtocolError::unsupported(
                "portable CPU worker lacks its exact declared origin",
            ));
        }
        Ok(())
    }
    pub(crate) fn finish(&self) -> PResult<()> {
        if !self.candidate_tags.is_subset(&self.images) {
            return Err(ProtocolError::unsupported(
                "portable CPU lineage lacks its exact image declaration",
            ));
        }
        Ok(())
    }
}

pub(crate) fn check_graph(
    manifest: &PortableProjectManifest,
    origins: &BTreeMap<String, PortableProjectManifest>,
) -> PResult<()> {
    let mut index = CpuArchiveIndex::default();
    for node in std::iter::once(manifest).chain(origins.values()) {
        index.observe_manifest(node)?;
    }
    index.finish()
}
