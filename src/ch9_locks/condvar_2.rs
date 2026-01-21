//! # Condition Variable: Avoiding Syscalls
//!
//! https://marabos.nl/atomics/building-locks.html#avoiding-syscalls
//!
//! Optimizing a locking primitive is mainly about avoiding unnecessary `wait` and `wake` operations.
//!
//! Those two operations involve system calls, and if we can avoid them somehow, at least in some cases,
//! we can get a performance improvement.
//!
//! We introduce keeping track of the number of waiting threads.
//! If there are no waiters, the notify functions don't do anything. That's how we save execution time.

use super::mutex_3::{Mutex, MutexGuard};
use atomic_wait::{wait, wake_all, wake_one};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicU32, AtomicUsize};
use std::thread;
use std::time::{Duration, Instant};

pub struct Condvar {
    counter: AtomicU32,
    num_waiters: AtomicUsize,
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
            num_waiters: AtomicUsize::new(0),
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

    pub fn wait<'a, T>(&self, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
        self.num_waiters.fetch_add(1, Relaxed);

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

        self.num_waiters.fetch_sub(1, Relaxed);

        mutex.lock()
    }
}

pub fn run_example1() {
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
        if *m < 100 {
            m = condvar.wait(m);
            wakeups += 1;
        }

        assert_eq!(*m, 123);
    });

    assert_eq!(*mutex.lock(), 123);

    // Check that the main thread really did wait, without wasting processor time (cycles) busy-waiting
    // (spin-looping), while still allowing for a few spurious wake-ups.
    // Namely, if it spin-loops, it will increment `wakeups` to lot more than ten.
    println!("Number of wake-ups: {wakeups}"); // 1
    assert!(wakeups < 10);
}

pub fn run_example2() {
    let counter = Mutex::new(0);
    std::hint::black_box(&counter); // Doesn't affect performance (on Apple M2 Pro).

    let condvar = Condvar::new();
    std::hint::black_box(&condvar); // Doesn't affect performance (on Apple M2 Pro).

    let start = Instant::now();

    thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| {
                for _ in 0..1_000_000 {
                    *counter.lock() += 1;
                    condvar.notify_one();
                }
            });
        }

        for _ in 0..4 {
            s.spawn(|| {
                for _ in 0..1_000_000 {
                    let mut cnt = counter.lock();
                    if *cnt < 4_000_000 {
                        cnt = condvar.wait(cnt);
                    }
                }
            });
        }
    });

    // 2.7 s on Apple M2 Pro
    let elapsed = start.elapsed();

    let result = counter.lock();
    assert_eq!(4 * 1_000_000, *result);

    // Total 4: 4000000; mutex state = 1; condvar counter 3944489; elapsed = 2.666s
    println!(
        "Total 4: {}; mutex state = {:?}; condvar counter {}; elapsed = {:.3?}",
        *result,
        result.mutex.state,
        condvar.counter.into_inner(),
        elapsed
    );
}
