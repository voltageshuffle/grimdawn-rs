// Port of the particle*.ssh DirectX11 vertex/pixel shaders (particleadditive /
// particlecombine). The game builds the billboard in
// object space: C = camera direction (or +Y with orientVertically),
// A = D' × C (or +X), B = normalize(A × C), corner axes
//   U = B·sin θ − Â·cos θ,  V = (B·sin(θ+1.57) − Â·cos(θ+1.57))·(1 + stretch·speed)
//   pos = centre + (U·(u−½) + V·(v−½))·2·size
// vs of every particle*.ssh (three dp3 of the vertex's D with the
// invObjectToWorldMatrix rows, as for the camera direction): D' = D taken
// through the inverse object matrix, although D (the particle velocity, or +Y)
// is already in the emitter's space. For world-space emitters the object
// matrix is the identity (Emitter::RenderPass); a local emitter's billboards
// roll (and velocity-aligned ones stretch) by the inverse of its frame.
// PS: texture · vertex colour, blended on the gamma-space target.
// particledistort* (GD_DISTORT; render/distort.rs draws them after the scene):
// psDistort: t = tex(uv), a = t.a·c.a, discard a < 0.01;
// k = 1 + a·(2·c.b² − ½); p = (corner − centre)/k + centre (clip xyw);
// suv = (p.x/p.w, −p.y/p.w)·½ + ½ + (2·c.rg − 1)·a·screenTextureScale·16;
// rgb = lerp(1, t.rgb, a)·screen(suv) (`particledistortadditive`: t.rgb + screen),
// blend One / Zero.
import bevy_pbr::render::{
    mesh_functions,
    mesh_view_bindings::{view, view_transmission_texture, view_transmission_sampler},
    view_transformations::position_world_to_clip,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> particle: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var particle_texture: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var particle_sampler: sampler;

// Header vec4 (x = particle count as u32 bits), then 4 vec4 per particle:
// [centre, half-size], [dir, stretch], [rotation, speed, frame, 0], [colour].
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var<storage, read> particles: array<vec4<f32>>;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    // x = particle index, y = corner 0..3 (the mesh positions are all zero).
    @location(1) slot: vec2<f32>,
}

struct PVertex {
    instance_index: u32,
    position: vec3<f32>,
    uv: vec2<f32>,
    dir: vec4<f32>,
    params: vec4<f32>,
    color: vec4<f32>,
}

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) layer: u32,
    // Clip positions of the particle's centre and of this corner (distortion).
    @location(3) centre: vec4<f32>,
    @location(4) corner: vec4<f32>,
}

@vertex
fn vertex(vin: Vertex) -> Out {
    let index = u32(vin.slot.x);
    if index >= bitcast<u32>(particles[0].x) {
        // Unused slot of the capacity-sized mesh: collapse to a point.
        var dead: Out;
        dead.clip = vec4(0.0, 0.0, 0.0, 1.0);
        dead.uv = vec2(0.0);
        dead.color = vec4(0.0);
        dead.layer = 0u;
        dead.centre = vec4(0.0, 0.0, 0.0, 1.0);
        dead.corner = vec4(0.0, 0.0, 0.0, 1.0);
        return dead;
    }
    let base = 1u + index * 4u;
    let p0 = particles[base];
    let p1 = particles[base + 1u];
    let p2 = particles[base + 2u];
    let corner = u32(vin.slot.y);
    var v: PVertex;
    v.instance_index = vin.instance_index;
    v.position = p0.xyz;
    v.uv = vec2(f32(corner == 1u || corner == 2u), f32(corner >= 2u));
    v.dir = p1;
    v.params = vec4(p2.x, p0.w, p2.y, p2.z);
    v.color = particles[base + 3u];
    return shade(v);
}

fn shade(v: PVertex) -> Out {
    let m = mesh_functions::get_world_from_local(v.instance_index);
    let rot = mat3x3<f32>(m[0].xyz, m[1].xyz, m[2].xyz);
    // Camera forward in object space (invObjectToWorldMatrix · cameraDirection).
    let fwd = -view.world_from_view[2].xyz;
    let c0 = normalize(transpose(rot) * fwd);
    let ov = particle.x;
    let ow = 1.0 - ov;
    let c = c0 * ow + vec3(0.0, 1.0, 0.0) * ov;
    let d = transpose(rot) * v.dir.xyz;
    let a = cross(d, c) * ow + vec3(1.0, 0.0, 0.0) * ov;
    let b = normalize(cross(a, c));
    let an = normalize(a);
    let th = v.params.x;
    let vax = (b * sin(th + 1.57) - an * cos(th + 1.57)) * (v.dir.w * v.params.z + 1.0) * (v.uv.y - 0.5);
    let uax = (b * sin(th) - an * cos(th)) * (v.uv.x - 0.5);
    let s2 = v.params.y + v.params.y;
    let p = v.position + uax * s2 + vax * s2;
    var out: Out;
    let world = m * vec4(p, 1.0);
    out.clip = position_world_to_clip(world.xyz);
    out.corner = out.clip;
    out.centre = position_world_to_clip((m * vec4(v.position, 1.0)).xyz);
    out.uv = v.uv;
    out.color = v.color;
    out.layer = u32(v.params.w);
    return out;
}

@fragment
fn fragment(in: Out) -> @location(0) vec4<f32> {
    // Particles are forward-drawn after the deferred passes and are not part
    // of the G-buffer glow: skip the port's glow camera (marked by exposure,
    // see render/post.rs).
    if view.exposure > 0.1 {
        discard;
    }
    let t = textureSample(particle_texture, particle_sampler, in.uv, in.layer);
@if(GD_DISTORT) {
    let a = t.a * in.color.a;
    if a < 0.01 {
        discard;
    }
    let k = a * (2.0 * in.color.b * in.color.b - 0.5) + 1.0;
    let p = (in.corner.xyw - in.centre.xyw) / k + in.centre.xyw;
    let off = (in.color.rg * 2.0 - vec2(1.0)) * a * particle.y * 16.0;
    let suv = vec2(p.x / p.z, -p.y / p.z) * 0.5 + vec2(0.5) + off;
    let screen = textureSampleLevel(view_transmission_texture, view_transmission_sampler, suv, 0.0).rgb;
    if particle.z > 0.5 {
        return vec4(t.rgb + screen, a);
    }
    return vec4(mix(vec3(1.0), t.rgb, a) * screen, a);
}
    return t * in.color;
}
