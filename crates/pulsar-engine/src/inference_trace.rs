//! Consistency checks for bounded ORT node traces, not trace authenticity or qualification.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::InferenceProvider;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const MAX_TRACE_BYTES: usize = 8 * 1024 * 1024;
const MAX_EVENTS: usize = 65_536;
const MAX_RUNS: u64 = 16_384;

struct BoundedEvents(Vec<Value>);
impl<'de> Deserialize<'de> for BoundedEvents {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = BoundedEvents;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a bounded native trace event array")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut events = Vec::new();
                while let Some(event) = sequence.next_element::<Value>()? {
                    if events.len() == MAX_EVENTS {
                        return Err(serde::de::Error::custom("trace event admission"));
                    }
                    events.push(event);
                }
                Ok(BoundedEvents(events))
            }
        }
        deserializer.deserialize_seq(Visitor)
    }
}

#[derive(Debug, Serialize)]
pub struct StrictNodeTrace {
    pub schema_version: u16,
    pub provider: InferenceProvider,
    pub runs: u64,
    pub computation_events: u64,
    pub transfer_events: u64,
    pub trace_sha256: String,
}

#[derive(Clone, Copy)]
struct Span {
    process: u64,
    thread: u64,
    start: u64,
    end: u64,
}
fn integer(event: &Value, key: &str) -> Result<u64> {
    event
        .get(key)
        .and_then(Value::as_u64)
        .with_context(|| format!("invalid trace {key}"))
}
fn span(event: &Value) -> Result<Span> {
    ensure!(
        event.get("ph").and_then(Value::as_str) == Some("X"),
        "unsupported trace phase"
    );
    let start = integer(event, "ts")?;
    let end = start
        .checked_add(integer(event, "dur")?)
        .context("trace clock overflow")?;
    Ok(Span {
        process: integer(event, "pid")?,
        thread: integer(event, "tid")?,
        start,
        end,
    })
}

/// Validate sequential model-run containment and every observed computation provider.
/// Callers must independently bind trace/model/runtime/attempt bytes and actual run count.
/// Parsed JSON cannot authenticate native execution or establish a complete runtime lock.
pub fn validate_node_trace(
    bytes: &[u8],
    provider: InferenceProvider,
    expected_runs: u64,
) -> Result<StrictNodeTrace> {
    use sha2::{Digest, Sha256};
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_TRACE_BYTES,
        "trace byte admission"
    );
    ensure!(
        (1..=MAX_RUNS).contains(&expected_runs),
        "trace run admission"
    );
    let expected = match provider {
        InferenceProvider::Cpu => "CPUExecutionProvider",
        InferenceProvider::Cuda => "CUDAExecutionProvider",
        _ => anyhow::bail!("unsupported strict trace provider"),
    };
    let events: BoundedEvents = serde_json::from_slice(bytes).context("invalid node trace JSON")?;
    ensure!(!events.0.is_empty(), "trace event admission");
    let mut runs: BTreeMap<(u64, u64), Vec<Span>> = BTreeMap::new();
    let mut nodes = Vec::new();
    let mut run_count = 0u64;
    for event in &events.0 {
        ensure!(event.is_object(), "trace event must be an object");
        let category = event
            .get("cat")
            .and_then(Value::as_str)
            .context("missing trace category")?;
        let name = event
            .get("name")
            .and_then(Value::as_str)
            .context("missing trace name")?;
        ensure!(
            !name.is_empty() && name.len() <= 1024,
            "trace name admission"
        );
        match category {
            "Session" => {
                let interval = span(event)?;
                if name == "model_run" {
                    ensure!(interval.start < interval.end, "empty model-run interval");
                    run_count += 1;
                    ensure!(run_count <= expected_runs, "unexpected trace model run");
                    runs.entry((interval.process, interval.thread))
                        .or_default()
                        .push(interval);
                }
            }
            "Node" => {
                ensure!(
                    name.ends_with("_kernel_time"),
                    "unsupported node trace event"
                );
                let interval = span(event)?;
                let args = event
                    .get("args")
                    .and_then(Value::as_object)
                    .context("missing node arguments")?;
                ensure!(
                    args.get("provider").and_then(Value::as_str) == Some(expected),
                    "strict computation provider violated"
                );
                let operator = args
                    .get("op_name")
                    .and_then(Value::as_str)
                    .context("missing node operator")?;
                ensure!(
                    !operator.is_empty() && operator.len() <= 256,
                    "node operator admission"
                );
                let transfer = matches!(operator, "MemcpyFromHost" | "MemcpyToHost");
                nodes.push((interval, transfer));
            }
            _ => anyhow::bail!("unsupported trace event category"),
        }
    }
    ensure!(run_count == expected_runs, "trace model-run count differs");
    let mut hits: BTreeMap<(u64, u64), Vec<u64>> = BTreeMap::new();
    for (key, intervals) in &mut runs {
        intervals.sort_by_key(|interval| interval.start);
        ensure!(
            intervals
                .windows(2)
                .all(|pair| pair[0].end <= pair[1].start),
            "overlapping trace model runs"
        );
        hits.insert(*key, vec![0; intervals.len()]);
    }
    let (mut computation_events, mut transfer_events) = (0, 0);
    for (node, transfer) in nodes {
        let key = (node.process, node.thread);
        let intervals = runs
            .get(&key)
            .context("node has no matching process/thread model run")?;
        let index = intervals
            .partition_point(|interval| interval.start <= node.start)
            .checked_sub(1)
            .context("node precedes model run")?;
        let interval = intervals[index];
        ensure!(
            node.start < interval.end && node.end <= interval.end,
            "node escapes model run"
        );
        if transfer {
            transfer_events += 1;
        } else {
            computation_events += 1;
            hits.get_mut(&key).context("missing run counters")?[index] += 1;
        }
    }
    ensure!(
        hits.values().flatten().all(|count| *count > 0),
        "model run has no observed computation"
    );
    Ok(StrictNodeTrace {
        schema_version: 1,
        provider,
        runs: run_count,
        computation_events,
        transfer_events,
        trace_sha256: format!("{:x}", Sha256::digest(bytes)),
    })
}

/// No execution claim: only bounded native session initialization events.
/// Kept separate from positive-run provider evidence so generation cannot use it.
pub(crate) fn validate_initialization_trace(bytes: &[u8]) -> Result<StrictNodeTrace> {
    use sha2::{Digest, Sha256};
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_TRACE_BYTES,
        "trace byte admission"
    );
    let events: BoundedEvents =
        serde_json::from_slice(bytes).context("invalid initialization trace JSON")?;
    ensure!(!events.0.is_empty(), "initialization trace is absent");
    let mut initialized = false;
    for event in &events.0 {
        ensure!(
            event.get("cat").and_then(Value::as_str) == Some("Session"),
            "no-inference trace contains non-session events"
        );
        let name = event
            .get("name")
            .and_then(Value::as_str)
            .context("missing initialization trace name")?;
        ensure!(
            matches!(
                name,
                "model_loading_uri" | "model_loading_array" | "session_initialization"
            ),
            "no-inference trace contains execution or unknown event"
        );
        span(event)?;
        if name == "session_initialization" {
            initialized = true;
        }
    }
    ensure!(initialized, "native session initialization is absent");
    Ok(StrictNodeTrace {
        schema_version: 2,
        provider: InferenceProvider::Cuda,
        runs: 0,
        computation_events: 0,
        transfer_events: 0,
        trace_sha256: format!("{:x}", Sha256::digest(bytes)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> Value {
        json!([
            {"cat":"Node","name":"Conv_0_kernel_time","ph":"X","ts":11,"dur":2,"pid":1,"tid":2,"args":{"provider":"CUDAExecutionProvider","op_name":"Conv"}},
            {"cat":"Session","name":"model_run","ph":"X","ts":10,"dur":10,"pid":1,"tid":2},
            {"cat":"Node","name":"Copy_1_kernel_time","ph":"X","ts":15,"dur":1,"pid":1,"tid":2,"args":{"provider":"CUDAExecutionProvider","op_name":"MemcpyToHost"}}
        ])
    }
    fn check(value: &Value) -> Result<StrictNodeTrace> {
        validate_node_trace(&serde_json::to_vec(value)?, InferenceProvider::Cuda, 1)
    }
    #[test]
    fn initialization_only_trace_cannot_be_positive_run_evidence() {
        let init = json!([{"cat":"Session","name":"session_initialization","ph":"X","ts":1,"dur":5,"pid":1,"tid":2}]);
        let bytes = serde_json::to_vec(&init).unwrap();
        assert_eq!(validate_initialization_trace(&bytes).unwrap().runs, 0);
        assert!(validate_node_trace(&bytes, InferenceProvider::Cuda, 0).is_err());
        assert!(validate_node_trace(&bytes, InferenceProvider::Cuda, 1).is_err());
        assert!(validate_initialization_trace(b"[]").is_err());
        for name in ["model_run", "SequentialExecutor::Execute", "unknown"] {
            let mut changed = init.clone();
            changed[0]["name"] = name.into();
            assert!(validate_initialization_trace(&serde_json::to_vec(&changed).unwrap()).is_err());
        }
        for category in ["Node", "Unknown"] {
            let mut changed = init.clone();
            changed[0]["cat"] = category.into();
            assert!(validate_initialization_trace(&serde_json::to_vec(&changed).unwrap()).is_err());
        }
        assert!(validate_initialization_trace(&serde_json::to_vec(&fixture()).unwrap()).is_err());
    }
    #[test]
    fn checks_provider_run_containment_and_separate_transfers() {
        let result = check(&fixture()).unwrap();
        assert_eq!(
            (
                result.runs,
                result.computation_events,
                result.transfer_events
            ),
            (1, 1, 1)
        );
        assert!(validate_node_trace(
            &serde_json::to_vec(&fixture()).unwrap(),
            InferenceProvider::Cpu,
            1
        )
        .is_err());
        let mut changed = fixture();
        changed[0]["args"]["provider"] = json!("CPUExecutionProvider");
        assert!(check(&changed).is_err());
        let mut changed = fixture();
        changed[0]["args"]["op_name"] = json!("MemcpyFromHost");
        assert!(check(&changed).is_err());
        for (key, value) in [
            ("pid", json!(3)),
            ("tid", json!(3)),
            ("ts", json!(9)),
            ("dur", json!(11)),
            ("ts", json!(20)),
            ("ts", json!(false)),
            ("dur", json!(-1)),
            ("ph", json!("B")),
        ] {
            let mut changed = fixture();
            changed[0][key] = value;
            assert!(check(&changed).is_err(), "{key}");
        }
    }
    #[test]
    fn refuses_missing_overlapping_extra_runs_and_unknown_nodes() {
        let mut changed = fixture();
        changed.as_array_mut().unwrap().remove(1);
        assert!(check(&changed).is_err());
        let mut changed = fixture();
        let duplicate = changed[1].clone();
        changed.as_array_mut().unwrap().push(duplicate);
        assert!(validate_node_trace(
            &serde_json::to_vec(&changed).unwrap(),
            InferenceProvider::Cuda,
            2
        )
        .is_err());
        for replacement in [
            json!(null),
            json!({}),
            json!({"cat":"Unknown","name":"x"}),
            json!({"cat":"Node","name":"unknown"}),
        ] {
            let mut changed = fixture();
            changed[0] = replacement;
            assert!(check(&changed).is_err());
        }
        for runs in [0, 2, MAX_RUNS + 1] {
            assert!(validate_node_trace(
                &serde_json::to_vec(&fixture()).unwrap(),
                InferenceProvider::Cuda,
                runs
            )
            .is_err());
        }
        assert!(validate_node_trace(b"[]", InferenceProvider::Cuda, 1).is_err());
        assert!(
            validate_node_trace(&vec![b' '; MAX_TRACE_BYTES + 1], InferenceProvider::Cuda, 1)
                .is_err()
        );
        assert!(validate_node_trace(b"[]", InferenceProvider::TensorRt, 1).is_err());
        assert!(validate_node_trace(b"{}", InferenceProvider::Cuda, 1).is_err());
        let mut changed = fixture();
        changed[0]["ts"] = json!(u64::MAX);
        assert!(check(&changed).is_err());
        let too_many = serde_json::to_vec(&vec![Value::Null; MAX_EVENTS + 1]).unwrap();
        assert!(validate_node_trace(&too_many, InferenceProvider::Cuda, 1)
            .unwrap_err()
            .to_string()
            .contains("invalid node trace JSON"));
    }
}
