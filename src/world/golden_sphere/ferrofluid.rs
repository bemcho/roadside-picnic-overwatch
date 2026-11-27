//! The ferrofluid pool under the Golden Sphere: a disc built once, its spikes
//! raised in the vertex shader (`assets/shaders/ferrofluid.wesl`) from the
//! field the Sphere's hover puts on it. A crown of tall spikes stands under the
//! Sphere; rings of small ones creep in from the rim and sink into it.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

use crate::assets;
use crate::assets::gpu_scans::TierLevel;
use crate::world::sites::SPHERE_POOL_RADIUS;

use super::{Hover, WEAKEST_FIELD};

pub(super) type FerrofluidMaterial = ExtendedMaterial<StandardMaterial, Ferrofluid>;

/// The tallest spike with the Sphere at the top and at the bottom of its bob
/// (the user's 1 to 1.4 m), and the spacing of the crown's hexagonal lattice,
/// in metres.
const SHORTEST_TALLEST: f32 = 1.0;
const TALLEST: f32 = 1.4;
const SPIKE_SPACING: f32 = 0.45;
/// The width of the field's peak under the Sphere (a Gaussian's sigma) and of
/// the rim where the liquid bends down to the floor and the spikes die away.
const PEAK_SIGMA: f32 = 0.35;
const RIM: f32 = 0.4;
/// The small spikes' rings: the seconds one takes from the rim in to the crown,
/// where it starts and ends (metres from the middle), and the spikes' heights
/// (the user's 20 to 40 cm).
const RING_SECONDS: f32 = 9.0;
const RING_START: f32 = 1.2;
const RING_END: f32 = 0.55;
const SMALL_SPIKE: [f32; 2] = [0.2, 0.4];
const RINGS: usize = 3;
/// Seconds after which the small spikes' pulse clock wraps: each spike pulses
/// a whole number of times in it (`ferrofluid.wesl`), so the wrap never shows.
const PULSE_WRAP: f32 = 60.0;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub(super) struct Ferrofluid {
    /// x: the field (1 with the Sphere lowest, a third with it highest), y: the
    /// lattice's turn in radians, z: the tallest spike's height, w: their spacing.
    #[uniform(100)]
    pub field: Vec4,
    /// x: the pool's radius, y: its rim's width, z: the peak's sigma.
    #[uniform(101)]
    shape: Vec4,
    /// How far each of the three rings of small spikes has come from the rim
    /// in to the crown (0..1); w: the seconds of their pulse, wrapping.
    #[uniform(102)]
    pub rings: Vec4,
    /// x: where the rings start, y: where they end (metres from the middle),
    /// z, w: the small spikes' least and greatest height.
    #[uniform(103)]
    flow: Vec4,
}

impl Ferrofluid {
    /// The liquid as the Sphere's `hover` pulls it.
    pub fn under(hover: Hover) -> Self {
        let strength = (hover.field - WEAKEST_FIELD) / (1.0 - WEAKEST_FIELD);
        let tallest = SHORTEST_TALLEST + (TALLEST - SHORTEST_TALLEST) * strength;
        let mut rings = Vec4::W * hover.seconds.rem_euclid(PULSE_WRAP);
        for ring in 0..RINGS {
            let stagger = ring as f32 / RINGS as f32;
            rings[ring] = (hover.seconds / RING_SECONDS + stagger).rem_euclid(1.0);
        }
        Self {
            field: Vec4::new(hover.field, hover.spin, tallest, SPIKE_SPACING),
            shape: Vec4::new(SPHERE_POOL_RADIUS, RIM, PEAK_SIGMA, 0.0),
            rings,
            flow: Vec4::new(RING_START, RING_END, SMALL_SPIKE[0], SMALL_SPIKE[1]),
        }
    }

    /// The pool: a disc of `SPHERE_POOL_RADIUS` about its centre at the
    /// liquid's level, on a grid fine enough for the spikes (half as fine on
    /// modest GPUs, twice on exceptional ones), its rim bent down to the floor:
    /// `floor` gives the floor's height under a point, relative to the liquid.
    pub fn pool_mesh(level: TierLevel, floor: impl Fn(Vec2) -> f32) -> Mesh {
        let step = match level {
            TierLevel::Modest => 0.02,
            TierLevel::Strong => 0.01,
            TierLevel::Exceptional => 0.005,
        };
        let cells = (2.0 * SPHERE_POOL_RADIUS / step).ceil() as usize;
        let corner = |i: usize, j: usize| {
            Vec2::new(i as f32, j as f32) * step - Vec2::splat(SPHERE_POOL_RADIUS)
        };
        let mut index = vec![None; (cells + 1) * (cells + 1)];
        let mut positions: Vec<[f32; 3]> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut vertex = |i: usize, j: usize, positions: &mut Vec<[f32; 3]>| {
            *index[j * (cells + 1) + i].get_or_insert_with(|| {
                // Corners past the edge are pulled onto it, rounding the disc.
                let at = corner(i, j).clamp_length_max(SPHERE_POOL_RADIUS);
                let bend = smoothstep(SPHERE_POOL_RADIUS - RIM, SPHERE_POOL_RADIUS, at.length());
                positions.push([at.x, floor(at).min(0.0) * bend, at.y]);
                positions.len() as u32 - 1
            })
        };
        for j in 0..cells {
            for i in 0..cells {
                let inside = [(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)]
                    .iter()
                    .any(|&(i, j)| corner(i, j).length() < SPHERE_POOL_RADIUS);
                if !inside {
                    continue;
                }
                let [a, b, c, d] = [(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)]
                    .map(|(i, j)| vertex(i, j, &mut positions));
                indices.extend([a, c, b, b, c, d]);
            }
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_indices(Indices::U32(indices));
        mesh.compute_smooth_normals();
        mesh
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl MaterialExtension for Ferrofluid {
    fn vertex_shader() -> ShaderRef {
        assets::FERROFLUID_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        assets::FERROFLUID_SHADER.into()
    }

    // The shadow pass raises the same spikes, so they cast their shadows.
    fn prepass_vertex_shader() -> ShaderRef {
        assets::FERROFLUID_SHADER.into()
    }

    // No depth prepass: nothing in the game reads one from the liquid.
    fn enable_prepass() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use bevy::mesh::VertexAttributeValues;

    use super::super::SPHERE_DIAMETER;

    use super::*;

    fn positions(mesh: &Mesh) -> Vec<Vec3> {
        let Some(VertexAttributeValues::Float32x3(points)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("the pool has positions");
        };
        points.iter().map(|&p| Vec3::from(p)).collect()
    }

    #[test]
    fn the_pool_mesh_is_a_disc_dense_enough_for_its_spikes() {
        for (level, longest_mean) in [
            (TierLevel::Modest, 0.024),
            (TierLevel::Strong, 0.012),
            (TierLevel::Exceptional, 0.006),
        ] {
            let mesh = Ferrofluid::pool_mesh(level, |_| -0.1);
            let points = positions(&mesh);
            assert!(
                points
                    .iter()
                    .all(|p| p.xz().length() <= SPHERE_POOL_RADIUS + 1e-4)
            );
            let Some(Indices::U32(indices)) = mesh.indices() else {
                panic!("the pool is indexed");
            };
            let edges: Vec<f32> = indices
                .chunks(3)
                .flat_map(|t| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])])
                .map(|(a, b)| points[a as usize].xz().distance(points[b as usize].xz()))
                .collect();
            let mean = edges.iter().sum::<f32>() / edges.len() as f32;
            assert!(mean <= longest_mean, "{level:?}: edges {mean} m on average");
        }
    }

    #[test]
    fn the_pools_rim_bends_down_to_the_floor() {
        let points = positions(&Ferrofluid::pool_mesh(TierLevel::Strong, |_| -0.1));
        let middle = points
            .iter()
            .min_by(|a, b| a.xz().length().total_cmp(&b.xz().length()))
            .expect("points");
        assert_eq!(middle.y, 0.0, "the liquid lies level in the middle");
        for rim in points
            .iter()
            .filter(|p| p.xz().length() >= SPHERE_POOL_RADIUS - 1e-3)
        {
            assert!(
                (rim.y + 0.1).abs() < 1e-3,
                "the rim at {rim} meets the floor"
            );
        }
    }

    #[test]
    fn the_spikes_take_the_spheres_field_and_turn_with_it() {
        let hover = Hover::at(2.0);
        let fluid = Ferrofluid::under(hover);
        assert_eq!((fluid.field.x, fluid.field.y), (hover.field, hover.spin));
        assert_eq!(fluid.field.w, SPIKE_SPACING);
    }

    fn bob_ends() -> (Hover, Hover) {
        let bob: Vec<Hover> = (0..700).map(|i| Hover::at(i as f32 * 0.01)).collect();
        let lowest = *bob
            .iter()
            .min_by(|a, b| a.height.total_cmp(&b.height))
            .expect("bob");
        let highest = *bob
            .iter()
            .max_by(|a, b| a.height.total_cmp(&b.height))
            .expect("bob");
        (lowest, highest)
    }

    #[test]
    fn the_central_spikes_stand_1_4_m_under_the_lowest_sphere_and_1_m_under_the_highest() {
        let (lowest, highest) = bob_ends();
        let tallest = |hover: Hover| Ferrofluid::under(hover).field.z;
        assert!((tallest(lowest) - 1.4).abs() < 0.01, "{}", tallest(lowest));
        assert!(
            (tallest(highest) - 1.0).abs() < 0.01,
            "{}",
            tallest(highest)
        );
        let clearance = lowest.height - SPHERE_DIAMETER / 2.0 - tallest(lowest);
        assert!(
            clearance > 0.19,
            "the tallest spike comes within {clearance} m of the Sphere"
        );
    }

    #[test]
    fn the_small_spikes_creep_inward_ring_after_ring() {
        let start = Ferrofluid::under(Hover::at(10.0)).rings;
        let later = Ferrofluid::under(Hover::at(10.5)).rings;
        for ring in 0..3 {
            assert!(
                (0.0..1.0).contains(&start[ring]),
                "progress {}",
                start[ring]
            );
            let moved = (later[ring] - start[ring]).rem_euclid(1.0);
            assert!(
                (moved - 0.5 / RING_SECONDS).abs() < 1e-4,
                "ring {ring} moved {moved}"
            );
        }
        let gaps = [start[1] - start[0], start[2] - start[1]].map(|gap| gap.rem_euclid(1.0));
        assert!(
            gaps.iter().all(|gap| (gap - 1.0 / 3.0).abs() < 1e-4),
            "staggered {gaps:?}"
        );
        let flow = Ferrofluid::under(Hover::at(0.0)).flow;
        assert!(
            flow.x > flow.y,
            "from the rim at {} m in to {} m",
            flow.x,
            flow.y
        );
        assert_eq!((flow.z, flow.w), (0.2, 0.4), "20 to 40 cm tall");
    }

    #[test]
    fn the_small_spikes_pulse_on_a_clock_that_wraps_without_a_jump() {
        let pulse = |seconds: f32| Ferrofluid::under(Hover::at(seconds)).rings.w;
        assert!((pulse(10.0) - 10.0).abs() < 1e-4);
        assert!((pulse(PULSE_WRAP + 1.0) - 1.0).abs() < 1e-3, "wrapped");
    }
}
