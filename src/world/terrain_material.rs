//! Tiling ground layers, blended in world space by the terrain shader.

use bevy::image::{ImageAddressMode, ImageSampler};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, TextureFormat};
use bevy::shader::ShaderRef;

use crate::assets::{self, load_repeating};

use super::roads::RoadNetwork;

pub(super) type TerrainMaterial = ExtendedMaterial<StandardMaterial, GroundLayers>;

const FOG_SETTINGS_EPSILON: f32 = 0.25;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub(super) struct GroundLayers {
    #[texture(100)]
    #[sampler(101)]
    grass_albedo: Handle<Image>,
    #[texture(102)]
    grass_normal: Handle<Image>,
    #[texture(103)]
    grass_surface: Handle<Image>,
    #[texture(104)]
    earth_albedo: Handle<Image>,
    #[texture(105)]
    earth_normal: Handle<Image>,
    #[texture(106)]
    earth_surface: Handle<Image>,
    #[texture(107)]
    road_surface: Handle<Image>,
    #[uniform(108)]
    road_bounds: Vec4,
    #[uniform(109)]
    fog_settings: Vec4,
    #[texture(110)]
    asphalt_albedo: Handle<Image>,
    #[texture(111)]
    asphalt_normal: Handle<Image>,
    #[texture(112)]
    asphalt_surface: Handle<Image>,
    #[texture(113)]
    asphalt_crack: Handle<Image>,
}

impl MaterialExtension for GroundLayers {
    fn fragment_shader() -> ShaderRef {
        assets::TERRAIN_SHADER.into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        assets::TERRAIN_SHADER.into()
    }
}

pub(super) fn load_terrain_material(
    server: &AssetServer,
    roads: &RoadNetwork,
    images: &mut Assets<Image>,
) -> TerrainMaterial {
    let load = |path, is_srgb| load_repeating(server, path, is_srgb);
    let (road_surface, road_bounds) = roads.surface_texture();
    TerrainMaterial {
        base: StandardMaterial {
            // The terrain shader applies distance fog with a low-ground mask.
            fog_enabled: false,
            perceptual_roughness: 1.0,
            ..default()
        },
        extension: GroundLayers {
            grass_albedo: load(assets::TERRAIN_GRASS_ALBEDO, true),
            grass_normal: load(assets::TERRAIN_GRASS_NORMAL, false),
            grass_surface: load(assets::TERRAIN_GRASS_SURFACE, false),
            earth_albedo: load(assets::TERRAIN_EARTH_ALBEDO, true),
            earth_normal: load(assets::TERRAIN_EARTH_NORMAL, false),
            earth_surface: load(assets::TERRAIN_EARTH_SURFACE, false),
            road_surface: images.add(road_surface),
            road_bounds,
            fog_settings: Vec4::ZERO,
            asphalt_albedo: load(assets::TERRAIN_ASPHALT_ALBEDO, true),
            asphalt_normal: load(assets::TERRAIN_ASPHALT_NORMAL, false),
            asphalt_surface: load(assets::TERRAIN_ASPHALT_SURFACE, false),
            asphalt_crack: load(assets::TERRAIN_ASPHALT_CRACK, false),
        },
    }
}

pub(super) fn update_terrain_fog_settings(
    materials: &mut Assets<TerrainMaterial>,
    handle: &Handle<TerrainMaterial>,
    fog_top: f32,
    fog_height: f32,
) {
    let Some(material) = materials.get(handle) else {
        return;
    };
    if (material.extension.fog_settings.x - fog_top).abs() < FOG_SETTINGS_EPSILON
        && (material.extension.fog_settings.y - fog_height).abs() < FOG_SETTINGS_EPSILON
    {
        return;
    }
    if let Some(mut material) = materials.get_mut(handle) {
        material.extension.fog_settings = Vec4::new(fog_top, fog_height, 0.0, 0.0);
    }
}

/// Builds mip chains for new repeating or anisotropically filtered images that arrive
/// without them: the procedural textures and small tiling PNGs. GLB textures ship with
/// precompiled mips (`tools/compress_glb_textures.py`), so they are skipped.
pub(super) fn mipmap_filtered_textures(
    mut events: MessageReader<AssetEvent<Image>>,
    mut images: ResMut<Assets<Image>>,
) {
    for event in events.read() {
        let (AssetEvent::Added { id } | AssetEvent::Modified { id }) = *event else {
            continue;
        };
        if images.get(id).is_some_and(needs_mipmaps)
            && let Some(mut image) = images.get_mut(id)
        {
            generate_mipmaps(&mut image);
        }
    }
}

fn needs_mipmaps(image: &Image) -> bool {
    image.texture_descriptor.mip_level_count == 1
        && image.data.is_some()
        && matches!(
            &image.sampler,
            ImageSampler::Descriptor(sampler)
                if sampler.address_mode_u == ImageAddressMode::Repeat
                    || sampler.anisotropy_clamp > 1
        )
}

fn generate_mipmaps(image: &mut Image) {
    let format = image.texture_descriptor.format;
    if !matches!(
        format,
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb
    ) {
        return;
    }
    let (mut width, mut height) = (image.width() as usize, image.height() as usize);
    let Some(data) = image.data.as_mut() else {
        return;
    };
    let decode: [f32; 256] = std::array::from_fn(|i| {
        let v = i as f32 / 255.0;
        if format.is_srgb() {
            LinearRgba::from(Srgba::rgb(v, v, v)).red
        } else {
            v
        }
    });
    let mut offset = 0;
    while width > 1 || height > 1 {
        let next_offset = data.len();
        let (next_width, next_height) = ((width / 2).max(1), (height / 2).max(1));
        for y in 0..next_height {
            for x in 0..next_width {
                let samples = [
                    (2 * x, 2 * y),
                    ((2 * x + 1).min(width - 1), 2 * y),
                    (2 * x, (2 * y + 1).min(height - 1)),
                    ((2 * x + 1).min(width - 1), (2 * y + 1).min(height - 1)),
                ];
                for channel in 0..4 {
                    let mut value = samples
                        .iter()
                        .map(|&(sx, sy)| {
                            let v = data[offset + (sy * width + sx) * 4 + channel];
                            if channel == 3 {
                                v as f32 / 255.0
                            } else {
                                decode[v as usize]
                            }
                        })
                        .sum::<f32>()
                        * 0.25;
                    if format.is_srgb() && channel < 3 {
                        value = Srgba::from(LinearRgba::rgb(value, value, value)).red;
                    }
                    data.push((value * 255.0).round() as u8);
                }
            }
        }
        (width, height) = (next_width, next_height);
        offset = next_offset;
        image.texture_descriptor.mip_level_count += 1;
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::{AssetPlugin, RenderAssetUsages};
    use bevy::render::render_resource::{Extent3d, TextureDimension};

    use super::*;

    /// A `const NAME: f32 = value;` of the terrain shader.
    fn shader_const(name: &str) -> f32 {
        let source = include_str!("../../assets/shaders/terrain.wesl");
        let declaration = format!("const {name}: f32 = ");
        let value = source
            .split_once(&declaration)
            .and_then(|(_, rest)| rest.split_once(';'))
            .unwrap_or_else(|| panic!("shader constant {name} is missing"))
            .0;
        value
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("shader constant {name} is not a number: {value}"))
    }

    #[test]
    fn crack_decals_fit_inside_their_cells() {
        // A decal turned any way, shifted by the jitter, must stay inside its cell, or the
        // neighbouring cell's pixels would cut it off at a straight edge.
        let (cell, size, jitter) = (
            shader_const("CRACK_CELL"),
            shader_const("CRACK_SIZE"),
            shader_const("CRACK_JITTER"),
        );
        assert!(
            size * std::f32::consts::SQRT_2 / 2.0 + jitter / 2.0 <= cell / 2.0,
            "a {size} m decal jittered by {jitter} m leaves its {cell} m cell"
        );
    }

    #[test]
    fn mipmaps_opt_in_for_anisotropic_clamped_atlases() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .add_systems(Update, mipmap_filtered_textures);
        let baseline = Image::new_fill(
            Extent3d {
                width: 3,
                height: 2,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[128, 128, 255, 255],
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::default(),
        );
        let mut atlas = baseline.clone();
        atlas.sampler = ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
            anisotropy_clamp: 8,
            ..bevy::image::ImageSamplerDescriptor::linear()
        });
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        let ordinary = images.add(baseline);
        let filtered = images.add(atlas);

        // Asset events are flushed after Update, so the system sees them a frame later.
        app.update();
        app.update();
        let images = app.world().resource::<Assets<Image>>();
        let levels = |handle| {
            images
                .get(handle)
                .expect("test image")
                .texture_descriptor
                .mip_level_count
        };
        assert_eq!(levels(&ordinary), 1);
        assert_eq!(levels(&filtered), 2);
    }

    #[test]
    fn mipmaps_average_color_in_linear_space_but_preserve_data_maps() {
        for (format, expected) in [
            (TextureFormat::Rgba8UnormSrgb, 188),
            (TextureFormat::Rgba8Unorm, 128),
        ] {
            let mut image = Image::new(
                Extent3d {
                    width: 2,
                    height: 2,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                vec![
                    0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255,
                ],
                format,
                RenderAssetUsages::default(),
            );
            generate_mipmaps(&mut image);
            assert_eq!(image.texture_descriptor.mip_level_count, 2);
            let data = image.data.expect("mip pixels");
            assert_eq!(data.len(), 20);
            assert_eq!(&data[16..], &[expected, expected, expected, 255]);
        }
    }

    #[test]
    fn mipmaps_extend_to_one_pixel_for_square_and_narrow_images() {
        for (width, height) in [(4, 4), (1, 4), (4, 1)] {
            let mut image = Image::new_fill(
                Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                &[128, 128, 255, 255],
                TextureFormat::Rgba8Unorm,
                RenderAssetUsages::default(),
            );
            generate_mipmaps(&mut image);
            assert_eq!(image.texture_descriptor.mip_level_count, 3);
            let pixels = (width * height + (width / 2).max(1) * (height / 2).max(1) + 1) as usize;
            let data = image.data.expect("mip pixels");
            assert_eq!(data.len(), pixels * 4);
            assert!(
                data.as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| *p == [128, 128, 255, 255])
            );
        }
    }
}
