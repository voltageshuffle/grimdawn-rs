//! The blended draws the engine makes into its G-buffer: the terrain patches, the decals' base layer and the grass
//! fringe.
//!
//! Engine.dll `GraphicsDeferredRenderer::RenderFinalScene`: the `deferred` technique of
//! every shader is drawn into the G-buffer before the lights and `ApplySSAO`, so its pixels are lit and occluded like
//! the ground under them. Blended ones among them: terrainpatch*.ssh `deferred` (sort 1050, a `TerrainDecoration`'s
//! painted terrain over its mesh, render/terrain.rs; SrcAlpha / OneMinusSrcAlpha, DepthWrite false, DepthFunc
//! LessEqual), decal_*.ssh `deferred` (sort 2000; SrcAlpha / OneMinusSrcAlpha,
//! DepthWrite false, DepthFunc LessEqual: blended over the terrain's albedo before lighting)
//! and grass.ssh `deferred` (sort 3001, after the opaque `deferredBase` 3000;
//! SrcAlpha / OneMinusSrcAlpha, no DepthWrite state so the D3D default, on, DepthFunc Less). The glow layer of a decal
//! is not part of it (it goes to the glow, after `ApplySSAO`), nor are the forward passes (water, blended meshes,
//! effects).
//!
//! Here: Bevy queues those meshes (entities with [`GBufferBlend`]) in its `Transparent3d` phase. [`hold_gbuffer_blend`]
//! takes them out of the main views' phases after batching (each kind keeps its back-to-front order; patches, decals,
//! then the fringe, as their sort values), [`draw_gbuffer_blend`] draws them after Bevy's opaque pass and before
//! `post::apply_ssao`, with their own pipelines (blend and depth states unchanged), and [`restore_gbuffer_blend`] puts
//! them back for the next frame's queue. So SSAO darkens them, and its depth input holds the fringe (it writes depth,
//! `material.rs` `depth_write_of`) but not the patches or decals (no depth write), as in the engine. The glow view
//! keeps them in its transparent pass (no SSAO there). PORT: the port lights each patch / decal / fringe pixel itself
//! and blends the lit colour (linear, equivalent to blending the albedo for the same light; a patch's own normals
//! are lit before the blend instead of blended).
//!
//! The hold / restore reuses its scratch (the taken item map, the picked `Vec` and each held phase's map) across
//! frames. It moves Bevy's queued items rather than queueing a phase of its own, which would re-implement Bevy's
//! transparent mesh queue and have to keep these entities out of it.
use super::post::{GammaOut, SsaoSet};
use bevy::camera::{MainPassResolutionOverride, Viewport};
use bevy::core_pipeline::core_3d::{Transparent3d, main_opaque_pass_3d};
use bevy::core_pipeline::{Core3d, Core3dSystems};
use bevy::ecs::entity::EntityHashMap;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::render_phase::{PhaseItem, SortedRenderPhase, ViewSortedRenderPhases};
use bevy::render::render_resource::{RenderPassDescriptor, StoreOp};
use bevy::render::renderer::{RenderContext, ViewQuery};
use bevy::render::view::{ExtractedView, RetainedViewEntity, ViewDepthStencilTexture, ViewTarget};
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};
use gd_formats::collections::HashMap;

/// A blended mesh drawn with the G-buffer (module doc), in this order.
#[derive(Component, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum GBufferBlend {
    /// terrainpatch*.ssh `deferred` (sort 1050): a `TerrainDecoration`'s painted terrain.
    TerrainPatch,
    /// decal_*.ssh `deferred` (sort 2000): a decal's base layer.
    Decal,
    /// grass.ssh `deferred` (sort 3001): the grass fringe.
    GrassFringe,
}

pub struct GBufferBlendPlugin;

impl Plugin for GBufferBlendPlugin {
    fn build(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<GBufferBlendEntities>()
            .init_resource::<HeldGBufferBlend>()
            .add_systems(ExtractSchedule, extract_gbuffer_blend)
            .add_systems(
                Render,
                (
                    hold_gbuffer_blend.in_set(RenderSystems::PrepareBindGroups),
                    restore_gbuffer_blend.in_set(RenderSystems::Cleanup),
                ),
            )
            .add_systems(
                Core3d,
                draw_gbuffer_blend
                    .after(main_opaque_pass_3d)
                    .before(SsaoSet)
                    .in_set(Core3dSystems::MainPass),
            );
    }
}

/// The main-world entities with [`GBufferBlend`], rebuilt only when the marked set changed.
#[derive(Resource, Default)]
struct GBufferBlendEntities(EntityHashMap<GBufferBlend>);

fn extract_gbuffer_blend(
    mut map: ResMut<GBufferBlendEntities>,
    parts: Extract<Query<(Entity, Ref<GBufferBlend>)>>,
) {
    if !super::marked_changed(map.0.len(), &parts) {
        return;
    }
    map.0.clear();
    map.0.extend(parts.iter().map(|(e, k)| (e, *k)));
}

/// Per main view, the held items: patches, decals, then the fringe.
#[derive(Resource, Default)]
struct HeldGBufferBlend(HashMap<RetainedViewEntity, SortedRenderPhase<Transparent3d>>);

/// After batching: a batch only joins neighbours of one pipeline, and each kind has its own (the decal material; the
/// fringe's `depth_strict` key), so moving a kind's items out in order keeps its batches whole.
fn hold_gbuffer_blend(
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut held: ResMut<HeldGBufferBlend>,
    kinds: Res<GBufferBlendEntities>,
    views: Query<&ExtractedView, With<GammaOut>>,
    mut take: Local<SortedRenderPhase<Transparent3d>>,
    mut picked: Local<Vec<(GBufferBlend, Transparent3d)>>,
) {
    if kinds.0.is_empty() {
        return;
    }
    for view in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        if !(phase.items.keys()).any(|k| kinds.0.contains_key(&k.1.id())) {
            continue;
        }
        // One pass: the rest goes back in its order, the held ones leave in theirs.
        // The taken map, the picked scratch and the held phase's map are reused across frames: swap the
        // phase's items out, then drain them back in place.
        let out = held.0.entry(view.retained_view_entity).or_default();
        out.items.clear();
        std::mem::swap(&mut phase.items, &mut take.items);
        picked.clear();
        for (k, item) in take.items.drain(..) {
            match kinds.0.get(&k.1.id()) {
                Some(kind) => picked.push((*kind, item)),
                None => {
                    phase.items.insert(k, item);
                }
            }
        }
        picked.sort_by_key(|(kind, _)| *kind); // stable: the back-to-front order within a kind
        out.items.extend(
            picked
                .drain(..)
                .map(|(_, item)| ((item.entity(), item.main_entity()), item)),
        );
    }
}

fn restore_gbuffer_blend(
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut held: ResMut<HeldGBufferBlend>,
) {
    // Put the held items back for the next frame's queue and batching; keep the now-empty
    // maps for reuse, dropping only views that are gone.
    held.0.retain(|view, out| {
        let Some(p) = phases.get_mut(view) else {
            return false;
        };
        p.items.extend(out.items.drain(..));
        p.transient_items.append(&mut out.transient_items);
        true
    });
}

/// The held items over the opaque pass's frame and depth, before `ApplySSAO`.
fn draw_gbuffer_blend(
    world: &World,
    view: ViewQuery<(
        &ExtractedCamera,
        &ExtractedView,
        &ViewTarget,
        &ViewDepthStencilTexture,
        Option<&MainPassResolutionOverride>,
    )>,
    held: Res<HeldGBufferBlend>,
    mut ctx: RenderContext,
) {
    let view_entity = view.entity();
    let (camera, extracted, target, depth, resolution_override) = view.into_inner();
    let Some(phase) = held.0.get(&extracted.retained_view_entity) else {
        return;
    };
    if phase.items.is_empty() {
        return;
    }
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("gd_gbuffer_blend"),
        color_attachments: &[Some(target.get_color_attachment())],
        depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some(viewport) =
        Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution_override)
    {
        pass.set_camera_viewport(&viewport);
    }
    if let Err(err) = phase.render(&mut pass, world, view_entity) {
        error!("G-buffer blend pass: {err:?}");
    }
}
