//! The reference-flow document, read only for its step identifiers.
//!
//! `platform/reference-flows.json` is the single definition of the reference
//! application flows. The walkthrough contract cites those steps by identifier
//! instead of restating their actions, outcomes or hooks, so this module keeps
//! the smallest possible view of the document.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;

use crate::error::WalkthroughError;

/// The top-level reference-flow document.
#[derive(Debug, Clone, Deserialize)]
pub struct ReferenceFlows {
    /// The ordered flows, each with its ordered steps.
    pub flows: Vec<Flow>,
}

/// One reference-application flow.
#[derive(Debug, Clone, Deserialize)]
pub struct Flow {
    /// The steps that make up the flow.
    pub steps: Vec<FlowStep>,
}

/// One step in a flow.
#[derive(Debug, Clone, Deserialize)]
pub struct FlowStep {
    /// The stable identifier a screen cites.
    pub step_id: String,
}

impl ReferenceFlows {
    /// Reads the reference-flow document from disk.
    ///
    /// # Errors
    /// Returns [`WalkthroughError::Read`] when the file cannot be read and
    /// [`WalkthroughError::Parse`] when it is not valid JSON.
    pub fn from_file(path: &Path) -> Result<Self, WalkthroughError> {
        crate::json::read_json_file(path)
    }

    /// Collects every step identifier in the document.
    pub fn step_ids(&self) -> BTreeSet<&str> {
        self.flows
            .iter()
            .flat_map(|flow| flow.steps.iter())
            .map(|step| step.step_id.as_str())
            .collect()
    }
}
