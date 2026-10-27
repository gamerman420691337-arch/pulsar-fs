//! Explicit offline host installation; no Engine authority, loading or activation.
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    ffi::CString,
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        },
    },
    path::{Component, Path},
    time::{Duration, Instant},
};

#[path = "runtime_install_recovery.rs"]
mod recovery;
pub use recovery::{
    discard_runtime_stage, inspect_runtime_stage, RuntimeStageInspection, RuntimeStageState,
};

#[path = "runtime_archive_recovery.rs"]
mod archive_recovery;
pub use archive_recovery::{
    discard_runtime_archive_stage, inspect_runtime_archive_stage, RuntimeArchiveStageInspection,
};

#[path = "runtime_archive.rs"]
mod archive;
pub use archive::pack_runtime_candidate;
#[path = "runtime_first_use.rs"]
mod first_use;
pub use first_use::{
    inspect_cached_generation_packs, prepare_cached_runtime, prepare_runtime_launch,
    PreparedRuntime,
};
#[path = "runtime_generation.rs"]
mod generation;
pub use generation::BundledGenerationReport;

#[path = "runtime_delta.rs"]
mod delta;
pub use delta::pack_runtime_update;

#[path = "runtime_update.rs"]
mod update;
pub use update::{update_runtime_candidate, RuntimeUpdateReport};

const BASE: [&str; 8] = [
    "pulsar",
    "pulsar-worker",
    "runtimes/ffmpeg",
    "runtimes/ffprobe",
    "media-source/COPYING.LGPLv2.1",
    "media-source/LICENSE.md",
    "media-source/ffmpeg-9.0.2.tar.xz",
    "media-source/configure.log",
];
const ASSISTANT: [&str; 4] = [
    "assistant/llama-completion",
    "assistant/model.gguf",
    "assistant/LLAMA-CPP-LICENSE",
    "assistant/MODEL-LICENSE",
];
const CPU: [&str; 3] = [
    "runtimes/libonnxruntime.so.1",
    "runtime-source/ONNX-RUNTIME-LICENSE",
    "runtime-source/ONNX-RUNTIME-THIRD-PARTY-NOTICES",
];
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    path: String,
    byte_len: u64,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Assistant {
    runtime_path: String,
    model_path: String,
    host_approval_required: bool,
    elf_notes: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cpu {
    path: String,
    needed_system_libraries: Vec<String>,
    qualified: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u16,
    kind: String,
    qualified: bool,
    #[serde(default)]
    assistant: Option<Assistant>,
    #[serde(default)]
    cpu_runtime: Option<Cpu>,
    #[serde(default)]
    generation_packs: Vec<generation::GenerationPack>,
    limitations: Vec<String>,
    files: Vec<Record>,
}
impl Manifest {
    fn validate(&self) -> Result<u64> {
        ensure!(
            self.schema_version == 1
                && self.kind == "linux-x86-64-runtime-candidate"
                && !self.qualified,
            "unsupported or qualified runtime candidate manifest"
        );
        ensure!(
            !self.limitations.is_empty()
                && self.limitations.len() <= 16
                && self.limitations.iter().all(|s| s.len() <= 512),
            "invalid candidate limitations"
        );
        let mut expected = BASE
            .iter()
            .map(|s| (*s).to_owned())
            .collect::<BTreeSet<_>>();
        if let Some(a) = &self.assistant {
            ensure!(
                a.runtime_path == ASSISTANT[0]
                    && a.model_path == ASSISTANT[1]
                    && a.host_approval_required
                    && a.elf_notes.len() <= 32
                    && a.elf_notes.iter().all(|s| s.len() <= 512),
                "invalid assistant candidate declaration"
            );
            expected.extend(ASSISTANT.map(str::to_owned));
        }
        if let Some(c) = &self.cpu_runtime {
            let baseline = [
                "libdl.so.2",
                "librt.so.1",
                "libpthread.so.0",
                "libstdc++.so.6",
                "libm.so.6",
                "libgcc_s.so.1",
                "libc.so.6",
                "ld-linux-x86-64.so.2",
            ];
            ensure!(
                c.path == CPU[0]
                    && !c.qualified
                    && c.needed_system_libraries.len() <= 8
                    && c.needed_system_libraries
                        .iter()
                        .all(|s| baseline.contains(&s.as_str())),
                "invalid CPU candidate declaration"
            );
            expected.extend(CPU.map(str::to_owned));
        }
        let generation_assets = generation::inventory(&self.generation_packs, &mut expected)?;
        ensure!(
            self.files.len() == expected.len(),
            "candidate file membership differs"
        );
        let mut seen = BTreeSet::new();
        let mut total = 0u64;
        for r in &self.files {
            ensure!(
                expected.contains(r.path.as_str()) && seen.insert(&r.path),
                "unknown or duplicate candidate file"
            );
            if let Some((byte_len, sha256)) = generation_assets.get(&r.path) {
                ensure!(
                    byte_len.is_none_or(|n| n == r.byte_len) && sha256 == &r.sha256,
                    "generation payload identity differs from pack declaration"
                );
                if byte_len.is_none() {
                    ensure!(
                        r.byte_len <= pulsar_protocol::PACK_MANIFEST_LIMIT as u64,
                        "generation manifest exceeds admission"
                    );
                }
            }
            let limit = if r.path == ASSISTANT[1] {
                32 * 1024 * 1024 * 1024
            } else {
                1024 * 1024 * 1024
            };
            ensure!(
                r.byte_len > 0
                    && r.byte_len <= limit
                    && r.sha256.len() == 64
                    && r.sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "invalid candidate file identity"
            );
            total = total
                .checked_add(r.byte_len)
                .context("candidate size overflow")?;
        }
        ensure!(
            total <= 64 * 1024 * 1024 * 1024,
            "candidate exceeds installation admission"
        );
        Ok(total)
    }
}
fn child(parent: &File, name: &std::ffi::OsStr, directory: bool) -> Result<File> {
    let name = CString::new(name.as_bytes())?;
    let flags = libc::O_RDONLY
        | libc::O_CLOEXEC
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | if directory { libc::O_DIRECTORY } else { 0 };
    // SAFETY: held parent and one NUL-terminated component; successful fd uniquely owned.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error()).context("cannot open candidate component");
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn directory(path: &Path) -> Result<File> {
    ensure!(path.is_absolute(), "installation paths must be absolute");
    let mut current = File::open("/")?;
    for c in path.components() {
        match c {
            Component::RootDir => {}
            Component::Normal(n) => current = child(&current, n, true)?,
            _ => anyhow::bail!("installation path must contain normal components"),
        }
    }
    Ok(current)
}
fn source_file(root: &File, path: &str) -> Result<File> {
    let mut current = root.try_clone()?;
    let mut parts = Path::new(path).components().peekable();
    while let Some(c) = parts.next() {
        let Component::Normal(n) = c else {
            anyhow::bail!("invalid candidate asset path")
        };
        current = child(&current, n, parts.peek().is_some())?;
    }
    ensure!(
        current.metadata()?.is_file(),
        "candidate asset is not regular"
    );
    Ok(current)
}
fn stable(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    a.len() == b.len()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}
fn manifest(root: &File) -> Result<(Vec<u8>, Manifest)> {
    let mut file = source_file(root, "runtime-candidate.json")?;
    let before = file.metadata()?;
    ensure!(
        before.len() > 0 && before.len() <= 65536,
        "candidate manifest exceeds capture admission"
    );
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(65537)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 65536
            && bytes.len() as u64 == before.len()
            && stable(&before, &file.metadata()?),
        "candidate manifest changed during capture"
    );
    let manifest = serde_json::from_slice(&bytes)?;
    Ok((bytes, manifest))
}
fn executable(path: &str) -> bool {
    matches!(
        path,
        "pulsar"
            | "pulsar-worker"
            | "runtimes/ffmpeg"
            | "runtimes/ffprobe"
            | "assistant/llama-completion"
    )
}
/// Verify and publish a new installation in an existing owned parent. Existing
/// destinations are never changed. Inference approval and model selection are separate.
pub fn install_runtime_candidate(source: &Path, destination: &Path) -> Result<String> {
    ensure!(
        cfg!(target_arch = "x86_64"),
        "runtime candidate installation requires Linux x86-64"
    );
    ensure!(
        source.is_absolute() && destination.is_absolute(),
        "installation paths must be absolute"
    );
    let (mut input, bytes, manifest) = archive::Input::open(source)?;
    publish_runtime_input(&mut input, bytes, manifest, destination)
}
fn publish_runtime_input(
    input: &mut archive::Input,
    bytes: Vec<u8>,
    manifest: Manifest,
    destination: &Path,
) -> Result<String> {
    let parent = directory(
        destination
            .parent()
            .context("installation parent missing")?,
    )?;
    let meta = parent.metadata()?;
    ensure!(
        meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o022 == 0,
        "installation parent must be owned and not writable by other users"
    );
    let name = destination
        .file_name()
        .context("installation destination basename missing")?;
    let anchored = std::path::PathBuf::from(format!("/proc/self/fd/{}", parent.as_raw_fd()));
    ensure!(
        !anchored.join(name).try_exists()?
            && std::fs::symlink_metadata(anchored.join(name))
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
        "installation destination already exists or is inaccessible"
    );
    let total = manifest.validate()?;
    ensure!(
        fs2::available_space(&anchored)? > total + 1024 * 1024,
        "insufficient installation space"
    );
    let deadline = Instant::now() + Duration::from_secs(600);
    let stage = tempfile::Builder::new()
        .prefix(".pulsar-runtime-stage-")
        .tempdir_in(&anchored)?;
    std::fs::set_permissions(stage.path(), std::fs::Permissions::from_mode(0o700))?;
    let stage_dir = File::open(stage.path())?;
    let _stage_marker = recovery::write_marker(stage.path(), &bytes)?;
    for r in &manifest.files {
        ensure!(
            Instant::now() < deadline,
            "runtime installation deadline exceeded"
        );
        let target = stage.path().join(&r.path);
        let target_parent = target.parent().context("asset parent missing")?;
        std::fs::create_dir_all(target_parent)?;
        for parent in target_parent.ancestors().take_while(|p| *p != stage.path()) {
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&target)?;
        output.set_permissions(std::fs::Permissions::from_mode(if executable(&r.path) {
            0o755
        } else {
            0o644
        }))?;
        input.copy_asset(r, &mut output, deadline)?;
        output.sync_all()?;
    }
    input.finish()?;
    generation::verify(&stage_dir, &manifest.generation_packs)?;
    let mut captured = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(stage.path().join("runtime-candidate.json"))?;
    captured.set_permissions(std::fs::Permissions::from_mode(0o644))?;
    captured.write_all(&bytes)?;
    captured.sync_all()?;
    for name in generation::directories(&manifest.files).into_iter().rev() {
        File::open(stage.path().join(name))?.sync_all()?;
    }
    stage_dir.sync_all()?;
    ensure!(
        Instant::now() < deadline,
        "runtime installation deadline exceeded before publication"
    );
    let from = CString::new(
        stage
            .path()
            .file_name()
            .context("stage basename missing")?
            .as_bytes(),
    )?;
    let to = CString::new(name.as_bytes())?;
    // SAFETY: held parent descriptor, engine-created stage, destination single component;
    // RENAME_NOREPLACE atomically refuses an existing or concurrently created destination.
    let renamed = unsafe {
        libc::renameat2(
            parent.as_raw_fd(),
            from.as_ptr(),
            parent.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    ensure!(
        renamed == 0,
        "cannot publish runtime installation: {}",
        std::io::Error::last_os_error()
    );
    parent.sync_all().context(
        "installation published but parent sync failed; inspect destination before retry",
    )?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn fixture(root: &Path) -> std::path::PathBuf {
        let source = root.join("source");
        std::fs::create_dir(&source).unwrap();
        let mut records = Vec::new();
        for name in BASE {
            let path = source.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let bytes = format!("authored installation fixture {name}").into_bytes();
            std::fs::write(path, &bytes).unwrap();
            records.push(serde_json::json!({"path":name,"byte_len":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}));
        }
        let manifest = serde_json::json!({"schema_version":1,"kind":"linux-x86-64-runtime-candidate","qualified":false,"assistant":null,"cpu_runtime":null,"limitations":["authored integrity fixture; no executable qualification"],"files":records});
        std::fs::write(
            source.join("runtime-candidate.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        source
    }
    fn no_stage(root: &Path) {
        assert!(std::fs::read_dir(root).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".pulsar-runtime-stage-")));
    }
    fn generation_fixture(source: &Path) -> String {
        let mut outer: serde_json::Value =
            serde_json::from_slice(&std::fs::read(source.join("runtime-candidate.json")).unwrap())
                .unwrap();
        let assets = [
            ("models/deep/model.onnx", "model"),
            ("runtime.bin", "runtime"),
            ("profile.json", "profile"),
            ("LICENSE", "license"),
        ];
        let pack = serde_json::json!({"schema_version":1,"pack_id":"authored-inert-delivery",
            "target_os":"linux","target_arch":"x86_64","ort_api_version":24,"labels":["fixture"],
            "assets":assets.iter().map(|(path,role)|serde_json::json!({"path":path,"role":role,
                "byte_len":1,"sha256":format!("{:x}",Sha256::digest(b"x"))})).collect::<Vec<_>>(),
            "license_path":"LICENSE","source_description":"Authored inert byte fixture"});
        let bytes = serde_json::to_vec(&pack).unwrap();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let prefix = format!("generation/{hash}/");
        let mut files = assets
            .iter()
            .map(|(p, _)| (format!("{prefix}{p}"), b"x".to_vec()))
            .collect::<Vec<_>>();
        files.push((format!("{prefix}manifest.json"), bytes));
        for (name, bytes) in files {
            let path = source.join(&name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, &bytes).unwrap();
            outer["files"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({"path":name,
                "byte_len":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}));
        }
        outer["generation_packs"] = serde_json::json!([{"manifest_sha256":hash,"manifest":pack,
            "native_execution_approval_required":true,"qualified":false}]);
        std::fs::write(
            source.join("runtime-candidate.json"),
            serde_json::to_vec(&outer).unwrap(),
        )
        .unwrap();
        hash
    }
    #[test]
    fn generation_bundle_archive_cache_and_nested_inventory_preserve_authority() {
        let root = tempfile::tempdir().unwrap();
        let source = fixture(root.path());
        let pack = generation_fixture(&source);
        let archive = root.path().join("runtime.pack");
        pack_runtime_candidate(&source, &archive).unwrap();
        let cache = root.path().join("cache");
        let installed = prepare_runtime_launch(&archive, &cache).unwrap();
        let nested = installed
            .directory
            .join(format!("generation/{pack}/models/deep"));
        assert_eq!(std::fs::read(nested.join("model.onnx")).unwrap(), b"x");
        for dir in nested.ancestors().take_while(|p| *p != installed.directory) {
            assert_eq!(std::fs::metadata(dir).unwrap().mode() & 0o777, 0o700);
        }
        assert!(
            prepare_cached_runtime(&cache, &installed.manifest_sha256)
                .unwrap()
                .reused
        );
        let discovery: serde_json::Value = serde_json::from_str(
            &inspect_cached_generation_packs(&cache, &installed.manifest_sha256)
                .unwrap()
                .json()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(discovery["packs"][0]["manifest_sha256"], pack);
        assert_eq!(discovery["integrity_verified"], true);
        for flag in ["native_execution_approved", "rights_verified", "qualified"] {
            assert_eq!(discovery[flag], false);
        }
        std::fs::write(nested.join("owner-file"), b"keep").unwrap();
        assert!(prepare_cached_runtime(&cache, &installed.manifest_sha256).is_err());
        assert!(inspect_cached_generation_packs(&cache, &installed.manifest_sha256).is_err());
        assert_eq!(std::fs::read(nested.join("owner-file")).unwrap(), b"keep");
        no_stage(root.path());
    }
    #[test]
    fn contradictory_embedded_generation_manifest_never_publishes() {
        let root = tempfile::tempdir().unwrap();
        let source = fixture(root.path());
        generation_fixture(&source);
        let path = source.join("runtime-candidate.json");
        let original: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for kind in ["labels", "authority", "identity"] {
            let mut outer = original.clone();
            match kind {
                "labels" => {
                    outer["generation_packs"][0]["manifest"]["labels"] =
                        serde_json::json!(["different"])
                }
                "authority" => outer["generation_packs"][0]["qualified"] = serde_json::json!(true),
                _ => {
                    outer["generation_packs"][0]["manifest"]["assets"][0]["sha256"] =
                        serde_json::json!("0".repeat(64))
                }
            }
            std::fs::write(&path, serde_json::to_vec(&outer).unwrap()).unwrap();
            let destination = root.path().join(kind);
            assert!(install_runtime_candidate(&source, &destination).is_err());
            assert!(!destination.exists());
            let archive = root.path().join(format!("{kind}.pack"));
            assert!(pack_runtime_candidate(&source, &archive).is_err());
            assert!(!archive.exists());
            no_stage(root.path());
        }
    }
    #[test]
    fn abandoned_nested_generation_stage_requires_complete_preflight_before_discard() {
        let root = tempfile::tempdir().unwrap();
        let source = fixture(root.path());
        let pack = generation_fixture(&source);
        let bytes = std::fs::read(source.join("runtime-candidate.json")).unwrap();
        let stage = root.path().join(".pulsar-runtime-stage-authored");
        std::fs::create_dir(&stage).unwrap();
        std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o700)).unwrap();
        drop(recovery::write_marker(&stage, &bytes).unwrap());
        let nested = stage.join(format!("generation/{pack}/models/deep"));
        std::fs::create_dir_all(&nested).unwrap();
        for path in nested.ancestors().take_while(|p| *p != stage) {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let model = nested.join("model.onnx");
        std::fs::write(&model, b"x").unwrap();
        std::fs::set_permissions(&model, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            inspect_runtime_stage(&stage).unwrap().present_files,
            Some(1)
        );
        let unknown = nested.join("owner-file");
        std::fs::write(&unknown, b"preserve").unwrap();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        assert!(discard_runtime_stage(&stage, &hash, false).is_err());
        assert!(discard_runtime_stage(&stage, &hash, true).is_err());
        assert_eq!(std::fs::read(&model).unwrap(), b"x");
        assert_eq!(std::fs::read(&unknown).unwrap(), b"preserve");
        std::fs::remove_file(unknown).unwrap();
        discard_runtime_stage(&stage, &hash, true).unwrap();
        assert!(!stage.exists());
    }
    #[test]
    fn installation_preserves_manifest_and_bytes_and_refuses_existing_destination() {
        let root = tempfile::tempdir().unwrap();
        let source = fixture(root.path());
        let dest = root.path().join("installed");
        let manifest = std::fs::read(source.join("runtime-candidate.json")).unwrap();
        let hash = install_runtime_candidate(&source, &dest).unwrap();
        assert_eq!(hash, format!("{:x}", Sha256::digest(&manifest)));
        assert_eq!(
            std::fs::read(dest.join("runtime-candidate.json")).unwrap(),
            manifest
        );
        for name in BASE {
            assert_eq!(
                std::fs::read(source.join(name)).unwrap(),
                std::fs::read(dest.join(name)).unwrap()
            );
            assert_eq!(
                std::fs::metadata(dest.join(name)).unwrap().mode() & 0o777,
                if executable(name) { 0o755 } else { 0o644 }
            );
        }
        std::fs::write(dest.join("owner-file"), b"preserve").unwrap();
        assert!(install_runtime_candidate(&source, &dest).is_err());
        assert_eq!(std::fs::read(dest.join("owner-file")).unwrap(), b"preserve");
        no_stage(root.path());
    }
    #[test]
    fn late_integrity_failure_never_publishes_partial_installation() {
        let root = tempfile::tempdir().unwrap();
        let source = fixture(root.path());
        let dest = root.path().join("installed");
        let path = source.join(BASE[7]);
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, vec![b'x'; bytes.len()]).unwrap();
        assert!(install_runtime_candidate(&source, &dest)
            .unwrap_err()
            .to_string()
            .contains("integrity differs"));
        assert!(!dest.exists());
        no_stage(root.path());
    }
    #[test]
    fn unknown_traversal_duplicate_and_qualified_manifests_refuse() {
        let root = tempfile::tempdir().unwrap();
        let source = fixture(root.path());
        let dest = root.path().join("installed");
        let path = source.join("runtime-candidate.json");
        let original: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for kind in ["traversal", "duplicate", "qualified", "unknown"] {
            let mut value = original.clone();
            match kind {
                "traversal" => value["files"][0]["path"] = "../escape".into(),
                "duplicate" => value["files"][0] = value["files"][1].clone(),
                "qualified" => value["qualified"] = true.into(),
                _ => value["unknown-authority"] = true.into(),
            }
            std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(install_runtime_candidate(&source, &dest).is_err(), "{kind}");
            assert!(!dest.exists());
            no_stage(root.path());
        }
    }
    #[test]
    fn symlink_source_and_writable_parent_refuse() {
        let root = tempfile::tempdir().unwrap();
        let source = fixture(root.path());
        let dest = root.path().join("installed");
        let asset = source.join(BASE[0]);
        let bytes = std::fs::read(&asset).unwrap();
        std::fs::remove_file(&asset).unwrap();
        std::fs::write(root.path().join("external"), bytes).unwrap();
        std::os::unix::fs::symlink(root.path().join("external"), asset).unwrap();
        assert!(install_runtime_candidate(&source, &dest).is_err());
        assert!(!dest.exists());
        no_stage(root.path());
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(install_runtime_candidate(&source, &dest)
            .unwrap_err()
            .to_string()
            .contains("parent must be owned"));
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}
