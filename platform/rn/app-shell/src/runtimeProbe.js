/**
 * Startup probe for the reference shell's runtime bring-up: logs, before any
 * React Native module is required, which global bridges the Hermes runtime
 * exposed (`__turboModuleProxy` is the TurboModuleManager JSI binding, the
 * legacy `NativeModules` bridge comes through the batched bridge shim). The
 * TurboModuleRegistry's first core lookup (`PlatformConstants`) has been the
 * observed failure; this distinguishes a missing JSI proxy from a missing
 * module registration without a debugger attached.
 */
const probeResult = {
  turboModuleProxy: typeof global.__turboModuleProxy,
  nativeBridge: typeof global.__fbBatchedBridge,
  bridgelessSetupFlagSet: typeof global.RN$BridgelessSetup,
  appRegistryGlobal: typeof global.RN$AppRegistry,
};

console.log('RuntimeProbe: ' + JSON.stringify(probeResult));

module.exports = probeResult;