//! Independent, bounded browser timing IPC. Native clocks and credentials stay native.
use super::*;

const OPEN_TIMEOUT: Duration = Duration::from_secs(3);
const CONTROL_TIMEOUT: Duration = Duration::from_millis(200);
const RECORD_LIMIT: usize = 16;
const QUEUE_LIMIT: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReleaseState {
    Needed,
    Running,
    Acknowledged,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum FailureStage {
    Local,
    Exchange,
    Reply,
    Queue,
    Delivery,
}
impl std::fmt::Display for FailureStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "browser timing {self:?}")
    }
}
impl std::error::Error for FailureStage {}

/// Created only by the locked pre-dispatch retirement check, before any IPC.
#[derive(Debug)]
struct RetiredBeforeDispatch;
impl std::fmt::Display for RetiredBeforeDispatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("timing scope retired before dispatch")
    }
}
impl std::error::Error for RetiredBeforeDispatch {}

/// One bounded, allowlisted cause per original record. Raw error text can
/// contain credentials or paths and must never enter the public diagnostic.
#[derive(Clone, Copy, serde::Serialize)]
struct FailureDiagnostic {
    operation: &'static str,
    stage: FailureStage,
    category: &'static str,
    protocol_code: Option<ErrorCode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transport_phase: Option<crate::session::BrowserTransportPhase>,
}
impl FailureDiagnostic {
    fn from_error(operation: &'static str, error: &anyhow::Error) -> Self {
        use pulsar_protocol::TransportError;
        let stage = error
            .downcast_ref::<FailureStage>()
            .copied()
            .unwrap_or(FailureStage::Local);
        let transport = error.downcast_ref::<TransportError>();
        let protocol = error.downcast_ref::<ProtocolError>().or(match transport {
            Some(TransportError::Protocol(error)) => Some(error),
            _ => None,
        });
        let io = error.downcast_ref::<std::io::Error>().or(match transport {
            Some(TransportError::Io(error)) => Some(error),
            _ => None,
        });
        let category = if error.is::<RetiredBeforeDispatch>() {
            "retired_before_dispatch"
        } else if stage == FailureStage::Reply {
            "reply_validation"
        } else if let Some(protocol) = protocol {
            match (stage, protocol.message.as_str()) {
                (FailureStage::Exchange, "browser timing scope or sequence differs") => {
                    "scope_or_sequence"
                }
                (FailureStage::Exchange, "browser timing binding is not current") => {
                    "binding_not_current"
                }
                (FailureStage::Exchange, _) => "protocol_rejected",
                _ => "local_protocol_validation",
            }
        } else if let Some(io) = io {
            if matches!(
                io.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) {
                "transport_timeout"
            } else {
                "transport_io"
            }
        } else if transport.is_some() {
            "transport_framing"
        } else {
            if stage == FailureStage::Exchange {
                "exchange_unclassified"
            } else {
                "local_state"
            }
        };
        Self {
            operation,
            stage,
            category,
            protocol_code: protocol.map(|p| p.code),
            transport_phase: error
                .downcast_ref::<crate::session::BrowserTransportPhase>()
                .copied(),
        }
    }
}
struct Record {
    request_id: RequestId,
    predecessor: Option<ArtifactId>,
    active: bool,
    player: HandyPlayerIdentity,
    media: BrowserMediaBinding,
    cancelled: Arc<AtomicBool>,
    retired: bool,
    opening: bool,
    busy: bool,
    pending_reply: bool,
    report: Option<BrowserTimingReport>,
    probe: Option<BrowserTimingProbe>,
    release: ReleaseState,
    issue: Option<&'static str>,
    first_failure: Option<FailureDiagnostic>,
}
impl Record {
    fn acknowledge_release(&mut self) {
        self.release = ReleaseState::Acknowledged;
        // Permanent retirement settles probe/observation custody even when the
        // exchange failed before cancellation. Keep first_failure as history;
        // it must neither revive the binding nor invent unresolved ownership.
        let failed_exchange = self.issue == Some("timing_exchange_unconfirmed")
            && self
                .first_failure
                .as_ref()
                .is_some_and(|failure| matches!(failure.operation, "probe" | "observe"));
        if self.issue == Some("cancelled_timing_exchange_unconfirmed") || failed_exchange {
            self.issue = None;
        }
    }
}
#[derive(Default)]
struct Records {
    entries: Vec<Record>,
    worker_failed: bool,
    #[cfg(test)]
    before_execute: Option<Arc<TestBeforeExecute>>,
}
#[cfg(test)]
struct TestBeforeExecute {
    entered: mpsc::SyncSender<()>,
    resume: Mutex<mpsc::Receiver<()>>,
}
type Shared = Arc<Mutex<Records>>;
fn state(shared: &Shared) -> std::sync::MutexGuard<'_, Records> {
    shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
fn policy() -> BrowserTimingPolicy {
    BrowserTimingPolicy {
        probe_timeout_ms: 1000,
        observation_timeout_ms: 3000,
    }
}
fn diagnostic(record: &Record) -> Value {
    json!({"open_request_id":record.request_id,
        "binding_id":record.report.as_ref().map(|r| &r.binding.binding_id),
        "player":record.player,"issue":record.issue,"first_failure":record.first_failure})
}
fn unavailable(record: &Record) -> anyhow::Error {
    anyhow::anyhow!(
        "Browser timing unavailable; original operation {}; native expiry remains fallback",
        diagnostic(record)
    )
}
fn checked_report(
    reply: pulsar_protocol::ResponseBody,
    record: &Record,
) -> Result<BrowserTimingReport> {
    let pulsar_protocol::ResponseBody::BrowserTiming(report) = reply else {
        anyhow::bail!("unexpected timing report");
    };
    report.validate()?;
    let binding = &report.binding;
    ensure!(
        binding.request_id == record.request_id
            && binding.player == record.player
            && binding.project_id == record.media.project_id
            && binding.revision == record.media.revision
            && binding.engine_epoch == record.media.engine_epoch
            && binding.browser_media == record.media
            && binding.policy == policy()
            && record
                .predecessor
                .as_ref()
                .is_none_or(|old| &binding.binding_id != old)
            && record
                .report
                .as_ref()
                .is_none_or(|old| old.binding.binding_id == binding.binding_id),
        "timing report subject changed"
    );
    Ok(*report)
}
fn projection(report: &BrowserTimingReport) -> Value {
    let observation = report.observation.as_ref().map(|o| json!({
        "observation_id":o.observation_id,"sequence":o.sequence.to_string(),
        "bracket_width_ns":(o.bracket.native_receive_ns-o.bracket.native_issue_ns).to_string(),
        "project_phase":{"numerator":o.project_phase.numerator().to_string(),"denominator":o.project_phase.denominator().to_string()},"frame":o.frame,
    }));
    json!({"current":report.current,"qualified":false,"binding_id":report.binding.binding_id,
        "open_request_id":report.binding.request_id,"observation":observation})
}

enum Operation {
    Open,
    Probe,
    Observe(Observe),
    Status,
}
impl Operation {
    fn name(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Probe => "probe",
            Self::Observe(_) => "observe",
            Self::Status => "status",
        }
    }
}
struct Job {
    identity: HandyPlayerIdentity,
    record_id: RequestId,
    ticket: Option<u64>,
    operation: Operation,
    deadline: Instant,
}
struct Event {
    identity: HandyPlayerIdentity,
    record_id: RequestId,
    ticket: u64,
    historical: bool,
    result: Result<Value>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Observe {
    sequence: String,
    probe_id: ArtifactId,
    frame: BrowserTimingFrame,
}
impl Observe {
    fn sequence(&self) -> Result<u64> {
        let value: u64 = self.sequence.parse().context("invalid timing sequence")?;
        ensure!(
            value > 0 && value.to_string() == self.sequence,
            "invalid timing sequence"
        );
        Ok(value)
    }
}

pub(super) struct Timing {
    jobs: Option<mpsc::SyncSender<Job>>,
    events: mpsc::Receiver<Event>,
    shared: Shared,
    stopping: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

/// Release ownership is reserved before IPC; a checked acknowledgment suppresses
/// duplicate late-result cleanup, while an unknown result is never called success.
fn release_retired(client: &SessionClient, shared: &Shared) {
    let releases: Vec<_> = {
        let mut records = state(shared);
        records
            .entries
            .iter_mut()
            .filter_map(|record| {
                if !(record.retired || record.cancelled.load(Ordering::Acquire))
                    || record.release != ReleaseState::Needed
                {
                    return None;
                }
                let report = record.report.clone()?;
                record.retired = true;
                record.probe = None;
                if !report.current {
                    record.acknowledge_release();
                    return None;
                }
                record.release = ReleaseState::Running;
                Some((record.request_id.clone(), report))
            })
            .collect()
    };
    for (original, report) in releases {
        let reply = client.browser_call_until(
            new_request_id(),
            Command::ReleaseBrowserTiming {
                binding_id: report.binding.binding_id.clone(),
                player: report.binding.player.clone(),
            },
            report.binding.project_id.clone(),
            None,
            Instant::now() + CONTROL_TIMEOUT,
        );
        let mut records = state(shared);
        let Some(record) = records
            .entries
            .iter_mut()
            .find(|r| r.request_id == original)
        else {
            continue;
        };
        let checked = reply.context(FailureStage::Exchange).and_then(|reply| {
            checked_report(reply, record)
                .and_then(|report| {
                    ensure!(!report.current, "timing release was not acknowledged");
                    Ok(report)
                })
                .context(FailureStage::Reply)
        });
        match checked {
            Ok(report) => {
                record.report = Some(report);
                record.acknowledge_release();
            }
            Err(error) => {
                record
                    .first_failure
                    .get_or_insert_with(|| FailureDiagnostic::from_error("release", &error));
                record.release = ReleaseState::Unknown;
                record.issue = Some("release_unconfirmed");
            }
        }
    }
}

fn execute(client: &SessionClient, shared: &Shared, job: &Job) -> Result<Value> {
    let (request_id, project, revision, command) = {
        let records = state(shared);
        let record = records
            .entries
            .iter()
            .find(|r| r.request_id == job.record_id && r.player == job.identity)
            .context("timing scope absent")?;
        if !matches!(job.operation, Operation::Status)
            && (record.retired || record.cancelled.load(Ordering::Acquire))
        {
            return Err(RetiredBeforeDispatch.into());
        }
        let command = match &job.operation {
            Operation::Open => match &record.predecessor {
                Some(predecessor_binding_id) => Command::PrepareBrowserTimingSuccessor {
                    predecessor_binding_id: predecessor_binding_id.clone(),
                    player: record.player.clone(),
                    browser_media: record.media.clone(),
                    policy: policy(),
                },
                None => Command::OpenBrowserTiming {
                    player: record.player.clone(),
                    browser_media: record.media.clone(),
                    policy: policy(),
                },
            },
            Operation::Probe => {
                let report = record.report.as_ref().context("timing binding absent")?;
                ensure!(
                    report.current && record.probe.is_none(),
                    "timing probe already pending or binding retired"
                );
                let sequence = report
                    .observation
                    .as_ref()
                    .map_or(0, |o| o.sequence)
                    .checked_add(1)
                    .context("timing sequence overflow")?;
                Command::IssueBrowserTimingProbe {
                    binding_id: report.binding.binding_id.clone(),
                    player: record.player.clone(),
                    sequence,
                }
            }
            Operation::Observe(observation) => {
                let probe = record.probe.as_ref().context("timing probe absent")?;
                ensure!(
                    observation.sequence()? == probe.sequence
                        && observation.probe_id == probe.probe_id,
                    "timing nonce differs"
                );
                observation.frame.validate()?;
                Command::ObserveBrowserTiming {
                    binding_id: probe.binding_id.clone(),
                    player: record.player.clone(),
                    sequence: probe.sequence,
                    probe_id: probe.probe_id.clone(),
                    frame: observation.frame.clone(),
                }
            }
            Operation::Status if record.retired || record.cancelled.load(Ordering::Acquire) => {
                Command::BrowserTimingOpenStatus {
                    original_request_id: record.request_id.clone(),
                }
            }
            Operation::Status => match &record.report {
                Some(report) => Command::BrowserTimingStatus {
                    binding_id: report.binding.binding_id.clone(),
                    player: record.player.clone(),
                },
                None => Command::BrowserTimingOpenStatus {
                    original_request_id: record.request_id.clone(),
                },
            },
        };
        (
            if matches!(job.operation, Operation::Open) {
                record.request_id.clone()
            } else {
                new_request_id()
            },
            record.media.project_id.clone(),
            matches!(job.operation, Operation::Open).then_some(record.media.revision),
            command,
        )
    };
    let reply = client
        .browser_call_until(request_id, command, project, revision, job.deadline)
        .context(FailureStage::Exchange)?;
    accept_reply(shared, job, reply).context(FailureStage::Reply)
}
fn accept_reply(shared: &Shared, job: &Job, reply: pulsar_protocol::ResponseBody) -> Result<Value> {
    let mut records = state(shared);
    let record = records
        .entries
        .iter_mut()
        .find(|r| r.request_id == job.record_id && r.player == job.identity)
        .context("timing scope absent")?;
    if matches!(job.operation, Operation::Probe | Operation::Observe(_)) {
        ensure!(
            !record.retired && !record.cancelled.load(Ordering::Acquire),
            "timing scope retired during exchange"
        );
    }
    if matches!(job.operation, Operation::Probe) {
        let pulsar_protocol::ResponseBody::BrowserTimingProbe(probe) = reply else {
            anyhow::bail!("unexpected timing probe reply");
        };
        probe.validate()?;
        let report = record.report.as_ref().context("timing binding absent")?;
        let expected = report
            .observation
            .as_ref()
            .map_or(0, |o| o.sequence)
            .checked_add(1)
            .context("timing sequence overflow")?;
        ensure!(
            probe.binding_id == report.binding.binding_id
                && probe.player == record.player
                && probe.sequence == expected
                && probe
                    .expires_at_boottime_ns
                    .checked_sub(probe.issued_at_boottime_ns)
                    .is_some_and(|span| span <= u64::from(policy().probe_timeout_ms) * 1_000_000),
            "timing probe subject changed"
        );
        let value = json!({"sequence":probe.sequence.to_string(),"probe_id":probe.probe_id,"probe_timeout_ms":policy().probe_timeout_ms,"observation_timeout_ms":policy().observation_timeout_ms});
        record.probe = Some(*probe);
        Ok(value)
    } else {
        let report = checked_report(reply, record)?;
        if matches!(job.operation, Operation::Status) && !report.current && record.retired {
            record.acknowledge_release();
            if matches!(
                record.issue,
                Some("open_unconfirmed" | "release_unconfirmed")
            ) {
                record.issue = None;
            }
        }
        if let Operation::Observe(observation) = &job.operation {
            let accepted = report
                .observation
                .as_ref()
                .context("timing observation absent")?;
            let probe = record.probe.as_ref().context("timing probe absent")?;
            ensure!(
                accepted.sequence == observation.sequence()?
                    && accepted.frame == observation.frame
                    && accepted.bracket.native_issue_ns == probe.issued_at_boottime_ns
                    && accepted.bracket.native_receive_ns < probe.expires_at_boottime_ns,
                "timing observation reply differs"
            );
            record.probe = None;
        }
        let value = projection(&report);
        record.report = Some(report);
        Ok(value)
    }
}

impl Timing {
    pub(super) fn current_binding(
        &self,
        player: &HandyPlayerIdentity,
    ) -> Result<BrowserTimingBinding> {
        self.selected_binding(player, false)
    }
    pub(super) fn successor_binding(
        &self,
        player: &HandyPlayerIdentity,
    ) -> Result<BrowserTimingBinding> {
        self.selected_binding(player, true)
    }
    fn selected_binding(
        &self,
        player: &HandyPlayerIdentity,
        next: bool,
    ) -> Result<BrowserTimingBinding> {
        let records = state(&self.shared);
        let record = records
            .entries
            .iter()
            .find(|r| &r.player == player && r.active != next && (!next || !r.retired))
            .context("native browser timing absent")?;
        ensure!(
            !record.retired && !record.cancelled.load(Ordering::Acquire) && record.issue.is_none(),
            "native browser timing unavailable"
        );
        let report = record
            .report
            .as_ref()
            .context("native browser timing still opening")?;
        report.validate()?;
        ensure!(
            report.current && report.observation.is_some(),
            "wait for a current browser observation"
        );
        Ok(report.binding.clone())
    }
    pub(super) fn open_successor(
        &self,
        player: &HandyPlayerIdentity,
        media: BrowserMediaBinding,
    ) -> Result<()> {
        media.validate()?;
        let predecessor = self.current_binding(player)?;
        ensure!(
            predecessor.browser_media.selection == media.selection
                && predecessor.browser_media.clock == media.clock
                && predecessor.browser_media.project_id == media.project_id
                && predecessor.browser_media.revision == media.revision
                && predecessor.browser_media.engine_epoch == media.engine_epoch
                && predecessor.browser_media != media,
            "timing successor source differs"
        );
        let request_id = new_request_id();
        {
            let mut records = state(&self.shared);
            records.entries.retain(|record| {
                !(record.retired
                    && record.release == ReleaseState::Acknowledged
                    && record.issue.is_none()
                    && !record.opening
                    && !record.busy
                    && !record.pending_reply
                    && record.probe.is_none())
            });
            ensure!(
                !records.worker_failed && records.entries.len() < RECORD_LIMIT,
                "timing successor admission full"
            );
            ensure!(
                !records
                    .entries
                    .iter()
                    .any(|r| &r.player == player && !r.active && !r.retired),
                "timing successor already retained"
            );
            let cancelled = records
                .entries
                .iter()
                .find(|r| &r.player == player && r.active)
                .context("timing predecessor absent")?
                .cancelled
                .clone();
            records.entries.push(Record {
                request_id: request_id.clone(),
                predecessor: Some(predecessor.binding_id),
                active: false,
                player: player.clone(),
                media,
                cancelled,
                retired: false,
                opening: true,
                busy: true,
                pending_reply: false,
                report: None,
                probe: None,
                release: ReleaseState::Needed,
                issue: None,
                first_failure: None,
            });
        }
        self.submit(Job {
            identity: player.clone(),
            record_id: request_id,
            ticket: None,
            operation: Operation::Open,
            deadline: Instant::now() + OPEN_TIMEOUT,
        })
    }
    pub(super) fn check_successor(
        &self,
        old: &BrowserTimingBinding,
        new: &BrowserTimingBinding,
    ) -> Result<()> {
        ensure!(
            self.current_binding(&old.player)? == *old
                && self.successor_binding(&old.player)? == *new,
            "timing transfer differs from retained generations"
        );
        let records = state(&self.shared);
        let next = records
            .entries
            .iter()
            .find(|r| r.request_id == new.request_id)
            .context("timing successor absent")?;
        ensure!(
            next.predecessor.as_ref() == Some(&old.binding_id),
            "timing successor link differs"
        );
        Ok(())
    }
    /// Called only after the native capture transfer has actually committed.
    pub(super) fn commit_successor(
        &self,
        old: &BrowserTimingBinding,
        new: &BrowserTimingBinding,
    ) -> Result<()> {
        self.check_successor(old, new)?;
        let mut records = state(&self.shared);
        for record in records
            .entries
            .iter_mut()
            .filter(|r| r.player == old.player)
        {
            if record.request_id == old.request_id {
                record.active = false;
                record.retired = true;
            }
            if record.request_id == new.request_id {
                record.active = true;
            }
        }
        Ok(())
    }
    pub(super) fn new(client: &SessionClient) -> Result<Self> {
        let (jobs, receiver) = mpsc::sync_channel::<Job>(QUEUE_LIMIT);
        let (events, completed) = mpsc::sync_channel::<Event>(RECORD_LIMIT);
        let shared = Arc::new(Mutex::new(Records::default()));
        let records = shared.clone();
        let stopping = Arc::new(AtomicBool::new(false));
        let stopped = stopping.clone();
        let client = client.fork_native();
        let worker = thread::Builder::new()
            .name("pulsar-browser-timing".into())
            .spawn(move || {
                loop {
                    release_retired(&client, &records);
                    let job = match receiver.recv_timeout(Duration::from_millis(5)) {
                        Ok(job) => job,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let skip = stopped.load(Ordering::Acquire)
                        || state(&records)
                            .entries
                            .iter()
                            .find(|r| r.request_id == job.record_id && r.player == job.identity)
                            .is_none_or(|r| {
                                (r.cancelled.load(Ordering::Acquire) || r.retired)
                                    && !matches!(job.operation, Operation::Status)
                            });
                    if skip {
                        if let Some(record) = state(&records)
                            .entries
                            .iter_mut()
                            .find(|r| r.request_id == job.record_id && r.player == job.identity)
                        {
                            record.opening = false;
                            record.busy = false;
                            record.retired = true;
                            if matches!(job.operation, Operation::Open) && record.report.is_none() {
                                record.release = ReleaseState::Acknowledged;
                            }
                        }
                        continue;
                    }
                    #[cfg(test)]
                    let hook = {
                        let mut records = state(&records);
                        records.before_execute.take()
                    };
                    #[cfg(test)]
                    if let Some(hook) = hook {
                        hook.entered.send(()).expect("test dispatch signal");
                        hook.resume
                            .lock()
                            .unwrap()
                            .recv_timeout(Duration::from_secs(2))
                            .expect("bounded test dispatch release");
                    }
                    let result = execute(&client, &records, &job);
                    let mut result = {
                        let mut records = state(&records);
                        let record = records
                            .entries
                            .iter_mut()
                            .find(|r| r.request_id == job.record_id && r.player == job.identity)
                            .expect("retained timing record");
                        match result {
                            Ok(value) => Ok(value),
                            Err(error) => {
                                record.first_failure.get_or_insert_with(|| {
                                    FailureDiagnostic::from_error(job.operation.name(), &error)
                                });
                                let cancelled_exchange = matches!(
                                    job.operation,
                                    Operation::Probe | Operation::Observe(_)
                                ) && (record.retired
                                    || record.cancelled.load(Ordering::Acquire));
                                let unsent_open = matches!(job.operation, Operation::Open)
                                    && error.is::<RetiredBeforeDispatch>()
                                    && record.report.is_none();
                                record.retired = true;
                                if unsent_open {
                                    // Same no-IPC outcome as the earlier skip check.
                                    // Never infer this from a transport/native error.
                                    record.acknowledge_release();
                                    record.issue = None;
                                } else {
                                    record.issue =
                                        Some(if matches!(job.operation, Operation::Open) {
                                            "open_unconfirmed"
                                        } else if cancelled_exchange {
                                            "cancelled_timing_exchange_unconfirmed"
                                        } else {
                                            "timing_exchange_unconfirmed"
                                        });
                                }
                                Err(unavailable(record))
                            }
                        }
                    };
                    release_retired(&client, &records);
                    if matches!(job.operation, Operation::Status) && result.is_ok() {
                        let mut records = state(&records);
                        let record = records
                            .entries
                            .iter_mut()
                            .find(|r| r.request_id == job.record_id && r.player == job.identity)
                            .expect("retained timing record");
                        if record.retired {
                            result = if record.release == ReleaseState::Acknowledged {
                                if record.issue == Some("open_unconfirmed") {
                                    record.issue = None;
                                }
                                record
                                    .report
                                    .as_ref()
                                    .map(projection)
                                    .context("original timing history unavailable")
                            } else {
                                Err(unavailable(record))
                            };
                        }
                    }
                    if let Some(ticket) = job.ticket {
                        if let Some(record) = state(&records)
                            .entries
                            .iter_mut()
                            .find(|r| r.request_id == job.record_id && r.player == job.identity)
                        {
                            record.pending_reply = true;
                            record.busy = false;
                            record.opening = false;
                        }
                        if events
                            .try_send(Event {
                                identity: job.identity.clone(),
                                record_id: job.record_id.clone(),
                                ticket,
                                historical: matches!(job.operation, Operation::Status),
                                result,
                            })
                            .is_err()
                        {
                            if let Some(record) = state(&records)
                                .entries
                                .iter_mut()
                                .find(|r| r.request_id == job.record_id && r.player == job.identity)
                            {
                                record.retired = true;
                                record.pending_reply = false;
                                record.issue = Some("timing_result_delivery_failed");
                                record.first_failure.get_or_insert(FailureDiagnostic {
                                    operation: job.operation.name(),
                                    stage: FailureStage::Delivery,
                                    category: "result_delivery",
                                    protocol_code: None,
                                    transport_phase: None,
                                });
                            }
                        }
                    } else {
                        // Keep custody until all response/release post-processing has finished.
                        if let Some(record) = state(&records)
                            .entries
                            .iter_mut()
                            .find(|r| r.request_id == job.record_id && r.player == job.identity)
                        {
                            record.busy = false;
                            record.opening = false;
                        }
                    }
                }
                release_retired(&client, &records);
            })?;
        Ok(Self {
            jobs: Some(jobs),
            events: completed,
            shared,
            stopping,
            worker: Some(worker),
        })
    }
    pub(super) fn open(
        &self,
        player: HandyPlayerIdentity,
        cancelled: Arc<AtomicBool>,
        media: BrowserMediaBinding,
    ) -> Result<()> {
        media.validate()?;
        let request_id = new_request_id();
        {
            let mut records = state(&self.shared);
            records.entries.retain(|record| {
                !(record.retired
                    && record.release == ReleaseState::Acknowledged
                    && record.issue.is_none()
                    && !record.opening
                    && !record.busy
                    && !record.pending_reply
                    && record.probe.is_none())
            });
            ensure!(
                !records.worker_failed && records.entries.len() < RECORD_LIMIT,
                "browser timing admission full or unavailable"
            );
            ensure!(
                !records.entries.iter().any(|r| r.player == player),
                "original timing operation already exists"
            );
            records.entries.push(Record {
                request_id: request_id.clone(),
                predecessor: None,
                active: true,
                player: player.clone(),
                media,
                cancelled,
                retired: false,
                opening: true,
                busy: true,
                pending_reply: false,
                report: None,
                probe: None,
                release: ReleaseState::Needed,
                issue: None,
                first_failure: None,
            });
        }
        self.submit(Job {
            identity: player,
            record_id: request_id,
            ticket: None,
            operation: Operation::Open,
            deadline: Instant::now() + OPEN_TIMEOUT,
        })
    }
    fn submit(&self, job: Job) -> Result<()> {
        let operation = job.operation.name();
        let identity = job.identity.clone();
        let record_id = job.record_id.clone();
        if self
            .jobs
            .as_ref()
            .is_some_and(|tx| tx.try_send(job).is_ok())
        {
            return Ok(());
        }
        let mut records = state(&self.shared);
        let record = records
            .entries
            .iter_mut()
            .find(|r| r.request_id == record_id && r.player == identity)
            .context("timing scope absent")?;
        record.busy = false;
        record.opening = false;
        record.retired = true;
        record.issue = Some("timing_queue_unavailable");
        record.first_failure.get_or_insert(FailureDiagnostic {
            operation,
            stage: FailureStage::Queue,
            category: "queue_unavailable",
            protocol_code: None,
            transport_phase: None,
        });
        Err(unavailable(record))
    }
    pub(super) fn handle(
        &self,
        path: &str,
        player: &HandyPlayerIdentity,
        body: &[u8],
        ticket: u64,
    ) -> Result<ControlReply> {
        let next = path.starts_with("/player/timing-next-");
        let operation = match path {
            "/player/timing-probe" | "/player/timing-next-probe" => {
                ensure!(body.is_empty(), "timing probe body refused");
                Operation::Probe
            }
            "/player/timing-observe" | "/player/timing-next-observe" => {
                Operation::Observe(serde_json::from_slice(body)?)
            }
            "/player/timing-status" | "/player/timing-next-status" => {
                ensure!(body.is_empty(), "timing status body refused");
                Operation::Status
            }
            _ => anyhow::bail!("unknown timing operation"),
        };
        let record_id = {
            let mut records = state(&self.shared);
            let record = records
                .entries
                .iter_mut()
                .find(|r| &r.player == player && r.active != next && (!next || !r.retired))
                .context("timing requires a prepared Engine source")?;
            if record.opening {
                return Ok(ControlReply::Ready(
                    json!({"pending":true,"open_request_id":record.request_id}),
                ));
            }
            ensure!(
                !record.busy && !record.pending_reply,
                "timing exchange already pending"
            );
            if !matches!(operation, Operation::Status) && (record.retired || record.issue.is_some())
            {
                return Err(unavailable(record));
            }
            record.busy = true;
            record.request_id.clone()
        };
        let timeout = if matches!(operation, Operation::Status) {
            OPEN_TIMEOUT
        } else {
            CONTROL_TIMEOUT
        };
        self.submit(Job {
            identity: player.clone(),
            record_id,
            ticket: Some(ticket),
            operation,
            deadline: Instant::now() + timeout,
        })?;
        Ok(ControlReply::Pending)
    }
    pub(super) fn poll(
        &self,
        retained: Option<&HandyPlayerIdentity>,
        active: bool,
    ) -> Vec<(u64, Result<Value>)> {
        let mut replies = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            if let Some(record) = state(&self.shared)
                .entries
                .iter_mut()
                .find(|r| r.request_id == event.record_id && r.player == event.identity)
            {
                record.pending_reply = false;
            }
            let result = if retained == Some(&event.identity) && (active || event.historical) {
                event.result
            } else {
                Err(anyhow::anyhow!("timing player retired during operation"))
            };
            replies.push((event.ticket, result));
        }
        replies
    }
    pub(super) fn retire(&self, player: &HandyPlayerIdentity) {
        for record in state(&self.shared)
            .entries
            .iter_mut()
            .filter(|r| &r.player == player)
        {
            record.retired = true;
            record.probe = None;
        }
    }
    pub(super) fn cancel_successor(&self, player: &HandyPlayerIdentity) {
        for record in state(&self.shared)
            .entries
            .iter_mut()
            .filter(|r| &r.player == player && !r.active)
        {
            record.retired = true;
            record.probe = None;
        }
    }
    pub(super) fn shutdown(&mut self) -> Result<()> {
        self.stopping.store(true, Ordering::Release);
        for record in &mut state(&self.shared).entries {
            record.retired = true;
        }
        self.jobs.take();
        if self
            .worker
            .take()
            .is_some_and(|worker| worker.join().is_err())
        {
            state(&self.shared).worker_failed = true;
        }
        let records = state(&self.shared);
        let unresolved: Vec<_> = records
            .entries
            .iter()
            .filter(|r| r.issue.is_some() || r.report.as_ref().is_some_and(|r| r.current))
            .map(diagnostic)
            .collect();
        ensure!(!records.worker_failed && unresolved.is_empty(),
            "Browser timing cleanup unconfirmed; original operations {}; worker_failed={}; native expiry remains fallback", json!(unresolved), records.worker_failed);
        Ok(())
    }
}
impl Drop for Timing {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(all(test, unix))]
#[path = "browser_player_timing_tests.rs"]
mod tests;
