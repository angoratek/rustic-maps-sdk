//! The tile payload cache seam: the one boundary between the wasm tile fetch
//! path and an optional persistent store.
//!
//! The canvas fetches every missing tile through
//! [`super::tile_fetch::fetch_tile_payload_bytes`]. That path consults this
//! seam before any same-origin fixture probe or provider request and offers
//! every fetched payload back to it. The default implementation stores
//! nothing, so a page that installs no cache keeps the fetch behaviour
//! exactly; a page may install an IndexedDB-backed implementation through
//! [`install_tile_payload_cache`] and remove it through
//! [`clear_tile_payload_cache`].
//!
//! The seam is provider-neutral: an address is the XYZ tile address plus the
//! tile resolution, and the installed implementation owns any namespacing
//! beyond that address.

use super::*;
use async_trait::async_trait;

/// The storage key's retina marker, the same `@2x` marker the recorded
/// fixture route uses, so one address names one payload per resolution.
const RETINA_TILE_CACHE_KEY_MARKER: &str = "@2x";

/// One tile address a cached payload is stored under: the XYZ address and
/// the pixel resolution the payload was fetched at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct TilePayloadCacheAddress {
    /// The tile's XYZ address.
    pub(super) tile_key: TileKey,
    /// The pixel resolution the payload was fetched at.
    pub(super) tile_resolution: RasterTileResolution,
}

impl TilePayloadCacheAddress {
    /// The provider-neutral storage key handed to an installed cache:
    /// `{zoom}_{x}_{y}` with the same `@2x` marker the fixture route uses.
    fn storage_key(&self) -> String {
        let resolution_marker = match self.tile_resolution {
            RasterTileResolution::Standard => "",
            RasterTileResolution::Retina => RETINA_TILE_CACHE_KEY_MARKER,
        };
        format!(
            "{}_{}_{}{resolution_marker}",
            self.tile_key.zoom_level, self.tile_key.tile_x, self.tile_key.tile_y
        )
    }
}

/// The cache seam the tile fetch path consults.
///
/// The trait's futures stay on the canvas thread (`?Send`): the browser
/// implementation awaits JavaScript promises, which are not `Send`, and the
/// fetch itself runs in a `spawn_local` task.
#[async_trait(?Send)]
pub(super) trait TilePayloadCache {
    /// The cached encoded payload at `address`, or `None` when the address
    /// is not cached.
    async fn cached_payload(&self, address: &TilePayloadCacheAddress) -> Option<Vec<u8>>;

    /// Offers one successfully fetched encoded payload for storage.
    async fn store_payload(&self, address: &TilePayloadCacheAddress, payload: &[u8]);
}

/// The default cache: stores nothing, so the fetch path behaves exactly as
/// it did before a page installed a cache.
pub(super) struct UncachedTilePayloads;

#[async_trait(?Send)]
impl TilePayloadCache for UncachedTilePayloads {
    async fn cached_payload(&self, _address: &TilePayloadCacheAddress) -> Option<Vec<u8>> {
        None
    }

    async fn store_payload(&self, _address: &TilePayloadCacheAddress, _payload: &[u8]) {}
}

/// A cache backed by two page-supplied JavaScript functions: `read` resolves
/// the stored payload (`Uint8Array`, `null` when absent) for a storage key,
/// and `write` persists one. An IndexedDB store is the intended backing, but
/// the seam accepts any pair with that shape.
pub(super) struct JavaScriptTilePayloadCache {
    /// The page function that resolves a cached payload.
    read: js_sys::Function,
    /// The page function that persists a payload.
    write: js_sys::Function,
}

impl JavaScriptTilePayloadCache {
    /// Builds a cache over the page's read and write functions.
    fn new(read: js_sys::Function, write: js_sys::Function) -> Self {
        Self { read, write }
    }
}

#[async_trait(?Send)]
impl TilePayloadCache for JavaScriptTilePayloadCache {
    async fn cached_payload(&self, address: &TilePayloadCacheAddress) -> Option<Vec<u8>> {
        let storage_key = JsValue::from_str(&address.storage_key());
        let promise_value = self.read.call1(&JsValue::NULL, &storage_key).ok()?;
        let promise = promise_value.dyn_into::<js_sys::Promise>().ok()?;
        let result = JsFuture::from(promise).await.ok()?;
        if result.is_null() || result.is_undefined() {
            return None;
        }
        if let Ok(payload) = result.clone().dyn_into::<Uint8Array>() {
            return Some(payload.to_vec());
        }
        result
            .dyn_into::<js_sys::ArrayBuffer>()
            .ok()
            .map(|buffer| Uint8Array::new(&buffer).to_vec())
    }

    async fn store_payload(&self, address: &TilePayloadCacheAddress, payload: &[u8]) {
        let storage_key = JsValue::from_str(&address.storage_key());
        let payload_value: JsValue = Uint8Array::from(payload).into();
        let Some(promise_value) = self
            .write
            .call2(&JsValue::NULL, &storage_key, &payload_value)
            .ok()
        else {
            return;
        };
        if let Ok(promise) = promise_value.dyn_into::<js_sys::Promise>() {
            let _ = JsFuture::from(promise).await;
        }
    }
}

thread_local! {
    /// The cache the fetch path consults; no cache installed by default.
    static TILE_PAYLOAD_CACHE: RefCell<Rc<dyn TilePayloadCache>> =
        RefCell::new(Rc::new(UncachedTilePayloads));
}

/// The cache the fetch path currently consults.
pub(super) fn current_tile_payload_cache() -> Rc<dyn TilePayloadCache> {
    TILE_PAYLOAD_CACHE.with(|cache| cache.borrow().clone())
}

/// Installs a page-backed payload cache. `read` receives a storage key and
/// resolves the stored payload (`Uint8Array`, or `null` when absent); `write`
/// receives a storage key and an encoded payload and resolves when it is
/// stored. This is the persistent-cache seam; the default stores nothing.
#[wasm_bindgen(js_name = installTilePayloadCache)]
pub fn install_tile_payload_cache(read: js_sys::Function, write: js_sys::Function) {
    TILE_PAYLOAD_CACHE.with(|cache| {
        *cache.borrow_mut() = Rc::new(JavaScriptTilePayloadCache::new(read, write));
    });
}

/// Removes the installed payload cache, returning the fetch path to its
/// store-nothing default.
#[wasm_bindgen(js_name = clearTilePayloadCache)]
pub fn clear_tile_payload_cache() {
    TILE_PAYLOAD_CACHE.with(|cache| {
        *cache.borrow_mut() = Rc::new(UncachedTilePayloads);
    });
}
