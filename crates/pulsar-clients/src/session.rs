//! Authentication and request construction shared by every first-party client.
//! Only this module persists client credentials; project data remains engine-owned.

use anyhow::{bail, Context, Result};
use pulsar_protocol::*;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Bounded transport phase only; never include endpoint or credential text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BrowserTransportPhase {
    Connect,
    Deadline,
    Call,
}
impl std::fmt::Display for BrowserTransportPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "browser transport {self:?}")
    }
}
impl std::error::Error for BrowserTransportPhase {}

pub trait EngineApi {
    fn execute(
        &mut self,
        command: Command,
        project: Option<ProjectId>,
        revision: Option<RevisionId>,
    ) -> Result<crate::motion::ResponseBody>;

    /// Explicit effect identity for reconciliation; never invent a retry ID.
    fn execute_identified(
        &mut self,
        _request_id: RequestId,
        _command: Command,
        _project: Option<ProjectId>,
        _revision: Option<RevisionId>,
    ) -> Result<crate::motion::ResponseBody> {
        Err(ProtocolError::unsupported("identified request on this client transport").into())
    }

    /// Proposes checked values through the shared upload broker; never commits.
    /// Read is separately required to hydrate the resulting candidate.
    fn upload_edit(
        &mut self,
        _project: ProjectId,
        _revision: RevisionId,
        _program: &MotionProgram,
        _label: &str,
    ) -> Result<crate::motion::CandidateSnapshot> {
        Err(ProtocolError::unsupported("edit upload on this client transport").into())
    }
}

pub struct SessionClient {
    endpoint: PathBuf,
    credentials: ClientCredentials,
    motion: crate::motion::MotionCache,
    operation_deadline: Option<Instant>,
}
impl SessionClient {
    /// Assign one budget to all RPCs and transfers belonging to an admitted job.
    /// A new job or cleanup must explicitly replace or clear the previous budget.
    pub(crate) fn set_operation_deadline(&mut self, deadline: Option<Instant>) {
        self.operation_deadline = deadline;
    }

    /// Source media control retains exact identities without exposing credentials to the page.
    pub(crate) fn browser_media_call_until(
        &self,
        request_id: RequestId,
        command: Command,
        project: ProjectId,
        revision: Option<RevisionId>,
        deadline: Instant,
    ) -> Result<pulsar_protocol::ResponseBody> {
        match &command {
            Command::BrowserMediaCatalog
            | Command::PrepareBrowserMedia { .. }
            | Command::PrepareBrowserMediaSuccessor { .. } => {
                if revision.is_none() {
                    bail!("browser media selection requires a revision");
                }
            }
            Command::BrowserMediaPreparationStatus { .. } | Command::ReleaseBrowserMedia { .. } => {
                if revision.is_some() {
                    bail!("browser media history has no current revision");
                }
            }
            _ => bail!("browser media lane refuses unrelated commands"),
        }
        let request = build_request(
            self.credentials.session.clone(),
            self.credentials.auth_token.clone(),
            request_id,
            command,
            Some(project),
            revision,
        );
        request.validate()?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .context("browser media operation deadline expired")?;
        let mut transport = LocalClient::connect(&self.endpoint, remaining)?;
        transport.set_deadline(deadline)?;
        let response = transport.call(&request)?;
        if response.version != request.version || response.request_id != request.request_id {
            bail!("browser media response identity differs; original outcome remains unknown");
        }
        response.result.map_err(anyhow::Error::from)
    }
    pub(crate) fn open_source_media_until(
        &self,
        binding: &BrowserMediaBinding,
        deadline: Instant,
    ) -> Result<SourceMediaBulkClient> {
        binding.validate()?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .context("source media connection deadline expired")?;
        let mut bulk = SourceMediaBulkClient::connect(
            self.package_bulk_endpoint()?,
            remaining.min(Duration::from_secs(3)),
        )?;
        bulk.set_operation_deadline(Some(deadline));
        let actual = bulk.handshake(&SourceMediaBulkHandshake {
            version: PROTOCOL_VERSION,
            channel: SourceMediaBulkChannel::SourceMedia,
            session: self.credentials.session.clone(),
            auth_token: self.credentials.auth_token.clone(),
            lease_id: binding.lease_id.clone(),
            engine_epoch: binding.engine_epoch.clone(),
            preparation_request_id: binding.preparation_request_id.clone(),
            source_sha256: binding.selection.source.sha256.clone(),
        })?;
        if actual != *binding {
            bail!("source media handshake changed the exact binding");
        }
        Ok(bulk)
    }

    /// Independent native lane using the same credentials and an empty motion cache.
    pub(crate) fn fork_native(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            credentials: ClientCredentials {
                session: self.credentials.session.clone(),
                auth_token: self.credentials.auth_token.clone(),
            },
            motion: crate::motion::MotionCache::default(),
            operation_deadline: None,
        }
    }

    /// Bounded browser native control without hydration or automatic replay.
    pub(crate) fn browser_call_until(
        &self,
        request_id: RequestId,
        command: Command,
        project: ProjectId,
        revision: Option<RevisionId>,
        deadline: std::time::Instant,
    ) -> Result<pulsar_protocol::ResponseBody> {
        match &command {
            Command::BindHandyPlayerGuard { .. }
            | Command::OpenBrowserTiming { .. }
            | Command::PrepareBrowserTimingSuccessor { .. }
            | Command::StartHandyPlaybackFromBrowser { .. } => {
                if revision.is_none() {
                    bail!("Browser guard binding requires an exact revision");
                }
            }
            Command::IssueHandyPlayerChallenge { .. }
            | Command::ObserveHandyPlayer { .. }
            | Command::ReleaseHandyPlayerGuard { .. }
            | Command::HandyPlayerGuardStatus { .. }
            | Command::IssueBrowserTimingProbe { .. }
            | Command::ObserveBrowserTiming { .. }
            | Command::BrowserTimingStatus { .. }
            | Command::BrowserTimingOpenStatus { .. }
            | Command::ReleaseBrowserTiming { .. }
            | Command::HandyControllerStatus { .. }
            | Command::HandyPlaybackPreparationStatus { .. }
            | Command::HandyBrowserStartQualificationStatus { .. }
            | Command::HandyPlaybackStatus { .. }
            | Command::RenewHandyBrowserPlayback { .. }
            | Command::ReleaseHandyBrowserPlayback { .. }
            | Command::HandyBrowserPlaybackStatus { .. }
            | Command::RefreshHandyBrowserPlaybackTiming { .. } => {
                if revision.is_some() {
                    bail!("Browser guard control does not accept a revision");
                }
            }
            _ => bail!("Browser guard control refuses unrelated commands"),
        }
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .context(BrowserTransportPhase::Deadline)
            .context("Browser guard control deadline expired before connection")?;
        let request = build_request(
            self.credentials.session.clone(),
            self.credentials.auth_token.clone(),
            request_id,
            command,
            Some(project),
            revision,
        );
        request.validate()?;
        let mut transport = LocalClient::connect(&self.endpoint, remaining)
            .context(BrowserTransportPhase::Connect)?;
        transport
            .set_deadline(deadline)
            .context(BrowserTransportPhase::Deadline)?;
        let response = transport
            .call(&request)
            .context(BrowserTransportPhase::Call)?;
        if response.version != request.version || response.request_id != request.request_id {
            bail!(
                "Browser guard response identity mismatch; outcome is unknown and was not replayed"
            );
        }
        response.result.map_err(anyhow::Error::from)
    }

    pub(crate) fn upload_edit_retained(
        &mut self,
        intent: &mut crate::motion::RetainedEditUpload,
        program: &MotionProgram,
        checkpoint: &mut impl FnMut(&crate::motion::RetainedEditUpload) -> Result<()>,
    ) -> Result<crate::motion::CandidateSnapshot> {
        let mut io =
            AuthenticatedMotionIo::new(&self.endpoint, &self.credentials, self.operation_deadline)?;
        self.motion
            .upload_edit_retained(intent, program, &mut io, checkpoint)
    }
    /// App-owned output retention beside the Engine endpoint; never credentials.
    pub(crate) fn output_state_directory(&self) -> Result<PathBuf> {
        Ok(self
            .endpoint
            .parent()
            .context("Engine endpoint parent absent")?
            .to_path_buf())
    }
}

/// Public session identity is not an authentication secret. Do not derive
/// Debug: diagnostics must never print the bearer credential.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientCredentials {
    session: SessionId,
    auth_token: String,
}

impl ClientCredentials {
    fn validate(&self) -> Result<()> {
        if self.auth_token.is_empty() || self.auth_token.len() > 2048 {
            bail!("Invalid first-party authentication credential");
        }
        Ok(())
    }
}

fn private_new(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

struct CredentialLock(PathBuf);
impl Drop for CredentialLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn read_session(path: &Path) -> Result<Option<ClientCredentials>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4096 {
        bail!("Unsafe client credential file: {}", path.display());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            bail!(
                "Client credential file must be owner-only: {}",
                path.display()
            );
        }
    }
    let credentials: ClientCredentials = serde_json::from_slice(&fs::read(path)?)
        .context("First-party credentials are invalid or use the obsolete session-only format; explicit re-pairing is required")?;
    credentials.validate()?;
    Ok(Some(credentials))
}

impl SessionClient {
    pub fn open_live_encoded(&self, report: &LiveCaptureReport) -> Result<LiveEncodedClient> {
        report.validate()?;
        if !report.active || report.options.encoded_source.is_none() {
            bail!("Active encoded source capture required");
        }
        let mut bulk = LiveEncodedClient::connect(
            self.package_bulk_endpoint()?,
            operation_timeout(self.operation_deadline, Duration::from_secs(35))?,
        )?;
        if let Some(deadline) = self.operation_deadline {
            bulk.limit_deadline(deadline)?;
        }
        bulk.handshake(&LiveBulkHandshake {
            version: PROTOCOL_VERSION,
            channel: LiveBulkChannel::LiveEncodedSource,
            session: self.credentials.session.clone(),
            auth_token: self.credentials.auth_token.clone(),
            engine_epoch: report.engine_epoch.clone(),
            identity: report.identity.clone(),
        })?;
        Ok(bulk)
    }
    /// Read capture status within one absolute control deadline, without retry.
    pub(crate) fn retained_evidence_io(&self) -> Result<impl crate::motion::MotionIo + '_> {
        AuthenticatedMotionIo::new(&self.endpoint, &self.credentials, self.operation_deadline)
    }
    pub fn capture_status_until(
        &self,
        project: ProjectId,
        revision: RevisionId,
        epoch: AttemptId,
        deadline: std::time::Instant,
    ) -> Result<LiveCaptureReport> {
        let deadline = earlier_deadline(deadline, self.operation_deadline);
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .context("Capture status deadline expired")?;
        let request = build_request(
            self.credentials.session.clone(),
            self.credentials.auth_token.clone(),
            new_request_id(),
            Command::LiveCaptureStatus {
                epoch: epoch.clone(),
            },
            Some(project.clone()),
            Some(revision),
        );
        let mut transport = LocalClient::connect(&self.endpoint, remaining)?;
        transport.set_deadline(deadline)?;
        let response = transport.call(&request)?;
        if response.version != request.version || response.request_id != request.request_id {
            bail!("Capture response identity mismatch; outcome was not replayed");
        }
        let pulsar_protocol::ResponseBody::LiveCapture(report) = response.result? else {
            bail!("Unexpected capture status response");
        };
        report.validate()?;
        if report.identity.project != project
            || report.identity.revision != revision
            || report.identity.epoch != epoch
        {
            bail!("Capture status scope mismatch");
        }
        Ok(*report)
    }
    /// Attach encoded input with all transport I/O bounded by a caller deadline.
    pub fn open_live_encoded_until(
        &self,
        report: &LiveCaptureReport,
        deadline: std::time::Instant,
    ) -> Result<LiveEncodedClient> {
        let deadline = earlier_deadline(deadline, self.operation_deadline);
        report.validate()?;
        if !report.active || report.options.encoded_source.is_none() {
            bail!("Active encoded source capture required");
        }
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .context("Encoded attachment deadline expired")?;
        let mut bulk = LiveEncodedClient::connect(
            self.package_bulk_endpoint()?,
            remaining.min(Duration::from_secs(35)),
        )?;
        bulk.limit_deadline(deadline)?;
        bulk.handshake(&LiveBulkHandshake {
            version: PROTOCOL_VERSION,
            channel: LiveBulkChannel::LiveEncodedSource,
            session: self.credentials.session.clone(),
            auth_token: self.credentials.auth_token.clone(),
            engine_epoch: report.engine_epoch.clone(),
            identity: report.identity.clone(),
        })?;
        Ok(bulk)
    }
    /// Non-secret namespace binding; does not reveal or establish authentication.
    pub fn recovery_credential_binding(&self) -> String {
        use sha2::{Digest, Sha256};
        format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(&self.credentials.session, &self.credentials.auth_token))
                    .expect("string credentials serialize")
            )
        )
    }
    /// Public claimed identity; each request still requires engine authentication.
    pub fn session_id(&self) -> &SessionId {
        &self.credentials.session
    }

    pub fn from_credentials(
        endpoint: &Path,
        session: SessionId,
        auth_token: String,
    ) -> Result<Self> {
        let credentials = ClientCredentials {
            session,
            auth_token,
        };
        credentials.validate()?;
        Ok(Self {
            endpoint: endpoint.to_owned(),
            credentials,
            motion: crate::motion::MotionCache::default(),
            operation_deadline: None,
        })
    }

    pub fn connect(endpoint: &Path, bootstrap: &Path) -> Result<Self> {
        let directory = bootstrap
            .parent()
            .context("Bootstrap must live in the engine's private state directory")?;
        let credentials = directory.join("first-party.session.json");
        if let Some(credentials) = read_session(&credentials)? {
            return Ok(Self {
                endpoint: endpoint.to_owned(),
                credentials,
                motion: crate::motion::MotionCache::default(),
                operation_deadline: None,
            });
        }
        let lock_path = directory.join("first-party.session.lock");
        let mut acquired = false;
        for _ in 0..100 {
            match private_new(&lock_path) {
                Ok(_) => {
                    acquired = true;
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if let Some(credentials) = read_session(&credentials)? {
                        return Ok(Self {
                            endpoint: endpoint.to_owned(),
                            credentials,
                            motion: crate::motion::MotionCache::default(),
                            operation_deadline: None,
                        });
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(error.into()),
            }
        }
        if !acquired {
            bail!("First-party pairing is busy or interrupted. Check {} before explicitly removing an abandoned lock.", lock_path.display());
        }
        let _lock = CredentialLock(lock_path);
        if let Some(credentials) = read_session(&credentials)? {
            return Ok(Self {
                endpoint: endpoint.to_owned(),
                credentials,
                motion: crate::motion::MotionCache::default(),
                operation_deadline: None,
            });
        }
        let metadata = fs::symlink_metadata(bootstrap)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4096 {
            bail!("Invalid engine bootstrap credential");
        }
        let pairing_token = fs::read_to_string(bootstrap)?.trim().to_owned();
        let request = Request::new(
            new_request_id(),
            Command::Pair {
                client_name: "Pulsar first-party clients".to_owned(),
                pairing_token,
            },
        );
        let response = call(endpoint, &request, None)?;
        let ResponseBody::Paired {
            session,
            auth_token,
        } = response
        else {
            bail!("Engine returned an invalid pairing response")
        };
        let credential_value = ClientCredentials {
            session,
            auth_token,
        };
        credential_value.validate()?;
        let temporary =
            directory.join(format!(".first-party.session.{}.tmp", uuid::Uuid::new_v4()));
        let outcome = (|| -> Result<()> {
            let mut file = private_new(&temporary)?;
            file.write_all(&serde_json::to_vec(&credential_value)?)?;
            file.sync_all()?;
            fs::rename(&temporary, &credentials)?;
            #[cfg(unix)]
            std::fs::File::open(directory)?.sync_all()?;
            Ok(())
        })();
        if outcome.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        outcome?;
        Ok(Self {
            endpoint: endpoint.to_owned(),
            credentials: credential_value,
            motion: crate::motion::MotionCache::default(),
            operation_deadline: None,
        })
    }
}

pub fn new_request_id() -> RequestId {
    RequestId::new(uuid::Uuid::new_v4().to_string()).expect("UUID is a valid request identifier")
}

pub fn build_request(
    session: SessionId,
    auth_token: String,
    request_id: RequestId,
    command: Command,
    project: Option<ProjectId>,
    revision: Option<RevisionId>,
) -> Request {
    Request {
        version: PROTOCOL_VERSION,
        request_id,
        session: Some(session),
        auth_token: Some(auth_token),
        project,
        expected_revision: revision,
        command,
    }
}

fn earlier_deadline(deadline: Instant, operation_deadline: Option<Instant>) -> Instant {
    operation_deadline.map_or(deadline, |operation| operation.min(deadline))
}

fn operation_timeout(deadline: Option<Instant>, maximum: Duration) -> Result<Duration> {
    match deadline {
        Some(deadline) => deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .map(|remaining| remaining.min(maximum))
            .context("Client operation deadline expired before connection"),
        None => Ok(maximum),
    }
}

fn call(endpoint: &Path, request: &Request, deadline: Option<Instant>) -> Result<ResponseBody> {
    let mut transport = LocalClient::connect(
        endpoint,
        operation_timeout(deadline, Duration::from_secs(120))?,
    )?;
    if let Some(deadline) = deadline {
        transport.set_deadline(deadline)?;
    }
    let response = transport.call(request)?;
    if response.version != request.version || response.request_id != request.request_id {
        bail!("Engine response identity mismatch; request outcome is unknown and was not replayed");
    }
    let body = response.result.map_err(anyhow::Error::from)?;
    if matches!(request.command, Command::PrepareAssistantImage { .. }) {
        let ResponseBody::AssistantImage(image) = &body else {
            bail!("Unexpected prepared assistant image response");
        };
        image.validate_for(request)?;
    }
    if matches!(
        request.command,
        Command::StartWasmEdit { .. }
            | Command::WasmEditStatus { .. }
            | Command::CancelWasmEdit { .. }
    ) {
        let ResponseBody::WasmEdit(report) = &body else {
            bail!("Unexpected WASM edit response");
        };
        report.validate_for(request)?;
    }
    if matches!(request.command, Command::ResolveWasmStart { .. }) {
        let ResponseBody::WasmStartResolution(resolution) = &body else {
            bail!("Unexpected WASM start resolution response");
        };
        resolution.validate_for(request)?;
    }
    if matches!(request.command, Command::ListArchivedWasmEdits { .. }) {
        let ResponseBody::WasmArchivePage(page) = &body else {
            bail!("Unexpected WASM archive history response");
        };
        page.validate_for(request)?;
    }
    if matches!(request.command, Command::ListWasmEdits) {
        let ResponseBody::WasmEditCatalog(catalog) = &body else {
            bail!("Unexpected WASM catalog response");
        };
        catalog.validate_for(request)?;
    }
    if matches!(request.command, Command::ArchiveCompletedWasmEdit { .. }) {
        let ResponseBody::WasmEditArchived(report) = &body else {
            bail!("Unexpected WASM archive response");
        };
        report.validate_archive_for(request)?;
    }
    if matches!(request.command, Command::RemoveUnsuccessfulWasmEdit { .. }) {
        let ResponseBody::WasmEditRemoved(report) = &body else {
            bail!("Unexpected WASM removal response");
        };
        report.validate_removal_for(request)?;
    }
    if matches!(
        request.command,
        Command::StartAssistant { .. }
            | Command::AssistantStatus { .. }
            | Command::CancelAssistant { .. }
    ) {
        let ResponseBody::Assistant(report) = &body else {
            bail!("Unexpected assistant response");
        };
        report.validate_for(request)?;
    }
    if matches!(request.command, Command::ResolveAssistantStart { .. }) {
        let ResponseBody::AssistantStartResolution(value) = &body else {
            bail!("Unexpected assistant resolution response");
        };
        value.validate_for(request)?;
    }
    if matches!(request.command, Command::AssistantProfiles) {
        let ResponseBody::AssistantProfiles(profiles) = &body else {
            bail!("Unexpected assistant profile response");
        };
        if profiles.len() > 8 {
            bail!("Assistant profile catalog exceeds bounds");
        }
        let mut ids = std::collections::BTreeSet::new();
        for profile in profiles {
            profile.validate()?;
            if !ids.insert(profile.profile_id.as_str()) {
                bail!("Duplicate assistant profile");
            }
        }
    }
    if matches!(request.command, Command::ListAssistantAttempts) {
        let ResponseBody::AssistantAttempts(reports) = &body else {
            bail!("Unexpected assistant attempt catalog");
        };
        if reports.len() > 128 {
            bail!("Assistant attempt catalog exceeds bounds");
        }
        let mut ids = std::collections::BTreeSet::new();
        for report in reports {
            report.validate_for(request)?;
            if !ids.insert(report.attempt_id.as_str()) {
                bail!("Duplicate assistant attempt");
            }
        }
    }
    if matches!(request.command, Command::WasmEditorInfo) {
        let ResponseBody::WasmEditorInfo(info) = &body else {
            bail!("Unexpected WASM editor info response");
        };
        info.worker.validate()?;
        if info.abi != WASM_PROCESSING_ABI
            || info.maximum_buffer_bytes != WASM_BUFFER_MAX_BYTES as u64
        {
            bail!("WASM editor ABI information differs");
        }
    }
    if matches!(&request.command, Command::CheckModelPackCatalog { .. }) {
        let ResponseBody::ModelPackCatalog(report) = &body else {
            bail!("Unexpected installed catalog association response");
        };
        report.validate_for(&request.command)?;
    }
    Ok(body)
}

impl EngineApi for SessionClient {
    fn execute_identified(
        &mut self,
        request_id: RequestId,
        command: Command,
        project: Option<ProjectId>,
        revision: Option<RevisionId>,
    ) -> Result<crate::motion::ResponseBody> {
        self.execute_with_id(request_id, command, project, revision)
    }

    fn execute(
        &mut self,
        command: Command,
        project: Option<ProjectId>,
        revision: Option<RevisionId>,
    ) -> Result<crate::motion::ResponseBody> {
        self.execute_with_id(new_request_id(), command, project, revision)
    }

    fn upload_edit(
        &mut self,
        project: ProjectId,
        revision: RevisionId,
        program: &MotionProgram,
        label: &str,
    ) -> Result<crate::motion::CandidateSnapshot> {
        let mut io =
            AuthenticatedMotionIo::new(&self.endpoint, &self.credentials, self.operation_deadline)?;
        self.motion
            .upload_edit(project, revision, program, label, &mut io)
    }
}

struct AuthenticatedMotionIo<'a> {
    endpoint: &'a Path,
    credentials: &'a ClientCredentials,
    bulk_endpoint: PathBuf,
    operation_deadline: Option<Instant>,
}
impl<'a> AuthenticatedMotionIo<'a> {
    fn new(
        endpoint: &'a Path,
        credentials: &'a ClientCredentials,
        operation_deadline: Option<Instant>,
    ) -> Result<Self> {
        let bulk_endpoint = endpoint
            .parent()
            .context("Engine endpoint has no private parent")?
            .join("bulk.sock");
        Ok(Self {
            endpoint,
            credentials,
            bulk_endpoint,
            operation_deadline,
        })
    }
}
impl crate::motion::MotionIo for AuthenticatedMotionIo<'_> {
    fn control_identified(
        &mut self,
        request: RequestId,
        command: Command,
        project: ProjectId,
        revision: Option<RevisionId>,
    ) -> Result<pulsar_protocol::ResponseBody> {
        call(
            self.endpoint,
            &build_request(
                self.credentials.session.clone(),
                self.credentials.auth_token.clone(),
                request,
                command,
                Some(project),
                revision,
            ),
            self.operation_deadline,
        )
    }
    fn control(
        &mut self,
        command: Command,
        project: ProjectId,
        revision: Option<RevisionId>,
    ) -> Result<ResponseBody> {
        call(
            self.endpoint,
            &build_request(
                self.credentials.session.clone(),
                self.credentials.auth_token.clone(),
                new_request_id(),
                command,
                Some(project),
                revision,
            ),
            self.operation_deadline,
        )
    }

    fn bulk_endpoint(&self) -> &Path {
        &self.bulk_endpoint
    }

    fn open_bulk(
        &mut self,
        lease: &TransferLease,
        deadline: std::time::Instant,
    ) -> Result<Box<dyn crate::motion::BulkIo>> {
        let deadline = earlier_deadline(deadline, self.operation_deadline);
        if lease.bulk_endpoint != self.bulk_endpoint {
            bail!("Bulk endpoint redirect rejected before authentication");
        }
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .context("Transfer lease deadline elapsed before bulk connection")?;
        let timeout = remaining.min(Duration::from_secs(10));
        let mut bulk = BulkClient::connect(&self.bulk_endpoint, timeout)?;
        bulk.set_deadline(deadline)?;
        let actual = bulk.handshake(&BulkHandshake {
            version: PROTOCOL_VERSION,
            session: self.credentials.session.clone(),
            auth_token: self.credentials.auth_token.clone(),
            lease_id: lease.lease_id.clone(),
            engine_epoch: lease.engine_epoch.clone(),
        })?;
        crate::motion::validate_handshake(lease, &actual)?;
        Ok(Box::new(bulk))
    }
}

impl SessionClient {
    pub(crate) fn package_request(
        &self,
        request_id: RequestId,
        command: Command,
        project: ProjectId,
        revision: Option<RevisionId>,
    ) -> Result<pulsar_protocol::ResponseBody> {
        self.package_request_until(
            request_id,
            command,
            project,
            revision,
            std::time::Instant::now() + Duration::from_secs(10),
        )
    }

    pub(crate) fn package_request_until(
        &self,
        request_id: RequestId,
        command: Command,
        project: ProjectId,
        revision: Option<RevisionId>,
        deadline: std::time::Instant,
    ) -> Result<pulsar_protocol::ResponseBody> {
        let deadline = earlier_deadline(deadline, self.operation_deadline)
            .min(std::time::Instant::now() + Duration::from_secs(10));
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .context("Package control absolute deadline expired before connection")?;
        let request = build_request(
            self.credentials.session.clone(),
            self.credentials.auth_token.clone(),
            request_id,
            command,
            Some(project),
            revision,
        );
        let mut transport = LocalClient::connect(&self.endpoint, remaining)?;
        transport.set_deadline(deadline)?;
        let response = transport.call(&request)?;
        if response.version != request.version || response.request_id != request.request_id {
            bail!("Package response identity mismatch; request outcome is unknown and was not replayed");
        }
        response.result.map_err(anyhow::Error::from)
    }

    pub(crate) fn package_bulk_endpoint(&self) -> Result<PathBuf> {
        Ok(self
            .endpoint
            .parent()
            .context("Engine endpoint has no private parent")?
            .join("bulk.sock"))
    }

    pub(crate) fn open_package_bulk(
        &self,
        lease: &PackageDownloadLease,
        deadline: std::time::Instant,
        range: PackageChunkRange,
    ) -> Result<PackageBulkClient> {
        let deadline = earlier_deadline(deadline, self.operation_deadline);
        let endpoint = self.package_bulk_endpoint()?;
        if lease.bulk_endpoint != endpoint {
            bail!("Package bulk endpoint redirect rejected before authentication");
        }
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .context("Package lease expired before connection")?;
        let mut bulk =
            PackageBulkClient::connect(&endpoint, remaining.min(Duration::from_secs(10)))?;
        bulk.set_deadline(deadline)?;
        let actual = bulk.handshake(&PackageBulkHandshake {
            version: PROTOCOL_VERSION,
            channel: PackageBulkChannel::ProjectPackageDownload,
            session: self.credentials.session.clone(),
            auth_token: self.credentials.auth_token.clone(),
            lease_id: lease.lease_id.clone(),
            engine_epoch: lease.engine_epoch.clone(),
            operation_id: lease.operation_id.clone(),
            artifact_sha256: lease.artifact.sha256.clone(),
        })?;
        crate::project_package::validate_package_handshake(lease, &actual, range)?;
        Ok(bulk)
    }
}

use pulsar_protocol::{
    PackageUploadChannel, PackageUploadChunkReceipt, PackageUploadClient, PackageUploadHandshake,
    PackageUploadLease,
};

impl SessionClient {
    /// Import operations create a new authority context; an existing project or
    /// revision must never be invented to satisfy an export/motion wrapper.
    pub(crate) fn import_request_until(
        &self,
        request_id: RequestId,
        command: Command,
        deadline: std::time::Instant,
    ) -> Result<pulsar_protocol::ResponseBody> {
        if !matches!(
            &command,
            Command::StartProjectImport { .. }
                | Command::ProjectImportStatus { .. }
                | Command::BeginProjectPackageUpload { .. }
                | Command::ProjectPackageUploadStatus { .. }
                | Command::AbandonProjectPackageUpload { .. }
                | Command::SealProjectImport { .. }
                | Command::CancelProjectImport { .. }
        ) {
            bail!("Import authority wrapper refuses unrelated commands");
        }
        let deadline = earlier_deadline(deadline, self.operation_deadline)
            .min(std::time::Instant::now() + Duration::from_secs(10));
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .context("Import control absolute deadline expired before connection")?;
        let request = build_request(
            self.credentials.session.clone(),
            self.credentials.auth_token.clone(),
            request_id,
            command,
            None,
            None,
        );
        let mut transport = LocalClient::connect(&self.endpoint, remaining)?;
        transport.set_deadline(deadline)?;
        let response = transport.call(&request)?;
        if response.version != request.version || response.request_id != request.request_id {
            bail!("Import response identity mismatch; outcome is unknown and was not replayed with a new ID");
        }
        response.result.map_err(anyhow::Error::from)
    }

    pub(crate) fn open_package_upload(
        &self,
        lease: &PackageUploadLease,
        deadline: std::time::Instant,
        handshake_deadline: std::time::Instant,
        chunk: &PackageUploadChunkReceipt,
    ) -> Result<PackageUploadClient> {
        let deadline = earlier_deadline(deadline, self.operation_deadline);
        let endpoint = self.package_bulk_endpoint()?;
        if lease.bulk_endpoint != endpoint {
            bail!("Import bulk endpoint redirect rejected before authentication");
        }
        let handshake_deadline = handshake_deadline.min(deadline);
        let mut transport = PackageUploadClient::connect_until(
            &endpoint,
            Duration::from_secs(10),
            handshake_deadline,
        )?;
        // This remains the ORIGINAL transfer lifetime, never the shorter
        // connection-contention retry budget.
        transport.set_deadline(deadline)?;
        let actual = transport.handshake_until(
            &PackageUploadHandshake {
                version: PROTOCOL_VERSION,
                channel: PackageUploadChannel::ProjectPackageUpload,
                session: self.credentials.session.clone(),
                auth_token: self.credentials.auth_token.clone(),
                lease_id: lease.lease_id.clone(),
                engine_epoch: lease.engine_epoch.clone(),
                operation_id: lease.operation_id.clone(),
                generation: lease.generation,
                package_sha256: lease.package.sha256.clone(),
            },
            handshake_deadline,
        )?;
        crate::project_package_import::validate_import_handshake(lease, &actual, chunk)?;
        Ok(transport)
    }
}

impl SessionClient {
    /// Caller retains the ID before dispatch. Lost acknowledgements may only be
    /// retried with the same ID and exact arguments; no automatic replay occurs.
    pub(crate) fn execute_with_id(
        &mut self,
        request_id: RequestId,
        command: Command,
        project: Option<ProjectId>,
        revision: Option<RevisionId>,
    ) -> Result<crate::motion::ResponseBody> {
        let expected_project = match &command {
            Command::OpenProject { project_id } => Some(project_id.clone()),
            _ => project.clone(),
        };
        let expected_candidate = match &command {
            Command::GetCandidate { candidate_id }
            | Command::CandidateImageOriginStatus { candidate_id }
            | Command::CandidateImageOriginPixels { candidate_id, .. } => {
                Some(candidate_id.clone())
            }
            _ => None,
        };
        let request = build_request(
            self.credentials.session.clone(),
            self.credentials.auth_token.clone(),
            request_id,
            command,
            project,
            revision,
        );
        let response = call(&self.endpoint, &request, self.operation_deadline)?;
        if let Command::CandidateImageOriginPixels { record_sha256, .. } = &request.command {
            match &response {
                pulsar_protocol::ResponseBody::CandidateImageOriginPixels(pixels)
                    if pixels.record.sha256 == *record_sha256 => {}
                _ => bail!("Image pixels response selected origin mismatch"),
            }
        }
        if let Command::WithdrawModelPackApproval { .. } = &request.command {
            match &response {
                pulsar_protocol::ResponseBody::ModelPackWithdrawal(report) => {
                    report.validate_reply(&request)?
                }
                _ => bail!("Unexpected withdrawal response"),
            }
        }
        let mut io =
            AuthenticatedMotionIo::new(&self.endpoint, &self.credentials, self.operation_deadline)?;
        self.motion.hydrate(
            response,
            expected_project.as_ref(),
            expected_candidate.as_ref(),
            &mut io,
        )
    }
}

impl SessionClient {
    /// Opens a boot/actor/project/revision-bound pixel channel. A capture status
    /// report is a descriptor; the engine authenticates it again before effects.
    pub fn open_live_bulk(&self, report: &LiveCaptureReport) -> Result<LiveBulkClient> {
        report.validate()?;
        if !report.active {
            bail!("Capture lease is retired");
        }
        let endpoint = self.package_bulk_endpoint()?;
        let mut bulk = LiveBulkClient::connect(
            endpoint,
            operation_timeout(self.operation_deadline, Duration::from_secs(10))?,
        )?;
        bulk.set_operation_deadline(self.operation_deadline);
        bulk.handshake(&LiveBulkHandshake {
            version: PROTOCOL_VERSION,
            channel: LiveBulkChannel::LiveCapture,
            session: self.credentials.session.clone(),
            auth_token: self.credentials.auth_token.clone(),
            engine_epoch: report.engine_epoch.clone(),
            identity: report.identity.clone(),
        })?;
        Ok(bulk)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn accept_test_stream(
        listener: &std::os::unix::net::UnixListener,
    ) -> std::os::unix::net::UnixStream {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    stream
                        .set_write_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    return stream;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "test accept deadline expired");
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(error) => panic!("test accept failed: {error}"),
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn stalled_execute_uses_operation_deadline_and_keeps_original_request_id() {
        use std::io::Read;
        use std::os::unix::net::UnixListener;

        let root = tempfile::tempdir().unwrap();
        let endpoint = root.path().join("engine.sock");
        let listener = UnixListener::bind(&endpoint).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = accept_test_stream(&listener);
            let request: Request = read_message(&mut stream).unwrap();
            let disconnected = matches!(stream.read(&mut [0]), Ok(0));
            let no_replay = matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock);
            (request.request_id, disconnected, no_replay)
        });
        let original = RequestId::new("original-stalled-execute").unwrap();
        let mut client = SessionClient::from_credentials(
            &endpoint,
            SessionId::new("operation-session").unwrap(),
            "test-credential".into(),
        )
        .unwrap();
        let started = Instant::now();
        client.set_operation_deadline(Some(started + Duration::from_millis(150)));
        let result = client.execute_identified(original.clone(), Command::Capabilities, None, None);
        let elapsed = started.elapsed();
        let (observed, disconnected, no_replay) = server.join().unwrap();
        assert!(result.is_err());
        assert!(
            elapsed < Duration::from_secs(1),
            "operation timeout was reset: {elapsed:?}"
        );
        assert_eq!(observed, original);
        assert!(disconnected && no_replay);
    }

    #[cfg(unix)]
    #[test]
    fn stalled_motion_handshake_cannot_extend_client_operation_deadline() {
        use crate::motion::MotionIo;
        use std::io::Read;
        use std::os::unix::net::UnixListener;

        let root = tempfile::tempdir().unwrap();
        let endpoint = root.path().join("engine.sock");
        let bulk_endpoint = root.path().join("bulk.sock");
        let listener = UnixListener::bind(&bulk_endpoint).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = accept_test_stream(&listener);
            let handshake: BulkHandshake = read_bulk_header(&mut stream).unwrap();
            let disconnected = matches!(stream.read(&mut [0]), Ok(0));
            (handshake.lease_id, disconnected)
        });
        let mut client = SessionClient::from_credentials(
            &endpoint,
            SessionId::new("operation-session").unwrap(),
            "test-credential".into(),
        )
        .unwrap();
        let lease = TransferLease {
            lease_id: TransferId::new("original-motion-lease").unwrap(),
            engine_epoch: "engine-epoch".into(),
            project_id: ProjectId::new("operation-project").unwrap(),
            direction: TransferDirection::Download,
            sha256: "a".repeat(64),
            total_byte_len: 64,
            offset: 0,
            byte_len: 64,
            accepted_prefix: 0,
            expires_after_ms: 5000,
            bulk_endpoint,
        };
        let started = Instant::now();
        client.set_operation_deadline(Some(started + Duration::from_millis(150)));
        let mut io = client.retained_evidence_io().unwrap();
        let result = io.open_bulk(&lease, started + Duration::from_secs(5));
        let elapsed = started.elapsed();
        let (observed, disconnected) = server.join().unwrap();
        assert!(result.is_err());
        assert!(
            elapsed < Duration::from_secs(1),
            "bulk extended the operation: {elapsed:?}"
        );
        assert_eq!(observed, lease.lease_id);
        assert!(disconnected);
    }

    #[cfg(unix)]
    #[test]
    fn motion_subcalls_keep_one_deadline_until_the_caller_replaces_it() {
        use crate::motion::MotionIo;
        use std::os::unix::net::UnixListener;

        let root = tempfile::tempdir().unwrap();
        let endpoint = root.path().join("engine.sock");
        let listener = UnixListener::bind(&endpoint).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = accept_test_stream(&listener);
            let request: Request = read_message(&mut stream).unwrap();
            write_message(
                &mut stream,
                &Response {
                    version: PROTOCOL_VERSION,
                    request_id: request.request_id.clone(),
                    result: Ok(pulsar_protocol::ResponseBody::Ack),
                },
            )
            .unwrap();
            request.request_id
        });
        let original = RequestId::new("first-motion-control").unwrap();
        let project = ProjectId::new("operation-project").unwrap();
        let mut client = SessionClient::from_credentials(
            &endpoint,
            SessionId::new("operation-session").unwrap(),
            "test-credential".into(),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_millis(250);
        client.set_operation_deadline(Some(deadline));
        let first = client.retained_evidence_io().unwrap().control_identified(
            original.clone(),
            Command::Capabilities,
            project.clone(),
            None,
        );
        assert_eq!(server.join().unwrap(), original);
        assert!(first.is_ok());
        if let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            std::thread::sleep(remaining + Duration::from_millis(5));
        }
        let error = client
            .retained_evidence_io()
            .unwrap()
            .control_identified(
                RequestId::new("second-motion-control").unwrap(),
                Command::Capabilities,
                project,
                None,
            )
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("operation deadline expired before connection"));
        assert!(client.fork_native().operation_deadline.is_none());
        client.set_operation_deadline(None);
        assert!(client.operation_deadline.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn browser_control_distinguishes_deadline_connect_and_call_failures() {
        use std::os::unix::net::UnixListener;

        let root = tempfile::tempdir().unwrap();
        let endpoint = root.path().join("engine.sock");
        let client = SessionClient::from_credentials(
            &endpoint,
            SessionId::new("phase-session").unwrap(),
            "synthetic-private-token".into(),
        )
        .unwrap();
        let invoke = |deadline| {
            client.browser_call_until(
                new_request_id(),
                Command::BrowserTimingOpenStatus {
                    original_request_id: new_request_id(),
                },
                ProjectId::new("phase-project").unwrap(),
                None,
                deadline,
            )
        };
        let error = invoke(Instant::now()).unwrap_err();
        assert_eq!(
            error.downcast_ref::<BrowserTransportPhase>(),
            Some(&BrowserTransportPhase::Deadline)
        );
        let error = invoke(Instant::now() + Duration::from_millis(500)).unwrap_err();
        assert_eq!(
            error.downcast_ref::<BrowserTransportPhase>(),
            Some(&BrowserTransportPhase::Connect)
        );

        let listener = UnixListener::bind(&endpoint).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _: Request = read_message(&mut stream).unwrap();
            std::thread::sleep(Duration::from_millis(150));
        });
        let error = invoke(Instant::now() + Duration::from_millis(50)).unwrap_err();
        assert_eq!(
            error.downcast_ref::<BrowserTransportPhase>(),
            Some(&BrowserTransportPhase::Call)
        );
        server.join().unwrap();
    }

    #[test]
    fn browser_control_refuses_unrelated_commands_before_connecting() {
        let client = SessionClient::from_credentials(
            Path::new("/unused/engine.sock"),
            SessionId::new("browser-session").unwrap(),
            "test-credential".into(),
        )
        .unwrap();
        let error = client
            .fork_native()
            .browser_call_until(
                new_request_id(),
                Command::Capabilities,
                ProjectId::new("browser-project").unwrap(),
                None,
                std::time::Instant::now() + Duration::from_secs(1),
            )
            .unwrap_err();
        assert!(error.to_string().contains("refuses unrelated commands"));
    }

    #[test]
    fn browser_control_refuses_expired_deadline_and_unexpected_revision() {
        let client = SessionClient::from_credentials(
            Path::new("/unused/engine.sock"),
            SessionId::new("browser-session").unwrap(),
            "test-credential".into(),
        )
        .unwrap();
        for revision in [None, Some(RevisionId::new(0))] {
            let error = client
                .browser_call_until(
                    new_request_id(),
                    Command::HandyPlayerGuardStatus {
                        binding_request_id: new_request_id(),
                        owner_proof: "test-owner-proof".into(),
                    },
                    ProjectId::new("browser-project").unwrap(),
                    revision,
                    std::time::Instant::now(),
                )
                .unwrap_err();
            assert!(error.to_string().contains(if revision.is_some() {
                "does not accept a revision"
            } else {
                "deadline expired before connection"
            }));
        }
    }

    #[test]
    fn cli_and_gui_share_identical_authority_and_revision_envelopes() {
        let session = SessionId::new("session-1").unwrap();
        let request = RequestId::new("request-1").unwrap();
        let project = ProjectId::new("project-1").unwrap();
        let cli = build_request(
            session.clone(),
            "secret-credential".into(),
            request.clone(),
            Command::Undo,
            Some(project.clone()),
            Some(RevisionId::new(7)),
        );
        let gui = build_request(
            session,
            "secret-credential".into(),
            request,
            Command::Undo,
            Some(project),
            Some(RevisionId::new(7)),
        );
        assert_eq!(
            serde_json::to_value(cli).unwrap(),
            serde_json::to_value(gui).unwrap()
        );
    }

    #[test]
    fn session_identity_alone_is_not_a_client_credential() {
        assert!(serde_json::from_str::<ClientCredentials>("\"public-session\"").is_err());
        assert!(
            serde_json::from_str::<ClientCredentials>("{\"session\":\"public-session\"}").is_err()
        );
        assert!(ClientCredentials {
            session: SessionId::new("public-session").unwrap(),
            auth_token: String::new()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn request_authentication_is_distinct_and_redacted_in_debug() {
        let request = build_request(
            SessionId::new("public-session").unwrap(),
            "private-auth-token".into(),
            new_request_id(),
            Command::Capabilities,
            None,
            None,
        );
        assert_eq!(request.auth_token.as_deref(), Some("private-auth-token"));
        assert!(!format!("{request:?}").contains("private-auth-token"));
    }
}
