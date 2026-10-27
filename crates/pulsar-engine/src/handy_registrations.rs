//! Inert owner configuration, atomically committed with ordinary request receipts.
//! These rows never restore sessions, qualification, loaded scripts or playback.
use super::*;

const ROW_LIMIT: usize = 4096;

pub(super) fn is_command(command: &Command) -> bool {
    matches!(
        command,
        Command::ResolveHandyRegistration { .. }
            | Command::SetHandyRegistration { .. }
            | Command::DisableHandyRegistration { .. }
            | Command::ListHandyRegistrations { .. }
            | Command::InspectHandyRegistration { .. }
    )
}

fn layout(db: &Connection) -> Result<bool> {
    let mut found = 0;
    for (table, columns) in [
        (
            "handy_registration_schema",
            vec![("singleton", 1), ("version", 0)],
        ),
        (
            "handy_registrations",
            vec![
                ("device", 1),
                ("incarnation", 0),
                ("revision", 0),
                ("settings", 0),
                ("disabled", 0),
            ],
        ),
    ] {
        let kind: Option<String> = db
            .query_row(
                "SELECT type FROM sqlite_master WHERE name=?1",
                [table],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(kind) = kind {
            anyhow::ensure!(kind == "table", "invalid Handy registration object");
            found += 1;
            let mut query = db.prepare(&format!("PRAGMA table_info({table})"))?;
            let actual = query
                .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(5)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            anyhow::ensure!(
                actual
                    == columns
                        .into_iter()
                        .map(|(name, key)| (name.to_string(), key))
                        .collect::<Vec<_>>(),
                "invalid Handy registration layout"
            );
        }
    }
    anyhow::ensure!(
        found == 0 || found == 2,
        "incomplete Handy registration layout"
    );
    if found == 0 {
        return Ok(false);
    }
    let versions: Vec<(i64, i64)> = {
        let mut query =
            db.prepare("SELECT singleton,version FROM handy_registration_schema LIMIT 2")?;
        let values = query
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        values
    };
    anyhow::ensure!(
        versions == [(1, 1)],
        "Handy registration metadata requires explicit migration"
    );
    let triggers: u64 = db.query_row("SELECT count(*) FROM sqlite_master WHERE type='trigger' AND tbl_name IN ('handy_registration_schema','handy_registrations')", [], |r| r.get(0))?;
    anyhow::ensure!(triggers == 0, "unexpected Handy registration trigger");
    Ok(true)
}

fn initialize(tx: &Transaction<'_>) -> PResult<()> {
    if !layout(tx).map_err(internal)? {
        tx.execute_batch("CREATE TABLE handy_registration_schema(singleton INTEGER PRIMARY KEY CHECK(singleton=1),version INTEGER NOT NULL CHECK(version=1));
            INSERT INTO handy_registration_schema VALUES(1,1);
            CREATE TABLE handy_registrations(device TEXT PRIMARY KEY NOT NULL,incarnation TEXT NOT NULL,revision INTEGER NOT NULL CHECK(revision>0),settings TEXT NOT NULL,disabled INTEGER NOT NULL CHECK(disabled IN (0,1)));").map_err(internal)?;
    }
    Ok(())
}

struct Stored {
    incarnation: String,
    registration: HandyRegistration,
}
fn stored(
    device: String,
    incarnation: String,
    revision: i64,
    settings: String,
    disabled: i64,
) -> Result<Stored> {
    anyhow::ensure!(
        incarnation.len() == 36 && Uuid::parse_str(&incarnation).is_ok(),
        "invalid registration incarnation"
    );
    anyhow::ensure!(
        revision > 0 && matches!(disabled, 0 | 1),
        "invalid registration state"
    );
    let device_id = DeviceId::new(device)?;
    let settings: HandyRegistrationSettings = serde_json::from_str(&settings)?;
    settings.validate()?;
    let disabled = disabled == 1;
    let binding_sha256 = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(
            1u8,
            &device_id,
            &incarnation,
            revision,
            &settings,
            disabled
        ))?)
    );
    let registration = HandyRegistration {
        device_id,
        revision: revision as u64,
        settings,
        disabled,
        binding_sha256,
    };
    registration.validate()?;
    Ok(Stored {
        incarnation,
        registration,
    })
}

fn records(db: &Connection) -> Result<Vec<Stored>> {
    if !layout(db)? {
        return Ok(vec![]);
    }
    let mut query=db.prepare("SELECT CASE WHEN length(CAST(device AS BLOB))<=128 THEN device ELSE NULL END,CASE WHEN length(CAST(incarnation AS BLOB))=36 THEN incarnation ELSE NULL END,revision,CASE WHEN length(CAST(settings AS BLOB))<=4096 THEN settings ELSE NULL END,disabled FROM handy_registrations ORDER BY device LIMIT 65")?;
    let rows = query
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    anyhow::ensure!(
        rows.len() <= HANDY_REGISTRATION_LIMIT,
        "Handy registration count exceeds limit"
    );
    rows.into_iter()
        .map(|(device, incarnation, revision, settings, disabled)| {
            stored(device, incarnation, revision, settings, disabled)
        })
        .collect()
}
/// Read-only validation before Engine initialization or recovery mutations.
pub(super) fn validate(db: &Connection) -> Result<()> {
    records(db).map(|_| ())
}

pub(super) fn current(
    db: &Connection,
    device: &DeviceId,
    expected: u64,
) -> PResult<HandyRegistration> {
    let record = records(db)
        .map_err(internal)?
        .into_iter()
        .find(|r| r.registration.device_id == *device)
        .ok_or_else(|| ProtocolError::new(ErrorCode::NotFound, "Handy registration not found"))?
        .registration;
    if record.revision != expected {
        return Err(ProtocolError::new(
            ErrorCode::RevisionConflict,
            "Handy registration revision changed",
        ));
    }
    Ok(record)
}

impl Engine {
    pub(super) fn inspect_handy_registration_with<F>(
        &self,
        request: &Request,
        session: &SessionId,
        inspect: F,
    ) -> PResult<ResponseBody>
    where
        F: FnOnce(&HandyInspectionRequest) -> PResult<HandyInspection>,
    {
        if let Command::InspectHandyRegistration {
            device_id,
            expected_registration_revision,
            binding_sha256,
            allow_online_inspection,
            ..
        } = &request.command
        {
            let before = {
                let db = self.db.lock().map_err(internal)?;
                self.authorize(&db, request, session)?;
                let registration = current(&db, device_id, *expected_registration_revision)?;
                if registration.disabled || registration.binding_sha256 != *binding_sha256 {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "Handy registration is disabled or its binding changed",
                    ));
                }
                registration
            };
            let _reservation = self.pool.reserve_memory(1024 * 1024)?;
            let inspection = inspect(&HandyInspectionRequest {
                api_key_env: before.settings.api_key_env.clone(),
                connection_key_env: before.settings.connection_key_env.clone(),
                allow_online_inspection: *allow_online_inspection,
            })?;
            inspection.validate()?;
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, session)?;
            if current(&db, device_id, *expected_registration_revision)? != before {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "Handy registration changed during inspection",
                ));
            }
            return Ok(ResponseBody::HandyRegisteredInspection(Box::new(
                HandyRegisteredInspection {
                    registration: before,
                    inspection,
                },
            )));
        }
        Err(ProtocolError::unsupported("registered Handy inspection"))
    }

    pub(super) fn handy_registration_control(
        &self,
        request: &Request,
        session: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        if matches!(request.command, Command::InspectHandyRegistration { .. }) {
            return self.inspect_handy_registration_with(
                request,
                session,
                crate::handy_online::inspect,
            );
        }
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, session)?;
        check_request_identity(&db, session.as_str(), request, fingerprint)?;
        if let Command::ListHandyRegistrations { .. } = &request.command {
            let catalog = HandyRegistrationCatalog {
                registrations: records(&db)
                    .map_err(internal)?
                    .into_iter()
                    .map(|r| r.registration)
                    .collect(),
            };
            catalog.validate()?;
            return Ok(ResponseBody::HandyRegistrations(Box::new(catalog)));
        }
        if matches!(request.command, Command::ResolveHandyRegistration { .. }) {
            return resolve(&mut db, request, session, fingerprint);
        }
        if let Some(body) = replay(&db, session.as_str(), request, fingerprint)? {
            if let ResponseBody::HandyRegistrationResolution(resolution) = &body {
                resolution.validate()?;
                if resolution.request_id == request.request_id
                    && Some(&resolution.intent)
                        == request.command.handy_registration_intent().as_ref()
                    && matches!(resolution.outcome, HandyRegistrationOutcome::Withdrawn)
                {
                    return Err(ProtocolError::new(
                        ErrorCode::Cancelled,
                        "original Handy registration request was withdrawn",
                    ));
                }
                return Err(ProtocolError::invalid(
                    "registration withdrawal correspondence mismatch",
                ));
            }

            let ResponseBody::HandyRegistrationSaved(receipt) = &body else {
                return Err(ProtocolError::invalid(
                    "registration replay response mismatch",
                ));
            };
            receipt.validate()?;
            let expected = match &request.command {
                Command::SetHandyRegistration {
                    device_id,
                    expected_registration_revision,
                    settings,
                    disabled,
                    ..
                } => {
                    &receipt.registration.device_id == device_id
                        && receipt.registration.revision == expected_registration_revision + 1
                        && receipt.registration.settings == *settings
                        && receipt.registration.disabled == *disabled
                }
                Command::DisableHandyRegistration {
                    device_id,
                    expected_registration_revision,
                    ..
                } => {
                    &receipt.registration.device_id == device_id
                        && receipt.registration.revision == expected_registration_revision + 1
                        && receipt.registration.disabled
                }
                _ => false,
            };
            if !expected || receipt.request_id != request.request_id {
                return Err(ProtocolError::invalid(
                    "registration replay correspondence mismatch",
                ));
            }
            return Ok(body);
        }
        let tx = db.transaction().map_err(internal)?;
        let all = records(&tx).map_err(internal)?;
        let (device, expected) = match &request.command {
            Command::SetHandyRegistration {
                device_id,
                expected_registration_revision,
                ..
            }
            | Command::DisableHandyRegistration {
                device_id,
                expected_registration_revision,
                ..
            } => (device_id, *expected_registration_revision),
            _ => return Err(ProtocolError::unsupported("Handy registration command")),
        };
        let prior = all.iter().find(|r| r.registration.device_id == *device);
        if prior.map_or(0, |r| r.registration.revision) != expected {
            return Err(ProtocolError::new(
                ErrorCode::RevisionConflict,
                "Handy registration revision changed",
            ));
        }
        let (settings, disabled) = match &request.command {
            Command::SetHandyRegistration {
                settings, disabled, ..
            } => (settings.clone(), *disabled),
            Command::DisableHandyRegistration { .. } => (
                prior
                    .ok_or_else(|| {
                        ProtocolError::new(ErrorCode::NotFound, "Handy registration not found")
                    })?
                    .registration
                    .settings
                    .clone(),
                true,
            ),
            _ => unreachable!(),
        };
        settings.validate()?;
        if prior.is_none() && all.len() >= HANDY_REGISTRATION_LIMIT {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "Handy registration limit reached",
            ));
        }
        let revision = expected
            .checked_add(1)
            .filter(|n| *n <= i64::MAX as u64)
            .ok_or_else(|| ProtocolError::invalid("Handy registration revision overflow"))?;
        let incarnation = prior
            .map(|r| r.incarnation.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let encoded = serde_json::to_string(&settings).map_err(internal)?;
        if encoded.len() > ROW_LIMIT {
            return Err(ProtocolError::invalid(
                "Handy registration settings exceed limit",
            ));
        }
        initialize(&tx)?;
        tx.execute("INSERT INTO handy_registrations(device,incarnation,revision,settings,disabled) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(device) DO UPDATE SET revision=excluded.revision,settings=excluded.settings,disabled=excluded.disabled",params![device.as_str(),incarnation,revision as i64,encoded,i64::from(disabled)]).map_err(internal)?;
        let registration = current(&tx, device, revision)?;
        let body = ResponseBody::HandyRegistrationSaved(Box::new(HandyRegistrationReceipt {
            request_id: request.request_id.clone(),
            registration,
        }));
        remember(&tx, session.as_str(), request, fingerprint, &body)?;
        // The DB -> registry lock order also covers open completion. A successful
        // configuration commit retires its observation before either lock releases.
        let mut connections = self.handy_sessions.lock().map_err(internal)?;
        tx.commit().map_err(internal)?;
        connections.invalidate(device);
        self.retire_handy_preparations(handy_playback_preparation::Retirement::Device(device));
        self.retire_changed_handy_browser_start_qualifications(&db);
        self.retire_changed_handy_hsp_source_qualifications(&db);
        Ok(body)
    }
}

fn resolve(
    db: &mut Connection,
    request: &Request,
    session: &SessionId,
    fingerprint: &str,
) -> PResult<ResponseBody> {
    let Command::ResolveHandyRegistration {
        original_request_id,
        intent,
        owner_proof,
    } = &request.command
    else {
        return Err(ProtocolError::unsupported("registration resolution"));
    };
    let tx = db.transaction().map_err(internal)?;
    if let Some(body) = replay(&tx, session.as_str(), request, fingerprint)? {
        let ResponseBody::HandyRegistrationResolution(resolution) = &body else {
            return Err(ProtocolError::invalid(
                "resolution replay response mismatch",
            ));
        };
        resolution.validate()?;
        if resolution.request_id != *original_request_id || resolution.intent != *intent {
            return Err(ProtocolError::invalid("resolution replay binding mismatch"));
        }
        return Ok(body);
    }
    let mut original = request.clone();
    original.request_id = original_request_id.clone();
    original.command = intent.command(owner_proof.clone());
    original.validate()?;
    let original_fingerprint = format!(
        "{:x}",
        Sha256::digest(request_fingerprint(&original).map_err(internal)?)
    );
    check_request_identity(&tx, session.as_str(), &original, &original_fingerprint)?;
    let outcome = match replay(&tx, session.as_str(), &original, &original_fingerprint)? {
        Some(ResponseBody::HandyRegistrationSaved(receipt)) => {
            receipt.validate()?;
            if receipt.request_id != *original_request_id || !intent.matches(&receipt) {
                return Err(ProtocolError::invalid(
                    "original registration receipt mismatch",
                ));
            }
            HandyRegistrationOutcome::Applied(receipt)
        }
        Some(ResponseBody::HandyRegistrationResolution(resolution)) => {
            resolution.validate()?;
            if resolution.request_id != *original_request_id
                || resolution.intent != *intent
                || !matches!(resolution.outcome, HandyRegistrationOutcome::Withdrawn)
            {
                return Err(ProtocolError::invalid(
                    "original withdrawal receipt mismatch",
                ));
            }
            HandyRegistrationOutcome::Withdrawn
        }
        Some(_) => {
            return Err(ProtocolError::invalid(
                "original registration result mismatch",
            ))
        }
        None => {
            let tombstone =
                ResponseBody::HandyRegistrationResolution(Box::new(HandyRegistrationResolution {
                    request_id: original_request_id.clone(),
                    intent: intent.clone(),
                    outcome: HandyRegistrationOutcome::Withdrawn,
                }));
            remember(
                &tx,
                session.as_str(),
                &original,
                &original_fingerprint,
                &tombstone,
            )?;
            HandyRegistrationOutcome::Withdrawn
        }
    };
    let body = ResponseBody::HandyRegistrationResolution(Box::new(HandyRegistrationResolution {
        request_id: original_request_id.clone(),
        intent: intent.clone(),
        outcome,
    }));
    remember(&tx, session.as_str(), request, fingerprint, &body)?;
    tx.commit().map_err(internal)?;
    Ok(body)
}
