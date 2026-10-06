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

impl BuildInfo {
    /// Concise version identity for Clap; full revision remains available in build-info.
    #[must_use]
    pub fn version_label(&self) -> String {
        let revision = self.revision.get(..7).unwrap_or(self.revision);
        format!("{} ({revision}, {})", self.version, self.source_state)
    }
}

/// Stable storage for Clap versions without enabling owned-string builder support.
#[must_use]
pub fn cli_version() -> &'static str {
    static VERSION: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| current().version_label());
    VERSION.as_str()
}

#[cfg(test)]
mod tests {
    use super::BuildInfo;

    #[test]
    fn version_label_supports_validated_object_ids_and_archives() {
        for revision in [
            "0123456789abcdef0123456789abcdef01234567",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ] {
            for state in ["clean", "dirty", "unknown"] {
                let info = BuildInfo {
                    version: "1.2.3",
                    revision,
                    source_state: state,
                };
                assert_eq!(info.version_label(), format!("1.2.3 (0123456, {state})"));
            }
        }
        let info = BuildInfo {
            version: "1.2.3",
            revision: "unknown",
            source_state: "unknown",
        };
        assert_eq!(info.version_label(), "1.2.3 (unknown, unknown)");
    }
}
