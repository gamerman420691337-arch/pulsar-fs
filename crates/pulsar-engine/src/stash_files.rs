//! Filesystem namespace planning only. No file is generated, overwritten,
//! imported or scanned here; a later generation must capture immutable inputs.
use pulsar_protocol::*;
#[cfg(unix)]
use std::path::{Component, Path};

pub(crate) fn plan(
    discovery: StashDiscovery,
    mappings: &[StashRootMapping],
) -> Result<StashPlan, ProtocolError> {
    if mappings.is_empty() || mappings.len() > 16 {
        return Err(ProtocolError::invalid(
            "Stash requires 1..=16 root mappings",
        ));
    }
    for mapping in mappings {
        mapping.validate()?;
    }
    let files = discovery
        .scenes
        .iter()
        .map(|scene| inspect(scene, mappings))
        .collect();
    let result = StashPlan {
        discovery,
        files,
        planning_only: true,
    };
    let size = serde_json::to_vec(&result)
        .map_err(|_| ProtocolError::new(ErrorCode::Internal, "Stash plan encoding failed"))?
        .len();
    if size > 768 * 1024 {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "Stash plan exceeds bounded control response; request fewer scenes",
        ));
    }
    Ok(result)
}
fn inspect(scene: &StashScene, mappings: &[StashRootMapping]) -> StashFilePlan {
    let mut result = StashFilePlan {
        scene_id: scene.id.clone(),
        file_id: None,
        status: StashFileStatus::AmbiguousFiles,
        local_source: None,
        script_destination: None,
        observed_source_bytes: None,
    };
    if scene.interactive {
        result.status = StashFileStatus::AlreadyInteractive;
        return result;
    }
    let [file] = scene.files.as_slice() else {
        return result;
    };
    result.file_id = Some(file.id.clone());
    let valid = file.path.starts_with('/')
        && !file.path.contains('\\')
        && !file.path.chars().any(char::is_control)
        && !file
            .path
            .split('/')
            .skip(1)
            .any(|part| part.is_empty() || matches!(part, "." | ".."));
    if !valid {
        result.status = StashFileStatus::UnsafePath;
        return result;
    }
    let mut selected = None;
    for mapping in mappings {
        let root = mapping.server_root.trim_end_matches('/');
        if let Some(relative) = file
            .path
            .strip_prefix(root)
            .and_then(|suffix| suffix.strip_prefix('/'))
        {
            if selected.is_some() {
                result.status = StashFileStatus::AmbiguousMapping;
                return result;
            }
            selected = Some((mapping, relative));
        }
    }
    let Some((mapping, relative)) = selected else {
        result.status = StashFileStatus::Unmapped;
        return result;
    };
    let source = mapping.local_root.join(relative);
    let script = source.with_extension("funscript");
    if source == script {
        result.status = StashFileStatus::UnsafePath;
        return result;
    }
    #[cfg(unix)]
    let observation = observe(&source, &script);
    #[cfg(not(unix))]
    let observation: Result<u64, StashFileStatus> = Err(StashFileStatus::UnsupportedPlatform);
    match observation {
        Ok(bytes) => {
            result.status = StashFileStatus::ReadyForSnapshot;
            result.local_source = Some(source);
            result.script_destination = Some(script);
            result.observed_source_bytes = Some(bytes);
        }
        Err(status) => result.status = status,
    }
    result
}

// Explicit selection is separate from automatic planning. Multiple records with
// the same ID remain ambiguous, and the selected path still uses no-follow IO.
fn inspect_selected(
    scene: &StashScene,
    file_id: &str,
    mappings: &[StashRootMapping],
) -> StashFilePlan {
    if scene.interactive {
        return inspect(scene, mappings);
    }
    let mut seen = std::collections::HashSet::new();
    if scene.files.iter().any(|file| !seen.insert(&file.id)) {
        let mut ambiguous = scene.clone();
        ambiguous.files.clear();
        return inspect(&ambiguous, mappings);
    }
    let mut matching = scene.files.iter().filter(|file| file.id == file_id);
    let Some(file) = matching.next() else {
        let mut ambiguous = scene.clone();
        ambiguous.files.clear();
        return inspect(&ambiguous, mappings);
    };
    if matching.next().is_some() {
        let mut ambiguous = scene.clone();
        ambiguous.files.clear();
        return inspect(&ambiguous, mappings);
    }
    let mut selected = scene.clone();
    selected.files = vec![file.clone()];
    inspect(&selected, mappings)
}

/// Walk from the filesystem root with directory descriptors. O_NOFOLLOW is
/// applied to every component, not just the final source. Existing dangling
/// symlinks and special entries at the destination also block generation.
#[cfg(unix)]
fn observe(source: &Path, script: &Path) -> Result<u64, StashFileStatus> {
    open_observed(source, script).map(|(_, bytes)| bytes)
}

#[cfg(unix)]
fn open_observed(source: &Path, script: &Path) -> Result<(std::fs::File, u64), StashFileStatus> {
    use std::{
        ffi::CString,
        fs::File,
        os::fd::{AsRawFd, FromRawFd},
    };
    fn name(value: &std::ffi::OsStr) -> Result<CString, StashFileStatus> {
        use std::os::unix::ffi::OsStrExt;
        CString::new(value.as_bytes()).map_err(|_| StashFileStatus::UnsafePath)
    }
    let directory = open_parent(source)?;
    let filename = name(source.file_name().ok_or(StashFileStatus::UnsafePath)?)?;
    let raw = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            filename.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if raw < 0 {
        return Err(StashFileStatus::UnavailableSource);
    }
    let input = unsafe { File::from_raw_fd(raw) };
    let metadata = input
        .metadata()
        .map_err(|_| StashFileStatus::UnavailableSource)?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(StashFileStatus::UnavailableSource);
    }
    let destination = name(script.file_name().ok_or(StashFileStatus::UnsafePath)?)?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    let found = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            destination.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if found == 0 {
        return Err(StashFileStatus::ExistingScript);
    }
    if std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOENT) {
        return Err(StashFileStatus::UnavailableSource);
    }
    Ok((input, metadata.len()))
}

#[cfg(unix)]
fn open_parent(path: &Path) -> Result<std::fs::File, StashFileStatus> {
    use std::{
        ffi::CString,
        fs::File,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
    };
    if !path.is_absolute() {
        return Err(StashFileStatus::UnsafePath);
    }
    fn name(value: &std::ffi::OsStr) -> Result<CString, StashFileStatus> {
        CString::new(value.as_bytes()).map_err(|_| StashFileStatus::UnsafePath)
    }
    let raw = unsafe {
        libc::open(
            c"/".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if raw < 0 {
        return Err(StashFileStatus::UnavailableSource);
    }
    let mut directory = unsafe { File::from_raw_fd(raw) };
    let parent = path.parent().ok_or(StashFileStatus::UnsafePath)?;
    for part in parent.components() {
        let Component::Normal(part) = part else {
            if matches!(part, Component::RootDir) {
                continue;
            }
            return Err(StashFileStatus::UnsafePath);
        };
        let part = name(part)?;
        let raw = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                part.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if raw < 0 {
            return Err(StashFileStatus::UnsafePath);
        }
        directory = unsafe { File::from_raw_fd(raw) };
    }
    Ok(directory)
}

/// A successful capture owns bytes, not the mutable path returned by planning.
/// No script output or Stash mutation occurs here.
#[cfg(unix)]
pub(crate) fn capture(
    scene: &StashScene,
    explicit_file_id: Option<&str>,
    mappings: &[StashRootMapping],
    snapshots: &Path,
    max_bytes: u64,
    held_storage: u64,
) -> Result<(std::path::PathBuf, String, u64, StashFilePlan), ProtocolError> {
    if mappings.is_empty() || mappings.len() > 16 {
        return Err(ProtocolError::invalid(
            "Stash requires 1..=16 root mappings",
        ));
    }
    for mapping in mappings {
        mapping.validate()?;
    }
    let plan = match explicit_file_id {
        Some(file_id) => inspect_selected(scene, file_id, mappings),
        None => inspect(scene, mappings),
    };
    if plan.status != StashFileStatus::ReadyForSnapshot {
        return Err(ProtocolError::new(
            ErrorCode::RequestConflict,
            format!("Stash source is not eligible: {:?}", plan.status),
        ));
    }
    let source = plan.local_source.as_ref().expect("ready source");
    let destination = plan.script_destination.as_ref().expect("ready destination");
    let (mut input, _) = open_observed(source, destination).map_err(|status| {
        ProtocolError::new(
            ErrorCode::RequestConflict,
            format!("Stash eligibility changed: {status:?}"),
        )
    })?;
    let (snapshot, hash, bytes) =
        crate::artifacts::snapshot_opened_reserved(&mut input, snapshots, max_bytes, held_storage)
            .map_err(|_| {
                ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "Stash source snapshot failed or exceeded its storage bound",
                )
            })?;
    Ok((snapshot, hash, bytes, plan))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::{fs, path::PathBuf};
    fn scene(path: &str) -> StashScene {
        StashScene {
            id: "scene".into(),
            title: "fixture".into(),
            interactive: false,
            files: vec![StashVideoFile {
                id: "file".into(),
                path: path.into(),
            }],
        }
    }
    fn root(path: &Path) -> StashRootMapping {
        StashRootMapping {
            server_root: "/server/media".into(),
            local_root: path.into(),
        }
    }
    #[test]
    fn mapping_is_explicit_component_bounded_and_never_chooses_a_primary_file() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("video.mp4"), b"authored source").unwrap();
        let mapping = root(temp.path());
        let ready = inspect(
            &scene("/server/media/video.mp4"),
            std::slice::from_ref(&mapping),
        );
        assert_eq!(ready.status, StashFileStatus::ReadyForSnapshot);
        assert_eq!(ready.observed_source_bytes, Some(15));
        assert_eq!(
            ready.script_destination,
            Some(temp.path().join("video.funscript"))
        );
        for path in ["/server/media2/video.mp4", "/other/video.mp4"] {
            assert_eq!(
                inspect(&scene(path), std::slice::from_ref(&mapping)).status,
                StashFileStatus::Unmapped
            );
        }
        for path in [
            "/server/media/../video.mp4",
            "/server/media//video.mp4",
            "C:\\video.mp4",
        ] {
            assert_eq!(
                inspect(&scene(path), std::slice::from_ref(&mapping)).status,
                StashFileStatus::UnsafePath
            );
        }
        assert_eq!(
            inspect(
                &scene("/server/media/video.mp4"),
                &[mapping.clone(), mapping.clone()]
            )
            .status,
            StashFileStatus::AmbiguousMapping
        );
        let mut multi = scene("/server/media/video.mp4");
        multi.files.push(multi.files[0].clone());
        assert_eq!(
            inspect(&multi, std::slice::from_ref(&mapping)).status,
            StashFileStatus::AmbiguousFiles
        );
        multi.interactive = true;
        assert_eq!(
            inspect(&multi, &[mapping]).status,
            StashFileStatus::AlreadyInteractive
        );
    }
    #[test]
    fn explicit_multifile_selection_requires_one_id_and_preserves_no_follow() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        fs::create_dir(&real).unwrap();
        fs::write(real.join("video.mp4"), b"sterile fixture").unwrap();
        symlink(&real, temp.path().join("alias")).unwrap();
        let mapping = root(temp.path());
        let mut source = scene("/server/media/real/video.mp4");
        source.files.push(StashVideoFile {
            id: "alias".into(),
            path: "/server/media/alias/video.mp4".into(),
        });
        assert_eq!(
            inspect(&source, std::slice::from_ref(&mapping)).status,
            StashFileStatus::AmbiguousFiles
        );
        let selected = inspect_selected(&source, "file", std::slice::from_ref(&mapping));
        assert_eq!(selected.status, StashFileStatus::ReadyForSnapshot);
        assert_eq!(selected.file_id.as_deref(), Some("file"));
        fs::write(real.join("video.funscript"), b"existing").unwrap();
        assert_eq!(
            inspect_selected(&source, "file", std::slice::from_ref(&mapping)).status,
            StashFileStatus::ExistingScript
        );
        fs::remove_file(real.join("video.funscript")).unwrap();
        assert_eq!(
            inspect_selected(
                &scene("/server/media/real/video.mp4"),
                "missing",
                std::slice::from_ref(&mapping)
            )
            .status,
            StashFileStatus::AmbiguousFiles
        );
        assert_eq!(
            inspect_selected(&source, "alias", std::slice::from_ref(&mapping)).status,
            StashFileStatus::UnsafePath
        );
        assert_eq!(
            inspect_selected(&source, "missing", std::slice::from_ref(&mapping)).status,
            StashFileStatus::AmbiguousFiles
        );
        source.files.push(source.files[0].clone());
        assert_eq!(
            inspect_selected(&source, "file", std::slice::from_ref(&mapping)).status,
            StashFileStatus::AmbiguousFiles
        );
        source.interactive = true;
        assert_eq!(
            inspect_selected(&source, "file", &[mapping]).status,
            StashFileStatus::AlreadyInteractive
        );
    }

    #[test]
    fn stale_interactive_false_cannot_bypass_existing_or_unsafe_host_entries() {
        let temp = tempfile::tempdir().unwrap();
        let mapping = root(temp.path());
        let media = temp.path().join("video.mp4");
        fs::write(&media, b"fixture").unwrap();
        let script = temp.path().join("video.funscript");
        fs::write(&script, b"keep original").unwrap();
        let scene = scene("/server/media/video.mp4");
        assert_eq!(
            inspect(&scene, std::slice::from_ref(&mapping)).status,
            StashFileStatus::ExistingScript
        );
        assert_eq!(fs::read(&script).unwrap(), b"keep original");
        fs::remove_file(&script).unwrap();
        symlink("missing", &script).unwrap();
        assert_eq!(
            inspect(&scene, std::slice::from_ref(&mapping)).status,
            StashFileStatus::ExistingScript
        );
        fs::remove_file(&script).unwrap();
        fs::remove_file(&media).unwrap();
        symlink("missing", &media).unwrap();
        assert_eq!(
            inspect(&scene, std::slice::from_ref(&mapping)).status,
            StashFileStatus::UnavailableSource
        );
        symlink(temp.path(), temp.path().join("alias")).unwrap();
        assert_eq!(
            inspect(&self::scene("/server/media/alias/video.mp4"), &[mapping]).status,
            StashFileStatus::UnsafePath
        );
    }
    #[test]
    fn invalid_mappings_are_rejected_before_planning() {
        let discovery = StashDiscovery {
            scenes: vec![],
            reported_count: 0,
            pagination_complete: true,
        };
        for server in ["relative", "/server/../media", "/server//media"] {
            assert!(plan(
                discovery.clone(),
                &[StashRootMapping {
                    server_root: server.into(),
                    local_root: PathBuf::from("/tmp")
                }]
            )
            .is_err());
        }
        assert!(plan(discovery, &[]).is_err());
    }
}

/// A publication lease holds the selected directory and source descriptors.
/// Paths remain observations; the lease rechecks namespace and source witnesses
/// before/after publication. It never replaces a destination entry.
#[cfg(unix)]
pub(crate) struct Publication {
    directory: std::fs::File,
    source: std::fs::File,
    observed: std::fs::Metadata,
    source_name: std::ffi::CString,
    destination_name: std::ffi::CString,
    source_path: std::path::PathBuf,
}
#[cfg(unix)]
fn io_failure(message: &str) -> std::io::Error {
    std::io::Error::other(message)
}
#[cfg(unix)]
fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.len() == b.len()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}
#[cfg(unix)]
impl Publication {
    pub(crate) fn prepare(capture: &StashSourceCapture) -> Result<Self, ProtocolError> {
        use sha2::{Digest, Sha256};
        use std::{
            ffi::CString,
            fs::File,
            io::Read,
            os::{
                fd::{AsRawFd, FromRawFd},
                unix::ffi::OsStrExt,
            },
        };
        let mismatch = || {
            ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "Stash source or namespace changed since capture",
            )
        };
        if capture.script_destination != capture.local_source.with_extension("funscript")
            || capture.script_destination == capture.local_source
            || capture.identity.sha256 != capture.source_version.as_str()
        {
            return Err(mismatch());
        }
        let directory = open_parent(&capture.local_source).map_err(|_| mismatch())?;
        let source_name = CString::new(
            capture
                .local_source
                .file_name()
                .ok_or_else(mismatch)?
                .as_bytes(),
        )
        .map_err(|_| mismatch())?;
        let destination_name = CString::new(
            capture
                .script_destination
                .file_name()
                .ok_or_else(mismatch)?
                .as_bytes(),
        )
        .map_err(|_| mismatch())?;
        let raw = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                source_name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if raw < 0 {
            return Err(mismatch());
        }
        let mut source = unsafe { File::from_raw_fd(raw) };
        let observed = source.metadata().map_err(|_| mismatch())?;
        if !observed.is_file() || observed.len() != capture.identity.byte_len {
            return Err(mismatch());
        }
        let mut hash = Sha256::new();
        let mut total = 0_u64;
        let mut buffer = [0_u8; 65536];
        loop {
            let n = source.read(&mut buffer).map_err(|_| mismatch())?;
            if n == 0 {
                break;
            }
            total = total.checked_add(n as u64).ok_or_else(mismatch)?;
            if total > capture.identity.byte_len {
                return Err(mismatch());
            }
            hash.update(&buffer[..n]);
        }
        if total != capture.identity.byte_len
            || format!("{:x}", hash.finalize()) != capture.identity.sha256
            || !same_file(&observed, &source.metadata().map_err(|_| mismatch())?)
        {
            return Err(mismatch());
        }
        let result = Self {
            directory,
            source,
            observed,
            source_name,
            destination_name,
            source_path: capture.local_source.clone(),
        };
        result.verify().map_err(|_| mismatch())?;
        result.absent().map_err(|_| {
            ProtocolError::new(
                ErrorCode::RequestConflict,
                "Stash sibling script already exists or cannot be inspected",
            )
        })?;
        Ok(result)
    }
    fn verify(&self) -> std::io::Result<()> {
        use std::os::unix::fs::MetadataExt;
        use std::{
            fs::File,
            os::fd::{AsRawFd, FromRawFd},
        };
        let current = open_parent(&self.source_path)
            .map_err(|_| io_failure("Stash source directory namespace changed"))?;
        let a = current.metadata()?;
        let b = self.directory.metadata()?;
        if a.dev() != b.dev() || a.ino() != b.ino() {
            return Err(io_failure("Stash source directory namespace changed"));
        }
        let raw = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                self.source_name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if raw < 0 {
            return Err(io_failure("Stash selected source is no longer present"));
        }
        let selected = unsafe { File::from_raw_fd(raw) };
        if !same_file(&self.observed, &self.source.metadata()?)
            || !same_file(&self.observed, &selected.metadata()?)
        {
            return Err(io_failure("Stash selected source changed"));
        }
        Ok(())
    }
    fn absent(&self) -> std::io::Result<()> {
        use std::os::fd::AsRawFd;
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                self.directory.as_raw_fd(),
                self.destination_name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0
        {
            return Err(io_failure("Stash sibling script already exists"));
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ENOENT) {
            return Err(error);
        }
        Ok(())
    }
    pub(crate) fn available_space(&self) -> std::io::Result<u64> {
        use std::os::fd::AsRawFd;
        let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        if unsafe { libc::fstatvfs(self.directory.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let stat = unsafe { stat.assume_init() };
        // statvfs field widths differ across Unix targets; widen before multiplication.
        #[allow(clippy::unnecessary_cast)]
        let available_blocks = stat.f_bavail as u64;
        #[allow(clippy::unnecessary_cast)]
        let fragment_size = stat.f_frsize as u64;
        available_blocks
            .checked_mul(fragment_size)
            .ok_or_else(|| io_failure("Stash free-space observation overflow"))
    }
    pub(crate) fn publish(&self, bytes: &[u8]) -> std::io::Result<()> {
        use std::{
            ffi::CString,
            fs::File,
            io::Write,
            os::fd::{AsRawFd, FromRawFd},
        };
        self.verify()?;
        self.absent()?;
        let temporary = CString::new(format!(".pulsar-export-{}", uuid::Uuid::new_v4()))
            .expect("UUID filename");
        let raw = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                temporary.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if raw < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut output = unsafe { File::from_raw_fd(raw) };
        struct Stage<'a> {
            directory: &'a File,
            name: &'a CString,
        }
        impl Drop for Stage<'_> {
            fn drop(&mut self) {
                unsafe {
                    libc::unlinkat(self.directory.as_raw_fd(), self.name.as_ptr(), 0);
                }
            }
        }
        let _stage = Stage {
            directory: &self.directory,
            name: &temporary,
        };
        output.write_all(bytes)?;
        output.sync_all()?;
        self.verify()?;
        if unsafe {
            libc::linkat(
                self.directory.as_raw_fd(),
                temporary.as_ptr(),
                self.directory.as_raw_fd(),
                self.destination_name.as_ptr(),
                0,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error());
        }
        if unsafe { libc::unlinkat(self.directory.as_raw_fd(), temporary.as_ptr(), 0) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        self.directory.sync_all()?;
        self.verify()?;
        let published = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                self.destination_name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if published < 0 {
            return Err(io_failure("published Stash script disappeared"));
        }
        let mut published = unsafe { File::from_raw_fd(published) };
        use std::os::unix::fs::MetadataExt;
        let actual = published.metadata()?;
        let expected = output.metadata()?;
        if actual.dev() != expected.dev()
            || actual.ino() != expected.ino()
            || actual.len() != bytes.len() as u64
        {
            return Err(io_failure("published Stash script identity changed"));
        }
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let mut hash = Sha256::new();
        let mut buffer = [0_u8; 65536];
        let mut count = 0_usize;
        loop {
            let n = published.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            count = count
                .checked_add(n)
                .ok_or_else(|| io_failure("published Stash script size overflow"))?;
            if count > bytes.len() {
                return Err(io_failure("published Stash script grew"));
            }
            hash.update(&buffer[..n]);
        }
        if count != bytes.len()
            || hash.finalize() != Sha256::digest(bytes)
            || !same_file(&actual, &published.metadata()?)
        {
            return Err(io_failure("published Stash script bytes changed"));
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod publication_tests {
    use super::*;
    use std::{fs, path::PathBuf};
    fn setup() -> (tempfile::TempDir, StashSourceCapture) {
        use sha2::{Digest, Sha256};
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("video.mp4");
        fs::write(&source, b"selected source").unwrap();
        let hash = format!("{:x}", Sha256::digest(b"selected source"));
        let capture = StashSourceCapture {
            endpoint: None,
            source_version: SourceVersionId::new(hash.clone()).unwrap(),
            identity: ArtifactIdentity {
                artifact_id: ArtifactId::new(hash.clone()).unwrap(),
                sha256: hash,
                byte_len: 15,
            },
            scene_id: "scene".into(),
            file_id: "file".into(),
            server_path: "/server/video.mp4".into(),
            local_source: source.clone(),
            script_destination: source.with_extension("funscript"),
        };
        (root, capture)
    }
    #[test]
    fn publication_is_atomic_no_clobber_and_binds_source_content() {
        let (_root, capture) = setup();
        let lease = Publication::prepare(&capture).unwrap();
        assert!(lease.available_space().unwrap() > 0);
        fs::write(&capture.script_destination, b"concurrent existing script").unwrap();
        assert!(lease.publish(b"new script").is_err());
        assert_eq!(
            fs::read(&capture.script_destination).unwrap(),
            b"concurrent existing script"
        );
        fs::remove_file(&capture.script_destination).unwrap();
        let lease = Publication::prepare(&capture).unwrap();
        lease.publish(b"new script").unwrap();
        assert_eq!(
            fs::read(&capture.script_destination).unwrap(),
            b"new script"
        );
        assert!(Publication::prepare(&capture).is_err());
        fs::remove_file(&capture.script_destination).unwrap();
        fs::write(&capture.local_source, b"modified source").unwrap();
        assert!(Publication::prepare(&capture).is_err());
    }
    #[test]
    fn publication_rejects_replaced_source_and_directory_namespace() {
        let (root, capture) = setup();
        let lease = Publication::prepare(&capture).unwrap();
        fs::rename(&capture.local_source, root.path().join("old.mp4")).unwrap();
        fs::write(&capture.local_source, b"selected source").unwrap();
        assert!(lease.publish(b"script").is_err());
        assert!(!capture.script_destination.exists());
        let outer = tempfile::tempdir().unwrap();
        let old = root.path().to_path_buf();
        let moved = outer.path().join("moved");
        let fresh = Publication::prepare(&capture).unwrap();
        fs::rename(&old, &moved).unwrap();
        fs::create_dir(&old).unwrap();
        assert!(fresh.publish(b"script").is_err());
        assert!(!capture.script_destination.exists());
        assert!(!moved.join("video.funscript").exists());
        fs::remove_dir(&old).unwrap();
        fs::rename(moved, &old).unwrap();
    }
    #[test]
    fn publication_refuses_dangling_script_and_symlink_directories() {
        use std::os::unix::fs::symlink;
        let (root, capture) = setup();
        symlink("missing", &capture.script_destination).unwrap();
        assert!(Publication::prepare(&capture).is_err());
        fs::remove_file(&capture.script_destination).unwrap();
        let alias = root.path().join("alias");
        symlink(root.path(), &alias).unwrap();
        let mut linked = capture.clone();
        linked.local_source = alias.join("video.mp4");
        linked.script_destination = alias.join("video.funscript");
        assert!(Publication::prepare(&linked).is_err());
        linked.script_destination = PathBuf::from("/arbitrary.funscript");
        assert!(Publication::prepare(&linked).is_err());
    }
}

/// Independently verify the exact source and published sibling before scan/link.
pub(crate) fn verify_published(publication: &StashScriptExport) -> Result<(), ProtocolError> {
    #[cfg(not(unix))]
    {
        let _ = publication;
        Err(ProtocolError::unsupported(
            "Stash scan requires descriptor-relative verification",
        ))
    }
    #[cfg(unix)]
    {
        use sha2::{Digest, Sha256};
        use std::{
            ffi::CString,
            fs::File,
            io::Read,
            os::{
                fd::{AsRawFd, FromRawFd},
                unix::ffi::OsStrExt,
            },
        };
        let mismatch = || {
            ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "published Stash source/script bytes or namespace changed",
            )
        };
        if publication.path != publication.source.script_destination
            || publication.path != publication.source.local_source.with_extension("funscript")
            || publication.path == publication.source.local_source
        {
            return Err(mismatch());
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        for (path, identity) in [
            (
                &publication.source.local_source,
                &publication.source.identity,
            ),
            (&publication.path, &publication.identity),
        ] {
            let directory = open_parent(path).map_err(|_| mismatch())?;
            let name = CString::new(path.file_name().ok_or_else(mismatch)?.as_bytes())
                .map_err(|_| mismatch())?;
            let raw = unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
            };
            if raw < 0 {
                return Err(mismatch());
            }
            let mut file = unsafe { File::from_raw_fd(raw) };
            let before = file.metadata().map_err(|_| mismatch())?;
            if !before.is_file() || before.len() != identity.byte_len {
                return Err(mismatch());
            }
            let mut hash = Sha256::new();
            let mut count = 0u64;
            let mut buffer = [0u8; 65536];
            loop {
                if std::time::Instant::now() >= deadline {
                    return Err(ProtocolError::new(
                        ErrorCode::Unavailable,
                        "Stash published identity verification deadline expired",
                    ));
                }
                let n = file.read(&mut buffer).map_err(|_| mismatch())?;
                if n == 0 {
                    break;
                }
                count = count.checked_add(n as u64).ok_or_else(mismatch)?;
                if count > identity.byte_len {
                    return Err(mismatch());
                }
                hash.update(&buffer[..n]);
            }
            if count != identity.byte_len
                || format!("{:x}", hash.finalize()) != identity.sha256
                || !same_file(&before, &file.metadata().map_err(|_| mismatch())?)
            {
                return Err(mismatch());
            }
            let current_directory = open_parent(path).map_err(|_| mismatch())?;
            use std::os::unix::fs::MetadataExt;
            let original = directory.metadata().map_err(|_| mismatch())?;
            let current = current_directory.metadata().map_err(|_| mismatch())?;
            if original.dev() != current.dev() || original.ino() != current.ino() {
                return Err(mismatch());
            }
            let selected_raw = unsafe {
                libc::openat(
                    current_directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
            };
            if selected_raw < 0 {
                return Err(mismatch());
            }
            let selected = unsafe { File::from_raw_fd(selected_raw) };
            if !same_file(&before, &selected.metadata().map_err(|_| mismatch())?) {
                return Err(mismatch());
            }
        }
        Ok(())
    }
}
