// Included only in the private native SOFTWARE SIMULATION module. The CPU
// attempts are real; the eligibility records remain authored simulation shapes.
fn archive_native_fallback(
    engine: &Arc<Engine>,
    actor: &SessionId,
    project: &ProjectId,
) -> Result<Value> {
    call(
        engine,
        actor,
        Some(project),
        None,
        Command::AllowProjectPackaging {
            owner_proof: engine.package_owner_token.clone(),
        },
    )?;
    let revision = snapshot(engine, actor, project)?.revision.value();
    let ResponseBody::PackageExport(started) = call(
        engine,
        actor,
        Some(project),
        Some(revision),
        Command::StartProjectExport,
    )?
    else {
        anyhow::bail!("native archival export response absent");
    };
    let end = Instant::now() + Duration::from_secs(60);
    let ready = loop {
        let ResponseBody::PackageExport(status) = call(
            engine,
            actor,
            Some(project),
            None,
            Command::ProjectPackageStatus {
                operation_id: started.operation_id.clone(),
            },
        )?
        else {
            anyhow::bail!("native archival status absent");
        };
        if status.state == PackageExportOperationState::Ready {
            break status;
        }
        ensure!(
            !matches!(
                status.state,
                PackageExportOperationState::Failed | PackageExportOperationState::Cancelled
            ) && Instant::now() < end,
            "native archival export failed or timed out: {status:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let artifact = ready
        .artifact
        .as_ref()
        .context("native archive artifact absent")?;
    ensure!(
        artifact.byte_len <= 512 * 1024 * 1024,
        "native archive exceeds test bound"
    );
    let bytes = fs::read(
        engine
            .config
            .state_dir
            .join("project-packages")
            .join(&artifact.sha256),
    )?;
    ensure!(
        bytes.len() as u64 == artifact.byte_len && fixture_hash(&bytes) == artifact.sha256,
        "native archive bytes differ"
    );
    ensure!(bytes.len() >= 52, "native archive framing absent");
    let length = u64::from_le_bytes(bytes[12..20].try_into()?) as usize;
    let manifest: PortableProjectManifest = serde_json::from_slice(
        bytes
            .get(
                52..52usize
                    .checked_add(length)
                    .context("manifest length overflow")?,
            )
            .context("native archive manifest exceeds container")?,
    )?;
    ensure!(
        manifest.format_version == 16 && manifest.generated_origins.len() == 2,
        "native archive omitted original attempts"
    );
    let payload = manifest.generated_origins[0]
        .fallback_history
        .as_ref()
        .context("native fallback payload absent")?;
    ensure!(
        manifest
            .generated_origins
            .iter()
            .all(|origin| origin.cpu_runtime.is_some()
                && origin.fallback_history.as_ref() == Some(payload)),
        "native archive lost CPU image or shared fallback binding"
    );
    // This consumer validates root/ancestor mutations and the public upload,
    // clone, restart and second-clone paths, including absence of restored jobs,
    // attempts, CPU execution records and equivalence acceptance authority.
    project_package_imports::exercise_fallback_portable_lifecycle(&bytes, payload);
    Ok(
        json!({"passed":true,"artifact":artifact,"format_version":16,
        "original_states":manifest.generated_origins.iter().map(|o| &o.state).collect::<Vec<_>>(),
        "cpu_images_retained":2,"fallback_payload":payload,"two_clones_restarted_reexported":true,
        "no_execution_or_acceptance_authority_restored":true,
        "exported_eligibility_is_simulated":true,"qualified":false}),
    )
}

#[test]
#[ignore = "requires pinned native CPU worker, private closure policy, simulator and owned host supervisor; SOFTWARE SIMULATION only"]
fn native_cpu_fallback_portable_history_roundtrip() {
    let output = environment_path("PULSAR_FALLBACK_NATIVE_OUTPUT").expect("explicit native output");
    let mut receipt = json!({"schema_version":1,"passed":false,"simulated_eligibility":true,
        "primary_provider_absence_simulated":true,"real_measured_equivalence":false,
        "qualified":false,"physical_device":false,"public_acceptance_called":false,
        "accepted_fixture_records_exported":true,"cases":[],"error":null,
        "scope":"SOFTWARE SIMULATION eligibility; real confined CPU execution and inert portable histories only. No scientific equivalence, human approval or physical qualification."});
    let result = (|| -> Result<()> {
        ensure!(
            file_identity(Path::new(MEDIA))?.sha256 == MEDIA_SHA
                && file_identity(Path::new(MODEL))?.sha256 == MODEL_SHA,
            "neutral inputs changed"
        );
        for mode in ["success", "active_cancel", "active_withdraw"] {
            receipt["cases"]
                .as_array_mut()
                .unwrap()
                .push(campaign_case(mode, true)?);
        }
        Ok(())
    })();
    match &result {
        Ok(()) => receipt["passed"] = json!(true),
        Err(error) => receipt["error"] = json!(format!("{error:#}")),
    }
    save(&output.join("receipt.json"), &receipt).expect("save native archival receipt");
    result.expect("native CPU fallback archival campaign failed");
}
