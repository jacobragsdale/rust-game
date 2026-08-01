//! Saving a run, and loading one back.
//!
//! # A deliberate deviation from PLAN.md
//!
//! PLAN.md specifies rusqlite. This is a serde [`SaveState`] behind the
//! [`SaveStore`] trait, with a RON file backend. The requirement that actually
//! matters — *whatever it stores has to be reconstructible into a [`Sim`]* — is
//! unchanged, and a file backend keeps save tests headless, fast and diffable
//! in a commit, with no C build dependency for anyone who checks the repo out.
//! A rusqlite backend is an implementation of [`SaveStore`] and nothing above
//! this line changes when one arrives; [`MemoryStore`] exists partly to keep
//! that seam honest, by proving the rest of the module never touches a file.
//!
//! # What is saved
//!
//! A save is **the player entity in full**, plus the things that say which run
//! it belongs to and where in it you are: the map, the clock, the generator,
//! and the quest flags.
//!
//! - **the map**, so a load knows what to rebuild;
//! - **the quest flags**, because a quest that does not survive a save is not
//!   a quest. They are also the sanctioned way to make the *world* remember
//!   something across a load — see "what is deliberately not saved" below: a
//!   boss that stays dead is `quest.boss.stage`, not a saved NPC;
//! - **the clock**, which is more load-bearing than it looks, and is three
//!   numbers rather than one:
//!   - [`SaveState::tick`], where the run is;
//!   - [`SaveState::hitstop`], how many ticks of impact freeze are still to
//!     run. See below — this is the one entry on this list that used to be on
//!     the other one;
//!   - [`SaveState::decided_tick`], where the *world* is. Fires, moving
//!     platforms and swinging hazards are pure functions of a tick, and this is
//!     the tick they are functions of. It is usually `tick - 1` and it is not
//!     always, because a freeze moves the clock without moving the world —
//!     [`Sim::decided_tick`] argues why nothing recovers it from the other two.
//!
//!   All three are put back by [`Sim::resume_at`], which is a call and not an
//!   assignment for a reason: `Sim::load` builds the whole map at tick 0, so a
//!   load that merely set `Sim::tick` left every fire, platform and hazard
//!   describing a run that was not happening.
//!   [`crate::systems::mover::place`] has what that costs;
//! - **the RNG seed *and* its stream position**. Loading a save and getting
//!   different loot than you would have is a bug report that takes a day to
//!   find, so [`Rng::position`] is saved beside the seed and
//!   [`Rng::resume`] puts the generator back exactly where it stood.
//! - **the player**: position, velocity, health, mana, bag, equipment, and the
//!   pile of small timers that make up how it is *moving* — coyote ticks, the
//!   jump buffer, air jumps, the slide, the plunge, the swing, the cast and the
//!   animation frame.
//!
//! # What is deliberately not saved, and why
//!
//! **The rest of the world. You reload at the map's spawn state.** Every NPC is
//! back where the map puts it, at full health, alive; every bolt in the air is
//! gone; every item lying on the floor is gone with it. This is a decision, not
//! an oversight, and the reason is that the map is *already a description of
//! that state* — the save would be duplicating it, and a duplicate that can
//! disagree with the map is how a save file starts spawning knights inside
//! walls three level edits later. It is also the genre's convention: you reload,
//! the corridor is repopulated. The cost is real and worth stating — an item you
//! killed something for and had not picked up yet is gone, and a knight you had
//! nearly killed is whole.
//!
//! **Hitstop used to be on this list, and was wrong to be.** The argument was
//! that the four-tick freeze after a hit exists to make an impact read as
//! impact, and there is no impact to sell on the tick you load. That is true of
//! the *feel* and irrelevant to the *clock*. Dropping the freeze does not cost
//! four ticks of animation and stop there: the reloaded run starts moving four
//! ticks before the run it was saved from would have, and thereafter every tick
//! of it is a tick the other one will not reach for another four — forever, on
//! a fixed timestep, drifting further out of step with every hazard cycle it
//! now meets four ticks early. It is one `u32` and it is a fact about where the
//! run *is*, so it is saved. `tests/save.rs`'s
//! `a_save_taken_mid_impact_reloads_still_frozen` is what holds it down.
//!
//! Also not saved, each for its own reason:
//!
//! - **The mode and the inventory screen's cursor.** A load always lands in
//!   [`crate::sim::Mode::Playing`], with the world running.
//! - **What was held last tick.** Nothing is held at the moment a save is
//!   loaded, so edge-triggered input starts clean.
//! - **`Health::max`, `Mana::max` and `DerivedStats`.** These are caches of
//!   `base + sum(modifiers)`, not state — see
//!   [`crate::ecs::components::DerivedStats`]. Saving a derived number is
//!   exactly how an item edited between a save and a load leaves a player's
//!   maximum health permanently wrong with nothing in the code to point at, so
//!   the load recomputes them from the equipment it just restored.
//! - **Which entities a swing has already hit.** That list can only ever name
//!   entities that do not survive the load, so dropping it loses nothing.
//!
//! The ticket lists animation frames among the reasonable omissions, and they
//! are saved here anyway. The reason is the test: a trace records `clip` and
//! `frame` every tick, so a save that dropped them could only be checked
//! against a trace with those columns blanked out — and the columns you agree
//! not to look at are exactly where a forgotten field hides. Saving them makes
//! `a_reloaded_run_steps_identically` an *exact* comparison, which is the only
//! version of that test worth having. NPC animation is not saved, because no
//! part of an NPC is.
//!
//! # Adding to the schema later
//!
//! Quest flags were the worked example of this and are now [`SaveState::flags`]:
//! one field, `#[serde(default)]`, and **no version bump**. Every save written
//! before Q-1 still loads, with no flags set — which is exactly what a run from
//! before quests existed had. [`SAVE_VERSION`] is for changes that alter what
//! an existing field *means*, which no additive field does.
//!
//! [`SaveState::hitstop`] and [`SaveState::decided_tick`] are the second and
//! third, added the same way. Both default, and what they default *to* is the
//! interesting part: a missing `hitstop` is no freeze left to run, and a missing
//! `decided_tick` is `tick - 1`, which is what every tick of every run written
//! by a build without these fields actually had — that build could not carry a
//! freeze across a load at all, so it never produced a save in which the world
//! and the clock disagreed. Three additive fields, still version 1, and
//! `a_save_written_before_flags_existed_still_loads` covers all of them.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ggez::glam::Vec2;
use hecs::World;
use serde::{Deserialize, Serialize};

use crate::assets::{Assets, Slot};
use crate::ecs::components::{
    AnimationState, Attacking, Avatar, Body, Casting, Equipment, Health, Inventory, ItemStack,
    Mana, Plunge, Position, Velocity,
};
use crate::sim::rng::Rng;
use crate::sim::Sim;
use crate::systems::inventory;

/// The format version written into every save and checked on every load.
///
/// Bump this when a change alters what an existing field means. Adding a field
/// does not need it — see the module docs.
pub const SAVE_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Everything that can go wrong reaching a save.
///
/// A typed enum rather than a bare string because the three cases the ticket
/// names — missing, corrupt, wrong version — have to be *distinguishable* by
/// the caller. A load screen offers to start a new game for one of them and
/// says something quite different for the other two.
#[derive(Debug)]
pub enum SaveError {
    /// Nothing is stored under that slot.
    Missing { slot: String },
    /// Something is stored there, but it is not a save.
    Corrupt { slot: String, detail: String },
    /// A save from a build that wrote a different format.
    Version { found: u32, expected: u32 },
    /// A slot name that could not be a file name — including anything that
    /// could climb out of the save directory.
    BadSlot { slot: String },
    /// The store itself could not be reached. `path` is whatever the backend
    /// was talking to when it failed, so the message names something a person
    /// can go and look at.
    Io { path: String, detail: String },
    /// A save that could not be turned into text. Nothing in the schema can
    /// actually do this today; it is here so that adding something which can
    /// is a compile error at every call site rather than an `unwrap`.
    Encode { detail: String },
    /// A sim with no map to name: [`Sim::fixture`] builds one from an inline
    /// grid, and there is no file for a load to rebuild it from.
    NoMap,
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Missing { slot } => write!(f, "there is no save in slot `{slot}`"),
            SaveError::Corrupt { slot, detail } => {
                write!(f, "the save in slot `{slot}` is not readable: {detail}")
            }
            SaveError::Version { found, expected } => write!(
                f,
                "this save is format version {found}, but this build of the game \
                 reads version {expected} — it was written by a different version \
                 of the game and cannot be loaded",
            ),
            SaveError::BadSlot { slot } => write!(
                f,
                "`{slot}` is not a usable save slot name (letters, digits, `-` and `_`)",
            ),
            SaveError::Io { path, detail } => {
                write!(f, "could not read or write {path}: {detail}")
            }
            SaveError::Encode { detail } => {
                write!(f, "this save could not be written out: {detail}")
            }
            SaveError::NoMap => write!(
                f,
                "this run has no map file to reload (it was built from an inline grid), \
                 so it cannot be saved",
            ),
        }
    }
}

impl std::error::Error for SaveError {}

// ---------------------------------------------------------------------------
// The schema
// ---------------------------------------------------------------------------

/// One saved run.
///
/// Flat scalars rather than `Vec2` throughout, for the reason
/// [`crate::sim::probe`] uses them: glam's serde support is not enabled through
/// ggez, and a file of named numbers is one a human can read in a diff.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SaveState {
    /// Checked on load. First field so a corrupt-looking file still shows it.
    pub version: u32,
    /// The map to rebuild, relative to the assets directory.
    pub map: String,
    /// Ticks elapsed. Every cycle-driven hazard is a function of this.
    pub tick: u64,
    /// Ticks of impact freeze left to run.
    ///
    /// `#[serde(default)]` and no version bump, the same recipe [`Self::flags`]
    /// followed. A save written before this field existed loads with no freeze
    /// left, which is what that build was claiming by not writing it.
    #[serde(default)]
    pub hitstop: u32,
    /// The tick the *world* is standing at, which a freeze makes a different
    /// number from [`Self::tick`] — see [`Sim::decided_tick`], which explains
    /// why it cannot be worked out from the other two.
    ///
    /// An `Option` rather than a bare `u64`, so that the additive-field recipe
    /// can give an older save the right answer instead of a zero. Every tick of
    /// a run written by a build without this field had its world current for
    /// `tick - 1`, because that build could not carry a freeze across a load at
    /// all; `None` means exactly that, and [`restore`] fills it in.
    #[serde(default)]
    pub decided_tick: Option<u64>,
    pub rng: RngSave,
    pub player: PlayerSave,
    /// Quest flags, by name.
    ///
    /// `#[serde(default)]` and no version bump — the recipe this module's docs
    /// set out for an additive field, applied to the first one to arrive. A
    /// save written before quests existed loads with an empty map, which is
    /// what that run had.
    ///
    /// A `BTreeMap` for the reason [`PlayerSave::equipment`] is one: sorted by
    /// name, so the file a save writes is stable rather than hash-ordered, and
    /// two saves of the same run are the same text.
    #[serde(default)]
    pub flags: BTreeMap<String, i64>,
}

/// Where the run's randomness stood.
///
/// Both halves matter. The seed alone would restart the stream, so the first
/// loot roll after a load would repeat the first loot roll of the run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RngSave {
    pub seed: u64,
    /// [`Rng::position`]: the counter, which for SplitMix64 *is* the position.
    pub position: u64,
}

/// The player entity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayerSave {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub health: HealthSave,
    /// Absent for a kind with no pool at all — which is the knight today, and
    /// would be the player if `max_mana` were tuned to zero.
    pub mana: Option<ManaSave>,
    pub casting: Option<CastSave>,
    /// The bag, in bag order: the order the screen lists it in and the order
    /// things were picked up.
    pub inventory: Vec<StackSave>,
    /// What is worn.
    ///
    /// A `BTreeMap`, matching [`Equipment`], and that is load-bearing rather
    /// than incidental: deriving stats sums modifiers in this order, `f32`
    /// addition is not associative, and a `HashMap` here would make a reloaded
    /// run's numbers depend on hash seeding. Restoring into a `BTreeMap` puts
    /// the ordering back by construction, whatever order the file is in.
    pub equipment: BTreeMap<Slot, String>,
    pub avatar: AvatarSave,
    pub body: BodySave,
    /// A swing in progress, if there is one. The list of entities it has
    /// already connected with is not here — see the module docs.
    pub attack: Option<AttackSave>,
    pub animation: AnimationSave,
}

/// A stack in the bag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackSave {
    pub id: String,
    pub count: u32,
}

/// Health, minus the two numbers that are not state: `max` is derived from
/// equipment on load, and `iframe_ticks` comes from the kind's stat block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthSave {
    pub current: i32,
    pub iframes: u32,
    pub hitstun: u32,
}

/// The mana pool, minus `max` (derived) and `regen` (a stat).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManaSave {
    pub current: i32,
    /// Thousandths accumulated toward the next whole point. Saved so that a
    /// load does not quietly hand back — or take away — up to a point of mana.
    pub partial: u32,
}

/// A cast in progress and the cooldown behind it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CastSave {
    pub spell: Option<String>,
    pub elapsed: u32,
    pub cooldown: u32,
}

/// A swing in progress.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackSave {
    pub attack: String,
    pub elapsed: u32,
    pub chained: Option<String>,
}

/// The avatar's timers: every field of [`Avatar`].
///
/// Written out one by one rather than by deriving serde on the component,
/// because a save format that tracked a component's layout would change
/// whenever the component did, silently and without a version bump.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AvatarSave {
    pub facing_right: bool,
    pub coyote_ticks: u32,
    pub jump_buffer: u32,
    pub air_jumps: u8,
    pub drop_ticks: u32,
    pub wall_sliding: bool,
    pub wall_dir: f32,
    pub wall_coyote_ticks: u32,
    pub double_jumping: bool,
    pub crouching: bool,
    pub slide_ticks: u32,
    pub slide_cooldown: u32,
    pub plunge: Plunge,
    pub plunge_ticks: u32,
    pub dead_ticks: u32,
}

/// The body: its contact results, and the knobs the controller sets.
///
/// The knobs (`gravity`, `max_fall`, `fall_cap`, `ignore_one_way`) are rewritten
/// by the controller every tick before anything reads them, so restoring them
/// changes no outcome. They are here so that a restored entity is *identical*
/// to the one that was captured rather than merely equivalent — which is what
/// lets the round-trip test be an exact comparison instead of a judgement call
/// about which fields were allowed to differ.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BodySave {
    pub prev_x: f32,
    pub prev_y: f32,
    pub gravity: f32,
    pub max_fall: f32,
    pub fall_cap: Option<f32>,
    pub ignore_one_way: bool,
    pub frozen: bool,
    pub grounded: bool,
    pub on_solid: bool,
    pub on_one_way: bool,
    pub landed: bool,
}

/// Which clip is playing and how far into it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimationSave {
    pub clip: String,
    pub frame: usize,
    /// Seconds accumulated toward the next frame.
    pub elapsed: f32,
}

// ---------------------------------------------------------------------------
// Capture and restore
// ---------------------------------------------------------------------------

impl SaveState {
    /// Snapshot a running sim. [`Sim::save`] is the way to call this.
    ///
    /// Fails only for a sim with no map file to name — see [`SaveError::NoMap`].
    pub fn capture(sim: &Sim) -> Result<SaveState, SaveError> {
        let map = sim.map.clone().ok_or(SaveError::NoMap)?;
        let entity = player_entity(&sim.world).expect("sim has no avatar to save");

        #[allow(clippy::type_complexity)]
        let mut query = sim
            .world
            .query_one::<(
                &Position,
                &Velocity,
                &Avatar,
                &Body,
                &Health,
                &Attacking,
                &AnimationState,
                Option<&Mana>,
                Option<&Casting>,
                Option<&Inventory>,
                Option<&Equipment>,
            )>(entity)
            .expect("the avatar was just found in this world");
        let (pos, vel, avatar, body, health, attacking, anim, mana, casting, bag, gear) =
            query.get().expect("an avatar carries all of these");

        Ok(SaveState {
            version: SAVE_VERSION,
            map,
            tick: sim.tick,
            hitstop: sim.hitstop(),
            decided_tick: Some(sim.decided_tick()),
            rng: RngSave {
                seed: sim.rng.seed(),
                position: sim.rng.position(),
            },
            flags: sim.flags().clone(),
            player: PlayerSave {
                x: pos.0.x,
                y: pos.0.y,
                vx: vel.0.x,
                vy: vel.0.y,
                health: HealthSave {
                    current: health.current,
                    iframes: health.iframes,
                    hitstun: health.hitstun,
                },
                mana: mana.map(|mana| ManaSave {
                    current: mana.current,
                    partial: mana.partial,
                }),
                casting: casting.map(|casting| CastSave {
                    spell: casting.spell.clone(),
                    elapsed: casting.elapsed,
                    cooldown: casting.cooldown,
                }),
                inventory: bag
                    .map(|bag| {
                        bag.slots
                            .iter()
                            .map(|stack| StackSave {
                                id: stack.id.clone(),
                                count: stack.count,
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                equipment: gear.map(|gear| gear.slots.clone()).unwrap_or_default(),
                avatar: AvatarSave {
                    facing_right: avatar.facing_right,
                    coyote_ticks: avatar.coyote_ticks,
                    jump_buffer: avatar.jump_buffer,
                    air_jumps: avatar.air_jumps,
                    drop_ticks: avatar.drop_ticks,
                    wall_sliding: avatar.wall_sliding,
                    wall_dir: avatar.wall_dir,
                    wall_coyote_ticks: avatar.wall_coyote_ticks,
                    double_jumping: avatar.double_jumping,
                    crouching: avatar.crouching,
                    slide_ticks: avatar.slide_ticks,
                    slide_cooldown: avatar.slide_cooldown,
                    plunge: avatar.plunge,
                    plunge_ticks: avatar.plunge_ticks,
                    dead_ticks: avatar.dead_ticks,
                },
                body: BodySave {
                    prev_x: body.prev_pos.x,
                    prev_y: body.prev_pos.y,
                    gravity: body.gravity,
                    max_fall: body.max_fall,
                    fall_cap: body.fall_cap,
                    ignore_one_way: body.ignore_one_way,
                    frozen: body.frozen,
                    grounded: body.grounded,
                    on_solid: body.on_solid,
                    on_one_way: body.on_one_way,
                    landed: body.landed,
                },
                attack: attacking.attack.as_ref().map(|attack| AttackSave {
                    attack: attack.clone(),
                    elapsed: attacking.elapsed,
                    chained: attacking.chained.clone(),
                }),
                animation: AnimationSave {
                    clip: anim.clip.clone(),
                    frame: anim.frame,
                    elapsed: anim.elapsed,
                },
            },
        })
    }

    /// The version check, as its own step so that every path into a save goes
    /// through it: the store checks on read, and [`restore`] checks again for
    /// a `SaveState` that arrived some other way.
    pub fn check_version(&self) -> Result<(), SaveError> {
        if self.version == SAVE_VERSION {
            Ok(())
        } else {
            Err(SaveError::Version {
                found: self.version,
                expected: SAVE_VERSION,
            })
        }
    }

    /// Encode to RON.
    ///
    /// Without the `implicit_some` extension `assets.rs` enables, and
    /// deliberately: that exists so a *hand-authored* content file can write
    /// `sheet: "x"` instead of `Some("x")`. A save is machine-written, and the
    /// same round trip on both sides is worth more here than the brevity.
    pub fn to_ron(&self) -> Result<String, SaveError> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).map_err(|e| {
            SaveError::Encode {
                detail: e.to_string(),
            }
        })
    }

    /// Decode from RON, checking the version.
    pub fn from_ron(slot: &str, text: &str) -> Result<SaveState, SaveError> {
        let state: SaveState = ron::from_str(text).map_err(|e| SaveError::Corrupt {
            slot: slot.to_string(),
            detail: e.to_string(),
        })?;
        state.check_version()?;
        Ok(state)
    }
}

/// Rebuild a sim from a save: load the map it names, then put the player back.
/// [`Sim::load_save`] is the way to call this.
///
/// The order matters. `Sim::load` produces the map's spawn state — every NPC
/// where the map puts it — and this then overwrites exactly the player, the
/// clock and the generator. Whatever the save does not mention keeps the value
/// the map gave it, which is the whole of the "you reload at the map's spawn
/// state" design in one sentence.
///
/// "The clock" is [`Sim::resume_at`] rather than an assignment to `Sim::tick`,
/// and that is the load-bearing part of this function. `Sim::load` has just
/// built every fire, platform and swinging hazard on the map at tick 0; putting
/// the clock somewhere else without moving them with it leaves the world
/// describing a run that is not happening, and a platform in that state carries
/// its riders by its entire travel on the very next tick.
pub fn restore(assets: &mut Assets, save: &SaveState) -> anyhow::Result<Sim> {
    save.check_version()?;

    let mut sim = Sim::load(assets, &save.map)?;
    sim.resume_at(
        save.tick,
        save.hitstop,
        // A save from before the field existed had no freeze to be out of step
        // with, so its world was current for the tick before the one it names.
        save.decided_tick
            .unwrap_or_else(|| save.tick.saturating_sub(1)),
    );
    sim.rng = Rng::resume(save.rng.seed, save.rng.position);
    for (name, value) in &save.flags {
        sim.set_flag(name, *value);
    }

    let entity = player_entity(&sim.world)
        .ok_or_else(|| anyhow::anyhow!("`{}` spawned no player to load into", save.map))?;
    let player = &save.player;

    {
        let mut query = sim
            .world
            .query_one::<(&mut Position, &mut Velocity, &mut Avatar, &mut Body)>(entity)
            .expect("the avatar was just found in this world");
        let (pos, vel, avatar, body) = query.get().expect("an avatar carries all of these");

        pos.0 = Vec2::new(player.x, player.y);
        vel.0 = Vec2::new(player.vx, player.vy);

        let saved = &player.avatar;
        avatar.facing_right = saved.facing_right;
        avatar.coyote_ticks = saved.coyote_ticks;
        avatar.jump_buffer = saved.jump_buffer;
        avatar.air_jumps = saved.air_jumps;
        avatar.drop_ticks = saved.drop_ticks;
        avatar.wall_sliding = saved.wall_sliding;
        avatar.wall_dir = saved.wall_dir;
        avatar.wall_coyote_ticks = saved.wall_coyote_ticks;
        avatar.double_jumping = saved.double_jumping;
        avatar.crouching = saved.crouching;
        avatar.slide_ticks = saved.slide_ticks;
        avatar.slide_cooldown = saved.slide_cooldown;
        avatar.plunge = saved.plunge;
        avatar.plunge_ticks = saved.plunge_ticks;
        avatar.dead_ticks = saved.dead_ticks;

        let saved = &player.body;
        body.prev_pos = Vec2::new(saved.prev_x, saved.prev_y);
        body.gravity = saved.gravity;
        body.max_fall = saved.max_fall;
        body.fall_cap = saved.fall_cap;
        body.ignore_one_way = saved.ignore_one_way;
        body.frozen = saved.frozen;
        body.grounded = saved.grounded;
        body.on_solid = saved.on_solid;
        body.on_one_way = saved.on_one_way;
        body.landed = saved.landed;
    }

    {
        let mut query = sim
            .world
            .query_one::<(&mut Health, &mut Attacking, &mut AnimationState)>(entity)
            .expect("the avatar was just found in this world");
        let (health, attacking, anim) = query.get().expect("an avatar carries all of these");

        // `max` is not restored: it is a cache of the derived stat block, and
        // `derive_stats` below assigns it from the equipment this load just
        // put back on. Current health is clamped to it there, so a save whose
        // items have since been re-tuned downward loads as the smaller number
        // rather than as a permanently wrong maximum.
        health.current = player.health.current;
        health.iframes = player.health.iframes;
        health.hitstun = player.health.hitstun;

        match &player.attack {
            Some(saved) => {
                attacking.attack = Some(saved.attack.clone());
                attacking.elapsed = saved.elapsed;
                attacking.chained = saved.chained.clone();
            }
            None => attacking.stop(),
        }
        // Never restored, whether or not a swing was: it can only name entities
        // that the load has just replaced.
        attacking.hit.clear();

        anim.clip.clone_from(&player.animation.clip);
        anim.frame = player.animation.frame;
        anim.elapsed = player.animation.elapsed;
    }

    if let Some(saved) = &player.mana {
        if let Ok(mut mana) = sim.world.get::<&mut Mana>(entity) {
            mana.current = saved.current;
            mana.partial = saved.partial;
        }
    }
    if let Some(saved) = &player.casting {
        if let Ok(mut casting) = sim.world.get::<&mut Casting>(entity) {
            casting.spell.clone_from(&saved.spell);
            casting.elapsed = saved.elapsed;
            casting.cooldown = saved.cooldown;
        }
    }
    if let Ok(mut bag) = sim.world.get::<&mut Inventory>(entity) {
        // `capacity` is not restored: it comes from the kind's stat block, so a
        // bigger bag is content and takes effect for saves written before it.
        bag.slots = player
            .inventory
            .iter()
            .map(|stack| ItemStack {
                id: stack.id.clone(),
                count: stack.count,
            })
            .collect();
    }
    if let Ok(mut gear) = sim.world.get::<&mut Equipment>(entity) {
        gear.slots.clone_from(&player.equipment);
    }

    // Bring the derived block, `Health::max` and `Mana::max` up to date with
    // the equipment just restored — the same call the end of every tick makes.
    // Without it the first frame after a load reports the bare maximum, and a
    // trace taken before the first step would disagree with the run it came
    // from.
    inventory::derive_stats(&mut sim.world, &sim.items);

    // And the interact prompt, for the same reason the geometry was rebuilt
    // above: `Sim::new` computed it from the *spawn* point, and the player has
    // since been moved. Without this, the first `Interact` press after a load
    // acts on whatever happened to be standing near the spawn, and the first
    // `update_prompt` of the run emits or swallows an `InteractPrompted` that
    // does not describe anything that happened.
    //
    // The events it would push belong to no tick — nothing has been stepped —
    // so they are dropped rather than left for the first frame to report.
    sim.update_prompt();
    sim.clear_events();

    Ok(sim)
}

/// The player entity: the lowest-id thing with an [`Avatar`], so the answer is
/// stable rather than whatever hecs iterated first.
fn player_entity(world: &World) -> Option<hecs::Entity> {
    world
        .query::<&Avatar>()
        .iter()
        .map(|(entity, _)| entity)
        .min_by_key(|entity| entity.id())
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Where saves live.
///
/// The seam PLAN.md's rusqlite would slot into: a store owns *how* a
/// [`SaveState`] is kept and nothing else, and every caller above it is written
/// against this trait. Methods take `&self` so a store can be held behind a
/// `&dyn SaveStore` — a game with one save directory and a test with a map in
/// memory are the same code path.
pub trait SaveStore {
    /// Write `state` into `slot`, replacing whatever was there.
    fn write(&self, slot: &str, state: &SaveState) -> Result<(), SaveError>;

    /// Read `slot`, checking the version.
    fn read(&self, slot: &str) -> Result<SaveState, SaveError>;

    /// Whether anything is stored under `slot`. Never an error for a slot that
    /// is merely empty — that is the answer, not a failure.
    fn exists(&self, slot: &str) -> Result<bool, SaveError>;

    /// Every occupied slot, sorted, so a load menu lists them in a stable
    /// order rather than in directory order.
    fn slots(&self) -> Result<Vec<String>, SaveError>;

    /// Delete `slot`. Deleting an empty one is not an error.
    fn delete(&self, slot: &str) -> Result<(), SaveError>;
}

/// Slot names are file names. Anything outside this set could climb out of the
/// save directory or fail to round trip through a file system that folds case
/// or normalizes unicode.
fn check_slot(slot: &str) -> Result<(), SaveError> {
    let ok = !slot.is_empty()
        && slot.len() <= 64
        && slot
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(SaveError::BadSlot {
            slot: slot.to_string(),
        })
    }
}

/// Saves as RON files in a directory, one file per slot.
///
/// The shipping backend, and the reason the ticket is a file and not a
/// database: a save is a text file a human can read, a test can write in a
/// temporary directory in microseconds, and a commit can diff.
#[derive(Clone, Debug)]
pub struct FileStore {
    dir: PathBuf,
}

impl FileStore {
    pub fn new(dir: impl Into<PathBuf>) -> FileStore {
        FileStore { dir: dir.into() }
    }

    /// Where the game keeps its saves when nothing says otherwise.
    ///
    /// `./saves` when the game is run from the repo root, and the crate's own
    /// `saves/` when it is run from somewhere else during development — the
    /// rule [`Assets::new`] follows for content, decided the same way and for
    /// the same reason. Keyed off `assets/` rather than off `saves/`, because
    /// the save directory does not exist until the first save is written and
    /// "does it exist yet" would answer the wrong question on a fresh install.
    pub fn default_dir() -> PathBuf {
        if Path::new("assets").is_dir() {
            PathBuf::from("saves")
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("saves")
        }
    }

    /// The directory saves are kept in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, slot: &str) -> Result<PathBuf, SaveError> {
        check_slot(slot)?;
        Ok(self.dir.join(format!("{slot}.ron")))
    }

    fn io(path: &Path, error: std::io::Error) -> SaveError {
        SaveError::Io {
            path: path.display().to_string(),
            detail: error.to_string(),
        }
    }
}

impl SaveStore for FileStore {
    fn write(&self, slot: &str, state: &SaveState) -> Result<(), SaveError> {
        let path = self.path(slot)?;
        let text = state.to_ron()?;
        std::fs::create_dir_all(&self.dir).map_err(|e| FileStore::io(&self.dir, e))?;
        std::fs::write(&path, text).map_err(|e| FileStore::io(&path, e))
    }

    fn read(&self, slot: &str) -> Result<SaveState, SaveError> {
        let path = self.path(slot)?;
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(SaveError::Missing {
                    slot: slot.to_string(),
                })
            }
            Err(e) => return Err(FileStore::io(&path, e)),
        };
        SaveState::from_ron(slot, &text)
    }

    fn exists(&self, slot: &str) -> Result<bool, SaveError> {
        Ok(self.path(slot)?.is_file())
    }

    fn slots(&self) -> Result<Vec<String>, SaveError> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            // No directory yet is no saves yet, not a failure: the first save
            // of a fresh install creates it.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(FileStore::io(&self.dir, e)),
        };

        let mut slots: Vec<String> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|e| e == "ron"))
            .filter_map(|path| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .collect();
        slots.sort();
        Ok(slots)
    }

    fn delete(&self, slot: &str) -> Result<(), SaveError> {
        let path = self.path(slot)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(FileStore::io(&path, e)),
        }
    }
}

/// Saves in memory, as the text they would have been written as.
///
/// For tests, and for the seam: it keeps the *encoded* form rather than the
/// struct, so a test that never touches a file still exercises the same
/// serialization the real backend does — and a store that cheated by handing
/// the same `SaveState` back would not be testing anything.
#[derive(Debug, Default)]
pub struct MemoryStore {
    slots: Mutex<BTreeMap<String, String>>,
}

impl MemoryStore {
    pub fn new() -> MemoryStore {
        MemoryStore::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, String>> {
        // A poisoned lock means a test panicked mid-write; the map is a plain
        // string map with no invariant to violate, so carrying on is safe.
        self.slots.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl SaveStore for MemoryStore {
    fn write(&self, slot: &str, state: &SaveState) -> Result<(), SaveError> {
        check_slot(slot)?;
        self.lock().insert(slot.to_string(), state.to_ron()?);
        Ok(())
    }

    fn read(&self, slot: &str) -> Result<SaveState, SaveError> {
        check_slot(slot)?;
        let text = self
            .lock()
            .get(slot)
            .cloned()
            .ok_or_else(|| SaveError::Missing {
                slot: slot.to_string(),
            })?;
        SaveState::from_ron(slot, &text)
    }

    fn exists(&self, slot: &str) -> Result<bool, SaveError> {
        check_slot(slot)?;
        Ok(self.lock().contains_key(slot))
    }

    fn slots(&self) -> Result<Vec<String>, SaveError> {
        Ok(self.lock().keys().cloned().collect())
    }

    fn delete(&self, slot: &str) -> Result<(), SaveError> {
        check_slot(slot)?;
        self.lock().remove(slot);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::rng::DEFAULT_SEED;
    use crate::systems::input::PlayerInput;

    fn sim() -> Sim {
        Sim::load(&mut Assets::new(), "maps/testbed.ron").expect("the testbed map loads")
    }

    fn state() -> SaveState {
        sim().save().expect("a loaded map has a map name")
    }

    /// A directory of this test's own, emptied first so a rerun starts clean.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("supergame-save-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    // --- the schema ------------------------------------------------------

    #[test]
    fn a_save_round_trips_through_ron() {
        let before = state();
        let after = SaveState::from_ron("test", &before.to_ron().unwrap()).unwrap();
        assert_eq!(after, before);
    }

    /// Every shape in the schema that serde can get wrong: a `None`, a `Some`,
    /// an enum key in a map, a nested enum, an empty list.
    #[test]
    fn the_awkward_corners_of_the_schema_round_trip() {
        let mut before = state();
        before.player.mana = Some(ManaSave {
            current: 3,
            partial: 250,
        });
        before.player.casting = Some(CastSave {
            spell: Some("shock".to_string()),
            elapsed: 4,
            cooldown: 90,
        });
        before.player.attack = Some(AttackSave {
            attack: "player_slash2".to_string(),
            elapsed: 3,
            chained: None,
        });
        before.player.body.fall_cap = Some(60.0);
        before.player.avatar.plunge = Plunge::Falling;
        before.player.inventory = vec![StackSave {
            id: "minor_potion".to_string(),
            count: 2,
        }];
        before.player.equipment = BTreeMap::from([
            (Slot::Weapon, "iron_sword".to_string()),
            (Slot::Head, "knight_helm".to_string()),
        ]);

        let after = SaveState::from_ron("test", &before.to_ron().unwrap()).unwrap();
        assert_eq!(after, before);
        assert_eq!(
            after.player.equipment.keys().copied().collect::<Vec<_>>(),
            vec![Slot::Head, Slot::Weapon],
            "equipment comes back in slot order, whatever order it went in",
        );
    }

    /// The additive-field recipe, checked rather than described: a save file
    /// written before flags existed has no `flags` key, and must still load —
    /// with no quest started, which is the state that run was in.
    ///
    /// `hitstop` and `decided_tick` arrived the same way and are checked beside
    /// it, together and one at a time, so that the recipe is held down for the
    /// *fourth* field rather than for these three.
    #[test]
    fn a_save_written_before_flags_existed_still_loads() {
        const ADDED: [&str; 3] = ["flags", "hitstop", "decided_tick"];

        let text = state().to_ron().unwrap();
        for field in ADDED {
            assert!(text.contains(field), "`{field}` is written out: {text}");
        }

        // The same file with those keys removed, which is what an older build
        // wrote. Line-oriented because `to_ron` pretty-prints.
        let without = |dropped: &[&str]| -> String {
            text.lines()
                .filter(|line| {
                    let line = line.trim_start();
                    !dropped
                        .iter()
                        .any(|key| line.starts_with(&format!("{key}:")))
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        let mut older =
            SaveState::from_ron("test", &without(&ADDED)).expect("an older save still loads");
        assert!(older.flags.is_empty());
        assert_eq!(
            older.hitstop, 0,
            "no freeze left to run, which is what a save written before the \
             field existed was claiming by its silence",
        );
        assert_eq!(older.decided_tick, None);
        assert_eq!(older.version, SAVE_VERSION, "and needed no version bump");

        // ...and a missing `decided_tick` is filled in as `tick - 1` on the way
        // into a sim, which is where every tick of such a run had its world.
        older.tick = 137;
        assert_eq!(
            Sim::load_save(&mut Assets::new(), &older)
                .expect("a save with no decided tick loads")
                .decided_tick(),
            136,
        );

        // ...and each defaults on its own, not only as a set.
        for dropped in ADDED {
            SaveState::from_ron("test", &without(&[dropped]))
                .unwrap_or_else(|e| panic!("a save with no `{dropped}` key still loads: {e}"));
        }
    }

    /// The freeze after a hit is state about *where the run is*, not decoration
    /// on the tick it happened — so it is carried, and it comes back on the sim
    /// rather than only in the file. The tick the *world* stands at is carried
    /// beside it, because a freeze is exactly what makes that a different
    /// number.
    ///
    /// `tests/save.rs` has the half that matters: what a dropped freeze costs is
    /// not four ticks of animation but a reloaded run four ticks ahead of the
    /// one it was saved from for the rest of its life.
    #[test]
    fn the_impact_freeze_survives_a_save() {
        let mut sim = sim();
        assert_eq!(sim.save().unwrap().hitstop, 0, "a moving world has none");

        // Set by hand, because `maps/testbed.ron` places nothing to hit — which
        // is exactly why this field went unsaved for a milestone without any
        // test noticing. The clock reads 90 while the world stands at 86: four
        // ticks of freeze, one of them already spent.
        sim.resume_at(90, 3, 86);
        let state = sim.save().unwrap();
        assert_eq!(state.hitstop, 3);
        assert_eq!(state.tick, 90);
        assert_eq!(state.decided_tick, Some(86));

        let loaded = Sim::load_save(&mut Assets::new(), &state).unwrap();
        assert_eq!(loaded.hitstop(), 3, "and it came back on the sim");
        assert_eq!(loaded.tick, 90);
        assert_eq!(loaded.decided_tick(), 86, "world and clock still apart");
    }

    #[test]
    fn flags_survive_the_file_and_come_back_on_the_sim() {
        let mut sim = sim();
        sim.set_flag("quest.helm.stage", 2);
        sim.set_flag("quest.draught.stage", 1);

        let store = MemoryStore::new();
        store.write("slot1", &sim.save().unwrap()).unwrap();
        let state = store.read("slot1").unwrap();
        assert_eq!(state.flags.get("quest.helm.stage"), Some(&2));

        let loaded = Sim::load_save(&mut Assets::new(), &state).unwrap();
        assert_eq!(loaded.flag("quest.helm.stage"), 2);
        assert_eq!(loaded.flag("quest.draught.stage"), 1);
        assert_eq!(loaded.flag("quest.never.set"), 0, "and no others appeared");
        assert_eq!(loaded.flags(), sim.flags());
    }

    #[test]
    fn a_save_records_the_map_the_tick_and_the_generator() {
        let mut sim = sim();
        for _ in 0..30 {
            sim.step(Default::default());
        }
        let state = sim.save().unwrap();
        assert_eq!(state.version, SAVE_VERSION);
        assert_eq!(state.map, "maps/testbed.ron");
        assert_eq!(state.tick, 30);
        assert_eq!(state.rng.seed, DEFAULT_SEED);
    }

    /// A fixture sim has no map file, so there is nothing a load could rebuild
    /// it from. That is an error with a sentence in it, not a panic.
    #[test]
    fn a_sim_with_no_map_cannot_be_saved() {
        let sim = Sim::fixture(&["..........", "..P.......", "##########"]);
        let err = sim.save().unwrap_err();
        assert!(matches!(err, SaveError::NoMap), "{err}");
        assert!(err.to_string().contains("inline grid"), "{err}");
    }

    // --- the generator ---------------------------------------------------

    /// The bug this exists to prevent: a load that restarts the stream hands
    /// out the loot the *first* kill of the run would have dropped.
    #[test]
    fn a_load_resumes_the_stream_rather_than_restarting_it() {
        let mut sim = sim();
        for _ in 0..17 {
            sim.rng.next_u64();
        }
        let state = sim.save().unwrap();
        let expected: Vec<u64> = (0..8).map(|_| sim.rng.next_u64()).collect();

        let mut loaded = Sim::load_save(&mut Assets::new(), &state).unwrap();
        assert_eq!(loaded.rng.seed(), sim.seed(), "the seed came back");
        let actual: Vec<u64> = (0..8).map(|_| loaded.rng.next_u64()).collect();
        assert_eq!(actual, expected, "the stream carried on where it left off");

        // ...and it is genuinely the position doing the work, not the seed.
        let restarted: Vec<u64> = (0..8)
            .map(|_| Rng::new(state.rng.seed).next_u64())
            .collect();
        assert_ne!(actual, restarted);
    }

    /// The prompt is computed from where the player *is*, and a load moves
    /// them after the map has been built.
    ///
    /// `Sim::new` resolves it at the spawn point, so without a refresh at the
    /// end of `restore` the first `Interact` press of a loaded run acts on
    /// whatever happened to be standing near the spawn — and the first
    /// `update_prompt` of the run emits or swallows an `InteractPrompted`
    /// describing a walk that never happened. Both directions are checked
    /// here, because saving *out* of reach is the case a spawn-side prompt
    /// gets wrong in the more surprising way.
    #[test]
    fn the_interact_prompt_follows_the_player_across_a_load() {
        let village = |ticks: u32, x: f32| -> Sim {
            let mut sim =
                Sim::load(&mut Assets::new(), "maps/testbed_village.ron").expect("village loads");
            for _ in 0..ticks {
                sim.step(PlayerInput::default());
            }
            let entity = player_entity(&sim.world).expect("the village places a player");
            sim.world.get::<&mut Position>(entity).unwrap().0.x = x;
            sim
        };

        // Saved standing next to the villager: the prompt is there on load,
        // before a single tick has run.
        let near = village(30, 470.0);
        let loaded = Sim::load_save(&mut Assets::new(), &near.save().unwrap()).unwrap();
        assert!(
            loaded.prompt().is_some(),
            "a load beside an NPC should offer the prompt immediately"
        );
        assert!(
            loaded.events().is_empty(),
            "and the events that fall out of resolving it belong to no tick"
        );

        // Saved far away: the prompt must not be inherited from the spawn
        // point, which on this map is within reach of nothing.
        let far = village(30, 70.0);
        let loaded = Sim::load_save(&mut Assets::new(), &far.save().unwrap()).unwrap();
        assert!(
            loaded.prompt().is_none(),
            "a load away from every NPC should offer nothing"
        );
    }

    #[test]
    fn a_non_default_seed_survives_a_load() {
        let mut sim = sim();
        sim.reseed(4242);
        for _ in 0..5 {
            sim.rng.next_u64();
        }
        let loaded = Sim::load_save(&mut Assets::new(), &sim.save().unwrap()).unwrap();
        assert_eq!(loaded.seed(), 4242);
        assert_eq!(loaded.rng, sim.rng);
    }

    // --- versions, corruption, absence -----------------------------------

    #[test]
    fn a_save_from_another_version_is_refused_by_name() {
        let store = MemoryStore::new();
        let mut state = state();
        state.version = SAVE_VERSION + 1;
        store.write("slot1", &state).unwrap();

        let err = store.read("slot1").unwrap_err();
        assert!(
            matches!(err, SaveError::Version { found, expected }
                     if found == SAVE_VERSION + 1 && expected == SAVE_VERSION),
            "{err}",
        );
        let message = err.to_string();
        assert!(message.contains("version 2"), "{message}");
        assert!(message.contains("different version"), "{message}");
    }

    /// ...and it is refused on every path in, not only through a store.
    #[test]
    fn a_version_mismatch_is_refused_by_the_loader_too() {
        let mut state = state();
        state.version = 99;
        let Err(err) = Sim::load_save(&mut Assets::new(), &state) else {
            panic!("a save from version 99 loaded");
        };
        assert!(
            err.downcast_ref::<SaveError>()
                .is_some_and(|e| matches!(e, SaveError::Version { .. })),
            "{err:#}",
        );
    }

    /// A save names its map by path, so a map that gets renamed or deleted
    /// makes every save of it unloadable. That has to fail saying which file it
    /// wanted, rather than somewhere further down inside the level loader.
    #[test]
    fn a_save_naming_a_map_that_is_gone_says_which_map() {
        let mut state = state();
        state.map = "maps/no_such_place.ron".to_string();
        let Err(err) = Sim::load_save(&mut Assets::new(), &state) else {
            panic!("a save naming a missing map loaded");
        };
        assert!(format!("{err:#}").contains("no_such_place"), "{err:#}");
    }

    #[test]
    fn a_corrupt_save_is_an_error_rather_than_a_panic() {
        let dir = scratch("corrupt");
        let store = FileStore::new(&dir);
        store.write("slot1", &state()).unwrap();
        std::fs::write(dir.join("slot1.ron"), "this is not a save file").unwrap();

        let err = store.read("slot1").unwrap_err();
        assert!(matches!(err, SaveError::Corrupt { .. }), "{err}");
        assert!(err.to_string().contains("slot1"), "{err}");

        // Truncated half way through is the more realistic corruption, and
        // must land in the same place rather than deserializing to nonsense.
        let text = state().to_ron().unwrap();
        std::fs::write(dir.join("slot1.ron"), &text[..text.len() / 2]).unwrap();
        assert!(matches!(
            store.read("slot1").unwrap_err(),
            SaveError::Corrupt { .. }
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_save_says_so_rather_than_failing_some_other_way() {
        let dir = scratch("missing");
        let store = FileStore::new(&dir);

        let err = store.read("slot1").unwrap_err();
        assert!(matches!(err, SaveError::Missing { .. }), "{err}");
        assert!(err.to_string().contains("slot1"), "{err}");

        // A directory that does not exist at all is the same answer, and
        // listing it is empty rather than an error — a fresh install has no
        // saves and no save directory.
        assert!(!store.exists("slot1").unwrap());
        assert_eq!(store.slots().unwrap(), Vec::<String>::new());
        assert!(matches!(
            MemoryStore::new().read("slot1").unwrap_err(),
            SaveError::Missing { .. }
        ));
    }

    /// A slot name is a file name, so one that could climb out of the save
    /// directory is refused before it is ever joined to a path.
    #[test]
    fn a_slot_name_cannot_escape_the_save_directory() {
        let store = FileStore::new(scratch("slots"));
        for bad in ["../escape", "a/b", "", "with space", "sl.ot"] {
            assert!(
                matches!(store.read(bad), Err(SaveError::BadSlot { .. })),
                "`{bad}` should not be a usable slot",
            );
            assert!(matches!(
                store.write(bad, &state()),
                Err(SaveError::BadSlot { .. })
            ));
        }
        assert!(store.path("autosave_1").is_ok());
    }

    // --- the store contract ----------------------------------------------

    /// Both backends answer the same questions the same way, which is what
    /// makes the trait a seam rather than a decoration.
    #[test]
    fn every_backend_keeps_the_same_contract() {
        let dir = scratch("contract");
        let file = FileStore::new(&dir);
        let memory = MemoryStore::new();
        let stores: [&dyn SaveStore; 2] = [&file, &memory];

        for store in stores {
            assert_eq!(store.slots().unwrap(), Vec::<String>::new());
            assert!(!store.exists("slot1").unwrap());

            let mut state = state();
            state.tick = 123;
            store.write("slot1", &state).unwrap();
            store.write("autosave", &state).unwrap();

            assert!(store.exists("slot1").unwrap());
            assert_eq!(store.slots().unwrap(), vec!["autosave", "slot1"]);
            assert_eq!(store.read("slot1").unwrap(), state);

            // Overwriting replaces rather than appending.
            state.tick = 456;
            store.write("slot1", &state).unwrap();
            assert_eq!(store.read("slot1").unwrap().tick, 456);
            assert_eq!(store.slots().unwrap().len(), 2);

            store.delete("slot1").unwrap();
            assert!(!store.exists("slot1").unwrap());
            assert_eq!(store.slots().unwrap(), vec!["autosave"]);
            store
                .delete("slot1")
                .expect("deleting nothing is not an error");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The file backend's whole point: a save is a text file you can read.
    #[test]
    fn a_save_file_is_readable_text_on_disk() {
        let dir = scratch("text");
        let store = FileStore::new(&dir);
        store.write("slot1", &state()).unwrap();

        let text = std::fs::read_to_string(dir.join("slot1.ron")).unwrap();
        assert!(text.contains("version: 1"), "{text}");
        assert!(text.contains("maps/testbed.ron"), "{text}");
        assert!(text.lines().count() > 10, "pretty-printed, not one line");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
