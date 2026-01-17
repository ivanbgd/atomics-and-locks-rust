//! # Safety Through Types
//!
//! https://marabos.nl/atomics/building-channels.html#safety-through-types
//!
//! Very convenient to use, at the cost of some performance, as it has to allocate memory.
//!
//! The runtime overhead comes from the split ownership of Channel between Sender and Receiver,
//! because of which we have to use [`Arc`].
//!
//! On the other hand, this variant catches mistakes at compile time.
//! Namely, [`Sender::send`] and [`Receiver::recv`] take ownership of `self`,
//! so the user cannot call them twice.
//!
//! This improves convenience of use for the user.
//!
//! This also improves performance, as it results in a reduced number of runtime checks
//! for things that are now statically guaranteed.
//!
//! Non-blocking send & non-blocking receive. User needs to block if they wish.

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Creates a channel and returns it through a sender-receiver pair.
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let channel = Channel {
        message: UnsafeCell::new(MaybeUninit::uninit()),
        ready: AtomicBool::new(false),
    };
    let channel = Arc::new(channel);
    let sender = Sender {
        channel: Arc::clone(&channel),
    };
    let receiver = Receiver { channel };

    (sender, receiver)
}

pub struct Sender<T> {
    channel: Arc<Channel<T>>,
}

impl<T> Sender<T> {
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

pub struct Receiver<T> {
    channel: Arc<Channel<T>>,
}

impl<T> Receiver<T> {
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
        // SAFETY: We've just checked (and reset) the ready flag.
        unsafe { (*self.channel.message.get()).assume_init_read() }
    }
}

/// One-shot channel
#[derive(Debug)]
struct Channel<T> {
    /// Message item
    message: UnsafeCell<MaybeUninit<T>>,
    /// Has another `send` already finished?
    ready: AtomicBool,
}

unsafe impl<T: Send> Sync for Channel<T> {}

impl<T> Drop for Channel<T> {
    fn drop(&mut self) {
        if *self.ready.get_mut() {
            unsafe { self.message.get_mut().assume_init_drop() }
        }
    }
}

/// There's one sender thread (child) and one receiver thread (parent/main), and one one-shot channel between them.
pub fn run_example() {
    let (sender, receiver) = channel::<&str>();

    let t = thread::current();

    thread::scope(|s| {
        s.spawn(|| {
            thread::sleep(Duration::from_secs(1));
            let message = "Hello, types world!";
            sender.send(message);
            t.unpark();
        });
    });

    // We have to block manually waiting for a message, as the `Receiver::recv()` method is non-blocking.
    // We park the thread to achieve that.
    while !receiver.is_ready() {
        thread::park();
    }

    let message = receiver.recv();
    println!("{message}");
    assert_eq!("Hello, types world!", message);
}
