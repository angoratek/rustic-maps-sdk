//! Raster and vector tile payloads, tile sources and offline regions.

use super::BoundingBox;
use crate::error::SdkError;

/// Encoded image formats a raster tile provider may deliver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterImageFormat {
    /// PNG imagery.
    Png,
    /// JPEG imagery.
    Jpeg,
}

impl RasterImageFormat {
    /// Builds the mirror enum from the native image format.
    pub(crate) fn from_native_image_format(
        native_image_format: geospatial_types::RasterImageFormat,
    ) -> Self {
        match native_image_format {
            geospatial_types::RasterImageFormat::Png => Self::Png,
            geospatial_types::RasterImageFormat::Jpeg => Self::Jpeg,
        }
    }

    /// Builds the native image format from the mirror enum.
    pub(crate) fn to_native_image_format(self) -> geospatial_types::RasterImageFormat {
        match self {
            Self::Png => geospatial_types::RasterImageFormat::Png,
            Self::Jpeg => geospatial_types::RasterImageFormat::Jpeg,
        }
    }
}

/// Pixel resolution a raster tile is rendered at; `Standard` is the plain
/// tile form, `Retina` the `@2x` form with twice the texels per edge for the
/// same logical tile span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterTileResolution {
    /// One texel per logical tile pixel.
    Standard,
    /// Twice the texels per logical tile pixel (the `@2x` request form).
    Retina,
}

impl RasterTileResolution {
    /// Builds the mirror enum from the native tile resolution.
    pub(crate) fn from_native_tile_resolution(
        native_tile_resolution: geospatial_types::RasterTileResolution,
    ) -> Self {
        match native_tile_resolution {
            geospatial_types::RasterTileResolution::Standard => Self::Standard,
            geospatial_types::RasterTileResolution::Retina => Self::Retina,
        }
    }

    /// Builds the native tile resolution from the mirror enum.
    pub(crate) fn to_native_tile_resolution(self) -> geospatial_types::RasterTileResolution {
        match self {
            Self::Standard => geospatial_types::RasterTileResolution::Standard,
            Self::Retina => geospatial_types::RasterTileResolution::Retina,
        }
    }
}

/// One fetched web-mercator XYZ raster tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RasterTileData {
    /// Tile column coordinate at the given zoom level.
    pub tile_x: u32,
    /// Tile row coordinate at the given zoom level.
    pub tile_y: u32,
    /// Zoom level this tile belongs to.
    pub zoom_level: u8,
    /// Encoding of the byte payload.
    pub image_format: RasterImageFormat,
    /// Pixel resolution the payload was rendered at.
    pub tile_resolution: RasterTileResolution,
    /// Encoded image bytes.
    pub image_bytes: Vec<u8>,
}

impl RasterTileData {
    /// Builds the mirror record from a native raster tile.
    pub(crate) fn from_native_raster_tile_data(
        native_raster_tile_data: &geospatial_types::RasterTileData,
    ) -> Self {
        Self {
            tile_x: native_raster_tile_data.tile_x,
            tile_y: native_raster_tile_data.tile_y,
            zoom_level: native_raster_tile_data.zoom_level,
            image_format: RasterImageFormat::from_native_image_format(
                native_raster_tile_data.image_format,
            ),
            tile_resolution: RasterTileResolution::from_native_tile_resolution(
                native_raster_tile_data.tile_resolution,
            ),
            image_bytes: native_raster_tile_data.image_bytes.clone(),
        }
    }

    /// Builds the native raster tile from the mirror record, consuming it
    /// (the tile payload moves without a copy).
    pub(crate) fn into_native_raster_tile_data(self) -> geospatial_types::RasterTileData {
        geospatial_types::RasterTileData {
            tile_x: self.tile_x,
            tile_y: self.tile_y,
            zoom_level: self.zoom_level,
            image_format: self.image_format.to_native_image_format(),
            tile_resolution: self.tile_resolution.to_native_tile_resolution(),
            image_bytes: self.image_bytes,
        }
    }
}

/// Wire encoding of a vector tile's byte payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorTileEncoding {
    /// Plain MVT protobuf bytes.
    MapboxVectorTile,
    /// MVT protobuf bytes wrapped in gzip (the CDN's `Content-Encoding`).
    GzippedMapboxVectorTile,
}

impl VectorTileEncoding {
    /// Builds the native encoding from the mirror variant — the foreign
    /// provider adapters' direction.
    pub(crate) fn to_native_vector_tile_encoding(self) -> geospatial_types::VectorTileEncoding {
        match self {
            VectorTileEncoding::MapboxVectorTile => {
                geospatial_types::VectorTileEncoding::MapboxVectorTile
            }
            VectorTileEncoding::GzippedMapboxVectorTile => {
                geospatial_types::VectorTileEncoding::GzippedMapboxVectorTile
            }
        }
    }
}

/// One HTTP header a vector-tile source's requests carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceHeader {
    /// Header name.
    pub name: String,
    /// Header value.
    pub value: String,
}

/// Description of one vector-tileset source a foreign vector-tile provider
/// serves (see `rustic_maps.udl`'s `VectorTileSourceDescriptor` for the field
/// contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorTileSourceDescriptor {
    /// Stable unique name of the tileset, used to namespace its cached vector
    /// payloads so two tilesets serving one address never collide.
    pub source_name: String,
    /// Human-readable attribution line the provider's terms require.
    pub attribution_text: String,
    /// Lowest zoom level the provider serves.
    pub minimum_zoom_level: u8,
    /// Highest zoom level the provider serves.
    pub maximum_zoom_level: u8,
    /// Wire encoding every tile from this source arrives in.
    pub tile_encoding: VectorTileEncoding,
    /// Style names or families the tileset is designed to be rendered with.
    pub style_hints: Vec<String>,
}

impl VectorTileSourceDescriptor {
    /// Builds the native descriptor from the mirror record, consuming it.
    ///
    /// The native trait's descriptor carries the cache namespace, attribution
    /// and zoom span plus the provider's declared style hints. The mirror's
    /// `tile_encoding` has no native field: every fetched [`VectorTileData`]
    /// reports the encoding its own payload arrived in, and the decoder
    /// unwraps by that per-tile value. The hints are the provider's own
    /// declaration of the tileset's schema or style family; the installed
    /// style id alone decides which style document paints the tiles, while a
    /// consumer verifying a document's declared source uses the hints as
    /// additional served-tileset identities.
    pub(crate) fn into_native_vector_tile_source_descriptor(
        self,
    ) -> geospatial_traits::VectorTileSourceDescriptor {
        geospatial_traits::VectorTileSourceDescriptor {
            source_name: self.source_name,
            attribution_text: self.attribution_text,
            minimum_zoom_level: self.minimum_zoom_level,
            maximum_zoom_level: self.maximum_zoom_level,
            style_hints: self.style_hints,
        }
    }
}

/// One fetched web-mercator XYZ vector tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorTileData {
    /// Tile column coordinate at the given zoom level.
    pub tile_x: u32,
    /// Tile row coordinate at the given zoom level.
    pub tile_y: u32,
    /// Zoom level this tile belongs to.
    pub zoom_level: u8,
    /// Wire encoding of the byte payload.
    pub tile_encoding: VectorTileEncoding,
    /// Encoded tile bytes: MVT protobuf, optionally gzip-wrapped.
    pub tile_bytes: Vec<u8>,
}

impl VectorTileData {
    /// Builds the native vector tile from the mirror record, consuming it (the
    /// payload moves without a copy).
    pub(crate) fn into_native_vector_tile_data(self) -> geospatial_types::VectorTileData {
        geospatial_types::VectorTileData {
            tile_x: self.tile_x,
            tile_y: self.tile_y,
            zoom_level: self.zoom_level,
            tile_encoding: self.tile_encoding.to_native_vector_tile_encoding(),
            tile_bytes: self.tile_bytes,
        }
    }
}

/// One offline region selection: the identity the region is stored and
/// managed under, the area it covers, the zoom range and tile form the pack
/// holds, and the retention the application requests (see
/// `rustic_maps.udl`'s `RegionDefinition` for the field contract).
#[derive(Debug, Clone, PartialEq)]
pub struct RegionDefinition {
    /// Caller-chosen stable identifier; re-downloading the same identifier is
    /// idempotent and a redefined shape replaces the stored region.
    pub region_id: String,
    /// Human-readable name for region-listing surfaces.
    pub display_name: String,
    /// The region's bounding box; boxes never span the antimeridian.
    pub bounding_box: BoundingBox,
    /// Lowest zoom level downloaded; ranges download coarse-first.
    pub minimum_zoom_level: u8,
    /// Highest zoom level downloaded.
    pub maximum_zoom_level: u8,
    /// Encoded format the stored tile payloads carry.
    pub image_format: RasterImageFormat,
    /// Pixel resolution the stored tile payloads carry.
    pub tile_resolution: RasterTileResolution,
    /// Requested retention in seconds; null takes the raster-tiles category
    /// default, clamped to the provider's declared ceiling. Zero is refused.
    pub time_to_live_seconds: Option<u64>,
}

/// One offline region as it stands on disk: the stored shape, the effective
/// retention and the live tiles among its members (see `rustic_maps.udl`'s
/// `RegionRecord` for the field contract).
#[derive(Debug, Clone, PartialEq)]
pub struct RegionRecord {
    /// The region's identifier.
    pub region_id: String,
    /// The region's display name.
    pub display_name: String,
    /// The recorded area the region covers.
    pub bounding_box: BoundingBox,
    /// The recorded lowest zoom level.
    pub minimum_zoom_level: u8,
    /// The recorded highest zoom level.
    pub maximum_zoom_level: u8,
    /// The recorded encoded format.
    pub image_format: RasterImageFormat,
    /// The recorded pixel resolution.
    pub tile_resolution: RasterTileResolution,
    /// The retention the region's entries carry (requested, ceiling-clamped).
    pub effective_time_to_live_seconds: u64,
    /// Tile fingerprints the region claims membership for.
    pub member_tile_count: u64,
    /// Members whose entries are live on disk right now.
    pub present_tile_count: u64,
    /// Payload bytes of the live members.
    pub present_payload_bytes: u64,
}

/// The phase of one region download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionDownloadPhase {
    /// The download is running; progress reports carry the counts so far.
    Downloading,
    /// The download finished with every member tile present.
    Completed,
    /// The application cancelled the download; the partial region is kept and
    /// resumable.
    Cancelled,
    /// The download stopped on a failure after keeping the partial region.
    Failed,
}

/// One progress report of a region download, delivered to a
/// [`RegionDownloadProgressListener`](crate::RegionDownloadProgressListener):
/// one at zero completions, one per completed tile and exactly one terminal
/// phase (see `rustic_maps.udl`'s `RegionDownloadProgress` for the field
/// contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionDownloadProgress {
    /// The region this report belongs to.
    pub region_id: String,
    /// Tiles the region enumerates.
    pub total_tile_count: u64,
    /// Tiles downloaded and refreshed so far.
    pub completed_tile_count: u64,
    /// Completed tiles the provider had to fetch.
    pub network_fetched_tile_count: u64,
    /// Completed tiles the store already held.
    pub disk_served_tile_count: u64,
    /// The phase this report describes.
    pub phase: RegionDownloadPhase,
}

/// What a region's explicit deletion removed (see `rustic_maps.udl`'s
/// `RegionRemovalReport` for the field contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionRemovalReport {
    /// Tile entries deleted (never a tile another live region still claims).
    pub removed_entry_count: u64,
    /// Payload bytes those entries carried.
    pub removed_payload_bytes: u64,
}

/// One provider-neutral base-map tile source
/// (`MapSdk::register_url_template_raster_tile_source`'s parameter), shaped
/// so an application hands the SDK exactly what its shared provider registry
/// declares: a tileset's URL template, name, zoom range, served image format
/// and resolution, and the User-Agent its requests identify themselves with
/// (see `rustic_maps.udl`'s `UrlTemplateRasterTileSource` for the field
/// contract).
#[derive(Debug, Clone, PartialEq)]
pub struct UrlTemplateRasterTileSource {
    /// The `{z}/{x}/{y}` URL template the tileset publishes.
    pub url_template: String,
    /// The tileset's name, used verbatim in every diagnostic it raises.
    pub source_name: String,
    /// Lowest zoom level the tileset serves.
    pub minimum_zoom_level: u8,
    /// Highest zoom level the tileset serves.
    pub maximum_zoom_level: u8,
    /// The image format the tileset serves.
    pub image_format: RasterImageFormat,
    /// The pixel resolution the tileset serves.
    pub tile_resolution: RasterTileResolution,
    /// The `User-Agent` header every tile request carries; empty sends none.
    pub user_agent: String,
    /// Immediate requests the source admits before pacing starts; `None`
    /// keeps the documented default.
    pub token_bucket_capacity_tokens: Option<f64>,
    /// Sustained request rate in tokens per second; `None` keeps the
    /// documented default.
    pub refill_tokens_per_second: Option<f64>,
}

impl UrlTemplateRasterTileSource {
    /// Converts the binding record into the provider layer's source
    /// declaration, pacing and user agent included. The provider layer owns
    /// the tile semantics and the request budget; the binding crate owns only
    /// the contract mirror.
    pub(crate) fn into_provider_source(
        self,
    ) -> Result<everymap_providers_url_template_raster::UrlTemplateRasterTileSource, SdkError> {
        let pacing = everymap_tile_source_pacing(
            &self.source_name,
            self.token_bucket_capacity_tokens,
            self.refill_tokens_per_second,
        )?;
        let headers = everymap_request_headers(self.user_agent);
        Ok(
            everymap_providers_url_template_raster::UrlTemplateRasterTileSource {
                url_template: self.url_template,
                source_name: self.source_name,
                attribution_text: String::new(),
                terms_text: String::new(),
                minimum_zoom_level: self.minimum_zoom_level,
                maximum_zoom_level: self.maximum_zoom_level,
                image_format: everymap_image_format(self.image_format),
                tile_resolution: everymap_tile_resolution(self.tile_resolution),
                subdomains: Vec::new(),
                headers,
                pacing: Some(pacing),
            },
        )
    }
}

/// Builds the provider layer's request budget from the contract's pacing
/// fields, with the documented defaults for absent values.
///
/// The provider layer admits a whole number of requests, so a fractional
/// capacity is refused rather than rounded: rounding would silently change
/// the budget the application declared.
fn everymap_tile_source_pacing(
    source_name: &str,
    capacity_tokens: Option<f64>,
    refill_tokens_per_second: Option<f64>,
) -> Result<everymap_providers_url_template_raster::TileSourcePacing, SdkError> {
    const DEFAULT_CAPACITY_TOKENS: f64 = 8.0;
    const DEFAULT_REFILL_TOKENS_PER_SECOND: f64 = 8.0;

    let (capacity_tokens, refill_tokens_per_second) =
        rustic_maps_core::everymap_bridge::tile_source_budget_rates(
            source_name,
            capacity_tokens.unwrap_or(DEFAULT_CAPACITY_TOKENS),
            refill_tokens_per_second.unwrap_or(DEFAULT_REFILL_TOKENS_PER_SECOND),
        )
        .map_err(SdkError::from)?;
    let pacing = everymap_providers_url_template_raster::TileSourcePacing::new(
        capacity_tokens,
        refill_tokens_per_second,
    )
    .with_maximum_wait_seconds(rustic_maps_core::everymap_bridge::TILE_SOURCE_MAXIMUM_WAIT_SECONDS);
    pacing.validate(source_name).map_err(|error| {
        SdkError::from(rustic_maps_core::everymap_bridge::map_provider_error(error))
    })?;
    Ok(pacing)
}

/// The headers every tile request carries: the declared user agent, when one
/// is set. Any credential a public template needs belongs in the template.
fn everymap_request_headers(user_agent: String) -> Vec<(String, String)> {
    if user_agent.is_empty() {
        return Vec::new();
    }
    vec![("User-Agent".to_string(), user_agent)]
}

fn everymap_image_format(
    image_format: RasterImageFormat,
) -> everymap_core::domains::tiling::RasterImageFormat {
    match image_format {
        RasterImageFormat::Png => everymap_core::domains::tiling::RasterImageFormat::Png,
        RasterImageFormat::Jpeg => everymap_core::domains::tiling::RasterImageFormat::Jpeg,
    }
}

fn everymap_tile_resolution(
    tile_resolution: RasterTileResolution,
) -> everymap_core::domains::tiling::RasterTileResolution {
    match tile_resolution {
        RasterTileResolution::Standard => {
            everymap_core::domains::tiling::RasterTileResolution::Standard
        }
        RasterTileResolution::Retina => {
            everymap_core::domains::tiling::RasterTileResolution::Retina
        }
    }
}

/// A vector base map served by a provider configured through
/// `MapSdk::configure_provider_set` (`MapView::set_provider_vector_tile_source`'s
/// parameter). The provider supplies its own tileset name, attribution and
/// zoom span; the application chooses only the provider and the committed
/// style the base paints with (see `rustic_maps.udl`'s
/// `ProviderVectorTileSource` for the field contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderVectorTileSource {
    /// The provider key `configure_provider_set` registered for vector tiles.
    pub provider_key: String,
    /// The committed style id the base paints with.
    pub style_id: String,
}

/// One provider-neutral vector base-map source
/// (`MapView::set_vector_tile_source`'s parameter), shaped so an application
/// hands the SDK exactly what its
/// shared provider registry declares: a tileset's URL template, name, the
/// attribution line its terms require, the committed style id the base paints
/// with, and its zoom range (see `rustic_maps.udl`'s
/// `UrlTemplateVectorTileSource` for the field contract).
#[derive(Debug, Clone, PartialEq)]
pub struct UrlTemplateVectorTileSource {
    /// The `{z}/{x}/{y}` URL template the tileset publishes.
    pub url_template: String,
    /// The tileset's name, used verbatim in every diagnostic it raises.
    pub source_name: String,
    /// The attribution line the tileset's terms require wherever it is shown.
    pub attribution_text: String,
    /// The committed style id the base paints with.
    pub style_id: String,
    /// Lowest zoom level the tileset serves.
    pub minimum_zoom_level: u8,
    /// Highest zoom level the tileset serves.
    pub maximum_zoom_level: u8,
    /// Immediate requests the source admits before pacing starts; `None`
    /// keeps the documented default.
    pub token_bucket_capacity_tokens: Option<f64>,
    /// Sustained request rate in tokens per second; `None` keeps the
    /// documented default.
    pub refill_tokens_per_second: Option<f64>,
    /// Terms the tileset's usage policy requires alongside attribution;
    /// `None` declares none.
    pub terms_text: Option<String>,
    /// Values substituted for the template's optional `{s}` placeholder, in
    /// the source's preference order; `None` when the template has no `{s}`.
    pub subdomains: Option<Vec<String>>,
    /// Headers every tile request carries, in the order given; `None` sends
    /// none.
    pub headers: Option<Vec<SourceHeader>>,
    /// Style names or families the tileset is designed to be rendered with;
    /// `None` when it declares none.
    pub style_hints: Option<Vec<String>>,
}

impl UrlTemplateVectorTileSource {
    /// Converts the binding record into the provider layer's source
    /// declaration and the style id the base paints with, so the binding
    /// crate owns only the contract mirror. Native-only with the surface view
    /// that installs it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn into_provider_source(
        self,
    ) -> Result<
        (
            everymap_providers_url_template_vector::UrlTemplateVectorTileSource,
            String,
        ),
        SdkError,
    > {
        let pacing = everymap_tile_source_pacing(
            &self.source_name,
            self.token_bucket_capacity_tokens,
            self.refill_tokens_per_second,
        )?;
        // A keyless tileset's named declaration carries the terms, subdomains,
        // headers and style hints its operator publishes. A caller that
        // leaves one of them out inherits the recorded value rather than
        // dropping it.
        let recorded_source = recorded_url_template_vector_source(&self.url_template);
        let style_hints = self
            .style_hints
            .or_else(|| recorded_source.as_ref().map(recorded_source_style_hints))
            .unwrap_or_default();
        let terms_text = self
            .terms_text
            .or_else(|| recorded_source.as_ref().map(recorded_source_terms_text))
            .unwrap_or_default();
        let subdomains = self
            .subdomains
            .or_else(|| recorded_source.as_ref().map(recorded_source_subdomains))
            .unwrap_or_default();
        let headers = self
            .headers
            .map(source_header_pairs)
            .or_else(|| recorded_source.as_ref().map(recorded_source_headers))
            .unwrap_or_default();
        let source = everymap_providers_url_template_vector::UrlTemplateVectorTileSource {
            url_template: self.url_template,
            source_descriptor: everymap_core::domains::tiling::VectorTileSourceDescriptor {
                source_name: self.source_name,
                attribution_text: self.attribution_text,
                minimum_zoom_level: self.minimum_zoom_level,
                maximum_zoom_level: self.maximum_zoom_level,
                // A URL template does not declare its wire encoding; the
                // response sniffs it, so the descriptor names plain MVT and
                // the fetched tile reports what actually arrived.
                tile_encoding: everymap_core::domains::tiling::VectorTileEncoding::MapboxVectorTile,
                style_hints,
            },
            terms_text,
            subdomains,
            headers,
            pacing: Some(pacing),
        };
        Ok((source, self.style_id))
    }
}

/// The named keyless declaration the provider layer publishes for a URL
/// template, when one matches. The provider crate owns the declarations, so
/// the SDK names no tileset itself; a match lets the caller inherit the
/// terms, subdomains, headers and style hints the operator recorded.
///
/// The provider crate exposes each declaration as its own constructor but no
/// lookup by template, so this list is the SDK-side coupling to its current
/// pair: a third declaration would need an edit here. An upstream accessor
/// such as `vector_source_for_template(url_template: &str)` (returning the
/// named declaration that publishes a template, if any) would remove it.
#[cfg(not(target_arch = "wasm32"))]
fn recorded_url_template_vector_source(
    url_template: &str,
) -> Option<everymap_providers_url_template_vector::UrlTemplateVectorTileSource> {
    [
        everymap_providers_url_template_vector::openstreetmap_us_vector_source(),
        everymap_providers_url_template_vector::openfreemap_vector_source(),
    ]
    .into_iter()
    .find(|declaration| declaration.url_template == url_template)
}

/// The recorded terms text of a named declaration.
#[cfg(not(target_arch = "wasm32"))]
fn recorded_source_terms_text(
    declaration: &everymap_providers_url_template_vector::UrlTemplateVectorTileSource,
) -> String {
    declaration.terms_text.clone()
}

/// The recorded `{s}` subdomains of a named declaration.
#[cfg(not(target_arch = "wasm32"))]
fn recorded_source_subdomains(
    declaration: &everymap_providers_url_template_vector::UrlTemplateVectorTileSource,
) -> Vec<String> {
    declaration.subdomains.clone()
}

/// The recorded request headers of a named declaration.
#[cfg(not(target_arch = "wasm32"))]
fn recorded_source_headers(
    declaration: &everymap_providers_url_template_vector::UrlTemplateVectorTileSource,
) -> Vec<(String, String)> {
    declaration.headers.clone()
}

/// The recorded style hints of a named declaration.
#[cfg(not(target_arch = "wasm32"))]
fn recorded_source_style_hints(
    declaration: &everymap_providers_url_template_vector::UrlTemplateVectorTileSource,
) -> Vec<String> {
    declaration.source_descriptor.style_hints.clone()
}

/// Converts the contract's `SourceHeader` records into the provider layer's
/// name-value pairs.
#[cfg(not(target_arch = "wasm32"))]
fn source_header_pairs(headers: Vec<SourceHeader>) -> Vec<(String, String)> {
    headers
        .into_iter()
        .map(|header| (header.name, header.value))
        .collect()
}

/// One vector source's layer accounting for the last resolved frame
/// (`VectorTileSourceStatus::source_layer_counts`): how many of the source's
/// style layers the painted tileset matched and how many painted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorTileSourceLayerCount {
    /// The style source id the counts belong to.
    pub source_id: String,
    /// The source's layers whose source layer the painted tileset carries.
    pub matched_layer_count: u32,
    /// The source's layers that painted in the frame.
    pub drawn_layer_count: u32,
}

/// The vector base source's status readback
/// (`MapView::vector_tile_source_status`): what the view draws a styled base
/// with and what its last resolved
/// frame achieved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorTileSourceStatus {
    /// Whether the view carries a vector base source at all.
    pub is_configured: bool,
    /// The active style id; empty when no source is configured.
    pub style_id: String,
    /// The tileset's name; empty when no source is configured.
    pub source_name: String,
    /// The attribution line the tileset's terms require; empty when no source
    /// is configured.
    pub attribution_text: String,
    /// Highest zoom level the tileset serves; zero when no source is
    /// configured.
    pub maximum_zoom_level: u8,
    /// Tile addresses the last resolved frame drew.
    pub drawn_tile_count: u32,
    /// Tile addresses the last resolved frame skipped after a failed fetch or
    /// decode.
    pub failed_tile_count: u32,
    /// The last resolved frame's last tile failure detail, if any.
    pub last_failure_detail: Option<String>,
    /// Every unsupported-property diagnostic the installed style produced.
    pub unsupported_property_count: u32,
    /// The distinct unsupported properties, scope-qualified and sorted, capped
    /// at the renderer's documented bound.
    pub unsupported_property_names: Vec<String>,
    /// The assets that fell back to a degraded draw, sorted and capped at the
    /// renderer's documented bound.
    pub degraded_asset_names: Vec<String>,
    /// Each referenced source's layer accounting for the last resolved frame,
    /// sorted by source id.
    pub source_layer_counts: Vec<VectorTileSourceLayerCount>,
}

impl VectorTileSourceStatus {
    /// The honest not-configured status (no base installed). Only the native
    /// surface view asks for it; the browser canvas reports its own status
    /// through the hand-written facade.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn not_configured() -> Self {
        Self {
            is_configured: false,
            style_id: String::new(),
            source_name: String::new(),
            attribution_text: String::new(),
            maximum_zoom_level: 0,
            drawn_tile_count: 0,
            failed_tile_count: 0,
            last_failure_detail: None,
            unsupported_property_count: 0,
            unsupported_property_names: Vec::new(),
            degraded_asset_names: Vec::new(),
            source_layer_counts: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SourceHeader, UrlTemplateVectorTileSource, everymap_tile_source_pacing};
    use rustic_maps_core::everymap_bridge::TILE_SOURCE_MAXIMUM_WAIT_SECONDS;

    #[test]
    fn the_foreign_vector_tile_source_mirror_keeps_encoding_and_style_hints() {
        let mirror = super::VectorTileSourceDescriptor {
            source_name: "application/declared".to_string(),
            attribution_text: "Declared tiles".to_string(),
            minimum_zoom_level: 0,
            maximum_zoom_level: 14,
            tile_encoding: super::VectorTileEncoding::GzippedMapboxVectorTile,
            style_hints: vec!["openmaptiles".to_string()],
        };
        // The contract mirror is where a foreign provider declares its wire
        // encoding and style hints; removing either field fails to compile
        // here rather than silently narrowing the foreign declaration.
        assert_eq!(
            mirror.tile_encoding,
            super::VectorTileEncoding::GzippedMapboxVectorTile
        );
        assert_eq!(mirror.style_hints, vec!["openmaptiles".to_string()]);
        let native = mirror.into_native_vector_tile_source_descriptor();
        assert_eq!(native.source_name, "application/declared");
        assert_eq!(native.attribution_text, "Declared tiles");
        assert_eq!(native.maximum_zoom_level, 14);
    }

    #[test]
    fn a_foreign_tile_keeps_the_wire_encoding_it_declares() {
        let tile = super::VectorTileData {
            tile_x: 3,
            tile_y: 4,
            zoom_level: 5,
            tile_encoding: super::VectorTileEncoding::GzippedMapboxVectorTile,
            tile_bytes: vec![0x1f, 0x8b, 0x00],
        };
        let native = tile.into_native_vector_tile_data();
        assert_eq!(
            native.tile_encoding,
            geospatial_types::VectorTileEncoding::GzippedMapboxVectorTile,
            "the decoder reads the per-tile encoding, so the mirror must preserve it"
        );
    }

    #[test]
    fn a_declared_tile_source_budget_carries_the_documented_ceiling() {
        let pacing = everymap_tile_source_pacing("test/source", None, None)
            .expect("the documented defaults are usable");
        assert_eq!(
            pacing.maximum_wait_seconds,
            Some(TILE_SOURCE_MAXIMUM_WAIT_SECONDS),
            "a tile request is refused after the documented ceiling instead of stalling"
        );
        assert_eq!(TILE_SOURCE_MAXIMUM_WAIT_SECONDS, 10.0);
    }

    #[test]
    fn a_url_template_vector_source_keeps_the_declaration_extras() {
        let (provider_source, style_id) = UrlTemplateVectorTileSource {
            url_template: "https://tiles.example.com/{z}/{x}/{y}.pbf".to_string(),
            source_name: "openfreemap/planet".to_string(),
            attribution_text: "Map data © OpenStreetMap contributors".to_string(),
            style_id: "bright-light".to_string(),
            minimum_zoom_level: 0,
            maximum_zoom_level: 14,
            token_bucket_capacity_tokens: None,
            refill_tokens_per_second: None,
            terms_text: Some("Tileset terms apply".to_string()),
            subdomains: Some(vec!["a".to_string(), "b".to_string()]),
            headers: Some(vec![SourceHeader {
                name: "User-Agent".to_string(),
                value: "test-agent".to_string(),
            }]),
            style_hints: Some(vec!["openmaptiles".to_string()]),
        }
        .into_provider_source()
        .expect("the source declaration builds");
        assert_eq!(style_id, "bright-light");
        assert_eq!(provider_source.terms_text, "Tileset terms apply");
        assert_eq!(
            provider_source.subdomains,
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            provider_source.headers,
            vec![("User-Agent".to_string(), "test-agent".to_string())]
        );
        assert_eq!(
            provider_source.source_descriptor.style_hints,
            vec!["openmaptiles".to_string()]
        );
    }

    #[test]
    fn empty_url_template_vector_source_extras_stay_empty() {
        let (provider_source, _style_id) = UrlTemplateVectorTileSource {
            url_template: "https://tiles.example.com/{z}/{x}/{y}.pbf".to_string(),
            source_name: "test/source".to_string(),
            attribution_text: "Test vector tiles".to_string(),
            style_id: "bright-light".to_string(),
            minimum_zoom_level: 0,
            maximum_zoom_level: 14,
            token_bucket_capacity_tokens: None,
            refill_tokens_per_second: None,
            terms_text: None,
            subdomains: None,
            headers: None,
            style_hints: None,
        }
        .into_provider_source()
        .expect("the source declaration builds");
        assert!(provider_source.terms_text.is_empty());
        assert!(provider_source.subdomains.is_empty());
        assert!(provider_source.headers.is_empty());
        assert!(provider_source.source_descriptor.style_hints.is_empty());
    }

    #[test]
    fn a_keyless_named_source_inherits_its_recorded_terms_and_headers() {
        let openfreemap_declaration =
            everymap_providers_url_template_vector::openfreemap_vector_source();
        let (openfreemap_source, _style_id) = UrlTemplateVectorTileSource {
            url_template: openfreemap_declaration.url_template.clone(),
            source_name: openfreemap_declaration
                .source_descriptor
                .source_name
                .clone(),
            attribution_text: openfreemap_declaration
                .source_descriptor
                .attribution_text
                .clone(),
            style_id: "bright-light".to_string(),
            minimum_zoom_level: 0,
            maximum_zoom_level: 14,
            token_bucket_capacity_tokens: None,
            refill_tokens_per_second: None,
            terms_text: None,
            subdomains: None,
            headers: None,
            style_hints: None,
        }
        .into_provider_source()
        .expect("the source declaration builds");
        assert_eq!(
            openfreemap_source.terms_text, openfreemap_declaration.terms_text,
            "the named declaration's terms survive an empty caller field"
        );
        assert_eq!(
            openfreemap_source.source_descriptor.style_hints,
            openfreemap_declaration.source_descriptor.style_hints,
            "the named declaration's style hints survive an empty caller field"
        );

        let openstreetmap_us_declaration =
            everymap_providers_url_template_vector::openstreetmap_us_vector_source();
        let (openstreetmap_us_source, _style_id) = UrlTemplateVectorTileSource {
            url_template: openstreetmap_us_declaration.url_template.clone(),
            source_name: openstreetmap_us_declaration
                .source_descriptor
                .source_name
                .clone(),
            attribution_text: openstreetmap_us_declaration
                .source_descriptor
                .attribution_text
                .clone(),
            style_id: "carto-light".to_string(),
            minimum_zoom_level: 0,
            maximum_zoom_level: 15,
            token_bucket_capacity_tokens: None,
            refill_tokens_per_second: None,
            terms_text: None,
            subdomains: None,
            headers: None,
            style_hints: None,
        }
        .into_provider_source()
        .expect("the source declaration builds");
        assert_eq!(
            openstreetmap_us_source.headers, openstreetmap_us_declaration.headers,
            "the identifying User-Agent the tileservice requires is inherited"
        );
    }
}
