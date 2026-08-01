//! Property sweeps over the collision system.
//!
//! These are not example-based tests. Each one asserts a property that must
//! hold across a swept range of thicknesses, speeds, positions, and geometry
//! orderings — the kinds of bug that a hand-picked case walks straight past.
//!
//! Randomized cases use a fixed-seed LCG rather than `rand`, so a failure is
//! always reproducible and the suite cannot go flaky.

use std::sync::{Arc, LazyLock};

use ggez::glam::Vec2;
use hecs::World;

use supergame::assets::{StatBlock, StatTable};
use supergame::ecs::components::{Body, Collider, ColliderKind, Mover, Position, Size, Velocity};
use supergame::physics::{
    resolve_move, touching_wall, Aabb, Geometry, HazardQuery, SolidQuery, SolidRect,
};
use supergame::sim::TICK;
use supergame::systems::{body, mover};

/// The player's real numbers, from `assets/data/stats.ron`.
///
/// A `LazyLock` rather than a `const`: these used to be `Avatar::WIDTH` and
/// friends, five compile-time copies of five RON values that a mirror test had
/// to keep honest (ticket H-3b). Reading the shipped table once, here, deletes
/// the copies — the sweeps below sweep the collision system at the fall and
/// jump speeds the shipped player actually has, and a tuning pass moves one
/// file rather than two.
static PLAYER: LazyLock<Arc<StatBlock>> = LazyLock::new(|| {
    StatTable::shipped()
        .get("player")
        .expect("`player` has a stat block")
});

/// The player's collider box.
static SIZE: LazyLock<Vec2> = LazyLock::new(|| PLAYER.size());

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
fn step<Q: SolidQuery + ?Sized>(
    pos: &mut Vec2,
    vel: &mut Vec2,
    solids: &Q,
) -> supergame::physics::Contact {
    let prev = *pos;
    *pos += *vel * TICK;
    resolve_move(pos, vel, prev, *SIZE, solids)
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
    let mut vel = Vec2::new(0.0, PLAYER.max_fall);

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
    let mut vel = Vec2::new(0.0, -PLAYER.avatar().jump_speed);

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
        stuck
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// The same world, arranged every way it can reach the physics.
///
/// A `Geometry` splits its rects into the level's own and the ones entities
/// contribute, and the entity-owned half comes out of a hecs query — whose
/// order reshuffles the moment anything adds a component to an entity.
/// `body::rebuild_geometry` sorts by entity id, but a sort only helps if
/// resolution does not care about the order in the first place, which is what
/// these arrangements pin down.
fn arrangements(solids: &[SolidRect]) -> Vec<(&'static str, Box<dyn SolidQuery>)> {
    let split = |at: usize, reverse_owned: bool| -> Box<dyn SolidQuery> {
        let mut geometry = Geometry::from_rects(solids[..at].to_vec());
        let mut owned: Vec<SolidRect> = solids[at..].to_vec();
        if reverse_owned {
            owned.reverse();
        }
        geometry.set_entity_rects(owned);
        Box::new(geometry)
    };
    let mid = solids.len() / 2;
    vec![
        ("reversed slice", Box::new(reversed(solids))),
        ("grid, all static", split(solids.len(), false)),
        ("grid, all entity-owned", split(0, false)),
        ("grid, half entity-owned", split(mid, false)),
        ("grid, entity-owned reversed", split(mid, true)),
        ("grid, every rect entity-owned, reversed", split(0, true)),
    ]
}

fn reversed(solids: &[SolidRect]) -> Vec<SolidRect> {
    solids.iter().copied().rev().collect()
}

#[test]
fn resolution_is_independent_of_solid_order() {
    let solids = test_geometry();
    let arrangements = arrangements(&solids);
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

        for (label, arrangement) in &arrangements {
            let (mut pos_b, mut vel_b) = (pos, vel);
            step(&mut pos_b, &mut vel_b, &**arrangement);
            if pos_a != pos_b || vel_a != vel_b {
                divergent.push(format!(
                    "{label}: from {pos:?} vel {vel:?}: {pos_a:?}/{vel_a:?} vs {pos_b:?}/{vel_b:?}"
                ));
            }
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

/// The grid is a broadphase, not a physics change.
///
/// It may only ever hand back a subset of the same rects in the same relative
/// order, with the omitted ones provably unable to overlap the body — so a
/// `Geometry` has to resolve *bit*-identically to a flat scan, not merely
/// close enough. Starts inside solids are included on purpose: they are the
/// case that reaches the depenetration fallback, which is the one pass the
/// swept query box does not bound.
#[test]
fn the_grid_resolves_identically_to_a_linear_scan() {
    let solids = test_geometry();
    let grid = Geometry::from_rects(solids.clone());
    let mut rng = Lcg::new();
    let mut divergent = Vec::new();

    for _ in 0..40_000 {
        let pos = Vec2::new(rng.f32_in(-40.0, 360.0), rng.f32_in(160.0, 440.0));
        let vel = Vec2::new(rng.f32_in(-900.0, 900.0), rng.f32_in(-900.0, 900.0));

        let (mut pos_a, mut vel_a) = (pos, vel);
        let contact_a = step(&mut pos_a, &mut vel_a, &solids);
        let (mut pos_b, mut vel_b) = (pos, vel);
        let contact_b = step(&mut pos_b, &mut vel_b, &grid);

        if pos_a != pos_b
            || vel_a != vel_b
            || contact_a.grounded != contact_b.grounded
            || contact_a.on_solid != contact_b.on_solid
            || contact_a.on_one_way != contact_b.on_one_way
        {
            divergent.push(format!(
                "from {pos:?} vel {vel:?}: scan {pos_a:?}/{vel_a:?} vs grid {pos_b:?}/{vel_b:?}"
            ));
        }
    }

    assert!(
        divergent.is_empty(),
        "{} resolutions differed between the grid and a linear scan, e.g.:\n  {}",
        divergent.len(),
        divergent
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// The wall probes go through the same broadphase, so they get the same
/// treatment: every probe position must agree with a flat scan.
#[test]
fn the_grid_probes_walls_identically_to_a_linear_scan() {
    let solids = test_geometry();
    let grid = Geometry::from_rects(solids.clone());
    let mut rng = Lcg::new();

    for _ in 0..40_000 {
        let pos = Vec2::new(rng.f32_in(-40.0, 360.0), rng.f32_in(160.0, 440.0));
        for dir in [-1.0, 1.0] {
            assert_eq!(
                touching_wall(pos, *SIZE, &solids, dir),
                touching_wall(pos, *SIZE, &grid, dir),
                "wall probe at {pos:?} dir {dir} disagreed with a linear scan"
            );
        }
    }
}

/// What the grid is for: on a map-sized world, a body's query touches a
/// handful of rects rather than all of them.
///
/// Thirty bodies is the load the roadmap has in mind, and the assertion is a
/// ratio rather than a time so it cannot go flaky on a busy machine. The time
/// was measured separately: 600 ticks of thirty bodies falling and running
/// across this same 450-rect world, release build, resolving through a
/// `Geometry` versus through a flat `Vec<SolidRect>` of the same rects —
/// **~3.5 ms against ~19 ms, a 5x saving**, and it widens as the map does,
/// since the scan's cost is the whole map and the grid's is a body's
/// neighbourhood.
#[test]
fn the_grid_narrows_the_candidate_set_for_a_map_sized_world() {
    // A 200x60-tile world: a floor, a ceiling, and a pillar every eight tiles,
    // stored the way `merge_runs` emits geometry — one rect per tile row.
    let mut rects: Vec<SolidRect> = Vec::new();
    for x in 0..200 {
        rects.push(SolidRect::solid(Aabb::new(
            x as f32 * 32.0,
            60.0 * 32.0,
            32.0,
            32.0,
        )));
    }
    for x in (0..200).step_by(8) {
        for row in 0..10 {
            rects.push(SolidRect::solid(Aabb::new(
                x as f32 * 32.0,
                (50 + row) as f32 * 32.0,
                32.0,
                32.0,
            )));
        }
    }
    let total = rects.len();
    let grid = Geometry::from_rects(rects);

    // Thirty bodies spread across the map, each falling at full tilt.
    let mut examined = 0usize;
    let mut candidates = Vec::new();
    for i in 0..30 {
        let pos = Vec2::new(i as f32 * 210.0, 1500.0);
        let swept = Aabb::new(pos.x, pos.y, SIZE.x, SIZE.y).union(&Aabb::new(
            pos.x,
            pos.y + PLAYER.max_fall * TICK,
            SIZE.x,
            SIZE.y,
        ));
        grid.overlapping(swept, &mut candidates);
        examined += candidates.len();
    }

    let scanned = total * 30;
    assert!(
        examined * 20 < scanned,
        "grid examined {examined} rects where a scan would touch {scanned}; \
         expected at least a 20x saving"
    );
}

#[test]
fn resolution_is_left_right_symmetric() {
    /// Mirror the world about x = 0, keeping rects anchored at their top-left.
    fn mirror(solid: &SolidRect) -> SolidRect {
        let r = Aabb::new(
            -solid.rect.right(),
            solid.rect.y,
            solid.rect.w,
            solid.rect.h,
        );
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
        vel.y = (vel.y + PLAYER.gravity * TICK).min(PLAYER.max_fall);
        step(&mut pos, &mut vel, &solids);
    }
    let settled = pos;

    for tick in 0..600 {
        vel.y = (vel.y + PLAYER.gravity * TICK).min(PLAYER.max_fall);
        let contact = step(&mut pos, &mut vel, &solids);
        assert!(
            contact.grounded,
            "lost contact with the floor at tick {tick}"
        );
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
    assert!(touching_wall(flush, *SIZE, &wall, 1.0), "flush contact");
    assert!(
        !touching_wall(flush, *SIZE, &wall, -1.0),
        "wall is on the right, not the left"
    );

    let apart = Vec2::new(200.0 - SIZE.x - 2.0, y);
    assert!(
        !touching_wall(apart, *SIZE, &wall, 1.0),
        "2px away is not touching"
    );

    let one_way = [SolidRect::one_way(Aabb::new(200.0, 100.0, 32.0, 200.0))];
    assert!(
        !touching_wall(flush, *SIZE, &one_way, 1.0),
        "one-way platforms are never walls"
    );

    // a wall shorter than the probe's vertical inset must still register
    let stub = [SolidRect::solid(Aabb::new(200.0, y + 10.0, 32.0, 6.0))];
    assert!(
        touching_wall(flush, *SIZE, &stub, 1.0),
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
            let mut vel = Vec2::new(0.0, PLAYER.gravity * TICK);

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
        escaped
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
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

// ---------------------------------------------------------------------------
// 9. Hazards
// ---------------------------------------------------------------------------

/// Lethal boxes scattered through the sweep area, of the shape a fire owns:
/// one hanging in mid-air, one on the floor, one straddling a wall.
fn test_hazards() -> Vec<Aabb> {
    vec![
        Aabb::new(102.0, 366.0, 20.0, 26.0),
        Aabb::new(230.0, 300.0, 20.0, 26.0),
        Aabb::new(280.0, 366.0, 60.0, 26.0),
    ]
}

/// A hazard kills; it does not obstruct. Adding hazards to a world — the
/// level's own or an entity's, however many, wherever — must not move a body
/// by a single bit, or fire is a wall that you happen to die on.
///
/// This is the property that makes it safe for `avatar::after_move` to read
/// hazards out of the same `Geometry` resolution walks: the two halves cannot
/// leak into each other, so unifying them costs nothing.
#[test]
fn hazards_never_change_a_resolution() {
    let solids = test_geometry();
    let plain = Geometry::from_rects(solids.clone());

    let full: Vec<Aabb> = solids
        .iter()
        .filter(|s| !s.one_way)
        .map(|s| s.rect)
        .collect();
    let one_way: Vec<Aabb> = solids
        .iter()
        .filter(|s| s.one_way)
        .map(|s| s.rect)
        .collect();
    let mut with_hazards = Geometry::from_level(&full, &one_way, &test_hazards());
    // ...and a second set on top, the way a lit fire arrives.
    with_hazards.set_entity_hazards(test_hazards().into_iter().map(|mut hazard| {
        hazard.x += 8.0;
        hazard
    }));

    let mut rng = Lcg::new();
    let mut divergent = Vec::new();

    for _ in 0..40_000 {
        let pos = Vec2::new(rng.f32_in(-40.0, 360.0), rng.f32_in(160.0, 440.0));
        let vel = Vec2::new(rng.f32_in(-900.0, 900.0), rng.f32_in(-900.0, 900.0));

        let (mut pos_a, mut vel_a) = (pos, vel);
        step(&mut pos_a, &mut vel_a, &plain);
        let (mut pos_b, mut vel_b) = (pos, vel);
        step(&mut pos_b, &mut vel_b, &with_hazards);

        if pos_a != pos_b || vel_a != vel_b {
            divergent.push(format!(
                "from {pos:?} vel {vel:?}: {pos_a:?}/{vel_a:?} vs {pos_b:?}/{vel_b:?}"
            ));
        }
    }

    assert!(
        divergent.is_empty(),
        "{} resolutions were changed by hazards, e.g.:\n  {}",
        divergent.len(),
        divergent
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// The hazard query has to answer exactly what a scan over the same boxes
/// would, wherever they came from. Half the set is the level's here and half
/// is entity-owned, because the death check must not be able to tell which is
/// which — that is the whole of "a spike and a lit fire kill the same way".
#[test]
fn a_hazard_query_matches_a_scan_over_both_halves() {
    let all = test_hazards();
    let mut geometry = Geometry::from_level(&[], &[], &all[..1]);
    geometry.set_entity_hazards(all[1..].to_vec());

    let mut rng = Lcg::new();
    for _ in 0..40_000 {
        let pos = Vec2::new(rng.f32_in(-40.0, 400.0), rng.f32_in(160.0, 440.0));
        let body = Aabb::new(pos.x, pos.y, SIZE.x, SIZE.y);
        assert_eq!(
            geometry.hazard_overlapping(body),
            all.iter().any(|hazard| body.overlaps(hazard)),
            "hazard query disagreed with a scan at {pos:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// 10. Moving geometry and rider carry
// ---------------------------------------------------------------------------
//
// The properties below are the exit criterion for M7's hardest piece. They are
// sweeps rather than examples on purpose: rider carry fails in narrow bands —
// at one speed, on the tick a platform reverses, at the moment a rider is
// pressed into a wall — and every one of those is a case a hand-picked example
// walks straight past.
//
// They drive the real systems in the real order, spelled out in `mover_tick`
// below, so a change to that order in `Sim::step` shows up here as a failure
// rather than as a subtly different game.

/// One tick of the three systems that make a platform carry a rider, in the
/// order [`supergame::sim::Sim::step`] runs them: platforms move and carry,
/// then the geometry is rebuilt, then every body integrates and resolves.
///
/// Written out rather than calling `Sim::step` so a sweep can run thousands of
/// configurations with no avatar, animation or combat in the way — and so that
/// the ordering these properties depend on is visible right here.
fn mover_tick(world: &mut World, geometry: &mut Geometry, tick: u64) {
    mover::advance(world, tick);
    body::rebuild_geometry(geometry, world);
    body::move_bodies(world, geometry, TICK);
}

/// A bare body: no `Avatar`, no controller, nothing player-specific. If carry
/// works for this it works for a knight or anything else that can stand.
fn spawn_rider(world: &mut World, pos: Vec2) -> hecs::Entity {
    world.spawn((
        Position(pos),
        Velocity(Vec2::ZERO),
        Size(*SIZE),
        Body::new(pos, PLAYER.gravity, PLAYER.max_fall),
    ))
}

fn spawn_platform(world: &mut World, path: Mover, size: Vec2, one_way: bool) -> hecs::Entity {
    let collider = Collider {
        rect_offset: Vec2::ZERO,
        size,
        kind: if one_way {
            ColliderKind::OneWay
        } else {
            ColliderKind::Solid
        },
    };
    world.spawn((Position(path.at(0)), collider, path))
}

fn pos_of(world: &World, entity: hecs::Entity) -> Vec2 {
    world
        .get::<&Position>(entity)
        .expect("entity has a position")
        .0
}

fn body_of(world: &World, entity: hecs::Entity) -> Body {
    *world.get::<&Body>(entity).expect("entity has a body")
}

/// How deep a body is inside any full solid the geometry currently holds, and
/// which one. Mirrors `Sim::check_invariants`: resting contact shares an edge
/// and is not penetration, so only a genuine overlap counts.
fn penetration(body: Aabb, geometry: &Geometry) -> Option<(Aabb, f32)> {
    geometry
        .rects()
        .iter()
        .filter(|s| !s.one_way)
        .filter_map(|s| {
            let overlap_x = body.right().min(s.rect.right()) - body.x.max(s.rect.x);
            let overlap_y = body.bottom().min(s.rect.bottom()) - body.y.max(s.rect.y);
            let depth = overlap_x.min(overlap_y);
            (depth > 0.0).then_some((s.rect, depth))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// The same tolerance `Sim::check_invariants` uses.
const PENETRATION_TOLERANCE: f32 = 1.0;

/// A wide range of platform speeds, as (path length, ticks per leg) — from a
/// crawl at 32px/s to 1080px/s, faster than the player can fall.
const LEGS: [(f32, u32); 6] = [
    (64.0, 120),
    (128.0, 90),
    (192.0, 60),
    (256.0, 45),
    (300.0, 30),
    (360.0, 20),
];

/// **Property: a grounded rider's displacement is the platform's, every tick.**
///
/// The acceptance criterion in its exact form. Written as "the rider's new
/// position is its old position plus the platform's displacement" rather than
/// as "the two measured deltas are equal", because the former is bit-exact
/// while the latter is not — `(a + d) - a` need not give back `d` — and because
/// the former also catches resolution nudging a rider nothing should have
/// touched.
///
/// Catches: carry applied to velocity instead of position (a one-tick lag, so
/// the two disagree on every tick after a speed change); carry computed against
/// a stale platform rect; a rider re-resolved into a different column; and any
/// jitter at all, since one wrong tick out of 400 fails it.
#[test]
fn a_grounded_riders_delta_is_the_platforms_delta_every_tick() {
    let mut divergent: Vec<String> = Vec::new();

    for (length, leg) in LEGS {
        for direction in [1.0f32, -1.0] {
            for offset_frac in [0.0f32, 0.3, 0.75] {
                for phase in [0u32, 17, 61] {
                    let start = Vec2::new(400.0, 300.0);
                    let path = Mover::new(
                        start,
                        start + Vec2::new(direction * length, 0.0),
                        leg,
                        phase,
                    );
                    let plat_size = Vec2::new(96.0, 16.0);

                    let mut world = World::new();
                    let platform = spawn_platform(&mut world, path, plat_size, false);
                    let seat =
                        path.at(0) + Vec2::new(offset_frac * (plat_size.x - SIZE.x), -SIZE.y);
                    let rider = spawn_rider(&mut world, seat);
                    let mut geometry = Geometry::default();

                    for tick in 0..400u64 {
                        let before = (pos_of(&world, rider), pos_of(&world, platform));
                        mover_tick(&mut world, &mut geometry, tick);
                        let after = (pos_of(&world, rider), pos_of(&world, platform));

                        // The first tick is the rider settling onto the
                        // platform; it is not a passenger until it is grounded.
                        if !body_of(&world, rider).grounded {
                            continue;
                        }
                        let expected = before.0.x + (after.1.x - before.1.x);
                        if after.0.x != expected {
                            divergent.push(format!(
                                "{length}px/{leg}t dir {direction} offset {offset_frac} \
                                 phase {phase}: tick {tick} rider at {} not {expected}",
                                after.0.x
                            ));
                            break;
                        }
                    }
                }
            }
        }
    }

    assert!(
        divergent.is_empty(),
        "{} configurations drifted away from their platform, e.g.:\n  {}",
        divergent.len(),
        divergent
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// **Property: a rider never becomes airborne while a platform moves under it.**
///
/// The classic failure: the platform is moved, the geometry is rebuilt, and for
/// one tick the rider is standing over the hole where the platform used to be —
/// `grounded` blinks off, coyote time starts, the fall animation flickers, and
/// on the next tick everything is fine again. It is a one-frame bug that no
/// end-state assertion can see, which is why this checks every tick.
///
/// Swept over speeds up to well past the player's run, because the faster the
/// platform the wider the hole a lagging carry leaves.
#[test]
fn a_rider_never_becomes_airborne_while_the_platform_moves() {
    let mut lost: Vec<String> = Vec::new();

    for (length, leg) in LEGS {
        for direction in [1.0f32, -1.0] {
            let start = Vec2::new(400.0, 300.0);
            let path = Mover::new(start, start + Vec2::new(direction * length, 0.0), leg, 0);

            let mut world = World::new();
            spawn_platform(&mut world, path, Vec2::new(96.0, 16.0), false);
            let rider = spawn_rider(&mut world, path.at(0) + Vec2::new(30.0, -SIZE.y));
            let mut geometry = Geometry::default();

            let mut grounded_since = None;
            for tick in 0..600u64 {
                mover_tick(&mut world, &mut geometry, tick);
                let grounded = body_of(&world, rider).grounded;
                match (grounded_since, grounded) {
                    (None, true) => grounded_since = Some(tick),
                    (Some(since), false) => {
                        lost.push(format!(
                            "{length}px/{leg}t dir {direction}: airborne at tick {tick} \
                             after riding since {since}"
                        ));
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    assert!(
        lost.is_empty(),
        "{} riders were dropped mid-ride, e.g.:\n  {}",
        lost.len(),
        lost.iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// **Property: reversing direction does not displace a rider.**
///
/// The turn is where a platform's per-tick delta changes sign, and it is where
/// a carry that leans on last tick's delta, or that treats the path as
/// monotonic, quietly leaves the rider a pixel behind — once per cycle, in the
/// same direction every time, so it reads as slow drift toward one end.
///
/// Swept over phases so the reversal lands on a different tick of the run each
/// time, and asserted as "the rider's offset from the platform's left edge is
/// still the one it started with", which is drift and jitter in one number.
#[test]
fn reversing_direction_does_not_displace_a_rider() {
    let mut displaced: Vec<String> = Vec::new();

    for (length, leg) in LEGS {
        for phase in [0u32, 1, 7, 29, 44, 59] {
            let start = Vec2::new(400.0, 300.0);
            let path = Mover::new(start, start + Vec2::new(length, 0.0), leg, phase);

            let mut world = World::new();
            let platform = spawn_platform(&mut world, path, Vec2::new(96.0, 16.0), false);
            let rider = spawn_rider(&mut world, path.at(0) + Vec2::new(30.0, -SIZE.y));
            let mut geometry = Geometry::default();

            // One tick to settle, then the offset must hold across several
            // full there-and-back cycles.
            mover_tick(&mut world, &mut geometry, 0);
            let seated = pos_of(&world, rider).x - pos_of(&world, platform).x;

            for tick in 1..(path.period() as u64 * 4) {
                mover_tick(&mut world, &mut geometry, tick);
                let offset = pos_of(&world, rider).x - pos_of(&world, platform).x;
                // A tenth of a pixel: far below anything visible, far above the
                // float rounding of one addition per tick, and nothing like the
                // whole pixel per cycle that a reversal bug costs.
                if (offset - seated).abs() > 0.1 {
                    displaced.push(format!(
                        "{length}px/{leg}t phase {phase}: tick {tick} offset {offset} \
                         (seated at {seated})"
                    ));
                    break;
                }
            }
        }
    }

    assert!(
        displaced.is_empty(),
        "{} riders slid on their platform across a reversal, e.g.:\n  {}",
        displaced.len(),
        displaced
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// **Property: a vertical platform carries a rider both ways, and the trip down
/// never leaves it briefly airborne.**
///
/// Going up is easy — the platform arrives under the feet. Going down is the
/// one that breaks: the floor drops away faster than gravity pulls the rider
/// after it, so a rider that is not carried falls a tick behind, loses contact,
/// and stutters the whole way down. Asserted as feet flush on the platform's
/// top every tick, which rules out sinking into it as well.
#[test]
fn a_vertical_platform_carries_a_rider_up_and_down_without_dropping_it() {
    let mut problems: Vec<String> = Vec::new();

    for (length, leg) in LEGS {
        for phase in [0u32, 13, 47] {
            let start = Vec2::new(400.0, 300.0);
            let path = Mover::new(start, start + Vec2::new(0.0, length), leg, phase);

            let mut world = World::new();
            let platform = spawn_platform(&mut world, path, Vec2::new(96.0, 16.0), false);
            let rider = spawn_rider(&mut world, path.at(0) + Vec2::new(30.0, -SIZE.y));
            let mut geometry = Geometry::default();

            for tick in 0..(path.period() as u64 * 3) {
                mover_tick(&mut world, &mut geometry, tick);
                if !body_of(&world, rider).grounded {
                    problems.push(format!(
                        "{length}px/{leg}t phase {phase}: airborne at tick {tick}"
                    ));
                    break;
                }
                let feet = pos_of(&world, rider).y + SIZE.y;
                let top = pos_of(&world, platform).y;
                if (feet - top).abs() > 1e-3 {
                    problems.push(format!(
                        "{length}px/{leg}t phase {phase}: tick {tick} feet at {feet}, \
                         platform top at {top}"
                    ));
                    break;
                }
            }
        }
    }

    assert!(
        problems.is_empty(),
        "{} vertical rides went wrong, e.g.:\n  {}",
        problems.len(),
        problems
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// Static geometry with three platforms sweeping through it: the corner-and-
/// seam world the other sweeps use, plus a solid platform running along the
/// floor, a fast one crossing the open space, and a one-way one rising and
/// falling through where a body would stand.
///
/// No platform's path dead-ends against static geometry, because a map is not
/// allowed to place one that does — `a_moving_platform_never_sweeps_into_solid
/// _geometry` in `tests/levels.rs` is the check that says so. A platform that
/// *did* would be able to wedge a body against a wall with nowhere legal to
/// put it, which is the crush case and has its own property below.
fn moving_world() -> (World, Geometry) {
    let statics = test_geometry();
    let full: Vec<Aabb> = statics
        .iter()
        .filter(|s| !s.one_way)
        .map(|s| s.rect)
        .collect();
    let one_way: Vec<Aabb> = statics
        .iter()
        .filter(|s| s.one_way)
        .map(|s| s.rect)
        .collect();
    let geometry = Geometry::from_level(&full, &one_way, &[]);

    let mut world = World::new();
    // Along the floor, stopping well short of the wall at x = 288.
    spawn_platform(
        &mut world,
        Mover::new(Vec2::new(0.0, 368.0), Vec2::new(160.0, 368.0), 60, 0),
        Vec2::new(96.0, 32.0),
        false,
    );
    // Across the open space, fast, stopping short of the wall as well.
    spawn_platform(
        &mut world,
        Mover::new(Vec2::new(20.0, 300.0), Vec2::new(200.0, 300.0), 25, 11),
        Vec2::new(64.0, 16.0),
        false,
    );
    // A one-way strip rising and falling through where a body would stand.
    spawn_platform(
        &mut world,
        Mover::new(Vec2::new(120.0, 240.0), Vec2::new(120.0, 392.0), 40, 5),
        Vec2::new(128.0, 8.0),
        true,
    );
    (world, geometry)
}

/// **Property: no body ends a tick inside a solid, static or moving.**
///
/// The invariant everything else rests on, extended to geometry that moves.
/// Static geometry can only be entered by a body's own motion; a platform can
/// arrive *around* a body that never moved, and a body pinned between a
/// platform and a wall can be left with nowhere legal to be at all. The answer
/// is the depenetration fallback that already existed — this sweep is what says
/// it is enough, and that no second escape hatch is needed.
///
/// Randomized starts and velocities, so bodies meet the platforms from every
/// direction, at rest and at speed, from inside and from outside.
#[test]
fn no_body_ends_a_tick_inside_a_solid_when_geometry_moves() {
    let mut rng = Lcg::new();
    let mut stuck: Vec<String> = Vec::new();

    let statics = test_geometry();
    for case in 0..400 {
        let start = Vec2::new(rng.f32_in(-20.0, 340.0), rng.f32_in(180.0, 420.0));
        let velocity = Vec2::new(rng.f32_in(-300.0, 300.0), rng.f32_in(-600.0, 900.0));
        // Starts already buried in the level are `a_body_inside_a_solid_is_
        // pushed_out`'s business, not this one; escaping takes a tick or two
        // and would read here as a violation.
        if overlaps_any_solid(start, &statics).is_some() {
            continue;
        }

        let (mut world, mut geometry) = moving_world();
        let rider = spawn_rider(&mut world, start);
        world.get::<&mut Velocity>(rider).unwrap().0 = velocity;

        for tick in 0..180u64 {
            mover_tick(&mut world, &mut geometry, tick);
            let pos = pos_of(&world, rider);
            let box_ = Aabb::new(pos.x, pos.y, SIZE.x, SIZE.y);
            if let Some((solid, depth)) = penetration(box_, &geometry) {
                if depth > PENETRATION_TOLERANCE {
                    stuck.push(format!(
                        "case {case} from {start:?} vel {velocity:?}: tick {tick} body {box_:?} \
                         is {depth:.2}px inside {solid:?}"
                    ));
                    break;
                }
            }
        }
    }

    assert!(
        stuck.is_empty(),
        "{} runs ended a tick inside solid geometry, e.g.:\n  {}",
        stuck.len(),
        stuck
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// **Property: a rider carried into a wall is squeezed out, not swallowed and
/// not wedged.**
///
/// The crush case, isolated: a platform runs a rider along the floor into a
/// wall it cannot pass. What must not happen is the rider ending up inside the
/// wall, or inside the platform, or somewhere it can never leave. What must
/// happen is the platform sliding on underneath while the rider stops flush
/// against the wall — which is the depenetration pass pushing out along the
/// shallowest axis, and nothing else.
#[test]
fn a_rider_pressed_between_a_platform_and_a_wall_is_squeezed_out() {
    // A wall rising out of a floor, with the platform running along the floor
    // beneath it. The platform's far end carries its passenger past the wall
    // face, so the wall has to be what stops the rider.
    let wall = Aabb::new(400.0, 200.0, 32.0, 232.0);
    let floor = Aabb::new(0.0, 400.0, 440.0, 32.0);
    let mut problems: Vec<String> = Vec::new();

    for (length, leg) in LEGS {
        let mut world = World::new();
        let far = Vec2::new(344.0, 368.0);
        let path = Mover::new(far - Vec2::new(length, 0.0), far, leg, 0);
        spawn_platform(&mut world, path, Vec2::new(96.0, 32.0), false);
        // Seated near the platform's trailing right edge, so the ride ends
        // with the rider hard against the wall while the platform slides on.
        let rider = spawn_rider(&mut world, path.at(0) + Vec2::new(76.0, -SIZE.y));
        let mut geometry = Geometry::from_level(&[wall, floor], &[], &[]);

        let mut pressed = false;
        for tick in 0..(leg as u64 * 2) {
            mover_tick(&mut world, &mut geometry, tick);
            let pos = pos_of(&world, rider);
            let box_ = Aabb::new(pos.x, pos.y, SIZE.x, SIZE.y);

            if let Some((solid, depth)) = penetration(box_, &geometry) {
                if depth > PENETRATION_TOLERANCE {
                    problems.push(format!(
                        "{length}px/{leg}t: tick {tick} body {box_:?} is {depth:.2}px inside \
                         {solid:?}"
                    ));
                    break;
                }
            }
            if box_.right() > wall.x + PENETRATION_TOLERANCE {
                problems.push(format!(
                    "{length}px/{leg}t: tick {tick} body passed through the wall face, \
                     right edge at {}",
                    box_.right()
                ));
                break;
            }
            pressed |= box_.right() >= wall.x - 1.0;
        }

        if !pressed {
            problems.push(format!(
                "{length}px/{leg}t: the platform never pressed the rider into the wall, \
                 so this case proved nothing"
            ));
        }
    }

    assert!(
        problems.is_empty(),
        "{} crush cases went wrong:\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

/// **Property: a body run down by a platform never tunnels, and is never left
/// stuck once the platform has gone.**
///
/// The other crush: not a rider being carried into something, but a solid
/// platform sweeping along a floor *through* a body standing on that floor —
/// a battering ram — and pinning it against a wall.
///
/// This one deliberately asserts less than the sweep above, because less is
/// true. A body wedged between an advancing solid and an immovable one has
/// nowhere legal to be, and no depenetration can invent somewhere: pushed out
/// of the wall it is inside the platform, pushed out of the platform it is
/// inside the wall. Every platformer answers this the same way — the body
/// overlaps something until the press ends — and the ticket asks for exactly
/// the two properties that *can* hold: it must not **tunnel** (end up on the
/// far side of either), and it must not **stick** (still be inside anything
/// once the platform retreats).
///
/// This is also why `tests/levels.rs` refuses to ship a map whose platform path
/// dead-ends into geometry: the situation is survivable, but it is never what
/// an author meant.
#[test]
fn a_body_run_down_by_a_platform_never_tunnels_and_comes_free() {
    let wall = Aabb::new(400.0, 200.0, 32.0, 232.0);
    let floor = Aabb::new(0.0, 400.0, 400.0, 32.0);
    let mut problems: Vec<String> = Vec::new();

    for (length, leg) in LEGS {
        let mut world = World::new();
        // Straight at the wall, at exactly the height of a standing body.
        let path = Mover::new(
            Vec2::new(304.0 - length, 368.0),
            Vec2::new(304.0, 368.0),
            leg,
            0,
        );
        spawn_platform(&mut world, path, Vec2::new(96.0, 32.0), false);
        let rider = spawn_rider(&mut world, Vec2::new(370.0, 400.0 - SIZE.y));
        let mut geometry = Geometry::from_level(&[wall, floor], &[], &[]);

        for tick in 0..(leg as u64 * 4) {
            mover_tick(&mut world, &mut geometry, tick);
            let pos = pos_of(&world, rider);
            let box_ = Aabb::new(pos.x, pos.y, SIZE.x, SIZE.y);

            // No tunnelling, in either direction: never through the wall, and
            // never popped out behind the platform.
            if box_.right() > wall.right() {
                problems.push(format!(
                    "{length}px/{leg}t: tick {tick} body tunnelled through the wall to {box_:?}"
                ));
                break;
            }
            if box_.right() < pos_of(&world, rider).x {
                problems.push(format!("{length}px/{leg}t: tick {tick} body inverted"));
                break;
            }
        }

        // And once the platform has retreated, nothing is holding it.
        for tick in (leg as u64 * 4)..(leg as u64 * 8) {
            mover_tick(&mut world, &mut geometry, tick);
        }
        let pos = pos_of(&world, rider);
        let box_ = Aabb::new(pos.x, pos.y, SIZE.x, SIZE.y);
        if let Some((solid, depth)) = penetration(box_, &geometry) {
            if depth > PENETRATION_TOLERANCE {
                problems.push(format!(
                    "{length}px/{leg}t: still {depth:.2}px inside {solid:?} long after the \
                     platform went away"
                ));
            }
        }
    }

    assert!(
        problems.is_empty(),
        "{} battering-ram cases went wrong:\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

/// **Property: jumping off a moving platform is the same jump either way.**
///
/// The design decision documented in [`supergame::systems::mover`], stated as a
/// test. Carry adds to position and never to velocity, and a body with upward
/// velocity is not a passenger — so the arc off a platform racing right, the
/// arc off the same platform racing left, and the arc off static ground are one
/// arc, translated. Inheriting the platform's momentum is the other defensible
/// answer; this is what says which one shipped.
///
/// It fails on any velocity-based carry, and on a position carry that still
/// ran on the tick of the launch.
#[test]
fn jumping_off_is_the_same_jump_whichever_way_the_platform_is_going() {
    /// Launch on tick 30 and record the whole airborne arc, relative to where
    /// it launched from. The arc *ends* at the landing: once back on the
    /// platform the rider is a passenger again, and where a passenger goes
    /// obviously depends on which way the platform is headed.
    fn arc(platform_direction: f32) -> Vec<Vec2> {
        let start = Vec2::new(0.0, 300.0);
        let mut world = World::new();
        // Direction 0 is the control: ground of the same shape, going nowhere.
        // Wide enough that the rider comes down on it whichever way it went,
        // so the three runs differ in one thing only.
        spawn_platform(
            &mut world,
            Mover::new(
                start,
                start + Vec2::new(platform_direction * 240.0, 0.0),
                45,
                0,
            ),
            Vec2::new(1200.0, 16.0),
            false,
        );
        let rider = spawn_rider(&mut world, start + Vec2::new(600.0, -SIZE.y));
        let mut geometry = Geometry::default();

        let mut samples = Vec::new();
        let mut launched_from = Vec2::ZERO;
        for tick in 0..90u64 {
            if tick == 30 {
                // Exactly what `avatar::control` does, at the same point in the
                // tick: decide a velocity, before `mover::advance` runs.
                world.get::<&mut Velocity>(rider).unwrap().0.y = -PLAYER.avatar().jump_speed;
                launched_from = pos_of(&world, rider);
            }
            mover_tick(&mut world, &mut geometry, tick);
            if tick < 30 {
                continue;
            }
            samples.push(pos_of(&world, rider) - launched_from);
            if tick > 30 && body_of(&world, rider).grounded {
                break;
            }
        }
        samples
    }

    let still = arc(0.0);
    assert!(
        still.iter().any(|p| p.y < -50.0),
        "the control jump did not actually jump"
    );
    assert!(
        still.len() > 40,
        "the control jump landed suspiciously fast"
    );
    assert_eq!(arc(1.0), still, "jumping off a platform moving right");
    assert_eq!(arc(-1.0), still, "jumping off a platform moving left");
}
