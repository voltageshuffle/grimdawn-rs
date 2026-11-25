// Vertex program of the wind ("wiggle") shaders:
// Bevy's mesh / prepass vertex, skinned or not, with the position moved by
//   a_i = (p_i + o_i)·rippleScale_i + time·foliageSpeed,  s = sin(a)
//   d   = (s_y·s_z, s_z·s_x, s_x·s_y), or (s_y·k_z, s_z·k_x, s_x·k_y) with
//         k = sin(a + 1.57) (additivescrollwiggly*)
//   p  += d · c.rgb · c.a · 0.1 · foliageAmplitude · rippleAmplitude
// c = vertex colour, o = the object's world position (`objectToWorldMatrix`
// translation; PORT for `objectToRegionOffset`: only a phase) or none
// ("wiggly"). p is the bind-pose position ("wiggly" skinned programs) or the
// skinned one in object space ("wiggle" skinned programs; the offset is turned
// into the world by the object's frame). Also the vertex program of the
// distortion material under GD_WIND_DISTORT.
// Mode 4 is the terrain grass bend instead (grass.ssh vertex programs):
// with h = 1 − v (1 at the blade's top) and
// a = sin(p.x + time)·h·windStrength, b = sin(p.z + time + 0.5)·h·windStrength,
//   p.x += 0.075·a + 0.025·b,  p.z += 0.025·a + 0.075·b
// (windStrength = the mesh tag's f32 bits; p terrain-local).
// Then the offset field d (binding 14 at the point uv_b.x):
//   p.xz += h·d,  p.y −= H − √(H² − |h·d|²)   (H = bladeHeight, ripple_scale.x).
@if(PREPASS_PIPELINE)
import bevy_pbr::prepass::io::{Vertex, VertexOutput, decompress_vertex};
@if(!PREPASS_PIPELINE)
import bevy_pbr::render::forward_io::{Vertex, VertexOutput, decompress_vertex};
@if(!PREPASS_PIPELINE)
import bevy_pbr::render::mesh_view_bindings::globals;
@if(PREPASS_PIPELINE)
import bevy_render::globals::Globals;
@if(PREPASS_PIPELINE)
import bevy_pbr::render::mesh_view_bindings::view;
import bevy_pbr::render::{
    mesh_functions,
    skinning,
    view_transformations::position_world_to_clip,
};

// Same layout as gd_standard.wesl.
@if(!GD_WIND_DISTORT)
struct GdParams {
    diffuse: vec4<f32>,
    specular: vec4<f32>,
    scroll: vec4<f32>,
    snow: vec4<f32>,
    flags: vec4<u32>,
    fade: vec4<f32>,
    outline: vec4<f32>,
    // x: foliageSpeed, y: foliageAmplitude, z: 1 = add the object offset,
    // w: 1 = the sin·cos form, 2 = before skinning, 4 = the grass bend.
    wind: vec4<f32>,
    ripple_scale: vec4<f32>,
    ripple_amplitude: vec4<f32>,
    ice_inner: vec4<f32>,
    ice_top: vec4<f32>,
    ice_bottom: vec4<f32>,
    ice: vec4<f32>,
    dissolve: vec4<f32>,
}

// Same layout as gd_distort.wesl, the wind fields as above.
@if(GD_WIND_DISTORT)
struct GdParams {
    scroll: vec4<f32>,
    values: vec4<f32>,
    flags: vec4<u32>,
    wind: vec4<f32>,
    ripple_scale: vec4<f32>,
    ripple_amplitude: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> material: GdParams;

// The grass offset fields: per point the disturbance offset (xy) and the
// disturbance + wind offset (zw).
@if(!GD_WIND_DISTORT)
@group(constants::MATERIAL_BIND_GROUP) @binding(14) var<storage, read> grass_offsets: array<vec4<f32>>;

// The prepass view layout has the globals at binding 1.
@if(PREPASS_PIPELINE)
@group(0) @binding(1) var<uniform> globals: Globals;

fn wind_mode() -> u32 {
    return u32(material.wind.w);
}

fn wind_offset(p: vec3<f32>, c: vec4<f32>, origin: vec3<f32>) -> vec3<f32> {
    let q = p + origin * material.wind.z;
    let a = q * material.ripple_scale.xyz + vec3<f32>(globals.time * material.wind.x);
    let s = sin(a);
    var d = vec3<f32>(s.y * s.z, s.z * s.x, s.x * s.y);
    if (wind_mode() & 1u) != 0u {
        let k = sin(a + vec3<f32>(1.57));
        d = vec3<f32>(s.y * k.z, s.z * k.x, s.x * k.y);
    }
    return d * c.rgb * (c.a * 0.1 * material.wind.y) * material.ripple_amplitude.xyz;
}

/// grass.ssh: the blade bend by the wind.
fn grass_bend(p: vec3<f32>, v: f32, strength: f32) -> vec3<f32> {
    let h = 1.0 - v;
    let ab = sin(vec2<f32>(p.x + globals.time, p.z + globals.time + 0.5)) * (h * strength);
    return vec3<f32>(0.075 * ab.x + 0.025 * ab.y, 0.0, 0.025 * ab.x + 0.075 * ab.y);
}

/// grass.ssh: the offset field's push. PORT: the root's argument is kept ≥ 0 (the engine's would be NaN
/// for an offset longer than the blade).
fn grass_push(v: f32, d: vec2<f32>) -> vec3<f32> {
    let hd = (1.0 - v) * d;
    let tall = material.ripple_scale.x;
    let drop = tall - sqrt(max(tall * tall - dot(hd, hd), 0.0));
    return vec3<f32>(hd.x, -drop, hd.y);
}

/// The point-light shadow layout of this view: (W / S, S) of the engine map
/// (W texels) in the top-left of the S-texel layer, or (0, 0) for any other
/// view. Tag: an aspect-1 infinite reverse perspective whose focal length is
/// (n + 64 k)·1e-4 (a field of view just under 180°), n = 64 W / S, S = 1024·2^k
/// (render/point_shadow.rs `spot_code`).
@if(PREPASS_PIPELINE)
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
@if(PREPASS_PIPELINE)
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
    var vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let mesh_world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    let origin = mesh_world_from_local[3].xyz;
@if(VERTEX_COLORS && SKINNED) {
    if (wind_mode() & 2u) != 0u {
        vertex.position += wind_offset(vertex.position, vertex.color, origin);
    }
}
@if(VERTEX_COLORS && !SKINNED)
    vertex.position += wind_offset(vertex.position, vertex.color, origin);
@if(VERTEX_UVS_A && !SKINNED) {
    if (wind_mode() & 4u) != 0u {
        vertex.position += grass_bend(
            vertex.position,
            vertex.uv.y,
            bitcast<f32>(mesh_functions::get_tag(vertex_in.instance_index)),
        );
    }
}
@if(VERTEX_UVS_A && VERTEX_UVS_B && !SKINNED && !GD_WIND_DISTORT) {
    if (wind_mode() & 4u) != 0u {
        // uv_b: the blade's point in the offset fields, 1 when its layer takes the wind.
        let q = grass_offsets[u32(vertex.uv_b.x)];
        vertex.position += grass_push(vertex.uv.y, select(q.xy, q.zw, vertex.uv_b.y > 0.5));
    }
}
@if(SKINNED)
    let world_from_local = skinning::skin_model(vertex.joint_indices, vertex.joint_weights, vertex_in.instance_index);
@else
    let world_from_local = mesh_world_from_local;

    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
@if(VERTEX_COLORS && SKINNED) {
    if (wind_mode() & 2u) == 0u {
        // The skinned position in object space, moved there.
        let local = mesh_functions::get_local_from_world(vertex_in.instance_index) * out.world_position;
        let off = wind_offset(local.xyz, vertex.color, origin);
        out.world_position += mesh_world_from_local * vec4<f32>(off, 0.0);
    }
}
    out.position = position_world_to_clip(out.world_position.xyz);
@if(PREPASS_PIPELINE) {
    let map_layout = paraboloid_layout();
    if map_layout.x > 0.0 {
        out.position = paraboloid_clip(out.world_position.xyz, map_layout);
    }
}
@if(PREPASS_PIPELINE && UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
}
@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;
@if((!PREPASS_PIPELINE || NORMAL_PREPASS_OR_DEFERRED_PREPASS) && SKINNED && (VERTEX_NORMALS || VERTEX_TANGENTS))
    // standardskinned / skinned-wiggle deferredVS: the normal / tangent
    // frame turns by the FIRST bone only (weight slot 0); the position stays blended.
    let first_bone_world = skinning::skin_model(vertex.joint_indices, vec4<f32>(1.0, 0.0, 0.0, 0.0), vertex_in.instance_index);
@if(VERTEX_NORMALS && (!PREPASS_PIPELINE || NORMAL_PREPASS_OR_DEFERRED_PREPASS) && SKINNED)
    out.world_normal = skinning::skin_normals(first_bone_world, vertex.normal);
@if(VERTEX_NORMALS && (!PREPASS_PIPELINE || NORMAL_PREPASS_OR_DEFERRED_PREPASS) && !SKINNED)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex_in.instance_index);
@if(VERTEX_TANGENTS && (!PREPASS_PIPELINE || NORMAL_PREPASS_OR_DEFERRED_PREPASS) && SKINNED)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(first_bone_world, vertex.tangent, vertex_in.instance_index);
@if(VERTEX_TANGENTS && (!PREPASS_PIPELINE || NORMAL_PREPASS_OR_DEFERRED_PREPASS) && !SKINNED)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, vertex.tangent, vertex_in.instance_index);
@if(VERTEX_COLORS)
    out.color = vertex.color;
@if(PREPASS_PIPELINE && MOTION_VECTOR_PREPASS)
    out.previous_world_position = out.world_position;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_in.instance_index, mesh_world_from_local[3]);
    return out;
}
