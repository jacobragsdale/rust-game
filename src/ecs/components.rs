//! Components are plain data. Behavior lives in `crate::systems`.

use ggez::glam::Vec2;

#[derive(Clone, Copy, Debug)]
pub struct Position(pub Vec2);

#[derive(Clone, Copy, Debug)]
pub struct Velocity(pub Vec2);

/// AABB extent of an entity, anchored at its `Position` (top-left corner).
#[derive(Clone, Copy, Debug)]
pub struct Size(pub Vec2);

/// The player. Tile-scale physics tuned for 32px tiles and the 50x37
/// Adventurer sprite. The collider is smaller than the sprite;
/// `Sprite::offset` aligns them.
#[derive(Clone, Debug)]
pub struct Avatar {
    pub prev_pos: Vec2,
    pub grounded: bool,
    pub facing_right: bool,
}

impl Avatar {
    pub const WIDTH: f32 = 20.0;
    pub const HEIGHT: f32 = 34.0;
    pub const RUN_SPEED: f32 = 200.0;
    pub const ACCEL: f32 = 1500.0;
    pub const DECEL: f32 = 1800.0;
    /// Jump clears 3 tiles up and ~4.5 tiles across at full run speed.
    pub const JUMP_SPEED: f32 = 520.0;
    pub const GRAVITY: f32 = 1400.0;
    pub const MAX_FALL: f32 = 900.0;

    pub fn new(spawn: Vec2) -> Self {
        Avatar {
            prev_pos: spawn,
            grounded: false,
            facing_right: true,
        }
    }
}

/// A sprite-sheet image drawn at `Position + offset`, animated by
/// `AnimationState` against a clip set.
#[derive(Clone, Debug)]
pub struct Sprite {
    /// Drawing offset from the entity's collider position (the collider is
    /// smaller than the sprite).
    pub offset: Vec2,
}

#[derive(Clone, Debug)]
pub struct AnimationState {
    pub clip: String,
    pub frame: usize,
    /// Seconds accumulated toward the next frame.
    pub elapsed: f32,
}

impl AnimationState {
    pub fn new(clip: &str) -> Self {
        AnimationState {
            clip: clip.to_string(),
            frame: 0,
            elapsed: 0.0,
        }
    }

    pub fn switch_to(&mut self, clip: &str) {
        if self.clip != clip {
            self.clip = clip.to_string();
            self.frame = 0;
            self.elapsed = 0.0;
        }
    }
}
