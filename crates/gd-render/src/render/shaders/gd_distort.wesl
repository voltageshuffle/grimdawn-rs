// Port of the distort*.ssh `distortPS` programs (DirectX11): refracts the frame copy (Bevy's view transmission
// texture) through the mesh.
import bevy_pbr::render::{
    forward_io::VertexOutput,
    mesh_view_bindings::{view, globals, view_transmission_texture, view_transmission_sampler},
    view_transformations::position_world_to_clip,
};

struct DistortParams {
    scroll: vec4<f32>,
    values: vec4<f32>,
    flags: vec4<u32>,
    // The wind vertex program's.
    wind: vec4<f32>,
    ripple_scale: vec4<f32>,
    ripple_amplitude: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> material: DistortParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var base_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var base_smp: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var bump_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var bump_smp: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var uv = vec2<f32>(0.0);
@if(VERTEX_UVS_A)
    uv = in.uv;
    let t = globals.time;
    // VS: v1.zw = uv + fract(scrollRate·t), v2.xy = uv + fract(bumpScrollRate·t).
    let uv_s = uv + fract(material.scroll.xy * t);
    let uv_b = uv + fract(material.scroll.zw * t);

    // View vector in the (T, −B, N) frame (B = sign·(N×T)).
    let Nv = normalize(in.world_normal);
    var T = vec3<f32>(1.0, 0.0, 0.0);
    var Bn = vec3<f32>(0.0, 0.0, 1.0);
@if(VERTEX_TANGENTS) {
    if material.flags.z != 0u {
        T = in.world_tangent.xyz;
        Bn = -in.world_tangent.w * cross(Nv, T);
    }
}
    let V = normalize(view.world_position - in.world_position.xyz);
    let vts = normalize(vec3<f32>(dot(V, T), dot(V, Bn), dot(V, Nv)));

    let n = (textureSample(bump_tex, bump_smp, uv).rgb - vec3<f32>(0.5)) * 2.0;
    let edge = 1.0 - saturate(dot(vts, n));
    let k = textureSample(bump_tex, bump_smp, uv_b).a * material.values.x * edge + 1.0;
    let c = position_world_to_clip(in.world_position.xyz);
    let w = c.w * k;
    let suv = vec2<f32>(c.x / w, -c.y / w) * 0.5 + vec2<f32>(0.5);
    let screen = textureSampleLevel(view_transmission_texture, view_transmission_sampler, suv, 0.0).rgb;

    // Colour from uv and alpha from the scrolled uv; `*colorscroll*` swaps them.
    var col_uv = uv;
    var a_uv = uv_s;
    if material.flags.y != 0u {
        col_uv = uv_s;
        a_uv = uv;
    }
    let base = textureSample(base_tex, base_smp, col_uv).rgb;
    let a = textureSample(base_tex, base_smp, a_uv).a * material.values.z;
    // The glow view never draws these (main layer only).
    if view.exposure > 0.1 {
        return vec4<f32>(0.0);
    }
    if material.flags.x != 0u {
        // distortadditive*: screen·a + base, both × fade (One / InvSrcAlpha).
        return vec4<f32>(screen * a + base * material.values.z, a);
    }
    // distort*: screen·sat(base + 1 − tint)·a.
    let tinted = saturate(base + vec3<f32>(1.0 - material.values.y));
    return vec4<f32>(screen * tinted * a, a);
}
