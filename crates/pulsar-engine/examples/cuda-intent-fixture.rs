//! Authored private database fixture. No admission, capture or native effects.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use rusqlite::{params, Connection, OpenFlags};
fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    ensure!((2..=3).contains(&args.len()), "MODE DATABASE [TAMPER]");
    let path = std::path::Path::new(&args[1]);
    ensure!(
        path.is_absolute() && path.is_file(),
        "fixture requires existing private database"
    );
    let mut db = Connection::open_with_flags(
        path,
        if args[0] == "check" {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        },
    )?;
    match args[0].as_str() {
        "hold" => {
            ensure!(args.len() == 2, "hold arguments");
            db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE IF NOT EXISTS cuda_intent_fixture_note(value INTEGER); INSERT INTO cuda_intent_fixture_note VALUES(1);")?;
            use std::io::{Read, Write};
            println!("{{\"wal_fixture_ready\":true}}");
            std::io::stdout().flush()?;
            let mut byte = [0];
            std::io::stdin().read_exact(&mut byte)?;
            return Ok(());
        }
        "seed" => {
            ensure!(args.len() == 2, "seed arguments");
            let project: (String, i64) = db.query_row(
                "SELECT id,revision FROM projects ORDER BY id LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let session: String =
                db.query_row("SELECT id FROM sessions ORDER BY id LIMIT 1", [], |r| {
                    r.get(0)
                })?;
            let mut request: WorkerRequest =
                serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))?;
            request.project_id = ProjectId::new(project.0)?;
            request.base_revision = RevisionId::new(project.1.try_into()?);
            request.output_dir = path
                .parent()
                .context("database parent")?
                .join("attempts/authored-gpu-attempt");
            request.validate()?;
            let raw = serde_json::to_string(&request)?;
            let epoch = uuid::Uuid::new_v4().to_string();
            let tx = db.transaction()?;
            tx.execute("INSERT INTO jobs(id,attempt,project,base_revision,session,source,state,manifest) VALUES(?1,?2,?3,?4,?5,?6,'queued',?7)",params![request.job_id.as_str(),request.attempt_id.as_str(),request.project_id.as_str(),project.1,session,request.source.source_version.as_str(),raw])?;
            tx.execute(
                "INSERT INTO attempts VALUES(?1,?2,?3,'queued')",
                params![request.attempt_id.as_str(), request.job_id.as_str(), raw],
            )?;
            pulsar_engine::cuda_attempts::declare(&tx, &request, &epoch)?;
            tx.commit()?;
        }
        "check" => {
            ensure!(args.len() == 2, "check arguments");
        }
        "accept" => {
            ensure!(args.len() == 2, "accept arguments");
            let (raw,epoch):(String,String)=db.query_row("SELECT manifest,engine_epoch FROM cuda_attempt_intents WHERE attempt='authored-gpu-attempt'",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
            let request: WorkerRequest = serde_json::from_str(&raw)?;
            ensure!(
                request.output_dir == path.parent().unwrap().join("attempts/authored-gpu-attempt"),
                "fixture output root differs"
            );
            pulsar_engine::artifacts::private_directory(&request.output_dir)?;
            let execution = request
                .cuda_execution
                .as_ref()
                .context("CUDA fixture declaration missing")?;
            let profile = match &request.operation {
                WorkerOperation::Generate { settings, .. } => {
                    settings.inference_profile.as_ref().unwrap()
                }
                _ => anyhow::bail!("fixture generation missing"),
            };
            let trace = serde_json::to_vec(&serde_json::json!([
                {"cat":"Node","name":"Conv_0_kernel_time","ph":"X","ts":11,"dur":2,"pid":1,"tid":2,"args":{"provider":"CUDAExecutionProvider","op_name":"Conv"}},
                {"cat":"Session","name":"model_run","ph":"X","ts":10,"dur":10,"pid":1,"tid":2}
            ]))?;
            use sha2::{Digest, Sha256};
            let checked = pulsar_engine::inference_trace::validate_node_trace(
                &trace,
                InferenceProvider::Cuda,
                1,
            )?;
            let receipt = serde_json::json!({"provider":"cuda","provider_evidence":"strict-cuda-node-trace",
                "device":{"device_uuid":execution.device_uuid,"pci_bus_address":execution.pci_bus_address,"device_ordinal":execution.device_ordinal,"visible_device_count":1,"total_bytes":25_313_280_000u64,"driver_version":13040},
                "successful_runs":1,"trace_path":request.output_dir.join("cuda_model_authored.json"),"trace":checked,
                "worker_binding":{"attempt_id":request.attempt_id,"job_id":request.job_id,"request_sha256":format!("{:x}",Sha256::digest(raw.as_bytes())),"profile_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(profile)?)),"model":profile.model,"runtime":profile.runtime,"owner_policy":execution.owner_policy}});
            std::fs::write(request.output_dir.join("cuda-trace.json"), trace)?;
            std::fs::write(
                request.output_dir.join("cuda-inference.json"),
                serde_json::to_vec(&receipt)?,
            )?;
            let evidence = WorkerCudaArtifacts {
                receipt: pulsar_engine::worker::pin_artifact(
                    &request.output_dir.join("cuda-inference.json"),
                )?,
                trace: pulsar_engine::worker::pin_artifact(
                    &request.output_dir.join("cuda-trace.json"),
                )?,
            };
            let tx = db.transaction()?;
            tx.execute_batch("UPDATE jobs SET state='running' WHERE id='authored-gpu-job';UPDATE attempts SET state='running' WHERE id='authored-gpu-attempt';")?;
            pulsar_engine::cuda_attempts::require_running(&tx, &request, &epoch)?;
            tx.execute("INSERT INTO candidates(id,project,base_revision,program,job,lineage,review) SELECT 'authored-gpu-candidate',id,revision,program,?1,?2,'[]' FROM projects WHERE id=?3",params![request.job_id.as_str(),serde_json::to_string(&vec![evidence.receipt.identity.clone(),evidence.trace.identity.clone()])?,request.project_id.as_str()])?;
            tx.execute_batch("UPDATE jobs SET state='completed',candidate='authored-gpu-candidate' WHERE id='authored-gpu-job';UPDATE attempts SET state='completed' WHERE id='authored-gpu-attempt';")?;
            pulsar_engine::cuda_acceptance::retain(
                &tx,
                &request,
                &CandidateId::new("authored-gpu-candidate")?,
                &evidence,
            )?;
            tx.commit()?;
        }
        "tamper-acceptance" => {
            ensure!(args.len() == 3, "tamper-acceptance arguments");
            db.execute_batch("PRAGMA foreign_keys=OFF;")?;
            let sql = match args[2].as_str() {
                "downgrade" => "PRAGMA user_version=15;",
                "missing-table" => "DROP TABLE cuda_candidate_evidence;",
                "membership" => "DELETE FROM cuda_candidate_evidence;",
                "digest" => "UPDATE cuda_candidate_evidence SET request_sha256=printf('%064d',0);",
                "lineage" => {
                    "UPDATE candidates SET lineage='[]' WHERE id='authored-gpu-candidate';"
                }
                "candidate" => "UPDATE jobs SET candidate=NULL WHERE id='authored-gpu-job';",
                "state" => "UPDATE attempts SET state='running' WHERE id='authored-gpu-attempt';",
                "oversized" => "UPDATE cuda_candidate_evidence SET artifacts=zeroblob(32769);",
                "noncanonical" => "UPDATE cuda_candidate_evidence SET artifacts=' '||artifacts;",
                "layout" => "ALTER TABLE cuda_candidate_evidence ADD COLUMN accepted INTEGER;",
                _ => anyhow::bail!("unknown acceptance tamper"),
            };
            db.execute_batch(sql)?;
            println!("{{\"authored_acceptance_tamper\":true}}");
            return Ok(());
        }
        "tamper" => {
            ensure!(args.len() == 3, "tamper arguments");
            db.execute_batch("PRAGMA foreign_keys=OFF;")?;
            let sql = match args[2].as_str() {
                "downgrade" => "PRAGMA user_version=14;",
                "missing-table" => "DROP TABLE cuda_attempt_intents;",
                "digest" => "UPDATE cuda_attempt_intents SET request_sha256=printf('%064d',0);",
                "membership" => "DELETE FROM cuda_attempt_intents;",
                "oversized" => "UPDATE cuda_attempt_intents SET manifest=zeroblob(1048577);",
                "oversized-text" => "UPDATE cuda_attempt_intents SET manifest=replace(hex(zeroblob(1048577)),'00','x');",
                "attempt" => "UPDATE attempts SET manifest='{}' WHERE id='authored-gpu-attempt';",
                "job" => "UPDATE jobs SET manifest='{}' WHERE id='authored-gpu-job';",
                "scope" => "UPDATE jobs SET project='another-project' WHERE id='authored-gpu-job';",
                "epoch" => "UPDATE cuda_attempt_intents SET engine_epoch='another-boot';",
                "oversized-epoch" => {
                    "UPDATE cuda_attempt_intents SET engine_epoch=zeroblob(1048577);"
                }
                "layout" => "ALTER TABLE cuda_attempt_intents ADD COLUMN approved INTEGER;",
                _ => anyhow::bail!("unknown fixture tamper"),
            };
            db.execute_batch(sql)?;
            println!("{{\"authored_tamper\":true}}");
            return Ok(());
        }
        _ => anyhow::bail!("unknown fixture mode"),
    }
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    pulsar_engine::cuda_attempts::validate(&db, version)?;
    pulsar_engine::cuda_acceptance::validate(&db, version)?;
    let row:(String,String,String)=db.query_row("SELECT engine_epoch,request_sha256,manifest FROM cuda_attempt_intents WHERE attempt='authored-gpu-attempt'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    let request: WorkerRequest = serde_json::from_str(&row.2)?;
    let states:(String,String)=db.query_row("SELECT j.state,a.state FROM jobs j JOIN attempts a ON a.id=j.attempt WHERE j.id='authored-gpu-job'",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
    println!(
        "{}",
        serde_json::json!({"schema":version,"engine_epoch":row.0,"request_sha256":row.1,"job_state":states.0,"attempt_state":states.1,"attempt_directory_exists":request.output_dir.exists(),"native_execution":false,"authorization":"declared-intent-only"})
    );
    Ok(())
}
