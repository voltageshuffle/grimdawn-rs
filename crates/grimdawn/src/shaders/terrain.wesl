// Terrain splatting: layer 0 is the base; each further layer is blended over
// the result by its painted weight (uv = level-local grid coordinate), then
// Bevy's PBR lighting runs on the composited colour.
import bevy_pbr::{
    render::pbr_fragment::pbr_input_from_standard_material,
    render::pbr_functions::alpha_discard,
};

@if(PREPASS_PIPELINE)
import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    deferred::functions::deferred_output,
};
@else
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

struct TerrainParams {
    layer_count: u32,
    grid_size: f32,
    tile_scale: f32,
    _pad: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> terrain: TerrainParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var layer_colors: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var layer_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var layer_weights: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var weight_sampler: sampler;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let grid = in.uv;                        // level-local metres
    let wuv = grid / terrain.grid_size;      // 0..1 over the weight maps
    let tuv = grid * terrain.tile_scale;     // layer texture tiling
    var color = textureSample(layer_colors, layer_sampler, tuv, 0).rgb;
    for (var i = 1u; i < terrain.layer_count; i = i + 1u) {
        let w = textureSample(layer_weights, weight_sampler, wuv, i).r;
        let c = textureSample(layer_colors, layer_sampler, tuv, i).rgb;
        color = mix(color, c, w);
    }
    pbr_input.material.base_color = vec4(color, 1.0) * pbr_input.material.base_color;

@if(PREPASS_PIPELINE) {
    let out = deferred_output(in, pbr_input);
    return out;
}
@else {
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
}
