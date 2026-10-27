//! Exact imported-media metadata and declared clock mapping, never native authority.
use crate::*;
use std::collections::HashSet;

pub const BROWSER_MEDIA_CATALOG_LIMIT: usize = 256;
pub const BROWSER_MEDIA_MAX_LEASE_MS: u32 = 600_000;
pub const BROWSER_MEDIA_MAX_EXACT_INTEGER: u64 = (1u64 << 53) - 1;
pub const BROWSER_MEDIA_PREPARATION_ERROR: &str = "browser media preparation failed";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserMediaSelection {
    pub source_version: SourceVersionId,
    pub source: ArtifactIdentity,
    pub placement: SourcePlacement,
}
impl BrowserMediaSelection {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.source.validate()?;
        self.placement.spec.validate().map_err(core_error)?;
        if self.source.byte_len == 0
            || self.source.byte_len > BROWSER_MEDIA_MAX_EXACT_INTEGER
            || self.source_version != self.placement.spec.source_version
        {
            return Err(ProtocolError::invalid(
                "browser media source selection differs",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserMediaCatalogEntry {
    pub label: String,
    pub selection: BrowserMediaSelection,
}
impl BrowserMediaCatalogEntry {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        bounded_text(&self.label, 256, "browser media label")?;
        self.selection.validate()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserMediaCatalog {
    pub engine_epoch: AttemptId,
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub entries: Vec<BrowserMediaCatalogEntry>,
}
impl BrowserMediaCatalog {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let mut ids = HashSet::new();
        if self.entries.len() > BROWSER_MEDIA_CATALOG_LIMIT {
            return Err(ProtocolError::invalid(
                "browser media catalog exceeds bound",
            ));
        }
        for entry in &self.entries {
            entry.validate()?;
            if entry.selection.placement.created_revision > self.revision
                || !ids.insert(&entry.selection.placement.placement_id)
            {
                return Err(ProtocolError::invalid(
                    "browser media catalog placement differs",
                ));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserMediaBinding {
    pub preparation_request_id: RequestId,
    pub lease_id: ArtifactId,
    pub engine_epoch: AttemptId,
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub selection: BrowserMediaSelection,
    pub clock: DeclaredSourceClock,
    pub lease_ms: u32,
}
fn core_error(error: CoreError) -> ProtocolError {
    ProtocolError::invalid(error.to_string())
}
fn validate_binding_fields(
    selection: &BrowserMediaSelection,
    clock: &DeclaredSourceClock,
    revision: RevisionId,
    lease_ms: u32,
) -> Result<(), ProtocolError> {
    selection.validate()?;
    clock.validate().map_err(core_error)?;
    let spec = &selection.placement.spec;
    let (_, end) = clock.presentation_trim_micros(spec).map_err(core_error)?;
    if selection.placement.created_revision > revision
        || !(1..=BROWSER_MEDIA_MAX_LEASE_MS).contains(&lease_ms)
        || i64::from(spec.rate_numerator) != i64::from(spec.rate_denominator)
        || end > BROWSER_MEDIA_MAX_EXACT_INTEGER
    {
        return Err(ProtocolError::invalid(
            "unsupported browser media placement or lifetime",
        ));
    }
    Ok(())
}
impl BrowserMediaBinding {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_binding_fields(&self.selection, &self.clock, self.revision, self.lease_ms)
    }
    /// Check exact declared source/project anchoring, without claiming the RGB
    /// body actually came from this file or a calibrated browser frame.
    pub fn validate_capture_anchor(
        &self,
        basis: LiveSourceTimeBasis,
        source_anchor: SourceTimestamp,
        project_anchor: ProjectTime,
        exact_project_anchor: Option<ExactSeconds>,
    ) -> Result<(), ProtocolError> {
        self.validate()?;
        let exact = self
            .clock
            .project_at(source_anchor, &self.selection.placement.spec)
            .map_err(core_error)?;
        if basis != LiveSourceTimeBasis::SourceMediaPresentation
            || exact_project_anchor != Some(exact)
            || project_anchor != exact.quantize_nanoseconds().map_err(core_error)?.time
        {
            return Err(ProtocolError::invalid(
                "browser media capture clock differs",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserMediaPreparationState {
    Pending,
    Ready,
    Cancelled,
    Failed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserMediaPreparationReport {
    pub request_id: RequestId,
    pub engine_epoch: AttemptId,
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub selection: BrowserMediaSelection,
    pub clock: DeclaredSourceClock,
    pub lease_ms: u32,
    pub state: BrowserMediaPreparationState,
    pub verified_bytes: u64,
    pub binding: Option<BrowserMediaBinding>,
    pub current: bool,
    pub error: Option<String>,
}
impl BrowserMediaPreparationReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_binding_fields(&self.selection, &self.clock, self.revision, self.lease_ms)?;
        if self.verified_bytes > self.selection.source.byte_len {
            return Err(ProtocolError::invalid(
                "browser media progress exceeds source",
            ));
        }
        if self.state == BrowserMediaPreparationState::Ready {
            let binding = self
                .binding
                .as_ref()
                .ok_or_else(|| ProtocolError::invalid("ready browser media lacks binding"))?;
            binding.validate()?;
            if binding.preparation_request_id != self.request_id
                || binding.engine_epoch != self.engine_epoch
                || binding.project_id != self.project_id
                || binding.revision != self.revision
                || binding.selection != self.selection
                || binding.clock != self.clock
                || binding.lease_ms != self.lease_ms
                || self.verified_bytes != self.selection.source.byte_len
            {
                return Err(ProtocolError::invalid(
                    "ready browser media binding differs",
                ));
            }
        } else if self.binding.is_some() {
            return Err(ProtocolError::invalid(
                "unfinished browser media carries a binding",
            ));
        }
        if matches!(
            self.state,
            BrowserMediaPreparationState::Failed | BrowserMediaPreparationState::Cancelled
        ) && self.current
        {
            return Err(ProtocolError::invalid(
                "terminal browser media cannot be current",
            ));
        }
        let expected_error = (self.state == BrowserMediaPreparationState::Failed)
            .then_some(BROWSER_MEDIA_PREPARATION_ERROR);
        if self.error.as_deref() != expected_error {
            return Err(ProtocolError::invalid("browser media error must be static"));
        }
        Ok(())
    }
}

pub(crate) fn validate_browser_media_request(request: &Request) -> Result<(), ProtocolError> {
    match &request.command {
        Command::PrepareBrowserMedia {
            selection,
            clock,
            lease_ms,
            allow_declared_clock,
            ..
        } => {
            let revision = request
                .expected_revision
                .ok_or_else(|| ProtocolError::invalid("browser media requires revision"))?;
            validate_binding_fields(selection, clock, revision, *lease_ms)?;
            if !allow_declared_clock {
                return Err(ProtocolError::new(
                    ErrorCode::Forbidden,
                    "browser media requires explicit declared-clock acknowledgment",
                ));
            }
        }
        Command::PrepareBrowserMediaSuccessor {
            predecessor,
            lease_ms,
        } => {
            predecessor.validate()?;
            if request.project.as_ref() != Some(&predecessor.project_id)
                || request.expected_revision != Some(predecessor.revision)
                || request.request_id == predecessor.preparation_request_id
                || !(1..=BROWSER_MEDIA_MAX_LEASE_MS).contains(lease_ms)
            {
                return Err(ProtocolError::invalid("media successor requires its own request and unchanged predecessor project/revision"));
            }
        }
        Command::BrowserMediaPreparationStatus { .. } | Command::ReleaseBrowserMedia { .. }
            if request.expected_revision.is_some() =>
        {
            return Err(ProtocolError::invalid(
                "browser media history requires no revision",
            ));
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn test_binding() -> BrowserMediaBinding {
    BrowserMediaBinding {
        preparation_request_id: RequestId::new("media-prepare").unwrap(),
        lease_id: ArtifactId::new("media-lease").unwrap(),
        engine_epoch: AttemptId::new("boot").unwrap(),
        project_id: ProjectId::new("project").unwrap(),
        revision: RevisionId::new(2),
        selection: BrowserMediaSelection {
            source_version: SourceVersionId::new("source").unwrap(),
            source: ArtifactIdentity {
                artifact_id: ArtifactId::new("source-bytes").unwrap(),
                sha256: "a".repeat(64),
                byte_len: 100,
            },
            placement: SourcePlacement {
                placement_id: SourcePlacementId::new("placement").unwrap(),
                created_revision: RevisionId::new(1),
                spec: SourcePlacementSpec {
                    source_version: SourceVersionId::new("source").unwrap(),
                    source_start: SourceTimestamp::new(0, 1).unwrap(),
                    source_end: SourceTimestamp::new(10, 1).unwrap(),
                    source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                    project_anchor: ProjectTime::ZERO,
                    rate_numerator: 1,
                    rate_denominator: 1,
                },
            },
        },
        clock: DeclaredSourceClock {
            presentation_anchor: SourceTimestamp::new(5, 1).unwrap(),
            elapsed_anchor: SourceTimestamp::new(0, 1).unwrap(),
        },
        lease_ms: 1000,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ready() -> BrowserMediaPreparationReport {
        let b = test_binding();
        BrowserMediaPreparationReport {
            request_id: b.preparation_request_id.clone(),
            engine_epoch: b.engine_epoch.clone(),
            project_id: b.project_id.clone(),
            revision: b.revision,
            selection: b.selection.clone(),
            clock: b.clock,
            lease_ms: b.lease_ms,
            state: BrowserMediaPreparationState::Ready,
            verified_bytes: b.selection.source.byte_len,
            binding: Some(b),
            current: true,
            error: None,
        }
    }
    #[test]
    fn browser_media_selection_binding_and_catalog_refuse_ambiguous_metadata() {
        test_binding().validate().unwrap();
        for case in 0..8 {
            let mut b = test_binding();
            match case {
                0 => b.selection.source.byte_len = 0,
                1 => b.selection.source.byte_len = BROWSER_MEDIA_MAX_EXACT_INTEGER + 1,
                2 => b.selection.source_version = SourceVersionId::new("other").unwrap(),
                3 => b.selection.placement.created_revision = RevisionId::new(3),
                4 => b.selection.placement.spec.rate_numerator = -1,
                5 => b.lease_ms = 600_001,
                6 => b.clock.presentation_anchor = SourceTimestamp::new(i64::MAX, 1).unwrap(),
                _ => b.clock.elapsed_anchor = SourceTimestamp::new(6, 1).unwrap(),
            }
            assert!(b.validate().is_err(), "binding {case}");
        }
        let b = test_binding();
        let mut catalog = BrowserMediaCatalog {
            engine_epoch: b.engine_epoch,
            project_id: b.project_id,
            revision: b.revision,
            entries: vec![BrowserMediaCatalogEntry {
                label: "Imported clip".into(),
                selection: b.selection,
            }],
        };
        catalog.validate().unwrap();
        catalog.entries.push(catalog.entries[0].clone());
        assert!(catalog.validate().is_err());
        catalog.entries = vec![catalog.entries[0].clone(); 257];
        assert!(catalog.validate().is_err());
        catalog.entries.clear();
        catalog.validate().unwrap();
    }
    #[test]
    fn browser_media_ready_reports_match_every_original_field_and_terminal_errors_are_static() {
        ready().validate().unwrap();
        for case in 0..10 {
            let mut r = ready();
            match case {
                0 => r.request_id = RequestId::new("other").unwrap(),
                1 => r.engine_epoch = AttemptId::new("other").unwrap(),
                2 => r.project_id = ProjectId::new("other").unwrap(),
                3 => r.revision = RevisionId::new(3),
                4 => r.selection.source.sha256 = "b".repeat(64),
                5 => r.clock.presentation_anchor = SourceTimestamp::new(6, 1).unwrap(),
                6 => r.lease_ms += 1,
                7 => r.verified_bytes -= 1,
                8 => r.binding = None,
                _ => r.error = Some("/private/path".into()),
            }
            assert!(r.validate().is_err(), "report {case}");
        }
        let mut r = ready();
        r.current = false;
        r.validate().unwrap();
        for state in [
            BrowserMediaPreparationState::Pending,
            BrowserMediaPreparationState::Cancelled,
            BrowserMediaPreparationState::Failed,
        ] {
            let mut r = ready();
            r.state = state;
            r.current = false;
            assert!(r.validate().is_err());
            r.binding = None;
            r.verified_bytes = 1;
            r.error = (state == BrowserMediaPreparationState::Failed)
                .then(|| BROWSER_MEDIA_PREPARATION_ERROR.into());
            r.validate().unwrap();
            r.current = true;
            assert_eq!(
                r.validate().is_ok(),
                state == BrowserMediaPreparationState::Pending
            );
        }
    }
    #[test]
    fn browser_media_commands_require_scope_revision_and_explicit_declared_clock() {
        let b = test_binding();
        let commands = [
            Command::BrowserMediaCatalog,
            Command::PrepareBrowserMedia {
                engine_epoch: b.engine_epoch.clone(),
                selection: b.selection.clone(),
                clock: b.clock,
                lease_ms: b.lease_ms,
                allow_declared_clock: true,
            },
            Command::BrowserMediaPreparationStatus {
                original_request_id: b.preparation_request_id.clone(),
            },
            Command::ReleaseBrowserMedia {
                original_request_id: b.preparation_request_id.clone(),
            },
        ];
        for (i, command) in commands.into_iter().enumerate() {
            assert!(command.requires_project());
            assert_eq!(command.requires_revision(), i < 2);
            assert_eq!(command.is_mutating(), i == 1 || i == 3);
            let r = Request::new(RequestId::new("operation").unwrap(), command)
                .in_project(b.project_id.clone(), (i < 2).then_some(b.revision));
            r.validate().unwrap();
            let mut bad = r.clone();
            bad.project = None;
            assert!(bad.validate().is_err());
            let mut bad = r;
            bad.expected_revision = if i < 2 { None } else { Some(b.revision) };
            assert!(bad.validate().is_err());
        }
        let mut r = Request::new(
            RequestId::new("prepare").unwrap(),
            Command::PrepareBrowserMedia {
                engine_epoch: b.engine_epoch,
                selection: b.selection,
                clock: b.clock,
                lease_ms: b.lease_ms,
                allow_declared_clock: false,
            },
        )
        .in_project(b.project_id, Some(b.revision));
        assert_eq!(r.validate().unwrap_err().code, ErrorCode::Forbidden);
        if let Command::PrepareBrowserMedia {
            allow_declared_clock,
            ..
        } = &mut r.command
        {
            *allow_declared_clock = true;
        }
        r.validate().unwrap();
        let wire =
            serde_json::to_value(ResponseBody::BrowserMediaPreparation(Box::new(ready()))).unwrap();
        assert_eq!(wire["kind"], "browser_media_preparation");
        let mut invented = serde_json::to_value(test_binding()).unwrap();
        invented["synchronized"] = true.into();
        assert!(serde_json::from_value::<BrowserMediaBinding>(invented).is_err());
    }
    #[test]
    fn browser_media_capture_requires_exact_declared_anchor_and_preserves_legacy_omission() {
        let capture: LiveCaptureReport = serde_json::from_str(include_str!(
            "../tests/fixtures/live-cpu-capture-report.json"
        ))
        .unwrap();
        let mut options = capture.options;
        options.validate().unwrap();
        assert!(serde_json::to_value(&options)
            .unwrap()
            .get("browser_media")
            .is_none());
        let b = test_binding();
        options.browser_media = Some(b.clone());
        options.source_time_basis = LiveSourceTimeBasis::SourceMediaPresentation;
        options.source_anchor = SourceTimestamp::new(21, 4).unwrap();
        let exact = b
            .clock
            .project_at(options.source_anchor, &b.selection.placement.spec)
            .unwrap();
        options.project_anchor = exact.quantize_nanoseconds().unwrap().time;
        options.exact_project_anchor = Some(exact);
        options.validate().unwrap();
        assert_eq!(options.project_anchor, ProjectTime::from_nanos(250_000_000));
        for case in 0..7 {
            let mut bad = options.clone();
            match case {
                0 => bad.source_time_basis = LiveSourceTimeBasis::DisplayCapturePresentation,
                1 => bad.exact_project_anchor = None,
                2 => bad.project_anchor = ProjectTime::ZERO,
                3 => bad.source_anchor = SourceTimestamp::new(15, 1).unwrap(),
                4 => bad.source_anchor = SourceTimestamp::new(4, 1).unwrap(),
                5 => {
                    bad.browser_media
                        .as_mut()
                        .unwrap()
                        .selection
                        .placement
                        .spec
                        .rate_numerator = 2
                }
                _ => {
                    bad.encoded_source = Some(LiveEncodedSourceOptions {
                        format: LiveEncodedFormat::Matroska,
                        maximum_input_bytes: 100,
                        maximum_frames: 1,
                    })
                }
            }
            assert!(bad.validate().is_err(), "capture {case}");
        }
        let raw = serde_json::to_value(&options).unwrap();
        let restored: LiveCaptureOptions = serde_json::from_value(raw).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored.browser_media, Some(b));
    }
    #[test]
    fn browser_media_capture_report_and_continuation_keep_the_imported_subject_and_exact_anchor() {
        let mut r: LiveCaptureReport = serde_json::from_str(include_str!(
            "../tests/fixtures/live-cpu-capture-report.json"
        ))
        .unwrap();
        let mut b = test_binding();
        b.engine_epoch = r.engine_epoch.clone();
        b.revision = r.identity.revision;
        b.clock.presentation_anchor = SourceTimestamp::new(0, 1).unwrap();
        r.options.browser_media = Some(b.clone());
        r.options.source_time_basis = LiveSourceTimeBasis::SourceMediaPresentation;
        r.options.exact_project_anchor = Some(ExactSeconds::new(0, 1).unwrap());
        r.clock = r.options.clock(r.identity.epoch.clone());
        let LiveInferenceReply::Estimated { input, .. } = &mut r
            .inference
            .as_mut()
            .unwrap()
            .latest
            .as_mut()
            .unwrap()
            .result
        else {
            panic!("CPU fixture")
        };
        input.pair.presentation_clock = Some(r.clock.clone());
        r.validate().unwrap();
        let continuation = r
            .continuation_options(SourceTimestamp::new(1, 4).unwrap(), 1000)
            .unwrap();
        assert_eq!(continuation.browser_media, Some(b));
        assert_eq!(
            continuation.exact_project_anchor,
            Some(ExactSeconds::new(1, 4).unwrap())
        );
        for case in 0..4 {
            let mut bad = r.clone();
            let b = bad.options.browser_media.as_mut().unwrap();
            match case {
                0 => b.engine_epoch = AttemptId::new("foreign").unwrap(),
                1 => b.project_id = ProjectId::new("foreign").unwrap(),
                2 => b.revision = RevisionId::new(4),
                _ => {
                    b.selection.source_version = SourceVersionId::new("foreign").unwrap();
                    b.selection.placement.spec.source_version = b.selection.source_version.clone();
                }
            }
            assert!(bad.validate().is_err());
        }
    }
    #[test]
    fn browser_media_successor_requires_original_scope_and_distinct_request_without_changing_binding_wire_shape(
    ) {
        let b = test_binding();
        let r = Request::new(
            RequestId::new("media-successor").unwrap(),
            Command::PrepareBrowserMediaSuccessor {
                predecessor: b.clone(),
                lease_ms: 1000,
            },
        )
        .in_project(b.project_id.clone(), Some(b.revision));
        r.validate().unwrap();
        assert!(r.command.is_mutating());
        let roundtrip: Request = serde_json::from_slice(&serde_json::to_vec(&r).unwrap()).unwrap();
        roundtrip.validate().unwrap();
        for case in 0..5 {
            let mut bad = r.clone();
            match case {
                0 => bad.request_id = b.preparation_request_id.clone(),
                1 => bad.project = Some(ProjectId::new("foreign").unwrap()),
                2 => bad.expected_revision = Some(RevisionId::new(b.revision.value() + 1)),
                3 => {
                    if let Command::PrepareBrowserMediaSuccessor { lease_ms, .. } = &mut bad.command
                    {
                        *lease_ms = 0;
                    }
                }
                _ => {
                    if let Command::PrepareBrowserMediaSuccessor { lease_ms, .. } = &mut bad.command
                    {
                        *lease_ms = BROWSER_MEDIA_MAX_LEASE_MS + 1;
                    }
                }
            }
            assert!(bad.validate().is_err());
        }
        let value = serde_json::to_value(&b).unwrap();
        assert!(value.get("predecessor").is_none());
        assert!(value.get("successor").is_none());
        assert_eq!(
            serde_json::from_value::<BrowserMediaBinding>(value).unwrap(),
            b
        );
    }
}
