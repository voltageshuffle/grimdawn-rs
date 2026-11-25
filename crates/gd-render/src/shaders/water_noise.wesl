// The water types' NoiseTexture target (water.rs render_noise, once per frame per type with visible
// water): noisetexturegen.ssh compositePS evaluated at each texel centre of the size² target, then
// convertPS (normalMap = 1) or the raw height (normalMap = 0) written out. water.wesl / lava.wesl
// sample it bilinearly (repeat: the composite is periodic over one uv unit, so the target tiles).
import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

// The engine's 256-texture bank (water.rs noise_bank).
@group(0) @binding(0) var noise_textures: texture_2d_array<f32>;
@group(0) @binding(1) var noise_sampler: sampler;
// The type's state (water.rs noise_gpu, rewritten in place every frame): [0] phases, [1] previous bank
// slices, [2] current bank slices, [3] octave weights (levels), [4] x size, y 1 = normalMap.
@group(0) @binding(2) var<storage, read> noise: array<vec4<f32>>;

// compositePS: Σ levels[i] · mix(prev, cur, phase) sampled at uv · 2^i — the octave scales
// (1, 2, 4, 8), Engine.dll `Engine::NoiseOctaveUvScales`.
fn height(uv: vec2<f32>) -> f32 {
    var h = 0.0;
    for (var i = 0; i < 4; i = i + 1) {
        let k = exp2(f32(i));
        let p = textureSampleLevel(noise_textures, noise_sampler, uv * k, i32(noise[1][i]), 0.0).r;
        let c = textureSampleLevel(noise_textures, noise_sampler, uv * k, i32(noise[2][i]), 0.0).r;
        h = h + noise[3][i] * mix(p, c, noise[0][i]);
    }
    return h;
}

// convertPS: the height gradient over one texel (1/size), ×8, capped at 1 (basePS reads the [0,1]
// normal back as xy·2−1, i.e. the capped value). normalMap = 0: the raw height in both channels.
@fragment
fn composite(in: FullscreenVertexOutput) -> @location(0) vec2<f32> {
    let texel = 1.0 / noise[4].x;
    let uv = in.position.xy * texel;
    let h0 = height(uv);
    if noise[4].y < 0.5 {
        return vec2(h0);
    }
    let hx = height(uv + vec2(texel, 0.0));
    let hy = height(uv + vec2(0.0, texel));
    return vec2(min((h0 - hx) * 8.0, 1.0), min((h0 - hy) * 8.0, 1.0));
}
