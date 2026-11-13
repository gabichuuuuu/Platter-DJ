//! An allocator that counts what the current thread allocates and frees.
//!
//! A test binary installs it with `#[global_allocator]` and wraps the code under test in
//! [`count`]. Real-time code must report zero.
//!
//! This module is the single place in the workspace where `unsafe` is allowed: implementing
//! `GlobalAlloc` requires it. Every method forwards to the system allocator unchanged.

#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    // Constant initialisers and no destructors: reading these never allocates, which matters
    // because they are read from inside the allocator.
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static EVENTS: Cell<usize> = const { Cell::new(0) };
}

/// Forwards to the system allocator and counts calls made while [`count`] is running on the
/// calling thread.
pub struct CountingAllocator;

fn record() {
    // `try_with` because a thread may allocate while its thread-locals are being torn down.
    let _ = COUNTING.try_with(|counting| {
        if counting.get() {
            let _ = EVENTS.try_with(|events| events.set(events.get() + 1));
        }
    });
}

// SAFETY: every method forwards its arguments unchanged to `System`, which upholds the
// contract of `GlobalAlloc`; the counting touches only thread-local cells.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record();
        // SAFETY: the caller upholds the contract of `GlobalAlloc::alloc`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        record();
        // SAFETY: the caller upholds the contract of `GlobalAlloc::dealloc`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record();
        // SAFETY: the caller upholds the contract of `GlobalAlloc::alloc_zeroed`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record();
        // SAFETY: the caller upholds the contract of `GlobalAlloc::realloc`.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// Runs `work` and returns its result with the number of allocations, reallocations and
/// frees the calling thread made meanwhile. Without [`CountingAllocator`] installed as the
/// global allocator the count is always zero, so a test should first check that it can see
/// an allocation.
pub fn count<R>(work: impl FnOnce() -> R) -> (R, usize) {
    EVENTS.with(|events| events.set(0));
    COUNTING.with(|counting| counting.set(true));
    let result = work();
    COUNTING.with(|counting| counting.set(false));
    (result, EVENTS.with(Cell::get))
}
