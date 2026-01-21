//! # Borrowing to Avoid Allocation
//!
//! https://marabos.nl/atomics/building-channels.html#borrowing-to-avoid-allocation
//!
//! Slightly more performant than the previous version (Safety through types: the Arc-based version),
//! but also a little less convenient to use.
//!
//! In order to optimize for efficiency, we trade some convenience for performance
//! by making the user responsible for the shared [`Channel`] object.
//!
//! The reduction in convenience compared to the Arc-based version is minimal,
//! as it requires only one more line of code when using it, to create a channel.

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
    /// Has another send already finished?
    ready: AtomicBool,
}

unsafe impl<T: Send> Sync for Channel<T> {}

impl<T> Default for Channel<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for Channel<T> {
    fn drop(&mut self) {
        if *self.ready.get_mut() {
            unsafe { self.message.get_mut().assume_init_drop() }
        }
    }
}

impl<T> Channel<T> {
    /// New channel
    pub const fn new() -> Self {
        Self {
            message: UnsafeCell::new(MaybeUninit::uninit()),
            ready: AtomicBool::new(false),
        }
    }

    /// Returns sending and receiving sides of the channel.
    pub fn split<'a>(&'a mut self) -> (Sender<'a, T>, Receiver<'a, T>) {
        *self = Self::new();
        (Sender { channel: self }, Receiver { channel: self })
    }
}

pub struct Sender<'a, T> {
    channel: &'a Channel<T>,
}

impl<T> Sender<'_, T> {
    /// Non-blocking send
    ///
    /// Never panics.
    pub fn send(self, message: T) {
        unsafe {
            (*self.channel.message.get()).write(message);
        }
        self.channel.ready.store(true, Release);
    }
}

pub struct Receiver<'a, T> {
    channel: &'a Channel<T>,
}

impl<T> Receiver<'_, T> {
    /// Returns whether a message is stored (available) in the channel.
    pub fn is_ready(&self) -> bool {
        self.channel.ready.load(Relaxed)
    }

    /// Non-blocking receive
    ///
    /// Panics if no message is available yet.
    ///
    /// Tip: Use [`Channel::is_ready`] to check first.
    pub fn recv(self) -> T {
        if !self.channel.ready.swap(false, Acquire) {
            panic!("no message available");
        }
        unsafe { (*self.channel.message.get()).assume_init_read() }
    }
}

pub fn run_example() {
    let mut channel = Channel::new();
    let (tx, rx) = channel.split();

    let t = thread::current();

    thread::scope(|s| {
        s.spawn(|| {
            thread::sleep(Duration::from_secs(1));
            tx.send("Hello, borrowing world!");
            t.unpark();
        });

        while !rx.is_ready() {
            thread::park();
        }
    });

    let msg = rx.recv();
    println!("{msg}");
    assert_eq!("Hello, borrowing world!", msg);
}
