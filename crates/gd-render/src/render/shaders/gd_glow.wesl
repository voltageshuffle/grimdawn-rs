// Blurred-glow composite (render/post.rs): ApplyGlow's separable 8×8 taps,
// added onto the main view. rgb: the scene glow (0.75, 4 texels); alpha:
// the outline mask (1.25, 1 texel), each texel's class (1 / 2 / 3) its
// palette colour (outline / outline2 / outline3). Neither is added over an
// outlined object's own pixels (glow.ssh's stencil test).
import bevy_pbr::render::mesh_view_bindings::view;

struct GlowParams {
    params: vec4<f32>,
    outline: vec4<f32>,
    outline2: vec4<f32>,
    outline3: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> glow: GlowParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var glow_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var glow_smp: sampler;

struct VOut {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn vertex(@location(0) position: vec3<f32>) -> VOut {
    var o: VOut;
    o.position = vec4<f32>(position.xy, 0.0, 1.0);
    return o;
}

const W: array<f32, 8> = array<f32, 8>(0.1, 0.11, 0.14, 0.15, 0.15, 0.14, 0.11, 0.1);

// The 8 taps at ±0.5…3.5 texels, bilinear: texel k (−4…4) of the 9 they
// cover weighs (W[k + 4] + W[k + 3]) / 2.
fn w9(k: i32) -> f32 {
    var w = 0.0;
    if k + 4 <= 7 {
        w += W[k + 4];
    }
    if k + 3 >= 0 {
        w += W[k + 3];
    }
    return w * 0.5;
}

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

@fragment
fn fragment(in: VOut) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(glow_tex));
    let uv = (in.position.xy - view.viewport.xy) / view.viewport.zw;
    let centre = textureSampleLevel(glow_tex, glow_smp, uv, 0.0);
    if glow.params.w > 0.5 {
        // GD_GLOW_DEBUG: the unblurred glow target, mask in magenta.
        return vec4<f32>(centre.rgb + vec3<f32>(centre.a, 0.0, centre.a), 0.0);
    }
    // Stencil 255 (the outlined object's visible pixels): nothing added.
    if centre.a > 0.5 {
        return vec4<f32>(0.0);
    }
    let step = glow.params.y / size;
    var acc = vec3<f32>(0.0);
    for (var j = 0; j < 8; j++) {
        for (var i = 0; i < 8; i++) {
            let o = vec2<f32>(f32(i) - 3.5, f32(j) - 3.5) * step;
            acc += W[i] * W[j] * textureSampleLevel(glow_tex, glow_smp, uv + o, 0.0).rgb;
        }
    }
    var c = acc * glow.params.x;
    if glow.outline.w > 0.5 {
        // Texels, not filtered samples: the classes must not blend before
        // they are decoded.
        let at = vec2<i32>(uv * size);
        let top = vec2<i32>(size) - vec2<i32>(1);
        var m = vec3<f32>(0.0);
        for (var j = -4; j <= 4; j++) {
            for (var i = -4; i <= 4; i++) {
                let p = clamp(at + vec2<i32>(i, j), vec2<i32>(0), top);
                m += w9(i) * w9(j) * class_colour(textureLoad(glow_tex, p, 0).a);
            }
        }
        c += m * glow.params.z;
    }
    return vec4<f32>(c, 0.0);
}
