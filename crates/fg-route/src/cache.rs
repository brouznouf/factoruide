//! Results worth keeping: a computation (`Compute`) and the same one keeping its results
//! (`Cached`), for what is asked for again with the same inputs (the walks between the passages
//! of a world, its boats' chain tables...).

use std::convert::Infallible;

/// A computation whose result depends only on what its key says: the same key, the same result.
pub trait Compute<I: ?Sized> {
    /// Cheap to copy (an `Arc`): a kept result is handed out again and again.
    type Output: Clone;
    /// What the result depends on; None when it is not to be kept.
    fn key(&self, input: &I) -> Option<u64>;
    fn compute(&self, input: &I) -> Self::Output;
}

/// `S` keeping its last results in memory, `capacity` of them: an input of a known key gets the
/// result computed for it, and inputs of the same key asked for at once are computed once (the
/// others wait for it).
pub struct Cached<S, O> {
    inner: S,
    kept: quick_cache::sync::Cache<u64, O>,
}

impl<S, O: Clone> Cached<S, O> {
    pub fn new(inner: S, capacity: usize) -> Self {
        Self {
            inner,
            kept: quick_cache::sync::Cache::new(capacity),
        }
    }
}

impl<I: ?Sized, S: Compute<I>> Compute<I> for Cached<S, S::Output> {
    type Output = S::Output;

    fn key(&self, input: &I) -> Option<u64> {
        self.inner.key(input)
    }

    fn compute(&self, input: &I) -> S::Output {
        let Some(key) = self.inner.key(input) else {
            return self.inner.compute(input);
        };
        let computed = self
            .kept
            .get_or_insert_with(&key, || Ok::<_, Infallible>(self.inner.compute(input)));
        match computed {
            Ok(output) => output,
            Err(never) => match never {},
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Cached, Compute};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The square of a number, counting its computations; odd numbers are not to be kept.
    #[derive(Default)]
    struct Square(AtomicUsize);

    impl Compute<u64> for Square {
        type Output = u64;
        fn key(&self, n: &u64) -> Option<u64> {
            n.is_multiple_of(2).then_some(*n)
        }
        fn compute(&self, n: &u64) -> u64 {
            self.0.fetch_add(1, Ordering::Relaxed);
            n * n
        }
    }

    #[test]
    fn a_result_is_computed_once_per_key_and_not_kept_without_one() {
        let cached = Cached::new(Square::default(), 4);
        assert_eq!((cached.compute(&4), cached.compute(&4)), (16, 16));
        assert_eq!(cached.inner.0.load(Ordering::Relaxed), 1);
        assert_eq!((cached.compute(&3), cached.compute(&3)), (9, 9));
        assert_eq!(cached.inner.0.load(Ordering::Relaxed), 3);
    }

    /// Threads asking for the same key at once: one computes, the others get its result.
    #[test]
    fn the_same_key_asked_at_once_is_computed_once() {
        let cached = Cached::new(Square::default(), 4);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| assert_eq!(cached.compute(&6), 36));
            }
        });
        assert_eq!(cached.inner.0.load(Ordering::Relaxed), 1);
    }
}
