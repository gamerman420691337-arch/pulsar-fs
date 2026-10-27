//! Durable per-source placement resolution. Acknowledged Engine effects, not
//! client-generated placement identifiers, advance the generation revision.
use super::*;

pub(super) fn entry<'a>(
    options: &'a StashBatchOptions,
    item: &Item,
) -> PResult<Option<&'a StashBatchSourcePlan>> {
    if options.source_plans.is_empty() {
        return Ok(None);
    }
    options
        .source_plans
        .iter()
        .find(|p| {
            p.scene_id == item.view.scene_id && Some(&p.file_id) == item.view.file_id.as_ref()
        })
        .map(Some)
        .ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "selected Stash source has no source plan",
            )
        })
}
pub(super) fn validate_selection(options: &StashBatchOptions, items: &[Item]) -> PResult<()> {
    if options.source_plans.is_empty() {
        return Ok(());
    }
    for item in items
        .iter()
        .filter(|i| i.view.eligibility == StashFileStatus::ReadyForSnapshot)
    {
        entry(options, item)?;
    }
    for p in &options.source_plans {
        if !items.iter().any(|i| {
            i.view.eligibility == StashFileStatus::ReadyForSnapshot
                && i.view.scene_id == p.scene_id
                && i.view.file_id.as_ref() == Some(&p.file_id)
        }) {
            return Err(ProtocolError::invalid(
                "source plan catalog contains an unselected or ineligible Stash file",
            ));
        }
    }
    Ok(())
}
pub(super) fn validate_future(options: &StashBatchOptions, item: &Item) -> PResult<()> {
    let Some(plan) = entry(options, item)? else {
        return Ok(());
    };
    if let Some(capture) = &item.capture {
        if plan.source_sha256 != capture.identity.sha256
            || capture.scene_id != plan.scene_id
            || capture.file_id != plan.file_id
            || item.view.source_version.as_ref() != Some(&capture.source_version)
            || plan
                .placement
                .as_ref()
                .is_some_and(|p| p.source_version != capture.source_version)
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "source plan differs from captured Stash media",
            ));
        }
    }
    Ok(())
}
pub(super) fn placement_command(
    options: &StashBatchOptions,
    item: &Item,
) -> PResult<Option<(&'static str, Command)>> {
    validate_future(options, item)?;
    if item.capture.is_none() && !options.source_plans.is_empty() {
        return Err(ProtocolError::invalid(
            "source plan requires acknowledged media capture",
        ));
    }
    let desired = entry(options, item)?.and_then(|p| p.placement.as_ref());
    if desired == item.placement.as_ref().map(|p| &p.spec) {
        return Ok(None);
    }
    Ok(Some(match desired {
        Some(spec) => (
            "place",
            Command::SetSourcePlacement {
                spec: spec.clone(),
                replace: item.placement.as_ref().map(|p| p.placement_id.clone()),
            },
        ),
        None => (
            "unplace",
            Command::RemoveSourcePlacement {
                placement_id: item
                    .placement
                    .as_ref()
                    .ok_or_else(|| ProtocolError::invalid("batch placement absent"))?
                    .placement_id
                    .clone(),
            },
        ),
    }))
}
pub(super) fn acknowledge(
    item: &mut Item,
    pending: &Pending,
    project: &ProjectSnapshot,
) -> PResult<()> {
    if item.view.project_id.as_ref() != Some(&project.project_id)
        || pending.project.as_ref() != Some(&project.project_id)
        || pending.revision != Some(item.revision)
        || project.revision != item.revision.checked_next().map_err(internal)?
    {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "placement acknowledgement differs from batch project/revision",
        ));
    }
    let placement = match &pending.command {
        Command::SetSourcePlacement { spec, replace } if pending.phase == "place" => {
            if replace.as_ref() != item.placement.as_ref().map(|p| &p.placement_id) {
                return Err(ProtocolError::invalid(
                    "placement replacement acknowledgement differs",
                ));
            }
            let matching: Vec<_> = project
                .placements
                .iter()
                .filter(|p| p.spec == *spec && p.created_revision == project.revision)
                .collect();
            if matching.len() != 1
                || replace
                    .as_ref()
                    .is_some_and(|id| project.placements.iter().any(|p| &p.placement_id == id))
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "placement acknowledgement does not identify one new active definition",
                ));
            }
            Some(matching[0].clone())
        }
        Command::RemoveSourcePlacement { placement_id } if pending.phase == "unplace" => {
            if item.placement.as_ref().map(|p| &p.placement_id) != Some(placement_id)
                || project
                    .placements
                    .iter()
                    .any(|p| &p.placement_id == placement_id)
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "removed batch placement remains active",
                ));
            }
            None
        }
        _ => {
            return Err(ProtocolError::invalid(
                "not a batch placement acknowledgement",
            ))
        }
    };
    item.placement = placement;
    item.revision = project.revision;
    item.cursor = 2;
    Ok(())
}
pub(super) fn generation_command(options: &StashBatchOptions, item: &Item) -> PResult<Command> {
    validate_future(options, item)?;
    let capture = item
        .capture
        .as_ref()
        .ok_or_else(|| ProtocolError::invalid("source plan requires retained capture"))?;
    let mut resolved = options.clone();
    resolved.settings = resolve_stash_batch_source_settings(
        &options.settings,
        &options.annotated_views,
        &options.source_plans,
        capture,
        item.placement.as_ref(),
    )?;
    Ok(batch_generation_command(
        &resolved,
        capture.source_version.clone(),
    ))
}
pub(super) fn requires_reader(record: &Record) -> bool {
    !record.options.source_plans.is_empty()
        || record
            .migration_history
            .iter()
            .any(|h| !h.previous_options.source_plans.is_empty())
        || record
            .items
            .iter()
            .any(|i| i.placement.is_some() || i.generation_plan.is_some())
}
pub(super) fn validate_record(record: &Record) -> Result<()> {
    anyhow::ensure!(
        (record.report.schema_version == 6) == requires_reader(record),
        "source plan batch requires its version-6 ledger reader"
    );
    if !requires_reader(record) {
        return Ok(());
    }
    validate_selection(&record.options, &record.items)?;
    for item in &record.items {
        anyhow::ensure!(
            item.cursor < 3 || item.placement.is_none() || item.generation_plan.is_some(),
            "placed generation is missing its frozen source plan"
        );
        if let Some(p) = &item.placement {
            p.spec.validate()?;
            anyhow::ensure!(
                p.created_revision <= item.revision
                    && item.view.source_version.as_ref() == Some(&p.spec.source_version),
                "batch placement source/revision differs"
            );
        }
        if let Some(plan) = &item.generation_plan {
            plan.validate()?;
            let known = std::iter::once(&record.options)
                .chain(record.migration_history.iter().map(|h| &h.previous_options))
                .any(|o| {
                    o.source_plans
                        .iter()
                        .any(|p| encode(p).ok() == encode(plan).ok())
                });
            let capture = item
                .capture
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("frozen batch plan capture absent"))?;
            anyhow::ensure!(
                known
                    && plan.source_sha256 == capture.identity.sha256
                    && plan.scene_id == item.view.scene_id
                    && Some(&plan.file_id) == item.view.file_id.as_ref()
                    && capture.scene_id == plan.scene_id
                    && capture.file_id == plan.file_id
                    && item.view.source_version.as_ref() == Some(&capture.source_version)
                    && plan.placement.as_ref() == item.placement.as_ref().map(|p| &p.spec),
                "frozen generation plan differs from retained source/placement intent"
            );
        }
        if item.cursor == 2 {
            if let Some(pending) = &item.pending {
                let (phase, command) = if pending.phase == "generate" {
                    anyhow::ensure!(
                        encode(&item.generation_plan)? == encode(&entry(&record.options, item)?)?,
                        "pending generation clock intent differs"
                    );
                    (
                        "generate",
                        annotations::generation_command(&record.options, item)?,
                    )
                } else {
                    placement_command(&record.options, item)?
                        .ok_or_else(|| anyhow::anyhow!("unexpected batch placement effect"))?
                };
                anyhow::ensure!(
                    pending.phase == phase
                        && encode(&pending.command)? == encode(&command)?
                        && pending.project == item.view.project_id
                        && pending.revision == Some(item.revision)
                        && item.view.requests.iter().any(|r| r.phase == phase
                            && r.request_id == pending.id
                            && r.expected_revision == pending.revision),
                    "retained batch placement/generation intent differs"
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn persist(engine: &Engine, owner: &SessionId, record: &Record) {
        let mut db = engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        annotations::arm_reader(&tx, record).unwrap();
        tx.execute(
            "UPDATE stash_batches SET state=?3,record=?4 WHERE session=?1 AND request=?2",
            params![
                owner.as_str(),
                record.report.batch_request_id.as_str(),
                record.report.state,
                encode(record).unwrap()
            ],
        )
        .unwrap();
        tx.commit().unwrap();
    }
    fn captured() -> (
        tempfile::TempDir,
        EngineConfig,
        Arc<Engine>,
        SessionId,
        String,
        Record,
    ) {
        let (dir, config, engine, owner, token, mut record, create) = super::super::tests::setup();
        let ResponseBody::Project(project) = engine
            .batch_call(&owner, &record.report.batch_request_id, &create)
            .unwrap()
        else {
            panic!()
        };
        let file = dir.path().join("authored.mkv");
        fs::write(&file, b"authored placement custody; worker not run").unwrap();
        let ResponseBody::Source { source_version } = engine
            .handle(
                Request::new(id!(RequestId), Command::ImportSource { path: file.clone() })
                    .with_session(owner.clone())
                    .with_auth_token(token.clone())
                    .in_project(project.project_id.clone(), None),
            )
            .result
            .unwrap()
        else {
            panic!()
        };
        let identity = engine
            .source(
                &engine.db.lock().unwrap(),
                &project.project_id,
                &source_version,
            )
            .unwrap()
            .identity;
        let spec = SourcePlacementSpec {
            source_version: source_version.clone(),
            source_start: SourceTimestamp::new(0, 1).unwrap(),
            source_end: SourceTimestamp::new(10, 1).unwrap(),
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::from_nanos(2_000_000_000),
            rate_numerator: 2,
            rate_denominator: 1,
        };
        record.options.source_plans = vec![StashBatchSourcePlan {
            scene_id: "authored".into(),
            file_id: "file".into(),
            source_sha256: identity.sha256.clone(),
            placement: Some(spec),
            project_interval: Some(
                TimeRange::new(
                    ProjectTime::from_nanos(3_000_000_000),
                    ProjectTime::from_nanos(5_000_000_000),
                )
                .unwrap(),
            ),
            source_time: true,
        }];
        record.report.schema_version = 6;
        let item = &mut record.items[0];
        item.view.project_id = Some(project.project_id);
        item.revision = project.revision;
        item.view.source_version = Some(source_version.clone());
        item.cursor = 2;
        item.pending = None;
        item.capture = Some(StashSourceCapture {
            endpoint: None,
            source_version,
            identity,
            scene_id: "authored".into(),
            file_id: "file".into(),
            server_path: "/stash/authored.mkv".into(),
            local_source: file,
            script_destination: dir.path().join("authored.funscript"),
        });
        persist(&engine, &owner, &record);
        (dir, config, engine, owner, token, record)
    }
    fn advance(engine: &Arc<Engine>, owner: &SessionId, record: &mut Record) {
        engine
            .advance_batch_item(
                owner,
                &record.report.batch_request_id,
                &record.options,
                &record.worker_stamp,
                &mut record.items[0],
            )
            .unwrap();
        persist(engine, owner, record);
    }
    fn migration(
        engine: &Engine,
        owner: &SessionId,
        token: &str,
        record: &Record,
        replacement: Vec<StashBatchSourcePlan>,
    ) -> Request {
        let raw = encode(record).unwrap();
        Request::new(
            id!(RequestId),
            Command::StashMigrateBatch {
                batch_request_id: record.report.batch_request_id.clone(),
                expected_ledger_sha256: sha256_text(&raw),
                generation: StashBatchGeneration {
                    source_plans: replacement,
                    preset: record.options.preset.clone(),
                    settings: record.options.settings.clone(),
                    annotated_views: vec![],
                    model_path: None,
                    model_pack: None,
                },
                owner_proof: engine.stash_owner_token.clone(),
            },
        )
        .with_session(owner.clone())
        .with_auth_token(token.into())
    }

    #[test]
    fn durable_placement_acknowledgement_precedes_exact_generation_and_clock_intent() {
        let (_dir, _config, engine, owner, _token, mut record) = captured();
        advance(&engine, &owner, &mut record);
        let pending = record.items[0].pending.clone().unwrap();
        assert_eq!(pending.phase, "place");
        assert!(record.items[0].placement.is_none());
        assert!(record.items[0].generation_plan.is_none());
        validate_record(&record).unwrap();
        advance(&engine, &owner, &mut record);
        let placement = record.items[0].placement.clone().unwrap();
        assert_eq!(placement.created_revision, record.items[0].revision);
        assert_eq!(
            placement.spec,
            record.options.source_plans[0].placement.clone().unwrap()
        );
        advance(&engine, &owner, &mut record);
        let pending = record.items[0].pending.as_ref().unwrap();
        assert_eq!(pending.phase, "generate");
        let Command::Generate {
            settings: Some(settings),
            ..
        } = &pending.command
        else {
            panic!()
        };
        assert_eq!(settings.source_placement, Some(placement));
        assert_eq!(
            settings.project_interval,
            record.options.source_plans[0].project_interval
        );
        assert!(
            record.items[0]
                .generation_plan
                .as_ref()
                .unwrap()
                .source_time
        );
        validate_record(&record).unwrap();
        let mut tampered = record.clone();
        tampered.items[0]
            .generation_plan
            .as_mut()
            .unwrap()
            .source_time = false;
        assert!(validate_record(&tampered).is_err());
    }

    #[test]
    fn committed_placement_replays_after_reopen_without_new_definition_or_revision() {
        let (_dir, config, engine, owner, _token, mut record) = captured();
        advance(&engine, &owner, &mut record);
        let pending = record.items[0].pending.clone().unwrap();
        let ResponseBody::Project(original) = engine
            .batch_call(&owner, &record.report.batch_request_id, &pending)
            .unwrap()
        else {
            panic!()
        };
        record.report.state = "needs_attention".into();
        persist(&engine, &owner, &record);
        drop(engine);
        let engine = Engine::open(config).unwrap();
        record.report.state = "running".into();
        persist(&engine, &owner, &record);
        advance(&engine, &owner, &mut record);
        assert_eq!(record.items[0].revision, original.revision);
        assert_eq!(
            record.items[0].placement,
            Some(original.placements[0].clone())
        );
        let count: i64 = engine
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM source_placement_definitions",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            record.items[0]
                .view
                .requests
                .iter()
                .filter(|r| r.phase == "place")
                .count(),
            1
        );
    }

    #[test]
    fn migration_reconciles_committed_placement_then_replaces_or_removes_it_explicitly() {
        for remove in [false, true] {
            let (_dir, _config, engine, owner, token, mut record) = captured();
            advance(&engine, &owner, &mut record);
            let pending = record.items[0].pending.clone().unwrap();
            let ResponseBody::Project(placed) = engine
                .batch_call(&owner, &record.report.batch_request_id, &pending)
                .unwrap()
            else {
                panic!()
            };
            record.report.state = "needs_attention".into();
            persist(&engine, &owner, &record);
            let mut replacement = record.options.source_plans.clone();
            if remove {
                replacement.clear();
            } else {
                replacement[0].placement.as_mut().unwrap().rate_numerator = -2;
            }
            let request = migration(&engine, &owner, &token, &record, replacement);
            let ResponseBody::StashBatch(report) = engine.handle(request.clone()).result.unwrap()
            else {
                panic!()
            };
            assert_eq!(report.state, "needs_attention");
            assert_eq!(report.schema_version, 6);
            assert_eq!(
                report.migrations[0].reconciled_placement_requests,
                vec![pending.id]
            );
            assert!(report.migrations[0]
                .superseded_placement_requests
                .is_empty());
            assert!(engine.handle(request).result.is_ok());
            record = engine
                .load_batch(&owner, &record.report.batch_request_id)
                .unwrap()
                .1;
            assert_eq!(
                record.items[0].placement,
                Some(placed.placements[0].clone())
            );
            record.report.state = "running".into();
            persist(&engine, &owner, &record);
            advance(&engine, &owner, &mut record);
            assert_eq!(
                record.items[0].pending.as_ref().unwrap().phase,
                if remove { "unplace" } else { "place" }
            );
            advance(&engine, &owner, &mut record);
            if remove {
                assert!(record.items[0].placement.is_none());
            } else {
                assert_eq!(
                    record.items[0]
                        .placement
                        .as_ref()
                        .unwrap()
                        .spec
                        .rate_numerator,
                    -2
                );
            }
            assert_eq!(
                record.items[0].revision.value(),
                placed.revision.value() + 1
            );
            advance(&engine, &owner, &mut record);
            validate_record(&record).unwrap();
        }
    }

    #[test]
    fn migration_supersedes_only_unaccepted_placement_and_retains_accepted_job_clock() {
        let (_dir, _config, engine, owner, token, mut record) = captured();
        advance(&engine, &owner, &mut record);
        let old = record.items[0].pending.clone().unwrap();
        record.report.state = "needs_attention".into();
        persist(&engine, &owner, &record);
        let mut replacement = record.options.source_plans.clone();
        replacement[0].placement.as_mut().unwrap().rate_numerator = 3;
        let response = engine
            .handle(migration(&engine, &owner, &token, &record, replacement))
            .result
            .unwrap();
        let ResponseBody::StashBatch(report) = response else {
            panic!()
        };
        assert_eq!(
            report.migrations[0].superseded_placement_requests,
            vec![old.id]
        );
        record = engine
            .load_batch(&owner, &record.report.batch_request_id)
            .unwrap()
            .1;
        assert!(record.items[0].placement.is_none());
        record.report.state = "running".into();
        persist(&engine, &owner, &record);
        advance(&engine, &owner, &mut record);
        advance(&engine, &owner, &mut record);
        advance(&engine, &owner, &mut record);
        let pending = record.items[0].pending.clone().unwrap();
        let frozen = encode(&record.items[0].generation_plan).unwrap();
        // Model the lost ledger acknowledgement at the authoritative journal
        // seam. Native jobs/publication are exercised by the external campaign.
        let job = JobSnapshot {
            job_id: id!(JobId),
            attempt_id: id!(AttemptId),
            project_id: pending.project.clone().unwrap(),
            base_revision: pending.revision.unwrap(),
            state: "queued".into(),
            candidate_id: None,
            error: None,
        };
        let mut accepted =
            Request::new(pending.id.clone(), pending.command.clone()).with_session(owner.clone());
        accepted.project = pending.project.clone();
        accepted.expected_revision = pending.revision;
        let fp = format!(
            "{:x}",
            Sha256::digest(request_fingerprint(&accepted).unwrap())
        );
        {
            let mut db = engine.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            remember(
                &tx,
                owner.as_str(),
                &accepted,
                &fp,
                &ResponseBody::Job(job.clone()),
            )
            .unwrap();
            tx.commit().unwrap();
        }
        record.report.state = "needs_attention".into();
        persist(&engine, &owner, &record);
        let ResponseBody::StashBatch(report) = engine
            .handle(migration(&engine, &owner, &token, &record, vec![]))
            .result
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            report.migrations[1].reconciled_generation_requests,
            vec![pending.id]
        );
        record = engine
            .load_batch(&owner, &record.report.batch_request_id)
            .unwrap()
            .1;
        assert_eq!(record.items[0].cursor, 3);
        assert_eq!(record.items[0].view.job_id, Some(job.job_id));
        assert_eq!(encode(&record.items[0].generation_plan).unwrap(), frozen);
        assert!(record.options.source_plans.is_empty());
        validate_record(&record).unwrap();
        record.items[0].generation_plan = None;
        assert!(validate_record(&record).is_err());
    }

    #[test]
    fn coverage_capture_and_reader_corruption_refuse_without_new_effects() {
        let (_dir, _config, engine, owner, _token, mut record) = captured();
        let mut bad = record.options.clone();
        bad.source_plans[0].file_id = "other".into();
        assert!(validate_selection(&bad, &record.items).is_err());
        bad = record.options.clone();
        bad.source_plans[0].source_sha256 = "b".repeat(64);
        assert_eq!(
            placement_command(&bad, &record.items[0]).unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        assert!(record.items[0]
            .view
            .requests
            .iter()
            .all(|r| r.phase != "place" && r.phase != "generate"));
        advance(&engine, &owner, &mut record);
        validate_record(&record).unwrap();
        let mut bad = record.clone();
        bad.report.schema_version = 5;
        assert!(validate_record(&bad).is_err());
        bad = record.clone();
        if let Command::SetSourcePlacement { spec, .. } =
            &mut bad.items[0].pending.as_mut().unwrap().command
        {
            spec.rate_numerator = 7;
        }
        assert!(validate_record(&bad).is_err());
        let db = engine.db.lock().unwrap();
        annotations::validate_reader_requirement(&db).unwrap();
        db.execute_batch("PRAGMA user_version=12").unwrap();
        assert!(annotations::validate_reader_requirement(&db).is_err());
    }
}
