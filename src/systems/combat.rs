//! Swings, hits, damage, and death.
//!
//! Runs *after* [`crate::systems::body`], so hitboxes are tested where things
//! actually ended the tick rather than where they were when the controller
//! made its decision. With a hitbox live for several ticks and bodies moving
//! at up to 900 px/s, that difference is the whole fight.
//!
//! Nothing here mutates health directly from a controller. A swing produces a
//! hit, a hit produces damage, and damage produces an event — so every blow
//! lands in the trace and a tape can say `expect damaged == 3` without
//! guessing which tick it happened on.

use ggez::glam::Vec2;
use hecs::World;

use crate::assets::AttackTable;
use crate::ecs::components::{
    Attacking, Avatar, Body, Health, Kind, Patrol, Position, Size, Team, Velocity,
};
use crate::physics::Aabb;
use crate::sim::event::{DeathCause, GameEvent};

/// Count down the per-tick combat timers.
///
/// Separate from [`resolve`] and run at the *start* of the tick, before any
/// controller reads them: a controller asking "am I stunned?" must see this
/// tick's value, not last tick's.
pub fn tick_timers(world: &mut World) {
    for (_, health) in world.query_mut::<&mut Health>() {
        health.iframes = health.iframes.saturating_sub(1);
        health.hitstun = health.hitstun.saturating_sub(1);
    }
}

/// Advance any swing in progress, and end it when its duration is up.
///
/// Also at the start of the tick, so that the elapsed count a hitbox is tested
/// against is this tick's.
pub fn advance_attacks(world: &mut World, attacks: &AttackTable, events: &mut Vec<GameEvent>) {
    for (_, (attacking, health)) in world.query_mut::<(&mut Attacking, Option<&Health>)>() {
        // (`Casting` is advanced by `crate::systems::spell`, in this same
        // phase and for the same reasons.)
        // Dying mid-swing drops the sword, and so does being hit during one.
        //
        // Interrupting on hitstun is what makes an exchange readable: whoever
        // connects first wins it, so closing in on a knight already committed
        // to a wind-up is a real opening rather than a mutual trade. It cuts
        // both ways — the player's swing dies to a knight's the same way.
        if health.is_some_and(|h| h.dead() || h.hitstun > 0) {
            attacking.stop();
            continue;
        }
        let Some(id) = attacking.attack.clone() else {
            continue;
        };
        let Some(def) = attacks.get(&id) else {
            attacking.stop();
            continue;
        };

        attacking.elapsed += 1;

        // A buffered press taken during the chain window starts the next link
        // the moment this one's animation ends -- so a combo reads as one
        // continuous motion rather than three separate swings.
        if def.finished(attacking.elapsed) {
            match attacking.chained.take().filter(|_| def.chains()) {
                Some(next) => {
                    attacking.start(&next);
                    // Each link announces itself, so a tape can tell a combo
                    // from three separate swings.
                    events.push(GameEvent::Attacked { attack: next });
                }
                None if def.released(attacking.elapsed) => attacking.stop(),
                None => {}
            }
        }
    }
}

/// Test every live hitbox against everything on the other team, and apply what
/// lands.
pub fn resolve(world: &mut World, attacks: &AttackTable, events: &mut Vec<GameEvent>) {
    // Collect first, mutate second: the borrow checker will not allow querying
    // targets while an attacker is mutably borrowed, and collecting also fixes
    // the order in which simultaneous hits resolve.
    let mut landed: Vec<(hecs::Entity, hecs::Entity, String)> = Vec::new();

    for (attacker, (attacking, pos, size, team)) in world
        .query::<(&Attacking, &Position, &Size, &Team)>()
        .iter()
    {
        let Some(id) = &attacking.attack else {
            continue;
        };
        let Some(def) = attacks.get(id) else { continue };
        if !def.is_active(attacking.elapsed) {
            continue;
        }

        let facing = facing_right(world, attacker);
        let rect = def.hitbox(pos.0, size.0, facing);
        let hitbox = Aabb::new(rect.x, rect.y, rect.w, rect.h);

        for (target, (target_pos, target_size, target_team, health)) in
            world.query::<(&Position, &Size, &Team, &Health)>().iter()
        {
            if target_team == team || !health.vulnerable() {
                continue;
            }
            if attacking.hit.contains(&target) {
                continue;
            }
            let body = Aabb::new(
                target_pos.0.x,
                target_pos.0.y,
                target_size.0.x,
                target_size.0.y,
            );
            if hitbox.overlaps(&body) {
                landed.push((attacker, target, id.clone()));
            }
        }
    }

    for (attacker, target, id) in landed {
        let Some(def) = attacks.get(&id) else {
            continue;
        };
        let facing = facing_right(world, attacker);
        let impulse = def.impulse_on(centre_x(world, attacker), centre_x(world, target), facing);

        // Record the hit on the attacker so this swing cannot land twice.
        if let Ok(mut attacking) = world.get::<&mut Attacking>(attacker) {
            attacking.hit.push(target);
        }

        apply_hit(world, target, def.damage, impulse, def.hitstun, events);
    }
}

/// Take `damage` off `target`, throw it, stun it, and say so.
///
/// The single path from "something connected" to "something is hurt". A sword
/// and a spell bolt are entirely different systems above this line and must be
/// exactly the same below it — two damage routines that can drift apart is the
/// bug [`Health`] was made shared to avoid, and it shows up as an enemy that
/// dies to one weapon and not the other for reasons nobody can find.
///
/// Returns whether the hit actually landed: an earlier hit on the same tick
/// may have granted i-frames or killed the target outright.
pub fn apply_hit(
    world: &mut World,
    target: hecs::Entity,
    damage: i32,
    impulse: Vec2,
    hitstun: u32,
    events: &mut Vec<GameEvent>,
) -> bool {
    let who = kind_of(world, target);
    let mut died = false;

    if let Ok(mut health) = world.get::<&mut Health>(target) {
        if !health.vulnerable() {
            return false;
        }
        health.current -= damage;
        health.iframes = health.iframe_ticks;
        health.hitstun = hitstun;
        died = health.dead();

        events.push(GameEvent::Damaged {
            who: who.clone(),
            amount: damage,
            remaining: health.current.max(0),
        });
    }

    if let Ok(mut vel) = world.get::<&mut Velocity>(target) {
        vel.0 = impulse;
    }

    if died {
        events.push(GameEvent::Died {
            who,
            cause: DeathCause::Slain,
        });
    }
    true
}

/// The horizontal centre of an entity's collider, or 0 for something that has
/// no box. Only [`crate::assets::HitboxAnchor::Down`] reads it, to decide
/// which way "away from the impact" points.
fn centre_x(world: &World, entity: hecs::Entity) -> f32 {
    let Ok(pos) = world.get::<&Position>(entity) else {
        return 0.0;
    };
    let half = world
        .get::<&Size>(entity)
        .map(|s| s.0.x / 2.0)
        .unwrap_or(0.0);
    pos.0.x + half
}

/// Stop the dead: a corpse keeps its position but does nothing further.
///
/// Deliberately not a despawn. NPCs are addressed by spawn index in traces and
/// tape assertions, and removing one would silently renumber every knight
/// after it — turning a passing assertion into a passing assertion about a
/// different entity. Corpses are cheap; renumbering is not.
pub fn settle_dead(world: &mut World) {
    for (_, (health, body, vel)) in world.query_mut::<(&Health, &mut Body, &mut Velocity)>() {
        if !health.dead() {
            continue;
        }
        // Let the knockback play out, then stay put.
        if body.grounded {
            vel.0 = Vec2::ZERO;
            body.frozen = true;
        }
    }
}

/// What to call an entity in an event. NPCs carry their map kind; the player
/// has no `Kind`, because nothing places it from data.
fn kind_of(world: &World, entity: hecs::Entity) -> String {
    world
        .get::<&Kind>(entity)
        .map(|k| k.0.clone())
        .unwrap_or_else(|_| "player".to_string())
}

/// Which way an attacker is facing, for aiming its hitbox.
fn facing_right(world: &World, entity: hecs::Entity) -> bool {
    if let Ok(avatar) = world.get::<&Avatar>(entity) {
        return avatar.facing_right;
    }
    if let Ok(patrol) = world.get::<&Patrol>(entity) {
        return patrol.dir >= 0.0;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{AttackDef, AttackTable, StatTable};
    use crate::ecs::components::Kind;
    use std::collections::HashMap;

    const SIZE: Vec2 = Vec2::new(20.0, 30.0);

    /// How long a kind is invulnerable after a hit, from
    /// `assets/data/stats.ron`. The player's window and an enemy's are
    /// deliberately different lengths, and these tests turn on that
    /// difference — so they read the shipped numbers rather than pick their
    /// own.
    fn iframes(kind: &str) -> u32 {
        StatTable::shipped()
            .get(kind)
            .unwrap_or_else(|e| panic!("{e:#}"))
            .iframe_ticks
    }

    fn table() -> AttackTable {
        let mut map = HashMap::new();
        map.insert(
            "swing".to_string(),
            AttackDef {
                clip: "attack".to_string(),
                duration: 10,
                active: (2, 5),
                recovery: 0,
                chain: None,
                anchor: Default::default(),
                offset: (14.0, 0.0),
                size: (24.0, 24.0),
                damage: 2,
                knockback: (100.0, -50.0),
                hitstun: 6,
            },
        );
        AttackTable(map)
    }

    fn attacker(world: &mut World, x: f32, facing_right: bool) -> hecs::Entity {
        let stats = StatTable::shipped().get("player").unwrap();
        let mut avatar = Avatar::new(stats.avatar());
        avatar.facing_right = facing_right;
        world.spawn((
            avatar,
            Team::Player,
            Attacking::default(),
            Position(Vec2::new(x, 100.0)),
            Velocity(Vec2::ZERO),
            Size(SIZE),
            Health::new(5, iframes("player")),
            Body::new(Vec2::new(x, 100.0), 0.0, 900.0),
        ))
    }

    fn victim(world: &mut World, x: f32, hp: i32) -> hecs::Entity {
        victim_with(world, x, hp, iframes("player"))
    }

    fn victim_with(world: &mut World, x: f32, hp: i32, iframes: u32) -> hecs::Entity {
        world.spawn((
            Kind("knight".to_string()),
            Team::Enemy,
            Position(Vec2::new(x, 100.0)),
            Velocity(Vec2::ZERO),
            Size(SIZE),
            Health::new(hp, iframes),
            Body::new(Vec2::new(x, 100.0), 0.0, 900.0),
        ))
    }

    /// Run `ticks` of the combat phases, returning everything that happened.
    fn run(world: &mut World, ticks: u32) -> Vec<GameEvent> {
        let attacks = table();
        let mut events = Vec::new();
        for _ in 0..ticks {
            tick_timers(world);
            advance_attacks(world, &attacks, &mut events);
            resolve(world, &attacks, &mut events);
            settle_dead(world);
        }
        events
    }

    fn hp(world: &World, e: hecs::Entity) -> i32 {
        world.get::<&Health>(e).unwrap().current
    }

    #[test]
    fn a_swing_in_range_damages_once_despite_a_multi_tick_hitbox() {
        let mut world = World::new();
        let a = attacker(&mut world, 100.0, true);
        let v = victim(&mut world, 122.0, 10);
        world.get::<&mut Attacking>(a).unwrap().start("swing");

        let events = run(&mut world, 10);

        assert_eq!(hp(&world, v), 8, "one hit for 2, not one per active tick");
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, GameEvent::Damaged { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn a_swing_out_of_range_does_nothing() {
        let mut world = World::new();
        let a = attacker(&mut world, 100.0, true);
        let v = victim(&mut world, 400.0, 10);
        world.get::<&mut Attacking>(a).unwrap().start("swing");

        run(&mut world, 10);
        assert_eq!(hp(&world, v), 10);
    }

    /// The hitbox mirrors with the attacker, so facing matters.
    #[test]
    fn a_swing_only_reaches_the_way_the_attacker_faces() {
        let mut world = World::new();
        let a = attacker(&mut world, 100.0, false); // facing left
        let behind = victim(&mut world, 122.0, 10); // to the right
        let ahead = victim(&mut world, 78.0, 10); // to the left
        world.get::<&mut Attacking>(a).unwrap().start("swing");

        run(&mut world, 10);
        assert_eq!(hp(&world, behind), 10, "nothing behind the swing");
        assert_eq!(hp(&world, ahead), 8, "the one in front takes it");
    }

    /// Per-entity i-frames are what make a combo work. An enemy's window is
    /// deliberately shorter than the gap between combo links, so the second
    /// and third hits land; the player's is long enough to break a chain.
    #[test]
    fn a_short_i_frame_window_lets_a_follow_up_land() {
        let mut world = World::new();
        let a = attacker(&mut world, 100.0, true);
        let v = victim_with(&mut world, 122.0, 10, iframes("knight"));

        world.get::<&mut Attacking>(a).unwrap().start("swing");
        run(&mut world, 10);
        assert_eq!(hp(&world, v), 8);

        // straight into another swing: the short window has lapsed by the
        // time this one's hitbox is live
        world.get::<&mut Attacking>(a).unwrap().start("swing");
        run(&mut world, 10);
        assert_eq!(hp(&world, v), 6, "the follow-up connected");
    }

    #[test]
    fn i_frames_stop_a_second_swing_landing_immediately() {
        let mut world = World::new();
        let a = attacker(&mut world, 100.0, true);
        let v = victim(&mut world, 122.0, 10);

        world.get::<&mut Attacking>(a).unwrap().start("swing");
        run(&mut world, 10);
        assert_eq!(hp(&world, v), 8);

        // straight into another swing, well inside the i-frame window
        world.get::<&mut Attacking>(a).unwrap().start("swing");
        run(&mut world, 10);
        assert_eq!(hp(&world, v), 8, "still invulnerable");

        // and once it expires, the next one lands
        run(&mut world, iframes("player"));
        world.get::<&mut Attacking>(a).unwrap().start("swing");
        run(&mut world, 10);
        assert_eq!(hp(&world, v), 6);
    }

    #[test]
    fn a_hit_knocks_back_and_stuns() {
        let mut world = World::new();
        let a = attacker(&mut world, 100.0, true);
        let v = victim(&mut world, 122.0, 10);
        world.get::<&mut Attacking>(a).unwrap().start("swing");

        run(&mut world, 5);

        let vel = world.get::<&Velocity>(v).unwrap().0;
        assert!(vel.x > 0.0, "knocked away from the attacker");
        assert!(vel.y < 0.0, "and popped upward");
        assert!(world.get::<&Health>(v).unwrap().hitstun > 0);
    }

    #[test]
    fn friendly_fire_is_impossible() {
        let mut world = World::new();
        let a = attacker(&mut world, 100.0, true);
        // a second player-team entity right where the hitbox is
        let ally = world.spawn((
            Team::Player,
            Position(Vec2::new(122.0, 100.0)),
            Velocity(Vec2::ZERO),
            Size(SIZE),
            Health::new(10, iframes("knight")),
            Body::new(Vec2::new(122.0, 100.0), 0.0, 900.0),
        ));
        world.get::<&mut Attacking>(a).unwrap().start("swing");

        run(&mut world, 10);
        assert_eq!(hp(&world, ally), 10);
    }

    #[test]
    fn enough_hits_kill_and_the_corpse_stays_put() {
        let mut world = World::new();
        let a = attacker(&mut world, 100.0, true);
        let v = victim(&mut world, 122.0, 4);

        let mut events = Vec::new();
        for _ in 0..3 {
            world.get::<&mut Attacking>(a).unwrap().start("swing");
            events.extend(run(&mut world, 10));
            events.extend(run(&mut world, iframes("player")));
        }

        assert!(
            hp(&world, v) <= 0,
            "four hp, two damage a hit, three swings"
        );
        assert!(events.iter().any(|e| matches!(
            e,
            GameEvent::Died {
                cause: DeathCause::Slain,
                ..
            }
        )));
        assert!(
            world.contains(v),
            "the corpse is not despawned — spawn indices must stay stable"
        );
    }

    // --- the `Down` hitbox anchor ----------------------------------------

    /// The shipped plunge, so this tests the attack the game actually has.
    fn plunge() -> AttackDef {
        let stats = StatTable::shipped().get("player").unwrap();
        crate::assets::Assets::new()
            .attacks()
            .unwrap()
            .get(&stats.avatar().plunge_attack)
            .expect("the plunge is in assets/data/attacks.ron")
            .clone()
    }

    /// A `Down` box is centred on the attacker and does not mirror. A
    /// facing-mirrored offset can be *made* symmetric for one collider width,
    /// which is exactly why the anchor is named rather than arithmetic — this
    /// is the test that would fail if someone reverted it to a clever number.
    #[test]
    fn a_down_anchored_hitbox_is_centred_and_never_mirrors() {
        let def = plunge();
        let pos = Vec2::new(100.0, 50.0);

        let right = def.hitbox(pos, SIZE, true);
        let left = def.hitbox(pos, SIZE, false);
        assert_eq!(
            (right.x, right.y),
            (left.x, left.y),
            "facing changes nothing"
        );
        assert_eq!(
            right.x + right.w / 2.0,
            pos.x + SIZE.x / 2.0,
            "centred on the attacker"
        );
        assert!(right.y > pos.y, "and underneath it");

        // ...and it stays centred on a differently-sized attacker, which the
        // arithmetic version could not manage.
        let wide = Vec2::new(40.0, 30.0);
        let box_ = def.hitbox(pos, wide, true);
        assert_eq!(box_.x + box_.w / 2.0, pos.x + wide.x / 2.0);
    }

    /// The blow throws whatever it landed on *away from the impact* and up,
    /// rather than the way the attacker happens to be looking. Landing on
    /// something and having it fly through you is what this prevents.
    #[test]
    fn a_plunge_throws_its_victim_away_from_the_impact_and_up() {
        let def = plunge();
        for facing_right in [true, false] {
            let to_the_right = def.impulse_on(100.0, 140.0, facing_right);
            let to_the_left = def.impulse_on(100.0, 60.0, facing_right);
            assert!(to_the_right.x > 0.0, "thrown right, facing {facing_right}");
            assert!(to_the_left.x < 0.0, "thrown left, facing {facing_right}");
            assert!(to_the_right.y < 0.0 && to_the_left.y < 0.0, "and upward");
        }
    }

    /// A swing is unchanged: it throws the way it was swung, whatever side of
    /// the attacker the victim ended up on.
    #[test]
    fn a_facing_anchored_swing_still_throws_the_way_it_was_swung() {
        let table = table();
        let def = table.get("swing").unwrap();
        assert!(def.impulse_on(100.0, 60.0, true).x > 0.0);
        assert!(def.impulse_on(100.0, 140.0, false).x < 0.0);
    }

    /// A dying attacker drops its swing rather than landing a hit from beyond
    /// the grave.
    #[test]
    fn dying_mid_swing_cancels_the_attack() {
        let mut world = World::new();
        let a = attacker(&mut world, 100.0, true);
        let v = victim(&mut world, 122.0, 10);
        world.get::<&mut Attacking>(a).unwrap().start("swing");
        world.get::<&mut Health>(a).unwrap().current = 0;

        run(&mut world, 10);
        assert_eq!(hp(&world, v), 10);
        assert!(!world.get::<&Attacking>(a).unwrap().busy());
    }
}
