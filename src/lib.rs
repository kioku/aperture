pub mod agent;
pub mod atomic;
pub mod batch;
pub mod cache;
pub mod cli;
pub mod config;
pub mod constants;
pub mod docs;
pub mod duration;
pub mod engine;
pub mod error;
pub mod fs;
pub mod interactive;
pub mod invocation;
pub mod logging;
pub mod output;
pub mod pagination;
pub mod resilience;
pub mod response_cache;
pub mod search;
pub mod shortcuts;
pub mod spec;
pub mod suggestions;
pub mod utils;

// Unit tests in src/ call reqwest::Client directly and have no access to
// tests/test_helpers.rs. This block installs the platform provider once per
// unit-test binary so those clients don't hit reqwest's no-provider panic.
#[cfg(test)]
mod test_crypto_init {
    #[ctor::ctor]
    fn init() {
        #[cfg(not(windows))]
        let _ = rustls::crypto::ring::default_provider().install_default();
        #[cfg(windows)]
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    }
}
