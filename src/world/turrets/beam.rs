//! The laser beam's material: the beam's glow, kept a smooth line at any range
//! (`assets/shaders/laser_beam.wesl`).

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

use crate::assets;

pub(super) type LaserMaterial = ExtendedMaterial<StandardMaterial, LaserBeam>;

/// The least width of a beam on screen, in pixels: under it the beam is widened
/// and faded in step, so a far beam reads as a faint line instead of dashes.
const LEAST_PIXELS: f32 = 1.5;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub(super) struct LaserBeam {
    /// x: the beam's radius in meters, y: its least width on screen in pixels.
    #[uniform(100)]
    beam: Vec4,
}

impl LaserBeam {
    /// A beam cylinder of `radius` m along its local Y.
    pub(super) fn new(radius: f32) -> Self {
        Self {
            beam: Vec4::new(radius, LEAST_PIXELS, 0.0, 0.0),
        }
    }
}

impl MaterialExtension for LaserBeam {
    fn vertex_shader() -> ShaderRef {
        assets::LASER_BEAM_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        assets::LASER_BEAM_SHADER.into()
    }

    // The beam is translucent: no depth prepass and no shadows.
    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}
