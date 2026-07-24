//! Translates raw keyboard state into gameplay intent. Gameplay systems only
//! ever see `PlayerInput`, never the keyboard. Arrows and WASD both work.

use ggez::winit::event::VirtualKeyCode as Key;
use ggez::Context;

/// Every key that jumps.
const JUMP_KEYS: [Key; 3] = [Key::Up, Key::W, Key::Space];

#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerInput {
    pub left: bool,
    pub right: bool,
    pub down: bool,
    /// Edge-triggered jump: one press, one jump (buffered by the avatar).
    pub jump_pressed: bool,
    /// Jump key currently held: releasing early cuts the jump short.
    pub jump_held: bool,
}

/// Holds a jump press from the moment the OS reports it until a simulation
/// tick consumes it.
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
/// jump per press, whatever the frames and ticks are doing.
#[derive(Debug, Default)]
pub struct JumpLatch {
    pressed: bool,
}

impl JumpLatch {
    /// Record a key-down event. Non-jump keys are ignored, and the caller is
    /// expected to have already filtered out OS key-repeat — holding the key
    /// must not re-arm the latch.
    pub fn key_down(&mut self, key: Key) {
        if JUMP_KEYS.contains(&key) {
            self.pressed = true;
        }
    }

    /// Drop a pending press. Called on scene changes so a press meant for a
    /// menu does not turn into a jump the instant gameplay resumes.
    pub fn clear(&mut self) {
        self.pressed = false;
    }

    /// Consume the latched press, if any.
    fn take(&mut self) -> bool {
        std::mem::take(&mut self.pressed)
    }
}

pub fn read(ctx: &Context, jump: &mut JumpLatch) -> PlayerInput {
    let kb = &ctx.keyboard;
    let left = kb.is_key_pressed(Key::Left) || kb.is_key_pressed(Key::A);
    let right = kb.is_key_pressed(Key::Right) || kb.is_key_pressed(Key::D);

    PlayerInput {
        left: left && !right,
        right: right && !left,
        down: kb.is_key_pressed(Key::Down) || kb.is_key_pressed(Key::S),
        jump_pressed: jump.take(),
        jump_held: JUMP_KEYS.iter().any(|&key| kb.is_key_pressed(key)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_survives_until_a_tick_consumes_it() {
        let mut latch = JumpLatch::default();
        latch.key_down(Key::Space);
        // ...however many frames render before the next fixed tick.
        assert!(latch.take(), "the press is still there when a tick runs");
        assert!(!latch.take(), "and only fires once");
    }

    #[test]
    fn every_jump_key_latches_and_other_keys_do_not() {
        for key in JUMP_KEYS {
            let mut latch = JumpLatch::default();
            latch.key_down(key);
            assert!(latch.take(), "{key:?} should jump");
        }
        let mut latch = JumpLatch::default();
        latch.key_down(Key::Left);
        latch.key_down(Key::P);
        assert!(!latch.take());
    }

    /// Two presses inside one tick are still one jump. The player cannot
    /// physically double-tap in 16 ms, but key repeat and a stalled frame
    /// both look like this, and both used to burn the double jump.
    #[test]
    fn repeated_presses_before_a_tick_collapse_to_one_jump() {
        let mut latch = JumpLatch::default();
        latch.key_down(Key::Up);
        latch.key_down(Key::Up);
        assert!(latch.take());
        assert!(!latch.take());
    }

    #[test]
    fn clearing_drops_a_pending_press() {
        let mut latch = JumpLatch::default();
        latch.key_down(Key::Up);
        latch.clear();
        assert!(!latch.take(), "a press meant for a menu is not a jump");
    }
}
