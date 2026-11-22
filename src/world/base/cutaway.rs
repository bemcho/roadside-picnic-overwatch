//! Roof cutaway: the hangar's roof and upper walls disappear while the camera
//! looks inside, so the tactical view can follow the stalker indoors. Only the
//! main pass cuts; shadows still use the whole shell, keeping the hall dim.
//!
//! The same shader repaints the shell as an industrial hall: steel floor
//! plates with yellow walkway lines, the parked car's bay, a trench grate and
//! a hazard band at the door, and painted-steel walls with a hazard kick band.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

use crate::assets;
use crate::player::PlayerCamera;
use crate::player::vehicle::parked_vehicle;
use crate::world::ViewFocus;

use super::Hall;

pub(in crate::world) type CutawayMaterial = ExtendedMaterial<StandardMaterial, Cutaway>;

/// Nothing is cut while the view is outside.
const UNCUT: f32 = 1.0e6;
/// Wall height left standing while cut away, above the hall floor.
const CUT_HEIGHT: f32 = 6.5;
/// Half the hall's footprint in meters, plus room to start cutting at the door.
const INTERIOR_HALF_EXTENTS: Vec2 = Vec2::new(19.0, 21.0);

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub(in crate::world) struct Cutaway {
    /// x: world height above which fragments are discarded.
    #[uniform(100)]
    pub(super) height: Vec4,
    /// xyz: the middle of the hall's floor in the world, where the floor paint
    /// is laid out from (the hall stands unrotated).
    #[uniform(101)]
    pub(super) floor: Vec4,
    #[texture(102)]
    #[sampler(103)]
    pub(super) plate_albedo: Handle<Image>,
    #[texture(104)]
    #[sampler(105)]
    pub(super) plate_normal: Handle<Image>,
    /// AO, roughness and metallic.
    #[texture(106)]
    #[sampler(107)]
    pub(super) plate_surface: Handle<Image>,
    /// The parked car's painted bay: x, z and yaw from the floor's middle.
    #[uniform(108)]
    pub(super) bay: Vec4,
}

impl Cutaway {
    /// The uncut shell of a hall whose floor's middle lies at `floor`.
    pub(super) fn new(server: &AssetServer, floor: Vec3) -> Self {
        let parked = parked_vehicle(Vec3::ZERO);
        let (yaw, _, _) = parked.rotation.to_euler(EulerRot::YXZ);
        Self {
            floor: floor.extend(0.0),
            bay: Vec4::new(parked.translation.x, parked.translation.z, yaw, 0.0),
            plate_albedo: assets::load_repeating(server, assets::HANGAR_FLOOR_ALBEDO, true),
            plate_normal: assets::load_repeating(server, assets::HANGAR_FLOOR_NORMAL, false),
            plate_surface: assets::load_repeating(server, assets::HANGAR_FLOOR_SURFACE, false),
            ..default()
        }
    }
}

impl Default for Cutaway {
    fn default() -> Self {
        Self {
            height: Vec4::new(UNCUT, 0.0, 0.0, 0.0),
            floor: Vec4::ZERO,
            plate_albedo: Handle::default(),
            plate_normal: Handle::default(),
            plate_surface: Handle::default(),
            bay: Vec4::ZERO,
        }
    }
}

impl MaterialExtension for Cutaway {
    fn fragment_shader() -> ShaderRef {
        assets::CUTAWAY_SHADER.into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        assets::CUTAWAY_SHADER.into()
    }
}

/// Lamps and cords hanging from the roof; they go with it.
#[derive(Component)]
pub(in crate::world) struct RoofFixture;

/// The material whose height is driven by the view.
#[derive(Component)]
pub(in crate::world) struct CutawayShell(pub Handle<CutawayMaterial>);

/// Past this camera distance from its focus the hall shows whole again, roof
/// and walls, as a landmark in the wider view.
const CUT_ZOOM: f32 = 120.0;

/// Cuts the shell down to its lower walls while the view focus is inside or
/// the camera looks down from above it, unless the camera is zoomed out.
pub(in crate::world) fn update_cutaway(
    focus: Query<&GlobalTransform, With<ViewFocus>>,
    cameras: Query<&GlobalTransform, With<PlayerCamera>>,
    halls: Query<(&GlobalTransform, &CutawayShell), With<Hall>>,
    mut materials: ResMut<Assets<CutawayMaterial>>,
    mut fixtures: Query<&mut Visibility, With<RoofFixture>>,
) {
    let Ok(focus) = focus.single() else {
        return;
    };
    let eye = cameras
        .single()
        .map_or(focus.translation(), GlobalTransform::translation);
    for (hall, shell) in &halls {
        let to_local = hall.affine().inverse();
        let over = |at: Vec3| {
            to_local
                .transform_point3(at)
                .xz()
                .abs()
                .cmple(INTERIOR_HALF_EXTENTS)
                .all()
        };
        let close = eye.distance(focus.translation()) < CUT_ZOOM;
        let inside = close && (over(focus.translation()) || over(eye));
        let shown = if inside {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        for mut fixture in &mut fixtures {
            fixture.set_if_neq(shown);
        }
        let height = if inside {
            hall.translation().y + CUT_HEIGHT
        } else {
            UNCUT
        };
        if materials
            .get(&shell.0)
            .is_some_and(|material| material.extension.height.x != height)
            && let Some(mut material) = materials.get_mut(&shell.0)
        {
            material.extension.height.x = height;
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin;

    use super::*;

    #[test]
    fn the_roof_is_cut_only_while_the_view_is_inside_the_hall() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<CutawayMaterial>()
            .add_systems(Update, update_cutaway);
        let shell = app
            .world_mut()
            .resource_mut::<Assets<CutawayMaterial>>()
            .add(CutawayMaterial::default());
        app.world_mut().spawn((
            Hall,
            GlobalTransform::from_xyz(40.0, 12.0, -128.0),
            CutawayShell(shell.clone()),
        ));
        let focus = app
            .world_mut()
            .spawn((ViewFocus, GlobalTransform::from_xyz(42.0, 13.0, -120.0)))
            .id();
        let lamp = app
            .world_mut()
            .spawn((RoofFixture, Visibility::Inherited))
            .id();
        let lamp_visibility = |app: &App| *app.world().get::<Visibility>(lamp).expect("lamp");
        let height = |app: &App| {
            app.world()
                .resource::<Assets<CutawayMaterial>>()
                .get(&shell)
                .expect("shell material")
                .extension
                .height
                .x
        };
        app.update();
        assert_eq!(height(&app), 12.0 + CUT_HEIGHT);
        assert_eq!(lamp_visibility(&app), Visibility::Hidden);

        *app.world_mut()
            .get_mut::<GlobalTransform>(focus)
            .expect("focus") = GlobalTransform::from_xyz(40.0, 13.0, -60.0);
        app.update();
        assert_eq!(height(&app), UNCUT);
        assert_eq!(lamp_visibility(&app), Visibility::Inherited);
    }
}
