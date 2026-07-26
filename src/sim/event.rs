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
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeathCause {
    Hazard,
    FellOutOfWorld,
    Slain,
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
}

impl GameEvent {
    /// The name a tape refers to this event by.
    pub fn kind(&self) -> &'static str {
        match self {
            GameEvent::Jumped => "jumped",
            GameEvent::DoubleJumped => "double_jumped",
            GameEvent::WallJumped { .. } => "wall_jumped",
            GameEvent::Landed { .. } => "landed",
            GameEvent::DroppedThrough => "dropped_through",
            GameEvent::Died { .. } => "died",
            GameEvent::Respawned => "respawned",
            GameEvent::Attacked { .. } => "attacked",
            GameEvent::Damaged { .. } => "damaged",
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
pub struct EventCounts(HashMap<&'static str, u32>);

impl EventCounts {
    pub fn new() -> Self {
        EventCounts::default()
    }

    pub fn record(&mut self, events: &[GameEvent]) {
        for event in events {
            *self.0.entry(event.kind()).or_insert(0) += 1;
        }
    }

    pub fn count(&self, name: &str) -> u32 {
        self.0.get(name).copied().unwrap_or(0)
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
