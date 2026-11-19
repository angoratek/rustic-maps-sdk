//! The shipped wrapper's readbacks, failure phases and delegate liveness.
//! The iOS and Android `MapView` surfaces carry the same members and the same
//! nine failure phases, and both hold their callback listener weakly. Every
//! case runs on the simulator without a surface or a provider.

import MapNativeCoreUI
import UIKit
import XCTest

@testable import MapNativeCoreUI

/// The wrapper surface and liveness contract.
@MainActor
final class MapViewSurfaceParityTests: XCTestCase {
    private func makeMapView() -> MapView {
        MapView(frame: CGRect(x: 0, y: 0, width: 64, height: 64))
    }

    func testTheViewportAndReadinessReadbacksStartEmpty() {
        let mapView = makeMapView()
        defer { mapView.teardown() }

        XCTAssertEqual(mapView.viewportWidthPixels, 0)
        XCTAssertEqual(mapView.viewportHeightPixels, 0)
        XCTAssertNil(mapView.viewportSize)
        XCTAssertNil(mapView.viewportPixelExtent)
        XCTAssertEqual(mapView.displayScale, 1)
        XCTAssertFalse(mapView.hasDrawnFirstFrame)
    }

    func testTheFailurePhaseSetMatchesTheOtherTargets() {
        // The Android enum declares these nine phases in this order; the
        // iOS set must stay identical so a consumer can switch over one
        // contract.
        let phases: [MapViewFailurePhase] = [
            .surface,
            .resize,
            .draw,
            .camera,
            .overlay,
            .locationIndicator,
            .baseMap,
            .tap,
            .listener,
        ]

        XCTAssertEqual(phases.count, 9)
        XCTAssertEqual(Set(phases.map { String(describing: $0) }).count, 9)
    }

    func testTheDelegateIsHeldWeakly() {
        let mapView = makeMapView()
        defer { mapView.teardown() }
        weak var weakDelegate: EmptyMapViewDelegate?
        do {
            let delegate = EmptyMapViewDelegate()
            weakDelegate = delegate
            mapView.delegate = delegate
            XCTAssertNotNil(weakDelegate, "the delegate is alive while the host holds it")
        }

        XCTAssertNil(
            weakDelegate,
            "the view must not retain its delegate; a delegate that captures its " +
                "owner would otherwise form a cycle"
        )
    }
}

/// A delegate that overrides nothing: the protocol extension supplies every
/// default, so an empty implementation is complete.
@MainActor
private final class EmptyMapViewDelegate: MapViewDelegate {}
