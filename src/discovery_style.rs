//! Semantic ANSI styling for human-facing discovery output.
//!
//! Styling is enabled only when stdout is a TTY and `NO_COLOR` is not set.

use std::io::IsTerminal;

#[derive(Debug, Clone, Copy)]
pub struct DiscoveryStyle {
    enabled: bool,
}

impl DiscoveryStyle {
    #[must_use]
    pub fn for_stdout() -> Self {
        let no_color = std::env::var_os("NO_COLOR");
        Self::from_detection(std::io::stdout().is_terminal(), no_color.as_deref())
    }

    #[must_use]
    pub const fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    #[must_use]
    const fn from_detection(is_stdout_tty: bool, no_color: Option<&std::ffi::OsStr>) -> Self {
        Self::new(is_stdout_tty && no_color.is_none())
    }

    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub fn api_title(self, text: impl AsRef<str>) -> String {
        self.wrap("1;97", text)
    }

    #[must_use]
    pub fn heading(self, text: impl AsRef<str>) -> String {
        self.wrap("1", text)
    }

    #[must_use]
    pub fn metadata(self, text: impl AsRef<str>) -> String {
        self.wrap("2", text)
    }

    #[must_use]
    pub fn next_label(self, text: impl AsRef<str>) -> String {
        self.wrap("36", text)
    }

    #[must_use]
    pub fn required(self, text: impl AsRef<str>) -> String {
        self.wrap("1;33", text)
    }

    #[must_use]
    pub fn success(self, text: impl AsRef<str>) -> String {
        self.wrap("32", text)
    }

    #[must_use]
    pub fn warning(self, text: impl AsRef<str>) -> String {
        self.wrap("33", text)
    }

    #[must_use]
    pub fn method(self, method: &str) -> String {
        let code = match method.to_ascii_uppercase().as_str() {
            "GET" => "32",
            "POST" => "34",
            "PUT" => "33",
            "PATCH" => "36",
            "DELETE" => "31",
            _ => "1",
        };
        self.wrap(code, method.to_ascii_uppercase())
    }

    #[must_use]
    pub fn muted_count(self, text: impl AsRef<str>) -> String {
        self.metadata(text)
    }

    #[must_use]
    fn wrap(self, code: &str, text: impl AsRef<str>) -> String {
        if !self.enabled {
            return text.as_ref().to_string();
        }

        format!("\x1b[{code}m{}\x1b[0m", text.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::DiscoveryStyle;

    #[test]
    fn enabled_styles_wrap_with_ansi() {
        let style = DiscoveryStyle::new(true);
        let rendered = style.method("GET");
        assert!(rendered.starts_with("\u{1b}[32mGET"));
        assert!(rendered.ends_with("\u{1b}[0m"));
    }

    #[test]
    fn disabled_styles_return_plain_text() {
        let style = DiscoveryStyle::new(false);
        assert_eq!(style.heading("Usage"), "Usage");
    }

    #[test]
    fn no_color_disables_even_when_tty() {
        let style = DiscoveryStyle::from_detection(true, Some(std::ffi::OsStr::new("1")));
        assert!(!style.is_enabled());
    }

    #[test]
    fn empty_no_color_value_disables_when_tty() {
        let style = DiscoveryStyle::from_detection(true, Some(std::ffi::OsStr::new("")));
        assert!(!style.is_enabled());
    }

    #[test]
    fn method_normalizes_lowercase_tokens() {
        let style = DiscoveryStyle::new(false);
        assert_eq!(style.method("get"), "GET");
    }

    #[test]
    fn method_falls_back_to_bold_for_unknown_tokens() {
        let style = DiscoveryStyle::new(true);
        assert_eq!(style.method("BREW"), "\u{1b}[1mBREW\u{1b}[0m");
    }

    #[test]
    fn method_handles_empty_or_whitespace_tokens_safely() {
        let style = DiscoveryStyle::new(false);
        assert_eq!(style.method(""), "");
        assert_eq!(style.method("   "), "   ");
    }
}
