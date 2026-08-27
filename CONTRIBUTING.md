# Contributing

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release
```

Prefer resilience and correctness over micro-optimizations. Do not load whole files into memory.
