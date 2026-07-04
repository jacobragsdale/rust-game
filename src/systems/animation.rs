//! Animation: picks a clip from movement state and advances frames from
//! data-driven clip definitions. No frame indices in code, ever.

use hecs::World;

use crate::assets::ClipSet;
use crate::ecs::components::{AnimationState, Avatar, Velocity};

/// Map the avatar's movement state to a clip name.
pub fn select_avatar_clip(world: &mut World) {
    for (_, (avatar, vel, anim)) in world.query_mut::<(&Avatar, &Velocity, &mut AnimationState)>() {
        let clip = if avatar.dead() {
            "hurt"
        } else if avatar.wall_sliding {
            "wall_slide"
        } else if !avatar.grounded {
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

/// Advance every animation against its clip set.
pub fn advance(world: &mut World, clips: &ClipSet, dt: f32) {
    for (_, anim) in world.query_mut::<&mut AnimationState>() {
        let Some(clip) = clips.clip(&anim.clip) else {
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
    use crate::assets::Clip;
    use std::collections::HashMap;

    fn clip_set() -> ClipSet {
        let mut clips = HashMap::new();
        clips.insert(
            "loop3".to_string(),
            Clip {
                frames: vec![(0, 0), (1, 0), (2, 0)],
                fps: 10.0,
                looping: true,
            },
        );
        clips.insert(
            "once2".to_string(),
            Clip {
                frames: vec![(0, 1), (1, 1)],
                fps: 10.0,
                looping: false,
            },
        );
        ClipSet {
            sheet: "test".to_string(),
            frame_size: (50.0, 37.0),
            clips,
        }
    }

    #[test]
    fn looping_clip_wraps() {
        let clips = clip_set();
        let mut world = World::new();
        let e = world.spawn((AnimationState::new("loop3"),));

        // 10 fps -> 0.1s per frame; 0.35s -> 3 advances -> frame 0 again
        advance(&mut world, &clips, 0.35);
        let anim = world.get::<&AnimationState>(e).unwrap();
        assert_eq!(anim.frame, 0);
    }

    #[test]
    fn non_looping_clip_holds_last_frame() {
        let clips = clip_set();
        let mut world = World::new();
        let e = world.spawn((AnimationState::new("once2"),));

        advance(&mut world, &clips, 1.0);
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
