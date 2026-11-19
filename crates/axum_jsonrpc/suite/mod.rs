//! Unit-test suite for `axum_jsonrpc` (kept out of Cargo `tests/` IT dir).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod batch;
mod common;
mod content_type;
mod dispatch;
mod errors;
mod http;
mod merge;
mod params;
mod parse;
