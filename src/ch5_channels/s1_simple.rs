//! # A Simple Mutex-Based Channel
//!
//! https://marabos.nl/atomics/building-channels.html#a-simple-mutex-based-channel
//!
//! Non-blocking send with blocking receive

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

/// A simple MPMC channel implemented using [`Mutex`], [`VecDeque`] and [`Condvar`]
/// with non-blocking send and blocking receive.
#[derive(Debug, Default)]
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

/// Runs infinitely long.
///
/// Has one sender (the main thread) and one additional receiver thread.
///
/// We get exactly one output per second (in receiver).
///
/// Uses [`thread::scoped::scope`].
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

/// Returns after four iterations.
/// If we had joined the child thread, we'd be waiting infinitely long for it to finish.
///
/// Has one sender (the main thread) and one additional receiver thread.
///
/// We get exactly one output per second (in receiver).
///
/// Uses [`thread::spawn`].
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
/// We get exactly one output per second (in receiver).
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
/// We get exactly one output per second (in receiver).
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

/// Four receiver threads run indefinitely.
///
/// Three sender threads also run indefinitely.
///
/// We get three outputs at once, each second (in receiver), because we have three senders now.
///
///  Uses [`thread::scoped::scope`].
pub fn run_example5() {
    let ch = SimpleChannel::new();
    let ch = Arc::new(ch);

    thread::scope(|s| {
        for rcv in 0..4 {
            let ch = Arc::clone(&ch);
            s.spawn(move || {
                loop {
                    let message = ch.recv();
                    println!("{rcv}: {message:?}");
                }
            });
        }

        for snd in 0..3 {
            let ch = Arc::clone(&ch);
            s.spawn(move || {
                for i in 0.. {
                    ch.send((snd, i));
                    thread::sleep(Duration::from_secs(1));
                }
            });
        }
    });
}

/// Four receiver threads run indefinitely.
///
/// Three sender threads also run indefinitely.
///
/// We get three outputs at once, each second (in receiver), because we have three senders now.
///
///  Uses [`thread::spawn`].
pub fn run_example6() {
    let ch = SimpleChannel::new();
    let ch = Arc::new(ch);

    let mut rcv_handles: Vec<JoinHandle<(i32, i32)>> = Vec::new();
    for rcv in 0..4 {
        let ch = Arc::clone(&ch);
        let h = thread::spawn(move || {
            loop {
                let message = ch.recv();
                println!("{rcv}: {message:?}");
            }
        });
        rcv_handles.push(h);
    }

    let mut snd_handles: Vec<JoinHandle<()>> = Vec::new();
    for snd in 0..3 {
        let ch = Arc::clone(&ch);
        let h = thread::spawn(move || {
            for i in 0.. {
                ch.send((snd, i));
                thread::sleep(Duration::from_secs(1));
            }
        });
        snd_handles.push(h);
    }

    // We only need one barrier of the two, any one (either one).
    for h in rcv_handles {
        h.join().unwrap();
    }
    for h in snd_handles {
        h.join().unwrap();
    }
}

/// A single receiver thread runs indefinitely.
///
/// Four sender threads also run indefinitely.
///
/// We get four outputs at once, each second (in receiver), because we have four senders now.
///
///  Uses [`thread::scoped::scope`].
pub fn run_example7() {
    let ch = SimpleChannel::new();
    let ch = Arc::new(ch);

    thread::scope(|s| {
        let chc = Arc::clone(&ch);
        s.spawn(move || {
            loop {
                let message = chc.recv();
                println!("{message:?}");
            }
        });

        for snd in 0..4 {
            let ch = Arc::clone(&ch);
            s.spawn(move || {
                for i in 0.. {
                    ch.send((snd, i));
                    thread::sleep(Duration::from_secs(1));
                }
            });
        }
    });
}

/// A single receiver thread runs indefinitely.
///
/// Four sender threads also run indefinitely.
///
/// We get four outputs at once, each second (in receiver), because we have four senders now.
///
///  Uses [`thread::spawn`].
pub fn run_example8() {
    let ch = SimpleChannel::new();
    let ch = Arc::new(ch);

    let chc = Arc::clone(&ch);
    thread::spawn(move || {
        loop {
            let message = chc.recv();
            println!("{message:?}");
        }
    });

    let mut snd_handles: Vec<JoinHandle<()>> = Vec::new();
    for snd in 0..4 {
        let ch = Arc::clone(&ch);
        let h = thread::spawn(move || {
            for i in 0.. {
                ch.send((snd, i));
                thread::sleep(Duration::from_secs(1));
            }
        });
        snd_handles.push(h);
    }

    for h in snd_handles {
        h.join().unwrap();
    }
}
