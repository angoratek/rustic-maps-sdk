//! The React Native bridge module of the reference application's iOS shell
//! (`RefappMap` in JavaScript; the Kotlin `RefappModule` mirror): thin
//! platform glue over the `RefappDemoSession` singleton — the keyless
//! SDK start, the runtime provider credential and its opt-in Keychain store
//! (the credential is entered at runtime, never baked into the app;
//! docs/ProviderCredentialsAndBrowserProviders.md §2), the camera presets,
//! the simulated trace replay, the attach retry control, and the diagnostics
//! lines. All SDK-driving methods run on the main thread
//! (`MapView`'s contract) and report nothing silently: a refused call
//! publishes its verbatim failure through the controller's error-report event
//! (§6.3).

#import <React/RCTBridgeModule.h>
#import <React/RCTEventEmitter.h>

@interface RefappMapModule : RCTEventEmitter <RCTBridgeModule>
@end