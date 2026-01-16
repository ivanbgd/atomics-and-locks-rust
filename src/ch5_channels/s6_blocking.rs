//! # Blocking
//!
//! https://marabos.nl/atomics/building-channels.html#blocking
//!
//! Has a blocking interface - for receiving.
//!
//! It is more convenient to use, as blocking is now done inside [`Receiver::recv()`].
//! This means that a user does not have to block manually anymore, when waiting for messages.
//!
//! Some flexibility has been lost as now only the thread that called [`Channel::split()`]
//! can receive data through [`Receiver::recv()`].

use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::{Acquire, Release};
use std::thread;
use std::time::Duration;

/// One-shot channel
///
/// Only the thread that calls [`Channel::split()`] may call [`Receiver::recv()`].
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
            unsafe {
                self.message.get_mut().assume_init_drop();
            }
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
    ///
    /// Only the thread that calls [`Channel::split()`] may call [`Receiver::recv()`].
    pub fn split<'a>(&'a mut self) -> (Sender<'a, T>, Receiver<'a, T>) {
        *self = Self::new();
        let sender = Sender {
            channel: self,
            receiving_thread: thread::current(),
        };
        let receiver = Receiver {
            channel: self,
            _no_send: PhantomData,
        };
        (sender, receiver)
    }
}

pub struct Sender<'a, T> {
    channel: &'a Channel<T>,
    receiving_thread: thread::Thread,
}

impl<T> Sender<'_, T> {
    /// Non-blocking send
    ///
    /// Unblocks the receiving thread after sending the message.
    ///
    /// Never panics.
    pub fn send(self, message: T) {
        unsafe {
            (*self.channel.message.get()).write(message);
        }
        self.channel.ready.store(true, Release);
        self.receiving_thread.unpark();
    }
}

pub struct Receiver<'a, T> {
    channel: &'a Channel<T>,
    _no_send: PhantomData<*const ()>,
}

impl<T> Receiver<'_, T> {
    /// Blocking receive
    ///
    /// Only the thread that calls [`Channel::split()`] may call [`Receiver::recv()`].
    ///
    /// Never panics.
    pub fn recv(self) -> T {
        while !self.channel.ready.swap(false, Acquire) {
            thread::park();
        }

        unsafe { (*self.channel.message.get()).assume_init_read() }
    }
}

pub fn run_example() {
    let mut channel = Channel::<&str>::new();
    let (tx, rx) = channel.split();

    thread::scope(|s| {
        s.spawn(|| {
            thread::sleep(Duration::from_secs(1));
            tx.send("Hello, blocking world!");
        });
    });

    let msg = rx.recv();
    println!("{msg}");
    assert_eq!("Hello, blocking world!", msg);
}
