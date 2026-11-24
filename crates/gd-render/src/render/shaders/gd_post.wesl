// The engine's final image chain (render/post.rs module doc): the deferred
// fog (deferred.ssh fogPS), bloom (filter.ssh extractHot, the 8-tap blur,
// composite lighten), enhancement (postprocessing/enhance.ssh), depth of
// field (downsample.ssh normal, depthoffield.ssh), then gamma → linear so
// the sRGB swapchain's encode reproduces the game's back-buffer bytes. Before
// them the ambient occlusion (postprocessing/ssao.ssh) multiplies the scene.
import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
import bevy_render::view::View;

struct GammaOut {
    // 1 − a, ½ / a, saturation, 1 = on
    bloom: vec4<f32>,
    // x: blur gain
    bloom_gain: vec4<f32>,
    // brightness, contrast, shader saturation, 1 = on
    adjust: vec4<f32>,
    tint: vec4<f32>,
    // rgb, 1 = on
    fog_color: vec4<f32>,
    // start, 1 / (end - start), density
    depth_fog: vec4<f32>,
    height_fog: vec4<f32>,
    // dofDistance, dofRange, -, 1 = on
    dof: vec4<f32>,
    // colour-blind remap rows; remap_r.w = 1 on
    remap_r: vec4<f32>,
    remap_g: vec4<f32>,
    remap_b: vec4<f32>,
    // x: 1 = hdrLighting's tone map
    hdr: vec4<f32>,
    // 1 = on, ssaoStrength, ssaoScale, zFar
    ao: vec4<f32>,
    // fullscreen gamma ramp: 2c, 2b - 1, 1 / g, 1 = on (post.rs `with_gamma_ramp`)
    gamma_ramp: vec4<f32>,
}

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var<uniform> settings: GammaOut;
@group(0) @binding(3) var aux_tex: texture_2d<f32>;
@group(0) @binding(4) var depth_tex: texture_depth_2d;
@group(0) @binding(5) var<uniform> view: View;

// Hardware depth at uv (reverse Z: 0 = nothing drawn / infinitely far).
fn depth_at(uv: vec2<f32>) -> f32 {
    let size = vec2<f32>(textureDimensions(depth_tex));
    let p = clamp(vec2<i32>(uv * size), vec2<i32>(0), vec2<i32>(size) - vec2<i32>(1));
    return textureLoad(depth_tex, p, 0);
}

// Linear view depth (the engine's zNear·zFar / (z·(zNear − zFar) + zFar));
// Bevy's infinite reverse-Z perspective stores near / z.
fn view_depth(d: f32) -> f32 {
    return view.clip_from_view[3][2] / d;
}

// deferred.ssh fogPS (tech fog, SrcAlpha / OneMinusSrcAlpha): where geometry
// was drawn, a = max(sat((z − start)·inv)·density, sat((h − start)·inv)·density)
// with z the view depth and h the world height (the engine passes the height
// start relative to the camera and dots the camera-space position with the
// up vector: the same), colour = fogColor.
@fragment
fn fog(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let c = textureSample(src_tex, smp, in.uv);
    let d = depth_at(in.uv);
    if d <= 0.0 {
        return c;
    }
    let z = view_depth(d);
    let ndc = vec4<f32>(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0, d, 1.0);
    let w = view.world_from_clip * ndc;
    let h = w.y / w.w;
    let df = saturate((z - settings.depth_fog.x) * settings.depth_fog.y) * settings.depth_fog.z;
    let hf = saturate((h - settings.height_fog.x) * settings.height_fog.y) * settings.height_fog.z;
    let a = max(df, hf);
    return vec4<f32>(mix(c.rgb, settings.fog_color.rgb, a), c.a);
}

// deferred.ssh tonemapPS (`PerformTonemapping`, hdrLighting only): in log2 space, below
// log2 c = −2.161 (c < 0.2236) unchanged, above a Hermite shoulder that reaches 1 only at
// infinity (c = 1 → 0.64, 2 → 0.82, 4 → 0.93); c <= 0 → 0.
fn tonemap(c: vec3<f32>) -> vec3<f32> {
    let l = log2(max(c, vec3<f32>(1e-30)));
    let x = l * 0.231378213 + vec3<f32>(0.5);
    let t = x * 0.666666687;
    let t2 = t * t;
    let t3 = t2 * t;
    let h00 = 2.0 * t3 - 3.0 * t2 + vec3<f32>(1.0);
    let h10 = t3 - 2.0 * t2 + t;
    let y = select(1.5 * h10 - 0.5 * h00, l * 0.231378213, l < vec3<f32>(-2.16096401));
    let o = exp2(min(y * 4.32192802, vec3<f32>(0.0)));
    return select(o, vec3<f32>(0.0), c <= vec3<f32>(0.0));
}

fn src(uv: vec2<f32>) -> vec3<f32> {
    // The scene target is float; the engine's back buffer is UNORM. With hdrLighting the
    // scene is in a float target that the tone map copies into the back buffer.
    let c = textureSample(src_tex, smp, uv).rgb;
    if settings.hdr.x > 0.5 {
        return saturate(tonemap(c));
    }
    return saturate(c);
}

// filter.ssh psExtractHot: 4 taps at ±½ texel of the 512² frame copy.
@fragment
fn extract_hot(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let o = vec2<f32>(0.5 / 512.0);
    let avg = 0.25 * (src(in.uv + vec2<f32>(-o.x, -o.y)) + src(in.uv + vec2<f32>(o.x, -o.y))
        + src(in.uv + vec2<f32>(-o.x, o.y)) + src(in.uv + vec2<f32>(o.x, o.y)));
    let h = saturate(avg - vec3<f32>(settings.bloom.x)) * settings.bloom.y;
    let r = h + h;
    let m = dot(r, vec3<f32>(0.333333));
    return vec4<f32>(m + settings.bloom.z * (r - vec3<f32>(m)), 1.0);
}

// `GraphicsCanvas_Blur8Tap`: gain · {.10,.11,.14,.15,.15,.14,.11,.10} at ±{0.5..3.5}
// source texels.
fn blur(uv: vec2<f32>, step: vec2<f32>, gain: f32) -> vec4<f32> {
    let w = array<f32, 8>(0.10, 0.11, 0.14, 0.15, 0.15, 0.14, 0.11, 0.10);
    var c = vec3<f32>(0.0);
    for (var i = 0; i < 8; i++) {
        c += w[i] * textureSample(src_tex, smp, uv + step * (f32(i) - 3.5)).rgb;
    }
    return vec4<f32>(c * gain, 1.0);
}

fn step_v() -> vec2<f32> {
    return vec2<f32>(0.0, 1.0 / f32(textureDimensions(src_tex).y));
}

fn step_h() -> vec2<f32> {
    return vec2<f32>(1.0 / f32(textureDimensions(src_tex).x), 0.0);
}

@fragment
fn blur_v(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return blur(in.uv, step_v(), settings.bloom_gain.x);
}

@fragment
fn blur_h(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return blur(in.uv, step_h(), settings.bloom_gain.x);
}

// ssao.ssh createPS (render/post.rs module doc). The kernel as compiled
// (its z repeats x).
const AO_KERNEL = array<vec3<f32>, 8>(
    vec3<f32>(0.527837, -0.085868, 0.527837),
    vec3<f32>(-0.040088, 0.536087, -0.040088),
    vec3<f32>(-0.670445, -0.179949, -0.670445),
    vec3<f32>(-0.419418, -0.616039, -0.419418),
    vec3<f32>(0.440453, -0.639399, 0.440453),
    vec3<f32>(-0.757088, 0.349334, -0.757088),
    vec3<f32>(0.574619, 0.685879, 0.574619),
    vec3<f32>(0.038510, -0.939059, 0.038510),
);

// The 4 × 4 rotation texture (`GraphicsCanvas::CreateSsaoRotationTexture`) as the shader reads it
// (byte / 255 · 2 − 1): the first 16 vectors of a seed-1 MSVC rand() stream.
const AO_ROTATION = array<vec3<f32>, 16>(
    vec3<f32>(-0.850980, 0.105882, -0.521569),
    vec3<f32>(0.960784, 0.262745, -0.066667),
    vec3<f32>(-0.286275, 0.741176, 0.600000),
    vec3<f32>(0.450980, -0.600000, 0.654902),
    vec3<f32>(0.725490, 0.043137, -0.686275),
    vec3<f32>(-0.749020, -0.631373, -0.215686),
    vec3<f32>(-0.513725, -0.490196, 0.701961),
    vec3<f32>(-0.090196, -0.607843, -0.796078),
    vec3<f32>(-0.968627, -0.247059, 0.058824),
    vec3<f32>(0.427451, 0.615686, 0.647059),
    vec3<f32>(-0.898039, 0.427451, -0.137255),
    vec3<f32>(-0.309804, -0.929412, 0.223529),
    vec3<f32>(0.678431, 0.725490, 0.043137),
    vec3<f32>(-0.411765, 0.780392, 0.466667),
    vec3<f32>(0.725490, 0.678431, 0.058824),
    vec3<f32>(-0.803922, -0.090196, -0.600000),
);

// View depth / zFar at uv, the raw depth filtered bilinearly first like the
// engine's depth sampler (clamped; nothing drawn = the far plane).
fn ao_depth(uv: vec2<f32>) -> f32 {
    let size = vec2<i32>(textureDimensions(depth_tex));
    let p = uv * vec2<f32>(size) - vec2<f32>(0.5);
    let b = floor(p);
    let f = p - b;
    let i0 = vec2<i32>(b);
    let hi = size - vec2<i32>(1);
    let t00 = textureLoad(depth_tex, clamp(i0, vec2<i32>(0), hi), 0);
    let t10 = textureLoad(depth_tex, clamp(i0 + vec2<i32>(1, 0), vec2<i32>(0), hi), 0);
    let t01 = textureLoad(depth_tex, clamp(i0 + vec2<i32>(0, 1), vec2<i32>(0), hi), 0);
    let t11 = textureLoad(depth_tex, clamp(i0 + vec2<i32>(1, 1), vec2<i32>(0), hi), 0);
    let d = mix(mix(t00, t10, f.x), mix(t01, t11, f.x), f.y);
    let far = settings.ao.w;
    if d <= 0.0 {
        return 1.0;
    }
    return min(view_depth(d), far) / far;
}

@fragment
fn ao_create(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let px = vec2<u32>(in.position.xy) % vec2<u32>(4u);
    let r = AO_ROTATION[px.y * 4u + px.x];
    let c = ao_depth(in.uv);
    var sum = 0.0;
    for (var i = 0; i < 8; i++) {
        let k = AO_KERNEL[i];
        let o = k.xy - r.xy * (2.0 * dot(k, r));
        let s = ao_depth(o * settings.ao.z + in.uv);
        if c < s {
            sum += 1.0 - min(abs(s - c) * 12.0, 1.0);
        }
    }
    let occ = min(sum * 0.25, 1.0);
    return vec4<f32>(vec3<f32>(occ * (1.0 - settings.ao.y) + settings.ao.y), 1.0);
}

// `GraphicsCanvas_Blur8Tap` with gain 1 and spread 1 (`Engine.ssaoBlurSpread`).
@fragment
fn ao_blur_v(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return blur(in.uv, step_v(), 1.0);
}

@fragment
fn ao_blur_h(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return blur(in.uv, step_h(), 1.0);
}

// ssao.ssh renderPS: a plain sample, multiplied onto the scene (blend Dst / Zero).
@fragment
fn ao_apply(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSample(src_tex, smp, in.uv).rgb, 1.0);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

// Bloom composite and enhancement, in gamma space.
fn graded(uv: vec2<f32>) -> vec3<f32> {
    var c = src(uv);
    if settings.bloom.w > 0.5 {
        // composite.ssh lighten: One / One onto the UNORM frame.
        c = saturate(c + textureSample(aux_tex, smp, uv).rgb);
    }
    if settings.adjust.w > 0.5 {
        let lum = dot(c, vec3<f32>(0.3, 0.59, 0.11));
        c = settings.adjust.z * (vec3<f32>(lum) - c) + c;
        c = (c - vec3<f32>(0.5)) * settings.adjust.y + c;
        c = saturate(c + vec3<f32>(settings.adjust.x)) * settings.tint.rgb;
    }
    if settings.remap_r.w > 0.5 {
        c = saturate(vec3<f32>(dot(settings.remap_r.xyz, c), dot(settings.remap_g.xyz, c), dot(settings.remap_b.xyz, c)));
    }
    return c;
}

// Into the frame (gamma values, the UI is drawn over them) or the UNORM frame copy that the depth of field reads.
@fragment
fn finish(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(graded(in.uv), 1.0);
}

// After the UI: the gamma-space frame → linear, which the sRGB swapchain encodes back to the same values.
@fragment
fn to_linear(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    var c = saturate(textureSample(src_tex, smp, in.uv).rgb);
    let r = settings.gamma_ramp;
    if r.w > 0.5 {
        // UpdateGammaRamp: sat(((x - 0.5)·2c + 0.5 + 2b - 1)^(1/g)).
        c = saturate(pow(max((c - 0.5) * r.x + 0.5 + r.y, vec3<f32>(0.0)), vec3<f32>(r.z)));
    }
    return vec4<f32>(srgb_to_linear(c), 1.0);
}

// downsample.ssh `normal`: one bilinear tap per half-size texel (its centre
// lies between 2 × 2 source texels: their mean).
@fragment
fn downsample(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSample(src_tex, smp, in.uv).rgb, 1.0);
}

// depthoffield.ssh: f = sat((1.27·z − dofDistance) / dofRange), out =
// lerp(frame, half-size blur, f).
@fragment
fn depth_of_field(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let base = textureSample(src_tex, smp, in.uv).rgb;
    let blur = textureSample(aux_tex, smp, in.uv).rgb;
    let d = depth_at(in.uv);
    // Nothing drawn: the cleared depth is the far plane (blurred).
    var f = 1.0;
    if d > 0.0 {
        f = saturate((view_depth(d) * 1.27 - settings.dof.x) / settings.dof.y);
    }
    return vec4<f32>(mix(base, blur, f), 1.0);
}

// postprocessing/fxaa.ssh `normal` (GraphicsCanvas::ApplyFXAA; render/post.rs module doc): FXAA from the UNORM
// frame copy, pixelSize = 1 / its size. A transcription of the shader's DXBC (Lottes' FXAA, no edge threshold early
// out): luma = 1.963·r + g, sub-pixel blend sat(|avg4 − M| / range − ¼)·4/3 capped at ¾ towards the 3 × 3 mean, an
// edge search of up to 32 steps on each side ending where |luma − pair| >= ¼ gradient.
fn fxaa_luma(c: vec3<f32>) -> f32 {
    return c.r * 1.9632107 + c.g;
}

fn fxaa_tap(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(src_tex, smp, uv, 0.0).rgb;
}

@fragment
fn fxaa(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let px = 1.0 / vec2<f32>(textureDimensions(src_tex));
    let rgb_n = fxaa_tap(uv + vec2<f32>(0.0, -px.y));
    let rgb_w = fxaa_tap(uv + vec2<f32>(-px.x, 0.0));
    let rgb_m = fxaa_tap(uv);
    let rgb_e = fxaa_tap(uv + vec2<f32>(px.x, 0.0));
    let rgb_s = fxaa_tap(uv + vec2<f32>(0.0, px.y));
    let l_n = fxaa_luma(rgb_n);
    let l_w = fxaa_luma(rgb_w);
    let l_m = fxaa_luma(rgb_m);
    let l_e = fxaa_luma(rgb_e);
    let l_s = fxaa_luma(rgb_s);
    let range = max(max(max(l_n, l_s), max(l_w, l_e)), l_m) - min(min(min(l_n, l_s), min(l_w, l_e)), l_m);
    let luma_l = (l_n + l_w + l_e + l_s) * 0.25 - l_m;
    var blend_l = 0.0;
    if range != 0.0 {
        blend_l = min(max(abs(luma_l) / range - 0.25, 0.0) * 1.3333334, 0.75);
    }
    let rgb_nw = fxaa_tap(uv + vec2<f32>(-px.x, -px.y));
    let rgb_ne = fxaa_tap(uv + vec2<f32>(px.x, -px.y));
    let rgb_sw = fxaa_tap(uv + vec2<f32>(-px.x, px.y));
    let rgb_se = fxaa_tap(uv + vec2<f32>(px.x, px.y));
    let rgb_l = rgb_n + rgb_w + rgb_m + rgb_e + rgb_s + rgb_nw + rgb_ne + rgb_sw + rgb_se;
    let l_nw = fxaa_luma(rgb_nw);
    let l_ne = fxaa_luma(rgb_ne);
    let l_sw = fxaa_luma(rgb_sw);
    let l_se = fxaa_luma(rgb_se);
    let edge_vert = abs(0.25 * l_nw - 0.5 * l_n + 0.25 * l_ne) + abs(0.5 * l_w - l_m + 0.5 * l_e)
        + abs(0.25 * l_sw - 0.5 * l_s + 0.25 * l_se);
    let edge_horz = abs(0.25 * l_nw - 0.5 * l_w + 0.25 * l_sw) + abs(0.5 * l_n - l_m + 0.5 * l_s)
        + abs(0.25 * l_ne - 0.5 * l_e + 0.25 * l_se);
    let horz = edge_horz >= edge_vert;
    var length_sign = select(-px.x, -px.y, horz);
    var luma_a = select(l_w, l_n, horz);
    var luma_b = select(l_e, l_s, horz);
    let grad_a = abs(luma_a - l_m);
    let grad_b = abs(luma_b - l_m);
    var pair = (luma_a + l_m) * 0.5;
    var gradient = grad_a;
    if grad_a < grad_b {
        pair = (luma_b + l_m) * 0.5;
        length_sign = -length_sign;
        gradient = grad_b;
    }
    let half_step = length_sign * 0.5;
    let threshold = gradient * 0.25;
    let pos = uv + select(vec2<f32>(half_step, 0.0), vec2<f32>(0.0, half_step), horz);
    let off = select(vec2<f32>(0.0, px.y), vec2<f32>(px.x, 0.0), horz);
    var pos_n = pos - off;
    var pos_p = pos + off;
    var end_n = pair;
    var end_p = pair;
    var done_n = false;
    var done_p = false;
    for (var i = 0; i < 32; i++) {
        if !done_n {
            end_n = fxaa_luma(fxaa_tap(pos_n));
        }
        if !done_p {
            end_p = fxaa_luma(fxaa_tap(pos_p));
        }
        done_n = done_n || abs(end_n - pair) >= threshold;
        done_p = done_p || abs(end_p - pair) >= threshold;
        if done_n && done_p {
            break;
        }
        if !done_n {
            pos_n -= off;
        }
        if !done_p {
            pos_p += off;
        }
    }
    let dst_n = select(uv.y - pos_n.y, uv.x - pos_n.x, horz);
    let dst_p = select(pos_p.y - uv.y, pos_p.x - uv.x, horz);
    let luma_end = select(end_p, end_n, dst_n < dst_p);
    if (l_m - pair < 0.0) == (luma_end - pair < 0.0) {
        length_sign = 0.0;
    }
    let sub = (0.5 - min(dst_n, dst_p) / (dst_n + dst_p)) * length_sign;
    let rgb_f = fxaa_tap(uv + select(vec2<f32>(sub, 0.0), vec2<f32>(0.0, sub), horz));
    return vec4<f32>(mix(rgb_f, rgb_l * (1.0 / 9.0), blend_l), 1.0);
}
