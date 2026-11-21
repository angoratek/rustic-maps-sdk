/**
 * React Native autolinking descriptor for @angoratek/rustic-maps-sdk.
 *
 * React Native autolinking registers one package per dependency. The
 * ubrn-generated `RusticMapsSdkPackage` only exposes the JSI TurboModule, and the
 * package's own `MapsSdkNative` module and `MapsSdkMapView` view manager live
 * beside it, so a host that relied on the generated default registered only
 * the TurboModule. This descriptor points autolinking at `MapsSdkMapHost`,
 * the package's one hand-written entry point, which exposes all three.
 *
 * Consumers need no manual `add(...)` in `MainApplication`: autolinking adds
 * this package from the dependency's own configuration.
 */
module.exports = {
  dependency: {
    platforms: {
      android: {
        packageImportPath: 'import com.angoratek.rusticmaps.MapsSdkMapHost;',
        packageInstance: 'new MapsSdkMapHost()',
      },
    },
  },
};
