//! Explicit data-only worker harness. No project/engine mutation authority.
fn main() {
    #[cfg(feature = "wasm")]
    if let Err(error) = pulsar_engine::wasm_worker::run_worker() {
        eprintln!("WASM worker: {error:#}");
        std::process::exit(1);
    }
    #[cfg(not(feature = "wasm"))]
    {
        eprintln!("WASM worker support was not compiled into this build");
        std::process::exit(1);
    }
}
