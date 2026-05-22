# Getting Started

[Back to docs](../README.md)

trench is a Rust terminal UI. Install Rust 1.88 or newer, then build from the
workspace root:

```sh
git clone https://github.com/VictoryChianumba/trench
cd trench
cargo build -p trench --release
./target/release/trench
```

Install it into `~/.cargo/bin` from a local checkout with:

```sh
cargo install --path trench
trench
```

The first run creates `~/.config/trench/config.json`. API keys are optional:

- `claude_api_key` for Claude chat and AI source discovery.
- `openai_api_key` for OpenAI chat.
- `github_token` for the repository viewer.
- `semantic_scholar_key` for citation enrichment.
- `core_api_key` to enable CORE ingestion.
- `perplexity_api_key` for discovery web search.

Use `Ldr+S` (`Ctrl+T`, then `S`) to open Settings in the TUI. The root
[`README.md`](../../README.md) has the current config schema and keybinding
overview.
