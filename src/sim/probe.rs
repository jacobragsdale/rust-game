//! A flat snapshot of the player's state at one tick.
//!
//! Fields are plain scalars rather than `Vec2` for two reasons: glam's serde
//! support is not enabled through ggez, and flat fields make a JSONL trace
//! greppable and line-diffable.

use ggez::glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::ecs::components::{AnimationState, Attacking, Avatar, Body, Casting, Health, Mana};
use crate::sim::Mode;
use crate::systems::dialogue::{self, Conversation, Prompt};
use crate::systems::inventory::Screen;

/// `coyote_ticks` uses `u32::MAX`-ish sentinels to mean "no coyote jump
/// available". Clamping keeps traces readable; any value past the coyote
/// window is behaviorally identical anyway.
const TICK_DISPLAY_CAP: u32 = 999;

/// Numeric fields addressable by tape assertions. Kept beside `Probe::field`
/// so the two cannot drift apart — a test enforces it.
const FIELD_NAMES: &[&str] = &[
    "x",
    "y",
    "vx",
    "vy",
    "tick",
    "air_jumps",
    "frame",
    "hp",
    "hp_max",
    "iframes",
    "hitstun",
    "mana",
    "mana_max",
    "cast_cooldown",
    "projectiles",
    "pickups",
    "inventory_count",
    "selection",
    "dialogue_choices",
    "dialogue_selection",
];

/// Text fields addressable by tape assertions. Only `==` and `!=` apply.
const TEXT_NAMES: &[&str] = &["clip", "mode", "pane", "prompt", "dialogue_node"];

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
    "casting",
    "plunging",
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
    /// Maximum health *as derived* — base plus whatever is equipped. This is
    /// the field a tape watches to say "the helm did something", since current
    /// health is clamped rather than raised when the maximum moves.
    pub hp_max: i32,
    pub iframes: u32,
    pub hitstun: u32,
    pub attacking: bool,
    /// Mana in the pool, and its size. Zero for anything with no pool at all,
    /// which is indistinguishable from an empty one — and correctly so: both
    /// mean "cannot cast".
    pub mana: i32,
    pub mana_max: i32,
    /// Ticks until another cast may start.
    pub cast_cooldown: u32,
    pub casting: bool,
    /// Committed to a plunge, in any of its three phases.
    pub plunging: bool,
    /// How many projectiles are in the air, anywhere in the world.
    pub projectiles: usize,
    /// How many items are lying on the floor, anywhere in the world.
    pub pickups: usize,
    /// Occupied bag slots, which is a count of *kinds* carried: stacking means
    /// six potions are one. `item.<id>.count` is the per-item number.
    pub inventory_count: usize,
    /// What the simulation is doing: `playing`, `inventory`, `dialogue`.
    pub mode: String,
    /// Which pane of the inventory screen has focus, and where the selection
    /// is in it. Both are meaningful whatever the mode — the screen remembers
    /// them while it is shut.
    pub pane: String,
    pub selection: usize,
    /// What pressing `Interact` would act on right now, by the word the
    /// interactable offers — `talk` — or [`dialogue::NO_PROMPT`] when nothing
    /// is in reach. A word rather than an empty string so a tape can assert
    /// both states; see that constant for why.
    pub prompt: String,
    /// Where a conversation is: the current node's id, or
    /// [`dialogue::NO_PROMPT`] when there is no conversation.
    pub dialogue_node: String,
    /// How many replies the current node offers. This is the *filtered* count —
    /// choices whose conditions fail are hidden rather than greyed out, so it
    /// is a count of what the player can actually do, and it going up is how a
    /// tape sees a gated branch unlock.
    ///
    /// It is the node's count from the moment the node is entered, including
    /// while there is still speech to page through: the filtering is a fact
    /// about the world, not about how far into a speech you have read. Only
    /// whether the overlay *draws* them waits for the last line, which is
    /// `Conversation::choosing`.
    pub dialogue_choices: usize,
    /// Which of those is highlighted.
    pub dialogue_selection: usize,
    pub clip: String,
    pub frame: usize,
}

/// A snapshot of one thing the player is carrying or wearing, addressable as
/// `item.<id>.<field>`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemProbe {
    pub id: String,
    /// How many are in the bag. Zero for something that is worn and no longer
    /// carried.
    pub count: u32,
    pub equipped: bool,
}

/// Numeric fields of an item.
const ITEM_FIELD_NAMES: &[&str] = &["count"];

/// Boolean fields of an item.
const ITEM_FLAG_NAMES: &[&str] = &["equipped"];

impl ItemProbe {
    pub fn field(&self, name: &str) -> Option<f32> {
        match name {
            "count" => Some(self.count as f32),
            _ => None,
        }
    }

    pub fn flag(&self, name: &str) -> Option<bool> {
        match name {
            "equipped" => Some(self.equipped),
            _ => None,
        }
    }

    pub fn field_names() -> &'static [&'static str] {
        ITEM_FIELD_NAMES
    }

    pub fn flag_names() -> &'static [&'static str] {
        ITEM_FLAG_NAMES
    }

    pub fn known_names() -> String {
        let mut all: Vec<&str> = ITEM_FIELD_NAMES.to_vec();
        all.extend_from_slice(ITEM_FLAG_NAMES);
        all.join(", ")
    }

    /// What an item nobody is carrying looks like: none of it, not worn.
    ///
    /// `item.minor_potion.count == 0` has to resolve *before* the first potion
    /// is ever picked up, or a tape could only assert the half of a pickup
    /// that comes after it.
    pub fn absent(id: &str) -> ItemProbe {
        ItemProbe {
            id: id.to_string(),
            count: 0,
            equipped: false,
        }
    }
}

/// Numeric fields of an NPC, addressable as `<kind>.<index>.<field>`.
const NPC_FIELD_NAMES: &[&str] = &["x", "y", "vx", "vy", "dir", "frame", "hp", "hitstun"];

/// Boolean fields of an NPC.
const NPC_FLAG_NAMES: &[&str] = &["grounded", "facing_right", "dead"];

/// Text fields of an NPC.
const NPC_TEXT_NAMES: &[&str] = &["clip", "kind"];

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

    pub fn text(&self, name: &str) -> Option<&str> {
        match name {
            "clip" => Some(&self.clip),
            "kind" => Some(&self.kind),
            _ => None,
        }
    }

    pub fn field_names() -> &'static [&'static str] {
        NPC_FIELD_NAMES
    }

    pub fn flag_names() -> &'static [&'static str] {
        NPC_FLAG_NAMES
    }

    pub fn text_names() -> &'static [&'static str] {
        NPC_TEXT_NAMES
    }

    pub fn known_names() -> String {
        let mut all: Vec<&str> = NPC_FIELD_NAMES.to_vec();
        all.extend_from_slice(NPC_FLAG_NAMES);
        all.extend_from_slice(NPC_TEXT_NAMES);
        all.join(", ")
    }
}

impl Probe {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tick: u64,
        mode: Mode,
        avatar: &Avatar,
        body: &Body,
        health: &Health,
        attacking: &Attacking,
        pos: Vec2,
        vel: Vec2,
        anim: &AnimationState,
        mana: Option<&Mana>,
        casting: Option<&Casting>,
        projectiles: usize,
        pickups: usize,
        inventory_count: usize,
        screen: &Screen,
        prompt: Option<&Prompt>,
        talk: Option<&Conversation>,
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
            hp_max: health.max,
            iframes: health.iframes,
            hitstun: health.hitstun,
            attacking: attacking.busy(),
            mana: mana.map_or(0, |m| m.current),
            mana_max: mana.map_or(0, |m| m.max),
            cast_cooldown: casting.map_or(0, |c| c.cooldown),
            casting: casting.is_some_and(|c| c.busy()),
            plunging: avatar.plunging(),
            projectiles,
            pickups,
            inventory_count,
            mode: mode.name().to_string(),
            pane: screen.pane.name().to_string(),
            selection: screen.selection,
            prompt: prompt
                .map_or(dialogue::NO_PROMPT, |p| p.prompt.as_str())
                .to_string(),
            dialogue_node: talk
                .map_or(dialogue::NO_PROMPT, |t| t.node_id())
                .to_string(),
            dialogue_choices: talk.map_or(0, |t| t.choice_count()),
            dialogue_selection: talk.map_or(0, |t| t.selection()),
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
            "hp_max" => self.hp_max as f32,
            "iframes" => self.iframes as f32,
            "hitstun" => self.hitstun as f32,
            "mana" => self.mana as f32,
            "mana_max" => self.mana_max as f32,
            "cast_cooldown" => self.cast_cooldown as f32,
            "projectiles" => self.projectiles as f32,
            "pickups" => self.pickups as f32,
            "inventory_count" => self.inventory_count as f32,
            "selection" => self.selection as f32,
            "dialogue_choices" => self.dialogue_choices as f32,
            "dialogue_selection" => self.dialogue_selection as f32,
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
            "casting" => self.casting,
            "plunging" => self.plunging,
            _ => return None,
        })
    }

    /// Text field lookup, used by tape assertions.
    pub fn text(&self, name: &str) -> Option<&str> {
        match name {
            "clip" => Some(&self.clip),
            "mode" => Some(&self.mode),
            "pane" => Some(&self.pane),
            "prompt" => Some(&self.prompt),
            "dialogue_node" => Some(&self.dialogue_node),
            _ => None,
        }
    }

    pub fn field_names() -> &'static [&'static str] {
        FIELD_NAMES
    }

    pub fn flag_names() -> &'static [&'static str] {
        FLAG_NAMES
    }

    pub fn text_names() -> &'static [&'static str] {
        TEXT_NAMES
    }

    /// Every name `field`, `flag` or `text` accepts, for error messages.
    pub fn known_names() -> String {
        let mut all: Vec<&str> = FIELD_NAMES.to_vec();
        all.extend_from_slice(FLAG_NAMES);
        all.extend_from_slice(TEXT_NAMES);
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
    use crate::assets::StatTable;

    fn sample() -> Probe {
        let stats = StatTable::shipped().get("player").unwrap();
        Probe::new(
            0,
            Mode::Playing,
            &Avatar::new(stats.avatar()),
            &Body::new(Vec2::ZERO, stats.gravity, stats.max_fall),
            &Health::new(stats.max_health, stats.iframe_ticks),
            &Attacking::default(),
            Vec2::ZERO,
            Vec2::ZERO,
            &AnimationState::new("idle"),
            Some(&Mana::new(stats.max_mana, stats.mana_regen)),
            Some(&Casting::default()),
            0,
            0,
            0,
            &Screen::default(),
            None,
            None,
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
        for name in Probe::text_names() {
            assert!(probe.text(name).is_some(), "text `{name}` does not resolve");
            assert!(
                probe.field(name).is_none() && probe.flag(name).is_none(),
                "`{name}` is text and something else as well"
            );
        }
    }

    /// The item probe's advertised names have to resolve too, for the same
    /// reason: a documented field that a tape is rejected for using is worse
    /// than no field at all.
    #[test]
    fn advertised_item_names_all_resolve() {
        let probe = ItemProbe {
            id: "minor_potion".to_string(),
            count: 2,
            equipped: false,
        };
        for name in ItemProbe::field_names() {
            assert!(probe.field(name).is_some(), "`{name}` does not resolve");
            assert!(probe.flag(name).is_none(), "`{name}` is field and flag");
        }
        for name in ItemProbe::flag_names() {
            assert!(probe.flag(name).is_some(), "`{name}` does not resolve");
            assert!(probe.field(name).is_none(), "`{name}` is flag and field");
        }
        assert_eq!(ItemProbe::absent("anything").count, 0);
    }

    /// The mode is a text field so that `assert mode == inventory` reads the
    /// way it should, and every mode has to spell itself.
    #[test]
    fn every_mode_has_a_name_a_tape_can_write() {
        for mode in [Mode::Playing, Mode::Inventory, Mode::Dialogue] {
            assert!(!mode.name().is_empty());
            assert_eq!(mode.is_modal(), mode != Mode::Playing);
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
