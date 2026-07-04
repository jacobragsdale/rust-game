//! Per-tick event queues. Systems communicate through these instead of
//! calling into each other; the level scene drains them at defined points.

#[derive(Clone, Copy, Debug)]
pub enum SpawnRequest {
    Platform { x: f32, y: f32, w: f32, h: f32 },
    SpikeBall { x: f32, y: f32 },
}

#[derive(Default)]
pub struct Events {
    pub spawns: Vec<SpawnRequest>,
    pub player_died: bool,
}

impl Events {
    pub fn clear(&mut self) {
        self.spawns.clear();
        self.player_died = false;
    }
}
