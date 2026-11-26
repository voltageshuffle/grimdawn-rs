// The glow blur: `GraphicsCanvas_Blur8Tap` over the glow view's
// target, vertical into w × h/2 (`blur_v`, `blur_v_occluded`), horizontal into w/2 × h/2 (`blur_h`), for the scene
// glow (rgb, taps `gains.y` texels apart) and the outline mask (alpha: each texel's class 1 / 2 / 3 decoded
// to its palette colour before blending, taps half a texel apart) at once. The scene glow view is half
// screen size (post.rs `setup_glow`), so spacings are halved in glow texels to keep the engine's on-screen
// radius: scene glow `gains.y` = 2 (= 4 screen px), outline ½ texel (= the engine's 1 screen px). A
// portrait's glow view is full size but has no outline mask, so the outline spacing never applies to it.
import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

// The composite's uniform (`GlowParams`).
struct GlowParams {
    // x: scene glow gain (0.75²), y: its tap spacing in texels (2 = 4 screen px), z: outline gain (1.25²), w: debug view.
    gains: vec4<f32>,
    // Class 1's colour (rgb; w 1 = something is outlined), classes 2 and 3.
    outline: vec4<f32>,
    outline2: vec4<f32>,
    outline3: vec4<f32>,
}

@group(0) @binding(0) var<uniform> glow: GlowParams;
// blur_v: the glow target (both); blur_h: the vertical scene glow and outline.
@group(0) @binding(1) var src_a: texture_2d<f32>;
@group(0) @binding(2) var src_b: texture_2d<f32>;
@group(0) @binding(3) var smp: sampler;
// blur_v_occluded only: the glow view's depth and the scene's (reverse Z, near / distance).
@group(0) @binding(4) var glow_depth: texture_depth_2d;
@group(0) @binding(5) var scene_depth: texture_depth_2d;

const W: array<f32, 8> = array<f32, 8>(0.10, 0.11, 0.14, 0.15, 0.15, 0.14, 0.11, 0.10);

// A mask texel's colour: its class's palette entry.
fn class_colour(a: f32) -> vec3<f32> {
    if a > 2.5 {
        return glow.outline3.rgb;
    }
    if a > 1.5 {
        return glow.outline2.rgb;
    }
    if a > 0.5 {
        return glow.outline.rgb;
    }
    return vec3<f32>(0.0);
}

// A scene surface hides a glowing one more than 0.1 % of its view distance behind it.
const HIDDEN: f32 = 1.001;

// The glow at texel `p` the scene shows: none where a nearer scene surface covers the glowing one. A texel the glow
// view has no depth at (only blended glow there, a decal's glow layer) keeps its glow.
fn shown_glow(p: vec2<i32>) -> vec3<f32> {
    let g = textureLoad(src_a, p, 0).rgb;
    let d = textureLoad(glow_depth, p, 0);
    // The scene depth is the main view's size: scale the glow texel into it (the scene glow view is
    // half size; a portrait's is full size — the ratio covers both).
    let s = vec2<i32>(vec2<f32>(p) * vec2<f32>(textureDimensions(scene_depth))
        / vec2<f32>(textureDimensions(src_a)));
    if d > 0.0 && textureLoad(scene_depth, s, 0) > d * HIDDEN {
        return vec3<f32>(0.0);
    }
    return g;
}

// The outline's vertical pass: taps half a source texel apart (the engine's 1 screen px at the half-size
// glow view), centred on the output texel's centre, source row 2y + ½. Each tap lands between two texel
// centres: the two classes are decoded then mixed, so they do not blend before decoding.
fn outline_v(position: vec4<f32>, size: vec2<i32>) -> vec3<f32> {
    var o = vec3<f32>(0.0);
    if glow.outline.w > 0.5 {
        let x = i32(position.x);
        let y = f32(i32(position.y)) * 2.0 + 0.5;
        for (var i = 0; i < 8; i++) {
            let t = y + 0.5 * (f32(i) - 3.5);
            let r = i32(floor(t));
            let a = class_colour(textureLoad(src_a, vec2<i32>(x, clamp(r, 0, size.y - 1)), 0).a);
            let b = class_colour(textureLoad(src_a, vec2<i32>(x, clamp(r + 1, 0, size.y - 1)), 0).a);
            o += W[i] * mix(a, b, t - floor(t));
        }
    }
    return o;
}

struct Vertical {
    @location(0) glow: vec4<f32>,
    @location(1) outline: vec4<f32>,
}

// Taps at ±{0.5..3.5}·`gains.y` source texels from the output texel's centre, which lies on the edge
// between source rows 2y and 2y + 1.
@fragment
fn blur_v(in: FullscreenVertexOutput) -> Vertical {
    let size = vec2<i32>(textureDimensions(src_a));
    let step = vec2<f32>(0.0, glow.gains.y / f32(size.y));
    var g = vec3<f32>(0.0);
    for (var i = 0; i < 8; i++) {
        g += W[i] * textureSampleLevel(src_a, smp, in.uv + step * (f32(i) - 3.5), 0.0).rgb;
    }
    var out: Vertical;
    out.glow = vec4<f32>(g, 0.0);
    out.outline = vec4<f32>(outline_v(in.position, size), 0.0);
    return out;
}

// `blur_v` for the scene glow, without the texels the scene hides (`shown_glow`): each tap's two rows (its
// bilinear filter by hand) are tested alone.
@fragment
fn blur_v_occluded(in: FullscreenVertexOutput) -> Vertical {
    let size = vec2<i32>(textureDimensions(src_a));
    let x = i32(in.position.x);
    var g = vec3<f32>(0.0);
    for (var i = 0; i < 8; i++) {
        let t = in.uv.y * f32(size.y) + glow.gains.y * (f32(i) - 3.5) - 0.5;
        let r = i32(floor(t));
        let a = shown_glow(vec2<i32>(x, clamp(r, 0, size.y - 1)));
        let b = shown_glow(vec2<i32>(x, clamp(r + 1, 0, size.y - 1)));
        g += W[i] * mix(a, b, t - floor(t));
    }
    var out: Vertical;
    out.glow = vec4<f32>(g, 0.0);
    out.outline = vec4<f32>(outline_v(in.position, size), 0.0);
    return out;
}

// The second axis and both gains: scene glow and outline summed into one target.
@fragment
fn blur_h(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let texel = 1.0 / f32(textureDimensions(src_a).x);
    let step = vec2<f32>(glow.gains.y * texel, 0.0);
    var g = vec3<f32>(0.0);
    for (var i = 0; i < 8; i++) {
        g += W[i] * textureSampleLevel(src_a, smp, in.uv + step * (f32(i) - 3.5), 0.0).rgb;
    }
    var c = g * glow.gains.x;
    if glow.outline.w > 0.5 {
        var o = vec3<f32>(0.0);
        for (var i = 0; i < 8; i++) {
            // Half a texel: the engine's 1 screen px at the half-size glow view; the colours are
            // already decoded, so the bilinear filter blends them.
            let uv = in.uv + vec2<f32>(0.5 * texel * (f32(i) - 3.5), 0.0);
            o += W[i] * textureSampleLevel(src_b, smp, uv, 0.0).rgb;
        }
        c += o * glow.gains.z;
    }
    return vec4<f32>(c, 0.0);
}
