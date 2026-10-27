// Included in the Linux pack adapter; shares no-follow descriptor helpers.
const CATALOG_CAPSULE_LIMIT: usize = 64 * 1024 * 1024;
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogReplay {
    fingerprint: String,
    report: ModelCatalogReport,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogPayload {
    schema_version: u16,
    bootstrap: String,
    cache: std::collections::BTreeMap<String, String>,
    manifests: std::collections::BTreeMap<String, String>,
    report: ModelCatalogReport,
    receipts: std::collections::BTreeMap<String, CatalogReplay>,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogCapsule {
    payload_sha256: String,
    payload: CatalogPayload,
}
fn catalog_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}
fn catalog_decode(text: &str, limit: usize) -> Result<Vec<u8>> {
    use base64::Engine;
    ensure!(
        text.len() <= limit.div_ceil(3) * 4,
        "catalog encoded object exceeds admission"
    );
    let bytes = base64::engine::general_purpose::STANDARD.decode(text)?;
    ensure!(
        bytes.len() <= limit && catalog_encode(&bytes) == text,
        "invalid catalog object encoding"
    );
    Ok(bytes)
}
fn catalog_owned(file: &File, directory: bool) -> Result<()> {
    let meta = file.metadata()?;
    ensure!(
        meta.uid() == unsafe { libc::geteuid() }
            && if directory {
                meta.is_dir() && meta.mode() & 0o777 == 0o700
            } else {
                meta.is_file() && meta.nlink() == 1 && meta.mode() & 0o777 == 0o600
            },
        "catalog state must be owned, private and single-link regular storage"
    );
    Ok(())
}
fn catalog_root(state: &Path, create: bool) -> Result<File> {
    let path = state.join("model-catalogs");
    let root = if create {
        crate::project_package_storage::unix::directory(&path).map_err(|e| anyhow::anyhow!(e))?
    } else {
        source_directory(&path)?
    };
    catalog_owned(&root, true)?;
    Ok(root)
}
fn catalog_lock(root: &File, digest: &str) -> Result<File> {
    let name = CString::new(format!("{digest}.lock"))?;
    // SAFETY: held directory and validated single-component name; uniquely owned result.
    let fd = unsafe {
        libc::openat(
            root.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let lock = unsafe { File::from_raw_fd(fd) };
    catalog_owned(&lock, false)?;
    fs2::FileExt::try_lock_exclusive(&lock).context("catalog refresh is busy")?;
    catalog_lock_matches(root, digest, &lock)?;
    Ok(lock)
}
fn catalog_lock_matches(root: &File, digest: &str, lock: &File) -> Result<()> {
    let named = open_child(root, &format!("{digest}.lock"), false)?;
    catalog_owned(&named, false)?;
    let a = lock.metadata()?;
    let b = named.metadata()?;
    ensure!(
        a.dev() == b.dev() && a.ino() == b.ino(),
        "catalog lock identity changed"
    );
    Ok(())
}
fn catalog_load(root: &File, digest: &str) -> Result<Option<CatalogPayload>> {
    let file = match open_child(root, &format!("{digest}.json"), false) {
        Ok(file) => file,
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None)
        }
        Err(e) => return Err(e),
    };
    catalog_owned(&file, false)?;
    ensure!(
        file.metadata()?.len() <= CATALOG_CAPSULE_LIMIT as u64,
        "catalog capsule exceeds admission"
    );
    let mut bytes = Vec::new();
    file.take(CATALOG_CAPSULE_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= CATALOG_CAPSULE_LIMIT,
        "catalog capsule grew beyond admission"
    );
    let capsule: CatalogCapsule =
        serde_json::from_slice(&bytes).context("corrupt catalog capsule; original preserved")?;
    ensure!(
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&capsule.payload)?)
        ) == capsule.payload_sha256,
        "catalog capsule integrity mismatch"
    );
    let payload = capsule.payload;
    catalog_validate(&payload, digest)?;
    Ok(Some(payload))
}
fn catalog_validate(payload: &CatalogPayload, digest: &str) -> Result<()> {
    ensure!(
        payload.schema_version == 1 && payload.receipts.len() <= 64,
        "unsupported catalog capsule layout"
    );
    payload.report.validate()?;
    ensure!(
        payload.report.bootstrap_sha256 == digest,
        "catalog namespace changed"
    );
    let bootstrap = catalog_decode(&payload.bootstrap, 2 * 1024 * 1024)?;
    ensure!(
        format!("{:x}", Sha256::digest(&bootstrap)) == digest,
        "catalog bootstrap changed"
    );
    let state = catalog_state(payload)?;
    use tough::schema::{Root, Signed, Snapshot, Targets, Timestamp};
    let root: Signed<Root> = serde_json::from_slice(&state.cache["root.json"])?;
    root.signed.verify_role(&root)?;
    ensure!(
        root.signed.version.get() == payload.report.root_version
            && format!("{:x}", Sha256::digest(&state.cache["root.json"]))
                == payload.report.current_root_sha256,
        "catalog current root changed"
    );
    let timestamp: Signed<Timestamp> = serde_json::from_slice(&state.cache["timestamp.json"])?;
    root.signed.verify_role(&timestamp)?;
    let snapshot: Signed<Snapshot> = serde_json::from_slice(&state.cache["snapshot.json"])?;
    root.signed.verify_role(&snapshot)?;
    let targets: Signed<Targets> = serde_json::from_slice(&state.cache["targets.json"])?;
    root.signed.verify_role(&targets)?;
    ensure!(
        timestamp.signed.version.get() == payload.report.timestamp_version
            && snapshot.signed.version.get() == payload.report.snapshot_version
            && targets.signed.version.get() == payload.report.targets_version,
        "catalog role versions changed"
    );
    let _: jiff::Timestamp = serde_json::from_slice(&state.cache["latest_known_time.json"])?;
    ensure!(
        payload.manifests.keys().cloned().collect::<Vec<_>>() == payload.report.manifest_sha256s,
        "catalog manifest inventory changed"
    );
    ensure!(
        targets.signed.delegations.is_none()
            && targets.signed.targets.len() == payload.manifests.len(),
        "catalog signed target inventory changed"
    );
    let snapshot_meta = timestamp
        .signed
        .meta
        .get("snapshot.json")
        .context("catalog snapshot binding absent")?;
    ensure!(
        snapshot_meta.version.get() == snapshot.signed.version.get(),
        "catalog snapshot binding changed"
    );
    let targets_meta = snapshot
        .signed
        .meta
        .get("targets.json")
        .context("catalog targets binding absent")?;
    ensure!(
        targets_meta.version.get() == targets.signed.version.get(),
        "catalog targets binding changed"
    );
    for (hash, encoded) in &payload.manifests {
        let bytes = catalog_decode(encoded, PACK_MANIFEST_LIMIT)?;
        ensure!(
            format!("{:x}", Sha256::digest(&bytes)) == *hash,
            "catalog retained manifest changed"
        );
        let name = tough::TargetName::new(format!("manifests/{hash}.json"))?;
        let target = targets
            .signed
            .targets
            .get(&name)
            .context("retained manifest absent from signed catalog")?;
        ensure!(
            target.length == bytes.len() as u64
                && target.hashes.sha256.as_ref() == Sha256::digest(&bytes).as_slice(),
            "retained manifest differs from authenticated target"
        );
        let manifest: ModelPackManifest = serde_json::from_slice(&bytes)?;
        manifest.validate()?;
    }
    for (key, receipt) in &payload.receipts {
        validate_sha256(key)?;
        validate_sha256(&receipt.fingerprint)?;
        receipt.report.validate()?;
        ensure!(
            receipt.report.bootstrap_sha256 == digest
                && receipt.report.catalog_revision <= payload.report.catalog_revision,
            "invalid catalog replay namespace/revision"
        );
        let expected = format!(
            "{:x}",
            Sha256::digest(
                format!(
                    "{}\0{}",
                    receipt.report.accepted_by.as_str(),
                    receipt.report.accepted_request.as_str()
                )
                .as_bytes()
            )
        );
        ensure!(*key == expected, "catalog replay actor/request changed");
    }
    Ok(())
}
fn catalog_state(payload: &CatalogPayload) -> Result<crate::offline_catalog::CatalogState> {
    let names = [
        "root.json",
        "timestamp.json",
        "snapshot.json",
        "targets.json",
        "latest_known_time.json",
    ];
    ensure!(
        payload.cache.len() == names.len(),
        "incomplete catalog trust state"
    );
    let mut cache = std::collections::BTreeMap::new();
    for name in names {
        cache.insert(
            name.into(),
            catalog_decode(
                payload
                    .cache
                    .get(name)
                    .context("missing catalog metadata")?,
                2 * 1024 * 1024,
            )?,
        );
    }
    Ok(crate::offline_catalog::CatalogState {
        bootstrap_sha256: payload.report.bootstrap_sha256.clone(),
        root_version: payload.report.root_version,
        cache,
    })
}
fn catalog_walk(
    dir: &File,
    prefix: &str,
    depth: usize,
    files: &mut std::collections::BTreeMap<String, Vec<u8>>,
    total: &mut usize,
    dirs: &mut usize,
) -> Result<()> {
    ensure!(depth <= 4, "catalog directory nesting exceeds admission");
    *dirs += 1;
    ensure!(*dirs <= 64, "catalog directory count exceeds admission");
    let names = std::fs::read_dir(format!("/proc/self/fd/{}", dir.as_raw_fd()))?
        .take(257)
        .map(|e| e.map(|e| e.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    ensure!(names.len() <= 256, "catalog entries exceed admission");
    for name in names {
        let name = name.to_str().context("nonportable catalog entry")?;
        ensure!(
            !name.is_empty()
                && name != "."
                && name != ".."
                && name.len() <= 128
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
            "unsafe catalog component"
        );
        let path = if prefix.is_empty() {
            name.into()
        } else {
            format!("{prefix}/{name}")
        };
        ensure!(path.len() <= 256, "catalog path exceeds admission");
        let file = open_child(dir, name, false)?;
        let meta = file.metadata()?;
        if meta.is_dir() {
            ensure!(
                depth != 0 || matches!(name, "metadata" | "targets"),
                "unknown catalog top-level directory"
            );
            catalog_walk(&file, &path, depth + 1, files, total, dirs)?;
        } else {
            ensure!(
                depth > 0 && meta.is_file() && meta.nlink() == 1 && meta.len() <= 2 * 1024 * 1024,
                "unsafe or oversized catalog file"
            );
            let mut bytes = vec![];
            file.take(2 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() <= 2 * 1024 * 1024,
                "catalog file grew beyond admission"
            );
            *total += bytes.len();
            ensure!(
                *total <= 32 * 1024 * 1024 && files.len() < 256,
                "catalog aggregate admission exceeded"
            );
            files.insert(path, bytes);
        }
    }
    Ok(())
}
pub(super) fn catalog_command(
    state: &Path,
    request: &Request,
    session: &SessionId,
    fingerprint: &str,
) -> Result<ModelCatalogReport> {
    let (digest, refresh) = match &request.command {
        Command::RefreshModelCatalog {
            bootstrap_sha256, ..
        } => (bootstrap_sha256, true),
        Command::ModelCatalogStatus {
            bootstrap_sha256, ..
        } => (bootstrap_sha256, false),
        _ => bail!("not a catalog command"),
    };
    validate_sha256(digest)?;
    let root = catalog_root(state, refresh)?;
    if refresh {
        let names = std::fs::read_dir(format!("/proc/self/fd/{}", root.as_raw_fd()))?
            .take(129)
            .map(|e| e.map(|e| e.file_name()))
            .collect::<std::io::Result<Vec<_>>>()?;
        ensure!(
            names.len() <= 128,
            "catalog namespace inventory exceeds admission"
        );
        ensure!(
            names
                .iter()
                .any(|n| n == std::ffi::OsStr::new(&format!("{digest}.lock")))
                || names.len() < 128,
            "catalog namespace inventory is full"
        );
    }
    let lock = if refresh {
        Some(catalog_lock(&root, digest)?)
    } else {
        None
    };
    let previous = catalog_load(&root, digest)?;
    if !refresh {
        return Ok(previous.context("catalog has not been accepted")?.report);
    }
    let key = format!(
        "{:x}",
        Sha256::digest(format!("{}\0{}", session.as_str(), request.request_id.as_str()).as_bytes())
    );
    if let Some(receipt) = previous.as_ref().and_then(|p| p.receipts.get(&key)) {
        ensure!(
            receipt.fingerprint == fingerprint,
            "catalog request identity conflicts with original intent"
        );
        return Ok(receipt.report.clone());
    }
    let Command::RefreshModelCatalog {
        source,
        bootstrap,
        expected_catalog_revision,
        approve_bootstrap,
        ..
    } = &request.command
    else {
        bail!("not a refresh")
    };
    ensure!(*approve_bootstrap, "explicit bootstrap approval required");
    let revision = previous.as_ref().map_or(0, |p| p.report.catalog_revision);
    ensure!(
        revision == *expected_catalog_revision,
        "catalog revision changed; inspect current status before refresh"
    );
    ensure!(
        previous.as_ref().is_none_or(|p| p.receipts.len() < 64),
        "catalog replay retention is full; no trust state was replaced"
    );
    if previous.is_none() {
        let entries = std::fs::read_dir(format!("/proc/self/fd/{}", root.as_raw_fd()))?
            .take(129)
            .collect::<std::io::Result<Vec<_>>>()?;
        ensure!(
            entries.len() < 128,
            "catalog namespace inventory has no room for its first snapshot"
        );
    }
    let source_root = source_directory(source)?;
    let mut files = std::collections::BTreeMap::new();
    catalog_walk(&source_root, "", 0, &mut files, &mut 0, &mut 0)?;
    let root_parent = source_directory(bootstrap.parent().context("bootstrap parent absent")?)?;
    let root_name = bootstrap
        .file_name()
        .and_then(|n| n.to_str())
        .context("nonportable bootstrap filename")?;
    let root_bytes = read_bounded(&root_parent, root_name, 2 * 1024 * 1024)?;
    let prior = previous.as_ref().map(catalog_state).transpose()?;
    let verified = crate::offline_catalog::verify(&root_bytes, digest, prior.as_ref(), files)?;
    let report = ModelCatalogReport {
        schema_version: 1,
        bootstrap_sha256: digest.clone(),
        current_root_sha256: format!("{:x}", Sha256::digest(&verified.state.cache["root.json"])),
        catalog_revision: revision + 1,
        root_version: verified.state.root_version,
        timestamp_version: verified.timestamp_version,
        snapshot_version: verified.snapshot_version,
        targets_version: verified.targets_version,
        manifest_sha256s: verified.manifests.keys().cloned().collect(),
        accepted_by: session.clone(),
        accepted_request: request.request_id.clone(),
        authenticated_at_acceptance: true,
        current_publisher_trust: false,
        native_execution_approved: false,
        rights_verified: false,
        compatible: false,
        qualified: false,
    };
    report.validate()?;
    let mut receipts = previous.map_or_else(std::collections::BTreeMap::new, |p| p.receipts);
    receipts.insert(
        key,
        CatalogReplay {
            fingerprint: fingerprint.into(),
            report: report.clone(),
        },
    );
    let payload = CatalogPayload {
        schema_version: 1,
        bootstrap: catalog_encode(&root_bytes),
        cache: verified
            .state
            .cache
            .iter()
            .map(|(k, v)| (k.clone(), catalog_encode(v)))
            .collect(),
        manifests: verified
            .manifests
            .iter()
            .map(|(k, v)| (k.clone(), catalog_encode(v)))
            .collect(),
        report: report.clone(),
        receipts,
    };
    catalog_validate(&payload, digest)?;
    let capsule = CatalogCapsule {
        payload_sha256: format!("{:x}", Sha256::digest(serde_json::to_vec(&payload)?)),
        payload,
    };
    let bytes = serde_json::to_vec(&capsule)?;
    ensure!(
        bytes.len() <= CATALOG_CAPSULE_LIMIT,
        "catalog capsule exceeds admission"
    );
    let temporary = format!(".catalog-pending-{}", uuid::Uuid::new_v4());
    let name = CString::new(temporary.as_str())?;
    let fd = unsafe {
        libc::openat(
            root.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let result = (|| -> Result<()> {
        file.write_all(&bytes)?;
        file.sync_all()?;
        catalog_lock_matches(&root, digest, lock.as_ref().context("catalog lock absent")?)?;
        let destination = CString::new(format!("{digest}.json"))?;
        let rc = unsafe {
            libc::renameat(
                root.as_raw_fd(),
                name.as_ptr(),
                root.as_raw_fd(),
                destination.as_ptr(),
            )
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        root.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = unsafe { libc::unlinkat(root.as_raw_fd(), name.as_ptr(), 0) };
    }
    result?;
    Ok(report)
}

fn catalog_freshness(cache: &std::collections::BTreeMap<String, Vec<u8>>, now: jiff::Timestamp) -> Result<jiff::Timestamp> {
    use tough::schema::{Root, Signed, Snapshot, Targets, Timestamp};
    let root: Signed<Root> = serde_json::from_slice(&cache["root.json"])?;
    let timestamp: Signed<Timestamp> = serde_json::from_slice(&cache["timestamp.json"])?;
    let snapshot: Signed<Snapshot> = serde_json::from_slice(&cache["snapshot.json"])?;
    let targets: Signed<Targets> = serde_json::from_slice(&cache["targets.json"])?;
    let floor: jiff::Timestamp = serde_json::from_slice(&cache["latest_known_time.json"])?;
    catalog_expiry(floor, [root.signed.expires, timestamp.signed.expires, snapshot.signed.expires, targets.signed.expires], now)
}
fn catalog_expiry(floor: jiff::Timestamp, expiries: [jiff::Timestamp; 4], now: jiff::Timestamp) -> Result<jiff::Timestamp> {
    ensure!(now >= floor, "catalog check clock predates accepted watermark");
    let expiry = expiries.into_iter().min().context("missing metadata expiry")?;
    ensure!(now < expiry, "accepted catalog metadata has expired; refresh before association");
    Ok(expiry)
}
pub(super) fn catalog_pack_check(state: &Path, bootstrap: &str, manifest_hash: &str, revision: u64) -> Result<ModelPackCatalogReport> {
    validate_sha256(bootstrap)?;validate_sha256(manifest_hash)?;
    let root = catalog_root(state, false)?;
    let payload = catalog_load(&root, bootstrap)?.context("catalog has not been accepted")?;
    ensure!(payload.report.catalog_revision == revision, "catalog revision changed; inspect current status before association");
    let retained = catalog_decode(payload.manifests.get(manifest_hash).context("installed manifest is absent from accepted catalog")?, PACK_MANIFEST_LIMIT)?;
    let cache = catalog_state(&payload)?.cache;
    let started = jiff::Timestamp::now();
    catalog_freshness(&cache, started)?;
    let pack = status(state, manifest_hash)?;
    // Compare the exact original manifest, not supplier reserialization.
    let store = store(state, false)?;
    let installed = open_child(&store, manifest_hash, true)?;
    let (bytes, _, _) = manifest(&installed)?;
    ensure!(bytes == retained, "installed manifest differs from retained authenticated bytes");
    let checked = jiff::Timestamp::now();
    ensure!(checked >= started, "clock moved backwards during installed-byte check");
    let expiry = catalog_freshness(&cache, checked)?;
    let report = ModelPackCatalogReport {
        schema_version: 1, catalog: payload.report, pack,
        checked_at_unix_ms: checked.as_millisecond().try_into()?,
        metadata_expires_at_unix_ms: expiry.as_millisecond().try_into()?,
        listed_in_catalog: true, online_revocation_checked: false,
    };
    report.validate()?;
    Ok(report)
}

#[cfg(test)]
mod catalog_association_tests {
    use super::*;
    #[test]
    fn every_role_expiry_and_accepted_clock_floor_fence_association() {
        let floor: jiff::Timestamp = "2026-10-01T00:00:00Z".parse().unwrap();
        let now: jiff::Timestamp = "2026-10-02T00:00:00Z".parse().unwrap();
        let future: jiff::Timestamp = "2026-10-03T00:00:00Z".parse().unwrap();
        assert_eq!(catalog_expiry(floor,[future;4],now).unwrap(),future);
        assert!(catalog_expiry(now,[future;4],floor).is_err());
        for role in 0..4 {let mut expires=[future;4];expires[role]=now;assert!(catalog_expiry(floor,expires,now).is_err());expires[role]=floor;assert!(catalog_expiry(floor,expires,now).is_err());}
        assert!(catalog_expiry(floor,[future;4],future).is_err());
    }
}

pub(super) fn catalog_binding_check(state: &Path, manifest: &str, binding: &ModelCatalogBinding) -> Result<()> {
    binding.validate()?;validate_sha256(manifest)?;
    let root=catalog_root(state,false)?;
    let payload=catalog_load(&root,&binding.bootstrap_sha256)?.context("bound catalog is absent")?;
    ensure!(payload.report.catalog_revision == binding.catalog_revision,"bound catalog revision changed; explicit reapproval is required");
    ensure!(payload.manifests.contains_key(manifest),"bound catalog no longer lists selected manifest");
    catalog_freshness(&catalog_state(&payload)?.cache,jiff::Timestamp::now())?;
    Ok(())
}
