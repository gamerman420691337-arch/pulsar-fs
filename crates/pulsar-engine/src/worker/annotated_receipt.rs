//! Request-bound annotated result admission. No inference or historical path resolution.
use super::*;
type PResult<T> = std::result::Result<T, ProtocolError>;
use serde::{
    de::{self, SeqAccess, Visitor},
    Deserialize,
};
use std::{
    fmt,
    io::{Read, Seek, SeekFrom},
    marker::PhantomData,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Model {
    contract: AnnotatedTemporalModelContract,
    training_intake_sha256: String,
}

// Unknown analysis arrays stream through serde's IgnoredAny. Only the bounded
// metadata used to admit publication is retained here; clocks have their own verifier.
#[derive(Deserialize)]
struct Receipt {
    schema_version: u16,
    recipe: String,
    qualification: String,
    project_id: ProjectId,
    base_revision: RevisionId,
    job_id: JobId,
    attempt_id: AttemptId,
    source_version: SourceVersionId,
    program: ArtifactIdentity,
    configuration_sha256: String,
    configuration: GenerationSettings,
    preset: String,
    #[serde(deserialize_with = "bounded::<_,_,4096>")]
    dependencies: Vec<ArtifactIdentity>,
    source_dimensions: [u32; 2],
    analysis_dimensions: [u32; 2],
    coordinate_projection: TransformId,
    annotated_input: annotated_input::AnnotatedInput,
    annotated_model: Model,
    #[serde(deserialize_with = "bounded::<_,_,1024>")]
    review: Vec<ReviewFlag>,
}
fn bounded<'de, D, T, const N: usize>(d: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Items<T, const N: usize>(PhantomData<T>);
    impl<'de, T: Deserialize<'de>, const N: usize> Visitor<'de> for Items<T, N> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "at most {N} receipt entries")
        }
        fn visit_seq<A: SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut items = Vec::new();
            while let Some(item) = seq.next_element()? {
                if items.len() == N {
                    return Err(de::Error::custom(
                        "annotated receipt array exceeds admission",
                    ));
                }
                items.push(item);
            }
            Ok(items)
        }
    }
    d.deserialize_seq(Items::<T, N>(PhantomData))
}
fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "annotated receipt differs from immutable worker request/result",
    )
}
fn same<T: Serialize>(a: &T, b: &T) -> PResult<bool> {
    Ok(serde_json::to_vec(a).map_err(|_| mismatch())?
        == serde_json::to_vec(b).map_err(|_| mismatch())?)
}
fn validate(
    request: &WorkerRequest,
    program: &ArtifactIdentity,
    lineage: &[ArtifactIdentity],
    review: &[ReviewFlag],
    binding: &crate::pack_onnx::AnnotatedModelBinding,
    receipt: Receipt,
) -> PResult<()> {
    request.validate()?;
    let WorkerOperation::Generate {
        preset,
        settings,
        model: Some(model),
    } = &request.operation
    else {
        return Err(mismatch());
    };
    let view = settings.annotated_view.as_ref().ok_or_else(mismatch)?;
    crate::inference_profiles::resolve_bound_profile(request)
        .map_err(|_| mismatch())?
        .ok_or_else(mismatch)?;
    let expected = annotated_input::plan(
        view,
        &request.source,
        receipt.source_dimensions,
        request.budget.memory_bytes,
    )
    .map_err(|_| mismatch())?;
    let mut dependencies = vec![
        request.source.identity.clone(),
        request.tools.ffmpeg.identity.clone(),
        request.tools.ffprobe.identity.clone(),
        model.identity.clone(),
        request
            .tools
            .onnx_runtime
            .as_ref()
            .ok_or_else(mismatch)?
            .identity
            .clone(),
    ];
    // Match the worker's existing ordered lineage, including repeated extra
    // dependencies in a valid request. Do not normalize historical identities.
    dependencies.extend(
        request
            .dependencies
            .iter()
            .filter(|item| !dependencies.contains(item))
            .cloned()
            .collect::<Vec<_>>(),
    );
    let configuration = serde_json::to_vec(&(preset, settings)).map_err(|_| mismatch())?;
    binding.contract.validate()?;
    program.validate()?;
    validate_reviews(review)?;
    if program.artifact_id.as_str() != program.sha256
        || receipt.schema_version != 3
        || receipt.recipe != ANNOTATED_TEMPORAL_RECIPE
        || receipt.qualification != "unqualified"
        || receipt.project_id != request.project_id
        || receipt.base_revision != request.base_revision
        || receipt.job_id != request.job_id
        || receipt.attempt_id != request.attempt_id
        || receipt.source_version != request.source.source_version
        || receipt.program != *program
        || receipt.preset != *preset
        || !same(&receipt.configuration, settings)?
        || receipt.configuration_sha256 != format!("{:x}", Sha256::digest(configuration))
        || receipt.analysis_dimensions != [64, 64]
        || receipt.coordinate_projection != expected.transform
        || !same(&receipt.annotated_input, &expected)?
        || receipt.annotated_model.contract != binding.contract
        || receipt.annotated_model.training_intake_sha256 != binding.training_intake_sha256
        || receipt.dependencies != dependencies
        || lineage != dependencies
        || !same(&receipt.review, &review.to_vec())?
    {
        return Err(mismatch());
    }
    Ok(())
}

fn hash_model(file: &mut (impl Read + Seek), identity: &ArtifactIdentity) -> PResult<()> {
    identity.validate()?;
    if identity.byte_len == 0 || identity.byte_len > 256 * 1024 * 1024 {
        return Err(mismatch());
    }
    file.seek(SeekFrom::Start(0)).map_err(|_| mismatch())?;
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf).map_err(|_| mismatch())?;
        if n == 0 {
            break;
        }
        count = count.checked_add(n as u64).ok_or_else(mismatch)?;
        if count > identity.byte_len {
            return Err(mismatch());
        }
        hash.update(&buf[..n]);
    }
    if count != identity.byte_len || format!("{:x}", hash.finalize()) != identity.sha256 {
        return Err(mismatch());
    }
    file.seek(SeekFrom::Start(0)).map_err(|_| mismatch())?;
    Ok(())
}

struct ReceiptReader<R> {
    reader: R,
    hash: Sha256,
    bytes: u64,
    limit: u64,
}
impl<R: Read> Read for ReceiptReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let n = self.reader.read(buffer)?;
        self.bytes = self
            .bytes
            .checked_add(n as u64)
            .ok_or_else(|| std::io::Error::other("annotated receipt size overflow"))?;
        if self.bytes > self.limit {
            return Err(std::io::Error::other(
                "annotated receipt exceeds identity bound",
            ));
        }
        self.hash.update(&buffer[..n]);
        Ok(n)
    }
}

/// Caller owns reader provenance and memory admission. Never resolves any path
/// in the immutable request; this also permits retained portable object readers.
pub(crate) fn verify_readers(
    request: &WorkerRequest,
    program: &ArtifactIdentity,
    receipt: &ArtifactIdentity,
    lineage: &[ArtifactIdentity],
    review: &[ReviewFlag],
    reader: impl Read,
    mut model_reader: impl Read + Seek,
) -> PResult<()> {
    let WorkerOperation::Generate {
        settings,
        model: Some(model),
        ..
    } = &request.operation
    else {
        return Err(mismatch());
    };
    let limit = (request.budget.memory_bytes / 16)
        .min(16 * 1024 * 1024)
        .min(
            request
                .budget
                .output_bytes
                .checked_sub(program.byte_len)
                .ok_or_else(mismatch)?,
        );
    receipt.validate()?;
    if receipt.byte_len == 0
        || receipt.byte_len > limit
        || receipt.artifact_id.as_str() != receipt.sha256
    {
        return Err(mismatch());
    }
    let mut input = ReceiptReader {
        reader: std::io::BufReader::new(reader),
        hash: Sha256::new(),
        bytes: 0,
        limit: receipt.byte_len,
    };
    let mut decoder = serde_json::Deserializer::from_reader(&mut input);
    let header = Receipt::deserialize(&mut decoder).map_err(|_| mismatch())?;
    decoder.end().map_err(|_| mismatch())?;
    if input.bytes != receipt.byte_len || format!("{:x}", input.hash.finalize()) != receipt.sha256 {
        return Err(mismatch());
    }
    hash_model(&mut model_reader, &model.identity)?;
    let binding = crate::pack_onnx::validate_annotated(
        &mut model_reader,
        model.identity.byte_len,
        settings.model_input.as_ref().ok_or_else(mismatch)?,
    )
    .map_err(|_| mismatch())?;
    hash_model(&mut model_reader, &model.identity)?;
    validate(request, program, lineage, review, &binding, header)
}

/// Current attempt files only. Archived callers use verify_readers instead.
pub(crate) fn verify_file(
    request: &WorkerRequest,
    program: &ArtifactIdentity,
    receipt: &ArtifactReference,
    lineage: &[ArtifactIdentity],
    review: &[ReviewFlag],
) -> PResult<()> {
    let WorkerOperation::Generate {
        model: Some(model), ..
    } = &request.operation
    else {
        return Err(mismatch());
    };
    verify_readers(
        request,
        program,
        &receipt.identity,
        lineage,
        review,
        crate::artifacts::open_regular(&receipt.path).map_err(|_| mismatch())?,
        crate::artifacts::open_regular(&model.path).map_err(|_| mismatch())?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipt_digest_and_exact_length_refuse_before_model_reading() {
        struct Unread;
        impl Read for Unread {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                panic!("unverified receipt must not reach model")
            }
        }
        impl Seek for Unread {
            fn seek(&mut self, _: SeekFrom) -> std::io::Result<u64> {
                panic!("unverified receipt must not reach model")
            }
        }
        let (request, program, lineage, _, value) = fixture();
        let raw = serde_json::to_vec(&value).unwrap();
        let digest = format!("{:x}", Sha256::digest(&raw));
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(digest.clone()).unwrap(),
            sha256: digest,
            byte_len: raw.len() as u64,
        };
        for changed in ["digest", "short", "long", "extra"] {
            let mut id = identity.clone();
            let mut bytes = raw.clone();
            match changed {
                "digest" => {
                    id.sha256 = "d".repeat(64);
                    id.artifact_id = ArtifactId::new(id.sha256.clone()).unwrap();
                }
                "short" => id.byte_len -= 1,
                "long" => id.byte_len += 1,
                _ => bytes.push(b' '),
            }
            assert!(
                verify_readers(
                    &request,
                    &program,
                    &id,
                    &lineage,
                    &[],
                    std::io::Cursor::new(bytes),
                    Unread
                )
                .is_err(),
                "{changed}"
            );
        }
    }
    #[test]
    fn ordered_extra_dependency_duplicates_are_preserved() {
        let (mut request, program, mut lineage, binding, mut value) = fixture();
        let profile = request.dependencies[0].clone();
        request.dependencies.push(profile.clone());
        lineage.push(profile);
        value["dependencies"] = serde_json::to_value(&lineage).unwrap();
        validate(
            &request,
            &program,
            &lineage,
            &[],
            &binding,
            serde_json::from_value(value).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn receipt_bounds_refuse_before_reading_and_model_digest_is_exact() {
        struct Unread;
        impl Read for Unread {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                panic!("reader must remain untouched")
            }
        }
        impl Seek for Unread {
            fn seek(&mut self, _: SeekFrom) -> std::io::Result<u64> {
                panic!("reader must remain untouched")
            }
        }
        let (request, program, lineage, _, _) = fixture();
        for size in [
            0,
            request.budget.memory_bytes / 16 + 1,
            request.budget.output_bytes,
        ] {
            let mut identity = program.clone();
            identity.byte_len = size;
            assert!(
                verify_readers(&request, &program, &identity, &lineage, &[], Unread, Unread)
                    .is_err()
            );
        }
        let data = b"authored model bytes";
        let digest = format!("{:x}", Sha256::digest(data));
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(digest.clone()).unwrap(),
            sha256: digest,
            byte_len: data.len() as u64,
        };
        hash_model(&mut std::io::Cursor::new(data), &identity).unwrap();
        assert!(hash_model(
            &mut std::io::Cursor::new(&data[..data.len() - 1]),
            &identity
        )
        .is_err());
        let mut extra = data.to_vec();
        extra.push(0);
        assert!(hash_model(&mut std::io::Cursor::new(extra), &identity).is_err());
    }
    fn fixture() -> (
        WorkerRequest,
        ArtifactIdentity,
        Vec<ArtifactIdentity>,
        crate::pack_onnx::AnnotatedModelBinding,
        serde_json::Value,
    ) {
        let request = crate::annotated_generation_db::tests::request();
        let WorkerOperation::Generate {
            settings,
            preset,
            model: Some(model),
        } = &request.operation
        else {
            panic!()
        };
        let input = annotated_input::plan(
            settings.annotated_view.as_ref().unwrap(),
            &request.source,
            [128, 64],
            request.budget.memory_bytes,
        )
        .unwrap();
        let program = ArtifactIdentity {
            artifact_id: ArtifactId::new("b".repeat(64)).unwrap(),
            sha256: "b".repeat(64),
            byte_len: 100,
        };
        let mut lineage = vec![
            request.source.identity.clone(),
            request.tools.ffmpeg.identity.clone(),
            request.tools.ffprobe.identity.clone(),
            model.identity.clone(),
            request
                .tools
                .onnx_runtime
                .as_ref()
                .unwrap()
                .identity
                .clone(),
        ];
        for item in &request.dependencies {
            if !lineage.contains(item) {
                lineage.push(item.clone());
            }
        }
        let binding = crate::pack_onnx::AnnotatedModelBinding {
            contract: AnnotatedTemporalModelContract::expected(),
            training_intake_sha256: "c".repeat(64),
        };
        let value = serde_json::json!({"schema_version":3,"recipe":ANNOTATED_TEMPORAL_RECIPE,"qualification":"unqualified","project_id":request.project_id,"base_revision":request.base_revision,"job_id":request.job_id,"attempt_id":request.attempt_id,"source_version":request.source.source_version,"program":program,"configuration_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&(preset,settings)).unwrap())),"configuration":settings,"preset":preset,"dependencies":lineage,"source_dimensions":[128,64],"analysis_dimensions":[64,64],"coordinate_projection":input.transform,"annotated_input":input,"annotated_model":{"contract":binding.contract,"training_intake_sha256":binding.training_intake_sha256},"review":[],"samples":[{"ignored_analysis":"no inference/clock claim"}]});
        (request, program, lineage, binding, value)
    }
    #[test]
    fn exact_metadata_passes_and_mutations_refuse() {
        let (request, program, lineage, binding, value) = fixture();
        validate(
            &request,
            &program,
            &lineage,
            &[],
            &binding,
            serde_json::from_value(value.clone()).unwrap(),
        )
        .unwrap();
        for pointer in [
            "/schema_version",
            "/recipe",
            "/qualification",
            "/project_id",
            "/base_revision",
            "/job_id",
            "/attempt_id",
            "/source_version",
            "/program/sha256",
            "/configuration_sha256",
            "/configuration/analysis_width",
            "/preset",
            "/dependencies/0/sha256",
            "/source_dimensions/0",
            "/analysis_dimensions/0",
            "/coordinate_projection",
            "/annotated_input/transform",
            "/annotated_input/native_plan/spatial_filter",
            "/annotated_input/native_plan/declaration/target_id",
            "/annotated_model/contract/input_name",
            "/annotated_model/training_intake_sha256",
        ] {
            let mut changed = value.clone();
            let slot = changed.pointer_mut(pointer).unwrap();
            *slot = if slot.is_number() {
                serde_json::json!(7)
            } else {
                serde_json::json!("changed")
            };
            let result = serde_json::from_value(changed)
                .map_err(|_| mismatch())
                .and_then(|r| validate(&request, &program, &lineage, &[], &binding, r));
            assert!(result.is_err(), "{pointer}");
        }
        let mut truncated = lineage.clone();
        truncated.pop();
        assert!(validate(
            &request,
            &program,
            &truncated,
            &[],
            &binding,
            serde_json::from_value(value).unwrap()
        )
        .is_err());
    }
    #[test]
    fn duplicate_fields_extra_plan_fields_and_unbounded_arrays_refuse() {
        let (_, _, _, _, value) = fixture();
        let raw = serde_json::to_string(&value).unwrap();
        let duplicate = raw.replacen('{', "{\"schema_version\":3,", 1);
        assert!(serde_json::from_str::<Receipt>(&duplicate).is_err());
        let mut extra = value.clone();
        extra["annotated_input"]["native_plan"]["extra"] = true.into();
        assert!(serde_json::from_value::<Receipt>(extra).is_err());
        let mut many = value;
        many["dependencies"] = serde_json::json!(vec![many["dependencies"][0].clone(); 4097]);
        assert!(serde_json::from_value::<Receipt>(many).is_err());
    }
}
