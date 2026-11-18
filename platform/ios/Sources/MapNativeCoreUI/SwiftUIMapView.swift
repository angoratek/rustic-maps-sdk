//! The SwiftUI entry point of the shipped iOS `MapView`.
//!
//! The representable hosts one `MapView`, forwards the current SDK and
//! delegate on every update, and releases the view's display link and
//! surface in `dismantleUIView` (never in the view's `deinit`: the Rust
//! side must release its surface while the Metal layer still exists). A
//! SwiftUI screen therefore needs no frame-pump or attach glue.
//!
//! The `onViewCreated` callback hands the created view to the caller for
//! imperative camera, overlay and location-layer calls (for example
//! `view.fitBounds(boundingBox: ..., ...)`), mirroring the Compose
//! wrapper's `onViewCreated`.

import RusticMapsSDK
import SwiftUI

/// The SwiftUI wrapper around one shipped `MapView` and its frame cadence.
public struct SwiftUIMapView: UIViewRepresentable {
    /// The SDK the view draws from; nil draws nothing (the black canvas
    /// stands) until a provider credential builds the session.
    public let sdk: MapSdk?

    /// The map callbacks (readiness, camera, tap, failure, statistics).
    public let delegate: MapViewDelegate?

    /// Receives the created view once, for imperative calls.
    public let onViewCreated: (MapView) -> Void

    /// Builds the wrapper.
    public init(
        sdk: MapSdk?,
        delegate: MapViewDelegate?,
        onViewCreated: @escaping (MapView) -> Void = { _ in }
    ) {
        self.sdk = sdk
        self.delegate = delegate
        self.onViewCreated = onViewCreated
    }

    public func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    public func makeUIView(context: Context) -> MapView {
        let mapView = MapView(frame: .zero)
        mapView.sdk = sdk
        mapView.delegate = delegate
        onViewCreated(mapView)
        return mapView
    }

    public func updateUIView(_ mapView: MapView, context: Context) {
        mapView.sdk = sdk
        mapView.delegate = delegate
    }

    public static func dismantleUIView(
        _ mapView: MapView,
        coordinator: Coordinator
    ) {
        mapView.teardown()
    }

    /// The representable's (empty) coordinator: the view owns its own
    /// lifecycle, so no per-view state lives here.
    public final class Coordinator {
        public init() {}
    }
}
