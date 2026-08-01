//! Headless simulation runner.
//!
//! Plays an input tape through the game's real simulation with no window and
//! no GPU, checks the tape's assertions, and optionally writes a tick-by-tick
//! JSONL trace. Exits non-zero if any assertion fails, so it works directly
//! as a test command.
//!
//! ```text
//! cargo run --bin sim -- --tape tapes/wall_jump.tape
//! cargo run --bin sim -- --tape tapes/run.tape --trace out.jsonl
//! cargo run --bin sim -- --ticks 120 --trace -        # idle, trace to stdout
//! ```
//!
//! Because the sim is deterministic, `diff` over two traces of the same tape
//! pinpoints the exact tick a change altered behavior.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context as _};

use supergame::assets::Assets;
use supergame::sim::run::{run_idle, run_tape};
use supergame::sim::tape::Tape;
use supergame::sim::Sim;

const USAGE: &str = "\
usage: sim [options]

  --map <path>     map to load, relative to assets/
                   (default: the tape's `map` directive, else maps/castle.ron)
  --tape <path>    input tape to play
  --trace <path>   write a JSONL trace ('-' for stdout)
  --ticks <n>      run this many idle ticks (ignored when --tape is given)
  --geometry       print the level's collision rectangles and exit
  --quiet          suppress the summary
  -h, --help       show this help
";

/// Fallback when neither the command line nor the tape names a map.
const DEFAULT_MAP: &str = "maps/castle.ron";

struct Args {
    /// `None` means "whatever the tape asks for, else the default".
    map: Option<String>,
    tape: Option<PathBuf>,
    trace: Option<PathBuf>,
    ticks: usize,
    geometry: bool,
    quiet: bool,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            map: None,
            tape: None,
            trace: None,
            ticks: 60,
            geometry: false,
            quiet: false,
        }
    }
}

fn parse_args() -> anyhow::Result<Option<Args>> {
    let mut args = Args::default();
    let mut argv = std::env::args().skip(1);

    while let Some(flag) = argv.next() {
        let mut value = || -> anyhow::Result<String> {
            argv.next().with_context(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--map" => args.map = Some(value()?),
            "--tape" => args.tape = Some(PathBuf::from(value()?)),
            "--trace" => args.trace = Some(PathBuf::from(value()?)),
            "--ticks" => {
                let raw = value()?;
                args.ticks = raw
                    .parse()
                    .with_context(|| format!("`{raw}` is not a tick count"))?;
            }
            "--geometry" => args.geometry = true,
            "--quiet" | "-q" => args.quiet = true,
            "-h" | "--help" => return Ok(None),
            other => bail!("unknown argument `{other}`\n\n{USAGE}"),
        }
    }
    Ok(Some(args))
}

fn main() -> ExitCode {
    match run() {
        Ok(passed) => {
            if passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(err) => {
            eprintln!("sim: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<bool> {
    let Some(args) = parse_args()? else {
        print!("{USAGE}");
        return Ok(true);
    };

    // The tape is loaded first because it may name the map it belongs to.
    let tape = match &args.tape {
        Some(path) => Some(Tape::load(path)?),
        None => None,
    };
    let map = args
        .map
        .clone()
        .or_else(|| tape.as_ref().and_then(|t| t.map.clone()))
        .unwrap_or_else(|| DEFAULT_MAP.to_string());

    let mut assets = Assets::new();
    let mut sim =
        Sim::load(&mut assets, &map).with_context(|| format!("failed to load map `{map}`"))?;

    if !args.quiet {
        println!(
            "map {} — {}x{} tiles, {} solids, {} one-way, {} hazards, spawn ({:.0}, {:.0})",
            map,
            sim.level.width,
            sim.level.height,
            sim.level.solids.len(),
            sim.level.one_way.len(),
            sim.level.hazards.len(),
            sim.level.player_spawn.x,
            sim.level.player_spawn.y,
        );
    }

    if args.geometry {
        // Authoring aid: what the ASCII grid actually became, so a tape can
        // be written against real coordinates instead of guessed ones.
        let groups = [
            ("solid", &sim.level.solids),
            ("one-way", &sim.level.one_way),
            ("hazard", &sim.level.hazards),
        ];
        for (label, rects) in groups {
            for rect in rects.iter() {
                println!(
                    "{label:<8} x {:>7.1} .. {:>7.1}   y {:>7.1} .. {:>7.1}",
                    rect.x,
                    rect.right(),
                    rect.y,
                    rect.bottom()
                );
            }
        }
        // Fires are geometry an entity owns rather than level rects, and their
        // timing is half of what a tape has to be written against. Sorted by
        // entity id, which is map order: a lit fire and an unlit one sit in
        // different hecs archetypes, so query order is whatever the schedule
        // happened to say at tick 0.
        let mut fires: Vec<_> = sim
            .world
            .query::<(
                &supergame::ecs::components::Position,
                &supergame::ecs::components::Fire,
                &supergame::ecs::components::Schedule,
            )>()
            .iter()
            .map(|(entity, (pos, fire, schedule))| {
                (entity.id(), fire.collider.aabb(pos.0), *schedule)
            })
            .collect();
        fires.sort_by_key(|(id, _, _)| *id);
        for (_, rect, schedule) in fires {
            println!(
                "fire     x {:>7.1} .. {:>7.1}   y {:>7.1} .. {:>7.1}   \
                 lit {} of every {} ticks, phase {}",
                rect.x,
                rect.right(),
                rect.y,
                rect.bottom(),
                schedule.duty,
                schedule.period,
                schedule.phase,
            );
        }
        return Ok(true);
    }

    // Geometry the player cannot use is reported on every run, tape or not:
    // it is silent in-game and looks like a physics bug when you hit it.
    let player = sim.stats.get("player")?;
    let issues = sim.level.blocked_platforms(player.size());
    if !issues.is_empty() {
        eprintln!("level warnings:");
        for issue in &issues {
            eprintln!("  {}", issue.describe());
        }
    }

    let (trace, failures) = match &tape {
        Some(tape) => {
            if !args.quiet {
                println!(
                    "tape {} — {} ticks, {} assertions, seed {}",
                    args.tape.as_ref().expect("tape implies a path").display(),
                    tape.ticks(),
                    tape.asserts.len(),
                    tape.seed(),
                );
            }
            let outcome = run_tape(&mut sim, tape);
            (outcome.trace, outcome.failures)
        }
        None => (run_idle(&mut sim, args.ticks), Vec::new()),
    };

    if let Some(path) = &args.trace {
        trace.write_jsonl(path)?;
        if !args.quiet && path != Path::new("-") {
            println!("trace {} — {} ticks", path.display(), trace.len());
        }
    }

    if !args.quiet {
        // Events are transient, so a run that only prints final state hides
        // everything that happened on the way there.
        let total: usize = trace.frames().iter().map(|f| f.events.len()).sum();
        if total > 0 {
            println!("events — {total} total:");
            for frame in trace.frames().iter().filter(|f| !f.events.is_empty()) {
                for event in &frame.events {
                    println!("  tick {:>5}  {event:?}", frame.probe.tick);
                }
            }
        }
        if let Some(last) = trace.last() {
            println!("final {}", last.probe.summary());
        }
    }

    if failures.is_empty() {
        if !args.quiet && args.tape.is_some() {
            println!("PASS");
        }
        Ok(true)
    } else {
        eprintln!("\nFAIL — {} assertion(s):", failures.len());
        for failure in &failures {
            eprintln!("  {failure}");
        }
        Ok(false)
    }
}
