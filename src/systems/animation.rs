//! Animation: picks a clip from movement state and advances frames from
//! data-driven clip definitions. No frame indices in code, ever.

use hecs::World;

use crate::ecs::components::{AnimationState, Avatar, Body, Patrol, Sprite, Velocity};

/// Every clip [`select_avatar_clip`] can ask for. A clip set that is missing
/// one of these freezes the sprite silently, so `Sim::check_invariants` fails
/// the moment the selector reaches it — which is what keeps this list honest
/// without a test that duplicates the selector's logic.
pub const AVATAR_CLIPS: &[&str] = &[
    "idle",
    "run",
    "jump",
    "fall",
    "crouch",
    "double_jump",
    "wall_slide",
    "hurt",
];

/// Map the avatar's movement state to a clip name.
pub fn select_avatar_clip(world: &mut World) {
    for (_, (avatar, body, vel, anim)) in
        world.query_mut::<(&Avatar, &Body, &Velocity, &mut AnimationState)>()
    {
        let clip = if avatar.dead() {
            "hurt"
        } else if avatar.wall_sliding {
            "wall_slide"
        } else if !body.grounded {
            if avatar.double_jumping {
                "double_jump"
            } else if vel.0.y < 0.0 {
                "jump"
            } else {
                "fall"
            }
        } else if avatar.crouching {
            "crouch"
        } else if vel.0.x.abs() > 5.0 {
            "run"
        } else {
            "idle"
        };
        anim.switch_to(clip);
    }
}

/// Every clip [`select_patrol_clip`] can ask for. Shorter than the avatar's
/// list because a patroller has fewer states, not because its art is poorer —
/// the knight's attack, shield, roll, and death sheets are all in the repo
/// waiting for something to select them.
pub const PATROL_CLIPS: &[&str] = &["idle", "run", "jump", "fall"];

/// Map a patroller's movement state to a clip name.
pub fn select_patrol_clip(world: &mut World) {
    for (_, (_, body, vel, anim)) in
        world.query_mut::<(&Patrol, &Body, &Velocity, &mut AnimationState)>()
    {
        let clip = if !body.grounded {
            if vel.0.y < 0.0 {
                "jump"
            } else {
                "fall"
            }
        } else if vel.0.x.abs() > 5.0 {
            "run"
        } else {
            "idle"
        };
        anim.switch_to(clip);
    }
}

/// Advance every animation against its own entity's clip set.
pub fn advance(world: &mut World, dt: f32) {
    for (_, (sprite, anim)) in world.query_mut::<(&Sprite, &mut AnimationState)>() {
        let Some(clip) = sprite.clips.clip(&anim.clip) else {
            continue;
        };
        if clip.frames.is_empty() || clip.fps <= 0.0 {
            continue;
        }

        anim.elapsed += dt;
        let frame_time = 1.0 / clip.fps;
        while anim.elapsed >= frame_time {
            anim.elapsed -= frame_time;
            if anim.frame + 1 < clip.frames.len() {
                anim.frame += 1;
            } else if clip.looping {
                anim.frame = 0;
            }
        }
        anim.frame = anim.frame.min(clip.frames.len() - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{Clip, ClipSet};
    use ggez::glam::Vec2;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn clip_set() -> ClipSet {
        let mut clips = HashMap::new();
        clips.insert(
            "loop3".to_string(),
            Clip {
                frames: vec![(0, 0), (1, 0), (2, 0)],
                fps: 10.0,
                looping: true,
                sheet: None,
                frame_size: None,
            },
        );
        clips.insert(
            "once2".to_string(),
            Clip {
                frames: vec![(0, 1), (1, 1)],
                fps: 10.0,
                looping: false,
                sheet: None,
                frame_size: None,
            },
        );
        ClipSet {
            sheet: Some("test".to_string()),
            frame_size: Some((50.0, 37.0)),
            offset: None,
            clips,
        }
    }

    fn spawn_animated(world: &mut World, clip: &str) -> hecs::Entity {
        world.spawn((
            Sprite {
                clips: Arc::new(clip_set()),
                offset: Vec2::ZERO,
            },
            AnimationState::new(clip),
        ))
    }

    #[test]
    fn looping_clip_wraps() {
        let mut world = World::new();
        let e = spawn_animated(&mut world, "loop3");

        // 10 fps -> 0.1s per frame; 0.35s -> 3 advances -> frame 0 again
        advance(&mut world, 0.35);
        let anim = world.get::<&AnimationState>(e).unwrap();
        assert_eq!(anim.frame, 0);
    }

    #[test]
    fn non_looping_clip_holds_last_frame() {
        let mut world = World::new();
        let e = spawn_animated(&mut world, "once2");

        advance(&mut world, 1.0);
        let anim = world.get::<&AnimationState>(e).unwrap();
        assert_eq!(anim.frame, 1);
    }

    #[test]
    fn switching_clips_resets_frame() {
        let mut anim = AnimationState::new("loop3");
        anim.frame = 2;
        anim.switch_to("once2");
        assert_eq!(anim.frame, 0);
        // switching to the same clip is a no-op
        anim.frame = 1;
        anim.switch_to("once2");
        assert_eq!(anim.frame, 1);
    }
}
