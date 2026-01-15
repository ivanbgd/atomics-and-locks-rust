//! # Using a Single Atomic for the Channel State
//!
//! https://marabos.nl/atomics/building-channels.html#single-atomic
//!
//! Doesn’t provide a blocking interface (has non-blocking receive). User needs to block if they wish.

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::thread;
use std::time::Duration;

const EMPTY: u8 = 0;
const WRITING: u8 = 1;
const READY: u8 = 2;
const READING: u8 = 3;

/// One-shot channel
#[derive(Debug)]
pub struct Channel<T> {
    /// Message item
    message: UnsafeCell<MaybeUninit<T>>,
    /// States: empty, writing, ready, reading
    state: AtomicU8,
}

unsafe impl<T: Send> Sync for Channel<T> {}

impl<T> Default for Channel<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Channel<T> {
    pub const fn new() -> Self {
        Self {
            message: UnsafeCell::new(MaybeUninit::uninit()),
            state: AtomicU8::new(EMPTY),
        }
    }

    /// Non-blocking send
    ///
    /// Panics when trying to send more than one message.
    pub fn send(&self, message: T) {
        if self
            .state
            .compare_exchange(EMPTY, WRITING, Relaxed, Relaxed)
            .is_err()
        {
            panic!("can't send more than one message");
        }
        unsafe {
            (*self.message.get()).write(message);
        }
        self.state.store(READY, Release);
    }

    /// Returns whether a message is stored (available) in the channel.
    pub fn is_ready(&self) -> bool {
        self.state.load(Relaxed) == READY
    }

    /// Non-blocking receive
    ///
    /// Panics if no message is available yet, or if the message was already consumed.
    ///
    /// Tip: Use [`Channel::is_ready`] to check first.
    pub fn recv(&self) -> T {
        if self
            .state
            .compare_exchange(READY, READING, Acquire, Relaxed)
            .is_err()
        {
            panic!("no message available");
        }
        // Safety: We've just checked (and reset) the ready flag.
        unsafe { (*self.message.get()).assume_init_read() }
    }
}

impl<T> Drop for Channel<T> {
    fn drop(&mut self) {
        if *self.state.get_mut() == READY {
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
            channel.send("hello world!");
            t.unpark();
        });
    });

    while !channel.is_ready() {
        thread::park();
    }

    let message = channel.recv();
    println!("{message}");
    assert_eq!("hello world!", message);
}
