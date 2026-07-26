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
pub mod run;
pub mod tape;
pub mod trace;

use std::rc::Rc;

use ggez::glam::Vec2;
use hecs::World;

use crate::assets::{Assets, Clip, ClipSet};
use crate::ecs::components::{AnimationState, Avatar, Body, Position, Size, Sprite, Velocity};
use crate::level::LevelData;
use crate::physics::{Aabb, SolidRect};
use crate::systems::input::PlayerInput;
use crate::systems::{animation, avatar, body};

pub use event::GameEvent;
pub use probe::Probe;

/// The simulation runs on a fixed timestep so that behavior is identical at
/// any frame rate — and, just as importantly, reproducible between runs.
/// Determinism is what makes input tapes and trace diffing work.
pub const TICKS_PER_SECOND: u32 = 60;
pub const TICK: f32 = 1.0 / TICKS_PER_SECOND as f32;

pub struct Sim {
    pub world: World,
    pub level: LevelData,
    /// Solids + one-way platforms, pre-flattened for the physics pass.
    pub geometry: Vec<SolidRect>,
    pub clips: Rc<ClipSet>,
    /// Ticks elapsed since the sim was created.
    pub tick: u64,
    /// What happened during the most recent [`Sim::step`]. Cleared at the
    /// start of each tick, so this is never a running log — the tape runner
    /// and the trace are what accumulate.
    events: Vec<GameEvent>,
}

impl Sim {
    /// Load a map (path relative to the assets directory) and spawn the player.
    pub fn load(assets: &mut Assets, map: &str) -> anyhow::Result<Self> {
        let map_path = assets.base_dir().join(map);
        let level = LevelData::load(&map_path, assets)?;
        let clips = assets.clip_set("player")?;
        Ok(Self::new(level, clips))
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
        Sim::new(level, Rc::new(fixture_clips()))
    }

    pub fn new(level: LevelData, clips: Rc<ClipSet>) -> Self {
        let mut geometry: Vec<SolidRect> =
            level.solids.iter().map(|&r| SolidRect::solid(r)).collect();
        geometry.extend(level.one_way.iter().map(|&r| SolidRect::one_way(r)));

        let mut world = World::new();
        let spawn = level.player_spawn;
        let (fw, fh) = clips.frame_size;
        world.spawn((
            Avatar::new(),
            Avatar::body(spawn),
            Position(spawn),
            Velocity(Vec2::ZERO),
            Size(Vec2::new(Avatar::WIDTH, Avatar::HEIGHT)),
            Sprite {
                // sprite centered horizontally on the collider, feet aligned
                offset: Vec2::new((Avatar::WIDTH - fw) / 2.0, Avatar::HEIGHT - fh),
            },
            AnimationState::new("idle"),
        ));

        Sim {
            world,
            level,
            geometry,
            clips,
            tick: 0,
            events: Vec::new(),
        }
    }

    /// Advance one fixed tick.
    ///
    /// Decide, move, react — in that order, and with movement as one pass over
    /// every body. New systems slot into the phase they belong to rather than
    /// being appended here, which is what keeps this readable as the game
    /// grows past one entity.
    pub fn step(&mut self, input: PlayerInput) {
        self.events.clear();
        avatar::control(
            &mut self.world,
            &self.level,
            &self.geometry,
            input,
            TICK,
            &mut self.events,
        );
        body::move_bodies(&mut self.world, &self.geometry, TICK);
        avatar::after_move(&mut self.world, &self.level, input, &mut self.events);
        animation::select_avatar_clip(&mut self.world);
        animation::advance(&mut self.world, &self.clips, TICK);
        self.tick += 1;

        #[cfg(debug_assertions)]
        self.check_invariants();
    }

    /// What happened during the most recent [`Sim::step`].
    pub fn events(&self) -> &[GameEvent] {
        &self.events
    }

    /// Snapshot the player's state for tracing and assertions.
    pub fn probe(&self) -> Probe {
        let mut query = self
            .world
            .query::<(&Avatar, &Body, &Position, &Velocity, &AnimationState)>();
        let (_, (avatar, body, pos, vel, anim)) =
            query.iter().next().expect("sim has no avatar to probe");
        Probe::new(self.tick, avatar, body, pos.0, vel.0, anim)
    }

    /// Things that must be true at the end of every tick. Debug builds only,
    /// so this costs nothing in a release game — but during development it
    /// turns silent visual weirdness ("the knight sank into the floor") into
    /// a loud failure naming the exact tick.
    #[cfg(debug_assertions)]
    fn check_invariants(&self) {
        for (_, (pos, vel, size, anim)) in self
            .world
            .query::<(&Position, &Velocity, &Size, &AnimationState)>()
            .iter()
        {
            assert!(
                pos.0.is_finite() && vel.0.is_finite(),
                "tick {}: non-finite avatar state (pos {:?}, vel {:?})",
                self.tick,
                pos.0,
                vel.0
            );

            // A missing clip freezes the sprite silently; catch it here
            // instead of wondering why the animation stopped.
            assert!(
                self.clips.clip(&anim.clip).is_some(),
                "tick {}: animation clip `{}` is not defined in the clip set",
                self.tick,
                anim.clip
            );

            // The player should never end a tick meaningfully inside a solid.
            // Resting contact is not penetration, hence the tolerance.
            let body = Aabb::new(pos.0.x, pos.0.y, size.0.x, size.0.y);
            for solid in &self.geometry {
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

/// Placeholder clips for [`Sim::fixture`]: every clip the avatar's selector
/// can ask for, each two frames at 10 fps on a sheet that does not exist.
/// Nothing headless reads the pixels, and `Sim::check_invariants` fails loudly
/// if the selector ever reaches a name that is missing here.
fn fixture_clips() -> ClipSet {
    let clips = animation::AVATAR_CLIPS
        .iter()
        .map(|&name| {
            (
                name.to_string(),
                Clip {
                    frames: vec![(0, 0), (1, 0)],
                    fps: 10.0,
                    looping: true,
                },
            )
        })
        .collect();
    ClipSet {
        sheet: "fixture".to_string(),
        frame_size: (50.0, 37.0),
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
    a.x + eps < b.right() && a.right() - eps > b.x && a.y + eps < b.bottom() && a.bottom() - eps > b.y
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::event::DeathCause;

    /// A flat floor with a pit on the right.
    fn test_sim() -> Sim {
        let mut level = LevelData::empty(20, 10, 32.0);
        level.solids = vec![Aabb::new(0.0, 256.0, 400.0, 64.0)];
        level.player_spawn = Vec2::new(100.0, 256.0 - Avatar::HEIGHT);
        Sim::new(level, Rc::new(fixture_clips()))
    }

    const JUMP: PlayerInput = PlayerInput {
        left: false,
        right: false,
        down: false,
        jump_pressed: true,
        jump_held: true,
    };

    /// Step until `pred` holds, returning the tick it happened on. Fixture
    /// tests care about "did this happen", not about counting ticks by hand.
    fn step_until(sim: &mut Sim, limit: u32, input: PlayerInput, pred: impl Fn(&Sim) -> bool) -> u32 {
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
        assert_eq!(probe.y, 256.0 - Avatar::HEIGHT);
    }

    #[test]
    fn running_right_moves_and_switches_to_the_run_clip() {
        let mut sim = test_sim();
        for _ in 0..10 {
            sim.step(PlayerInput::default());
        }
        let start_x = sim.probe().x;

        let run = PlayerInput {
            right: true,
            ..Default::default()
        };
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
        let mut sim = Sim::fixture(&[
            "..........",
            "..P.......",
            "##########",
        ]);
        step_until(&mut sim, 30, PlayerInput::default(), |s| s.probe().grounded);

        let probe = sim.probe();
        assert_eq!(probe.clip, "idle");
        // spawn cell is row 1, so the floor surface is at y = 64
        assert_eq!(probe.y, 64.0 - Avatar::HEIGHT);
        assert_eq!(sim.level.solids.len(), 1, "the floor row merged into one rect");
    }

    #[test]
    fn fixture_grids_carry_platforms_and_hazards() {
        let sim = Sim::fixture(&[
            "..........",
            "..P..==...",
            ".....^^...",
            "##########",
        ]);
        assert_eq!(sim.level.one_way.len(), 1);
        assert_eq!(sim.level.hazards.len(), 1);
    }

    // --- events ---------------------------------------------------------

    #[test]
    fn jumping_and_landing_emit_events() {
        let mut sim = Sim::fixture(&[
            "..........",
            "..........",
            "..P.......",
            "##########",
        ]);
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
        let mut sim = Sim::fixture(&[
            "..........",
            "..P.......",
            "..^.......",
            "##########",
        ]);
        // the spike is directly below the spawn cell; falling onto it kills
        step_until(&mut sim, 60, PlayerInput::default(), |s| s.probe().dead);
        assert_eq!(
            sim.events(),
            [GameEvent::Died {
                cause: DeathCause::Hazard
            }]
        );

        step_until(&mut sim, 120, PlayerInput::default(), |s| {
            s.events().contains(&GameEvent::Respawned)
        });
    }

    /// The same inputs must always produce the same trace, or tapes and
    /// trace diffing are worthless.
    #[test]
    fn simulation_is_deterministic() {
        let run = PlayerInput {
            right: true,
            jump_held: true,
            ..Default::default()
        };
        let trace = |_| {
            let mut sim = test_sim();
            let mut probes = Vec::new();
            for tick in 0..120 {
                let mut input = run;
                input.jump_pressed = tick == 20;
                sim.step(input);
                probes.push(sim.probe());
            }
            probes
        };
        assert_eq!(trace(()), trace(()));
    }
}
