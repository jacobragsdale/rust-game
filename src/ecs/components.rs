//! Components are plain data. Behavior lives in `crate::systems`.

use ggez::glam::Vec2;

#[derive(Clone, Copy, Debug)]
pub struct Position(pub Vec2);

#[derive(Clone, Copy, Debug)]
pub struct Velocity(pub Vec2);

/// AABB extent of an entity, anchored at its `Position` (top-left corner).
#[derive(Clone, Copy, Debug)]
pub struct Size(pub Vec2);

/// The player's body collides with this entity and can stand on it.
#[derive(Clone, Copy, Debug)]
pub struct Solid;

/// Moves left with the level's current scroll speed and is despawned once it
/// leaves the screen.
#[derive(Clone, Copy, Debug)]
pub struct Scroller;

/// Marker inserted when the player dies; systems switch to rag-doll behavior.
#[derive(Clone, Copy, Debug)]
pub struct Dead;

/// Circular hazard. `Position` is the center.
#[derive(Clone, Copy, Debug)]
pub struct SpikeBall;

impl SpikeBall {
    pub const BODY_RADIUS: f32 = 18.0;
    pub const SPIKE_LENGTH: f32 = 14.0;
    pub const TOTAL_RADIUS: f32 = Self::BODY_RADIUS + Self::SPIKE_LENGTH;
    pub const NUM_SPIKES: usize = 8;
}

/// Player movement state. Tuning constants are in px/sec (velocities) and
/// px/sec^2 (accelerations); the original per-frame values assumed 60 fps, so
/// they are converted with factors of 60 to stay identical under the fixed
/// 60 Hz timestep.
#[derive(Clone, Debug)]
pub struct Player {
    pub prev_pos: Vec2,
    pub grounded: bool,
    pub was_grounded: bool,
    pub can_jump: bool,
    pub grounded_ticks: u32,
    pub horizontal_input: f32,
    /// Horizontal velocity (px/sec) imparted by the platform being ridden.
    pub ride_speed: f32,
}

impl Player {
    pub const SIZE: f32 = 100.0;
    pub const MAX_SPEED: f32 = 12.0 * 60.0;
    pub const GROUND_ACCEL: f32 = 12.0 * 60.0 * 60.0;
    pub const AIR_ACCEL: f32 = 8.0 * 60.0 * 60.0;
    pub const JUMP_SPEED: f32 = 32.0 * 60.0;
    pub const GRAVITY: f32 = 1.3 * 60.0 * 60.0;
    pub const FRICTION: f32 = 3.0 * 60.0 * 60.0;
    /// Per-tick multiplier (fixed 60 Hz tick), applied when airborne with no input.
    pub const AIR_DRAG: f32 = 0.85;
    /// Velocity below this (px/sec) snaps to zero under air drag.
    pub const AIR_DRAG_STOP: f32 = 0.5 * 60.0;
    /// Ticks the player must stay grounded before jumping again (100 ms).
    pub const JUMP_DELAY_TICKS: u32 = 6;

    pub fn new(spawn: Vec2) -> Self {
        Player {
            prev_pos: spawn,
            grounded: true,
            was_grounded: false,
            can_jump: true,
            grounded_ticks: 0,
            horizontal_input: 0.0,
            ride_speed: 0.0,
        }
    }
}
