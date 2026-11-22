//! Asset path constants.
//!
//! All asset handles are loaded through `AssetServer` using these paths.
//! Never scatter inline string literals for asset paths across systems.
//!
//! The `#[expect(dead_code)]` markers are intentional: these constants are
//! the project's forward-looking contract for asset locations. When a module
//! starts using one, the compiler will demand the attribute be removed.

use bevy::gltf::{GltfLoaderSettings, GltfMaterial};
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::shader::Shader;

pub const SPLASH_HANGAR: &str = "textures/splash/hangar.png";
pub const STALKER_MODEL: &str = "models/stalker/uhd/stalker_rigged.glb";
pub const TERRAIN_SHADER: &str = "shaders/terrain.wesl";
pub const WATER_SHADER: &str = "shaders/water.wesl";
pub const CUTAWAY_SHADER: &str = "shaders/cutaway.wesl";
pub const GROUND_FOG_SHADER: &str = "shaders/ground_fog.wesl";
pub const PLANT_WIND_SHADER: &str = "shaders/plant_wind.wesl";
pub const TERRAIN_GRASS_ALBEDO: &str = "textures/terrain/grass_albedo.png";
pub const TERRAIN_GRASS_NORMAL: &str = "textures/terrain/grass_normal.png";
pub const TERRAIN_GRASS_SURFACE: &str = "textures/terrain/grass_surface.png";
pub const TERRAIN_EARTH_ALBEDO: &str = "textures/terrain/earth_albedo.png";
pub const TERRAIN_EARTH_NORMAL: &str = "textures/terrain/earth_normal.png";
pub const TERRAIN_EARTH_SURFACE: &str = "textures/terrain/earth_surface.png";
pub const VEGETATION_DEAD_FOREST: &str = "models/vegetation/medium/dead_forest.glb";
pub const VEGETATION_OVERGROWTH: &str = "models/vegetation/medium/overgrowth.glb";
pub const VEGETATION_MUD_BOG: &str = "models/vegetation/medium/mud_bog.glb";
pub const VEGETATION_SLIME_DITCH: &str = "models/vegetation/medium/slime_ditch.glb";
pub const TREE_SMALL: &str = "models/trees/medium/tree_small.glb";
pub const TREE_BIG: &str = "models/trees/medium/tree_big.glb";
pub const TREE_VERY_TALL: &str = "models/trees/medium/tree_veryhigh.glb";
pub const TREE_BIRCH: &str = "models/trees/medium/tree_birch.glb";
pub const TREE_DEAD: &str = "models/trees/medium/tree_dead.glb";
pub const FROSTWOLF: &str = "models/animals/medium/frostwolf.glb";
pub const HEXADEER: &str = "models/animals/medium/hexadeer.glb";
pub const SKYGLIDER: &str = "models/animals/medium/skyglider.glb";
pub const VENOM_CHAMELEON: &str = "models/animals/medium/venomchameleon.glb";
pub const HAWK: &str = "models/birds/medium/hawk.glb";
pub const HERON: &str = "models/birds/medium/heron.glb";
pub const OWL: &str = "models/birds/medium/owl.glb";
pub const RAVEN: &str = "models/birds/medium/raven.glb";
pub const SPARROW: &str = "models/birds/medium/sparrow.glb";
pub const BOVINE: &str = "models/mutants/medium/bovine.glb";
pub const CHIMERA: &str = "models/mutants/medium/chimera.glb";
pub const FLESHDEER: &str = "models/mutants/medium/fleshdeer.glb";
pub const MUTT: &str = "models/mutants/medium/mutt.glb";
pub const BASE_HANGAR: &str = "models/hangar/medium/hangar.glb";
pub const BASE_VEHICLE: &str = "models/vehicles/medium/armored_vehicle.glb";
pub const VEHICLE_CRAWLER: &str = "models/vehicles/medium/vehicle-crawler.glb";
pub const VEHICLE_QUARRY: &str = "models/vehicles/medium/vehicle-quarry.glb";
pub const VEHICLE_SEDAN: &str = "models/vehicles/medium/vehicle-sedan.glb";
pub const VEHICLE_TRAIN: &str = "models/vehicles/medium/vehicle-train.glb";
/// The long walls repeat one panel thousands of times, so they keep the medium
/// model; every standalone site uses its UHD scan.
pub const SITE_CORDON_WALL: &str = "models/sites/medium/site-cordon-wall.glb";
pub const SITE_CHECKPOINT: &str = "models/sites/uhd/site-checkpoint.glb";
pub const HANGAR_LAB: &str = "models/props/medium/lab.glb";
pub const HANGAR_MAP_BOARD: &str = "models/props/medium/mapboard.glb";
pub const HANGAR_REPAIR_STAND: &str = "models/props/medium/repairstand.glb";
pub const HANGAR_TABLE: &str = "models/props/medium/table.glb";
pub const HANGAR_TOILET: &str = "models/props/medium/toilet.glb";
pub const HANGAR_TOOLS: &str = "models/props/medium/tools.glb";
pub const HANGAR_WORKSHOP: &str = "models/props/medium/workshop.glb";
pub const BUSH_UHD_SHRUB: &str = "models/plants/uhd/shrub.glb";
pub const BUSH_UHD_THORNBUSH: &str = "models/plants/uhd/thornbush.glb";
pub const BUSH_SHRUB_VARIANTS: [&str; 6] = [
    "models/plants/medium/bramble.glb",
    "models/plants/medium/dogrose.glb",
    "models/plants/medium/elder.glb",
    "models/plants/medium/juniper.glb",
    "models/plants/medium/wormwood.glb",
    "models/plants/medium/shrub.glb",
];
pub const BUSH_MEDIUM_THORNBUSH: &str = "models/plants/medium/thornbush.glb";
pub const ARTIFACTS: [&str; 14] = [
    "models/artifacts/medium/artifact-death-lamp.glb",
    "models/artifacts/medium/artifact-golden-sphere.glb",
    "models/artifacts/medium/artifact-itcher.glb",
    "models/artifacts/medium/artifact-lobster-eyes.glb",
    "models/artifacts/medium/artifact-spinning-ring.glb",
    "models/artifacts/medium/artifact-sponge.glb",
    "models/artifacts/medium/artifact-white-ring.glb",
    "models/artifacts/medium/artifact-wriggling-magnet.glb",
    "models/artifacts/medium/blackspark.glb",
    "models/artifacts/medium/bracelet.glb",
    "models/artifacts/medium/empty.glb",
    "models/artifacts/medium/fullempty.glb",
    "models/artifacts/medium/pinsneedles.glb",
    "models/artifacts/medium/soso.glb",
];
pub const ARTIFACT_PREVIEWS: [&str; 14] = [
    "models/artifacts/medium/artifact-death-lamp.glb",
    "models/artifacts/medium/artifact-golden-sphere.glb",
    "models/artifacts/medium/artifact-itcher.glb",
    "models/artifacts/medium/artifact-lobster-eyes.glb",
    "models/artifacts/medium/artifact-spinning-ring.glb",
    "models/artifacts/medium/artifact-sponge.glb",
    "models/artifacts/medium/artifact-white-ring.glb",
    "models/artifacts/medium/artifact-wriggling-magnet.glb",
    "models/artifacts/medium/blackspark.glb",
    "models/artifacts/medium/bracelet.glb",
    "models/artifacts/medium/empty.glb",
    "models/artifacts/medium/fullempty.glb",
    "models/artifacts/medium/pinsneedles.glb",
    "models/artifacts/medium/soso.glb",
];

/// Root folder for textures and materials.
#[expect(dead_code)]
pub const TEXTURES: &str = "textures";

/// Root folder for audio files.
#[expect(dead_code)]
pub const AUDIO: &str = "audio";

/// Root folder for fonts.
#[expect(dead_code)]
pub const FONTS: &str = "fonts";

#[derive(Resource, Default)]
pub(crate) struct PreloadedAssets(pub Vec<UntypedHandle>);

pub(crate) fn preload_game_assets(server: Res<AssetServer>, mut assets: ResMut<PreloadedAssets>) {
    let scenes = [
        STALKER_MODEL,
        VEGETATION_DEAD_FOREST,
        VEGETATION_OVERGROWTH,
        VEGETATION_MUD_BOG,
        VEGETATION_SLIME_DITCH,
        HANGAR_LAB,
        HANGAR_MAP_BOARD,
        HANGAR_REPAIR_STAND,
        HANGAR_TABLE,
        HANGAR_TOILET,
        HANGAR_TOOLS,
        HANGAR_WORKSHOP,
        VEHICLE_CRAWLER,
        VEHICLE_QUARRY,
        VEHICLE_SEDAN,
        VEHICLE_TRAIN,
        BUSH_MEDIUM_THORNBUSH,
    ];
    let albedo = [TERRAIN_GRASS_ALBEDO, TERRAIN_EARTH_ALBEDO];
    let data_maps = [
        TERRAIN_GRASS_NORMAL,
        TERRAIN_GRASS_SURFACE,
        TERRAIN_EARTH_NORMAL,
        TERRAIN_EARTH_SURFACE,
    ];
    assets.0.extend(scenes.map(|path| {
        server
            .load::<WorldAsset>(GltfAssetLabel::Scene(0).from_asset(path))
            .untyped()
    }));
    assets.0.extend(BUSH_SHRUB_VARIANTS.map(|path| {
        server
            .load::<WorldAsset>(GltfAssetLabel::Scene(0).from_asset(path))
            .untyped()
    }));
    assets.0.extend(ARTIFACT_PREVIEWS.map(|path| {
        server
            .load::<WorldAsset>(GltfAssetLabel::Scene(0).from_asset(path))
            .untyped()
    }));
    let (golden_sphere, gold) = load_single_primitive(&server, ARTIFACT_PREVIEWS[1]);
    assets.0.extend([golden_sphere.untyped(), gold.untyped()]);
    let (hangar, hangar_paint) = load_single_primitive(&server, BASE_HANGAR);
    assets.0.extend([hangar.untyped(), hangar_paint.untyped()]);
    assets.0.push(load_vehicle_scene(&server).untyped());
    assets
        .0
        .extend(albedo.map(|path| load_repeating(&server, path, true).untyped()));
    assets
        .0
        .extend(data_maps.map(|path| load_repeating(&server, path, false).untyped()));
    assets.0.extend([
        server.load::<Image>(SPLASH_HANGAR).untyped(),
        server.load::<Shader>(TERRAIN_SHADER).untyped(),
        server.load::<Shader>(WATER_SHADER).untyped(),
        server.load::<Shader>(CUTAWAY_SHADER).untyped(),
        server.load::<Shader>(GROUND_FOG_SHADER).untyped(),
        server.load::<Shader>(PLANT_WIND_SHADER).untyped(),
    ]);
}

pub(crate) fn load_vehicle_scene(server: &AssetServer) -> Handle<WorldAsset> {
    server
        .load_builder()
        .with_settings(|settings: &mut GltfLoaderSettings| {
            settings.default_sampler = Some(ImageSamplerDescriptor {
                anisotropy_clamp: 8,
                ..ImageSamplerDescriptor::linear()
            });
        })
        .load(GltfAssetLabel::Scene(0).from_asset(BASE_VEHICLE))
}

/// Loads the shared mesh and material of a single-primitive GLB.
pub(crate) fn load_single_primitive(
    server: &AssetServer,
    path: &'static str,
) -> (Handle<Mesh>, Handle<GltfMaterial>) {
    (
        server.load(
            GltfAssetLabel::Primitive {
                mesh: 0,
                primitive: 0,
            }
            .from_asset(path),
        ),
        server.load(
            GltfAssetLabel::Material {
                index: 0,
                is_scale_inverted: false,
            }
            .from_asset(path),
        ),
    )
}

/// Loads a tiling texture: repeating, anisotropically filtered.
pub(crate) fn load_repeating(
    server: &AssetServer,
    path: &'static str,
    is_srgb: bool,
) -> Handle<Image> {
    server
        .load_builder()
        .with_settings(move |settings: &mut ImageLoaderSettings| {
            settings.is_srgb = is_srgb;
            settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                address_mode_u: ImageAddressMode::Repeat,
                address_mode_v: ImageAddressMode::Repeat,
                anisotropy_clamp: 8,
                ..ImageSamplerDescriptor::linear()
            });
        })
        .load(path)
}
