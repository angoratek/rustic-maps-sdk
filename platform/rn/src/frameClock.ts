/**
 * The React Native shell's frame clock.
 *
 * The location puck is drawn from an interpolated pose on every frame while a
 * motion runs, exactly like the web pump and the native shells' frame
 * callbacks. The clock is injectable so jest can step it deterministically;
 * the default scheduler is the host's `requestAnimationFrame`.
 *
 * The clock reports the scheduler's wall clock rather than the animation
 * callback's timestamp, because the fix stream stamps fixes with the same
 * wall clock the animator reads.
 */

/** The frame scheduling seam; the host's animation frame is the default. */
export interface FrameScheduler {
  /** Schedules one callback and answers its cancellation handle. */
  requestFrame(callback: () => void): number;
  /** Cancels a scheduled callback. */
  cancelFrame(handle: number): void;
  /** The clock the animator and the fix stream share, in milliseconds. */
  nowMilliseconds(): number;
}

/** The host scheduler: `requestAnimationFrame` and the epoch clock. */
export const animationFrameScheduler: FrameScheduler = {
  requestFrame: callback => requestAnimationFrame(callback),
  cancelFrame: handle => cancelAnimationFrame(handle),
  nowMilliseconds: () => Date.now(),
};

/**
 * One frame loop: `start` runs `onFrame` once per frame until it answers
 * false or `stop` cancels the pending frame. A second start replaces the
 * previous callback.
 */
export class FrameClock {
  private readonly scheduler: FrameScheduler;
  private pendingFrameHandle: number | null = null;
  private onFrame: ((nowMilliseconds: number) => boolean) | null = null;

  constructor(scheduler: FrameScheduler = animationFrameScheduler) {
    this.scheduler = scheduler;
  }

  /** Whether a frame is pending. */
  get isRunning(): boolean {
    return this.pendingFrameHandle !== null;
  }

  /**
   * Starts the loop with `onFrame`; the callback answers whether another
   * frame is needed. A frame already pending is cancelled first.
   */
  start(onFrame: (nowMilliseconds: number) => boolean): void {
    this.stop();
    this.onFrame = onFrame;
    this.scheduleNextFrame();
  }

  /** Cancels the pending frame and forgets the callback. */
  stop(): void {
    if (this.pendingFrameHandle !== null) {
      this.scheduler.cancelFrame(this.pendingFrameHandle);
      this.pendingFrameHandle = null;
    }
    this.onFrame = null;
  }

  private scheduleNextFrame(): void {
    this.pendingFrameHandle = this.scheduler.requestFrame(() => {
      this.pendingFrameHandle = null;
      const onFrame = this.onFrame;
      if (onFrame === null) {
        return;
      }
      if (onFrame(this.scheduler.nowMilliseconds())) {
        this.scheduleNextFrame();
      } else {
        this.onFrame = null;
      }
    });
  }
}
