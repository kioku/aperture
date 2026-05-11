//! Output abstraction for quiet mode support.
//!
//! This module provides a centralized way to control CLI output based on
//! quiet mode settings. It distinguishes between:
//! - Informational messages (suppressed in quiet mode)
//! - Success messages (suppressed in quiet mode)
//! - Tips/hints (suppressed in quiet mode)
//! - Data output (never suppressed)

use crate::error::Error;
use std::io::{self, ErrorKind, Write};

/// Write one line of data to stdout.
///
/// Broken pipes are treated as successful termination so commands compose with
/// consumers like `head` without panicking after the consumer exits early.
///
/// # Errors
///
/// Returns an error when writing to stdout fails for a reason other than a
/// broken pipe.
pub fn write_stdout_line(msg: &str) -> Result<(), Error> {
    let stdout = io::stdout();
    write_line(&mut stdout.lock(), msg)
}

/// Write data to stdout without appending a newline.
///
/// Broken pipes are treated as successful termination so commands compose with
/// consumers like `head` without panicking after the consumer exits early.
///
/// # Errors
///
/// Returns an error when writing to stdout fails for a reason other than a
/// broken pipe.
pub fn write_stdout(msg: &str) -> Result<(), Error> {
    let stdout = io::stdout();
    write_all(&mut stdout.lock(), msg.as_bytes())
}

fn write_line(writer: &mut impl Write, msg: &str) -> Result<(), Error> {
    write_all_fmt(writer, format_args!("{msg}\n"))
}

fn write_all(writer: &mut impl Write, bytes: &[u8]) -> Result<(), Error> {
    match writer.write_all(bytes) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::BrokenPipe => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn write_all_fmt(writer: &mut impl Write, args: std::fmt::Arguments<'_>) -> Result<(), Error> {
    match writer.write_fmt(args) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::BrokenPipe => Ok(()),
        Err(err) => Err(err.into()),
    }
}

#[allow(clippy::missing_panics_doc)]
pub fn write_stdout_line_or_panic(msg: &str) {
    write_stdout_line(msg).unwrap_or_else(|err| panic!("failed writing to stdout: {err}"));
}

#[macro_export]
macro_rules! stdoutln {
    () => {
        $crate::output::write_stdout_line_or_panic("");
    };
    ($($arg:tt)*) => {
        $crate::output::write_stdout_line_or_panic(&format!($($arg)*));
    };
}

/// Output handler that respects quiet mode.
///
/// Quiet mode is enabled if either `--quiet` is passed or `--json-errors` is used.
/// In quiet mode, only requested data and errors are output.
#[derive(Debug, Clone)]
pub struct Output {
    quiet: bool,
}

impl Output {
    /// Create new Output handler.
    ///
    /// Quiet mode is enabled if `--quiet` is passed OR `--json-errors` is passed.
    #[must_use]
    pub const fn new(quiet: bool, json_errors: bool) -> Self {
        Self {
            quiet: quiet || json_errors,
        }
    }

    /// Print informational message (suppressed in quiet mode).
    ///
    /// Use for general status messages like "Registered API specifications:".
    pub fn info(&self, msg: impl std::fmt::Display) {
        if !self.quiet {
            write_stdout_line_or_panic(&msg.to_string());
        }
    }

    /// Print success message (suppressed in quiet mode).
    ///
    /// Use for confirmation messages like "Spec 'foo' added successfully".
    pub fn success(&self, msg: impl std::fmt::Display) {
        if !self.quiet {
            write_stdout_line_or_panic(&msg.to_string());
        }
    }

    /// Print tip or hint (suppressed in quiet mode).
    ///
    /// Use for helpful suggestions like usage tips after commands.
    pub fn tip(&self, msg: impl std::fmt::Display) {
        if !self.quiet {
            write_stdout_line_or_panic(&msg.to_string());
        }
    }

    /// Check if quiet mode is enabled.
    #[must_use]
    pub const fn is_quiet(&self) -> bool {
        self.quiet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quiet_mode_from_quiet_flag() {
        let output = Output::new(true, false);
        assert!(output.is_quiet());
    }

    #[test]
    fn test_quiet_mode_from_json_errors() {
        let output = Output::new(false, true);
        assert!(output.is_quiet());
    }

    #[test]
    fn test_quiet_mode_both_flags() {
        let output = Output::new(true, true);
        assert!(output.is_quiet());
    }

    #[test]
    fn test_not_quiet_when_no_flags() {
        let output = Output::new(false, false);
        assert!(!output.is_quiet());
    }

    struct BrokenPipeWriter;

    impl std::io::Write for BrokenPipeWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "closed pipe",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct OtherErrorWriter;

    impl std::io::Write for OtherErrorWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_write_line_ignores_broken_pipe() {
        let mut writer = BrokenPipeWriter;
        assert!(write_line(&mut writer, "data").is_ok());
    }

    #[test]
    fn test_write_line_surfaces_other_errors() {
        let mut writer = OtherErrorWriter;
        assert!(write_line(&mut writer, "data").is_err());
    }

    #[test]
    fn test_write_all_ignores_broken_pipe() {
        let mut writer = BrokenPipeWriter;
        assert!(write_all(&mut writer, b"data").is_ok());
    }

    #[test]
    fn test_write_all_surfaces_other_errors() {
        let mut writer = OtherErrorWriter;
        assert!(write_all(&mut writer, b"data").is_err());
    }
}
