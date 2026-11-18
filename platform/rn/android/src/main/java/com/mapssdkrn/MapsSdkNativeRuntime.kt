package com.mapssdkrn

import android.app.Activity
import android.content.Context
import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.WritableMap
import com.rusticmaps.sdk.DeviceLocationSource
import uniffi.rustic_maps.BoundingBox
import uniffi.rustic_maps.DistanceMatrix
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.MapSdk
import uniffi.rustic_maps.OfflineManager
import uniffi.rustic_maps.Place
import uniffi.rustic_maps.PositionController
import uniffi.rustic_maps.PositionFix
import uniffi.rustic_maps.PositionSourceError
import uniffi.rustic_maps.ProviderContract
import uniffi.rustic_maps.ProviderFailureDiagnostic
import uniffi.rustic_maps.ProviderRegistration
import uniffi.rustic_maps.RegionDefinition
import uniffi.rustic_maps.RegionDownloadProgress
import uniffi.rustic_maps.RegionDownloadProgressListener
import uniffi.rustic_maps.RegionRecord
import uniffi.rustic_maps.RegionRemovalReport
import uniffi.rustic_maps.Route
import uniffi.rustic_maps.RouteCapabilities
import uniffi.rustic_maps.RouteOptions
import uniffi.rustic_maps.TileCacheStatistics
import uniffi.rustic_maps.TileCacheTerms
import uniffi.rustic_maps.TrafficFlowCacheStatistics
import uniffi.rustic_maps.TrafficFlowData
import uniffi.rustic_maps.TrafficIncident
import uniffi.rustic_maps.TransportationMode
import uniffi.rustic_maps.UrlTemplateRasterTileSource
import uniffi.rustic_maps.getSdkCompiledVersion
import java.io.File
import java.lang.ref.WeakReference

/**
 * The one native `MapSdk` the React Native package owns.
 *
 * The generated JSI bindings construct their own native SDK, and a foreign
 * handle cannot be imported into a native view; a second `MapSdk` in the same
 * process would open a second provider registry and a second SQLite tile
 * cache. The package therefore creates the SDK here once, every `<MapView>`
 * draws from it, and the package module proxies the contract calls through it
 * (one SDK object, owned by the package).
 *
 * Every SDK failure travels verbatim: this object never swallows an exception
 * and never invents a success.
 */
object MapsSdkNativeRuntime {

    /** The provider key the registration dispatch serves today. */
    const val MAPQUEST_PROVIDER_KEY: String = "mapquest"

    /** The launch-intent extra that supplies a runtime credential. */
    private const val RUNTIME_CREDENTIAL_INTENT_EXTRA: String = "providerApiKey"

    /** The optional launch-intent extra naming the credential's provider. */
    private const val RUNTIME_PROVIDER_KEY_INTENT_EXTRA: String = "providerKey"

    /**
     * The assets file a build made for the hosted demo carries: `{ "provider_key": …, "api_key": … }`, written on
     * the build machine from a dedicated, separately issued credential. It is
     * never committed, and it is absent from every other build — so a
     * distributed APK stays keyless.
     */
    private const val HOSTED_DEMO_CREDENTIAL_ASSET: String = "hosted-demo-credential.json"

    /** The provider key field of the hosted-demo credential file. */
    private const val HOSTED_DEMO_CREDENTIAL_PROVIDER_KEY_FIELD: String = "provider_key"

    /** The credential field of the hosted-demo credential file. */
    private const val HOSTED_DEMO_CREDENTIAL_API_KEY_FIELD: String = "api_key"

    /** The tile cache's file inside the host app's caches directory. */
    private const val TILE_CACHE_DATABASE_FILE_NAME: String = "rustic-maps-tile-cache.sqlite"

    /** The tile cache's named byte budget (the demo's 16 MiB budget). */
    private const val TILE_CACHE_ABSOLUTE_MAXIMUM_BYTES: ULong = 16_777_216UL

    /** The empty-credential refusal (the credential is never logged). */
    private const val PROVIDER_CREDENTIAL_EMPTY_TEXT: String = "the provider credential is empty"

    /** The unsupported-provider refusal template. */
    private const val PROVIDER_CREDENTIAL_UNSUPPORTED_TEMPLATE: String = "provider '%s' is not supported"

    /** The not-started refusal. */
    private const val PROVIDER_CREDENTIAL_SDK_NOT_STARTED_TEXT: String = "the SDK has not started"

    /** The refusal for a provider without a credential-based base map. */
    private const val NO_CREDENTIALED_BASE_MAP_TEMPLATE: String =
        "provider '%s' has no credential-based base map on this platform"

    /** The refusal when no credential is remembered for the restore. */
    private const val NO_APPLIED_CREDENTIAL_TEXT: String =
        "no provider credential is applied; set one in Settings before selecting its base map"

    /** The missing-cache-directory refusal. */
    private const val PROVIDER_CREDENTIAL_CACHE_DIRECTORY_UNAVAILABLE_TEXT: String =
        "the app cache directory is unavailable"

    /** The stable refusal template for a time-to-live above the terms ceiling. */
    private const val TILE_CACHE_TIME_TO_LIVE_ABOVE_CEILING_TEMPLATE: String =
        "the tile-cache time-to-live exceeds the provider's terms ceiling of %d seconds"

    /** The native SDK, or null before the first successful start. */
    @Volatile
    private var nativeSdk: MapSdk? = null

    /** The caches directory the tile-cache registration opens under. */
    @Volatile
    private var cacheDirectoryPath: String? = null

    /** The installed cache's state line (read by the diagnostics sheet). */
    @Volatile
    var tileCacheStateText: String = "Cache not registered"
        private set

    /** Whether a runtime credential registered (the diagnostics line reads it). */
    @Volatile
    var isProviderCredentialApplied: Boolean = false
        private set

    /**
     * The applied credential, retained in native memory only so a base-map
     * switch can re-register its raster slot (the credential never crosses to
     * JavaScript). Mirror of the Compose facade's `appliedMapQuestCredential`.
     */
    @Volatile
    private var appliedProviderCredential: String? = null

    /** The contract's one position controller every host source feeds. */
    @Volatile
    private var positionController: PositionController? = null

    /** The listening host source, present between a successful start and a stop. */
    @Volatile
    private var deviceLocationSource: DeviceLocationSource? = null

    /** Weak references to every live host view (SDK start re-attaches them). */
    private val hostViewReferences: MutableList<WeakReference<MapsSdkMapHostView>> =
        mutableListOf()

    /** The SDK, or null before `ensureStarted` has run. */
    val mapSdk: MapSdk?
        get() = nativeSdk

    /** The compiled SDK version (safe before the SDK object is constructed). */
    val compiledVersion: String
        get() = getSdkCompiledVersion()

    /**
     * Builds the SDK keyless on first call; later calls are a no-op. A
     * construction failure returns its throwable so the module reports it
     * verbatim through the promise.
     */
    fun ensureStarted(context: Context): Result<Unit> {
        nativeSdk?.let { return Result.success(Unit) }
        return try {
            val constructedSdk = MapSdk()
            nativeSdk = constructedSdk
            cacheDirectoryPath = context.applicationContext.cacheDir.absolutePath
            attachSdkToLiveHostViews()
            Result.success(Unit)
        } catch (startupFailure: Exception) {
            Result.failure(startupFailure)
        }
    }

    /**
     * Applies the launch-intent automation credential seam: a launch extra
     * supplies a runtime credential for local/CI UI automation; an optional
     * second extra names its provider, defaulting to the provider the
     * registration dispatch serves. The injection goes through the public
     * registration path, is never persisted by the injection and never logged.
     */
    @Volatile
    private var launchCredentialApplied: Boolean = false

    fun applyLaunchInjectedCredential(activity: Activity?) {
        if (launchCredentialApplied) {
            return
        }
        val launchIntent = activity?.intent
        val hostedDemoCredential = hostedDemoCredential(activity)
        val intentCredential = launchIntent
            ?.getStringExtra(RUNTIME_CREDENTIAL_INTENT_EXTRA)
            ?.takeIf { credential -> credential.isNotBlank() }
        val runtimeCredential = intentCredential ?: hostedDemoCredential?.second ?: return
        val intentProviderKey = if (intentCredential == null) {
            null
        } else {
            launchIntent
                ?.getStringExtra(RUNTIME_PROVIDER_KEY_INTENT_EXTRA)
                ?.takeIf { providerKey -> providerKey.isNotBlank() }
        }
        val runtimeProviderKey = intentProviderKey
            ?: hostedDemoCredential?.first
            ?: MAPQUEST_PROVIDER_KEY
        val (wasApplied, _) =
            setProviderCredential(runtimeProviderKey, runtimeCredential, withTileCache = true)
        if (wasApplied) {
            launchCredentialApplied = true
        }
    }

    /**
     * Reads the hosted demo's credential file from the build's assets, when
     * it carries one. Returns null for every other build, and for a malformed
     * file — an unreadable injected credential leaves the app keyless rather
     * than failing.
     */
    private fun hostedDemoCredential(activity: Activity?): Pair<String, String>? {
        val context: Context = activity?.applicationContext ?: return null
        return try {
            val credentialText = context.assets
                .open(HOSTED_DEMO_CREDENTIAL_ASSET)
                .bufferedReader()
                .use { reader -> reader.readText() }
            val credentialJson = org.json.JSONObject(credentialText)
            val providerKey = credentialJson
                .optString(HOSTED_DEMO_CREDENTIAL_PROVIDER_KEY_FIELD)
                .takeIf { field -> field.isNotBlank() }
            val credential = credentialJson
                .optString(HOSTED_DEMO_CREDENTIAL_API_KEY_FIELD)
                .takeIf { field -> field.isNotBlank() }
            if (providerKey != null && credential != null) providerKey to credential else null
        } catch (missingOrMalformedCredential: Exception) {
            null
        }
    }

    /** Registers one live host view so a later SDK start attaches to it. */
    fun registerHostView(hostView: MapsSdkMapHostView) {
        hostViewReferences.removeAll { reference -> reference.get() == null }
        if (hostViewReferences.none { reference -> reference.get() === hostView }) {
            hostViewReferences.add(WeakReference(hostView))
        }
        hostView.attachPackageSdk(nativeSdk)
    }

    /** Drops one host view from the live registry. */
    fun unregisterHostView(hostView: MapsSdkMapHostView) {
        hostViewReferences.removeAll { reference ->
            reference.get() == null || reference.get() === hostView
        }
    }

    private fun attachSdkToLiveHostViews() {
        val constructedSdk = nativeSdk ?: return
        hostViewReferences.removeAll { reference -> reference.get() == null }
        hostViewReferences.forEach { reference -> reference.get()?.attachPackageSdk(constructedSdk) }
    }

    /**
     * Registers one runtime provider credential, optionally wrapping the
     * raster-tile contract in the disk response cache. The provider dispatch
     * is one arm per provider: adding a provider adds an arm, never a view
     * rewrite.
     */
    fun setProviderCredential(
        providerKey: String,
        credential: String,
        withTileCache: Boolean,
    ): Pair<Boolean, String> {
        val sdk = nativeSdk ?: return Pair(false, PROVIDER_CREDENTIAL_SDK_NOT_STARTED_TEXT)
        if (credential.isBlank()) {
            return Pair(false, PROVIDER_CREDENTIAL_EMPTY_TEXT)
        }
        if (!withTileCache) {
            return try {
                sdk.configureProviderSet(providerKey, credential)
                appliedProviderCredential = credential
                isProviderCredentialApplied = true
                tileCacheStateText = "Cache not registered"
                Pair(true, "")
            } catch (credentialFailure: Exception) {
                Pair(false, credentialFailure.message ?: credentialFailure.toString())
            }
        }
        val cacheDirectory = cacheDirectoryPath
            ?: return Pair(false, PROVIDER_CREDENTIAL_CACHE_DIRECTORY_UNAVAILABLE_TEXT)
        // `java.io.File` builds the path on every supported API level, while
        // the NIO path API arrived only in API 26: the package supports the
        // AAR's API 24 floor, where that API is unavailable.
        val cacheDatabaseFilePath = File(
            cacheDirectory,
            TILE_CACHE_DATABASE_FILE_NAME,
        ).absolutePath
        return try {
            sdk.setProviderCredentialWithTileCache(
                providerKey,
                credential,
                cacheDatabaseFilePath,
                TILE_CACHE_ABSOLUTE_MAXIMUM_BYTES,
            )
            appliedProviderCredential = credential
            isProviderCredentialApplied = true
            tileCacheStateText = "Cache registered: $cacheDatabaseFilePath"
            Pair(true, "")
        } catch (credentialFailure: Exception) {
            Pair(false, credentialFailure.message ?: credentialFailure.toString())
        }
    }

    /**
     * Chooses the provider that serves one capability. The SDK validates the
     * key and the credential before the slot moves, so a refusal throws the
     * SDK's own reason and leaves the previously serving provider in place.
     */
    fun configureProvider(
        contract: ProviderContract,
        providerKey: String,
        credential: String,
    ) {
        val sdk = nativeSdk ?: throw SdkNotStartedException()
        sdk.configureProvider(contract, providerKey, credential)
    }

    /** Reports which provider serves each capability, in the SDK's order. */
    fun providerRegistrations(): List<ProviderRegistration> {
        val sdk = nativeSdk ?: throw SdkNotStartedException()
        return sdk.providerRegistrations()
    }

    /** The serving routing provider's supported modes and strategy. */
    fun routeCapabilities(): RouteCapabilities {
        val sdk = nativeSdk ?: throw SdkNotStartedException()
        return sdk.routeCapabilities()
    }

    /**
     * Registers one provider-neutral base map (M9's native base-map switch).
     * The SDK validates the whole declaration before the raster-tile slot
     * moves, so a refusal surfaces verbatim and the previously serving source
     * stays installed.
     */
    fun registerUrlTemplateRasterTileSource(source: UrlTemplateRasterTileSource) {
        val sdk = nativeSdk ?: throw SdkNotStartedException()
        sdk.registerUrlTemplateRasterTileSource(source)
    }

    /**
     * Re-registers the applied credential so its raster-tile slot serves the
     * base map again — the switch back from a credential-less source. The
     * re-registration repeats the credential the running session already
     * holds (native memory only) and installs the tile cache with it, exactly
     * as the original registration did; a credential-less switch had dropped
     * both. Refuses honestly when no credential has been applied.
     */
    fun restoreCredentialedBaseMap(providerKey: String): Pair<Boolean, String> {
        if (providerKey != MAPQUEST_PROVIDER_KEY) {
            return Pair(false, NO_CREDENTIALED_BASE_MAP_TEMPLATE.format(providerKey))
        }
        val credential = appliedProviderCredential
            ?: return Pair(false, NO_APPLIED_CREDENTIAL_TEXT)
        return setProviderCredential(providerKey, credential, withTileCache = true)
    }

    /** Full provider search results for a settled query. */
    suspend fun search(queryText: String, searchBoundary: BoundingBox?): List<Place> {
        val sdk = nativeSdk ?: throw SdkNotStartedException()
        return sdk.search(queryText, searchBoundary)
    }

    /** Fast completion candidates while the user types. */
    suspend fun suggest(queryText: String, searchBoundary: BoundingBox?): List<Place> {
        val sdk = nativeSdk ?: throw SdkNotStartedException()
        return sdk.suggest(queryText, searchBoundary)
    }

    /** Resolves a free-form address text into candidate places. */
    suspend fun forwardGeocode(addressText: String): List<Place> {
        val sdk = nativeSdk ?: throw SdkNotStartedException()
        return sdk.forwardGeocode(addressText)
    }

    /** Calculates a route through the ordered waypoints with the given options. */
    suspend fun calculateRoute(
        waypoints: List<GeographicCoordinate>,
        routeOptions: RouteOptions,
    ): Route {
        val sdk = nativeSdk ?: throw SdkNotStartedException()
        return sdk.calculateRoute(waypoints, routeOptions)
    }

    /** Resolves a coordinate into candidate places describing that position. */
    suspend fun reverseGeocode(
        coordinate: uniffi.rustic_maps.GeographicCoordinate,
    ): List<Place> {
        val sdk = nativeSdk ?: throw SdkNotStartedException()
        return sdk.reverseGeocode(coordinate)
    }

    /** Calculates the travel-time matrix for every origin-destination pair. */
    suspend fun calculateDistanceMatrix(
        origins: List<GeographicCoordinate>,
        destinations: List<GeographicCoordinate>,
        transportationMode: TransportationMode,
    ): DistanceMatrix {
        val sdk = nativeSdk ?: throw SdkNotStartedException()
        return sdk.calculateDistanceMatrix(origins, destinations, transportationMode)
    }

    /** Fetches traffic incidents intersecting the bounding box. */
    suspend fun fetchTrafficIncidents(boundingBox: BoundingBox): List<TrafficIncident> {
        val sdk = nativeSdk ?: throw SdkNotStartedException()
        return sdk.fetchTrafficIncidents(boundingBox)
    }

    /** Fetches per-segment traffic flow inside the bounding box. */
    suspend fun fetchTrafficFlow(boundingBox: BoundingBox): List<TrafficFlowData> {
        val sdk = nativeSdk ?: throw SdkNotStartedException()
        return sdk.fetchTrafficFlow(boundingBox)
    }

    /** The installed raster-tile cache's effective terms. */
    fun tileCacheTerms(): TileCacheTerms {
        val sdk = nativeSdk ?: throw SdkNotStartedException()
        return sdk.rasterTileCacheTerms()
    }

    /**
     * The serving traffic-flow cache's counters, or null when the serving
     * provider reports no cache (a foreign provider serves uncached).
     */
    fun trafficFlowCacheStatistics(): TrafficFlowCacheStatistics? {
        val sdk = nativeSdk ?: throw SdkNotStartedException()
        return sdk.trafficFlowCacheStatistics()
    }

    /**
     * The installed tile payload cache's exact counters, or null when no tile
     * cache is installed (a keyless session).
     */
    fun tileCacheStatistics(): TileCacheStatistics? {
        val sdk = nativeSdk ?: throw SdkNotStartedException()
        return sdk.tileCacheStatistics()
    }

    /**
     * Overrides the installed tile cache's time-to-live, in seconds.
     *
     * Refuses a non-positive value and a value above the installed
     * provider's declared terms ceiling with [InvalidTimeToLiveException],
     * so an invalid request never reaches the contract's `u64` conversion.
     * The ceiling readback requires an installed tile cache, exactly like
     * the override itself; a keyless session still surfaces the SDK's
     * not-configured failure.
     */
    fun setTileCacheTimeToLive(timeToLiveSeconds: Long) {
        if (timeToLiveSeconds <= 0L) {
            throw InvalidTimeToLiveException()
        }
        val sdk = nativeSdk ?: throw SdkNotStartedException()
        val providerTermsCeilingSeconds =
            sdk.rasterTileCacheTerms().providerTermsCeilingSeconds
        if (providerTermsCeilingSeconds != null &&
            timeToLiveSeconds.toULong() > providerTermsCeilingSeconds
        ) {
            throw InvalidTimeToLiveException(
                TILE_CACHE_TIME_TO_LIVE_ABOVE_CEILING_TEMPLATE.format(
                    providerTermsCeilingSeconds.toLong(),
                ),
            )
        }
        sdk.setRasterTileCacheTimeToLive(timeToLiveSeconds.toULong())
    }

    // ------------------------------------------------------------------
    // Offline regions: the pack manager over the installed cache
    // ------------------------------------------------------------------

    /** The offline-region facade over the one package SDK. */
    private fun offlineManager(): OfflineManager {
        val sdk = nativeSdk ?: throw SdkNotStartedException()
        return OfflineManager(sdk)
    }

    /** How many tiles the region enumerates (nothing is written). */
    fun regionTileCount(region: RegionDefinition): ULong =
        offlineManager().regionTileCount(region)

    /**
     * Downloads the region's tiles, forwarding each progress report to the
     * package's event channel. The generated listener is called on an SDK
     * worker thread; the channel's emitter hops to the UI thread itself.
     */
    suspend fun downloadRegion(region: RegionDefinition): RegionRecord {
        val progressListener = object : RegionDownloadProgressListener {
            override fun onRegionDownloadProgress(progress: RegionDownloadProgress) {
                MapsSdkNativeEventChannel.emitOfflineRegionProgress(
                    regionProgressPayload(progress),
                )
            }
        }
        return offlineManager().downloadRegion(region, progressListener)
    }

    /** Signals the region's in-flight download; idempotent. */
    fun cancelRegionDownload(regionId: String) {
        offlineManager().cancelRegionDownload(regionId)
    }

    /** Every region stored on disk, in the store's listing order. */
    suspend fun regionRecords(): List<RegionRecord> = offlineManager().regionRecords()

    /** One stored region's record, or null when none is stored under the id. */
    suspend fun regionRecord(regionId: String): RegionRecord? =
        offlineManager().regionRecord(regionId)

    /** Deletes the region's stored tiles and membership. */
    suspend fun deleteRegion(regionId: String): RegionRemovalReport =
        offlineManager().deleteRegion(regionId)

    /** Encodes one progress report for the JavaScript event channel. */
    private fun regionProgressPayload(progress: RegionDownloadProgress): WritableMap {
        val payload = Arguments.createMap()
        payload.putString("regionId", progress.regionId)
        payload.putDouble(
            "totalTileCount",
            progress.totalTileCount.toLong().toDouble(),
        )
        payload.putDouble(
            "completedTileCount",
            progress.completedTileCount.toLong().toDouble(),
        )
        payload.putDouble(
            "networkFetchedTileCount",
            progress.networkFetchedTileCount.toLong().toDouble(),
        )
        payload.putDouble(
            "diskServedTileCount",
            progress.diskServedTileCount.toLong().toDouble(),
        )
        payload.putInt("phase", progress.phase.ordinal)
        return payload
    }

    /** The retained provider-failure diagnostics, oldest first. */
    fun latestProviderFailureDiagnostics(): List<ProviderFailureDiagnostic> {
        val sdk = nativeSdk ?: throw SdkNotStartedException()
        return sdk.latestProviderFailureDiagnostics()
    }

    // ------------------------------------------------------------------
    // Device positioning
    // ------------------------------------------------------------------

    /** The controller, built empty on first use. */
    private fun ensurePositionController(): PositionController =
        positionController ?: PositionController().also { controller ->
            positionController = controller
        }

    /**
     * Starts the host's device-location source, creating it over the
     * controller on first use. Answers false when the source cannot start —
     * a missing permission, a disabled location service or no enabled
     * provider — and the source records the matching failure in the
     * controller. The permission prompt stays the application's job.
     */
    fun startDeviceLocation(context: Context): Boolean {
        val source = deviceLocationSource ?: DeviceLocationSource(
            context.applicationContext,
            ensurePositionController(),
        ).also { createdSource ->
            deviceLocationSource = createdSource
        }
        return source.start()
    }

    /** Stops and clears the host source; true when one was running. */
    fun stopDeviceLocation(): Boolean {
        val source = deviceLocationSource ?: return false
        source.stop()
        deviceLocationSource = null
        return true
    }

    /** The latest accepted fix, or null before the first one. */
    fun latestDeviceFix(): PositionFix? = positionController?.latestFix()

    /** The smoothed course across the fixes that reported one, or null. */
    fun deviceLocationSmoothedCourseDegrees(): Double? =
        positionController?.smoothedCourseDegrees()

    /** The last source failure, or null when the last event was a fix. */
    fun deviceLocationSourceStatus(): PositionSourceError? =
        positionController?.sourceStatus()

    /** Read-only diagnostics lines for the package's diagnostics surface. */
    fun diagnosticsLines(): List<String> {
        val lines = mutableListOf<String>()
        lines.add("SDK version: $compiledVersion")
        lines.add(
            if (isProviderCredentialApplied) {
                "Provider credential: applied"
            } else {
                "Provider credential: not configured (keyless)"
            },
        )
        lines.add(tileCacheStateText)
        nativeSdk?.let { sdk ->
            runCatching { lines.add("Tile cache terms: ${sdk.rasterTileCacheTerms()}") }
        }
        return lines
    }

    /** Thrown when a caller reaches the SDK before `initialize`. */
    class SdkNotStartedException : IllegalStateException(PROVIDER_CREDENTIAL_SDK_NOT_STARTED_TEXT)
}

/**
 * Thrown when a requested tile-cache time-to-live is invalid: not a positive
 * whole number of seconds, or above the provider's terms ceiling. Carries the
 * stable refusal message the native module rejects with.
 */
class InvalidTimeToLiveException(
    message: String = TILE_CACHE_TIME_TO_LIVE_INVALID_TEXT,
) : IllegalArgumentException(message)

/** The stable refusal for a time-to-live that is not a positive whole number. */
private const val TILE_CACHE_TIME_TO_LIVE_INVALID_TEXT: String =
    "the tile-cache time-to-live must be a finite, positive whole number of seconds"
