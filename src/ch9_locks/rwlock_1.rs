//! # Reader-Writer Lock
//!
//! https://marabos.nl/atomics/building-locks.html#reader-writer-lock
//!
//! Prioritizes readers over writers. This can lead to writer starvation.
//!
//! I've added an accompanying [`ReadCondvar`] to be able to test correctness of the implementation,
//! and also to be able to compare it with mutex with condvar.
//!
//! I've also added an example which doesn't use a condvar.
//! It seems to be slightly slower than with the condvar.
//!
//! The book and its repository don't have examples for testing it.

use atomic_wait::{wait, wake_all, wake_one};
use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::thread;
use std::time::{Duration, Instant};

/// A special value to mark the state as write-locked.
///
/// Maximum allowed number of concurrent readers is one less than this.
const WRITE_LOCKED: u32 = u32::MAX;

#[derive(Debug)]
pub struct RwLock<T> {
    /// Data
    value: UnsafeCell<T>,
    /// The number of readers or [`WRITE_LOCKED`] (`u32::MAX`) if write-locked. Zero means unlocked.
    state: AtomicU32,
}

// Multiple readers should be able to access the data at the same time, hence additionally `T: Sync`.
// SAFETY: All interior mutations are synchronized properly (by RwLock and memory orderings).
unsafe impl<T: Send + Sync> Sync for RwLock<T> {}

impl<T> RwLock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
            state: AtomicU32::new(0), // Unlocked
        }
    }

    pub fn read_lock(&self) -> ReadGuard<'_, T> {
        let mut state = self.state.load(Relaxed);

        loop {
            if state < WRITE_LOCKED {
                assert!(state < WRITE_LOCKED - 1, "too many readers");
                match self
                    .state
                    .compare_exchange_weak(state, state + 1, Acquire, Relaxed)
                {
                    Ok(_) => return ReadGuard { rwlock: self },
                    Err(new_state) => state = new_state,
                }
            }

            if state == WRITE_LOCKED {
                wait(&self.state, WRITE_LOCKED);
                state = self.state.load(Relaxed);
            }
        }
    }

    pub fn write_lock(&self) -> WriteGuard<'_, T> {
        while let Err(state) = self
            .state
            .compare_exchange(0, WRITE_LOCKED, Acquire, Relaxed)
        {
            // If we have a lot of readers, they could change the state counter in rapid succession,
            // so there's a high chance of `self.state` changing between the above CAS operation and
            // the below wait operation in such cases, if the wait operation is directly implemented
            // as a (relatively slow) syscall.
            // This wait is implemented as a syscall, which is relatively slow.
            // This might result in an accidental busy-waiting loop.
            // There is room for improvement here, and we do it in the version 2.
            wait(&self.state, state);
        }

        WriteGuard { rwlock: self }
    }
}

#[derive(Debug)]
pub struct ReadGuard<'a, T> {
    rwlock: &'a RwLock<T>,
}

impl<T> Drop for ReadGuard<'_, T> {
    fn drop(&mut self) {
        if self.rwlock.state.fetch_sub(1, Release) == 1 {
            // Wake up a waiting writer, if any.
            // Waking up just one thread is enough, because we know there cannot be any waiting readers at this point.
            // There would simply be no reason for a reader to be waiting on a read-locked RwLock.
            wake_one(&self.rwlock.state);
        }
    }
}

impl<T> Deref for ReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.rwlock.value.get() }
    }
}

#[derive(Debug)]
pub struct WriteGuard<'a, T> {
    rwlock: &'a RwLock<T>,
}

impl<T> Drop for WriteGuard<'_, T> {
    fn drop(&mut self) {
        // A writer must reset the state to zero to unlock,
        // after which it should wake either one waiting writer or all waiting readers.
        // We don’t know whether readers or writers are waiting, nor do we have a way to wake up
        // only a writer or only the readers. So, we’ll just wake all threads.
        self.rwlock.state.store(0, Release);
        // Wake up all waiting readers and writers.
        wake_all(&self.rwlock.state);
    }
}

impl<T> Deref for WriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.rwlock.value.get() }
    }
}

impl<T> DerefMut for WriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.rwlock.value.get() }
    }
}

pub struct ReadCondvar {
    counter: AtomicU32,
    num_waiters: AtomicU32,
}

impl Default for ReadCondvar {
    fn default() -> Self {
        Self::new()
    }
}

impl ReadCondvar {
    pub const fn new() -> Self {
        Self {
            counter: AtomicU32::new(0),
            num_waiters: AtomicU32::new(0),
        }
    }

    pub fn notify_one(&self) {
        // If there are no waiters, don't do anything!
        if self.num_waiters.load(Relaxed) > 0 {
            self.counter.fetch_add(1, Relaxed);
            wake_one(&self.counter);
        }
    }

    pub fn notify_all(&self) {
        // If there are no waiters, don't do anything!
        if self.num_waiters.load(Relaxed) > 0 {
            self.counter.fetch_add(1, Relaxed);
            wake_all(&self.counter);
        }
    }

    pub fn wait<'a, T>(&self, guard: ReadGuard<'a, T>) -> ReadGuard<'a, T> {
        self.num_waiters.fetch_add(1, Relaxed);

        // Load the counter value and save it, before unlocking the rwlock.
        let count = self.counter.load(Relaxed);

        // Unlock the rwlock by dropping the guard, but remember the rwlock so we can lock it again later.
        let rwlock = guard.rwlock;
        drop(guard);

        // Wait, but only if the counter hasn't changed since unlocking.
        // We should only wait if the counter hasn’t changed, to make sure we didn’t miss any signals.
        // This way this thread doesn't go to sleep if it has received a signal since unlocking the rwlock.
        //
        // So, we wait until count changes.
        // When it changes, that means that we've been notified by some other thread and we may resume.
        wait(&self.counter, count);

        self.num_waiters.fetch_sub(1, Relaxed);

        rwlock.read_lock()
    }
}

/// A single writer and a single reader, with [`ReadCondvar`].
pub fn run_example1() {
    let rwlock = RwLock::new(0);
    let condvar = ReadCondvar::new();

    let mut wakeups = 0;

    thread::scope(|s| {
        s.spawn(|| {
            thread::sleep(Duration::from_secs(1));
            *rwlock.write_lock() = 123;
            condvar.notify_one();
        });

        let mut m = rwlock.read_lock();
        if *m < 100 {
            m = condvar.wait(m);
            wakeups += 1;
        }

        assert_eq!(*m, 123);
    });

    assert_eq!(*rwlock.read_lock(), 123);

    // Check that the main thread really did wait, without wasting processor time (cycles) busy-waiting
    // (spin-looping), while still allowing for a few spurious wake-ups.
    // Namely, if it spin-loops, it will increment `wakeups` to lot more than ten.
    println!("Number of wake-ups: {wakeups}"); // 1
    assert!(wakeups < 10);
}

/// Multiple writers (four) and multiple readers (four), with [`ReadCondvar`].
///
/// It doesn't make a difference whether we notify one or all waiting threads.
pub fn run_example2() {
    let counter = RwLock::new(0);
    std::hint::black_box(&counter); // Doesn't affect performance (on Apple M2 Pro).

    let condvar = ReadCondvar::new();
    std::hint::black_box(&condvar); // Doesn't affect performance (on Apple M2 Pro).

    let start = Instant::now();

    thread::scope(|s| {
        // Writer threads
        for _ in 0..4 {
            s.spawn(|| {
                for _ in 0..1_000_000 {
                    *counter.write_lock() += 1;
                    condvar.notify_one(); // One or all, it doesn't make a difference.
                }
            });
        }

        // Reader threads
        for _ in 0..4 {
            s.spawn(|| {
                for _ in 0..1_000_000 {
                    let mut cnt = counter.read_lock();
                    if *cnt < 4_000_000 {
                        cnt = condvar.wait(cnt);
                    }
                }
            });
        }
    });

    // 2.9 s on Apple M2 Pro, but 2.5 s if we have three readers instead of four,
    // 2.1 with only have two readers, and 1.4 s with only one reader.
    // Also, 0.9 s with no readers at all.
    let elapsed = start.elapsed();

    let result = counter.read_lock();
    assert_eq!(4 * 1_000_000, *result);

    // We only expect to have a single reader at this point, hence rwlock state = 1.
    // Total: 4000000; rwlock state = 1; condvar counter 3946594; elapsed = 2.877s
    println!(
        "Total: {}; rwlock state = {:?}; condvar counter {}; elapsed = {:.3?}",
        *result,
        result.rwlock.state,
        condvar.counter.into_inner(),
        elapsed
    );
}

/// Multiple writers (four) and multiple readers (four), without [`ReadCondvar`].
///
/// It doesn't make a difference whether we notify one or all waiting threads.
pub fn run_example3() {
    let counter = RwLock::new(0);
    std::hint::black_box(&counter); // Doesn't affect performance (on Apple M2 Pro).

    let start = Instant::now();

    thread::scope(|s| {
        // Writer threads
        for _ in 0..4 {
            s.spawn(|| {
                for _ in 0..1_000_000 {
                    *counter.write_lock() += 1;
                }
            });
        }

        // Reader threads
        for _ in 0..4 {
            s.spawn(|| {
                for _ in 0..1_000_000 {
                    let _cnt = counter.read_lock();
                }
            });
        }
    });

    // 3.1 s on Apple M2 Pro, but 2.6 s with three readers,
    // 2.2 s with two readers, 1.5 s with a single reader, and 1.1 s with no readers.
    let elapsed = start.elapsed();

    let result = counter.read_lock();
    assert_eq!(4 * 1_000_000, *result);

    // We only expect to have a single reader at this point, hence rwlock state = 1.
    // Total: 4000000; rwlock state = 1; elapsed = 3.143
    println!(
        "Total: {}; rwlock state = {:?}; elapsed = {:.3?}",
        *result, result.rwlock.state, elapsed
    );
}
