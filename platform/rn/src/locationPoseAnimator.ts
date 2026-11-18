/**
 * The location puck's interpolation between fixes, mirrored from the SDK's
 * shared Rust animation model.
 *
 * The React Native Turbo Module exposes the SDK contract only, so this shell
 * has no seam to the shared crate; the mirror is intentional and the Rust
 * crate remains the reference model. The math below is a one-for-one port:
 * the first fix draws immediately, a later fix starts a motion from the pose
 * currently drawn (never the previous fix), the duration is the fix interval
 * clamped to the documented band, the course takes the shorter arc and the
 * accuracy radius eases linearly.
 *
 * The animator holds no clock: the caller passes the time in, so the app
 * shell's frame clock drives it and a test drives it deterministically.
 */

/**
 * The longest motion one fix may start, in milliseconds
 * (`location-animation-duration-milliseconds`).
 */
export const locationAnimationMaximumMilliseconds = 1000;

/**
 * The shortest motion one fix starts, in milliseconds
 * (`location-animation-minimum-milliseconds`): a burst of fixes closer
 * together than this still animates, so a fast stream never teleports.
 */
export const locationAnimationMinimumMilliseconds = 120;

/** One full circle in degrees (structural math). */
const fullCircleDegrees = 360;

/** Half a circle in degrees (structural math). */
const halfCircleDegrees = 180;

/** One measured position the animator receives. */
export interface LocationFix {
  /** Measured latitude in degrees. */
  latitudeDegrees: number;
  /** Measured longitude in degrees. */
  longitudeDegrees: number;
  /** Course over ground in degrees clockwise from north, when reported. */
  courseDegrees: number | null;
  /** Horizontal accuracy radius in meters, when reported. */
  horizontalAccuracyMeters: number | null;
  /** When the fix was received, in milliseconds on the caller's clock. */
  receivedAtMilliseconds: number;
}

/** One pose the renderer draws. */
export interface LocationPose {
  /** Latitude to draw at, in degrees. */
  latitudeDegrees: number;
  /** Longitude to draw at, in degrees. */
  longitudeDegrees: number;
  /** Course to draw with, in degrees clockwise from north. */
  courseDegrees: number | null;
  /** Accuracy radius to draw, in meters. */
  horizontalAccuracyMeters: number | null;
  /** Whether a motion is still running, so a shell keeps drawing. */
  isAnimating: boolean;
}

/** The pose a fix draws when no motion is running. */
function poseFromFix(fix: LocationFix): LocationPose {
  return {
    latitudeDegrees: fix.latitudeDegrees,
    longitudeDegrees: fix.longitudeDegrees,
    courseDegrees: fix.courseDegrees,
    horizontalAccuracyMeters: fix.horizontalAccuracyMeters,
    isAnimating: false,
  };
}

/** Linear interpolation between two scalars. */
function interpolate(from: number, to: number, progress: number): number {
  return from + (to - from) * progress;
}

/** Linear interpolation where an absent endpoint holds the other one. */
function interpolateOptional(
  from: number | null,
  to: number | null,
  progress: number,
): number | null {
  if (from !== null && to !== null) {
    return interpolate(from, to, progress);
  }
  return from ?? to;
}

/** The signed shortest angular difference from one bearing to another. */
export function signedAngularDifferenceDegrees(
  toDegrees: number,
  fromDegrees: number,
): number {
  let difference = (toDegrees - fromDegrees) % fullCircleDegrees;
  if (difference > halfCircleDegrees) {
    difference -= fullCircleDegrees;
  }
  if (difference < -halfCircleDegrees) {
    difference += fullCircleDegrees;
  }
  return difference;
}

/** Wraps an angle into `[0, 360)`. */
function normalizeDegrees(angleDegrees: number): number {
  const wrapped = angleDegrees % fullCircleDegrees;
  return wrapped < 0 ? wrapped + fullCircleDegrees : wrapped;
}

/** Interpolates a course along the shorter arc. */
function interpolateCourse(
  from: number | null,
  to: number | null,
  progress: number,
): number | null {
  if (from !== null && to !== null) {
    return normalizeDegrees(
      from + signedAngularDifferenceDegrees(to, from) * progress,
    );
  }
  return from ?? to;
}

/** The animator that turns a fix stream into a drawn pose. */
export class LocationPoseAnimator {
  private fromPose: LocationPose | null = null;
  private toPose: LocationPose | null = null;
  private motionStartedAtMilliseconds = 0;
  private motionDurationMilliseconds = 0;
  private previousFixReceivedAtMilliseconds: number | null = null;

  /**
   * Starts or retargets the motion toward one fix.
   *
   * The first fix draws immediately. A later fix starts a motion from the
   * pose currently drawn — never from the previous fix — so the drawn
   * position is continuous however irregular the stream is.
   */
  pushFix(fix: LocationFix): void {
    const targetPose = poseFromFix(fix);
    const fromPose = this.poseAt(fix.receivedAtMilliseconds);
    if (fromPose === null) {
      this.fromPose = targetPose;
      this.toPose = targetPose;
      this.motionDurationMilliseconds = 0;
      this.previousFixReceivedAtMilliseconds = fix.receivedAtMilliseconds;
      return;
    }
    const interval =
      this.previousFixReceivedAtMilliseconds === null
        ? 0
        : fix.receivedAtMilliseconds -
          this.previousFixReceivedAtMilliseconds;
    this.motionDurationMilliseconds = Math.min(
      Math.max(interval, locationAnimationMinimumMilliseconds),
      locationAnimationMaximumMilliseconds,
    );
    this.fromPose = fromPose;
    this.toPose = targetPose;
    this.motionStartedAtMilliseconds = fix.receivedAtMilliseconds;
    this.previousFixReceivedAtMilliseconds = fix.receivedAtMilliseconds;
  }

  /**
   * The pose to draw at `nowMilliseconds`, or null before the first fix. A
   * clock that runs backwards clamps to the motion's start, so the frame
   * pump never draws a position outside the segment.
   */
  poseAt(nowMilliseconds: number): LocationPose | null {
    const fromPose = this.fromPose;
    const toPose = this.toPose;
    if (fromPose === null || toPose === null) {
      return null;
    }
    if (this.motionDurationMilliseconds <= 0) {
      return { ...toPose, isAnimating: false };
    }
    const elapsed = nowMilliseconds - this.motionStartedAtMilliseconds;
    const linearProgress = Math.min(
      Math.max(elapsed / this.motionDurationMilliseconds, 0),
      1,
    );
    if (linearProgress >= 1) {
      return { ...toPose, isAnimating: false };
    }
    return {
      latitudeDegrees: interpolate(
        fromPose.latitudeDegrees,
        toPose.latitudeDegrees,
        linearProgress,
      ),
      longitudeDegrees: interpolate(
        fromPose.longitudeDegrees,
        toPose.longitudeDegrees,
        linearProgress,
      ),
      courseDegrees: interpolateCourse(
        fromPose.courseDegrees,
        toPose.courseDegrees,
        linearProgress,
      ),
      horizontalAccuracyMeters: interpolateOptional(
        fromPose.horizontalAccuracyMeters,
        toPose.horizontalAccuracyMeters,
        linearProgress,
      ),
      isAnimating: true,
    };
  }

  /** The pose the next motion starts from, or null before the first fix. */
  currentPose(): LocationPose | null {
    return this.poseAt(
      this.motionStartedAtMilliseconds + this.motionDurationMilliseconds,
    );
  }

  /** Forgets every fix, so the next one draws immediately. */
  reset(): void {
    this.fromPose = null;
    this.toPose = null;
    this.motionStartedAtMilliseconds = 0;
    this.motionDurationMilliseconds = 0;
    this.previousFixReceivedAtMilliseconds = null;
  }
}
