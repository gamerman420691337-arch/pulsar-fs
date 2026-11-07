//! Bounded, content-bound originating clocks. No publication or master mutation.
use crate::artifacts;
use pulsar_protocol::*;
use serde::{
    de::{self, SeqAccess, Visitor},
    Deserialize, Deserializer,
};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    io::{self, BufReader, Read, Write},
    marker::PhantomData,
};

const MAX_SAMPLES: usize = 1_000_000;
pub(crate) const MAX_RECEIPT_BYTES: u64 = 64 * 1024 * 1024;

pub(crate) fn bounded<'de, D, T, const N: usize>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Items<T, const N: usize>(PhantomData<T>);
    impl<'de, T: Deserialize<'de>, const N: usize> Visitor<'de> for Items<T, N> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "at most {N} items")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut items = Vec::new();
            while let Some(item) = seq.next_element()? {
                if items.len() == N {
                    return Err(de::Error::custom("receipt array exceeds admission"));
                }
                items.push(item);
            }
            Ok(items)
        }
    }
    deserializer.deserialize_seq(Items::<T, N>(PhantomData))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    placement: SourcePlacement,
    source_origin: SourceTimestamp,
    #[serde(deserialize_with = "bounded::<_, _, MAX_SAMPLES>")]
    samples: Vec<PlacedGenerationSample>,
    #[serde(deserialize_with = "bounded::<_, _, MAX_SAMPLES>")]
    project_order: Vec<usize>,
    project_trim: TimeRange,
    #[serde(deserialize_with = "bounded::<_, _, 2>")]
    uncovered_boundaries: Vec<TimeRange>,
}
// Irrelevant receipt fields (including duplicated analysis arrays) are streamed
// through IgnoredAny by serde, rather than retained as a serde_json::Value.
#[derive(Deserialize)]
struct Receipt {
    schema_version: u32,
    project_id: ProjectId,
    base_revision: RevisionId,
    job_id: JobId,
    attempt_id: AttemptId,
    source_version: SourceVersionId,
    program: ArtifactIdentity,
    configuration_sha256: String,
    configuration: GenerationSettings,
    preset: String,
    source_placement: Option<SourcePlacement>,
    placed_generation_plan: Option<Plan>,
    #[serde(default)]
    recipe: Option<String>,
    #[serde(default)]
    project_time_mapping: Option<String>,
}

struct DigestReader<R> {
    inner: R,
    hash: Sha256,
    bytes: u64,
    limit: u64,
}
impl<R: Read> Read for DigestReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.bytes = self
            .bytes
            .checked_add(n as u64)
            .ok_or_else(|| io::Error::other("receipt byte count overflow"))?;
        if self.bytes > self.limit {
            return Err(io::Error::other("receipt exceeds byte admission"));
        }
        self.hash.update(&buf[..n]);
        Ok(n)
    }
}
fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "originating clock receipt does not match retained candidate/attempt identity",
    )
}
/// Parse only the desired header while hashing every byte of the same file.
pub(crate) fn read_verified<T: for<'de> Deserialize<'de>>(
    path: &std::path::Path,
    identity: &ArtifactIdentity,
) -> Result<T, ProtocolError> {
    identity.validate()?;
    if identity.byte_len == 0
        || identity.byte_len > MAX_RECEIPT_BYTES
        || identity.artifact_id.as_str() != identity.sha256
    {
        return Err(mismatch());
    }
    let file = artifacts::open_regular(path).map_err(|_| mismatch())?;
    if file.metadata().map_err(|_| mismatch())?.len() != identity.byte_len {
        return Err(mismatch());
    }
    let mut reader = DigestReader {
        inner: BufReader::new(file),
        hash: Sha256::new(),
        bytes: 0,
        limit: identity.byte_len,
    };
    let mut decoder = serde_json::Deserializer::from_reader(&mut reader);
    let value = T::deserialize(&mut decoder).map_err(|_| mismatch())?;
    decoder.end().map_err(|_| mismatch())?;
    if reader.bytes != identity.byte_len
        || format!("{:x}", reader.hash.finalize()) != identity.sha256
    {
        return Err(mismatch());
    }
    Ok(value)
}

struct CanonicalDigest {
    hash: Sha256,
    bytes: u64,
    limit: u64,
}
impl Write for CanonicalDigest {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(buf.len() as u64)
            .ok_or_else(|| io::Error::other("motion byte count overflow"))?;
        if self.bytes > self.limit {
            return Err(io::Error::other("canonical motion exceeds admission"));
        }
        self.hash.update(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn verify_program(
    manifest: &WorkerRequest,
    raw: &ArtifactIdentity,
    canonical: &ArtifactIdentity,
    receipt_bytes: u64,
    mut reader: impl Read,
    expected_size: u64,
) -> Result<(), ProtocolError> {
    raw.validate()?;
    let limit = manifest
        .budget
        .output_bytes
        .checked_sub(receipt_bytes)
        .ok_or_else(mismatch)?
        .min(crate::motion_artifacts::MAX_PROGRAM_BYTES);
    if raw.byte_len > limit || raw.artifact_id.as_str() != raw.sha256 {
        return Err(mismatch());
    }
    if expected_size != raw.byte_len {
        return Err(mismatch());
    }
    let mut reader = DigestReader {
        inner: BufReader::new(&mut reader),
        hash: Sha256::new(),
        bytes: 0,
        limit: expected_size,
    };
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).map_err(|_| mismatch())?;
    if reader.bytes != raw.byte_len || format!("{:x}", reader.hash.finalize()) != raw.sha256 {
        return Err(mismatch());
    }
    let program = crate::motion_artifacts::decode_bounded(&bytes)?;
    let mut digest = CanonicalDigest {
        hash: Sha256::new(),
        bytes: 0,
        limit: crate::motion_artifacts::MAX_PROGRAM_BYTES,
    };
    serde_json::to_writer(&mut digest, &program).map_err(|_| mismatch())?;
    if digest.bytes != canonical.byte_len
        || format!("{:x}", digest.hash.finalize()) != canonical.sha256
        || canonical.artifact_id.as_str() != format!("motion.{}", canonical.sha256)
    {
        return Err(mismatch());
    }
    Ok(())
}

/// Caller supplies the engine-owned originating manifest and stored candidate
/// lineage, under Export authority and memory admission. This does not establish
/// current revision derivation; publication must separately check that ancestry.
#[cfg(test)]
pub(crate) fn load(
    manifest: &WorkerRequest,
    lineage: &[ArtifactIdentity],
    program: &ArtifactIdentity,
) -> Result<PlacedGenerationPlan, ProtocolError> {
    Ok(load_verified(manifest, lineage, program)?.plan)
}
pub(crate) struct VerifiedClocks {
    pub(crate) plan: PlacedGenerationPlan,
    pub(crate) identity: ArtifactIdentity,
}
#[cfg(test)]
pub(crate) fn load_verified(
    manifest: &WorkerRequest,
    lineage: &[ArtifactIdentity],
    program: &ArtifactIdentity,
) -> Result<VerifiedClocks, ProtocolError> {
    manifest.validate()?;
    program.validate()?;
    let WorkerOperation::Generate { settings, .. } = &manifest.operation else {
        return Err(mismatch());
    };
    if settings.source_placement.is_none()
        || lineage.len()
            > if settings.annotated_view.is_some() {
                134
            } else {
                129
            }
    {
        return Err(mismatch());
    }
    for identity in lineage {
        identity.validate()?;
    }
    let file = artifacts::open_regular(&manifest.output_dir.join("receipt.json"))
        .map_err(|_| mismatch())?;
    let size = file.metadata().map_err(|_| mismatch())?.len();
    load_verified_readers(manifest, lineage, program, file, size, |raw, _| {
        let file = artifacts::open_regular(&manifest.output_dir.join("program.json"))
            .map_err(|_| mismatch())?;
        if file.metadata().map_err(|_| mismatch())?.len() != raw.byte_len {
            return Err(mismatch());
        }
        Ok(Box::new(file) as Box<dyn Read>)
    })
}

pub(crate) struct VerifiedOrigin {
    pub(crate) plan: Option<PlacedGenerationPlan>,
    pub(crate) identity: ArtifactIdentity,
}

/// Verify the same originating clocks from engine-selected retained objects.
/// The callback must resolve only declared CAS evidence, never manifest paths.
/// Caller owns memory admission for buffers/plan and current lineage authority.
pub(crate) fn load_verified_readers(
    manifest: &WorkerRequest,
    lineage: &[ArtifactIdentity],
    program: &ArtifactIdentity,
    reader: impl Read,
    size: u64,
    original_program: impl FnMut(&ArtifactIdentity, u64) -> Result<Box<dyn Read>, ProtocolError>,
) -> Result<VerifiedClocks, ProtocolError> {
    let origin = load_origin_readers(manifest, lineage, program, reader, size, original_program)?;
    Ok(VerifiedClocks {
        plan: origin.plan.ok_or_else(mismatch)?,
        identity: origin.identity,
    })
}

/// Authenticate a generation receipt without inventing a placement/sample plan.
/// Both branches retain producer/configuration/raw and canonical content checks.
pub(crate) fn load_origin_readers(
    manifest: &WorkerRequest,
    lineage: &[ArtifactIdentity],
    program: &ArtifactIdentity,
    reader: impl Read,
    size: u64,
    mut original_program: impl FnMut(&ArtifactIdentity, u64) -> Result<Box<dyn Read>, ProtocolError>,
) -> Result<VerifiedOrigin, ProtocolError> {
    manifest.validate()?;
    program.validate()?;
    let WorkerOperation::Generate {
        preset,
        settings,
        model,
    } = &manifest.operation
    else {
        return Err(mismatch());
    };
    let placement = settings.source_placement.as_ref();
    if lineage.len()
        > if settings.annotated_view.is_some() {
            134
        } else {
            129
        }
    {
        return Err(mismatch());
    }
    for identity in lineage {
        identity.validate()?;
    }
    let limit = manifest.budget.output_bytes.min(MAX_RECEIPT_BYTES);
    if size == 0 || size > limit {
        return Err(mismatch());
    }
    let mut input = DigestReader {
        inner: BufReader::new(reader),
        hash: Sha256::new(),
        bytes: 0,
        // Size is the caller's observed/admitted byte count. A growing reader
        // must not consume the larger general budget before identity refusal.
        limit: size,
    };
    let mut decoder = serde_json::Deserializer::from_reader(&mut input);
    let receipt = Receipt::deserialize(&mut decoder).map_err(|_| mismatch())?;
    decoder.end().map_err(|_| mismatch())?;
    let hash = format!("{:x}", input.hash.finalize());
    if input.bytes != size
        || lineage
            .iter()
            .filter(|id| {
                id.sha256 == hash && id.byte_len == input.bytes && id.artifact_id.as_str() == hash
            })
            .count()
            != 1
    {
        return Err(mismatch());
    }
    let configuration = serde_json::to_vec(&(preset, settings)).map_err(|_| mismatch())?;
    if receipt.schema_version
        != if settings.annotated_view.is_some() {
            3
        } else if placement.is_some() {
            2
        } else {
            1
        }
        || receipt.project_id != manifest.project_id
        || receipt.base_revision != manifest.base_revision
        || receipt.job_id != manifest.job_id
        || receipt.attempt_id != manifest.attempt_id
        || receipt.source_version != manifest.source.source_version
        || receipt.preset != *preset
        || receipt.source_placement.as_ref() != placement
        || receipt.configuration_sha256 != format!("{:x}", Sha256::digest(&configuration))
        || serde_json::to_vec(&receipt.configuration).map_err(|_| mismatch())?
            != serde_json::to_vec(settings).map_err(|_| mismatch())?
        || receipt
            .placed_generation_plan
            .as_ref()
            .map(|p| &p.placement)
            != placement
    {
        return Err(mismatch());
    }
    let raw_reader = original_program(&receipt.program, input.bytes)?;
    verify_program(
        manifest,
        &receipt.program,
        program,
        input.bytes,
        raw_reader,
        receipt.program.byte_len,
    )?;
    let identity = ArtifactIdentity {
        artifact_id: ArtifactId::new(hash.clone()).map_err(|_| mismatch())?,
        sha256: hash,
        byte_len: input.bytes,
    };
    let Some(p) = receipt.placed_generation_plan else {
        // Raw-frame-pair receipts have a separate clock contract and cannot be
        // authenticated as media generation merely because placement is absent.
        if settings.raw_frame_pair.is_some() {
            return Err(mismatch());
        }
        let expected_recipe = if settings.annotated_view.is_some() {
            ANNOTATED_TEMPORAL_RECIPE
        } else if settings
            .model_input
            .as_ref()
            .is_some_and(|c| c.decoder == DetectorDecoder::TemporalScriptPosition)
        {
            "temporal-reference-script-position-v1"
        } else if model.is_some() {
            "tracked-relative-image-flow-stroke-v2"
        } else {
            "vertical-translation-stroke-v1"
        };
        let recipe_matches = receipt.recipe.as_deref() == Some(expected_recipe)
            || (expected_recipe == "tracked-relative-image-flow-stroke-v2"
                && matches!(
                    receipt.recipe.as_deref(),
                    Some(
                        "tracked-relative-image-flow-stroke-v3"
                            | "tracked-relative-image-flow-stroke-v4"
                    )
                ));
        if !recipe_matches
            || receipt.project_time_mapping.as_deref()
                != Some("source PTS minus first PTS, floor nanoseconds; exact remainder retained")
        {
            return Err(mismatch());
        }
        return Ok(VerifiedOrigin {
            plan: None,
            identity,
        });
    };

    let max =
        MAX_SAMPLES.min(usize::try_from(manifest.budget.memory_bytes / 1024).unwrap_or(usize::MAX));
    if p.samples.is_empty() || p.samples.len() > max || p.project_order.len() != p.samples.len() {
        return Err(mismatch());
    }
    let plan = PlacedGenerationPlan {
        placement: p.placement,
        source_origin: p.source_origin,
        samples: p.samples,
        project_order: p.project_order,
        project_trim: p.project_trim,
        uncovered_boundaries: p.uncovered_boundaries,
    };
    if settings.annotated_view.is_some() {
        validate_annotated_plan(&plan, settings.source_sample_stride, max)?;
    }
    Ok(VerifiedOrigin {
        plan: Some(plan),
        identity,
    })
}

fn validate_annotated_plan(
    plan: &PlacedGenerationPlan,
    stride: u32,
    max: usize,
) -> Result<(), ProtocolError> {
    pulsar_core::SourceClockMapper::new(&plan.placement.spec, Some(plan), max)
        .map_err(|_| mismatch())?;
    let spec = &plan.placement.spec;
    let start = pulsar_core::ExactSeconds::from_source(spec.source_start);
    let end = pulsar_core::ExactSeconds::from_source(spec.source_end);
    for sample in &plan.samples {
        if stride == 0
            || sample.source_frame_index % u64::from(stride) != 0
            || sample
                .elapsed_source_time
                .checked_cmp(start)
                .map_err(|_| mismatch())?
                == std::cmp::Ordering::Less
            || sample
                .elapsed_source_time
                .checked_cmp(end)
                .map_err(|_| mismatch())?
                != std::cmp::Ordering::Less
        {
            return Err(mismatch());
        }
    }
    let a = spec
        .project_at_source_boundary(start)
        .map_err(|_| mismatch())?;
    let b = spec
        .project_at_source_boundary(end)
        .map_err(|_| mismatch())?;
    let (low, high) = if a.checked_cmp(b).map_err(|_| mismatch())? == std::cmp::Ordering::Less {
        (a, b)
    } else {
        (b, a)
    };
    if low
        .checked_cmp(pulsar_core::ExactSeconds::from_project(
            ProjectTime::from_nanos(0),
        ))
        .map_err(|_| mismatch())?
        == std::cmp::Ordering::Less
    {
        return Err(mismatch());
    }
    let low = low.quantize_nanoseconds().map_err(|_| mismatch())?;
    let high = high.quantize_nanoseconds().map_err(|_| mismatch())?;
    let upper = if spec.rate_numerator < 0 || high.remainder_numerator != "0" {
        high.time.checked_add(1).map_err(|_| mismatch())?
    } else {
        high.time
    };
    let trim = TimeRange::new(low.time, upper).map_err(|_| mismatch())?;
    let first = plan.samples[plan.project_order[0]].quantization.time;
    let last = plan.samples[*plan.project_order.last().ok_or_else(mismatch)?]
        .quantization
        .time;
    if plan.project_trim != trim || !trim.contains(first) || !trim.contains(last) {
        return Err(mismatch());
    }
    let mut boundaries = vec![];
    if trim.start() < first {
        boundaries.push(TimeRange::new(trim.start(), first).map_err(|_| mismatch())?);
    }
    let after = last.checked_add(1).map_err(|_| mismatch())?;
    if after < trim.end() {
        boundaries.push(TimeRange::new(after, trim.end()).map_err(|_| mismatch())?);
    }
    if boundaries != plan.uncovered_boundaries {
        return Err(mismatch());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Deserialize)]
    struct Header {
        id: u32,
    }
    #[test]
    fn verified_header_hashes_ignored_arrays_and_refuses_changed_or_trailing_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proof.json");
        let bytes =
            serde_json::to_vec(&serde_json::json!({"id":1,"mappings":vec![0;100_000]})).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(hash.clone()).unwrap(),
            sha256: hash,
            byte_len: bytes.len() as u64,
        };
        assert_eq!(read_verified::<Header>(&path, &identity).unwrap().id, 1);
        let mut changed = bytes.clone();
        let offset = changed.windows(6).position(|b| b == b"\"id\":1").unwrap() + 5;
        changed[offset] = b'2';
        std::fs::write(&path, &changed).unwrap();
        assert!(read_verified::<Header>(&path, &identity).is_err());
        let mut trailing = bytes.clone();
        trailing.extend_from_slice(b" {}");
        std::fs::write(&path, &trailing).unwrap();
        let hash = format!("{:x}", Sha256::digest(&trailing));
        let id = ArtifactIdentity {
            artifact_id: ArtifactId::new(hash.clone()).unwrap(),
            sha256: hash,
            byte_len: trailing.len() as u64,
        };
        assert!(read_verified::<Header>(&path, &id).is_err());
    }
    #[derive(Deserialize)]
    struct Tiny {
        #[serde(deserialize_with = "bounded::<_, _, 2>")]
        items: Vec<usize>,
    }
    #[test]
    fn bounded_arrays_refuse_before_retaining_extra_items() {
        assert_eq!(
            serde_json::from_str::<Tiny>(r#"{"items":[1,2]}"#)
                .unwrap()
                .items,
            vec![1, 2]
        );
        assert!(serde_json::from_str::<Tiny>(r#"{"items":[1,2,3]}"#).is_err());
        assert!(serde_json::from_str::<Tiny>(r#"{"items":[1,-1]}"#).is_err());
    }
    #[test]
    fn same_stream_digest_bounds_all_bytes_including_trailing_data() {
        let bytes = b"{} trailing";
        let mut r = DigestReader {
            inner: &bytes[..],
            hash: Sha256::new(),
            bytes: 0,
            limit: 2,
        };
        assert!(r.read_to_end(&mut Vec::new()).is_err());
        let mut r = DigestReader {
            inner: &bytes[..],
            hash: Sha256::new(),
            bytes: 0,
            limit: 100,
        };
        r.read_to_end(&mut Vec::new()).unwrap();
        assert_eq!(r.bytes, bytes.len() as u64);
        assert_eq!(r.hash.finalize(), Sha256::digest(bytes));
    }
}
