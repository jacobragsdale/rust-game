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
//! cargo run --bin sim -- --tape crypt_to_hall.tape --fight warden.0
//! ```
//!
//! `--fight` is for writing a tape through a fight: after the tape, a simple
//! closed-loop player — close in, swing, back out of the enemy's swing, jump
//! its bolts — plays against that NPC until it dies, and the inputs it used
//! are printed as tape lines to append. The sim is deterministic, so the lines
//! replay exactly; when a balance change breaks the fight, record it again.
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
  --fight <npc>    after the tape, fight this NPC (`warden.0`) and print the
                   inputs that won as tape lines on stdout
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
    fight: Option<String>,
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
            fight: None,
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
            "--fight" => args.fight = Some(value()?),
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
    // A tape names its own map, and `tests/tapes.rs` refuses one that does
    // not — so this does too, rather than quietly playing it on the castle.
    if let Some(tape) = &tape {
        if tape.map.is_none() && args.map.is_none() {
            bail!("the tape has no `map` line; add one, or pass --map");
        }
    }
    let map = args
        .map
        .clone()
        .or_else(|| tape.as_ref().and_then(|t| t.map.clone()))
        .unwrap_or_else(|| DEFAULT_MAP.to_string());

    // `--trace -` makes stdout the trace, so it has to be nothing but JSONL:
    // everything written for a person goes to stderr instead.
    // So does `--fight`, whose stdout is tape lines.
    let to_stderr = args.trace.as_deref() == Some(Path::new("-")) || args.fight.is_some();
    macro_rules! say {
        ($($arg:tt)*) => {
            if to_stderr {
                eprintln!($($arg)*)
            } else {
                println!($($arg)*)
            }
        };
    }

    let mut assets = Assets::new();
    let mut sim =
        Sim::load(&mut assets, &map).with_context(|| format!("failed to load map `{map}`"))?;

    if !args.quiet {
        say!(
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
                say!(
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
            say!(
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
        // Platforms and swings: where they go, not just where they start, since
        // a tape that rides or dodges one is written against the whole path.
        for mover in &sim.level.movers {
            say!(
                "platform x {:>7.1} .. {:>7.1}   y {:>7.1} .. {:>7.1}   \
                 to ({:.1}, {:.1}), {:.0} px/s, phase {}{}",
                mover.from.x,
                mover.from.x + mover.size.x,
                mover.from.y,
                mover.from.y + mover.size.y,
                mover.to.x,
                mover.to.y,
                mover.speed,
                mover.phase,
                if mover.one_way { ", one-way" } else { "" },
            );
        }
        for swing in &sim.level.pendulums {
            say!(
                "swing    anchor ({:.1}, {:.1})   chain {:.0}px, +/-{:.0} deg, \
                 {} ticks a swing, phase {}, ball r {:.0}",
                swing.anchor.x,
                swing.anchor.y,
                swing.length,
                swing.amplitude.to_degrees(),
                swing.period,
                swing.phase,
                swing.radius,
            );
        }
        // NPCs by the index a tape addresses them with.
        let mut counts: std::collections::HashMap<String, usize> = Default::default();
        for (entity, probe) in sim.npcs().into_iter().zip(sim.npc_probes()) {
            let index = counts.entry(probe.kind.clone()).or_insert(0);
            let size = sim
                .world
                .get::<&supergame::ecs::components::Size>(entity)
                .map(|s| s.0)
                .unwrap_or_default();
            say!(
                "npc      {}.{:<3}   x {:>7.1} .. {:>7.1}   y {:>7.1} .. {:>7.1}",
                probe.kind,
                index,
                probe.x,
                probe.x + size.x,
                probe.y,
                probe.y + size.y,
            );
            *index += 1;
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
                say!(
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
            say!("trace {} — {} ticks", path.display(), trace.len());
        }
    }

    if !args.quiet {
        // Events are transient, so a run that only prints final state hides
        // everything that happened on the way there.
        let total: usize = trace.frames().iter().map(|f| f.events.len()).sum();
        if total > 0 {
            say!("events — {total} total:");
            for frame in trace.frames().iter().filter(|f| !f.events.is_empty()) {
                for event in &frame.events {
                    say!("  tick {:>5}  {event:?}", frame.probe.tick);
                }
            }
        }
        if let Some(last) = trace.last() {
            say!("final {}", last.probe.summary());
        }
    }

    if let Some(target) = &args.fight {
        return fight(&mut sim, target);
    }

    if failures.is_empty() {
        if !args.quiet && args.tape.is_some() {
            say!("PASS");
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

/// Play the fight against `target` (`kind.index`) closed-loop, and print the
/// inputs that won it as tape lines.
///
/// The player is flown by the same brain a rival champion is
/// (`supergame::systems::brain`), deciding every tick, taking to the air — to
/// reach a flyer, and to plunge onto a shield — and throwing the player's
/// spell from middle range: a way to write a tape through a fight, not an
/// opponent. Every distance it
/// keeps comes from `assets/data/attacks.ron`: it swings from well inside the
/// player's opener and backs out past the reach of the enemy's swing until
/// that swing's hitbox has gone. Presses are edges in a tape, so it releases a
/// key for a tick before pressing it again.
fn fight(sim: &mut Sim, target: &str) -> anyhow::Result<bool> {
    use supergame::assets::BrainStats;
    use supergame::ecs::components::{Brain, Health};
    use supergame::systems::brain;
    use supergame::systems::input::{Action, ActionSet};

    let (kind, index) = target
        .split_once('.')
        .and_then(|(kind, index)| Some((kind, index.parse::<usize>().ok()?)))
        .with_context(|| format!("`{target}` is not an NPC — write it `kind.index`"))?;
    let enemy = sim
        .npcs()
        .into_iter()
        .zip(sim.npc_probes())
        .filter(|(_, probe)| probe.kind == kind)
        .nth(index)
        .map(|(entity, _)| entity)
        .with_context(|| format!("the map has no `{target}`"))?;
    let player = supergame::systems::avatar::player(&sim.world).context("the map has no player")?;
    let knobs = BrainStats {
        sight: f32::INFINITY,
        reaction: 1,
        cast_range: 160.0,
        aerial: true,
    };
    let mut mind = Brain::default();

    let mut lines: Vec<(ActionSet, u32)> = Vec::new();
    let mut deaths = 0;
    let limit = 6000;
    for _ in 0..limit {
        if sim.world.get::<&Health>(enemy)?.dead() {
            break;
        }
        let input = brain::step(
            &mut mind,
            &sim.world,
            player,
            Some(enemy),
            &knobs,
            &sim.attacks,
            &sim.spells,
        );
        let held = input.held_set();
        sim.step(input);
        deaths += sim
            .events()
            .iter()
            .filter(|e| matches!(e, supergame::sim::GameEvent::Died { who, .. } if who == "player"))
            .count();
        match lines.last_mut() {
            Some((set, n)) if *set == held => *n += 1,
            _ => lines.push((held, 1)),
        }
    }

    for (set, n) in &lines {
        let names: Vec<&str> = set.iter().map(Action::name).collect();
        let keys = if names.is_empty() {
            "wait".to_string()
        } else {
            names.join("+")
        };
        println!("{keys} {n}");
    }
    let won = sim.world.get::<&Health>(enemy)?.dead();
    eprintln!(
        "fight {target}: {} at tick {}, player hp {}, {deaths} death(s), {} lines",
        if won { "won" } else { "not won" },
        sim.tick,
        sim.probe().hp,
        lines.len(),
    );
    Ok(won)
}
