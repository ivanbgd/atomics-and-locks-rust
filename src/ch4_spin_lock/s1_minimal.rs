//! [A Minimal Implementation](https://marabos.nl/atomics/building-spinlock.html#a-minimal-implementation)

use std::thread::JoinHandle;
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

pub fn run_example1() {
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

    assert_eq!(2, unsafe { *counter.value.get() });
    println!("Total 1: {}", unsafe { *counter.value.get() });
}

pub fn run_example2() {
    let counter = SharedCounter {
        lock: SpinLock::new(),
        value: UnsafeCell::new(0),
    };
    let counter = Arc::new(counter);

    let handles: [JoinHandle<()>; 4] = std::array::from_fn(|_| {
        let counter = Arc::clone(&counter);
        thread::spawn(move || {
            for _ in 0..1_000_000 {
                counter.lock.lock();
                unsafe {
                    *counter.value.get() += 1;
                }
                counter.lock.unlock();
            }
        })
    });

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(4 * 1_000_000, unsafe { *counter.value.get() });
    println!("Total 2: {}", unsafe { *counter.value.get() });
}
