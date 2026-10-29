//! Canonical inert fallback payloads. Original decisions and evidence remain
//! historical data; no current acceptance, native handle or grant is imported.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

const MAGIC: &[u8; 8] = b"PULSFH1\0";
const HEADER_LIMIT: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Header {
    pub(super) schema_version: u16,
    pub(super) history: InferenceFallbackHistory,
    pub(super) job_state: String,
    pub(super) pending: Option<PendingObservation>,
    /// Canonical order by dossier, revision and report digest; used records only.
    pub(super) reports: Vec<InferenceEquivalenceReport>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PendingObservation {
    pub(super) predecessor: AttemptId,
    pub(super) engine_epoch: AttemptId,
    pub(super) outcome_sha256: String,
}

#[derive(Debug)]
pub(super) struct Payload {
    pub(super) header: Header,
    pub(super) documents: BTreeMap<String, Vec<u8>>,
}

fn invalid(message: &str) -> ProtocolError {
    ProtocolError::invalid(message)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn key(reference: &InferenceFallbackReference) -> (String, u64, String) {
    (
        reference.dossier_id.to_string(),
        reference.expected_revision,
        reference.expected_report_sha256.clone(),
    )
}
fn report_key(report: &InferenceEquivalenceReport) -> PResult<(String, u64, String)> {
    Ok((
        report.dossier_id.to_string(),
        report.revision,
        digest(encode(report)?.as_bytes()),
    ))
}

impl Header {
    fn validate(&self) -> PResult<BTreeMap<String, u64>> {
        self.history.validate()?;
        if !matches!(
            self.job_state.as_str(),
            "queued"
                | "running"
                | "completed"
                | "failed"
                | "cancelled"
                | "interrupted"
                | "assistant_paused"
                | "fallback_pending"
        ) {
            return Err(invalid("fallback original job state is unknown"));
        }
        match (&self.pending, self.job_state.as_str()) {
            (Some(pending), "fallback_pending") => {
                validate_sha256(&pending.outcome_sha256)?;
                let current = self
                    .history
                    .attempts
                    .last()
                    .expect("validated nonempty history");
                let outcome = current
                    .outcome
                    .as_ref()
                    .ok_or_else(|| invalid("pending fallback history lacks original outcome"))?;
                if current.state != "failed"
                    || pending.predecessor != current.attempt_id
                    || pending.engine_epoch != outcome.engine_epoch
                    || pending.outcome_sha256 != digest(encode(outcome)?.as_bytes())
                    || !outcome
                        .worker_retirement
                        .as_ref()
                        .is_some_and(|r| r.provider_unavailable.is_some())
                {
                    return Err(invalid(
                        "pending fallback observation differs from original settled predecessor",
                    ));
                }
            }
            (None, state) if state != "fallback_pending" => {}
            _ => {
                return Err(invalid(
                    "pending fallback observation or job state is missing",
                ))
            }
        }
        if self.schema_version != 1
            || self.reports.is_empty()
            || self.reports.len() > MAX_INFERENCE_FALLBACK_REFERENCES
        {
            return Err(invalid("fallback payload schema or report count differs"));
        }
        let used: BTreeSet<_> = self
            .history
            .attempts
            .iter()
            .map(|a| key(&a.trace.reference))
            .collect();
        let mut seen = BTreeSet::new();
        let mut previous = None;
        let mut documents = BTreeMap::new();
        for report in &self.reports {
            report.validate()?;
            let identity = report_key(report)?;
            if report.state != InferenceEquivalenceState::Accepted
                || report.manifest.subject.origin != InferenceEquivalenceOrigin::Measured
                || previous.as_ref().is_some_and(|old| old >= &identity)
                || !used.contains(&identity)
            {
                return Err(invalid(
                    "fallback payload report identity, order or historical acceptance differs",
                ));
            }
            previous = Some(identity.clone());
            seen.insert(identity);
            for item in &report.manifest.evidence {
                if documents
                    .insert(item.identity.sha256.clone(), item.identity.byte_len)
                    .is_some_and(|n| n != item.identity.byte_len)
                {
                    return Err(invalid("fallback payload evidence aliases disagree"));
                }
            }
        }
        if seen != used {
            return Err(invalid("fallback payload lacks an original decision"));
        }
        let total = documents
            .values()
            .try_fold(0u64, |n, len| n.checked_add(*len))
            .ok_or_else(|| invalid("fallback payload evidence byte count overflow"))?;
        // The original registry has this aggregate document bound; retaining it
        // also bounds allocation before any raw evidence body is copied.
        if total > 128 * 1024 * 1024 {
            return Err(invalid(
                "fallback payload evidence exceeds aggregate admission",
            ));
        }
        if encode(self)?.len() > HEADER_LIMIT {
            return Err(invalid("fallback payload header exceeds admission"));
        }
        Ok(documents)
    }
}

impl Payload {
    pub(super) fn validate(&self) -> PResult<()> {
        let expected = self.header.validate()?;
        if expected.len() != self.documents.len() {
            return Err(invalid(
                "fallback payload contains detached or missing evidence",
            ));
        }
        for (hash, len) in &expected {
            let data = self
                .documents
                .get(hash)
                .ok_or_else(|| invalid("fallback payload evidence absent"))?;
            if data.len() as u64 != *len || digest(data) != *hash {
                return Err(invalid("fallback payload evidence bytes differ"));
            }
        }
        for report in &self.header.reports {
            let docs: BTreeMap<_, _> = report
                .manifest
                .evidence
                .iter()
                .map(|item| (item.kind, self.documents[&item.identity.sha256].clone()))
                .collect();
            let assessment = pulsar_protocol::validate_evidence(&report.manifest, &docs)?;
            let decision = report
                .decision
                .as_ref()
                .ok_or_else(|| invalid("fallback original decision absent"))?;
            let at = decision
                .recorded_at_unix_ms
                .ok_or_else(|| invalid("fallback original decision clock absent"))?;
            assessment.require_acceptance(at)?;
            if decision.review_identity.as_ref() != Some(assessment.review_identity()) {
                return Err(invalid("fallback original review identity differs"));
            }
            let identity = report_key(report)?;
            for attempt in self
                .header
                .history
                .attempts
                .iter()
                .filter(|a| key(&a.trace.reference) == identity)
            {
                assessment.require_acceptance(attempt.trace.admitted_at_unix_ms)?;
                if attempt.trace.admitted_at_unix_ms < at
                    || attempt.trace.expires_at_unix_ms != report.manifest.subject.expires_unix_ms
                {
                    return Err(invalid(
                        "fallback original admission and decision clocks differ",
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn validate_with_manifests(
        &self,
        mut resolve: impl FnMut(&AttemptId) -> PResult<WorkerRequest>,
    ) -> PResult<()> {
        self.validate()?;
        self.header.history.validate_with_manifests(&mut resolve)?;
        let reports: BTreeMap<_, _> = self
            .header
            .reports
            .iter()
            .map(|report| Ok((report_key(report)?, report)))
            .collect::<PResult<_>>()?;
        let mut previous: Option<WorkerRequest> = None;
        for (i, entry) in self.header.history.attempts.iter().enumerate() {
            let request = resolve(&entry.attempt_id)?;
            let canonical = encode(&request)?;
            if canonical.len() > 1024 * 1024
                || digest(canonical.as_bytes()) != entry.manifest_sha256
            {
                return Err(invalid(
                    "fallback worker changed between original correspondence reads",
                ));
            }
            let identity = key(&entry.trace.reference);
            let report = reports
                .get(&identity)
                .ok_or_else(|| invalid("fallback original report absent"))?;
            let subject = &report.manifest.subject;
            super::inference_fallback_execution::scope_matches(
                &self.header.history.selection,
                subject,
                &request,
            )?;
            let (profile, environment) = if entry.trace.ordinal == 0 {
                (&subject.from_profile, &subject.from_environment)
            } else {
                (&subject.to_profile, &subject.to_environment)
            };
            if encode(super::inference_fallback_execution::profile(&request)?)? != encode(profile)?
                || entry.observed_environment.identity_sha256()? != environment.identity_sha256()?
            {
                return Err(invalid(
                    "fallback original profile or environment differs from referenced decision",
                ));
            }
            if entry.trace.kind == JobAttemptFallbackKind::Fallback {
                let old = previous
                    .as_ref()
                    .ok_or_else(|| invalid("fallback predecessor manifest absent"))?;
                if encode(super::inference_fallback_execution::profile(old)?)?
                    != encode(&subject.from_profile)?
                {
                    return Err(invalid(
                        "fallback original decision does not cover its predecessor",
                    ));
                }
            } else if entry.trace.kind == JobAttemptFallbackKind::Resume
                && key(&self.header.history.attempts[i - 1].trace.reference) != identity
            {
                return Err(invalid("fallback resume replaced its original decision"));
            }
            previous = Some(request);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn encode(&self) -> PResult<Vec<u8>> {
        self.encode_limited(MAX_PACKAGE_FALLBACK_HISTORY_BYTES)
    }
    fn encode_limited(&self, admission: usize) -> PResult<Vec<u8>> {
        self.validate()?;
        let header = encode(&self.header)?.into_bytes();
        let size = self
            .documents
            .values()
            .try_fold(12usize + header.len(), |n, data| n.checked_add(data.len()))
            .ok_or_else(|| invalid("fallback payload size overflow"))?;
        if size > MAX_PACKAGE_FALLBACK_HISTORY_BYTES {
            return Err(invalid("fallback payload exceeds object admission"));
        }
        if size > admission {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "fallback payload exceeds remaining capture admission",
            ));
        }
        let mut raw = Vec::with_capacity(size);
        raw.extend_from_slice(MAGIC);
        raw.extend_from_slice(&(header.len() as u32).to_le_bytes());
        raw.extend_from_slice(&header);
        for data in self.documents.values() {
            raw.extend_from_slice(data);
        }
        Ok(raw)
    }

    pub(super) fn decode(raw: &[u8]) -> PResult<Self> {
        if raw.len() < 12 || raw.len() > MAX_PACKAGE_FALLBACK_HISTORY_BYTES || &raw[..8] != MAGIC {
            return Err(invalid("fallback payload magic or byte admission differs"));
        }
        let len = u32::from_le_bytes(raw[8..12].try_into().expect("fixed frame slice")) as usize;
        let end = 12usize
            .checked_add(len)
            .filter(|end| *end <= raw.len() && len <= HEADER_LIMIT)
            .ok_or_else(|| invalid("fallback payload header length differs"))?;
        let header: Header = serde_json::from_slice(&raw[12..end])
            .map_err(|_| invalid("invalid fallback payload header"))?;
        let expected = header.validate()?;
        if encode(&header)?.as_bytes() != &raw[12..end] {
            return Err(invalid("fallback payload header is not canonical"));
        }
        let mut cursor = end;
        let mut documents = BTreeMap::new();
        for (hash, len) in expected {
            let next = cursor
                .checked_add(len as usize)
                .filter(|n| *n <= raw.len())
                .ok_or_else(|| invalid("fallback payload evidence truncated"))?;
            documents.insert(hash, raw[cursor..next].to_vec());
            cursor = next;
        }
        if cursor != raw.len() {
            return Err(invalid("fallback payload has trailing bytes"));
        }
        let value = Self { header, documents };
        value.validate()?;
        Ok(value)
    }
}

/// Snapshot-owned capture. Filesystem locators and current acceptance APIs are
/// deliberately absent. The caller retains its metadata reservation throughout.
pub(super) fn capture(
    tx: &Transaction<'_>,
    project: &ProjectId,
    job: &JobId,
    remaining: usize,
) -> PResult<Option<Vec<u8>>> {
    let Some(history) =
        super::inference_fallback_store::history_snapshot(tx, project, job)?.into_archive()
    else {
        return Ok(None);
    };
    let admission = remaining;
    let mut remaining = remaining
        .checked_sub(history.canonical_bytes()?.len())
        .ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "fallback history exceeds capture admission",
            )
        })?;
    let references: BTreeMap<_, _> = history
        .attempts
        .iter()
        .map(|a| (key(&a.trace.reference), a.trace.reference.clone()))
        .collect();
    let mut reports = Vec::new();
    let mut documents = BTreeMap::new();
    for reference in references.values() {
        let (report, docs) =
            super::inference_equivalence_store::archival_decision(tx, reference, &mut remaining)?;
        reports.push(report);
        documents.extend(docs);
    }
    let payload = Payload {
        header: Header {
            schema_version: 1,
            history,
            job_state:tx.query_row("SELECT state FROM main.jobs WHERE id=?1 AND project=?2 AND typeof(state)='text' AND length(CAST(state AS BLOB)) BETWEEN 1 AND 64",params![job.as_str(),project.as_str()],|r|r.get(0)).map_err(internal)?,
            pending:super::inference_fallback_store::pending(tx,job)?.map(|p|PendingObservation{predecessor:p.predecessor,engine_epoch:p.engine_epoch,outcome_sha256:p.outcome_sha256}),
            reports,
        },
        documents,
    };
    payload.validate_with_manifests(|id| {
        let raw: String = tx.query_row("SELECT manifest FROM main.attempts WHERE id=?1 AND job=?2 AND typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND 1048576",params![id.as_str(),job.as_str()],|r|r.get(0)).map_err(internal)?;
        let request:WorkerRequest=decode(&raw)?;
        if encode(&request)? != raw { return Err(invalid("fallback snapshot worker bytes are not canonical")); } Ok(request)
    })?;
    payload.encode_limited(admission).map(Some)
}

/// Full original-node correspondence. Root/ancestor composition and storage
/// verify the complete container separately; every historical locator is inert.
pub(super) fn validate_node(
    manifest: &PortableProjectManifest,
    mut bytes: impl FnMut(&PackageObjectRef) -> PResult<Vec<u8>>,
) -> PResult<BTreeSet<(String, u64)>> {
    Ok(check_node(manifest, &mut bytes)?.workers)
}

struct CheckedNode {
    workers: BTreeSet<(String, u64)>,
    tags: BTreeMap<JobId, ArtifactIdentity>,
    payloads: BTreeSet<(String, u64)>,
}
fn check_node(
    manifest: &PortableProjectManifest,
    bytes: &mut impl FnMut(&PackageObjectRef) -> PResult<Vec<u8>>,
) -> PResult<CheckedNode> {
    let mut groups: BTreeMap<&JobId, Vec<&PackageGeneratedOrigin>> = BTreeMap::new();
    for origin in &manifest.generated_origins {
        groups.entry(&origin.job_id).or_default().push(origin);
    }
    let mut verified = BTreeSet::new();
    let mut tags = BTreeMap::new();
    let mut payloads = BTreeSet::new();
    for (job, origins) in groups {
        let references: BTreeSet<_> = origins
            .iter()
            .filter_map(|o| {
                o.fallback_history
                    .as_ref()
                    .map(|r| (r.sha256.clone(), r.byte_len))
            })
            .collect();
        if references.is_empty() {
            continue;
        }
        if references.len() != 1 || origins.iter().any(|o| o.fallback_history.is_none()) {
            return Err(invalid(
                "fallback origins have stripped or contradictory job histories",
            ));
        }
        let (hash, len) = references.first().expect("nonempty reference set");
        let reference = PackageObjectRef {
            sha256: hash.clone(),
            byte_len: *len,
        };
        if !manifest.objects.iter().any(|o| {
            o.sha256 == *hash
                && o.byte_len == *len
                && o.roles
                    .contains(&PackageObjectRole::InferenceFallbackHistory)
        }) {
            return Err(invalid("fallback payload object role or identity absent"));
        }
        let raw = bytes(&reference)?;
        if raw.len() as u64 != *len || digest(&raw) != *hash {
            return Err(invalid("fallback payload object bytes differ"));
        }
        let payload = Payload::decode(&raw)?;
        let history = &payload.header.history;
        tags.insert(job.clone(), history.selection.identity()?);
        payloads.insert((reference.sha256.clone(), reference.byte_len));
        if history.project_id != manifest.origin_project_id
            || history.job_id != *job
            || history.attempts.len() != origins.len()
        {
            return Err(invalid(
                "fallback payload original namespace or attempt membership differs",
            ));
        }
        let by_attempt: BTreeMap<_, _> = origins.iter().map(|o| (&o.attempt_id, *o)).collect();
        payload.validate_with_manifests(|id| {
            let origin = by_attempt
                .get(id)
                .ok_or_else(|| invalid("fallback payload has detached original attempt"))?;
            let entry = history
                .attempts
                .iter()
                .find(|a| a.attempt_id == *id)
                .ok_or_else(|| invalid("fallback original entry absent"))?;
            if origin.state != entry.state
                || origin.manifest.sha256 != entry.manifest_sha256
                || origin.base_revision != history.base_revision
                || origin.source_version != history.source_version
            {
                return Err(invalid(
                    "fallback captured origin differs from its original payload",
                ));
            }
            let data = bytes(&origin.manifest)?;
            if data.len() > 1024 * 1024
                || data.len() as u64 != origin.manifest.byte_len
                || digest(&data) != origin.manifest.sha256
            {
                return Err(invalid("fallback original worker object bytes differ"));
            }
            let request: WorkerRequest = serde_json::from_slice(&data)
                .map_err(|_| invalid("invalid fallback worker object"))?;
            if encode(&request)?.as_bytes() != data {
                return Err(invalid(
                    "fallback original worker encoding is not canonical",
                ));
            }
            if !manifest.sources.iter().any(|s| {
                s.source_version == request.source.source_version
                    && s.kind == SourceKind::Media
                    && s.identity == request.source.identity
            }) {
                return Err(invalid("fallback original decoded-media source differs"));
            }
            Ok(request)
        })?;
        for origin in origins {
            verified.insert((origin.manifest.sha256.clone(), origin.manifest.byte_len));
        }
    }
    // A fallback dependency without a checked declaration cannot become legacy
    // history merely by omitting its new metadata field.
    for origin in &manifest.generated_origins {
        let data = bytes(&origin.manifest)?;
        if data.len() > 1024 * 1024
            || data.len() as u64 != origin.manifest.byte_len
            || digest(&data) != origin.manifest.sha256
        {
            return Err(invalid("archived worker bytes differ from declared object"));
        }
        let request: WorkerRequest = serde_json::from_slice(&data)
            .map_err(|_| invalid("invalid archived worker manifest"))?;
        let tags: Vec<_> = request
            .dependencies
            .iter()
            .filter(|d| {
                d.artifact_id
                    .as_str()
                    .starts_with(INFERENCE_FALLBACK_SELECTION_ID_PREFIX)
            })
            .collect();
        if !tags.is_empty()
            && !verified.contains(&(origin.manifest.sha256.clone(), origin.manifest.byte_len))
        {
            return Err(invalid("fallback worker has no checked original payload"));
        }
        if tags.is_empty() && origin.fallback_history.is_some() {
            return Err(invalid("fallback history attached to an ordinary worker"));
        }
    }
    for candidate in &manifest.candidates {
        if let Some(tag) = candidate.job_origin.as_ref().and_then(|job| tags.get(job)) {
            let selected: Vec<_> = candidate.lineage.iter().filter(|d| is_tag(d)).collect();
            if selected.len() != 1 || selected[0] != tag {
                return Err(invalid(
                    "fallback candidate lost its exact original job policy",
                ));
            }
        }
    }
    Ok(CheckedNode {
        workers: verified,
        tags,
        payloads,
    })
}

fn is_tag(identity: &ArtifactIdentity) -> bool {
    identity
        .artifact_id
        .as_str()
        .starts_with(INFERENCE_FALLBACK_SELECTION_ID_PREFIX)
}
fn identity_key(identity: &ArtifactIdentity) -> (String, String, u64) {
    (
        identity.artifact_id.to_string(),
        identity.sha256.clone(),
        identity.byte_len,
    )
}

/// Metadata-only closure preflight. Bytes still require GraphIndex below before
/// a captured/exported/imported graph can be published.
pub(crate) fn validate_metadata_graph(
    manifest: &PortableProjectManifest,
    origins: &BTreeMap<String, PortableProjectManifest>,
) -> PResult<()> {
    let has_payload = std::iter::once(manifest)
        .chain(origins.values())
        .any(|node| {
            node.generated_origins
                .iter()
                .any(|o| o.fallback_history.is_some())
        });
    for node in std::iter::once(manifest).chain(origins.values()) {
        let mut jobs: BTreeMap<&JobId, Vec<&PackageGeneratedOrigin>> = BTreeMap::new();
        for origin in &node.generated_origins {
            jobs.entry(&origin.job_id).or_default().push(origin);
        }
        for group in jobs.values() {
            let refs: BTreeSet<_> = group
                .iter()
                .filter_map(|o| o.fallback_history.as_ref().map(|r| (&r.sha256, r.byte_len)))
                .collect();
            if refs.len() > 1
                || (!refs.is_empty() && group.iter().any(|o| o.fallback_history.is_none()))
            {
                return Err(invalid(
                    "fallback job payload declarations differ or are stripped",
                ));
            }
        }
        if !has_payload && node.candidates.iter().any(|c| c.lineage.iter().any(is_tag)) {
            return Err(ProtocolError::unsupported(
                "portable fallback lineage has no declared original payload",
            ));
        }
    }
    Ok(())
}

/// Bounded original-graph membership only. No report is installed in the local
/// owner registry and no historical observation becomes execution authority.
#[derive(Default)]
pub(crate) struct GraphIndex {
    workers: BTreeSet<(String, u64)>,
    tags: BTreeSet<(String, String, u64)>,
    candidates: BTreeSet<(String, String, u64)>,
    payloads: BTreeSet<(String, u64)>,
    bytes: usize,
}
impl GraphIndex {
    fn charge(&mut self, bytes: usize) -> PResult<()> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .filter(|n| *n <= 128 * 1024 * 1024)
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "fallback graph index admission exceeded",
                )
            })?;
        Ok(())
    }
    pub(crate) fn observe_node(
        &mut self,
        node: &PortableProjectManifest,
        mut bytes: impl FnMut(&PackageObjectRef) -> PResult<Vec<u8>>,
    ) -> PResult<()> {
        let checked = check_node(node, &mut bytes)?;
        for worker in checked.workers {
            if !self.workers.contains(&worker) {
                self.charge(128)?;
                self.workers.insert(worker);
            }
        }
        for tag in checked.tags.values().map(identity_key) {
            if !self.tags.contains(&tag) {
                self.charge(384)?;
                self.tags.insert(tag);
            }
        }
        for payload in checked.payloads {
            if !self.payloads.contains(&payload) {
                self.charge(128)?;
                self.payloads.insert(payload);
            }
        }
        for tag in node
            .candidates
            .iter()
            .flat_map(|c| &c.lineage)
            .filter(|d| is_tag(d))
            .map(identity_key)
        {
            if !self.candidates.contains(&tag) {
                self.charge(384)?;
                self.candidates.insert(tag);
            }
        }
        Ok(())
    }
    pub(crate) fn check_worker(
        &self,
        reference: &PackageObjectRef,
        request: &WorkerRequest,
    ) -> PResult<()> {
        if self
            .workers
            .contains(&(reference.sha256.clone(), reference.byte_len))
            || request.dependencies.iter().any(is_tag)
        {
            request.validate()?;
            let raw = encode(request)?;
            if raw.len() as u64 != reference.byte_len || digest(raw.as_bytes()) != reference.sha256
            {
                return Err(invalid("fallback graph worker identity changed"));
            }
        }
        if request.dependencies.iter().any(is_tag)
            && !self
                .workers
                .contains(&(reference.sha256.clone(), reference.byte_len))
        {
            return Err(ProtocolError::unsupported(
                "portable fallback worker lacks its exact checked original payload",
            ));
        }
        Ok(())
    }
    pub(crate) fn check_payload(&self, reference: &PackageObjectRef) -> PResult<()> {
        if !self
            .payloads
            .contains(&(reference.sha256.clone(), reference.byte_len))
        {
            return Err(invalid("detached fallback payload object in archive graph"));
        }
        Ok(())
    }
    pub(crate) fn finish(&self) -> PResult<()> {
        if !self.candidates.is_subset(&self.tags) {
            return Err(invalid(
                "fallback lineage lacks its exact checked policy in the origin graph",
            ));
        }
        Ok(())
    }
}
