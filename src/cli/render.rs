//! Rendering layer for [`ExecutionResult`] values.
//!
//! Converts structured execution results into user-facing output
//! (stdout) in the requested format (JSON, YAML, table). This module
//! owns all `println!` calls for API response rendering.
