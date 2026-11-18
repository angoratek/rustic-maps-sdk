# Location puck interpolation

**Status:** design, owner-directed 2026-11-05. Work: `PLAN.md` M72.
**Question it answers:** how does the location puck move between fixes, so it
glides instead of hopping, on every platform and at every fix cadence?

## Outcome

A visitor watching a trip sees the position disc (and its accuracy ring) move
continuously between fixes. A new fix that lands while the puck is still
moving retargets the motion instead of jumping, a course change takes the
short arc, and all four reference applications animate from the same model.

## The problem

Every shell hands the renderer the newest fix as a static record. The puck
therefore moves in discrete steps the size of the distance between fixes — at
a 250 ms cadence and the demo's pace that is about 7 m, which is a visible hop
at navigation zoom, and a slower or irregular fix stream makes it worse. The
accuracy ring jumps with it, and a course change snaps the arrow.

## Reference behaviour

- **MapLibre Native (Android/iOS)** draws the puck from a `LocationAnimator`:
  a fix starts an animation from the current interpolated pose to the new
  one over a configurable duration (the library default is about one second),
  the accuracy radius is interpolated with it, a fix arriving mid-flight
  retargets from the current pose, and the bearing is interpolated along the
  shorter arc. The animator is read on every frame; the location component
  never sets a static marker position.
- **MapLibre GL JS** eases marker moves with the same shape: the marker leaves
  its current position, arrives at the new one after a duration, and a new
  target restarts the ease from where the marker is.
- Neither library dead-reckons between fixes by default; the navigation SDKs
  add that on top when the fix stream is slow.

## Model

A pure animator lives in `map-refapp-core`, beside the follow-camera chase and
the easing primitives, so the four shells call shared math instead of keeping
a mirror:

```rust
pub struct LocationFix {
    pub latitude_degrees: f64,
    pub longitude_degrees: f64,
    pub course_degrees: Option<f64>,
    pub horizontal_accuracy_meters: Option<f64>,
    pub received_at_milliseconds: f64,
}

pub struct LocationPose {
    pub latitude_degrees: f64,
    pub longitude_degrees: f64,
    pub course_degrees: Option<f64>,
    pub horizontal_accuracy_meters: Option<f64>,
    pub is_animating: bool,
}

impl LocationPoseAnimator {
    /// Starts (or retargets) the motion toward the given fix.
    pub fn push_fix(&mut self, fix: LocationFix);
    /// The pose to draw at this instant. Never panics; a clock that runs
    /// backwards clamps to the last pose.
    pub fn pose_at(&mut self, now_milliseconds: f64) -> LocationPose;
    pub fn reset(&mut self);
}
```

The animator holds no clock and no platform handle: the caller passes the time
in, exactly like `SmoothstepTimedEase`, so a test drives it deterministically.

## Rules

1. **Position** interpolates linearly between the pose the animator is
   currently drawing and the new fix. Linear, not smoothed: a fix is a
   measurement, and overshoot between measurements would draw the puck where
   the user never was. The demo's own walker is linear too.
2. **Retarget, never jump.** A fix received before the previous motion
   finishes starts a new motion from the *current interpolated pose*, so the
   drawn position is continuous across every fix, however irregular.
3. **Duration** is the interval between the previous and the new fix, clamped
   to the documented band (`location-animation-duration-milliseconds`, with a
   floor of `location-animation-minimum-milliseconds`). A duration longer than
   the interval would lag the truth; a much shorter one re-creates the hop.
   The first fix of a session draws immediately.
4. **Bearing** takes the shorter arc (`signed_angular_difference_degrees`),
   eased with the existing smoothstep, so 359° → 1° turns one degree forward
   rather than a full turn backwards. An absent course keeps the previous
   bearing and does not animate.
5. **Accuracy** interpolates linearly in radius. A shrinking ring is how a
   visitor reads "the fix is getting better"; snapping it hides that.
6. **Idle.** When no motion is running, `pose_at` returns the latest pose and
   `is_animating` is false, so a shell can stop redrawing.
7. **Dead reckoning stays out** of the default model. The token exists so a
   host can opt in later; the note records why it is not the default: it
   draws a guess, and a wrong guess is worse than a still puck.

## Platform driving

Each shell already owns a frame pump; the animator rides it.

| Platform | Clock | Draw call |
|---|---|---|
| WebAssembly | the page's `requestAnimationFrame` pump | `setLocationIndicator` with the interpolated pose, once per frame |
| Android | the Compose frame clock | the canvas overlay draws the interpolated pose |
| iOS | SwiftUI `TimelineView` | the Canvas layer draws the interpolated pose |
| React Native | the module's frame callback | the same call as the web |

The follow camera keeps its own chase: it consumes fixes, not poses, and the
map must not be dragged by an interpolation artefact.

## Tokens

`platform/reference-tokens.json` (mirrored to the four shells and gated by
`check_ux_parity.mjs`):

| Token | Default | Meaning |
|---|---|---|
| `location-animation-duration-milliseconds` | 1000 | Longest motion one fix may start; the actual duration is the fix interval, clamped |
| `location-animation-minimum-milliseconds` | 120 | Floor, so a burst of fixes still animates rather than snapping |
| `location-animation-dead-reckoning` | `off` | Reserved; the design keeps it off |

## Tests

- Unit, in the crate: midpoint of a motion, a retarget whose drawn position is
  continuous, the shortest arc across 0°/360°, accuracy easing, a zero-length
  interval, and a clock that runs backwards.
- Browser: sample the indicator pose per frame across one fix interval and
  assert the frame-to-frame ground distance is bounded and monotonic along the
  segment; a teleport fails the case.
- Platform: the same bounded-delta assertion on Android, iOS and React Native.

## Open risks

- **Cadence mismatch.** A fix stream slower than the animation duration makes
  the puck arrive early and wait; the duration band is the mitigation, and the
  note records the measured cadences in the demo.
- **Accuracy spikes.** A fix with a much larger accuracy than the previous one
  produces a visible growth; interpolation makes it a growth rather than a
  flash, which is the honest reading.
