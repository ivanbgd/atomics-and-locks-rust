//! # Chapter 4: Building Our Own Spin Lock
//!
//! https://marabos.nl/atomics/building-spinlock.html
//!
//! See [`run_example2()`] for some performance measurements and comparison with our implementations of mutex.

use std::ops::{Deref, DerefMut};
use std::thread::JoinHandle;
use std::time::Instant;
use std::{
    cell::UnsafeCell,
    sync::{
        atomic::{Ordering::*, *},
        *,
    },
    thread,
};

#[derive(Debug)]
pub struct SpinLock<T> {
    locked: AtomicBool,
    // [`UnsafeCell`] is required for interior mutability.
    value: UnsafeCell<T>,
}

// SAFETY: All interior mutations are synchronized properly (by spinlock and memory orderings).
unsafe impl<T> Sync for SpinLock<T> where T: Send {}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        // Works incorrectly if we use `Relaxed` instead of `Acquire` for the success case.
        // We could use `swap()` instead of `compare_exchange`.
        while self
            .locked
            .compare_exchange_weak(false, true, Acquire, Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }

        SpinLockGuard { lock: self }
    }
}

#[derive(Debug)]
pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
}

unsafe impl<T> Send for SpinLockGuard<'_, T> where T: Send {}
unsafe impl<T> Sync for SpinLockGuard<'_, T> where T: Sync {}

impl<T> Drop for SpinLockGuard<'_, T> {
    fn drop(&mut self) {
        // Might work incorrectly if we use `Relaxed` instead of `Release`.
        self.lock.locked.store(false, Release);
    }
}

impl<T> Deref for SpinLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: Existence of guard means that we have exclusively locked the lock.
        unsafe { &*self.lock.value.get() }
    }
}

impl<T> DerefMut for SpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: Existence of guard means that we have exclusively locked the lock.
        unsafe { &mut *self.lock.value.get() }
    }
}

pub fn run_example1() -> i32 {
    let counter = SpinLock::new(0);

    thread::scope(|s| {
        s.spawn(|| {
            let mut guard = counter.lock();
            *guard += 1;
        });

        s.spawn(|| {
            *counter.lock() += 1;
        });
    });

    let result = *counter.lock();
    assert_eq!(2, result);
    println!("Total 1: {}", result);

    result
}

/// A test with lots of contention, with multiple threads repeatedly trying to lock an already locked spin-lock.
///
/// Note that this is an extreme and unrealistic scenario.
/// The spin-lock is only kept for an extremely short time (only to increment an integer),
/// and the threads will immediately attempt to lock the spin-lock again after unlocking.
/// A different scenario will most likely result in very different results.
pub fn run_example2() -> i32 {
    let counter = Arc::new(SpinLock::new(0));
    std::hint::black_box(&counter); // Doesn't affect performance (on Apple M2 Pro).

    let start = Instant::now();

    let handles: [JoinHandle<()>; 4] = std::array::from_fn(|_| {
        let counter = Arc::clone(&counter);
        thread::spawn(move || {
            for _ in 0..1_000_000 {
                *counter.lock() += 1;
            }
        })
    });

    for h in handles {
        h.join().unwrap();
    }

    // ~350 ms on Apple M2 Pro; it takes ~900 ms for ch9_locks::mutex_1.rs,
    // but ~100 ms for ch9_locks::mutex_2.rs and ~100 ms for ch9_locks::mutex_3.rs
    let elapsed = start.elapsed();

    let result = counter.lock();
    assert_eq!(4 * 1_000_000, *result);
    println!(
        "Total 2: {}; locked state = {:?}; elapsed = {:.3?}",
        *result, result.lock.locked, elapsed
    );

    *result
}

pub fn run_example3() {
    let vec = SpinLock::new(Vec::new());

    thread::scope(|s| {
        s.spawn(|| vec.lock().push(1));
    });

    thread::scope(|s| {
        s.spawn(|| {
            let mut guard = vec.lock();
            guard.push(2);
            guard.push(2);
        });
    });

    let guard = vec.lock();
    assert!(guard.as_slice() == [1, 2, 2] || guard.as_slice() == [2, 2, 1]);
    println!(
        "vec = {:?}; locked state = {:?}",
        guard.as_slice(),
        guard.lock.locked
    );
}

/// Like [`run_example2()`] but without `Arc` protecting the `SpinLock`. The speed is the same.
pub fn run_example4() -> i32 {
    let counter = SpinLock::new(0);
    std::hint::black_box(&counter); // Doesn't affect performance (on Apple M2 Pro).

    let start = Instant::now();

    thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| {
                for _ in 0..1_000_000 {
                    *counter.lock() += 1;
                }
            });
        }
    });

    // ~350 ms on Apple M2 Pro; it takes ~900 ms for ch9_locks::mutex_1.rs,
    // but ~90 ms for ch9_locks::mutex_2.rs and ~90 ms for ch9_locks::mutex_3.rs
    let elapsed = start.elapsed();

    let result = counter.lock();
    assert_eq!(4 * 1_000_000, *result);
    println!(
        "Total 4: {}; locked state = {:?}; elapsed = {:.3?}",
        *result, result.lock.locked, elapsed
    );

    *result
}

/// Single thread.
/// This is a test for the trivial uncontended scenario, where there are never any threads that need to be woken up.
pub fn run_example5() {
    let m = SpinLock::new(0);
    // We use std::hint::black_box() to force the compiler to assume there might be more code that accesses the mutex,
    // preventing it from optimizing away the loop or locking operations.
    std::hint::black_box(&m);

    let start = Instant::now();

    for _ in 0..5_000_000 {
        *m.lock() += 1;
    }

    let duration = start.elapsed();

    println!("Locked {} times in {:?}", *m.lock(), duration); // 22 ms on Apple M2 Pro with macOS
    assert_eq!(5_000_000, *m.lock());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_1() {
        assert_eq!(2, run_example1());
    }

    #[test]
    fn test_2() {
        assert_eq!(4 * 1_000_000, run_example2());
    }

    #[test]
    fn test_3() {
        let vec = SpinLock::new(Vec::new());

        thread::scope(|s| {
            s.spawn(|| vec.lock().push(1));
        });

        thread::scope(|s| {
            s.spawn(|| {
                let mut guard = vec.lock();
                guard.push(2);
                guard.push(2);
            });
        });

        let guard = vec.lock();
        assert!(guard.as_slice() == [1, 2, 2] || guard.as_slice() == [2, 2, 1]);
    }
}
