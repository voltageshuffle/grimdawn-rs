import bevy_ui_render::ui_vertex_output::UiVertexOutput;

// The paper doll's image (doll.rs): the scene shaders write gamma-space colours
// (as the engine's UNORM targets hold them) and the UI target holds gamma values
// too (render/post.rs, "UI"), so they are drawn as they are. Alpha is kept: the
// view clears to transparent and shows the window behind the doll.
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv);
}
