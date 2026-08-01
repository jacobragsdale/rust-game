//! The simulation: everything that decides what happens in the game, with
//! no `ggez::Context`, no window, and no GPU.
//!
//! This split is what makes the game verifiable without playing it. A [`Sim`]
//! can be built and stepped from a unit test, an integration test, or the
//! headless `sim` binary, and it produces exactly the same results as the
//! real game because the real game runs this same code — `AdventureScene`
//! owns a `Sim` and delegates its whole update to [`Sim::step`].
//!
//! It works because only `Assets::image` needs a graphics context; the RON
//! side of the asset cache (tilesets, animation clips) is pure file I/O.

pub mod event;
pub mod probe;
pub mod rng;
pub mod run;
pub mod tape;
pub mod trace;

use std::collections::{hash_map, BTreeMap, HashMap};
use std::sync::Arc;

use anyhow::Context as _;
use ggez::glam::Vec2;
use hecs::World;
use serde::{Deserialize, Serialize};

use crate::assets::{
    Assets, AttackTable, Clip, ClipSet, DialogueTable, ItemTable, SpellEffect, SpellTable,
    StatTable,
};
use crate::ecs::components::{
    AnimationState, Attacking, Avatar, Body, Casting, Equipment, Health, InteractTarget, Inventory,
    Kind, Mana, Patrol, Position, Size, Sprite, Velocity,
};
use crate::ecs::spawn;
use crate::level::LevelData;
use crate::physics::{Aabb, Geometry};
use crate::sim::rng::Rng;
use crate::systems::input::{Action, ActionSet, PlayerInput};
use crate::systems::{animation, avatar, body, combat, dialogue, inventory, mover, npc, spell};

pub use event::GameEvent;
pub use probe::{ItemProbe, NpcProbe, Probe};
pub use rng::DEFAULT_SEED;

/// The simulation runs on a fixed timestep so that behavior is identical at
/// any frame rate — and, just as importantly, reproducible between runs.
/// Determinism is what makes input tapes and trace diffing work.
pub const TICKS_PER_SECOND: u32 = 60;
pub const TICK: f32 = 1.0 / TICKS_PER_SECOND as f32;

/// How long the world freezes when a hit lands. Four ticks is about 65 ms —
/// long enough to register as impact, short enough not to read as a hitch.
const HITSTOP_TICKS: u32 = 4;

/// What the simulation is currently doing.
///
/// A modal mode pauses the world but is still *simulation*: drinking a potion
/// with the inventory open changes health, which changes whether the next fight
/// is survivable. That is why the mode lives on [`Sim`] and why the systems a
/// mode runs are systems — a screen whose state lived in `scenes/` could not be
/// driven by a tape, and the two largest features left on the roadmap
/// (inventory and dialogue) are both this shape.
///
/// [`Mode::Dialogue`] is here unused so that M5 does not have to reopen the
/// dispatch to add it.
///
/// Pause is deliberately **not** a mode. Pausing stops the sim being stepped at
/// all; nothing about it is simulation, and it stays where it is, in
/// [`crate::scenes::pause`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// The world runs.
    #[default]
    Playing,
    /// The inventory screen is open and the world is frozen.
    Inventory,
    /// A conversation is running and the world is frozen. Nothing enters this
    /// yet; ticket D-2 does.
    Dialogue,
}

impl Mode {
    /// How a tape spells it: `assert mode == inventory`.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Playing => "playing",
            Mode::Inventory => "inventory",
            Mode::Dialogue => "dialogue",
        }
    }

    /// Whether the world is frozen in this mode.
    pub fn is_modal(self) -> bool {
        !matches!(self, Mode::Playing)
    }
}

pub struct Sim {
    pub world: World,
    pub level: LevelData,
    /// The map this run was loaded from, relative to the assets directory —
    /// `None` for a sim built from an inline grid, which has no file to name.
    /// Nothing in the tick reads it; it is here because a save has to say what
    /// to rebuild, and the level itself does not remember where it came from.
    pub map: Option<String>,
    /// Everything a body can collide with: the level's solids and one-way
    /// platforms, plus whatever colliders entities own this tick. Rebuilt at
    /// one point per tick — see [`Sim::step`].
    pub geometry: Geometry,
    /// Every attack's timing and effect, shared by every attacker.
    pub attacks: Arc<AttackTable>,
    /// Every spell's cost, timing and effect, shared by every caster.
    pub spells: Arc<SpellTable>,
    /// Every kind's movement and combat numbers. `spawn` hands each entity the
    /// block for its kind as a `Stats` component; this is kept so that
    /// anything spawned later in the run can be given one too.
    pub stats: Arc<StatTable>,
    /// Every item in the game, shared by every bag, loot table and pickup.
    pub items: Arc<ItemTable>,
    /// Every conversation in the game, by graph id. An `Interactable` names
    /// one; nothing else reaches in here.
    pub dialogues: Arc<DialogueTable>,
    /// The simulation's only source of randomness, seeded per run.
    ///
    /// Anything random — a loot roll, a crit, a spell's spread — draws from
    /// here and from nowhere else. A tape and its golden trace are only worth
    /// anything if the same seed replays the same run, and an unseeded
    /// generator anywhere would silently end that.
    pub rng: Rng,
    /// Ticks elapsed since the sim was created.
    pub tick: u64,
    /// The tick this world's geometry is current for: the last one whose
    /// *decide* phase actually ran, and so the tick every fire, platform and
    /// swinging hazard is standing at.
    ///
    /// Almost always `tick - 1` — the decide phase of tick *t* places all three
    /// and the counter advances afterwards. [`Sim::hitstop`] is what makes it a
    /// separate number: [`Sim::step_playing`] returns before anything decides
    /// while a freeze runs, so the clock moves through it and the world does
    /// not, and the two end the freeze [`HITSTOP_TICKS`] apart until the next
    /// live tick closes the gap in one step.
    ///
    /// Kept rather than derived, because it cannot be derived. A `hitstop` that
    /// has counted down to 0 tells exactly the same story for "the freeze
    /// ended, nothing has run yet" as for "nothing has been hit in a hundred
    /// ticks", and those two need opposite answers — four ticks behind the
    /// clock, and one. A load that guessed would hand a rider four ticks of
    /// platform travel in a single tick; see [`Sim::resume_at`].
    decided_tick: u64,
    /// What the simulation is doing: running the world, or holding it still
    /// while a modal screen is up. Private, because every transition emits a
    /// [`GameEvent::ModeChanged`] and a mode set behind the event's back would
    /// be a stretch of trace nobody could explain.
    mode: Mode,
    /// The inventory screen's own state: which pane has focus and where the
    /// selection is. On `Sim` rather than in a scene because using a potion is
    /// simulation — see [`Mode`].
    screen: inventory::Screen,
    /// The conversation in progress, if any: which graph, which node, which of
    /// its replies are on offer and which is highlighted. Here rather than in
    /// [`crate::scenes::dialogue`] for the reason the screen above is — a
    /// choice takes an item out of the bag and sets a quest flag — and the
    /// argument is made in full in [`crate::systems::dialogue`].
    ///
    /// An `Option` rather than an always-present struct because "not talking to
    /// anyone" has no sensible default node, and because closing one
    /// conversation must not leave a selection behind for the next.
    conversation: Option<dialogue::Conversation>,
    /// What pressing `Interact` would act on, recomputed once per tick in the
    /// resolve phase. Cached rather than worked out at the moment of the press
    /// so that the prompt drawn on screen and the thing the key does are one
    /// answer to one question.
    prompt: Option<dialogue::Prompt>,
    /// Quest flags: named integers, defaulting to 0.
    ///
    /// Integers rather than booleans because a quest *stage* is the common
    /// case, and a boolean forces `quest.x.started` plus `quest.x.done` plus
    /// their interaction. Written by dialogue's `SetFlag` and read by its
    /// `FlagEq` / `FlagAtLeast`.
    ///
    /// A `BTreeMap` rather than a `HashMap`, for the reason
    /// [`crate::ecs::components::Equipment`] is one: this is serialized — into
    /// every trace frame and into every save — and a hash map's iteration order
    /// varies with the hash seed, which would make a golden trace disagree with
    /// itself between runs. Sorted by name is also the order a person reading a
    /// save file wants.
    ///
    /// Surfaced to tapes and traces by [`Sim::flags`]; see
    /// [`crate::sim::trace::ProbePath`] for the path syntax.
    flags: BTreeMap<String, i64>,
    /// What was held last tick.
    ///
    /// Directions are level-triggered, because movement needs them held down;
    /// a menu needs one step per press. The difference between two ticks is
    /// taken here, once, so that any mode reading a direction as a one-shot
    /// reads the same answer. This is not a second
    /// [`crate::systems::input::InputLatch`] — that one is about presses being
    /// lost between rendered frames and simulation ticks, and it is upstream of
    /// everything here.
    prev_held: ActionSet,
    /// Ticks of impact freeze remaining. A hit stops the whole world for a
    /// few frames, which is the cheapest way to make a sword feel like it
    /// weighs something — the blow reads as landing rather than as a number
    /// changing. Input is still read, so it never feels like a stall.
    hitstop: u32,
    /// What happened during the most recent [`Sim::step`]. Cleared at the
    /// start of each tick, so this is never a running log — the tape runner
    /// and the trace are what accumulate.
    events: Vec<GameEvent>,
}

impl Sim {
    /// Load a map (path relative to the assets directory), spawn the player,
    /// and spawn everything the map places.
    pub fn load(assets: &mut Assets, map: &str) -> anyhow::Result<Self> {
        let map_path = assets.base_dir().join(map);
        let level = LevelData::load(&map_path, assets)?;

        // One clip set per kind actually present, so a map that places no
        // knights does not need the knight art to exist.
        let mut clip_sets: HashMap<String, Arc<ClipSet>> = HashMap::new();
        clip_sets.insert("player".to_string(), assets.clip_set("player")?);
        for placement in &level.entities {
            if let hash_map::Entry::Vacant(slot) = clip_sets.entry(placement.kind.clone()) {
                slot.insert(assets.clip_set(&placement.kind).with_context(|| {
                    format!(
                        "map places `{}` but its clip set is missing",
                        placement.kind
                    )
                })?);
            }
        }

        let attacks = assets.attacks()?;
        let stats = assets.stats()?;
        // Loaded through the cache here so a broken `spells.ron`, item file or
        // dialogue graph fails map loading with a message, rather than
        // panicking inside `Sim::new`. A dangling `next` is reported by
        // `Assets::dialogue`, naming the graph and the node.
        assets.spells()?;
        assets.items()?;
        assets.dialogue()?;
        let mut sim = Self::new(level, &clip_sets, attacks, stats, rng::DEFAULT_SEED);
        // What to rebuild from, for a save. The level itself does not remember
        // where it was loaded from.
        sim.map = Some(map.to_string());
        Ok(sim)
    }

    /// A sim built from an inline ASCII grid, with placeholder animation
    /// clips and no filesystem access at all.
    ///
    /// ```
    /// # use supergame::sim::Sim;
    /// let mut sim = Sim::fixture(&[
    ///     "..........",
    ///     "..P.......",
    ///     "##########",
    /// ]);
    /// sim.step(Default::default());
    /// assert!(sim.probe().grounded);
    /// ```
    ///
    /// Panics on a malformed grid: this is a test helper, and a fixture that
    /// does not parse is a mistake in the test, not a condition to handle.
    pub fn fixture(grid: &[&str]) -> Self {
        let level = LevelData::from_grid(grid).expect("fixture grid should parse");
        // Fixtures read the real attack and stat tables: movement and combat
        // numbers are content, and a test that invents its own would not be
        // testing the game.
        let attacks = Assets::new()
            .attacks()
            .expect("assets/data/attacks.ron should load");
        Sim::new(
            level,
            &fixture_clip_sets(),
            attacks,
            StatTable::shipped(),
            rng::DEFAULT_SEED,
        )
    }

    /// Build a sim from level data, a clip set per entity kind (plus
    /// `"player"`), the tables everything reads its numbers from, and the seed
    /// its randomness runs at. Panics if a kind the level places has no clip
    /// set or no stat block — callers are expected to have loaded both, and
    /// there is nothing sensible to spawn otherwise.
    ///
    /// The seed is a parameter rather than a constant because a run has to be
    /// reproducible from what is written down: a tape's `seed` directive, or
    /// [`rng::DEFAULT_SEED`] when nothing says otherwise.
    ///
    /// The spell and item tables are deliberately *not* parameters. Attacks
    /// and stats are, for historical reasons — they were threaded through
    /// every caller before there was anywhere else to put them — but there is
    /// exactly one of each of the other two in the game, with no per-map and
    /// no per-run variation, so they are read from [`SpellTable::shipped`] and
    /// [`ItemTable::shipped`] here. Give either a parameter the day something
    /// genuinely needs a different one.
    pub fn new(
        level: LevelData,
        clip_sets: &HashMap<String, Arc<ClipSet>>,
        attacks: Arc<AttackTable>,
        stats: Arc<StatTable>,
        seed: u64,
    ) -> Self {
        let geometry = Geometry::from_level(&level.solids, &level.one_way, &level.hazards);

        let clips_for = |kind: &str| -> Arc<ClipSet> {
            clip_sets
                .get(kind)
                .unwrap_or_else(|| panic!("no clip set loaded for `{kind}`"))
                .clone()
        };
        let stats_for = |kind: &str| stats.get(kind).unwrap_or_else(|e| panic!("{e:#}"));

        let mut world = World::new();
        spawn::player(
            &mut world,
            level.player_spawn,
            clips_for("player"),
            stats_for("player"),
        );

        // Map order, which is the grid scanned top-left to bottom-right and
        // then the explicit entity list. Spawn order decides `npc.<n>` in
        // traces and tape assertions, so it has to be a property of the map
        // rather than of iteration luck.
        for placement in &level.entities {
            spawn::entity(
                &mut world,
                placement,
                level.tile_size,
                clips_for(&placement.kind),
                stats_for(&placement.kind),
            )
            .expect("level entities were validated on load");
        }
        // Fires and platforms last, so adding one to a map cannot renumber the
        // NPCs that tapes and traces address by spawn index.
        crate::systems::hazard::spawn_fires(&mut world, &level);
        crate::systems::mover::spawn_movers(&mut world, &level);
        crate::systems::pendulum::spawn_pendulums(&mut world, &level);

        let mut sim = Sim {
            world,
            level,
            // Set by `Sim::load`, which is the only caller that has one.
            map: None,
            geometry,
            attacks,
            spells: SpellTable::shipped(),
            stats,
            items: ItemTable::shipped(),
            dialogues: DialogueTable::shipped(),
            rng: Rng::new(seed),
            tick: 0,
            // The spawn helpers above placed every fire, platform and ball
            // where tick 0 says it is, so that is the tick this world is
            // current for before a single step.
            decided_tick: 0,
            mode: Mode::Playing,
            screen: inventory::Screen::default(),
            conversation: None,
            prompt: None,
            flags: BTreeMap::new(),
            prev_held: ActionSet::EMPTY,
            hitstop: 0,
            events: Vec::new(),
        };
        // Colliders the map placed exist from tick 0, not from the first
        // rebuild inside `step` — otherwise the first tick's controllers probe
        // a world with holes in it.
        body::rebuild_geometry(&mut sim.geometry, &sim.world);
        // ...and the same for the prompt: standing next to someone at the
        // spawn point should offer the conversation on tick 0 rather than one
        // tick later, which is what a map with an NPC by the door looks like.
        sim.prompt = dialogue::nearest_interactable(&sim.world);
        sim
    }

    /// The seed this run's randomness started from.
    pub fn seed(&self) -> u64 {
        self.rng.seed()
    }

    /// Restart the randomness at a new seed. Used by the tape runner, which
    /// only learns the seed after the sim has been built from a map.
    pub fn reseed(&mut self, seed: u64) {
        self.rng = Rng::new(seed);
    }

    /// Ticks of impact freeze left to run. Zero for a world that is moving.
    ///
    /// Read by [`crate::save`], which has to carry it: four ticks of freeze
    /// dropped from a save do not cost four ticks of animation, they leave the
    /// reloaded run permanently four ticks ahead of the one it was saved from.
    pub fn hitstop(&self) -> u32 {
        self.hitstop
    }

    /// The tick this world's geometry is current for — see the field.
    pub fn decided_tick(&self) -> u64 {
        self.decided_tick
    }

    /// Put the clock back where a save left it — and, with it, everything that
    /// is a pure function of the clock.
    ///
    /// [`Sim::new`] builds a world at tick 0: platforms at `Mover::at(0)`,
    /// balls at `Pendulum::at(0)`, fires lit or out according to tick 0, and
    /// the [`Geometry`] built from all three. Setting [`Sim::tick`] afterwards
    /// left every one of them describing a run that was not happening. For a
    /// fire or a ball that is a stale box in the geometry the next tick's
    /// controllers probe, which the tick after overwrites; for a platform it is
    /// not cosmetic at all, because [`mover::advance`] reads the difference
    /// between where the platform *is* and where the tick says it should be and
    /// hands that difference to its riders — see [`mover::place`]. So the clock
    /// and the world it drives move together, here and nowhere else.
    ///
    /// Three numbers rather than one, because the clock is genuinely three
    /// things. `tick` is where the run is; `hitstop` is how much of it is still
    /// frozen; `decided_tick` is where the *world* is, which a freeze makes a
    /// different number from the first and which no arithmetic on the other two
    /// can recover — see [`Sim::decided_tick`].
    pub(crate) fn resume_at(&mut self, tick: u64, hitstop: u32, decided_tick: u64) {
        self.tick = tick;
        self.hitstop = hitstop;
        self.decided_tick = decided_tick;

        crate::systems::hazard::tick_schedules(&mut self.world, decided_tick);
        crate::systems::pendulum::advance(&mut self.world, decided_tick);
        mover::place(&mut self.world, decided_tick);
        body::rebuild_geometry(&mut self.geometry, &self.world);
    }

    /// Snapshot this run into a [`crate::save::SaveState`].
    ///
    /// Fails only for a sim with no map file to name — [`Sim::fixture`] builds
    /// one from an inline grid, and there would be nothing for a load to
    /// rebuild. What a save does and does not carry is documented on
    /// [`crate::save`].
    pub fn save(&self) -> Result<crate::save::SaveState, crate::save::SaveError> {
        crate::save::SaveState::capture(self)
    }

    /// Rebuild a sim from a save: load the map it names, put the map's own
    /// entities where the map puts them, and then put the player, the tick and
    /// the generator back where the save left them.
    pub fn load_save(assets: &mut Assets, save: &crate::save::SaveState) -> anyhow::Result<Sim> {
        crate::save::restore(assets, save)
    }

    /// What the simulation is currently doing.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The inventory screen's state, for [`crate::scenes::inventory`] to draw
    /// and for nothing to decide with.
    pub fn screen(&self) -> &inventory::Screen {
        &self.screen
    }

    /// The conversation in progress, for [`crate::scenes::dialogue`] to draw
    /// and for nothing to decide with.
    pub fn conversation(&self) -> Option<&dialogue::Conversation> {
        self.conversation.as_ref()
    }

    /// What pressing `Interact` would act on right now, for [`crate::hud`] to
    /// draw over whoever is offering it.
    pub fn prompt(&self) -> Option<&dialogue::Prompt> {
        self.prompt.as_ref()
    }

    /// Read a quest flag. An unset one is 0 rather than an error: a quest has
    /// to be able to ask about a stage it has not reached.
    pub fn flag(&self, name: &str) -> i64 {
        dialogue::flag(&self.flags, name)
    }

    /// Every flag that has been set, by name, in name order.
    ///
    /// Only the ones actually written: an unset flag is 0 everywhere that
    /// reads one, and writing every name a quest could ever use into every
    /// trace frame would be a column per quest in the game forever. The
    /// consequence a baseline depends on is that a run which sets no flag has
    /// *no* flags key at all — see [`crate::sim::trace::Frame::flags`].
    pub fn flags(&self) -> &BTreeMap<String, i64> {
        &self.flags
    }

    /// Set a quest flag, exactly as a dialogue `SetFlag` effect does.
    ///
    /// For tests and for content that has no conversation behind it yet. The
    /// game itself sets flags through dialogue.
    pub fn set_flag(&mut self, name: &str, value: i64) {
        self.flags.insert(name.to_string(), value);
    }

    /// Advance one fixed tick, dispatching on the mode.
    ///
    /// `Playing` runs the world, in the phases documented on
    /// [`Sim::step_playing`]. A modal mode runs only that mode's own state
    /// machine and leaves the world untouched — no controllers, no movement,
    /// no combat, no animation, and not even the hitstop counter, which
    /// resumes exactly where it was. "Frozen" here means every field of the
    /// probe except `tick` and `mode`, and
    /// `a_modal_mode_freezes_every_probe_field_but_the_tick` is the test that
    /// says so.
    ///
    /// **The tick counter advances in every mode**, exactly as it does during
    /// hitstop, so a trace stays aligned with wall-clock time and time spent in
    /// a menu is visible as the run of identical frames it is.
    pub fn step(&mut self, input: PlayerInput) {
        self.events.clear();

        // Level-triggered actions are held; a menu wants one step per press.
        // Taken once, here, so every mode reads the same answer.
        let newly_held = input.held_set().newly_set(self.prev_held);

        match self.mode {
            Mode::Playing => self.step_playing(input),
            Mode::Inventory => {
                if inventory::step_screen(
                    &mut self.world,
                    &self.items,
                    &mut self.screen,
                    input,
                    newly_held,
                    &mut self.events,
                ) {
                    self.set_mode(Mode::Playing);
                }
            }
            Mode::Dialogue => {
                let close = match self.conversation.as_mut() {
                    Some(talk) => dialogue::step_conversation(
                        &mut self.world,
                        &self.items,
                        &mut self.flags,
                        talk,
                        input,
                        newly_held,
                        &mut self.events,
                    ),
                    // The mode set by hand with no conversation behind it, as
                    // a test may do. `cancel` is honoured so it is never a
                    // dead end.
                    None => input.pressed(Action::Cancel),
                };
                if close {
                    self.end_dialogue();
                }
            }
        }

        // `base + sum(modifiers)`, brought up to date once per tick in every
        // mode.
        //
        // Outside the dispatch on purpose, and *after* it. This is not the
        // world advancing — it recomputes a view of state that already
        // changed, and with nothing equipped it does not even allocate — so a
        // frozen world stays frozen through it. It has to run in the modal
        // modes because the inventory screen is the one place equipment ever
        // changes, and it has to run after them because otherwise the tick you
        // put a helm on would end with the old maximum health still showing.
        // Running at the end also means the controllers at the top of the next
        // tick read a block that is already correct, which is the ordering
        // every read site assumes.
        inventory::derive_stats(&mut self.world, &self.items);

        self.prev_held = input.held_set();
        self.tick += 1;

        #[cfg(debug_assertions)]
        self.check_invariants();
    }

    /// Enter a mode, announcing it.
    ///
    /// Private, and the only way the mode changes: a transition that did not
    /// emit its event would leave a stretch of frozen frames in a trace with
    /// nothing to explain them.
    fn set_mode(&mut self, to: Mode) {
        if self.mode == to {
            return;
        }
        let from = self.mode;
        self.mode = to;
        if to == Mode::Inventory {
            // The bag can shrink while the screen is shut, so the remembered
            // selection is pulled back inside it rather than trusted.
            inventory::clamp_selection(&self.world, &mut self.screen);
        }
        self.events.push(GameEvent::ModeChanged { from, to });
    }

    /// Open the conversation `graph_id` names, if there is one.
    ///
    /// Silent about an id the table does not define: load-time validation and
    /// `tests/data.rs` are where a typo in content is caught, and the
    /// simulation's job is to keep running. The `Interacted` event has already
    /// been emitted by then, so a trace still shows the key was pressed.
    fn begin_dialogue(&mut self, graph_id: &str) {
        let Some(graph) = self.dialogues.get(graph_id).cloned() else {
            return;
        };
        let Some(talk) = dialogue::open(&self.world, &self.flags, graph) else {
            return;
        };
        self.conversation = Some(talk);
        self.events.push(GameEvent::DialogueOpened {
            graph: graph_id.to_string(),
        });
        self.set_mode(Mode::Dialogue);
    }

    /// End whatever conversation is running and hand the world back.
    ///
    /// `DialogueClosed` before `ModeChanged`, because the conversation ending
    /// is the cause and the world restarting is the consequence — a trace reads
    /// in that order.
    fn end_dialogue(&mut self) {
        if let Some(talk) = self.conversation.take() {
            self.events.push(GameEvent::DialogueClosed {
                graph: talk.graph_id().to_string(),
            });
        }
        self.set_mode(Mode::Playing);
    }

    /// One tick of the world.
    ///
    /// Decide, move, react — in that order, and with movement as one pass over
    /// every body. New systems slot into the phase they belong to rather than
    /// being appended here, which is what keeps this readable as the game
    /// grows past one entity.
    ///
    /// **Geometry is rebuilt exactly once per tick**, by
    /// [`body::rebuild_geometry`] on the line before `body::move_bodies`:
    /// after every controller has decided and before anything has moved. A
    /// system that owns a collider — a fire on a cycle, a moving platform —
    /// therefore belongs in the decide phase, and its collider is picked up
    /// the same tick; [`crate::systems::hazard::tick_schedules`] is the first
    /// of those. Everything from that line to the end of movement sees one
    /// unchanging world; `avatar::control` and `npc::think` probe the geometry
    /// as it stood when they ran, which is the previous rebuild.
    ///
    /// **[`mover::advance`] is the last thing before that rebuild**, and its
    /// position in this list is a decision, not an accident. Moving platforms
    /// have to be somewhere new *before* the geometry is rebuilt, or a rider
    /// would collide against last tick's rect and sink; they have to carry
    /// their riders *before* `move_bodies`, so that a ride and the rider's own
    /// velocity are resolved by one pass; and they have to run *after*
    /// `avatar::control`, so that pressing down to drop through a one-way
    /// platform is visible here as `Body::ignore_one_way` on the tick it was
    /// pressed. The reasoning in full is in [`crate::systems::mover`]; the
    /// tests that fail if this line moves are named there.
    ///
    /// The rebuild covers hazards as well as solids, which is why
    /// `avatar::after_move` is handed the geometry rather than the level: a
    /// spike and a lit fire are one set of boxes by the time anything dies to
    /// either.
    fn step_playing(&mut self, input: PlayerInput) {
        // Opening a modal screen is decided before anything moves, so the tick
        // it happens on is already a frozen one. That is what makes a detour
        // through the inventory cost the world exactly nothing: the same
        // number of ticks are spent running it either way.
        if input.pressed(Action::Inventory) {
            self.set_mode(Mode::Inventory);
            return;
        }

        // Talking is decided in the same place and for the same reason, off
        // `prompt` — which was worked out in the resolve phase of the previous
        // tick, from where the bodies actually ended it. Pressing the key with
        // nothing in reach does nothing at all and emits nothing, so a trace
        // never has to be read to find out whether a press meant anything.
        if input.pressed(Action::Interact) {
            if let Some(prompt) = self.prompt.clone() {
                self.events.push(GameEvent::Interacted {
                    target: prompt.target.label().to_string(),
                });
                match &prompt.target {
                    InteractTarget::Dialogue(graph) => self.begin_dialogue(graph),
                }
                return;
            }
        }

        // Impact freeze: hold everything for a few ticks after a hit lands.
        // The tick counter still advances, so traces and tapes stay aligned
        // with wall-clock time and a frozen frame is visible as one.
        if self.hitstop > 0 {
            self.hitstop -= 1;
            return;
        }

        // Past every early return, so the world genuinely runs from here: this
        // is the tick its geometry will be current for once the decide phase
        // below has placed everything. Recorded rather than worked out later —
        // see the field.
        self.decided_tick = self.tick;

        // Combat timers first, so a controller asking "am I stunned?" reads
        // this tick's answer rather than last tick's.
        combat::tick_timers(&mut self.world);
        combat::advance_attacks(&mut self.world, &self.attacks, &mut self.events);
        // Mana, cooldowns and casts count with the other timers — and a cast
        // that reaches its release tick puts its projectile into the world
        // here, before `move_bodies`, so a bolt travels on the tick it appears
        // rather than hanging in the air for one.
        spell::advance_casts(&mut self.world, &self.spells);

        // Geometry that owns itself decides here, with the controllers: a fire
        // lights or goes out, a swinging hazard moves to where this tick's
        // angle puts it, and the rebuild below picks both up this tick. Only
        // `mover::advance` needs a more particular slot, because only it has
        // riders to carry.
        crate::systems::hazard::tick_schedules(&mut self.world, self.tick);
        crate::systems::pendulum::advance(&mut self.world, self.tick);

        avatar::control(
            &mut self.world,
            &self.level,
            &self.geometry,
            &self.attacks,
            &self.spells,
            input,
            TICK,
            &mut self.events,
        );
        npc::think(&mut self.world, &self.geometry, &mut self.events);

        // Last decision of the tick: platforms go where this tick puts them and
        // take their riders with them. Ordered here deliberately — see above.
        mover::advance(&mut self.world, self.tick);

        // The one point in the tick where the world's geometry changes.
        body::rebuild_geometry(&mut self.geometry, &self.world);
        body::move_bodies(&mut self.world, &self.geometry, TICK);
        avatar::after_move(
            &mut self.world,
            &self.level,
            &self.geometry,
            input,
            &mut self.events,
        );

        // Hitboxes are tested once everything has finished moving, so a swing
        // connects where the bodies actually ended the tick.
        combat::resolve(&mut self.world, &self.attacks, &mut self.events);
        // Beside it, and after it: a bolt is a hitbox that happens to have
        // been travelling, and it is tested where it actually ended the tick.
        // Both are done before `settle_dead`, so whatever either of them
        // killed stops moving on the tick it died.
        spell::resolve_projectiles(&mut self.world, &self.geometry, &mut self.events);
        // Beside them, and tested at final positions for the same reason:
        // walking over something is contact, and contact is decided once
        // everything has stopped moving.
        inventory::collect_pickups(&mut self.world, &mut self.events);
        // Standing next to something is the same kind of question as walking
        // over it, so it is answered in the same place: after everything has
        // moved, from the boxes as they finally are.
        self.update_prompt();
        combat::settle_dead(&mut self.world);
        // On the tick the corpse died, so a kill and its drops are one frame
        // of the trace rather than two.
        inventory::drop_loot(&mut self.world, &mut self.rng);
        if self
            .events
            .iter()
            .any(|e| matches!(e, GameEvent::Damaged { .. }))
        {
            self.hitstop = HITSTOP_TICKS;
        }

        animation::select_avatar_clip(&mut self.world, &self.attacks, &self.spells);
        animation::select_patrol_clip(&mut self.world);
        animation::advance(&mut self.world, TICK);
    }

    /// Work out what is in reach, announcing it when the answer changes.
    ///
    /// On the *change* rather than every tick, for the reason
    /// [`GameEvent::InventoryFull`] fires once per approach: standing beside a
    /// villager for ten seconds is six hundred ticks, and six hundred identical
    /// events is not a trace. Walking from one NPC to another announces the
    /// second, because the entity offered changed.
    fn update_prompt(&mut self) {
        let found = dialogue::nearest_interactable(&self.world);
        let changed = found.as_ref().map(|p| p.entity) != self.prompt.as_ref().map(|p| p.entity);
        if changed {
            if let Some(prompt) = &found {
                self.events.push(GameEvent::InteractPrompted {
                    target: prompt.target.label().to_string(),
                });
            }
        }
        self.prompt = found;
    }

    /// What happened during the most recent [`Sim::step`].
    pub fn events(&self) -> &[GameEvent] {
        &self.events
    }

    /// NPC entities in spawn order, which is map order.
    ///
    /// Sorted by entity id rather than taken in query order. hecs iterates
    /// archetypes in creation order, so the moment anything adds or removes a
    /// component at runtime — a stun, a damage flash — query order reshuffles.
    /// `npc.0` in a tape has to keep meaning the same knight, so the ordering
    /// is made explicit here instead of being inherited from iteration luck.
    pub fn npcs(&self) -> Vec<hecs::Entity> {
        let mut entities: Vec<hecs::Entity> = self
            .world
            .query::<&Patrol>()
            .iter()
            .map(|(entity, _)| entity)
            .collect();
        entities.sort_by_key(|entity| entity.id());
        entities
    }

    /// Snapshot every NPC, in spawn order, for tracing and assertions.
    pub fn npc_probes(&self) -> Vec<NpcProbe> {
        self.npcs()
            .into_iter()
            .filter_map(|entity| {
                let mut query = self
                    .world
                    .query_one::<(
                        &Kind,
                        &Patrol,
                        &Position,
                        &Velocity,
                        &Body,
                        &Health,
                        &AnimationState,
                    )>(entity)
                    .ok()?;
                let (kind, patrol, pos, vel, body, health, anim) = query.get()?;
                Some(NpcProbe {
                    kind: kind.0.clone(),
                    x: pos.0.x,
                    y: pos.0.y,
                    vx: vel.0.x,
                    vy: vel.0.y,
                    dir: patrol.dir,
                    grounded: body.grounded,
                    hp: health.current,
                    hitstun: health.hitstun,
                    dead: health.dead(),
                    clip: anim.clip.clone(),
                    frame: anim.frame,
                })
            })
            .collect()
    }

    /// Every NPC's position, in spawn order.
    pub fn npc_positions(&self) -> Vec<Vec2> {
        self.npcs()
            .into_iter()
            .map(|entity| {
                self.world
                    .get::<&Position>(entity)
                    .expect("npc has a position")
                    .0
            })
            .collect()
    }

    /// Everything the player is carrying or wearing, for tracing and
    /// assertions as `item.<id>.count` and `item.<id>.equipped`.
    ///
    /// Nested in the frame rather than flattened into the probe, the way
    /// `npcs` is: a bag is a list, and a run with an empty one serializes
    /// exactly as it did before items existed.
    ///
    /// Equipped items appear even once they have left the bag — otherwise
    /// `item.knight_helm.equipped` would stop resolving at the moment it
    /// became true.
    pub fn item_probes(&self) -> Vec<ItemProbe> {
        let Some(holder) = inventory::holder(&self.world) else {
            return Vec::new();
        };
        let Ok(bag) = self.world.get::<&Inventory>(holder) else {
            return Vec::new();
        };
        let gear = self.world.get::<&Equipment>(holder).ok();

        // Bag order first — which is the order the screen lists it in, and the
        // order things were picked up — then whatever is worn but no longer
        // carried, in slot order. Both are stable; neither is hecs's.
        let mut probes: Vec<ItemProbe> = bag
            .slots
            .iter()
            .map(|stack| ItemProbe {
                id: stack.id.clone(),
                count: stack.count,
                equipped: gear.as_ref().is_some_and(|gear| gear.holds(&stack.id)),
            })
            .collect();

        if let Some(gear) = &gear {
            for id in gear.slots.values() {
                if probes.iter().any(|probe| &probe.id == id) {
                    continue;
                }
                probes.push(ItemProbe {
                    id: id.clone(),
                    count: 0,
                    equipped: true,
                });
            }
        }
        probes
    }

    /// Snapshot the player's state for tracing and assertions.
    pub fn probe(&self) -> Probe {
        #[allow(clippy::type_complexity)]
        let mut query = self.world.query::<(
            &Avatar,
            &Body,
            &Health,
            &Attacking,
            &Position,
            &Velocity,
            &AnimationState,
            Option<&Mana>,
            Option<&Casting>,
            Option<&Inventory>,
        )>();
        let (_, (avatar, body, health, attacking, pos, vel, anim, mana, casting, bag)) =
            query.iter().next().expect("sim has no avatar to probe");
        Probe::new(
            self.tick,
            self.mode,
            avatar,
            body,
            health,
            attacking,
            pos.0,
            vel.0,
            anim,
            mana,
            casting,
            // Counts, not a probe each: what a tape wants to say is "a bolt
            // existed" and "the kill dropped something", and a line per entity
            // per tick makes a trace unreadable long before it makes it
            // informative.
            spell::projectile_count(&self.world),
            inventory::pickup_count(&self.world),
            bag.map_or(0, |bag| bag.used_slots()),
            &self.screen,
            self.prompt.as_ref(),
            self.conversation.as_ref(),
        )
    }

    /// Things that must be true at the end of every tick. Debug builds only,
    /// so this costs nothing in a release game — but during development it
    /// turns silent visual weirdness ("the knight sank into the floor") into
    /// a loud failure naming the exact tick.
    #[cfg(debug_assertions)]
    fn check_invariants(&self) {
        for (_, (pos, vel, size, sprite, anim)) in self
            .world
            .query::<(&Position, &Velocity, &Size, &Sprite, &AnimationState)>()
            .iter()
        {
            assert!(
                pos.0.is_finite() && vel.0.is_finite(),
                "tick {}: non-finite state (pos {:?}, vel {:?})",
                self.tick,
                pos.0,
                vel.0
            );

            // A missing clip freezes the sprite silently; catch it here
            // instead of wondering why the animation stopped. Checked against
            // the entity's own clip set, since they no longer share one.
            assert!(
                sprite.clips.clip(&anim.clip).is_some(),
                "tick {}: animation clip `{}` is not defined in this entity's clip set",
                self.tick,
                anim.clip
            );

            // The player should never end a tick meaningfully inside a solid.
            // Resting contact is not penetration, hence the tolerance.
            let body = Aabb::new(pos.0.x, pos.0.y, size.0.x, size.0.y);
            for solid in self.geometry.rects() {
                if solid.one_way {
                    continue;
                }
                assert!(
                    !penetrates(&body, &solid.rect, PENETRATION_TOLERANCE),
                    "tick {}: avatar {:?} is inside solid {:?}",
                    self.tick,
                    body,
                    solid.rect
                );
            }
        }
    }
}

/// Placeholder clips for [`Sim::fixture`]: every clip any selector can ask
/// for, each two frames at 10 fps on a sheet that does not exist.
///
/// One set serves every entity kind, since nothing headless reads the pixels.
/// `Sim::check_invariants` fails loudly if a selector ever reaches a name that
/// is missing here, which is what keeps the clip lists honest.
/// The placeholder clip set, registered under every kind a map can place.
pub(crate) fn fixture_clip_sets() -> HashMap<String, Arc<ClipSet>> {
    let stub = Arc::new(fixture_clips());
    std::iter::once("player")
        .chain(spawn::KINDS.iter().copied())
        .map(|kind| (kind.to_string(), stub.clone()))
        .collect()
}

pub(crate) fn fixture_clips() -> ClipSet {
    // Spell clips are read out of the shipped table rather than listed here:
    // a spell names the clip its caster plays and the clip its projectile is
    // drawn with, and hard-coding either in Rust is the seam this whole
    // arrangement exists to avoid. Adding a spell needs no change to this.
    let spells = SpellTable::shipped();
    let spell_clips: Vec<String> = spells
        .ids()
        .into_iter()
        .filter_map(|id| spells.get(id))
        .flat_map(|def| {
            let SpellEffect::Projectile { clip, .. } = &def.effect;
            [def.clip.clone(), clip.clone()]
        })
        .collect();

    let clips = animation::AVATAR_CLIPS
        .iter()
        .chain(animation::PATROL_CLIPS)
        .map(|&name| name.to_string())
        .chain(spell_clips)
        .map(|name| {
            (
                name,
                Clip {
                    frames: vec![(0, 0), (1, 0)],
                    fps: 10.0,
                    looping: true,
                    sheet: None,
                    frame_size: None,
                },
            )
        })
        .collect();
    ClipSet {
        sheet: Some("fixture".to_string()),
        frame_size: Some((50.0, 37.0)),
        offset: None,
        clips,
    }
}

/// Overlap tolerance for the penetration invariant, in pixels. Resting on a
/// surface puts the edges exactly equal, and a single resolution pass can
/// leave sub-pixel residue in corners; anything deeper is a real bug.
#[cfg(debug_assertions)]
const PENETRATION_TOLERANCE: f32 = 1.0;

/// Strict overlap, shrunk by `eps` on every side. Unlike `Aabb::overlaps`,
/// merely touching does not count.
#[cfg(debug_assertions)]
fn penetrates(a: &Aabb, b: &Aabb, eps: f32) -> bool {
    a.x + eps < b.right()
        && a.right() - eps > b.x
        && a.y + eps < b.bottom()
        && a.bottom() - eps > b.y
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::event::DeathCause;
    use crate::systems::input::Action;

    /// The player's real numbers, read from `assets/data/stats.ron`. Tests
    /// assert against these rather than against literals, so a tuning pass
    /// moves one file and not twenty assertions.
    fn player_stats() -> Arc<crate::assets::StatBlock> {
        StatTable::shipped().get("player").unwrap()
    }

    /// A flat floor with a pit on the right.
    fn test_sim() -> Sim {
        let mut level = LevelData::empty(20, 10, 32.0);
        level.solids = vec![Aabb::new(0.0, 256.0, 400.0, 64.0)];
        level.player_spawn = Vec2::new(100.0, 256.0 - player_stats().height);
        Sim::new(
            level,
            &fixture_clip_sets(),
            Assets::new().attacks().unwrap(),
            StatTable::shipped(),
            rng::DEFAULT_SEED,
        )
    }

    const JUMP: PlayerInput = PlayerInput::from_actions(&[Action::Jump]);

    /// Step until `pred` holds, returning the tick it happened on. Fixture
    /// tests care about "did this happen", not about counting ticks by hand.
    fn step_until(
        sim: &mut Sim,
        limit: u32,
        input: PlayerInput,
        pred: impl Fn(&Sim) -> bool,
    ) -> u32 {
        for tick in 0..limit {
            sim.step(input);
            if pred(sim) {
                return tick;
            }
        }
        panic!("condition never held within {limit} ticks");
    }

    #[test]
    fn steps_advance_the_tick_counter() {
        let mut sim = test_sim();
        assert_eq!(sim.probe().tick, 0);
        sim.step(PlayerInput::default());
        sim.step(PlayerInput::default());
        assert_eq!(sim.probe().tick, 2);
    }

    #[test]
    fn player_settles_on_the_floor_and_idles() {
        let mut sim = test_sim();
        for _ in 0..10 {
            sim.step(PlayerInput::default());
        }
        let probe = sim.probe();
        assert!(probe.grounded);
        assert_eq!(probe.clip, "idle");
        assert_eq!(probe.y, 256.0 - player_stats().height);
    }

    #[test]
    fn running_right_moves_and_switches_to_the_run_clip() {
        let mut sim = test_sim();
        for _ in 0..10 {
            sim.step(PlayerInput::default());
        }
        let start_x = sim.probe().x;

        let run = PlayerInput::from_actions(&[Action::Right]);
        for _ in 0..30 {
            sim.step(run);
        }
        let probe = sim.probe();
        assert!(probe.x > start_x);
        assert!(probe.facing_right);
        assert_eq!(probe.clip, "run");
    }

    /// The point of the fixture builder: a level, a player, and a working sim
    /// in three lines, with the geometry visible right there in the test.
    #[test]
    fn a_fixture_grid_builds_a_playable_sim() {
        let mut sim = Sim::fixture(&["..........", "..P.......", "##########"]);
        step_until(&mut sim, 30, PlayerInput::default(), |s| s.probe().grounded);

        let probe = sim.probe();
        assert_eq!(probe.clip, "idle");
        // spawn cell is row 1, so the floor surface is at y = 64
        assert_eq!(probe.y, 64.0 - player_stats().height);
        assert_eq!(
            sim.level.solids.len(),
            1,
            "the floor row merged into one rect"
        );
    }

    #[test]
    fn fixture_grids_carry_platforms_and_hazards() {
        let sim = Sim::fixture(&["..........", "..P..==...", ".....^^...", "##########"]);
        assert_eq!(sim.level.one_way.len(), 1);
        assert_eq!(sim.level.hazards.len(), 1);
    }

    /// Geometry an entity owns is geometry: it stops the player like a tile
    /// does, it appears the tick the entity does, and it is gone the tick the
    /// entity is. Everything M7 places — fire, moving platforms, swinging
    /// hazards — is this test with a system driving the position.
    #[test]
    fn an_entity_collider_blocks_the_player_and_stops_when_it_goes_away() {
        use crate::ecs::components::Collider;

        let run = PlayerInput::from_actions(&[Action::Right]);
        let mut sim = Sim::fixture(&["................", "..P.............", "################"]);
        step_until(&mut sim, 30, PlayerInput::default(), |s| s.probe().grounded);
        let statics = sim.geometry.rects().len();

        // A wall drops in mid-run, well to the player's right.
        let wall = sim.world.spawn((
            Position(Vec2::new(250.0, 0.0)),
            Collider::solid(Vec2::new(32.0, 64.0)),
        ));

        for _ in 0..120 {
            sim.step(run);
        }
        assert_eq!(
            sim.geometry.entity_rect_count(),
            1,
            "the wall joined the geometry"
        );
        assert_eq!(sim.geometry.rects().len(), statics + 1);
        assert_eq!(
            sim.probe().x + player_stats().width,
            250.0,
            "ran into the wall and stopped flush against it"
        );

        // Take it away and the same run carries straight through.
        sim.world.despawn(wall).unwrap();
        for _ in 0..120 {
            sim.step(run);
        }
        assert_eq!(sim.geometry.entity_rect_count(), 0);
        assert!(
            sim.probe().x > 250.0,
            "walked through where the wall used to be, ended at {}",
            sim.probe().x
        );
    }

    // --- events ---------------------------------------------------------

    #[test]
    fn jumping_and_landing_emit_events() {
        let mut sim = Sim::fixture(&["..........", "..........", "..P.......", "##########"]);
        step_until(&mut sim, 30, PlayerInput::default(), |s| s.probe().grounded);

        sim.step(JUMP);
        assert_eq!(sim.events(), [GameEvent::Jumped]);

        // ...and the landing, some ticks later, is its own event
        step_until(&mut sim, 200, PlayerInput::default(), |s| {
            !s.events().is_empty()
        });
        assert_eq!(sim.events(), [GameEvent::Landed { on_one_way: false }]);
    }

    /// Events are transient: they belong to the tick they happened on and are
    /// gone the next tick. That is the whole reason they are recorded into the
    /// trace rather than sampled from state.
    #[test]
    fn events_do_not_survive_into_the_next_tick() {
        let mut sim = Sim::fixture(&["..........", "..P.......", "##########"]);
        step_until(&mut sim, 30, PlayerInput::default(), |s| s.probe().grounded);

        sim.step(JUMP);
        assert!(!sim.events().is_empty());
        sim.step(PlayerInput::default());
        assert!(sim.events().is_empty(), "the jump event was cleared");
    }

    #[test]
    fn a_hazard_death_names_its_cause() {
        let mut sim = Sim::fixture(&["..........", "..P.......", "..^.......", "##########"]);
        // the spike is directly below the spawn cell; falling onto it kills
        step_until(&mut sim, 60, PlayerInput::default(), |s| s.probe().dead);
        assert_eq!(
            sim.events(),
            [GameEvent::Died {
                who: "player".to_string(),
                cause: DeathCause::Hazard
            }]
        );

        step_until(&mut sim, 120, PlayerInput::default(), |s| {
            s.events().contains(&GameEvent::Respawned)
        });
    }

    /// `npc.<n>` in a tape has to keep meaning the same entity for the whole
    /// run. hecs iterates archetypes in creation order, so adding or removing
    /// a component moves an entity between archetypes and reshuffles query
    /// order — which combat will do constantly, with stuns and damage flashes.
    ///
    /// This mutates components mid-run and checks the ordering survives. The
    /// guard is `Sim::npcs` sorting by entity id; delete that sort and this
    /// fails.
    #[test]
    fn npc_order_survives_components_being_added_and_removed() {
        let mut sim = Sim::fixture(&["................", "..P..K....K...K.", "################"]);

        let original = sim.npcs();
        assert_eq!(original.len(), 3, "three knights, one per K");
        let spawn_xs: Vec<f32> = sim.npc_positions().iter().map(|p| p.x).collect();
        assert!(
            spawn_xs.windows(2).all(|w| w[0] < w[1]),
            "spawn order should run left to right across the grid: {spawn_xs:?}"
        );

        // Give the middle knight a component, which moves it to a different
        // archetype, then take it away again.
        #[derive(Clone, Copy, Debug)]
        struct Stunned;

        sim.world.insert_one(original[1], Stunned).unwrap();
        assert_eq!(
            sim.npcs(),
            original,
            "order held while a component was added"
        );

        for _ in 0..30 {
            sim.step(PlayerInput::default());
        }
        assert_eq!(sim.npcs(), original, "order held across a step");

        sim.world.remove_one::<Stunned>(original[1]).unwrap();
        assert_eq!(sim.npcs(), original, "order held once it was removed again");

        // And the probes still line up with the entities they describe.
        let kinds: Vec<String> = sim.npc_probes().into_iter().map(|n| n.kind).collect();
        assert_eq!(kinds, vec!["knight"; 3]);
    }

    // --- the clock, and the world it drives -------------------------------

    /// Where the platform on a map is standing, straight out of the world.
    fn platform_x(sim: &Sim) -> f32 {
        sim.world
            .query::<(&Position, &crate::ecs::components::Mover)>()
            .iter()
            .map(|(_, (pos, _))| pos.0.x)
            .next()
            .expect("this map places a platform")
    }

    /// The world's clock and the run's clock are two different numbers, and
    /// hitstop is what makes them differ: nothing is decided during a freeze, so
    /// a moving platform stands still through it while the tick counter does
    /// not.
    ///
    /// This is what a save has to reproduce, at every point of the freeze **and
    /// on the tick after it ends** — which is the case no arithmetic on `tick`
    /// and `hitstop` can reach, because by then `hitstop` is back to 0 and says
    /// nothing about the four ticks the world sat out.
    ///
    /// Through the real `save`/`load_save` round trip rather than through
    /// [`Sim::resume_at`] directly, so that a capture which stopped writing one
    /// of the three numbers down fails here too. It is here rather than in
    /// `tests/save.rs` because getting a run into a freeze needs a blow to land
    /// and no fixture map has both a knight and a platform — `hitstop` is
    /// reached for by hand instead, which only this module can do.
    #[test]
    fn a_freeze_holds_the_world_still_while_the_clock_runs_on() {
        let map = "maps/testbed_mover.ron";
        let reload = |sim: &Sim| {
            Sim::load_save(&mut Assets::new(), &sim.save().expect("this run has a map"))
                .expect("a save of it loads")
        };
        let mut frozen = Sim::load(&mut Assets::new(), map).expect("the mover testbed loads");
        for _ in 0..100 {
            frozen.step(PlayerInput::default());
        }
        let held = platform_x(&frozen);
        assert_eq!(frozen.decided_tick, frozen.tick - 1, "live, so one behind");

        // A blow lands. The world holds for `HITSTOP_TICKS` while the clock does
        // not, and a save taken anywhere in there has to come back to exactly
        // this world.
        let world_stands_at = frozen.decided_tick;
        frozen.hitstop = HITSTOP_TICKS;
        for elapsed in 1..=HITSTOP_TICKS {
            frozen.step(PlayerInput::default());
            assert_eq!(
                platform_x(&frozen),
                held,
                "{elapsed} ticks in: the platform moved during the freeze",
            );
            assert_eq!(frozen.decided_tick, world_stands_at);
            assert_eq!(frozen.hitstop, HITSTOP_TICKS - elapsed);

            let reloaded = reload(&frozen);
            assert_eq!(
                platform_x(&reloaded),
                held,
                "{elapsed} ticks in (hitstop {}): the platform came back somewhere else",
                frozen.hitstop,
            );
            assert_eq!(reloaded.hitstop, frozen.hitstop);
            assert_eq!(reloaded.decided_tick, frozen.decided_tick);
        }

        // The last of those ticks left the sim in the state no arithmetic can
        // read: `hitstop` is back to 0, the world is still `HITSTOP_TICKS`
        // behind the clock, and nothing but `decided_tick` says so. A save taken
        // here is indistinguishable — from `tick` and `hitstop` alone — from one
        // taken in a run that has not been hit for a minute, and the two want
        // their platforms four ticks apart.
        assert_eq!(frozen.hitstop, 0);
        assert_eq!(
            frozen.tick - frozen.decided_tick,
            u64::from(HITSTOP_TICKS) + 1,
        );
        let mut reloaded = reload(&frozen);
        assert_eq!(
            platform_x(&reloaded),
            held,
            "the tick a freeze ends on: the platform came back somewhere else",
        );

        // ...and the first live tick snaps the platform across the whole freeze
        // in one step, which is the behaviour `crate::systems::mover` documents
        // and the reason every delta above had to be right. Both runs make the
        // same jump, because both were standing in the same place.
        frozen.step(PlayerInput::default());
        reloaded.step(PlayerInput::default());
        assert_ne!(platform_x(&frozen), held, "the world never started again");
        assert_eq!(platform_x(&reloaded), platform_x(&frozen));
        assert_eq!(
            frozen.decided_tick,
            frozen.tick - 1,
            "caught up in one tick"
        );
    }

    /// The same inputs must always produce the same trace, or tapes and
    /// trace diffing are worthless.
    #[test]
    fn simulation_is_deterministic() {
        let run = PlayerInput::holding(&[Action::Right, Action::Jump]);
        let trace = |_| {
            let mut sim = test_sim();
            let mut probes = Vec::new();
            for tick in 0..120 {
                let mut input = run;
                input.set_pressed(Action::Jump, tick == 20);
                sim.step(input);
                probes.push(sim.probe());
            }
            probes
        };
        assert_eq!(trace(()), trace(()));
    }

    // --- casting ----------------------------------------------------------

    /// The shipped `shock`, which every number below is read from rather than
    /// written out: this is a test of the mechanism, not of the balance.
    fn shock() -> crate::assets::SpellDef {
        crate::assets::SpellTable::shipped()
            .get("shock")
            .expect("assets/data/spells.ron defines `shock`")
            .clone()
    }

    const CAST: PlayerInput = PlayerInput::from_actions(&[Action::Cast]);

    fn casting_sim() -> Sim {
        let mut sim = Sim::fixture(&["..........", "..P.......", "##########"]);
        step_until(&mut sim, 30, PlayerInput::default(), |s| s.probe().grounded);
        sim
    }

    /// The whole of C-1 in one run: a cast spends, a second is refused, the
    /// cooldown expires on schedule, and the pool refills to exactly full.
    #[test]
    fn a_cast_spends_mana_and_the_cooldown_gates_the_next_one() {
        let spell = shock();
        let mut sim = casting_sim();
        let full = sim.probe().mana;
        assert_eq!(full, sim.probe().mana_max, "starts with a full pool");

        sim.step(CAST);
        assert_eq!(sim.probe().mana, full - spell.cost, "paid up front");
        assert!(sim.probe().casting);
        assert_eq!(sim.probe().cast_cooldown, spell.cooldown);
        assert_eq!(
            sim.events(),
            [GameEvent::SpellCast {
                spell: "shock".to_string()
            }]
        );

        // Immediately again: refused, and it says why rather than doing
        // nothing observable.
        let spent = sim.probe().mana;
        sim.step(PlayerInput::default());
        sim.step(CAST);
        assert_eq!(sim.probe().mana, spent, "a refusal costs nothing");
        assert_eq!(
            sim.events(),
            [GameEvent::CastFailed {
                spell: "shock".to_string(),
                reason: crate::sim::event::CastFailure::Busy,
            }]
        );

        // Wait the cooldown out and it goes off again.
        step_until(&mut sim, 600, PlayerInput::default(), |s| {
            s.probe().cast_cooldown == 0 && !s.probe().casting
        });
        sim.step(CAST);
        assert!(
            sim.events().contains(&GameEvent::SpellCast {
                spell: "shock".to_string()
            }),
            "the cooldown lapsed and the cast went through"
        );
    }

    #[test]
    fn casting_on_an_empty_pool_fails_for_want_of_mana_and_does_not_animate() {
        let mut sim = casting_sim();
        let spell = shock();

        // Drain the pool: cast whenever it is legal to, until it is not.
        let mut drained = false;
        for _ in 0..2000 {
            sim.step(CAST);
            let probe = sim.probe();
            if probe.mana < spell.cost && !probe.casting && probe.cast_cooldown == 0 {
                drained = true;
                break;
            }
        }
        assert!(drained, "never managed to empty the pool");

        let before = sim.probe();
        sim.step(CAST);
        let after = sim.probe();
        assert_eq!(after.mana, before.mana, "spent nothing");
        assert!(!after.casting, "and did not animate");
        assert_ne!(after.clip, "cast");
        assert_eq!(
            sim.events(),
            [GameEvent::CastFailed {
                spell: "shock".to_string(),
                reason: crate::sim::event::CastFailure::NoMana,
            }]
        );
    }

    /// Regeneration reaches the maximum and stops there — no overflow, and
    /// nothing banked for the next spend.
    #[test]
    fn mana_regenerates_to_full_and_no_further() {
        let mut sim = casting_sim();
        let max = sim.probe().mana_max;
        sim.step(CAST);
        assert!(sim.probe().mana < max);

        step_until(&mut sim, 3000, PlayerInput::default(), |s| {
            s.probe().mana == max
        });
        for _ in 0..600 {
            sim.step(PlayerInput::default());
            assert_eq!(sim.probe().mana, max, "never past the maximum");
        }
    }

    /// A cast puts a bolt into the world at its release tick, and the bolt is
    /// counted in the probe rather than given a probe of its own.
    #[test]
    fn a_cast_releases_exactly_one_projectile() {
        let mut sim = Sim::fixture(&[
            "....................",
            "..P.................",
            "####################",
        ]);
        step_until(&mut sim, 30, PlayerInput::default(), |s| s.probe().grounded);
        assert_eq!(sim.probe().projectiles, 0);

        sim.step(CAST);
        let released = step_until(&mut sim, 120, PlayerInput::default(), |s| {
            s.probe().projectiles > 0
        });
        assert_eq!(sim.probe().projectiles, 1, "one bolt, not one per tick");
        assert_eq!(
            released + 1,
            shock().cast_ticks,
            "released on the tick the spell says"
        );
    }

    // --- the plunge -------------------------------------------------------

    /// Down+attack in the air is the plunge; attack alone is still the air
    /// attack; down+attack on the ground is still the ground combo. Three
    /// meanings for one button, and the new one must not have eaten either of
    /// the two that were there first.
    #[test]
    fn down_attack_means_three_different_things() {
        let attack = PlayerInput::from_actions(&[Action::Attack]);
        let down_attack = PlayerInput::from_actions(&[Action::Down, Action::Attack]);
        let stats = player_stats();
        let mv = stats.avatar();

        // On the ground, with down held: the combo opener.
        let mut sim = casting_sim();
        sim.step(down_attack);
        assert!(!sim.probe().plunging);
        assert_eq!(
            sim.events(),
            [GameEvent::Attacked {
                attack: stats.attack.clone()
            }]
        );

        // In the air, without down: the air attack.
        let mut sim = casting_sim();
        sim.step(JUMP);
        for _ in 0..6 {
            sim.step(PlayerInput::default());
        }
        sim.step(attack);
        assert!(!sim.probe().plunging);
        assert_eq!(
            sim.events(),
            [GameEvent::Attacked {
                attack: mv.air_attack.clone()
            }]
        );

        // In the air, with down: the plunge — which hangs first, so the
        // attack itself does not start on the press.
        let mut sim = casting_sim();
        sim.step(JUMP);
        for _ in 0..6 {
            sim.step(PlayerInput::default());
        }
        sim.step(down_attack);
        assert!(sim.probe().plunging);
        assert_eq!(sim.probe().clip, "plunge_ready");
        assert!(sim.events().is_empty(), "the swing waits for the drop");
        assert_eq!(sim.probe().vy, 0.0, "hanging");
    }

    /// The three clips play in order, the loop actually loops on the way down,
    /// and the impact roots the player when the ground arrives.
    #[test]
    fn a_long_plunge_cycles_its_loop_and_ends_on_the_impact() {
        let mv = player_stats().avatar().clone();
        // A tall shaft: enough fall for the loop to come round more than once.
        let mut grid = vec!["#..................#"; 18];
        grid[1] = "#.P................#";
        grid.push("####################");
        let mut sim = Sim::fixture(&grid);
        let down_attack = PlayerInput::from_actions(&[Action::Down, Action::Attack]);
        let down = PlayerInput::holding(&[Action::Down]);

        // Plunge from the spawn rather than from a jump: the point of this
        // test is a *long* drop, and a jump from the floor of a shaft only
        // ever falls back the height of the jump.
        sim.step(PlayerInput::default());
        assert!(
            !sim.probe().grounded,
            "still on the way down from the spawn"
        );
        sim.step(down_attack);
        assert_eq!(sim.probe().clip, "plunge_ready");

        // Hover, then drop.
        let mut clips: Vec<String> = vec![sim.probe().clip.clone()];
        let mut loop_frames: Vec<usize> = Vec::new();
        for _ in 0..400 {
            sim.step(down);
            let probe = sim.probe();
            // Stop the moment the plunge lets go, before the ordinary
            // movement clips start again.
            if !probe.plunging {
                break;
            }
            if clips.last() != Some(&probe.clip) {
                clips.push(probe.clip.clone());
            }
            if probe.clip == "plunge_loop" {
                loop_frames.push(probe.frame);
            }
        }

        assert_eq!(
            clips,
            vec!["plunge_ready", "plunge_loop", "plunge_impact"],
            "the three clips play once each, in order"
        );
        // The fixture's placeholder clips are two frames; a loop that loops
        // returns to frame 0 after reaching the last one.
        let wrapped = loop_frames.windows(2).any(|w| w[0] > 0 && w[1] == 0);
        assert!(
            wrapped,
            "the loop never came round in {} ticks of falling: {loop_frames:?}",
            loop_frames.len()
        );
        assert!(sim.probe().grounded);
        assert!(
            loop_frames.len() > mv.plunge_hover_ticks as usize,
            "the drop should be the long part of a plunge from this height, \
             but it lasted only {} ticks",
            loop_frames.len()
        );
    }

    /// Being hit drops the plunge, exactly as it drops a swing — otherwise a
    /// player knocked out of the air keeps a live hitbox under their feet.
    #[test]
    fn a_hit_cancels_a_plunge() {
        use crate::ecs::components::Health;

        let mut sim = casting_sim();
        let down_attack = PlayerInput::from_actions(&[Action::Down, Action::Attack]);
        sim.step(JUMP);
        for _ in 0..6 {
            sim.step(PlayerInput::default());
        }
        sim.step(down_attack);
        assert!(sim.probe().plunging);

        let player = sim
            .world
            .query::<&Avatar>()
            .iter()
            .map(|(e, _)| e)
            .next()
            .expect("the fixture has a player");
        sim.world.get::<&mut Health>(player).unwrap().hitstun = 20;
        sim.step(PlayerInput::default());
        assert!(!sim.probe().plunging, "knocked out of it");
        assert!(!sim.probe().attacking);
    }

    // --- modes ------------------------------------------------------------

    const OPEN_BAG: PlayerInput = PlayerInput::from_actions(&[Action::Inventory]);

    /// A sim mid-run: on the ground, moving, with an animation part-way
    /// through — so that "nothing changed" is a claim about something.
    fn running_sim() -> Sim {
        let mut sim = Sim::fixture(&[
            "....................",
            "..P.................",
            "####################",
        ]);
        step_until(&mut sim, 30, PlayerInput::default(), |s| s.probe().grounded);
        for _ in 0..20 {
            sim.step(PlayerInput::holding(&[Action::Right]));
        }
        sim
    }

    #[test]
    fn the_inventory_key_opens_and_closes_the_screen_and_says_so() {
        let mut sim = running_sim();
        assert_eq!(sim.mode(), Mode::Playing);

        sim.step(OPEN_BAG);
        assert_eq!(sim.mode(), Mode::Inventory);
        assert_eq!(sim.probe().mode, "inventory");
        assert_eq!(
            sim.events(),
            [GameEvent::ModeChanged {
                from: Mode::Playing,
                to: Mode::Inventory
            }]
        );

        // Held rather than pressed: one press is one toggle, however long the
        // key stays down.
        for _ in 0..10 {
            sim.step(PlayerInput::holding(&[Action::Inventory]));
            assert_eq!(sim.mode(), Mode::Inventory);
        }

        sim.step(PlayerInput::default());
        sim.step(OPEN_BAG);
        assert_eq!(sim.mode(), Mode::Playing);
        assert_eq!(
            sim.events(),
            [GameEvent::ModeChanged {
                from: Mode::Inventory,
                to: Mode::Playing
            }]
        );
    }

    #[test]
    fn cancel_also_closes_the_screen() {
        let mut sim = running_sim();
        sim.step(OPEN_BAG);
        sim.step(PlayerInput::from_actions(&[Action::Cancel]));
        assert_eq!(sim.mode(), Mode::Playing);
    }

    /// The tick counter is the one thing that keeps moving, exactly as it does
    /// through hitstop — so a trace stays aligned with wall-clock time and a
    /// menu shows up as the run of identical frames it is.
    #[test]
    fn steps_advance_the_tick_counter_in_every_mode() {
        let mut sim = running_sim();
        let before = sim.probe().tick;
        sim.step(OPEN_BAG);
        for _ in 0..60 {
            sim.step(PlayerInput::default());
        }
        assert_eq!(sim.probe().tick, before + 61);
    }

    /// The whole of I-1's acceptance criterion, checked exhaustively rather
    /// than field by field: enter, step sixty ticks, leave, and every single
    /// thing the probe can see is where it was. `tick` is exempt because it
    /// must move; `mode` is exempt because it is the thing that moved.
    ///
    /// A comparison written out as a list of fields would silently stop
    /// covering whatever the probe gained next. Serializing both and blanking
    /// the two exempt keys cannot.
    #[test]
    fn a_modal_mode_freezes_every_probe_field_but_the_tick() {
        let mut sim = running_sim();

        let flatten = |sim: &Sim| {
            let frame = trace::Frame {
                probe: sim.probe(),
                npcs: sim.npc_probes(),
                items: sim.item_probes(),
                flags: sim.flags().clone(),
                events: Vec::new(),
                seed: None,
            };
            let mut value: serde_json::Value = serde_json::to_value(&frame).unwrap();
            let map = value.as_object_mut().unwrap();
            map.remove("tick");
            map.remove("mode");
            value
        };

        let before = flatten(&sim);
        sim.step(OPEN_BAG);
        assert_eq!(
            flatten(&sim),
            before,
            "opening the screen moved something on the tick it happened"
        );

        for tick in 0..60 {
            sim.step(PlayerInput::default());
            assert_eq!(
                flatten(&sim),
                before,
                "the world moved on frozen tick {tick}"
            );
        }

        sim.step(OPEN_BAG);
        assert_eq!(sim.mode(), Mode::Playing);
        assert_eq!(flatten(&sim), before, "leaving the screen moved something");
    }

    /// ...and the consequence that matters to a player: a detour through the
    /// inventory does not cost the run a single tick of world time. Two sims,
    /// one of which stops to look in its bag, and identical traces at the end.
    #[test]
    fn a_detour_through_the_inventory_costs_the_world_nothing() {
        let run = PlayerInput::holding(&[Action::Right]);

        let mut plain = running_sim();
        for _ in 0..40 {
            plain.step(run);
        }

        let mut detoured = running_sim();
        for _ in 0..20 {
            detoured.step(run);
        }
        detoured.step(OPEN_BAG);
        for _ in 0..90 {
            detoured.step(PlayerInput::default());
        }
        detoured.step(OPEN_BAG);
        for _ in 0..20 {
            detoured.step(run);
        }

        let (a, b) = (plain.probe(), detoured.probe());
        assert_eq!(a.x, b.x, "the detour cost the run distance");
        assert_eq!((a.y, a.vx, a.vy), (b.y, b.vx, b.vy));
        assert_eq!((a.clip.as_str(), a.frame), (b.clip.as_str(), b.frame));
        assert_ne!(a.tick, b.tick, "but the clock did keep running");
    }

    /// Nothing enters dialogue mode yet, and it must not be a trap for the
    /// ticket that does.
    #[test]
    fn dialogue_mode_exists_and_can_be_left() {
        let mut sim = running_sim();
        sim.set_mode(Mode::Dialogue);
        assert_eq!(sim.probe().mode, "dialogue");
        let frozen = sim.probe().x;
        for _ in 0..30 {
            sim.step(PlayerInput::holding(&[Action::Right]));
        }
        assert_eq!(sim.probe().x, frozen, "the world is frozen in every mode");
        sim.step(PlayerInput::from_actions(&[Action::Cancel]));
        assert_eq!(sim.mode(), Mode::Playing);
    }

    // --- randomness -----------------------------------------------------

    /// The property every tape and golden trace rests on: the seed is the
    /// whole of what the randomness depends on.
    #[test]
    fn two_sims_with_the_same_seed_roll_the_same_numbers() {
        let sample = |seed| {
            let mut sim = Sim::fixture(&["..........", "..P.......", "##########"]);
            sim.reseed(seed);
            (0..32).map(|_| sim.rng.next_u64()).collect::<Vec<u64>>()
        };
        assert_eq!(sample(rng::DEFAULT_SEED), sample(rng::DEFAULT_SEED));
        assert_ne!(sample(rng::DEFAULT_SEED), sample(99));
    }

    #[test]
    fn a_sim_starts_at_the_default_seed() {
        let sim = Sim::fixture(&["..........", "..P.......", "##########"]);
        assert_eq!(sim.seed(), rng::DEFAULT_SEED);
    }

    /// Loot rolls are the first and so far only draw from the generator, and
    /// they happen on the tick something dies. A run in which nothing dies
    /// must therefore consume nothing — which is why adding loot moved no
    /// golden trace but `knight_kill`'s, and why a future system that rolled
    /// dice every tick would be a change worth arguing about rather than one
    /// that slipped in.
    #[test]
    fn stepping_the_sim_consumes_no_randomness_unless_something_dies() {
        let mut sim = Sim::fixture(&["..........", "..P.......", "##########"]);
        let before = sim.rng.clone();
        for _ in 0..60 {
            sim.step(JUMP);
        }
        assert_eq!(sim.rng, before, "a system started rolling dice");
    }
}
