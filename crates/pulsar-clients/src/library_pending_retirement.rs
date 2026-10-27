// Explicit acknowledgement-covered pending journal retirement; no data deletion.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingRetirementReceipt {
    pub directory: std::path::PathBuf,
    pub original_path: std::path::PathBuf,
    pub retained_path: std::path::PathBuf,
    pub sha256: String,
    pub byte_len: u64,
    pub already_retired: bool,
    pub file_and_directory_synced: bool,
    pub qualified: bool,
}

pub(crate) fn valid_retirement_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Debug)]
pub struct PendingRetirementUnknown;
impl std::fmt::Display for PendingRetirementUnknown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f,"Pending metadata retirement may have changed a name; preserve both paths and retry the exact original path/digest")
    }
}
impl std::error::Error for PendingRetirementUnknown {}

/// Move an explicitly inspected, covered pending file to a retained name.
/// Cached journal acknowledgements are historical facts, not fresh output adoption.
pub fn retire_pending(
    client: &mut SessionClient,
    directory: &Path,
    pending: &Path,
    expected_sha256: &str,
) -> Result<PendingRetirementReceipt> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let store = local::Store::open(directory, &client.recovery_credential_binding(), None)?;
        store.retire_pending(pending, expected_sha256)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (client, directory, pending, expected_sha256);
        Err(
            ProtocolError::unsupported("pending publication journal retirement on this platform")
                .into(),
        )
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn require_covered_pending(current: &Journal, pending: &Journal) -> Result<()> {
    current.validate(&current.credential_binding)?;
    pending.validate(&current.credential_binding)?;
    ensure!(
        current.report.publication_completed,
        "Complete current publication acknowledgements before retiring pending metadata"
    );
    ensure!(
        current.report.items.len() == pending.report.items.len(),
        "Pending item count differs"
    );
    let mut normalized = pending.clone();
    for (old, now) in normalized
        .report
        .items
        .iter_mut()
        .zip(&current.report.items)
    {
        ensure!(
            now.phase == PublicationPhase::Published,
            "Current entry is not acknowledged"
        );
        match (&old.partial, &now.partial) {
            (Some(a), Some(b)) => {
                ensure!(
                    a.directory_identity == b.directory_identity
                        && a.channels.len() == b.channels.len()
                        && a.channels
                            .iter()
                            .zip(&b.channels)
                            .all(|(x, y)| x.axis == y.axis
                                && x.request_id == y.request_id
                                && (!x.acknowledged || y.acknowledged)),
                    "Pending channel identities or acknowledgements differ"
                );
                old.partial = now.partial.clone();
            }
            (None, None) => {
                ensure!(
                    old.prepared.is_some()
                        && serde_json::to_vec(&old.prepared)? == serde_json::to_vec(&now.prepared)?
                        && (old.receipt.is_none()
                            || serde_json::to_vec(&old.receipt)?
                                == serde_json::to_vec(&now.receipt)?),
                    "Pending aggregate preparation differs"
                );
                old.receipt = now.receipt.clone();
            }
            _ => anyhow::bail!("Pending publication family differs"),
        }
        old.phase = now.phase;
        old.error = now.error.clone();
    }
    normalized.report.publication_completed = current.report.publication_completed;
    ensure!(
        serde_json::to_vec(&normalized)? == serde_json::to_vec(current)?,
        "Pending immutable generation/batch history differs"
    );
    Ok(())
}
