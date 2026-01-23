//! # Rust Atomics and Locks
//!
//! ### Low-Level Concurrency in Practice
//!
//! ## Running
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

use atomics_and_locks_rust::ch9_locks::*;

fn main() {
    std::thread::sleep(std::time::Duration::from_secs(3));
    rwlock_1::run_example3();
    std::thread::sleep(std::time::Duration::from_secs(3));
    rwlock_2::run_example3();
    std::thread::sleep(std::time::Duration::from_secs(3));
    rwlock_3::run_example3();
}
