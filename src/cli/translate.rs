//! CLI translation layer: converts clap `ArgMatches` into domain types.
//!
//! This module bridges the clap-specific parsing world with the
//! CLI-agnostic [`OperationCall`] and [`ExecutionContext`] types used
//! by the execution engine.
