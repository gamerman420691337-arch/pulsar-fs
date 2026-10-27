//! Bounded TUF verification over captured bytes. No supplier filesystem or network transport.
//! The caller must independently approve the bootstrap root and atomically retain returned state.
use anyhow::{ensure, Context, Result};
use bytes::Bytes;
use pulsar_protocol::{validate_sha256, ModelPackManifest, PACK_MANIFEST_LIMIT};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tough::{
    IntoVec, RepositoryLoader, Transport, TransportError, TransportErrorKind, TransportStream,
};
use url::Url;

const FILE_LIMIT: usize = 2 * 1024 * 1024;
const TOTAL_LIMIT: usize = 32 * 1024 * 1024;
const FILE_COUNT: usize = 256;
const CACHE_NAMES: [&str; 5] = [
    "root.json",
    "latest_known_time.json",
    "timestamp.json",
    "snapshot.json",
    "targets.json",
];

/// Private engine state, not supplier input or transferable authority.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogState {
    pub bootstrap_sha256: String,
    pub root_version: u64,
    pub cache: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Serialize)]
pub struct VerifiedCatalog {
    pub state: CatalogState,
    pub timestamp_version: u64,
    pub snapshot_version: u64,
    pub targets_version: u64,
    /// Validated manifests are publisher-authenticated declarations, not qualified assets.
    pub manifests: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug)]
struct CapturedTransport {
    files: Arc<BTreeMap<String, Vec<u8>>>,
    requests: Arc<AtomicUsize>,
    fetched_bytes: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl Transport for CapturedTransport {
    async fn fetch(&self, url: Url) -> std::result::Result<TransportStream, TransportError> {
        if self.requests.fetch_add(1, Ordering::Relaxed) >= 256 {
            return Err(TransportError::new(
                TransportErrorKind::Other,
                "catalog request admission exceeded",
            ));
        }
        let prefix = "pulsar-catalog://offline/";
        let key = url
            .as_str()
            .strip_prefix(prefix)
            .filter(|_| url.query().is_none() && url.fragment().is_none());
        let Some(key) = key else {
            return Err(TransportError::new(
                TransportErrorKind::UnsupportedUrlScheme,
                url.as_str(),
            ));
        };
        let Some(bytes) = self.files.get(key) else {
            return Err(TransportError::new(
                TransportErrorKind::FileNotFound,
                url.as_str(),
            ));
        };
        if self.fetched_bytes.fetch_add(bytes.len(), Ordering::Relaxed) + bytes.len()
            > 2 * TOTAL_LIMIT
        {
            return Err(TransportError::new(
                TransportErrorKind::Other,
                "catalog fetched-byte admission exceeded",
            ));
        }
        let bytes = Bytes::copy_from_slice(bytes);
        Ok(Box::pin(futures_util::stream::once(std::future::ready(
            Ok(bytes),
        ))))
    }
}

fn valid_capture_name(name: &str) -> bool {
    name.len() <= 256
        && name.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
        && (name.starts_with("metadata/") || name.starts_with("targets/"))
}

/// Verify an offline captured repository against an independently approved exact root.
///
/// `prior` must come from protected engine storage. This function never commits it: failure
/// leaves it unchanged, and success returns the entire successor for atomic persistence.
/// First use verifies from the approved bootstrap. Later rotations start from the protected
/// verified root, so key-change recovery cannot reset watermarks on every refresh. At most
/// 32 updates per refresh are admitted. Cache metadata and clock facts must be retained.
/// All target streams are consumed and checked before any manifest is returned.
pub fn verify(
    bootstrap: &[u8],
    expected_bootstrap_sha256: &str,
    prior: Option<&CatalogState>,
    captured: BTreeMap<String, Vec<u8>>,
) -> Result<VerifiedCatalog> {
    validate_sha256(expected_bootstrap_sha256)?;
    ensure!(
        !bootstrap.is_empty() && bootstrap.len() <= FILE_LIMIT,
        "Catalog bootstrap exceeds admission"
    );
    ensure!(
        format!("{:x}", Sha256::digest(bootstrap)) == expected_bootstrap_sha256,
        "Catalog bootstrap identity mismatch"
    );
    ensure!(
        captured.len() <= FILE_COUNT,
        "Catalog file count exceeds admission"
    );
    let mut total = 0usize;
    for (name, bytes) in &captured {
        ensure!(
            valid_capture_name(name) && bytes.len() <= FILE_LIMIT,
            "Unsafe or oversized captured catalog file"
        );
        total = total
            .checked_add(bytes.len())
            .context("Catalog byte count overflow")?;
        ensure!(
            total <= TOTAL_LIMIT,
            "Catalog aggregate bytes exceed admission"
        );
    }
    let mut builder = tempfile::Builder::new();
    builder.prefix("pulsar-catalog-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let scratch = builder.tempdir()?;
    if let Some(prior) = prior {
        ensure!(
            prior.bootstrap_sha256 == expected_bootstrap_sha256 && prior.root_version > 0,
            "Catalog retained bootstrap binding changed"
        );
        ensure!(
            prior.cache.len() == CACHE_NAMES.len(),
            "Incomplete retained catalog state"
        );
        for name in CACHE_NAMES {
            let bytes = prior
                .cache
                .get(name)
                .context("Missing retained catalog role or clock")?;
            ensure!(
                !bytes.is_empty() && bytes.len() <= FILE_LIMIT,
                "Retained catalog cache exceeds admission"
            );
            // Validate syntax before the client; it treats malformed old metadata as absent.
            use tough::schema::{Root, Signed, Snapshot, Targets, Timestamp};
            let parsed = match name {
                "latest_known_time.json" => {
                    serde_json::from_slice::<jiff::Timestamp>(bytes).map(|_| ())
                }
                "timestamp.json" => serde_json::from_slice::<Signed<Timestamp>>(bytes).map(|_| ()),
                "snapshot.json" => serde_json::from_slice::<Signed<Snapshot>>(bytes).map(|_| ()),
                "targets.json" => serde_json::from_slice::<Signed<Targets>>(bytes).map(|_| ()),
                "root.json" => serde_json::from_slice::<Signed<Root>>(bytes).map(|_| ()),
                _ => unreachable!(),
            };
            parsed.context("Corrupt retained catalog state")?;
            std::fs::write(scratch.path().join(name), bytes)?;
        }
    }
    let trusted_root = match prior {
        Some(state) => state
            .cache
            .get("root.json")
            .context("Missing retained catalog root")?
            .as_slice(),
        None => bootstrap,
    };
    let root: tough::schema::Signed<tough::schema::Root> = serde_json::from_slice(trusted_root)?;
    ensure!(
        root.signed.version.get() <= i64::MAX as u64,
        "Catalog root version exceeds admission"
    );
    ensure!(
        prior.is_none_or(|state| root.signed.version.get() == state.root_version),
        "Retained catalog root version disagrees with state"
    );
    ensure!(
        tokio::runtime::Handle::try_current().is_err(),
        "Synchronous catalog verifier cannot run inside an async runtime"
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    runtime.block_on(async {
        let repository = RepositoryLoader::new(
            &trusted_root,
            Url::parse("pulsar-catalog://offline/metadata/")?,
            Url::parse("pulsar-catalog://offline/targets/")?,
        )
        .transport(CapturedTransport {
            files: Arc::new(captured),
            requests: Arc::new(AtomicUsize::new(0)),
            fetched_bytes: Arc::new(AtomicUsize::new(0)),
        })
        .datastore(scratch.path())
        .expiration_enforcement(tough::ExpirationEnforcement::Safe)
        .limits(tough::Limits {
            max_root_size: FILE_LIMIT as u64,
            max_targets_size: FILE_LIMIT as u64,
            max_timestamp_size: FILE_LIMIT as u64,
            max_snapshot_size: FILE_LIMIT as u64,
            max_root_updates: 32,
        })
        .load()
        .await
        .context("Offline catalog authentication failed")?;
        let root_version = repository.root().signed.version.get();
        ensure!(
            root_version <= i64::MAX as u64,
            "Catalog rotated root version exceeds admission"
        );
        ensure!(
            prior.is_none_or(|p| root_version >= p.root_version),
            "Catalog root version rollback"
        );
        ensure!(
            repository.targets().signed.delegations.is_none(),
            "Delegated catalog roles are not admitted"
        );
        let targets = &repository.targets().signed.targets;
        ensure!(
            targets.len() <= 128,
            "Catalog target count exceeds admission"
        );
        let mut manifests = BTreeMap::new();
        for (name, target) in targets {
            ensure!(name.raw() == name.resolved(), "Catalog target path alias");
            let digest = name
                .raw()
                .strip_prefix("manifests/")
                .and_then(|n| n.strip_suffix(".json"))
                .context("Catalog target must be an exact manifest digest")?;
            validate_sha256(digest)?;
            ensure!(
                target.length > 0 && target.length <= PACK_MANIFEST_LIMIT as u64,
                "Catalog manifest exceeds admission"
            );
            let bytes = repository
                .read_target(name)
                .await?
                .context("Catalog target absent")?
                .into_vec()
                .await?;
            ensure!(
                format!("{:x}", Sha256::digest(&bytes)) == digest,
                "Catalog target manifest-name identity mismatch"
            );
            let manifest: ModelPackManifest = serde_json::from_slice(&bytes)?;
            manifest.validate()?;
            manifests.insert(digest.to_owned(), bytes);
        }
        let mut cache = BTreeMap::new();
        for name in CACHE_NAMES {
            let bytes = std::fs::read(scratch.path().join(name))?;
            ensure!(
                !bytes.is_empty() && bytes.len() <= FILE_LIMIT,
                "Verified catalog cache exceeds admission"
            );
            cache.insert(name.into(), bytes);
        }
        Ok(VerifiedCatalog {
            state: CatalogState {
                bootstrap_sha256: expected_bootstrap_sha256.into(),
                root_version,
                cache,
            },
            timestamp_version: repository.timestamp().signed.version.get(),
            snapshot_version: repository.snapshot().signed.version.get(),
            targets_version: repository.targets().signed.version.get(),
            manifests,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::{
        rand::SystemRandom,
        signature::{Ed25519KeyPair, KeyPair},
    };
    use serde_json::{json, Value};

    // First-party ephemeral fixture keys exist only in memory. No signing credentials are stored.
    struct Fixture {
        keys: Vec<Ed25519KeyPair>,
        bootstrap: Vec<u8>,
        digest: String,
        manifest: Vec<u8>,
    }
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
    fn hash(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }
    fn canonical(value: &Value) -> String {
        match value {
            Value::Object(map) => {
                let mut names: Vec<_> = map.keys().collect();
                names.sort();
                format!(
                    "{{{}}}",
                    names
                        .into_iter()
                        .map(|k| format!(
                            "{}:{}",
                            serde_json::to_string(k).unwrap(),
                            canonical(&map[k])
                        ))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }
            Value::Array(values) => format!(
                "[{}]",
                values.iter().map(canonical).collect::<Vec<_>>().join(",")
            ),
            _ => serde_json::to_string(value).unwrap(),
        }
    }
    impl Fixture {
        fn key(&self, index: usize) -> Value {
            json!({"keytype":"ed25519","scheme":"ed25519","keyval":{"public":hex(self.keys[index].public_key().as_ref())}})
        }
        fn id(&self, index: usize) -> String {
            hash(canonical(&self.key(index)).as_bytes())
        }
        fn signed(&self, value: Value, indices: &[usize]) -> Vec<u8> {
            let message = canonical(&value);
            let signatures: Vec<_> = indices.iter().map(|i| json!({"keyid":self.id(*i),"sig":hex(self.keys[*i].sign(message.as_bytes()).as_ref())})).collect();
            serde_json::to_vec(&json!({"signed":value,"signatures":signatures})).unwrap()
        }
        fn root(&self, version: u64, roots: &[usize], signers: &[usize]) -> Vec<u8> {
            self.root_for_roles(version, roots, signers, 4, 3)
        }
        fn root_for_roles(
            &self,
            version: u64,
            roots: &[usize],
            signers: &[usize],
            timestamp_key: usize,
            snapshot_key: usize,
        ) -> Vec<u8> {
            let mut keys = serde_json::Map::new();
            for i in roots
                .iter()
                .copied()
                .chain([2, snapshot_key, timestamp_key])
            {
                keys.insert(self.id(i), self.key(i));
            }
            self.signed(json!({"_type":"root","spec_version":"1.0.0","version":version,"expires":"2099-01-01T00:00:00Z","consistent_snapshot":false,"keys":keys,
                "roles":{"root":{"keyids":roots.iter().map(|i|self.id(*i)).collect::<Vec<_>>(),"threshold":2},"targets":{"keyids":[self.id(2)],"threshold":1},"snapshot":{"keyids":[self.id(snapshot_key)],"threshold":1},"timestamp":{"keyids":[self.id(timestamp_key)],"threshold":1}}}), signers)
        }
        fn new() -> Self {
            let keys = (0..9)
                .map(|_| {
                    Ed25519KeyPair::from_pkcs8(
                        Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
                            .unwrap()
                            .as_ref(),
                    )
                    .unwrap()
                })
                .collect();
            let assets: Vec<_> = [("model.onnx","model"),("runtime.so","runtime"),("profile.json","profile"),("LICENSE","license")].iter().map(|(path,role)|json!({"path":path,"role":role,"byte_len":1,"sha256":"a".repeat(64)})).collect();
            let manifest = serde_json::to_vec(&json!({"schema_version":1,"pack_id":"authored-catalog-fixture","target_os":"linux","target_arch":"x86_64","ort_api_version":24,"labels":["fixture"],"assets":assets,"license_path":"LICENSE","source_description":"First-party inert test declarations"})).unwrap();
            let mut fixture = Self {
                keys,
                bootstrap: vec![],
                digest: String::new(),
                manifest,
            };
            fixture.bootstrap = fixture.root(1, &[0, 1], &[0, 1]);
            fixture.digest = hash(&fixture.bootstrap);
            fixture
        }
        fn capture(&self, version: u64, expired: Option<&str>) -> BTreeMap<String, Vec<u8>> {
            self.capture_for_roles(version, expired, 4, 3)
        }
        fn capture_for_roles(
            &self,
            version: u64,
            expired: Option<&str>,
            timestamp_key: usize,
            snapshot_key: usize,
        ) -> BTreeMap<String, Vec<u8>> {
            let expires = |role| {
                if expired == Some(role) {
                    "2000-01-01T00:00:00Z"
                } else {
                    "2099-01-01T00:00:00Z"
                }
            };
            let name = format!("manifests/{}.json", hash(&self.manifest));
            let targets = self.signed(json!({"_type":"targets","spec_version":"1.0.0","version":version,"expires":expires("targets"),"targets":{name.clone():{"length":self.manifest.len(),"hashes":{"sha256":hash(&self.manifest)}}}}), &[2]);
            let snapshot = self.signed(json!({"_type":"snapshot","spec_version":"1.0.0","version":version,"expires":expires("snapshot"),"meta":{"targets.json":{"version":version,"length":targets.len(),"hashes":{"sha256":hash(&targets)}}}}), &[snapshot_key]);
            let timestamp = self.signed(json!({"_type":"timestamp","spec_version":"1.0.0","version":version,"expires":expires("timestamp"),"meta":{"snapshot.json":{"version":version,"length":snapshot.len(),"hashes":{"sha256":hash(&snapshot)}}}}), &[timestamp_key]);
            [
                ("metadata/targets.json".into(), targets),
                ("metadata/snapshot.json".into(), snapshot),
                ("metadata/timestamp.json".into(), timestamp),
                (format!("targets/{name}"), self.manifest.clone()),
            ]
            .into()
        }
    }

    #[test]
    fn signed_catalog_preserves_original_manifest_and_refuses_metadata_rollback() {
        let f = Fixture::new();
        let first = verify(&f.bootstrap, &f.digest, None, f.capture(1, None)).unwrap();
        assert_eq!(first.manifests.values().next().unwrap(), &f.manifest);
        let second = verify(
            &f.bootstrap,
            &f.digest,
            Some(&first.state),
            f.capture(2, None),
        )
        .unwrap();
        assert_eq!(second.targets_version, 2);
        let before = serde_json::to_vec(&second.state).unwrap();
        assert!(verify(
            &f.bootstrap,
            &f.digest,
            Some(&second.state),
            f.capture(1, None)
        )
        .is_err());
        assert_eq!(serde_json::to_vec(&second.state).unwrap(), before);
        assert!(verify(&f.bootstrap, &"b".repeat(64), None, f.capture(1, None)).is_err());
    }

    #[test]
    fn signatures_expiration_stream_integrity_and_missing_targets_refuse() {
        let f = Fixture::new();
        for role in ["timestamp", "snapshot", "targets"] {
            assert!(
                verify(&f.bootstrap, &f.digest, None, f.capture(1, Some(role))).is_err(),
                "expired {role}"
            );
        }
        let mut captured = f.capture(1, None);
        let target = captured
            .keys()
            .find(|key| key.starts_with("targets/"))
            .unwrap()
            .clone();
        captured.get_mut(&target).unwrap()[0] = b'x';
        assert!(verify(&f.bootstrap, &f.digest, None, captured).is_err());
        let oversized_version = f.root(u64::MAX, &[0, 1], &[0, 1]);
        assert!(verify(
            &oversized_version,
            &hash(&oversized_version),
            None,
            f.capture(1, None)
        )
        .is_err());
        let root = f.root(1, &[0, 1], &[0, 0]);
        assert!(
            verify(&root, &hash(&root), None, f.capture(1, None)).is_err(),
            "one root key signed twice is not a quorum"
        );
        let mut root: Value = serde_json::from_slice(&f.bootstrap).unwrap();
        root["signed"]["expires"] = json!("2000-01-01T00:00:00Z");
        let root = f.signed(root["signed"].clone(), &[0, 1]);
        assert!(
            verify(&root, &hash(&root), None, f.capture(1, None)).is_err(),
            "expired bootstrap cannot authorize intake"
        );
        let mut captured = f.capture(1, None);
        captured.remove(&target);
        assert!(verify(&f.bootstrap, &f.digest, None, captured).is_err());
        let mut captured = f.capture(1, None);
        let mut timestamp: Value =
            serde_json::from_slice(&captured["metadata/timestamp.json"]).unwrap();
        timestamp["signatures"][0]["sig"] = json!("00".repeat(64));
        captured.insert(
            "metadata/timestamp.json".into(),
            serde_json::to_vec(&timestamp).unwrap(),
        );
        assert!(verify(&f.bootstrap, &f.digest, None, captured).is_err());
    }

    #[test]
    fn root_rotation_needs_both_thresholds_and_cannot_replay_an_older_root() {
        let f = Fixture::new();
        let mut captured = f.capture(1, None);
        captured.insert(
            "metadata/2.root.json".into(),
            f.root(2, &[5, 6], &[0, 1, 5, 6]),
        );
        let rotated = verify(&f.bootstrap, &f.digest, None, captured).unwrap();
        assert_eq!(rotated.state.root_version, 2);
        let retained = verify(
            &f.bootstrap,
            &f.digest,
            Some(&rotated.state),
            f.capture(1, None),
        )
        .unwrap();
        assert_eq!(
            retained.state.root_version, 2,
            "absence of old root chain does not roll back the retained root"
        );
        let mut stale_root = f.capture(1, None);
        stale_root.insert("metadata/3.root.json".into(), f.root(1, &[5, 6], &[5, 6]));
        assert!(verify(&f.bootstrap, &f.digest, Some(&rotated.state), stale_root).is_err());
        for signers in [&[0, 1, 5][..], &[0, 5, 6][..], &[0, 0, 5, 6][..]] {
            let mut captured = f.capture(1, None);
            captured.insert("metadata/2.root.json".into(), f.root(2, &[5, 6], signers));
            assert!(
                verify(&f.bootstrap, &f.digest, None, captured).is_err(),
                "threshold {signers:?}"
            );
        }
    }

    #[test]
    fn unsafe_and_oversized_inputs_and_corrupt_retained_cache_are_not_reset() {
        let f = Fixture::new();
        let verified = verify(&f.bootstrap, &f.digest, None, f.capture(1, None)).unwrap();
        for name in [
            "metadata/../targets.json",
            "targets/%2e%2e/x",
            "https://example.invalid/root.json",
        ] {
            let mut captured = f.capture(1, None);
            captured.insert(name.into(), vec![0]);
            assert!(verify(&f.bootstrap, &f.digest, None, captured).is_err());
        }
        let mut captured = f.capture(1, None);
        captured.insert("metadata/large.json".into(), vec![0; FILE_LIMIT + 1]);
        assert!(verify(&f.bootstrap, &f.digest, None, captured).is_err());
        for name in CACHE_NAMES {
            let mut state = verified.state.clone();
            state.cache.insert(name.into(), b"{}".to_vec());
            let before = serde_json::to_vec(&state).unwrap();
            assert!(verify(&f.bootstrap, &f.digest, Some(&state), f.capture(2, None)).is_err());
            assert_eq!(serde_json::to_vec(&state).unwrap(), before);
        }
        let mut state = verified.state.clone();
        state.cache.insert(
            "latest_known_time.json".into(),
            br#""2098-01-01T00:00:00Z""#.to_vec(),
        );
        assert!(
            verify(&f.bootstrap, &f.digest, Some(&state), f.capture(2, None)).is_err(),
            "wall clock cannot roll back retained watermark"
        );
    }

    #[test]
    fn repeated_rotated_root_refresh_preserves_new_role_watermarks() {
        let f = Fixture::new();
        let root = f.root_for_roles(2, &[5, 6], &[0, 1, 5, 6], 7, 8);
        let mut current = f.capture_for_roles(2, None, 7, 8);
        current.insert("metadata/2.root.json".into(), root.clone());
        let accepted = verify(&f.bootstrap, &f.digest, None, current).unwrap();
        let mut stale = f.capture_for_roles(1, None, 7, 8);
        stale.insert("metadata/2.root.json".into(), root);
        let before = serde_json::to_vec(&accepted.state).unwrap();
        assert!(
            verify(&f.bootstrap, &f.digest, Some(&accepted.state), stale).is_err(),
            "reapplying a previously accepted key rotation must not reset newer metadata versions"
        );
        assert_eq!(serde_json::to_vec(&accepted.state).unwrap(), before);
    }

    #[test]
    fn authenticated_invalid_manifest_never_returns_successor_state() {
        let mut f = Fixture::new();
        let mut manifest: Value = serde_json::from_slice(&f.manifest).unwrap();
        manifest["assets"][0]["path"] = json!("../escape.onnx");
        f.manifest = serde_json::to_vec(&manifest).unwrap();
        assert!(
            verify(&f.bootstrap, &f.digest, None, f.capture(1, None)).is_err(),
            "valid publisher signatures do not admit unsafe manifests"
        );
    }
}
