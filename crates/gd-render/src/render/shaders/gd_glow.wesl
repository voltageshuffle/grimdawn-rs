// Glow composite (render/post.rs): adds the half-size blur of the scene glow and the outline mask
// (gd_glow_blur.wesl, ApplyGlow's separable 8-tap blurs) onto the main view, bilinearly upsampled.
// Nothing is added over an outlined object's own pixels (glow.ssh's stencil test): the glow target's
// unblurred mask (alpha, the texel's class 1 / 2 / 3) at the pixel.
import bevy_pbr::render::mesh_view_bindings::view;

struct GlowParams {
    gains: vec4<f32>,
    outline: vec4<f32>,
    outline2: vec4<f32>,
    outline3: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> glow: GlowParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var glow_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var glow_smp: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var blur_tex: texture_2d<f32>;

struct VOut {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn vertex(@location(0) position: vec3<f32>) -> VOut {
    var o: VOut;
    o.position = vec4<f32>(position.xy, 0.0, 1.0);
    return o;
}

@fragment
fn fragment(in: VOut) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(glow_tex));
    let uv = (in.position.xy - view.viewport.xy) / view.viewport.zw;
    let at = clamp(vec2<i32>(uv * vec2<f32>(size)), vec2<i32>(0), size - vec2<i32>(1));
    let centre = textureLoad(glow_tex, at, 0);
    // Stencil 255 (the outlined object's visible pixels): nothing added.
    if centre.a > 0.5 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(textureSampleLevel(blur_tex, glow_smp, uv, 0.0).rgb, 0.0);
}
