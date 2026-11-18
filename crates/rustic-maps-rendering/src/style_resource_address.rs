//! Resolves the addresses a style document names for its own resources and
//! holds them to the provider credential's host scope.
//!
//! A style document may name a resource absolutely, or relative to the
//! document's own address, as the style specification allows. Resolution
//! happens here so the glyph and sprite fetchers apply one rule: an address is
//! fetchable when it lands on the style document's own origin or on a host the
//! provider declared. Any other host is refused before a request is made,
//! because a style document must not be able to aim the provider credential at
//! a host the provider did not declare.
//!
//! A provider's document may name its resources with the vendor's own URI
//! scheme. The provider-owned
//! [`StyleResourceClient::resolve_declared_style_resource_address`] mapping
//! runs before this resolver, so a scheme the provider knows becomes an
//! ordinary web address and is scope-checked here like any other.
//!
//! An address this crate cannot turn into an `http`/`https` URL — an
//! unrecognized scheme, or a relative reference that climbs above the
//! document's own directory — is reported as a parsing failure rather than a
//! refusal. It is no credential risk, so the caller degrades to its embedded
//! font or its iconless draw instead of refusing the install.
//!
//! [`StyleResourceClient::resolve_declared_style_resource_address`]:
//! rustic_maps_core::StyleResourceClient::resolve_declared_style_resource_address

use geospatial_types::SDKError;
use url::Url;

use crate::hosted_glyphs::web_origin;

/// The segment a relative reference climbs above its directory with.
const PARENT_DIRECTORY_SEGMENT: &str = "..";

/// Resolves one resource address a style document names and checks it against
/// the credential's host scope.
///
/// # Parameters
/// - `style_document_url` — the style document's address, whose origin bounds
///   every resource request and whose directory a relative address resolves
///   against.
/// - `declared_address` — the document's value for the resource: an absolute
///   `http`/`https` address, a root-relative path, or a path relative to the
///   document's directory.
/// - `declared_hosts` — the hosts the provider declared for its published
///   style resources; a resource may live on any of them as well as on the
///   document's own origin.
/// - `resource_label` — the resource's name in a refusal (`glyph`, `sprite`).
///
/// # Returns
/// The resolved absolute address, ready to fetch.
///
/// # Errors
/// [`SDKError::ResponseParsingFailure`] when the address cannot be turned into
/// an `http`/`https` URL, and [`SDKError::ProviderRequestRejected`] when the
/// resolved address names a host that is neither the document's origin nor one
/// the provider declared.
pub(crate) fn resolve_style_resource_address(
    style_document_url: &str,
    declared_address: &str,
    declared_hosts: &[String],
    resource_label: &str,
) -> Result<String, SDKError> {
    let document_origin = web_origin(style_document_url)?;
    let document = Url::parse(style_document_url).map_err(|parse_failure| {
        SDKError::ResponseParsingFailure(format!(
            "the style document address {style_document_url} is not an absolute URL: \
             {parse_failure}"
        ))
    })?;
    let address = declared_address.trim();
    let resolved = if Url::parse(address).is_ok() {
        address.to_string()
    } else {
        relative_address(&document, address)?
    };
    let resolved_origin = web_origin(&resolved)?;
    if resolved_origin != document_origin {
        let resolved_host = Url::parse(&resolved)
            .ok()
            .and_then(|parsed| parsed.host_str().map(str::to_string))
            .unwrap_or_default();
        let host_was_declared = declared_hosts
            .iter()
            .any(|declared_host| declared_host.eq_ignore_ascii_case(&resolved_host));
        if !host_was_declared {
            return Err(SDKError::ProviderRequestRejected(format!(
                "the style's {resource_label} address {resolved} names the host \
                 {resolved_host}, which is neither the style document's origin nor a host the \
                 provider declared"
            )));
        }
    }
    Ok(resolved)
}

/// Resolves a reference relative to the style document's own directory, or to
/// the document's origin when the reference is root-relative.
fn relative_address(document: &Url, address: &str) -> Result<String, SDKError> {
    let document_path = document.path();
    let origin_prefix = document.origin().ascii_serialization();
    if let Some(root_relative_address) = address.strip_prefix('/') {
        return Ok(format!("{origin_prefix}/{root_relative_address}"));
    }
    if address
        .split('/')
        .any(|segment| segment == PARENT_DIRECTORY_SEGMENT)
    {
        return Err(SDKError::ResponseParsingFailure(format!(
            "the style resource address {address} climbs above the style document's own \
             directory"
        )));
    }
    let directory = &document_path[..document_path.rfind('/').unwrap_or(0)];
    Ok(format!("{origin_prefix}{directory}/{address}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCUMENT_URL: &str = "https://styles.example.test/published/family/style.json";

    #[test]
    fn a_relative_address_resolves_against_the_documents_own_directory() {
        assert_eq!(
            resolve_style_resource_address(
                DOCUMENT_URL,
                "glyphs/{fontstack}/{range}.pbf",
                &[],
                "glyph"
            )
            .expect("a same-directory reference stays on the document's origin"),
            "https://styles.example.test/published/family/glyphs/{fontstack}/{range}.pbf"
        );
        assert_eq!(
            resolve_style_resource_address(DOCUMENT_URL, "sprite", &[], "sprite")
                .expect("a bare name stays on the document's origin"),
            "https://styles.example.test/published/family/sprite"
        );
    }

    #[test]
    fn a_document_address_carrying_a_query_still_resolves_against_its_own_origin() {
        assert_eq!(
            resolve_style_resource_address(
                "https://styles.example.test/published/style.json?key=value",
                "sprite",
                &[],
                "sprite"
            )
            .expect("a query on the document address does not change its own origin"),
            "https://styles.example.test/published/sprite"
        );
    }

    #[test]
    fn a_root_relative_address_resolves_against_the_documents_origin() {
        assert_eq!(
            resolve_style_resource_address(DOCUMENT_URL, "/assets/sprite", &[], "sprite")
                .expect("a root-relative reference stays on the document's origin"),
            "https://styles.example.test/assets/sprite"
        );
    }

    #[test]
    fn a_declared_host_beyond_the_documents_origin_is_allowed() {
        let declared_hosts = vec!["assets.example.test".to_string()];
        assert_eq!(
            resolve_style_resource_address(
                DOCUMENT_URL,
                "https://assets.example.test/published/sprite",
                &declared_hosts,
                "sprite"
            )
            .expect("a host the provider declared is inside the credential's scope"),
            "https://assets.example.test/published/sprite"
        );
    }

    #[test]
    fn an_undeclared_host_is_refused_and_named() {
        let failure = resolve_style_resource_address(
            DOCUMENT_URL,
            "https://sprites.example.net/published/sprite",
            &["assets.example.test".to_string()],
            "sprite",
        )
        .expect_err("an undeclared host is outside the credential's scope");
        let SDKError::ProviderRequestRejected(detail) = failure else {
            panic!("an undeclared host is a refusal, not a parsing failure");
        };
        assert!(detail.contains("sprites.example.net"), "{detail}");
        assert!(detail.contains("neither"), "{detail}");
    }

    #[test]
    fn a_vendor_scheme_address_is_reported_as_unfetchable() {
        let failure = resolve_style_resource_address(
            DOCUMENT_URL,
            "mapbox://fonts/{fontstack}",
            &[],
            "glyph",
        )
        .expect_err("a vendor URI scheme has no web origin");
        assert!(matches!(failure, SDKError::ResponseParsingFailure(_)));
    }

    #[test]
    fn a_reference_climbing_above_the_documents_directory_is_unfetchable() {
        let failure = resolve_style_resource_address(DOCUMENT_URL, "../sprite", &[], "sprite")
            .expect_err("a climbing reference is not resolved outside the document's scope");
        assert!(matches!(failure, SDKError::ResponseParsingFailure(_)));
    }
}
