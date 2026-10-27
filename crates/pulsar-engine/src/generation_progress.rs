//! Private CPU generation stream. Legacy public worker framing is unchanged.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::Path,
};

pub(crate) const MAX_UPDATES: u16 = 128;
const RESUME: &str = "initial-progress-boundary.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub schema: u16,
    pub sequence: u16,
    pub attempt: AttemptId,
    pub job: JobId,
    pub basis_sha256: String,
    pub next_source_frame: u64,
    pub analysed_samples: u64,
    pub body_sha256: String,
    pub byte_len: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    pub record: Record,
    pub body: ArtifactReference,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Envelope {
    pub request: WorkerRequest,
    pub resume: Option<Binding>,
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum Event {
    Progress(Record),
    Finished,
}

pub(crate) fn eligible(request: &WorkerRequest) -> bool {
    request.cuda_execution.is_none()
        && matches!(request.operation, WorkerOperation::Generate { .. })
}
pub(crate) fn basis(request: &WorkerRequest) -> Result<String> {
    let mut value = serde_json::to_value(request)?;
    let object = value.as_object_mut().context("worker request object")?;
    object.remove("attempt_id");
    object.remove("output_dir");
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(&value)?)))
}
fn body_name(sequence: u16) -> String {
    format!(".progress-{sequence:03}.json")
}
fn meta_name(sequence: u16) -> String {
    format!(".progress-{sequence:03}.meta.json")
}

impl Record {
    pub(crate) fn validate(&self, request: &WorkerRequest, origin: bool) -> Result<()> {
        ensure!(
            eligible(request)
                && self.schema == 1
                && self.sequence > 0
                && self.sequence <= MAX_UPDATES,
            "unsupported generation progress schema/budget"
        );
        ensure!(
            self.job == request.job_id
                && (origin || self.attempt == request.attempt_id)
                && self.basis_sha256 == basis(request)?,
            "generation progress identity/input mismatch"
        );
        validate_sha256(&self.body_sha256)?;
        ensure!(
            self.byte_len > 0
                && self.byte_len <= request.budget.output_bytes
                && self.next_source_frame > 0
                && self.analysed_samples > 0
                && self.analysed_samples <= self.next_source_frame,
            "generation progress length/count mismatch"
        );
        Ok(())
    }
}

pub(crate) fn envelope(request: &WorkerRequest) -> Result<Envelope> {
    let path = request.output_dir.join(RESUME);
    let present = match std::fs::symlink_metadata(&path) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    let resume = if present {
        let bytes = read_bounded(&path, MAX_CONTROL_BYTES as u64)?;
        let binding: Binding = serde_json::from_slice(&bytes)?;
        binding.record.validate(request, true)?;
        ensure!(
            binding.body.path == request.output_dir.join("resume-progress.json")
                && binding.body.identity.artifact_id.as_str() == binding.record.body_sha256
                && binding.body.identity.sha256 == binding.record.body_sha256
                && binding.body.identity.byte_len == binding.record.byte_len,
            "progress boundary member mismatch"
        );
        let (hash, size) =
            crate::artifacts::digest_file(&binding.body.path, request.budget.output_bytes)?;
        ensure!(
            hash == binding.record.body_sha256 && size == binding.record.byte_len,
            "progress boundary bytes changed"
        );
        Some(binding)
    } else {
        None
    };
    Ok(Envelope {
        request: request.clone(),
        resume,
    })
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = crate::artifacts::open_regular(path)?;
    ensure!(
        file.metadata()?.len() <= limit,
        "generation progress metadata exceeds bound"
    );
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "generation progress metadata changed/exceeds bound"
    );
    Ok(bytes)
}

/// Select only a fully published pair, never an orphan body left before meta.
pub(crate) fn latest(request: &WorkerRequest) -> Result<Option<Binding>> {
    if !eligible(request) {
        return Ok(None);
    }
    let mut sequence = None;
    let mut entries = 0;
    for entry in std::fs::read_dir(&request.output_dir)? {
        entries += 1;
        ensure!(entries <= 512, "progress directory entry budget");
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if let Some(number) = name
            .strip_prefix(".progress-")
            .and_then(|s| s.strip_suffix(".meta.json"))
        {
            let n: u16 = number.parse()?;
            ensure!(
                n > 0 && n <= MAX_UPDATES && name == meta_name(n),
                "noncanonical progress record name"
            );
            sequence = Some(sequence.map_or(n, |old: u16| old.max(n)));
        }
    }
    let Some(n) = sequence else {
        return Ok(envelope(request)?.resume);
    };
    let record: Record = serde_json::from_slice(&read_bounded(
        &request.output_dir.join(meta_name(n)),
        MAX_CONTROL_BYTES as u64,
    )?)?;
    record.validate(request, false)?;
    ensure!(record.sequence == n, "progress sequence mismatch");
    let path = request.output_dir.join(body_name(n));
    let (sha256, byte_len) = crate::artifacts::digest_file(&path, request.budget.output_bytes)?;
    ensure!(
        sha256 == record.body_sha256 && byte_len == record.byte_len,
        "retained progress body changed"
    );
    Ok(Some(Binding {
        record,
        body: ArtifactReference {
            path,
            identity: ArtifactIdentity {
                artifact_id: ArtifactId::new(sha256.clone())?,
                sha256,
                byte_len,
            },
        },
    }))
}

/// Called during new-attempt admission after original input validation. The
/// exact captured progress is a separate private immutable envelope boundary.
pub(crate) fn capture_resume(previous: &WorkerRequest, next: &WorkerRequest) -> Result<()> {
    let Some(mut binding) = latest(previous)? else {
        return Ok(());
    };
    binding.record.validate(next, true)?;
    let mut input = crate::artifacts::open_regular(&binding.body.path)?;
    let before = input.metadata()?;
    let mut temporary = tempfile::NamedTempFile::new_in(&next.output_dir)?;
    let count = std::io::copy(
        &mut Read::by_ref(&mut input).take(binding.record.byte_len + 1),
        &mut temporary,
    )?;
    ensure!(
        count == binding.record.byte_len
            && input.metadata()?.len() == before.len()
            && input.metadata()?.modified().ok() == before.modified().ok(),
        "progress changed during capture"
    );
    temporary.as_file().sync_all()?;
    let target = next.output_dir.join("resume-progress.json");
    temporary.persist_noclobber(&target)?;
    let (hash, size) = crate::artifacts::digest_file(&target, next.budget.output_bytes)?;
    ensure!(
        hash == binding.record.body_sha256 && size == binding.record.byte_len,
        "captured progress differs"
    );
    let mut permissions = std::fs::metadata(&target)?.permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&target, permissions)?;
    binding.body.path = target;
    crate::artifacts::publish_export(
        &next.output_dir.join(RESUME),
        &serde_json::to_vec(&binding)?,
    )?;
    crate::artifacts::sync_dir(&next.output_dir)?;
    Ok(())
}

pub(crate) fn receive(
    reader: &mut impl Read,
    request: &WorkerRequest,
) -> Result<WorkerResult, ProtocolError> {
    let result = (|| -> Result<WorkerResult> {
        let baseline = envelope(request)?.resume.map(|b| b.record);
        let mut previous: Option<Record> = None;
        loop {
            match read_message::<_, Event>(reader)? {
                Event::Finished => {
                    return crate::launch::receive_artifacts(reader, request).map_err(Into::into)
                }
                Event::Progress(record) => {
                    record.validate(request, false)?;
                    ensure!(
                        record.sequence == previous.as_ref().map_or(1, |r| r.sequence + 1)
                            && previous
                                .as_ref()
                                .or(baseline.as_ref())
                                .is_none_or(|r| record.next_source_frame > r.next_source_frame
                                    && record.analysed_samples >= r.analysed_samples),
                        "progress must advance monotonically"
                    );
                    let mut temporary = tempfile::NamedTempFile::new_in(&request.output_dir)?;
                    let mut hash = Sha256::new();
                    let mut count = 0;
                    while count < record.byte_len {
                        let bytes = read_artifact_chunk(reader)?;
                        ensure!(
                            !bytes.is_empty() && bytes.len() as u64 <= record.byte_len - count,
                            "progress chunk exceeds exact remaining bytes"
                        );
                        temporary.write_all(&bytes)?;
                        hash.update(&bytes);
                        count += bytes.len() as u64;
                    }
                    ensure!(
                        format!("{:x}", hash.finalize()) == record.body_sha256,
                        "progress body digest mismatch"
                    );
                    temporary.as_file().sync_all()?;
                    temporary
                        .persist_noclobber(request.output_dir.join(body_name(record.sequence)))?;
                    crate::artifacts::publish_export(
                        &request.output_dir.join(meta_name(record.sequence)),
                        &serde_json::to_vec(&record)?,
                    )?;
                    crate::artifacts::sync_dir(&request.output_dir)?;
                    if let Some(old) = previous {
                        // Only this receiver's own already validated generated pair.
                        std::fs::remove_file(request.output_dir.join(body_name(old.sequence)))?;
                        std::fs::remove_file(request.output_dir.join(meta_name(old.sequence)))?;
                        crate::artifacts::sync_dir(&request.output_dir)?;
                    }
                    previous = Some(record);
                }
            }
        }
    })();
    result.map_err(|error| {
        ProtocolError::new(
            ErrorCode::DependencyMismatch,
            format!("generation progress stream: {error:#}"),
        )
    })
}

/// Generated checkpoint members have a separate reservation; final result bytes
/// retain the ordinary output budget. Only canonical private names qualify.
pub(crate) fn is_checkpoint_member(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    if name == RESUME || name == "resume-progress.json" {
        return true;
    }
    (1..=MAX_UPDATES).any(|sequence| name == body_name(sequence) || name == meta_name(sequence))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(output: &Path) -> WorkerRequest {
        let mut request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        request.cuda_execution = None;
        request.output_dir = output.to_path_buf();
        request
    }
    fn record(request: &WorkerRequest, sequence: u16, cursor: u64, bytes: &[u8]) -> Record {
        Record {
            schema: 1,
            sequence,
            attempt: request.attempt_id.clone(),
            job: request.job_id.clone(),
            basis_sha256: basis(request).unwrap(),
            next_source_frame: cursor,
            analysed_samples: cursor,
            body_sha256: format!("{:x}", Sha256::digest(bytes)),
            byte_len: bytes.len() as u64,
        }
    }
    fn send(stream: &mut Vec<u8>, record: &Record, bytes: &[u8]) {
        write_message(stream, &Event::Progress(record.clone())).unwrap();
        write_artifact_chunk(stream, bytes).unwrap();
    }
    #[test]
    fn complete_checkpoint_survives_truncated_next_update_and_capture_checks_bytes() {
        let old = tempfile::tempdir().unwrap();
        let next = tempfile::tempdir().unwrap();
        let request = request(old.path());
        let bytes = b"authored completed state";
        let first = record(&request, 1, 4, bytes);
        let second = record(&request, 2, 8, b"incomplete state");
        let mut stream = Vec::new();
        send(&mut stream, &first, bytes);
        write_message(&mut stream, &Event::Progress(second)).unwrap();
        assert!(receive(&mut stream.as_slice(), &request).is_err());
        assert_eq!(
            latest(&request).unwrap().unwrap().record.next_source_frame,
            4
        );
        let mut resumed = request.clone();
        resumed.output_dir = next.path().to_path_buf();
        resumed.attempt_id = AttemptId::new("new-progress-attempt").unwrap();
        capture_resume(&request, &resumed).unwrap();
        assert_eq!(
            std::fs::read(envelope(&resumed).unwrap().resume.unwrap().body.path).unwrap(),
            bytes
        );
        let mut backward = Vec::new();
        send(&mut backward, &record(&resumed, 1, 4, bytes), bytes);
        assert!(receive(&mut backward.as_slice(), &resumed)
            .unwrap_err()
            .message
            .contains("monotonically"));
        assert!(latest(&resumed).unwrap().is_some());
        std::fs::write(old.path().join(body_name(1)), b"corrupt").unwrap();
        assert!(latest(&request).is_err());
    }
    #[test]
    fn corrupt_body_is_never_published_and_new_pairs_replace_only_validated_old_pairs() {
        let dir = tempfile::tempdir().unwrap();
        let request = request(dir.path());
        let bytes = b"state";
        let mut corrupt = Vec::new();
        send(&mut corrupt, &record(&request, 1, 2, bytes), b"wrong");
        assert!(receive(&mut corrupt.as_slice(), &request).is_err());
        assert!(latest(&request).unwrap().is_none());
        let mut good = Vec::new();
        send(&mut good, &record(&request, 1, 2, bytes), bytes);
        send(&mut good, &record(&request, 2, 4, bytes), bytes);
        assert!(receive(&mut good.as_slice(), &request).is_err()); // EOF, no final result.
        assert!(!dir.path().join(body_name(1)).exists());
        assert_eq!(latest(&request).unwrap().unwrap().record.sequence, 2);
    }
    #[cfg(unix)]
    #[test]
    fn dangling_resume_boundary_is_refused_instead_of_silent_restart() {
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(dir.path().join("missing"), dir.path().join(RESUME)).unwrap();
        assert!(envelope(&request(dir.path())).is_err());
    }
    #[test]
    fn checkpoint_accounting_accepts_only_canonical_members() {
        for name in [
            "initial-progress-boundary.json",
            "resume-progress.json",
            ".progress-001.json",
            ".progress-128.meta.json",
        ] {
            assert!(super::is_checkpoint_member(name.as_ref()));
        }
        for name in [
            "program.json",
            ".progress-000.json",
            ".progress-129.json",
            ".progress-1.json",
            ".progress-001.json.tmp",
            "resume-progress.json.tmp",
        ] {
            assert!(!super::is_checkpoint_member(name.as_ref()));
        }
    }
}
