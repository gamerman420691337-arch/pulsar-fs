//! Durable expected CPU images, bound to immutable attempt manifests.
//! These declarations prove neither execution nor native closure or qualification.
//! Startup never opens captured files: their lifetime ends after worker retirement.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};

const TABLE: &str = "cpu_runtime_attempts";
const CREATE: &str = "CREATE TABLE cpu_runtime_attempts(attempt TEXT PRIMARY KEY NOT NULL REFERENCES attempts(id),job TEXT NOT NULL REFERENCES jobs(id),manifest_sha256 TEXT NOT NULL,image_sha256 TEXT NOT NULL,image TEXT NOT NULL)";
const MAX_RECORDS: u64 = 4096;
const MAX_IMAGE: usize = 128 * 1024;
const MAX_MANIFEST: usize = 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 128 * 1024 * 1024;

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn runtime_version(db: &Connection) -> Result<i64> {
    crate::cpu_runtime_reader::version(db)?;
    crate::inference_fallback_reader::version(db)
}

fn layout(db: &Connection) -> Result<bool> {
    let raw = runtime_version(db)?;
    let temporary: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM temp.sqlite_master WHERE lower(name) GLOB 'cpu_runtime_*' OR lower(tbl_name) GLOB 'cpu_runtime_*')", [], |row| row.get(0),
    )?;
    ensure!(
        !temporary,
        "temporary CPU runtime storage objects are forbidden"
    );
    let objects: u64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM main.sqlite_master WHERE (lower(name) GLOB 'cpu_runtime_*' OR lower(tbl_name) GLOB 'cpu_runtime_*') AND lower(name)!='cpu_runtime_reader' AND lower(tbl_name)!='cpu_runtime_reader' AND sql IS NOT NULL LIMIT 2)", [], |row| row.get(0),
    )?;
    let exact: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name=?1 AND tbl_name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))=?2 AND sql=?3)", params![TABLE, CREATE.len() as i64, CREATE], |row| row.get(0),
    )?;
    if objects == 0 {
        ensure!(
            raw != 36,
            "CPU runtime reader requires its retained attempt table"
        );
        return Ok(false);
    }
    ensure!(objects == 1 && exact, "CPU runtime storage layout differs");
    ensure!(raw == 36, "CPU runtime declarations require reader 36");
    Ok(true)
}

fn capacity(db: &Connection) -> Result<()> {
    let count: u64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM main.cpu_runtime_attempts LIMIT 4097)",
        [],
        |row| row.get(0),
    )?;
    ensure!(
        count <= MAX_RECORDS,
        "CPU runtime declaration row capacity exceeded"
    );
    let invalid: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM main.cpu_runtime_attempts WHERE typeof(attempt)!='text' OR length(CAST(attempt AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(job)!='text' OR length(CAST(job AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(manifest_sha256)!='text' OR length(CAST(manifest_sha256 AS BLOB))!=64 OR manifest_sha256 GLOB '*[^0-9a-f]*' OR typeof(image_sha256)!='text' OR length(CAST(image_sha256 AS BLOB))!=64 OR image_sha256 GLOB '*[^0-9a-f]*' OR typeof(image)!='text' OR length(CAST(image AS BLOB)) NOT BETWEEN 1 AND 131072)", [], |row| row.get(0),
    )?;
    ensure!(
        !invalid,
        "CPU runtime declaration scalar or byte admission differs"
    );
    let bytes: u64 = db.query_row(
        "SELECT COALESCE(SUM(length(CAST(image AS BLOB))),0) FROM main.cpu_runtime_attempts",
        [],
        |row| row.get(0),
    )?;
    ensure!(
        bytes <= MAX_IMAGE_BYTES,
        "CPU runtime declaration byte capacity exceeded"
    );
    Ok(())
}

fn encoded_request(request: &WorkerRequest) -> Result<String> {
    request.validate()?;
    let raw = serde_json::to_string(request)?;
    ensure!(
        !raw.is_empty() && raw.len() <= MAX_MANIFEST,
        "CPU runtime manifest exceeds byte admission"
    );
    Ok(raw)
}

fn image_tag(request: &WorkerRequest) -> Result<Option<&ArtifactIdentity>> {
    let mut tagged = request.dependencies.iter().filter(|identity| {
        identity
            .artifact_id
            .as_str()
            .starts_with(CPU_RUNTIME_IMAGE_ID_PREFIX)
    });
    let first = tagged.next();
    ensure!(
        tagged.next().is_none(),
        "CPU runtime manifest has multiple image declarations"
    );
    Ok(first)
}

fn exactly_declared(request: &WorkerRequest, identity: &ArtifactIdentity) -> Result<()> {
    let mut declarations = request
        .dependencies
        .iter()
        .filter(|value| value.artifact_id == identity.artifact_id);
    ensure!(
        declarations.next() == Some(identity) && declarations.next().is_none(),
        "CPU runtime dependency identity is missing, duplicated or changed"
    );
    Ok(())
}

fn bind_image(request: &WorkerRequest, image: &CpuRuntimeImage) -> Result<String> {
    request.validate()?;
    image.validate()?;
    let identity = image.identity()?;
    ensure!(
        image_tag(request)? == Some(&identity),
        "CPU runtime image tag differs from its declaration"
    );
    ensure!(
        request.cuda_execution.is_none(),
        "CPU runtime declaration cannot accompany CUDA execution"
    );
    let (profile, model, input) = match &request.operation {
        WorkerOperation::Generate {
            settings, model, ..
        } => (
            settings.inference_profile.as_ref(),
            model.as_ref(),
            settings.model_input.as_ref(),
        ),
        WorkerOperation::Preview {
            inference_profile,
            model,
            model_input,
            ..
        } => (
            inference_profile.as_deref(),
            model.as_ref(),
            model_input.as_ref(),
        ),
        WorkerOperation::GenerateInput { .. } | WorkerOperation::ProbeMedia => (None, None, None),
    };
    let profile =
        profile.context("CPU runtime declaration requires an explicit inference profile")?;
    profile.validate()?;
    profile.admit(&request.budget)?;
    ensure!(
        profile.provider == InferenceProvider::Cpu,
        "CPU runtime declaration requires the CPU provider"
    );
    ensure!(
        model.is_some_and(|value| value.identity == profile.model),
        "CPU runtime model differs from the declared profile"
    );
    ensure!(
        input.is_some_and(|value| serde_json::to_vec(value).ok()
            == serde_json::to_vec(&profile.preprocessing.input).ok()),
        "CPU runtime input differs from the declared profile"
    );
    let runtime = request
        .tools
        .onnx_runtime
        .as_ref()
        .context("CPU runtime artifact is missing")?;
    ensure!(
        runtime.identity == profile.runtime
            && runtime.path.to_str() == Some("/pulsar-cpu-libraries/libonnxruntime.so.1"),
        "CPU runtime artifact or fixed guest path differs from the declared profile"
    );
    let captured = image
        .libraries
        .iter()
        .find(|library| {
            library
                .names
                .iter()
                .any(|name| name == "libonnxruntime.so.1")
        })
        .context("CPU runtime image lacks its runtime alias")?;
    ensure!(
        captured.identity == profile.runtime,
        "CPU runtime image differs from the declared profile runtime"
    );
    exactly_declared(request, &identity)?;
    exactly_declared(request, &image.owner_policy)?;
    exactly_declared(request, &profile.model)?;
    for library in &image.libraries {
        exactly_declared(request, &library.identity)?;
    }
    let raw = serde_json::to_string(image)?;
    ensure!(
        raw.len() <= MAX_IMAGE
            && raw.len() as u64 == identity.byte_len
            && digest(raw.as_bytes()) == identity.sha256,
        "CPU runtime image canonical byte admission differs"
    );
    Ok(raw)
}

/// Pure historical correspondence check. It opens no files and confers no
/// current owner-policy, execution, acceptance or scheduling authority.
pub(crate) fn check_image_declaration(
    request: &WorkerRequest,
    image: &CpuRuntimeImage,
) -> Result<()> {
    bind_image(request, image).map(|_| ())
}

struct Membership {
    manifest: String,
    current: String,
    project: String,
    revision: i64,
    source: String,
    count: i64,
    current_manifest: String,
}

/// Check exact attempt membership and the current job pointer even for a prior attempt.
fn membership(db: &Connection, request: &WorkerRequest) -> Result<String> {
    let expected = encoded_request(request)?;
    let row: Option<Membership> = db.query_row(
        "SELECT CASE WHEN typeof(a.manifest)='text' AND length(CAST(a.manifest AS BLOB)) BETWEEN 1 AND 1048576 THEN a.manifest ELSE NULL END,CASE WHEN typeof(j.attempt)='text' AND length(CAST(j.attempt AS BLOB)) BETWEEN 1 AND 128 THEN j.attempt ELSE NULL END,CASE WHEN typeof(j.project)='text' AND length(CAST(j.project AS BLOB)) BETWEEN 1 AND 128 THEN j.project ELSE NULL END,CASE WHEN typeof(j.base_revision)='integer' AND j.base_revision>=0 THEN j.base_revision ELSE NULL END,CASE WHEN typeof(j.source)='text' AND length(CAST(j.source AS BLOB)) BETWEEN 1 AND 128 THEN j.source ELSE NULL END,CASE WHEN typeof(j.attempts)='integer' AND j.attempts BETWEEN 1 AND 128 THEN j.attempts ELSE NULL END,CASE WHEN typeof(j.manifest)='text' AND length(CAST(j.manifest AS BLOB)) BETWEEN 1 AND 1048576 THEN j.manifest ELSE NULL END FROM main.attempts a JOIN main.jobs j ON j.id=a.job WHERE a.id=?1 AND a.job=?2", params![request.attempt_id.as_str(),request.job_id.as_str()], |row| Ok(Membership {manifest:row.get(0)?,current:row.get(1)?,project:row.get(2)?,revision:row.get(3)?,source:row.get(4)?,count:row.get(5)?,current_manifest:row.get(6)?}),
    ).optional()?;
    let Membership {
        manifest: raw,
        current,
        project,
        revision,
        source,
        count: declared_count,
        current_manifest: current_raw,
    } = row.context("CPU runtime declaration has no retained attempt membership")?;
    ensure!(
        raw == expected
            && project == request.project_id.as_str()
            && u64::try_from(revision)? == request.base_revision.value()
            && source == request.source.source_version.as_str(),
        "CPU runtime attempt immutable manifest identity differs"
    );
    let count: i64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM main.attempts WHERE job=?1 LIMIT 129)",
        [request.job_id.as_str()],
        |row| row.get(0),
    )?;
    ensure!(
        count == declared_count && count <= 128,
        "CPU runtime job attempt counter differs from retained history"
    );
    let current_bound: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM main.attempts WHERE id=?1 AND job=?2 AND typeof(manifest)='text' AND manifest=?3)", params![current,request.job_id.as_str(),current_raw], |row| row.get(0))?;
    ensure!(
        current_bound,
        "CPU runtime current job attempt manifest membership differs"
    );
    let current_request: WorkerRequest = serde_json::from_str(&current_raw)?;
    ensure!(
        encoded_request(&current_request)? == current_raw
            && current_request.attempt_id.as_str() == current
            && current_request.job_id == request.job_id
            && current_request.project_id == request.project_id
            && current_request.base_revision == request.base_revision
            && current_request.source.source_version == request.source.source_version,
        "CPU runtime current job manifest context differs"
    );
    Ok(raw)
}

struct Stored {
    job: String,
    manifest_sha256: String,
    image_sha256: String,
    image: String,
}
fn row(db: &Connection, attempt: &AttemptId) -> Result<Option<Stored>> {
    Ok(db.query_row(
        "SELECT CASE WHEN typeof(job)='text' AND length(CAST(job AS BLOB)) BETWEEN 1 AND 128 THEN job ELSE NULL END,CASE WHEN typeof(manifest_sha256)='text' AND length(CAST(manifest_sha256 AS BLOB))=64 AND manifest_sha256 NOT GLOB '*[^0-9a-f]*' THEN manifest_sha256 ELSE NULL END,CASE WHEN typeof(image_sha256)='text' AND length(CAST(image_sha256 AS BLOB))=64 AND image_sha256 NOT GLOB '*[^0-9a-f]*' THEN image_sha256 ELSE NULL END,CASE WHEN typeof(image)='text' AND length(CAST(image AS BLOB)) BETWEEN 1 AND 131072 THEN image ELSE NULL END FROM main.cpu_runtime_attempts WHERE attempt=?1", [attempt.as_str()], |row| Ok(Stored { job:row.get(0)?,manifest_sha256:row.get(1)?,image_sha256:row.get(2)?,image:row.get(3)? }),
    ).optional()?)
}
fn retained_inner(
    db: &Connection,
    request: &WorkerRequest,
    present: bool,
) -> Result<Option<CpuRuntimeImage>> {
    let raw = membership(db, request)?;
    let tag = image_tag(request)?;
    let stored = if present {
        row(db, &request.attempt_id)?
    } else {
        None
    };
    match (tag, stored) {
        (None, None) => Ok(None),
        (Some(_), None) => anyhow::bail!("CPU runtime image tag has no retained declaration"),
        (None, Some(_)) => anyhow::bail!("CPU runtime declaration lost its manifest image tag"),
        (Some(tag), Some(stored)) => {
            ensure!(
                stored.job == request.job_id.as_str()
                    && stored.manifest_sha256 == digest(raw.as_bytes())
                    && stored.image_sha256 == tag.sha256,
                "CPU runtime indexed declaration identity differs"
            );
            let image: CpuRuntimeImage = serde_json::from_str(&stored.image)?;
            ensure!(
                bind_image(request, &image)? == stored.image
                    && image.identity()?.sha256 == stored.image_sha256,
                "CPU runtime retained image bytes differ"
            );
            Ok(Some(image))
        }
    }
}

/// Read-only lookup. Legacy attempts are None only if no contradictory image row exists.
pub(crate) fn retained(
    db: &Connection,
    request: &WorkerRequest,
) -> Result<Option<CpuRuntimeImage>> {
    let present = layout(db)?;
    if present {
        capacity(db)?;
    }
    retained_inner(db, request, present)
}

fn history_tables(db: &Connection) -> Result<bool> {
    let mut present = 0;
    for name in ["jobs", "attempts"] {
        let count: i64 = db.query_row("SELECT COUNT(*) FROM (SELECT 1 FROM main.sqlite_master WHERE name=?1 COLLATE NOCASE LIMIT 2)", [name], |row| row.get(0))?;
        if count != 0 {
            let table: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE name=?1 AND type='table')",
                [name],
                |row| row.get(0),
            )?;
            ensure!(
                count == 1 && table,
                "CPU runtime history table identity differs"
            );
            present += 1;
        }
    }
    ensure!(
        present == 0 || present == 2,
        "CPU runtime history membership tables are incomplete"
    );
    Ok(present == 2)
}

/// Scan one bounded JSON document at a time. json_tree also finds escaped IDs and
/// duplicate-key representations; typed canonical decoding then refuses ambiguity.
fn scan_tagged(db: &Connection, present: bool, jobs: bool) -> Result<()> {
    let table = if jobs { "jobs" } else { "attempts" };
    let invalid: bool = db.query_row(&format!("SELECT EXISTS(SELECT 1 FROM main.{table} WHERE CASE WHEN typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND 1048576 THEN json_valid(manifest)=0 ELSE 1 END)"), [], |row| row.get(0))?;
    ensure!(
        !invalid,
        "CPU runtime history manifest byte or JSON admission differs"
    );
    let member = if jobs { "attempt" } else { "job" };
    let sql = format!("SELECT CASE WHEN typeof(id)='text' AND length(CAST(id AS BLOB)) BETWEEN 1 AND 128 THEN id ELSE NULL END,CASE WHEN typeof({member})='text' AND length(CAST({member} AS BLOB)) BETWEEN 1 AND 128 THEN {member} ELSE NULL END,CASE WHEN typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND 1048576 THEN manifest ELSE NULL END FROM main.{table} WHERE EXISTS(SELECT 1 FROM json_tree(CASE WHEN typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND 1048576 THEN CASE WHEN json_valid(manifest) THEN manifest ELSE 'null' END ELSE 'null' END) jt WHERE jt.key='artifact_id' AND jt.type='text' AND jt.value GLOB 'cpu-runtime-image-*')");
    let mut statement = db.prepare(&sql)?;
    let mut rows = statement.query([])?;
    let mut count = 0u64;
    let mut bytes = 0u64;
    while let Some(row) = rows.next()? {
        count += 1;
        ensure!(
            count <= MAX_RECORDS,
            "CPU runtime tagged history row capacity exceeded"
        );
        let id: String = row.get(0)?;
        let member: String = row.get(1)?;
        let raw: String = row.get(2)?;
        bytes = bytes
            .checked_add(raw.len() as u64)
            .context("CPU runtime history byte overflow")?;
        ensure!(
            bytes <= MAX_MANIFEST_BYTES,
            "CPU runtime tagged history byte capacity exceeded"
        );
        ensure!(
            present,
            "CPU runtime tagged history requires its retained declaration table"
        );
        let request: WorkerRequest = serde_json::from_str(&raw)?;
        ensure!(
            (if jobs {
                request.job_id.as_str() == id && request.attempt_id.as_str() == member
            } else {
                request.attempt_id.as_str() == id && request.job_id.as_str() == member
            }) && encoded_request(&request)? == raw,
            "CPU runtime tagged manifest identity is not canonical"
        );
        ensure!(
            retained_inner(db, &request, present)?.is_some(),
            "CPU runtime tagged history is missing its dependency declaration"
        );
    }
    Ok(())
}

/// Startup preflight only. Does not create tables, publish headers, open files or
/// infer whether an attempt executed. Missing portable declarations fail closed.
pub(crate) fn validate(db: &Connection) -> Result<()> {
    let present = layout(db)?;
    let history = history_tables(db)?;
    ensure!(
        !present || history,
        "CPU runtime declarations have no job history"
    );
    if present {
        capacity(db)?;
        let orphan: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM main.cpu_runtime_attempts c LEFT JOIN main.attempts a ON a.id=c.attempt AND a.job=c.job LEFT JOIN main.jobs j ON j.id=c.job WHERE a.id IS NULL OR j.id IS NULL)", [], |row| row.get(0))?;
        ensure!(!orphan, "CPU runtime declaration membership differs");
        let mut statement = db.prepare("SELECT c.attempt,c.job,CASE WHEN typeof(a.manifest)='text' AND length(CAST(a.manifest AS BLOB)) BETWEEN 1 AND 1048576 THEN a.manifest ELSE NULL END FROM main.cpu_runtime_attempts c JOIN main.attempts a ON a.id=c.attempt AND a.job=c.job")?;
        let mut rows = statement.query([])?;
        let mut bytes = 0u64;
        while let Some(row) = rows.next()? {
            let attempt: String = row.get(0)?;
            let job: String = row.get(1)?;
            let raw: String = row.get(2)?;
            bytes = bytes
                .checked_add(raw.len() as u64)
                .context("CPU runtime manifest byte overflow")?;
            ensure!(
                bytes <= MAX_MANIFEST_BYTES,
                "CPU runtime retained manifest byte capacity exceeded"
            );
            let request: WorkerRequest = serde_json::from_str(&raw)?;
            ensure!(
                request.attempt_id.as_str() == attempt
                    && request.job_id.as_str() == job
                    && encoded_request(&request)? == raw,
                "CPU runtime retained manifest identity is not canonical"
            );
            ensure!(
                retained_inner(db, &request, true)?.is_some(),
                "CPU runtime retained declaration lost its image"
            );
        }
    }
    if history {
        scan_tagged(db, present, false)?;
        scan_tagged(db, present, true)?;
    }
    Ok(())
}

/// Declare after inserting jobs/attempts, in their same transaction. An internal
/// savepoint removes every local change on refusal, even if the caller continues.
pub(crate) fn declare(
    tx: &Transaction<'_>,
    request: &WorkerRequest,
    image: &CpuRuntimeImage,
) -> Result<()> {
    tx.execute_batch("SAVEPOINT pulsar_cpu_runtime_declaration")?;
    let result = declare_inner(tx, request, image);
    match result {
        Ok(()) => {
            tx.execute_batch("RELEASE pulsar_cpu_runtime_declaration")?;
            Ok(())
        }
        Err(error) => {
            tx.execute_batch("ROLLBACK TO pulsar_cpu_runtime_declaration; RELEASE pulsar_cpu_runtime_declaration").context("CPU runtime declaration rollback failed")?;
            Err(error)
        }
    }
}
fn declare_inner(
    tx: &Transaction<'_>,
    request: &WorkerRequest,
    image: &CpuRuntimeImage,
) -> Result<()> {
    let image_raw = bind_image(request, image)?;
    let manifest = membership(tx, request)?;
    let present = layout(tx)?;
    if present {
        capacity(tx)?;
        if let Some(existing) = row(tx, &request.attempt_id)? {
            ensure!(
                existing.job == request.job_id.as_str()
                    && existing.manifest_sha256 == digest(manifest.as_bytes())
                    && existing.image_sha256 == image.identity()?.sha256
                    && existing.image == image_raw,
                "CPU runtime attempt declaration is immutable"
            );
            return validate(tx);
        }
    } else {
        tx.execute_batch(CREATE)?;
    }
    tx.execute("INSERT INTO main.cpu_runtime_attempts(attempt,job,manifest_sha256,image_sha256,image) VALUES(?1,?2,?3,?4,?5)", params![request.attempt_id.as_str(),request.job_id.as_str(),digest(manifest.as_bytes()),image.identity()?.sha256,image_raw])?;
    crate::cpu_runtime_reader::arm(tx)?;
    validate(tx)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn identity(index: u8) -> ArtifactIdentity {
        let sha256 = format!("{index:064x}");
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: 1,
        }
    }
    fn image() -> CpuRuntimeImage {
        CpuRuntimeImage {
            schema_version: 1,
            owner_policy: identity(9),
            libraries: ["ld-linux-x86-64.so.2", "libc.so.6", "libonnxruntime.so.1"]
                .iter()
                .enumerate()
                .map(|(index, name)| CapturedNativeLibrary {
                    identity: identity(index as u8 + 1),
                    names: vec![(*name).into()],
                })
                .collect(),
        }
    }
    #[test]
    fn portable_cpu_declaration_is_exact_and_inert_across_attempt_states() {
        let closed = request("portable-original", true);
        let mut origin = PackageGeneratedOrigin {
            fallback_history: None,
            cpu_runtime: Some(Box::new(image())),
            image_origin: None,
            annotated_model: None,
            cuda_evidence: None,
            job_id: closed.job_id.clone(),
            attempt_id: closed.attempt_id.clone(),
            base_revision: closed.base_revision,
            source_version: closed.source.source_version.clone(),
            manifest: PackageObjectRef {
                sha256: digest(&serde_json::to_vec(&closed).unwrap()),
                byte_len: serde_json::to_vec(&closed).unwrap().len() as u64,
            },
            receipt: None,
            program: None,
            state: "failed".into(),
        };
        for state in [
            "queued",
            "running",
            "interrupted",
            "completed",
            "failed",
            "cancelled",
        ] {
            origin.state = state.into();
            crate::portable_cpu_archives::check_request(&origin, &closed).unwrap();
        }
        let original = origin.cpu_runtime.take().unwrap();
        assert!(crate::portable_cpu_archives::check_request(&origin, &closed).is_err());
        origin.cpu_runtime = Some(original.clone());
        let ordinary = request("ordinary-portable", false);
        assert!(crate::portable_cpu_archives::check_request(&origin, &ordinary).is_err());
        origin.cpu_runtime.as_mut().unwrap().owner_policy = identity(10);
        assert!(crate::portable_cpu_archives::check_request(&origin, &closed).is_err());
        origin.cpu_runtime = Some(original);
        let mut missing_library = closed.clone();
        missing_library
            .dependencies
            .retain(|value| value != &identity(1));
        assert!(crate::portable_cpu_archives::check_request(&origin, &missing_library).is_err());
    }
    fn request(attempt: &str, closed: bool) -> WorkerRequest {
        let tool = ArtifactReference {
            identity: identity(6),
            path: "/nonexistent/metadata-only/tool".into(),
        };
        let model = SourceArtifact {
            identity: identity(8),
            source_version: SourceVersionId::new("model").unwrap(),
            path: "/nonexistent/metadata-only/model".into(),
        };
        let input = ModelInputContract {
            width: 64,
            height: 64,
            channels: 3,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: DetectorDecoder::YoloChannelMajor,
        };
        let profile = DeclaredInferenceProfile {
            schema_version: 1,
            model: model.identity.clone(),
            runtime: identity(3),
            preprocessing: InferencePreprocessing {
                input: input.clone(),
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 1024,
            minimum_output_bytes: 1024,
            cpu_threads: 1,
            cuda_resources: None,
        };
        let settings = GenerationSettings {
            model_input: Some(input),
            inference_profile: Some(profile),
            ..Default::default()
        };
        let image = image();
        let mut dependencies = vec![identity(6), identity(7), identity(8), identity(3)];
        if closed {
            dependencies.extend([
                image.owner_policy.clone(),
                identity(1),
                identity(2),
                image.identity().unwrap(),
            ]);
        }
        let request = WorkerRequest {
            cuda_execution: None,
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new(attempt).unwrap(),
            job_id: JobId::new("job").unwrap(),
            project_id: ProjectId::new("project").unwrap(),
            base_revision: RevisionId::new(0),
            source: SourceArtifact {
                source_version: SourceVersionId::new("source").unwrap(),
                path: "/nonexistent/metadata-only/source".into(),
                identity: identity(7),
            },
            independent_audio: None,
            output_dir: format!("/nonexistent/metadata-only/{attempt}").into(),
            operation: WorkerOperation::Generate {
                preset: "default".into(),
                settings: Box::new(settings),
                model: Some(model),
            },
            budget: ResourceBudget {
                memory_bytes: 64 << 20,
                output_bytes: 16 << 20,
                wall_time_ms: 1000,
                cpu_threads: 1,
            },
            dependencies,
            tools: WorkerTools {
                ffmpeg: tool.clone(),
                ffprobe: tool,
                onnx_runtime: Some(ArtifactReference {
                    identity: identity(3),
                    path: "/pulsar-cpu-libraries/libonnxruntime.so.1".into(),
                }),
            },
        };
        request.validate().unwrap();
        request
    }
    fn database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON;PRAGMA user_version=20;CREATE TABLE jobs(id TEXT PRIMARY KEY,attempt TEXT NOT NULL,project TEXT NOT NULL,base_revision INTEGER NOT NULL,source TEXT NOT NULL,manifest TEXT NOT NULL,state TEXT NOT NULL,attempts INTEGER NOT NULL);CREATE TABLE attempts(id TEXT PRIMARY KEY,job TEXT NOT NULL REFERENCES jobs(id),manifest TEXT NOT NULL,state TEXT NOT NULL);").unwrap();
        db
    }
    fn seed(db: &Connection, request: &WorkerRequest) {
        let raw = serde_json::to_string(request).unwrap();
        db.execute("INSERT INTO jobs VALUES(?1,?2,?3,?4,?5,?6,'queued',1) ON CONFLICT(id) DO UPDATE SET attempt=excluded.attempt,manifest=excluded.manifest,state=excluded.state,attempts=jobs.attempts+1", params![request.job_id.as_str(),request.attempt_id.as_str(),request.project_id.as_str(),request.base_revision.value(),request.source.source_version.as_str(),raw]).unwrap();
        db.execute(
            "INSERT INTO attempts VALUES(?1,?2,?3,'queued')",
            params![request.attempt_id.as_str(), request.job_id.as_str(), raw],
        )
        .unwrap();
    }
    fn declared() -> (Connection, WorkerRequest) {
        let mut db = database();
        let request = request("attempt-1", true);
        let tx = db.transaction().unwrap();
        seed(&tx, &request);
        declare(&tx, &request, &image()).unwrap();
        tx.commit().unwrap();
        (db, request)
    }

    #[test]
    fn fallback_outer_reader_preserves_actual_cpu_declarations_in_both_orders() {
        for fallback_first in [false, true] {
            let mut db = database();
            let request = request("closed-with-fallback-reader", true);
            let tx = db.transaction().unwrap();
            seed(&tx, &request);
            if fallback_first {
                crate::inference_fallback_reader::arm(&tx).unwrap();
            }
            declare(&tx, &request, &image()).unwrap();
            if !fallback_first {
                crate::inference_fallback_reader::arm(&tx).unwrap();
            }
            tx.commit().unwrap();
            validate(&db).unwrap();
            assert_eq!(runtime_version(&db).unwrap(), 36);
            assert_eq!(
                db.query_row("PRAGMA main.user_version", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                37
            );
            assert_eq!(
                retained(&db, &request)
                    .unwrap()
                    .unwrap()
                    .identity()
                    .unwrap(),
                image().identity().unwrap()
            );
            db.execute_batch("DROP TABLE main.cpu_runtime_attempts")
                .unwrap();
            assert!(validate(&db).is_err());
            assert!(retained(&db, &request).is_err());
            assert_eq!(
                db.query_row("PRAGMA main.user_version", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                37
            );
        }
    }

    #[test]
    fn fallback_reader_alone_does_not_invent_cpu_runtime_storage() {
        let mut db = database();
        let request = request("ordinary-with-fallback-reader", false);
        let tx = db.transaction().unwrap();
        seed(&tx, &request);
        crate::inference_fallback_reader::arm(&tx).unwrap();
        tx.commit().unwrap();
        validate(&db).unwrap();
        assert_eq!(runtime_version(&db).unwrap(), 20);
        assert!(retained(&db, &request).unwrap().is_none());
        assert!(no_cpu_table(&db));
        assert_eq!(
            db.query_row("PRAGMA main.user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            37
        );
    }
    fn replace_manifest(db: &Connection, request: &WorkerRequest, raw: &str) {
        db.execute(
            "UPDATE main.attempts SET manifest=?1 WHERE id=?2",
            params![raw, request.attempt_id.as_str()],
        )
        .unwrap();
        db.execute(
            "UPDATE main.jobs SET manifest=?1 WHERE id=?2 AND attempt=?3",
            params![raw, request.job_id.as_str(), request.attempt_id.as_str()],
        )
        .unwrap();
    }
    fn no_cpu_table(db: &Connection) -> bool {
        !db.query_row(
            "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE name='cpu_runtime_attempts')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .unwrap()
    }
    fn assert_refuses(db: &Connection, request: &WorkerRequest) {
        assert!(validate(db).is_err());
        assert!(retained(db, request).is_err());
    }

    #[test]
    fn legacy_history_remains_unknown_without_creating_a_reader_or_table() {
        let db = database();
        let request = request("legacy", false);
        seed(&db, &request);
        db.execute_batch("PRAGMA query_only=ON").unwrap();
        validate(&db).unwrap();
        assert!(retained(&db, &request).unwrap().is_none());
        assert!(no_cpu_table(&db));
        assert_eq!(runtime_version(&db).unwrap(), 20);
    }

    #[test]
    fn declaration_roundtrips_exact_bytes_and_preserves_profile_without_reading_files() {
        let (mut db, request) = declared();
        assert_eq!(runtime_version(&db).unwrap(), 36);
        let original = encoded_request(&request).unwrap();
        let saved = retained(&db, &request).unwrap().unwrap();
        assert_eq!(saved.identity().unwrap(), image().identity().unwrap());
        let stored = row(&db, &request.attempt_id).unwrap().unwrap();
        assert_eq!(stored.image, serde_json::to_string(&image()).unwrap());
        assert_eq!(stored.manifest_sha256, digest(original.as_bytes()));
        let tx = db.transaction().unwrap();
        declare(&tx, &request, &image()).unwrap();
        tx.commit().unwrap();
        assert_eq!(membership(&db, &request).unwrap(), original);
        db.execute_batch("PRAGMA query_only=ON").unwrap();
        validate(&db).unwrap();
    }

    #[test]
    fn old_attempt_image_remains_bound_after_a_new_current_attempt() {
        let (mut db, old) = declared();
        let next = request("attempt-2", true);
        let tx = db.transaction().unwrap();
        seed(&tx, &next);
        declare(&tx, &next, &image()).unwrap();
        tx.commit().unwrap();
        validate(&db).unwrap();
        assert!(retained(&db, &old).unwrap().is_some());
        assert!(retained(&db, &next).unwrap().is_some());
        db.execute(
            "UPDATE jobs SET manifest=?1",
            [serde_json::to_string(&old).unwrap()],
        )
        .unwrap();
        assert_refuses(&db, &old);
    }

    #[test]
    fn outer_rollback_restores_history_and_reader_together() {
        let mut db = database();
        {
            let tx = db.transaction().unwrap();
            let request = request("rolled-back", true);
            seed(&tx, &request);
            declare(&tx, &request, &image()).unwrap();
            assert_eq!(runtime_version(&tx).unwrap(), 36);
        }
        assert_eq!(runtime_version(&db).unwrap(), 20);
        assert!(no_cpu_table(&db));
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM jobs", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
        validate(&db).unwrap();
    }

    #[test]
    fn declaration_refusal_rolls_back_its_own_table_and_header_before_caller_commit() {
        let mut db = database();
        let first = request("first", true);
        let mut missing = request("missing", true);
        missing.job_id = JobId::new("other-job").unwrap();
        let tx = db.transaction().unwrap();
        seed(&tx, &first);
        seed(&tx, &missing);
        assert!(declare(&tx, &first, &image()).is_err());
        assert!(no_cpu_table(&tx));
        assert_eq!(runtime_version(&tx).unwrap(), 20);
        tx.commit().unwrap();
        assert!(no_cpu_table(&db));
        assert!(validate(&db).is_err());
    }

    #[test]
    fn image_record_hashes_canonical_bytes_and_membership_cannot_be_changed() {
        for update in [
            "UPDATE cpu_runtime_attempts SET manifest_sha256=printf('%064d',0)",
            "UPDATE cpu_runtime_attempts SET image_sha256=printf('%064d',0)",
            "UPDATE cpu_runtime_attempts SET image=' '||image",
            "UPDATE cpu_runtime_attempts SET image=json_set(image,'$.qualified',1)",
            "UPDATE cpu_runtime_attempts SET image=zeroblob(131073)",
            "UPDATE jobs SET base_revision=1",
            "UPDATE jobs SET attempts=2",
            "UPDATE jobs SET attempt='missing-attempt'",
            "DELETE FROM cpu_runtime_attempts",
        ] {
            let (db, request) = declared();
            db.execute_batch(update).unwrap();
            assert_refuses(&db, &request);
        }
    }

    #[test]
    fn missing_tables_or_removed_reader_cannot_turn_closed_history_into_legacy() {
        for update in [
            "DROP TABLE cpu_runtime_attempts",
            "DROP TABLE cpu_runtime_reader; PRAGMA user_version=20",
            "DROP TABLE cpu_runtime_attempts; DROP TABLE cpu_runtime_reader; PRAGMA user_version=20",
            "PRAGMA user_version=20",
        ] {
            let (db, request) = declared();
            db.execute_batch(update).unwrap();
            assert_refuses(&db, &request);
        }
        let db = database();
        db.execute_batch("PRAGMA user_version=36").unwrap();
        assert!(validate(&db).is_err());
    }

    #[test]
    fn undeclared_tagged_portable_history_and_escaped_or_duplicate_keys_refuse() {
        for representation in 0..3 {
            let db = database();
            let request = request("portable-without-declaration", true);
            seed(&db, &request);
            let raw = serde_json::to_string(&request).unwrap();
            let raw = match representation {
                1 => raw.replace("cpu-runtime-image-", "cpu\\u002druntime\\u002dimage\\u002d"),
                2 => raw.replacen('{', "{\"dependencies\":[],", 1),
                _ => raw,
            };
            replace_manifest(&db, &request, &raw);
            assert!(validate(&db).is_err());
            assert!(retained(&db, &request).is_err());
            assert!(no_cpu_table(&db));
        }
    }

    #[test]
    fn retained_row_refuses_removed_or_duplicated_dependency_tags() {
        for duplicate in [false, true] {
            let (db, mut request) = declared();
            if duplicate {
                request.dependencies.push(image().identity().unwrap());
            } else {
                request.dependencies.retain(|value| {
                    !value
                        .artifact_id
                        .as_str()
                        .starts_with(CPU_RUNTIME_IMAGE_ID_PREFIX)
                });
            }
            replace_manifest(&db, &request, &serde_json::to_string(&request).unwrap());
            assert_refuses(&db, &request);
        }
    }

    #[test]
    fn declaration_requires_exact_image_dependencies_and_fixed_cpu_runtime_path() {
        for mutate in [
            |request: &mut WorkerRequest| {
                request.tools.onnx_runtime.as_mut().unwrap().path =
                    "/host/capture/libonnxruntime.so.1".into();
            },
            |request: &mut WorkerRequest| {
                request
                    .dependencies
                    .retain(|value| value != &image().owner_policy);
            },
            |request: &mut WorkerRequest| {
                request
                    .dependencies
                    .retain(|value| value != &image().libraries[0].identity);
            },
            |request: &mut WorkerRequest| {
                if let WorkerOperation::Generate { settings, .. } = &mut request.operation {
                    settings.inference_profile = None;
                }
            },
            |request: &mut WorkerRequest| {
                if let WorkerOperation::Generate { settings, .. } = &mut request.operation {
                    settings.inference_profile.as_mut().unwrap().runtime = identity(5);
                }
            },
        ] {
            let mut db = database();
            let mut request = request("invalid", true);
            mutate(&mut request);
            seed(&db, &request);
            let tx = db.transaction().unwrap();
            assert!(declare(&tx, &request, &image()).is_err());
            assert!(no_cpu_table(&tx));
            assert_eq!(runtime_version(&tx).unwrap(), 20);
        }
    }

    #[test]
    fn unexpected_layout_or_temporary_shadow_refuses_read_only() {
        for update in [
            "CREATE TRIGGER cpu_runtime_extra AFTER INSERT ON cpu_runtime_attempts BEGIN SELECT 1; END",
            "CREATE TEMP TABLE cpu_runtime_attempts(dummy TEXT)",
            "CREATE TABLE cpu_runtime_unexpected(dummy TEXT)",
        ] {
            let (db, request) = declared();
            db.execute_batch(update).unwrap();
            db.execute_batch("PRAGMA query_only=ON").unwrap();
            assert_refuses(&db, &request);
        }
    }

    #[test]
    fn tagged_rows_cannot_borrow_another_valid_attempts_manifest() {
        let (db, request) = declared();
        let raw = serde_json::to_string(&request).unwrap();
        db.execute(
            "INSERT INTO attempts VALUES('forged-attempt',?1,?2,'failed')",
            params![request.job_id.as_str(), raw],
        )
        .unwrap();
        db.execute("UPDATE jobs SET attempts=2", []).unwrap();
        assert!(validate(&db).is_err());
    }

    #[test]
    fn declaration_count_admission_precedes_semantic_row_loading() {
        let (db, request) = declared();
        db.execute_batch("PRAGMA foreign_keys=OFF; WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<4096) INSERT INTO cpu_runtime_attempts SELECT 'extra-'||i,job,manifest_sha256,image_sha256,image FROM n CROSS JOIN cpu_runtime_attempts WHERE attempt='attempt-1'").unwrap();
        assert!(validate(&db)
            .unwrap_err()
            .to_string()
            .contains("row capacity"));
        assert!(retained(&db, &request)
            .unwrap_err()
            .to_string()
            .contains("row capacity"));
    }
}
