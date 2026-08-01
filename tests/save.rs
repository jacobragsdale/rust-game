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
//! It runs on `maps/testbed.ron`, which places no entities. That is not
//! incidental: a save deliberately does not carry NPCs, projectiles or items
//! lying on the floor (see `src/save.rs` for why), so an exact comparison is
//! only meaningful where there are none of them to lose. Each save point
//! asserts that precondition rather than assuming it, and
//! `the_world_comes_back_at_the_maps_spawn_state` covers the other side: on a
//! map that *does* place a knight, the knight is deliberately back at its post.

use std::path::PathBuf;

use supergame::assets::{Assets, Slot};
use supergame::ecs::components::{Avatar, Equipment, Health, Inventory, Mana};
use supergame::save::{FileStore, SaveStore};
use supergame::sim::tape::Tape;
use supergame::sim::trace::Trace;
use supergame::sim::Sim;
use supergame::systems::input::PlayerInput;

/// No entities, and geometry a tape can move around in without dying: the
/// floor under the spawn runs from x=0 to x=256 and the tape below stays on it.
const MAP: &str = "maps/testbed.ron";

/// A knight on a ledge, for the half of the design that is about *not* saving.
const KNIGHT_MAP: &str = "maps/testbed_knight.ron";

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
fn play(sim: &mut Sim, inputs: &[PlayerInput]) -> Trace {
    let mut trace = Trace::new();
    for input in inputs {
        sim.step(*input);
        trace.push(
            sim.probe(),
            sim.npc_probes(),
            sim.item_probes(),
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
/// combo opener and adds damage) — and neither pool full.
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
}

// ---------------------------------------------------------------------------

/// The test the ticket asks for, at a dozen different save points.
///
/// A save that forgets the jump buffer, the coyote timer, the animation frame,
/// the mana fraction or which way the player was facing shows up here as two
/// traces that diverge, at the tick it started to matter.
#[test]
fn a_reloaded_run_steps_identically_to_one_that_was_never_saved() {
    let inputs = tape().inputs();
    let store = FileStore::new(scratch("save-replay"));

    for &split in SAVE_POINTS {
        assert!(split < inputs.len(), "save point {split} is past the tape");

        let mut original = sim(MAP);
        outfit_the_player(&mut original);
        play(&mut original, &inputs[..split]);

        // The things a save deliberately drops must not be present, or an
        // exact comparison would be measuring the design rather than the code.
        let probe = original.probe();
        assert_eq!(
            probe.projectiles, 0,
            "a bolt was in the air at tick {split}"
        );
        assert_eq!(probe.pickups, 0, "an item was on the floor at tick {split}");

        store.write("slot1", &original.save().unwrap()).unwrap();
        let mut reloaded = Sim::load_save(&mut assets(), &store.read("slot1").unwrap())
            .unwrap_or_else(|e| panic!("the save from tick {split} should load: {e:#}"));

        // Before a single tick is stepped: the load landed on the same frame.
        assert_eq!(
            reloaded.probe(),
            original.probe(),
            "the sim loaded from a save at tick {split} did not start where it was saved",
        );
        assert_eq!(reloaded.item_probes(), original.item_probes());

        // ...and stays there for the whole rest of the run.
        let expected = play(&mut original, &inputs[split..]);
        let actual = play(&mut reloaded, &inputs[split..]);
        assert_eq!(
            first_difference(&expected, &actual),
            None,
            "saving at tick {split} changed the run that followed",
        );
    }
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
/// the bag, the gear, the health and the mana are all still there — through a
/// file on disk, not through a struct held in memory.
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
