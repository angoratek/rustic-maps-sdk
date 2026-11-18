package com.rusticmaps.sdk.refapprn

import android.content.res.AssetManager
import org.json.JSONObject
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.Route
import uniffi.rustic_maps.TrafficFlowCongestionLevel
import uniffi.rustic_maps.TrafficFlowData

/**
 * The shared demo artifact's native half (docs/ReferenceAppUX.md §9): the
 * `platform/reference-scenes.json` file the Gradle copy task bundles from its
 * single committed source, decoded with the snake_case keys consumed as-is
 * (nothing rekeyed, mirroring the Compose reference application's
 * `ReferenceScenesLoader`). The JavaScript side reads the same file through a
 * Metro `require`, so the bundled demo data cannot drift between the seams.
 *
 * The controller consumes the sections the Kotlin side needs: the artifact's
 * demo position (the plan flow's origin default, §4.2), the overlay scenes'
 * descriptors (§4.1's demo-scene chips over the SDK overlay API), the
 * navigation demo's trace asset name, and the keyless demo places and
 * recorded routes (§4.1's keyless search and plan).
 */
data class RefappScenesArtifact(
    /** The artifact's demo position (the first camera preset, §4.2's origin default). */
    val demoPosition: GeographicCoordinate,
    /** The overlay scenes by presentation order (§4.1's chip carousel rows). */
    val overlayScenes: Map<String, RefappOverlayScene>,
    /** The application-asset file name of the navigation demo's recorded trace. */
    val traceAssetName: String,
    /** The keyless search's demo places, in artifact order. */
    val demoPlaces: List<RefappDemoPlace>,
    /** The keyless plan's recorded routes, one per ordered demo-place pair. */
    val demoRoutes: List<RefappDemoRoute>,
)

/**
 * One keyless demo place of the artifact's `demo_places` section: the
 * consumer name and note the React shell's search offers when no provider
 * credential is applied.
 */
data class RefappDemoPlace(
    val name: String,
    val note: String,
    val latitudeDegrees: Double,
    val longitudeDegrees: Double,
)

/**
 * One recorded route of the artifact's `demo_routes` section: the ordered
 * demo-place pair it answers and the contract's `Route` record the keyless
 * plan applies (never invented geometry).
 */
data class RefappDemoRoute(
    val originName: String,
    val destinationName: String,
    val origin: GeographicCoordinate,
    val destination: GeographicCoordinate,
    val route: Route,
)

/** One overlay scene of the shared artifact, holding exactly one payload form. */
data class RefappOverlayScene(
    val sceneKey: String,
    val coordinates: List<GeographicCoordinate>?,
    val ringCoordinates: List<GeographicCoordinate>?,
    val positions: List<GeographicCoordinate>?,
    val trafficFlowSegments: List<TrafficFlowData>?,
)

/** Loads the bundled shared demo artifact (the Gradle copy task's file). */
object RefappScenesLoader {
    const val REFERENCE_SCENES_ASSET_NAME: String = "reference-scenes.json"
    const val TRACE_ASSET_FILE_SUFFIX: String = ".json"

    fun load(assetManager: AssetManager): RefappScenesArtifact {
        assetManager.open(REFERENCE_SCENES_ASSET_NAME).use { assetStream ->
            val artifactObject = JSONObject(assetStream.readBytes().decodeToString())
            val cameraPresets = artifactObject.getJSONArray("camera_presets")
            val overlaySceneObject = artifactObject.getJSONObject("overlay_scenes")
            val navigationTraceName = artifactObject.getJSONObject("navigation_demo")
                .getJSONObject("simulated_trace")
                .getString("name")
            val overlayScenes = mutableMapOf<String, RefappOverlayScene>()
            for (sceneKey in RefappConstants.OVERLAY_SCENE_KEYS) {
                val sceneObject = overlaySceneObject.optJSONObject(sceneKey) ?: continue
                overlayScenes[sceneKey] = RefappOverlayScene(
                    sceneKey = sceneKey,
                    coordinates = sceneObject.optJSONArray("coordinates")?.map(::decodeCoordinate),
                    ringCoordinates = sceneObject.optJSONArray("ring_coordinates")?.map(::decodeCoordinate),
                    positions = sceneObject.optJSONArray("positions")?.map(::decodeCoordinate),
                    trafficFlowSegments = sceneObject.optJSONArray("segments")?.map(::decodeTrafficSegment),
                )
            }
            return RefappScenesArtifact(
                demoPosition = GeographicCoordinate(
                    latitude = cameraPresets.getJSONObject(0).getDouble("latitude_degrees"),
                    longitude = cameraPresets.getJSONObject(0).getDouble("longitude_degrees"),
                ),
                overlayScenes = overlayScenes,
                traceAssetName = navigationTraceName + TRACE_ASSET_FILE_SUFFIX,
                demoPlaces = artifactObject.getJSONArray("demo_places").map { placeObject ->
                    RefappDemoPlace(
                        name = placeObject.getString("name"),
                        note = placeObject.getString("note"),
                        latitudeDegrees = placeObject.getDouble("latitude_degrees"),
                        longitudeDegrees = placeObject.getDouble("longitude_degrees"),
                    )
                },
                demoRoutes = artifactObject.getJSONArray("demo_routes").map { routeObject ->
                    RefappDemoRoute(
                        originName = routeObject.getString("origin_name"),
                        destinationName = routeObject.getString("destination_name"),
                        origin = decodeRouteCoordinate(routeObject.getJSONObject("origin")),
                        destination = decodeRouteCoordinate(
                            routeObject.getJSONObject("destination"),
                        ),
                        route = RefappTraceLoader.decodeRoute(
                            routeObject.getJSONObject("route"),
                        ),
                    )
                },
            )
        }
    }

    private fun decodeCoordinate(coordinateObject: JSONObject): GeographicCoordinate =
        GeographicCoordinate(
            latitude = coordinateObject.getDouble("latitude_degrees"),
            longitude = coordinateObject.getDouble("longitude_degrees"),
        )

    /**
     * The recorded-route coordinate keys (`latitude`/`longitude`): the
     * artifact's `demo_routes` geometry uses the contract fixture's own
     * spelling, unlike `demo_places`' degrees-suffixed keys.
     */
    private fun decodeRouteCoordinate(coordinateObject: JSONObject): GeographicCoordinate =
        GeographicCoordinate(
            latitude = coordinateObject.getDouble("latitude"),
            longitude = coordinateObject.getDouble("longitude"),
        )

    /**
     * Decodes one traffic segment: the congestion-level label matching is
     * normalized (the Compose application's helper) so the artifact's
     * `FreeFlow` spelling meets the contract enum's `FREE_FLOW` spelling.
     */
    private fun decodeTrafficSegment(segmentObject: JSONObject): TrafficFlowData =
        TrafficFlowData(
            shapeCoordinates = segmentObject.getJSONArray("shape_coordinates")
                .map(::decodeCoordinate),
            currentSpeedKilometersPerHour = null,
            freeFlowSpeedKilometersPerHour = null,
            congestionLevel = congestionLevelOf(segmentObject.getString("congestion_level")),
        )
}

/**
 * Normalizes a congestion label for matching (the Compose application's
 * helper): the artifact writes `FreeFlow` while the contract enum writes
 * `FREE_FLOW`, so separators (`_`, `-`) and case are stripped from both sides
 * before the match; an unknown label refuses loudly (never a guess).
 */
internal fun congestionLevelOf(congestionLevelText: String): TrafficFlowCongestionLevel =
    TrafficFlowCongestionLevel.entries.firstOrNull { congestionLevel ->
        normalizedCongestionLevelText(congestionLevel.name) ==
            normalizedCongestionLevelText(congestionLevelText)
    } ?: throw IllegalArgumentException(
        "traffic congestion level '$congestionLevelText' matches no known level",
    )

private fun normalizedCongestionLevelText(congestionLevelText: String): String =
    congestionLevelText.lowercase().replace("_", "").replace("-", "")