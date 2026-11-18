//! The typed error model shared across every crate and exposed over the
//! binding layer.

use serde::{Deserialize, Serialize};

/// Underlying cause reported by [`SDKError::CacheOperationFailure`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CacheOperationFailureReason {
    /// The backing store could not be opened or migrated.
    StoreUnavailable,
    /// A persisted record failed integrity validation.
    RecordCorrupted,
    /// A write or eviction could not complete within budget.
    WriteRejected,
}

/// Underlying cause reported by [`SDKError::OfflineRoutingFailure`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OfflineGraphInvalidReason {
    /// The archive header or vector offsets are misaligned or truncated.
    ArchiveMalformed,
    /// The archive schema version is not understood by this SDK.
    VersionUnsupported,
    /// Referenced node or edge indices fall outside the archive bounds.
    IndexOutOfRange,
}

/// Every failure path surfaced by the SDK.
///
/// The variants carry a stable serialized shape: variant names and field
/// names are part of the public API and do not change incompatibly within a
/// major version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SDKError {
    /// A coordinate, bounding box or request option failed validation.
    InvalidCoordinateInput(String),
    /// The network transport failed (connection, timeout, or DNS).
    NetworkRequestFailed(String),
    /// The provider responded with a payload the parser could not use.
    ResponseParsingFailure(String),
    /// The provider returned a documented application-level failure status.
    ProviderRequestRejected(String),
    /// The token-bucket limiter is exhausted; retry after the next refill.
    RateLimitExhausted,
    /// No provider implementation is registered for the requested contract.
    ProviderNotConfigured(String),
    /// A cache read or write failed for the stated reason.
    CacheOperationFailure(CacheOperationFailureReason),
    /// The offline routing graph failed validation for the stated reason.
    OfflineRoutingFailure(OfflineGraphInvalidReason),
}

impl std::fmt::Display for SDKError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCoordinateInput(detail) => {
                write!(formatter, "invalid coordinate input: {detail}")
            }
            Self::NetworkRequestFailed(detail) => {
                write!(formatter, "network request failed: {detail}")
            }
            Self::ResponseParsingFailure(detail) => {
                write!(formatter, "response parsing failure: {detail}")
            }
            Self::ProviderRequestRejected(detail) => {
                write!(formatter, "provider rejected request: {detail}")
            }
            Self::RateLimitExhausted => write!(formatter, "rate limit exhausted"),
            Self::ProviderNotConfigured(contract_name) => {
                write!(formatter, "no provider configured for {contract_name}")
            }
            Self::CacheOperationFailure(reason) => {
                write!(formatter, "cache operation failure: {reason:?}")
            }
            Self::OfflineRoutingFailure(reason) => {
                write!(formatter, "offline graph invalid: {reason:?}")
            }
        }
    }
}

impl std::error::Error for SDKError {}
