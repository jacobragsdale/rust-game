//! Components are plain data. Behavior lives in `crate::systems`.

use std::sync::Arc;

use ggez::glam::Vec2;

use crate::assets::{AvatarStats, ClipSet, StatBlock};
use crate::physics::{Aabb, SolidRect};

/// What this entity's kind is worth, numerically: the block
/// `assets/data/stats.ron` holds for it.
///
/// Every entity carries one, and every movement and combat number the
/// simulation reads comes through it. Shared behind an `Arc` — one block per
/// kind, however many of that kind are alive — which is also what makes it
/// `Send + Sync`, as a hecs component must be.
#[derive(Clone, Debug)]
pub struct Stats(pub Arc<StatBlock>);

#[derive(Clone, Copy, Debug)]
pub struct Position(pub Vec2);

#[derive(Clone, Copy, Debug)]
pub struct Velocity(pub Vec2);

/// AABB extent of an entity, anchored at its `Position` (top-left corner).
#[derive(Clone, Copy, Debug)]
pub struct Size(pub Vec2);

/// What an entity's collision box is *to everything else*.
///
/// [`Size`] is what the world does to an entity — the box gravity and
/// resolution push around. This is the other direction: an entity with a
/// [`Position`] and one of these becomes part of the level's geometry, and
/// bodies collide with it exactly as they collide with a tile.
///
/// The distinction matters because the two are rarely the same box. A moving
/// platform is all collider and no body; a knight is all body and no collider,
/// since NPCs walk through each other rather than shoving.
#[derive(Clone, Copy, Debug)]
pub struct Collider {
    /// Top-left of the box relative to the owner's `Position`.
    pub rect_offset: Vec2,
    pub size: Vec2,
    pub kind: ColliderKind,
}

/// What kind of geometry a [`Collider`] contributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColliderKind {
    /// Blocks from every side, like a wall.
    Solid,
    /// Blocks only from above, like a platform.
    OneWay,
    /// Blocks nothing and kills on contact, like spikes.
    Hazard,
}

impl Collider {
    pub fn solid(size: Vec2) -> Self {
        Collider {
            rect_offset: Vec2::ZERO,
            size,
            kind: ColliderKind::Solid,
        }
    }

    pub fn one_way(size: Vec2) -> Self {
        Collider {
            rect_offset: Vec2::ZERO,
            size,
            kind: ColliderKind::OneWay,
        }
    }

    pub fn hazard(size: Vec2) -> Self {
        Collider {
            rect_offset: Vec2::ZERO,
            size,
            kind: ColliderKind::Hazard,
        }
    }

    /// Where the box is, for an owner at `pos`.
    pub fn aabb(&self, pos: Vec2) -> Aabb {
        let origin = pos + self.rect_offset;
        Aabb::new(origin.x, origin.y, self.size.x, self.size.y)
    }

    /// The box as something a body can be stopped by, or `None` for a kind
    /// that does not stop anything.
    ///
    /// Hazards are geometry but not obstruction: you walk into fire, you do
    /// not bump into it. They contribute nothing here and everything to
    /// [`Collider::hazard_rect`], which is the other half of the same split.
    pub fn solid_rect(&self, pos: Vec2) -> Option<SolidRect> {
        match self.kind {
            ColliderKind::Solid => Some(SolidRect::solid(self.aabb(pos))),
            ColliderKind::OneWay => Some(SolidRect::one_way(self.aabb(pos))),
            ColliderKind::Hazard => None,
        }
    }

    /// The box as something that kills on contact, or `None` for a kind that
    /// is merely in the way.
    pub fn hazard_rect(&self, pos: Vec2) -> Option<Aabb> {
        match self.kind {
            ColliderKind::Hazard => Some(self.aabb(pos)),
            ColliderKind::Solid | ColliderKind::OneWay => None,
        }
    }
}

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
/// [`Body`], which anything else in the world can have too. How fast it runs
/// and how high it jumps belong to [`Stats`], which is data.
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
    // --- the last two numbers still in code, and why -----------------------
    //
    // Every gameplay constant moved to `assets/data/stats.ron` (ticket H-3),
    // read through [`Stats`]. These two could not: the level loaders
    // (`level::ascii`, `level::tiled`) resolve a map's player spawn from the
    // player's box *before any entity exists*, so there is no `Stats` to ask,
    // and `tests/physics_diagnostics.rs` needs the box in a `const` context.
    // Threading a stat table into `LevelData::load` is what deletes these.
    //
    // Until then they are not a second source of truth: `stats_match_the_
    // shipped_table` below fails the build if they ever disagree with the RON.
    pub const WIDTH: f32 = 20.0;
    pub const HEIGHT: f32 = 34.0;
    /// Only `tests/physics_diagnostics.rs` reads these three — it sweeps the
    /// collision system at realistic fall and jump speeds.
    pub const GRAVITY: f32 = 1400.0;
    pub const MAX_FALL: f32 = 900.0;
    pub const JUMP_SPEED: f32 = 520.0;

    /// A fresh avatar with a full set of air jumps.
    pub fn new(stats: &AvatarStats) -> Self {
        Avatar {
            facing_right: true,
            coyote_ticks: u32::MAX,
            jump_buffer: 0,
            air_jumps: stats.max_air_jumps,
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

    pub fn dead(&self) -> bool {
        self.dead_ticks > 0
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
    /// How long invulnerability lasts after a hit, for this entity. Copied
    /// from the kind's `iframe_ticks` at spawn; see [`Stats`].
    pub iframe_ticks: u32,
}

impl Health {
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

/// A duty cycle measured in ticks: `duty` ticks on out of every `period`,
/// shifted by `phase`.
///
/// Evaluated as a closed-form function of `Sim::tick` rather than counted
/// down, for the reason L-4's pendulum will be: a counter is state, and state
/// drifts. A tick the world skips — hitstop skips several — would slide a
/// counting fire permanently out of step with a second one, while
/// [`Schedule::on_at`] gives the same answer for tick *t* forever, and gives
/// it without having been stepped at all.
///
/// Two schedules with the same `period` and different `phase` are the
/// authored way to make hazards alternate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Schedule {
    /// Length of one full cycle, in ticks.
    pub period: u32,
    /// How much of that cycle is spent on.
    pub duty: u32,
    /// Ticks to shift the cycle by, so two of these can take turns.
    pub phase: u32,
}

impl Schedule {
    pub fn new(period: u32, duty: u32, phase: u32) -> Self {
        Schedule {
            period,
            duty,
            phase,
        }
    }

    /// Is this on at `tick`?
    ///
    /// A zero `period` is a schedule that never turns over: on forever if it
    /// has any duty at all, off forever otherwise. That is a degenerate map
    /// rather than a crash — a fire authored `period: 0` is a permanent one.
    pub fn on_at(&self, tick: u64) -> bool {
        if self.period == 0 {
            return self.duty > 0;
        }
        (tick + self.phase as u64) % (self.period as u64) < self.duty as u64
    }
}

/// A hazard that lights and goes out on a [`Schedule`].
///
/// While it is lit the entity carries the [`Collider`] below and
/// `body::rebuild_geometry` picks it up like any other entity-owned geometry;
/// while it is out the entity has no collider at all. The collider's
/// *presence* is the lit state — there is no second flag that could disagree
/// with the geometry, which is the failure mode a `lit: bool` invites.
#[derive(Clone, Copy, Debug)]
pub struct Fire {
    /// The box it presents while lit, relative to its `Position`.
    pub collider: Collider,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::StatTable;

    /// The handful of player numbers still spelled as `const` (see the note on
    /// [`Avatar`]) are a convenience for code that runs before any entity
    /// exists, not a second opinion. If the RON is tuned and these are not,
    /// the level loader would place the player at a spawn point computed from
    /// the wrong box — so this fails the build instead.
    #[test]
    fn avatar_consts_match_the_shipped_stat_table() {
        let player = StatTable::shipped().get("player").unwrap();
        assert_eq!(Avatar::WIDTH, player.width, "width");
        assert_eq!(Avatar::HEIGHT, player.height, "height");
        assert_eq!(Avatar::GRAVITY, player.gravity, "gravity");
        assert_eq!(Avatar::MAX_FALL, player.max_fall, "max_fall");
        assert_eq!(Avatar::JUMP_SPEED, player.avatar().jump_speed, "jump_speed");
    }
}
