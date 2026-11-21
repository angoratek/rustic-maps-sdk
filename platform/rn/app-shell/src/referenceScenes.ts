/**
 * The shared demo artifact (docs/ReferenceAppUX.md §8): the RN shell consumes
 * `platform/reference-scenes.json` by name through a Metro `require`, exactly
 * the file the SwiftPM bundle, the Android Gradle copy task, and the web
 * loader carry, so the bundled demo data cannot drift. The snake-case keys of
 * the artifact are consumed as-is; nothing is rekeyed on the RN side.
 */
import referenceScenesArtifact from '../../../reference-scenes.json';
import type {
  RegionDefinition,
  RegionImageFormat,
  RegionTileResolution,
} from '@angoratek/rustic-maps-sdk/MapsSdk';
import type { PlaceRow } from './RefappNative';

/** One camera framing row of the artifact's `camera_presets`. */
export interface CameraPreset {
  id: string;
  label: string;
  latitude_degrees: number;
  longitude_degrees: number;
  zoom_level: number;
  bearing_degrees: number;
  pitch_degrees: number;
}

export interface ReferenceScenes {
  artifact: Record<string, unknown>;
  camera_presets: CameraPreset[];
  overlay_scenes: {
    route?: OverlayScene;
    polygon?: OverlayScene;
    markers?: OverlayScene;
    traffic?: OverlayScene;
  };
  navigation_demo: {
    simulated_trace: {
      name: string;
      fixture_path: string;
      sample_interval_seconds: number;
    };
  };
  demo_plans: DemoPlanDefinition[];
  demo_places?: DemoPlace[];
  demo_routes?: DemoRoute[];
  offline_regions?: OfflineRegion[];
}

/**
 * One keyless demo place of the artifact's `demo_places` section: the
 * consumer name and note the plan search offers when no provider credential
 * is applied, plus its coordinate.
 */
export interface DemoPlace {
  name: string;
  note: string;
  latitude_degrees: number;
  longitude_degrees: number;
}

/** One coordinate of the artifact's recorded route geometry. */
export interface DemoRouteCoordinate {
  latitude: number;
  longitude: number;
}

/** One maneuver step of the artifact's recorded route geometry. */
export interface DemoRouteStep {
  distance_meters: number;
  duration_seconds: number;
  maneuver_type: string;
  instruction_display_text: string;
  instruction_speech_text: string;
  shape: DemoRouteCoordinate[];
}

/** One leg of the artifact's recorded route geometry. */
export interface DemoRouteLeg {
  distance_meters: number;
  duration_seconds: number;
  steps: DemoRouteStep[];
}

/** The recorded route's geometry (the contract's `Route` record shapes). */
export interface DemoRouteRecord {
  distance_meters: number;
  duration_seconds: number;
  bounding_box: {
    south_west: DemoRouteCoordinate;
    north_east: DemoRouteCoordinate;
  };
  legs: DemoRouteLeg[];
}

/**
 * One recorded route of the artifact's `demo_routes` section: the ordered
 * demo-place pair it answers (by name) and the recorded geometry the keyless
 * plan presents.
 */
export interface DemoRoute {
  origin_name: string;
  destination_name: string;
  origin: DemoRouteCoordinate;
  destination: DemoRouteCoordinate;
  route: DemoRouteRecord;
}

/**
 * One row of the artifact's `demo_plans`: the consumer-facing label and
 * info-affordance description beside the trace the plan replays (§4.2: the
 * rows name the feature, never the seam).
 */
export interface DemoPlanDefinition {
  scenario: string;
  fixture_path: string;
  label: string;
  description: string;
}

/**
 * One offline region of the artifact's `offline_regions` section: the demo
 * region the offline card offers, with the artifact's snake-case fields.
 */
export interface OfflineRegion {
  region_id: string;
  display_name: string;
  bounding_box: {
    south_west: { latitude: number; longitude: number };
    north_east: { latitude: number; longitude: number };
  };
  minimum_zoom_level: number;
  maximum_zoom_level: number;
  image_format: string;
  tile_resolution: string;
  time_to_live_seconds: number;
  note: string;
}

/**
 * One overlay scene (§9's `overlay_scenes`): exactly one coordinate payload
 * form is set, with the label/description the chip carousel renders; the
 * snake_case keys are the artifact's own.
 */
export interface OverlayScene {
  label: string;
  description: string;
  kind: string;
  coordinates?: Array<{ latitude_degrees: number; longitude_degrees: number }>;
  ring_coordinates?: Array<{ latitude_degrees: number; longitude_degrees: number }>;
  positions?: Array<{ latitude_degrees: number; longitude_degrees: number }>;
  segments?: Array<{
    congestion_level: string;
    shape_coordinates: Array<{ latitude_degrees: number; longitude_degrees: number }>;
  }>;
}

export const referenceScenes: ReferenceScenes =
  referenceScenesArtifact as ReferenceScenes;

export const cameraPresets: CameraPreset[] = referenceScenes.camera_presets;

/** The keyless search's demo places, in artifact order. */
export const demoPlaces: DemoPlace[] = referenceScenes.demo_places ?? [];

/** The keyless plan's recorded routes, one per ordered demo-place pair. */
export const demoRoutes: DemoRoute[] = referenceScenes.demo_routes ?? [];

/** The artifact's offline demo regions, in artifact order. */
export const offlineRegions: OfflineRegion[] =
  referenceScenes.offline_regions ?? [];

/** The image-format ordinal the generated contract's enum names. */
function regionImageFormatOrdinal(imageFormatName: string): RegionImageFormat {
  return imageFormatName.toLowerCase() === 'jpeg' ? 1 : 0;
}

/** The tile-resolution ordinal the generated contract's enum names. */
function regionTileResolutionOrdinal(
  tileResolutionName: string,
): RegionTileResolution {
  return tileResolutionName.toLowerCase() === 'retina' ? 1 : 0;
}

/**
 * The artifact's offline regions as the typed SDK definitions the offline
 * card downloads: the snake-case artifact fields mapped once, here, so the
 * screen never re-keys the shared data.
 */
export const offlineRegionDefinitions: RegionDefinition[] = offlineRegions.map(
  region => ({
    regionId: region.region_id,
    displayName: region.display_name,
    boundingBox: {
      southWest: {
        latitude: region.bounding_box.south_west.latitude,
        longitude: region.bounding_box.south_west.longitude,
      },
      northEast: {
        latitude: region.bounding_box.north_east.latitude,
        longitude: region.bounding_box.north_east.longitude,
      },
    },
    minimumZoomLevel: region.minimum_zoom_level,
    maximumZoomLevel: region.maximum_zoom_level,
    imageFormat: regionImageFormatOrdinal(region.image_format),
    tileResolution: regionTileResolutionOrdinal(region.tile_resolution),
    timeToLiveSeconds: region.time_to_live_seconds,
  }),
);

/**
 * The keyless search (§4.1): filters the artifact's demo places by the typed
 * text, never a provider call and never an invented row. The returned rows
 * carry their note and the demo marker, so the search view can say they are
 * demo places rather than live results.
 */
export function searchDemoPlaces(queryText: string): PlaceRow[] {
  const queryLowercase = queryText.trim().toLowerCase();
  if (queryLowercase === '') {
    return [];
  }
  return demoPlaces
    .filter(demoPlace => demoPlace.name.toLowerCase().includes(queryLowercase))
    .map(demoPlace => ({
      structuredAddressText: demoPlace.name,
      latitudeDegrees: demoPlace.latitude_degrees,
      longitudeDegrees: demoPlace.longitude_degrees,
      qualityText: '',
      noteText: demoPlace.note,
      isDemoPlace: true,
    }));
}

/** The artifact demo place whose name matches the text exactly (case-insensitive). */
export function demoPlaceNamed(placeNameText: string): DemoPlace | null {
  const trimmedText = placeNameText.trim().toLowerCase();
  return demoPlaces.find(demoPlace => demoPlace.name.toLowerCase() === trimmedText) ?? null;
}

/** The artifact's recorded route for the ordered demo-place pair, or null. */
export function recordedDemoRoute(
  originName: string,
  destinationName: string,
): DemoRoute | null {
  return demoRoutes.find(
    demoRoute =>
      demoRoute.origin_name === originName &&
      demoRoute.destination_name === destinationName,
  ) ?? null;
}

/**
 * The overlay scenes in their artifact presentation order (§4.1's chip
 * carousel rows); a scene the artifact omits is skipped (honest, never a
 * guessed placeholder).
 */
export const overlaySceneKeys: string[] = ['route', 'polygon', 'markers', 'traffic'];

export const overlayScenes: Array<{ sceneKey: string; scene: OverlayScene }> =
  overlaySceneKeys
    .map(sceneKey => {
      const scene = referenceScenes.overlay_scenes?.[sceneKey as keyof typeof referenceScenes.overlay_scenes];
      return scene ? { sceneKey, scene } : null;
    })
    .filter((entry): entry is { sceneKey: string; scene: OverlayScene } => entry !== null);

// ---------------------------------------------------------------------------
// The demo-plan picker (§4.1; the Compose reference application's demo plans)
// ---------------------------------------------------------------------------

/**
 * One demo-plan picker row: one recorded walk the guided-walk demo replays
 * through the simulated navigation. The three plans carry exactly the
 * committed trace set the Compose reference application's picker offers.
 */
export interface DemoPlan {
  /** The bundled trace asset the plan replays (the Gradle copy task's file). */
  traceAssetName: string;
  /** Consumer-facing plan name (never fixture paths — §4.2). */
  label: string;
  /** The plan's info affordance description (expanded below the chip row). */
  description: string;
  /** Whether this plan is the shared artifact's own navigation demo trace. */
  isArtifactTrace: boolean;
}

/**
 * The recorded synthetic walks the demo-plan picker offers, in the shared
 * artifact's own order and with its own consumer copy. The plan whose trace
 * the artifact's `simulated_trace` record names is marked as the artifact
 * trace, the picker's default selection (the Compose application's
 * `demoPlans.first { it.isArtifactTrace }` rule).
 */
/** The artifact's navigation demo trace asset, as the native loaders name it. */
const artifactTraceAssetName =
  referenceScenes.navigation_demo.simulated_trace.name + '.json';

export const demoPlans: DemoPlan[] = referenceScenes.demo_plans.map(
  planDefinition => ({
    traceAssetName: planDefinition.fixture_path.split('/').pop() ?? '',
    label: planDefinition.label,
    description: planDefinition.description,
    isArtifactTrace:
      (planDefinition.fixture_path.split('/').pop() ?? '') === artifactTraceAssetName,
  }),
);