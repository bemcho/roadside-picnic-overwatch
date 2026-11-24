//! Creatures a Perimeter gun brought down: they tip onto their side, lie a
//! while, sink out of sight and later return to the start of their route as a
//! fresh animal, so the belt along the wall stays populated.

use bevy::prelude::*;

use super::Terrain;
use super::movement::{Wildlife, smooth};
use super::mutants::Mutant;

/// Seconds to tip over, to lie on the ground and to sink out of sight.
const FALL: f32 = 0.6;
const LIE: f32 = 8.0;
const SINK: f32 = 2.5;
/// Seconds after the hit when the animal returns to its route.
const RETURN: f32 = 45.0;
/// How far it rolls, short of lying flat, in radians.
const ROLL: f32 = 1.45;

/// A creature brought down: it stops moving and posing until it returns.
#[derive(Component, Clone, Copy, Debug)]
pub(in crate::world) struct Downed {
    elapsed: f32,
    /// 1 tips it onto its right side (model +X), -1 onto its left.
    side: f32,
    /// Where it stood when hit, taken on its first downed frame.
    rest: Option<Transform>,
}

impl Downed {
    pub(in crate::world) fn new(side: f32) -> Self {
        Self {
            elapsed: 0.0,
            side: side.signum(),
            rest: None,
        }
    }
}

/// How far it has tipped over and sunk, each from 0 to 1, `elapsed` seconds after the hit.
fn fall(elapsed: f32) -> (f32, f32) {
    (
        smooth(elapsed / FALL),
        smooth((elapsed - FALL - LIE) / SINK),
    )
}

/// Downed creatures, with what returns them to their route.
type Fallen<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut Downed,
        &'static mut Transform,
        &'static mut Wildlife,
        Option<&'static mut Mutant>,
        &'static mut Visibility,
    ),
>;

/// Tips downed creatures over and sinks them, then sends them back to their route.
pub(in crate::world) fn fell_downed(
    mut commands: Commands,
    time: Res<Time>,
    terrain: Res<Terrain>,
    mut downed: Fallen,
) {
    let dt = time.delta_secs();
    for (entity, mut down, mut pose, mut motion, mutant, mut visibility) in &mut downed {
        let rest = *down.rest.get_or_insert(*pose);
        down.elapsed += dt;
        if down.elapsed >= RETURN {
            *motion = motion.reborn();
            if let Some(mut mutant) = mutant {
                *mutant = mutant.reborn(motion.kind);
            }
            *pose = motion.initial_pose(&terrain);
            *visibility = Visibility::Inherited;
            commands.entity(entity).remove::<Downed>();
            continue;
        }
        let (tip, sink) = fall(down.elapsed);
        // The origin is at its feet: lift it by about half its girth as it rolls.
        let size = rest.scale.x;
        pose.rotation = rest.rotation * Quat::from_rotation_z(-down.side * ROLL * tip);
        pose.translation = rest.translation + Vec3::Y * size * (0.12 * tip - 0.9 * sink);
        if sink >= 1.0 {
            *visibility = Visibility::Hidden;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::super::{Kind, Route};
    use super::*;

    #[test]
    fn a_fall_tips_lies_sinks_in_that_order() {
        assert_eq!(fall(0.0), (0.0, 0.0));
        assert_eq!(fall(FALL), (1.0, 0.0));
        assert_eq!(fall(FALL + LIE).1, 0.0);
        assert!(fall(FALL + LIE + SINK * 0.5).1 > 0.4);
        assert_eq!(fall(FALL + LIE + SINK), (1.0, 1.0));
    }

    #[test]
    fn a_downed_deer_falls_on_its_side_vanishes_and_returns_to_its_route() {
        let terrain = Terrain::new();
        let route = Route {
            center: Vec2::new(-400.0, 300.0),
            radius: Vec2::new(5.2, 4.0),
            phase: 0.7,
            speed: 0.15,
        };
        let deer = Wildlife::new(Kind::Hexadeer, route, None);
        let start = deer.initial_pose(&terrain);
        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .insert_resource(terrain.clone())
            .add_systems(Update, fell_downed);
        let mut moved = start;
        moved.translation += Vec3::new(3.0, 0.0, 2.0);
        let id = app
            .world_mut()
            .spawn((moved, deer, Visibility::default(), Downed::new(1.0)))
            .id();
        let step = |seconds: f32, app: &mut App| {
            for _ in 0..(seconds / 0.05).round() as u32 {
                app.world_mut()
                    .resource_mut::<Time>()
                    .advance_by(Duration::from_millis(50));
                app.update();
            }
        };
        step(1.0, &mut app);
        let pose = *app.world().get::<Transform>(id).expect("deer");
        let up = pose.rotation * Vec3::Y;
        assert!(up.y < 0.3, "the deer lies on its side: up {up}");
        assert!(app.world().get::<Downed>(id).is_some());
        step(FALL + LIE + SINK, &mut app);
        assert_eq!(app.world().get::<Visibility>(id), Some(&Visibility::Hidden));
        step(RETURN, &mut app);
        let world = app.world();
        assert!(world.get::<Downed>(id).is_none());
        assert_eq!(world.get::<Visibility>(id), Some(&Visibility::Inherited));
        let back = world.get::<Transform>(id).expect("deer");
        assert!(back.translation.distance(start.translation) < 0.01);
        assert!((back.rotation * Vec3::Y).y > 0.99);
    }
}
