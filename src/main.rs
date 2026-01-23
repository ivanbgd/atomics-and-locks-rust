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

use atomics_and_locks_rust::ch9_locks::rwlock_3::run_example3;

fn main() {
    run_example3();
}
