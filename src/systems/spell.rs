//! Casting a spell, and the projectile it produces.
//!
//! **Why one module and not two.** The ticket offered `spell.rs` or
//! `projectile.rs`. They are the same feature seen from either end: a cast is
//! a countdown whose entire purpose is the tick it releases a bolt on, and a
//! bolt is a hit that was decided twelve ticks earlier. Splitting them would
//! put the release in one file and the thing released in the other, and the
//! first question anyone asks — "when does the bolt appear?" — would need
//! both. `combat.rs` stays about swings, which resolve inside the tick they
//! are thrown; this is about the ones that do not.
//!
//! **Why projectiles may be despawned, and NPCs may not.** The never-despawn
//! rule exists because NPCs are addressed by *spawn index*: tapes and traces
//! say `knight.0`, and removing a knight silently renumbers every later one,
//! turning a passing assertion into a passing assertion about a different
//! entity. Projectiles are not addressed at all. A trace records how many
//! exist, never which; nothing can name one, so nothing can be repointed by
//! one going away. A bolt that stayed in the world as a corpse would instead
//! be a permanent, invisible entity per cast, which is the actual leak.
//! `Sim::npcs` picks entities out by `Patrol`, which a projectile has not —
//! and `projectiles_are_never_npcs` below fails if that ever changes.
//!
//! Two phases of the tick, for the same reason combat has two:
//!
//! - [`advance_casts`] runs with the other timers, at the top of the tick,
//!   *before* any controller reads them. It is also where a bolt is spawned,
//!   so the bolt is in the world before `move_bodies` and travels on the tick
//!   it appears rather than hanging still for one.
//! - [`resolve_projectiles`] runs in the resolve phase beside
//!   [`crate::systems::combat::resolve`], once everything has finished
//!   moving — so a bolt connects where the bodies actually ended the tick.

use std::sync::Arc;

use ggez::glam::Vec2;
use hecs::World;

use crate::assets::{ClipSet, SpellDef, SpellEffect, SpellTable};
use crate::ecs::components::{
    Avatar, Casting, Health, Lifetime, Mana, Position, Projectile, Size, Sprite, Team, Velocity,
};
use crate::ecs::spawn;
use crate::physics::{Aabb, SolidQuery, SolidRect};
use crate::sim::event::{CastFailure, GameEvent};
use crate::systems::combat;

/// Whether a caster may start `spell` right now, or why not.
///
/// Pulled out of [`crate::systems::avatar::control`] so that the refusal has
/// exactly one definition and the event that reports it cannot disagree with
/// the check that produced it.
pub fn refusal(
    spells: &SpellTable,
    spell: &str,
    casting: &Casting,
    mana: &Mana,
    busy: bool,
) -> Option<CastFailure> {
    let Some(def) = spells.get(spell) else {
        return Some(CastFailure::Unknown);
    };
    if busy || casting.busy() {
        Some(CastFailure::Busy)
    } else if casting.cooldown > 0 {
        Some(CastFailure::Cooling)
    } else if !mana.affords(def.cost) {
        Some(CastFailure::NoMana)
    } else {
        None
    }
}

/// Count down cooldowns, regenerate mana, advance any cast in progress, and
/// release the effect on the tick the spell says to.
///
/// Runs at the top of the tick, so a controller asking "am I still casting?"
/// or "has the cooldown lapsed?" reads this tick's answer rather than last
/// tick's — the same rule [`crate::systems::combat::tick_timers`] follows.
///
/// Emits nothing, unlike its counterpart `combat::advance_attacks`: a cast
/// announces itself when it *starts*, because that is the tick the player
/// pressed the key and the tick the mana left the pool. What happens twelve
/// ticks later is a consequence, and reporting both would make
/// `expect shock.spell_cast == 1` mean two different things.
pub fn advance_casts(world: &mut World, spells: &SpellTable) {
    // Everything a release needs, collected while the world is only borrowed
    // immutably; hecs cannot spawn during a query.
    let mut releases: Vec<(hecs::Entity, String)> = Vec::new();

    for (caster, (casting, mana, health, avatar)) in
        world.query_mut::<(&mut Casting, &mut Mana, Option<&Health>, Option<&Avatar>)>()
    {
        mana.regenerate();
        casting.cooldown = casting.cooldown.saturating_sub(1);

        // Dying mid-cast drops the spell, and so does being hit during one —
        // exactly as a swing is dropped, and for the same reason: whoever
        // connects first wins the exchange. The mana is spent either way,
        // which is what makes a cast a commitment rather than a free action.
        let interrupted =
            health.is_some_and(|h| h.dead() || h.hitstun > 0) || avatar.is_some_and(|a| a.dead());
        if interrupted {
            casting.stop();
            continue;
        }

        let Some(id) = casting.spell.clone() else {
            continue;
        };
        let Some(def) = spells.get(&id) else {
            casting.stop();
            continue;
        };

        casting.elapsed += 1;
        if def.releases_at(casting.elapsed) {
            releases.push((caster, id.clone()));
        }
        if def.released(casting.elapsed) {
            casting.stop();
        }
    }

    for (caster, id) in releases {
        let Some(def) = spells.get(&id) else { continue };
        release(world, caster, def);
    }
}

/// Put a spell's effect into the world.
///
/// Silent: `SpellCast` was emitted when the cast started, because that is the
/// tick the player pressed the key and the tick the mana left the pool. An
/// event here would announce the same decision twice.
fn release(world: &mut World, caster: hecs::Entity, def: &SpellDef) {
    match &def.effect {
        SpellEffect::Projectile { size, .. } => {
            let Some((origin, facing_right, team, clips)) = launch_site(world, caster, *size)
            else {
                return;
            };
            spawn::projectile(world, caster, origin, facing_right, def, team, clips);
        }
    }
}

/// Where a caster's projectile starts, which way it goes, whose side it is on,
/// and what art it is drawn from.
///
/// The bolt leaves from the caster's leading edge at chest height, so it is
/// never spawned inside the body that threw it — a projectile that starts
/// overlapping its caster reads as a bolt that fires half a tile late.
fn launch_site(
    world: &World,
    caster: hecs::Entity,
    size: (f32, f32),
) -> Option<(Vec2, bool, Team, Arc<ClipSet>)> {
    let pos = world.get::<&Position>(caster).ok()?.0;
    let body = world.get::<&Size>(caster).ok()?.0;
    let team = *world.get::<&Team>(caster).ok()?;
    let clips = world.get::<&Sprite>(caster).ok()?.clips.clone();
    let facing_right = world
        .get::<&Avatar>(caster)
        .map(|a| a.facing_right)
        .unwrap_or(true);

    let x = if facing_right {
        pos.x + body.x
    } else {
        pos.x - size.0
    };
    // A third of the way down the body: roughly where the hand is, and clear
    // of the ground so a bolt cast while standing does not clip the floor.
    let y = pos.y + body.y / 3.0 - size.1 / 2.0;
    Some((Vec2::new(x, y), facing_right, team, clips))
}

/// Age every projectile, land the ones that are touching something, and
/// remove the ones that are finished.
///
/// Runs after `move_bodies`, so a bolt is tested where it actually ended the
/// tick — at 320 px/s it covers five pixels a tick, and testing it at the
/// position it was launched from would miss thin targets entirely.
pub fn resolve_projectiles<Q: SolidQuery + ?Sized>(
    world: &mut World,
    geometry: &Q,
    events: &mut Vec<GameEvent>,
) {
    // What landed, and what is finished. Collected first, because applying
    // damage mutates the world while these queries are live.
    let mut landed: Vec<(hecs::Entity, Vec2, i32, u32)> = Vec::new();
    let mut expired: Vec<hecs::Entity> = Vec::new();
    let mut probe: Vec<SolidRect> = Vec::new();

    for (entity, (bolt, lifetime, pos, size, team, vel)) in world
        .query::<(
            &Projectile,
            &mut Lifetime,
            &Position,
            &Size,
            &Team,
            &Velocity,
        )>()
        .iter()
    {
        lifetime.ticks = lifetime.ticks.saturating_sub(1);
        let box_ = Aabb::new(pos.0.x, pos.0.y, size.0.x, size.0.y);

        // A wall. `resolve_move` has already pushed the bolt flush against
        // whatever stopped it and zeroed the velocity component it was
        // travelling on, and nothing else in the tick touches a projectile's
        // velocity — so a bolt that has lost its horizontal speed is a bolt
        // that hit something solid. Checked before the geometry query because
        // it is free and catches the ordinary case.
        let blocked = vel.0.x == 0.0 || {
            geometry.overlapping(box_, &mut probe);
            probe.iter().any(|s| !s.one_way && box_.overlaps(&s.rect))
        };

        let mut hits = 0;
        for (target, (target_pos, target_size, target_team, health)) in
            world.query::<(&Position, &Size, &Team, &Health)>().iter()
        {
            if target == bolt.source || target_team == team || !health.vulnerable() {
                continue;
            }
            let body = Aabb::new(
                target_pos.0.x,
                target_pos.0.y,
                target_size.0.x,
                target_size.0.y,
            );
            if box_.overlaps(&body) {
                landed.push((target, bolt.knockback, bolt.damage, bolt.hitstun));
                hits += 1;
                if !bolt.pierces {
                    break;
                }
            }
        }

        if lifetime.ticks == 0 || blocked || (hits > 0 && !bolt.pierces) {
            expired.push(entity);
        }
    }

    for (target, knockback, damage, hitstun) in landed {
        combat::apply_hit(world, target, damage, knockback, hitstun, events);
    }

    for entity in expired {
        // The one place in the simulation that removes an entity. See the
        // module doc for why this is not the rule everything else follows.
        let _ = world.despawn(entity);
    }
}

/// How many projectiles are in the air, for the trace.
///
/// A count rather than a probe each: a line per projectile per tick makes a
/// trace unreadable, and what a tape wants to say is "the bolt existed" and
/// "it hit", both of which this plus the damage events cover.
pub fn projectile_count(world: &World) -> usize {
    world.query::<&Projectile>().iter().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::StatTable;
    use crate::ecs::components::{AnimationState, Body, Kind};
    use crate::sim::fixture_clips;

    fn spells() -> Arc<SpellTable> {
        SpellTable::shipped()
    }

    fn caster(world: &mut World, x: f32, facing_right: bool) -> hecs::Entity {
        let stats = StatTable::shipped().get("player").unwrap();
        let mut avatar = Avatar::new(stats.avatar());
        avatar.facing_right = facing_right;
        world.spawn((
            avatar,
            Team::Player,
            Casting::default(),
            Mana::new(stats.max_mana, stats.mana_regen),
            Position(Vec2::new(x, 100.0)),
            Velocity(Vec2::ZERO),
            Size(stats.size()),
            Health::new(stats.max_health, stats.iframe_ticks),
            Body::new(Vec2::new(x, 100.0), 0.0, 900.0),
            Sprite {
                clips: Arc::new(fixture_clips()),
                offset: Vec2::ZERO,
            },
            AnimationState::new("idle"),
        ))
    }

    fn victim(world: &mut World, x: f32, hp: i32) -> hecs::Entity {
        let stats = StatTable::shipped().get("knight").unwrap();
        world.spawn((
            Kind("knight".to_string()),
            Team::Enemy,
            Position(Vec2::new(x, 100.0)),
            Velocity(Vec2::ZERO),
            Size(stats.size()),
            Health::new(hp, stats.iframe_ticks),
            Body::new(Vec2::new(x, 100.0), 0.0, 900.0),
        ))
    }

    /// One tick of everything this module owns, plus the movement in between.
    fn tick(world: &mut World, geometry: &[SolidRect], events: &mut Vec<GameEvent>) {
        advance_casts(world, &spells());
        crate::systems::body::move_bodies(world, geometry, crate::sim::TICK);
        resolve_projectiles(world, geometry, events);
    }

    fn shock() -> SpellDef {
        spells().get("shock").expect("shock exists").clone()
    }

    fn hp(world: &World, e: hecs::Entity) -> i32 {
        world.get::<&Health>(e).unwrap().current
    }

    #[test]
    fn a_bolt_travels_the_way_its_caster_faces_at_the_speed_in_the_ron() {
        let SpellEffect::Projectile { speed, .. } = shock().effect;

        for facing_right in [true, false] {
            let mut world = World::new();
            let c = caster(&mut world, 100.0, facing_right);
            world
                .get::<&mut Casting>(c)
                .unwrap()
                .start("shock", shock().cooldown);

            let mut events = Vec::new();
            for _ in 0..(shock().cast_ticks + 1) {
                tick(&mut world, &[], &mut events);
            }

            let (_, (_, pos, vel)) = world
                .query_mut::<(&Projectile, &Position, &Velocity)>()
                .into_iter()
                .next()
                .expect("a bolt was released");
            assert_eq!(vel.0.x.abs(), speed);
            assert_eq!(vel.0.x > 0.0, facing_right);
            assert_eq!(vel.0.y, 0.0, "a bolt flies flat");
            assert!(
                if facing_right {
                    pos.0.x > 100.0
                } else {
                    pos.0.x < 100.0
                },
                "and it left in front of the caster"
            );
        }
    }

    #[test]
    fn a_bolt_damages_an_enemy_once_and_disappears() {
        let mut world = World::new();
        let c = caster(&mut world, 100.0, true);
        let v = victim(&mut world, 200.0, 10);
        world
            .get::<&mut Casting>(c)
            .unwrap()
            .start("shock", shock().cooldown);

        let mut events = Vec::new();
        for _ in 0..60 {
            tick(&mut world, &[], &mut events);
        }

        let SpellEffect::Projectile { damage, .. } = shock().effect;
        assert_eq!(hp(&world, v), 10 - damage, "one hit, not one per tick");
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, GameEvent::Damaged { .. }))
                .count(),
            1
        );
        assert_eq!(projectile_count(&world), 0, "and the bolt is gone");
        assert!(
            world.get::<&Velocity>(v).unwrap().0.x > 0.0,
            "knocked away from the caster"
        );
        assert!(world.get::<&Health>(v).unwrap().hitstun > 0);
    }

    #[test]
    fn a_bolt_stops_at_a_wall() {
        let wall = [SolidRect::solid(Aabb::new(200.0, 0.0, 32.0, 300.0))];
        let mut world = World::new();
        let c = caster(&mut world, 100.0, true);
        world
            .get::<&mut Casting>(c)
            .unwrap()
            .start("shock", shock().cooldown);

        let mut events = Vec::new();
        for _ in 0..60 {
            tick(&mut world, &wall, &mut events);
        }
        assert_eq!(projectile_count(&world), 0, "the wall ate it");
    }

    #[test]
    fn a_bolt_over_open_ground_expires_on_its_lifetime() {
        let SpellEffect::Projectile { lifetime, .. } = shock().effect;
        let mut world = World::new();
        let c = caster(&mut world, 100.0, true);
        world
            .get::<&mut Casting>(c)
            .unwrap()
            .start("shock", shock().cooldown);

        let mut events = Vec::new();
        for _ in 0..shock().cast_ticks {
            tick(&mut world, &[], &mut events);
        }
        assert_eq!(projectile_count(&world), 1, "released");

        // It ages on the tick it is spawned, so it exists for exactly
        // `lifetime` ticks counting that one.
        for _ in 0..(lifetime - 2) {
            tick(&mut world, &[], &mut events);
        }
        assert_eq!(projectile_count(&world), 1, "still flying");
        tick(&mut world, &[], &mut events);
        assert_eq!(projectile_count(&world), 0, "and gone on schedule");
    }

    #[test]
    fn a_bolt_cannot_hit_its_own_team() {
        let mut world = World::new();
        let c = caster(&mut world, 100.0, true);
        let ally = world.spawn((
            Team::Player,
            Position(Vec2::new(200.0, 100.0)),
            Velocity(Vec2::ZERO),
            Size(Vec2::new(20.0, 34.0)),
            Health::new(10, 10),
            Body::new(Vec2::new(200.0, 100.0), 0.0, 900.0),
        ));
        world
            .get::<&mut Casting>(c)
            .unwrap()
            .start("shock", shock().cooldown);

        let mut events = Vec::new();
        for _ in 0..60 {
            tick(&mut world, &[], &mut events);
        }
        assert_eq!(hp(&world, ally), 10);
    }

    /// A bolt is not an NPC, and `Sim::npcs` must never sweep one in — doing
    /// so would renumber every `knight.<n>` in every tape the moment someone
    /// cast a spell.
    #[test]
    fn projectiles_are_never_npcs() {
        let mut sim = crate::sim::Sim::fixture(&["..........", "..P....K..", "##########"]);
        let before = sim.npcs();
        let caster = sim
            .world
            .query::<&Avatar>()
            .iter()
            .map(|(e, _)| e)
            .next()
            .unwrap();
        let clips = sim.world.get::<&Sprite>(caster).unwrap().clips.clone();
        spawn::projectile(
            &mut sim.world,
            caster,
            Vec2::new(100.0, 40.0),
            true,
            &shock(),
            Team::Player,
            clips,
        );

        assert_eq!(projectile_count(&sim.world), 1);
        assert_eq!(sim.npcs(), before, "the bolt is not a knight");
        assert_eq!(sim.npc_probes().len(), before.len());
    }

    /// The property that keeps the world honest: over a sweep of launch
    /// positions and speeds, a bolt never ends a tick inside a solid and
    /// never outlives its lifetime.
    #[test]
    fn a_bolt_never_ends_a_tick_inside_a_solid_or_outlives_itself() {
        let SpellEffect::Projectile { lifetime, .. } = shock().effect;
        let walls: Vec<SolidRect> = (0..6)
            .map(|i| SolidRect::solid(Aabb::new(120.0 + i as f32 * 90.0, 0.0, 32.0, 400.0)))
            .collect();

        for start_x in [40.0f32, 88.0, 119.0, 151.0, 205.0] {
            for speed in [60.0f32, 320.0, 1500.0] {
                let mut world = World::new();
                let c = caster(&mut world, start_x, true);
                let mut def = shock();
                let SpellEffect::Projectile { speed: s, .. } = &mut def.effect;
                *s = speed;
                let clips = world.get::<&Sprite>(c).unwrap().clips.clone();
                spawn::projectile(
                    &mut world,
                    c,
                    Vec2::new(start_x + 20.0, 100.0),
                    true,
                    &def,
                    Team::Player,
                    clips,
                );

                let mut events = Vec::new();
                for tick_no in 0..(lifetime + 10) {
                    crate::systems::body::move_bodies(&mut world, &walls, crate::sim::TICK);
                    resolve_projectiles(&mut world, &walls, &mut events);

                    for (_, (_, pos, size)) in
                        world.query::<(&Projectile, &Position, &Size)>().iter()
                    {
                        let box_ = Aabb::new(pos.0.x, pos.0.y, size.0.x, size.0.y);
                        for wall in &walls {
                            assert!(
                                !overlaps_deeply(&box_, &wall.rect),
                                "start {start_x} speed {speed}: bolt {box_:?} is inside \
                                 {:?} at tick {tick_no}",
                                wall.rect
                            );
                        }
                    }
                    if projectile_count(&world) == 0 {
                        break;
                    }
                }
                assert_eq!(
                    projectile_count(&world),
                    0,
                    "start {start_x} speed {speed}: a bolt outlived its lifetime"
                );
            }
        }
    }

    /// A bolt is drawn out of its *caster's* clip set, and that is not a
    /// stylistic choice — it is what makes the bolt visible at all.
    ///
    /// `AdventureScene::new` uploads one texture per sheet named by any clip
    /// of any entity's clip set, once, when the scene is built. A projectile
    /// is spawned mid-run and would therefore never get its texture uploaded
    /// if it carried a clip set of its own; carrying the caster's means its
    /// sheet was already collected before the level started. Break that chain
    /// — by giving the effect a raw `sheet` and building a set at spawn — and
    /// every bolt in the game silently draws nothing, with no test, no panic
    /// and no trace difference to show for it.
    #[test]
    fn a_projectiles_art_comes_from_a_sheet_the_caster_already_names() {
        let mut assets = crate::assets::Assets::new();
        let player = assets
            .clip_set("player")
            .expect("the player's clip set loads");

        for id in spells().ids() {
            let def = spells().get(id).expect("id came from the table").clone();
            let SpellEffect::Projectile { clip, .. } = &def.effect;
            let bolt = player.clip(clip).unwrap_or_else(|| {
                panic!("spell `{id}` throws clip `{clip}`, which the player's clip set lacks")
            });
            // Resolving is the whole point: `sheet_of` is what the scene calls
            // when it collects textures, and it must not fall over.
            assert!(
                !player.sheet_of(bolt).is_empty(),
                "spell `{id}`'s bolt names no sheet"
            );
            assert!(
                player.clip(&def.clip).is_some(),
                "spell `{id}` casts clip `{}`, which the player's clip set lacks",
                def.clip
            );
        }
    }

    /// Resting flush against a surface is not penetration; anything deeper is.
    fn overlaps_deeply(a: &Aabb, b: &Aabb) -> bool {
        const EPS: f32 = 0.5;
        a.x + EPS < b.right()
            && a.right() - EPS > b.x
            && a.y + EPS < b.bottom()
            && a.bottom() - EPS > b.y
    }

    // --- mana ------------------------------------------------------------

    #[test]
    fn mana_regenerates_to_full_and_stops() {
        let mut mana = Mana::new(5, 12);
        mana.spend(5);
        assert_eq!(mana.current, 0);

        // 1000 thousandths per point at 12 per tick: 84 ticks each.
        for _ in 0..83 {
            mana.regenerate();
        }
        assert_eq!(mana.current, 0, "not yet");
        mana.regenerate();
        assert_eq!(mana.current, 1, "exactly on schedule");

        for _ in 0..(84 * 4) {
            mana.regenerate();
        }
        assert_eq!(mana.current, 5);
        for _ in 0..600 {
            mana.regenerate();
        }
        assert_eq!(mana.current, 5, "and never past the maximum");
        assert_eq!(mana.partial, 0, "with nothing banked for the next spend");
    }

    #[test]
    fn a_pool_with_no_regen_never_refills() {
        let mut mana = Mana::new(5, 0);
        mana.spend(2);
        for _ in 0..600 {
            mana.regenerate();
        }
        assert_eq!(mana.current, 3);
    }

    // --- refusal ---------------------------------------------------------

    #[test]
    fn a_cast_is_refused_for_exactly_one_reason_at_a_time() {
        let table = spells();
        let ready = Casting::default();
        let full = Mana::new(5, 12);

        assert_eq!(refusal(&table, "shock", &ready, &full, false), None);
        assert_eq!(
            refusal(&table, "shock", &ready, &full, true),
            Some(CastFailure::Busy)
        );
        assert_eq!(
            refusal(&table, "nonesuch", &ready, &full, false),
            Some(CastFailure::Unknown)
        );

        let cooling = Casting {
            cooldown: 10,
            ..Default::default()
        };
        assert_eq!(
            refusal(&table, "shock", &cooling, &full, false),
            Some(CastFailure::Cooling)
        );

        let mut empty = Mana::new(5, 12);
        empty.spend(5);
        assert_eq!(
            refusal(&table, "shock", &ready, &empty, false),
            Some(CastFailure::NoMana)
        );

        let mut mid_cast = Casting::default();
        mid_cast.start("shock", 45);
        assert_eq!(
            refusal(&table, "shock", &mid_cast, &full, false),
            Some(CastFailure::Busy)
        );
    }

    /// Being hit drops the cast, the way it drops a swing. The mana stays
    /// spent — that is the whole risk of a long cast.
    #[test]
    fn a_hit_interrupts_a_cast_and_the_mana_is_still_gone() {
        let mut world = World::new();
        let c = caster(&mut world, 100.0, true);
        world.get::<&mut Mana>(c).unwrap().spend(2);
        world
            .get::<&mut Casting>(c)
            .unwrap()
            .start("shock", shock().cooldown);

        let mut events = Vec::new();
        tick(&mut world, &[], &mut events);
        world.get::<&mut Health>(c).unwrap().hitstun = 10;
        tick(&mut world, &[], &mut events);

        assert!(!world.get::<&Casting>(c).unwrap().busy());
        assert_eq!(projectile_count(&world), 0, "no bolt was ever released");
        assert_eq!(world.get::<&Mana>(c).unwrap().current, 3, "still paid");
        assert!(
            world.get::<&Casting>(c).unwrap().cooldown > 0,
            "and the cooldown is still running: interrupting is not a way out"
        );
    }
}
