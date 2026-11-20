// Platform-parity floor (M31): the checks that keep the four targets bound
// to the same contract, the same theme model and the same accessible web
// surface. This is a source-text gate, deliberately narrow: it does not try
// to prove behaviour (the Playwright, Espresso, XCUITest and jest suites do
// that), it fails when a target silently loses a binding path or a documented
// parity fact. It runs in Playwright's `globalSetup` beside the contract-sync
// and provider-name gates, so `npm test` and CI run it before any browser.
//
// What it guards:
//   1. the three UniFFI binding-generation invocations still exist (Kotlin
//      through uniffi-bindgen, Swift through uniffi-bindgen, RN through ubrn);
//   2. the UDL still declares every interface the reference apps consume;
//   3. the wasm façade still exports the capability floor it has reached
//      (camera, SDK root, providers, search, routing, tiles, navigation,
//      diagnostics, vector geometry);
//   4. every surface still implements the three-way Light/Dark/Auto theme
//      with Auto as the default;
//   5. the web map keeps its keyboard/screen-reader contract and the mobile
//      page its ARIA tabs pattern;
//   6. the registry mirror is byte-identical and every fixture it names is
//      committed.
//
// Run directly (no server or browser involved):
//   node --test platform/web/tests/platform_parity_gate.mjs

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { test } from 'node:test';
import { run_reference_flow_checks } from '../../../scripts/check-reference-flows.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');

/**
 * Reads one repository-relative file as UTF-8. An array of paths is read as
 * one text, which is how the split playground controller stays scanned as a
 * single source.
 */
function read(relative_path) {
  const paths = Array.isArray(relative_path) ? relative_path : [relative_path];
  return paths
    .map((one_path) => readFileSync(path.join(REPOSITORY_ROOT, one_path), 'utf8'))
    .join('\n');
}

/**
 * The playground controller's entry and concern modules. The controller is
 * split by concern, so every web fragment is checked against the whole set.
 */
const WEB_CONTROLLER_SOURCES = [
  'platform/web/playground.mjs',
  ...readdirSync(path.join(REPOSITORY_ROOT, 'platform', 'web'))
    .filter((file_name) => /^playground_.*\.mjs$/.test(file_name))
    .sort()
    .map((file_name) => `platform/web/${file_name}`),
];

/** Fails with `message` when `condition` is falsy. */
function assert(condition, message) {
  if (!condition) throw new Error(message);
}

/** The binding-generation paths each native target must keep. */
const REQUIRED_BINDING_PATHS = [
  {
    source: 'scripts/build-android.sh',
    fragments: ['uniffi-bindgen', '--language kotlin'],
    description: 'Android generates its Kotlin bindings from the UDL',
  },
  {
    source: 'scripts/build-ios.sh',
    fragments: ['uniffi-bindgen', '--language swift'],
    description: 'iOS generates its Swift bindings from the UDL',
  },
  {
    // The two ubrn invocations live in the shared generator so the app build
    // and the JS gate (jest/typecheck) regenerate the same sources; the app
    // build script must delegate to it.
    source: 'scripts/generate-rn-bindings.sh',
    fragments: ['generate jsi bindings', 'generate jsi turbo-module'],
    description: 'React Native generates its JSI bindings and Turbo Module',
  },
  {
    source: 'scripts/build-rn.sh',
    fragments: ['generate-rn-bindings.sh'],
    description: 'the React Native app build uses the shared bindings generator',
  },
];

/** The contract interfaces every platform story depends on. */
const REQUIRED_CONTRACT_INTERFACES = [
  'MapCamera',
  'MapSdk',
  'MapView',
  'Navigator',
  'RouteSimulator',
  'SearchEngine',
  'RoutingEngine',
  'OfflineManager',
  'Diagnostics',
  'GeocodingProvider',
  'RasterTileProvider',
  'NavigationListener',
  'PositionController',
  'SdkDiagnosticEventListener',
];

/**
 * The hand-written browser surface the wasm-bindings crate must keep. The
 * generated contract package supplies the SDK root and every provider,
 * navigation, positioning and diagnostics member; this floor guards only the
 * canvas classes and the browser-only seams the contract cannot express, so a
 * refactor cannot silently drop one.
 */
const REQUIRED_WASM_EXPORTS = [
  // Camera pose algebra and the canvas class.
  'MapCamera',
  'BrowserMapView',
  // Canvas rendering and the vector base.
  'vectorTileGeographicBounds',
  'setVectorTileSource',
  'setProviderVectorTileSource',
  'setProviderVectorTileSourceFromDocument',
  'clearVectorTileSource',
  'vectorTileSourceStatus',
  'reloadTileSource',
  // Browser-only seams the contract has no member for: the fixture endpoint
  // override, the Clear act and its shared-slot companion, and the keyless
  // URL-template raster fallback.
  'setProviderBaseUrl',
  'clearProviderRegistrations',
  'clearSharedProviderSlots',
  'registerUrlTemplateRasterTileSource',
  'clearUrlTemplateRasterTileSource',
];

/** The theme model every surface must implement (§3, owner ruling). */
const THEME_MODEL_SOURCES = [
  {
    source:
      'platform/android/reference-application/src/main/kotlin/com/rusticmaps/sdk/referenceapplication/ThemeChoice.kt',
    fragments: ['AUTO("auto")', '?: AUTO'],
    description: 'Android declares Auto and defaults a never-chosen device to it',
  },
  {
    source: 'platform/ios/reference-application/Sources/ThemeChoice.swift',
    fragments: ['case auto', '?? .auto'],
    description: 'iOS declares Auto and defaults a never-chosen device to it',
  },
  {
    source: 'platform/rn/app-shell/src/theme.ts',
    fragments: ["defaultThemeChoice: ThemeChoice = 'auto'"],
    description: 'React Native declares Auto as the default choice',
  },
  {
    source: WEB_CONTROLLER_SOURCES,
    fragments: [
      "THEME_CHOICE_NAMES = Object.freeze(['light', 'dark', 'auto'])",
      "theme_choice = 'auto'",
    ],
    description: 'the browser demo offers all three choices and defaults to Auto',
  },
];

/** The web accessibility contract the page and the mobile page must keep. */
const ACCESSIBILITY_MARKUP = [
  {
    source: 'platform/web/index.html',
    fragments: [
      'class="skip-link"',
      'id="map-overlay"',
      'role="application"',
      'aria-keyshortcuts="',
      'aria-describedby="map-keyboard-instructions"',
      'id="map-keyboard-instructions"',
      'id="map-status"',
    ],
    description: 'the map is a labelled, keyboard-operable focus stop with a live status',
  },
  {
    source: 'platform/web/mobile-demo/index.html',
    fragments: [
      'role="tablist"',
      'role="tabpanel"',
      'aria-controls="mobile-demo-stage"',
      'tabindex="-1"',
    ],
    description: 'the mobile page follows the ARIA tabs pattern',
  },
];

/** Each target's device-positioning path (M33): the source helper or bridge
 * that feeds the contract's `PositionController`. */
const POSITIONING_SOURCES = [
  {
    source:
      'platform/android/maps-sdk/src/main/kotlin/com/rusticmaps/sdk/DeviceLocationSource.kt',
    fragments: ['LocationManager', 'requestLocationUpdates', 'feedFix', 'sourceFailed'],
    description: 'Android feeds the contract controller from LocationManager',
  },
  {
    source: 'platform/ios/Sources/MapNativeCoreUI/DeviceLocationSource.swift',
    fragments: ['CLLocationManager', 'feedFix', 'sourceFailed'],
    description: 'iOS feeds the contract controller from Core Location',
  },
  {
    source: 'platform/rn/src/MapsSdk.ts',
    fragments: ['startDeviceLocation', 'latestDeviceFix', 'deviceLocationSourceStatus'],
    description: 'React Native exposes the positioning surface on the curated SDK',
  },
  {
    source: WEB_CONTROLLER_SOURCES,
    fragments: ['device-location-control', 'feedFix', 'mapDeviceLocationState'],
    description: 'the browser feeds the controller from the Geolocation API',
  },
];

/** Device-driven navigation: the Location group's second control on every
 * target drives the planned route's guidance from the device source's fixes
 * instead of the recorded trace. */
const DEVICE_NAVIGATION_SOURCES = [
  {
    source: WEB_CONTROLLER_SOURCES,
    fragments: [
      'device-navigation-control',
      'start_device_navigation',
      'DEVICE_NAVIGATION_PLAN_FIRST_TEXT',
    ],
    description: 'the browser demo navigates the planned route from device fixes',
  },
  {
    source:
      'platform/android/reference-application/src/main/kotlin/com/rusticmaps/sdk/referenceapplication/ReferenceApplicationState.kt',
    fragments: [
      'startDeviceNavigation',
      'deliverDeviceFix',
      'DEVICE_NAVIGATION_PLAN_FIRST_TEXT',
    ],
    description: 'the Android reference app navigates the planned route from device fixes',
  },
  {
    source: 'platform/ios/reference-application/Sources/ReferenceApplicationModel.swift',
    fragments: [
      'startDeviceNavigation',
      'applyDevicePositionFix',
      'deviceNavigationPlanFirstText',
    ],
    description: 'the iOS reference app navigates the planned route from device fixes',
  },
  {
    source: 'platform/rn/app-shell/App.tsx',
    fragments: [
      'device-navigation-control',
      'startDeviceNavigation',
      'deliverDeviceNavigationFix',
    ],
    description: 'the React Native reference app navigates the planned route from device fixes',
  },
];

/** The hand-written view seams every target ships must expose the
 * provider-document style install the reference apps select through; a
 * refactor that drops one turns a working base map into a silent no-op. */
const PROVIDER_DOCUMENT_VIEW_SOURCES = [
  'platform/android/maps-sdk/src/main/kotlin/com/rusticmaps/sdk/MapView.kt',
  'platform/ios/Sources/MapNativeCoreUI/MapView.swift',
  'platform/rn/src/MapView.tsx',
  'platform/rn/ios/MapsSdkRusticMapView.swift',
  'platform/rn/ios/MapsSdkMapHostView.swift',
  'platform/rn/ios/MapsSdkNativeModule.mm',
  'platform/rn/android/src/main/java/com/mapssdkrn/MapsSdkMapHostView.kt',
  'platform/rn/android/src/main/java/com/mapssdkrn/MapsSdkNativeModule.kt',
];

/** Asserts every view seam carries the provider-document member. */
function assert_provider_document_view_member_present() {
  for (const source of PROVIDER_DOCUMENT_VIEW_SOURCES) {
    assert(
      read(source).includes('setProviderVectorTileSourceFromDocument'),
      `The view seam ${source} no longer carries the provider-document member, `
        + 'so the reference apps cannot install a provider-published style.',
    );
  }
}

/** The on-map layer control and device-location affordances the browser demo
 * carries: the shared identifiers are the contract the other shells must
 * adopt, so the web source is checked here until they do. */
const MAP_LAYERS_SOURCES = [
  {
    source: 'platform/web/index.html',
    fragments: [
      'id="map-layers-control"',
      'id="map-layers-panel"',
      'id="device-location-status"',
      'id="device-location-stop"',
      'id="origin-my-location"',
      'id="destination-my-location"',
      'id="attribution-notice" aria-live="polite"',
    ],
    description: 'the browser maps layers panel and route-field chips keep their identifiers',
  },
  {
    source: 'platform/web/map_layers.mjs',
    fragments: [
      "'map-layers-control'",
      "'map-layer-base-'",
      "'map-layer-style-'",
      "'map-layer-scene-'",
      "'map-layer-traffic'",
      'traffic_flow',
    ],
    description: 'the shared layer identifiers and capability name stay declared once',
  },
  {
    source: WEB_CONTROLLER_SOURCES,
    fragments: [
      'mapTrafficLayerState',
      'traffic_flow_overlay_descriptor',
      'set_map_layers_panel_open',
      'start_my_location_chip_fill',
    ],
    description: 'the browser drives the traffic overlay and the route-field chips',
  },
];

/** The Directions control identifiers every route-planning surface shares
 * (PRD.md §6 item 36). The browser demo declares them and
 * routes with the chosen values; a shell that drops one loses the parity the
 * design names. */
const DIRECTIONS_CONTROL_SOURCES = [
  {
    source: 'platform/web/index.html',
    fragments: [
      'id="trip-directions"',
      'id="directions-mode-car"',
      'id="directions-mode-walk"',
      'id="directions-strategy-fastest"',
      'id="directions-strategy-shortest"',
    ],
    description: 'the browser Directions groups keep their shared identifiers',
  },
  {
    source: WEB_CONTROLLER_SOURCES,
    fragments: [
      'directionsTransportationMode',
      'directionsRouteStrategy',
      'current_directions_route_options',
      'calculateRouteWithOptions',
    ],
    description: 'the browser sends the remembered Directions choices on every plan',
  },
];

/** The one worked example's shared identifiers (M25): the same names in the
 * Android, iOS and React Native scenario modules. */
/**
 * The capability names a provider descriptor may declare. They match the
 * contract's `ProviderContract` values, so a descriptor that names anything
 * else cannot be offered by a settings surface.
 */
const CAPABILITY_NAMES = new Set([
  'geocoding',
  'routing',
  'distance_matrix',
  'place_search',
  'traffic_incidents',
  'traffic_flow',
  'elevation',
  'raster_tiles',
  'vector_tiles',
  'offline_routing',
]);

const WORKED_EXAMPLE_SOURCES = [
  'platform/android/reference-application/src/main/kotlin/com/rusticmaps/sdk/referenceapplication/walkthrough/WalkthroughScenario.kt',
  'platform/ios/reference-application/Sources/WalkthroughScenario.swift',
  'platform/rn/app-shell/src/walkthroughScenario.ts',
  'platform/web/walkthroughScenario.mjs',
];

/** The identifiers every platform's worked example must use. */
const WORKED_EXAMPLE_IDENTIFIERS = [
  'mapSdk',
  'mapView',
  'mapCamera',
  'originSearchQuery',
  'destinationSearchQuery',
  'originPlace',
  'destinationPlace',
  'routeOptions',
  'plannedRoute',
  'routeOverlay',
];

/** Vocabulary the tutorial rewrite retired; its return is a regression. */
const RETIRED_TUTORIAL_VOCABULARY = [
  'mapSurface',
  'MapSurfaceView',
  'ANativeWindow',
  'JNI shim',
  'attach/resize/detach',
];

/** Asserts each file contains every fragment, with one useful message. */
function assert_fragments_present({ source, fragments, description }) {
  const text = read(source);
  const source_label = Array.isArray(source) ? source.join(' + ') : source;
  for (const fragment of fragments) {
    assert(
      text.includes(fragment),
      `${description}: ${source_label} no longer contains "${fragment}".`,
    );
  }
}

/** Asserts every required contract interface is still declared in the UDL. */
function assert_contract_interfaces_present() {
  const udl = read('crates/rustic-maps-native-core/src/rustic_maps.udl');
  for (const interface_name of REQUIRED_CONTRACT_INTERFACES) {
    assert(
      new RegExp(`(?:^|\\n)\\s*(?:\\[[^\\]]*\\]\\s+)?interface\\s+${interface_name}\\b`)
        .test(udl),
      `The UDL contract no longer declares the ${interface_name} interface.`,
    );
  }
}

/** Asserts the wasm Rust sources still name every export in the floor. */
function assert_wasm_capability_floor_present() {
  const wasm_source_directory = path.join(
    REPOSITORY_ROOT,
    'crates',
    'rustic-maps-web-bindings',
    'src',
  );
  const wasm_sources = readdirSync(wasm_source_directory)
    .filter((file_name) => file_name.endsWith('.rs'))
    .map((file_name) => read(`crates/rustic-maps-web-bindings/src/${file_name}`))
    .join('\n');
  for (const export_name of REQUIRED_WASM_EXPORTS) {
    // Functions carry a `js_name`; wasm-bindgen classes are exported under
    // their Rust struct name (some spell it again with `js_name`).
    assert(
      new RegExp(`(?:js_name\\s*=\\s*${export_name}\\b|pub struct ${export_name}\\b)`)
        .test(wasm_sources),
      `The wasm façade no longer exports ${export_name}.`,
    );
  }
}

/** Asserts the registry mirror and every fixture it names exist. */
function assert_registry_and_fixtures_are_committed() {
  const registry_path = 'platform/providers.json';
  const mirror_path =
    'platform/ios/Sources/MapNativeCoreUI/Resources/providers.json';
  assert(
    read(registry_path) === read(mirror_path),
    'The iOS registry mirror has drifted from platform/providers.json.',
  );

  const registry = JSON.parse(read(registry_path));
  for (const descriptor of registry.providers) {
    assert(
      Array.isArray(descriptor.served_contracts)
        && descriptor.served_contracts.length > 0,
      `The provider "${descriptor.provider_key}" declares no served_contracts, `
        + 'so no settings surface can offer it for a capability.',
    );
    for (const served_contract of descriptor.served_contracts) {
      assert(
        CAPABILITY_NAMES.has(served_contract),
        `The provider "${descriptor.provider_key}" declares the unknown capability `
          + `"${served_contract}".`,
      );
    }
    // A contract with no committed fixture is recorded here with its reviewed
    // reason, so a live-only source stays explicit instead of silently
    // uncovered.
    const fixtureless_contracts = descriptor.fixtureless_contracts ?? [];
    assert(
      Array.isArray(fixtureless_contracts),
      `The provider "${descriptor.provider_key}" declares fixtureless_contracts `
        + 'that are not an array.',
    );
    const fixtureless_contract_names = new Set();
    for (const fixtureless_contract of fixtureless_contracts) {
      const contract_name = fixtureless_contract.contract;
      assert(
        typeof contract_name === 'string'
          && descriptor.served_contracts.includes(contract_name),
        `The provider "${descriptor.provider_key}" records a fixtureless `
          + `contract "${contract_name}" it does not declare in served_contracts.`,
      );
      assert(
        typeof fixtureless_contract.reason === 'string'
          && fixtureless_contract.reason.trim() !== '',
        `The provider "${descriptor.provider_key}" records the fixtureless `
          + `contract "${contract_name}" without a reason.`,
      );
      assert(
        !fixtureless_contract_names.has(contract_name),
        `The provider "${descriptor.provider_key}" records the fixtureless `
          + `contract "${contract_name}" more than once.`,
      );
      fixtureless_contract_names.add(contract_name);
    }
    for (const vector_style of descriptor.vector_styles ?? []) {
      if (typeof vector_style.document_url === 'string') {
        assert(
          vector_style.document_url.startsWith('https://'),
          `The provider "${descriptor.provider_key}" names the style document `
            + `"${vector_style.style_id}" at a non-https address, so a shipped `
            + 'descriptor could send a style fetch over plaintext.',
        );
        assert(
          !vector_style.document_url.includes('?'),
          `The provider "${descriptor.provider_key}" names the style document `
            + `"${vector_style.style_id}" with a query, so a credential could `
            + 'enter the shared registry.',
        );
      }
    }
    const route_contract_names = new Set();
    for (const fixture_route of descriptor.fixture_routes ?? []) {
      assert(
        typeof fixture_route.contract === 'string'
          && descriptor.served_contracts.includes(fixture_route.contract),
        `The registry route "${fixture_route.purpose}" on `
          + `"${descriptor.provider_key}" must name a served contract the `
          + 'descriptor declares.',
      );
      assert(
        !fixtureless_contract_names.has(fixture_route.contract),
        `The provider "${descriptor.provider_key}" declares both a fixture route `
          + `and a fixtureless record for "${fixture_route.contract}".`,
      );
      route_contract_names.add(fixture_route.contract);
      if (typeof fixture_route.fixture === 'string') {
        assert(
          existsSync(path.join(REPOSITORY_ROOT, fixture_route.fixture)),
          `The registry route "${fixture_route.purpose}" names the missing fixture `
            + `"${fixture_route.fixture}".`,
        );
      }
      if (typeof fixture_route.fixture_directory === 'string') {
        const directory_path = path.join(
          REPOSITORY_ROOT,
          fixture_route.fixture_directory,
        );
        assert(
          existsSync(directory_path) && statSync(directory_path).isDirectory(),
          `The registry route "${fixture_route.purpose}" names the missing fixture `
            + `directory "${fixture_route.fixture_directory}".`,
        );
      }
    }
    for (const served_contract of descriptor.served_contracts) {
      assert(
        route_contract_names.has(served_contract)
          || fixtureless_contract_names.has(served_contract),
        `The provider "${descriptor.provider_key}" declares the served contract `
          + `"${served_contract}" with neither a fixture route nor an explicit `
          + 'fixtureless record.',
      );
    }
  }
}

/** Asserts the four-platform worked example keeps one identifier set. */
function assert_worked_example_identifiers_are_shared() {
  for (const source of WORKED_EXAMPLE_SOURCES) {
    const text = read(source);
    for (const identifier of WORKED_EXAMPLE_IDENTIFIERS) {
      assert(
        new RegExp(`\\b${identifier}\\b`).test(text),
        `The worked example ${source} no longer uses the shared identifier `
          + `"${identifier}".`,
      );
    }
  }
}

/** Asserts the tutorial chapters never teach retired surface vocabulary. */function assert_tutorials_use_the_shipped_vocabulary() {
  const book_directory = path.join(REPOSITORY_ROOT, 'site', 'book-src');
  const chapter_names = readdirSync(book_directory)
    .filter((file_name) => file_name.endsWith('.md'));
  for (const chapter_name of chapter_names) {
    const chapter_text = read(`site/book-src/${chapter_name}`);
    for (const retired_term of RETIRED_TUTORIAL_VOCABULARY) {
      assert(
        !chapter_text.includes(retired_term),
        `The tutorial chapter ${chapter_name} still teaches the retired `
          + `vocabulary "${retired_term}".`,
      );
    }
  }
}

/** Runs every parity-floor check. Throws with the first failure's detail. */
export function run_platform_parity_checks() {
  for (const binding_path of REQUIRED_BINDING_PATHS) {
    assert_fragments_present({
      source: binding_path.source,
      fragments: binding_path.fragments,
      description: binding_path.description,
    });
  }
  assert_contract_interfaces_present();
  assert_wasm_capability_floor_present();
  for (const theme_source of THEME_MODEL_SOURCES) {
    assert_fragments_present(theme_source);
  }
  for (const markup_source of ACCESSIBILITY_MARKUP) {
    assert_fragments_present(markup_source);
  }
  for (const positioning_source of POSITIONING_SOURCES) {
    assert_fragments_present(positioning_source);
  }
  for (const navigation_source of DEVICE_NAVIGATION_SOURCES) {
    assert_fragments_present(navigation_source);
  }
  for (const map_layers_source of MAP_LAYERS_SOURCES) {
    assert_fragments_present(map_layers_source);
  }
  for (const directions_source of DIRECTIONS_CONTROL_SOURCES) {
    assert_fragments_present(directions_source);
  }
  assert_registry_and_fixtures_are_committed();
  assert_provider_document_view_member_present();
  assert_worked_example_identifiers_are_shared();
  assert_tutorials_use_the_shipped_vocabulary();
  // The reference-flow schema, its copy IDs, the keyless routing demo
  // addresses and every declared platform hook. Runs from the same floor so a
  // broken flow cannot reach a browser test.
  run_reference_flow_checks(REPOSITORY_ROOT);
}

/** The `node --test` suite name of the parity-floor assertions. */
const PLATFORM_PARITY_TEST_NAME =
  'every target keeps its binding path, theme model, wasm floor and web accessibility';

// Run the suite only when this file is the entry point; the Playwright
// `globalSetup` imports the exported runner instead.
const RUNNING_AS_ENTRY = process.argv[1] !== undefined
  && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url;

if (RUNNING_AS_ENTRY) {
  test(PLATFORM_PARITY_TEST_NAME, () => {
    run_platform_parity_checks();
  });
}
