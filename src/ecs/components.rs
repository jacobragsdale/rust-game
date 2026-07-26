//! Components are plain data. Behavior lives in `crate::systems`.

use ggez::glam::Vec2;

#[derive(Clone, Copy, Debug)]
pub struct Position(pub Vec2);

#[derive(Clone, Copy, Debug)]
pub struct Velocity(pub Vec2);

/// AABB extent of an entity, anchored at its `Position` (top-left corner).
#[derive(Clone, Copy, Debug)]
pub struct Size(pub Vec2);

/// Anything that falls, moves, and collides with the level.
///
/// [`crate::systems::body::move_bodies`] applies gravity, integrates, and
/// resolves collisions for every entity that has one, so a controller — the
/// player's, an NPC's, eventually a projectile's — only has to decide a
/// velocity and set the few per-tick knobs below. Before this existed, all of
/// that lived inside a `&mut Avatar` query and nothing else could reach it.
///
/// The tick has three phases and this type is the contract between them:
/// a controller writes the knobs, `move_bodies` writes the contact results,
/// and the controller reads those back on the next phase or the next tick.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    /// Position at the start of the current tick. Collision resolution uses it
    /// to work out which side a surface was approached from.
    pub prev_pos: Vec2,

    // --- knobs: set by the controller, read by `move_bodies` ---
    /// Downward acceleration in px/s². A per-tick value rather than a constant
    /// because variable jump height is exactly "heavier gravity while rising
    /// with the button released".
    pub gravity: f32,
    /// Terminal velocity.
    pub max_fall: f32,
    /// A tighter fall-speed cap for this tick only, applied after gravity.
    /// Wall sliding is the only user so far; `None` means no extra cap.
    pub fall_cap: Option<f32>,
    /// Fall through one-way platforms this tick (drop-through).
    pub ignore_one_way: bool,
    /// Skip movement entirely: no gravity, no integration, no collision. The
    /// death freeze uses this, and stuns and cutscenes will want it too.
    pub frozen: bool,

    // --- results: written by `move_bodies`, read by the controller ---
    pub grounded: bool,
    /// Standing on a full solid.
    pub on_solid: bool,
    /// Standing on a one-way platform.
    pub on_one_way: bool,
    /// True only on the tick the body touches down after being airborne.
    /// A transition, so it cannot be recovered from `grounded` alone.
    pub landed: bool,
}

impl Body {
    pub fn new(pos: Vec2, gravity: f32, max_fall: f32) -> Self {
        Body {
            prev_pos: pos,
            gravity,
            max_fall,
            fall_cap: None,
            ignore_one_way: false,
            frozen: false,
            grounded: false,
            on_solid: false,
            on_one_way: false,
            landed: false,
        }
    }

    /// Standing on a one-way platform and nothing else — the case where "down"
    /// should mean "drop through" rather than "crouch".
    pub fn on_one_way_only(&self) -> bool {
        self.on_one_way && !self.on_solid
    }
}

/// The player. Tile-scale physics tuned for 32px tiles and the 50x37
/// Adventurer sprite. The collider is smaller than the sprite;
/// `Sprite::offset` aligns them.
///
/// Only the state that is specific to *being the player* lives here. Where the
/// body is, how fast it is going, and whether it is on the ground belong to
/// [`Body`], which anything else in the world can have too.
#[derive(Clone, Debug)]
pub struct Avatar {
    pub facing_right: bool,
    /// Ticks since the avatar was last grounded (0 while grounded).
    pub coyote_ticks: u32,
    /// Countdown holding a recent jump press until it can be honored.
    pub jump_buffer: u32,
    /// Mid-air jumps still available (refilled on landing / wall jump).
    pub air_jumps: u8,
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

    pub fn new() -> Self {
        Avatar {
            facing_right: true,
            coyote_ticks: u32::MAX,
            jump_buffer: 0,
            air_jumps: Self::MAX_AIR_JUMPS,
            drop_ticks: 0,
            wall_sliding: false,
            wall_dir: 0.0,
            wall_coyote_ticks: u32::MAX,
            double_jumping: false,
            crouching: false,
            dead_ticks: 0,
        }
    }

    /// The body the player drives, at its spawn point.
    pub fn body(spawn: Vec2) -> Body {
        Body::new(spawn, Self::GRAVITY, Self::MAX_FALL)
    }

    pub fn dead(&self) -> bool {
        self.dead_ticks > 0
    }
}

impl Default for Avatar {
    fn default() -> Self {
        Avatar::new()
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
