//! The Zone: anomalies, artifacts, and per-run state.
//!
//! Anomalies are dangerous; artifacts are valuable. Both are spawned as
//! placeholder geometry for now, to be replaced with generated content.

use bevy::prelude::*;

use crate::player::{Player, Respawn};
use crate::states::{AppState, InGamePhase};
use crate::world::{Collider, Pillar, Terrain};

pub struct ZonePlugin;

/// Per-run Zone statistics. Shown in the HUD and used for scoring.
#[derive(Resource, Default)]
pub struct ZoneRun {
    pub artifacts_collected: u32,
    pub anomalies_triggered: u32,
}

/// A valuable artifact — the reason stalkers enter the Zone.
#[derive(Component)]
pub struct Artifact;

/// A dangerous anomaly — deadly to touch.
#[derive(Component)]
pub struct Anomaly;

/// Solid shapes of the placeholder artifact sphere and anomaly cylinder.
const ARTIFACT_SHAPE: &[Pillar] = &[Pillar::centered(0.2, 0.4)];
const ANOMALY_SHAPE: &[Pillar] = &[Pillar::centered(0.4, 1.2)];

/// Horizontal distance between the stalker and an anomaly's axis at which
/// it is touched: anomaly radius, body radius, and a hand's breadth.
const ANOMALY_TOUCH: f32 = 0.4 + 0.35 + 0.1;

/// Root marker for everything spawned by this plugin's OnEnter.
#[derive(Component)]
struct ZoneRoot;

impl Plugin for ZonePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ZoneRun>()
            .add_systems(OnEnter(AppState::InGame), spawn_zone_objects)
            .add_systems(OnExit(AppState::InGame), despawn_zone_objects)
            .add_systems(
                Update,
                trigger_anomalies.run_if(in_state(InGamePhase::Running)),
            );
    }
}

fn spawn_zone_objects(
    mut commands: Commands,
    terrain: Res<Terrain>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let root = commands
        .spawn((ZoneRoot, Transform::default(), Visibility::default()))
        .id();

    let artifact_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 1.0, 0.6),
        emissive: LinearRgba::rgb(2.0, 6.0, 2.0),
        ..default()
    });
    let anomaly_material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.1, 0.1),
        emissive: LinearRgba::rgb(6.0, 1.0, 1.0),
        ..default()
    });

    // Keep the flats' artifacts on the verge of the junction, not in the driving lane.
    for (x, z) in [
        (4.0, 20.0),
        (-7.0, 5.0),
        (0.0, -15.0),
        (-5.0, -4.0),
        (19.0, -3.0),
        (-75.0, -55.0), // bald hill
        (78.0, 44.0),   // quarry floor
    ] {
        let y = terrain.height_at(x, z) + 0.35;
        commands.entity(root).with_children(|zone| {
            zone.spawn((
                Artifact,
                Collider(ARTIFACT_SHAPE),
                Mesh3d(meshes.add(Sphere::new(0.2))),
                MeshMaterial3d(artifact_material.clone()),
                Transform::from_xyz(x, y, z),
            ));
        });
    }

    // A couple of placeholder anomalies: one in the flats, one at the very
    // bottom of the quarry, where the Zone is said to be meanest.
    for (x, z) in [(-13.0, 10.0), (21.0, -12.0), (70.0, 40.0)] {
        let y = terrain.height_at(x, z) + 0.6;
        commands.entity(root).with_children(|zone| {
            zone.spawn((
                Anomaly,
                Collider(ANOMALY_SHAPE),
                Mesh3d(meshes.add(Cylinder::new(0.4, 1.2))),
                MeshMaterial3d(anomaly_material.clone()),
                Transform::from_xyz(x, y, z),
            ));
        });
    }
}

/// Touching an anomaly kills the stalker, who wakes up back at base.
fn trigger_anomalies(
    mut run: ResMut<ZoneRun>,
    mut respawns: MessageWriter<Respawn>,
    players: Query<&Transform, With<Player>>,
    anomalies: Query<&GlobalTransform, With<Anomaly>>,
) {
    let Ok(player) = players.single() else {
        return;
    };
    let feet = player.translation;
    let touched = anomalies.iter().any(|anomaly| {
        let center = anomaly.translation();
        feet.xz().distance(center.xz()) <= ANOMALY_TOUCH && (feet.y - center.y).abs() < 1.5
    });
    if touched {
        run.anomalies_triggered += 1;
        respawns.write(Respawn);
    }
}

fn despawn_zone_objects(mut commands: Commands, roots: Query<Entity, With<ZoneRoot>>) {
    for root in &roots {
        commands.entity(root).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_run_starts_empty() {
        let run = ZoneRun::default();
        assert_eq!(run.artifacts_collected, 0);
        assert_eq!(run.anomalies_triggered, 0);
    }

    #[test]
    fn touching_an_anomaly_sends_the_stalker_back_to_base() {
        let mut app = App::new();
        app.init_resource::<ZoneRun>()
            .add_message::<Respawn>()
            .add_systems(Update, trigger_anomalies);
        let anomaly = GlobalTransform::from_xyz(0.0, 0.6, 0.0);
        app.world_mut().spawn((Anomaly, anomaly));
        let player = app
            .world_mut()
            .spawn((Player::default(), Transform::from_xyz(5.0, 0.0, 0.0)))
            .id();
        app.update();
        assert_eq!(app.world().resource::<ZoneRun>().anomalies_triggered, 0);

        app.world_mut()
            .get_mut::<Transform>(player)
            .expect("player")
            .translation
            .x = 0.75;
        app.update();
        assert_eq!(app.world().resource::<ZoneRun>().anomalies_triggered, 1);
        let respawns = app.world().resource::<Messages<Respawn>>();
        assert_eq!(respawns.len(), 1);
    }
}
