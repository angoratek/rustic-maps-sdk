//! The provider seam between a style document's abstract sources and the
//! tiles a provider fetches.
//!
//! A style document names its sources (`"base"`, `"terrain"`, …) and never
//! carries a vendor URL or a credential. The host binds each declared source
//! id to a provider through [`StyleSourceBindings`]:
//!
//! ```text
//! style JSON: { "sources": { "base": { "type": "vector" } } }
//!        │
//!        ▼  StyleSourceBindings::bind_style
//! BoundVectorSource { source_id: "base", provider_key: "…",
//!                     provider: Arc<dyn VectorTileProvider>, policy }
//!        │
//!        ▼  provider crates own the URL template and credential
//!        fetch, decode, style in global layer order
//! ```
//!
//! The renderer never sees a URL: it holds a [`VectorTileProvider`] or a
//! [`RasterTileProvider`] and the TileJSON policy the style declared. Provider
//! crates construct those providers from the shared registry, so a product
//! style, a test and a reference-app shell all bind by id rather than copying
//! a vendor address.
//!
//! # TileJSON policy
//!
//! [`SourceFetchPolicy`] folds the style source's TileJSON fields into the
//! provider descriptor:
//!
//! - `minzoom`/`maxzoom` clamp the requested zoom level
//!   ([`BoundVectorSource::served_zoom_level`]);
//! - `scheme: "tms"` flips a tile's row before the provider fetch;
//! - `bounds` drops a tile whose geographic span does not intersect them;
//! - `attribution` is the presentation text the base reports, preferring the
//!   style's declaration over the provider's descriptor.
//!
//! # Multi-source frames
//!
//! [`resolve_multi_source_vector_frame`] fetches each vector source's tiles
//! independently and styles them through the compiled plan's global layer
//! order. A layer therefore reads only its own source's geometry, and two
//! sources contribute to the same frame. Raster layers are not part of that
//! frame: [`raster_layer_requests`] routes them to the existing
//! [`crate::tile_scheduler::TileScheduler`].

use crate::geojson_source::{GeoJsonTilingOptions, tile_geojson_for_tile};
use crate::labels::{MapLabelCandidate, label_candidates_for_compiled_layer_at_zoom};
use crate::overlay::MapOverlay;
use crate::projection::normalized_world_to_coordinate;
use crate::tile_key::TileKey;
use crate::vector_base::decode_wire_vector_tile;
use crate::vector_style::{VectorTileAddress, overlays_for_compiled_layer_at_zoom};
use async_trait::async_trait;
use futures::future::join_all;
use geospatial_traits::{RasterTileProvider, VectorTileProvider};
use geospatial_types::{RasterImageFormat, RasterTileData, RasterTileResolution, SDKError};
use rustic_maps_style::{
    CompiledGeoJsonSource, CompiledLayerKind, CompiledSource, CompiledStyle, CompiledVectorSource,
    TileScheme,
};
use rustic_maps_vector_tiles::{DEFAULT_TILE_EXTENT, VectorTile};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Separator between distinct attribution lines a multi-source base reports.
pub const SOURCE_ATTRIBUTION_SEPARATOR: &str = " — ";

/// One vector source binding: the style source id a host binds to a provider.
///
/// The provider owns the URL template and the credential; this record carries
/// only the abstract registry key and the TileJSON policy overrides a host
/// may need when the style omits them.
pub struct VectorSourceBinding {
    /// The style source id the document names.
    pub source_id: String,
    /// The shared registry key of the provider that serves it.
    pub provider_key: String,
    /// The provider the host constructed from its registration path.
    pub provider: Arc<dyn VectorTileProvider>,
    /// The tile scheme, when the provider's addressing overrides the style's.
    pub scheme: Option<TileScheme>,
    /// The served bounds, when the provider narrows the style's.
    pub bounds: Option<[f64; 4]>,
    /// The lowest served zoom, when the provider narrows the descriptor.
    pub minimum_zoom_level: Option<u8>,
    /// The highest served zoom, when the provider narrows the descriptor.
    pub maximum_zoom_level: Option<u8>,
}

impl std::fmt::Debug for VectorSourceBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VectorSourceBinding")
            .field("source_id", &self.source_id)
            .field("provider_key", &self.provider_key)
            .field("scheme", &self.scheme)
            .field("bounds", &self.bounds)
            .field("minimum_zoom_level", &self.minimum_zoom_level)
            .field("maximum_zoom_level", &self.maximum_zoom_level)
            .finish_non_exhaustive()
    }
}

impl VectorSourceBinding {
    /// Binds a style source id to a provider.
    pub fn new(
        source_id: impl Into<String>,
        provider_key: impl Into<String>,
        provider: Arc<dyn VectorTileProvider>,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            provider_key: provider_key.into(),
            provider,
            scheme: None,
            bounds: None,
            minimum_zoom_level: None,
            maximum_zoom_level: None,
        }
    }

    /// Overrides the tile scheme the provider's addressing follows.
    pub fn with_scheme(mut self, scheme: TileScheme) -> Self {
        self.scheme = Some(scheme);
        self
    }

    /// Narrows the served bounds.
    pub fn with_bounds(mut self, bounds: [f64; 4]) -> Self {
        self.bounds = Some(bounds);
        self
    }

    /// Narrows the served zoom span.
    pub fn with_zoom_span(mut self, minimum_zoom_level: u8, maximum_zoom_level: u8) -> Self {
        self.minimum_zoom_level = Some(minimum_zoom_level);
        self.maximum_zoom_level = Some(maximum_zoom_level);
        self
    }
}

/// One raster source binding: the style source id a host binds to a raster
/// provider, with the image format and resolution the surface asks for.
pub struct RasterSourceBinding {
    /// The style source id the document names.
    pub source_id: String,
    /// The shared registry key of the provider that serves it.
    pub provider_key: String,
    /// The provider the host constructed from its registration path.
    pub provider: Arc<dyn RasterTileProvider>,
    /// The image encoding the surface requests.
    pub image_format: RasterImageFormat,
    /// The texel density the surface requests.
    pub tile_resolution: RasterTileResolution,
    /// The tile scheme, when the provider's addressing overrides the style's.
    pub scheme: Option<TileScheme>,
    /// The served bounds, when the provider narrows the style's.
    pub bounds: Option<[f64; 4]>,
    /// The lowest served zoom, when the provider narrows the descriptor.
    pub minimum_zoom_level: Option<u8>,
    /// The highest served zoom, when the provider narrows the descriptor.
    pub maximum_zoom_level: Option<u8>,
}

impl std::fmt::Debug for RasterSourceBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RasterSourceBinding")
            .field("source_id", &self.source_id)
            .field("provider_key", &self.provider_key)
            .field("image_format", &self.image_format)
            .field("tile_resolution", &self.tile_resolution)
            .finish_non_exhaustive()
    }
}

impl RasterSourceBinding {
    /// Binds a style source id to a raster provider.
    pub fn new(
        source_id: impl Into<String>,
        provider_key: impl Into<String>,
        provider: Arc<dyn RasterTileProvider>,
        image_format: RasterImageFormat,
        tile_resolution: RasterTileResolution,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            provider_key: provider_key.into(),
            provider,
            image_format,
            tile_resolution,
            scheme: None,
            bounds: None,
            minimum_zoom_level: None,
            maximum_zoom_level: None,
        }
    }

    /// Overrides the tile scheme the provider's addressing follows.
    pub fn with_scheme(mut self, scheme: TileScheme) -> Self {
        self.scheme = Some(scheme);
        self
    }

    /// Narrows the served bounds.
    pub fn with_bounds(mut self, bounds: [f64; 4]) -> Self {
        self.bounds = Some(bounds);
        self
    }

    /// Narrows the served zoom span.
    pub fn with_zoom_span(mut self, minimum_zoom_level: u8, maximum_zoom_level: u8) -> Self {
        self.minimum_zoom_level = Some(minimum_zoom_level);
        self.maximum_zoom_level = Some(maximum_zoom_level);
        self
    }
}

/// One GeoJSON source binding: the data the source serves.
///
/// A style may carry the collection inline, in which case no binding is
/// needed. A source that names a `data` URL requires a binding here, because
/// fetching that URL belongs to the provider layer.
#[derive(Debug, Clone)]
pub struct GeoJsonSourceBinding {
    /// The style source id the document names.
    pub source_id: String,
    /// The shared registry key of the provider that supplies the data.
    pub provider_key: String,
    /// The GeoJSON collection the source serves.
    pub data: serde_json::Value,
}

impl GeoJsonSourceBinding {
    /// Binds a style source id to a GeoJSON collection.
    pub fn new(
        source_id: impl Into<String>,
        provider_key: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            provider_key: provider_key.into(),
            data,
        }
    }
}

/// The TileJSON policy a bound source fetches under.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceFetchPolicy {
    /// The address convention the source's rows follow.
    pub scheme: TileScheme,
    /// The served bounds, when the source declares any.
    pub bounds: Option<[f64; 4]>,
    /// The lowest zoom level the source serves.
    pub minimum_zoom_level: u8,
    /// The highest zoom level the source serves.
    pub maximum_zoom_level: u8,
    /// The attribution line the source requires.
    pub attribution_text: String,
}

impl SourceFetchPolicy {
    /// The requested zoom level clamped into the served span. An inverted
    /// span clamps to its lower edge rather than panicking.
    pub fn served_zoom_level(&self, requested_zoom_level: u8) -> u8 {
        let highest = self.maximum_zoom_level.max(self.minimum_zoom_level);
        requested_zoom_level
            .max(self.minimum_zoom_level)
            .min(highest)
    }

    /// Whether the tile's geographic span intersects the served bounds.
    pub fn tile_intersects_bounds(&self, tile_key: TileKey) -> bool {
        let Some(bounds) = self.bounds else {
            return true;
        };
        let [tile_west, tile_south, tile_east, tile_north] = tile_geographic_bounds(tile_key);
        let [source_west, source_south, source_east, source_north] = bounds;
        let longitude_intersects = tile_east > source_west && tile_west < source_east;
        let latitude_intersects = tile_north > source_south && tile_south < source_north;
        longitude_intersects && latitude_intersects
    }

    /// The tile address a fetch uses: the request address with the row
    /// flipped for a TMS source. `None` when the tile is outside the served
    /// zoom span or bounds.
    pub fn fetch_tile_key(&self, tile_key: TileKey) -> Option<TileKey> {
        if tile_key.zoom_level < self.minimum_zoom_level
            || tile_key.zoom_level > self.maximum_zoom_level
            || !self.tile_intersects_bounds(tile_key)
        {
            return None;
        }
        let row = match self.scheme {
            TileScheme::Xyz => tile_key.tile_y,
            TileScheme::Tms => flipped_tile_row(tile_key.tile_y, tile_key.zoom_level),
        };
        Some(TileKey {
            tile_x: tile_key.tile_x,
            tile_y: row,
            zoom_level: tile_key.zoom_level,
        })
    }
}

/// A vector source resolved against a style declaration.
pub struct BoundVectorSource {
    /// The style source id.
    pub source_id: String,
    /// The shared registry key of the provider.
    pub provider_key: String,
    /// The provider that owns the URL template and credential.
    pub provider: Arc<dyn VectorTileProvider>,
    /// The TileJSON policy the fetch honours.
    pub policy: SourceFetchPolicy,
}

impl std::fmt::Debug for BoundVectorSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BoundVectorSource")
            .field("source_id", &self.source_id)
            .field("provider_key", &self.provider_key)
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl BoundVectorSource {
    /// The requested zoom level clamped into the served span.
    pub fn served_zoom_level(&self, requested_zoom_level: u8) -> u8 {
        self.policy.served_zoom_level(requested_zoom_level)
    }

    /// The attribution line this source requires.
    pub fn attribution_text(&self) -> &str {
        &self.policy.attribution_text
    }

    /// Fetches and decodes one tile through the provider.
    ///
    /// `Ok(None)` means the tile lies outside the served zoom span or bounds,
    /// so no request was made; a provider or decode failure is the `Err`
    /// detail.
    pub async fn fetch_tile(&self, tile_key: TileKey) -> Result<Option<Arc<VectorTile>>, String> {
        let Some(fetch_tile_key) = self.policy.fetch_tile_key(tile_key) else {
            return Ok(None);
        };
        let tile_data = self
            .provider
            .fetch_vector_tile(
                fetch_tile_key.tile_x,
                fetch_tile_key.tile_y,
                fetch_tile_key.zoom_level,
            )
            .await
            .map_err(|sdk_error| {
                format!(
                    "source {:?} tile {}/{}/{} failed to fetch: {sdk_error}",
                    self.source_id, tile_key.zoom_level, tile_key.tile_x, tile_key.tile_y
                )
            })?;
        let decoded_tile = decode_wire_vector_tile(&tile_data).map_err(|decode_detail| {
            format!(
                "source {:?} tile {}/{}/{} failed to decode: {decode_detail}",
                self.source_id, tile_key.zoom_level, tile_key.tile_x, tile_key.tile_y
            )
        })?;
        Ok(Some(Arc::new(decoded_tile)))
    }
}

/// A raster source resolved against a style declaration.
pub struct BoundRasterSource {
    /// The style source id.
    pub source_id: String,
    /// The shared registry key of the provider.
    pub provider_key: String,
    /// The provider that owns the URL template and credential.
    pub provider: Arc<dyn RasterTileProvider>,
    /// The image encoding the surface requests.
    pub image_format: RasterImageFormat,
    /// The texel density the surface requests.
    pub tile_resolution: RasterTileResolution,
    /// The TileJSON policy the fetch honours.
    pub policy: SourceFetchPolicy,
}

impl std::fmt::Debug for BoundRasterSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BoundRasterSource")
            .field("source_id", &self.source_id)
            .field("provider_key", &self.provider_key)
            .field("image_format", &self.image_format)
            .field("tile_resolution", &self.tile_resolution)
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl RasterTileProvider for BoundRasterSource {
    async fn fetch_raster_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
        image_format: RasterImageFormat,
        tile_resolution: RasterTileResolution,
    ) -> Result<RasterTileData, SDKError> {
        let requested_tile_key = TileKey::new(tile_x, tile_y, zoom_level)?;
        let Some(fetch_tile_key) = self.policy.fetch_tile_key(requested_tile_key) else {
            return Err(SDKError::ProviderRequestRejected(format!(
                "source {:?} does not serve tile {zoom_level}/{tile_x}/{tile_y}",
                self.source_id
            )));
        };
        self.provider
            .fetch_raster_tile(
                fetch_tile_key.tile_x,
                fetch_tile_key.tile_y,
                fetch_tile_key.zoom_level,
                image_format,
                tile_resolution,
            )
            .await
    }
}

/// A GeoJSON source resolved against a style declaration, ready to tile.
#[derive(Debug, Clone)]
pub struct BoundGeoJsonSource {
    /// The style source id.
    pub source_id: String,
    /// The shared registry key of the provider, or `"inline"` for a source the
    /// style itself carries.
    pub provider_key: String,
    /// The data the source serves.
    pub data: serde_json::Value,
    /// The tiling options built from the source's declared buffer, tolerance
    /// and id policy.
    pub options: GeoJsonTilingOptions,
}

impl BoundGeoJsonSource {
    /// Tiles the collection for one address.
    pub fn tile(&self, tile_key: TileKey) -> Result<VectorTile, String> {
        tile_geojson_for_tile(&self.data, tile_key, &self.options)
            .map_err(|tiling_error| tiling_error.to_string())
    }
}

/// A diagnostic for a source that could not be bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceBindingDiagnostic {
    /// The style source id.
    pub source_id: String,
    /// Why no binding was resolved.
    pub detail: String,
}

/// The host's bindings for every source a style may declare.
#[derive(Default)]
pub struct StyleSourceBindings {
    vector_bindings: BTreeMap<String, Arc<VectorSourceBinding>>,
    raster_bindings: BTreeMap<String, Arc<RasterSourceBinding>>,
    geojson_bindings: BTreeMap<String, Arc<GeoJsonSourceBinding>>,
}

impl StyleSourceBindings {
    /// An empty binding set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces a vector source binding.
    pub fn with_vector_source(mut self, binding: VectorSourceBinding) -> Self {
        self.vector_bindings
            .insert(binding.source_id.clone(), Arc::new(binding));
        self
    }

    /// Adds or replaces a raster source binding.
    pub fn with_raster_source(mut self, binding: RasterSourceBinding) -> Self {
        self.raster_bindings
            .insert(binding.source_id.clone(), Arc::new(binding));
        self
    }

    /// Adds or replaces a GeoJSON source binding.
    pub fn with_geojson_source(mut self, binding: GeoJsonSourceBinding) -> Self {
        self.geojson_bindings
            .insert(binding.source_id.clone(), Arc::new(binding));
        self
    }

    /// Resolves every source the compiled style declares against these
    /// bindings.
    ///
    /// A missing binding, a kind mismatch and a GeoJSON source that names a
    /// remote URL without a binding each become a [`SourceBindingDiagnostic`];
    /// the remaining sources still bind. The style always compiles: an
    /// unbound source simply contributes no geometry.
    pub fn bind_style(&self, style: &CompiledStyle) -> BoundStyleSources {
        let mut vector_sources = BTreeMap::new();
        let mut raster_sources = BTreeMap::new();
        let mut geojson_sources = BTreeMap::new();
        let mut diagnostics = Vec::new();
        for (source_id, source) in &style.sources {
            match source {
                CompiledSource::Vector(vector_source) => {
                    match self.vector_bindings.get(source_id) {
                        Some(binding) => {
                            let policy = vector_binding_policy(binding, vector_source);
                            vector_sources.insert(
                                source_id.clone(),
                                Arc::new(BoundVectorSource {
                                    source_id: source_id.clone(),
                                    provider_key: binding.provider_key.clone(),
                                    provider: Arc::clone(&binding.provider),
                                    policy,
                                }),
                            );
                        }
                        None => diagnostics.push(SourceBindingDiagnostic {
                            source_id: source_id.clone(),
                            detail: if self.raster_bindings.contains_key(source_id)
                                || self.geojson_bindings.contains_key(source_id)
                            {
                                "the style declares a vector source but only another kind is bound"
                                    .to_string()
                            } else {
                                "no provider binding serves this vector source".to_string()
                            },
                        }),
                    }
                }
                CompiledSource::Raster(raster_source) => {
                    match self.raster_bindings.get(source_id) {
                        Some(binding) => {
                            let policy = raster_binding_policy(binding, raster_source);
                            raster_sources.insert(
                                source_id.clone(),
                                Arc::new(BoundRasterSource {
                                    source_id: source_id.clone(),
                                    provider_key: binding.provider_key.clone(),
                                    provider: Arc::clone(&binding.provider),
                                    image_format: binding.image_format,
                                    tile_resolution: binding.tile_resolution,
                                    policy,
                                }),
                            );
                        }
                        None => diagnostics.push(SourceBindingDiagnostic {
                            source_id: source_id.clone(),
                            detail: "no provider binding serves this raster source".to_string(),
                        }),
                    }
                }
                CompiledSource::GeoJson(geojson_source) => {
                    match self.geojson_bindings.get(source_id) {
                        Some(binding) => geojson_sources.insert(
                            source_id.clone(),
                            Arc::new(bound_geojson_source(
                                source_id,
                                &binding.provider_key,
                                binding.data.clone(),
                                geojson_source,
                            )),
                        ),
                        None => match geojson_source.data.clone() {
                            Some(data) => geojson_sources.insert(
                                source_id.clone(),
                                Arc::new(bound_geojson_source(
                                    source_id,
                                    "inline",
                                    data,
                                    geojson_source,
                                )),
                            ),
                            None => {
                                diagnostics.push(SourceBindingDiagnostic {
                                    source_id: source_id.clone(),
                                    detail: "the GeoJSON source names remote data and no provider binding supplies it"
                                        .to_string(),
                                });
                                None
                            }
                        },
                    };
                }
                CompiledSource::Other(opaque_source) => {
                    diagnostics.push(SourceBindingDiagnostic {
                        source_id: source_id.clone(),
                        detail: format!(
                            "source type {:?} is not rendered",
                            opaque_source.source_type
                        ),
                    });
                }
            }
        }
        BoundStyleSources {
            style: style.clone(),
            vector_sources,
            raster_sources,
            geojson_sources,
            diagnostics,
        }
    }
}

/// Builds the effective TileJSON policy for a vector source binding.
pub(crate) fn vector_binding_policy(
    binding: &VectorSourceBinding,
    source: &CompiledVectorSource,
) -> SourceFetchPolicy {
    let descriptor = binding.provider.source_descriptor();
    SourceFetchPolicy {
        scheme: binding.scheme.unwrap_or(source.scheme),
        bounds: binding.bounds.or(source.bounds),
        minimum_zoom_level: effective_zoom_level(
            descriptor.minimum_zoom_level,
            source.minimum_zoom,
            binding.minimum_zoom_level,
            true,
        ),
        maximum_zoom_level: effective_zoom_level(
            descriptor.maximum_zoom_level,
            source.maximum_zoom,
            binding.maximum_zoom_level,
            false,
        ),
        attribution_text: source
            .attribution
            .clone()
            .unwrap_or(descriptor.attribution_text),
    }
}

/// Builds the effective TileJSON policy for a raster source binding.
fn raster_binding_policy(
    binding: &RasterSourceBinding,
    source: &rustic_maps_style::CompiledRasterSource,
) -> SourceFetchPolicy {
    SourceFetchPolicy {
        scheme: binding.scheme.unwrap_or(source.scheme),
        bounds: binding.bounds.or(source.bounds),
        minimum_zoom_level: effective_zoom_level(
            0,
            source.minimum_zoom,
            binding.minimum_zoom_level,
            true,
        ),
        maximum_zoom_level: effective_zoom_level(
            u8::MAX,
            source.maximum_zoom,
            binding.maximum_zoom_level,
            false,
        ),
        attribution_text: source.attribution.clone().unwrap_or_default(),
    }
}

/// Folds a provider's capability and the style's declared span into one
/// level. The lower edge rounds up, the upper edge rounds down, so the clamp
/// never claims a level either side does not serve.
fn effective_zoom_level(
    provider_level: u8,
    declared_level: Option<f64>,
    binding_level: Option<u8>,
    lower_edge: bool,
) -> u8 {
    let mut level = provider_level;
    if let Some(declared_level) = declared_level {
        let rounded = if lower_edge {
            declared_level.ceil()
        } else {
            declared_level.floor()
        };
        let clamped = rounded.clamp(0.0, f64::from(u8::MAX)) as u8;
        level = if lower_edge {
            level.max(clamped)
        } else {
            level.min(clamped)
        };
    }
    if let Some(binding_level) = binding_level {
        level = if lower_edge {
            level.max(binding_level)
        } else {
            level.min(binding_level)
        };
    }
    level
}

/// Builds a bound GeoJSON source from its declaration.
fn bound_geojson_source(
    source_id: &str,
    provider_key: &str,
    data: serde_json::Value,
    source: &CompiledGeoJsonSource,
) -> BoundGeoJsonSource {
    BoundGeoJsonSource {
        source_id: source_id.to_string(),
        provider_key: provider_key.to_string(),
        data,
        options: GeoJsonTilingOptions {
            extent: DEFAULT_TILE_EXTENT,
            buffer_pixels: source
                .buffer
                .unwrap_or(crate::geojson_source::DEFAULT_GEOJSON_BUFFER_PIXELS),
            simplification_tolerance: source
                .tolerance
                .unwrap_or(crate::geojson_source::DEFAULT_GEOJSON_SIMPLIFICATION_TOLERANCE),
            generate_identifier: source.generate_id.unwrap_or(false),
            layer_name: source_id.to_string(),
        },
    }
}

/// A style resolved against a host's source bindings.
pub struct BoundStyleSources {
    /// The compiled style the bindings serve.
    pub style: CompiledStyle,
    /// The vector sources, keyed by style source id.
    pub vector_sources: BTreeMap<String, Arc<BoundVectorSource>>,
    /// The raster sources, keyed by style source id.
    pub raster_sources: BTreeMap<String, Arc<BoundRasterSource>>,
    /// The GeoJSON sources, keyed by style source id.
    pub geojson_sources: BTreeMap<String, Arc<BoundGeoJsonSource>>,
    /// The sources that could not be bound.
    pub diagnostics: Vec<SourceBindingDiagnostic>,
}

impl BoundStyleSources {
    /// The attribution lines of every bound source, deduplicated in source-id
    /// order.
    pub fn attribution_text(&self) -> String {
        let mut lines = BTreeSet::new();
        for source in self.vector_sources.values() {
            if !source.policy.attribution_text.is_empty() {
                lines.insert(source.policy.attribution_text.clone());
            }
        }
        for source in self.raster_sources.values() {
            if !source.policy.attribution_text.is_empty() {
                lines.insert(source.policy.attribution_text.clone());
            }
        }
        lines
            .into_iter()
            .collect::<Vec<_>>()
            .join(SOURCE_ATTRIBUTION_SEPARATOR)
    }
}

/// One resolved multi-source frame.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MultiSourceVectorFrame {
    /// The overlays in global style order: every source's layer 0 before any
    /// source's layer 1.
    pub overlays: Vec<MapOverlay>,
    /// The frame's label candidates, in layer order.
    pub label_candidates: Vec<MapLabelCandidate>,
    /// How many source tiles contributed geometry.
    pub drawn_source_tile_count: usize,
    /// How many source tiles failed to fetch or decode.
    pub failed_source_tile_count: usize,
    /// The binding diagnostics the frame's style carried.
    pub diagnostics: Vec<SourceBindingDiagnostic>,
    /// The attribution line the frame's sources require.
    pub attribution_text: String,
}

/// Resolves one frame from several vector sources plus any GeoJSON sources.
///
/// Each vector source's visible tiles are fetched through its own provider,
/// with its TileJSON zoom clamp, scheme flip and bounds applied. Each layer is
/// then styled against the tile of the source it names, and the resulting
/// batches concatenate in global compiled layer order. Raster layers are
/// skipped: [`raster_layer_requests`] routes them to the raster scheduler.
pub async fn resolve_multi_source_vector_frame(
    bound: &BoundStyleSources,
    visible_tile_keys: &[TileKey],
    style_zoom_level: f64,
) -> MultiSourceVectorFrame {
    let mut visible_keys = visible_tile_keys.to_vec();
    visible_keys.sort();
    visible_keys.dedup();

    // Fetch every vector source's admitted tiles concurrently. Each future
    // borrows its source and the visible list, so no lock spans an await.
    let mut fetch_futures = Vec::new();
    for source in bound.vector_sources.values() {
        for tile_key in &visible_keys {
            let source = Arc::clone(source);
            let tile_key = *tile_key;
            fetch_futures.push(async move {
                (
                    source.source_id.clone(),
                    tile_key,
                    source.fetch_tile(tile_key).await,
                )
            });
        }
    }
    let mut source_tiles: BTreeMap<(String, TileKey), Arc<VectorTile>> = BTreeMap::new();
    let mut frame = MultiSourceVectorFrame {
        diagnostics: bound.diagnostics.clone(),
        attribution_text: bound.attribution_text(),
        ..MultiSourceVectorFrame::default()
    };
    for (source_id, tile_key, fetched_tile) in join_all(fetch_futures).await {
        match fetched_tile {
            Ok(Some(tile)) => {
                frame.drawn_source_tile_count += 1;
                source_tiles.insert((source_id, tile_key), tile);
            }
            Ok(None) => {}
            Err(_) => frame.failed_source_tile_count += 1,
        }
    }
    // A GeoJSON source tiles locally and deterministically, so it costs no
    // network and cannot fail the frame as a whole.
    for source in bound.geojson_sources.values() {
        for tile_key in &visible_keys {
            match source.tile(*tile_key) {
                Ok(tile) => {
                    frame.drawn_source_tile_count += 1;
                    source_tiles.insert((source.source_id.clone(), *tile_key), Arc::new(tile));
                }
                Err(_) => frame.failed_source_tile_count += 1,
            }
        }
    }

    let mut layer_batches: Vec<Vec<MapOverlay>> = vec![Vec::new(); bound.style.layer_count()];
    for layer in &bound.style.layers {
        if matches!(
            layer.kind,
            CompiledLayerKind::Raster | CompiledLayerKind::Background | CompiledLayerKind::Other
        ) {
            continue;
        }
        let Some(source_id) = layer.source.as_deref() else {
            continue;
        };
        for tile_key in &visible_keys {
            let Some(tile) = source_tiles.get(&(source_id.to_string(), *tile_key)) else {
                continue;
            };
            let address = vector_tile_address(*tile_key);
            if let Some(batch) = layer_batches.get_mut(layer.order_index) {
                batch.extend(overlays_for_compiled_layer_at_zoom(
                    tile,
                    address,
                    layer,
                    style_zoom_level,
                ));
            }
            if layer.kind == CompiledLayerKind::Symbol {
                frame
                    .label_candidates
                    .extend(label_candidates_for_compiled_layer_at_zoom(
                        tile,
                        address,
                        layer,
                        style_zoom_level,
                    ));
            }
        }
    }
    frame.overlays = layer_batches.into_iter().flatten().collect();
    frame
}

/// One raster layer's tile request group, ready for a
/// [`crate::tile_scheduler::TileScheduler`] built over the bound source's
/// provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RasterLayerRequest {
    /// The style layer id.
    pub layer_id: String,
    /// The style source id the layer reads.
    pub source_id: String,
    /// The tile addresses to feed the raster scheduler, after the source's
    /// zoom span, bounds and scheme policy.
    pub tile_keys: Vec<TileKey>,
}

/// Routes every visible `raster` style layer to its bound raster source.
///
/// The returned groups are in compiled layer order. Each group's
/// [`RasterLayerRequest::tile_keys`] is what the host passes to a
/// [`crate::tile_scheduler::TileScheduler`] constructed with the source's
/// [`BoundRasterSource`] as the provider, so a style raster layer composites
/// through the existing raster scheduler and never the vector overlay
/// pipeline. A layer whose source is unbound, not raster, outside its zoom
/// span or off its bounds contributes no request.
pub fn raster_layer_requests(
    bound: &BoundStyleSources,
    visible_tile_keys: &[TileKey],
    style_zoom_level: f64,
) -> Vec<RasterLayerRequest> {
    let mut visible_keys = visible_tile_keys.to_vec();
    visible_keys.sort();
    visible_keys.dedup();
    let mut requests = Vec::new();
    for layer in &bound.style.layers {
        if layer.kind != CompiledLayerKind::Raster || !layer.is_visible_at_zoom(style_zoom_level) {
            continue;
        }
        let Some(source_id) = layer.source.as_deref() else {
            continue;
        };
        let Some(source) = bound.raster_sources.get(source_id) else {
            continue;
        };
        let tile_keys = visible_keys
            .iter()
            .copied()
            .filter(|tile_key| source.policy.fetch_tile_key(*tile_key).is_some())
            .collect::<Vec<_>>();
        if tile_keys.is_empty() {
            continue;
        }
        requests.push(RasterLayerRequest {
            layer_id: layer.id.clone(),
            source_id: source_id.to_string(),
            tile_keys,
        });
    }
    requests
}

/// The geographic bounds of one web-mercator tile as
/// `[west, south, east, north]`.
pub fn tile_geographic_bounds(tile_key: TileKey) -> [f64; 4] {
    let tiles_per_axis = 1u64
        .checked_shl(u32::from(tile_key.zoom_level))
        .unwrap_or(u64::MAX) as f64;
    let west = f64::from(tile_key.tile_x) / tiles_per_axis;
    let east = (f64::from(tile_key.tile_x) + 1.0) / tiles_per_axis;
    let north = f64::from(tile_key.tile_y) / tiles_per_axis;
    let south = (f64::from(tile_key.tile_y) + 1.0) / tiles_per_axis;
    let north_west = normalized_world_to_coordinate([west, north]);
    let south_east = normalized_world_to_coordinate([east, south]);
    [
        north_west.longitude,
        south_east.latitude,
        south_east.longitude,
        north_west.latitude,
    ]
}

/// Flips a row from the XYZ (north-origin) convention to TMS (south-origin).
pub fn flipped_tile_row(tile_y: u32, zoom_level: u8) -> u32 {
    let tiles_per_axis = 1u64.checked_shl(u32::from(zoom_level)).unwrap_or(u64::MAX);
    tiles_per_axis
        .saturating_sub(1)
        .saturating_sub(u64::from(tile_y)) as u32
}

/// The address the vector conversion uses for a tile key.
fn vector_tile_address(tile_key: TileKey) -> VectorTileAddress {
    VectorTileAddress {
        zoom_level: tile_key.zoom_level,
        tile_x: tile_key.tile_x,
        tile_y: tile_key.tile_y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tile_scheduler::TileScheduler;
    use crate::tile_texture_pool::TileTexturePool;
    use crate::vector_style::vector_tile_style_from_json;
    use async_trait::async_trait;
    use geospatial_traits::VectorTileSourceDescriptor;
    use geospatial_types::VectorTileData;
    use std::sync::Mutex;

    /// The committed OpenMapTiles z14 fixture: its water polygons and
    /// transportation_name lines let one source serve each half of the test
    /// style.
    const FIXTURE_TILE_BYTES: &[u8] =
        include_bytes!("../../../fixtures/openfreemap/openmaptiles_tile_z14_x2661_y6286.pbf");
    /// The fixture tile's address.
    const FIXTURE_TILE_KEY: TileKey = TileKey {
        tile_x: 2661,
        tile_y: 6286,
        zoom_level: 14,
    };

    /// A vector provider serving the committed fixture and recording requests.
    struct RecordingVectorProvider {
        descriptor_source_name: &'static str,
        attribution_text: &'static str,
        requested_tiles: Mutex<Vec<(u32, u32, u8)>>,
    }

    impl RecordingVectorProvider {
        fn new(descriptor_source_name: &'static str, attribution_text: &'static str) -> Arc<Self> {
            Arc::new(Self {
                descriptor_source_name,
                attribution_text,
                requested_tiles: Mutex::new(Vec::new()),
            })
        }

        fn requested_tiles(&self) -> Vec<(u32, u32, u8)> {
            self.requested_tiles
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }
    }

    #[async_trait]
    impl VectorTileProvider for RecordingVectorProvider {
        fn source_descriptor(&self) -> VectorTileSourceDescriptor {
            VectorTileSourceDescriptor {
                source_name: self.descriptor_source_name.to_string(),
                attribution_text: self.attribution_text.to_string(),
                minimum_zoom_level: 0,
                maximum_zoom_level: 14,
                style_hints: Vec::new(),
            }
        }

        async fn fetch_vector_tile(
            &self,
            tile_x: u32,
            tile_y: u32,
            zoom_level: u8,
        ) -> Result<VectorTileData, SDKError> {
            self.requested_tiles
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((tile_x, tile_y, zoom_level));
            VectorTileData::from_wire_bytes(tile_x, tile_y, zoom_level, FIXTURE_TILE_BYTES.to_vec())
        }
    }

    /// A raster provider recording requests and returning opaque bytes; the
    /// scheduler never decodes them, so routing is what the test observes.
    struct RecordingRasterProvider {
        requested_tiles: Mutex<Vec<(u32, u32, u8)>>,
    }

    impl RecordingRasterProvider {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                requested_tiles: Mutex::new(Vec::new()),
            })
        }

        fn fetch_count(&self) -> usize {
            self.requested_tiles
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len()
        }
    }

    #[async_trait]
    impl RasterTileProvider for RecordingRasterProvider {
        async fn fetch_raster_tile(
            &self,
            tile_x: u32,
            tile_y: u32,
            zoom_level: u8,
            image_format: RasterImageFormat,
            tile_resolution: RasterTileResolution,
        ) -> Result<RasterTileData, SDKError> {
            self.requested_tiles
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((tile_x, tile_y, zoom_level));
            Ok(RasterTileData {
                tile_x,
                tile_y,
                zoom_level,
                image_format,
                tile_resolution,
                image_bytes: vec![0x89, b'P', b'N', b'G'],
            })
        }
    }

    /// A two-source style: each layer reads a different source-layer from its
    /// own source.
    const TWO_SOURCE_STYLE_JSON: &str = r##"{
        "version": 8,
        "sources": {
            "west": { "type": "vector" },
            "east": { "type": "vector" }
        },
        "layers": [
            {
                "id": "water",
                "type": "fill",
                "source": "west",
                "source-layer": "water",
                "paint": { "fill-color": "#0000ff" }
            },
            {
                "id": "roads",
                "type": "line",
                "source": "east",
                "source-layer": "transportation_name",
                "paint": { "line-color": "#ff0000", "line-width": 2 }
            }
        ]
    }"##;

    #[tokio::test]
    async fn two_vector_sources_render_into_one_frame_in_global_layer_order() {
        let style = vector_tile_style_from_json(TWO_SOURCE_STYLE_JSON).expect("the style compiles");
        let west_provider = RecordingVectorProvider::new("fixture-west", "West data");
        let east_provider = RecordingVectorProvider::new("fixture-east", "East data");
        let bindings = StyleSourceBindings::new()
            .with_vector_source(VectorSourceBinding::new(
                "west",
                "fixture-west",
                Arc::clone(&west_provider) as Arc<dyn VectorTileProvider>,
            ))
            .with_vector_source(VectorSourceBinding::new(
                "east",
                "fixture-east",
                Arc::clone(&east_provider) as Arc<dyn VectorTileProvider>,
            ));
        let bound = bindings.bind_style(&style);
        assert!(bound.diagnostics.is_empty(), "{:?}", bound.diagnostics);
        let frame = resolve_multi_source_vector_frame(&bound, &[FIXTURE_TILE_KEY], 14.0).await;
        assert_eq!(frame.drawn_source_tile_count, 2);
        assert_eq!(frame.failed_source_tile_count, 0);
        assert_eq!(west_provider.requested_tiles(), [(2661, 6286, 14)]);
        assert_eq!(east_provider.requested_tiles(), [(2661, 6286, 14)]);
        assert!(
            frame
                .overlays
                .iter()
                .any(|overlay| matches!(overlay, MapOverlay::FilledPolygon(_))),
            "the water source contributes a fill"
        );
        assert!(
            frame
                .overlays
                .iter()
                .any(|overlay| matches!(overlay, MapOverlay::RoutePolyline(_))),
            "the road source contributes a line"
        );
        // Global layer order: every water fill paints before every road line.
        let last_fill_index = frame
            .overlays
            .iter()
            .rposition(|overlay| matches!(overlay, MapOverlay::FilledPolygon(_)))
            .expect("a fill exists");
        let first_line_index = frame
            .overlays
            .iter()
            .position(|overlay| matches!(overlay, MapOverlay::RoutePolyline(_)))
            .expect("a line exists");
        assert!(last_fill_index < first_line_index);
        assert_eq!(frame.attribution_text, "East data — West data");
    }

    #[test]
    fn an_unbound_source_is_a_diagnostic_and_the_rest_still_binds() {
        let style = vector_tile_style_from_json(TWO_SOURCE_STYLE_JSON).expect("the style compiles");
        let west_provider = RecordingVectorProvider::new("fixture-west", "West data");
        let bindings = StyleSourceBindings::new().with_vector_source(VectorSourceBinding::new(
            "west",
            "fixture-west",
            Arc::clone(&west_provider) as Arc<dyn VectorTileProvider>,
        ));
        let bound = bindings.bind_style(&style);
        assert!(bound.vector_sources.contains_key("west"));
        assert!(!bound.vector_sources.contains_key("east"));
        assert_eq!(bound.diagnostics.len(), 1);
        assert_eq!(bound.diagnostics[0].source_id, "east");
        assert!(bound.diagnostics[0].detail.contains("no provider binding"));
    }

    #[tokio::test]
    async fn the_tilejson_zoom_span_scheme_and_bounds_shape_the_fetch() {
        let style = vector_tile_style_from_json(
            r#"{
                "version": 8,
                "sources": {
                    "base": {
                        "type": "vector",
                        "minzoom": 5,
                        "maxzoom": 6,
                        "scheme": "tms",
                        "bounds": [-180.0, -85.0, 180.0, 85.0]
                    }
                },
                "layers": [{ "id": "roads", "type": "line", "source": "base", "source-layer": "transportation_name" }]
            }"#,
        )
        .expect("the style compiles");
        let provider = RecordingVectorProvider::new("fixture", "Fixture data");
        let bindings = StyleSourceBindings::new().with_vector_source(VectorSourceBinding::new(
            "base",
            "fixture",
            Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
        ));
        let bound = bindings.bind_style(&style);
        let source = bound.vector_sources.get("base").expect("the source binds");
        assert_eq!(source.served_zoom_level(3), 5);
        assert_eq!(source.served_zoom_level(5), 5);
        assert_eq!(source.served_zoom_level(6), 6);
        assert_eq!(source.served_zoom_level(9), 6);
        assert_eq!(source.policy.scheme, TileScheme::Tms);

        let requested_tile_key = TileKey::new(1, 2, 5).expect("the tile address is valid");
        source
            .fetch_tile(requested_tile_key)
            .await
            .expect("the fixture decodes")
            .expect("the tile is inside the policy");
        // z5 has 32 rows, so XYZ row 2 becomes TMS row 29.
        assert_eq!(provider.requested_tiles(), [(1, 29, 5)]);

        // Outside the declared zoom span no request is made.
        assert!(
            source
                .fetch_tile(TileKey::new(1, 2, 7).expect("valid"))
                .await
                .expect("no request")
                .is_none()
        );
        assert_eq!(provider.requested_tiles().len(), 1);

        // Outside the declared bounds no request is made either.
        let bounded_style = vector_tile_style_from_json(
            r#"{
                "version": 8,
                "sources": {
                    "base": { "type": "vector", "bounds": [0.0, 0.0, 1.0, 1.0] }
                },
                "layers": [{ "id": "roads", "type": "line", "source": "base", "source-layer": "transportation_name" }]
            }"#,
        )
        .expect("the bounded style compiles");
        let bounded_bindings =
            StyleSourceBindings::new().with_vector_source(VectorSourceBinding::new(
                "base",
                "fixture",
                Arc::clone(&provider) as Arc<dyn VectorTileProvider>,
            ));
        let bounded = bounded_bindings.bind_style(&bounded_style);
        let bounded_source = bounded
            .vector_sources
            .get("base")
            .expect("the source binds");
        assert!(
            bounded_source
                .fetch_tile(requested_tile_key)
                .await
                .expect("no request")
                .is_none()
        );
        assert_eq!(provider.requested_tiles().len(), 1);
    }

    #[tokio::test]
    async fn a_style_raster_layer_routes_to_the_raster_scheduler_not_the_vector_frame() {
        let style = vector_tile_style_from_json(
            r#"{
                "version": 8,
                "sources": {
                    "imagery": { "type": "raster", "minzoom": 0, "maxzoom": 20 },
                    "base": { "type": "vector" }
                },
                "layers": [
                    { "id": "imagery", "type": "raster", "source": "imagery" },
                    { "id": "water", "type": "fill", "source": "base", "source-layer": "water" }
                ]
            }"#,
        )
        .expect("the style compiles");
        let raster_provider = RecordingRasterProvider::new();
        let vector_provider = RecordingVectorProvider::new("fixture", "Fixture data");
        let bindings = StyleSourceBindings::new()
            .with_raster_source(RasterSourceBinding::new(
                "imagery",
                "fixture-raster",
                Arc::clone(&raster_provider) as Arc<dyn RasterTileProvider>,
                RasterImageFormat::Png,
                RasterTileResolution::Standard,
            ))
            .with_vector_source(VectorSourceBinding::new(
                "base",
                "fixture",
                Arc::clone(&vector_provider) as Arc<dyn VectorTileProvider>,
            ));
        let bound = bindings.bind_style(&style);
        let requests = raster_layer_requests(&bound, &[FIXTURE_TILE_KEY], 14.0);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].layer_id, "imagery");
        assert_eq!(requests[0].source_id, "imagery");
        assert_eq!(requests[0].tile_keys, [FIXTURE_TILE_KEY]);

        let raster_binding = Arc::clone(
            bound
                .raster_sources
                .get("imagery")
                .expect("the raster source binds"),
        );
        let raster_source: Arc<dyn RasterTileProvider> = raster_binding;
        let mut scheduler = TileScheduler::new(
            raster_source,
            TileTexturePool::new(0),
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
            4,
        );
        scheduler.prepare_frame(&requests[0].tile_keys).await;
        assert_eq!(raster_provider.fetch_count(), 1);
        assert!(vector_provider.requested_tiles().is_empty());
    }

    #[test]
    fn flipped_tile_rows_and_bounds_use_the_web_mercator_grid() {
        assert_eq!(flipped_tile_row(0, 0), 0);
        assert_eq!(flipped_tile_row(0, 1), 1);
        assert_eq!(flipped_tile_row(2, 5), 29);
        let bounds = tile_geographic_bounds(TileKey::new(0, 0, 1).expect("valid"));
        let [west, south, east, north] = bounds;
        assert!((west - -180.0).abs() < 1e-9);
        assert!((east - 0.0).abs() < 1e-9);
        assert!(north > south);
        assert!((north - 85.051_128_779_806_59).abs() < 1e-9);
        assert!((south - 0.0).abs() < 1e-9);
    }
}
