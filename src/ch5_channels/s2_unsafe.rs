//! # An Unsafe One-Shot Channel
//!
//! https://marabos.nl/atomics/building-channels.html#an-unsafe-one-shot-channel
//!
//! This variant has non-blocking receive. User needs to block if they wish.

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::{Acquire, Release};
use std::thread;

/// One-shot channel
#[derive(Debug)]
pub struct Channel<T> {
    /// Message item
    message: UnsafeCell<MaybeUninit<T>>,
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

/// There's one sender thread and one receiver thread, and one one-shot channel between them.
pub fn run_example1() {
    let channel = Channel::new();

    thread::scope(|s| {
        s.spawn(|| {
            loop {
                if channel.is_ready() {
                    let message = unsafe { channel.recv() };
                    println!("{message}");
                    assert_eq!("hello world 1!", message);
                    break;
                }
            }
        });

        s.spawn(|| unsafe {
            channel.send("hello world 1!");
        });
    });
}

/// There's one sender thread and one receiver thread, and one one-shot channel between them.
pub fn run_example2() {
    let channel = Channel::new();

    let t = thread::current();

    thread::scope(|s| {
        s.spawn(|| {
            unsafe {
                channel.send("hello world 2!");
            }
            t.unpark();
        });

        while !channel.is_ready() {
            thread::park();
        }

        let message = unsafe { channel.recv() };
        println!("{message}");
        assert_eq!("hello world 2!", message);
    });
}
