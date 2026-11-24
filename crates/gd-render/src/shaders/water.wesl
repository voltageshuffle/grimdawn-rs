// Port of waterbasic.ssh basePS (DX11 profile) with the engine's NoiseTexture
// (noisetexturegen.ssh): the bump is 4 octaves of the 256-texture bank, crossfaded
// and weighted exactly as the engine composites them into its size² render target
// (PORT: evaluated per water pixel — 3 heights × 8 array samples — instead of
// rendering the target; the same function, the 32² slices are cache-resident).
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
    scroll_time: vec4<f32>,    // x textureScroll, y 1 = a path, z 1/noise size, w 1 = normalMap
    noise_levels: vec4<f32>,   // NoiseTexture octave weights
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> water: WaterParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var noise_textures: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var noise_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var sky_texture: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var sky_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var<storage, read> env: array<vec4<f32>>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var reflection_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var reflection_sampler: sampler;
// The water type's live octave crossfade (water.rs update_water_noise, one shared buffer per type,
// rewritten in place every frame): [0] phases, [1] previous bank slices, [2] current bank slices.
@group(constants::MATERIAL_BIND_GROUP) @binding(8) var<storage, read> noise_state: array<vec4<f32>>;

// noisetexturegen.ssh compositePS: Σ levels[i] · mix(prev, cur, phase) sampled at
// uv · 2^i — the octave scales (1, 2, 4, 8), Engine.dll `Engine::NoiseOctaveUvScales`.
fn noise_height(uv: vec2<f32>) -> f32 {
    var h = 0.0;
    for (var i = 0; i < 4; i = i + 1) {
        let k = exp2(f32(i));
        let p = textureSample(noise_textures, noise_sampler, uv * k, i32(noise_state[1][i])).r;
        let c = textureSample(noise_textures, noise_sampler, uv * k, i32(noise_state[2][i])).r;
        h = h + water.noise_levels[i] * mix(p, c, noise_state[0][i]);
    }
    return h;
}

// noisetexturegen.ssh convertPS: the height gradient over one texel (1/size), ×8,
// capped at 1; basePS reads the [0,1] normal back as xy·2−1, i.e. the capped value.
fn convert_b(h0: f32, hx: f32, hy: f32) -> vec2<f32> {
    return vec2(min((h0 - hx) * 8.0, 1.0), min((h0 - hy) * 8.0, 1.0));
}

// The engine composites + converts into a size² render target which the water then
// bilinearly samples. Reproduce that exactly: evaluate the (periodic) composite on the
// target's texel grid over a 3×3 neighbourhood, convert the four surrounding texels and
// bilinearly blend. This band-limits the bump to the target resolution — taking the
// gradient of the raw composite per pixel aliases into high-frequency chop the engine
// never shows. The scrolling water uv moves over this fixed grid, as in the engine.
fn noise_bump(uv: vec2<f32>) -> vec2<f32> {
    let size = 1.0 / water.scroll_time.z;
    let t = uv * size - 0.5;
    let cell = floor(t);
    let f = t - cell;
    var h: array<f32, 9>;
    for (var j = 0; j < 3; j = j + 1) {
        for (var i = 0; i < 3; i = i + 1) {
            h[j * 3 + i] = noise_height((cell + vec2(f32(i), f32(j)) + 0.5) / size);
        }
    }
    let b00 = convert_b(h[0], h[1], h[3]);
    let b10 = convert_b(h[1], h[2], h[4]);
    let b01 = convert_b(h[3], h[4], h[6]);
    let b11 = convert_b(h[4], h[5], h[7]);
    return mix(mix(b00, b10, f.x), mix(b01, b11, f.x), f.y);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.world_position.xyz;
    var dir = water.tex_scale_dir.zw;
    var uv = p.xz * water.tex_scale_dir.xy + dir * water.scroll_time.x * globals.time;
    if water.scroll_time.y > 0.5 {
        dir = in.uv_b;
        uv = in.uv + vec2(0.0, water.scroll_time.x * globals.time);
    }
    // normalMap = 0: basePS samples the raw height's .xy·2−1 (both channels equal).
    var b: vec2<f32>;
    if water.scroll_time.w > 0.5 {
        b = noise_bump(uv);
    } else {
        b = vec2(noise_height(uv) * 2.0 - 1.0);
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
    // reflectPS (waterbasic.ssh): the planar scene reflection (render/reflection.rs)
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
    // Gamma-space output like every Grim Dawn material (render/mod.rs); the
    // glow view gets nothing (water has no glow).
    if view.exposure > 0.1 {
        return vec4(0.0, 0.0, 0.0, 0.0);
    }
    return vec4(saturate(rgb), clamp(alpha * in.color.a, 0.0, 1.0));
}