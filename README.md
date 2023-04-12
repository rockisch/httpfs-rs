# httpfs-rs

Reimplementation of `python -m http.server` in rust.

As of writing, requires Rust Nightly (1.70-nigthly).

## Extras

- Relatively minimal dependencies
- Request handling is done with async using tokio
- Minimal allocations
- Handles `HEAD` requests correctly
- Handles HTTP1.1's chunked transfers (WIP)
