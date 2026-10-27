//! Per-source annotated batch resolution and its conditional ledger reader.
use super::*;

impl Engine {
    pub(super) fn admit_stash_generation(
        &self,
        settings: &GenerationSettings,
        views: &[StashBatchAnnotatedView],
    ) -> PResult<()> {
        validate_stash_batch_settings(settings, views)?;
        if views.is_empty() {
            return self.admit_stash_profile(settings);
        }
        let profile = settings
            .inference_profile
            .as_ref()
            .ok_or_else(|| ProtocolError::invalid("annotated batch profile absent"))?;
        let budget = ResourceBudget {
            memory_bytes: WORKER_MEMORY,
            output_bytes: OUTPUT_LIMIT,
            wall_time_ms: self.config.worker_timeout.as_millis().min(u64::MAX as u128) as u64,
            cpu_threads: 2,
        };
        crate::inference_profiles::admit_tensor_adapter(profile, &budget)
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        match profile.provider {
            InferenceProvider::Cpu => Ok(()),
            InferenceProvider::Cuda => {
                #[cfg(all(target_os = "linux", feature = "cuda"))]
                {
                    let owner = self.cuda_owner_policy.as_ref().ok_or_else(|| {
                        ProtocolError::unsupported(
                            "annotated CUDA batch requires owner-configured CUDA policy",
                        )
                    })?;
                    let tools = self.tools.as_ref().map_err(unavailable)?;
                    owner
                        .policy()
                        .match_runtime(tools.onnx_runtime.as_ref())
                        .and_then(|()| {
                            owner.policy().authorize_profile(
                                profile,
                                &budget,
                                crate::resources::GpuPriority::Offline,
                            )
                        })
                        .map_err(|e| {
                            ProtocolError::new(ErrorCode::DependencyMismatch, e.to_string())
                        })
                }
                #[cfg(not(all(target_os = "linux", feature = "cuda")))]
                Err(ProtocolError::unsupported(
                    "annotated CUDA batch requires a Linux CUDA-enabled build",
                ))
            }
            _ => Err(ProtocolError::unsupported(
                "unsupported annotated batch provider; no provider substitution",
            )),
        }
    }
}
fn entry<'a>(options: &'a StashBatchOptions, item: &Item) -> PResult<&'a StashBatchAnnotatedView> {
    options
        .annotated_views
        .iter()
        .find(|entry| {
            entry.scene_id == item.view.scene_id
                && Some(&entry.file_id) == item.view.file_id.as_ref()
        })
        .ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "selected Stash source has no explicit annotated view",
            )
        })
}
pub(super) fn validate_selection(options: &StashBatchOptions, items: &[Item]) -> PResult<()> {
    if options.annotated_views.is_empty() {
        return Ok(());
    }
    for item in items
        .iter()
        .filter(|item| item.view.eligibility == StashFileStatus::ReadyForSnapshot)
    {
        entry(options, item)?;
    }
    for view in &options.annotated_views {
        if !items.iter().any(|item| {
            item.view.eligibility == StashFileStatus::ReadyForSnapshot
                && item.view.scene_id == view.scene_id
                && item.view.file_id.as_ref() == Some(&view.file_id)
        }) {
            return Err(ProtocolError::invalid(
                "annotated catalog contains an unselected or ineligible Stash source",
            ));
        }
    }
    Ok(())
}
pub(super) fn validate_future(options: &StashBatchOptions, item: &Item) -> PResult<()> {
    if options.annotated_views.is_empty() {
        return Ok(());
    }
    let selected = entry(options, item)?;
    if let Some(capture) = &item.capture {
        if capture.identity.sha256 != selected.view.source_sha256
            || item.view.source_version.as_ref() != Some(&capture.source_version)
            || capture.scene_id != selected.scene_id
            || capture.file_id != selected.file_id
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "annotated batch view differs from captured source content",
            ));
        }
    }
    Ok(())
}
pub(super) fn generation_command(options: &StashBatchOptions, item: &Item) -> PResult<Command> {
    if !options.source_plans.is_empty() {
        return plans::generation_command(options, item);
    }
    let source = item
        .view
        .source_version
        .clone()
        .ok_or_else(|| ProtocolError::invalid("captured source absent"))?;
    if options.annotated_views.is_empty() {
        return Ok(batch_generation_command(options, source));
    }
    validate_future(options, item)?;
    if item.capture.is_none() {
        return Err(ProtocolError::invalid(
            "annotated batch requires retained source capture",
        ));
    }
    let mut resolved = options.clone();
    resolved.settings.annotated_view = Some(entry(options, item)?.view.clone());
    resolved.settings.validate()?;
    Ok(batch_generation_command(&resolved, source))
}
pub(super) fn requires_reader(record: &Record) -> bool {
    !record.options.annotated_views.is_empty()
        || record
            .migration_history
            .iter()
            .any(|h| !h.previous_options.annotated_views.is_empty())
}
pub(super) fn validate_record(record: &Record) -> Result<()> {
    anyhow::ensure!(
        (record.report.schema_version == 5)
            == (requires_reader(record) && !plans::requires_reader(record)),
        "annotated batch ledger reader requirement differs"
    );
    if record.options.annotated_views.is_empty() {
        return Ok(());
    }
    for item in &record.items {
        if item.cursor == 2 {
            if let Some(pending) = &item.pending {
                if pending.phase == "generate" {
                    anyhow::ensure!(
                        encode(&pending.command)?
                            == encode(&generation_command(&record.options, item)?)?,
                        "retained batch generation differs from per-source intent"
                    );
                }
            }
        }
    }
    Ok(())
}
pub(super) fn arm_reader(tx: &rusqlite::Transaction<'_>, record: &Record) -> PResult<()> {
    if requires_reader(record) || plans::requires_reader(record) {
        let version = crate::portable_annotated_db::storage_version(tx).map_err(internal)?;
        if version < 13 {
            crate::portable_annotated_db::publish(tx, 13).map_err(internal)?;
        }
    }
    Ok(())
}
/// The existing schema-13 startup validator rejects the new options/report in
/// preceding readers. Detect stripped header requirements before writable open.
pub(super) fn validate_reader_requirement(db: &Connection) -> Result<()> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='stash_batches')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let required:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM stash_batches WHERE CASE WHEN typeof(record)!='text' OR length(CAST(record AS BLOB)) NOT BETWEEN 1 AND 16777216 OR NOT json_valid(record) THEN 0 ELSE json_extract(record,'$.report.schema_version')>=5 OR json_array_length(record,'$.options.annotated_views')>0 OR json_array_length(record,'$.options.source_plans')>0 OR EXISTS(SELECT 1 FROM json_each(record,'$.items') i WHERE json_type(i.value,'$.placement')='object' OR json_type(i.value,'$.generation_plan')='object') OR EXISTS(SELECT 1 FROM json_each(record,'$.migration_history') h WHERE json_array_length(h.value,'$.previous_options.annotated_views')>0 OR json_array_length(h.value,'$.previous_options.source_plans')>0) END)",[],|r|r.get(0))?;
    let version = crate::portable_annotated_db::storage_version(db)?;
    anyhow::ensure!(
        !required || version >= 13,
        "annotated batch requires its pre-recovery schema-13 ledger reader"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn annotated(record: &mut Record) {
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 1,
        };
        let input = ModelInputContract {
            width: 64,
            height: 64,
            channels: 6,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: DetectorDecoder::TemporalScriptPosition,
        };
        record.options.settings.analysis_width = 64;
        record.options.settings.analysis_height = 64;
        record.options.settings.model_input = Some(input.clone());
        record.options.settings.inference_profile = Some(DeclaredInferenceProfile {
            schema_version: 1,
            model: identity.clone(),
            runtime: identity.clone(),
            preprocessing: InferencePreprocessing {
                input,
                resize: InferenceResize::AnnotatedTemporalView,
                color: InferenceColor::RgbFramePair,
                scale: vec![1.0 / 255.0; 6],
                bias: vec![0.0; 6],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 128 * 1024 * 1024,
            minimum_output_bytes: 1024,
            cpu_threads: 2,
            cuda_resources: None,
        });
        record.options.model_path = Some("/authored.onnx".into());
        record.options.annotated_views=vec![StashBatchAnnotatedView {scene_id:"authored".into(),file_id:"file".into(),view:AnnotatedGenerationView {
            source_sha256:"a".repeat(64),declaration:serde_json::from_value(serde_json::json!({"schema_version":1,"source_dimensions":[64,64],"analysis_dimensions":[64,64],"target_region":[0,0,64,64],"target_id":"explicit","video_view":{"layout":"mono","eye":"left","projection":{"kind":"flat"}}})).unwrap(),
        }}];
        record.report.schema_version = 5;
        let source = SourceVersionId::new("source").unwrap();
        let item = &mut record.items[0];
        item.cursor = 2;
        item.pending = None;
        item.view.source_version = Some(source.clone());
        item.capture = Some(StashSourceCapture {
            endpoint: None,
            source_version: source,
            identity,
            scene_id: "authored".into(),
            file_id: "file".into(),
            server_path: "/stash/source.mkv".into(),
            local_source: "/authored/source.mkv".into(),
            script_destination: "/authored/source.funscript".into(),
        });
    }
    #[test]
    fn pack_catalog_resolves_per_source_without_changing_template() {
        let (_dir, _config, _engine, _owner, _token, mut record, _pending) =
            super::super::tests::setup();
        annotated(&mut record);
        let choice = ModelPackChoice {
            manifest_sha256: "c".repeat(64),
            selection_revision: 1,
        };
        record.options.model_pack = Some(choice.clone());
        assert!(record.options.validate().is_err());
        record.options.model_path = None;
        record.options.validate().unwrap();
        let Command::GenerateFromModelPack {
            choice: resolved,
            settings: Some(settings),
            ..
        } = generation_command(&record.options, &record.items[0]).unwrap()
        else {
            panic!()
        };
        assert_eq!(resolved, choice);
        assert_eq!(
            settings.annotated_view.unwrap().source_sha256,
            "a".repeat(64)
        );
        assert!(record.options.settings.annotated_view.is_none());
        record.options.model_pack = None;
        assert!(record.options.validate().is_err());
    }

    #[test]
    fn catalog_resolves_exact_source_and_refuses_hash_or_key_substitution() {
        let (_dir, _config, engine, _owner, _token, mut record, _pending) =
            super::super::tests::setup();
        annotated(&mut record);
        record.options.validate().unwrap();
        engine
            .admit_stash_generation(&record.options.settings, &record.options.annotated_views)
            .unwrap();
        validate_selection(&record.options, &record.items).unwrap();
        let Command::Generate {
            settings: Some(settings),
            ..
        } = generation_command(&record.options, &record.items[0]).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            settings.annotated_view.unwrap().source_sha256,
            "a".repeat(64)
        );
        record.items[0].capture.as_mut().unwrap().identity.sha256 = "b".repeat(64);
        assert_eq!(
            generation_command(&record.options, &record.items[0])
                .unwrap_err()
                .code,
            ErrorCode::DependencyMismatch
        );
        record.items[0].capture.as_mut().unwrap().identity.sha256 = "a".repeat(64);
        record.items[0].view.file_id = Some("other".into());
        assert!(validate_selection(&record.options, &record.items).is_err());
        assert!(generation_command(&record.options, &record.items[0]).is_err());
    }
    #[test]
    fn catalog_rejects_missing_duplicate_global_view_and_provider_before_dispatch() {
        let (_dir, _config, engine, _owner, _token, mut record, _pending) =
            super::super::tests::setup();
        annotated(&mut record);
        let view = record.options.annotated_views[0].clone();
        record.options.annotated_views.push(view.clone());
        assert!(record.options.validate().is_err());
        record.options.annotated_views.pop();
        record.options.settings.annotated_view = Some(view.view);
        assert!(record.options.validate().is_err());
        record.options.settings.annotated_view = None;
        record.options.annotated_views.clear();
        assert!(record.options.validate().is_err());
        annotated(&mut record);
        record
            .options
            .settings
            .inference_profile
            .as_mut()
            .unwrap()
            .provider = InferenceProvider::Cuda;
        assert!(engine
            .admit_stash_generation(&record.options.settings, &record.options.annotated_views)
            .is_err());
    }
    #[test]
    fn valid_cuda_catalog_requires_owner_policy_before_dispatch() {
        let (_dir, _config, engine, _owner, _token, mut record, _pending) =
            super::super::tests::setup();
        annotated(&mut record);
        let request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        let WorkerOperation::Generate { settings, .. } = request.operation else {
            panic!()
        };
        let cuda = settings.inference_profile.unwrap();
        let profile = record.options.settings.inference_profile.as_mut().unwrap();
        profile.schema_version = 2;
        profile.provider = InferenceProvider::Cuda;
        profile.cuda_resources = cuda.cuda_resources;
        let budget = ResourceBudget {
            memory_bytes: WORKER_MEMORY,
            output_bytes: OUTPUT_LIMIT,
            wall_time_ms: engine
                .config
                .worker_timeout
                .as_millis()
                .min(u64::MAX as u128) as u64,
            cpu_threads: 2,
        };
        crate::inference_profiles::admit_tensor_adapter(profile, &budget).unwrap();
        record.options.validate().unwrap();
        let error = engine
            .admit_stash_generation(&record.options.settings, &record.options.annotated_views)
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unsupported);
        #[cfg(all(target_os = "linux", feature = "cuda"))]
        assert!(error.message.contains("owner-configured CUDA policy"));
        #[cfg(not(all(target_os = "linux", feature = "cuda")))]
        assert!(error.message.contains("Linux CUDA-enabled build"));
    }
    #[test]
    fn conditional_reader_is_transactional_and_detects_header_or_report_downgrade() {
        let (_dir, _config, engine, owner, _token, mut record, _pending) =
            super::super::tests::setup();
        annotated(&mut record);
        let mut db = engine.db.lock().unwrap();
        assert!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap()
                < 13
        );
        {
            let tx = db.transaction().unwrap();
            arm_reader(&tx, &record).unwrap();
            assert_eq!(
                tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                13
            );
        }
        assert!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap()
                < 13
        );
        let tx = db.transaction().unwrap();
        arm_reader(&tx, &record).unwrap();
        tx.execute(
            "UPDATE stash_batches SET record=?1 WHERE session=?2",
            params![encode(&record).unwrap(), owner.as_str()],
        )
        .unwrap();
        tx.commit().unwrap();
        validate_reader_requirement(&db).unwrap();
        validate_record(&record).unwrap();
        record.report.schema_version = 4;
        assert!(validate_record(&record).is_err());
        db.execute_batch("PRAGMA user_version=12").unwrap();
        assert!(validate_reader_requirement(&db).is_err());
    }
    #[test]
    fn legacy_empty_catalog_omits_new_wire_field() {
        let (_dir, _config, _engine, _owner, _token, record, _pending) =
            super::super::tests::setup();
        assert!(!encode(&record.options).unwrap().contains("annotated_views"));
    }
}
