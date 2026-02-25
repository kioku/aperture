//! Tracing/logging initialization for the CLI.

use tracing_subscriber::EnvFilter;

/// Wrapper type to write logs to file or stderr.
struct FileOrStderr {
    file: Option<std::sync::Mutex<std::fs::File>>,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for FileOrStderr {
    type Writer = Box<dyn std::io::Write + 'a>;

    fn make_writer(&'a self) -> Self::Writer {
        self.file
            .as_ref()
            .and_then(|mutex| mutex.lock().ok())
            .and_then(|file| file.try_clone().ok())
            .map_or_else(
                || Box::new(std::io::stderr()) as Self::Writer,
                |cloned| Box::new(cloned) as Self::Writer,
            )
    }
}

/// Initialize tracing-subscriber for request/response logging.
pub fn init_tracing(verbosity: u8) {
    use std::fs::OpenOptions;
    use std::sync::Mutex;
    use tracing_subscriber::fmt::format::FmtSpan;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let log_level_str = if verbosity > 0 {
        match verbosity {
            1 => "debug".to_string(),
            _ => "trace".to_string(),
        }
    } else {
        std::env::var("APERTURE_LOG").unwrap_or_else(|_| "error".to_string())
    };

    let env_filter = EnvFilter::try_new(&log_level_str)
        .or_else(|_| EnvFilter::try_new("error"))
        .unwrap_or_else(|_| EnvFilter::new("error"));

    let log_format = std::env::var("APERTURE_LOG_FORMAT")
        .map_or_else(|_| "text".to_string(), |s| s.to_lowercase());

    if log_format != "json" && log_format != "text" {
        // Tracing is not yet initialized; eprintln! is the only output channel available.
        // ast-grep-ignore: no-println
        eprintln!(
            "Warning: Unrecognized APERTURE_LOG_FORMAT '{log_format}'. Valid values: 'json', 'text'. Using 'text'."
        );
    }

    let writer = std::env::var("APERTURE_LOG_FILE").ok().map_or_else(
        || FileOrStderr { file: None },
        |path| match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => FileOrStderr {
                file: Some(Mutex::new(file)),
            },
            Err(e) => {
                // Tracing is not yet initialized; eprintln! is the only output channel available.
                // ast-grep-ignore: no-println
                eprintln!("Warning: Could not open log file '{path}': {e}. Using stderr.");
                FileOrStderr { file: None }
            }
        },
    );

    if log_format == "json" {
        let json_layer = tracing_subscriber::fmt::layer()
            .json()
            .with_span_list(false)
            .with_target(true)
            .with_thread_ids(false)
            .with_line_number(true)
            .with_writer(writer);
        tracing_subscriber::registry()
            .with(env_filter)
            .with(json_layer)
            .init();
    } else {
        let fmt_layer = tracing_subscriber::fmt::layer()
            .pretty()
            .with_span_events(FmtSpan::CLOSE)
            .with_target(false)
            .with_thread_ids(false)
            .with_line_number(false)
            .with_writer(writer);
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .init();
    }
}
