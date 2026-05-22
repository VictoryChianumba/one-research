# Development

[Back to docs](../README.md)

Build and run trench from the workspace root:

```sh
cargo run -p trench --release
```

Useful checks:

```sh
cargo fmt --check
cargo check -p trench
cargo test -p trench
cargo clippy --workspace --all-targets
```

The terminal reader is integrated from the sibling `tread` repo through the
path dependency in `trench/Cargo.toml`; reader-internal changes belong there.
