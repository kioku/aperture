//! `OpenAPI` specification validation and transformation module
//!
//! This module separates the concerns of validating and transforming `OpenAPI` specifications
//! into distinct, testable components following the Single Responsibility Principle.

pub mod transformer;
pub mod validator;

pub use transformer::SpecTransformer;
pub use validator::SpecValidator;

use openapiv3::{Operation, PathItem};

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
        ("GET", &item.get),
        ("POST", &item.post),
        ("PUT", &item.put),
        ("DELETE", &item.delete),
        ("PATCH", &item.patch),
        ("HEAD", &item.head),
        ("OPTIONS", &item.options),
        ("TRACE", &item.trace),
    ]
}
