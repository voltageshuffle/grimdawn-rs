//! Screen distortion: the `distort*.ssh` mesh shaders and `particledistort*`
//! emitters refract a copy of the finished frame.
//!
//! When: `GraphicsDeferredRenderer::RenderFinalScene` (Engine.dll): after
//! every scene pass (sort orders up to 11000: opaque,
//! decals, emissive FX, particles), the fog and the scene glow,
//! `GraphicsMTRenderer::RenderPostObjects` collects the passes of
//! the `postObjectRenderStyle` technique, copies the frame
//! (`UpdateFrameBufferCopy`, the `screenTexture` parameter,
//! `GraphicsMTRenderer.screenTexture`, set
//! by `SetShaderParamsForScene`) and draws them; then the same for
//! `postRenderStyle` (a second copy). The post effects (bloom, enhancement,
//! depth of field) come later. `screenTextureScale` is the constant
//! (1/512, 1/512) (`SetShaderParamsForScene`).
//!
//! Distort mesh shaders (DirectX11 programs):
//! the VS passes uv, uv + fract(`scrollRate`·time), uv + fract(`bumpScrollRate`·time),
//! the clip position and the view vector in the (T, −B, N) frame. The PS:
//!   n = 2·(bump(uv).rgb − ½), k = 1 + `distortion`·bump(uv_b).a·(1 − sat(n·V))
//!   screen uv = (x / (w·k), −y / (w·k))·½ + ½   (a zoom about the screen centre,
//!   strongest where the surface faces away: the bubble's rim)
//!   additive family (`distortadditive*`): rgb = screen·a + base.rgb, a = base.a
//!   tint family (`distort*`, the PS reads `tint`): rgb = screen·sat(base.rgb + 1 − tint)·a
//!   both × `blendedGlobalFadeFactor`; colour from uv and alpha from the scrolled
//!   uv, swapped by the `*colorscroll*` shaders.
//! Pass states (all of them): blend One / InvSrcAlpha, depth test LessEqual,
//! cull back, alpha write off. A mesh without a `bumpTexture` gets the shader's
//! default `/system/textures/flatbmp.tex` (flat normal, alpha 1). The `*wiggle*`
//! programs move the vertices by the foliage wind first (the object's world
//! position as offset, no ripple amplitude). Only `distortadditivestatic`
//! and `distortadditiveunusedcolorstatic` are `postObjectRenderStyle`; every
//! other distort mesh shader and both `particledistort*` are `postRenderStyle`,
//! so they refract the first group. `distortstandard*` has only a forward
//! `dLightDiffuseAmbientStyle`, which the deferred renderer never draws.
//!
//! Here: the materials ([`DistortMaterial`], the particle material with a
//! distort shader) read Bevy's view transmission texture and are queued in
//! its `Transmissive3d` phase, which Bevy draws before the transparent pass.
//! [`hold_distortion`] takes that phase's items away before the frame is
//! drawn, split by style (entities with [`PostObjectStyle`] first),
//! [`draw_distortion`] copies the frame into the transmission texture after
//! the transparent pass, the fog and the glow composite (`post::fog_and_glow`)
//! and draws the first group, then copies again and draws the second (a copy
//! only for a group with items), and
//! [`restore_distortion`] puts the retained items back for the next frame's
//! queue. The glow view never draws them (main layer only).
use super::material::{PassStates, blend_factor};
use bevy::core_pipeline::{Core3d, Core3dSystems, core_3d::main_transparent_pass_3d};
use bevy::ecs::entity::EntityHashSet;
use bevy::image::ToExtents;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    MaterialPipeline, MaterialPipelineKey, ScreenSpaceTransmission, Transmissive3d,
    ViewTransmissionTexture,
};
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::render_phase::{SortedRenderPhase, ViewSortedRenderPhases};
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, Face,
    RenderPassDescriptor, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    StoreOp,
};
use bevy::render::renderer::{RenderContext, ViewQuery};
use bevy::render::view::{ExtractedView, RetainedViewEntity, ViewDepthStencilTexture, ViewTarget};
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};
use bevy::shader::ShaderRef;
use gd_formats::collections::HashMap;

const SHADER: &str = "embedded://gd_render/render/shaders/gd_distort.wesl";

/// `screenTextureScale` (`SetShaderParamsForScene`: 0x3B000000 twice).
pub const SCREEN_TEXTURE_SCALE: f32 = 1.0 / 512.0;

pub struct DistortPlugin;

impl Plugin for DistortPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<DistortMaterial>::default())
            .add_systems(PostUpdate, enable_transmission);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<HeldDistortion>()
            .init_resource::<PostObjectEntities>()
            .add_systems(ExtractSchedule, extract_post_object)
            .add_systems(
                Render,
                (
                    hold_distortion.in_set(RenderSystems::PrepareBindGroups),
                    restore_distortion.in_set(RenderSystems::Cleanup),
                ),
            )
            .add_systems(
                Core3d,
                draw_distortion
                    .after(main_transparent_pass_3d)
                    .after(super::post::fog_and_glow)
                    .in_set(Core3dSystems::MainPass),
            );
    }
}

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct DistortParams {
    /// xy `scrollRate`, zw `bumpScrollRate`.
    pub scroll: Vec4,
    /// x `distortion`, y `tint`, z `blendedGlobalFadeFactor`.
    pub values: Vec4,
    /// x 1 = additive family, y 1 = colour scroll (`*colorscroll*`), z 1 = the
    /// mesh has tangents.
    pub flags: UVec4,
    /// The `*wiggle*` vertex wobble.
    pub wind: Vec4,
    pub ripple_scale: Vec4,
    pub ripple_amplitude: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
#[bind_group_data(DistortKey)]
pub struct DistortMaterial {
    #[uniform(0)]
    pub params: DistortParams,
    #[texture(1)]
    #[sampler(2)]
    pub base: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    pub bump: Handle<Image>,
    pub states: PassStates,
    /// Drawn by `postObjectRenderStyle`.
    pub post_object: bool,
}

/// A distortion part drawn by `postObjectRenderStyle`, before the
/// `postRenderStyle` ones and their frame copy.
#[derive(Component)]
pub struct PostObjectStyle;

#[derive(Resource, Default)]
struct PostObjectEntities(EntityHashSet);

fn extract_post_object(
    mut set: ResMut<PostObjectEntities>,
    parts: Extract<Query<(Entity, Ref<PostObjectStyle>)>>,
) {
    if !super::marked_changed(set.0.len(), &parts) {
        return;
    }
    set.0.clear();
    set.0.extend(parts.iter().map(|(e, _)| e));
}

#[repr(C)]
#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct DistortKey {
    states: PassStates,
    /// Draws with the wind vertex program (`GD_WIND_DISTORT`).
    wind: bool,
}

impl From<&DistortMaterial> for DistortKey {
    fn from(m: &DistortMaterial) -> Self {
        Self {
            states: m.states,
            wind: m.params.wind.y != 0.0,
        }
    }
}

impl Material for DistortMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
    fn reads_view_transmission_texture(&self) -> bool {
        true
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let s = key.bind_group_data.states;
        if key.bind_group_data.wind {
            descriptor.vertex.shader = super::material::WIND_SHADER;
            descriptor.vertex.shader_defs.push("GD_WIND_DISTORT".into());
        }
        descriptor.primitive.cull_mode = match s.cull {
            1 => Some(Face::Front),
            2 => Some(Face::Back),
            _ => None,
        };
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_write_enabled = Some(s.depth_write);
        }
        let (src, dst) = s.blend.unwrap_or((0, 8));
        let c = BlendComponent {
            src_factor: blend_factor(src),
            dst_factor: blend_factor(dst),
            operation: BlendOperation::Add,
        };
        // Alpha write off (state 19 = 0): the target's alpha is kept.
        let keep = BlendComponent {
            src_factor: BlendFactor::Zero,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        };
        if let Some(frag) = descriptor.fragment.as_mut() {
            for t in frag.targets.iter_mut().flatten() {
                t.blend = Some(BlendState {
                    color: c,
                    alpha: keep,
                });
            }
        }
        Ok(())
    }
}

/// Every world / doll camera gets Bevy's transmission texture (the glow camera,
/// which never draws distortion, does not).
fn enable_transmission(
    mut commands: Commands,
    cams: Query<
        Entity,
        (
            With<Camera3d>,
            Without<ScreenSpaceTransmission>,
            Without<super::post::GlowCamera>,
        ),
    >,
) {
    for e in &cams {
        commands
            .entity(e)
            .try_insert(ScreenSpaceTransmission::default());
    }
}

/// The distortion items of each view, kept out of Bevy's transmissive pass
/// between [`hold_distortion`] and [`restore_distortion`]: the
/// `postObjectRenderStyle` group, then the `postRenderStyle` one.
type Groups = (
    SortedRenderPhase<Transmissive3d>,
    SortedRenderPhase<Transmissive3d>,
);

#[derive(Resource, Default)]
struct HeldDistortion(HashMap<RetainedViewEntity, Groups>);

fn hold_distortion(
    mut phases: ResMut<ViewSortedRenderPhases<Transmissive3d>>,
    mut held: ResMut<HeldDistortion>,
    objects: Res<PostObjectEntities>,
) {
    for (view, phase) in phases.iter_mut() {
        if phase.items.is_empty() {
            continue;
        }
        let mut second = std::mem::take(phase);
        let mut first = SortedRenderPhase::<Transmissive3d>::default();
        let keys: Vec<_> = (second.items.keys())
            .filter(|k| objects.0.contains(&k.1.id()))
            .copied()
            .collect();
        for k in keys {
            if let Some(item) = second.items.shift_remove(&k) {
                first.items.insert(k, item);
            }
        }
        held.0.insert(*view, (first, second));
    }
}

fn restore_distortion(
    mut phases: ResMut<ViewSortedRenderPhases<Transmissive3d>>,
    mut held: ResMut<HeldDistortion>,
) {
    for (view, (mut first, second)) in held.0.drain() {
        if let Some(p) = phases.get_mut(&view) {
            first.items.extend(second.items);
            first.transient_items.extend(second.transient_items);
            *p = first;
        }
    }
}

/// `RenderPostObjects`: per style, copy the frame, then draw that style's
/// distortion over it.
fn draw_distortion(
    world: &World,
    view: ViewQuery<(
        &ExtractedCamera,
        &ExtractedView,
        &ViewTarget,
        &ViewDepthStencilTexture,
        Option<&ViewTransmissionTexture>,
    )>,
    held: Res<HeldDistortion>,
    mut ctx: RenderContext,
) {
    let view_entity = view.entity();
    let (camera, extracted, target, depth, transmission) = view.into_inner();
    let (Some((first, second)), Some(transmission), Some(size)) = (
        held.0.get(&extracted.retained_view_entity),
        transmission,
        camera.physical_target_size,
    ) else {
        return;
    };
    for phase in [first, second] {
        if phase.items.is_empty() {
            continue;
        }
        ctx.command_encoder().copy_texture_to_texture(
            target.main_texture().as_image_copy(),
            transmission.texture.as_image_copy(),
            size.to_extents(),
        );
        let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("gd_distortion"),
            color_attachments: &[Some(target.get_color_attachment())],
            depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(viewport) = camera.viewport.as_ref() {
            pass.set_camera_viewport(viewport);
        }
        if let Err(err) = phase.render(&mut pass, world, view_entity) {
            error!("distortion pass: {err:?}");
        }
    }
}
