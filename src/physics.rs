//! Collision primitives, independent of ggez so they can be unit tested.

use ggez::glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Aabb {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Aabb { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// Inclusive overlap test.
    pub fn overlaps(&self, other: &Aabb) -> bool {
        self.x <= other.right()
            && self.right() >= other.x
            && self.y <= other.bottom()
            && self.bottom() >= other.y
    }
}

/// A solid rectangle a body can collide with. `one_way` platforms only
/// collide from above: land on them, pass through from below or the sides.
#[derive(Clone, Copy, Debug)]
pub struct SolidRect {
    pub rect: Aabb,
    pub one_way: bool,
}

impl SolidRect {
    pub fn solid(rect: Aabb) -> Self {
        SolidRect {
            rect,
            one_way: false,
        }
    }

    pub fn one_way(rect: Aabb) -> Self {
        SolidRect {
            rect,
            one_way: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Contact {
    pub grounded: bool,
}

/// Swept AABB resolution: uses the previous position to decide which side the
/// body approached from, then pushes it out and kills velocity on that axis.
pub fn resolve_move(
    pos: &mut Vec2,
    vel: &mut Vec2,
    prev_pos: Vec2,
    size: Vec2,
    solids: &[SolidRect],
) -> Contact {
    let prev = Aabb::new(prev_pos.x, prev_pos.y, size.x, size.y);
    let mut contact = Contact::default();

    for solid in solids {
        let current = Aabb::new(pos.x, pos.y, size.x, size.y);
        if !current.overlaps(&solid.rect) {
            continue;
        }

        let from_top = prev.bottom() <= solid.rect.y;
        let from_bottom = prev.y >= solid.rect.bottom();
        let from_left = prev.right() <= solid.rect.x;
        let from_right = prev.x >= solid.rect.right();

        if from_top {
            pos.y = solid.rect.y - size.y;
            vel.y = 0.0;
            contact.grounded = true;
        } else if solid.one_way {
            // approached from below or the side: pass through
        } else if from_bottom {
            pos.y = solid.rect.bottom();
            if vel.y < 0.0 {
                vel.y = 0.0;
            }
        } else if from_left {
            pos.x = solid.rect.x - size.x;
            if vel.x > 0.0 {
                vel.x = 0.0;
            }
        } else if from_right {
            pos.x = solid.rect.right();
            if vel.x < 0.0 {
                vel.x = 0.0;
            }
        }
    }

    contact
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(x: f32, y: f32, w: f32, h: f32) -> SolidRect {
        SolidRect::solid(Aabb::new(x, y, w, h))
    }

    #[test]
    fn lands_on_platform_from_above() {
        let prev = Vec2::new(0.0, 40.0);
        let mut pos = Vec2::new(0.0, 60.0); // fell into the platform
        let mut vel = Vec2::new(0.0, 20.0);
        let size = Vec2::new(10.0, 10.0);
        let solids = [solid(-50.0, 55.0, 200.0, 30.0)];

        let contact = resolve_move(&mut pos, &mut vel, prev, size, &solids);

        assert!(contact.grounded);
        assert_eq!(pos.y, 45.0); // snapped on top
        assert_eq!(vel.y, 0.0);
    }

    #[test]
    fn bumps_head_from_below() {
        let prev = Vec2::new(0.0, 100.0);
        let mut pos = Vec2::new(0.0, 85.0); // jumped up into the platform
        let mut vel = Vec2::new(0.0, -30.0);
        let size = Vec2::new(10.0, 10.0);
        let solids = [solid(-50.0, 60.0, 200.0, 30.0)];

        let contact = resolve_move(&mut pos, &mut vel, prev, size, &solids);

        assert!(!contact.grounded);
        assert_eq!(pos.y, 90.0); // pushed back below the platform
        assert_eq!(vel.y, 0.0); // upward velocity killed
    }

    #[test]
    fn pushed_out_when_hitting_side() {
        let prev = Vec2::new(30.0, 0.0);
        let mut pos = Vec2::new(45.0, 0.0); // ran into the left face
        let mut vel = Vec2::new(15.0, 0.0);
        let size = Vec2::new(10.0, 10.0);
        let solids = [solid(50.0, -50.0, 30.0, 100.0)];

        let contact = resolve_move(&mut pos, &mut vel, prev, size, &solids);

        assert!(!contact.grounded);
        assert_eq!(pos.x, 40.0);
        assert_eq!(vel.x, 0.0);
    }

    #[test]
    fn one_way_platform_lands_from_above_but_passes_from_below() {
        let size = Vec2::new(10.0, 10.0);
        let platform = SolidRect::one_way(Aabb::new(0.0, 50.0, 100.0, 8.0));

        // falling onto it: lands
        let mut pos = Vec2::new(20.0, 45.0);
        let mut vel = Vec2::new(0.0, 30.0);
        let contact = resolve_move(&mut pos, &mut vel, Vec2::new(20.0, 30.0), size, &[platform]);
        assert!(contact.grounded);
        assert_eq!(pos.y, 40.0);

        // jumping up through it: passes clean
        let mut pos = Vec2::new(20.0, 50.0);
        let mut vel = Vec2::new(0.0, -30.0);
        let contact = resolve_move(&mut pos, &mut vel, Vec2::new(20.0, 62.0), size, &[platform]);
        assert!(!contact.grounded);
        assert_eq!(pos.y, 50.0); // untouched
        assert_eq!(vel.y, -30.0);
    }

    #[test]
    fn no_contact_when_clear() {
        let prev = Vec2::new(0.0, 0.0);
        let mut pos = Vec2::new(5.0, 0.0);
        let mut vel = Vec2::new(5.0, 0.0);
        let size = Vec2::new(10.0, 10.0);
        let solids = [solid(100.0, 100.0, 30.0, 30.0)];

        let contact = resolve_move(&mut pos, &mut vel, prev, size, &solids);

        assert!(!contact.grounded);
        assert_eq!(pos, Vec2::new(5.0, 0.0));
        assert_eq!(vel.x, 5.0);
    }
}
