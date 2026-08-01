//! Things that *happened* during a tick, as opposed to things that are true
//! at the end of one.
//!
//! Every assertion tool in this crate samples state: the probe records where
//! the player is and what flags are set once per tick. That is the wrong shape
//! for most of what a game does. "The goblin died", "the chest opened", "the
//! quest advanced" are transient — a hit that lands and resolves inside a
//! single tick leaves no trace in any state field, so no state assertion can
//! ever see it. Sampling state at 60 Hz and hoping to catch the moment is not
//! a test.
//!
//! So the sim also emits events. They are cleared at the start of every tick,
//! recorded into the trace beside the probe, and counted cumulatively by the
//! tape runner so a tape can say `expect landed` or `expect no died`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::assets::Slot;
use crate::sim::Mode;

/// Event names addressable by tape `expect` directives. Kept beside
/// [`GameEvent::kind`] so the two cannot drift apart — a test enforces it.
const EVENT_NAMES: &[&str] = &[
    "jumped",
    "double_jumped",
    "wall_jumped",
    "landed",
    "dropped_through",
    "died",
    "respawned",
    "attacked",
    "damaged",
    "slid",
    "spell_cast",
    "cast_failed",
    "mode_changed",
    "picked_up",
    "inventory_full",
    "item_used",
    "equipped",
    "unequipped",
    "interact_prompted",
    "interacted",
    "dialogue_opened",
    "choice_taken",
    "dialogue_closed",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeathCause {
    Hazard,
    FellOutOfWorld,
    Slain,
}

/// Why a cast did not happen.
///
/// Not decoration. Without it, "the key did nothing" and "you were out of
/// mana" are the same absence in a trace, and telling them apart means
/// reasoning backwards from a mana column — which is exactly the afternoon
/// this enum is here to save.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CastFailure {
    /// Already swinging, already casting, or committed to a plunge.
    Busy,
    /// The pool is short of the spell's cost.
    NoMana,
    /// The cooldown from the last cast has not lapsed.
    Cooling,
    /// The caster names a spell `assets/data/spells.ron` does not define.
    /// A content bug, reported rather than swallowed.
    Unknown,
}

/// Serialized into the trace as `{"event": "landed", ...}`, so a JSONL trace
/// stays greppable by event name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum GameEvent {
    Jumped,
    DoubleJumped,
    /// `wall_dir` is the side the wall was on: -1 left, +1 right.
    WallJumped {
        wall_dir: f32,
    },
    Landed {
        on_one_way: bool,
    },
    DroppedThrough,
    Slid,
    /// `who` is the victim's kind — `"player"`, `"knight"`. Counting with
    /// `expect died == 1` cannot yet filter on it, but a trace that does not
    /// record who died is unreadable the moment two things can die.
    Died {
        who: String,
        cause: DeathCause,
    },
    Respawned,
    /// A swing started. Fires whether or not it goes on to connect, which is
    /// what lets a tape tell "missed" from "never swung".
    Attacked {
        attack: String,
    },
    /// A hit landed. `remaining` is the victim's health after it, so a tape
    /// can follow a fight without also tracking the damage table.
    Damaged {
        who: String,
        amount: i32,
        remaining: i32,
    },
    /// A cast started: the mana is spent and the caster is committed. Fires on
    /// the press, not on the release, because that is the tick the decision
    /// was made — the bolt appearing `cast_ticks` later is a consequence.
    SpellCast {
        spell: String,
    },
    /// A cast was refused, and why.
    CastFailed {
        spell: String,
        reason: CastFailure,
    },
    /// The simulation entered or left a modal state. Emitted on the tick the
    /// world freezes and the tick it unfreezes, so a trace says exactly which
    /// ticks were spent in a menu rather than leaving a stretch of identical
    /// frames for a reader to interpret.
    ModeChanged {
        from: Mode,
        to: Mode,
    },
    /// Something was walked over and went into the bag.
    PickedUp {
        item: String,
        count: u32,
    },
    /// There was no room. Covers both directions of the same problem: an item
    /// left on the floor because the bag is full, and a piece of equipment
    /// that could not be taken off because there was nowhere to put it.
    ///
    /// Not decoration, for the reason [`GameEvent::CastFailed`] is not: without
    /// it, "the pickup does not work" and "your bag is full" are the same
    /// absence in a trace. It fires once per approach rather than once per
    /// tick of standing on the thing.
    InventoryFull {
        item: String,
    },
    /// A consumable was spent.
    ItemUsed {
        item: String,
    },
    Equipped {
        item: String,
        slot: Slot,
    },
    Unequipped {
        item: String,
        slot: Slot,
    },
    /// Something interactable came within reach and is now being offered.
    /// Fires when the offer *changes*, not once per tick of standing there —
    /// the same rule [`GameEvent::InventoryFull`] follows, and for the same
    /// reason: sixty identical events a second is not a trace.
    ///
    /// `target` is what the press would do, which for now is a dialogue graph
    /// id, so `expect elder_intro.interact_prompted == 1` reads.
    InteractPrompted {
        target: String,
    },
    /// The key was actually pressed with something in reach. Emitted whatever
    /// the target turns out to be, so "I pressed E and nothing happened" is
    /// distinguishable from "there was nothing to press E on" — which is the
    /// same absence otherwise.
    Interacted {
        target: String,
    },
    /// A conversation started. The world freezes on this tick.
    DialogueOpened {
        graph: String,
    },
    /// A reply was taken. `index` is the choice's position in the *authored*
    /// list rather than in the filtered one on screen, so it names the same
    /// line of the same content file however much of it was on offer.
    ChoiceTaken {
        node: String,
        index: usize,
    },
    /// A conversation ended, by reaching a choice with no `next` or by being
    /// backed out of. Emitted before the `ModeChanged` that follows from it.
    DialogueClosed {
        graph: String,
    },
}

/// Where a subject's name has to come from, so that a mistyped one is a
/// rejected tape rather than a count of zero.
///
/// Every one of these is a *content* namespace: a subject is an id some RON
/// file defines, or a mode. That is what makes `expect no shcok.spell_cast`
/// checkable at all — the set of things it could have meant is finite and
/// loaded, so the tape parser can say which of them was probably meant. See
/// [`crate::sim::tape`], which resolves each of these against the shipped
/// tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Subject {
    /// An entity kind from `assets/data/stats.ron`: `player`, `knight`.
    Kind,
    /// An attack id from `assets/data/attacks.ron`.
    Attack,
    /// A spell id from `assets/data/spells.ron`.
    Spell,
    /// An item id from `assets/data/items/`.
    Item,
    /// A [`Mode`], spelled the way [`Mode::name`] spells it.
    Mode,
    /// A dialogue graph id from `assets/data/dialogue/`.
    Graph,
    /// A node id inside some dialogue graph.
    Node,
}

impl Subject {
    /// What to call this in an error message, as "`x` is not ...".
    pub fn label(self) -> &'static str {
        match self {
            Subject::Kind => "an entity kind (assets/data/stats.ron)",
            Subject::Attack => "an attack id (assets/data/attacks.ron)",
            Subject::Spell => "a spell id (assets/data/spells.ron)",
            Subject::Item => "an item id (assets/data/items/)",
            Subject::Mode => "a mode",
            Subject::Graph => "a dialogue graph id (assets/data/dialogue/)",
            Subject::Node => "a dialogue node id (assets/data/dialogue/)",
        }
    }
}

/// Every mode, by the name a tape addresses it with.
///
/// Hand-written for the reason `every_variant` in this file's tests is: stable
/// Rust cannot enumerate an enum. `mode_list_is_exhaustive` below is the
/// guard — it matches on `Mode` exhaustively, so adding a variant stops this
/// file compiling until the variant is added here too.
pub const MODES: &[Mode] = &[Mode::Playing, Mode::Inventory, Mode::Dialogue];

/// Events carrying a discriminating name that `expect` can filter on, and
/// where that name has to come from: `knight.damaged` is a kind,
/// `player_slash3.attacked` an attack id, `shock.spell_cast` a spell id.
///
/// `interact_prompted` and `interacted` say `Graph` because what an
/// interactable offers today is a conversation and nothing else. The day a
/// door or a chest is interactable, that is the line to widen — and a tape
/// naming one will be rejected here until it is, which is the point.
const SUBJECT_EVENTS: &[(&str, Subject)] = &[
    ("died", Subject::Kind),
    ("damaged", Subject::Kind),
    ("attacked", Subject::Attack),
    ("spell_cast", Subject::Spell),
    ("cast_failed", Subject::Spell),
    ("mode_changed", Subject::Mode),
    ("picked_up", Subject::Item),
    ("inventory_full", Subject::Item),
    ("item_used", Subject::Item),
    ("equipped", Subject::Item),
    ("unequipped", Subject::Item),
    ("interact_prompted", Subject::Graph),
    ("interacted", Subject::Graph),
    ("dialogue_opened", Subject::Graph),
    ("choice_taken", Subject::Node),
    ("dialogue_closed", Subject::Graph),
];

impl GameEvent {
    /// The name this event can be filtered by: the victim for damage and
    /// death, the attack id for a swing, the spell id for a cast.
    pub fn subject(&self) -> Option<&str> {
        match self {
            GameEvent::Died { who, .. } | GameEvent::Damaged { who, .. } => Some(who),
            GameEvent::Attacked { attack } => Some(attack),
            GameEvent::SpellCast { spell } | GameEvent::CastFailed { spell, .. } => Some(spell),
            // The mode entered, so `expect inventory.mode_changed == 1` counts
            // openings and not openings-and-closings.
            GameEvent::ModeChanged { to, .. } => Some(to.name()),
            GameEvent::PickedUp { item, .. }
            | GameEvent::InventoryFull { item }
            | GameEvent::ItemUsed { item }
            | GameEvent::Equipped { item, .. }
            | GameEvent::Unequipped { item, .. } => Some(item),
            GameEvent::InteractPrompted { target } | GameEvent::Interacted { target } => {
                Some(target)
            }
            GameEvent::DialogueOpened { graph } | GameEvent::DialogueClosed { graph } => {
                Some(graph)
            }
            // The node rather than the graph: `expect greet.choice_taken == 2`
            // is the question worth asking of a conversation, and the graph is
            // already countable through `dialogue_opened`.
            GameEvent::ChoiceTaken { node, .. } => Some(node),
            _ => None,
        }
    }

    /// Whether `expect <subject>.<name>` is meaningful for this event name.
    pub fn has_subject(name: &str) -> bool {
        GameEvent::subject_kind(name).is_some()
    }

    /// What a subject of this event has to be a name *of*, or `None` for an
    /// event that records nobody.
    pub fn subject_kind(name: &str) -> Option<Subject> {
        SUBJECT_EVENTS
            .iter()
            .find(|(event, _)| *event == name)
            .map(|(_, kind)| *kind)
    }

    pub fn subject_names() -> String {
        SUBJECT_EVENTS
            .iter()
            .map(|(event, _)| *event)
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The name a tape refers to this event by.
    pub fn kind(&self) -> &'static str {
        match self {
            GameEvent::Jumped => "jumped",
            GameEvent::DoubleJumped => "double_jumped",
            GameEvent::WallJumped { .. } => "wall_jumped",
            GameEvent::Landed { .. } => "landed",
            GameEvent::DroppedThrough => "dropped_through",
            GameEvent::Slid => "slid",
            GameEvent::Died { .. } => "died",
            GameEvent::Respawned => "respawned",
            GameEvent::Attacked { .. } => "attacked",
            GameEvent::Damaged { .. } => "damaged",
            GameEvent::SpellCast { .. } => "spell_cast",
            GameEvent::CastFailed { .. } => "cast_failed",
            GameEvent::ModeChanged { .. } => "mode_changed",
            GameEvent::PickedUp { .. } => "picked_up",
            GameEvent::InventoryFull { .. } => "inventory_full",
            GameEvent::ItemUsed { .. } => "item_used",
            GameEvent::Equipped { .. } => "equipped",
            GameEvent::Unequipped { .. } => "unequipped",
            GameEvent::InteractPrompted { .. } => "interact_prompted",
            GameEvent::Interacted { .. } => "interacted",
            GameEvent::DialogueOpened { .. } => "dialogue_opened",
            GameEvent::ChoiceTaken { .. } => "choice_taken",
            GameEvent::DialogueClosed { .. } => "dialogue_closed",
        }
    }

    pub fn names() -> &'static [&'static str] {
        EVENT_NAMES
    }

    /// Every name `expect` accepts, for error messages.
    pub fn known_names() -> String {
        EVENT_NAMES.join(", ")
    }
}

/// A running tally of events by kind.
///
/// Cumulative rather than per-tick because that is how a tape reads: by the
/// time you have written `right+jump 25`, you want to assert the player landed
/// *somewhere* in there, not guess which tick it was.
#[derive(Clone, Debug, Default)]
pub struct EventCounts {
    by_kind: HashMap<&'static str, u32>,
    /// Counted separately by who it happened to, so that `damaged` can mean
    /// "any damage" and `knight.damaged` can mean "damage to the knight".
    /// Once both sides of a fight can bleed, the unqualified count stops
    /// being a useful assertion on its own.
    by_subject: HashMap<(String, &'static str), u32>,
}

impl EventCounts {
    pub fn new() -> Self {
        EventCounts::default()
    }

    pub fn record(&mut self, events: &[GameEvent]) {
        for event in events {
            *self.by_kind.entry(event.kind()).or_insert(0) += 1;
            if let Some(subject) = event.subject() {
                *self
                    .by_subject
                    .entry((subject.to_string(), event.kind()))
                    .or_insert(0) += 1;
            }
        }
    }

    /// How many times `name` fired, whoever it happened to.
    pub fn count(&self, name: &str) -> u32 {
        self.by_kind.get(name).copied().unwrap_or(0)
    }

    /// How many times `name` fired for `subject`.
    pub fn count_for(&self, subject: &str, name: &str) -> u32 {
        self.by_subject
            .get(&(subject.to_string(), name))
            .copied()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One sample of every variant. There is no way to enumerate an enum's
    /// variants in stable Rust, so adding a variant means adding it here —
    /// and the compiler's exhaustiveness check on `kind` is what reminds you.
    fn every_variant() -> Vec<GameEvent> {
        vec![
            GameEvent::Jumped,
            GameEvent::DoubleJumped,
            GameEvent::WallJumped { wall_dir: 1.0 },
            GameEvent::Landed { on_one_way: false },
            GameEvent::DroppedThrough,
            GameEvent::Slid,
            GameEvent::Died {
                who: "player".to_string(),
                cause: DeathCause::Hazard,
            },
            GameEvent::Respawned,
            GameEvent::Attacked {
                attack: "player_slash".to_string(),
            },
            GameEvent::Damaged {
                who: "knight".to_string(),
                amount: 1,
                remaining: 2,
            },
            GameEvent::SpellCast {
                spell: "shock".to_string(),
            },
            GameEvent::CastFailed {
                spell: "shock".to_string(),
                reason: CastFailure::NoMana,
            },
            GameEvent::ModeChanged {
                from: Mode::Playing,
                to: Mode::Inventory,
            },
            GameEvent::PickedUp {
                item: "minor_potion".to_string(),
                count: 1,
            },
            GameEvent::InventoryFull {
                item: "iron_sword".to_string(),
            },
            GameEvent::ItemUsed {
                item: "minor_potion".to_string(),
            },
            GameEvent::Equipped {
                item: "knight_helm".to_string(),
                slot: Slot::Head,
            },
            GameEvent::Unequipped {
                item: "knight_helm".to_string(),
                slot: Slot::Head,
            },
            GameEvent::InteractPrompted {
                target: "elder_intro".to_string(),
            },
            GameEvent::Interacted {
                target: "elder_intro".to_string(),
            },
            GameEvent::DialogueOpened {
                graph: "elder_intro".to_string(),
            },
            GameEvent::ChoiceTaken {
                node: "greet".to_string(),
                index: 1,
            },
            GameEvent::DialogueClosed {
                graph: "elder_intro".to_string(),
            },
        ]
    }

    /// The advertised names and the actual `kind()` values must agree, or a
    /// tape would be rejected for using a documented event.
    #[test]
    fn advertised_names_match_the_variants() {
        let kinds: Vec<&str> = every_variant().iter().map(|e| e.kind()).collect();
        for name in GameEvent::names() {
            assert!(
                kinds.contains(name),
                "`{name}` is advertised but unreachable"
            );
        }
        for kind in &kinds {
            assert!(
                GameEvent::names().contains(kind),
                "`{kind}` is emitted but not advertised to tapes"
            );
        }
        assert_eq!(kinds.len(), GameEvent::names().len(), "duplicate kind name");
    }

    #[test]
    fn counts_accumulate_by_kind() {
        let mut counts = EventCounts::new();
        counts.record(&[GameEvent::Jumped, GameEvent::Landed { on_one_way: true }]);
        counts.record(&[GameEvent::Jumped]);

        assert_eq!(counts.count("jumped"), 2);
        assert_eq!(counts.count("landed"), 1);
        assert_eq!(
            counts.count("died"),
            0,
            "unseen kinds count zero, not absent"
        );
    }

    /// Once both sides can bleed, "three damage events" is not an assertion
    /// about anyone in particular.
    #[test]
    fn damage_is_countable_per_victim() {
        let mut counts = EventCounts::new();
        counts.record(&[
            GameEvent::Damaged {
                who: "knight".to_string(),
                amount: 1,
                remaining: 2,
            },
            GameEvent::Damaged {
                who: "player".to_string(),
                amount: 1,
                remaining: 4,
            },
            GameEvent::Damaged {
                who: "knight".to_string(),
                amount: 1,
                remaining: 1,
            },
        ]);

        assert_eq!(counts.count("damaged"), 3, "everything, unqualified");
        assert_eq!(counts.count_for("knight", "damaged"), 2);
        assert_eq!(counts.count_for("player", "damaged"), 1);
        assert_eq!(counts.count_for("goblin", "damaged"), 0);
    }

    /// [`MODES`] has to be every mode, because it is the whole of what
    /// `expect <mode>.mode_changed` will accept. The match is what enforces
    /// it: a new variant fails to compile here, and the fix is one line in
    /// each place.
    #[test]
    fn mode_list_is_exhaustive() {
        fn index(mode: Mode) -> usize {
            match mode {
                Mode::Playing => 0,
                Mode::Inventory => 1,
                Mode::Dialogue => 2,
            }
        }
        let indices: Vec<usize> = MODES.iter().copied().map(index).collect();
        assert_eq!(indices, (0..MODES.len()).collect::<Vec<_>>());
    }

    /// Every event that records a subject has to say what kind of name that
    /// subject is, or the tape parser cannot check it against anything.
    #[test]
    fn every_subject_event_names_a_namespace() {
        for event in every_variant().iter().filter(|e| e.subject().is_some()) {
            assert!(
                GameEvent::subject_kind(event.kind()).is_some(),
                "`{}` records a subject but does not say what it is a name of",
                event.kind()
            );
        }
    }

    #[test]
    fn only_some_events_have_a_subject() {
        assert!(GameEvent::has_subject("damaged"));
        assert!(GameEvent::has_subject("died"));
        assert!(!GameEvent::has_subject("jumped"));
        for event in every_variant() {
            assert_eq!(
                event.subject().is_some(),
                GameEvent::has_subject(event.kind()),
                "`{}` disagrees about whether it has a subject",
                event.kind()
            );
        }
    }

    /// Events go into the trace, so they have to survive the round trip.
    #[test]
    fn events_round_trip_through_json() {
        for event in every_variant() {
            let json = serde_json::to_string(&event).unwrap();
            assert!(
                json.contains(event.kind()),
                "`{json}` should be greppable by kind"
            );
            let parsed: GameEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(event, parsed);
        }
    }
}
