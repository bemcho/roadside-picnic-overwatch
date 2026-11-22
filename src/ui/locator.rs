//! A yellow ring on the ground around the stalker, or the car they drive, so
//! they stay easy to find at every zoom. It hugs the rendered terrain (or the
//! water surface when wading), never shrinks below `MIN_DIAMETER` pixels on
//! screen, and pulses while the stalker stands still.

use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::color::palettes::css::GOLD;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::player::vehicle::Driving;
use crate::player::{Player, PlayerCamera};
use crate::states::AppState;
use crate::world::Terrain;

/// Vertices around the ring.
const SEGMENTS: usize = 96;
/// Ring radius and line width in meters, around the stalker and around the car.
const ON_FOOT: (f32, f32) = (0.75, 0.08);
const DRIVING: (f32, f32) = (3.6, 0.2);
/// Smallest ring diameter and line width on screen, in pixels.
const MIN_DIAMETER: f32 = 34.0;
const MIN_WIDTH: f32 = 3.0;
/// Height above the ground in meters (the asphalt slabs lie 8 cm up), plus
/// this many screen pixels' worth so coarse distant tiles don't swallow it.
const LIFT: f32 = 0.1;
const LIFT_PIXELS: f32 = 1.5;
/// Constant depth bias. On a float depth buffer it is relative to depth: this
/// pulls the ring 1–2 % of its distance toward the camera, over the road
/// slabs and stones it lies among but behind the stalker standing in it.
const DEPTH_BIAS: f32 = 84_000.0;
/// Below this ground speed, in m/s, the stalker counts as standing still.
const STILL_SPEED: f32 = 0.15;
/// One pulse, in seconds, and how far the ring swells and fades at its peak.
const PULSE_PERIOD: f32 = 1.4;
const PULSE_SWELL: f32 = 0.2;
const PULSE_FADE: f32 = 0.55;
/// How quickly the pulse sets in when the stalker stops, per second.
const PULSE_RATE: f32 = 4.0;

#[derive(Component, Default)]
pub(super) struct Locator {
    /// The ring's center last frame, to tell whether the stalker moves.
    last: Option<Vec2>,
    /// 0 while the stalker moves, easing to 1 while they stand still.
    stillness: f32,
}

impl Locator {
    /// Eases the pulse in while the center stays put and out while it moves.
    fn track(&mut self, center: Vec2, dt: f32) {
        if dt <= 0.0 {
            return; // paused
        }
        let speed = self.last.map_or(0.0, |last| last.distance(center) / dt);
        self.last = Some(center);
        let target = if speed < STILL_SPEED { 1.0 } else { 0.0 };
        self.stillness += (target - self.stillness) * (1.0 - (-PULSE_RATE * dt).exp());
    }

    /// Radius scale and opacity of the ring at `seconds`.
    fn pulse(&self, seconds: f32) -> (f32, f32) {
        let beat = self.stillness * (0.5 - 0.5 * (seconds * TAU / PULSE_PERIOD).cos());
        (1.0 + PULSE_SWELL * beat, 1.0 - PULSE_FADE * beat)
    }
}

pub(super) fn spawn_locator(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let corners = (0..SEGMENTS as u32).flat_map(|i| {
        let next = (i + 1) % SEGMENTS as u32;
        let (inner, outer, next_inner, next_outer) = (2 * i, 2 * i + 1, 2 * next, 2 * next + 1);
        [inner, next_inner, outer, outer, next_inner, next_outer]
    });
    let ring = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3]; 2 * SEGMENTS])
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 2 * SEGMENTS])
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0; 4]; 2 * SEGMENTS])
    .with_inserted_indices(Indices::U32(corners.collect()));
    commands.spawn((
        Locator::default(),
        Mesh3d(meshes.add(ring)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: GOLD.into(),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            fog_enabled: false,
            depth_bias: DEPTH_BIAS,
            ..default()
        })),
        Transform::IDENTITY,
        Visibility::Hidden,
        // The vertices move every frame; one small mesh isn't worth culling.
        NoFrustumCulling,
        NotShadowCaster,
        NotShadowReceiver,
        DespawnOnExit(AppState::InGame),
    ));
}

/// Lays the ring on the ground around the stalker each frame.
pub(super) fn update_locator(
    time: Res<Time>,
    terrain: Res<Terrain>,
    player: Query<(&Transform, Has<Driving>), With<Player>>,
    camera: Single<(&Camera, &GlobalTransform), With<PlayerCamera>>,
    ring: Single<(&mut Locator, &Mesh3d, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let (camera, eye) = *camera;
    let (mut locator, mesh, mut visibility) = ring.into_inner();
    let (Ok((pose, driving)), Some(mut mesh)) = (player.single(), meshes.get_mut(&mesh.0)) else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    let center = pose.translation.xz();
    locator.track(center, time.delta_secs());
    let (swell, alpha) = locator.pulse(time.elapsed_secs());
    // Screen pixels per meter at the stalker; off screen, the ring keeps its size.
    let pixels = pixels_per_meter(camera, eye, pose.translation).unwrap_or(f32::INFINITY);
    let (radius, width) = if driving { DRIVING } else { ON_FOOT };
    let radius = radius.max(0.5 * MIN_DIAMETER / pixels) * swell;
    let width = width.max(MIN_WIDTH / pixels);
    let lift = LIFT + LIFT_PIXELS / pixels.max(1.0);
    let ground = |at: Vec2| terrain.mesh_height_at(at) + terrain.water_depth_at(at.x, at.y);
    let positions = ring_vertices(center, radius, width, |at| ground(at) + lift);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        vec![[1.0, 1.0, 1.0, alpha]; 2 * SEGMENTS],
    );
    visibility.set_if_neq(Visibility::Inherited);
}

/// How many screen pixels one meter across the view spans at `point`.
fn pixels_per_meter(camera: &Camera, eye: &GlobalTransform, point: Vec3) -> Option<f32> {
    let middle = camera.world_to_viewport(eye, point).ok()?;
    let side = camera
        .world_to_viewport(eye, point + eye.right() * 1.0)
        .ok()?;
    Some(middle.distance(side)).filter(|&pixels| pixels > 0.0)
}

/// Inner and outer edge vertices, in turn, around a ring standing on `height`.
fn ring_vertices(
    center: Vec2,
    radius: f32,
    width: f32,
    height: impl Fn(Vec2) -> f32,
) -> Vec<[f32; 3]> {
    (0..SEGMENTS)
        .flat_map(|i| {
            let toward = Vec2::from_angle(i as f32 * TAU / SEGMENTS as f32);
            [radius - 0.5 * width, radius + 0.5 * width].map(|r| {
                let at = center + toward * r;
                [at.x, height(at), at.y]
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_follows_the_ground_around_its_center() {
        let center = Vec2::new(40.0, -12.0);
        let slope = |at: Vec2| 0.1 * at.x + 2.0;
        let vertices = ring_vertices(center, 3.0, 0.2, slope);
        assert_eq!(vertices.len(), 2 * SEGMENTS);
        for (i, [x, y, z]) in vertices.into_iter().enumerate() {
            let at = Vec2::new(x, z);
            let expected = if i % 2 == 0 { 2.9 } else { 3.1 };
            assert!((at.distance(center) - expected).abs() < 1e-4);
            assert!((y - slope(at)).abs() < 1e-4);
        }
    }

    #[test]
    fn the_ring_pulses_only_while_the_stalker_stands_still() {
        let mut locator = Locator::default();
        let dt = 1.0 / 60.0;
        let mut at = Vec2::ZERO;
        for _ in 0..120 {
            at.x += 3.0 * dt;
            locator.track(at, dt);
        }
        assert!(locator.stillness < 0.01);
        let (swell, alpha) = locator.pulse(0.5 * PULSE_PERIOD);
        assert!(swell < 1.01 && alpha > 0.99);

        for _ in 0..120 {
            locator.track(at, dt);
        }
        assert!(locator.stillness > 0.99);
        let (rest, _) = locator.pulse(0.0);
        let (peak, faded) = locator.pulse(0.5 * PULSE_PERIOD);
        assert!((rest - 1.0).abs() < 1e-3);
        assert!(peak > 1.15 && faded < 0.5);
    }

    #[test]
    fn pausing_holds_the_pulse() {
        let mut locator = Locator {
            last: Some(Vec2::ZERO),
            stillness: 0.4,
        };
        locator.track(Vec2::new(5.0, 0.0), 0.0);
        assert_eq!(locator.stillness, 0.4);
        assert_eq!(locator.last, Some(Vec2::ZERO));
    }
}
