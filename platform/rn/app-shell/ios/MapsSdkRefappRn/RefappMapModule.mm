#import "RefappMapModule.h"

#import <React/RCTBridge.h>
#import <React/RCTEventDispatcher.h>
#import <React/RCTLog.h>

// The Swift controller singleton (the app target's generated Swift header).
// The generated Swift header declares the AVSpeech delegate protocol and
// the RN factory superclass — both resolved here for the ObjC++ importer.
#import <AVFAudio/AVFAudio.h>
#import <React-RCTAppDelegate/RCTDefaultReactNativeFactoryDelegate.h>

#import "MapsSdkRefappRn-Swift.h"
// The pod's generated Swift header (Xcode 26 writes it below the pod product
// directory's `Swift Compatibility Header` folder; the shell's wiring adds
// that folder to the header search path).
#import "MapsSdkRn-Swift.h"

namespace {
/// The defaults key of the versioned remembered-choices record.
NSString *const RefappPreferencesKey = @"rusticMapsSdk.referenceAppPreferences";
/// The defaults key a build before the record stored the theme choice in.
NSString *const RefappThemeChoiceKey = @"rusticMapsSdk.themeChoice";

/// The event channel's supported event names (the exact set the controller
/// emits; the React side's `RefappNative.ts` subscribes to the same list).
NSArray<NSString *> *RefappSupportedEventNames() {
  static NSArray<NSString *> *names = @[
    @"RefappReadiness",
    @"RefappErrorReport",
    @"RefappTraceState",
    @"RefappTraceUpdate",
    @"RefappDeviationAlert",
    @"RefappPhaseChange",
    @"RefappPlanState",
    @"RefappCacheState",
    @"RefappFrameStatistics",
    @"RefappSpeechInstruction",
    @"RefappSpeechState",
    @"RefappCameraReadback",
    @"RefappDiagnosticEvent",
  ];
  return names;
}
} // namespace

@implementation RefappMapModule

RCT_EXPORT_MODULE(RefappMap)

- (dispatch_queue_t)methodQueue {
  // The SDK-driving contract's main thread (the Kotlin module's
  // UiThreadUtil hops all land here).
  return dispatch_get_main_queue();
}

- (instancetype)init {
  if (self = [super init]) {
    // The speech engine (the app-owned AVSpeechSynthesizer voice) builds once
    // at module start so its initialization state is honest before the first
    // trip; the ready/unavailable state travels the event channel.
    RefappDemoSession *controller = RefappDemoSession.shared;
    [controller initializeSpeechEngine];
    __weak RefappMapModule *weakSelf = self;
    controller.eventSink = ^(NSString *eventName, NSDictionary *arguments) {
      RefappMapModule *strongSelf = weakSelf;
      if (strongSelf) {
        [strongSelf sendEventWithName:eventName body:arguments];
      }
    };
  }
  return self;
}

- (void)dealloc {
  RefappDemoSession *controller = RefappDemoSession.shared;
  controller.eventSink = nil;
}

+ (BOOL)requiresMainQueueSetup {
  return YES;
}

- (NSArray<NSString *> *)supportedEvents {
  return RefappSupportedEventNames();
}

RCT_EXPORT_METHOD(createSdk) {
  // The package native module owns the one MapSdk; the demo session only
  // loads its shared scene artifact once the SDK is up. The launch-environment
  // automation credential seam (PROVIDER_API_KEY / PROVIDER_KEY) is applied by
  // the package module's own initialize path, never persisted and never
  // logged.
  [RefappDemoSession.shared ensureSdkStarted];
}

/// Applies one runtime provider credential (§3's explicit Apply); the result
/// carries the verbatim refusal detail when the SDK refused it.
RCT_EXPORT_METHOD(setProviderCredential
                  : (NSString *)providerKey credential
                  : (NSString *)credential resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve([RefappDemoSession.shared setProviderCredentialWithProviderKey:providerKey
      credential:credential]);
}

/// The opt-in Keychain store's value (nil when nothing is stored).
RCT_EXPORT_METHOD(getStoredProviderCredential
                  : (NSString *)providerKey resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve([RefappDemoSession.shared getStoredProviderCredentialWithProviderKey:providerKey]);
}

/// Persists the credential behind the Keychain; resolves false on failure.
RCT_EXPORT_METHOD(storeProviderCredential
                  : (NSString *)providerKey credential
                  : (NSString *)credential resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve(@([RefappDemoSession.shared storeProviderCredentialWithProviderKey:providerKey
      credential:credential]));
}

/// Removes the stored credential; resolves false when the store is unavailable.
RCT_EXPORT_METHOD(clearStoredProviderCredential
                  : (NSString *)providerKey resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve(@([RefappDemoSession.shared clearStoredProviderCredentialWithProviderKey:providerKey]));
}

/// The device-remembered theme choice (§3), or nil when this device never chose.
RCT_EXPORT_METHOD(loadThemeChoice
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve([[NSUserDefaults standardUserDefaults]
      stringForKey:RefappThemeChoiceKey]);
}

/// Remembers one theme choice on this device (§3).
RCT_EXPORT_METHOD(storeThemeChoice : (NSString *)themeChoice) {
  [[NSUserDefaults standardUserDefaults] setObject:themeChoice
                                            forKey:RefappThemeChoiceKey];
}

/// The versioned remembered-choices record as JSON, or nil when unset.
RCT_EXPORT_METHOD(loadPreferences
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve([[NSUserDefaults standardUserDefaults]
      stringForKey:RefappPreferencesKey]);
}

/// Remembers the versioned choices record on this device.
RCT_EXPORT_METHOD(storePreferences : (NSString *)preferencesJson) {
  [[NSUserDefaults standardUserDefaults] setObject:preferencesJson
                                            forKey:RefappPreferencesKey];
}

/// Clears the record and the theme choice a build before it stored separately.
RCT_EXPORT_METHOD(clearPreferences) {
  [[NSUserDefaults standardUserDefaults] removeObjectForKey:RefappPreferencesKey];
  [[NSUserDefaults standardUserDefaults] removeObjectForKey:RefappThemeChoiceKey];
}

RCT_EXPORT_METHOD(applyCameraPreset
                  : (double)latitudeDegrees longitudeDegrees
                  : (double)longitudeDegrees zoomLevel
                  : (double)zoomLevel bearingDegrees
                  : (double)bearingDegrees pitchDegrees
                  : (double)pitchDegrees) {
  [RefappDemoSession.shared applyCameraPresetWithLatitudeDegrees:latitudeDegrees
      longitudeDegrees:longitudeDegrees
      zoomLevel:zoomLevel
      bearingDegrees:bearingDegrees
      pitchDegrees:pitchDegrees];
}

/// One zoom control step (§4.2's cluster; the React side repeats on hold).
RCT_EXPORT_METHOD(zoomInControl) {
  [RefappDemoSession.shared zoomInControl];
}

RCT_EXPORT_METHOD(zoomOutControl) {
  [RefappDemoSession.shared zoomOutControl];
}

/// The my-location control (§4.2): centers on the latest fix.
RCT_EXPORT_METHOD(myLocationControl) {
  [RefappDemoSession.shared centerOnLatestLocation];
}

/// The 2D/3D plane toggle (§4.2).
RCT_EXPORT_METHOD(cameraPlaneControl) {
  [RefappDemoSession.shared toggleCameraPlane];
}

/// The compass tap (§2.4): eases the camera north up.
RCT_EXPORT_METHOD(compassControl) {
  [RefappDemoSession.shared easeCameraNorthUp];
}

/// The plan search (§4.1): the result rows resolve through the promise.
RCT_EXPORT_METHOD(searchPlaces
                  : (NSString *)queryText resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  [RefappDemoSession.shared searchPlacesWithQueryText:queryText
      resolve:resolve
      reject:^(NSString *code, NSString *message) {
        reject(code, message, nil);
      }];
}

/// One result row's plan-field assignment; both resolved ends plan the route.
RCT_EXPORT_METHOD(assignSearchResult
                  : (BOOL)assignToOrigin placeNameText
                  : (NSString *)placeNameText latitudeDegrees
                  : (double)latitudeDegrees longitudeDegrees
                  : (double)longitudeDegrees resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  [RefappDemoSession.shared assignSearchResultWithAssignToOrigin:assignToOrigin
      placeNameText:placeNameText
      latitudeDegrees:latitudeDegrees
      longitudeDegrees:longitudeDegrees
      resolve:resolve
      reject:^(NSString *code, NSString *message) {
        reject(code, message, nil);
      }];
}

/// The explicit Plan route action (§4.1): plans between the two assigned
/// plan fields with the Directions choice; a missing field rejects with a
/// reason naming it.
RCT_EXPORT_METHOD(planRoute
                  : (double)transportationModeOrdinal routeStrategyOrdinal
                  : (double)routeStrategyOrdinal resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  [RefappDemoSession.shared
      planRouteWithTransportationModeOrdinal:(NSInteger)transportationModeOrdinal
      routeStrategyOrdinal:(NSInteger)routeStrategyOrdinal
      resolve:resolve
      reject:^(NSString *code, NSString *message) {
        reject(code, message, nil);
      }];
}

/// The keyless plan (§4.1): applies the artifact's recorded route for the
/// ordered demo-place pair; no provider call is involved.
RCT_EXPORT_METHOD(planRecordedDemoRoute
                  : (NSString *)originName destinationName
                  : (NSString *)destinationName resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  [RefappDemoSession.shared planRecordedDemoRouteWithOriginName:originName
      destinationName:destinationName
      resolve:resolve
      reject:^(NSString *code, NSString *message) {
        reject(code, message, nil);
      }];
}

/// The offline card's route control (§2.5 `offline.plan_route_action`): plans
/// over the bundled committed test graph; no provider or network.
RCT_EXPORT_METHOD(planOfflineRouteFromArchive
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  [RefappDemoSession.shared planOfflineRouteFromArchiveWithResolve:resolve
      reject:^(NSString *code, NSString *message) {
        reject(code, message, nil);
      }];
}

/// Whether the running SDK session has a provider credential applied.
RCT_EXPORT_METHOD(isProviderCredentialApplied
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve(@([RefappDemoSession.shared isProviderCredentialApplied]));
}

/// One demo-scene chip's selection over the SDK overlay API (§4.1).
RCT_EXPORT_METHOD(setSceneOverlayToggle
                  : (NSString *)sceneKey isSelected
                  : (BOOL)isSelected) {
  [RefappDemoSession.shared setSceneOverlayToggleWithSceneKey:sceneKey isSelected:isSelected];
}

/// The scene chips' Clear control.
RCT_EXPORT_METHOD(clearSceneOverlays) {
  [RefappDemoSession.shared clearSceneOverlays];
}

RCT_EXPORT_METHOD(startTraceReplay) {
  [RefappDemoSession.shared startTraceReplay];
}

/// One demo-plan pick: the recorded walk the Start control replays (§4.1).
RCT_EXPORT_METHOD(selectTraceReplayPlan
                  : (NSString *)traceAssetName resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  [RefappDemoSession.shared selectTraceReplayPlanWithTraceAssetName:traceAssetName
      resolve:resolve
      reject:^(NSString *code, NSString *message) {
        reject(code, message, nil);
      }];
}

/// Starts guidance on the planned route from the device's own fixes; the
/// resolved value says whether a planned route let it start (the React side
/// states the plan-first sentence when it did not).
RCT_EXPORT_METHOD(startDeviceNavigation
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve(@([RefappDemoSession.shared startDeviceNavigation]));
}

/// Ends device-driven guidance (the React side stops the source it owns).
RCT_EXPORT_METHOD(stopDeviceNavigation) {
  [RefappDemoSession.shared stopDeviceNavigation];
}

/// One accepted device fix into the device-driven trip's navigator. A null
/// course or accuracy means the source reported none (the session supplies the
/// reducer's fallback accuracy).
RCT_EXPORT_METHOD(deliverDeviceNavigationFix
                  : (double)latitudeDegrees longitudeDegrees
                  : (double)longitudeDegrees courseHeadingDegrees
                  : (nullable NSNumber *)courseHeadingDegrees horizontalAccuracyMeters
                  : (nullable NSNumber *)horizontalAccuracyMeters) {
  [RefappDemoSession.shared
      deliverDeviceNavigationFixWithLatitudeDegrees:latitudeDegrees
      longitudeDegrees:longitudeDegrees
      courseHeadingDegrees:courseHeadingDegrees
      horizontalAccuracyMeters:horizontalAccuracyMeters];
}

RCT_EXPORT_METHOD(exitTraceReplay) {
  [RefappDemoSession.shared exitTraceReplay];
}

/// The drawer's pause act (§7.2): the playback holds its current fix.
RCT_EXPORT_METHOD(pauseTraceReplay) {
  [RefappDemoSession.shared pauseTraceReplay];
}

/// The drawer's resume act: the playback continues from its current fix.
RCT_EXPORT_METHOD(resumeTraceReplay) {
  [RefappDemoSession.shared resumeTraceReplay];
}

/// The drawer's seek act (M27): moves the trip to a travelled distance along
/// the route through the walker.
RCT_EXPORT_METHOD(seekTraceReplay
                  : (double)travelledMeters) {
  [RefappDemoSession.shared seekTraceReplay:travelledMeters];
}

/// The drawer's speed act: one of the offered multipliers.
RCT_EXPORT_METHOD(selectTraceReplaySpeedMultiplier
                  : (double)speedMultiplier) {
  [RefappDemoSession.shared selectTraceReplaySpeedMultiplier:speedMultiplier];
}

/// One verbatim guidance line into the speech engine (the consumer's speak hop).
RCT_EXPORT_METHOD(speakUtterance
                  : (NSString *)speechText engineUtteranceId
                  : (NSString *)engineUtteranceId) {
  [RefappDemoSession.shared speakGuidanceUtteranceWithSpeechText:speechText
      engineUtteranceId:engineUtteranceId];
}

/// The speech consumer's toggle-off path: whatever speaks, stops.
RCT_EXPORT_METHOD(stopSpeech) {
  [RefappDemoSession.shared stopGuidanceSpeech];
}

/// The speech engine's initialization state, read back at the React mount (§6.2).
RCT_EXPORT_METHOD(getSpeechEngineState
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve([RefappDemoSession.shared speechEngineStateDictionary]);
}

RCT_EXPORT_METHOD(retrySurfaceAttach) {
  [RefappDemoSession.shared retrySurfaceAttach];
}

RCT_EXPORT_METHOD(getDiagnosticsLines
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  // The package runtime's SDK/cache lines lead the demo's camera and tile
  // readback (the same order the Kotlin module builds).
  NSArray<NSString *> *runtimeLines =
      [[MapsSdkNativeRuntime shared] diagnosticsLines];
  NSArray<NSString *> *demoLines =
      [RefappDemoSession.shared diagnosticsLinesToArray];
  resolve([runtimeLines arrayByAddingObjectsFromArray:demoLines]);
}

RCT_EXPORT_METHOD(startDiagnosticEvents
                  : (double)minimumSeverityOrdinal
                  maximumBacklogCount : (double)maximumBacklogCount) {
  [RefappDemoSession.shared
      startDiagnosticEventsWithMinimumSeverityOrdinal:(NSInteger)minimumSeverityOrdinal
                                    maximumBacklogCount:(NSInteger)maximumBacklogCount];
}

RCT_EXPORT_METHOD(stopDiagnosticEvents) {
  [RefappDemoSession.shared stopDiagnosticEvents];
}

@end