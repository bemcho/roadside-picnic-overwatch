//! Static collision: every solid model carries a [`Collider`] made of upright
//! [`Pillar`]s, and moving bodies are pushed out of the pillars they overlap.
//! Pillars low enough to step over, or high enough to pass under, never block.

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
