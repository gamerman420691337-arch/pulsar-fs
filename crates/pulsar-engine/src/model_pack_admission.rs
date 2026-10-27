struct PreparedModelPack {
    model: SourceArtifact,
    settings: GenerationSettings,
    dependencies: Vec<ArtifactIdentity>,
}

impl Engine {
    /// Select a runtime for this explicit profile without changing its provider.
    /// CPU owner policy can name a different ORT image from the default/CUDA runtime.
    fn selected_profile_runtime(
        &self,
        profile: &DeclaredInferenceProfile,
    ) -> PResult<ArtifactReference> {
        #[cfg(target_os = "linux")]
        if profile.provider == InferenceProvider::Cpu {
            if let Some(owner) = &self.cpu_owner_policy {
                owner.match_profile(profile).map_err(unavailable)?;
                return owner.runtime_reference().map_err(unavailable);
            }
        }
        let runtime = self
            .tools
            .as_ref()
            .map_err(unavailable)?
            .onnx_runtime
            .as_ref()
            .ok_or_else(|| ProtocolError::unsupported("pinned inference runtime is unavailable"))?;
        if profile.runtime != runtime.identity {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "profile runtime differs from the selected inference runtime",
            ));
        }
        Ok(runtime.clone())
    }

    fn require_current_pack_choice(
        &self,
        db: &Connection,
        choice: &ModelPackChoice,
    ) -> PResult<()> {
        let selection = crate::model_pack_selection::status(db).map_err(unavailable)?;
        if selection.choice.as_ref() != Some(choice) {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "requested model pack is not the current owner selection",
            ));
        }
        if let Some(binding) = &selection.catalog_binding {
            let _reservation = self.pool.reserve_memory(256 * 1024 * 1024)?;
            crate::model_packs::catalog_binding_check(
                &self.config.state_dir,
                &choice.manifest_sha256,
                binding,
            )
            .map_err(|e| ProtocolError::new(ErrorCode::DependencyMismatch, e.to_string()))?;
        }
        Ok(())
    }

    fn checked_pack_input(
        &self,
        sha: &str,
        priority: crate::resources::GpuPriority,
    ) -> PResult<crate::model_packs::PackExecutionInput> {
        let input = crate::model_packs::execution_input(&self.config.state_dir, sha)
            .map_err(unavailable)?;
        self.selected_profile_runtime(&input.profile)?;
        let budget = ResourceBudget {
            memory_bytes: WORKER_MEMORY,
            output_bytes: OUTPUT_LIMIT,
            wall_time_ms: self.config.worker_timeout.as_millis().min(u64::MAX as u128) as u64,
            cpu_threads: 2,
        };
        if input.profile.provider == InferenceProvider::Cuda {
            #[cfg(not(target_os = "linux"))]
            return Err(ProtocolError::unsupported("CUDA model packs require Linux"));
            #[cfg(target_os = "linux")]
            {
                if !cfg!(feature = "cuda") {
                    return Err(ProtocolError::unsupported(
                        "CUDA model packs require a CUDA-enabled build",
                    ));
                }
                let owner = self.cuda_owner_policy.as_ref().ok_or_else(|| {
                    ProtocolError::unsupported("CUDA model pack requires owner policy")
                })?;
                owner
                    .policy()
                    .authorize_profile(&input.profile, &budget, priority)
                    .map_err(unavailable)?;
                crate::inference_profiles::admit_tensor_adapter(&input.profile, &budget)
                    .map_err(unavailable)?;
            }
        } else if matches!(
            input.profile.preprocessing.resize,
            InferenceResize::AnnotatedTemporalView
        ) {
            crate::inference_profiles::admit_tensor_adapter(&input.profile, &budget)
                .map_err(|e| ProtocolError::unsupported(e.to_string()))?;
        } else {
            crate::inference_profiles::admit_adapter(&input.profile, &budget)
                .map_err(|e| ProtocolError::unsupported(e.to_string()))?;
        }
        Ok(input)
    }

    fn model_pack_selection_command(
        &self,
        request: &Request,
        session: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        let _gate = self.artifact_gate.lock().map_err(internal)?;
        let _reservation = self.pool.reserve_memory(256 * 1024 * 1024)?;
        {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, session)?;
            if let Some(body) = replay(&db, session.as_str(), request, fingerprint)? {
                return Ok(body);
            }
            if matches!(&request.command, Command::ModelPackSelectionStatus { .. }) {
                // Recorded approval only; generation performs fresh integrity checks.
                return crate::model_pack_selection::status(&db)
                    .map(|value| ResponseBody::ModelPackSelection(value.into()))
                    .map_err(unavailable);
            }
        }
        let (manifest_sha256, expected_selection_revision, binding) = match &request.command {
            Command::SelectModelPack {
                manifest_sha256,
                expected_selection_revision,
                ..
            } => (manifest_sha256.clone(), *expected_selection_revision, None),
            Command::SelectCatalogModelPack {
                manifest_sha256,
                expected_selection_revision,
                binding,
                ..
            } => {
                crate::model_packs::catalog_pack_check(
                    &self.config.state_dir,
                    &binding.bootstrap_sha256,
                    manifest_sha256,
                    binding.catalog_revision,
                )
                .map_err(unavailable)?;
                (
                    Some(manifest_sha256.clone()),
                    *expected_selection_revision,
                    Some(binding.clone()),
                )
            }
            _ => unreachable!(),
        };
        // Full integrity/profile checking stays outside the database mutex.
        let selected = manifest_sha256
            .as_ref()
            .map(|sha| {
                self.checked_pack_input(sha, crate::resources::GpuPriority::Interactive)
                    .map(|input| (sha.clone(), input.profile))
            })
            .transpose()?;
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(internal)?;
        self.authorize(&tx, request, session)?;
        if let Some(body) = replay(&tx, session.as_str(), request, fingerprint)? {
            return Ok(body);
        }
        claim_request_identity(&tx, session.as_str(), request, fingerprint)?;
        if let Some(binding) = &binding {
            crate::model_packs::catalog_binding_check(
                &self.config.state_dir,
                manifest_sha256.as_ref().expect("bound manifest"),
                binding,
            )
            .map_err(unavailable)?;
        }
        let body = ResponseBody::ModelPackSelection(
            (crate::model_pack_selection::select_bound(
                &tx,
                expected_selection_revision,
                selected,
                binding,
                session,
                &request.request_id,
            )?)
            .into(),
        );
        remember(&tx, session.as_str(), request, fingerprint, &body)?;
        tx.commit().map_err(internal)?;
        Ok(body)
    }

    fn snapshot_pack_document(&self, bytes: &[u8]) -> PResult<ArtifactIdentity> {
        // An unnamed private file feeds the same immutable CAS admission as a model.
        let mut file = tempfile::tempfile_in(&self.config.state_dir).map_err(unavailable)?;
        file.write_all(bytes).map_err(unavailable)?;
        let (_, sha256, byte_len) = artifacts::snapshot_opened_reserved(
            &mut file,
            &self.config.state_dir.join("snapshots"),
            2 * 1024 * 1024,
            self.pool.reserved_storage(),
        )
        .map_err(unavailable)?;
        Ok(ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).map_err(internal)?,
            sha256,
            byte_len,
        })
    }

    fn prepare_selected_pack(
        &self,
        choice: &ModelPackChoice,
        settings: GenerationSettings,
    ) -> PResult<PreparedModelPack> {
        self.prepare_selected_pack_with_priority(
            choice,
            settings,
            crate::resources::GpuPriority::Offline,
        )
    }

    fn prepare_selected_pack_with_priority(
        &self,
        choice: &ModelPackChoice,
        settings: GenerationSettings,
        priority: crate::resources::GpuPriority,
    ) -> PResult<PreparedModelPack> {
        self.prepare_selected_pack_settings(choice, settings, priority, None)
    }

    fn prepare_selected_stash_pack(
        &self,
        choice: &ModelPackChoice,
        settings: GenerationSettings,
        views: &[StashBatchAnnotatedView],
    ) -> PResult<PreparedModelPack> {
        self.prepare_selected_pack_settings(
            choice,
            settings,
            crate::resources::GpuPriority::Offline,
            Some(views),
        )
    }

    fn prepare_selected_pack_settings(
        &self,
        choice: &ModelPackChoice,
        mut settings: GenerationSettings,
        priority: crate::resources::GpuPriority,
        batch_views: Option<&[StashBatchAnnotatedView]>,
    ) -> PResult<PreparedModelPack> {
        let _reservation = self.pool.reserve_memory(4 * 1024 * 1024)?;
        let selection = {
            let db = self.db.lock().map_err(internal)?;
            self.require_current_pack_choice(&db, choice)?;
            crate::model_pack_selection::status(&db).map_err(unavailable)?
        };
        if selection.choice.as_ref() != Some(choice) {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "requested model pack is not the current owner selection",
            ));
        }
        let mut input = self.checked_pack_input(&choice.manifest_sha256, priority)?;
        if serde_json::to_value(&selection.profile).map_err(internal)?
            != serde_json::to_value(Some(&input.profile)).map_err(internal)?
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "selected pack profile changed",
            ));
        }
        if let Some(profile) = &settings.inference_profile {
            if serde_json::to_value(profile).map_err(internal)?
                != serde_json::to_value(&input.profile).map_err(internal)?
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "caller profile conflicts with selected pack",
                ));
            }
        }
        if let Some(contract) = &settings.model_input {
            if serde_json::to_value(contract).map_err(internal)?
                != serde_json::to_value(&input.profile.preprocessing.input).map_err(internal)?
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "caller input conflicts with selected pack",
                ));
            }
        }
        settings.model_input = Some(input.profile.preprocessing.input.clone());
        settings.inference_profile = Some(input.profile.clone());
        if let Some(views) = batch_views {
            validate_stash_batch_settings(&settings, views)?;
        } else {
            settings.validate()?;
        }
        let (path, sha256, byte_len) = artifacts::snapshot_opened_reserved(
            &mut input.model,
            &self.config.state_dir.join("snapshots"),
            self.config.max_snapshot_bytes,
            self.pool.reserved_storage(),
        )
        .map_err(unavailable)?;
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).map_err(internal)?,
            sha256: sha256.clone(),
            byte_len,
        };
        if identity != input.profile.model {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "selected model changed during capture",
            ));
        }
        let dependencies = vec![
            self.snapshot_pack_document(&input.manifest_bytes)?,
            self.snapshot_pack_document(&input.profile_bytes)?,
            self.snapshot_pack_document(&serde_json::to_vec(&selection).map_err(internal)?)?,
        ];
        settings.model_input = Some(input.profile.preprocessing.input.clone());
        settings.inference_profile = Some(input.profile);
        if let Some(views) = batch_views {
            validate_stash_batch_settings(&settings, views)?;
        } else {
            settings.validate()?;
        }
        Ok(PreparedModelPack {
            model: SourceArtifact {
                source_version: SourceVersionId::new(sha256).map_err(internal)?,
                path,
                identity,
            },
            settings,
            dependencies,
        })
    }
}

impl Engine {
    /// Caller holds the artifact gate through capture admission and rechecks actor/revision.
    pub(crate) fn prepare_selected_live_pack(
        &self,
        choice: &ModelPackChoice,
    ) -> PResult<LiveCaptureInference> {
        let prepared = self.prepare_selected_pack_with_priority(
            choice,
            GenerationSettings {
                analysis_width: 64,
                analysis_height: 64,
                ..GenerationSettings::default()
            },
            crate::resources::GpuPriority::Interactive,
        )?;
        Ok(LiveCaptureInference {
            model_path: prepared.model.path,
            profile: prepared
                .settings
                .inference_profile
                .ok_or_else(|| ProtocolError::invalid("selected pack has no profile"))?,
        })
    }
}
