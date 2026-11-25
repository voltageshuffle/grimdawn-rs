// Port of effects/portalenvmapped*.ssh: a cube map seen by reflection, tinted by the base texture.
import bevy_pbr::render::{
    forward_io::VertexOutput,
    mesh_view_bindings::view,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var base_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var base_smp: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var env_tex: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var env_smp: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var uv = vec2<f32>(0.0);
@if(VERTEX_UVS_A)
    uv = in.uv;
    let I = normalize(in.world_position.xyz - view.world_position);
    let N = normalize(in.world_normal);
    let R = I - 2.0 * dot(I, N) * N;
    let env = textureSample(env_tex, env_smp, normalize(R)).rgb;
    let base = textureSample(base_tex, base_smp, uv);
    if view.exposure > 0.1 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(env * base.rgb, base.a);
}
