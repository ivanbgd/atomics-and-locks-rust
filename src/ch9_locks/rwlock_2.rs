//! # Reader-Writer Lock: Avoiding Busy-Looping Writers
//!
//! https://marabos.nl/atomics/building-locks.html#avoiding-busy-looping-writers
//!
//! Prioritizes readers over writers. This can lead to writer starvation.
//!
//! To avoid potential busy-waiting loop in `write_lock()` (see comments in
//! [version 1](super::rwlock_1::RwLock::write_lock)), we introduce `writer_wake_counter` which we increment
//! when a writer should be woken. We never decrement it.

use atomic_wait::{wait, wake_all, wake_one};
use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};

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
        // The `write_lock()` method now needs to wait for the new atomic variable instead,
        // which is `self.writer_wake_counter`.
        // But, we first need to check if there are any readers that have the lock.
        while self
            .state
            .compare_exchange(0, WRITE_LOCKED, Acquire, Relaxed)
            .is_err()
        {
            // To make sure we don’t miss any notifications between seeing that the RwLock is read-locked
            // and actually going to sleep, we’ll use a pattern similar to the one we used for implementing our
            // condition variable: load the `writer_wake_counter` before checking whether we still want to sleep.
            //
            // We also want to re-check the "read" counter, `state`, to see if it has changed in the meantime,
            // during this brief period.
            // The "read" is under quotes because `state` can also denote the write-locked state.
            let writer_wake_counter = self.writer_wake_counter.load(Acquire);
            // Re-check `state` to see if there are still some active readers.
            // If there aren't any active readers, we won't go to sleep (`wait()`), and we'll check `state`
            // in the very next iteration of the `while` loop, to see if we can then write-lock.
            // So, we check for readers (`state`) in two places: during the CAS operation and here.
            //
            // This clearly prioritizes readers over writers.
            // This gives readers a chance to take over the lock (again).
            if self.state.load(Relaxed) != 0 {
                // Wait if the RwLock is still (read- and write-) locked, but only if there
                // have been no wake signals since we last checked (just above).
                // If `writer_wake_counter` has not changed, this means that no other thread has released the lock,
                // and we have to wait.
                // If `writer_wake_counter` has changed, this means that some other thread has released
                // the write-lock and we don't have to wait. But, we'll re-check `state` in the very next iteration
                // to see if there are new readers who acquired the lock.
                // We have also re-checked that there still are some active readers holding the lock.
                // `wait()` checks for the write-lock state only, but we generally check
                // for both read- and write-locking in this `if` statement.
                wait(&self.writer_wake_counter, writer_wake_counter);
            }
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
