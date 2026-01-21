# Rust Atomics and Locks

### Low-Level Concurrency in Practice

https://marabos.nl/atomics/

## Running

**Important note:** Run with optimizations turned on, i.e., in the *Release* profile.

```shell
cargo run --release
rustc -O
```

Without optimization, the same code often results in many more instructions,
which can hide the subtle effects of instruction reordering.

## Testing

Test in *Release* mode.

```shell
cargo test --release
```
