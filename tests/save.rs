//! Save and load, checked the only way that catches the field nobody
//! remembered to save.
//!
//! Comparing a save's fields one by one tests the fields you thought of, which
//! is the same set you thought of while writing the save. So the test that
//! matters here does not look at fields at all: it plays a tape, saves half way
//! through, loads the save back through a file, and then runs *the same
//! remaining ticks* into both sims and diffs the traces. Identical, or the save
//! is lossy — and a forgotten timer shows up as a trace that drifts two ticks
//! later, naming the tick it drifted on.
//!
//! It runs on **maps that place no NPCs**, and that restriction is a
//! precondition rather than a preference: a save deliberately does not carry
//! NPCs, projectiles or items lying on the floor (see `src/save.rs` for why), so
//! an exact comparison is only *possible* where there are none of them to lose.
//! Every case asserts that rather than assuming it, and
//! `the_world_comes_back_at_the_maps_spawn_state` covers the other side: on a
//! map that *does* place a knight, the knight is deliberately back at its post.
//!
//! What it must **not** be is one map. For a whole milestone it ran only on
//! `maps/testbed.ron`, whose entity list is empty — no platform, no fire, no
//! swinging hazard, no knight — and the consequence was not that the test was
//! weak but that it was *blind*. Nothing on that map moves independently of the
//! player, so a load that rebuilt every tick-driven thing at tick 0 compared
//! equal; and nothing on it can land a blow, so `Sim::hitstop` was provably 0 at
//! all twelve save points and a save that dropped it compared equal too. Both
//! were real desyncs and neither could be seen from there. So the comparison is
//! a table now, one row per kind of thing a map can place that has a life of its
//! own, and `the_maps_these_are_checked_against_place_what_they_are_for` is the
//! guard that keeps the table honest.

use std::path::PathBuf;

use supergame::assets::{Assets, Slot};
use supergame::ecs::components::{
    Avatar, Equipment, Fire, Health, Inventory, Mana, Mover, Pendulum, Position,
};
use supergame::save::{FileStore, SaveStore};
use supergame::sim::tape::Tape;
use supergame::sim::trace::Trace;
use supergame::sim::{GameEvent, Probe, Sim};
use supergame::systems::hazard;
use supergame::systems::input::{Action, PlayerInput};

/// No entities, and geometry a tape can move around in without dying: the
/// floor under the spawn runs from x=0 to x=256 and the tape below stays on it.
const MAP: &str = "maps/testbed.ron";

/// A ferry over a spiked pit. The player spends two hundred ticks standing on
/// something that is somewhere new every tick, which is the case a load that
/// rebuilt the platform at tick 0 destroys.
const MOVER_MAP: &str = "maps/testbed_mover.ron";

/// Two fires on opposite phases: one crossed while it is out, one stood in
/// until it lights.
const FIRE_MAP: &str = "maps/testbed_fire.ron";

/// A ball on a chain: ducked under once, then walked into.
const SWING_MAP: &str = "maps/testbed_swing.ron";

/// A knight on a ledge, for the half of the design that is about *not* saving.
const KNIGHT_MAP: &str = "maps/testbed_knight.ron";

/// A knight within reach of a walk and a swing, for the half of the design that
/// is about impact freeze — which needs a blow to land, and so needs something
/// to land it on.
const ARENA_MAP: &str = "maps/testbed_arena.ron";

/// Everything a save has to survive, in one run: running, a jump, a landing, an
/// air swing, a ground combo, a slide, a cast, and enough idling that the
/// animation is part-way through a clip at most ticks.
///
/// The cast is deliberately at the very end, after every save point below. A
/// bolt in the air is one of the things a save does not carry, so a save taken
/// while one exists could not be compared exactly — the run casts *after* the
/// split so that both sims cast, out of the mana pool the load restored.
const TAPE: &str = "
    wait 2          # settle onto the floor
    right 30
    right+jump 1    # up
    right 18        # ...and along
    attack 1        # an air swing, mid-flight
    wait 14
    left 30         # turn around and run back
    attack 1        # a ground combo opener
    wait 10
    attack 1        # the second link, chained
    wait 24
    left+down+jump 1   # a slide
    wait 24
    right 24
    wait 20
    # every save point is somewhere in the ticks above

    cast 1          # from here on there is a bolt in the air
    wait 60
    left 20
    left+jump 1
    left 16
    attack 1
    wait 40
    right 30
    attack 1
    wait 40
";

/// The tick the tape casts on, found by running it. Anything at or after this
/// has a bolt in the air for a while, which is state a save does not carry.
const CAST_TICK: usize = 202;

/// Ticks to save at: mid-air, mid-swing, mid-slide, mid-turn, and standing
/// still. Every one of them is before the cast, so no save point has a
/// projectile in the air — which the test asserts rather than assumes.
const SAVE_POINTS: &[usize] = &[1, 12, 33, 40, 52, 66, 81, 96, 120, 135, 150, 175];

/// The crossing `tapes/platform_ride.tape` makes, without its assertions: walk
/// to the lip, wait for the ferry to dock, board it, ride it across with no
/// input at all, and step off onto the far ledge.
///
/// The two hundred ticks of `wait` are the whole point. Through all of them the
/// only thing moving the player is the platform under their feet, so their
/// position is a running total of the platform's displacement — and a load that
/// rebuilt the platform at `Mover::at(0)` hands the next tick a delta of its
/// entire travel and carries the player by it.
const RIDE_TAPE: &str = "
    wait 5
    right 20        # out to the lip of the near ledge
    wait 30         # ...and wait there for the ferry, which docks on tick 60
    right 26        # aboard, while it is docked
    wait 204        # the ride: no input at all
    right 60        # off onto the far ledge
";

/// Save points across the ride: one before boarding, one the tick after, five
/// spread through the crossing, and one after stepping off.
const RIDE_POINTS: &[usize] = &[40, 82, 110, 150, 190, 230, 270, 320];

/// `tapes/fire_timing.tape` without its assertions: past fire A while it is
/// out, on into fire B's cell, and stand there until B lights.
///
/// It ends in a death and a respawn on purpose. A fire is the one piece of
/// tick-driven geometry whose state is a *presence* rather than a position, and
/// dying to it is the only way that presence is ever observable.
const FIRE_TAPE: &str = "
    wait 5
    right 100       # through A's cell during the gap that opens on tick 60
    right 170       # on into B's cell, which is out when they arrive
    wait 40         # ...until it lights
    wait 40         # the death freeze, and the respawn
";

const FIRE_POINTS: &[usize] = &[20, 70, 100, 160, 230, 290];

/// `tapes/swing_dodge.tape` without its assertions: wait out one pass of the
/// ball, run under it at the end of its arc, then walk back into the middle of
/// the room and stand where the bottom of the swing is.
const SWING_TAPE: &str = "
    wait 95         # let one pass go by, so the run starts on a known tick
    right 180       # under the ball while it is out at the left of its arc
    left 84         # back into the middle of the room, and stop
    wait 20         # ...where the bottom of the swing is
    wait 40         # the death freeze, and the respawn
";

const SWING_POINTS: &[usize] = &[40, 120, 200, 260, 330, 380];

/// One run to save part way through: a map, the tape to play on it, and the
/// ticks to split at.
///
/// A table rather than a single map, because what this comparison can catch is
/// decided entirely by what the map places — see the module docs.
struct Case {
    /// The map. It must place no NPCs; every case asserts that.
    map: &'static str,
    tape: &'static str,
    save_points: &'static [usize],
    /// What this row is here to hold down, named in the failure message.
    about: &'static str,
}

const CASES: &[Case] = &[
    Case {
        map: MAP,
        tape: TAPE,
        save_points: SAVE_POINTS,
        about: "the player entity in full — every timer, pool and frame of it",
    },
    Case {
        map: MOVER_MAP,
        tape: RIDE_TAPE,
        save_points: RIDE_POINTS,
        about: "a moving platform, with the player standing on it",
    },
    Case {
        map: FIRE_MAP,
        tape: FIRE_TAPE,
        save_points: FIRE_POINTS,
        about: "two fires on opposite phases",
    },
    Case {
        map: SWING_MAP,
        tape: SWING_TAPE,
        save_points: SWING_POINTS,
        about: "a swinging hazard mid-arc",
    },
];

fn assets() -> Assets {
    Assets::new()
}

fn sim(map: &str) -> Sim {
    Sim::load(&mut assets(), map).unwrap_or_else(|e| panic!("`{map}` should load: {e:#}"))
}

fn tape() -> Tape {
    Tape::parse(TAPE).expect("the tape above parses")
}

/// A directory of this test's own under the target dir, emptied first.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Step `inputs` into `sim`, recording the frame the tape runner records.
///
/// The frame includes the flags, which is what makes the trace comparison
/// below cover them: a save that dropped a quest flag would show up as two
/// traces that differ on the `flags` key of every frame after the load.
fn play(sim: &mut Sim, inputs: &[PlayerInput]) -> Trace {
    let mut trace = Trace::new();
    for input in inputs {
        sim.step(*input);
        trace.push(
            sim.probe(),
            sim.npc_probes(),
            sim.item_probes(),
            sim.flags().clone(),
            sim.events(),
        );
    }
    trace
}

/// The player entity.
fn player(sim: &Sim) -> hecs::Entity {
    sim.world
        .query::<&Avatar>()
        .iter()
        .map(|(entity, _)| entity)
        .min_by_key(|entity| entity.id())
        .expect("the sim has a player")
}

/// Give the player something worth saving: a bag with two kinds in it, a helm
/// (which moves the derived maximum health) and a sword (which replaces the
/// combo opener and adds damage), two quest flags mid-run — and neither pool
/// full.
///
/// Done by hand rather than by killing something for the drops, because this
/// test is about the save and not about the loot table — and because the map it
/// runs on has nothing to kill.
///
/// The empty mana pool is not decoration. It leaves the pool *regenerating* for
/// the whole run, so `Mana::partial` — thousandths of a point toward the next
/// one, and the only saved field the probe cannot see — is part-way through at
/// almost every save point below. Without it a save could drop `partial` and
/// nothing would notice for another 50 ticks.
fn outfit_the_player(sim: &mut Sim) {
    let player = player(sim);
    let mut bag = sim.world.get::<&mut Inventory>(player).unwrap();
    assert!(bag.add("minor_potion", 3));
    assert!(bag.add("knight_helm", 1));
    drop(bag);

    let mut gear = sim.world.get::<&mut Equipment>(player).unwrap();
    // Inserted weapon-first on purpose: `Equipment` is a `BTreeMap`, so what
    // comes back must be in slot order whatever order it went in.
    gear.slots.insert(Slot::Weapon, "iron_sword".to_string());
    gear.slots.insert(Slot::Head, "knight_helm".to_string());
    drop(gear);

    sim.world.get::<&mut Health>(player).unwrap().current = 3;
    let mut mana = sim.world.get::<&mut Mana>(player).unwrap();
    mana.current = 0;
    mana.partial = 0;
    drop(mana);

    // Two quests part-way through, one of them at a stage that is neither the
    // start nor the end. `maps/testbed.ron` has nobody to talk to, so these are
    // set by hand — what is under test is the save, not the conversation that
    // would have set them; `tapes/quest_fetch.tape` is the other end of that.
    sim.set_flag("quest.helm.stage", 1);
    sim.set_flag("quest.draught.stage", 2);
}

// ---------------------------------------------------------------------------

/// The test the ticket asks for, at dozens of save points across four maps.
///
/// A save that forgets the jump buffer, the coyote timer, the animation frame,
/// the mana fraction, which way the player was facing or how many ticks of
/// impact freeze are left to run — or that rebuilds a moving platform at the
/// wrong tick — shows up here as two traces that diverge, at the tick it
/// started to matter.
#[test]
fn a_reloaded_run_steps_identically_to_one_that_was_never_saved() {
    let store = FileStore::new(scratch("save-replay"));

    for case in CASES {
        let inputs = Tape::parse(case.tape)
            .unwrap_or_else(|e| panic!("the tape for `{}` parses: {e:#}", case.map))
            .inputs();

        for &split in case.save_points {
            let at = format!("`{}` ({}), saved at tick {split}", case.map, case.about);
            assert!(split < inputs.len(), "{at}: past the end of the tape");

            let mut original = sim(case.map);

            // The exact comparison below is only *possible* where the save
            // loses nothing: NPCs are deliberately not carried, so a map that
            // places one could never match tick for tick however correct the
            // save was. Asserted rather than assumed, because the whole reason
            // this is a table is that nobody was checking what the one map it
            // used to run on actually contained.
            assert!(
                original.npc_probes().is_empty(),
                "{at}: this map places an NPC, and a save does not carry one — \
                 an exact trace comparison on it could never pass",
            );

            outfit_the_player(&mut original);
            play(&mut original, &inputs[..split]);

            // The other two things a save deliberately drops must not be
            // present either, or the comparison would be measuring the design
            // rather than the code.
            let probe = original.probe();
            assert_eq!(probe.projectiles, 0, "{at}: a bolt was in the air");
            assert_eq!(probe.pickups, 0, "{at}: an item was on the floor");

            store.write("slot1", &original.save().unwrap()).unwrap();
            let mut reloaded = Sim::load_save(&mut assets(), &store.read("slot1").unwrap())
                .unwrap_or_else(|e| panic!("{at}: should load: {e:#}"));

            // Before a single tick is stepped: the load landed on the same
            // frame.
            assert_eq!(
                reloaded.probe(),
                original.probe(),
                "{at}: the load did not start where the save left off",
            );
            assert_eq!(reloaded.item_probes(), original.item_probes());
            assert_eq!(
                reloaded.flags(),
                original.flags(),
                "{at}: the quest flags did not come back",
            );

            // ...and stays there for the whole rest of the run.
            let expected = play(&mut original, &inputs[split..]);
            let actual = play(&mut reloaded, &inputs[split..]);
            assert_eq!(
                first_difference(&expected, &actual),
                None,
                "{at}: the save changed the run that followed",
            );
        }
    }
}

/// The guard on the table above: each row has to place the thing it claims to,
/// and between them they have to cover every kind of geometry that is a
/// function of `Sim::tick`.
///
/// This is the test that would have caught the blind spot rather than the two
/// bugs hiding in it. `maps/testbed.ron` places none of the three, so for a
/// whole milestone the replay comparison proved that the *player entity* round
/// trips and nothing else at all.
#[test]
fn the_maps_these_are_checked_against_place_what_they_are_for() {
    // (platforms, fires, swinging hazards) a map places.
    let placed = |map: &str| {
        let sim = sim(map);
        (
            sim.level.movers.len(),
            sim.level.fires.len(),
            sim.level.pendulums.len(),
        )
    };

    assert_eq!(
        placed(MAP),
        (0, 0, 0),
        "`{MAP}` places nothing that moves on its own — which is exactly why it \
         cannot be the only map this is checked against",
    );
    assert!(placed(MOVER_MAP).0 > 0, "`{MOVER_MAP}` places no platform");
    assert!(placed(FIRE_MAP).1 > 0, "`{FIRE_MAP}` places no fire");
    assert!(
        placed(SWING_MAP).2 > 0,
        "`{SWING_MAP}` places no swinging hazard"
    );

    // ...and every row is a map with no NPCs on it, which is the precondition
    // the replay comparison asserts at every save point.
    for case in CASES {
        assert!(
            sim(case.map).npc_probes().is_empty(),
            "`{}` places an NPC and cannot be replayed exactly",
            case.map,
        );
    }
}

/// ...and the platform row has to be an actual *ride*, not a walk past a
/// platform that happens to be on the map. A tape that never boarded would
/// compare two runs neither of which touched the thing under test.
///
/// What is counted is the signature of rider carry: a tick on which the player
/// moved while standing still — no velocity of their own, feet on the ground,
/// and a different `x` from the tick before.
#[test]
fn the_platform_case_is_actually_a_ride() {
    let mut sim = sim(MOVER_MAP);
    let trace = play(&mut sim, &Tape::parse(RIDE_TAPE).unwrap().inputs());
    let frames = trace.frames();

    let carried = frames
        .windows(2)
        .filter(|w| w[1].probe.grounded && w[1].probe.vx == 0.0 && w[1].probe.x != w[0].probe.x)
        .count();
    assert!(
        carried > 150,
        "the player was carried on only {carried} ticks, so this tape is not a ride",
    );
    assert!(
        frames.iter().all(|f| !f.probe.dead),
        "the pit is spiked, so a run that fell in proves the opposite of a ride",
    );
    let airborne = frames.iter().filter(|f| !f.probe.grounded).count();
    assert!(
        airborne <= 2,
        "the crossing is walked onto and off rather than jumped, but the player \
         was off the ground for {airborne} ticks",
    );
}

/// The comparison above is only worth something if the run it compares actually
/// does things. This is the guard on that: the tape has to move the player,
/// leave the ground, swing, cast, and spend mana — otherwise a save that
/// carried nothing but a position would pass.
#[test]
fn the_tape_this_is_checked_with_exercises_something() {
    let mut sim = sim(MAP);
    outfit_the_player(&mut sim);
    let trace = play(&mut sim, &tape().inputs());
    let frames = trace.frames();

    let any = |f: fn(&supergame::sim::trace::Frame) -> bool| frames.iter().any(f);
    assert!(any(|f| !f.probe.grounded), "never left the ground");
    assert!(any(|f| f.probe.attacking), "never swung");
    assert!(any(|f| f.probe.casting), "never cast");
    assert!(any(|f| f.probe.projectiles > 0), "no bolt ever existed");
    assert!(any(|f| !f.probe.facing_right), "never turned around");
    assert!(any(|f| f.probe.mana < f.probe.mana_max), "never spent mana");
    assert!(
        frames.iter().all(|f| !f.probe.dead),
        "the tape is supposed to survive the map",
    );
    assert!(
        frames
            .iter()
            .map(|f| f.probe.clip.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            >= 4,
        "the run should play more than a couple of animations",
    );
}

/// The acceptance criterion in the ticket's own words: save, quit, load, and
/// the bag, the gear, the health, the mana and the quest flags are all still
/// there — through a file on disk, not through a struct held in memory.
#[test]
fn a_bag_gear_health_and_mana_survive_a_trip_through_a_file() {
    let store = FileStore::new(scratch("save-round-trip"));

    let mut original = sim(MAP);
    outfit_the_player(&mut original);
    play(&mut original, &tape().inputs()[..90]);

    // Spend some of both pools, so "it survived" is a claim about a number
    // that is neither full nor empty.
    {
        let player = player(&original);
        original.world.get::<&mut Health>(player).unwrap().current = 4;
        let mut mana = original.world.get::<&mut Mana>(player).unwrap();
        mana.current = 3;
        mana.partial = 750;
    }
    original.step(PlayerInput::default());

    store.write("slot1", &original.save().unwrap()).unwrap();

    // "Quit": nothing of the original sim is used from here on.
    let reloaded = Sim::load_save(&mut assets(), &store.read("slot1").unwrap()).unwrap();

    let probe = reloaded.probe();
    assert_eq!(probe.hp, 4, "health came back");
    assert_eq!(probe.hp_max, 7, "and the helm's +2 was derived, not stored");
    assert_eq!(probe.mana, 3, "mana came back");
    assert_eq!(probe.x, original.probe().x, "and so did the player");
    assert_eq!(probe.y, original.probe().y);
    assert_eq!(probe.tick, original.probe().tick, "and the clock");

    let entity = player(&reloaded);
    let bag = reloaded.world.get::<&Inventory>(entity).unwrap();
    assert_eq!(bag.count("minor_potion"), 3);
    assert_eq!(bag.count("knight_helm"), 1);
    assert_eq!(
        bag.capacity,
        original
            .world
            .get::<&Inventory>(player(&original))
            .unwrap()
            .capacity,
        "capacity is a stat, and comes from the stat block rather than the save",
    );

    let gear = reloaded.world.get::<&Equipment>(entity).unwrap();
    assert_eq!(gear.get(Slot::Head), Some("knight_helm"));
    assert_eq!(gear.get(Slot::Weapon), Some("iron_sword"));
    assert_eq!(
        gear.slots.keys().copied().collect::<Vec<_>>(),
        vec![Slot::Head, Slot::Weapon],
        "worn gear comes back in slot order — the order stat modifiers are \
         summed in, which is what keeps a reloaded run's floats identical",
    );
    assert_eq!(
        reloaded.rng, original.rng,
        "and the generator is where the run left it, seed and position both",
    );

    // M6's half of the criterion: quest progress is progress. A stage that
    // came back as 0 would mean a player who did the errand and reloaded is
    // asked to do it again.
    assert_eq!(reloaded.flag("quest.helm.stage"), 1);
    assert_eq!(reloaded.flag("quest.draught.stage"), 2);
    assert_eq!(reloaded.flags(), original.flags());
    assert_eq!(
        reloaded.flag("quest.nobody.set.this"),
        0,
        "and a flag nothing ever set still reads as zero rather than erroring",
    );
}

/// The deliberate omission, as a tested decision rather than a comment: the
/// world is rebuilt from the map, so a knight you fought is back at its post
/// and whole.
///
/// If this ever needs to change — a boss that stays dead, a door that stays
/// open — the answer is a quest flag in the save (M5/M6), not NPC positions.
#[test]
fn the_world_comes_back_at_the_maps_spawn_state() {
    let store = FileStore::new(scratch("save-world"));

    let mut original = sim(KNIGHT_MAP);
    let spawn = original.npc_positions();
    assert_eq!(spawn.len(), 1, "the fixture places one knight");

    // Let it walk its route, then hurt it.
    for _ in 0..120 {
        original.step(PlayerInput::default());
    }
    let knight = original.npcs()[0];
    original.world.get::<&mut Health>(knight).unwrap().current = 1;
    let moved = original.npc_positions();
    assert_ne!(moved, spawn, "the knight should have patrolled somewhere");

    store.write("slot1", &original.save().unwrap()).unwrap();
    let reloaded = Sim::load_save(&mut assets(), &store.read("slot1").unwrap()).unwrap();

    assert_eq!(
        reloaded.npc_positions(),
        spawn,
        "the knight is back where the map puts it",
    );
    let probes = reloaded.npc_probes();
    assert_eq!(probes.len(), 1, "and there is still exactly one of it");
    assert!(!probes[0].dead);
    assert_eq!(
        probes[0].hp,
        original.npc_probes()[0].hp + 5,
        "at full health again, not at the one point it was left on",
    );

    // The player, meanwhile, is exactly where the save left them.
    assert_eq!(reloaded.probe(), original.probe());
}

/// The other deliberate omission, likewise as a test: a bolt in the air is not
/// saved, so a load that happened mid-cast finds the air empty.
///
/// The reasoning is the knight's. A projectile is something the world is
/// *doing*, not something the player *has*; the alternative is a save file that
/// describes every entity in flight and can then disagree with the spell table
/// it was written against.
#[test]
fn a_bolt_in_the_air_is_not_saved() {
    let inputs = tape().inputs();
    let mut original = sim(MAP);
    // Past the cast, but not so far past that the bolt has expired.
    play(&mut original, &inputs[..CAST_TICK + 30]);
    assert_eq!(
        original.probe().projectiles,
        1,
        "the bolt should still be in flight here",
    );

    let reloaded = Sim::load_save(&mut assets(), &original.save().unwrap()).unwrap();
    assert_eq!(reloaded.probe().projectiles, 0, "and is gone after a load");
    // The cooldown it left behind is state on the caster, and does come back.
    assert_eq!(
        reloaded.probe().cast_cooldown,
        original.probe().cast_cooldown,
    );
}

/// Everything on a map that is somewhere — or something — different every
/// tick, in spawn order: where each platform and each ball is, whether each
/// fire is lit, and the collision world all three actually reach the game
/// through.
///
/// As text, because `SolidRect` is not `PartialEq` and because a failure here
/// wants to *say* what moved.
fn tick_driven_world(sim: &Sim) -> Vec<String> {
    let mut world: Vec<(u32, String)> = Vec::new();
    for (entity, (pos, _)) in sim.world.query::<(&Position, &Mover)>().iter() {
        world.push((entity.id(), format!("platform at {:?}", pos.0)));
    }
    for (entity, (pos, _)) in sim.world.query::<(&Position, &Pendulum)>().iter() {
        world.push((entity.id(), format!("ball at {:?}", pos.0)));
    }
    for (entity, (pos, _)) in sim.world.query::<(&Position, &Fire)>().iter() {
        world.push((
            entity.id(),
            format!(
                "fire at {:?}, lit {}",
                pos.0,
                hazard::is_lit(&sim.world, entity)
            ),
        ));
    }
    world.sort();

    let mut lines: Vec<String> = world.into_iter().map(|(_, line)| line).collect();
    // The boxes the next tick's controllers will probe, which `Sim::new` builds
    // from wherever it just put all three.
    lines.push(format!("solids {:?}", sim.geometry.rects()));
    lines.push(format!("hazards {:?}", sim.geometry.hazards()));
    lines
}

/// The claim `src/save.rs` makes for storing four numbers instead of a world:
/// fires, moving platforms and swinging hazards are pure functions of the tick,
/// so restoring the tick restores all three with no fields of their own.
///
/// It was not true. `Sim::load` rebuilds the map at tick 0 and the tick was put
/// back *afterwards*, so all three came back describing a run that was not
/// happening — and for a platform that is not cosmetic, because the next
/// `mover::advance` reads the difference between where the platform is and
/// where the tick says it should be, and hands that difference to whatever is
/// standing on it.
///
/// Checked here directly as well as through the replay comparison, because two
/// of the three are only *indirectly* observable: a load leaves a stale fire and
/// a stale ball in the geometry the next tick's controllers probe, and the tick
/// after that overwrites both. This is what fails the moment any of the three
/// stops being put back.
#[test]
fn the_tick_driven_world_comes_back_where_the_tick_says() {
    // Odd numbers, so none of them lands on a boundary of any of the cycles.
    for (map, ticks) in [(MOVER_MAP, 137), (FIRE_MAP, 91), (SWING_MAP, 211)] {
        let mut original = sim(map);
        assert!(
            tick_driven_world(&original).len() > 2,
            "`{map}` places nothing that the tick drives",
        );
        for _ in 0..ticks {
            original.step(PlayerInput::default());
        }

        let reloaded = Sim::load_save(&mut assets(), &original.save().unwrap())
            .unwrap_or_else(|e| panic!("the save from `{map}` should load: {e:#}"));
        assert_eq!(
            tick_driven_world(&reloaded),
            tick_driven_world(&original),
            "`{map}`: reloaded at tick {ticks}, the world the tick drives came \
             back somewhere else",
        );
    }
}

/// Impact freeze has to survive a save, which needs a map with something to hit
/// — the reason this is not on `maps/testbed.ron` with everything else.
///
/// A hit stops the world for four ticks. Dropping those from the save does not
/// lose four ticks of animation and stop there: the reloaded run resumes early,
/// and from then on every tick of it is a tick the never-saved run will not
/// reach for another four, forever.
///
/// The comparison is in two halves, because of what a save deliberately does
/// *not* carry. The first is against the never-saved run itself and is limited
/// to the freeze, which is the whole of what a lost `hitstop` gets wrong and the
/// only stretch in which the knight — back at its post after the load — cannot
/// make the two runs legitimately differ. The second is a full-trace diff, and
/// it is exact *including* the knight: a frozen world has not moved from the
/// spawn state a load put it in, so during the freeze, and only during the
/// freeze, saving again and reloading has to be a whole-trace no-op.
#[test]
fn a_save_taken_mid_impact_reloads_still_frozen() {
    let mut original = sim(ARENA_MAP);
    assert_eq!(
        original.npc_probes().len(),
        1,
        "the arena places one knight"
    );

    // Walk into the knight, swinging. Which tick connects falls out of reach,
    // chase speed and cooldown at once, so it is found rather than counted.
    let mut connected = false;
    for tick in 0..600 {
        let input = if tick % 15 == 0 {
            PlayerInput::from_actions(&[Action::Right, Action::Attack])
        } else {
            PlayerInput::holding(&[Action::Right])
        };
        original.step(input);
        if original
            .events()
            .iter()
            .any(|e| matches!(e, GameEvent::Damaged { .. }))
        {
            connected = true;
            break;
        }
    }
    assert!(
        connected,
        "never landed a blow, so there is no freeze to save"
    );

    // The save is taken on the tick after the blow, which is the first frozen
    // one — exactly the case the module docs used to argue was not worth
    // carrying.
    let state = original.save().unwrap();
    assert!(
        state.to_ron().unwrap().contains("hitstop"),
        "the save says nothing about the freeze it was taken in the middle of",
    );

    let mut reloaded = Sim::load_save(&mut assets(), &state).unwrap();
    assert_eq!(reloaded.probe(), original.probe(), "the load landed wrong");

    // Frozen means frozen: nothing about the player changes but the clock, and
    // the reloaded run has to be just as still for just as long. One tick is
    // enough to catch a run that resumed early — the swing advances, the
    // i-frames count down and the animation moves on — and the freeze is walked
    // out in full so that "just as long" is checked too.
    let mut frozen = 0u32;
    loop {
        let before = original.probe();
        original.step(PlayerInput::default());
        if original.probe() != frame_after(&before) {
            // The never-saved run has just taken a live tick, so the freeze is
            // over. From here the knight — back at its post in the reloaded run
            // — is free to make the two differ for reasons that are the design
            // rather than a bug.
            break;
        }

        let before = reloaded.probe();
        reloaded.step(PlayerInput::default());
        assert_eq!(
            reloaded.probe(),
            frame_after(&before),
            "{frozen} ticks after the save, the reloaded run resumed early",
        );
        assert_eq!(
            reloaded.probe(),
            original.probe(),
            "{frozen} ticks after the save, the reloaded run was somewhere else",
        );
        frozen += 1;
        assert!(frozen < 60, "the world never started again");
    }
    assert!(
        frozen >= 2,
        "only {frozen} frozen ticks, so this proved almost nothing",
    );

    // ...and it starts again on the same tick, rather than staying frozen for
    // longer than the run it came from.
    let before = reloaded.probe();
    reloaded.step(PlayerInput::default());
    assert_ne!(
        reloaded.probe(),
        frame_after(&before),
        "the reloaded run was still frozen after the never-saved one had resumed",
    );

    // The full-trace half. `a` is the loaded run one frozen tick in; `b` is `a`
    // saved and loaded again. Because `a` has not taken a live tick, its world
    // is still the spawn state a load produces, so the two are the same run in
    // every respect a trace records — knight, events and all.
    let mut a = Sim::load_save(&mut assets(), &state).unwrap();
    a.step(PlayerInput::default());
    let mut b = Sim::load_save(&mut assets(), &a.save().unwrap()).unwrap();

    let rest = vec![PlayerInput::default(); 150];
    assert_eq!(
        first_difference(&play(&mut a, &rest), &play(&mut b, &rest)),
        None,
        "saving again during the freeze changed the run that followed",
    );
}

/// The same probe one tick later with nothing having happened: only the clock
/// moved. Used to tell a frozen tick from a live one without reaching into
/// `Sim` for a counter that is nobody else's business.
fn frame_after(probe: &Probe) -> Probe {
    let mut next = probe.clone();
    next.tick += 1;
    next
}

/// Where two traces first differ, as the tick and the fields that moved. A raw
/// "these differ" would be useless for a test whose whole job is to point at
/// the field somebody forgot.
fn first_difference(expected: &Trace, actual: &Trace) -> Option<String> {
    let (expected, actual) = (expected.to_jsonl(), actual.to_jsonl());
    if expected == actual {
        return None;
    }

    let expected: Vec<&str> = expected.lines().collect();
    let actual: Vec<&str> = actual.lines().collect();
    let Some(index) = expected.iter().zip(&actual).position(|(e, a)| e != a) else {
        return Some(format!(
            "the run ended early: {} ticks against {}",
            expected.len(),
            actual.len()
        ));
    };

    let mut report = format!("first difference {} ticks after the save", index + 1);
    let parse = |line: &str| serde_json::from_str::<serde_json::Value>(line).ok();
    if let (Some(e), Some(a)) = (parse(expected[index]), parse(actual[index])) {
        if let (Some(e), Some(a)) = (e.as_object(), a.as_object()) {
            let mut keys: Vec<&String> = e.keys().chain(a.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                if e.get(key) != a.get(key) {
                    report.push_str(&format!(
                        "\n      {key}: never saved {} -> reloaded {}",
                        e.get(key).map_or("(absent)".into(), |v| v.to_string()),
                        a.get(key).map_or("(absent)".into(), |v| v.to_string()),
                    ));
                }
            }
            return Some(report);
        }
    }
    report.push_str(&format!(
        "\n      never saved: {}\n      reloaded:    {}",
        expected[index], actual[index]
    ));
    Some(report)
}
