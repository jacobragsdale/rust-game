//! Translates raw keyboard state into gameplay intent. Gameplay systems only
//! ever see `PlayerInput`, never the keyboard. Arrows and WASD both work.

use ggez::winit::event::VirtualKeyCode as Key;
use ggez::Context;

/// Every key that jumps.
const JUMP_KEYS: [Key; 3] = [Key::Up, Key::W, Key::Space];
/// Every key that swings the sword. `J` for WASD hands, `X` for arrow hands.
const ATTACK_KEYS: [Key; 2] = [Key::J, Key::X];

#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerInput {
    pub left: bool,
    pub right: bool,
    pub down: bool,
    /// Edge-triggered jump: one press, one jump (buffered by the avatar).
    pub jump_pressed: bool,
    /// Jump key currently held: releasing early cuts the jump short.
    pub jump_held: bool,
    /// Edge-triggered attack: one press, one swing. Holding does not flail.
    pub attack_pressed: bool,
}

/// Holds one-shot presses from the moment the OS reports them until a
/// simulation tick consumes them.
///
/// This exists because presses and ticks are not on the same clock. The sim
/// runs on a fixed 60 Hz accumulator, but ggez clears its "just pressed" edge
/// once per rendered *frame*. Polling that edge from inside the tick loop
/// therefore loses presses: whenever the window renders faster than 60 Hz —
/// vsync on a 120 Hz display, say — a good share of frames run no tick at
/// all, and any press that landed on one of those frames is gone before the
/// simulation ever sees it. That is the "I hit jump and nothing happened",
/// and because which frames get a tick drifts, it feels random.
///
/// A frame that runs *two* ticks has the mirror-image bug: the same edge is
/// read twice, so one tap spends the ground jump and the double jump back to
/// back and leaves the player airborne with nothing left.
///
/// Latching at the event and clearing on consumption makes it exactly one
/// action per press, whatever the frames and ticks are doing.
///
/// One latch per one-shot action rather than a single bool, because attacking
/// has the same problem jumping does and would otherwise be dropped on exactly
/// the frames a jump is.
#[derive(Debug, Default)]
pub struct InputLatch {
    jump: bool,
    attack: bool,
}

impl InputLatch {
    /// Record a key-down event. Keys bound to nothing are ignored, and the
    /// caller is expected to have already filtered out OS key-repeat — holding
    /// a key must not re-arm its latch.
    pub fn key_down(&mut self, key: Key) {
        if JUMP_KEYS.contains(&key) {
            self.jump = true;
        }
        if ATTACK_KEYS.contains(&key) {
            self.attack = true;
        }
    }

    /// Drop every pending press. Called on scene changes so a press meant for
    /// a menu does not turn into a jump the instant gameplay resumes.
    pub fn clear(&mut self) {
        *self = InputLatch::default();
    }
}

pub fn read(ctx: &Context, latch: &mut InputLatch) -> PlayerInput {
    let kb = &ctx.keyboard;
    let left = kb.is_key_pressed(Key::Left) || kb.is_key_pressed(Key::A);
    let right = kb.is_key_pressed(Key::Right) || kb.is_key_pressed(Key::D);

    PlayerInput {
        left: left && !right,
        right: right && !left,
        down: kb.is_key_pressed(Key::Down) || kb.is_key_pressed(Key::S),
        jump_pressed: std::mem::take(&mut latch.jump),
        jump_held: JUMP_KEYS.iter().any(|&key| kb.is_key_pressed(key)),
        attack_pressed: std::mem::take(&mut latch.attack),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_survives_until_a_tick_consumes_it() {
        let mut latch = InputLatch::default();
        latch.key_down(Key::Space);
        // ...however many frames render before the next fixed tick.
        assert!(
            std::mem::take(&mut latch.jump),
            "the press is still there when a tick runs"
        );
        assert!(!std::mem::take(&mut latch.jump), "and only fires once");
    }

    #[test]
    fn every_jump_key_latches_and_other_keys_do_not() {
        for key in JUMP_KEYS {
            let mut latch = InputLatch::default();
            latch.key_down(key);
            assert!(std::mem::take(&mut latch.jump), "{key:?} should jump");
        }
        let mut latch = InputLatch::default();
        latch.key_down(Key::Left);
        latch.key_down(Key::P);
        assert!(!std::mem::take(&mut latch.jump));
    }

    /// Two presses inside one tick are still one jump. The player cannot
    /// physically double-tap in 16 ms, but key repeat and a stalled frame
    /// both look like this, and both used to burn the double jump.
    #[test]
    fn repeated_presses_before_a_tick_collapse_to_one_jump() {
        let mut latch = InputLatch::default();
        latch.key_down(Key::Up);
        latch.key_down(Key::Up);
        assert!(std::mem::take(&mut latch.jump));
        assert!(!std::mem::take(&mut latch.jump));
    }

    #[test]
    fn clearing_drops_a_pending_press() {
        let mut latch = InputLatch::default();
        latch.key_down(Key::Up);
        latch.clear();
        assert!(
            !std::mem::take(&mut latch.jump),
            "a press meant for a menu is not a jump"
        );
    }
}
