//! A fighter's brain: the keys an avatar presses when nobody is at its
//! keyboard — a rival champion, a warrior taken from another world who fights
//! the way the player does.
//!
//! **It drives the player's controller.** A brain decides a [`PlayerInput`]
//! and [`crate::systems::avatar::control`] does the rest exactly as it would
//! for a key press, so a rival jumps, chains the combo and casts with the same
//! code and the same numbers the player does, and can do nothing the player
//! cannot. That is the difference between a rival and an enemy: an enemy runs
//! [`crate::systems::npc`], and a rival is a player the machine is playing.
//!
//! What it decides is `sim --fight`'s closed-loop fighter grown up: close in,
//! swing and chain when in reach, give ground to a swing that is coming, jump
//! what is thrown at it, throw its own spell from range, and — for a kind that
//! takes to the air — leap up to a target above and plunge onto one below.
//! Every distance comes from the tables (its opener's reach, the other side's
//! swing), never a literal, so retuning a sword retunes the brain with it.
//!
//! No randomness. A reaction time is a countdown, so the same fight plays out
//! the same way every time — which is what lets a tape assert who won it.

use ggez::glam::Vec2;
use hecs::World;

use crate::assets::{AttackTable, BrainStats, SpellTable};
use crate::ecs::components::{
    Attacking, Avatar, Body, Brain, Casting, DerivedStats, Health, Mana, Position, Projectile,
    Size, Team, Velocity,
};
use crate::systems::input::{Action, ActionSet, PlayerInput, EDGE_ACTIONS};

/// How far out, and how near its own height, a bolt has to be before it is
/// worth jumping.
const BOLT_WARNING: Vec2 = Vec2::new(72.0, 28.0);

/// How far past its own reach a brain waits on a raised shield.
const SHIELD_WAIT: f32 = 24.0;

/// How far out a brain has to be before it will root itself in a cast in
/// front of something a blow does not stagger.
const CAREFUL_CAST: f32 = 110.0;

/// How far past its own swing's hitbox a brain counts a target as caught.
const REACH_SLACK: f32 = 6.0;

/// How close beside a swing's hitbox counts as in the way of it.
const THREAT_MARGIN: f32 = 12.0;

/// Decide, for every avatar with a brain at its controls, what it presses this
/// tick — into its [`Brain::input`], for the controller to read.
///
/// Runs in the decide phase just before `avatar::control`, reading the world
/// as the last tick left it: every brain sees the same world, whatever order
/// they are asked in.
pub fn think(world: &mut World, attacks: &AttackTable, spells: &SpellTable) {
    let mut decided: Vec<(hecs::Entity, Brain)> = Vec::new();
    for (entity, (brain, stats)) in world.query::<(&Brain, &DerivedStats)>().iter() {
        let Some(knobs) = stats.0.brain.as_ref() else {
            continue;
        };
        let mut brain = *brain;
        let target = nearest_foe(world, entity);
        brain.input = step(&mut brain, world, entity, target, knobs, attacks, spells);
        decided.push((entity, brain));
    }
    for (entity, brain) in decided {
        if let Ok(mut mind) = world.get::<&mut Brain>(entity) {
            *mind = brain;
        }
    }
}

/// One tick of one brain against `target`: make up its mind if it is time to,
/// and turn what it is holding into this tick's input.
///
/// Public so that `sim --fight` can put a brain at the *player's* controls and
/// record what it pressed as tape lines.
pub fn step(
    brain: &mut Brain,
    world: &World,
    me: hecs::Entity,
    target: Option<hecs::Entity>,
    knobs: &BrainStats,
    attacks: &AttackTable,
    spells: &SpellTable,
) -> PlayerInput {
    let alive = world.get::<&Health>(me).is_ok_and(|h| !h.dead());
    let target = target.filter(|_| alive);
    if let (false, Some(t)) = (brain.awake, target) {
        brain.awake = distance(world, me, t).is_some_and(|d| d <= knobs.sight);
    }

    let rising = world.get::<&Velocity>(me).is_ok_and(|v| v.0.y < 0.0);
    let mut held = match target.filter(|_| brain.awake) {
        None => ActionSet::EMPTY,
        Some(_) if brain.wait > 0 => {
            brain.wait -= 1;
            // Between decisions: keep the directions, let go of every one-shot
            // — a swing is a tap, not a hold.
            brain.held.without(EDGE_ACTIONS)
        }
        Some(target) => {
            brain.wait = knobs.reaction.saturating_sub(1);
            decide(world, me, target, knobs, attacks, spells, brain.prev)
        }
    };
    // ...except a jump still climbing, which is held to the top of its arc
    // whatever was decided, so that it is a full jump and not a hop.
    if brain.held.contains(Action::Jump) && rising {
        held.insert(Action::Jump);
    }
    brain.held = held;
    let input = PlayerInput::new(brain.held, brain.held.newly_set(brain.prev));
    brain.prev = brain.held;
    input
}

/// What to hold against `target`, from scratch. One-shot actions already held
/// last tick are left out, so that a decision to swing again is a fresh press
/// rather than a key that never came up.
fn decide(
    world: &World,
    me: hecs::Entity,
    target: hecs::Entity,
    knobs: &BrainStats,
    attacks: &AttackTable,
    spells: &SpellTable,
    prev: ActionSet,
) -> ActionSet {
    let mut held = ActionSet::EMPTY;
    let (Some(mine), Some(theirs)) = (boxed(world, me), boxed(world, target)) else {
        return held;
    };
    let stats = world.get::<&DerivedStats>(me).map(|s| s.0.clone()).ok();
    let Some(stats) = stats else { return held };
    let grounded = world.get::<&Body>(me).is_ok_and(|b| b.grounded);
    let rising = world.get::<&Velocity>(me).is_ok_and(|v| v.0.y < 0.0);
    let facing_right = world.get::<&Avatar>(me).is_ok_and(|a| a.facing_right);

    let (me_c, them_c) = (mine.0 + mine.1 / 2.0, theirs.0 + theirs.1 / 2.0);
    let right = them_c.x > me_c.x;
    let (toward, away) = if right {
        (Action::Right, Action::Left)
    } else {
        (Action::Left, Action::Right)
    };
    // Between the two boxes, not their centres, so reach does not depend on
    // how wide either body is.
    let gap = if right {
        theirs.0.x - (mine.0.x + mine.1.x)
    } else {
        mine.0.x - (theirs.0.x + theirs.1.x)
    };
    let dy = them_c.y - me_c.y;
    let press = |held: &mut ActionSet, action: Action| {
        if !prev.contains(action) {
            held.insert(action);
        }
    };

    // Half of what its opening swing reaches past its body: close enough that
    // the swing lands whichever way the bodies drift while it winds up.
    let reach = stats
        .attack
        .as_deref()
        .and_then(|id| attacks.get(id))
        .map_or(0.0, |def| (def.offset.0 + def.size.0 - mine.1.x) / 2.0);

    let flyer = world
        .get::<&DerivedStats>(target)
        .is_ok_and(|s| s.0.ai.as_ref().is_some_and(|ai| ai.flying));
    // Something a blow only bounces off or gets answered by — a shield, or a
    // heavy too steadfast to stagger — and whether it is committed right now:
    // swinging, and past the point its own blow can land.
    let careful = world
        .get::<&DerivedStats>(target)
        .is_ok_and(|stats| stats.0.guard || stats.0.steadfast);
    let committed = world.get::<&Attacking>(target).is_ok_and(|a| {
        a.attack
            .as_deref()
            .and_then(|id| attacks.get(id))
            .is_some_and(|def| a.elapsed >= def.active.1)
    });
    // Whether a swing started now would catch them: the opener's own hitbox,
    // aimed their way and grown by what moves in the ticks before it opens.
    // The box rather than a distance, so something diving in from above is
    // met on the way down rather than once it is already biting.
    let opener = if grounded {
        stats.attack.clone()
    } else {
        stats.avatar.as_ref().map(|a| a.air_attack.clone())
    };
    let in_reach = opener
        .as_deref()
        .and_then(|id| attacks.get(id))
        .is_some_and(|def| {
            let r = def.hitbox(mine.0, mine.1, right);
            crate::physics::Aabb::new(
                r.x - REACH_SLACK,
                r.y - REACH_SLACK,
                r.w + REACH_SLACK * 2.0,
                r.h + REACH_SLACK * 2.0,
            )
            .overlaps(&crate::physics::Aabb::new(
                theirs.0.x, theirs.0.y, theirs.1.x, theirs.1.y,
            ))
        });

    // Their swing, while its hitbox can still come out — and whether that
    // hitbox, grown by a margin, would find this body where it stands. The box
    // itself rather than a distance, so a blow that passes under a ledge you
    // are standing on is not one to run from.
    let threatened = world.get::<&Attacking>(target).ok().is_some_and(|a| {
        let Some(def) = a.attack.as_deref().and_then(|id| attacks.get(id)) else {
            return false;
        };
        let facing = crate::systems::combat::facing_right(world, target);
        let r = def.hitbox(theirs.0, theirs.1, facing);
        // Widened, not heightened: a body drifts into a blow sideways.
        let danger =
            crate::physics::Aabb::new(r.x - THREAT_MARGIN, r.y, r.w + THREAT_MARGIN * 2.0, r.h);
        let body = crate::physics::Aabb::new(mine.0.x, mine.0.y, mine.1.x, mine.1.y);
        a.elapsed < def.active.1 + 2 && danger.overlaps(&body)
    });
    let my_team = world.get::<&Team>(me).ok().map(|t| *t);
    let incoming = world
        .query::<(&Projectile, &Position, &Velocity, &Team)>()
        .iter()
        .any(|(_, (_, pos, vel, team))| {
            let d = pos.0 - me_c;
            Some(*team) != my_team
                && d.x.abs() < BOLT_WARNING.x
                && d.y.abs() < BOLT_WARNING.y
                && (vel.0.x < 0.0) == (d.x > 0.0)
        });
    let (swinging, chains) = world
        .get::<&Attacking>(me)
        .ok()
        .and_then(|a| a.attack.clone())
        .and_then(|id| attacks.get(&id))
        .map_or((false, false), |def| (true, def.chains()));
    let can_cast = knobs.cast_range > 0.0
        && stats.spell.as_deref().is_some_and(|spell| {
            let (Ok(casting), Ok(mana)) = (world.get::<&Casting>(me), world.get::<&Mana>(me))
            else {
                return false;
            };
            crate::systems::spell::refusal(spells, spell, &casting, &mana, swinging).is_none()
        });

    // A shield held ready in its face: a swing only bounces. Go over it and
    // come down on top — a plunge is never blocked — or, with no wings for
    // that, wait at the edge of its reach for it to swing, and hit it while
    // it is committed (by then it is not ready, and this does not apply).
    let shielded = crate::systems::combat::guarded(world, target, me_c.x);
    // A plunge is a commitment: never start one into a swing that is coming.
    let swinging_at_me = world.get::<&Attacking>(target).is_ok_and(|a| a.busy());
    let plunge_ok = knobs.aerial
        && !grounded
        && dy > 40.0
        && (them_c.x - me_c.x).abs() < 20.0
        && !swinging_at_me;

    if incoming && grounded {
        press(&mut held, Action::Jump);
        held.insert(toward);
    } else if shielded && gap <= reach + SHIELD_WAIT {
        if knobs.aerial && grounded {
            press(&mut held, Action::Jump);
            held.insert(toward);
        } else if plunge_ok {
            held.insert(Action::Down);
            press(&mut held, Action::Attack);
        } else if knobs.aerial {
            held.insert(toward);
        } else if gap < reach {
            held.insert(away);
        }
    } else if threatened && !stats.steadfast {
        held.insert(away);
    } else if plunge_ok {
        // Above them and over them: come down blade first.
        held.insert(Action::Down);
        press(&mut held, Action::Attack);
    } else if knobs.aerial && grounded && dy < -48.0 && (them_c.x - me_c.x).abs() < 128.0 {
        // They are up on something: go up after them.
        press(&mut held, Action::Jump);
        held.insert(toward);
    } else if flyer && grounded && dy < 8.0 && (them_c.x - me_c.x).abs() < 64.0 {
        // A flyer at head height comes in over the top of every standing
        // swing: jump into it, and the air attack takes it from there.
        press(&mut held, Action::Jump);
        held.insert(toward);
    } else if flyer && !grounded && !rising && dy < -16.0 && (them_c.x - me_c.x).abs() < 48.0 {
        // Still under it at the top of the jump: the second one.
        press(&mut held, Action::Jump);
        held.insert(toward);
    } else if careful && !committed && gap <= reach + SHIELD_WAIT && grounded {
        // A shield or a heavy that is not committed to anything: a blow now
        // is turned, or answered. Wait at the edge of its reach for it to
        // swing, and give it room if it comes closer.
        if gap < reach {
            held.insert(away);
        }
    } else if in_reach {
        // Chain the combo — but not into a shield, whose opening is one blow
        // long, nor into something too heavy to stagger, which swings back
        // straight through the rest of it. One cut, and out.
        if swinging && chains && !careful {
            press(&mut held, Action::Attack);
        } else if swinging && careful {
            held.insert(away);
        } else if facing_right != right {
            held.insert(toward);
        } else {
            press(&mut held, Action::Attack);
        }
    } else if can_cast
        && !shielded
        && gap <= knobs.cast_range
        && dy.abs() < 20.0
        && (!careful || gap > CAREFUL_CAST)
    {
        held.insert(toward);
        press(&mut held, Action::Cast);
    } else {
        held.insert(toward);
        // Pushing toward it last tick and going nowhere: something is in the
        // way — a step, a ledge's lip. Hop it. Not while a cast roots it in
        // place: that is standing still, not being stopped.
        let rooted = world.get::<&Casting>(me).is_ok_and(|c| c.busy());
        let stalled = !rooted && world.get::<&Velocity>(me).is_ok_and(|v| v.0.x.abs() < 1.0);
        if grounded && prev.contains(toward) && stalled {
            press(&mut held, Action::Jump);
        }
    }
    held
}

/// The nearest living avatar on the other side of `me`'s fight. Only avatars:
/// a rival has come for the player, not for a villager who happens to be on
/// the player's team.
pub fn nearest_foe(world: &World, me: hecs::Entity) -> Option<hecs::Entity> {
    let team = *world.get::<&Team>(me).ok()?;
    let mut foes: Vec<(f32, hecs::Entity)> = world
        .query::<(&Team, &Health)>()
        .with::<&Avatar>()
        .iter()
        .filter(|(entity, (their, health))| *entity != me && **their != team && !health.dead())
        .filter_map(|(entity, _)| Some((distance(world, me, entity)?, entity)))
        .collect();
    // Nearest first, then lowest id, so a tie is decided by the world and not
    // by the order hecs stored it in.
    foes.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id().cmp(&b.1.id())));
    foes.first().map(|(_, entity)| *entity)
}

fn boxed(world: &World, entity: hecs::Entity) -> Option<(Vec2, Vec2)> {
    let pos = world.get::<&Position>(entity).ok()?.0;
    let size = world.get::<&Size>(entity).ok()?.0;
    Some((pos, size))
}

fn distance(world: &World, a: hecs::Entity, b: hecs::Entity) -> Option<f32> {
    let (pa, sa) = boxed(world, a)?;
    let (pb, sb) = boxed(world, b)?;
    Some((pa + sa / 2.0).distance(pb + sb / 2.0))
}
