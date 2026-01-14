//! [A Minimal Implementation](https://marabos.nl/atomics/building-spinlock.html#a-minimal-implementation)
//!
//! Improved.
//!
//! Our spinlock contains a generic value.
//!
//! [`UnsafeCell`] is required for interior mutability.

use std::thread::JoinHandle;
use std::{
    cell::UnsafeCell,
    sync::{
        atomic::{Ordering::*, *},
        *,
    },
    thread,
};

pub struct SpinLock<T> {
    locked: AtomicBool,
    pub value: UnsafeCell<T>,
}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    pub fn lock(&self) {
        // Works incorrectly if we use `Relaxed` instead of `Acquire` for the success case.
        // SAFETY: Acquire/Release
        while self
            .locked
            .compare_exchange_weak(false, true, Acquire, Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
    }

    pub fn unlock(&self) {
        // Works incorrectly if we use `Relaxed` instead of `Release`.
        // SAFETY: Acquire/Release
        self.locked.store(false, Release);
    }
}

// SAFETY: All interior mutations are synchronized properly (by spinlock and memory orderings).
unsafe impl<T> Sync for SpinLock<T> {}

pub fn run_example1() -> i32 {
    let counter = Arc::new(SpinLock::new(0));

    thread::scope(|s| {
        let counter1 = Arc::clone(&counter);
        s.spawn(move || {
            counter1.lock();
            unsafe { *counter1.value.get() += 1 };
            counter1.unlock();
        });

        let counter2 = Arc::clone(&counter);
        s.spawn(move || {
            counter2.lock();
            unsafe { *counter2.value.get() += 1 };
            counter2.unlock();
        });
    });

    let result = unsafe { *counter.value.get() };
    assert_eq!(2, result);
    println!("Total 1: {}", result);

    result
}

pub fn run_example2() -> i32 {
    let counter = Arc::new(SpinLock::new(0));

    let handles: [JoinHandle<()>; 4] = std::array::from_fn(|_| {
        let counter = Arc::clone(&counter);
        thread::spawn(move || {
            for _ in 0..1_000_000 {
                counter.lock();
                unsafe {
                    *counter.value.get() += 1;
                }
                counter.unlock();
            }
        })
    });

    for h in handles {
        h.join().unwrap();
    }

    let result = unsafe { *counter.value.get() };
    assert_eq!(4 * 1_000_000, result);
    println!("Total 2: {}", result);

    result
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
}
