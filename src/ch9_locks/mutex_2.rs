//! # Mutex: Avoiding Syscalls
//!
//! https://marabos.nl/atomics/building-locks.html#mutex-avoid-syscalls
//!
//! On Apple M2 Pro on macOS this implementation seems to be much faster (multiple times)
//! than the first implementation (see [`run_example2()`]).

use atomic_wait::{wait, wake_one};
use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;
use std::time::Instant;

#[derive(Debug)]
#[repr(u32)]
enum State {
    /// Unlocked (0)
    Unlocked = 0,
    /// Locked and no other threads waiting (1)
    LockedWithoutWaiters = 1,
    /// Locked with other threads waiting (2)
    LockedWithWaiters = 2,
}

#[derive(Debug)]
pub struct Mutex<T> {
    state: AtomicU32,
    value: UnsafeCell<T>,
}

// SAFETY: All interior mutations are synchronized properly (by Mutex and memory orderings).
unsafe impl<T: Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
            state: AtomicU32::new(State::Unlocked as u32),
        }
    }

    /// If the state is unlocked, locks the mutex, as we know that there are no other waiters,
    /// since the mutex wasn't locked before.
    ///
    /// Ultimately, in either of the three cases, it locks an unlocked mutex and returns the mutex guard to itself.
    pub fn lock(&self) -> MutexGuard<'_, T> {
        // Set the state to locked without other waiters if it is unlocked and return the mutex guard to itself.
        // But, if it's locked, we need to set the state to locked with other waiters.
        if self
            .state
            .compare_exchange(
                State::Unlocked as u32,
                State::LockedWithoutWaiters as u32,
                Acquire,
                Relaxed,
            )
            .is_err()
        {
            // It was already locked, so set the state to locked with other waiters.
            while self.state.swap(State::LockedWithWaiters as u32, Acquire)
                != State::Unlocked as u32
            {
                // The mutex is indeed still locked. We should block.
                // Wait until the state is no longer locked with other waiters.
                // Still, if the state is not unlocked, we'll get back in this loop.
                //
                // `wait()` also checks the state, so we can't miss a wake-up call
                // between the `swap()` and `wait()` calls.
                // `wait()` will block if the state is locked with other waiters.
                // Since it may return spuriously, we need to call it in a loop.
                wait(&self.state, State::LockedWithWaiters as u32);
            }
            // The state was unlocked, and we've successfully set it to locked with other waiters.
        }
        // The state was unlocked, and we've successfully set it to locked without other waiters.

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
        // Set the state to unlocked, and if it was locked without other waiters, there is no other thread
        // that we should wake, so we can return.
        // If there were other waiters, we should wake up one of them.
        if self.mutex.state.swap(State::Unlocked as u32, Release) == State::LockedWithWaiters as u32
        {
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

    // ~100 ms on Apple M2 Pro; it takes ~350 ms for ch4_spin_lock.rs, ~900 ms for mutex_1
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

/// Like [`run_example2()`] but without `Arc` protecting the `Mutex`.
/// This one is slightly faster than [`run_example2()`].
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

    // ~80 ms on Apple M2 Pro; it takes ~350 ms for ch4_spin_lock.rs, ~900 ms for mutex_1
    let elapsed = start.elapsed();

    let result = counter.lock();
    assert_eq!(4 * 1_000_000, *result);
    println!(
        "Total 4: {}; locked state = {:?}; elapsed = {:.3?}",
        *result, result.mutex.state, elapsed
    );

    *result
}
