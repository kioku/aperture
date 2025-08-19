//! `OpenAPI` specification validation and transformation module
//!
//! This module separates the concerns of validating and transforming `OpenAPI` specifications
//! into distinct, testable components following the Single Responsibility Principle.

use crate::constants;

pub mod parser;
pub mod transformer;
pub mod validator;

pub use parser::parse_openapi;
pub use transformer::SpecTransformer;
pub use validator::SpecValidator;

use crate::error::Error;
use openapiv3::{OpenAPI, Operation, Parameter, PathItem, ReferenceOr};
use std::collections::HashSet;

/// A helper type to iterate over all HTTP methods in a `PathItem`
pub type HttpMethodsIter<'a> = [(&'static str, &'a Option<Operation>); 8];

/// Creates an iterator over all HTTP methods and their operations in a `PathItem`
///
/// # Arguments
/// * `item` - The `PathItem` to extract operations from
///
/// # Returns
/// An array of tuples containing the HTTP method name and its optional operation
#[must_use]
pub const fn http_methods_iter(item: &PathItem) -> HttpMethodsIter<'_> {
    [
        (constants::HTTP_METHOD_GET, &item.get),
        (constants::HTTP_METHOD_POST, &item.post),
        (constants::HTTP_METHOD_PUT, &item.put),
        (constants::HTTP_METHOD_DELETE, &item.delete),
        (constants::HTTP_METHOD_PATCH, &item.patch),
        (constants::HTTP_METHOD_HEAD, &item.head),
        (constants::HTTP_METHOD_OPTIONS, &item.options),
        ("TRACE", &item.trace),
    ]
}

/// Maximum depth for resolving parameter references to prevent stack overflow
pub const MAX_REFERENCE_DEPTH: usize = 10;

/// Resolves a parameter reference to its actual parameter definition
///
/// # Arguments
/// * `spec` - The `OpenAPI` specification containing the components
/// * `reference` - The reference string (e.g., "#/components/parameters/userId")
///
/// # Returns
/// * `Ok(Parameter)` - The resolved parameter
/// * `Err(Error)` - If resolution fails
///
/// # Errors
/// Returns an error if:
/// - The reference format is invalid
/// - The referenced parameter doesn't exist
/// - Circular references are detected
/// - Maximum reference depth is exceeded
pub fn resolve_parameter_reference(spec: &OpenAPI, reference: &str) -> Result<Parameter, Error> {
    let mut visited = HashSet::new();
    resolve_parameter_reference_with_visited(spec, reference, &mut visited, 0)
}

/// Internal method that resolves parameter references with circular reference detection
fn resolve_parameter_reference_with_visited(
    spec: &OpenAPI,
    reference: &str,
    visited: &mut HashSet<String>,
    depth: usize,
) -> Result<Parameter, Error> {
    // Check depth limit
    if depth >= MAX_REFERENCE_DEPTH {
        return Err(Error::validation_error(format!(
            "Maximum reference depth ({MAX_REFERENCE_DEPTH}) exceeded while resolving '{reference}'"
        )));
    }

    // Check for circular references
    if !visited.insert(reference.to_string()) {
        return Err(Error::validation_error(format!(
            "Circular reference detected: '{reference}' is part of a reference cycle"
        )));
    }

    // Parse the reference path
    // Expected format: #/components/parameters/{parameter_name}
    if !reference.starts_with("#/components/parameters/") {
        return Err(Error::validation_error(format!(
            "Invalid parameter reference format: '{reference}'. Expected format: #/components/parameters/{{name}}"
        )));
    }

    let param_name = reference
        .strip_prefix("#/components/parameters/")
        .ok_or_else(|| {
            Error::validation_error(format!("Invalid parameter reference: '{reference}'"))
        })?;

    // Look up the parameter in components
    let components = spec.components.as_ref().ok_or_else(|| {
        Error::validation_error(
            "Cannot resolve parameter reference: OpenAPI spec has no components section"
                .to_string(),
        )
    })?;

    let param_ref = components.parameters.get(param_name).ok_or_else(|| {
        Error::validation_error(format!("Parameter '{param_name}' not found in components"))
    })?;

    // Handle nested references (reference pointing to another reference)
    match param_ref {
        ReferenceOr::Item(param) => Ok(param.clone()),
        ReferenceOr::Reference {
            reference: nested_ref,
        } => resolve_parameter_reference_with_visited(spec, nested_ref, visited, depth + 1),
    }
}
