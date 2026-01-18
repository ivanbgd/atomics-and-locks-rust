//! # Basic Atomic Reference Counting
//!
//! https://marabos.nl/atomics/building-arc.html#basic-reference-counting
//!
//! We can't use [`Box<ArcInner<T>>`] (because that would be exclusive ownership) or `&ArcInner<T>`
//! (because of lifetimes) to handle the allocation of [`ArcInner<T>`].
//!
//! Instead, we have to use a pointer and handle the allocation and ownership manually.
//!
//! Instead of raw pointers (`*const T` or `*mut T`), we use [`NonNull`].
//! That way [`Option<Arc<T>>`] has the same size as [`Arc<T>`], using the null-pointer representation for [`None`].

use std::ops::Deref;
use std::process::abort;
use std::ptr::NonNull;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::sync::atomic::{fence, AtomicBool, AtomicUsize};
use std::thread;

/// Maximum number of [`Arc`] references.
///
/// Going above this limit will abort the program.
const MAX_REF_COUNT: usize = isize::MAX as usize;

#[derive(Debug)]
struct ArcInner<T> {
    data: T,
    ref_count: AtomicUsize,
}

/// A thread-safe reference-counting pointer. `Arc` stands for "Atomically Reference Counted".
#[derive(Debug)]
pub struct Arc<T> {
    ptr: NonNull<ArcInner<T>>,
}

unsafe impl<T: Send + Sync> Send for Arc<T> {}
unsafe impl<T: Send + Sync> Sync for Arc<T> {}

impl<T> Arc<T> {
    pub fn new(data: T) -> Self {
        let ptr = NonNull::from(Box::leak(Box::new(ArcInner {
            data,
            ref_count: AtomicUsize::new(1),
        })));

        Self { ptr }
    }

    fn get_inner(&self) -> &ArcInner<T> {
        // SAFETY: Pointer always points to valid `ArcInner<T>` as long as the `Arc` object exists.
        unsafe { self.ptr.as_ref() }
    }

    pub fn get_mut(arc: &mut Self) -> Option<&mut T> {
        if arc.get_inner().ref_count.load(Relaxed) == 1 {
            fence(Acquire);

            // SAFETY: This is the last reference, so we can safely return data.
            unsafe { Some(&mut arc.ptr.as_mut().data) }
        } else {
            None
        }
    }
}

impl<T> Deref for Arc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.get_inner().data
    }
}

impl<T> Clone for Arc<T> {
    fn clone(&self) -> Self {
        if self.get_inner().ref_count.fetch_add(1, Relaxed) > MAX_REF_COUNT {
            abort();
        }

        Self { ptr: self.ptr }
    }
}

impl<T> Drop for Arc<T> {
    fn drop(&mut self) {
        if self.get_inner().ref_count.fetch_sub(1, Release) == 1 {
            fence(Acquire);

            // SAFETY: This is the last reference, so we can safely drop the object.
            unsafe {
                drop(Box::from_raw(self.ptr.as_ptr()));
            }
        }
    }
}

pub fn run_example() {
    // This could be AtomicBool instead.
    static NUM_DROPS: AtomicUsize = AtomicUsize::new(0);

    struct DetectDrop;

    impl Drop for DetectDrop {
        fn drop(&mut self) {
            NUM_DROPS.fetch_add(1, Relaxed);
        }
    }

    let arc = Arc::new(("hello", DetectDrop));
    // let arc_clone = arc.clone();
    let arc_clone = Arc::clone(&arc);

    let t = thread::spawn(move || {
        assert_eq!("hello", arc_clone.0);
        assert_eq!(0, NUM_DROPS.load(Relaxed));
    });

    assert_eq!(0, NUM_DROPS.load(Relaxed));

    assert_eq!("hello", arc.0);

    t.join().unwrap();

    assert_eq!(0, NUM_DROPS.load(Relaxed));

    drop(arc);

    assert_eq!(1, NUM_DROPS.load(Relaxed));
}

/// We are not proving anything in this test function, i.e., that `dropped` was set to `true`.
/// Namely, we can't prove it, because the flag is contained within the `DetectDrop` struct, so we can't
/// access the flag after dropping the last instance (manually, in the main thread).
pub fn _run_example() {
    struct DetectDrop {
        dropped: AtomicBool,
    }

    impl Drop for DetectDrop {
        fn drop(&mut self) {
            self.dropped.store(true, Relaxed);
        }
    }

    let arc = Arc::new((
        "hello",
        DetectDrop {
            dropped: AtomicBool::new(false),
        },
    ));
    // let arc_clone = arc.clone();
    let arc_clone = Arc::clone(&arc);

    let t = thread::spawn(move || {
        assert_eq!("hello", arc_clone.0);
        assert!(!arc_clone.1.dropped.load(Relaxed));
    });

    assert!(!arc.1.dropped.load(Relaxed));

    assert_eq!("hello", arc.0);

    t.join().unwrap();

    assert!(!arc.1.dropped.load(Relaxed));

    drop(arc);
}
