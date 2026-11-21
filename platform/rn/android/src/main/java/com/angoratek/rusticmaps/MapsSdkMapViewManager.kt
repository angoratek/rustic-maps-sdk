package com.angoratek.rusticmaps

import com.facebook.react.uimanager.SimpleViewManager
import com.facebook.react.uimanager.ThemedReactContext
import com.facebook.react.uimanager.annotations.ReactProp

/**
 * The view manager exposing [MapsSdkMapHostView] to JavaScript as
 * `MapsSdkMapView`.
 *
 * The manager creates the host and applies the declared props; the drawing
 * lifecycle, gestures and frame pump belong to the AAR widget inside the host.
 * `onDropViewInstance` releases the surface and the native objects.
 */
class MapsSdkMapViewManager : SimpleViewManager<MapsSdkMapHostView>() {

    companion object {
        /** The component name the JavaScript `MapView` registers. */
        const val VIEW_NAME: String = "MapsSdkMapView"

        /** The stable JavaScript-side view identifier prop. */
        const val VIEW_IDENTIFIER_PROP: String = "viewIdentifier"

        /** The four safe-area inset prop names (physical pixels). */
        const val TOP_INSET_PROP: String = "topBarInsetPixels"
        const val BOTTOM_INSET_PROP: String = "bottomBarInsetPixels"
        const val LEFT_INSET_PROP: String = "leftBarInsetPixels"
        const val RIGHT_INSET_PROP: String = "rightBarInsetPixels"

        /** The gesture and render-pause props. */
        const val GESTURE_ENABLED_PROP: String = "isGestureEnabled"
        const val RENDERING_PAUSED_PROP: String = "isRenderingPaused"
    }

    override fun getName(): String = VIEW_NAME

    override fun createViewInstance(reactContext: ThemedReactContext): MapsSdkMapHostView =
        MapsSdkMapHostView(reactContext)

    override fun onDropViewInstance(view: MapsSdkMapHostView) {
        view.onDestroy()
        super.onDropViewInstance(view)
    }

    @ReactProp(name = VIEW_IDENTIFIER_PROP)
    fun setViewIdentifier(hostView: MapsSdkMapHostView, viewIdentifier: String?) {
        hostView.viewIdentifier = viewIdentifier
    }

    @ReactProp(name = TOP_INSET_PROP)
    fun setTopBarInsetPixels(hostView: MapsSdkMapHostView, insetTopPixels: Int) {
        hostView.setTopBarInsetPixels(insetTopPixels)
    }

    @ReactProp(name = BOTTOM_INSET_PROP)
    fun setBottomBarInsetPixels(hostView: MapsSdkMapHostView, insetBottomPixels: Int) {
        hostView.setBottomBarInsetPixels(insetBottomPixels)
    }

    @ReactProp(name = LEFT_INSET_PROP)
    fun setLeftBarInsetPixels(hostView: MapsSdkMapHostView, insetLeftPixels: Int) {
        hostView.setLeftBarInsetPixels(insetLeftPixels)
    }

    @ReactProp(name = RIGHT_INSET_PROP)
    fun setRightBarInsetPixels(hostView: MapsSdkMapHostView, insetRightPixels: Int) {
        hostView.setRightBarInsetPixels(insetRightPixels)
    }

    @ReactProp(name = GESTURE_ENABLED_PROP, defaultBoolean = true)
    fun setGestureEnabled(hostView: MapsSdkMapHostView, isGestureEnabled: Boolean) {
        hostView.isGestureEnabled = isGestureEnabled
    }

    @ReactProp(name = RENDERING_PAUSED_PROP, defaultBoolean = false)
    fun setRenderingPaused(hostView: MapsSdkMapHostView, isRenderingPaused: Boolean) {
        hostView.setRenderingPaused(isRenderingPaused)
    }
}
