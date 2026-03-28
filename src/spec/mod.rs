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

const REFERENCE_PLACEHOLDER: &str = "{reference}";

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

/// Resolves a schema reference to its actual schema definition
///
/// This function resolves top-level `$ref` references to schemas defined in
/// `#/components/schemas/`. It handles chained references (schema A references
/// schema B which references schema C) with circular reference detection.
///
/// # Arguments
/// * `spec` - The `OpenAPI` specification containing the components
/// * `reference` - The reference string (e.g., "#/components/schemas/User")
///
/// # Returns
/// * `Ok(Schema)` - The resolved schema
/// * `Err(Error)` - If resolution fails
///
/// # Errors
/// Returns an error if:
/// - The reference format is invalid
/// - The referenced schema doesn't exist
/// - Circular references are detected
/// - Maximum reference depth is exceeded
///
/// # Limitations
///
/// **Nested references are not resolved**: This function only resolves the
/// top-level schema reference. If the resolved schema contains nested `$ref`
/// within its properties, those remain unresolved. For example:
///
/// ```json
/// // #/components/schemas/Order resolves to:
/// {
///   "type": "object",
///   "properties": {
///     "customer": { "$ref": "#/components/schemas/Customer" }  // NOT resolved
///   }
/// }
/// ```
///
/// Implementing recursive resolution of nested references would require
/// traversing the entire schema tree, which adds complexity and risk of
/// infinite loops with self-referential schemas (e.g., a `User` with a
/// `friends: User[]` property).
pub fn resolve_schema_reference(
    spec: &OpenAPI,
    reference: &str,
) -> Result<openapiv3::Schema, Error> {
    let mut visited = HashSet::new();
    resolve_schema_reference_with_visited(spec, reference, &mut visited, 0)
}

/// Internal method that resolves schema references with circular reference detection
fn resolve_schema_reference_with_visited(
    spec: &OpenAPI,
    reference: &str,
    visited: &mut HashSet<String>,
    depth: usize,
) -> Result<openapiv3::Schema, Error> {
    let resolution = prepare_component_reference_resolution(
        spec,
        reference,
        visited,
        depth,
        "#/components/schemas/",
        format!(
            "Invalid schema reference format: '{reference}'. Expected format: #/components/schemas/{{name}}"
        ),
        "Cannot resolve schema reference: OpenAPI spec has no components section",
    )?;

    let schema_ref = resolution
        .components
        .schemas
        .get(&resolution.name)
        .ok_or_else(|| {
            Error::validation_error(format!(
                "Schema '{}' not found in components",
                resolution.name
            ))
        })?;

    match schema_ref {
        ReferenceOr::Item(schema) => Ok(schema.clone()),
        ReferenceOr::Reference {
            reference: nested_ref,
        } => resolve_schema_reference_with_visited(spec, nested_ref, visited, depth + 1),
    }
}

/// Internal method that resolves parameter references with circular reference detection
fn resolve_parameter_reference_with_visited(
    spec: &OpenAPI,
    reference: &str,
    visited: &mut HashSet<String>,
    depth: usize,
) -> Result<Parameter, Error> {
    let resolution = prepare_component_reference_resolution(
        spec,
        reference,
        visited,
        depth,
        "#/components/parameters/",
        format!(
            "Invalid parameter reference format: '{reference}'. Expected format: #/components/parameters/{{name}}"
        ),
        "Cannot resolve parameter reference: OpenAPI spec has no components section",
    )?;

    let param_ref = resolution
        .components
        .parameters
        .get(&resolution.name)
        .ok_or_else(|| {
            Error::validation_error(format!(
                "Parameter '{}' not found in components",
                resolution.name
            ))
        })?;

    match param_ref {
        ReferenceOr::Item(param) => Ok(param.clone()),
        ReferenceOr::Reference {
            reference: nested_ref,
        } => resolve_parameter_reference_with_visited(spec, nested_ref, visited, depth + 1),
    }
}

struct ComponentReferenceResolution<'a> {
    name: String,
    components: &'a openapiv3::Components,
}

#[allow(clippy::needless_pass_by_value)]
fn prepare_component_reference_resolution<'a>(
    spec: &'a OpenAPI,
    reference: &str,
    visited: &mut HashSet<String>,
    depth: usize,
    prefix: &str,
    invalid_format_message: String,
    missing_components_message: &str,
) -> Result<ComponentReferenceResolution<'a>, Error> {
    if depth >= MAX_REFERENCE_DEPTH {
        return Err(Error::validation_error(format!(
            "Maximum reference depth ({MAX_REFERENCE_DEPTH}) exceeded while resolving '{reference}'"
        )));
    }

    if !visited.insert(reference.to_string()) {
        return Err(Error::validation_error(format!(
            "Circular reference detected: '{reference}' is part of a reference cycle"
        )));
    }

    if !reference.starts_with(prefix) {
        return Err(Error::validation_error(
            invalid_format_message.replace(REFERENCE_PLACEHOLDER, reference),
        ));
    }

    let name = reference
        .strip_prefix(prefix)
        .ok_or_else(|| Error::validation_error(format!("Invalid reference: '{reference}'")))?;

    let components = spec
        .components
        .as_ref()
        .ok_or_else(|| Error::validation_error(missing_components_message.to_string()))?;

    Ok(ComponentReferenceResolution {
        name: name.to_string(),
        components,
    })
}
