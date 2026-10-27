//! One-shot learned inference in the data-only confined worker role.
//! Parent owns immutable mounts, aggregate/cgroup admission, cancellation and
//! whole-tree retirement. No host command, project state, or device capability.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    process::{Child, Command, Stdio},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
const EOG_TRAILER: &[u8] = b" [end of text]\n\n\n";
const RUNTIME: &str = "/assistant/runtime";
const MODEL: &str = "/assistant/model.gguf";
const PROJECTOR: &str = "/assistant/mmproj.gguf";
const RGB: &str = "/assistant/image.rgb";
fn proposal_schema(scope: &AssistantProposalScope) -> Result<Vec<u8>> {
    scope.validate()?;
    let mut variants = vec![
        serde_json::json!({"type":"object", "properties":{"kind":{"const":"no_action"}}, "required":["kind"], "additionalProperties":false}),
    ];
    for (kind, field, ids) in [
        ("select_target", "target_id", &scope.target_ids),
        ("explain_review", "review_id", &scope.review_ids),
        ("apply_edit_recipe", "recipe_id", &scope.edit_recipe_ids),
    ] {
        if ids.is_empty() {
            continue;
        }
        let mut properties = serde_json::Map::new();
        properties.insert("kind".into(), serde_json::json!({"const":kind}));
        properties.insert(
            field.into(),
            serde_json::json!({"type":"string","enum":ids}),
        );
        variants.push(serde_json::json!({"type":"object","properties":properties,"required":["kind",field],"additionalProperties":false}));
    }
    if !scope.allowed_sample_strides.is_empty() {
        variants.push(serde_json::json!({"type":"object","properties":{"kind":{"const":"set_sample_stride"},"stride":{"type":"integer","enum":scope.allowed_sample_strides}},"required":["kind","stride"],"additionalProperties":false}));
    }
    if let Some(creative) = &scope.creative_patterns {
        // The pinned native grammar does not enforce numeric range keywords.
        // Offer a bounded palette; the independent protocol/Core validator still
        // checks every completion, including pattern-specific cross-field rules.
        let palette = crate::assistant_palette::CreativePalette::new(creative)?;
        let frequencies = &palette.frequencies_hz;
        let item = serde_json::json!({"type":"object", "properties":{
            "pattern":{"type":"string","enum":palette.patterns},
            "axis":{"type":"string","enum":creative.axes},
            "duration":{"const":creative.duration},
            "frequency_hz":{"type":"number","enum":frequencies},
            "amplitude":{"type":"number","enum":palette.amplitudes},
            "offset":{"const":palette.offset},
            "phase_cycles":{"type":"number","enum":palette.phase_cycles},
            "sample_hz":{"const":creative.sample_hz},
            "duty_cycle":{"type":"number","enum":[0.25,0.5,0.75]}
        },"required":["pattern","axis","duration","frequency_hz","amplitude","offset","phase_cycles","sample_hz","duty_cycle"],"additionalProperties":false});
        let mut channels = Vec::new();
        for axis in &creative.axes {
            let mut kinds = Vec::new();
            for kind in palette.patterns {
                for (duty, allowed) in palette.choices(kind) {
                    if kind != "hold" && allowed.is_empty() {
                        continue;
                    }
                    let mut variant = item.clone();
                    let properties = variant["properties"]
                        .as_object_mut()
                        .expect("fixed schema object");
                    properties.insert("pattern".into(), serde_json::json!({"const":kind}));
                    properties.insert("axis".into(), serde_json::json!({"const":axis}));
                    properties.insert("duty_cycle".into(), serde_json::json!({"const":duty}));
                    if kind == "hold" {
                        for (field, value) in [
                            ("frequency_hz", palette.hold.frequency_hz),
                            ("amplitude", palette.hold.amplitude),
                            ("phase_cycles", palette.hold.phase_cycles),
                        ] {
                            properties.insert(field.into(), serde_json::json!({"const":value}));
                        }
                    } else {
                        properties.insert(
                            "frequency_hz".into(),
                            serde_json::json!({"type":"number","enum":allowed}),
                        );
                    }
                    kinds.push(variant);
                }
            }
            channels.push(serde_json::json!({"oneOf":kinds}));
        }
        // The pinned converter supports tuple item schemas. Fix one slot per
        // host axis rather than offering repeated unrestricted channel choices.
        let items = if channels.len() == 1 {
            channels.remove(0)
        } else {
            serde_json::json!(channels)
        };
        variants.push(serde_json::json!({"type":"object","properties":{
            "kind":{"const":"synthesize_patterns"},
            "patterns":{"type":"array","items":items,"additionalItems":false,"minItems":creative.axes.len(),"maxItems":creative.axes.len()}
        },"required":["kind","patterns"],"additionalProperties":false}));
    }
    Ok(crate::assistant_schema::encode(
        &serde_json::json!({"oneOf":variants}),
    )?)
}

pub fn run_worker() -> Result<()> {
    ensure!(
        std::env::var("PULSAR_WORKER_CONFINED").as_deref() == Ok("1"),
        "assistant role requires parent-owned confinement"
    );
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(MAX_ASSISTANT_WORKER_REQUEST_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_ASSISTANT_WORKER_REQUEST_BYTES,
        "assistant request transport exceeded"
    );
    let request: AssistantWorkerRequest =
        serde_json::from_slice(&bytes).context("decode assistant worker request")?;
    request.validate()?;
    let result = execute(
        &request,
        Path::new(RUNTIME),
        Path::new(MODEL),
        Path::new("/tmp"),
    )?;
    let response = AssistantWorkerResponse {
        vision: request.vision,
        version: PROTOCOL_VERSION,
        attempt_id: request.attempt_id,
        runtime: request.runtime,
        model: request.model,
        proposal: result,
    };
    let encoded = serde_json::to_vec(&response)?;
    ensure!(
        encoded.len() as u64 <= MAX_ASSISTANT_WORKER_RESPONSE_BYTES,
        "assistant response transport exceeded"
    );
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&encoded)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}
fn check(deadline: Instant) -> Result<()> {
    ensure!(
        Instant::now() < deadline,
        "assistant whole-worker deadline exceeded"
    );
    Ok(())
}
fn verify(path: &Path, expected: &ArtifactIdentity, deadline: Instant) -> Result<File> {
    check(deadline)?;
    let mut file = crate::artifacts::open_regular(path)?;
    ensure!(
        file.metadata()?.len() == expected.byte_len,
        "assistant mounted artifact length mismatch"
    );
    let mut hasher = Sha256::new();
    let mut read = 0u64;
    let mut block = [0; 64 * 1024];
    loop {
        check(deadline)?;
        let count = file.read(&mut block)?;
        if count == 0 {
            break;
        }
        read = read
            .checked_add(count as u64)
            .context("assistant hash length overflow")?;
        ensure!(
            read <= expected.byte_len,
            "assistant artifact grew during verification"
        );
        hasher.update(&block[..count]);
    }
    ensure!(
        read == expected.byte_len && format!("{:x}", hasher.finalize()) == expected.sha256,
        "assistant mounted artifact digest mismatch"
    );
    Ok(file)
}
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    file.write_all(bytes)?;
    Ok(())
}
fn decode_completion(bytes: &[u8], scope: &AssistantProposalScope) -> Result<AssistantProposal> {
    let body = bytes
        .strip_suffix(EOG_TRAILER)
        .context("assistant runtime omitted exact EOG envelope")?;
    Ok(AssistantProposal::decode(body, scope)?)
}
/// Only bounded, host-declared raw pixels enter this converter. Native image
/// parsing remains in the confined child; no ambient compressed media is opened.
fn prepare_vision_image(
    vision: &AssistantVisionInput,
    projector: &Path,
    rgb: &Path,
    output: &Path,
    deadline: Instant,
) -> Result<()> {
    vision.validate()?;
    let _projector = verify(projector, &vision.projector, deadline)?;
    let mut pixels = verify(rgb, &vision.rgb, deadline)?;
    pixels.seek(SeekFrom::Start(0))?;
    let mut bytes = vec![0; usize::try_from(vision.rgb.byte_len)?];
    pixels.read_exact(&mut bytes)?;
    check(deadline)?;
    let header = format!("P6\n{} {}\n255\n", vision.width, vision.height);
    let mut image = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(output)?;
    image.write_all(header.as_bytes())?;
    image.write_all(&bytes)?;
    check(deadline)?;
    Ok(())
}
fn runtime_command(
    request: &AssistantWorkerRequest,
    runtime: &Path,
    model: &Path,
    user: &Path,
    system: &Path,
    schema: &Path,
    vision_image: Option<&Path>,
) -> Command {
    let mut command = Command::new(runtime);
    command
        .env_clear()
        .env("LANG", "C.UTF-8")
        .arg("-m")
        .arg(model);
    if let Some(image) = vision_image {
        // MTMD has a separate flag/output contract from completion. Never
        // pass interactive-only text flags or scrape JSON from surrounding text.
        command
            .arg("--mmproj")
            .arg(PROJECTOR)
            .arg("--no-mmproj-offload")
            .arg("--image")
            .arg(image)
            .arg("-f")
            .arg(user)
            .arg("-sys")
            .arg(&request.system_context);
    } else {
        command
            .args([
                "--offline",
                "--conversation",
                "--single-turn",
                "--jinja",
                "--reasoning",
                "off",
                "-f",
            ])
            .arg(user)
            .arg("--system-prompt-file")
            .arg(system);
    }
    command
        .arg("--json-schema-file")
        .arg(schema)
        .args(["-c", "2048", "-n"])
        .arg(if request.scope.creative_patterns.is_some() {
            "1024"
        } else {
            "256"
        })
        .arg("-t")
        .arg(request.cpu_threads.to_string())
        .arg("-tb")
        .arg(request.cpu_threads.to_string())
        .args(["-b", "256", "-ub", "128", "-ngl", "0"]);
    if vision_image.is_none() {
        command.args(["--no-context-shift", "--no-display-prompt", "--simple-io"]);
    }
    command
        .args(["--seed", "42", "--temp", "0"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}
fn decode_runtime(bytes: &[u8], request: &AssistantWorkerRequest) -> Result<AssistantProposal> {
    let proposal = if request.vision.is_some() {
        // MTMD emits the proposal and whitespace, without the completion EOG
        // trailer. Strict whole-body parsing keeps all diagnostics separate.
        AssistantProposal::decode(bytes, &request.scope)?
    } else {
        decode_completion(bytes, &request.scope)?
    };
    if let (Some(scope), AssistantProposal::SynthesizePatterns { patterns }) =
        (&request.scope.creative_patterns, &proposal)
    {
        crate::assistant_palette::CreativePalette::new(scope)?.validate_patterns(patterns)?;
    }
    Ok(proposal)
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = self.0.kill();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if !matches!(self.0.try_wait(), Ok(None)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        // Parent cgroup/tree custody must outlive this role; an unobservable
        // kernel-blocked descendant is not permission to release admission.
    }
}
fn execute(
    request: &AssistantWorkerRequest,
    runtime: &Path,
    model: &Path,
    work_root: &Path,
) -> Result<AssistantProposal> {
    request.validate()?;
    let deadline = Instant::now()
        .checked_add(Duration::from_millis(request.wall_time_ms))
        .context("assistant deadline overflow")?;
    let _runtime = verify(runtime, &request.runtime, deadline)?;
    let _model = verify(model, &request.model, deadline)?;
    let directory = tempfile::Builder::new()
        .prefix("assistant-")
        .tempdir_in(work_root)?;
    let user = directory.path().join("user.txt");
    let system = directory.path().join("system.txt");
    let schema = directory.path().join("proposal.schema.json");
    write_private(&user, request.user_request.as_bytes())?;
    write_private(&system, request.system_context.as_bytes())?;
    write_private(&schema, &proposal_schema(&request.scope)?)?;
    check(deadline)?;
    let vision_image = directory.path().join("image.ppm");
    if let Some(vision) = &request.vision {
        prepare_vision_image(
            vision,
            Path::new(PROJECTOR),
            Path::new(RGB),
            &vision_image,
            deadline,
        )?;
    }
    let mut command = runtime_command(
        request,
        runtime,
        model,
        &user,
        &system,
        &schema,
        request.vision.as_ref().map(|_| vision_image.as_path()),
    );
    let mut child = OwnedChild(
        command
            .spawn()
            .context("spawn confined assistant runtime")?,
    );
    let mut stdout = child.0.stdout.take().context("assistant stdout missing")?;
    let stderr = child.0.stderr.take().context("assistant stderr missing")?;
    crate::launch_pipes::nonblocking(&stdout)?;
    crate::launch_pipes::nonblocking(&stderr)?;
    let cancel = AtomicBool::new(false);
    let mut monitor = crate::launch_pipes::Monitor::new(deadline, &cancel, stderr);
    let limit = MAX_ASSISTANT_PROPOSAL_BYTES
        + if request.vision.is_none() {
            EOG_TRAILER.len()
        } else {
            0
        };
    let mut bytes = Vec::new();
    monitor
        .read(&mut stdout)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limit,
        "assistant runtime output byte ceiling exceeded"
    );
    let status = loop {
        monitor.tick()?;
        match child.0.try_wait()? {
            Some(status) => break status,
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    };
    check(deadline)?;
    // Diagnostics never become user explanation or an inferred action.
    ensure!(status.success(), "assistant runtime failed: {status}");
    decode_runtime(&bytes, request)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vision_rgb_converter_checks_digest_dimensions_and_emits_exact_pnm() {
        let root = tempfile::tempdir().unwrap();
        let projector = root.path().join("mmproj");
        let rgb = root.path().join("rgb");
        let output = root.path().join("image.ppm");
        std::fs::write(&projector, b"projector").unwrap();
        std::fs::write(&rgb, [255, 0, 0, 0, 0, 255]).unwrap();
        let identity = |p: &Path| {
            let bytes = std::fs::read(p).unwrap();
            ArtifactIdentity {
                artifact_id: ArtifactId::new("vision-fixture").unwrap(),
                byte_len: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(bytes)),
            }
        };
        let good = AssistantVisionInput {
            projector: identity(&projector),
            rgb: identity(&rgb),
            width: 2,
            height: 1,
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        prepare_vision_image(&good, &projector, &rgb, &output, deadline).unwrap();
        assert_eq!(
            std::fs::read(&output).unwrap(),
            [b"P6\n2 1\n255\n".as_slice(), &[255, 0, 0, 0, 0, 255]].concat()
        );
        std::fs::remove_file(&output).unwrap();
        for case in 0..3 {
            let mut bad = good.clone();
            match case {
                0 => bad.rgb.sha256 = "a".repeat(64),
                1 => bad.width += 1,
                _ => bad.projector.sha256 = "a".repeat(64),
            }
            assert!(prepare_vision_image(&bad, &projector, &rgb, &output, deadline).is_err());
            assert!(!output.exists());
        }
        assert!(prepare_vision_image(&good, &projector, &rgb, &output, Instant::now()).is_err());
        assert!(!output.exists());
    }
    #[test]
    fn vision_command_and_whole_body_decoder_do_not_use_text_cli_contract() {
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new("test").unwrap(),
            sha256: "a".repeat(64),
            byte_len: 3,
        };
        let mut request = AssistantWorkerRequest {
            vision: None,
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("test").unwrap(),
            runtime: identity.clone(),
            model: identity.clone(),
            system_context: "context".into(),
            user_request: "prompt".into(),
            scope: AssistantProposalScope::default(),
            cpu_threads: 2,
            wall_time_ms: 1000,
        };
        let p = Path::new("/fixture");
        let command = runtime_command(&request, p, p, p, p, p, None);
        let text: Vec<_> = command.get_args().map(|a| a.to_str().unwrap()).collect();
        assert!(
            text.contains(&"--simple-io")
                && text.contains(&"--system-prompt-file")
                && text.contains(&"--single-turn")
        );
        request.vision = Some(AssistantVisionInput {
            projector: identity.clone(),
            rgb: identity,
            width: 1,
            height: 1,
        });
        request.scope.creative_patterns = Some(CreativePatternScope {
            axes: vec![Axis::Stroke],
            duration: ProjectTime::from_nanos(2_000_000_000),
            sample_hz: 8,
            max_amplitude: 0.4,
        });
        request.validate().unwrap();
        let command = runtime_command(&request, p, p, p, p, p, Some(p));
        let vision: Vec<_> = command.get_args().map(|a| a.to_str().unwrap()).collect();
        for required in [
            "--mmproj",
            "--no-mmproj-offload",
            "--image",
            "-sys",
            "--json-schema-file",
        ] {
            assert!(vision.contains(&required));
        }
        for forbidden in [
            "--simple-io",
            "--single-turn",
            "--system-prompt-file",
            "--no-context-shift",
            "--no-display-prompt",
        ] {
            assert!(!vision.contains(&forbidden));
        }
        assert!(decode_runtime(b"{\"kind\":\"no_action\"}\n\n", &request).is_ok());
        for invalid in [
            b"log {\"kind\":\"no_action\"}".as_slice(),
            b"{\"kind\":\"no_action\"} [end of text]",
            b"{\"kind\":\"no_action\",\"approve\":true}",
            b"{\"kind\":\"no_action\",\"kind\":\"no_action\"}",
            b"{\"kind\":",
        ] {
            assert!(decode_runtime(invalid, &request).is_err());
        }
    }
    #[test]
    fn native_decoding_refuses_core_valid_but_unoffered_creative_choices() {
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new("fixture").unwrap(),
            sha256: "a".repeat(64),
            byte_len: 3,
        };
        let mut request = AssistantWorkerRequest {
            vision: None,
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("palette-check").unwrap(),
            runtime: identity.clone(),
            model: identity.clone(),
            system_context: "context".into(),
            user_request: "prompt".into(),
            scope: AssistantProposalScope {
                creative_patterns: Some(CreativePatternScope {
                    axes: vec![Axis::Stroke],
                    duration: ProjectTime::from_nanos(2_000_000_000),
                    sample_hz: 8,
                    max_amplitude: 0.1,
                }),
                ..Default::default()
            },
            cpu_threads: 2,
            wall_time_ms: 1000,
        };
        let good = serde_json::json!({"kind":"synthesize_patterns","patterns":[{"pattern":"triangle","axis":"stroke","duration":2_000_000_000i64,"frequency_hz":1.0,"amplitude":0.1,"offset":0.5,"phase_cycles":0.0,"sample_hz":8,"duty_cycle":0.5}]});
        for vision in [false, true] {
            request.vision = vision.then(|| AssistantVisionInput {
                projector: identity.clone(),
                rgb: identity.clone(),
                width: 1,
                height: 1,
            });
            let envelope = |value: &serde_json::Value| {
                let mut bytes = serde_json::to_vec(value).unwrap();
                if !vision {
                    bytes.extend_from_slice(EOG_TRAILER);
                }
                bytes
            };
            assert!(decode_runtime(&envelope(&good), &request).is_ok());
            for (field, value) in [
                ("amplitude", 0.03),
                ("frequency_hz", 0.75),
                ("phase_cycles", 0.2),
                ("offset", 0.6),
            ] {
                let mut bad = good.clone();
                bad["patterns"][0][field] = serde_json::json!(value);
                assert!(
                    AssistantProposal::decode(&serde_json::to_vec(&bad).unwrap(), &request.scope)
                        .is_ok(),
                    "fixture must remain valid under broad Core/scope rules"
                );
                assert!(
                    decode_runtime(&envelope(&bad), &request).is_err(),
                    "native palette must refuse {field}={value}"
                );
            }
        }
    }

    #[test]
    fn ordinary_native_schema_bytes_remain_identical_to_sorted_encoding() {
        let scope = AssistantProposalScope {
            target_ids: vec!["cup2".into()],
            review_ids: vec!["flag1".into()],
            edit_recipe_ids: vec!["smooth_selection".into()],
            allowed_sample_strides: vec![1, 4],
            ..Default::default()
        };
        for scope in [scope, AssistantProposalScope::default()] {
            let bytes = proposal_schema(&scope).unwrap();
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(bytes, serde_json::to_vec(&value).unwrap());
        }
    }
    #[test]
    fn actual_creative_schema_emits_pattern_choice_before_amplitude() {
        let scope = AssistantProposalScope {
            creative_patterns: Some(CreativePatternScope {
                axes: vec![Axis::Stroke],
                duration: ProjectTime::from_nanos(2_000_000_000),
                sample_hz: 8,
                max_amplitude: 0.1,
            }),
            ..Default::default()
        };
        let text = String::from_utf8(proposal_schema(&scope).unwrap()).unwrap();
        assert!(text.contains("\"properties\":{\"pattern\":{\"const\":\"hold\"},\"axis\":"));
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        let variants = parsed["oneOf"][1]["properties"]["patterns"]["items"]["oneOf"]
            .as_array()
            .unwrap();
        assert_eq!(variants.len(), 6);
        for variant in variants {
            assert_eq!(variant["properties"]["duration"]["const"], 2_000_000_000i64);
            assert_eq!(variant["properties"]["sample_hz"]["const"], 8);
            assert_eq!(variant["additionalProperties"], false);
        }
    }
    #[test]
    fn creative_tuple_slots_fix_every_host_axis_and_all_choices_pass_core() {
        let mut checked = 0;
        for sample_hz in [1, 8, 1000] {
            let scope = AssistantProposalScope {
                creative_patterns: Some(CreativePatternScope {
                    axes: Axis::ALL.to_vec(),
                    duration: ProjectTime::from_nanos(2_000_000_000),
                    sample_hz,
                    max_amplitude: 0.1,
                }),
                ..Default::default()
            };
            let schema: serde_json::Value =
                serde_json::from_slice(&proposal_schema(&scope).unwrap()).unwrap();
            let patterns = &schema["oneOf"][1]["properties"]["patterns"];
            assert_eq!(patterns["additionalItems"], false);
            let slots = patterns["items"].as_array().unwrap();
            assert_eq!(slots.len(), 6);
            for (axis, slot) in Axis::ALL.iter().zip(slots) {
                for variant in slot["oneOf"].as_array().unwrap() {
                    assert_eq!(
                        variant["properties"]["axis"]["const"],
                        serde_json::to_value(axis).unwrap()
                    );
                    let mut completions = vec![serde_json::Map::new()];
                    for (field, rule) in variant["properties"].as_object().unwrap() {
                        let choices = if let Some(value) = rule.get("const") {
                            vec![value.clone()]
                        } else {
                            rule["enum"].as_array().unwrap().clone()
                        };
                        let mut next = Vec::new();
                        for completion in completions {
                            for choice in &choices {
                                let mut object = completion.clone();
                                object.insert(field.clone(), choice.clone());
                                next.push(object);
                            }
                        }
                        completions = next;
                    }
                    for completion in completions {
                        let spec: PatternSpec =
                            serde_json::from_value(serde_json::Value::Object(completion)).unwrap();
                        assert_eq!(spec.parameters().axis, *axis);
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 1000);
    }
    #[test]
    fn pulse_duty_palettes_respect_both_short_plateaus() {
        let scope = AssistantProposalScope {
            creative_patterns: Some(CreativePatternScope {
                axes: vec![Axis::Stroke],
                duration: ProjectTime::from_nanos(2_000_000_000),
                sample_hz: 8,
                max_amplitude: 0.1,
            }),
            ..Default::default()
        };
        let schema: serde_json::Value =
            serde_json::from_slice(&proposal_schema(&scope).unwrap()).unwrap();
        let variants = schema["oneOf"][1]["properties"]["patterns"]["items"]["oneOf"]
            .as_array()
            .unwrap();
        let mut duties = Vec::new();
        for variant in variants {
            let fields = &variant["properties"];
            if fields["pattern"]["const"] != "pulse" {
                continue;
            }
            let duty = fields["duty_cycle"]["const"].as_f64().unwrap();
            duties.push(duty);
            let frequencies = fields["frequency_hz"]["enum"].as_array().unwrap();
            assert!(frequencies.contains(&serde_json::json!(1.0)));
            assert_eq!(frequencies.contains(&serde_json::json!(2.0)), duty == 0.5);
        }
        assert_eq!(duties, vec![0.25, 0.5, 0.75]);
    }
    #[test]
    fn completion_requires_eog_and_context_and_refuses_log_recovery() {
        let scope = AssistantProposalScope {
            target_ids: vec!["cup2".into()],
            ..Default::default()
        };
        let good = [
            br#"{"kind":"select_target","target_id":"cup2"}"#.as_slice(),
            EOG_TRAILER,
        ]
        .concat();
        assert!(matches!(
            decode_completion(&good, &scope).unwrap(),
            AssistantProposal::SelectTarget { .. }
        ));
        for body in [
            b"log "
                .iter()
                .chain(good.iter())
                .copied()
                .collect::<Vec<_>>(),
            br#"{"kind":"no_action"}"#.to_vec(),
            [
                br#"{"kind":"select_target","target_id":"invented"}"#.as_slice(),
                EOG_TRAILER,
            ]
            .concat(),
        ] {
            assert!(decode_completion(&body, &scope).is_err());
        }
    }
    #[test]
    fn mounted_digest_and_deadline_checks_use_actual_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("model");
        std::fs::write(&path, b"model").unwrap();
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new("candidate").unwrap(),
            sha256: format!("{:x}", Sha256::digest(b"model")),
            byte_len: 5,
        };
        assert!(verify(&path, &identity, Instant::now() + Duration::from_secs(1)).is_ok());
        std::fs::write(&path, b"other").unwrap();
        assert!(verify(&path, &identity, Instant::now() + Duration::from_secs(1)).is_err());
        assert!(verify(&path, &identity, Instant::now()).is_err());
    }
    #[test]
    #[ignore = "explicit pinned real CPU candidate; not model or platform qualification"]
    fn real_candidate_returns_observed_target_through_production_worker_adapter() {
        let runtime = std::path::PathBuf::from(
            std::env::var_os("PULSAR_ASSISTANT_RUNTIME").expect("explicit candidate runtime"),
        );
        let model = std::path::PathBuf::from(
            std::env::var_os("PULSAR_ASSISTANT_MODEL").expect("explicit candidate model"),
        );
        fn identity(path: &Path) -> ArtifactIdentity {
            let mut file = File::open(path).unwrap();
            let byte_len = file.metadata().unwrap().len();
            let mut hash = Sha256::new();
            let mut block = [0; 65536];
            loop {
                let n = file.read(&mut block).unwrap();
                if n == 0 {
                    break;
                }
                hash.update(&block[..n]);
            }
            ArtifactIdentity {
                artifact_id: ArtifactId::new("candidate").unwrap(),
                byte_len,
                sha256: format!("{:x}", hash.finalize()),
            }
        }
        let runtime_identity = identity(&runtime);
        assert_eq!(
            runtime_identity.sha256,
            "f332257812cf358954a1701c9fb205e5ac50b4fbdf0e2c09944cc25ea58459ae"
        );
        let model_identity = identity(&model);
        assert!(matches!(
            model_identity.sha256.as_str(),
            "74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db"
                | "9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031"
        ));
        let work = tempfile::tempdir().unwrap();
        let request=AssistantWorkerRequest { vision: None,version:PROTOCOL_VERSION,attempt_id:AttemptId::new("probe").unwrap(),runtime:runtime_identity,model:model_identity,system_context:"You are Pulsar. Return one JSON action. Observed targets: cup1 is the first cup, cup2 is the second cup. Use {\"kind\":\"select_target\",\"target_id\":\"ID\"} to select a supplied target, or {\"kind\":\"no_action\"} for unsupported actions.".into(),user_request:"Track the second cup.".into(),scope:AssistantProposalScope {target_ids:vec!["cup1".into(),"cup2".into()],..Default::default()},cpu_threads:4,wall_time_ms:45_000};
        assert_eq!(
            execute(&request, &runtime, &model, work.path()).unwrap(),
            AssistantProposal::SelectTarget {
                target_id: "cup2".into()
            }
        );
    }
}
