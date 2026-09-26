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
//! [`crate::view::View`] draws this whenever the sim says the mode is
//! [`crate::sim::Mode::Dialogue`]. One source of truth.
//!
//! There are no portraits, because there is no portrait art. The speaker's name
//! sits where one would go, so adding them later moves this file and nothing
//! else.

use ggez::glam::Vec2;

use crate::render::{font, Color, Frame};
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
/// Room for the `> ` marker in front of a reply.
const MARKER_W: f32 = 10.0;
/// How tall the box is, as a fraction of the view. Leaves the fight you
/// walked away from visible above it.
const BOX_FRACTION: f32 = 0.42;

/// Where the box goes in a view of `view` size: its origin and size. At the
/// bottom of the screen unless `top` says the person speaking is down there.
fn panel(view: Vec2, top: bool) -> (Vec2, Vec2) {
    let size = Vec2::new(view.x - MARGIN * 2.0, (view.y * BOX_FRACTION).floor());
    let y = if top {
        MARGIN
    } else {
        view.y - MARGIN - size.y
    };
    (Vec2::new(MARGIN, y), size)
}

/// How wide a line of speech may be, in pixels.
fn text_width(view: Vec2) -> f32 {
    panel(view, false).1.x - PAD * 2.0
}

/// Draw the overlay over the frozen world. `view` is the picture's size and
/// `offset` the camera's, which is what says where on screen the person
/// speaking is standing.
pub fn draw(frame: &mut Frame, sim: &Sim, view: Vec2, offset: Vec2) {
    let Some(talk) = sim.conversation() else {
        return;
    };

    frame.rect(Vec2::ZERO, view, SCRIM);
    // The person you are talking to has to stay visible: when they are in the
    // bottom half of the screen — a speaker in a doorway, a sign on the floor —
    // the box goes to the top instead of over them.
    let speaker_low = sim
        .prompt()
        .and_then(|p| {
            sim.world
                .get::<&crate::ecs::components::Position>(p.entity)
                .ok()
        })
        .is_some_and(|pos| pos.0.y - offset.y > view.y / 2.0);
    let (origin, size) = panel(view, speaker_low);
    frame.panel(origin, size, PANEL, BORDER);

    let width = text_width(view);
    let mut at = origin + Vec2::splat(PAD);

    frame.text(at, talk.speaker(), SPEAKER);
    at.y += font::LINE_H + 2.0;

    // The speech, wrapped rather than clipped: a line longer than the box is
    // the ordinary case for anything anyone would actually write. Widths are
    // the font's real ones, so a wrapped line is inside the box by
    // construction rather than by an estimate of how wide a letter is.
    for line in font::wrap(talk.line(), width) {
        frame.text(at, &line, TEXT);
        at.y += font::LINE_H;
    }

    if !talk.choosing() {
        // Still speaking. Say which key reads on, and how far through it is,
        // so a long speech does not read as a hang.
        let (line, lines) = talk.line_index();
        frame.text(
            Vec2::new(origin.x + PAD, origin.y + size.y - PAD - font::LINE_H),
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
        let lines = font::wrap(&choice.text, width - MARKER_W);
        if selected {
            frame.rect(
                Vec2::new(origin.x + PAD / 2.0, at.y - 1.0),
                Vec2::new(size.x - PAD, font::LINE_H * lines.len() as f32),
                SELECTION,
            );
            frame.text(at, ">", TEXT);
        }
        for line in lines {
            frame.text(
                at + Vec2::new(MARKER_W, 0.0),
                &line,
                if selected { TEXT } else { DIM_TEXT },
            );
            at.y += font::LINE_H;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::VIEW;

    /// Every shipped line has to fit the real box — there is no scrolling, so
    /// a speech that needs fifteen lines would be drawn straight through the
    /// bottom of the panel. Measured with the font the box is drawn in, so
    /// this is the same arithmetic as the picture rather than an estimate of it.
    #[test]
    fn every_shipped_line_fits_the_box() {
        let (_, size) = panel(VIEW, false);
        let width = text_width(VIEW);
        // Speaker, the speech, and the replies, all inside the panel.
        let rows = ((size.y - PAD * 2.0 - 6.0) / font::LINE_H) as usize;

        let table = crate::assets::DialogueTable::shipped();
        for id in table.ids() {
            let graph = table.get(id).expect("just listed");
            for node_id in graph.node_ids() {
                let node = graph.node(node_id).expect("just listed");
                let speech: usize = node
                    .lines
                    .iter()
                    .map(|line| font::wrap(line, width).len())
                    .max()
                    .unwrap_or(0);
                let replies: usize = node
                    .choices
                    .iter()
                    .map(|choice| font::wrap(&choice.text, width - MARKER_W).len())
                    .sum();
                let needed = 1 + speech + replies;
                assert!(
                    needed <= rows,
                    "`{id}` node `{node_id}` needs {needed} lines and the box holds {rows}"
                );
                for line in node
                    .lines
                    .iter()
                    .chain(node.choices.iter().map(|c| &c.text))
                {
                    for wrapped in font::wrap(line, width) {
                        assert!(font::width(&wrapped) <= width, "`{wrapped}` overflows");
                    }
                }
            }
        }
    }
}
