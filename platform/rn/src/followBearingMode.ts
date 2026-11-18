/**
 * The navigation follow camera's bearing mode, mirrored from the SDK's shared
 * Rust follow-camera model.
 *
 * The React Native Turbo Module exposes the SDK contract only, so this shell
 * has no seam to the shared crate; the mirror is intentional and the Rust
 * crate remains the reference model. The port keeps the mode transitions the
 * compass control drives:
 *
 * - `courseUp` is the default at trip start;
 * - choosing `northUp` pins the target bearing at 0 and every following fix
 *   keeps it there;
 * - a gesture break does not change the chosen mode, so the my-location
 *   re-engage preserves it;
 * - choosing `courseUp` again clears the pin and the next fix resumes the
 *   eased course chase;
 * - the trip's end restores the default.
 */

/** The default number of recent fixes the course median smooths over. */
export const courseSmoothingWindowFixes = 5;

/** The follow camera's fixed deep zoom at trip start. */
export const navigationFollowZoom = 17;

/** The follow camera's zoom clamp toward trip end. */
export const navigationFollowZoomMaximum = 17.5;

/** One full circle in degrees (structural math). */
const fullCircleDegrees = 360;

/** How the follow camera treats its bearing while a trip runs. */
export enum FollowBearingMode {
  /** The camera turns with the trip's smoothed course — the default. */
  CourseUp = 'courseUp',
  /** The visitor chose north-up; bearing stays 0 until course-up is chosen. */
  NorthUp = 'northUp',
}

/** The camera's engage, gesture-break and re-engage states. */
export enum FollowPhase {
  /** No trip is running. */
  Disengaged = 'disengaged',
  /** The camera follows the trip's fixes. */
  Engaged = 'engaged',
  /** A user gesture broke follow; only an explicit re-engage resumes it. */
  BrokenByGesture = 'brokenByGesture',
}

/** The camera values captured at trip start and restored at trip end. */
export interface PreTripCamera {
  centerLatitudeDegrees: number;
  centerLongitudeDegrees: number;
  zoomLevel: number;
  bearingDegrees: number;
  pitchDegrees: number;
}

/** The angular distance between two bearings, always in `[0, 180]`. */
export function angularDistanceDegrees(
  firstDegrees: number,
  secondDegrees: number,
): number {
  const absoluteDifference =
    Math.abs(firstDegrees - secondDegrees) % fullCircleDegrees;
  return absoluteDifference > fullCircleDegrees / 2
    ? fullCircleDegrees - absoluteDifference
    : absoluteDifference;
}

/** A fixed-window circular median over recent course-over-ground samples. */
export class CourseMedianSmoother {
  private readonly windowFixCount: number;
  private recentCourseDegrees: number[] = [];

  constructor(windowFixCount: number = courseSmoothingWindowFixes) {
    this.windowFixCount = windowFixCount;
  }

  /** Adds one fix's course, dropping the window's oldest once it is full. */
  add(courseDegrees: number): void {
    this.recentCourseDegrees.push(courseDegrees);
    if (this.recentCourseDegrees.length > this.windowFixCount) {
      this.recentCourseDegrees.shift();
    }
  }

  /** The circular median, or null while the window holds no sample. */
  median(): number | null {
    let bestMedian: number | null = null;
    let bestDistanceSum = Number.POSITIVE_INFINITY;
    for (const candidateDegrees of this.recentCourseDegrees) {
      const distanceSum = this.recentCourseDegrees.reduce(
        (sum, otherDegrees) =>
          sum + angularDistanceDegrees(candidateDegrees, otherDegrees),
        0,
      );
      // Ties keep the earliest sample in the window, exactly like the crate.
      if (distanceSum < bestDistanceSum) {
        bestDistanceSum = distanceSum;
        bestMedian = candidateDegrees;
      }
    }
    return bestMedian;
  }

  /** Empties the window. */
  reset(): void {
    this.recentCourseDegrees = [];
  }
}

/** The follow camera's targets and phase. */
export class FollowCamera {
  private phaseState: FollowPhase = FollowPhase.Disengaged;
  private preTripCamera: PreTripCamera | null = null;
  private targetCenterLatitudeDegreesValue = 0;
  private targetCenterLongitudeDegreesValue = 0;
  private targetZoomLevelValue = 0;
  private targetBearingDegreesValue: number | null = null;
  private bearingModeState: FollowBearingMode = FollowBearingMode.CourseUp;
  private readonly courseSmoother: CourseMedianSmoother;

  constructor() {
    this.courseSmoother = new CourseMedianSmoother();
  }

  /** The current follow phase. */
  get phase(): FollowPhase {
    return this.phaseState;
  }

  /** The captured pre-trip camera, or null outside a trip. */
  get preTripCameraValue(): PreTripCamera | null {
    return this.preTripCamera;
  }

  /** The target centre latitude in degrees. */
  get targetCenterLatitudeDegrees(): number {
    return this.targetCenterLatitudeDegreesValue;
  }

  /** The target centre longitude in degrees. */
  get targetCenterLongitudeDegrees(): number {
    return this.targetCenterLongitudeDegreesValue;
  }

  /** The target zoom level. */
  get targetZoomLevel(): number {
    return this.targetZoomLevelValue;
  }

  /** The target bearing in degrees, or null until a course is known. */
  get targetBearingDegrees(): number | null {
    return this.targetBearingDegreesValue;
  }

  /** How the camera treats its bearing: course-up or north-up. */
  get bearingMode(): FollowBearingMode {
    return this.bearingModeState;
  }

  /**
   * Chooses how the camera treats its bearing. North-up pins the target at 0;
   * course-up clears it and the next fix resumes the eased chase.
   */
  setBearingMode(bearingMode: FollowBearingMode): void {
    this.bearingModeState = bearingMode;
    this.courseSmoother.reset();
    this.targetBearingDegreesValue =
      bearingMode === FollowBearingMode.NorthUp ? 0 : null;
  }

  /**
   * Applies the trip's follow camera and captures the pre-trip camera for the
   * later restore. The fixed deep zoom applies at once.
   */
  engageTrip(
    currentCenterLatitudeDegrees: number,
    currentCenterLongitudeDegrees: number,
    currentZoomLevel: number,
    currentBearingDegrees: number,
    currentPitchDegrees: number,
  ): void {
    this.preTripCamera = {
      centerLatitudeDegrees: currentCenterLatitudeDegrees,
      centerLongitudeDegrees: currentCenterLongitudeDegrees,
      zoomLevel: currentZoomLevel,
      bearingDegrees: currentBearingDegrees,
      pitchDegrees: currentPitchDegrees,
    };
    this.phaseState = FollowPhase.Engaged;
    this.targetCenterLatitudeDegreesValue = currentCenterLatitudeDegrees;
    this.targetCenterLongitudeDegreesValue = currentCenterLongitudeDegrees;
    this.targetZoomLevelValue = navigationFollowZoom;
    this.targetBearingDegreesValue = null;
    this.bearingModeState = FollowBearingMode.CourseUp;
    this.courseSmoother.reset();
  }

  /** A user gesture breaks follow immediately; the chosen mode survives. */
  breakOnGesture(): void {
    if (this.phaseState === FollowPhase.Engaged) {
      this.phaseState = FollowPhase.BrokenByGesture;
    }
  }

  /**
   * Applies one location fix. The fix re-targets centre, smoothed bearing and
   * ramped zoom, but a gesture-broken follow stays broken until `reengage`.
   */
  onFix(
    latitudeDegrees: number,
    longitudeDegrees: number,
    courseHeadingDegrees: number | null,
    remainingDistanceFraction: number,
  ): void {
    if (this.phaseState !== FollowPhase.BrokenByGesture) {
      this.phaseState = FollowPhase.Engaged;
    }
    this.targetCenterLatitudeDegreesValue = latitudeDegrees;
    this.targetCenterLongitudeDegreesValue = longitudeDegrees;
    this.targetZoomLevelValue = followZoomTowardTripEnd(
      remainingDistanceFraction,
    );
    if (courseHeadingDegrees !== null) {
      this.courseSmoother.add(courseHeadingDegrees);
      if (this.bearingModeState === FollowBearingMode.CourseUp) {
        this.targetBearingDegreesValue = this.courseSmoother.median();
      }
    }
    if (this.bearingModeState === FollowBearingMode.NorthUp) {
      this.targetBearingDegreesValue = 0;
    }
  }

  /** The my-location control's explicit re-engage. */
  reengage(): void {
    if (this.phaseState === FollowPhase.BrokenByGesture) {
      this.phaseState = FollowPhase.Engaged;
    }
  }

  /**
   * Ends the trip, returning the captured pre-trip camera for the caller to
   * restore. The camera returns to disengaged and the default mode.
   */
  restorePreTripCamera(): PreTripCamera | null {
    const capturedCamera = this.preTripCamera;
    this.phaseState = FollowPhase.Disengaged;
    this.preTripCamera = null;
    this.targetBearingDegreesValue = null;
    this.bearingModeState = FollowBearingMode.CourseUp;
    this.courseSmoother.reset();
    return capturedCamera;
  }
}

/**
 * The follow camera's zoom toward trip end: the base zoom ramps linearly with
 * the travelled fraction to the maximum at arrival.
 */
export function followZoomTowardTripEnd(
  remainingDistanceFraction: number,
): number {
  const clampedRemainingFraction = Math.min(
    Math.max(remainingDistanceFraction, 0),
    1,
  );
  const travelledFraction = 1 - clampedRemainingFraction;
  return (
    navigationFollowZoom +
    (navigationFollowZoomMaximum - navigationFollowZoom) * travelledFraction
  );
}
