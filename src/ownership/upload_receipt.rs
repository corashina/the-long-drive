use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

// Low bits preserve the original queued, registered, and complete receipt states.
const RECEIPT_STATE: u8 = 3;
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const STOPPED: u8 = 4;
/// Shared by material forks, CPU accounting, and the render queue callback.
#[derive(Clone, Default)]
pub struct UploadReceipt(Arc<AtomicU8>);

impl UploadReceipt {
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn owners(&self) -> usize {
        Arc::strong_count(&self.0)
    }
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn allocation_bytes() -> usize {
        std::alloc::Layout::new::<[usize; 2]>()
            .extend(std::alloc::Layout::new::<AtomicU8>())
            .unwrap()
            .0
            .pad_to_align()
            .size()
    }

    pub fn cpu_bytes(&self, reservation: usize, started: bool) -> usize {
        if started && self.0.load(Ordering::Acquire) & RECEIPT_STATE != 2 {
            reservation
        } else {
            0
        }
    }

    pub fn completed(&self) -> bool {
        self.0.load(Ordering::Acquire) & RECEIPT_STATE == 2
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn stop(&self) {
        self.0.fetch_or(STOPPED, Ordering::AcqRel);
    }
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn stopped(&self) -> bool {
        self.0.load(Ordering::Acquire) & STOPPED != 0
    }
    pub fn register(&self) -> bool {
        self.0
            .try_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                (state & RECEIPT_STATE == 0).then_some(state | 1)
            })
            .is_ok()
    }

    pub fn complete(&self) {
        self.0
            .try_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                Some((state & !RECEIPT_STATE) | 2)
            })
            .unwrap();
    }
}
