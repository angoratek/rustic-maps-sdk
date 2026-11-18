package com.rusticmaps.sdk.referenceapplication.map

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationConstants
import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationState
import com.rusticmaps.sdk.ux.ReferenceAppUXDimensions
import kotlinx.coroutines.delay
import uniffi.map_refapp_core.FollowPhase
import uniffi.map_refapp_core.bearingOffNorthDegrees
import uniffi.map_refapp_core.scaleBarDistanceLabel
import uniffi.map_refapp_core.scaleBarSelection

/**
 * Private layout constants of the map control cluster (docs/ReferenceAppUX.md
 * §4.2): the token table fixes the cluster's behaviors (the §2.4 constants),
 * not its button sizes or docks — those are layout constants, named here.
 * The cluster docks at the map's end edge, the compass at the top-end, the
 * scale bar at the bottom-start above the sheet's dock.
 */
private object MapControlClusterLayoutConstants {
    /** The control button's circular size (`map-control-diameter`). */
    val MAP_CONTROL_SIZE: Dp = ReferenceAppUXDimensions.MapControlDiameter

    /** The gap in the control cluster's stack. */
    val CONTROL_STACK_SPACING: Dp = 8.dp

    /**
     * The compass control's top clearance in navigation mode (§1's turn
     * banner is top-center at full width on a phone, so the top-end compass
     * must clear the banner stack to stay readable).
     */
    val COMPASS_NAVIGATION_TOP_CLEARANCE: Dp = 176.dp

    /** The scale bar's tick height above its bar line. */
    val SCALE_BAR_TICK_HEIGHT: Dp = 5.dp

    /** The scale bar line's thickness. */
    val SCALE_BAR_LINE_THICKNESS: Dp = 2.dp

    /** The scale bar's text-to-bar gap. */
    val SCALE_BAR_TEXT_GAP: Dp = 2.dp

    /** The hairline border width (the screen file's same sheet token companion). */
    val HAIRLINE_BORDER_WIDTH: Dp = 1.dp
}

/**
 * The map control cluster (docs/ReferenceAppUX.md §4.2, round 10): floating
 * on the map's end edge — zoom −/+, my-location (centers on the indicator;
 * during a trip it also re-engages the follow camera, §5.3) and the 2D/3D
 * plane toggle (§2.4's `navigation-follow-pitch-3d`). The zoom buttons
 * repeat their step at the token's 250 ms cadence while held (§2.4's
 * `map-control-zoom-repeat-interval`); a plain tap steps once.
 */
@Composable
fun MapControlCluster(applicationState: ReferenceApplicationState, modifier: Modifier = Modifier) {
    // The my-location control's active re-center affordance (§4.2/§5.3):
    // while a gesture-broken follow waits for an explicit re-engage, the
    // control takes the cluster's primary look and names the act.
    val isRecentering = applicationState.followPhase == FollowPhase.BROKEN_BY_GESTURE
    Column(
        modifier = modifier,
        verticalArrangement = Arrangement.spacedBy(MapControlClusterLayoutConstants.CONTROL_STACK_SPACING),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        MapControlButton(
            contentDescription = ReferenceApplicationConstants.MAP_CONTROL_ZOOM_IN_ACCESSIBILITY_TEXT,
            onActivated = { applicationState.onZoomInControlRequested() },
        ) {
            Text(
                text = ReferenceApplicationConstants.MAP_CONTROL_ZOOM_IN_SYMBOL,
                style = MaterialTheme.typography.headlineSmall,
            )
        }
        MapControlButton(
            contentDescription = ReferenceApplicationConstants.MAP_CONTROL_ZOOM_OUT_ACCESSIBILITY_TEXT,
            onActivated = { applicationState.onZoomOutControlRequested() },
        ) {
            Text(
                text = ReferenceApplicationConstants.MAP_CONTROL_ZOOM_OUT_SYMBOL,
                style = MaterialTheme.typography.headlineSmall,
            )
        }
        MapControlButton(
            contentDescription = if (isRecentering) {
                ReferenceApplicationConstants.MAP_CONTROL_RECENTER_ACCESSIBILITY_TEXT
            } else {
                ReferenceApplicationConstants.MAP_CONTROL_MY_LOCATION_ACCESSIBILITY_TEXT
            },
            isActive = isRecentering,
            onActivated = { applicationState.onMyLocationControlRequested() },
        ) {
            MyLocationGlyph(
                color = if (isRecentering) {
                    MaterialTheme.colorScheme.onPrimary
                } else {
                    MaterialTheme.colorScheme.primary
                },
            )
        }
        MapControlButton(
            contentDescription = ReferenceApplicationConstants.MAP_CONTROL_PLANE_ACCESSIBILITY_TEXT,
            onActivated = { applicationState.onCameraPlaneControlRequested() },
        ) {
            Text(
                text = if (applicationState.isThreeDCameraEnabled) {
                    ReferenceApplicationConstants.MAP_CONTROL_THREE_D_LABEL_TEXT
                } else {
                    ReferenceApplicationConstants.MAP_CONTROL_TWO_D_LABEL_TEXT
                },
                style = MaterialTheme.typography.labelLarge,
                color = if (applicationState.isThreeDCameraEnabled) {
                    MaterialTheme.colorScheme.primary
                } else {
                    MaterialTheme.colorScheme.onSurface
                },
            )
        }
    }
}

/**
 * One circular control button driven through its press interactions: the
 * activation fires on the *press* (so a tap steps once), and holding repeats
 * the activation at the §2.4 repeat cadence until the release. The empty
 * clickable's tap is left unhandled so a hold + release never double-steps.
 */
@Composable
private fun MapControlButton(
    contentDescription: String,
    isActive: Boolean = false,
    onActivated: () -> Unit,
    content: @Composable () -> Unit,
) {
    val interactionSource = remember { MutableInteractionSource() }
    val isPressed by interactionSource.collectIsPressedAsState()
    LaunchedEffect(isPressed) {
        if (!isPressed) {
            return@LaunchedEffect
        }
        onActivated()
        while (true) {
            delay(ReferenceAppUXDimensions.MAP_CONTROL_ZOOM_REPEAT_INTERVAL_MILLISECONDS)
            onActivated()
        }
    }
    Box(
        modifier = Modifier
            .size(MapControlClusterLayoutConstants.MAP_CONTROL_SIZE)
            .clip(shape = CircleShape)
            .background(
                color = if (isActive) {
                    MaterialTheme.colorScheme.primary
                } else {
                    MaterialTheme.colorScheme.surface
                },
            )
            .border(
                width = MapControlClusterLayoutConstants.HAIRLINE_BORDER_WIDTH,
                color = if (isActive) {
                    MaterialTheme.colorScheme.primary
                } else {
                    MaterialTheme.colorScheme.outlineVariant
                },
                shape = CircleShape,
            )
            .clickable(
                interactionSource = interactionSource,
                indication = null,
                onClickLabel = contentDescription,
            ) {
                // Activation rides the press state above; this tap handler
                // stays empty so a release never double-activates.
            }
            .semantics { this.contentDescription = contentDescription },
        contentAlignment = Alignment.Center,
    ) {
        content()
    }
}

/** The my-location glyph: a painted ring with a centered dot (no icon font). */
@Composable
private fun MyLocationGlyph(color: Color) {
    Canvas(modifier = Modifier.size(20.dp)) {
        val center = Offset(size.width / 2f, size.height / 2f)
        drawCircle(
            color = color,
            radius = size.width * MY_LOCATION_RING_FRACTION / 2f,
            center = center,
            style = androidx.compose.ui.graphics.drawscope.Stroke(width = MAP_CENTER_DOT_STROKE_WIDTH.dp.toPx()),
        )
        drawCircle(color = color, radius = size.width * MY_LOCATION_DOT_FRACTION / 2f, center = center)
    }
}

private const val MY_LOCATION_RING_FRACTION: Float = 0.8f
private const val MY_LOCATION_DOT_FRACTION: Float = 0.36f
private const val MAP_CENTER_DOT_STROKE_WIDTH: Float = 2f

/**
 * The compass control (§4.2: top-end, appears only when the camera's bearing
 * is off north by the §2.4 threshold; tap eases north up over the token's
 * 300 ms). The north letter rotates opposite the camera's bearing, so the
 * letter always points at true north on the screen.
 */
@Composable
fun CompassControl(
    applicationState: ReferenceApplicationState,
    isNavigationMode: Boolean,
    modifier: Modifier = Modifier,
) {
    val offNorthDegrees = bearingOffNorthDegrees(applicationState.bearingDegrees)
    // During a guided trip the compass stays visible while north-up holds
    // (M73): it is the toggle that returns the camera to course-up.
    val isHoldingNorthUp = isNavigationMode && applicationState.isHoldingNorthUp()
    if (!isHoldingNorthUp
        && offNorthDegrees < ReferenceAppUXDimensions.COMPASS_VISIBILITY_BEARING_THRESHOLD_DEGREES
    ) {
        return
    }
    val topClearance = if (isNavigationMode) {
        MapControlClusterLayoutConstants.COMPASS_NAVIGATION_TOP_CLEARANCE
    } else {
        ReferenceAppUXDimensions.GutterBlock
    }
    Box(
        modifier = modifier.padding(top = topClearance),
    ) {
        Box(
            modifier = Modifier
                .size(MapControlClusterLayoutConstants.MAP_CONTROL_SIZE)
                .clip(shape = CircleShape)
                .background(color = MaterialTheme.colorScheme.surface)
                .border(
                    width = MapControlClusterLayoutConstants.HAIRLINE_BORDER_WIDTH,
                    color = MaterialTheme.colorScheme.outlineVariant,
                    shape = CircleShape,
                )
                .clickable { applicationState.onCompassControlRequested() },
            contentAlignment = Alignment.Center,
        ) {
            Text(
                text = ReferenceApplicationConstants.MAP_CONTROL_COMPASS_NORTH_LETTER,
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier.rotate(
                    degrees = -applicationState.bearingDegrees.toFloat(),
                ),
            )
        }
    }
}

/**
 * The scale bar (§4.2: bottom-start; built on the camera's
 * `ground_meters_per_screen_pixel` with the §2.4 width range and the 1-2-3-5-10
 * distance ladder). The chosen bar's physical pixels convert to screen
 * density-independent units for the drawing; a camera without a ground
 * answer (or a settled frame before the first draw) renders no bar.
 */
@Composable
fun ScaleBarOverlay(applicationState: ReferenceApplicationState, modifier: Modifier = Modifier) {
    val selectedScaleBarScale = applicationState.currentGroundMetersPerScreenPixel
        ?.let { groundMetersPerScreenPixel ->
            scaleBarSelection(
                groundMetersPerScreenPixel,
                ReferenceAppUXDimensions.SCALE_BAR_WIDTH_MINIMUM_PIXELS,
                ReferenceAppUXDimensions.SCALE_BAR_WIDTH_MAXIMUM_PIXELS,
            )
        } ?: return
    // The chosen bar's physical viewport pixels are the drawing width: the
    // density receiver converts the pixel count to screen-independent units.
    val barLineWidth = with(LocalDensity.current) {
        selectedScaleBarScale.widthPixels.toFloat().toDp()
    }
    val barColor = MaterialTheme.colorScheme.onSurface
    Column(
        modifier = modifier
            .clip(shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusInline))
            .background(color = MaterialTheme.colorScheme.surface)
            .padding(horizontal = ReferenceAppUXDimensions.StackTight, vertical = ReferenceAppUXDimensions.StackTight / 2),
        horizontalAlignment = Alignment.Start,
    ) {
        Text(
            text = scaleBarDistanceLabel(selectedScaleBarScale.distanceMeters),
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurface,
        )
        Spacer(modifier = Modifier.height(MapControlClusterLayoutConstants.SCALE_BAR_TEXT_GAP))
        Box(
            modifier = Modifier
                .width(barLineWidth)
                .height(MapControlClusterLayoutConstants.SCALE_BAR_TICK_HEIGHT),
            contentAlignment = Alignment.BottomStart,
        ) {
            Canvas(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(MapControlClusterLayoutConstants.SCALE_BAR_LINE_THICKNESS),
            ) {
                val barY = size.height / 2f
                drawLine(
                    color = barColor,
                    start = Offset(0f, barY),
                    end = Offset(size.width, barY),
                    strokeWidth = size.height,
                    cap = StrokeCap.Square,
                )
            }
        }
    }
}