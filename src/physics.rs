//! Collision primitives, independent of ggez so they can be unit tested.
//!
//! The collision routines here never take a list of rectangles. They take a
//! [`SolidQuery`] — "give me the solids that could overlap this box" — so that
//! where the geometry lives, and how it is indexed, is not their problem.
//! [`Geometry`] is the implementation the game uses: static level rects plus
//! whatever colliders entities own this tick, behind a uniform grid.

use std::collections::HashMap;

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

    /// The smallest box containing both.
    pub fn union(&self, other: &Aabb) -> Aabb {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Aabb {
            x,
            y,
            w: self.right().max(other.right()) - x,
            h: self.bottom().max(other.bottom()) - y,
        }
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

/// What the collision routines ask the world for: the solids that could
/// overlap a box.
///
/// This exists so that `resolve_move` and the wall probes do not care whether
/// the geometry is a flat slice, a level's static rects, or a grid holding
/// both those and a moving platform's collider. Two rules make swapping the
/// implementation safe:
///
/// 1. **Conservative.** A query may return solids that do not overlap `area`,
///    but never omit one that does. A caller that hands over a box containing
///    every position it will test therefore behaves exactly as if it had
///    scanned everything: the extra solids it never sees would have failed
///    their overlap test anyway.
/// 2. **Stable order.** Every method returns solids in one fixed order, the
///    same relative order for a subset as for [`SolidQuery::all`]. Resolution
///    walks the list, so an order that depended on which grid cells happened
///    to be visited — or on hecs archetype layout — would let the game move
///    when nothing about it changed.
pub trait SolidQuery {
    /// Replace `out` with every solid that could overlap `area`.
    fn overlapping(&self, area: Aabb, out: &mut Vec<SolidRect>);

    /// Replace `out` with every solid there is, in the same order.
    fn all(&self, out: &mut Vec<SolidRect>);
}

/// A slice is its own broadphase: it hands back everything, which is what the
/// collision routines did with a `&[SolidRect]` before this trait existed.
/// Tests and small fixtures use it, and it is the reference the grid is
/// checked against.
impl SolidQuery for [SolidRect] {
    fn overlapping(&self, _area: Aabb, out: &mut Vec<SolidRect>) {
        self.all(out);
    }

    fn all(&self, out: &mut Vec<SolidRect>) {
        out.clear();
        out.extend_from_slice(self);
    }
}

impl<const N: usize> SolidQuery for [SolidRect; N] {
    fn overlapping(&self, area: Aabb, out: &mut Vec<SolidRect>) {
        self[..].overlapping(area, out);
    }

    fn all(&self, out: &mut Vec<SolidRect>) {
        self[..].all(out);
    }
}

impl SolidQuery for Vec<SolidRect> {
    fn overlapping(&self, area: Aabb, out: &mut Vec<SolidRect>) {
        self[..].overlapping(area, out);
    }

    fn all(&self, out: &mut Vec<SolidRect>) {
        self[..].all(out);
    }
}

/// What the death check asks the world for: is anything lethal overlapping
/// this box?
///
/// Deliberately not part of [`SolidQuery`]. A hazard is geometry but not
/// obstruction — it never enters resolution, and a body walks into fire rather
/// than bumping off it — so the two are different questions asked by different
/// phases of the tick. What they share is where the answer comes from: a spike
/// carved into the map and a fire an entity lit this tick both arrive through
/// this trait, so there is one way to die on contact rather than one per
/// source.
pub trait HazardQuery {
    /// Does any hazard overlap `area`?
    fn hazard_overlapping(&self, area: Aabb) -> bool;
}

/// A slice of boxes is its own hazard set. Tests and fixtures use it, and it
/// is what `LevelData::hazards` was before hazards could move.
impl HazardQuery for [Aabb] {
    fn hazard_overlapping(&self, area: Aabb) -> bool {
        self.iter().any(|hazard| area.overlaps(hazard))
    }
}

impl HazardQuery for Vec<Aabb> {
    fn hazard_overlapping(&self, area: Aabb) -> bool {
        self[..].hazard_overlapping(area)
    }
}

impl<const N: usize> HazardQuery for [Aabb; N] {
    fn hazard_overlapping(&self, area: Aabb) -> bool {
        self[..].hazard_overlapping(area)
    }
}

/// A view of a solid set with one-way platforms filtered out: what a body
/// dropping through a platform sees.
pub struct SolidsOnly<'a, Q: ?Sized>(pub &'a Q);

impl<Q: SolidQuery + ?Sized> SolidQuery for SolidsOnly<'_, Q> {
    fn overlapping(&self, area: Aabb, out: &mut Vec<SolidRect>) {
        self.0.overlapping(area, out);
        out.retain(|s| !s.one_way);
    }

    fn all(&self, out: &mut Vec<SolidRect>) {
        self.0.all(out);
        out.retain(|s| !s.one_way);
    }
}

/// Cell size of the broadphase grid, in pixels: two 32px tiles.
///
/// One tile makes the buckets tiny and the cell walk long; four puts most of a
/// room in one bucket. Two sits between, and a merged floor row — one rect per
/// horizontal run of tiles — still spans few enough cells to bucket cheaply.
const CELL: f32 = 64.0;

/// How many cells one query may walk before it gives up and scans everything.
///
/// A query box that large is a body crossing the map in a tick, or a caller
/// asking about the whole world; walking thousands of buckets to reach the
/// same answer is slower than the linear scan the grid replaced.
const MAX_CELLS_PER_QUERY: i64 = 4096;

/// How many cells one rect may be bucketed into. A rect wider than this is
/// kept in `always`, so it is a candidate for every query — cheaper than
/// writing its index into a thousand buckets it will never be looked up from.
const MAX_CELLS_PER_RECT: i64 = 4096;

/// A uniform grid over rect *indices*. Indices rather than rects so a query
/// can sort them and hand back a subset in the owner's order.
#[derive(Clone, Debug, Default)]
struct Grid {
    /// Ascending index lists, bucketed by cell.
    cells: HashMap<(i32, i32), Vec<u32>>,
    /// Rects too large to bucket: candidates for every query.
    always: Vec<u32>,
}

impl Grid {
    fn clear(&mut self) {
        self.cells.clear();
        self.always.clear();
    }

    /// Bucket one rect. Indices must be inserted in ascending order, which is
    /// what keeps each bucket sorted.
    fn insert(&mut self, index: u32, rect: &Aabb) {
        let Some(((x0, y0), (x1, y1))) = cell_range(rect, MAX_CELLS_PER_RECT) else {
            self.always.push(index);
            return;
        };
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                self.cells.entry((cx, cy)).or_default().push(index);
            }
        }
    }

    /// Append the indices of everything bucketed near `area` to `out`.
    /// Returns false when the area cannot be walked cell by cell, in which
    /// case `out` is meaningless and the caller should scan all.
    fn gather(&self, area: &Aabb, out: &mut Vec<u32>) -> bool {
        let Some(((x0, y0), (x1, y1))) = cell_range(area, MAX_CELLS_PER_QUERY) else {
            return false;
        };
        out.extend_from_slice(&self.always);
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                if let Some(bucket) = self.cells.get(&(cx, cy)) {
                    out.extend_from_slice(bucket);
                }
            }
        }
        true
    }
}

/// The inclusive cell coordinates a box touches, or `None` when it covers
/// more than `budget` of them.
///
/// `None` also covers a box with a non-finite corner. Casting NaN to `i32`
/// yields 0, so without the check a garbage box would quietly bucket itself
/// at the origin and be invisible everywhere else — a wrong answer, where the
/// fallback is merely a slow one.
fn cell_range(rect: &Aabb, budget: i64) -> Option<((i32, i32), (i32, i32))> {
    if !(rect.x.is_finite() && rect.y.is_finite() && rect.w.is_finite() && rect.h.is_finite()) {
        return None;
    }
    let lo = |v: f32| (v / CELL).floor() as i32;
    let (x0, y0) = (lo(rect.x), lo(rect.y));
    let (x1, y1) = (lo(rect.right()), lo(rect.bottom()));
    let w = (x1 as i64 - x0 as i64 + 1).max(0);
    let h = (y1 as i64 - y0 as i64 + 1).max(0);
    (w.saturating_mul(h) <= budget).then_some(((x0, y0), (x1, y1)))
}

/// Every solid in the world: the level's own rects, plus the colliders
/// entities own this tick.
///
/// Static rects are bucketed once when the level loads. Entity-owned rects are
/// replaced wholesale each tick by [`Geometry::set_entity_rects`] — cheap
/// because there are a handful of them next to thousands of tiles, and
/// simpler than tracking which ones moved.
///
/// The two halves live in one list, statics first, so an index into it is a
/// total order over the world's geometry. That order is what
/// [`SolidQuery::overlapping`] restores by sorting, and it is why swapping the
/// grid in for a linear scan cannot move the game: the grid returns a subset
/// of the same list in the same order, and the solids it filtered out would
/// have failed their overlap test anyway.
#[derive(Clone, Debug, Default)]
pub struct Geometry {
    /// Static level rects, then entity-owned ones.
    rects: Vec<SolidRect>,
    /// Where the entity-owned half starts.
    static_len: usize,
    statics: Grid,
    dynamics: Grid,
    /// Lethal boxes, the level's own first and then entity-owned ones — the
    /// same two halves as `rects`, kept apart from it because a hazard is not
    /// a solid and must never reach resolution.
    hazards: Vec<Aabb>,
    /// Where the entity-owned hazards start.
    static_hazard_len: usize,
}

impl Geometry {
    /// Bucket a level's static geometry: solids first, then one-way platforms.
    ///
    /// That order is load-bearing. It is the order the flattened
    /// `Vec<SolidRect>` had before `Geometry` existed, and resolution walks
    /// the list, so keeping it is what makes this refactor invisible in the
    /// golden traces.
    ///
    /// Hazards come along because they are the level's geometry too, even
    /// though nothing collides with them: the death check reads them from here
    /// so that a map's spikes and an entity's fire are one set of boxes.
    pub fn from_level(solids: &[Aabb], one_way: &[Aabb], hazards: &[Aabb]) -> Self {
        let mut rects: Vec<SolidRect> = solids.iter().map(|&r| SolidRect::solid(r)).collect();
        rects.extend(one_way.iter().map(|&r| SolidRect::one_way(r)));
        let mut geometry = Geometry::from_rects(rects);
        geometry.hazards = hazards.to_vec();
        geometry.static_hazard_len = geometry.hazards.len();
        geometry
    }

    /// Bucket an already-flattened list, keeping its order.
    pub fn from_rects(rects: Vec<SolidRect>) -> Self {
        let mut geometry = Geometry {
            static_len: rects.len(),
            rects,
            statics: Grid::default(),
            dynamics: Grid::default(),
            hazards: Vec::new(),
            static_hazard_len: 0,
        };
        for (index, solid) in geometry.rects.iter().enumerate() {
            geometry.statics.insert(index as u32, &solid.rect);
        }
        geometry
    }

    /// Replace the entity-owned rects and rebucket them.
    ///
    /// The caller decides the order and this preserves it; see
    /// [`crate::systems::body::rebuild_geometry`], which sorts by entity id so
    /// that hecs archetype layout is never observable in the physics.
    pub fn set_entity_rects(&mut self, rects: impl IntoIterator<Item = SolidRect>) {
        self.rects.truncate(self.static_len);
        self.rects.extend(rects);
        self.dynamics.clear();
        for index in self.static_len..self.rects.len() {
            let rect = self.rects[index].rect;
            self.dynamics.insert(index as u32, &rect);
        }
    }

    /// Replace the entity-owned hazards, keeping the caller's order.
    ///
    /// No grid behind these. A hazard set is a handful of boxes where the
    /// solid set is thousands, and only bodies that can die ask about it, once
    /// each per tick — a bucket walk would cost more than the scan it saves.
    /// The interface is the same either way, so putting a grid here later
    /// changes nothing above it.
    pub fn set_entity_hazards(&mut self, hazards: impl IntoIterator<Item = Aabb>) {
        self.hazards.truncate(self.static_hazard_len);
        self.hazards.extend(hazards);
    }

    /// Every solid, static and entity-owned, in query order.
    pub fn rects(&self) -> &[SolidRect] {
        &self.rects
    }

    /// How many of them are entity-owned.
    pub fn entity_rect_count(&self) -> usize {
        self.rects.len() - self.static_len
    }

    /// Every hazard box, the level's own then the entity-owned ones.
    pub fn hazards(&self) -> &[Aabb] {
        &self.hazards
    }

    /// How many of them are entity-owned.
    pub fn entity_hazard_count(&self) -> usize {
        self.hazards.len() - self.static_hazard_len
    }
}

impl HazardQuery for Geometry {
    fn hazard_overlapping(&self, area: Aabb) -> bool {
        self.hazards[..].hazard_overlapping(area)
    }
}

impl SolidQuery for Geometry {
    fn overlapping(&self, area: Aabb, out: &mut Vec<SolidRect>) {
        let mut hits: Vec<u32> = Vec::new();
        if !self.statics.gather(&area, &mut hits) || !self.dynamics.gather(&area, &mut hits) {
            self.all(out);
            return;
        }
        hits.sort_unstable();
        hits.dedup();

        out.clear();
        out.extend(
            hits.iter()
                .map(|&i| self.rects[i as usize])
                // Safe to drop the near-misses the buckets turned up: a solid
                // that overlaps the body overlaps any box containing it, so
                // anything failing here would have failed inside the passes.
                .filter(|s| area.overlaps(&s.rect)),
        );
    }

    fn all(&self, out: &mut Vec<SolidRect>) {
        out.clear();
        out.extend_from_slice(&self.rects);
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

/// Resolve a body's move against the world's geometry, one axis at a time,
/// using the previous position to decide which side it approached from.
///
/// The two passes are not cosmetic. Resolving both axes in a single loop lets
/// a decision made at one position survive a later correction that invalidates
/// it: a body can land on a floor it overlaps by a fraction of a pixel, then
/// get pushed sideways clear of that floor and still come out `grounded`.
/// Which of those happened depended on the order of `solids`. Settling x first
/// and only then testing y means the vertical pass sees the position the body
/// actually ends the tick at.
///
/// One broadphase query serves both passes. Neither pass can push the body
/// outside the box swept from `prev_pos` to `pos` — each correction moves it
/// back *toward* where it came from and stops at the surface — so a query over
/// that box holds every solid either pass could touch.
pub fn resolve_move<Q: SolidQuery + ?Sized>(
    pos: &mut Vec2,
    vel: &mut Vec2,
    prev_pos: Vec2,
    size: Vec2,
    solids: &Q,
) -> Contact {
    let mut contact = Contact::default();

    let swept = Aabb::new(prev_pos.x, prev_pos.y, size.x, size.y)
        .union(&Aabb::new(pos.x, pos.y, size.x, size.y));
    let mut candidates: Vec<SolidRect> = Vec::new();
    solids.overlapping(swept, &mut candidates);

    // --- horizontal: test the new x against the previous y ---
    for solid in &candidates {
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
    for solid in &candidates {
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
    //
    // This is the one pass the swept box does not bound: a push escapes along
    // the shallowest axis, which can carry the body half a rect clear of
    // anywhere it has been. So it runs against everything — but only once the
    // candidates prove the body is stuck, which they can, being a superset of
    // what overlaps the final position. On every ordinary tick this costs one
    // overlap test per candidate and stops.
    let body = Aabb::new(pos.x, pos.y, size.x, size.y);
    if !candidates
        .iter()
        .any(|s| !s.one_way && body.overlaps(&s.rect))
    {
        return contact;
    }
    solids.all(&mut candidates);

    for solid in &candidates {
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
pub fn touching_wall<Q: SolidQuery + ?Sized>(pos: Vec2, size: Vec2, solids: &Q, dir: f32) -> bool {
    let probe_x = if dir > 0.0 {
        pos.x + size.x
    } else {
        pos.x - 1.0
    };
    let probe = Aabb::new(probe_x, pos.y + 2.0, 1.0, size.y - 4.0);
    let mut candidates: Vec<SolidRect> = Vec::new();
    solids.overlapping(probe, &mut candidates);
    candidates
        .iter()
        .any(|s| !s.one_way && probe.overlaps(&s.rect))
}

/// Which side, if either, a body is pressed against a wall on: -1 left,
/// +1 right, 0 for neither.
///
/// `prefer` (-1, 0, or +1) breaks the tie when both sides touch: in a shaft
/// barely wider than the body, the wall the player is pushing into is the one
/// they mean. With no preference the right wall wins, arbitrarily but stably.
pub fn wall_contact<Q: SolidQuery + ?Sized>(pos: Vec2, size: Vec2, solids: &Q, prefer: f32) -> f32 {
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
mod geometry_tests {
    use super::*;

    fn tiles(count: u32) -> Vec<SolidRect> {
        (0..count)
            .map(|i| SolidRect::solid(Aabb::new(i as f32 * 32.0, 0.0, 32.0, 32.0)))
            .collect()
    }

    fn query(geometry: &Geometry, area: Aabb) -> Vec<Aabb> {
        let mut out = Vec::new();
        geometry.overlapping(area, &mut out);
        out.into_iter().map(|s| s.rect).collect()
    }

    #[test]
    fn a_query_finds_what_overlaps_it_and_leaves_out_the_rest() {
        let geometry = Geometry::from_rects(tiles(40));
        let found = query(&geometry, Aabb::new(100.0, 8.0, 16.0, 16.0));
        assert_eq!(found, vec![Aabb::new(96.0, 0.0, 32.0, 32.0)]);
        assert!(
            query(&geometry, Aabb::new(0.0, 200.0, 16.0, 16.0)).is_empty(),
            "nothing down there"
        );
    }

    /// The contract the whole refactor rests on: a query is a *subsequence* of
    /// the full list. Resolution walks whatever it is handed, so a query that
    /// reordered — by grid cell, by bucket, by hash — would move the game while
    /// claiming to be a lookup optimisation.
    #[test]
    fn a_query_is_a_subsequence_of_the_full_list() {
        let mut geometry = Geometry::from_rects(tiles(40));
        geometry.set_entity_rects([
            SolidRect::one_way(Aabb::new(150.0, 10.0, 64.0, 8.0)),
            SolidRect::solid(Aabb::new(40.0, 4.0, 20.0, 20.0)),
        ]);

        let mut everything = Vec::new();
        geometry.all(&mut everything);

        // A box over the whole world: many cells, many buckets, and every rect
        // reachable from more than one of them.
        let found = query(&geometry, Aabb::new(-100.0, -100.0, 1500.0, 300.0));
        let order: Vec<usize> = found
            .iter()
            .map(|rect| {
                everything
                    .iter()
                    .position(|s| s.rect == *rect)
                    .expect("query returned a rect that is not in the world")
            })
            .collect();
        assert!(
            order.windows(2).all(|w| w[0] < w[1]),
            "query order diverged from list order: {order:?}"
        );
        assert_eq!(found.len(), everything.len(), "and found all of them");
    }

    #[test]
    fn entity_rects_replace_rather_than_accumulate() {
        // Well clear of the static tiles, so a hit can only be the entity's.
        let mut geometry = Geometry::from_rects(tiles(4));
        for x in [1000.0, 2000.0, 3000.0] {
            geometry.set_entity_rects([SolidRect::solid(Aabb::new(x, 0.0, 16.0, 16.0))]);
            assert_eq!(geometry.entity_rect_count(), 1);
            assert_eq!(geometry.rects().len(), 5);
            assert_eq!(query(&geometry, Aabb::new(x + 4.0, 4.0, 4.0, 4.0)).len(), 1);
        }
        // ...and the ones from earlier calls really are gone from the grid.
        assert!(query(&geometry, Aabb::new(1004.0, 4.0, 4.0, 4.0)).is_empty());
    }

    /// Test worlds sit at negative coordinates (the mirror-symmetry sweeps
    /// build one), and cell indices have to floor rather than truncate or the
    /// bucket for -0.5 and the bucket for +0.5 are the same one.
    #[test]
    fn negative_coordinates_bucket_correctly() {
        let geometry = Geometry::from_rects(vec![
            SolidRect::solid(Aabb::new(-300.0, 400.0, 200.0, 32.0)),
            SolidRect::solid(Aabb::new(-32.0, -32.0, 32.0, 32.0)),
        ]);
        assert_eq!(
            query(&geometry, Aabb::new(-200.0, 410.0, 4.0, 4.0)).len(),
            1
        );
        assert_eq!(query(&geometry, Aabb::new(-16.0, -16.0, 4.0, 4.0)).len(), 1);
        assert!(query(&geometry, Aabb::new(16.0, 16.0, 4.0, 4.0)).is_empty());
    }

    /// A rect far bigger than the grid, and a query far bigger than the grid,
    /// both have to still find each other — by whatever route.
    #[test]
    fn rects_and_queries_too_large_to_bucket_still_find_each_other() {
        let huge = SolidRect::solid(Aabb::new(-1e6, -1e6, 2e6, 2e6));
        let geometry =
            Geometry::from_rects(vec![huge, SolidRect::solid(Aabb::new(0.0, 0.0, 8.0, 8.0))]);

        assert_eq!(
            query(&geometry, Aabb::new(500.0, 500.0, 4.0, 4.0)).len(),
            1,
            "a small query still finds the oversized rect"
        );
        assert_eq!(
            query(&geometry, Aabb::new(-1e5, -1e5, 2e5, 2e5)).len(),
            2,
            "an oversized query gives up on the grid and finds everything"
        );
    }

    /// A garbage box must fall back to scanning, not bucket itself at the
    /// origin and come back with the wrong answer.
    #[test]
    fn a_non_finite_query_falls_back_to_everything() {
        let geometry = Geometry::from_rects(tiles(4));
        assert_eq!(
            query(&geometry, Aabb::new(f32::NAN, 0.0, 8.0, 8.0)).len(),
            4
        );
        assert_eq!(
            query(&geometry, Aabb::new(0.0, 0.0, f32::INFINITY, 8.0)).len(),
            4
        );
    }

    #[test]
    fn a_level_flattens_solids_first_then_platforms() {
        let geometry = Geometry::from_level(
            &[
                Aabb::new(0.0, 0.0, 32.0, 32.0),
                Aabb::new(32.0, 0.0, 32.0, 32.0),
            ],
            &[Aabb::new(0.0, 64.0, 64.0, 8.0)],
            &[],
        );
        let kinds: Vec<bool> = geometry.rects().iter().map(|s| s.one_way).collect();
        assert_eq!(kinds, vec![false, false, true]);
    }

    // --- hazards ---------------------------------------------------------

    /// A hazard is lethal and nothing else: it must not appear in anything
    /// resolution walks, or fire would be a wall.
    #[test]
    fn hazards_are_queryable_but_never_solid() {
        let mut geometry = Geometry::from_level(
            &[Aabb::new(0.0, 64.0, 128.0, 32.0)],
            &[],
            &[Aabb::new(32.0, 48.0, 32.0, 16.0)],
        );
        geometry.set_entity_hazards([Aabb::new(96.0, 32.0, 20.0, 26.0)]);

        assert_eq!(geometry.rects().len(), 1, "only the floor is solid");
        assert_eq!(geometry.hazards().len(), 2);
        assert_eq!(geometry.entity_hazard_count(), 1);

        // the level's spike
        assert!(geometry.hazard_overlapping(Aabb::new(40.0, 40.0, 10.0, 20.0)));
        // the entity's box
        assert!(geometry.hazard_overlapping(Aabb::new(100.0, 40.0, 10.0, 20.0)));
        // the gap between them
        assert!(!geometry.hazard_overlapping(Aabb::new(70.0, 40.0, 10.0, 20.0)));

        // and a query over the whole world finds one solid, not three
        let mut out = Vec::new();
        geometry.overlapping(Aabb::new(0.0, 0.0, 512.0, 512.0), &mut out);
        assert_eq!(out.len(), 1);
    }

    /// The same replace-don't-accumulate contract the solid half has: an
    /// entity's hazard is gone the tick it stops presenting one, which is how
    /// a fire goes out.
    #[test]
    fn entity_hazards_replace_rather_than_accumulate() {
        let mut geometry = Geometry::from_level(&[], &[], &[Aabb::new(0.0, 0.0, 16.0, 16.0)]);
        let probe = Aabb::new(200.0, 0.0, 8.0, 8.0);

        geometry.set_entity_hazards([Aabb::new(196.0, 0.0, 16.0, 16.0)]);
        assert!(geometry.hazard_overlapping(probe), "lit");

        geometry.set_entity_hazards([]);
        assert!(!geometry.hazard_overlapping(probe), "out");
        assert_eq!(geometry.hazards().len(), 1, "the level's own survived");
        assert!(geometry.hazard_overlapping(Aabb::new(4.0, 4.0, 4.0, 4.0)));
    }

    /// Sharing an edge is not a hit, for the same reason it is not for solids:
    /// standing flush beside a fire is standing beside it.
    #[test]
    fn touching_a_hazards_edge_is_not_overlapping_it() {
        let geometry = Geometry::from_level(&[], &[], &[Aabb::new(100.0, 0.0, 20.0, 20.0)]);
        assert!(!geometry.hazard_overlapping(Aabb::new(80.0, 0.0, 20.0, 20.0)));
        assert!(geometry.hazard_overlapping(Aabb::new(80.1, 0.0, 20.0, 20.0)));
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
