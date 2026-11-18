/**
 * The home sheet's peek height (D10): derived from the §2.4 home-sheet rows
 * and the two rendered peek rows, never a viewport fraction or a literal.
 */

import {
  peekDockHeight,
  sheetDockTopBorderWidth,
  sheetFullHeightFraction,
  sheetGrabberLayout,
  sheetHalfHeightFraction,
  sheetHalfPhoneHeightFraction,
  sheetPeekContentRows,
  sheetPeekRowTypography,
  sheetPeekRowVerticalPadding,
  sheetRowBorderWidth,
} from '../src/sheetLayout';
import { themeSpacing } from '../src/theme';

describe('sheet layout tokens', () => {
  test('the §2.4 home-sheet rows keep the table values', () => {
    expect(sheetHalfHeightFraction).toBe(0.5);
    expect(sheetHalfPhoneHeightFraction).toBe(0.55);
    expect(sheetPeekContentRows).toBe(2);
    expect(sheetFullHeightFraction).toBe(0.72);
  });

  test('the peek height is the grabber plus exactly two rendered rows', () => {
    const grabberStripHeight =
      sheetGrabberLayout.topMargin + sheetGrabberLayout.height;
    const searchFieldRowHeight =
      sheetRowBorderWidth * 2 +
      sheetPeekRowVerticalPadding * 2 +
      sheetPeekRowTypography.searchField.lineHeight;
    const actionRowHeight =
      sheetRowBorderWidth * 2 +
      sheetPeekRowVerticalPadding * 2 +
      sheetPeekRowTypography.actionRow.lineHeight;
    expect(peekDockHeight()).toBe(
      sheetDockTopBorderWidth +
        grabberStripHeight +
        searchFieldRowHeight +
        themeSpacing.stackTight +
        actionRowHeight,
    );
  });
});
