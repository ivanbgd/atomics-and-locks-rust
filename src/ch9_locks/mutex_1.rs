//! # Mutex
//!
//! https://marabos.nl/atomics/building-locks.html#mutex
//!
//! We use an enum to represent the unlocked and locked states, while the book uses concrete integer values
//! (magic numbers).
//!
//! We use trivial conversions (`as u32`), as well as `into()`, because we implement the [`From`] trait for `u32`.
//!
//! Perhaps it could be faster to use plain integer values, but this is just an example anyway,
//! and it follows the clean-code principles. Proper naming better conveys the purpose and use.
//!
//! If performance matters, it needs to be benchmarked on concrete platforms of interest (CPU + OS),
//! and it may also depend on the compiler (Rust) version, i.e., on compiler optimizations.
//!
//! I tried it on Apple M2 Pro with macOS and the speed was exactly the same for `into()` and `as u32`.

use atomic_wait::{wait, wake_one};
use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering::{Acquire, Release};
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;
use std::time::Instant;

#[derive(Debug)]
#[repr(u32)]
enum State {
    Unlocked = 0,
    Locked = 1,
}

impl From<State> for u32 {
    fn from(value: State) -> Self {
        value as Self
    }
}

#[derive(Debug)]
pub struct Mutex<T> {
    value: UnsafeCell<T>,
    state: AtomicU32,
}

// SAFETY: All interior mutations are synchronized properly (by Mutex and memory orderings).
unsafe impl<T: Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
            state: AtomicU32::new(State::Unlocked as u32), // We can't use `.into()` here because it isn't `const`.
        }
    }

    /// If the state is unlocked, locks the mutex.
    ///
    /// If it is already locked, waits until it becomes unlocked.
    ///
    /// Ultimately, in either case, it locks an unlocked mutex and returns the mutex guard to itself.
    pub fn lock(&self) -> MutexGuard<'_, T> {
        // Set the state to locked if it is unlocked...
        while self.state.swap(State::Locked.into(), Acquire) == State::Locked.into() {
            // ..., but, if it is already locked, wait until it becomes unlocked.
            // `wait()` also checks the state, so we can't miss a wake-up call between the `swap()` and `wait()` calls.
            // `wait()` will block if the state is locked. Since it may return spuriously, we need to call it in a loop.
            wait(&self.state, State::Locked.into());
        }

        MutexGuard { mutex: self }
    }
}

#[derive(Debug)]
pub struct MutexGuard<'a, T> {
    mutex: &'a Mutex<T>,
}

unsafe impl<T: Send> Send for MutexGuard<'_, T> {}
unsafe impl<T: Sync> Sync for MutexGuard<'_, T> {}

impl<T> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        // Set the state back to unlocked.
        self.mutex.state.store(State::Unlocked.into(), Release);

        // Wake up one of the threads waiting on `self.mutex.state`, if any.
        // It's enough to wake up only one thread because this is mutex.
        // Waking up all threads that are waiting would be a waste of time,
        // because only one can lock the mutex again.
        // So, a thread wakes up a thread.
        // Note that there is no guarantee that the one thread that we wake up will be able to grab the lock.
        // Another thread might still grab the lock right before it gets the chance.
        wake_one(&self.mutex.state);
    }
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: Existence of guard means that we have exclusively locked the mutex.
        unsafe { &*self.mutex.value.get() }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: Existence of guard means that we have exclusively locked the mutex.
        unsafe { &mut *self.mutex.value.get() }
    }
}

pub fn run_example1() -> i32 {
    let counter = Mutex::new(0);

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
    let counter = Arc::new(Mutex::new(0));
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

    // ~900 ms on Apple M2 Pro; it takes ~350 ms for ch4_spin_lock.rs, ~100 ms for mutex_2
    let elapsed = start.elapsed();

    let result = counter.lock();
    assert_eq!(4 * 1_000_000, *result);
    println!(
        "Total 2: {}; guard state = {:?}; elapsed = {:.3?}",
        *result, result.mutex.state, elapsed
    );

    *result
}

pub fn run_example3() {
    let vec = Mutex::new(Vec::new());

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
        "vec = {:?}; guard state = {:?}",
        guard.as_slice(),
        guard.mutex.state
    );
}

/// Like [`run_example2()`] but without `Arc` protecting the `Mutex`. The speed is the same.
pub fn run_example4() -> i32 {
    let counter = Mutex::new(0);
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

    // ~900 ms on Apple M2 Pro; it takes ~350 ms for ch4_spin_lock.rs, but ~80 ms for ch9_locks::mutex_2.rs
    let elapsed = start.elapsed();

    let result = counter.lock();
    assert_eq!(4 * 1_000_000, *result);
    println!(
        "Total 4: {}; locked state = {:?}; elapsed = {:.3?}",
        *result, result.mutex.state, elapsed
    );

    *result
}
