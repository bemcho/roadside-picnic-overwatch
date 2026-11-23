//! Heavy bodies leave their mark. Every [`Crusher`] (the stalker, their car and
//! the UN patrol) tramples the plants under its footprint and knocks down the
//! lighter solids it touches: trees fall like the ones that snap on their own,
//! slender marsh props topple and squat ones are pressed into the mud. Neither
//! comes back while the run lasts.

use bevy::platform::collections::HashSet;
use bevy::prelude::*;

use super::collision::{Collider, Crusher, Weight};
use super::plants::Plant;
use super::scenery::StandingTree;
use super::terrain::Terrain;
use super::tree_fall::{FallingTree, TreeBreak};

/// How far above its feet a crusher reaches.
const CRUSH_HEIGHT: f32 = 2.0;

/// The spots, to the decimeter, where plants were trampled, so a streamed tile
/// grows back without them.
#[derive(Resource, Default)]
pub(super) struct Trampled(HashSet<IVec2>);

impl Trampled {
    fn spot(at: Vec3) -> IVec2 {
        (at.xz() * 10.0).round().as_ivec2()
    }

    pub(super) fn contains(&self, at: Vec3) -> bool {
        self.0.contains(&Self::spot(at))
    }

    /// Marks the plant rooted at `at` trampled; false if it already was.
    pub(super) fn trample(&mut self, at: Vec3) -> bool {
        self.0.insert(Self::spot(at))
    }
}

pub(super) fn trample_plants(
    mut commands: Commands,
    mut trampled: ResMut<Trampled>,
    crushers: Query<(&Crusher, &Transform)>,
    plants: Query<(Entity, &Transform), With<Plant>>,
) {
    for (crusher, pose) in &crushers {
        for (plant, at) in &plants {
            let root = at.translation;
            let rise = root.y - pose.translation.y;
            if (-CRUSH_HEIGHT..CRUSH_HEIGHT).contains(&rise)
                && crusher.reaches(pose, root.xz(), 0.0)
                && trampled.trample(root)
            {
                commands.entity(plant).despawn();
            }
        }
    }
}

/// Solids that give way to something heavier, posed locally and in the world.
type LightSolids<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Collider,
        &'static Weight,
        &'static Transform,
        &'static GlobalTransform,
        Option<&'static StandingTree>,
    ),
>;

pub(super) fn knock_down_light_solids(
    mut commands: Commands,
    terrain: Res<Terrain>,
    crushers: Query<(&Crusher, &Transform)>,
    solids: LightSolids,
    mut breaks: MessageWriter<TreeBreak>,
) {
    for (entity, collider, weight, pose, placed, tree) in &solids {
        let Some((_, by)) = crushers.iter().find(|(crusher, at)| {
            weight.0 < crusher.mass && touches(**crusher, at, collider, placed)
        }) else {
            continue;
        };
        let away = (placed.translation().xz() - by.translation.xz()).normalize_or(Vec2::X);
        // Taking the collider away at once keeps it from being knocked twice.
        commands.entity(entity).remove::<(Collider, Weight)>();
        match tree {
            Some(&tree) => {
                breaks.write(TreeBreak {
                    trunk: entity,
                    tree,
                    direction: away,
                });
            }
            None => {
                let knocked = FallingTree::knocked(&terrain, *pose, collider, placed, away);
                commands.entity(entity).insert(knocked);
            }
        }
    }
}

/// Whether any of the collider's pillars meets the crusher's footprint
/// between its feet and `CRUSH_HEIGHT` above them.
fn touches(
    crusher: Crusher,
    at: &Transform,
    collider: &Collider,
    placed: &GlobalTransform,
) -> bool {
    let (scale, _, _) = placed.to_scale_rotation_translation();
    let feet = at.translation.y;
    collider.0.iter().any(|pillar| {
        let center = placed.transform_point(Vec3::new(pillar.x, 0.0, pillar.z));
        let top = center.y + pillar.top * scale.y;
        let bottom = center.y + pillar.bottom * scale.y;
        top > feet
            && bottom < feet + CRUSH_HEIGHT
            && crusher.reaches(at, center.xz(), pillar.radius * scale.x)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::collision::Pillar;

    const CAR: Crusher = Crusher {
        mass: 12_000.0,
        half_size: Vec2::new(1.5, 2.6),
    };
    const LOG: &[Pillar] = &[Pillar {
        x: 0.0,
        z: 0.0,
        radius: 0.5,
        bottom: 0.0,
        top: 0.8,
    }];

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(Terrain::new())
            .init_resource::<Trampled>()
            .add_message::<TreeBreak>()
            .add_systems(Update, (trample_plants, knock_down_light_solids));
        app
    }

    fn placed(at: Vec3) -> (Transform, GlobalTransform) {
        let pose = Transform::from_translation(at);
        (pose, GlobalTransform::from(pose))
    }

    #[test]
    fn a_car_flattens_plants_and_light_props_but_not_heavy_ones() {
        let mut app = app();
        let world = app.world_mut();
        world.spawn((CAR, Transform::from_xyz(0.0, 0.0, 0.0)));
        let under = world
            .spawn((Plant::test(), placed(Vec3::new(1.0, 0.0, 2.0)).0))
            .id();
        let beside = world
            .spawn((Plant::test(), placed(Vec3::new(3.0, 0.0, 0.0)).0))
            .id();
        let log = world
            .spawn((
                Collider(LOG),
                Weight(600.0),
                placed(Vec3::new(0.0, 0.0, 2.9)),
            ))
            .id();
        let anchored = world
            .spawn((
                Collider(LOG),
                Weight(50_000.0),
                placed(Vec3::new(0.0, 0.0, -2.9)),
            ))
            .id();
        app.update();
        let world = app.world();
        assert!(
            world.get_entity(under).is_err(),
            "plant under the car survived"
        );
        assert!(
            world.get_entity(beside).is_ok(),
            "plant beside the car was trampled"
        );
        assert!(
            world
                .resource::<Trampled>()
                .contains(Vec3::new(1.0, 0.0, 2.0))
        );
        assert!(world.get::<Collider>(log).is_none());
        assert!(world.get::<FallingTree>(log).is_some());
        assert!(world.get::<Collider>(anchored).is_some());
    }
}
