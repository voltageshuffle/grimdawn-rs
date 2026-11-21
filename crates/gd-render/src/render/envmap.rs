//! `effects/portalenvmapped*.ssh`: the Shattered Realm portal's starfield
//! (`endlessdungeon_portal01a.msh`, `envmapTexture` a cube map).
//!
//! DirectX11 programs (`effects/portalenvmapped.ssh`),
//! technique `emissiveStyle` (sort 6500): the VS reflects the eye ray about the
//! world normal, R = I − 2(I·N)N with I = normalize(position − cameraOrigin);
//! `emmisivePS`: rgb = cube(normalize(R)).rgb · base.rgb, a = base.a; blend
//! SrcAlpha / InvSrcAlpha, cull back (states read from the technique,
//! material.rs). PORT: drawn in the main view only (not the glow view).
use super::material::{PassStates, blend_factor};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, Face,
    RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

const SHADER: &str = "embedded://gd_render/render/shaders/gd_envmap.wesl";

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
#[bind_group_data(EnvmapKey)]
pub struct EnvmapMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub base: Handle<Image>,
    #[texture(2, dimension = "cube")]
    #[sampler(3)]
    pub envmap: Handle<Image>,
    pub states: PassStates,
}

#[repr(C)]
#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct EnvmapKey {
    states: PassStates,
}

impl From<&EnvmapMaterial> for EnvmapKey {
    fn from(m: &EnvmapMaterial) -> Self {
        Self { states: m.states }
    }
}

impl Material for EnvmapMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        if self.states.blend.is_some() {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        }
    }
    /// Sort 6500: after the particles of the portal's own effect
    /// (`particlecombine` 6000, `particleadditive` 6100), whose dark ring orbs
    /// and glows the starfield then covers where `base.a` is high.
    /// `GraphicsMTRenderer::RenderScenePasses` (fx/render.rs `ParticleMaterial::sort`);
    /// same bias scale as `ParticleMaterial::depth_bias`.
    fn depth_bias(&self) -> f32 {
        (6500 - 5900) as f32 * 10.0
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
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let s = key.bind_group_data.states;
        descriptor.primitive.cull_mode = match s.cull {
            1 => Some(Face::Front),
            2 => Some(Face::Back),
            _ => None,
        };
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_write_enabled = Some(s.depth_write);
        }
        if let (Some((src, dst)), Some(frag)) = (s.blend, descriptor.fragment.as_mut()) {
            let c = BlendComponent {
                src_factor: blend_factor(src),
                dst_factor: blend_factor(dst),
                operation: BlendOperation::Add,
            };
            let keep = BlendComponent {
                src_factor: BlendFactor::Zero,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            };
            for t in frag.targets.iter_mut().flatten() {
                t.blend = Some(BlendState {
                    color: c,
                    alpha: keep,
                });
            }
        }
        Ok(())
    }
}
