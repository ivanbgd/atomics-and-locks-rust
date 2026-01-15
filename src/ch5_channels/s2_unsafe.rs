//! # An Unsafe One-Shot Channel
//!
//! https://marabos.nl/atomics/building-channels.html#an-unsafe-one-shot-channel
//!
//! This variant isn't correct.

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::{Acquire, Release};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[derive(Debug)]
pub struct Channel<T> {
    message: UnsafeCell<MaybeUninit<T>>,
    ready: AtomicBool,
}

unsafe impl<T> Sync for Channel<T> where T: Send {}

impl<T> Default for Channel<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Channel<T> {
    pub const fn new() -> Self {
        Self {
            message: UnsafeCell::new(MaybeUninit::uninit()),
            ready: AtomicBool::new(false),
        }
    }

    /// Non-blocking send
    ///
    /// # Safety
    ///
    /// Only call this once!
    pub unsafe fn send(&self, message: T) {
        unsafe {
            (*self.message.get()).write(message);
        }
        self.ready.store(true, Release);
    }

    /// Returns whether a message is stored (available) in the channel.
    pub fn is_ready(&self) -> bool {
        self.ready.load(Acquire)
    }

    /// Non-blocking receive
    ///
    /// # Safety
    ///
    /// Only call this once, and only after [`Channel::is_ready`] returns `true`!
    pub unsafe fn recv(&self) -> T {
        unsafe { (*self.message.get()).assume_init_read() }
    }
}

/// Four receiver threads run indefinitely.
///
/// Three sender threads.
///
/// This runs only once - with only one sender! I'm not sure about usage.
///
///  Uses [`thread::scoped::scope`].
pub fn run_example() {
    let ch = Channel::new();
    let ch = Arc::new(ch);

    thread::scope(|s| {
        for rcv in 0..4 {
            let ch = Arc::clone(&ch);
            s.spawn(move || {
                loop {
                    if ch.is_ready() {
                        let message = unsafe { ch.recv() };
                        println!("{rcv}: {message:?}");
                        break;
                    }
                }
            });
        }

        for snd in 0..3 {
            let ch = Arc::clone(&ch);
            s.spawn(move || {
                for i in 0.. {
                    unsafe {
                        ch.send((snd, i));
                    }
                    thread::sleep(Duration::from_secs(1));
                }
            });
        }
    });
}
