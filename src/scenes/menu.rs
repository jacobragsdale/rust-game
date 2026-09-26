//! The look every menu shares: a panel in the middle of the screen, a title,
//! a column of entries with the selected one marked, and a line of small print.
//!
//! Drawing only. What an entry *does* is the owning scene's business; this is
//! here so the title screen and the pause menu cannot drift into two styles.

use ggez::glam::Vec2;

use crate::render::{font, Color, Frame, VIEW};

const SCRIM: Color = Color::new(0.02, 0.02, 0.05, 0.6);
const PANEL: Color = Color::new(0.07, 0.06, 0.11, 0.95);
const BORDER: Color = Color::new(0.55, 0.55, 0.68, 1.0);
const TITLE: Color = Color::new(0.95, 0.85, 0.35, 1.0);
const ENTRY: Color = Color::new(0.75, 0.75, 0.82, 1.0);
const SELECTED: Color = Color::new(1.0, 0.93, 0.62, 1.0);
const DIM: Color = Color::new(0.52, 0.52, 0.62, 1.0);

/// Scale of the title's letters, and of each entry's.
const TITLE_SCALE: f32 = 3.0;
const ENTRY_SCALE: f32 = 2.0;
const ENTRY_GAP: f32 = 6.0;

pub struct Menu<'a> {
    pub title: &'a str,
    pub entries: &'a [&'a str],
    pub selection: usize,
    /// A line of small print under the entries — a status message or a hint.
    /// Wrapped to the panel, so an error that names a file path still fits.
    pub footer: &'a str,
    /// Whether to darken what is behind the panel.
    pub scrim: bool,
}

impl Menu<'_> {
    pub fn draw(&self, frame: &mut Frame) {
        if self.scrim {
            frame.rect(Vec2::ZERO, VIEW, SCRIM);
        }
        let entry_h = font::GLYPH_H as f32 * ENTRY_SCALE + ENTRY_GAP;
        let width = 360.0;
        let footer = font::wrap(self.footer, width - 16.0);
        let height = 24.0
            + font::GLYPH_H as f32 * TITLE_SCALE
            + 16.0
            + entry_h * self.entries.len() as f32
            + 8.0
            + font::LINE_H * footer.len() as f32
            + 12.0;
        let origin = ((VIEW - Vec2::new(width, height)) / 2.0).floor();
        frame.panel(origin, Vec2::new(width, height), PANEL, BORDER);

        let centre = VIEW.x / 2.0;
        let mut y = origin.y + 16.0;
        frame.text_centred(centre, y, self.title, TITLE, TITLE_SCALE);
        y += font::GLYPH_H as f32 * TITLE_SCALE + 16.0;

        for (index, entry) in self.entries.iter().enumerate() {
            let selected = index == self.selection;
            let label = if selected {
                format!("> {entry} <")
            } else {
                entry.to_string()
            };
            let color = if selected { SELECTED } else { ENTRY };
            frame.text_centred(centre, y, &label, color, ENTRY_SCALE);
            y += entry_h;
        }

        y += 8.0;
        for line in footer {
            frame.text_centred(centre, y, &line, DIM, 1.0);
            y += font::LINE_H;
        }
    }
}
