//! Bounded observations of companion paths, never authority to replace a file.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::{Axis, MAX_LIBRARY_BATCH_ITEMS};
use serde::Serialize;
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    Present,
    Missing,
    Blocked,
}

#[derive(Debug, Serialize)]
pub struct Observation {
    pub path: PathBuf,
    pub presence: Presence,
    pub bytes: Option<u64>,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Channel {
    pub axis: Axis,
    pub presence: Presence,
    pub observations: Vec<Observation>,
}

#[derive(Debug, Serialize)]
pub struct Entry {
    pub source: PathBuf,
    pub channels: Vec<Channel>,
}

#[derive(Debug, Serialize)]
pub struct Inventory {
    pub schema: u16,
    pub folder: PathBuf,
    pub recursive: bool,
    pub entries: Vec<Entry>,
    pub present_channels: usize,
    pub missing_channels: usize,
    pub blocked_channels: usize,
    pub qualified: bool,
}

fn observe(path: PathBuf) -> Observation {
    let (presence, bytes, reason) = match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => (Presence::Present, Some(meta.len()), None),
        Ok(_) => (Presence::Blocked, None, Some("not a regular file".into())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => (Presence::Missing, None, None),
        Err(error) => (
            Presence::Blocked,
            None,
            Some(format!("metadata: {:?}", error.kind())),
        ),
    };
    Observation {
        path,
        presence,
        bytes,
        reason,
    }
}

fn bundle_observation(bundle: &Path, axis: Axis) -> Observation {
    let path = bundle.join(format!("{}.funscript", axis.as_str()));
    match std::fs::symlink_metadata(bundle) {
        Ok(meta) if meta.is_dir() => observe(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Observation {
            path,
            presence: Presence::Missing,
            bytes: None,
            reason: None,
        },
        result => Observation {
            path,
            presence: Presence::Blocked,
            bytes: None,
            reason: Some(match result {
                Ok(_) => "bundle is not a directory".into(),
                Err(error) => format!("bundle metadata: {:?}", error.kind()),
            }),
        },
    }
}

/// Recognizes adjacent stroke and Pulsar's neutral audio/visual bundles only.
/// Presence is a mutable filesystem observation, not usable-motion validation.
pub fn inspect(folder: &Path, recursive: bool) -> Result<Inventory> {
    let folder = crate::absolute_path(folder)?;
    ensure!(
        std::fs::symlink_metadata(&folder)?.is_dir(),
        "Choose a directory, not a symlink"
    );
    let discovered = crate::library_entries(&folder, recursive, true)?;
    ensure!(
        discovered.len() <= MAX_LIBRARY_BATCH_ITEMS,
        "Axis inventory exceeds 100 media entries; choose a smaller folder"
    );
    let mut inventory = Inventory {
        schema: 1,
        folder,
        recursive,
        entries: Vec::new(),
        present_channels: 0,
        missing_channels: 0,
        blocked_channels: 0,
        qualified: false,
    };
    for entry in discovered {
        let mut visual_name = entry
            .video
            .file_name()
            .context("Media filename absent")?
            .to_os_string();
        visual_name.push(".pulsar-visual-bundle");
        let visual = entry.video.with_file_name(visual_name);
        let channels = Axis::ALL
            .into_iter()
            .map(|axis| {
                let mut observations = vec![bundle_observation(&entry.script, axis)];
                if axis == Axis::Stroke {
                    observations.push(observe(entry.video.with_extension("funscript")));
                    observations.push(bundle_observation(&visual, axis));
                }
                // A blocker remains visible even if another companion is present.
                let presence = if observations.iter().any(|o| o.presence == Presence::Blocked) {
                    Presence::Blocked
                } else if observations.iter().any(|o| o.presence == Presence::Present) {
                    Presence::Present
                } else {
                    Presence::Missing
                };
                match presence {
                    Presence::Present => inventory.present_channels += 1,
                    Presence::Missing => inventory.missing_channels += 1,
                    Presence::Blocked => inventory.blocked_channels += 1,
                }
                Channel {
                    axis,
                    presence,
                    observations,
                }
            })
            .collect();
        inventory.entries.push(Entry {
            source: entry.video,
            channels,
        });
    }
    Ok(inventory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_stroke_does_not_hide_missing_secondary_channels_or_change_bytes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("clip.mp4"), b"media").unwrap();
        let protected = dir.path().join("clip.funscript");
        std::fs::write(&protected, b"authored, unvalidated").unwrap();
        let report = inspect(dir.path(), false).unwrap();
        assert_eq!(
            (
                report.present_channels,
                report.missing_channels,
                report.blocked_channels
            ),
            (1, 5, 0)
        );
        assert_eq!(report.entries[0].channels[0].presence, Presence::Present);
        assert!(!report.qualified);
        assert_eq!(std::fs::read(protected).unwrap(), b"authored, unvalidated");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn partial_bundle_reports_present_missing_and_blocked_without_adopting_content() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("clip.wav"), b"audio").unwrap();
        let bundle = dir.path().join("clip.wav.pulsar-audio-bundle");
        std::fs::create_dir(&bundle).unwrap();
        std::fs::write(bundle.join("roll.funscript"), b"").unwrap();
        std::fs::create_dir(bundle.join("pitch.funscript")).unwrap();
        let report = inspect(dir.path(), false).unwrap();
        assert_eq!(
            (
                report.present_channels,
                report.missing_channels,
                report.blocked_channels
            ),
            (1, 4, 1)
        );
        let roll = report.entries[0]
            .channels
            .iter()
            .find(|c| c.axis == Axis::Roll)
            .unwrap();
        assert_eq!(roll.observations[0].bytes, Some(0));
    }

    #[test]
    fn bounded_recursive_discovery_and_invalid_root_refuse_without_writes() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("child");
        std::fs::create_dir(&child).unwrap();
        std::fs::write(child.join("clip.mp4"), b"media").unwrap();
        assert!(inspect(dir.path(), false).unwrap().entries.is_empty());
        assert_eq!(inspect(dir.path(), true).unwrap().entries.len(), 1);
        assert!(inspect(&child.join("clip.mp4"), false).is_err());
        for n in 0..100 {
            std::fs::write(child.join(format!("{n}.wav")), b"audio").unwrap();
        }
        assert!(inspect(dir.path(), true).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_bundle_and_leaf_block_instead_of_becoming_missing_channels() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("clip.mp4"), b"media").unwrap();
        let bundle = dir.path().join("clip.mp4.pulsar-audio-bundle");
        symlink(external.path(), &bundle).unwrap();
        let report = inspect(dir.path(), false).unwrap();
        assert_eq!(report.blocked_channels, 6);
        std::fs::remove_file(&bundle).unwrap();
        std::fs::create_dir(&bundle).unwrap();
        symlink(
            external.path().join("absent"),
            bundle.join("roll.funscript"),
        )
        .unwrap();
        let report = inspect(dir.path(), false).unwrap();
        assert_eq!((report.missing_channels, report.blocked_channels), (5, 1));
        assert!(std::fs::read_dir(external.path()).unwrap().next().is_none());
    }
}
