//! The dialogue overlay. It draws, and it decides nothing.
//!
//! Which graph, which node, which replies are on offer, where the highlight is
//! and what taking one does all live on [`Sim`], in
//! [`crate::systems::dialogue`], because a choice takes an item out of the bag
//! and sets a quest flag — which is simulation wearing a UI hat. This file
//! reads that state and turns it into rectangles and text. If anything here
//! ever decides something, a tape stops being able to see it, and the largest
//! system in the game becomes untestable in one commit.
//!
//! This is exactly [`crate::scenes::inventory`]'s arrangement, and deliberately
//! so — M4 established it on the simpler of the two modal features precisely so
//! that this one would be a copy rather than a design.
//!
//! **Why this is not a [`crate::scenes::Scene`] on the stack.** A stack entry
//! would be a second copy of "a conversation is open" living beside the sim's
//! mode, and the two would eventually disagree. Instead
//! [`crate::scenes::adventure::AdventureScene`] calls [`draw`] whenever the sim
//! says the mode is [`crate::sim::Mode::Dialogue`]. One source of truth.
//!
//! There are no portraits, because there is no portrait art. The speaker's name
//! sits where one would go, so adding them later moves this file and nothing
//! else.

use ggez::glam::Vec2;
use ggez::graphics::{Canvas, Color, DrawParam, PxScale, Quad, Text, TextFragment};

use crate::sim::Sim;

/// A scrim over the frozen world, deliberately lighter than the inventory's:
/// a conversation happens *in* the world, and the person you are talking to has
/// to stay visible behind the box.
const SCRIM: Color = Color::new(0.02, 0.02, 0.05, 0.45);
const PANEL: Color = Color::new(0.07, 0.06, 0.11, 0.95);
const BORDER: Color = Color::new(0.55, 0.55, 0.68, 1.0);
const SPEAKER: Color = Color::new(0.95, 0.85, 0.35, 1.0);
const TEXT: Color = Color::new(0.90, 0.90, 0.95, 1.0);
const DIM_TEXT: Color = Color::new(0.52, 0.52, 0.62, 1.0);
const SELECTION: Color = Color::new(0.24, 0.22, 0.34, 1.0);

const MARGIN: f32 = 16.0;
const PAD: f32 = 6.0;
const FONT: f32 = 10.0;
const LINE_H: f32 = 12.0;
const BORDER_W: f32 = 1.0;
/// How wide a glyph is at [`FONT`], near enough for wrapping. The font is not
/// monospaced, so this is an estimate — deliberately a slight over-estimate, so
/// a wrapped line is short of the box rather than over its edge.
const CHAR_W: f32 = 5.2;
/// How tall the box is, as a fraction of the view. A third leaves the fight you
/// walked away from visible above it.
const BOX_FRACTION: f32 = 0.42;

/// Draw the overlay over the frozen world. `view` is the internal canvas size.
pub fn draw(canvas: &mut Canvas, sim: &Sim, view: Vec2) {
    let Some(talk) = sim.conversation() else {
        return;
    };

    fill(canvas, Vec2::ZERO, view, SCRIM);

    let size = Vec2::new(view.x - MARGIN * 2.0, (view.y * BOX_FRACTION).floor());
    let origin = Vec2::new(MARGIN, view.y - MARGIN - size.y);
    bordered(canvas, origin, size, PANEL);

    let columns = ((size.x - PAD * 2.0) / CHAR_W) as usize;
    let mut at = origin + Vec2::splat(PAD);

    label(canvas, at, talk.speaker(), SPEAKER);
    at.y += LINE_H + 2.0;

    // The speech, wrapped rather than clipped: a line longer than the box is
    // the ordinary case for anything anyone would actually write.
    for line in wrap(talk.line(), columns) {
        label(canvas, at, &line, TEXT);
        at.y += LINE_H;
    }

    if !talk.choosing() {
        // Still speaking. Say which key reads on, and how far through it is,
        // so a long speech does not read as a hang.
        let (line, lines) = talk.line_index();
        label(
            canvas,
            Vec2::new(origin.x + PAD, origin.y + size.y - PAD - LINE_H),
            &format!("enter: more  ({}/{})", line + 1, lines),
            DIM_TEXT,
        );
        return;
    }

    at.y += 4.0;
    for (row, choice) in talk.choices().iter().enumerate() {
        // Only what may actually be taken is listed: a choice whose condition
        // fails is absent rather than greyed out. `crate::systems::dialogue`
        // has the argument; this file could not reintroduce a locked row if it
        // wanted to, because it is never handed one.
        let selected = row == talk.selection();
        if selected {
            fill(
                canvas,
                Vec2::new(origin.x + PAD / 2.0, at.y - 1.0),
                Vec2::new(size.x - PAD, LINE_H),
                SELECTION,
            );
        }
        let marker = if selected { ">" } else { " " };
        for (index, line) in wrap(&choice.text, columns.saturating_sub(2))
            .into_iter()
            .enumerate()
        {
            let prefix = if index == 0 { marker } else { " " };
            label(
                canvas,
                Vec2::new(at.x, at.y),
                &format!("{prefix} {line}"),
                if selected { TEXT } else { DIM_TEXT },
            );
            at.y += LINE_H;
        }
    }
}

/// Break `text` into lines of at most `columns` characters, on word boundaries.
///
/// Greedy, and it breaks a word longer than the whole line rather than letting
/// it run off the edge — which is what an id or a URL in a line of dialogue
/// would be. Pure and total, so it is tested without a graphics context; the
/// acceptance criterion is "text longer than the box wraps rather than
/// overflowing", and this is the whole of what makes that true.
pub(crate) fn wrap(text: &str, columns: usize) -> Vec<String> {
    if columns == 0 {
        return vec![text.to_string()];
    }

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        // A word that cannot fit on a line of its own is cut, not hidden.
        let mut word = word;
        while word.chars().count() > columns {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            let cut = word
                .char_indices()
                .nth(columns)
                .map(|(index, _)| index)
                .unwrap_or(word.len());
            lines.push(word[..cut].to_string());
            word = &word[cut..];
        }
        if word.is_empty() {
            continue; // the word was an exact multiple of the width
        }

        let room = if current.is_empty() {
            columns
        } else {
            columns.saturating_sub(current.chars().count() + 1)
        };
        if word.chars().count() > room {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }

    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn label(canvas: &mut Canvas, at: Vec2, text: &str, colour: Color) {
    let text = Text::new(
        TextFragment::new(text)
            .scale(PxScale::from(FONT))
            .color(colour),
    );
    canvas.draw(&text, DrawParam::default().dest(at.floor()));
}

/// A filled box with a one-pixel border, the same treatment the HUD's bars and
/// the inventory's panes use.
fn bordered(canvas: &mut Canvas, origin: Vec2, size: Vec2, colour: Color) {
    fill(
        canvas,
        origin - Vec2::splat(BORDER_W),
        size + Vec2::splat(BORDER_W * 2.0),
        BORDER,
    );
    fill(canvas, origin, size, colour);
}

fn fill(canvas: &mut Canvas, origin: Vec2, size: Vec2, colour: Color) {
    canvas.draw(
        &Quad,
        DrawParam::new()
            .dest(origin.floor())
            .scale(size)
            .color(colour),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D-3's acceptance criterion: text longer than the box wraps rather than
    /// overflowing. Checked on the function that decides it, because "does this
    /// overflow?" is otherwise a question only a screenshot answers.
    #[test]
    fn a_long_line_wraps_at_word_boundaries_and_never_exceeds_the_width() {
        let text = "You are the one who came down out of the castle, and you \
                    have the look of someone who has been fighting.";
        // Wide enough for the longest word, so this is a test of *wrapping*
        // rather than of the word-breaking below it.
        for columns in [12usize, 20, 40, 64] {
            let lines = wrap(text, columns);
            assert!(lines.len() > 1, "{columns} columns should need wrapping");
            for line in &lines {
                assert!(
                    line.chars().count() <= columns,
                    "`{line}` is {} wide, over {columns}",
                    line.chars().count()
                );
            }
            assert_eq!(
                lines.join(" ").split_whitespace().collect::<Vec<_>>(),
                text.split_whitespace().collect::<Vec<_>>(),
                "wrapping lost or reordered a word"
            );
        }
    }

    #[test]
    fn a_short_line_is_left_alone() {
        assert_eq!(wrap("Not now.", 40), vec!["Not now.".to_string()]);
        assert_eq!(wrap("", 40), vec![String::new()]);
    }

    /// A word longer than the whole line is cut rather than allowed to run off
    /// the edge — an item id or a place name is exactly this shape.
    #[test]
    fn a_word_wider_than_the_box_is_broken_rather_than_hidden() {
        let lines = wrap("aaa supercalifragilistic bbb", 8);
        for line in &lines {
            assert!(line.chars().count() <= 8, "`{line}`");
        }
        assert_eq!(
            lines.concat().replace(' ', ""),
            "aaasupercalifragilisticbbb",
            "characters were dropped: {lines:?}"
        );
    }

    /// Every shipped line has to fit the real box in a sane number of lines —
    /// there is no scrolling, so a speech that needs fifteen lines would be
    /// drawn straight through the bottom of the panel.
    #[test]
    fn every_shipped_line_fits_the_box() {
        let view = Vec2::new(
            crate::scenes::adventure::INTERNAL_WIDTH,
            crate::scenes::adventure::INTERNAL_HEIGHT,
        );
        let width = view.x - MARGIN * 2.0;
        let height = (view.y * BOX_FRACTION).floor();
        let columns = ((width - PAD * 2.0) / CHAR_W) as usize;
        // Speaker, the speech, and the replies, all inside the panel.
        let rows = ((height - PAD * 2.0) / LINE_H) as usize;

        let table = crate::assets::DialogueTable::shipped();
        for id in table.ids() {
            let graph = table.get(id).expect("just listed");
            for node_id in graph.node_ids() {
                let node = graph.node(node_id).expect("just listed");
                let speech: usize = node
                    .lines
                    .iter()
                    .map(|line| wrap(line, columns).len())
                    .max()
                    .unwrap_or(0);
                let replies: usize = node
                    .choices
                    .iter()
                    .map(|choice| wrap(&choice.text, columns.saturating_sub(2)).len())
                    .sum();
                let needed = 1 + speech + replies;
                assert!(
                    needed <= rows,
                    "`{id}` node `{node_id}` needs {needed} lines and the box holds {rows}"
                );
            }
        }
    }
}
