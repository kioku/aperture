//! Bind clap's selected header values to their caller/capture segments.
use super::{interpolation, BatchOperation};
use crate::error::Error;
use reqwest::header::{HeaderName, HeaderValue};

type Segments = Vec<(String, bool)>;

fn provenance_error() -> Error {
    Error::validation_error("Invalid batch header provenance (input omitted)")
}

/// Build the same argv as batch translation, retaining the origin of each byte.
fn argument_segments(
    operation: &BatchOperation,
    store: &interpolation::VariableStore,
    extra_body_file: &[String],
) -> Result<Vec<Segments>, Error> {
    let id = operation
        .id
        .as_deref()
        .unwrap_or(crate::constants::DEFAULT_OPERATION_NAME);
    let mut args = operation
        .args
        .iter()
        .map(|arg| interpolation::interpolate_segments(arg, store, id))
        .collect::<Result<Vec<_>, _>>()?;
    args.extend(extra_body_file.iter().map(|arg| vec![(arg.clone(), false)]));
    let mut headers: Vec<_> = operation.headers.iter().collect();
    headers.sort_by_key(|(name, _)| *name);
    for (name, value) in headers {
        args.push(vec![("--header".to_string(), true)]);
        let mut segments = vec![(format!("{name}: "), true)];
        segments.extend(interpolation::interpolate_segments(value, store, id)?);
        args.push(segments);
    }
    Ok(args)
}

fn text(segments: &Segments) -> String {
    segments.iter().map(|(value, _)| value.as_str()).collect()
}

fn suffix(segments: &Segments, start: usize) -> Segments {
    let mut offset = 0;
    segments
        .iter()
        .filter_map(|(value, authored)| {
            let from = start.saturating_sub(offset).min(value.len());
            offset += value.len();
            (from < value.len()).then(|| (value[from..].to_string(), *authored))
        })
        .collect()
}

fn attached_value_start(value: &str) -> Option<usize> {
    if value.starts_with("--") {
        return value.find('=').map(|position| position + 1);
    }
    if value.starts_with("-H") && value.len() > 2 {
        return Some(if value.as_bytes()[2] == b'=' { 3 } else { 2 });
    }
    None
}

pub(super) fn overlay_map_headers(
    headers: &mut Option<Vec<(HeaderName, HeaderValue)>>,
    count: usize,
) {
    if let Some(headers) = headers.as_mut() {
        headers.rotate_right(count);
    }
}

/// Clap indexes tokens after splitting `--option=value` and attached `-Hvalue`.
/// Verify the selected bytes too: a mapping mismatch fails before delivery.
fn indexed_arguments(args: &[Segments]) -> std::collections::HashMap<usize, Segments> {
    let mut indexed = std::collections::HashMap::new();
    let mut index = 1;
    for segments in args {
        let value = text(segments);
        let split = attached_value_start(&value);
        if let Some(start) = split {
            index += 1;
            indexed.insert(index, suffix(segments, start));
        } else {
            indexed.insert(index, segments.clone());
        }
        index += 1;
    }
    indexed
}

pub(super) fn resolve_headers(
    operation: &BatchOperation,
    store: &interpolation::VariableStore,
    extra_body_file: &[String],
    matches: &clap::ArgMatches,
) -> Result<Vec<(HeaderName, HeaderValue)>, Error> {
    let args = argument_segments(operation, store, extra_body_file)?;
    let raw: Vec<_> = args.iter().map(text).collect();
    let offset = crate::engine::generator::batch_operation_argument_offset(&raw)?;
    let indexed = indexed_arguments(&args[offset..]);
    let mut leaf = matches;
    while let Some((_, child)) = leaf.subcommand() {
        leaf = child;
    }
    let values = leaf
        .try_get_many::<String>("header")
        .ok()
        .flatten()
        .into_iter()
        .flatten();
    let indices = leaf.indices_of("header").into_iter().flatten();
    values
        .zip(indices)
        .map(|(value, index)| {
            let segments = indexed.get(&index).ok_or_else(provenance_error)?;
            if text(segments) != *value {
                return Err(provenance_error());
            }
            crate::engine::executor::parse_custom_header_segments(segments)
        })
        .collect()
}
