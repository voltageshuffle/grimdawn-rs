// Port of the weatherparticle{simple,directional}[additive] and soft* DirectX11 programs
// (Shaders/particle/*.ssh; see grimdawn/src/weather.rs). Per vertex the engine sends the
// centre, the colour, (u, v, size, spin) and (directional) the velocity; C = cameraDirection:
// simple:      R = normalize((C.z, 0, -C.x)) (horizontal), axis(a) = R cos a + Y sin a,
//              pos = centre + (u - 1/2) size axis(spin) + (v - 1/2) size axis(spin + 1.57)
// directional: D = normalize(velocity), S = normalize(D x C),
//              pos = centre + (u - 1/2) size S + (v - 1/2) size D
// PS: texture x colour; the soft variants read the scene depth: a linear depth difference
// d (metres) < 0 is discarded, else alpha x saturate(d / 1.0) (the PS constant is 1.0).
import bevy_pbr::render::{
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
};

// x = 1 directional, y = 1 soft.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var weather_texture: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var weather_sampler: sampler;
// Header vec4 (x = particle count as u32 bits), then 3 vec4 per particle:
// [centre, size], [velocity, spin], [colour].
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var<storage, read> particles: array<vec4<f32>>;
// The scene depth after the opaque passes (copied by weather.rs).
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var scene_depth: texture_depth_2d;

struct Vertex {
    @location(0) position: vec3<f32>,
    // x = particle index, y = corner 0..3: uv (0,0)(1,0)(1,1)(0,1).
    @location(1) slot: vec2<f32>,
}

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    // View-space distance of the fragment along the view direction (metres).
    @location(2) depth: f32,
}

@vertex
fn vertex(vin: Vertex) -> Out {
    var out: Out;
    let index = u32(vin.slot.x);
    if index >= bitcast<u32>(particles[0].x) {
        out.clip = vec4(0.0, 0.0, 0.0, 1.0);
        out.uv = vec2(0.0);
        out.color = vec4(0.0);
        out.depth = 0.0;
        return out;
    }
    let base = 1u + index * 3u;
    let p0 = particles[base];
    let p1 = particles[base + 1u];
    let corner = u32(vin.slot.y);
    let uv = vec2(f32(corner == 1u || corner == 2u), f32(corner >= 2u));
    let c = -view.world_from_view[2].xyz;
    let size = p0.w;
    var world: vec3<f32>;
    if params.x > 0.5 {
        let d = normalize(p1.xyz);
        let s = normalize(cross(d, c));
        world = p0.xyz + s * ((uv.x - 0.5) * size) + d * ((uv.y - 0.5) * size);
    } else {
        let r = normalize(vec3(c.z, 0.0, -c.x));
        let up = vec3(0.0, 1.0, 0.0);
        let a1 = r * cos(p1.w) + up * sin(p1.w);
        let a2 = r * cos(p1.w + 1.57) + up * sin(p1.w + 1.57);
        world = p0.xyz + a1 * ((uv.x - 0.5) * size) + a2 * ((uv.y - 0.5) * size);
    }
    out.clip = position_world_to_clip(world);
    out.uv = uv;
    out.color = particles[base + 2u];
    out.depth = -(view.view_from_world * vec4(world, 1.0)).z;
    return out;
}

@fragment
fn fragment(in: Out) -> @location(0) vec4<f32> {
    // Not part of the G-buffer glow: skip the port's glow camera (see fx/particle.wesl).
    if view.exposure > 0.1 {
        discard;
    }
    var t = textureSample(weather_texture, weather_sampler, in.uv, 0) * in.color;
    if params.y > 0.5 {
        // The sky (depth 0, reverse Z) never fades.
        let dims = vec2<i32>(textureDimensions(scene_depth)) - vec2(1);
        let px = clamp(vec2<i32>(in.clip.xy), vec2(0), dims);
        let ndc = textureLoad(scene_depth, px, 0);
        if ndc <= 0.0 {
            return t;
        }
        let p = view.view_from_clip * vec4(0.0, 0.0, ndc, 1.0);
        let diff = -p.z / p.w - in.depth;
        if diff < 0.0 {
            discard;
        }
        t.a = t.a * saturate(diff / 1.0);
    }
    return t;
}
