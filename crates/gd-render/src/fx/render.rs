//! Particle billboards: per emitter one storage buffer ([`fill_buffer`]) updated
//! in place every frame (same size: one `write_buffer`, no bind group change;
//! grow-only power-of-two capacity) and a shared static quad mesh ([`quad_mesh`]),
//! expanded on the GPU by a port of the `particle*.ssh` vertex shader (see
//! `particle.wesl`). PORT: the engine rewrites a dynamic vertex buffer; a Bevy
//! `Mesh` asset rewritten each frame is freed and re-allocated by the mesh
//! allocator, a `ShaderBuffer` is not.
//!
//! Per particle (`Emitter::RenderPass`): centre, 4 corners uv
//! (0,0)(1,0)(1,1)(0,1), "dir" = (up or velocity, stretch), params = (rotation
//! rad, half-size, speed·[stretch>0.01], frame), colour; indices (2,1,0, 0,3,2).
//! Slots past the header's count are collapsed by the shader.
use super::sim::Particle;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, Extent3d,
    RenderPipelineDescriptor, SpecializedMeshPipelineError, TextureViewDescriptor,
    TextureViewDimension,
};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;
use gd_formats::pfx::Blend;

pub const SHADER: &str = "embedded://gd_render/fx/particle.wesl";

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
#[bind_group_data(ParticleKey)]
pub struct ParticleMaterial {
    /// x: `orientVertically` (bools[10]); distortion (`particledistort*`,
    /// render/distort.rs): y `screenTextureScale`, z 1 = `particledistortadditive`.
    #[uniform(0)]
    pub params: Vec4,
    /// Flipbook frames as array layers (UNORM: blended in gamma space).
    #[texture(1, dimension = "2d_array")]
    #[sampler(2)]
    pub texture: Handle<Image>,
    /// The emitter's particles (see [`fill_buffer`]); rewritten in place each frame.
    #[storage(3, read_only)]
    pub particles: Handle<ShaderBuffer>,
    pub blend: Blend,
    /// Shader sort order (`Emitter::sort_order`) and emitter index: the queue
    /// is sorted by the shader order first, then distance (`build_mesh`
    /// anchor), then the emitter's place in its effect.
    /// `GraphicsMTRenderer::RenderScenePasses`: the scene passes
    /// (sort orders 0-8999, particles included) are merge-sorted (stable,
    /// `GraphicsMTRenderer::MergeSortPasses` / comparator `GraphicsMTRenderer::ComparePasses`) by sort order,
    /// then shader, style, light and texture pointer; no distance (only the 10000-11000
    /// transparency passes are sorted far to near, `ExecutePrepareTransparency`).
    /// PORT: within one sort order the engine's draw order follows pointer
    /// values and collection order, which a port cannot reproduce; back to
    /// front stands in.
    pub sort: u32,
    pub order: u32,
}

#[repr(C)]
#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct ParticleKey {
    /// 0 additive, 1 alpha, 2 distortion.
    blend: u8,
}

impl From<&ParticleMaterial> for ParticleKey {
    fn from(m: &ParticleMaterial) -> Self {
        Self {
            blend: match m.blend {
                Blend::Additive => 0,
                Blend::Alpha => 1,
                Blend::Distort => 2,
            },
        }
    }
}

impl Material for ParticleMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
    /// Distortion reads the frame copy and is drawn after the scene
    /// (render/distort.rs).
    fn reads_view_transmission_texture(&self) -> bool {
        self.blend == Blend::Distort
    }
    fn depth_bias(&self) -> f32 {
        // Distances are view-space metres (< far plane 250): 10 m per sort
        // step (100 between lit/combine/additive) dominates any distance.
        (self.sort.saturating_sub(5900)) as f32 * 10.0 + self.order as f32 * 0.01
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vl = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
        ])?;
        descriptor.vertex.buffers = vec![vl];
        descriptor.primitive.cull_mode = None;
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_write_enabled = Some(false);
        }
        // particleadditive: SrcAlpha/One; particlecombine/lit: SrcAlpha/InvSrcAlpha;
        // particledistort*: One/Zero, alpha write off (postRenderStyle states).
        let (src, dst) = match key.bind_group_data.blend {
            0 => (BlendFactor::SrcAlpha, BlendFactor::One),
            1 => (BlendFactor::SrcAlpha, BlendFactor::OneMinusSrcAlpha),
            _ => (BlendFactor::One, BlendFactor::Zero),
        };
        let c = BlendComponent {
            src_factor: src,
            dst_factor: dst,
            operation: BlendOperation::Add,
        };
        let distort = key.bind_group_data.blend == 2;
        let alpha = if distort {
            BlendComponent {
                src_factor: BlendFactor::Zero,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            }
        } else {
            c
        };
        if let Some(frag) = descriptor.fragment.as_mut() {
            for t in frag.targets.iter_mut().flatten() {
                t.blend = Some(BlendState { color: c, alpha });
            }
            if distort {
                frag.shader_defs.push("GD_DISTORT".into());
            }
        }
        Ok(())
    }
}

/// vec4s per particle in the storage buffer.
const VEC4S_PER_PARTICLE: usize = 4;

/// Smallest particle capacity of an emitter's quad mesh / buffer.
pub const MIN_CAPACITY: usize = 16;

/// Size in bytes of an emitter buffer holding `cap` particles (header vec4 + 4 vec4s each).
pub fn buffer_bytes(cap: usize) -> u64 {
    ((1 + cap * VEC4S_PER_PARTICLE) * 16) as u64
}

/// Static quad mesh for `cap` particles: the vertex shader reads the particle
/// from the emitter's storage buffer, so the mesh never changes. POSITION is
/// all zero (the mesh's AABB centre, which Bevy sorts transparent meshes by, is
/// the entity origin = the effect origin) and UV_0 = (particle index, corner
/// 0..3 = (0,0)(1,0)(1,1)(0,1)); indices (2,1,0, 0,3,2) as in the engine.
pub fn quad_mesh(cap: usize) -> Mesh {
    let mut uv = Vec::with_capacity(cap * 4);
    let mut idx = Vec::with_capacity(cap * 6);
    for i in 0..cap {
        for corner in 0..4 {
            uv.push([i as f32, corner as f32]);
        }
        let b = (i * 4) as u32;
        idx.extend_from_slice(&[b + 2, b + 1, b, b, b + 3, b + 2]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; cap * 4])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
    .with_inserted_indices(Indices::U32(idx))
}

/// Fills `out` with the emitter buffer contents: a header vec4 (x = particle
/// count as u32 bits), then per particle (positions relative to `anchor`, the
/// effect origin in the emitter's frame, which is the draw entity's origin):
/// [centre, half-size], [dir (up or velocity), stretch], [rotation rad,
/// speed·[stretch>0.01], frame, 0], [colour]. `velocity_aligned` = bools[2]:
/// D is the velocity and the rotation is dropped (`Emitter::RenderPass`).
pub fn fill_buffer(
    particles: &[Particle],
    velocity_aligned: bool,
    anchor: Vec3,
    out: &mut Vec<[f32; 4]>,
) {
    out.clear();
    out.reserve(1 + particles.len() * VEC4S_PER_PARTICLE);
    out.push([f32::from_bits(particles.len() as u32), 0.0, 0.0, 0.0]);
    for p in particles {
        let speed = p.vel.length();
        let (d, rot) = if velocity_aligned {
            (p.vel, 0.0)
        } else {
            (Vec3::Y, p.rotation.to_radians())
        };
        let c = p.pos - anchor;
        out.push([c.x, c.y, c.z, p.size]);
        out.push([d.x, d.y, d.z, p.stretch]);
        out.push([
            rot,
            if p.stretch > 0.01 { speed } else { 0.0 },
            p.frame as f32,
            0.0,
        ]);
        out.push(p.color.to_array());
    }
}

/// Half extents of an origin-centred box holding every particle's quad, in the
/// draw entity's local space (positions relative to `anchor`, as in [`fill_buffer`]).
/// Centred on the origin so the transparent sort centre stays the effect origin
/// (see [`quad_mesh`]). Each quad is bounded by a sphere of `size · (2 + stretch·speed)`
/// (the shader's corner reach for any facing: `U`, `V` unit, `V` scaled by
/// `1 + stretch·speed`), padded 25 %.
pub fn half_extents(particles: &[Particle], anchor: Vec3) -> Vec3 {
    let mut ext = Vec3::ZERO;
    for p in particles {
        let stretch = if p.stretch > 0.01 {
            p.stretch * p.vel.length()
        } else {
            0.0
        };
        let reach = p.size.abs() * (2.0 + stretch.abs()) * 1.25;
        ext = ext.max((p.pos - anchor).abs() + reach);
    }
    ext
}

/// `.tex` → 2D array image, one layer per flipbook frame. Frames that do not
/// match frame 0's size/format are dropped.
pub fn flipbook_image(tex: &gd_formats::tex::Tex) -> anyhow::Result<(Image, u32)> {
    let first = crate::textures::dds_to_image(&tex.frames[0], false)?;
    let mut data = first.data.clone().unwrap_or_default();
    let mut layers = 1;
    for f in &tex.frames[1..] {
        let img = crate::textures::dds_to_image(f, false)?;
        if img.texture_descriptor.size != first.texture_descriptor.size
            || img.texture_descriptor.format != first.texture_descriptor.format
            || img.texture_descriptor.mip_level_count != first.texture_descriptor.mip_level_count
        {
            break;
        }
        data.extend_from_slice(img.data.as_deref().unwrap_or_default());
        layers += 1;
    }
    let size = Extent3d {
        depth_or_array_layers: layers,
        ..first.texture_descriptor.size
    };
    let mut img = crate::textures::raw_image(size, first.texture_descriptor.format, data);
    img.data_order = first.data_order;
    img.texture_descriptor.mip_level_count = first.texture_descriptor.mip_level_count;
    img.sampler = first.sampler.clone();
    img.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    Ok((img, layers))
}
