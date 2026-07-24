//! Every tape has a recorded trace, and it must still produce it.
//!
//! A tape's own assertions check a handful of values at a handful of ticks —
//! whatever the author thought to write down. A trace is every field of the
//! probe plus every event, on every tick. So this catches the class of change
//! a tape assertion structurally cannot: a refactor that leaves `x > 384` true
//! but moves the player two pixels, shifts the landing by a tick, or drops an
//! event nobody was asserting on.
//!
//! That is exactly the risk profile of the work ahead — pulling movement apart
//! into shared systems is supposed to change the code and not the game — which
//! is what these baselines are for.
//!
//! ```bash
//! cargo test --test traces              # check against the baselines
//! UPDATE_TRACES=1 cargo test --test traces   # re-record them
//! ```
//!
//! Re-recording is a deliberate act: the diff lands in the commit, and a
//! reviewer (or a later you) sees precisely which ticks moved and can decide
//! whether that was the point of the change.

use std::path::{Path, PathBuf};

use supergame::assets::Assets;
use supergame::sim::run::run_tape;
use supergame::sim::tape::Tape;
use supergame::sim::trace::Trace;
use supergame::sim::Sim;

fn repo_path(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(sub)
}

fn tape_paths() -> Vec<PathBuf> {
    let dir = repo_path("tapes");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "tape"))
        .collect();
    paths.sort();
    paths
}

fn rerecording() -> bool {
    std::env::var("UPDATE_TRACES").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Replay a tape and return the trace it produces.
fn replay(tape_path: &Path) -> anyhow::Result<Trace> {
    let tape = Tape::load(tape_path)?;
    let map = tape
        .map
        .clone()
        .ok_or_else(|| anyhow::anyhow!("tape has no `map` directive"))?;
    let mut sim = Sim::load(&mut Assets::new(), &map)?;
    Ok(run_tape(&mut sim, &tape).trace)
}

/// Where the first difference is, and what it is. A raw "these two 400-line
/// files differ" is not actionable; the tick and the field names are.
fn describe_difference(expected: &str, actual: &str) -> String {
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();

    let first_diff = expected_lines
        .iter()
        .zip(&actual_lines)
        .position(|(e, a)| e != a);

    let Some(index) = first_diff else {
        return format!(
            "trace length changed: baseline has {} ticks, this run produced {}",
            expected_lines.len(),
            actual_lines.len()
        );
    };

    let mut report = format!("first difference at trace line {}", index + 1);
    if expected_lines.len() != actual_lines.len() {
        report.push_str(&format!(
            " (and the length changed: {} -> {})",
            expected_lines.len(),
            actual_lines.len()
        ));
    }

    // Field-level detail where both lines parse; fall back to the raw text.
    let parsed = |line: &str| serde_json::from_str::<serde_json::Value>(line).ok();
    match (parsed(expected_lines[index]), parsed(actual_lines[index])) {
        (Some(serde_json::Value::Object(e)), Some(serde_json::Value::Object(a))) => {
            let mut keys: Vec<&String> = e.keys().chain(a.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                let (was, now) = (e.get(key), a.get(key));
                if was != now {
                    report.push_str(&format!(
                        "\n      {key}: {} -> {}",
                        was.map_or("(absent)".to_string(), |v| v.to_string()),
                        now.map_or("(absent)".to_string(), |v| v.to_string()),
                    ));
                }
            }
        }
        _ => {
            report.push_str(&format!(
                "\n      baseline: {}\n      this run: {}",
                expected_lines[index], actual_lines[index]
            ));
        }
    }
    report
}

#[test]
fn every_tape_reproduces_its_recorded_trace() {
    let tapes = tape_paths();
    assert!(!tapes.is_empty(), "no tapes to trace");

    let mut problems: Vec<String> = Vec::new();
    let mut rerecorded: Vec<String> = Vec::new();

    for tape_path in &tapes {
        let stem = tape_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let baseline_path = repo_path("traces").join(format!("{stem}.jsonl"));

        let trace = match replay(tape_path) {
            Ok(trace) => trace,
            Err(err) => {
                problems.push(format!("{stem}: failed to replay: {err:#}"));
                continue;
            }
        };
        let actual = trace.to_jsonl();

        if rerecording() {
            match trace.write_jsonl(&baseline_path) {
                Ok(()) => rerecorded.push(format!("{stem} ({} ticks)", trace.len())),
                Err(err) => problems.push(format!("{stem}: failed to write baseline: {err:#}")),
            }
            continue;
        }

        let Ok(expected) = std::fs::read_to_string(&baseline_path) else {
            problems.push(format!(
                "{stem}: no baseline at {}\n    record one with: UPDATE_TRACES=1 cargo test --test traces",
                baseline_path.display()
            ));
            continue;
        };

        // The baseline must also still parse as a trace: a hand-edited or
        // truncated file would otherwise "pass" by matching nothing.
        if let Err(err) = Trace::read_jsonl(&baseline_path) {
            problems.push(format!("{stem}: baseline is not a readable trace: {err:#}"));
            continue;
        }

        if expected != actual {
            problems.push(format!(
                "{stem}: behavior changed\n    {}\n    If this change was intended: UPDATE_TRACES=1 cargo test --test traces",
                describe_difference(&expected, &actual)
            ));
        }
    }

    if !rerecorded.is_empty() {
        println!("re-recorded {} trace(s):", rerecorded.len());
        for name in &rerecorded {
            println!("  {name}");
        }
    }

    assert!(
        problems.is_empty(),
        "{} of {} traces did not match:\n\n  {}\n",
        problems.len(),
        tapes.len(),
        problems.join("\n\n  ")
    );
}

/// A baseline with no trace file is a silent hole in the coverage, and a
/// trace file with no tape is dead weight left behind by a rename.
#[test]
fn baselines_and_tapes_are_in_one_to_one_correspondence() {
    if rerecording() {
        return; // the other test is mid-write
    }

    let dir = repo_path("traces");
    let baselines: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();

    let tapes: Vec<String> = tape_paths()
        .iter()
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();

    let orphaned: Vec<&String> = baselines.iter().filter(|b| !tapes.contains(b)).collect();
    assert!(
        orphaned.is_empty(),
        "trace baselines with no matching tape (delete them): {orphaned:?}"
    );
}
