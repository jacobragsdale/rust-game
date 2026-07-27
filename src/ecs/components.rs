//! Components are plain data. Behavior lives in `crate::systems`.

use std::sync::Arc;

use ggez::glam::Vec2;

use crate::assets::ClipSet;

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
    /// Ticks left in a slide (0 when not sliding).
    pub slide_ticks: u32,
    /// Ticks until another slide may start.
    pub slide_cooldown: u32,
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
    pub const MAX_HEALTH: i32 = 5;
    /// The first link of the ground combo, from `assets/data/attacks.ron`.
    /// The rest of the chain is data: each attack names its own successor.
    pub const ATTACK: &'static str = "player_slash1";
    /// What a press in mid-air performs instead.
    pub const AIR_ATTACK: &'static str = "player_air";
    /// How long a slide lasts, matching the `slide` clip.
    pub const SLIDE_TICKS: u32 = 20;
    /// Slides start faster than a run and bleed off across their length.
    pub const SLIDE_SPEED: f32 = 300.0;
    /// Ticks after a slide before another may start, so it is a move rather
    /// than a faster way to walk.
    pub const SLIDE_COOLDOWN: u32 = 20;

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
            slide_ticks: 0,
            slide_cooldown: 0,
            dead_ticks: 0,
        }
    }

    pub fn sliding(&self) -> bool {
        self.slide_ticks > 0
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

/// Hit points, plus the two timers that follow from losing some.
///
/// The same component on the player and on every enemy, so one damage system
/// serves both and they cannot drift apart.
#[derive(Clone, Copy, Debug)]
pub struct Health {
    pub current: i32,
    pub max: i32,
    /// Ticks of invulnerability left. Prevents a single swing that overlaps
    /// for six ticks from dealing six hits.
    pub iframes: u32,
    /// Ticks left during which the controller does not steer.
    ///
    /// Distinct from `Body::frozen`, which stops movement dead. Knockback has
    /// to keep flying while the victim has no say in it, so this suppresses
    /// the *controller* and leaves the body integrating normally.
    pub hitstun: u32,
    /// How long invulnerability lasts after a hit, for this entity.
    ///
    /// Per-entity because the two sides want opposite things. The player's is
    /// long: a mercy window that stops a bad moment becoming a death spiral.
    /// An enemy's must be shorter than the gap between combo links, or only
    /// the first hit of a combo ever lands and the other two are decoration.
    pub iframe_ticks: u32,
}

impl Health {
    /// The player's mercy window, half a second.
    pub const PLAYER_IFRAMES: u32 = 30;
    /// An enemy's, short enough that every link of a combo connects.
    pub const ENEMY_IFRAMES: u32 = 10;

    pub fn new(max: i32, iframe_ticks: u32) -> Self {
        Health {
            current: max,
            max,
            iframes: 0,
            hitstun: 0,
            iframe_ticks,
        }
    }

    pub fn dead(&self) -> bool {
        self.current <= 0
    }

    /// Can this be hit right now?
    pub fn vulnerable(&self) -> bool {
        !self.dead() && self.iframes == 0
    }

    /// Has this taken any damage? Enemies only show a health bar once it has.
    pub fn damaged(&self) -> bool {
        self.current < self.max
    }

    pub fn fraction(&self) -> f32 {
        if self.max <= 0 {
            0.0
        } else {
            (self.current.max(0) as f32) / (self.max as f32)
        }
    }
}

/// A swing in progress.
///
/// One component rather than an added/removed marker: mutating an entity's
/// component set at runtime reshuffles hecs archetype order, which is exactly
/// what `Sim::npcs` has to defend against. Idle attackers just carry `None`.
#[derive(Clone, Debug, Default)]
pub struct Attacking {
    /// Which attack from `assets/data/attacks.ron`, if any is running.
    pub attack: Option<String>,
    /// Ticks since the swing started.
    pub elapsed: u32,
    /// Everything this swing has already connected with, so a hitbox that is
    /// live for six ticks still deals one hit per target.
    pub hit: Vec<hecs::Entity>,
    /// The next link of a combo, buffered by pressing attack during the
    /// current one's chain window. Buffered rather than immediate so a combo
    /// reads as one motion — you press on the swing you can see, and the next
    /// starts when this one's animation is done.
    pub chained: Option<String>,
}

impl Attacking {
    pub fn busy(&self) -> bool {
        self.attack.is_some()
    }

    pub fn start(&mut self, attack: &str) {
        self.attack = Some(attack.to_string());
        self.elapsed = 0;
        self.hit.clear();
        self.chained = None;
    }

    pub fn stop(&mut self) {
        self.attack = None;
        self.elapsed = 0;
        self.hit.clear();
        self.chained = None;
    }
}

/// Which side of a fight an entity is on. Hitboxes only damage the other team,
/// so the knight's sword cannot clip another knight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Team {
    Player,
    Enemy,
}

/// What the map called this entity — `"knight"`, later `"goblin"`.
///
/// Kept on the entity because traces and tape assertions address NPCs as
/// `<kind>.<index>`, so `knight.0` needs something to resolve against. An
/// index rather than a name because nothing needs naming yet; when a quest
/// NPC does, this is where a name would sit beside the kind.
#[derive(Clone, Debug)]
pub struct Kind(pub String);

/// What a hostile NPC is currently doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stance {
    /// Walking its route, unaware.
    Patrol,
    /// Closing on the player.
    Chase,
    /// Committed to a swing, standing still.
    Attack,
    /// Lost the player; walking back to where it started.
    Return,
}

/// The fight brain, layered on top of [`Patrol`].
///
/// Separate from `Patrol` so the two compose: a village blacksmith can pace
/// back and forth without also being willing to stab you. Everything hostile
/// about the knight lives here.
#[derive(Clone, Debug)]
pub struct Hostile {
    pub stance: Stance,
    /// Where it spawned. It walks back here after losing the player, so a
    /// chase does not permanently relocate every enemy on the map.
    pub home: f32,
    /// Ticks until it may swing again.
    pub cooldown: u32,
    /// Which attack it throws, from `assets/data/attacks.ron`.
    pub attack: String,
}

impl Hostile {
    /// How far ahead it notices the player. Sight is a box in front of it, not
    /// a radius: walking up behind a patrolling knight should work.
    pub const SIGHT: f32 = 130.0;
    /// Vertical tolerance on that box. Roughly a body height, so a player on
    /// the platform above is not "in front of" anything.
    pub const SIGHT_HEIGHT: f32 = 36.0;
    /// Gives up once the player is this far away — wider than `SIGHT`, so a
    /// knight at the edge of its vision does not flicker between states.
    pub const LOSE: f32 = 200.0;
    /// Close enough to swing.
    pub const REACH: f32 = 30.0;
    /// Ticks between swings. Long enough that closing in, landing a hit and
    /// backing out is a plan rather than a gamble — a first enemy that wins
    /// every exchange teaches the player nothing except to avoid it.
    pub const COOLDOWN: u32 = 75;
    /// How close to home counts as home.
    pub const HOME_SLACK: f32 = 8.0;
    /// Chasing is faster than strolling, but still much slower than the
    /// player runs: backing off has to work.
    pub const CHASE_MULTIPLIER: f32 = 1.25;

    pub fn new(home: f32, attack: &str) -> Self {
        Hostile {
            stance: Stance::Patrol,
            home,
            cooldown: 0,
            attack: attack.to_string(),
        }
    }
}

/// Walks back and forth, turning at walls and at the edges of what it is
/// standing on. On its own, no awareness of the player at all — [`Hostile`]
/// is what adds that.
///
/// Deliberately needs no authoring: a `K` dropped anywhere in any map is the
/// whole specification, and the geometry decides the route. Patrol bounds in
/// the map data would be one more thing to get wrong, and would stop working
/// the moment a level was edited underneath them.
#[derive(Clone, Copy, Debug)]
pub struct Patrol {
    pub speed: f32,
    /// Facing and travel direction: -1 left, +1 right.
    pub dir: f32,
}

impl Patrol {
    /// How far ahead of the collider to look for a wall, or for missing floor.
    /// A little under half a tile: far enough to stop before the edge, close
    /// enough that a one-tile ledge is still walkable.
    pub const LOOKAHEAD: f32 = 6.0;
    /// How far below the feet counts as "there is still floor here".
    pub const FLOOR_PROBE: f32 = 4.0;

    pub fn new(dir: f32, speed: f32) -> Self {
        Patrol { dir, speed }
    }
}

/// How an entity is drawn: which animations it owns, and any nudge to where
/// they sit relative to its collider.
///
/// The clip set lives here rather than on the `Sim` because entities do not
/// share one — the player is a single atlas of 50x37 cells, the knight is
/// seven files of four different frame widths.
#[derive(Clone, Debug)]
pub struct Sprite {
    pub clips: Arc<ClipSet>,
    /// Adjustment on top of the default placement, for art that is not
    /// centred in its own frame. Usually zero.
    pub offset: Vec2,
}

impl Sprite {
    /// Where to draw a `frame`-sized image for a body at `pos` with a
    /// `collider`-sized box.
    ///
    /// Sprites are centred horizontally on the collider and stand on its
    /// bottom edge. Computing this per frame rather than storing it is what
    /// lets one entity mix clips of different frame sizes without its feet
    /// sliding around.
    pub fn draw_origin(&self, pos: Vec2, collider: Vec2, frame: (f32, f32)) -> Vec2 {
        pos + Vec2::new((collider.x - frame.0) / 2.0, collider.y - frame.1) + self.offset
    }
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
