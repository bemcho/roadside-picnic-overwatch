//! Render-world pass for the ground fog and the cloud layers: a fullscreen
//! raymarch over the fog field and through the clouds aloft, drawn over the
//! opaque scene and the sky before post-processing. It uses the
//! view's own bindings, so the fog is lit by the sun and moon through the
//! atmosphere and shadowed by their cascades. Last, it covers unsurveyed ground
//! with the shroud's black smoke (`shroud.rs`).

use bevy::camera::{MainPassResolutionOverride, Viewport};
use bevy::core_pipeline::{Core3dSystems, FullscreenShader, schedule::Core3d};
use bevy::pbr::{
    MeshPipelineSystems, MeshPipelineViewLayoutKey, MeshPipelineViewLayouts, MeshViewBindGroup,
    ViewKeyCache,
};
use bevy::prelude::*;
use bevy::render::{
    Render, RenderApp, RenderStartup, RenderSystems,
    camera::ExtractedCamera,
    diagnostic::RecordDiagnostics,
    extract_component::{
        ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
        UniformComponentPlugin,
    },
    extract_resource::{ExtractResource, ExtractResourcePlugin},
    render_asset::RenderAssets,
    render_resource::{
        BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendComponent,
        BlendFactor, BlendOperation, BlendState, CachedRenderPipelineId, ColorTargetState,
        ColorWrites, FilterMode, FragmentState, LoadOp, Operations, PipelineCache,
        RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
        SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, SpecializedRenderPipeline,
        SpecializedRenderPipelines, StoreOp, TextureFormat, TextureSampleType, VertexState,
        binding_types::{
            sampler, texture_2d, texture_depth_2d, texture_depth_2d_multisampled, uniform_buffer,
        },
    },
    renderer::{RenderContext, RenderDevice, ViewQuery},
    texture::GpuImage,
    view::{ExtractedView, Msaa, ViewDepthStencilTexture, ViewTarget},
};

use crate::assets;

use super::super::shroud::ShroudMask;

/// Per-camera fog parameters, uploaded as the shader's `GroundFog` uniform.
#[derive(Component, Clone, Copy, Default, ExtractComponent, ShaderType)]
#[extract_app(RenderApp)]
pub(crate) struct GroundFog {
    /// Field origin (x, z) and inverse extent.
    pub(super) field: Vec4,
    /// Fog tendency and overview fade.
    pub(super) weather: Vec4,
    /// Layer top range (m) and extinction range (1/m).
    pub(super) shape: Vec4,
    /// Wind drift (x, z) and slow churn phase.
    pub(super) wind: Vec4,
    /// Exposed sky light.
    pub(super) ambient: Vec4,
    /// Fog-free box (center x, z, half extents).
    pub(super) clear_box: Vec4,
    /// Cloud cover of the cloud layers (0-1) and how dark rain clouds grow.
    pub(super) clouds: Vec4,
    /// Shroud mask corner (x, z) and one over its extent.
    pub(super) shroud: Vec4,
    /// Harmont's fog: top range (m) and extinction range (1/m), thin to dense.
    pub(super) harmont: Vec4,
}

/// The fog field texture, shared with the render world.
#[derive(Resource, Clone, ExtractResource)]
#[extract_app(RenderApp)]
pub(in crate::world) struct GroundFogField(pub Handle<Image>);

pub(in crate::world) struct GroundFogRenderPlugin;

impl Plugin for GroundFogRenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ExtractComponentPlugin::<GroundFog>::default(),
            UniformComponentPlugin::<GroundFog>::default(),
            ExtractResourcePlugin::<GroundFogField>::default(),
            ExtractResourcePlugin::<ShroudMask>::default(),
        ));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<SpecializedRenderPipelines<GroundFogPipeline>>()
            .add_systems(RenderStartup, init_pipeline.after(MeshPipelineSystems))
            .add_systems(Render, specialize_pipelines.in_set(RenderSystems::Prepare))
            .add_systems(
                Core3d,
                draw_ground_fog
                    .after(Core3dSystems::MainPass)
                    .before(Core3dSystems::EarlyPostProcess),
            );
    }
}

#[derive(Resource)]
struct GroundFogPipeline {
    view_layouts: MeshPipelineViewLayouts,
    /// Without and with a multisampled depth buffer.
    layouts: [BindGroupLayoutDescriptor; 2],
    vertex: VertexState,
    shader: Handle<Shader>,
    sampler: Sampler,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GroundFogKey {
    view: MeshPipelineViewLayoutKey,
    target: TextureFormat,
}

#[derive(Component)]
struct GroundFogPipelineId(CachedRenderPipelineId);

fn init_pipeline(
    mut commands: Commands,
    view_layouts: Res<MeshPipelineViewLayouts>,
    server: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
    device: Res<RenderDevice>,
) {
    let layout = |multisampled: bool| {
        let depth = if multisampled {
            texture_depth_2d_multisampled()
        } else {
            texture_depth_2d()
        };
        BindGroupLayoutDescriptor::new(
            "ground_fog_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<GroundFog>(true),
                    depth,
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                ),
            ),
        )
    };
    commands.insert_resource(GroundFogPipeline {
        view_layouts: view_layouts.clone(),
        layouts: [layout(false), layout(true)],
        vertex: fullscreen.to_vertex_state(),
        shader: server.load(assets::GROUND_FOG_SHADER),
        sampler: device.create_sampler(&SamplerDescriptor {
            label: Some("ground_fog_field_sampler"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
    });
}

impl SpecializedRenderPipeline for GroundFogPipeline {
    type Key = GroundFogKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        let multisampled = key.view.contains(MeshPipelineViewLayoutKey::MULTISAMPLED);
        let mut shader_defs = vec!["SHADOW_FILTER_METHOD_HARDWARE_2X2".into()];
        if multisampled {
            shader_defs.push("MULTISAMPLED".into());
        }
        if key.view.contains(MeshPipelineViewLayoutKey::ATMOSPHERE) {
            shader_defs.push("ATMOSPHERE".into());
        }
        RenderPipelineDescriptor {
            label: Some("ground_fog_pipeline".into()),
            layout: vec![
                self.view_layouts.get_view_layout(key.view).main_layout,
                self.layouts[usize::from(multisampled)].clone(),
            ],
            vertex: self.vertex.clone(),
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                shader_defs,
                // Premultiplied: fog light plus the scene times transmittance.
                targets: vec![Some(ColorTargetState {
                    format: key.target,
                    blend: Some(BlendState {
                        color: BlendComponent {
                            src_factor: BlendFactor::One,
                            dst_factor: BlendFactor::OneMinusSrcAlpha,
                            operation: BlendOperation::Add,
                        },
                        alpha: BlendComponent {
                            src_factor: BlendFactor::Zero,
                            dst_factor: BlendFactor::One,
                            operation: BlendOperation::Add,
                        },
                    }),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        }
    }
}

fn specialize_pipelines(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<GroundFogPipeline>>,
    pipeline: Res<GroundFogPipeline>,
    view_keys: Res<ViewKeyCache>,
    views: Query<(Entity, &ExtractedView), With<GroundFog>>,
) {
    for (entity, view) in &views {
        let Some(key) = view_keys.get(&view.retained_view_entity) else {
            continue;
        };
        let key = GroundFogKey {
            view: (*key).into(),
            target: view.target_format,
        };
        let id = pipelines.specialize(&cache, &pipeline, key);
        commands.entity(entity).insert(GroundFogPipelineId(id));
    }
}

/// What the fog pass needs from each view. An upscaler (DLSS) renders the
/// main pass, and so the fog, into the top-left `MainPassResolutionOverride`
/// of the view's textures.
type FogView = (
    &'static ExtractedCamera,
    Option<&'static MainPassResolutionOverride>,
    &'static ViewTarget,
    &'static ViewDepthStencilTexture,
    &'static MeshViewBindGroup,
    &'static GroundFogPipelineId,
    &'static DynamicUniformIndex<GroundFog>,
    &'static Msaa,
);

fn draw_ground_fog(
    view: ViewQuery<FogView>,
    cache: Res<PipelineCache>,
    pipeline: Res<GroundFogPipeline>,
    uniforms: Res<ComponentUniforms<GroundFog>>,
    (field, shroud): (Option<Res<GroundFogField>>, Option<Res<ShroudMask>>),
    images: Res<RenderAssets<GpuImage>>,
    mut ctx: RenderContext,
) {
    let (camera, render_size, target, depth, view_bind_group, pipeline_id, uniform_index, msaa) =
        view.into_inner();
    let (Some(render_pipeline), Some(uniform), Some(field), Some(shroud), Some(depth)) = (
        cache.get_render_pipeline(pipeline_id.0),
        uniforms.uniforms().binding(),
        field.and_then(|field| images.get(&field.0)),
        shroud.and_then(|shroud| images.get(&shroud.0)),
        depth.attachment.depth_stencil_views().depth_only_view(),
    ) else {
        return;
    };
    let multisampled = *msaa != Msaa::Off;
    let bind_group = ctx.render_device().create_bind_group(
        "ground_fog_bind_group",
        &cache.get_bind_group_layout(&pipeline.layouts[usize::from(multisampled)]),
        &BindGroupEntries::sequential((
            uniform,
            depth,
            &field.texture_view,
            &pipeline.sampler,
            &shroud.texture_view,
            &shroud.sampler,
        )),
    );
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(ctx.command_encoder(), "ground_fog");
    {
        let mut pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("ground_fog_pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: target.main_texture_view(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        if let Some(viewport) =
            Viewport::from_viewport_and_override(camera.viewport.as_ref(), render_size)
        {
            pass.set_viewport(
                viewport.physical_position.x as f32,
                viewport.physical_position.y as f32,
                viewport.physical_size.x as f32,
                viewport.physical_size.y as f32,
                viewport.depth.start,
                viewport.depth.end,
            );
        }
        pass.set_pipeline(render_pipeline);
        pass.set_bind_group(0, &view_bind_group.main, &view_bind_group.main_offsets);
        pass.set_bind_group(1, &bind_group, &[uniform_index.index()]);
        pass.draw(0..3, 0..1);
    }
    span.end(ctx.command_encoder());
}
