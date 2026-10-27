//! Bounded effect adapter for explicit synthetic and audio-derived candidates.
//! No prompt is a shell command; no modality can bypass immutable input checks.

use super::*;

const AUDIO_SAMPLE_RATE: u32 = 16_000;
const AUDIO_WINDOW_SAMPLES: u32 = 160;

pub(super) fn generate(
    request: &WorkerRequest,
    input: &GenerationInput,
    deadline: Instant,
) -> Result<WorkerOutput> {
    input.validate()?;
    let mut notes = Vec::new();
    let (program, recipe, details) = match input {
        GenerationInput::Text { prompt } => {
            check_declared_source(request, input)?;
            let spec = parse_pattern_prompt(prompt)?;
            notes.push("constrained_pattern_parser_not_local_ai");
            (
                synthesize_pattern(&spec)?,
                "explicit-text-pattern-v1",
                serde_json::json!({"language": "strict-pattern-parameters", "natural_language_inference": false}),
            )
        }
        GenerationInput::Patterns { patterns } => {
            check_declared_source(request, input)?;
            (
                synthesize_patterns(patterns)?,
                "explicit-six-axis-pattern-v1",
                serde_json::json!({"observed_motion": false}),
            )
        }
        GenerationInput::Image { prompt, .. } => {
            let spec = validate_still_image_prompt(Some(prompt))?;
            let index = media::probe(&request.tools.ffprobe.path, &request.source.path, deadline)?;
            if index.pts.len() != 1 {
                return Err(ProtocolError::invalid(
                    "image input requires exactly one decoded still frame",
                )
                .into());
            }
            let (width, height) = media::fit_dimensions(index.width, index.height, 64, 64)?;
            let _pixels = media::extract(
                &request.tools.ffmpeg.path,
                &request.source.path,
                media::SnapshotFrame {
                    index: 0,
                    source_time: SourceTimestamp::new(
                        i64::try_from(i128::from(index.pts[0]) * i128::from(index.time_base_num))?,
                        index.time_base_den,
                    )?,
                },
                width,
                height,
                request.budget.cpu_threads,
                deadline,
            )?;
            notes.push("image_not_semantically_conditioning_motion");
            notes.push("constrained_pattern_parser_not_local_ai");
            (
                synthesize_pattern(&spec)?,
                "validated-still-plus-explicit-pattern-v1",
                serde_json::json!({
                    "image_dimensions": [index.width, index.height], "image_semantic_inference": false,
                    "conditioning": "prompt pattern only; image bytes decoded and bound to lineage"
                }),
            )
        }
        GenerationInput::Audio {
            mapping,
            mode,
            analysis,
            ..
        } => {
            let decoded = super::audio::decode(request, deadline)?;
            generate_audio_channel(&decoded, mapping, mode, analysis, &mut notes)?
        }
        GenerationInput::AudioBundle { recipe, .. } => {
            let decoded = super::audio::decode(request, deadline)?;
            let mut tracks = Vec::new();
            let mut channel_details = Vec::new();
            for channel in recipe.channels() {
                if Instant::now() >= deadline {
                    bail!("audio bundle deadline exceeded");
                }
                let (program, algorithm, details) = generate_audio_channel(
                    &decoded,
                    &channel.mapping,
                    &channel.mode,
                    &Some(channel.analysis),
                    &mut notes,
                )?;
                tracks.extend(program.tracks().iter().cloned());
                channel_details.push(serde_json::json!({"axis": channel.mapping.axis(), "recipe": algorithm, "details": details}));
            }
            (
                MotionProgram::new(tracks)?,
                "explicit-six-axis-audio-v1",
                serde_json::json!({
                    "decode_count": 1, "channels": channel_details, "observed_motion": false,
                    "physical_axis_calibration": false, "source_timeline": decoded.timeline,
                }),
            )
        }
    };
    if Instant::now() >= deadline {
        bail!("synthetic generation deadline exceeded");
    }
    let duration = program
        .tracks()
        .iter()
        .flat_map(|track| track.actions())
        .map(|action| action.time())
        .max()
        .unwrap_or(ProjectTime::ZERO);
    let mut spans = ReviewSpans::new();
    for note in notes {
        record_review(
            &mut spans,
            note.to_owned(),
            ProjectTime::ZERO,
            duration.checked_add(1)?,
            EvidenceKind::Synthesized,
        )?;
    }
    let review = finish_reviews(spans, None)?;
    let program_artifact = artifacts::write_json(
        &request.output_dir,
        "candidate.json",
        &program,
        request.budget.output_bytes,
    )?;
    let mut lineage = vec![request.source.identity.clone()];
    for dependency in request.dependencies.iter().chain([
        &request.tools.ffmpeg.identity,
        &request.tools.ffprobe.identity,
    ]) {
        if !lineage.contains(dependency) {
            lineage.push(dependency.clone());
        }
    }
    let configuration = serde_json::to_vec(input)?;
    let receipt = serde_json::json!({
        "schema_version": 2, "recipe": recipe, "qualification": "unqualified",
        "project_id": request.project_id, "base_revision": request.base_revision,
        "job_id": request.job_id, "attempt_id": request.attempt_id,
        "source_version": request.source.source_version, "program": program_artifact.identity,
        "configuration_sha256": format!("{:x}", Sha256::digest(configuration)),
        "dependencies": lineage, "details": details, "review": review,
        "evidence": "synthesized", "confidence_calibration": "none",
        "claim": "Deterministic synthetic mapping, not neural, creative-quality, performance or device qualification"
    });
    let remaining = request
        .budget
        .output_bytes
        .checked_sub(program_artifact.identity.byte_len)
        .context("candidate artifact budget exhausted")?;
    let receipt = artifacts::write_json(&request.output_dir, "lineage.json", &receipt, remaining)?;
    Ok(WorkerOutput::Generated {
        program: program_artifact,
        receipt,
        lineage,
        review,
    })
}

fn check_declared_source(request: &WorkerRequest, input: &GenerationInput) -> Result<()> {
    let expected = serde_json::to_vec(input)?;
    if expected.len() as u64 > request.budget.memory_bytes / 16 {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "synthetic input exceeds memory admission",
        )
        .into());
    }
    let file = std::fs::File::open(&request.source.path)?;
    let mut actual = Vec::new();
    file.take(expected.len() as u64 + 1)
        .read_to_end(&mut actual)?;
    if actual != expected {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "synthetic input differs from immutable source snapshot",
        )
        .into());
    }
    Ok(())
}

fn generate_audio_channel(
    decoded: &super::audio::DecodedAudio,
    mapping: &AudioMapping,
    mode: &AudioMode,
    analysis: &Option<AudioAnalysisOptions>,
    notes: &mut Vec<&'static str>,
) -> Result<(MotionProgram, &'static str, serde_json::Value)> {
    Ok({
        let (features, saturated_windows) = if let Some(options) = analysis {
            let result = analyze_audio(
                &decoded.samples,
                AUDIO_SAMPLE_RATE,
                AUDIO_WINDOW_SAMPLES,
                *options,
            )?;
            (result.features, result.saturated_windows)
        } else {
            (
                extract_audio_features(&decoded.samples, AUDIO_SAMPLE_RATE, AUDIO_WINDOW_SAMPLES)?,
                0,
            )
        };
        let band = analysis.map_or(AudioBand::Full, |a| a.band());
        let threshold = analysis.map_or(AUDIO_ONSET_THRESHOLD, |a| a.threshold());
        let spectral = serde_json::json!({"options": analysis, "cutoffs_hz": band.cutoffs_hz(),
                "filter": if band == AudioBand::Full {"none"} else {"causal-butterworth-hp-lp-q-sqrt-half-v1"},
                "initial_state": "zero", "filter_delay": if band == AudioBand::Full {"none"} else {"frequency-dependent-uncompensated"},
                "lowpass_bypassed_at_nyquist": band.cutoffs_hz().is_some_and(|(_, upper)| 2 * upper == AUDIO_SAMPLE_RATE),
                "timestamp": "original-window-first-sample", "envelope_gated": analysis.is_some(),
                "rms_clamp_max": 1, "saturated_windows": saturated_windows});
        notes.push("audio_mapping_uncalibrated");
        match mode {
            AudioMode::Envelope => (
                synthesize_audio_envelope(&features.envelope, mapping)?,
                if analysis.is_some() {
                    "spectral-rms-envelope-v1"
                } else {
                    "full-band-rms-envelope-v1"
                },
                serde_json::json!({"sample_rate": AUDIO_SAMPLE_RATE, "window_samples": AUDIO_WINDOW_SAMPLES,
                        "channels": 1, "band": band, "analysis": spectral, "normalization": "none", "resampled_sample_count": decoded.samples.len(), "source_timeline": decoded.timeline}),
            ),
            AudioMode::Vibe => {
                notes.push("audio_oscillation_not_device_qualified");
                (
                    synthesize_audio_oscillation(&features.envelope, mapping)?,
                    "audio-rms-modulated-oscillation-8hz-v1",
                    serde_json::json!({"sample_rate": AUDIO_SAMPLE_RATE, "window_samples": AUDIO_WINDOW_SAMPLES,
                            "channels": 1, "band": band, "analysis": spectral, "normalization": "none",
                            "carrier_hz": AUDIO_OSCILLATION_HZ, "carrier_period_ns": AUDIO_OSCILLATION_PERIOD_NS,
                            "phase_origin": "decoded-source-relative-time-zero", "phase_arithmetic": "integer-nanosecond-modulo",
                            "position_center": mapping.offset().value() + mapping.amplitude() / 2.0,
                            "position_half_span": mapping.amplitude() / 2.0, "energy_coupling": "raw-gated-rms-times-half-span",
                            "silence": "hold-center", "musical_beat_tracking": false,
                            "resampled_sample_count": decoded.samples.len(), "source_timeline": decoded.timeline}),
                )
            }
            AudioMode::Beats => {
                // This mode generates pulses at RMS threshold onsets, not a
                // claim of musical beat tracking. Unsupported edge/overlap
                // pulses are omitted and reported, never retimed.
                let rise = 50_000_000u64;
                let fall = 100_000_000u64;
                let mut selected = Vec::new();
                let mut omitted = 0usize;
                for onset in &features.amplitude_onsets {
                    let t = onset.as_nanos();
                    let fits = t >= rise as i64
                        && t.checked_add(fall as i64)
                            .is_some_and(|end| end <= features.duration.as_nanos())
                        && selected.last().is_none_or(|last: &ProjectTime| {
                            t - last.as_nanos() >= (rise + fall) as i64
                        });
                    if fits {
                        selected.push(*onset);
                    } else {
                        omitted += 1;
                    }
                }
                notes.push("amplitude_onsets_not_musical_beat_tracking");
                if omitted > 0 {
                    notes.push("unsupported_onset_pulses_omitted");
                }
                (
                    synthesize_beats(
                        &selected,
                        features.duration,
                        &BeatMapping::new(
                            mapping.axis(),
                            mapping.amplitude(),
                            mapping.offset(),
                            rise,
                            fall,
                        )?,
                    )?,
                    if analysis.is_some() {
                        "spectral-rms-onset-pulses-v1"
                    } else {
                        "full-band-rms-onset-pulses-v1"
                    },
                    serde_json::json!({"sample_rate": AUDIO_SAMPLE_RATE, "window_samples": AUDIO_WINDOW_SAMPLES,
                            "onset_threshold": threshold, "rise_ns": rise, "fall_ns": fall,
                            "raw_onsets": features.amplitude_onsets.len(), "retained_onsets": selected.len(),
                            "omitted_edge_or_overlapping_onsets": omitted, "musical_beat_tracking": false,
                            "band": band, "analysis": spectral, "normalization": "none", "resampled_sample_count": decoded.samples.len(), "source_timeline": decoded.timeline}),
                )
            }
        }
    })
}
