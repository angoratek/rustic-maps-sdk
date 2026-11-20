//! The §2.4 spacing, radii, gutter and framing tokens of the design language
//! as named constants (§8: "radii/spacings become an `AppLayout` enum of named
//! constants"), plus the responsive-rule constants (§1) that no layout may
//! invent ad hoc.

import SwiftUI

/// The design language's named layout constants (§2.4 and the §1 responsive
/// rule). Every floating panel and control in the reference applications
/// reads its metrics from here; nothing outside this table is a design
/// decision.
public enum AppLayout {
    // MARK: §2.4 spacing tokens

    /// `gutter-inline`: viewport edge → panel edge, horizontal (16px).
    public static let gutterInline: CGFloat = 16
    /// `gutter-block`: viewport edge → panel edge, vertical (16px).
    public static let gutterBlock: CGFloat = 16
    /// `panel-padding`: panel inner padding (14px).
    public static let panelPadding: CGFloat = 14
    /// `stack-tight`: between controls inside a group (8px).
    public static let stackTight: CGFloat = 8
    /// `stack-group`: between groups inside a panel (12px).
    public static let stackGroup: CGFloat = 12
    /// `map-control-diameter`: every circular map control's edge, including
    /// zoom, my-location, 2D/3D, the layers control and attribution (40px).
    public static let mapControlDiameter: CGFloat = 40

    // MARK: §2.4 radius tokens

    /// `radius-surface`: panels (12px).
    public static let radiusSurface: CGFloat = 12
    /// `radius-control`: cards, embedded boxes, buttons (8px).
    public static let radiusControl: CGFloat = 8
    /// `radius-inline`: status line, chips, small reasons (6px).
    public static let radiusInline: CGFloat = 6

    // MARK: §2.4 elevation token

    /// `elevation-surface`: `0 1px 4px rgba(0, 0, 0, 0.25)`, decomposed into
    /// the shadow parameters the floating panels carry.
    public static let elevationSurfaceColor: Color = Color.black.opacity(
        elevationSurfaceOpacity
    )
    /// The elevation's vertical offset in points.
    public static let elevationSurfaceOffsetY: CGFloat = 1
    /// The elevation's blur radius in points.
    public static let elevationSurfaceRadius: CGFloat = 4
    /// The elevation shadow's opacity channel (`rgba(0, 0, 0, 0.25)`).
    private static let elevationSurfaceOpacity: Double = 0.25

    /// Characters a search query needs before type-ahead asks the provider
    /// (`autocomplete-minimum-query-characters`).
    public static let autocompleteMinimumQueryCharacters: Int = 3

    /// Quiet period between the last keystroke and the type-ahead request
    /// (`autocomplete-debounce-milliseconds`).
    public static let autocompleteDebounceMilliseconds: TimeInterval = 250

    /// The type-ahead row cap (`autocomplete-maximum-results`).
    public static let autocompleteMaximumResults: Int = 5

    // MARK: §2.4 map-control-cluster tokens

    /// `map-control-zoom-step` (1 zoom level): one tap of the zoom −/+ control.
    public static let mapControlZoomStep: Double = 1
    /// `map-control-zoom-repeat-interval` (250 ms): holding a zoom button
    /// repeats its step at this cadence.
    public static let mapControlZoomRepeatInterval: TimeInterval = 0.25
    /// `compass-visibility-bearing-threshold` (1 degree): the compass control
    /// appears only while the camera's bearing is off north by at least this
    /// much.
    public static let compassVisibilityBearingThresholdDegrees: Double = 1
    /// `compass-renorth-animation` (300 ms): the eased "north up" return when
    /// the compass is tapped (the ease runs in the platform frame pump).
    public static let compassRenorthAnimationSeconds: TimeInterval = 0.3
    /// `location-animation-duration-milliseconds` (1000 ms): the longest
    /// motion one location fix may start.
    public static let locationAnimationDurationMilliseconds: Double = 1000
    /// `location-animation-minimum-milliseconds` (120 ms): the shortest motion
    /// one fix starts, so a fast fix stream still animates.
    public static let locationAnimationMinimumMilliseconds: Double = 120
    /// `location-animation-dead-reckoning` (off): the puck never predicts
    /// motion between fixes.
    public static let locationAnimationDeadReckoning: String = "off"
    /// `scale-bar-width-range` (50–120 px): the scale bar targets a width in
    /// this range, then picks a clean distance from the 1-2-3-5-10 ladder.
    public static let scaleBarMinimumWidthPixels: CGFloat = 50
    /// The range's upper end (120 px).
    public static let scaleBarMaximumWidthPixels: CGFloat = 120

    // MARK: §2.4 home-sheet tokens

    /// `sheet-half-height`: the home sheet's half state as a fraction of
    /// the viewport height (0.5).
    public static let sheetHalfHeightFraction: CGFloat = 0.5
    /// `sheet-half-phone-height`: the half state's phone-width cap (0.55 of
    /// the viewport height so the map stays the larger half, §1).
    public static let sheetHalfPhoneHeightFraction: CGFloat = 0.55
    /// `sheet-peek-content-rows` (2): the peek state's row count (the
    /// destination search field and the primary action row); the peek
    /// height is content-driven, never a viewport fraction.
    public static let sheetPeekContentRows: Int = 2

    // MARK: §1 responsive rule

    /// `PHONE_WIDTH_BREAKPOINT_PIXELS` (560): below this viewport width the
    /// phone-width layout applies (panels span start–end, the readout
    /// compresses, the bottom dock stacks).
    public static let phoneWidthBreakpointPixels: CGFloat = 560

    /// `PANEL_MAXIMUM_HEIGHT_FRACTION`: the *full*-state home sheet's cap
    /// as a fraction of the viewport height (§1: the scrolled full state is
    /// capped "at `PANEL_MAXIMUM_HEIGHT_FRACTION` of the viewport height").
    public static let panelMaximumHeightFraction: CGFloat = 0.8
    /// The tightened full-state cap at phone width (§1: 0.72 "so the
    /// attribution and any docked dock stay visible").
    public static let phonePanelMaximumHeightFraction: CGFloat = 0.72
}

/// The home sheet's three fold states and the navigation drawer's implicit
/// replacement of the sheet during a simulated trip (§1's v2 archetype and
/// §4.2's Home sheet component). Shared so every native reference
/// application spells the states identically.
public enum ReferenceAppUXSheetState: String, CaseIterable, Sendable {
    /// The fold: the destination search field and the primary action row.
    case peek
    /// The working content, grown to `sheet-half-height`.
    case half
    /// The scrolled content (the search view), capped at the full-state cap.
    case full
}