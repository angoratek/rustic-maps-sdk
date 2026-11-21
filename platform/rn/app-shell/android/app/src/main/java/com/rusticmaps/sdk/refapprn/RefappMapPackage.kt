package com.rusticmaps.sdk.refapprn

import com.facebook.react.ReactPackage
import com.facebook.react.bridge.NativeModule
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.uimanager.ViewManager

/**
 * The React Native package registration of the reference application shell:
 * the `RefappMap` demo module only.
 *
 * The map surface, gestures, camera, overlays and the package-owned SDK moved
 * to the shipped package (`com.angoratek.rusticmaps.MapsSdkMapHost` registers its module
 * and `MapsSdkMapView` view manager); this package carries the demo's trace
 * replay, speech engine, secure credential store and plan/search glue.
 */
class RefappMapPackage : ReactPackage {

    override fun createNativeModules(
        reactContext: ReactApplicationContext,
    ): List<NativeModule> = listOf(RefappModule(reactContext))

    override fun createViewManagers(
        reactContext: ReactApplicationContext,
    ): List<ViewManager<*, *>> = emptyList()
}
