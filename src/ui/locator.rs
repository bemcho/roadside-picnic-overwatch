//! A yellow ring around the stalker, or the car they drive, so they stay easy
//! to find from the tactical camera. It fades in as the camera pulls away and
//! never shrinks below `MIN_DIAMETER` pixels.

use bevy::color::palettes::css::GOLD;
use bevy::prelude::*;

use crate::player::vehicle::Driving;
use crate::player::{Player, PlayerCamera};

const MIN_DIAMETER: f32 = 34.0;
const BORDER: f32 = 3.0;
/// The ring fades in between these camera distances, in meters.
const FADE_IN: (f32, f32) = (18.0, 45.0);
/// The ring's center above the stalker's feet and its radius, on foot and driving.
const ON_FOOT: (f32, f32) = (0.9, 1.1);
const DRIVING: (f32, f32) = (1.2, 3.6);

#[derive(Component)]
pub(super) struct Locator;

pub(super) fn spawn_locator(hud: &mut ChildSpawnerCommands) {
    hud.spawn((
        Locator,
        Visibility::Hidden,
        Node {
            position_type: PositionType::Absolute,
            border: UiRect::all(Val::Px(BORDER)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BorderColor::all(Color::NONE),
    ));
}

/// Centers the ring on the stalker's projected position each frame.
pub(super) fn update_locator(
    player: Query<(&GlobalTransform, Has<Driving>), With<Player>>,
    camera: Single<(&Camera, &GlobalTransform), With<PlayerCamera>>,
    ring: Single<(&mut Node, &mut Visibility, &mut BorderColor), With<Locator>>,
) {
    let (camera, eye) = *camera;
    let (mut node, mut visibility, mut color) = ring.into_inner();
    let placed = player.single().ok().and_then(|(pose, driving)| {
        let (lift, radius) = if driving { DRIVING } else { ON_FOOT };
        let center = pose.translation() + Vec3::Y * lift;
        let distance = eye.translation().distance(center);
        let alpha = ((distance - FADE_IN.0) / (FADE_IN.1 - FADE_IN.0)).clamp(0.0, 1.0);
        let middle = camera.world_to_viewport(eye, center).ok()?;
        let edge = camera
            .world_to_viewport(eye, center + eye.right() * radius)
            .ok()?;
        (alpha > 0.0).then_some((middle, middle.distance(edge), alpha))
    });
    let Some((middle, radius, alpha)) = placed else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    let diameter = (2.0 * radius).max(MIN_DIAMETER);
    node.left = Val::Px(middle.x - diameter / 2.0);
    node.top = Val::Px(middle.y - diameter / 2.0);
    node.width = Val::Px(diameter);
    node.height = Val::Px(diameter);
    *color = BorderColor::all(GOLD.with_alpha(alpha));
    visibility.set_if_neq(Visibility::Inherited);
}
