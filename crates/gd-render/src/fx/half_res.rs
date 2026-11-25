//! Half-resolution particles. PORT: the engine draws every particle at full size; the port draws them at half
//! size, its own fill-rate saving for effect-heavy fights, where the particles' overdraw dominates the frame.
//!
//! A main view's ([`GammaOut`]) [`super::render::ParticleMaterial`] draws (additive and
//! alpha-blended; the distortion emitters are in Bevy's transmissive phase and stay with `render/distort.rs`) are
//! taken out of Bevy's transparent phase once it is sorted and batched ([`hold_particles`]), and [`draw_particles`]
//! draws them right after the transparent pass, before the fog and the glow composite (`render::post::fog_and_glow`),
//! so their place relative to fog, glow and distortion is unchanged:
//! 1. the view depth is reduced to half size (rounded up), each texel the nearest of its 2 × 2 (reverse Z: the
//!    largest), so a particle behind geometry stays hidden (one in front of the background may lose a half-size
//!    texel along a silhouette);
//! 2. the particles are drawn in the phase's order into a cleared half-size target (the frame's format) with that
//!    depth, by variants of their own pipelines ([`HalfResPipelines::variant`]): the colour blend is the material's
//!    (additive SrcAlpha / One, alpha SrcAlpha / InvSrcAlpha, `fx/render.rs`), the alpha blend tracks coverage
//!    instead: additive Zero / One (the background shows through), alpha One / InvSrcAlpha (A = a + (1 − a)·A). The
//!    target then holds the particles premultiplied by what covers them, and 1 − A is the share of the background
//!    still visible;
//! 3. a bilinear upsample adds it to the frame, rgb + (1 − A)·frame (One / InvSrcAlpha), which reproduces any
//!    sequence of the two blends at full size.
//!
//! [`restore_particles`] puts the items back (with their own pipelines) for the next frame's queue. A view with a
//! custom viewport or a non-HDR target, and a pipeline whose variant is still compiling, draw full size in Bevy's
//! transparent pass.
//!
//! PORT: the held particles draw after every other transparent-pass item (blended meshes, the weather's rain / fog
//! particles at sort 6200, `grimdawn` weather.rs) instead of interleaved with them by the sort, so a particle behind
//! a blended mesh or a weather quad shows over it. Weather particles stay full size.
use super::render::SHADER as PARTICLE_SHADER;
use crate::render::post::GammaOut;
use bevy::asset::embedded_asset;
use bevy::core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d, main_transparent_pass_3d};
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::render_phase::{SortedRenderPhase, ViewSortedRenderPhases};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, texture_depth_2d};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendComponent,
    BlendFactor, BlendOperation, BlendState, CachedPipelineState, CachedRenderPipelineId,
    ColorTargetState, ColorWrites, CompareFunction, DepthStencilState, Extent3d, FilterMode,
    FragmentState, LoadOp, Operations, Pipeline, PipelineCache, RenderPassColorAttachment,
    RenderPassDepthStencilAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
    SamplerBindingType, SamplerDescriptor, ShaderStages, StoreOp, TextureDescriptor,
    TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{ExtractedView, RetainedViewEntity, ViewDepthStencilTexture, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use gd_formats::collections::HashMap;

const SHADER: &str = "embedded://gd_render/fx/half_res.wesl";
/// The main views' HDR frame (`render/post.rs`); the particle pipelines are specialized for it.
const MAIN_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

pub struct HalfResPlugin;

impl Plugin for HalfResPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "half_res.wesl");
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<HeldParticles>()
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(
                Render,
                (
                    hold_particles.in_set(RenderSystems::PrepareBindGroups),
                    restore_particles.in_set(RenderSystems::Cleanup),
                ),
            )
            .add_systems(
                Core3d,
                draw_particles
                    .after(main_transparent_pass_3d)
                    .before(crate::render::post::fog_and_glow)
                    .in_set(Core3dSystems::MainPass),
            );
    }
}

/// Render world: the depth reduction and composite pipelines, and the particle pipelines' half-size variants.
#[derive(Resource)]
pub(crate) struct HalfResPipelines {
    particle_shader: Handle<Shader>,
    depth_layout: BindGroupLayoutDescriptor,
    composite_layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    depth: CachedRenderPipelineId,
    composite: CachedRenderPipelineId,
    /// Per compiled transparent-phase pipeline: its half-size variant (`None`: not a particle pipeline).
    variants: HashMap<CachedRenderPipelineId, Option<CachedRenderPipelineId>>,
    /// Variant → the pipeline it was made from ([`restore_particles`]).
    originals: HashMap<CachedRenderPipelineId, CachedRenderPipelineId>,
}

impl HalfResPipelines {
    /// The half-size variant of a particle pipeline (module doc, step 2), queued the first time the pipeline is
    /// seen compiled; `None` for other pipelines and until the variant is compiled.
    fn variant(
        &mut self,
        id: CachedRenderPipelineId,
        cache: &PipelineCache,
    ) -> Option<CachedRenderPipelineId> {
        let v = match self.variants.get(&id) {
            Some(v) => *v,
            None => {
                // A queued pipeline's descriptor cannot be read yet.
                if !matches!(
                    cache.get_render_pipeline_state(id),
                    CachedPipelineState::Ok(Pipeline::RenderPipeline(_))
                ) {
                    return None;
                }
                let d = cache.get_render_pipeline_descriptor(id);
                let v = (d.vertex.shader.id() == self.particle_shader.id())
                    .then(|| coverage_variant(d))
                    .flatten()
                    .map(|d| cache.queue_render_pipeline(d));
                self.variants.insert(id, v);
                if let Some(v) = v {
                    self.originals.insert(v, id);
                }
                v
            }
        };
        v.filter(|&v| cache.get_render_pipeline(v).is_some())
    }
}

/// A particle pipeline with the alpha blend tracking coverage (module doc, step 2); `None` for another blend.
fn coverage_variant(d: &RenderPipelineDescriptor) -> Option<RenderPipelineDescriptor> {
    let mut d = d.clone();
    let frag = d.fragment.as_mut()?;
    for t in frag.targets.iter_mut().flatten() {
        let b = t.blend.as_mut()?;
        let (src, dst) = match (b.color.src_factor, b.color.dst_factor) {
            (BlendFactor::SrcAlpha, BlendFactor::One) => (BlendFactor::Zero, BlendFactor::One),
            (BlendFactor::SrcAlpha, BlendFactor::OneMinusSrcAlpha) => {
                (BlendFactor::One, BlendFactor::OneMinusSrcAlpha)
            }
            _ => return None,
        };
        b.alpha = BlendComponent {
            src_factor: src,
            dst_factor: dst,
            operation: BlendOperation::Add,
        };
    }
    d.label = Some("gd_particle_half_res".into());
    Some(d)
}

fn init_pipelines(
    mut commands: Commands,
    device: Res<RenderDevice>,
    assets: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
) {
    let depth_layout = BindGroupLayoutDescriptor::new(
        "gd_particles_half_depth",
        &BindGroupLayoutEntries::single(ShaderStages::FRAGMENT, texture_depth_2d()),
    );
    let composite_layout = BindGroupLayoutDescriptor::new(
        "gd_particles_composite",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::FRAGMENT,
            (
                (1, texture_2d(TextureSampleType::Float { filterable: true })),
                (2, sampler(SamplerBindingType::Filtering)),
            ),
        ),
    );
    let sampler = device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    let shader: Handle<Shader> = assets.load(SHADER);
    let depth = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("gd_particles_half_depth".into()),
        layout: vec![depth_layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: shader.clone(),
            entry_point: Some("downsample_depth".into()),
            targets: vec![],
            ..default()
        }),
        depth_stencil: Some(DepthStencilState {
            format: CORE_3D_DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(CompareFunction::Always),
            stencil: default(),
            bias: default(),
        }),
        ..default()
    });
    // rgb + (1 − A)·frame, the frame's alpha kept.
    let composite = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("gd_particles_composite".into()),
        layout: vec![composite_layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader,
            entry_point: Some("composite".into()),
            targets: vec![Some(ColorTargetState {
                format: MAIN_FORMAT,
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
    });
    commands.insert_resource(HalfResPipelines {
        particle_shader: assets.load(PARTICLE_SHADER),
        depth_layout,
        composite_layout,
        sampler,
        depth,
        composite,
        variants: HashMap::default(),
        originals: HashMap::default(),
    });
}

/// A view's held particles and, while it has some, its half-size colour and depth targets.
#[derive(Default)]
struct Held {
    phase: SortedRenderPhase<Transparent3d>,
    targets: Option<[CachedTexture; 2]>,
}

/// Render world: the particle items held between [`hold_particles`] and [`restore_particles`] (one frame), per
/// view; `spare` is the phase item map swapped in while splitting (kept for its capacity).
#[derive(Resource, Default)]
pub(crate) struct HeldParticles {
    views: HashMap<RetainedViewEntity, Held>,
    spare: SortedRenderPhase<Transparent3d>,
}

#[allow(clippy::too_many_arguments)]
fn hold_particles(
    pipelines: Option<ResMut<HalfResPipelines>>,
    cache: Res<PipelineCache>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut held: ResMut<HeldParticles>,
    mut textures: ResMut<TextureCache>,
    device: Res<RenderDevice>,
    views: Query<(&ExtractedView, &ExtractedCamera, &ViewTarget), With<GammaOut>>,
) {
    let Some(mut p) = pipelines else {
        return;
    };
    if cache.get_render_pipeline(p.depth).is_none()
        || cache.get_render_pipeline(p.composite).is_none()
    {
        return;
    }
    let HeldParticles { views: held, spare } = &mut *held;
    for (view, camera, target) in &views {
        let (None, Some(size)) = (&camera.viewport, camera.physical_target_size) else {
            continue;
        };
        if target.main_texture_format() != MAIN_FORMAT {
            continue;
        }
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let out = held.entry(view.retained_view_entity).or_default();
        // Split in order: a batch shares one pipeline, so it moves whole.
        std::mem::swap(&mut phase.items, &mut spare.items);
        for (k, mut item) in spare.items.drain(..) {
            if let Some(v) = p.variant(item.pipeline, &cache) {
                item.pipeline = v;
                out.phase.items.insert(k, item);
            } else {
                phase.items.insert(k, item);
            }
        }
        if out.phase.items.is_empty() {
            continue;
        }
        let half = UVec2::new(size.x.div_ceil(2), size.y.div_ceil(2));
        let tex = |textures: &mut TextureCache, label, format, usage| {
            textures.get(
                &device,
                TextureDescriptor {
                    label: Some(label),
                    size: Extent3d {
                        width: half.x.max(1),
                        height: half.y.max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                },
            )
        };
        out.targets = Some([
            tex(
                &mut textures,
                "gd_particles_half",
                MAIN_FORMAT,
                TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            ),
            tex(
                &mut textures,
                "gd_particles_half_depth",
                CORE_3D_DEPTH_FORMAT,
                TextureUsages::RENDER_ATTACHMENT,
            ),
        ]);
    }
}

/// Back for the next frame's queue and batching, with their own pipelines; the emptied phases are kept for reuse,
/// views that are gone dropped.
fn restore_particles(
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut held: ResMut<HeldParticles>,
    pipelines: Option<Res<HalfResPipelines>>,
) {
    let Some(p) = pipelines else {
        return;
    };
    held.views.retain(|view, h| {
        h.targets = None;
        let Some(phase) = phases.get_mut(view) else {
            return false;
        };
        for (k, mut item) in h.phase.items.drain(..) {
            if let Some(&o) = p.originals.get(&item.pipeline) {
                item.pipeline = o;
            }
            phase.items.insert(k, item);
        }
        true
    });
}

/// Module doc, steps 1-3.
fn draw_particles(
    world: &World,
    view: ViewQuery<(&ExtractedView, &ViewTarget, &ViewDepthStencilTexture)>,
    held: Res<HeldParticles>,
    pipelines: Option<Res<HalfResPipelines>>,
    cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let view_entity = view.entity();
    let (extracted, target, depth) = view.into_inner();
    let Some(Held {
        phase,
        targets: Some([color, half_depth]),
    }) = held.views.get(&extracted.retained_view_entity)
    else {
        return;
    };
    let Some(p) = pipelines.as_deref() else {
        return;
    };
    let (Some(depth_pipeline), Some(composite_pipeline), Some(depth_view)) = (
        cache.get_render_pipeline(p.depth),
        cache.get_render_pipeline(p.composite),
        depth.attachment.depth_stencil_views().depth_only_view(),
    ) else {
        return;
    };
    let depth_bind = ctx.render_device().create_bind_group(
        "gd_particles_half_depth",
        &cache.get_bind_group_layout(&p.depth_layout),
        &BindGroupEntries::single(depth_view),
    );
    let composite_bind = ctx.render_device().create_bind_group(
        "gd_particles_composite",
        &cache.get_bind_group_layout(&p.composite_layout),
        &BindGroupEntries::with_indices(((1, &color.default_view), (2, &p.sampler))),
    );
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time = diagnostics.time_span(ctx.command_encoder(), "gd_particles_half_res");
    {
        let mut pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("gd_particles_half_depth"),
                color_attachments: &[],
                depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                    view: &half_depth.default_view,
                    depth_ops: Some(Operations {
                        load: LoadOp::Clear(0.0),
                        store: StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        pass.set_pipeline(depth_pipeline);
        pass.set_bind_group(0, &depth_bind, &[]);
        pass.draw(0..3, 0..1);
    }
    {
        let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("gd_particles_half"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &color.default_view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Default::default()),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: &half_depth.default_view,
                depth_ops: Some(Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Err(err) = phase.render(&mut pass, world, view_entity) {
            error!("half-resolution particle pass: {err:?}");
        }
    }
    {
        let mut pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("gd_particles_composite"),
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
        pass.set_pipeline(composite_pipeline);
        pass.set_bind_group(0, &composite_bind, &[]);
        pass.draw(0..3, 0..1);
    }
    time.end(ctx.command_encoder());
}
