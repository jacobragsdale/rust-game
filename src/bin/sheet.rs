//! Render a clip set's animations as PNGs, to check by eye that each clip
//! points at the art it claims to.
//!
//! This exists because of a bug the rest of the harness structurally cannot
//! see. Golden traces record clip *names* and frame indices, not which sheet
//! cells those resolve to — so a clip set that had `wall_slide` pointing at
//! the spell-cast frames produced byte-identical traces before and after the
//! fix. Nothing automated caught it and nothing automated could: "is this the
//! right animation?" is a question about pixels.
//!
//! What can be fixed is the cost of looking. Verifying a clip set used to mean
//! writing a throwaway script; now it is one command, and each clip lands in
//! its own file named after itself, so no legend is needed to read the output.
//!
//! ```text
//! cargo run --bin sheet -- player            # every clip of player.ron
//! cargo run --bin sheet -- knight --clip run # just one
//! cargo run --bin sheet -- player --grid     # the raw sheet, cells numbered
//!                                            # row-major, for deriving a
//!                                            # layout from a new art pack
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context as _};
use image::{Rgba, RgbaImage};

use supergame::assets::{Assets, Clip, ClipSet};

const USAGE: &str = "\
usage: sheet <clip-set> [options]

  <clip-set>       name under assets/data/animations/, e.g. player, knight
  --clip <name>    render only this clip
  --grid           also render the raw sheet with a cell grid
  --scale <n>      pixel zoom (default 5)
  --out <dir>      output directory (default target/sheets/<clip-set>)
  -h, --help       show this help
";

/// Backdrop, so transparent pixels are visible as absence rather than as
/// whatever the image viewer decides.
const BG: Rgba<u8> = Rgba([24, 22, 34, 255]);
/// Frame separator, and the grid in `--grid` mode.
const RULE: Rgba<u8> = Rgba([70, 68, 92, 255]);
/// Every tenth grid line, so counting cells does not require care.
const RULE_TEN: Rgba<u8> = Rgba([130, 120, 170, 255]);

struct Args {
    set: String,
    clip: Option<String>,
    grid: bool,
    scale: u32,
    out: Option<PathBuf>,
}

fn parse_args() -> anyhow::Result<Option<Args>> {
    let mut argv = std::env::args().skip(1);
    let mut set = None;
    let mut clip = None;
    let mut grid = false;
    let mut scale = 5;
    let mut out = None;

    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--clip" => clip = Some(argv.next().context("--clip needs a name")?),
            "--grid" => grid = true,
            "--scale" => {
                let raw = argv.next().context("--scale needs a number")?;
                scale = raw
                    .parse()
                    .with_context(|| format!("`{raw}` is not a scale"))?;
                anyhow::ensure!(scale > 0, "scale must be at least 1");
            }
            "--out" => out = Some(PathBuf::from(argv.next().context("--out needs a path")?)),
            other if other.starts_with('-') => bail!("unknown argument `{other}`\n\n{USAGE}"),
            other if set.is_none() => set = Some(other.to_string()),
            other => bail!("unexpected argument `{other}`\n\n{USAGE}"),
        }
    }

    let Some(set) = set else {
        bail!("which clip set?\n\n{USAGE}");
    };
    Ok(Some(Args {
        set,
        clip,
        grid,
        scale,
        out,
    }))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("sheet: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    let Some(args) = parse_args()? else {
        print!("{USAGE}");
        return Ok(());
    };

    let mut assets = Assets::new();
    let set = assets
        .clip_set(&args.set)
        .with_context(|| format!("failed to load clip set `{}`", args.set))?;

    let dir = args
        .out
        .clone()
        .unwrap_or_else(|| PathBuf::from("target/sheets").join(&args.set));
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;

    // Sorted so repeated runs list clips in the same order — a clip set is a
    // HashMap, and unstable output is miserable to compare between runs.
    let mut names: Vec<&String> = set.clips.keys().collect();
    names.sort();

    let mut rendered = 0;
    for name in names {
        if args.clip.as_ref().is_some_and(|only| only != name) {
            continue;
        }
        let clip = set.clip(name).expect("name came from the set");
        let sheet_name = set.sheet_of(clip);
        let sheet = assets
            .decode_image(sheet_name, None)
            .with_context(|| format!("clip `{name}` names sheet `{sheet_name}`"))?;

        let (fw, fh) = set.frame_size_of(clip);
        let image = render_clip(&sheet, clip, fw as u32, fh as u32, args.scale)?;
        let path = dir.join(format!("{name}.png"));
        image
            .save(&path)
            .with_context(|| format!("failed to write {}", path.display()))?;

        let cells: Vec<String> = clip
            .frames
            .iter()
            .map(|(c, r)| format!("({c},{r})"))
            .collect();
        println!(
            "  {name:<14} {sheet_name:<26} {fw:>4.0}x{fh:<4.0} {:>2} frames  {}",
            clip.frames.len(),
            cells.join(" ")
        );
        rendered += 1;
    }

    if rendered == 0 {
        match &args.clip {
            Some(name) => bail!("clip set `{}` has no clip `{name}`", args.set),
            None => bail!("clip set `{}` defines no clips", args.set),
        }
    }

    if args.grid {
        let sheet_name = default_sheet(&set)?;
        let sheet = assets.decode_image(&sheet_name, None)?;
        let (fw, fh) = default_frame_size(&set)?;
        let image = render_grid(&sheet, fw as u32, fh as u32, args.scale.max(2));
        let path = dir.join("_grid.png");
        image.save(&path)?;
        let cols = sheet.width() / fw as u32;
        println!(
            "\n  _grid.png      {sheet_name} at {fw:.0}x{fh:.0}, {cols} columns \
             — frame i is at cell (i % {cols}, i / {cols})"
        );
    }

    println!("\n{rendered} clip(s) -> {}", dir.display());
    Ok(())
}

/// A clip's frames left to right, separated by a rule, upscaled with nearest
/// neighbour so pixel art stays pixel art.
fn render_clip(
    sheet: &RgbaImage,
    clip: &Clip,
    fw: u32,
    fh: u32,
    scale: u32,
) -> anyhow::Result<RgbaImage> {
    anyhow::ensure!(!clip.frames.is_empty(), "clip has no frames");
    let gap = 2;
    let cell_w = fw * scale + gap;
    let mut out = RgbaImage::from_pixel(
        cell_w * clip.frames.len() as u32 + gap,
        fh * scale + gap * 2,
        BG,
    );

    for (i, &(col, row)) in clip.frames.iter().enumerate() {
        let (sx, sy) = (col * fw, row * fh);
        anyhow::ensure!(
            sx + fw <= sheet.width() && sy + fh <= sheet.height(),
            "frame ({col}, {row}) is outside the {}x{} sheet",
            sheet.width(),
            sheet.height()
        );
        let ox = gap + i as u32 * cell_w;
        blit(sheet, &mut out, sx, sy, fw, fh, ox, gap, scale);

        // A rule between frames, so where one ends is never ambiguous.
        if i + 1 < clip.frames.len() {
            let x = ox + fw * scale;
            for y in 0..out.height() {
                out.put_pixel(x, y, RULE);
            }
        }
    }
    Ok(out)
}

/// The whole sheet with a cell grid drawn over it, for working out how a new
/// art pack lays its animations out.
fn render_grid(sheet: &RgbaImage, fw: u32, fh: u32, scale: u32) -> RgbaImage {
    let mut out = RgbaImage::from_pixel(sheet.width() * scale, sheet.height() * scale, BG);
    blit(
        sheet,
        &mut out,
        0,
        0,
        sheet.width(),
        sheet.height(),
        0,
        0,
        scale,
    );

    let cols = sheet.width() / fw.max(1);
    for c in 0..=cols {
        let x = (c * fw * scale).min(out.width() - 1);
        let colour = if c % 10 == 0 { RULE_TEN } else { RULE };
        for y in 0..out.height() {
            out.put_pixel(x, y, colour);
        }
    }
    let rows = sheet.height() / fh.max(1);
    for r in 0..=rows {
        let y = (r * fh * scale).min(out.height() - 1);
        let colour = if r % 10 == 0 { RULE_TEN } else { RULE };
        for x in 0..out.width() {
            out.put_pixel(x, y, colour);
        }
    }
    out
}

/// Copy a region of `src` into `dst`, scaled up, compositing over whatever is
/// already there so transparency reads against the backdrop.
#[allow(clippy::too_many_arguments)]
fn blit(
    src: &RgbaImage,
    dst: &mut RgbaImage,
    sx: u32,
    sy: u32,
    w: u32,
    h: u32,
    dx: u32,
    dy: u32,
    scale: u32,
) {
    for y in 0..h {
        for x in 0..w {
            let px = src.get_pixel(sx + x, sy + y);
            if px[3] == 0 {
                continue;
            }
            for oy in 0..scale {
                for ox in 0..scale {
                    let (tx, ty) = (dx + x * scale + ox, dy + y * scale + oy);
                    if tx < dst.width() && ty < dst.height() {
                        dst.put_pixel(tx, ty, *px);
                    }
                }
            }
        }
    }
}

/// The sheet most of a set's clips use, for `--grid`.
fn default_sheet(set: &ClipSet) -> anyhow::Result<String> {
    if let Some(sheet) = &set.sheet {
        return Ok(sheet.clone());
    }
    let mut names: Vec<&String> = set.clips.keys().collect();
    names.sort();
    let first = names.first().context("clip set defines no clips")?;
    let clip = set.clip(first).expect("name came from the set");
    Ok(set.sheet_of(clip).to_string())
}

fn default_frame_size(set: &ClipSet) -> anyhow::Result<(f32, f32)> {
    if let Some(size) = set.frame_size {
        return Ok(size);
    }
    let mut names: Vec<&String> = set.clips.keys().collect();
    names.sort();
    let first = names.first().context("clip set defines no clips")?;
    let clip = set.clip(first).expect("name came from the set");
    Ok(set.frame_size_of(clip))
}
