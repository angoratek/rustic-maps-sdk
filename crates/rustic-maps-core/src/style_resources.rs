//! Provider-authenticated fetches of rendering style resources.
//!
//! A vector base is designed to be painted with a style published by the
//! tileset's provider, and that style references hosted glyph ranges and a
//! sprite sheet. Loading them needs the provider credential, but the
//! credential belongs to the provider layer: it lives in the vendor's HTTP
//! client, whose authentication provider applies it to whatever address a
//! request names.
//!
//! [`StyleResourceClient`] is that seam. The SDK supplies only the address the
//! shared registry declares for one provider's style resource; the
//! implementation, built beside the vendor client, sends the request through
//! the same transport, authentication, pacing, retry policy and redacted
//! request observation the provider's contract calls use. The credential
//! never leaves the client, this crate names no provider, and no provider URL
//! shape lives here.

use async_trait::async_trait;
use geospatial_types::SDKError;

/// Fetches one rendering style resource with the configured credential.
///
/// Implementations wrap the client that serves the provider's own contract
/// calls, so the credential, transport and request policy are the provider
/// layer's and the SDK holds neither the credential nor a vendor URL shape.
#[async_trait]
pub trait StyleResourceClient: Send + Sync {
    /// Fetches the bytes one style resource addresses.
    ///
    /// # Parameters
    /// - `url` — the absolute resource address without a credential: the
    ///   client appends the configured credential in the shape its provider
    ///   uses.
    ///
    /// # Returns
    /// The resource bytes as the provider serves them.
    ///
    /// # Errors
    /// Returns [`SDKError::NetworkRequestFailed`] when the request cannot
    /// reach the provider, [`SDKError::RateLimitExhausted`] when the provider
    /// refuses its rate, and [`SDKError::ProviderRequestRejected`] when the
    /// provider answers with a refusal.
    async fn fetch_style_resource(&self, url: &str) -> Result<Vec<u8>, SDKError>;

    /// Rewrites a resource address the style document declares into the
    /// absolute address the provider serves it from.
    ///
    /// A provider's published documents may name their glyph templates and
    /// sprite bases with the vendor's own URI scheme, which is not a web
    /// address. The provider owns that mapping because the scheme and the
    /// endpoints are the vendor's; the renderer calls this method before it
    /// resolves the address and applies its host scope, so the rewritten
    /// address is still held to the style document's origin and the hosts the
    /// provider declared.
    ///
    /// # Parameters
    /// - `declared_address` — the document's `glyphs` or `sprite` value, or
    ///   the placeholder-carrying glyph template, exactly as parsed.
    ///
    /// # Returns
    /// The absolute address to resolve, or `None` to leave the declaration
    /// unchanged. The default leaves every address unchanged, so a provider
    /// whose resources are ordinary web addresses or document-relative paths
    /// needs no mapping.
    fn resolve_declared_style_resource_address(&self, _declared_address: &str) -> Option<String> {
        None
    }
}
