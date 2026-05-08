//! `impl App` blocks split by concern. Each submodule contributes methods
//! to the App type via `impl App { ... }`. The submodules don't need to
//! be `pub` — methods are reachable on App regardless of which file
//! their impl block lives in.

mod process;
mod reader;
mod repo;
