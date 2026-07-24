//! Level validation: geometry a player cannot actually use.
//!
//! A one-way platform with less than a body-height of free space above it is
//! unreachable — the player's head hits the ceiling before their feet clear
//! the platform top, so they fall back through from below and the platform
//! reads as broken. Nothing about the physics is wrong, and no amount of
//! staring at the map file makes it obvious, because the offending ceiling is
//! two rows away from the platform in the ASCII grid.
//!
//! This is cheap to check and impossible to eyeball, so it is checked.

use ggez::glam::Vec2;

use super::LevelData;
use crate::physics::Aabb;

/// How finely to sample standing positions along a platform, in pixels.
const SAMPLE_STEP: f32 = 2.0;

#[derive(Clone, Debug)]
pub struct PlatformIssue {
    pub platform: Aabb,
    /// Standing positions tested along the platform.
    pub sampled: u32,
    /// How many of them leave room for the body.
    pub standable: u32,
}

impl PlatformIssue {
    pub fn fully_blocked(&self) -> bool {
        self.standable == 0
    }

    pub fn describe(&self) -> String {
        let p = self.platform;
        if self.fully_blocked() {
            format!(
                "platform x {:.0}..{:.0} y {:.0} is unreachable: \
                 no standing position has room for the body",
                p.x,
                p.right(),
                p.y
            )
        } else {
            format!(
                "platform x {:.0}..{:.0} y {:.0} is partly blocked: \
                 only {}/{} standing positions have room for the body",
                p.x,
                p.right(),
                p.y,
                self.standable,
                self.sampled
            )
        }
    }
}

impl LevelData {
    /// One-way platforms that a body of `body` size cannot stand on, in whole
    /// or in part, because solid geometry occupies the space above them.
    pub fn blocked_platforms(&self, body: Vec2) -> Vec<PlatformIssue> {
        let mut issues = Vec::new();

        for platform in &self.one_way {
            // Positions where the body sits fully on the platform. A platform
            // narrower than the body is a separate authoring problem.
            let last_x = platform.right() - body.x;
            if last_x < platform.x {
                continue;
            }

            let mut sampled = 0;
            let mut standable = 0;
            let mut x = platform.x;
            while x <= last_x {
                sampled += 1;
                let resting = Aabb::new(x, platform.y - body.y, body.x, body.y);
                if !self.solids.iter().any(|s| resting.overlaps(s)) {
                    standable += 1;
                }
                x += SAMPLE_STEP;
            }

            if standable < sampled {
                issues.push(PlatformIssue {
                    platform: *platform,
                    sampled,
                    standable,
                });
            }
        }

        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::Avatar;

    fn level(solids: Vec<Aabb>, one_way: Vec<Aabb>) -> LevelData {
        let mut level = LevelData::empty(20, 10, 32.0);
        level.solids = solids;
        level.one_way = one_way;
        level
    }

    fn body() -> Vec2 {
        Vec2::new(Avatar::WIDTH, Avatar::HEIGHT)
    }

    #[test]
    fn open_headroom_is_not_an_issue() {
        let level = level(vec![], vec![Aabb::new(0.0, 288.0, 128.0, 8.0)]);
        assert!(level.blocked_platforms(body()).is_empty());
    }

    /// One tile of clearance is 32px; the body is 34px. Two pixels short is
    /// the difference between a usable platform and a broken one.
    #[test]
    fn one_tile_of_clearance_is_not_enough() {
        let level = level(
            vec![Aabb::new(0.0, 224.0, 128.0, 32.0)], // underside at 256
            vec![Aabb::new(0.0, 288.0, 128.0, 8.0)],  // top at 288 -> 32px gap
        );
        let issues = level.blocked_platforms(body());
        assert_eq!(issues.len(), 1);
        assert!(issues[0].fully_blocked());
    }

    #[test]
    fn two_tiles_of_clearance_is_enough() {
        let level = level(
            vec![Aabb::new(0.0, 192.0, 128.0, 32.0)], // underside at 224
            vec![Aabb::new(0.0, 288.0, 128.0, 8.0)],  // 64px gap
        );
        assert!(level.blocked_platforms(body()).is_empty());
    }

    #[test]
    fn a_ceiling_over_part_of_a_platform_is_reported_as_partial() {
        let level = level(
            vec![Aabb::new(0.0, 224.0, 64.0, 32.0)], // covers the left half
            vec![Aabb::new(0.0, 288.0, 128.0, 8.0)],
        );
        let issues = level.blocked_platforms(body());
        assert_eq!(issues.len(), 1);
        assert!(!issues[0].fully_blocked());
        assert!(issues[0].standable > 0);
    }
}
