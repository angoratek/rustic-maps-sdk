package com.mapssdkrn

import com.facebook.react.BaseReactPackage
import com.facebook.react.bridge.NativeModule
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.module.model.ReactModuleInfo
import com.facebook.react.module.model.ReactModuleInfoProvider
import com.facebook.react.uimanager.ViewManager
import java.util.HashMap

/**
 * The package's single autolinked ReactPackage entry point.
 *
 * React Native autolinking registers one package per dependency, and
 * `react-native.config.js` names this class. It exposes every native surface
 * the package ships:
 *
 * - the ubrn-generated `MapsSdkRn` TurboModule that installs the Rust crate
 *   into Hermes;
 * - `MapsSdkNative`, the package's SDK proxy module; and
 * - `MapsSdkMapView`, the typed `<MapView>` component's view manager.
 *
 * The class deliberately does not end in `Package`: the generated
 * `MapsSdkRnPackage` keeps that filename for autolinking's discovery scan,
 * while this hand-written package is the instance the generated
 * `AndroidRNPackageList` registers.
 */
class MapsSdkMapHost : BaseReactPackage() {

    override fun getModule(
        name: String,
        reactContext: ReactApplicationContext,
    ): NativeModule? = when (name) {
        MapsSdkRnModule.NAME -> MapsSdkRnModule(reactContext)
        MapsSdkNativeModule.MODULE_NAME -> MapsSdkNativeModule(reactContext)
        else -> null
    }

    override fun getReactModuleInfoProvider(): ReactModuleInfoProvider =
        ReactModuleInfoProvider {
            val moduleInfos: MutableMap<String, ReactModuleInfo> = HashMap()
            // The ubrn TurboModule keeps the same ReactModuleInfo the
            // generated package declared: it is a TurboModule.
            moduleInfos[MapsSdkRnModule.NAME] = ReactModuleInfo(
                MapsSdkRnModule.NAME,
                MapsSdkRnModule.NAME,
                false, // canOverrideExistingModule
                false, // needsEagerInit
                false, // isCxxModule
                true, // isTurboModule
            )
            moduleInfos[MapsSdkNativeModule.MODULE_NAME] = ReactModuleInfo(
                MapsSdkNativeModule.MODULE_NAME,
                MapsSdkNativeModule.MODULE_NAME,
                false, // canOverrideExistingModule
                false, // needsEagerInit
                false, // isCxxModule
                false, // isTurboModule
            )
            moduleInfos
        }

    override fun createViewManagers(
        reactContext: ReactApplicationContext,
    ): List<ViewManager<*, *>> = listOf(MapsSdkMapViewManager())
}
