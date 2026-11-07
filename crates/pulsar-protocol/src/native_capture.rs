//! Pure native capture declarations. No filesystem access or loading.
use crate::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};
macro_rules! ensure {
    ($condition:expr, $message:expr) => {
        if !$condition {
            return Err(ProtocolError::invalid($message));
        }
    };
}

pub const MAX_NATIVE_LIBRARIES: usize = 128;
pub const MAX_NATIVE_LIBRARY_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_NATIVE_TOTAL_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeLibrarySpec {
    pub path: PathBuf,
    pub sha256: String,
    pub byte_len: u64,
    /// Explicit loader aliases, not automatically discovered SONAMEs.
    pub names: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeCaptureRequest {
    pub version: u16,
    pub attempt_id: AttemptId,
    /// Paths are fixed guest input coordinates, never supplier paths.
    pub image: NativeDependencySpec,
    pub inputs: Vec<NativeCaptureInputIdentity>,
    pub budget: ResourceBudget,
    pub address_space_bytes: u64,
}
impl NativeCaptureRequest {
    pub fn validate(&self) -> Result<u64, ProtocolError> {
        self.budget.validate()?;
        let bytes = self.image.validate_guest_inputs()?;
        ensure!(
            self.inputs.len() == self.image.libraries.len(),
            "native capture input identities differ"
        );
        ensure!(
            self.version == PROTOCOL_VERSION,
            "unsupported native capture protocol"
        );
        ensure!(
            (64 * 1024 * 1024..=1024 * 1024 * 1024).contains(&self.budget.memory_bytes)
                && self.address_space_bytes >= self.budget.memory_bytes
                && self.address_space_bytes <= 4 * 1024 * 1024 * 1024
                && self.budget.output_bytes >= bytes + 16 * 1024 * 1024
                && self.budget.output_bytes <= MAX_NATIVE_TOTAL_BYTES + 16 * 1024 * 1024
                && self.budget.wall_time_ms <= 3_600_000
                && self.budget.cpu_threads <= 4,
            "invalid native capture budget"
        );
        Ok(bytes)
    }
    pub fn identity(&self) -> Result<String, ProtocolError> {
        use sha2::{Digest, Sha256};
        let bytes = serde_json::to_vec(self)
            .map_err(|_| ProtocolError::invalid("native capture serialization failed"))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeCaptureInputIdentity {
    pub filesystem_device: u64,
    pub inode: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeCaptureCompletion {
    pub root_name: String,
    pub declared_bytes: u64,
    pub request_sha256: String,
}
impl NativeCaptureCompletion {
    pub fn validate(&self, request: &NativeCaptureRequest) -> Result<(), ProtocolError> {
        ensure!(
            self.root_name.starts_with("native-dependencies-")
                && self.root_name.len() > 20
                && self.root_name.len() <= 96
                && self
                    .root_name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
                && self.declared_bytes == request.validate()?
                && self.request_sha256 == request.identity()?,
            "native capture completion differs from request"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeCaptureReply {
    pub version: u16,
    pub attempt_id: AttemptId,
    pub result: Result<NativeCaptureCompletion, ProtocolError>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeDependencySpec {
    pub schema_version: u16,
    pub libraries: Vec<NativeLibrarySpec>,
}

impl NativeDependencySpec {
    pub fn validate(&self) -> Result<u64, ProtocolError> {
        self.validate_paths(|_, path| path.is_absolute())
    }

    /// Capture coordinates belong to the guest POSIX namespace on every host.
    fn validate_guest_inputs(&self) -> Result<u64, ProtocolError> {
        self.validate_paths(|index, path| {
            path.to_str() == Some(format!("/inputs/{index}").as_str())
        })
    }

    fn validate_paths(
        &self,
        valid_path: impl Fn(usize, &std::path::Path) -> bool,
    ) -> Result<u64, ProtocolError> {
        ensure!(
            self.schema_version == 1
                && !self.libraries.is_empty()
                && self.libraries.len() <= MAX_NATIVE_LIBRARIES,
            "unsupported native dependency set"
        );
        let mut names = BTreeSet::new();
        let mut hashes = BTreeSet::new();
        let mut total = 0u64;
        for (index, library) in self.libraries.iter().enumerate() {
            crate::validate_sha256(&library.sha256)?;
            ensure!(
                hashes.insert(&library.sha256),
                "duplicate native dependency identity"
            );
            ensure!(
                valid_path(index, &library.path)
                    && library.byte_len > 0
                    && library.byte_len <= MAX_NATIVE_LIBRARY_BYTES,
                "invalid native dependency input"
            );
            ensure!(
                !library.names.is_empty() && library.names.len() <= 4,
                "invalid native dependency aliases"
            );
            for name in &library.names {
                ensure!(
                    name.len() <= 128
                        && !name.is_empty()
                        && name != "."
                        && name != ".."
                        && name
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric()
                                || matches!(b, b'.' | b'_' | b'-' | b'+'))
                        && names.insert(name),
                    "invalid or conflicting native dependency alias"
                );
            }
            total = total
                .checked_add(library.byte_len)
                .ok_or_else(|| ProtocolError::invalid("native dependency size overflow"))?;
            ensure!(
                total <= MAX_NATIVE_TOTAL_BYTES,
                "native dependency set exceeds byte limit"
            );
        }
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guest_coordinates_are_exact_posix_strings_independent_of_host_paths() {
        let request = fixture();
        assert_eq!(request.validate().unwrap(), 1);
        for path in [
            "\\inputs\\0",
            "/inputs//0",
            "/inputs/./0",
            "/inputs/00",
            "/inputs/1",
            "/supplier/input",
        ] {
            let mut bad = request.clone();
            bad.image.libraries[0].path = path.into();
            assert!(bad.validate().is_err(), "accepted guest alias {path}");
        }
        let mut host = request.image.clone();
        host.libraries[0].path = std::env::temp_dir().join("pulsar-native-fixture");
        assert_eq!(host.validate().unwrap(), 1);
        let mut supplier = request.clone();
        supplier.image = host;
        assert!(supplier.validate().is_err());
        supplier.image.libraries[0].path = "relative-supplier".into();
        assert!(supplier.image.validate().is_err());
    }

    #[test]
    fn guest_validation_preserves_dependency_identity_alias_and_size_guards() {
        for mutate in [
            |r: &mut NativeCaptureRequest| r.image.libraries[0].sha256 = "bad".into(),
            |r: &mut NativeCaptureRequest| r.image.libraries[0].names = vec!["../alias".into()],
            |r: &mut NativeCaptureRequest| r.image.libraries[0].byte_len = 0,
            |r: &mut NativeCaptureRequest| {
                r.image.libraries[0].byte_len = MAX_NATIVE_LIBRARY_BYTES + 1;
            },
            |r: &mut NativeCaptureRequest| r.image.schema_version = 2,
        ] {
            let mut bad = fixture();
            mutate(&mut bad);
            assert!(bad.validate().is_err());
            assert!(bad.image.validate().is_err());
        }
    }
    fn fixture() -> NativeCaptureRequest {
        NativeCaptureRequest {
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("capture").unwrap(),
            image: NativeDependencySpec {
                schema_version: 1,
                libraries: vec![NativeLibrarySpec {
                    path: "/inputs/0".into(),
                    sha256: "a".repeat(64),
                    byte_len: 1,
                    names: vec!["libfixture.so".into()],
                }],
            },
            inputs: vec![NativeCaptureInputIdentity {
                filesystem_device: 1,
                inode: 2,
            }],
            budget: ResourceBudget {
                memory_bytes: 64 * 1024 * 1024,
                output_bytes: 16 * 1024 * 1024 + 1,
                wall_time_ms: 1000,
                cpu_threads: 1,
            },
            address_space_bytes: 128 * 1024 * 1024,
        }
    }
    #[test]
    fn request_and_completion_bind_all_input_budget_and_attempt_metadata() {
        let request = fixture();
        assert_eq!(request.validate().unwrap(), 1);
        let completion = NativeCaptureCompletion {
            root_name: "native-dependencies-abcdef".into(),
            declared_bytes: 1,
            request_sha256: request.identity().unwrap(),
        };
        completion.validate(&request).unwrap();
        let mut changed = request.clone();
        changed.inputs[0].inode += 1;
        assert!(completion.validate(&changed).is_err());
        let mut bad = completion.clone();
        bad.root_name = "native-dependencies-../escape".into();
        assert!(bad.validate(&request).is_err());
        let mut bad = completion.clone();
        bad.declared_bytes = 2;
        assert!(bad.validate(&request).is_err());
        for mutate in [
            |r: &mut NativeCaptureRequest| r.version += 1,
            |r: &mut NativeCaptureRequest| r.image.libraries[0].path = "/supplier/input".into(),
            |r: &mut NativeCaptureRequest| r.budget.output_bytes -= 1,
            |r: &mut NativeCaptureRequest| r.budget.memory_bytes = 1,
            |r: &mut NativeCaptureRequest| r.address_space_bytes = 1,
            |r: &mut NativeCaptureRequest| r.budget.wall_time_ms = 3_600_001,
            |r: &mut NativeCaptureRequest| r.budget.cpu_threads = 5,
            |r: &mut NativeCaptureRequest| r.inputs.clear(),
        ] {
            let mut bad = request.clone();
            mutate(&mut bad);
            assert!(bad.validate().is_err());
        }
        let bytes = serde_json::to_vec(&request).unwrap();
        let decoded: NativeCaptureRequest = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(bytes, serde_json::to_vec(&decoded).unwrap());
        let mut value = serde_json::to_value(&request).unwrap();
        value["load_libraries"] = true.into();
        assert!(serde_json::from_value::<NativeCaptureRequest>(value).is_err());
    }
}
