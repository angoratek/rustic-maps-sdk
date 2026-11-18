/**
 * The reference application's stylesheet, built per owned theme (§3: the
 * theme is explicit-owned, so the styles rebuild when the theme toggles).
 * The factory takes the active theme's color tokens; every dimension comes
 * from the §2 spacing/shape/type tokens, never a bare literal.
 */
import { StyleSheet } from 'react-native';

import { themeRadii, themeSpacing, themeTypography, type ThemeColorTokens } from './theme';
import { mapControlLayout } from './mapControls';
import {
  sheetDockTopBorderWidth,
  sheetGrabberLayout,
  sheetPeekRowTypography,
  sheetPeekRowVerticalPadding,
  sheetRowBorderWidth,
} from './sheetLayout';

/** The active theme's color tokens (one §2 table, re-exported for the screen). */
export type { ThemeColorTokens };

/** The seek control's track height, in density-independent units. */
const seekTrackHeight = 6;

/**
 * The developer console's own visual metrics: the alert dot's diameter and
 * inset, and the sheet's height fraction (the design's full-height phone
 * sheet, leaving the map's top edge visible behind the scrim).
 */
const developerConsoleAlertDotDiameter = 8;
const developerConsoleAlertDotInset = 4;
const developerConsoleSheetHeightFraction = '88%';

/**
 * The settings surface's named layout values
 * (docs/Providers.md (Credentials): the gear's hit target
 * beside the readiness chip and the credential field's single-line height.
 */
/**
 * The layer control's own visual metrics (docs/ReferenceAppUX.md §4.2):
 * the glyph size, the non-default-layer dot, and the panel's height cap.
 */
export const mapLayersControlLayout = {
  /** The layers glyph's type size in density-independent units. */
  glyphFontSize: 18,
  /** The non-default-layer dot's diameter. */
  activeDotDiameter: 8,
  /** The dot's inset from the control's top-trailing edge. */
  activeDotInset: 6,
  /** The panel's height cap as a fraction of the viewport height (70%). */
  panelMaximumHeightFraction: '70%',
} as const;

export const settingsLayout = {
  /** The gear control's square hit target, in density-independent units. */
  gearButtonSize: 40,
  /** The gear glyph's type size. */
  gearGlyphFontSize: 18,
  /** The masked credential field's comfortable single-line height. */
  credentialInputMinimumHeight: 40,
  /** The gap between a credential field and its status line. */
  credentialStatusMarginTop: 2,
} as const;

export function buildScreenStyles(colors: ThemeColorTokens) {
  return StyleSheet.create({
    screen: {
      flex: 1,
      backgroundColor: colors.screenBackdrop,
    },
    mapFill: {
      position: 'absolute',
      top: 0,
      left: 0,
      right: 0,
      bottom: 0,
    },
    /** The readiness chip and the settings gear at the top-start dock (§6.1). */
    readinessBar: {
      position: 'absolute',
      left: themeSpacing.gutterInline,
      right: themeSpacing.gutterInline,
      flexDirection: 'row',
      alignItems: 'center',
      gap: themeSpacing.stackTight,
    },
    readinessChip: {
      flexShrink: 1,
      maxWidth: '72%',
      backgroundColor: colors.surface,
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.control,
      paddingHorizontal: themeSpacing.panelPadding,
      paddingVertical: themeSpacing.stackTight,
    },
    readinessChipText: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeBody.fontSize,
    },
    /** The settings gear beside the readiness chip (§3's entry point). */
    settingsGearButton: {
      width: settingsLayout.gearButtonSize,
      height: settingsLayout.gearButtonSize,
      borderRadius: themeRadii.control,
      backgroundColor: colors.surface,
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      alignItems: 'center',
      justifyContent: 'center',
    },
    settingsGearGlyph: {
      color: colors.textPrimary,
      fontSize: settingsLayout.gearGlyphFontSize,
    },
    errorStrip: {
      position: 'absolute',
      left: themeSpacing.gutterInline,
      right: themeSpacing.gutterInline,
      backgroundColor: colors.surface,
      borderColor: colors.danger,
      borderWidth: 1,
      borderRadius: themeRadii.control,
      padding: themeSpacing.stackTight,
    },
    errorStripVariant: {
      color: colors.danger,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    errorStripDetail: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    attribution: {
      alignSelf: 'stretch',
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    // The attribution info control (§1/§4.2): the circular "ⓘ" that
    // replaced the always-visible line, and the panel it opens. It shares the
    // map controls' diameter.
    attributionInfoControl: {
      width: mapControlLayout.buttonSize,
      height: mapControlLayout.buttonSize,
      borderRadius: mapControlLayout.buttonSize / 2,
      backgroundColor: colors.surface,
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      alignItems: 'center',
      justifyContent: 'center',
    },
    attributionInfoControlActive: {
      backgroundColor: colors.accent,
      borderColor: colors.accent,
    },
    attributionInfoGlyph: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeManeuver.fontSize,
      fontWeight: '600',
    },
    attributionPanelBackdrop: {
      flex: 1,
      justifyContent: 'flex-end',
      alignItems: 'flex-end',
      paddingHorizontal: themeSpacing.gutterInline,
      paddingBottom: themeSpacing.gutterBlock,
    },
    attributionPanel: {
      maxWidth: 280,
      backgroundColor: colors.surface,
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.inline,
      padding: themeSpacing.stackGroup,
      gap: themeSpacing.stackTight,
    },
    attributionPanelTitle: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeCaption.fontSize,
      fontWeight: '600',
    },
    attributionTermsNote: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    /** §6.2's floating disabled-with-reason strip (warning-bordered panel). */
    floatingReasonStrip: {
      alignSelf: 'stretch',
      backgroundColor: colors.surface,
      borderColor: colors.warning,
      borderWidth: 1,
      borderRadius: themeRadii.inline,
      paddingHorizontal: themeSpacing.stackGroup,
      paddingVertical: themeSpacing.stackTight,
    },
    // --- The map control cluster (§4.2) ---
    mapControlCluster: {
      position: 'absolute',
      right: themeSpacing.gutterInline,
      top: 0,
      bottom: 0,
      justifyContent: 'center',
      alignItems: 'center',
      gap: mapControlLayout.stackSpacing,
    },
    mapControlButton: {
      width: mapControlLayout.buttonSize,
      height: mapControlLayout.buttonSize,
      borderRadius: mapControlLayout.buttonSize / 2,
      backgroundColor: colors.surface,
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      alignItems: 'center',
      justifyContent: 'center',
    },
    mapControlButtonText: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeManeuver.fontSize,
      lineHeight: themeTypography.typeManeuver.lineHeight,
    },
    mapControlButtonTextActive: {
      color: colors.accent,
    },
    /** The re-center affordance's accent fill (§4.2/§5.3). */
    mapControlButtonActive: {
      backgroundColor: colors.accent,
      borderColor: colors.accent,
    },
    myLocationGlyphRing: {
      width: mapControlLayout.myLocationGlyphSize,
      height: mapControlLayout.myLocationGlyphSize,
      borderRadius: mapControlLayout.myLocationGlyphSize / 2,
      borderWidth: mapControlLayout.myLocationGlyphRingStroke,
      borderColor: colors.accent,
      alignItems: 'center',
      justifyContent: 'center',
    },
    myLocationGlyphRingActive: {
      borderColor: colors.textPrimary,
    },
    myLocationGlyphDot: {
      width: mapControlLayout.myLocationGlyphDotSize,
      height: mapControlLayout.myLocationGlyphDotSize,
      borderRadius: mapControlLayout.myLocationGlyphDotSize / 2,
      backgroundColor: colors.accent,
    },
    myLocationGlyphDotActive: {
      backgroundColor: colors.textPrimary,
    },
    // --- The compass control (§4.2) ---
    compassButton: {
      position: 'absolute',
      right: themeSpacing.gutterInline,
      width: mapControlLayout.buttonSize,
      height: mapControlLayout.buttonSize,
      borderRadius: mapControlLayout.buttonSize / 2,
      backgroundColor: colors.surface,
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      alignItems: 'center',
      justifyContent: 'center',
    },
    compassLetter: {
      color: colors.accent,
      fontSize: themeTypography.typeBody.fontSize,
      fontWeight: '600',
    },
    // --- The floating bottom stack (§4.2, D6) ---
    /**
     * One column over the map's bottom edge: the scale bar, the
     * disabled-with-reason strip and the attribution notice, ordered like the
     * Compose dock so the three lines never collide. The column's bottom is
     * the present dock's measured height plus the tight stack.
     */
    floatingBottomStack: {
      position: 'absolute',
      left: themeSpacing.gutterInline,
      right: themeSpacing.gutterInline,
      alignItems: 'flex-start',
      gap: themeSpacing.stackTight,
    },
    /** The map's bottom-end corner: the layers control above the attribution. */
    bottomEndDock: {
      position: 'absolute',
      right: themeSpacing.gutterInline,
      alignItems: 'flex-end',
      gap: themeSpacing.stackTight,
    },
    // --- The on-map layer control and panel (docs/ReferenceAppUX.md §4.2) ---
    /** The collapsed round control above the attribution notice. */
    mapLayersControl: {
      width: mapControlLayout.buttonSize,
      height: mapControlLayout.buttonSize,
      borderRadius: mapControlLayout.buttonSize / 2,
      backgroundColor: colors.surface,
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      alignItems: 'center',
      justifyContent: 'center',
    },
    mapLayersControlGlyph: {
      color: colors.textPrimary,
      fontSize: mapLayersControlLayout.glyphFontSize,
      fontWeight: '600',
    },
    /** The non-default-layer dot at the control's top-trailing edge. */
    mapLayersActiveDot: {
      position: 'absolute',
      top: mapLayersControlLayout.activeDotInset,
      right: mapLayersControlLayout.activeDotInset,
      width: mapLayersControlLayout.activeDotDiameter,
      height: mapLayersControlLayout.activeDotDiameter,
      borderRadius: mapLayersControlLayout.activeDotDiameter / 2,
      backgroundColor: colors.accent,
    },
    /** The modal backdrop: the panel opens upward, clamped to 70% height. */
    mapLayersModalBackdrop: {
      flex: 1,
      backgroundColor: colors.scrim,
      justifyContent: 'flex-end',
      paddingHorizontal: themeSpacing.gutterInline,
    },
    mapLayersPanel: {
      maxHeight: mapLayersControlLayout.panelMaximumHeightFraction,
      backgroundColor: colors.surface,
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.surface,
      padding: themeSpacing.panelPadding,
      gap: themeSpacing.stackGroup,
    },
    mapLayersHeaderRow: {
      flexDirection: 'row',
      alignItems: 'center',
      justifyContent: 'space-between',
    },
    mapLayersTitle: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeTitle.fontSize,
      fontWeight: '600',
    },
    mapLayersCloseButton: {
      paddingHorizontal: themeSpacing.stackGroup,
      paddingVertical: themeSpacing.stackTight,
    },
    mapLayersCloseText: {
      color: colors.accent,
      fontSize: themeTypography.typeBody.fontSize,
      fontWeight: '600',
    },
    /** The overlays section's shared clear action, aligned with its rows. */
    mapLayersClearAction: {
      alignSelf: 'flex-start',
      paddingVertical: themeSpacing.stackTight,
    },
    mapLayersClearActionText: {
      color: colors.accent,
      fontSize: themeTypography.typeBody.fontSize,
      fontWeight: '600',
    },
    mapLayersScroll: {
      flexGrow: 0,
    },
    mapLayersScrollContent: {
      gap: themeSpacing.stackGroup,
    },
    mapLayersSection: {
      gap: themeSpacing.stackTight,
    },
    mapLayersSectionHeader: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeBodyStrong.fontSize,
      fontWeight: '600',
    },
    mapLayersRow: {
      flexDirection: 'row',
      alignItems: 'center',
      justifyContent: 'space-between',
      gap: themeSpacing.stackTight,
      paddingHorizontal: themeSpacing.stackGroup,
      paddingVertical: themeSpacing.stackTight,
      backgroundColor: colors.surface,
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.control,
    },
    mapLayersRowServing: {
      borderColor: colors.accent,
    },
    mapLayersRowDisabled: {
      opacity: 0.45,
    },
    mapLayersRowName: {
      flexShrink: 1,
      color: colors.textPrimary,
      fontSize: themeTypography.typeBody.fontSize,
    },
    mapLayersRowStatus: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    mapLayersRowStatusServing: {
      color: colors.accent,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    mapLayersSwitchRow: {
      flexDirection: 'row',
      alignItems: 'center',
      justifyContent: 'space-between',
      gap: themeSpacing.stackTight,
    },
    mapLayersSwitchLabel: {
      flexShrink: 1,
      color: colors.textPrimary,
      fontSize: themeTypography.typeBody.fontSize,
    },
    mapLayersReason: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    mapLayersStatus: {
      color: colors.warning,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    // --- The scale bar (§4.2) ---
    scaleBar: {
      alignItems: 'flex-start',
      backgroundColor: colors.surface,
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.inline,
      paddingHorizontal: themeSpacing.stackTight,
      paddingVertical: themeSpacing.stackTight / 2,
    },
    scaleBarText: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    scaleBarLine: {
      height: mapControlLayout.scaleBarLineThickness,
      backgroundColor: colors.textPrimary,
      marginTop: themeSpacing.stackTight / 2,
    },
    // --- The turn banner stack (§7.1) ---
    bannerStack: {
      position: 'absolute',
      left: themeSpacing.gutterInline,
      right: themeSpacing.gutterInline,
      gap: themeSpacing.stackTight,
      alignItems: 'center',
    },
    bannerCard: {
      width: '100%',
      backgroundColor: colors.surface,
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.surface,
      padding: themeSpacing.stackTight,
    },
    deviationCard: {
      borderColor: colors.warning,
    },
    deviationText: {
      color: colors.warning,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    bannerHeadline: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    bannerBody: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeTitle.fontSize,
    },
    bannerUpcoming: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeBody.fontSize,
    },
    arrivedText: {
      color: colors.positive,
      fontSize: themeTypography.typeManeuver.fontSize,
      textAlign: 'center',
    },
    // --- The sheet dock (§4.2's peek/half/full) ---
    dockHitSlop: {
      position: 'absolute',
      left: 0,
      right: 0,
      bottom: 0,
      top: 0,
    },
    sheetDock: {
      position: 'absolute',
      left: 0,
      right: 0,
      bottom: 0,
      backgroundColor: colors.surface,
      borderTopColor: colors.surfaceBorder,
      borderTopWidth: sheetDockTopBorderWidth,
      borderTopLeftRadius: themeRadii.surface,
      borderTopRightRadius: themeRadii.surface,
    },
    sheetGrabber: {
      alignSelf: 'center',
      width: sheetGrabberLayout.width,
      height: sheetGrabberLayout.height,
      borderRadius: sheetGrabberLayout.cornerRadius,
      backgroundColor: colors.surfaceBorder,
      marginTop: sheetGrabberLayout.topMargin,
    },
    sheetScroll: {
      flex: 1,
    },
    sheetContent: {
      paddingHorizontal: themeSpacing.gutterInline,
      paddingBottom: themeSpacing.panelPadding,
      gap: themeSpacing.stackTight,
    },
    sheetHeader: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    chipRow: {
      flexDirection: 'row',
      flexWrap: 'wrap',
      gap: themeSpacing.stackTight,
      alignItems: 'center',
    },
    chip: {
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.inline,
      paddingHorizontal: themeSpacing.stackGroup,
      paddingVertical: themeSpacing.stackTight / 2,
    },
    chipActive: {
      borderColor: colors.accent,
      backgroundColor: colors.accentSoft,
    },
    chipText: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeBody.fontSize,
    },
    chipTextActive: {
      color: colors.accent,
    },
    /** §6.2's warning-colored reason strip inside a disabled group. */
    reasonStripText: {
      color: colors.warning,
      fontSize: themeTypography.typeCaption.fontSize,
      lineHeight: themeTypography.typeCaption.lineHeight,
    },
    // --- The peek rows (§4.2's two-row peek) ---
    peekSearchField: {
      borderColor: colors.surfaceBorder,
      borderWidth: sheetRowBorderWidth,
      borderRadius: themeRadii.control,
      paddingVertical: sheetPeekRowVerticalPadding,
      paddingHorizontal: themeSpacing.stackGroup,
    },
    peekSearchFieldText: {
      color: colors.textPrimary,
      fontSize: sheetPeekRowTypography.searchField.fontSize,
      lineHeight: sheetPeekRowTypography.searchField.lineHeight,
    },
    peekActionRow: {
      flexDirection: 'row',
      flexWrap: 'wrap',
      gap: themeSpacing.stackTight,
      alignItems: 'center',
    },
    /** The peek's trip-source line (`plan.trip_source_demo_plan`). */
    peekTripSource: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
      lineHeight: themeTypography.typeCaption.lineHeight,
    },
    // --- The control buttons (the action row and card controls) ---
    actionControl: {
      borderColor: colors.surfaceBorder,
      borderWidth: sheetRowBorderWidth,
      borderRadius: themeRadii.control,
      paddingHorizontal: themeSpacing.stackGroup,
      paddingVertical: sheetPeekRowVerticalPadding,
    },
    actionControlPrimary: {
      borderColor: colors.accent,
      backgroundColor: colors.accentSoft,
    },
    actionControlText: {
      color: colors.textPrimary,
      fontSize: sheetPeekRowTypography.actionRow.fontSize,
      lineHeight: sheetPeekRowTypography.actionRow.lineHeight,
    },
    actionControlTextPrimary: {
      color: colors.accent,
    },
    // --- The route summary card (§4.2) ---
    routeSummaryCard: {
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.surface,
      padding: themeSpacing.panelPadding,
      gap: themeSpacing.stackTight / 2,
    },
    routeSummaryTitle: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeTitle.fontSize,
    },
    routeSummaryMeta: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeBody.fontSize,
    },
    // --- The search view (§4.1) ---
    searchViewHeading: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeTitle.fontSize,
      lineHeight: themeTypography.typeTitle.lineHeight,
    },
    searchFieldInput: {
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.control,
      paddingHorizontal: themeSpacing.stackGroup,
      paddingVertical: themeSpacing.stackTight,
      color: colors.textPrimary,
      fontSize: themeTypography.typeTitle.fontSize,
    },
    searchFieldInputFlexible: {
      flex: 1,
    },
    /** One labeled field: its A/B marker and label, the input + Search row, the chip. */
    searchFieldGroup: {
      gap: themeSpacing.stackTight,
    },
    searchFieldLabelRow: {
      flexDirection: 'row',
      alignItems: 'center',
      gap: themeSpacing.stackTight,
    },
    searchFieldMarker: {
      color: colors.accent,
      fontSize: themeTypography.typeCaption.fontSize,
      fontWeight: '600',
    },
    searchFieldLabel: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeBodyStrong.fontSize,
      fontWeight: '600',
    },
    searchFieldControlRow: {
      flexDirection: 'row',
      alignItems: 'center',
      gap: themeSpacing.stackTight,
    },
    searchFieldAction: {
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.control,
      paddingHorizontal: themeSpacing.stackGroup,
      paddingVertical: themeSpacing.stackTight,
    },
    searchFieldActionText: {
      color: colors.accent,
      fontSize: themeTypography.typeBodyStrong.fontSize,
      fontWeight: '600',
    },
    /** One field's My location chip; it fills that field only. */
    myLocationChip: {
      alignSelf: 'flex-start',
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.inline,
      paddingHorizontal: themeSpacing.stackGroup,
      paddingVertical: themeSpacing.stackTight / 2,
    },
    /** The chip's filled state: it marks the field it filled. */
    myLocationChipFilled: {
      borderColor: colors.accent,
    },
    /** The chip's disabled-with-reason state (§6.2). */
    myLocationChipDisabled: {
      opacity: 0.45,
    },
    myLocationChipText: {
      color: colors.accent,
      fontSize: themeTypography.typeBody.fontSize,
    },
    resultList: {
      gap: themeSpacing.stackTight,
    },
    searchResultsHeader: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    resultRow: {
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.inline,
      paddingHorizontal: themeSpacing.stackGroup,
      paddingVertical: themeSpacing.stackTight,
    },
    resultRowAssigned: {
      borderColor: colors.accent,
      backgroundColor: colors.accentSoft,
    },
    resultRowName: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeTitle.fontSize,
    },
    resultRowSecondary: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    // --- The offline maps card (§4.1's two honest lines) ---
    offlineCard: {
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.surface,
      padding: themeSpacing.panelPadding,
      gap: themeSpacing.stackTight / 2,
    },
    offlineCardTitle: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeTitle.fontSize,
    },
    offlineLine: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    /** One region row (the demo region and each stored region). */
    offlineStoredRegion: {
      borderTopColor: colors.surfaceBorder,
      borderTopWidth: 1,
      paddingTop: themeSpacing.stackTight,
      gap: themeSpacing.stackTight / 2,
    },
    offlineRegionName: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeBody.fontSize,
    },
    // --- The navigation drawer (§7.2) ---
    navigationDrawer: {
      position: 'absolute',
      left: 0,
      right: 0,
      bottom: 0,
      backgroundColor: colors.surface,
      borderTopColor: colors.surfaceBorder,
      borderTopWidth: 1,
      borderTopLeftRadius: themeRadii.surface,
      borderTopRightRadius: themeRadii.surface,
      padding: themeSpacing.panelPadding,
      gap: themeSpacing.stackTight,
    },
    drawerPhaseLabel: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeTitle.fontSize,
    },
    drawerEtaLabel: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeBody.fontSize,
    },
    /** The playback-speed row's label (§7.2; the chips carry the values). */
    drawerSpeedLabel: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeBody.fontSize,
    },
    drawerExitControl: {
      alignSelf: 'flex-start',
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.control,
      paddingHorizontal: themeSpacing.stackGroup,
      paddingVertical: themeSpacing.stackTight,
    },
    drawerExitControlText: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeBody.fontSize,
    },
    /** The drawer's speech row (§4.2: the toggle and the exit share the row). */
    drawerControlRow: {
      flexDirection: 'row',
      flexWrap: 'wrap',
      gap: themeSpacing.stackTight,
      alignItems: 'center',
    },
    /** The seek row: the label and one scrubber over the route's length. */
    seekRow: {
      flexDirection: 'row',
      alignItems: 'center',
      gap: themeSpacing.stackTight,
    },
    seekTrack: {
      flex: 1,
      height: seekTrackHeight,
      borderRadius: themeRadii.inline,
      backgroundColor: colors.surfaceBorder,
      overflow: 'hidden',
    },
    seekFill: {
      height: '100%',
      backgroundColor: colors.accent,
    },
    drawerSpeechReason: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
      flexShrink: 1,
    },
    /** The last-spoken speech line the drawer carries (§4.2's honest log). */
    drawerSpeechLine: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    // --- The diagnostics sheet (§6.1) ---
    diagnosticsScrim: {
      position: 'absolute',
      top: 0,
      left: 0,
      right: 0,
      bottom: 0,
      backgroundColor: colors.scrim,
      justifyContent: 'flex-end',
    },
    diagnosticsBackdrop: {
      flex: 1,
    },
    diagnosticsSheet: {
      backgroundColor: colors.surface,
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.surface,
      margin: themeSpacing.gutterInline,
      padding: themeSpacing.panelPadding,
      gap: themeSpacing.stackTight,
    },
    diagnosticsTitle: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeTitle.fontSize,
    },
    diagnosticsHeaderRow: {
      flexDirection: 'row',
      alignItems: 'center',
      justifyContent: 'space-between',
    },
    diagnosticsControlRow: {
      flexDirection: 'row',
      alignItems: 'center',
      gap: themeSpacing.stackTight,
    },
    diagnosticsLine: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    diagnosticsEventList: {
      gap: themeSpacing.stackTight / 2,
    },
    diagnosticsSectionHeader: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeBodyStrong.fontSize,
      fontWeight: themeTypography.typeBodyStrong.fontWeight,
      marginTop: themeSpacing.stackTight,
    },
    // --- The developer console (M58) ---
    // The floating control sits on the edge opposite the map layer control,
    // above the attribution notice; the sheet covers the map on a phone.
    developerConsoleControl: {
      position: 'absolute',
      left: themeSpacing.gutterInline,
      width: mapControlLayout.buttonSize,
      height: mapControlLayout.buttonSize,
      borderRadius: mapControlLayout.buttonSize / 2,
      backgroundColor: colors.surface,
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      alignItems: 'center',
      justifyContent: 'center',
    },
    developerConsoleControlText: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeCaption.fontSize,
      fontWeight: '600',
    },
    developerConsoleAlertDot: {
      position: 'absolute',
      top: developerConsoleAlertDotInset,
      right: developerConsoleAlertDotInset,
      width: developerConsoleAlertDotDiameter,
      height: developerConsoleAlertDotDiameter,
      borderRadius: developerConsoleAlertDotDiameter / 2,
      backgroundColor: colors.danger,
    },
    developerConsoleScrim: {
      position: 'absolute',
      top: 0,
      left: 0,
      right: 0,
      bottom: 0,
      backgroundColor: colors.scrim,
      justifyContent: 'flex-end',
    },
    developerConsoleBackdrop: {
      flex: 1,
    },
    developerConsoleSheet: {
      height: developerConsoleSheetHeightFraction,
      backgroundColor: colors.surface,
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderTopLeftRadius: themeRadii.surface,
      borderTopRightRadius: themeRadii.surface,
      padding: themeSpacing.panelPadding,
      gap: themeSpacing.stackTight,
    },
    developerConsoleTitle: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeTitle.fontSize,
      fontWeight: themeTypography.typeTitle.fontWeight,
    },
    developerConsoleHeaderRow: {
      flexDirection: 'row',
      alignItems: 'center',
      justifyContent: 'space-between',
    },
    developerConsoleFilterRow: {
      flexDirection: 'row',
      alignItems: 'center',
      gap: themeSpacing.stackTight,
    },
    developerConsoleFilterChip: {
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.control,
      paddingHorizontal: themeSpacing.stackGroup,
      paddingVertical: themeSpacing.stackTight / 2,
    },
    developerConsoleFilterChipActive: {
      backgroundColor: colors.accentSoft,
      borderColor: colors.accent,
    },
    developerConsoleFilterChipText: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    developerConsoleFilterChipTextActive: {
      color: colors.accent,
    },
    developerConsoleList: {
      flex: 1,
    },
    developerConsoleListContent: {
      gap: themeSpacing.stackTight,
    },
    developerConsoleRow: {
      borderBottomColor: colors.surfaceBorder,
      borderBottomWidth: 1,
      paddingVertical: themeSpacing.stackTight,
      gap: themeSpacing.stackTight / 2,
    },
    developerConsoleRowText: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    developerConsoleRowExcerpt: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    developerConsoleEmptyText: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeBody.fontSize,
    },
    developerConsoleDetail: {
      flex: 1,
      gap: themeSpacing.stackTight / 2,
    },
    developerConsoleDetailLine: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    developerConsoleSectionHeader: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeBodyStrong.fontSize,
      fontWeight: themeTypography.typeBodyStrong.fontWeight,
      marginTop: themeSpacing.stackTight,
    },
    developerConsoleStatusText: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    retryControl: {
      alignSelf: 'flex-start',
      borderColor: colors.accent,
      borderWidth: 1,
      borderRadius: themeRadii.control,
      paddingHorizontal: themeSpacing.stackGroup,
      paddingVertical: themeSpacing.stackTight / 2,
    },
    retryControlText: {
      color: colors.accent,
      fontSize: themeTypography.typeBody.fontSize,
    },
    actionControlDisabled: {
      opacity: 0.45,
    },
    // --- The settings sheet (§3's provider credential surface) ---
    settingsScrim: {
      position: 'absolute',
      top: 0,
      left: 0,
      right: 0,
      bottom: 0,
      backgroundColor: colors.scrim,
      justifyContent: 'flex-end',
    },
    settingsBackdrop: {
      flex: 1,
    },
    settingsSheet: {
      backgroundColor: colors.surface,
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.surface,
      margin: themeSpacing.gutterInline,
      padding: themeSpacing.panelPadding,
      gap: themeSpacing.stackTight,
    },
    /** The pinned title and Close action above the scrolling content. */
    settingsHeaderRow: {
      flexDirection: 'row',
      alignItems: 'center',
      justifyContent: 'space-between',
      gap: themeSpacing.stackTight,
    },
    /** The bounded content column: the panel's cap makes this the scroller. */
    settingsScroll: {
      flexShrink: 1,
    },
    settingsContent: {
      gap: themeSpacing.stackTight,
      paddingBottom: themeSpacing.stackTight,
    },
    settingsTitle: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeTitle.fontSize,
      lineHeight: themeTypography.typeTitle.lineHeight,
    },
    settingsSectionHeader: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    settingsKeylessLine: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeBody.fontSize,
    },
    settingsProviderRow: {
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.inline,
      padding: themeSpacing.stackGroup,
      gap: themeSpacing.stackTight,
    },
    settingsProviderName: {
      color: colors.textPrimary,
      fontSize: themeTypography.typeTitle.fontSize,
    },
    /** A capability row's serving line: the provider, or "Not configured". */
    settingsCapabilityServing: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeBody.fontSize,
    },
    settingsCredentialLabel: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    settingsCredentialInput: {
      minHeight: settingsLayout.credentialInputMinimumHeight,
      borderColor: colors.surfaceBorder,
      borderWidth: 1,
      borderRadius: themeRadii.control,
      paddingHorizontal: themeSpacing.stackGroup,
      paddingVertical: themeSpacing.stackTight,
      color: colors.textPrimary,
      fontSize: themeTypography.typeTitle.fontSize,
    },
    settingsCredentialStatus: {
      marginTop: settingsLayout.credentialStatusMarginTop,
      color: colors.textPrimary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    settingsCredentialHint: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    settingsCredentialAttribution: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    settingsRememberRow: {
      flexDirection: 'row',
      alignItems: 'center',
      justifyContent: 'space-between',
      gap: themeSpacing.stackTight,
    },
    settingsRememberLabel: {
      flexShrink: 1,
      color: colors.textPrimary,
      fontSize: themeTypography.typeBody.fontSize,
    },
    settingsRestartNote: {
      color: colors.textSecondary,
      fontSize: themeTypography.typeCaption.fontSize,
    },
    settingsUnavailableText: {
      color: colors.warning,
      fontSize: themeTypography.typeCaption.fontSize,
    },
  });
}