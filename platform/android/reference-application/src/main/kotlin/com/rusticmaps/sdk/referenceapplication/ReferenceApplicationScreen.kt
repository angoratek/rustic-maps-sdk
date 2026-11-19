package com.rusticmaps.sdk.referenceapplication

import android.Manifest
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.wrapContentWidth
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.rusticmaps.sdk.compose.MapView
import com.rusticmaps.sdk.demo.CameraPreset
import com.rusticmaps.sdk.ProviderCredentialDescriptor
import com.rusticmaps.sdk.ux.LocalReferenceAppUXExtendedColors
import com.rusticmaps.sdk.ux.ReferenceAppUXDimensions
import com.rusticmaps.sdk.ux.numericReadoutStyle
import com.rusticmaps.sdk.referenceapplication.console.DeveloperConsoleControl
import com.rusticmaps.sdk.referenceapplication.console.DeveloperConsoleSheet
import com.rusticmaps.sdk.referenceapplication.map.CompassControl
import com.rusticmaps.sdk.referenceapplication.map.MapControlCluster
import com.rusticmaps.sdk.referenceapplication.map.MapLayersCollapsedControl
import com.rusticmaps.sdk.referenceapplication.map.MapLayersPanel
import com.rusticmaps.sdk.referenceapplication.map.ScaleBarOverlay
import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationState.ReadinessState
import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationState.HomeSheetState
import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationState.PlanSearchField
import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationState.SheetContentMode
import uniffi.rustic_maps.DeviationStatusChangeKind
import uniffi.rustic_maps.DistancePhrase
import uniffi.rustic_maps.DistancePhraseKind
import uniffi.rustic_maps.DistanceUnit
import uniffi.rustic_maps.NavigationPhase
import uniffi.rustic_maps.NavigationPhaseChange
import uniffi.rustic_maps.Place
import uniffi.rustic_maps.ProviderContract
import uniffi.rustic_maps.instructionBannerText
import kotlin.math.floor

/**
 * Private layout constants of the screen: widths and clearances the archetype
 * does not fix in the token table (docs/ReferenceAppUX.md §1 names the sheet
 * and its states, not the banner's exact width). The token dimensions
 * (gutters, paddings, spacings, radii, sheet fractions) always come from
 * [ReferenceAppUXDimensions].
 */
private object ScreenLayoutConstants {
    /** Width of the top-center turn banner at wide width. */
    val TURN_BANNER_WIDTH: Dp = 360.dp

    /**
     * Top clearance of the banner stack, keeping it clear of the readiness
     * dock: the chip row plus the credential status line below it (§6.5).
     */
    val TURN_BANNER_TOP_CLEARANCE: Dp = 56.dp

    /** Hairline border width of the sheet, cards, boxes and inline chips. */
    val HAIRLINE_BORDER_WIDTH: Dp = 1.dp

    /** The attribution info control's circular size (`map-control-diameter`). */
    val ATTRIBUTION_INFO_CONTROL_SIZE: Dp = ReferenceAppUXDimensions.MapControlDiameter

    /** The attribution panel's readable maximum width. */
    val ATTRIBUTION_PANEL_MAXIMUM_WIDTH: Dp = 300.dp

    /** The sheet grabber's width and height. */
    val GRABBER_WIDTH: Dp = 40.dp
    val GRABBER_HEIGHT: Dp = 4.dp

    /** The grabber's internal touch strip height (the tap + drag target). */
    val GRABBER_TOUCH_STRIP_HEIGHT: Dp = 20.dp

    /** Vertical drag the grabber needs to travel before the sheet changes state. */
    val GRABBER_STATE_SWITCH_DRAG_DISTANCE: Dp = 40.dp

    /** Max lines of the composed display text and the readout label lines. */
    const val DISPLAY_TEXT_MAX_LINES: Int = 2

    /** Max lines of the verbatim error-strip detail (nothing is clipped away). */
    const val ERROR_STRIP_MAX_LINES: Int = 6

    /** Max lines of the info-affordance detail under a demo scene or plan. */
    const val INFO_DETAIL_MAX_LINES: Int = 3

    /** The camera bearing slider's full circle in degrees (the wide diagnostics row). */
    const val CAMERA_BEARING_SLIDER_FULL_CIRCLE_DEGREES: Double = 360.0

    /**
     * The seek control's smallest range, in meters: a zero-length route still
     * renders a valid slider range (its only value is the destination).
     */
    const val MINIMUM_SEEK_RANGE_METERS: Float = 1f

    /** Length constants of the friendly distance and duration formatting. */
    const val METERS_PER_KILOMETER: Double = 1000.0
    const val SECONDS_PER_MINUTE: Double = 60.0

    /** The search field's weight inside the peek's single-line row. */
    const val SEARCH_FIELD_WEIGHT: Float = 1f
}

/**
 * The layout archetype of `docs/ReferenceAppUX.md` §1 (v2): the map fills the
 * viewport, the readiness chip docks top-start, the turn-banner stack sits
 * top-center during a simulated trip, the attribution notice and the error
 * strip stack above the one bottom dock, and the dock is the home sheet
 * (peek/half/full) or — during a simulated trip — the navigation drawer. The
 * diagnostics sheet opens from the readiness chip and floats above.
 */
@Composable
fun ReferenceApplicationScreen(applicationState: ReferenceApplicationState) {
    BackHandler {
        applicationState.collapseTowardPeek()
    }
    // The attribution panel's open state (§1/§4.2): a temporary reveal of the
    // compliance text, dismissed by its control, the system back gesture, or
    // a tap outside it.
    var isAttributionPanelOpen by remember { mutableStateOf(false) }
    BackHandler(enabled = isAttributionPanelOpen) {
        isAttributionPanelOpen = false
    }
    // The layer panel's close paths (the platform back action and its own
    // Close control) return focus to the collapsed control that opened it.
    val mapLayersControlFocusRequester = remember { FocusRequester() }
    var wasMapLayersPanelOpen by remember { mutableStateOf(false) }
    BackHandler(enabled = applicationState.mapLayersState.isPanelOpen) {
        applicationState.closeMapLayersPanel()
    }
    // The console's back affordance returns from a typed detail first, then
    // closes the console (the same two-step rule as the platform back action).
    BackHandler(enabled = applicationState.isProviderConsoleOpen) {
        if (applicationState.selectedProviderConsoleEntry != null) {
            applicationState.closeProviderConsoleDetail()
        } else {
            applicationState.dismissProviderConsole()
        }
    }
    val isMapLayersPanelOpen = applicationState.mapLayersState.isPanelOpen
    LaunchedEffect(isMapLayersPanelOpen) {
        if (isMapLayersPanelOpen) {
            wasMapLayersPanelOpen = true
        } else if (wasMapLayersPanelOpen) {
            wasMapLayersPanelOpen = false
            // The panel's focus node leaves composition in this frame; one
            // frame of settlement lets the control take focus back. A control
            // the composition has not attached yet cannot take focus, so the
            // next open/close pair retries.
            withFrameNanos { }
            try {
                mapLayersControlFocusRequester.requestFocus()
            } catch (_: IllegalStateException) {
                Unit
            }
        }
    }
    BoxWithConstraints(modifier = Modifier.fillMaxSize()) {
        val viewportHeight = maxHeight
        val isPhoneLayout = maxWidth < ReferenceAppUXDimensions.PhoneWidthBreakpoint

        // The map is the screen's background layer (§1). The shipped,
        // lifecycle-aware MapView owns the surface, the Choreographer pump,
        // resize, gestures and the camera animation (M17); the state supplies
        // the SDK, the callbacks and the initial camera values.
        MapView(
            sdk = applicationState.nativeMapSdk,
            listener = applicationState.mapViewListener,
            onViewCreated = { view -> applicationState.onMapViewReady(view) },
            modifier = Modifier.fillMaxSize(),
        )

        // The map control cluster (§4.2, round 10): the end-edge stack, the
        // top-end compass, and the bottom-start scale bar float over the map
        // under the sheet dock (declared before the docks, so they never
        // draw above them).
        MapControlCluster(
            applicationState = applicationState,
            modifier = Modifier
                .align(Alignment.CenterEnd)
                .padding(end = ReferenceAppUXDimensions.GutterInline),
        )
        CompassControl(
            applicationState = applicationState,
            isNavigationMode = applicationState.isNavigationInTrip,
            modifier = Modifier
                .align(Alignment.TopEnd)
                .padding(end = ReferenceAppUXDimensions.GutterInline),
        )

        // The overlay set follows the scene toggles and the active plan; a
        // push failure is reported verbatim in the state (§6.3).
        val sceneToggleSnapshot = applicationState.sceneToggleStates.toMap()
        val activeSimulatedTrace = applicationState.activeSimulatedTrace
        LaunchedEffect(sceneToggleSnapshot, activeSimulatedTrace) {
            applicationState.pushOverlaysToSurface()
        }
        // The phase chip follows the phase topic (§7.2: the chip is the state
        // form; read here so a missed topic still resolves from the controller)
        // and the walker's own arrival report.
        val phaseChangeSnapshot = applicationState.navigationListener.phaseChangeState.value
        LaunchedEffect(phaseChangeSnapshot, applicationState.tripDriver.hasArrived) {
            applicationState.refreshNavigationPhaseChip()
        }
        // Spoken guidance follows the speech-instruction topic (round 10,
        // OQ5: the SDK's own trigger policy decides when a line is spoken;
        // the platform keeps playback and the verbatim rule) — the effect
        // keys on both the speech delivery and the instruction payload, so
        // the round-9 payload fallback still fires when no listener is
        // subscribed, while a subscribed speech topic alone speaks.
        val speechInstructionSnapshot = applicationState.navigationListener.speechInstructionState.value
        val instructionSnapshot = applicationState.navigationListener.instructionState.value
        LaunchedEffect(speechInstructionSnapshot, instructionSnapshot) {
            applicationState.speakTopicGuidanceIfNeeded()
        }
        // The simulated playback's end: an arrival keeps the destination
        // presentation (the trip does not snap back), while a trace whose
        // samples drained behind a reroute restores the pre-trip camera
        // (§5.3: restored after) — a user exit restores through the exit
        // control itself.
        val isTripPlaybackActive = applicationState.tripDriver.isTripActive
        LaunchedEffect(isTripPlaybackActive) {
            if (!isTripPlaybackActive) {
                applicationState.onSimulatedPlaybackEnded()
            }
        }

        // Top-start dock: the readiness chip (the one readiness signal, §1)
        // with the provider-settings gear beside it, and the neutral
        // credential line underneath (§6.5: keyless is a first-class state).
        Column(
            modifier = Modifier
                .align(Alignment.TopStart)
                .padding(ReferenceAppUXDimensions.GutterInline),
            verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
        ) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                ReadinessChip(applicationState = applicationState)
                ProviderSettingsControl(applicationState = applicationState)
            }
            if (!applicationState.isProviderKeyless) {
                // The chip carries the keyless prompt; the line reports the
                // applied summary once there is one (§6.5).
                CredentialStatusLine(applicationState = applicationState)
            }
        }

        // Top-center dock: the banner stack during a simulated trip (§7.2).
        val isNavigationMode = applicationState.isNavigationInTrip
        if (isNavigationMode) {
            TurnBannerStack(
                applicationState = applicationState,
                isPhoneLayout = isPhoneLayout,
                modifier = Modifier
                    .align(Alignment.TopCenter)
                    .padding(
                        top = ReferenceAppUXDimensions.GutterBlock + ScreenLayoutConstants.TURN_BANNER_TOP_CLEARANCE,
                    ),
            )
        }

        // A tap outside the open attribution panel dismisses it (§1's panel
        // close rule); the scrim sits above the map and controls but below
        // the bottom dock that carries the panel itself.
        if (isAttributionPanelOpen) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .pointerInput(Unit) {
                        detectTapGestures { isAttributionPanelOpen = false }
                    },
            )
        }

        // Bottom dock: error strip, then the sheet or the drawer (the
        // attribution info control rides the dock's bottom-start stack).
        BottomDock(
            applicationState = applicationState,
            isPhoneLayout = isPhoneLayout,
            viewportHeight = viewportHeight,
            isAttributionPanelOpen = isAttributionPanelOpen,
            onToggleAttributionPanel = {
                isAttributionPanelOpen = !isAttributionPanelOpen
            },
            mapLayersControlFocusRequester = mapLayersControlFocusRequester,
            modifier = Modifier.align(Alignment.BottomCenter),
        )

        // The layer panel floats above the map and the dock but below the
        // diagnostics and settings sheets; it opens upward over the map's
        // end edge, directly above the control that opened it.
        if (isMapLayersPanelOpen) {
            MapLayersPanel(
                applicationState = applicationState,
                viewportHeight = viewportHeight,
                modifier = Modifier.align(Alignment.BottomEnd),
            )
        }

        // The diagnostics sheet (§6.1) floats above the whole dock — it is
        // declared after the dock so it never draws under the open sheet.
        if (applicationState.isDiagnosticsOpen) {
            DiagnosticsSheet(
                applicationState = applicationState,
                isPhoneLayout = isPhoneLayout,
                viewportHeight = viewportHeight,
                modifier = Modifier.align(Alignment.BottomCenter),
            )
        }

        // The developer console floats above the dock as a full-height sheet
        // on a phone and a readable side panel on a wide viewport. Its Export
        // hands the filtered text to the platform share sheet.
        if (applicationState.isProviderConsoleOpen) {
            DeveloperConsoleSheet(
                applicationState = applicationState,
                isPhoneLayout = isPhoneLayout,
                viewportHeight = viewportHeight,
                modifier = Modifier
                    .align(
                        if (isPhoneLayout) Alignment.BottomCenter else Alignment.BottomEnd,
                    )
                    .then(
                        if (isPhoneLayout) {
                            Modifier
                        } else {
                            Modifier.padding(
                                end = ReferenceAppUXDimensions.GutterInline,
                            )
                        },
                    ),
            )
        }

        // The provider-settings sheet (§6.5) draws above everything: it is the
        // gear's act and carries the runtime credential rows.
        if (applicationState.isSettingsOpen) {
            ProviderSettingsSheet(
                applicationState = applicationState,
                isPhoneLayout = isPhoneLayout,
                viewportHeight = viewportHeight,
                modifier = Modifier.align(Alignment.BottomCenter),
            )
        }
    }
}

/**
 * The bottom dock (§1): the map band (scale bar at the start edge, the
 * attribution info control at the end edge), then the error strip, then the
 * sheet or the drawer. The attribution is a compact info control now, so it
 * no longer claims a full line above the dock; its panel opens upward on
 * request.
 */
@Composable
private fun BottomDock(
    applicationState: ReferenceApplicationState,
    isPhoneLayout: Boolean,
    viewportHeight: Dp,
    isAttributionPanelOpen: Boolean,
    onToggleAttributionPanel: () -> Unit,
    mapLayersControlFocusRequester: FocusRequester,
    modifier: Modifier = Modifier,
) {
    val dockWidthModifier = if (isPhoneLayout) {
        Modifier.fillMaxWidth() // full-bleed below 560 px (§1's phone rule)
    } else {
        Modifier.fillMaxWidth().padding(horizontal = ReferenceAppUXDimensions.GutterInline)
    }
    // The map band keeps the map's inline gutter on every width, so the scale
    // bar and the attribution never touch the viewport edge (the full-bleed
    // phone dock leaves the inset to this row; the desktop dock already
    // applies it, hence the zero). The iOS reference app's control row applies
    // the same rule.
    val mapBandInset = if (isPhoneLayout) ReferenceAppUXDimensions.GutterInline else 0.dp
    Column(
        modifier = modifier.then(dockWidthModifier),
        verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
    ) {
        // The map's bottom band (§4.2): the scale bar at the start edge and
        // the compact attribution info control at the end edge, so neither
        // crowds the other and the dock's width stays the search surface. The
        // weighted spacer keeps the attribution pinned to the end edge while
        // the scale bar renders nothing (no ground answer before the first
        // draw) — a fixed arrangement would drag it to the start edge.
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = mapBandInset),
            verticalAlignment = Alignment.Bottom,
        ) {
            // The developer console's floating control sits on the start edge,
            // above the scale bar and opposite the map layer control, so it is
            // one tap from the map without crowding the attribution notice.
            Column(
                horizontalAlignment = Alignment.Start,
                verticalArrangement = Arrangement.spacedBy(
                    ReferenceAppUXDimensions.StackTight,
                ),
            ) {
                DeveloperConsoleControl(applicationState = applicationState)
                ScaleBarOverlay(applicationState = applicationState)
            }
            Spacer(modifier = Modifier.weight(1f))
            // The layer control sits directly above the attribution notice,
            // as the specification places it on every target.
            Column(
                horizontalAlignment = Alignment.End,
                verticalArrangement = Arrangement.spacedBy(
                    ReferenceAppUXDimensions.StackTight,
                ),
            ) {
                MapLayersCollapsedControl(
                    applicationState = applicationState,
                    focusRequester = mapLayersControlFocusRequester,
                )
                AttributionInfoControl(
                    attributionText = applicationState.attributionText,
                    termsNotes = applicationState.attributionTermsNotes,
                    isPanelOpen = isAttributionPanelOpen,
                    onTogglePanel = onToggleAttributionPanel,
                )
            }
        }
        ErrorStrip(applicationState = applicationState)
        if (applicationState.isNavigationInTrip) {
            NavigationDrawer(applicationState = applicationState)
        } else {
            HomeSheet(
                applicationState = applicationState,
                isPhoneLayout = isPhoneLayout,
                viewportHeight = viewportHeight,
            )
        }
    }
}

// ---------------------------------------------------------------------------
// The home sheet (docs/ReferenceAppUX.md §1's three states)
// ---------------------------------------------------------------------------

/**
 * The home sheet: grabber on top, then — per [HomeSheetState] — the peek's
 * two content rows, or the scrollable working content grown to the half/full
 * fractions. The peek's height is content-driven (§1); half and full are
 * viewport fractions.
 */
@Composable
private fun HomeSheet(
    applicationState: ReferenceApplicationState,
    isPhoneLayout: Boolean,
    viewportHeight: Dp,
) {
    val sheetState = applicationState.homeSheetState
    val isSearchMode = applicationState.sheetContentMode == SheetContentMode.SEARCH
    val sheetFraction: Float? = when {
        isSearchMode -> ReferenceAppUXDimensions.PANEL_MAXIMUM_HEIGHT_FRACTION
        sheetState == HomeSheetState.HALF -> if (isPhoneLayout) {
            ReferenceAppUXDimensions.SHEET_HALF_PHONE_HEIGHT_FRACTION
        } else {
            ReferenceAppUXDimensions.SHEET_HALF_HEIGHT_FRACTION
        }
        sheetState == HomeSheetState.FULL -> ReferenceAppUXDimensions.PANEL_MAXIMUM_HEIGHT_FRACTION
        else -> null
    }
    val sheetShape = RoundedCornerShape(
        topStart = ReferenceAppUXDimensions.RadiusSurface,
        topEnd = ReferenceAppUXDimensions.RadiusSurface,
    )
    val surfaceBackgroundColor = MaterialTheme.colorScheme.surface
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .then(
                if (sheetFraction == null) {
                    Modifier
                } else {
                    Modifier.height(viewportHeight * sheetFraction)
                },
            )
            .clip(shape = sheetShape)
            .background(color = surfaceBackgroundColor)
            .border(
                width = ScreenLayoutConstants.HAIRLINE_BORDER_WIDTH,
                color = LocalReferenceAppUXExtendedColors.current.surfaceBorder,
                shape = sheetShape,
            )
            .padding(horizontal = ReferenceAppUXDimensions.PanelPadding)
            .animateContentSize(),
        verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
    ) {
        SheetGrabber(
            onStateRequested = { grow ->
                when (sheetState) {
                    HomeSheetState.PEEK ->
                        applicationState.requestHomeSheetState(
                            if (grow) HomeSheetState.HALF else HomeSheetState.PEEK,
                        )
                    HomeSheetState.HALF ->
                        applicationState.requestHomeSheetState(
                            if (grow) HomeSheetState.FULL else HomeSheetState.PEEK,
                        )
                    HomeSheetState.FULL ->
                        applicationState.requestHomeSheetState(
                            if (grow) HomeSheetState.FULL else HomeSheetState.HALF,
                        )
                }
            },
            onStateAdvance = { applicationState.advanceSheetState() },
        )
        if (isSearchMode) {
            PlanSearchView(
                applicationState = applicationState,
                modifier = Modifier
                    .fillMaxWidth()
                    .verticalScroll(rememberScrollState()),
            )
        } else if (sheetState == HomeSheetState.PEEK) {
            HomeSheetPeekContent(applicationState = applicationState)
        } else {
            HomeSheetWorkingContent(
                applicationState = applicationState,
                modifier = Modifier
                    .fillMaxWidth()
                    .verticalScroll(rememberScrollState()),
            )
        }
    }
}

/**
 * The sheet grabber (§4.2: the sheet's only gesture target): tap cycles the
 * states, a vertical drag of the named travel distance steps the state up or
 * down. Centered, with `panel-padding` from each top corner.
 */
@Composable
private fun SheetGrabber(
    onStateRequested: (grow: Boolean) -> Unit,
    onStateAdvance: () -> Unit,
) {
    var verticalDragAccumulationPixels: Float by remember { mutableStateOf(0f) }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .height(ScreenLayoutConstants.GRABBER_TOUCH_STRIP_HEIGHT)
            .pointerInput(Unit) {
                detectTapGestures { _ -> onStateAdvance() }
            }
            .pointerInput(Unit) {
                detectVerticalDragGestures(
                    onDragEnd = {
                        val switchDistancePixels =
                            ScreenLayoutConstants.GRABBER_STATE_SWITCH_DRAG_DISTANCE.toPx()
                        if (verticalDragAccumulationPixels <= -switchDistancePixels) {
                            onStateRequested(true)
                        } else if (verticalDragAccumulationPixels >= switchDistancePixels) {
                            onStateRequested(false)
                        }
                        verticalDragAccumulationPixels = 0f
                    },
                ) { change, dragAmount ->
                    change.consume()
                    verticalDragAccumulationPixels += dragAmount
                }
            },
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier
                .width(ScreenLayoutConstants.GRABBER_WIDTH)
                .height(ScreenLayoutConstants.GRABBER_HEIGHT)
                .clip(shape = RoundedCornerShape(ScreenLayoutConstants.GRABBER_HEIGHT / 2))
                .background(color = LocalReferenceAppUXExtendedColors.current.textSecondary),
        )
    }
}

/** The peek's exactly two content rows (§2.4's `sheet-peek-content-rows`). */
@Composable
private fun HomeSheetPeekContent(applicationState: ReferenceApplicationState) {
    // Row 1 — the destination search field (its only act: open the search view).
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusControl))
            .border(
                width = ScreenLayoutConstants.HAIRLINE_BORDER_WIDTH,
                color = MaterialTheme.colorScheme.outlineVariant,
                shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusControl),
            )
            .clickable { applicationState.openPlanSearch() }
            .testTag(ReferenceApplicationConstants.HOME_SHEET_PEEK_SEARCH_TEST_TAG)
            .padding(ReferenceAppUXDimensions.StackGroup),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = ReferenceApplicationConstants.DESTINATION_FIELD_HINT_TEXT,
            style = MaterialTheme.typography.titleSmall,
            color = LocalReferenceAppUXExtendedColors.current.textSecondary,
            modifier = Modifier.weight(ScreenLayoutConstants.SEARCH_FIELD_WEIGHT),
        )
    }
    // The trip source the action row's Start will replay: a planned route when
    // one stands, otherwise the selected demo plan, stated before the tap.
    Text(
        text = applicationState.tripSourceText,
        style = MaterialTheme.typography.labelSmall,
        color = LocalReferenceAppUXExtendedColors.current.textSecondary,
    )
    // Row 2 — the action row: the route's start (§1) beside the origin entry
    // that opens the search view armed for the origin. Before a route is
    // planned the start drives the selected demo plan's trip; with one
    // planned it starts that route. The origin entry is the same control the
    // React Native and iOS peeks carry, so both plan fields are one tap from
    // the home sheet.
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        PrimaryButton(
            labelText = ReferenceApplicationConstants.ROUTE_SUMMARY_START_BUTTON_TEXT,
            enabled = !applicationState.tripDriver.isTripActive,
            onClick = { applicationState.startSimulatedTrip() },
            modifier = Modifier
                .weight(ScreenLayoutConstants.SEARCH_FIELD_WEIGHT)
                .testTag(ReferenceApplicationConstants.START_SIMULATED_NAVIGATION_TEST_TAG),
        )
        SubtleButton(
            labelText = ReferenceApplicationConstants.ORIGIN_FIELD_HINT_TEXT,
            enabled = true,
            onClick = {
                applicationState.openPlanSearch(ReferenceApplicationState.PlanSearchField.ORIGIN)
            },
            // Wrap to the label's width inside the row: the button's default
            // is fillMaxWidth, which would take the whole row from the
            // weighted Start control (the iOS row's proportions).
            modifier = Modifier,
        )
    }
}

/** The search view: origin + destination fields over the shared provider rows. */
@Composable
private fun PlanSearchView(
    applicationState: ReferenceApplicationState,
    modifier: Modifier = Modifier,
) {
    // Each field's My location chip shares the one runtime permission path the
    // Location group uses: the grant fills the field that asked, never both.
    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted -> applicationState.onDeviceLocationPermissionResult(granted) }
    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight)) {
        // The search view's heading and its dismiss action (§4.1): the same
        // words the iOS and React Native search views show.
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = ReferenceApplicationConstants.PLAN_SEARCH_HEADING_TEXT,
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurface,
                modifier = Modifier.weight(ScreenLayoutConstants.SEARCH_FIELD_WEIGHT),
            )
            Text(
                text = ReferenceApplicationConstants.PLAN_SEARCH_DONE_ACTION_TEXT,
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier.clickable { applicationState.dismissPlanSearch() },
            )
        }
        LabeledSearchField(
            labelText = ReferenceApplicationConstants.ORIGIN_FIELD_LABEL_TEXT,
            placeholderText = ReferenceApplicationConstants.ORIGIN_FIELD_HINT_TEXT,
            value = applicationState.originSearchText,
            onValueChange = { value ->
                applicationState.onPlanSearchQueryChanged(PlanSearchField.ORIGIN, value)
            },
            onSearch = { applicationState.runPlanSearch(PlanSearchField.ORIGIN) },
            onMyLocation = {
                if (applicationState.hasDeviceLocationPermission()) {
                    applicationState.fillPlanFieldFromMyLocation(PlanSearchField.ORIGIN)
                } else {
                    applicationState.notePlanFieldMyLocationPermissionRequested(
                        PlanSearchField.ORIGIN,
                    )
                    permissionLauncher.launch(Manifest.permission.ACCESS_FINE_LOCATION)
                }
            },
            autocompleteState = applicationState.autocompleteViewState(PlanSearchField.ORIGIN),
            onAutocompleteSuggestionSelected = { place ->
                applicationState.selectAutocompleteSuggestion(PlanSearchField.ORIGIN, place)
            },
            autocompleteListTestTag =
                ReferenceApplicationConstants.PLAN_ORIGIN_AUTOCOMPLETE_LIST_TEST_TAG,
            autocompleteNoMatchTestTag =
                ReferenceApplicationConstants.PLAN_ORIGIN_AUTOCOMPLETE_NO_MATCH_TEST_TAG,
            autocompleteFailureTestTag =
                ReferenceApplicationConstants.PLAN_ORIGIN_AUTOCOMPLETE_FAILURE_TEST_TAG,
            fieldTestTag = ReferenceApplicationConstants.PLAN_ORIGIN_FIELD_TEST_TAG,
            myLocationTestTag = ReferenceApplicationConstants.PLAN_ORIGIN_MY_LOCATION_TEST_TAG,
            fieldMarkerText = "A",
        )
        LabeledSearchField(
            labelText = ReferenceApplicationConstants.DESTINATION_FIELD_LABEL_TEXT,
            placeholderText = ReferenceApplicationConstants.DESTINATION_FIELD_PLACEHOLDER_TEXT,
            value = applicationState.destinationSearchText,
            onValueChange = { value ->
                applicationState.onPlanSearchQueryChanged(PlanSearchField.DESTINATION, value)
            },
            onSearch = { applicationState.runPlanSearch(PlanSearchField.DESTINATION) },
            onMyLocation = {
                if (applicationState.hasDeviceLocationPermission()) {
                    applicationState.fillPlanFieldFromMyLocation(PlanSearchField.DESTINATION)
                } else {
                    applicationState.notePlanFieldMyLocationPermissionRequested(
                        PlanSearchField.DESTINATION,
                    )
                    permissionLauncher.launch(Manifest.permission.ACCESS_FINE_LOCATION)
                }
            },
            autocompleteState = applicationState.autocompleteViewState(PlanSearchField.DESTINATION),
            onAutocompleteSuggestionSelected = { place ->
                applicationState.selectAutocompleteSuggestion(PlanSearchField.DESTINATION, place)
            },
            autocompleteListTestTag =
                ReferenceApplicationConstants.PLAN_DESTINATION_AUTOCOMPLETE_LIST_TEST_TAG,
            autocompleteNoMatchTestTag =
                ReferenceApplicationConstants.PLAN_DESTINATION_AUTOCOMPLETE_NO_MATCH_TEST_TAG,
            autocompleteFailureTestTag =
                ReferenceApplicationConstants.PLAN_DESTINATION_AUTOCOMPLETE_FAILURE_TEST_TAG,
            fieldTestTag = ReferenceApplicationConstants.PLAN_DESTINATION_FIELD_TEST_TAG,
            myLocationTestTag = ReferenceApplicationConstants.PLAN_DESTINATION_MY_LOCATION_TEST_TAG,
            fieldMarkerText = "B",
        )
        // The Swap action exchanges the pair and never plans.
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.End,
        ) {
            SubtleButton(
                labelText = ReferenceApplicationConstants.PLAN_SWAP_ACTION_TEXT,
                enabled = applicationState.originSearchText.isNotBlank() ||
                    applicationState.destinationSearchText.isNotBlank(),
                onClick = { applicationState.swapPlanFields() },
                modifier = Modifier
                    .wrapContentWidth()
                    .testTag(ReferenceApplicationConstants.PLAN_SWAP_ACTION_TEST_TAG),
            )
        }
        applicationState.planFieldMyLocationStatusText?.let { myLocationFailureText ->
            ReasonStrip(reasonText = myLocationFailureText)
        }
        // The result list's assignment hint: which field a tapped row fills.
        Text(
            text = applicationState.searchAssignmentHintText,
            style = MaterialTheme.typography.labelSmall,
            color = LocalReferenceAppUXExtendedColors.current.textSecondary,
            modifier = Modifier.testTag(
                ReferenceApplicationConstants.SEARCH_ASSIGNMENT_HINT_TEST_TAG,
            ),
        )
        // The Directions section sits beside the plan action: the mode and
        // strategy every plan in the application carries, with a choice the
        // serving provider cannot honour disabled and annotated
        // (PRD.md §6 item 36).
        DirectionsGroup(applicationState = applicationState)
        // The explicit Plan route action: enabled while both fields hold text;
        // pressing it resolves each field and names any field that resolves to
        // nothing. While either field is empty it states the missing half.
        if (applicationState.planRouteReasonText.isNotEmpty()) {
            ReasonStrip(
                reasonText = applicationState.planRouteReasonText,
                modifier = Modifier.testTag(
                    ReferenceApplicationConstants.PLAN_ROUTE_REASON_TEST_TAG,
                ),
            )
        }
        PrimaryButton(
            labelText = ReferenceApplicationConstants.PLAN_ROUTE_BUTTON_TEXT,
            enabled = applicationState.canPlanRoute && !applicationState.routePlanInFlight,
            onClick = { applicationState.planRouteFromPlanFields() },
            modifier = Modifier
                .fillMaxWidth()
                .testTag(ReferenceApplicationConstants.PLAN_ROUTE_BUTTON_TEST_TAG),
        )
        Text(
            text = if (applicationState.searchInFlight) {
                ReferenceApplicationConstants.SEARCH_IN_FLIGHT_TEXT
            } else if (applicationState.searchResultsAreDemoPlaces &&
                applicationState.searchResults.isNotEmpty()
            ) {
                // The keyless filter's rows are the artifact's demo
                // places (§4.1): the heading says so, never presenting
                // them as live search results.
                ReferenceApplicationConstants.DEMO_PLACES_HEADING_TEXT
            } else {
                ReferenceApplicationConstants.SEARCH_RESULTS_LABEL_TEXT
            },
            style = MaterialTheme.typography.bodySmall,
            color = LocalReferenceAppUXExtendedColors.current.textSecondary,
        )
        val demoNoMatchText = applicationState.demoSearchNoMatchText
        if (!applicationState.searchInFlight && demoNoMatchText != null) {
            // The keyless filter's honest no-match line: it points at the
            // provider key for live search instead of a silent empty list.
            ReasonStrip(reasonText = demoNoMatchText)
        } else if (!applicationState.searchInFlight &&
            applicationState.searchResults.isEmpty() &&
            applicationState.destinationPlace == null
        ) {
            ReasonStrip(reasonText = ReferenceApplicationConstants.EMPTY_RESULTS_HINT_TEXT)
        }
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .testTag(ReferenceApplicationConstants.PLAN_SEARCH_RESULTS_TEST_TAG),
            verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
        ) {
            applicationState.searchResults.forEach { result ->
                SearchPlaceResultRow(
                    applicationState = applicationState,
                    placeResult = result,
                )
            }
        }
    }
}

/**
 * The Directions section beside the plan action
 * (PRD.md §6 item 36): the Car/Walk mode control and the
 * Fastest/Shortest strategy control, with a choice the serving provider
 * cannot honour disabled and annotated with its reason. The strategy control
 * is shown for automobile plans, the only ones its providers document.
 */
@Composable
private fun DirectionsGroup(applicationState: ReferenceApplicationState) {
    val directionsState = applicationState.directionsState
    PanelGroup(groupLabel = ReferenceApplicationConstants.DIRECTIONS_GROUP_LABEL_TEXT) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
        ) {
            DirectionChoiceChip(
                labelText = ReferenceApplicationConstants.DIRECTIONS_MODE_CAR_TEXT,
                testTag = ReferenceApplicationConstants.DIRECTIONS_MODE_CAR_TEST_TAG,
                isSelected = directionsState.selectedTransportationMode ==
                    DirectionsTransportationMode.CAR,
                isEnabled = directionsState.isTransportationModeAvailable(
                    DirectionsTransportationMode.CAR,
                ),
                onClick = {
                    directionsState.selectTransportationMode(DirectionsTransportationMode.CAR)
                },
            )
            DirectionChoiceChip(
                labelText = ReferenceApplicationConstants.DIRECTIONS_MODE_WALK_TEXT,
                testTag = ReferenceApplicationConstants.DIRECTIONS_MODE_WALK_TEST_TAG,
                isSelected = directionsState.selectedTransportationMode ==
                    DirectionsTransportationMode.WALK,
                isEnabled = directionsState.isTransportationModeAvailable(
                    DirectionsTransportationMode.WALK,
                ),
                onClick = {
                    directionsState.selectTransportationMode(DirectionsTransportationMode.WALK)
                },
            )
        }
        if (directionsState.selectedTransportationMode == DirectionsTransportationMode.CAR) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
            ) {
                DirectionChoiceChip(
                    labelText = ReferenceApplicationConstants.DIRECTIONS_STRATEGY_FASTEST_TEXT,
                    testTag = ReferenceApplicationConstants.DIRECTIONS_STRATEGY_FASTEST_TEST_TAG,
                    isSelected = directionsState.selectedRouteStrategy ==
                        DirectionsRouteStrategy.FASTEST,
                    isEnabled = directionsState.capabilities.unavailableReasonText == null,
                    onClick = {
                        directionsState.selectRouteStrategy(DirectionsRouteStrategy.FASTEST)
                    },
                )
                DirectionChoiceChip(
                    labelText = ReferenceApplicationConstants.DIRECTIONS_STRATEGY_SHORTEST_TEXT,
                    testTag = ReferenceApplicationConstants.DIRECTIONS_STRATEGY_SHORTEST_TEST_TAG,
                    isSelected = directionsState.selectedRouteStrategy ==
                        DirectionsRouteStrategy.SHORTEST,
                    isEnabled = directionsState.isShortestRouteStrategyAvailable(),
                    onClick = {
                        directionsState.selectRouteStrategy(DirectionsRouteStrategy.SHORTEST)
                    },
                )
            }
        }
        val providerUnavailableReason = directionsState.capabilities.unavailableReasonText
        if (providerUnavailableReason != null) {
            // The capability read itself failed (no routing provider is
            // installed): one honest line states the contract's reason.
            ReasonStrip(reasonText = providerUnavailableReason)
        } else {
            DirectionsTransportationMode.entries.forEach { mode ->
                directionsState.transportationModeUnavailableReason(mode)?.let { reasonText ->
                    ReasonStrip(reasonText = reasonText)
                }
            }
            directionsState.routeStrategyUnavailableReason()?.let { reasonText ->
                ReasonStrip(reasonText = reasonText)
            }
        }
    }
}

/** One Directions choice chip: the shared test identifier and disabled state on a chip. */
@Composable
private fun DirectionChoiceChip(
    labelText: String,
    testTag: String,
    isSelected: Boolean,
    isEnabled: Boolean,
    onClick: () -> Unit,
) {
    ChipSurface(
        isSelected = isSelected,
        isEnabled = isEnabled,
        onClick = onClick,
        modifier = Modifier.testTag(testTag),
    ) {
        Text(
            text = labelText,
            style = MaterialTheme.typography.labelLarge,
            color = when {
                !isEnabled -> LocalReferenceAppUXExtendedColors.current.textSecondary
                isSelected -> MaterialTheme.colorScheme.primary
                else -> MaterialTheme.colorScheme.onSurface
            },
        )
    }
}

/**
 * One provider result row (§4.2: name, secondary detail, quality; an assigned
 * row marks itself subtly with `accent`). The rows are the provider's own
 * results, never invented.
 */
@Composable
private fun SearchPlaceResultRow(
    applicationState: ReferenceApplicationState,
    placeResult: Place,
) {
    val isAssigned = applicationState.isPlaceAssigned(placeResult)
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusControl))
            .border(
                width = ScreenLayoutConstants.HAIRLINE_BORDER_WIDTH,
                color = if (isAssigned) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outlineVariant,
                shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusControl),
            )
            .clickable { applicationState.assignSearchResult(placeResult) }
            .padding(ReferenceAppUXDimensions.StackTight),
        horizontalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
    ) {
        Checkbox(
            checked = isAssigned,
            onCheckedChange = { _ -> applicationState.assignSearchResult(placeResult) },
        )
        Column {
            Text(
                text = placeResult.structuredAddressText,
                style = MaterialTheme.typography.bodySmall,
            )
            Text(
                text = "(${placeResult.coordinate.latitude}, ${placeResult.coordinate.longitude}) · " +
                    placeResult.geocodeQuality.name.lowercase(),
                style = numericReadoutStyle,
                color = LocalReferenceAppUXExtendedColors.current.textSecondary,
                maxLines = ScreenLayoutConstants.DISPLAY_TEXT_MAX_LINES,
            )
        }
    }
}

/** The half/full working content: the working rows of the home sheet (§4.1). */
@Composable
private fun HomeSheetWorkingContent(
    applicationState: ReferenceApplicationState,
    modifier: Modifier = Modifier,
) {
    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackGroup)) {
        RouteSummaryCard(applicationState = applicationState)
        DeviceLocationGroup(applicationState = applicationState)
        OfflineMapsCard(applicationState = applicationState)
        ThemeRow(applicationState = applicationState)
        CameraPresetChips(applicationState = applicationState)
    }
}

/**
 * The Location group (M33): the first control asks for the runtime
 * permission, starts the platform source, and stops it again; the status
 * strip states a denied permission or an unavailable service honestly. The
 * second control drives the planned route's guidance from the same source's
 * fixes, and states the plan-first sentence when no route exists.
 */
@Composable
private fun DeviceLocationGroup(applicationState: ReferenceApplicationState) {
    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted -> applicationState.onDeviceLocationPermissionResult(granted) }
    PanelGroup(groupLabel = "Location") {
        Column(verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight)) {
            SubtleButton(
                labelText = if (applicationState.isDeviceLocationActive) {
                    ReferenceApplicationConstants.DEVICE_LOCATION_STOP_TEXT
                } else {
                    ReferenceApplicationConstants.DEVICE_LOCATION_ACTION_TEXT
                },
                enabled = true,
                onClick = {
                    if (applicationState.isDeviceLocationActive) {
                        applicationState.stopDeviceLocation()
                    } else {
                        permissionLauncher.launch(Manifest.permission.ACCESS_FINE_LOCATION)
                    }
                },
                modifier = Modifier
                    .fillMaxWidth()
                    .testTag(ReferenceApplicationConstants.DEVICE_LOCATION_CONTROL_TEST_TAG),
            )
            SubtleButton(
                labelText = if (applicationState.isDeviceNavigationActive) {
                    ReferenceApplicationConstants.DEVICE_NAVIGATION_STOP_TEXT
                } else {
                    ReferenceApplicationConstants.DEVICE_NAVIGATION_ACTION_TEXT
                },
                enabled = true,
                onClick = {
                    if (applicationState.isDeviceNavigationActive) {
                        applicationState.stopDeviceNavigation()
                    } else if (applicationState.plannedRoute == null ||
                        applicationState.hasDeviceLocationPermission()
                    ) {
                        // Without a route the state reports the plan-first
                        // sentence instead of asking for a permission the
                        // trip could not use.
                        applicationState.startDeviceNavigation()
                    } else {
                        applicationState.noteDeviceNavigationPermissionRequested()
                        permissionLauncher.launch(Manifest.permission.ACCESS_FINE_LOCATION)
                    }
                },
                modifier = Modifier
                    .fillMaxWidth()
                    .testTag(ReferenceApplicationConstants.DEVICE_NAVIGATION_CONTROL_TEST_TAG),
            )
            // The plan-first reason is visible before the control is tapped:
            // device navigation follows a planned route, never a demo plan.
            if (applicationState.plannedRoute == null) {
                ReasonStrip(
                    reasonText = ReferenceApplicationConstants.DEVICE_NAVIGATION_PLAN_FIRST_TEXT,
                )
            }
            val statusText = applicationState.deviceLocationStatusText
            if (statusText != null) {
                ReasonStrip(reasonText = statusText)
            }
        }
    }
}

/**
 * The route summary card (§4.2): the distance phrase + duration from
 * `Route`, the primary start, and the subtle edit; the route line on the
 * map is the card's visual (pushed as the active plan's overlay).
 */
@Composable
private fun RouteSummaryCard(applicationState: ReferenceApplicationState) {
    val plannedRoute = applicationState.plannedRoute ?: return
    val route = plannedRoute.route
    PanelGroup(groupLabel = "Route") {
        Column(verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight)) {
            Text(
                text = applicationState.routeSummaryTitleText,
                style = MaterialTheme.typography.titleSmall,
                maxLines = ScreenLayoutConstants.DISPLAY_TEXT_MAX_LINES,
            )
            Text(
                text = "${formatDistanceMeters(route.distanceMeters)} · " +
                    formatDurationSeconds(route.durationSeconds),
                style = numericReadoutStyle,
            )
            // The route card's ETA: the same metric duration, labelled with the
            // drawer's ETA prefix so the card and the drawer read as one trip.
            Text(
                text = "${ReferenceApplicationConstants.NAVIGATION_DRAWER_ETA_PREFIX_TEXT} " +
                    formatDurationSeconds(route.durationSeconds),
                style = MaterialTheme.typography.labelSmall,
                color = LocalReferenceAppUXExtendedColors.current.textSecondary,
            )
            if (plannedRoute.isRecordedDemoRoute) {
                // The route summary names the artifact's recorded route as
                // such (§4.1): the same wording the web planner uses. The
                // recorded geometry is a walk, so the note does not follow the
                // Directions choices.
                Text(
                    text = ReferenceApplicationConstants.RECORDED_ROUTE_NOTE_TEXT,
                    style = MaterialTheme.typography.labelSmall,
                    color = LocalReferenceAppUXExtendedColors.current.textSecondary,
                )
            } else {
                // A live plan states the mode and strategy it carried.
                Text(
                    text = applicationState.directionsState.routeOptionsSummaryText,
                    style = MaterialTheme.typography.labelSmall,
                    color = LocalReferenceAppUXExtendedColors.current.textSecondary,
                )
            }
            if (plannedRoute.isOfflineArchiveRoute) {
                // The offline route is the bundled test graph's answer, never
                // live data; the summary says so (§2.5's honesty note).
                Text(
                    text = ReferenceApplicationConstants.OFFLINE_ROUTE_NOTE_TEXT,
                    style = MaterialTheme.typography.labelSmall,
                    color = LocalReferenceAppUXExtendedColors.current.textSecondary,
                )
            }
            if (applicationState.routePlanInFlight) {
                ReasonStrip(reasonText = ReferenceApplicationConstants.ROUTE_PLAN_IN_FLIGHT_TEXT)
            }
            PrimaryButton(
                labelText = ReferenceApplicationConstants.ROUTE_SUMMARY_START_BUTTON_TEXT,
                enabled = !applicationState.tripDriver.isTripActive,
                onClick = { applicationState.startSimulatedTrip() },
                modifier = Modifier
                    .fillMaxWidth()
                    .testTag(ReferenceApplicationConstants.START_SIMULATED_NAVIGATION_TEST_TAG),
            )
            SubtleButton(
                labelText = ReferenceApplicationConstants.ROUTE_SUMMARY_EDIT_BUTTON_TEXT,
                enabled = true,
                onClick = { applicationState.openPlanSearch() },
            )
        }
    }
}

/**
 * The offline-maps card (§4.1: two honest lines over the registered tile
 * cache, plus the region-download flow of §2.5): the cache state, the live
 * tile accounting, the recorded-graph route control, the demo region's tile
 * count, its download with live progress and cancel, and the stored regions
 * with their delete controls.
 */
@Composable
private fun OfflineMapsCard(applicationState: ReferenceApplicationState) {
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    val regionState = applicationState.offlineRegionState
    PanelGroup(groupLabel = ReferenceApplicationConstants.OFFLINE_MAPS_CARD_TITLE_TEXT) {
        Column(
            verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
        ) {
            Text(
                text = applicationState.offlineCacheStateText,
                style = numericReadoutStyle,
                color = if (applicationState.tileCacheRegisteredPath != null) {
                    extendedColors.positive
                } else {
                    extendedColors.warning
                },
                maxLines = ScreenLayoutConstants.INFO_DETAIL_MAX_LINES,
            )
            Text(
                text = applicationState.offlineLiveTileLineText,
                style = numericReadoutStyle,
                color = extendedColors.textSecondary,
                maxLines = ScreenLayoutConstants.DISPLAY_TEXT_MAX_LINES,
            )
            SubtleButton(
                labelText = ReferenceApplicationConstants.OFFLINE_ROUTE_ACTION_TEXT,
                enabled = !applicationState.offlineRoutePlanInFlight,
                onClick = { applicationState.planOfflineRouteFromArchive() },
            )
            if (applicationState.offlineRoutePlanInFlight) {
                ReasonStrip(reasonText = ReferenceApplicationConstants.ROUTE_PLAN_IN_FLIGHT_TEXT)
            }
            OfflineRegionFlow(regionState = regionState)
        }
    }
}

/**
 * The offline card's region flow: the demo region and its tile count, the
 * download control with its progress/cancel states, and the stored-region
 * list. A cache refusal replaces the flow with the SDK's own reason, so the
 * card never pretends a download could start.
 */
@Composable
private fun OfflineRegionFlow(regionState: OfflineRegionCardState) {
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    val refusalReason = regionState.tileCacheRefusalReason
    if (refusalReason != null) {
        ReasonStrip(reasonText = refusalReason)
        return
    }
    regionState.demoRegionLineText?.let { regionLineText ->
        Text(
            text = regionLineText,
            style = numericReadoutStyle,
            color = extendedColors.textSecondary,
            maxLines = ScreenLayoutConstants.INFO_DETAIL_MAX_LINES,
        )
    }
    regionState.downloadStatusText?.let { statusText ->
        Text(
            text = statusText,
            style = numericReadoutStyle,
            color = if (regionState.downloadFailureReason != null) {
                extendedColors.danger
            } else {
                extendedColors.textSecondary
            },
            maxLines = ScreenLayoutConstants.DISPLAY_TEXT_MAX_LINES,
            modifier = Modifier.testTag(
                ReferenceApplicationConstants.OFFLINE_REGION_STATUS_TEST_TAG,
            ),
        )
    }
    if (regionState.isDownloading) {
        SubtleButton(
            labelText = ReferenceApplicationConstants.OFFLINE_CANCEL_ACTION_TEXT,
            enabled = true,
            onClick = { regionState.cancelDownload() },
            modifier = Modifier
                .fillMaxWidth()
                .testTag(ReferenceApplicationConstants.OFFLINE_REGION_CANCEL_TEST_TAG),
        )
    } else if (regionState.demoRegion != null) {
        // Download is also the resume control after a failed or cancelled
        // prepare: the SDK serves the present tiles from disk and fetches
        // only the missing ones.
        SubtleButton(
            labelText = ReferenceApplicationConstants.OFFLINE_DOWNLOAD_ACTION_TEXT,
            enabled = regionState.canDownload,
            onClick = { regionState.downloadDemoRegion() },
            modifier = Modifier
                .fillMaxWidth()
                .testTag(ReferenceApplicationConstants.OFFLINE_REGION_DOWNLOAD_TEST_TAG),
        )
    }
    if (regionState.storedRegionRows.isNotEmpty()) {
        Text(
            text = ReferenceApplicationConstants.OFFLINE_STORED_REGIONS_HEADING_TEXT,
            style = MaterialTheme.typography.labelSmall,
            color = extendedColors.textSecondary,
        )
        regionState.storedRegionRows.forEach { row ->
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = row.displayName,
                        style = MaterialTheme.typography.bodySmall,
                        maxLines = ScreenLayoutConstants.INFO_DETAIL_MAX_LINES,
                    )
                    Text(
                        text = row.footprintText,
                        style = MaterialTheme.typography.labelSmall,
                        color = extendedColors.textSecondary,
                        maxLines = ScreenLayoutConstants.INFO_DETAIL_MAX_LINES,
                    )
                }
                TextButton(
                    onClick = { regionState.deleteStoredRegion(row.regionId) },
                    modifier = Modifier.testTag(row.deleteTestTag),
                ) {
                    Text(
                        text = ReferenceApplicationConstants.OFFLINE_DELETE_ACTION_TEXT,
                        style = MaterialTheme.typography.labelSmall,
                        color = extendedColors.danger,
                    )
                }
            }
        }
    }
    regionState.storedRegionsFailureReason?.let { listingReason ->
        ReasonStrip(reasonText = listingReason)
    }
    regionState.deletedStatusText?.let { deletedText ->
        Text(
            text = deletedText,
            style = MaterialTheme.typography.labelSmall,
            color = extendedColors.positive,
            maxLines = ScreenLayoutConstants.INFO_DETAIL_MAX_LINES,
        )
    }
}

/** The theme row (§1's half-state theme row; explicit-owned theme, §3). */
@Composable
private fun ThemeRow(applicationState: ReferenceApplicationState) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = ReferenceApplicationConstants.THEME_ROW_TITLE_TEXT,
            style = MaterialTheme.typography.bodySmall,
        )
        // §3: one control cycles light → dark → auto, and its label always
        // names the choice it switches to next — the same words the iOS
        // appearance row and the web theme control render.
        TextButton(onClick = { applicationState.cycleThemeChoice() }) {
            Text(
                text = applicationState.themeChoice.switchControlLabel,
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}

/** Camera preset chips (the shared artifact's demo cameras, applied as camera values). */
@Composable
private fun CameraPresetChips(applicationState: ReferenceApplicationState) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
    ) {
        applicationState.artifact.cameraPresets.forEach { preset ->
            val isSelected = isCurrentPreset(applicationState, preset)
            ChipSurface(isSelected = isSelected, onClick = { applicationState.applyPreset(preset) }) {
                Text(
                    text = preset.label,
                    style = MaterialTheme.typography.labelLarge,
                    color = if (isSelected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                )
            }
        }
    }
}

private fun isCurrentPreset(applicationState: ReferenceApplicationState, preset: CameraPreset): Boolean =
    applicationState.centerLatitudeDegrees == preset.latitudeDegrees &&
        applicationState.centerLongitudeDegrees == preset.longitudeDegrees &&
        applicationState.zoomLevel == preset.zoomLevel &&
        applicationState.bearingDegrees == preset.bearingDegrees &&
        applicationState.pitchDegrees == preset.pitchDegrees

// ---------------------------------------------------------------------------
// The navigation drawer (docs/ReferenceAppUX.md §7.2's low dock)
// ---------------------------------------------------------------------------

/**
 * The navigation drawer: the low dock that replaces the home sheet during a
 * simulated trip — the ETA-distance line, the phase chip, the playback speed
 * and pause/resume controls, the speech toggle, and the exit. Never scrolls;
 * the banner stack at the map's top carries the guidance content (§7.2).
 * Arrival swaps the drawer's contents (§7.1) and hides the playback controls.
 */
@Composable
private fun NavigationDrawer(applicationState: ReferenceApplicationState) {
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    val tripState = applicationState.tripDriver.latestTripState
    val arrived = tripState?.phase == NavigationPhase.ARRIVED ||
        applicationState.navigationListener.phaseChangeState.value == NavigationPhaseChange.ARRIVED ||
        applicationState.tripDriver.hasArrived
    PanelSurface {
        if (arrived) {
            Text(
                text = ReferenceApplicationConstants.NAVIGATION_PHASE_LABEL_ARRIVED,
                style = MaterialTheme.typography.headlineSmall,
                color = extendedColors.positive,
            )
        } else {
            val progress = tripState?.progress
            Text(
                text = if (progress == null) {
                    formatEtaLine(
                        distanceMeters = applicationState.activeSimulatedTrace.route.distanceMeters,
                        durationSeconds = applicationState.activeSimulatedTrace.route.durationSeconds,
                    )
                } else {
                    formatEtaLine(
                        distanceMeters = progress.distanceRemainingMeters,
                        durationSeconds = progress.durationRemainingSeconds,
                    )
                },
                style = numericReadoutStyle,
            )
        }
        // The phone-width drawer cannot fit the phase line, the toggle, and
        // the exit in one row without starving the phase text (§4.2's dock
        // rows wrap instead): the phase reads on its own row, the controls
        // share the row under it.
        Text(
            text =
                if (applicationState.navigationPhaseChip.isEmpty()) {
                    ReferenceApplicationConstants.NAVIGATION_DRAWER_PHASE_NOT_STARTED_TEXT
                } else {
                    applicationState.navigationPhaseChip
                },
            style = MaterialTheme.typography.bodySmall,
        )
        // A device-driven trip replaces the sheet with this drawer, so the
        // Location group's own navigation control travels with the trip: the
        // visitor ends it from where the trip is presented, with the same
        // label the half sheet carries.
        if (applicationState.isDeviceNavigationActive) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                SubtleButton(
                    labelText = ReferenceApplicationConstants.DEVICE_NAVIGATION_STOP_TEXT,
                    enabled = true,
                    onClick = { applicationState.stopDeviceNavigation() },
                    modifier = Modifier
                        .wrapContentWidth()
                        .testTag(ReferenceApplicationConstants.DEVICE_NAVIGATION_CONTROL_TEST_TAG),
                )
                Spacer(modifier = Modifier.weight(1f))
            }
        }
        if (applicationState.isSimulatedTripSeekable) {
            NavigationSeekControl(applicationState = applicationState)
        }
        // Playback pace and pause belong to the simulated replay; a
        // device-driven trip has no playback to steer.
        if (!arrived && !applicationState.isDeviceNavigationActive) {
            NavigationSpeedControl(applicationState = applicationState)
        }
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackGroup),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (applicationState.speechEngineReady) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
                ) {
                    Text(
                        text = ReferenceApplicationConstants.SPEECH_TOGGLE_LABEL_TEXT,
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Switch(
                        checked = applicationState.isSpeechEnabled,
                        onCheckedChange = { enabled -> applicationState.requestSpeechEnabled(enabled) },
                        colors = SwitchDefaults.colors(
                            checkedTrackColor = MaterialTheme.colorScheme.primary,
                            uncheckedTrackColor = MaterialTheme.colorScheme.surfaceVariant,
                        ),
                    )
                }
            } else {
                ReasonStrip(
                    reasonText = "${ReferenceApplicationConstants.SPEECH_TOGGLE_LABEL_TEXT} unavailable: " +
                        (applicationState.speechEngineUnavailableReason ?: "the engine is starting…"),
                )
            }
            Spacer(modifier = Modifier.weight(1f))
            if (!arrived && !applicationState.isDeviceNavigationActive) {
                SubtleButton(
                    labelText = if (applicationState.isSimulatedTripPaused) {
                        ReferenceApplicationConstants.NAVIGATION_DRAWER_RESUME_BUTTON_TEXT
                    } else {
                        ReferenceApplicationConstants.NAVIGATION_DRAWER_PAUSE_BUTTON_TEXT
                    },
                    enabled = true,
                    onClick = {
                        if (applicationState.isSimulatedTripPaused) {
                            applicationState.resumeSimulatedTrip()
                        } else {
                            applicationState.pauseSimulatedTrip()
                        }
                    },
                    modifier = Modifier.wrapContentWidth(),
                )
            }
            SubtleButton(
                labelText = ReferenceApplicationConstants.NAVIGATION_DRAWER_EXIT_BUTTON_TEXT,
                enabled = true,
                onClick = { applicationState.exitSimulatedTrip() },
                modifier = Modifier.wrapContentWidth(),
            )
        }
        applicationState.tripDriverFailureDetailText()?.let { failureDetail ->
            ReasonStrip(reasonText = "Trip delivery failed: $failureDetail", reasonColor = extendedColors.danger)
        }
    }
}

/**
 * The drawer's playback speed control (M27): one selectable chip per named
 * multiplier, the active one tinted. A route-following trip rebuilds its
 * walker at the new pace without moving the travelled position; a recorded
 * trace's replay cadence follows the same choice.
 */
@Composable
private fun NavigationSpeedControl(applicationState: ReferenceApplicationState) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = ReferenceApplicationConstants.NAVIGATION_DRAWER_SPEED_LABEL_TEXT,
            style = MaterialTheme.typography.bodySmall,
        )
        ReferenceApplicationConstants.SIMULATED_TRIP_SPEED_MULTIPLIER_CHOICES.forEach { multiplier ->
            ChipSurface(
                isSelected = applicationState.simulatedTripSpeedMultiplier == multiplier,
                onClick = { applicationState.selectSimulatedTripSpeedMultiplier(multiplier) },
            ) {
                Text(
                    text = ReferenceApplicationConstants.NAVIGATION_SPEED_MULTIPLIER_LABEL_TEMPLATE
                        .format(multiplier.toInt()),
                    style = MaterialTheme.typography.labelSmall,
                )
            }
        }
    }
}

/**
 * The drawer's seek control (M27): one scrubber over the route's own length.
 * Every movement asks the walker for that position, so the camera, the
 * indicator, the ETA line and the arrival state follow exactly as they do on
 * a normal fix; the control stays visible in the arrival state so a backwards
 * seek can leave it.
 */
@Composable
private fun NavigationSeekControl(applicationState: ReferenceApplicationState) {
    Column(modifier = Modifier.fillMaxWidth()) {
        Text(
            text = ReferenceApplicationConstants.NAVIGATION_DRAWER_SEEK_LABEL_TEXT,
            style = MaterialTheme.typography.bodySmall,
        )
        Slider(
            value = applicationState.simulatedTripTravelledMeters.toFloat(),
            onValueChange = { travelledMeters ->
                applicationState.seekSimulatedTrip(travelledMeters.toDouble())
            },
            valueRange = 0f..applicationState.simulatedTripSeekableDistanceMeters
                .toFloat()
                .coerceAtLeast(ScreenLayoutConstants.MINIMUM_SEEK_RANGE_METERS),
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

/** The banner stack at the map's top-center (§7.2): banner, then line, deviation alert. */
@Composable
private fun TurnBannerStack(
    applicationState: ReferenceApplicationState,
    isPhoneLayout: Boolean,
    modifier: Modifier = Modifier,
) {
    val bannerWidthModifier = if (isPhoneLayout) {
        Modifier.fillMaxWidth().padding(horizontal = ReferenceAppUXDimensions.GutterInline)
    } else {
        Modifier.width(ScreenLayoutConstants.TURN_BANNER_WIDTH)
    }
    Column(
        modifier = modifier.padding(top = ReferenceAppUXDimensions.StackTight),
        verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        TurnBannerCard(
            applicationState = applicationState,
            modifier = bannerWidthModifier,
        )
        DeviationAlert(
            applicationState = applicationState,
            modifier = bannerWidthModifier,
        )
    }
}

/** The turn banner (§7.1: live distance phrase + composed display text). */
@Composable
private fun TurnBannerCard(
    applicationState: ReferenceApplicationState,
    modifier: Modifier = Modifier,
) {
    val instruction = applicationState.navigationListener.instructionState.value ?: return
    val arriving = instruction.isArrival ||
        applicationState.tripDriver.latestTripState?.phase == NavigationPhase.ARRIVED ||
        applicationState.tripDriver.hasArrived
    if (arriving) {
        ArrivedCard(modifier = modifier)
        return
    }
    val activeManeuver = instruction.activeManeuver
    BannerCard(
        modifier = modifier,
        headlineText = formatDistancePhrase(activeManeuver.distancePhrase),
        bodyText = instructionBannerText(
            composedDisplayText = activeManeuver.instruction.composedDisplayText,
            providerDisplayText = activeManeuver.instruction.providerDisplayText,
            roundaboutExitIndex = activeManeuver.roundaboutExitIndex,
        ),
        warnState = instruction.isReroutingRequired,
    )
    val upcoming = instruction.upcomingManeuver
    if (upcoming != null) {
        PanelSurface(contentPadding = ReferenceAppUXDimensions.StackTight, modifier = modifier) {
            Text(
                text = "Then: " + instructionBannerText(
                    composedDisplayText = upcoming.instruction.composedDisplayText,
                    providerDisplayText = upcoming.instruction.providerDisplayText,
                    roundaboutExitIndex = upcoming.roundaboutExitIndex,
                ),
                style = MaterialTheme.typography.bodySmall,
                color = LocalReferenceAppUXExtendedColors.current.textSecondary,
            )
        }
    }
}

/** The arrival card (§7: positive-stated "Arrived"). */
@Composable
private fun ArrivedCard(modifier: Modifier = Modifier) {
    BannerCard(
        modifier = modifier,
        headlineText = "",
        bodyText = ReferenceApplicationConstants.NAVIGATION_PHASE_LABEL_ARRIVED,
        warnState = false,
        headlineColor = LocalReferenceAppUXExtendedColors.current.positive,
    )
}

/** Deviation alert strip (§7.5: warning styling, honest frozen-guidance text). */
@Composable
private fun DeviationAlert(
    applicationState: ReferenceApplicationState,
    modifier: Modifier = Modifier,
) {
    val deviationStatusChange = applicationState.navigationListener.deviationStatusChangeState.value ?: return
    if (deviationStatusChange.statusChangeKind != DeviationStatusChangeKind.OFF_ROUTE) {
        return
    }
    val deviationDistanceMeters = deviationStatusChange.deviationInfo?.deviationFromRouteMeters ?: 0.0
    PanelSurface(
        modifier = modifier,
        borderColor = LocalReferenceAppUXExtendedColors.current.warning,
        contentPadding = ReferenceAppUXDimensions.StackTight,
    ) {
        Text(
            text = ReferenceApplicationConstants.DEVIATION_ALERT_TEXT_TEMPLATE
                .format(formatDistanceMeters(deviationDistanceMeters)),
            style = MaterialTheme.typography.labelSmall,
            color = LocalReferenceAppUXExtendedColors.current.warning,
        )
    }
}

// ---------------------------------------------------------------------------
// Readiness chip, diagnostics sheet, error strip, attribution
// ---------------------------------------------------------------------------

/** The readiness chip (§1: the one readiness signal, top-start, opens diagnostics). */
@Composable
private fun ReadinessChip(
    applicationState: ReferenceApplicationState,
    modifier: Modifier = Modifier,
) {
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    val isKeylessPrompt = applicationState.isProviderKeyless &&
        applicationState.readinessState != ReadinessState.READY
    val chipColor = if (isKeylessPrompt) {
        extendedColors.textSecondary
    } else when (applicationState.readinessState) {
        ReadinessState.LOADING -> extendedColors.textSecondary
        ReadinessState.READY -> extendedColors.positive
        ReadinessState.DEGRADED -> extendedColors.warning
        ReadinessState.FAILED -> extendedColors.danger
    }
    val chipText = if (isKeylessPrompt) {
        // The neutral keyless prompt, never an endless "Loading…" (§6.5).
        ReferenceApplicationConstants.KEYLESS_CREDENTIAL_STATUS_TEXT
    } else when (applicationState.readinessState) {
        ReadinessState.LOADING -> ReferenceApplicationConstants.READINESS_CHIP_LOADING_TEXT
        ReadinessState.READY -> ReferenceApplicationConstants.READINESS_CHIP_READY_TEXT
        ReadinessState.DEGRADED ->
            ReferenceApplicationConstants.READINESS_CHIP_DEGRADED_PREFIX_TEXT +
                (applicationState.readinessDetail
                    ?: ReferenceApplicationConstants.READINESS_CHIP_DEGRADED_FALLBACK_DETAIL_TEXT)
        ReadinessState.FAILED ->
            ReferenceApplicationConstants.READINESS_CHIP_FAILED_PREFIX_TEXT +
                (applicationState.readinessDetail
                    ?: ReferenceApplicationConstants.READINESS_CHIP_FAILED_FALLBACK_DETAIL_TEXT)
    }
    Row(
        modifier = modifier
            .clip(shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusInline))
            .background(color = MaterialTheme.colorScheme.surface)
            .border(
                width = ScreenLayoutConstants.HAIRLINE_BORDER_WIDTH,
                color = extendedColors.surfaceBorder,
                shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusInline),
            )
            .clickable { applicationState.openDiagnostics() }
            .testTag(ReferenceApplicationConstants.READINESS_CHIP_TEST_TAG),
        horizontalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = chipText,
            style = MaterialTheme.typography.labelSmall,
            color = chipColor,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(
                horizontal = ReferenceAppUXDimensions.StackGroup,
                vertical = ReferenceAppUXDimensions.StackTight / 2,
            ),
        )
        if (applicationState.readinessState == ReadinessState.FAILED) {
            Text(
                text = "Retry",
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier
                    .clickable { applicationState.retrySurfaceAttach() }
                    .padding(end = ReferenceAppUXDimensions.StackGroup),
            )
        }
    }
}

/** The gear control beside the readiness chip (§6.5: the settings surface's door). */
@Composable
private fun ProviderSettingsControl(
    applicationState: ReferenceApplicationState,
    modifier: Modifier = Modifier,
) {
    val controlShape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusInline)
    Text(
        text = ReferenceApplicationConstants.SETTINGS_GEAR_SYMBOL,
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.onSurface,
        modifier = modifier
            .clip(shape = controlShape)
            .background(color = MaterialTheme.colorScheme.surface)
            .border(
                width = ScreenLayoutConstants.HAIRLINE_BORDER_WIDTH,
                color = LocalReferenceAppUXExtendedColors.current.surfaceBorder,
                shape = controlShape,
            )
            .clickable { applicationState.openSettings() }
            .testTag(ReferenceApplicationConstants.SETTINGS_OPEN_TEST_TAG)
            .semantics {
                contentDescription = ReferenceApplicationConstants.SETTINGS_OPEN_ACCESSIBILITY_TEXT
            }
            .padding(
                horizontal = ReferenceAppUXDimensions.StackTight,
                vertical = ReferenceAppUXDimensions.StackTight / 2,
            ),
    )
}

/**
 * The neutral readiness-dock credential line (§6.5): the keyless product
 * wording until a credential is applied, then the applied summary. It is
 * informational, never a renderer or startup failure.
 */
@Composable
private fun CredentialStatusLine(
    applicationState: ReferenceApplicationState,
    modifier: Modifier = Modifier,
) {
    Text(
        text = applicationState.credentialStatusLineText,
        style = MaterialTheme.typography.labelSmall,
        color = LocalReferenceAppUXExtendedColors.current.textSecondary,
        maxLines = ScreenLayoutConstants.DISPLAY_TEXT_MAX_LINES,
        modifier = modifier
            .clip(shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusInline))
            .background(color = MaterialTheme.colorScheme.surface)
            .padding(
                horizontal = ReferenceAppUXDimensions.StackTight,
                vertical = ReferenceAppUXDimensions.StackTight / 2,
            ),
    )
}

/**
 * The provider-settings sheet (§6.5): one row per shared-registry
 * descriptor of the shared artifact, the opt-in remember toggle, and the
 * honest secure-store note. The credential field is masked and has no
 * show-key affordance; Apply and Clear are explicit acts.
 */
@Composable
private fun ProviderSettingsSheet(
    applicationState: ReferenceApplicationState,
    isPhoneLayout: Boolean,
    viewportHeight: Dp,
    modifier: Modifier = Modifier,
) {
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    val dockWidthModifier = if (isPhoneLayout) {
        Modifier.fillMaxWidth()
    } else {
        Modifier.fillMaxWidth().padding(horizontal = ReferenceAppUXDimensions.GutterInline)
    }
    val sheetShape = RoundedCornerShape(
        topStart = ReferenceAppUXDimensions.RadiusSurface,
        topEnd = ReferenceAppUXDimensions.RadiusSurface,
    )
    val density = LocalDensity.current
    val viewportHeightPixels = with(density) { viewportHeight.roundToPx() }
    val contentScrollState = rememberScrollState()
    var sheetHeightPixels by remember { mutableStateOf(0) }
    LaunchedEffect(
        sheetHeightPixels,
        contentScrollState.maxValue,
        contentScrollState.value,
        viewportHeightPixels,
    ) {
        applicationState.noteSettingsSheetMetrics(
            viewportHeightPixels = viewportHeightPixels,
            sheetHeightPixels = sheetHeightPixels,
            maximumScrollPixels = contentScrollState.maxValue,
            scrollOffsetPixels = contentScrollState.value,
        )
    }
    Column(
        modifier = modifier
            .then(dockWidthModifier)
            // The settings panel is capped at the documented maximum height
            // fraction; its content column scrolls inside it, so nothing is
            // cut off under the system bars (docs/ReferenceAppUX.md §1).
            .heightIn(max = viewportHeight * ReferenceAppUXDimensions.PANEL_MAXIMUM_HEIGHT_FRACTION)
            .onGloballyPositioned { coordinates ->
                sheetHeightPixels = coordinates.size.height
            }
            .clip(shape = sheetShape)
            .background(color = MaterialTheme.colorScheme.surface)
            .border(
                width = ScreenLayoutConstants.HAIRLINE_BORDER_WIDTH,
                color = extendedColors.surfaceBorder,
                shape = sheetShape,
            )
            .padding(ReferenceAppUXDimensions.PanelPadding),
        verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = ReferenceApplicationConstants.SETTINGS_TITLE_TEXT,
                style = MaterialTheme.typography.titleSmall,
            )
            Text(
                text = ReferenceApplicationConstants.SETTINGS_CLOSE_TEXT,
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier
                    .clickable { applicationState.closeSettings() }
                    .testTag(ReferenceApplicationConstants.SETTINGS_CLOSE_TEST_TAG),
            )
        }
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .weight(1f, fill = false)
                .verticalScroll(contentScrollState),
            verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackGroup),
        ) {
            applicationState.providerCredentialDescriptors.forEach { descriptor ->
                ProviderCredentialRow(
                    applicationState = applicationState,
                    descriptor = descriptor,
                )
            }
            CapabilitySettingsSection(applicationState = applicationState)
            RememberCredentialRow(applicationState = applicationState)
            SettingsResetRow(applicationState = applicationState)
            applicationState.credentialStoreUnavailableReason?.let { unavailableReason ->
                // Honest secure-store failure (§6.5): the session keeps the
                // applied credential, the device does not.
                Text(
                    text = ReferenceApplicationConstants.PROVIDER_CREDENTIAL_NOT_STORED_TEMPLATE
                        .format(unavailableReason),
                    style = MaterialTheme.typography.labelSmall,
                    color = extendedColors.warning,
                )
            }
        }
    }
}

/**
 * The settings surface's reset row: clears the remembered explicit choices
 * while leaving the separate, opt-in credential store untouched.
 */
@Composable
private fun SettingsResetRow(applicationState: ReferenceApplicationState) {
    Text(
        text = ReferenceApplicationConstants.SETTINGS_RESET_REMEMBERED_CHOICES_TEXT,
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier
            .fillMaxWidth()
            .clickable { applicationState.resetRememberedChoices() }
            .padding(vertical = ReferenceAppUXDimensions.StackTight),
    )
}

/** One provider descriptor's settings row (§6.5: display name, masked field, Apply/Clear, status). */
@Composable
private fun ProviderCredentialRow(
    applicationState: ReferenceApplicationState,
    descriptor: ProviderCredentialDescriptor,
) {
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    val rowShape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusControl)
    val isAvailableOnThisPlatform = descriptor.isRegistrationAvailableOnAndroid
    val statusText = applicationState.credentialStatusText(descriptor)
    // Keyless is neutral, never a scary failure (§6.5): only a refusal or a
    // nudge that needs the user's attention takes the warning color.
    val statusColor = when {
        !isAvailableOnThisPlatform -> extendedColors.textSecondary
        applicationState.isCredentialApplied(descriptor.providerKey) -> extendedColors.positive
        applicationState.credentialStatusDetails[descriptor.providerKey] != null -> extendedColors.warning
        else -> extendedColors.textSecondary
    }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape = rowShape)
            .border(
                width = ScreenLayoutConstants.HAIRLINE_BORDER_WIDTH,
                color = extendedColors.surfaceBorder,
                shape = rowShape,
            )
            .padding(ReferenceAppUXDimensions.PanelPadding),
        verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
    ) {
        Text(
            text = descriptor.displayName,
            style = MaterialTheme.typography.titleSmall,
        )
        OutlinedTextField(
            value = applicationState.credentialDraftTexts[descriptor.providerKey].orEmpty(),
            onValueChange = { draftText ->
                applicationState.updateCredentialDraft(descriptor.providerKey, draftText)
            },
            label = { Text(text = descriptor.credentialLabel) },
            singleLine = true,
            enabled = isAvailableOnThisPlatform,
            // Masked only: there is deliberately no "show key" affordance
            // (docs/ReferenceAppUX.md §6.5).
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Password,
                imeAction = ImeAction.Done,
            ),
            modifier = Modifier.fillMaxWidth(),
        )
        Row(
            horizontalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackGroup),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Button(
                onClick = { applicationState.applyCredential(descriptor.providerKey) },
                enabled = isAvailableOnThisPlatform,
                shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusControl),
            ) {
                Text(text = ReferenceApplicationConstants.PROVIDER_CREDENTIAL_APPLY_TEXT)
            }
            Text(
                text = ReferenceApplicationConstants.PROVIDER_CREDENTIAL_CLEAR_TEXT,
                style = MaterialTheme.typography.labelLarge,
                color = if (isAvailableOnThisPlatform) {
                    MaterialTheme.colorScheme.primary
                } else {
                    extendedColors.textSecondary
                },
                modifier = Modifier
                    .clickable(enabled = isAvailableOnThisPlatform) {
                        applicationState.clearCredential(descriptor.providerKey)
                    }
                    .inlineShape()
                    .padding(horizontal = ReferenceAppUXDimensions.StackGroup),
            )
        }
        Text(
            text = statusText,
            style = MaterialTheme.typography.labelSmall,
            color = statusColor,
        )
        Text(
            text = descriptor.settingsHint,
            style = MaterialTheme.typography.labelSmall,
            color = extendedColors.textSecondary,
        )
        Text(
            text = descriptor.attributionText,
            style = MaterialTheme.typography.labelSmall,
            color = extendedColors.textSecondary,
        )
    }
}

/**
 * The per-capability provider section: one row per capability the SDK's
 * readback reports, in the SDK's order. Each row names the capability and the
 * provider serving it; a picker offers the shared registry's eligible
 * providers. Raster and vector tiles are read-only, because the map's layer
 * control owns their source.
 */
@Composable
private fun CapabilitySettingsSection(applicationState: ReferenceApplicationState) {
    val capabilityRows = applicationState.capabilityProviderState.rows
    if (capabilityRows.isEmpty()) {
        return
    }
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
    ) {
        Text(
            text = ReferenceApplicationConstants.CAPABILITY_SECTION_LABEL_TEXT,
            style = MaterialTheme.typography.labelLarge,
            color = extendedColors.textSecondary,
        )
        capabilityRows.forEach { capabilityRow ->
            CapabilityProviderSettingsRow(
                applicationState = applicationState,
                capabilityRow = capabilityRow,
            )
        }
    }
}

/** One capability's settings row: the serving provider, its picker or its read-only note. */
@Composable
private fun CapabilityProviderSettingsRow(
    applicationState: ReferenceApplicationState,
    capabilityRow: CapabilityProviderRow,
) {
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    val rowShape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusControl)
    var isMenuExpanded by remember { mutableStateOf(false) }
    val rowModifier = Modifier
        .fillMaxWidth()
        .clip(shape = rowShape)
        .border(
            width = ScreenLayoutConstants.HAIRLINE_BORDER_WIDTH,
            color = extendedColors.surfaceBorder,
            shape = rowShape,
        )
        .then(
            if (capabilityRow.contract == ProviderContract.ROUTING) {
                Modifier.testTag(ReferenceApplicationConstants.CAPABILITY_ROUTING_ROW_TEST_TAG)
            } else {
                Modifier
            },
        )
        .padding(ReferenceAppUXDimensions.PanelPadding)
    Column(
        modifier = rowModifier,
        verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(
                modifier = Modifier
                    .weight(1f)
                    .padding(end = ReferenceAppUXDimensions.StackGroup),
                verticalArrangement = Arrangement.spacedBy(
                    ReferenceAppUXDimensions.StackTight / 2,
                ),
            ) {
                Text(
                    text = capabilityRow.capabilityDisplayName,
                    style = MaterialTheme.typography.titleSmall,
                )
                Text(
                    text = capabilityRow.servingStatusText,
                    style = MaterialTheme.typography.labelSmall,
                    color = if (capabilityRow.isRegistered) {
                        extendedColors.positive
                    } else {
                        extendedColors.textSecondary
                    },
                )
            }
            if (!capabilityRow.isReadOnly && capabilityRow.choices.isNotEmpty()) {
                Box {
                    Text(
                        text = ReferenceApplicationConstants.CAPABILITY_PROVIDER_CHANGE_ACTION_TEXT,
                        style = MaterialTheme.typography.labelLarge,
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.clickable { isMenuExpanded = true },
                    )
                    DropdownMenu(
                        expanded = isMenuExpanded,
                        onDismissRequest = { isMenuExpanded = false },
                    ) {
                        capabilityRow.choices.forEach { choice ->
                            DropdownMenuItem(
                                text = { Text(text = choice.pickerLabel) },
                                onClick = {
                                    isMenuExpanded = false
                                    applicationState.selectCapabilityProvider(
                                        capabilityRow.contract,
                                        choice.providerKey,
                                    )
                                },
                                modifier = Modifier.testTag(
                                    ReferenceApplicationConstants
                                        .CAPABILITY_PROVIDER_CHOICE_TEST_TAG_PREFIX +
                                        choice.providerKey,
                                ),
                            )
                        }
                    }
                }
            }
        }
        capabilityRow.readOnlyNoteText?.let { readOnlyNote ->
            Text(
                text = readOnlyNote,
                style = MaterialTheme.typography.labelSmall,
                color = extendedColors.textSecondary,
            )
        }
        // A keyless provider states the public demo address it was registered
        // with and the descriptor's own terms note (§ "Demo endpoints").
        capabilityRow.demoEndpointText?.let { demoEndpointText ->
            Text(
                text = demoEndpointText,
                style = MaterialTheme.typography.labelSmall,
                color = extendedColors.textSecondary,
            )
        }
        capabilityRow.demoEndpointNote?.let { demoEndpointNote ->
            Text(
                text = demoEndpointNote,
                style = MaterialTheme.typography.labelSmall,
                color = extendedColors.textSecondary,
            )
        }
        capabilityRow.providerTermsNote?.let { providerTermsNote ->
            Text(
                text = providerTermsNote,
                style = MaterialTheme.typography.labelSmall,
                color = extendedColors.textSecondary,
            )
        }
        capabilityRow.detailText?.let { detailText ->
            // A refusal or a needed credential, verbatim: the previous
            // provider keeps serving, and the reason is a next step.
            Text(
                text = detailText,
                style = MaterialTheme.typography.labelSmall,
                color = extendedColors.warning,
            )
        }
    }
}

/** The opt-in persistence toggle (§6.5: default off; store availability is stated honestly). */
@Composable
private fun RememberCredentialRow(applicationState: ReferenceApplicationState) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(
            modifier = Modifier
                .weight(1f)
                .padding(end = ReferenceAppUXDimensions.StackGroup),
            verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight / 2),
        ) {
            Text(
                text = ReferenceApplicationConstants.PROVIDER_CREDENTIAL_REMEMBER_LABEL_TEXT,
                style = MaterialTheme.typography.titleSmall,
            )
            Text(
                text = ReferenceApplicationConstants.PROVIDER_CREDENTIAL_REMEMBER_HINT_TEXT,
                style = MaterialTheme.typography.labelSmall,
                color = LocalReferenceAppUXExtendedColors.current.textSecondary,
            )
        }
        Switch(
            checked = applicationState.isRememberCredentialEnabled,
            onCheckedChange = { isEnabled ->
                applicationState.updateRememberCredentialPreference(isEnabled)
            },
        )
    }
}

/** The diagnostics sheet (§4.1/§6.1: the renderer facts, dismissed like a sheet). */
@Composable
private fun DiagnosticsSheet(
    applicationState: ReferenceApplicationState,
    isPhoneLayout: Boolean,
    viewportHeight: Dp,
    modifier: Modifier = Modifier,
) {
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    val dockWidthModifier = if (isPhoneLayout) {
        Modifier.fillMaxWidth()
    } else {
        Modifier.fillMaxWidth().padding(horizontal = ReferenceAppUXDimensions.GutterInline)
    }
    val sheetShape = RoundedCornerShape(
        topStart = ReferenceAppUXDimensions.RadiusSurface,
        topEnd = ReferenceAppUXDimensions.RadiusSurface,
    )
    Column(
        modifier = modifier
            .then(dockWidthModifier)
            .height(viewportHeight * ReferenceAppUXDimensions.SHEET_HALF_HEIGHT_FRACTION)
            .clip(shape = sheetShape)
            .background(color = MaterialTheme.colorScheme.surface)
            .border(
                width = ScreenLayoutConstants.HAIRLINE_BORDER_WIDTH,
                color = extendedColors.surfaceBorder,
                shape = sheetShape,
            )
            .testTag(ReferenceApplicationConstants.DIAGNOSTICS_SHEET_TEST_TAG)
            .padding(ReferenceAppUXDimensions.PanelPadding),
        verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = ReferenceApplicationConstants.DIAGNOSTICS_TITLE_TEXT,
                style = MaterialTheme.typography.titleSmall,
            )
            Text(
                text = ReferenceApplicationConstants.SETTINGS_CLOSE_TEXT,
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier
                    .clickable { applicationState.dismissDiagnostics() }
                    .testTag(ReferenceApplicationConstants.DIAGNOSTICS_CLOSE_TEST_TAG),
            )
        }
        // The explicit debug switch: the pane's one stream opens at Info; this
        // lowers it to Debug for the session. The SDK gate never raises back,
        // so the scope note states that the detail stays on until restart.
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = if (applicationState.isDiagnosticsDebugDetailEnabled) {
                    ReferenceApplicationConstants.DIAGNOSTICS_HIDE_DEBUG_EVENTS_TEXT
                } else {
                    ReferenceApplicationConstants.DIAGNOSTICS_SHOW_DEBUG_EVENTS_TEXT
                },
                style = MaterialTheme.typography.bodySmall,
            )
            Switch(
                checked = applicationState.isDiagnosticsDebugDetailEnabled,
                onCheckedChange = { isEnabled ->
                    applicationState.updateDiagnosticsDebugDetailEnabled(isEnabled)
                },
                modifier = Modifier.testTag(
                    ReferenceApplicationConstants.DIAGNOSTICS_DEBUG_TOGGLE_TEST_TAG,
                ),
            )
        }
        Text(
            text = ReferenceApplicationConstants.DIAGNOSTICS_STREAM_SCOPE_NOTE_TEXT,
            style = MaterialTheme.typography.labelSmall,
            color = extendedColors.textSecondary,
        )
        // The fact rows are read-only; longer content scrolls inside the
        // sheet's half height instead of drawing past its clip.
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
        ) {
        Text(text = ReferenceApplicationConstants.DIAGNOSTICS_RENDERER_MODE_TEXT, style = MaterialTheme.typography.bodySmall)
        Text(text = applicationState.cameraReadbackText, style = numericReadoutStyle, color = extendedColors.textSecondary)
        Text(
            text = "Map taps: ${applicationState.mapClickCount}",
            style = numericReadoutStyle,
            color = extendedColors.textSecondary,
        )
        val latestStatistics = applicationState.latestTileStatistics
        Text(
            text = if (latestStatistics == null) {
                applicationState.offlineLiveTileLineText
            } else {
                "Tile statistics: requested ${latestStatistics.requestedTileCount}, " +
                    "drawn ${latestStatistics.presentTileCount}, fallback ${latestStatistics.fallbackTileCount}, " +
                    "deferred ${latestStatistics.deferredTileCount}, missing ${latestStatistics.unavailableTileCount}, " +
                    "overlay triangles ${latestStatistics.overlayTriangleCount}"
            },
            style = numericReadoutStyle,
            color = extendedColors.textSecondary,
        )
        applicationState.activeVectorBase?.let { servingVectorBase ->
            val vectorStatus = applicationState.vectorBaseStatus
            Text(
                text = ReferenceApplicationConstants.DIAGNOSTICS_VECTOR_BASE_TEMPLATE.format(
                    servingVectorBase.displayName,
                    vectorStatus?.drawnTileCount?.toInt() ?: 0,
                    vectorStatus?.failedTileCount?.toInt() ?: 0,
                ),
                style = numericReadoutStyle,
                color = extendedColors.textSecondary,
            )
        }
        Text(
            text = "Frame facts: ${applicationState.framesDrawnCount} drawn · ${applicationState.framesDeferredCount} deferred · " +
                "${applicationState.framesFailedCount} failed · ${applicationState.framesSettledCount} idle-settled skips · " +
                "viewport ${applicationState.mapView?.viewportWidthPixels ?: 0} x " +
                "${applicationState.mapView?.viewportHeightPixels ?: 0} px · overlays ${applicationState.pushedOverlayCount}",
            style = numericReadoutStyle,
            color = extendedColors.textSecondary,
        )
        Text(
            text = "Compiled version: ${applicationState.sdkVersion}",
            style = numericReadoutStyle,
            color = extendedColors.textSecondary,
        )
        Text(
            text = "Offline maps: ${applicationState.offlineCacheStateText}",
            style = MaterialTheme.typography.bodySmall,
            color = extendedColors.textSecondary,
        )
        val instruction = applicationState.navigationListener.instructionState.value
        Text(
            text = if (instruction == null) {
                "Simulated navigation: not started"
            } else {
                "Simulated navigation: ${applicationState.navigationPhaseChip.ifEmpty { "started" }} · " +
                    "plan '${applicationState.routeSummaryTitleText}'"
            },
            style = numericReadoutStyle,
            color = extendedColors.textSecondary,
        )
        // The recent-events readout (round 10): the diagnostics subscription
        // (Info severity, engaged when the sheet opened) rendered oldest
        // first — subsystem target + severity + the verbatim detail line
        // (§1: diagnostics detail is the one place SDK jargon is required).
        DiagnosticsEventsSection(applicationState = applicationState)
        }
    }
}

/**
 * The diagnostics sheet's recent-events readout (round 10): the live Info
 * subscription plus the flushed backlog (the backlog line's flushed count),
 * each event rendered as its subsystem target + severity + verbatim detail
 * line (§1: the one place SDK jargon is required verbatim).
 */
@Composable
private fun DiagnosticsEventsSection(applicationState: ReferenceApplicationState) {
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    val eventLines = applicationState.diagnosticEventListener.eventLines
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = ReferenceApplicationConstants.DIAGNOSTICS_RECENT_EVENTS_TITLE_TEXT,
            style = MaterialTheme.typography.titleSmall,
        )
        Text(
            text = ReferenceApplicationConstants.DIAGNOSTICS_CLEAR_ACTION_TEXT,
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.primary,
            modifier = Modifier
                .clickable { applicationState.clearDiagnosticsEvents() }
                .testTag(ReferenceApplicationConstants.DIAGNOSTICS_CLEAR_TEST_TAG),
        )
    }
    Text(
        // The flushed count arrives as a Kotlin UInt, which Android's
        // Formatter rejects for `%d` (`d != kotlin.UInt`) — the readout
        // formats the count as its Int value (the backlog limit is an Int
        // constant, so no Int range is lost).
        text = ReferenceApplicationConstants.DIAGNOSTICS_EVENTS_BACKLOG_TEMPLATE.format(
            (applicationState.diagnosticEventListener.flushedEventCount ?: 0u).toInt(),
        ),
        style = MaterialTheme.typography.labelSmall,
        color = extendedColors.textSecondary,
    )
    if (eventLines.isEmpty()) {
        Text(
            text = ReferenceApplicationConstants.DIAGNOSTICS_RECENT_EVENTS_NONE_TEXT,
            style = numericReadoutStyle,
            color = extendedColors.textSecondary,
        )
        return
    }
    eventLines.forEach { eventLine ->
        Text(
            text = "${eventLine.subsystemTarget} · ${eventLine.severity} · ${eventLine.messageLine}",
            style = numericReadoutStyle,
            color = extendedColors.textSecondary,
            maxLines = 2,
        )
    }
}

/** The error strip (§6.3: variant label plus verbatim detail, danger-bordered). */
@Composable
private fun ErrorStrip(
    applicationState: ReferenceApplicationState,
    modifier: Modifier = Modifier,
) {
    val report = applicationState.errorReport ?: return
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    PanelSurface(
        modifier = modifier.fillMaxWidth(),
        borderColor = extendedColors.danger,
        contentPadding = ReferenceAppUXDimensions.StackTight,
    ) {
        Text(text = "SdkException.${report.variantLabel}", style = MaterialTheme.typography.labelLarge)
        Text(
            text = report.detailText,
            style = MaterialTheme.typography.bodySmall,
            color = extendedColors.textSecondary,
            maxLines = ScreenLayoutConstants.ERROR_STRIP_MAX_LINES,
        )
    }
}

/**
 * The attribution info control (PRD §5 Compliance; §1/§4.2): a compact
 * circular "ⓘ" at the map band's end edge opposite the scale bar, whose panel
 * carries the shared provider registry's attribution lines and terms notes.
 * The text comes from the descriptors, never a shell literal; selecting it is
 * allowed (compliance text is readable/copyable).
 */
@Composable
private fun AttributionInfoControl(
    attributionText: String,
    termsNotes: List<String>,
    isPanelOpen: Boolean,
    onTogglePanel: () -> Unit,
    modifier: Modifier = Modifier,
) {
    // No registry line: the control renders nothing rather than an empty
    // panel (never a guess).
    if (attributionText.isBlank()) {
        return
    }
    Column(
        modifier = modifier,
        verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
        horizontalAlignment = Alignment.End,
    ) {
        if (isPanelOpen) {
            AttributionPanel(
                attributionText = attributionText,
                termsNotes = termsNotes,
            )
        }
        Box(
            modifier = Modifier
                .width(ScreenLayoutConstants.ATTRIBUTION_INFO_CONTROL_SIZE)
                .height(ScreenLayoutConstants.ATTRIBUTION_INFO_CONTROL_SIZE)
                .clip(shape = CircleShape)
                .background(
                    color = if (isPanelOpen) {
                        MaterialTheme.colorScheme.primary
                    } else {
                        MaterialTheme.colorScheme.surface
                    },
                )
                .border(
                    width = ScreenLayoutConstants.HAIRLINE_BORDER_WIDTH,
                    color = if (isPanelOpen) {
                        MaterialTheme.colorScheme.primary
                    } else {
                        MaterialTheme.colorScheme.outlineVariant
                    },
                    shape = CircleShape,
                )
                .clickable(onClickLabel = ReferenceApplicationConstants.ATTRIBUTION_INFO_ACCESSIBILITY_TEXT) {
                    onTogglePanel()
                }
                .semantics {
                    contentDescription = ReferenceApplicationConstants.ATTRIBUTION_INFO_ACCESSIBILITY_TEXT
                },
            contentAlignment = Alignment.Center,
        ) {
            Text(
                text = ReferenceApplicationConstants.ATTRIBUTION_INFO_SYMBOL,
                style = MaterialTheme.typography.titleMedium,
                color = if (isPanelOpen) {
                    MaterialTheme.colorScheme.onPrimary
                } else {
                    MaterialTheme.colorScheme.onSurface
                },
            )
        }
    }
}

/**
 * The attribution panel: the registry descriptors' attribution lines joined
 * as the notice, then each descriptor's terms note. The text is selectable
 * (compliance text stays readable and copyable).
 */
@Composable
private fun AttributionPanel(
    attributionText: String,
    termsNotes: List<String>,
    modifier: Modifier = Modifier,
) {
    SelectionContainer {
        Column(
            modifier = modifier
                .widthIn(max = ScreenLayoutConstants.ATTRIBUTION_PANEL_MAXIMUM_WIDTH)
                .clip(shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusSurface))
                .background(color = MaterialTheme.colorScheme.surface)
                .border(
                    width = ScreenLayoutConstants.HAIRLINE_BORDER_WIDTH,
                    color = MaterialTheme.colorScheme.outlineVariant,
                    shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusSurface),
                )
                .padding(ReferenceAppUXDimensions.PanelPadding),
            verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
        ) {
            Text(
                text = ReferenceApplicationConstants.ATTRIBUTION_PANEL_TITLE_TEXT,
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.onSurface,
            )
            Text(
                text = attributionText,
                style = MaterialTheme.typography.labelSmall,
                color = LocalReferenceAppUXExtendedColors.current.textSecondary,
            )
            termsNotes.forEach { termsNote ->
                Text(
                    text = termsNote,
                    style = MaterialTheme.typography.labelSmall,
                    color = LocalReferenceAppUXExtendedColors.current.textSecondary,
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Building blocks of the design language (§2/§4)
// ---------------------------------------------------------------------------

/**
 * A translucent token panel (`surface` fill, `surface-border` hairline,
 * `radius-surface`) with `panel-padding` and a `stack-tight` column.
 */
@Composable
private fun PanelSurface(
    modifier: Modifier = Modifier,
    borderColor: Color = LocalReferenceAppUXExtendedColors.current.surfaceBorder,
    contentPadding: Dp = ReferenceAppUXDimensions.PanelPadding,
    content: @Composable () -> Unit,
) {
    Column(
        modifier = modifier
            .clip(shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusSurface))
            .background(color = MaterialTheme.colorScheme.surface)
            .border(
                width = ScreenLayoutConstants.HAIRLINE_BORDER_WIDTH,
                color = borderColor,
                shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusSurface),
            )
            .padding(contentPadding),
        verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
    ) {
        content()
    }
}

/** One labeled control group: `stack-group` between the group's controls. */
@Composable
private fun PanelGroup(groupLabel: String, content: @Composable () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackGroup)) {
        Text(text = groupLabel, style = MaterialTheme.typography.titleSmall)
        content()
    }
}

/** A selectable chip surface (`radius-control`, hairline, content row). */
@Composable
private fun ChipSurface(
    isSelected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    isEnabled: Boolean = true,
    content: @Composable () -> Unit,
) {
    Row(
        modifier = modifier
            .clip(shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusControl))
            .background(
                color = if (isSelected && isEnabled) {
                    MaterialTheme.colorScheme.surfaceVariant
                } else {
                    Color.Transparent
                },
            )
            .border(
                width = ScreenLayoutConstants.HAIRLINE_BORDER_WIDTH,
                color = if (isSelected && isEnabled) {
                    MaterialTheme.colorScheme.primary
                } else {
                    MaterialTheme.colorScheme.outlineVariant
                },
                shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusControl),
            )
            .clickable(enabled = isEnabled, onClick = onClick)
            .padding(horizontal = ReferenceAppUXDimensions.StackGroup, vertical = ReferenceAppUXDimensions.StackTight / 2),
        horizontalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        content()
    }
}

/** The accent primary button of §4. */
@Composable
private fun PrimaryButton(
    labelText: String,
    enabled: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier.fillMaxWidth(),
) {
    Button(
        onClick = onClick,
        enabled = enabled,
        shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusControl),
        colors = ButtonDefaults.buttonColors(
            containerColor = MaterialTheme.colorScheme.primary,
            contentColor = MaterialTheme.colorScheme.onPrimary,
        ),
        modifier = modifier,
    ) {
        Text(text = labelText, style = MaterialTheme.typography.labelLarge)
    }
}

/** The subtle (surface-filled, hairline-bordered) button of §4. */
@Composable
private fun SubtleButton(
    labelText: String,
    enabled: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier.fillMaxWidth(),
) {
    Button(
        onClick = onClick,
        enabled = enabled,
        shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusControl),
        colors = ButtonDefaults.buttonColors(
            containerColor = Color.Transparent,
            contentColor = MaterialTheme.colorScheme.onSurface,
        ),
        modifier = modifier.border(
            width = ScreenLayoutConstants.HAIRLINE_BORDER_WIDTH,
            color = MaterialTheme.colorScheme.outlineVariant,
            shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusControl),
        ),
    ) {
        Text(text = labelText, style = MaterialTheme.typography.labelLarge)
    }
}

/**
 * The search view's labelled field row: an explicit label over the field, an
 * inline `Search` action beside it, and the assignment-target badge on the
 * field the shared result list currently fills (§4.1, the iOS reference's
 * per-field submit control). The IME search key commits the same query.
 */
@Composable
private fun LabeledSearchField(
    labelText: String,
    placeholderText: String,
    value: String,
    onValueChange: (String) -> Unit,
    onSearch: () -> Unit,
    onMyLocation: () -> Unit,
    autocompleteState: PlanAutocompleteViewState,
    onAutocompleteSuggestionSelected: (Place) -> Unit,
    autocompleteListTestTag: String,
    autocompleteNoMatchTestTag: String,
    autocompleteFailureTestTag: String,
    fieldTestTag: String,
    myLocationTestTag: String,
    fieldMarkerText: String,
) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight / 2),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // The visible A/B endpoint marker (§4.1's shared route markers).
            Text(
                text = fieldMarkerText,
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.primary,
            )
            OutlinedTextField(
                value = value,
                onValueChange = onValueChange,
                label = { Text(text = labelText) },
                placeholder = { Text(text = placeholderText) },
                singleLine = true,
                textStyle = MaterialTheme.typography.bodySmall,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                keyboardActions = KeyboardActions(onSearch = { onSearch() }),
                modifier = Modifier
                    .weight(ScreenLayoutConstants.SEARCH_FIELD_WEIGHT)
                    .testTag(fieldTestTag),
            )
            Text(
                text = ReferenceApplicationConstants.SEARCH_SUBMIT_LABEL_TEXT,
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier
                    .clickable(onClick = onSearch)
                    .padding(horizontal = ReferenceAppUXDimensions.StackTight),
            )
        }
        // The field's type-ahead answer (docs/ReferenceAppUX.md §5.5): at
        // most the shared cap of suggestion rows, one no-match row instead of
        // an empty list, or the provider's one inline sentence while the text
        // stays. A cancelled request renders none of these.
        autocompleteState.failureText?.let { failureText ->
            Text(
                text = failureText,
                style = MaterialTheme.typography.labelSmall,
                color = LocalReferenceAppUXExtendedColors.current.danger,
                modifier = Modifier
                    .fillMaxWidth()
                    .testTag(autocompleteFailureTestTag),
            )
        }
        if (autocompleteState.isVisible) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .testTag(autocompleteListTestTag),
                verticalArrangement = Arrangement.spacedBy(
                    ReferenceAppUXDimensions.StackTight / 2,
                ),
            ) {
                autocompleteState.suggestions.forEachIndexed { rowIndex, suggestion ->
                    Text(
                        text = suggestion.structuredAddressText,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurface,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(ReferenceAppUXDimensions.RadiusInline))
                            .clickable { onAutocompleteSuggestionSelected(suggestion) }
                            .padding(
                                horizontal = ReferenceAppUXDimensions.StackTight,
                                vertical = ReferenceAppUXDimensions.StackTight / 2,
                            )
                            .testTag("$autocompleteListTestTag-row-$rowIndex"),
                    )
                }
                if (autocompleteState.showsNoMatchRow) {
                    Text(
                        text = ReferenceApplicationConstants.AUTOCOMPLETE_NO_MATCH_TEXT,
                        style = MaterialTheme.typography.bodySmall,
                        color = LocalReferenceAppUXExtendedColors.current.textSecondary,
                        modifier = Modifier
                            .fillMaxWidth()
                            .testTag(autocompleteNoMatchTestTag)
                            .padding(
                                horizontal = ReferenceAppUXDimensions.StackTight,
                                vertical = ReferenceAppUXDimensions.StackTight / 2,
                            ),
                    )
                }
            }
        }
        // The field's own My location chip: it fills this field and only this
        // field, and the accessible name says which one.
        Text(
            text = ReferenceApplicationConstants.PLAN_MY_LOCATION_ACTION_TEXT,
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.primary,
            modifier = Modifier
                .align(Alignment.End)
                .clip(shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusInline))
                .clickable(onClick = onMyLocation)
                .semantics {
                    contentDescription =
                        ReferenceApplicationConstants.PLAN_MY_LOCATION_ACCESSIBILITY_TEMPLATE
                            .format(labelText.lowercase())
                }
                .testTag(myLocationTestTag)
                .padding(
                    horizontal = ReferenceAppUXDimensions.StackTight,
                    vertical = ReferenceAppUXDimensions.StackTight / 2,
                ),
        )
    }
}

/** The warning-styled reason strip (§4.2: disabled-with-reason, `radius-inline`). */
@Composable
private fun ReasonStrip(
    reasonText: String,
    reasonColor: Color? = null,
    modifier: Modifier = Modifier,
) {
    Text(
        text = reasonText,
        style = MaterialTheme.typography.labelSmall,
        color = reasonColor ?: LocalReferenceAppUXExtendedColors.current.warning,
        modifier = modifier
            .fillMaxWidth()
            .inlineShape()
            .background(color = Color.Transparent)
            .padding(horizontal = ReferenceAppUXDimensions.StackTight, vertical = 4.dp),
        maxLines = ScreenLayoutConstants.DISPLAY_TEXT_MAX_LINES,
    )
}

/** The turn banner card (§7.1: maneuver-scale type, panel tokens). */
@Composable
private fun BannerCard(
    headlineText: String,
    bodyText: String,
    warnState: Boolean,
    modifier: Modifier = Modifier,
    headlineColor: Color = if (warnState) {
        LocalReferenceAppUXExtendedColors.current.danger
    } else {
        MaterialTheme.colorScheme.onSurface
    },
) {
    PanelSurface(
        modifier = modifier,
        borderColor = if (warnState) LocalReferenceAppUXExtendedColors.current.danger else LocalReferenceAppUXExtendedColors.current.surfaceBorder,
        contentPadding = ReferenceAppUXDimensions.PanelPadding,
    ) {
        if (headlineText.isNotEmpty()) {
            Text(
                text = headlineText,
                style = MaterialTheme.typography.headlineSmall,
                color = headlineColor,
            )
        }
        Text(text = bodyText, style = MaterialTheme.typography.titleSmall, color = headlineColor)
    }
}

/** `radius-inline` clip modifier for chips, the attribution and reason strips. */
private fun Modifier.inlineShape(): Modifier =
    clip(shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusInline))

// ---------------------------------------------------------------------------
// Friend of the screen: formatting helpers (structural, unit-bearing text)
// ---------------------------------------------------------------------------

/** Formats the metric distance for the route summary and the meters-based deviation lines. */
internal fun formatDistanceMeters(distanceMeters: Double): String =
    if (distanceMeters < ScreenLayoutConstants.METERS_PER_KILOMETER) {
        "%.0f m".format(distanceMeters)
    } else {
        "%.2f km".format(distanceMeters / ScreenLayoutConstants.METERS_PER_KILOMETER)
    }

/** Formats the duration for the route summary and the drawer's ETA-distance line. */
internal fun formatDurationSeconds(durationSeconds: Double): String {
    val wholeMinutes = floor(durationSeconds / ScreenLayoutConstants.SECONDS_PER_MINUTE)
    val remainderSeconds = durationSeconds - wholeMinutes * ScreenLayoutConstants.SECONDS_PER_MINUTE
    return if (wholeMinutes > 0.0) {
        "%.0f min %.0f s".format(wholeMinutes, remainderSeconds)
    } else {
        "%.0f s".format(durationSeconds)
    }
}

/**
 * Formats the drawer's ETA-distance line (§7.2): the two labelled remaining
 * values. The labels are canonical consumer copy; the values keep the demo's
 * own metric formatting.
 */
private fun formatEtaLine(distanceMeters: Double, durationSeconds: Double): String =
    ReferenceApplicationConstants.NAVIGATION_DRAWER_ETA_PREFIX_TEXT + ": " +
        ReferenceApplicationConstants.NAVIGATION_DRAWER_ETA_DISTANCE_LABEL_TEXT + " " +
        formatDistanceMeters(distanceMeters) + " · " +
        ReferenceApplicationConstants.NAVIGATION_DRAWER_ETA_DURATION_LABEL_TEXT + " " +
        formatDurationSeconds(durationSeconds)

/** Formats the distance phrase of the active maneuver (§7.1 live prefix). */
internal fun formatDistancePhrase(distancePhrase: DistancePhrase): String = when (distancePhrase.phraseKind) {
    DistancePhraseKind.IMMEDIATE -> "Now"
    DistancePhraseKind.QUANTITY -> "%.0f %s".format(
        distancePhrase.magnitude ?: 0.0,
        distanceUnitSymbol(distancePhrase.magnitudeUnit ?: DistanceUnit.METERS),
    )
    // Round 11's suppressed kind (the frozen fully-off-route projection):
    // the contract composes no distance prefix on it, so the countdown line
    // renders empty — a phantom countdown off the frozen projection is
    // never fabricated.
    DistancePhraseKind.SUPPRESSED -> ""
}

private fun distanceUnitSymbol(distanceUnit: DistanceUnit): String = when (distanceUnit) {
    DistanceUnit.METERS -> "m"
    DistanceUnit.KILOMETERS -> "km"
    DistanceUnit.FEET -> "ft"
    DistanceUnit.MILES -> "mi"
}
