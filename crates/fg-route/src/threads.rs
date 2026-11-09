//! The planner's threads give their caches back when they end.
//!
//! In the browser (wasm) a thread's locals are never dropped: the standard library has no thread
//! exit there and leaks them. Each plan starts new threads, so their walk caches (up to some
//! 100 MB a thread) stayed for good, and the web app ran out of its 4 GiB after a few guides
//! (GitHub #132, #156). Natively they go a moment earlier than with the thread.

/// Empties this thread's caches when dropped (at the end of a thread's work, panic included).
pub(crate) struct Release;

impl Drop for Release {
    fn drop(&mut self) {
        release();
    }
}

/// Gives back the memory of this thread's caches.
pub(crate) fn release() {
    crate::world::release_thread_caches();
    crate::terrain::release_thread_caches();
}

#[cfg(test)]
mod tests {
    /// A thread's work ends with its caches empty, their memory given back.
    #[test]
    fn a_thread_ends_with_its_caches_given_back() {
        let sizes = std::thread::spawn(|| {
            crate::world::fill_thread_caches_for_tests();
            crate::terrain::fill_thread_caches_for_tests();
            let filled = (
                crate::world::thread_cache_capacity(),
                crate::terrain::thread_cache_capacity(),
            );
            drop(super::Release);
            (
                filled,
                (
                    crate::world::thread_cache_capacity(),
                    crate::terrain::thread_cache_capacity(),
                ),
            )
        })
        .join()
        .unwrap();
        let ((world, terrain), after) = sizes;
        assert!(world > 0 && terrain > 0, "filled: {world} {terrain}");
        assert_eq!(after, (0, 0));
    }
}
