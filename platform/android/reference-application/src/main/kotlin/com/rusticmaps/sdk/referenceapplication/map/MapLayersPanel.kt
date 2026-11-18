package com.rusticmaps.sdk.referenceapplication.map

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationConstants
import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationState
import com.rusticmaps.sdk.ux.LocalReferenceAppUXExtendedColors
import com.rusticmaps.sdk.ux.ReferenceAppUXDimensions

/**
 * The layer control's platform-glue metrics (visual decisions no token
 * governs; all named).
 */
private object MapLayersLayoutConstants {
    /** The collapsed control's circular size (`map-control-diameter`). */
    val CONTROL_SIZE: Dp = ReferenceAppUXDimensions.MapControlDiameter

    /** The non-default-layer dot's diameter. */
    val ACTIVE_DOT_DIAMETER: Dp = 8.dp

    /** The dot's inset from the control's top-trailing edge. */
    val ACTIVE_DOT_INSET: Dp = 6.dp

    /** The glyph's drawn edge. */
    val GLYPH_SIZE: Dp = 20.dp

    /** The panel's readable maximum width. */
    val PANEL_MAXIMUM_WIDTH: Dp = 360.dp

    /** The attribution info control's circular size (the panel clears it). */
    val ATTRIBUTION_CONTROL_SIZE: Dp = ReferenceAppUXDimensions.MapControlDiameter

    /**
     * The panel's bottom clearance: the map gutter, the attribution control,
     * and the collapsed layer control above it, so the panel opens upward
     * without covering either.
     */
    val PANEL_BOTTOM_CLEARANCE: Dp = ReferenceAppUXDimensions.GutterBlock +
        ATTRIBUTION_CONTROL_SIZE +
        ReferenceAppUXDimensions.StackTight +
        CONTROL_SIZE

    /** The hairline border width of the control, the panel and its rows. */
    val HAIRLINE_BORDER_WIDTH: Dp = 1.dp

    /** The number of layers the glyph draws. */
    const val LAYER_GLYPH_LAYER_COUNT: Int = 3
}

/**
 * The collapsed round control: it opens the layer panel and carries a dot
 * while a non-default layer is active.
 */
@Composable
fun MapLayersCollapsedControl(
    applicationState: ReferenceApplicationState,
    focusRequester: FocusRequester,
    modifier: Modifier = Modifier,
) {
    val layersState = applicationState.mapLayersState
    val enabledSceneCount = applicationState.sceneToggleStates.count { entry -> entry.value }
    val isNonDefaultLayerActive = layersState.isNonDefaultLayerActive(enabledSceneCount)
    Box(
        modifier = modifier
            .size(MapLayersLayoutConstants.CONTROL_SIZE)
            .clip(shape = CircleShape)
            .background(color = MaterialTheme.colorScheme.surface)
            .border(
                width = MapLayersLayoutConstants.HAIRLINE_BORDER_WIDTH,
                color = LocalReferenceAppUXExtendedColors.current.surfaceBorder,
                shape = CircleShape,
            )
            .focusRequester(focusRequester)
            .onFocusChanged { focusState ->
                layersState.noteControlFocused(focusState.isFocused)
            }
            .clickable { applicationState.toggleMapLayersPanel() }
            .testTag(ReferenceApplicationConstants.MAP_LAYERS_CONTROL_TEST_TAG)
            .semantics {
                contentDescription =
                    ReferenceApplicationConstants.MAP_LAYERS_CONTROL_ACCESSIBILITY_TEXT
                if (isNonDefaultLayerActive) {
                    stateDescription =
                        ReferenceApplicationConstants.MAP_LAYERS_ACTIVE_LAYER_VALUE_TEXT
                }
            },
        contentAlignment = Alignment.Center,
    ) {
        LayersGlyph(color = MaterialTheme.colorScheme.onSurface)
        if (isNonDefaultLayerActive) {
            Box(
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(MapLayersLayoutConstants.ACTIVE_DOT_INSET)
                    .size(MapLayersLayoutConstants.ACTIVE_DOT_DIAMETER)
                    .clip(shape = CircleShape)
                    .background(color = MaterialTheme.colorScheme.primary),
            )
        }
    }
}

/** The layers glyph: three stacked strokes (no icon font). */
@Composable
private fun LayersGlyph(color: Color) {
    Canvas(modifier = Modifier.size(MapLayersLayoutConstants.GLYPH_SIZE)) {
        val layerSpacing = size.height / MapLayersLayoutConstants.LAYER_GLYPH_LAYER_COUNT
        repeat(MapLayersLayoutConstants.LAYER_GLYPH_LAYER_COUNT) { layerIndex ->
            val centerY = layerSpacing * (layerIndex + 0.5f)
            val layerPath = Path().apply {
                moveTo(size.width * 0.5f, centerY - layerSpacing * 0.5f)
                lineTo(size.width, centerY)
                lineTo(size.width * 0.5f, centerY + layerSpacing * 0.5f)
                lineTo(0f, centerY)
                close()
            }
            drawPath(path = layerPath, color = color)
        }
    }
}

/**
 * The layer panel (docs/ReferenceAppUX.md §4.2): Base map, Traffic,
 * Overlays and Device location, in that order. It renders the shared
 * registry's descriptors and the shared scenes artifact at runtime, names no
 * provider and carries no credential.
 */
@Composable
fun MapLayersPanel(
    applicationState: ReferenceApplicationState,
    viewportHeight: Dp,
    modifier: Modifier = Modifier,
) {
    val layersState = applicationState.mapLayersState
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    val panelShape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusSurface)
    val panelFocusRequester = remember { FocusRequester() }
    // The panel takes focus when it opens, so a keyboard's Escape reaches it
    // and Tab walks its rows in order; the caller returns focus to the
    // collapsed control on close.
    LaunchedEffect(Unit) {
        layersState.notePanelPresented()
        try {
            panelFocusRequester.requestFocus()
        } catch (_: IllegalStateException) {
            Unit
        }
    }
    Column(
        modifier = modifier
            .padding(
                end = ReferenceAppUXDimensions.GutterInline,
                bottom = MapLayersLayoutConstants.PANEL_BOTTOM_CLEARANCE,
            )
            .widthIn(max = MapLayersLayoutConstants.PANEL_MAXIMUM_WIDTH)
            .heightIn(
                max = viewportHeight *
                    ReferenceAppUXDimensions.PANEL_MAXIMUM_HEIGHT_FRACTION,
            )
            .clip(shape = panelShape)
            .background(color = MaterialTheme.colorScheme.surface)
            .border(
                width = MapLayersLayoutConstants.HAIRLINE_BORDER_WIDTH,
                color = extendedColors.surfaceBorder,
                shape = panelShape,
            )
            .verticalScroll(rememberScrollState())
            .padding(ReferenceAppUXDimensions.PanelPadding)
            .testTag(ReferenceApplicationConstants.MAP_LAYERS_PANEL_TEST_TAG)
            .focusRequester(panelFocusRequester)
            .onPreviewKeyEvent { keyEvent ->
                if (keyEvent.type == KeyEventType.KeyDown && keyEvent.key == Key.Escape) {
                    applicationState.closeMapLayersPanel()
                    true
                } else {
                    false
                }
            }
            .focusable()
            .semantics {
                contentDescription =
                    ReferenceApplicationConstants.MAP_LAYERS_CONTROL_ACCESSIBILITY_TEXT
            },
        verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackGroup),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = ReferenceApplicationConstants.MAP_LAYERS_CONTROL_ACCESSIBILITY_TEXT,
                style = MaterialTheme.typography.titleSmall,
            )
            Text(
                text = ReferenceApplicationConstants.SETTINGS_CLOSE_TEXT,
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier
                    .clickable { applicationState.closeMapLayersPanel() }
                    .testTag(ReferenceApplicationConstants.MAP_LAYERS_CLOSE_TEST_TAG),
            )
        }
        BaseMapSection(applicationState = applicationState)
        TrafficSection(applicationState = applicationState)
        OverlaysSection(applicationState = applicationState)
        DeviceLocationSection(applicationState = applicationState)
        layersState.statusLineText?.let { statusLineText ->
            Text(
                text = statusLineText,
                style = MaterialTheme.typography.labelSmall,
                color = extendedColors.warning,
                modifier = Modifier.testTag(
                    ReferenceApplicationConstants.MAP_LAYERS_STATUS_TEST_TAG,
                ),
            )
        }
    }
}

/** One labelled section of the panel. */
@Composable
private fun PanelSection(title: String, content: @Composable ColumnScope.() -> Unit) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
    ) {
        Text(
            text = title,
            style = MaterialTheme.typography.labelLarge,
            color = LocalReferenceAppUXExtendedColors.current.textSecondary,
        )
        content()
    }
}

/**
 * The base-map section: one row per raster source the registry offers, then
 * one row per committed vector style. Selecting a row runs the provider-neutral
 * base switch, so a further base or style is a descriptor entry and nothing in
 * this view changes.
 */
@Composable
private fun BaseMapSection(applicationState: ReferenceApplicationState) {
    val layersState = applicationState.mapLayersState
    PanelSection(title = ReferenceApplicationConstants.BASE_MAP_SECTION_LABEL_TEXT) {
        layersState.baseRows.forEach { row ->
            BaseMapRow(
                row = row,
                onSelected = { applicationState.selectMapLayerBase(row.selection) },
            )
        }
        // The stated limit travels with the base that has it, so a user never
        // reads the absence of labels as a failure.
        applicationState.vectorBaseLabelsNote?.let { labelsNote ->
            Text(
                text = labelsNote,
                style = MaterialTheme.typography.labelSmall,
                color = LocalReferenceAppUXExtendedColors.current.textSecondary,
            )
        }
        val failedTileCount = applicationState.vectorBaseStatus?.failedTileCount?.toInt() ?: 0
        if (applicationState.activeVectorBase != null && failedTileCount > 0) {
            Text(
                text = ReferenceApplicationConstants.BASE_MAP_VECTOR_FAILED_STATUS_TEMPLATE
                    .format(failedTileCount),
                style = MaterialTheme.typography.labelSmall,
                color = LocalReferenceAppUXExtendedColors.current.warning,
            )
        }
    }
}

/** One base row: the name, its serving mark or missing-credential reason. */
@Composable
private fun BaseMapRow(row: MapLayerBaseRow, onSelected: () -> Unit) {
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    val rowShape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusControl)
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape = rowShape)
            .border(
                width = MapLayersLayoutConstants.HAIRLINE_BORDER_WIDTH,
                color = if (row.isServing) {
                    MaterialTheme.colorScheme.primary
                } else {
                    extendedColors.surfaceBorder
                },
                shape = rowShape,
            )
            .selectable(
                selected = row.isServing,
                enabled = row.disabledReasonText == null,
                role = Role.RadioButton,
                onClick = onSelected,
            )
            .testTag(row.identifier)
            .padding(ReferenceAppUXDimensions.StackTight),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = row.displayName,
            style = MaterialTheme.typography.bodyMedium,
            color = if (row.disabledReasonText == null) {
                MaterialTheme.colorScheme.onSurface
            } else {
                extendedColors.textSecondary
            },
            modifier = Modifier.weight(1f, fill = false),
        )
        Row(
            horizontalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = row.disabledReasonText
                    ?: if (row.isServing) {
                        ReferenceApplicationConstants.BASE_MAP_ACTIVE_STATUS_TEXT
                    } else {
                        ReferenceApplicationConstants.BASE_MAP_INACTIVE_STATUS_TEXT
                    },
                style = MaterialTheme.typography.labelSmall,
                color = if (row.isServing) {
                    MaterialTheme.colorScheme.primary
                } else {
                    extendedColors.textSecondary
                },
            )
        }
    }
}

/**
 * The traffic section: one switch and its availability reason. The switch
 * reads `Needs <credential>` while no applied credential serves the
 * capability, and `Not configured` when no descriptor declares it.
 */
@Composable
private fun TrafficSection(applicationState: ReferenceApplicationState) {
    val layersState = applicationState.mapLayersState
    PanelSection(
        title = ReferenceApplicationConstants.MAP_LAYERS_TRAFFIC_SECTION_LABEL_TEXT,
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = ReferenceApplicationConstants.MAP_LAYERS_TRAFFIC_SECTION_LABEL_TEXT,
                style = MaterialTheme.typography.bodyMedium,
            )
            Switch(
                checked = layersState.isTrafficEnabled,
                onCheckedChange = { isEnabled ->
                    applicationState.setTrafficLayerEnabled(isEnabled)
                },
                enabled = layersState.trafficAvailability.isAvailable,
                modifier = Modifier
                    .testTag(ReferenceApplicationConstants.MAP_LAYER_TRAFFIC_TEST_TAG)
                    .semantics {
                        contentDescription = ReferenceApplicationConstants
                            .MAP_LAYERS_TRAFFIC_SECTION_LABEL_TEXT
                        stateDescription = if (layersState.isTrafficEnabled) "On" else "Off"
                    },
            )
        }
        layersState.trafficAvailability.reasonText?.let { reasonText ->
            Text(
                text = reasonText,
                style = MaterialTheme.typography.labelSmall,
                color = LocalReferenceAppUXExtendedColors.current.textSecondary,
                modifier = Modifier.testTag(
                    ReferenceApplicationConstants.MAP_LAYER_TRAFFIC_REASON_TEST_TAG,
                ),
            )
        }
    }
}

/**
 * The overlays section: one switch per demo scene key of the shared scenes
 * artifact, writing the same remembered choice the home sheet used to carry.
 * The rows come from the artifact, never from a copied scene name; this panel
 * is the one scene home on the native refapps.
 */
@Composable
private fun OverlaysSection(applicationState: ReferenceApplicationState) {
    PanelSection(title = ReferenceApplicationConstants.MAP_LAYERS_OVERLAYS_SECTION_LABEL_TEXT) {
        applicationState.artifact.overlayScenes.rows.forEach { sceneRow ->
            val isEnabled = applicationState.sceneToggleStates[sceneRow.sceneKey] == true
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = sceneRow.scene.label,
                    style = MaterialTheme.typography.bodyMedium,
                )
                Switch(
                    checked = isEnabled,
                    onCheckedChange = { enabled ->
                        applicationState.setSceneToggle(sceneRow.sceneKey, enabled)
                    },
                    modifier = Modifier
                        .testTag(
                            ReferenceApplicationConstants.MAP_LAYER_SCENE_ROW_TEST_TAG_PREFIX +
                                sceneRow.sceneKey,
                        )
                        .semantics {
                            stateDescription = if (isEnabled) "On" else "Off"
                        },
                )
            }
        }
        Text(
            text = ReferenceApplicationConstants.OVERLAYS_CLEAR_ACTION_TEXT,
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.primary,
            modifier = Modifier
                .clickable {
                    applicationState.clearTheOverlaysOnSurface()
                    applicationState.clearSceneToggles()
                }
                .padding(vertical = ReferenceAppUXDimensions.StackTight),
        )
    }
}

/**
 * The device-location section: the source's state and, while it runs, the
 * Stop action. A refused or unavailable source states its reason here.
 */
@Composable
private fun DeviceLocationSection(applicationState: ReferenceApplicationState) {
    val layersState = applicationState.mapLayersState
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    PanelSection(
        title = ReferenceApplicationConstants.MAP_LAYERS_DEVICE_LOCATION_SECTION_LABEL_TEXT,
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = layersState.locationStatus.displayText,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.testTag(
                    ReferenceApplicationConstants.PANEL_DEVICE_LOCATION_STATUS_TEST_TAG,
                ),
            )
            if (layersState.locationStatus.isSourceRunning) {
                Text(
                    text = ReferenceApplicationConstants.MAP_LAYERS_STOP_ACTION_TEXT,
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.primary,
                    modifier = Modifier
                        .clickable { applicationState.stopDeviceLocation() }
                        .testTag(
                            ReferenceApplicationConstants.PANEL_DEVICE_LOCATION_STOP_TEST_TAG,
                        ),
                )
            }
        }
        layersState.locationStatus.cannotStartReasonText?.let { reasonText ->
            Text(
                text = reasonText,
                style = MaterialTheme.typography.labelSmall,
                color = extendedColors.textSecondary,
            )
        }
    }
}
