//! Wasmtime runs only in the explicit worker role, never during engine handling.
//! Outputs are untrusted data proposals, not admitted project edits.
use anyhow::{bail, Context, Result};
use pulsar_protocol::{
    WasmProcessingRequest, WasmProcessingResult, WasmWorkerRequest, WasmWorkerResponse,
    PROTOCOL_VERSION, WASM_FUEL_LIMIT, WASM_PROCESSING_ABI, WASM_REQUEST_MAX_BYTES,
};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use wasmtime::{Config, Engine, Instance, Module, Store, StoreLimitsBuilder};

pub fn run_worker() -> Result<()> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(WASM_REQUEST_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .context("read bounded WASM worker request")?;
    if bytes.len() as u64 > WASM_REQUEST_MAX_BYTES {
        bail!("WASM worker request exceeds transport admission");
    }
    let envelope: WasmWorkerRequest =
        serde_json::from_slice(&bytes).context("parse WASM worker request")?;
    envelope.validate()?;
    #[cfg(target_os = "linux")]
    if std::env::var("PULSAR_WORKER_CONFINED").as_deref() == Ok("1") {
        // Namespace setup needed a bounded executable copy. Restore the tighter
        // per-request file cap before compilation or any guest execution.
        let value = envelope.request.manifest.budget.output_bytes as libc::rlim_t;
        let limit = libc::rlimit {
            rlim_cur: value,
            rlim_max: value,
        };
        if unsafe { libc::setrlimit(libc::RLIMIT_FSIZE, &limit) } != 0 {
            return Err(std::io::Error::last_os_error())
                .context("restore WASM worker output file limit");
        }
    }
    let result = execute(&envelope.request)?;
    let response = WasmWorkerResponse {
        version: PROTOCOL_VERSION,
        attempt_id: envelope.attempt_id,
        output: result,
    };
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &response).context("write WASM worker result")?;
    stdout
        .write_all(b"\n")
        .context("finish WASM worker result")?;
    stdout.flush().context("flush WASM worker result")?;
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Native compilation is not preempted by guest fuel/epochs. A production
/// supervisor must enforce the whole-process deadline and native resource
/// containment before giving this role arbitrary modules. No engine command
/// currently dispatches this role.
fn execute(request: &WasmProcessingRequest) -> Result<WasmProcessingResult> {
    let started = Instant::now();
    request.validate()?;
    let deadline = started
        .checked_add(Duration::from_millis(request.manifest.budget.wall_time_ms))
        .context("WASM deadline overflow")?;
    if digest(&request.module) != request.manifest.artifact.sha256 {
        bail!("WASM module does not match captured artifact identity");
    }
    if request.module.get(..8) != Some(b"\0asm\x01\0\0\0") {
        bail!("WASM worker accepts binary core modules only");
    }
    let mut config = Config::new();
    config
        .consume_fuel(true)
        .epoch_interruption(true)
        .wasm_multi_memory(false)
        .wasm_memory64(false)
        .memory_reservation(16 * 1024 * 1024)
        .memory_reservation_for_growth(0)
        .memory_guard_size(65536)
        .max_wasm_stack(256 * 1024);
    let engine = runtime_result(Engine::new(&config)).context("initialize bounded WASM runtime")?;
    // Never deserialize precompiled native code supplied by an extension.
    let module =
        runtime_result(Module::new(&engine, &request.module)).context("compile WASM module")?;
    if module.imports().next().is_some() {
        bail!("data-only WASM modules cannot import host functions or resources");
    }
    check_deadline(deadline)?;
    let limits = StoreLimitsBuilder::new()
        .memory_size(request.guest_memory_bytes as usize)
        .memories(1)
        .instances(1)
        .tables(1)
        .table_elements(4096)
        .trap_on_grow_failure(true)
        .build();
    let mut store = Store::new(&engine, limits);
    store.limiter(|limits| limits);
    runtime_result(store.set_fuel(WASM_FUEL_LIMIT))?;
    // One epoch suffices: this invocation owns its Engine and timer. It is
    // armed before instantiation, so module start functions are covered too.
    store.set_epoch_deadline(1);
    let _timer = DeadlineTimer::start(engine.clone(), deadline)?;
    let instance = runtime_result(Instance::new(&mut store, &module, &[]))
        .context("instantiate WASM module")?;
    let memory = instance
        .get_memory(&mut store, "memory")
        .context("WASM ABI requires exported memory")?;
    let alloc = runtime_result(instance.get_typed_func::<i32, i32>(&mut store, "pulsar_alloc"))
        .context("WASM ABI requires pulsar_alloc(i32) -> i32")?;
    let process =
        runtime_result(instance.get_typed_func::<(i32, i32), i64>(&mut store, "pulsar_process"))
            .context("WASM ABI requires pulsar_process(i32, i32) -> i64")?;
    check_deadline(deadline)?;
    let length = i32::try_from(request.input.len()).context("WASM input length overflow")?;
    let pointer = runtime_result(alloc.call(&mut store, length)).context("allocate WASM input")?;
    let offset = usize::try_from(pointer).context("WASM allocator returned a negative pointer")?;
    memory
        .write(&mut store, offset, &request.input)
        .context("WASM input buffer is outside guest memory")?;
    check_deadline(deadline)?;
    let packed = runtime_result(process.call(&mut store, (pointer, length)))
        .context("execute WASM processor")? as u64;
    let output_offset = (packed >> 32) as usize;
    let output_len = (packed & u32::MAX as u64) as usize;
    if output_len as u64 > request.manifest.budget.output_bytes {
        bail!("WASM output exceeds declared buffer budget");
    }
    // Check the guest range before allocating/copying even a zero-length result.
    let end = output_offset
        .checked_add(output_len)
        .context("WASM output range overflow")?;
    if end > memory.data_size(&store) {
        bail!("WASM output buffer is outside guest memory");
    }
    let mut output = vec![0; output_len];
    memory
        .read(&store, output_offset, &mut output)
        .context("read WASM output buffer")?;
    check_deadline(deadline)?;
    let remaining = runtime_result(store.get_fuel())?;
    Ok(WasmProcessingResult {
        abi: WASM_PROCESSING_ABI.to_owned(),
        module_sha256: request.manifest.artifact.sha256.clone(),
        input_sha256: digest(&request.input),
        output_sha256: digest(&output),
        output,
        fuel_consumed: WASM_FUEL_LIMIT - remaining,
    })
}

fn runtime_result<T>(result: wasmtime::Result<T>) -> Result<T> {
    result.map_err(|error| anyhow::Error::from_boxed(error.into_boxed_dyn_error()))
}

fn check_deadline(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        bail!("WASM invocation deadline exceeded");
    }
    Ok(())
}

struct DeadlineTimer {
    cancelled: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl DeadlineTimer {
    fn start(engine: Engine, deadline: Instant) -> Result<Self> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let thread = std::thread::Builder::new()
            .name("pulsar-wasm-deadline".into())
            .spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    let now = Instant::now();
                    if now >= deadline {
                        engine.increment_epoch();
                        break;
                    }
                    std::thread::park_timeout(deadline.saturating_duration_since(now));
                }
            })
            .context("start WASM deadline timer")?;
        Ok(Self {
            cancelled,
            thread: Some(thread),
        })
    }
}
impl Drop for DeadlineTimer {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulsar_core::ArtifactId;
    use pulsar_protocol::{
        ArtifactIdentity, ConfinementRequirement, ExtensionManifest, ExtensionRole, ResourceBudget,
        PROTOCOL_VERSION,
    };

    fn request(wat: &str) -> WasmProcessingRequest {
        let module = wat::parse_str(wat).unwrap();
        WasmProcessingRequest {
            manifest: ExtensionManifest {
                schema_version: PROTOCOL_VERSION,
                name: "test".into(),
                role: ExtensionRole::Processor,
                abi: WASM_PROCESSING_ABI.into(),
                artifact: ArtifactIdentity {
                    artifact_id: ArtifactId::new("test").unwrap(),
                    sha256: digest(&module),
                    byte_len: module.len() as u64,
                },
                requested_scopes: vec![],
                budget: ResourceBudget {
                    memory_bytes: 64 * 1024 * 1024,
                    output_bytes: 65536,
                    wall_time_ms: 5000,
                    cpu_threads: 2,
                },
                confinement: ConfinementRequirement::Wasm,
            },
            module,
            input: vec![1, 2, 3],
            guest_memory_bytes: 65536,
        }
    }
    const IDENTITY: &str = r#"(module (memory (export "memory") 1)
        (func (export "pulsar_alloc") (param i32) (result i32) i32.const 16)
        (func (export "pulsar_process") (param i32 i32) (result i64)
            local.get 0 i64.extend_i32_u i64.const 32 i64.shl
            local.get 1 i64.extend_i32_u i64.or))"#;

    #[test]
    fn actual_guest_output_is_identified_and_fresh_per_invocation() {
        let mut r = request(IDENTITY);
        let first = execute(&r).unwrap();
        assert_eq!(first.output, r.input);
        assert_eq!(first.output_sha256, digest(&r.input));
        assert!(first.fuel_consumed > 0);
        r.input = vec![9];
        assert_eq!(execute(&r).unwrap().output, vec![9]);
    }
    #[test]
    fn actual_guest_transforms_the_copied_input() {
        let r = request(
            r#"(module (memory (export "memory") 1)
            (func (export "pulsar_alloc") (param i32) (result i32) i32.const 16)
            (func (export "pulsar_process") (param i32 i32) (result i64)
                local.get 0 local.get 0 i32.load8_u i32.const 255 i32.xor i32.store8
                local.get 0 i64.extend_i32_u i64.const 32 i64.shl
                local.get 1 i64.extend_i32_u i64.or))"#,
        );
        let result = execute(&r).unwrap();
        assert_eq!(result.output, vec![254, 2, 3]);
        assert_ne!(result.input_sha256, result.output_sha256);
        assert_eq!(r.input, vec![1, 2, 3]);
    }
    #[test]
    fn epoch_timer_interrupts_a_running_guest_without_waiting_for_fuel() {
        let mut config = Config::new();
        config.consume_fuel(true).epoch_interruption(true);
        let engine = Engine::new(&config).unwrap();
        let binary = wat::parse_str(r#"(module (func (export "loop") (loop br 0)))"#).unwrap();
        let module = Module::new(&engine, binary).unwrap();
        let mut store = Store::new(&engine, ());
        store.set_fuel(u64::MAX).unwrap();
        store.set_epoch_deadline(1);
        let instance = Instance::new(&mut store, &module, &[]).unwrap();
        let run = instance
            .get_typed_func::<(), ()>(&mut store, "loop")
            .unwrap();
        let timer =
            DeadlineTimer::start(engine, Instant::now() + Duration::from_millis(20)).unwrap();
        assert!(format!("{:#}", run.call(&mut store, ()).unwrap_err()).contains("interrupt"));
        drop(timer);
    }
    #[test]
    fn identity_roles_scopes_and_abi_do_not_create_authority() {
        let r = request(IDENTITY);
        let mut bad = r.clone();
        bad.module[8] ^= 1;
        assert!(execute(&bad).unwrap_err().to_string().contains("identity"));
        for role in [ExtensionRole::Runtime, ExtensionRole::Driver] {
            let mut bad = r.clone();
            bad.manifest.role = role;
            assert!(execute(&bad).is_err());
        }
        let mut bad = r.clone();
        bad.manifest.requested_scopes = vec![pulsar_protocol::Scope::Edit];
        assert!(execute(&bad).is_err());
        let mut bad = r;
        bad.manifest.abi = "unknown".into();
        assert!(execute(&bad).is_err());
    }
    #[test]
    fn imports_and_wrong_signatures_refuse() {
        let r = request(r#"(module (import "host" "effect" (func)))"#);
        assert!(execute(&r).unwrap_err().to_string().contains("import"));
        let r = request(
            r#"(module (memory (export "memory") 1)
            (func (export "pulsar_alloc") (result i32) i32.const 0))"#,
        );
        assert!(execute(&r).is_err());
    }
    #[test]
    fn start_and_processor_loops_exhaust_fuel() {
        for wat in [
            r#"(module (func $start (loop br 0)) (start $start))"#,
            r#"(module (memory (export "memory") 1)
            (func (export "pulsar_alloc") (param i32) (result i32) i32.const 0)
            (func (export "pulsar_process") (param i32 i32) (result i64)
                (loop br 0) i64.const 0))"#,
        ] {
            let r = request(wat);
            assert!(format!("{:#}", execute(&r).unwrap_err()).contains("fuel"));
        }
    }
    #[test]
    fn initial_memory_growth_and_bad_guest_ranges_refuse() {
        for body in [
            "i32.const 1 memory.grow drop i64.const 0",
            "i64.const 281474976710659", // offset 65536, length 3
            "i64.const 65537",           // output above budget
        ] {
            let wat = format!(
                r#"(module (memory (export "memory") 1)
                (func (export "pulsar_alloc") (param i32) (result i32) i32.const 0)
                (func (export "pulsar_process") (param i32 i32) (result i64) {body}))"#
            );
            assert!(execute(&request(&wat)).is_err());
        }
        let r = request(&IDENTITY.replace("memory\") 1", "memory\") 2"));
        assert!(execute(&r).is_err());
        let r = request(&IDENTITY.replace("i32.const 16", "i32.const -1"));
        assert!(execute(&r).is_err());
    }
}
