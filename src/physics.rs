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

    /// Strict overlap: sharing an edge is *not* an intersection.
    ///
    /// This has to be strict. `resolve_move` pushes a body flush against a
    /// surface (`pos.x == solid.right()`), and a wall is stored as one rect
    /// per tile row. Under an inclusive test, a body sliding down a wall
    /// touches every rect in that column — including the ones entirely below
    /// it — and the `from_top` branch then snaps it onto a phantom ledge at
    /// each tile boundary. Grounding still works because gravity penetrates
    /// the floor by a fraction of a pixel every tick before resolution.
    pub fn overlaps(&self, other: &Aabb) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
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
    /// Landed on a full solid this resolution.
    pub on_solid: bool,
    /// Landed on a one-way platform this resolution (drop-through eligible
    /// when this is the only support).
    pub on_one_way: bool,
}

/// Resolve a body's move against static geometry, one axis at a time, using
/// the previous position to decide which side it approached from.
///
/// The two passes are not cosmetic. Resolving both axes in a single loop lets
/// a decision made at one position survive a later correction that invalidates
/// it: a body can land on a floor it overlaps by a fraction of a pixel, then
/// get pushed sideways clear of that floor and still come out `grounded`.
/// Which of those happened depended on the order of `solids`. Settling x first
/// and only then testing y means the vertical pass sees the position the body
/// actually ends the tick at.
pub fn resolve_move(
    pos: &mut Vec2,
    vel: &mut Vec2,
    prev_pos: Vec2,
    size: Vec2,
    solids: &[SolidRect],
) -> Contact {
    let mut contact = Contact::default();

    // --- horizontal: test the new x against the previous y ---
    for solid in solids {
        // One-way platforms never block horizontal motion.
        if solid.one_way {
            continue;
        }
        let body = Aabb::new(pos.x, prev_pos.y, size.x, size.y);
        if !body.overlaps(&solid.rect) {
            continue;
        }

        if prev_pos.x + size.x <= solid.rect.x {
            pos.x = solid.rect.x - size.x;
            if vel.x > 0.0 {
                vel.x = 0.0;
            }
        } else if prev_pos.x >= solid.rect.right() {
            pos.x = solid.rect.right();
            if vel.x < 0.0 {
                vel.x = 0.0;
            }
        }
    }

    // --- vertical: x is settled, so this sees the final column ---
    for solid in solids {
        let body = Aabb::new(pos.x, pos.y, size.x, size.y);
        if !body.overlaps(&solid.rect) {
            continue;
        }

        if prev_pos.y + size.y <= solid.rect.y {
            pos.y = solid.rect.y - size.y;
            vel.y = 0.0;
            contact.grounded = true;
            if solid.one_way {
                contact.on_one_way = true;
            } else {
                contact.on_solid = true;
            }
        } else if solid.one_way {
            // approached from below or the side: pass through
        } else if prev_pos.y >= solid.rect.bottom() {
            pos.y = solid.rect.bottom();
            if vel.y < 0.0 {
                vel.y = 0.0;
            }
        }
    }

    // --- depenetration fallback ---
    // Neither pass applies when the body is already inside a solid with no
    // approach direction to infer — a spawn point placed in a wall, or a
    // diagonal entry into a corner. Without this the body is trapped forever,
    // sinking a fraction of a pixel per tick. Push out along the shallowest
    // axis so nothing can be permanently stuck.
    for solid in solids {
        if solid.one_way {
            continue;
        }
        let body = Aabb::new(pos.x, pos.y, size.x, size.y);
        if !body.overlaps(&solid.rect) {
            continue;
        }

        let out_left = solid.rect.x - body.right();
        let out_right = solid.rect.right() - body.x;
        let out_up = solid.rect.y - body.bottom();
        let out_down = solid.rect.bottom() - body.y;
        let dx = shallower(out_left, out_right);
        let dy = shallower(out_up, out_down);

        if dx.abs() <= dy.abs() {
            pos.x += dx;
            vel.x = 0.0;
        } else {
            pos.y += dy;
            vel.y = 0.0;
            if dy < 0.0 {
                contact.grounded = true;
                contact.on_solid = true;
            }
        }
    }

    contact
}

/// Of two opposing escape distances, the one with the smaller magnitude.
fn shallower(a: f32, b: f32) -> f32 {
    if a.abs() <= b.abs() {
        a
    } else {
        b
    }
}

/// Is a body pressed against a full solid on the given side (`dir` -1 left,
/// +1 right)? Probes a 1px strip beside the body, inset vertically so floor
/// and ceiling contacts don't count as walls.
pub fn touching_wall(pos: Vec2, size: Vec2, solids: &[SolidRect], dir: f32) -> bool {
    let probe_x = if dir > 0.0 {
        pos.x + size.x
    } else {
        pos.x - 1.0
    };
    let probe = Aabb::new(probe_x, pos.y + 2.0, 1.0, size.y - 4.0);
    solids.iter().any(|s| !s.one_way && probe.overlaps(&s.rect))
}

/// Which side, if either, a body is pressed against a wall on: -1 left,
/// +1 right, 0 for neither.
///
/// `prefer` (-1, 0, or +1) breaks the tie when both sides touch: in a shaft
/// barely wider than the body, the wall the player is pushing into is the one
/// they mean. With no preference the right wall wins, arbitrarily but stably.
pub fn wall_contact(pos: Vec2, size: Vec2, solids: &[SolidRect], prefer: f32) -> f32 {
    let left = touching_wall(pos, size, solids, -1.0);
    let right = touching_wall(pos, size, solids, 1.0);
    match (left, right) {
        (true, true) if prefer != 0.0 => prefer.signum(),
        (true, true) => 1.0,
        (true, false) => -1.0,
        (false, true) => 1.0,
        (false, false) => 0.0,
    }
}

#[cfg(test)]
mod overlap_tests {
    use super::*;
    use ggez::glam::Vec2;

    /// Regression: a wall is stored as one rect per tile row, and a body
    /// pressed flush against it shares an edge with every rect in the column.
    /// Under an inclusive overlap test each of those seams read as a ledge,
    /// so a wall slide would silently stop in mid-air at every tile boundary.
    /// Found by tapes/wall_jump.tape.
    #[test]
    fn sliding_flush_down_a_wall_finds_no_ledge_at_tile_seams() {
        let wall: Vec<SolidRect> = (0..4)
            .map(|row| SolidRect::solid(Aabb::new(0.0, row as f32 * 32.0, 32.0, 32.0)))
            .collect();
        let size = Vec2::new(20.0, 34.0);

        // Flush against the wall's right face, falling across the seam at y=64.
        let prev = Vec2::new(32.0, 30.0);
        let mut pos = Vec2::new(32.0, 34.0);
        let mut vel = Vec2::new(0.0, 240.0);

        let contact = resolve_move(&mut pos, &mut vel, prev, size, &wall);

        assert!(
            !contact.grounded,
            "a flush wall slide must not find a ledge"
        );
        assert_eq!(pos, Vec2::new(32.0, 34.0), "and must not be displaced");
        assert_eq!(vel.y, 240.0, "and must keep falling");
    }

    /// The flip side: a real ledge must still catch a body that is genuinely
    /// above it, however slightly.
    #[test]
    fn a_real_ledge_still_catches_a_falling_body() {
        let ledge = [SolidRect::solid(Aabb::new(0.0, 64.0, 32.0, 32.0))];
        let size = Vec2::new(20.0, 34.0);

        let prev = Vec2::new(10.0, 29.0);
        let mut pos = Vec2::new(10.0, 34.0);
        let mut vel = Vec2::new(0.0, 240.0);

        let contact = resolve_move(&mut pos, &mut vel, prev, size, &ledge);

        assert!(contact.grounded);
        assert_eq!(pos.y, 64.0 - 34.0);
        assert_eq!(vel.y, 0.0);
    }
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
    fn contact_distinguishes_solid_from_one_way() {
        let size = Vec2::new(10.0, 10.0);

        let mut pos = Vec2::new(20.0, 45.0);
        let mut vel = Vec2::new(0.0, 30.0);
        let c = resolve_move(
            &mut pos,
            &mut vel,
            Vec2::new(20.0, 30.0),
            size,
            &[SolidRect::one_way(Aabb::new(0.0, 50.0, 100.0, 8.0))],
        );
        assert!(c.on_one_way && !c.on_solid);

        let mut pos = Vec2::new(20.0, 45.0);
        let mut vel = Vec2::new(0.0, 30.0);
        let c = resolve_move(
            &mut pos,
            &mut vel,
            Vec2::new(20.0, 30.0),
            size,
            &[solid(0.0, 50.0, 100.0, 8.0)],
        );
        assert!(c.on_solid && !c.on_one_way);
    }

    #[test]
    fn wall_probe_detects_solids_but_not_platforms_or_floor() {
        let size = Vec2::new(10.0, 30.0);
        let pos = Vec2::new(50.0, 100.0); // body occupies x 50-60, y 100-130
        let wall_right = [solid(60.0, 80.0, 20.0, 100.0)];
        let platform_right = [SolidRect::one_way(Aabb::new(60.0, 80.0, 20.0, 100.0))];
        let floor_below = [solid(0.0, 130.0, 200.0, 20.0)];

        assert!(touching_wall(pos, size, &wall_right, 1.0));
        assert!(!touching_wall(pos, size, &wall_right, -1.0)); // wrong side
        assert!(!touching_wall(pos, size, &platform_right, 1.0)); // one-way
        assert!(!touching_wall(pos, size, &floor_below, 1.0)); // floor is not a wall
        assert!(!touching_wall(pos, size, &floor_below, -1.0));
    }

    #[test]
    fn wall_contact_reports_the_side_and_honors_the_preference() {
        let size = Vec2::new(20.0, 30.0); // body occupies x 50-70
        let pos = Vec2::new(50.0, 100.0);
        let right = [solid(70.0, 80.0, 20.0, 100.0)];
        let left = [solid(30.0, 80.0, 20.0, 100.0)];
        let shaft = [
            solid(70.0, 80.0, 20.0, 100.0),
            solid(30.0, 80.0, 20.0, 100.0),
        ];

        assert_eq!(wall_contact(pos, size, &right, 0.0), 1.0);
        assert_eq!(wall_contact(pos, size, &left, 0.0), -1.0);
        assert_eq!(wall_contact(pos, size, &[], 0.0), 0.0);
        // pressing into one wall of a tight shaft picks that wall
        assert_eq!(wall_contact(pos, size, &shaft, -1.0), -1.0);
        assert_eq!(wall_contact(pos, size, &shaft, 1.0), 1.0);
        // a preference for a side that is not there does not invent contact
        assert_eq!(wall_contact(pos, size, &right, -1.0), 1.0);
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
