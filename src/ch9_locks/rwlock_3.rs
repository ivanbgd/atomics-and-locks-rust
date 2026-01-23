//! # Reader-Writer Lock: Avoiding Writer Starvation
//!
//! https://marabos.nl/atomics/building-locks.html#avoiding-writer-starvation
//!
//! A common use case for an RwLock is a situation with many frequent readers, but very few, often only one,
//! infrequent writer. For example, one thread might be responsible for reading out some sensor input or
//! periodically downloading some new data that many other threads need to use.
//!
//! This implementation, version 3, is useful in cases in which we want writers
//! to be more or less equal with readers in terms of priority.
//!
//! We don't let any new readers acquire the lock if there is at least one writer waiting on the `RwLock`,
//! even if the lock is read-locked.
//!
//! This way, new readers have to wait for at least one writer to take its turn,
//! At the same time, this also means that readers will have access to fresh data.
//!
//! For a reader-writer lock that’s optimized for the "frequent reading and infrequent writing" use case, this is
//! quite acceptable, since write-locking (and therefore write-unlocking) happens infrequently.
//!
//! For a more general purpose reader-writer lock, however, it is definitely worth optimizing further,
//! to bring the performance of write-locking and -unlocking near the performance of an efficient 3-state mutex.
//!
//! This implementation seems to be significantly faster than version 1, at least in [`run_example3()`].
//! We get half the number of writer wake-ups as compared to version 2 of RwLock.
//! This proves that writers are valued more equally with readers in this implementation than in previous ones.

use atomic_wait::{wait, wake_all, wake_one};
use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::thread;
use std::time::Instant;

/// A special value to mark the state as write-locked.
///
/// Maximum allowed number of concurrent readers is one less than this.
const WRITE_LOCKED: u32 = u32::MAX;

#[derive(Debug)]
pub struct RwLock<T> {
    /// Data
    value: UnsafeCell<T>,
    /// The number of read locks times two, plus one if there's a writer waiting.
    /// [`WRITE_LOCKED`] (`u32::MAX`) if write-locked.
    /// Zero means unlocked.
    /// This means that readers may acquire the lock when the state is even, but need to block when odd.
    state: AtomicU32,
    /// Incremented to wake up writers. We *never* decrement it!
    writer_wake_counter: AtomicU32,
}

// Multiple readers should be able to access the data at the same time, hence additionally `T: Sync`.
// SAFETY: All interior mutations are synchronized properly (by RwLock and memory orderings).
unsafe impl<T: Send + Sync> Sync for RwLock<T> {}

impl<T> RwLock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
            state: AtomicU32::new(0), // Unlocked
            writer_wake_counter: AtomicU32::new(0),
        }
    }

    pub fn read_lock(&self) -> ReadGuard<'_, T> {
        let mut state = self.state.load(Relaxed);

        loop {
            if state.is_multiple_of(2) {
                assert!(state < WRITE_LOCKED - 2, "too many readers");
                match self
                    .state
                    .compare_exchange_weak(state, state + 2, Acquire, Relaxed)
                {
                    Ok(_) => return ReadGuard { rwlock: self },
                    Err(new_state) => state = new_state,
                }
            }

            if !state.is_multiple_of(2) {
                wait(&self.state, state);
                state = self.state.load(Relaxed);
            }
        }
    }

    pub fn write_lock(&self) -> WriteGuard<'_, T> {
        let mut state = self.state.load(Relaxed);

        loop {
            // Try to lock if unlocked: completely unlocked (no readers or writers) or there's only one active reader
            // (readers are blocked when state is odd, and this also means there are no writers).
            if state <= 1 {
                match self
                    .state
                    .compare_exchange(state, WRITE_LOCKED, Acquire, Relaxed)
                {
                    Ok(_) => return WriteGuard { rwlock: self },
                    Err(new_state) => {
                        state = new_state;
                        continue;
                    }
                }
            }

            // Block new readers by making sure the updated state is odd.
            if state.is_multiple_of(2) {
                match self
                    .state
                    .compare_exchange(state, state + 1, Relaxed, Relaxed)
                {
                    Ok(_) => {}
                    Err(new_state) => {
                        state = new_state;
                        continue;
                    }
                }
            }

            // Wait if it's still locked (if the lock hasn’t been unlocked in the meantime).
            let writer_wake_counter = self.writer_wake_counter.load(Acquire);
            state = self.state.load(Relaxed);
            if state >= 2 {
                wait(&self.writer_wake_counter, writer_wake_counter);
                state = self.state.load(Relaxed);
            }
        }
    }
}

#[derive(Debug)]
pub struct ReadGuard<'a, T> {
    rwlock: &'a RwLock<T>,
}

impl<T> Drop for ReadGuard<'_, T> {
    fn drop(&mut self) {
        // Since we now track whether there are any waiting writers, read-unlocking can now skip
        // the `wake_one()` call when it is unnecessary.
        // If we decrement from 5 to 3, for example, that means that there is another writer holding the RwLock.

        // Decrement the state by two to remove one read-lock.
        if self.rwlock.state.fetch_sub(2, Release) == 3 {
            // If we decremented from 3 to 1, that means that RwLock is now unlocked
            // *and* there is a waiting writer, which we'll now wake up.

            // Signalize that a writer can be woken up.
            self.rwlock.writer_wake_counter.fetch_add(1, Release);
            // Wake up a waiting writer, if any.
            // Waking up just one thread is enough, because we know there cannot be any waiting readers at this point.
            // There would simply be no reason for a reader to be waiting on a read-locked RwLock.
            wake_one(&self.rwlock.writer_wake_counter);
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
        // While write-locked (a state of u32::MAX), we do not track any information on whether any thread is waiting.
        // So, we have no new information to use for write-unlocking, which will remain identical

        // A writer must reset the `state` to zero to unlock,
        // after which it should wake either one waiting writer or all waiting readers.
        // We don’t know whether readers or writers are waiting, nor do we have a way to wake up
        // only a writer or only the readers. So, we’ll just wake a writer and all readers.
        self.rwlock.state.store(0, Release);
        // Signalize that a writer can be woken up.
        self.rwlock.writer_wake_counter.fetch_add(1, Release);
        // Wake up one waiting writer.
        wake_one(&self.rwlock.writer_wake_counter);
        // Wake up all waiting readers.
        wake_all(&self.rwlock.state);
        // On some OSes, the wake functions return the number of threads they awoke, so that can be used as
        // an optimization. If `wake_one()` returns that it woke up a thread, we don't need to call `wake_all()`.
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

/// Multiple writers (four) and multiple readers (four).
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

    // 1.6 s on Apple M2 Pro, but 1.4 s with three readers,
    // 1.1 s with two readers, 1.0 s with a single reader, and 0.7 s with no readers.
    // This is significantly faster than versions 1 and 2.
    let elapsed = start.elapsed();

    let result = counter.read_lock();
    assert_eq!(4 * 1_000_000, *result);

    // We only expect to have a single reader at this point, hence rwlock state = 2.
    // Total: 4000000; rwlock state = 2, writer wake counter: 4022776; elapsed = 1.610s
    // This is half the number of writer wake-ups as compared to version 2 of RwLock.
    println!(
        "Total: {}; rwlock state = {:?}, writer wake counter: {:?}; elapsed = {:.3?}",
        *result, result.rwlock.state, result.rwlock.writer_wake_counter, elapsed,
    );
}
