//! # Chapter 7. Understanding the Processor
//!
//! https://marabos.nl/atomics/hardware.html
//!
//! Talks about *x86-64* and *ARM64* ISAs, atomic instructions, cache,
//! compiler optimizations that can affect benchmarking, reordering, memory ordering (in CPUs).
//!
//! **Important note:** Run with optimizations turned on, i.e., in the *Release* profile.
//!
//! ```shell
//! cargo run --release
//! rustc -O
//! ```
//!
//! Without optimization, the same code often results in many more instructions,
//! which can hide the subtle effects of instruction reordering.
//!
//! We need to separate the two `ATOMIC_`s that we use in [`optimized_away()`] and [`not_optimized()`],
//! because if we use only one for both functions, the code might not be optimized in [`optimized_away()`],
//! but we'd like to see it be optimized away in that function.
//!
//! That depends on the Rust target triple. For some triples, and in case of a single common `ATOMIC` variable,
//! [`optimized_away()`] will indeed be optimized away (but not fully!), while for some other triples it won't
//! be optimized away at all, i.e., it will have the same execution length as [`not_optimized()`].
//! It's interesting that it may be partially optimized in such a case.
//!
//! So, in general, these functions can result in three different execution speeds (times), with three different
//! levels of optimization or lack of it, in case we use a common `ATOMIC` variable for them.
//!
//! The bottom line is that we should use two separate `ATOMIC_` variables for the two functions
//! to be able to reliably see differences between them.
//!
//! **Important note:** Actually, in general, we should not put unrelated atomic variables close to each other!
//! This is particularly important if we store them in an array. We should not have dense arrays of atomic variables.
//! This includes mutexes and similar. We should space them apart. We have examples below.
//! This is related to caching and issues in cache lines resulting in the false sharing effect.
//! On the other hand, when multiple (atomic) variables are related and often accessed in quick succession,
//! it can be good to put them close together.
//!
//! [`bg_load()`] performance also depends on the target triple.
//! It could be of the same speed as [`not_optimized()`], or significantly slower than it!
//!
//! Apple M2 Pro (ARM64: ARMv8.6-A) seems to have cache line of the 128 byte size.
//! The *ARM64* architecture supports 64 and 128 bytes cache lines, but measurements performed on a particular
//! CPU show drastic difference in execution speed (time) when using striding to circumvent the false sharing effect.
//! On this particular CPU, false sharing is not solved when cache line was assumed to be 64 bytes,
//! but it was solved when cache line was considered to be 128 bytes.
//!
//! x86-64 seems to have cache line of the 64 byte size. If we use a larger stride, assuming a 128-byte cache line,
//! that might even slow the execution down.
//!
//! **Important note:** Keep in mind that not all ARM64 CPUs are the same! There are different generations (versions)
//! of them, and they have different capabilities.
//! See [An Experiment](https://marabos.nl/atomics/hardware.html#reordering-experiment) in the book.
//!
//! # Running All Examples
//!
//! ## Apple M2 Pro (ARM64: ARMv8.6-A) macOS Run
//! - Optimized: 42ns
//! - Unoptimized: 308.044334ms
//! - With background load: 297.785083ms
//! - With background cas: 305.175667ms
//! - With background store: 469.119459ms
//! - With array & background store: 481.861416ms
//! - With array & background store strided: 299.096417ms  The stride is 16 elements or 128 bytes. The alignment is 8 bytes.
//! - With array & background store aligned: 306.608375ms  The stride is 1 element or 128 bytes. The alignment is 128 bytes.
//! - reordering_experiment_compiler_fence: 225.427584ms, counter = 3996221 (or 3990333 or 3999887 or ...) - without `std::hint::spin_loop();`
//!     - 125.127458ms with `std::hint::spin_loop();`
//!     - In Debug mode, without optimizations, counter = 4000000! Execution speed is lower, naturally.
//! - reordering_experiment_fence: 236.701125ms, counter = 4000000 - without `std::hint::spin_loop();`
//!     - 234.962334ms with `std::hint::spin_loop();`
//!
//! ## x86-64 Linux Run @ Rust Playground
//! - Optimized: 90ns
//! - Unoptimized: 319.994728ms
//! - With background load: 613.967669ms
//! - With background cas: 2.800181547s
//! - With background store: 2.834904964s
//! - With array & background store: 4.538064287s
//! - With array & background store strided: 2.489058667s  The stride is 8 elements or 64 bytes. The alignment is 8 bytes.
//! - With array & background store aligned: 2.262635902s  The stride is 1 element or 64 bytes. The alignment is 64 bytes.
//! - reordering_experiment_compiler_fence: 126.593857ms usually, counter = 4000000 - without `std::hint::spin_loop();` - as low as 59.701365ms
//!     - 94.438222ms with `std::hint::spin_loop();` - as low as 82.212583ms
//! - reordering_experiment_fence: 130.153356ms usually, counter = 4000000 - without `std::hint::spin_loop();` - as low as 66.092554ms
//!     - 94.602893ms with `std::hint::spin_loop();` - as low as 70.348412ms

use std::hint::black_box;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::sync::atomic::{compiler_fence, fence, AtomicBool, AtomicU64, AtomicUsize};
use std::thread;
use std::time::Instant;

const NUM_ITERS: usize = 1_000_000_000;

static ATOMIC1: AtomicU64 = AtomicU64::new(0);

/// https://marabos.nl/atomics/hardware.html#performance-experiment
///
/// Basically optimized away, i.e., fully, by the compiler.
///
/// Run as: `cargo run --release`
pub fn optimized_away() {
    let start = Instant::now();
    for _ in 0..NUM_ITERS {
        ATOMIC1.load(Relaxed);
    }
    println!("Optimized: {:?}", start.elapsed());
}

static ATOMIC2: AtomicU64 = AtomicU64::new(0);

/// https://marabos.nl/atomics/hardware.html#performance-experiment
///
/// Uses the black box compiler hint to try to avoid being optimized (away).
///
/// Run as: `cargo run --release`
pub fn not_optimized() {
    // Might not be necessary (on some triplets at least), but we should keep it in general case.
    black_box(&ATOMIC2);

    let start = Instant::now();
    for _ in 0..NUM_ITERS {
        black_box(ATOMIC2.load(Relaxed));
    }
    println!("Unoptimized: {:?}", start.elapsed());
}

static ATOMIC3: AtomicU64 = AtomicU64::new(0);

/// https://marabos.nl/atomics/hardware.html#performance-experiment
pub fn bg_load() {
    black_box(&ATOMIC3);

    thread::spawn(|| {
        loop {
            black_box(ATOMIC3.load(Relaxed));
        }
    });

    let start = Instant::now();
    for _ in 0..NUM_ITERS {
        black_box(ATOMIC3.load(Relaxed));
    }
    println!("With background load: {:?}", start.elapsed());
}

static ATOMIC4: AtomicU64 = AtomicU64::new(0);

/// https://marabos.nl/atomics/hardware.html#performance-experiment
pub fn bg_store() {
    black_box(&ATOMIC4);

    thread::spawn(|| {
        loop {
            ATOMIC4.store(0, Relaxed);
        }
    });

    let start = Instant::now();
    for _ in 0..NUM_ITERS {
        black_box(ATOMIC4.load(Relaxed));
    }
    println!("With background store: {:?}", start.elapsed());
}

static ATOMIC5: AtomicU64 = AtomicU64::new(0);

/// https://marabos.nl/atomics/hardware.html#performance-experiment
pub fn bg_cas() {
    black_box(&ATOMIC5);

    thread::spawn(|| {
        loop {
            // Never succeeds, because ATOMIC5 is never 10.
            let _ = black_box(ATOMIC5.compare_exchange(10, 20, Relaxed, Relaxed));
        }
    });

    let start = Instant::now();
    for _ in 0..NUM_ITERS {
        black_box(ATOMIC5.load(Relaxed));
    }
    println!("With background cas: {:?}", start.elapsed());
}

const NUM_ELTS: usize = 3;

/// Contains [`NUM_ELTS`] (three) elements.
static ATOMIC_ARRAY: [AtomicU64; NUM_ELTS] =
    [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

/// https://marabos.nl/atomics/hardware.html#performance-experiment
///
/// False sharing
pub fn array_bg_store() {
    black_box(&ATOMIC_ARRAY);

    thread::spawn(|| {
        loop {
            ATOMIC_ARRAY[0].store(0, Relaxed);
            ATOMIC_ARRAY[2].store(0, Relaxed);
        }
    });

    let start = Instant::now();
    for _ in 0..NUM_ITERS {
        black_box(ATOMIC_ARRAY[1].load(Relaxed));
    }
    println!("With array & background store: {:?}", start.elapsed());
}

/// Apple M2 Pro seems to have cache line of the 128 byte size: 128 / 8 = 16.
/// So, the stride is 16 elements or 128 bytes.
/// We should use each 16-th element, i.e., the first element of each 16-element block.
/// For each useful element, we lose 15 elements memory-wise, but we gain execution speed.
/// [`AtomicU64`] has size of eight bytes, hence the `/ 8` in calculations.
#[cfg(all(target_arch = "aarch64", target_vendor = "apple", target_os = "macos"))]
const STRIDE: usize = 16;
/// The array has 48 elements of which only 3 are useful as array items,
/// and the rest are used to improve the runtime performance.
#[cfg(all(target_arch = "aarch64", target_vendor = "apple", target_os = "macos"))]
static STRIDED_ATOMIC_ARRAY: [AtomicU64; NUM_ELTS * STRIDE] =
    [const { AtomicU64::new(0) }; NUM_ELTS * STRIDE];

/// x86-64 seems to have cache line of the 64 byte size: 64 / 8 = 8.
/// So, the stride is 8 elements or 64 bytes.
/// We'll also use it for other architectures.
/// We should use each 8-th element, i.e., the first element of each 8-element block.
/// For each useful element, we lose 7 elements memory-wise, but we gain execution speed.
/// [`AtomicU64`] has size of eight bytes, hence the `/ 8` in calculations.
#[cfg(not(all(target_arch = "aarch64", target_vendor = "apple", target_os = "macos")))]
const STRIDE: usize = 8;
/// The array has 24 elements of which only 3 are useful as array items,
/// and the rest are used to improve the runtime performance.
#[cfg(not(all(target_arch = "aarch64", target_vendor = "apple", target_os = "macos")))]
static STRIDED_ATOMIC_ARRAY: [AtomicU64; NUM_ELTS * STRIDE] =
    [const { AtomicU64::new(0) }; NUM_ELTS * STRIDE];

/// https://marabos.nl/atomics/hardware.html#performance-experiment
///
/// Solving false sharing through striding
pub fn array_bg_store_strided() {
    black_box(&STRIDED_ATOMIC_ARRAY);

    thread::spawn(|| {
        loop {
            STRIDED_ATOMIC_ARRAY[0].store(0, Relaxed);
            STRIDED_ATOMIC_ARRAY[STRIDE * 2].store(0, Relaxed);
        }
    });

    let start = Instant::now();
    for _ in 0..NUM_ITERS {
        black_box(STRIDED_ATOMIC_ARRAY[STRIDE].load(Relaxed));
    }
    print!(
        "With array & background store strided: {:?}  ",
        start.elapsed()
    );

    print!(
        "The stride is {} elements or {} bytes. ",
        STRIDED_ATOMIC_ARRAY.len() / NUM_ELTS,
        size_of::<[AtomicU64; NUM_ELTS * STRIDE]>() / NUM_ELTS,
    );
    println!(
        "The alignment is {} bytes.",
        align_of_val(&STRIDED_ATOMIC_ARRAY)
    );
}

/// This struct is 128-byte aligned.
#[cfg(all(target_arch = "aarch64", target_vendor = "apple", target_os = "macos"))]
#[repr(align(128))]
struct Aligned(AtomicU64);

/// This struct is 64-byte aligned.
#[cfg(not(all(target_arch = "aarch64", target_vendor = "apple", target_os = "macos")))]
#[repr(align(64))]
struct Aligned(AtomicU64);

/// Contains (only) three elements, but with a large enough alignment (spacing) to avoid the false sharing effect.
static ALIGNED_ATOMIC_ARRAY: [Aligned; NUM_ELTS] = [const { Aligned(AtomicU64::new(0)) }; NUM_ELTS];

/// https://marabos.nl/atomics/hardware.html#performance-experiment
///
/// Solving false sharing through alignment
pub fn array_bg_store_aligned() {
    black_box(&ALIGNED_ATOMIC_ARRAY);

    thread::spawn(|| {
        loop {
            ALIGNED_ATOMIC_ARRAY[0].0.store(0, Relaxed);
            ALIGNED_ATOMIC_ARRAY[2].0.store(0, Relaxed);
        }
    });

    let start = Instant::now();
    for _ in 0..NUM_ITERS {
        black_box(ALIGNED_ATOMIC_ARRAY[1].0.load(Relaxed));
    }
    print!(
        "With array & background store aligned: {:?}  ",
        start.elapsed()
    );

    print!(
        "The stride is {} element or {} bytes. ",
        ALIGNED_ATOMIC_ARRAY.len() / NUM_ELTS,
        size_of_val(&ALIGNED_ATOMIC_ARRAY) / NUM_ELTS,
    );
    println!(
        "The alignment is {} bytes.",
        align_of_val(&ALIGNED_ATOMIC_ARRAY)
    );
}

const NUM_THREADS: usize = 4;
const NUM_ITERS_REORDER: usize = 1_000_000;

/// [Reordering Experiment](https://marabos.nl/atomics/hardware.html#reordering-experiment)
///
/// [`compiler_fence()`] prevents only the compiler from reordering atomic memory operations.
///
/// It doesn't prevent the processor from reordering them.
///
/// This experiment uses wrong memory orderings for acquiring and releasing the lock and [`compiler_fence()`]
/// with correct memory ordering. It modifies a variable in a non-atomic way when we have a lock.
///
/// Correct memory orderings during acquiring and releasing the lock would not require any fence.
/// They are slower than with wrong memory ordering, but correct.
pub fn reordering_experiment_compiler_fence() {
    let locked = AtomicBool::new(false);
    let counter = AtomicUsize::new(0);

    let start = Instant::now();
    thread::scope(|s| {
        // Spawn four threads, that each iterate a million times.
        for _ in 0..NUM_THREADS {
            s.spawn(|| {
                for _ in 0..NUM_ITERS_REORDER {
                    // Acquire the lock, using the wrong memory ordering.
                    // The correct ordering would be "Acquire". We don't need a fence in that case.
                    // It is slower than with wrong memory ordering, but correct.
                    while locked.swap(true, Relaxed) {
                        // Speeds things up on both ARM64 and x86-64.
                        // std::hint::spin_loop();
                    }
                    compiler_fence(Acquire);

                    // Non-atomically increment the counter, while holding the lock.
                    let old = counter.load(Relaxed);
                    let new = old + 1;
                    counter.store(new, Relaxed);

                    // Release the lock, using the wrong memory ordering.
                    // The correct ordering would be "Release". We don't need a fence in that case.
                    // It is slower than with wrong memory ordering, but correct.
                    compiler_fence(Release);
                    locked.store(false, Relaxed);
                }
            });
        }
    });

    let inner = counter.into_inner();
    println!(
        "reordering_experiment_compiler_fence: {:?}, counter = {}",
        start.elapsed(),
        inner
    );
    if cfg!(target_arch = "x86_64") {
        assert_eq!(NUM_THREADS * NUM_ITERS_REORDER, inner);
    } else {
        assert!(inner <= NUM_THREADS * NUM_ITERS_REORDER);
    }
}

/// [Reordering Experiment](https://marabos.nl/atomics/hardware.html#reordering-experiment)
///
/// [`fence()`] prevents both the compiler and the processor from reordering atomic memory operations.
///
/// I've added this example.
pub fn reordering_experiment_fence() {
    let locked = AtomicBool::new(false);
    let counter = AtomicUsize::new(0);

    let start = Instant::now();
    thread::scope(|s| {
        // Spawn four threads, that each iterate a million times.
        for _ in 0..NUM_THREADS {
            s.spawn(|| {
                for _ in 0..NUM_ITERS_REORDER {
                    // Acquire the lock, using the wrong memory ordering.
                    while locked.swap(true, Relaxed) {
                        // On Apple M2 Pro (ARM64) the speed is the same. On x86-64 it speeds things up.
                        // std::hint::spin_loop();
                    }
                    // If we try with `compiler_fence` instead, it breaks on ARM64.
                    fence(Acquire);

                    // Non-atomically increment the counter, while holding the lock.
                    let old = counter.load(Relaxed);
                    let new = old + 1;
                    counter.store(new, Relaxed);

                    // Release the lock, using the wrong memory ordering.
                    // This one might pass on Apple M2 Pro (ARM64) with only `compiler_fence` instead of `fence`.
                    fence(Release);
                    locked.store(false, Relaxed);
                }
            });
        }
    });

    let inner = counter.into_inner();
    println!(
        "reordering_experiment_fence: {:?}, counter = {}",
        start.elapsed(),
        inner
    );
    assert_eq!(NUM_THREADS * NUM_ITERS_REORDER, inner);
}
