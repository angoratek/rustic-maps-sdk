package com.rusticmaps.sdk.referenceapplication.console

import android.content.Context
import android.content.Intent
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationConstants
import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationState
import com.rusticmaps.sdk.ux.LocalReferenceAppUXExtendedColors
import com.rusticmaps.sdk.ux.ReferenceAppUXDimensions
import com.rusticmaps.sdk.ux.numericReadoutStyle

/**
 * The console's layout values: the reused sheet cap, the wide-viewport panel
 * width, the warning dot and the row truncation. The color, spacing and radius
 * values come from the shared design tokens; these are layout facts the token
 * tables do not declare.
 */
private object ProviderConsoleLayoutConstants {
    /** The sheet's height cap (the shared `panel-maximum-height-fraction`). */
    const val SHEET_HEIGHT_FRACTION: Float =
        ReferenceAppUXDimensions.PANEL_MAXIMUM_HEIGHT_FRACTION

    /** The console panel's readable width beside the map on a wide viewport. */
    val WIDE_PANEL_WIDTH: Dp = 520.dp

    /** The Warning/Error dot's diameter on the floating control. */
    val WARNING_DOT_SIZE: Dp = 8.dp

    /** The hairline border the floating panels share. */
    val HAIRLINE_BORDER_WIDTH: Dp = 1.dp

    /** Visual lines a row renders before the detail view carries the rest. */
    const val ROW_TEXT_MAX_LINES: Int = 3

    /** Visual lines a row's response excerpt renders. */
    const val ROW_EXCERPT_MAX_LINES: Int = 2

    /** The terminal glyph the floating control paints (no icon font). */
    const val CONTROL_GLYPH: String = ">_"

    /** The separator between a row tag and its index. */
    const val ROW_TAG_SEPARATOR: String = "-"
}

/**
 * The floating developer-console control: a terminal glyph above the
 * attribution notice, on the opposite edge from the map layer control. The
 * dot reports a Warning or Error published while the console was closed. A tap
 * opens the console and starts the same bounded diagnostics stream the
 * diagnostics pane uses.
 */
@Composable
fun DeveloperConsoleControl(
    applicationState: ReferenceApplicationState,
    modifier: Modifier = Modifier,
) {
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    val hasUnseenAlert = applicationState.hasUnseenProviderConsoleAlert
    val accessibilityText = if (hasUnseenAlert) {
        ReferenceApplicationConstants.PROVIDER_CONSOLE_OPEN_WITH_ALERT_ACCESSIBILITY_TEXT
    } else {
        ReferenceApplicationConstants.PROVIDER_CONSOLE_OPEN_ACCESSIBILITY_TEXT
    }
    Box(modifier = modifier) {
        Box(
            modifier = Modifier
                .size(ReferenceAppUXDimensions.MapControlDiameter)
                .clip(shape = CircleShape)
                .background(color = MaterialTheme.colorScheme.surface)
                .border(
                    width = ProviderConsoleLayoutConstants.HAIRLINE_BORDER_WIDTH,
                    color = MaterialTheme.colorScheme.outlineVariant,
                    shape = CircleShape,
                )
                .clickable(onClickLabel = accessibilityText) {
                    applicationState.openProviderConsole()
                }
                .semantics {
                    contentDescription = accessibilityText
                }
                .testTag(ReferenceApplicationConstants.DEVELOPER_CONSOLE_CONTROL_TEST_TAG),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                text = ProviderConsoleLayoutConstants.CONTROL_GLYPH,
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.primary,
            )
        }
        if (hasUnseenAlert) {
            Box(
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .size(ProviderConsoleLayoutConstants.WARNING_DOT_SIZE)
                    .clip(shape = CircleShape)
                    .background(color = extendedColors.danger)
                    .testTag(
                        ReferenceApplicationConstants.DEVELOPER_CONSOLE_WARNING_DOT_TEST_TAG,
                    ),
            )
        }
    }
}

/**
 * The developer console sheet: the scrubbed diagnostics stream newest first,
 * the level threshold (Error, Warning, Info), the live tile- and traffic-cache
 * counter lines, a typed detail view for one event and an Export that hands
 * the filtered text to the platform share sheet. It reads the SDK stream; it
 * never issues a provider call.
 */
@Composable
fun DeveloperConsoleSheet(
    applicationState: ReferenceApplicationState,
    isPhoneLayout: Boolean,
    viewportHeight: Dp,
    modifier: Modifier = Modifier,
) {
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    val context = LocalContext.current
    val entries = applicationState.providerConsoleEntries
    val levelFilter = applicationState.providerConsoleLevelFilter
    val visibleEntries = ProviderConsoleFormatting.filtered(entries, levelFilter)
    val selectedEntry = applicationState.selectedProviderConsoleEntry
    val sheetShape = RoundedCornerShape(
        topStart = ReferenceAppUXDimensions.RadiusSurface,
        topEnd = ReferenceAppUXDimensions.RadiusSurface,
    )
    val widthModifier = if (isPhoneLayout) {
        Modifier.fillMaxWidth()
    } else {
        Modifier.width(ProviderConsoleLayoutConstants.WIDE_PANEL_WIDTH)
    }
    Column(
        modifier = modifier
            .then(widthModifier)
            .height(viewportHeight * ProviderConsoleLayoutConstants.SHEET_HEIGHT_FRACTION)
            .clip(shape = sheetShape)
            .background(color = MaterialTheme.colorScheme.surface)
            .border(
                width = ProviderConsoleLayoutConstants.HAIRLINE_BORDER_WIDTH,
                color = extendedColors.surfaceBorder,
                shape = sheetShape,
            )
            .testTag(ReferenceApplicationConstants.DEVELOPER_CONSOLE_SHEET_TEST_TAG)
            .padding(ReferenceAppUXDimensions.PanelPadding),
        verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
    ) {
        ConsoleHeader(
            applicationState = applicationState,
            onExport = {
                shareProviderConsoleText(
                    context = context,
                    text = ProviderConsoleFormatting.exportText(entries, levelFilter),
                )
            },
        )
        if (selectedEntry != null) {
            ConsoleDetail(
                applicationState = applicationState,
                entry = selectedEntry,
                modifier = Modifier.weight(1f),
            )
        } else {
            ConsoleLevelFilterRow(
                applicationState = applicationState,
                selectedFilter = levelFilter,
            )
            ConsoleCacheSummary(applicationState = applicationState)
            if (visibleEntries.isEmpty()) {
                Text(
                    text = ProviderConsoleFormatting.emptyStateText(levelFilter),
                    style = numericReadoutStyle,
                    color = extendedColors.textSecondary,
                )
            } else {
                LazyColumn(
                    modifier = Modifier.fillMaxWidth().weight(1f),
                    verticalArrangement = Arrangement.spacedBy(
                        ReferenceAppUXDimensions.StackTight,
                    ),
                ) {
                    items(visibleEntries) { entry ->
                        ConsoleRow(
                            entry = entry,
                            onSelect = { selected -> applicationState.selectProviderConsoleEntry(selected) },
                        )
                    }
                }
            }
        }
    }
}

/** The sheet's title, Export action and Close action. */
@Composable
private fun ConsoleHeader(
    applicationState: ReferenceApplicationState,
    onExport: () -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = ReferenceApplicationConstants.PROVIDER_CONSOLE_TITLE_TEXT,
            style = MaterialTheme.typography.titleSmall,
        )
        Row(
            horizontalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackGroup),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = ReferenceApplicationConstants.PROVIDER_CONSOLE_EXPORT_TEXT,
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier
                    .clickable { onExport() }
                    .testTag(ReferenceApplicationConstants.DEVELOPER_CONSOLE_EXPORT_TEST_TAG),
            )
            Text(
                text = ReferenceApplicationConstants.PROVIDER_CONSOLE_CLOSE_TEXT,
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier
                    .clickable { applicationState.dismissProviderConsole() }
                    .testTag(ReferenceApplicationConstants.DEVELOPER_CONSOLE_CLOSE_TEST_TAG),
            )
        }
    }
}

/** The three threshold choices; Info is the widest and the default. */
@Composable
private fun ConsoleLevelFilterRow(
    applicationState: ReferenceApplicationState,
    selectedFilter: ProviderConsoleLevelFilter,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
    ) {
        ProviderConsoleLevelFilter.entries.forEach { levelFilter ->
            val isSelected = levelFilter == selectedFilter
            Text(
                text = levelFilter.label,
                style = MaterialTheme.typography.labelLarge,
                color = if (isSelected) {
                    MaterialTheme.colorScheme.primary
                } else {
                    MaterialTheme.colorScheme.onSurface
                },
                modifier = Modifier
                    .clip(shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusInline))
                    .border(
                        width = ProviderConsoleLayoutConstants.HAIRLINE_BORDER_WIDTH,
                        color = if (isSelected) {
                            MaterialTheme.colorScheme.primary
                        } else {
                            MaterialTheme.colorScheme.outlineVariant
                        },
                        shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusInline),
                    )
                    .clickable {
                        applicationState.updateProviderConsoleLevelFilter(levelFilter)
                    }
                    .padding(
                        horizontal = ReferenceAppUXDimensions.StackTight,
                        vertical = ReferenceAppUXDimensions.StackTight / 2,
                    )
                    .testTag(filterTestTag(levelFilter)),
            )
        }
    }
}

/** The live tile-cache and traffic-cache counter lines. */
@Composable
private fun ConsoleCacheSummary(applicationState: ReferenceApplicationState) {
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    Text(
        text = ProviderConsoleFormatting.tileCacheRowText(
            applicationState.providerConsoleTileCacheStatistics,
        ),
        style = MaterialTheme.typography.labelSmall,
        color = extendedColors.textSecondary,
    )
    Text(
        text = ProviderConsoleFormatting.tileFrameRowText(
            applicationState.latestTileStatistics,
        ),
        style = MaterialTheme.typography.labelSmall,
        color = extendedColors.textSecondary,
    )
    Text(
        text = ProviderConsoleFormatting.trafficCacheRowText(
            statistics = applicationState.providerConsoleTrafficCacheStatistics,
            providerDisplayName = applicationState.providerConsoleTrafficProviderDisplayName,
        ),
        style = MaterialTheme.typography.labelSmall,
        color = extendedColors.textSecondary,
    )
}

/** One console row: the provider/request line and its truncated excerpt. */
@Composable
private fun ConsoleRow(
    entry: ProviderConsoleEntry,
    onSelect: (ProviderConsoleEntry) -> Unit,
) {
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape = RoundedCornerShape(ReferenceAppUXDimensions.RadiusInline))
            .clickable { onSelect(entry) }
            .padding(ReferenceAppUXDimensions.StackTight / 2)
            .testTag(
                ReferenceApplicationConstants.DEVELOPER_CONSOLE_ROW_TEST_TAG_PREFIX +
                    entry.timestampMillisecondsSinceEpoch.toString() +
                    ProviderConsoleLayoutConstants.ROW_TAG_SEPARATOR +
                    entry.subsystemTarget,
            ),
        verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight / 2),
    ) {
        Text(
            text = ProviderConsoleFormatting.rowText(entry),
            style = MaterialTheme.typography.bodySmall,
            maxLines = ProviderConsoleLayoutConstants.ROW_TEXT_MAX_LINES,
            overflow = TextOverflow.Ellipsis,
        )
        ProviderConsoleFormatting.rowResponseExcerptText(entry)?.let { excerpt ->
            Text(
                text = excerpt,
                style = numericReadoutStyle,
                color = extendedColors.textSecondary,
                maxLines = ProviderConsoleLayoutConstants.ROW_EXCERPT_MAX_LINES,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/** The typed detail view for one event, with the live cache rows beneath it. */
@Composable
private fun ConsoleDetail(
    applicationState: ReferenceApplicationState,
    entry: ProviderConsoleEntry,
    modifier: Modifier = Modifier,
) {
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    Text(
        text = ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_BACK_TEXT,
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier
            .clickable { applicationState.closeProviderConsoleDetail() }
            .testTag(ReferenceApplicationConstants.DEVELOPER_CONSOLE_DETAIL_BACK_TEST_TAG),
    )
    Column(
        modifier = modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .testTag(ReferenceApplicationConstants.DEVELOPER_CONSOLE_DETAIL_TEST_TAG),
        verticalArrangement = Arrangement.spacedBy(ReferenceAppUXDimensions.StackTight),
    ) {
        ProviderConsoleFormatting.detailRows(entry).forEach { detailRow ->
            Column {
                Text(
                    text = detailRow.label,
                    style = MaterialTheme.typography.labelSmall,
                    color = extendedColors.textSecondary,
                )
                Text(
                    text = detailRow.value,
                    style = numericReadoutStyle,
                )
            }
        }
        Text(
            text = ProviderConsoleFormatting.tileCacheRowText(
                applicationState.providerConsoleTileCacheStatistics,
            ),
            style = MaterialTheme.typography.labelSmall,
            color = extendedColors.textSecondary,
        )
        Text(
            text = ProviderConsoleFormatting.tileFrameRowText(
                applicationState.latestTileStatistics,
            ),
            style = MaterialTheme.typography.labelSmall,
            color = extendedColors.textSecondary,
        )
        Text(
            text = ProviderConsoleFormatting.trafficCacheRowText(
                statistics = applicationState.providerConsoleTrafficCacheStatistics,
                providerDisplayName = applicationState.providerConsoleTrafficProviderDisplayName,
            ),
            style = MaterialTheme.typography.labelSmall,
            color = extendedColors.textSecondary,
        )
    }
}

/** Hands the filtered, credential-free export text to the platform share sheet. */
private fun shareProviderConsoleText(context: Context, text: String) {
    val sendIntent = Intent(Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(
            Intent.EXTRA_SUBJECT,
            ReferenceApplicationConstants.PROVIDER_CONSOLE_EXPORT_SUBJECT_TEXT,
        )
        putExtra(Intent.EXTRA_TEXT, text)
    }
    val chooserIntent = Intent.createChooser(
        sendIntent,
        ReferenceApplicationConstants.PROVIDER_CONSOLE_SHARE_CHOOSER_TEXT,
    )
    chooserIntent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    context.startActivity(chooserIntent)
}

/** The filter chip's stable UI-automation identifier. */
private fun filterTestTag(levelFilter: ProviderConsoleLevelFilter): String = when (levelFilter) {
    ProviderConsoleLevelFilter.ERROR ->
        ReferenceApplicationConstants.DEVELOPER_CONSOLE_FILTER_ERROR_TEST_TAG
    ProviderConsoleLevelFilter.WARNING ->
        ReferenceApplicationConstants.DEVELOPER_CONSOLE_FILTER_WARNING_TEST_TAG
    ProviderConsoleLevelFilter.INFO ->
        ReferenceApplicationConstants.DEVELOPER_CONSOLE_FILTER_INFO_TEST_TAG
}
