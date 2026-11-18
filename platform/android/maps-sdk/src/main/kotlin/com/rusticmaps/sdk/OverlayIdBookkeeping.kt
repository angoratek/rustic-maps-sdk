package com.rusticmaps.sdk

import uniffi.rustic_maps.MapOverlay

/**
 * The identifier-addressed overlay bookkeeping behind [MapView]'s
 * `addOverlay`/`updateOverlay`/`removeOverlay` members.
 *
 * The SDK push can refuse a list. A refused push must leave the bookkeeping
 * exactly as it was: otherwise a later call believes an overlay it never
 * pushed, and a refused removal leaves an overlay drawn but forgotten. Each
 * mutation therefore takes a snapshot, pushes the candidate list, and restores
 * the snapshot when [push] answers false. [push] is the view's native push
 * seam; the unit tests substitute a scripted answer.
 */
internal class OverlayIdBookkeeping {
    private var descriptorsById: LinkedHashMap<String, MapOverlay> = LinkedHashMap()

    /** The tracked overlays in insertion order. */
    val descriptors: List<MapOverlay>
        get() = descriptorsById.values.toList()

    /** Whether [id] is currently tracked. */
    fun contains(id: String): Boolean = descriptorsById.containsKey(id)

    /** A copy of the current bookkeeping, for a whole-list replacement rollback. */
    fun snapshot(): Map<String, MapOverlay> = LinkedHashMap(descriptorsById)

    /** Restores a snapshot taken before an accepted-or-refused whole-list replacement. */
    fun restore(snapshot: Map<String, MapOverlay>) {
        descriptorsById = LinkedHashMap(snapshot)
    }

    /** Drops every identifier; the whole-list replacement starts from this empty map. */
    fun clear() {
        descriptorsById.clear()
    }

    /** Adds or replaces [overlay] under [id]; restores the bookkeeping when [push] refuses. */
    fun add(id: String, overlay: MapOverlay, push: (List<MapOverlay>) -> Boolean): Boolean {
        val previous = snapshot()
        descriptorsById[id] = overlay
        return pushOrRestore(previous, push)
    }

    /** Replaces the overlay under a known [id]; restores the bookkeeping when [push] refuses. */
    fun update(id: String, overlay: MapOverlay, push: (List<MapOverlay>) -> Boolean): Boolean {
        val previous = snapshot()
        descriptorsById[id] = overlay
        return pushOrRestore(previous, push)
    }

    /**
     * Removes the overlay under [id] (an unknown identifier is a no-op);
     * restores the bookkeeping when [push] refuses the removal.
     */
    fun remove(id: String, push: (List<MapOverlay>) -> Boolean): Boolean {
        val previous = snapshot()
        if (descriptorsById.remove(id) == null) {
            return true
        }
        return pushOrRestore(previous, push)
    }

    private fun pushOrRestore(
        previous: Map<String, MapOverlay>,
        push: (List<MapOverlay>) -> Boolean,
    ): Boolean {
        if (push(descriptors)) {
            return true
        }
        restore(previous)
        return false
    }
}
