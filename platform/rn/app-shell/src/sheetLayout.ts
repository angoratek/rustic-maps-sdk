/**
 * The home sheet's named layout values and its content-driven peek height
 * (docs/ReferenceAppUX.md §1's peek/half/full states, §2.4's home-sheet rows
 * and §4.2's grabber/two-row peek).
 *
 * The peek is the sheet's content-driven fold: `sheet-peek-content-rows`
 * (two rows, the destination search field and the action row) plus the
 * grabber strip and the dock's top border. It is never a viewport fraction
 * and never a bare literal — the height below is derived from the same
 * metrics the rows are styled with, so a token change moves both together.
 */

import { themeSpacing, themeTypography } from './theme';

/** `sheet-half-height` (§2.4): the home sheet's half state. */
export const sheetHalfHeightFraction = 0.5;
/** `sheet-half-phone-height` (§2.4): the half state's phone-width cap. */
export const sheetHalfPhoneHeightFraction = 0.55;
/** `sheet-peek-content-rows` (§2.4): the peek's exactly two content rows. */
export const sheetPeekContentRows = 2;

/** The full state's viewport-fraction cap (§1's `PANEL_MAXIMUM_HEIGHT_FRACTION`). */
export const sheetFullHeightFraction = 0.72;

/**
 * `PHONE_WIDTH_BREAKPOINT_PIXELS` (§1): at or below this viewport width the
 * phone-width rules apply — the half state uses `sheet-half-phone-height`.
 */
export const phoneWidthBreakpointPixels = 560;

/** The dock's top hairline border (the §4.2 panel border convention). */
export const sheetDockTopBorderWidth = 1;
/** The embedded row box's hairline border (search field and action control). */
export const sheetRowBorderWidth = 1;

/** The sheet grabber (§4.2: the drag target, centered under the top edge). */
export const sheetGrabberLayout = {
  width: 36,
  height: 4,
  /** `stack-tight` from the dock's top edge to the grabber. */
  topMargin: themeSpacing.stackTight,
  /** The grabber's fully rounded ends (half its height). */
  cornerRadius: 2,
} as const;

/**
 * The two peek rows' text metrics: the search field's entry text and the
 * action row's control label (§2.3's `type-title` and `type-body` rows). The
 * screen styles read these so the derived peek height cannot drift from the
 * rendered rows.
 */
export const sheetPeekRowTypography = {
  searchField: themeTypography.typeTitle,
  actionRow: themeTypography.typeBody,
} as const;

/** The inner vertical padding both peek rows share (`stack-tight`). */
export const sheetPeekRowVerticalPadding = themeSpacing.stackTight;

/** One embedded row's rendered height: borders, vertical padding, text line. */
function embeddedRowHeight(lineHeight: number): number {
  return sheetRowBorderWidth * 2 + sheetPeekRowVerticalPadding * 2 + lineHeight;
}

/**
 * The peek dock's rendered height: the top border, the grabber strip, the
 * search field row, the row gap and the action row. The dock is clipped at
 * this height, so the peek shows exactly the §2.4 two rows and no sliver of
 * the working content below them.
 */
export function peekDockHeight(): number {
  const grabberStripHeight =
    sheetGrabberLayout.topMargin + sheetGrabberLayout.height;
  const searchFieldRowHeight = embeddedRowHeight(
    sheetPeekRowTypography.searchField.lineHeight,
  );
  const actionRowHeight = embeddedRowHeight(
    sheetPeekRowTypography.actionRow.lineHeight,
  );
  return (
    sheetDockTopBorderWidth +
    grabberStripHeight +
    searchFieldRowHeight +
    themeSpacing.stackTight +
    actionRowHeight
  );
}
