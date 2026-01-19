//! # Weak Pointers
//!
//! https://marabos.nl/atomics/building-arc.html#weak-pointers
//!
//! Weak pointers help break cycles and can also keep a reference to the allocation
//! inside `Arc` around if there is currently no `Arc` (strong reference) around.
//!
//! A weak pointer does not prevent the value stored in the allocation from being dropped.
//! It will be dropped if the strong count goes down to zero.
//!
//! Circular (mutual) references are possible in case of tree data structure, for example,
//! where a parent node points to its children nodes, and children nodes point to their parent node.
//! Without weak references, mutual strong references would prevent `Arc` from being dropped.
//! So, parents can hold strong references to children, and children can hold weak references to parents.
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
use std::ops::Deref;
use std::ptr::NonNull;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::sync::atomic::{fence, AtomicBool, AtomicUsize};
use std::thread;

/// Maximum number of [`Arc`] or total references.
///
/// Going above this limit will abort the program.
const MAX_REF_COUNT: usize = isize::MAX as usize;

/// Holds data, strong count and total (strong plus weak) count.
#[derive(Debug)]
struct ArcInner<T> {
    /// The data. `None` if there's only one weak pointer left.
    data: UnsafeCell<Option<T>>,
    /// Number of [`Arc`]s.
    strong_count: AtomicUsize,
    /// Total number of [`Arc`]s and [`Weak`]s combined.
    total_count: AtomicUsize,
}

/// Contains pointer to [`ArcInner`].
#[derive(Debug)]
pub struct Weak<T> {
    ptr: NonNull<ArcInner<T>>,
}

/// A thread-safe reference-counting pointer. `Arc` stands for "Atomically Reference Counted".
///
/// Contains [`Weak`].
#[derive(Debug)]
pub struct Arc<T> {
    weak: Weak<T>,
}

unsafe impl<T: Send + Sync> Send for Weak<T> {}
unsafe impl<T: Send + Sync> Sync for Weak<T> {}

impl<T> Arc<T> {
    pub fn new(data: T) -> Self {
        let inner = ArcInner {
            data: UnsafeCell::new(Some(data)),
            strong_count: AtomicUsize::new(1),
            total_count: AtomicUsize::new(1),
        };
        let ptr = NonNull::from(Box::leak(Box::new(inner)));
        let weak = Weak { ptr };

        Self { weak }
    }

    /// Returns `Some(&mut T)` if total count is exactly one, i.e., there is
    /// one strong count (one Arc) and no weak counts (no Weak pointers).
    ///
    /// `T` stands for the actual data protected by this `Arc`.
    ///
    /// Returns `None` otherwise.
    pub fn get_mut(arc: &mut Self) -> Option<&mut T> {
        if arc.weak.get_inner().total_count.load(Relaxed) == 1 {
            fence(Acquire);

            // SAFETY: This is the only strong reference and we hold it. This is the only Arc.
            // Total reference count is one, and there are no weak references.
            let inner = unsafe { arc.weak.ptr.as_mut() };
            let option = inner.data.get_mut();
            // We know that the data is still available as we hold Arc to it, so this won't panic.
            let data = option.as_mut().expect("expected some data");

            Some(data)
        } else {
            None
        }
    }

    /// Increments the total count (strong plus weak) by one.
    pub fn downgrade(arc: &Self) -> Weak<T> {
        arc.weak.clone()
    }
}

impl<T> Deref for Arc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let ptr = self.weak.get_inner().data.get();
        // SAFETY: Since there is an Arc to the data, the data exists and can be shared.
        unsafe { (*ptr).as_ref().expect("expected some value") }
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
    /// Increments the strong count and the total (strong + weak) count, both by one, in this case.
    ///
    /// Otherwise, i.e., when strong count is zero, returns [`None`].
    pub fn upgrade(&self) -> Option<Arc<T>> {
        let mut cnt = self.get_inner().strong_count.load(Relaxed);
        loop {
            if cnt == 0 {
                return None;
            }

            assert!(cnt < MAX_REF_COUNT);

            // Not sure about this:
            // Since we use `compare_exchange_weak()` instead of `compare_exchange()`, we have to do it in a loop,
            // because on the ARM64 architecture it doesn't loop internally, unlike `compare_exchange()`, which does.
            // On x86-64 `compare_exchange()` and `compare_exchange_weak()` are the same, and they both loop internally.
            // So, we need to loop here in case `compare_exchange_weak()` doesn't loop on an architecture,
            // such as ARM64, for example.
            // What we mean by "internally" is when the operation gets translated to assembly.
            // Namely, reading https://marabos.nl/atomics/atomics.html#cas again, explains why a loop is needed
            // even with `compare_exchange()`, through an example.
            //
            // At https://marabos.nl/atomics/hardware.html#compare-exchange-optimization, it is advised, at least
            // as of Rust 1.66, "to use the dedicated fetch-and-modify methods rather than a compare-and-exchange loop,
            // if possible".
            // A little before that, at https://marabos.nl/atomics/hardware.html#armv81, it is mentioned that newer
            // ARM64 editions have new powerful instructions.
            // So, compilers might not be up to speed, and we should listen to the advice.
            // Namely, it isn't easy for compilers to translate and optimize these loops.
            match self.get_inner().strong_count.compare_exchange_weak(
                cnt,
                cnt + 1,
                Relaxed,
                Relaxed,
            ) {
                Ok(_) => {
                    return Some(Arc {
                        // `Weak::clone()` increments the total count (strong plus weak) by one.
                        weak: Self::clone(self),
                    });
                }
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
    /// Increments the total count (strong plus weak) by one.
    fn clone(&self) -> Self {
        if self.get_inner().total_count.fetch_add(1, Relaxed) > MAX_REF_COUNT {
            std::process::abort();
        }

        Self { ptr: self.ptr }
    }
}

impl<T> Clone for Arc<T> {
    /// Increments the strong count and the total (strong + weak) count, both by one.
    fn clone(&self) -> Self {
        // `Weak::clone()` increments the total count (strong plus weak) by one.
        let weak = Weak::clone(&self.weak);

        if self.weak.get_inner().strong_count.fetch_add(1, Relaxed) > MAX_REF_COUNT {
            std::process::abort();
        }

        Self { weak }
    }
}

impl<T> Drop for Weak<T> {
    /// Decrements the total count (strong plus weak) by one.
    fn drop(&mut self) {
        if self.get_inner().total_count.fetch_sub(1, Release) == 1 {
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
    fn drop(&mut self) {
        if self.weak.get_inner().strong_count.fetch_sub(1, Release) == 1 {
            fence(Acquire);

            let ptr = self.weak.get_inner().data.get();

            // SAFETY: This is the last strong reference, so nothing else can access it.
            unsafe {
                *ptr = None;
            }
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
