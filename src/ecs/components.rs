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
    /// Ticks since the avatar was last grounded (0 while grounded).
    pub coyote_ticks: u32,
    /// Countdown holding a recent jump press until it can be honored.
    pub jump_buffer: u32,
    /// Mid-air jumps still available (refilled on landing / wall jump).
    pub air_jumps: u8,
    /// Standing on a one-way platform (and nothing solid) last tick.
    pub on_one_way_only: bool,
    /// Countdown during which one-way platforms are ignored (drop-through).
    pub drop_ticks: u32,
    pub wall_sliding: bool,
    /// Which side the last touched wall is on (-1 left, +1 right).
    pub wall_dir: f32,
    /// Ticks since the avatar last touched a wall (0 while touching).
    pub wall_coyote_ticks: u32,
    /// Currently in the rising arc of a double jump (drives the animation).
    pub double_jumping: bool,
    pub crouching: bool,
    /// Death freeze countdown; respawns when it reaches zero.
    pub dead_ticks: u32,
}

impl Avatar {
    pub const WIDTH: f32 = 20.0;
    pub const HEIGHT: f32 = 34.0;
    pub const RUN_SPEED: f32 = 200.0;
    pub const ACCEL: f32 = 1500.0;
    pub const DECEL: f32 = 1800.0;
    /// Jump clears 3 tiles up and ~4.5 tiles across at full run speed.
    pub const JUMP_SPEED: f32 = 520.0;
    pub const DOUBLE_JUMP_SPEED: f32 = 470.0;
    pub const GRAVITY: f32 = 1400.0;
    /// Extra gravity while rising with the jump key released: tap = short
    /// hop, hold = full jump.
    pub const LOW_JUMP_GRAVITY: f32 = 1800.0;
    pub const MAX_FALL: f32 = 900.0;
    /// Fall speed cap while pressed against a wall.
    pub const WALL_SLIDE_SPEED: f32 = 70.0;
    /// Horizontal kick away from the wall on a wall jump.
    pub const WALL_JUMP_PUSH: f32 = 260.0;
    pub const WALL_JUMP_SPEED: f32 = 480.0;
    /// Jump grace after walking off a ledge (100 ms at 60 Hz).
    pub const COYOTE_TICKS: u32 = 6;
    /// Jump grace after leaving a wall. Wall contact is often a single tick —
    /// clipping a corner, or bouncing off on the way up — and without a grace
    /// window the wall jump only exists while a slide is held.
    pub const WALL_COYOTE_TICKS: u32 = 6;
    /// How early a jump press may land and still count.
    pub const JUMP_BUFFER_TICKS: u32 = 6;
    pub const MAX_AIR_JUMPS: u8 = 1;
    /// One-way platforms are ignored for this long after down+jump.
    pub const DROP_TICKS: u32 = 8;
    /// Death freeze before respawning (0.6 s).
    pub const DEATH_TICKS: u32 = 36;

    pub fn new(spawn: Vec2) -> Self {
        Avatar {
            prev_pos: spawn,
            grounded: false,
            facing_right: true,
            coyote_ticks: u32::MAX,
            jump_buffer: 0,
            air_jumps: Self::MAX_AIR_JUMPS,
            on_one_way_only: false,
            drop_ticks: 0,
            wall_sliding: false,
            wall_dir: 0.0,
            wall_coyote_ticks: u32::MAX,
            double_jumping: false,
            crouching: false,
            dead_ticks: 0,
        }
    }

    pub fn dead(&self) -> bool {
        self.dead_ticks > 0
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
