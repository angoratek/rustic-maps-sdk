//! The same-origin recorded tile fixtures: the staging provider, payload
//! decoding and the fixture route fetch.

use super::*;

/// Fetches recorded raster tiles for the scheduler from the same-origin
/// `/tiles` route the local static server exposes.
///
/// `JsFuture` is not `Send` on wasm, so
/// [`RasterTileProvider::fetch_raster_tile`] — whose trait futures must be
/// `Send` — cannot await a browser fetch itself. The render frame instead
/// awaits the fetches directly (its own future is handed to the browser
/// runtime, which needs no `Send`) and
/// [`RecordedTilePayloadProvider::preload_tile_payload`]s the results; the
/// provider then serves them to the scheduler synchronously. A tile that
/// answers non-200 (no recorded fixture at that address), or whose payload
/// does not decode, is marked unavailable and rejected instantly on later
/// requests, so a camera over unrecorded ground never spams the network.
/// One tile payload decoded to plain RGBA8 texels, kept from the staging
/// decode so the upload does not decode the same bytes again. Shared behind an
/// `Arc`: the texels are a megabyte per tile, so a lookup hands out the same
/// buffer rather than copying it.
pub(super) struct DecodedTilePayload {
    /// Texel extent of [`Self::rgba_bytes`].
    pub(super) width_pixels: u32,
    pub(super) height_pixels: u32,
    /// Row-major RGBA8 texels, exactly what `write_texture` consumes.
    pub(super) rgba_bytes: Vec<u8>,
    /// Fingerprint of the encoded bytes this decode came from; a payload
    /// re-fetched with different bytes must be decoded afresh.
    encoded_fingerprint: u64,
}

/// One tile's transient-failure retry schedule.
#[derive(Clone, Copy)]
pub(super) struct TileRetrySchedule {
    /// How many times in a row the tile's fetch has failed.
    consecutive_failure_count: u32,
    /// The wall-clock millisecond before which a retry must not start.
    retry_not_before_milliseconds: f64,
}

/// Why a same-origin tile request produced no bytes.
#[derive(Debug)]
pub(super) enum SameOriginTileFetchError {
    /// The server answered that the address has no recorded tile.
    NotRecorded,
    /// A transport failure or a non-success status other than "not recorded".
    Failed(String),
}

impl std::fmt::Display for SameOriginTileFetchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRecorded => formatter.write_str("the tile is not recorded"),
            Self::Failed(detail) => formatter.write_str(detail),
        }
    }
}

pub(super) struct RecordedTilePayloadProvider {
    /// Tile pixel resolution this canvas fetches every fixture at; the
    /// preload pass maps `tile_key + tile_resolution` onto the recorded
    /// fixture URL (`...@2x.png` markers double-texel fixtures).
    pub(super) tile_resolution: RasterTileResolution,
    /// Payloads fetched during the current render frame and not consumed by
    /// the scheduler's pass-through fetch; each is consumed once.
    pub(super) preloaded_payloads: Mutex<HashMap<TileKey, Vec<u8>>>,
    /// Tiles known to have no usable fixture: unavailable for good, never
    /// retried.
    unavailable_tiles: Mutex<std::collections::HashSet<TileKey>>,
    /// Decoded texels of recently staged payloads, keyed by tile and stamped
    /// with a fingerprint of the encoded bytes they came from. A fetched
    /// payload is decoded once here, at staging time, and the upload consumes
    /// this decode instead of decoding the same bytes a second time on the
    /// frame path.
    decoded_payloads: Mutex<HashMap<TileKey, std::sync::Arc<DecodedTilePayload>>>,
    /// The retry schedule of tiles whose last fetch failed transiently.
    retry_schedules: Mutex<HashMap<TileKey, TileRetrySchedule>>,
}

impl RecordedTilePayloadProvider {
    /// Builds an empty staging provider at `tile_resolution`: no payload
    /// staged, no tile marked unavailable and no retry scheduled.
    pub(super) fn new(tile_resolution: RasterTileResolution) -> Self {
        Self {
            tile_resolution,
            preloaded_payloads: Mutex::new(HashMap::new()),
            unavailable_tiles: Mutex::new(Default::default()),
            decoded_payloads: Mutex::new(HashMap::new()),
            retry_schedules: Mutex::new(HashMap::new()),
        }
    }

    /// Whether the tile is known to have no usable fixture.
    pub(super) fn tile_is_unavailable(&self, tile_key: &TileKey) -> bool {
        self.unavailable_tiles
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(tile_key)
    }

    /// Marks a tile unavailable after a failed request so later frames skip
    /// the retry. A lock poisoned by an impossible (lock-held) panic is
    /// reused rather than propagated.
    pub(super) fn record_unavailable_tile(&self, tile_key: TileKey) {
        let mut unavailable_tiles = self
            .unavailable_tiles
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if unavailable_tiles.len() >= MAXIMUM_UNAVAILABLE_TILE_KEYS {
            unavailable_tiles.clear();
        }
        unavailable_tiles.insert(tile_key);
    }

    /// Whether a transiently failing tile is still inside its retry back-off.
    pub(super) fn tile_fetch_is_deferred(&self, tile_key: &TileKey) -> bool {
        self.retry_schedules
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(tile_key)
            .is_some_and(|retry_schedule| {
                retry_schedule.retry_not_before_milliseconds > js_sys::Date::now()
            })
    }

    /// Schedules the next retry of a transiently failing tile, doubling the
    /// delay per consecutive failure up to the cap.
    pub(super) fn record_transient_tile_failure(&self, tile_key: TileKey) {
        let mut retry_schedules = self
            .retry_schedules
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !retry_schedules.contains_key(&tile_key)
            && retry_schedules.len() >= MAXIMUM_TRACKED_TILE_RETRY_SCHEDULES
        {
            retry_schedules.clear();
        }
        let consecutive_failure_count =
            retry_schedules.get(&tile_key).map_or(1, |retry_schedule| {
                retry_schedule.consecutive_failure_count.saturating_add(1)
            });
        let doubling_exponent =
            i32::try_from(consecutive_failure_count.saturating_sub(1)).unwrap_or(i32::MAX);
        let delay_milliseconds = (TILE_FETCH_RETRY_BASE_DELAY_MILLISECONDS
            * 2f64.powi(doubling_exponent))
        .min(TILE_FETCH_RETRY_MAXIMUM_DELAY_MILLISECONDS);
        retry_schedules.insert(
            tile_key,
            TileRetrySchedule {
                consecutive_failure_count,
                retry_not_before_milliseconds: js_sys::Date::now() + delay_milliseconds,
            },
        );
    }

    /// How many tile addresses currently hold a transient-failure retry
    /// schedule. A frame that listed tiles, drew none and reports a nonzero
    /// count is waiting on the tile source rather than settled.
    pub(super) fn retry_scheduled_tile_count(&self) -> usize {
        self.retry_schedules
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Forgets a tile's transient-failure history after a successful fetch.
    pub(super) fn clear_tile_retry_schedule(&self, tile_key: &TileKey) {
        self.retry_schedules
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(tile_key);
    }

    /// Decodes a fetched payload once (it must decode to RGBA8), stages the
    /// encoded bytes for the scheduler's pass-through fetch and keeps the
    /// decoded texels for the upload, so the frame never decodes the same
    /// payload twice. Decode failures mark the tile unavailable.
    pub(super) fn preload_tile_payload(&self, tile_key: TileKey, encoded_image_bytes: &[u8]) {
        match decode_tile_payload(encoded_image_bytes) {
            Ok(decoded_payload) => {
                let mut decoded_payloads = self
                    .decoded_payloads
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if decoded_payloads.len() >= MAXIMUM_CACHED_DECODED_TILE_PAYLOADS {
                    decoded_payloads.clear();
                }
                decoded_payloads.insert(tile_key, std::sync::Arc::new(decoded_payload));
                drop(decoded_payloads);
                let mut preloaded_payloads = self
                    .preloaded_payloads
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if preloaded_payloads.len() >= MAXIMUM_STAGED_TILE_PAYLOADS {
                    preloaded_payloads.clear();
                }
                preloaded_payloads.insert(tile_key, encoded_image_bytes.to_vec());
            }
            Err(_) => {
                self.record_unavailable_tile(tile_key);
            }
        }
    }

    /// The decoded texels of a staged payload, when the tile is cached and
    /// the bytes still fingerprint to the decode that was made from them (a
    /// re-fetched payload never serves a stale decode). `None` asks the caller
    /// to decode the bytes itself.
    pub(super) fn decoded_tile_payload(
        &self,
        tile_key: &TileKey,
        encoded_image_bytes: &[u8],
    ) -> Option<std::sync::Arc<DecodedTilePayload>> {
        let decoded_payloads = self
            .decoded_payloads
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let decoded_payload = decoded_payloads.get(tile_key)?;
        (decoded_payload.encoded_fingerprint == fingerprint_encoded_bytes(encoded_image_bytes))
            .then(|| std::sync::Arc::clone(decoded_payload))
    }
}

/// Decodes one encoded tile payload to plain RGBA8 texels and stamps the
/// result with a fingerprint of the bytes it came from.
pub(super) fn decode_tile_payload(
    encoded_image_bytes: &[u8],
) -> Result<DecodedTilePayload, String> {
    let rgba_image = normalize_tile_payload_rgba(encoded_image_bytes)?;
    let (width_pixels, height_pixels) = rgba_image.dimensions();
    Ok(DecodedTilePayload {
        width_pixels,
        height_pixels,
        rgba_bytes: rgba_image.into_raw(),
        encoded_fingerprint: fingerprint_encoded_bytes(encoded_image_bytes),
    })
}

/// FNV-1a over the encoded bytes, used only to tell one payload from another
/// (a payload that decodes to the same texels is interchangeable). Cheap
/// relative to a PNG decode, and independent of any hashing crate so the
/// wasm build carries no extra dependency.
pub(super) fn fingerprint_encoded_bytes(encoded_image_bytes: &[u8]) -> u64 {
    let mut fingerprint = FNV_OFFSET_BASIS;
    for byte in encoded_image_bytes {
        fingerprint ^= u64::from(*byte);
        fingerprint = fingerprint.wrapping_mul(FNV_PRIME);
    }
    fingerprint
}

#[async_trait::async_trait]
impl RasterTileProvider for RecordedTilePayloadProvider {
    async fn fetch_raster_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
        _image_format: RasterImageFormat,
        tile_resolution: RasterTileResolution,
    ) -> Result<RasterTileData, SDKError> {
        let tile_key = TileKey::new(tile_x, tile_y, zoom_level)
            .map_err(|sdk_error| SDKError::ProviderRequestRejected(sdk_error.to_string()))?;
        if self.tile_is_unavailable(&tile_key) {
            return Err(SDKError::ProviderRequestRejected(
                "no recorded tile fixture exists at this address".to_string(),
            ));
        }
        let preloaded_payload = self
            .preloaded_payloads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&tile_key)
            .ok_or_else(|| {
                // Not a fixture problem: this render frame simply spent its
                // fetch budget (or the browser fetch is still outstanding),
                // so the scheduler defers the tile and a later frame retries.
                SDKError::NetworkRequestFailed(
                    "the tile was left over for a later frame to retry".to_string(),
                )
            })?;
        Ok(RasterTileData {
            tile_x,
            tile_y,
            zoom_level,
            image_format: RasterImageFormat::Png,
            tile_resolution,
            image_bytes: preloaded_payload,
        })
    }
}

/// Fetches one same-origin URL's body as bytes. The browser only ever
/// contacts the local static server (the surface of the recorded tile
/// fixtures); no external network is fetched.
pub(super) async fn fetch_same_origin_bytes(
    payload_url: &str,
) -> Result<Vec<u8>, SameOriginTileFetchError> {
    let browser_window = web_sys::window().ok_or_else(|| {
        SameOriginTileFetchError::Failed("the browser window is unavailable".to_string())
    })?;
    let fetch_promise = browser_window.fetch_with_str(payload_url);
    let response_value = JsFuture::from(fetch_promise).await.map_err(|error| {
        SameOriginTileFetchError::Failed(format!(
            "the tile request to {payload_url} failed: {error:?}"
        ))
    })?;
    let response: Response = response_value.dyn_into().map_err(|_| {
        SameOriginTileFetchError::Failed("the tile response is not a Response object".to_string())
    })?;
    if !response.ok() {
        let status_code = response.status();
        if status_code == HTTP_NOT_FOUND_STATUS_CODE || status_code == HTTP_GONE_STATUS_CODE {
            return Err(SameOriginTileFetchError::NotRecorded);
        }
        return Err(SameOriginTileFetchError::Failed(format!(
            "the tile request to {payload_url} answered HTTP {status_code}"
        )));
    }
    let array_buffer_promise = response.array_buffer().map_err(|error| {
        SameOriginTileFetchError::Failed(format!("the tile body could not be read: {error:?}"))
    })?;
    let array_buffer = JsFuture::from(array_buffer_promise)
        .await
        .map_err(|error| {
            SameOriginTileFetchError::Failed(format!("the tile body promise failed: {error:?}"))
        })?;
    Ok(Uint8Array::new(&array_buffer).to_vec())
}

/// Normalizes a decoded tile payload to plain RGBA8, mirroring the native
/// headless renderer's normalize stage: no extra copy for the RGBA8 PNGs the
/// fixtures record, conversion for anything else. The decode applies the
/// shared image bounds, so an oversized payload is refused rather than
/// allocated.
pub(super) fn normalize_tile_payload_rgba(
    encoded_image_bytes: &[u8],
) -> Result<image::RgbaImage, String> {
    let decoded_image: DynamicImage =
        rustic_maps_rendering::image_limits::decode_image_with_limits(encoded_image_bytes)
            .map_err(|error| format!("the recorded tile payload failed to decode: {error}"))?;
    Ok(match decoded_image {
        DynamicImage::ImageRgba8(rgba_image) => rgba_image,
        other_decoded_image => other_decoded_image.to_rgba8(),
    })
}
