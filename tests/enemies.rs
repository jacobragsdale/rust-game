//! The crypt's enemies — a flyer, a caster, and a boss that does both — run
//! through the real `Sim` on grids small enough to read.
//!
//! None of them has a line of code of its own: each is a stat block and a clip
//! set, and `npc::think` runs whichever moves the block gives it. So these are
//! the checks that the moves themselves work, one property at a time, where a
//! tape would have to go out of its way to isolate them.

use ggez::glam::Vec2;
use supergame::assets::{Assets, StatTable};
use supergame::ecs::components::{Avatar, Body, Health, Hostile, Position, Stance};
use supergame::level::{EntitySpawn, LevelData};
use supergame::sim::{fixture_clip_sets, GameEvent, Sim, DEFAULT_SEED};
use supergame::systems::input::PlayerInput;

/// A grid, with `placed` kinds added at (column, row) cells.
fn arena(grid: &[&str], placed: &[(&str, usize, usize)]) -> Sim {
    let mut level = LevelData::from_grid(grid).unwrap();
    let ts = level.tile_size;
    for &(kind, col, row) in placed {
        level.entities.push(EntitySpawn {
            kind: kind.to_string(),
            pos: Vec2::new(col as f32 * ts, row as f32 * ts),
            ..Default::default()
        });
    }
    Sim::new(
        level,
        &fixture_clip_sets(),
        Assets::new().attacks().unwrap(),
        StatTable::shipped(),
        DEFAULT_SEED,
    )
}

/// Five rows, the floor at the bottom.
const ROOM: [&str; 5] = [
    "........................",
    "........................",
    "........................",
    "..P.....................",
    "########################",
];

fn pos(sim: &Sim, e: hecs::Entity) -> Vec2 {
    sim.world.get::<&Position>(e).unwrap().0
}

fn player_pos(sim: &Sim) -> Vec2 {
    let (_, (_, pos)) = sim
        .world
        .query::<(&Avatar, &Position)>()
        .iter()
        .next()
        .map(|(e, (a, p))| (e, (a.facing_right, p.0)))
        .unwrap();
    pos
}

fn player_hurt(events: &[GameEvent]) -> bool {
    events
        .iter()
        .any(|e| matches!(e, GameEvent::Damaged { who, .. } if who == "player"))
}

// --- the bat ---------------------------------------------------------------

/// A flyer holds itself up: placed in mid-air with nobody about, it is still
/// there two seconds later — it neither falls nor wanders.
#[test]
fn a_bat_hangs_where_it_was_placed_until_it_sees_someone() {
    let mut sim = arena(&ROOM, &[("bat", 18, 1)]);
    let bat = sim.npcs()[0];
    let start = pos(&sim, bat);
    for _ in 0..120 {
        sim.step(PlayerInput::default());
    }
    assert_eq!(pos(&sim, bat), start, "neither fell nor wandered");
    assert_eq!(
        sim.world.get::<&Hostile>(bat).unwrap().stance,
        Stance::Patrol
    );
}

/// Seen, it flies straight at the player, bites, breaks off — away and up —
/// while its cooldown runs, and comes round again for a second bite.
#[test]
fn a_bat_swoops_bites_breaks_off_and_comes_back() {
    let mut sim = arena(&ROOM, &[("bat", 6, 1)]);
    let bat = sim.npcs()[0];

    let mut bites = Vec::new();
    for tick in 0..600 {
        sim.step(PlayerInput::default());
        if player_hurt(sim.events()) {
            bites.push(tick);
        }
        if bites.len() == 1 && tick == bites[0] + 20 {
            let apart = pos(&sim, bat).distance(player_pos(&sim));
            assert!(
                apart > 30.0,
                "broke off after biting (only {apart:.0}px away 20 ticks later)"
            );
            assert!(
                pos(&sim, bat).y < player_pos(&sim).y,
                "and went up, out of reach"
            );
        }
        if bites.len() == 2 {
            break;
        }
    }
    assert_eq!(
        bites.len(),
        2,
        "bit, and came back to bite again: {bites:?}"
    );
}

/// Dead, a flyer is a body like any other: it falls to the floor and stays.
#[test]
fn a_dead_bat_falls_to_the_floor() {
    let mut sim = arena(&ROOM, &[("bat", 18, 1)]);
    let bat = sim.npcs()[0];
    sim.world.get::<&mut Health>(bat).unwrap().current = 0;
    for _ in 0..90 {
        sim.step(PlayerInput::default());
    }
    let body = *sim.world.get::<&Body>(bat).unwrap();
    assert!(body.grounded && body.frozen, "landed and settled");
    let floor = 4.0 * 32.0;
    assert_eq!(pos(&sim, bat).y + 10.0, floor, "on the floor");
}

// --- the wolf --------------------------------------------------------------

/// A walking biter lunges, bites, and gives ground while it recovers — the
/// wolf's lunge and circle, where the bat has its swoop — rather than standing
/// in your face chewing until its next bite comes round.
#[test]
fn a_wolf_bites_then_gives_ground_until_it_can_bite_again() {
    // Placed facing the player: a walker sees only what is in front of it.
    let grid = [
        "........................",
        "........................",
        "........................",
        "..........P.............",
        "########################",
    ];
    let mut sim = arena(&grid, &[("wolf", 5, 3)]);
    let wolf = sim.npcs()[0];
    let mut bitten = None;
    for tick in 0..600 {
        sim.step(PlayerInput::default());
        if bitten.is_none() && player_hurt(sim.events()) {
            let away = (pos(&sim, wolf).x - player_pos(&sim).x).signum();
            bitten = Some((tick, pos(&sim, wolf).x, away));
        }
        if let Some((at, x, away)) = bitten {
            if tick == at + 20 {
                let backed = (pos(&sim, wolf).x - x) * away;
                assert!(backed > 4.0, "gave ground after the bite ({backed:.1}px)");
                return;
            }
        }
    }
    panic!("the wolf never bit");
}

// --- the mage --------------------------------------------------------------

/// In range, a caster does not close in: it stands where it is and throws,
/// and the ember crosses the gap to the player.
#[test]
fn a_mage_stands_off_and_throws_embers() {
    let grid = [
        "........................",
        "........................",
        "........................",
        "..........P.............",
        "########################",
    ];
    let mut sim = arena(&grid, &[("mage", 4, 3)]);
    let mage = sim.npcs()[0];
    let start = pos(&sim, mage).x;

    let mut cast = false;
    let mut hit = false;
    for _ in 0..240 {
        sim.step(PlayerInput::default());
        cast |= sim.events().contains(&GameEvent::SpellCast {
            spell: "ember".to_string(),
        });
        hit |= player_hurt(sim.events());
        assert_eq!(pos(&sim, mage).x, start, "held its ground");
        if hit {
            break;
        }
    }
    assert!(cast, "cast an ember");
    assert!(hit, "and it landed on a player who stood still");
}

// --- the warden ------------------------------------------------------------

/// Both moves in one kind: from across the room it throws, and up close it
/// swings — the sword takes precedence over the spell once the player is in
/// reach.
#[test]
fn the_warden_throws_from_afar_and_swings_up_close() {
    let grid = [
        "........................",
        "........................",
        "........................",
        "..........P.............",
        "########################",
    ];

    // Afar: about 150px.
    let mut sim = arena(&grid, &[("warden", 5, 3)]);
    let mut first = None;
    for _ in 0..60 {
        sim.step(PlayerInput::default());
        first = first.or_else(|| {
            sim.events().iter().find_map(|e| match e {
                GameEvent::SpellCast { spell } => Some(spell.clone()),
                GameEvent::Attacked { attack } => Some(attack.clone()),
                _ => None,
            })
        });
    }
    assert_eq!(first.as_deref(), Some("ember"), "threw from afar");

    // Up close: the cell beside the player.
    let mut sim = arena(&grid, &[("warden", 9, 3)]);
    let mut first = None;
    for _ in 0..60 {
        sim.step(PlayerInput::default());
        first = first.or_else(|| {
            sim.events().iter().find_map(|e| match e {
                GameEvent::SpellCast { spell } => Some(spell.clone()),
                GameEvent::Attacked { attack } => Some(attack.clone()),
                _ => None,
            })
        });
    }
    assert_eq!(first.as_deref(), Some("warden_slash"), "swung up close");
}

/// A kind with a sword and a spell stands off only while it can throw. Out of
/// mana, the warden walks in to use the sword — it used to stand at the far
/// end of its hall waiting for its pool to refill, and could be ignored.
#[test]
fn the_warden_closes_in_when_it_has_no_ember_to_throw() {
    let grid = [
        "........................",
        "........................",
        "........................",
        "..........P.............",
        "########################",
    ];
    let mut sim = arena(&grid, &[("warden", 5, 3)]);
    let warden = sim.npcs()[0];
    sim.world
        .get::<&mut supergame::ecs::components::Mana>(warden)
        .unwrap()
        .current = 0;
    let start = pos(&sim, warden).x;
    let mut swung = false;
    for _ in 0..240 {
        sim.step(PlayerInput::default());
        swung |= sim.events().contains(&GameEvent::Attacked {
            attack: "warden_slash".to_string(),
        });
        if swung {
            break;
        }
    }
    assert!(pos(&sim, warden).x > start + 60.0, "walked in");
    assert!(swung, "and swung");
}

// --- a rival ---------------------------------------------------------------

/// A champion of the Proving Circle stands where it was put until the player
/// comes within its sight, then closes in and fights with its own style — the
/// same combo engine as the player, opening with the saber's first cut.
#[test]
fn a_rival_waits_for_you_then_comes_for_you_with_its_style() {
    let mut sim = arena(&ROOM, &[("kesh", 20, 3)]);
    let kesh = sim.npcs()[0];
    let start = pos(&sim, kesh);
    for _ in 0..90 {
        sim.step(PlayerInput::default());
    }
    assert_eq!(
        pos(&sim, kesh).x,
        start.x,
        "no one in sight: it holds its ground"
    );

    let (mut swung, mut hurt) = (false, false);
    for _ in 0..400 {
        sim.step(PlayerInput::holding(&[
            supergame::systems::input::Action::Right,
        ]));
        swung |= sim
            .events()
            .iter()
            .any(|e| matches!(e, GameEvent::Attacked { attack } if attack == "saber_1"));
        hurt |= player_hurt(sim.events());
        if hurt {
            break;
        }
    }
    assert!(pos(&sim, kesh).x < start.x, "it came to meet you");
    assert!(swung, "it swung the saber's opener");
    assert!(hurt, "and it landed");
    // The probe is still the player's with a second avatar in the world.
    assert!(
        sim.probe().hp < sim.probe().hp_max,
        "the probe reads the player"
    );
}

/// Cut down, a rival stays down: it is an avatar, but only the player's
/// avatar gets up again. Its death names it, and it leaves its blade behind.
#[test]
fn a_rival_that_falls_stays_down_and_drops_its_weapon() {
    let mut sim = arena(&ROOM, &[("kesh", 20, 3)]);
    let kesh = sim.npcs()[0];
    let mut events = Vec::new();
    supergame::systems::combat::apply_hit(&mut sim.world, kesh, 99, Vec2::ZERO, 0, &mut events);
    assert!(events
        .iter()
        .any(|e| matches!(e, GameEvent::Died { who, .. } if who == "kesh")));
    for _ in 0..300 {
        sim.step(PlayerInput::default());
    }
    let health = *sim.world.get::<&Health>(kesh).unwrap();
    assert!(health.dead(), "still down five seconds later");
    assert!(sim.npc_probes()[0].dead);
    assert!(sim.world.get::<&Body>(kesh).unwrap().frozen);
    assert_eq!(sim.probe().pickups, 1, "the saber is on the floor");
    assert!(
        !sim.probe().dead,
        "and the player was never the one who died"
    );
}
