// An IndexedDB-backed implementation of the wasm canvas' tile payload cache
// seam. The seam lives in `crates/rustic-maps-web-bindings/src/canvas/tile_cache.rs`:
// the page installs a read function and a write function with
// `installTilePayloadCache(read, write)` on the generated canvas module, and
// the fetch path then consults them before any tile request and offers every
// fetched payload back to them. The default seam stores nothing, so a page
// that never installs this module keeps the plain fetch behaviour.
//
// The store is keyed by the provider-neutral address the seam hands over
// (`{zoom}_{x}_{y}` with an `@2x` marker for retina payloads). Database and
// object-store names are private to this module; no tile address ever leaves
// the browser.

/** The IndexedDB database the tile payloads live in. */
const DATABASE_NAME = 'rustic-maps-tile-payload-cache';
/** The object store inside that database. */
const OBJECT_STORE_NAME = 'tile-payloads';
/** The schema version; a future schema change bumps it and handles the upgrade. */
const DATABASE_VERSION = 1;

/**
 * Opens (and, on first use, creates) the cache database.
 * @returns {Promise<IDBDatabase>}
 */
function open_database() {
  return new Promise((resolve, reject) => {
    const open_request = indexedDB.open(DATABASE_NAME, DATABASE_VERSION);
    open_request.onupgradeneeded = () => {
      const database = open_request.result;
      if (!database.objectStoreNames.contains(OBJECT_STORE_NAME)) {
        database.createObjectStore(OBJECT_STORE_NAME);
      }
    };
    open_request.onsuccess = () => resolve(open_request.result);
    open_request.onerror = () => reject(open_request.error);
  });
}

/**
 * Runs one request inside a transaction and resolves with its result.
 * @param {IDBDatabase} database
 * @param {IDBTransactionMode} mode
 * @param {(store: IDBObjectStore) => IDBRequest} issue_request
 * @returns {Promise<unknown>}
 */
function run_request(database, mode, issue_request) {
  return new Promise((resolve, reject) => {
    const transaction = database.transaction(OBJECT_STORE_NAME, mode);
    const request = issue_request(transaction.objectStore(OBJECT_STORE_NAME));
    let result;
    request.onsuccess = () => {
      result = request.result;
    };
    request.onerror = () => reject(request.error);
    // Resolve on the transaction's own completion, so a persisted write is
    // durable before the caller proceeds.
    transaction.oncomplete = () => resolve(result);
    transaction.onerror = () => reject(transaction.error);
    transaction.onabort = () => reject(transaction.error);
  });
}

/**
 * Creates an IndexedDB-backed tile payload cache for the canvas seam.
 *
 * The returned `read` and `write` functions have the shape
 * `installTilePayloadCache` expects; `count` and `clear` are for the page and
 * its automated suite.
 *
 * @returns {{
 *   read: (storage_key: string) => Promise<Uint8Array | null>,
 *   write: (storage_key: string, payload: Uint8Array) => Promise<void>,
 *   count: () => Promise<number>,
 *   clear: () => Promise<void>,
 * }}
 */
export function createIndexedDbTilePayloadCache() {
  const database_promise = open_database();
  return {
    async read(storage_key) {
      const database = await database_promise;
      const payload = await run_request(
        database,
        'readonly',
        (store) => store.get(storage_key),
      );
      if (payload === undefined || payload === null) {
        return null;
      }
      return payload instanceof Uint8Array ? payload : new Uint8Array(payload);
    },
    async write(storage_key, payload) {
      const database = await database_promise;
      await run_request(
        database,
        'readwrite',
        (store) => store.put(payload, storage_key),
      );
    },
    async count() {
      const database = await database_promise;
      return run_request(database, 'readonly', (store) => store.count());
    },
    async clear() {
      const database = await database_promise;
      await run_request(
        database,
        'readwrite',
        (store) => store.clear(),
      );
    },
  };
}
