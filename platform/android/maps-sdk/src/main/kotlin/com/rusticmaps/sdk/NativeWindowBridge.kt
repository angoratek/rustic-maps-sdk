package com.rusticmaps.sdk

import android.view.Surface

/**
 * The JNI bridge from an Android [Surface] to the `ANativeWindow*` the SDK's
 * `SurfaceHandle` record needs.
 *
 * Android exposes no public Kotlin API for a surface's native window, so the
 * SDK ships the two-export shim compiled into the same `librustic_maps.so`
 * the UniFFI bindings load. The exported symbol names are bound to this
 * object's exact package and class name; renaming or moving it breaks the
 * link, which is why the `:maps-sdk-native` AAR ships a consumer ProGuard keep
 * rule for it.
 *
 * The returned pointer is opaque to Kotlin: it is passed to the native
 * `MapView.attach` and released through [releaseNativeWindow] exactly once.
 * A refused surface answers zero, which [MapView] lowers to `null` so the
 * SDK's exact-one-platform-pointer validation reports the failure verbatim.
 */
internal object NativeWindowBridge {
    init {
        // The same shared object the generated UniFFI bindings resolve by
        // name; the AAR stages it per ABI in `jniLibs`.
        System.loadLibrary("rustic_maps")
    }

    /**
     * Acquires the `ANativeWindow` behind [surface] and answers its raw
     * pointer, or zero when the platform refuses the surface.
     */
    external fun nativeWindowPointerFromSurface(surface: Surface): Long

    /**
     * Releases a pointer answered by [nativeWindowPointerFromSurface]; zero
     * is a no-op so teardown paths release unconditionally.
     */
    external fun releaseNativeWindow(nativeWindowPointer: Long)
}
