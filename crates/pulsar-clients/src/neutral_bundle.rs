//! Authenticated, revision-bound neutral exports with atomic directory publication.
use crate::{project_package::PackageDownloadControl, SessionClient};
use anyhow::Result;
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedNeutralBundle {
    pub destination: PathBuf,
    pub project: ProjectId,
    pub revision: RevisionId,
    pub axes: Vec<Axis>,
    pub file_and_directory_synced: bool,
    pub qualified: bool,
}
/// Captured local stage identity, retained before any Engine export request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedNeutralBundle {
    pub schema: u16,
    pub staging: PathBuf,
    pub destination: PathBuf,
    pub project: ProjectId,
    pub revision: RevisionId,
    pub requests: Vec<NeutralBundleAxisIntent>,
    pub parent_identity: NeutralBundleFileIdentity,
    pub staging_identity: NeutralBundleFileIdentity,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeutralBundleFileIdentity {
    pub device: u64,
    pub inode: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeutralBundleAxisIntent {
    pub axis: Axis,
    pub request: RequestId,
}

fn checked_axes(axes: &[Axis]) -> Result<Vec<Axis>> {
    let selected: std::collections::BTreeSet<_> = axes.iter().copied().collect();
    anyhow::ensure!(
        !axes.is_empty() && axes.len() <= Axis::ALL.len() && selected.len() == axes.len(),
        "Choose one through six distinct neutral axes"
    );
    Ok(Axis::ALL
        .into_iter()
        .filter(|axis| selected.contains(axis))
        .collect())
}

/// Retain explicitly selected axis/request identities without dispatching exports.
pub fn prepare_neutral_axis_bundle(
    client: &SessionClient,
    project: ProjectId,
    revision: RevisionId,
    destination: &Path,
    axes: &[Axis],
) -> Result<PreparedNeutralBundle> {
    let axes = checked_axes(axes)?;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        local::Stage::new_axes(
            client.recovery_credential_binding(),
            project,
            revision,
            destination,
            &axes,
        )?
        .prepared()
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (client, project, revision, destination, axes);
        Err(ProtocolError::unsupported("selected neutral axis preparation on this platform").into())
    }
}

/// Creates only a retained private stage. No export or publication is dispatched.
pub fn prepare_neutral_bundle(
    client: &SessionClient,
    project: ProjectId,
    revision: RevisionId,
    destination: &Path,
) -> Result<PreparedNeutralBundle> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        local::Stage::new(
            client.recovery_credential_binding(),
            project,
            revision,
            destination,
        )?
        .prepared()
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (client, project, revision, destination);
        Err(ProtocolError::unsupported("neutral bundle preparation on this platform").into())
    }
}
/// Reuses exact captured stage/request identities; never creates a replacement stage.
pub fn publish_prepared_neutral_bundle(
    client: &mut SessionClient,
    prepared: &PreparedNeutralBundle,
    control: &PackageDownloadControl,
) -> Result<PublishedNeutralBundle> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        control.check()?;
        let stage = local::Stage::resume(&client.recovery_credential_binding(), &prepared.staging)?;
        anyhow::ensure!(
            &stage.prepared()? == prepared,
            "Prepared bundle identity differs; no export dispatched"
        );
        local::run(client, stage, control)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (client, prepared, control);
        Err(
            ProtocolError::unsupported("prepared neutral bundle publication on this platform")
                .into(),
        )
    }
}

#[derive(Debug, Serialize)]
pub struct NeutralBundleError {
    pub destination: PathBuf,
    pub outcome: crate::project_package::PackagePublicationOutcome,
    pub recovery_staging_path: PathBuf,
    pub detail: String,
}
impl std::fmt::Display for NeutralBundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Neutral bundle {:?}: {}; retained path {}. Inspect destination and staging before retry; export-bundle-resume reuses the exact intent and refuses unknown Engine outcomes.",
            self.outcome, self.detail, self.recovery_staging_path.display())
    }
}
impl std::error::Error for NeutralBundleError {}

/// Local files are observations, never acknowledgements of Engine export effects.
#[derive(Debug, Serialize)]
pub struct NeutralBundleInspection {
    pub staging: PathBuf,
    pub destination: PathBuf,
    pub project: ProjectId,
    pub revision: RevisionId,
    pub destination_present: bool,
    pub members: Vec<NeutralBundleMemberInspection>,
    pub all_local_members_match_master: bool,
    pub engine_export_outcomes_verified: bool,
    pub qualified: bool,
}
#[derive(Debug, Serialize)]
pub struct NeutralBundleMemberInspection {
    pub axis: Axis,
    pub request: RequestId,
    pub present_and_matches_master: bool,
}

/// Authenticate retained intent and compare present files without reserving exports.
pub fn inspect_neutral_bundle(
    client: &mut SessionClient,
    staging: &Path,
) -> Result<NeutralBundleInspection> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let stage = local::Stage::resume(&client.recovery_credential_binding(), staging)?;
        local::inspect(client, stage)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (client, staging);
        Err(ProtocolError::unsupported("neutral bundle inspection on this platform").into())
    }
}

/// Inspect the exact retained stage and return its original identities without export dispatch.
pub fn recover_prepared_neutral_bundle(
    client: &mut SessionClient,
    staging: &Path,
) -> Result<PreparedNeutralBundle> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let stage = local::Stage::resume(&client.recovery_credential_binding(), staging)?;
        let prepared = stage.prepared()?;
        let inspection = local::inspect(client, stage)?;
        anyhow::ensure!(
            !inspection.destination_present,
            "Destination already exists; no preparation attached"
        );
        Ok(prepared)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (client, staging);
        Err(
            ProtocolError::unsupported("neutral bundle preparation recovery on this platform")
                .into(),
        )
    }
}

pub fn export_neutral_bundle(
    client: &mut SessionClient,
    project: ProjectId,
    revision: RevisionId,
    destination: &Path,
    control: &PackageDownloadControl,
) -> Result<PublishedNeutralBundle> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        control.check()?;
        let stage = local::Stage::new(
            client.recovery_credential_binding(),
            project,
            revision,
            destination,
        )?;
        local::run(client, stage, control)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (client, project, revision, destination, control);
        Err(ProtocolError::unsupported("atomic neutral bundle publication on this platform").into())
    }
}
/// Publish only the explicitly selected current master axes; no secondary axis is synthesized.
pub fn export_neutral_axis_bundle(
    client: &mut SessionClient,
    project: ProjectId,
    revision: RevisionId,
    destination: &Path,
    axes: &[Axis],
    control: &PackageDownloadControl,
) -> Result<PublishedNeutralBundle> {
    let axes = checked_axes(axes)?;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        control.check()?;
        let stage = local::Stage::new_axes(
            client.recovery_credential_binding(),
            project,
            revision,
            destination,
            &axes,
        )?;
        local::run(client, stage, control)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (client, project, revision, destination, axes, control);
        Err(ProtocolError::unsupported("selected neutral axis publication on this platform").into())
    }
}

pub fn resume_neutral_bundle(
    client: &mut SessionClient,
    staging: &Path,
    control: &PackageDownloadControl,
) -> Result<PublishedNeutralBundle> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        control.check()?;
        let stage = local::Stage::resume(&client.recovery_credential_binding(), staging)?;
        local::run(client, stage, control)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (client, staging, control);
        Err(ProtocolError::unsupported("atomic neutral bundle recovery on this platform").into())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
mod local {
    use super::*;
    use crate::project_package::PackagePublicationOutcome;
    use crate::{get_snapshot, session::new_request_id, EngineApi, ResponseBody};
    use anyhow::{ensure, Context};
    use rustix::fs::{open, openat, statat, AtFlags, Mode, OFlags};
    use std::{
        ffi::OsString,
        fs::File,
        io::{Read, Write},
        os::unix::fs::MetadataExt,
        time::{Duration, Instant},
    };
    const INTENT: &str = ".pulsar-bundle.json";
    const PREFIX: &str = ".pulsar-neutral-bundle-";
    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Intent {
        schema: u16,
        credential_binding: String,
        project: ProjectId,
        revision: RevisionId,
        destination: PathBuf,
        requests: Vec<NeutralBundleAxisIntent>,
    }
    pub(super) struct Stage {
        intent: Intent,
        parent_path: PathBuf,
        parent: File,
        directory: File,
        name: OsString,
    }
    impl Drop for Stage {
        fn drop(&mut self) {
            let _ = rustix::fs::flock(&self.directory, rustix::fs::FlockOperation::Unlock);
        }
    }
    fn directory(path: &Path) -> Result<File> {
        Ok(File::from(open(
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?))
    }
    fn absent(parent: &File, name: &std::ffi::OsStr) -> Result<()> {
        match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
            Err(rustix::io::Errno::NOENT) => Ok(()),
            Ok(_) => anyhow::bail!("Bundle destination exists; nothing overwritten"),
            Err(error) => Err(error.into()),
        }
    }
    fn private(file: &File, is_directory: bool) -> Result<()> {
        let m = file.metadata()?;
        ensure!(
            m.uid() == rustix::process::geteuid().as_raw()
                && m.mode() & 0o077 == 0
                && if is_directory {
                    m.is_dir()
                } else {
                    m.is_file() && m.nlink() == 1
                },
            "Bundle recovery storage is not private and owned"
        );
        Ok(())
    }
    fn regular(directory: &File, name: &str, limit: u64) -> Result<File> {
        let file = File::from(openat(
            directory,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        private(&file, false)?;
        ensure!(
            file.metadata()?.len() > 0 && file.metadata()?.len() <= limit,
            "Bundle member exceeds admission"
        );
        Ok(file)
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
    fn filename(axis: Axis) -> String {
        format!("{}.funscript", axis.as_str())
    }
    impl Stage {
        pub(super) fn new(
            binding: String,
            project: ProjectId,
            revision: RevisionId,
            destination: &Path,
        ) -> Result<Self> {
            Self::new_axes(binding, project, revision, destination, &Axis::ALL)
        }
        pub(super) fn new_axes(
            binding: String,
            project: ProjectId,
            revision: RevisionId,
            destination: &Path,
            axes: &[Axis],
        ) -> Result<Self> {
            let axes = checked_axes(axes)?;
            ensure!(
                destination.is_absolute(),
                "Absolute bundle destination required"
            );
            let name = destination
                .file_name()
                .context("New directory name required")?;
            let parent_path = destination
                .parent()
                .context("Destination parent required")?
                .canonicalize()?;
            let parent = directory(&parent_path)?;
            absent(&parent, name)?;
            let stage_name = OsString::from(format!("{PREFIX}{}", uuid::Uuid::new_v4()));
            rustix::fs::mkdirat(&parent, &stage_name, Mode::from_raw_mode(0o700))?;
            let held = File::from(openat(
                &parent,
                &stage_name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            private(&held, true)?;
            rustix::fs::flock(&held, rustix::fs::FlockOperation::NonBlockingLockExclusive)?;
            let stage = Self {
                intent: Intent {
                    schema: if axes.len() == Axis::ALL.len() { 1 } else { 2 },
                    credential_binding: binding,
                    project,
                    revision,
                    destination: parent_path.join(name),
                    requests: axes
                        .into_iter()
                        .map(|axis| NeutralBundleAxisIntent {
                            axis,
                            request: new_request_id(),
                        })
                        .collect(),
                },
                parent_path,
                parent,
                directory: held,
                name: stage_name,
            };
            (|| -> Result<()> {
                stage.check()?;
                let mut file = File::from(openat(
                    &stage.directory,
                    INTENT,
                    OFlags::WRONLY
                        | OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::NOFOLLOW
                        | OFlags::CLOEXEC,
                    Mode::from_raw_mode(0o600),
                )?);
                file.write_all(&serde_json::to_vec_pretty(&stage.intent)?)?;
                file.sync_all()?;
                stage.directory.sync_all()?;
                stage.parent.sync_all()?;
                Ok(())
            })()
            .map_err(|error| stage.failure(PackagePublicationOutcome::NotPublished, error))?;
            Ok(stage)
        }
        pub(super) fn resume(binding: &str, path: &Path) -> Result<Self> {
            ensure!(
                path.is_absolute(),
                "Absolute retained staging path required"
            );
            let name = path
                .file_name()
                .context("Staging name required")?
                .to_owned();
            ensure!(
                name.to_str().is_some_and(|name| name.starts_with(PREFIX)),
                "Not a neutral bundle staging directory"
            );
            let parent_path = path
                .parent()
                .context("Staging parent required")?
                .canonicalize()?;
            let parent = directory(&parent_path)?;
            let held = File::from(openat(
                &parent,
                &name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            private(&held, true)?;
            rustix::fs::flock(&held, rustix::fs::FlockOperation::NonBlockingLockExclusive)?;
            let mut file = regular(&held, INTENT, 65536)?;
            let mut bytes = Vec::new();
            (&mut file).take(65537).read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 65536, "Bounded intent required");
            let intent: Intent = serde_json::from_slice(&bytes)?;
            let axes: Vec<_> = intent.requests.iter().map(|record| record.axis).collect();
            let canonical = checked_axes(&axes)?;
            ensure!(
                intent.schema == if axes.len() == Axis::ALL.len() { 1 } else { 2 }
                    && (intent.schema == 1 || axes == canonical),
                "Retained axis selection/schema differs"
            );
            ensure!(
                intent.credential_binding == binding
                    && intent.destination.parent() == Some(parent_path.as_path())
                    && intent.destination.file_name().is_some()
                    && intent
                        .requests
                        .iter()
                        .map(|record| &record.request)
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        == axes.len(),
                "Retained bundle credential, destination or request identities differ"
            );
            let stage = Self {
                intent,
                parent_path,
                parent,
                directory: held,
                name,
            };
            stage.check()?;
            Ok(stage)
        }
        pub(super) fn prepared(&self) -> Result<PreparedNeutralBundle> {
            self.check()?;
            let parent = self.parent.metadata()?;
            let directory = self.directory.metadata()?;
            Ok(PreparedNeutralBundle {
                schema: self.intent.schema,
                staging: self.path(),
                destination: self.intent.destination.clone(),
                project: self.intent.project.clone(),
                revision: self.intent.revision,
                requests: self.intent.requests.to_vec(),
                parent_identity: NeutralBundleFileIdentity {
                    device: parent.dev(),
                    inode: parent.ino(),
                },
                staging_identity: NeutralBundleFileIdentity {
                    device: directory.dev(),
                    inode: directory.ino(),
                },
            })
        }
        fn path(&self) -> PathBuf {
            self.parent_path.join(&self.name)
        }
        fn check(&self) -> Result<()> {
            let held = self.parent.metadata()?;
            let named = std::fs::symlink_metadata(&self.parent_path)?;
            ensure!(
                named.is_dir() && held.dev() == named.dev() && held.ino() == named.ino(),
                "Bundle parent replaced or renamed"
            );
            let held = self.directory.metadata()?;
            let named = statat(&self.parent, &self.name, AtFlags::SYMLINK_NOFOLLOW)?;
            ensure!(
                held.dev() == named.st_dev && held.ino() == named.st_ino,
                "Bundle staging directory replaced or renamed"
            );
            private(&self.directory, true)?;
            Ok(())
        }
        fn failure(
            &self,
            outcome: PackagePublicationOutcome,
            detail: impl std::fmt::Display,
        ) -> anyhow::Error {
            NeutralBundleError {
                destination: self.intent.destination.clone(),
                outcome,
                recovery_staging_path: self.path(),
                detail: detail.to_string(),
            }
            .into()
        }
        fn require_program(&self, program: &MotionProgram) -> Result<()> {
            for record in &self.intent.requests {
                let track = program
                    .track(record.axis)
                    .context("Selected neutral master axis absent")?;
                if let Some(first) = track.gaps().first() {
                    anyhow::bail!(
                        "Selected master axis {} has {} unresolved interval(s); first gap {}..{} ns in project time. Inspect retained review flags or explicitly edit the gaps before export",
                        record.axis.as_str(), track.gaps().len(), first.start().as_nanos(), first.end().as_nanos()
                    );
                }
                ensure!(
                    !track.actions().is_empty(),
                    "Selected master axis {} contains no motion actions",
                    record.axis.as_str()
                );
            }
            Ok(())
        }
        fn verify_members(
            &self,
            program: &MotionProgram,
            deadline: Instant,
        ) -> Result<Vec<(String, File, std::fs::Metadata)>> {
            self.verify_present_members(program, deadline, false)
        }
        fn verify_present_members(
            &self,
            program: &MotionProgram,
            deadline: Instant,
            allow_missing: bool,
        ) -> Result<Vec<(String, File, std::fs::Metadata)>> {
            self.check()?;
            let mut verified = Vec::new();
            let mut intent = regular(&self.directory, INTENT, 65536)?;
            let before = intent.metadata()?;
            let mut bytes = Vec::new();
            (&mut intent).take(65537).read_to_end(&mut bytes)?;
            ensure!(
                serde_json::from_slice::<serde_json::Value>(&bytes)?
                    == serde_json::to_value(&self.intent)?,
                "Durable intent changed"
            );
            ensure!(
                same(&before, &intent.metadata()?),
                "Intent changed during verification"
            );
            verified.push((INTENT.to_owned(), intent, before));
            for axis in self.intent.requests.iter().map(|record| record.axis) {
                ensure!(
                    Instant::now() < deadline,
                    "Bundle verification deadline exceeded"
                );
                let name = filename(axis);
                if allow_missing {
                    match statat(&self.directory, name.as_str(), AtFlags::SYMLINK_NOFOLLOW) {
                        Err(rustix::io::Errno::NOENT) => continue,
                        Err(error) => return Err(error.into()),
                        Ok(_) => {}
                    }
                }
                let mut file = regular(&self.directory, &name, MAX_MOTION_BYTES)?;
                let before = file.metadata()?;
                let mut bytes = Vec::new();
                (&mut file)
                    .take(MAX_MOTION_BYTES + 1)
                    .read_to_end(&mut bytes)?;
                ensure!(
                    bytes.len() as u64 <= MAX_MOTION_BYTES,
                    "Bounded script required"
                );
                let script: serde_json::Value = serde_json::from_slice(&bytes)?;
                let track = program.track(axis).context("Missing neutral master axis")?;
                ensure!(
                    track.gaps().is_empty() && !track.actions().is_empty(),
                    "Unresolved or empty master axis"
                );
                let actions = track.actions().iter().map(|action| -> Result<serde_json::Value> {
                    let ns = action.time().as_nanos(); ensure!(ns >= 0, "Negative master timestamp");
                    let at = ns.checked_add(500000).context("Timestamp overflow")?/1000000;
                    Ok(serde_json::json!({"at":at,"pos":(action.position().value()*100.0).round() as u32}))
                }).collect::<Result<Vec<_>>>()?;
                ensure!(
                    script
                        == serde_json::json!({"version":"1.0","inverted":false,"range":90,"actions":actions}),
                    "Bundle actions differ from exact neutral master"
                );
                ensure!(
                    same(&before, &file.metadata()?),
                    "Bundle member changed during verification"
                );
                verified.push((name, file, before));
            }
            ensure!(
                Instant::now() < deadline,
                "Bundle verification deadline exceeded"
            );
            Ok(verified)
        }
        fn require_only_members(&self, files: &[(String, File, std::fs::Metadata)]) -> Result<()> {
            for entry in rustix::fs::Dir::read_from(&self.directory)? {
                let entry = entry?;
                let name = entry.file_name().to_bytes();
                ensure!(
                    [b".".as_slice(), b"..".as_slice()].contains(&name)
                        || files.iter().any(|(file, _, _)| file.as_bytes() == name),
                    "Unexpected bundle member"
                );
            }
            Ok(())
        }
        fn publish(&self, files: &[(String, File, std::fs::Metadata)]) -> Result<()> {
            self.check()?;
            self.require_only_members(files)?;
            for (name, file, before) in files {
                let after = file.metadata()?;
                let named = statat(&self.directory, name.as_str(), AtFlags::SYMLINK_NOFOLLOW)?;
                ensure!(
                    same(before, &after)
                        && after.nlink() == 1
                        && after.dev() == named.st_dev
                        && after.ino() == named.st_ino,
                    "Verified bundle member replaced or changed"
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
                    .context("Destination name required")?,
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
            let result = (|| -> Result<()> {
                let named = statat(
                    &self.parent,
                    self.intent
                        .destination
                        .file_name()
                        .context("Destination name required")?,
                    AtFlags::SYMLINK_NOFOLLOW,
                )?;
                let held = self.directory.metadata()?;
                ensure!(
                    held.dev() == named.st_dev && held.ino() == named.st_ino,
                    "Published directory name replaced"
                );
                let named = std::fs::symlink_metadata(&self.parent_path)?;
                let held = self.parent.metadata()?;
                ensure!(
                    named.is_dir() && named.dev() == held.dev() && named.ino() == held.ino(),
                    "Published parent name replaced"
                );
                self.parent.sync_all()?;
                Ok(())
            })();
            result.map_err(|error| NeutralBundleError {
                destination: self.intent.destination.clone(),
                outcome: PackagePublicationOutcome::PublishedDurabilityUnconfirmed,
                recovery_staging_path: self.intent.destination.clone(),
                detail: error.to_string(),
            })?;
            Ok(())
        }
    }
    pub(super) fn inspect(
        client: &mut SessionClient,
        stage: Stage,
    ) -> Result<NeutralBundleInspection> {
        let snapshot = get_snapshot(client, &stage.intent.project)?;
        ensure!(
            snapshot.project_id == stage.intent.project
                && snapshot.revision == stage.intent.revision,
            "Inspection requires exact current project and revision"
        );
        stage.require_program(&snapshot.program)?;
        let files = stage.verify_present_members(
            &snapshot.program,
            Instant::now() + Duration::from_secs(30),
            true,
        )?;
        stage.require_only_members(&files)?;
        for (name, file, before) in &files {
            let after = file.metadata()?;
            let named = statat(&stage.directory, name.as_str(), AtFlags::SYMLINK_NOFOLLOW)?;
            ensure!(
                same(before, &after)
                    && after.nlink() == 1
                    && after.dev() == named.st_dev
                    && after.ino() == named.st_ino,
                "Inspected bundle member replaced or changed"
            );
        }
        let current = get_snapshot(client, &stage.intent.project)?;
        ensure!(
            current.revision == stage.intent.revision && current.program == snapshot.program,
            "Master changed during bundle inspection"
        );
        stage.check()?;
        let destination_present = match statat(
            &stage.parent,
            stage
                .intent
                .destination
                .file_name()
                .context("Destination name required")?,
            AtFlags::SYMLINK_NOFOLLOW,
        ) {
            Ok(_) => true,
            Err(rustix::io::Errno::NOENT) => false,
            Err(error) => return Err(error.into()),
        };
        Ok(NeutralBundleInspection {
            staging: stage.path(),
            destination: stage.intent.destination.clone(),
            project: stage.intent.project.clone(),
            revision: stage.intent.revision,
            destination_present,
            members: stage
                .intent
                .requests
                .iter()
                .map(|record| NeutralBundleMemberInspection {
                    axis: record.axis,
                    request: record.request.clone(),
                    present_and_matches_master: files
                        .iter()
                        .any(|(name, _, _)| *name == filename(record.axis)),
                })
                .collect(),
            all_local_members_match_master: files.len() == 7,
            engine_export_outcomes_verified: false,
            qualified: false,
        })
    }

    pub(super) fn run(
        client: &mut SessionClient,
        stage: Stage,
        control: &PackageDownloadControl,
    ) -> Result<PublishedNeutralBundle> {
        let result = (|| -> Result<PublishedNeutralBundle> {
            control.check()?;
            stage.check()?;
            let snapshot = get_snapshot(client, &stage.intent.project)?;
            ensure!(
                snapshot.project_id == stage.intent.project
                    && snapshot.revision == stage.intent.revision,
                "Bundle requires exact current project and revision"
            );
            stage.require_program(&snapshot.program)?;
            let existing = stage.verify_present_members(
                &snapshot.program,
                Instant::now() + Duration::from_secs(30),
                true,
            )?;
            stage.require_only_members(&existing)?;
            drop(existing);
            for record in &stage.intent.requests {
                let axis = record.axis;
                control.check()?;
                stage.check()?;
                let path = stage.path().join(filename(axis));
                let response = client.execute_identified(
                    record.request.clone(),
                    Command::ExportAxis {
                        path: path.clone(),
                        axis,
                    },
                    Some(stage.intent.project.clone()),
                    Some(stage.intent.revision),
                )?;
                ensure!(
                    matches!(response,ResponseBody::AxisExported {path:actual,revision,axis:actual_axis} if actual==path && revision==stage.intent.revision && actual_axis==axis),
                    "Bundle export acknowledgement differs from durable intent"
                );
            }
            control.check()?;
            stage.check()?;
            let current = get_snapshot(client, &stage.intent.project)?;
            ensure!(
                current.revision == stage.intent.revision && current.program == snapshot.program,
                "Master changed during bundle preparation"
            );
            let files = stage
                .verify_members(&snapshot.program, Instant::now() + Duration::from_secs(30))?;
            control.check()?;
            stage.publish(&files)?;
            Ok(PublishedNeutralBundle {
                destination: stage.intent.destination.clone(),
                project: stage.intent.project.clone(),
                revision: stage.intent.revision,
                axes: stage
                    .intent
                    .requests
                    .iter()
                    .map(|record| record.axis)
                    .collect(),
                file_and_directory_synced: true,
                qualified: false,
            })
        })();
        result.map_err(|error| {
            if error.downcast_ref::<NeutralBundleError>().is_some() {
                error
            } else {
                stage.failure(PackagePublicationOutcome::NotPublished, error)
            }
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::unix::fs::{symlink, OpenOptionsExt};
        fn stage(root: &Path) -> Stage {
            Stage::new(
                "b".repeat(64),
                ProjectId::new("authored-project").unwrap(),
                RevisionId::new(3),
                &root.join("published"),
            )
            .unwrap()
        }
        fn program() -> MotionProgram {
            MotionProgram::new(
                Axis::ALL
                    .iter()
                    .map(|axis| {
                        MotionTrack::new(
                            *axis,
                            vec![
                                MotionAction::new(
                                    ProjectTime::ZERO,
                                    NormalizedPosition::new(0.4).unwrap(),
                                    EvidenceKind::Synthesized,
                                )
                                .unwrap(),
                                MotionAction::new(
                                    ProjectTime::from_nanos(1_000_000_000),
                                    NormalizedPosition::new(0.6).unwrap(),
                                    EvidenceKind::Synthesized,
                                )
                                .unwrap(),
                            ],
                        )
                        .unwrap()
                    })
                    .collect(),
            )
            .unwrap()
        }
        fn fill(stage: &Stage) {
            for axis in stage.intent.requests.iter().map(|record| record.axis) {
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(stage.path().join(filename(axis)))
                    .unwrap();
                file.write_all(br#"{"version":"1.0","inverted":false,"range":90,"actions":[{"at":0,"pos":40},{"at":1000,"pos":60}]}"#).unwrap();
            }
        }
        fn selected_stage(root: &Path, axes: &[Axis]) -> Result<Stage> {
            Stage::new_axes(
                "b".repeat(64),
                ProjectId::new("authored-project").unwrap(),
                RevisionId::new(3),
                &root.join("published"),
                axes,
            )
        }
        #[test]
        fn invalid_axis_selection_creates_no_staging_and_full_selection_keeps_schema_one() {
            let root = tempfile::tempdir().unwrap();
            assert!(selected_stage(root.path(), &[]).is_err());
            assert!(selected_stage(root.path(), &[Axis::Stroke, Axis::Stroke]).is_err());
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
            let all = selected_stage(
                root.path(),
                &Axis::ALL.into_iter().rev().collect::<Vec<_>>(),
            )
            .unwrap();
            assert_eq!(all.intent.schema, 1);
            assert_eq!(
                all.intent
                    .requests
                    .iter()
                    .map(|r| r.axis)
                    .collect::<Vec<_>>(),
                Axis::ALL
            );
        }
        #[test]
        fn selected_stage_resumes_original_requests_and_rejects_schema_downgrade() {
            let root = tempfile::tempdir().unwrap();
            let stage = selected_stage(root.path(), &[Axis::Roll, Axis::Stroke]).unwrap();
            let prepared = stage.prepared().unwrap();
            assert_eq!(prepared.schema, 2);
            assert_eq!(
                prepared.requests.iter().map(|r| r.axis).collect::<Vec<_>>(),
                vec![Axis::Stroke, Axis::Roll]
            );
            let path = stage.path();
            drop(stage);
            let resumed = Stage::resume(&"b".repeat(64), &path).unwrap();
            assert_eq!(resumed.prepared().unwrap(), prepared);
            drop(resumed);
            let original = std::fs::read(path.join(INTENT)).unwrap();
            let mut changed: serde_json::Value = serde_json::from_slice(&original).unwrap();
            changed["schema"] = 1.into();
            std::fs::write(path.join(INTENT), serde_json::to_vec(&changed).unwrap()).unwrap();
            assert!(Stage::resume(&"b".repeat(64), &path).is_err());
            assert!(!prepared.destination.exists());
        }
        #[test]
        fn selected_master_requires_only_selected_tracks_and_publishes_exact_members() {
            let root = tempfile::tempdir().unwrap();
            let stage = selected_stage(root.path(), &[Axis::Stroke]).unwrap();
            let stroke =
                MotionProgram::new(vec![program().track(Axis::Stroke).unwrap().clone()]).unwrap();
            stage.require_program(&stroke).unwrap();
            fill(&stage);
            let files = stage
                .verify_members(&stroke, Instant::now() + Duration::from_secs(2))
                .unwrap();
            assert_eq!(files.len(), 2);
            stage.publish(&files).unwrap();
            let mut names: Vec<_> = std::fs::read_dir(&stage.intent.destination)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            names.sort();
            assert_eq!(
                names,
                vec![
                    std::ffi::OsString::from(INTENT),
                    std::ffi::OsString::from("stroke.funscript")
                ]
            );
            let other = tempfile::tempdir().unwrap();
            let missing = selected_stage(other.path(), &[Axis::Roll, Axis::Stroke]).unwrap();
            assert!(missing.require_program(&stroke).is_err());
            assert_eq!(std::fs::read_dir(missing.path()).unwrap().count(), 1);
        }
        #[test]
        fn unselected_member_blocks_selected_publication_without_replacing_destination() {
            let root = tempfile::tempdir().unwrap();
            let stage = selected_stage(root.path(), &[Axis::Stroke]).unwrap();
            fill(&stage);
            std::fs::write(stage.path().join("roll.funscript"), b"unselected").unwrap();
            let files = stage
                .verify_members(&program(), Instant::now() + Duration::from_secs(2))
                .unwrap();
            assert!(stage.publish(&files).is_err());
            assert!(!stage.intent.destination.exists());
            assert_eq!(
                std::fs::read(stage.path().join("roll.funscript")).unwrap(),
                b"unselected"
            );
        }
        #[test]
        fn prepared_descriptor_detects_stage_replacement_and_preserves_exact_axis_requests() {
            let root = tempfile::tempdir().unwrap();
            let stage = stage(root.path());
            let descriptor = stage.prepared().unwrap();
            let path = descriptor.staging.clone();
            let inherited_alias = stage.directory.try_clone().unwrap();
            drop(stage);
            let resumed = Stage::resume(&"b".repeat(64), &path).unwrap();
            assert_eq!(resumed.prepared().unwrap(), descriptor);
            drop(resumed);
            drop(inherited_alias);
            let old = root.path().join("retained-original");
            std::fs::rename(&path, &old).unwrap();
            std::fs::create_dir(&path).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            let bytes = std::fs::read(old.join(INTENT)).unwrap();
            let mut copied = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(path.join(INTENT))
                .unwrap();
            copied.write_all(&bytes).unwrap();
            drop(copied);
            let replaced = Stage::resume(&"b".repeat(64), &path).unwrap();
            let observation = replaced.prepared().unwrap();
            assert_eq!(observation.requests, descriptor.requests);
            assert_ne!(observation.staging_identity, descriptor.staging_identity);
            assert_ne!(observation, descriptor);
            assert!(path.join("stroke.funscript").symlink_metadata().is_err());
        }
        #[test]
        fn inspection_allows_missing_members_but_rejects_present_wrong_bytes_and_links() {
            let root = tempfile::tempdir().unwrap();
            let stage = stage(root.path());
            let deadline = || Instant::now() + Duration::from_secs(2);
            assert_eq!(
                stage
                    .verify_present_members(&program(), deadline(), true)
                    .unwrap()
                    .len(),
                1
            );
            assert!(stage.verify_members(&program(), deadline()).is_err());
            fill(&stage);
            assert_eq!(
                stage
                    .verify_present_members(&program(), deadline(), true)
                    .unwrap()
                    .len(),
                7
            );
            let path = stage.path().join(filename(Axis::Roll));
            let original = std::fs::read(&path).unwrap();
            std::fs::write(&path, b"{}").unwrap();
            assert!(stage
                .verify_present_members(&program(), deadline(), true)
                .is_err());
            std::fs::remove_file(&path).unwrap();
            assert_eq!(
                stage
                    .verify_present_members(&program(), deadline(), true)
                    .unwrap()
                    .len(),
                6
            );
            let outside = root.path().join("outside");
            std::fs::write(&outside, &original).unwrap();
            symlink(&outside, &path).unwrap();
            assert!(stage
                .verify_present_members(&program(), deadline(), true)
                .is_err());
            assert_eq!(std::fs::read(outside).unwrap(), original);
            assert!(!stage.intent.destination.exists());
        }
        #[test]
        fn inspection_rejects_changed_intent_without_modifying_it() {
            let root = tempfile::tempdir().unwrap();
            let stage = stage(root.path());
            let path = stage.path().join(INTENT);
            let mut changed = serde_json::to_value(&stage.intent).unwrap();
            changed["revision"] = serde_json::json!(42);
            let bytes = serde_json::to_vec(&changed).unwrap();
            std::fs::write(&path, &bytes).unwrap();
            assert!(stage
                .verify_present_members(&program(), Instant::now() + Duration::from_secs(2), true)
                .is_err());
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
        #[test]
        fn private_bundle_is_invisible_until_one_no_replace_publication() {
            let root = tempfile::tempdir().unwrap();
            let stage = stage(root.path());
            fill(&stage);
            assert!(!stage.intent.destination.exists());
            let files = stage
                .verify_members(&program(), Instant::now() + Duration::from_secs(2))
                .unwrap();
            stage.publish(&files).unwrap();
            assert!(!stage.path().exists());
            assert_eq!(
                std::fs::read_dir(&stage.intent.destination)
                    .unwrap()
                    .count(),
                7
            );
            assert_eq!(
                std::fs::metadata(&stage.intent.destination).unwrap().mode() & 0o777,
                0o700
            );
        }
        #[test]
        fn conflict_preserves_both_destination_and_complete_staging_for_resume() {
            let root = tempfile::tempdir().unwrap();
            let stage = stage(root.path());
            fill(&stage);
            let files = stage
                .verify_members(&program(), Instant::now() + Duration::from_secs(2))
                .unwrap();
            std::fs::create_dir(&stage.intent.destination).unwrap();
            std::fs::write(stage.intent.destination.join("sentinel"), b"retain").unwrap();
            assert!(stage.publish(&files).is_err());
            assert_eq!(
                std::fs::read(stage.intent.destination.join("sentinel")).unwrap(),
                b"retain"
            );
            let path = stage.path();
            drop(files);
            drop(stage);
            let resumed = Stage::resume(&"b".repeat(64), &path).unwrap();
            assert_eq!(
                resumed
                    .verify_members(&program(), Instant::now() + Duration::from_secs(2))
                    .unwrap()
                    .len(),
                7
            );
        }
        #[test]
        fn lock_credential_and_directory_identity_refuse_wrong_recovery() {
            let root = tempfile::tempdir().unwrap();
            let stage = stage(root.path());
            let path = stage.path();
            assert!(Stage::resume(&"b".repeat(64), &path).is_err());
            drop(stage);
            assert!(Stage::resume(&"c".repeat(64), &path).is_err());
            let resumed = Stage::resume(&"b".repeat(64), &path).unwrap();
            std::fs::rename(&path, root.path().join("moved")).unwrap();
            std::fs::create_dir(&path).unwrap();
            assert!(resumed.check().is_err());
        }
        #[test]
        fn recovery_records_bind_each_request_to_a_distinct_named_axis() {
            let root = tempfile::tempdir().unwrap();
            let stage = stage(root.path());
            let path = stage.path();
            drop(stage);
            let file = path.join(INTENT);
            let original: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
            let mut duplicate = original.clone();
            duplicate["requests"][5]["axis"] = duplicate["requests"][0]["axis"].clone();
            std::fs::write(&file, serde_json::to_vec(&duplicate).unwrap()).unwrap();
            assert!(Stage::resume(&"b".repeat(64), &path).is_err());
            let mut duplicate = original.clone();
            duplicate["requests"][5]["request"] = duplicate["requests"][0]["request"].clone();
            std::fs::write(&file, serde_json::to_vec(&duplicate).unwrap()).unwrap();
            assert!(Stage::resume(&"b".repeat(64), &path).is_err());
            let mut reordered = original.clone();
            reordered["requests"].as_array_mut().unwrap().reverse();
            std::fs::write(&file, serde_json::to_vec(&reordered).unwrap()).unwrap();
            let stage = Stage::resume(&"b".repeat(64), &path).unwrap();
            assert_eq!(serde_json::to_value(&stage.intent).unwrap(), reordered);
        }
        #[test]
        fn changed_content_links_and_extra_members_never_publish_as_verified() {
            let root = tempfile::tempdir().unwrap();
            let stage = stage(root.path());
            fill(&stage);
            let files = stage
                .verify_members(&program(), Instant::now() + Duration::from_secs(2))
                .unwrap();
            std::fs::write(stage.path().join("extra"), b"retain").unwrap();
            assert!(stage.publish(&files).is_err());
            std::fs::remove_file(stage.path().join("extra")).unwrap();
            std::fs::write(stage.path().join("stroke.funscript"), b"{}").unwrap();
            assert!(stage.publish(&files).is_err());
            assert!(stage
                .verify_members(&program(), Instant::now() + Duration::from_secs(2))
                .is_err());
            std::fs::remove_file(stage.path().join("stroke.funscript")).unwrap();
            symlink(
                stage.path().join("surge.funscript"),
                stage.path().join("stroke.funscript"),
            )
            .unwrap();
            assert!(stage
                .verify_members(&program(), Instant::now() + Duration::from_secs(2))
                .is_err());
            std::fs::remove_file(stage.path().join("stroke.funscript")).unwrap();
            std::fs::hard_link(
                stage.path().join("surge.funscript"),
                stage.path().join("stroke.funscript"),
            )
            .unwrap();
            assert!(stage
                .verify_members(&program(), Instant::now() + Duration::from_secs(2))
                .is_err());
            assert!(!stage.intent.destination.exists());
        }
    }
}
