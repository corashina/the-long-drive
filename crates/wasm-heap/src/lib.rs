//! The global allocator of every browser WASM module. Link it with `use wasm_heap as _;`.
//!
//! DLmalloc and the default talc source grow the memory by the size of each request.
//! A browser `memory.grow` costs more as the memory gets bigger. The reference parse
//! makes many small allocations, so those grows took most of a 10 s parse.
//! This source grows by half the current memory at least, so a large heap needs few grows.
#![no_std]

#[cfg(target_arch = "wasm32")]
mod heap {
    use core::{alloc::Layout, arch::wasm32, ptr::NonNull};
    use talc::{base::Talc, base::binning::Binning, cell::TalcSyncCell, wasm::WasmBinning};

    const PAGE: usize = 64 * 1024;

    #[global_allocator]
    static ALLOCATOR: TalcSyncCell<Grow, WasmBinning> = TalcSyncCell::new_wasm(Grow { end: None });

    /// Grows the WASM memory and extends the one heap at its end.
    #[derive(Debug)]
    pub struct Grow {
        end: Option<NonNull<u8>>,
    }

    // SAFETY: it does not allocate. It only claims or extends memory that `memory.grow` returns.
    unsafe impl talc::source::Source for Grow {
        fn acquire<B: Binning>(talc: &mut Talc<Self, B>, layout: Layout) -> Result<(), ()> {
            // Chunk overhead and alignment padding fit in the extra 1 KiB.
            let needed = (layout.size() + layout.align() + 1024).div_ceil(PAGE);
            let step = needed.max(wasm32::memory_size(0) / 2);
            // Near the memory limit, grow only by what this request needs.
            let (previous, pages) = match wasm32::memory_grow(0, step) {
                usize::MAX => match wasm32::memory_grow(0, needed) {
                    usize::MAX => return Err(()),
                    previous => (previous, needed),
                },
                previous => (previous, step),
            };
            let base = (previous * PAGE) as *mut u8;
            let end = base.wrapping_add(pages * PAGE);
            if let Some(old) = talc.source.end.take() {
                if old.as_ptr() == base {
                    // SAFETY: the new pages start at the old heap end.
                    talc.source.end = Some(unsafe { talc.extend(old, end) });
                    return Ok(());
                }
            }
            // SAFETY: the grown pages belong to no other heap.
            talc.source.end = unsafe { talc.claim(base, pages * PAGE) };
            Ok(())
        }
    }
}
