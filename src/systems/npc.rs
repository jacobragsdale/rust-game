//! NPC controllers. Like the player's, these decide a velocity and hand the
//! rest to [`crate::systems::body`] — which is the whole point of M1: an NPC
//! falls, lands, and collides using exactly the code the player does, so the
//! two can never drift apart.
//!
//! One brain for every kind, steered by its stat block rather than by a branch
//! per kind: what a kind *can* do — swing its `attack`, throw its `spell`,
//! fly — is what its block says it has, and the state machine below simply
//! skips the moves a kind lacks. A new enemy is therefore numbers and art.

use ggez::glam::Vec2;
use hecs::World;

use crate::assets::{AiStats, SpellTable};
use crate::ecs::components::{
    Attacking, Avatar, Body, Casting, DerivedStats, Health, Hostile, Mana, Patrol, Position, Size,
    Stance, Team, Velocity,
};
use crate::physics::{Aabb, SolidQuery, SolidRect};
use crate::sim::event::GameEvent;
use crate::systems::spell;

/// Decide what every NPC does this tick.
///
/// Entities with only a [`Patrol`] walk their route. Entities that also have a
/// [`Hostile`] run the full state machine on top of it: patrol until the player
/// is in front of them, chase, swing — or cast — when close enough, and go
/// home when the player gets away.
///
/// A flyer (`ai.flying`) runs the same machine in two dimensions: it keeps
/// station at home instead of pacing, sees all round rather than only ahead,
/// chases straight at the player, and breaks off while its cooldown runs —
/// which, after a bite, is the swoop.
///
/// Runs in phase 1 alongside `avatar::control`, and like it only sets a
/// velocity and `Body` knobs — an NPC moves through exactly the same
/// `move_bodies` the player does, which is the point of the M1 split.
pub fn think<Q: SolidQuery + ?Sized>(
    world: &mut World,
    geometry: &Q,
    spells: &SpellTable,
    events: &mut Vec<GameEvent>,
) {
    let avatars = avatars(world);

    let mut swings: Vec<(hecs::Entity, String)> = Vec::new();
    let mut casts: Vec<(hecs::Entity, String)> = Vec::new();

    #[allow(clippy::type_complexity)]
    for (
        entity,
        (patrol, hostile, pos, vel, size, body, health, attacking, stats, casting, mana, team),
    ) in world
        .query::<(
            &mut Patrol,
            Option<&mut Hostile>,
            &Position,
            &mut Velocity,
            &Size,
            &mut Body,
            &Health,
            &Attacking,
            &DerivedStats,
            Option<&Casting>,
            Option<&Mana>,
            Option<&Team>,
        )>()
        .iter()
    {
        let target = nearest(&avatars, team.copied(), pos.0 + size.0 / 2.0);
        // Sight, reach, patience and how far it peers over a ledge are all
        // per-kind data — `assets/data/stats.ron`, the `ai` group — read
        // through the derived block, so an enemy that ever wears something
        // gets the benefit of it without a change here.
        let ai = stats.0.ai();

        // A flyer holds itself up, and rises through planks, for as long as
        // it lives. Knobs are set every tick, so the tick it dies is the tick
        // it starts to fall like anything else.
        if ai.flying {
            let alive = !health.dead();
            body.gravity = if alive { 0.0 } else { stats.0.gravity };
            body.ignore_one_way = alive;
        }

        if body.frozen || health.dead() {
            continue;
        }
        // Knocked about: keep flying, no steering. Same rule as the player —
        // but a blow is also news. Something that hits a patrolling knight
        // from behind has been noticed, and the chase starts the moment the
        // stun lets go rather than never: six hits in the back used to kill
        // a knight that went on patrolling the whole time.
        if health.hitstun > 0 {
            if let Some(hostile) = hostile {
                if matches!(hostile.stance, Stance::Patrol | Stance::Return) {
                    hostile.stance = Stance::Chase;
                }
            }
            continue;
        }

        let Some(hostile) = hostile else {
            if ai.flying {
                vel.0 = Vec2::ZERO; // nowhere to be: hang in the air
            } else {
                walk(patrol, pos.0, size.0, vel, body, geometry, ai, patrol.speed);
            }
            continue;
        };
        hostile.cooldown = hostile.cooldown.saturating_sub(1);

        // A swing or a cast in progress owns the entity until it finishes.
        if attacking.busy() || casting.is_some_and(Casting::busy) {
            hostile.stance = Stance::Attack;
            halt(vel, ai);
            continue;
        }

        let seen = target
            .and_then(|t| sighted(pos.0, size.0, patrol.dir, t, ai))
            .filter(|_| target.is_some_and(|t| in_view(pos.0, size.0, t, geometry)));
        // Close enough to swing — for a kind with something to swing.
        let reachable = target
            .filter(|_| hostile.attack.is_some())
            .and_then(|t| within(pos.0, size.0, t, ai.reach, ai.sight_height));
        // Where the player is, for as long as it is worth chasing them at all.
        // Looser vertically than sight, so jumping in place does not read as
        // escaping: a jump is 100px of air, and `sight_height` is a body.
        let chasing = target.and_then(|t| within(pos.0, size.0, t, ai.lose, ai.lose_height));
        // In range of its spell with a clear line to throw it down: which way,
        // what, and whether it can go now or has to wait for mana or cooldown.
        let castable = match (&stats.0.spell, casting, mana, target) {
            (Some(spell), Some(casting), Some(mana), Some(t)) if ai.cast_range > 0.0 => {
                within(pos.0, size.0, t, ai.cast_range, ai.sight_height)
                    .filter(|_| in_view(pos.0, size.0, t, geometry))
                    .map(|toward| {
                        let ready = spell::refusal(spells, spell, casting, mana, false).is_none();
                        (toward, spell, ready)
                    })
            }
            _ => None,
        };

        hostile.stance = match hostile.stance {
            Stance::Patrol | Stance::Return if seen.is_some() => Stance::Chase,
            Stance::Patrol => Stance::Patrol,
            // A finished swing drops back to chasing, and re-evaluates from
            // there — otherwise a killed player leaves the knight swinging at
            // the spot they died on.
            Stance::Chase | Stance::Attack => match chasing {
                Some(_) => Stance::Chase,
                None => Stance::Return,
            },
            Stance::Return => Stance::Return,
        };

        match hostile.stance {
            Stance::Patrol if ai.flying => {
                fly_home(hostile, patrol, pos.0, size.0, vel, geometry, ai);
            }
            Stance::Patrol => walk(patrol, pos.0, size.0, vel, body, geometry, ai, patrol.speed),
            Stance::Chase => {
                // Face the player whichever side they are on. Sight is a box
                // in front, but once a chase is on the knight knows where you
                // went: a player who slipped behind it used to leave it
                // standing in Chase facing the wall for as long as they liked.
                if let Some(toward) = chasing {
                    patrol.dir = toward;
                }
                if reachable.is_some() {
                    // In reach: swing when ready, and otherwise hold ground
                    // rather than walking into the player, which flipped its
                    // facing every tick their centres crossed.
                    halt(vel, ai);
                    if let (0, Some(attack)) = (hostile.cooldown, &hostile.attack) {
                        swings.push((entity, attack.clone()));
                        hostile.stance = Stance::Attack;
                        hostile.cooldown = ai.cooldown;
                    }
                } else if let Some((toward, spell, ready)) =
                    castable.filter(|(_, _, ready)| *ready || hostile.attack.is_none())
                {
                    // In range of its spell: a caster keeps its distance
                    // rather than closing, and throws when it can. One with a
                    // sword as well only stands off while it has a spell to
                    // throw — out of mana, it comes to use the sword.
                    patrol.dir = toward;
                    halt(vel, ai);
                    if ready {
                        casts.push((entity, spell.clone()));
                        hostile.stance = Stance::Attack;
                    }
                } else if ai.flying {
                    let speed = patrol.speed * ai.chase_multiplier;
                    vel.0 = target
                        .filter(|_| chasing.is_some())
                        .map_or(Vec2::ZERO, |t| {
                            swoop(pos.0, size.0, t, hostile.cooldown) * speed
                        });
                } else if let Some(toward) = chasing {
                    // A biter that has just bitten gives ground until it can
                    // bite again, still facing you — a wolf's lunge and circle,
                    // where a bat has its swoop.
                    let resting = stats.0.contact.is_some() && hostile.cooldown > 0;
                    let (way, pace) = if resting {
                        (-toward, BACK_OFF)
                    } else {
                        (toward, 1.0)
                    };
                    // Close, but still refuse to walk off a ledge.
                    let speed = patrol.speed * ai.chase_multiplier * pace;
                    if body.grounded && !floor_ahead(pos.0, size.0, way, geometry, ai) {
                        vel.0.x = 0.0;
                    } else {
                        vel.0.x = way * speed;
                    }
                } else {
                    vel.0.x = 0.0;
                }
            }
            Stance::Attack => halt(vel, ai),
            Stance::Return if ai.flying => {
                if fly_home(hostile, patrol, pos.0, size.0, vel, geometry, ai) {
                    hostile.stance = Stance::Patrol;
                }
            }
            Stance::Return => {
                let toward = (hostile.home.x - pos.0.x).signum();
                let blocked = body.grounded
                    && (wall_ahead(pos.0, size.0, toward, geometry, ai)
                        || !floor_ahead(pos.0, size.0, toward, geometry, ai));
                if (hostile.home.x - pos.0.x).abs() <= ai.home_slack {
                    hostile.stance = Stance::Patrol;
                    vel.0.x = 0.0;
                } else if blocked {
                    // Home is across something it cannot walk over — a
                    // finisher knocked it off its ledge. Where it stands is
                    // home now; aiming at the old one flipped it back and forth
                    // against the wall every tick, forever.
                    hostile.home = pos.0;
                    hostile.stance = Stance::Patrol;
                    vel.0.x = 0.0;
                } else {
                    patrol.dir = toward;
                    walk(patrol, pos.0, size.0, vel, body, geometry, ai, patrol.speed);
                }
            }
        }
    }

    // Spawn order, so two knights swinging on one tick are announced — and
    // started — in the same order every run.
    swings.sort_by_key(|(entity, _)| entity.id());
    for (entity, attack) in swings {
        if let Ok(mut attacking) = world.get::<&mut Attacking>(entity) {
            attacking.start(&attack);
            events.push(GameEvent::Attacked { attack });
        }
    }
    // Then the casts, the same way the player's is started: the mana goes and
    // the cooldown starts now, and the bolt follows when the spell says.
    casts.sort_by_key(|(entity, _)| entity.id());
    for (entity, id) in casts {
        let Some(def) = spells.get(&id) else { continue };
        if let Ok((casting, mana)) = world.query_one_mut::<(&mut Casting, &mut Mana)>(entity) {
            mana.spend(def.cost);
            casting.start(&id, def.cooldown);
            events.push(GameEvent::SpellCast { spell: id });
        }
    }
}

/// How fast a biter backs off after a bite, as a share of its chase.
const BACK_OFF: f32 = 0.6;

/// Stand still: a walker keeps falling, a flyer simply hangs.
fn halt(vel: &mut Velocity, ai: &AiStats) {
    if ai.flying {
        vel.0 = Vec2::ZERO;
    } else {
        vel.0.x = 0.0;
    }
}

/// Which way a flyer goes after the player: straight at them — or, with its
/// cooldown running after a bite, away from them and up, so the next pass is
/// a swoop rather than a bat stuck to the player's face. A unit vector.
fn swoop(pos: Vec2, size: Vec2, target: (Vec2, Vec2), cooldown: u32) -> Vec2 {
    let me = pos + size / 2.0;
    let them = target.0 + target.1 / 2.0;
    if cooldown > 0 {
        let away = if me.x >= them.x { 1.0 } else { -1.0 };
        Vec2::new(away, -1.0).normalize()
    } else {
        (them - me).normalize_or_zero()
    }
}

/// Fly straight back to `home` and keep station there, which is the whole of
/// a flyer's patrol. Returns whether it is home.
///
/// A home it cannot see is a home it cannot fly to in a straight line — a
/// knockback carried it round a corner — so where it is becomes home, the
/// same way a walker gives up on a home across a drop.
fn fly_home<Q: SolidQuery + ?Sized>(
    hostile: &mut Hostile,
    patrol: &mut Patrol,
    pos: Vec2,
    size: Vec2,
    vel: &mut Velocity,
    geometry: &Q,
    ai: &AiStats,
) -> bool {
    let to_home = hostile.home - pos;
    if to_home.length() <= ai.home_slack {
        vel.0 = Vec2::ZERO;
        return true;
    }
    if !in_view(pos, size, (hostile.home, size), geometry) {
        hostile.home = pos;
        vel.0 = Vec2::ZERO;
        return true;
    }
    if to_home.x != 0.0 {
        patrol.dir = to_home.x.signum();
    }
    vel.0 = to_home.normalize() * patrol.speed;
    false
}

/// Walk in `patrol.dir`, turning at anything that stops you.
///
/// Two reasons to turn: a wall ahead, or no floor ahead. The second is what
/// keeps a walker on its ledge instead of marching off it — and it has to be a
/// look-ahead probe rather than a reaction to falling, because by the time the
/// body is airborne it is already too late to not have walked off.
#[allow(clippy::too_many_arguments)]
fn walk<Q: SolidQuery + ?Sized>(
    patrol: &mut Patrol,
    pos: Vec2,
    size: Vec2,
    vel: &mut Velocity,
    body: &Body,
    geometry: &Q,
    ai: &AiStats,
    speed: f32,
) {
    // Airborne walkers keep their horizontal speed and do not steer: turning
    // mid-air would let one walk off a ledge and immediately scuttle back on.
    if body.grounded
        && (wall_ahead(pos, size, patrol.dir, geometry, ai)
            || !floor_ahead(pos, size, patrol.dir, geometry, ai))
    {
        patrol.dir = -patrol.dir;
    }
    vel.0.x = patrol.dir * speed;
}

/// Every living avatar's side and collider, in entity order — what an enemy
/// can hunt.
///
/// `Avatar` rather than `Team::Player`, and the distinction matters: M5's
/// villager is on the player's team so that friendly fire cannot touch it, so
/// "the first player-team body" would have a knight chasing the herbalist. An
/// avatar is something a player — or a rival's brain — actually drives.
fn avatars(world: &World) -> Vec<(Option<Team>, Vec2, Vec2)> {
    let mut found: Vec<(u32, Option<Team>, Vec2, Vec2)> = world
        .query::<(&Avatar, &Position, &Size, &Health, Option<&Team>)>()
        .iter()
        .filter(|(_, (_, _, _, health, _))| !health.dead())
        .map(|(entity, (_, pos, size, _, team))| (entity.id(), team.copied(), pos.0, size.0))
        .collect();
    found.sort_by_key(|(id, ..)| *id);
    found.into_iter().map(|(_, t, p, s)| (t, p, s)).collect()
}

/// The collider of the nearest avatar not on `team`'s side, measured from
/// `centre`. A rival is an avatar too, and a knight does not hunt its own.
fn nearest(
    avatars: &[(Option<Team>, Vec2, Vec2)],
    team: Option<Team>,
    centre: Vec2,
) -> Option<(Vec2, Vec2)> {
    avatars
        .iter()
        .filter(|(theirs, ..)| team.is_none() || *theirs != team)
        .map(|(_, pos, size)| (*pos, *size))
        .min_by(|a, b| {
            let da = (a.0 + a.1 / 2.0).distance_squared(centre);
            let db = (b.0 + b.1 / 2.0).distance_squared(centre);
            da.total_cmp(&db)
        })
}

/// Is the player in the box in front of this NPC? Returns the direction to
/// them if so.
///
/// In *front* rather than in a radius, so that walking up behind a patrolling
/// knight goes unnoticed — which is the difference between an enemy and a
/// proximity trigger. A flyer is the exception: hanging in the air, it sees
/// all round.
fn sighted(pos: Vec2, size: Vec2, facing: f32, target: (Vec2, Vec2), ai: &AiStats) -> Option<f32> {
    let (their_pos, their_size) = target;
    let dy = (their_pos.y + their_size.y / 2.0) - (pos.y + size.y / 2.0);
    if dy.abs() > ai.sight_height {
        return None;
    }
    let dx = (their_pos.x + their_size.x / 2.0) - (pos.x + size.x / 2.0);
    if dx.abs() > ai.sight || (!ai.flying && dx.signum() != facing.signum()) {
        return None;
    }
    Some(dx.signum())
}

/// Is there a clear line from this NPC's eyes to the player's middle?
///
/// Sampled rather than swept: a point every few pixels along the line, each
/// tested against the solids around it. A wall is at least a tile thick, so
/// nothing a map can build fits between two samples. One-way platforms do not
/// block sight — they are planks, and you can see between them.
fn in_view<Q: SolidQuery + ?Sized>(
    pos: Vec2,
    size: Vec2,
    target: (Vec2, Vec2),
    geometry: &Q,
) -> bool {
    const STEP: f32 = 8.0;
    let (their_pos, their_size) = target;
    let eye = pos + Vec2::new(size.x / 2.0, size.y / 4.0);
    let them = their_pos + their_size / 2.0;
    let steps = ((them - eye).length() / STEP).ceil().max(1.0) as u32;
    let mut near = Vec::new();
    (1..steps).all(|i| {
        let at = eye.lerp(them, i as f32 / steps as f32);
        let point = Aabb::new(at.x - 0.5, at.y - 0.5, 1.0, 1.0);
        near.clear();
        geometry.overlapping(point, &mut near);
        !near.iter().any(|s| !s.one_way && s.rect.overlaps(&point))
    })
}

/// Direction to the player if they are within `range` horizontally and
/// `height` vertically.
fn within(pos: Vec2, size: Vec2, target: (Vec2, Vec2), range: f32, height: f32) -> Option<f32> {
    let (their_pos, their_size) = target;
    let dy = (their_pos.y + their_size.y / 2.0) - (pos.y + size.y / 2.0);
    if dy.abs() > height {
        return None;
    }
    // Gap between the two colliders, not between their centres, so reach does
    // not depend on how wide the bodies happen to be.
    let gap = if their_pos.x > pos.x {
        their_pos.x - (pos.x + size.x)
    } else {
        pos.x - (their_pos.x + their_size.x)
    };
    if gap > range {
        return None;
    }
    let dx = (their_pos.x + their_size.x / 2.0) - (pos.x + size.x / 2.0);
    Some(if dx == 0.0 { 1.0 } else { dx.signum() })
}

/// Is there a solid directly in front of the body, at body height?
fn wall_ahead<Q: SolidQuery + ?Sized>(
    pos: Vec2,
    size: Vec2,
    dir: f32,
    geometry: &Q,
    ai: &AiStats,
) -> bool {
    let x = if dir > 0.0 {
        pos.x + size.x
    } else {
        pos.x - ai.lookahead
    };
    // Inset vertically so the floor being stood on is not read as a wall.
    let probe = Aabb::new(x, pos.y + 2.0, ai.lookahead, size.y - 4.0);
    probed(geometry, probe).any(|s| !s.one_way && probe.overlaps(&s.rect))
}

/// Is there anything to stand on just beyond the leading edge?
fn floor_ahead<Q: SolidQuery + ?Sized>(
    pos: Vec2,
    size: Vec2,
    dir: f32,
    geometry: &Q,
    ai: &AiStats,
) -> bool {
    let x = if dir > 0.0 {
        pos.x + size.x
    } else {
        pos.x - ai.lookahead
    };
    // One-way platforms count: they hold a body up from above, which is all
    // that matters for deciding whether the next step lands on something.
    let probe = Aabb::new(x, pos.y + size.y, ai.lookahead, ai.floor_probe);
    probed(geometry, probe).any(|s| probe.overlaps(&s.rect))
}

/// The solids a lookahead probe has to consider.
fn probed<Q: SolidQuery + ?Sized>(geometry: &Q, probe: Aabb) -> impl Iterator<Item = SolidRect> {
    let mut candidates = Vec::new();
    geometry.overlapping(probe, &mut candidates);
    candidates.into_iter()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::assets::{StatBlock, StatTable};
    use crate::ecs::components::Team;
    use crate::sim::{Sim, TICK};
    use crate::systems::body;
    use crate::systems::input::PlayerInput;

    /// A deliberately un-knightly body and pace: the AI must be driven by the
    /// stat block and the geometry it is handed, not by the knight's numbers
    /// being baked into it.
    const SIZE: Vec2 = Vec2::new(20.0, 30.0);
    const SPEED: f32 = 60.0;

    /// The real numbers for a kind, from `assets/data/stats.ron`. Sight,
    /// reach and patience are content; a test that invented its own would not
    /// be testing the game.
    fn stats(kind: &str) -> Arc<StatBlock> {
        StatTable::shipped()
            .get(kind)
            .unwrap_or_else(|e| panic!("{e:#}"))
    }

    /// A plain walker: paces, and does not care about the player.
    fn spawn(world: &mut World, pos: Vec2, dir: f32) -> hecs::Entity {
        let knight = stats("knight");
        world.spawn((
            Patrol::new(dir, SPEED),
            Team::Enemy,
            Health::new(3, knight.iframe_ticks),
            Attacking::default(),
            Position(pos),
            Velocity(Vec2::ZERO),
            Size(SIZE),
            Body::new(pos, knight.gravity, knight.max_fall),
            DerivedStats(knight),
        ))
    }

    /// A walker that will come after you.
    fn spawn_hostile(world: &mut World, pos: Vec2, dir: f32) -> hecs::Entity {
        let entity = spawn(world, pos, dir);
        let knight = stats("knight");
        world
            .insert_one(entity, Hostile::new(pos, knight.attack.clone()))
            .unwrap();
        entity
    }

    /// A stand-in player for the AI to notice. Carries an `Avatar`, because
    /// that is what `player_position` looks for — being on the player's team is
    /// no longer enough, and deliberately so: villagers are too.
    fn spawn_target(world: &mut World, pos: Vec2) -> hecs::Entity {
        let player = stats("player");
        world.spawn((
            Avatar::new(player.avatar()),
            Team::Player,
            Health::new(player.max_health, player.iframe_ticks),
            Position(pos),
            Velocity(Vec2::ZERO),
            Size(SIZE),
        ))
    }

    /// A friendly walker on the player's team, to prove a knight does not hunt
    /// one. This is what a villager is.
    fn spawn_friendly(world: &mut World, pos: Vec2, dir: f32) -> hecs::Entity {
        let entity = spawn(world, pos, dir);
        world.insert_one(entity, Team::Player).unwrap();
        entity
    }

    fn tick(world: &mut World, geo: &[SolidRect]) {
        think(world, geo, &SpellTable::shipped(), &mut Vec::new());
        body::move_bodies(world, geo, TICK);
    }

    fn stance_of(world: &World, e: hecs::Entity) -> Stance {
        world.get::<&Hostile>(e).unwrap().stance
    }

    fn x_of(world: &World, e: hecs::Entity) -> f32 {
        world.get::<&Position>(e).unwrap().0.x
    }

    fn dir_of(world: &World, e: hecs::Entity) -> f32 {
        world.get::<&Patrol>(e).unwrap().dir
    }

    /// A ledge with open air at both ends. The patroller must stay on it.
    fn ledge() -> Vec<SolidRect> {
        vec![SolidRect::solid(Aabb::new(100.0, 400.0, 200.0, 32.0))]
    }

    #[test]
    fn turns_around_at_a_ledge_instead_of_walking_off() {
        let mut world = World::new();
        let geo = ledge();
        let e = spawn(&mut world, Vec2::new(150.0, 400.0 - SIZE.y), 1.0);

        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        for _ in 0..1200 {
            tick(&mut world, &geo);
            let x = x_of(&world, e);
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            assert!(
                world.get::<&Body>(e).unwrap().grounded,
                "walked off the ledge at x={x}"
            );
        }

        assert!(min_x >= 100.0, "went off the left end: {min_x}");
        assert!(max_x + SIZE.x <= 300.0, "went off the right end: {max_x}");
        assert!(
            max_x - min_x > 100.0,
            "barely moved: patrolled {min_x}..{max_x}"
        );
    }

    #[test]
    fn turns_around_at_a_wall() {
        let mut world = World::new();
        let geo = vec![
            SolidRect::solid(Aabb::new(0.0, 400.0, 400.0, 32.0)), // floor
            SolidRect::solid(Aabb::new(250.0, 300.0, 32.0, 100.0)), // wall
        ];
        let e = spawn(&mut world, Vec2::new(150.0, 400.0 - SIZE.y), 1.0);

        for _ in 0..300 {
            tick(&mut world, &geo);
            assert!(
                x_of(&world, e) + SIZE.x <= 250.0 + 0.01,
                "walked into the wall"
            );
        }
        assert_eq!(dir_of(&world, e), -1.0, "turned back from the wall");
    }

    /// The route is decided by geometry, so mirroring the world must mirror
    /// the patrol exactly. Catches a look-ahead probe that is right-biased.
    #[test]
    fn patrolling_is_left_right_symmetric() {
        let run = |dir: f32, mirror: bool| -> Vec<f32> {
            let geo = if mirror {
                vec![SolidRect::solid(Aabb::new(-300.0, 400.0, 200.0, 32.0))]
            } else {
                ledge()
            };
            let start = if mirror {
                Vec2::new(-150.0 - SIZE.x, 400.0 - SIZE.y)
            } else {
                Vec2::new(150.0, 400.0 - SIZE.y)
            };
            let mut world = World::new();
            let e = spawn(&mut world, start, dir);
            (0..600)
                .map(|_| {
                    tick(&mut world, &geo);
                    let x = x_of(&world, e);
                    if mirror {
                        -x - SIZE.x
                    } else {
                        x
                    }
                })
                .collect()
        };

        let normal = run(1.0, false);
        let mirrored = run(-1.0, true);
        for (t, (a, b)) in normal.iter().zip(&mirrored).enumerate() {
            assert!((a - b).abs() < 1e-3, "tick {t}: {a} vs mirrored {b}");
        }
    }

    /// A patroller is just a body with a controller, so it obeys gravity and
    /// lands on whatever is beneath it like anything else.
    #[test]
    fn a_patroller_dropped_in_midair_falls_and_then_patrols() {
        let mut world = World::new();
        let geo = vec![SolidRect::solid(Aabb::new(0.0, 400.0, 400.0, 32.0))];
        let e = spawn(&mut world, Vec2::new(200.0, 100.0), 1.0);

        for _ in 0..180 {
            tick(&mut world, &geo);
        }
        let body = *world.get::<&Body>(e).unwrap();
        assert!(body.grounded, "never landed");
        assert_eq!(
            world.get::<&Position>(e).unwrap().0.y,
            400.0 - SIZE.y,
            "came to rest on the floor"
        );
    }

    // --- the fight brain -------------------------------------------------

    fn flat_floor() -> Vec<SolidRect> {
        vec![SolidRect::solid(Aabb::new(0.0, 400.0, 600.0, 32.0))]
    }

    /// Sight is a box in front, so a player behind a patrolling knight is not
    /// noticed. Walking up behind something ought to work.
    #[test]
    fn a_player_behind_the_knight_goes_unnoticed() {
        let mut world = World::new();
        let geo = flat_floor();
        let knight = spawn_hostile(&mut world, Vec2::new(300.0, 400.0 - SIZE.y), 1.0);
        spawn_target(&mut world, Vec2::new(260.0, 400.0 - SIZE.y));

        // one tick, before the patrol can turn it around
        tick(&mut world, &geo);
        assert_eq!(stance_of(&world, knight), Stance::Patrol);
    }

    /// A villager is on the player's team so the player's sword cannot touch
    /// it. That must not make it prey: a knight hunts the *avatar*, not the
    /// team, or the first friendly NPC on a map turns every knight on the
    /// nearest civilian.
    #[test]
    fn a_knight_ignores_a_friendly_walker_standing_in_front_of_it() {
        let mut world = World::new();
        let geo = flat_floor();
        let knight = spawn_hostile(&mut world, Vec2::new(200.0, 400.0 - SIZE.y), 1.0);
        spawn_friendly(&mut world, Vec2::new(240.0, 400.0 - SIZE.y), -1.0);

        for _ in 0..60 {
            tick(&mut world, &geo);
            assert_eq!(
                stance_of(&world, knight),
                Stance::Patrol,
                "the knight went hunting a neighbour"
            );
        }
    }

    #[test]
    fn a_player_in_front_is_chased() {
        let mut world = World::new();
        let geo = flat_floor();
        let knight = spawn_hostile(&mut world, Vec2::new(200.0, 400.0 - SIZE.y), 1.0);
        spawn_target(&mut world, Vec2::new(300.0, 400.0 - SIZE.y));

        tick(&mut world, &geo);
        assert_eq!(stance_of(&world, knight), Stance::Chase);

        let before = world.get::<&Position>(knight).unwrap().0.x;
        for _ in 0..20 {
            tick(&mut world, &geo);
        }
        let after = world.get::<&Position>(knight).unwrap().0.x;
        assert!(after > before, "closed the distance: {before} -> {after}");
    }

    /// A player directly above on a platform is not "in front of" anything.
    #[test]
    fn a_player_far_above_is_not_in_the_sight_box() {
        let mut world = World::new();
        let geo = flat_floor();
        let knight = spawn_hostile(&mut world, Vec2::new(200.0, 400.0 - SIZE.y), 1.0);
        spawn_target(&mut world, Vec2::new(240.0, 400.0 - SIZE.y - 120.0));

        tick(&mut world, &geo);
        assert_eq!(stance_of(&world, knight), Stance::Patrol);
    }

    #[test]
    fn closing_to_reach_produces_a_swing() {
        let mut world = World::new();
        let geo = flat_floor();
        let knight = spawn_hostile(&mut world, Vec2::new(200.0, 400.0 - SIZE.y), 1.0);
        spawn_target(&mut world, Vec2::new(232.0, 400.0 - SIZE.y));

        let mut events = Vec::new();
        for _ in 0..30 {
            think(&mut world, &geo, &SpellTable::shipped(), &mut events);
            body::move_bodies(&mut world, &geo, TICK);
            if world.get::<&Attacking>(knight).unwrap().busy() {
                break;
            }
        }

        assert!(world.get::<&Attacking>(knight).unwrap().busy(), "swung");
        assert_eq!(stance_of(&world, knight), Stance::Attack);
        assert!(events
            .iter()
            .any(|e| matches!(e, GameEvent::Attacked { .. })));
        assert_eq!(
            world.get::<&Velocity>(knight).unwrap().0.x,
            0.0,
            "committed to the swing rather than walking through it"
        );
    }

    /// Losing the player sends it home, not wherever the chase ended.
    #[test]
    fn losing_the_player_walks_back_home() {
        let mut world = World::new();
        let geo = flat_floor();
        let home = 200.0;
        let knight = spawn_hostile(&mut world, Vec2::new(home, 400.0 - SIZE.y), 1.0);
        let target = spawn_target(&mut world, Vec2::new(300.0, 400.0 - SIZE.y));

        for _ in 0..30 {
            tick(&mut world, &geo);
        }
        assert_eq!(stance_of(&world, knight), Stance::Chase);
        let chased_to = world.get::<&Position>(knight).unwrap().0.x;
        assert!(chased_to > home, "actually left home");

        // the player vanishes over the horizon
        world.get::<&mut Position>(target).unwrap().0.x = 5000.0;
        tick(&mut world, &geo);
        assert_eq!(stance_of(&world, knight), Stance::Return);

        for _ in 0..600 {
            tick(&mut world, &geo);
            if stance_of(&world, knight) == Stance::Patrol {
                break;
            }
        }
        assert_eq!(stance_of(&world, knight), Stance::Patrol, "settled back");
        let back = world.get::<&Position>(knight).unwrap().0.x;
        assert!(
            (back - home).abs() <= stats("knight").ai().home_slack + 2.0,
            "came home to {home}, ended at {back}"
        );
    }

    /// Chasing must not override the ledge check — an enemy that walks off a
    /// cliff after you is a bug, not tenacity.
    #[test]
    fn a_chase_still_stops_at_a_ledge() {
        let mut world = World::new();
        // ledge ends at x=300; the player stands past it in mid-air
        let geo = vec![SolidRect::solid(Aabb::new(100.0, 400.0, 200.0, 32.0))];
        let knight = spawn_hostile(&mut world, Vec2::new(240.0, 400.0 - SIZE.y), 1.0);
        // Far enough past the drop to be out of reach, close enough to be seen.
        spawn_target(&mut world, Vec2::new(390.0, 400.0 - SIZE.y));

        for _ in 0..120 {
            tick(&mut world, &geo);
            assert!(
                world.get::<&Body>(knight).unwrap().grounded,
                "chased off the ledge at x={}",
                world.get::<&Position>(knight).unwrap().0.x
            );
        }
        assert_eq!(stance_of(&world, knight), Stance::Chase, "still wants to");
        assert!(
            world.get::<&Position>(knight).unwrap().0.x + SIZE.x <= 300.0 + 1.0,
            "stopped at the edge rather than teetering over it"
        );
    }

    /// End to end through the real `Sim`: a `K` in a fixture grid becomes a
    /// knight that walks its platform and stays on it.
    ///
    /// The assertion is "never stops being grounded" rather than an exact x
    /// range. A patroller turns when its leading edge reaches the drop, so it
    /// can overhang by up to one tick of movement — which is not falling off,
    /// and pinning the exact overhang would just encode the walk speed.
    #[test]
    fn a_knight_placed_in_a_map_patrols_its_platform() {
        let mut sim = Sim::fixture(&["..............", "..P.......K...", "..############"]);

        let knight = sim.npcs()[0];
        let start = sim.npc_positions()[0];
        let (mut min_x, mut max_x) = (f32::MAX, f32::MIN);
        let mut reversals = 0;
        let mut last_dir = sim.world.get::<&Patrol>(knight).unwrap().dir;

        for tick in 0..900 {
            sim.step(PlayerInput::default());

            let pos = sim.npc_positions()[0];
            min_x = min_x.min(pos.x);
            max_x = max_x.max(pos.x);

            assert!(
                sim.world.get::<&Body>(knight).unwrap().grounded,
                "tick {tick}: knight left the platform at x={:.2}, y={:.2}",
                pos.x,
                pos.y
            );

            let dir = sim.world.get::<&Patrol>(knight).unwrap().dir;
            if dir != last_dir {
                reversals += 1;
                last_dir = dir;
            }
        }

        assert!(
            reversals >= 2,
            "should have turned around repeatedly, got {reversals}"
        );
        assert!(max_x - min_x > 50.0, "barely moved: {min_x:.1}..{max_x:.1}");
        assert_eq!(
            sim.npc_positions()[0].y,
            start.y,
            "stayed at platform height throughout"
        );
    }

    // --- the fixes from the M8 review --------------------------------------

    /// A flat floor, wide enough for anything.
    fn floor() -> Vec<SolidRect> {
        vec![SolidRect::solid(Aabb::new(0.0, 400.0, 2000.0, 32.0))]
    }

    fn standing(x: f32) -> Vec2 {
        Vec2::new(x, 400.0 - SIZE.y)
    }

    /// Sight is a box in front, but a chase knows where you went. A player
    /// who slipped behind a chasing knight used to leave it facing the wall.
    #[test]
    fn a_chasing_knight_turns_to_face_a_player_behind_it() {
        let mut world = World::new();
        let geo = floor();
        let knight = spawn_hostile(&mut world, standing(500.0), 1.0);
        spawn_target(&mut world, standing(400.0));
        world.get::<&mut Hostile>(knight).unwrap().stance = Stance::Chase;

        tick(&mut world, &geo);
        assert_eq!(dir_of(&world, knight), -1.0, "faced the player behind it");
        assert_eq!(stance_of(&world, knight), Stance::Chase);
        for _ in 0..30 {
            tick(&mut world, &geo);
        }
        assert!(x_of(&world, knight) < 500.0, "and went after them");
    }

    /// A blow is news. Six hits in the back used to kill a knight that never
    /// stopped patrolling.
    #[test]
    fn a_knight_hit_from_behind_gives_chase() {
        let mut world = World::new();
        let geo = floor();
        let knight = spawn_hostile(&mut world, standing(500.0), 1.0);
        spawn_target(&mut world, standing(460.0));
        assert_eq!(stance_of(&world, knight), Stance::Patrol);

        world.get::<&mut Health>(knight).unwrap().hitstun = 10;
        tick(&mut world, &geo);
        assert_eq!(stance_of(&world, knight), Stance::Chase);
    }

    /// A jump is not an escape: the chase outlasts a hundred pixels of air.
    #[test]
    fn jumping_in_place_does_not_end_a_chase() {
        let mut world = World::new();
        let geo = floor();
        let knight = spawn_hostile(&mut world, standing(500.0), -1.0);
        let player = spawn_target(&mut world, standing(400.0));
        world.get::<&mut Hostile>(knight).unwrap().stance = Stance::Chase;

        // The top of a full jump: about a hundred pixels up.
        world.get::<&mut Position>(player).unwrap().0.y -= 100.0;
        for _ in 0..10 {
            tick(&mut world, &geo);
        }
        assert_eq!(stance_of(&world, knight), Stance::Chase);
    }

    /// Home across a drop it cannot walk down: it settles where it is rather
    /// than turning back and forth against the edge forever.
    #[test]
    fn a_knight_whose_home_is_out_of_reach_settles_where_it_is() {
        let mut world = World::new();
        let geo = ledge();
        let knight = spawn_hostile(&mut world, standing(120.0), -1.0);
        {
            let mut hostile = world.get::<&mut Hostile>(knight).unwrap();
            hostile.stance = Stance::Return;
            hostile.home.x = 20.0; // off the left end of the ledge
        }
        let mut flips = 0;
        let mut last = dir_of(&world, knight);
        for _ in 0..300 {
            tick(&mut world, &geo);
            if dir_of(&world, knight) != last {
                flips += 1;
                last = dir_of(&world, knight);
            }
        }
        assert_eq!(stance_of(&world, knight), Stance::Patrol);
        assert!(flips < 20, "turned {flips} times in five seconds");
    }

    /// Walls block sight. A knight used to notice — and chase — a player on the
    /// other side of solid stone.
    #[test]
    fn a_knight_cannot_see_through_a_wall() {
        let mut world = World::new();
        let mut geo = floor();
        geo.push(SolidRect::solid(Aabb::new(550.0, 300.0, 32.0, 100.0)));
        let knight = spawn_hostile(&mut world, standing(500.0), 1.0);
        spawn_target(&mut world, standing(600.0));

        tick(&mut world, &geo);
        assert_eq!(
            stance_of(&world, knight),
            Stance::Patrol,
            "the wall hides them"
        );

        // The same player with nothing in between is seen at once.
        let mut world = World::new();
        let geo = floor();
        let knight = spawn_hostile(&mut world, standing(500.0), 1.0);
        spawn_target(&mut world, standing(600.0));
        tick(&mut world, &geo);
        assert_eq!(stance_of(&world, knight), Stance::Chase);
    }

    /// In reach and cooling down, a knight holds its ground rather than
    /// walking into the player — which flipped its facing every tick their
    /// centres crossed.
    #[test]
    fn a_knight_in_reach_holds_its_ground_while_it_cools_down() {
        let mut world = World::new();
        let geo = floor();
        let knight = spawn_hostile(&mut world, standing(500.0), -1.0);
        spawn_target(&mut world, standing(470.0));
        {
            let mut hostile = world.get::<&mut Hostile>(knight).unwrap();
            hostile.stance = Stance::Chase;
            hostile.cooldown = 50;
        }
        let start = x_of(&world, knight);
        for _ in 0..40 {
            tick(&mut world, &geo);
        }
        assert_eq!(x_of(&world, knight), start, "did not walk into the player");
        assert_eq!(dir_of(&world, knight), -1.0);
    }
}
