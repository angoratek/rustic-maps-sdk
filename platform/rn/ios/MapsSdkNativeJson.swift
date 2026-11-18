import Foundation

/// JSON conversions between the package's typed JavaScript surface and the
/// generated contract records.
///
/// Complex records cross the React Native bridge as JSON strings: the typed
/// TypeScript declarations keep the exact record shapes, while the native side
/// parses one representation. Enums serialize by their zero-based JavaScript
/// enum ordinal, so this file owns the one ordinal mapping per enum.
enum MapsSdkNativeJson {

    // MARK: Decoding

    /// Decodes one camera pose from the camera object.
    static func camera(from dictionary: [String: Any]) -> MapsSdkCameraPose? {
        guard
            let latitudeDegrees = dictionary["latitudeDegrees"] as? NSNumber,
            let longitudeDegrees = dictionary["longitudeDegrees"] as? NSNumber,
            let zoomLevel = dictionary["zoomLevel"] as? NSNumber,
            let bearingDegrees = dictionary["bearingDegrees"] as? NSNumber,
            let pitchDegrees = dictionary["pitchDegrees"] as? NSNumber
        else {
            return nil
        }
        return MapsSdkCameraPose(
            latitudeDegrees: latitudeDegrees.doubleValue,
            longitudeDegrees: longitudeDegrees.doubleValue,
            zoomLevel: zoomLevel.doubleValue,
            bearingDegrees: bearingDegrees.doubleValue,
            pitchDegrees: pitchDegrees.doubleValue
        )
    }

    /// Decodes one bounding box from the bounds object.
    static func boundingBox(from dictionary: [String: Any]) -> BoundingBox? {
        guard
            let southWest = coordinate(from: dictionary["southWest"]),
            let northEast = coordinate(from: dictionary["northEast"])
        else {
            return nil
        }
        return BoundingBox(southWest: southWest, northEast: northEast)
    }

    /// Decodes one edge inset from the insets object.
    static func edgeInsets(from dictionary: [String: Any]) -> EdgeInsets {
        EdgeInsets(
            left: number(dictionary["left"]),
            top: number(dictionary["top"]),
            right: number(dictionary["right"]),
            bottom: number(dictionary["bottom"])
        )
    }

    /// Decodes one overlay descriptor from the overlay object.
    static func overlay(from dictionary: [String: Any]) -> MapOverlay? {
        guard let kindIndex = dictionary["kind"] as? NSNumber else {
            return nil
        }
        let kind: MapOverlayKind
        switch kindIndex.intValue {
        case 0: kind = .routePolyline
        case 1: kind = .filledPolygon
        case 2: kind = .pointMarkers
        case 3: kind = .trafficFlow
        case 4: kind = .pointPins
        default: return nil
        }
        return MapOverlay(
            kind: kind,
            coordinates: coordinateList(from: dictionary["coordinates"]),
            rgbaColor: color(from: dictionary["rgbaColor"]),
            widthPixels: optionalNumber(dictionary["widthPixels"]),
            ringCoordinates: coordinateList(from: dictionary["ringCoordinates"]),
            pointMarkerPositions: coordinateList(
                from: dictionary["pointMarkerPositions"]
            ),
            pointMarkerRadiusPixels: optionalNumber(
                dictionary["pointMarkerRadiusPixels"]
            ),
            trafficFlowSegments: trafficFlowSegments(
                from: dictionary["trafficFlowSegments"]
            )
        )
    }

    /// Decodes one location-layer state from the state object.
    static func locationIndicatorState(
        from dictionary: [String: Any]
    ) -> LocationIndicatorState? {
        guard let coordinate = coordinate(from: dictionary["coordinate"]) else {
            return nil
        }
        return LocationIndicatorState(
            coordinate: coordinate,
            courseHeadingDegrees: optionalNumber(
                dictionary["courseHeadingDegrees"]
            ),
            horizontalAccuracyMeters: optionalNumber(
                dictionary["horizontalAccuracyMeters"]
            )
        )
    }

    /// Decodes one location-layer style from the style object.
    static func locationIndicatorStyle(
        from dictionary: [String: Any]
    ) -> LocationIndicatorStyle? {
        guard let visualKindIndex = dictionary["visualKind"] as? NSNumber else {
            return nil
        }
        let visualKind: LocationIndicatorVisualKind
        switch visualKindIndex.intValue {
        case 0: visualKind = .trackingDot
        case 1: visualKind = .guidedCourseArrow
        default: return nil
        }
        return LocationIndicatorStyle(
            visualKind: visualKind,
            radiusPixels: optionalNumber(dictionary["radiusPixels"]),
            rgbaColor: color(from: dictionary["rgbaColor"]),
            ringRgbaColor: color(from: dictionary["ringRgbaColor"]),
            haloRgbaColor: color(from: dictionary["haloRgbaColor"]),
            showAccuracyRing: dictionary["showAccuracyRing"] as? Bool
        )
    }

    /// Decodes one base-map tile source from the source object.
    static func urlTemplateRasterTileSource(
        from dictionary: [String: Any]
    ) -> UrlTemplateRasterTileSource? {
        guard
            let urlTemplate = dictionary["urlTemplate"] as? String,
            let sourceName = dictionary["sourceName"] as? String,
            let minimumZoomLevel = zoomLevel(dictionary["minimumZoomLevel"]),
            let maximumZoomLevel = zoomLevel(dictionary["maximumZoomLevel"]),
            let imageFormatOrdinal = dictionary["imageFormat"] as? NSNumber,
            let tileResolutionOrdinal = dictionary["tileResolution"] as? NSNumber,
            let userAgent = dictionary["userAgent"] as? String
        else {
            return nil
        }
        let imageFormat: RasterImageFormat
        switch imageFormatOrdinal.intValue {
        case 0: imageFormat = .png
        case 1: imageFormat = .jpeg
        default: return nil
        }
        let tileResolution: RasterTileResolution
        switch tileResolutionOrdinal.intValue {
        case 0: tileResolution = .standard
        case 1: tileResolution = .retina
        default: return nil
        }
        return UrlTemplateRasterTileSource(
            urlTemplate: urlTemplate,
            sourceName: sourceName,
            minimumZoomLevel: minimumZoomLevel,
            maximumZoomLevel: maximumZoomLevel,
            imageFormat: imageFormat,
            tileResolution: tileResolution,
            userAgent: userAgent,
            tokenBucketCapacityTokens: optionalNumber(
                dictionary["tokenBucketCapacityTokens"]
            ),
            refillTokensPerSecond: optionalNumber(
                dictionary["refillTokensPerSecond"]
            )
        )
    }

    /// Decodes one provider-backed base source.
    static func providerVectorTileSource(
        from dictionary: [String: Any]
    ) -> ProviderVectorTileSource? {
        guard
            let providerKey = dictionary["providerKey"] as? String,
            let styleId = dictionary["styleId"] as? String
        else {
            return nil
        }
        return ProviderVectorTileSource(
            providerKey: providerKey,
            styleId: styleId
        )
    }

    /// Decodes one styled vector base source from the source object.
    static func vectorTileSource(
        from dictionary: [String: Any]
    ) -> UrlTemplateVectorTileSource? {
        guard
            let urlTemplate = dictionary["urlTemplate"] as? String,
            let sourceName = dictionary["sourceName"] as? String,
            let attributionText = dictionary["attributionText"] as? String,
            let styleId = dictionary["styleId"] as? String,
            let minimumZoomLevel = zoomLevel(dictionary["minimumZoomLevel"]),
            let maximumZoomLevel = zoomLevel(dictionary["maximumZoomLevel"])
        else {
            return nil
        }
        return UrlTemplateVectorTileSource(
            urlTemplate: urlTemplate,
            sourceName: sourceName,
            attributionText: attributionText,
            styleId: styleId,
            minimumZoomLevel: minimumZoomLevel,
            maximumZoomLevel: maximumZoomLevel,
            tokenBucketCapacityTokens: optionalNumber(
                dictionary["tokenBucketCapacityTokens"]
            ),
            refillTokensPerSecond: optionalNumber(
                dictionary["refillTokensPerSecond"]
            ),
            termsText: dictionary["termsText"] as? String,
            subdomains: stringList(dictionary["subdomains"]),
            headers: sourceHeaders(dictionary["headers"]),
            styleHints: stringList(dictionary["styleHints"])
        )
    }

    /// Decodes one route request's options from the options object.
    static func routeOptions(from dictionary: [String: Any]) -> RouteOptions? {
        guard
            let transportationModeOrdinal =
                dictionary["transportationMode"] as? NSNumber,
            let routeStrategyOrdinal = dictionary["routeStrategy"] as? NSNumber,
            let unitSystemOrdinal = dictionary["unitSystem"] as? NSNumber,
            let avoidOptionsDictionary = dictionary["avoidOptions"] as? [String: Any],
            let tollRoads = avoidOptionsDictionary["tollRoads"] as? Bool,
            let limitedAccessHighways =
                avoidOptionsDictionary["limitedAccessHighways"] as? Bool,
            let ferries = avoidOptionsDictionary["ferries"] as? Bool,
            let unpavedRoads = avoidOptionsDictionary["unpavedRoads"] as? Bool
        else {
            return nil
        }
        let transportationMode: TransportationMode
        switch transportationModeOrdinal.intValue {
        case 0: transportationMode = .automobile
        case 1: transportationMode = .bicycle
        case 2: transportationMode = .pedestrian
        default: return nil
        }
        let routeStrategy: RouteStrategy
        switch routeStrategyOrdinal.intValue {
        case 0: routeStrategy = .fastest
        case 1: routeStrategy = .shortest
        default: return nil
        }
        let unitSystem: UnitSystem
        switch unitSystemOrdinal.intValue {
        case 0: unitSystem = .metric
        case 1: unitSystem = .imperial
        default: return nil
        }
        return RouteOptions(
            transportationMode: transportationMode,
            routeStrategy: routeStrategy,
            unitSystem: unitSystem,
            avoidOptions: AvoidOptions(
                tollRoads: tollRoads,
                limitedAccessHighways: limitedAccessHighways,
                ferries: ferries,
                unpavedRoads: unpavedRoads
            ),
            departureTimestampSeconds:
                (dictionary["departureTimestampSeconds"] as? NSNumber)?.uint64Value
        )
    }

    // MARK: Encoding

    /// Encodes a provider result list for the typed search promises.
    static func placesJSON(_ places: [Place]) -> String {
        let encodedPlaces: [[String: Any]] = places.map { place in
            [
                "structuredAddressText": place.structuredAddressText,
                "coordinate": coordinateDictionary(place.coordinate),
                "geocodeQuality": geocodeQualityOrdinal(place.geocodeQuality),
            ]
        }
        return jsonString(encodedPlaces)
    }

    /// Encodes one calculated route for the typed routing promise.
    static func routeJSON(_ route: Route) -> String {
        let encodedRoute: [String: Any] = [
            "distanceMeters": route.distanceMeters,
            "durationSeconds": route.durationSeconds,
            "boundingBox": boundingBoxDictionary(route.boundingBox),
            "legs": route.legs.map { leg in
                [
                    "distanceMeters": leg.distanceMeters,
                    "durationSeconds": leg.durationSeconds,
                    "steps": leg.steps.map { step in
                        [
                            "distanceMeters": step.distanceMeters,
                            "durationSeconds": step.durationSeconds,
                            "maneuverType": step.maneuverType,
                            "instructionDisplayText": step.instructionDisplayText,
                            "instructionSpeechText": step.instructionSpeechText,
                            "shape": step.shape.map(coordinateDictionary),
                        ] as [String: Any]
                    },
                ] as [String: Any]
            },
        ]
        return jsonString(encodedRoute)
    }

    /// The transportation mode a generated enum ordinal names.
    static func transportationMode(fromOrdinal ordinal: Int) -> TransportationMode? {
        switch ordinal {
        case 0: return .automobile
        case 1: return .bicycle
        case 2: return .pedestrian
        default: return nil
        }
    }

    /// The provider contract a generated enum ordinal names.
    static func providerContract(fromOrdinal ordinal: Int) -> ProviderContract? {
        switch ordinal {
        case 0: return .geocoding
        case 1: return .routing
        case 2: return .distanceMatrix
        case 3: return .placeSearch
        case 4: return .trafficIncidents
        case 5: return .trafficFlow
        case 6: return .elevation
        case 7: return .rasterTiles
        case 8: return .vectorTiles
        case 9: return .offlineRouting
        default: return nil
        }
    }

    /// The zero-based ordinal the generated TypeScript enum uses.
    private static func providerContractOrdinal(_ contract: ProviderContract) -> Int {
        switch contract {
        case .geocoding: return 0
        case .routing: return 1
        case .distanceMatrix: return 2
        case .placeSearch: return 3
        case .trafficIncidents: return 4
        case .trafficFlow: return 5
        case .elevation: return 6
        case .rasterTiles: return 7
        case .vectorTiles: return 8
        case .offlineRouting: return 9
        }
    }

    /// The zero-based ordinal the generated TypeScript enum uses.
    private static func transportationModeOrdinal(
        _ transportationMode: TransportationMode
    ) -> Int {
        switch transportationMode {
        case .automobile: return 0
        case .bicycle: return 1
        case .pedestrian: return 2
        }
    }

    /// Encodes the SDK's per-capability readback, in the SDK's order.
    static func providerRegistrationsJSON(
        _ registrations: [ProviderRegistration]
    ) -> String {
        let encodedRegistrations: [[String: Any]] = registrations.map { registration in
            [
                "contract": providerContractOrdinal(registration.contract),
                "providerKey": registration.providerKey,
                "displayName": registration.displayName,
                "registered": registration.registered,
            ]
        }
        return jsonString(encodedRegistrations)
    }

    /// Encodes the serving routing provider's capabilities.
    static func routeCapabilitiesJSON(_ capabilities: RouteCapabilities) -> String {
        jsonString([
            "supportedTransportationModes":
                capabilities.supportedTransportationModes.map(transportationModeOrdinal),
            "supportsShortestRouteStrategy":
                capabilities.supportsShortestRouteStrategy,
        ])
    }

    /// Encodes the contract's distance matrix.
    static func distanceMatrixJSON(_ distanceMatrix: DistanceMatrix) -> String {
        let encodedEntries: [[String: Any]] = distanceMatrix.entries.map { entry in
            [
                "originIndex": Int(entry.originIndex),
                "destinationIndex": Int(entry.destinationIndex),
                "distanceMeters": entry.distanceMeters,
                "durationSeconds": entry.durationSeconds,
            ]
        }
        return jsonString(["entries": encodedEntries])
    }

    /// Encodes the contract's traffic incidents.
    static func trafficIncidentsJSON(_ incidents: [TrafficIncident]) -> String {
        let encodedIncidents: [[String: Any]] = incidents.map { incident in
            var encodedIncident: [String: Any] = [
                "identifier": incident.identifier,
                "incidentType": trafficIncidentTypeOrdinal(incident.incidentType),
                "severity": trafficIncidentSeverityOrdinal(incident.severity),
                "shortDescriptionText": incident.shortDescriptionText,
                "startCoordinate": coordinateDictionary(incident.startCoordinate),
            ]
            if let endCoordinate = incident.endCoordinate {
                encodedIncident["endCoordinate"] = coordinateDictionary(endCoordinate)
            }
            return encodedIncident
        }
        return jsonString(encodedIncidents)
    }

    /// Encodes the contract's traffic flow observations.
    static func trafficFlowJSON(_ flowObservations: [TrafficFlowData]) -> String {
        let encodedFlow: [[String: Any]] = flowObservations.map { flowObservation in
            var encodedObservation: [String: Any] = [
                "shapeCoordinates":
                    flowObservation.shapeCoordinates.map(coordinateDictionary),
                "congestionLevel":
                    trafficFlowCongestionLevelOrdinal(flowObservation.congestionLevel),
            ]
            if let currentSpeed = flowObservation.currentSpeedKilometersPerHour {
                encodedObservation["currentSpeedKilometersPerHour"] = currentSpeed
            }
            if let freeFlowSpeed = flowObservation.freeFlowSpeedKilometersPerHour {
                encodedObservation["freeFlowSpeedKilometersPerHour"] = freeFlowSpeed
            }
            return encodedObservation
        }
        return jsonString(encodedFlow)
    }

    private static func trafficIncidentTypeOrdinal(_ incidentType: TrafficIncidentType) -> Int {
        switch incidentType {
        case .construction: return 0
        case .event: return 1
        case .accident: return 2
        case .weather: return 3
        case .closure: return 4
        }
    }

    private static func trafficIncidentSeverityOrdinal(
        _ severity: TrafficIncidentSeverity
    ) -> Int {
        switch severity {
        case .low: return 0
        case .medium: return 1
        case .high: return 2
        }
    }

    private static func trafficFlowCongestionLevelOrdinal(
        _ congestionLevel: TrafficFlowCongestionLevel
    ) -> Int {
        switch congestionLevel {
        case .freeFlow: return 0
        case .slow: return 1
        case .congested: return 2
        case .blocked: return 3
        }
    }

    /// Encodes the installed cache's terms.
    static func tileCacheTermsJSON(_ terms: TileCacheTerms) -> String {
        var encodedTerms: [String: Any] = [
            "effectiveTimeToLiveSeconds": terms.effectiveTimeToLiveSeconds,
            "defaultTimeToLiveSeconds": terms.defaultTimeToLiveSeconds,
        ]
        if let ceilingSeconds = terms.providerTermsCeilingSeconds {
            encodedTerms["providerTermsCeilingSeconds"] = ceilingSeconds
        }
        return jsonString(encodedTerms)
    }

    /// Encodes the installed tile payload cache's exact counters.
    static func tileCacheStatisticsJSON(
        _ statistics: TileCacheStatistics
    ) -> String {
        var encodedStatistics: [String: Any] = [
            "hitCount": statistics.hitCount,
            "missCount": statistics.missCount,
        ]
        if let storedByteCount = statistics.storedByteCount {
            encodedStatistics["storedByteCount"] = storedByteCount
        }
        return jsonString(encodedStatistics)
    }

    /// Encodes the serving traffic-flow cache's counters.
    static func trafficFlowCacheStatisticsJSON(
        _ statistics: TrafficFlowCacheStatistics
    ) -> String {
        jsonString([
            "hitCount": statistics.hitCount,
            "missCount": statistics.missCount,
            "entryCount": statistics.entryCount,
            "evictionCount": statistics.evictionCount,
            "segmentCount": statistics.segmentCount,
            "gridZoomLevel": statistics.gridZoomLevel,
            "maximumEntryCount": statistics.maximumEntryCount,
            "timeToLiveSeconds": statistics.timeToLiveSeconds,
        ])
    }

    /// Decodes one offline region definition from the region object.
    static func regionDefinition(
        from dictionary: [String: Any]
    ) -> RegionDefinition? {
        guard
            let regionId = dictionary["regionId"] as? String,
            let displayName = dictionary["displayName"] as? String,
            let boundingBoxDictionary = dictionary["boundingBox"] as? [String: Any],
            let boundingBox = boundingBox(from: boundingBoxDictionary),
            let minimumZoomLevel = zoomLevel(dictionary["minimumZoomLevel"]),
            let maximumZoomLevel = zoomLevel(dictionary["maximumZoomLevel"]),
            let imageFormatOrdinal = dictionary["imageFormat"] as? NSNumber,
            let tileResolutionOrdinal = dictionary["tileResolution"] as? NSNumber
        else {
            return nil
        }
        let imageFormat: RasterImageFormat
        switch imageFormatOrdinal.intValue {
        case 0: imageFormat = .png
        case 1: imageFormat = .jpeg
        default: return nil
        }
        let tileResolution: RasterTileResolution
        switch tileResolutionOrdinal.intValue {
        case 0: tileResolution = .standard
        case 1: tileResolution = .retina
        default: return nil
        }
        let timeToLiveSeconds = (dictionary["timeToLiveSeconds"] as? NSNumber)?
            .uint64Value
        return RegionDefinition(
            regionId: regionId,
            displayName: displayName,
            boundingBox: boundingBox,
            minimumZoomLevel: minimumZoomLevel,
            maximumZoomLevel: maximumZoomLevel,
            imageFormat: imageFormat,
            tileResolution: tileResolution,
            timeToLiveSeconds: timeToLiveSeconds
        )
    }

    /// Encodes one stored region record.
    static func regionRecordJSON(_ record: RegionRecord) -> String {
        jsonString(regionRecordDictionary(record))
    }

    /// Encodes the stored-region listing.
    static func regionRecordsJSON(_ records: [RegionRecord]) -> String {
        jsonString(records.map(regionRecordDictionary))
    }

    /// Encodes what a region's explicit deletion removed.
    static func regionRemovalReportJSON(
        _ report: RegionRemovalReport
    ) -> String {
        jsonString([
            "removedEntryCount": report.removedEntryCount,
            "removedPayloadBytes": report.removedPayloadBytes,
        ])
    }

    private static func regionRecordDictionary(
        _ record: RegionRecord
    ) -> [String: Any] {
        [
            "regionId": record.regionId,
            "displayName": record.displayName,
            "boundingBox": [
                "southWest": coordinateDictionary(record.boundingBox.southWest),
                "northEast": coordinateDictionary(record.boundingBox.northEast),
            ],
            "minimumZoomLevel": Int(record.minimumZoomLevel),
            "maximumZoomLevel": Int(record.maximumZoomLevel),
            "imageFormat": regionImageFormatOrdinal(record.imageFormat),
            "tileResolution": regionTileResolutionOrdinal(record.tileResolution),
            "effectiveTimeToLiveSeconds": record.effectiveTimeToLiveSeconds,
            "memberTileCount": record.memberTileCount,
            "presentTileCount": record.presentTileCount,
            "presentPayloadBytes": record.presentPayloadBytes,
        ]
    }

    private static func regionImageFormatOrdinal(
        _ imageFormat: RasterImageFormat
    ) -> Int {
        switch imageFormat {
        case .png: return 0
        case .jpeg: return 1
        }
    }

    private static func regionTileResolutionOrdinal(
        _ tileResolution: RasterTileResolution
    ) -> Int {
        switch tileResolution {
        case .standard: return 0
        case .retina: return 1
        }
    }

    /// Encodes the retained provider-failure diagnostics.
    static func providerFailureDiagnosticsJSON(
        _ diagnostics: [ProviderFailureDiagnostic]
    ) -> String {
        let encodedDiagnostics: [[String: Any]] = diagnostics.map { diagnostic in
            [
                "timestampMillisecondsSinceEpoch":
                    diagnostic.timestampMillisecondsSinceEpoch,
                "failureClassification":
                    providerFailureClassificationOrdinal(
                        diagnostic.failureClassification
                    ),
                "providerContractName": diagnostic.providerContractName,
                "providerMethodName": diagnostic.providerMethodName,
                "failureDetail": diagnostic.failureDetail,
            ]
        }
        return jsonString(encodedDiagnostics)
    }

    /// Decodes a JSON string into a dictionary, or answers nil.
    static func dictionary(from jsonString: String) -> [String: Any]? {
        guard let data = jsonString.data(using: .utf8) else { return nil }
        return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
    }

    /// Decodes a JSON string into an array of dictionaries, or answers nil.
    static func dictionaryArray(from jsonString: String) -> [[String: Any]]? {
        guard let data = jsonString.data(using: .utf8) else { return nil }
        return (try? JSONSerialization.jsonObject(with: data)) as? [[String: Any]]
    }

    private static func jsonString(_ object: Any) -> String {
        guard
            let data = try? JSONSerialization.data(withJSONObject: object),
            let encoded = String(data: data, encoding: .utf8)
        else {
            return "[]"
        }
        return encoded
    }

    private static func coordinate(from object: Any?) -> GeographicCoordinate? {
        guard
            let dictionary = object as? [String: Any],
            let latitude = dictionary["latitude"] as? NSNumber,
            let longitude = dictionary["longitude"] as? NSNumber
        else {
            return nil
        }
        return GeographicCoordinate(
            latitude: latitude.doubleValue,
            longitude: longitude.doubleValue
        )
    }

    /// Decodes the ordered routing waypoints from the waypoint dictionaries.
    static func coordinates(
        from dictionaryArray: [[String: Any]]
    ) -> [GeographicCoordinate] {
        dictionaryArray.compactMap { dictionary in coordinate(from: dictionary) }
    }

    private static func coordinateDictionary(
        _ coordinate: GeographicCoordinate
    ) -> [String: Any] {
        ["latitude": coordinate.latitude, "longitude": coordinate.longitude]
    }

    private static func boundingBoxDictionary(
        _ boundingBox: BoundingBox
    ) -> [String: Any] {
        [
            "southWest": coordinateDictionary(boundingBox.southWest),
            "northEast": coordinateDictionary(boundingBox.northEast),
        ]
    }

    private static func coordinateList(from object: Any?) -> [GeographicCoordinate]? {
        guard let list = object as? [Any] else { return nil }
        return list.compactMap { coordinate(from: $0) }
    }

    private static func color(from object: Any?) -> OverlayColor? {
        guard
            let dictionary = object as? [String: Any],
            let red = dictionary["red"] as? NSNumber,
            let green = dictionary["green"] as? NSNumber,
            let blue = dictionary["blue"] as? NSNumber,
            let alpha = dictionary["alpha"] as? NSNumber
        else {
            return nil
        }
        return OverlayColor(
            red: red.doubleValue,
            green: green.doubleValue,
            blue: blue.doubleValue,
            alpha: alpha.doubleValue
        )
    }

    private static func trafficFlowSegments(
        from object: Any?
    ) -> [TrafficFlowData]? {
        guard let list = object as? [Any] else { return nil }
        return list.compactMap { segmentObject in
            guard
                let dictionary = segmentObject as? [String: Any],
                let congestionIndex = dictionary["congestionLevel"] as? NSNumber
            else {
                return nil
            }
            let congestionLevel: TrafficFlowCongestionLevel
            switch congestionIndex.intValue {
            case 0: congestionLevel = .freeFlow
            case 1: congestionLevel = .slow
            case 2: congestionLevel = .congested
            case 3: congestionLevel = .blocked
            default: return nil
            }
            return TrafficFlowData(
                shapeCoordinates: coordinateList(
                    from: dictionary["shapeCoordinates"]
                ) ?? [],
                currentSpeedKilometersPerHour: optionalNumber(
                    dictionary["currentSpeedKilometersPerHour"]
                ),
                freeFlowSpeedKilometersPerHour: optionalNumber(
                    dictionary["freeFlowSpeedKilometersPerHour"]
                ),
                congestionLevel: congestionLevel
            )
        }
    }

    private static func number(_ object: Any?) -> Double {
        (object as? NSNumber)?.doubleValue ?? 0
    }

    private static func optionalNumber(_ object: Any?) -> Double? {
        (object as? NSNumber)?.doubleValue
    }

    /// Decodes an optional string sequence: an absent or empty entry is null,
    /// a present array keeps its order and values.
    private static func stringList(_ object: Any?) -> [String]? {
        object as? [String]
    }

    /// Decodes an optional header sequence. A malformed entry drops the whole
    /// header list, so the record installs without headers rather than with a
    /// partially applied set; it never silently drops one entry and keeps the
    /// rest.
    private static func sourceHeaders(_ object: Any?) -> [SourceHeader]? {
        guard let headerDictionaries = object as? [[String: Any]] else {
            return nil
        }
        var headers: [SourceHeader] = []
        for headerDictionary in headerDictionaries {
            guard
                let name = headerDictionary["name"] as? String,
                let value = headerDictionary["value"] as? String
            else {
                return nil
            }
            headers.append(SourceHeader(name: name, value: value))
        }
        return headers
    }

    /// Decodes one `u8` zoom level, refusing a value outside its range.
    private static func zoomLevel(_ object: Any?) -> UInt8? {
        guard
            let number = object as? NSNumber,
            let zoomLevel = UInt8(exactly: number.intValue)
        else {
            return nil
        }
        return zoomLevel
    }

    private static func geocodeQualityOrdinal(_ quality: GeocodeQuality) -> Int {
        switch quality {
        case .point: return 0
        case .streetAddress: return 1
        case .intersection: return 2
        case .street: return 3
        case .postalCode: return 4
        case .city: return 5
        case .county: return 6
        case .state: return 7
        case .country: return 8
        }
    }

    private static func providerFailureClassificationOrdinal(
        _ classification: ProviderFailureClassification
    ) -> Int {
        switch classification {
        case .unclassifiedCallbackFailure: return 0
        case .mappedContractError: return 1
        }
    }
}
