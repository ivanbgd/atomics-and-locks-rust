//! # Safety Through Runtime Checks
//!
//! https://marabos.nl/atomics/building-channels.html#safety-through-runtime-checks
//!
//! Doesn’t provide a blocking interface (has non-blocking receive). User needs to block if they wish.

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::thread;
use std::time::Duration;

/// One-shot channel
#[derive(Debug)]
pub struct Channel<T> {
    /// Message item
    message: UnsafeCell<MaybeUninit<T>>,
    /// Is a `send` in progress?
    in_use: AtomicBool,
    /// Has another `send` already finished?
    ready: AtomicBool,
}

unsafe impl<T> Sync for Channel<T> where T: Send {}

impl<T> Default for Channel<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Channel<T> {
    /// New channel
    pub const fn new() -> Self {
        Self {
            message: UnsafeCell::new(MaybeUninit::uninit()),
            in_use: AtomicBool::new(false),
            ready: AtomicBool::new(false),
        }
    }

    /// Non-blocking send
    ///
    /// Panics when trying to send more than one message.
    pub fn send(&self, message: T) {
        if self.in_use.swap(true, Relaxed) {
            panic!("can't send more than one message");
        }
        unsafe {
            (*self.message.get()).write(message);
        }
        self.ready.store(true, Release);
    }

    /// Returns whether a message is stored (available) in the channel.
    pub fn is_ready(&self) -> bool {
        self.ready.load(Relaxed)
    }

    /// Non-blocking receive
    ///
    /// Panics if no message is available yet, or if the message was already consumed.
    ///
    /// Tip: Use [`Channel::is_ready`] to check first.
    pub fn recv(&self) -> T {
        if !self.ready.swap(false, Acquire) {
            panic!("no message available");
        }
        // SAFETY: We've just checked (and reset) the ready flag.
        unsafe { (*self.message.get()).assume_init_read() }
    }
}

impl<T> Drop for Channel<T> {
    fn drop(&mut self) {
        if *self.ready.get_mut() {
            unsafe { self.message.get_mut().assume_init_drop() }
        }
    }
}

/// There's one sender thread (child) and one receiver thread (parent/main), and one one-shot channel between them.
pub fn run_example() {
    let channel = Channel::new();

    let t = thread::current();

    thread::scope(|s| {
        s.spawn(|| {
            thread::sleep(Duration::from_secs(1));
            channel.send("hello checks world!");
            t.unpark();
        });
    });

    // We have to block manually waiting for a message, as the `Channel::recv()` method is non-blocking.
    // We park the thread to achieve that.
    while !channel.is_ready() {
        thread::park();
    }

    let message = channel.recv();
    println!("{message}");
    assert_eq!("hello checks world!", message);
}
