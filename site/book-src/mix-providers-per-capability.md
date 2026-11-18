# Mix providers per capability

You can serve each capability of the map from a different provider. A typical
application searches with one vendor, routes with a self-hosted engine and
draws its base map from a public tileset. This page shows how selection
works, what the SDK reports back, and what the reference apps put on screen.

## What you get

- One call per capability, so a choice never disturbs the rest of the map.
- A readback that names the serving provider for every capability.
- A refused choice that leaves the previous provider serving, so a bad
  credential never leaves the app without a provider.

## How selection works

The SDK keeps one slot per capability. A call into an empty slot fails with
`ProviderNotConfigured` naming the capability, never with another provider's
answer. Two calls fill the slots:

| Call | Effect |
|---|---|
| `setProviderCredential(credential)` | Fills every capability the full-service provider serves, in one call |
| `configureProvider(contract, providerKey, credential)` | Fills exactly one capability |

Both are addition-safe: calling either replaces only the slots it names.

## A worked mix

The example is an Android application in Kotlin. It serves geocoding from
MapQuest, draws OpenStreetMap US vector tiles, and routes through a
self-hosted OSRM deployment. Every target exposes these calls; the React
Native package handle spells the whole-set call
`configureProviderSet(providerKey, credential)`, the React Native wrapper `setProviderCredential(providerKey, credential)`.

1. **Set the vendor credential.** This fills every capability the full-service
   provider serves: geocoding, routing, distance matrix, place search, traffic
   incidents, traffic flow and raster tiles.

   ```kotlin
   mapSdk.configureProviderSet("mapquest", mapQuestApiKey)
   ```

2. **Replace routing with the self-hosted engine.** The routing slot now
   serves OSRM; every other capability keeps its provider.

   ```kotlin
   mapSdk.configureProvider(
       contract = ProviderContract.Routing,
       providerKey = "osrm",
       credential = "http://10.0.2.2:5000",
   )
   ```

3. **Install the vector base.** The base map is a surface registration, not a
   registry slot, so it does not disturb any capability.

   ```kotlin
   mapView.setVectorTileSource(openStreetMapUnitedStatesSource)
   ```

4. **Read the result.** `providerRegistrations()` reports one entry per
   capability in a stable order.

```kotlin
mapSdk.providerRegistrations().forEach { registration ->
    println("${registration.contract}: ${registration.displayName.ifEmpty { "not configured" }}")
}
// Geocoding: MapQuest
// Routing: OSRM
// DistanceMatrix: MapQuest
// PlaceSearch: MapQuest
// TrafficIncidents: MapQuest
// TrafficFlow: MapQuest
// Elevation: not configured
// RasterTiles: MapQuest
// VectorTiles: OpenStreetMap US
// OfflineRouting: not configured
```

## What the readback reports

`providerRegistrations()` returns one entry per capability, every time,
including capabilities with no provider. The order is stable:
Geocoding, Routing, Distance Matrix, Place Search, Traffic Incidents, Traffic
Flow, Elevation, Raster Tiles, Vector Tiles, Offline Routing.

| Field | Meaning |
|---|---|
| `contract` | The capability the entry describes |
| `provider_key` | The serving provider's stable key, empty when none is installed |
| `display_name` | The provider's name from the shared registry, empty when none is installed |
| `registered` | Whether a provider is installed for the capability |

Binding surfaces spell the fields in their own style; the Kotlin example
above reads `displayName`. Read the list after a selection and once at
startup. The readback never changes a provider.

## Keyless and keyed providers

A provider's descriptor declares whether it needs a credential. The two paths
differ only in the value passed to `configureProvider`.

**Keyed.** Apply the credential through the settings surface, then select the
provider for a capability:

1. Apply the provider's key. `setProviderCredential(credential)` fills every
   capability the full-service provider serves: geocoding, routing, the
   distance matrix, place search, traffic incidents, traffic flow and raster
   tiles. For any other provider, register its capabilities with
   `configureProvider`, one call per contract its descriptor declares.
2. Replace any capability that should use another provider with
   `configureProvider`. The other capabilities keep serving.
3. A provider that declares a credential refuses a blank one. The refusal
   leaves the previous provider serving and states the reason in the row.

**Keyless.** A self-hosted routing engine or a public instance has no key:

| Provider | Credential value |
|---|---|
| OSRM | The deployment's base URL; an empty value means `http://localhost:5000` |
| Valhalla | The deployment's base URL; an empty value means `http://localhost:8002` |
| Photon | The deployment's base URL; an empty value means the public instance at `https://photon.komoot.io` |

For OSRM and Valhalla, pass the public demo address when you want the shared
server. An empty value points at the local deployment instead. The reference
applications pass the registry descriptor's public address for a keyless
pick, so a selection never silently points at a local deployment.

## Public demo endpoints and their limits

These services are shared, and each one states what it accepts. They are for
evaluation and reference applications, not production traffic.

| Provider | Capabilities | Public endpoint | Limit |
|---|---|---|---|
| OSRM | Routing | `https://router.project-osrm.org` | One request per second; reasonable non-commercial use; no uptime, latency or data-update guarantee |
| Valhalla | Routing | `https://valhalla1.openstreetmap.de` | One request per second; an identifying User-Agent; OpenStreetMap attribution; no high-traffic or production use |
| Photon | Geocoding, place search | `https://photon.komoot.io` | Reasonable non-commercial use; best effort; no uptime or latency guarantee |

The tile services carry their own limits:

| Provider | Capabilities | Endpoint | Limit |
|---|---|---|---|
| OpenStreetMap Standard | Raster tiles | `https://tile.openstreetmap.org/{z}/{x}/{y}.png` | Low-volume use only, with attribution |
| OpenFreeMap | Vector tiles | `https://tiles.openfreemap.org/planet/latest/{z}/{x}/{y}.pbf` | Free public use with attribution; bulk downloads limited to 3,000 tiles or 50 MB per download |
| OpenStreetMap US | Vector tiles | `https://tiles.openstreetmap.us/vector/sourdough/{z}/{x}/{y}.mvt` | Free for low-volume, non-commercial use with attribution |
| MapQuest | Geocoding, routing, distance matrix, place search, traffic, raster tiles | Keyed service | Rate limits apply; raster tiles carry a 30-day caching ceiling |

Run a deployment of your own when your volume exceeds a shared service's
limit. The terms note on each provider's settings row repeats the limit that
applies.

## What the settings surface shows

The settings surface renders one row per capability in the readback's order.
Each row composes the serving provider and its state in one line, such as
`MapQuest · Ready`, `OSRM · Ready` or `MapQuest · Needs API key`. A picker
offers the providers that declare that capability on the platform; a
capability with no provider reads `Not configured`.

The choice is remembered with the app's other remembered choices and
re-applied at startup, after the session's credentials. A refused
re-application reports in its row and leaves the previous provider serving.
Raster tiles and vector tiles are read-only in settings because the map's
layer control owns the base map; their rows point to the control.

## Failure and next step

| Situation | What you see |
|---|---|
| A capability has no provider | `Not configured` on its row; a call fails with `ProviderNotConfigured` naming the capability |
| A provider declares a credential the session lacks | `{display name} · Needs API key`; the picker entry is disabled |
| A selection is refused | The provider's reason in the row; the previous provider keeps serving |
| A keyless routing provider is selected | `Demo server: {url}` under the row, with the descriptor's limit note |
| A public demo routing server does not answer | `The demo server did not answer. Retry, or enter your own deployment address.` |
| A keyless demo has no recorded result | `No demo place matches the {field}; apply a provider key in Settings for live search.` or `No recorded demo route from {origin} to {destination}; apply a provider key for live routing.` |

A failure never swaps in a different provider. The row states the reason and
the previous choice keeps serving until you choose again.

## Rules to keep

- A provider's name belongs in its crate, its sample-data directory, the
  shared registry descriptor and the catalog row. Nothing else names it.
- Sample responses are recorded data served from the demo's own origin;
  tests never reach the network.
- Changing a capability never silently changes another. If a call would
  replace more than the capability it names, it is not this API.

## Next

- [Add a provider](adding-a-provider.md) walks the crate, descriptor and
  catalog row.
- [Search and routing](search-and-routing.md) covers the calls the
  capabilities serve.
- [Map layers and traffic](map-layers-and-traffic.md) covers the on-map
  control that owns the base map and the traffic layer.
- [Provider capabilities in Rust](provider-capabilities-rust.md) lists the
  traits each capability maps to.
