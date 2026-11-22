//! Preprocesses the first primitive of a dense GLB into a Bevy meshlet mesh.
//!
//! Only positions, normals, UVs and indices are kept (the format's exact
//! requirement); materials are loaded separately at runtime.
//!
//! cargo run --release --example meshletize --features meshlet_processor -- in.glb out.meshlet_mesh

use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::asset::saver::{AssetSaver, SavedAsset};
use bevy::gltf::gltf;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::experimental::meshlet::{
    MESHLET_DEFAULT_VERTEX_POSITION_QUANTIZATION_FACTOR, MeshletMesh, MeshletMeshSaver,
};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, ComputeTaskPool, IoTaskPool, TaskPool, block_on};

fn main() {
    ComputeTaskPool::get_or_init(TaskPool::new);
    AsyncComputeTaskPool::get_or_init(TaskPool::new);
    IoTaskPool::get_or_init(TaskPool::new);
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("usage: meshletize <in.glb> <out.meshlet_mesh>");
        std::process::exit(2);
    };
    let bytes = std::fs::read(input).expect("readable GLB");
    let document = gltf::Gltf::from_slice(&bytes).expect("valid GLB");
    let blob = document.blob.as_deref().expect("GLB binary chunk");
    let primitive = document
        .meshes()
        .next()
        .and_then(|mesh| mesh.primitives().next())
        .expect("a mesh primitive");
    let reader = primitive.reader(|_| Some(blob));
    let positions: Vec<[f32; 3]> = reader.read_positions().expect("positions").collect();
    let normals: Vec<[f32; 3]> = reader.read_normals().expect("normals").collect();
    let uvs: Vec<[f32; 2]> = reader.read_tex_coords(0).expect("UV0").into_f32().collect();
    let indices: Vec<u32> = reader.read_indices().expect("indices").into_u32().collect();

    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices));
    let started = std::time::Instant::now();
    let meshlets =
        MeshletMesh::from_mesh(&mesh, MESHLET_DEFAULT_VERTEX_POSITION_QUANTIZATION_FACTOR)
            .expect("meshlet conversion");

    let mut file = Vec::new();
    block_on(MeshletMeshSaver.save(
        &mut file,
        SavedAsset::from_asset(&meshlets),
        &(),
        output.to_string_lossy().into_owned().into(),
    ))
    .expect("serialized meshlet mesh");
    std::fs::write(output, &file).expect("writable output");
    println!(
        "{}: converted in {:.1}s, {:.1} MB",
        output.display(),
        started.elapsed().as_secs_f32(),
        file.len() as f32 / 1e6
    );
}
