// Port of lavabasic.ssh basePS (DX11 profile) for the
// water types whose shader is lavabasic (water.rs WaterKey::lava swaps it in for
// water.wesl; same bind group, plus the type's lavaTexture / lavaNormal / ashTexture /
// ashNormal at 9-16). Its waterBlockVS / waterPathVS pass the uv without the scroll
// (TEXCOORD1.xy), the scroll offset apart (.zw: textureDirection · textureScroll · time
// on layers, (0, textureScroll · time) on paths) and the bump frame's tangent (layers:
// the normalised textureDirection, paths: the vertex's flow tangent, uv_b); vertex
// colour r = the depth opacity (water.rs depth_opacity), a = the painted alpha.
// The bump (bumpSampler) is the type's NoiseTexture target (water.rs render_noise), normalMap = 0
// on the stock lava record (lavadefault.dbr): the raw composite height, its r channel.
// No reflection, no sky, no specular, no glow style (the technique list has none):
// the glow view gets nothing, like the water. Gamma-space output.
import bevy_pbr::{
    render::forward_io::VertexOutput,
    render::mesh_view_bindings::{view, globals},
};

struct WaterParams {
    surface: vec4<f32>,        // surfaceParams: x normal-z weight, y melt scale, z (unused here)
    water_color: vec4<f32>,
    tex_scale_dir: vec4<f32>,  // xy textureScale, zw textureDirection
    scroll_time: vec4<f32>,    // x textureScroll, y 1 = a path, z unused, w 1 = normalMap
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> water: WaterParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var noise_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var noise_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var<storage, read> env: array<vec4<f32>>;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var lava_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(10) var lava_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(11) var lava_normal: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(12) var lava_normal_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(13) var ash_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(14) var ash_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(15) var ash_normal: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(16) var ash_normal_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    if view.exposure > 0.1 {
        return vec4(0.0);
    }
    let time = globals.time;
    var uv = in.world_position.xz * water.tex_scale_dir.xy;
    var scroll = water.tex_scale_dir.zw * water.scroll_time.x * time;
    var tangent = normalize(vec3(water.tex_scale_dir.z, 0.0, water.tex_scale_dir.w));
    if water.scroll_time.y > 0.5 {
        uv = in.uv;
        scroll = vec2(0.0, water.scroll_time.x * time);
        tangent = vec3(in.uv_b.x, 0.0, in.uv_b.y);
    }
    let h = textureSample(noise_texture, noise_sampler, uv + time * vec2(0.0025, 0.00125)).r;
    // The melt: noise height × (surfaceParams.y − 10) / 495 × depth opacity, against
    // the ash texture's alpha. Above it the lava shows, within 0.1 the two blend.
    let melt = h * (water.surface.y - 10.0) * in.color.r * 0.0020202;
    let wa = time * 0.5 + h * 30.0;
    let wb = time * 0.125 + h * 30.0;
    let ash_uv = uv + scroll + vec2(sin(wb), cos(wb)) * 0.0006;
    let lava_uv = uv + scroll * 1.2 + vec2(sin(wa), cos(wa)) * 0.003;
    let ash = textureSample(ash_texture, ash_sampler, ash_uv);
    let ash_n = textureSample(ash_normal, ash_normal_sampler, ash_uv).xyz;
    let lava = textureSample(lava_texture, lava_sampler, lava_uv).rgb;
    let lava_n = textureSample(lava_normal, lava_normal_sampler, lava_uv).xyz;
    let k = (melt - ash.a) * 10.0;
    let rim = saturate(ash.a - h) * ash.a * 0.15;
    var albedo = ash.rgb;
    var nmap = ash_n;
    // Red heat on the crust, rising with the melt as it nears the ash alpha.
    let q = melt / max(ash.a, 1e-6);
    var heat = q * q * q + rim;
    // How much the scene light shades the surface (molten lava: nearly none).
    var shade = 1.0;
    if melt > ash.a + 0.1 {
        albedo = lava;
        nmap = lava_n;
        heat = 0.0;
        shade = 0.0;
    } else if melt > ash.a {
        albedo = mix(ash.rgb, lava, k);
        nmap = mix(ash_n, lava_n, k);
        heat = 1.0 - k * k * k + rim;
        shade = 1.0 - k;
    }
    shade = shade + 0.2;
    let b = nmap * 2.0 - 1.0;
    let bitangent = vec3(-tangent.z, 0.0, tangent.x);
    // surfaceNormal: the shader's default (0, 1, 0).
    let n = normalize(b.x * tangent + b.y * bitangent + b.z * water.surface.x * vec3(0.0, 1.0, 0.0));
    let ndl = max(dot(n, env[0].xyz), 0.0);
    // AddSceneLight skips a near-black sun (gd_standard.wesl's env[0].w gate); env[3] = ground ambient.
    let sun = select(vec3(0.0), env[1].rgb, env[0].w > 0.5);
    let light = sun * ndl + max(env[3].rgb, vec3(0.5, 0.4, 0.4));
    let rgb = albedo * (1.0 + shade * (light - 1.0)) + vec3(heat * 0.35, 0.0, 0.0);
    return vec4(saturate(rgb), in.color.a);
}
