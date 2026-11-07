//! Checked domain values and deterministic state transitions.
//!
//! This crate does not discover hardware, read files, spawn workers, grant
//! authority, or acknowledge durability. The engine supplies validated facts
//! and performs effects only after these transitions succeed. These kernels
//! are not a claim of physical-device or neural-model qualification.
#![forbid(unsafe_code)]

mod adaptation;
mod authority;
mod device;
mod edit_recipes;
mod edit_values;
pub use adaptation::*;
pub use edit_recipes::*;
mod dsp;
mod geometry;
mod video_view;
mod view_coordinates;
pub use video_view::*;
pub use view_coordinates::*;
mod handy_stream;
mod handy_stream_envelope;
mod ids;
mod jobs;
mod live;
mod live_clock;
pub use handy_stream::*;
pub use handy_stream_envelope::*;
mod live_motion_assembly;
pub use live_motion_assembly::*;
mod gap_resolution;
pub use gap_resolution::*;
mod browser_clock;
mod browser_start;
mod live_clock_probe;
mod live_decode_clock;
pub use browser_clock::*;
pub use browser_start::*;
pub use live::*;
pub use live_clock::*;
pub use live_clock_probe::*;
pub use live_decode_clock::*;
mod motion;
mod multimodal;
pub use multimodal::*;
mod script_repair;
pub use script_repair::*;
mod audio_oscillation;
pub use audio_oscillation::*;
mod audio_analysis;
pub mod perception;
pub use audio_analysis::*;
mod synthesis;
pub use synthesis::*;
mod segment_speed;
pub use segment_speed::*;
mod editing;
pub use editing::*;
mod declared_source_clock;
mod generation_interval;
mod placed_generation;
mod project;
mod reference_curve;
pub use reference_curve::*;
mod source_placement;
pub use declared_source_clock::*;
mod source_seek;
mod time;
pub use generation_interval::*;
mod motion_retiming;
mod source_clock_motion;
mod source_clock_selection;
pub use motion_retiming::*;
pub use placed_generation::*;
pub use source_clock_motion::*;
pub use source_clock_selection::*;
pub use source_placement::*;
pub use source_seek::*;

pub use authority::*;
pub use device::*;
pub use dsp::*;
pub use edit_values::*;
pub use geometry::*;
pub use ids::*;
pub use jobs::*;
pub use motion::*;
pub use project::*;
pub use time::*;

#[cfg(kani)]
mod proofs;

/// Errors are domain failures, not opaque runtime or transport failures.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CoreError {
    #[error("identifier must contain 1..=128 ASCII letters, digits, '.', '_', ':', or '-'")]
    InvalidIdentifier,
    #[error("checked arithmetic overflow")]
    Overflow,
    #[error("rational timestamp denominator must be nonzero")]
    InvalidTimebase,
    #[error("invalid source-frame seek: {0}")]
    InvalidSourceSeek(String),
    #[error("value must be finite")]
    NonFinite,
    #[error("normalized value must be in [0, 1]")]
    OutOfRange,
    #[error("invalid coordinate geometry: {0}")]
    InvalidGeometry(String),
    #[error("invalid motion: {0}")]
    InvalidMotion(String),
    #[error("permission denied")]
    PermissionDenied,
    #[error("revision conflict: expected {expected}, actual {actual}")]
    RevisionConflict { expected: u64, actual: u64 },
    #[error("edit intersects a protected region")]
    ProtectedRegion,
    #[error("invalid state transition: {0}")]
    InvalidTransition(String),
    #[error("candidate belongs to a different project")]
    CandidateMismatch,
    #[error("no undo or redo entry exists")]
    EmptyHistory,
    #[error("qualification does not match the exact device configuration")]
    MissingQualification,
    #[error("device session or controller does not match")]
    DeviceAuthorityMismatch,
    #[error("headless playback requires an approved, qualified stopping bound")]
    MissingStoppingBound,
}
