//! Provider slots and the browser fixture-endpoint bridge.
//!
//! The generated contract `MapSdk` owns the per-capability provider registry
//! and answers every provider call. This module keeps only what the canvas
//! still needs beside it: the shared slots that mirror a registration made
//! through the generated `MapSdk` into the canvas' live base, the raster
//! registration revision the canvas stamps in-flight fetches with, the
//! browser-only fixture endpoint override the test harness points at its
//! recorded payloads, the Clear companion the browser surface needs because
//! the contract has no clearing member, and the keyless URL-template raster
//! fallback.
//!
//! The registry is module-local and single-threaded; no credential is
//! persisted or logged.

use crate::exports::{ERROR_CODE_INVALID_ARGUMENT, failure_with_code, sdk_error_failure};
use geospatial_traits::{RasterTileProvider, VectorTileProvider};
use geospatial_types::{RasterImageFormat, RasterTileResolution};
use rustic_maps_core::everymap_bridge::{
    EveryMapRasterTileProvider, map_provider_error, tile_source_budget_rates,
};
use rustic_maps_core::provider_observation::{
    DEFAULT_REFILL_TOKENS_PER_SECOND, DEFAULT_TOKEN_BUCKET_CAPACITY_TOKENS,
};
use rustic_maps_core::{ProviderContract, ProviderRegistry, StyleResourceClient};
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use wasm_bindgen::prelude::*;

/// Lowest zoom a raster source serves when its declaration omits the field:
/// the whole world in one tile.
const DEFAULT_RASTER_SOURCE_MINIMUM_ZOOM_LEVEL: u8 = 0;
/// Highest zoom a raster source serves when its declaration omits the field:
/// the deepest zoom the shipped provider serves.
const DEFAULT_RASTER_SOURCE_MAXIMUM_ZOOM_LEVEL: u8 = 22;

/// What one provider selection records: which key serves which capability.
struct ProviderSelection {
    /// The capability the selection serves.
    contract: ProviderContract,
    /// The provider's stable registry key.
    provider_key: String,
}

/// A browser-only request-redirection seam: the base URL a provider key
/// addresses instead of its public endpoint. The test harness points the
/// generated registry at the same-origin fixture routes with it.
struct ProviderBaseUrlOverride {
    /// The provider key the override applies to.
    provider_key: String,
    /// The base URL the provider's client is built with.
    base_url: String,
}

/// The module's canvas-facing provider state: the selections the canvas reads
/// and the browser-only base URL overrides.
struct ProviderState {
    /// One slot per provider contract. A page configures the generated
    /// contract registry, not this one, so the slots stay empty; the canvas
    /// still consults them as a fallback when no shared slot is mirrored.
    registry: ProviderRegistry,
    /// Which provider serves each configured capability.
    selections: Vec<ProviderSelection>,
    /// Base URL overrides, by provider key.
    base_url_overrides: Vec<ProviderBaseUrlOverride>,
}

thread_local! {
    /// The module's canvas-facing provider state.
    static PROVIDER_STATE: RefCell<ProviderState> = RefCell::new(ProviderState {
        registry: ProviderRegistry::new(),
        selections: Vec::new(),
        base_url_overrides: Vec::new(),
    });
    /// The raster-tile provider the generated contract registry installed.
    /// The binding crate mirrors every raster-tile registration here, and the
    /// canvas consults this slot before its own registration.
    static SHARED_RASTER_TILE_PROVIDER: RefCell<Option<Arc<dyn RasterTileProvider>>> =
        const { RefCell::new(None) };
    /// The vector-tile provider the generated contract registry installed:
    /// the mirror that lets a provider-backed base installed through the
    /// generated `MapSdk` and one configured through the page's own surface
    /// resolve to the same source.
    static SHARED_VECTOR_TILE_PROVIDER: RefCell<Option<Arc<dyn VectorTileProvider>>> =
        const { RefCell::new(None) };
    /// The authenticated style-resource fetch the generated contract registry
    /// installed beside the vector-tile provider.
    static SHARED_VECTOR_STYLE_RESOURCES: RefCell<Option<Arc<dyn StyleResourceClient>>> =
        const { RefCell::new(None) };
    /// The hosts the provider declared for the style resources it publishes,
    /// mirrored beside the shared fetch so a base installed through the
    /// generated `MapSdk` applies the same credential scope the canvas does.
    static SHARED_VECTOR_STYLE_RESOURCE_HOSTS: RefCell<Vec<String>> =
        const { RefCell::new(Vec::new()) };
    /// The page-registered URL-template raster source: the keyless fallback.
    /// It serves only while no provider is configured for the raster-tile
    /// contract.
    static LIVE_RASTER_FALLBACK_PROVIDER: RefCell<Option<Arc<dyn RasterTileProvider>>> =
        const { RefCell::new(None) };
    /// The zoom ceiling the canvas' live raster fallback fetches at, as the
    /// fallback source declares it.
    static LIVE_RASTER_MAXIMUM_ZOOM_LEVEL: RefCell<Option<u8>> = const { RefCell::new(None) };
    /// Monotonic revision of the provider a raster tile request resolves
    /// through. Every registration or clear that can change the serving
    /// source bumps it; the canvas stamps its in-flight fetches with the
    /// revision they started under and discards a result that resolves after
    /// a later revision.
    static RASTER_TILE_PROVIDER_REGISTRATION_REVISION: Cell<u64> = const { Cell::new(0) };
}

/// Marks a change to the provider raster tile requests resolve through.
fn bump_raster_tile_provider_registration_revision() {
    RASTER_TILE_PROVIDER_REGISTRATION_REVISION.with(|revision_slot| {
        revision_slot.set(revision_slot.get().wrapping_add(1));
    });
}

/// The current revision of the raster provider registration. A consumer that
/// pools decoded payloads or retry schedules compares the revision it built
/// them under against this value and discards state from an older source.
pub(crate) fn raster_tile_provider_registration_revision() -> u64 {
    RASTER_TILE_PROVIDER_REGISTRATION_REVISION.with(|revision_slot| revision_slot.get())
}

/// Runs `operation` against the module state under one borrow.
fn with_provider_state<T>(operation: impl FnOnce(&mut ProviderState) -> T) -> T {
    PROVIDER_STATE.with(|state_slot| operation(&mut state_slot.borrow_mut()))
}

/// The base URL override a provider key was given, if any.
fn base_url_override_for(state: &ProviderState, provider_key: &str) -> Option<String> {
    state
        .base_url_overrides
        .iter()
        .find(|override_entry| override_entry.provider_key == provider_key)
        .map(|override_entry| override_entry.base_url.clone())
}

/// The base URL override a provider key was given, if any.
///
/// This is the read side of [`set_provider_base_url`], carried across the
/// crate boundary so the generated contract registry in
/// `rustic-maps-native-core` installs the same fixture-backed provider the
/// browser canvas' own registration builds. It stays browser-only and
/// exists for a page that serves a provider's routes itself, such as the
/// test harness; a deployed page never sets an override.
#[cfg(target_arch = "wasm32")]
pub fn base_url_override(provider_key: &str) -> Option<String> {
    PROVIDER_STATE.with(|state_slot| base_url_override_for(&state_slot.borrow(), provider_key))
}

/// Records which provider serves a capability, replacing any earlier
/// selection for it.
fn record_selection(
    state: &mut ProviderState,
    contract: ProviderContract,
    provider_key: &str,
    _display_name: &str,
) {
    state
        .selections
        .retain(|selection| selection.contract != contract);
    state.selections.push(ProviderSelection {
        contract,
        provider_key: provider_key.to_string(),
    });
}

/// Records the provider the generated contract registry chose for a
/// capability, so the canvas' own provider-key checks (a provider-backed
/// vector base's descriptor, a raster selection) read the same choice the
/// page made through `MapSdk`.
///
/// `rustic-maps-native-core` mirrors every successful registration here on
/// wasm, beside the provider handle it publishes to the canvas' shared
/// slots. A native build has no canvas to inform.
pub fn record_shared_provider_selection(
    contract: ProviderContract,
    provider_key: &str,
    display_name: &str,
) {
    with_provider_state(|state| {
        record_selection(state, contract, provider_key, display_name);
    });
}

/// Points a provider key's requests at a base URL instead of its public
/// endpoint — a browser-only seam for a page that serves the provider's
/// routes itself, such as the test harness. Register the provider after
/// calling this; the override applies to every later registration of the key.
#[wasm_bindgen(js_name = setProviderBaseUrl)]
pub fn set_provider_base_url(provider_key: String, base_url: String) -> Result<(), JsValue> {
    if base_url.trim().is_empty() {
        return Err(failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            "a provider base URL must not be empty",
        ));
    }
    with_provider_state(|state| {
        state
            .base_url_overrides
            .retain(|override_entry| override_entry.provider_key != provider_key);
        state.base_url_overrides.push(ProviderBaseUrlOverride {
            provider_key,
            base_url: base_url.trim().to_string(),
        });
        Ok(())
    })
}

/// Empties every canvas-facing provider slot and forgets the selections — the
/// browser counterpart of the settings surface's Clear act. The contract
/// carries no clearing member, and a page that clears keeps nothing
/// afterwards.
#[wasm_bindgen(js_name = clearProviderRegistrations)]
pub fn clear_provider_registrations() {
    with_provider_state(|state| {
        *state = ProviderState {
            registry: ProviderRegistry::new(),
            selections: Vec::new(),
            base_url_overrides: std::mem::take(&mut state.base_url_overrides),
        };
    });
    // The page's URL-template fallback survives a clear, and so does its
    // fetch ceiling: a cleared page keeps drawing live tiles.
    bump_raster_tile_provider_registration_revision();
}

/// Installs the raster-tile provider the generated contract registry holds,
/// so the canvas' live fallback fetches through the same source the contract
/// surface serves. `None` clears the shared slot. The binding crate mirrors
/// every registration here; the canvas consults it before its own
/// registration, so one provider serves both surfaces.
pub fn set_shared_raster_tile_provider(provider: Option<Arc<dyn RasterTileProvider>>) {
    SHARED_RASTER_TILE_PROVIDER.with(|provider_slot| {
        *provider_slot.borrow_mut() = provider;
    });
    bump_raster_tile_provider_registration_revision();
}

/// Records the zoom ceiling the canvas' live raster fallback should fetch at.
fn set_live_raster_maximum_zoom_level(maximum_zoom_level: Option<u8>) {
    LIVE_RASTER_MAXIMUM_ZOOM_LEVEL.with(|zoom_slot| {
        *zoom_slot.borrow_mut() = maximum_zoom_level;
    });
}

/// The zoom ceiling the canvas' live raster fallback fetches at, when a
/// raster source is installed, and `None` while the canvas serves only its
/// recorded fixtures.
pub(crate) fn live_raster_maximum_zoom_level() -> Option<u8> {
    LIVE_RASTER_MAXIMUM_ZOOM_LEVEL.with(|zoom_slot| *zoom_slot.borrow())
}

/// The raster-tile provider the canvas' live fallback fetches through: the
/// shared slot the contract registry mirrors first, then the configured
/// provider for the raster-tile contract, then the page-registered
/// URL-template fallback. A configured provider always outranks the
/// fallback, so a credential is never displaced by the keyless source.
/// Absent while nothing serves, in which case the canvas draws only its
/// recorded fixtures.
pub(crate) fn registered_raster_tile_provider() -> Option<Arc<dyn RasterTileProvider>> {
    SHARED_RASTER_TILE_PROVIDER
        .with(|provider_slot| provider_slot.borrow().clone())
        .or_else(|| with_provider_state(|state| state.registry.require_raster_tile_provider().ok()))
        .or_else(|| {
            LIVE_RASTER_FALLBACK_PROVIDER.with(|provider_slot| provider_slot.borrow().clone())
        })
}

/// The page-registered URL-template raster source, if any.
///
/// The generated contract registry in `rustic-maps-native-core` reads this on
/// wasm when its own raster-tile slot is empty, so the keyless fallback the
/// canvas draws also answers a generated `fetch_raster_tile`.
pub fn browser_raster_tile_fallback() -> Option<Arc<dyn RasterTileProvider>> {
    LIVE_RASTER_FALLBACK_PROVIDER.with(|provider_slot| provider_slot.borrow().clone())
}

/// Clears every provider slot the canvas consults on the generated contract
/// registry's behalf — the browser-only companion of the page's Clear act.
///
/// A page that replaces its `MapSdk` drops the native registry, but the
/// mirrors it published to the canvas would otherwise keep serving. The
/// page-registered URL-template fallback survives, exactly as it survives
/// [`clear_provider_registrations`].
#[wasm_bindgen(js_name = clearSharedProviderSlots)]
pub fn clear_shared_provider_slots() {
    SHARED_RASTER_TILE_PROVIDER.with(|provider_slot| {
        *provider_slot.borrow_mut() = None;
    });
    SHARED_VECTOR_TILE_PROVIDER.with(|provider_slot| {
        *provider_slot.borrow_mut() = None;
    });
    SHARED_VECTOR_STYLE_RESOURCES.with(|style_slot| {
        *style_slot.borrow_mut() = None;
    });
    SHARED_VECTOR_STYLE_RESOURCE_HOSTS.with(|hosts_slot| hosts_slot.borrow_mut().clear());
    // The selections the generated registry mirrored are its choices too: a
    // cleared page names no serving provider.
    with_provider_state(|state| state.selections.clear());
    bump_raster_tile_provider_registration_revision();
}

/// Installs the vector-tile provider the generated contract registry holds,
/// so a provider-backed base installed through that surface resolves to the
/// same source the canvas draws. `None` clears the shared slot.
pub fn set_shared_vector_tile_provider(provider: Option<Arc<dyn VectorTileProvider>>) {
    SHARED_VECTOR_TILE_PROVIDER.with(|provider_slot| {
        *provider_slot.borrow_mut() = provider;
    });
}

/// The vector-tile provider the module serves: the shared slot the contract
/// registry mirrors first, then the provider configured through the page's
/// own registration. `None` while nothing serves the capability.
pub(crate) fn registered_vector_tile_provider() -> Option<Arc<dyn VectorTileProvider>> {
    SHARED_VECTOR_TILE_PROVIDER
        .with(|provider_slot| provider_slot.borrow().clone())
        .or_else(|| with_provider_state(|state| state.registry.require_vector_tile_provider().ok()))
}

/// Installs the authenticated style-resource fetch and the hosts the
/// provider declared for it, so a base installed through the generated
/// contract registry loads the provider's own style document and resources.
/// `None` clears both shared slots.
pub fn set_shared_vector_style_resources(
    style_resources: Option<Arc<dyn StyleResourceClient>>,
    declared_hosts: Vec<String>,
) {
    SHARED_VECTOR_STYLE_RESOURCES.with(|style_slot| {
        *style_slot.borrow_mut() = style_resources;
    });
    SHARED_VECTOR_STYLE_RESOURCE_HOSTS.with(|hosts_slot| {
        *hosts_slot.borrow_mut() = declared_hosts;
    });
}

/// The style-resource fetch the module serves: the shared slot the contract
/// registry mirrors first, then the one registered through the page's own
/// surface. `None` while the vector-tile provider publishes no hosted style.
pub(crate) fn registered_vector_style_resources() -> Option<Arc<dyn StyleResourceClient>> {
    SHARED_VECTOR_STYLE_RESOURCES
        .with(|style_slot| style_slot.borrow().clone())
        .or_else(|| {
            with_provider_state(|state| state.registry.require_vector_tile_style_resources().ok())
        })
}

/// The hosts the serving provider declared for its published style
/// resources: the shared slot the contract registry mirrors first, then the
/// one registered through the page's own surface.
pub(crate) fn registered_vector_style_resource_hosts() -> Vec<String> {
    let shared_hosts =
        SHARED_VECTOR_STYLE_RESOURCE_HOSTS.with(|hosts_slot| hosts_slot.borrow().clone());
    if shared_hosts.is_empty() {
        with_provider_state(|state| state.registry.vector_tile_style_resource_hosts())
    } else {
        shared_hosts
    }
}

/// The provider key the module recorded for one capability, or `None` while
/// nothing is configured for it. The canvas uses it to refuse a
/// provider-backed source that names a different provider.
pub(crate) fn selected_provider_key(contract: ProviderContract) -> Option<String> {
    with_provider_state(|state| {
        state
            .selections
            .iter()
            .find(|selection| selection.contract == contract)
            .map(|selection| selection.provider_key.clone())
    })
}

/// Registers a keyless URL-template raster source as the canvas' live
/// fallback. The declaration's required texts, optional zoom range, image
/// format, tile resolution and pacing rates are validated here; absent zoom
/// levels and pacing fields fall back to the documented defaults, and the
/// format defaults to `Png`, the resolution to `Standard`.
///
/// A malformed template, a blank source name, an inverted zoom range or an
/// unusable pacing rate throws `InvalidCoordinateInput` with a precise detail,
/// and the previous source keeps serving. The canvas' live raster fallback
/// fetches through the registered source at the zoom ceiling it declares.
#[wasm_bindgen(js_name = registerUrlTemplateRasterTileSource)]
pub fn register_url_template_raster_tile_source(source: JsValue) -> Result<(), JsValue> {
    if !source.is_object() {
        return Err(failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            "the raster tile source argument must be an object",
        ));
    }
    let url_template = required_source_text(&source, "urlTemplate")?;
    let source_name = required_source_text(&source, "sourceName")?;
    let minimum_zoom_level = source_zoom_level(
        &source,
        "minimumZoomLevel",
        DEFAULT_RASTER_SOURCE_MINIMUM_ZOOM_LEVEL,
    )?;
    let maximum_zoom_level = source_zoom_level(
        &source,
        "maximumZoomLevel",
        DEFAULT_RASTER_SOURCE_MAXIMUM_ZOOM_LEVEL,
    )?;
    let image_format = match source_text_field(&source, "imageFormat")?
        .unwrap_or_else(|| "png".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => RasterImageFormat::Png,
        "jpeg" | "jpg" => RasterImageFormat::Jpeg,
        _ => {
            return Err(failure_with_code(
                ERROR_CODE_INVALID_ARGUMENT,
                "the raster tile image format must be png or jpeg",
            ));
        }
    };
    let tile_resolution = match source_text_field(&source, "tileResolution")?
        .unwrap_or_else(|| "standard".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "standard" => RasterTileResolution::Standard,
        "retina" => RasterTileResolution::Retina,
        _ => {
            return Err(failure_with_code(
                ERROR_CODE_INVALID_ARGUMENT,
                "the raster tile resolution must be standard or retina",
            ));
        }
    };
    let user_agent = source_text_field(&source, "userAgent")?.unwrap_or_default();
    let (capacity_tokens, refill_tokens_per_second) = tile_source_budget_rates(
        &source_name,
        source_number_field(&source, "tokenBucketCapacityTokens")?
            .unwrap_or(DEFAULT_TOKEN_BUCKET_CAPACITY_TOKENS),
        source_number_field(&source, "refillTokensPerSecond")?
            .unwrap_or(DEFAULT_REFILL_TOKENS_PER_SECOND),
    )
    .map_err(|error| sdk_error_failure(&error))?;
    let pacing = everymap_providers_url_template_raster::TileSourcePacing::new(
        capacity_tokens,
        refill_tokens_per_second,
    )
    .with_maximum_wait_seconds(rustic_maps_core::everymap_bridge::TILE_SOURCE_MAXIMUM_WAIT_SECONDS);
    let headers = if user_agent.is_empty() {
        Vec::new()
    } else {
        vec![("User-Agent".to_string(), user_agent)]
    };
    let raster_tile_provider =
        everymap_providers_url_template_raster::UrlTemplateRasterTileProvider::new(
            everymap_providers_url_template_raster::UrlTemplateRasterTileSource {
                url_template,
                source_name: source_name.clone(),
                attribution_text: String::new(),
                terms_text: String::new(),
                minimum_zoom_level,
                maximum_zoom_level,
                image_format: match image_format {
                    RasterImageFormat::Png => {
                        everymap_core::domains::tiling::RasterImageFormat::Png
                    }
                    RasterImageFormat::Jpeg => {
                        everymap_core::domains::tiling::RasterImageFormat::Jpeg
                    }
                },
                tile_resolution: match tile_resolution {
                    RasterTileResolution::Standard => {
                        everymap_core::domains::tiling::RasterTileResolution::Standard
                    }
                    RasterTileResolution::Retina => {
                        everymap_core::domains::tiling::RasterTileResolution::Retina
                    }
                },
                subdomains: Vec::new(),
                headers,
                pacing: Some(pacing),
            },
        )
        .map_err(|error| sdk_error_failure(&map_provider_error(error)))?;
    LIVE_RASTER_FALLBACK_PROVIDER.with(|provider_slot| {
        *provider_slot.borrow_mut() = Some(Arc::new(EveryMapRasterTileProvider::new(Arc::new(
            raster_tile_provider,
        ))));
    });
    set_live_raster_maximum_zoom_level(Some(maximum_zoom_level));
    bump_raster_tile_provider_registration_revision();
    Ok(())
}

/// Removes the page-registered URL-template raster source. A configured
/// raster provider is untouched; with none, the canvas falls back to its
/// recorded fixtures.
#[wasm_bindgen(js_name = clearUrlTemplateRasterTileSource)]
pub fn clear_url_template_raster_tile_source() {
    LIVE_RASTER_FALLBACK_PROVIDER.with(|provider_slot| {
        *provider_slot.borrow_mut() = None;
    });
    set_live_raster_maximum_zoom_level(None);
    bump_raster_tile_provider_registration_revision();
}

/// Reads a required text field from a source declaration object.
fn required_source_text(source: &JsValue, field_name: &str) -> Result<String, JsValue> {
    let field_text = source_text_field(source, field_name)?;
    match field_text {
        Some(text) if !text.trim().is_empty() => Ok(text),
        _ => Err(failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            &format!("the raster tile source {field_name} must be a non-empty string"),
        )),
    }
}

/// Reads an optional text field from a source declaration object.
fn source_text_field(source: &JsValue, field_name: &str) -> Result<Option<String>, JsValue> {
    let field_value =
        js_sys::Reflect::get(source, &JsValue::from_str(field_name)).map_err(|_| {
            failure_with_code(
                ERROR_CODE_INVALID_ARGUMENT,
                &format!("the raster tile source object refuses {field_name}"),
            )
        })?;
    if field_value.is_undefined() || field_value.is_null() {
        return Ok(None);
    }
    field_value.as_string().map(Some).ok_or_else(|| {
        failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            &format!("the raster tile source {field_name} must be a string"),
        )
    })
}

/// Reads an optional numeric field from a source declaration object. An
/// absent (`undefined` or `null`) field is `None`; a present non-number is
/// refused. The provider validates the numeric range, because its refusal
/// names the source the value belongs to.
fn source_number_field(source: &JsValue, field_name: &str) -> Result<Option<f64>, JsValue> {
    let field_value =
        js_sys::Reflect::get(source, &JsValue::from_str(field_name)).map_err(|_| {
            failure_with_code(
                ERROR_CODE_INVALID_ARGUMENT,
                &format!("the raster tile source object refuses {field_name}"),
            )
        })?;
    if field_value.is_undefined() || field_value.is_null() {
        return Ok(None);
    }
    field_value.as_f64().map(Some).ok_or_else(|| {
        failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            &format!("the raster tile source {field_name} must be a number"),
        )
    })
}

/// Reads an optional zoom-level field, defaulting when absent and refusing a
/// value outside the `u8` zoom range.
fn source_zoom_level(
    source: &JsValue,
    field_name: &str,
    default_zoom_level: u8,
) -> Result<u8, JsValue> {
    let field_value =
        js_sys::Reflect::get(source, &JsValue::from_str(field_name)).map_err(|_| {
            failure_with_code(
                ERROR_CODE_INVALID_ARGUMENT,
                &format!("the raster tile source object refuses {field_name}"),
            )
        })?;
    if field_value.is_undefined() || field_value.is_null() {
        return Ok(default_zoom_level);
    }
    let zoom_level = field_value.as_f64().ok_or_else(|| {
        failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            &format!("the raster tile source {field_name} must be a number"),
        )
    })?;
    if !zoom_level.is_finite()
        || zoom_level.fract() != 0.0
        || !(0.0..=f64::from(u8::MAX)).contains(&zoom_level)
    {
        return Err(failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            &format!("the raster tile source {field_name} must be a whole zoom level"),
        ));
    }
    Ok(zoom_level as u8)
}
