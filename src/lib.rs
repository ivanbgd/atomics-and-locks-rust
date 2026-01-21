//! # Rust Atomics and Locks
//!
//! ### Low-Level Concurrency in Practice
//!
//! **Important note:** Build/run with optimizations turned on, i.e., in the *Release* profile.
//!
//! ```shell
//! cargo build --release
//! cargo run --release
//! rustc -O
//! ```
//!
//! Without optimization, the same code often results in many more instructions,
//! which can hide the subtle effects of instruction reordering.

pub mod ch1_condvar;
pub mod ch3_lazy_init;
pub mod ch4_spin_lock;
pub mod ch5_channels;
pub mod ch6_arc;
pub mod ch7_hardware;
pub mod ch9_locks;
