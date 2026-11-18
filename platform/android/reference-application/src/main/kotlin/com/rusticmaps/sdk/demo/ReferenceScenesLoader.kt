package com.rusticmaps.sdk.demo

import android.content.res.AssetManager
import org.json.JSONArray
import org.json.JSONObject
import uniffi.rustic_maps.BoundingBox
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.RasterImageFormat
import uniffi.rustic_maps.RasterTileResolution
import uniffi.rustic_maps.RegionDefinition
import uniffi.rustic_maps.Route
import uniffi.rustic_maps.RouteLeg
import uniffi.rustic_maps.RouteStep

/**
 * Model of the language-neutral demo artifact `platform/reference-scenes.json`
 * (`docs/ReferenceAppUX.md` §9): the demo content every reference application
 * loads so all three present identical demo data. Field names mirror the
 * artifact's snake_case keys one to one.
 */
data class CameraPreset(
    val identifier: String,
    val label: String,
    val latitudeDegrees: Double,
    val longitudeDegrees: Double,
    val zoomLevel: Double,
    val bearingDegrees: Double,
    val pitchDegrees: Double,
)

/** One scene overlay coordinate of the artifact (`latitude_degrees`/`longitude_degrees`). */
data class SceneCoordinate(
    val latitudeDegrees: Double,
    val longitudeDegrees: Double,
)

/** One traffic-flow segment of the artifact: shape plus congestion classification. */
data class SceneTrafficFlowSegment(
    val shapeCoordinates: List<SceneCoordinate>,
    val congestionLevel: String,
)

/** One overlay scene together with the artifact key it is addressed by. */
data class OverlaySceneRow(
    /** The scene key the artifact's `overlay_scenes` object carries. */
    val sceneKey: String,
    /** The scene's label, description and geometry. */
    val scene: OverlayScene,
)

/** The four demo overlay scenes of the artifact. */
data class OverlayScenes(
    val route: OverlayScene,
    val polygon: OverlayScene,
    val markers: OverlayScene,
    val traffic: OverlayScene,
    /**
     * Every scene keyed by the artifact's own key, in artifact order. The map
     * layer panel renders its switches from this list, so no scene name is
     * copied into a shell's source.
     */
    val rows: List<OverlaySceneRow>,
)

/** A single overlay scene: label, description, and its geometry. */
data class OverlayScene(
    val label: String,
    val description: String,
    val coordinates: List<SceneCoordinate>?,
    val ringCoordinates: List<SceneCoordinate>?,
    val positions: List<SceneCoordinate>?,
    val trafficFlowSegments: List<SceneTrafficFlowSegment>?,
)

/**
 * One recorded demo plan of the artifact's `demo_plans` section: the
 * consumer-facing label and info-affordance description live beside the
 * trace the plan replays, so every reference application offers the same
 * rows without copying the copy.
 */
data class ReferenceSceneDemoPlan(
    val scenarioName: String,
    val traceAssetName: String,
    val label: String,
    val description: String,
)

/**
 * One keyless demo place of the artifact's `demo_places` section: the
 * consumer name and note the search view offers when no provider credential
 * is applied, plus its coordinate.
 */
data class ReferenceSceneDemoPlace(
    val name: String,
    val note: String,
    val latitudeDegrees: Double,
    val longitudeDegrees: Double,
)

/**
 * One recorded route of the artifact's `demo_routes` section: the ordered
 * pair it answers (by the demo places' names) and the contract's `Route`
 * record the keyless plan presents — the same record shape a live planning
 * call returns, so the app draws and drives it identically.
 */
data class ReferenceSceneDemoRoute(
    val originName: String,
    val destinationName: String,
    val origin: SceneCoordinate,
    val destination: SceneCoordinate,
    val route: Route,
)

/**
 * One demo offline region of the artifact's `offline_regions` section: the
 * contract's [RegionDefinition] the offline card downloads plus the artifact's
 * own note describing the demo values.
 */
data class ReferenceSceneOfflineRegion(
    val region: RegionDefinition,
    val note: String,
)

/** The demo navigation description of the artifact's `navigation_demo` section. */
data class NavigationDemo(
    val traceName: String,
    val traceAssetName: String,
    val sampleIntervalSeconds: Double,
    val provenance: String,
)

/** The whole parsed artifact. */
data class ReferenceScenesArtifact(
    val cameraPresets: List<CameraPreset>,
    val overlayScenes: OverlayScenes,
    val navigationDemo: NavigationDemo,
    val demoPlans: List<ReferenceSceneDemoPlan>,
    /** The keyless search's demo places, in artifact order. */
    val demoPlaces: List<ReferenceSceneDemoPlace>,
    /** The keyless plan's recorded routes, one per ordered demo-place pair. */
    val demoRoutes: List<ReferenceSceneDemoRoute>,
    /** The offline card's demo regions, in artifact order. */
    val offlineRegions: List<ReferenceSceneOfflineRegion>,
)

/**
 * Decodes the shared demo artifact. The repository file
 * `platform/reference-scenes.json` is copied into the application assets at
 * build time (a Gradle task, never a checked-in duplicate), so the data
 * cannot drift from the single source the web refapp loads.
 */
object ReferenceScenesLoader {
    /** Asset name the copy task writes the artifact to inside the application. */
    const val ARTIFACT_ASSET_NAME: String = "reference-scenes.json"

    /** Loads and decodes the artifact from the application's assets. */
    fun load(assets: AssetManager): ReferenceScenesArtifact {
        assets.open(ARTIFACT_ASSET_NAME).use { stream ->
            val artifactText = stream.readBytes().decodeToString()
            return decode(artifactText)
        }
    }

    /** Decodes the artifact's JSON text (separate from loading for tests). */
    fun decode(artifactText: String): ReferenceScenesArtifact {
        val root = JSONObject(artifactText)
        val presets = root.getJSONArray("camera_presets").map { presetObject ->
            presetObject.let { preset ->
                CameraPreset(
                    identifier = preset.getString("id"),
                    label = preset.getString("label"),
                    latitudeDegrees = preset.getDouble("latitude_degrees"),
                    longitudeDegrees = preset.getDouble("longitude_degrees"),
                    zoomLevel = preset.getDouble("zoom_level"),
                    bearingDegrees = preset.getDouble("bearing_degrees"),
                    pitchDegrees = preset.getDouble("pitch_degrees"),
                )
            }
        }
        return ReferenceScenesArtifact(
            cameraPresets = presets,
            overlayScenes = decodeOverlayScenes(root.getJSONObject("overlay_scenes")),
            navigationDemo = decodeNavigationDemo(root.getJSONObject("navigation_demo")),
            demoPlans = decodeDemoPlans(root.getJSONArray("demo_plans")),
            demoPlaces = decodeDemoPlaces(root.getJSONArray("demo_places")),
            demoRoutes = decodeDemoRoutes(root.getJSONArray("demo_routes")),
            offlineRegions = decodeOfflineRegions(root.getJSONArray("offline_regions")),
        )
    }

    /**
     * Decodes the artifact's offline regions into the contract's
     * [RegionDefinition]: the card passes the definition straight to the
     * SDK's region-download members, so the demo data cannot drift into a
     * second app-side shape.
     */
    private fun decodeOfflineRegions(regionsArray: JSONArray): List<ReferenceSceneOfflineRegion> =
        regionsArray.map { regionObject ->
            val boundingBoxObject = regionObject.getJSONObject("bounding_box")
            ReferenceSceneOfflineRegion(
                region = RegionDefinition(
                    regionId = regionObject.getString("region_id"),
                    displayName = regionObject.getString("display_name"),
                    boundingBox = BoundingBox(
                        southWest = decodePlainGeographicCoordinate(
                            boundingBoxObject.getJSONObject("south_west"),
                        ),
                        northEast = decodePlainGeographicCoordinate(
                            boundingBoxObject.getJSONObject("north_east"),
                        ),
                    ),
                    minimumZoomLevel = regionObject.getInt("minimum_zoom_level").toUByte(),
                    maximumZoomLevel = regionObject.getInt("maximum_zoom_level").toUByte(),
                    imageFormat = rasterImageFormatFromArtifact(
                        regionObject.getString("image_format"),
                    ),
                    tileResolution = rasterTileResolutionFromArtifact(
                        regionObject.getString("tile_resolution"),
                    ),
                    timeToLiveSeconds = if (regionObject.has("time_to_live_seconds")) {
                        regionObject.getLong("time_to_live_seconds").toULong()
                    } else {
                        null
                    },
                ),
                note = regionObject.optString("note"),
            )
        }

    /** The artifact's image-format spelling mapped onto the contract enum. */
    private fun rasterImageFormatFromArtifact(value: String): RasterImageFormat = when (value) {
        "Png" -> RasterImageFormat.PNG
        "Jpeg" -> RasterImageFormat.JPEG
        else -> throw IllegalArgumentException("unsupported raster image format '$value'")
    }

    /** The artifact's tile-resolution spelling mapped onto the contract enum. */
    private fun rasterTileResolutionFromArtifact(value: String): RasterTileResolution =
        when (value) {
            "Standard" -> RasterTileResolution.STANDARD
            "Retina" -> RasterTileResolution.RETINA
            else -> throw IllegalArgumentException("unsupported raster tile resolution '$value'")
        }

    private fun decodeDemoPlaces(placesArray: JSONArray): List<ReferenceSceneDemoPlace> =
        placesArray.map { placeObject ->
            ReferenceSceneDemoPlace(
                name = placeObject.getString("name"),
                note = placeObject.getString("note"),
                latitudeDegrees = placeObject.getDouble("latitude_degrees"),
                longitudeDegrees = placeObject.getDouble("longitude_degrees"),
            )
        }

    private fun decodeDemoRoutes(routesArray: JSONArray): List<ReferenceSceneDemoRoute> =
        routesArray.map { routeObject ->
            ReferenceSceneDemoRoute(
                originName = routeObject.getString("origin_name"),
                destinationName = routeObject.getString("destination_name"),
                origin = decodePlainCoordinate(routeObject.getJSONObject("origin")),
                destination = decodePlainCoordinate(routeObject.getJSONObject("destination")),
                route = decodeRoute(routeObject.getJSONObject("route")),
            )
        }

    /**
     * Decodes one recorded route into the contract's `Route` record (the
     * artifact's snake_case keys mirror the record one to one). The recorded
     * geometry is the artifact's data; no provider call is involved.
     */
    private fun decodeRoute(routeObject: JSONObject): Route {
        val boundingBoxObject = routeObject.getJSONObject("bounding_box")
        return Route(
            distanceMeters = routeObject.getDouble("distance_meters"),
            durationSeconds = routeObject.getDouble("duration_seconds"),
            boundingBox = BoundingBox(
                southWest = decodePlainGeographicCoordinate(
                    boundingBoxObject.getJSONObject("south_west"),
                ),
                northEast = decodePlainGeographicCoordinate(
                    boundingBoxObject.getJSONObject("north_east"),
                ),
            ),
            // The artifact records per-step shapes, so the route-level
            // geometry is empty (the contract's documented shape for it).
            geometry = emptyList(),
            legs = routeObject.getJSONArray("legs").map { legObject ->
                RouteLeg(
                    distanceMeters = legObject.getDouble("distance_meters"),
                    durationSeconds = legObject.getDouble("duration_seconds"),
                    steps = legObject.getJSONArray("steps").map { stepObject ->
                        RouteStep(
                            distanceMeters = stepObject.getDouble("distance_meters"),
                            durationSeconds = stepObject.getDouble("duration_seconds"),
                            maneuverType = stepObject.getString("maneuver_type"),
                            instructionDisplayText =
                                stepObject.getString("instruction_display_text"),
                            instructionSpeechText =
                                stepObject.getString("instruction_speech_text"),
                            shape = stepObject.getJSONArray("shape")
                                .map(::decodePlainGeographicCoordinate),
                        )
                    },
                )
            },
        )
    }

    private fun decodeDemoPlans(plansArray: JSONArray): List<ReferenceSceneDemoPlan> =
        plansArray.map { planObject ->
            ReferenceSceneDemoPlan(
                scenarioName = planObject.getString("scenario"),
                traceAssetName = planObject.getString("fixture_path").substringAfterLast('/'),
                label = planObject.getString("label"),
                description = planObject.getString("description"),
            )
        }

    private fun decodeOverlayScenes(overlayObject: JSONObject): OverlayScenes = OverlayScenes(
        route = decodeOverlayScene(overlayObject.getJSONObject("route")),
        polygon = decodeOverlayScene(overlayObject.getJSONObject("polygon")),
        markers = decodeOverlayScene(overlayObject.getJSONObject("markers")),
        traffic = decodeOverlayScene(overlayObject.getJSONObject("traffic")),
        rows = overlayObject.keys().asSequence().map { sceneKey ->
            OverlaySceneRow(
                sceneKey = sceneKey,
                scene = decodeOverlayScene(overlayObject.getJSONObject(sceneKey)),
            )
        }.toList(),
    )

    private fun decodeOverlayScene(sceneObject: JSONObject): OverlayScene {
        val coordinates: List<SceneCoordinate>? = sceneObject.optJSONArray("coordinates")?.map(::decodeCoordinate)
        val ringCoordinates: List<SceneCoordinate>? = sceneObject.optJSONArray("ring_coordinates")?.map(::decodeCoordinate)
        val positions: List<SceneCoordinate>? = sceneObject.optJSONArray("positions")?.map(::decodeCoordinate)
        val trafficFlowSegments: List<SceneTrafficFlowSegment>? = sceneObject.optJSONArray("segments")?.map { segment ->
            SceneTrafficFlowSegment(
                shapeCoordinates = segment.getJSONArray("shape_coordinates").map(::decodeCoordinate),
                congestionLevel = segment.getString("congestion_level"),
            )
        }
        return OverlayScene(
            label = sceneObject.getString("label"),
            description = sceneObject.getString("description"),
            coordinates = coordinates,
            ringCoordinates = ringCoordinates,
            positions = positions,
            trafficFlowSegments = trafficFlowSegments,
        )
    }

    private fun decodeCoordinate(coordinateObject: JSONObject) = SceneCoordinate(
        latitudeDegrees = coordinateObject.getDouble("latitude_degrees"),
        longitudeDegrees = coordinateObject.getDouble("longitude_degrees"),
    )

    /**
     * The recorded-route coordinate keys (`latitude`/`longitude`): the
     * artifact's `demo_routes` geometry uses the contract fixture's own
     * spelling, unlike `demo_places`' degrees-suffixed keys.
     */
    private fun decodePlainCoordinate(coordinateObject: JSONObject) = SceneCoordinate(
        latitudeDegrees = coordinateObject.getDouble("latitude"),
        longitudeDegrees = coordinateObject.getDouble("longitude"),
    )

    /** The contract's coordinate record for the recorded-route keys. */
    private fun decodePlainGeographicCoordinate(coordinateObject: JSONObject) =
        GeographicCoordinate(
            latitude = coordinateObject.getDouble("latitude"),
            longitude = coordinateObject.getDouble("longitude"),
        )

    private fun decodeNavigationDemo(navigationObject: JSONObject): NavigationDemo {
        val simulatedTrace = navigationObject.getJSONObject("simulated_trace")
        return NavigationDemo(
            traceName = simulatedTrace.getString("name"),
            traceAssetName = simulatedTrace.getString("fixture_path").substringAfterLast('/'),
            sampleIntervalSeconds = simulatedTrace.getDouble("sample_interval_seconds"),
            provenance = simulatedTrace.getString("provenance"),
        )
    }
}

private inline fun <T> JSONArray.map(transform: (JSONObject) -> T): List<T> =
    (0 until length()).map { index -> transform(getJSONObject(index)) }