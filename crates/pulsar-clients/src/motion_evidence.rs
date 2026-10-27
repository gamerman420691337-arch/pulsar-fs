//! Local, bounded read-only verification. Never imports or follows archive paths.
#[cfg(unix)]
use anyhow::Context;
use anyhow::{ensure, Result};
use pulsar_protocol::{project_package_format::PackageCodecLimits, *};
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
    time::{Duration, Instant},
};

struct DeadlineReader {
    file: File,
    deadline: Instant,
    control: Option<crate::project_package::PackageDownloadControl>,
}
impl Read for DeadlineReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if let Some(control) = &self.control {
            control.check().map_err(io::Error::other)?;
        }
        if Instant::now() >= self.deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "motion evidence verification deadline expired",
            ));
        }
        self.file.read(bytes)
    }
}
#[cfg(unix)]
fn open_regular(path: &Path, limit: u64) -> Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(i32::try_from(
            (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits(),
        )?);
    }
    let file = options.open(path).context("open motion evidence input")?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() > 0 && metadata.len() <= limit,
        "motion evidence input is not an admitted regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.nlink() == 1,
            "motion evidence input has multiple links"
        );
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open_regular(_path: &Path, _limit: u64) -> Result<File> {
    anyhow::bail!("held regular-file motion evidence verification is unsupported on this platform")
}

/// Checks two pinned regular files within an absolute local deadline. Hashes and
/// metadata are integrity evidence; this grants no execution or import authority.
pub fn verify_motion_evidence_files(
    script: &Path,
    package: &Path,
    project: &ProjectId,
    revision: RevisionId,
    axis: Axis,
) -> Result<MotionEvidenceAssociation> {
    verify_motion_evidence_files_control(script, package, project, revision, axis, None)
}

pub(crate) fn verify_motion_evidence_files_control(
    script: &Path,
    package: &Path,
    project: &ProjectId,
    revision: RevisionId,
    axis: Axis,
    control: Option<&crate::project_package::PackageDownloadControl>,
) -> Result<MotionEvidenceAssociation> {
    let deadline = Instant::now() + Duration::from_secs(300);
    let script_file = open_regular(script, MAX_MOTION_BYTES)?;
    let package_file = open_regular(package, DEFAULT_MAX_PROJECT_PACKAGE_BYTES)?;
    #[cfg(unix)]
    let before_script = script_file.metadata()?;
    #[cfg(unix)]
    let before_package = package_file.metadata()?;
    let mut script_reader = DeadlineReader {
        file: script_file,
        deadline,
        control: control.cloned(),
    };
    let mut package_reader = DeadlineReader {
        file: package_file,
        deadline,
        control: control.cloned(),
    };
    let mut bytes = Vec::new();
    // The extra byte refuses growth after admission instead of truncating input.
    (&mut script_reader)
        .take(MAX_MOTION_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_MOTION_BYTES,
        "neutral script grew beyond admission"
    );
    let report = verify_motion_evidence(
        &bytes,
        &mut package_reader,
        project,
        revision,
        axis,
        PackageCodecLimits::default(),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        for (before, after) in [
            (before_script, script_reader.file.metadata()?),
            (before_package, package_reader.file.metadata()?),
        ] {
            ensure!(
                before.dev() == after.dev()
                    && before.ino() == after.ino()
                    && before.len() == after.len()
                    && before.mtime() == after.mtime()
                    && before.mtime_nsec() == after.mtime_nsec()
                    && before.ctime() == after.ctime()
                    && before.ctime_nsec() == after.ctime_nsec()
                    && after.nlink() == 1,
                "motion evidence input changed during verification"
            );
        }
    }
    ensure!(
        Instant::now() < deadline,
        "motion evidence verification deadline expired"
    );
    Ok(report)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    #[test]
    fn evidence_input_refuses_symlinks_hardlinks_directories_empty_and_oversized_files() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        std::fs::write(&source, b"private input").unwrap();
        assert!(open_regular(&source, 64).is_ok());
        let link = root.path().join("symlink");
        symlink(&source, &link).unwrap();
        assert!(open_regular(&link, 64).is_err());
        let hard = root.path().join("hardlink");
        std::fs::hard_link(&source, &hard).unwrap();
        assert!(open_regular(&source, 64).is_err());
        assert!(open_regular(root.path(), 64).is_err());
        let empty = root.path().join("empty");
        File::create(&empty).unwrap();
        assert!(open_regular(&empty, 64).is_err());
        let large = root.path().join("large");
        File::create(&large).unwrap().set_len(65).unwrap();
        assert!(open_regular(&large, 64).is_err());
    }
    #[test]
    fn local_deadline_precedes_even_a_regular_file_read() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        std::fs::write(&source, b"bytes").unwrap();
        let mut reader = DeadlineReader {
            file: open_regular(&source, 64).unwrap(),
            deadline: Instant::now(),
            control: None,
        };
        assert_eq!(
            reader.read(&mut [0; 1]).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
    }
    #[test]
    fn evidence_check_is_explicit_read_only_cli_and_requires_binding() {
        use clap::Parser;
        let parsed = crate::cli::Cli::try_parse_from([
            "pulsar",
            "motion-evidence-check",
            "motion.funscript",
            "project.pulsar",
            "--project",
            "p",
            "--revision",
            "0",
            "--axis",
            "yaw",
        ])
        .unwrap();
        assert!(!crate::command_needs_engine(
            &[
                "pulsar",
                "motion-evidence-check",
                "motion.funscript",
                "project.pulsar",
                "--project",
                "p",
                "--revision",
                "0"
            ]
            .map(std::ffi::OsString::from)
        ));
        assert!(matches!(
            parsed.command,
            Some(crate::cli::Commands::MotionEvidenceCheck { revision: 0, .. })
        ));
        assert!(crate::cli::Cli::try_parse_from([
            "pulsar",
            "motion-evidence-check",
            "motion.funscript",
            "project.pulsar"
        ])
        .is_err());
    }
}
