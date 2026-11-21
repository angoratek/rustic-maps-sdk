package com.angoratek.rusticmaps

import com.rusticmaps.sdk.MapCameraPose
import org.json.JSONArray
import org.json.JSONObject
import uniffi.rustic_maps.AvoidOptions
import uniffi.rustic_maps.BoundingBox
import uniffi.rustic_maps.DistanceMatrix
import uniffi.rustic_maps.EdgeInsets
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.LocationIndicatorState
import uniffi.rustic_maps.LocationIndicatorStyle
import uniffi.rustic_maps.LocationIndicatorVisualKind
import uniffi.rustic_maps.MapOverlay
import uniffi.rustic_maps.MapOverlayKind
import uniffi.rustic_maps.OverlayColor
import uniffi.rustic_maps.Place
import uniffi.rustic_maps.ProviderContract
import uniffi.rustic_maps.ProviderFailureDiagnostic
import uniffi.rustic_maps.ProviderRegistration
import uniffi.rustic_maps.ProviderVectorTileSource
import uniffi.rustic_maps.RasterImageFormat
import uniffi.rustic_maps.RasterTileResolution
import uniffi.rustic_maps.RegionDefinition
import uniffi.rustic_maps.RegionRecord
import uniffi.rustic_maps.RegionRemovalReport
import uniffi.rustic_maps.Route
import uniffi.rustic_maps.RouteCapabilities
import uniffi.rustic_maps.RouteLeg
import uniffi.rustic_maps.RouteOptions
import uniffi.rustic_maps.RouteStep
import uniffi.rustic_maps.RouteStrategy
import uniffi.rustic_maps.SourceHeader
import uniffi.rustic_maps.TileCacheStatistics
import uniffi.rustic_maps.TileCacheTerms
import uniffi.rustic_maps.TrafficFlowCacheStatistics
import uniffi.rustic_maps.TrafficIncident
import uniffi.rustic_maps.TrafficFlowCongestionLevel
import uniffi.rustic_maps.TrafficFlowData
import uniffi.rustic_maps.TransportationMode
import uniffi.rustic_maps.UnitSystem
import uniffi.rustic_maps.UrlTemplateVectorTileSource
import uniffi.rustic_maps.VectorTileSourceStatus

/**
 * JSON conversions between the package's typed JavaScript surface and the
 * generated Kotlin records.
 *
 * Complex records cross the React Native bridge as JSON strings: the typed
 * TypeScript declarations still describe the exact record shapes, while the
 * native side parses one representation instead of mirroring every nested
 * record as a bridge map. Enums serialize by their zero-based ordinal, which
 * is the numeric value of the matching TypeScript enum.
 */
object MapsSdkNativeJson {

    /** Decodes one styled vector base source from the source object. */
    fun vectorTileSourceFromJson(sourceJson: String): UrlTemplateVectorTileSource {
        val sourceObject = JSONObject(sourceJson)
        return UrlTemplateVectorTileSource(
            urlTemplate = sourceObject.getString("urlTemplate"),
            sourceName = sourceObject.getString("sourceName"),
            attributionText = sourceObject.getString("attributionText"),
            styleId = sourceObject.getString("styleId"),
            minimumZoomLevel = sourceObject.getInt("minimumZoomLevel").toUByte(),
            maximumZoomLevel = sourceObject.getInt("maximumZoomLevel").toUByte(),
            tokenBucketCapacityTokens = optionalDouble(sourceObject, "tokenBucketCapacityTokens"),
            refillTokensPerSecond = optionalDouble(sourceObject, "refillTokensPerSecond"),
            termsText = optionalString(sourceObject, "termsText"),
            subdomains = optionalStringList(sourceObject, "subdomains"),
            headers = optionalSourceHeaders(sourceObject, "headers"),
            styleHints = optionalStringList(sourceObject, "styleHints"),
        )
    }

    /** Decodes one provider-backed base source from the source object. */
    fun providerVectorTileSourceFromJson(sourceJson: String): ProviderVectorTileSource {
        val sourceObject = JSONObject(sourceJson)
        return ProviderVectorTileSource(
            providerKey = sourceObject.getString("providerKey"),
            styleId = sourceObject.getString("styleId"),
        )
    }

    /** Encodes one vector base status for the JavaScript readback. */
    fun vectorTileSourceStatusToJson(status: VectorTileSourceStatus): String {
        val statusObject = JSONObject()
        statusObject.put("isConfigured", status.isConfigured)
        statusObject.put("styleId", status.styleId)
        statusObject.put("sourceName", status.sourceName)
        statusObject.put("attributionText", status.attributionText)
        statusObject.put("maximumZoomLevel", status.maximumZoomLevel.toInt())
        statusObject.put("drawnTileCount", status.drawnTileCount.toLong())
        statusObject.put("failedTileCount", status.failedTileCount.toLong())
        statusObject.put("unsupportedPropertyCount", status.unsupportedPropertyCount.toInt())
        statusObject.put("unsupportedPropertyNames", JSONArray(status.unsupportedPropertyNames))
        statusObject.put("degradedAssetNames", JSONArray(status.degradedAssetNames))
        val sourceLayerCounts = JSONArray()
        status.sourceLayerCounts.forEach { sourceLayerCount ->
            sourceLayerCounts.put(
                JSONObject()
                    .put("sourceId", sourceLayerCount.sourceId)
                    .put("matchedLayerCount", sourceLayerCount.matchedLayerCount.toInt())
                    .put("drawnLayerCount", sourceLayerCount.drawnLayerCount.toInt()),
            )
        }
        statusObject.put("sourceLayerCounts", sourceLayerCounts)
        status.lastFailureDetail?.let { detail -> statusObject.put("lastFailureDetail", detail) }
        return statusObject.toString()
    }

    /** Decodes one `MapCameraPose` from the camera object. */
    fun cameraFromJson(cameraJson: String): MapCameraPose {
        val cameraObject = JSONObject(cameraJson)
        return MapCameraPose(
            latitudeDegrees = cameraObject.getDouble("latitudeDegrees"),
            longitudeDegrees = cameraObject.getDouble("longitudeDegrees"),
            zoomLevel = cameraObject.getDouble("zoomLevel"),
            bearingDegrees = cameraObject.getDouble("bearingDegrees"),
            pitchDegrees = cameraObject.getDouble("pitchDegrees"),
        )
    }

    /** Decodes one `BoundingBox` from the bounds object. */
    fun boundingBoxFromJson(boundingBoxJson: String): BoundingBox {
        val boundingBoxObject = JSONObject(boundingBoxJson)
        return BoundingBox(
            southWest = coordinateFromJson(boundingBoxObject.getJSONObject("southWest")),
            northEast = coordinateFromJson(boundingBoxObject.getJSONObject("northEast")),
        )
    }

    /** Decodes one `EdgeInsets` from the insets object. */
    fun edgeInsetsFromJson(edgeInsetsJson: String): EdgeInsets {
        val edgeInsetsObject = JSONObject(edgeInsetsJson)
        return EdgeInsets(
            left = edgeInsetsObject.optDouble("left", 0.0),
            top = edgeInsetsObject.optDouble("top", 0.0),
            right = edgeInsetsObject.optDouble("right", 0.0),
            bottom = edgeInsetsObject.optDouble("bottom", 0.0),
        )
    }

    /** Decodes one overlay descriptor from the overlay object. */
    fun overlayFromJson(overlayJson: String): MapOverlay {
        val overlayObject = JSONObject(overlayJson)
        val kind = MapOverlayKind.values()[overlayObject.getInt("kind")]
        return MapOverlay(
            kind = kind,
            coordinates = optionalCoordinateList(overlayObject, "coordinates"),
            rgbaColor = optionalColor(overlayObject, "rgbaColor"),
            widthPixels = optionalDouble(overlayObject, "widthPixels"),
            ringCoordinates = optionalCoordinateList(overlayObject, "ringCoordinates"),
            pointMarkerPositions = optionalCoordinateList(
                overlayObject,
                "pointMarkerPositions",
            ),
            pointMarkerRadiusPixels = optionalDouble(
                overlayObject,
                "pointMarkerRadiusPixels",
            ),
            trafficFlowSegments = optionalTrafficFlowSegments(overlayObject),
        )
    }

    /** Decodes one location-layer state from the state object. */
    fun locationIndicatorStateFromJson(stateJson: String): LocationIndicatorState {
        val stateObject = JSONObject(stateJson)
        return LocationIndicatorState(
            coordinate = coordinateFromJson(stateObject.getJSONObject("coordinate")),
            courseHeadingDegrees = optionalDouble(stateObject, "courseHeadingDegrees"),
            horizontalAccuracyMeters = optionalDouble(
                stateObject,
                "horizontalAccuracyMeters",
            ),
        )
    }

    /** Decodes one location-layer style from the style object. */
    fun locationIndicatorStyleFromJson(styleJson: String): LocationIndicatorStyle {
        val styleObject = JSONObject(styleJson)
        return LocationIndicatorStyle(
            visualKind = LocationIndicatorVisualKind.values()[styleObject.getInt("visualKind")],
            radiusPixels = optionalDouble(styleObject, "radiusPixels"),
            rgbaColor = optionalColor(styleObject, "rgbaColor"),
            ringRgbaColor = optionalColor(styleObject, "ringRgbaColor"),
            haloRgbaColor = optionalColor(styleObject, "haloRgbaColor"),
            showAccuracyRing = optionalBoolean(styleObject, "showAccuracyRing"),
        )
    }

    /** Decodes the ordered routing waypoints from the waypoints array. */
    fun coordinatesFromJson(coordinatesJson: String): List<GeographicCoordinate> {
        val coordinateArray = JSONArray(coordinatesJson)
        return (0 until coordinateArray.length()).map { coordinateIndex ->
            coordinateFromJson(coordinateArray.getJSONObject(coordinateIndex))
        }
    }

    /** Decodes one `RouteOptions` from the options object. */
    fun routeOptionsFromJson(routeOptionsJson: String): RouteOptions {
        val routeOptionsObject = JSONObject(routeOptionsJson)
        val avoidOptionsObject = routeOptionsObject.getJSONObject("avoidOptions")
        return RouteOptions(
            transportationMode = TransportationMode.values()[
                routeOptionsObject.getInt("transportationMode")
            ],
            routeStrategy = RouteStrategy.values()[
                routeOptionsObject.getInt("routeStrategy")
            ],
            unitSystem = UnitSystem.values()[routeOptionsObject.getInt("unitSystem")],
            avoidOptions = AvoidOptions(
                tollRoads = avoidOptionsObject.getBoolean("tollRoads"),
                limitedAccessHighways = avoidOptionsObject.getBoolean(
                    "limitedAccessHighways",
                ),
                ferries = avoidOptionsObject.getBoolean("ferries"),
                unpavedRoads = avoidOptionsObject.getBoolean("unpavedRoads"),
            ),
            departureTimestampSeconds = optionalLong(
                routeOptionsObject,
                "departureTimestampSeconds",
            )?.toULong(),
        )
    }

    /** Encodes one calculated route for the typed routing promise. */
    fun routeToJson(route: Route): String {
        val routeObject = JSONObject()
        routeObject.put("distanceMeters", route.distanceMeters)
        routeObject.put("durationSeconds", route.durationSeconds)
        routeObject.put("boundingBox", boundingBoxToJson(route.boundingBox))
        routeObject.put("legs", routeLegsToJson(route.legs))
        return routeObject.toString()
    }

    /** Encodes a provider result list for the typed search promises. */
    fun placesToJson(places: List<Place>): String {
        val placesArray = JSONArray()
        places.forEach { place ->
            val placeObject = JSONObject()
            placeObject.put("structuredAddressText", place.structuredAddressText)
            placeObject.put("coordinate", coordinateToJson(place.coordinate))
            placeObject.put("geocodeQuality", place.geocodeQuality.ordinal)
            placesArray.put(placeObject)
        }
        return placesArray.toString()
    }

    /** Encodes the installed cache's terms. */
    fun tileCacheTermsToJson(terms: TileCacheTerms): String {
        val termsObject = JSONObject()
        termsObject.put(
            "effectiveTimeToLiveSeconds",
            terms.effectiveTimeToLiveSeconds.toLong(),
        )
        terms.providerTermsCeilingSeconds?.let { ceilingSeconds ->
            termsObject.put("providerTermsCeilingSeconds", ceilingSeconds.toLong())
        }
        termsObject.put(
            "defaultTimeToLiveSeconds",
            terms.defaultTimeToLiveSeconds.toLong(),
        )
        return termsObject.toString()
    }

    /** Encodes the installed tile payload cache's exact counters. */
    fun tileCacheStatisticsToJson(statistics: TileCacheStatistics): String {
        val statisticsObject = JSONObject()
        statisticsObject.put("hitCount", statistics.hitCount.toLong())
        statisticsObject.put("missCount", statistics.missCount.toLong())
        statistics.storedByteCount?.let { storedByteCount ->
            statisticsObject.put("storedByteCount", storedByteCount.toLong())
        }
        return statisticsObject.toString()
    }

    /** Encodes the serving traffic-flow cache's counters. */
    fun trafficFlowCacheStatisticsToJson(
        statistics: TrafficFlowCacheStatistics,
    ): String {
        val statisticsObject = JSONObject()
        statisticsObject.put("hitCount", statistics.hitCount.toLong())
        statisticsObject.put("missCount", statistics.missCount.toLong())
        statisticsObject.put("entryCount", statistics.entryCount.toLong())
        statisticsObject.put("evictionCount", statistics.evictionCount.toLong())
        statisticsObject.put("segmentCount", statistics.segmentCount.toLong())
        statisticsObject.put("gridZoomLevel", statistics.gridZoomLevel.toInt())
        statisticsObject.put(
            "maximumEntryCount",
            statistics.maximumEntryCount.toLong(),
        )
        statisticsObject.put(
            "timeToLiveSeconds",
            statistics.timeToLiveSeconds.toLong(),
        )
        return statisticsObject.toString()
    }

    /** Decodes one region definition from the region object. */
    fun regionDefinitionFromJson(regionJson: String): RegionDefinition {
        val regionObject = JSONObject(regionJson)
        val imageFormat = when (regionObject.getInt("imageFormat")) {
            RASTER_IMAGE_FORMAT_PNG_ORDINAL -> RasterImageFormat.PNG
            RASTER_IMAGE_FORMAT_JPEG_ORDINAL -> RasterImageFormat.JPEG
            else -> throw IllegalArgumentException(
                "unknown raster image format ordinal " +
                    regionObject.getInt("imageFormat"),
            )
        }
        val tileResolution = when (regionObject.getInt("tileResolution")) {
            RASTER_TILE_RESOLUTION_STANDARD_ORDINAL -> RasterTileResolution.STANDARD
            RASTER_TILE_RESOLUTION_RETINA_ORDINAL -> RasterTileResolution.RETINA
            else -> throw IllegalArgumentException(
                "unknown raster tile resolution ordinal " +
                    regionObject.getInt("tileResolution"),
            )
        }
        val timeToLiveSeconds = if (regionObject.isNull("timeToLiveSeconds")) {
            null
        } else {
            regionObject.getLong("timeToLiveSeconds").toULong()
        }
        return RegionDefinition(
            regionId = regionObject.getString("regionId"),
            displayName = regionObject.getString("displayName"),
            boundingBox = boundingBoxFromJson(
                regionObject.getJSONObject("boundingBox").toString(),
            ),
            minimumZoomLevel = regionZoomLevelFromJson(
                regionObject,
                "minimumZoomLevel",
            ),
            maximumZoomLevel = regionZoomLevelFromJson(
                regionObject,
                "maximumZoomLevel",
            ),
            imageFormat = imageFormat,
            tileResolution = tileResolution,
            timeToLiveSeconds = timeToLiveSeconds,
        )
    }

    /** Encodes one stored region record. */
    fun regionRecordToJson(record: RegionRecord): String {
        val recordObject = JSONObject()
        recordObject.put("regionId", record.regionId)
        recordObject.put("displayName", record.displayName)
        recordObject.put("boundingBox", boundingBoxToJson(record.boundingBox))
        recordObject.put("minimumZoomLevel", record.minimumZoomLevel.toInt())
        recordObject.put("maximumZoomLevel", record.maximumZoomLevel.toInt())
        recordObject.put("imageFormat", record.imageFormat.ordinal)
        recordObject.put("tileResolution", record.tileResolution.ordinal)
        recordObject.put(
            "effectiveTimeToLiveSeconds",
            record.effectiveTimeToLiveSeconds.toLong(),
        )
        recordObject.put("memberTileCount", record.memberTileCount.toLong())
        recordObject.put("presentTileCount", record.presentTileCount.toLong())
        recordObject.put(
            "presentPayloadBytes",
            record.presentPayloadBytes.toLong(),
        )
        return recordObject.toString()
    }

    /** Encodes the stored-region listing. */
    fun regionRecordsToJson(records: List<RegionRecord>): String {
        val recordsArray = JSONArray()
        records.forEach { record ->
            recordsArray.put(JSONObject(regionRecordToJson(record)))
        }
        return recordsArray.toString()
    }

    /** Encodes what a region's explicit deletion removed. */
    fun regionRemovalReportToJson(report: RegionRemovalReport): String {
        val reportObject = JSONObject()
        reportObject.put("removedEntryCount", report.removedEntryCount.toLong())
        reportObject.put(
            "removedPayloadBytes",
            report.removedPayloadBytes.toLong(),
        )
        return reportObject.toString()
    }

    /** Decodes one region zoom level, refusing a value a `u8` cannot carry. */
    private fun regionZoomLevelFromJson(
        regionObject: JSONObject,
        fieldName: String,
    ): UByte {
        val zoomLevel = regionObject.getInt(fieldName)
        require(zoomLevel in MINIMUM_ZOOM_LEVEL..MAXIMUM_ZOOM_LEVEL) {
            "$fieldName must be within $MINIMUM_ZOOM_LEVEL..$MAXIMUM_ZOOM_LEVEL (was $zoomLevel)"
        }
        return zoomLevel.toUByte()
    }

    /** The generated contract's image-format ordinals (PNG, JPEG). */
    private const val RASTER_IMAGE_FORMAT_PNG_ORDINAL: Int = 0
    private const val RASTER_IMAGE_FORMAT_JPEG_ORDINAL: Int = 1

    /** The generated contract's tile-resolution ordinals (standard, retina). */
    private const val RASTER_TILE_RESOLUTION_STANDARD_ORDINAL: Int = 0
    private const val RASTER_TILE_RESOLUTION_RETINA_ORDINAL: Int = 1

    /** The inclusive zoom-level range a `u8` contract field can carry. */
    private const val MINIMUM_ZOOM_LEVEL: Int = 0
    private const val MAXIMUM_ZOOM_LEVEL: Int = 255

    /** The provider contract a generated enum ordinal names. */
    fun providerContractFromOrdinal(contractOrdinal: Int): ProviderContract {
        val contracts = ProviderContract.values()
        require(contractOrdinal in contracts.indices) {
            "unknown provider contract ordinal $contractOrdinal"
        }
        return contracts[contractOrdinal]
    }

    /**
     * Encodes the SDK's per-capability readback, in the order the SDK returns
     * it. The contract crosses as its zero-based ordinal, the generated
     * TypeScript enum's numeric value.
     */
    fun providerRegistrationsToJson(
        registrations: List<ProviderRegistration>,
    ): String {
        val registrationsArray = JSONArray()
        registrations.forEach { registration ->
            val registrationObject = JSONObject()
            registrationObject.put("contract", registration.contract.ordinal)
            registrationObject.put("providerKey", registration.providerKey)
            registrationObject.put("displayName", registration.displayName)
            registrationObject.put("registered", registration.registered)
            registrationsArray.put(registrationObject)
        }
        return registrationsArray.toString()
    }

    /** Encodes the serving routing provider's capabilities. */
    fun routeCapabilitiesToJson(capabilities: RouteCapabilities): String {
        val modesArray = JSONArray()
        capabilities.supportedTransportationModes.forEach { transportationMode ->
            modesArray.put(transportationMode.ordinal)
        }
        return JSONObject()
            .put("supportedTransportationModes", modesArray)
            .put(
                "supportsShortestRouteStrategy",
                capabilities.supportsShortestRouteStrategy,
            )
            .toString()
    }

    /** Encodes the retained provider-failure diagnostics. */
    /** The transportation mode a generated enum ordinal names. */
    fun transportationModeFromOrdinal(transportationModeOrdinal: Int): TransportationMode {
        val transportation_modes = TransportationMode.values()
        require(transportationModeOrdinal in transportation_modes.indices) {
            "unknown transportation mode ordinal $transportationModeOrdinal"
        }
        return transportation_modes[transportationModeOrdinal]
    }

    /** Encodes the contract's distance matrix. */
    fun distanceMatrixToJson(distanceMatrix: DistanceMatrix): String {
        val entries_array = JSONArray()
        for (entry in distanceMatrix.entries) {
            val entry_object = JSONObject()
            entry_object.put("originIndex", entry.originIndex.toInt())
            entry_object.put("destinationIndex", entry.destinationIndex.toInt())
            entry_object.put("distanceMeters", entry.distanceMeters)
            entry_object.put("durationSeconds", entry.durationSeconds)
            entries_array.put(entry_object)
        }
        return JSONObject().put("entries", entries_array).toString()
    }

    /** Encodes the contract's traffic incidents. */
    fun trafficIncidentsToJson(incidents: List<TrafficIncident>): String {
        val incidents_array = JSONArray()
        for (incident in incidents) {
            val incident_object = JSONObject()
            incident_object.put("identifier", incident.identifier)
            // Generated enums cross the bridge by their zero-based ordinal
            // (the matching TypeScript enum's numeric value).
            incident_object.put("incidentType", incident.incidentType.ordinal)
            incident_object.put("severity", incident.severity.ordinal)
            incident_object.put("shortDescriptionText", incident.shortDescriptionText)
            incident_object.put("startCoordinate", coordinateToJson(incident.startCoordinate))
            incident.endCoordinate?.let { end_coordinate ->
                incident_object.put("endCoordinate", coordinateToJson(end_coordinate))
            }
            incidents_array.put(incident_object)
        }
        return incidents_array.toString()
    }

    /** Encodes the contract's traffic flow observations. */
    fun trafficFlowToJson(flow_observations: List<TrafficFlowData>): String {
        val flow_array = JSONArray()
        for (flow_observation in flow_observations) {
            val flow_object = JSONObject()
            flow_object.put(
                "shapeCoordinates",
                coordinatesToJson(flow_observation.shapeCoordinates),
            )
            flow_observation.currentSpeedKilometersPerHour?.let { current_speed ->
                flow_object.put("currentSpeedKilometersPerHour", current_speed)
            }
            flow_observation.freeFlowSpeedKilometersPerHour?.let { free_flow_speed ->
                flow_object.put("freeFlowSpeedKilometersPerHour", free_flow_speed)
            }
            flow_object.put("congestionLevel", flow_observation.congestionLevel.ordinal)
            flow_array.put(flow_object)
        }
        return flow_array.toString()
    }

    fun providerFailureDiagnosticsToJson(
        diagnostics: List<ProviderFailureDiagnostic>,
    ): String {
        val diagnosticsArray = JSONArray()
        diagnostics.forEach { diagnostic ->
            val diagnosticObject = JSONObject()
            diagnosticObject.put(
                "timestampMillisecondsSinceEpoch",
                diagnostic.timestampMillisecondsSinceEpoch.toLong(),
            )
            diagnosticObject.put(
                "failureClassification",
                diagnostic.failureClassification.ordinal,
            )
            diagnosticObject.put(
                "providerContractName",
                diagnostic.providerContractName,
            )
            diagnosticObject.put("providerMethodName", diagnostic.providerMethodName)
            diagnosticObject.put("failureDetail", diagnostic.failureDetail)
            diagnosticsArray.put(diagnosticObject)
        }
        return diagnosticsArray.toString()
    }

    private fun coordinateFromJson(coordinateObject: JSONObject): GeographicCoordinate =
        GeographicCoordinate(
            latitude = coordinateObject.getDouble("latitude"),
            longitude = coordinateObject.getDouble("longitude"),
        )

    private fun coordinateToJson(coordinate: GeographicCoordinate): JSONObject {
        val coordinateObject = JSONObject()
        coordinateObject.put("latitude", coordinate.latitude)
        coordinateObject.put("longitude", coordinate.longitude)
        return coordinateObject
    }

    private fun coordinatesToJson(
        coordinates: List<GeographicCoordinate>,
    ): JSONArray {
        val coordinatesArray = JSONArray()
        coordinates.forEach { coordinate ->
            coordinatesArray.put(coordinateToJson(coordinate))
        }
        return coordinatesArray
    }

    private fun boundingBoxToJson(boundingBox: BoundingBox): JSONObject {
        val boundingBoxObject = JSONObject()
        boundingBoxObject.put("southWest", coordinateToJson(boundingBox.southWest))
        boundingBoxObject.put("northEast", coordinateToJson(boundingBox.northEast))
        return boundingBoxObject
    }

    private fun routeLegsToJson(legs: List<RouteLeg>): JSONArray {
        val legsArray = JSONArray()
        legs.forEach { leg ->
            val legObject = JSONObject()
            legObject.put("distanceMeters", leg.distanceMeters)
            legObject.put("durationSeconds", leg.durationSeconds)
            legObject.put("steps", routeStepsToJson(leg.steps))
            legsArray.put(legObject)
        }
        return legsArray
    }

    private fun routeStepsToJson(steps: List<RouteStep>): JSONArray {
        val stepsArray = JSONArray()
        steps.forEach { step ->
            val stepObject = JSONObject()
            stepObject.put("distanceMeters", step.distanceMeters)
            stepObject.put("durationSeconds", step.durationSeconds)
            stepObject.put("maneuverType", step.maneuverType)
            stepObject.put("instructionDisplayText", step.instructionDisplayText)
            stepObject.put("instructionSpeechText", step.instructionSpeechText)
            stepObject.put("shape", coordinatesToJson(step.shape))
            stepsArray.put(stepObject)
        }
        return stepsArray
    }

    private fun optionalCoordinateList(
        containerObject: JSONObject,
        fieldName: String,
    ): List<GeographicCoordinate>? {
        if (containerObject.isNull(fieldName) || !containerObject.has(fieldName)) {
            return null
        }
        val coordinateArray = containerObject.getJSONArray(fieldName)
        return (0 until coordinateArray.length()).map { coordinateIndex ->
            coordinateFromJson(coordinateArray.getJSONObject(coordinateIndex))
        }
    }

    private fun optionalColor(
        containerObject: JSONObject,
        fieldName: String,
    ): OverlayColor? {
        if (containerObject.isNull(fieldName) || !containerObject.has(fieldName)) {
            return null
        }
        val colorObject = containerObject.getJSONObject(fieldName)
        return OverlayColor(
            red = colorObject.getDouble("red"),
            green = colorObject.getDouble("green"),
            blue = colorObject.getDouble("blue"),
            alpha = colorObject.getDouble("alpha"),
        )
    }

    private fun optionalTrafficFlowSegments(
        containerObject: JSONObject,
    ): List<TrafficFlowData>? {
        val fieldName = "trafficFlowSegments"
        if (containerObject.isNull(fieldName) || !containerObject.has(fieldName)) {
            return null
        }
        val segmentArray = containerObject.getJSONArray(fieldName)
        return (0 until segmentArray.length()).map { segmentIndex ->
            val segmentObject = segmentArray.getJSONObject(segmentIndex)
            TrafficFlowData(
                shapeCoordinates = optionalCoordinateList(
                    segmentObject,
                    "shapeCoordinates",
                ) ?: emptyList(),
                currentSpeedKilometersPerHour = optionalDouble(
                    segmentObject,
                    "currentSpeedKilometersPerHour",
                ),
                freeFlowSpeedKilometersPerHour = optionalDouble(
                    segmentObject,
                    "freeFlowSpeedKilometersPerHour",
                ),
                congestionLevel = TrafficFlowCongestionLevel.values()[
                    segmentObject.getInt("congestionLevel")
                ],
            )
        }
    }

    private fun optionalString(containerObject: JSONObject, fieldName: String): String? =
        if (containerObject.has(fieldName) && !containerObject.isNull(fieldName)) {
            containerObject.getString(fieldName)
        } else {
            null
        }

    private fun optionalStringList(
        containerObject: JSONObject,
        fieldName: String,
    ): List<String>? {
        if (!containerObject.has(fieldName) || containerObject.isNull(fieldName)) {
            return null
        }
        val stringArray = containerObject.getJSONArray(fieldName)
        return (0 until stringArray.length()).map { stringIndex ->
            stringArray.getString(stringIndex)
        }
    }

    private fun optionalSourceHeaders(
        containerObject: JSONObject,
        fieldName: String,
    ): List<SourceHeader>? {
        if (!containerObject.has(fieldName) || containerObject.isNull(fieldName)) {
            return null
        }
        val headerArray = containerObject.getJSONArray(fieldName)
        return (0 until headerArray.length()).map { headerIndex ->
            val headerObject = headerArray.getJSONObject(headerIndex)
            SourceHeader(
                name = headerObject.getString("name"),
                value = headerObject.getString("value"),
            )
        }
    }

    private fun optionalDouble(containerObject: JSONObject, fieldName: String): Double? =
        if (containerObject.has(fieldName) && !containerObject.isNull(fieldName)) {
            containerObject.getDouble(fieldName)
        } else {
            null
        }

    private fun optionalLong(containerObject: JSONObject, fieldName: String): Long? =
        if (containerObject.has(fieldName) && !containerObject.isNull(fieldName)) {
            containerObject.getLong(fieldName)
        } else {
            null
        }

    private fun optionalBoolean(containerObject: JSONObject, fieldName: String): Boolean? =
        if (containerObject.has(fieldName) && !containerObject.isNull(fieldName)) {
            containerObject.getBoolean(fieldName)
        } else {
            null
        }
}
