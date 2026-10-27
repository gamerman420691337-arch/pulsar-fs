//! Bounded container declarations, never proof of successful decoding or quality.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaStreamInfo {
    pub index: u32,
    pub kind: String,
    pub codec: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
    pub frame_rate: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaProbeReport {
    pub schema_version: u16,
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub source_version: SourceVersionId,
    pub source: ArtifactIdentity,
    pub format: Option<String>,
    pub declared_duration_seconds: Option<f64>,
    pub streams: Vec<MediaStreamInfo>,
    pub decoded: bool,
    pub qualified: bool,
}
impl MediaProbeReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.source.validate()?;
        if self.schema_version != 1
            || self.decoded
            || self.qualified
            || self.streams.is_empty()
            || self.streams.len() > 64
            || self
                .declared_duration_seconds
                .is_some_and(|v| !v.is_finite() || v < 0.0)
        {
            return Err(ProtocolError::invalid(
                "media probe report bounds or decoding claim",
            ));
        }
        fn label(s: &str) -> bool {
            !s.is_empty()
                && s.len() <= 128
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-,.".contains(&b))
        }
        if self.format.as_deref().is_some_and(|v| !label(v)) {
            return Err(ProtocolError::invalid("media probe format label"));
        }
        let mut previous = None;
        for stream in &self.streams {
            if !label(&stream.kind)
                || stream.codec.as_deref().is_some_and(|v| !label(v))
                || previous.is_some_and(|i| stream.index <= i)
                || stream.width.is_some() != stream.height.is_some()
                || stream.width == Some(0)
                || stream.height == Some(0)
                || stream.sample_rate.is_some_and(|v| v == 0 || v > 10_000_000)
                || stream.channels.is_some_and(|v| v == 0 || v > 1024)
                || stream
                    .frame_rate
                    .is_some_and(|v| !v.is_finite() || v <= 0.0)
            {
                return Err(ProtocolError::invalid("media probe stream bounds or order"));
            }
            previous = Some(stream.index);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report() -> MediaProbeReport {
        MediaProbeReport {
            schema_version: 1,
            project_id: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(0),
            source_version: SourceVersionId::new("source").unwrap(),
            source: ArtifactIdentity {
                artifact_id: ArtifactId::new("source").unwrap(),
                sha256: "a".repeat(64),
                byte_len: 1,
            },
            format: Some("matroska,webm".into()),
            declared_duration_seconds: Some(1.0),
            decoded: false,
            qualified: false,
            streams: vec![MediaStreamInfo {
                index: 0,
                kind: "video".into(),
                codec: Some("ffv1".into()),
                width: Some(64),
                height: Some(64),
                sample_rate: None,
                channels: None,
                frame_rate: Some(8.0),
            }],
        }
    }
    #[test]
    fn media_probe_declarations_refuse_claims_ambiguous_dimensions_and_unbounded_fields() {
        report().validate().unwrap();
        for case in 0..10 {
            let mut value = report();
            match case {
                0 => value.decoded = true,
                1 => value.qualified = true,
                2 => value.declared_duration_seconds = Some(f64::NAN),
                3 => value.declared_duration_seconds = Some(-1.0),
                4 => value.streams[0].height = None,
                5 => value.streams[0].width = Some(0),
                6 => value.streams[0].frame_rate = Some(f64::INFINITY),
                7 => value.streams.push(value.streams[0].clone()),
                8 => value.streams[0].codec = Some("untrusted\nlabel".into()),
                _ => value.streams = vec![],
            }
            assert!(value.validate().is_err(), "case {case}");
        }
        let mut json = serde_json::to_value(report()).unwrap();
        json["authorized"] = true.into();
        assert!(serde_json::from_value::<MediaProbeReport>(json).is_err());
    }
    #[test]
    fn media_probe_command_requires_read_scope_coordinates_without_mutation() {
        let command = Command::ProbeMedia {
            source_version: SourceVersionId::new("source").unwrap(),
        };
        assert!(command.requires_project());
        assert!(command.requires_revision());
        assert!(!command.is_mutating());
        let mut req = Request::new(RequestId::new("request").unwrap(), command);
        assert!(req.validate().is_err());
        req.project = Some(ProjectId::new("project").unwrap());
        assert!(req.validate().is_err());
        req.expected_revision = Some(RevisionId::new(0));
        req.validate().unwrap();
    }
}
