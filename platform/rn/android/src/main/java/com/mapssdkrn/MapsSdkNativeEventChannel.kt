package com.mapssdkrn

import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.ReadableType
import com.facebook.react.bridge.WritableMap

/**
 * The package's single native-to-JavaScript view event channel.
 *
 * The native module installs the sink (the `RCTDeviceEventEmitter` on
 * Android, the `RCTEventEmitter` on iOS); every host view emits one named
 * event carrying its JavaScript-side identifier, the event name and the
 * event's own fields, so the component can filter events for its own view.
 */
object MapsSdkNativeEventChannel {

    /** The named event the JavaScript component subscribes to. */
    const val EVENT_NAME: String = "MapsSdkMapViewEvent"

    /** The named event the typed SDK proxy subscribes to for region progress. */
    const val OFFLINE_REGION_EVENT_NAME: String = "MapsSdkOfflineRegionEvent"

    /** The module's emitter, or null before the module initialized. */
    var sink: ((eventName: String, payload: WritableMap) -> Unit)? = null

    /** Emits one view event with the identifier and name folded in. */
    fun emit(viewIdentifier: String, eventName: String, payload: WritableMap?) {
        val mergedPayload = Arguments.createMap()
        mergedPayload.putString("viewIdentifier", viewIdentifier)
        mergedPayload.putString("event", eventName)
        payload?.let { eventPayload -> copyFields(eventPayload, mergedPayload) }
        sink?.invoke(EVENT_NAME, mergedPayload)
    }

    /** Emits one region-download progress report. */
    fun emitOfflineRegionProgress(payload: WritableMap) {
        sink?.invoke(OFFLINE_REGION_EVENT_NAME, payload)
    }

    private fun copyFields(source: WritableMap, destination: WritableMap) {
        val keys = source.keySetIterator()
        while (keys.hasNextKey()) {
            val key = keys.nextKey()
            when (source.getType(key)) {
                ReadableType.Null -> destination.putNull(key)
                ReadableType.Boolean ->
                    destination.putBoolean(key, source.getBoolean(key))
                ReadableType.Number ->
                    destination.putDouble(key, source.getDouble(key))
                ReadableType.String ->
                    destination.putString(key, source.getString(key))
                ReadableType.Map ->
                    destination.putMap(key, source.getMap(key))
                ReadableType.Array ->
                    destination.putArray(key, source.getArray(key))
            }
        }
    }
}
