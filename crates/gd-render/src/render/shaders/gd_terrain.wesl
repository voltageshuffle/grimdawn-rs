// TerrainRT, all layers in one pass: replicates the per-layer deferred passes
// of shaders/terrain/{terrain,snow}highquality.ssh (top layer first, blend
// src·dstAlpha + dst, dstAlpha *= a) or, at terrain quality 3,
// {terrain,snow}ultraquality.ssh (bottom first, SrcAlpha / InvSrcAlpha, the
// coverage times `ultra_soft`; render/terrain.rs) and then the shared deferred
// lighting (see gd_standard.wesl). Values are gamma-space (render/mod.rs).
// Patch mode (grid.w = 1): a TerrainDecoration's painted layers over its mesh,
// terrainpatch.ssh `deferred`, alpha-blended (render/terrain.rs).
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, lights, directional_shadow_textures, clustered_lights},
    mesh_view_types,
};

struct TerrainParams {
    origin: vec4<f32>,
    grid: vec4<f32>,
    layers: array<vec4<f32>, 16>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> terrain: TerrainParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var colors: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var colors_smp: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var normals: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var normals_smp: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var opacity: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var opacity_smp: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var<storage, read> env: array<vec4<f32>>;
// Quality 3's depthTexture: the swap-chain surface's depth, no scene in it (render/terrain.rs).
@group(constants::MATERIAL_BIND_GROUP) @binding(8) var scene_depth: texture_depth_2d;

const AMBIENT_INTENSITY: f32 = 0.65;
const SPECULAR_INTENSITY: f32 = 1.56;

/// The water reflection camera (render/reflection.rs): exposure ev100 −2 → 3.33.
fn is_reflection_view() -> bool {
    return view.exposure > 2.0;
}

/// The sun shadow the way deferred.ssh `directionalPS` tests it
/// (render/shadow.rs, which builds the engine's LiSPSM map): 4 bilinear taps at
/// ±½·shadowBluriness in a plus, ¼ each, lit unless the stored depth is
/// nearer than the receiver's + shadowBias (−0.001). The same function is in
/// gd_terrain.wesl / gd_standard.wesl / decal.wesl.
const SHADOW_BIAS: f32 = -0.001;

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
    // Outside the map (receivers past the fitted body: beyond the 80 m cut or the scene box) is lit; the clamped
    // edge texels would stretch the shadows on the map's border away from the camera.
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

/// View-space distance of an NDC depth (Bevy reverse Z): the engine's
/// n·f / (f − d(f − n)) of its own depth.
fn linear_depth(ndc: f32) -> f32 {
    let p = view.view_from_clip * vec4<f32>(0.0, 0.0, ndc, 1.0);
    return -p.z / p.w;
}

/// terrainultraquality.ssh `deferredPS` (value 38, per its disassembly; the
/// snow / parallax variants alike): the factor on every layer's coverage. With
/// the depthTexture's surface within 0.35 m of the fragment, k = |dz| / 0.35
/// and soft = k + (1 − k)·sat((max(face·N, 0) − 0.9)·10), the face normal from
/// coarse derivatives of the position rebuilt from that depth; else 1.
/// Called in uniform control flow (derivatives).
fn ultra_soft(frag: vec4<f32>, n0: vec3<f32>) -> f32 {
    let dims = vec2<i32>(textureDimensions(scene_depth));
    let px = clamp(vec2<i32>(frag.xy), vec2<i32>(0), dims - vec2<i32>(1));
    let d = textureLoad(scene_depth, px, 0);
    let uv = (frag.xy - view.viewport.xy) / view.viewport.zw;
    let c = view.world_from_clip * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, d, 1.0);
    let p = c.xyz / c.w;
    var f = cross(dpdxCoarse(p), dpdyCoarse(p));
    // PORT: turned to the camera (render/terrain.rs).
    if dot(f, view.world_position - p) < 0.0 {
        f = -f;
    }
    let agree = saturate((max(dot(normalize(f), n0), 0.0) - 0.9) * 10.0);
    let dz = abs(linear_depth(d) - linear_depth(frag.z));
    let k = dz * 2.857143;
    // A view without the copy (another size, e.g. the env map) or the sky
    // (depth 0): nothing within 0.35 m.
    let same = all(dims == vec2<i32>(view.viewport.zw));
    if same && d > 0.0 && dz <= 0.35 {
        return agree * (1.0 - k) + k;
    }
    return 1.0;
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    var out: FragmentOutput;
    let local = in.world_position.xyz - terrain.origin.xyz;
    let wpos = in.world_position.xyz;
    let tuv = wpos.xz * 0.1;
    // explicit gradients: the layer loop is non-uniform control flow
    let gx = dpdx(tuv);
    let gy = dpdy(tuv);
    // Weights are stored like the vertex layer mask (row z, column x): not
    // flipped (a flip made layer presence and weight disagree → black cells).
    // A terrain patch (render/terrain.rs): origin = its texel grid's corner, grid = its size, so the same
    // formula is terrainpatch.ssh's (x − centre) / width + ½.
    let is_patch = terrain.grid.w > 0.5;
    let ouv = vec2<f32>(local.x / terrain.grid.x, local.z / terrain.grid.y);
    let cell = vec2<i32>(clamp(vec2<i32>(floor(local.xz)), vec2<i32>(0), vec2<i32>(terrain.grid.xy) - 1));

    // vertex normal and the terrain tangent frame (deferred VS of terrain*.ssh)
    let n0 = in.world_normal;
    var Tt = vec3<f32>(n0.y, -n0.x, 0.0);
    var Bt = vec3<f32>(0.0, n0.z, n0.y);
@if(VERTEX_TANGENTS) {
    // A terrain patch: the decoration's own vertex frame (terrainpatch.ssh deferredVS / deferredPS).
    Tt = in.world_tangent.xyz;
    Bt = in.world_tangent.w * cross(n0, in.world_tangent.xyz);
}
    let V = normalize(view.world_position - wpos);
    // tangent-space view vector of the snow shader's VS
    let vts = vec3<f32>(
        dot(normalize(vec2<f32>(n0.y, -n0.x)), V.xy),
        dot(normalize(vec2<f32>(n0.z, n0.y)), V.yz),
        dot(n0, V));

    var T = 1.0;
    var albedo = vec3<f32>(0.0);
    var nenc = vec3<f32>(0.0);
    var matx = 0.0;
    var logp = 0.0;
    var spec = 0.0;
    // Quality 3: every layer's coverage times `soft`, composited bottom first
    // with SrcAlpha / InvSrcAlpha, which equals this top-first loop with
    // coverage soft·(1 − a); the cover left over keeps the cleared G-buffer (0).
    // A patch: its painted layers in the same composite, over the decoration (alpha = the cover).
    var soft = 1.0;
    if terrain.grid.z > 0.5 && !is_patch {
        soft = ultra_soft(in.position, n0);
    }
    let count = i32(terrain.origin.w);
    for (var i = count - 1; i >= 0; i--) {
        if is_patch {
            if terrain.layers[i].z < 0.5 {
                continue;
            }
        } else if textureLoad(opacity, cell, i, 0).g < 0.5 {
            continue;
        }
        let op = textureSampleLevel(opacity, opacity_smp, ouv, i, 0.0).r;
        var base = textureSampleGrad(colors, colors_smp, tuv, i, gx, gy);
        let nrm = textureSampleGrad(normals, normals_smp, tuv, i, gx, gy);
        let a = saturate(4.0 * (base.a - op));
        let cover = soft * (1.0 - a);
        let w = T * cover;
        T *= 1.0 - cover;
        let n = nrm.xyz * 2.0 - vec3<f32>(1.0);
        let N = normalize(n.x * Tt + n.y * Bt + n.z * n0);
        var c = base.rgb;
        if terrain.layers[i].x > 0.5 {
            // snowhighquality.ssh deferredPS
            c = c + (vec3<f32>(1.0) - c) * 0.1;
            let d = max(dot(n, vts), 0.0);
            let f = max((1.0 - pow(max(d, 1e-6), 1.15)) * 0.65, 0.0);
            c = c + f * (vec3<f32>(0.52, 0.81, 0.95) - c);
            c = c + min(d * d * 0.25, 1.0) * (vec3<f32>(0.74, 0.9, 1.0) - c);
            c = c + (1.0 - d) * (1.0 - d) * vec3<f32>(2.125, 2.25, 2.5) * (nrm.a * SPECULAR_INTENSITY + 0.125);
        }
        albedo += w * min(c, vec3<f32>(1.0));
        nenc += w * (N * 0.5 + vec3<f32>(0.5));
        matx += w * AMBIENT_INTENSITY;
        logp += w * log2(terrain.layers[i].y) / 10.5;
        spec += w * nrm.a * SPECULAR_INTENSITY;
        if T <= 0.0 {
            break;
        }
    }
    // A patch blends the G-buffer values of its layers over the decoration's with alpha = their cover: here
    // the lit colour of their normalised composite, blended (PORT, like gbuffer_blend.rs).
    var alpha = 1.0;
    if is_patch {
        alpha = 1.0 - T;
        if alpha <= 0.0 {
            discard;
        }
        let k = 1.0 / alpha;
        albedo *= k;
        nenc *= k;
        matx *= k;
        logp *= k;
        spec *= k;
    }
    albedo = min(albedo, vec3<f32>(1.0));
    let N = nenc * 2.0 - vec3<f32>(1.0);
    let power = exp2(10.5 * logp);
    // reflectionStyle (render/reflection.rs): base × (sat(N·L)·sun + hemisphere
    // ambient), no point lights / shadows / specular.
    if is_reflection_view() {
        let hemi = mix(env[3].rgb, env[2].rgb, N.y * 0.5 + 0.5);
        var col = albedo * matx * hemi;
        if env[0].w > 0.5 {
            col += saturate(dot(N, env[0].xyz)) * albedo * env[1].rgb;
        }
        out.color = vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(env[4].x)), alpha);
        return out;
    }

    // deferred ambient + directional + point passes
    let hemi = mix(env[3].rgb, env[2].rgb, dot(N, vec3<f32>(0.0, 1.0, 0.0)) * 0.5 + 0.5);
    var col = min(albedo * matx * hemi, vec3<f32>(env[4].x));
    if env[0].w > 0.5 {
        let L = env[0].xyz;
        let d = dot(N, L);
        let R = 2.0 * d * N - L;
        let s = exp2(log2(max(saturate(dot(V, R)), 1e-8)) * power);
        col += min((saturate(d) * albedo + s * min(spec, 1.0)) * env[1].rgb * sun_shadow(in.world_position), vec3<f32>(env[4].x));
    }
    let n_lights = u32(env[3].w);
    for (var k = 0u; k < n_lights; k++) {
        let lp = env[5u + 2u * k];
        // FireLights step their colour on the CPU (world::Flicker).
        let lc = env[6u + 2u * k];
        let lv = lp.xyz - wpos;
        let dist = length(lv);
        let att = max(1.0 - (dist / lp.w) * (dist / lp.w), 0.0);
        if att > 0.0 {
            let extra = env[5u + 2u * n_lights + k];
            let sh = point_shadow(lp.xyz, wpos, extra.x);
            let L = lv / dist;
            var t = saturate(dot(N, L)) * albedo;
            // pointSpecularPS: per light (lightingQuality, simple mode; render/env.rs).
            if extra.y > 0.5 {
                let R = 2.0 * dot(N, L) * N - L;
                t += exp2(log2(max(saturate(dot(V, R)), 1e-8)) * power) * min(spec, 1.0);
            }
            col += min(t * lc.rgb * att * sh, vec3<f32>(env[4].x));
        }
    }
    out.color = vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(env[4].x)), alpha);
    return out;
}
