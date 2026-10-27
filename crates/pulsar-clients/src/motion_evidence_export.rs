//! Explicit private paired publication. No implicit packaging grant or master edit.
use crate::{
    project_package::{PackageDownloadControl, PackagePrivacyConsent, PackagePublicationOutcome},
    session::SessionClient,
};
use anyhow::Result;
use pulsar_protocol::*;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct PublishedMotionEvidence {
    pub destination: PathBuf,
    pub association: MotionEvidenceAssociation,
    pub operation_id: PackageOperationId,
    pub file_and_directory_synced: bool,
    pub engine_release_requested: bool,
}
#[derive(Debug, Serialize)]
pub struct MotionEvidenceExportError {
    pub destination: PathBuf,
    pub outcome: PackagePublicationOutcome,
    pub recovery_staging_path: PathBuf,
    pub detail: String,
}
impl std::fmt::Display for MotionEvidenceExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f,"Motion/evidence publication {:?}: {}; private staging/output at {}. Engine exports were not cancelled or released. ",self.outcome,self.detail,self.recovery_staging_path.display())?;
        match self.outcome {
            PackagePublicationOutcome::NotPublished => f.write_str("Use motion-evidence-resume with the same authenticated session after resolving the stated failure."),
            PackagePublicationOutcome::OutcomeUnknown => f.write_str("Inspect both destination and staging before any retry; publication outcome is unknown."),
            PackagePublicationOutcome::PublishedDurabilityUnconfirmed => f.write_str("The directory is visible but durability is unconfirmed; do not start or replay another publication."),
        }
    }
}
impl std::error::Error for MotionEvidenceExportError {}

pub fn export_motion_evidence(
    client: &mut SessionClient,
    project: ProjectId,
    revision: RevisionId,
    axis: Axis,
    destination: &Path,
    consent: &PackagePrivacyConsent,
    control: &PackageDownloadControl,
) -> Result<PublishedMotionEvidence> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        control.check()?;
        let stage = local::Stage::new(client, project, revision, axis, destination)?;
        local::run(client, stage, consent, control)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (
            client,
            project,
            revision,
            axis,
            destination,
            consent,
            control,
        );
        Err(
            ProtocolError::unsupported("atomic paired directory publication on this platform")
                .into(),
        )
    }
}
pub fn resume_motion_evidence(
    client: &mut SessionClient,
    staging: &Path,
    consent: &PackagePrivacyConsent,
    control: &PackageDownloadControl,
) -> Result<PublishedMotionEvidence> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        control.check()?;
        let stage = local::Stage::resume(client, staging)?;
        local::run(client, stage, consent, control)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (client, staging, consent, control);
        Err(
            ProtocolError::unsupported("atomic paired directory publication on this platform")
                .into(),
        )
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
mod local {
    use super::*;
    use crate::{session::new_request_id, EngineApi, ResponseBody};
    use anyhow::{ensure, Context};
    use rustix::fs::{mkdirat, open, openat, statat, AtFlags, Mode, OFlags};
    use serde::{Deserialize, Serialize};
    use std::{
        ffi::OsString,
        fs::File,
        io::{Read, Write},
        os::unix::fs::MetadataExt,
        time::{Duration, Instant},
    };
    const INTENT: &str = "intent.json";
    const SCRIPT: &str = "motion.funscript";
    const PACKAGE: &str = "project.pulsar";
    const ASSOCIATION: &str = "association.json";
    const STARTED: &str = "download-started.json";
    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Intent {
        schema_version: u16,
        credential_binding: String,
        project: ProjectId,
        revision: RevisionId,
        axis: Axis,
        destination: PathBuf,
        script_request: RequestId,
        package_request: RequestId,
        download_request: RequestId,
    }
    pub(super) struct Stage {
        intent: Intent,
        parent_path: PathBuf,
        parent: File,
        directory: File,
        name: OsString,
    }
    impl Stage {
        pub(super) fn new(
            client: &SessionClient,
            project: ProjectId,
            revision: RevisionId,
            axis: Axis,
            destination: &Path,
        ) -> Result<Self> {
            ensure!(
                destination.is_absolute(),
                "Absolute paired destination required"
            );
            let name = destination
                .file_name()
                .context("New directory name required")?;
            let parent_path = destination
                .parent()
                .context("Destination parent required")?
                .canonicalize()?;
            let parent = File::from(open(
                &parent_path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            require_absent(&parent, name)?;
            let stage_name =
                OsString::from(format!(".pulsar-motion-evidence-{}", uuid::Uuid::new_v4()));
            mkdirat(&parent, &stage_name, Mode::from_raw_mode(0o700))?;
            let directory = File::from(openat(
                &parent,
                &stage_name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            rustix::fs::flock(
                &directory,
                rustix::fs::FlockOperation::NonBlockingLockExclusive,
            )
            .context("Another helper owns this paired staging directory")?;
            let stage = Self {
                intent: Intent {
                    schema_version: 1,
                    credential_binding: client.recovery_credential_binding(),
                    project,
                    revision,
                    axis,
                    destination: parent_path.join(name),
                    script_request: new_request_id(),
                    package_request: new_request_id(),
                    download_request: new_request_id(),
                },
                parent_path,
                parent,
                directory,
                name: stage_name,
            };
            (|| -> Result<()> {
                stage.check()?;
                stage.write_new(INTENT, &serde_json::to_vec_pretty(&stage.intent)?)?;
                stage.directory.sync_all()?;
                stage.parent.sync_all()?;
                Ok(())
            })()
            .map_err(|error| {
                stage.failure(
                    PackagePublicationOutcome::NotPublished,
                    format!("Preparation failed before engine effects: {error}"),
                )
            })?;
            Ok(stage)
        }
        pub(super) fn resume(client: &SessionClient, path: &Path) -> Result<Self> {
            ensure!(
                path.is_absolute(),
                "Absolute retained staging directory required"
            );
            let name = path
                .file_name()
                .context("Retained directory name required")?
                .to_owned();
            ensure!(
                name.to_str()
                    .is_some_and(|name| name.starts_with(".pulsar-motion-evidence-")),
                "Not a paired-export staging directory"
            );
            let parent_path = path
                .parent()
                .context("Staging parent required")?
                .canonicalize()?;
            let parent = File::from(open(
                &parent_path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            let directory = File::from(openat(
                &parent,
                &name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            rustix::fs::flock(
                &directory,
                rustix::fs::FlockOperation::NonBlockingLockExclusive,
            )
            .context("Another helper owns this retained paired export; no effect submitted")?;
            let metadata = directory.metadata()?;
            ensure!(
                metadata.uid() == rustix::process::geteuid().as_raw()
                    && metadata.mode() & 0o077 == 0,
                "Retained stage is not private and owned by this user"
            );
            let mut file = regular(&directory, INTENT, 64 * 1024)?;
            let mut bytes = Vec::new();
            (&mut file).take(64 * 1024 + 1).read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() <= 64 * 1024,
                "Retained intent exceeds admission"
            );
            let intent: Intent =
                serde_json::from_slice(&bytes).context("Invalid retained export intent")?;
            ensure!(
                intent.schema_version == 1
                    && intent.credential_binding == client.recovery_credential_binding(),
                "Retained export belongs to another credential or schema"
            );
            let dest_name = intent
                .destination
                .file_name()
                .context("Invalid retained destination")?;
            ensure!(
                intent.destination == parent_path.join(dest_name) && dest_name != name,
                "Retained destination escaped its pinned parent"
            );
            Ok(Self {
                intent,
                parent_path,
                parent,
                directory,
                name,
            })
        }
        fn path(&self) -> PathBuf {
            self.parent_path.join(&self.name)
        }
        fn check(&self) -> Result<()> {
            let current = std::fs::symlink_metadata(&self.parent_path)?;
            let held = self.parent.metadata()?;
            ensure!(
                current.is_dir() && current.dev() == held.dev() && current.ino() == held.ino(),
                "Paired export parent replaced or renamed"
            );
            let named = statat(&self.parent, &self.name, AtFlags::SYMLINK_NOFOLLOW)?;
            let held = self.directory.metadata()?;
            ensure!(
                held.dev() == named.st_dev && held.ino() == named.st_ino,
                "Paired staging directory replaced or renamed"
            );
            Ok(())
        }
        fn write_new(&self, name: &str, bytes: &[u8]) -> Result<()> {
            let mut file = File::from(openat(
                &self.directory,
                name,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )?);
            file.write_all(bytes)?;
            file.sync_all()?;
            self.directory.sync_all()?;
            Ok(())
        }
        fn exists(&self, name: &str) -> Result<bool> {
            match statat(&self.directory, name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(_) => Ok(true),
                Err(rustix::io::Errno::NOENT) => Ok(false),
                Err(error) => Err(error.into()),
            }
        }
        fn failure(
            &self,
            outcome: PackagePublicationOutcome,
            error: impl std::fmt::Display,
        ) -> anyhow::Error {
            MotionEvidenceExportError {
                destination: self.intent.destination.clone(),
                outcome,
                recovery_staging_path: self.path(),
                detail: error.to_string(),
            }
            .into()
        }
        fn publish(&self, files: &[(String, File, std::fs::Metadata)]) -> Result<()> {
            self.check()?;
            for entry in rustix::fs::Dir::read_from(&self.directory)? {
                let entry = entry?;
                ensure!(
                    [
                        b".".as_slice(),
                        b"..".as_slice(),
                        INTENT.as_bytes(),
                        SCRIPT.as_bytes(),
                        PACKAGE.as_bytes(),
                        ASSOCIATION.as_bytes(),
                        STARTED.as_bytes()
                    ]
                    .contains(&entry.file_name().to_bytes()),
                    "Unexpected staging member; retain and reconcile it before publication"
                );
            }
            for (name, file, before) in files {
                let after = file.metadata()?;
                let named = statat(&self.directory, name.as_str(), AtFlags::SYMLINK_NOFOLLOW)?;
                ensure!(
                    after.is_file()
                        && after.nlink() == 1
                        && after.dev() == named.st_dev
                        && after.ino() == named.st_ino
                        && same(before, &after),
                    "Paired member changed after verification"
                );
                file.sync_all()?;
            }
            self.directory.sync_all()?;
            rustix::fs::renameat_with(
                &self.parent,
                &self.name,
                &self.parent,
                self.intent
                    .destination
                    .file_name()
                    .context("Destination name lost")?,
                rustix::fs::RenameFlags::NOREPLACE,
            )
            .map_err(|error| {
                self.failure(
                    if error == rustix::io::Errno::EXIST {
                        PackagePublicationOutcome::NotPublished
                    } else {
                        PackagePublicationOutcome::OutcomeUnknown
                    },
                    error,
                )
            })?;
            self.parent
                .sync_all()
                .map_err(|error| MotionEvidenceExportError {
                    destination: self.intent.destination.clone(),
                    outcome: PackagePublicationOutcome::PublishedDurabilityUnconfirmed,
                    recovery_staging_path: self.intent.destination.clone(),
                    detail: format!("Directory visible, parent sync failed: {error}"),
                })?;
            Ok(())
        }
    }
    fn require_absent(parent: &File, name: &std::ffi::OsStr) -> Result<()> {
        match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(_) => anyhow::bail!("Paired destination exists; nothing overwritten"),
            Err(rustix::io::Errno::NOENT) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
    fn same(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
        a.dev() == b.dev()
            && a.ino() == b.ino()
            && a.len() == b.len()
            && a.mtime() == b.mtime()
            && a.mtime_nsec() == b.mtime_nsec()
            && a.ctime() == b.ctime()
            && a.ctime_nsec() == b.ctime_nsec()
    }
    fn regular(directory: &File, name: &str, limit: u64) -> Result<File> {
        let file = File::from(openat(
            directory,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file()
                && metadata.nlink() == 1
                && metadata.len() > 0
                && metadata.len() <= limit,
            "Paired member is not an admitted regular file"
        );
        Ok(file)
    }
    pub(super) fn run(
        client: &mut SessionClient,
        stage: Stage,
        consent: &PackagePrivacyConsent,
        control: &PackageDownloadControl,
    ) -> Result<PublishedMotionEvidence> {
        let result = (|| {
            control.check()?;
            stage.check()?;
            let script_path = stage.path().join(SCRIPT);
            let response = client.execute_identified(
                stage.intent.script_request.clone(),
                Command::ExportAxis {
                    path: script_path.clone(),
                    axis: stage.intent.axis,
                },
                Some(stage.intent.project.clone()),
                Some(stage.intent.revision),
            )?;
            ensure!(
                matches!(response,ResponseBody::AxisExported {path,revision,axis} if path==script_path && revision==stage.intent.revision && axis==stage.intent.axis),
                "Axis export response binding differs from durable intent"
            );
            control.check()?;
            stage.check()?;
            let mut status = client.start_project_export_with_request_id(
                stage.intent.project.clone(),
                stage.intent.revision,
                consent,
                stage.intent.package_request.clone(),
            )?;
            let operation = status.operation_id.clone();
            let deadline = Instant::now() + Duration::from_secs(30 * 60);
            while !status.state.is_terminal() {
                control.check()?;
                stage.check()?;
                ensure!(
                    Instant::now() < deadline,
                    "Package wait expired; retained operation {operation} was not cancelled"
                );
                std::thread::sleep(Duration::from_millis(100));
                status = client.project_package_status_until(
                    stage.intent.project.clone(),
                    operation.clone(),
                    deadline,
                )?;
                ensure!(
                    status.request_id == stage.intent.package_request
                        && status.requested_revision == stage.intent.revision
                        && status.operation_id == operation,
                    "Package status changed its request/revision/operation binding"
                );
            }
            ensure!(
                status.state == PackageExportOperationState::Ready,
                "Retained package {operation} ended {:?}: {:?}",
                status.state,
                status.error
            );
            let package_path = stage.path().join(PACKAGE);
            if !stage.exists(PACKAGE)? {
                ensure!(!stage.exists(STARTED)?,"Download was previously submitted. Do not blindly replay its Begin identity. Reconcile/retire the previous lease, then use package-download {} {} {} with explicit privacy consent to recover this member before resuming",stage.intent.project,operation,package_path.display());
                control.check()?;
                stage.check()?;
                stage.write_new(
                    STARTED,
                    &serde_json::to_vec(&stage.intent.download_request)?,
                )?;
                client.download_project_package_with_request_id(
                    stage.intent.project.clone(),
                    operation.clone(),
                    &package_path,
                    consent,
                    control,
                    stage.intent.download_request.clone(),
                )?;
            }
            control.check()?;
            stage.check()?;
            let mut files = Vec::new();
            for (name, limit) in [
                (SCRIPT, MAX_MOTION_BYTES),
                (PACKAGE, DEFAULT_MAX_PROJECT_PACKAGE_BYTES),
            ] {
                let file = regular(&stage.directory, name, limit)?;
                let metadata = file.metadata()?;
                files.push((name.to_owned(), file, metadata));
            }
            let association = crate::motion_evidence::verify_motion_evidence_files_control(
                &script_path,
                &package_path,
                &stage.intent.project,
                stage.intent.revision,
                stage.intent.axis,
                Some(control),
            )?;
            control.check()?;
            stage.check()?;
            let artifact = status
                .artifact
                .as_ref()
                .context("Ready package has no artifact")?;
            ensure!(
                association.package_sha256 == artifact.sha256
                    && association.package_byte_len == artifact.byte_len
                    && association.package_format_version == artifact.format_version
                    && artifact.project_id == stage.intent.project
                    && artifact.captured_revision == stage.intent.revision,
                "Recovered member differs from the original retained package artifact"
            );
            crate::package_staging_recovery::reconcile(
                &stage.directory,
                &files
                    .iter()
                    .find(|(name, _, _)| name == PACKAGE)
                    .context("Verified package file missing")?
                    .1,
                std::ffi::OsStr::new(PACKAGE),
                &crate::package_staging_recovery::DownloadBinding {
                    credential_binding: client.recovery_credential_binding(),
                    project: stage.intent.project.clone(),
                    operation: operation.clone(),
                    request: stage.intent.download_request.clone(),
                    artifact: artifact.clone(),
                },
                control,
            )?;
            let association_bytes = serde_json::to_vec_pretty(&association)?;
            if stage.exists(ASSOCIATION)? {
                let mut existing = regular(&stage.directory, ASSOCIATION, 64 * 1024)?;
                let mut bytes = Vec::new();
                (&mut existing)
                    .take(64 * 1024 + 1)
                    .read_to_end(&mut bytes)?;
                ensure!(
                    bytes == association_bytes,
                    "Retained association differs from independently verified inputs"
                );
            } else {
                stage.write_new(ASSOCIATION, &association_bytes)?;
            }
            for (name, expected) in [
                (INTENT, serde_json::to_vec_pretty(&stage.intent)?),
                (STARTED, serde_json::to_vec(&stage.intent.download_request)?),
            ] {
                if stage.exists(name)? {
                    let mut file = regular(&stage.directory, name, 64 * 1024)?;
                    let before = file.metadata()?;
                    let mut bytes = Vec::new();
                    (&mut file).take(64 * 1024 + 1).read_to_end(&mut bytes)?;
                    ensure!(
                        bytes == expected && same(&before, &file.metadata()?),
                        "Retained intent or transfer marker changed"
                    );
                    files.push((name.to_owned(), file, before));
                }
            }
            let file = regular(&stage.directory, ASSOCIATION, 64 * 1024)?;
            let metadata = file.metadata()?;
            files.push((ASSOCIATION.to_owned(), file, metadata));
            control.check()?;
            stage.publish(&files)?;
            Ok(PublishedMotionEvidence {
                destination: stage.intent.destination.clone(),
                association,
                operation_id: operation,
                file_and_directory_synced: true,
                engine_release_requested: false,
            })
        })();
        result.map_err(|error: anyhow::Error| {
            if error
                .chain()
                .any(|cause| cause.is::<MotionEvidenceExportError>())
            {
                error
            } else {
                stage.failure(PackagePublicationOutcome::NotPublished, error)
            }
        })
    }
    #[cfg(target_os = "linux")]
    #[cfg(test)]
    mod tests {
        use super as local;
        use super::*;
        use std::{
            fs::File,
            os::unix::fs::{symlink, MetadataExt},
        };
        fn client(auth: &str) -> SessionClient {
            SessionClient::from_credentials(
                Path::new("/nonexistent/paired-test.sock"),
                SessionId::new("paired-test-session").unwrap(),
                auth.into(),
            )
            .unwrap()
        }
        fn new(root: &Path) -> local::Stage {
            local::Stage::new(
                &client("test-placeholder"),
                ProjectId::new("p").unwrap(),
                RevisionId::new(4),
                Axis::Yaw,
                &root.join("published"),
            )
            .unwrap()
        }
        #[test]
        #[ignore = "owned Engine and project supplied by paired recovery harness"]
        fn native_prepare_retained_intent_before_effects() {
            let state = PathBuf::from(
                std::env::var_os("PULSAR_MOTION_EVIDENCE_STATE").expect("owned state"),
            );
            let project = ProjectId::new(
                std::env::var("PULSAR_MOTION_EVIDENCE_PROJECT").expect("owned project"),
            )
            .unwrap();
            let destination = PathBuf::from(
                std::env::var_os("PULSAR_MOTION_EVIDENCE_OUTPUT").expect("owned output"),
            );
            let mut client =
                SessionClient::connect(&state.join("engine.sock"), &state.join("pairing.token"))
                    .unwrap();
            let before = crate::get_snapshot(&mut client, &project).unwrap();
            let stage = local::Stage::new(
                &client,
                project.clone(),
                before.revision,
                Axis::Stroke,
                &destination,
            )
            .unwrap();
            assert_eq!(std::fs::read_dir(stage.path()).unwrap().count(), 1);
            assert!(!destination.exists());
            let after = crate::get_snapshot(&mut client, &project).unwrap();
            assert_eq!(before.program, after.program);
            assert_eq!(before.revision, after.revision);
        }
        #[test]
        fn paired_cancel_precedes_preparation_and_every_regular_read() {
            let root = tempfile::tempdir().unwrap();
            let control = PackageDownloadControl::default();
            control.cancel();
            assert!(export_motion_evidence(
                &mut client("test-placeholder"),
                ProjectId::new("p").unwrap(),
                RevisionId::new(0),
                Axis::Stroke,
                &root.path().join("out"),
                &PackagePrivacyConsent::acknowledge_unencrypted_private_data(),
                &control
            )
            .is_err());
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
            let path = root.path().join("motion");
            std::fs::write(&path, b"{} ").unwrap();
            assert!(
                crate::motion_evidence::verify_motion_evidence_files_control(
                    &path,
                    &path,
                    &ProjectId::new("p").unwrap(),
                    RevisionId::new(0),
                    Axis::Stroke,
                    Some(&control)
                )
                .unwrap_err()
                .to_string()
                .contains("cancelled")
            );
        }
        #[test]
        fn paired_cli_requires_explicit_privacy_and_revision() {
            use clap::Parser;
            assert!(crate::cli::Cli::try_parse_from([
                "pulsar",
                "motion-evidence-export",
                "p",
                "/new/output",
                "--revision",
                "4",
                "--axis",
                "yaw",
                "--acknowledge-unencrypted-private-data"
            ])
            .is_ok());
            assert!(crate::cli::Cli::try_parse_from([
                "pulsar",
                "motion-evidence-export",
                "p",
                "/new/output",
                "--revision",
                "4"
            ])
            .is_err());
            assert!(crate::cli::Cli::try_parse_from([
                "pulsar",
                "motion-evidence-resume",
                "/retained"
            ])
            .is_err());
        }
        #[test]
        fn paired_stage_persists_exact_ids_private_modes_and_credential_binding() {
            let root = tempfile::tempdir().unwrap();
            let stage = new(root.path());
            let path = stage.path();
            assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o700);
            assert_eq!(
                std::fs::metadata(path.join("intent.json")).unwrap().mode() & 0o777,
                0o600
            );
            let bytes = std::fs::read(path.join("intent.json")).unwrap();
            let script_id = stage.intent.script_request.clone();
            assert_ne!(stage.intent.script_request, stage.intent.package_request);
            assert_ne!(stage.intent.package_request, stage.intent.download_request);
            assert!(local::Stage::resume(&client("test-placeholder"), &path).is_err());
            drop(stage);
            let resumed = local::Stage::resume(&client("test-placeholder"), &path).unwrap();
            assert_eq!(script_id, resumed.intent.script_request);
            drop(resumed);
            assert!(local::Stage::resume(&client("different-test-placeholder"), &path).is_err());
            assert_eq!(bytes, std::fs::read(path.join("intent.json")).unwrap());
        }
        fn members(stage: &local::Stage) -> Vec<(String, File, std::fs::Metadata)> {
            ["motion.funscript", "project.pulsar", "association.json"]
                .into_iter()
                .map(|name| {
                    stage.write_new(name, name.as_bytes()).unwrap();
                    let file = File::open(stage.path().join(name)).unwrap();
                    let metadata = file.metadata().unwrap();
                    (name.into(), file, metadata)
                })
                .collect()
        }
        #[test]
        fn paired_directory_publication_is_all_at_once_and_no_replace() {
            let root = tempfile::tempdir().unwrap();
            let stage = new(root.path());
            let files = members(&stage);
            assert!(!root.path().join("published").exists());
            stage.publish(&files).unwrap();
            assert!(!stage.path().exists());
            for (name, _, _) in files {
                assert_eq!(
                    std::fs::read(root.path().join("published").join(&name)).unwrap(),
                    name.as_bytes()
                );
            }
            let other = tempfile::tempdir().unwrap();
            let stage = new(other.path());
            let files = members(&stage);
            std::fs::create_dir(other.path().join("published")).unwrap();
            std::fs::write(other.path().join("published/sentinel"), b"preserve").unwrap();
            let error = stage.publish(&files).unwrap_err();
            assert_eq!(
                error
                    .downcast_ref::<MotionEvidenceExportError>()
                    .unwrap()
                    .outcome,
                PackagePublicationOutcome::NotPublished
            );
            assert_eq!(
                std::fs::read(other.path().join("published/sentinel")).unwrap(),
                b"preserve"
            );
            assert!(stage.path().exists());
            assert!(new_checked(other.path()).is_err());
        }
        fn new_checked(root: &Path) -> Result<local::Stage> {
            local::Stage::new(
                &client("test-placeholder"),
                ProjectId::new("p").unwrap(),
                RevisionId::new(0),
                Axis::Stroke,
                &root.join("published"),
            )
        }
        #[test]
        fn paired_refuses_changed_members_extra_partial_artifacts_and_directory_substitution() {
            let root = tempfile::tempdir().unwrap();
            let stage = new(root.path());
            let files = members(&stage);
            std::fs::write(stage.path().join("motion.funscript"), b"changed").unwrap();
            assert!(stage.publish(&files).is_err());
            assert!(!root.path().join("published").exists());
            let root = tempfile::tempdir().unwrap();
            let stage = new(root.path());
            let files = members(&stage);
            std::fs::create_dir(stage.path().join("partial-package")).unwrap();
            assert!(stage.publish(&files).is_err());
            let root = tempfile::tempdir().unwrap();
            let stage = new(root.path());
            let files = members(&stage);
            let original = stage.path();
            let moved = root.path().join("moved");
            std::fs::rename(&original, &moved).unwrap();
            symlink(&moved, &original).unwrap();
            assert!(stage.check().is_err());
            assert!(stage.publish(&files).is_err());
            assert!(!root.path().join("published").exists());
        }
    }
}
