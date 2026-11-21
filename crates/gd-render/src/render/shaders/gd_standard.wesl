// Grim Dawn standard surface family, evaluated the way the shipping D3D11
// deferred renderer does it (deferred/deferred.ssh + each shader's
// `deferred` technique), in gamma space:
//   G-buffer: albedo = base·(2·diffuseColor)²  (clamped like a UNORM target)
//             normal = normalize(n.x·T + n.y·(−B) + n.z·N), n = 2·(nrm.rgb − ½)
//             specular = nrm.a·specularColor, power = specularPower
//   ambient     = albedo·(lerp(ground, sky, N·up·½+½) + diffuseColor.w)
//   directional = (sat(N·L)·albedo + sat(V·R)^power·specular)·lightColor·shadow, R = 2(N·L)N − L
//   point       = (sat(N·L̂)·albedo [+ sat(V·R)^power·specular])·lightColor·shadow·max(1 − (d/r)², 0)
//                 (pointPS / pointSpecularPS; the specular one per light: not at lightingQuality low, nor
//                 for a light in simple mode; the y of the light's entry after the lights, render/env.rs;
//                 re-checked 2026-11-11 against the deferred.ssh bytecode: pointSpecularPS's specular is
//                 directionalPS's, power = 2^(10.5·material.y), colour = the specular target, no extra factor;
//                 RenderDeferredPointLight's specularPower / specularIntensity have no shader parameter)
//   light rig   = the same for the character's own lights (render/light_rig.rs, rig_lights)
//   glow        = glow.rgb·glow.a added on top.
// Re-checked 2026-10-30 (directionalPS / directionalAmbientPS / pointPS): plain
// sat(N·L), no wrap or half-Lambert, no shadow intensity; the sun shadow is a
// 4-tap binary compare of bilinear depth at ±½·shadowBluriness (= softness 1.5, World ctor,
// never set, / shadow map size: 1536 at shadowQuality medium, sub_180093410),
// i.e. ±0.75 texel, on the engine's LiSPSM map (render/shadow.rs, sun_shadow).
@if(PREPASS_PIPELINE)
import bevy_pbr::prepass::io::VertexOutput;
@if(PREPASS_PIPELINE)
import bevy_pbr::render::mesh_view_bindings::view;
@if(PREPASS_PIPELINE && PREPASS_FRAGMENT)
import bevy_pbr::prepass::io::FragmentOutput;
@if(!PREPASS_PIPELINE)
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, globals, lights, directional_shadow_textures, clustered_lights},
    mesh_view_types,
};
@if(!PREPASS_PIPELINE && GD_TAGGED)
import bevy_pbr::render::{mesh_functions, clustered_forward};

struct GdParams {
    diffuse: vec4<f32>,
    specular: vec4<f32>,
    scroll: vec4<f32>,
    snow: vec4<f32>,
    flags: vec4<u32>,
    fade: vec4<f32>,
    outline: vec4<f32>,
    // Foliage wind (gd_wind.wesl, the vertex program).
    wind: vec4<f32>,
    ripple_scale: vec4<f32>,
    ripple_amplitude: vec4<f32>,
    // FLAG_GLACIAL: innerColor, topColor, bottomColor + meshHeight, (rimLight, sssParams).
    ice_inner: vec4<f32>,
    ice_top: vec4<f32>,
    ice_bottom: vec4<f32>,
    ice: vec4<f32>,
    // FLAG_DISSOLVE: rgb the dissolveColor of the emissiveStyle edge.
    dissolve: vec4<f32>,
}

const KIND_LIT: u32 = 0u;
const KIND_EMISSIVE: u32 = 1u;
const KIND_MULTIPLY: u32 = 2u;
const FLAG_GLOW: u32 = 1u;
const FLAG_GLOW_SCROLL: u32 = 2u;
const FLAG_DETAIL: u32 = 4u;
const FLAG_SNOW: u32 = 8u;
const FLAG_ALPHA_TEST: u32 = 16u;
const FLAG_HAS_TANGENTS: u32 = 32u;
const FLAG_GLOW_SCROLL_ALPHA: u32 = 64u;
const FLAG_DISSOLVE: u32 = 128u;
// The deferred pass writes material.x = 0 (albedo.ssh, render/material.rs FLAG_UNLIT).
const FLAG_UNLIT: u32 = 256u;
// iceyskinned.ssh deferredPS: albedo = lerp(base, iceyTexture (detail slot), fade.w) (render/material.rs FLAG_ICEY).
const FLAG_ICEY: u32 = 512u;
const FLAG_VERTEX_COLOR: u32 = 1024u;
const FLAG_GLACIAL: u32 = 2048u;
const FLAG_VERTEX_ALPHA: u32 = 4096u;
// bonefadeskinned.ssh (render/material.rs FLAG_BONE_FADE): fade.x holds the bone fade factor.
const FLAG_BONE_FADE: u32 = 8192u;
// standardblended* deferredPS: base.a · blendedGlobalFadeFactor tested against 0.1 (fade.x).
const FLAG_BLENDED_FADE: u32 = 16384u;

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> material: GdParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var base_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var base_smp: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var bump_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var bump_smp: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var glow_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var glow_smp: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var detail_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(8) var detail_smp: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var snow_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(10) var snow_smp: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(11) var snow_nrm_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(12) var snow_nrm_smp: sampler;
// Environment (render/env.rs EnvGpu): [0] sun dir (toward light) + enabled,
// [1] sun colour + shadow intensity (unused, render/env.rs), [2] sky ambient, [3] ground ambient + light count,
// [4] x: the light buffer's clamp (1 = the UNORM back buffer; hdrLighting: the float
// target, no clamp; render/env.rs), y: 1 = point lights with specular, then 2 vec4 per point
// light (pos+radius, colour).
@group(constants::MATERIAL_BIND_GROUP) @binding(13) var<storage, read> env: array<vec4<f32>>;

fn has(f: u32) -> bool {
    return (material.flags.y & f) != 0u;
}

/// The `standarddissolve*` shaders' cut (their DirectX11 `deferredPS` / `lightShadowPS`):
/// d = dissolveTexture.r − dissolveValue + 0.5, drop the texel when base.a · d < 0.5 (FLAG_DISSOLVE,
/// monsters.rs). The `emissiveStyle` edge is added in `fragment` (black for the ambush); their sun
/// `depthMapPS` has no cut.
fn dissolved(uv: vec2<f32>, a: f32) -> bool {
    if !has(FLAG_DISSOLVE) {
        return false;
    }
    let d = textureSample(detail_tex, detail_smp, uv).r - material.fade.z + 0.5;
    return a * d < 0.5;
}

@if(PREPASS_PIPELINE)
fn prepass_discard(in: VertexOutput) {
@if(VERTEX_UVS_A) {
    if material.flags.x != KIND_LIT {
        discard;
    }
    if has(FLAG_ALPHA_TEST) && textureSample(base_tex, base_smp, in.uv).a < material.fade.y {
        discard;
    }
    // The sun's depth map (orthographic) keeps the whole mesh, the other views take the dissolve cut.
    if view.clip_from_view[3][3] != 1.0 && dissolved(in.uv, textureSample(base_tex, base_smp, in.uv).a) {
        discard;
    }
    // A point-light paraboloid view (gd_prepass.wesl): `lightShadowPS` drops
    // a·(0.5 − q.y) < 0.5, q the light→pixel direction (q.y up).
    let p = view.clip_from_view;
    if p[2][3] == -1.0 && p[3][3] == 0.0 && p[1][1] < 0.02 && p[0][0] == p[1][1] {
        let a = select(1.0, textureSample(base_tex, base_smp, in.uv).a, has(FLAG_ALPHA_TEST));
        let qy = normalize(in.world_position.xyz - view.world_position).y;
        if a * (0.5 - qy) < 0.5 {
            discard;
        }
    }
}
}

@if(PREPASS_PIPELINE && PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    prepass_discard(in);
    var out: FragmentOutput;
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION)
    out.frag_depth = in.unclipped_depth;
    return out;
}

@if(PREPASS_PIPELINE && !PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) {
    prepass_discard(in);
}

/// The glow camera (post.rs) is marked by its exposure (ev100 0 → 0.83;
/// the main camera keeps Bevy's default ≈ 0.001, unused by these shaders).
@if(!PREPASS_PIPELINE)
fn is_glow_view() -> bool {
    return view.exposure > 0.1;
}

/// The water reflection camera (render/reflection.rs) is marked by its exposure
/// (ev100 −2 → 3.33); it also exceeds the glow threshold, so test it first.
@if(!PREPASS_PIPELINE)
fn is_reflection_view() -> bool {
    return view.exposure > 2.0;
}

/// A paper-doll view (doll.rs): its `UISceneView` light rig rides in the
/// camera's colour grading (exposure 1 marks it).
@if(!PREPASS_PIPELINE)
fn is_doll_view() -> bool {
    return view.color_grading.exposure > 0.5;
}

/// The doll view's lighting (Grim Dawn.exe sub_140239A40): sky / ground
/// ambient and one directional light from the record (gain = light colour,
/// lift = sky, gamma = ground, contrast = direction to the light in view
/// space: it turns with the camera); no shadows, no point lights.
@if(!PREPASS_PIPELINE)
fn shade_doll(p: vec4<f32>, N: vec3<f32>, albedo: vec3<f32>, ambient_add: f32, spec: vec3<f32>, power: f32) -> vec3<f32> {
    let g = view.color_grading;
    let hemi = mix(g.gamma, g.lift, dot(N, vec3<f32>(0.0, 1.0, 0.0)) * 0.5 + 0.5);
    var c = min(albedo * (hemi + ambient_add), vec3<f32>(1.0));
    let V = normalize(view.world_position - p.xyz);
    let L = normalize((view.world_from_view * vec4<f32>(g.contrast, 0.0)).xyz);
    let d = dot(N, L);
    let R = 2.0 * d * N - L;
    let s = exp2(log2(max(saturate(dot(V, R)), 1e-8)) * power);
    c += min((saturate(d) * albedo + s * spec) * g.gain, vec3<f32>(1.0));
    return c;
}

/// Glow-view alpha, the outline mask (post.rs): opaque passes leave 0 (the
/// masks of the outlined parts come after them, `outline.w`). Blended passes
/// keep the target's alpha (their pipeline's alpha blend is dst-only,
/// material.rs) and return their colour blend factor.
@if(!PREPASS_PIPELINE)
fn glow_view_alpha(blend_alpha: f32) -> f32 {
@if(GD_BLENDED) {
    return blend_alpha;
}
@if(!GD_BLENDED) {
    return 0.0;
}
}

/// The sun shadow the way deferred.ssh `directionalPS` tests it
/// (render/shadow.rs, which builds the engine's LiSPSM map): 4 bilinear taps at
/// ±½·shadowBluriness in a plus, ¼ each, lit unless the stored depth is
/// nearer than the receiver's + shadowBias (−0.001). The same function is in
/// gd_terrain.wesl / gd_standard.wesl / decal.wesl.
@if(!PREPASS_PIPELINE)
const SHADOW_BIAS: f32 = -0.001;

@if(!PREPASS_PIPELINE)
/// The stored depth at `uv` filtered like the engine's shadowTexture sampler:
/// bilinear (deferred.ssh `directional` pass, sampler 5 filter value 5 = not
/// point; `Direct3DDevice11::UpdateState` builds MIN_MAG_MIP_LINEAR), clamped,
/// a plain `sample` of the depth view, not a comparison. Depth textures are
/// not filterable here, so the four texels are blended by hand.
fn shadow_depth_bilinear(uv: vec2<f32>, size: vec2<i32>, layer: i32) -> f32 {
    let p = uv * vec2<f32>(size) - vec2<f32>(0.5);
    let b = floor(p);
    let f = p - b;
    let i0 = vec2<i32>(b);
    let hi = size - vec2<i32>(1);
    let t00 = textureLoad(directional_shadow_textures, clamp(i0, vec2<i32>(0), hi), layer, 0);
    let t10 = textureLoad(directional_shadow_textures, clamp(i0 + vec2<i32>(1, 0), vec2<i32>(0), hi), layer, 0);
    let t01 = textureLoad(directional_shadow_textures, clamp(i0 + vec2<i32>(0, 1), vec2<i32>(0), hi), layer, 0);
    let t11 = textureLoad(directional_shadow_textures, clamp(i0 + vec2<i32>(1, 1), vec2<i32>(0), hi), layer, 0);
    return mix(mix(t00, t10, f.x), mix(t01, t11, f.x), f.y);
}

@if(!PREPASS_PIPELINE)
fn sun_shadow(p: vec4<f32>) -> f32 {
    if lights.n_directional_lights == 0u {
        return 1.0;
    }
    let light = &lights.directional_lights[0];
    if ((*light).flags & mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) == 0u {
        return 1.0;
    }
    let c = (*light).cascades[0].clip_from_world * vec4<f32>(p.xyz, 1.0);
    if c.w <= 0.0 {
        return 1.0;
    }
    let ndc = c.xyz / c.w;
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    // Outside the map is lit (gd_terrain.wesl).
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return 1.0;
    }
    // Receiver depth + shadowBias in the engine's depth (0 = sun side), back
    // to Bevy's reverse Z.
    let z = 1.0 - saturate(1.0 - ndc.z + SHADOW_BIAS);
    let size = vec2<i32>(textureDimensions(directional_shadow_textures));
    let layer = i32((*light).depth_texture_base_index);
    // texel_size carries ½·shadowBluriness in map UV (render/shadow.rs).
    let o = (*light).cascades[0].texel_size;
    let taps = array<vec2<f32>, 4>(vec2<f32>(-o, 0.0), vec2<f32>(o, 0.0), vec2<f32>(0.0, -o), vec2<f32>(0.0, o));
    var lit = 0.0;
    for (var i = 0; i < 4; i++) {
        // Engine: shadowed when stored depth < receiver depth (reverse Z: >).
        if shadow_depth_bilinear(uv + taps[i], size, layer) <= z {
            lit += 0.25;
        }
    }
    // No shadow intensity: AmbianceManager's (env[1].w) only feeds its
    // statistics text (render/shadow.rs).
    return lit;
}

/// The point-light shadow of deferred.ssh `pointPS` (render/point_shadow.rs):
/// the engine's paraboloid uv of the light→pixel direction (light frame rows
/// (−1,0,0), (0,1,0), (0,0,−1): uv = (0.5 + 0.5 s, 0.5 − 0.5 t), s, t =
/// (q.x, q.z) / (q.y − 1)), 4 taps at ±½·shadowBluriness in a plus, ¼ each, lit
/// unless the bilinear stored depth < sat((d − 0.1) / 29.9 + shadowBias), bias
/// −0.0003. The map (W = 1.5 / bluriness texels) is the top-left of the light's
/// spot-shadow layer of the directional shadow array (gd_prepass.wesl).
@if(!PREPASS_PIPELINE)
fn point_shadow(lp: vec3<f32>, p: vec3<f32>, blur: f32) -> f32 {
    if blur <= 0.0 {
        return 1.0;
    }
    // The light's spot view: Bevy lists the spot lights after the point
    // lights (index = cascades − spot_light_shadowmap_offset), shadowed first.
    var cascades = 0u;
    for (var i = 0u; i < lights.n_directional_lights; i++) {
        if (lights.directional_lights[i].flags & mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
            cascades += lights.directional_lights[i].num_cascades;
        }
    }
    let j0 = i32(cascades) - lights.spot_light_shadowmap_offset;
    let n = min(i32(arrayLength(&clustered_lights.data)), j0 + 16);
    var layer = -1;
    for (var j = max(j0, 0); j < n; j++) {
        let l = &clustered_lights.data[j];
        let want = mesh_view_types::POINT_LIGHT_FLAGS_SPOT_LIGHT_BIT | mesh_view_types::POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT;
        if ((*l).flags & want) != want {
            break;
        }
        if all(abs((*l).position_radius.xyz - lp) < vec3<f32>(1e-3)) {
            layer = j + lights.spot_light_shadowmap_offset;
            break;
        }
    }
    if layer < 0 {
        return 1.0;
    }
    let v = p - lp;
    let dist = length(v);
    let q = vec3<f32>(-v.x, v.y, -v.z) / max(dist, 1e-6);
    // Above the light's horizon: the map's cleared border, lit.
    if q.y >= 0.0 {
        return 1.0;
    }
    let w = round(1.5 / blur);
    let st = vec2<f32>(q.x, q.z) / min(q.y - 1.0, -1e-3);
    let uv = vec2<f32>(0.5) + vec2<f32>(0.5, -0.5) * st;
    let r = saturate((dist - 0.1) * 0.0334448 - 0.0003);
    let o = 0.5 * blur;
    let taps = array<vec2<f32>, 4>(vec2<f32>(-o, 0.0), vec2<f32>(o, 0.0), vec2<f32>(0.0, -o), vec2<f32>(0.0, o));
    let hi = vec2<i32>(i32(w) - 1);
    var lit = 0.0;
    for (var i = 0; i < 4; i++) {
        // Bilinear depth of the clamped W map (texels at uv·W − ½).
        let t = (uv + taps[i]) * w - vec2<f32>(0.5);
        let b = floor(t);
        let f = t - b;
        let i0 = vec2<i32>(b);
        let t00 = textureLoad(directional_shadow_textures, clamp(i0, vec2<i32>(0), hi), layer, 0);
        let t10 = textureLoad(directional_shadow_textures, clamp(i0 + vec2<i32>(1, 0), vec2<i32>(0), hi), layer, 0);
        let t01 = textureLoad(directional_shadow_textures, clamp(i0 + vec2<i32>(0, 1), vec2<i32>(0), hi), layer, 0);
        let t11 = textureLoad(directional_shadow_textures, clamp(i0 + vec2<i32>(1, 1), vec2<i32>(0), hi), layer, 0);
        // Bevy's reverse Z holds 1 − the engine's depth (cleared 0 = far).
        let s2 = 1.0 - mix(mix(t00, t10, f.x), mix(t01, t11, f.x), f.y);
        if s2 >= r {
            lit += 0.25;
        }
    }
    return lit;
}

/// `sat(V·R)^power`, R = 2(N·L)N − L (directionalPS / pointSpecularPS).
@if(!PREPASS_PIPELINE)
fn spec_term(N: vec3<f32>, L: vec3<f32>, V: vec3<f32>, power: f32) -> f32 {
    let R = 2.0 * dot(N, L) * N - L;
    return exp2(log2(max(saturate(dot(V, R)), 1e-8)) * power);
}

/// The character's light rig (render/light_rig.rs): the lights of the pixel's cluster whose `radius`
/// carries the mesh tag's light group (+ 0.5: specular), drawn like `pointPS` / `pointSpecularPS`
/// (the engine's `pointGrouped` stencil test).
@if(!PREPASS_PIPELINE && GD_TAGGED)
fn rig_lights(p: vec4<f32>, N: vec3<f32>, V: vec3<f32>, albedo: vec3<f32>, spec: vec3<f32>, power: f32,
              frag: vec2<f32>, group: u32) -> vec3<f32> {
    var c = vec3<f32>(0.0);
    let vz = view.view_from_world;
    let view_z = dot(vec4<f32>(vz[0].z, vz[1].z, vz[2].z, vz[3].z), vec4<f32>(p.xyz, 1.0));
    let ortho = view.clip_from_view[3].w == 1.0;
    let ci = clustered_forward::view_fragment_cluster_index(frag, view_z, ortho);
    let r = clustered_forward::unpack_clusterable_object_index_ranges(ci);
    for (var i = r.first_point_light_index_offset; i < r.first_spot_light_index_offset; i++) {
        let l = &clustered_lights.data[clustered_forward::get_clusterable_object_id(i)];
        let w = (*l).position_radius.w;
        if w < 1.0 || u32(w) != group {
            continue;
        }
        let lv = (*l).position_radius.xyz - p.xyz;
        let dist = length(lv);
        let a = max(1.0 - (dist / (*l).range) * (dist / (*l).range), 0.0);
        if a <= 0.0 {
            continue;
        }
        let L = lv / max(dist, 1e-6);
        var t = saturate(dot(N, L)) * albedo;
        if fract(w) > 0.25 {
            t += spec_term(N, L, V, power) * spec;
        }
        c += min(t * (*l).color_inverse_square_range.rgb * a, vec3<f32>(env[4].x));
    }
    return c;
}

/// The deferred ambient and sun passes for one G-buffer texel (the point
/// lights: [`shade_points`]).
@if(!PREPASS_PIPELINE)
fn shade(p: vec4<f32>, geom_n: vec3<f32>, N: vec3<f32>, albedo: vec3<f32>, ambient_add: f32,
         spec: vec3<f32>, power: f32) -> vec3<f32> {
    if is_doll_view() {
        if has(FLAG_UNLIT) {
            return albedo;
        }
        return shade_doll(p, N, albedo, ambient_add, spec, power);
    }
    let hemi = mix(env[3].rgb, env[2].rgb, dot(N, vec3<f32>(0.0, 1.0, 0.0)) * 0.5 + 0.5);
    var c = min(albedo * (hemi + ambient_add), vec3<f32>(env[4].x));
    if has(FLAG_UNLIT) {
        // material.x = 0 (FLAG_UNLIT): ambientPS / directionalAmbientPS write the albedo as is.
        c = min(albedo, vec3<f32>(env[4].x));
    }
    let V = normalize(view.world_position - p.xyz);
    if env[0].w > 0.5 && !has(FLAG_UNLIT) {
        let L = env[0].xyz;
        let d = dot(N, L);
        let R = 2.0 * d * N - L;
        let s = exp2(log2(max(saturate(dot(V, R)), 1e-8)) * power);
        c += min((saturate(d) * albedo + s * spec) * env[1].rgb * sun_shadow(p), vec3<f32>(env[4].x));
    }
    return c;
}

/// The deferred point-light passes for one G-buffer texel (none in the doll view).
@if(!PREPASS_PIPELINE)
fn shade_points(p: vec4<f32>, N: vec3<f32>, albedo: vec3<f32>, spec: vec3<f32>, power: f32,
                frag: vec2<f32>, group: u32) -> vec3<f32> {
    var c = vec3<f32>(0.0);
    if is_doll_view() {
        return c;
    }
    let V = normalize(view.world_position - p.xyz);
    let n_lights = u32(env[3].w);
    for (var i = 0u; i < n_lights; i++) {
        let lp = env[5u + 2u * i];
        // FireLights step their colour on the CPU (world::Flicker).
        let lc = env[6u + 2u * i];
        let lv = lp.xyz - p.xyz;
        let dist = length(lv);
        let a = max(1.0 - (dist / lp.w) * (dist / lp.w), 0.0);
        if a > 0.0 {
            let extra = env[5u + 2u * n_lights + i];
            let sh = point_shadow(lp.xyz, p.xyz, extra.x);
            let L = lv / dist;
            var t = saturate(dot(N, L)) * albedo;
            // pointSpecularPS: per light (lightingQuality, simple mode; render/env.rs).
            if extra.y > 0.5 {
                t += spec_term(N, L, V, power) * spec;
            }
            c += min(t * lc.rgb * a * sh, vec3<f32>(env[4].x));
        }
    }
@if(GD_TAGGED) {
    if group != 0u {
        c += rig_lights(p, N, V, albedo, spec, power, frag, group);
    }
}
    return c;
}

@if(!PREPASS_PIPELINE)
@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var out: FragmentOutput;
    var uv = vec2<f32>(0.0);
@if(VERTEX_UVS_A)
    uv = in.uv;
    let t = globals.time;

    if material.flags.x != KIND_LIT {
        // additive/multiply scroll FX (emissiveStyle): colour and alpha scroll separately.
        let c = textureSample(base_tex, base_smp, uv + material.scroll.xy * t);
        let a = textureSample(base_tex, base_smp, uv + material.scroll.zw * t).a;
        if material.flags.x == KIND_MULTIPLY {
            out.color = vec4<f32>(c.rgb * a, 1.0);
        } else {
            out.color = vec4<f32>(c.rgb * (material.diffuse.rgb + material.diffuse.w), a);
        }
        if is_glow_view() {
            out.color = vec4<f32>(0.0, 0.0, 0.0, glow_view_alpha(out.color.a));
        }
        return out;
    }

    var base = textureSample(base_tex, base_smp, uv);
    let texel_a = base.a;
    // FLAG_VERTEX_COLOR (material.rs): the `*vertexcolor*` / `*vertcolor*` deferredPS take base.a · vcol.a and tint
    // the albedo by vcol.rgb, after the snow blend (snowvertcolor); FLAG_VERTEX_ALPHA: base.a · vcol.r.
    var vcol = vec3<f32>(1.0);
@if(VERTEX_COLORS) {
    if has(FLAG_VERTEX_COLOR) {
        base.a *= in.color.a;
        vcol = in.color.rgb;
    }
    if has(FLAG_VERTEX_ALPHA) {
        base.a *= in.color.r;
    }
}
    if material.outline.w > 0.5 {
        // `deferredOutlineStyle`'s outlinePS: alpha test >= 0.01, flat.
        if base.a < 0.01 {
            discard;
        }
        // alpha: the outline's palette class (post.rs).
        out.color = vec4<f32>(0.0, 0.0, 0.0, material.outline.w);
        return out;
    }
    // fade.y: the alpha-test reference (0.5; grass 0.65 / 0.35, render/grass.rs).
    if has(FLAG_ALPHA_TEST) && base.a < material.fade.y {
        discard;
    }
    // fade.x: a faded scenery part's opacity o (see_through.rs): the "…T.ssh"
    // `depthStyle` pass writes the depth of the texels with base alpha ≥ 0.5,
    // the lit pass draws on that depth (EQUAL) at alpha o, not at all at o = 0.
    let fade = material.fade.x;
@if(GD_FADE_DEPTH) {
    if texel_a < 0.5 {
        discard;
    }
    return out;
}
@if(GD_FADE_COLOUR) {
    if texel_a < 0.5 || fade <= 0.0 {
        discard;
    }
}
    // bonefadeskinned deferredPS (FLAG_BONE_FADE): alpha = base.a · boneFadeFactor (fade.x),
    // dropped below 0.01. standardblended* / *vertexalpha*: base.a · blendedGlobalFadeFactor
    // (fade.x) dropped below 0.1.
    if has(FLAG_BONE_FADE) {
        if base.a * fade < 0.01 {
            discard;
        }
    } else if has(FLAG_BLENDED_FADE) || has(FLAG_VERTEX_ALPHA) {
        if base.a * fade < 0.1 {
            discard;
        }
    }
    // The T lit pass's alpha is o alone (lightDiffuseAmbientPS: o0.w = transparencyValue.z).
    var alpha = base.a * fade;
@if(GD_FADE_COLOUR) {
    alpha = fade;
}
    if dissolved(uv, base.a) {
        discard;
    }
    if has(FLAG_ICEY) {
        base = vec4<f32>(mix(base.rgb, textureSample(detail_tex, detail_smp, uv).rgb, material.fade.w), base.a);
    }
    var uv_b = uv;
@if(VERTEX_UVS_B)
    uv_b = in.uv_b;
    if has(FLAG_DETAIL) {
        base = vec4<f32>(min(base.rgb * textureSample(detail_tex, detail_smp, uv_b).rgb * 2.0, vec3<f32>(1.0)), base.a);
    }
    var nrm = textureSample(bump_tex, bump_smp, uv);
    var power = material.specular.w;

    // Tangent frame (T, −B, N), B = sign·(N×T) as stored by model.rs.
    var Nv = normalize(in.world_normal);
    var T = vec3<f32>(1.0, 0.0, 0.0);
    var Bn = vec3<f32>(0.0, 0.0, 1.0);
@if(VERTEX_TANGENTS) {
    if has(FLAG_HAS_TANGENTS) {
        T = in.world_tangent.xyz;
        Bn = -in.world_tangent.w * cross(Nv, T);
    }
}

    // The vertex's snow factor (standardstaticglacialsnow's final blend, FLAG_GLACIAL).
    var snow_s = 0.0;
    if has(FLAG_SNOW) {
        // standard*snow (standardstaticsnow deferredVS / deferredPS):
        // s = sat((N.y − snowThreshold) / (1 − snowThreshold)) of the vertex's
        // world normal, snow UV = world xz · 0.1. The vertex stage lifts the
        // vertex by s · snowDepth (world y; gd_forward.wesl / gd_prepass.wesl).
        // PORT: s is taken per pixel here, not interpolated from the vertex.
        let thr = material.snow.y;
        let s = saturate((Nv.y - thr) / (1.0 - thr));
        snow_s = s;
        let k = min(pow(max(s, 1e-6), 0.4), 1.0);
        let suv = in.world_position.xz * 0.1;
        let sb = textureSample(snow_tex, snow_smp, suv);
        let a6 = sb.a * sb.a * sb.a * sb.a * sb.a * sb.a;
        var cov = min(k * min(a6 * 6.0, 1.0) + k * 0.25, 1.0);
        cov = min(max(cov - 0.1, 0.0) * 8.0, 1.0);
        let base_nrm = nrm;
        nrm = mix(base_nrm, textureSample(snow_nrm_tex, snow_nrm_smp, suv), cov * material.snow.z);
        // view vector in the (T, −B, N) frame
        let V = normalize(view.world_position - in.world_position.xyz);
        let vts = normalize(vec3<f32>(dot(V, T), dot(V, Bn), dot(V, Nv)));
        let d = max(dot(base_nrm.xyz, vts), 0.0);
        let r4 = material.specular.rgb * (nrm.a * 0.5) + vec3<f32>(0.02);
        let rz = min(d * d * 0.25, 1.0);
        let r5 = (1.0 - d) * (1.0 - d) * vec3<f32>(2.125, 2.25, 2.5);
        let ry = max((1.0 - pow(max(d, 1e-6), 1.15)) * 0.35, 0.0);
        let r6 = sb.rgb + (vec3<f32>(1.0) - sb.rgb) * 0.1;
        let r8 = r6 * 0.7;
        let r7 = ry * (vec3<f32>(0.52, 0.81, 0.95) - r6 * 0.7) + r8;
        var sc = rz * (vec3<f32>(0.74, 0.9, 1.0) - r7) + r7;
        sc = r5 * r4 + sc;
        sc = (sc - sb.rgb) * 0.6 + sb.rgb;
        // The glacial snow skips this fresnel step (standardstaticglacialsnow deferredPS).
        if !has(FLAG_GLACIAL) {
            sc = sc - r6 * 0.7;
            sc = material.snow.w * sc + r8;
        }
        base = vec4<f32>(mix(base.rgb, sc, cov), base.a);
        power = mix(power, 20.0, cov);
    }

    let n = 2.0 * (nrm.rgb - vec3<f32>(0.5));
    var N = normalize(n.x * T + n.y * Bn + n.z * Nv);
    if !has(FLAG_HAS_TANGENTS) {
        N = Nv;
    }
    let dc2 = material.diffuse.rgb * 2.0;
    var spec = nrm.a * material.specular.rgb;
    base = vec4<f32>(base.rgb * vcol, base.a);
    var albedo = min(base.rgb * dc2 * dc2, vec3<f32>(1.0));
    if has(FLAG_GLACIAL) {
        // standard*glacial deferredVS / deferredPS (render/material.rs FLAG_GLACIAL).
        var g = vec3<f32>(1.0);
@if(GD_TAGGED) {
        let h = in.world_position.y - mesh_functions::get_world_from_local(in.instance_index)[3].y;
        let mh = material.ice_bottom.w;
        // D3D div_sat: ±inf saturates, 0 / 0 (NaN) gives 0.
        var t = select(0.0, 1.0, h > 0.0);
        if mh != 0.0 {
            t = saturate(h / mh);
        }
        g = mix(material.ice_bottom.rgb, vec3<f32>(1.0), t);
}
        let V = normalize(view.world_position - in.world_position.xyz);
        let v = normalize(vec3<f32>(dot(V, T), dot(V, Bn), dot(V, Nv)));
        let d = max(dot(n, v), 0.0);
        let k = 1.0 - min(exp2(log2(max(1.0 - d, 1e-8)) * material.ice.x), 1.0);
        let fr = material.snow.w;
        let top = material.ice_top.rgb;
        let lit = base.rgb * dc2 * dc2;
        let a = lit * g;
        let b = mix(a, mix(top, a, k), k * fr);
        let ia = 1.0 - base.a;
        let dd = ia * mix(mix(top, material.ice_inner.rgb, ia), a, 1.0 - d * d);
        let m = textureSample(detail_tex, detail_smp, uv + vec2<f32>(v.x * material.ice.y, 0.0)).a;
        var ice = mix(b, mix(dd * m, b, base.a), d * d * fr);
        if has(FLAG_SNOW) {
            ice = mix(ice, lit, snow_s);
        }
        albedo = clamp(ice, vec3<f32>(0.0), vec3<f32>(1.0));
        spec += min(fr, 0.25) * k * top;
    }
    if has(FLAG_UNLIT) {
        // albedo.ssh deferredPS: albedo = the texel, normal target 0 (decoded −1, −1, −1, unnormalized,
        // which the point lights use), specular 0.
        albedo = base.rgb;
        N = vec3<f32>(-1.0);
        spec = vec3<f32>(0.0);
    }
    // reflectionStyle (render/reflection.rs): base × (sat(N·L)·sun + hemisphere
    // ambient), alpha base.a; no point lights, shadows, specular or glow.
    if is_reflection_view() {
        let hemi = mix(env[3].rgb, env[2].rgb, N.y * 0.5 + 0.5);
        var lit = hemi;
        if env[0].w > 0.5 {
            lit += saturate(dot(N, env[0].xyz)) * env[1].rgb;
        }
        out.color = vec4<f32>(min(albedo * lit, vec3<f32>(env[4].x)), alpha);
        return out;
    }

    // A character's mesh tag (render/light_rig.rs): its light group and ambient highlight (diffuseColor.w).
    var group = 0u;
    var highlight = 0.0;
@if(GD_TAGGED) {
    let tag = mesh_functions::get_tag(in.instance_index);
    group = tag & 0xFFFFu;
    highlight = f32((tag >> 16u) & 0x7FFFu) * 0.001;
    // The highlight flag (bit 31, `light_rig::mesh_tag`): + 0.25; the portrait clears it before drawing (ui/dialog.rs).
    if (tag >> 31u) != 0u && !is_doll_view() {
        highlight += 0.25;
    }
}
    var c = shade(in.world_position, Nv, N, albedo, material.diffuse.w + highlight, spec, power);
    let points = shade_points(in.world_position, N, albedo, spec, power, in.position.xy, group);
@if(GD_FADE_COLOUR) {
    // The T point-light passes add at full strength (ONE / BLEND_FACTOR) and
    // only while o ≥ 0.5 (they discard below); this pass is scaled by o.
    if fade >= 0.5 {
        c += points / fade;
    }
}
@if(!GD_FADE_COLOUR) {
    c += points;
}
    var glow = vec3<f32>(0.0);
    if has(FLAG_GLOW) {
        var guv = uv;
        if has(FLAG_GLOW_SCROLL) {
            guv = uv + fract(material.scroll.xy * t);
        }
        let g = textureSample(glow_tex, glow_smp, guv);
        var ga = textureSample(glow_tex, glow_smp, uv).a;
        if has(FLAG_GLOW_SCROLL_ALPHA) {
            ga = g.a;
        }
        glow = g.rgb * ga;
    }
    // The `standarddissolve*` `emissiveStyle` pass (its emissiveVS / emissivePS, SrcAlpha / One,
    // monsters.rs): where the texel is not cut yet (n > v), (0.25 − sat(n − v)) · dissolveColor · 4 · the
    // dissolve texture at uv·2 + (cos, sin)(time / 2) — a glow at the edge, a darkening further in.
    if has(FLAG_DISSOLVE) {
        let x = saturate(textureSample(detail_tex, detail_smp, uv).r - material.fade.z);
        let n2 = textureSample(detail_tex, detail_smp, uv * 2.0 + vec2<f32>(cos(t * 0.5), sin(t * 0.5))).rgb;
        c = max(c + select(vec3<f32>(0.0), (0.25 - x) * material.dissolve.rgb * n2 * 4.0, x > 0.0), vec3<f32>(0.0));
    }
    // The glow view renders only the glow buffer for the blur (post.rs).
    if is_glow_view() {
        // The engine's glow target is the G-buffer's UNORM format 2 (CreateRenderTargets @0x1800ad850): at most 1.
        out.color = vec4<f32>(clamp(glow, vec3<f32>(0.0), vec3<f32>(1.0)), glow_view_alpha(fade));
    } else {
        out.color = vec4<f32>(min(c + glow, vec3<f32>(env[4].x)), alpha);
    }
    return out;
}
