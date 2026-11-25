import bevy_ui_render::ui_vertex_output::UiVertexOutput;

struct AdditiveParams {
    color: vec4<f32>,
}

@group(1) @binding(0) var<uniform> material: AdditiveParams;
@group(1) @binding(1) var tex: texture_2d<f32>;
@group(1) @binding(2) var samp: sampler;

// sRGB encode.
fn enc(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055), x * 12.92, x <= vec3<f32>(0.0031308));
}

// The UI target holds gamma values: shown = dst + tex · colour · alpha (blend source-alpha / one).
@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, in.uv);
    return vec4<f32>(enc(t.rgb) * enc(material.color.rgb), t.a * material.color.a);
}
