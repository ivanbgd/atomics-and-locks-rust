//! # Optimized Arc with Weak
//!
//! https://marabos.nl/atomics/building-arc.html#optimizing-arc
//!
//! While weak pointers can be useful, the `Arc` type is often used without any weak pointers.
//! This implementation optimizes `Arc` for that.
//!
//! At [Optimization of Compare-and-Exchange Loops](https://marabos.nl/atomics/hardware.html#compare-exchange-optimization),
//! it is advised, at least as of Rust 1.66, "to use the dedicated fetch-and-modify methods rather than a
//! compare-and-exchange loop, if possible".
//!
//! A little before that, at [ARMv8.1 Atomic Instructions](https://marabos.nl/atomics/hardware.html#armv81),
//! it is mentioned that newer ARM64 editions have new powerful instructions.
//!
//! So, compilers might not be up to speed, and we should listen to the advice.
//! Namely, it isn't easy for compilers to translate and optimize these loops.

use std::cell::UnsafeCell;
use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::ptr::NonNull;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::sync::atomic::{fence, AtomicBool, AtomicUsize};
use std::thread;

/// Maximum number of [`Arc`] or total references.
///
/// Going above this limit will abort the program.
const MAX_REF_COUNT: usize = isize::MAX as usize;

/// A special sentinel value which represents a special "locked" state of the combined pointer counter.
const LOCKED: usize = usize::MAX;

/// Holds data, strong count and combined count.
///
/// The combined count counts all [`Arc`] pointers combined as one single [`Weak`] pointer.
#[derive(Debug)]
struct ArcInner<T> {
    /// The data. Dropped if there are only weak pointers left.
    data: UnsafeCell<ManuallyDrop<T>>,
    /// Number of [`Arc`]s.
    strong_count: AtomicUsize,
    /// Combined number of [`Arc`]s and [`Weak`]s:
    /// counts all [`Arc`] pointers combined as one single [`Weak`] pointer.
    ///
    /// Number of [`Weak`]s, plus one if there are any [`Arc`]s.
    combined_count: AtomicUsize,
}

#[derive(Debug)]
pub struct Weak<T> {
    ptr: NonNull<ArcInner<T>>,
}

/// A thread-safe reference-counting pointer. `Arc` stands for "Atomically Reference Counted".
#[derive(Debug)]
pub struct Arc<T> {
    ptr: NonNull<ArcInner<T>>,
}

unsafe impl<T: Send + Sync> Send for Weak<T> {}
unsafe impl<T: Send + Sync> Sync for Weak<T> {}

unsafe impl<T: Send + Sync> Send for Arc<T> {}
unsafe impl<T: Send + Sync> Sync for Arc<T> {}

impl<T> Arc<T> {
    pub fn new(data: T) -> Self {
        let inner = ArcInner {
            data: UnsafeCell::new(ManuallyDrop::new(data)),
            strong_count: AtomicUsize::new(1),
            combined_count: AtomicUsize::new(1),
        };
        let ptr = NonNull::from(Box::leak(Box::new(inner)));

        Self { ptr }
    }

    /// Returns the [`ArcInner`] struct.
    fn get_inner(&self) -> &ArcInner<T> {
        // SAFETY: Pointer always points to valid `ArcInner<T>` as long as the `Arc` object exists.
        unsafe { self.ptr.as_ref() }
    }

    /// Returns `Some(&mut T)` if both counters are exactly one, which means that
    /// there is one strong count (one Arc) and no weak counts (no Weak pointers).
    ///
    /// `T` stands for the actual data protected by this `Arc`.
    ///
    /// Returns `None` otherwise.
    pub fn get_mut(arc: &mut Self) -> Option<&mut T> {
        // If combined count isn't one (if it's zero or more than one), we return None.
        //
        // If it is one, we briefly "lock" the combined counter so that we can "atomically" load
        // the strong count - "atomically" meaning at the same time in this context.
        // We want to check both counters "at the same time", so we use this "trick" with briefly
        // "locking" one of them.
        // So, we set the combined counter to a sentinel value and resume.
        // We'll restore its original value of one soon, right after we load strong count, and no later.
        //
        // Acquire matches Weak::drop()'s Release decrement, to make sure any
        // upgraded pointers are visible in the next strong_count.load().
        //
        // Not sure about this:
        // `compare_exchange()` doesn't require a loop in our source code, at least not on the x86-64 and ARM64
        // platforms, because it translates to assembly instructions in a loop on those two platforms.
        // Namely, reading https://marabos.nl/atomics/atomics.html#cas again, explains why a loop is needed
        // even with `compare_exchange()`, through an example.
        // // We do use a loop in the corresponding `Self::downgrade()` method, in which we check the value of
        // // `combined_count`, so perhaps that offsets the lack of loop here, or perhaps we simply don't need it here
        // // as we don't load the value before the `compare_exchange()` operation, and we don't load it because we don't need it.
        // We don't need a loop here because we are not trying to update the value "at any cost", so to speak.
        // We just want to check it once and update it if it succeeds, and return None otherwise.
        // `compare_exchange()` won't update the value if it fails, i.e., if the starting value wasn't one in this case.
        if arc
            .get_inner()
            .combined_count
            .compare_exchange(1, LOCKED, Acquire, Relaxed)
            .is_err()
        {
            return None;
        }

        // Load and compare the strong count with desired value of one.
        // We store this in a temporary/auxiliary variable so that we can restore combined count as quickly as possible.
        let is_arc_unique = arc.get_inner().strong_count.load(Relaxed) == 1;

        // As soon as we've loaded the strong count, we store back the original value of one to the combined counter.
        // Release matches with Acquire increment of combined_count in Self::downgrade().
        // We do this to make sure that any changes to the strong_count that come after Self::downgrade()
        // don't change the `is_arc_unique` result above.
        arc.get_inner().combined_count.store(1, Release);

        // We can check the strong count now.
        // We could have checked it before restoring the combined count, but that would have been an unnecessary delay.
        if !is_arc_unique {
            return None;
        }

        // Acquire to match Arc::drop()'s Release decrement, to make sure nothing else is accessing the data.
        fence(Acquire);

        unsafe { Some(&mut **arc.get_inner().data.get()) }
    }

    /// Increments the combined counter by one.
    pub fn downgrade(arc: &Self) -> Weak<T> {
        let mut cnt = arc.get_inner().combined_count.load(Relaxed);

        loop {
            if cnt == LOCKED {
                // This shouldn't take long, so we spin-loop (busy-wait),
                // but we at least hint that to the compiler which hints that to the CPU (if supported).
                //
                // We are waiting for `combined_count` to become "unlocked".
                // It becomes "locked" in `Self::get_mut()` - that is the only place where we "lock" it .
                std::hint::spin_loop();
                cnt = arc.get_inner().combined_count.load(Relaxed);
            } else {
                assert!(cnt < MAX_REF_COUNT);

                // Acquire synchronizes with Release store of combined_count in Self::get_mut().
                match arc.get_inner().combined_count.compare_exchange_weak(
                    cnt,
                    cnt + 1,
                    Acquire,
                    Relaxed,
                ) {
                    Ok(_) => return Weak { ptr: arc.ptr },
                    Err(e) => {
                        // Some other thread has modified the combined count in the meantime,
                        // so we have to update its value in this thread.
                        cnt = e
                    }
                }
            }
        }
    }
}

impl<T> Deref for Arc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let ptr = self.get_inner().data.get();
        // SAFETY: Since there is an Arc to the data, the data exists and can be shared.
        unsafe { &(*ptr) }
    }
}

impl<T> Weak<T> {
    /// Returns the [`ArcInner`] struct.
    fn get_inner(&self) -> &ArcInner<T> {
        // SAFETY: Pointer always points to valid `ArcInner<T>` as long as the `Weak` object exists.
        unsafe { self.ptr.as_ref() }
    }

    /// Upgrades weak to strong ([`Weak`] to [`Arc`]) if there is at least one `Arc`,
    /// i.e., if the strong count is at least one, and returns it.
    /// Increments the strong count by one, in this case.
    ///
    /// Otherwise, i.e., when strong count is zero, returns [`None`].
    pub fn upgrade(&self) -> Option<Arc<T>> {
        let mut cnt = self.get_inner().strong_count.load(Relaxed);
        loop {
            if cnt == 0 {
                return None;
            }

            assert!(cnt < MAX_REF_COUNT);

            match self.get_inner().strong_count.compare_exchange_weak(
                cnt,
                cnt + 1,
                Relaxed,
                Relaxed,
            ) {
                Ok(_) => return Some(Arc { ptr: self.ptr }),
                Err(e) => {
                    // Some other thread has modified the strong count in the meantime,
                    // so we have to update its value in this thread.
                    cnt = e
                }
            }
        }
    }
}

impl<T> Clone for Weak<T> {
    /// Increments the combined count by one.
    fn clone(&self) -> Self {
        if self.get_inner().combined_count.fetch_add(1, Relaxed) > MAX_REF_COUNT {
            std::process::abort();
        }

        Self { ptr: self.ptr }
    }
}

impl<T> Clone for Arc<T> {
    /// Increments the strong count by one.
    fn clone(&self) -> Self {
        if self.get_inner().strong_count.fetch_add(1, Relaxed) > MAX_REF_COUNT {
            std::process::abort();
        }

        Self { ptr: self.ptr }
    }
}

impl<T> Drop for Weak<T> {
    /// Decrements the combined count by one.
    fn drop(&mut self) {
        if self.get_inner().combined_count.fetch_sub(1, Release) == 1 {
            fence(Acquire);

            // SAFETY: This is the last total reference, so we can safely drop the object.
            unsafe {
                drop(Box::from_raw(self.ptr.as_ptr()));
            }
        }
    }
}

impl<T> Drop for Arc<T> {
    /// Decrements the strong count by one.
    ///
    /// Only dropping the very last [`Arc`] will decrement the weak pointer counter too.
    fn drop(&mut self) {
        if self.get_inner().strong_count.fetch_sub(1, Release) == 1 {
            fence(Acquire);

            // SAFETY: This was the last strong reference, so nothing else can access it.
            unsafe {
                ManuallyDrop::drop(&mut *self.get_inner().data.get());
            }

            // Now that there are no `Arc<T>`s left, drop the implicit weak pointer that
            // represented all `Arc<T>`s.
            drop(Weak { ptr: self.ptr });
        }
    }
}

/// Taken from the basic implementation of Arc, to validate that this new Arc can be used in the same way.
///
/// We changed AtomicUsize to AtomicBool.
pub fn run_example_basic() {
    static NUM_DROPS: AtomicBool = AtomicBool::new(false);

    struct DetectDrop;

    impl Drop for DetectDrop {
        fn drop(&mut self) {
            NUM_DROPS.store(true, Relaxed);
        }
    }

    let arc = Arc::new(("hello", DetectDrop));
    let arc_clone = Arc::clone(&arc);

    let t = thread::spawn(move || {
        assert_eq!("hello", arc_clone.0);
        assert!(!NUM_DROPS.load(Relaxed));
    });

    assert!(!NUM_DROPS.load(Relaxed));
    assert_eq!("hello", arc.0);

    t.join().unwrap();
    assert!(!NUM_DROPS.load(Relaxed));

    drop(arc);
    assert!(NUM_DROPS.load(Relaxed));
}

pub fn run_example_weak1() {
    static NUM_DROPS: AtomicBool = AtomicBool::new(false);

    struct DetectDrop;

    impl Drop for DetectDrop {
        fn drop(&mut self) {
            NUM_DROPS.store(true, Relaxed);
        }
    }

    let arc = Arc::new(("hello", DetectDrop));
    let weak1 = Arc::downgrade(&arc);
    let weak2 = Arc::downgrade(&arc);

    let t = thread::spawn(move || {
        // Weak pointer should be upgradeable at this point.
        let strong1 = weak1.upgrade().unwrap();
        assert_eq!("hello", strong1.0);
        assert!(!NUM_DROPS.load(Relaxed));
    });

    assert!(!NUM_DROPS.load(Relaxed));
    assert_eq!("hello", arc.0);

    t.join().unwrap();

    assert!(!NUM_DROPS.load(Relaxed));

    // Weak pointer should be upgradeable at this point.
    assert!(weak2.upgrade().is_some());
    assert!(!NUM_DROPS.load(Relaxed));

    drop(arc);

    // The data should be dropped by now, and the weak pointer should no longer be upgradeable.
    assert!(NUM_DROPS.load(Relaxed));
    assert!(weak2.upgrade().is_none());
}

pub fn run_example_weak2() {
    static NUM_DROPS: AtomicBool = AtomicBool::new(false);

    struct DetectDrop;

    impl Drop for DetectDrop {
        fn drop(&mut self) {
            NUM_DROPS.store(true, Relaxed);
        }
    }

    let arc = Arc::new(("hello", DetectDrop));
    let weak1 = Arc::downgrade(&arc);
    let weak2 = Arc::downgrade(&arc);

    let t = thread::spawn(move || {
        // Weak pointer should be upgradeable at this point.
        let strong1 = weak1.upgrade().unwrap();
        assert_eq!("hello", strong1.0);
        assert!(!NUM_DROPS.load(Relaxed));
    });

    assert!(!NUM_DROPS.load(Relaxed));
    assert_eq!("hello", arc.0);

    t.join().unwrap();

    assert!(!NUM_DROPS.load(Relaxed));

    // Weak pointer should be upgradeable at this point.
    let strong2 = weak2.upgrade().unwrap();
    assert_eq!("hello", strong2.0);
    assert!(!NUM_DROPS.load(Relaxed));

    drop(strong2);
    assert!(!NUM_DROPS.load(Relaxed));

    drop(weak2);
    assert!(!NUM_DROPS.load(Relaxed));

    drop(arc);
    assert!(NUM_DROPS.load(Relaxed));
}

pub fn test_weak_remains() {
    println!("\ntest_weak_remains()");

    let arc = Arc::new(5);

    println!("{arc:?}");
    let weak = Arc::downgrade(&arc);
    println!("{weak:?}");

    drop(arc);

    // Weak remains after dropping Arc.
    println!("{weak:?}");

    // Weak pointer should not be upgradeable at this point.
    let none_arc = weak.upgrade();
    println!("{none_arc:?}"); // None
    assert!(none_arc.is_none());

    drop(weak);

    println!("{none_arc:?}");
    assert!(none_arc.is_none());
}
