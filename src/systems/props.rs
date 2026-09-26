//! The furniture of a level: doors and exits, chests, items lying about,
//! checkpoints, signs, levers and the gates they open.
//!
//! **Everything here that can change is remembered in a flag.** A chest that
//! has been opened, an item that has been taken, a door that has been
//! unlocked, a lever that has been thrown — each is a `world.` flag (or, for a
//! lever, the quest flag it throws) rather than a field on an entity. Two
//! reasons, and they are the same reason. A save deliberately carries no
//! entities — the map is rebuilt from its file and the flags are put back on
//! it — and leaving a map and coming back rebuilds it the same way; state kept
//! on an entity would be forgotten by both. So the map is *built* from the
//! flags: [`spawn_props`] reads them, and an opened chest is spawned open.
//!
//! What pressing `interact` on one of these does is [`crate::sim::Sim`]'s,
//! next to the conversation a press can also open: a door has to rebuild the
//! world, which only the sim can do, and a chest has to go through the same
//! inventory path a pickup does. This module holds the pieces that are only
//! about the props themselves.

use std::collections::BTreeMap;

use ggez::glam::Vec2;
use hecs::World;

use crate::ecs::components::{
    Checkpoint, Chest, Collider, Door, Exit, Gate, InteractTarget, Interactable, Lever, Pickup,
    Position, Sign, Size, Trigger,
};
use crate::ecs::spawn;
use crate::level::{LevelData, PropKind};
use crate::physics::Aabb;

/// A chest's box, standing on its cell's floor. The art is drawn from it.
pub const CHEST_SIZE: Vec2 = Vec2::new(20.0, 14.0);
/// A checkpoint's box — the brazier's.
pub const CHECKPOINT_SIZE: Vec2 = Vec2::new(12.0, 24.0);
/// A sign's box.
pub const SIGN_SIZE: Vec2 = Vec2::new(16.0, 16.0);
/// A lever's box.
pub const LEVER_SIZE: Vec2 = Vec2::new(12.0, 11.0);

/// The world flag a prop's state is filed under: `world.<map>.<name>`.
pub fn world_flag(map: &str, name: &str) -> String {
    format!("world.{map}.{name}")
}

/// Read a flag, 0 when unset.
fn flag(flags: &BTreeMap<String, i64>, name: &str) -> i64 {
    flags.get(name).copied().unwrap_or(0)
}

/// Put every prop the map places into the world, as `flags` says it now is.
///
/// Called after every NPC has been spawned, so a prop can never renumber the
/// NPCs a tape addresses by index. `gravity` and `max_fall` are what an item
/// lying on the floor falls with, should the floor go.
pub fn spawn_props(
    world: &mut World,
    level: &LevelData,
    flags: &BTreeMap<String, i64>,
    map: &str,
    gravity: f32,
    max_fall: f32,
) {
    let ts = level.tile_size;
    for prop in &level.props {
        let name = prop.name();
        let remembered = world_flag(map, &name);
        let standing = |size: Vec2| level.stand_in_cell(prop.cell, size);
        match &prop.kind {
            PropKind::Door {
                to,
                at,
                locked,
                keep_key,
                art,
                ..
            } => {
                let unlocked = locked.is_none() || flag(flags, &remembered) != 0;
                world.spawn((
                    Position(Vec2::new(prop.cell.x, prop.cell.y - ts)),
                    Size(Vec2::new(ts * 2.0, ts * 2.0)),
                    Door {
                        to: to.clone(),
                        at: at.clone(),
                        locked: locked.clone(),
                        keep_key: *keep_key,
                        flag: remembered,
                        art: art.clone().unwrap_or_else(|| "door".to_string()),
                    },
                    Interactable {
                        prompt: if unlocked { "enter" } else { "unlock" }.to_string(),
                        target: InteractTarget::Door,
                    },
                ));
            }
            PropKind::Exit { size, to, at } => {
                world.spawn((
                    Position(prop.cell),
                    Size(Vec2::new(size.0 as f32 * ts, size.1 as f32 * ts)),
                    Exit {
                        to: to.clone(),
                        at: at.clone(),
                    },
                ));
            }
            PropKind::Chest { items, .. } => {
                let entity = world.spawn((
                    Position(standing(CHEST_SIZE)),
                    Size(CHEST_SIZE),
                    Chest {
                        items: items.clone(),
                        flag: remembered.clone(),
                    },
                ));
                if flag(flags, &remembered) == 0 {
                    world
                        .insert_one(
                            entity,
                            Interactable {
                                prompt: "open".to_string(),
                                target: InteractTarget::Chest,
                            },
                        )
                        .expect("just spawned");
                }
            }
            PropKind::Item { item, count, .. } => {
                if flag(flags, &remembered) != 0 {
                    continue; // taken on an earlier visit
                }
                let entity = spawn::pickup(
                    world,
                    item,
                    *count,
                    standing(spawn::PICKUP_SIZE),
                    gravity,
                    max_fall,
                );
                if let Ok(mut pickup) = world.get::<&mut Pickup>(entity) {
                    pickup.flag = Some(remembered);
                }
            }
            PropKind::Checkpoint => {
                world.spawn((
                    Position(standing(CHECKPOINT_SIZE)),
                    Size(CHECKPOINT_SIZE),
                    Checkpoint,
                ));
            }
            PropKind::Sign { dialogue } => {
                world.spawn((
                    Position(standing(SIGN_SIZE)),
                    Size(SIGN_SIZE),
                    Sign,
                    Interactable {
                        prompt: "read".to_string(),
                        target: InteractTarget::Dialogue(dialogue.clone()),
                    },
                ));
            }
            PropKind::Lever { flag: throws, .. } => {
                let entity = world.spawn((
                    Position(standing(LEVER_SIZE)),
                    Size(LEVER_SIZE),
                    Lever {
                        flag: throws.clone(),
                    },
                ));
                if flag(flags, throws) == 0 {
                    world
                        .insert_one(
                            entity,
                            Interactable {
                                prompt: "pull".to_string(),
                                target: InteractTarget::Lever,
                            },
                        )
                        .expect("just spawned");
                }
            }
            PropKind::Trigger {
                size,
                dialogue,
                when,
                ..
            } => {
                if flag(flags, &remembered) != 0 {
                    continue; // told on an earlier visit
                }
                world.spawn((
                    Position(prop.cell),
                    Size(Vec2::new(size.0 as f32 * ts, size.1 as f32 * ts)),
                    Trigger {
                        dialogue: dialogue.clone(),
                        flag: remembered,
                        when: when.clone(),
                    },
                ));
            }
            PropKind::Gate {
                flag: opens,
                height,
            } => {
                let size = Vec2::new(ts, *height as f32 * ts);
                let top = Vec2::new(prop.cell.x, prop.cell.y + ts - size.y);
                let entity = world.spawn((
                    Position(top),
                    Gate {
                        flag: opens.clone(),
                        collider: Collider::solid(size),
                    },
                ));
                if flag(flags, opens) == 0 {
                    world
                        .insert_one(entity, Collider::solid(size))
                        .expect("just spawned");
                }
            }
        }
    }
}

/// Open or close every gate as its flag says, by giving it its collider or
/// taking it away.
///
/// In the decide phase, beside the fires, and for the same reason: the
/// geometry rebuild picks up the change the same tick, and nothing in the
/// physics has to know a gate exists.
pub fn tick_gates(world: &mut World, flags: &BTreeMap<String, i64>) {
    let mut changes: Vec<(hecs::Entity, Option<Collider>)> = Vec::new();
    for (entity, (gate, collider)) in world.query::<(&Gate, Option<&Collider>)>().iter() {
        let closed = flag(flags, &gate.flag) == 0;
        match (closed, collider.is_some()) {
            (true, false) => changes.push((entity, Some(gate.collider))),
            (false, true) => changes.push((entity, None)),
            _ => {}
        }
    }
    for (entity, change) in changes {
        match change {
            Some(collider) => {
                let _ = world.insert_one(entity, collider);
            }
            None => {
                let _ = world.remove_one::<Collider>(entity);
            }
        }
    }
}

/// Whether a gate is closed right now, for drawing it.
pub fn gate_closed(world: &World, entity: hecs::Entity) -> bool {
    world.get::<&Collider>(entity).is_ok()
}

/// The lowest-id thing of kind `T` whose box overlaps `body`.
fn touching<T: hecs::Component>(world: &World, body: Aabb) -> Option<hecs::Entity> {
    world
        .query::<(&T, &Position, &Size)>()
        .iter()
        .filter(|(_, (_, pos, size))| {
            body.overlaps(&Aabb::new(pos.0.x, pos.0.y, size.0.x, size.0.y))
        })
        .map(|(entity, _)| entity)
        .min_by_key(|entity| entity.id())
}

/// The exit a body at `body` is standing in, if any.
pub fn exit_at(world: &World, body: Aabb) -> Option<hecs::Entity> {
    touching::<Exit>(world, body)
}

/// The checkpoint a body at `body` is touching, if any.
pub fn checkpoint_at(world: &World, body: Aabb) -> Option<hecs::Entity> {
    touching::<Checkpoint>(world, body)
}

/// The trigger a body at `body` has walked into, if any.
pub fn trigger_at(world: &World, body: Aabb) -> Option<hecs::Entity> {
    touching::<Trigger>(world, body)
}
