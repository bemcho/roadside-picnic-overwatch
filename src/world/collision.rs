//! Static collision: every solid model carries a [`Collider`] made of upright
//! [`Pillar`]s, and moving bodies are pushed out of the pillars they overlap.
//! Pillars low enough to step over, or high enough to pass under, never block.
//!
//! A collider with a [`Weight`] gives way to a heavier [`Crusher`] (the stalker,
//! a car), which runs through it and knocks it down. The stalker climbs solids
//! and stands on their pillars' tops, except [`Unclimbable`] ones (the cordon
//! walls and checkpoints, trees, guards), which block at any height; a body
//! that fits under an unclimbable pillar's bottom still passes below it, as
//! the crawling stalker does through the breach under the Perimeter wall.

use bevy::prelude::*;

/// Highest obstacle a body walks straight over, in meters.
pub const STEP_HEIGHT: f32 = 0.4;

/// An upright cylinder in its collider's local space (scaled with it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pillar {
    pub x: f32,
    pub z: f32,
    pub radius: f32,
    pub bottom: f32,
    pub top: f32,
}

impl Pillar {
    /// A single pillar centered on its entity, `height` tall.
    pub const fn centered(radius: f32, height: f32) -> Self {
        Self {
            x: 0.0,
            z: 0.0,
            radius,
            bottom: -height / 2.0,
            top: height / 2.0,
        }
    }
}

/// The solid shape of a model, posed by the entity's `GlobalTransform`.
#[derive(Component, Clone, Copy, Debug)]
pub struct Collider(pub &'static [Pillar]);

/// What a solid object weighs, in kilograms. A heavier [`Crusher`] runs
/// through it and knocks it down; a collider without one is anchored and never
/// gives way.
#[derive(Component, Clone, Copy, Debug)]
pub struct Weight(pub f32);

/// A collider nothing climbs and no height clears, though a body small enough
/// passes under it: the cordon walls and checkpoints, trees, guards.
#[derive(Component, Default)]
pub struct Unclimbable;

/// A body moving through the world: it tramples the plants under its footprint
/// (a box `half_size` meters either side of its middle, turned with it) and
/// knocks down colliders lighter than `mass` kilograms that it touches.
#[derive(Component, Clone, Copy, Debug)]
pub struct Crusher {
    pub mass: f32,
    pub half_size: Vec2,
}

impl Crusher {
    /// Whether a collider weighing `weight` stands in this body's way.
    pub fn blocked_by(self, weight: Option<&Weight>) -> bool {
        weight.is_none_or(|weight| weight.0 >= self.mass)
    }

    /// Whether a circle of `radius` at `point` overlaps the footprint of the
    /// body posed at `pose`.
    pub fn reaches(self, pose: &Transform, point: Vec2, radius: f32) -> bool {
        let offset = point - pose.translation.xz();
        if offset.length_squared() > (self.half_size.length() + radius).powi(2) {
            return false;
        }
        let (yaw, _, _) = pose.rotation.to_euler(EulerRot::YXZ);
        let local = Vec2::from_angle(yaw).rotate(offset);
        let outside = (local.abs() - self.half_size).max(Vec2::ZERO);
        outside.length_squared() <= radius * radius
    }
}

/// A moving body: an upright cylinder standing at its feet.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub radius: f32,
    pub height: f32,
}

/// Pushes `body`, with feet at `feet` and standing at `point` on the ground
/// plane, out of every pillar it bumps into.
pub fn push_out<'a>(
    point: Vec2,
    feet: f32,
    body: Body,
    colliders: impl IntoIterator<Item = (&'a Collider, &'a GlobalTransform)>,
) -> Vec2 {
    colliders
        .into_iter()
        .fold(point, |point, (collider, transform)| {
            let (scale, _, _) = transform.to_scale_rotation_translation();
            collider.0.iter().fold(point, |point, pillar| {
                let center = transform.transform_point(Vec3::new(pillar.x, 0.0, pillar.z));
                let top = center.y + pillar.top * scale.y;
                let bottom = center.y + pillar.bottom * scale.y;
                if top <= feet + STEP_HEIGHT || bottom >= feet + body.height {
                    return point;
                }
                separate(point, center.xz(), pillar.radius * scale.x + body.radius)
            })
        })
}

/// Keeps `body`, with feet at `feet` and standing at `point`, out of every
/// pillar however high it stands, unless it fits under the pillar's bottom.
pub fn fence_out<'a>(
    point: Vec2,
    feet: f32,
    body: Body,
    colliders: impl IntoIterator<Item = (&'a Collider, &'a GlobalTransform)>,
) -> Vec2 {
    colliders
        .into_iter()
        .fold(point, |point, (collider, transform)| {
            let (scale, _, _) = transform.to_scale_rotation_translation();
            collider.0.iter().fold(point, |point, pillar| {
                let center = transform.transform_point(Vec3::new(pillar.x, 0.0, pillar.z));
                if center.y + pillar.bottom * scale.y >= feet + body.height {
                    return point;
                }
                separate(point, center.xz(), pillar.radius * scale.x + body.radius)
            })
        })
}

/// The highest pillar top a body of `radius` at `point`, feet at `feet`,
/// stands on: one under its footprint that it has climbed to within a step of.
pub fn support_height<'a>(
    point: Vec2,
    feet: f32,
    radius: f32,
    colliders: impl IntoIterator<Item = (&'a Collider, &'a GlobalTransform)>,
) -> Option<f32> {
    colliders
        .into_iter()
        .flat_map(|(collider, transform)| {
            let (scale, _, _) = transform.to_scale_rotation_translation();
            collider.0.iter().filter_map(move |pillar| {
                let center = transform.transform_point(Vec3::new(pillar.x, 0.0, pillar.z));
                let top = center.y + pillar.top * scale.y;
                let reach = pillar.radius * scale.x + radius;
                let under = point.distance_squared(center.xz()) < reach * reach;
                (under && top <= feet + STEP_HEIGHT).then_some(top)
            })
        })
        .reduce(f32::max)
}

/// The top of the highest pillar that blocks `body` at `point` (feet at
/// `feet`) and lies ahead of it, toward `toward`: what it climbs while
/// pressing on.
pub fn climb_top<'a>(
    point: Vec2,
    feet: f32,
    body: Body,
    toward: Vec2,
    colliders: impl IntoIterator<Item = (&'a Collider, &'a GlobalTransform)>,
) -> Option<f32> {
    colliders
        .into_iter()
        .flat_map(|(collider, transform)| {
            let (scale, _, _) = transform.to_scale_rotation_translation();
            collider.0.iter().filter_map(move |pillar| {
                let center = transform.transform_point(Vec3::new(pillar.x, 0.0, pillar.z));
                let top = center.y + pillar.top * scale.y;
                let bottom = center.y + pillar.bottom * scale.y;
                let offset = center.xz() - point;
                let reach = pillar.radius * scale.x + body.radius;
                let blocks = top > feet + STEP_HEIGHT && bottom < feet + body.height;
                let ahead = offset.dot(toward) > 0.7 * offset.length();
                (blocks && ahead && offset.length_squared() < reach * reach).then_some(top)
            })
        })
        .reduce(f32::max)
}

/// Moves `point` to at least `reach` from `center`.
fn separate(point: Vec2, center: Vec2, reach: f32) -> Vec2 {
    let offset = point - center;
    let distance = offset.length();
    if distance >= reach {
        point
    } else if distance > f32::EPSILON {
        center + offset / distance * reach
    } else {
        center + Vec2::X * reach
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    const BODY: Body = Body {
        radius: 0.3,
        height: 1.8,
    };
    const TRUNK: &[Pillar] = &[Pillar {
        x: 0.0,
        z: 0.0,
        radius: 0.05,
        bottom: 0.0,
        top: 1.0,
    }];
    const LOG: &[Pillar] = &[Pillar {
        top: 0.02,
        ..TRUNK[0]
    }];
    const BRANCH: &[Pillar] = &[Pillar {
        bottom: 0.1,
        ..TRUNK[0]
    }];

    /// A pillar scaled ten times, at (10, 0, 0).
    fn posed(pillars: &'static [Pillar]) -> (Collider, GlobalTransform) {
        let transform = Transform::from_xyz(10.0, 0.0, 0.0).with_scale(Vec3::splat(10.0));
        (Collider(pillars), GlobalTransform::from(transform))
    }

    #[test]
    fn walls_fence_bodies_out_at_any_height_and_tops_carry_climbers() {
        let (collider, transform) = posed(TRUNK);
        // Ten meters up, far above the 10 m pillar's top, it still fences out.
        let fenced = fence_out(Vec2::new(10.2, 0.0), 10.0, BODY, [(&collider, &transform)]);
        assert!((fenced.distance(Vec2::new(10.0, 0.0)) - 0.8).abs() < 1e-5);
        // A body that fits under a raised pillar passes below it.
        let (raised, pose) = posed(BRANCH);
        let crawler = Body {
            radius: 0.3,
            height: 0.5,
        };
        let under = Vec2::new(10.2, 0.0);
        assert_eq!(fence_out(under, 0.0, crawler, [(&raised, &pose)]), under);
        assert_ne!(fence_out(under, 0.0, BODY, [(&raised, &pose)]), under);
        // Its top carries a climber who reached it, under any part of the
        // body's footprint, but not one still below.
        let solid = || [(&collider, &transform)];
        assert_eq!(
            support_height(Vec2::new(10.7, 0.0), 9.9, 0.3, solid()),
            Some(10.0)
        );
        assert_eq!(
            support_height(Vec2::new(10.1, 0.0), 5.0, 0.3, solid()),
            None
        );
        assert_eq!(
            support_height(Vec2::new(12.0, 0.0), 10.0, 0.3, solid()),
            None
        );
    }

    #[test]
    fn a_body_climbs_only_what_blocks_it_ahead() {
        let (collider, transform) = posed(TRUNK);
        let solid = || [(&collider, &transform)];
        let body = Body {
            radius: 0.3,
            height: 1.8,
        };
        let at = Vec2::new(9.3, 0.0);
        assert_eq!(climb_top(at, 0.0, body, Vec2::X, solid()), Some(10.0));
        assert_eq!(climb_top(at, 0.0, body, Vec2::NEG_X, solid()), None);
        assert_eq!(climb_top(at, 0.0, body, Vec2::Y, solid()), None);
        // Once within a step of its top, the body walks on instead.
        assert_eq!(climb_top(at, 9.7, body, Vec2::X, solid()), None);
    }

    #[test]
    fn heavier_crushers_run_through_lighter_colliders_they_touch() {
        let car = Crusher {
            mass: 12_000.0,
            half_size: Vec2::new(1.5, 2.6),
        };
        assert!(!car.blocked_by(Some(&Weight(900.0))));
        assert!(car.blocked_by(Some(&Weight(20_000.0))));
        assert!(car.blocked_by(None));
        // Turned a quarter, the long footprint lies along X.
        let pose = Transform::from_rotation(Quat::from_rotation_y(FRAC_PI_2));
        assert!(car.reaches(&pose, Vec2::new(2.9, 0.0), 0.4));
        assert!(!car.reaches(&pose, Vec2::new(0.0, 2.9), 0.4));
        assert!(car.reaches(&Transform::IDENTITY, Vec2::new(0.0, 2.9), 0.4));
    }

    #[test]
    fn scaled_pillars_push_bodies_out_but_leave_clear_ground_alone() {
        let (collider, transform) = posed(TRUNK);
        let pushed = push_out(Vec2::new(10.2, 0.0), 0.0, BODY, [(&collider, &transform)]);
        assert!((pushed.distance(Vec2::new(10.0, 0.0)) - 0.8).abs() < 1e-5);
        let clear = Vec2::new(5.0, 5.0);
        assert_eq!(push_out(clear, 0.0, BODY, [(&collider, &transform)]), clear);
    }

    #[test]
    fn bodies_step_over_low_logs_and_pass_under_high_branches() {
        let inside = Vec2::new(10.1, 0.0);
        let (collider, transform) = posed(LOG);
        assert_eq!(
            push_out(inside, 0.0, BODY, [(&collider, &transform)]),
            inside
        );
        // Jumping clears a trunk's top.
        let (collider, transform) = posed(TRUNK);
        assert_eq!(
            push_out(inside, 9.8, BODY, [(&collider, &transform)]),
            inside
        );
        // A branch 1 m up stops a standing body; a crawling one slips under.
        let (collider, transform) = posed(BRANCH);
        assert_ne!(
            push_out(inside, 0.0, BODY, [(&collider, &transform)]),
            inside
        );
        let crawling = Body {
            height: 0.5,
            ..BODY
        };
        assert_eq!(
            push_out(inside, 0.0, crawling, [(&collider, &transform)]),
            inside
        );
    }
}
