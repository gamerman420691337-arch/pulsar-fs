//! Path-free declarations for an owner-selected CPU library image.
//! A content identity is not byte verification, native execution or qualification.
use crate::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MAX_CPU_OWNER_POLICY_BYTES: usize = 1024 * 1024;
pub const CPU_RUNTIME_IMAGE_ID_PREFIX: &str = "cpu-runtime-image-";
pub const CPU_RUNTIME_REQUIRED_ALIASES: [&str; 3] =
    ["ld-linux-x86-64.so.2", "libc.so.6", "libonnxruntime.so.1"];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CpuRuntimeImage {
    pub schema_version: u16,
    pub owner_policy: ArtifactIdentity,
    /// Strictly ascending content hashes, with strictly ascending aliases.
    pub libraries: Vec<CapturedNativeLibrary>,
}

impl CpuRuntimeImage {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let invalid = || ProtocolError::invalid("invalid or noncanonical CPU runtime image");
        self.owner_policy.validate()?;
        if self.schema_version != 1
            || self.owner_policy.artifact_id.as_str() != self.owner_policy.sha256
            || self.owner_policy.byte_len == 0
            || self.owner_policy.byte_len > MAX_CPU_OWNER_POLICY_BYTES as u64
            || self.libraries.is_empty()
            || self.libraries.len() > MAX_NATIVE_LIBRARIES
        {
            return Err(invalid());
        }
        let mut previous_hash: Option<&str> = None;
        let mut aliases = BTreeSet::new();
        let mut total = 0u64;
        for library in &self.libraries {
            library.identity.validate()?;
            let hash = library.identity.sha256.as_str();
            if library.identity.artifact_id.as_str() != hash
                || previous_hash.is_some_and(|previous| previous >= hash)
                || library.identity.byte_len == 0
                || library.identity.byte_len > MAX_NATIVE_LIBRARY_BYTES
                || library.names.is_empty()
                || library.names.len() > 4
            {
                return Err(invalid());
            }
            previous_hash = Some(hash);
            let mut previous_alias: Option<&str> = None;
            for name in &library.names {
                if name.is_empty()
                    || name.len() > 128
                    || name == "."
                    || name == ".."
                    || !name.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+')
                    })
                    || previous_alias.is_some_and(|previous| previous >= name.as_str())
                    || !aliases.insert(name.as_str())
                {
                    return Err(invalid());
                }
                previous_alias = Some(name.as_str());
            }
            total = total
                .checked_add(library.identity.byte_len)
                .ok_or_else(invalid)?;
            if total > MAX_NATIVE_TOTAL_BYTES {
                return Err(invalid());
            }
        }
        if CPU_RUNTIME_REQUIRED_ALIASES
            .iter()
            .any(|name| !aliases.contains(name))
        {
            return Err(ProtocolError::invalid(
                "CPU runtime image lacks a required loader, libc or runtime alias",
            ));
        }
        Ok(())
    }

    /// Digest the canonical typed declaration, including the exact owner-policy
    /// identity. This does not read or authenticate any library on the host.
    pub fn identity(&self) -> Result<ArtifactIdentity, ProtocolError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)
            .map_err(|_| ProtocolError::invalid("CPU runtime image serialization failed"))?;
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        Ok(ArtifactIdentity {
            artifact_id: ArtifactId::new(format!("{CPU_RUNTIME_IMAGE_ID_PREFIX}{sha256}"))
                .map_err(|error| ProtocolError::invalid(error.to_string()))?,
            sha256,
            byte_len: bytes.len() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(index: u8) -> ArtifactIdentity {
        let sha256 = format!("{index:064x}");
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: 1,
        }
    }

    fn fixture() -> CpuRuntimeImage {
        CpuRuntimeImage {
            schema_version: 1,
            owner_policy: identity(9),
            libraries: CPU_RUNTIME_REQUIRED_ALIASES
                .iter()
                .enumerate()
                .map(|(index, alias)| CapturedNativeLibrary {
                    identity: identity(index as u8),
                    names: vec![(*alias).into()],
                })
                .collect(),
        }
    }

    #[test]
    fn canonical_roundtrip_and_identity_bind_every_declared_input() {
        let image = fixture();
        let encoded = serde_json::to_vec(&image).unwrap();
        let decoded: CpuRuntimeImage = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(image.identity().unwrap(), decoded.identity().unwrap());
        assert_eq!(image.identity().unwrap().byte_len, encoded.len() as u64);
        assert_eq!(
            image.identity().unwrap().sha256,
            format!("{:x}", Sha256::digest(&encoded))
        );
        assert_eq!(
            image.identity().unwrap().artifact_id.as_str(),
            format!(
                "{CPU_RUNTIME_IMAGE_ID_PREFIX}{:x}",
                Sha256::digest(&encoded)
            )
        );
        for mutate in [
            |image: &mut CpuRuntimeImage| image.owner_policy = identity(10),
            |image: &mut CpuRuntimeImage| image.libraries[0].identity.byte_len += 1,
            |image: &mut CpuRuntimeImage| image.libraries[2].names.push("runtime-alias.so".into()),
        ] {
            let mut changed = image.clone();
            mutate(&mut changed);
            assert_ne!(image.identity().unwrap(), changed.identity().unwrap());
        }
        let mut unknown: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        unknown["qualified"] = true.into();
        assert!(serde_json::from_value::<CpuRuntimeImage>(unknown).is_err());
    }

    #[test]
    fn noncanonical_order_duplicates_paths_and_missing_required_aliases_refuse() {
        for mutate in [
            |image: &mut CpuRuntimeImage| image.libraries.swap(0, 1),
            |image: &mut CpuRuntimeImage| image.libraries[1] = image.libraries[0].clone(),
            |image: &mut CpuRuntimeImage| {
                image.libraries[1].names = image.libraries[0].names.clone()
            },
            |image: &mut CpuRuntimeImage| image.libraries[0].names.push("aaa.so".into()),
            |image: &mut CpuRuntimeImage| image.libraries[0].names.push("/host/library".into()),
            |image: &mut CpuRuntimeImage| image.libraries[0].names.push("../library".into()),
            |image: &mut CpuRuntimeImage| image.libraries[0].names.clear(),
            |image: &mut CpuRuntimeImage| {
                image.libraries[0].names[0] = "different-loader.so".into()
            },
            |image: &mut CpuRuntimeImage| {
                image.libraries[0].identity.artifact_id = ArtifactId::new("alias").unwrap();
            },
            |image: &mut CpuRuntimeImage| {
                image.owner_policy.artifact_id = ArtifactId::new("alias").unwrap();
            },
        ] {
            let mut changed = fixture();
            mutate(&mut changed);
            assert!(changed.validate().is_err());
            assert!(changed.identity().is_err());
        }
    }

    #[test]
    fn shared_native_size_and_alias_limits_apply() {
        for mutate in [
            |image: &mut CpuRuntimeImage| image.schema_version = 2,
            |image: &mut CpuRuntimeImage| image.owner_policy.byte_len = 0,
            |image: &mut CpuRuntimeImage| {
                image.owner_policy.byte_len = MAX_CPU_OWNER_POLICY_BYTES as u64 + 1;
            },
            |image: &mut CpuRuntimeImage| image.libraries[0].identity.byte_len = 0,
            |image: &mut CpuRuntimeImage| {
                image.libraries[0].identity.byte_len = MAX_NATIVE_LIBRARY_BYTES + 1;
            },
            |image: &mut CpuRuntimeImage| image.libraries[0].names = vec!["a".repeat(129)],
            |image: &mut CpuRuntimeImage| {
                image.libraries[0].names = ["a", "b", "c", "d", "e"].map(String::from).to_vec();
            },
            |image: &mut CpuRuntimeImage| image.libraries[0].identity.sha256 = "A".repeat(64),
        ] {
            let mut changed = fixture();
            mutate(&mut changed);
            assert!(changed.validate().is_err());
        }
        let mut changed = fixture();
        changed.libraries[0].identity.byte_len = MAX_NATIVE_LIBRARY_BYTES;
        changed.validate().unwrap();
        for index in 3..6 {
            let mut library = CapturedNativeLibrary {
                identity: identity(index),
                names: vec![format!("lib{index}.so")],
            };
            library.identity.byte_len = MAX_NATIVE_LIBRARY_BYTES;
            changed.libraries.push(library);
        }
        assert!(changed.validate().is_err());
        let mut changed = fixture();
        for index in 3..=MAX_NATIVE_LIBRARIES {
            changed.libraries.push(CapturedNativeLibrary {
                identity: identity(index as u8),
                names: vec![format!("lib{index}.so")],
            });
        }
        assert!(changed.validate().is_err());
    }
}
