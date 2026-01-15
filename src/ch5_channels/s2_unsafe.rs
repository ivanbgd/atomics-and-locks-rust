//! # An Unsafe One-Shot Channel
//!
//! https://marabos.nl/atomics/building-channels.html#an-unsafe-one-shot-channel
//!
//! This variant has non-blocking receive. User needs to block if they wish.

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::{Acquire, Release};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

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

/// There's one sender thread (child) and one receiver thread (also child), and one one-shot channel between them.
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
                } else {
                    thread::yield_now();
                }
            }
        });

        s.spawn(|| unsafe {
            thread::sleep(Duration::from_secs(1));
            channel.send("hello world 1!");
        });
    });
}

/// There's one sender thread (child) and one receiver thread (parent/main), and one one-shot channel between them.
pub fn run_example2() {
    let channel = Channel::new();

    let t = thread::current();

    thread::scope(|s| {
        s.spawn(|| {
            thread::sleep(Duration::from_secs(1));
            unsafe {
                channel.send("hello world 2!");
            }
            t.unpark();
        });
    });

    while !channel.is_ready() {
        thread::park();
    }

    let message = unsafe { channel.recv() };
    println!("{message}");
    assert_eq!("hello world 2!", message);
}

/// There's one sender thread (child) and one receiver thread (also child), and one one-shot channel between them.
///
/// I don't think this is a good usage pattern.
pub fn run_example3() {
    let channel = Channel::new();
    let channel = Arc::new(channel);
    let channel_clone = Arc::clone(&channel);

    let t = thread::current();

    let snd = thread::spawn(move || {
        thread::sleep(Duration::from_secs(1));
        unsafe {
            channel.send("hello world 3!");
        }
        t.unpark();
    });

    snd.join().unwrap();

    let rcv = thread::spawn(move || {
        while !channel_clone.is_ready() {
            thread::park();
        }

        let message = unsafe { channel_clone.recv() };
        println!("{message}");
        assert_eq!("hello world 3!", message);
    });

    rcv.join().unwrap();
}
