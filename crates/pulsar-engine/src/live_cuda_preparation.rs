//! Trusted host preparation, independently bounded from any live capture.
//! Holding verified bytes supplies no session, model, device or launch authority.
use super::*;
use crate::native_capture::SupervisedNativeDependencies;
use std::sync::atomic::AtomicUsize;
use std::time::{Duration, Instant};
const RETENTION_MEMORY: u64 = 512 * 1024;

pub(crate) struct RetentionAdmission {
    count: Arc<AtomicUsize>,
    _memory: Reservation,
}
impl RetentionAdmission {
    pub(crate) fn reserve(pool: &ResourcePool, count: Arc<AtomicUsize>) -> PResult<Self> {
        let mut observed = count.load(Ordering::Acquire);
        loop {
            if observed >= 8 {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "native retention slots exhausted",
                ));
            }
            match count.compare_exchange_weak(
                observed,
                observed + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(actual) => observed = actual,
            }
        }
        match pool.reserve_memory(RETENTION_MEMORY) {
            Ok(memory) => Ok(Self {
                count,
                _memory: memory,
            }),
            Err(error) => {
                count.fetch_sub(1, Ordering::AcqRel);
                Err(error)
            }
        }
    }
}
impl Drop for RetentionAdmission {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::AcqRel);
    }
}

pub type LiveCudaImagePreparationOptions = NativeImagePreparationOptions;

struct RetainedImage {
    image: Mutex<Option<SupervisedNativeDependencies>>,
    expires: Instant,
    started: Instant,
}
impl RetainedImage {
    fn take(&self) -> Option<SupervisedNativeDependencies> {
        self.image.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

/// An owned preparation use. Expiry or release removes this use, while workers
/// borrowing the same image retain its storage until their own retirement.
/// This is a trusted embedding API, not a client permission or readiness proof.
pub struct PreparedLiveCudaImage {
    owner: Arc<RetainedImage>,
    engine_epoch: AttemptId,
    image_sha256: String,
    declared_bytes: u64,
}
impl PreparedLiveCudaImage {
    pub(crate) fn retain(
        image: SupervisedNativeDependencies,
        engine_epoch: AttemptId,
        retention_ms: u64,
        admission: RetentionAdmission,
    ) -> PResult<Self> {
        if !(1..=3_600_000).contains(&retention_ms) {
            return Err(ProtocolError::invalid(
                "native image retention must be 1..=3600000 ms",
            ));
        }
        let spec = image.captured().specification();
        let declared_bytes = spec.validate().map_err(unavailable)?;
        let image_sha256 = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(spec).map_err(internal)?)
        );
        let started = Instant::now();
        let expires = started
            .checked_add(Duration::from_millis(retention_ms))
            .ok_or_else(|| ProtocolError::invalid("native image retention deadline overflow"))?;
        let owner = Arc::new(RetainedImage {
            image: Mutex::new(Some(image)),
            expires,
            started,
        });
        let weak = Arc::downgrade(&owner);
        std::thread::Builder::new()
            .name("pulsar-image-retention".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                let _admission = admission;
                loop {
                    let Some(owner) = weak.upgrade() else {
                        return;
                    };
                    if Instant::now() >= owner.expires {
                        // Cleanup executes outside the image mutex. Its existing
                        // staging obligation retains storage until confirmed deletion.
                        drop(owner.take());
                        return;
                    }
                    let remaining = owner.expires.saturating_duration_since(Instant::now());
                    drop(owner);
                    std::thread::sleep(remaining.min(Duration::from_millis(25)));
                }
            })
            .map_err(unavailable)?;
        Ok(Self {
            owner,
            engine_epoch,
            image_sha256,
            declared_bytes,
        })
    }
    pub(crate) fn retained_now(&self) -> bool {
        Instant::now() < self.owner.expires
            && self
                .owner
                .image
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_some()
    }
    pub(crate) fn retention_started(&self) -> Instant {
        self.owner.started
    }
    pub fn engine_epoch(&self) -> &AttemptId {
        &self.engine_epoch
    }
    pub fn image_sha256(&self) -> &str {
        &self.image_sha256
    }
    pub fn declared_bytes(&self) -> u64 {
        self.declared_bytes
    }
    /// Availability of this retained use, not inference or device readiness.
    pub fn available(&self) -> bool {
        if Instant::now() >= self.owner.expires {
            drop(self.owner.take());
            return false;
        }
        self.owner
            .image
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }
    /// Return whether this call removes the retained use. When it removes the
    /// final image use, wait for checked cleanup. An already-expired use or
    /// other borrowers can still have cleanup obligations; success alone does
    /// not prove physical deletion. An error never frees an unconfirmed claim.
    pub fn release(self) -> PResult<bool> {
        match self.owner.take() {
            Some(image) => image.close().map(|()| true).map_err(unavailable),
            None => Ok(false),
        }
    }
}

impl Engine {
    /// Explicit trusted-host work before capture. RPC callers cannot invoke this
    /// method; authenticated RPC preparation is owned by its separate controller.
    pub fn prepare_live_cuda_image(
        self: &Arc<Self>,
        options: LiveCudaImagePreparationOptions,
        cancel: &AtomicBool,
    ) -> PResult<PreparedLiveCudaImage> {
        options.validate()?;
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(options.preparation_ms))
            .ok_or_else(|| ProtocolError::invalid("native preparation deadline overflow"))?;
        self.prepare_live_cuda_image_in_lease(options, cancel, deadline)
    }
    pub(crate) fn prepare_live_cuda_image_in_lease(
        self: &Arc<Self>,
        options: LiveCudaImagePreparationOptions,
        cancel: &AtomicBool,
        deadline: Instant,
    ) -> PResult<PreparedLiveCudaImage> {
        if Instant::now() >= deadline {
            return Err(unavailable("native preparation deadline expired"));
        }
        if !cfg!(feature = "cuda") {
            return Err(ProtocolError::unsupported(
                "native image preparation requires CUDA",
            ));
        }
        options.validate()?;
        if Duration::from_millis(options.preparation_ms)
            > self.config.native_image_preparation_timeout
        {
            return Err(ProtocolError::invalid(
                "native preparation exceeds its independent owner limits",
            ));
        }
        if cancel.load(Ordering::Acquire) {
            return Err(ProtocolError::new(
                ErrorCode::Cancelled,
                "native image preparation cancelled",
            ));
        }
        let owner = self.cuda_owner_policy.as_ref().ok_or_else(|| {
            ProtocolError::unsupported("native preparation requires an owner policy")
        })?;
        let admission =
            RetentionAdmission::reserve(&self.pool, self.native_retention_slots.clone())?;
        let spec = &owner.policy().native_dependencies;
        let total = spec.validate().map_err(unavailable)?;
        {
            let mut db = self.db.lock().map_err(internal)?;
            let tx = db.transaction().map_err(internal)?;
            self.arm_worker_fence(&tx)?;
            tx.commit().map_err(internal)?;
        }
        let image = self
            .native_image_reuse
            .capture_with(spec, cancel, deadline, || {
                crate::native_capture::capture_tracked_in_lease(
                    &self.config,
                    &self.pool,
                    spec,
                    crate::native_capture::CaptureLimits {
                        budget: crate::native_capture::preparation_budget(
                            total,
                            options.preparation_ms,
                        )?,
                        address_space_bytes: 1536 << 20,
                    },
                    cancel,
                    self.worker_fence.clone(),
                    deadline,
                )
            })?;
        if Instant::now() >= deadline || cancel.load(Ordering::Acquire) {
            return Err(unavailable(
                "native preparation completed after deadline or cancellation",
            ));
        }
        let prepared = PreparedLiveCudaImage::retain(
            image,
            AttemptId::new(self.engine_epoch.clone()).map_err(internal)?,
            options.retention_ms,
            admission,
        )?;
        if Instant::now() >= deadline || cancel.load(Ordering::Acquire) {
            return Err(unavailable(
                "native retention publication exceeded preparation deadline or cancellation",
            ));
        }
        Ok(prepared)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retention_admission_bounds_slots_and_rolls_back_memory_refusal() {
        let count = Arc::new(AtomicUsize::new(0));
        let too_small = ResourcePool::new(RETENTION_MEMORY - 1, 1);
        assert!(RetentionAdmission::reserve(&too_small, count.clone()).is_err());
        assert_eq!(count.load(Ordering::Acquire), 0);
        let pool = ResourcePool::new(8 * RETENTION_MEMORY, 1);
        let mut held = Vec::new();
        for _ in 0..8 {
            held.push(RetentionAdmission::reserve(&pool, count.clone()).unwrap());
        }
        assert_eq!(count.load(Ordering::Acquire), 8);
        assert!(RetentionAdmission::reserve(&pool, count.clone()).is_err());
        assert_eq!(count.load(Ordering::Acquire), 8);
        assert!(pool.reserve_memory(1).is_err());
        drop(held);
        assert_eq!(count.load(Ordering::Acquire), 0);
        assert!(pool.reserve_memory(8 * RETENTION_MEMORY).is_ok());
    }
}
