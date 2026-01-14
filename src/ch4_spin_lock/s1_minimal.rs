//! [A Minimal Implementation](https://marabos.nl/atomics/building-spinlock.html#a-minimal-implementation)

use std::{
    cell::UnsafeCell,
    sync::{
        atomic::{Ordering::*, *},
        *,
    },
    thread,
};

pub struct SpinLock {
    locked: AtomicBool,
}

impl Default for SpinLock {
    fn default() -> Self {
        Self::new()
    }
}

impl SpinLock {
    pub const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
        }
    }

    pub fn lock(&self) {
        while self
            .locked
            .compare_exchange_weak(false, true, Acquire, Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
    }

    pub fn unlock(&self) {
        self.locked.store(false, Release);
    }
}

struct SharedCounter {
    lock: SpinLock,
    value: UnsafeCell<usize>,
}

unsafe impl Sync for SharedCounter {}

pub fn run_example() {
    let counter = SharedCounter {
        lock: SpinLock::new(),
        value: UnsafeCell::new(0),
    };
    let counter = Arc::new(counter);

    thread::scope(|s| {
        let counter1 = Arc::clone(&counter);
        s.spawn(move || {
            counter1.lock.lock();
            unsafe { *counter1.value.get() += 1 };
            counter1.lock.unlock();
        });

        let counter2 = Arc::clone(&counter);
        s.spawn(move || {
            counter2.lock.lock();
            unsafe { *counter2.value.get() += 1 };
            counter2.lock.unlock();
        });
    });

    println!("{}", unsafe { *counter.value.get() });
}
