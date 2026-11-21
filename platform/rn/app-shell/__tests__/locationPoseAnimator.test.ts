/**
 * The React Native mirror of the shared location-pose animator (M72). The Rust
 * crate `map-refapp-core` is the reference model; these cases mirror its unit
 * tests and add the platform test the design note requires: a bounded
 * frame-to-frame ground distance, so a teleport fails.
 */

import {
  locationAnimationMaximumMilliseconds,
  locationAnimationMinimumMilliseconds,
  LocationPoseAnimator,
  signedAngularDifferenceDegrees,
  type LocationFix,
} from '@angoratek/rustic-maps-sdk/src/locationPoseAnimator.ts';

/** One fix at the given position and time. */
function fixAt(
  latitudeDegrees: number,
  receivedAtMilliseconds: number,
  overrides: Partial<LocationFix> = {},
): LocationFix {
  return {
    latitudeDegrees,
    longitudeDegrees: 0,
    courseDegrees: 0,
    horizontalAccuracyMeters: 10,
    receivedAtMilliseconds,
    ...overrides,
  };
}

/** One degree of latitude in meters (the spherical mean). */
const metersPerDegreeLatitude = 111_195;

test('the first fix draws immediately with no motion', () => {
  const animator = new LocationPoseAnimator();
  animator.pushFix(fixAt(1, 0));

  const pose = animator.poseAt(0);
  expect(pose).not.toBeNull();
  expect(pose!.latitudeDegrees).toBe(1);
  expect(pose!.isAnimating).toBe(false);
});

test('a motion is half way at half its duration', () => {
  const animator = new LocationPoseAnimator();
  animator.pushFix(fixAt(0, 0));
  animator.pushFix(fixAt(1, 500));

  const halfWay = animator.poseAt(750);
  expect(halfWay!.latitudeDegrees).toBeCloseTo(0.5, 10);
  expect(halfWay!.isAnimating).toBe(true);

  const settled = animator.poseAt(1000);
  expect(settled!.latitudeDegrees).toBe(1);
  expect(settled!.isAnimating).toBe(false);
});

test('a fix arriving mid flight retargets without a jump', () => {
  const animator = new LocationPoseAnimator();
  animator.pushFix(fixAt(0, 0));
  animator.pushFix(fixAt(1, 500));

  const before = animator.poseAt(750);
  animator.pushFix(fixAt(2, 750));
  const after = animator.poseAt(750);
  expect(after!.latitudeDegrees).toBeCloseTo(before!.latitudeDegrees, 10);

  // The new motion runs over the 250 ms between the fixes.
  const halfWay = animator.poseAt(875);
  expect(halfWay!.latitudeDegrees).toBeCloseTo(1.25, 10);
  const settled = animator.poseAt(1000);
  expect(settled!.latitudeDegrees).toBe(2);
});

test('the duration is the fix interval clamped to the band', () => {
  const longGap = new LocationPoseAnimator();
  longGap.pushFix(fixAt(0, 0));
  longGap.pushFix(fixAt(1, 5000));
  const nearEnd = longGap.poseAt(
    5000 + locationAnimationMaximumMilliseconds - 1,
  );
  expect(nearEnd!.isAnimating).toBe(true);
  const settled = longGap.poseAt(
    5000 + locationAnimationMaximumMilliseconds,
  );
  expect(settled!.isAnimating).toBe(false);

  const burst = new LocationPoseAnimator();
  burst.pushFix(fixAt(0, 0));
  burst.pushFix(fixAt(1, 10));
  const early = burst.poseAt(10 + locationAnimationMinimumMilliseconds - 1);
  expect(early!.isAnimating).toBe(true);
});

test('the course takes the shorter arc across 0 and 360', () => {
  const animator = new LocationPoseAnimator();
  animator.pushFix(fixAt(0, 0, { courseDegrees: 359 }));
  animator.pushFix(fixAt(1, 500, { courseDegrees: 1 }));

  const pose = animator.poseAt(750);
  expect(pose!.courseDegrees).not.toBeNull();
  // One degree forward (0° at the midpoint), never a full turn backwards.
  expect(
    pose!.courseDegrees! <= 1 || pose!.courseDegrees! >= 359,
  ).toBe(true);
  expect(signedAngularDifferenceDegrees(1, 359)).toBeCloseTo(2, 10);
  expect(signedAngularDifferenceDegrees(359, 1)).toBeCloseTo(-2, 10);

  const settled = animator.poseAt(1000);
  expect(settled!.courseDegrees).toBe(1);
});

test('accuracy eases linearly between the fixes', () => {
  const animator = new LocationPoseAnimator();
  animator.pushFix(fixAt(0, 0, { horizontalAccuracyMeters: 10 }));
  animator.pushFix(fixAt(1, 500, { horizontalAccuracyMeters: 20 }));

  const pose = animator.poseAt(750);
  expect(pose!.horizontalAccuracyMeters).toBe(15);
});

test('a clock running backwards clamps to the motion start', () => {
  const animator = new LocationPoseAnimator();
  animator.pushFix(fixAt(0, 0));
  animator.pushFix(fixAt(1, 500));

  const pose = animator.poseAt(100);
  expect(pose!.latitudeDegrees).toBe(0);
});

test('a reset makes the next fix draw immediately', () => {
  const animator = new LocationPoseAnimator();
  animator.pushFix(fixAt(0, 0));
  animator.reset();
  expect(animator.poseAt(0)).toBeNull();

  animator.pushFix(fixAt(5, 10));
  const pose = animator.poseAt(10);
  expect(pose!.latitudeDegrees).toBe(5);
  expect(pose!.isAnimating).toBe(false);
});

test('a zero length interval still animates for the documented floor', () => {
  const animator = new LocationPoseAnimator();
  animator.pushFix(fixAt(0, 0));
  animator.pushFix(fixAt(0.0001, 0));

  const start = animator.poseAt(0);
  expect(start!.isAnimating).toBe(true);
  expect(start!.latitudeDegrees).toBe(0);
  const settled = animator.poseAt(locationAnimationMinimumMilliseconds);
  expect(settled!.isAnimating).toBe(false);
  expect(settled!.latitudeDegrees).toBe(0.0001);
});

test('the drawn pose moves a bounded ground distance per frame, never a teleport', () => {
  const animator = new LocationPoseAnimator();
  // About 11 m per fix, the demo's walk cadence at navigation zoom.
  const startLatitude = 0;
  const endLatitude = 0.0001;
  animator.pushFix(fixAt(startLatitude, 0));
  animator.pushFix(fixAt(endLatitude, 500));

  const frameIntervalMilliseconds = 16;
  const maximumMetersPerFrame = 1;
  let previousLatitude = startLatitude;
  let travelledMeters = 0;
  for (
    let elapsed = 500;
    elapsed < 1000;
    elapsed += frameIntervalMilliseconds
  ) {
    const pose = animator.poseAt(elapsed);
    expect(pose).not.toBeNull();
    const metresThisFrame =
      Math.abs(pose!.latitudeDegrees - previousLatitude) *
      metersPerDegreeLatitude;
    expect(metresThisFrame).toBeLessThanOrEqual(maximumMetersPerFrame);
    travelledMeters += metresThisFrame;
    previousLatitude = pose!.latitudeDegrees;
  }
  const settled = animator.poseAt(1000);
  expect(settled!.isAnimating).toBe(false);
  travelledMeters +=
    Math.abs(settled!.latitudeDegrees - previousLatitude) *
    metersPerDegreeLatitude;
  // The whole segment was walked monotonically, with no skipped interval.
  expect(travelledMeters).toBeGreaterThan(
    Math.abs(endLatitude - startLatitude) * metersPerDegreeLatitude * 0.99,
  );
});
