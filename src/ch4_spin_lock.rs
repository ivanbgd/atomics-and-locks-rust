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

pub fn run_example2() -> i32 {
    let counter = Arc::new(SpinLock::new(0));

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

    // ~300 ms on Apple M2 Pro; it takes ~800 ms for ch9_locks::mutex_1.rs, but ~100 ms for ch9_locks::mutex_2.rs
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
