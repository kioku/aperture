//! CLI command handlers extracted from main.rs.
//!
//! Each submodule handles a top-level command variant from [`Commands`].

// These modules contain CLI command handlers — not public library API.
// Suppress pedantic doc/hasher lints that are irrelevant for internal handlers.
#[allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::implicit_hasher
)]
pub mod api;
#[allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::implicit_hasher
)]
pub mod config;
#[allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]
pub mod docs;
#[allow(clippy::missing_errors_doc)]
pub mod search;
