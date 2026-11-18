//! Cross-fade between adjoining tile zoom levels.
//!
//! A camera crossing half a zoom level replaces its whole requested tile set
//! in one frame: the new level's tiles are fetched from scratch while the
//! old level's pixels are no longer drawn, so the view flashes the background
//! and then fills in tile by tile. This module keeps the previous level on
//! screen instead: while the new level's tiles arrive it draws the retired
//! level over them at full opacity, and once enough of the new level is
//! present it dissolves the retired level out over a short fade. The tiles
//! are projected with the current camera, so the per-tile world spans scale
//! them automatically as the zoom changes.
//!
//! The surface session (native) and the canvas draw state (browser) each own
//! one [`ZoomTransition`] and feed it every frame's requested tile zoom level,
//! its present-tile fraction and a monotonic timestamp.

use crate::tile_scheduler::{PreparedTile, TileReadiness};

/// Duration of the dissolve from the previous tile level to the new one.
pub const ZOOM_TRANSITION_FADE_MILLISECONDS: f64 = 250.0;

/// Longest the previous level stays at full opacity while the new level's
/// tiles are still arriving. Without it a provider failure or a viewport
/// wider than its fetch budget could keep a stale level up indefinitely.
pub const ZOOM_TRANSITION_MAXIMUM_HOLD_MILLISECONDS: f64 = 1_000.0;

/// Fraction of the new level's visible tiles that must be present before the
/// dissolve starts. A couple of slow stragglers must not hold the retired
/// level at full opacity, which would delay the new detail indefinitely.
pub const ZOOM_TRANSITION_READY_TILE_FRACTION: f64 = 0.85;

/// What one frame's transition contributes to the draw list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomTransitionFrame {
    /// The retired tile zoom level to draw over the requested one.
    pub retained_tile_zoom_level: Option<u8>,
    /// Opacity of the retired level: one while it is held, ramping to zero
    /// across the fade.
    pub retained_opacity: f32,
    /// Whether the transition still needs frames. A running transition keeps
    /// the view unsettled so the platform's pump draws the fade out.
    pub is_running: bool,
}

/// No level is retained and no transition is running.
const IDLE_FRAME: ZoomTransitionFrame = ZoomTransitionFrame {
    retained_tile_zoom_level: None,
    retained_opacity: 0.0,
    is_running: false,
};

/// Phase of the cross-fade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ZoomTransitionPhase {
    /// No level change is being transitioned.
    Idle,
    /// The previous level draws at full opacity while the new level fills in.
    Holding,
    /// The previous level dissolves into the new level.
    Fading,
}

/// The tile-level cross-fade state of one surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomTransition {
    /// The tile zoom level the last frame requested; `None` before the first.
    requested_tile_zoom_level: Option<u8>,
    /// The level being held or faded out.
    previous_tile_zoom_level: Option<u8>,
    phase: ZoomTransitionPhase,
    /// When the current phase started, on the caller's monotonic clock.
    phase_started_milliseconds: f64,
}

impl Default for ZoomTransition {
    fn default() -> Self {
        Self {
            requested_tile_zoom_level: None,
            previous_tile_zoom_level: None,
            phase: ZoomTransitionPhase::Idle,
            phase_started_milliseconds: 0.0,
        }
    }
}

impl ZoomTransition {
    /// A surface that has not drawn a frame yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Observes one frame and returns what the frame draws for the retired
    /// level.
    ///
    /// `tile_zoom_level` is the level this frame requests, `now_milliseconds`
    /// a monotonic timestamp and `ready_tile_fraction` the fraction of that
    /// level's requested tiles the pool holds (see
    /// [`requested_level_ready_tile_fraction`]). A level change starts the
    /// hold; once the new level is ready or the hold deadline expires, the
    /// fade starts and the retired level's opacity ramps to zero.
    pub fn update(
        &mut self,
        tile_zoom_level: u8,
        now_milliseconds: f64,
        ready_tile_fraction: f64,
    ) -> ZoomTransitionFrame {
        match self.requested_tile_zoom_level {
            Some(previous_tile_zoom_level) if previous_tile_zoom_level != tile_zoom_level => {
                self.previous_tile_zoom_level = Some(previous_tile_zoom_level);
                self.phase = ZoomTransitionPhase::Holding;
                self.phase_started_milliseconds = now_milliseconds;
            }
            _ => {}
        }
        self.requested_tile_zoom_level = Some(tile_zoom_level);

        let elapsed_milliseconds = now_milliseconds - self.phase_started_milliseconds;
        match self.phase {
            ZoomTransitionPhase::Idle => IDLE_FRAME,
            ZoomTransitionPhase::Holding => {
                let new_level_is_ready = ready_tile_fraction >= ZOOM_TRANSITION_READY_TILE_FRACTION;
                if !new_level_is_ready
                    && elapsed_milliseconds < ZOOM_TRANSITION_MAXIMUM_HOLD_MILLISECONDS
                {
                    return ZoomTransitionFrame {
                        retained_tile_zoom_level: self.previous_tile_zoom_level,
                        retained_opacity: 1.0,
                        is_running: true,
                    };
                }
                // The new level is ready (or waited long enough): the fade
                // starts now, so this frame still draws the retired level at
                // full opacity.
                self.phase = ZoomTransitionPhase::Fading;
                self.phase_started_milliseconds = now_milliseconds;
                ZoomTransitionFrame {
                    retained_tile_zoom_level: self.previous_tile_zoom_level,
                    retained_opacity: 1.0,
                    is_running: true,
                }
            }
            ZoomTransitionPhase::Fading => {
                if elapsed_milliseconds >= ZOOM_TRANSITION_FADE_MILLISECONDS {
                    self.phase = ZoomTransitionPhase::Idle;
                    self.previous_tile_zoom_level = None;
                    return IDLE_FRAME;
                }
                let remaining_fraction =
                    1.0 - (elapsed_milliseconds / ZOOM_TRANSITION_FADE_MILLISECONDS);
                ZoomTransitionFrame {
                    retained_tile_zoom_level: self.previous_tile_zoom_level,
                    retained_opacity: remaining_fraction.clamp(0.0, 1.0) as f32,
                    is_running: true,
                }
            }
        }
    }

    /// Whether the transition still owes frames (a hold or a fade is under
    /// way).
    pub fn is_running(&self) -> bool {
        self.phase != ZoomTransitionPhase::Idle
    }

    /// The tile zoom level the last observed frame requested, or `None`
    /// before the first frame. A caller that keeps the retired level's own
    /// pixels compares this against the level it is about to observe, so it
    /// captures the right snapshot before [`Self::update`] starts the hold.
    pub fn observed_tile_zoom_level(&self) -> Option<u8> {
        self.requested_tile_zoom_level
    }

    /// Records `tile_zoom_level` as the current level and drops any hold or
    /// fade. A caller that has no clock for this frame calls it instead of
    /// [`Self::update`], so the level it draws is still observed and a later
    /// timed frame does not report a level change that already happened.
    pub fn observe_without_transition(&mut self, tile_zoom_level: u8) {
        self.requested_tile_zoom_level = Some(tile_zoom_level);
        self.previous_tile_zoom_level = None;
        self.phase = ZoomTransitionPhase::Idle;
        self.phase_started_milliseconds = 0.0;
    }
}

/// The fraction of a resolved frame's requested tiles that are present at
/// their own level. An empty request counts as ready: there is nothing left
/// for the new level to fill in.
pub fn requested_level_ready_tile_fraction(prepared_tiles: &[PreparedTile]) -> f64 {
    if prepared_tiles.is_empty() {
        return 1.0;
    }
    let present_tile_count = prepared_tiles
        .iter()
        .filter(|prepared_tile| prepared_tile.readiness == TileReadiness::Present)
        .count();
    present_tile_count as f64 / prepared_tiles.len() as f64
}

#[cfg(test)]
mod tests {
    use super::{
        ZOOM_TRANSITION_FADE_MILLISECONDS, ZOOM_TRANSITION_MAXIMUM_HOLD_MILLISECONDS,
        ZoomTransition, requested_level_ready_tile_fraction,
    };
    use crate::tile_key::TileKey;
    use crate::tile_scheduler::{PreparedTile, TileReadiness};

    /// One presented tile's verdict.
    fn present_tile() -> PreparedTile {
        PreparedTile {
            requested_tile_key: TileKey::new(0, 0, 10).expect("the test tile is valid"),
            readiness: TileReadiness::Present,
        }
    }

    /// One deferred tile's verdict.
    fn deferred_tile() -> PreparedTile {
        PreparedTile {
            requested_tile_key: TileKey::new(1, 0, 10).expect("the test tile is valid"),
            readiness: TileReadiness::Deferred,
        }
    }

    #[test]
    fn the_first_observed_level_starts_no_transition() {
        let mut transition = ZoomTransition::new();
        let frame = transition.update(10, 0.0, 0.0);
        assert_eq!(frame.retained_tile_zoom_level, None);
        assert!(!frame.is_running);
        assert!(!transition.is_running());
    }

    #[test]
    fn the_previous_level_is_held_at_full_opacity_until_the_new_one_is_ready() {
        let mut transition = ZoomTransition::new();
        transition.update(10, 0.0, 1.0);
        let holding = transition.update(11, 16.0, 0.2);
        assert_eq!(holding.retained_tile_zoom_level, Some(10));
        assert_eq!(holding.retained_opacity, 1.0);
        assert!(holding.is_running);

        let fading = transition.update(11, 100.0, 0.9);
        assert_eq!(fading.retained_tile_zoom_level, Some(10));
        assert_eq!(fading.retained_opacity, 1.0);

        let halfway = transition.update(11, 100.0 + ZOOM_TRANSITION_FADE_MILLISECONDS / 2.0, 1.0);
        assert_eq!(halfway.retained_tile_zoom_level, Some(10));
        assert!((halfway.retained_opacity - 0.5).abs() < 1.0e-6);
        assert!(halfway.is_running);

        let settled = transition.update(11, 100.0 + ZOOM_TRANSITION_FADE_MILLISECONDS, 1.0);
        assert_eq!(settled.retained_tile_zoom_level, None);
        assert!(!settled.is_running);
        assert!(!transition.is_running());
    }

    #[test]
    fn the_hold_expires_when_the_new_level_never_becomes_ready() {
        let mut transition = ZoomTransition::new();
        transition.update(9, 0.0, 1.0);
        let holding = transition.update(10, 0.0, 0.0);
        assert_eq!(holding.retained_tile_zoom_level, Some(9));

        let still_holding =
            transition.update(10, ZOOM_TRANSITION_MAXIMUM_HOLD_MILLISECONDS - 1.0, 0.0);
        assert_eq!(still_holding.retained_opacity, 1.0);

        let fading = transition.update(10, ZOOM_TRANSITION_MAXIMUM_HOLD_MILLISECONDS, 0.0);
        assert_eq!(fading.retained_tile_zoom_level, Some(9));
        let faded_further = transition.update(
            10,
            ZOOM_TRANSITION_MAXIMUM_HOLD_MILLISECONDS + ZOOM_TRANSITION_FADE_MILLISECONDS / 2.0,
            0.0,
        );
        assert!((faded_further.retained_opacity - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn a_camera_jump_retains_the_level_it_left_and_fades_from_it() {
        let mut transition = ZoomTransition::new();
        transition.update(8, 0.0, 1.0);
        let jumped = transition.update(12, 10.0, 0.0);
        assert_eq!(jumped.retained_tile_zoom_level, Some(8));
        assert_eq!(jumped.retained_opacity, 1.0);

        let fading = transition.update(12, 20.0, 1.0);
        assert_eq!(fading.retained_tile_zoom_level, Some(8));
    }

    #[test]
    fn a_level_change_during_a_fade_starts_a_new_hold() {
        let mut transition = ZoomTransition::new();
        transition.update(10, 0.0, 1.0);
        transition.update(11, 10.0, 1.0);
        let halfway = transition.update(11, 10.0 + ZOOM_TRANSITION_FADE_MILLISECONDS / 2.0, 1.0);
        assert!(halfway.retained_opacity < 1.0);

        let returned = transition.update(10, 200.0, 0.0);
        assert_eq!(returned.retained_tile_zoom_level, Some(11));
        assert_eq!(returned.retained_opacity, 1.0);
    }

    #[test]
    fn the_ready_fraction_counts_only_tiles_present_at_their_own_level() {
        assert_eq!(requested_level_ready_tile_fraction(&[]), 1.0);
        assert_eq!(
            requested_level_ready_tile_fraction(&[present_tile(), deferred_tile()]),
            0.5
        );
        assert_eq!(
            requested_level_ready_tile_fraction(&[present_tile(), present_tile()]),
            1.0
        );
    }
}
