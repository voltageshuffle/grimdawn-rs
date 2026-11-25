// Port of waterbasic.ssh basePS (DX11 profile). The bump is the type's NoiseTexture target
// (noisetexturegen.ssh composite + convert, rendered once per frame per type into a size² target
// by water.rs render_noise / water_noise.wesl), sampled bilinearly as the engine does.
// Vertex colour: r = depth opacity (lerp of depthParams by water depth,
// precomputed on the CPU like the game's VS), a = painted block / path alpha.
// waterBlockVS: uv from the world position, the type's direction; waterPathVS
// (scroll_time.y = 1): uv = the vertex's (+ scroll along v), the vertex's flow
// tangent (uv_b). Lighting: basePS's lightColor / lightDirection / skyAmbientColor
// are the scene's per-frame constants — the same EnvGpu every material reads
// (render/env.rs): env[0] sun dir (towards light) + on, env[1] sun colour,
// env[2] sky ambient.
import bevy_pbr::{
    render::forward_io::VertexOutput,
    render::mesh_view_bindings::{view, globals},
};

struct WaterParams {
    surface: vec4<f32>,        // x bump-vs-up weight, y spec power, z lighting blend
    water_color: vec4<f32>,
    tex_scale_dir: vec4<f32>,  // xy textureScale, zw textureDirection
    scroll_time: vec4<f32>,    // x textureScroll, y 1 = a path, z unused, w 1 = normalMap
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> water: WaterParams;
// The type's NoiseTexture target (repeat, bilinear): normalMap = 1 the converted bump (rg),
// normalMap = 0 the raw composite height (r).
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var noise_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var noise_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var sky_texture: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var sky_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var<storage, read> env: array<vec4<f32>>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var reflection_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var reflection_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Gamma-space output like every Grim Dawn material; the glow view gets
    // nothing (water has no glow).
    if view.exposure > 0.1 {
        return vec4(0.0, 0.0, 0.0, 0.0);
    }
    let p = in.world_position.xyz;
    var dir = water.tex_scale_dir.zw;
    var uv = p.xz * water.tex_scale_dir.xy + dir * water.scroll_time.x * globals.time;
    if water.scroll_time.y > 0.5 {
        dir = in.uv_b;
        uv = in.uv + vec2(0.0, water.scroll_time.x * globals.time);
    }
    // The target is the convert's bump; normalMap = 0: basePS samples the raw height's .xy·2−1
    // (both channels equal). The scrolling water uv moves over the target's fixed texel grid.
    var b = textureSample(noise_texture, noise_sampler, uv).rg;
    if water.scroll_time.w < 0.5 {
        b = vec2(b.r * 2.0 - 1.0);
    }
    let tangent = vec3(dir.x, 0.0, dir.y);
    let bitangent = vec3(-dir.y, 0.0, dir.x);
    let n = normalize(b.x * tangent + b.y * bitangent + water.surface.x * vec3(0.0, 1.0, 0.0));
    let v = normalize(view.world_position - p);
    let f = 1.0 - dot(n, v);
    var alpha = f * f + (1.0 - f * f) * in.color.r;
    let r = reflect(-v, n);
    let light_dir = env[0].xyz;
    let spec = exp2(log2(max(dot(r, light_dir), 0.0)) * water.surface.y);
    alpha = alpha + spec;
    let sky = textureSample(sky_texture, sky_sampler, r).rgb;
    let c_sky = sky * water.water_color.rgb + spec;
    // reflectPS (waterbasic.ssh): the planar scene reflection
    // at the water pixel's screen uv, distorted by the bump, blended over the sky
    // reflection by min(refl.a·1.5, 0.75). The surface.z lerp below gates it to the
    // reflecting water types (AddReflectionPlanes: surfaceParams.z > 0).
    let screen_uv = in.position.xy / view.viewport.zw;
    let bump01 = b * 0.5 + vec2<f32>(0.5);
    let refl_uv = screen_uv + (bump01 - vec2<f32>(0.5, 0.55)) * 0.25 / water.surface.x;
    let refl = textureSample(reflection_texture, reflection_sampler, refl_uv);
    let c = mix(c_sky, refl.rgb, min(refl.a * 1.5, 0.75));
    // AddSceneLight skips a near-black sun (gd_standard.wesl's env[0].w gate).
    let sun = select(vec3(0.0), env[1].rgb, env[0].w > 0.5);
    let lit = sun * light_dir.y + env[2].rgb;
    let rgb = mix(water.water_color.rgb, c * lit, water.surface.z);
    return vec4(saturate(rgb), clamp(alpha * in.color.a, 0.0, 1.0));
}