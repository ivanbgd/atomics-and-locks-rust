//! # Chapter 1. Basics of Rust Concurrency
//!
//! [Condition Variables](https://marabos.nl/atomics/basics.html#condvar)

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::thread;
use std::time::Duration;

pub fn run_example() {
    let queue = Mutex::new(VecDeque::new());
    let not_empty = Condvar::new();

    thread::scope(|s| {
        s.spawn(|| {
            loop {
                let mut guard = queue.lock().unwrap();
                let item = loop {
                    if let Some(item) = guard.pop_front() {
                        break item;
                    } else {
                        // `wait()` blocks the current thread until this condition variable receives a notification.
                        guard = not_empty.wait(guard).unwrap();
                    }
                };
                drop(guard);
                println!("{item}");
            }
        });

        for i in 0.. {
            queue.lock().unwrap().push_back(i);
            not_empty.notify_one();
            thread::sleep(Duration::from_secs(1));
        }
    });
}
