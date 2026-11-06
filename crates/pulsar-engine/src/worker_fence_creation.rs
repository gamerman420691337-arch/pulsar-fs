//! Format-4 cgroup creation facts. A name never authorizes deletion on its own.
use super::*;
#[path = "worker_cgroup_creation.rs"]
pub(crate) mod admitted;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Roster {
    parent: Group,
    boot: String,
    records: BTreeMap<String, Phase>,
}
impl Roster {
    pub(super) fn has_pending_creation(&self) -> bool {
        !self.records.is_empty()
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Phase {
    Intent,
    Bound(Group),
    Retired(Group),
}
fn boot() -> Result<String> {
    let mut bytes = String::new();
    File::open("/proc/sys/kernel/random/boot_id")?
        .take(129)
        .read_to_string(&mut bytes)?;
    let value = bytes.trim();
    ensure!(
        bytes.len() <= 128 && uuid::Uuid::parse_str(value)?.to_string() == value,
        "invalid cgroup creation boot identity"
    );
    Ok(value.into())
}
fn prefix(scope: &str) -> String {
    format!("pulsar-worker-{scope}-")
}
impl Roster {
    pub(super) fn len(&self) -> usize {
        self.records.len()
    }
    fn capture() -> Result<Self> {
        let parent = group_identity(&crate::cgroup::delegated_root()?)?;
        Ok(Self {
            parent,
            boot: boot()?,
            records: BTreeMap::new(),
        })
    }
    pub(super) fn validate(&self, ledger: &Ledger) -> Result<()> {
        ensure!(
            self.parent.path.is_absolute()
                && self.parent.path.starts_with("/sys/fs/cgroup")
                && self.parent.path.as_os_str().len() <= 4096
                && self.parent.inode != 0,
            "invalid cgroup creation parent"
        );
        ensure!(
            uuid::Uuid::parse_str(&self.boot)?.to_string() == self.boot,
            "invalid cgroup creation boot identity"
        );
        let prefix = prefix(&ledger.scope);
        for (name, phase) in &self.records {
            let suffix = name
                .strip_prefix(&prefix)
                .context("creation name escapes scope")?;
            ensure!(
                uuid::Uuid::parse_str(suffix)?.to_string() == suffix,
                "creation name is not canonical"
            );
            ensure!(
                !ledger.groups.contains_key(name) && !ledger.retired.contains_key(name),
                "creation overlaps registered worker ownership"
            );
            if let Phase::Bound(group) | Phase::Retired(group) = phase {
                ensure!(
                    group.path == self.parent.path.join(name) && group.inode != 0,
                    "cgroup creation identity/path differs"
                );
            }
        }
        for (name, group) in ledger.groups.iter().chain(&ledger.retired) {
            if name.starts_with(&prefix) {
                let suffix = name.strip_prefix(&prefix).unwrap();
                ensure!(
                    uuid::Uuid::parse_str(suffix)?.to_string() == suffix
                        && group.path == self.parent.path.join(name),
                    "registered creation escapes its namespace"
                );
            }
        }
        Ok(())
    }
    fn parent(&self) -> Result<File> {
        ensure!(
            boot()? == self.boot,
            "cgroup creation boot changed; explicit recovery required"
        );
        let (held, identity) = group_directory(&self.parent.path)?;
        ensure!(
            identity == self.parent,
            "cgroup creation parent identity changed"
        );
        Ok(held)
    }
    pub(super) fn namespace(&self, ledger: &Ledger) -> Result<()> {
        let _held = self.parent()?;
        let prefix = prefix(&ledger.scope);
        for (index, entry) in fs::read_dir(&self.parent.path)?.enumerate() {
            ensure!(
                index < 8192,
                "cgroup creation namespace scan bound exceeded"
            );
            let name = entry?.file_name();
            if name.as_encoded_bytes().starts_with(prefix.as_bytes()) {
                let name = name.to_str().context("non-UTF8 cgroup creation name")?;
                ensure!(
                    self.records.contains_key(name)
                        || ledger.groups.contains_key(name)
                        || ledger.retired.contains_key(name),
                    "unrecorded scope cgroup; admission refused"
                );
            }
        }
        // Detect a parent replacement during the bounded observation.
        self.parent()?;
        Ok(())
    }
    fn absent(&self, name: &str) -> Result<bool> {
        self.parent()?;
        match fs::symlink_metadata(self.parent.path.join(name)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
            Err(error) => Err(error.into()),
            Ok(_) => Ok(false),
        }
    }
    pub(super) fn preflight(&self) -> Result<()> {
        self.parent()?;
        for (name, phase) in &self.records {
            match phase {
                Phase::Intent => ensure!(
                    self.absent(name)?,
                    "ambiguous cgroup creation intent; admission refused"
                ),
                Phase::Bound(group) => ensure!(
                    empty(group)?,
                    "prior creating worker tree is populated; admission refused"
                ),
                Phase::Retired(group) => crate::cgroup::check_recorded_empty(
                    &group.path,
                    group.device,
                    group.inode,
                    true,
                )?,
            }
        }
        Ok(())
    }
}

pub(super) fn recover(directory: &Path, ledger: &mut Ledger) -> Result<()> {
    let Some(roster) = &ledger.creation else {
        return Ok(());
    };
    let names: Vec<_> = roster.records.keys().cloned().collect();
    for name in names {
        let roster = ledger
            .creation
            .as_ref()
            .context("creation roster missing")?;
        roster.parent()?;
        let phase = roster
            .records
            .get(&name)
            .context("creation obligation missing")?
            .clone();
        if let Phase::Bound(group) = &phase {
            let mut next = ledger.clone();
            next.creation
                .as_mut()
                .unwrap()
                .records
                .insert(name.clone(), Phase::Retired(group.clone()));
            save(directory, &next)?;
            *ledger = next;
        }
        match &phase {
            Phase::Intent => ensure!(
                ledger.creation.as_ref().unwrap().absent(&name)?,
                "ambiguous cgroup creation intent; admission refused"
            ),
            Phase::Bound(group) | Phase::Retired(group) => {
                crate::cgroup::reclaim_recorded(&group.path, group.device, group.inode, true)?
            }
        }
        ensure!(
            ledger.creation.as_ref().unwrap().absent(&name)?,
            "cgroup creation clear requires actual absence"
        );
        let mut next = ledger.clone();
        next.creation.as_mut().unwrap().records.remove(&name);
        save(directory, &next)?;
        *ledger = next;
    }
    Ok(())
}
impl WorkerFence {
    /// Primitive activation only. Production must commit schema 20 before intent.
    pub(crate) fn enable_creation(&self) -> Result<()> {
        self.enable_staging()?;
        let mut ledger = self
            .ledger
            .lock()
            .map_err(|_| anyhow::anyhow!("worker fence ledger unavailable"))?;
        if ledger.version == 4 {
            ledger
                .creation
                .as_ref()
                .context("creation roster missing")?
                .namespace(&ledger)?;
            return Ok(());
        }
        let mut next = ledger.clone();
        next.version = 4;
        next.creation = Some(Roster::capture()?);
        validate_namespace(&self.directory, &next, true)?;
        save(&self.directory, &next)?;
        *ledger = next;
        Ok(())
    }
    /// Durable absence obligation. The caller must retain custody on any error.
    #[cfg(test)]
    pub(crate) fn declare_creation(&self) -> Result<PathBuf> {
        let mut ledger = self
            .ledger
            .lock()
            .map_err(|_| anyhow::anyhow!("worker fence ledger unavailable"))?;
        ensure!(ledger.version == 4, "cgroup creation requires format 4");
        let name = format!("{}{}", prefix(&ledger.scope), uuid::Uuid::new_v4());
        let mut next = ledger.clone();
        let roster = next.creation.as_mut().context("creation roster missing")?;
        roster.parent()?;
        ensure!(roster.absent(&name)?, "creation name is already present");
        let path = roster.parent.path.join(&name);
        roster.records.insert(name, Phase::Intent);
        validate(&next)?;
        // Keep attempted publication on IO failure, including a lost acknowledgement.
        *ledger = next;
        save(&self.directory, &ledger)?;
        Ok(path)
    }
    fn creation_identity(&self, group: &AttemptCgroup) -> Result<(String, Group)> {
        let identity = group_identity(group.path())?;
        ensure!(
            (identity.device, identity.inode) == group.identity()?,
            "creation differs from held cgroup identity"
        );
        let name = identity
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .context("invalid creation name")?
            .to_owned();
        Ok((name, identity))
    }
    #[cfg(test)]
    pub(crate) fn bind_creation(&self, group: &AttemptCgroup) -> Result<()> {
        self.bind_held_creation(group.path(), group.identity()?)
    }
    fn bind_held_creation(&self, path: &Path, held: (u64, u64)) -> Result<()> {
        let identity = group_identity(path)?;
        ensure!(
            (identity.device, identity.inode) == held,
            "creation differs from held cgroup identity"
        );
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .context("invalid creation name")?
            .to_owned();
        let mut ledger = self
            .ledger
            .lock()
            .map_err(|_| anyhow::anyhow!("worker fence ledger unavailable"))?;
        let roster = ledger
            .creation
            .as_mut()
            .context("creation roster missing")?;
        roster.parent()?;
        ensure!(
            identity.path == roster.parent.path.join(&name),
            "creation escapes parent"
        );
        match roster.records.get(&name) {
            Some(Phase::Intent) => (),
            Some(Phase::Bound(prior)) => {
                ensure!(prior == &identity, "creation binding identity changed")
            }
            _ => anyhow::bail!("creation intent missing or retired"),
        }
        roster.records.insert(name, Phase::Bound(identity));
        save(&self.directory, &ledger)
    }
    /// Atomic transfer from creation ownership to the registered worker roster.
    pub(crate) fn register_creation(&self, group: &AttemptCgroup) -> Result<()> {
        let (name, identity) = self.creation_identity(group)?;
        let mut ledger = self
            .ledger
            .lock()
            .map_err(|_| anyhow::anyhow!("worker fence ledger unavailable"))?;
        let roster = ledger
            .creation
            .as_ref()
            .context("creation roster missing")?;
        roster.parent()?;
        ensure!(
            roster.records.get(&name) == Some(&Phase::Bound(identity.clone())),
            "creation registration requires the bound identity"
        );
        ensure!(
            empty(&identity)?,
            "creation registration requires an empty group"
        );
        ensure!(
            !ledger.groups.contains_key(&name) && !ledger.retired.contains_key(&name),
            "creation registration overlaps existing worker"
        );
        ledger.creation.as_mut().unwrap().records.remove(&name);
        ledger.groups.insert(name, identity);
        save(&self.directory, &ledger)
    }
    /// Unspawned custody only. Registered ownership uses the common retirement path.
    pub(crate) fn retire_creation(&self, group: &AttemptCgroup) -> Result<()> {
        let (device, inode) = group.identity()?;
        let identity = Group {
            path: group.path().into(),
            device,
            inode,
        };
        let name = identity
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .context("invalid creation name")?
            .to_owned();
        let mut ledger = self
            .ledger
            .lock()
            .map_err(|_| anyhow::anyhow!("worker fence ledger unavailable"))?;
        if ledger.groups.contains_key(&name) || ledger.retired.contains_key(&name) {
            drop(ledger);
            return self.retire_unspawned(group);
        }
        let roster = ledger
            .creation
            .as_mut()
            .context("creation roster missing")?;
        roster.parent()?;
        ensure!(
            identity.path == roster.parent.path.join(&name),
            "creation escapes parent"
        );
        match roster.records.get(&name) {
            Some(Phase::Intent) => ensure!(
                group_identity(group.path())? == identity,
                "creation retirement locator differs"
            ),
            Some(Phase::Bound(prior) | Phase::Retired(prior)) => {
                ensure!(prior == &identity, "creation retirement identity differs")
            }
            None => anyhow::bail!("creation retirement obligation missing"),
        }
        ensure!(group.is_empty()?, "creating group is still populated");
        roster
            .records
            .insert(name.clone(), Phase::Retired(identity));
        save(&self.directory, &ledger)?;
        group.reclaim()?;
        ensure!(
            ledger.creation.as_ref().unwrap().absent(&name)?,
            "creation clear requires absence"
        );
        let mut next = ledger.clone();
        next.creation.as_mut().unwrap().records.remove(&name);
        save(&self.directory, &next)?;
        *ledger = next;
        Ok(())
    }
}

#[cfg(test)]
#[path = "worker_fence_creation_tests.rs"]
mod tests;

#[cfg(test)]
#[test]
fn cleanup_observation_keeps_creation_obligations() {
    let mut roster = Roster {
        parent: Group {
            path: PathBuf::from("/inert-test-parent"),
            device: 0,
            inode: 0,
        },
        boot: "test".into(),
        records: BTreeMap::new(),
    };
    assert!(!roster.has_pending_creation());
    roster.records.insert("pending".into(), Phase::Intent);
    assert!(roster.has_pending_creation());
    roster.records.clear();
    assert!(!roster.has_pending_creation());
}
