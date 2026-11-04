//! Admitted, unspawned cgroup creation. Failure retains the complete owner.
use super::*;
use crate::{resources::Reservation, staging_cleanup::Cleanup, worker_retirement::WorkerCustody};
use std::{os::fd::FromRawFd, sync::Arc};

struct Ticket {
    parent: File,
    identity: Group,
    name: String,
    path: PathBuf,
}
impl Ticket {
    fn check(&self, roster: &Roster) -> Result<()> {
        let _current = roster.parent()?;
        let held = self.parent.metadata()?;
        ensure!(
            roster.parent == self.identity
                && held.dev() == self.identity.device
                && held.ino() == self.identity.inode
                && self.path == roster.parent.path.join(&self.name),
            "creation ticket parent differs"
        );
        Ok(())
    }
    fn child(&self) -> Result<File> {
        let name = std::ffi::CString::new(self.name.as_str())?;
        let fd = unsafe {
            libc::openat(
                self.parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        ensure!(
            fd >= 0,
            "capture created cgroup: {}",
            std::io::Error::last_os_error()
        );
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}
struct State {
    fence: Arc<WorkerFence>,
    ticket: Option<Ticket>,
    root: Option<File>,
    group: Option<AttemptCgroup>,
    retirement_acked: bool,
    reservation: Arc<Reservation>,
    snapshots: Option<crate::staging_obligation::Staging>,
    snapshot_cleanup: Option<Cleanup>,
    group_retired: bool,
    assets: crate::worker_retirement::RetirementAssets,
}
impl State {
    fn retire(&mut self) -> Result<()> {
        if !self.group_retired {
            self.retire_group()?;
            self.group_retired = true;
        }
        if let Some(snapshots) = self.snapshots.take() {
            self.snapshot_cleanup = Some(
                snapshots.cleanup_after_use_retired((self.reservation.clone(), self.fence.clone())),
            );
        }
        ensure!(
            self.snapshot_cleanup
                .as_ref()
                .is_none_or(Cleanup::is_complete),
            "creation snapshot cleanup unconfirmed"
        );
        Ok(())
    }
    fn retire_group(&mut self) -> Result<()> {
        let Some(ticket) = &self.ticket else {
            return Ok(());
        };
        if let Some(group) = &self.group {
            return self.fence.retire_creation(group);
        }
        let mut ledger = self
            .fence
            .ledger
            .lock()
            .map_err(|_| anyhow::anyhow!("worker fence ledger unavailable"))?;
        let roster = ledger
            .creation
            .as_mut()
            .context("creation roster missing")?;
        ticket.check(roster)?;
        if let Some(root) = &self.root {
            let held = root.metadata()?;
            let identity = Group {
                path: ticket.path.clone(),
                device: held.dev(),
                inode: held.ino(),
            };
            match roster.records.get(&ticket.name) {
                Some(Phase::Intent) => (),
                Some(Phase::Bound(prior) | Phase::Retired(prior)) => ensure!(
                    prior == &identity,
                    "captured creation retirement identity differs"
                ),
                None => anyhow::bail!("captured creation obligation missing"),
            }
            crate::cgroup::check_recorded_empty(
                &ticket.path,
                identity.device,
                identity.inode,
                held.nlink() == 0,
            )?;
            if !self.retirement_acked {
                roster
                    .records
                    .insert(ticket.name.clone(), Phase::Retired(identity.clone()));
                save(&self.fence.directory, &ledger)?;
                self.retirement_acked = true;
            }
            crate::cgroup::reclaim_recorded(&ticket.path, identity.device, identity.inode, true)?;
            ensure!(
                root.metadata()?.nlink() == 0,
                "captured cgroup deletion unconfirmed"
            );
        } else {
            ensure!(
                roster
                    .records
                    .get(&ticket.name)
                    .is_none_or(|phase| *phase == Phase::Intent),
                "uncaptured creation cannot clear bound ownership"
            );
        }
        ensure!(
            ledger.creation.as_ref().unwrap().absent(&ticket.name)?,
            "creation clear requires actual absence"
        );
        let mut next = ledger.clone();
        next.creation.as_mut().unwrap().records.remove(&ticket.name);
        save(&self.fence.directory, &next)?;
        *ledger = next;
        Ok(())
    }
    fn cleanup(mut self) -> Cleanup {
        Cleanup::start_task(move || self.retire().map_err(std::io::Error::other))
    }
}
pub(crate) struct Admitted {
    state: Option<State>,
}
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    Ticket,
    Intent,
    Created,
    Captured,
    Bound,
    Configured,
    Registered,
}
impl Admitted {
    pub(crate) fn create(
        fence: Arc<WorkerFence>,
        reservation: Arc<Reservation>,
        memory: u64,
        threads: u16,
    ) -> Result<Self> {
        Self::create_staged(fence, reservation, memory, threads, None)
    }
    pub(crate) fn create_staged(
        fence: Arc<WorkerFence>,
        reservation: Arc<Reservation>,
        memory: u64,
        threads: u16,
        snapshots: Option<crate::staging_obligation::Staging>,
    ) -> Result<Self> {
        Self::create_inner(
            fence,
            reservation,
            memory,
            threads,
            snapshots,
            #[cfg(test)]
            &mut observe_engine_crash,
        )
    }
    fn create_inner(
        fence: Arc<WorkerFence>,
        reservation: Arc<Reservation>,
        memory: u64,
        threads: u16,
        snapshots: Option<crate::staging_obligation::Staging>,
        #[cfg(test)] observe: &mut dyn FnMut(Step, &State) -> Result<()>,
    ) -> Result<Self> {
        Self::create_inner_with_assets(
            fence,
            reservation,
            memory,
            threads,
            snapshots,
            Vec::new(),
            #[cfg(test)]
            observe,
        )
    }
    fn create_inner_with_assets(
        fence: Arc<WorkerFence>,
        reservation: Arc<Reservation>,
        memory: u64,
        threads: u16,
        snapshots: Option<crate::staging_obligation::Staging>,
        assets: crate::worker_retirement::RetirementAssets,
        #[cfg(test)] observe: &mut dyn FnMut(Step, &State) -> Result<()>,
    ) -> Result<Self> {
        // This owner exists before validation, namespace observation or publication.
        let mut admitted = Self {
            state: Some(State {
                fence,
                ticket: None,
                root: None,
                group: None,
                retirement_acked: false,
                reservation,
                snapshots,
                snapshot_cleanup: None,
                group_retired: false,
                assets,
            }),
        };
        let state = admitted.state.as_mut().unwrap();
        ensure!(
            state.reservation.admits_worker_budget(memory, threads),
            "cgroup configuration exceeds admitted worker claim"
        );
        {
            let ledger = state
                .fence
                .ledger
                .lock()
                .map_err(|_| anyhow::anyhow!("worker fence ledger unavailable"))?;
            ensure!(
                ledger.version == 4,
                "admitted cgroup creation requires format 4"
            );
            state.fence.lease_fd()?;
            let roster = ledger
                .creation
                .as_ref()
                .context("creation roster missing")?;
            let parent = roster.parent()?;
            let name = format!("{}{}", prefix(&ledger.scope), uuid::Uuid::new_v4());
            state.ticket = Some(Ticket {
                parent,
                identity: roster.parent.clone(),
                path: roster.parent.path.join(&name),
                name,
            });
        }
        #[cfg(test)]
        observe(Step::Ticket, state)?;
        {
            let ticket = state.ticket.as_ref().unwrap();
            let mut ledger = state
                .fence
                .ledger
                .lock()
                .map_err(|_| anyhow::anyhow!("worker fence ledger unavailable"))?;
            let mut next = ledger.clone();
            let roster = next.creation.as_mut().context("creation roster missing")?;
            ticket.check(roster)?;
            ensure!(roster.absent(&ticket.name)?, "creation name already exists");
            ensure!(
                !roster.records.contains_key(&ticket.name),
                "creation ticket already declared"
            );
            roster.records.insert(ticket.name.clone(), Phase::Intent);
            validate(&next)?;
            *ledger = next;
            save(&state.fence.directory, &ledger)?;
        }
        #[cfg(test)]
        observe(Step::Intent, state)?;
        let ticket = state.ticket.as_ref().unwrap();
        {
            let ledger = state
                .fence
                .ledger
                .lock()
                .map_err(|_| anyhow::anyhow!("worker fence ledger unavailable"))?;
            ticket.check(
                ledger
                    .creation
                    .as_ref()
                    .context("creation roster missing")?,
            )?;
        }
        let name = std::ffi::CString::new(ticket.name.as_str())?;
        ensure!(
            unsafe { libc::mkdirat(ticket.parent.as_raw_fd(), name.as_ptr(), 0o700) } == 0,
            "create admitted cgroup: {}",
            std::io::Error::last_os_error()
        );
        #[cfg(test)]
        observe(Step::Created, state)?;
        // Capture the child FD before another fallible journal/configuration step.
        state.root = Some(state.ticket.as_ref().unwrap().child()?);
        #[cfg(test)]
        observe(Step::Captured, state)?;
        let ticket = state.ticket.as_ref().unwrap();
        let root = state.root.as_ref().unwrap();
        let held = root.metadata()?;
        state
            .fence
            .bind_held_creation(&ticket.path, (held.dev(), held.ino()))?;
        #[cfg(test)]
        observe(Step::Bound, state)?;
        crate::cgroup::configure_limits(root, memory, threads)?;
        #[cfg(test)]
        observe(Step::Configured, state)?;
        state.group = Some(AttemptCgroup::from_held(
            ticket.path.clone(),
            root.try_clone()?,
        )?);
        state
            .fence
            .register_creation(state.group.as_ref().unwrap())?;
        #[cfg(test)]
        observe(Step::Registered, state)?;
        Ok(admitted)
    }
    /// No child or group handle escaped; the registered owner moves without IO.
    pub(crate) fn into_worker(self) -> WorkerCustody {
        WorkerCustody::from_admitted(self)
    }
    pub(crate) fn worker_parts(
        mut self,
    ) -> (
        AttemptCgroup,
        Arc<Reservation>,
        Arc<WorkerFence>,
        Option<crate::staging_obligation::Staging>,
        crate::worker_retirement::RetirementAssets,
    ) {
        let mut state = self.state.take().expect("admitted creation state");
        (
            state.group.take().expect("registered creation"),
            state.reservation.clone(),
            state.fence.clone(),
            state.snapshots.take(),
            std::mem::take(&mut state.assets),
        )
    }
}
impl Drop for Admitted {
    fn drop(&mut self) {
        if let Some(state) = self.state.take() {
            let _cleanup = state.cleanup();
        }
    }
}
impl WorkerFence {
    pub(crate) fn admit_staged_group_with_assets(
        self: &Arc<Self>,
        reservation: Arc<Reservation>,
        memory: u64,
        threads: u16,
        snapshots: crate::staging_obligation::Staging,
        assets: crate::worker_retirement::RetirementAssets,
    ) -> Result<Admitted> {
        Admitted::create_inner_with_assets(
            self.clone(),
            reservation,
            memory,
            threads,
            Some(snapshots),
            assets,
            #[cfg(test)]
            &mut observe_engine_crash,
        )
    }
    pub(crate) fn admit_group(
        self: &Arc<Self>,
        reservation: Arc<Reservation>,
        memory: u64,
        threads: u16,
    ) -> Result<Admitted> {
        Admitted::create(self.clone(), reservation, memory, threads)
    }
    #[cfg(test)]
    pub(crate) fn admit_staged_group(
        self: &Arc<Self>,
        reservation: Arc<Reservation>,
        memory: u64,
        threads: u16,
        snapshots: crate::staging_obligation::Staging,
    ) -> Result<Admitted> {
        Admitted::create_staged(self.clone(), reservation, memory, threads, Some(snapshots))
    }
}

#[cfg(test)]
#[path = "worker_cgroup_creation_tests.rs"]
mod tests;

// This hook is compiled only into tests; child-local environment selects one
// exact fixture. Production binaries contain no pause or crash control.
#[cfg(test)]
fn observe_engine_crash(step: Step, state: &State) -> Result<()> {
    let Some(root) = std::env::var_os("PULSAR_ADMITTED_ENGINE_CRASH_ROOT") else {
        return Ok(());
    };
    let root = PathBuf::from(root);
    if state.fence.directory != root.join("private-state")
        || std::env::var("PULSAR_ADMITTED_ENGINE_CRASH_CUT")
            .ok()
            .as_deref()
            != Some(format!("{step:?}").as_str())
    {
        return Ok(());
    }
    let ticket = state.ticket.as_ref().context("test creation ticket")?;
    let identity = state
        .root
        .as_ref()
        .map(|file| file.metadata().map(|meta| (meta.dev(), meta.ino())))
        .transpose()?;
    ensure!(
        state.reservation.admits_worker_memory(1),
        "test lost admitted worker claim"
    );
    let bytes = serde_json::to_vec(&serde_json::json!({
        "step": format!("{step:?}"), "path": ticket.path,
        "held_identity": identity, "scope": state.fence.scope()?,
        "staging": state.snapshots.as_ref().map(|s| s.path()),
    }))?;
    fs::write(root.join("paused.tmp"), bytes)?;
    fs::rename(root.join("paused.tmp"), root.join("paused.json"))?;
    let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < end {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    anyhow::bail!("test owner was not killed at admitted creation cut")
}
