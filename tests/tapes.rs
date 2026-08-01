//! Every input tape in `tapes/` is a regression test.
//!
//! Adding coverage for a movement feature means writing a tape, not writing
//! Rust — which is the point. `cargo test` then replays all of them against
//! the same simulation the game runs.

use std::path::{Path, PathBuf};

use supergame::assets::Assets;
use supergame::sim::run::run_tape;
use supergame::sim::tape::Tape;
use supergame::sim::Sim;

fn tapes_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tapes")
}

fn tape_paths() -> Vec<PathBuf> {
    let dir = tapes_dir();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "tape"))
        .collect();
    paths.sort();
    paths
}

#[test]
fn every_tape_passes() {
    let paths = tape_paths();
    assert!(
        !paths.is_empty(),
        "no tapes found in {}",
        tapes_dir().display()
    );

    let mut failures: Vec<String> = Vec::new();

    for path in &paths {
        let name = path.file_name().unwrap_or_default().to_string_lossy();

        let tape = match Tape::load(path) {
            Ok(tape) => tape,
            Err(err) => {
                failures.push(format!("{name}: {err:#}"));
                continue;
            }
        };

        // Enforced rather than defaulted: a tape that does not say which map
        // it belongs to would silently start asserting against a different
        // level the moment the default changed.
        let Some(map) = tape.map.clone() else {
            failures.push(format!("{name}: missing a `map` directive"));
            continue;
        };

        let mut sim = match Sim::load(&mut Assets::new(), &map) {
            Ok(sim) => sim,
            Err(err) => {
                failures.push(format!("{name}: failed to load map `{map}`: {err:#}"));
                continue;
            }
        };

        assert!(
            !tape.asserts.is_empty(),
            "{name}: a tape with no assertions verifies nothing"
        );

        for failure in run_tape(&mut sim, &tape).failures {
            failures.push(format!("{name}: {failure}"));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} tapes failed:\n  {}",
        failures.len(),
        paths.len(),
        failures.join("\n  ")
    );
}
