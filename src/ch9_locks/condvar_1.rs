//! # Condition Variable
//!
//! https://marabos.nl/atomics/building-locks.html#condition-variable
//!
//! A condition variable is used together with a mutex to wait until the mutex-protected data matches some condition.
//! It has a wait method that unlocks a mutex, waits for a signal, and locks the same mutex again.
//! Signals are sent by other threads, usually right after modifying the mutex-protected data,
//! to either one waiting thread (often called "notify one" or "signal") or all waiting threads
//! (often called "notify all" or "broadcast").
//!
//! We make sure that every notification changes an atomic variable (a counter), and our [`Condvar::wait()`]
//! methods makes use of that by passing its value to a futex-like `wait()` function after unlocking the mutex.
//! This way, the thread doesn't go to sleep if any notification signal arrived since unlocking the mutex,
//! i.e., if the count has changed.

use super::mutex_3::{Mutex, MutexGuard};
use atomic_wait::{wait, wake_all, wake_one};
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering::Relaxed;
use std::thread;
use std::time::Duration;

pub struct Condvar {
    counter: AtomicU32,
}

impl Default for Condvar {
    fn default() -> Self {
        Self::new()
    }
}

impl Condvar {
    pub const fn new() -> Self {
        Self {
            counter: AtomicU32::new(0),
        }
    }

    pub fn notify_one(&self) {
        self.counter.fetch_add(1, Relaxed);
        wake_one(&self.counter)
    }

    pub fn notify_all(&self) {
        self.counter.fetch_add(1, Relaxed);
        wake_all(&self.counter)
    }

    pub fn wait<'a, T>(&self, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
        // Load the counter value and save it, before unlocking the mutex.
        let count = self.counter.load(Relaxed);

        // Unlock the mutex by dropping the guard, but remember the mutex so we can lock it again later.
        let mutex = guard.mutex;
        drop(guard);

        // Wait, but only if the counter hasn't changed since unlocking.
        // We should only wait if the counter hasn’t changed, to make sure we didn’t miss any signals.
        // This way this thread doesn't go to sleep if it has received a signal since unlocking the mutex.
        //
        // So, we wait until count changes.
        // When it changes, that means that we've been notified by some other thread and we may resume.
        wait(&self.counter, count);

        mutex.lock()
    }
}

pub fn run_example() {
    let mutex = Mutex::new(0);
    let condvar = Condvar::new();

    let mut wakeups = 0;

    thread::scope(|s| {
        s.spawn(|| {
            thread::sleep(Duration::from_secs(1));
            *mutex.lock() = 123;
            condvar.notify_one();
        });

        let mut m = mutex.lock();
        if *m != 123 {
            m = condvar.wait(m);
            wakeups += 1;
        }

        assert_eq!(*m, 123);
    });

    assert_eq!(*mutex.lock(), 123);

    // Check that the main thread really did wait, without wasting processor time (cycles) busy-waiting
    // (spin-looping), while still allowing for a few spurious wake-ups.
    // Namely, if it spin-loops, it will increment `wakeups` to lot more than ten.
    println!("Number of wake-ups: {wakeups}");
    assert!(wakeups < 10);
}
