//! Translates raw keyboard state into gameplay intent. Gameplay systems only
//! ever see `PlayerInput`, never the keyboard.

use ggez::winit::event::VirtualKeyCode;
use ggez::Context;

#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerInput {
    pub left: bool,
    pub right: bool,
    pub jump: bool,
}

pub fn read(ctx: &Context) -> PlayerInput {
    let kb = &ctx.keyboard;
    PlayerInput {
        left: kb.is_key_pressed(VirtualKeyCode::Left) && !kb.is_key_pressed(VirtualKeyCode::Right),
        right: kb.is_key_pressed(VirtualKeyCode::Right) && !kb.is_key_pressed(VirtualKeyCode::Left),
        jump: kb.is_key_pressed(VirtualKeyCode::Up) && !kb.is_key_pressed(VirtualKeyCode::Down),
    }
}
