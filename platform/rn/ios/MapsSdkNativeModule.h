#import <React/RCTBridgeModule.h>
#import <React/RCTEventEmitter.h>

/**
 * The React Native package's iOS native module (`MapsSdkNative` in
 * JavaScript; the Kotlin `MapsSdkNativeModule` mirror).
 *
 * It owns the package's one `MapSdk` through `MapsSdkNativeRuntime`, proxies
 * the contract's provider/search/diagnostics members, forwards the `<MapView>`
 * imperative calls to the addressed host view, and installs the single view
 * event channel. All SDK-driving methods run on the main queue and every
 * failure is rejected verbatim.
 */
@interface MapsSdkNativeModule : RCTEventEmitter <RCTBridgeModule>
@end
