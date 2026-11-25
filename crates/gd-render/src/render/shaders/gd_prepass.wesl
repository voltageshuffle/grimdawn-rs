// Shadow-pass vertex program of the Grim Dawn materials:
// Bevy's prepass vertex (skinning; no morph targets in the port) plus the
// engine's point-light paraboloid: in a point-light
// shadow view (a shadow-only spot light whose projection carries the map
// layout) the vertex goes to the lower-hemisphere paraboloid of
// `pLightShadowVS` instead of the view's projection. The foliage wind vertex
// program (gd_wind.wesl) does the same. Skinned normals/tangents use the FIRST
// bone only (standardskinned.ssh deferredVS).
//
// The snow lift (gd_forward.wesl, standardstaticsnow deferredVS):
// world y += s · snowDepth, s = sat((Nworld.y − snowThreshold) / (1 − snowThreshold)).
// PORT: the engine's depthMapVS is not traced; the shadow depth lifts like the
// deferred VS so it matches the lifted surface.
import bevy_pbr::{
    prepass::io::{Vertex, VertexOutput, decompress_vertex},
    render::{
        mesh_functions,
        skinning,
        mesh_view_bindings::view,
        view_transformations::position_world_to_clip,
    },
};

// Same layout as gd_standard.wesl (the snow lift reads snow + flags).
struct GdParams {
    diffuse: vec4<f32>,
    specular: vec4<f32>,
    scroll: vec4<f32>,
    snow: vec4<f32>,
    flags: vec4<u32>,
    fade: vec4<f32>,
    outline: vec4<f32>,
    wind: vec4<f32>,
    ripple_scale: vec4<f32>,
    ripple_amplitude: vec4<f32>,
    ice_inner: vec4<f32>,
    ice_top: vec4<f32>,
    ice_bottom: vec4<f32>,
    ice: vec4<f32>,
    dissolve: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> material: GdParams;

const FLAG_SNOW: u32 = 8u;

/// The point-light shadow layout of this view: (W / S, S) of the engine map
/// (W texels) in the top-left of the S-texel layer, or (0, 0) for any other
/// view. Tag: an aspect-1 infinite reverse perspective whose focal length is
/// (n + 64 k)·1e-4 (a field of view just under 180°), n = 64 W / S, S = 1024·2^k
/// (point_shadow.rs `spot_code`).
fn paraboloid_layout() -> vec2<f32> {
    let p = view.clip_from_view;
    if p[2][3] != -1.0 || p[3][3] != 0.0 || p[1][1] >= 0.02 || p[0][0] != p[1][1] {
        return vec2<f32>(0.0);
    }
    let c = round(p[1][1] * 1e4);
    let k = floor(c / 64.0);
    let n = c - 64.0 * k;
    return vec2<f32>(n / 64.0, 1024.0 * exp2(k));
}

/// `pLightShadowVS`: light frame rows (−1,0,0), (0,1,0), (0,0,−1) about the
/// light; q = the unit direction, clip xy = (q.x, q.z) / (q.y − 1), depth
/// (d − 0.1) / 29.9 (Bevy's reverse Z: 1 − that), w = 1; the engine's viewport
/// (1, 1, W − 2, W − 2) of its W map. A vertex above the light's horizon is
/// clipped (the engine's alpha test `a·(0.5 − q.y) ≥ 0.5` drops it).
fn paraboloid_clip(world: vec3<f32>, map_layout: vec2<f32>) -> vec4<f32> {
    let v = world - view.world_position;
    let d = max(length(v), 1e-6);
    let q = vec3<f32>(-v.x, v.y, -v.z) / d;
    let st = vec2<f32>(q.x, q.z) / min(q.y - 1.0, -1e-3);
    let s = map_layout.y;
    let w = map_layout.x * s;
    let px = vec2<f32>(1.0) + (vec2<f32>(0.5, 0.5) + vec2<f32>(0.5, -0.5) * st) * (w - 2.0);
    let ndc = vec2<f32>(2.0 * px.x / s - 1.0, 1.0 - 2.0 * px.y / s);
    var z = 1.0 - (d - 0.1) * 0.0334448;
    if q.y > 0.0 {
        z = -1.0;
    }
    return vec4<f32>(ndc, z, 1.0);
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
@if(SKINNED)
    let world_from_local = skinning::skin_model(vertex.joint_indices, vertex.joint_weights, vertex_in.instance_index);
@else
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);

    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
@if(VERTEX_NORMALS) {
    if (material.flags.y & FLAG_SNOW) != 0u && material.snow.y < 1.0 {
@if(SKINNED)
        let n = skinning::skin_normals(skinning::skin_model(vertex.joint_indices, vec4<f32>(1.0, 0.0, 0.0, 0.0), vertex_in.instance_index), vertex.normal);
@if(!SKINNED)
        let n = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex_in.instance_index);
        let s = saturate((n.y - material.snow.y) / (1.0 - material.snow.y));
        out.world_position.y += s * material.snow.x;
    }
}
    let map_layout = paraboloid_layout();
    if map_layout.x > 0.0 {
        out.position = paraboloid_clip(out.world_position.xyz, map_layout);
    } else {
        out.position = position_world_to_clip(out.world_position.xyz);
    }
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
}
@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && SKINNED && (VERTEX_NORMALS || VERTEX_TANGENTS))
    // standardskinned.ssh deferredVS: the normal / tangent frame turns by the FIRST bone only (module doc).
    let first_bone_world = skinning::skin_model(vertex.joint_indices, vec4<f32>(1.0, 0.0, 0.0, 0.0), vertex_in.instance_index);
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_NORMALS && SKINNED)
    out.world_normal = skinning::skin_normals(first_bone_world, vertex.normal);
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_NORMALS && !SKINNED)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex_in.instance_index);
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_TANGENTS && SKINNED)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(first_bone_world, vertex.tangent, vertex_in.instance_index);
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_TANGENTS && !SKINNED)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, vertex.tangent, vertex_in.instance_index);
@if(VERTEX_COLORS)
    out.color = vertex.color;
@if(MOTION_VECTOR_PREPASS)
    out.previous_world_position = out.world_position;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_in.instance_index, world_from_local[3]);
    return out;
}
