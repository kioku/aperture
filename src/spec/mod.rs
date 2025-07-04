//! `OpenAPI` specification validation and transformation module
//!
//! This module separates the concerns of validating and transforming `OpenAPI` specifications
//! into distinct, testable components following the Single Responsibility Principle.

pub mod transformer;
pub mod validator;

pub use transformer::SpecTransformer;
pub use validator::SpecValidator;
