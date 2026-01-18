use atomics_and_locks_rust::ch6_arc::s3_optimized::*;

fn main() {
    run_example_basic();
    run_example_weak1();
    run_example_weak2();
    test_weak_remains();
}
