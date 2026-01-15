//! # A Simple Mutex-Based Channel
//!
//! https://marabos.nl/atomics/building-channels.html#a-simple-mutex-based-channel

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

/// A simple MPMC channel implemented using [`Mutex`], [`VecDeque`] and [`Condvar`]
/// with non-blocking send and blocking receive.
#[derive(Default, Debug)]
pub struct SimpleChannel<T> {
    queue: Mutex<VecDeque<T>>,
    item_ready: Condvar,
}

impl<T> SimpleChannel<T> {
    pub const fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            item_ready: Condvar::new(),
        }
    }

    /// Non-blocking send
    pub fn send(&self, message: T) {
        self.queue.lock().unwrap().push_back(message);
        self.item_ready.notify_one();
    }

    /// Blocking receive
    pub fn recv(&self) -> T {
        let mut guard = self.queue.lock().unwrap();
        loop {
            if let Some(message) = guard.pop_front() {
                return message;
            } else {
                // `wait()` blocks the current thread until this condition variable receives a notification.
                guard = self.item_ready.wait(guard).unwrap();
            }
        }
    }
}

/// Runs infinitely long. Uses [`thread::scoped::scope`].
pub fn run_example1() {
    let ch = SimpleChannel::new();

    thread::scope(|s| {
        s.spawn(|| {
            loop {
                let message = ch.recv();
                println!(": {message}");
            }
        });

        for i in 0.. {
            ch.send(i);
            thread::sleep(Duration::from_secs(1));
        }
    });
}

/// Returns after four iterations. Uses [`thread::spawn`].
///
/// If we had joined the thread, we'd be waiting infinitely long for it to finish.
pub fn run_example2() {
    let ch = SimpleChannel::new();
    let ch = Arc::new(ch);

    let chc = Arc::clone(&ch);
    thread::spawn(move || {
        loop {
            let message = chc.recv();
            println!(": {message}");
        }
    });

    for i in 0..4 {
        ch.send(i);
        thread::sleep(Duration::from_secs(1));
    }
}

/// Four receiver threads run indefinitely.
///
/// The main thread is the sole sender, and it also runs indefinitely.
///
///  Uses [`thread::scoped::scope`].
pub fn run_example3() {
    let ch = SimpleChannel::new();
    let ch = Arc::new(ch);

    thread::scope(|s| {
        for i in 0..4 {
            let ch = Arc::clone(&ch);
            s.spawn(move || {
                loop {
                    let message = ch.recv();
                    println!("{i}: {message}");
                }
            });
        }

        for i in 0.. {
            ch.send(i);
            thread::sleep(Duration::from_secs(1));
        }
    });
}

/// Four receiver threads run indefinitely.
///
/// The main thread is the sole sender, and it also runs indefinitely.
///
///  Uses [`thread::spawn`].
pub fn run_example4() {
    let ch = SimpleChannel::new();
    let ch = Arc::new(ch);

    for i in 0..4 {
        let ch = Arc::clone(&ch);
        thread::spawn(move || {
            loop {
                let message = ch.recv();
                println!("{i}: {message}");
            }
        });
    }

    for i in 0.. {
        ch.send(i);
        thread::sleep(Duration::from_secs(1));
    }
}
