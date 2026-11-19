package com.rusticmaps.sdk.compose

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.rusticmaps.sdk.MapCameraPose
import com.rusticmaps.sdk.MapView as MapViewWidget
import com.rusticmaps.sdk.MapViewListener
import uniffi.rustic_maps.MapSdk

/**
 * Compose entry point of the shipped Android [MapViewWidget].
 *
 * The composable hosts the lifecycle-aware view, forwards the current
 * [sdk] and [listener] on every recomposition, and drives the view's
 * `onStart`/`onStop`/`onDestroy` from the host lifecycle so a Compose screen
 * needs no manual lifecycle glue. The view is released when it leaves the
 * composition.
 *
 * @param sdk the SDK state the view draws from.
 * @param modifier the layout modifier.
 * @param listener the map callbacks, held for the view's lifetime.
 * @param initialCamera the camera applied once when the view is created.
 * @param isGestureEnabled whether drag, pinch and rotate reach the camera.
 * @param onViewCreated receives the created view for imperative camera,
 *   overlay and location-layer calls (for example
 *   `view.fitBounds(route.boundingBox, ...)`).
 */
@Composable
fun MapView(
    sdk: MapSdk,
    modifier: Modifier = Modifier,
    listener: MapViewListener? = null,
    initialCamera: MapCameraPose? = null,
    isGestureEnabled: Boolean = true,
    onViewCreated: (MapViewWidget) -> Unit = {},
) {
    val lifecycleOwner = LocalLifecycleOwner.current
    var createdView by remember { mutableStateOf<MapViewWidget?>(null) }

    AndroidView(
        modifier = modifier,
        factory = { viewContext ->
            MapViewWidget(viewContext).apply {
                this.sdk = sdk
                this.listener = listener
                this.isGestureEnabled = isGestureEnabled
                initialCamera?.let(::setCamera)
                createdView = this
                onViewCreated(this)
            }
        },
        update = { view ->
            view.sdk = sdk
            view.listener = listener
            view.isGestureEnabled = isGestureEnabled
        },
        onRelease = { view ->
            view.onDestroy()
            if (createdView === view) {
                createdView = null
            }
        },
    )

    DisposableEffect(lifecycleOwner, createdView) {
        val view = createdView ?: return@DisposableEffect onDispose { }
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_START -> view.onStart()
                Lifecycle.Event.ON_STOP -> view.onStop()
                Lifecycle.Event.ON_DESTROY -> view.onDestroy()
                else -> Unit
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        if (lifecycleOwner.lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)) {
            view.onStart()
        }
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
        }
    }
}
