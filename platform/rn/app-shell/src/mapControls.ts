/**
 * Pure values and helpers of the reference application's map control cluster
 * (docs/ReferenceAppUX.md §2.4 and §4.2): the compass visibility threshold,
 * the zoom controls' repeat cadence, and the scale bar's 1-2-3-5-10 distance
 * ladder. The module mirrors the web playground's `map_controls.mjs` and the
 * Compose application's `ScaleBarModel`; the jest suite pins the ladder and
 * threshold behavior, so the three platform mirrors cannot drift silently.
 */

import { themeSpacing } from './theme';

/** The full circle's degree sweep (structural math). */
export const fullCircleDegrees = 360.0;
/** The shortest-way wrap boundary (structural math). */
export const halfCircleDegrees = 180.0;

/** The bearing's off-north distance above which the compass shows (§2.4). */
export const compassVisibilityBearingThresholdDegrees = 1.0;

/** The zoom controls' held-repeat cadence in milliseconds (§2.4). */
export const mapControlZoomRepeatIntervalMilliseconds = 250;

/** One tap of the zoom +/− control (`map-control-zoom-step`, §2.4). */
export const mapControlZoomStep = 1;

/** The compass tap's eased "north up" return (`compass-renorth-animation`, §2.4). */
export const compassRenorthAnimationMilliseconds = 300;

/**
 * The longest motion one fix may start
 * (`location-animation-duration-milliseconds`, §2.4).
 */
export const locationAnimationDurationMilliseconds = 1000;

/**
 * The shortest motion one fix starts
 * (`location-animation-minimum-milliseconds`, §2.4).
 */
export const locationAnimationMinimumMilliseconds = 120;

/**
 * The puck never predicts motion between fixes
 * (`location-animation-dead-reckoning`, §2.4).
 */
export const locationAnimationDeadReckoning = 'off';

/** The scale bar's width floor in physical pixels (§2.4). */
export const scaleBarWidthMinimumPixels = 50.0;
/** The scale bar's width ceiling in physical pixels (§2.4). */
export const scaleBarWidthMaximumPixels = 120.0;

/** The clean distance ladder's multipliers, per decade (§2.4). */
export const scaleBarLadderMultipliers = [1, 2, 3, 5, 10] as const;

/**
 * The cluster's layout values (§4.2 fixes the behaviors, not the button
 * geometry): the Compose reference application's own layout constants.
 */
export const mapControlLayout = {
  /** The circular control button's size in density-independent units. */
  buttonSize: themeSpacing.mapControlDiameter,
  /** The gap between the cluster's stacked controls. */
  stackSpacing: 8,
  /** The compass's navigation-mode top clearance (the turn banner stack). */
  compassNavigationTopClearance: 176,
  /** The scale bar line's thickness. */
  scaleBarLineThickness: 2,
  /** The my-location glyph's outer ring size. */
  myLocationGlyphSize: 18,
  /** The my-location ring's stroke width. */
  myLocationGlyphRingStroke: 2,
  /** The my-location glyph's center dot size. */
  myLocationGlyphDotSize: 6,
} as const;

/** The ladder's lower decade scan range around one meter (§2.4). */
const scaleBarLadderLowerDecadeScanRange = 2;
/** The ladder's upper decade scan range around one meter (§2.4). */
const scaleBarLadderDecadeScanRange = 9;

/** Structural constant: `meters / metersPerKilometer = kilometers`. */
const metersPerKilometer = 1000.0;

/** One chosen scale-bar scale: the labeled distance and the bar's width. */
export interface SelectedScaleBarScale {
  /** The bar's labeled distance, in meters. */
  distanceMeters: number;
  /** The bar's width for that distance, in physical viewport pixels. */
  widthPixels: number;
}

/**
 * The camera bearing's smallest off-north angular distance in degrees (the
 * compass visibility threshold's input): wrapped into the shortest direction,
 * always in `[0, 180]`.
 */
export function bearingOffNorthDegrees(bearingDegrees: number): number {
  const normalizedDegrees =
    ((bearingDegrees % fullCircleDegrees) + fullCircleDegrees) % fullCircleDegrees;
  return normalizedDegrees <= halfCircleDegrees
    ? normalizedDegrees
    : fullCircleDegrees - normalizedDegrees;
}

/**
 * Selects the ladder distance fitting the token width range for one ground
 * meters-per-screen-pixel answer: the largest ladder distance whose bar width
 * does not exceed the maximum, when its width also reaches the minimum. A
 * non-finite or non-positive ground answer selects nothing (the bar then
 * renders no bar rather than an invented distance).
 */
export function selectScaleBarScale(
  groundMetersPerScreenPixel: number,
): SelectedScaleBarScale | null {
  if (
    !Number.isFinite(groundMetersPerScreenPixel) ||
    groundMetersPerScreenPixel <= 0.0
  ) {
    return null;
  }
  const ascendingCandidates: number[] = [];
  for (
    let decadeExponent = -scaleBarLadderLowerDecadeScanRange;
    decadeExponent <= scaleBarLadderDecadeScanRange;
    decadeExponent += 1
  ) {
    for (const multiplier of scaleBarLadderMultipliers) {
      ascendingCandidates.push(multiplier * 10 ** decadeExponent);
    }
  }
  ascendingCandidates.sort((left, right) => left - right);

  let selected: SelectedScaleBarScale | null = null;
  // Ascending scan; the last candidate under the maximum is the largest
  // fitting one, so it is kept as the selection when it also reaches the
  // minimum width.
  for (const candidateDistanceMeters of ascendingCandidates) {
    const candidateWidthPixels = candidateDistanceMeters / groundMetersPerScreenPixel;
    if (candidateWidthPixels > scaleBarWidthMaximumPixels) {
      break;
    }
    selected = {
      distanceMeters: candidateDistanceMeters,
      widthPixels: candidateWidthPixels,
    };
  }
  return selected !== null && selected.widthPixels >= scaleBarWidthMinimumPixels
    ? selected
    : null;
}

/** The ladder distance's metric label (the scale bar's text). */
export function distanceLabelText(distanceMeters: number): string {
  if (distanceMeters < metersPerKilometer) {
    return `${Math.trunc(distanceMeters)} m`;
  }
  // The Compose mirror rounds halves up (`kotlin.math.round`); JavaScript's
  // `Math.round` rounds halfway values up too, so the labels stay identical.
  return `${Math.round(distanceMeters / metersPerKilometer)} km`;
}
