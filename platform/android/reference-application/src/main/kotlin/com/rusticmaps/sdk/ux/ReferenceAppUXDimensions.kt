package com.rusticmaps.sdk.ux

import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/**
 * The layout constants of `docs/ReferenceAppUX.md` §2.4 and §1 as named
 * constants (the Kotlin counterpart of the CSS token block). Values are
 * density-independent: the design table states its sizes in CSS pixels, which
 * is the same density-independent unit Compose spells `dp`.
 */
object ReferenceAppUXDimensions {
    /** Viewport edge to panel edge, horizontal (`gutter-inline`). */
    val GutterInline: Dp = 16.dp

    /** Viewport edge to panel edge, vertical (`gutter-block`). */
    val GutterBlock: Dp = 16.dp

    /** Inner padding of every floating panel (`panel-padding`). */
    val PanelPadding: Dp = 14.dp

    /** Space between controls inside one group (`stack-tight`). */
    val StackTight: Dp = 8.dp

    /** Space between groups inside one panel (`stack-group`). */
    val StackGroup: Dp = 12.dp

    /**
     * The diameter every circular map control shares (`map-control-diameter`):
     * zoom, my-location, 2D/3D, the layers control and the attribution control
     * all draw at this size.
     */
    val MapControlDiameter: Dp = 40.dp

    /** Corner radius of floating panels (`radius-surface`). */
    val RadiusSurface: Dp = 12.dp

    /** Corner radius of cards, embedded boxes and buttons (`radius-control`). */
    val RadiusControl: Dp = 8.dp

    /** Corner radius of the status line, chips and small reason strips (`radius-inline`). */
    val RadiusInline: Dp = 6.dp

    /**
     * Viewport width under which the phone-width responsive rule of §1
     * applies (`PHONE_WIDTH_BREAKPOINT_PIXELS = 560`).
     */
    val PhoneWidthBreakpoint: Dp = 560.dp

    /** Upper height cap of the sheet's full state (`PANEL_MAXIMUM_HEIGHT_FRACTION`, §1: 0.72). */
    const val PANEL_MAXIMUM_HEIGHT_FRACTION: Float = 0.72f

    /** The cap at phone width (§1: 0.72 — the same cap on both widths in v2). */
    const val PANEL_MAXIMUM_HEIGHT_FRACTION_PHONE: Float = 0.72f

    /** The home sheet's half state, as a fraction of the viewport height (`sheet-half-height`, §2.4). */
    const val SHEET_HALF_HEIGHT_FRACTION: Float = 0.5f

    /** The half state's phone-width cap (`sheet-half-phone-height`, §2.4). */
    const val SHEET_HALF_PHONE_HEIGHT_FRACTION: Float = 0.55f

    /**
     * Rows of the home sheet's peek state (`sheet-peek-content-rows`, §2.4):
     * the destination search field plus one primary action row. The peek's
     * own height is content-driven (§1), never a viewport fraction.
     */
    const val SHEET_PEEK_CONTENT_ROWS: Int = 2

    /**
     * The archetypal framed camera's field of view (`field-of-view-degrees` §2.4),
     * mirroring `rustic-maps-rendering`'s default; everything the reference
     * application projects through the SDK camera uses this value.
     */
    const val FIELD_OF_VIEW_DEGREES: Double = 60.0

    /**
     * Characters a search query needs before type-ahead asks the provider
     * (`autocomplete-minimum-query-characters`).
     */
    const val AUTOCOMPLETE_MINIMUM_QUERY_CHARACTERS: Int = 3

    /**
     * Quiet period between the last keystroke and the type-ahead request
     * (`autocomplete-debounce-milliseconds`).
     */
    const val AUTOCOMPLETE_DEBOUNCE_MILLISECONDS: Long = 250

    /** The type-ahead row cap (`autocomplete-maximum-results`). */
    const val AUTOCOMPLETE_MAXIMUM_RESULTS: Int = 5

    // -------------------------------------------------------------------
    // The round-10 map-control tokens (docs/ReferenceAppUX.md §2.4). Values
    // mirror the table exactly; the Kotlin names carry the token names.
    // -------------------------------------------------------------------

    /**
     * The navigation follow camera's base zoom (`navigation-follow-zoom`, §5.3),
     * locked deeper than the navigation profile's own zoom ladder so turn
     * geometry stays legible (§2.4's product note).
     */
    const val NAVIGATION_FOLLOW_ZOOM: Double = 17.0

    /**
     * The fixed follow zoom's clamp toward the trip's end
     * (`navigation-follow-zoom-maximum`, §5.3): the follow zoom ramps from
     * the base value to this maximum as the remaining trip distance runs out.
     */
    const val NAVIGATION_FOLLOW_ZOOM_MAXIMUM: Double = 17.5

    /**
     * The 3D follow camera's pitch (`navigation-follow-pitch-3d`, §5.3); the
     * map control cluster's 2D/3D toggle swings between this value and zero.
     */
    const val NAVIGATION_FOLLOW_PITCH_3D_DEGREES: Double = 45.0

    /** One tap of the zoom +/− control (`map-control-zoom-step`, §2.4). */
    const val MAP_CONTROL_ZOOM_STEP: Double = 1.0

    /**
     * The held zoom button's repeat cadence (`map-control-zoom-repeat-interval`,
     * §2.4), in Compose frame-friendly milliseconds.
     */
    const val MAP_CONTROL_ZOOM_REPEAT_INTERVAL_MILLISECONDS: Long = 250

    /**
     * The compass control appears only while the camera's bearing is off
     * north by at least this much (`compass-visibility-bearing-threshold`,
     * §2.4), in degrees.
     */
    const val COMPASS_VISIBILITY_BEARING_THRESHOLD_DEGREES: Double = 1.0

    /**
     * The eased "north up" return when the compass is tapped
     * (`compass-renorth-animation`, §2.4), in milliseconds; the language's
     * one eased-camera-move cadence, so the 2D/3D pitch toggle's swing eases
     * over the same duration (§5.3's pump-easing note).
     */
    const val EASED_CAMERA_MOVE_DURATION_MILLISECONDS: Long = 300

    /**
     * The longest motion one location fix may start
     * (`location-animation-duration-milliseconds`, §2.4), in milliseconds:
     * the actual duration is the interval between fixes, clamped to this
     * ceiling.
     */
    const val LOCATION_ANIMATION_DURATION_MILLISECONDS: Long = 1000

    /**
     * The shortest motion one location fix starts
     * (`location-animation-minimum-milliseconds`, §2.4), in milliseconds, so
     * a fast fix stream still animates instead of teleporting.
     */
    const val LOCATION_ANIMATION_MINIMUM_MILLISECONDS: Long = 120

    /**
     * The scale bar's target width range in physical viewport pixels
     * (`scale-bar-width-range`, §2.4): the bar's chosen distance is one a
     * clean 1-2-3-5-10 ladder multiple whose bar width lands in this range.
     */
    const val SCALE_BAR_WIDTH_MINIMUM_PIXELS: Double = 50.0
    const val SCALE_BAR_WIDTH_MAXIMUM_PIXELS: Double = 120.0
}