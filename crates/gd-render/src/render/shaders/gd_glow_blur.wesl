// The glow blur (render/post.rs, "Scene glow"): `GraphicsCanvas_Blur8Tap` over the glow view's
// target, vertical into w × h/2 (`blur_v`), horizontal into w/2 × h/2 (`blur_h`), for the scene glow
// (rgb, taps `gains.y` texels apart) and the outline mask (alpha: each texel's class 1 / 2 / 3 decoded
// to its palette colour before blending, taps 1 texel apart) at once. The composite (gd_glow.wesl)
// adds the result.
import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

// The composite's uniform (post.rs `GlowParams`).
struct GlowParams {
    // x: scene glow gain (0.75²), y: its tap spacing in texels (4), z: outline gain (1.25²), w: debug view.
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

struct Vertical {
    @location(0) glow: vec4<f32>,
    @location(1) outline: vec4<f32>,
}

// Taps at ±{0.5..3.5} source texels from the output texel's centre, which lies on the edge between
// source rows 2y and 2y + 1.
@fragment
fn blur_v(in: FullscreenVertexOutput) -> Vertical {
    let size = vec2<i32>(textureDimensions(src_a));
    let step = vec2<f32>(0.0, glow.gains.y / f32(size.y));
    var g = vec3<f32>(0.0);
    for (var i = 0; i < 8; i++) {
        g += W[i] * textureSampleLevel(src_a, smp, in.uv + step * (f32(i) - 3.5), 0.0).rgb;
    }
    var o = vec3<f32>(0.0);
    if glow.outline.w > 0.5 {
        // One texel apart from a texel edge the taps hit texel centres: loads, so the classes do
        // not blend before they are decoded. Rows 2y − 3 … 2y + 4.
        let x = i32(in.position.x);
        let y0 = i32(in.position.y) * 2 - 3;
        for (var i = 0; i < 8; i++) {
            let y = clamp(y0 + i, 0, size.y - 1);
            o += W[i] * class_colour(textureLoad(src_a, vec2<i32>(x, y), 0).a);
        }
    }
    var out: Vertical;
    out.glow = vec4<f32>(g, 0.0);
    out.outline = vec4<f32>(o, 0.0);
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
            let uv = in.uv + vec2<f32>(texel * (f32(i) - 3.5), 0.0);
            o += W[i] * textureSampleLevel(src_b, smp, uv, 0.0).rgb;
        }
        c += o * glow.gains.z;
    }
    return vec4<f32>(c, 0.0);
}
