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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<GameEvent>,
}

#[derive(Clone, Debug, Default)]
pub struct Trace {
    frames: Vec<Frame>,
}

impl Trace {
    pub fn new() -> Self {
        Trace::default()
    }

    pub fn push(&mut self, probe: Probe, events: &[GameEvent]) {
        self.frames.push(Frame {
            probe,
            events: events.to_vec(),
        });
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
            let frame: Frame = serde_json::from_str(line)
                .with_context(|| format!("line {}", index + 1))?;
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
    use crate::ecs::components::{AnimationState, Avatar};
    use crate::sim::event::DeathCause;
    use ggez::glam::Vec2;

    fn probe(tick: u64) -> Probe {
        Probe::new(
            tick,
            &Avatar::new(),
            &Avatar::body(Vec2::ZERO),
            Vec2::new(1.5, 2.5),
            Vec2::ZERO,
            &AnimationState::new("idle"),
        )
    }

    #[test]
    fn writes_one_json_object_per_tick() {
        let mut trace = Trace::new();
        trace.push(probe(0), &[]);
        trace.push(probe(1), &[]);

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
            &[GameEvent::Died {
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
        trace.push(probe(0), &[]);
        assert!(!trace.to_jsonl().contains("events"));
    }

    /// Round-tripping keeps the trace comparable to a recorded baseline.
    #[test]
    fn jsonl_round_trips() {
        let mut trace = Trace::new();
        trace.push(probe(7), &[GameEvent::Jumped]);
        trace.push(probe(8), &[]);

        let parsed = Trace::parse(&trace.to_jsonl()).unwrap();
        assert_eq!(parsed.frames(), trace.frames());
        assert_eq!(parsed.to_jsonl(), trace.to_jsonl());
    }

    #[test]
    fn a_corrupt_line_is_reported_with_its_number() {
        let mut good = Trace::new();
        good.push(probe(0), &[]);

        let text = format!("{}not json\n", good.to_jsonl());
        let err = Trace::parse(&text).unwrap_err();
        assert!(format!("{err:#}").contains("line 2"), "{err:#}");
    }
}
