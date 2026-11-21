#import "MapsSdkNativeModule.h"

#import <React/RCTUtils.h>
#import <math.h>

#import "RusticMapsSdk-Swift.h"

namespace {
/// The promise rejection code every package failure carries.
NSString *const MapsSdkNativeFailureCode = @"MapsSdkNativeFailure";

/// The rejection code an invalid tile-cache time-to-live carries.
NSString *const MapsSdkInvalidTimeToLiveCode = @"MapsSdkInvalidTimeToLive";

/// The stable refusal for a time-to-live that is not a positive whole number.
NSString *const MapsSdkInvalidTimeToLiveText =
    @"the tile-cache time-to-live must be a finite, positive whole number of seconds";

/// The view-not-found rejection detail.
NSString *const MapsSdkNativeViewNotFoundText =
    @"the native MapView was not found";

/// The exclusive upper bound of the contract's `u64` time-to-live domain.
const double MapsSdkTimeToLiveExclusiveUpperBound = 18446744073709551616.0;

/// Whether the bridge's `number` is a positive, whole `u64` number of
/// seconds. Rejects NaN, infinities, fractions, non-positive values and
/// values the Swift `UInt64` conversion would refuse to represent.
BOOL MapsSdkIsValidTimeToLive(double timeToLiveSeconds) {
  return isfinite(timeToLiveSeconds) && timeToLiveSeconds > 0 &&
         floor(timeToLiveSeconds) == timeToLiveSeconds &&
         timeToLiveSeconds < MapsSdkTimeToLiveExclusiveUpperBound;
}
} // namespace

@implementation MapsSdkNativeModule

RCT_EXPORT_MODULE(MapsSdkNative)

- (dispatch_queue_t)methodQueue {
  return dispatch_get_main_queue();
}

+ (BOOL)requiresMainQueueSetup {
  return YES;
}

- (instancetype)init {
  if (self = [super init]) {
    __weak MapsSdkNativeModule *weakSelf = self;
    MapsSdkNativeEventChannel.sink =
        ^(NSString *eventName, NSDictionary<NSString *, id> *payload) {
          MapsSdkNativeModule *strongSelf = weakSelf;
          if (strongSelf) {
            dispatch_async(dispatch_get_main_queue(), ^{
              [strongSelf sendEventWithName:eventName body:payload];
            });
          }
        };
  }
  return self;
}

- (void)dealloc {
  MapsSdkNativeEventChannel.sink = nil;
}

- (NSArray<NSString *> *)supportedEvents {
  return @[
    MapsSdkNativeEventChannel.eventName,
    MapsSdkNativeEventChannel.offlineRegionEventName
  ];
}

- (void)startObserving {
  // The sink is installed at construction; the emitter only delivers while a
  // JavaScript listener is present, which React Native tracks through these
  // hooks.
}

- (void)stopObserving {
}

// ---------------------------------------------------------------------------
// SDK lifecycle and contract proxy
// ---------------------------------------------------------------------------

RCT_EXPORT_METHOD(initialize
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  NSError *startupError = nil;
  BOOL started = [[MapsSdkNativeRuntime shared]
      ensureStartedAndReturnError:&startupError];
  if (!started) {
    reject(MapsSdkNativeFailureCode,
           startupError.localizedDescription ?: @"the SDK did not start",
           startupError);
    return;
  }
  resolve(nil);
}

RCT_EXPORT_METHOD(getCompiledVersion
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve([[MapsSdkNativeRuntime shared] compiledVersion]);
}

RCT_EXPORT_METHOD(defaultProviderKey
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve([[MapsSdkNativeRuntime shared] defaultProviderKey]);
}

RCT_EXPORT_METHOD(setProviderCredential
                  : (NSString *)providerKey credential
                  : (NSString *)credential resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve([[MapsSdkNativeRuntime shared]
      setProviderCredentialWithProviderKey:providerKey
                                credential:credential
                            withTileCache:NO]);
}

RCT_EXPORT_METHOD(setProviderCredentialWithTileCache
                  : (NSString *)providerKey credential
                  : (NSString *)credential resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve([[MapsSdkNativeRuntime shared]
      setProviderCredentialWithProviderKey:providerKey
                                credential:credential
                            withTileCache:YES]);
}

RCT_EXPORT_METHOD(registerUrlTemplateRasterTileSource
                  : (NSString *)sourceJson resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  NSError *registrationError = nil;
  BOOL registered = [[MapsSdkNativeRuntime shared]
      registerUrlTemplateRasterTileSourceJSON:sourceJson
                                       error:&registrationError];
  if (registered) {
    resolve(@{ @"isApplied" : @YES, @"detailText" : @"" });
  } else {
    reject(MapsSdkNativeFailureCode,
           registrationError.localizedDescription,
           registrationError);
  }
}

RCT_EXPORT_METHOD(restoreCredentialedBaseMap
                  : (NSString *)providerKey resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve([[MapsSdkNativeRuntime shared]
      restoreCredentialedBaseMapWithProviderKey:providerKey]);
}

/// Chooses the provider that serves one capability.
RCT_EXPORT_METHOD(configureProvider
                  : (double)contractOrdinal providerKey
                  : (NSString *)providerKey credential
                  : (NSString *)credential resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  NSError *configurationError = nil;
  BOOL configured = [[MapsSdkNativeRuntime shared]
      configureProviderWithContractOrdinal:(NSInteger)contractOrdinal
                              providerKey:providerKey
                               credential:credential
                                    error:&configurationError];
  if (configured) {
    resolve(nil);
  } else {
    reject(MapsSdkNativeFailureCode,
           configurationError.localizedDescription,
           configurationError);
  }
}

/// Reports which provider serves each capability, as JSON.
RCT_EXPORT_METHOD(providerRegistrations
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  NSError *registrationsError = nil;
  NSString *registrations = [[MapsSdkNativeRuntime shared]
      providerRegistrationsJSONAndReturnError:&registrationsError];
  if (registrations != nil) {
    resolve(registrations);
  } else {
    reject(MapsSdkNativeFailureCode,
           registrationsError.localizedDescription,
           registrationsError);
  }
}

/// Reports the serving routing provider's capabilities, as JSON.
RCT_EXPORT_METHOD(routeCapabilities
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  NSError *capabilitiesError = nil;
  NSString *capabilities = [[MapsSdkNativeRuntime shared]
      routeCapabilitiesJSONAndReturnError:&capabilitiesError];
  if (capabilities != nil) {
    resolve(capabilities);
  } else {
    reject(MapsSdkNativeFailureCode,
           capabilitiesError.localizedDescription,
           capabilitiesError);
  }
}

RCT_EXPORT_METHOD(search
                  : (NSString *)queryText searchBoundaryJson
                  : (NSString *_Nullable)searchBoundaryJson resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  [[MapsSdkNativeRuntime shared] searchJSONWithQueryText:queryText
                                            boundaryJSON:searchBoundaryJson
                                              completion:^(NSString *result,
                                                           NSError *error) {
                                                if (result != nil) {
                                                  resolve(result);
                                                } else {
                                                  reject(MapsSdkNativeFailureCode,
                                                         error.localizedDescription,
                                                         error);
                                                }
                                              }];
}

RCT_EXPORT_METHOD(suggest
                  : (NSString *)queryText searchBoundaryJson
                  : (NSString *_Nullable)searchBoundaryJson resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  [[MapsSdkNativeRuntime shared] suggestJSONWithQueryText:queryText
                                             boundaryJSON:searchBoundaryJson
                                               completion:^(NSString *result,
                                                            NSError *error) {
                                                 if (result != nil) {
                                                   resolve(result);
                                                 } else {
                                                   reject(MapsSdkNativeFailureCode,
                                                          error.localizedDescription,
                                                          error);
                                                 }
                                               }];
}

RCT_EXPORT_METHOD(forwardGeocode
                  : (NSString *)addressText resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  [[MapsSdkNativeRuntime shared] forwardGeocodeJSONWithAddressText:addressText
                                                        completion:^(NSString *result,
                                                                     NSError *error) {
                                                          if (result != nil) {
                                                            resolve(result);
                                                          } else {
                                                            reject(MapsSdkNativeFailureCode,
                                                                   error.localizedDescription,
                                                                   error);
                                                          }
                                                        }];
}

RCT_EXPORT_METHOD(calculateRoute
                  : (NSString *)waypointsJson routeOptionsJson
                  : (NSString *)routeOptionsJson resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  [[MapsSdkNativeRuntime shared]
      calculateRouteJSONWithWaypointsJson:waypointsJson
                       routeOptionsJson:routeOptionsJson
                            completion:^(NSString *result, NSError *error) {
                              if (result != nil) {
                                resolve(result);
                              } else {
                                reject(MapsSdkNativeFailureCode,
                                       error.localizedDescription,
                                       error);
                              }
                            }];
}

RCT_EXPORT_METHOD(reverseGeocode
                  : (NSString *)coordinateJson resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  [[MapsSdkNativeRuntime shared] reverseGeocodeJSONWithCoordinateJSON:coordinateJson
                                                           completion:^(NSString *result,
                                                                        NSError *error) {
                                                             if (result != nil) {
                                                               resolve(result);
                                                             } else {
                                                               reject(MapsSdkNativeFailureCode,
                                                                      error.localizedDescription,
                                                                      error);
                                                             }
                                                           }];
}

/// Calculates the contract's distance matrix (M31).
RCT_EXPORT_METHOD(calculateDistanceMatrix
                  : (NSString *)originsJson destinationsJson
                  : (NSString *)destinationsJson transportationMode
                  : (double)transportationMode resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  [[MapsSdkNativeRuntime shared]
      calculateDistanceMatrixJSONWithOriginsJson:originsJson
                                destinationsJson:destinationsJson
                        transportationModeOrdinal:(NSInteger)transportationMode
                                      completion:^(NSString *result, NSError *error) {
                                        if (result != nil) {
                                          resolve(result);
                                        } else {
                                          reject(MapsSdkNativeFailureCode,
                                                 error.localizedDescription,
                                                 error);
                                        }
                                      }];
}

/// Fetches the incidents intersecting a bounding box (M31).
RCT_EXPORT_METHOD(fetchTrafficIncidents
                  : (NSString *)boundingBoxJson resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  [[MapsSdkNativeRuntime shared]
      fetchTrafficIncidentsJSONWithBoundingBoxJson:boundingBoxJson
                                        completion:^(NSString *result, NSError *error) {
                                          if (result != nil) {
                                            resolve(result);
                                          } else {
                                            reject(MapsSdkNativeFailureCode,
                                                   error.localizedDescription,
                                                   error);
                                          }
                                        }];
}

/// Fetches per-segment traffic flow inside a bounding box (M31).
RCT_EXPORT_METHOD(fetchTrafficFlow
                  : (NSString *)boundingBoxJson resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  [[MapsSdkNativeRuntime shared]
      fetchTrafficFlowJSONWithBoundingBoxJson:boundingBoxJson
                                   completion:^(NSString *result, NSError *error) {
                                     if (result != nil) {
                                       resolve(result);
                                     } else {
                                       reject(MapsSdkNativeFailureCode,
                                              error.localizedDescription,
                                              error);
                                     }
                                   }];
}

RCT_EXPORT_METHOD(tileCacheTerms
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  NSError *termsError = nil;
  NSString *terms = [[MapsSdkNativeRuntime shared]
      tileCacheTermsJSONAndReturnError:&termsError];
  if (terms != nil) {
    resolve(terms);
  } else {
    reject(MapsSdkNativeFailureCode, termsError.localizedDescription, termsError);
  }
}

RCT_EXPORT_METHOD(trafficFlowCacheStatistics
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  NSError *statisticsError = nil;
  NSString *statistics = [[MapsSdkNativeRuntime shared]
      trafficFlowCacheStatisticsJSONAndReturnError:&statisticsError];
  if (statistics != nil) {
    resolve(statistics);
  } else {
    reject(MapsSdkNativeFailureCode,
           statisticsError.localizedDescription,
           statisticsError);
  }
}

RCT_EXPORT_METHOD(tileCacheStatistics
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  NSError *statisticsError = nil;
  NSString *statistics = [[MapsSdkNativeRuntime shared]
      tileCacheStatisticsJSONAndReturnError:&statisticsError];
  if (statistics != nil) {
    resolve(statistics);
  } else {
    reject(MapsSdkNativeFailureCode,
           statisticsError.localizedDescription,
           statisticsError);
  }
}

RCT_EXPORT_METHOD(setTileCacheTimeToLive
                  : (double)timeToLiveSeconds resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  if (!MapsSdkIsValidTimeToLive(timeToLiveSeconds)) {
    reject(MapsSdkInvalidTimeToLiveCode, MapsSdkInvalidTimeToLiveText, nil);
    return;
  }
  NSError *timeToLiveError = nil;
  BOOL applied = [[MapsSdkNativeRuntime shared]
      setTileCacheTimeToLive:timeToLiveSeconds
                       error:&timeToLiveError];
  if (applied) {
    resolve(nil);
  } else {
    reject(MapsSdkNativeFailureCode,
           timeToLiveError.localizedDescription,
           timeToLiveError);
  }
}

// ---------------------------------------------------------------------------
// Offline regions (M63)
// ---------------------------------------------------------------------------

/// How many tiles the region enumerates, for the pre-download warning.
RCT_EXPORT_METHOD(regionTileCount
                  : (NSString *)regionJson resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  NSError *countError = nil;
  NSNumber *count = [[MapsSdkNativeRuntime shared]
      regionTileCount:regionJson
                error:&countError];
  if (count != nil) {
    resolve(count);
  } else {
    reject(MapsSdkNativeFailureCode,
           countError.localizedDescription,
           countError);
  }
}

/// Downloads one region; progress crosses on the region event channel.
RCT_EXPORT_METHOD(downloadRegion
                  : (NSString *)regionJson resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  [[MapsSdkNativeRuntime shared]
      downloadRegionJSON:regionJson
              completion:^(NSString *result, NSError *error) {
                if (result != nil) {
                  resolve(result);
                } else {
                  reject(MapsSdkNativeFailureCode,
                         error.localizedDescription,
                         error);
                }
              }];
}

/// Cancels the region's in-flight download; idempotent.
RCT_EXPORT_METHOD(cancelRegionDownload
                  : (NSString *)regionId resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  NSError *cancelError = nil;
  BOOL cancelled = [[MapsSdkNativeRuntime shared]
      cancelRegionDownload:regionId
                     error:&cancelError];
  if (cancelled) {
    resolve(nil);
  } else {
    reject(MapsSdkNativeFailureCode,
           cancelError.localizedDescription,
           cancelError);
  }
}

/// Every region stored on disk, as JSON.
RCT_EXPORT_METHOD(regionRecords
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  [[MapsSdkNativeRuntime shared]
      regionRecordsJSONWithCompletion:^(NSString *result, NSError *error) {
        if (result != nil) {
          resolve(result);
        } else {
          reject(MapsSdkNativeFailureCode, error.localizedDescription, error);
        }
      }];
}

/// One stored region's record as JSON, or null when none is stored.
RCT_EXPORT_METHOD(regionRecord
                  : (NSString *)regionId resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  [[MapsSdkNativeRuntime shared]
      regionRecordJSON:regionId
            completion:^(NSString *result, NSError *error) {
              if (result != nil) {
                resolve(result);
              } else if (error != nil) {
                reject(MapsSdkNativeFailureCode,
                       error.localizedDescription,
                       error);
              } else {
                resolve(nil);
              }
            }];
}

/// Deletes one stored region and answers what was removed, as JSON.
RCT_EXPORT_METHOD(deleteRegion
                  : (NSString *)regionId resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  [[MapsSdkNativeRuntime shared]
      deleteRegionJSON:regionId
            completion:^(NSString *result, NSError *error) {
              if (result != nil) {
                resolve(result);
              } else {
                reject(MapsSdkNativeFailureCode,
                       error.localizedDescription,
                       error);
              }
            }];
}

RCT_EXPORT_METHOD(latestProviderFailureDiagnostics
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  NSError *diagnosticsError = nil;
  NSString *diagnostics = [[MapsSdkNativeRuntime shared]
      latestProviderFailureDiagnosticsJSONAndReturnError:&diagnosticsError];
  if (diagnostics != nil) {
    resolve(diagnostics);
  } else {
    reject(MapsSdkNativeFailureCode,
           diagnosticsError.localizedDescription,
           diagnosticsError);
  }
}

RCT_EXPORT_METHOD(diagnosticsLines
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve([[MapsSdkNativeRuntime shared] diagnosticsLines]);
}

// ---------------------------------------------------------------------------
// Device positioning
// ---------------------------------------------------------------------------

RCT_EXPORT_METHOD(startDeviceLocation
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve(@([[MapsSdkNativeRuntime shared] startDeviceLocation]));
}

RCT_EXPORT_METHOD(stopDeviceLocation
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve(@([[MapsSdkNativeRuntime shared] stopDeviceLocation]));
}

RCT_EXPORT_METHOD(latestDeviceFix
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve([[MapsSdkNativeRuntime shared] latestDeviceFixJSON]);
}

RCT_EXPORT_METHOD(deviceLocationSmoothedCourseDegrees
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve([[MapsSdkNativeRuntime shared] deviceLocationSmoothedCourseDegrees]);
}

RCT_EXPORT_METHOD(deviceLocationSourceStatus
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  resolve([[MapsSdkNativeRuntime shared] deviceLocationSourceStatusName]);
}

// ---------------------------------------------------------------------------
// MapView imperative calls (addressed by the view identifier)
// ---------------------------------------------------------------------------

- (MapsSdkMapHostView *)hostViewForIdentifier:(NSString *)viewIdentifier {
  return [MapsSdkMapHostView findWithViewIdentifier:viewIdentifier];
}

RCT_EXPORT_METHOD(setCamera
                  : (NSString *)viewIdentifier cameraJson
                  : (NSString *)cameraJson) {
  [[self hostViewForIdentifier:viewIdentifier] setCameraJSON:cameraJson];
}

RCT_EXPORT_METHOD(animateCamera
                  : (NSString *)viewIdentifier cameraJson
                  : (NSString *)cameraJson durationMilliseconds
                  : (double)durationMilliseconds) {
  [[self hostViewForIdentifier:viewIdentifier]
      animateCameraJSON:cameraJson
      durationMilliseconds:durationMilliseconds];
}

RCT_EXPORT_METHOD(fitBounds
                  : (NSString *)viewIdentifier boundingBoxJson
                  : (NSString *)boundingBoxJson edgeInsetsJson
                  : (NSString *)edgeInsetsJson maximumZoomLevel
                  : (double)maximumZoomLevel) {
  [[self hostViewForIdentifier:viewIdentifier]
      fitBoundsJSON:boundingBoxJson
      edgeInsetsJson:edgeInsetsJson
      maximumZoomLevel:maximumZoomLevel];
}

RCT_EXPORT_METHOD(panByScreenPixels
                  : (NSString *)viewIdentifier deltaXPixels
                  : (double)deltaXPixels deltaYPixels
                  : (double)deltaYPixels isGestureDriven
                  : (BOOL)isGestureDriven) {
  [[self hostViewForIdentifier:viewIdentifier]
      panByScreenPixelsWithDeltaXPixels:deltaXPixels
                           deltaYPixels:deltaYPixels
                        isGestureDriven:isGestureDriven];
}

RCT_EXPORT_METHOD(zoomByLevels
                  : (NSString *)viewIdentifier deltaZoomLevels
                  : (double)deltaZoomLevels) {
  [[self hostViewForIdentifier:viewIdentifier]
      zoomByLevelsWithDeltaZoomLevels:deltaZoomLevels];
}

RCT_EXPORT_METHOD(rotateByDegrees
                  : (NSString *)viewIdentifier clockwiseDegrees
                  : (double)clockwiseDegrees) {
  [[self hostViewForIdentifier:viewIdentifier]
      rotateByDegreesWithClockwiseDegrees:clockwiseDegrees];
}

RCT_EXPORT_METHOD(viewportWidthPixels
                  : (NSString *)viewIdentifier resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  MapsSdkMapHostView *hostView =
      [self hostViewForIdentifier:viewIdentifier];
  if (hostView == nil) {
    reject(MapsSdkNativeFailureCode, MapsSdkNativeViewNotFoundText, nil);
    return;
  }
  resolve(@(hostView.viewportWidthPixels));
}

RCT_EXPORT_METHOD(viewportHeightPixels
                  : (NSString *)viewIdentifier resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  MapsSdkMapHostView *hostView =
      [self hostViewForIdentifier:viewIdentifier];
  if (hostView == nil) {
    reject(MapsSdkNativeFailureCode, MapsSdkNativeViewNotFoundText, nil);
    return;
  }
  resolve(@(hostView.viewportHeightPixels));
}

RCT_EXPORT_METHOD(viewportPixelExtent
                  : (NSString *)viewIdentifier resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  MapsSdkMapHostView *hostView =
      [self hostViewForIdentifier:viewIdentifier];
  if (hostView == nil) {
    reject(MapsSdkNativeFailureCode, MapsSdkNativeViewNotFoundText, nil);
    return;
  }
  CGSize extent = hostView.viewportPixelExtentPixels;
  if (extent.width <= 0 || extent.height <= 0) {
    resolve([NSNull null]);
    return;
  }
  resolve(@{
    @"widthPixels": @(extent.width),
    @"heightPixels": @(extent.height),
  });
}

RCT_EXPORT_METHOD(displayScale
                  : (NSString *)viewIdentifier resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  MapsSdkMapHostView *hostView =
      [self hostViewForIdentifier:viewIdentifier];
  if (hostView == nil) {
    reject(MapsSdkNativeFailureCode, MapsSdkNativeViewNotFoundText, nil);
    return;
  }
  resolve(@(hostView.displayScale));
}

RCT_EXPORT_METHOD(addOverlay
                  : (NSString *)viewIdentifier overlayIdentifier
                  : (NSString *)overlayIdentifier overlayJson
                  : (NSString *)overlayJson resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  MapsSdkMapHostView *hostView =
      [self hostViewForIdentifier:viewIdentifier];
  if (hostView == nil) {
    reject(MapsSdkNativeFailureCode, MapsSdkNativeViewNotFoundText, nil);
    return;
  }
  resolve(@([hostView addOverlayJSON:overlayIdentifier
                          overlayJson:overlayJson]));
}

RCT_EXPORT_METHOD(updateOverlay
                  : (NSString *)viewIdentifier overlayIdentifier
                  : (NSString *)overlayIdentifier overlayJson
                  : (NSString *)overlayJson resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  MapsSdkMapHostView *hostView =
      [self hostViewForIdentifier:viewIdentifier];
  if (hostView == nil) {
    reject(MapsSdkNativeFailureCode, MapsSdkNativeViewNotFoundText, nil);
    return;
  }
  resolve(@([hostView updateOverlayJSON:overlayIdentifier
                             overlayJson:overlayJson]));
}

RCT_EXPORT_METHOD(removeOverlay
                  : (NSString *)viewIdentifier overlayIdentifier
                  : (NSString *)overlayIdentifier resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  MapsSdkMapHostView *hostView =
      [self hostViewForIdentifier:viewIdentifier];
  if (hostView == nil) {
    reject(MapsSdkNativeFailureCode, MapsSdkNativeViewNotFoundText, nil);
    return;
  }
  resolve(@([hostView removeOverlayWithOverlayIdentifier:overlayIdentifier]));
}

RCT_EXPORT_METHOD(setOverlays
                  : (NSString *)viewIdentifier overlaysJson
                  : (NSString *)overlaysJson resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  MapsSdkMapHostView *hostView =
      [self hostViewForIdentifier:viewIdentifier];
  if (hostView == nil) {
    reject(MapsSdkNativeFailureCode, MapsSdkNativeViewNotFoundText, nil);
    return;
  }
  resolve(@([hostView setOverlaysJSON:overlaysJson]));
}

RCT_EXPORT_METHOD(clearOverlays
                  : (NSString *)viewIdentifier resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  MapsSdkMapHostView *hostView =
      [self hostViewForIdentifier:viewIdentifier];
  if (hostView == nil) {
    reject(MapsSdkNativeFailureCode, MapsSdkNativeViewNotFoundText, nil);
    return;
  }
  resolve(@([hostView clearOverlays]));
}

RCT_EXPORT_METHOD(setLocationIndicator
                  : (NSString *)viewIdentifier stateJson
                  : (NSString *)stateJson styleJson
                  : (NSString *)styleJson) {
  [[self hostViewForIdentifier:viewIdentifier]
      setLocationIndicatorJSON:stateJson
                     styleJson:styleJson];
}

RCT_EXPORT_METHOD(clearLocationIndicator
                  : (NSString *)viewIdentifier) {
  [[self hostViewForIdentifier:viewIdentifier] clearLocationIndicator];
}

RCT_EXPORT_METHOD(retryAttach
                  : (NSString *)viewIdentifier) {
  [[self hostViewForIdentifier:viewIdentifier] retryAttach];
}

RCT_EXPORT_METHOD(setVectorTileSource
                  : (NSString *)viewIdentifier sourceJson
                  : (NSString *)sourceJson resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  MapsSdkMapHostView *hostView =
      [self hostViewForIdentifier:viewIdentifier];
  if (hostView == nil) {
    reject(MapsSdkNativeFailureCode, MapsSdkNativeViewNotFoundText, nil);
    return;
  }
  resolve(@([hostView setVectorTileSourceJSON:sourceJson]));
}

RCT_EXPORT_METHOD(setProviderVectorTileSourceFromDocument
                  : (NSString *)viewIdentifier sourceJson
                  : (NSString *)sourceJson styleDocumentUrl
                  : (NSString *)styleDocumentUrl resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  MapsSdkMapHostView *hostView =
      [self hostViewForIdentifier:viewIdentifier];
  if (hostView == nil) {
    reject(MapsSdkNativeFailureCode, MapsSdkNativeViewNotFoundText, nil);
    return;
  }
  [hostView setProviderVectorTileSourceFromDocumentJSON:sourceJson
                                       styleDocumentURL:styleDocumentUrl
                                             completion:^(BOOL accepted) {
    resolve(@(accepted));
  }];
}

RCT_EXPORT_METHOD(clearVectorTileSource
                  : (NSString *)viewIdentifier resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  MapsSdkMapHostView *hostView =
      [self hostViewForIdentifier:viewIdentifier];
  if (hostView == nil) {
    reject(MapsSdkNativeFailureCode, MapsSdkNativeViewNotFoundText, nil);
    return;
  }
  resolve(@([hostView clearVectorTileSource]));
}

RCT_EXPORT_METHOD(getVectorTileSourceStatus
                  : (NSString *)viewIdentifier resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  MapsSdkMapHostView *hostView =
      [self hostViewForIdentifier:viewIdentifier];
  if (hostView == nil) {
    reject(MapsSdkNativeFailureCode, MapsSdkNativeViewNotFoundText, nil);
    return;
  }
  resolve([hostView vectorTileSourceStatusJSON]);
}

RCT_EXPORT_METHOD(reloadTileSource
                  : (NSString *)viewIdentifier) {
  [[self hostViewForIdentifier:viewIdentifier] reloadTileSource];
}

RCT_EXPORT_METHOD(setRenderingPaused
                  : (NSString *)viewIdentifier paused
                  : (BOOL)paused) {
  [[self hostViewForIdentifier:viewIdentifier] setRenderingPaused:paused];
}

RCT_EXPORT_METHOD(getCamera
                  : (NSString *)viewIdentifier resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  MapsSdkMapHostView *hostView =
      [self hostViewForIdentifier:viewIdentifier];
  if (hostView == nil) {
    reject(MapsSdkNativeFailureCode, MapsSdkNativeViewNotFoundText, nil);
    return;
  }
  resolve([hostView getCameraJSON]);
}

// The first-frame readiness readback. The `mapReady` event fires once; a
// JavaScript subscriber that attaches after it reads the same state here.
RCT_EXPORT_METHOD(isMapReady
                  : (NSString *)viewIdentifier resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  MapsSdkMapHostView *hostView =
      [self hostViewForIdentifier:viewIdentifier];
  if (hostView == nil) {
    reject(MapsSdkNativeFailureCode, MapsSdkNativeViewNotFoundText, nil);
    return;
  }
  resolve(@(hostView.hasDrawnFirstFrame));
}

RCT_EXPORT_METHOD(groundMetersPerScreenPixel
                  : (NSString *)viewIdentifier resolve
                  : (RCTPromiseResolveBlock)resolve reject
                  : (RCTPromiseRejectBlock)reject) {
  MapsSdkMapHostView *hostView =
      [self hostViewForIdentifier:viewIdentifier];
  if (hostView == nil) {
    reject(MapsSdkNativeFailureCode, MapsSdkNativeViewNotFoundText, nil);
    return;
  }
  resolve([hostView groundMetersPerScreenPixelNumber]);
}

@end
