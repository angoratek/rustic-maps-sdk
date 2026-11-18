//! The map-control support values (docs/ReferenceAppUX.md §4.2,
//! §5.3): the scale bar's readout and the pre-trip camera snapshot the follow
//! camera restores after. The scale-bar ladder and the circular course median
//! are generated from the refapp-only `map_refapp_core` contract; this file
//! only adapts their values to the view's types.

import CoreGraphics
import Foundation
import MapNativeCoreUI

/// The scale bar's readout for one drawn camera: the clean ladder distance
/// and the pixel width it renders at.
struct ScaleBarReadout: Equatable {
    /// The bar's labeled distance, in meters (a 1-2-3-5-10-ladder value).
    let distanceMeters: Double
    /// The bar's rendered width in pixels — the honest width the distance
    /// measures at the camera (never clamped into the target range).
    let barWidthPixels: CGFloat

    /// Picks the ladder readout for one camera: `groundMetersPerPixel` per
    /// the contract helper's value at the drawn camera and the §2.4
    /// pixel-width target range.
    ///
    /// Returns nil for a non-finite or non-positive ground answer, or when no
    /// ladder candidate reaches the minimum width; the bar then renders no
    /// distance rather than an invented one.
    static func readout(
        groundMetersPerPixel: Double,
        minimumWidthPixels: CGFloat,
        maximumWidthPixels: CGFloat
    ) -> ScaleBarReadout? {
        guard let selection = scaleBarSelection(
            groundMetersPerScreenPixel: groundMetersPerPixel,
            minimumWidthPixels: Double(minimumWidthPixels),
            maximumWidthPixels: Double(maximumWidthPixels)
        ) else {
            return nil
        }
        return ScaleBarReadout(
            distanceMeters: selection.distanceMeters,
            barWidthPixels: CGFloat(selection.widthPixels)
        )
    }
}

/// The bearing a camera carries modulo the full turn, wrapped into the
/// [0, 360) range the camera constructor takes (structural math).
func wrappedBearingDegrees(_ bearingDegrees: Double) -> Double {
    let fullTurnDegrees: Double = 360
    var wrappedBearing = bearingDegrees.truncatingRemainder(dividingBy: fullTurnDegrees)
    if wrappedBearing < 0 {
        wrappedBearing += fullTurnDegrees
    }
    return wrappedBearing
}

/// The pre-trip camera snapshot (§5.3: the framing camera is the pre-trip
/// state, the follow camera the in-trip state; the follow camera is
/// "applied at trip start, restored after").
struct PreTripCameraSnapshot {
    /// The camera values the trip's start interrupted (a fitted framing's
    /// readback is part of these values).
    let cameraValues: ReferenceCameraValues
    /// The selected camera preset the trip interrupted.
    let selectedPresetIdentifier: String?
    /// The 2D/3D pitch mode the trip interrupted (trips start on 3D, §5.3).
    let isThreeDimensionalPitchEnabled: Bool
}
