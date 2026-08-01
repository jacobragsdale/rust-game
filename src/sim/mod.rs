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

use std::collections::{hash_map, HashMap};
use std::sync::Arc;

use anyhow::Context as _;
use ggez::glam::Vec2;
use hecs::World;

use crate::assets::{Assets, AttackTable, Clip, ClipSet, StatTable};
use crate::ecs::components::{
    AnimationState, Attacking, Avatar, Body, Health, Kind, Patrol, Position, Size, Sprite, Velocity,
};
use crate::ecs::spawn;
use crate::level::LevelData;
use crate::physics::{Aabb, Geometry};
use crate::sim::rng::Rng;
use crate::systems::input::PlayerInput;
use crate::systems::{animation, avatar, body, combat, npc};

pub use event::GameEvent;
pub use probe::{NpcProbe, Probe};
pub use rng::DEFAULT_SEED;

/// The simulation runs on a fixed timestep so that behavior is identical at
/// any frame rate — and, just as importantly, reproducible between runs.
/// Determinism is what makes input tapes and trace diffing work.
pub const TICKS_PER_SECOND: u32 = 60;
pub const TICK: f32 = 1.0 / TICKS_PER_SECOND as f32;

/// How long the world freezes when a hit lands. Four ticks is about 65 ms —
/// long enough to register as impact, short enough not to read as a hitch.
const HITSTOP_TICKS: u32 = 4;

pub struct Sim {
    pub world: World,
    pub level: LevelData,
    /// Everything a body can collide with: the level's solids and one-way
    /// platforms, plus whatever colliders entities own this tick. Rebuilt at
    /// one point per tick — see [`Sim::step`].
    pub geometry: Geometry,
    /// Every attack's timing and effect, shared by every attacker.
    pub attacks: Arc<AttackTable>,
    /// Every kind's movement and combat numbers. `spawn` hands each entity the
    /// block for its kind as a `Stats` component; this is kept so that
    /// anything spawned later in the run can be given one too.
    pub stats: Arc<StatTable>,
    /// The simulation's only source of randomness, seeded per run.
    ///
    /// Anything random — a loot roll, a crit, a spell's spread — draws from
    /// here and from nowhere else. A tape and its golden trace are only worth
    /// anything if the same seed replays the same run, and an unseeded
    /// generator anywhere would silently end that.
    pub rng: Rng,
    /// Ticks elapsed since the sim was created.
    pub tick: u64,
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
        Ok(Self::new(
            level,
            &clip_sets,
            attacks,
            stats,
            rng::DEFAULT_SEED,
        ))
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
    pub fn new(
        level: LevelData,
        clip_sets: &HashMap<String, Arc<ClipSet>>,
        attacks: Arc<AttackTable>,
        stats: Arc<StatTable>,
        seed: u64,
    ) -> Self {
        let geometry = Geometry::from_level(&level.solids, &level.one_way);

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

        let mut sim = Sim {
            world,
            level,
            geometry,
            attacks,
            stats,
            rng: Rng::new(seed),
            tick: 0,
            hitstop: 0,
            events: Vec::new(),
        };
        // Colliders the map placed exist from tick 0, not from the first
        // rebuild inside `step` — otherwise the first tick's controllers probe
        // a world with holes in it.
        body::rebuild_geometry(&mut sim.geometry, &sim.world);
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

    /// Advance one fixed tick.
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
    /// the same tick. Everything from that line to the end of movement sees
    /// one unchanging world; `avatar::control` and `npc::think` probe the
    /// geometry as it stood when they ran, which is the previous rebuild.
    pub fn step(&mut self, input: PlayerInput) {
        self.events.clear();

        // Impact freeze: hold everything for a few ticks after a hit lands.
        // The tick counter still advances, so traces and tapes stay aligned
        // with wall-clock time and a frozen frame is visible as one.
        if self.hitstop > 0 {
            self.hitstop -= 1;
            self.tick += 1;
            return;
        }

        // Combat timers first, so a controller asking "am I stunned?" reads
        // this tick's answer rather than last tick's.
        combat::tick_timers(&mut self.world);
        combat::advance_attacks(&mut self.world, &self.attacks, &mut self.events);

        avatar::control(
            &mut self.world,
            &self.level,
            &self.geometry,
            &self.attacks,
            input,
            TICK,
            &mut self.events,
        );
        npc::think(&mut self.world, &self.geometry, &mut self.events);

        // The one point in the tick where the world's geometry changes.
        body::rebuild_geometry(&mut self.geometry, &self.world);
        body::move_bodies(&mut self.world, &self.geometry, TICK);
        avatar::after_move(&mut self.world, &self.level, input, &mut self.events);

        // Hitboxes are tested once everything has finished moving, so a swing
        // connects where the bodies actually ended the tick.
        combat::resolve(&mut self.world, &self.attacks, &mut self.events);
        combat::settle_dead(&mut self.world);
        if self
            .events
            .iter()
            .any(|e| matches!(e, GameEvent::Damaged { .. }))
        {
            self.hitstop = HITSTOP_TICKS;
        }

        animation::select_avatar_clip(&mut self.world, &self.attacks);
        animation::select_patrol_clip(&mut self.world);
        animation::advance(&mut self.world, TICK);
        self.tick += 1;

        #[cfg(debug_assertions)]
        self.check_invariants();
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

    /// Snapshot the player's state for tracing and assertions.
    pub fn probe(&self) -> Probe {
        let mut query = self.world.query::<(
            &Avatar,
            &Body,
            &Health,
            &Attacking,
            &Position,
            &Velocity,
            &AnimationState,
        )>();
        let (_, (avatar, body, health, attacking, pos, vel, anim)) =
            query.iter().next().expect("sim has no avatar to probe");
        Probe::new(
            self.tick, avatar, body, health, attacking, pos.0, vel.0, anim,
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
    let clips = animation::AVATAR_CLIPS
        .iter()
        .chain(animation::PATROL_CLIPS)
        .map(|&name| {
            (
                name.to_string(),
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

    /// Nothing draws from the generator yet, which is exactly why the golden
    /// traces did not move when it was added. Once something does, delete
    /// this — it is a statement about today, not an invariant.
    #[test]
    fn stepping_the_sim_consumes_no_randomness() {
        let mut sim = Sim::fixture(&["..........", "..P.......", "##########"]);
        let before = sim.rng.clone();
        for _ in 0..60 {
            sim.step(JUMP);
        }
        assert_eq!(sim.rng, before, "a system started rolling dice");
    }
}
