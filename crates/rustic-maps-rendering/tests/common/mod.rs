//! Shared support for the crate's integration tests.
//!
//! The render harnesses each compile this module into their own test target,
//! so the `dead_code` allowance keeps a helper that only one harness calls
//! from warning in the others.

#![allow(dead_code)] // Each test target compiles this module; no target uses every helper.

pub mod golden;
