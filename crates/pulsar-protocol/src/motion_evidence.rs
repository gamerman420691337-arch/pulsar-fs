//! Read-only association of neutral playback with a full portable project.
//! Hashes and historical receipts establish content integrity, never authenticity,
//! device suitability, model qualification, or permission to execute anything.
use crate::{project_package_format::*, *};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    io::{self, Read, Write},
    rc::Rc,
};

#[derive(Clone, Debug, Serialize)]
pub struct MotionEvidenceAssociation {
    pub schema_version: u16,
    pub origin_project_id: ProjectId,
    pub revision: RevisionId,
    pub axis: Axis,
    pub script_sha256: String,
    pub script_byte_len: u64,
    pub package_sha256: String,
    pub package_byte_len: u64,
    pub package_format_version: u16,
    pub master: ProgramDescriptor,
    pub records: PackageRecordCounts,
    pub verification: &'static str,
    pub execution_authority: bool,
    pub qualified: bool,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct NeutralScript {
    version: String,
    inverted: bool,
    range: u32,
    actions: Vec<NeutralAction>,
}
#[derive(Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct NeutralAction {
    at: i64,
    pos: u32,
}

struct SelectedSink {
    bytes: Option<Rc<RefCell<Vec<u8>>>>,
    remaining: u64,
}
impl Write for SelectedSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(io::Error::other(
                "selected motion sink exceeded its declared bound",
            ));
        }
        if let Some(captured) = &self.bytes {
            let mut captured = captured.borrow_mut();
            captured
                .try_reserve(bytes.len())
                .map_err(|_| io::Error::other("motion allocation refused"))?;
            captured.extend_from_slice(bytes);
        }
        self.remaining -= bytes.len() as u64;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn require(condition: bool, detail: &str) -> Result<(), ProtocolError> {
    if condition {
        Ok(())
    } else {
        Err(ProtocolError::invalid(detail))
    }
}

/// Validates all container bytes and objects, retains only the bounded current
/// master, and checks the selected-axis neutral action mapping. Other package
/// objects are hashed and discarded. Historical paths are never followed.
/// Callers own I/O deadlines/cancellation; success is not an import qualification.
pub fn verify_motion_evidence<R: Read>(
    script: &[u8],
    package: &mut R,
    project: &ProjectId,
    revision: RevisionId,
    axis: Axis,
    limits: PackageCodecLimits,
) -> Result<MotionEvidenceAssociation, ProtocolError> {
    require(
        !script.is_empty() && script.len() as u64 <= MAX_MOTION_BYTES,
        "neutral script exceeds the bounded motion admission",
    )?;
    let script_hash = format!("{:x}", Sha256::digest(script));
    let script_value: NeutralScript = serde_json::from_slice(script)
        .map_err(|_| ProtocolError::invalid("invalid strict neutral funscript"))?;
    require(
        script_value.version == "1.0" && !script_value.inverted && script_value.range == 90,
        "neutral playback header differs from the standard axis export",
    )?;
    require(
        !script_value.actions.is_empty() && script_value.actions.len() as u64 <= MAX_MOTION_ACTIONS,
        "neutral playback action count is outside admission",
    )?;
    let master_bytes = Rc::new(RefCell::new(Vec::new()));
    let captured = master_bytes.clone();
    let decoded =
        read_package_with_manifest(
            package,
            |manifest, object| {
                require(
                    &manifest.origin_project_id == project
                        && manifest.captured_revision == revision
                        && manifest.head.revision == revision,
                    "package project/revision differs from the requested association",
                )?;
                require(manifest.export_receipts.iter().any(|receipt| receipt.revision == revision
            && receipt.axis == PackageExportScope::Axis(axis) && receipt.sha256 == script_hash
            && matches!(receipt.state, PackageExportState::Complete)),
            "package has no completed receipt for these exact script bytes, revision and axis")?;
                let selected = object.sha256 == manifest.head.motion.sha256;
                if selected {
                    require(
                        object.byte_len <= MAX_MOTION_BYTES
                            && object.byte_len == manifest.head.motion.byte_len,
                        "current master exceeds its motion admission",
                    )?;
                }
                Ok(SelectedSink {
                    bytes: selected.then(|| captured.clone()),
                    remaining: object.byte_len,
                })
            },
            limits,
        )?;
    let master = master_bytes.borrow();
    let program: MotionProgram = serde_json::from_slice(&master).map_err(|_| {
        ProtocolError::invalid("package current master is not a valid motion program")
    })?;
    // Descriptor summaries must describe the actual typed object, not only hash bytes.
    let axes: Vec<_> = program
        .tracks()
        .iter()
        .map(|track| AxisSummary {
            axis: track.axis(),
            action_count: track.actions().len() as u64,
            gap_count: track.gaps().len() as u64,
        })
        .collect();
    require(
        axes == decoded.manifest.head.motion.axes,
        "current master axis summaries differ from its object",
    )?;
    let track = program
        .track(axis)
        .ok_or_else(|| ProtocolError::invalid("selected axis is absent from current master"))?;
    require(
        track.gaps().is_empty() && !track.actions().is_empty(),
        "selected axis has unresolved gaps or no actions",
    )?;
    require(
        track.actions().len() == script_value.actions.len(),
        "neutral action count differs from selected master axis",
    )?;
    let mut previous = None;
    for (action, playback) in track.actions().iter().zip(&script_value.actions) {
        let ns = action.time().as_nanos();
        require(
            ns >= 0,
            "neutral playback cannot encode negative project time",
        )?;
        let ms = ns
            .checked_add(500_000)
            .ok_or_else(|| ProtocolError::invalid("neutral timestamp overflow"))?
            / 1_000_000;
        require(
            !previous.is_some_and(|last| last >= ms),
            "millisecond quantization merges ordered master actions",
        )?;
        previous = Some(ms);
        require(
            playback.at == ms && playback.pos == (action.position().value() * 100.0).round() as u32,
            "neutral playback actions differ from the selected master axis",
        )?;
    }
    let manifest = decoded.manifest;
    Ok(MotionEvidenceAssociation {
        schema_version: 1, origin_project_id: manifest.origin_project_id, revision, axis,
        script_sha256: script_hash, script_byte_len: script.len() as u64,
        package_sha256: decoded.digest.sha256, package_byte_len: decoded.digest.byte_len,
        package_format_version: manifest.format_version, master: manifest.head.motion,
        records: manifest.counts,
        verification: "all_package_objects_and_completed_export_receipt_and_selected_master_actions_integrity_only",
        execution_authority: false, qualified: false,
    })
}
