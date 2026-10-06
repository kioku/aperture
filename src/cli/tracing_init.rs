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

fn resolve_log_level(verbosity: u8) -> String {
    if verbosity > 0 {
        return match verbosity {
            1 => "debug".to_string(),
            _ => "trace".to_string(),
        };
    }

    std::env::var("APERTURE_LOG").unwrap_or_else(|_| "warn".to_string())
}

fn resolve_log_format() -> String {
    std::env::var("APERTURE_LOG_FORMAT").map_or_else(|_| "text".to_string(), |s| s.to_lowercase())
}

fn resolve_writer() -> FileOrStderr {
    use std::fs::OpenOptions;
    use std::sync::Mutex;

    std::env::var("APERTURE_LOG_FILE").ok().map_or_else(
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
    )
}

fn diagnostic_registry(
    env_filter: EnvFilter,
) -> impl tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a> {
    use tracing_subscriber::layer::SubscriberExt;
    // Dependency transport events expose raw proxy/origin hosts, bypass lists
    // and protocol data outside the executor's omission boundary. Apply this
    // independently of user level directives, including explicit target levels.
    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::filter::filter_fn(|metadata| {
            matches!(
                metadata.target().split("::").next(),
                Some("aperture" | "aperture_cli")
            )
        }))
}

fn init_json_subscriber(env_filter: EnvFilter, writer: FileOrStderr) {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let json_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_span_list(false)
        .with_target(true)
        .with_thread_ids(false)
        .with_line_number(true)
        .with_writer(writer);
    diagnostic_registry(env_filter).with(json_layer).init();
}

fn init_text_subscriber(env_filter: EnvFilter, writer: FileOrStderr) {
    use tracing_subscriber::fmt::format::FmtSpan;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let fmt_layer = tracing_subscriber::fmt::layer()
        .pretty()
        .with_span_events(FmtSpan::CLOSE)
        .with_target(false)
        .with_thread_ids(false)
        .with_line_number(false)
        .with_writer(writer);
    diagnostic_registry(env_filter).with(fmt_layer).init();
}

/// Initialize tracing-subscriber for request/response logging.
pub fn init_tracing(verbosity: u8) {
    let log_level_str = resolve_log_level(verbosity);
    let env_filter = EnvFilter::try_new(&log_level_str)
        .or_else(|_| EnvFilter::try_new("error"))
        .unwrap_or_else(|_| EnvFilter::new("error"));

    let log_format = resolve_log_format();
    if log_format != "json" && log_format != "text" {
        // Tracing is not yet initialized; eprintln! is the only output channel available.
        // ast-grep-ignore: no-println
        eprintln!(
            "Warning: Unrecognized APERTURE_LOG_FORMAT '{log_format}'. Valid values: 'json', 'text'. Using 'text'."
        );
    }

    let writer = resolve_writer();

    match log_format.as_str() {
        "json" => init_json_subscriber(env_filter, writer),
        _ => init_text_subscriber(env_filter, writer),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::layer::SubscriberExt;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
        type Writer = Self;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn cli_trace_omits_dependency_routes_even_with_explicit_directives() {
        let capture = Capture::default();
        let subscriber =
            diagnostic_registry(EnvFilter::new("trace,reqwest=trace,hyper_util=trace")).with(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(capture.clone()),
            );
        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!(target: "reqwest::connect", "proxy http://localhost:8080/secret");
            tracing::trace!(target: "hyper_util::client::pool", "route secret.example");
            tracing::warn!(target: "rustls::connection", "reflected-secret");
            tracing::debug!(target: "aperture_rogue::transport", "prefix-secret");
            tracing::debug!(target: "aperture::executor", source = "cli", "Proxy configuration selected");
            tracing::info!(target: "aperture_cli::spec", "spec parsed");
        });
        let output = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
        assert!(!output.contains("localhost"), "{output}");
        assert!(!output.contains("secret"), "{output}");
        assert!(output.contains("Proxy configuration selected"));
        assert!(output.contains("spec parsed"));
    }
}
