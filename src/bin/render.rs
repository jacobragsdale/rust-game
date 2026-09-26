//! Render the game to PNG, headlessly: any tick of any tape, or a whole map.
//!
//! The simulation has been checkable without a window for as long as there
//! have been tapes; this makes the *picture* checkable the same way. It steps
//! the real sim, keeps the real [`View`] in step with it — the same camera
//! following the same player — and draws the frame the game would draw at
//! that tick, through the CPU backend in [`supergame::render::cpu`]. Nothing
//! here is a reconstruction: the draw code is the game's.
//!
//! ```text
//! cargo run --bin render -- --tape tapes/knight_kill.tape --at 120
//! cargo run --bin render -- --tape tapes/dungeon_run.tape --every 120
//! cargo run --bin render -- --map maps/dungeon.ron --level --debug
//! cargo run --bin render -- --map maps/village.ron --pause
//! ```
//!
//! The PNGs land under `target/render/` unless `--out` says otherwise, named
//! after the tape or map and the tick, and are upscaled 2x by default so a
//! person or an agent can read them without zooming.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context as _};

use supergame::assets::Assets;
use supergame::config::Config;
use supergame::render::cpu::CpuRenderer;
use supergame::render::Frame;
use supergame::save::FileStore;
use supergame::scenes::pause::PauseScene;
use supergame::scenes::{Resources, Scene};
use supergame::sim::run::run_tape_with;
use supergame::sim::tape::Tape;
use supergame::sim::Sim;
use supergame::systems::input::InputLatch;
use supergame::view::View;

const USAGE: &str = "\
usage: render [options]

  --tape <path>    play this tape; its `map` line picks the map
  --map <path>     map to load, relative to assets/ (default: the tape's)
  --at <tick>      draw the frame after this many ticks (default: the last)
  --every <n>      draw every n-th tick instead of one frame
  --level          draw the whole level at 1:1, not the camera's view
  --debug          draw the F1 debug overlay: colliders, hazards, arcs
  --pause          draw the pause menu over the frame
  --scale <n>      upscale the output, nearest neighbour (default 2)
  --out <path>     output file, or directory with --every
                   (default target/render/<name>_<tick>.png)
  -h, --help       show this help
";

struct Args {
    tape: Option<PathBuf>,
    map: Option<String>,
    at: Option<usize>,
    every: Option<usize>,
    level: bool,
    debug: bool,
    pause: bool,
    scale: u32,
    out: Option<PathBuf>,
}

fn parse_args() -> anyhow::Result<Option<Args>> {
    let mut args = Args {
        tape: None,
        map: None,
        at: None,
        every: None,
        level: false,
        debug: false,
        pause: false,
        scale: 2,
        out: None,
    };
    let mut argv = std::env::args().skip(1);
    while let Some(flag) = argv.next() {
        let mut value = || argv.next().with_context(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "-h" | "--help" => return Ok(None),
            "--tape" => args.tape = Some(PathBuf::from(value()?)),
            "--map" => args.map = Some(value()?),
            "--at" => args.at = Some(value()?.parse().context("--at takes a tick")?),
            "--every" => {
                let n: usize = value()?.parse().context("--every takes a tick count")?;
                anyhow::ensure!(n > 0, "--every must be at least 1");
                args.every = Some(n);
            }
            "--level" => args.level = true,
            "--debug" => args.debug = true,
            "--pause" => args.pause = true,
            "--scale" => {
                args.scale = value()?.parse().context("--scale takes a number")?;
                anyhow::ensure!(args.scale > 0, "--scale must be at least 1");
            }
            "--out" => args.out = Some(PathBuf::from(value()?)),
            other => bail!("unknown argument `{other}`\n\n{USAGE}"),
        }
    }
    if args.tape.is_none() && args.map.is_none() {
        bail!("give a --tape or a --map\n\n{USAGE}");
    }
    Ok(Some(args))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("render: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    let Some(args) = parse_args()? else {
        print!("{USAGE}");
        return Ok(());
    };

    let tape = args.tape.as_deref().map(Tape::load).transpose()?;
    let map = args
        .map
        .clone()
        .or_else(|| tape.as_ref().and_then(|t| t.map.clone()))
        .context("the tape has no `map` line; pass --map")?;
    let name = match &args.tape {
        Some(path) => stem(path),
        None => stem(Path::new(&map)),
    };

    let mut sim = Sim::load(&mut Assets::new(), &map)?;
    let mut view_assets = Assets::new();
    let mut view = View::new(&sim, &mut view_assets)?;
    view.debug.enabled = args.debug;
    let mut renderer = CpuRenderer::new(Assets::new());
    let resources = resources()?;

    let mut shots: Vec<(usize, Frame)> = Vec::new();
    let mut shoot = |sim: &Sim, view: &View, tick: usize| {
        let frame = if args.level {
            view.draw_level(sim)
        } else {
            let mut frame = Frame::default();
            view.draw(sim, &mut frame);
            if args.pause {
                PauseScene::new(sim.save()).draw(&mut frame, &resources);
            }
            frame
        };
        shots.push((tick, frame));
    };

    match &tape {
        Some(tape) => {
            let last = tape.ticks();
            let at = args.at.unwrap_or(last).min(last);
            let mut failed = None;
            let outcome = run_tape_with(&mut sim, tape, |sim, tick| {
                if failed.is_none() {
                    if let Err(err) = view.after_step(sim, &mut view_assets) {
                        failed = Some(err);
                    }
                }
                let wanted = match args.every {
                    Some(n) => tick % n == 0 || tick == last,
                    None => tick == at,
                };
                if wanted {
                    shoot(sim, &view, tick);
                }
            });
            if let Some(err) = failed {
                return Err(err);
            }
            for failure in &outcome.failures {
                eprintln!("note: tape assertion failed — {failure}");
            }
        }
        None => shoot(&sim, &view, 0),
    }

    let dir = match (&args.out, args.every) {
        (Some(out), Some(_)) => out.clone(),
        (Some(out), None) => out.parent().map(Path::to_path_buf).unwrap_or_default(),
        (None, _) => PathBuf::from("target/render"),
    };
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    for (tick, frame) in &shots {
        let image = upscale(renderer.render(frame), args.scale);
        let path = match (&args.out, args.every) {
            (Some(out), None) => out.clone(),
            _ => dir.join(format!("{name}_{tick:05}.png")),
        };
        image
            .save(&path)
            .with_context(|| format!("failed to write {}", path.display()))?;
        println!("{}", path.display());
    }
    Ok(())
}

/// The shared resources a scene draws with. Only the pause menu needs them,
/// and only to satisfy the scene interface — nothing here touches a window.
fn resources() -> anyhow::Result<Resources> {
    Ok(Resources {
        config: Config::load("config.toml")?,
        assets: Assets::new(),
        input: InputLatch::default(),
        saves: FileStore::new(FileStore::default_dir()),
    })
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "render".to_string())
}

fn upscale(image: image::RgbaImage, scale: u32) -> image::RgbaImage {
    if scale == 1 {
        return image;
    }
    image::imageops::resize(
        &image,
        image.width() * scale,
        image.height() * scale,
        image::imageops::FilterType::Nearest,
    )
}
