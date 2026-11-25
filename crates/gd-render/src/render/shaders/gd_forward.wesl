// Forward vertex program of the Grim Dawn materials: Bevy's mesh vertex (no morph targets in the port) with the
// engine's skinned normal/tangent frame.
//
// standardskinned.ssh `deferredVS` (and the skinned `*wiggle*` family,
// e.g. standardskinneddetailwiggle): the position is blended over the four bone
// influences, but the normal / tangent / binormal are turned by the FIRST bone
// only — the bone paired with weight slot 0 (`joint_indices.x`). The binormal is
// the negated stored one; the port rebuilds it in the fragment as
// `-tangent.w · cross(N, T)`, which is the same vector once N
// and T use the first bone. `skin_model` with weights (1,0,0,0) selects that
// bone's world matrix. The frame is normalised here (Bevy skin_normals /
// mesh_tangent_local_to_world) so the character's scale cancels in the
// fragment's `normalize(n.x·T + n.y·B + n.z·N)`, matching the deferredPS.
//
// standardstaticsnow.ssh `deferredVS`: the snow shaders lift the vertex
// by s · snowDepth along world y, s = sat((Nworld.y − snowThreshold) /
// (1 − snowThreshold)) of the vertex's world normal (the PS blends the snow at
// the same s; gd_standard.wesl takes it per pixel).
import bevy_pbr::render::forward_io::{Vertex, VertexOutput, decompress_vertex};
import bevy_pbr::render::{
    mesh_functions,
    skinning,
    view_transformations::position_world_to_clip,
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

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let mesh_world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
@if(SKINNED)
    let world_from_local = skinning::skin_model(vertex.joint_indices, vertex.joint_weights, vertex_in.instance_index);
@else
    let world_from_local = mesh_world_from_local;

    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;
@if(SKINNED && (VERTEX_NORMALS || VERTEX_TANGENTS))
    let first_bone_world = skinning::skin_model(vertex.joint_indices, vec4<f32>(1.0, 0.0, 0.0, 0.0), vertex_in.instance_index);
@if(VERTEX_NORMALS && SKINNED)
    out.world_normal = skinning::skin_normals(first_bone_world, vertex.normal);
@if(VERTEX_NORMALS && !SKINNED)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex_in.instance_index);
@if(VERTEX_TANGENTS && SKINNED)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(first_bone_world, vertex.tangent, vertex_in.instance_index);
@if(VERTEX_TANGENTS && !SKINNED)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, vertex.tangent, vertex_in.instance_index);
@if(VERTEX_NORMALS) {
    // The snow lift: world y += s · snowDepth. snowThreshold 1
    // (the ssh default) gives s = 0 (the engine's div_sat: 0/0 → 0).
    if (material.flags.y & FLAG_SNOW) != 0u && material.snow.y < 1.0 {
        let s = saturate((out.world_normal.y - material.snow.y) / (1.0 - material.snow.y));
        out.world_position.y += s * material.snow.x;
    }
}
    out.position = position_world_to_clip(out.world_position.xyz);
@if(VERTEX_COLORS)
    out.color = vertex.color;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_in.instance_index, mesh_world_from_local[3]);
    return out;
}
