//! Moves every `Scroller` entity left at the level's current scroll speed.

use hecs::World;

use crate::ecs::components::{Position, Scroller};

pub fn update(world: &mut World, scroll_speed: f32, dt: f32) {
    for (_, (pos, _)) in world.query_mut::<(&mut Position, &Scroller)>() {
        pos.0.x -= scroll_speed * dt;
    }
}
