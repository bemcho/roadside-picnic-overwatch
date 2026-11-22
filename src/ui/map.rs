//! The Harmont map on M: the reference map (`assets::UI_MAP`) with a marker at
//! the stalker (or the car they drive), fog over every place they have not
//! surveyed yet, and a few birds wheeling over it.
//!
//! The fog is a small texture, one texel per survey cell, stretched over the
//! map; linear filtering softens its edges. It is redrawn only when the map
//! opens or the survey grows.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::text;
use crate::assets;
use crate::player::Player;
use crate::world::{
    Explored, MAP_SIZE, METERS_PER_PIXEL, SURVEY_CELL, lattice_hash, map, map_pixel,
};

/// The map's height on screen, as a share of the window's.
const HEIGHT: f32 = 78.0;
/// Fog over unsurveyed ground: a pale haze, denser in patches.
const FOG: [u8; 3] = [178, 184, 188];
const FOG_ALPHA: (f32, f32) = (0.80, 0.95);
const BIRDS: usize = 7;
const MARKER: f32 = 12.0;

#[derive(Component)]
pub(super) struct MapOverlay;

#[derive(Component)]
pub(super) struct MapFog;

#[derive(Component)]
pub(super) struct MapStalker;

#[derive(Component)]
pub(super) struct MapBird(usize);

/// The fog texture's size: one texel per survey cell across the map.
fn fog_size() -> UVec2 {
    (MAP_SIZE * METERS_PER_PIXEL / SURVEY_CELL)
        .ceil()
        .as_uvec2()
}

/// Where a world position lies on the map, as a share of its width and height.
fn on_map(at: Vec2) -> Vec2 {
    map_pixel(at) / MAP_SIZE
}

pub(super) fn spawn_map(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
) {
    let size = fog_size();
    let mut fog = Image::new_fill(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    fog.sampler = ImageSampler::linear();
    let fog = images.add(fog);
    let fill = Node {
        position_type: PositionType::Absolute,
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        ..default()
    };
    let stretched = |image| ImageNode {
        image_mode: NodeImageMode::Stretch,
        ..ImageNode::new(image)
    };
    let label = |content: &'static str, size: f32| {
        (
            Text::new(content),
            TextFont {
                font_size: FontSize::Px(size),
                ..default()
            },
            TextColor(Color::srgb(0.88, 0.86, 0.80)),
        )
    };
    commands
        .spawn((
            MapOverlay,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(8.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.01, 0.012, 0.015, 0.82)),
            GlobalZIndex(10),
            Visibility::Hidden,
        ))
        .with_children(|overlay| {
            overlay.spawn(label(text::MAP_TITLE, 26.0));
            overlay
                .spawn((
                    Node {
                        height: Val::Percent(HEIGHT),
                        aspect_ratio: Some(MAP_SIZE.x / MAP_SIZE.y),
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgb(0.66, 0.52, 0.30)),
                ))
                .with_children(|frame| {
                    frame.spawn((fill.clone(), stretched(server.load(assets::UI_MAP))));
                    frame.spawn((MapFog, fill.clone(), stretched(fog)));
                    for index in 0..BIRDS {
                        frame.spawn((
                            MapBird(index),
                            Text::new(text::MAP_BIRD[0]),
                            TextFont {
                                font_size: FontSize::Px(15.0),
                                ..default()
                            },
                            TextColor(Color::srgba(0.08, 0.08, 0.09, 0.85)),
                            Node {
                                position_type: PositionType::Absolute,
                                ..default()
                            },
                        ));
                    }
                    frame.spawn((
                        MapStalker,
                        Node {
                            position_type: PositionType::Absolute,
                            width: Val::Px(MARKER),
                            height: Val::Px(MARKER),
                            margin: UiRect::new(
                                Val::Px(-MARKER / 2.0),
                                Val::ZERO,
                                Val::Px(-MARKER / 2.0),
                                Val::ZERO,
                            ),
                            border: UiRect::all(Val::Px(2.0)),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(1.0, 0.85, 0.1)),
                        BorderColor::all(Color::srgb(0.1, 0.08, 0.02)),
                    ));
                });
            overlay.spawn(label(text::MAP_HINT, 16.0));
        });
}

pub(super) fn despawn_map(mut commands: Commands, maps: Query<Entity, With<MapOverlay>>) {
    for map in &maps {
        commands.entity(map).despawn();
    }
}

/// M opens and closes the map.
pub(super) fn toggle_map(
    keys: Res<ButtonInput<KeyCode>>,
    mut maps: Query<&mut Visibility, With<MapOverlay>>,
) {
    if !keys.just_pressed(KeyCode::KeyM) {
        return;
    }
    for mut visibility in &mut maps {
        *visibility = match *visibility {
            Visibility::Hidden => Visibility::Inherited,
            _ => Visibility::Hidden,
        };
    }
}

/// Moves the stalker's marker and redraws the fog when the map has just opened
/// or the survey has grown.
pub(super) fn update_map(
    explored: Option<Res<Explored>>,
    maps: Query<Ref<Visibility>, With<MapOverlay>>,
    player: Query<&Transform, With<Player>>,
    fog: Query<&ImageNode, With<MapFog>>,
    mut marker: Query<&mut Node, With<MapStalker>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Ok(shown) = maps.single() else {
        return;
    };
    if *shown == Visibility::Hidden {
        return;
    }
    if let (Ok(player), Ok(mut node)) = (player.single(), marker.single_mut()) {
        let at = on_map(player.translation.xz()) * 100.0;
        node.left = Val::Percent(at.x);
        node.top = Val::Percent(at.y);
    }
    let Some(explored) = explored else {
        return;
    };
    if !shown.is_changed() && !explored.is_changed() {
        return;
    }
    let Some(mut image) = fog.single().ok().and_then(|fog| images.get_mut(&fog.image)) else {
        return;
    };
    if let Some(data) = image.data.as_mut() {
        draw_fog(data, &explored);
    }
}

/// Paints haze over every texel whose middle lies on unsurveyed ground.
fn draw_fog(data: &mut [u8], explored: &Explored) {
    let size = fog_size();
    for (index, texel) in data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let cell = UVec2::new(index as u32 % size.x, index as u32 / size.x);
        let pixel = (cell.as_vec2() + 0.5) / size.as_vec2() * MAP_SIZE;
        let alpha = if explored.contains(map(pixel.x, pixel.y)) {
            0.0
        } else {
            let patch = lattice_hash(cell.x as i32, cell.y as i32, 0x3F09);
            FOG_ALPHA.0 + (FOG_ALPHA.1 - FOG_ALPHA.0) * patch
        };
        *texel = [FOG[0], FOG[1], FOG[2], (alpha * 255.0) as u8];
    }
}

/// Where bird `index` flies over the map at `seconds`, as a share of the
/// map's size, and whether its wings are up: slow loops spread over the map.
fn bird_at(index: usize, seconds: f32) -> (Vec2, bool) {
    let spread = index as f32 * 2.399_963;
    let center = Vec2::new(0.5 + 0.3 * spread.cos(), 0.5 + 0.28 * (spread * 1.7).sin());
    let angle = seconds * (0.10 + 0.03 * (index % 3) as f32) + spread;
    let at = center + Vec2::new(0.2 * angle.cos(), 0.14 * (2.0 * angle).sin());
    let wings_up = (seconds * 2.6 + index as f32 * 0.37).fract() < 0.5;
    (at.clamp(Vec2::splat(0.02), Vec2::splat(0.97)), wings_up)
}

pub(super) fn fly_map_birds(time: Res<Time>, mut birds: Query<(&MapBird, &mut Node, &mut Text)>) {
    for (bird, mut node, mut glyph) in &mut birds {
        let (at, wings_up) = bird_at(bird.0, time.elapsed_secs());
        node.left = Val::Percent(at.x * 100.0);
        node.top = Val::Percent(at.y * 100.0);
        let shown = text::MAP_BIRD[usize::from(!wings_up)];
        if glyph.0 != shown {
            glyph.0 = shown.to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_map_projection_round_trips_and_spans_the_image() {
        for at in [
            Vec2::ZERO,
            Vec2::new(840.0, -113.0),
            Vec2::new(-1500.0, 600.0),
        ] {
            let pixel = map_pixel(at);
            assert!(map(pixel.x, pixel.y).distance(at) < 1e-3);
        }
        assert_eq!(on_map(map(0.0, 0.0)), Vec2::ZERO);
        assert_eq!(on_map(map(MAP_SIZE.x, MAP_SIZE.y)), Vec2::ONE);
    }

    #[test]
    fn fog_lifts_only_over_surveyed_ground() {
        let size = fog_size();
        let mut data = vec![0; (size.x * size.y * 4) as usize];
        let mut explored = Explored::default();
        explored.survey(Vec2::new(840.0, -113.0));
        draw_fog(&mut data, &explored);
        let alpha = |at: Vec2| {
            let cell = (on_map(at) * size.as_vec2()).as_uvec2();
            data[((cell.y * size.x + cell.x) * 4 + 3) as usize]
        };
        assert_eq!(alpha(Vec2::new(840.0, -113.0)), 0);
        assert!(alpha(Vec2::new(-1200.0, 400.0)) > 190);
    }

    #[test]
    fn birds_stay_over_the_map() {
        for index in 0..BIRDS {
            for step in 0..400 {
                let (at, _) = bird_at(index, step as f32 * 0.7);
                assert!(at.min_element() >= 0.0 && at.max_element() <= 1.0);
            }
        }
    }
}
