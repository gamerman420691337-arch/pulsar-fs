//! Profile RAM sizing within owner limits; output/CPU/time classes stay explicit.
use pulsar_protocol::*;

pub(crate) fn admit(
    baseline: ResourceBudget,
    profile: Option<&DeclaredInferenceProfile>,
    retained: Option<&ResourceBudget>,
    owner_memory_bytes: u64,
) -> Result<ResourceBudget, ProtocolError> {
    let mut budget = retained.cloned().unwrap_or(baseline);
    if retained.is_none() {
        if let Some(profile) = profile {
            budget.memory_bytes = budget.memory_bytes.max(profile.minimum_memory_bytes);
        }
    }
    budget.validate()?;
    if budget.memory_bytes > owner_memory_bytes {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "inference RAM exceeds owner pool",
        ));
    }
    if let Some(profile) = profile {
        profile.admit(&budget)?;
    }
    Ok(budget)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn baseline() -> ResourceBudget {
        ResourceBudget {
            memory_bytes: 1536 << 20,
            output_bytes: 64 << 20,
            wall_time_ms: 12345,
            cpu_threads: 2,
        }
    }
    fn profile(provider: InferenceProvider) -> DeclaredInferenceProfile {
        let request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        let WorkerOperation::Generate { settings, .. } = request.operation else {
            panic!("generate fixture")
        };
        let mut profile = settings.inference_profile.unwrap();
        profile.minimum_memory_bytes = 3 << 30;
        if provider == InferenceProvider::Cpu {
            profile.provider = provider;
            profile.schema_version = 1;
            profile.cuda_resources = None;
        }
        profile
    }
    #[test]
    fn cpu_and_cuda_ram_claims_grow_without_changing_other_limits() {
        for provider in [InferenceProvider::Cpu, InferenceProvider::Cuda] {
            let p = profile(provider);
            let actual = admit(baseline(), Some(&p), None, 4 << 30).unwrap();
            assert_eq!(actual.memory_bytes, 3 << 30);
            assert_eq!(actual.output_bytes, 64 << 20);
            assert_eq!(actual.cpu_threads, 2);
            assert_eq!(actual.wall_time_ms, 12345);
        }
        assert_eq!(
            serde_json::to_value(admit(baseline(), None, None, 4 << 30).unwrap()).unwrap(),
            serde_json::to_value(baseline()).unwrap()
        );
    }
    #[test]
    fn owner_output_and_cpu_limits_remain_strict() {
        let mut p = profile(InferenceProvider::Cpu);
        assert_eq!(
            admit(baseline(), Some(&p), None, 2 << 30).unwrap_err().code,
            ErrorCode::ResourceExhausted
        );
        p.minimum_output_bytes = (64 << 20) + 1;
        assert_eq!(
            admit(baseline(), Some(&p), None, 4 << 30).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        p.minimum_output_bytes = 1;
        p.cpu_threads = 3;
        assert_eq!(
            admit(baseline(), Some(&p), None, 4 << 30).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
    #[test]
    fn retained_budget_is_exact_and_cannot_be_repaired_or_shrunk_on_resume() {
        let p = profile(InferenceProvider::Cpu);
        let mut retained = baseline();
        retained.memory_bytes = 3 << 30;
        retained.wall_time_ms = 54321;
        let actual = admit(baseline(), Some(&p), Some(&retained), 4 << 30).unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(&retained).unwrap()
        );
        assert_eq!(
            admit(baseline(), Some(&p), Some(&retained), 2 << 30)
                .unwrap_err()
                .code,
            ErrorCode::ResourceExhausted
        );
        retained.memory_bytes = 1536 << 20;
        assert_eq!(
            admit(baseline(), Some(&p), Some(&retained), 4 << 30)
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    #[test]
    fn aggregate_pool_still_refuses_overlapping_claims_and_releases_them() {
        let pool = crate::resources::ResourcePool::new(4096, 2);
        let first = pool.reserve(3000).unwrap();
        assert!(pool.reserve(1500).is_err());
        drop(first);
        assert!(pool.reserve(1500).is_ok());
    }
}
