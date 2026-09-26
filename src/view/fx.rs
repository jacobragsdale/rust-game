//! Presentation that plays out over time: particles, the screen shaking,
//! fades, toasts, and a map's title as you arrive.
//!
//! **Derived, never told.** Nothing in the simulation announces an effect.
//! After every tick [`Fx::after_step`] compares the world with the one before
//! — whose health went down, which bolt has gone — and reads the tick's events
//! for the rest: a landing, a pickup, a checkpoint. So the sim carries no
//! presentation state, no trace moves when an effect is retuned in
//! `assets/data/effects.ron`, and nothing here can feed back into play. It has
//! its own generator for the same reason: drawing on the sim's would make the
//! look of a spark part of the game's randomness.

use std::sync::Arc;

use ggez::glam::Vec2;

use crate::assets::EffectTable;
use crate::ecs::components::{Avatar, Brain, DerivedStats, Health, Position, Projectile, Size};
use crate::render::{font, rgba, Color, Frame, VIEW};
use crate::sim::rng::Rng;
use crate::sim::{GameEvent, Sim, TICK};

/// Every effect the view asks `effects.ron` for. `tests/data.rs` checks each
/// is defined, so a typo is a failed build rather than a hit with no spark.
pub const CUES: &[&str] = &[
    "hit",
    "hurt",
    "death",
    "land",
    "bolt",
    "pickup",
    "checkpoint",
    "block",
];

/// Ticks to fade in from black, after a door or a respawn.
const FADE_TICKS: f32 = 18.0;
/// How long a toast stays up, how long its fade out is, and how many show.
const TOAST_TICKS: u32 = 150;
const TOAST_FADE: u32 = 30;
const TOASTS: usize = 4;
/// How long a map's title stays up.
const BANNER_TICKS: u32 = 150;
/// More particles than this and the oldest go first — a ceiling no fight
/// reaches, there so a pathological one cannot run away with the frame.
const MAX_PARTICLES: usize = 600;

const PANEL: (u8, u8, u8) = (13, 10, 23);
const TEXT: (u8, u8, u8) = (242, 237, 204);

struct Particle {
    pos: Vec2,
    vel: Vec2,
    life: u32,
    max: u32,
    size: f32,
    colour: (u8, u8, u8),
    gravity: f32,
}

/// Something with health, as it was at the end of a tick.
struct Seen {
    entity: hecs::Entity,
    health: i32,
    centre: Vec2,
    player: bool,
}

pub struct Fx {
    effects: Arc<EffectTable>,
    rng: Rng,
    particles: Vec<Particle>,
    seen: Vec<Seen>,
    bolts: Vec<(hecs::Entity, Vec2)>,
    /// Ticks of shaking left, how hard, and where this tick's shake puts the
    /// camera.
    shake: u32,
    shake_amp: f32,
    jolt: Vec2,
    /// How black the screen is, 0 to 1.
    fade: f32,
    toasts: Vec<(String, u32)>,
    banner: Option<(String, u32)>,
}

impl Fx {
    pub fn new(effects: Arc<EffectTable>, sim: &Sim) -> Fx {
        let mut fx = Fx {
            effects,
            rng: Rng::new(0x5EED_F1A5),
            particles: Vec::new(),
            seen: Vec::new(),
            bolts: Vec::new(),
            shake: 0,
            shake_amp: 0.0,
            jolt: Vec2::ZERO,
            fade: 0.0,
            toasts: Vec::new(),
            banner: None,
        };
        fx.arrive(sim);
        fx
    }

    /// A map has just been entered — a door, a load, the game starting:
    /// forget the last world, fade in from black, and put the map's name up.
    pub fn arrive(&mut self, sim: &Sim) {
        self.particles.clear();
        self.seen = seen(sim);
        self.bolts = bolts(sim);
        self.fade = 1.0;
        self.banner = sim.level.title.clone().map(|title| (title, BANNER_TICKS));
    }

    /// How far the shake moves the camera this tick.
    pub fn jolt(&self) -> Vec2 {
        self.jolt
    }

    /// Advance with the sim: called once after every step.
    pub fn after_step(&mut self, sim: &Sim) {
        // Toasts come from events, and a modal screen raises them too — a
        // reply that hands something over — so they are read in every mode.
        for event in sim.events() {
            if let Some(text) = toast(sim, event) {
                self.toasts.push((text, TOAST_TICKS));
            }
        }
        let excess = self.toasts.len().saturating_sub(TOASTS);
        self.toasts.drain(..excess);

        // A menu freezes the world, and everything drawn over it with it.
        if sim.mode().is_modal() {
            return;
        }

        // Whose health went down, and who died of it.
        let now = seen(sim);
        let mut bursts: Vec<(&str, Vec2)> = Vec::new();
        for thing in &now {
            let Some(before) = self
                .seen
                .iter()
                .find(|b| b.entity == thing.entity)
                .map(|b| b.health)
            else {
                continue;
            };
            if thing.health < before {
                bursts.push((if thing.player { "hurt" } else { "hit" }, thing.centre));
                if thing.player {
                    self.shake_for(10, 3.0);
                }
                if thing.health <= 0 && before > 0 {
                    bursts.push(("death", thing.centre));
                    self.shake_for(6, 2.0);
                }
            }
        }
        self.seen = now;

        // Which bolts are gone: they hit something, or ran out.
        let now = bolts(sim);
        for (bolt, at) in &self.bolts {
            if !now.iter().any(|(still, _)| still == bolt) {
                bursts.push(("bolt", *at));
            }
        }
        self.bolts = now;

        // What the player did.
        if let Some(body) = sim.player_box() {
            let (pos, size) = (Vec2::new(body.x, body.y), Vec2::new(body.w, body.h));
            for event in sim.events() {
                match event {
                    GameEvent::Landed { .. } => {
                        bursts.push(("land", pos + Vec2::new(size.x / 2.0, size.y)));
                    }
                    GameEvent::PickedUp { .. } => bursts.push(("pickup", pos + size / 2.0)),
                    GameEvent::Checkpoint => bursts.push(("checkpoint", pos + size / 2.0)),
                    // Off the shield, which is in front of whoever struck it.
                    GameEvent::Blocked { .. } => {
                        let right = crate::systems::avatar::player(&sim.world)
                            .is_some_and(|p| super::faces_right(&sim.world, p));
                        let ahead = (size.x / 2.0 + 10.0) * if right { 1.0 } else { -1.0 };
                        bursts.push(("block", pos + size / 2.0 + Vec2::new(ahead, -4.0)));
                    }
                    _ => {}
                }
            }
        }
        for (cue, at) in bursts {
            self.burst(cue, at);
        }

        for p in &mut self.particles {
            p.vel.y += p.gravity * TICK;
            p.pos += p.vel * TICK;
            p.life -= 1;
        }
        self.particles.retain(|p| p.life > 0);

        self.jolt = if self.shake > 0 {
            self.shake -= 1;
            let a = self.shake_amp;
            Vec2::new(self.rng.range(-a, a), self.rng.range(-a, a)).round()
        } else {
            Vec2::ZERO
        };

        // Dying fades out over the death freeze; everything else fades in.
        let dying = sim
            .world
            .query::<(&Avatar, &DerivedStats)>()
            .without::<&Brain>()
            .iter()
            .find(|(_, (avatar, _))| avatar.dead())
            .map(|(_, (avatar, stats))| {
                let total = stats.0.avatar().death_ticks.max(1) as f32;
                1.0 - avatar.dead_ticks as f32 / total
            });
        self.fade = match dying {
            Some(dark) => self.fade.max(dark),
            None => (self.fade - 1.0 / FADE_TICKS).max(0.0),
        };

        for (_, left) in &mut self.toasts {
            *left -= 1;
        }
        self.toasts.retain(|(_, left)| *left > 0);
        if let Some((_, left)) = &mut self.banner {
            *left -= 1;
            if *left == 0 {
                self.banner = None;
            }
        }
    }

    /// Shake for `ticks`, at least as hard as `amp`: a bigger jolt already
    /// under way is not cut short by a smaller one.
    fn shake_for(&mut self, ticks: u32, amp: f32) {
        if self.shake == 0 {
            self.shake_amp = 0.0;
        }
        self.shake = self.shake.max(ticks);
        self.shake_amp = self.shake_amp.max(amp);
    }

    fn burst(&mut self, cue: &str, at: Vec2) {
        let Some(def) = self.effects.get(cue) else {
            return;
        };
        for _ in 0..def.count {
            let angle = self.rng.range(def.angle.0, def.angle.1).to_radians();
            let speed = self.rng.range(def.speed.0, def.speed.1);
            let life = self.rng.range(def.life.0 as f32, def.life.1 as f32 + 1.0) as u32;
            let size = self.rng.range(def.size.0, def.size.1).round().max(1.0);
            let colour = *self.rng.pick(&def.colours).expect("validated non-empty");
            self.particles.push(Particle {
                pos: at,
                vel: Vec2::new(angle.cos(), angle.sin()) * speed,
                life: life.clamp(def.life.0, def.life.1),
                max: life.clamp(def.life.0, def.life.1),
                size,
                colour,
                gravity: def.gravity,
            });
        }
        let excess = self.particles.len().saturating_sub(MAX_PARTICLES);
        self.particles.drain(..excess);
    }

    /// The particles, in the world, under the HUD.
    pub fn draw_world(&self, frame: &mut Frame, offset: Vec2) {
        for p in &self.particles {
            // Full strength for the first half of its life, then fading.
            let alpha = (p.life as f32 / p.max as f32 * 2.0).min(1.0);
            let (r, g, b) = p.colour;
            frame.rect(
                (p.pos - offset - Vec2::splat(p.size / 2.0)).round(),
                Vec2::splat(p.size),
                rgba(r, g, b, alpha),
            );
        }
    }

    /// Everything over the HUD: the fade, then the map's title and the toasts,
    /// which are readable through it.
    pub fn draw_screen(&self, frame: &mut Frame) {
        frame.rect(Vec2::ZERO, VIEW, Color::new(0.0, 0.0, 0.0, self.fade));

        if let Some((title, left)) = &self.banner {
            let shown = (BANNER_TICKS - left) as f32;
            let alpha = (shown / 20.0).min(*left as f32 / 30.0).min(1.0);
            let w = font::width(title) * 2.0;
            let y = 64.0;
            frame.rect(
                Vec2::new((VIEW.x - w) / 2.0 - 10.0, y - 7.0),
                Vec2::new(w + 20.0, font::GLYPH_H as f32 * 2.0 + 13.0),
                rgba(PANEL.0, PANEL.1, PANEL.2, 0.75 * alpha),
            );
            frame.text_centred(
                VIEW.x / 2.0,
                y,
                title,
                rgba(TEXT.0, TEXT.1, TEXT.2, alpha),
                2.0,
            );
        }

        // Newest at the bottom.
        for (row, (text, left)) in self.toasts.iter().rev().enumerate() {
            let alpha = (*left as f32 / TOAST_FADE as f32).min(1.0);
            let y = VIEW.y - 20.0 - row as f32 * 15.0;
            frame.rect(
                Vec2::new(6.0, y - 3.0),
                Vec2::new(font::width(text) + 8.0, font::GLYPH_H as f32 + 5.0),
                rgba(PANEL.0, PANEL.1, PANEL.2, 0.8 * alpha),
            );
            frame.text(
                Vec2::new(10.0, y),
                text,
                rgba(TEXT.0, TEXT.1, TEXT.2, alpha),
            );
        }
    }
}

/// Everything with health, sorted so the order effects draw on the generator
/// does not depend on how the world happens to be stored.
fn seen(sim: &Sim) -> Vec<Seen> {
    let mut seen: Vec<Seen> = sim
        .world
        .query::<(&Health, &Position, &Size, Option<&Avatar>, Option<&Brain>)>()
        .iter()
        .map(|(entity, (health, pos, size, avatar, brain))| Seen {
            entity,
            health: health.current,
            centre: pos.0 + size.0 / 2.0,
            player: avatar.is_some() && brain.is_none(),
        })
        .collect();
    seen.sort_by_key(|s| s.entity.id());
    seen
}

fn bolts(sim: &Sim) -> Vec<(hecs::Entity, Vec2)> {
    let mut bolts: Vec<(hecs::Entity, Vec2)> = sim
        .world
        .query::<(&Projectile, &Position, &Size)>()
        .iter()
        .map(|(entity, (_, pos, size))| (entity, pos.0 + size.0 / 2.0))
        .collect();
    bolts.sort_by_key(|(entity, _)| entity.id());
    bolts
}

/// What a line at the bottom of the screen says about an event, if anything.
fn toast(sim: &Sim, event: &GameEvent) -> Option<String> {
    let name = |id: &str| {
        sim.items
            .get(id)
            .map_or_else(|| id.to_string(), |def| def.name.clone())
    };
    Some(match event {
        GameEvent::PickedUp { item, count } if *count > 1 => format!("{} x{count}", name(item)),
        GameEvent::PickedUp { item, .. } => name(item),
        GameEvent::InventoryFull { item } => format!("No room for {}", name(item)),
        GameEvent::Locked { key } => format!("Locked - it needs the {}", name(key)),
        GameEvent::Unlocked { key } => format!("Unlocked with the {}", name(key)),
        GameEvent::Checkpoint => "Checkpoint".to_string(),
        GameEvent::LeverPulled { .. } => "Somewhere, something moved".to_string(),
        GameEvent::TravelFailed { .. } => "The way is blocked".to_string(),
        _ => return None,
    })
}
