/**
 * Jest-level unit tests of the reference application's map control cluster
 * helpers (docs/ReferenceAppUX.md §2.4/§4.2): the token values, the compass
 * visibility distance, the scale bar's 1-2-3-5-10 ladder and its labels —
 * pure math, no native side.
 */

import {
  bearingOffNorthDegrees,
  compassVisibilityBearingThresholdDegrees,
  distanceLabelText,
  mapControlLayout,
  mapControlZoomRepeatIntervalMilliseconds,
  scaleBarLadderMultipliers,
  scaleBarWidthMaximumPixels,
  scaleBarWidthMinimumPixels,
  selectScaleBarScale,
} from '../src/mapControls';

test('the §2.4 token values mirror the design language', () => {
  expect(mapControlZoomRepeatIntervalMilliseconds).toBe(250);
  expect(compassVisibilityBearingThresholdDegrees).toBe(1);
  expect(scaleBarWidthMinimumPixels).toBe(50);
  expect(scaleBarWidthMaximumPixels).toBe(120);
  expect([...scaleBarLadderMultipliers]).toEqual([1, 2, 3, 5, 10]);
});

test('the compass visibility distance wraps to the shortest direction', () => {
  expect(bearingOffNorthDegrees(0)).toBe(0);
  expect(bearingOffNorthDegrees(1)).toBe(1);
  expect(bearingOffNorthDegrees(359)).toBeCloseTo(1, 10);
  expect(bearingOffNorthDegrees(180)).toBe(180);
  expect(bearingOffNorthDegrees(190)).toBeCloseTo(170, 10);
  expect(bearingOffNorthDegrees(-10)).toBeCloseTo(10, 10);
  expect(bearingOffNorthDegrees(720)).toBe(0);
});

test('the scale bar picks the largest ladder distance within the width range', () => {
  const atOneMeterPerPixel = selectScaleBarScale(1.0);
  expect(atOneMeterPerPixel).not.toBeNull();
  expect(atOneMeterPerPixel!.distanceMeters).toBe(100);
  expect(atOneMeterPerPixel!.widthPixels).toBe(100);

  const atTenMetersPerPixel = selectScaleBarScale(10.0);
  expect(atTenMetersPerPixel).not.toBeNull();
  expect(atTenMetersPerPixel!.distanceMeters).toBe(1000);
  expect(atTenMetersPerPixel!.widthPixels).toBe(100);

  // A finer ground answer still fits a minimum-width bar (the 1 m candidate).
  const atCentimeterPerPixel = selectScaleBarScale(0.01);
  expect(atCentimeterPerPixel!.distanceMeters).toBe(1);
  expect(atCentimeterPerPixel!.widthPixels).toBe(100);
});

test('a non-positive or non-finite ground answer selects no bar', () => {
  expect(selectScaleBarScale(0)).toBeNull();
  expect(selectScaleBarScale(-2)).toBeNull();
  expect(selectScaleBarScale(Number.NaN)).toBeNull();
  expect(selectScaleBarScale(Number.POSITIVE_INFINITY)).toBeNull();
});

test('the bar label follows the metric ladder text', () => {
  expect(distanceLabelText(120)).toBe('120 m');
  expect(distanceLabelText(999)).toBe('999 m');
  expect(distanceLabelText(1000)).toBe('1 km');
  expect(distanceLabelText(2500)).toBe('3 km');
});

test('the layout values are named and stable', () => {
  expect(mapControlLayout.buttonSize).toBe(40);
  expect(mapControlLayout.stackSpacing).toBe(8);
  expect(mapControlLayout.compassNavigationTopClearance).toBe(176);
});
