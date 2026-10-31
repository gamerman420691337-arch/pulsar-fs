//! Browser recording archives retain exact source bytes and historical decisions.
//! Nothing here resolves host paths, installs acceptance, or creates execution.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const RECORDING_LIMIT: u64 = 40 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"PULSBH1\0";
const HEADER_LIMIT: usize = 32 * 1024 * 1024;
const MAX_REPORTS: usize = 256;
type ReferenceKey = (String, u64, String);
fn invalid(message: &str) -> ProtocolError {
    ProtocolError::invalid(message)
}
fn digest(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn same<T: Serialize + ?Sized>(a: &T, b: &T) -> PResult<bool> {
    Ok(serde_json::to_vec(a).map_err(internal)? == serde_json::to_vec(b).map_err(internal)?)
}
fn key(r: &InferenceFallbackReference) -> ReferenceKey {
    (
        r.dossier_id.to_string(),
        r.expected_revision,
        r.expected_report_sha256.clone(),
    )
}
fn report_key(r: &InferenceEquivalenceReport) -> PResult<ReferenceKey> {
    Ok((
        r.dossier_id.to_string(),
        r.revision,
        digest(encode(r)?.as_bytes()),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Recording {
    schema: u16,
    kind: String,
    qualified: bool,
    stage_request_id: RequestId,
    pub(super) actor: SessionId,
    options: LiveRecordingStageOptions,
    source_time_basis: LiveSourceTimeBasis,
    contains_cuda_provisional: bool,
    pub(super) pages: Vec<LiveRecordingPage>,
    model_pack: Option<ModelPackChoice>,
    browser_media: Option<BrowserMediaBinding>,
}
impl Recording {
    pub(super) fn parse(raw: &[u8]) -> PResult<Self> {
        if raw.is_empty() || raw.len() as u64 > RECORDING_LIMIT {
            return Err(invalid("browser recording bytes exceed archival admission"));
        }
        let value: Self = serde_json::from_slice(raw)
            .map_err(|_| invalid("invalid original recording source"))?;
        value.options.validate()?;
        if value.schema != 1
            || value.kind != "engine-owned-live-recording-source"
            || value.qualified
            || value.pages.is_empty()
            || value.pages.len() > 256
            || value.stage_request_id.as_str().is_empty()
        {
            return Err(invalid("original recording schema or page count differs"));
        }
        let first = &value.pages[0];
        if value.options.engine_epoch != first.engine_epoch
            || value.source_time_basis != first.source_time_basis
            || value.model_pack != first.model_pack
            || value.browser_media != first.browser_media
        {
            return Err(invalid("original recording envelope scope differs"));
        }
        let mut selections = Vec::new();
        for page in &value.pages {
            page.validate()?;
            if selections
                .last()
                .is_none_or(|s: &LiveRecordingSelection| s.epoch != page.identity.epoch)
            {
                selections.push(LiveRecordingSelection {
                    epoch: page.identity.epoch.clone(),
                    recording_request_id: page.recording_request_id.clone(),
                });
            }
        }
        if !same(&selections, &value.options.recordings)? {
            return Err(invalid(
                "recording stage selection differs from retained scopes",
            ));
        }
        let assembled = assemble_live_recordings(
            &value.pages,
            value.options.range,
            value.options.maximum_interpolation_ns,
        )?;
        if assembled.contains_cuda_provisional != value.contains_cuda_provisional {
            return Err(invalid("recording provisional declaration differs"));
        }
        Ok(value)
    }
    pub(super) fn selected(&self) -> bool {
        self.pages.iter().any(|p| p.fallback.is_some())
    }
    pub(super) fn project(&self) -> &ProjectId {
        &self.pages[0].identity.project
    }
    fn epochs(&self) -> BTreeSet<AttemptId> {
        let mut epochs = BTreeSet::new();
        for page in &self.pages {
            if let Some(f) = &page.fallback {
                epochs.insert(page.identity.epoch.clone());
                for step in &f.admission.history {
                    epochs.insert(step.failure.manifest.stream.identity.epoch.clone());
                }
            }
        }
        epochs
    }
    fn references(&self) -> PResult<BTreeMap<ReferenceKey, InferenceFallbackReference>> {
        let mut refs = BTreeMap::new();
        for p in &self.pages {
            if let Some(f) = &p.fallback {
                refs.insert(key(&f.admission.reference), f.admission.reference.clone());
                for step in &f.admission.history {
                    refs.insert(key(&step.reference), step.reference.clone());
                }
            }
        }
        if refs.len() > MAX_REPORTS {
            return Err(invalid(
                "browser history referenced decision count exceeds admission",
            ));
        }
        Ok(refs)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OriginalAdmission {
    actor: SessionId,
    request_id: RequestId,
    fingerprint: String,
    response: String,
}
impl OriginalAdmission {
    fn capture(&self) -> PResult<LiveCaptureReport> {
        validate_sha256(&self.fingerprint)?;
        if self.response.is_empty() || self.response.len() > 1024 * 1024 {
            return Err(invalid(
                "browser original acknowledgement exceeds admission",
            ));
        }
        let body: ResponseBody = serde_json::from_str(&self.response)
            .map_err(|_| invalid("browser original acknowledgement is invalid"))?;
        if encode(&body)? != self.response {
            return Err(invalid("browser original acknowledgement is not canonical"));
        }
        let capture = match body {
            ResponseBody::LiveCapture(c) => *c,
            ResponseBody::BrowserAnalysisSuccessor(c) => {
                c.validate()?;
                if c.original_request_id != self.request_id {
                    return Err(invalid("browser original successor request differs"));
                }
                c.successor
            }
            _ => return Err(invalid("browser original acknowledgement kind differs")),
        };
        capture.validate()?;
        let info = capture
            .inference
            .as_ref()
            .ok_or_else(|| invalid("browser original inference absent"))?;
        if !capture.active
            || capture.received_frames != 0
            || info.state != LiveCaptureInferenceState::Starting
            || info.completed != 0
            || info.accepted != 0
            || info.fallback.is_none()
            || info.fallback_transition.is_some()
        {
            return Err(invalid(
                "browser original admission is not the initial acknowledgement",
            ));
        }
        Ok(capture)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema_version: u16,
    origin_project: ProjectId,
    source_version: SourceVersionId,
    snapshot: PackageObjectRef,
    admissions: Vec<OriginalAdmission>,
    reports: Vec<InferenceEquivalenceReport>,
}
struct Payload {
    header: Header,
    documents: BTreeMap<String, Vec<u8>>,
}
impl Header {
    fn documents(&self) -> PResult<BTreeMap<String, u64>> {
        self.snapshot.validate()?;
        if self.schema_version != 1
            || self.snapshot.byte_len == 0
            || self.snapshot.byte_len > RECORDING_LIMIT
            || self.reports.is_empty()
            || self.reports.len() > MAX_REPORTS
            || self.admissions.is_empty()
            || self.admissions.len() > 1024
        {
            return Err(invalid("browser history header admission differs"));
        }
        let mut docs = BTreeMap::new();
        let mut previous = None;
        for report in &self.reports {
            report.validate()?;
            let k = report_key(report)?;
            if previous.as_ref().is_some_and(|p| p >= &k)
                || report.state != InferenceEquivalenceState::Accepted
                || report.manifest.subject.origin != InferenceEquivalenceOrigin::Measured
            {
                return Err(invalid(
                    "browser history original decision state or ordering differs",
                ));
            }
            previous = Some(k);
            for e in &report.manifest.evidence {
                if docs
                    .insert(e.identity.sha256.clone(), e.identity.byte_len)
                    .is_some_and(|len| len != e.identity.byte_len)
                {
                    return Err(invalid("browser evidence aliases disagree"));
                }
            }
        }
        let total = docs
            .values()
            .try_fold(0u64, |n, v| n.checked_add(*v))
            .ok_or_else(|| invalid("browser evidence length overflow"))?;
        if total > 128 * 1024 * 1024 || encode(self)?.len() > HEADER_LIMIT {
            return Err(invalid("browser evidence aggregate admission exceeded"));
        }
        Ok(docs)
    }
}
impl Payload {
    fn validate(&self, recording: &Recording) -> PResult<()> {
        let expected = self.header.documents()?;
        if self.documents.len() != expected.len()
            || self.header.origin_project != *recording.project()
        {
            return Err(invalid("browser history project or document set differs"));
        }
        for (hash, len) in expected {
            let raw = self
                .documents
                .get(&hash)
                .ok_or_else(|| invalid("browser evidence document absent"))?;
            if raw.len() as u64 != len || digest(raw) != hash {
                return Err(invalid("browser evidence bytes differ"));
            }
        }
        let mut references = recording.references()?;
        let mut originals = BTreeMap::new();
        let mut previous = None;
        for declaration in &self.header.admissions {
            let original = declaration.capture()?;
            let epoch = original.identity.epoch.clone();
            if declaration.actor != recording.actor
                || previous.as_ref().is_some_and(|p| p >= &epoch)
                || original.identity.project != self.header.origin_project
            {
                return Err(invalid(
                    "browser original admission actor, namespace or ordering differs",
                ));
            }
            previous = Some(epoch.clone());
            let admission = original
                .inference
                .as_ref()
                .and_then(|i| i.fallback.as_ref())
                .expect("checked admission");
            references.insert(key(&admission.reference), admission.reference.clone());
            originals.insert(epoch, original);
        }
        if originals.keys().ne(recording.epochs().iter()) {
            return Err(invalid(
                "browser original admission scopes are missing or detached",
            ));
        }
        for page in &recording.pages {
            if let Some(f) = &page.fallback {
                let original = &originals[&page.identity.epoch];
                if original.identity != page.identity
                    || original.engine_epoch != page.engine_epoch
                    || original.clock != page.clock
                    || original
                        .inference
                        .as_ref()
                        .expect("checked inference")
                        .attempt_id
                        != page.inference_attempt
                    || !same(
                        &original
                            .inference
                            .as_ref()
                            .expect("checked inference")
                            .cpu_manifest,
                        &page.cpu_manifest,
                    )?
                    || !same(&original.options, &*f.options)?
                    || !same(
                        original
                            .inference
                            .as_ref()
                            .and_then(|i| i.fallback.as_ref())
                            .expect("checked admission"),
                        &f.admission,
                    )?
                {
                    return Err(invalid(
                        "browser recording differs from original admission acknowledgement",
                    ));
                }
                for (index, failure) in f
                    .admission
                    .history
                    .iter()
                    .map(|s| &s.failure)
                    .chain(f.transition.iter().map(|t| &t.failure))
                    .enumerate()
                {
                    let stream = &failure.manifest.stream;
                    let old = &originals[&stream.identity.epoch];
                    let info = old.inference.as_ref().expect("checked admission");
                    let admission = info.fallback.as_ref().expect("checked admission");
                    if admission.selection != f.admission.selection
                        || admission.semantic_identity != f.admission.semantic_identity
                        || usize::from(admission.ordinal) != index
                        || !same(&admission.history[..], &f.admission.history[..index])?
                    {
                        return Err(invalid(
                            "browser failure lost its original selection or exact history prefix",
                        ));
                    }
                    if old.identity != stream.identity
                        || old.clock != stream.clock
                        || old.engine_epoch != page.engine_epoch
                        || info.attempt_id != stream.attempt_id
                        || stream.budget.wall_time_ms != u64::from(old.options.lease_ms)
                        || !same(
                            &old.options
                                .inference
                                .as_ref()
                                .expect("checked profile")
                                .profile,
                            &stream.profile,
                        )?
                        || info
                            .fallback
                            .as_ref()
                            .expect("checked admission")
                            .observed_environment
                            .worker
                            != stream.worker
                    {
                        return Err(invalid(
                            "browser failure differs from original admission scope",
                        ));
                    }
                }
            }
        }
        let reports: BTreeMap<_, _> = self
            .header
            .reports
            .iter()
            .map(|r| Ok((report_key(r)?, r)))
            .collect::<PResult<_>>()?;
        if references.keys().ne(reports.keys()) {
            return Err(invalid("browser history decisions are missing or detached"));
        }
        for report in &self.header.reports {
            let docs = report
                .manifest
                .evidence
                .iter()
                .map(|e| (e.kind, self.documents[&e.identity.sha256].clone()))
                .collect();
            let assessment = pulsar_protocol::validate_evidence(&report.manifest, &docs)?;
            let decision = report
                .decision
                .as_ref()
                .ok_or_else(|| invalid("original browser decision absent"))?;
            let at = decision
                .recorded_at_unix_ms
                .ok_or_else(|| invalid("original browser decision clock absent"))?;
            assessment.require_acceptance(at)?;
            if decision.review_identity.as_ref() != Some(assessment.review_identity()) {
                return Err(invalid("browser original review differs"));
            }
            let subject = &report.manifest.subject;
            let report_key = report_key(report)?;
            for original in originals.values() {
                let admission = original
                    .inference
                    .as_ref()
                    .and_then(|i| i.fallback.as_ref())
                    .expect("checked admission");
                if key(&admission.reference) != report_key {
                    continue;
                }
                let selected = original
                    .options
                    .inference_fallback
                    .as_ref()
                    .expect("checked selection");
                if !selected.matches_capture(&original.engine_epoch, &original.options, subject)? {
                    return Err(invalid(
                        "browser decision does not cover original selected semantics",
                    ));
                }
                let (profile, env) = if admission.ordinal == 0 {
                    (&subject.from_profile, &subject.from_environment)
                } else {
                    (&subject.to_profile, &subject.to_environment)
                };
                if !same(
                    &original
                        .options
                        .inference
                        .as_ref()
                        .expect("checked profile")
                        .profile,
                    profile,
                )? || env.identity_sha256()?
                    != admission.observed_environment.identity_sha256()?
                    || admission.admitted_at_unix_ms < at
                    || admission.expires_at_unix_ms != subject.expires_unix_ms
                {
                    return Err(invalid(
                        "browser original profile, environment or decision clocks differ",
                    ));
                }
                assessment.require_acceptance(admission.admitted_at_unix_ms)?;
            }
            for page in &recording.pages {
                if let Some(f) = &page.fallback {
                    for step in &f.admission.history {
                        if key(&step.reference) != report_key {
                            continue;
                        }
                        let old = &originals[&step.failure.manifest.stream.identity.epoch];
                        let admission = old
                            .inference
                            .as_ref()
                            .and_then(|i| i.fallback.as_ref())
                            .expect("checked admission");
                        if !same(&step.failure.manifest.stream.profile, &subject.from_profile)? {
                            return Err(invalid(
                                "browser original directed failure profile differs",
                            ));
                        }
                        if admission.observed_environment.identity_sha256()?
                            != subject.from_environment.identity_sha256()?
                        {
                            return Err(invalid(
                                "browser original directed failure environment differs",
                            ));
                        }
                        if step.next_profile_sha256
                            != digest(encode(&subject.to_profile)?.as_bytes())
                        {
                            return Err(invalid(
                                "browser original directed failure target profile differs",
                            ));
                        }
                        // Native CUDA capture pins the host model before mapping it
                        // into the worker at the fixed guest path. Compare source
                        // scope against the original declaration without altering
                        // the retained manifest or accepting arbitrary path aliases.
                        let mut source_stream = step.failure.manifest.stream.clone();
                        let declared_path = &old
                            .options
                            .inference
                            .as_ref()
                            .expect("checked profile")
                            .model_path;
                        if source_stream.model.path != *declared_path {
                            if source_stream.model.path != std::path::Path::new(LIVE_CPU_MODEL_PATH)
                            {
                                return Err(invalid(
                                    "browser original confined model path differs",
                                ));
                            }
                            source_stream.model.path = declared_path.clone();
                        }
                        if !old
                            .options
                            .inference_fallback
                            .as_ref()
                            .expect("checked selection")
                            .matches_subject(
                                &old.engine_epoch,
                                &old.options,
                                &source_stream,
                                subject,
                            )?
                        {
                            return Err(invalid(
                                "browser original directed failure source scope differs",
                            ));
                        }
                        assessment.require_acceptance(admission.admitted_at_unix_ms)?;
                    }
                }
            }
        }
        Ok(())
    }
    fn encode(&self, admission: usize) -> PResult<Vec<u8>> {
        let expected = self.header.documents()?;
        let header = serde_json::to_vec(&self.header).map_err(internal)?;
        let total = expected
            .values()
            .try_fold(12usize + header.len(), |n, v| n.checked_add(*v as usize))
            .ok_or_else(|| invalid("browser history frame overflow"))?;
        if total > admission.min(MAX_PACKAGE_FALLBACK_HISTORY_BYTES) {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "browser history capture admission exceeded",
            ));
        }
        let mut raw = Vec::with_capacity(total);
        raw.extend_from_slice(MAGIC);
        raw.extend_from_slice(&(header.len() as u32).to_le_bytes());
        raw.extend(header);
        for hash in expected.keys() {
            raw.extend_from_slice(
                self.documents
                    .get(hash)
                    .ok_or_else(|| invalid("browser evidence absent during encoding"))?,
            );
        }
        Ok(raw)
    }
    fn decode(raw: &[u8]) -> PResult<Self> {
        if raw.len() < 12 || raw.len() > MAX_PACKAGE_FALLBACK_HISTORY_BYTES || &raw[..8] != MAGIC {
            return Err(invalid("browser history frame differs"));
        }
        let len = u32::from_le_bytes(raw[8..12].try_into().expect("bounded header")) as usize;
        if len == 0 || len > HEADER_LIMIT || 12 + len > raw.len() {
            return Err(invalid("browser history header length differs"));
        }
        let header: Header = serde_json::from_slice(&raw[12..12 + len])
            .map_err(|_| invalid("invalid browser history header"))?;
        if serde_json::to_vec(&header).map_err(internal)? != raw[12..12 + len] {
            return Err(invalid("browser history header is not canonical"));
        }
        let mut cursor = 12 + len;
        let mut documents = BTreeMap::new();
        for (hash, size) in header.documents()? {
            let end = cursor
                .checked_add(size as usize)
                .filter(|n| *n <= raw.len())
                .ok_or_else(|| invalid("browser history evidence truncated"))?;
            documents.insert(hash, raw[cursor..end].to_vec());
            cursor = end;
        }
        if cursor != raw.len() {
            return Err(invalid("browser history trailing bytes"));
        }
        Ok(Self { header, documents })
    }
}

pub(super) fn capture(
    tx: &Transaction<'_>,
    source: &PackageSource,
    recording: &Recording,
    remaining: usize,
) -> PResult<Option<Vec<u8>>> {
    if !recording.selected() {
        return Ok(None);
    }
    let mut remaining_documents = remaining;
    let mut reports = Vec::new();
    let mut documents = BTreeMap::new();
    let mut admissions = Vec::new();
    let mut references = recording.references()?;
    for epoch in recording.epochs() {
        let (actor, request_id, fingerprint, response) =
            crate::browser_fallback_history::archival_receipt(tx, &epoch).map_err(internal)?;
        remaining_documents = remaining_documents
            .checked_sub(response.len().saturating_mul(2))
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "browser original acknowledgements exceed capture admission",
                )
            })?;
        let declaration = OriginalAdmission {
            actor,
            request_id,
            fingerprint,
            response,
        };
        let original = declaration.capture()?;
        let admission = original
            .inference
            .as_ref()
            .and_then(|i| i.fallback.as_ref())
            .expect("checked admission");
        references.insert(key(&admission.reference), admission.reference.clone());
        admissions.push(declaration);
    }
    if references.len() > MAX_REPORTS {
        return Err(invalid("browser original decision count exceeds admission"));
    }
    for reference in references.values() {
        let (report, docs) = super::inference_equivalence_store::archival_decision(
            tx,
            reference,
            &mut remaining_documents,
        )?;
        reports.push(report);
        documents.extend(docs);
    }
    let payload = Payload {
        header: Header {
            schema_version: 1,
            origin_project: recording.project().clone(),
            source_version: source.source_version.clone(),
            snapshot: PackageObjectRef {
                sha256: source.identity.sha256.clone(),
                byte_len: source.identity.byte_len,
            },
            admissions,
            reports,
        },
        documents,
    };
    payload.validate(recording)?;
    payload.encode(remaining).map(Some)
}

/// Full checked root/ancestor closure; source and evidence paths remain opaque.
pub(crate) fn validate_graph(
    root: &PortableProjectManifest,
    origins: &BTreeMap<String, PortableProjectManifest>,
    mut read: impl FnMut(&PackageObjectRef) -> PResult<Vec<u8>>,
) -> PResult<()> {
    let nodes: Vec<_> = std::iter::once(root).chain(origins.values()).collect();
    if !nodes.iter().any(|node| {
        node.sources
            .iter()
            .any(|s| s.kind == SourceKind::LiveRecording || s.browser_fallback_history.is_some())
            || node.objects.iter().any(|o| {
                o.roles
                    .contains(&PackageObjectRole::BrowserInferenceFallbackHistory)
            })
    }) {
        return Ok(());
    }
    crate::project_package_import_plan::validate_retained_origin_graph(root, origins)?;
    let mut seen = BTreeSet::new();
    let mut declared = BTreeMap::new();
    let mut charged = 0usize;
    for node in &nodes {
        for source in &node.sources {
            if let Some(r) = &source.browser_fallback_history {
                declared.insert(r.sha256.clone(), r.byte_len);
            }
            if source.kind != SourceKind::LiveRecording {
                continue;
            }
            let binding = (
                source.source_version.clone(),
                source.identity.sha256.clone(),
                source
                    .browser_fallback_history
                    .clone()
                    .map(|r| (r.sha256, r.byte_len)),
            );
            if !seen.insert(binding) {
                continue;
            }
            let snapshot = PackageObjectRef {
                sha256: source.identity.sha256.clone(),
                byte_len: source.identity.byte_len,
            };
            let raw = checked_bytes(&snapshot, RECORDING_LIMIT, &mut charged, &mut read)?;
            let recording = Recording::parse(&raw)?;
            match &source.browser_fallback_history {
                None if !recording.selected() => {}
                Some(reference) => {
                    let body = checked_bytes(
                        reference,
                        MAX_PACKAGE_FALLBACK_HISTORY_BYTES as u64,
                        &mut charged,
                        &mut read,
                    )?;
                    let payload = Payload::decode(&body)?;
                    if payload.header.snapshot != snapshot
                        || payload.header.source_version != source.source_version
                    {
                        return Err(invalid(
                            "browser history snapshot or source identity differs",
                        ));
                    }
                    let original = nodes
                        .iter()
                        .find(|original| {
                            original.origin_project_id == payload.header.origin_project
                                && recording.pages.iter().all(|page| {
                                    page.identity.revision <= original.captured_revision
                                })
                                && original.sources.iter().any(|s| {
                                    s.source_version == source.source_version
                                        && s.identity == source.identity
                                        && s.browser_fallback_history
                                            == source.browser_fallback_history
                                })
                        })
                        .ok_or_else(|| {
                            invalid("browser history original source namespace is absent")
                        })?;
                    for page in recording.pages.iter().filter(|p| p.fallback.is_some()) {
                        let media = page.browser_media.as_ref().ok_or_else(|| {
                            invalid("selected browser recording lacks original media binding")
                        })?;
                        if !original.sources.iter().any(|s| {
                            s.source_version == media.selection.source_version
                                && s.kind == SourceKind::Media
                                && s.identity == media.selection.source
                        }) || !original
                            .placements
                            .as_ref()
                            .is_some_and(|p| p.definitions.contains(&media.selection.placement))
                        {
                            return Err(invalid("browser recording original media snapshot or placement declaration is absent"));
                        }
                    }
                    payload.validate(&recording)?;
                }
                _ => {
                    return Err(invalid(
                        "selected browser recording lost its evidence declaration",
                    ))
                }
            }
        }
    }
    for object in root.objects.iter().filter(|o| {
        o.roles
            .contains(&PackageObjectRole::BrowserInferenceFallbackHistory)
    }) {
        if declared.get(&object.sha256) != Some(&object.byte_len) {
            return Err(invalid("detached browser history object"));
        }
    }
    Ok(())
}
fn checked_bytes(
    reference: &PackageObjectRef,
    limit: u64,
    charged: &mut usize,
    read: &mut impl FnMut(&PackageObjectRef) -> PResult<Vec<u8>>,
) -> PResult<Vec<u8>> {
    reference.validate()?;
    *charged = charged
        .checked_add(reference.byte_len as usize)
        .filter(|n| *n <= 256 * 1024 * 1024)
        .ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "browser archive graph aggregate admission exceeded",
            )
        })?;
    if reference.byte_len == 0 || reference.byte_len > limit {
        return Err(invalid("browser archive object exceeds byte admission"));
    }
    let raw = read(reference)?;
    if raw.len() as u64 != reference.byte_len || digest(&raw) != reference.sha256 {
        return Err(invalid("browser archive object bytes differ"));
    }
    Ok(raw)
}

#[cfg(test)]
#[path = "portable_browser_history_tests.rs"]
mod tests;

#[cfg(test)]
pub(in crate::authority) fn authored_local_browser_recording(
    engine: &Engine,
    actor: &SessionId,
    token: &str,
    project: &ProjectId,
    revision: RevisionId,
    media: &SourceArtifact,
    placement: &SourcePlacement,
) -> Vec<u8> {
    tests::authored_local_browser_recording(
        engine, actor, token, project, revision, media, placement,
    )
}
