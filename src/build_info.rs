/// Immutable source identity embedded by the builder, independent of CLI or
/// runtime configuration. A dirty revision identifies its base, not its edits.
#[derive(serde::Serialize)]
pub struct BuildInfo {
    pub version: &'static str,
    pub revision: &'static str,
    pub source_state: &'static str,
}

#[must_use]
pub const fn current() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION"),
        revision: env!("APERTURE_SOURCE_REVISION"),
        source_state: env!("APERTURE_SOURCE_STATE"),
    }
}
