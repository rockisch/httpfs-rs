# httpfs-rs

Reimplementation of `python -m http.server` in rust.

As of writing, requires Rust Nightly (1.70-nigthly).

## Extras

- Minimal allocations
- Handles `HEAD` requests correctly
- Handles HTTP1.1's chunked transfers (WIP)
