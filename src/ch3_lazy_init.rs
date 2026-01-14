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

/// Different execution paths are expected in different runs.
pub fn get_data(x: u64, y: u64) -> &'static Data {
    static PTR: AtomicPtr<Data> = AtomicPtr::new(std::ptr::null_mut());

    let mut p = PTR.load(Acquire);
    // eprintln!("AAA {:?} {:?}", thread::current(), PTR);
    eprintln!("AAA {:?} {:?}", thread::current().id(), p);

    if p.is_null() {
        p = Box::into_raw(Box::new(Data { x, y }));
        eprintln!("BBB {:?} {:?}", thread::current().id(), p);
        if let Err(err) = PTR.compare_exchange(std::ptr::null_mut(), p, Release, Acquire) {
            eprintln!("CCC1 {:?} {:?}", thread::current().id(), p);
            drop(unsafe { Box::from_raw(p) });
            p = err;
            eprintln!("CCC2 {:?} {:?}", thread::current().id(), p);
        }
    }

    eprintln!("DDD {:?} {:?}", thread::current().id(), p);
    // SAFETY: p is not null and points at a properly-initialized value.
    unsafe { &*p }
}

/// Different results are expected in different runs.
pub fn run_example() {
    let t1 = thread::spawn(|| {
        let r = get_data(3, 4);
        println!("{:?}", r);
        r
    });
    let t2 = thread::spawn(|| {
        let r = get_data(5, 6);
        println!("{:?}", r);
        r
    });

    let r1 = t1.join().unwrap();
    let r2 = t2.join().unwrap();
    println!();
    println!("{:?}", r1);
    println!("{:?}", r2);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_1() {
        let t1 = thread::spawn(|| {
            let r = get_data(3, 4);
            println!("{:?}", r);
            r
        });
        let t2 = thread::spawn(|| {
            let r = get_data(5, 6);
            println!("{:?}", r);
            r
        });

        let r1 = t1.join().unwrap();
        let r2 = t2.join().unwrap();
        assert!(
            ((r1.x, r1.y) == (3, 4) && (r2.x, r2.y) == (3, 4))
                || ((r1.x, r1.y) == (5, 6) && (r2.x, r2.y) == (5, 6))
        );
    }
}
