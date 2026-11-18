# FAQ and troubleshooting

These are the problems that come up most often when you integrate the SDK,
with the check that resolves each. [Performance and profiling](performance.md)
carries tuning guidance; this page covers failures.

## WebGPU and browser support

**The browser surface needs WebGPU; there is no WebGL fallback.** The wasm
canvas fails its `initialize()` when no WebGPU adapter supports the canvas or
the device request fails, and reports the error. Catch it and present a
degradation message: name the missing capability (`navigator.gpu` is the
feature test) and link MDN's compatibility table. The version floor is
Chromium 113 flag-free, Safari 26, and Firefox 141 on desktop, always in a
secure context (HTTPS or localhost). Do not sniff the user agent.

**The map stays empty on an Android emulator after rendering starts.** Android
emulators render through SwiftShader over ANGLE, a software graphics stack, so
this is expected emulator behavior. The GPU surface negotiates through the GL
backend: set `WGPU_BACKEND=gl` in the environment. On real hardware the setting
is harmless.

## API key and provider failures

**How do I supply the provider key?** Read it at runtime: from your app's own
secure configuration or a settings screen, and pass it to the provider
registration call. Never commit it or bake it into an app bundle. The shipped
apps start keyless, take the key from their settings screen, and optionally
remember it in the device's secure store. Automated tests inject a key through
a launch value.

**The SDK throws immediately at registration when the key is refused.**
`setProviderCredential` returns the provider's detail. Surface it; do not
catch and continue.

**Calls throw `RateLimitExhausted`.** The shared token-bucket limiter is
exhausted. The bucket refills on its own schedule, so retry after the next
refill rather than surfacing an error. Cache-then-network behavior around every
provider contract keeps repeated identical requests free of quota.

**Calls throw `ProviderNotConfigured` naming a contract.** That contract has no
registered implementation on the `MapSdk` object. Registration is per object:
re-registration replaces every slot, so a second `setProviderCredential` call
is safe, but a second `MapSdk()` needs its own registration.

**Elevation throws `ProviderNotConfigured` even with the provider set
installed.** The shipped providers include no elevation implementation, so
elevation calls throw `ProviderNotConfigured`.

## Offline behavior

**Tiles render but routes fail offline, or the other way around.** The two
offline layers are separate. Tile imagery comes from the disk cache you
registered with `setProviderCredentialWithTileCache`, within its byte budget
and time-to-live. Routing comes from a compiled routing archive, independent
of the imagery cache. Check that the archive covers the queried region.

**A route refusal comes back offline.** The offline graph refuses rather than
guessing in three cases: bytes that fail archive validation, a coordinate
outside every node's snap tolerance, and a request pair the graph cannot
connect. Check that the archive covers the areas you query; a small extract is
the usual cause.

**Maneuver text has no street names offline.** The archived graph carries no
street-name strings. Offline maneuver text names the turn direction and the
distance and states what it does not know. A named maneuver comes from the
online provider.

## Tile cache

**A tuned time-to-live does not stick after re-registration.** Registration
resets the cache to the category default. Re-apply the override after
registering. The `rasterTileCacheTerms` readback shows the effective value.

**A large requested time-to-live is accepted but the cache still expires
entries.** The installed provider's declared ceiling, 30 calendar days for the
imagery, clamps what the cache retains. Read the ceiling from
`rasterTileCacheTerms` and display it next to the effective value.

**Setting a zero time-to-live throws.** A zero value would expire every entry
on capture. The refusal names the mistake.

## Compliance

**Attribution.** Keep the attribution line visible in your application:
imagery © MapQuest; map data © OpenStreetMap contributors (ODbL). If you
redistribute the SDK or ship an application built on it, reproduce the
[third-party notices](third-party-notices.md) with your distribution.
