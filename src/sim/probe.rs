//! A flat snapshot of the player's state at one tick.
//!
//! Fields are plain scalars rather than `Vec2` for two reasons: glam's serde
//! support is not enabled through ggez, and flat fields make a JSONL trace
//! greppable and line-diffable.

use ggez::glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::ecs::components::{AnimationState, Attacking, Avatar, Body, Health};

/// `coyote_ticks` uses `u32::MAX`-ish sentinels to mean "no coyote jump
/// available". Clamping keeps traces readable; any value past the coyote
/// window is behaviorally identical anyway.
const TICK_DISPLAY_CAP: u32 = 999;

/// Numeric fields addressable by tape assertions. Kept beside `Probe::field`
/// so the two cannot drift apart — a test enforces it.
const FIELD_NAMES: &[&str] = &[
    "x", "y", "vx", "vy", "tick", "air_jumps", "frame", "hp", "iframes", "hitstun",
];

/// Boolean fields addressable by tape assertions.
const FLAG_NAMES: &[&str] = &[
    "grounded",
    "facing_right",
    "wall_sliding",
    "double_jumping",
    "crouching",
    "dead",
    "on_one_way_only",
    "attacking",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Probe {
    pub tick: u64,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub grounded: bool,
    pub facing_right: bool,
    pub wall_sliding: bool,
    pub double_jumping: bool,
    pub crouching: bool,
    pub dead: bool,
    pub on_one_way_only: bool,
    pub air_jumps: u8,
    pub coyote_ticks: u32,
    pub wall_coyote_ticks: u32,
    pub jump_buffer: u32,
    pub hp: i32,
    pub iframes: u32,
    pub hitstun: u32,
    pub attacking: bool,
    pub clip: String,
    pub frame: usize,
}

/// Numeric fields of an NPC, addressable as `<kind>.<index>.<field>`.
const NPC_FIELD_NAMES: &[&str] = &["x", "y", "vx", "vy", "dir", "frame", "hp", "hitstun"];

/// Boolean fields of an NPC.
const NPC_FLAG_NAMES: &[&str] = &["grounded", "facing_right", "dead"];

/// A snapshot of one NPC.
///
/// Deliberately much thinner than the player's: an NPC has no coyote timer or
/// jump buffer, and a trace with a line per tick per entity gets unreadable
/// fast. Fields get added here when something needs to assert on them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NpcProbe {
    pub kind: String,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    /// Travel direction: -1 left, +1 right.
    pub dir: f32,
    pub grounded: bool,
    pub hp: i32,
    pub hitstun: u32,
    pub dead: bool,
    pub clip: String,
    pub frame: usize,
}

impl NpcProbe {
    pub fn field(&self, name: &str) -> Option<f32> {
        Some(match name {
            "x" => self.x,
            "y" => self.y,
            "vx" => self.vx,
            "vy" => self.vy,
            "dir" => self.dir,
            "frame" => self.frame as f32,
            "hp" => self.hp as f32,
            "hitstun" => self.hitstun as f32,
            _ => return None,
        })
    }

    pub fn flag(&self, name: &str) -> Option<bool> {
        Some(match name {
            "grounded" => self.grounded,
            "facing_right" => self.dir >= 0.0,
            "dead" => self.dead,
            _ => return None,
        })
    }

    pub fn field_names() -> &'static [&'static str] {
        NPC_FIELD_NAMES
    }

    pub fn flag_names() -> &'static [&'static str] {
        NPC_FLAG_NAMES
    }

    pub fn known_names() -> String {
        let mut all: Vec<&str> = NPC_FIELD_NAMES.to_vec();
        all.extend_from_slice(NPC_FLAG_NAMES);
        all.join(", ")
    }
}

impl Probe {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tick: u64,
        avatar: &Avatar,
        body: &Body,
        health: &Health,
        attacking: &Attacking,
        pos: Vec2,
        vel: Vec2,
        anim: &AnimationState,
    ) -> Self {
        Probe {
            tick,
            x: pos.x,
            y: pos.y,
            vx: vel.x,
            vy: vel.y,
            grounded: body.grounded,
            facing_right: avatar.facing_right,
            wall_sliding: avatar.wall_sliding,
            double_jumping: avatar.double_jumping,
            crouching: avatar.crouching,
            dead: avatar.dead(),
            on_one_way_only: body.on_one_way_only(),
            air_jumps: avatar.air_jumps,
            coyote_ticks: avatar.coyote_ticks.min(TICK_DISPLAY_CAP),
            wall_coyote_ticks: avatar.wall_coyote_ticks.min(TICK_DISPLAY_CAP),
            jump_buffer: avatar.jump_buffer,
            hp: health.current,
            iframes: health.iframes,
            hitstun: health.hitstun,
            attacking: attacking.busy(),
            clip: anim.clip.clone(),
            frame: anim.frame,
        }
    }

    /// Numeric field lookup, used by tape assertions.
    pub fn field(&self, name: &str) -> Option<f32> {
        Some(match name {
            "x" => self.x,
            "y" => self.y,
            "vx" => self.vx,
            "vy" => self.vy,
            "tick" => self.tick as f32,
            "air_jumps" => self.air_jumps as f32,
            "frame" => self.frame as f32,
            "hp" => self.hp as f32,
            "iframes" => self.iframes as f32,
            "hitstun" => self.hitstun as f32,
            _ => return None,
        })
    }

    /// Boolean field lookup, used by tape assertions.
    pub fn flag(&self, name: &str) -> Option<bool> {
        Some(match name {
            "grounded" => self.grounded,
            "facing_right" => self.facing_right,
            "wall_sliding" => self.wall_sliding,
            "double_jumping" => self.double_jumping,
            "crouching" => self.crouching,
            "dead" => self.dead,
            "on_one_way_only" => self.on_one_way_only,
            "attacking" => self.attacking,
            _ => return None,
        })
    }

    pub fn field_names() -> &'static [&'static str] {
        FIELD_NAMES
    }

    pub fn flag_names() -> &'static [&'static str] {
        FLAG_NAMES
    }

    /// Every name `field` or `flag` accepts, for error messages.
    pub fn known_names() -> String {
        let mut all: Vec<&str> = FIELD_NAMES.to_vec();
        all.extend_from_slice(FLAG_NAMES);
        all.join(", ")
    }

    /// Compact human-readable summary for terminal output.
    pub fn summary(&self) -> String {
        format!(
            "tick {:>5}  pos ({:>8.2}, {:>8.2})  vel ({:>8.2}, {:>8.2})  \
             {:<11} clip {}[{}]{}",
            self.tick,
            self.x,
            self.y,
            self.vx,
            self.vy,
            if self.grounded {
                "grounded"
            } else {
                "airborne"
            },
            self.clip,
            self.frame,
            if self.dead { "  DEAD" } else { "" },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Probe {
        Probe::new(
            0,
            &Avatar::new(),
            &Avatar::body(Vec2::ZERO),
            &Health::new(Avatar::MAX_HEALTH, Health::PLAYER_IFRAMES),
            &Attacking::default(),
            Vec2::ZERO,
            Vec2::ZERO,
            &AnimationState::new("idle"),
        )
    }

    /// The advertised name lists and the actual accessors must agree, or a
    /// tape assertion would be rejected for using a documented field.
    #[test]
    fn advertised_names_all_resolve() {
        let probe = sample();
        for name in Probe::field_names() {
            assert!(
                probe.field(name).is_some(),
                "field `{name}` does not resolve"
            );
            assert!(
                probe.flag(name).is_none(),
                "`{name}` is both field and flag"
            );
        }
        for name in Probe::flag_names() {
            assert!(probe.flag(name).is_some(), "flag `{name}` does not resolve");
            assert!(
                probe.field(name).is_none(),
                "`{name}` is both flag and field"
            );
        }
    }

    #[test]
    fn unknown_names_do_not_resolve() {
        let probe = sample();
        assert!(probe.field("nonsense").is_none());
        assert!(probe.flag("nonsense").is_none());
    }

    /// The coyote sentinels are `u32::MAX`-ish; traces should stay readable.
    #[test]
    fn coyote_sentinels_are_clamped() {
        assert_eq!(sample().coyote_ticks, TICK_DISPLAY_CAP);
        assert_eq!(sample().wall_coyote_ticks, TICK_DISPLAY_CAP);
    }
}
