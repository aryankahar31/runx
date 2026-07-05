// Library facade so that integration tests in `tests/` can access internal
// modules without duplicating code.  The binary entry point remains `main.rs`.

pub mod cache;
pub mod config;
pub mod detect;
pub mod downloader;
pub mod error;
pub mod executor;
pub mod extractor;
pub mod runtime;
