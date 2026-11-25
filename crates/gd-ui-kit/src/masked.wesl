import bevy_ui_render::ui_vertex_output::UiVertexOutput;

struct MaskedParams {
    color: vec4<f32>,
    mask_rect: vec4<f32>,
}

@group(1) @binding(0) var<uniform> material: MaskedParams;
@group(1) @binding(1) var tex: texture_2d<f32>;
@group(1) @binding(2) var samp: sampler;
@group(1) @binding(3) var mask: texture_2d<f32>;
@group(1) @binding(4) var mask_samp: sampler;

// sRGB encode (the UI target holds gamma values).
fn enc(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055), x * 12.92, x <= vec3<f32>(0.0031308));
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let s = textureSample(tex, samp, in.uv);
    let t = vec4<f32>(enc(s.rgb) * enc(material.color.rgb), s.a * material.color.a);
    let muv = (in.position.xy - material.mask_rect.xy) / material.mask_rect.zw;
    var m = textureSample(mask, mask_samp, clamp(muv, vec2<f32>(0.0), vec2<f32>(1.0))).a;
    if any(muv < vec2<f32>(0.0)) || any(muv > vec2<f32>(1.0)) {
        m = 0.0;
    }
    return vec4<f32>(t.rgb, t.a * m);
}
