//! Glyph ranges for a hosted vector style, fetched through the provider's
//! authenticated style-resource client.
//!
//! A style document published by a tileset's provider names its glyph
//! endpoint as a template (`…/{fontstack}/{range}.pbf`). The ranges are only
//! served to a request carrying the provider's credential, so the fetch goes
//! through [`StyleResourceClient`] and the credential never reaches this
//! crate. The template resolves against the style document's own address, and
//! the result must land on the document's origin or on a host the provider
//! declared: any other host is refused before a request is made, because a
//! style document must not be able to aim the provider credential at a host
//! the provider did not declare.
//!
//! Decoded ranges are retained in a [`GlyphRangeCache`] keyed by the
//! provider's namespace, the font stack and the range, so two providers
//! serving the same font-stack name never share a range and a repeated frame
//! costs no request.

use std::sync::Arc;

use geospatial_types::SDKError;
use rustic_maps_core::StyleResourceClient;
use rustic_maps_glyphs::{
    GlyphFontStack, GlyphRange, GlyphRangeCache, GlyphRangeCacheConfiguration, GlyphRangeKey,
    decode_glyph_font_stack,
};
use url::Url;

use crate::style_resource_address::resolve_style_resource_address;

/// The glyph URL template's font-stack placeholder.
pub const GLYPH_URL_FONT_STACK_PLACEHOLDER: &str = "{fontstack}";

/// The glyph URL template's range placeholder.
pub const GLYPH_URL_RANGE_PLACEHOLDER: &str = "{range}";

/// The characters a font-stack path segment keeps unescaped. The comma
/// separates the names of one stack, as the glyph protocol defines.
const FONT_STACK_SAFE_CHARACTERS: &[u8] = b"-._~,";

/// The hexadecimal digits percent-encoding uses.
const PERCENT_ENCODING_HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

/// Fetches and retains one hosted style's glyph ranges.
///
/// The fetch goes through the provider's [`StyleResourceClient`], so the
/// credential, transport, pacing and retry policy are the provider layer's.
pub struct HostedGlyphRanges {
    style_resources: Arc<dyn StyleResourceClient>,
    namespace: String,
    glyph_url_template: String,
    cache: GlyphRangeCache,
}

impl HostedGlyphRanges {
    /// Builds a glyph source for one provider's style document.
    ///
    /// # Parameters
    /// - `style_resources` — the provider's authenticated style-resource
    ///   client.
    /// - `namespace` — the provider key that namespaces the cached ranges.
    /// - `style_document_url` — the style document's address, whose origin and
    ///   directory bound every glyph request.
    /// - `glyph_url_template` — the document's `glyphs` template, carrying
    ///   the font-stack and range placeholders, either absolute or relative to
    ///   the document's own address.
    /// - `declared_hosts` — the hosts the provider declared for its published
    ///   style resources; a template may live on any of them as well as on the
    ///   document's own origin.
    ///
    /// # Errors
    /// Returns [`SDKError::ResponseParsingFailure`] when the template lacks a
    /// placeholder or does not resolve to an absolute `http`/`https` URL, and
    /// [`SDKError::ProviderRequestRejected`] when the resolved template names
    /// a host that is neither the document's origin nor one the provider
    /// declared.
    pub fn new(
        style_resources: Arc<dyn StyleResourceClient>,
        namespace: impl Into<String>,
        style_document_url: &str,
        glyph_url_template: &str,
        declared_hosts: &[String],
    ) -> Result<Self, SDKError> {
        let declared_template = glyph_url_template.trim();
        // A provider's document may name the template in the vendor's own URI
        // scheme; the provider owns that mapping, and the rewritten address is
        // still resolved and scope-checked below.
        let rewritten_template =
            style_resources.resolve_declared_style_resource_address(declared_template);
        let template = rewritten_template.as_deref().unwrap_or(declared_template);
        for placeholder in [
            GLYPH_URL_FONT_STACK_PLACEHOLDER,
            GLYPH_URL_RANGE_PLACEHOLDER,
        ] {
            if !template.contains(placeholder) {
                return Err(SDKError::ResponseParsingFailure(format!(
                    "the style's glyph address {template} does not carry the \
                     {placeholder} placeholder"
                )));
            }
        }
        let resolved_template =
            resolve_style_resource_address(style_document_url, template, declared_hosts, "glyph")?;
        Ok(Self {
            style_resources,
            namespace: namespace.into(),
            glyph_url_template: resolved_template,
            cache: GlyphRangeCache::new(GlyphRangeCacheConfiguration::default()),
        })
    }

    /// The provider namespace the cached ranges belong to.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// The address one range request names, with the font stack escaped and
    /// the range label substituted.
    pub fn resolved_url(&self, font_stack_name: &str, range: GlyphRange) -> String {
        self.glyph_url_template
            .replace(
                GLYPH_URL_FONT_STACK_PLACEHOLDER,
                &escaped_font_stack(font_stack_name),
            )
            .replace(GLYPH_URL_RANGE_PLACEHOLDER, &range.url_segment())
    }

    /// The number of cached ranges, for diagnostics and tests.
    pub fn cached_range_count(&self) -> usize {
        self.cache.len()
    }

    /// The Basic Latin range (`U+0000`–`U+00FF`) of one font stack, fetched
    /// on first use and cached per provider, font stack and range.
    ///
    /// # Errors
    /// Returns the client's error when the request fails and
    /// [`SDKError::ResponseParsingFailure`] when the payload is not one glyph
    /// range in the glyph protocol.
    pub async fn basic_latin_font_stack(
        &mut self,
        font_stack_name: &str,
    ) -> Result<GlyphFontStack, SDKError> {
        self.font_stack(font_stack_name, GlyphRange::BASIC_LATIN)
            .await
    }

    /// Both Latin blocks (`U+0000`–`U+01FF`) of one font stack, fetched on
    /// first use and combined into one stack the label atlas packs in a
    /// single pass.
    ///
    /// # Errors
    /// Returns the client's error when a request fails and
    /// [`SDKError::ResponseParsingFailure`] when a payload is not one glyph
    /// range in the glyph protocol.
    pub async fn latin_font_stack(
        &mut self,
        font_stack_name: &str,
    ) -> Result<GlyphFontStack, SDKError> {
        let mut combined = self
            .font_stack(font_stack_name, GlyphRange::BASIC_LATIN)
            .await?;
        let extended = self
            .font_stack(font_stack_name, GlyphRange::LATIN_EXTENDED)
            .await?;
        combined.glyphs.extend(extended.glyphs);
        Ok(combined)
    }

    /// One font stack's glyph range, fetched on first use and cached per
    /// provider, font stack and range.
    ///
    /// # Errors
    /// Returns the client's error when the request fails and
    /// [`SDKError::ResponseParsingFailure`] when the payload is not one glyph
    /// range in the glyph protocol.
    pub async fn font_stack(
        &mut self,
        font_stack_name: &str,
        range: GlyphRange,
    ) -> Result<GlyphFontStack, SDKError> {
        let key = GlyphRangeKey::new(self.namespace.clone(), font_stack_name, range);
        if let Some(cached) = self.cache.get(&key) {
            return Ok(cached.clone());
        }
        let url = self.resolved_url(font_stack_name, range);
        let bytes = self.style_resources.fetch_style_resource(&url).await?;
        let font_stack = decode_glyph_font_stack(&bytes).map_err(|decode_failure| {
            SDKError::ResponseParsingFailure(format!(
                "the glyph range {url} did not decode: {decode_failure}"
            ))
        })?;
        self.cache.insert(key, font_stack.clone());
        Ok(font_stack)
    }
}

/// The origin of an absolute `http`/`https` address.
pub(crate) fn web_origin(address: &str) -> Result<url::Origin, SDKError> {
    let parsed = Url::parse(address).map_err(|parse_failure| {
        SDKError::ResponseParsingFailure(format!(
            "the style resource address {address} is not an absolute URL: {parse_failure}"
        ))
    })?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err(SDKError::ResponseParsingFailure(format!(
            "the style resource address {address} is not an http or https URL"
        )));
    }
    Ok(parsed.origin())
}

/// Percent-encodes one font-stack path segment, keeping the characters the
/// glyph protocol uses for the stack's own names.
fn escaped_font_stack(font_stack_name: &str) -> String {
    let mut escaped = String::with_capacity(font_stack_name.len());
    for byte in font_stack_name.bytes() {
        let is_unreserved =
            byte.is_ascii_alphanumeric() || FONT_STACK_SAFE_CHARACTERS.contains(&byte);
        if is_unreserved {
            escaped.push(char::from(byte));
        } else {
            escaped.push('%');
            escaped.push(char::from(
                PERCENT_ENCODING_HEX_DIGITS[usize::from(byte >> 4)],
            ));
            escaped.push(char::from(
                PERCENT_ENCODING_HEX_DIGITS[usize::from(byte & 0x0F)],
            ));
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const DOCUMENT_URL: &str = "https://styles.example.test/published/style.json";
    const TEMPLATE: &str = "https://styles.example.test/published/{fontstack}/{range}.pbf";
    const BASIC_LATIN_FIXTURE_BYTES: &[u8] =
        include_bytes!("../../../fixtures/glyphs/noto_sans_regular_0-255.pbf");
    const LATIN_EXTENDED_FIXTURE_BYTES: &[u8] =
        include_bytes!("../../../fixtures/glyphs/noto_sans_regular_256-511.pbf");

    /// The committed fixture a glyph address answers with, keyed by its range
    /// segment the way the glyph protocol serves it.
    fn committed_range_bytes(url: &str) -> Vec<u8> {
        if url.ends_with(&format!(
            "/{}.pbf",
            GlyphRange::LATIN_EXTENDED.url_segment()
        )) {
            LATIN_EXTENDED_FIXTURE_BYTES.to_vec()
        } else {
            BASIC_LATIN_FIXTURE_BYTES.to_vec()
        }
    }

    /// A client that records every address and answers the committed range
    /// the address names.
    struct RecordingStyleResources {
        requested_urls: Mutex<Vec<String>>,
    }

    impl RecordingStyleResources {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                requested_urls: Mutex::new(Vec::new()),
            })
        }

        fn requested_urls(&self) -> Vec<String> {
            self.requested_urls.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl StyleResourceClient for RecordingStyleResources {
        async fn fetch_style_resource(&self, url: &str) -> Result<Vec<u8>, SDKError> {
            self.requested_urls.lock().unwrap().push(url.to_string());
            Ok(committed_range_bytes(url))
        }
    }

    fn glyph_source(style_resources: Arc<RecordingStyleResources>) -> HostedGlyphRanges {
        HostedGlyphRanges::new(
            style_resources,
            "example-vendor",
            DOCUMENT_URL,
            TEMPLATE,
            &[],
        )
        .expect("the committed template is on the document's origin")
    }

    #[test]
    fn the_request_escapes_the_font_stack_and_names_the_range() {
        let resources = RecordingStyleResources::new();
        let source = glyph_source(resources);
        assert_eq!(
            source.resolved_url(
                "Noto Sans Regular,Arial Unicode MS",
                GlyphRange::BASIC_LATIN
            ),
            "https://styles.example.test/published/Noto%20Sans%20Regular,Arial%20Unicode%20MS/0-255.pbf"
        );
    }

    #[tokio::test]
    async fn one_range_is_fetched_once_and_then_served_from_the_cache() {
        let resources = RecordingStyleResources::new();
        let mut source = glyph_source(Arc::clone(&resources));
        let first = source.basic_latin_font_stack("Noto Sans Regular").await;
        let second = source.basic_latin_font_stack("Noto Sans Regular").await;
        let first_range = first.expect("the committed fixture decodes");
        assert!(!first_range.glyphs.is_empty(), "the range carries glyphs");
        assert_eq!(first_range.range, GlyphRange::BASIC_LATIN.url_segment());
        assert!(second.is_ok());
        assert_eq!(resources.requested_urls().len(), 1);
        assert_eq!(source.cached_range_count(), 1);
    }

    #[tokio::test]
    async fn the_latin_font_stack_requests_both_ranges_and_combines_them() {
        let resources = RecordingStyleResources::new();
        let mut source = glyph_source(Arc::clone(&resources));
        let combined = source
            .latin_font_stack("Noto Sans Regular")
            .await
            .expect("both committed ranges decode");
        let requested_urls = resources.requested_urls();
        assert_eq!(requested_urls.len(), 2);
        assert!(requested_urls[0].ends_with("/0-255.pbf"));
        assert!(requested_urls[1].ends_with("/256-511.pbf"));
        assert_eq!(
            combined.glyphs.len(),
            decode_glyph_font_stack(BASIC_LATIN_FIXTURE_BYTES)
                .expect("the committed Basic Latin range decodes")
                .glyphs
                .len()
                + decode_glyph_font_stack(LATIN_EXTENDED_FIXTURE_BYTES)
                    .expect("the committed extended range decodes")
                    .glyphs
                    .len()
        );
    }

    #[tokio::test]
    async fn two_providers_never_share_a_cached_range() {
        let resources = RecordingStyleResources::new();
        let mut first_provider = HostedGlyphRanges::new(
            Arc::clone(&resources) as Arc<dyn StyleResourceClient>,
            "first-vendor",
            DOCUMENT_URL,
            TEMPLATE,
            &[],
        )
        .expect("the template builds");
        let mut second_provider = HostedGlyphRanges::new(
            Arc::clone(&resources) as Arc<dyn StyleResourceClient>,
            "second-vendor",
            DOCUMENT_URL,
            TEMPLATE,
            &[],
        )
        .expect("the template builds");
        first_provider
            .basic_latin_font_stack("Noto Sans Regular")
            .await
            .expect("the first provider's range decodes");
        second_provider
            .basic_latin_font_stack("Noto Sans Regular")
            .await
            .expect("the second provider's range decodes");
        assert_eq!(
            resources.requested_urls().len(),
            2,
            "the providers' caches are separate, so both ranges are fetched"
        );
    }

    #[test]
    fn a_template_on_another_origin_is_refused_before_any_request() {
        let resources = RecordingStyleResources::new();
        let refusal = HostedGlyphRanges::new(
            Arc::clone(&resources) as Arc<dyn StyleResourceClient>,
            "example-vendor",
            DOCUMENT_URL,
            "https://other.example.test/{fontstack}/{range}.pbf",
            &["assets.example.test".to_string()],
        );
        assert!(matches!(refusal, Err(SDKError::ProviderRequestRejected(_))));
        assert!(resources.requested_urls().is_empty());
    }

    /// A client that maps a vendor URI scheme to the vendor's web endpoint,
    /// the seam a provider implementation fills in.
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
            self.requested_urls.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl StyleResourceClient for VendorSchemeStyleResources {
        async fn fetch_style_resource(&self, url: &str) -> Result<Vec<u8>, SDKError> {
            self.requested_urls.lock().unwrap().push(url.to_string());
            Ok(committed_range_bytes(url))
        }

        fn resolve_declared_style_resource_address(
            &self,
            declared_address: &str,
        ) -> Option<String> {
            let template = declared_address.strip_prefix("vendorscheme://fonts/vendor/")?;
            Some(format!(
                "https://fonts.example.test/v1/vendor/{template}.pbf"
            ))
        }
    }

    #[tokio::test]
    async fn a_vendor_uri_template_is_rewritten_before_the_host_scope_rule() {
        let resources = VendorSchemeStyleResources::new();
        let mut source = HostedGlyphRanges::new(
            Arc::clone(&resources) as Arc<dyn StyleResourceClient>,
            "example-vendor",
            DOCUMENT_URL,
            "vendorscheme://fonts/vendor/{fontstack}/{range}",
            &["fonts.example.test".to_string()],
        )
        .expect("the provider's mapping lands on a host it declared");
        source
            .basic_latin_font_stack("Noto Sans Regular")
            .await
            .expect("the committed fixture decodes");
        assert_eq!(
            resources.requested_urls(),
            vec![
                "https://fonts.example.test/v1/vendor/Noto%20Sans%20Regular/0-255.pbf".to_string()
            ]
        );
    }

    #[test]
    fn a_vendor_uri_template_rewritten_to_an_undeclared_host_is_refused() {
        let resources = VendorSchemeStyleResources::new();
        let refusal = HostedGlyphRanges::new(
            Arc::clone(&resources) as Arc<dyn StyleResourceClient>,
            "example-vendor",
            DOCUMENT_URL,
            "vendorscheme://fonts/vendor/{fontstack}/{range}",
            &[],
        );
        assert!(matches!(refusal, Err(SDKError::ProviderRequestRejected(_))));
        assert!(resources.requested_urls().is_empty());
    }

    #[tokio::test]
    async fn a_template_on_a_host_the_provider_declared_is_fetched() {
        let resources = RecordingStyleResources::new();
        let mut source = HostedGlyphRanges::new(
            Arc::clone(&resources) as Arc<dyn StyleResourceClient>,
            "example-vendor",
            DOCUMENT_URL,
            "https://assets.example.test/{fontstack}/{range}.pbf",
            &["assets.example.test".to_string()],
        )
        .expect("a host the provider declared is inside the credential's scope");
        source
            .basic_latin_font_stack("Noto Sans Regular")
            .await
            .expect("the committed fixture decodes");
        assert_eq!(
            resources.requested_urls(),
            vec!["https://assets.example.test/Noto%20Sans%20Regular/0-255.pbf".to_string()]
        );
    }

    #[tokio::test]
    async fn a_relative_template_resolves_against_the_documents_own_directory() {
        let resources = RecordingStyleResources::new();
        let mut source = HostedGlyphRanges::new(
            Arc::clone(&resources) as Arc<dyn StyleResourceClient>,
            "example-vendor",
            DOCUMENT_URL,
            "fonts/{fontstack}/{range}.pbf",
            &[],
        )
        .expect("a relative template stays on the document's origin");
        source
            .basic_latin_font_stack("Noto Sans Regular")
            .await
            .expect("the committed fixture decodes");
        assert_eq!(
            resources.requested_urls(),
            vec![
                "https://styles.example.test/published/fonts/Noto%20Sans%20Regular/0-255.pbf"
                    .to_string()
            ]
        );
    }

    #[test]
    fn a_template_without_its_placeholders_is_refused() {
        let resources = RecordingStyleResources::new();
        let refusal = HostedGlyphRanges::new(
            Arc::clone(&resources) as Arc<dyn StyleResourceClient>,
            "example-vendor",
            DOCUMENT_URL,
            "https://styles.example.test/published/range.pbf",
            &[],
        );
        assert!(matches!(refusal, Err(SDKError::ResponseParsingFailure(_))));
    }
}
