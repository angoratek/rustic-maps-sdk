//! Sprite sheets for a hosted vector style, fetched through the provider's
//! authenticated style-resource client.
//!
//! A style document published by a tileset's provider names its sprite by a
//! base address; the sprite protocol resolves the index document at
//! `<base>.json` and the sheet at `<base>.png`. Both are served only to a
//! request carrying the provider's credential, so the fetches go through
//! [`StyleResourceClient`] and the credential never reaches this crate. The
//! base resolves against the style document's own address, exactly as a glyph
//! template does, and the resolved address must land on the document's origin
//! or on a host the provider declared: any other host is refused before a
//! request is made, because a style document must not be able to aim the
//! provider credential at a host the provider did not declare.
//!
//! The sheet is decoded here because the renderer's [`SpriteSource`] is
//! synchronous: both install paths prefetch once and hand the decoded atlas to
//! the scheduler. The PNG decoder builds on every target, so the native
//! surface and the browser canvas resolve a style's sprite the same way.
//!
//! [`SpriteSource`]: crate::sprite::SpriteSource

use std::sync::Arc;

use geospatial_types::SDKError;
use rustic_maps_core::StyleResourceClient;

use crate::sprite::SpriteAtlas;
use crate::style_resource_address::resolve_style_resource_address;

/// The suffix the sprite protocol names a style's index document by.
pub const SPRITE_INDEX_URL_SUFFIX: &str = ".json";

/// The suffix the sprite protocol names a style's sheet by.
pub const SPRITE_SHEET_URL_SUFFIX: &str = ".png";

/// Fetches and decodes the sprite sheet a hosted style names.
///
/// # Parameters
/// - `style_resources` — the provider's authenticated style-resource client.
/// - `style_document_url` — the style document's address, whose origin and
///   directory bound both sprite requests.
/// - `sprite_url` — the document's `sprite` value, the base address the
///   protocol resolves the index and the sheet from, either absolute or
///   relative to the document's own address.
/// - `declared_hosts` — the hosts the provider declared for its published
///   style resources; the base may live on any of them as well as on the
///   document's own origin.
///
/// # Returns
/// The decoded atlas, ready for the scheduler's per-URL sprite cache.
///
/// # Errors
/// Returns [`SDKError::ResponseParsingFailure`] when the base address does not
/// resolve to an absolute `http`/`https` URL, or when the index or sheet
/// cannot be fetched or decoded; [`SDKError::ProviderRequestRejected`] when
/// the resolved base names a host that is neither the document's origin nor
/// one the provider declared.
pub async fn fetch_hosted_sprite_atlas(
    style_resources: Arc<dyn StyleResourceClient>,
    style_document_url: &str,
    sprite_url: &str,
    declared_hosts: &[String],
) -> Result<SpriteAtlas, SDKError> {
    // A provider's document may name the sprite base in the vendor's own URI
    // scheme; the provider owns that mapping, and the rewritten base is still
    // resolved and scope-checked by `sprite_addresses`.
    let rewritten_sprite_url = style_resources.resolve_declared_style_resource_address(sprite_url);
    let sprite_url = rewritten_sprite_url.as_deref().unwrap_or(sprite_url);
    let (index_url, sheet_url) = sprite_addresses(style_document_url, sprite_url, declared_hosts)?;
    let index_json = style_resources.fetch_style_resource(&index_url).await?;
    let sheet_png = style_resources.fetch_style_resource(&sheet_url).await?;
    SpriteAtlas::from_index_and_sheet_png(&index_json, &sheet_png).map_err(|decode_failure| {
        SDKError::ResponseParsingFailure(format!(
            "the style's sprite at {sprite_url} did not decode: {decode_failure}"
        ))
    })
}

/// The index and sheet addresses a style's `sprite` base resolves to; the
/// protocol's two suffixes share the base's already-authorized origin.
fn sprite_addresses(
    style_document_url: &str,
    sprite_url: &str,
    declared_hosts: &[String],
) -> Result<(String, String), SDKError> {
    let base =
        resolve_style_resource_address(style_document_url, sprite_url, declared_hosts, "sprite")?;
    Ok((
        format!("{base}{SPRITE_INDEX_URL_SUFFIX}"),
        format!("{base}{SPRITE_SHEET_URL_SUFFIX}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use async_trait::async_trait;

    const DOCUMENT_URL: &str = "https://styles.example.test/published/style.json";
    const SPRITE_BASE_URL: &str = "https://styles.example.test/published/sprite";
    const SPRITE_INDEX_BYTES: &[u8] =
        include_bytes!("../../rustic-maps-style/fixtures/sprites/icon-sprite.json");
    const SPRITE_SHEET_BYTES: &[u8] =
        include_bytes!("../../rustic-maps-style/fixtures/sprites/icon-sprite.png");

    /// Serves the committed sprite index and sheet for the two addresses the
    /// protocol resolves, recording every address.
    struct SpriteServingStyleResourceClient {
        requested_urls: Mutex<Vec<String>>,
        refuse_sheet_requests: bool,
    }

    impl SpriteServingStyleResourceClient {
        fn new() -> Self {
            Self {
                requested_urls: Mutex::new(Vec::new()),
                refuse_sheet_requests: false,
            }
        }

        fn refusing_sheets() -> Self {
            Self {
                requested_urls: Mutex::new(Vec::new()),
                refuse_sheet_requests: true,
            }
        }

        fn requested_urls(&self) -> Vec<String> {
            self.requested_urls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }
    }

    #[async_trait]
    impl StyleResourceClient for SpriteServingStyleResourceClient {
        async fn fetch_style_resource(&self, url: &str) -> Result<Vec<u8>, SDKError> {
            self.requested_urls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(url.to_string());
            if url.ends_with(SPRITE_SHEET_URL_SUFFIX) {
                if self.refuse_sheet_requests {
                    return Err(SDKError::NetworkRequestFailed(format!(
                        "the sprite sheet {url} is unreachable"
                    )));
                }
                return Ok(SPRITE_SHEET_BYTES.to_vec());
            }
            Ok(SPRITE_INDEX_BYTES.to_vec())
        }
    }

    #[tokio::test]
    async fn a_hosted_style_sprite_resolves_to_the_committed_atlas() {
        let style_resources = Arc::new(SpriteServingStyleResourceClient::new());
        let sprite_atlas = fetch_hosted_sprite_atlas(
            Arc::clone(&style_resources) as Arc<dyn StyleResourceClient>,
            DOCUMENT_URL,
            SPRITE_BASE_URL,
            &[],
        )
        .await
        .expect("the protocol's two addresses serve one atlas");
        assert_eq!(
            style_resources.requested_urls(),
            vec![
                format!("{SPRITE_BASE_URL}{SPRITE_INDEX_URL_SUFFIX}"),
                format!("{SPRITE_BASE_URL}{SPRITE_SHEET_URL_SUFFIX}"),
            ],
            "the index is fetched before the sheet"
        );
        assert_eq!(sprite_atlas.width_pixels(), 64);
        assert!(
            sprite_atlas.icon("park").is_some(),
            "the committed index's icons are addressable"
        );
    }

    #[tokio::test]
    async fn a_sprite_base_on_another_origin_is_refused_before_any_request() {
        let style_resources = Arc::new(SpriteServingStyleResourceClient::new());
        let failure = fetch_hosted_sprite_atlas(
            Arc::clone(&style_resources) as Arc<dyn StyleResourceClient>,
            DOCUMENT_URL,
            "https://sprites.example.net/published/sprite",
            &[],
        )
        .await
        .expect_err("an off-origin sprite base must refuse");
        assert!(matches!(failure, SDKError::ProviderRequestRejected(_)));
        assert!(
            style_resources.requested_urls().is_empty(),
            "the refusal happens before any request"
        );
    }

    #[tokio::test]
    async fn a_sprite_base_in_a_vendor_scheme_is_reported_as_unfetchable() {
        let style_resources = Arc::new(SpriteServingStyleResourceClient::new());
        let failure = fetch_hosted_sprite_atlas(
            Arc::clone(&style_resources) as Arc<dyn StyleResourceClient>,
            DOCUMENT_URL,
            "vendorscheme://sprites/default",
            &[],
        )
        .await
        .expect_err("an address with no web origin cannot be fetched");
        assert!(matches!(failure, SDKError::ResponseParsingFailure(_)));
        assert!(style_resources.requested_urls().is_empty());
    }

    #[tokio::test]
    async fn an_unreachable_sheet_reports_the_fetch_failure() {
        let style_resources = Arc::new(SpriteServingStyleResourceClient::refusing_sheets());
        let failure = fetch_hosted_sprite_atlas(
            Arc::clone(&style_resources) as Arc<dyn StyleResourceClient>,
            DOCUMENT_URL,
            SPRITE_BASE_URL,
            &[],
        )
        .await
        .expect_err("an unreachable sheet cannot yield an atlas");
        assert!(matches!(failure, SDKError::NetworkRequestFailed(_)));
        assert_eq!(
            style_resources.requested_urls().len(),
            2,
            "the index is fetched before the sheet fails"
        );
    }

    /// A client that maps a vendor URI sprite base to the vendor's web
    /// endpoint, the seam a provider implementation fills in.
    struct VendorSchemeStyleResources {
        requested_urls: Mutex<Vec<String>>,
    }

    impl VendorSchemeStyleResources {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                requested_urls: Mutex::new(Vec::new()),
            })
        }

        fn requested_urls(&self) -> Vec<String> {
            self.requested_urls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }
    }

    #[async_trait]
    impl StyleResourceClient for VendorSchemeStyleResources {
        async fn fetch_style_resource(&self, url: &str) -> Result<Vec<u8>, SDKError> {
            self.requested_urls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(url.to_string());
            if url.ends_with(SPRITE_SHEET_URL_SUFFIX) {
                Ok(SPRITE_SHEET_BYTES.to_vec())
            } else {
                Ok(SPRITE_INDEX_BYTES.to_vec())
            }
        }

        fn resolve_declared_style_resource_address(
            &self,
            declared_address: &str,
        ) -> Option<String> {
            let base = declared_address.strip_prefix("vendorscheme://sprites/")?;
            Some(format!("https://sprites.example.test/v1/{base}/sprite"))
        }
    }

    #[tokio::test]
    async fn a_vendor_uri_sprite_base_is_rewritten_before_the_host_scope_rule() {
        let resources = VendorSchemeStyleResources::new();
        let sprite_atlas = fetch_hosted_sprite_atlas(
            Arc::clone(&resources) as Arc<dyn StyleResourceClient>,
            DOCUMENT_URL,
            "vendorscheme://sprites/vendor/neutral",
            &["sprites.example.test".to_string()],
        )
        .await
        .expect("the provider's mapping lands on a host it declared");
        assert_eq!(
            resources.requested_urls(),
            vec![
                "https://sprites.example.test/v1/vendor/neutral/sprite.json".to_string(),
                "https://sprites.example.test/v1/vendor/neutral/sprite.png".to_string(),
            ]
        );
        assert!(sprite_atlas.icon("park").is_some());
    }

    #[tokio::test]
    async fn a_vendor_uri_sprite_base_rewritten_to_an_undeclared_host_is_refused() {
        let resources = VendorSchemeStyleResources::new();
        let failure = fetch_hosted_sprite_atlas(
            Arc::clone(&resources) as Arc<dyn StyleResourceClient>,
            DOCUMENT_URL,
            "vendorscheme://sprites/vendor/neutral",
            &[],
        )
        .await
        .expect_err("the rewritten host must still be in scope");
        assert!(matches!(failure, SDKError::ProviderRequestRejected(_)));
        assert!(resources.requested_urls().is_empty());
    }

    #[tokio::test]
    async fn a_sprite_base_on_a_host_the_provider_declared_is_fetched() {
        let style_resources = Arc::new(SpriteServingStyleResourceClient::new());
        let declared_hosts = vec!["assets.example.test".to_string()];
        let sprite_atlas = fetch_hosted_sprite_atlas(
            Arc::clone(&style_resources) as Arc<dyn StyleResourceClient>,
            DOCUMENT_URL,
            "https://assets.example.test/published/sprite",
            &declared_hosts,
        )
        .await
        .expect("a host the provider declared is inside the credential's scope");
        assert_eq!(
            style_resources.requested_urls(),
            vec![
                "https://assets.example.test/published/sprite.json".to_string(),
                "https://assets.example.test/published/sprite.png".to_string(),
            ]
        );
        assert!(sprite_atlas.icon("park").is_some());
    }

    #[tokio::test]
    async fn a_relative_sprite_base_resolves_against_the_documents_own_directory() {
        let style_resources = Arc::new(SpriteServingStyleResourceClient::new());
        let sprite_atlas = fetch_hosted_sprite_atlas(
            Arc::clone(&style_resources) as Arc<dyn StyleResourceClient>,
            DOCUMENT_URL,
            "sprite",
            &[],
        )
        .await
        .expect("a bare sprite name resolves beside the document");
        assert_eq!(
            style_resources.requested_urls(),
            vec![
                "https://styles.example.test/published/sprite.json".to_string(),
                "https://styles.example.test/published/sprite.png".to_string(),
            ]
        );
        assert!(sprite_atlas.icon("park").is_some());
    }
}
