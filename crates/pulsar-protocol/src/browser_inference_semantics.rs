//! Browser applicability preserves requested semantics across fresh epochs.
//! Hash equality does not establish native source custody or accepted equivalence.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserInferenceFallbackViewPolicy {
    pub schema_version: u16,
    pub namespace: BrowserInferenceFallbackSourceNamespace,
    pub applicability: InferenceFallbackViewApplicability,
}
impl BrowserInferenceFallbackViewPolicy {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_view_applicability(self.schema_version, &self.applicability)?;
        encoded(self).map(|_| ())
    }
    pub fn identity(&self) -> Result<ArtifactIdentity, ProtocolError> {
        self.validate()?;
        identity(self)
    }
    pub fn matches_capture(&self, options: &LiveCaptureOptions) -> Result<bool, ProtocolError> {
        self.validate()?;
        let basis = browser_inference_semantic_identity(options)?;
        Ok(match &self.applicability {
            InferenceFallbackViewApplicability::PreserveRequestedSemantics => true,
            InferenceFallbackViewApplicability::ExactSettings { sha256 } => {
                sha256.contains(&basis.sha256)
            }
        })
    }
}

fn object(
    value: &mut serde_json::Value,
) -> Result<&mut serde_json::Map<String, serde_json::Value>, ProtocolError> {
    value.as_object_mut().ok_or_else(|| {
        ProtocolError::invalid("browser semantic projection requires typed capture objects")
    })
}

/// Preserve the complete typed capture declaration, removing only execution
/// selection and per-epoch coordinates. Native admission must independently
/// establish new capabilities, the original deadline, and an accepted profile.
///
/// Model content, pack choice, boot/project/revision, exact source placement and
/// declared source clock, player identity, lookahead, age and buffer policy are
/// retained. Source/project anchors may advance only along that same validated
/// mapping. No inferred tier, view, target, axis or device permission is added.
pub fn browser_inference_semantic_identity(
    options: &LiveCaptureOptions,
) -> Result<ArtifactIdentity, ProtocolError> {
    options.validate()?;
    let selected = options
        .inference
        .as_ref()
        .ok_or_else(|| ProtocolError::invalid("browser semantics require explicit inference"))?;
    check(
        options.browser_media.is_some()
            && options.browser_analysis.is_some()
            && options.encoded_source.is_none()
            && options.source_time_basis == LiveSourceTimeBasis::SourceMediaPresentation
            && selected.profile.supports_live_temporal_input(),
        "browser semantics require bound browser temporal inference",
    )?;

    // Start from the full serialization so new settings are preserved unless
    // deliberately classified here. Do not normalize a caller's arbitrary JSON.
    let mut preserved =
        serde_json::to_value(options).map_err(|error| ProtocolError::invalid(error.to_string()))?;
    let capture = object(&mut preserved)?;
    for field in [
        "inference",
        "continuation_mode",
        "lease_ms",
        "source_anchor",
        "project_anchor",
        "exact_project_anchor",
        "presentation_anchor",
    ] {
        capture.remove(field);
    }
    for (field, transient) in [
        (
            "browser_media",
            &["preparation_request_id", "lease_id", "lease_ms"][..],
        ),
        ("browser_analysis", &["feed_id", "timing_binding_id"][..]),
    ] {
        let nested = capture.get_mut(field).ok_or_else(|| {
            ProtocolError::invalid("browser semantic projection lost its declared source")
        })?;
        let nested = object(nested)?;
        for name in transient {
            nested.remove(*name);
        }
    }
    #[derive(Serialize)]
    struct Basis<'a> {
        schema_version: u16,
        namespace: BrowserInferenceFallbackSourceNamespace,
        model: &'a ArtifactIdentity,
        capture: serde_json::Value,
    }
    identity(&Basis {
        schema_version: 1,
        namespace: BrowserInferenceFallbackSourceNamespace::BrowserMediaCapture,
        model: &selected.profile.model,
        capture: preserved,
    })
}

/// Compare declared semantics only. This does not permit a profile switch,
/// revive an expired epoch, or substitute for native predecessor settlement.
pub fn validate_browser_inference_semantic_successor(
    previous: &LiveCaptureOptions,
    next: &LiveCaptureOptions,
) -> Result<(), ProtocolError> {
    check(
        browser_inference_semantic_identity(previous)?
            == browser_inference_semantic_identity(next)?,
        "browser successor changed original source, model, player, mapping or analysis policy",
    )
}
