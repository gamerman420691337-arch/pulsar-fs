//! Scoped imports, selective candidate commits and read-only review diagnostics.
use super::*;
use std::io::Read;

pub(super) struct PreparedFunscript {
    pub(super) program: ProgramDescriptor,
    pub(super) identity: ArtifactIdentity,
    pub(super) path: PathBuf,
    pub(super) label: String,
    pub(super) review: Vec<ReviewFlag>,
}

#[derive(Deserialize)]
struct FunscriptInput {
    actions: Vec<FunscriptAction>,
    #[serde(default)]
    inverted: bool,
}
#[derive(Deserialize)]
struct FunscriptAction {
    at: u64,
    pos: i64,
}

impl Engine {
    pub(super) fn prepare_funscript(&self, source: &Path) -> PResult<PreparedFunscript> {
        self.prepare_funscript_policy(source, false)
    }
    pub(super) fn prepare_funscript_for_repair(&self, source: &Path) -> PResult<PreparedFunscript> {
        self.prepare_funscript_policy(source, true)
    }
    fn prepare_funscript_policy(
        &self,
        source: &Path,
        sanitize: bool,
    ) -> PResult<PreparedFunscript> {
        let (path, hash, bytes) = artifacts::snapshot_reserved(
            source,
            &self.config.state_dir.join("snapshots"),
            MAX_MOTION_BYTES.min(self.config.max_snapshot_bytes),
            self.pool.reserved_storage(),
        )
        .map_err(|error| ProtocolError::invalid(error.to_string()))?;
        let mut input: FunscriptInput = serde_json::from_reader(
            artifacts::open_regular(&path)
                .map_err(unavailable)?
                .take(MAX_MOTION_BYTES + 1),
        )
        .map_err(|error| ProtocolError::invalid(format!("invalid funscript: {error}")))?;
        if input.actions.is_empty() {
            return Err(ProtocolError::invalid("funscript contains no actions"));
        }
        if input.actions.len() > EDIT_VALUES_MAX_ACTIONS {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "funscript exceeds action budget",
            ));
        }
        if sanitize {
            input.actions.sort_by_key(|a| a.at);
            for action in &mut input.actions {
                action.pos = action.pos.clamp(0, 100);
            }
            input.actions.dedup_by(|later, earlier| {
                if later.at == earlier.at {
                    earlier.pos = later.pos;
                    true
                } else {
                    false
                }
            });
        }
        let mut actions = Vec::with_capacity(input.actions.len());
        for action in input.actions {
            if !(0..=100).contains(&action.pos) {
                return Err(ProtocolError::invalid("funscript position exceeds 100"));
            }
            let time = action
                .at
                .checked_mul(1_000_000)
                .and_then(|time| i64::try_from(time).ok())
                .ok_or_else(|| ProtocolError::invalid("funscript timestamp overflow"))?;
            let position = (if input.inverted {
                100 - action.pos
            } else {
                action.pos
            }) as f64
                / 100.0;
            actions.push(
                MotionAction::new(
                    ProjectTime::from_nanos(time),
                    NormalizedPosition::new(position)
                        .map_err(|error| ProtocolError::invalid(error.to_string()))?,
                    EvidenceKind::Synthesized,
                )
                .map_err(|error| ProtocolError::invalid(error.to_string()))?,
            );
        }
        let track = MotionTrack::new(Axis::Stroke, actions)
            .map_err(|error| ProtocolError::invalid(error.to_string()))?;
        let range = track
            .span()
            .map_err(|error| ProtocolError::invalid(error.to_string()))?
            .ok_or_else(|| ProtocolError::invalid("funscript has no time span"))?;
        let program = MotionProgram::new(vec![track]).map_err(internal)?;
        validate_program(&program)?;
        let review = vec![ReviewFlag {
            axis: Some(Axis::Stroke),
            range,
            reason: ReviewReason::new(if sanitize {
                "imported_motion_sanitized"
            } else {
                "imported_motion"
            })?,
            evidence: EvidenceKind::Synthesized,
            confidence: None,
        }];
        Ok(PreparedFunscript {
            program: self.motion_store.publish(&program)?,
            review,
            path,
            label: source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            identity: ArtifactIdentity {
                artifact_id: ArtifactId::new(hash.clone()).map_err(internal)?,
                sha256: hash,
                byte_len: bytes,
            },
        })
    }

    pub(super) fn materialize_input(&self, input: &GenerationInput) -> PResult<SourceArtifact> {
        let bytes = serde_json::to_vec(input).map_err(internal)?;
        if bytes.len() > PROGRAM_LIMIT {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "generation input exceeds bounded control snapshot",
            ));
        }
        let mut temporary =
            tempfile::NamedTempFile::new_in(self.config.state_dir.join("snapshots"))
                .map_err(unavailable)?;
        temporary.write_all(&bytes).map_err(unavailable)?;
        temporary.as_file().sync_all().map_err(unavailable)?;
        self.snapshot_model(temporary.path())
    }

    pub(super) fn import_funscript(
        &self,
        tx: &Transaction<'_>,
        request: &Request,
        prepared: PreparedFunscript,
    ) -> PResult<ResponseBody> {
        self.stage_captured_candidate(tx, request, prepared, SourceKind::Funscript)
    }

    pub(super) fn stage_captured_candidate(
        &self,
        tx: &Transaction<'_>,
        request: &Request,
        prepared: PreparedFunscript,
        kind: SourceKind,
    ) -> PResult<ResponseBody> {
        let snapshot = project_snapshot(tx, project(request)?)?;
        let candidate = id!(CandidateId);
        let registered: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sources WHERE project=?1 AND version=?2)",
                params![snapshot.project_id.as_str(), prepared.identity.sha256],
                |row| row.get(0),
            )
            .map_err(internal)?;
        let count: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM sources WHERE project=?1",
                [snapshot.project_id.as_str()],
                |row| row.get(0),
            )
            .map_err(internal)?;
        if !registered && count >= 128 {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "project source catalog is full",
            ));
        }
        tx.execute(
            "INSERT INTO sources(project,version,path,original,label,sha256,bytes,kind)
            VALUES(?1,?2,?3,'',?4,?2,?5,?6) ON CONFLICT(project,version)
            DO UPDATE SET evicted=0,path=excluded.path,kind=excluded.kind",
            params![
                snapshot.project_id.as_str(),
                prepared.identity.sha256,
                prepared.path.to_string_lossy(),
                prepared.label,
                i64::try_from(prepared.identity.byte_len).map_err(internal)?,
                kind.as_str()
            ],
        )
        .map_err(internal)?;
        tx.execute("INSERT INTO candidates(id,project,base_revision,program,job,lineage,review) VALUES(?1,?2,?3,?4,NULL,?5,?6)",
            params![candidate.as_str(), snapshot.project_id.as_str(), rev_sql(snapshot.revision)?,
                motion_state::register(tx,&prepared.program)?, encode(&vec![prepared.identity])?, encode(&prepared.review)?])
            .map_err(internal)?;
        event(
            tx,
            &snapshot.project_id,
            snapshot.revision,
            EventBody::CandidateReady {
                candidate_id: candidate.clone(),
            },
        )?;
        Ok(ResponseBody::Candidate(
            (candidate_snapshot(tx, &candidate, &snapshot.project_id)?).into(),
        ))
    }

    pub(super) fn merge_candidate(
        &self,
        tx: &Transaction<'_>,
        request: &Request,
        session: &SessionId,
        candidate_id: &CandidateId,
        axes: &[Axis],
        range: Option<TimeRange>,
    ) -> PResult<ResponseBody> {
        let candidate = candidate_state(tx, candidate_id, project(request)?)?;
        let snapshot = project_state(tx, project(request)?)?;
        if candidate.base_revision != snapshot.revision {
            return Err(ProtocolError::new(
                ErrorCode::RevisionConflict,
                "candidate is stale; explicitly rebase before merging",
            ));
        }
        let committed: Option<i64> = tx
            .query_row(
                "SELECT committed_revision FROM candidates WHERE id=?1",
                [candidate_id.as_str()],
                |row| row.get(0),
            )
            .map_err(internal)?;
        if committed.is_some() {
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "candidate already committed",
            ));
        }
        if let Some(interval) =
            candidate_generation_interval(tx, candidate_id, &snapshot.project_id)?
        {
            if range.is_none_or(|selected| {
                selected.start() < interval.start() || selected.end() > interval.end()
            }) {
                return Err(ProtocolError::invalid("interval candidate requires an explicit merge range contained in its original project interval"));
            }
        }
        let protected = protected_regions(tx, &snapshot.project_id)?;
        let merged = pulsar_core::merge_candidate_preserving_protection(
            &snapshot.program,
            &candidate.program,
            axes,
            range,
            &protected,
        )
        .map_err(|error| ProtocolError::invalid(error.to_string()))?;
        if merged == snapshot.program {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "selection contains no writable change; protected segments were preserved",
            ));
        }
        let revision = commit_program(
            tx,
            request,
            session,
            &merged,
            "candidate_merge",
            "Merge selected candidate motion",
            true,
        )?;
        tx.execute(
            "UPDATE candidates SET committed_revision=?2 WHERE id=?1",
            params![candidate_id.as_str(), rev_sql(revision)?],
        )
        .map_err(internal)?;
        Ok(ResponseBody::Project(
            (project_snapshot(tx, &snapshot.project_id)?).into(),
        ))
    }

    pub(super) fn diagnostics(
        &self,
        tx: &Transaction<'_>,
        request: &Request,
        candidate_id: Option<&CandidateId>,
        maximum_speed_percent_per_second: Option<f64>,
    ) -> PResult<ResponseBody> {
        let snapshot = project_state(tx, project(request)?)?;
        let candidate = candidate_id
            .map(|candidate| candidate_state(tx, candidate, &snapshot.project_id))
            .transpose()?;
        let program = candidate
            .as_ref()
            .map(|candidate| &candidate.program)
            .unwrap_or(&snapshot.program);
        let review_state = if let Some(id) = candidate_id {
            lineage::candidate_review_state(tx, id)?
        } else {
            Some(lineage::state(tx, &snapshot.project_id, snapshot.revision)?)
        };
        let protected = protected_regions(tx, &snapshot.project_id)?;
        let segment_speed = maximum_speed_percent_per_second
            .map(|limit| {
                scan_segment_speeds(program, limit, 128)
                    .map_err(|error| ProtocolError::invalid(error.to_string()))
            })
            .transpose()?;
        let mut issues = Vec::new();
        if let Some(scan) = &segment_speed {
            for violation in &scan.violations {
                issues.push(DiagnosticIssue { code: "segment_average_speed_exceeded".into(),
                    message: format!("Average segment slope {:.6} exceeds {:.6} percent/second; this is not measured device velocity.", violation.average_percent_per_second, scan.maximum_percent_per_second),
                    axis: Some(violation.axis), range: Some(violation.range) });
            }
            if scan.truncated {
                issues.push(DiagnosticIssue { code: "segment_speed_truncated".into(),
                message: "Segment-speed scan stopped after its bounded violation page; counts cover only the visited prefix.".into(), axis: None, range: None });
            }
        }
        let imported = if let Some(id) = candidate_id {
            project_package_imports::candidate_review(tx, id)?.is_some()
        } else {
            project_package_imports::imported_revision(tx, &snapshot.project_id, snapshot.revision)?
        };
        if imported {
            issues.push(DiagnosticIssue{code:"imported_origin_unverified".into(),message:"Original archive review and evidence are retained exactly; imported provenance is not locally verified human resolution or authored-edit authority.".into(),axis:None,range:None});
        }
        if review_state.as_ref().is_some_and(|state| state.unknown) {
            issues.push(DiagnosticIssue { code: "review_history_unknown".into(),
                message: "Review ancestry is explicitly unknown; missing links are not inferred and no human resolution is claimed.".into(),
                axis: None, range: None });
        }
        if let Some(id) = candidate_id {
            let receipt:Option<(String,i64,i64)>=tx.query_row("SELECT receipt,authored_count,inherited_count FROM edit_candidate_lineage WHERE candidate=?1",
                [id.as_str()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional().map_err(internal)?;
            if let Some((raw, authored, inherited)) = receipt {
                let identity: ArtifactIdentity = decode(&raw)?;
                issues.push(DiagnosticIssue{code:"immutable_edit_receipt".into(),message:format!("Exact edit lineage retained in {} ({} authored intervals, {} inherited intervals); displayed reviews are conservative envelopes.",identity.artifact_id,authored,inherited),axis:None,range:None});
            }
        }
        if candidate
            .as_ref()
            .is_some_and(|candidate| candidate.base_revision != snapshot.revision)
        {
            issues.push(DiagnosticIssue {
                code: "candidate_stale".into(),
                message: "Candidate belongs to an earlier revision; explicit rebase is required."
                    .into(),
                axis: None,
                range: None,
            });
        }
        if program
            .tracks()
            .iter()
            .all(|track| track.actions().is_empty())
        {
            issues.push(DiagnosticIssue {
                code: "empty_program".into(),
                message: "No motion actions are available.".into(),
                axis: None,
                range: None,
            });
        }
        for track in program.tracks() {
            for gap in track.gaps() {
                if issues.len() >= 510 {
                    break;
                }
                issues.push(DiagnosticIssue { code: "unresolved_gap".into(), message: "Unavailable evidence remains a gap; standard export requires explicit resolution.".into(), axis: Some(track.axis()), range: Some(*gap) });
            }
        }
        for region in &protected {
            if issues.len() >= 510 {
                break;
            }
            issues.push(DiagnosticIssue {
                code: "protected_region".into(),
                message: "Selective merge preserves this region and its interpolation anchors."
                    .into(),
                axis: region.axis,
                range: Some(region.range),
            });
        }
        let unresolved = review_state
            .as_ref()
            .map(|state| state.unresolved())
            .transpose()?
            .unwrap_or_default();
        let reviews = candidate
            .as_ref()
            .map(|candidate| candidate.review.as_slice())
            .or(Some(unresolved.as_slice()))
            .unwrap_or(&[]);
        for flag in reviews {
            if issues.len() >= 510 {
                break;
            }
            let label = if review_state
                .as_ref()
                .is_some_and(|state| state.conservative)
            {
                "Conservative unresolved review envelope"
            } else if candidate.is_some() {
                "Candidate review required"
            } else {
                "Unresolved revision review"
            };
            issues.push(DiagnosticIssue {
                code: flag.reason.as_str().into(),
                message: format!("{label}: {} ({:?}).", flag.reason.as_str(), flag.evidence),
                axis: flag.axis,
                range: Some(flag.range),
            });
        }
        if issues.len() >= 510 {
            issues.push(DiagnosticIssue {
                code: "diagnostics_truncated".into(),
                message: "Diagnostic output reached its bounded page limit.".into(),
                axis: None,
                range: None,
            });
        }
        Ok(ResponseBody::Diagnostics(DiagnosticsReport {
            project_id: snapshot.project_id,
            revision: snapshot.revision,
            candidate_id: candidate_id.cloned(),
            base_revision: candidate.as_ref().map(|candidate| candidate.base_revision),
            protected_regions: protected,
            segment_speed,
            issues,
        }))
    }
}

fn protected_regions(db: &Connection, project: &ProjectId) -> PResult<Vec<ProtectedRegion>> {
    let raw: String = db
        .query_row(
            "SELECT protected FROM projects WHERE id=?1",
            [project.as_str()],
            |row| row.get(0),
        )
        .map_err(internal)?;
    decode(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        engine: Arc<Engine>,
        session: SessionId,
        token: String,
        project: ProjectId,
        dir: tempfile::TempDir,
    }
    impl Fixture {
        fn request(&self, command: Command, revision: Option<u64>) -> Request {
            Request::new(id!(RequestId), command)
                .with_session(self.session.clone())
                .with_auth_token(self.token.clone())
                .in_project(self.project.clone(), revision.map(RevisionId::new))
        }
        fn call(&self, command: Command, revision: Option<u64>) -> PResult<ResponseBody> {
            self.engine.handle(self.request(command, revision)).result
        }
        fn import(&self, bytes: &[u8], revision: u64) -> CandidateSnapshot {
            let path = self
                .dir
                .path()
                .join(format!("{}.funscript", Uuid::new_v4()));
            fs::write(&path, bytes).unwrap();
            match self
                .call(Command::ImportFunscript { path }, Some(revision))
                .unwrap()
            {
                ResponseBody::Candidate(candidate) => *candidate,
                _ => panic!("candidate expected"),
            }
        }
    }
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("worker");
        fs::write(&binary, b"nonexecuted editing test worker").unwrap();
        let engine = Engine::open(EngineConfig::new(dir.path().join("state"), binary)).unwrap();
        let (session, token) = match engine
            .handle(Request::new(
                id!(RequestId),
                Command::Pair {
                    client_name: "editing tests".into(),
                    pairing_token: engine.token.clone(),
                },
            ))
            .result
            .unwrap()
        {
            ResponseBody::Paired {
                session,
                auth_token,
            } => (session, auth_token),
            _ => panic!(),
        };
        let project = match engine
            .handle(
                Request::new(
                    id!(RequestId),
                    Command::CreateProject {
                        name: "editing".into(),
                    },
                )
                .with_session(session.clone())
                .with_auth_token(token.clone()),
            )
            .result
            .unwrap()
        {
            ResponseBody::Project(project) => project.project_id,
            _ => panic!(),
        };
        Fixture {
            engine,
            session,
            token,
            project,
            dir,
        }
    }
    fn dynamics_profile() -> DynamicsProfile {
        DynamicsProfile {
            axes: vec![AxisDynamics {
                axis: Axis::Stroke,
                unit: "mm".into(),
                minimum: 10.0,
                maximum: 90.0,
                max_velocity: 60.0,
                max_acceleration: 10000.0,
                max_jerk: 100000.0,
            }],
            coupled: vec![],
            minimum_gain: 0.01,
        }
    }
    #[test]
    fn stash_discovery_requires_authenticated_distinct_owner_authority_before_network() {
        let f = fixture();
        let command = |proof: String| Command::StashListMissing {
            connection: StashConnection {
                endpoint: "http://127.0.0.1:1/graphql".into(),
                credential_env: None,
            },
            limit: 10,
            owner_proof: proof,
        };
        let make = |command| {
            Request::new(id!(RequestId), command)
                .with_session(f.session.clone())
                .with_auth_token(f.token.clone())
        };
        assert_eq!(
            f.engine
                .handle(make(command("0".repeat(64))))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        assert_eq!(
            f.engine
                .handle(make(command(f.engine.package_owner_token.clone())))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        let valid = command(f.engine.stash_owner_token.clone());
        assert!(!format!("{:?}", valid).contains(&f.engine.stash_owner_token));
        let mut unauthorized = make(valid.clone());
        unauthorized.auth_token = Some("incorrect".into());
        assert_eq!(
            f.engine.handle(unauthorized).result.unwrap_err().code,
            ErrorCode::Unauthorized
        );
        assert_eq!(
            f.engine.handle(make(valid)).result.unwrap_err().code,
            ErrorCode::Unavailable
        );
    }
    #[test]
    fn stash_scan_and_tag_require_owner_and_current_export_grant_before_network() {
        let f = fixture();
        let connection = StashConnection {
            endpoint: "http://127.0.0.1:1/graphql".into(),
            credential_env: None,
        };
        let commands = [
            Command::StashSubmitScan {
                connection: connection.clone(),
                publication_request_id: id!(RequestId),
                owner_proof: f.engine.stash_owner_token.clone(),
            },
            Command::StashScanStatus {
                connection: connection.clone(),
                scan_request_id: id!(RequestId),
                owner_proof: f.engine.stash_owner_token.clone(),
            },
            Command::StashAddTag {
                connection: connection.clone(),
                scan_request_id: id!(RequestId),
                tag_name: "authored".into(),
                create: true,
                owner_proof: f.engine.stash_owner_token.clone(),
            },
            Command::StashTagStatus {
                connection: connection.clone(),
                tag_request_id: id!(RequestId),
                owner_proof: f.engine.stash_owner_token.clone(),
            },
            Command::StashResumeTag {
                connection,
                tag_request_id: id!(RequestId),
                owner_proof: f.engine.stash_owner_token.clone(),
            },
        ];
        let mut bad = commands[0].clone();
        if let Command::StashSubmitScan { owner_proof, .. } = &mut bad {
            *owner_proof = f.engine.package_owner_token.clone();
        }
        assert_eq!(f.call(bad, Some(0)).unwrap_err().code, ErrorCode::Forbidden);
        f.call(
            Command::Revoke {
                project_id: f.project.clone(),
                session: f.session.clone(),
            },
            None,
        )
        .unwrap();
        for command in commands {
            assert_eq!(
                f.call(command, Some(0)).unwrap_err().code,
                ErrorCode::Forbidden
            );
        }
    }
    #[cfg(unix)]
    #[test]
    fn stash_capture_uses_bound_immutable_bytes_and_durable_authorized_replay() {
        let f = fixture();
        let media = f.dir.path().join("source.mp4");
        fs::write(&media, b"authored immutable source").unwrap();
        let (connection,server)=crate::stash::tests::mock(vec![(200,String::new(),serde_json::json!({"data":{"findScene":{"id":"scene","title":"authored","interactive":false,"files":[{"id":"file","path":"/stash/source.mp4"}]}}}).to_string())]);
        let command = Command::StashImportSource {
            connection,
            scene_id: "scene".into(),
            file_id: "file".into(),
            expected_server_path: None,
            expected_source_version: None,
            mappings: vec![StashRootMapping {
                server_root: "/stash".into(),
                local_root: f.dir.path().into(),
            }],
            owner_proof: f.engine.stash_owner_token.clone(),
        };
        let request = f.request(command, Some(0));
        let mut stale = request.clone();
        stale.expected_revision = Some(RevisionId::new(1));
        assert_eq!(
            f.engine.handle(stale).result.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        let mut forbidden = request.clone();
        if let Command::StashImportSource { owner_proof, .. } = &mut forbidden.command {
            *owner_proof = f.engine.package_owner_token.clone();
        }
        assert_eq!(
            f.engine.handle(forbidden).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        let receipt = f.engine.handle(request.clone()).result.unwrap();
        let capture = match &receipt {
            ResponseBody::StashSourceCaptured(value) => value,
            _ => panic!(),
        };
        assert_eq!(capture.scene_id, "scene");
        assert_eq!(capture.file_id, "file");
        assert_eq!(capture.server_path, "/stash/source.mp4");
        assert_eq!(capture.local_source, media);
        assert_eq!(capture.identity.byte_len, 25);
        assert_eq!(capture.source_version.as_str(), capture.identity.sha256);
        assert_eq!(server.join().unwrap()[0]["variables"]["id"], "scene");
        let immutable = f
            .engine
            .config
            .state_dir
            .join("snapshots")
            .join(&capture.identity.sha256);
        fs::write(&media, b"later mutable contents").unwrap();
        assert_eq!(fs::read(&immutable).unwrap(), b"authored immutable source");
        assert!(!capture.script_destination.exists());
        let f = reopen_fixture(f);
        assert_eq!(
            encode(&f.engine.handle(request.clone()).result.unwrap()).unwrap(),
            encode(&receipt).unwrap()
        );
        f.call(
            Command::Revoke {
                project_id: f.project.clone(),
                session: f.session.clone(),
            },
            None,
        )
        .unwrap();
        assert_eq!(
            f.engine.handle(request).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
    }
    #[cfg(unix)]
    #[test]
    fn stash_publication_rejects_unrelated_master_and_stale_revision_before_effects() {
        let f = fixture();
        let media = f.dir.path().join("source.mp4");
        fs::write(&media, b"authored source").unwrap();
        let body=serde_json::json!({"data":{"findScene":{"id":"scene","title":"authored","interactive":false,"files":[{"id":"file","path":"/stash/source.mp4"}]}}}).to_string();
        let (connection, server) = crate::stash::tests::mock(vec![
            (200, String::new(), body.clone()),
            (200, String::new(), body),
        ]);
        let ResponseBody::StashSourceCaptured(capture) = f
            .call(
                Command::StashImportSource {
                    connection: connection.clone(),
                    scene_id: "scene".into(),
                    file_id: "file".into(),
                    expected_server_path: None,
                    expected_source_version: None,
                    mappings: vec![StashRootMapping {
                        server_root: "/stash".into(),
                        local_root: f.dir.path().into(),
                    }],
                    owner_proof: f.engine.stash_owner_token.clone(),
                },
                Some(0),
            )
            .unwrap()
        else {
            panic!()
        };
        let candidate = f.import(
            br#"{"actions":[{"at":0,"pos":20},{"at":1000,"pos":80}]}"#,
            0,
        );
        f.call(
            Command::CommitCandidate {
                candidate_id: candidate.candidate_id.clone(),
            },
            Some(0),
        )
        .unwrap();
        let command = Command::StashExportScript {
            connection,
            source_version: capture.source_version,
            scene_id: "scene".into(),
            file_id: "file".into(),
            candidate_id: candidate.candidate_id,
            job_id: id!(JobId),
            source_time: false,
            archived_origin: None,
            owner_proof: f.engine.stash_owner_token.clone(),
        };
        assert_eq!(
            f.call(command.clone(), Some(0)).unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            f.call(command, Some(1)).unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        assert!(!capture.script_destination.exists());
        assert_eq!(server.join().unwrap().len(), 2);
        let db = f.engine.db.lock().unwrap();
        let exports: i64 = db
            .query_row("SELECT COUNT(*) FROM exports", [], |row| row.get(0))
            .unwrap();
        assert_eq!(exports, 0);
    }
    #[test]
    fn device_motion_export_is_revision_bound_durable_and_preserves_master() {
        let f = fixture();
        let incoming = f.import(
            br#"{"actions":[{"at":0,"pos":20},{"at":1000,"pos":80}]}"#,
            0,
        );
        f.call(
            Command::CommitCandidate {
                candidate_id: incoming.candidate_id,
            },
            Some(0),
        )
        .unwrap();
        let path = f.dir.path().join("adapted.json");
        let request = f.request(
            Command::ExportDeviceMotion {
                path: path.clone(),
                profile: dynamics_profile(),
            },
            Some(1),
        );
        let receipt = f.engine.handle(request.clone()).result.unwrap();
        let (gain, profile_hash, master_hash) = match &receipt {
            ResponseBody::DeviceMotionExported {
                revision,
                gain,
                profile_sha256,
                master_sha256,
                ..
            } => {
                assert_eq!(*revision, RevisionId::new(1));
                (*gain, profile_sha256.clone(), master_sha256.clone())
            }
            _ => panic!(),
        };
        assert!(gain < 1.0);
        let raw = fs::read(&path).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(value["admitted"], false);
        assert_eq!(value["profile_sha256"], profile_hash);
        assert_eq!(value["master_sha256"], master_hash);
        assert_eq!(
            value["motion"]["tracks"][0]["actions"][1]["time"],
            1_000_000_000_i64
        );
        let snapshot = match f.call(Command::GetSnapshot, None).unwrap() {
            ResponseBody::Project(p) => p,
            _ => panic!(),
        };
        assert_eq!(snapshot.revision, RevisionId::new(1));
        assert_eq!(snapshot.motion.program.sha256, master_hash);
        let f = reopen_fixture(f);
        assert_eq!(
            encode(&f.engine.handle(request).result.unwrap()).unwrap(),
            encode(&receipt).unwrap()
        );
        assert_eq!(fs::read(&path).unwrap(), raw);
    }
    #[test]
    fn device_motion_export_rejects_stale_invalid_and_revoked_requests_without_effects() {
        let f = fixture();
        let incoming = f.import(
            br#"{"actions":[{"at":0,"pos":20},{"at":1000,"pos":80}]}"#,
            0,
        );
        f.call(
            Command::CommitCandidate {
                candidate_id: incoming.candidate_id,
            },
            Some(0),
        )
        .unwrap();
        let path = f.dir.path().join("adapted.json");
        let command = Command::ExportDeviceMotion {
            path: path.clone(),
            profile: dynamics_profile(),
        };
        assert_eq!(
            f.call(command.clone(), None).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            f.call(command.clone(), Some(0)).unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert!(!path.exists());
        let mut profile = dynamics_profile();
        profile.minimum_gain = 1.0;
        assert!(f
            .call(
                Command::ExportDeviceMotion {
                    path: path.clone(),
                    profile
                },
                Some(1)
            )
            .is_err());
        assert!(!path.exists());
        let request = f.request(command, Some(1));
        f.engine.handle(request.clone()).result.unwrap();
        f.call(
            Command::Revoke {
                session: f.session.clone(),
                project_id: f.project.clone(),
            },
            None,
        )
        .unwrap();
        assert_eq!(
            f.engine.handle(request).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
    }
    // Stage only a proposal; the public command still checks all authority and revision rules.
    fn commit_program(f: &Fixture, revision: u64, program: MotionProgram) -> Command {
        let candidate_id = id!(CandidateId);
        let db = f.engine.db.lock().unwrap();
        let reference = crate::authority::motion_state::save(&db, &program).unwrap();
        db.execute(
            "INSERT INTO candidates(id,project,base_revision,program,job,lineage,review) VALUES(?1,?2,?3,?4,NULL,'[]','[]')",
            params![candidate_id.as_str(), f.project.as_str(), revision as i64, reference],
        ).unwrap();
        Command::CommitCandidate { candidate_id }
    }
    #[test]
    fn funscript_import_is_revision_bound_durable_candidate_and_diagnostics_are_read_only() {
        let f = fixture();
        let path = f.dir.path().join("input.funscript");
        fs::write(
            &path,
            br#"{"actions":[{"at":0,"pos":20},{"at":1000,"pos":80}]}"#,
        )
        .unwrap();
        let request = f.request(Command::ImportFunscript { path: path.clone() }, Some(0));
        let first = f.engine.handle(request.clone()).result.unwrap();
        fs::write(path, b"changed original").unwrap();
        assert_eq!(
            encode(&first).unwrap(),
            encode(&f.engine.handle(request).result.unwrap()).unwrap()
        );
        let ResponseBody::Candidate(candidate) = first else {
            panic!()
        };
        assert_eq!(candidate.review[0].reason.as_str(), "imported_motion");
        let diagnostic = f.request(
            Command::Diagnostics {
                candidate_id: Some(candidate.candidate_id.clone()),
                maximum_speed_percent_per_second: None,
            },
            None,
        );
        for _ in 0..3 {
            f.engine.handle(diagnostic.clone()).result.unwrap();
        }
        let db = f.engine.db.lock().unwrap();
        assert_eq!(
            project_snapshot(&db, &f.project).unwrap().revision.value(),
            0
        );
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM requests WHERE request=?1",
                [diagnostic.request_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        assert!(project_state(&db, &f.project)
            .unwrap()
            .program
            .tracks()
            .is_empty());
    }
    #[test]
    fn import_rejects_missing_scope_stale_revision_and_malformed_signal() {
        let f = fixture();
        for raw in [
            br#"{"actions":[{"at":0,"pos":101}]}"#.as_slice(),
            br#"{"actions":[{"at":1,"pos":20},{"at":1,"pos":80}]}"#.as_slice(),
            br#"{"actions":[{"at":18446744073709551615,"pos":20}]}"#.as_slice(),
        ] {
            let path = f.dir.path().join("invalid.funscript");
            fs::write(&path, raw).unwrap();
            assert!(f.call(Command::ImportFunscript { path }, Some(0)).is_err());
        }
        let path = f.dir.path().join("valid.funscript");
        fs::write(&path, br#"{"actions":[{"at":0,"pos":20}]}"#).unwrap();
        assert_eq!(
            f.call(Command::ImportFunscript { path: path.clone() }, Some(99))
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let db = f.engine.db.lock().unwrap();
        db.execute(
            "DELETE FROM grants WHERE session=?1 AND project=?2 AND scope=?3",
            params![
                f.session.as_str(),
                f.project.as_str(),
                scope_name(Scope::ImportSource)
            ],
        )
        .unwrap();
        drop(db);
        assert_eq!(
            f.call(Command::ImportFunscript { path }, Some(0))
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
    }

    #[test]
    fn repair_import_sanitizes_explicitly_and_retains_raw_identity_and_replay() {
        let f = fixture();
        let path = f.dir.path().join("repair.funscript");
        let raw=br#"{"actions":[{"at":40,"pos":150},{"at":0,"pos":-5},{"at":20,"pos":2},{"at":10,"pos":1},{"at":20,"pos":100},{"at":100,"pos":50}]}"#;
        fs::write(&path, raw).unwrap();
        assert!(f
            .call(Command::ImportFunscript { path: path.clone() }, Some(0))
            .is_err());
        let request = f.request(
            Command::ImportFunscriptForRepair { path: path.clone() },
            Some(0),
        );
        let response = f.engine.handle(request.clone()).result.unwrap();
        let ResponseBody::Candidate(candidate) = &response else {
            panic!()
        };
        let program = f
            .engine
            .motion_store
            .read(&candidate.motion.program)
            .unwrap();
        let actions = program.track(Axis::Stroke).unwrap().actions();
        assert_eq!(actions.len(), 5);
        for (action, (t, p)) in
            actions
                .iter()
                .zip([(0, 0.0), (10, 1.0), (20, 100.0), (40, 100.0), (100, 50.0)])
        {
            assert_eq!(action.time().as_nanos(), t * 1_000_000);
            assert_eq!(action.position().value(), p / 100.0);
        }
        assert_eq!(
            candidate.review[0].reason.as_str(),
            "imported_motion_sanitized"
        );
        assert_eq!(fs::read(&path).unwrap(), raw);
        let hash = format!("{:x}", Sha256::digest(raw));
        let db = f.engine.db.lock().unwrap();
        let saved: String = db
            .query_row(
                "SELECT sha256 FROM sources WHERE project=?1",
                [f.project.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(saved, hash);
        drop(db);
        fs::write(&path, b"changed external input").unwrap();
        assert_eq!(
            encode(&response).unwrap(),
            encode(&f.engine.handle(request).result.unwrap()).unwrap()
        );
    }

    #[test]
    fn repair_import_requires_current_revision_and_refuses_invalid_clocks() {
        let f = fixture();
        let path = f.dir.path().join("repair-refusal.funscript");
        fs::write(&path, br#"{"actions":[{"at":0,"pos":20}]}"#).unwrap();
        assert!(f
            .call(
                Command::ImportFunscriptForRepair { path: path.clone() },
                None
            )
            .is_err());
        assert!(f
            .call(
                Command::ImportFunscriptForRepair { path: path.clone() },
                Some(99)
            )
            .is_err());
        for raw in [
            br#"{"actions":[]}"#.as_slice(),
            br#"{"actions":[{"at":-1,"pos":0}]}"#.as_slice(),
            br#"{"actions":[{"at":18446744073709551615,"pos":0}]}"#.as_slice(),
            b"not-json".as_slice(),
        ] {
            fs::write(&path, raw).unwrap();
            assert!(f
                .call(
                    Command::ImportFunscriptForRepair { path: path.clone() },
                    Some(0)
                )
                .is_err());
        }
    }
    #[test]
    fn selective_merge_preserves_protection_and_supports_undo_redo() {
        let f = fixture();
        let original = f.import(br#"{"actions":[{"at":0,"pos":0},{"at":100,"pos":100},{"at":200,"pos":20},{"at":300,"pos":80}]}"#, 0);
        f.call(
            Command::CommitCandidate {
                candidate_id: original.candidate_id,
            },
            Some(0),
        )
        .unwrap();
        let region = ProtectedRegion {
            axis: Some(Axis::Stroke),
            range: TimeRange::new(
                ProjectTime::from_nanos(20_000_000),
                ProjectTime::from_nanos(80_000_000),
            )
            .unwrap(),
        };
        f.call(
            Command::SetProtectedRegions {
                regions: vec![region],
            },
            Some(1),
        )
        .unwrap();
        let incoming = f.import(br#"{"actions":[{"at":0,"pos":100},{"at":50,"pos":50},{"at":100,"pos":0},{"at":200,"pos":90},{"at":300,"pos":10}]}"#, 2);
        let merge = f.request(
            Command::MergeCandidate {
                candidate_id: incoming.candidate_id.clone(),
                axes: vec![Axis::Stroke],
                range: None,
            },
            Some(2),
        );
        let ResponseBody::Project(merged) = f.engine.handle(merge.clone()).result.unwrap() else {
            panic!()
        };
        let merged_program = f.engine.motion_store.read(&merged.motion.program).unwrap();
        assert_eq!(
            merged_program.track(Axis::Stroke).unwrap().actions()[0]
                .position()
                .value(),
            0.0
        );
        assert_eq!(
            merged_program.track(Axis::Stroke).unwrap().actions()[1]
                .position()
                .value(),
            1.0
        );
        assert_eq!(
            merged_program.track(Axis::Stroke).unwrap().actions()[2]
                .position()
                .value(),
            0.9
        );
        assert_eq!(
            encode(&ResponseBody::Project(merged)).unwrap(),
            encode(&f.engine.handle(merge).result.unwrap()).unwrap()
        );
        f.call(Command::Undo, Some(3)).unwrap();
        f.call(Command::Redo, Some(4)).unwrap();
        assert_eq!(
            f.call(
                Command::MergeCandidate {
                    candidate_id: incoming.candidate_id,
                    axes: vec![Axis::Stroke],
                    range: None
                },
                Some(5)
            )
            .unwrap_err()
            .code,
            ErrorCode::RevisionConflict
        );
    }
    #[test]
    fn candidate_review_survives_rebase_and_revocation_denies_replay() {
        let f = fixture();
        let candidate = f.import(
            br#"{"actions":[{"at":0,"pos":20},{"at":1000,"pos":80}]}"#,
            0,
        );
        let request = f.request(
            Command::RebaseCandidate {
                candidate_id: candidate.candidate_id,
            },
            Some(0),
        );
        let ResponseBody::Candidate(rebased) = f.engine.handle(request.clone()).result.unwrap()
        else {
            panic!()
        };
        assert_eq!(
            encode(&rebased.review).unwrap(),
            encode(&candidate.review).unwrap()
        );
        f.engine
            .db
            .lock()
            .unwrap()
            .execute(
                "DELETE FROM grants WHERE session=?1 AND project=?2",
                params![f.session.as_str(), f.project.as_str()],
            )
            .unwrap();
        assert_eq!(
            f.engine.handle(request).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
    }
    #[test]
    fn explicit_axis_export_is_correct_revision_bound_and_has_axis_receipt() {
        let f = fixture();
        let make_track = |axis, position| {
            MotionTrack::new(
                axis,
                vec![
                    MotionAction::new(
                        ProjectTime::ZERO,
                        NormalizedPosition::new(position).unwrap(),
                        EvidenceKind::Synthesized,
                    )
                    .unwrap(),
                    MotionAction::new(
                        ProjectTime::from_nanos(1_000_000_000),
                        NormalizedPosition::new(position).unwrap(),
                        EvidenceKind::Synthesized,
                    )
                    .unwrap(),
                ],
            )
            .unwrap()
        };
        let program = MotionProgram::new(vec![
            make_track(Axis::Stroke, 0.2),
            make_track(Axis::Sway, 0.8),
        ])
        .unwrap();
        f.call(commit_program(&f, 0, program), Some(0)).unwrap();
        let ambiguous = f.dir.path().join("ambiguous.funscript");
        assert_eq!(
            f.call(
                Command::Export {
                    path: ambiguous.clone()
                },
                None
            )
            .unwrap_err()
            .code,
            ErrorCode::Unsupported
        );
        assert!(!ambiguous.exists());
        let path = f.dir.path().join("selected.sway.funscript");
        let command = Command::ExportAxis {
            path: path.clone(),
            axis: Axis::Sway,
        };
        assert_eq!(
            f.call(command.clone(), None).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            f.call(command.clone(), Some(0)).unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert!(!path.exists());
        let request = f.request(command, Some(1));
        let response = f.engine.handle(request.clone()).result.unwrap();
        let ResponseBody::AxisExported { axis, revision, .. } = &response else {
            panic!()
        };
        assert_eq!(*axis, Axis::Sway);
        assert_eq!(revision.value(), 1);
        let output: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(output["actions"][0]["pos"], 80);
        assert!(output.get("axis").is_none());
        assert!(output.get("lineage").is_none());
        assert_eq!(
            encode(&response).unwrap(),
            encode(&f.engine.handle(request.clone()).result.unwrap()).unwrap()
        );
        let db = f.engine.db.lock().unwrap();
        let axis: String = db
            .query_row(
                "SELECT axis FROM exports WHERE session=?1 AND request=?2",
                params![f.session.as_str(), request.request_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(axis, "sway");
        db.execute(
            "DELETE FROM grants WHERE session=?1 AND project=?2 AND scope=?3",
            params![
                f.session.as_str(),
                f.project.as_str(),
                scope_name(Scope::Export)
            ],
        )
        .unwrap();
        drop(db);
        assert_eq!(
            f.engine.handle(request).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
    }

    #[test]
    fn axis_export_rejects_absent_empty_gapped_and_quantization_collisions() {
        assert!(neutral_export_axis(&MotionProgram::default(), Axis::Stroke).is_err());
        let empty = MotionProgram::new(vec![MotionTrack::new(Axis::Yaw, vec![]).unwrap()]).unwrap();
        assert!(neutral_export_axis(&empty, Axis::Yaw).is_err());
        let action = |ns| {
            MotionAction::new(
                ProjectTime::from_nanos(ns),
                NormalizedPosition::new(0.5).unwrap(),
                EvidenceKind::Synthesized,
            )
            .unwrap()
        };
        let collision = MotionProgram::new(vec![MotionTrack::new(
            Axis::Pitch,
            vec![action(0), action(1)],
        )
        .unwrap()])
        .unwrap();
        assert!(neutral_export_axis(&collision, Axis::Pitch).is_err());
        let valid = MotionTrack::new(Axis::Stroke, vec![action(0), action(1_000_000)]).unwrap();
        let gapped = MotionTrack::with_gaps(
            Axis::Surge,
            vec![action(0), action(10_000_000)],
            vec![TimeRange::new(
                ProjectTime::from_nanos(1_000_000),
                ProjectTime::from_nanos(2_000_000),
            )
            .unwrap()],
        )
        .unwrap();
        let program = MotionProgram::new(vec![valid, gapped]).unwrap();
        assert!(neutral_export_axis(&program, Axis::Surge).is_err());
        assert!(neutral_export_axis(&program, Axis::Stroke).is_ok());
        assert!(neutral_export_axis(&program, Axis::Roll).is_err());
    }

    #[test]
    fn segment_speed_query_preserves_motion_protection_history_and_gap_uncertainty() {
        let f = fixture();
        let track = MotionTrack::with_gaps(
            Axis::Stroke,
            vec![
                MotionAction::new(
                    ProjectTime::ZERO,
                    NormalizedPosition::new(0.0).unwrap(),
                    EvidenceKind::Observed,
                )
                .unwrap(),
                MotionAction::new(
                    ProjectTime::from_nanos(100_000_000),
                    NormalizedPosition::new(1.0).unwrap(),
                    EvidenceKind::Observed,
                )
                .unwrap(),
                MotionAction::new(
                    ProjectTime::from_nanos(200_000_000),
                    NormalizedPosition::new(0.0).unwrap(),
                    EvidenceKind::Observed,
                )
                .unwrap(),
            ],
            vec![TimeRange::new(
                ProjectTime::from_nanos(150_000_000),
                ProjectTime::from_nanos(180_000_000),
            )
            .unwrap()],
        )
        .unwrap();
        let program = MotionProgram::new(vec![track]).unwrap();
        let commit = commit_program(&f, 0, program.clone());
        let Command::CommitCandidate { candidate_id } = &commit else {
            panic!()
        };
        let query = |candidate, limit| {
            f.call(
                Command::Diagnostics {
                    candidate_id: candidate,
                    maximum_speed_percent_per_second: limit,
                },
                None,
            )
            .unwrap()
        };
        let ResponseBody::Diagnostics(candidate) = query(Some(candidate_id.clone()), Some(450.0))
        else {
            panic!()
        };
        assert_eq!(candidate.revision.value(), 0);
        assert_eq!(candidate.segment_speed.unwrap().violations.len(), 1);
        f.call(commit, Some(0)).unwrap();
        let region = ProtectedRegion {
            axis: Some(Axis::Stroke),
            range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(100_000_000)).unwrap(),
        };
        f.call(
            Command::SetProtectedRegions {
                regions: vec![region],
            },
            Some(1),
        )
        .unwrap();
        let before = f
            .call(
                Command::OpenProject {
                    project_id: f.project.clone(),
                },
                None,
            )
            .unwrap();
        let review = f.call(Command::ReviewStatus, None).unwrap();
        for _ in 0..3 {
            let ResponseBody::Diagnostics(report) = query(None, Some(450.0)) else {
                panic!()
            };
            report.validate().unwrap();
            let scan = report.segment_speed.unwrap();
            assert_eq!(scan.scanned_segments, 2);
            assert_eq!(scan.unavailable_segments, 1);
            assert_eq!(scan.violations[0].average_percent_per_second, 1000.0);
            assert_eq!(report.protected_regions.len(), 1);
        }
        let ResponseBody::Diagnostics(report) = query(None, Some(1000.0)) else {
            panic!()
        };
        assert!(report.segment_speed.unwrap().violations.is_empty());
        let ResponseBody::Diagnostics(report) = query(None, None) else {
            panic!()
        };
        assert!(report.segment_speed.is_none());
        assert_eq!(
            encode(&before).unwrap(),
            encode(
                &f.call(
                    Command::OpenProject {
                        project_id: f.project.clone()
                    },
                    None
                )
                .unwrap()
            )
            .unwrap()
        );
        assert_eq!(
            encode(&review).unwrap(),
            encode(&f.call(Command::ReviewStatus, None).unwrap()).unwrap()
        );
        let f = reopen_fixture(f);
        let ResponseBody::Diagnostics(report) = f
            .call(
                Command::Diagnostics {
                    candidate_id: None,
                    maximum_speed_percent_per_second: Some(450.0),
                },
                None,
            )
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(report.segment_speed.unwrap().unavailable_segments, 1);
        f.call(Command::Undo, Some(report.revision.value()))
            .unwrap();
        let ResponseBody::Diagnostics(report) = f
            .call(
                Command::Diagnostics {
                    candidate_id: None,
                    maximum_speed_percent_per_second: Some(450.0),
                },
                None,
            )
            .unwrap()
        else {
            panic!()
        };
        assert!(report.protected_regions.is_empty());
    }

    fn project_review_issues(f: &Fixture) -> Vec<DiagnosticIssue> {
        match f
            .call(
                Command::Diagnostics {
                    candidate_id: None,
                    maximum_speed_percent_per_second: None,
                },
                None,
            )
            .unwrap()
        {
            ResponseBody::Diagnostics(report) => report.issues,
            _ => panic!("diagnostics expected"),
        }
    }
    fn reopen_fixture(f: Fixture) -> Fixture {
        let Fixture {
            engine,
            session,
            token,
            project,
            dir,
        } = f;
        let config = Engine::retire_for_test(engine);
        let engine = Engine::open(config).unwrap();
        Fixture {
            engine,
            session,
            token,
            project,
            dir,
        }
    }
    #[test]
    fn fixture_reopen_waits_for_a_retained_engine_owner() {
        let f = fixture();
        // Match a maintenance task retaining a strong owner after the caller drops it.
        let retained = f.engine.clone();
        let owner = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            drop(retained);
        });
        let reopened = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reopen_fixture(f)));
        owner.join().unwrap();
        assert!(
            reopened.is_ok(),
            "fixture reopened before retained Engine owner retired"
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn fixture_reopen_waits_for_independent_worker_fence_custody() {
        let f = fixture();
        let retained = f.engine.worker_fence.clone();
        let owner = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            drop(retained);
        });
        let reopened = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reopen_fixture(f)));
        owner.join().unwrap();
        assert!(
            reopened.is_ok(),
            "fixture reopened before independent fence custody retired"
        );
    }
    #[test]
    fn committed_reviews_and_explicit_history_edges_survive_restart_and_replay() {
        let f = fixture();
        let candidate = f.import(
            br#"{"actions":[{"at":0,"pos":20},{"at":1000,"pos":80}]}"#,
            0,
        );
        let commit = f.request(
            Command::CommitCandidate {
                candidate_id: candidate.candidate_id.clone(),
            },
            Some(0),
        );
        let committed = f.engine.handle(commit.clone()).result.unwrap();
        assert!(project_review_issues(&f)
            .iter()
            .any(|issue| issue.code == "imported_motion"));
        f.call(Command::SetProtectedRegions { regions: vec![] }, Some(1))
            .unwrap();
        f.call(Command::Undo, Some(2)).unwrap();
        f.call(Command::Redo, Some(3)).unwrap();
        let f = reopen_fixture(f);
        assert!(project_review_issues(&f)
            .iter()
            .any(|issue| issue.code == "imported_motion"));
        assert_eq!(
            encode(&committed).unwrap(),
            encode(&f.engine.handle(commit).result.unwrap()).unwrap()
        );
        let db = f.engine.db.lock().unwrap();
        let restored: (i64,i64,i64) = db.query_row(
            "SELECT parent_revision,restored_from_revision,restored_history_position FROM revision_lineage WHERE project=?1 AND revision=4",
            [f.project.as_str()], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).unwrap();
        assert_eq!(restored, (3, 2, 2));
        let origin: String = db
            .query_row(
                "SELECT candidate FROM revision_lineage WHERE project=?1 AND revision=1",
                [f.project.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(origin, candidate.candidate_id.as_str());
        assert_eq!(
            project_snapshot(&db, &f.project).unwrap().revision.value(),
            4
        );
    }
    fn review_status(f: &Fixture) -> ReviewStatus {
        match f.call(Command::ReviewStatus, None).unwrap() {
            ResponseBody::ReviewStatus(r) => r,
            _ => panic!("review status"),
        }
    }
    #[test]
    fn review_decisions_retain_evidence_actor_motion_protection_history_and_replay() {
        let f = fixture();
        let candidate = f.import(
            br#"{"actions":[{"at":0,"pos":20},{"at":1000,"pos":80}]}"#,
            0,
        );
        f.call(
            Command::CommitCandidate {
                candidate_id: candidate.candidate_id.clone(),
            },
            Some(0),
        )
        .unwrap();
        let initial = review_status(&f);
        let entry = initial
            .entries
            .iter()
            .find(|e| e.flag.reason.as_str() == "imported_motion")
            .unwrap();
        let snapshot = project_snapshot(&f.engine.db.lock().unwrap(), &f.project).unwrap();
        let request = f.request(
            Command::SetReviewDisposition {
                review_id: entry.review_id.clone(),
                disposition: ReviewDisposition::Resolved,
                note: "Fixture declaration only; interval reviewed".into(),
            },
            Some(1),
        );
        let resolved = f.engine.handle(request.clone()).result.unwrap();
        let r = review_status(&f);
        assert_eq!(r.entries.len(), initial.entries.len());
        let decision = r
            .entries
            .iter()
            .find(|e| e.review_id == entry.review_id)
            .unwrap()
            .decision
            .as_ref()
            .unwrap();
        assert_eq!(
            decision.actor.as_str(),
            actor(&f.engine.db.lock().unwrap(), &f.session).unwrap()
        );
        assert_eq!(decision.recorded_revision.value(), 2);
        assert!(!project_review_issues(&f)
            .iter()
            .any(|i| i.code == "imported_motion"));
        let changed = project_snapshot(&f.engine.db.lock().unwrap(), &f.project).unwrap();
        assert_eq!(
            encode(&snapshot.motion.program).unwrap(),
            encode(&changed.motion.program).unwrap()
        );
        let db = f.engine.db.lock().unwrap();
        let protected: String = db
            .query_row(
                "SELECT protected FROM projects WHERE id=?1",
                [f.project.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(protected, "[]");
        drop(db);
        assert_eq!(
            encode(&f.engine.handle(request.clone()).result.unwrap()).unwrap(),
            encode(&resolved).unwrap()
        );
        assert_eq!(review_status(&f).revision.value(), 2);
        f.call(Command::Undo, Some(2)).unwrap();
        assert!(project_review_issues(&f)
            .iter()
            .any(|i| i.code == "imported_motion"));
        f.call(Command::Redo, Some(3)).unwrap();
        assert!(!project_review_issues(&f)
            .iter()
            .any(|i| i.code == "imported_motion"));
        f.call(Command::SetProtectedRegions { regions: vec![] }, Some(4))
            .unwrap();
        assert_eq!(
            review_status(&f)
                .entries
                .iter()
                .filter(|e| e.decision.is_some())
                .count(),
            1
        );
        let f = reopen_fixture(f);
        assert!(!project_review_issues(&f)
            .iter()
            .any(|i| i.code == "imported_motion"));
        assert_eq!(
            encode(&f.engine.handle(request).result.unwrap()).unwrap(),
            encode(&resolved).unwrap()
        );
        f.call(
            Command::SetReviewDisposition {
                review_id: entry.review_id.clone(),
                disposition: ReviewDisposition::Reopened,
                note: "Fixture reopened after review".into(),
            },
            Some(5),
        )
        .unwrap();
        assert!(project_review_issues(&f)
            .iter()
            .any(|i| i.code == "imported_motion"));
        let db = f.engine.db.lock().unwrap();
        let old = lineage::state(&db, &f.project, RevisionId::new(2)).unwrap();
        assert_eq!(old.flags.len(), initial.entries.len());
        assert_eq!(old.decisions[0].disposition, ReviewDisposition::Resolved);
    }
    #[test]
    fn review_decisions_require_edit_scope_exact_revision_and_present_evidence() {
        let f = fixture();
        let candidate = f.import(
            br#"{"actions":[{"at":0,"pos":20},{"at":1000,"pos":80}]}"#,
            0,
        );
        f.call(
            Command::CommitCandidate {
                candidate_id: candidate.candidate_id,
            },
            Some(0),
        )
        .unwrap();
        let id = review_status(&f).entries[0].review_id.clone();
        let intent = Command::SetReviewDisposition {
            review_id: id.clone(),
            disposition: ReviewDisposition::Resolved,
            note: "Fixture only".into(),
        };
        assert_eq!(
            f.call(intent.clone(), Some(0)).unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            f.call(
                Command::SetReviewDisposition {
                    review_id: "f".repeat(64),
                    disposition: ReviewDisposition::Resolved,
                    note: "Absent evidence".into()
                },
                Some(1)
            )
            .unwrap_err()
            .code,
            ErrorCode::NotFound
        );
        {
            let db = f.engine.db.lock().unwrap();
            db.execute(
                "DELETE FROM grants WHERE session=?1 AND project=?2 AND scope=?3",
                params![
                    f.session.as_str(),
                    f.project.as_str(),
                    scope_name(Scope::Edit)
                ],
            )
            .unwrap();
        }
        assert_eq!(
            f.call(intent, Some(1)).unwrap_err().code,
            ErrorCode::Forbidden
        );
        assert_eq!(review_status(&f).revision.value(), 1);
        assert!(review_status(&f)
            .entries
            .iter()
            .all(|e| e.decision.is_none()));
    }
    #[test]
    fn regenerated_and_rebased_candidates_do_not_inherit_review_acceptance() {
        let f = fixture();
        let candidate = f.import(
            br#"{"actions":[{"at":0,"pos":20},{"at":1000,"pos":80}]}"#,
            0,
        );
        f.call(
            Command::CommitCandidate {
                candidate_id: candidate.candidate_id.clone(),
            },
            Some(0),
        )
        .unwrap();
        let id = review_status(&f).entries[0].review_id.clone();
        f.call(
            Command::SetReviewDisposition {
                review_id: id,
                disposition: ReviewDisposition::Resolved,
                note: "Fixture only".into(),
            },
            Some(1),
        )
        .unwrap();
        let rebase = f
            .call(
                Command::RebaseCandidate {
                    candidate_id: candidate.candidate_id,
                },
                Some(2),
            )
            .unwrap();
        let ResponseBody::Candidate(rebased) = rebase else {
            panic!("candidate")
        };
        f.call(
            Command::CommitCandidate {
                candidate_id: rebased.candidate_id,
            },
            Some(2),
        )
        .unwrap();
        assert!(review_status(&f)
            .entries
            .iter()
            .all(|e| e.decision.is_none()));
        assert!(project_review_issues(&f)
            .iter()
            .any(|i| i.code == "imported_motion"));
    }
    #[test]
    fn review_acceptance_does_not_fill_gaps_or_bypass_protection_and_alignment_changes_reopen() {
        let f = fixture();
        let range = TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(100)).unwrap();
        let program = MotionProgram::new(vec![MotionTrack::with_gaps(
            Axis::Stroke,
            vec![],
            vec![range],
        )
        .unwrap()])
        .unwrap();
        let proposal = commit_program(&f, 0, program);
        let Command::CommitCandidate { candidate_id } = &proposal else {
            panic!("candidate")
        };
        let flag = ReviewFlag {
            axis: Some(Axis::Stroke),
            range,
            reason: ReviewReason::new("missing_support_fixture").unwrap(),
            evidence: EvidenceKind::Unavailable,
            confidence: None,
        };
        f.engine
            .db
            .lock()
            .unwrap()
            .execute(
                "UPDATE candidates SET review=?2 WHERE id=?1",
                params![candidate_id.as_str(), encode(&vec![flag.clone()]).unwrap()],
            )
            .unwrap();
        f.call(proposal, Some(0)).unwrap();
        let protected = vec![ProtectedRegion {
            axis: Some(Axis::Stroke),
            range,
        }];
        f.call(
            Command::SetProtectedRegions {
                regions: protected.clone(),
            },
            Some(1),
        )
        .unwrap();
        f.call(
            Command::SetReviewDisposition {
                review_id: review_identity(&flag).unwrap(),
                disposition: ReviewDisposition::Resolved,
                note: "Fixture gap warning reviewed; evidence still unavailable".into(),
            },
            Some(2),
        )
        .unwrap();
        let db = f.engine.db.lock().unwrap();
        let state = project_state(&db, &f.project).unwrap();
        assert_eq!(state.program.track(Axis::Stroke).unwrap().gaps(), &[range]);
        let raw: String = db
            .query_row(
                "SELECT protected FROM projects WHERE id=?1",
                [f.project.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raw, encode(&protected).unwrap());
        drop(db);
        let output = f.dir.path().join("must-not-export.funscript");
        assert!(f
            .call(
                Command::ExportAxis {
                    path: output.clone(),
                    axis: Axis::Stroke
                },
                Some(3)
            )
            .is_err());
        assert!(!output.exists());
        assert!(project_review_issues(&f)
            .iter()
            .any(|i| i.code == "unresolved_gap"));
        let media = f.dir.path().join("nonexecuted-media.fixture");
        fs::write(&media, b"synthetic source; no decode performed").unwrap();
        let ResponseBody::Source { source_version } =
            f.call(Command::ImportSource { path: media }, None).unwrap()
        else {
            panic!("source")
        };
        let spec = SourcePlacementSpec {
            source_version,
            source_start: SourceTimestamp::new(0, 1).unwrap(),
            source_end: SourceTimestamp::new(1, 1).unwrap(),
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::ZERO,
            rate_numerator: 1,
            rate_denominator: 1,
        };
        f.call(
            Command::SetSourcePlacement {
                spec,
                replace: None,
            },
            Some(3),
        )
        .unwrap();
        let status = review_status(&f);
        assert!(status.entries.iter().all(|e| e.decision.is_none()));
        assert!(status
            .entries
            .iter()
            .any(|e| e.flag.reason.as_str() == "source_alignment_changed_motion_not_retimed"));
    }
    #[test]
    fn partial_merge_review_excludes_protected_and_omitted_axes() {
        let f = fixture();
        let action = |at, pos| {
            MotionAction::new(
                ProjectTime::from_nanos(at),
                NormalizedPosition::new(pos).unwrap(),
                EvidenceKind::Synthesized,
            )
            .unwrap()
        };
        let original = MotionProgram::new(vec![
            MotionTrack::new(
                Axis::Stroke,
                vec![
                    action(0, 0.1),
                    action(100, 0.2),
                    action(200, 0.3),
                    action(300, 0.4),
                ],
            )
            .unwrap(),
            MotionTrack::new(Axis::Sway, vec![action(0, 0.2), action(300, 0.2)]).unwrap(),
        ])
        .unwrap();
        f.call(commit_program(&f, 0, original), Some(0)).unwrap();
        f.call(
            Command::SetProtectedRegions {
                regions: vec![ProtectedRegion {
                    axis: Some(Axis::Stroke),
                    range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(100)).unwrap(),
                }],
            },
            Some(1),
        )
        .unwrap();
        let incoming = MotionProgram::new(vec![
            MotionTrack::new(
                Axis::Stroke,
                vec![
                    action(0, 0.9),
                    action(100, 0.9),
                    action(150, 0.8),
                    action(200, 0.9),
                    action(300, 0.9),
                ],
            )
            .unwrap(),
            MotionTrack::new(Axis::Sway, vec![action(0, 0.9), action(300, 0.9)]).unwrap(),
        ])
        .unwrap();
        let flag = |axis, end, code| ReviewFlag {
            axis,
            range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(end)).unwrap(),
            reason: ReviewReason::new(code).unwrap(),
            evidence: EvidenceKind::Synthesized,
            confidence: None,
        };
        let reviews = vec![
            flag(None, 301, "selected_motion"),
            flag(Some(Axis::Stroke), 100, "protected_unused"),
            flag(Some(Axis::Sway), 301, "omitted_axis"),
        ];
        let candidate = id!(CandidateId);
        {
            let db = f.engine.db.lock().unwrap();
            let reference = crate::authority::motion_state::save(&db, &incoming).unwrap();
            db.execute("INSERT INTO candidates(id,project,base_revision,program,job,lineage,review) VALUES(?1,?2,2,?3,NULL,'[]',?4)",
                params![candidate.as_str(),f.project.as_str(),reference,encode(&reviews).unwrap()]).unwrap();
        }
        f.call(
            Command::MergeCandidate {
                candidate_id: candidate,
                axes: vec![Axis::Stroke],
                range: None,
            },
            Some(2),
        )
        .unwrap();
        let issues = project_review_issues(&f);
        let contributed = issues
            .iter()
            .find(|issue| issue.code == "selected_motion")
            .unwrap();
        assert_eq!(contributed.axis, Some(Axis::Stroke));
        assert!(contributed.range.unwrap().start().as_nanos() > 100);
        assert!(contributed.message.contains("Conservative"));
        assert!(!issues
            .iter()
            .any(|issue| issue.code == "protected_unused" || issue.code == "omitted_axis"));
    }
    #[test]
    fn review_snapshot_overflow_rolls_back_revision_history_and_candidate_commit() {
        let f = fixture();
        let first = f.import(
            br#"{"actions":[{"at":0,"pos":20},{"at":1000,"pos":80}]}"#,
            0,
        );
        f.call(
            Command::CommitCandidate {
                candidate_id: first.candidate_id,
            },
            Some(0),
        )
        .unwrap();
        let incoming = f.import(
            br#"{"actions":[{"at":0,"pos":80},{"at":1000,"pos":20}]}"#,
            1,
        );
        let reviews = vec![incoming.review[0].clone(); 1024];
        f.engine
            .db
            .lock()
            .unwrap()
            .execute(
                "UPDATE candidates SET review=?2 WHERE id=?1",
                params![incoming.candidate_id.as_str(), encode(&reviews).unwrap()],
            )
            .unwrap();
        let request = f.request(
            Command::MergeCandidate {
                candidate_id: incoming.candidate_id.clone(),
                axes: vec![Axis::Stroke],
                range: None,
            },
            Some(1),
        );
        assert_eq!(
            f.engine.handle(request.clone()).result.unwrap_err().code,
            ErrorCode::ResourceExhausted
        );
        let db = f.engine.db.lock().unwrap();
        assert_eq!(
            project_snapshot(&db, &f.project).unwrap().revision.value(),
            1
        );
        let counts: (i64,i64,i64) = db.query_row(
            "SELECT (SELECT COUNT(*) FROM revisions WHERE project=?1),(SELECT COUNT(*) FROM edit_states WHERE project=?1),(SELECT COUNT(*) FROM revision_lineage WHERE project=?1)",
            [f.project.as_str()], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).unwrap();
        assert_eq!(counts, (2, 2, 2));
        let committed: Option<i64> = db
            .query_row(
                "SELECT committed_revision FROM candidates WHERE id=?1",
                [incoming.candidate_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(committed, None);
        let remembered: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM requests WHERE request=?1",
                [request.request_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remembered, 0);
    }
    #[test]
    fn legacy_history_is_explicitly_unknown_instead_of_inferred() {
        let f = fixture();
        let candidate = f.import(
            br#"{"actions":[{"at":0,"pos":20},{"at":1000,"pos":80}]}"#,
            0,
        );
        f.call(
            Command::CommitCandidate {
                candidate_id: candidate.candidate_id,
            },
            Some(0),
        )
        .unwrap();
        {
            let db = f.engine.db.lock().unwrap();
            db.execute("DELETE FROM history_lineage", []).unwrap();
            db.execute("DELETE FROM revision_lineage", []).unwrap();
        }
        let f = reopen_fixture(f);
        assert!(project_review_issues(&f)
            .iter()
            .any(|issue| issue.code == "review_history_unknown"));
        assert!(project_review_issues(&f)
            .iter()
            .any(|issue| issue.code == "imported_motion"));
        f.call(Command::Undo, Some(1)).unwrap();
        assert!(project_review_issues(&f)
            .iter()
            .any(|issue| issue.code == "review_history_unknown"));
        let db = f.engine.db.lock().unwrap();
        let edge: (Option<i64>,i64) = db.query_row(
            "SELECT restored_from_revision,restored_history_position FROM revision_lineage WHERE project=?1 AND revision=2",
            [f.project.as_str()], |row| Ok((row.get(0)?,row.get(1)?)),
        ).unwrap();
        assert_eq!(edge, (None, 0));
    }
    #[test]
    fn source_kind_prevents_funscript_preview_before_worker_admission() {
        let f = fixture();
        f.import(
            br#"{"actions":[{"at":0,"pos":20},{"at":1000,"pos":80}]}"#,
            0,
        );
        let ResponseBody::Project(project) = f.call(Command::GetSnapshot, None).unwrap() else {
            panic!()
        };
        assert_eq!(project.sources.len(), 1);
        assert_eq!(project.sources[0].kind, SourceKind::Funscript);
        let source = project.sources[0].source_version.clone();
        let result = f.call(
            Command::PreviewFrame {
                time_basis: PreviewTimeBasis::SourcePts,
                video_view: None,
                context: FrameContext {
                    source_version: source,
                    source_placement: SourcePlacementId::new("source").unwrap(),
                    frame: FrameId::new(0),
                    transform: TransformId::new("source-identity").unwrap(),
                    seek_generation: 0,
                    request_generation: 0,
                },
                source_time: SourceTimestamp::new(0, 1).unwrap(),
                model_path: None,
                model_input: None,
                inference_profile: None,
            },
            None,
        );
        assert_eq!(result.unwrap_err().code, ErrorCode::InvalidRequest);
        assert!(f.engine.workers.lock().unwrap().is_empty());
    }

    #[test]
    fn branching_history_retains_immutable_restoration_edges() {
        let f = fixture();
        let candidate = f.import(
            br#"{"actions":[{"at":0,"pos":20},{"at":1000,"pos":80}]}"#,
            0,
        );
        f.call(
            Command::CommitCandidate {
                candidate_id: candidate.candidate_id,
            },
            Some(0),
        )
        .unwrap();
        f.call(Command::SetProtectedRegions { regions: vec![] }, Some(1))
            .unwrap();
        f.call(Command::Undo, Some(2)).unwrap();
        let ResponseBody::Project(snapshot) = f.call(Command::GetSnapshot, None).unwrap() else {
            panic!()
        };
        assert_eq!(snapshot.revision.value(), 3);
        // Branch through the authored-edit API so existing review lineage is retained,
        // rather than inventing a new import with unrelated provenance.
        let program = f
            .engine
            .motion_store
            .read(&snapshot.motion.program)
            .unwrap();
        let tracks = program
            .tracks()
            .iter()
            .map(|track| {
                serde_json::json!({
                    "axis": track.axis(),
                    "actions": track.actions().iter().map(|action| serde_json::json!({
                        "time": action.time().as_nanos(),
                        "position": action.position().value(),
                    })).collect::<Vec<_>>(),
                    "gaps": track.gaps(),
                })
            })
            .collect::<Vec<_>>();
        let bytes = serde_json::to_vec(&serde_json::json!({"tracks": tracks})).unwrap();
        let sha256 = format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(&bytes));
        let ResponseBody::Transfer(lease) = f
            .call(
                Command::BeginEditUpload {
                    byte_len: bytes.len() as u64,
                    sha256,
                    label: "branch".into(),
                },
                Some(3),
            )
            .unwrap()
        else {
            panic!()
        };
        let handshake = BulkHandshake {
            version: PROTOCOL_VERSION,
            session: f.session.clone(),
            auth_token: f.token.clone(),
            lease_id: lease.lease_id.clone(),
            engine_epoch: lease.engine_epoch.clone(),
        };
        assert_eq!(
            f.engine.bulk_upload(&handshake, 0, &bytes).unwrap(),
            bytes.len() as u64
        );
        let ResponseBody::Candidate(branch) = f
            .call(
                Command::FinishEditUpload {
                    lease_id: lease.lease_id,
                },
                None,
            )
            .unwrap()
        else {
            panic!()
        };
        f.call(
            Command::CommitCandidate {
                candidate_id: branch.candidate_id,
            },
            Some(3),
        )
        .unwrap();
        let f = reopen_fixture(f);
        assert!(
            project_review_issues(&f)
                .iter()
                .any(|issue| issue.code == "imported_motion"
                    && issue.message.contains("Conservative"))
        );
        let db = f.engine.db.lock().unwrap();
        let revisions: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM revision_lineage WHERE project=?1",
                [f.project.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(revisions, 5);
        let restored: i64 = db.query_row("SELECT restored_from_revision FROM revision_lineage WHERE project=?1 AND revision=3",
            [f.project.as_str()],|row|row.get(0)).unwrap();
        assert_eq!(restored, 1);
        let branch: i64 = db
            .query_row(
                "SELECT revision FROM history_lineage WHERE project=?1 AND position=2",
                [f.project.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(branch, 4);
        let old: String = db
            .query_row(
                "SELECT operation FROM revision_lineage WHERE project=?1 AND revision=2",
                [f.project.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(old, "protection");
    }
    #[test]
    fn synthetic_source_kind_backfills_from_trusted_job_and_media_stays_media() {
        let f = fixture();
        let input = GenerationInput::Text {
            prompt: "stroke sine for one second".into(),
        };
        let source = f.engine.materialize_input(&input).unwrap();
        let path = f.dir.path().join("ordinary.fixture");
        fs::write(&path, b"ordinary imported media").unwrap();
        let ResponseBody::Source {
            source_version: media,
        } = f.call(Command::ImportSource { path }, None).unwrap()
        else {
            panic!()
        };
        let job = id!(JobId);
        let attempt = id!(AttemptId);
        let reference = ArtifactReference {
            identity: source.identity.clone(),
            path: source.path.clone(),
        };
        let manifest = WorkerRequest {
            cuda_execution: None,
            independent_audio: None,
            version: PROTOCOL_VERSION,
            job_id: job.clone(),
            attempt_id: attempt.clone(),
            project_id: f.project.clone(),
            base_revision: RevisionId::new(0),
            source: source.clone(),
            output_dir: f
                .engine
                .config
                .state_dir
                .join("attempts")
                .join(attempt.as_str()),
            operation: WorkerOperation::GenerateInput { input },
            budget: ResourceBudget {
                memory_bytes: 1024,
                output_bytes: 1024,
                wall_time_ms: 1000,
                cpu_threads: 1,
            },
            dependencies: vec![source.identity.clone()],
            tools: WorkerTools {
                ffmpeg: reference.clone(),
                ffprobe: reference,
                onnx_runtime: None,
            },
        };
        {
            let db = f.engine.db.lock().unwrap();
            db.execute("INSERT INTO sources(project,version,path,original,label,sha256,bytes,kind) VALUES(?1,?2,?3,'','misleading.mp4',?4,?5,'media')",
                params![f.project.as_str(),source.source_version.as_str(),source.path.to_string_lossy(),source.identity.sha256,source.identity.byte_len as i64]).unwrap();
            db.execute("INSERT INTO jobs(id,attempt,project,base_revision,session,source,state,manifest) VALUES(?1,?2,?3,0,?4,?5,'completed',?6)",
                params![job.as_str(),attempt.as_str(),f.project.as_str(),f.session.as_str(),source.source_version.as_str(),encode(&manifest).unwrap()]).unwrap();
        }
        let f = reopen_fixture(f);
        let db = f.engine.db.lock().unwrap();
        let project = project_snapshot(&db, &f.project).unwrap();
        assert_eq!(
            project
                .sources
                .iter()
                .find(|item| item.source_version == source.source_version)
                .unwrap()
                .kind,
            SourceKind::GenerationInput
        );
        assert_eq!(
            project
                .sources
                .iter()
                .find(|item| item.source_version == media)
                .unwrap()
                .kind,
            SourceKind::Media
        );
        assert_eq!(
            source_kinds::require_media(&db, &f.project, &source.source_version)
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn singleton_merge_review_covers_retained_anchor_interpolation() {
        let f = fixture();
        let original = f.import(br#"{"actions":[{"at":0,"pos":10},{"at":100,"pos":20},{"at":200,"pos":30},{"at":300,"pos":40}]}"#,0);
        f.call(
            Command::CommitCandidate {
                candidate_id: original.candidate_id,
            },
            Some(0),
        )
        .unwrap();
        let incoming = MotionProgram::new(vec![MotionTrack::new(
            Axis::Stroke,
            vec![MotionAction::new(
                ProjectTime::from_nanos(150_000_000),
                NormalizedPosition::new(0.8).unwrap(),
                EvidenceKind::Synthesized,
            )
            .unwrap()],
        )
        .unwrap()])
        .unwrap();
        let range = TimeRange::new(
            ProjectTime::from_nanos(100_000_000),
            ProjectTime::from_nanos(200_000_000),
        )
        .unwrap();
        let review = vec![ReviewFlag {
            axis: None,
            range,
            reason: ReviewReason::new("singleton_motion").unwrap(),
            evidence: EvidenceKind::Synthesized,
            confidence: None,
        }];
        let candidate = id!(CandidateId);
        {
            let db = f.engine.db.lock().unwrap();
            let reference = crate::authority::motion_state::save(&db, &incoming).unwrap();
            db.execute("INSERT INTO candidates(id,project,base_revision,program,job,lineage,review) VALUES(?1,?2,1,?3,NULL,'[]',?4)",
                params![candidate.as_str(),f.project.as_str(),reference,encode(&review).unwrap()]).unwrap();
        }
        f.call(
            Command::MergeCandidate {
                candidate_id: candidate,
                axes: vec![Axis::Stroke],
                range: Some(range),
            },
            Some(1),
        )
        .unwrap();
        let f = reopen_fixture(f);
        let issues = project_review_issues(&f);
        let issue = issues
            .iter()
            .find(|issue| issue.code == "singleton_motion")
            .unwrap();
        assert_eq!(issue.axis, Some(Axis::Stroke));
        assert!(issue.message.contains("Conservative"));
        assert_eq!(
            issue.range.unwrap(),
            TimeRange::new(
                ProjectTime::from_nanos(100_000_001),
                ProjectTime::from_nanos(200_000_000)
            )
            .unwrap()
        );
        assert!(issue
            .range
            .unwrap()
            .contains(ProjectTime::from_nanos(125_000_000)));
        let prior = issues
            .iter()
            .find(|issue| issue.code == "imported_motion")
            .unwrap();
        assert_eq!(prior.range.unwrap().start(), ProjectTime::ZERO);
        assert_eq!(
            prior.range.unwrap().end(),
            ProjectTime::from_nanos(300_000_001)
        );
    }
}
