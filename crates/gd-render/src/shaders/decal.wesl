// Terrain decals: UVs are the raw decal-space coordinates (Decal::CreateGeometry
// u = 0.5 - dot(p-c,U)/width, v = dot(p-c,V)/height + 0.5, decal.rs). Outside
// [0,1] is discarded; U is remapped into the texture variant strip.
// Textures are UNORM and shading is gamma-space like the terrain under it
// (gd_terrain.wesl: deferred ambient + sun + point lights, render/mod.rs).
// Deferred PS of decal_standard / decal_glow / decal_bump.ssh: the
// G-buffer gets albedo = texel (alpha = texel.a x opacity), material
// (ambient x1, ambient add 0), so ambient = albedo x hemi (terrain: x0.65);
// decal_standard / glow keep the terrain normal (normal alpha 0) and write no
// specular; decal_bump writes normal = n.x*T + n.y*B + n.z*N from bumpTexture
// (T = (N.y,-N.x,0), B = (0,N.z,N.y): the world-fixed terrain frame, not the
// decal's U/V), specular = bump.a, power 5 (0.221136 x 10.5 = log2 5).
// decal_selfillum.ssh is emissive: texel x colour, unlit, added after the
// lighting (its only technique emissiveStyle: SrcAlpha / One).
// The sun and point lights light that G-buffer like any other (deferred.ssh
// directionalPS / pointSpecularPS, gd_standard.wesl), the point lights with
// specular per light (not at lightingQuality low, nor in simple mode; render/env.rs).
// decal_standard / glow write specular (0, 0, 0, alpha) (decal_standard.ssh
// deferredPS): the terrain's is faded by 1 - alpha, as the blend over the lit
// terrain does here.
// PORT: the engine alpha-blends these into the G-buffer and lights the result;
// here the decal is lit on its own and alpha-blended over the lit terrain.
// A decal_glow decal adds its glow layer with a second, additive material
// (texture = the base for its alpha, bump binding = the glow texture), drawn
// in the main and the glow view.
import bevy_pbr::render::{
    forward_io::VertexOutput,
    mesh_functions,
    mesh_view_bindings::{view, lights, directional_shadow_textures, clustered_lights},
    mesh_view_types,
};

struct DecalParams {
    variant_offset: f32,
    variant_scale: f32,
    opacity: f32,
    // > 0: the additive decal_glow layer (bumpTexture rgb x alpha x gain).
    glow: f32,
    // 1: decal_bump (decal_bump binding: normal rgb, specular a).
    bump: f32,
    // 1: decal_selfillum (unlit).
    unlit: f32,
    pad0: f32,
    pad1: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> decal: DecalParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var decal_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var decal_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var<storage, read> env: array<vec4<f32>>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var decal_bump: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var decal_bump_sampler: sampler;

const BUMP_POWER: f32 = 5.0;

/// The sun shadow the way deferred.ssh `directionalPS` tests it (see
/// render/shadow.rs, which builds the engine's LiSPSM map): 4 bilinear taps at
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


@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        discard;
    }
    let tuv = vec2(uv.x * decal.variant_scale + decal.variant_offset, uv.y);
    let texel = textureSample(decal_texture, decal_sampler, tuv);
    // The layer's opacity (the record's, faded, × a ring alpha; its material is shared and unfaded): 1 − the f32
    // bits in its MeshTag (decal.rs `alpha_tag`; untagged = 1).
    let fade = 1.0 - bitcast<f32>(mesh_functions::get_tag(in.instance_index));
    let a = texel.a * decal.opacity * fade;
    if decal.glow > 0.0 {
        // decal_glow deferredPS: the G-buffer glow target gets
        // bumpTexture rgb x alpha x 4, blended by the base alpha x opacity
        // (the pass's SrcAlpha / OneMinusSrcAlpha on every target). That
        // target is added unblurred (here, additive) and blurred (the same
        // value into the glow view, post.rs).
        let g = textureSample(decal_bump, decal_bump_sampler, tuv);
        return vec4<f32>(g.rgb * g.a * texel.a * decal.glow * fade, 0.0);
    }
    let albedo = texel.rgb;
    if decal.unlit > 0.5 {
        // decal_selfillum emissiveStyle: SrcAlpha / One (additive; the
        // material's AlphaMode::Add is premultiplied with alpha 0). Not in
        // the glow view (its single pass writes no glow).
        if view.exposure > 0.1 {
            return vec4<f32>(0.0);
        }
        return vec4<f32>(albedo * a, 0.0);
    }
    if view.exposure > 0.1 {
        return vec4<f32>(0.0, 0.0, 0.0, a);
    }
    let n0 = normalize(in.world_normal);
    var N = n0;
    var spec = 0.0;
    // Sampled outside the branch: uniform control flow for the derivatives.
    let nrm = textureSample(decal_bump, decal_bump_sampler, tuv);
    if decal.bump > 0.5 {
        let n = nrm.xyz * 2.0 - vec3<f32>(1.0);
        let T = vec3<f32>(n0.y, -n0.x, 0.0);
        let B = vec3<f32>(0.0, n0.z, n0.y);
        N = normalize(n.x * T + n.y * B + n.z * n0);
        spec = nrm.a;
    }
    let wpos = in.world_position.xyz;
    let hemi = mix(env[3].rgb, env[2].rgb, dot(N, vec3<f32>(0.0, 1.0, 0.0)) * 0.5 + 0.5);
    var col = min(albedo * hemi, vec3<f32>(env[4].x));
    if env[0].w > 0.5 {
        let L = env[0].xyz;
        let d = dot(N, L);
        let V = normalize(view.world_position - wpos);
        let R = 2.0 * d * N - L;
        let s = exp2(log2(max(saturate(dot(V, R)), 1e-8)) * BUMP_POWER);
        col += min((saturate(d) * albedo + s * spec) * env[1].rgb * sun_shadow(in.world_position), vec3<f32>(env[4].x));
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
                let V = normalize(view.world_position - wpos);
                let R = 2.0 * dot(N, L) * N - L;
                t += exp2(log2(max(saturate(dot(V, R)), 1e-8)) * BUMP_POWER) * spec;
            }
            col += min(t * lc.rgb * att * sh, vec3<f32>(env[4].x));
        }
    }
    return vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(env[4].x)), a);
}
