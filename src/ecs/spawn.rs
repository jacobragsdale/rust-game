//! Turning map placements into entities.
//!
//! Maps name what they want by string — a `K` in the grid, or
//! `Npc(kind: "knight", ...)` in the entity list — and this is the one place
//! that decides what such a string becomes. Before it existed, [`EntitySpawn`]
//! was parsed out of every map and thrown away.
//!
//! A string registry rather than an enum on purpose: adding an NPC should be a
//! data file plus one arm here, and a map that names something unknown should
//! fail loudly at load with the name it did not recognize, rather than being a
//! compile error in a file the level designer never opens.

use std::sync::Arc;

use ggez::glam::Vec2;
use hecs::World;

use crate::assets::{ClipSet, SpellDef, SpellEffect, StatBlock};
use crate::ecs::components::{
    AnimationState, Attacking, Avatar, Body, Brain, Casting, Contact, DerivedStats, Equipment,
    Health, Hostile, InteractTarget, Interactable, Inventory, Kind, Lifetime, Loot, Mana, Patrol,
    Pickup, Position, Projectile, Size, Sprite, Stats, Team, Velocity,
};
use crate::level::EntitySpawn;

/// The box an item occupies while it is lying on the floor.
///
/// One size for everything on purpose: a potion and a helm are both "a thing
/// you walk over", and giving each item its own footprint would make whether
/// you picked something up depend on art nobody has drawn yet. It is a number
/// in code rather than in RON because it is the shape of an interaction, not
/// balance — no tuning pass ever wants to make helmets harder to stand on.
pub const PICKUP_SIZE: Vec2 = Vec2::new(12.0, 12.0);

/// Every entity kind a map may place, sorted: every kind
/// `assets/data/stats.ron` defines that [`placeable`] accepts.
///
/// Read out of the table rather than listed here, so a new enemy is a stat
/// block and a clip set — `assets/data/animations/<kind>.ron` — and nothing
/// else. `tests/data.rs` checks every one has art that loads.
pub fn kinds(stats: &crate::assets::StatTable) -> Vec<&str> {
    stats
        .kinds()
        .into_iter()
        .filter(|kind| {
            stats
                .get(kind)
                .is_ok_and(|block| placeable(kind, &block).is_ok())
        })
        .collect()
}

/// Whether a map may place `kind`, whose block is `stats` — and if not, why
/// not. Asked when a map is read, so the error names the map rather than
/// arriving as a panic halfway through building the world.
pub fn placeable(kind: &str, stats: &StatBlock) -> anyhow::Result<()> {
    anyhow::ensure!(
        kind != PLAYER,
        "a map cannot place `{PLAYER}`: the player is where the map's `P` is"
    );
    anyhow::ensure!(
        stats.ai.is_some() || rival(stats),
        "`{kind}` has no `ai` group in assets/data/stats.ron, and every kind a \
         map places walks, flies or fights with a `brain` and an `avatar` group — \
         give it one"
    );
    Ok(())
}

/// Whether a kind fights with the player's own kit: an `avatar` group to
/// steer with and a `brain` to do the steering.
pub fn rival(stats: &StatBlock) -> bool {
    stats.avatar.is_some() && stats.brain.is_some()
}

/// The one kind no map places: [`player`] spawns it.
pub const PLAYER: &str = "player";

/// Spawn the player at the level's spawn point.
///
/// `stats` is the `"player"` block of `assets/data/stats.ron`. Every number
/// this entity is built from comes out of it — there are none left here.
pub fn player(
    world: &mut World,
    spawn: Vec2,
    clips: Arc<ClipSet>,
    stats: Arc<StatBlock>,
) -> hecs::Entity {
    let entity = world.spawn((
        Avatar::new(stats.avatar()),
        Body::new(spawn, stats.gravity, stats.max_fall),
        Team::Player,
        Health::new(stats.max_health, stats.iframe_ticks),
        Attacking::default(),
        Position(spawn),
        Velocity(Vec2::ZERO),
        Size(stats.size()),
        Sprite {
            clips,
            offset: Vec2::ZERO,
        },
        AnimationState::new("idle"),
        Stats(stats.clone()),
        // Every entity carries the derived block, even one that can never wear
        // anything: a read site should never have to ask whether this is the
        // kind of thing that has equipment. With nothing on it shares the base
        // `Arc`, so it costs a pointer.
        DerivedStats(stats.clone()),
    ));
    give_mana(world, entity, &stats);
    give_bag(world, entity, &stats);
    entity
}

/// Give an entity a mana pool and somewhere to keep a cast, if its kind has a
/// pool at all.
///
/// Conditional rather than universal so that a kind which never casts is not
/// dragged into a different archetype for the sake of two components it will
/// never read — and so `Sim::npcs` keeps meeting the world it expects.
fn give_mana(world: &mut World, entity: hecs::Entity, stats: &StatBlock) {
    if stats.max_mana <= 0 {
        return;
    }
    world
        .insert(
            entity,
            (
                Mana::new(stats.max_mana, stats.mana_regen),
                Casting::default(),
            ),
        )
        .expect("the entity was just spawned");
}

/// Give an entity a bag and somewhere to keep what it is wearing, if its kind
/// carries anything at all.
///
/// Conditional for the same reason [`give_mana`] is: a kind with no bag should
/// not be dragged into a different archetype for two components it will never
/// read. `inventory_slots: 0` is what says so, and it is a stat rather than a
/// constant here because how much you can carry is balance.
fn give_bag(world: &mut World, entity: hecs::Entity, stats: &StatBlock) {
    if stats.inventory_slots == 0 {
        return;
    }
    world
        .insert(
            entity,
            (
                Inventory::new(stats.inventory_slots as usize),
                Equipment::default(),
            ),
        )
        .expect("the entity was just spawned");
}

/// Give an entity something to say, if its kind has anything to say.
///
/// Conditional for the reason [`give_mana`] and [`give_bag`] are: a kind there
/// is nothing to talk to should not carry a component that only means "no".
/// Absence is also what makes [`crate::systems::dialogue::nearest_interactable`]
/// cheap — it queries for `Interactable` and meets only the handful of entities
/// that have one.
fn give_interactable(world: &mut World, entity: hecs::Entity, stats: &StatBlock) {
    let Some(def) = &stats.interact else {
        return;
    };
    world
        .insert_one(
            entity,
            Interactable {
                prompt: def.prompt.clone(),
                target: InteractTarget::Dialogue(def.dialogue.clone()),
            },
        )
        .expect("the entity was just spawned");
}

/// Give an entity a loot table, if its kind leaves anything behind.
fn give_loot(world: &mut World, entity: hecs::Entity, stats: &StatBlock) {
    if stats.loot.is_empty() {
        return;
    }
    world
        .insert_one(entity, Loot::new(stats.loot.clone()))
        .expect("the entity was just spawned");
}

/// Put an item on the floor at `origin` (the top-left of its box).
///
/// A plain body with no controller, which is exactly the case `move_bodies`
/// was generalized for: it falls, it lands, it sits there. No `Sprite`, because
/// item art does not exist yet and the scene draws a coloured quad from the
/// item's kind — the `ItemDef` still names its sprite, so nothing authored
/// today has to be rewritten when the art arrives.
///
/// `gravity` and `max_fall` come from whatever dropped it, so a drop falls the
/// way its owner did rather than needing numbers of its own.
pub fn pickup(
    world: &mut World,
    item: &str,
    count: u32,
    origin: Vec2,
    gravity: f32,
    max_fall: f32,
) -> hecs::Entity {
    world.spawn((
        Pickup {
            item: item.to_string(),
            count,
            refused: false,
            flag: None,
        },
        Position(origin),
        Velocity(Vec2::ZERO),
        Size(PICKUP_SIZE),
        Body::new(origin, gravity, max_fall),
    ))
}

/// Launch a spell's projectile from `origin` (the top-left of its box),
/// travelling the way its caster faces — or along `aim`, a unit vector, for a
/// spell that is thrown at somebody.
///
/// It carries everything about the hit it will deal, so nothing downstream
/// needs the spell table; and it carries the caster's clip set, so a bolt is
/// drawn out of the art of whoever threw it.
#[allow(clippy::too_many_arguments)]
pub fn projectile(
    world: &mut World,
    caster: hecs::Entity,
    origin: Vec2,
    facing_right: bool,
    aim: Option<Vec2>,
    spell: &SpellDef,
    team: Team,
    clips: Arc<ClipSet>,
) -> hecs::Entity {
    let SpellEffect::Projectile {
        speed,
        damage,
        lifetime,
        size,
        clip,
        knockback,
        hitstun,
        pierces,
        ..
    } = &spell.effect;

    let size = Vec2::new(size.0, size.1);
    let heading = aim.unwrap_or(Vec2::new(if facing_right { 1.0 } else { -1.0 }, 0.0));
    let velocity = heading * *speed;
    // Knockback throws the way the bolt was going, sideways; an aimed bolt
    // falling straight down throws the way its caster faces.
    let dir = if heading.x != 0.0 {
        heading.x.signum()
    } else if facing_right {
        1.0
    } else {
        -1.0
    };
    let knockback = Vec2::new(dir * knockback.0, knockback.1);

    // A bolt has no feet: its art is centred on its box rather than stood on
    // it, which is what `Sprite::draw_origin` assumes for anything that walks.
    // The nudge is derived from the clip's own cell size, so changing the art
    // does not silently move the bolt off its hitbox.
    let frame_h = clips
        .clip(clip)
        .map(|c| clips.frame_size_of(c).1)
        .unwrap_or(size.y);
    let offset = Vec2::new(0.0, (frame_h - size.y) / 2.0);

    let mut body = Body::new(origin, 0.0, speed.abs());
    // Straight and flat: a bolt is not a thrown rock. And through planks,
    // which only ever stop something coming down on them — which an aimed
    // bolt can be, and a flat one never is.
    body.gravity = 0.0;
    body.ignore_one_way = true;

    world.spawn((
        Projectile {
            damage: *damage,
            knockback,
            hitstun: *hitstun,
            pierces: *pierces,
            source: caster,
            hit: Vec::new(),
            launched: velocity,
        },
        Lifetime { ticks: *lifetime },
        team,
        Position(origin),
        Velocity(velocity),
        Size(size),
        body,
        Sprite { clips, offset },
        AnimationState::new(clip),
    ))
}

/// Spawn one map placement. `pos` is the top-left of the cell it was placed in;
/// the entity is stood on that cell's floor, horizontally centred, the same way
/// the player's spawn point is resolved.
///
/// `stats` is the block `assets/data/stats.ron` holds for `placement.kind`,
/// and it is the whole of the specification — there is no arm per kind here.
/// What a kind *is* follows from what its block has:
///
/// - an `ai` group makes it walk a route (a `Patrol`), and every placed kind
///   needs one;
/// - `friendly: true` puts it on the player's side, where the player cannot
///   hurt it and no enemy hunts it; anything else is an enemy, and hunts
///   (a `Hostile`) — with its `attack` in reach, with its `spell` from
///   `ai.cast_range`, by touching with its `contact`, or all three;
/// - `ai.flying` takes it off the ground;
/// - `max_mana`, `inventory_slots`, `loot` and `interact` give it a pool, a
///   bag, something to drop and something to say, as they always have.
pub fn entity(
    world: &mut World,
    placement: &EntitySpawn,
    tile_size: f32,
    clips: Arc<ClipSet>,
    stats: Arc<StatBlock>,
) -> anyhow::Result<hecs::Entity> {
    placeable(&placement.kind, &stats)?;
    let pos = stand_in_cell(placement.pos, tile_size, stats.size());
    let team = if stats.friendly {
        Team::Player
    } else {
        Team::Enemy
    };
    if rival(&stats) {
        return Ok(rival_avatar(world, placement, pos, team, clips, stats));
    }
    let entity = world.spawn((
        Kind(placement.kind.clone()),
        Patrol::new(1.0, stats.run_speed),
        team,
        Health::new(stats.max_health, stats.iframe_ticks),
        // Present on every walker, whether or not it ever swings:
        // `animation::select_patrol_clip` reads it.
        Attacking::default(),
        Position(pos),
        Velocity(Vec2::ZERO),
        Size(stats.size()),
        Body::new(pos, stats.gravity, stats.max_fall),
        Sprite {
            clips,
            offset: Vec2::ZERO,
        },
        AnimationState::new("idle"),
        Stats(stats.clone()),
        DerivedStats(stats.clone()),
    ));
    // `Patrol` without `Hostile` is exactly the blacksmith who paces but will
    // not stab you — there is no "friendly" branch in `npc::think`, because a
    // walker with no fight brain simply walks.
    if !stats.friendly {
        world
            .insert_one(entity, Hostile::new(pos, stats.attack.clone()))
            .expect("the entity was just spawned");
    }
    if let Some(contact) = &stats.contact {
        world
            .insert_one(entity, Contact(contact.clone()))
            .expect("the entity was just spawned");
    }
    give_mana(world, entity, &stats);
    give_bag(world, entity, &stats);
    give_loot(world, entity, &stats);
    give_interactable(world, entity, &stats);
    Ok(entity)
}

/// A rival: an avatar exactly like the player's — the same controller, the
/// same body, the same clips if its art is built on the player's — with a
/// brain at its controls instead of a keyboard. See [`crate::systems::brain`].
///
/// It faces left, into the room it was placed to hold: a duel is walked into
/// from the left in every map that has one, and a champion with its back to
/// you would be a strange way to start.
fn rival_avatar(
    world: &mut World,
    placement: &EntitySpawn,
    pos: Vec2,
    team: Team,
    clips: Arc<ClipSet>,
    stats: Arc<StatBlock>,
) -> hecs::Entity {
    let mut avatar = Avatar::new(stats.avatar());
    avatar.facing_right = false;
    let entity = world.spawn((
        Kind(placement.kind.clone()),
        avatar,
        Brain::default(),
        Body::new(pos, stats.gravity, stats.max_fall),
        team,
        Health::new(stats.max_health, stats.iframe_ticks),
        Attacking::default(),
        Position(pos),
        Velocity(Vec2::ZERO),
        Size(stats.size()),
        Sprite {
            clips,
            offset: Vec2::ZERO,
        },
        AnimationState::new("idle"),
        Stats(stats.clone()),
        DerivedStats(stats.clone()),
    ));
    give_mana(world, entity, &stats);
    give_loot(world, entity, &stats);
    give_interactable(world, entity, &stats);
    entity
}

/// Apply what the map says about this one NPC on top of what its kind says:
/// a conversation of its own, and a flag its death sets.
///
/// An NPC whose death flag is already set is spawned as the corpse it already
/// is — dead, on its last frame of dying, with its loot long since dropped —
/// rather than not at all. Leaving it out would renumber every NPC after it in
/// the map, and `knight.1` in a tape would quietly become a different knight.
pub fn customise(
    world: &mut World,
    entity: hecs::Entity,
    placement: &EntitySpawn,
    flags: &std::collections::BTreeMap<String, i64>,
) {
    if let Some(graph) = &placement.dialogue {
        let prompt = world
            .get::<&Interactable>(entity)
            .map(|i| i.prompt.clone())
            .unwrap_or_else(|_| "talk".to_string());
        let _ = world.insert_one(
            entity,
            Interactable {
                prompt,
                target: InteractTarget::Dialogue(graph.clone()),
            },
        );
    }
    if let Some(flag) = &placement.flag {
        let _ = world.insert_one(entity, crate::ecs::components::DeathFlag(flag.clone()));
        if flags.get(flag).copied().unwrap_or(0) != 0 {
            if let Ok(mut health) = world.get::<&mut Health>(entity) {
                health.current = 0;
            }
            if let Ok(mut loot) = world.get::<&mut Loot>(entity) {
                loot.dropped = true;
            }
            // A walker dies on `death`; an avatar — a rival — on the player's
            // own `die`, and is already through the freeze it would have had.
            let dying = if world.get::<&Avatar>(entity).is_ok() {
                "die"
            } else {
                "death"
            };
            if let Ok(mut avatar) = world.get::<&mut Avatar>(entity) {
                avatar.dead_ticks = 1;
            }
            let last = world.get::<&Sprite>(entity).ok().and_then(|sprite| {
                sprite
                    .clips
                    .clip(dying)
                    .map(|clip| clip.frames.len().saturating_sub(1))
            });
            if let (Ok(mut anim), Some(last)) = (world.get::<&mut AnimationState>(entity), last) {
                anim.restart(dying);
                anim.frame = last;
            }
        }
    }
}

/// Stand a collider of `size` on the floor of the cell whose top-left is
/// `cell`, centred horizontally.
pub fn stand_in_cell(cell: Vec2, tile_size: f32, size: Vec2) -> Vec2 {
    Vec2::new(
        cell.x + (tile_size - size.x) / 2.0,
        cell.y + tile_size - size.y,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement(kind: &str) -> EntitySpawn {
        EntitySpawn {
            kind: kind.to_string(),
            pos: Vec2::new(64.0, 96.0),
            ..Default::default()
        }
    }

    fn clips() -> Arc<ClipSet> {
        Arc::new(crate::sim::fixture_clips())
    }

    /// The real numbers, from the real file. Movement and combat values are
    /// content; a test that invented its own would not be testing the game.
    fn stats(kind: &str) -> Arc<StatBlock> {
        crate::assets::StatTable::shipped()
            .get(kind)
            .unwrap_or_else(|e| panic!("{e:#}"))
    }

    #[test]
    fn a_knight_stands_on_the_floor_of_its_cell() {
        let mut world = World::new();
        let size = stats("knight").size();
        let e = entity(
            &mut world,
            &placement("knight"),
            32.0,
            clips(),
            stats("knight"),
        )
        .unwrap();

        let pos = world.get::<&Position>(e).unwrap().0;
        assert_eq!(pos.y + size.y, 96.0 + 32.0, "feet on the cell floor");
        assert_eq!(pos.x + size.x / 2.0, 64.0 + 16.0, "centred in the cell");
    }

    /// Two kinds a map may not place, each refused with the reason: the
    /// player, and a kind with no `ai` group to steer it.
    #[test]
    fn a_kind_a_map_cannot_place_says_why() {
        let err = placeable("player", &stats("player")).unwrap_err();
        assert!(format!("{err:#}").contains("`P`"), "{err:#}");

        let mut walkerless = (*stats("knight")).clone();
        walkerless.ai = None;
        let err = placeable("statue", &walkerless).unwrap_err();
        assert!(format!("{err:#}").contains("statue"), "{err:#}");
        assert!(format!("{err:#}").contains("`ai`"), "{err:#}");
    }

    /// Every kind the table offers a map actually spawns — and every one of
    /// them is on the side its block says.
    #[test]
    fn every_placeable_kind_spawns_on_its_side() {
        let table = crate::assets::StatTable::shipped();
        let kinds = kinds(&table);
        assert!(kinds.contains(&"knight") && !kinds.contains(&PLAYER));
        for kind in kinds {
            let mut world = World::new();
            let e = entity(&mut world, &placement(kind), 32.0, clips(), stats(kind))
                .unwrap_or_else(|e| panic!("`{kind}` is placeable but fails to spawn: {e:#}"));
            let friendly = stats(kind).friendly;
            assert_eq!(
                *world.get::<&Team>(e).unwrap(),
                if friendly { Team::Player } else { Team::Enemy },
                "{kind}"
            );
            // Everything not friendly comes for the player: a walker or a
            // flyer with a `Hostile`, a rival as an avatar with a brain.
            let fights = if rival(&stats(kind)) {
                world.get::<&Brain>(e).is_ok() && world.get::<&Avatar>(e).is_ok()
            } else {
                world.get::<&Hostile>(e).is_ok()
            };
            assert_eq!(fights, !friendly, "{kind}");
        }
    }

    /// An unknown kind must name itself here too, not fail with a lookup miss
    /// somewhere downstream.
    #[test]
    fn an_unknown_kind_has_no_stat_block_and_says_so() {
        let err = crate::assets::StatTable::shipped()
            .get("dragon")
            .unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains("dragon"), "{text}");
        assert!(text.contains("knight"), "should list what is known: {text}");
    }
}
