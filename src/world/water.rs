use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::PrimitiveTopology;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

use crate::assets::WATER_SHADER;

use super::hydrology::WATER_LEVEL;
use super::terrain::{TerrainGrid, WaterVertex};

pub(super) type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterRipples>;

#[derive(Asset, AsBindGroup, Reflect, Clone, Debug)]
pub(super) struct WaterRipples {
    #[uniform(100)]
    wave_scale: f32,
}

impl MaterialExtension for WaterRipples {
    fn fragment_shader() -> ShaderRef {
        WATER_SHADER.into()
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

pub(super) fn material() -> WaterMaterial {
    WaterMaterial {
        base: StandardMaterial {
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.2,
            reflectance: 0.32,
            ..default()
        },
        extension: WaterRipples { wave_scale: 1.0 },
    }
}

#[derive(Clone, Copy)]
struct Vertex {
    at: Vec2,
    water: WaterVertex,
}

impl Vertex {
    fn edge(self, other: Self) -> Self {
        let t = self.water.depth / (self.water.depth - other.water.depth);
        Self {
            at: self.at.lerp(other.at, t),
            water: WaterVertex {
                depth: 0.0,
                marsh: self.water.marsh.lerp(other.water.marsh, t),
                flow: self.water.flow.lerp(other.water.flow, t),
            },
        }
    }
}

/// Clip the flat surface against the same triangles that draw the riverbed.
pub(super) fn build_tile(grid: TerrainGrid, water: &[WaterVertex]) -> Option<Mesh> {
    if water.iter().all(|v| v.depth <= 0.0) {
        return None;
    }
    let TerrainGrid {
        origin,
        spacing,
        cells,
    } = grid;
    let vertex = |x, z| Vertex {
        at: origin + Vec2::new(x as f32, z as f32) * spacing,
        water: water[z * (cells + 1) + x],
    };
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    for z in 0..cells {
        for x in 0..cells {
            let (a, b, c, d) = (
                vertex(x, z),
                vertex(x + 1, z),
                vertex(x, z + 1),
                vertex(x + 1, z + 1),
            );
            for triangle in [[a, c, b], [b, c, d]] {
                let mut clipped = [a; 4];
                let mut len = 0;
                let mut from = triangle[2];
                for to in triangle {
                    if (from.water.depth > 0.0) != (to.water.depth > 0.0) {
                        clipped[len] = from.edge(to);
                        len += 1;
                    }
                    if to.water.depth > 0.0 {
                        clipped[len] = to;
                        len += 1;
                    }
                    from = to;
                }
                for i in 1..len.saturating_sub(1) {
                    for v in [clipped[0], clipped[i], clipped[i + 1]] {
                        positions.push([v.at.x, WATER_LEVEL, v.at.y]);
                        colors.push([v.water.depth, v.water.marsh, v.water.flow.x, v.water.flow.y]);
                    }
                }
            }
        }
    }
    let normals = vec![Vec3::Y.to_array(); positions.len()];
    Some(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors),
    )
}

pub(super) fn surface(
    mesh: Handle<Mesh>,
    material: &Handle<WaterMaterial>,
) -> (Mesh3d, MeshMaterial3d<WaterMaterial>, NotShadowCaster) {
    (
        Mesh3d(mesh),
        MeshMaterial3d(material.clone()),
        NotShadowCaster,
    )
}
