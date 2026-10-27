// Test-only authored declarations, not measurement or operator acceptance.
// Include beneath a test module importing Protocol names (use super::*).
use super::*;
use std::collections::BTreeMap;

fn fixture_bytes(value: &impl serde::Serialize) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}
fn fixture_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
fn fixture_artifact(label: &str) -> ArtifactIdentity {
    let sha256 = fixture_hash(label.as_bytes());
    ArtifactIdentity {
        artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
        sha256,
        byte_len: label.len() as u64,
    }
}
fn fixture_path(name: &str) -> std::path::PathBuf {
    if cfg!(windows) {
        format!("C:\\fixture\\{name}").into()
    } else {
        format!("/fixture/{name}").into()
    }
}

pub(super) fn synthetic_fixture() -> (
    InferenceEquivalenceManifest,
    BTreeMap<InferenceEquivalenceEvidenceKind, Vec<u8>>,
) {
    use InferenceEquivalenceEvidenceDetails as D;
    use InferenceEquivalenceEvidenceKind as K;
    let profile = |side: &str, width| DeclaredInferenceProfile {
        schema_version: 1,
        model: fixture_artifact("same-authored-model"),
        runtime: fixture_artifact(&format!("{side}-runtime")),
        preprocessing: InferencePreprocessing {
            input: ModelInputContract {
                width,
                height: 64,
                channels: 3,
                layout: TensorLayout::Nchw,
                class_count: 1,
                decoder: DetectorDecoder::YoloChannelMajor,
            },
            resize: InferenceResize::Stretch,
            color: InferenceColor::Rgb,
            scale: vec![1.0 / 255.0; 3],
            bias: vec![0.0; 3],
            padding_byte: 0,
        },
        provider: InferenceProvider::Cpu,
        minimum_memory_bytes: 4096,
        minimum_output_bytes: 1024,
        cpu_threads: 1,
        cuda_resources: None,
    };
    let environment = |profile: &DeclaredInferenceProfile| InferenceEquivalenceEnvironment {
        engine: fixture_artifact("authored-engine-identity"),
        worker: fixture_artifact("authored-worker-identity"),
        os: "linux".into(),
        architecture: "x86_64".into(),
        platform_build: fixture_artifact("authored-platform-observation"),
        hardware: fixture_artifact("authored-hardware-observation"),
        dependencies: vec![
            profile.runtime.clone(),
            fixture_artifact("ffmpeg"),
            fixture_artifact("ffprobe"),
        ],
    };
    let from_profile = profile("from", 64);
    let to_profile = profile("to", 32);
    let subject=InferenceEquivalenceSubject {
        schema_version:1,origin:InferenceEquivalenceOrigin::Synthetic,
        from_environment:environment(&from_profile),to_environment:environment(&to_profile),from_profile,to_profile,
        scope:InferenceEquivalenceScope {tier:InferenceEquivalenceTier::Default,recipe:InferenceEquivalenceRecipe::YoloTracking,axes:vec![Axis::Stroke],source_policy:fixture_artifact("authored-source-scope"),view_policy:fixture_artifact("authored-view-scope"),limitations:"Authored schema fixture only; no actual execution, measurement, reviewer or qualification.".into()},
        not_before_unix_ms:1000,expires_unix_ms:10000,
    };
    subject.validate().unwrap();
    let scope_sha256 = subject.scope_sha256().unwrap();
    let window = InferenceEquivalenceWindow {
        start_ns: 0,
        end_ns: 1_000_000_000,
    };
    let corpus = InferenceEquivalenceCorpus {
        split: InferenceEquivalenceSplit::Development,
        heldout_freeze: None,
        cases: vec![InferenceEquivalenceCase {
            id: "authored-case".into(),
            source_sha256: fixture_artifact("source").sha256,
            reference_sha256: fixture_artifact("reference").sha256,
            group: "authored-group".into(),
            target_id: "authored-target".into(),
            window_ns: window,
            independent_annotations: false,
            evaluation_rights: true,
        }],
    };
    let execution = |side: &str,
                     profile: &DeclaredInferenceProfile,
                     environment: &InferenceEquivalenceEnvironment| {
        let source = |label: &str| SourceArtifact {
            source_version: SourceVersionId::new(label).unwrap(),
            path: fixture_path(label),
            identity: fixture_artifact(label),
        };
        let tool = |label: &str| ArtifactReference {
            path: fixture_path(label),
            identity: fixture_artifact(label),
        };
        let request = WorkerRequest {
            cuda_execution: None,
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new(format!("{side}-attempt")).unwrap(),
            job_id: JobId::new(format!("{side}-job")).unwrap(),
            project_id: ProjectId::new("authored-project").unwrap(),
            base_revision: RevisionId::new(0),
            source: source("source"),
            independent_audio: None,
            output_dir: fixture_path(&format!("{side}-output")),
            operation: WorkerOperation::Generate {
                preset: "default".into(),
                model: Some(SourceArtifact {
                    source_version: SourceVersionId::new("model").unwrap(),
                    path: fixture_path("model"),
                    identity: profile.model.clone(),
                }),
                settings: Box::new(GenerationSettings {
                    analysis_width: 64,
                    analysis_height: 64,
                    model_input: Some(profile.preprocessing.input.clone()),
                    inference_profile: Some(profile.clone()),
                    project_interval: Some(
                        TimeRange::new(
                            ProjectTime::from_nanos(window.start_ns),
                            ProjectTime::from_nanos(window.end_ns),
                        )
                        .unwrap(),
                    ),
                    ..GenerationSettings::default()
                }),
            },
            budget: ResourceBudget {
                memory_bytes: 4096,
                output_bytes: 1024,
                wall_time_ms: 1000,
                cpu_threads: 1,
            },
            dependencies: vec![profile.model.clone(), profile.runtime.clone()],
            tools: WorkerTools {
                ffmpeg: tool("ffmpeg"),
                ffprobe: tool("ffprobe"),
                onnx_runtime: Some(ArtifactReference {
                    path: fixture_path(&format!("{side}-runtime")),
                    identity: profile.runtime.clone(),
                }),
            },
        };
        request.validate().unwrap();
        InferenceEquivalenceExecution {
            scope_sha256: scope_sha256.clone(),
            cases: vec![InferenceEquivalenceExecutionCase {
                case_id: "authored-case".into(),
                target_id: "authored-target".into(),
                window_ns: window,
                manifest: request,
                profile_sha256: fixture_hash(&fixture_bytes(profile)),
                environment_sha256: environment.identity_sha256().unwrap(),
                execution_receipt: fixture_artifact(&format!("{side}-receipt")),
                output: fixture_artifact(&format!("{side}-program")),
                successful_runs: 2,
                elapsed_ns: 10,
                peak_memory_bytes: Some(1024),
            }],
        }
    };
    let from = execution("from", &subject.from_profile, &subject.from_environment);
    let to = execution("to", &subject.to_profile, &subject.to_environment);
    let protocol = InferenceEquivalenceProtocol {
        scope_sha256,
        approval_record: fixture_artifact("authored-criteria-declaration-not-human-approval"),
        criteria: vec![InferenceEquivalenceCriterion {
            id: "authored-score".into(),
            axis: Axis::Stroke,
            unit: "arbitrary fixture unit".into(),
            min: Some(0.0),
            max: Some(1.0),
            max_delta: Some(0.2),
        }],
    };
    let quality = InferenceEquivalenceQualityComparison {
        corpus_sha256: String::new(),
        protocol_sha256: String::new(),
        from_execution_sha256: String::new(),
        to_execution_sha256: String::new(),
        cases: vec![InferenceEquivalenceQualityCase {
            case_id: "authored-case".into(),
            values: vec![InferenceEquivalenceMeasuredValue {
                criterion_id: "authored-score".into(),
                from_value: 0.5,
                to_value: 0.6,
            }],
        }],
    };
    let placement = InferenceEquivalenceProviderPlacement {
        from_execution_sha256: String::new(),
        to_execution_sha256: String::new(),
        cases: vec![InferenceEquivalencePlacementCase {
            case_id: "authored-case".into(),
            from: InferenceEquivalencePlacementObservation {
                profile_sha256: from.cases[0].profile_sha256.clone(),
                executed_providers: vec![InferenceProvider::Cpu],
                trace: fixture_artifact("authored-from-trace"),
                successful_runs: 2,
            },
            to: InferenceEquivalencePlacementObservation {
                profile_sha256: to.cases[0].profile_sha256.clone(),
                executed_providers: vec![InferenceProvider::Cpu],
                trace: fixture_artifact("authored-to-trace"),
                successful_runs: 2,
            },
        }],
    };
    let resources = InferenceEquivalenceResources {
        from_execution_sha256: String::new(),
        to_execution_sha256: String::new(),
        cases: vec![InferenceEquivalenceResourceCase {
            case_id: "authored-case".into(),
            from_elapsed_ns: 10,
            to_elapsed_ns: 10,
            from_peak_memory_bytes: Some(1024),
            to_peak_memory_bytes: Some(1024),
        }],
    };
    let review = InferenceEquivalenceReview {
        reviewer: "Authored fixture attribution; not a human review".into(),
        reviewed_at_unix_ms: 2000,
        reviewed_evidence: vec![],
        conclusion: InferenceEquivalenceConclusion::Inconclusive,
        limitations: "Authored correlation inputs only. No measurements or acceptance.".into(),
    };
    seal_synthetic(
        subject,
        [
            (K::Corpus, D::Corpus(corpus)),
            (K::Protocol, D::Protocol(protocol)),
            (K::FromExecution, D::FromExecution(from)),
            (K::ToExecution, D::ToExecution(to)),
            (K::QualityComparison, D::QualityComparison(quality)),
            (K::ProviderPlacement, D::ProviderPlacement(placement)),
            (K::Resources, D::Resources(resources)),
            (K::Review, D::Review(review)),
        ]
        .into_iter()
        .collect(),
    )
}

pub(super) fn fixture_documents(
    bytes: &BTreeMap<InferenceEquivalenceEvidenceKind, Vec<u8>>,
) -> BTreeMap<InferenceEquivalenceEvidenceKind, InferenceEquivalenceEvidenceDetails> {
    bytes
        .iter()
        .map(|(kind, raw)| {
            (
                *kind,
                serde_json::from_slice::<InferenceEquivalenceEvidenceDocument>(raw)
                    .unwrap()
                    .details,
            )
        })
        .collect()
}

/// Rebind authored fixtures after a test mutation, so semantic checks are
/// exercised independently of an earlier stale-digest refusal. Never Measured.
pub(super) fn seal_synthetic(
    subject: InferenceEquivalenceSubject,
    mut details: BTreeMap<InferenceEquivalenceEvidenceKind, InferenceEquivalenceEvidenceDetails>,
) -> (
    InferenceEquivalenceManifest,
    BTreeMap<InferenceEquivalenceEvidenceKind, Vec<u8>>,
) {
    use InferenceEquivalenceEvidenceDetails as D;
    use InferenceEquivalenceEvidenceKind as K;
    assert_eq!(subject.origin, InferenceEquivalenceOrigin::Synthetic);
    let subject_sha256 = subject.subject_sha256().unwrap();
    let mut bytes: BTreeMap<K, Vec<u8>> = BTreeMap::new();
    for kind in K::ALL {
        let hashes = |role| fixture_hash(&bytes[&role]);
        let value = details.get_mut(&kind).unwrap();
        match value {
            D::QualityComparison(v) => {
                v.corpus_sha256 = hashes(K::Corpus);
                v.protocol_sha256 = hashes(K::Protocol);
                v.from_execution_sha256 = hashes(K::FromExecution);
                v.to_execution_sha256 = hashes(K::ToExecution);
            }
            D::ProviderPlacement(v) => {
                v.from_execution_sha256 = hashes(K::FromExecution);
                v.to_execution_sha256 = hashes(K::ToExecution);
            }
            D::Resources(v) => {
                v.from_execution_sha256 = hashes(K::FromExecution);
                v.to_execution_sha256 = hashes(K::ToExecution);
            }
            D::Review(v) => {
                v.reviewed_evidence = K::ALL
                    .into_iter()
                    .filter(|role| *role != K::Review)
                    .map(|role| InferenceEquivalenceReviewedEvidence {
                        kind: role,
                        sha256: hashes(role),
                    })
                    .collect();
            }
            _ => {}
        }
        bytes.insert(
            kind,
            fixture_bytes(&InferenceEquivalenceEvidenceDocument {
                schema_version: 1,
                origin: InferenceEquivalenceOrigin::Synthetic,
                subject_sha256: subject_sha256.clone(),
                kind,
                details: value.clone(),
            }),
        );
    }
    let evidence = bytes
        .iter()
        .map(|(kind, raw)| {
            let sha256 = fixture_hash(raw);
            InferenceEquivalenceEvidence {
                kind: *kind,
                identity: ArtifactIdentity {
                    artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
                    sha256,
                    byte_len: raw.len() as u64,
                },
            }
        })
        .collect();
    let manifest = InferenceEquivalenceManifest { subject, evidence };
    manifest.validate().unwrap();
    (manifest, bytes)
}
