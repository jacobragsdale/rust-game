//! Components are plain data. Behavior lives in `crate::systems`.

use std::collections::BTreeMap;
use std::sync::Arc;

use ggez::glam::Vec2;

use crate::assets::{AvatarStats, Clip, ClipSet, LootDrop, Slot, StatBlock};
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

/// [`Stats`] plus whatever is equipped: `base + sum(modifiers)`, recomputed
/// from scratch once per tick by [`crate::systems::inventory::derive_stats`].
///
/// **This is what every system reads.** [`Stats`] is the base and is never
/// modified; this is the answer. The split is the whole of PLAN.md's rule that
/// stats are never mutated in place, and the reason is worth restating: if
/// equipping added 2 to a number, unequipping would have to know it was 2 —
/// and the first time those two facts disagree (an item edited between a save
/// and a load, an effect applied twice, a death mid-equip) the player's maximum
/// health is permanently wrong with nothing in the code to point at. Deriving
/// makes that unrepresentable: there is no subtraction anywhere.
///
/// Entities with no [`Equipment`] carry a `DerivedStats` too, sharing the same
/// `Arc` as their base — so a read site never has to ask whether this entity is
/// the kind that can wear things.
#[derive(Clone, Debug)]
pub struct DerivedStats(pub Arc<StatBlock>);

/// A count of one item id. One stack occupies one slot of an [`Inventory`],
/// however many are in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemStack {
    pub id: String,
    pub count: u32,
}

/// What an entity is carrying.
///
/// Capacity counts *stacks*, not items, which is what makes "the bag is full"
/// a state about kinds rather than about quantity: a thirteenth potion always
/// fits, a thirteenth kind of thing does not. It comes from the kind's
/// `inventory_slots` stat rather than a constant here, so a bigger bag is
/// content.
#[derive(Clone, Debug, Default)]
pub struct Inventory {
    pub slots: Vec<ItemStack>,
    pub capacity: usize,
}

impl Inventory {
    pub fn new(capacity: usize) -> Self {
        Inventory {
            slots: Vec::new(),
            capacity,
        }
    }

    /// How many of `id` are carried.
    pub fn count(&self, id: &str) -> u32 {
        self.slots
            .iter()
            .find(|stack| stack.id == id)
            .map_or(0, |stack| stack.count)
    }

    /// Every stack is one slot, so this is the number of distinct kinds held.
    pub fn used_slots(&self) -> usize {
        self.slots.len()
    }

    pub fn is_full(&self) -> bool {
        self.slots.len() >= self.capacity
    }

    /// Would `id` fit? True whenever a stack of it already exists, since that
    /// stack simply grows.
    pub fn has_room_for(&self, id: &str) -> bool {
        self.slots.iter().any(|stack| stack.id == id) || !self.is_full()
    }

    /// Add `count` of `id`, stacking onto an existing entry. Returns false and
    /// changes nothing if there is no room — the caller is expected to say so
    /// rather than swallow it.
    pub fn add(&mut self, id: &str, count: u32) -> bool {
        if count == 0 {
            return true;
        }
        if let Some(stack) = self.slots.iter_mut().find(|stack| stack.id == id) {
            // Saturating: an absurd count in content is a very large stack,
            // not a panic in a debug build.
            stack.count = stack.count.saturating_add(count);
            return true;
        }
        if self.is_full() {
            return false;
        }
        self.slots.push(ItemStack {
            id: id.to_string(),
            count,
        });
        true
    }

    /// Take `count` of `id`, removing the stack when it empties. Returns false
    /// and changes nothing if there are not that many.
    pub fn remove(&mut self, id: &str, count: u32) -> bool {
        let Some(index) = self.slots.iter().position(|stack| stack.id == id) else {
            return false;
        };
        if self.slots[index].count < count {
            return false;
        }
        self.slots[index].count -= count;
        if self.slots[index].count == 0 {
            self.slots.remove(index);
        }
        true
    }
}

/// What an entity is wearing, by slot.
///
/// A `BTreeMap` rather than a `HashMap`, and that is load-bearing: deriving
/// stats walks this and sums modifiers, so iteration order is observable in
/// every float it produces. `HashMap` order varies between runs, which would
/// make a golden trace depend on hash seeding — a class of bug that shows up
/// as "the trace differs on one machine" and takes a day to find.
#[derive(Clone, Debug, Default)]
pub struct Equipment {
    pub slots: BTreeMap<Slot, String>,
}

impl Equipment {
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn get(&self, slot: Slot) -> Option<&str> {
        self.slots.get(&slot).map(String::as_str)
    }

    /// Is `id` worn in any slot?
    pub fn holds(&self, id: &str) -> bool {
        self.slots.values().any(|worn| worn == id)
    }
}

/// Something the player can stand next to and press `Interact` on.
///
/// The component carries only *what is offered*: which word the prompt shows
/// and what the press does. Who is nearest, whether anything is in reach at
/// all, and what happens next are [`crate::systems::dialogue`]'s — a component
/// is data, and "the nearest of these" is a decision about the whole world.
///
/// Both fields are owned `String`s rather than `&'static str`, because both
/// come out of RON at load: hecs components must be `Send + Sync`, and this is.
#[derive(Clone, Debug)]
pub struct Interactable {
    /// The verb the HUD shows and a tape reads: `assert prompt == talk`.
    pub prompt: String,
    pub target: InteractTarget,
}

/// What interacting with something does.
///
/// An enum from the start, which is what let doors, chests and levers arrive as
/// variants rather than as a rewrite of every map that names a conversation.
/// What each one does with the press is [`crate::sim::Sim`]'s — the component
/// only says which kind of thing is there, and the thing's own component
/// ([`Door`], [`Chest`], [`Lever`]) holds the rest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractTarget {
    /// Open this dialogue graph, from `assets/data/dialogue/` — an NPC, or a
    /// sign.
    Dialogue(String),
    /// Go through to another map.
    Door,
    /// Take what is inside.
    Chest,
    /// Throw it.
    Lever,
}

impl InteractTarget {
    /// What an event and a tape call this target: the graph id for a
    /// conversation, so `expect elder_intro.dialogue_opened == 1` reads, and
    /// the kind of thing for everything else — `expect door.interacted`.
    pub fn label(&self) -> &str {
        match self {
            InteractTarget::Dialogue(graph) => graph,
            InteractTarget::Door => "door",
            InteractTarget::Chest => "chest",
            InteractTarget::Lever => "lever",
        }
    }
}

/// A way through to another map, on the far side of a press of `interact`.
///
/// Its box is two tiles wide and two tall standing on its cell — the part of a
/// doorway a player can stand in front of — whatever the art is.
#[derive(Clone, Debug)]
pub struct Door {
    /// The map it leads to, relative to `assets/`.
    pub to: String,
    /// Where on that map: a `Spawn` or a door id.
    pub at: String,
    /// The item that opens it, if it is locked.
    pub locked: Option<String>,
    /// Keep the key after unlocking, rather than leaving it in the lock.
    pub keep_key: bool,
    /// The world flag that remembers it has been unlocked.
    pub flag: String,
    /// The tileset prop it is drawn with.
    pub art: String,
}

/// A region you go through to another map by walking into — the edge of a
/// map. Invisible; what shows the way is the level art around it.
#[derive(Clone, Debug)]
pub struct Exit {
    pub to: String,
    pub at: String,
}

/// A chest, and what is in it. Opened is a world flag rather than a field, so
/// it stays opened across a load and across leaving and coming back.
#[derive(Clone, Debug)]
pub struct Chest {
    pub items: Vec<(String, u32)>,
    pub flag: String,
}

/// Touch it and it is where you come back to.
#[derive(Clone, Copy, Debug)]
pub struct Checkpoint;

/// Walk into it and a conversation opens — once, which its world flag
/// remembers, and not before `when` is set if it names a flag. Invisible; the
/// story it tells is the only sign it was there.
#[derive(Clone, Debug)]
pub struct Trigger {
    pub dialogue: String,
    pub flag: String,
    pub when: Option<String>,
}

/// Something to read. What it says is the dialogue graph on its
/// [`Interactable`]; this only marks what to draw.
#[derive(Clone, Copy, Debug)]
pub struct Sign;

/// A switch: throwing it sets `flag` to 1, for good. Thrown is the flag
/// itself, so two levers wired to one gate both show it.
#[derive(Clone, Debug)]
pub struct Lever {
    pub flag: String,
}

/// A barrier that is solid while `flag` is 0 — a portcullis a lever raises, a
/// door that opens when the guard is dead.
///
/// Like a fire, the [`Collider`] it carries while closed *is* its closedness:
/// there is no second field that could disagree with the geometry.
#[derive(Clone, Debug)]
pub struct Gate {
    pub flag: String,
    /// Its box while closed, relative to its `Position`.
    pub collider: Collider,
}

/// A flag set to 1 when this entity dies. See
/// [`crate::level::EntitySpawn::flag`].
#[derive(Clone, Debug)]
pub struct DeathFlag(pub String);

/// An item lying in the world, waiting to be walked over.
///
/// A plain entity with a [`Body`], which is what `move_bodies` was generalized
/// for: a drop falls, lands, and sits there using exactly the code the player
/// falls with. It has no [`Sprite`] on purpose — item art does not exist yet,
/// so the scene draws a coloured quad from this and the item's kind, and the
/// day `assets/graphics/items/` arrives the content that names those sprites
/// does not have to be rewritten.
#[derive(Clone, Debug)]
pub struct Pickup {
    pub item: String,
    pub count: u32,
    /// Whether a full bag has already been reported for this one. Without it,
    /// standing on an item you have no room for emits an event every tick for
    /// as long as you stand there, and a trace becomes unreadable.
    pub refused: bool,
    /// The world flag that remembers it was taken, for an item a map placed:
    /// set when it is picked up, and read when the map is built so it is not
    /// placed again. `None` for loot, which is gone with the fight it came from.
    pub flag: Option<String>,
}

/// What an entity leaves behind when it dies.
///
/// The table itself is authored on the kind's stat block, not here and not in
/// code; this is the per-entity copy plus the one bit that has to be state —
/// whether the roll has happened. A corpse is never despawned, so without
/// `dropped` it would drop again on every tick it spends dead.
#[derive(Clone, Debug)]
pub struct Loot {
    pub drops: Vec<LootDrop>,
    pub dropped: bool,
}

impl Loot {
    pub fn new(drops: Vec<LootDrop>) -> Self {
        Loot {
            drops,
            dropped: false,
        }
    }
}

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

/// Which phase of a plunge an avatar is in.
///
/// The plunge is the one attack whose length is not in `attacks.ron`: how long
/// the fall lasts depends on how far up it started, and the recovery starts
/// when the ground arrives. So the phase is state on the avatar rather than an
/// elapsed count against a fixed timeline, and it is what
/// [`crate::systems::animation::select_avatar_clip`] reads to pick between the
/// three clips.
///
/// Serde because a save carries it: which phase of a plunge you were in is
/// state like any other. See [`crate::save`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Plunge {
    #[default]
    None,
    /// Hanging still, blade coming up. The hitbox is not live yet.
    Ready,
    /// Dropping fast with a live hitbox underneath.
    Falling,
    /// Landed and rooted while the impact plays out.
    Impact,
}

/// The player. Tile-scale physics tuned for 32px tiles and the 50x37
/// Adventurer sprite. The collider is smaller than the sprite;
/// `Sprite::offset` aligns them.
///
/// Only the state that is specific to *being the player* lives here. Where the
/// body is, how fast it is going, and whether it is on the ground belong to
/// [`Body`], which anything else in the world can have too. How fast it runs
/// and how high it jumps belong to [`Stats`], which is data.
///
/// There are no numbers in this file at all. Every one of them lives in
/// `assets/data/stats.ron` (tickets H-3 and H-3b) — including the player's
/// collider box, which the level loaders need before any entity exists and now
/// read from that table via [`crate::level::LevelData::load`] rather than from
/// a `const` mirror of it kept true by hand.
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
    /// Which phase of a plunge is running, if any.
    pub plunge: Plunge,
    /// Ticks left in the current plunge phase (the hover, then the impact).
    /// The fall in between is not timed — it ends when the ground does.
    pub plunge_ticks: u32,
    /// Death freeze countdown; respawns when it reaches zero.
    pub dead_ticks: u32,
}

impl Avatar {
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
            plunge: Plunge::None,
            plunge_ticks: 0,
            dead_ticks: 0,
        }
    }

    pub fn sliding(&self) -> bool {
        self.slide_ticks > 0
    }

    /// Committed to a plunge, in any of its three phases: no steering, no
    /// jumping, no other attack, no cast.
    pub fn plunging(&self) -> bool {
        self.plunge != Plunge::None
    }

    /// Give up on the plunge wherever it is. Called on being hit and on
    /// dying, the same way a swing is dropped.
    pub fn cancel_plunge(&mut self) {
        self.plunge = Plunge::None;
        self.plunge_ticks = 0;
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

/// A pool that spells are paid out of, and refills itself.
///
/// Nothing about this is player-specific — an enemy caster wants exactly the
/// same component — but only the player has one today, because
/// [`crate::ecs::spawn`] attaches it to a kind whose `max_mana` is non-zero
/// and the knight's is zero.
///
/// Regeneration is integer arithmetic on purpose. `regen` is thousandths of a
/// point per tick and `partial` is the running remainder, so an empty pool is
/// full again on an exact, stated tick — `1000 * max / regen` of them — and a
/// tape can assert `mana == 5` at a tick it worked out rather than one it
/// discovered. A float accumulator gives the same answer to within a rounding
/// error, and a rounding error is precisely what an `==` assertion is not
/// allowed to depend on.
#[derive(Clone, Copy, Debug)]
pub struct Mana {
    pub current: i32,
    pub max: i32,
    /// Thousandths of a point regenerated per tick.
    pub regen: u32,
    /// Thousandths accumulated toward the next whole point.
    pub partial: u32,
}

impl Mana {
    pub fn new(max: i32, regen: u32) -> Self {
        Mana {
            current: max,
            max,
            regen,
            partial: 0,
        }
    }

    /// Can `cost` be paid right now?
    pub fn affords(&self, cost: i32) -> bool {
        self.current >= cost
    }

    /// Pay `cost`, which the caller has checked with [`Mana::affords`].
    pub fn spend(&mut self, cost: i32) {
        self.current = (self.current - cost).max(0);
        // Spending resets the fraction, so two casts in a row do not bank a
        // free point out of whatever happened to be accumulated.
        self.partial = 0;
    }

    /// One tick of regeneration. Never exceeds `max`, and a full pool stops
    /// accumulating rather than banking a head start on the next spend.
    pub fn regenerate(&mut self) {
        if self.current >= self.max || self.regen == 0 {
            self.partial = 0;
            return;
        }
        self.partial += self.regen;
        while self.partial >= 1000 && self.current < self.max {
            self.partial -= 1000;
            self.current += 1;
        }
        if self.current >= self.max {
            self.current = self.max;
            self.partial = 0;
        }
    }

    pub fn fraction(&self) -> f32 {
        if self.max <= 0 {
            0.0
        } else {
            (self.current.max(0) as f32) / (self.max as f32)
        }
    }
}

/// A spell in progress, and the cooldown left behind by the last one.
///
/// One component rather than an added/removed marker, for the reason
/// [`Attacking`] is: mutating an entity's component set at runtime reshuffles
/// hecs archetype order, and `Sim::npcs` should not have to defend against
/// something this file could simply not do. Idle casters carry `None`.
#[derive(Clone, Debug, Default)]
pub struct Casting {
    /// Which spell from `assets/data/spells.ron`, if one is running.
    pub spell: Option<String>,
    /// Ticks since the cast started.
    pub elapsed: u32,
    /// Ticks until another cast may start.
    ///
    /// One timer for the caster rather than one per spell, which is exactly
    /// right for one spell and exactly wrong for two. The second entry in
    /// `spells.ron` is what turns this into a map keyed by spell id.
    pub cooldown: u32,
}

impl Casting {
    pub fn busy(&self) -> bool {
        self.spell.is_some()
    }

    pub fn start(&mut self, spell: &str, cooldown: u32) {
        self.spell = Some(spell.to_string());
        self.elapsed = 0;
        self.cooldown = cooldown;
    }

    /// End the cast. The cooldown is deliberately left running: it started
    /// when the cast did, so interrupting one is not a way to skip it.
    pub fn stop(&mut self) {
        self.spell = None;
        self.elapsed = 0;
    }
}

/// A thing in flight that damages what it touches.
///
/// Everything about a hit is carried here rather than looked up from the spell
/// that made it, so a bolt already in the air is unaffected by the table being
/// reloaded, and so the projectile system needs no access to `spells.ron`.
#[derive(Clone, Debug)]
pub struct Projectile {
    pub damage: i32,
    /// Impulse applied to what it hits, in its direction of travel.
    pub knockback: Vec2,
    pub hitstun: u32,
    /// Carry on after connecting rather than expiring.
    pub pierces: bool,
    /// Who threw it. Kept so a projectile can never hit its own caster even
    /// if teams are ever allowed to overlap, and so an event could name them.
    pub source: hecs::Entity,
    /// Everything it has already hit, so a bolt that pierces lands once per
    /// target — the same rule `Attacking::hit` keeps for a swing.
    pub hit: Vec<hecs::Entity>,
    /// The velocity it was launched at. Nothing in the tick steers a bolt, so
    /// a velocity that is no longer this one is a bolt the level stopped.
    pub launched: Vec2,
}

/// Ticks an entity has left before it removes itself.
///
/// Only projectiles have one today. It is a plain countdown rather than an
/// expiry tick so that hitstop — which skips whole ticks of the world —
/// pauses it along with everything else.
#[derive(Clone, Copy, Debug)]
pub struct Lifetime {
    pub ticks: u32,
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
    /// Where it spawned. It goes back here after losing the player, so a
    /// chase does not permanently relocate every enemy on the map. A walker
    /// only ever uses the `x`; a flyer returns to the height too.
    pub home: Vec2,
    /// Ticks until it may swing, or bite, again.
    pub cooldown: u32,
    /// Which attack it throws, from `assets/data/attacks.ron` — `None` for a
    /// kind with no swing, which fights with a spell or by touch.
    pub attack: Option<String>,
}

impl Hostile {
    pub fn new(home: Vec2, attack: Option<String>) -> Self {
        Hostile {
            stance: Stance::Patrol,
            home,
            cooldown: 0,
            attack,
        }
    }
}

/// A hit dealt by touching: see [`crate::assets::ContactDef`].
#[derive(Clone, Debug)]
pub struct Contact(pub crate::assets::ContactDef);

/// A brain at an [`Avatar`]'s controls in place of the player: a rival
/// champion, fighting with the player's own kit. The deciding is
/// [`crate::systems::brain`]'s; the numbers it decides with are the kind's
/// `brain` group in `assets/data/stats.ron`.
///
/// **An avatar without one is the player's** — the one a tape drives, the
/// camera follows, a checkpoint heals and every "the player" in the code
/// means. That is a rule about absence on purpose: every avatar that existed
/// before rivals did is still the player's without a change to how it was
/// built.
#[derive(Clone, Copy, Debug, Default)]
pub struct Brain {
    /// What it decided to press this tick, written before the controllers run
    /// and read by [`crate::systems::avatar`] exactly where it would read the
    /// keyboard.
    pub input: crate::systems::input::PlayerInput,
    /// Has it seen anyone worth fighting yet. Until then it stands where it
    /// was put, which is what makes a duel start when you walk into it.
    pub awake: bool,
    /// Ticks until it next makes up its mind. Between decisions it keeps
    /// holding what it chose — which is what a reaction time *is*.
    pub wait: u32,
    /// What it is holding, and what it held the tick before: a press is the
    /// difference, as it is for a tape.
    pub held: crate::systems::input::ActionSet,
    pub prev: crate::systems::input::ActionSet,
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
    /// A nudge for this one entity, on top of the offsets its art declares —
    /// a projectile centres its art on its box with this. Not mirrored.
    pub offset: Vec2,
}

impl Sprite {
    /// Where to draw a frame of `clip` for a body at `pos` with a
    /// `collider`-sized box, facing the given way.
    ///
    /// Sprites are centred horizontally on the collider and stand on its
    /// bottom edge, then nudged by the art's own offsets — the set's and the
    /// clip's, mirrored when facing left, since an offset that moves art
    /// forward has to move it forward whichever way forward is. Computing this
    /// per frame rather than storing it is what lets one entity mix clips of
    /// different frame sizes without its feet sliding around.
    pub fn draw_origin(&self, pos: Vec2, collider: Vec2, clip: &Clip, facing_right: bool) -> Vec2 {
        let (fw, fh) = self.clips.frame_size_of(clip);
        let (ox, oy) = self.clips.offset_of(clip);
        let ox = if facing_right { ox } else { -ox };
        pos + Vec2::new((collider.x - fw) / 2.0 + ox, collider.y - fh + oy) + self.offset
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
            self.restart(clip);
        }
    }

    /// Play `clip` from its first frame, even if it is the one already
    /// playing.
    pub fn restart(&mut self, clip: &str) {
        self.clip = clip.to_string();
        self.frame = 0;
        self.elapsed = 0.0;
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

/// Geometry that shuttles between two points: a moving platform.
///
/// Like [`Schedule`], and for the same reason, this is a closed-form function
/// of `Sim::tick` rather than something that integrates. A platform that added
/// `speed * dt` to its position every tick would accumulate float error, would
/// slide permanently out of step with a second platform after the ticks
/// hitstop skips, and could not answer "where were you at tick 900?" without
/// being stepped there. [`Mover::at`] answers that from the tick alone, gives
/// the same answer forever, and is exactly periodic — `at(t) == at(t + period)`
/// for every `t`, with no accumulated error, because nothing accumulates.
///
/// The path is a there-and-back between `from` and `to` at constant speed: a
/// triangle wave, linear in both directions, with the turn taking no time. The
/// leg is a whole number of *ticks* rather than a speed in px/s so that the
/// period is an integer and the exact-periodicity above is a fact rather than
/// an approximation; [`crate::systems::mover::leg_ticks`] is where an authored
/// speed becomes one.
#[derive(Clone, Copy, Debug)]
pub struct Mover {
    /// Top-left of the collider at the start of the path, in world pixels.
    pub from: Vec2,
    /// Top-left of the collider at the far end.
    pub to: Vec2,
    /// Ticks to travel `from` -> `to`. One full there-and-back is twice this.
    pub leg_ticks: u32,
    /// Ticks to shift the cycle by, so two platforms can be out of step.
    pub phase: u32,
}

impl Mover {
    pub fn new(from: Vec2, to: Vec2, leg_ticks: u32, phase: u32) -> Self {
        Mover {
            from,
            to,
            leg_ticks,
            phase,
        }
    }

    /// One full there-and-back, in ticks.
    pub fn period(&self) -> u32 {
        self.leg_ticks.saturating_mul(2)
    }

    /// Where this platform is at `tick`.
    ///
    /// A zero-length leg is a platform that is simply parked — a degenerate
    /// map rather than a crash, the same call [`Schedule::on_at`] makes for a
    /// zero period.
    pub fn at(&self, tick: u64) -> Vec2 {
        if self.leg_ticks == 0 {
            return self.from;
        }
        let leg = self.leg_ticks as u64;
        let step = (tick + self.phase as u64) % (leg * 2);
        // Reflect the second half of the cycle back onto the first: the
        // outbound and return legs are the same line walked in reverse, so a
        // rider's displacement per tick has the same magnitude either way.
        let along = if step <= leg { step } else { leg * 2 - step };
        self.from + (self.to - self.from) * (along as f32 / leg as f32)
    }
}

/// A hazard on a chain: the spiked ball, swinging under a fixed anchor.
///
/// The third and last of the closed-form movers, after [`Schedule`] and
/// [`Mover`], and closed form for the same reasons plus one of its own. A
/// numerically integrated pendulum does not merely accumulate float error — it
/// accumulates *energy*, because a symplectic-free Euler step adds a little to
/// the swing every cycle. The amplitude authored in the map would then not be
/// the amplitude the ball actually reaches, and it would not be the same
/// amplitude on tick 100 and tick 100_000. [`Pendulum::at`] answers "where is
/// the ball at tick *t*?" from the tick alone, gives the same answer forever,
/// and is exactly periodic: `at(t) == at(t + period)` bit for bit, because the
/// tick reaches the trigonometry only through an integer remainder.
///
/// The motion is simple harmonic in the *angle* — `theta(t) = amplitude *
/// cos(2*pi * (t + phase) / period)` — which is the linearised pendulum, and
/// which is also what a swinging hazard should look like whatever the physics
/// says: it dwells at the extremes and is fastest at the bottom. It starts at
/// one extreme, momentarily still, so a map with `phase: 0` begins in a state
/// an author can picture.
///
/// The ball's box is a [`Collider`] of kind [`ColliderKind::Hazard`]: it kills
/// through the same path spikes and fire do, and it obstructs nothing. A
/// wrecking ball you could stand on is a moving platform, and there is already
/// one of those.
#[derive(Clone, Copy, Debug)]
pub struct Pendulum {
    /// Where the chain is fixed, in world pixels. Nothing moves it.
    pub anchor: Vec2,
    /// Chain length: anchor to the centre of the ball, in pixels.
    pub length: f32,
    /// Half-swing in **radians**, measured from straight down. The ball
    /// travels between `-amplitude` and `+amplitude`; maps author it in
    /// degrees and [`crate::level::ascii`] converts.
    pub amplitude: f32,
    /// Ticks for one full there-and-back. Whole ticks for the reason
    /// [`crate::systems::mover::leg_ticks`] rounds to them: an integer period
    /// is what makes exact periodicity a fact rather than an approximation.
    pub period: u32,
    /// Ticks to shift the cycle by, so two balls can swing out of step.
    pub phase: u32,
}

impl Pendulum {
    pub fn new(anchor: Vec2, length: f32, amplitude: f32, period: u32, phase: u32) -> Self {
        Pendulum {
            anchor,
            length,
            amplitude,
            period,
            phase,
        }
    }

    /// The angle from straight down, in radians, at `tick`.
    ///
    /// The tick reaches the cosine only as `step`, an integer in
    /// `0..period` — which is the whole of why this is exactly periodic.
    /// Dividing `(tick + phase)` by the period in floating point instead would
    /// give an argument that drifts by an ulp or two over a long run, and
    /// `at(t) == at(t + period)` would stop being true somewhere out past a
    /// few million ticks with nothing to say it had.
    ///
    /// A zero `period` is a ball hanging still at its tick-0 position — a
    /// degenerate map rather than a crash, the same call [`Schedule::on_at`]
    /// and [`Mover::at`] make.
    pub fn angle_at(&self, tick: u64) -> f32 {
        if self.period == 0 {
            return self.amplitude;
        }
        let step = (tick + self.phase as u64) % (self.period as u64);
        let turn = std::f32::consts::TAU * step as f32 / self.period as f32;
        self.amplitude * turn.cos()
    }

    /// Where the ball's centre is for a chain at `angle` radians from straight
    /// down. Positive angles swing right; `+y` is down, so the resting ball
    /// hangs at `anchor + (0, length)`.
    ///
    /// Split out from [`Pendulum::at`] so the arc can be walked by angle
    /// rather than by tick — which is what `tests/levels.rs` does to check
    /// that a map's swing is not buried in a wall, independently of how long
    /// the map said the swing takes.
    pub fn at_angle(&self, angle: f32) -> Vec2 {
        self.anchor + self.length * Vec2::new(angle.sin(), angle.cos())
    }

    /// Where the ball's centre is at `tick`.
    pub fn at(&self, tick: u64) -> Vec2 {
        self.at_angle(self.angle_at(tick))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule this file is now under: components are shape, not balance.
    ///
    /// `avatar_consts_match_the_shipped_stat_table` used to live here, guarding
    /// five `const`s against the RON drifting away from them. H-3b deleted the
    /// consts, which deletes the class of bug rather than watching for it —
    /// there is nothing left here for the table to disagree with.
    #[test]
    fn a_fresh_avatar_takes_its_air_jumps_from_the_stat_table() {
        let player = crate::assets::StatTable::shipped().get("player").unwrap();
        let avatar = Avatar::new(player.avatar());
        assert_eq!(avatar.air_jumps, player.avatar().max_air_jumps);
    }
}
