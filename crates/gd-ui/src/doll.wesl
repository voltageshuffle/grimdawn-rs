import bevy_ui_render::ui_vertex_output::UiVertexOutput;

// Scene colours are gamma-space and so is the UI target: drawn as they are, alpha kept.
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv);
}
