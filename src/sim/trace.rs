//! Tick-by-tick trace recording, written as JSONL.
//!
//! One [`Probe`] per line. The format is deliberately line-oriented because
//! the sim is deterministic: running the same tape before and after a change
//! and then running `diff` over the two traces reports the exact tick at
//! which behavior changed. That makes "my refactor subtly altered the jump
//! arc" a mechanically detectable regression rather than something you only
//! notice while playing.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::Context as _;

use crate::sim::Probe;

#[derive(Clone, Debug, Default)]
pub struct Trace {
    probes: Vec<Probe>,
}

impl Trace {
    pub fn new() -> Self {
        Trace::default()
    }

    pub fn push(&mut self, probe: Probe) {
        self.probes.push(probe);
    }

    pub fn probes(&self) -> &[Probe] {
        &self.probes
    }

    pub fn len(&self) -> usize {
        self.probes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.probes.is_empty()
    }

    pub fn last(&self) -> Option<&Probe> {
        self.probes.last()
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

    fn write_to(&self, out: &mut impl Write) -> anyhow::Result<()> {
        for probe in &self.probes {
            serde_json::to_writer(&mut *out, probe)?;
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
    use ggez::glam::Vec2;

    fn probe(tick: u64) -> Probe {
        Probe::new(
            tick,
            &Avatar::new(Vec2::ZERO),
            Vec2::new(1.5, 2.5),
            Vec2::ZERO,
            &AnimationState::new("idle"),
        )
    }

    #[test]
    fn writes_one_json_object_per_tick() {
        let mut trace = Trace::new();
        trace.push(probe(0));
        trace.push(probe(1));

        let mut out = Vec::new();
        trace.write_to(&mut out).unwrap();
        let text = String::from_utf8(out).unwrap();

        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in lines {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(value["x"], 1.5);
        }
    }

    /// Round-tripping keeps the trace comparable to a recorded baseline.
    #[test]
    fn jsonl_round_trips() {
        let original = probe(7);
        let json = serde_json::to_string(&original).unwrap();
        let parsed: Probe = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }
}
