// Existing-directory channel intents. Included by library_publication.rs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialPublicationChannel {
    pub axis: Axis,
    pub request_id: RequestId,
    /// Retained positive Engine acknowledgement, never inferred from file presence.
    pub acknowledged: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialPublication {
    pub directory_identity: PublicationDirectoryIdentity,
    pub channels: Vec<PartialPublicationChannel>,
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn validate_partial_publication(item: &PublicationItem) -> Result<()> {
    let input = &item.generation;
    ensure!(
        input.publication_directory.is_some() && item.prepared.is_none() && item.receipt.is_none(),
        "Partial publication must retain its existing directory without aggregate receipt"
    );
    match &item.partial {
        None => ensure!(
            matches!(
                item.phase,
                PublicationPhase::AwaitingGeneration | PublicationPhase::Pending
            ),
            "Partial publication phase lacks channel intents"
        ),
        Some(partial) => {
            ensure!(
                matches!(
                    item.phase,
                    PublicationPhase::Prepared | PublicationPhase::Published
                ) && Some(partial.directory_identity) == input.publication_directory
                    && partial.channels.iter().map(|c| c.axis).collect::<Vec<_>>()
                        == input.publication_axes()
                    && partial
                        .channels
                        .iter()
                        .map(|c| c.request_id.clone())
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        == partial.channels.len()
                    && partial.channels.iter().all(|c| c
                        .error
                        .as_ref()
                        .is_none_or(|s| s.len() <= 4096)
                        && (!c.acknowledged || c.error.is_none()))
                    && ((item.phase == PublicationPhase::Published)
                        == partial.channels.iter().all(|c| c.acknowledged)),
                "Partial publication identity/acknowledgement shape differs"
            );
        }
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn publish_partial_item(
    client: &mut SessionClient,
    store: &mut local::Store,
    index: usize,
    prepare_only: bool,
) -> Result<()> {
    if store.journal.report.items[index].phase == PublicationPhase::Pending {
        let item = &mut store.journal.report.items[index];
        let channels = item
            .generation
            .publication_axes()
            .iter()
            .map(|axis| -> Result<_> {
                Ok(PartialPublicationChannel {
                    axis: *axis,
                    request_id: RequestId::new(uuid::Uuid::new_v4().to_string())?,
                    acknowledged: false,
                    error: None,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        item.partial = Some(PartialPublication {
            directory_identity: item.generation.publication_directory.expect("validated"),
            channels,
        });
        item.phase = PublicationPhase::Prepared;
        store.save()?; // Every exact export identity is durable before the first dispatch.
    }
    if prepare_only {
        return Ok(());
    }
    let item = &store.journal.report.items[index];
    let partial = item
        .partial
        .as_ref()
        .context("Partial channel intents absent")?;
    let Some(channel_index) = partial.channels.iter().position(|c| !c.acknowledged) else {
        return Ok(());
    };
    let channel = &partial.channels[channel_index];
    let axis = channel.axis;
    let path = item
        .generation
        .destination
        .join(format!("{}.funscript", axis.as_str()));
    let revision = item.generation.revision;
    let result = client.execute_identified(channel.request_id.clone(), Command::ExportAxisPinned {
        path:path.clone(), axis, directory_identity:partial.directory_identity,
    }, item.generation.project_id.clone(), Some(revision)).and_then(|body| {
        ensure!(matches!(body,ResponseBody::AxisExported { path: actual, axis: actual_axis, revision: actual_revision } if actual==path && actual_axis==axis && actual_revision==revision), "Partial axis acknowledgement differs");
        Ok(())
    });
    let item = &mut store.journal.report.items[index];
    let partial = item.partial.as_mut().expect("retained");
    match result {
        Ok(()) => {
            partial.channels[channel_index].acknowledged = true;
            partial.channels[channel_index].error = None;
            item.error = None;
            if partial.channels.iter().all(|c| c.acknowledged) {
                item.phase = PublicationPhase::Published;
            }
        }
        Err(error) => {
            let error = bounded_error(error);
            partial.channels[channel_index].error = Some(error.clone());
            item.error = Some(error);
        }
    }
    store.journal.report.publication_completed = store
        .journal
        .report
        .items
        .iter()
        .all(|item| item.phase == PublicationPhase::Published);
    store.save()?;
    Ok(())
}
