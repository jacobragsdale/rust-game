//! Property sweeps over the collision system.
//!
//! These are not example-based tests. Each one asserts a property that must
//! hold across a swept range of thicknesses, speeds, positions, and geometry
//! orderings — the kinds of bug that a hand-picked case walks straight past.
//!
//! Randomized cases use a fixed-seed LCG rather than `rand`, so a failure is
//! always reproducible and the suite cannot go flaky.

use ggez::glam::Vec2;

use supergame::ecs::components::Avatar;
use supergame::physics::{resolve_move, touching_wall, Aabb, SolidRect};
use supergame::sim::TICK;

const SIZE: Vec2 = Vec2::new(Avatar::WIDTH, Avatar::HEIGHT);

/// Deterministic stand-in for a random number generator.
struct Lcg(u64);

impl Lcg {
    fn new() -> Self {
        Lcg(0x2545_F491_4F6C_DD1D)
    }

    fn f32_in(&mut self, lo: f32, hi: f32) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let unit = ((self.0 >> 40) as f32) / ((1u64 << 24) as f32);
        lo + unit * (hi - lo)
    }
}

/// Advance one tick and resolve, the way `avatar::update` does.
fn step(pos: &mut Vec2, vel: &mut Vec2, solids: &[SolidRect]) -> supergame::physics::Contact {
    let prev = *pos;
    *pos += *vel * TICK;
    resolve_move(pos, vel, prev, SIZE, solids)
}

fn overlaps_any_solid(pos: Vec2, solids: &[SolidRect]) -> Option<Aabb> {
    let body = Aabb::new(pos.x, pos.y, SIZE.x, SIZE.y);
    solids
        .iter()
        .find(|s| !s.one_way && body.overlaps(&s.rect))
        .map(|s| s.rect)
}

// ---------------------------------------------------------------------------
// 1. Tunneling
// ---------------------------------------------------------------------------

/// Drop a body onto a horizontal strip at a fixed speed and report whether it
/// ended up underneath without ever making contact.
fn tunnels_through(thickness: f32, speed: f32, one_way: bool) -> bool {
    let rect = Aabb::new(0.0, 400.0, 400.0, thickness);
    let strip = [if one_way {
        SolidRect::one_way(rect)
    } else {
        SolidRect::solid(rect)
    }];

    let mut pos = Vec2::new(100.0, 300.0);
    let mut vel = Vec2::new(0.0, speed);

    for _ in 0..600 {
        if step(&mut pos, &mut vel, &strip).grounded {
            return false;
        }
        if pos.y > 400.0 + thickness {
            return true;
        }
    }
    false
}

#[test]
fn fast_falls_never_tunnel_through_solid_ground() {
    let mut escaped = Vec::new();
    for thickness in [4.0, 8.0, 16.0, 32.0] {
        for speed_step in 1..=20 {
            let speed = speed_step as f32 * 60.0;
            if tunnels_through(thickness, speed, false) {
                escaped.push(format!("{thickness}px solid at {speed}px/s"));
            }
        }
    }
    assert!(
        escaped.is_empty(),
        "body passed through solid ground without contact:\n  {}",
        escaped.join("\n  ")
    );
}

#[test]
fn fast_falls_never_tunnel_through_one_way_platforms() {
    let mut escaped = Vec::new();
    for thickness in [4.0, 8.0, 16.0, 32.0] {
        for speed_step in 1..=20 {
            let speed = speed_step as f32 * 60.0;
            if tunnels_through(thickness, speed, true) {
                escaped.push(format!("{thickness}px platform at {speed}px/s"));
            }
        }
    }
    assert!(
        escaped.is_empty(),
        "body passed through a one-way platform without landing:\n  {}",
        escaped.join("\n  ")
    );
}

/// The speeds that matter are the ones the avatar can actually reach.
#[test]
fn a_platform_catches_a_body_falling_at_terminal_velocity() {
    let platform = [SolidRect::one_way(Aabb::new(0.0, 400.0, 400.0, 8.0))];
    let mut pos = Vec2::new(100.0, 300.0);
    let mut vel = Vec2::new(0.0, Avatar::MAX_FALL);

    let mut landed = false;
    for _ in 0..120 {
        if step(&mut pos, &mut vel, &platform).grounded {
            landed = true;
            break;
        }
    }

    assert!(landed, "an 8px platform must catch a 900px/s fall");
    assert_eq!(pos.y, 400.0 - SIZE.y);
}

#[test]
fn ceilings_stop_a_fast_rising_body() {
    let ceiling = [SolidRect::solid(Aabb::new(0.0, 100.0, 400.0, 8.0))];
    let mut pos = Vec2::new(100.0, 300.0);
    let mut vel = Vec2::new(0.0, -Avatar::JUMP_SPEED);

    for _ in 0..120 {
        step(&mut pos, &mut vel, &ceiling);
        if vel.y >= 0.0 {
            break;
        }
    }

    assert!(
        pos.y >= 108.0,
        "body rose past the ceiling to y={} (expected >= 108)",
        pos.y
    );
    assert_eq!(vel.y, 0.0, "upward velocity must be killed by the ceiling");
}

// ---------------------------------------------------------------------------
// 2-4. Resolution invariants
// ---------------------------------------------------------------------------

/// A cluster of solids that produces corners, seams, and narrow gaps.
fn test_geometry() -> Vec<SolidRect> {
    let mut solids: Vec<SolidRect> = Vec::new();
    // floor, as per-row rects the way merge_runs emits them
    for row in 0..3 {
        solids.push(SolidRect::solid(Aabb::new(
            0.0,
            400.0 + row as f32 * 32.0,
            320.0,
            32.0,
        )));
    }
    // a wall rising from the floor, also per-row
    for row in 0..5 {
        solids.push(SolidRect::solid(Aabb::new(
            288.0,
            240.0 + row as f32 * 32.0,
            32.0,
            32.0,
        )));
    }
    // a ceiling overhang and a one-way ledge
    solids.push(SolidRect::solid(Aabb::new(0.0, 200.0, 160.0, 32.0)));
    solids.push(SolidRect::one_way(Aabb::new(96.0, 336.0, 128.0, 8.0)));
    solids
}

#[test]
fn resolution_never_leaves_a_body_inside_a_solid() {
    let solids = test_geometry();
    let mut rng = Lcg::new();
    let mut stuck = Vec::new();

    for _ in 0..20_000 {
        let mut pos = Vec2::new(rng.f32_in(-20.0, 340.0), rng.f32_in(180.0, 420.0));
        let mut vel = Vec2::new(rng.f32_in(-300.0, 300.0), rng.f32_in(-600.0, 900.0));

        // Skip starts that are already inside geometry; that is property 8.
        if overlaps_any_solid(pos, &solids).is_some() {
            continue;
        }

        let start = pos;
        step(&mut pos, &mut vel, &solids);

        if let Some(rect) = overlaps_any_solid(pos, &solids) {
            stuck.push(format!(
                "from {start:?} vel {vel:?} -> {pos:?} inside {rect:?}"
            ));
        }
    }

    assert!(
        stuck.is_empty(),
        "{} resolutions left the body inside a solid, e.g.:\n  {}",
        stuck.len(),
        stuck.iter().take(5).cloned().collect::<Vec<_>>().join("\n  ")
    );
}

#[test]
fn resolution_is_independent_of_solid_order() {
    let solids = test_geometry();
    let reversed: Vec<SolidRect> = solids.iter().copied().rev().collect();
    let mut rng = Lcg::new();
    let mut divergent = Vec::new();

    for _ in 0..20_000 {
        let pos = Vec2::new(rng.f32_in(-20.0, 340.0), rng.f32_in(180.0, 420.0));
        let vel = Vec2::new(rng.f32_in(-300.0, 300.0), rng.f32_in(-600.0, 900.0));
        if overlaps_any_solid(pos, &solids).is_some() {
            continue;
        }

        let (mut pos_a, mut vel_a) = (pos, vel);
        step(&mut pos_a, &mut vel_a, &solids);
        let (mut pos_b, mut vel_b) = (pos, vel);
        step(&mut pos_b, &mut vel_b, &reversed);

        if pos_a != pos_b || vel_a != vel_b {
            divergent.push(format!(
                "from {pos:?} vel {vel:?}: {pos_a:?}/{vel_a:?} vs {pos_b:?}/{vel_b:?}"
            ));
        }
    }

    assert!(
        divergent.is_empty(),
        "{} resolutions depended on solid ordering, e.g.:\n  {}",
        divergent.len(),
        divergent
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn resolution_is_left_right_symmetric() {
    /// Mirror the world about x = 0, keeping rects anchored at their top-left.
    fn mirror(solid: &SolidRect) -> SolidRect {
        let r = Aabb::new(-solid.rect.right(), solid.rect.y, solid.rect.w, solid.rect.h);
        if solid.one_way {
            SolidRect::one_way(r)
        } else {
            SolidRect::solid(r)
        }
    }

    let solids = test_geometry();
    let mirrored: Vec<SolidRect> = solids.iter().map(mirror).collect();
    let mut rng = Lcg::new();
    let mut asymmetric = Vec::new();

    for _ in 0..20_000 {
        let pos = Vec2::new(rng.f32_in(-20.0, 340.0), rng.f32_in(180.0, 420.0));
        let vel = Vec2::new(rng.f32_in(-300.0, 300.0), rng.f32_in(-600.0, 900.0));
        if overlaps_any_solid(pos, &solids).is_some() {
            continue;
        }

        let (mut pos_a, mut vel_a) = (pos, vel);
        step(&mut pos_a, &mut vel_a, &solids);

        let (mut pos_b, mut vel_b) = (Vec2::new(-pos.x - SIZE.x, pos.y), Vec2::new(-vel.x, vel.y));
        step(&mut pos_b, &mut vel_b, &mirrored);

        let unmirrored_x = -pos_b.x - SIZE.x;
        if (unmirrored_x - pos_a.x).abs() > 1e-3
            || (pos_b.y - pos_a.y).abs() > 1e-3
            || (-vel_b.x - vel_a.x).abs() > 1e-3
        {
            asymmetric.push(format!(
                "from {pos:?} vel {vel:?}: x {} vs mirrored {}",
                pos_a.x, unmirrored_x
            ));
        }
    }

    assert!(
        asymmetric.is_empty(),
        "{} resolutions were not left/right symmetric, e.g.:\n  {}",
        asymmetric.len(),
        asymmetric
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// 5. Stability
// ---------------------------------------------------------------------------

#[test]
fn a_resting_body_never_drifts() {
    let solids = test_geometry();
    let mut pos = Vec2::new(100.0, 400.0 - SIZE.y);
    let mut vel = Vec2::ZERO;

    // settle
    for _ in 0..10 {
        vel.y = (vel.y + Avatar::GRAVITY * TICK).min(Avatar::MAX_FALL);
        step(&mut pos, &mut vel, &solids);
    }
    let settled = pos;

    for tick in 0..600 {
        vel.y = (vel.y + Avatar::GRAVITY * TICK).min(Avatar::MAX_FALL);
        let contact = step(&mut pos, &mut vel, &solids);
        assert!(contact.grounded, "lost contact with the floor at tick {tick}");
        assert_eq!(pos, settled, "body drifted at tick {tick}");
    }
}

// ---------------------------------------------------------------------------
// 6. One-way semantics
// ---------------------------------------------------------------------------

#[test]
fn one_way_platforms_are_solid_only_from_above() {
    let platform = [SolidRect::one_way(Aabb::new(100.0, 300.0, 128.0, 8.0))];

    // rising from below: passes through
    let mut pos = Vec2::new(150.0, 360.0);
    let mut vel = Vec2::new(0.0, -400.0);
    for _ in 0..30 {
        let contact = step(&mut pos, &mut vel, &platform);
        assert!(!contact.grounded, "rose into a one-way platform and stuck");
    }
    assert!(pos.y < 300.0, "should have risen past the platform");

    // moving horizontally at platform height: passes through
    let mut pos = Vec2::new(40.0, 290.0);
    let mut vel = Vec2::new(300.0, 0.0);
    for _ in 0..60 {
        step(&mut pos, &mut vel, &platform);
    }
    // 60 ticks at 300px/s covers 300px, clearing the platform's far edge.
    assert!(pos.x > 300.0, "horizontal motion was blocked by a one-way");
    assert_eq!(vel.x, 300.0, "horizontal velocity was killed by a one-way");

    // falling from above: lands
    let mut pos = Vec2::new(150.0, 200.0);
    let mut vel = Vec2::new(0.0, 300.0);
    let mut landed = false;
    for _ in 0..60 {
        if step(&mut pos, &mut vel, &platform).grounded {
            landed = true;
            break;
        }
    }
    assert!(landed, "failed to land on a one-way from above");
    assert_eq!(pos.y, 300.0 - SIZE.y);
}

// ---------------------------------------------------------------------------
// 7. Wall probe
// ---------------------------------------------------------------------------

#[test]
fn touching_wall_requires_contact_and_ignores_one_ways() {
    let wall = [SolidRect::solid(Aabb::new(200.0, 100.0, 32.0, 200.0))];
    let y = 150.0;

    let flush = Vec2::new(200.0 - SIZE.x, y);
    assert!(touching_wall(flush, SIZE, &wall, 1.0), "flush contact");
    assert!(
        !touching_wall(flush, SIZE, &wall, -1.0),
        "wall is on the right, not the left"
    );

    let apart = Vec2::new(200.0 - SIZE.x - 2.0, y);
    assert!(
        !touching_wall(apart, SIZE, &wall, 1.0),
        "2px away is not touching"
    );

    let one_way = [SolidRect::one_way(Aabb::new(200.0, 100.0, 32.0, 200.0))];
    assert!(
        !touching_wall(flush, SIZE, &one_way, 1.0),
        "one-way platforms are never walls"
    );

    // a wall shorter than the probe's vertical inset must still register
    let stub = [SolidRect::solid(Aabb::new(200.0, y + 10.0, 32.0, 6.0))];
    assert!(
        touching_wall(flush, SIZE, &stub, 1.0),
        "a short wall segment beside the body still counts"
    );
}

// ---------------------------------------------------------------------------
// 8. Escaping geometry
// ---------------------------------------------------------------------------

#[test]
fn a_body_inside_a_solid_is_pushed_out() {
    let solids = [SolidRect::solid(Aabb::new(100.0, 300.0, 128.0, 64.0))];
    let mut escaped = Vec::new();

    // Nudge the body from every point well inside the block.
    for dx in 0..8 {
        for dy in 0..4 {
            let start = Vec2::new(100.0 + dx as f32 * 13.0, 300.0 + dy as f32 * 7.0);
            let mut pos = start;
            let mut vel = Vec2::new(0.0, Avatar::GRAVITY * TICK);

            for _ in 0..120 {
                step(&mut pos, &mut vel, &solids);
                if overlaps_any_solid(pos, &solids).is_none() {
                    break;
                }
            }
            if overlaps_any_solid(pos, &solids).is_some() {
                escaped.push(format!("{start:?} stayed stuck at {pos:?}"));
            }
        }
    }

    assert!(
        escaped.is_empty(),
        "{} starts never escaped the solid, e.g.:\n  {}",
        escaped.len(),
        escaped.iter().take(5).cloned().collect::<Vec<_>>().join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// 9. Seams
// ---------------------------------------------------------------------------

#[test]
fn landing_across_a_seam_between_two_floor_rects_is_stable() {
    let floor = [
        SolidRect::solid(Aabb::new(0.0, 400.0, 32.0, 32.0)),
        SolidRect::solid(Aabb::new(32.0, 400.0, 32.0, 32.0)),
    ];

    // straddle the seam at x = 32
    let mut pos = Vec2::new(22.0, 300.0);
    let mut vel = Vec2::new(0.0, 400.0);

    let mut landed = false;
    for _ in 0..60 {
        if step(&mut pos, &mut vel, &floor).grounded {
            landed = true;
            break;
        }
    }
    assert!(landed, "failed to land while straddling a seam");
    assert_eq!(pos.y, 400.0 - SIZE.y);
    assert_eq!(pos.x, 22.0, "landing must not shove the body sideways");
}
