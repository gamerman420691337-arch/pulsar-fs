//! Pulsar process composition. Product logic belongs to the four workspace libraries.
//! Historical attribution and the pre-engine source are retained during migration.

use anyhow::{bail, Context, Result};
use pulsar_engine::EngineConfig;
use std::ffi::OsString;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn main() {
    if let Err(error) = run() {
        eprintln!("pulsar: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<OsString> = std::env::args_os().collect();
    if pulsar_engine::worker_entry::run_worker_role(&args[1..])? {
        return Ok(());
    }
    match args.get(1).and_then(|arg| arg.to_str()) {
        Some("training-input-plan") => {
            return pulsar_engine::worker::run_training_view_plan(&args[2..]);
        }
        Some("engine") | Some("__pulsar-engine") => {
            if args.len() != 2 {
                bail!("engine role accepts no user commands; use PULSAR_STATE_DIR for an isolated state directory");
            }
            return pulsar_engine::run_server(EngineConfig::for_user()?);
        }
        _ => {}
    }
    if pulsar_clients::run_informational_command(&args)? {
        return Ok(());
    }
    if let Some(
        command @ (pulsar_clients::cli::Commands::RuntimeModels { .. }
        | pulsar_clients::cli::Commands::BundledModels),
    ) = pulsar_clients::runtime_install_command(&args)
    {
        #[cfg(target_os = "linux")]
        {
            let (cache_root, manifest_sha256) = match command {
                pulsar_clients::cli::Commands::RuntimeModels {
                    cache_root,
                    manifest_sha256,
                } => (cache_root, manifest_sha256),
                _ => {
                    let executable = std::env::current_exe()?;
                    let directory = executable
                        .parent()
                        .context("installed runtime directory missing")?;
                    let hash = directory
                        .file_name()
                        .and_then(|n| n.to_str())
                        .context("installed runtime identity missing")?
                        .to_owned();
                    let cache = directory
                        .parent()
                        .context("runtime cache missing")?
                        .to_owned();
                    (cache, hash)
                }
            };
            let report =
                pulsar_engine::inspect_cached_generation_packs(&cache_root, &manifest_sha256)?;
            println!("{}", report.json()?);
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = command;
            bail!("bundled model discovery requires Linux storage support");
        }
    }
    if let Some(pulsar_clients::cli::Commands::RuntimeLaunch {
        source,
        cache_root,
        arguments,
    }) = pulsar_clients::runtime_install_command(&args)
    {
        #[cfg(target_os = "linux")]
        {
            return launch_prepared_runtime(arguments, move || {
                Ok(pulsar_engine::prepare_runtime_launch(&source, &cache_root)?.directory)
            });
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (source, cache_root, arguments);
            bail!("persistent runtime launch requires Linux storage support");
        }
    }
    if let Some(pulsar_clients::cli::Commands::RuntimeCacheLaunch {
        cache_root,
        manifest_sha256,
        arguments,
    }) = pulsar_clients::runtime_install_command(&args)
    {
        #[cfg(target_os = "linux")]
        {
            return launch_prepared_runtime(arguments, move || {
                Ok(pulsar_engine::prepare_cached_runtime(&cache_root, &manifest_sha256)?.directory)
            });
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (cache_root, manifest_sha256, arguments);
            bail!("cached runtime launch requires Linux storage support");
        }
    }
    if let Some(pulsar_clients::cli::Commands::RuntimeCacheInspect {
        cache_root,
        manifest_sha256,
    }) = pulsar_clients::runtime_install_command(&args)
    {
        #[cfg(target_os = "linux")]
        {
            let prepared = pulsar_engine::prepare_cached_runtime(&cache_root, &manifest_sha256)?;
            println!("Verified retained runtime: {} (manifest SHA-256 {}). No runtime launched or Engine changed.", prepared.directory.display(), prepared.manifest_sha256);
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (cache_root, manifest_sha256);
            bail!("cached runtime inspection requires Linux storage support");
        }
    }
    if let Some(pulsar_clients::cli::Commands::RuntimeUpdatePack {
        source,
        base,
        destination,
        expected_base_manifest_sha256,
    }) = pulsar_clients::runtime_install_command(&args)
    {
        #[cfg(target_os = "linux")]
        {
            println!(
                "{}",
                pulsar_engine::pack_runtime_update(
                    &source,
                    &base,
                    &destination,
                    &expected_base_manifest_sha256
                )?
                .json()?
            );
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (source, base, destination, expected_base_manifest_sha256);
            bail!("runtime update archive creation requires Linux storage support");
        }
    }
    if let Some(pulsar_clients::cli::Commands::RuntimePack {
        source,
        destination,
    }) = pulsar_clients::runtime_install_command(&args)
    {
        #[cfg(target_os = "linux")]
        {
            let manifest_sha256 = pulsar_engine::pack_runtime_candidate(&source, &destination)?;
            println!(
                "Created offline runtime archive: {} (manifest SHA-256 {}).",
                destination.display(),
                manifest_sha256
            );
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (source, destination);
            bail!("runtime archive creation requires Linux storage support");
        }
    }
    if let Some(pulsar_clients::cli::Commands::RuntimeArchiveStageInspect { stage }) =
        pulsar_clients::runtime_install_command(&args)
    {
        #[cfg(target_os = "linux")]
        {
            println!(
                "{}",
                pulsar_engine::inspect_runtime_archive_stage(&stage)?.json()?
            );
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = stage;
            bail!("archive stage inspection requires Linux storage support");
        }
    }
    if let Some(pulsar_clients::cli::Commands::RuntimeArchiveStageDiscard {
        stage,
        expected_stage_sha256,
        approve_discard,
    }) = pulsar_clients::runtime_install_command(&args)
    {
        #[cfg(target_os = "linux")]
        {
            pulsar_engine::discard_runtime_archive_stage(
                &stage,
                &expected_stage_sha256,
                approve_discard,
            )?;
            println!("Discarded recognized archive stage: {}", stage.display());
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (stage, expected_stage_sha256, approve_discard);
            bail!("archive stage discard requires Linux storage support");
        }
    }
    if let Some(pulsar_clients::cli::Commands::RuntimeStageInspect { stage }) =
        pulsar_clients::runtime_install_command(&args)
    {
        #[cfg(target_os = "linux")]
        {
            println!("{}", pulsar_engine::inspect_runtime_stage(&stage)?.json()?);
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = stage;
            bail!("runtime stage inspection requires Linux storage support");
        }
    }
    if let Some(pulsar_clients::cli::Commands::RuntimeStageDiscard {
        stage,
        expected_manifest_sha256,
        approve_discard,
    }) = pulsar_clients::runtime_install_command(&args)
    {
        #[cfg(target_os = "linux")]
        {
            pulsar_engine::discard_runtime_stage(
                &stage,
                &expected_manifest_sha256,
                approve_discard,
            )?;
            println!(
                "Discarded recognized runtime installation stage: {}",
                stage.display()
            );
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (stage, expected_manifest_sha256, approve_discard);
            bail!("runtime stage discard requires Linux storage support");
        }
    }
    if let Some(pulsar_clients::cli::Commands::RuntimeUpdate {
        source,
        base,
        destination,
        expected_base_manifest_sha256,
    }) = pulsar_clients::runtime_install_command(&args)
    {
        #[cfg(target_os = "linux")]
        {
            let report = pulsar_engine::update_runtime_candidate(
                &source,
                &base,
                &destination,
                &expected_base_manifest_sha256,
            )?;
            println!("{}", report.json()?);
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (source, base, destination, expected_base_manifest_sha256);
            bail!("runtime candidate update requires Linux storage support");
        }
    }
    if let Some(pulsar_clients::cli::Commands::RuntimeInstall {
        source,
        destination,
    }) = pulsar_clients::runtime_install_command(&args)
    {
        #[cfg(target_os = "linux")]
        {
            let manifest_sha256 = pulsar_engine::install_runtime_candidate(&source, &destination)?;
            println!("Installed runtime candidate: {} (manifest SHA-256 {}). Model selection and inference approval remain separate.", destination.display(), manifest_sha256);
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (source, destination);
            bail!("runtime candidate installation requires Linux storage support");
        }
    }
    if let Some(
        command @ (pulsar_clients::cli::Commands::AssistantSelect { .. }
        | pulsar_clients::cli::Commands::AssistantDisable { .. }),
    ) = pulsar_clients::assistant_setup_command(&args)
    {
        #[cfg(target_os = "linux")]
        {
            let config = EngineConfig::for_user()?;
            match command {
                pulsar_clients::cli::Commands::AssistantSelect {
                    policy,
                    approve_host_inference,
                } => {
                    let digest = pulsar_engine::select_assistant_policy(
                        &config,
                        &policy,
                        approve_host_inference,
                    )?;
                    println!("Saved assistant startup choice (policy SHA-256 {digest}). Applies to the next Engine startup; inference and project effects require separate approval.");
                }
                pulsar_clients::cli::Commands::AssistantDisable {
                    expected_policy_sha256,
                    approve_disable,
                } => {
                    pulsar_engine::disable_assistant_policy(
                        &config,
                        &expected_policy_sha256,
                        approve_disable,
                    )?;
                    println!("Disabled saved assistant choice for the next Engine startup. A running Engine is unchanged; an environment override remains explicit.");
                }
                _ => unreachable!(),
            }
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = command;
            bail!("assistant startup selection requires Linux storage support");
        }
    }
    if let Some(pulsar_clients::cli::Commands::AssistantSetup {
        output,
        runtime,
        model,
        projector,
        cpu_threads,
        wall_time_ms,
        worker_memory_bytes,
        absolute_assets,
        approve_host_inference,
    }) = pulsar_clients::assistant_setup_command(&args)
    {
        #[cfg(target_os = "linux")]
        {
            let config = EngineConfig::for_user()?;
            let destination = output.clone();
            pulsar_engine::create_assistant_policy(
                &config,
                pulsar_engine::AssistantPolicySetup {
                    output,
                    runtime,
                    model,
                    projector,
                    cpu_threads,
                    wall_time_ms,
                    worker_memory_bytes,
                    absolute_assets,
                    approve_host_inference,
                },
            )?;
            println!("Saved private assistant host policy: {}. Select it with assistant-select --policy or PULSAR_ASSISTANT_OWNER_POLICY on the next Engine startup; inference and project effects require separate approval.", destination.display());
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (
                output,
                runtime,
                model,
                projector,
                cpu_threads,
                wall_time_ms,
                worker_memory_bytes,
                absolute_assets,
                approve_host_inference,
            );
            bail!("assistant host policy setup requires Linux confinement support");
        }
    }
    // Help/version/completion parsing must not start an engine or initialize inference.
    let config = EngineConfig::for_user()?;
    launch_client(
        &args,
        || ensure_engine(&config),
        |warning| {
            #[cfg(target_os = "linux")]
            let discovery: Option<pulsar_clients::BundledModelDiscovery> =
                Some(std::sync::Arc::new(|| {
                    let executable = std::env::current_exe()?;
                    let directory = executable
                        .parent()
                        .context("Installed runtime directory missing")?;
                    let hash = directory
                        .file_name()
                        .and_then(|p| p.to_str())
                        .context("Installed runtime identity missing")?;
                    let cache = directory.parent().context("Runtime cache missing")?;
                    pulsar_engine::inspect_cached_generation_packs(cache, hash)?.json()
                }));
            #[cfg(not(target_os = "linux"))]
            let discovery = None;
            let assistant_host: Option<pulsar_clients::AssistantHostService> = {
                #[cfg(target_os = "linux")]
                {
                    {
                        let assistant_config = config.clone();
                        Some(std::sync::Arc::new(move |request| {
                            handle_assistant_host_request_with_config(&assistant_config, request)
                        }))
                    }
                }
                #[cfg(not(target_os = "linux"))]
                {
                    None
                }
            };
            pulsar_clients::run_gui_with_host_services(
                config.endpoint(),
                config.bootstrap_path(),
                warning,
                discovery,
                assistant_host,
            )
        },
        || pulsar_clients::run_cli(args.clone(), &config.endpoint(), &config.bootstrap_path()),
    )
}

#[cfg(target_os = "linux")]
fn handle_assistant_host_request_with_config(
    config: &EngineConfig,
    request: pulsar_clients::AssistantHostRequest,
) -> Result<pulsar_clients::AssistantHostReply> {
    use pulsar_clients::AssistantHostRequest;
    let message = match request {
        AssistantHostRequest::Inspect => {
            "Saved assistant settings inspected. Changes apply at the next Engine startup."
                .to_string()
        }
        AssistantHostRequest::Setup(options) => {
            let path = options.output.clone();
            pulsar_engine::create_assistant_policy(
                config,
                pulsar_engine::AssistantPolicySetup {
                    output: options.output,
                    runtime: options.runtime,
                    model: options.model,
                    projector: options.projector,
                    cpu_threads: options.cpu_threads,
                    wall_time_ms: options.wall_time_ms,
                    worker_memory_bytes: options.worker_memory_bytes,
                    absolute_assets: false,
                    approve_host_inference: options.approve_host_inference,
                },
            )?;
            format!(
                "Measured policy created at {}. Select it separately for startup.",
                path.display()
            )
        }
        AssistantHostRequest::Select {
            policy,
            approve_host_inference,
        } => {
            pulsar_engine::select_assistant_policy(config, &policy, approve_host_inference)?;
            "Assistant choice saved for the next Engine startup.".into()
        }
        AssistantHostRequest::Disable {
            expected_policy_sha256,
            approve_disable,
        } => {
            pulsar_engine::disable_assistant_policy(
                config,
                &expected_policy_sha256,
                approve_disable,
            )?;
            "Saved assistant choice disabled for the next Engine startup.".into()
        }
    };
    let saved = pulsar_engine::inspect_assistant_policy_selection(config)?;
    Ok(pulsar_clients::AssistantHostReply {
        saved_policy_sha256: saved.as_ref().map(|s| s.policy_sha256.clone()),
        validation_error: saved.and_then(|s| s.validation_error),
        external_override: std::env::var_os("PULSAR_ASSISTANT_OWNER_POLICY").is_some(),
        message,
    })
}

#[cfg(target_os = "linux")]
fn launch_prepared_runtime(
    arguments: Vec<OsString>,
    prepare: impl Fn() -> Result<std::path::PathBuf> + Send + Sync + 'static,
) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let directory = if pulsar_clients::runtime_launch_opens_gui(&arguments) {
        pulsar_clients::run_runtime_bootstrap(prepare)?
    } else {
        prepare()?
    };
    let error = Command::new(directory.join("pulsar"))
        .args(arguments)
        .env_remove("APPDIR")
        .env_remove("APPIMAGE")
        .env_remove("APPIMAGE_EXTRACT_AND_RUN")
        .exec();
    Err(error).context("execute verified persistent runtime")
}

fn launch_client(
    args: &[OsString],
    start_engine: impl FnOnce() -> Result<()>,
    open_gui: impl FnOnce(Option<String>) -> Result<()>,
    run_cli: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let startup = if pulsar_clients::command_needs_engine(args) {
        start_engine()
    } else {
        Ok(())
    };
    if pulsar_clients::command_opens_gui(args) {
        return open_gui(startup.err().map(|error| format!("{error:#}")));
    }
    startup?;
    run_cli()
}

const ENGINE_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const ENDPOINT_PROBE_TIMEOUT: Duration = Duration::from_millis(250);

fn ensure_engine(config: &EngineConfig) -> Result<()> {
    // Every readiness probe, contender-exit check and retry shares this budget.
    let deadline = Instant::now()
        .checked_add(ENGINE_STARTUP_TIMEOUT)
        .context("engine startup timeout overflow")?;
    if endpoint_accepts_connections(config, deadline)? {
        // Authentication and version negotiation happen in the client. An existing
        // incompatible engine must never be killed or replaced by this launcher.
        return Ok(());
    }
    let executable = std::env::current_exe().context("locate Pulsar executable")?;
    let mut child = Command::new(executable)
        .arg("engine")
        .env("PULSAR_STATE_DIR", &config.state_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("start per-user Pulsar engine")?;
    loop {
        if endpoint_accepts_connections(config, deadline)? {
            // Dropping Child does not kill the engine. Authorized offline jobs
            // intentionally survive closure of their originating client.
            return Ok(());
        }
        if let Some(status) = child.try_wait().context("observe engine startup")? {
            // Another launcher can acquire the instance lock before its listener
            // exists. Wait only for actual lock contention, never mere lock-file
            // existence, and never spawn another contender in this loop.
            if endpoint_accepts_connections(config, deadline)? {
                return Ok(());
            }
            if !pulsar_engine::instance_lock_is_held(config)
                .context("inspect existing engine ownership without modifying it")?
            {
                bail!("engine startup exited with {status}; run 'pulsar engine' to see its diagnostic");
            }
        }
        std::thread::sleep(Duration::from_millis(40).min(startup_remaining(deadline)?));
    }
}

fn startup_remaining(deadline: Instant) -> Result<Duration> {
    // Do not kill a process whose successful instance ownership is unknown.
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .context("engine startup deadline exceeded; inspect the engine before retrying")
}

#[cfg(unix)]
fn endpoint_accepts_connections(config: &EngineConfig, deadline: Instant) -> Result<bool> {
    let timeout = startup_remaining(deadline)?.min(ENDPOINT_PROBE_TIMEOUT);
    match pulsar_protocol::connect_local(&config.endpoint(), timeout) {
        Ok(_) => Ok(true),
        Err(error) if matches!(error.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused) => Ok(false),
        Err(error) => Err(error).context(
            "engine endpoint is busy, unresponsive, or inaccessible; existing engine left untouched"),
    }
}

#[cfg(not(unix))]
fn endpoint_accepts_connections(_config: &EngineConfig, deadline: Instant) -> Result<bool> {
    startup_remaining(deadline)?;
    // A platform-specific local transport must be supplied before this platform
    // can advertise a running engine. Never mistake a token file for readiness.
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_host_callback_uses_actual_private_configuration_without_engine_authority() {
        use pulsar_clients::{AssistantHostRequest, AssistantHostSetup};
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let runtime = root.path().join("runtime");
        let model = root.path().join("model");
        let policy = root.path().join("policy.json");
        std::fs::copy("/usr/bin/true", &runtime).unwrap();
        std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(&model, b"nonexecuted model metadata fixture").unwrap();
        let config = EngineConfig::new(root.path().join("state"), "/usr/bin/true".into());
        let mut setup = AssistantHostSetup {
            output: policy.clone(),
            runtime,
            model,
            projector: None,
            cpu_threads: 1,
            wall_time_ms: 1000,
            worker_memory_bytes: 512 << 20,
            approve_host_inference: false,
        };
        assert!(handle_assistant_host_request_with_config(
            &config,
            AssistantHostRequest::Setup(setup.clone())
        )
        .is_err());
        assert!(!policy.exists());
        setup.approve_host_inference = true;
        let created =
            handle_assistant_host_request_with_config(&config, AssistantHostRequest::Setup(setup))
                .unwrap();
        assert!(created.saved_policy_sha256.is_none());
        let source = std::fs::read(&policy).unwrap();
        let selected = handle_assistant_host_request_with_config(
            &config,
            AssistantHostRequest::Select {
                policy: policy.clone(),
                approve_host_inference: true,
            },
        )
        .unwrap();
        assert!(selected.validation_error.is_none());
        let original = selected.saved_policy_sha256.unwrap();
        let saved = config.state_dir.join("assistant-startup-policy.json");
        std::fs::write(&saved, b"truncated").unwrap();
        let corrupt =
            handle_assistant_host_request_with_config(&config, AssistantHostRequest::Inspect)
                .unwrap();
        assert!(corrupt.validation_error.is_some());
        assert!(handle_assistant_host_request_with_config(
            &config,
            AssistantHostRequest::Disable {
                expected_policy_sha256: original,
                approve_disable: true
            }
        )
        .is_err());
        assert!(saved.exists());
        let disabled = handle_assistant_host_request_with_config(
            &config,
            AssistantHostRequest::Disable {
                expected_policy_sha256: corrupt.saved_policy_sha256.unwrap(),
                approve_disable: true,
            },
        )
        .unwrap();
        assert!(disabled.saved_policy_sha256.is_none() && !saved.exists());
        assert_eq!(std::fs::read(&policy).unwrap(), source);
        assert!(
            !config.bootstrap_path().exists()
                && !config.endpoint().exists()
                && !config.state_dir.join("projects.sqlite3").exists()
        );
    }
    #[test]
    fn graphical_startup_failure_reaches_window_with_diagnostic() {
        for values in [vec!["pulsar"], vec!["pulsar", "gui"]] {
            let args: Vec<_> = values.into_iter().map(OsString::from).collect();
            launch_client(
                &args,
                || bail!("startup refused"),
                |warning| {
                    assert_eq!(warning.as_deref(), Some("startup refused"));
                    Ok(())
                },
                || panic!("graphical invocation routed to CLI"),
            )
            .unwrap();
        }
    }

    #[test]
    fn command_startup_failure_is_returned_without_dispatch() {
        let args = ["pulsar", "generate", "video.mp4"].map(OsString::from);
        let error = launch_client(
            &args,
            || bail!("startup refused"),
            |_| panic!("CLI opened window"),
            || panic!("failed startup dispatched command"),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "startup refused");
    }

    #[test]
    fn informational_and_invalid_commands_skip_startup_and_window() {
        for values in [
            vec!["pulsar", "--help"],
            vec!["pulsar", "--version"],
            vec!["pulsar", "completions", "bash"],
            vec!["pulsar", "gui", "unexpected"],
        ] {
            let args: Vec<_> = values.into_iter().map(OsString::from).collect();
            launch_client(
                &args,
                || panic!("unexpected startup"),
                |_| panic!("unexpected window"),
                || Ok(()),
            )
            .unwrap();
        }
    }

    #[test]
    fn successful_graphical_startup_has_no_warning() {
        let args = [OsString::from("pulsar")];
        launch_client(
            &args,
            || Ok(()),
            |warning| {
                assert!(warning.is_none());
                Ok(())
            },
            || panic!("unexpected CLI"),
        )
        .unwrap();
    }
}
