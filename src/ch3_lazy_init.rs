//! # Chapter 3: Memory Ordering
//!
//! [Example: Lazy Initialization with Indirection](https://marabos.nl/atomics/memory-ordering.html#example-lazy-initialization-with-indirection)

use std::sync::atomic::AtomicPtr;
use std::sync::atomic::Ordering::{Acquire, Release};
use std::thread;

#[derive(Debug, Clone)]
pub struct Data {
    pub x: u64,
    pub y: u64,
}

pub fn get_data(x: u64, y: u64) -> &'static Data {
    static PTR: AtomicPtr<Data> = AtomicPtr::new(std::ptr::null_mut());

    let mut p = PTR.load(Acquire);
    // println!("AAA {:?} {:?}", thread::current(), PTR);
    println!("AAA {:?} {:?}", thread::current().id(), p);

    if p.is_null() {
        p = Box::into_raw(Box::new(Data {x, y}));
        println!("BBB {:?} {:?}", thread::current().id(), p);
        if let Err(err) = PTR.compare_exchange(std::ptr::null_mut(), p, Release, Acquire) {
            println!("CCC1 {:?} {:?}", thread::current().id(), p);
            drop(unsafe { Box::from_raw(p) });
            p = err;
            println!("CCC2 {:?} {:?}", thread::current().id(), p);
        }
    }

    println!("DDD {:?} {:?}", thread::current().id(), p);
    // SAFETY: p is not null and points at a properly-initialized value.
    unsafe { &*p }
}

pub fn run_example() {
    let t1 = thread::spawn(|| { let r = get_data(3, 4); println!("{:?}", r); r });
    let t2 = thread::spawn(|| { let r = get_data(5, 6); println!("{:?}", r); r });

    let r1 = t1.join().unwrap();
    let r2 = t2.join().unwrap();
    println!();
    println!("{:?}", r1);
    println!("{:?}", r2);
}
