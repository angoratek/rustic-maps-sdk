/**
 * The React Native mirror of the follow camera's bearing mode (M73). The Rust
 * crate `map-refapp-core` is the reference model; these cases mirror its unit
 * tests: course-up is the default, north-up holds across fixes, course-up
 * resumes the chase, a re-engage preserves the chosen mode and the trip's end
 * restores the default.
 */

import {
  FollowBearingMode,
  FollowCamera,
  FollowPhase,
  followZoomTowardTripEnd,
  navigationFollowZoom,
  navigationFollowZoomMaximum,
} from '@angoratek/rustic-maps-sdk/src/followBearingMode.ts';

test('course-up is the default and the trip captures the pre-trip camera', () => {
  const camera = new FollowCamera();
  expect(camera.phase).toBe(FollowPhase.Disengaged);
  expect(camera.bearingMode).toBe(FollowBearingMode.CourseUp);

  camera.engageTrip(38, -121, 12, 15, 0);
  expect(camera.phase).toBe(FollowPhase.Engaged);
  expect(camera.bearingMode).toBe(FollowBearingMode.CourseUp);
  expect(camera.targetZoomLevel).toBe(navigationFollowZoom);
  expect(camera.preTripCameraValue!.zoomLevel).toBe(12);
  expect(camera.preTripCameraValue!.bearingDegrees).toBe(15);
});

test('north-up holds across every following fix', () => {
  const camera = new FollowCamera();
  camera.engageTrip(38, -121, 12, 15, 0);
  camera.onFix(38.0001, -121.0001, 90, 0.8);
  expect(camera.targetBearingDegrees).toBe(90);

  camera.setBearingMode(FollowBearingMode.NorthUp);
  expect(camera.targetBearingDegrees).toBe(0);
  camera.onFix(38.0002, -121.0002, 120, 0.7);
  expect(camera.targetBearingDegrees).toBe(0);
  camera.breakOnGesture();
  camera.onFix(38.0003, -121.0003, 135, 0.6);
  expect(camera.targetBearingDegrees).toBe(0);
});

test('course-up resumes the eased chase on the next fix', () => {
  const camera = new FollowCamera();
  camera.engageTrip(38, -121, 12, 15, 0);
  camera.setBearingMode(FollowBearingMode.NorthUp);
  camera.onFix(38.0002, -121.0002, 120, 0.7);
  expect(camera.targetBearingDegrees).toBe(0);

  camera.setBearingMode(FollowBearingMode.CourseUp);
  expect(camera.targetBearingDegrees).toBeNull();
  camera.onFix(38.0003, -121.0003, 140, 0.6);
  expect(camera.targetBearingDegrees).toBe(140);
});

test('a re-engage preserves the visitor choice', () => {
  const camera = new FollowCamera();
  camera.engageTrip(38, -121, 12, 15, 0);
  camera.setBearingMode(FollowBearingMode.NorthUp);

  camera.breakOnGesture();
  expect(camera.phase).toBe(FollowPhase.BrokenByGesture);
  camera.onFix(38.0002, -121.0002, 120, 0.7);
  // A later fix re-targets the camera but does not resume follow.
  expect(camera.phase).toBe(FollowPhase.BrokenByGesture);
  expect(camera.targetBearingDegrees).toBe(0);

  camera.reengage();
  expect(camera.phase).toBe(FollowPhase.Engaged);
  expect(camera.bearingMode).toBe(FollowBearingMode.NorthUp);
  camera.onFix(38.0003, -121.0003, 140, 0.6);
  expect(camera.targetBearingDegrees).toBe(0);
});

test('the trip end restores the pre-trip camera and the default mode', () => {
  const camera = new FollowCamera();
  camera.engageTrip(38, -121, 12, 15, 0);
  camera.setBearingMode(FollowBearingMode.NorthUp);
  camera.onFix(38.0002, -121.0002, 120, 0.7);

  const restored = camera.restorePreTripCamera();
  expect(restored).not.toBeNull();
  expect(restored!.zoomLevel).toBe(12);
  expect(restored!.bearingDegrees).toBe(15);
  expect(camera.phase).toBe(FollowPhase.Disengaged);
  expect(camera.preTripCameraValue).toBeNull();
  expect(camera.targetBearingDegrees).toBeNull();
  expect(camera.bearingMode).toBe(FollowBearingMode.CourseUp);

  // A new trip starts course-up again.
  camera.engageTrip(38, -120, 10, 0, 0);
  camera.onFix(38.0005, -120.0005, 20, 0.9);
  expect(camera.targetBearingDegrees).toBe(20);
});

test('the follow zoom ramps to its maximum at arrival', () => {
  expect(followZoomTowardTripEnd(1)).toBe(navigationFollowZoom);
  expect(followZoomTowardTripEnd(0)).toBe(navigationFollowZoomMaximum);
  expect(followZoomTowardTripEnd(0.5)).toBeCloseTo(
    (navigationFollowZoom + navigationFollowZoomMaximum) / 2,
    10,
  );
  // A malformed progress report cannot leave the documented range.
  expect(followZoomTowardTripEnd(2)).toBe(navigationFollowZoom);
  expect(followZoomTowardTripEnd(-1)).toBe(navigationFollowZoomMaximum);
});
