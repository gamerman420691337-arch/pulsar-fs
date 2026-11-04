//! Correlated inference evidence and owner decisions. JSON does not authenticate
//! measurements, prove scientific qualification, or grant execution authority.
use crate::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path},
};

pub const INFERENCE_EQUIVALENCE_MANIFEST_LIMIT: u64 = 128 * 1024;
pub const INFERENCE_EQUIVALENCE_EVIDENCE_LIMIT: u64 = 4 * 1024 * 1024;
pub const INFERENCE_EQUIVALENCE_EVIDENCE_TOTAL_LIMIT: u64 = 32 * 1024 * 1024;
pub const INFERENCE_EQUIVALENCE_PAGE_LIMIT: usize = 512 * 1024;
pub const INFERENCE_EQUIVALENCE_MAX_CASES: usize = 64;
pub const INFERENCE_EQUIVALENCE_MAX_CRITERIA: usize = 64;

fn check(value: bool, message: &str) -> Result<(), ProtocolError> {
    if value {
        Ok(())
    } else {
        Err(ProtocolError::invalid(message))
    }
}
fn text(value: &str, maximum: usize) -> Result<(), ProtocolError> {
    check(
        !value.trim().is_empty()
            && value.len() <= maximum
            && !value
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t'),
        "equivalence text is empty, too long or contains control characters",
    )
}
fn token(value: &str) -> Result<(), ProtocolError> {
    check(
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b)),
        "invalid bounded equivalence token",
    )
}
fn artifact(value: &ArtifactIdentity) -> Result<(), ProtocolError> {
    value.validate()?;
    check(
        value.byte_len > 0 && value.byte_len <= i64::MAX as u64,
        "equivalence artifact must have a positive bounded length",
    )
}
fn encoded<T: Serialize>(value: &T) -> Result<Vec<u8>, ProtocolError> {
    serde_json::to_vec(value).map_err(|_| ProtocolError::invalid("equivalence encoding failed"))
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn typed_hash<T: Serialize>(value: &T) -> Result<String, ProtocolError> {
    Ok(hash(&encoded(value)?))
}
fn clock(value: u64) -> Result<(), ProtocolError> {
    check(
        value > 0 && value <= i64::MAX as u64,
        "equivalence time must be positive and fit signed storage",
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceEquivalenceOrigin {
    Synthetic,
    Measured,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceEquivalenceTier {
    Fast,
    Default,
    Maximum,
}
impl InferenceEquivalenceTier {
    pub fn from_preset(value: &str) -> Result<Self, ProtocolError> {
        match value {
            "fast" | "economy" => Ok(Self::Fast),
            "default" => Ok(Self::Default),
            "maximum" | "quality" => Ok(Self::Maximum),
            _ => Err(ProtocolError::invalid("unknown equivalence quality tier")),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceEquivalenceRecipe {
    YoloTracking,
    TemporalPosition,
    AnnotatedTemporalPosition,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceEnvironment {
    pub engine: ArtifactIdentity,
    pub worker: ArtifactIdentity,
    pub os: String,
    pub architecture: String,
    pub platform_build: ArtifactIdentity,
    pub hardware: ArtifactIdentity,
    pub dependencies: Vec<ArtifactIdentity>,
}
impl InferenceEquivalenceEnvironment {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        for value in [
            &self.engine,
            &self.worker,
            &self.platform_build,
            &self.hardware,
        ] {
            artifact(value)?;
        }
        token(&self.os)?;
        token(&self.architecture)?;
        check(
            !self.dependencies.is_empty() && self.dependencies.len() <= MAX_NATIVE_LIBRARIES,
            "equivalence dependency count is invalid",
        )?;
        let mut seen = BTreeSet::new();
        for value in &self.dependencies {
            artifact(value)?;
            check(
                seen.insert(value.sha256.clone()),
                "duplicate equivalence dependency digest",
            )?;
        }
        Ok(())
    }
    pub fn identity_sha256(&self) -> Result<String, ProtocolError> {
        self.validate()?;
        typed_hash(self)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceScope {
    pub tier: InferenceEquivalenceTier,
    pub recipe: InferenceEquivalenceRecipe,
    pub axes: Vec<Axis>,
    pub source_policy: ArtifactIdentity,
    pub view_policy: ArtifactIdentity,
    pub limitations: String,
}
impl InferenceEquivalenceScope {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        check(
            !self.axes.is_empty()
                && self.axes.len() <= Axis::ALL.len()
                && self.axes.iter().copied().collect::<BTreeSet<_>>().len() == self.axes.len(),
            "equivalence axes must be nonempty and unique",
        )?;
        artifact(&self.source_policy)?;
        artifact(&self.view_policy)?;
        text(&self.limitations, 4096)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceSubject {
    pub schema_version: u16,
    pub origin: InferenceEquivalenceOrigin,
    pub from_profile: DeclaredInferenceProfile,
    pub to_profile: DeclaredInferenceProfile,
    pub from_environment: InferenceEquivalenceEnvironment,
    pub to_environment: InferenceEquivalenceEnvironment,
    pub scope: InferenceEquivalenceScope,
    pub not_before_unix_ms: u64,
    pub expires_unix_ms: u64,
}
impl InferenceEquivalenceSubject {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        check(
            self.schema_version == 1,
            "unsupported inference equivalence subject version",
        )?;
        self.from_profile.validate()?;
        self.to_profile.validate()?;
        self.from_environment.validate()?;
        self.to_environment.validate()?;
        self.scope.validate()?;
        clock(self.not_before_unix_ms)?;
        clock(self.expires_unix_ms)?;
        check(
            self.expires_unix_ms > self.not_before_unix_ms,
            "equivalence validity interval must be positive",
        )?;
        check(self.from_profile.model == self.to_profile.model, "equivalence v1 requires identical model identity; class_count alone cannot establish label ontology")?;
        let a = &self.from_profile.preprocessing.input;
        let b = &self.to_profile.preprocessing.input;
        check(
            a.decoder == b.decoder
                && a.class_count == b.class_count
                && a.channels == b.channels
                && encoded(&a.layout)? == encoded(&b.layout)?,
            "equivalence model decoder, class semantics or channel/layout differs",
        )?;
        for (profile, environment) in [
            (&self.from_profile, &self.from_environment),
            (&self.to_profile, &self.to_environment),
        ] {
            check(
                environment.dependencies.contains(&profile.runtime),
                "equivalence environment omits exact profile runtime",
            )?;
            let p = &profile.preprocessing;
            let coherent = match self.scope.recipe {
                InferenceEquivalenceRecipe::YoloTracking => {
                    p.input.decoder == DetectorDecoder::YoloChannelMajor
                        && matches!(p.color, InferenceColor::Rgb)
                        && matches!(
                            p.resize,
                            InferenceResize::Stretch | InferenceResize::Letterbox
                        )
                }
                InferenceEquivalenceRecipe::TemporalPosition => {
                    p.input.decoder == DetectorDecoder::TemporalScriptPosition
                        && matches!(p.color, InferenceColor::RgbFramePair)
                        && matches!(p.resize, InferenceResize::NearestStretch)
                }
                InferenceEquivalenceRecipe::AnnotatedTemporalPosition => {
                    p.input.decoder == DetectorDecoder::TemporalScriptPosition
                        && matches!(p.color, InferenceColor::RgbFramePair)
                        && matches!(p.resize, InferenceResize::AnnotatedTemporalView)
                }
            };
            check(
                coherent,
                "profile decoder/preprocessing does not implement the declared equivalence recipe",
            )?;
        }
        check(
            self.from_profile_sha256()? != self.to_profile_sha256()?,
            "equivalence profiles must be distinct",
        )?;
        check(
            encoded(self)?.len() as u64 <= INFERENCE_EQUIVALENCE_MANIFEST_LIMIT,
            "equivalence subject exceeds manifest bound",
        )
    }
    pub fn from_profile_sha256(&self) -> Result<String, ProtocolError> {
        self.from_profile.validate()?;
        typed_hash(&self.from_profile)
    }
    pub fn to_profile_sha256(&self) -> Result<String, ProtocolError> {
        self.to_profile.validate()?;
        typed_hash(&self.to_profile)
    }
    pub fn subject_sha256(&self) -> Result<String, ProtocolError> {
        self.validate()?;
        typed_hash(self)
    }
    pub fn scope_sha256(&self) -> Result<String, ProtocolError> {
        self.scope.validate()?;
        typed_hash(&self.scope)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceEquivalenceEvidenceKind {
    Corpus,
    Protocol,
    FromExecution,
    ToExecution,
    QualityComparison,
    ProviderPlacement,
    Resources,
    Review,
}
impl InferenceEquivalenceEvidenceKind {
    pub const ALL: [Self; 8] = [
        Self::Corpus,
        Self::Protocol,
        Self::FromExecution,
        Self::ToExecution,
        Self::QualityComparison,
        Self::ProviderPlacement,
        Self::Resources,
        Self::Review,
    ];
}
fn roles(
    kinds: impl IntoIterator<Item = InferenceEquivalenceEvidenceKind>,
) -> Result<(), ProtocolError> {
    let list: Vec<_> = kinds.into_iter().collect();
    check(
        list.len() == 8
            && list.into_iter().collect::<BTreeSet<_>>()
                == InferenceEquivalenceEvidenceKind::ALL.into_iter().collect(),
        "equivalence requires exactly eight unique evidence roles",
    )
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceInputEvidence {
    pub kind: InferenceEquivalenceEvidenceKind,
    pub path: String,
    pub sha256: String,
    pub byte_len: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceInput {
    pub subject: InferenceEquivalenceSubject,
    pub evidence: Vec<InferenceEquivalenceInputEvidence>,
}
impl InferenceEquivalenceInput {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.subject.validate()?;
        roles(self.evidence.iter().map(|v| v.kind))?;
        for item in &self.evidence {
            validate_sha256(&item.sha256)?;
            check(
                (1..=INFERENCE_EQUIVALENCE_EVIDENCE_LIMIT).contains(&item.byte_len),
                "equivalence evidence byte bound",
            )?;
            let path = Path::new(&item.path);
            check(
                path.is_absolute()
                    && item.path.len() <= 4096
                    && !item.path.contains('\0')
                    && !path.components().any(|c| matches!(c, Component::ParentDir)),
                "equivalence evidence requires a bounded absolute path without traversal",
            )?;
        }
        check(
            encoded(self)?.len() as u64 <= INFERENCE_EQUIVALENCE_MANIFEST_LIMIT,
            "equivalence input manifest exceeds bound",
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceEvidence {
    pub kind: InferenceEquivalenceEvidenceKind,
    pub identity: ArtifactIdentity,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceManifest {
    pub subject: InferenceEquivalenceSubject,
    pub evidence: Vec<InferenceEquivalenceEvidence>,
}
impl InferenceEquivalenceManifest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.subject.validate()?;
        roles(self.evidence.iter().map(|v| v.kind))?;
        check(
            self.evidence.windows(2).all(|p| p[0].kind < p[1].kind),
            "equivalence manifest evidence must use canonical role order",
        )?;
        for item in &self.evidence {
            artifact(&item.identity)?;
            check(
                item.identity.byte_len <= INFERENCE_EQUIVALENCE_EVIDENCE_LIMIT
                    && item.identity.artifact_id.as_str() == item.identity.sha256,
                "equivalence evidence identity must bind bounded retained bytes",
            )?;
        }
        check(
            encoded(self)?.len() as u64 <= INFERENCE_EQUIVALENCE_MANIFEST_LIMIT,
            "equivalence manifest exceeds bound",
        )
    }
    pub fn id(&self) -> Result<ArtifactId, ProtocolError> {
        self.validate()?;
        ArtifactId::new(typed_hash(self)?).map_err(|e| ProtocolError::invalid(e.to_string()))
    }
    pub fn evidence_identity(
        &self,
        kind: InferenceEquivalenceEvidenceKind,
    ) -> Result<&ArtifactIdentity, ProtocolError> {
        self.evidence
            .iter()
            .find(|e| e.kind == kind)
            .map(|e| &e.identity)
            .ok_or_else(|| ProtocolError::invalid("equivalence evidence role missing"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceEquivalenceSplit {
    Development,
    Heldout,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceWindow {
    pub start_ns: i64,
    pub end_ns: i64,
}
impl InferenceEquivalenceWindow {
    fn validate(&self) -> Result<(), ProtocolError> {
        check(
            self.start_ns >= 0 && self.end_ns > self.start_ns,
            "equivalence source window must be nonnegative and nonempty",
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceCase {
    pub id: String,
    pub source_sha256: String,
    pub reference_sha256: String,
    pub group: String,
    pub target_id: String,
    pub window_ns: InferenceEquivalenceWindow,
    pub independent_annotations: bool,
    pub evaluation_rights: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceCorpus {
    pub split: InferenceEquivalenceSplit,
    pub heldout_freeze: Option<ArtifactIdentity>,
    pub cases: Vec<InferenceEquivalenceCase>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceCriterion {
    pub id: String,
    pub axis: Axis,
    pub unit: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub max_delta: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceProtocol {
    pub scope_sha256: String,
    pub approval_record: ArtifactIdentity,
    pub criteria: Vec<InferenceEquivalenceCriterion>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceExecutionCase {
    pub case_id: String,
    pub target_id: String,
    pub window_ns: InferenceEquivalenceWindow,
    pub manifest: WorkerRequest,
    pub profile_sha256: String,
    pub environment_sha256: String,
    pub execution_receipt: ArtifactIdentity,
    pub output: ArtifactIdentity,
    pub successful_runs: u64,
    pub elapsed_ns: u64,
    pub peak_memory_bytes: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceExecution {
    pub scope_sha256: String,
    pub cases: Vec<InferenceEquivalenceExecutionCase>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceMeasuredValue {
    pub criterion_id: String,
    pub from_value: f64,
    pub to_value: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceQualityCase {
    pub case_id: String,
    pub values: Vec<InferenceEquivalenceMeasuredValue>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceQualityComparison {
    pub corpus_sha256: String,
    pub protocol_sha256: String,
    pub from_execution_sha256: String,
    pub to_execution_sha256: String,
    pub cases: Vec<InferenceEquivalenceQualityCase>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalencePlacementObservation {
    pub profile_sha256: String,
    pub executed_providers: Vec<InferenceProvider>,
    pub trace: ArtifactIdentity,
    pub successful_runs: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalencePlacementCase {
    pub case_id: String,
    pub from: InferenceEquivalencePlacementObservation,
    pub to: InferenceEquivalencePlacementObservation,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceProviderPlacement {
    pub from_execution_sha256: String,
    pub to_execution_sha256: String,
    pub cases: Vec<InferenceEquivalencePlacementCase>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceResourceCase {
    pub case_id: String,
    pub from_elapsed_ns: u64,
    pub to_elapsed_ns: u64,
    pub from_peak_memory_bytes: Option<u64>,
    pub to_peak_memory_bytes: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceResources {
    pub from_execution_sha256: String,
    pub to_execution_sha256: String,
    pub cases: Vec<InferenceEquivalenceResourceCase>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceEquivalenceConclusion {
    EquivalentWithinScope,
    Inconclusive,
    Rejected,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceReviewedEvidence {
    pub kind: InferenceEquivalenceEvidenceKind,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceReview {
    pub reviewer: String,
    pub reviewed_at_unix_ms: u64,
    pub reviewed_evidence: Vec<InferenceEquivalenceReviewedEvidence>,
    pub conclusion: InferenceEquivalenceConclusion,
    pub limitations: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum InferenceEquivalenceEvidenceDetails {
    Corpus(InferenceEquivalenceCorpus),
    Protocol(InferenceEquivalenceProtocol),
    FromExecution(InferenceEquivalenceExecution),
    ToExecution(InferenceEquivalenceExecution),
    QualityComparison(InferenceEquivalenceQualityComparison),
    ProviderPlacement(InferenceEquivalenceProviderPlacement),
    Resources(InferenceEquivalenceResources),
    Review(InferenceEquivalenceReview),
}
impl InferenceEquivalenceEvidenceDetails {
    fn kind(&self) -> InferenceEquivalenceEvidenceKind {
        use InferenceEquivalenceEvidenceKind as K;
        match self {
            Self::Corpus(_) => K::Corpus,
            Self::Protocol(_) => K::Protocol,
            Self::FromExecution(_) => K::FromExecution,
            Self::ToExecution(_) => K::ToExecution,
            Self::QualityComparison(_) => K::QualityComparison,
            Self::ProviderPlacement(_) => K::ProviderPlacement,
            Self::Resources(_) => K::Resources,
            Self::Review(_) => K::Review,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceEvidenceDocument {
    pub schema_version: u16,
    pub origin: InferenceEquivalenceOrigin,
    pub subject_sha256: String,
    pub kind: InferenceEquivalenceEvidenceKind,
    pub details: InferenceEquivalenceEvidenceDetails,
}

/// Created only after cross-document checking. It does not authenticate any
/// measurement or person, decide scientific validity, or authorize execution.
#[derive(Clone, Debug, Serialize)]
pub struct InferenceEquivalenceAssessment {
    manifest_id: ArtifactId,
    subject_sha256: String,
    origin: InferenceEquivalenceOrigin,
    split: InferenceEquivalenceSplit,
    not_before_unix_ms: u64,
    expires_unix_ms: u64,
    independent_annotations: bool,
    evaluation_rights: bool,
    heldout_frozen: bool,
    declared_criteria_satisfied: bool,
    provider_placement_complete: bool,
    resources_complete: bool,
    review_conclusion: InferenceEquivalenceConclusion,
    reviewed_at_unix_ms: u64,
    review_identity: ArtifactIdentity,
    structurally_correlated: bool,
    measurement_authenticated: bool,
    scientific_qualification_claimed: bool,
    report_only: bool,
}
impl InferenceEquivalenceAssessment {
    pub fn manifest_id(&self) -> &ArtifactId {
        &self.manifest_id
    }
    pub fn review_identity(&self) -> &ArtifactIdentity {
        &self.review_identity
    }
    pub fn declared_criteria_satisfied(&self) -> bool {
        self.declared_criteria_satisfied
    }
    pub fn require_acceptance(&self, now_ms: u64) -> Result<(), ProtocolError> {
        clock(now_ms)?;
        check(
            self.origin == InferenceEquivalenceOrigin::Measured,
            "synthetic inference evidence cannot receive public acceptance",
        )?;
        check(
            now_ms >= self.not_before_unix_ms
                && now_ms < self.expires_unix_ms
                && now_ms >= self.reviewed_at_unix_ms,
            "equivalence evidence is not currently valid",
        )?;
        check(
            self.split == InferenceEquivalenceSplit::Heldout
                && self.heldout_frozen
                && self.independent_annotations
                && self.evaluation_rights,
            "acceptance requires a frozen independent evaluation-permitted heldout corpus",
        )?;
        check(self.declared_criteria_satisfied && self.provider_placement_complete && self.resources_complete && self.review_conclusion == InferenceEquivalenceConclusion::EquivalentWithinScope, "declared equivalence criteria, complete evidence or supplied review do not support acceptance")
    }
}

fn cases<'a, T>(
    items: &'a [T],
    id: impl Fn(&'a T) -> &'a str,
    expected: &BTreeSet<String>,
) -> Result<BTreeMap<String, &'a T>, ProtocolError> {
    check(
        items.len() == expected.len(),
        "equivalence case membership/count differs",
    )?;
    let mut result = BTreeMap::new();
    for item in items {
        let key = id(item);
        token(key)?;
        check(
            result.insert(key.to_owned(), item).is_none(),
            "duplicate equivalence case",
        )?;
    }
    check(
        result.keys().cloned().collect::<BTreeSet<_>>() == *expected,
        "equivalence case identities differ",
    )?;
    Ok(result)
}

fn check_execution<'a>(
    run: &'a InferenceEquivalenceExecution,
    subject: &InferenceEquivalenceSubject,
    corpus: &InferenceEquivalenceCorpus,
    profile: &DeclaredInferenceProfile,
    environment: &InferenceEquivalenceEnvironment,
) -> Result<BTreeMap<String, &'a InferenceEquivalenceExecutionCase>, ProtocolError> {
    check(
        run.scope_sha256 == subject.scope_sha256()?,
        "execution scope differs",
    )?;
    let expected = corpus.cases.iter().map(|v| v.id.clone()).collect();
    let entries = cases(&run.cases, |v| v.case_id.as_str(), &expected)?;
    let profile_hash = typed_hash(profile)?;
    let environment_hash = environment.identity_sha256()?;
    let mut attempts = BTreeSet::new();
    let mut executed_axes = BTreeSet::new();
    for case in &corpus.cases {
        let entry = entries[&case.id];
        entry.manifest.validate()?;
        check(
            attempts.insert(entry.manifest.attempt_id.clone()),
            "execution attempt reused across corpus cases",
        )?;
        check(
            entry.profile_sha256 == profile_hash && entry.environment_sha256 == environment_hash,
            "execution profile/environment digest differs",
        )?;
        check(
            entry.target_id == case.target_id
                && entry.window_ns == case.window_ns
                && entry.manifest.source.identity.sha256 == case.source_sha256,
            "execution source/target/window differs from corpus",
        )?;
        artifact(&entry.execution_receipt)?;
        artifact(&entry.output)?;
        clock(entry.elapsed_ns)?;
        check(
            entry.successful_runs > 0 && entry.successful_runs <= i64::MAX as u64,
            "execution requires positive bounded successful run count",
        )?;
        if let Some(memory) = entry.peak_memory_bytes {
            clock(memory)?;
        }
        let WorkerOperation::Generate {
            preset,
            settings,
            model,
        } = &entry.manifest.operation
        else {
            return Err(ProtocolError::invalid(
                "equivalence v1 requires explicit file-generation executions",
            ));
        };
        settings.validate_execution_preset(preset, true)?;
        executed_axes.insert(settings.axis);
        check(
            InferenceEquivalenceTier::from_preset(preset)? == subject.scope.tier
                && subject.scope.axes.contains(&settings.axis),
            "execution tier or axis differs from scope",
        )?;
        check(
            settings.project_interval.is_some_and(|range| {
                range.start().as_nanos() == case.window_ns.start_ns
                    && range.end().as_nanos() == case.window_ns.end_ns
            }),
            "execution requires the exact explicit corpus project-time interval",
        )?;
        check(
            settings
                .inference_profile
                .as_ref()
                .is_some_and(|p| encoded(p).ok() == encoded(profile).ok())
                && model.as_ref().is_some_and(|m| m.identity == profile.model),
            "execution explicit profile/model differs",
        )?;
        check(
            settings.model_input.as_ref().is_some_and(|input| {
                encoded(input).ok() == encoded(&profile.preprocessing.input).ok()
            }),
            "execution model input differs from exact profile",
        )?;
        check(
            entry
                .manifest
                .tools
                .onnx_runtime
                .as_ref()
                .is_some_and(|runtime| runtime.identity == profile.runtime),
            "execution pinned runtime differs",
        )?;
        profile.admit(&entry.manifest.budget)?;
        for tool in [&entry.manifest.tools.ffmpeg, &entry.manifest.tools.ffprobe] {
            check(
                environment.dependencies.contains(&tool.identity),
                "execution media tool absent from environment dependencies",
            )?;
        }
        if let Some(cuda) = &entry.manifest.cuda_execution {
            for library in &cuda.libraries {
                check(
                    environment.dependencies.contains(&library.identity),
                    "CUDA dependency absent from environment",
                )?;
            }
        }
        if subject.scope.recipe == InferenceEquivalenceRecipe::AnnotatedTemporalPosition {
            check(
                settings.annotated_view.as_ref().is_some_and(|view| {
                    view.source_sha256 == case.source_sha256
                        && view.declaration.target_id == case.target_id
                }),
                "annotated target/source differs from corpus",
            )?;
        } else {
            check(
                settings.annotated_view.is_none(),
                "unannotated recipe cannot carry annotated target settings",
            )?;
        }
    }
    check(
        executed_axes == subject.scope.axes.iter().copied().collect(),
        "execution documents do not cover every scoped axis",
    )?;
    Ok(entries)
}

fn paired_settings(
    from: &InferenceEquivalenceExecutionCase,
    to: &InferenceEquivalenceExecutionCase,
) -> Result<(), ProtocolError> {
    let a = &from.manifest;
    let b = &to.manifest;
    check(
        a.attempt_id != b.attempt_id
            && a.project_id == b.project_id
            && a.base_revision == b.base_revision
            && a.source.source_version == b.source.source_version
            && a.source.identity == b.source.identity,
        "paired execution attempt/project/source basis differs",
    )?;
    check(
        a.independent_audio
            .as_ref()
            .map(|v| (&v.source_version, &v.identity))
            == b.independent_audio
                .as_ref()
                .map(|v| (&v.source_version, &v.identity)),
        "paired independent audio basis differs",
    )?;
    check(
        a.tools.ffmpeg.identity == b.tools.ffmpeg.identity
            && a.tools.ffprobe.identity == b.tools.ffprobe.identity,
        "paired media decoder/probe identity differs",
    )?;
    let (
        WorkerOperation::Generate { settings: a, .. },
        WorkerOperation::Generate { settings: b, .. },
    ) = (&a.operation, &b.operation)
    else {
        return Err(ProtocolError::invalid(
            "paired execution must be generation",
        ));
    };
    let mut a = a.clone();
    let mut b = b.clone();
    a.inference_profile = None;
    b.inference_profile = None;
    a.model_input = None;
    b.model_input = None;
    check(
        encoded(&a)? == encoded(&b)?,
        "paired source/view/target/generation settings changed beyond profile/model_input",
    )
}

/// Checks retained document bytes and all declared correspondences. It never
/// treats a self-declared origin, signature name or metric as authenticated fact.
pub fn validate_evidence(
    manifest: &InferenceEquivalenceManifest,
    bytes: &BTreeMap<InferenceEquivalenceEvidenceKind, Vec<u8>>,
) -> Result<InferenceEquivalenceAssessment, ProtocolError> {
    use InferenceEquivalenceEvidenceDetails as D;
    use InferenceEquivalenceEvidenceKind as K;
    manifest.validate()?;
    roles(bytes.keys().copied())?;
    let subject = &manifest.subject;
    let subject_hash = subject.subject_sha256()?;
    let mut documents = BTreeMap::new();
    let mut total = 0u64;
    for kind in K::ALL {
        let raw = &bytes[&kind];
        let identity = manifest.evidence_identity(kind)?;
        total = total
            .checked_add(raw.len() as u64)
            .ok_or_else(|| ProtocolError::invalid("evidence total overflow"))?;
        check(
            !raw.is_empty()
                && raw.len() as u64 <= INFERENCE_EQUIVALENCE_EVIDENCE_LIMIT
                && total <= INFERENCE_EQUIVALENCE_EVIDENCE_TOTAL_LIMIT
                && raw.len() as u64 == identity.byte_len
                && hash(raw) == identity.sha256,
            "retained equivalence evidence bytes/digest differ",
        )?;
        let document: InferenceEquivalenceEvidenceDocument = serde_json::from_slice(raw)
            .map_err(|_| ProtocolError::invalid("invalid strict equivalence evidence document"))?;
        check(
            document.schema_version == 1
                && document.origin == subject.origin
                && document.subject_sha256 == subject_hash
                && document.kind == kind
                && document.details.kind() == kind,
            "equivalence evidence header/role/subject differs",
        )?;
        documents.insert(kind, document.details);
    }
    let (
        D::Corpus(corpus),
        D::Protocol(protocol),
        D::FromExecution(from),
        D::ToExecution(to),
        D::QualityComparison(quality),
        D::ProviderPlacement(placement),
        D::Resources(resources),
        D::Review(review),
    ) = (
        &documents[&K::Corpus],
        &documents[&K::Protocol],
        &documents[&K::FromExecution],
        &documents[&K::ToExecution],
        &documents[&K::QualityComparison],
        &documents[&K::ProviderPlacement],
        &documents[&K::Resources],
        &documents[&K::Review],
    )
    else {
        return Err(ProtocolError::invalid("equivalence role details differ"));
    };
    check(
        !corpus.cases.is_empty() && corpus.cases.len() <= INFERENCE_EQUIVALENCE_MAX_CASES,
        "equivalence corpus case bound",
    )?;
    let mut ids = BTreeSet::new();
    let mut source_groups = BTreeMap::new();
    for case in &corpus.cases {
        token(&case.id)?;
        token(&case.group)?;
        token(&case.target_id)?;
        validate_sha256(&case.source_sha256)?;
        validate_sha256(&case.reference_sha256)?;
        case.window_ns.validate()?;
        check(
            ids.insert(case.id.clone()),
            "duplicate equivalence corpus case",
        )?;
        if let Some(group) = source_groups.insert(case.source_sha256.clone(), case.group.clone()) {
            check(
                group == case.group,
                "identical source bytes have conflicting corpus groups",
            )?;
        }
    }
    if let Some(freeze) = &corpus.heldout_freeze {
        artifact(freeze)?;
    }
    check(
        corpus.split != InferenceEquivalenceSplit::Heldout || corpus.heldout_freeze.is_some(),
        "heldout corpus requires a supplied freeze identity",
    )?;
    check(
        protocol.scope_sha256 == subject.scope_sha256()?,
        "criteria protocol scope differs",
    )?;
    artifact(&protocol.approval_record)?;
    check(
        !protocol.criteria.is_empty()
            && protocol.criteria.len() <= INFERENCE_EQUIVALENCE_MAX_CRITERIA,
        "declared criteria bound",
    )?;
    let mut criteria = BTreeMap::new();
    let mut criterion_axes = BTreeSet::new();
    for criterion in &protocol.criteria {
        token(&criterion.id)?;
        text(&criterion.unit, 64)?;
        check(
            subject.scope.axes.contains(&criterion.axis)
                && criteria.insert(criterion.id.clone(), criterion).is_none(),
            "duplicate or out-of-scope criterion",
        )?;
        criterion_axes.insert(criterion.axis);
        check(
            [criterion.min, criterion.max, criterion.max_delta]
                .into_iter()
                .flatten()
                .all(f64::is_finite)
                && (criterion.min.is_some()
                    || criterion.max.is_some()
                    || criterion.max_delta.is_some())
                && criterion.max_delta.is_none_or(|v| v >= 0.0)
                && !matches!((criterion.min,criterion.max),(Some(a),Some(b)) if a>b),
            "invalid supplied criterion bounds",
        )?;
    }
    check(
        criterion_axes == subject.scope.axes.iter().copied().collect(),
        "criteria do not cover every scoped axis",
    )?;
    let from_cases = check_execution(
        from,
        subject,
        corpus,
        &subject.from_profile,
        &subject.from_environment,
    )?;
    let to_cases = check_execution(
        to,
        subject,
        corpus,
        &subject.to_profile,
        &subject.to_environment,
    )?;
    for id in &ids {
        paired_settings(from_cases[id], to_cases[id])?;
    }
    let digest = |kind| manifest.evidence_identity(kind).map(|a| a.sha256.as_str());
    let execution_refs = |a: &str, b: &str| -> Result<(), ProtocolError> {
        check(
            a == digest(K::FromExecution)? && b == digest(K::ToExecution)?,
            "comparison execution document digests differ",
        )
    };
    check(
        quality.corpus_sha256 == digest(K::Corpus)?
            && quality.protocol_sha256 == digest(K::Protocol)?,
        "quality corpus/protocol digests differ",
    )?;
    execution_refs(&quality.from_execution_sha256, &quality.to_execution_sha256)?;
    execution_refs(
        &placement.from_execution_sha256,
        &placement.to_execution_sha256,
    )?;
    execution_refs(
        &resources.from_execution_sha256,
        &resources.to_execution_sha256,
    )?;
    let quality_cases = cases(&quality.cases, |v| v.case_id.as_str(), &ids)?;
    let placement_cases = cases(&placement.cases, |v| v.case_id.as_str(), &ids)?;
    let resource_cases = cases(&resources.cases, |v| v.case_id.as_str(), &ids)?;
    let mut criteria_satisfied = true;
    let mut resources_complete = true;
    for id in &ids {
        let values = &quality_cases[id].values;
        check(
            values.len() == criteria.len(),
            "quality measurement count differs from criteria",
        )?;
        let mut measured = BTreeSet::new();
        for value in values {
            check(
                measured.insert(value.criterion_id.clone()),
                "duplicate quality criterion measurement",
            )?;
            let criterion = criteria.get(&value.criterion_id).ok_or_else(|| {
                ProtocolError::invalid("quality measurement names undeclared criterion")
            })?;
            check(
                value.from_value.is_finite() && value.to_value.is_finite(),
                "quality values must be finite",
            )?;
            criteria_satisfied &= [value.from_value, value.to_value].into_iter().all(|v| {
                criterion.min.is_none_or(|min| v >= min) && criterion.max.is_none_or(|max| v <= max)
            }) && criterion
                .max_delta
                .is_none_or(|delta| (value.to_value - value.from_value).abs() <= delta);
        }
        let pair = placement_cases[id];
        for (observed, execution, profile) in [
            (&pair.from, from_cases[id], &subject.from_profile),
            (&pair.to, to_cases[id], &subject.to_profile),
        ] {
            artifact(&observed.trace)?;
            check(
                observed.profile_sha256 == execution.profile_sha256
                    && observed.executed_providers.as_slice() == [profile.provider]
                    && observed.successful_runs == execution.successful_runs,
                "provider placement/profile/run count differs from execution",
            )?;
        }
        let resource = resource_cases[id];
        let a = from_cases[id];
        let b = to_cases[id];
        check(
            resource.from_elapsed_ns == a.elapsed_ns
                && resource.to_elapsed_ns == b.elapsed_ns
                && resource.from_peak_memory_bytes == a.peak_memory_bytes
                && resource.to_peak_memory_bytes == b.peak_memory_bytes,
            "resource measurements differ from execution",
        )?;
        resources_complete &= a.peak_memory_bytes.is_some() && b.peak_memory_bytes.is_some();
    }
    text(&review.reviewer, 256)?;
    text(&review.limitations, 4096)?;
    clock(review.reviewed_at_unix_ms)?;
    check(
        review.reviewed_evidence.len() == 7,
        "review must bind the other seven evidence documents",
    )?;
    let mut reviewed = BTreeSet::new();
    for reference in &review.reviewed_evidence {
        validate_sha256(&reference.sha256)?;
        check(
            reference.kind != K::Review
                && reviewed.insert(reference.kind)
                && reference.sha256 == digest(reference.kind)?,
            "review reference is duplicate, recursive or differs from retained evidence",
        )?;
    }
    Ok(InferenceEquivalenceAssessment {
        manifest_id: manifest.id()?,
        subject_sha256: subject_hash,
        origin: subject.origin,
        split: corpus.split,
        not_before_unix_ms: subject.not_before_unix_ms,
        expires_unix_ms: subject.expires_unix_ms,
        independent_annotations: corpus.cases.iter().all(|c| c.independent_annotations),
        evaluation_rights: corpus.cases.iter().all(|c| c.evaluation_rights),
        heldout_frozen: corpus.heldout_freeze.is_some(),
        declared_criteria_satisfied: criteria_satisfied,
        provider_placement_complete: true,
        resources_complete,
        review_conclusion: review.conclusion,
        reviewed_at_unix_ms: review.reviewed_at_unix_ms,
        review_identity: manifest.evidence_identity(K::Review)?.clone(),
        structurally_correlated: true,
        measurement_authenticated: false,
        scientific_qualification_claimed: false,
        report_only: true,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceEquivalenceState {
    Pending,
    Accepted,
    Withdrawn,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceBinding {
    pub dossier_id: ArtifactId,
    pub revision: u64,
}
impl InferenceEquivalenceBinding {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(self.dossier_id.as_str())?;
        check(
            self.revision > 0 && self.revision <= i64::MAX as u64,
            "equivalence binding revision invalid",
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceDecision {
    pub revision: u64,
    pub state: InferenceEquivalenceState,
    pub request_id: RequestId,
    pub actor_session: SessionId,
    pub operator: Option<String>,
    pub note: String,
    pub recorded_at_unix_ms: Option<u64>,
    pub review_identity: Option<ArtifactIdentity>,
}
impl InferenceEquivalenceDecision {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        check(
            self.revision >= 2
                && self.revision <= i64::MAX as u64
                && self.state != InferenceEquivalenceState::Pending,
            "equivalence decision revision/state invalid",
        )?;
        check(
            self.note.len() <= 4096
                && !self
                    .note
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\t'),
            "equivalence decision note invalid",
        )?;
        if let Some(at) = self.recorded_at_unix_ms {
            clock(at)?;
        }
        match self.state {
            InferenceEquivalenceState::Accepted => {
                text(
                    self.operator.as_deref().ok_or_else(|| {
                        ProtocolError::invalid("acceptance operator attribution absent")
                    })?,
                    256,
                )?;
                clock(
                    self.recorded_at_unix_ms
                        .ok_or_else(|| ProtocolError::invalid("acceptance timestamp absent"))?,
                )?;
                artifact(
                    self.review_identity.as_ref().ok_or_else(|| {
                        ProtocolError::invalid("acceptance review identity absent")
                    })?,
                )
            }
            InferenceEquivalenceState::Withdrawn => {
                text(&self.note, 4096)?;
                check(
                    self.operator.is_none() && self.review_identity.is_none(),
                    "withdrawal carries acceptance-only fields",
                )
            }
            InferenceEquivalenceState::Pending => unreachable!("rejected above"),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceReport {
    pub dossier_id: ArtifactId,
    pub revision: u64,
    pub state: InferenceEquivalenceState,
    pub manifest: InferenceEquivalenceManifest,
    pub decision: Option<InferenceEquivalenceDecision>,
    pub report_only: bool,
}
impl InferenceEquivalenceReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.manifest.validate()?;
        InferenceEquivalenceBinding {
            dossier_id: self.dossier_id.clone(),
            revision: self.revision,
        }
        .validate()?;
        check(
            self.report_only && self.dossier_id == self.manifest.id()?,
            "equivalence report identity or nonauthority boundary differs",
        )?;
        if self.state == InferenceEquivalenceState::Pending {
            return check(
                self.revision == 1 && self.decision.is_none(),
                "pending equivalence must be undecided revision one",
            );
        }
        let decision = self
            .decision
            .as_ref()
            .ok_or_else(|| ProtocolError::invalid("equivalence decision absent"))?;
        decision.validate()?;
        check(
            self.revision >= 2
                && decision.revision == self.revision
                && decision.state == self.state,
            "equivalence decision revision/state differs",
        )?;
        check(
            decision.note.len() <= 4096
                && !decision
                    .note
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\t'),
            "equivalence decision note invalid",
        )?;
        if let Some(at) = decision.recorded_at_unix_ms {
            clock(at)?;
        }
        match self.state {
            InferenceEquivalenceState::Accepted => {
                check(
                    self.manifest.subject.origin == InferenceEquivalenceOrigin::Measured,
                    "synthetic equivalence cannot be publicly accepted",
                )?;
                text(
                    decision.operator.as_deref().ok_or_else(|| {
                        ProtocolError::invalid(
                            "accepted equivalence requires supplied operator attribution",
                        )
                    })?,
                    256,
                )?;
                let at = decision.recorded_at_unix_ms.ok_or_else(|| {
                    ProtocolError::invalid("accepted equivalence requires a recorded decision time")
                })?;
                check(
                    at >= self.manifest.subject.not_before_unix_ms
                        && at < self.manifest.subject.expires_unix_ms,
                    "acceptance decision is outside declared validity",
                )?;
                check(
                    decision.review_identity.as_ref()
                        == Some(
                            self.manifest
                                .evidence_identity(InferenceEquivalenceEvidenceKind::Review)?,
                        ),
                    "acceptance decision must bind the exact retained review",
                )
            }
            InferenceEquivalenceState::Withdrawn => {
                text(&decision.note, 4096)?;
                check(
                    decision.operator.is_none() && decision.review_identity.is_none(),
                    "withdrawal must not invent an acceptance operator or review",
                )
            }
            InferenceEquivalenceState::Pending => unreachable!("handled above"),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceReceipt {
    pub input_sha256: Option<String>,
    pub request_id: RequestId,
    pub report: InferenceEquivalenceReport,
}
impl InferenceEquivalenceReceipt {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.report.validate()?;
        match &self.input_sha256 {
            // Reimport may observe an existing decision. Storage must create new
            // dossiers Pending and retain the original decision receipt linkage.
            Some(hash) => validate_sha256(hash),
            None => check(
                self.report
                    .decision
                    .as_ref()
                    .is_some_and(|d| d.request_id == self.request_id),
                "equivalence decision receipt request differs",
            ),
        }
    }
}

/// Fresh store observations are separate from the exact historical report.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalenceStatus {
    pub report: InferenceEquivalenceReport,
    pub withdrawal_pending: bool,
    pub clock_floor_unix_ms: Option<u64>,
    pub report_only: bool,
}
impl InferenceEquivalenceStatus {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.report.validate()?;
        check(self.report_only, "equivalence status is observation only")?;
        if let Some(floor) = self.clock_floor_unix_ms {
            clock(floor)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEquivalencePage {
    pub entries: Vec<InferenceEquivalenceStatus>,
    pub next_after: Option<ArtifactId>,
    pub report_only: bool,
}
impl InferenceEquivalencePage {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        check(
            self.report_only && self.entries.len() <= 16,
            "equivalence list scope or size invalid",
        )?;
        for entry in &self.entries {
            entry.validate()?;
        }
        check(
            self.entries.windows(2).all(|pair| {
                pair[0].report.dossier_id.as_str() < pair[1].report.dossier_id.as_str()
            }),
            "equivalence page IDs must be unique and ordered",
        )?;
        if let Some(cursor) = &self.next_after {
            validate_sha256(cursor.as_str())?;
            check(
                self.entries
                    .last()
                    .is_some_and(|entry| &entry.report.dossier_id == cursor),
                "equivalence cursor must identify the last returned report",
            )?;
        }
        check(
            encoded(self)?.len() <= INFERENCE_EQUIVALENCE_PAGE_LIMIT,
            "equivalence page exceeds the bounded control response",
        )
    }
}
pub fn validate_inference_equivalence_page_limit(limit: u16) -> Result<(), ProtocolError> {
    check(
        (1..=16).contains(&limit),
        "equivalence page limit must be 1..=16",
    )
}
pub fn validate_inference_equivalence_note(note: &str) -> Result<(), ProtocolError> {
    text(note, 4096)
}

#[cfg(test)]
#[path = "inference_equivalence_fixture.rs"]
mod fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use InferenceEquivalenceEvidenceDetails as D;
    use InferenceEquivalenceEvidenceKind as K;

    fn pending(manifest: InferenceEquivalenceManifest) -> InferenceEquivalenceReport {
        InferenceEquivalenceReport {
            dossier_id: manifest.id().unwrap(),
            revision: 1,
            state: InferenceEquivalenceState::Pending,
            manifest,
            decision: None,
            report_only: true,
        }
    }
    fn replace_document(
        manifest: &mut InferenceEquivalenceManifest,
        bytes: &mut BTreeMap<K, Vec<u8>>,
        kind: K,
        value: &InferenceEquivalenceEvidenceDocument,
    ) {
        let raw = encoded(value).unwrap();
        let sha256 = hash(&raw);
        manifest
            .evidence
            .iter_mut()
            .find(|v| v.kind == kind)
            .unwrap()
            .identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: raw.len() as u64,
        };
        bytes.insert(kind, raw);
    }

    #[test]
    fn synthetic_correlation_is_not_measurement_or_acceptance() {
        let (manifest, bytes) = fixture::synthetic_fixture();
        let assessment = validate_evidence(&manifest, &bytes).unwrap();
        assert!(assessment.declared_criteria_satisfied());
        assert_eq!(assessment.manifest_id(), &manifest.id().unwrap());
        assert!(assessment
            .require_acceptance(3000)
            .unwrap_err()
            .message
            .contains("synthetic"));
        let view = serde_json::to_value(&assessment).unwrap();
        assert_eq!(view["structurally_correlated"], true);
        assert_eq!(view["measurement_authenticated"], false);
        assert_eq!(view["scientific_qualification_claimed"], false);
        assert_eq!(view["report_only"], true);
        pending(manifest).validate().unwrap();
    }

    #[test]
    fn same_model_restriction_prevents_ontology_assumptions_from_class_count() {
        let (manifest, _) = fixture::synthetic_fixture();
        let mut subject = manifest.subject;
        subject.to_profile.model.sha256 = "b".repeat(64);
        assert!(subject
            .validate()
            .unwrap_err()
            .message
            .contains("class_count alone"));
        for (name, tier) in [
            ("fast", InferenceEquivalenceTier::Fast),
            ("economy", InferenceEquivalenceTier::Fast),
            ("default", InferenceEquivalenceTier::Default),
            ("maximum", InferenceEquivalenceTier::Maximum),
            ("quality", InferenceEquivalenceTier::Maximum),
        ] {
            assert_eq!(InferenceEquivalenceTier::from_preset(name).unwrap(), tier);
        }
        assert!(InferenceEquivalenceTier::from_preset("automatic").is_err());
    }

    #[test]
    fn correlated_documents_refuse_source_target_settings_placement_and_resource_drift() {
        for case in 0..7 {
            let (manifest, bytes) = fixture::synthetic_fixture();
            let mut documents = fixture::fixture_documents(&bytes);
            match case {
                0..=3 => {
                    let D::ToExecution(run) = documents.get_mut(&K::ToExecution).unwrap() else {
                        unreachable!()
                    };
                    match case {
                        0 => run.cases[0].manifest.source.identity.sha256 = "c".repeat(64),
                        1 => run.cases[0].target_id = "different-target".into(),
                        2 => run.cases[0].window_ns.end_ns += 1,
                        _ => {
                            let WorkerOperation::Generate { settings, .. } =
                                &mut run.cases[0].manifest.operation
                            else {
                                unreachable!()
                            };
                            settings.source_sample_stride += 1;
                        }
                    }
                }
                4 => {
                    let D::QualityComparison(value) =
                        documents.get_mut(&K::QualityComparison).unwrap()
                    else {
                        unreachable!()
                    };
                    value.cases[0].values.clear();
                }
                5 => {
                    let D::ProviderPlacement(value) =
                        documents.get_mut(&K::ProviderPlacement).unwrap()
                    else {
                        unreachable!()
                    };
                    value.cases[0].to.executed_providers = vec![InferenceProvider::Cuda];
                }
                _ => {
                    let D::Resources(value) = documents.get_mut(&K::Resources).unwrap() else {
                        unreachable!()
                    };
                    value.cases[0].to_elapsed_ns += 1;
                }
            }
            let (changed, bytes) = fixture::seal_synthetic(manifest.subject, documents);
            assert!(validate_evidence(&changed, &bytes).is_err(), "case {case}");
        }
    }

    #[test]
    fn supplied_bounds_are_calculated_without_filling_missing_measurements() {
        let (manifest, bytes) = fixture::synthetic_fixture();
        let mut documents = fixture::fixture_documents(&bytes);
        let D::QualityComparison(value) = documents.get_mut(&K::QualityComparison).unwrap() else {
            unreachable!()
        };
        value.cases[0].values[0].to_value = 2.0;
        let (manifest, bytes) = fixture::seal_synthetic(manifest.subject, documents);
        let assessment = validate_evidence(&manifest, &bytes).unwrap();
        assert!(!assessment.declared_criteria_satisfied());
        assert!(assessment.require_acceptance(3000).is_err());
    }

    #[test]
    fn bytes_header_and_review_hash_tampering_are_not_accepted() {
        let (manifest, mut bytes) = fixture::synthetic_fixture();
        bytes.get_mut(&K::Corpus).unwrap().push(b' ');
        assert!(validate_evidence(&manifest, &bytes).is_err());
        let (mut manifest, mut bytes) = fixture::synthetic_fixture();
        let mut document: InferenceEquivalenceEvidenceDocument =
            serde_json::from_slice(&bytes[&K::Review]).unwrap();
        let D::Review(review) = &mut document.details else {
            unreachable!()
        };
        review.reviewed_evidence[0].sha256 = "d".repeat(64);
        replace_document(&mut manifest, &mut bytes, K::Review, &document);
        assert!(validate_evidence(&manifest, &bytes)
            .unwrap_err()
            .message
            .contains("review reference"));
        let (manifest, bytes) = fixture::synthetic_fixture();
        let mut value: serde_json::Value = serde_json::from_slice(&bytes[&K::Corpus]).unwrap();
        value["accepted"] = true.into();
        assert!(serde_json::from_value::<InferenceEquivalenceEvidenceDocument>(value).is_err());
        let mut missing = bytes;
        missing.remove(&K::Resources);
        assert!(validate_evidence(&manifest, &missing).is_err());
    }

    #[test]
    fn pending_and_clockless_withdrawal_preserve_history_but_synthetic_acceptance_refuses() {
        let (manifest, _) = fixture::synthetic_fixture();
        let report = pending(manifest);
        let mut withdrawn = report.clone();
        withdrawn.revision = 2;
        withdrawn.state = InferenceEquivalenceState::Withdrawn;
        withdrawn.decision = Some(InferenceEquivalenceDecision {
            revision: 2,
            state: InferenceEquivalenceState::Withdrawn,
            request_id: RequestId::new("authored-withdrawal").unwrap(),
            actor_session: SessionId::new("authored-session").unwrap(),
            operator: None,
            note: "Authored withdrawal without an available clock".into(),
            recorded_at_unix_ms: None,
            review_identity: None,
        });
        withdrawn.validate().unwrap();
        InferenceEquivalenceReceipt {
            input_sha256: Some("a".repeat(64)),
            request_id: RequestId::new("authored-reimport").unwrap(),
            report: withdrawn.clone(),
        }
        .validate()
        .unwrap();
        let receipt = InferenceEquivalenceReceipt {
            input_sha256: None,
            request_id: RequestId::new("authored-withdrawal").unwrap(),
            report: withdrawn,
        };
        receipt.validate().unwrap();
        let mut invalid = report;
        invalid.revision = 2;
        invalid.state = InferenceEquivalenceState::Accepted;
        invalid.decision = Some(InferenceEquivalenceDecision {
            revision: 2,
            state: InferenceEquivalenceState::Accepted,
            request_id: RequestId::new("invalid-acceptance").unwrap(),
            actor_session: SessionId::new("authored-session").unwrap(),
            operator: Some("Invalid synthetic acceptance fixture".into()),
            note: String::new(),
            recorded_at_unix_ms: Some(3000),
            review_identity: Some(
                invalid
                    .manifest
                    .evidence_identity(K::Review)
                    .unwrap()
                    .clone(),
            ),
        });
        assert!(invalid
            .validate()
            .unwrap_err()
            .message
            .contains("synthetic"));
    }

    #[test]
    fn status_and_pages_are_bounded_observations_not_mutable_reports() {
        let (manifest, _) = fixture::synthetic_fixture();
        let report = pending(manifest);
        let status = InferenceEquivalenceStatus {
            report: report.clone(),
            withdrawal_pending: true,
            clock_floor_unix_ms: None,
            report_only: true,
        };
        status.validate().unwrap();
        let page = InferenceEquivalencePage {
            entries: vec![status],
            next_after: Some(report.dossier_id.clone()),
            report_only: true,
        };
        page.validate().unwrap();
        assert!(validate_inference_equivalence_page_limit(0).is_err());
        assert!(validate_inference_equivalence_page_limit(17).is_err());
        validate_inference_equivalence_page_limit(16).unwrap();
        let mut invalid = page.clone();
        invalid.entries.push(invalid.entries[0].clone());
        assert!(invalid.validate().is_err());
        let mut invalid = page;
        invalid.report_only = false;
        assert!(invalid.validate().is_err());
        assert_eq!(
            encoded(&report).unwrap(),
            encoded(&invalid.entries[0].report).unwrap()
        );
    }
}
