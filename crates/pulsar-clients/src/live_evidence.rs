//! Original-byte downloads use existing authenticated bounded transfer leases.
use crate::{motion::MotionIo, project_package::DestinationStage, SessionClient};
use anyhow::{bail, ensure, Result};
use pulsar_protocol::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Serialize)]
pub(crate) struct DownloadReceipt {
    pub(crate) path: PathBuf,
    pub(crate) epoch: AttemptId,
    pub(crate) part: LiveCudaEvidencePart,
    pub(crate) identity: ArtifactIdentity,
}
pub(crate) fn download(
    client: &SessionClient,
    project: ProjectId,
    revision: RevisionId,
    epoch: AttemptId,
    part: LiveCudaEvidencePart,
    output: &Path,
) -> Result<DownloadReceipt> {
    let mut io = client.retained_evidence_io()?;
    download_with(&mut io, project, revision, epoch, part, output)
}
fn download_with(
    io: &mut impl MotionIo,
    project: ProjectId,
    revision: RevisionId,
    epoch: AttemptId,
    part: LiveCudaEvidencePart,
    output: &Path,
) -> Result<DownloadReceipt> {
    let mut stage = DestinationStage::new(output)?;
    let started = Instant::now();
    let ResponseBody::Transfer(lease) = io.control(
        Command::BeginLiveCudaEvidenceDownload {
            epoch: epoch.clone(),
            part,
        },
        project.clone(),
        Some(revision),
    )?
    else {
        bail!("Unexpected retained evidence download response");
    };
    let result = (|| -> Result<ArtifactIdentity> {
        lease.validate()?;
        ensure!(
            lease.project_id == project
                && lease.direction == TransferDirection::Download
                && lease.offset == 0
                && lease.byte_len == lease.total_byte_len
                && lease.accepted_prefix == 0
                && lease.byte_len <= part.maximum_bytes()
                && lease.bulk_endpoint == io.bulk_endpoint(),
            "Evidence lease scope, range or endpoint differs"
        );
        let deadline = started + Duration::from_millis(lease.expires_after_ms.min(30000));
        let mut bulk = io.open_bulk(&lease, deadline)?;
        let mut offset = 0;
        let mut hash = Sha256::new();
        while offset < lease.byte_len {
            ensure!(
                Instant::now() < deadline,
                "Evidence download deadline elapsed"
            );
            let count = (lease.byte_len - offset).min(MAX_ARTIFACT_CHUNK_BYTES as u64) as u32;
            let bytes = bulk.download(offset, count)?;
            ensure!(
                bytes.len() == count as usize,
                "Evidence download chunk length differs"
            );
            stage.file_mut().write_all(&bytes)?;
            hash.update(&bytes);
            offset += bytes.len() as u64;
        }
        ensure!(
            Instant::now() < deadline,
            "Evidence download deadline elapsed"
        );
        let sha256 = format!("{:x}", hash.finalize());
        ensure!(sha256 == lease.sha256, "Evidence download digest differs");
        stage.file_mut().sync_all()?;
        Ok(ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone())?,
            sha256,
            byte_len: offset,
        })
    })();
    // Always abandon: completed evidence leases retain memory to serve exact retries.
    let abandoned = io.control(
        Command::AbandonTransfer {
            lease_id: lease.lease_id.clone(),
        },
        project,
        None,
    );
    let identity = result?;
    ensure!(
        matches!(abandoned?,ResponseBody::TransferAbandoned {lease_id} if lease_id==lease.lease_id),
        "Evidence lease retirement response differs"
    );
    let path = stage.publish()?;
    Ok(DownloadReceipt {
        path,
        epoch,
        part,
        identity,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::motion::BulkIo;
    use std::{cell::RefCell, rc::Rc};
    #[derive(Default)]
    struct Observations {
        chunks: usize,
        abandoned: bool,
    }
    struct Fake {
        lease: TransferLease,
        bytes: Rc<Vec<u8>>,
        observed: Rc<RefCell<Observations>>,
        fault: &'static str,
    }
    struct Reader {
        bytes: Rc<Vec<u8>>,
        observed: Rc<RefCell<Observations>>,
        fault: &'static str,
    }
    impl BulkIo for Reader {
        fn upload(&mut self, _: u64, _: &[u8]) -> Result<u64> {
            bail!("unexpected upload")
        }
        fn download(&mut self, offset: u64, len: u32) -> Result<Vec<u8>> {
            self.observed.borrow_mut().chunks += 1;
            let mut bytes = self.bytes[offset as usize..offset as usize + len as usize].to_vec();
            if self.fault == "truncated" {
                bytes.pop();
            }
            if self.fault == "digest" {
                bytes[0] ^= 1;
            }
            Ok(bytes)
        }
    }
    impl MotionIo for Fake {
        fn control(
            &mut self,
            command: Command,
            _: ProjectId,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            match command {
                Command::BeginLiveCudaEvidenceDownload { .. } => {
                    Ok(ResponseBody::Transfer(self.lease.clone()))
                }
                Command::AbandonTransfer { lease_id } => {
                    self.observed.borrow_mut().abandoned = true;
                    Ok(ResponseBody::TransferAbandoned { lease_id })
                }
                _ => bail!("unexpected control"),
            }
        }
        fn open_bulk(&mut self, _: &TransferLease, _: Instant) -> Result<Box<dyn BulkIo>> {
            Ok(Box::new(Reader {
                bytes: self.bytes.clone(),
                observed: self.observed.clone(),
                fault: self.fault,
            }))
        }
        fn bulk_endpoint(&self) -> &Path {
            Path::new("/authored/bulk")
        }
    }
    #[test]
    fn multichunk_digest_and_scope_gate_publication_and_always_abandon() {
        for fault in ["none", "truncated", "digest", "project", "redirect"] {
            let dir = tempfile::tempdir().unwrap();
            let output = dir.path().join("trace.json");
            let bytes = Rc::new(vec![42; MAX_ARTIFACT_CHUNK_BYTES + 17]);
            let observed = Rc::new(RefCell::new(Observations::default()));
            let project = ProjectId::new("project").unwrap();
            let lease = TransferLease {
                lease_id: TransferId::new("lease").unwrap(),
                engine_epoch: "boot".into(),
                project_id: if fault == "project" {
                    ProjectId::new("foreign").unwrap()
                } else {
                    project.clone()
                },
                direction: TransferDirection::Download,
                sha256: format!("{:x}", Sha256::digest(bytes.as_slice())),
                total_byte_len: bytes.len() as u64,
                offset: 0,
                byte_len: bytes.len() as u64,
                accepted_prefix: 0,
                expires_after_ms: 300000,
                bulk_endpoint: PathBuf::from(if fault == "redirect" {
                    "/foreign/bulk"
                } else {
                    "/authored/bulk"
                }),
            };
            let mut io = Fake {
                lease,
                bytes: bytes.clone(),
                observed: observed.clone(),
                fault,
            };
            let result = download_with(
                &mut io,
                project,
                RevisionId::new(0),
                AttemptId::new("capture").unwrap(),
                LiveCudaEvidencePart::Trace,
                &output,
            );
            assert!(observed.borrow().abandoned);
            if fault == "none" {
                assert_eq!(std::fs::read(&output).unwrap(), *bytes);
                assert!(result.is_ok());
                assert_eq!(observed.borrow().chunks, 2);
                assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
            } else {
                assert!(result.is_err());
                assert!(!output.exists());
                assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
            }
        }
    }
}
