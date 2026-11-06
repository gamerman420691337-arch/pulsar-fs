//! Folder consumers author the same checked generation contract as single files.
use crate::motion::ResponseBody;
use crate::{absolute_path, cli::LibraryGenerationOptions, inference, EngineApi};
use anyhow::{Context, Result};
use pulsar_protocol::*;
use std::path::{Path, PathBuf};

pub(crate) fn is_default(options: &LibraryGenerationOptions) -> bool {
    options.audio_bundle_recipe.is_none() && visual_defaults(options)
}
fn visual_defaults(options: &LibraryGenerationOptions) -> bool {
    options.model.is_none()
        && options.model_pack.is_none()
        && options.model_class_count.is_none()
        && options.inference_profile.is_none()
        && options.conf.is_none()
        && options.width.is_none()
        && options.height.is_none()
        && options.video_view.is_none()
        && options.preset == "default"
        && options.source_sample_stride == 1
}

pub(crate) fn configure(
    options: &LibraryGenerationOptions,
) -> Result<(GenerationSettings, Option<PathBuf>)> {
    anyhow::ensure!(
        options.model.is_some()
            || (options.model_class_count.is_none()
                && options.inference_profile.is_none()
                && (options.conf.is_none() || options.model_pack.is_some())),
        "Model declarations require --model; threshold requires --model or --model-pack"
    );
    anyhow::ensure!(
        options.model_class_count.is_none() || options.inference_profile.is_none(),
        "Choose class count or a declared inference profile"
    );
    anyhow::ensure!(
        options.model_pack.is_none()
            || (options.model.is_none()
                && options.model_class_count.is_none()
                && options.inference_profile.is_none()),
        "Selected packs cannot be combined with raw model declarations"
    );
    if let Some(digest) = &options.model_pack {
        anyhow::ensure!(
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "Model pack digest must be 64 lowercase hexadecimal characters"
        );
    }
    let profile = options
        .inference_profile
        .as_deref()
        .map(inference::read_profile)
        .transpose()?;
    let input = if let Some(profile) = &profile {
        Some(profile.preprocessing.input.clone())
    } else if options.model.is_some() {
        Some(ModelInputContract {width:options.width.unwrap_or(640),height:options.height.unwrap_or(640),channels:3,layout:TensorLayout::Nchw,class_count:options.model_class_count.context("Custom folder models require --model-class-count or --inference-profile; tensor layout is never guessed")?,decoder:DetectorDecoder::YoloChannelMajor})
    } else {
        None
    };
    let (analysis_width, analysis_height) =
        inference::generation_analysis_dimensions(input.as_ref(), options.width, options.height);
    let video_view = options
        .video_view
        .as_deref()
        .map(|path| -> Result<VideoView> {
            Ok(serde_json::from_slice(&inference::read_bounded_profile(
                path,
            )?)?)
        })
        .transpose()?;
    let settings = GenerationSettings {
        detector_score_threshold: if options.model_pack.is_some() {
            None
        } else {
            options.conf
        },
        video_view,
        inference_profile: profile,
        model_input: input,
        analysis_width,
        analysis_height,
        source_sample_stride: options.source_sample_stride,
        ..Default::default()
    };
    if options.model_pack.is_some() {
        settings.validate()?;
    } else {
        settings.validate_execution_preset(&options.preset, options.model.is_some())?;
    }
    Ok((
        settings,
        options.model.as_deref().map(absolute_path).transpose()?,
    ))
}

/// Read the exact owner selection once; Engine rechecks this fence for every job.
/// No import, selection or native approval is performed by a folder consumer.
pub(crate) fn selected_pack(
    options: &LibraryGenerationOptions,
    settings: &mut GenerationSettings,
    endpoint: &Path,
    bootstrap: &Path,
) -> Result<Option<ModelPackChoice>> {
    let Some(digest) = &options.model_pack else {
        return Ok(None);
    };
    let report = current_pack_selection(endpoint, bootstrap)?;
    let choice = report.choice.context("No offline model pack selected")?;
    anyhow::ensure!(
        &choice.manifest_sha256 == digest,
        "Requested model pack is not the current owner selection"
    );
    let profile = report
        .profile
        .context("Selected model pack profile is absent")?;
    settings.detector_score_threshold = options.conf;
    settings.model_input = Some(profile.preprocessing.input.clone());
    settings.inference_profile = Some(profile);
    (settings.analysis_width, settings.analysis_height) = inference::generation_analysis_dimensions(
        settings.model_input.as_ref(),
        options.width,
        options.height,
    );
    settings.validate_execution_preset(&options.preset, true)?;
    Ok(Some(choice))
}

/// Recorded owner choice/profile only; no import, approval or batch mutation.
pub(crate) fn current_pack_selection(
    endpoint: &Path,
    bootstrap: &Path,
) -> Result<ModelPackSelectionReport> {
    let proof = crate::project_package::read_local_owner_proof(
        bootstrap
            .parent()
            .context("missing private Engine directory")?,
        "model-pack-owner.token",
    )?;
    let mut client = crate::SessionClient::connect(endpoint, bootstrap)?;
    let ResponseBody::ModelPackSelection(report) = client.execute(
        Command::ModelPackSelectionStatus { owner_proof: proof },
        None,
        None,
    )?
    else {
        anyhow::bail!("Unexpected model-pack selection response");
    };
    report.validate()?;
    Ok(*report)
}

/// Same single-file contracts for each discovered input; no second generator.
pub(crate) struct FolderGeneration {
    settings: GenerationSettings,
    model_path: Option<PathBuf>,
    choice: Option<ModelPackChoice>,
    audio_bundle: Option<AudioBundleRecipe>,
}
impl FolderGeneration {
    pub(crate) fn is_audio_bundle(&self) -> bool {
        self.audio_bundle.is_some()
    }
    pub(crate) fn ensure_execution_platform(&self) -> Result<()> {
        if self.is_audio_bundle() && !cfg!(any(target_os = "linux", target_os = "android")) {
            anyhow::bail!(ProtocolError::unsupported(
                "atomic audio folder publication on this platform"
            ));
        }
        Ok(())
    }
    pub(crate) fn generate_file(
        &self,
        client: &mut crate::SessionClient,
        source: &Path,
        output: &Path,
        preset: &str,
    ) -> Result<()> {
        if let Some(recipe) = &self.audio_bundle {
            let receipt = crate::modality::publish_atomic_audio_bundle(
                client,
                source,
                recipe.clone(),
                output,
            )?;
            println!(
                "{}",
                serde_json::to_string(
                    &serde_json::json!({"source":source,"bundle":receipt,"evidence":"synthesized","qualified":false})
                )?
            );
            Ok(())
        } else {
            crate::generate_to_file_with_pack(
                client,
                crate::FileGenerationInput {
                    fallback_selection: None,
                    video: source,
                    output,
                    preset,
                    settings: self.settings.clone(),
                    model_path: self.model_path.clone(),
                    independent_audio: None,
                    choice: self.choice.clone(),
                },
            )
        }
    }
}
pub(crate) fn folder_mode(
    options: &LibraryGenerationOptions,
    multi_axis: bool,
    endpoint: &Path,
    bootstrap: &Path,
) -> Result<FolderGeneration> {
    if let Some(path) = &options.audio_bundle_recipe {
        anyhow::ensure!(multi_axis && visual_defaults(options), "Audio folder bundles require --multi-axis and default visual/model/preset/stride controls; use per-axis recipes");
        let recipe: AudioBundleRecipe =
            serde_json::from_slice(&inference::read_bounded_profile(path)?)?;
        return Ok(FolderGeneration {
            settings: GenerationSettings::default(),
            model_path: None,
            choice: None,
            audio_bundle: Some(recipe),
        });
    }
    anyhow::ensure!(!multi_axis, "Multi-axis folder generation requires --audio-bundle-recipe; visual six-axis reconstruction is not implemented");
    let (mut settings, model_path) = configure(options)?;
    let choice = selected_pack(options, &mut settings, endpoint, bootstrap)?;
    Ok(FolderGeneration {
        settings,
        model_path,
        choice,
        audio_bundle: None,
    })
}

/// Shared retained-folder input selection; invoke outside a graphical frame.
pub(crate) fn retained_batch_options(
    folder: &Path,
    recursive: bool,
    recipe_path: &Path,
) -> Result<LibraryBatchOptions> {
    let recipe: LibraryBatchRecipe =
        serde_json::from_slice(&crate::inference::read_bounded_profile(recipe_path)?)?;
    retained_batch_options_with_generation(folder, recursive, recipe)
}

pub(crate) fn retained_batch_options_with_recipe(
    folder: &Path,
    recursive: bool,
    recipe: AudioBundleRecipe,
) -> Result<LibraryBatchOptions> {
    retained_batch_options_with_generation(folder, recursive, recipe.into())
}

pub(crate) fn retained_batch_options_with_generation(
    folder: &Path,
    recursive: bool,
    recipe: LibraryBatchRecipe,
) -> Result<LibraryBatchOptions> {
    let visual = matches!(recipe, LibraryBatchRecipe::Visual(_));
    let mut inputs = Vec::new();
    for entry in crate::library_entries(folder, recursive, !visual)? {
        if visual {
            if entry.has_script {
                continue;
            }
            let mut name = entry
                .video
                .file_name()
                .context("Media filename absent")?
                .to_os_string();
            name.push(".pulsar-visual-bundle");
            let destination = entry.video.with_file_name(name);
            match std::fs::symlink_metadata(&destination) {
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            inputs.push(LibraryBatchInput {
                source: entry.video,
                destination,
                publication_axes: None,
                publication_directory: None,
            });
        } else {
            let mut present = std::collections::BTreeSet::new();
            match std::fs::symlink_metadata(entry.video.with_extension("funscript")) {
                Ok(meta) if meta.is_file() => {
                    present.insert(Axis::Stroke);
                }
                Ok(_) => anyhow::bail!("Adjacent stroke companion is not a regular file"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            let publication_directory = match std::fs::symlink_metadata(&entry.script) {
                Ok(meta) if meta.is_dir() => {
                    for axis in Axis::ALL {
                        let leaf = entry.script.join(format!("{}.funscript", axis.as_str()));
                        match std::fs::symlink_metadata(leaf) {
                            Ok(meta) if meta.is_file() => {
                                present.insert(axis);
                            }
                            Ok(_) => anyhow::bail!("Existing bundle channel is not a regular file"),
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                            Err(error) => return Err(error.into()),
                        }
                    }
                    Some(existing_publication_directory_identity(&meta)?)
                }
                Ok(_) => continue, // Preserve legacy whole-entry exclusion for conflicting bundle targets.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
            let axes: Vec<_> = Axis::ALL
                .into_iter()
                .filter(|axis| !present.contains(axis))
                .collect();
            if axes.is_empty() {
                continue;
            }
            inputs.push(LibraryBatchInput {
                source: entry.video,
                destination: entry.script,
                publication_axes: if axes.len() == 6 { None } else { Some(axes) },
                publication_directory,
            });
        }
    }
    let options = LibraryBatchOptions { inputs, recipe };
    options.validate()?;
    Ok(options)
}

fn existing_publication_directory_identity(
    meta: &std::fs::Metadata,
) -> Result<PublicationDirectoryIdentity> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(PublicationDirectoryIdentity {
            device: meta.dev(),
            inode: meta.ino(),
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = meta;
        anyhow::bail!("Existing bundle fill is unavailable on this platform")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn partial_audio_intake_keeps_existing_channels_and_captures_only_missing_axes() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("clip.wav");
        std::fs::write(&source, b"media").unwrap();
        std::fs::write(dir.path().join("clip.funscript"), b"stroke").unwrap();
        let bundle = dir.path().join("clip.wav.pulsar-audio-bundle");
        std::fs::create_dir(&bundle).unwrap();
        std::fs::write(bundle.join("roll.funscript"), b"unvalidated roll").unwrap();
        let recipe: AudioBundleRecipe = serde_json::from_value(serde_json::json!({"channels":Axis::ALL.iter().map(|axis|serde_json::json!({"mapping":{"axis":axis,"amplitude":0.2,"offset":0.4},"mode":"envelope","analysis":{"band":"full","threshold":0.05}})).collect::<Vec<_>>()})).unwrap();
        let options =
            retained_batch_options_with_recipe(dir.path(), false, recipe.clone()).unwrap();
        assert_eq!(options.publication_schema(false), 6);
        assert_eq!(
            options.inputs[0].publication_axes,
            Some(vec![Axis::Sway, Axis::Surge, Axis::Pitch, Axis::Yaw])
        );
        assert!(options.inputs[0].publication_directory.is_some());
        std::fs::create_dir(bundle.join("pitch.funscript")).unwrap();
        assert!(retained_batch_options_with_recipe(dir.path(), false, recipe).is_err());
        assert_eq!(
            std::fs::read(bundle.join("roll.funscript")).unwrap(),
            b"unvalidated roll"
        );
    }
    #[test]
    fn retained_audio_preserves_adjacent_stroke_by_capturing_secondary_publication() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("clip.wav"), b"authored media").unwrap();
        std::fs::write(dir.path().join("clip.funscript"), b"protected stroke").unwrap();
        let recipe: AudioBundleRecipe = serde_json::from_value(serde_json::json!({"channels":Axis::ALL.iter().map(|axis|serde_json::json!({"mapping":{"axis":axis,"amplitude":0.2,"offset":0.4},"mode":"envelope","analysis":{"band":"full","threshold":0.05}})).collect::<Vec<_>>()})).unwrap();
        let result = retained_batch_options_with_recipe(dir.path(), false, recipe.clone()).unwrap();
        assert_eq!(
            result.inputs[0].publication_axes,
            Some(Axis::ALL[1..].to_vec())
        );
        assert_eq!(result.publication_schema(false), 5);
        assert_eq!(
            std::fs::read(dir.path().join("clip.funscript")).unwrap(),
            b"protected stroke"
        );
        std::fs::remove_file(dir.path().join("clip.funscript")).unwrap();
        std::fs::create_dir(dir.path().join("clip.funscript")).unwrap();
        assert!(retained_batch_options_with_recipe(dir.path(), false, recipe).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
    use crate::cli::{Cli, Commands};
    use clap::Parser;
    fn options(args: &[&str]) -> LibraryGenerationOptions {
        let args = [vec!["pulsar", "batch", "/fixture"], args.to_vec()].concat();
        let Some(Commands::Batch { generation, .. }) = Cli::try_parse_from(args).unwrap().command
        else {
            panic!("batch")
        };
        generation
    }
    #[test]
    fn checked_audio_folder_recipe_refuses_mixed_defaults_duplicates_and_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recipe.json");
        let mut wire = serde_json::json!({"channels":Axis::ALL.iter().map(|axis| serde_json::json!({"mapping":{"axis":axis,"amplitude":0.2,"offset":0.4},"mode":"envelope","analysis":{"band":"full","threshold":0.05}})).collect::<Vec<_>>()});
        std::fs::write(&path, serde_json::to_vec(&wire).unwrap()).unwrap();
        let opts = options(&[
            "--multi-axis",
            "--audio-bundle-recipe",
            path.to_str().unwrap(),
        ]);
        assert!(!is_default(&opts));
        let mode = folder_mode(
            &opts,
            true,
            Path::new("missing-engine"),
            Path::new("missing-proof"),
        )
        .unwrap();
        assert!(mode.is_audio_bundle());
        assert!(folder_mode(
            &opts,
            false,
            Path::new("missing-engine"),
            Path::new("missing-proof")
        )
        .is_err());
        let mixed = options(&[
            "--multi-axis",
            "--audio-bundle-recipe",
            path.to_str().unwrap(),
            "--preset",
            "maximum",
        ]);
        assert!(folder_mode(
            &mixed,
            true,
            Path::new("missing-engine"),
            Path::new("missing-proof")
        )
        .is_err());
        wire["channels"][1]["mapping"]["axis"] = wire["channels"][0]["mapping"]["axis"].clone();
        std::fs::write(&path, serde_json::to_vec(&wire).unwrap()).unwrap();
        assert!(folder_mode(
            &opts,
            true,
            Path::new("missing-engine"),
            Path::new("missing-proof")
        )
        .is_err());
        wire["channels"][1]["mapping"]["axis"] = serde_json::to_value(Axis::ALL[1]).unwrap();
        wire["unexpected"] = serde_json::json!(true);
        std::fs::write(&path, serde_json::to_vec(&wire).unwrap()).unwrap();
        assert!(folder_mode(
            &opts,
            true,
            Path::new("missing-engine"),
            Path::new("missing-proof")
        )
        .is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "batch",
            "/fixture",
            "--audio-bundle-recipe",
            "recipe.json"
        ])
        .is_err());
        assert!(folder_mode(
            &options(&["--multi-axis"]),
            true,
            Path::new("missing-engine"),
            Path::new("missing-proof")
        )
        .is_err());
    }
    #[test]
    fn folder_models_require_explicit_contract_and_preserve_tier_stride_and_threshold() {
        assert!(is_default(&options(&[])));
        assert!(configure(&options(&["--model", "/detector.onnx"])).is_err());
        assert!(configure(&options(&["--preset", "maximum"])).is_err());
        let (settings, path) = configure(&options(&[
            "--model",
            "/detector.onnx",
            "--model-class-count",
            "1",
            "--width",
            "64",
            "--height",
            "64",
            "--preset",
            "maximum",
            "--source-sample-stride",
            "2",
            "--conf",
            "0.7",
        ]))
        .unwrap();
        assert_eq!(path.unwrap(), PathBuf::from("/detector.onnx"));
        assert_eq!(settings.source_sample_stride, 2);
        assert_eq!(settings.detector_score_threshold, Some(0.7));
        assert_eq!(
            (settings.analysis_width, settings.analysis_height),
            (64, 64)
        );
        assert_eq!(settings.model_input.unwrap().class_count, 1);
        assert!(configure(&options(&["--width", "0"])).is_err());
    }
    #[test]
    fn selected_pack_intent_is_explicit_and_shape_is_deferred_until_owner_lookup() {
        let digest = "a".repeat(64);
        let options = options(&[
            "--model-pack",
            &digest,
            "--preset",
            "maximum",
            "--conf",
            "0.7",
        ]);
        assert!(!is_default(&options));
        let (settings, path) = configure(&options).unwrap();
        assert!(
            path.is_none()
                && settings.model_input.is_none()
                && settings.inference_profile.is_none()
        );
        assert_eq!(options.conf, Some(0.7));
        assert!(settings.detector_score_threshold.is_none());
        assert!(configure(&self::options(&["--model-pack", "invalid"])).is_err());
    }
    #[test]
    fn folder_grammar_rejects_unpaired_or_ambiguous_model_controls() {
        for args in [
            vec!["--model-class-count", "1"],
            vec!["--model-pack", "pack", "--model", "/model.onnx"],
            vec![
                "--model-pack",
                "pack",
                "--inference-profile",
                "/profile.json",
            ],
            vec!["--conf", "0.5"],
            vec!["--inference-profile", "/profile.json"],
            vec!["--source-sample-stride", "0"],
            vec![
                "--model",
                "/model.onnx",
                "--model-class-count",
                "1",
                "--inference-profile",
                "/profile.json",
            ],
        ] {
            assert!(
                Cli::try_parse_from([vec!["pulsar", "batch", "/fixture"], args].concat()).is_err()
            );
        }
    }
}
