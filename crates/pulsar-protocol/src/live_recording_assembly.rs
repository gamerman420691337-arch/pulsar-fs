//! Checked conversion of complete recording pages. This grants no authority;
//! Engine callers must obtain pages from their own actor-bound registry.
use crate::*;
use std::collections::HashSet;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveRecordingSelection {
    pub epoch: AttemptId,
    pub recording_request_id: RequestId,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveRecordingStageOptions {
    pub engine_epoch: AttemptId,
    pub recordings: Vec<LiveRecordingSelection>,
    pub range: TimeRange,
    pub maximum_interpolation_ns: u64,
}
impl LiveRecordingStageOptions {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let mut seen = HashSet::new();
        if self.recordings.is_empty()
            || self.recordings.len() > 64
            || self.range.start() < ProjectTime::ZERO
            || !(1..=1_000_000_000).contains(&self.maximum_interpolation_ns)
            || self
                .recordings
                .iter()
                .any(|r| !seen.insert(r.epoch.clone()))
        {
            return Err(ProtocolError::invalid(
                "invalid recording stage bounds or repeated scope",
            ));
        }
        Ok(())
    }
}

pub struct LiveRecordingAssembly {
    pub program: MotionProgram,
    /// Inferred actions never erase the original unfinished CUDA provenance.
    pub contains_cuda_provisional: bool,
    pub recording_requests: Vec<RequestId>,
}

pub fn assemble_live_recordings(
    pages: &[LiveRecordingPage],
    range: TimeRange,
    maximum_interpolation_ns: u64,
) -> Result<LiveRecordingAssembly, ProtocolError> {
    if pages.is_empty() || pages.len() > 256 {
        return Err(ProtocolError::invalid(
            "assembly requires 1..=256 complete recording pages",
        ));
    }
    let root = &pages[0];
    let mut seen = HashSet::new();
    let mut header = None;
    let mut expected_after = 0;
    let mut scope = None;
    let mut previous: Option<LiveInferenceInput> = None;
    let mut points = Vec::new();
    let mut requests = Vec::new();
    let mut provisional = false;
    for page in pages {
        page.validate()?;
        if page.active || page.discarded {
            return Err(ProtocolError::invalid(
                "assemble stopped, retained recordings only",
            ));
        }
        if !same_cpu_mode(page.cpu_manifest.as_deref(), root.cpu_manifest.as_deref())?
            || page.model_pack != root.model_pack
            || page.browser_media != root.browser_media
            || page.source_time_basis != root.source_time_basis
            || page.engine_epoch != root.engine_epoch
            || page.identity.project != root.identity.project
            || page.identity.revision != root.identity.revision
            || page.identity.source != root.identity.source
            || serde_json::to_value(&page.profile).map_err(invalid_json)?
                != serde_json::to_value(&root.profile).map_err(invalid_json)?
        {
            return Err(ProtocolError::invalid(
                "assembly boot/project/source/profile or captured CPU mode differs",
            ));
        }
        let new_scope = scope.as_ref() != Some(&page.identity.epoch);
        if new_scope {
            if expected_after != 0 || !seen.insert(page.identity.epoch.clone()) || seen.len() > 64 {
                return Err(ProtocolError::invalid(
                    "incomplete, repeated or excessive recording scopes",
                ));
            }
            scope = Some(page.identity.epoch.clone());
            header = None;
            previous = None;
            requests.push(page.recording_request_id.clone());
        }
        if page.after != expected_after {
            return Err(ProtocolError::invalid(
                "recording pages are missing or out of order",
            ));
        }
        let mut current_header = serde_json::to_value(page).map_err(invalid_json)?;
        let object = current_header.as_object_mut().expect("recording struct");
        for key in ["after", "next", "samples"] {
            object.remove(key);
        }
        if header.as_ref().is_some_and(|h| h != &current_header) {
            return Err(ProtocolError::invalid(
                "recording metadata changed between pages",
            ));
        }
        header = Some(current_header);
        for sample in &page.samples {
            let (input, position) = match &sample.result {
                LiveInferenceReply::Estimated {
                    input, position, ..
                }
                | LiveInferenceReply::CapturedCpuEstimated {
                    input, position, ..
                } => (input, *position),
                LiveInferenceReply::CudaProvisional {
                    input, position, ..
                } => {
                    provisional = true;
                    (input, *position)
                }
                _ => return Err(ProtocolError::invalid("recording has no position result")),
            };
            if previous
                .as_ref()
                .is_some_and(|p| p.sequence >= input.sequence)
            {
                return Err(ProtocolError::invalid(
                    "recording sequence regresses across pages",
                ));
            }
            let continuous = previous.as_ref().is_some_and(|p| {
                p.pair.current_frame == input.pair.previous_frame
                    && p.pair.current_time == input.pair.previous_time
                    && p.current == input.previous
            });
            let time = page
                .clock
                .project_at_source(input.pair.current_time)
                .and_then(|t| t.quantize_nanoseconds())
                .map_err(|e| ProtocolError::invalid(e.to_string()))?
                .time;
            points.push(LiveMotionPoint {
                time,
                position,
                continuous_from_previous: continuous,
            });
            if points.len() > 4096 {
                return Err(ProtocolError::invalid(
                    "assembly exceeds 4096 accepted points",
                ));
            }
            previous = Some(input.clone());
        }
        expected_after = page.next.unwrap_or(0);
        // An exhausted cursor is terminal even when the same scope follows.
        if page.next.is_none() {
            scope = None;
        }
    }
    if expected_after != 0 {
        return Err(ProtocolError::invalid("final recording page is missing"));
    }
    let program = assemble_live_motion(&points, range, maximum_interpolation_ns)
        .map_err(|e| ProtocolError::invalid(e.to_string()))?;
    Ok(LiveRecordingAssembly {
        program,
        contains_cuda_provisional: provisional,
        recording_requests: requests,
    })
}

fn same_cpu_mode(
    page: Option<&LiveCpuInferenceManifest>,
    root: Option<&LiveCpuInferenceManifest>,
) -> Result<bool, ProtocolError> {
    match (page, root) {
        (None, None) => Ok(true),
        (Some(page), Some(root)) => Ok(page.image.identity()? == root.image.identity()?
            && page.image.owner_policy == root.image.owner_policy
            && page.stream.worker == root.stream.worker
            && serde_json::to_value(&page.stream.profile).map_err(invalid_json)?
                == serde_json::to_value(&root.stream.profile).map_err(invalid_json)?),
        _ => Ok(false),
    }
}

fn invalid_json(error: serde_json::Error) -> ProtocolError {
    ProtocolError::invalid(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bind_page(page: &mut LiveRecordingPage) {
        let mut binding = crate::browser_media::test_binding();
        binding.engine_epoch = page.engine_epoch.clone();
        binding.project_id = page.identity.project.clone();
        binding.revision = page.identity.revision;
        binding.selection.source_version = page.identity.source.clone();
        binding.selection.placement.spec.source_version = page.identity.source.clone();
        binding.clock.presentation_anchor = page.clock.source_anchor;
        binding.clock.elapsed_anchor = SourceTimestamp::new(0, 1).unwrap();
        binding.selection.placement.spec.project_anchor = page.clock.project_anchor;
        page.source_time_basis = LiveSourceTimeBasis::SourceMediaPresentation;
        page.clock.exact_project_anchor =
            Some(ExactSeconds::from_project(page.clock.project_anchor));
        for sample in &mut page.samples {
            let LiveInferenceReply::Estimated { input, .. } = &mut sample.result else {
                panic!("CPU fixture")
            };
            input.pair.presentation_clock = Some(page.clock.clone());
        }
        page.browser_media = Some(binding);
        page.validate().unwrap();
    }
    #[test]
    fn bound_recordings_preserve_exact_source_and_refuse_mixed_pages_and_continuations() {
        let mut first = page();
        bind_page(&mut first);
        first.total_samples = 2;
        first.next = Some(1);
        let second = second(&first);
        assert!(
            assemble_live_recordings(&[first.clone(), second.clone()], range(), 100_000_000)
                .is_ok()
        );
        for mode in 0..2 {
            let mut bad = second.clone();
            if mode == 0 {
                bad.browser_media = None;
            } else {
                bad.browser_media.as_mut().unwrap().lease_id =
                    ArtifactId::new("other-lease").unwrap();
            }
            bad.validate().unwrap();
            assert!(assemble_live_recordings(&[first.clone(), bad], range(), 100_000_000).is_err());
        }
        first.total_samples = 1;
        first.next = None;
        let mut next = second;
        next.after = 0;
        next.total_samples = 1;
        next.identity.epoch = AttemptId::new("next-bound-scope").unwrap();
        next.clock.epoch = next.identity.epoch.clone();
        let LiveInferenceReply::Estimated {
            identity, input, ..
        } = &mut next.samples[0].result
        else {
            panic!("CPU fixture")
        };
        *identity = next.identity.clone();
        input.pair.source_epoch = next.identity.epoch.clone();
        input.pair.presentation_clock = Some(next.clock.clone());
        assert!(
            assemble_live_recordings(&[first.clone(), next.clone()], range(), 100_000_000).is_ok()
        );
        next.browser_media = None;
        next.validate().unwrap();
        assert!(assemble_live_recordings(&[first, next], range(), 100_000_000).is_err());
    }
    #[test]
    fn bound_recording_page_checks_clock_identity_and_both_recorded_frame_times() {
        let mut p = page();
        bind_page(&mut p);
        for case in 0..5 {
            let mut bad = p.clone();
            match case {
                0 => {
                    bad.browser_media.as_mut().unwrap().project_id =
                        ProjectId::new("foreign").unwrap()
                }
                1 => bad.browser_media.as_mut().unwrap().revision = RevisionId::new(4),
                2 => {
                    bad.browser_media.as_mut().unwrap().engine_epoch =
                        AttemptId::new("foreign").unwrap()
                }
                3 => bad.clock.exact_project_anchor = None,
                _ => {
                    bad.browser_media
                        .as_mut()
                        .unwrap()
                        .selection
                        .placement
                        .spec
                        .source_end = SourceTimestamp::new(1, 10).unwrap()
                }
            }
            assert!(bad.validate().is_err(), "recording {case}");
        }
        let mut bad = p;
        let LiveInferenceReply::Estimated { input, .. } = &mut bad.samples[0].result else {
            panic!("CPU fixture")
        };
        input.pair.previous_time = SourceTimestamp::new(-1, 10).unwrap();
        assert!(bad.validate().is_err());
        assert!(serde_json::to_value(page())
            .unwrap()
            .get("browser_media")
            .is_none());
    }
    #[test]
    fn clock_basis_is_preserved_and_mixed_domains_refuse_assembly() {
        let mut first = page();
        first.source_time_basis = LiveSourceTimeBasis::DisplayCapturePresentation;
        first.total_samples = 2;
        first.next = Some(1);
        let mut second = second(&first);
        second.source_time_basis = first.source_time_basis;
        assert!(
            assemble_live_recordings(&[first.clone(), second.clone()], range(), 100_000_000)
                .is_ok()
        );
        second.model_pack = Some(crate::ModelPackChoice {
            manifest_sha256: "a".repeat(64),
            selection_revision: 1,
        });
        assert!(
            assemble_live_recordings(&[first.clone(), second.clone()], range(), 100_000_000)
                .is_err()
        );
        second.model_pack = None;
        for basis in [
            LiveSourceTimeBasis::SourceMediaPresentation,
            LiveSourceTimeBasis::CameraCapturePresentation,
        ] {
            second.source_time_basis = basis;
            assert!(assemble_live_recordings(
                &[first.clone(), second.clone()],
                range(),
                100_000_000
            )
            .is_err());
        }
        first.source_time_basis = LiveSourceTimeBasis::CameraCapturePresentation;
        second.source_time_basis = first.source_time_basis;
        assert!(
            assemble_live_recordings(&[first.clone(), second.clone()], range(), 100_000_000)
                .is_ok()
        );
        first.source_time_basis = LiveSourceTimeBasis::DisplayCapturePresentation;
        let raw = serde_json::to_value(&first).unwrap();
        assert_eq!(raw["source_time_basis"], "display_capture_presentation");
        let mut legacy = raw;
        legacy.as_object_mut().unwrap().remove("source_time_basis");
        let legacy: LiveRecordingPage = serde_json::from_value(legacy).unwrap();
        assert_eq!(
            legacy.source_time_basis,
            LiveSourceTimeBasis::ProducerDeclared
        );
        assert!(serde_json::to_value(legacy)
            .unwrap()
            .get("source_time_basis")
            .is_none());
    }
    fn page() -> LiveRecordingPage {
        let c: LiveCaptureReport = serde_json::from_str(include_str!(
            "../tests/fixtures/live-cpu-capture-report.json"
        ))
        .unwrap();
        let mut s = c.inference.as_ref().unwrap().latest.clone().unwrap();
        s.capture_age_ns = s.capture_to_analysis_ns;
        LiveRecordingPage {
            cpu_manifest: None,
            browser_media: None,
            model_pack: None,
            source_time_basis: LiveSourceTimeBasis::ProducerDeclared,
            schema: 1,
            engine_epoch: c.engine_epoch,
            identity: c.identity,
            clock: c.clock,
            profile: c.options.inference.unwrap().profile,
            inference_attempt: c.inference.unwrap().attempt_id,
            recording_request_id: RequestId::new("assembly-recording").unwrap(),
            maximum_samples: 2,
            total_samples: 1,
            discarded: false,
            exhausted: false,
            active: false,
            retirement: Some(LiveCaptureRetirement::Stopped),
            after: 0,
            next: None,
            samples: vec![s],
            qualified: false,
        }
    }
    fn range() -> TimeRange {
        TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(10_000_000_000)).unwrap()
    }
    #[test]
    fn staging_options_are_revision_bound_mutating_and_bounded() {
        let p = page();
        let good = LiveRecordingStageOptions {
            engine_epoch: p.engine_epoch,
            recordings: vec![LiveRecordingSelection {
                epoch: p.identity.epoch,
                recording_request_id: p.recording_request_id,
            }],
            range: range(),
            maximum_interpolation_ns: 100_000_000,
        };
        good.validate().unwrap();
        let command = Command::StageLiveRecording {
            options: good.clone(),
        };
        assert!(command.is_mutating() && command.requires_revision());
        let mut bad = good.clone();
        bad.recordings.push(bad.recordings[0].clone());
        assert!(bad.validate().is_err());
        let mut bad = good.clone();
        bad.maximum_interpolation_ns = 0;
        assert!(bad.validate().is_err());
        let mut bad = good;
        bad.recordings.clear();
        assert!(bad.validate().is_err());
    }
    fn second(first: &LiveRecordingPage) -> LiveRecordingPage {
        let mut p = first.clone();
        p.after = 1;
        p.next = None;
        let LiveInferenceReply::Estimated { input, .. } = &mut p.samples[0].result else {
            panic!("CPU fixture")
        };
        input.sequence = 2;
        input.previous = input.current.clone();
        input.pair.previous_frame = FrameId::new(1);
        input.pair.current_frame = FrameId::new(2);
        input.pair.previous_time = SourceTimestamp::new(1, 10).unwrap();
        input.pair.current_time = SourceTimestamp::new(2, 10).unwrap();
        p
    }
    #[test]
    fn paging_preserves_checked_edges_and_refuses_mutation_and_regression() {
        let mut first = page();
        first.total_samples = 2;
        first.next = Some(1);
        let second = second(&first);
        let a = assemble_live_recordings(&[first.clone(), second.clone()], range(), 100_000_000)
            .unwrap();
        assert_eq!(a.program.tracks()[0].actions().len(), 2);
        assert_eq!(a.program.tracks()[0].gaps().len(), 2); // Leading and trailing only.
        let mut bad = second.clone();
        bad.exhausted = true;
        assert!(assemble_live_recordings(&[first.clone(), bad], range(), 100_000_000).is_err());
        let mut bad = second.clone();
        let LiveInferenceReply::Estimated { input, .. } = &mut bad.samples[0].result else {
            panic!("fixture")
        };
        input.sequence = 1;
        assert!(assemble_live_recordings(&[first.clone(), bad], range(), 100_000_000).is_err());
        let mut disconnected = second;
        let LiveInferenceReply::Estimated { input, .. } = &mut disconnected.samples[0].result
        else {
            panic!("fixture")
        };
        let LiveInferenceReply::Estimated {
            input: original, ..
        } = &first.samples[0].result
        else {
            panic!("fixture")
        };
        input.previous = original.previous.clone();
        let a = assemble_live_recordings(&[first, disconnected], range(), 100_000_000).unwrap();
        assert_eq!(a.program.tracks()[0].gaps().len(), 3);
    }
    #[test]
    fn handoff_never_bridges_a_scope_boundary_and_foreign_source_refuses() {
        let first = page();
        let mut next = second(&first);
        next.after = 0;
        next.identity.epoch = AttemptId::new("successor").unwrap();
        next.clock.epoch = next.identity.epoch.clone();
        let LiveInferenceReply::Estimated {
            identity, input, ..
        } = &mut next.samples[0].result
        else {
            panic!("fixture")
        };
        *identity = next.identity.clone();
        input.pair.source_epoch = next.identity.epoch.clone();
        input.pair.presentation_clock = Some(next.clock.clone());
        let a =
            assemble_live_recordings(&[first.clone(), next.clone()], range(), 100_000_000).unwrap();
        assert_eq!(a.program.tracks()[0].gaps().len(), 3);
        next.identity.source = SourceVersionId::new("foreign-source").unwrap();
        let LiveInferenceReply::Estimated { identity, .. } = &mut next.samples[0].result else {
            panic!("fixture")
        };
        *identity = next.identity.clone();
        assert!(assemble_live_recordings(&[first, next], range(), 100_000_000).is_err());
    }
    #[test]
    fn stopped_recording_preserves_inference_and_declared_unavailable_coverage() {
        let page = page();
        let a = assemble_live_recordings(&[page], range(), 100_000_000).unwrap();
        assert!(!a.contains_cuda_provisional);
        assert_eq!(a.program.tracks()[0].actions().len(), 1);
        assert_eq!(
            a.program.tracks()[0].actions()[0].evidence(),
            EvidenceKind::Inferred
        );
        assert!(!a.program.tracks()[0].gaps().is_empty());
    }
    #[test]
    fn empty_stopped_scope_retains_identity_and_unavailable_coverage_without_zero_motion() {
        let first = page();
        let mut empty = first.clone();
        empty.identity.epoch = AttemptId::new("empty-successor").unwrap();
        empty.clock.epoch = empty.identity.epoch.clone();
        empty.recording_request_id = RequestId::new("empty-recording").unwrap();
        empty.total_samples = 0;
        empty.samples.clear();
        empty.after = 0;
        empty.next = None;
        empty.validate().unwrap();
        let original =
            assemble_live_recordings(std::slice::from_ref(&first), range(), 100_000_000).unwrap();
        let assembled =
            assemble_live_recordings(&[first, empty.clone()], range(), 100_000_000).unwrap();
        assert_eq!(assembled.program, original.program);
        assert_eq!(assembled.recording_requests.len(), 2);
        assert_eq!(assembled.recording_requests[1], empty.recording_request_id);
        assert!(
            assemble_live_recordings(std::slice::from_ref(&empty), range(), 100_000_000).is_err()
        );
        empty.active = true;
        assert!(assemble_live_recordings(&[page(), empty], range(), 100_000_000).is_err());
    }
    #[test]
    fn exact_anchor_quantizes_once_and_cuda_remains_provisional() {
        use sha2::{Digest, Sha256};
        let mut p = page();
        p.clock.project_anchor = ProjectTime::from_nanos(333_333_333);
        p.clock.exact_project_anchor = Some(ExactSeconds::new(1, 3).unwrap());
        p.profile.provider = InferenceProvider::Cuda;
        let hash = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&p.profile).unwrap())
        );
        let LiveInferenceReply::Estimated {
            attempt_id,
            identity,
            mut input,
            position,
            pair_analysis_elapsed_ns,
            ..
        } = p.samples[0].result.clone()
        else {
            panic!("fixture")
        };
        input.pair.presentation_clock = Some(p.clock.clone());
        p.samples[0].result = LiveInferenceReply::CudaProvisional {
            attempt_id,
            identity,
            profile_sha256: hash,
            manifest_sha256: "a".repeat(64),
            input,
            position,
            pair_analysis_elapsed_ns,
        };
        let a = assemble_live_recordings(&[p], range(), 100_000_000).unwrap();
        assert!(a.contains_cuda_provisional);
        assert_eq!(
            a.program.tracks()[0].actions()[0].time().as_nanos(),
            433_333_333
        );
        assert_eq!(
            a.program.tracks()[0].actions()[0].evidence(),
            EvidenceKind::Inferred
        );
    }
    #[test]
    fn incomplete_active_duplicate_and_foreign_recordings_refuse() {
        let good = page();
        let mut bad = good.clone();
        bad.active = true;
        bad.retirement = None;
        assert!(assemble_live_recordings(&[bad], range(), 100).is_err());
        let mut bad = good.clone();
        bad.total_samples = 2;
        bad.next = Some(1);
        assert!(assemble_live_recordings(&[bad], range(), 100).is_err());
        assert!(assemble_live_recordings(&[good.clone(), good.clone()], range(), 100).is_err());
        let mut bad = good.clone();
        bad.after = 1;
        assert!(assemble_live_recordings(&[bad], range(), 100).is_err());
        let mut bad = good.clone();
        bad.qualified = true;
        assert!(assemble_live_recordings(&[bad], range(), 100).is_err());
        assert!(assemble_live_recordings(&[], range(), 100).is_err());
    }
    fn captured(page: &mut LiveRecordingPage) {
        crate::live_capture::captured_cpu_report_tests::bind_page(page);
    }
    fn refresh_manifest(page: &mut LiveRecordingPage) {
        let previous = page.cpu_manifest.as_ref().unwrap();
        let mut stream = previous.stream.clone();
        stream.identity = page.identity.clone();
        stream.clock = page.clock.clone();
        stream.attempt_id = page.inference_attempt.clone();
        stream.profile = page.profile.clone();
        let manifest = LiveCpuInferenceManifest::new(stream, previous.image.clone()).unwrap();
        for sample in &mut page.samples {
            let LiveInferenceReply::CapturedCpuEstimated { input, .. } = &mut sample.result else {
                panic!("captured fixture");
            };
            input.pair.source_epoch = page.identity.epoch.clone();
            input.pair.presentation_clock = Some(page.clock.clone());
            crate::live_capture::captured_cpu_report_tests::bind_result(
                &mut sample.result,
                &manifest,
            );
        }
        page.cpu_manifest = Some(Box::new(manifest));
        page.validate().unwrap();
    }
    #[test]
    fn captured_pages_assemble_without_promoting_provisional_or_qualified_motion() {
        let mut first = page();
        first.total_samples = 2;
        first.next = Some(1);
        let mut next = second(&first);
        captured(&mut first);
        captured(&mut next);
        let assembled = assemble_live_recordings(&[first, next], range(), 100_000_000).unwrap();
        assert!(!assembled.contains_cuda_provisional);
        assert_eq!(assembled.program.tracks()[0].actions().len(), 2);
    }
    #[test]
    fn assembly_refuses_host_captured_substitution_between_pages() {
        let mut first = page();
        first.total_samples = 2;
        first.next = Some(1);
        let next = second(&first);
        captured(&mut first);
        next.validate().unwrap();
        assert!(assemble_live_recordings(&[first, next], range(), 100_000_000).is_err());
    }
    #[test]
    fn same_epoch_pages_require_full_manifest_not_just_same_image() {
        let mut first = page();
        first.total_samples = 2;
        first.next = Some(1);
        let mut next = second(&first);
        captured(&mut first);
        captured(&mut next);
        let manifest = next.cpu_manifest.as_ref().unwrap();
        let mut stream = manifest.stream.clone();
        stream.budget.wall_time_ms -= 1;
        let modified = LiveCpuInferenceManifest::new(stream, manifest.image.clone()).unwrap();
        for sample in &mut next.samples {
            crate::live_capture::captured_cpu_report_tests::bind_result(
                &mut sample.result,
                &modified,
            );
        }
        next.cpu_manifest = Some(Box::new(modified));
        next.validate().unwrap();
        assert!(assemble_live_recordings(&[first, next], range(), 100_000_000).is_err());
    }
    fn next_epoch() -> (LiveRecordingPage, LiveRecordingPage) {
        let mut first = page();
        let mut next = second(&first);
        next.after = 0;
        next.next = None;
        next.total_samples = 1;
        captured(&mut first);
        captured(&mut next);
        next.identity.epoch = AttemptId::new("second-captured-epoch").unwrap();
        next.clock.epoch = next.identity.epoch.clone();
        next.inference_attempt = AttemptId::new("second-captured-attempt").unwrap();
        next.recording_request_id = RequestId::new("second-recording").unwrap();
        refresh_manifest(&mut next);
        (first, next)
    }
    #[test]
    fn new_epoch_may_change_manifest_scope_while_preserving_runtime_image_and_worker() {
        let (first, next) = next_epoch();
        assert_ne!(
            first
                .cpu_manifest
                .as_ref()
                .unwrap()
                .identity_sha256()
                .unwrap(),
            next.cpu_manifest
                .as_ref()
                .unwrap()
                .identity_sha256()
                .unwrap()
        );
        let assembled = assemble_live_recordings(&[first, next], range(), 100_000_000).unwrap();
        assert_eq!(assembled.recording_requests.len(), 2);
        assert_eq!(assembled.program.tracks()[0].gaps().len(), 3); // Epoch boundary is never interpolated.
    }
    #[test]
    fn cross_epoch_runtime_owner_worker_and_mode_substitution_are_rejected() {
        for case in 0..5 {
            let (first, mut next) = next_epoch();
            if case == 0 {
                let sample = &mut next.samples[0].result;
                let LiveInferenceReply::CapturedCpuEstimated {
                    attempt_id,
                    identity,
                    profile_sha256,
                    input,
                    position,
                    pair_analysis_elapsed_ns,
                    ..
                } = sample.clone()
                else {
                    panic!("fixture");
                };
                *sample = LiveInferenceReply::Estimated {
                    attempt_id,
                    identity,
                    profile_sha256,
                    input,
                    position,
                    pair_analysis_elapsed_ns,
                };
                next.cpu_manifest = None;
            } else {
                let manifest = next.cpu_manifest.as_ref().unwrap();
                let mut stream = manifest.stream.clone();
                let mut image = manifest.image.clone();
                let identity = ArtifactIdentity {
                    artifact_id: ArtifactId::new("f".repeat(64)).unwrap(),
                    sha256: "f".repeat(64),
                    byte_len: 11,
                };
                match case {
                    1 => image.owner_policy = identity,
                    2 => stream.worker = identity,
                    4 => {
                        next.profile.minimum_memory_bytes += 1;
                        stream.profile = next.profile.clone();
                    }
                    _ => {
                        let library = image
                            .libraries
                            .iter_mut()
                            .find(|l| !l.names.iter().any(|n| n == "libonnxruntime.so.1"))
                            .unwrap();
                        library.identity = identity;
                        image
                            .libraries
                            .sort_by(|a, b| a.identity.sha256.cmp(&b.identity.sha256));
                    }
                }
                let modified = LiveCpuInferenceManifest::new(stream, image).unwrap();
                for sample in &mut next.samples {
                    crate::live_capture::captured_cpu_report_tests::bind_result(
                        &mut sample.result,
                        &modified,
                    );
                }
                next.cpu_manifest = Some(Box::new(modified));
            }
            next.validate().unwrap();
            assert!(
                assemble_live_recordings(&[first, next], range(), 100_000_000).is_err(),
                "cross-epoch mode {case}"
            );
        }
    }
}
