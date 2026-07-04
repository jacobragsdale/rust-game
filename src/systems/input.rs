//! Translates raw keyboard state into gameplay intent. Gameplay systems only
//! ever see `PlayerInput`, never the keyboard. Arrows and WASD both work.

use ggez::winit::event::VirtualKeyCode as Key;
use ggez::Context;

#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerInput {
    pub left: bool,
    pub right: bool,
    /// Edge-triggered jump: one press, one jump.
    pub jump_pressed: bool,
}

pub fn read(ctx: &Context) -> PlayerInput {
    let kb = &ctx.keyboard;
    let left = kb.is_key_pressed(Key::Left) || kb.is_key_pressed(Key::A);
    let right = kb.is_key_pressed(Key::Right) || kb.is_key_pressed(Key::D);

    PlayerInput {
        left: left && !right,
        right: right && !left,
        jump_pressed: kb.is_key_just_pressed(Key::Up)
            || kb.is_key_just_pressed(Key::W)
            || kb.is_key_just_pressed(Key::Space),
    }
}
