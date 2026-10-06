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
    if value.starts_with("--header=") {
        return Some("--header=".len());
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

/// Select header values from physical argv, rather than reconstructing clap's
/// virtual indices (which also count implicit boolean values). Operation options
/// do not consume hyphen-leading values, and `--` ends option interpretation.
/// The complete selected sequence is verified against clap before execution.
fn header_arguments(args: &[Segments]) -> Result<Vec<Segments>, Error> {
    let mut headers = Vec::new();
    let mut args = args.iter();
    while let Some(segments) = args.next() {
        let value = text(segments);
        match value.as_str() {
            "--" => break,
            "--header" | "-H" => {
                headers.push(args.next().cloned().ok_or_else(provenance_error)?);
            }
            _ => {
                if let Some(start) = attached_value_start(&value) {
                    headers.push(suffix(segments, start));
                }
            }
        }
    }
    Ok(headers)
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
    let headers = header_arguments(&args[offset..])?;
    let mut leaf = matches;
    while let Some((_, child)) = leaf.subcommand() {
        leaf = child;
    }
    let values: Vec<_> = leaf
        .try_get_many::<String>("header")
        .ok()
        .flatten()
        .into_iter()
        .flatten()
        .collect();
    if values.len() != headers.len() {
        return Err(provenance_error());
    }
    values
        .into_iter()
        .zip(headers)
        .map(|(value, segments)| {
            if text(&segments) != *value {
                return Err(provenance_error());
            }
            crate::engine::executor::parse_custom_header_segments(&segments)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_selection_retains_origins_without_virtual_indices() {
        let args = vec![
            vec![("--flag".into(), true)],
            vec![("--label=X-Data: ${TOKEN}".into(), true)],
            vec![("-H".into(), true)],
            vec![("X-Data: ${TOKEN}".into(), false)],
            vec![("--header=X-Other: ".into(), true), ("é".into(), false)],
            vec![("-H=X-Last: literal".into(), true)],
        ];
        let selected = header_arguments(&args).unwrap();
        assert_eq!(selected.len(), 3);
        assert_eq!(selected[0], vec![("X-Data: ${TOKEN}".into(), false)]);
        assert_eq!(
            selected[1],
            vec![("X-Other: ".into(), true), ("é".into(), false)]
        );
        assert_eq!(text(&selected[2]), "X-Last: literal");
    }

    #[test]
    fn delimiter_and_missing_header_value_are_fail_closed() {
        let args = vec![
            vec![("--label=-Hfake".into(), true)],
            vec![("--".into(), true)],
            vec![("--header=X-Data: ${TOKEN}".into(), false)],
        ];
        assert!(header_arguments(&args).unwrap().is_empty());
        assert!(header_arguments(&[vec![("-H".into(), true)]]).is_err());
        assert!(header_arguments(&[vec![("--header".into(), true)]]).is_err());
    }
}
