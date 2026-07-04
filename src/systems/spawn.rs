//! Drains queued `SpawnRequest`s into real entities. Systems never spawn
//! directly mid-iteration; they queue requests instead.

use ggez::glam::Vec2;
use hecs::World;

use crate::ecs::components::{Position, Scroller, Size, Solid, SpikeBall};
use crate::ecs::events::{Events, SpawnRequest};

pub fn drain(world: &mut World, events: &mut Events) {
    for request in events.spawns.drain(..) {
        match request {
            SpawnRequest::Platform { x, y, w, h } => {
                world.spawn((
                    Position(Vec2::new(x, y)),
                    Size(Vec2::new(w, h)),
                    Solid,
                    Scroller,
                ));
            }
            SpawnRequest::SpikeBall { x, y } => {
                world.spawn((Position(Vec2::new(x, y)), SpikeBall, Scroller));
            }
        }
    }
}
