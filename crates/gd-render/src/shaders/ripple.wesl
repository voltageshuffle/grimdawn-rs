// Port of WaterRipple.ssh (DirectX11 profile; see water.rs). The engine builds one quad per
// live ripple on the CPU: corners c + r + q (uv 1,1), c - r + q (0,1), c - r - q (0,0), c + r - q (1,0),
// r = (cos a, 0, sin a) size, q = (-sin a, 0, cos a) size; vertex colour alpha only.
// VS: o2 = (0.5, 0.5 + 0.5 k, 0.5), k = dot(lightDirection, (-1, -1, -1)).
// PS: n = 2 (tex.rgb - 0.5); rgb = 0.5 + 0.5 (k n.y); a = tex.a x vertex alpha.
import bevy_pbr::render::{
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
};

// x = k.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var ripple_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var ripple_sampler: sampler;
// Header vec4 (x = ripple count as u32 bits), then 2 vec4 per ripple: [centre, 0], [r.x, r.z, alpha, 0].
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var<storage, read> ripples: array<vec4<f32>>;

struct Vertex {
    @location(0) position: vec3<f32>,
    // x = ripple index, y = corner 0..3: uv (0,0)(1,0)(1,1)(0,1).
    @location(1) slot: vec2<f32>,
}

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
}

@vertex
fn vertex(vin: Vertex) -> Out {
    var out: Out;
    let index = u32(vin.slot.x);
    if index >= bitcast<u32>(ripples[0].x) {
        out.clip = vec4(0.0, 0.0, 0.0, 1.0);
        out.uv = vec2(0.0);
        out.alpha = 0.0;
        return out;
    }
    let c = ripples[1u + index * 2u];
    let d = ripples[2u + index * 2u];
    let corner = u32(vin.slot.y);
    let uv = vec2(f32(corner == 1u || corner == 2u), f32(corner >= 2u));
    let r = vec3(d.x, 0.0, d.y);
    let q = vec3(-d.y, 0.0, d.x);
    let world = c.xyz + r * (uv.x * 2.0 - 1.0) + q * (uv.y * 2.0 - 1.0);
    out.clip = position_world_to_clip(world);
    out.uv = uv;
    out.alpha = d.z;
    return out;
}

@fragment
fn fragment(in: Out) -> @location(0) vec4<f32> {
    // Not part of the G-buffer glow: skip the port's glow camera (see fx/particle.wesl).
    if view.exposure > 0.1 {
        discard;
    }
    let t = textureSample(ripple_texture, ripple_sampler, in.uv);
    let n = (t.rgb - vec3(0.5)) * 2.0;
    let shade = 0.5 + 0.5 * (params.x * n.y);
    return vec4(vec3(shade), t.a * in.alpha);
}
