import CoreLocation
import Foundation

/// The package's one native `MapSdk` on iOS.
///
/// The generated JSI bindings construct their own native SDK, and a foreign
/// handle cannot be imported into a native view; a second `MapSdk` in the same
/// process would open a second provider registry and a second SQLite tile
/// cache. The package therefore creates the SDK here once, every `<MapView>`
/// draws from it, and the package module proxies the contract calls through it, as one SDK object owned by the package.
@objc(MapsSdkNativeRuntime)
public final class MapsSdkNativeRuntime: NSObject {

    /// The plugin's one runtime instance.
    @objc public static let shared = MapsSdkNativeRuntime()

    /// The provider key the registration dispatch serves today.
    @objc public static let mapquestProviderKey = "mapquest"

    /// The tile cache's file inside the host app's caches directory.
    private static let tileCacheDatabaseFileName = "rustic-maps-tile-cache.sqlite"

    /// The tile cache's named byte budget (the demo's 16 MiB budget).
    private static let tileCacheAbsoluteMaximumBytes: UInt64 = 16_777_216

    /// The empty-credential refusal (the credential is never logged).
    private static let emptyCredentialText = "the provider credential is empty"

    /// The unsupported-provider refusal template.
    private static let unsupportedProviderTemplate = "provider '%@' is not supported"

    /// The not-started refusal.
    private static let sdkNotStartedText = "the SDK has not started"

    /// The missing-cache-directory refusal.
    private static let cacheDirectoryUnavailableText =
        "the app cache directory is unavailable"

    /// The refusal for a provider without a credential-based base map.
    private static let noCredentialedBaseMapTemplate =
        "provider '%@' has no credential-based base map on this platform"

    /// The refusal when no credential is remembered for the restore.
    private static let noAppliedCredentialText =
        "no provider credential is applied; set one in Settings before selecting its base map"

    /// The native SDK, or nil before the first successful start.
    public private(set) var mapSdk: MapSdk?

    /// The contract's one position controller every host source feeds.
    private lazy var positionController = PositionController()

    /// The listening host source, present between a successful start and a stop.
    private var deviceLocationSource: MapsSdkDeviceLocationSource?

    private var cacheDirectoryURL: URL?
    /// Whether a runtime credential registered (the diagnostics line reads it).
    @objc public private(set) var isProviderCredentialApplied = false

    /// The applied credential, retained in native memory only so a base-map
    /// switch can re-register its raster slot (it never crosses to
    /// JavaScript). Mirror of the Compose facade's `appliedMapQuestCredential`.
    private var appliedProviderCredential: String?

    /// Whether the launch-environment credential seam already ran. The seam
    /// is applied once per process; a repeated `initialize()` must neither
    /// re-register the credential nor reinstall its tile cache.
    private var launchInjectedCredentialApplied = false

    /// The installed cache's state line (read by the diagnostics sheet).
    @objc public private(set) var tileCacheStateText = "Cache not registered"

    private override init() {
        super.init()
    }

    /// Builds the SDK keyless on first call; later calls are a no-op.
    @objc public func ensureStarted() throws {
        guard mapSdk == nil else {
            applyLaunchInjectedCredentialFromEnvironment()
            return
        }
        let constructedSdk = MapSdk()
        mapSdk = constructedSdk
        MapsSdkMapHostView.attachSdkToLiveHostViews(constructedSdk)
        cacheDirectoryURL = FileManager.default.urls(
            for: .cachesDirectory,
            in: .userDomainMask
        ).first
        applyLaunchInjectedCredentialFromEnvironment()
    }

    /// The automation credential seam: launch-environment variables supply a
    /// runtime credential and an optional provider key for local/CI UI
    /// automation. The injection goes through the public registration path,
    /// is never persisted by the injection and never logged.
    private func applyLaunchInjectedCredentialFromEnvironment() {
        guard !launchInjectedCredentialApplied else {
            return
        }
        let environment = ProcessInfo.processInfo.environment
        guard let runtimeCredential = environment["PROVIDER_API_KEY"],
              !runtimeCredential.isEmpty
        else {
            return
        }
        let runtimeProviderKey = environment["PROVIDER_KEY"].flatMap { providerKey in
            providerKey.isEmpty ? nil : providerKey
        } ?? Self.mapquestProviderKey
        let applicationResult = setProviderCredential(
            providerKey: runtimeProviderKey,
            credential: runtimeCredential,
            withTileCache: true
        )
        launchInjectedCredentialApplied = applicationResult["isApplied"] as? Bool ?? false
    }

    /// Registers one runtime provider credential, optionally with the disk
    /// tile cache. The provider dispatch is one arm per provider.
    @objc public func setProviderCredential(
        providerKey: String,
        credential: String,
        withTileCache: Bool
    ) -> [String: Any] {
        guard let sdk = mapSdk else {
            return credentialResult(isApplied: false, detailText: Self.sdkNotStartedText)
        }
        guard !credential.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return credentialResult(isApplied: false, detailText: Self.emptyCredentialText)
        }
        if !withTileCache {
            do {
                try sdk.configureProviderSet(providerKey: providerKey, credential: credential)
                appliedProviderCredential = credential
                isProviderCredentialApplied = true
                tileCacheStateText = "Cache not registered"
                return credentialResult(isApplied: true, detailText: "")
            } catch {
                return credentialResult(
                    isApplied: false,
                    detailText: Self.failureText(error)
                )
            }
        }
        guard let cacheDirectoryURL else {
            return credentialResult(
                isApplied: false,
                detailText: Self.cacheDirectoryUnavailableText
            )
        }
        let cacheDatabasePath = cacheDirectoryURL
            .appendingPathComponent(Self.tileCacheDatabaseFileName)
            .path
        do {
            try sdk.setProviderCredentialWithTileCache(
                providerKey: providerKey,
                credential: credential,
                tileCacheDatabasePath: cacheDatabasePath,
                tileCacheAbsoluteMaximumBytes: Self.tileCacheAbsoluteMaximumBytes
            )
            appliedProviderCredential = credential
            isProviderCredentialApplied = true
            tileCacheStateText = "Cache registered: \(cacheDatabasePath)"
            return credentialResult(isApplied: true, detailText: "")
        } catch {
            return credentialResult(
                isApplied: false,
                detailText: Self.failureText(error)
            )
        }
    }

    /// The compiled SDK version.
    @objc public func compiledVersion() -> String {
        getSdkCompiledVersion()
    }

    /// Chooses the provider that serves one capability. The SDK validates the
    /// key and the credential before the slot moves, so a refusal throws the
    /// SDK's own reason and leaves the previously serving provider in place.
    @objc public func configureProvider(
        contractOrdinal: Int,
        providerKey: String,
        credential: String
    ) throws {
        guard let sdk = mapSdk else { throw MapsSdkNativeRuntimeError.sdkNotStarted }
        guard let contract = MapsSdkNativeJson.providerContract(
            fromOrdinal: contractOrdinal
        ) else {
            throw MapsSdkNativeRuntimeError.invalidProviderContract
        }
        try sdk.configureProvider(
            contract: contract,
            providerKey: providerKey,
            credential: credential
        )
    }

    /// Reports which provider serves each capability, as JSON.
    @objc public func providerRegistrationsJSON() throws -> String {
        guard let sdk = mapSdk else { throw MapsSdkNativeRuntimeError.sdkNotStarted }
        return MapsSdkNativeJson.providerRegistrationsJSON(
            sdk.providerRegistrations()
        )
    }

    /// Reports the serving routing provider's capabilities, as JSON.
    @objc public func routeCapabilitiesJSON() throws -> String {
        guard let sdk = mapSdk else { throw MapsSdkNativeRuntimeError.sdkNotStarted }
        return MapsSdkNativeJson.routeCapabilitiesJSON(try sdk.routeCapabilities())
    }

    /// Registers one provider-neutral base map (M9's native base-map switch).
    /// The SDK validates the whole declaration before the raster-tile slot
    /// moves, so a refusal throws verbatim and the previously serving source
    /// stays installed.
    @objc public func registerUrlTemplateRasterTileSourceJSON(
        _ sourceJSON: String
    ) throws {
        guard let sdk = mapSdk else {
            throw MapsSdkNativeRuntimeError.sdkNotStarted
        }
        guard
            let sourceDictionary = MapsSdkNativeJson.dictionary(from: sourceJSON),
            let source = MapsSdkNativeJson.urlTemplateRasterTileSource(
                from: sourceDictionary
            )
        else {
            throw MapsSdkNativeRuntimeError.invalidTileSourceDeclaration
        }
        try sdk.registerUrlTemplateRasterTileSource(source: source)
    }

    /// Re-registers the applied credential so its raster-tile slot serves the
    /// base map again — the switch back from a credential-less source. The
    /// re-registration repeats the credential the running session already
    /// holds (native memory only) and installs the tile cache with it, exactly
    /// as the original registration did; a credential-less switch had dropped
    /// both. Refuses honestly when no credential has been applied.
    @objc public func restoreCredentialedBaseMap(
        providerKey: String
    ) -> [String: Any] {
        guard providerKey == Self.mapquestProviderKey else {
            return credentialResult(
                isApplied: false,
                detailText: String(
                    format: Self.noCredentialedBaseMapTemplate,
                    providerKey
                )
            )
        }
        guard let credential = appliedProviderCredential else {
            return credentialResult(
                isApplied: false,
                detailText: Self.noAppliedCredentialText
            )
        }
        return setProviderCredential(
            providerKey: providerKey,
            credential: credential,
            withTileCache: true
        )
    }

    /// The provider key a launch-injected credential targets by default.
    @objc public func defaultProviderKey() -> String {
        Self.mapquestProviderKey
    }

    /// Read-only diagnostics lines for the package's diagnostics surface.
    @objc public func diagnosticsLines() -> [String] {
        var lines: [String] = []
        lines.append("SDK version: \(compiledVersion())")
        lines.append(
            isProviderCredentialApplied
                ? "Provider credential: applied"
                : "Provider credential: not configured (keyless)"
        )
        lines.append(tileCacheStateText)
        if let sdk = mapSdk, let terms = try? sdk.rasterTileCacheTerms() {
            lines.append(
                "Tile cache terms: effective \(terms.effectiveTimeToLiveSeconds)s, default \(terms.defaultTimeToLiveSeconds)s"
            )
        }
        return lines
    }

    /// The installed raster-tile cache's effective terms as JSON.
    @objc public func tileCacheTermsJSON() throws -> String {
        guard let sdk = mapSdk else { throw MapsSdkNativeRuntimeError.sdkNotStarted }
        return MapsSdkNativeJson.tileCacheTermsJSON(try sdk.rasterTileCacheTerms())
    }

    /// The serving traffic-flow cache's counters as JSON, or `null` when the
    /// serving provider reports no cache (a foreign provider serves uncached).
    @objc public func trafficFlowCacheStatisticsJSON() throws -> String {
        guard let sdk = mapSdk else { throw MapsSdkNativeRuntimeError.sdkNotStarted }
        guard let statistics = sdk.trafficFlowCacheStatistics() else {
            return "null"
        }
        return MapsSdkNativeJson.trafficFlowCacheStatisticsJSON(statistics)
    }

    /// The installed tile payload cache's exact counters as JSON, or `null`
    /// when no tile cache is installed (a keyless session).
    @objc public func tileCacheStatisticsJSON() throws -> String {
        guard let sdk = mapSdk else { throw MapsSdkNativeRuntimeError.sdkNotStarted }
        guard let statistics = sdk.tileCacheStatistics() else {
            return "null"
        }
        return MapsSdkNativeJson.tileCacheStatisticsJSON(statistics)
    }

    /// Overrides the installed tile cache's time-to-live, in seconds.
    ///
    /// Refuses a non-positive value and a value above the installed
    /// provider's declared terms ceiling. `UInt64(exactly:)` is the
    /// non-trapping conversion: it answers nil for NaN, infinities,
    /// fractions, negatives and values beyond the `u64` range, where the
    /// plain initializer would trap. The ceiling readback requires an
    /// installed tile cache, exactly like the override itself; a keyless
    /// session still surfaces the SDK's not-configured failure.
    @objc public func setTileCacheTimeToLive(_ timeToLiveSeconds: Double) throws {
        guard let wholeSeconds = UInt64(exactly: timeToLiveSeconds),
              wholeSeconds > 0
        else {
            throw MapsSdkNativeRuntimeError.invalidTimeToLive
        }
        guard let sdk = mapSdk else { throw MapsSdkNativeRuntimeError.sdkNotStarted }
        let providerTermsCeilingSeconds =
            try sdk.rasterTileCacheTerms().providerTermsCeilingSeconds
        if let providerTermsCeilingSeconds,
           wholeSeconds > providerTermsCeilingSeconds {
            throw MapsSdkNativeRuntimeError.timeToLiveAboveTermsCeiling(
                ceilingSeconds: providerTermsCeilingSeconds
            )
        }
        try sdk.setRasterTileCacheTimeToLive(timeToLiveSeconds: wholeSeconds)
    }

    // ------------------------------------------------------------------
    // Offline regions: the pack manager over the installed cache
    // ------------------------------------------------------------------

    /// How many tiles the region enumerates (nothing is written).
    @objc public func regionTileCount(_ regionJson: String) throws -> NSNumber {
        guard let sdk = mapSdk else {
            throw MapsSdkNativeRuntimeError.sdkNotStarted
        }
        guard let region = regionDefinition(from: regionJson) else {
            throw MapsSdkNativeRuntimeError.invalidRegionDefinition
        }
        let count = try OfflineManager(mapSdk: sdk).regionTileCount(region: region)
        return NSNumber(value: count)
    }

    /// Downloads one region, forwarding each progress report through the
    /// package's event channel, and answers the stored record as JSON.
    @objc public func downloadRegionJSON(
        _ regionJson: String,
        completion: @escaping (String?, Error?) -> Void
    ) {
        guard let sdk = mapSdk else {
            completion(nil, MapsSdkNativeRuntimeError.sdkNotStarted)
            return
        }
        guard let region = regionDefinition(from: regionJson) else {
            completion(nil, MapsSdkNativeRuntimeError.invalidRegionDefinition)
            return
        }
        let progressListener = MapsSdkRegionProgressListener { progress in
            MapsSdkNativeEventChannel.emitOfflineRegionProgress(
                Self.regionProgressPayload(progress)
            )
        }
        Task { @MainActor in
            do {
                let record = try await OfflineManager(mapSdk: sdk)
                    .downloadRegion(
                        region: region,
                        progressListener: progressListener
                    )
                completion(MapsSdkNativeJson.regionRecordJSON(record), nil)
            } catch {
                completion(nil, error)
            }
        }
    }

    /// Cancels the region's in-flight download; idempotent.
    @objc public func cancelRegionDownload(_ regionId: String) throws {
        guard let sdk = mapSdk else {
            throw MapsSdkNativeRuntimeError.sdkNotStarted
        }
        OfflineManager(mapSdk: sdk).cancelRegionDownload(regionId: regionId)
    }

    /// Every region stored on disk, as JSON.
    @objc public func regionRecordsJSON(
        completion: @escaping (String?, Error?) -> Void
    ) {
        guard let sdk = mapSdk else {
            completion(nil, MapsSdkNativeRuntimeError.sdkNotStarted)
            return
        }
        Task { @MainActor in
            do {
                let records = try await OfflineManager(mapSdk: sdk).regionRecords()
                completion(MapsSdkNativeJson.regionRecordsJSON(records), nil)
            } catch {
                completion(nil, error)
            }
        }
    }

    /// One stored region's record as JSON, or nil when none is stored.
    @objc public func regionRecordJSON(
        _ regionId: String,
        completion: @escaping (String?, Error?) -> Void
    ) {
        guard let sdk = mapSdk else {
            completion(nil, MapsSdkNativeRuntimeError.sdkNotStarted)
            return
        }
        Task { @MainActor in
            do {
                let record = try await OfflineManager(mapSdk: sdk)
                    .regionRecord(regionId: regionId)
                guard let record else {
                    completion(nil, nil)
                    return
                }
                completion(MapsSdkNativeJson.regionRecordJSON(record), nil)
            } catch {
                completion(nil, error)
            }
        }
    }

    /// Deletes one stored region and answers what was removed, as JSON.
    @objc public func deleteRegionJSON(
        _ regionId: String,
        completion: @escaping (String?, Error?) -> Void
    ) {
        guard let sdk = mapSdk else {
            completion(nil, MapsSdkNativeRuntimeError.sdkNotStarted)
            return
        }
        Task { @MainActor in
            do {
                let report = try await OfflineManager(mapSdk: sdk)
                    .deleteRegion(regionId: regionId)
                completion(MapsSdkNativeJson.regionRemovalReportJSON(report), nil)
            } catch {
                completion(nil, error)
            }
        }
    }

    /// Decodes one region definition from the camel-case JSON wire shape.
    private func regionDefinition(from regionJson: String) -> RegionDefinition? {
        guard let dictionary = MapsSdkNativeJson.dictionary(from: regionJson) else {
            return nil
        }
        return MapsSdkNativeJson.regionDefinition(from: dictionary)
    }

    /// Encodes one progress report for the JavaScript event channel.
    private static func regionProgressPayload(
        _ progress: RegionDownloadProgress
    ) -> [String: Any] {
        [
            "regionId": progress.regionId,
            "totalTileCount": progress.totalTileCount,
            "completedTileCount": progress.completedTileCount,
            "networkFetchedTileCount": progress.networkFetchedTileCount,
            "diskServedTileCount": progress.diskServedTileCount,
            "phase": regionDownloadPhaseOrdinal(progress.phase),
        ]
    }

    /// The generated phase enum's zero-based JavaScript enum ordinal.
    private static func regionDownloadPhaseOrdinal(
        _ phase: RegionDownloadPhase
    ) -> Int {
        switch phase {
        case .downloading: return 0
        case .completed: return 1
        case .cancelled: return 2
        case .failed: return 3
        }
    }

    /// The retained provider-failure diagnostics as JSON.
    @objc public func latestProviderFailureDiagnosticsJSON() throws -> String {
        guard let sdk = mapSdk else { throw MapsSdkNativeRuntimeError.sdkNotStarted }
        return MapsSdkNativeJson.providerFailureDiagnosticsJSON(
            sdk.latestProviderFailureDiagnostics()
        )
    }

    /// Full provider search results for a settled query, as JSON.
    @objc public func searchJSON(
        queryText: String,
        boundaryJSON: String?,
        completion: @escaping (String?, Error?) -> Void
    ) {
        runPlaceSearch(
            boundaryJSON: boundaryJSON,
            completion: completion
        ) { sdk, boundary in
            try await sdk.search(queryText: queryText, searchBoundary: boundary)
        }
    }

    /// Fast completion candidates while the user types, as JSON.
    @objc public func suggestJSON(
        queryText: String,
        boundaryJSON: String?,
        completion: @escaping (String?, Error?) -> Void
    ) {
        runPlaceSearch(
            boundaryJSON: boundaryJSON,
            completion: completion
        ) { sdk, boundary in
            try await sdk.suggest(queryText: queryText, searchBoundary: boundary)
        }
    }

    /// Resolves a free-form address text into candidate places, as JSON.
    @objc public func forwardGeocodeJSON(
        addressText: String,
        completion: @escaping (String?, Error?) -> Void
    ) {
        guard let sdk = mapSdk else {
            completion(nil, MapsSdkNativeRuntimeError.sdkNotStarted)
            return
        }
        Task { @MainActor in
            do {
                let places = try await sdk.forwardGeocode(addressText: addressText)
                completion(MapsSdkNativeJson.placesJSON(places), nil)
            } catch {
                completion(nil, error)
            }
        }
    }

    /// Calculates a route through the ordered waypoints, as JSON.
    @objc public func calculateRouteJSON(
        waypointsJson: String,
        routeOptionsJson: String,
        completion: @escaping (String?, Error?) -> Void
    ) {
        guard let sdk = mapSdk else {
            completion(nil, MapsSdkNativeRuntimeError.sdkNotStarted)
            return
        }
        guard
            let waypointDictionaries = MapsSdkNativeJson.dictionaryArray(
                from: waypointsJson
            ),
            let routeOptionsDictionary = MapsSdkNativeJson.dictionary(
                from: routeOptionsJson
            ),
            let routeOptions = MapsSdkNativeJson.routeOptions(
                from: routeOptionsDictionary
            )
        else {
            completion(nil, MapsSdkNativeRuntimeError.invalidRouteRequest)
            return
        }
        let waypoints = MapsSdkNativeJson.coordinates(
            from: waypointDictionaries
        )
        Task { @MainActor in
            do {
                let route = try await sdk.calculateRoute(
                    waypoints: waypoints,
                    routeOptions: routeOptions
                )
                completion(MapsSdkNativeJson.routeJSON(route), nil)
            } catch {
                completion(nil, error)
            }
        }
    }

    /// Resolves a coordinate into candidate places, as JSON.
    @objc public func reverseGeocodeJSON(
        coordinateJSON: String,
        completion: @escaping (String?, Error?) -> Void
    ) {
        guard
            let sdk = mapSdk,
            let coordinateDictionary = MapsSdkNativeJson.dictionary(from: coordinateJSON),
            let latitude = coordinateDictionary["latitude"] as? NSNumber,
            let longitude = coordinateDictionary["longitude"] as? NSNumber
        else {
            completion(nil, MapsSdkNativeRuntimeError.sdkNotStarted)
            return
        }
        let coordinate = GeographicCoordinate(
            latitude: latitude.doubleValue,
            longitude: longitude.doubleValue
        )
        Task { @MainActor in
            do {
                let places = try await sdk.reverseGeocode(coordinate: coordinate)
                completion(MapsSdkNativeJson.placesJSON(places), nil)
            } catch {
                completion(nil, error)
            }
        }
    }

    /// Calculates the contract's distance matrix, as JSON.
    @objc public func calculateDistanceMatrixJSON(
        originsJson: String,
        destinationsJson: String,
        transportationModeOrdinal: Int,
        completion: @escaping (String?, Error?) -> Void
    ) {
        guard
            let sdk = mapSdk,
            let originDictionaries = MapsSdkNativeJson.dictionaryArray(from: originsJson),
            let destinationDictionaries = MapsSdkNativeJson.dictionaryArray(
                from: destinationsJson
            ),
            let transportationMode = MapsSdkNativeJson.transportationMode(
                fromOrdinal: transportationModeOrdinal
            )
        else {
            completion(nil, MapsSdkNativeRuntimeError.invalidRouteRequest)
            return
        }
        let origins = MapsSdkNativeJson.coordinates(from: originDictionaries)
        let destinations = MapsSdkNativeJson.coordinates(from: destinationDictionaries)
        Task { @MainActor in
            do {
                let distanceMatrix = try await sdk.calculateDistanceMatrix(
                    origins: origins,
                    destinations: destinations,
                    transportationMode: transportationMode
                )
                completion(MapsSdkNativeJson.distanceMatrixJSON(distanceMatrix), nil)
            } catch {
                completion(nil, error)
            }
        }
    }

    /// Fetches traffic incidents intersecting a bounding box, as JSON.
    @objc public func fetchTrafficIncidentsJSON(
        boundingBoxJson: String,
        completion: @escaping (String?, Error?) -> Void
    ) {
        guard
            let sdk = mapSdk,
            let boundingBoxDictionary = MapsSdkNativeJson.dictionary(from: boundingBoxJson),
            let boundingBox = MapsSdkNativeJson.boundingBox(from: boundingBoxDictionary)
        else {
            completion(nil, MapsSdkNativeRuntimeError.invalidBoundingBoxRequest)
            return
        }
        Task { @MainActor in
            do {
                let incidents = try await sdk.fetchTrafficIncidents(boundingBox: boundingBox)
                completion(MapsSdkNativeJson.trafficIncidentsJSON(incidents), nil)
            } catch {
                completion(nil, error)
            }
        }
    }

    /// Fetches per-segment traffic flow inside a bounding box, as JSON.
    @objc public func fetchTrafficFlowJSON(
        boundingBoxJson: String,
        completion: @escaping (String?, Error?) -> Void
    ) {
        guard
            let sdk = mapSdk,
            let boundingBoxDictionary = MapsSdkNativeJson.dictionary(from: boundingBoxJson),
            let boundingBox = MapsSdkNativeJson.boundingBox(from: boundingBoxDictionary)
        else {
            completion(nil, MapsSdkNativeRuntimeError.invalidBoundingBoxRequest)
            return
        }
        Task { @MainActor in
            do {
                let flowObservations = try await sdk.fetchTrafficFlow(boundingBox: boundingBox)
                completion(MapsSdkNativeJson.trafficFlowJSON(flowObservations), nil)
            } catch {
                completion(nil, error)
            }
        }
    }

    private func runPlaceSearch(
        boundaryJSON: String?,
        completion: @escaping (String?, Error?) -> Void,
        search: @escaping (MapSdk, BoundingBox?) async throws -> [Place]
    ) {
        guard let sdk = mapSdk else {
            completion(nil, MapsSdkNativeRuntimeError.sdkNotStarted)
            return
        }
        let boundary = currentSearchBoundary(from: boundaryJSON)
        Task { @MainActor in
            do {
                let places = try await search(sdk, boundary)
                completion(MapsSdkNativeJson.placesJSON(places), nil)
            } catch {
                completion(nil, error)
            }
        }
    }

    private func currentSearchBoundary(from boundaryJSON: String?) -> BoundingBox? {
        guard
            let boundaryJSON,
            let dictionary = MapsSdkNativeJson.dictionary(from: boundaryJSON)
        else {
            return nil
        }
        return MapsSdkNativeJson.boundingBox(from: dictionary)
    }

    // ------------------------------------------------------------------
    // Device positioning
    // ------------------------------------------------------------------

    /// Starts the host's device-location source, creating it over the
    /// controller on first use. Answers false when the source cannot start —
    /// an undetermined or denied authorization, or disabled location
    /// services — and the source records the matching failure in the
    /// controller. The application owns the authorization prompt.
    @objc public func startDeviceLocation() -> Bool {
        if let source = deviceLocationSource {
            return source.start()
        }
        let source = MapsSdkDeviceLocationSource(
            positionController: positionController
        )
        deviceLocationSource = source
        return source.start()
    }

    /// Stops and clears the host source; true when one was running.
    @objc public func stopDeviceLocation() -> Bool {
        guard let source = deviceLocationSource else {
            return false
        }
        source.stop()
        deviceLocationSource = nil
        return true
    }

    /// The controller's latest accepted fix as JSON, or nil before the first.
    @objc public func latestDeviceFixJSON() -> String? {
        guard let fix = positionController.latestFix() else {
            return nil
        }
        let fixDictionary: [String: Any] = [
            "coordinate": [
                "latitude": fix.coordinate.latitude,
                "longitude": fix.coordinate.longitude,
            ],
            "horizontalAccuracyMeters":
                fix.horizontalAccuracyMeters as Any? ?? NSNull(),
            "courseHeadingDegrees": fix.courseHeadingDegrees as Any? ?? NSNull(),
            "timestampSeconds": fix.timestampSeconds as Any? ?? NSNull(),
        ]
        guard
            let fixData = try? JSONSerialization.data(withJSONObject: fixDictionary),
            let encodedFix = String(data: fixData, encoding: .utf8)
        else {
            return nil
        }
        return encodedFix
    }

    /// The smoothed course in degrees clockwise from true north, or nil.
    @objc public func deviceLocationSmoothedCourseDegrees() -> NSNumber? {
        guard let courseDegrees = positionController.smoothedCourseDegrees() else {
            return nil
        }
        return NSNumber(value: courseDegrees)
    }

    /// The last device-location source failure, or nil once a fix arrives.
    @objc public func deviceLocationSourceStatusName() -> String? {
        guard let status = positionController.sourceStatus() else {
            return nil
        }
        return Self.positionSourceErrorName(status)
    }

    /// The lower-camel name the typed JavaScript surface declares.
    private static func positionSourceErrorName(
        _ error: PositionSourceError
    ) -> String {
        switch error {
        case .permissionDenied:
            return "permissionDenied"
        case .serviceDisabled:
            return "serviceDisabled"
        case .unavailable:
            return "unavailable"
        case .timeout:
            return "timeout"
        }
    }

    private func credentialResult(
        isApplied: Bool,
        detailText: String
    ) -> [String: Any] {
        ["isApplied": isApplied, "detailText": detailText]
    }

    private static func failureText(_ error: Error) -> String {
        (error as NSError).localizedDescription
    }
}

/// Bridges the contract's region-progress listener to a plain closure.
///
/// The generated contract calls the listener on an SDK worker thread, so the
/// closure must hand work to the emitter rather than touching UI state.
private final class MapsSdkRegionProgressListener:
    RegionDownloadProgressListener, @unchecked Sendable {
    private let onProgress: (RegionDownloadProgress) -> Void

    init(onProgress: @escaping (RegionDownloadProgress) -> Void) {
        self.onProgress = onProgress
    }

    func onRegionDownloadProgress(progress: RegionDownloadProgress) {
        onProgress(progress)
    }
}

/// Pod-local mirror of the shipped location source.
///
/// The pod compiles the contract's generated bindings into its own module and
/// cannot import the SwiftPM `MapNativeCoreUI` product — the same reason
/// `MapsSdkRusticMapView` is a pod copy of the shipped map view. The two
/// copies are kept in sync by the iOS packaging slice.
///
/// The application owns the authorization prompt (its Info.plist declares
/// `NSLocationWhenInUseUsageDescription`) and calls `start()` after the user
/// grants it. The source reports every fix to the controller with the
/// platform's accuracy, course and timestamp; authorization and service
/// failures are reported to the controller, so the controller's state and
/// the interface agree.
private final class MapsSdkDeviceLocationSource: NSObject, CLLocationManagerDelegate {
    /// The controller every fix and failure is reported to.
    private let positionController: PositionController

    /// The platform's location manager.
    private let locationManager = CLLocationManager()

    /// Whether the source is currently updating.
    private(set) var isActive = false

    /// Builds the source over a controller, without starting it.
    init(positionController: PositionController) {
        self.positionController = positionController
        super.init()
        locationManager.delegate = self
        locationManager.desiredAccuracy = kCLLocationAccuracyBest
        locationManager.distanceFilter = Self.minimumDistanceMeters
    }

    /// Starts location updates. Answers false when the permission is
    /// undetermined (the prompt was requested and the caller should retry
    /// after it resolves), denied or restricted, or when location services
    /// are disabled; the controller records the failure.
    @discardableResult
    func start() -> Bool {
        if isActive {
            return true
        }
        switch locationManager.authorizationStatus {
        case .notDetermined:
            locationManager.requestWhenInUseAuthorization()
            return false
        case .denied, .restricted:
            positionController.sourceFailed(error: .permissionDenied)
            return false
        case .authorizedAlways, .authorizedWhenInUse:
            break
        @unknown default:
            positionController.sourceFailed(error: .unavailable)
            return false
        }
        guard CLLocationManager.locationServicesEnabled() else {
            positionController.sourceFailed(error: .serviceDisabled)
            return false
        }
        locationManager.startUpdatingLocation()
        isActive = true
        return true
    }

    /// Stops location updates; a later `start()` resumes the same controller.
    func stop() {
        locationManager.stopUpdatingLocation()
        isActive = false
    }

    /// Reports one platform fix to the controller.
    func locationManager(
        _ manager: CLLocationManager,
        didUpdateLocations locations: [CLLocation]
    ) {
        guard let location = locations.last else { return }
        let positionFix = PositionFix(
            coordinate: GeographicCoordinate(
                latitude: location.coordinate.latitude,
                longitude: location.coordinate.longitude
            ),
            horizontalAccuracyMeters: location.horizontalAccuracy >= 0
                ? location.horizontalAccuracy : nil,
            courseHeadingDegrees: location.course >= 0 ? location.course : nil,
            timestampSeconds: location.timestamp.timeIntervalSince1970
        )
        do {
            try positionController.feedFix(fix: positionFix)
        } catch {
            // A refused fix leaves the controller's previous state in force;
            // the next accepted fix continues the same stream.
        }
    }

    /// Reports a source failure to the controller.
    func locationManager(
        _ manager: CLLocationManager,
        didFailWithError error: Error
    ) {
        positionController.sourceFailed(error: Self.sourceError(for: error))
    }

    /// Maps a Core Location failure onto the contract's source status.
    private static func sourceError(for error: Error) -> PositionSourceError {
        guard let coreLocationError = error as? CLError else {
            return .unavailable
        }
        switch coreLocationError.code {
        case .denied:
            return .permissionDenied
        default:
            return .unavailable
        }
    }

    /// The shortest movement between delivered fixes, in meters.
    private static let minimumDistanceMeters: CLLocationDistance = 1
}

/// A package-runtime failure that is not a typed SDK error.
public enum MapsSdkNativeRuntimeError: LocalizedError {
    case sdkNotStarted
    case invalidTileSourceDeclaration
    case invalidRouteRequest
    case invalidBoundingBoxRequest
    case invalidProviderContract
    case invalidRegionDefinition
    case invalidTimeToLive
    case timeToLiveAboveTermsCeiling(ceilingSeconds: UInt64)

    public var errorDescription: String? {
        switch self {
        case .sdkNotStarted:
            return "the SDK has not started"
        case .invalidTileSourceDeclaration:
            return "the base-map tile source declaration is malformed"
        case .invalidRouteRequest:
            return "the route request is malformed"
        case .invalidBoundingBoxRequest:
            return "the bounding box request is malformed"
        case .invalidProviderContract:
            return "the provider contract is not recognised"
        case .invalidRegionDefinition:
            return "the offline region definition is malformed"
        case .invalidTimeToLive:
            return "the tile-cache time-to-live must be a finite, positive whole number of seconds"
        case .timeToLiveAboveTermsCeiling(let ceilingSeconds):
            return "the tile-cache time-to-live exceeds the provider's terms ceiling of \(ceilingSeconds) seconds"
        }
    }
}
