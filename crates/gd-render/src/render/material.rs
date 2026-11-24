//! Mesh materials driven by the `.ssh` techniques.
//!
//! The deferred renderer draws a mesh with its shader's `deferred` technique
//! (G-buffer write, then shared lighting passes) or, if it has none, with its
//! forward-only style (`emissiveStyle` for the additive/multiply scroll FX,
//! `postObjectRenderStyle` / `postRenderStyle` for screen distortion). Blend,
//! cull and depth-write states are read from that technique's pass, so
//! families are not guessed from names. Two families get their own material:
//! a shader with a `screenTexture` parameter is a distortion
//! ([`super::distort::DistortMaterial`]; the tint family's pixel program reads
//! `tint`; the colour-scroll swap is in the program only, so `colorscroll` in
//! the name selects it), an `emissiveStyle` one with an `envmapTexture` the
//! portal's cube-map look ([`super::envmap::EnvmapMaterial`]).
//!
//! A model's materials are built with the model (`model.rs` `build_model`, [`MaterialAssets::msh_material`]):
//! each part spawns with its [`PartMaterial`]. An entity record's overrides (shader, glow, textures) are
//! variants of a model material, made once and kept while it lives ([`MaterialCache`]).
use super::GdDefaults;
use super::distort::{DistortMaterial, DistortParams};
use super::envmap::EnvmapMaterial;
use crate::textures::Textures;
use bevy::asset::UntypedAssetId;
use bevy::ecs::system::SystemParam;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, ColorWrites,
    CompareFunction, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::{ShaderDefVal, ShaderRef};
use gd_formats::collections::HashMap;
use gd_formats::msh::{Material as MshMaterial, ParamValue};
use gd_formats::ssh::{Profile, Ssh, Value};
use gd_game_core::data::GameData;

const SHADER: &str = "embedded://gd_render/render/shaders/gd_standard.wesl";
const PREPASS_SHADER: &str = "embedded://gd_render/render/shaders/gd_prepass.wesl";
const FORWARD_SHADER: &str = "embedded://gd_render/render/shaders/gd_forward.wesl";
/// The foliage wind vertex program ([`Wind`]), swapped in by [`GdMaterial::specialize`].
pub const WIND_SHADER: Handle<Shader> =
    bevy::asset::uuid_handle!("6f4c1d2e-8a37-4b55-9e0c-3d7a1b2c4e91");

pub const KIND_LIT: u32 = 0;
pub const KIND_EMISSIVE: u32 = 1;
pub const KIND_MULTIPLY: u32 = 2;

pub const FLAG_GLOW: u32 = 1;
pub const FLAG_GLOW_SCROLL: u32 = 2;
pub const FLAG_DETAIL: u32 = 4;
pub const FLAG_SNOW: u32 = 8;
pub const FLAG_ALPHA_TEST: u32 = 16;
pub const FLAG_HAS_TANGENTS: u32 = 32;
pub const FLAG_GLOW_SCROLL_ALPHA: u32 = 64;
/// A mesh being (un)dissolved: the `standarddissolve*` override shader (`fade.z` = `dissolveValue`, the
/// dissolve texture in the detail slot; grimdawn `monsters.rs`).
pub const FLAG_DISSOLVE: u32 = 128;
/// The `deferred` pass writes the material target's x (the lit flag) as 0: deferred.ssh
/// `ambientPS` / `directionalAmbientPS` then output the albedo as is (no ambient, no sun), `pointPS`
/// still adds its light with the normal target's decoded (−1, −1, −1). `albedo.ssh` (the NPC marks'
/// gems, `npc_marks.rs`) and `effects/skybox*.ssh` do it: their deferredPS writes the texel, then
/// (0, 0, 0, 1) to the normal, material and specular targets. Recognised by [`unlit_pixel_program`].
pub const FLAG_UNLIT: u32 = 256;
/// The `icey*.ssh` override shader of a frozen / petrified character (grimdawn `icy.rs`):
/// iceyskinned.ssh `deferredPS`: the albedo is `lerp(base, iceyTexture, iceyFactor)` after the base alpha test
/// (the icy texture in the detail slot, `fade.w` = `iceyFactor`).
pub const FLAG_ICEY: u32 = 512;
/// The `deferred` pixel program reads the vertex colour (its input signature has a `COLOR`, [`ShaderInfo::vertex_color`])
/// and the mesh has colours: standardvertexcolorstatic.ssh `deferredPS` (value 43): the albedo is
/// `base · diffuseColor · vcol.rgb`, the alpha (tested against 0.5) `base.a · vcol.a` (~107 stock meshes, e.g. gdx1
/// `voidabyss/void_tarcone*.msh`, `focus_032a_01.msh`); the same in the glow / scroll / blended `*vertexcolor*`
/// programs (the blended ones test `base.a · vcol.a · blendedGlobalFadeFactor` against 0.1).
/// standardstaticsnowvertcolor.ssh `deferredPS`: the tint applies after the snow blend (12 gdx3 granite rocks).
///
/// The other programs with a `COLOR` input in stock data:
/// - `standardblendedvertexalpha*` ([`FLAG_VERTEX_ALPHA`]);
/// - `*glacial*` ([`FLAG_GLACIAL`]): the colour is the vertex program's height gradient, not a mesh colour;
///   the `*glacialvertcolor*` ones also tint like this flag (no stock mesh uses them);
/// - `bonefadeskinned.ssh` ([`FLAG_BONE_FADE`]): its `deferredVS` writes rgb 1 and
///   alpha `boneFadeFactor[first bone]`, `deferredPS` drops alpha < 0.01 and blends `base.a · a`
///   (SrcAlpha / OneMinusSrcAlpha). The factor is `GraphicsMeshInstance::SetBoneTransparancies`
///   (Engine.dll, bound in `SetShaderParameters`), which only `Destructible` sets
///   (Game.dll `Destructible::UpdateSelf`: after `rubbleLife`, every piece bone the same
///   `remaining / rubbleFade`; the bone bitset it also writes has no writer in the binaries).
///   Without it the factor is 1. Stock: the harpy gibs and the breakables' `*_break` meshes;
///   the other gibs are `standardblendedstatic` and fade through `blendedGlobalFadeFactor`
///   (`Gib::UpdateSelf` → `BeginFade`, grimdawn `gibs.rs`). `fade.x` carries the factor here.
/// - the decal shaders (`effects/decal_*.ssh`, `fxdecal.ssh`): `decal.rs`.
pub const FLAG_VERTEX_COLOR: u32 = 1024;
/// standardstaticglacial.ssh (= `_nofade`, `standardskinnedglacial`; the `t` variant also × the
/// see-through `transparencyValue.z`) `deferredVS` / `deferredPS`, ice:
/// - vertex colour g = lerp(bottomColor, 1, sat((world.y − object origin.y) / meshHeight));
///   v = the view vector in the (T, −B, N) frame, normalised; n = the bump normal (2·texel − 1);
/// - d = max(n·v, 0); k = 1 − min((1 − d)^rimLight, 1); A = base · (2·diffuseColor)² · g;
///   B = lerp(A, lerp(topColor, A, k), k · fresnelAmount); C = lerp(topColor, innerColor, 1 − base.a);
///   D = (1 − base.a) · lerp(C, A, 1 − d²); m = materialTexture.a at uv + (v.x · sssParams, v.z · sssParams.y);
///   albedo = lerp(B, lerp(D·m, B, base.a), d² · fresnelAmount);
///   specular = specularColor · bump.a + min(fresnelAmount, 0.25) · k · topColor; no alpha test.
/// - standardstaticglacialsnow: the snow blend first (`FLAG_SNOW`, without the fresnel step of the snow colour),
///   A without the snow, then albedo = lerp(ice, (2·diffuseColor)² · snowed base, s), s the vertex's snow factor.
///
/// `materialTexture` rides in the detail slot, the colours in [`GdParams`]'s `ice_*`. PORT: `sssParams` is a
/// Float in a 16-byte constant; its y (the v offset) is taken as the zero padding.
/// ~110 gdx3 meshes (ice caves, glacier cliffs).
pub const FLAG_GLACIAL: u32 = 2048;
/// standardblendedvertexalpha{skinned,glowscrollstatic}.ssh `deferredPS`: alpha = base.a · vcol.r ·
/// blendedGlobalFadeFactor, dropped below 0.1, albedo without the colour (the gdx3 blood / magma pools).
/// `blendedGlobalFadeFactor` is the instance's fade (`SetShaderParameters`: min of its two opacities), `fade.x` here.
pub const FLAG_VERTEX_ALPHA: u32 = 4096;
/// `bonefadeskinned.ssh` `deferredVS` / `deferredPS` (the [`FLAG_VERTEX_COLOR`] doc):
/// alpha = `base.a · boneFadeFactor`, dropped below 0.01, rgb untinted. `fade.x` is the factor
/// (the breakables' rubble fade, grimdawn `breakables.rs`; 1 otherwise). The engine binds it per
/// bone, but `Destructible` sets every bone the same value, so one per-instance value is exact.
pub const FLAG_BONE_FADE: u32 = 8192;
/// The `standardblended*` `deferredPS` (standardblendedstatic / skinned_nofade /
/// glowstatic DXBC): alpha = `base.a · blendedGlobalFadeFactor`, dropped below 0.1 — always,
/// not only while fading. `fade.x` is the factor (the gibs' `BeginFade`, grimdawn `gibs.rs`;
/// the see-through opacity, `see_through.rs`). The "…T.ssh" variants test 0.5 instead
/// (`see_through.rs` reshades to them) and are excluded.
pub const FLAG_BLENDED_FADE: u32 = 16384;

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct GdParams {
    /// Engine `diffuseColor`: 0.5 · instance colour (white), w = ambient add.
    pub diffuse: Vec4,
    /// specularColor rgb, specularPower w.
    pub specular: Vec4,
    /// xy: glow `scrollRate` or emissive colour scroll; zw: alpha scroll.
    pub scroll: Vec4,
    /// snowDepth, snowThreshold, snowNormals, fresnelAmount (also the ice's, [`FLAG_GLACIAL`]).
    pub snow: Vec4,
    /// x: kind, y: flags.
    pub flags: UVec4,
    /// x: `blendedGlobalFadeFactor` — the see-through scenery opacity (`see_through.rs`), the
    /// gibs' fade (`gibs.rs`), or the bone fade factor ([`FLAG_BONE_FADE`]); 1 = full.
    /// y: the alpha-test reference (0.5; the grass passes' 0.65 / 0.35), z: `dissolveValue` ([`FLAG_DISSOLVE`]).
    pub fade: Vec4,
    /// w: > 0 = this is an outline mask copy ([`GdMaterial::outline_mask`]), its palette class (1, 2, 3).
    pub outline: Vec4,
    /// [`WindParams`]: foliageSpeed, foliageAmplitude, 1 = add the object
    /// offset, [`Wind::mode`].
    pub wind: Vec4,
    /// x/y/zRippleScale.
    pub ripple_scale: Vec4,
    /// x/y/zRippleAmplitude.
    pub ripple_amplitude: Vec4,
    /// [`FLAG_GLACIAL`]: innerColor.
    pub ice_inner: Vec4,
    /// [`FLAG_GLACIAL`]: topColor.
    pub ice_top: Vec4,
    /// [`FLAG_GLACIAL`]: bottomColor rgb, meshHeight w.
    pub ice_bottom: Vec4,
    /// [`FLAG_GLACIAL`]: rimLight, sssParams.
    pub ice: Vec4,
    /// [`FLAG_DISSOLVE`]: rgb the `dissolveColor` (0..1) of the `standarddissolve*` `emissiveStyle` edge
    /// (black for the ambush's un-dissolve).
    pub dissolve: Vec4,
}

impl Default for GdParams {
    /// A lit opaque material with no textured extras: diffuse 0.5, specular power 1, opacity 1, test reference 0.5.
    fn default() -> Self {
        Self {
            diffuse: Vec4::new(0.5, 0.5, 0.5, 0.0),
            specular: Vec4::new(0.0, 0.0, 0.0, 1.0),
            scroll: Vec4::ZERO,
            snow: Vec4::ZERO,
            flags: UVec4::ZERO,
            fade: Vec4::new(1.0, 0.5, 1.0, 1.0),
            outline: Vec4::ZERO,
            wind: Vec4::ZERO,
            ripple_scale: Vec4::ZERO,
            ripple_amplitude: Vec4::ZERO,
            ice_inner: Vec4::ZERO,
            ice_top: Vec4::ZERO,
            ice_bottom: Vec4::ZERO,
            ice: Vec4::ZERO,
            dissolve: Vec4::ZERO,
        }
    }
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
#[bind_group_data(GdMaterialKey)]
pub struct GdMaterial {
    #[uniform(0)]
    pub params: GdParams,
    #[texture(1)]
    #[sampler(2)]
    pub base: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    pub bump: Handle<Image>,
    #[texture(5)]
    #[sampler(6)]
    pub glow: Handle<Image>,
    #[texture(7)]
    #[sampler(8)]
    pub detail: Handle<Image>,
    #[texture(9)]
    #[sampler(10)]
    pub snow_base: Handle<Image>,
    #[texture(11)]
    #[sampler(12)]
    pub snow_normal: Handle<Image>,
    #[storage(13, read_only)]
    pub env: Handle<ShaderBuffer>,
    /// Every loaded terrain's grass offset field (render/grass.rs `GrassOffsets`): read by the grass bend only.
    #[storage(14, read_only)]
    pub grass: Handle<ShaderBuffer>,
    pub states: PassStates,
}

/// Fixed-function state of the technique pass the engine would use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PassStates {
    /// Game blend factors; `None` = blending off.
    pub blend: Option<(u8, u8)>,
    /// 0 none, 1 front, 2 back (game winding).
    pub cull: u8,
    pub depth_write: bool,
    /// The 0.5 cut on base alpha.
    pub alpha_test: bool,
    /// `deferredOutlineStyle` ([`GdMaterial::outline_mask`]).
    pub outline: bool,
    /// Depth test `Less` (DepthFunc 2) instead of `LessEqual`: the grass
    /// fringe pass (render/grass.rs) skips the pixels its base pass drew.
    pub depth_strict: bool,
    /// One of the two forward passes of a faded scenery part (`see_through.rs`).
    pub fade: Option<FadePass>,
}

/// The "…T.ssh" passes the engine's transparency list draws a faded section
/// with (`see_through.rs` module doc), after the lit scene, far to near.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FadePass {
    /// `depthStyle`: depth only, texels with base alpha ≥ 0.5.
    Depth,
    /// `dLightDiffuseAmbientStyle` (+ the additive point-light styles): lit
    /// colour at alpha = opacity, SrcAlpha / OneMinusSrcAlpha, on the depth
    /// the [`FadePass::Depth`] pass left, no depth write.
    Colour,
}

impl Default for PassStates {
    fn default() -> Self {
        Self {
            blend: None,
            cull: 2,
            depth_write: true,
            alpha_test: true,
            outline: false,
            depth_strict: false,
            fade: None,
        }
    }
}

impl PassStates {
    /// Bevy's phase for the pass. The see-through passes draw in the
    /// transparent phase, after every opaque and alpha-masked mesh, like the
    /// engine's transparency list after the deferred scene.
    pub fn alpha_mode(&self) -> AlphaMode {
        if self.fade.is_some() || self.blend.is_some() || self.outline {
            AlphaMode::Blend
        } else if self.alpha_test {
            AlphaMode::Mask(0.5)
        } else {
            AlphaMode::Opaque
        }
    }
}

#[repr(C)]
#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct GdMaterialKey {
    states: PassStates,
    /// Draws with [`WIND_SHADER`].
    wind: bool,
}

impl GdMaterial {
    /// This material's `deferredOutlineStyle` pass (post.rs,
    /// standard*.ssh): same mesh and base texture, cull none, no depth
    /// writes, the depth test of the scene; every lit shader has it, blended
    /// ones too, the additive / multiply FX shaders do not (`None`). `class`: the outline colour's palette entry
    /// (1, 2, 3; `render::post::OutlineGlow`).
    pub fn outline_mask(&self, class: u8) -> Option<Self> {
        (self.params.flags.x == KIND_LIT).then(|| {
            let mut mask = self.clone();
            mask.params.outline = Vec4::W * class as f32;
            mask.states = PassStates {
                blend: None,
                cull: 0,
                depth_write: false,
                alpha_test: false,
                outline: true,
                depth_strict: false,
                fade: None,
            };
            mask
        })
    }
}

impl From<&GdMaterial> for GdMaterialKey {
    fn from(m: &GdMaterial) -> Self {
        Self {
            states: m.states,
            wind: m.params.wind.y != 0.0,
        }
    }
}

/// The pass's depth write where Bevy's default for the alpha mode differs:
/// off when the pass turns it off, and on for a blended pass that keeps the
/// default (Bevy's `AlphaMode::Blend` never writes depth).
/// standardblendedskinned.ssh `deferred`: SrcAlpha / OneMinusSrcAlpha with no
/// DepthWrite state (the D3D default, on) and DepthFunc LessEqual, so a
/// blended mesh hides its own far side; without it the inner lining of
/// Mazaan's hood (`head_055_01b`) drew over the swirl of its outer shell.
fn depth_write_of(s: &PassStates) -> Option<bool> {
    match s.fade {
        Some(FadePass::Depth) => Some(true),
        Some(FadePass::Colour) => Some(false),
        None if !s.depth_write => Some(false),
        None if s.blend.is_some() => Some(true),
        None => None,
    }
}

/// Sort bias of a faded part's depth pass (view-space metres): drawn just
/// before its colour pass, which shares its mesh centre (`see_through.rs`).
const FADE_DEPTH_FIRST: f32 = 1e-3;

pub(crate) fn blend_factor(v: u8) -> BlendFactor {
    match v {
        0 => BlendFactor::One,
        2 => BlendFactor::Dst,
        3 => BlendFactor::Src,
        5 => BlendFactor::DstAlpha,
        6 => BlendFactor::SrcAlpha,
        7 => BlendFactor::OneMinusDstAlpha,
        8 => BlendFactor::OneMinusSrcAlpha,
        9 => BlendFactor::OneMinusDst,
        10 => BlendFactor::OneMinusSrc,
        11 => BlendFactor::Constant,
        12 => BlendFactor::OneMinusConstant,
        _ => BlendFactor::Zero,
    }
}

impl Material for GdMaterial {
    /// The forward vertex: Bevy's mesh vertex with the engine's first-bone
    /// skinned normal/tangent frame (gd_forward.wesl).
    fn vertex_shader() -> ShaderRef {
        FORWARD_SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    /// The shadow passes: Bevy's prepass vertex plus the point-light
    /// paraboloid (render/point_shadow.rs).
    fn prepass_vertex_shader() -> ShaderRef {
        PREPASS_SHADER.into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        self.states.alpha_mode()
    }
    fn depth_bias(&self) -> f32 {
        if self.states.fade == Some(FadePass::Depth) {
            -FADE_DEPTH_FIRST
        } else {
            0.0
        }
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let s = key.bind_group_data.states;
        if key.bind_group_data.wind {
            descriptor.vertex.shader = WIND_SHADER;
        } else if let Some(frag) = descriptor.fragment.as_mut() {
            // The mesh tag is a character's light group and highlight (render/light_rig.rs); on the wind
            // pipeline it is the grass wind (render/grass.rs).
            frag.shader_defs.push("GD_TAGGED".into());
        }
        // Indices are reversed on load, so game "back" is Bevy back.
        descriptor.primitive.cull_mode = match s.cull {
            1 => Some(Face::Front),
            2 => Some(Face::Back),
            _ => None,
        };
        // The shadow / prepass pipelines write depth whatever the colour pass does: the T shaders' and the blended
        // shaders' `shadow` techniques write the caster's depth (`see_through.rs`), so a faded part keeps its shadow.
        let prepass = descriptor
            .vertex
            .shader_defs
            .iter()
            .any(|d| matches!(d, ShaderDefVal::Bool(n, true) if n == "PREPASS_PIPELINE"));
        if let Some(ds) = descriptor.depth_stencil.as_mut()
            && !prepass
        {
            if let Some(w) = depth_write_of(&s) {
                ds.depth_write_enabled = Some(w);
            }
            if s.depth_strict {
                // Reverse Z: the engine's `Less` is `Greater`.
                ds.depth_compare = Some(CompareFunction::Greater);
            }
        }
        if let (Some((src, dst)), Some(frag)) = (s.blend, descriptor.fragment.as_mut()) {
            let c = BlendComponent {
                src_factor: blend_factor(src),
                dst_factor: blend_factor(dst),
                operation: BlendOperation::Add,
            };
            // Target alpha is kept: in the glow view it holds the outline
            // mask (post.rs), which depth-writing passes clear and the
            // masks write; blended passes leave it alone.
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
            frag.shader_defs.push("GD_BLENDED".into());
        }
        if let Some(frag) = descriptor.fragment.as_mut() {
            match s.fade {
                Some(FadePass::Depth) => {
                    for t in frag.targets.iter_mut().flatten() {
                        t.write_mask = ColorWrites::empty();
                    }
                    frag.shader_defs.push("GD_FADE_DEPTH".into());
                }
                Some(FadePass::Colour) => frag.shader_defs.push("GD_FADE_COLOUR".into()),
                None => {}
            }
        }
        if s.outline
            && let Some(frag) = descriptor.fragment.as_mut()
        {
            // Only the target's alpha, the mask (post.rs); the glow stays.
            let keep = BlendComponent {
                src_factor: BlendFactor::Zero,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            };
            let replace = BlendComponent {
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::Zero,
                operation: BlendOperation::Add,
            };
            for t in frag.targets.iter_mut().flatten() {
                t.blend = Some(BlendState {
                    color: keep,
                    alpha: replace,
                });
            }
        }
        Ok(())
    }
}

/// What the engine would do with one `.ssh` file.
#[derive(Clone, Debug, Default)]
pub struct ShaderInfo {
    pub kind: u32,
    pub states: PassStates,
    pub special: Special,
    /// The foliage wind of the `*wiggle*` / `*wiggly*` / `*foliage*` vertex
    /// programs ([`Wind`]).
    pub wind: Option<Wind>,
    /// The shader has a `glowTexture` parameter: parameters are bound by
    /// name (Engine.dll `GraphicsMeshInstance::SetShaderParameters`),
    /// so a glow texture of a shader without one does not draw.
    pub glow: bool,
    /// [`FLAG_UNLIT`].
    pub unlit: bool,
    /// The lit `deferred` pixel program tints by the vertex colour ([`FLAG_VERTEX_COLOR`]).
    pub vertex_color: bool,
    /// Its alpha is `base.a · vcol.r` ([`FLAG_VERTEX_ALPHA`]).
    pub vertex_alpha: bool,
    /// The bone fade program ([`FLAG_BONE_FADE`]).
    pub bone_fade: bool,
    /// The `standardblended*` 0.1 fade test ([`FLAG_BLENDED_FADE`]).
    pub blended_fade: bool,
    /// The ice program ([`FLAG_GLACIAL`]).
    pub glacial: bool,
}

/// A shader's foliage wind parameters and their `.ssh` defaults.
///
/// Per the DirectX11 vertex programs of every `.ssh` with a `foliageSpeed`,
/// every pass: the position p moves by `d · c.rgb · c.a · 0.1 · foliageAmplitude
/// · rippleAmplitude` with c the vertex colour (B8G8R8A8, Direct3D11.dll
/// `Direct3DVertexDeclaration::Direct3DVertexDeclaration`),
/// `a_i = (p_i + o_i)·rippleScale_i + time·foliageSpeed`, `s = sin a`,
/// `d = (s_y·s_z, s_z·s_x, s_x·s_y)`; additivescrollwiggly{static,skinned} take
/// `d = (s_y·k_z, s_z·k_x, s_x·k_y)`, `k = sin(a + 1.57)`. The shaders without
/// `*RippleAmplitude` (additive / multiply / distort families) have none (1).
/// o: `objectToRegionOffset` (standard "wiggle" / "foliage" shaders), the
/// `objectToWorldMatrix` translation (additive / multiply / distort "wiggle"),
/// none in every "wiggly" one. Skinned: "wiggle" programs move the skinned
/// position (object space), "wiggly" ones the bind-pose one before skinning.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wind {
    /// foliageSpeed, foliageAmplitude, x/y/zRippleScale, x/y/zRippleAmplitude.
    pub defaults: [f32; 8],
    /// Adds the object offset o.
    pub offset: bool,
    /// The sin·cos form.
    pub cos: bool,
    /// Moves the bind-pose position (skinned meshes).
    pub pre_skin: bool,
    /// The shader has the `*RippleAmplitude` parameters (else they are 1;
    /// parameters bind by name, so a material's own values are unused).
    pub amplitude: bool,
}

/// [`Wind::mode`] bit of the grass bend (render/grass.rs).
pub const WIND_GRASS: f32 = 4.0;

impl Wind {
    /// `GdParams::wind.w` / `DistortParams::wind.w` (gd_wind.wesl).
    pub fn mode(&self) -> f32 {
        (self.cos as u32 | (self.pre_skin as u32) << 1) as f32
    }
}

/// A material's wind uniforms (the same in [`GdParams`] and
/// [`DistortParams`]; all zero = no wind).
#[derive(Clone, Copy, Debug, Default)]
pub struct WindParams {
    /// foliageSpeed, foliageAmplitude, 1 = add the object offset, [`Wind::mode`].
    pub wind: Vec4,
    pub ripple_scale: Vec4,
    pub ripple_amplitude: Vec4,
}

pub const WIND_PARAMS: [&str; 8] = [
    "foliageSpeed",
    "foliageAmplitude",
    "xRippleScale",
    "yRippleScale",
    "zRippleScale",
    "xRippleAmplitude",
    "yRippleAmplitude",
    "zRippleAmplitude",
];

fn wind_of(p: &Profile, path: &str) -> Option<Wind> {
    let default = |n: &str| {
        let par = p.params.iter().find(|par| par.name == n)?;
        match par.default.and_then(|i| p.values.get(i as usize)) {
            Some(Value::Float(f)) => Some(*f),
            _ => None,
        }
    };
    let has = |n: &str| p.params.iter().any(|par| par.name == n);
    let wiggly = path.contains("wiggly");
    let amplitude = has("xRippleAmplitude");
    has("foliageSpeed").then(|| Wind {
        defaults: std::array::from_fn(|i| {
            default(WIND_PARAMS[i]).unwrap_or(if i >= 5 && !amplitude { 1.0 } else { 0.0 })
        }),
        offset: !wiggly,
        cos: wiggly && !amplitude,
        pre_skin: wiggly,
        amplitude,
    })
}

/// Families drawn by their own material (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Special {
    #[default]
    None,
    /// `distort*` (render/distort.rs): `additive` = no `tint` in the pixel
    /// program, `color_scroll` = `*colorscroll*`, `post_object` = drawn by
    /// `postObjectRenderStyle` (else `postRenderStyle`).
    Distort {
        additive: bool,
        color_scroll: bool,
        post_object: bool,
    },
    /// Never drawn: no technique the deferred renderer collects
    /// (`GraphicsDeferredRenderer::ExecutePrepareScene`:
    /// `deferredTerrain`, `deferred`, `emissiveStyle`, `deferredLit`,
    /// `deferredStencilStyle`; then `RenderPostObjects`' two post styles and
    /// `deferredOutlineStyle`): `distortstandard*`.
    Hidden,
    /// `portalenvmapped*` (render/envmap.rs).
    Envmap,
}

/// Whether the pass's pixel program has a constant bound to parameter `name`.
fn ps_reads(p: &Profile, pass: &gd_formats::ssh::Pass, name: &str) -> bool {
    let Some(s) = pass.states.iter().find(|s| s.state == 2 && !s.literal) else {
        return false;
    };
    let Some(Value::PixelShader(ps)) = p
        .params
        .get(s.value as usize)
        .and_then(|par| par.default)
        .and_then(|i| p.values.get(i as usize))
    else {
        return false;
    };
    ps.bindings.iter().any(|b| {
        p.params
            .get(b.param as usize)
            .is_some_and(|par| par.name == name)
    })
}

/// The pass's pixel program (`deferredPS` & co.), parsed.
fn pixel_program(p: &Profile, pass: &gd_formats::ssh::Pass) -> Option<gd_formats::ssh::Dxbc> {
    let s = pass.states.iter().find(|s| s.state == 2 && !s.literal)?;
    let Some(Value::PixelShader(ps)) = p
        .params
        .get(s.value as usize)
        .and_then(|par| par.default)
        .and_then(|i| p.values.get(i as usize))
    else {
        return None;
    };
    gd_formats::ssh::Dxbc::parse(&ps.bytecode).ok()
}

/// [`FLAG_UNLIT`]: the pass's pixel program reads nothing but its texture coordinates (no
/// interpolated normal or tangent frame), so it cannot write a surface normal. Checked against the
/// disassembly of every stock `deferredPS`: the shaders with that input signature are exactly the
/// ones that write the lit flag 0 (`albedo.ssh`, `effects/skybox.ssh`, `effects/skyboxscroll.ssh`).
fn unlit_pixel_program(p: &Profile, pass: &gd_formats::ssh::Pass) -> bool {
    pixel_program(p, pass).is_some_and(|d| {
        let mut user = d.inputs.iter().filter(|e| e.system_value == 0);
        user.next()
            .is_some_and(|e| e.semantic.eq_ignore_ascii_case("TEXCOORD"))
            && user.next().is_none()
    })
}

/// [`FLAG_VERTEX_COLOR`]: the pass's pixel program takes the vertex colour (a `COLOR` input).
fn vertex_color_pixel_program(p: &Profile, pass: &gd_formats::ssh::Pass) -> bool {
    pixel_program(p, pass).is_some_and(|d| {
        d.inputs
            .iter()
            .any(|e| e.system_value == 0 && e.semantic.eq_ignore_ascii_case("COLOR"))
    })
}

fn int_state(p: &Profile, pass: &gd_formats::ssh::Pass, state: u32) -> Option<i32> {
    let s = pass
        .states
        .iter()
        .find(|s| s.state == state && s.index == 0)?;
    match p.state_value(s)? {
        Value::Int(i) => Some(*i),
        Value::Bool(b) => Some(*b as i32),
        Value::Float(f) => Some(*f as i32),
        _ => None,
    }
}

fn ps_param_name<'a>(p: &'a Profile, pass: &gd_formats::ssh::Pass) -> Option<&'a str> {
    let s = pass.states.iter().find(|s| s.state == 2 && !s.literal)?;
    p.params.get(s.value as usize).map(|p| p.name.as_str())
}

fn analyse(ssh: &Ssh, path: &str) -> Option<ShaderInfo> {
    let mut info = analyse_passes(ssh, path)?;
    info.wind = ssh.profile("DirectX11").and_then(|p| wind_of(p, path));
    info.glow = ssh
        .profile("DirectX11")
        .is_some_and(|p| p.params.iter().any(|par| par.name == "glowTexture"));
    Some(info)
}

fn analyse_passes(ssh: &Ssh, path: &str) -> Option<ShaderInfo> {
    let p = ssh.profile("DirectX11")?;
    let tech = |n: &str| p.techniques.iter().find(|t| t.name.eq_ignore_ascii_case(n));
    let has_param = |n: &str| p.params.iter().any(|par| par.name == n);
    let states_of = |pass: &gd_formats::ssh::Pass, alpha_test: bool| {
        let src = int_state(p, pass, 3);
        let dst = int_state(p, pass, 4);
        let blend = match (src, dst) {
            (Some(s), Some(d)) if !(s == 0 && d == 1) => Some((s as u8, d as u8)),
            _ => None,
        };
        PassStates {
            blend,
            cull: int_state(p, pass, 18)
                .map_or(2, |c| if (1..=2).contains(&c) { c as u8 } else { 0 }),
            depth_write: int_state(p, pass, 9).is_none_or(|v| v != 0),
            alpha_test: alpha_test && blend.is_none(),
            outline: false,
            depth_strict: false,
            fade: None,
        }
    };
    if let Some(t) = tech("deferred") {
        let pass = t.passes.first()?;
        // The programs reading a `COLOR` are told apart by family (FLAG_VERTEX_COLOR doc).
        let colour = vertex_color_pixel_program(p, pass);
        let glacial = colour && path.contains("glacial");
        let mut states = states_of(pass, true);
        // The glacial deferredPS has no discard: base.a is the ice's inner-colour mask.
        states.alpha_test &= !glacial;
        return Some(ShaderInfo {
            kind: KIND_LIT,
            states,
            special: Special::None,
            unlit: unlit_pixel_program(p, pass),
            vertex_color: colour && (path.contains("vertexcolor") || path.contains("vertcolor")),
            vertex_alpha: colour && path.contains("vertexalpha"),
            bone_fade: path.contains("bonefade"),
            // The T variants test transparencyValue against 0.5 (see_through.rs), not this.
            blended_fade: path.contains("standardblended") && !path.ends_with("t.ssh"),
            glacial,
            ..Default::default()
        });
    }
    if has_param("screenTexture") {
        // Distortion (render/distort.rs): its post technique. The
        // `distortstandard*` shaders have only forward styles
        // (`dLightDiffuseAmbientStyle`), which the deferred renderer never
        // collects: not drawn.
        let object = tech("postObjectRenderStyle");
        let Some(t) = object.or_else(|| tech("postRenderStyle")) else {
            return Some(ShaderInfo {
                special: Special::Hidden,
                ..Default::default()
            });
        };
        let pass = t.passes.first()?;
        return Some(ShaderInfo {
            kind: KIND_EMISSIVE,
            states: states_of(pass, false),
            special: Special::Distort {
                additive: !ps_reads(p, pass, "tint"),
                color_scroll: path.contains("colorscroll"),
                post_object: object.is_some(),
            },
            ..Default::default()
        });
    }
    if let Some(t) = tech("emissiveStyle") {
        let pass = t.passes.first()?;
        let kind = if ps_param_name(p, pass)
            .is_some_and(|n| n.to_ascii_lowercase().contains("multiply"))
        {
            KIND_MULTIPLY
        } else {
            KIND_EMISSIVE
        };
        return Some(ShaderInfo {
            kind,
            states: states_of(pass, false),
            special: if has_param("envmapTexture") {
                Special::Envmap
            } else {
                Special::None
            },
            ..Default::default()
        });
    }
    // Forward-only lit shaders: use the ambient pass states.
    let t = tech("dLightDiffuseAmbientStyle").or_else(|| tech("ambientLightStyle"))?;
    Some(ShaderInfo {
        kind: KIND_LIT,
        states: states_of(t.passes.first()?, true),
        special: Special::None,
        ..Default::default()
    })
}

/// The material caches: the `.ssh` analyses and grass materials by data path (emptied on a mod change,
/// `model::drop_on_mod_change`), and the variants of each model material ([`MaterialAssets::reshade`],
/// [`MaterialAssets::retexture`], [`MaterialAssets::reglow`]), dropped with it ([`evict_variants`]).
#[derive(Resource, Default)]
pub struct MaterialCache {
    /// `.ssh` analysis by normalised path ([`MaterialCache::shader_info`]).
    shaders: HashMap<String, Option<ShaderInfo>>,
    /// [`MaterialAssets::grass`] by (texture, `bladeHeight` bits, fringe pass): one per pass for every level.
    grass: HashMap<(String, u32, bool), Handle<GdMaterial>>,
    /// The variants of a material, by its asset id.
    variants: HashMap<UntypedAssetId, HashMap<Variant, PartMaterial>>,
    /// Shaders analysed in the background ([`MaterialCache::shader_ready`]), by normalised path.
    shader_loads: HashMap<String, bevy::tasks::Task<(String, Option<ShaderInfo>)>>,
}

/// One override of a model material.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Variant {
    /// [`Reshade`]: shader, glow texture (lowercase).
    Reshade(String, String),
    /// Base, bump texture.
    Retexture(String, String),
    /// Glow texture.
    Glow(String),
}

impl MaterialCache {
    /// Forgets everything (the mounted mod changed).
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// `.ssh` analysis, cached ([`analyse_shader`] on a miss).
    pub fn shader_info(
        &mut self,
        data: &GameData,
        shader: &str,
        skinned: bool,
    ) -> Option<ShaderInfo> {
        let path = gd_formats::vfs::normalize(&shader.replace('\\', "/").to_ascii_lowercase());
        if let Some(v) = self.shaders.get(&path) {
            return v.clone();
        }
        let (path, info) = analyse_shader(data, shader, skinned);
        self.shaders.insert(path, info.clone());
        info
    }

    /// Files a shader analysed elsewhere ([`analyse_shader`], a background model load), unless it is known.
    pub fn put_shader(&mut self, path: String, info: Option<ShaderInfo>) {
        self.shaders.entry(path).or_insert(info);
    }

    /// Whether `shader` is analysed (or unusable): [`MaterialCache::shader_info`] then costs nothing. Otherwise its
    /// analysis runs in the background and this is false: a spawn whose override names a new shader waits a few
    /// frames instead of parsing it on the game thread.
    pub fn shader_ready(&mut self, data: &GameData, shader: &str, skinned: bool) -> bool {
        use bevy::tasks::{block_on, futures_lite::future};
        let path = gd_formats::vfs::normalize(&shader.replace('\\', "/").to_ascii_lowercase());
        if self.shaders.contains_key(&path) {
            return true;
        }
        if let Some(task) = self.shader_loads.get_mut(&path) {
            let Some((key, info)) = block_on(future::poll_once(task)) else {
                return false;
            };
            self.shader_loads.remove(&path);
            self.shaders.entry(key).or_insert(info);
            return true;
        }
        let (shader, data) = (shader.to_owned(), data.clone());
        self.shader_loads.insert(
            path,
            bevy::tasks::AsyncComputeTaskPool::get()
                .spawn(async move { analyse_shader(&data, &shader, skinned) }),
        );
        false
    }
}

/// The `.ssh` analysis of `shader` and its cache key, on any thread ([`MaterialCache::shader_info`]'s load). The
/// engine lowercases the path and swaps static/skinned variants to match the mesh; we fall back the same way.
pub fn analyse_shader(
    data: &GameData,
    shader: &str,
    skinned: bool,
) -> (String, Option<ShaderInfo>) {
    let path = gd_formats::vfs::normalize(&shader.replace('\\', "/").to_ascii_lowercase());
    let swapped = if skinned {
        path.replace("static", "skinned")
    } else {
        path.replace("skinned", "static")
    };
    let info = [path.as_str(), swapped.as_str()].iter().find_map(|p| {
        data.vfs
            .read(p)
            .ok()
            .and_then(|b| Ssh::parse(&b).ok())
            .and_then(|s| analyse(&s, &path))
    });
    if info.is_none() {
        warn!("shader {path}: no usable technique");
    }
    (path, info)
}

impl MaterialCache {
    fn variant(&self, of: UntypedAssetId, v: &Variant) -> Option<PartMaterial> {
        self.variants.get(&of)?.get(v).cloned()
    }
}

/// Drops the variants of the removed materials ([`MaterialCache`]): a model's materials go with its last
/// instance, the variants once no entity draws them either.
pub fn evict_variants(
    mut cache: ResMut<MaterialCache>,
    mut gd: MessageReader<AssetEvent<GdMaterial>>,
    mut distort: MessageReader<AssetEvent<DistortMaterial>>,
    mut envmap: MessageReader<AssetEvent<EnvmapMaterial>>,
) {
    fn forget<A: Asset>(cache: &mut MaterialCache, events: &mut MessageReader<AssetEvent<A>>) {
        for ev in events.read() {
            if let AssetEvent::Removed { id } = ev {
                cache.variants.remove(&id.untyped());
            }
        }
    }
    forget(&mut cache, &mut gd);
    forget(&mut cache, &mut distort);
    forget(&mut cache, &mut envmap);
}

fn f1(m: &MshMaterial, n: &str) -> Option<f32> {
    match m.param(n)? {
        ParamValue::Float(f) => Some(*f),
        _ => None,
    }
}
fn f2(m: &MshMaterial, n: &str) -> Option<[f32; 2]> {
    match m.param(n)? {
        ParamValue::Float2(f) => Some(*f),
        _ => None,
    }
}
fn f3(m: &MshMaterial, n: &str) -> Option<[f32; 3]> {
    match m.param(n)? {
        ParamValue::Float3(f) => Some(*f),
        ParamValue::Float4(f) => Some([f[0], f[1], f[2]]),
        _ => None,
    }
}
/// A Float4 / Float3 colour parameter (0 when absent: the ice colours have no `.ssh` default).
fn f4(m: &MshMaterial, n: &str) -> Vec4 {
    match m.param(n) {
        Some(ParamValue::Float4(f)) => Vec4::from_array(*f),
        Some(ParamValue::Float3(f)) => Vec4::new(f[0], f[1], f[2], 0.0),
        _ => Vec4::ZERO,
    }
}

/// What a model part draws with.
#[derive(Clone, Debug, PartialEq)]
pub enum PartMaterial {
    Gd(Handle<GdMaterial>),
    /// The handle; `true` = drawn by `postObjectRenderStyle` ([`super::distort::PostObjectStyle`]).
    Distort(Handle<DistortMaterial>, bool),
    Envmap(Handle<EnvmapMaterial>),
    /// [`Special::Hidden`]: the part gets no material.
    Hidden,
}

impl PartMaterial {
    fn id(&self) -> Option<UntypedAssetId> {
        match self {
            PartMaterial::Gd(h) => Some(h.id().untyped()),
            PartMaterial::Distort(h, _) => Some(h.id().untyped()),
            PartMaterial::Envmap(h) => Some(h.id().untyped()),
            PartMaterial::Hidden => None,
        }
    }

    /// Gives a part its material component. The glow view takes the models that glow (post.rs
    /// `sync_glow_layers`); distortion and the cube-map portal are main-view only.
    pub fn insert(&self, e: &mut EntityCommands<'_>) {
        match self {
            PartMaterial::Gd(h) => {
                e.insert(MeshMaterial3d(h.clone()));
            }
            PartMaterial::Distort(h, object) => {
                e.insert(MeshMaterial3d(h.clone()));
                if *object {
                    e.insert(super::distort::PostObjectStyle);
                }
            }
            PartMaterial::Envmap(h) => {
                e.insert(MeshMaterial3d(h.clone()));
            }
            PartMaterial::Hidden => {}
        }
    }
}

/// The mesh facts a material is built for.
#[derive(Clone, Copy, Debug, Default)]
pub struct MeshFacts {
    pub skinned: bool,
    pub tangents: bool,
    /// Vertex colours.
    pub colors: bool,
}

/// An entity record's shader and glow overrides, as `Actor::Load` applies
/// them (Engine.dll `Actor::Load`): `shader` →
/// `GraphicsMeshInstance::SetDbrShader`, the shader of every
/// face set (`GetShader2` returns it for every material unless an item's
/// transmute turned it off, `DisableDbrShader`), drawn with the msh material's
/// own parameters; `glowTexture` → `SetGlowTexture` (every material),
/// `glowTextures[i]` per material, the global one first (`GetTexture`). Shader
/// parameters are bound by name (`SetShaderParameters`), so the glow shows only
/// when the shader has a `glowTexture` (e.g. a robe's runes: Mazaan's
/// `c109_torso` draws its `standardskinned` mesh with `standardglowskinned`
/// and the record's `_glo.tex`). The main menu's `kurnplatformstoneround_01a_paintable`
/// draws its `standardstaticdetailsnow` mesh with `standardstaticdetail` (no
/// snow), the snowy rose bushes with `standardstaticds_unusedcolor` (no wind).
#[derive(Clone, Copy, Debug, Default)]
pub struct Reshade<'a> {
    pub shader: Option<&'a str>,
    pub glow: Option<&'a str>,
}

/// The variant key of a [`Reshade`] and its trimmed shader / glow overrides; `None` without an override.
fn reshade_key<'a>(ov: &Reshade<'a>) -> Option<(Variant, Option<&'a str>, Option<&'a str>)> {
    fn some(s: Option<&str>) -> Option<&str> {
        s.map(str::trim).filter(|s| !s.is_empty())
    }
    let (shader, glow) = (some(ov.shader), some(ov.glow));
    if shader.is_none() && glow.is_none() {
        return None;
    }
    let key = Variant::Reshade(
        shader.unwrap_or("").to_ascii_lowercase(),
        glow.unwrap_or("").to_ascii_lowercase(),
    );
    Some((key, shader, glow))
}

/// The 2D texture parameters [`MaterialAssets::msh_material`] reads (UNORM), for [`MaterialAssets::variant_ready`].
const RESHADE_TEXTURES: [&str; 7] = [
    "baseTexture",
    "bumpTexture",
    "glowTexture",
    "detailTexture",
    "snowBase",
    "snowNormal",
    "materialTexture",
];

/// A material with the fallback textures and the shared buffers, lit and opaque.
fn blank(d: &GdDefaults) -> GdMaterial {
    GdMaterial {
        params: GdParams::default(),
        base: d.white.clone(),
        bump: d.flat_normal.clone(),
        glow: d.black.clone(),
        detail: d.white.clone(),
        snow_base: d.white.clone(),
        snow_normal: d.flat_normal.clone(),
        env: d.env.clone(),
        grass: d.grass.clone(),
        states: PassStates::default(),
    }
}

/// What the Grim Dawn materials are built into and from.
#[derive(SystemParam)]
pub struct MaterialAssets<'w> {
    pub gd: ResMut<'w, Assets<GdMaterial>>,
    pub distort: ResMut<'w, Assets<DistortMaterial>>,
    pub envmap: ResMut<'w, Assets<EnvmapMaterial>>,
    pub cache: ResMut<'w, MaterialCache>,
    pub defaults: Res<'w, GdDefaults>,
}

impl MaterialAssets<'_> {
    /// The plain lit material of a mesh without materials.
    pub fn plain(&mut self) -> PartMaterial {
        PartMaterial::Gd(self.gd.add(blank(&self.defaults)))
    }

    /// The material one msh material draws with (module doc).
    pub fn msh_material(
        &mut self,
        data: &GameData,
        m: &MshMaterial,
        mesh: MeshFacts,
        textures: &mut Textures,
        images: &mut Assets<Image>,
    ) -> PartMaterial {
        let tex = |name: &str, textures: &mut Textures, images: &mut Assets<Image>| {
            m.texture(name)
                .and_then(|t| textures.get(data, t, false, images))
        };
        let info = self
            .cache
            .shader_info(data, &m.shader, mesh.skinned)
            .unwrap_or(ShaderInfo {
                kind: KIND_LIT,
                states: PassStates::default(),
                special: Special::None,
                glow: true,
                ..Default::default()
            });
        let defaults = &*self.defaults;
        let base = tex("baseTexture", textures, images);
        let bump = tex("bumpTexture", textures, images);
        // Wind: meshes with vertex colours (gd_wind.wesl); the material's values,
        // else the shader's defaults (the amplitudes a shader lacks stay 1).
        let wind = info
            .wind
            .filter(|_| mesh.colors)
            .map(|w| {
                let mut v = w.defaults;
                for (i, (x, n)) in v.iter_mut().zip(WIND_PARAMS).enumerate() {
                    if let Some(f) = f1(m, n).filter(|_| i < 5 || w.amplitude) {
                        *x = f;
                    }
                }
                WindParams {
                    wind: Vec4::new(v[0], v[1], w.offset as u32 as f32, w.mode()),
                    ripple_scale: Vec4::new(v[2], v[3], v[4], 0.0),
                    ripple_amplitude: Vec4::new(v[5], v[6], v[7], 0.0),
                }
            })
            .unwrap_or_default();
        match info.special {
            Special::Hidden => PartMaterial::Hidden,
            Special::Distort {
                additive,
                color_scroll,
                post_object,
            } => {
                let s = f2(m, "scrollRate").unwrap_or([0.0; 2]);
                let b = f2(m, "bumpScrollRate").unwrap_or([0.0; 2]);
                let material = DistortMaterial {
                    params: DistortParams {
                        scroll: Vec4::new(s[0], s[1], b[0], b[1]),
                        // ssh defaults: distortion 0.5, tint 0; fade 1.
                        values: Vec4::new(
                            f1(m, "distortion").unwrap_or(0.5),
                            f1(m, "tint").unwrap_or(0.0),
                            1.0,
                            0.0,
                        ),
                        flags: UVec4::new(
                            additive as u32,
                            color_scroll as u32,
                            mesh.tangents as u32,
                            0,
                        ),
                        wind: wind.wind,
                        ripple_scale: wind.ripple_scale,
                        ripple_amplitude: wind.ripple_amplitude,
                    },
                    base: base.unwrap_or_else(|| defaults.white.clone()),
                    // The shader's default bump map (flat, alpha 1).
                    bump: bump
                        .or_else(|| {
                            textures.get(data, "system/textures/flatbmp.tex", false, images)
                        })
                        .unwrap_or_else(|| defaults.white.clone()),
                    states: info.states,
                    post_object,
                };
                PartMaterial::Distort(self.distort.add(material), post_object)
            }
            Special::Envmap => {
                let material = EnvmapMaterial {
                    base: base.unwrap_or_else(|| defaults.white.clone()),
                    envmap: m
                        .texture("envmapTexture")
                        .and_then(|t| textures.get_cube(data, t, images))
                        .unwrap_or_else(|| defaults.black_cube.clone()),
                    states: info.states,
                };
                PartMaterial::Envmap(self.envmap.add(material))
            }
            Special::None => {
                let shader = m.shader.to_ascii_lowercase();
                let glow = tex("glowTexture", textures, images);
                let detail = tex("detailTexture", textures, images);
                let snow_base = tex("snowBase", textures, images);
                let snow_normal = tex("snowNormal", textures, images);
                let mut flags = 0;
                if glow.is_some() && info.glow {
                    flags |= FLAG_GLOW;
                    // deferredPS of standard*glowscrollrgb* / *glowscrollglow*:
                    // the glow is sampled at uv + frac(scrollRate · time), its alpha too
                    // in the "glowscrollglow" family, at uv in the "rgb" one.
                    if shader.contains("glowscrollrgb") {
                        flags |= FLAG_GLOW_SCROLL;
                    } else if shader.contains("glowscrollglow") {
                        flags |= FLAG_GLOW_SCROLL | FLAG_GLOW_SCROLL_ALPHA;
                    }
                }
                if detail.is_some() {
                    flags |= FLAG_DETAIL;
                }
                if snow_base.is_some() && shader.contains("snow") {
                    flags |= FLAG_SNOW;
                }
                if info.states.alpha_test {
                    flags |= FLAG_ALPHA_TEST;
                }
                if mesh.tangents {
                    flags |= FLAG_HAS_TANGENTS;
                }
                if info.unlit {
                    flags |= FLAG_UNLIT;
                }
                if mesh.colors && info.vertex_color {
                    flags |= FLAG_VERTEX_COLOR;
                }
                if mesh.colors && info.vertex_alpha {
                    flags |= FLAG_VERTEX_ALPHA;
                }
                if info.bone_fade {
                    flags |= FLAG_BONE_FADE;
                }
                if info.blended_fade {
                    flags |= FLAG_BLENDED_FADE;
                }
                // The ice gradient is the vertex program's own colour: no mesh colour needed.
                let material_tex = info
                    .glacial
                    .then(|| tex("materialTexture", textures, images))
                    .flatten();
                if info.glacial {
                    flags |= FLAG_GLACIAL;
                }
                let spec = f3(m, "specularColor").unwrap_or([0.0; 3]);
                let scroll = if info.kind == KIND_LIT {
                    let s = f2(m, "scrollRate").unwrap_or([0.0; 2]);
                    Vec4::new(s[0], s[1], 0.0, 0.0)
                } else {
                    Vec4::new(
                        f1(m, "xColorScrollSpeed").unwrap_or(0.0),
                        f1(m, "yColorScrollSpeed").unwrap_or(0.0),
                        f1(m, "xAlphaScrollSpeed").unwrap_or(0.0),
                        f1(m, "yAlphaScrollSpeed").unwrap_or(0.0),
                    )
                };
                let material = GdMaterial {
                    params: GdParams {
                        specular: Vec4::new(
                            spec[0],
                            spec[1],
                            spec[2],
                            f1(m, "specularPower").unwrap_or(1.0).max(1e-3),
                        ),
                        scroll,
                        // ssh defaults: snowDepth 2, snowThreshold 0.7, snowNormals 1, fresnelAmount 1.
                        snow: Vec4::new(
                            f1(m, "snowDepth").unwrap_or(2.0),
                            f1(m, "snowThreshold").unwrap_or(0.7),
                            f1(m, "snowNormals").unwrap_or(1.0),
                            f1(m, "fresnelAmount").unwrap_or(1.0),
                        ),
                        flags: UVec4::new(info.kind, flags, 0, 0),
                        wind: wind.wind,
                        ripple_scale: wind.ripple_scale,
                        ripple_amplitude: wind.ripple_amplitude,
                        ice_inner: f4(m, "innerColor"),
                        ice_top: f4(m, "topColor"),
                        ice_bottom: f4(m, "bottomColor").with_w(f1(m, "meshHeight").unwrap_or(0.0)),
                        // ssh defaults: rimLight 3, sssParams 0.6.
                        ice: Vec4::new(
                            f1(m, "rimLight").unwrap_or(3.0),
                            f1(m, "sssParams").unwrap_or(0.6),
                            0.0,
                            0.0,
                        ),
                        ..GdParams::default()
                    },
                    base: base.unwrap_or_else(|| defaults.white.clone()),
                    bump: bump.unwrap_or_else(|| defaults.flat_normal.clone()),
                    glow: glow.unwrap_or_else(|| defaults.black.clone()),
                    // FLAG_GLACIAL: its materialTexture (no glacial shader has a detailTexture).
                    detail: material_tex
                        .or(detail)
                        .unwrap_or_else(|| defaults.white.clone()),
                    snow_base: snow_base.unwrap_or_else(|| defaults.white.clone()),
                    snow_normal: snow_normal.unwrap_or_else(|| defaults.flat_normal.clone()),
                    env: defaults.env.clone(),
                    grass: defaults.grass.clone(),
                    states: info.states,
                };
                PartMaterial::Gd(self.gd.add(material))
            }
        }
    }

    /// `def`, the material of msh material `m`, drawn with [`Reshade`]'s overrides (apply before
    /// [`MaterialAssets::retexture`]).
    #[allow(clippy::too_many_arguments)]
    pub fn reshade(
        &mut self,
        data: &GameData,
        def: &PartMaterial,
        m: &MshMaterial,
        mesh: MeshFacts,
        ov: Reshade,
        textures: &mut Textures,
        images: &mut Assets<Image>,
    ) -> PartMaterial {
        let Some((key, shader, glow)) = reshade_key(&ov) else {
            return def.clone();
        };
        // A hidden part has no material to key on: its (rare) reshade is built each time.
        let id = def.id();
        if let Some(v) = id.and_then(|id| self.cache.variant(id, &key)) {
            return v;
        }
        let mut m = m.clone();
        if let Some(sh) = shader {
            m.shader = sh.to_owned();
        }
        if let Some(g) = glow {
            let v = ParamValue::Texture(g.to_owned());
            match m.params.iter_mut().find(|p| p.name == "glowTexture") {
                Some(p) => p.value = v,
                None => m.params.push(gd_formats::msh::MaterialParam {
                    name: "glowTexture".into(),
                    value: v,
                }),
            }
        }
        let v = self.msh_material(data, &m, mesh, textures, images);
        if let Some(id) = id {
            self.cache
                .variants
                .entry(id)
                .or_default()
                .insert(key, v.clone());
        }
        v
    }

    /// Whether [`Self::reshade`] of `def` with `ov`, then [`Self::retexture`] with `base` / `bump`, decode and analyse
    /// nothing on the game thread: the override shader is analysed and the textures the variant reads (the msh
    /// material's [`RESHADE_TEXTURES`] with the glow override, the replacements) are decoded or failed. What is not
    /// starts loading in the background ([`MaterialCache::shader_ready`], [`Textures::ready_held`]); the decoded
    /// textures go into `held`, so a caller waiting on several keeps the ones already in. A cached variant costs
    /// nothing. Not covered: an override to a cube-map portal shader (its `envmapTexture` still decodes on the spot).
    #[allow(clippy::too_many_arguments)]
    pub fn variant_ready(
        &mut self,
        data: &GameData,
        def: &PartMaterial,
        m: &MshMaterial,
        mesh: MeshFacts,
        ov: Reshade,
        base: Option<&str>,
        bump: Option<&str>,
        textures: &mut Textures,
        images: &mut Assets<Image>,
        held: &mut Vec<Handle<Image>>,
    ) -> bool {
        let mut ready = true;
        for p in [base, bump]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            ready &= textures.ready_held(data, p, false, images, held);
        }
        let Some((key, shader, glow)) = reshade_key(&ov) else {
            return ready;
        };
        if def
            .id()
            .is_some_and(|id| self.cache.variant(id, &key).is_some())
        {
            return ready;
        }
        ready &= self
            .cache
            .shader_ready(data, shader.unwrap_or(m.shader.as_str()), mesh.skinned);
        for name in RESHADE_TEXTURES {
            let t = match name {
                "glowTexture" => glow.or_else(|| m.texture(name)),
                _ => m.texture(name),
            };
            if let Some(t) = t {
                ready &= textures.ready_held(data, t, false, images, held);
            }
        }
        ready
    }

    /// Per-entity texture override (Decoration records' `baseTextures` /
    /// `bumpTextures`, one entry per msh material; empty = keep): `def` with the
    /// replacement textures.
    pub fn retexture(
        &mut self,
        data: &GameData,
        def: &PartMaterial,
        base: Option<&str>,
        bump: Option<&str>,
        textures: &mut Textures,
        images: &mut Assets<Image>,
    ) -> PartMaterial {
        let base = base.map(str::trim).unwrap_or("");
        let bump = bump.map(str::trim).unwrap_or("");
        let Some(id) = def.id().filter(|_| !base.is_empty() || !bump.is_empty()) else {
            return def.clone();
        };
        let key = Variant::Retexture(base.to_owned(), bump.to_owned());
        if let Some(v) = self.cache.variant(id, &key) {
            return v;
        }
        let mut load = |p: &str| {
            (!p.is_empty())
                .then(|| textures.get(data, p, false, images))
                .flatten()
        };
        let (base, bump) = (load(base), load(bump));
        if base.is_none() && bump.is_none() {
            return def.clone();
        }
        let v = match def {
            PartMaterial::Gd(h) => {
                let Some(mut m) = self.gd.get(h).cloned() else {
                    return def.clone();
                };
                if let Some(t) = base {
                    m.base = t;
                }
                if let Some(t) = bump {
                    m.bump = t;
                }
                PartMaterial::Gd(self.gd.add(m))
            }
            PartMaterial::Distort(h, object) => {
                let Some(mut m) = self.distort.get(h).cloned() else {
                    return def.clone();
                };
                if let Some(t) = base {
                    m.base = t;
                }
                if let Some(t) = bump {
                    m.bump = t;
                }
                PartMaterial::Distort(self.distort.add(m), *object)
            }
            PartMaterial::Envmap(h) => {
                let (Some(mut m), Some(base)) = (self.envmap.get(h).cloned(), base) else {
                    return def.clone();
                };
                m.base = base;
                PartMaterial::Envmap(self.envmap.add(m))
            }
            PartMaterial::Hidden => return PartMaterial::Hidden,
        };
        self.cache
            .variants
            .entry(id)
            .or_default()
            .insert(key, v.clone());
        v
    }

    /// `def` glowing with `glow` (an entity record's `glowTexture` overriding the mesh's, e.g. Devastation's
    /// meteors); only the lit family has a glow.
    pub fn reglow(
        &mut self,
        data: &GameData,
        def: &PartMaterial,
        glow: &str,
        textures: &mut Textures,
        images: &mut Assets<Image>,
    ) -> PartMaterial {
        let PartMaterial::Gd(h) = def else {
            return def.clone();
        };
        let (id, key) = (h.id().untyped(), Variant::Glow(glow.to_owned()));
        if let Some(v) = self.cache.variant(id, &key) {
            return v;
        }
        // UNORM like every engine texture (the msh glow too, [`MaterialAssets::msh_material`]):
        // gd_standard.wesl adds the stored values; an sRGB view would decode
        // them and dim the glow (0.5 → 0.21).
        let (Some(t), Some(mut m)) = (
            textures.get(data, glow, false, images),
            self.gd.get(h).cloned(),
        ) else {
            return def.clone();
        };
        m.glow = t;
        m.params.flags.y |= FLAG_GLOW;
        let v = PartMaterial::Gd(self.gd.add(m));
        self.cache
            .variants
            .entry(id)
            .or_default()
            .insert(key, v.clone());
        v
    }

    /// The material for one pass of a terrain grass layer's blades
    /// (render/grass.rs): lit, its blade atlas as base texture, both faces, no
    /// specular, the grass wind bend (gd_wind.wesl, strength set by
    /// [`super::grass::grass_wind`]); `fringe` = grass.ssh `deferred` (alpha ≥ 0.35,
    /// SrcAlpha / InvSrcAlpha, depth `Less`), else `deferredBase` (alpha ≥ 0.65, opaque);
    /// `blade_height` = the layer's `bladeHeight` (the offset field's drop, gd_wind.wesl).
    /// One per (texture, height, pass) until the mod changes ([`MaterialCache`]): every load of a level reuses it.
    pub fn grass(
        &mut self,
        data: &GameData,
        texture: &str,
        fringe: bool,
        blade_height: f32,
        textures: &mut Textures,
        images: &mut Assets<Image>,
    ) -> Handle<GdMaterial> {
        let key = (texture.to_ascii_lowercase(), blade_height.to_bits(), fringe);
        if let Some(h) = self.cache.grass.get(&key) {
            return h.clone();
        }
        let d = &*self.defaults;
        let material = GdMaterial {
            params: GdParams {
                snow: Vec4::new(2.0, 0.7, 1.0, 1.0),
                flags: UVec4::new(KIND_LIT, FLAG_ALPHA_TEST, 0, 0),
                fade: Vec4::new(1.0, if fringe { 0.35 } else { 0.65 }, 1.0, 1.0),
                // y ≠ 0 selects WIND_SHADER; the strength is the mesh tag (`grass_wind`).
                wind: Vec4::new(0.0, 1.0, 0.0, WIND_GRASS),
                // Grass: x = `bladeHeight` (no ripple for the grass bend).
                ripple_scale: Vec4::new(blade_height, 0.0, 0.0, 0.0),
                ..GdParams::default()
            },
            base: textures
                .get(data, texture, false, images)
                .unwrap_or_else(|| d.white.clone()),
            bump: d.flat_normal.clone(),
            glow: d.black.clone(),
            detail: d.white.clone(),
            snow_base: d.white.clone(),
            snow_normal: d.flat_normal.clone(),
            env: d.env.clone(),
            grass: d.grass.clone(),
            states: PassStates {
                cull: 0,
                // grass.ssh `deferred`: SrcBlend 6, DstBlend 8, DepthFunc Less.
                blend: fringe.then_some((6, 8)),
                alpha_test: !fringe,
                depth_strict: fringe,
                ..PassStates::default()
            },
        };
        let h = self.gd.add(material);
        self.cache.grass.insert(key, h.clone());
        h
    }
}
