use atomics_and_locks_rust::ch7_hardware::*;

fn main() {
    optimized_away();
    not_optimized();
    bg_load();
    bg_cas();
    bg_store();
    array_bg_store();
    array_bg_store_strided();
    array_bg_store_aligned();
    reordering_experiment_compiler_fence();
    reordering_experiment_fence();
}
