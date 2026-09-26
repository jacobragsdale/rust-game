//! Every shipped map, and every menu, draws — headlessly, through the same
//! code the window uses.
//!
//! Draw code used to be the one part of the game no test could reach: it
//! needed a graphics context, so a clip with no frames or a sheet name with a
//! typo was a crash or a hole on screen that only a person would ever see.
//! Now a frame is data and the CPU backend can draw it, so the spawn view of
//! every map and the whole of every map are drawn here on every `cargo test`,
//! and a picture that comes out blank fails.

use std::path::Path;

use supergame::assets::Assets;
use supergame::config::Config;
use supergame::render::cpu::CpuRenderer;
use supergame::render::Frame;
use supergame::save::FileStore;
use supergame::scenes::main_menu::MainMenuScene;
use supergame::scenes::pause::PauseScene;
use supergame::scenes::{Resources, Scene};
use supergame::sim::Sim;
use supergame::systems::input::{InputLatch, PlayerInput};
use supergame::view::View;

fn maps() -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/maps");
    let mut maps: Vec<String> = std::fs::read_dir(&dir)
        .expect("assets/maps exists")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "ron"))
        .map(|p| format!("maps/{}", p.file_name().unwrap().to_string_lossy()))
        .collect();
    maps.sort();
    maps
}

/// How many distinct colours a picture has. A frame that drew nothing but its
/// clear colour has one; a frame with a world in it has hundreds.
fn colours(image: &image::RgbaImage) -> usize {
    let mut seen: Vec<[u8; 4]> = image.pixels().map(|p| p.0).collect();
    seen.sort_unstable();
    seen.dedup();
    seen.len()
}

#[test]
fn every_map_draws_from_its_spawn_point_and_in_full() {
    let mut renderer = CpuRenderer::new(Assets::new());
    for map in maps() {
        let mut sim =
            Sim::load(&mut Assets::new(), &map).unwrap_or_else(|e| panic!("{map}: {e:#}"));
        let mut assets = Assets::new();
        let mut view = View::new(&sim, &mut assets).unwrap_or_else(|e| panic!("{map}: {e:#}"));
        for _ in 0..30 {
            sim.step(PlayerInput::default());
            view.after_step(&sim, &mut assets).unwrap();
        }
        // A map may open with a story — a trigger on its spawn, told over the
        // black of the fade-in, which a modal screen holds still. Hear it out,
        // then let the fade finish, and judge the view the player is left in.
        if sim.mode() == supergame::sim::Mode::Dialogue {
            let cancel = PlayerInput::from_actions(&[supergame::systems::input::Action::Cancel]);
            sim.step(cancel);
            view.after_step(&sim, &mut assets).unwrap();
            for _ in 0..30 {
                sim.step(PlayerInput::default());
                view.after_step(&sim, &mut assets).unwrap();
            }
        }

        let mut frame = Frame::default();
        view.draw(&sim, &mut frame);
        let image = renderer.render(&frame);
        assert_eq!(image.dimensions(), (640, 360));
        assert!(
            colours(&image) > 20,
            "{map}: the spawn view is nearly blank"
        );

        view.debug.enabled = true;
        let level = renderer.render(&view.draw_level(&sim));
        assert_eq!(
            image::GenericImageView::dimensions(&level),
            (
                sim.level.pixel_width() as u32,
                sim.level.pixel_height() as u32
            ),
            "{map}: the whole-level picture is the level's size"
        );
        assert!(
            colours(&level) > 20,
            "{map}: the level picture is nearly blank"
        );
    }
}

#[test]
fn the_menus_draw() {
    let res = Resources {
        config: Config::load("config.toml").expect("config.toml loads"),
        assets: Assets::new(),
        input: InputLatch::default(),
        saves: FileStore::new(std::env::temp_dir().join("supergame-render-test-saves")),
    };
    let mut renderer = CpuRenderer::new(Assets::new());

    let mut frame = Frame::default();
    MainMenuScene::default().draw(&mut frame, &res);
    assert!(
        colours(&renderer.render(&frame)) > 3,
        "the title screen is blank"
    );

    let sim = Sim::load(&mut Assets::new(), "maps/village.ron").unwrap();
    let mut frame = Frame::default();
    PauseScene::new(sim.save()).draw(&mut frame, &res);
    assert!(
        colours(&renderer.render(&frame)) > 3,
        "the pause menu is blank"
    );
}
