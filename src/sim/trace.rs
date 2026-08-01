//! Tick-by-tick trace recording, written as JSONL.
//!
//! One frame per line: the [`Probe`] fields inline, plus any [`GameEvent`]s
//! from that tick. The format is deliberately line-oriented because the sim is
//! deterministic: running the same tape before and after a change and then
//! running `diff` over the two traces reports the exact tick at which behavior
//! changed. That makes "my refactor subtly altered the jump arc" a
//! mechanically detectable regression rather than something you only notice
//! while playing.
//!
//! `tests/traces.rs` turns that from a workflow you have to remember into a
//! test that runs on every `cargo test`.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use crate::sim::probe::{ItemProbe, NpcProbe};
use crate::sim::{GameEvent, Probe};

/// One tick's worth of trace: what was true, and what happened.
///
/// `Probe`'s fields are flattened into the frame so a line stays greppable
/// (`{"tick":12,"x":...,"events":[...]}`) and an events-free frame serializes
/// exactly as the probe alone would.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    #[serde(flatten)]
    pub probe: Probe,
    /// Every NPC, in spawn order. Nested rather than flattened so the player's
    /// columns stay where they have always been and a trace diff over a map
    /// with no NPCs is unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub npcs: Vec<NpcProbe>,
    /// Everything carried or worn. Nested and omitted when empty, for the same
    /// two reasons `npcs` is: a bag is a list rather than a column, and a run
    /// that never picks anything up serializes exactly as it did before items
    /// existed — so adding this churned no baseline by itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ItemProbe>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<GameEvent>,
    /// The random seed this run used, recorded on the first frame so a trace
    /// says what it takes to reproduce it.
    ///
    /// Absent means [`crate::sim::rng::DEFAULT_SEED`]: writing it out on every
    /// baseline would churn all of them to record the one value that is
    /// already implied, which is exactly the kind of noise that makes a trace
    /// diff unreadable. A tape that sets a seed says so, here, on line 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

impl Frame {
    /// Resolve a dotted path to a number, for tape assertions.
    ///
    /// `x` and `player.x` are the player; `knight.0.x` is the first knight in
    /// spawn order. The bare form stays the player's so that every tape
    /// written before there were NPCs still means what it said.
    pub fn field(&self, path: &str) -> Option<f32> {
        match ProbePath::parse(path)? {
            ProbePath::Player(name) => self.probe.field(name),
            ProbePath::Npc { kind, index, name } => self.npc(kind, index)?.field(name),
            ProbePath::Item { id, name } => self.item(id).field(name),
        }
    }

    /// Resolve a dotted path to a boolean, for tape assertions.
    pub fn flag(&self, path: &str) -> Option<bool> {
        match ProbePath::parse(path)? {
            ProbePath::Player(name) => self.probe.flag(name),
            ProbePath::Npc { kind, index, name } => self.npc(kind, index)?.flag(name),
            ProbePath::Item { id, name } => self.item(id).flag(name),
        }
    }

    /// Resolve a dotted path to a string, for tape assertions.
    pub fn text(&self, path: &str) -> Option<&str> {
        match ProbePath::parse(path)? {
            ProbePath::Player(name) => self.probe.text(name),
            ProbePath::Npc { kind, index, name } => self.npc(kind, index)?.text(name),
            // Nothing about an item is text: the id is in the path already.
            ProbePath::Item { .. } => None,
        }
    }

    /// The `index`-th NPC of `kind`, in spawn order.
    fn npc(&self, kind: &str, index: usize) -> Option<&NpcProbe> {
        self.npcs.iter().filter(|n| n.kind == kind).nth(index)
    }

    /// What is known about `id`, which for something nobody is carrying is
    /// "none of it".
    ///
    /// Deliberately not `None`: `assert item.minor_potion.count == 0` has to
    /// mean something *before* the first potion is picked up, or a tape could
    /// only ever assert the second half of a pickup. The price is that a
    /// mistyped id reads as zero rather than as an error — which is why
    /// `tests/data.rs` cross-references every id content names, and why the
    /// tape that matters asserts the count going *up*.
    fn item(&self, id: &str) -> std::borrow::Cow<'_, ItemProbe> {
        match self.items.iter().find(|item| item.id == id) {
            Some(item) => std::borrow::Cow::Borrowed(item),
            None => std::borrow::Cow::Owned(ItemProbe::absent(id)),
        }
    }
}

/// A parsed assertion path.
pub enum ProbePath<'a> {
    Player(&'a str),
    Npc {
        kind: &'a str,
        index: usize,
        name: &'a str,
    },
    /// `item.minor_potion.count`. An item is addressed by id rather than by
    /// index, because unlike an NPC nothing about an item's position in a list
    /// is stable — a stack is removed the moment it empties.
    Item {
        id: &'a str,
        name: &'a str,
    },
}

impl<'a> ProbePath<'a> {
    pub fn parse(path: &'a str) -> Option<ProbePath<'a>> {
        let parts: Vec<&str> = path.split('.').collect();
        match parts.as_slice() {
            [name] => Some(ProbePath::Player(name)),
            ["player", name] => Some(ProbePath::Player(name)),
            // Before the NPC arm, which is the same shape. Unambiguous because
            // the NPC form's middle segment is a number and this one's is an
            // id — but written first so that is a fact about this file rather
            // than about `usize::from_str`.
            ["item", id, name] => Some(ProbePath::Item { id, name }),
            [kind, index, name] => Some(ProbePath::Npc {
                kind,
                index: index.parse().ok()?,
                name,
            }),
            _ => None,
        }
    }

    /// Whether this path names a numeric field, without needing a frame to
    /// resolve against — used to reject typos when a tape is parsed rather
    /// than when it runs.
    pub fn is_field(&self) -> bool {
        match self {
            ProbePath::Player(name) => Probe::field_names().contains(name),
            ProbePath::Npc { name, .. } => NpcProbe::field_names().contains(name),
            ProbePath::Item { name, .. } => ItemProbe::field_names().contains(name),
        }
    }

    pub fn is_flag(&self) -> bool {
        match self {
            ProbePath::Player(name) => Probe::flag_names().contains(name),
            ProbePath::Npc { name, .. } => NpcProbe::flag_names().contains(name),
            ProbePath::Item { name, .. } => ItemProbe::flag_names().contains(name),
        }
    }

    /// Whether this path names a string field — a clip name, a kind.
    pub fn is_text(&self) -> bool {
        match self {
            ProbePath::Player(name) => Probe::text_names().contains(name),
            ProbePath::Npc { name, .. } => NpcProbe::text_names().contains(name),
            ProbePath::Item { .. } => false,
        }
    }

    /// What this path could have meant, for an error message.
    pub fn known_names(&self) -> String {
        match self {
            ProbePath::Player(_) => Probe::known_names(),
            ProbePath::Npc { .. } => NpcProbe::known_names(),
            ProbePath::Item { .. } => ItemProbe::known_names(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Trace {
    frames: Vec<Frame>,
}

impl Trace {
    pub fn new() -> Self {
        Trace::default()
    }

    pub fn push(
        &mut self,
        probe: Probe,
        npcs: Vec<NpcProbe>,
        items: Vec<ItemProbe>,
        events: &[GameEvent],
    ) {
        self.push_frame(Frame {
            probe,
            npcs,
            items,
            events: events.to_vec(),
            seed: None,
        });
    }

    pub fn push_frame(&mut self, frame: Frame) {
        self.frames.push(frame);
    }

    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn last(&self) -> Option<&Frame> {
        self.frames.last()
    }

    /// Serialize to JSONL. A path of `-` writes to stdout.
    pub fn write_jsonl(&self, path: &Path) -> anyhow::Result<()> {
        if path == Path::new("-") {
            let stdout = std::io::stdout();
            return self.write_to(&mut stdout.lock());
        }
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let file = File::create(path)
            .with_context(|| format!("failed to create trace {}", path.display()))?;
        let mut writer = BufWriter::new(file);
        self.write_to(&mut writer)
            .with_context(|| format!("failed to write trace {}", path.display()))
    }

    /// Read back a trace written by [`Trace::write_jsonl`], for comparison
    /// against a recorded baseline.
    pub fn read_jsonl(path: &Path) -> anyhow::Result<Trace> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read trace {}", path.display()))?;
        Trace::parse(&text).with_context(|| format!("invalid trace {}", path.display()))
    }

    pub fn parse(text: &str) -> anyhow::Result<Trace> {
        let mut frames = Vec::new();
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let frame: Frame =
                serde_json::from_str(line).with_context(|| format!("line {}", index + 1))?;
            frames.push(frame);
        }
        Ok(Trace { frames })
    }

    /// The trace as it would be written, for diffing without a file.
    pub fn to_jsonl(&self) -> String {
        let mut out = Vec::new();
        self.write_to(&mut out)
            .expect("writing to a Vec cannot fail");
        String::from_utf8(out).expect("serde_json emits UTF-8")
    }

    fn write_to(&self, out: &mut impl Write) -> anyhow::Result<()> {
        for frame in &self.frames {
            serde_json::to_writer(&mut *out, frame)?;
            out.write_all(b"\n")?;
        }
        out.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::StatTable;
    use crate::ecs::components::{AnimationState, Attacking, Avatar, Body, Health};
    use crate::sim::event::DeathCause;
    use ggez::glam::Vec2;

    fn probe(tick: u64) -> Probe {
        let stats = StatTable::shipped().get("player").unwrap();
        Probe::new(
            tick,
            crate::sim::Mode::Playing,
            &Avatar::new(stats.avatar()),
            &Body::new(Vec2::ZERO, stats.gravity, stats.max_fall),
            &Health::new(stats.max_health, stats.iframe_ticks),
            &Attacking::default(),
            Vec2::new(1.5, 2.5),
            Vec2::ZERO,
            &AnimationState::new("idle"),
            None,
            None,
            0,
            0,
            0,
            &crate::systems::inventory::Screen::default(),
        )
    }

    #[test]
    fn writes_one_json_object_per_tick() {
        let mut trace = Trace::new();
        trace.push(probe(0), Vec::new(), Vec::new(), &[]);
        trace.push(probe(1), Vec::new(), Vec::new(), &[]);

        let text = trace.to_jsonl();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in lines {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(value["x"], 1.5);
        }
    }

    /// Probe fields sit at the top level of the line, not nested under a
    /// `probe` key — a trace has to stay greppable and column-diffable.
    #[test]
    fn events_ride_alongside_flattened_probe_fields() {
        let mut trace = Trace::new();
        trace.push(
            probe(3),
            Vec::new(),
            Vec::new(),
            &[GameEvent::Died {
                who: "player".to_string(),
                cause: DeathCause::Hazard,
            }],
        );

        let line = trace.to_jsonl();
        let value: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(value["tick"], 3);
        assert_eq!(value["events"][0]["event"], "died");
        assert_eq!(value["events"][0]["cause"], "hazard");
    }

    /// An eventless frame must serialize exactly as the probe alone, so that
    /// adding the events field did not churn every baseline.
    #[test]
    fn eventless_frames_carry_no_events_key() {
        let mut trace = Trace::new();
        trace.push(probe(0), Vec::new(), Vec::new(), &[]);
        assert!(!trace.to_jsonl().contains("events"));
    }

    /// Round-tripping keeps the trace comparable to a recorded baseline.
    #[test]
    fn jsonl_round_trips() {
        let mut trace = Trace::new();
        trace.push(probe(7), Vec::new(), Vec::new(), &[GameEvent::Jumped]);
        trace.push(probe(8), Vec::new(), Vec::new(), &[]);

        let parsed = Trace::parse(&trace.to_jsonl()).unwrap();
        assert_eq!(parsed.frames(), trace.frames());
        assert_eq!(parsed.to_jsonl(), trace.to_jsonl());
    }

    fn npc(kind: &str, x: f32, dir: f32) -> NpcProbe {
        NpcProbe {
            kind: kind.to_string(),
            x,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
            dir,
            grounded: true,
            hp: 3,
            hitstun: 0,
            dead: false,
            clip: "run".to_string(),
            frame: 0,
        }
    }

    fn frame_with_npcs() -> Frame {
        Frame {
            probe: probe(0),
            npcs: vec![npc("knight", 10.0, 1.0), npc("knight", 20.0, -1.0)],
            items: vec![
                ItemProbe {
                    id: "minor_potion".to_string(),
                    count: 2,
                    equipped: false,
                },
                ItemProbe {
                    id: "knight_helm".to_string(),
                    count: 0,
                    equipped: true,
                },
            ],
            events: Vec::new(),
            seed: None,
        }
    }

    /// The default seed is the absence of a seed, so adding the field did not
    /// rewrite a single baseline.
    #[test]
    fn a_default_seeded_run_carries_no_seed_key() {
        let mut trace = Trace::new();
        trace.push(probe(0), Vec::new(), Vec::new(), &[]);
        assert!(!trace.to_jsonl().contains("seed"));
    }

    #[test]
    fn a_recorded_seed_survives_a_round_trip() {
        let mut trace = Trace::new();
        trace.push_frame(Frame {
            probe: probe(0),
            npcs: Vec::new(),
            items: Vec::new(),
            events: Vec::new(),
            seed: Some(4242),
        });
        let line = trace.to_jsonl();
        let value: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(value["seed"], 4242);
        assert_eq!(Trace::parse(&line).unwrap().frames(), trace.frames());
    }

    /// A bare name has always meant the player and must keep meaning it, or
    /// every tape written before NPCs existed would change meaning silently.
    #[test]
    fn bare_and_player_prefixed_paths_both_resolve_to_the_player() {
        let frame = frame_with_npcs();
        assert_eq!(frame.field("x"), Some(1.5));
        assert_eq!(frame.field("player.x"), Some(1.5));
        assert_eq!(frame.flag("grounded"), frame.flag("player.grounded"));
    }

    #[test]
    fn npcs_resolve_by_kind_and_spawn_index() {
        let frame = frame_with_npcs();
        assert_eq!(frame.field("knight.0.x"), Some(10.0));
        assert_eq!(frame.field("knight.1.x"), Some(20.0));
        assert_eq!(frame.flag("knight.0.facing_right"), Some(true));
        assert_eq!(frame.flag("knight.1.facing_right"), Some(false));
    }

    /// The bag is addressed by id, and an item nobody has reads as zero — so a
    /// tape can assert both sides of a pickup rather than only the one after
    /// it.
    #[test]
    fn items_resolve_by_id_and_absence_reads_as_zero() {
        let frame = frame_with_npcs();
        assert_eq!(frame.field("item.minor_potion.count"), Some(2.0));
        assert_eq!(frame.flag("item.minor_potion.equipped"), Some(false));

        assert_eq!(
            frame.field("item.knight_helm.count"),
            Some(0.0),
            "worn, so no longer in the bag"
        );
        assert_eq!(frame.flag("item.knight_helm.equipped"), Some(true));

        assert_eq!(frame.field("item.nothing_like_it.count"), Some(0.0));
        assert_eq!(frame.flag("item.nothing_like_it.equipped"), Some(false));
    }

    /// `item.<id>.<field>` and `<kind>.<index>.<field>` are the same shape, and
    /// an item id is not an index. The item arm wins.
    #[test]
    fn an_item_path_is_not_mistaken_for_an_npc_one() {
        assert!(matches!(
            ProbePath::parse("item.minor_potion.count"),
            Some(ProbePath::Item { .. })
        ));
        assert!(matches!(
            ProbePath::parse("knight.0.hp"),
            Some(ProbePath::Npc { .. })
        ));
        assert!(ProbePath::parse("item.minor_potion.count")
            .unwrap()
            .is_field());
        assert!(ProbePath::parse("item.minor_potion.equipped")
            .unwrap()
            .is_flag());
        assert!(!ProbePath::parse("item.minor_potion.hp").unwrap().is_field());
    }

    #[test]
    fn paths_that_cannot_resolve_return_none_rather_than_a_wrong_answer() {
        let frame = frame_with_npcs();
        assert_eq!(frame.field("knight.2.x"), None, "only two knights");
        assert_eq!(frame.field("goblin.0.x"), None, "no goblins on this map");
        assert_eq!(frame.field("knight.0.nonsense"), None);
        assert_eq!(frame.field("a.b.c.d"), None, "too many segments");
        assert_eq!(frame.field("knight.left.x"), None, "index is not a number");
    }

    /// Typos are rejected when a tape is parsed, not when it runs, so the
    /// error names the line instead of appearing 300 ticks in.
    #[test]
    fn path_validity_is_decidable_without_a_frame() {
        assert!(ProbePath::parse("knight.0.x").unwrap().is_field());
        assert!(ProbePath::parse("knight.0.grounded").unwrap().is_flag());
        assert!(
            !ProbePath::parse("knight.0.air_jumps").unwrap().is_field(),
            "air_jumps is a player field; an NPC has no jump budget"
        );
        assert!(ProbePath::parse("x").unwrap().is_field());
        assert!(ProbePath::parse("a.b.c.d").is_none());
    }

    #[test]
    fn a_corrupt_line_is_reported_with_its_number() {
        let mut good = Trace::new();
        good.push(probe(0), Vec::new(), Vec::new(), &[]);

        let text = format!("{}not json\n", good.to_jsonl());
        let err = Trace::parse(&text).unwrap_err();
        assert!(format!("{err:#}").contains("line 2"), "{err:#}");
    }
}
