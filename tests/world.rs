//! The level's furniture and the ways between maps, checked one property at a
//! time. `tapes/world_props.tape` and `tapes/world_travel.tape` walk the whole
//! of it end to end; these pin down the edges a tape would have to go out of
//! its way to reach.

use std::collections::BTreeMap;

use supergame::assets::Assets;
use supergame::ecs::components::{Avatar, Collider, Equipment, Gate, Health, Inventory, Mana};
use supergame::sim::{GameEvent, Sim};
use supergame::systems::input::{Action, PlayerInput};

fn load(map: &str) -> Sim {
    Sim::load(&mut Assets::new(), map).unwrap_or_else(|e| panic!("{map}: {e:#}"))
}

fn player(sim: &Sim) -> hecs::Entity {
    sim.world
        .query::<&Avatar>()
        .iter()
        .map(|(e, _)| e)
        .next()
        .expect("a player")
}

fn step_until(sim: &mut Sim, limit: u32, input: PlayerInput, done: impl Fn(&Sim) -> bool) {
    for _ in 0..limit {
        sim.step(input);
        if done(sim) {
            return;
        }
    }
    panic!("never happened in {limit} ticks");
}

/// What a door carries is the player: health, mana, the bag and what is worn
/// all arrive on the far side exactly as they left.
#[test]
fn a_door_carries_the_player_and_nothing_else() {
    let mut sim = load("maps/testbed_world_b.ron");
    sim.step(PlayerInput::default());
    let me = player(&sim);
    sim.world.get::<&mut Health>(me).unwrap().current = 3;
    sim.world.get::<&mut Mana>(me).unwrap().current = 1;
    sim.world.get::<&mut Inventory>(me).unwrap().add("coin", 7);
    sim.world
        .get::<&mut Equipment>(me)
        .unwrap()
        .slots
        .insert(supergame::assets::Slot::Head, "knight_helm".to_string());

    // West into the annex's exit.
    step_until(&mut sim, 120, PlayerInput::holding(&[Action::Left]), |s| {
        s.map_name() == "testbed_world"
    });

    let me = player(&sim);
    assert_eq!(sim.world.get::<&Health>(me).unwrap().current, 3);
    assert_eq!(sim.world.get::<&Mana>(me).unwrap().current, 1);
    assert_eq!(sim.world.get::<&Inventory>(me).unwrap().count("coin"), 7);
    assert!(sim
        .world
        .get::<&Equipment>(me)
        .unwrap()
        .holds("knight_helm"));
    // The derived maximum came through with the helm, the same tick.
    assert_eq!(sim.probe().hp_max, 7);
    // ...and dying here brings you back to where you came in, not to a `P`
    // on a map you never saw.
    let probe = sim.probe();
    assert_eq!(
        sim.respawn_point(),
        ggez::glam::Vec2::new(probe.x, probe.y) - {
            // One tick of walking past the arrival point before the check ran.
            ggez::glam::Vec2::new(probe.x - sim.respawn_point().x, 0.0)
        }
    );
}

/// A way through to nowhere says so, and leaves the player where they were.
#[test]
fn a_door_to_a_map_that_does_not_exist_fails_loudly_and_goes_nowhere() {
    let mut sim = load("maps/testbed_world_b.ron");
    sim.step(PlayerInput::default());
    let exit = sim
        .world
        .query::<&supergame::ecs::components::Exit>()
        .iter()
        .map(|(e, _)| e)
        .next()
        .unwrap();
    sim.world
        .get::<&mut supergame::ecs::components::Exit>(exit)
        .unwrap()
        .to = "maps/no_such_place.ron".to_string();

    let mut failed = false;
    for _ in 0..120 {
        sim.step(PlayerInput::holding(&[Action::Left]));
        failed |= sim
            .events()
            .iter()
            .any(|e| matches!(e, GameEvent::TravelFailed { map } if map == "no_such_place"));
    }
    assert!(failed, "the failure was announced");
    assert_eq!(
        sim.map_name(),
        "testbed_world_b",
        "and nobody went anywhere"
    );
}

/// A chest is all or nothing: with no room for one of the things inside, it
/// stays shut and says why, rather than handing over half.
#[test]
fn a_chest_with_no_room_for_its_contents_stays_shut() {
    let mut sim = load("maps/testbed_world.ron");
    sim.step(PlayerInput::default());
    let me = player(&sim);
    {
        let mut bag = sim.world.get::<&mut Inventory>(me).unwrap();
        for index in 0..bag.capacity {
            bag.add(&format!("filler_{index}"), 1);
        }
    }
    // Stand at the chest.
    let chest_x = 646.0;
    sim.world
        .get::<&mut supergame::ecs::components::Position>(me)
        .unwrap()
        .0
        .x = chest_x - 10.0;
    sim.step(PlayerInput::default());
    assert_eq!(sim.probe().prompt, "open");
    sim.step(PlayerInput::from_actions(&[Action::Interact]));
    assert!(sim
        .events()
        .iter()
        .any(|e| matches!(e, GameEvent::InventoryFull { .. })));
    assert!(!sim.events().contains(&GameEvent::ChestOpened));
    assert_eq!(sim.flag("world.testbed_world.chest_20_9"), 0);
    assert_eq!(sim.probe().prompt, "open", "still there to open");
}

/// A gate's collider is its closedness: it is in the geometry while its flag
/// is 0 and gone the tick the flag is set.
#[test]
fn a_gate_is_solid_until_its_flag_is_set() {
    let mut sim = load("maps/testbed_world.ron");
    let gate = sim
        .world
        .query::<&Gate>()
        .iter()
        .map(|(e, _)| e)
        .next()
        .unwrap();
    let solids = sim.geometry.rects().len();
    assert!(
        sim.world.get::<&Collider>(gate).is_ok(),
        "shut at the start"
    );

    sim.set_flag("quest.testbed.gate", 1);
    sim.step(PlayerInput::default());
    assert!(sim.world.get::<&Collider>(gate).is_err(), "raised");
    assert_eq!(sim.geometry.rects().len(), solids - 1);

    // ...and a run loaded with the flag already set starts with it raised.
    let mut flags = BTreeMap::new();
    flags.insert("quest.testbed.gate".to_string(), 1);
    let state = supergame::save::SaveState {
        flags,
        ..sim.save().unwrap()
    };
    let loaded = Sim::load_save(&mut Assets::new(), &state).unwrap();
    assert_eq!(loaded.geometry.rects().len(), solids - 1);
}

/// A death flag is set when its NPC dies — and the NPC is never despawned,
/// which would renumber every NPC after it in the map.
#[test]
fn a_death_flag_is_set_by_the_death() {
    let mut level =
        supergame::level::LevelData::from_grid(&["..........", "..P....K..", "##########"])
            .unwrap();
    level.entities[0].flag = Some("quest.test.guard".to_string());
    let mut sim = Sim::new(
        level,
        &supergame::sim::fixture_clip_sets(),
        Assets::new().attacks().unwrap(),
        supergame::assets::StatTable::shipped(),
        supergame::sim::DEFAULT_SEED,
    );
    let knight = sim.npcs()[0];
    sim.world.get::<&mut Health>(knight).unwrap().current = 0;
    sim.step(PlayerInput::default());
    assert_eq!(sim.flag("quest.test.guard"), 1);
    assert_eq!(sim.npcs().len(), 1, "never despawned");
}

/// ...and an NPC whose flag is already set is spawned as the corpse it is:
/// dead, on the last frame of dying, with its loot long gone.
#[test]
fn an_npc_whose_death_flag_is_set_is_spawned_dead() {
    let stats = supergame::assets::StatTable::shipped();
    let clips = std::sync::Arc::new(supergame::sim::fixture_clips_for_kind());
    let mut world = hecs::World::new();
    let placement = supergame::level::EntitySpawn {
        kind: "knight".to_string(),
        pos: ggez::glam::Vec2::new(64.0, 0.0),
        dialogue: None,
        flag: Some("quest.test.guard".to_string()),
    };
    let knight = supergame::ecs::spawn::entity(
        &mut world,
        &placement,
        32.0,
        clips,
        stats.get("knight").unwrap(),
    )
    .unwrap();
    let mut flags = BTreeMap::new();
    flags.insert("quest.test.guard".to_string(), 1);
    supergame::ecs::spawn::customise(&mut world, knight, &placement, &flags);

    assert!(world.get::<&Health>(knight).unwrap().dead());
    assert!(
        world
            .get::<&supergame::ecs::components::Loot>(knight)
            .unwrap()
            .dropped
    );
    assert_eq!(
        world
            .get::<&supergame::ecs::components::AnimationState>(knight)
            .unwrap()
            .clip,
        "death"
    );
}

/// A trigger tells its story the first time it is walked into, and never
/// again — its world flag remembers, even across a reload. One that waits on
/// a flag says nothing until the flag is set.
#[test]
fn a_trigger_tells_its_story_once_and_waits_on_its_flag() {
    let opened = |sim: &Sim, graph: &str| {
        sim.events()
            .iter()
            .any(|e| matches!(e, GameEvent::DialogueOpened { graph: g } if g == graph))
    };
    let mut sim = load("maps/testbed_secrets.ron");
    step_until(&mut sim, 120, PlayerInput::holding(&[Action::Right]), |s| {
        opened(s, "testbed_trigger")
    });
    assert_eq!(sim.flag("world.testbed_secrets.hello"), 1);
    // Close it, walk back and forth through the same doorway: nothing more.
    sim.step(PlayerInput::from_actions(&[Action::Cancel]));
    for dir in [Action::Left, Action::Right, Action::Left] {
        for _ in 0..40 {
            sim.step(PlayerInput::holding(&[dir]));
            assert!(!opened(&sim, "testbed_trigger"), "told only once");
        }
    }
    // Past the late one, with its bell unrung: silence.
    while sim.probe().x < 300.0 {
        sim.step(PlayerInput::holding(&[Action::Right]));
        assert!(!opened(&sim, "testbed_trigger_late"), "not before the bell");
    }
    // Ring it, walk back into it: now it speaks.
    sim.set_flag("quest.testbed.bell", 1);
    step_until(&mut sim, 120, PlayerInput::holding(&[Action::Left]), |s| {
        opened(s, "testbed_trigger_late")
    });
}

/// A false wall is walked straight through to what is behind it.
#[test]
fn a_false_wall_is_walked_through_to_the_secret_behind_it() {
    let mut sim = load("maps/testbed_secrets.ron");
    sim.set_flag("world.testbed_secrets.hello", 1);
    step_until(&mut sim, 200, PlayerInput::holding(&[Action::Right]), |s| {
        s.probe().x > 12.0 * 32.0
    });
}
