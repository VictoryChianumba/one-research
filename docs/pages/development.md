# Development

[Back to docs](../README.md)

Build and run one-research from the workspace root:

```sh
cargo run -p one-research --release
```

Useful checks:

```sh
cargo fmt --check
cargo check -p one-research
cargo test -p one-research
cargo clippy --workspace --all-targets
```

The terminal reader is integrated from the sibling `tread` repo through the
path dependency in `one-research/Cargo.toml`; reader-internal changes belong there.
