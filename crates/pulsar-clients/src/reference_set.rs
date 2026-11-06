//! Local reference declarations. These files confer no project, training,
//! alignment, native-plan, model-quality or physical-device authority.
use anyhow::Result;
use pulsar_protocol::{AnnotatedGenerationView, ReferenceCurve};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub(crate) enum ReferenceViewInput {
    Declared(AnnotatedGenerationView),
    /// Absolute sidecars may live outside the input root. Relative sidecars
    /// resolve beneath it; every sidecar must be a bounded regular file.
    File(PathBuf),
}

#[derive(Clone, Debug)]
pub(crate) struct ReferenceEntryInput {
    pub id: String,
    pub group: String,
    pub video: PathBuf,
    pub reference: PathBuf,
    pub view: ReferenceViewInput,
    pub offset_ms: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct ReferenceEntry {
    pub id: String,
    pub group: String,
    pub input_root: PathBuf,
    pub video: PathBuf,
    pub reference: PathBuf,
    pub view: AnnotatedGenerationView,
    pub curve: Arc<ReferenceCurve>,
    pub offset_ms: i64,
    pub source_sha256: String,
    pub reference_sha256: String,
    binding: platform::EntryBinding,
}

/// Hashing and file work must run outside the UI thread.
pub(crate) fn load_entry(root: &Path, input: ReferenceEntryInput) -> Result<ReferenceEntry> {
    platform::load_entry(root, input)
}

/// Reopen an exported declaration set, including current source/reference
/// correspondence. No training, evaluation or alignment check is performed.
pub(crate) fn load_set(directory: &Path) -> Result<(String, Vec<ReferenceEntry>)> {
    platform::load_set(directory)
}

/// Publish only declaration files into a new directory; never overwrite or
/// copy media, dispatch generation, or mutate a project master.
pub(crate) fn export_new(
    destination: &Path,
    authorization: &str,
    entries: &[ReferenceEntry],
) -> Result<PathBuf> {
    platform::export_new(destination, authorization, entries)
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use super::*;
    use anyhow::bail;
    #[derive(Clone, Debug)]
    pub(super) struct EntryBinding;
    pub(super) fn load_entry(_: &Path, _: ReferenceEntryInput) -> Result<ReferenceEntry> {
        bail!("Descriptor-bound reference intake is currently implemented only on Linux")
    }
    pub(super) fn load_set(_: &Path) -> Result<(String, Vec<ReferenceEntry>)> {
        bail!("Descriptor-bound reference intake is currently implemented only on Linux")
    }
    pub(super) fn export_new(_: &Path, _: &str, _: &[ReferenceEntry]) -> Result<PathBuf> {
        bail!("Atomic no-replace reference publication is unavailable on this platform")
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use anyhow::{bail, ensure, Context};
    use pulsar_protocol::{AnnotatedTemporalView, ReferencePoint};
    use rustix::fs::{open, openat, statat, AtFlags, Mode, OFlags};
    use serde::{
        de::{MapAccess, SeqAccess, Visitor},
        Deserialize, Deserializer, Serialize,
    };
    use serde_json::{Map, Value};
    use sha2::{Digest, Sha256};
    use std::{
        collections::{BTreeMap, BTreeSet},
        ffi::OsString,
        fs::{self, File, Metadata},
        io::{Read, Seek, SeekFrom, Write},
        os::unix::fs::MetadataExt,
        path::Component,
        time::{Duration, Instant},
    };

    const JSON_LIMIT: u64 = 8 * 1024 * 1024;
    const VIEW_LIMIT: u64 = 4096;
    const SOURCE_LIMIT: u64 = 16 * 1024 * 1024 * 1024;
    const SOURCE_TOTAL_LIMIT: u64 = 64 * 1024 * 1024 * 1024;
    const OPERATION_LIMIT: Duration = Duration::from_secs(300);
    const SCOPE: &str = "Authored source/script/view and offset declarations only; no training, evaluation, native-plan or human alignment verification";

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Fingerprint {
        dev: u64,
        ino: u64,
        len: u64,
        modified: (i64, i64),
        changed: (i64, i64),
    }
    impl Fingerprint {
        fn new(metadata: &Metadata) -> Self {
            Self {
                dev: metadata.dev(),
                ino: metadata.ino(),
                len: metadata.len(),
                modified: (metadata.mtime(), metadata.mtime_nsec()),
                changed: (metadata.ctime(), metadata.ctime_nsec()),
            }
        }
    }
    #[derive(Clone, Debug)]
    struct FileBinding {
        path: PathBuf,
        fingerprint: Fingerprint,
        sha256: String,
    }
    #[derive(Clone, Debug)]
    pub(super) struct EntryBinding {
        root: PathBuf,
        root_id: (u64, u64),
        video: FileBinding,
        reference: FileBinding,
    }
    struct ReadRecord {
        binding: FileBinding,
        bytes: Vec<u8>,
    }

    fn check_time(deadline: Instant) -> Result<()> {
        ensure!(
            Instant::now() < deadline,
            "Reference file operation exceeded its elapsed budget"
        );
        Ok(())
    }
    fn no_parent(path: &Path) -> Result<()> {
        ensure!(
            !path.components().any(|c| matches!(c, Component::ParentDir)),
            "Parent traversal is not admitted"
        );
        ensure!(
            path.to_str()
                .is_some_and(|s| !s.is_empty() && s.len() <= 4096),
            "Reference path must be bounded UTF-8"
        );
        Ok(())
    }
    fn identifier(value: &str) -> Result<()> {
        ensure!(
            !value.is_empty()
                && value.len() <= 64
                && value.bytes().all(|b| b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b == b'_'
                    || b == b'-'),
            "Reference ID/group requires 1..64 lowercase letters, digits, underscores or hyphens"
        );
        Ok(())
    }
    fn offset(value: i64) -> Result<()> {
        ensure!(
            (-86_400_000..=86_400_000).contains(&value),
            "Reference offset exceeds signed integer millisecond admission"
        );
        Ok(())
    }
    fn authorization(value: &str) -> Result<()> {
        ensure!(
            !value.trim().is_empty() && value.chars().count() <= 4096,
            "Explicit authorization declaration must contain 1..4096 characters"
        );
        Ok(())
    }
    fn digest(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }
    fn json_bytes(value: &impl Serialize, limit: u64) -> Result<Vec<u8>> {
        let mut bytes = serde_json::to_vec_pretty(value)?;
        bytes.push(b'\n');
        ensure!(
            bytes.len() as u64 <= limit,
            "Reference document exceeds byte admission"
        );
        Ok(bytes)
    }

    struct Directory {
        path: PathBuf,
        file: File,
    }
    impl Directory {
        fn open(path: &Path, live_root: bool) -> Result<Self> {
            no_parent(path)?;
            let path = path.canonicalize().context("Resolve reference directory")?;
            no_parent(&path)?;
            if live_root {
                ensure!(path.file_name().is_some_and(|n| n == "Live-Reference"), "Only the explicitly declared Live-Reference root is admitted for training declarations");
            }
            let file = File::from(open(
                &path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            let result = Self { path, file };
            result.check_name()?;
            Ok(result)
        }
        fn id(&self) -> Result<(u64, u64)> {
            let metadata = self.file.metadata()?;
            Ok((metadata.dev(), metadata.ino()))
        }
        fn check_name(&self) -> Result<()> {
            let current = fs::symlink_metadata(&self.path)?;
            let held = self.file.metadata()?;
            ensure!(
                current.is_dir()
                    && !current.file_type().is_symlink()
                    && current.dev() == held.dev()
                    && current.ino() == held.ino(),
                "Reference directory was replaced or renamed"
            );
            Ok(())
        }
        fn relative(&self, path: &Path) -> Result<PathBuf> {
            no_parent(path)?;
            let relative = if path.is_absolute() {
                path.strip_prefix(&self.path)
                    .context("Input path escapes the declared reference root")?
            } else {
                path
            };
            let mut result = PathBuf::new();
            for component in relative.components() {
                match component {
                    Component::Normal(name) => result.push(name),
                    Component::CurDir => {}
                    _ => bail!("Input path escapes the declared reference root"),
                }
            }
            ensure!(
                !result.as_os_str().is_empty(),
                "Reference input must name a file"
            );
            Ok(result)
        }
        fn open_relative(&self, path: &Path) -> Result<File> {
            let relative = self.relative(path)?;
            ensure!(
                relative == path,
                "Expected a normalized contained relative path"
            );
            self.check_name()?;
            let mut parent = self.file.try_clone()?;
            let names: Vec<_> = relative.components().map(|c| c.as_os_str()).collect();
            for name in &names[..names.len() - 1] {
                parent = File::from(openat(
                    &parent,
                    *name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )?);
            }
            let file = File::from(openat(
                &parent,
                names[names.len() - 1],
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            ensure!(
                file.metadata()?.is_file(),
                "Reference input must be a regular file"
            );
            Ok(file)
        }
        fn confirm(&self, binding: &FileBinding, deadline: Instant) -> Result<()> {
            check_time(deadline)?;
            let file = self.open_relative(&binding.path)?;
            ensure!(
                Fingerprint::new(&file.metadata()?) == binding.fingerprint,
                "Reference input identity or metadata changed: {}",
                binding.path.display()
            );
            self.check_name()?;
            check_time(deadline)
        }
        fn read(
            &self,
            path: &Path,
            limit: u64,
            retain: bool,
            deadline: Instant,
        ) -> Result<ReadRecord> {
            check_time(deadline)?;
            let relative = self.relative(path)?;
            let mut file = self.open_relative(&relative)?;
            let before = Fingerprint::new(&file.metadata()?);
            ensure!(
                before.len > 0 && before.len <= limit,
                "Reference file is empty or exceeds byte admission"
            );
            let (bytes, sha256, length) = read_file(&mut file, limit, retain, deadline)?;
            ensure!(
                length == before.len && Fingerprint::new(&file.metadata()?) == before,
                "Reference file changed during its full read"
            );
            let binding = FileBinding {
                path: relative,
                fingerprint: before,
                sha256,
            };
            self.confirm(&binding, deadline)?;
            Ok(ReadRecord { binding, bytes })
        }
        fn recheck(
            &self,
            expected: &FileBinding,
            limit: u64,
            retain: bool,
            deadline: Instant,
        ) -> Result<ReadRecord> {
            let actual = self.read(&expected.path, limit, retain, deadline)?;
            ensure!(
                actual.binding.fingerprint == expected.fingerprint
                    && actual.binding.sha256 == expected.sha256,
                "Reference content or identity differs from the loaded entry: {}",
                expected.path.display()
            );
            Ok(actual)
        }
    }

    fn read_file(
        file: &mut File,
        limit: u64,
        retain: bool,
        deadline: Instant,
    ) -> Result<(Vec<u8>, String, u64)> {
        let mut hash = Sha256::new();
        let mut bytes = Vec::new();
        let mut length = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            check_time(deadline)?;
            let count = file.read(&mut buffer)?;
            check_time(deadline)?;
            if count == 0 {
                break;
            }
            length = length
                .checked_add(count as u64)
                .context("Reference byte count overflow")?;
            ensure!(length <= limit, "Reference file grew beyond byte admission");
            hash.update(&buffer[..count]);
            if retain {
                bytes.extend_from_slice(&buffer[..count]);
            }
        }
        Ok((bytes, format!("{:x}", hash.finalize()), length))
    }

    /// Preserve JSON value types while rejecting duplicate keys at every level.
    /// serde_json's normal depth limit and finite-number admission still apply.
    struct StrictValue(Value);
    impl<'de> Deserialize<'de> for StrictValue {
        fn deserialize<D: Deserializer<'de>>(
            deserializer: D,
        ) -> std::result::Result<Self, D::Error> {
            struct StrictVisitor;
            impl<'de> Visitor<'de> for StrictVisitor {
                type Value = StrictValue;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("JSON without duplicate keys or non-finite numbers")
                }
                fn visit_bool<E: serde::de::Error>(
                    self,
                    v: bool,
                ) -> std::result::Result<Self::Value, E> {
                    Ok(StrictValue(Value::Bool(v)))
                }
                fn visit_i64<E: serde::de::Error>(
                    self,
                    v: i64,
                ) -> std::result::Result<Self::Value, E> {
                    Ok(StrictValue(Value::Number(v.into())))
                }
                fn visit_u64<E: serde::de::Error>(
                    self,
                    v: u64,
                ) -> std::result::Result<Self::Value, E> {
                    Ok(StrictValue(Value::Number(v.into())))
                }
                fn visit_f64<E: serde::de::Error>(
                    self,
                    v: f64,
                ) -> std::result::Result<Self::Value, E> {
                    serde_json::Number::from_f64(v)
                        .map(|v| StrictValue(Value::Number(v)))
                        .ok_or_else(|| E::custom("non-finite JSON number"))
                }
                fn visit_str<E: serde::de::Error>(
                    self,
                    v: &str,
                ) -> std::result::Result<Self::Value, E> {
                    Ok(StrictValue(Value::String(v.to_owned())))
                }
                fn visit_string<E: serde::de::Error>(
                    self,
                    v: String,
                ) -> std::result::Result<Self::Value, E> {
                    Ok(StrictValue(Value::String(v)))
                }
                fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                    Ok(StrictValue(Value::Null))
                }
                fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                    self.visit_unit()
                }
                fn visit_seq<A: SeqAccess<'de>>(
                    self,
                    mut seq: A,
                ) -> std::result::Result<Self::Value, A::Error> {
                    let mut values = Vec::new();
                    while let Some(StrictValue(value)) = seq.next_element()? {
                        values.push(value);
                    }
                    Ok(StrictValue(Value::Array(values)))
                }
                fn visit_map<A: MapAccess<'de>>(
                    self,
                    mut map: A,
                ) -> std::result::Result<Self::Value, A::Error> {
                    let mut values = Map::new();
                    while let Some((key, StrictValue(value))) =
                        map.next_entry::<String, StrictValue>()?
                    {
                        if values.insert(key, value).is_some() {
                            return Err(serde::de::Error::custom("duplicate JSON field"));
                        }
                    }
                    Ok(StrictValue(Value::Object(values)))
                }
            }
            deserializer.deserialize_any(StrictVisitor)
        }
    }
    fn strict_json(bytes: &[u8]) -> Result<Value> {
        ensure!(
            bytes.len() as u64 <= JSON_LIMIT,
            "JSON exceeds reference admission"
        );
        Ok(serde_json::from_slice::<StrictValue>(bytes)
            .context("Parse strict reference JSON")?
            .0)
    }
    fn parse_reference(bytes: &[u8]) -> Result<ReferenceCurve> {
        let value = strict_json(bytes)?;
        let actions = value
            .get("actions")
            .and_then(Value::as_array)
            .context("Reference actions missing")?;
        ensure!(
            (2..=250_000).contains(&actions.len()),
            "Reference action count exceeds admission"
        );
        let points = actions
            .iter()
            .map(|action| {
                ensure!(action.is_object(), "Reference action must be an object");
                let at = action
                    .get("at")
                    .and_then(Value::as_u64)
                    .context("Reference timestamp must be a nonnegative integer")?;
                let pos = action
                    .get("pos")
                    .and_then(Value::as_u64)
                    .context("Reference position must be a nonnegative integer")?;
                ensure!(
                    at <= 86_400_000 && pos <= 100,
                    "Reference timestamp/position exceeds admission"
                );
                Ok(ReferencePoint {
                    at_ms: u32::try_from(at)?,
                    pos: u8::try_from(pos)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        // The intake contract consumes supplied at/pos, including when other
        // funscript metadata says inverted. No repair, sorting or inversion.
        Ok(ReferenceCurve::new(points)?)
    }
    fn parse_view(bytes: &[u8]) -> Result<AnnotatedGenerationView> {
        ensure!(
            bytes.len() as u64 <= VIEW_LIMIT,
            "Saved target view exceeds 4096 bytes"
        );
        let view: AnnotatedGenerationView = serde_json::from_value(strict_json(bytes)?)?;
        view.validate()?;
        Ok(view)
    }
    fn read_view(
        root: &Directory,
        path: &Path,
        deadline: Instant,
    ) -> Result<AnnotatedGenerationView> {
        no_parent(path)?;
        let absolute = if path.is_absolute() {
            path.to_owned()
        } else {
            root.path.join(path)
        };
        let parent = Directory::open(
            absolute.parent().context("Target view parent is missing")?,
            false,
        )?;
        let leaf = Path::new(
            absolute
                .file_name()
                .context("Target view filename is missing")?,
        );
        parse_view(&parent.read(leaf, VIEW_LIMIT, true, deadline)?.bytes)
    }

    pub(super) fn load_entry(root: &Path, input: ReferenceEntryInput) -> Result<ReferenceEntry> {
        let deadline = Instant::now() + OPERATION_LIMIT;
        load_entry_in(&Directory::open(root, true)?, input, deadline)
    }
    fn load_entry_in(
        root: &Directory,
        input: ReferenceEntryInput,
        deadline: Instant,
    ) -> Result<ReferenceEntry> {
        identifier(&input.id)?;
        identifier(&input.group)?;
        offset(input.offset_ms)?;
        let video = root.read(&input.video, SOURCE_LIMIT, false, deadline)?;
        let reference = root.read(&input.reference, JSON_LIMIT, true, deadline)?;
        let curve = Arc::new(parse_reference(&reference.bytes)?);
        let view = match input.view {
            ReferenceViewInput::Declared(view) => {
                view.validate()?;
                view
            }
            ReferenceViewInput::File(path) => read_view(root, &path, deadline)?,
        };
        json_bytes(&view, VIEW_LIMIT)?;
        ensure!(
            view.source_sha256 == video.binding.sha256,
            "Saved target view belongs to different source bytes"
        );
        root.confirm(&video.binding, deadline)?;
        root.confirm(&reference.binding, deadline)?;
        Ok(ReferenceEntry {
            id: input.id,
            group: input.group,
            input_root: root.path.clone(),
            video: video.binding.path.clone(),
            reference: reference.binding.path.clone(),
            view,
            curve,
            offset_ms: input.offset_ms,
            source_sha256: video.binding.sha256.clone(),
            reference_sha256: reference.binding.sha256.clone(),
            binding: EntryBinding {
                root: root.path.clone(),
                root_id: root.id()?,
                video: video.binding,
                reference: reference.binding,
            },
        })
    }

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Manifest {
        schema_version: u16,
        split: String,
        authorization: String,
        input_root: PathBuf,
        entries: Vec<ManifestEntry>,
    }
    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ManifestEntry {
        id: String,
        group: String,
        video: PathBuf,
        reference: PathBuf,
        permitted_uses: Vec<String>,
        reference_offset_ms: i64,
    }
    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ViewCatalog {
        schema_version: u16,
        entries: Vec<ViewEntry>,
    }
    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ViewEntry {
        id: String,
        source_sha256: String,
        declaration: AnnotatedTemporalView,
    }
    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct AuthoringReceipt {
        schema_version: u16,
        scope: String,
        qualified: bool,
        human_alignment_verified: bool,
        source_dimensions_verified: bool,
        native_plan_verified: bool,
        manifest_sha256: String,
        input_views_sha256: String,
        entries: Vec<EntryReceipt>,
    }
    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct EntryReceipt {
        id: String,
        source_sha256: String,
        source_byte_len: u64,
        reference_sha256: String,
        reference_byte_len: u64,
        view_sha256: String,
        reference_offset_ms: i64,
    }

    fn validate_entry(root: &Directory, entry: &ReferenceEntry, deadline: Instant) -> Result<()> {
        identifier(&entry.id)?;
        identifier(&entry.group)?;
        offset(entry.offset_ms)?;
        ensure!(
            entry.input_root == root.path
                && entry.binding.root == root.path
                && entry.binding.root_id == root.id()?,
            "Reference entries must retain the same original input root"
        );
        ensure!(
            entry.video == entry.binding.video.path
                && entry.reference == entry.binding.reference.path
                && entry.source_sha256 == entry.binding.video.sha256
                && entry.reference_sha256 == entry.binding.reference.sha256,
            "Reference entry identity changed after loading"
        );
        entry.view.validate()?;
        ensure!(
            entry.view.source_sha256 == entry.source_sha256,
            "Reference target view source binding differs"
        );
        json_bytes(&entry.view, VIEW_LIMIT)?;
        root.recheck(&entry.binding.video, SOURCE_LIMIT, false, deadline)?;
        let reference = root.recheck(&entry.binding.reference, JSON_LIMIT, true, deadline)?;
        let curve = parse_reference(&reference.bytes)?;
        ensure!(
            curve.points().iter().map(|p| (p.at_ms, p.pos)).eq(entry
                .curve
                .points()
                .iter()
                .map(|p| (p.at_ms, p.pos))),
            "Reference curve differs from its retained script bytes"
        );
        Ok(())
    }
    fn count(entries: usize) -> Result<()> {
        ensure!(
            (1..=64).contains(&entries),
            "Reference set requires 1..64 entries"
        );
        Ok(())
    }
    pub(super) fn export_new(
        destination: &Path,
        declared_authorization: &str,
        entries: &[ReferenceEntry],
    ) -> Result<PathBuf> {
        let deadline = Instant::now() + OPERATION_LIMIT;
        authorization(declared_authorization)?;
        count(entries.len())?;
        let root = Directory::open(&entries[0].input_root, true)?;
        let mut ids = BTreeSet::new();
        let mut sources = BTreeSet::new();
        let mut source_bytes = 0u64;
        for entry in entries {
            ensure!(ids.insert(entry.id.clone()), "Duplicate reference entry ID");
            ensure!(
                sources.insert(entry.source_sha256.clone()),
                "Duplicate source bytes in reference set"
            );
            source_bytes = source_bytes
                .checked_add(entry.binding.video.fingerprint.len)
                .context("Aggregate source byte count overflow")?;
            ensure!(
                source_bytes <= SOURCE_TOTAL_LIMIT,
                "Reference source set exceeds aggregate byte admission"
            );
            validate_entry(&root, entry, deadline)?;
        }
        let manifest = Manifest {
            schema_version: 2,
            split: "training".into(),
            authorization: declared_authorization.to_owned(),
            input_root: root.path.clone(),
            entries: entries
                .iter()
                .map(|e| ManifestEntry {
                    id: e.id.clone(),
                    group: e.group.clone(),
                    video: e.video.clone(),
                    reference: e.reference.clone(),
                    permitted_uses: vec!["training".into()],
                    reference_offset_ms: e.offset_ms,
                })
                .collect(),
        };
        let catalog = ViewCatalog {
            schema_version: 1,
            entries: entries
                .iter()
                .map(|e| ViewEntry {
                    id: e.id.clone(),
                    source_sha256: e.view.source_sha256.clone(),
                    declaration: e.view.declaration.clone(),
                })
                .collect(),
        };
        let manifest = json_bytes(&manifest, JSON_LIMIT)?;
        let catalog = json_bytes(&catalog, JSON_LIMIT)?;
        let mut receipt = AuthoringReceipt {
            schema_version: 1,
            scope: SCOPE.into(),
            qualified: false,
            human_alignment_verified: false,
            source_dimensions_verified: false,
            native_plan_verified: false,
            manifest_sha256: digest(&manifest),
            input_views_sha256: digest(&catalog),
            entries: Vec::new(),
        };
        let mut payloads = vec![
            ("manifest.json".to_owned(), manifest),
            ("input-views.json".to_owned(), catalog),
        ];
        for entry in entries {
            let view = json_bytes(&entry.view, VIEW_LIMIT)?;
            receipt.entries.push(EntryReceipt {
                id: entry.id.clone(),
                source_sha256: entry.source_sha256.clone(),
                source_byte_len: entry.binding.video.fingerprint.len,
                reference_sha256: entry.reference_sha256.clone(),
                reference_byte_len: entry.binding.reference.fingerprint.len,
                view_sha256: digest(&view),
                reference_offset_ms: entry.offset_ms,
            });
            payloads.push((format!("{}.view.json", entry.id), view));
        }
        payloads.push((
            "authoring-receipt.json".into(),
            json_bytes(&receipt, JSON_LIMIT)?,
        ));
        publish_payloads(destination, payloads, deadline, || {
            for entry in entries {
                root.confirm(&entry.binding.video, deadline)?;
                root.confirm(&entry.binding.reference, deadline)?;
            }
            Ok(())
        })
    }

    pub(super) fn load_set(directory: &Path) -> Result<(String, Vec<ReferenceEntry>)> {
        let deadline = Instant::now() + OPERATION_LIMIT;
        let directory = Directory::open(directory, false)?;
        let manifest_file =
            directory.read(Path::new("manifest.json"), JSON_LIMIT, true, deadline)?;
        let catalog_file =
            directory.read(Path::new("input-views.json"), JSON_LIMIT, true, deadline)?;
        let receipt_file = directory.read(
            Path::new("authoring-receipt.json"),
            JSON_LIMIT,
            true,
            deadline,
        )?;
        let manifest: Manifest = serde_json::from_value(strict_json(&manifest_file.bytes)?)?;
        let catalog: ViewCatalog = serde_json::from_value(strict_json(&catalog_file.bytes)?)?;
        let receipt: AuthoringReceipt = serde_json::from_value(strict_json(&receipt_file.bytes)?)?;
        ensure!(
            manifest.schema_version == 2
                && manifest.split == "training"
                && catalog.schema_version == 1,
            "Reference declaration schema or training split differs"
        );
        count(manifest.entries.len())?;
        authorization(&manifest.authorization)?;
        ensure!(
            manifest.input_root.is_absolute(),
            "Saved reference root must be absolute"
        );
        ensure!(
            receipt.schema_version == 1
                && receipt.scope == SCOPE
                && !receipt.qualified
                && !receipt.human_alignment_verified
                && !receipt.source_dimensions_verified
                && !receipt.native_plan_verified,
            "Authoring receipt makes an unsupported verification claim"
        );
        ensure!(
            receipt.manifest_sha256 == manifest_file.binding.sha256
                && receipt.input_views_sha256 == catalog_file.binding.sha256,
            "Saved reference documents differ from their authoring receipt"
        );
        ensure!(
            catalog.entries.len() == manifest.entries.len()
                && receipt.entries.len() == manifest.entries.len(),
            "Saved view/receipt entry coverage differs"
        );
        let mut views = BTreeMap::new();
        for view in catalog.entries {
            ensure!(
                views.insert(view.id.clone(), view).is_none(),
                "Duplicate saved view ID"
            );
        }
        let mut receipts = BTreeMap::new();
        for entry in receipt.entries {
            ensure!(
                receipts.insert(entry.id.clone(), entry).is_none(),
                "Duplicate authoring receipt ID"
            );
        }
        let root = Directory::open(&manifest.input_root, true)?;
        ensure!(
            root.path == manifest.input_root,
            "Saved input root is not canonical"
        );
        let mut entries = Vec::new();
        let mut ids = BTreeSet::new();
        let mut sources = BTreeSet::new();
        let mut sidecars = Vec::new();
        let mut source_bytes = 0u64;
        for declared in manifest.entries {
            identifier(&declared.id)?;
            ensure!(
                ids.insert(declared.id.clone()),
                "Duplicate manifest entry ID"
            );
            ensure!(
                declared.permitted_uses == ["training"],
                "Reference set must retain explicit training-only use"
            );
            ensure!(
                !declared.video.is_absolute() && !declared.reference.is_absolute(),
                "Manifest source/reference paths must be relative"
            );
            let view = views
                .remove(&declared.id)
                .context("Manifest ID has no saved target view")?;
            let recorded = receipts
                .remove(&declared.id)
                .context("Manifest ID has no authoring receipt")?;
            let sidecar = directory.read(
                Path::new(&format!("{}.view.json", declared.id)),
                VIEW_LIMIT,
                true,
                deadline,
            )?;
            let sidecar_view = parse_view(&sidecar.bytes)?;
            ensure!(
                sidecar.binding.sha256 == recorded.view_sha256
                    && serde_json::to_value(&sidecar_view)?
                        == serde_json::to_value(AnnotatedGenerationView {
                            source_sha256: view.source_sha256,
                            declaration: view.declaration
                        })?,
                "Saved target sidecar differs from view catalog or authoring receipt"
            );
            ensure!(
                declared.reference_offset_ms == recorded.reference_offset_ms,
                "Saved reference offset differs from authoring receipt"
            );
            let entry = load_entry_in(
                &root,
                ReferenceEntryInput {
                    id: declared.id,
                    group: declared.group,
                    video: declared.video,
                    reference: declared.reference,
                    view: ReferenceViewInput::Declared(sidecar_view),
                    offset_ms: declared.reference_offset_ms,
                },
                deadline,
            )?;
            ensure!(
                entry.source_sha256 == recorded.source_sha256
                    && entry.reference_sha256 == recorded.reference_sha256
                    && entry.binding.video.fingerprint.len == recorded.source_byte_len
                    && entry.binding.reference.fingerprint.len == recorded.reference_byte_len,
                "Saved source/reference correspondence differs from authoring receipt"
            );
            ensure!(
                sources.insert(entry.source_sha256.clone()),
                "Duplicate source bytes in saved reference set"
            );
            source_bytes = source_bytes
                .checked_add(entry.binding.video.fingerprint.len)
                .context("Aggregate source byte count overflow")?;
            ensure!(
                source_bytes <= SOURCE_TOTAL_LIMIT,
                "Saved reference source set exceeds aggregate admission"
            );
            sidecars.push(sidecar.binding);
            entries.push(entry);
        }
        ensure!(
            views.is_empty() && receipts.is_empty(),
            "Unconsumed saved view or authoring receipt"
        );
        for binding in [
            &manifest_file.binding,
            &catalog_file.binding,
            &receipt_file.binding,
        ]
        .into_iter()
        .chain(sidecars.iter())
        {
            directory.confirm(binding, deadline)?;
        }
        for entry in &entries {
            root.confirm(&entry.binding.video, deadline)?;
            root.confirm(&entry.binding.reference, deadline)?;
        }
        Ok((manifest.authorization, entries))
    }

    #[derive(Debug)]
    struct PublicationError {
        destination: PathBuf,
        outcome: crate::project_package::PackagePublicationOutcome,
        recovery: PathBuf,
        detail: String,
    }
    impl std::fmt::Display for PublicationError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "Reference declaration publication {:?} at {}: {}; last known retained location: {}",
                self.outcome,
                self.destination.display(),
                self.detail,
                self.recovery.display()
            )
        }
    }
    impl std::error::Error for PublicationError {}
    struct Member {
        name: String,
        file: File,
        fingerprint: Fingerprint,
        sha256: String,
    }
    struct Stage {
        parent: Directory,
        directory: File,
        name: String,
        destination_name: OsString,
        destination: PathBuf,
        members: Vec<Member>,
        preserve: bool,
        published: bool,
    }
    impl Stage {
        fn new(destination: &Path) -> Result<Self> {
            no_parent(destination)?;
            let absolute = if destination.is_absolute() {
                destination.to_owned()
            } else {
                std::env::current_dir()?.join(destination)
            };
            let destination_name = absolute
                .file_name()
                .context("New reference directory name required")?
                .to_owned();
            let parent = Directory::open(
                absolute
                    .parent()
                    .context("Reference destination parent required")?,
                false,
            )?;
            match statat(&parent.file, &destination_name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(_) => bail!("Reference destination already exists; nothing was overwritten"),
                Err(rustix::io::Errno::NOENT) => {}
                Err(error) => return Err(error.into()),
            }
            let name = format!(".pulsar-reference-set-{}", uuid::Uuid::new_v4());
            rustix::fs::mkdirat(&parent.file, name.as_str(), Mode::from_raw_mode(0o700))?;
            let directory = File::from(
                openat(
                    &parent.file,
                    name.as_str(),
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .with_context(|| {
                    format!(
                        "Open newly created reference staging directory; retained {}",
                        parent.path.join(&name).display()
                    )
                })?,
            );
            Ok(Self {
                destination: parent.path.join(&destination_name),
                parent,
                directory,
                name,
                destination_name,
                members: Vec::new(),
                preserve: false,
                published: false,
            })
        }
        fn failure(
            &self,
            outcome: crate::project_package::PackagePublicationOutcome,
            detail: impl std::fmt::Display,
        ) -> anyhow::Error {
            PublicationError {
                destination: self.destination.clone(),
                outcome,
                recovery: if self.published {
                    self.destination.clone()
                } else {
                    self.parent.path.join(&self.name)
                },
                detail: detail.to_string(),
            }
            .into()
        }
        fn check(&self) -> Result<()> {
            self.parent.check_name()?;
            let name = if self.published {
                self.destination_name.as_os_str()
            } else {
                std::ffi::OsStr::new(&self.name)
            };
            let named = statat(&self.parent.file, name, AtFlags::SYMLINK_NOFOLLOW)?;
            let held = self.directory.metadata()?;
            ensure!(
                held.dev() == named.st_dev && held.ino() == named.st_ino,
                "Reference publication directory was replaced or renamed"
            );
            ensure!(
                held.mode() & 0o077 == 0 && held.uid() == rustix::process::geteuid().as_raw(),
                "Reference staging directory must remain private and owned"
            );
            Ok(())
        }
        fn write(&mut self, name: String, bytes: &[u8], deadline: Instant) -> Result<()> {
            let result = (|| -> Result<()> {
                check_time(deadline)?;
                self.check()?;
                ensure!(
                    Path::new(&name).components().count() == 1
                        && matches!(
                            Path::new(&name).components().next(),
                            Some(Component::Normal(_))
                        ),
                    "Reference export member must be a leaf"
                );
                let mut file = File::from(openat(
                    &self.directory,
                    name.as_str(),
                    OFlags::RDWR
                        | OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::NOFOLLOW
                        | OFlags::CLOEXEC,
                    Mode::from_raw_mode(0o600),
                )?);
                file.write_all(bytes)?;
                file.sync_all()?;
                check_time(deadline)?;
                self.members.push(Member {
                    name,
                    fingerprint: Fingerprint::new(&file.metadata()?),
                    file,
                    sha256: digest(bytes),
                });
                Ok(())
            })();
            result.map_err(|error| {
                self.preserve = true;
                self.failure(
                    crate::project_package::PackagePublicationOutcome::NotPublished,
                    error,
                )
            })
        }
        fn verify_members(&self, deadline: Instant) -> Result<()> {
            self.check()?;
            for entry in rustix::fs::Dir::read_from(&self.directory)? {
                let entry = entry?;
                let name = entry.file_name().to_bytes();
                ensure!(
                    name == b"."
                        || name == b".."
                        || self.members.iter().any(|m| m.name.as_bytes() == name),
                    "Unexpected reference staging member; retain for inspection"
                );
            }
            for member in &self.members {
                check_time(deadline)?;
                let named = statat(
                    &self.directory,
                    member.name.as_str(),
                    AtFlags::SYMLINK_NOFOLLOW,
                )?;
                let metadata = member.file.metadata()?;
                ensure!(
                    metadata.is_file()
                        && metadata.nlink() == 1
                        && metadata.mode() & 0o077 == 0
                        && metadata.dev() == named.st_dev
                        && metadata.ino() == named.st_ino
                        && Fingerprint::new(&metadata) == member.fingerprint,
                    "Reference staging member changed after writing"
                );
                let mut file = member.file.try_clone()?;
                file.seek(SeekFrom::Start(0))?;
                let (_, hash, length) = read_file(&mut file, JSON_LIMIT, false, deadline)?;
                ensure!(
                    hash == member.sha256
                        && length == member.fingerprint.len
                        && Fingerprint::new(&file.metadata()?) == member.fingerprint,
                    "Reference staging bytes changed after writing"
                );
            }
            Ok(())
        }
        fn publish(
            mut self,
            deadline: Instant,
            before_publish: impl FnOnce() -> Result<()>,
            after_rename: impl FnOnce(),
        ) -> Result<PathBuf> {
            use crate::project_package::PackagePublicationOutcome as Outcome;
            self.preserve = true;
            (|| -> Result<()> {
                self.verify_members(deadline)?;
                self.directory.sync_all()?;
                before_publish()?;
                self.check()?;
                check_time(deadline)
            })()
            .map_err(|error| self.failure(Outcome::NotPublished, error))?;
            rustix::fs::renameat_with(
                &self.parent.file,
                self.name.as_str(),
                &self.parent.file,
                &self.destination_name,
                rustix::fs::RenameFlags::NOREPLACE,
            )
            .map_err(|error| {
                self.failure(
                    if error == rustix::io::Errno::EXIST || error == rustix::io::Errno::NOTEMPTY {
                        Outcome::NotPublished
                    } else {
                        Outcome::OutcomeUnknown
                    },
                    format!("Atomic no-replace directory publication failed: {error}"),
                )
            })?;
            self.published = true;
            after_rename();
            self.parent
                .file
                .sync_all()
                .map_err(|error| self.failure(Outcome::PublishedDurabilityUnconfirmed, error))?;
            self.check()
                .map_err(|error| self.failure(Outcome::OutcomeUnknown, error))?;
            check_time(deadline)
                .map_err(|error| self.failure(Outcome::PublishedDurabilityUnconfirmed, error))?;
            Ok(self.destination.clone())
        }
    }
    impl Drop for Stage {
        fn drop(&mut self) {
            if self.preserve || self.published || self.check().is_err() {
                return;
            }
            for member in &self.members {
                if let (Ok(named), Ok(held)) = (
                    statat(
                        &self.directory,
                        member.name.as_str(),
                        AtFlags::SYMLINK_NOFOLLOW,
                    ),
                    member.file.metadata(),
                ) {
                    if Fingerprint::new(&held) == member.fingerprint
                        && named.st_dev == held.dev()
                        && named.st_ino == held.ino()
                    {
                        let _ = rustix::fs::unlinkat(
                            &self.directory,
                            member.name.as_str(),
                            AtFlags::empty(),
                        );
                    }
                }
            }
            let _ = rustix::fs::unlinkat(&self.parent.file, self.name.as_str(), AtFlags::REMOVEDIR);
        }
    }
    fn publish_payloads(
        destination: &Path,
        payloads: Vec<(String, Vec<u8>)>,
        deadline: Instant,
        before_publish: impl FnOnce() -> Result<()>,
    ) -> Result<PathBuf> {
        let mut stage = Stage::new(destination)?;
        for (name, bytes) in payloads {
            stage.write(name, &bytes, deadline)?;
        }
        stage.publish(deadline, before_publish, || {})
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use pulsar_protocol::{StereoEye, StereoLayout, VideoProjection, VideoView};

        struct Fixture {
            directory: tempfile::TempDir,
            root: PathBuf,
        }
        impl Fixture {
            fn new() -> Self {
                let directory = tempfile::tempdir().unwrap();
                let root = directory.path().join("Live-Reference");
                fs::create_dir(&root).unwrap();
                fs::write(
                    root.join("source.bin"),
                    b"authored neutral bytes, not decoded",
                )
                .unwrap();
                fs::write(
                    root.join("reference.funscript"),
                    br#"{"actions":[{"at":0,"pos":20},{"at":1000,"pos":80}],"inverted":true}"#,
                )
                .unwrap();
                Self { directory, root }
            }
            fn view(&self) -> AnnotatedGenerationView {
                AnnotatedGenerationView {
                    source_sha256: digest(&fs::read(self.root.join("source.bin")).unwrap()),
                    declaration: AnnotatedTemporalView {
                        schema_version: 1,
                        source_dimensions: [64, 64],
                        analysis_dimensions: [640, 480],
                        video_view: VideoView {
                            layout: StereoLayout::Mono,
                            eye: StereoEye::Left,
                            projection: VideoProjection::Flat,
                        },
                        target_region: [1, 2, 20, 20],
                        target_id: "declared-target".into(),
                    },
                }
            }
            fn input(&self) -> ReferenceEntryInput {
                ReferenceEntryInput {
                    id: "one".into(),
                    group: "source-family".into(),
                    video: "source.bin".into(),
                    reference: "reference.funscript".into(),
                    view: ReferenceViewInput::Declared(self.view()),
                    offset_ms: -125,
                }
            }
            fn entry(&self) -> ReferenceEntry {
                load_entry(&self.root, self.input()).unwrap()
            }
            fn output(&self) -> PathBuf {
                self.directory.path().join("declarations")
            }
            fn exported(&self) -> PathBuf {
                export_new(&self.output(), "Authored fixture only", &[self.entry()]).unwrap()
            }
        }
        fn read_json(path: &Path) -> Value {
            strict_json(&fs::read(path).unwrap()).unwrap()
        }
        fn write_json(path: &Path, value: &Value) {
            fs::write(path, json_bytes(value, JSON_LIMIT).unwrap()).unwrap();
        }
        fn refresh_document_hashes(output: &Path) {
            let path = output.join("authoring-receipt.json");
            let mut receipt = read_json(&path);
            receipt["manifest_sha256"] =
                digest(&fs::read(output.join("manifest.json")).unwrap()).into();
            receipt["input_views_sha256"] =
                digest(&fs::read(output.join("input-views.json")).unwrap()).into();
            write_json(&path, &receipt);
        }

        #[test]
        fn reference_json_preserves_raw_positions_and_refuses_invalid_actions_or_duplicates() {
            let curve = parse_reference(
                br#"{"actions":[{"at":0,"pos":20},{"at":1000,"pos":80}],"inverted":true}"#,
            )
            .unwrap();
            assert_eq!(
                curve
                    .points()
                    .iter()
                    .map(|p| (p.at_ms, p.pos))
                    .collect::<Vec<_>>(),
                vec![(0, 20), (1000, 80)]
            );
            for raw in [
                r#"{"actions":[],"actions":[{"at":0,"pos":0},{"at":1,"pos":1}]}"#,
                r#"{"actions":[{"at":0,"at":1,"pos":0},{"at":2,"pos":1}]}"#,
                r#"{"actions":[{"at":0,"pos":0},{"at":1,"pos":1}],"meta":{"x":1,"x":2}}"#,
                r#"{"actions":[{"at":0.0,"pos":0},{"at":1,"pos":1}]}"#,
                r#"{"actions":[{"at":false,"pos":0},{"at":1,"pos":1}]}"#,
                r#"{"actions":[{"at":0,"pos":0},{"at":1,"pos":1.0}]}"#,
                r#"{"actions":[{"at":0,"pos":0},{"at":1,"pos":101}]}"#,
                r#"{"actions":[{"at":0,"pos":0},{"at":86400001,"pos":1}]}"#,
                r#"{"actions":[{"at":1,"pos":0},{"at":1,"pos":1}]}"#,
                r#"{"actions":[{"at":2,"pos":0},{"at":1,"pos":1}]}"#,
                r#"{"actions":[{"at":0,"pos":0}]}"#,
                r#"{"actions":[{"at":0,"pos":0},{"at":1,"pos":1}],"meta":NaN}"#,
                r#"{"actions":[{"at":0,"pos":0},{"at":1,"pos":1}],"meta":1e999}"#,
            ] {
                assert!(parse_reference(raw.as_bytes()).is_err(), "{raw}");
            }
        }

        #[test]
        fn entry_accepts_contained_absolute_paths_and_explicit_external_view() {
            let f = Fixture::new();
            let sidecar = f.directory.path().join("saved.view.json");
            fs::write(&sidecar, json_bytes(&f.view(), VIEW_LIMIT).unwrap()).unwrap();
            let mut input = f.input();
            input.video = f.root.join(&input.video);
            input.reference = f.root.join(&input.reference);
            input.view = ReferenceViewInput::File(sidecar);
            let entry = load_entry(&f.root, input).unwrap();
            assert_eq!(entry.input_root, f.root.canonicalize().unwrap());
            assert_eq!(entry.video, Path::new("source.bin"));
            assert_eq!(entry.reference, Path::new("reference.funscript"));
            assert_eq!(entry.view.source_sha256, entry.source_sha256);
            assert_eq!(
                entry.reference_sha256,
                digest(&fs::read(f.root.join("reference.funscript")).unwrap())
            );
            assert_eq!(entry.offset_ms, -125);
            let cloned = entry.clone();
            assert!(Arc::ptr_eq(&entry.curve, &cloned.curve));
            fs::write(
                f.root.join("relative.view.json"),
                json_bytes(&f.view(), VIEW_LIMIT).unwrap(),
            )
            .unwrap();
            let mut input = f.input();
            input.view = ReferenceViewInput::File("relative.view.json".into());
            assert!(load_entry(&f.root, input).is_ok());
        }

        #[test]
        fn intake_refuses_escapes_symlinks_nonregular_and_wrong_root() {
            use std::os::unix::fs::symlink;
            let f = Fixture::new();
            let outside = f.directory.path().join("outside.bin");
            fs::write(&outside, b"outside").unwrap();
            symlink(f.root.join("source.bin"), f.root.join("leaf-link")).unwrap();
            symlink(&f.root, f.root.join("parent-link")).unwrap();
            fs::create_dir(f.root.join("not-file")).unwrap();
            for path in [
                outside,
                PathBuf::from("../outside.bin"),
                PathBuf::from("leaf-link"),
                PathBuf::from("parent-link/source.bin"),
                PathBuf::from("not-file"),
            ] {
                let mut input = f.input();
                input.video = path;
                assert!(load_entry(&f.root, input).is_err());
            }
            symlink(
                f.root.join("reference.funscript"),
                f.root.join("script-link"),
            )
            .unwrap();
            let mut input = f.input();
            input.reference = "script-link".into();
            assert!(load_entry(&f.root, input).is_err());
            assert!(load_entry(f.directory.path(), f.input()).is_err());
            assert!(load_entry(&f.root.join("../Live-Reference"), f.input()).is_err());
        }

        #[test]
        fn target_sidecars_refuse_wrong_binding_duplicate_fields_unknown_fields_and_oversize() {
            let f = Fixture::new();
            let path = f.directory.path().join("saved.view.json");
            let valid = serde_json::to_value(f.view()).unwrap();
            let mut wrong = valid.clone();
            wrong["source_sha256"] = "0".repeat(64).into();
            let mut unknown = valid.clone();
            unknown["verified"] = true.into();
            let duplicate = format!(
                "{{\"source_sha256\":\"{}\",\"source_sha256\":\"{}\",\"declaration\":{}}}",
                f.view().source_sha256,
                f.view().source_sha256,
                valid["declaration"]
            );
            let mut oversized = serde_json::to_vec(&valid).unwrap();
            oversized.resize(VIEW_LIMIT as usize + 1, b' ');
            for raw in [
                serde_json::to_vec(&wrong).unwrap(),
                serde_json::to_vec(&unknown).unwrap(),
                duplicate.into_bytes(),
                oversized,
            ] {
                fs::write(&path, raw).unwrap();
                let mut input = f.input();
                input.view = ReferenceViewInput::File(path.clone());
                assert!(load_entry(&f.root, input).is_err());
            }
            let link = f.directory.path().join("sidecar-link");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            let mut input = f.input();
            input.view = ReferenceViewInput::File(link);
            assert!(load_entry(&f.root, input).is_err());
        }

        #[test]
        fn export_rechecks_content_identity_and_curve_before_creating_destination() {
            for case in 0..5 {
                let f = Fixture::new();
                let mut entry = f.entry();
                match case {
                    0 => {
                        let mut bytes = fs::read(f.root.join("source.bin")).unwrap();
                        bytes[0] ^= 1;
                        fs::write(f.root.join("source.bin"), bytes).unwrap();
                    }
                    1 => {
                        let bytes = fs::read_to_string(f.root.join("reference.funscript"))
                            .unwrap()
                            .replace("80", "81");
                        fs::write(f.root.join("reference.funscript"), bytes).unwrap();
                    }
                    2 => {
                        let bytes = fs::read(f.root.join("source.bin")).unwrap();
                        fs::rename(f.root.join("source.bin"), f.root.join("original.bin")).unwrap();
                        fs::write(f.root.join("source.bin"), bytes).unwrap();
                    }
                    3 => {
                        entry.curve = Arc::new(
                            ReferenceCurve::new(vec![
                                ReferencePoint { at_ms: 0, pos: 99 },
                                ReferencePoint {
                                    at_ms: 1000,
                                    pos: 80,
                                },
                            ])
                            .unwrap(),
                        )
                    }
                    4 => entry.video = "different.bin".into(),
                    _ => unreachable!(),
                }
                assert!(
                    export_new(&f.output(), "Authored fixture only", &[entry]).is_err(),
                    "case {case}"
                );
                assert!(!f.output().exists());
                assert!(!fs::read_dir(f.directory.path()).unwrap().any(|e| e
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".pulsar-reference-set-")));
            }
        }

        #[test]
        fn exported_documents_are_exact_unqualified_private_and_reopenable() {
            let f = Fixture::new();
            let entry = f.entry();
            let output = export_new(
                &f.output(),
                "Authored fixture only",
                std::slice::from_ref(&entry),
            )
            .unwrap();
            let manifest = read_json(&output.join("manifest.json"));
            assert_eq!(
                manifest,
                serde_json::json!({"schema_version":2,"split":"training","authorization":"Authored fixture only","input_root":f.root.canonicalize().unwrap(),"entries":[{"id":"one","group":"source-family","video":"source.bin","reference":"reference.funscript","permitted_uses":["training"],"reference_offset_ms":-125}]})
            );
            let catalog = read_json(&output.join("input-views.json"));
            assert_eq!(
                catalog,
                serde_json::json!({"schema_version":1,"entries":[{"id":"one","source_sha256":entry.source_sha256,"declaration":entry.view.declaration}]})
            );
            assert_eq!(
                read_json(&output.join("one.view.json")),
                serde_json::to_value(&entry.view).unwrap()
            );
            let receipt = read_json(&output.join("authoring-receipt.json"));
            for field in [
                "qualified",
                "human_alignment_verified",
                "source_dimensions_verified",
                "native_plan_verified",
            ] {
                assert_eq!(receipt[field], false);
            }
            assert_eq!(receipt["entries"][0]["reference_offset_ms"], -125);
            assert_eq!(
                receipt["entries"][0]["reference_sha256"],
                entry.reference_sha256
            );
            assert_eq!(fs::metadata(&output).unwrap().mode() & 0o077, 0);
            let names: BTreeSet<_> = fs::read_dir(&output)
                .unwrap()
                .map(|e| {
                    let e = e.unwrap();
                    assert_eq!(e.metadata().unwrap().mode() & 0o077, 0);
                    e.file_name().into_string().unwrap()
                })
                .collect();
            assert_eq!(
                names,
                [
                    "manifest.json",
                    "input-views.json",
                    "one.view.json",
                    "authoring-receipt.json"
                ]
                .into_iter()
                .map(str::to_owned)
                .collect()
            );
            let (authorization, reopened) = load_set(&output).unwrap();
            assert_eq!(authorization, "Authored fixture only");
            assert_eq!(reopened.len(), 1);
            assert_eq!(reopened[0].offset_ms, -125);
            assert_eq!(reopened[0].source_sha256, entry.source_sha256);
            assert_eq!(reopened[0].curve.points()[0].pos, 20);
            let second = f.directory.path().join("reopened-export");
            export_new(&second, &authorization, &reopened).unwrap();
            assert_eq!(
                fs::read(output.join("manifest.json")).unwrap(),
                fs::read(second.join("manifest.json")).unwrap()
            );
        }

        #[test]
        fn export_refuses_duplicate_ids_sources_mixed_roots_bounds_and_missing_authorization() {
            let f = Fixture::new();
            let entry = f.entry();
            let mut different_id = entry.clone();
            different_id.id = "two".into();
            assert!(export_new(&f.output(), "Authored", &[entry.clone(), entry.clone()]).is_err());
            assert!(export_new(&f.output(), "Authored", &[entry.clone(), different_id]).is_err());
            let other = Fixture::new();
            // Different bytes make root mismatch, rather than duplicate content,
            // the reason this independent entry cannot join the same set.
            fs::write(other.root.join("source.bin"), b"different neutral bytes").unwrap();
            let other_entry = load_entry(
                &other.root,
                ReferenceEntryInput {
                    id: "other".into(),
                    ..other.input()
                },
            )
            .unwrap();
            assert!(export_new(&f.output(), "Authored", &[entry.clone(), other_entry]).is_err());
            assert!(export_new(&f.output(), "Authored", &[]).is_err());
            assert!(export_new(&f.output(), "Authored", &vec![entry.clone(); 65]).is_err());
            for authorization in [String::new(), " ".into(), "a".repeat(4097)] {
                assert!(
                    export_new(&f.output(), &authorization, std::slice::from_ref(&entry)).is_err()
                );
            }
            let mut invalid = entry.clone();
            invalid.offset_ms = i64::MIN;
            assert!(export_new(&f.output(), "Authored", &[invalid]).is_err());
            let mut invalid = entry;
            invalid.group = "".into();
            assert!(export_new(&f.output(), "Authored", &[invalid]).is_err());
            assert!(!f.output().exists());
        }

        #[test]
        fn reopen_refuses_tampered_offsets_coverage_rights_unknown_fields_and_content() {
            for case in 0..6 {
                let f = Fixture::new();
                let output = f.exported();
                match case {
                    0 => {
                        let path = output.join("manifest.json");
                        let mut m = read_json(&path);
                        m["entries"][0]["reference_offset_ms"] = 125.into();
                        write_json(&path, &m);
                        refresh_document_hashes(&output);
                    }
                    1 => {
                        let path = output.join("input-views.json");
                        let mut m = read_json(&path);
                        m["entries"][0]["id"] = "other".into();
                        write_json(&path, &m);
                        refresh_document_hashes(&output);
                    }
                    2 => {
                        let path = output.join("manifest.json");
                        let mut m = read_json(&path);
                        m["entries"][0]["permitted_uses"] = serde_json::json!(["evaluation"]);
                        write_json(&path, &m);
                        refresh_document_hashes(&output);
                    }
                    3 => {
                        let path = output.join("authoring-receipt.json");
                        let mut m = read_json(&path);
                        m["approved"] = true.into();
                        write_json(&path, &m);
                    }
                    4 => {
                        fs::write(
                            f.root.join("reference.funscript"),
                            br#"{"actions":[{"at":0,"pos":1},{"at":1000,"pos":2}]}"#,
                        )
                        .unwrap();
                    }
                    5 => {
                        let path = output.join("one.view.json");
                        let mut m = read_json(&path);
                        m["declaration"]["target_id"] = "other-target".into();
                        write_json(&path, &m);
                    }
                    _ => unreachable!(),
                }
                assert!(load_set(&output).is_err(), "case {case}");
            }
        }

        #[test]
        fn publication_preserves_existing_destination_including_late_noreplace_race() {
            let f = Fixture::new();
            fs::create_dir(f.output()).unwrap();
            fs::write(f.output().join("sentinel"), b"keep").unwrap();
            assert!(export_new(&f.output(), "Authored", &[f.entry()]).is_err());
            assert_eq!(fs::read(f.output().join("sentinel")).unwrap(), b"keep");
            let raced = f.directory.path().join("raced");
            let result = publish_payloads(
                &raced,
                vec![("declaration.json".into(), b"{}\n".to_vec())],
                Instant::now() + OPERATION_LIMIT,
                || {
                    fs::create_dir(&raced)?;
                    fs::write(raced.join("sentinel"), b"late owner file")?;
                    Ok(())
                },
            );
            let error = result.unwrap_err();
            assert!(matches!(
                &error.downcast_ref::<PublicationError>().unwrap().outcome,
                crate::project_package::PackagePublicationOutcome::NotPublished
            ));
            assert_eq!(
                fs::read(raced.join("sentinel")).unwrap(),
                b"late owner file"
            );
            assert!(!raced.join("declaration.json").exists());
        }

        #[test]
        fn post_rename_namespace_failure_never_deletes_possibly_published_output() {
            let f = Fixture::new();
            let parent = f.directory.path().join("parent");
            let moved = f.directory.path().join("moved-parent");
            fs::create_dir(&parent).unwrap();
            let destination = parent.join("set");
            let deadline = Instant::now() + OPERATION_LIMIT;
            let mut stage = Stage::new(&destination).unwrap();
            stage
                .write("declaration.json".into(), b"{}\n", deadline)
                .unwrap();
            let error = stage
                .publish(
                    deadline,
                    || Ok(()),
                    || {
                        fs::rename(&parent, &moved).unwrap();
                        fs::create_dir(&parent).unwrap();
                    },
                )
                .unwrap_err();
            assert!(matches!(
                &error.downcast_ref::<PublicationError>().unwrap().outcome,
                crate::project_package::PackagePublicationOutcome::OutcomeUnknown
            ));
            assert_eq!(
                fs::read(moved.join("set/declaration.json")).unwrap(),
                b"{}\n"
            );
            assert!(!destination.exists());
        }

        #[test]
        fn file_reads_check_byte_and_elapsed_budgets_before_consumption() {
            let f = Fixture::new();
            let root = Directory::open(&f.root, true).unwrap();
            assert!(root
                .read(
                    Path::new("source.bin"),
                    2,
                    false,
                    Instant::now() + OPERATION_LIMIT
                )
                .is_err());
            assert!(root
                .read(Path::new("source.bin"), SOURCE_LIMIT, false, Instant::now())
                .is_err());
            assert!(strict_json(&vec![b' '; JSON_LIMIT as usize + 1]).is_err());
        }
    }
}
