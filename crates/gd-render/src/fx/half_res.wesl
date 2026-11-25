// Half-resolution particles: the view depth reduced to half size, then the
// particles' premultiplied target composited onto the frame.
import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

@group(0) @binding(0) var depth_tex: texture_depth_2d;
@group(0) @binding(1) var half_tex: texture_2d<f32>;
@group(0) @binding(2) var half_smp: sampler;

// The nearest of the texel's 2 × 2 full-size depths (reverse Z: the largest),
// so a particle behind geometry stays hidden.
@fragment
fn downsample_depth(in: FullscreenVertexOutput) -> @builtin(frag_depth) f32 {
    let hi = vec2<i32>(textureDimensions(depth_tex)) - vec2<i32>(1);
    let p = vec2<i32>(in.position.xy) * 2;
    let a = textureLoad(depth_tex, min(p, hi), 0);
    let b = textureLoad(depth_tex, min(p + vec2<i32>(1, 0), hi), 0);
    let c = textureLoad(depth_tex, min(p + vec2<i32>(0, 1), hi), 0);
    let d = textureLoad(depth_tex, min(p + vec2<i32>(1, 1), hi), 0);
    return max(max(a, b), max(c, d));
}

// Bilinear upsample of rgb (premultiplied) and A (the particles' coverage),
// blended One / OneMinusSrcAlpha: rgb + (1 − A)·frame.
@fragment
fn composite(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return textureSampleLevel(half_tex, half_smp, in.uv, 0.0);
}
