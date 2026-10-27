//! Compatibility probe for the ordinary CPU live-worker manifest.
use std::io::Read;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err("live manifest exceeds control bound".into());
    }
    if std::env::args().nth(1).as_deref() == Some("--cuda-contract") {
        let manifest: pulsar_protocol::LiveCudaInferenceManifest = serde_json::from_slice(&bytes)?;
        use std::io::Write;
        std::io::stdout().write_all(manifest.identity_sha256()?.as_bytes())?;
        return Ok(());
    }
    let manifest: pulsar_protocol::LiveInferenceManifest = serde_json::from_slice(&bytes)?;
    manifest.validate()?;
    use std::io::Write;
    std::io::stdout().write_all(&serde_json::to_vec(&manifest)?)?;
    Ok(())
}
