//! Inert generation pack delivery. Installation never selects or executes a pack.
use super::*;
use pulsar_protocol::{ModelPackManifest, PACK_MANIFEST_LIMIT};
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GenerationPack {
    manifest_sha256: String,
    manifest: ModelPackManifest,
    native_execution_approval_required: bool,
    qualified: bool,
}
type AssetIdentities = BTreeMap<String, (Option<u64>, String)>;
#[derive(serde::Serialize)]
pub struct BundledGenerationReport {
    schema_version: u16,
    runtime_manifest_sha256: String,
    directory: std::path::PathBuf,
    packs: Vec<BundledGenerationPack>,
    integrity_verified: bool,
    native_execution_approved: bool,
    rights_verified: bool,
    qualified: bool,
}
impl BundledGenerationReport {
    pub fn json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}
#[derive(serde::Serialize)]
struct BundledGenerationPack {
    manifest_sha256: String,
    source: std::path::PathBuf,
    manifest: ModelPackManifest,
}
pub(super) fn report(
    directory: &Path,
    hash: &str,
    packs: &[GenerationPack],
) -> BundledGenerationReport {
    BundledGenerationReport {
        schema_version: 1,
        runtime_manifest_sha256: hash.to_owned(),
        directory: directory.to_owned(),
        packs: packs
            .iter()
            .map(|p| BundledGenerationPack {
                manifest_sha256: p.manifest_sha256.clone(),
                source: directory.join("generation").join(&p.manifest_sha256),
                manifest: p.manifest.clone(),
            })
            .collect(),
        integrity_verified: true,
        native_execution_approved: false,
        rights_verified: false,
        qualified: false,
    }
}
pub(super) fn inventory(
    packs: &[GenerationPack],
    expected: &mut BTreeSet<String>,
) -> Result<AssetIdentities> {
    ensure!(packs.len() <= 8, "too many bundled generation packs");
    let mut identities = BTreeMap::new();
    for pack in packs {
        ensure!(
            pack.native_execution_approval_required && !pack.qualified,
            "generation pack cannot grant execution or qualification"
        );
        ensure!(
            pack.manifest_sha256.len() == 64
                && pack
                    .manifest_sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid generation manifest identity"
        );
        pack.manifest.validate()?;
        ensure!(
            pack.manifest.target_os == "linux" && pack.manifest.target_arch == "x86_64",
            "generation pack target differs from runtime candidate"
        );
        let prefix = format!("generation/{}/", pack.manifest_sha256);
        let name = format!("{prefix}manifest.json");
        ensure!(expected.insert(name.clone()), "duplicate generation pack");
        identities.insert(name, (None, pack.manifest_sha256.clone()));
        for asset in &pack.manifest.assets {
            let name = format!("{prefix}{}", asset.path);
            ensure!(
                expected.insert(name.clone()),
                "duplicate generation payload"
            );
            identities.insert(name, (Some(asset.byte_len), asset.sha256.clone()));
        }
    }
    for name in expected.iter() {
        for parent in Path::new(name)
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            ensure!(
                !expected.contains(parent.to_str().context("invalid payload path")?),
                "payload path collides with a directory"
            );
        }
    }
    Ok(identities)
}
pub(super) fn directories(records: &[Record]) -> BTreeSet<String> {
    records
        .iter()
        .flat_map(|record| {
            Path::new(&record.path)
                .ancestors()
                .skip(1)
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| p.to_string_lossy().into_owned())
        })
        .collect()
}
pub(super) fn verify(root: &File, packs: &[GenerationPack]) -> Result<()> {
    for pack in packs {
        let name = format!("generation/{}/manifest.json", pack.manifest_sha256);
        let mut input = source_file(root, &name)?;
        let before = input.metadata()?;
        ensure!(
            before.len() <= PACK_MANIFEST_LIMIT as u64,
            "generation manifest exceeds admission"
        );
        let mut bytes = Vec::new();
        (&mut input)
            .take(PACK_MANIFEST_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= PACK_MANIFEST_LIMIT
                && bytes.len() as u64 == before.len()
                && stable(&before, &input.metadata()?)
                && format!("{:x}", Sha256::digest(&bytes)) == pack.manifest_sha256,
            "generation manifest integrity differs"
        );
        let actual: ModelPackManifest = serde_json::from_slice(&bytes)?;
        actual.validate()?;
        ensure!(
            serde_json::to_value(actual)? == serde_json::to_value(&pack.manifest)?,
            "generation manifest differs from embedded declaration"
        );
    }
    Ok(())
}
