//! Local custody of the roleplay consent document.
//!
//! Consent state is mutable, unlike the one-shot assistant declarations it sits
//! beside, so this module adds an explicit atomic-replace primitive rather than
//! reusing the create-only private store. The primitives keep the same custody
//! rules: absolute path, owned parent that other users cannot write, single-link
//! mode-0600 regular file, no symlink following, and a bounded size.
//!
//! Consent withdrawal and revocation are ordinary writes here. They are never
//! gated on approval, because a stop or a withdrawal that needs consent is not one.
use super::*;
use std::io::{Read, Write};
use std::path::Path;

/// Bound on the retained document, matching the private declaration store.
pub const MAX_CONSENT_DOCUMENT_BYTES: usize = 32 * 1024;

/// Load and validate the retained consent document.
pub fn load(path: &Path) -> Result<RoleplayConsentDocument> {
    let document: RoleplayConsentDocument = assistant_client::load_private(path)?;
    document.validate()?;
    Ok(document)
}

fn serialize(document: &RoleplayConsentDocument) -> Result<Vec<u8>> {
    document.validate()?;
    let bytes = serde_json::to_vec_pretty(document)?;
    anyhow::ensure!(
        bytes.len() <= MAX_CONSENT_DOCUMENT_BYTES,
        "consent document exceeds its bound"
    );
    Ok(bytes)
}

/// Create a new consent document. Never replaces an existing path.
pub fn create(path: &Path, document: &RoleplayConsentDocument) -> Result<()> {
    serialize(document)?;
    assistant_client::save_private(path, document)
}

/// Atomically replace the consent document in place.
///
/// Refuses a target that is not our own single-link mode-0600 regular file, so a
/// pre-existing symlink, hardlink or shared file is never overwritten.
#[cfg(unix)]
pub fn replace(path: &Path, document: &RoleplayConsentDocument) -> Result<()> {
    use rustix::fs::{open, openat, Mode, OFlags};
    use std::os::unix::fs::MetadataExt;

    let bytes = serialize(document)?;
    anyhow::ensure!(path.is_absolute(), "consent document path must be absolute");
    let parent = path.parent().context("missing consent parent")?;
    let file_name = path.file_name().context("consent filename missing")?;
    let directory = std::fs::File::from(open(
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    let metadata = directory.metadata()?;
    anyhow::ensure!(
        metadata.is_dir()
            && metadata.uid() == rustix::process::geteuid().as_raw()
            && metadata.mode() & 0o022 == 0,
        "consent parent must be owned and not writable by other users"
    );

    // The target may be absent or must already be exactly our private file.
    match openat(
        &directory,
        file_name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(existing) => {
            let existing = std::fs::File::from(existing);
            let meta = existing.metadata()?;
            anyhow::ensure!(
                meta.is_file()
                    && meta.uid() == rustix::process::geteuid().as_raw()
                    && meta.mode() & 0o777 == 0o600
                    && meta.nlink() == 1
                    && meta.len() as usize <= MAX_CONSENT_DOCUMENT_BYTES,
                "existing consent document is not a private single-link file"
            );
        }
        Err(rustix::io::Errno::NOENT) => {}
        Err(rustix::io::Errno::LOOP) => {
            anyhow::bail!("existing consent document is not a private single-link file")
        }
        Err(error) => return Err(error.into()),
    }

    let temporary_name = format!("{}.new-{}", file_name.to_string_lossy(), std::process::id());
    let temporary_path = parent.join(&temporary_name);
    let mut file = std::fs::File::from(openat(
        &directory,
        temporary_name.as_str(),
        OFlags::WRONLY
            | OFlags::CREATE
            | OFlags::EXCL
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK
            | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?);
    let written = (|| -> Result<()> {
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(())
    })();
    drop(file);
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(error);
    }
    if let Err(error) = std::fs::rename(&temporary_path, path) {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(error.into());
    }
    directory.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
pub fn replace(_path: &Path, _document: &RoleplayConsentDocument) -> Result<()> {
    anyhow::bail!("private consent document replacement is unavailable on this platform")
}

/// Read the current document, apply `change`, and replace it atomically.
fn mutate(
    path: &Path,
    change: impl FnOnce(&mut RoleplayConsentDocument) -> Result<()>,
) -> Result<RoleplayConsentDocument> {
    let mut document = load(path)?;
    change(&mut document)?;
    replace(path, &document)?;
    Ok(document)
}

/// Record a fresh local adult attestation. This is not verification.
pub fn attest(path: &Path, now_ns: u64) -> Result<RoleplayConsentDocument> {
    mutate(path, |document| {
        document.attest(now_ns);
        Ok(())
    })
}

/// Withdraw the attestation. Ungated and irreversible within this document.
pub fn withdraw_attestation(path: &Path, now_ns: u64) -> Result<RoleplayConsentDocument> {
    mutate(path, |document| {
        document.withdraw_attestation(now_ns);
        Ok(())
    })
}

/// Record one participant's consent to the declared class and scope.
pub fn grant(
    path: &Path,
    participant: ParticipantRef,
    content_class: ContentClass,
    now_ns: u64,
) -> Result<RoleplayConsentDocument> {
    mutate(path, |document| {
        document
            .session
            .grant(participant, content_class, now_ns)
            .map_err(Into::into)
    })
}

/// Withdraw one participant's consent. Ungated.
pub fn revoke(
    path: &Path,
    participant: &ParticipantRef,
    now_ns: u64,
) -> Result<RoleplayConsentDocument> {
    mutate(path, |document| {
        document
            .session
            .revoke(participant, now_ns)
            .map_err(Into::into)
    })
}

/// Escalate to a more explicit class, clearing every prior consent.
pub fn escalate(
    path: &Path,
    content_class: ContentClass,
    now_ns: u64,
) -> Result<RoleplayConsentDocument> {
    mutate(path, |document| {
        document
            .session
            .escalate(content_class, now_ns)
            .map_err(Into::into)
    })
}

/// Switch between local and remote processing, clearing every prior consent.
pub fn switch_provider(
    path: &Path,
    provider: ProviderScope,
    now_ns: u64,
) -> Result<RoleplayConsentDocument> {
    mutate(path, |document| {
        document
            .session
            .switch_provider(provider, now_ns)
            .map_err(Into::into)
    })
}

/// Redacted status for the operator.
pub fn status(path: &Path, now_ns: u64) -> Result<ConsentSummary> {
    Ok(load(path)?.summary(now_ns))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn scratch() -> tempfile::TempDir {
        // Owner-only directory, matching the custody rule for the parent.
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        directory
    }

    fn document_path(directory: &tempfile::TempDir) -> std::path::PathBuf {
        directory.path().join("session.consent.json")
    }

    fn participant(id: &str) -> ParticipantRef {
        ParticipantRef::new(id).unwrap()
    }

    fn provider(id: &str) -> ProviderRef {
        ProviderRef::new(id).unwrap()
    }

    /// Evaluate admission at a time after every recorded event in these tests.
    fn admitted_at(path: &Path, now_ns: u64) -> bool {
        load(path).unwrap().admit_effect(now_ns).is_ok()
    }

    fn admitted(path: &Path) -> bool {
        admitted_at(path, 1_500)
    }

    #[test]
    fn create_writes_a_private_document_and_refuses_to_replace_it() {
        let directory = scratch();
        let path = document_path(&directory);
        create(&path, &RoleplayConsentDocument::least_permissive()).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "consent document must be mode 0600");
        assert_eq!(
            load(&path).unwrap().session.declared_class,
            ContentClass::General
        );
        assert!(
            create(&path, &RoleplayConsentDocument::least_permissive()).is_err(),
            "create must never overwrite an existing document"
        );
    }

    #[test]
    fn replace_updates_in_place() {
        let directory = scratch();
        let path = document_path(&directory);
        create(&path, &RoleplayConsentDocument::least_permissive()).unwrap();
        attest(&path, 1_000).unwrap();
        assert!(load(&path).unwrap().attestation.is_some());
    }

    #[test]
    fn replace_refuses_a_symlink_target() {
        let directory = scratch();
        let target = directory.path().join("elsewhere.json");
        std::fs::write(&target, b"{}").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        let path = document_path(&directory);
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let error = replace(&path, &RoleplayConsentDocument::least_permissive()).unwrap_err();
        assert!(
            error.to_string().contains("private single-link file")
                || error.to_string().contains("consent document is not")
        );
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"{}",
            "a symlinked target is never written through"
        );
    }

    #[test]
    fn a_relative_path_is_refused() {
        let document = RoleplayConsentDocument::least_permissive();
        assert!(replace(Path::new("session.consent.json"), &document).is_err());
        assert!(create(Path::new("session.consent.json"), &document).is_err());
    }

    #[test]
    fn a_tampered_document_is_refused_on_load() {
        let directory = scratch();
        let path = document_path(&directory);
        // A participant record whose class disagrees with the declared class.
        let tampered = serde_json::json!({
            "schema_version": 1,
            "session": {
                "schema_version": 1,
                "declared_class": "general",
                "declared_provider": {"local": true, "remote_provider": null},
                "participants": [{
                    "participant": "operator",
                    "content_class": "explicit",
                    "provider": {"local": true, "remote_provider": null},
                    "granted_at_ns": 10,
                    "revoked_at_ns": null
                }]
            },
            "attestation": null
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&tampered).unwrap()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(load(&path).is_err(), "mismatched class must not load");
    }

    #[test]
    fn consent_is_required_before_effect_and_attestation_before_roleplay() {
        let directory = scratch();
        let path = document_path(&directory);
        create(&path, &RoleplayConsentDocument::least_permissive()).unwrap();

        let summary = status(&path, 1_000).unwrap();
        assert!(!summary.attested);
        assert!(!summary.roleplay_admitted);
        assert_eq!(
            summary.effect_refusal,
            Some("adult_attestation_missing"),
            "roleplay is refused before consent is considered"
        );

        attest(&path, 1_000).unwrap();
        let summary = status(&path, 1_000).unwrap();
        assert!(summary.attested && summary.roleplay_admitted);
        assert_eq!(summary.effect_refusal, Some("consent_absent"));

        grant(&path, participant("operator"), ContentClass::General, 1_000).unwrap();
        let summary = status(&path, 1_000).unwrap();
        assert!(summary.effect_admitted);
        assert_eq!(summary.effect_refusal, None);
        assert_eq!(summary.participants.len(), 1);
    }

    #[test]
    fn revocation_is_ungated_and_effective_immediately() {
        let directory = scratch();
        let path = document_path(&directory);
        create(&path, &RoleplayConsentDocument::least_permissive()).unwrap();
        attest(&path, 1_000).unwrap();
        grant(&path, participant("operator"), ContentClass::General, 1_000).unwrap();
        assert!(admitted(&path));

        revoke(&path, &participant("operator"), 1_100).unwrap();
        assert!(!admitted(&path));
        assert_eq!(
            status(&path, 1_100).unwrap().effect_refusal,
            Some("consent_revoked")
        );
    }

    #[test]
    fn withdrawing_the_attestation_disables_roleplay_and_effect() {
        let directory = scratch();
        let path = document_path(&directory);
        create(&path, &RoleplayConsentDocument::least_permissive()).unwrap();
        attest(&path, 1_000).unwrap();
        grant(&path, participant("operator"), ContentClass::General, 1_000).unwrap();
        withdraw_attestation(&path, 1_200).unwrap();
        let summary = status(&path, 1_200).unwrap();
        assert!(!summary.attested && !summary.roleplay_admitted && !summary.effect_admitted);
        assert_eq!(summary.effect_refusal, Some("adult_attestation_withdrawn"));
    }

    #[test]
    fn escalation_and_scope_switching_clear_consent() {
        let directory = scratch();
        let path = document_path(&directory);
        create(&path, &RoleplayConsentDocument::least_permissive()).unwrap();
        attest(&path, 1_000).unwrap();
        grant(&path, participant("operator"), ContentClass::General, 1_000).unwrap();
        assert!(admitted(&path));

        escalate(&path, ContentClass::Explicit, 1_100).unwrap();
        assert_eq!(
            status(&path, 1_100).unwrap().effect_refusal,
            Some("consent_revoked")
        );

        grant(
            &path,
            participant("operator"),
            ContentClass::Explicit,
            1_200,
        )
        .unwrap();
        assert!(admitted(&path));

        switch_provider(&path, ProviderScope::remote(provider("provider-a")), 1_300).unwrap();
        let summary = status(&path, 1_300).unwrap();
        assert!(summary.remote);
        assert_eq!(summary.effect_refusal, Some("consent_revoked"));

        grant(
            &path,
            participant("operator"),
            ContentClass::Explicit,
            1_400,
        )
        .unwrap();
        assert!(admitted(&path), "fresh consent covers the remote scope");
    }

    #[test]
    fn a_grant_for_a_remote_scope_does_not_survive_a_switch_to_local() {
        let directory = scratch();
        let path = document_path(&directory);
        create(
            &path,
            &RoleplayConsentDocument::new(RoleplaySessionConsent::declare_with(
                ContentClass::Mature,
                ProviderScope::remote(provider("provider-a")),
            )),
        )
        .unwrap();
        attest(&path, 1_000).unwrap();
        grant(&path, participant("operator"), ContentClass::Mature, 1_000).unwrap();
        assert!(admitted(&path));

        switch_provider(&path, ProviderScope::local(), 1_100).unwrap();
        assert_eq!(
            status(&path, 1_100).unwrap().effect_refusal,
            Some("consent_revoked")
        );
    }
}
