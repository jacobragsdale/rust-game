//! Hazards that come and go: fire on a cycle.
//!
//! A fire is the cheapest possible user of entity-owned geometry, and that is
//! the point of it. It never moves; all it does is stop existing for a while.
//! While it is lit its entity carries a `Hazard` [`Collider`], which
//! [`crate::systems::body::rebuild_geometry`] folds into the world's geometry
//! along with every solid; while it is out the entity has no collider, and
//! there is nothing anywhere in the physics or the death check that knows the
//! difference between an unlit fire and no fire at all.
//!
//! [`tick_schedules`] therefore belongs in the *decide* phase of the tick,
//! beside the controllers: it is a thing that owns a collider deciding where —
//! here, whether — that collider is this tick. `Sim::step` rebuilds the
//! geometry once, after everything has decided, so a fire that lights on tick
//! *t* is lethal on tick *t*.

use ggez::glam::Vec2;
use hecs::World;

use crate::ecs::components::{Collider, ColliderKind, Fire, Position, Schedule};
use crate::level::LevelData;

/// The default cycle for a fire placed as a grid `F`: two seconds, the first
/// second of it lit.
///
/// A whole second on and a whole second off is long enough to watch, decide
/// and cross at a run — a fire that blinks faster is a coin flip rather than a
/// pattern. Maps that want otherwise say so with a `Fire(...)` entry.
pub const FIRE_PERIOD: u32 = 120;
pub const FIRE_DUTY: u32 = 60;

/// The box a lit fire kills inside.
///
/// Narrower than its cell so that standing on the tile beside a fire is safe —
/// with a full-width box the lethal edge lands exactly on the tile seam, and
/// "I wasn't even touching it" is the resulting bug report. Tall enough to
/// catch a body standing on the cell's floor, and no taller: jumping over a
/// fire has to work.
pub const FIRE_SIZE: Vec2 = Vec2::new(20.0, 26.0);

/// Spawn an entity for every fire the map places.
///
/// Called from `Sim::new` after the map's entity list, so adding a fire to a
/// map cannot renumber the NPCs a tape addresses by index.
pub fn spawn_fires(world: &mut World, level: &LevelData) {
    for fire in &level.fires {
        let schedule = Schedule::new(fire.period, fire.duty, fire.phase);
        let collider = Collider {
            // Centred in its cell, standing on the cell floor, the same way a
            // spawned body is placed in one.
            rect_offset: Vec2::new(
                (level.tile_size - FIRE_SIZE.x) / 2.0,
                level.tile_size - FIRE_SIZE.y,
            ),
            size: FIRE_SIZE,
            kind: ColliderKind::Hazard,
        };
        let entity = world.spawn((Position(fire.cell), Fire { collider }, schedule));
        // Spawned in the state tick 0 calls for, because `Sim::new` builds the
        // geometry before the first `step` and a fire that started lit has to
        // be lethal on the tick the player sees it lit.
        if schedule.on_at(0) {
            world
                .insert_one(entity, collider)
                .expect("just spawned this entity");
        }
    }
}

/// Light and douse every scheduled hazard for this tick.
///
/// The collider is inserted and removed rather than flagged, so "lit" has
/// exactly one representation and the geometry cannot disagree with the
/// renderer about it. Adding and removing a component moves an entity between
/// hecs archetypes, which reshuffles query order — harmless here only because
/// `rebuild_geometry` sorts by entity id before handing anything to the
/// physics. Without that sort this would move the game every time a fire
/// blinked.
///
/// Hitstop returns from `Sim::step` before this runs, so a fire holds its
/// state for the few frozen ticks — which is right, since nothing else in the
/// world moves either — and then snaps to the correct one on the first live
/// tick. It snaps rather than drifts precisely because the schedule is read
/// from the tick rather than counted.
pub fn tick_schedules(world: &mut World, tick: u64) {
    // Collected first: the world cannot be structurally changed while a query
    // over it is alive.
    let mut light: Vec<(hecs::Entity, Collider)> = Vec::new();
    let mut douse: Vec<hecs::Entity> = Vec::new();

    for (entity, (fire, schedule, collider)) in world
        .query::<(&Fire, &Schedule, Option<&Collider>)>()
        .iter()
    {
        match (schedule.on_at(tick), collider.is_some()) {
            (true, false) => light.push((entity, fire.collider)),
            (false, true) => douse.push(entity),
            _ => {}
        }
    }

    for (entity, collider) in light {
        world
            .insert_one(entity, collider)
            .expect("entity came from a query over this world");
    }
    for entity in douse {
        world
            .remove_one::<Collider>(entity)
            .expect("entity came from a query over this world");
    }
}

/// Is this fire lit? Its collider's presence is the answer — see [`Fire`].
///
/// For the renderer and the overlay, which need to draw the difference.
pub fn is_lit(world: &World, entity: hecs::Entity) -> bool {
    world.get::<&Collider>(entity).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::Avatar;
    use crate::physics::{Aabb, HazardQuery};
    use crate::sim::event::{DeathCause, GameEvent};
    use crate::sim::Sim;
    use crate::systems::input::{Action, PlayerInput};

    #[test]
    fn a_schedule_is_a_closed_form_function_of_the_tick() {
        let schedule = Schedule::new(120, 60, 0);
        assert!(schedule.on_at(0));
        assert!(schedule.on_at(59));
        assert!(!schedule.on_at(60));
        assert!(!schedule.on_at(119));
        assert!(schedule.on_at(120), "and it comes back round");

        // Periodic forever, with no accumulated error, because nothing
        // accumulates.
        for tick in 0..500u64 {
            assert_eq!(schedule.on_at(tick), schedule.on_at(tick + 120));
        }
    }

    /// The authored case from the ticket: two fires that take turns.
    #[test]
    fn phase_makes_two_fires_alternate() {
        let a = Schedule::new(120, 60, 0);
        let b = Schedule::new(120, 60, 60);
        for tick in 0..360u64 {
            assert_ne!(
                a.on_at(tick),
                b.on_at(tick),
                "tick {tick}: both fires agreed, so they are not alternating"
            );
        }
    }

    /// Degenerate, but a map is allowed to say it: a fire that is simply
    /// always there.
    #[test]
    fn a_period_of_zero_is_a_permanent_hazard() {
        assert!(Schedule::new(0, 1, 0).on_at(12_345));
        assert!(!Schedule::new(0, 0, 0).on_at(12_345));
    }

    /// The whole feature, end to end: geometry appears and disappears on the
    /// cycle, and it is hazard geometry, not solid.
    #[test]
    fn a_fire_hands_the_geometry_a_hazard_only_while_it_is_lit() {
        let mut sim = Sim::fixture(&["........", "..P..F..", "########"]);
        let solids = sim.geometry.rects().len();

        let mut lit_ticks = 0;
        for tick in 0..240u64 {
            sim.step(PlayerInput::default());
            let hazards = sim.geometry.entity_hazard_count();
            let expected = Schedule::new(FIRE_PERIOD, FIRE_DUTY, 0).on_at(tick);
            assert_eq!(
                hazards == 1,
                expected,
                "tick {tick}: hazard count {hazards} does not match the schedule"
            );
            assert_eq!(
                sim.geometry.rects().len(),
                solids,
                "a fire must never become solid"
            );
            lit_ticks += hazards;
        }
        assert_eq!(lit_ticks, 120, "half of four seconds, lit");
    }

    /// The acceptance criterion, as a fixture: walking into an unlit fire is
    /// survivable, and standing in it when it lights is not.
    ///
    /// The fire is in the last cell of the map, so running right parks the
    /// player inside it against the map's edge — no timing to get right, and
    /// the only thing that changes across the run is the schedule.
    #[test]
    fn an_unlit_fire_is_harmless_and_a_lit_one_kills() {
        // Phase puts it out for the first second, which is long enough to
        // walk into it, and lights it on tick 60.
        let mut level = crate::level::LevelData::from_grid(&["........", "..P....F", "########"])
            .expect("fixture grid parses");
        level.fires[0].phase = FIRE_DUTY;

        let mut sim = Sim::new(
            level,
            &crate::sim::fixture_clip_sets(),
            crate::assets::Assets::new().attacks().unwrap(),
            crate::assets::StatTable::shipped(),
            crate::sim::DEFAULT_SEED,
        );

        let run = PlayerInput::holding(&[Action::Right]);
        // Ticks 0..59: the fire is out the whole time, and the player spends
        // the last third of them standing where it will be.
        for tick in 0..FIRE_DUTY {
            sim.step(run);
            assert!(
                !sim.probe().dead,
                "tick {tick}: died to a fire that was not lit"
            );
        }
        assert!(
            !sim.geometry.hazard_overlapping(player_box(&sim)),
            "nothing lethal exists yet: the fire is out"
        );
        assert!(
            player_box(&sim).overlaps(&fire_box(&sim)),
            "the player should be standing where the fire is about to light"
        );

        sim.step(run); // tick 60: it lights
        assert!(sim.probe().dead, "standing in a lit fire is lethal");
    }

    /// A fire kills by the same path a spike does, and says so.
    #[test]
    fn burning_to_death_is_a_hazard_death() {
        let mut sim = Sim::fixture(&["........", "..P.....", "########"]);
        for _ in 0..10 {
            sim.step(PlayerInput::default());
        }

        // Drop a lit fire straight onto the player rather than walking into
        // one: this is about the cause, not the approach.
        let collider = Collider::hazard(FIRE_SIZE);
        let pos = Vec2::new(sim.probe().x - 6.0, sim.probe().y);
        sim.world.spawn((
            Position(pos),
            Fire { collider },
            Schedule::new(0, 1, 0),
            collider,
        ));

        sim.step(PlayerInput::default());
        assert_eq!(
            sim.events(),
            [GameEvent::Died {
                who: "player".to_string(),
                cause: DeathCause::Hazard,
            }]
        );
    }

    /// Where the map's one fire burns, lit or not.
    fn fire_box(sim: &Sim) -> Aabb {
        let mut query = sim.world.query::<(&Position, &Fire)>();
        let (_, (pos, fire)) = query.iter().next().expect("the map placed a fire");
        fire.collider.aabb(pos.0)
    }

    fn player_box(sim: &Sim) -> Aabb {
        let probe = sim.probe();
        Aabb::new(probe.x, probe.y, Avatar::WIDTH, Avatar::HEIGHT)
    }
}
