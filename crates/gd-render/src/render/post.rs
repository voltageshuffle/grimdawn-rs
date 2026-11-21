//! Camera setup for the gamma-space pipeline and the engine's final image
//! chain ([`GammaOut`]): fog, tone map, bloom, enhancement, depth of field,
//! then the UI over the gamma values and gamma → linear.
//!
//! UI: the engine draws the UI last, alpha-blended (source-alpha / inverse source-alpha) onto its back buffer, a
//! B8G8R8A8_UNORM target holding gamma values (`GraphicsDeferredRenderer::CreateRenderTargets` @0x1800ad850,
//! Direct3D11.dll sub_1800142F0: format 2, textures UNORM, no sRGB conversion on read or write), so every blend
//! (colour alphas, fades, textures' own alpha: glows, soft shadows, glyph edges) mixes gamma values. Here the chain's
//! last pass leaves gamma values in the frame, Bevy's UI pass draws over them, and [`ui_to_linear`] (after `ui_pass`,
//! before `upscaling`) turns the frame to linear for the sRGB swapchain, which encodes it back. Bevy's UI shaders work in
//! linear (colours converted from sRGB on extraction, `_srgb` textures decoded on sampling), so [`patch_ui_shaders`]
//! rewrites their sources to sRGB-encode what they write: `ui.wesl` encodes the node colour and the texel separately
//! (their product is then the engine's colour × texel), the gradient / box-shadow / slice shaders their result; the
//! port's UI materials (`ui/additive.wesl`, `ui/masked.wesl`, `doll.wesl`) do the same themselves. One full-screen
//! pass (Rgba16Float, the frame copy of `post_process_write`). PORT: the float frame does not clamp between UI draws
//! (the UNORM back buffer does; only additive draws can exceed 1, clamped once at the end); the patch matches the
//! shaders' text, so a Bevy update that changes it falls back to wrapping the fragment (warned once if not even that).
//!
//! Ambient occlusion (`ambientocclusion`, `Options` 30 → `SetAmbientOcclusionEnabled`, renderer +484, Grim Dawn.exe
//! sub_1400BF650 / sub_140028D10), Engine.dll: `RenderFinalScene` @0x1800adeb0 calls
//! `GraphicsCanvas::ApplySSAO(depth, zNear, zFar)` @0x1800a2460 right after the ambient / sun / point lights, before the
//! forward passes, fog and glow. postprocessing/ssao.ssh `create` (createPS) draws a full-size temporary (format 2):
//! per pixel a rotation vector r from a 4 × 4 texture tiled one texel per pixel (sub_1800A2230: 16 random unit
//! vectors from `rand()`, B8G8R8A8), the 8 kernel vectors k reflected about it (k − 2(k·r)r), each sample at
//! uv + reflected.xy · ssaoScale; with depths linearised to view depth / zFar, a sample farther than the centre adds
//! 1 − min(12·|Δ|, 1); occ = min(¼·Σ, 1), out = lerp(ssaoStrength, 1, occ). ssaoStrength = 0.7, ssaoScale = 0.015
//! (`Engine` +3064 / +3068, Engine ctor @0x180072930; nothing else writes them). Then sub_1800A48A0 blurs it (gain 1,
//! spread `Engine` +3072 = 1: the 8-tap blur below, vertical into w × h/2, horizontal into w/2 × h/2) and ssao.ssh
//! `render` (renderPS: a plain sample) multiplies it onto the light target (blend Dst / Zero, the states of
//! multiplyscroll*.ssh). So it only darkens: flat ground ~4 % on average (about half the reflected samples land
//! farther), creases and the feet of walls up to 30 %. The engine's rotation vectors depend on the `rand()`
//! state at device creation (PORT: the first 16 of a seed-1 `rand()` stream). PORT: the depth taps are filtered by
//! hand (bilinear, as the engine's sampler). [`apply_ssao`] runs between Bevy's opaque pass (the engine's deferred
//! lights) and its transparent pass, so as in the engine the forward passes (water, blended meshes, effects) are
//! neither occluded nor part of the depth it reads. The decals' base layer and the grass fringe, which the engine
//! blends into the G-buffer, are drawn before it (`gbuffer_blend.rs`), so they are occluded (a few % on flat ground).
//!
//! Fog, `GraphicsDeferredRenderer::RenderFog` @0x1800ac690 and
//! deferred.ssh `fog` (fogPS, SrcAlpha / OneMinusSrcAlpha), drawn by
//! `RenderFinalScene` after the scene passes and before the scene glow when
//! the renderer's fog flag (+440) is set and a depth or height density is
//! positive. The flag is the `fog` option (`Options` 61, Grim Dawn.exe sub_140028D10
//! `SetFogEnabled(GraphicsEngine fog && option 61)`; [`VideoOptions`], `GD_FOG=1`
//! forces it on). Where geometry
//! was drawn, a = max(sat((z − start)·inv)·density, sat((h − start)·inv)·
//! density) for the depth (view depth z) and height (world height h) fog,
//! inv = 1 / (end − start) (`SetDepthFogParams` / `SetHeightFogParams`),
//! colour = fog colour ([`super::env::Ambiance`]: the camera clamp keeps the
//! depth fog behind the player: at Devil's Crossing a blue-grey haze over the
//! upper part of the screen, as in the real game's fog-on shots). [`fog_and_glow`] draws it in the engine's
//! place: after the scene passes (particles, decals, blended meshes), before the scene glow and the distortion.
//!
//! Tone map (`hdrLighting`, `Options` 70, `SetHDRLighting(on, factor)` → renderer +1968 / +1972),
//! Engine.dll: `GraphicsDeferredRenderer::RenderFinalScene` @0x1800adeb0 then lights and draws the whole
//! scene (ambient, lights, forward passes, fog, glow) into renderer +254, a float target (format 13 =
//! R16G16B16A16_FLOAT, `CreateRenderTargets` @0x1800ad850), instead of the back buffer; the lights are
//! scaled by `GetHdrLightingFactor` (render/env.rs `HDR_LIGHTING_FACTOR`). `GraphicsMTRenderer::Render`
//! @0x1800d5830 then calls `PerformTonemapping` (vtable +24, @0x1800ad170) before `RenderPostEffects`: a
//! fullscreen deferred.ssh `tonemap` (tonemapPS) from that target into the back buffer: per channel, with
//! l = log2 c, x = 0.2314·l + ½, t = x / 1.5: y = 1.5·(t³ − 2t² + t) − ½·(2t³ − 3t² + 1), or 0.2314·l
//! below l = −2.161; out = 2^min(4.3219·y, 0) (c ≤ 0 → 0): unchanged below 0.224, a shoulder above
//! (1 → 0.64, 2 → 0.82, 4 → 0.93). The bloom and enhancement below then read the tone-mapped UNORM frame.
//!
//! `GraphicsMTRenderer::RenderPostEffects` @0x1800cc200 (Engine.dll),
//! run after the scene (glow and outline included) while post effects are on
//! (renderer +481, the `advancedEffects` option, bool 10) and the AmbianceManager's
//! environment effects are enabled (+177, on by default), with the outdoor
//! slot's values ([`super::env::Ambiance`]):
//! 1. Bloom, `GraphicsCanvas::HotBlurFrameBuffer(512, 512, a, gain, sat)`
//!    @0x1800a3760 when a > 0 and gain > 0.25: the frame is copied to 512²,
//!    filter.ssh `extractHot` renders it into 256² (4 taps at ±½ texel of
//!    512, `extractConst` = (1 − a, 1 − a, 1 − a, ½ / a)):
//!    h = sat(avg − (1 − a)) · ½ / a, r = 2h, m = mean(r), out = m + sat·(r − m);
//!    then the 8-tap blur sub_1800A48A0 (weights gain·{.10,.11,.14,.15,.15,
//!    .14,.11,.10} at ±{0.5..3.5} texels, vertical into 256 × 128, horizontal
//!    into 128²), added to the frame (composite.ssh `lighten`: One / One).
//! 2. Enhancement, `GraphicsCanvas::ApplyEnhancement(contrast, brightness,
//!    −saturation, tint)` @0x1800a12e0 with postprocessing/enhance.ssh:
//!    c += s·(lum − c) (lum = .3 r + .59 g + .11 b, s = −saturation),
//!    c += (c − ½)·contrast, out = sat(c + brightness)·tint; skipped when all
//!    are neutral.
//! 3. Depth of field (renderer +483, the `depthOfField` option,
//!    `GD_DOF=0` turns it off),
//!    `GraphicsCanvas::ApplyDepthOfField` @0x1800a1ab0: the frame is copied
//!    to a full-size temporary (format 2), downsample.ssh `normal` draws it
//!    into a half-size one (one bilinear tap per texel: the 2 × 2 mean), then
//!    depthoffield.ssh draws lerp(frame, blur, sat((1.27·z − dofDistance) /
//!    dofRange)) over the frame, z the linear view depth; dofDistance = eye to
//!    camera target + [`DOF_DISTANCE`], dofRange = [`DOF_RANGE`]
//!    (`RenderPostEffects`, `Engine` +3076 / +3080). Pixels with nothing
//!    drawn (cleared depth) take the blur.
//! 4. FXAA (renderer +485, [`Fxaa`]: `SetFXAAEnabled(deferredrendering && device generation 7 / 8 && fxaa), the port always renders deferred so it is `fxaa``, Grim
//!    Dawn.exe sub_140028D10 every frame, so the option applies live), `GraphicsCanvas::ApplyFXAA`
//!    @0x1800a2ce0, the last call of `RenderPostEffects` and not gated by `advancedEffects` (+481): the frame is
//!    copied into a full-size temporary (format 2) and postprocessing/fxaa.ssh `normal` draws it back with
//!    `pixelSize` = 1 / its size (gd_post.wesl `fxaa`, transcribed from the shader's DXBC). PORT: the colour-blind
//!    remap (`Display::Render`, after the world render) is applied before it, in the grading.
//!
//! Multisampling (`antiAliasing`, int 1: the literal sample count, 0 / 1 off), Engine.dll sub_180093240
//! (`RenderDeviceSettings` +24, built by `GraphicsEngine::UpdateFromOptions`): with `deferredrendering` (bool 51) on a
//! generation 7 / 8 (D3D11) device the count is forced to 0, so the deferred renderer never multisamples and FXAA is
//! its only anti-aliasing. PORT: the port always renders the engine's deferred way (no `deferredrendering` option, no
//! forward renderer), so MSAA never applies; the main camera is `Msaa::Off`.
//!
//! The `brightness` / `contrast` / `gamma` options (`Options` floats 6 / 7 / 8) drive only the hardware gamma
//! ramp of exclusive fullscreen ([`GammaOut::with_gamma_ramp`], applied by the last pass, [`ui_to_linear`]);
//! `GraphicsCanvas::GammaCorrect` (gammacorrect.ssh) has no caller and `WinWindow::SetGammaRamp`
//! (SetDeviceGammaRamp) is only a vtable slot.
//! The deferred `tonemap` technique runs only with `hdrLighting` (above).
//!
//! The copy, the temporaries (format 2) and the back buffer are 8-bit UNORM
//! targets, so values are clamped between the passes (PORT: the temporaries
//! here are Rgba8Unorm; the 512² copy is skipped: the 4 extract taps sample
//! the frame at the same points with the same bilinear filter). The scene
//! target here is float and unclamped; the chain saturates (or tone maps) it first.
use bevy::camera::Hdr;
use bevy::core_pipeline::core_3d::{Transparent3d, main_opaque_pass_3d, main_transparent_pass_3d};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping, tonemapping};
use bevy::core_pipeline::upscaling::upscaling;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::ecs::entity::EntityHashSet;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_component::{
    ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
    UniformComponentPlugin,
};
use bevy::render::render_phase::{SortedRenderPhase, ViewSortedRenderPhases};
use bevy::render::render_resource::binding_types::{
    sampler, texture_2d, texture_depth_2d, uniform_buffer,
};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId,
    ColorTargetState, ColorWrites, FragmentState, LoadOp, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
    SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, StoreOp, TextureDescriptor,
    TextureDimension, TextureSampleType, TextureUsages, TextureView,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{
    ExtractedView, RetainedViewEntity, ViewDepthStencilTexture, ViewTarget, ViewUniform,
    ViewUniformOffset, ViewUniforms,
};
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::ShaderRef;
use bevy::ui_render::ui_pass;
use std::collections::HashMap;

const POST_SHADER: &str = "embedded://gd_render/render/shaders/gd_post.wesl";
/// Bloom temporaries (engine format 2; see the module doc).
const BLOOM_FORMAT: bevy::render::render_resource::TextureFormat =
    bevy::render::render_resource::TextureFormat::Rgba8Unorm;

/// The `ApplySSAO` pass ([`apply_ssao`]), for ordering the passes before it.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SsaoSet;

pub struct PostPlugin;

impl Plugin for PostPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ExtractComponentPlugin::<GammaOut>::default(),
            UniformComponentPlugin::<GammaOut>::default(),
        ))
        .add_plugins(ExtractComponentPlugin::<Fxaa>::default())
        .add_systems(PostStartup, setup_cameras)
        .add_systems(PostUpdate, apply_fxaa_option)
        .add_systems(Startup, load_ui_shaders)
        .add_systems(First, patch_ui_shaders);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<CompositeEntities>()
            .init_resource::<HeldComposite>()
            .add_systems(RenderStartup, init_post_pipelines)
            .add_systems(ExtractSchedule, extract_composite)
            .add_systems(
                Render,
                (
                    prepare_post_textures.in_set(RenderSystems::PrepareResources),
                    hold_composite.in_set(RenderSystems::PrepareBindGroups),
                    restore_composite.in_set(RenderSystems::Cleanup),
                ),
            )
            .add_systems(
                Core3d,
                (
                    apply_ssao
                        .in_set(SsaoSet)
                        .after(main_opaque_pass_3d)
                        .before(main_transparent_pass_3d)
                        .in_set(Core3dSystems::MainPass),
                    fog_and_glow
                        .after(main_transparent_pass_3d)
                        .in_set(Core3dSystems::MainPass),
                    post_chain
                        .in_set(Core3dSystems::PostProcess)
                        .before(tonemapping),
                    ui_to_linear.after(ui_pass).before(upscaling),
                ),
            );
    }
}

/// The final-image parameters of a main camera ([`super::env::Ambiance`],
/// written by `env::update_env`; module doc).
#[derive(Component, ExtractComponent, Clone, Copy, ShaderType, Default, PartialEq, Debug)]
#[extract_app(RenderApp)]
pub struct GammaOut {
    /// 1 − a, ½ / a, saturation, 1 = bloom on.
    pub bloom: Vec4,
    /// x: blur gain; yzw unused.
    pub bloom_gain: Vec4,
    /// brightness, contrast, shader saturation (−saturation), 1 = enhancement on.
    pub adjust: Vec4,
    /// rgb tint, w unused.
    pub tint: Vec4,
    /// Fog colour rgb, w: 1 = the fog pass runs.
    pub fog_color: Vec4,
    /// Depth fog: start, 1 / (end − start), density (`SetDepthFogParams`).
    pub depth_fog: Vec4,
    /// Height fog: start, 1 / (end − start), density (`SetHeightFogParams`).
    pub height_fog: Vec4,
    /// Depth of field: dofDistance, dofRange, unused, 1 = on.
    pub dof: Vec4,
    /// The colour-blind remap (`GraphicsCanvas::ApplyColorRemap` @0x1800a3180): the rows of the 3 x 3 matrix
    /// (xyz) applied to the final colour; `w` of the first row 1 = on.
    pub remap_r: Vec4,
    pub remap_g: Vec4,
    pub remap_b: Vec4,
    /// x: 1 = `hdrLighting`'s tone map runs (module doc).
    pub hdr: Vec4,
    /// Ambient occlusion (module doc): 1 = on, ssaoStrength, ssaoScale, zFar.
    pub ao: Vec4,
    /// The fullscreen gamma ramp ([`GammaOut::with_gamma_ramp`]): 2c, 2b − 1, 1 / g, 1 = on.
    pub gamma_ramp: Vec4,
}

impl GammaOut {
    /// `advancedEffects` (bool 10, "Post Processing"): off skips the bloom / contrast / colour-grade chain
    /// (`RenderPostEffects` @0x1800CC213).
    pub fn with_post_effects(mut self, on: bool) -> Self {
        if !on {
            self.bloom.w = 0.0;
            self.adjust.w = 0.0;
        }
        self
    }

    /// The `colorblind` option's remap (matrices @0x18033F6D0: 1 protanopia,
    /// 2 deuteranopia, 3 tritanopia; 0 off). PORT: applied to the world view only (the UI is drawn after this chain).
    pub fn with_colorblind(mut self, mode: i32) -> Self {
        let rows = match mode {
            1 => [Vec3::new(0.0, 2.0234, -2.5281), Vec3::Y, Vec3::Z],
            2 => [Vec3::X, Vec3::new(0.4942, 0.0, 1.2483), Vec3::Z],
            3 => [Vec3::X, Vec3::Y, Vec3::new(-0.3959, 0.8011, 0.0)],
            _ => [Vec3::X, Vec3::Y, Vec3::Z],
        };
        self.remap_r = rows[0].extend(if mode > 0 { 1.0 } else { 0.0 });
        self.remap_g = rows[1].extend(0.0);
        self.remap_b = rows[2].extend(0.0);
        self
    }

    /// The hardware gamma ramp of exclusive fullscreen (`screenMode` 0):
    /// `GraphicsEngine::UpdateFromOptions` (Engine.dll @0x180093194) takes gamma = lerp(0.25, 1.75, `gamma`
    /// float 8, the "Fullscreen Gamma" slider), brightness = lerp(0.25, 0.75, float 6), contrast = lerp(0.25,
    /// 0.75, float 7) to `RenderDevice::SetBrightnessContrastGamma` (Direct3D11.dll @0x180007350), which in
    /// exclusive fullscreen writes the 256-entry ramp of `UpdateGammaRamp` @0x180001E40,
    /// `out = sat(((x − ½)·2c + ½ + 2b − 1)^(1/g))` per channel, into `IDXGIOutput::SetGammaControl`; windowed
    /// it restores the default ramp. The ramp acts on the whole output (the UI too): here the last pass
    /// ([`ui_to_linear`]) applies it to the gamma values. PORT: the curve itself, not a 256-step table.
    pub fn with_gamma_ramp(
        mut self,
        fullscreen: bool,
        brightness: f32,
        contrast: f32,
        gamma: f32,
    ) -> Self {
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let (b, c, g) = (
            lerp(0.25, 0.75, brightness),
            lerp(0.25, 0.75, contrast),
            lerp(0.25, 1.75, gamma),
        );
        self.gamma_ramp = Vec4::new(
            2.0 * c,
            2.0 * b - 1.0,
            1.0 / g,
            if fullscreen { 1.0 } else { 0.0 },
        );
        self
    }
}

/// The video options the final image reads (Grim Dawn.exe sub_140028D10 pushes them to the renderer every
/// frame).
#[derive(Clone, Copy, Debug)]
pub struct VideoOptions {
    /// `fog` (61): `SetFogEnabled` (renderer +440).
    pub fog: bool,
    /// `depthOfField` (19): renderer +483.
    pub depth_of_field: bool,
    /// `hdrLighting` (70): `SetHDRLighting` (renderer +1968).
    pub hdr: bool,
    /// `ambientocclusion` (30): `SetAmbientOcclusionEnabled` (renderer +484).
    pub ambient_occlusion: bool,
}

/// The `Options` constructor's defaults (fog, depth of field and ambient occlusion on, `hdrLighting` off).
impl Default for VideoOptions {
    fn default() -> Self {
        Self {
            fog: true,
            depth_of_field: true,
            hdr: false,
            ambient_occlusion: true,
        }
    }
}

impl VideoOptions {
    pub fn from_options(o: &gd_game::options::GameOptions) -> Self {
        Self {
            fog: o.flag("fog"),
            depth_of_field: o.flag("depthOfField"),
            hdr: o.flag("hdrLighting"),
            ambient_occlusion: o.flag("ambientocclusion"),
        }
    }
}

/// `GraphicsMTRenderer::SetFXAAEnabled` (renderer +485) on a main camera: on with the `fxaa` option (bool 52) (the engine also
/// needs `deferredrendering`, always on here), Grim Dawn.exe sub_140028D10 (every frame, module doc).
#[derive(Component, ExtractComponent, Clone, Copy, PartialEq, Debug)]
#[extract_app(RenderApp)]
pub struct Fxaa(pub bool);

/// The options' FXAA flag onto the main cameras (no options file: the `Options` default, on).
fn apply_fxaa_option(
    options: Option<Res<gd_game::options::GameOptions>>,
    mut cams: Query<&mut Fxaa>,
) {
    let on = options.as_ref().is_none_or(|o| o.flag("fxaa"));
    for mut f in &mut cams {
        f.set_if_neq(Fxaa(on));
    }
}

/// `Engine` +3076 / +3080 (constructor; nothing else writes them): the depth
/// of field starts 8 m behind the camera target and ramps over 20 m.
pub const DOF_DISTANCE: f32 = 8.0;
pub const DOF_RANGE: f32 = 20.0;

/// `Engine` +3064 / +3068 (ctor @0x180072930): ssao.ssh `ssaoStrength` (the darkest the occlusion gets) and
/// `ssaoScale` (the kernel radius in UV).
pub const SSAO_STRENGTH: f32 = 0.7;
pub const SSAO_SCALE: f32 = 0.015;

impl GammaOut {
    /// `cam_distance`: eye to camera target (the depth of field's focus). `video`: the `fog` (`Options` 61),
    /// `depthOfField` (19) and `hdrLighting` (70) options (`GD_FOG=1` forces the fog on, `GD_DOF=0` the depth
    /// of field off).
    pub fn from_ambiance(
        a: &super::env::Ambiance,
        cam_distance: f32,
        hooks: &gd_game::hooks::RenderHooks,
        video: VideoOptions,
    ) -> Self {
        let (b, adj) = (a.bloom, a.adjust);
        let bloom_on = b.x > 0.0 && b.y > 0.25;
        let k = b.x.clamp(0.0, 1.0);
        let enhance_on =
            adj.x != 0.0 || adj.y != 0.0 || adj.z != 0.0 || a.tint.cmplt(Vec3::ONE).any();
        // RenderFinalScene: the fog pass needs the option (renderer +440)
        // and a depth or height density > 0.
        let fog_on = (video.fog || hooks.fog) && (a.depth_fog.z > 0.0 || a.height_fog.z > 0.0);
        // SetDepthFogParams / SetHeightFogParams store 1 / (end − start).
        let inv = |f: Vec3| if f.y != f.x { 1.0 / (f.y - f.x) } else { 0.0 };
        Self {
            bloom: Vec4::new(
                1.0 - k,
                if k > 0.0 { 0.5 / k } else { 0.0 },
                b.z,
                if bloom_on { 1.0 } else { 0.0 },
            ),
            bloom_gain: Vec4::new(b.y, 0.0, 0.0, 0.0),
            adjust: Vec4::new(adj.x, adj.y, -adj.z, if enhance_on { 1.0 } else { 0.0 }),
            tint: a.tint.extend(1.0),
            fog_color: a.fog_color.extend(if fog_on { 1.0 } else { 0.0 }),
            depth_fog: Vec4::new(a.depth_fog.x, inv(a.depth_fog), a.depth_fog.z, 0.0),
            height_fog: Vec4::new(a.height_fog.x, inv(a.height_fog), a.height_fog.z, 0.0),
            dof: Vec4::new(
                cam_distance + DOF_DISTANCE,
                DOF_RANGE,
                0.0,
                if hooks.no_dof || !video.depth_of_field {
                    0.0
                } else {
                    1.0
                },
            ),
            hdr: Vec4::new(if video.hdr { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0),
            // zFar: the scene camera's far plane (`ApplySSAO`'s a4 = scene +204).
            ao: Vec4::new(
                if video.ambient_occlusion { 1.0 } else { 0.0 },
                SSAO_STRENGTH,
                SSAO_SCALE,
                crate::camera::world_projection().far,
            ),
            ..default()
        }
    }
}

/// Float scene target, no tonemapping/dither, then [`GammaOut`]. The depth
/// buffer is made samplable for the fog and depth-of-field passes.
fn setup_cameras(
    mut commands: Commands,
    mut cams: Query<(Entity, &mut Camera3d), Without<GlowCamera>>,
) {
    for (e, mut cam) in &mut cams {
        let usage = TextureUsages::from(cam.depth_texture_usages) | TextureUsages::TEXTURE_BINDING;
        cam.depth_texture_usages = usage.into();
        commands.entity(e).insert((
            Hdr,
            Tonemapping::None,
            DebandDither::Disabled,
            // sub_180093240: the deferred renderer zeroes the device's multisample count (module doc).
            Msaa::Off,
            GammaOut::default(),
            Fxaa(true),
        ));
    }
}

#[derive(Resource)]
pub(crate) struct PostPipelines {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    fog: CachedRenderPipelineId,
    extract: CachedRenderPipelineId,
    blur_v: CachedRenderPipelineId,
    blur_h: CachedRenderPipelineId,
    finish: CachedRenderPipelineId,
    finish_gamma: CachedRenderPipelineId,
    to_linear: CachedRenderPipelineId,
    downsample: CachedRenderPipelineId,
    dof: CachedRenderPipelineId,
    ao_create: CachedRenderPipelineId,
    ao_blur_v: CachedRenderPipelineId,
    ao_blur_h: CachedRenderPipelineId,
    ao_apply: CachedRenderPipelineId,
    dof_gamma: CachedRenderPipelineId,
    fxaa: CachedRenderPipelineId,
}

/// The Hdr main texture (ViewTarget::main_texture_format).
const MAIN_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

fn init_post_pipelines(
    mut commands: Commands,
    device: Res<RenderDevice>,
    assets: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "gd_post_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<GammaOut>(true),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_depth_2d(),
                uniform_buffer::<ViewUniform>(true),
            ),
        ),
    );
    // Bilinear, clamped (the engine canvas samplers).
    let sampler = device.create_sampler(&SamplerDescriptor {
        mag_filter: bevy::render::render_resource::FilterMode::Linear,
        min_filter: bevy::render::render_resource::FilterMode::Linear,
        ..default()
    });
    let shader: Handle<Shader> = assets.load(POST_SHADER);
    let pipe = |label: &'static str, entry: &'static str, format, blend| {
        cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some(label.into()),
            layout: vec![layout.clone()],
            vertex: fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: shader.clone(),
                entry_point: Some(entry.into()),
                targets: vec![Some(ColorTargetState {
                    format,
                    blend,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        })
    };
    let fog = pipe("gd_post_fog", "fog", MAIN_FORMAT, None);
    let extract = pipe("gd_post_extract", "extract_hot", BLOOM_FORMAT, None);
    let blur_v = pipe("gd_post_blur_v", "blur_v", BLOOM_FORMAT, None);
    let blur_h = pipe("gd_post_blur_h", "blur_h", BLOOM_FORMAT, None);
    let finish = pipe("gd_post_finish", "finish", MAIN_FORMAT, None);
    let finish_gamma = pipe("gd_post_finish_gamma", "finish", BLOOM_FORMAT, None);
    let to_linear = pipe("gd_post_to_linear", "to_linear", MAIN_FORMAT, None);
    let downsample = pipe("gd_post_downsample", "downsample", BLOOM_FORMAT, None);
    let dof = pipe("gd_post_dof", "depth_of_field", MAIN_FORMAT, None);
    let dof_gamma = pipe("gd_post_dof_gamma", "depth_of_field", BLOOM_FORMAT, None);
    let fxaa = pipe("gd_post_fxaa", "fxaa", MAIN_FORMAT, None);
    let ao_create = pipe("gd_post_ao_create", "ao_create", BLOOM_FORMAT, None);
    let ao_blur_v = pipe("gd_post_ao_blur_v", "ao_blur_v", BLOOM_FORMAT, None);
    let ao_blur_h = pipe("gd_post_ao_blur_h", "ao_blur_h", BLOOM_FORMAT, None);
    // ssao.ssh `render`: src · dst.
    let multiply = BlendState {
        color: BlendComponent {
            src_factor: BlendFactor::Dst,
            dst_factor: BlendFactor::Zero,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponent::REPLACE,
    };
    let ao_apply = pipe("gd_post_ao_apply", "ao_apply", MAIN_FORMAT, Some(multiply));
    commands.insert_resource(PostPipelines {
        layout,
        sampler,
        fog,
        extract,
        blur_v,
        blur_h,
        finish,
        finish_gamma,
        to_linear,
        downsample,
        dof,
        ao_create,
        ao_blur_v,
        ao_blur_h,
        ao_apply,
        dof_gamma,
        fxaa,
    });
}

impl PostPipelines {
    /// Every pipeline compiled: the chain (which leaves gamma values) and [`ui_to_linear`] run together or not at all.
    fn ready(&self, cache: &PipelineCache) -> bool {
        [
            self.fog,
            self.extract,
            self.blur_v,
            self.blur_h,
            self.finish,
            self.finish_gamma,
            self.to_linear,
            self.downsample,
            self.dof,
            self.ao_create,
            self.ao_blur_v,
            self.ao_blur_h,
            self.ao_apply,
            self.dof_gamma,
            self.fxaa,
        ]
        .into_iter()
        .all(|id| cache.get_render_pipeline(id).is_some())
    }
}

/// Per view: the 256², 256 × 128 and 128² bloom temporaries, the
/// depth of field's full-size frame copy and half-size blur, then the
/// ambient occlusion's full-size, w × h/2 and half-size targets, the FXAA frame copy.
#[derive(Component)]
pub(crate) struct PostTextures([CachedTexture; 9]);

fn prepare_post_textures(
    mut commands: Commands,
    mut cache: ResMut<TextureCache>,
    device: Res<RenderDevice>,
    views: Query<(Entity, &ExtractedCamera), With<GammaOut>>,
) {
    for (e, cam) in &views {
        let Some(size) = cam.physical_viewport_size else {
            continue;
        };
        let tex = |cache: &mut TextureCache, w: u32, h: u32, label: &'static str| {
            cache.get(
                &device,
                TextureDescriptor {
                    label: Some(label),
                    size: bevy::render::render_resource::Extent3d {
                        width: w.max(1),
                        height: h.max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: BLOOM_FORMAT,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        };
        let t = [
            tex(&mut cache, 256, 256, "gd_bloom_hot"),
            tex(&mut cache, 256, 128, "gd_bloom_v"),
            tex(&mut cache, 128, 128, "gd_bloom_h"),
            tex(&mut cache, size.x, size.y, "gd_dof_frame"),
            tex(&mut cache, size.x / 2, size.y / 2, "gd_dof_blur"),
            tex(&mut cache, size.x, size.y, "gd_ao"),
            tex(&mut cache, size.x, size.y / 2, "gd_ao_v"),
            tex(&mut cache, size.x / 2, size.y / 2, "gd_ao_h"),
            tex(&mut cache, size.x, size.y, "gd_fxaa_frame"),
        ];
        commands.entity(e).insert(PostTextures(t));
    }
}

/// A post pass: pipeline, source, auxiliary texture, target.
type Pass<'a> = (
    &'a bevy::render::render_resource::RenderPipeline,
    &'a TextureView,
    &'a TextureView,
    &'a TextureView,
);

/// What every `gd_post.wesl` pass binds besides its two textures.
struct PostBindings<'a> {
    layout: bevy::render::render_resource::BindGroupLayout,
    sampler: &'a Sampler,
    settings: bevy::render::render_resource::BindingResource<'a>,
    view: bevy::render::render_resource::BindingResource<'a>,
    depth: &'a TextureView,
    index: u32,
    view_offset: u32,
}

impl<'a> PostBindings<'a> {
    /// `None` until every post pipeline is compiled ([`PostPipelines::ready`]) and the uniforms are uploaded.
    fn new(
        p: &'a PostPipelines,
        cache: &'a PipelineCache,
        uniforms: &'a ComponentUniforms<GammaOut>,
        view_uniforms: &'a ViewUniforms,
        depth: &'a ViewDepthStencilTexture,
        index: &DynamicUniformIndex<GammaOut>,
        view_offset: &ViewUniformOffset,
    ) -> Option<Self> {
        if !p.ready(cache) {
            return None;
        }
        Some(Self {
            layout: cache.get_bind_group_layout(&p.layout),
            sampler: &p.sampler,
            settings: uniforms.uniforms().binding()?,
            view: view_uniforms.uniforms.binding()?,
            depth: depth.attachment.depth_stencil_views().depth_only_view()?,
            index: index.index(),
            view_offset: view_offset.offset,
        })
    }

    /// Draws `passes` in order; `load`: draw over the target (the blended passes) instead of replacing it.
    fn run(&self, ctx: &mut RenderContext, passes: &[Pass], load: bool) {
        for &(pipeline, src, aux, dst) in passes {
            let bg = ctx.render_device().create_bind_group(
                "gd_post_bind_group",
                &self.layout,
                &BindGroupEntries::sequential((
                    src,
                    self.sampler,
                    self.settings.clone(),
                    aux,
                    self.depth,
                    self.view.clone(),
                )),
            );
            let desc = RenderPassDescriptor {
                label: Some("gd_post"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: dst,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: if load {
                            LoadOp::Load
                        } else {
                            LoadOp::Clear(Default::default())
                        },
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            };
            let mut pass = ctx.command_encoder().begin_render_pass(&desc);
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bg, &[self.index, self.view_offset]);
            pass.draw(0..3, 0..1);
        }
    }
}

/// `ApplySSAO` (module doc): create (reads only the depth), blur, multiply onto the light target. Runs between the
/// opaque pass (the engine's deferred lights) and the transparent one, so the forward passes are not occluded.
/// `GraphicsDeferredRenderer::RenderFinalScene` @0x1800adeb0: ApplySSAO after the lights, before the
/// forward groups (water, alpha-blended meshes, effects).
fn apply_ssao(
    view: ViewQuery<(
        &ViewTarget,
        &PostTextures,
        &DynamicUniformIndex<GammaOut>,
        &GammaOut,
        &ViewUniformOffset,
        &ViewDepthStencilTexture,
    )>,
    pipelines: Option<Res<PostPipelines>>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<GammaOut>>,
    view_uniforms: Res<ViewUniforms>,
    mut ctx: RenderContext,
) {
    let (target, textures, index, settings, view_offset, depth) = view.into_inner();
    if settings.ao.x <= 0.5 {
        return;
    }
    let Some(p) = pipelines.as_deref() else {
        return;
    };
    let Some(b) = PostBindings::new(
        p,
        &cache,
        &uniforms,
        &view_uniforms,
        depth,
        index,
        view_offset,
    ) else {
        return;
    };
    let get = |id| cache.get_render_pipeline(id);
    let (Some(create), Some(blur_v), Some(blur_h), Some(apply)) = (
        get(p.ao_create),
        get(p.ao_blur_v),
        get(p.ao_blur_h),
        get(p.ao_apply),
    ) else {
        return;
    };
    let [.., ao, ao_v, ao_h, _] = &textures.0;
    let (a, av, ah) = (&ao.default_view, &ao_v.default_view, &ao_h.default_view);
    b.run(
        &mut ctx,
        &[
            (create, ah, ah, a),
            (blur_v, a, ah, av),
            (blur_h, av, a, ah),
        ],
        false,
    );
    b.run(
        &mut ctx,
        &[(apply, ah, a, target.main_texture_view())],
        true,
    );
}

fn post_chain(
    view: ViewQuery<(
        &ViewTarget,
        &PostTextures,
        &DynamicUniformIndex<GammaOut>,
        &GammaOut,
        &ViewUniformOffset,
        &ViewDepthStencilTexture,
        Option<&Fxaa>,
    )>,
    pipelines: Option<Res<PostPipelines>>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<GammaOut>>,
    view_uniforms: Res<ViewUniforms>,
    mut ctx: RenderContext,
) {
    let (target, textures, index, settings, view_offset, depth, fxaa_on) = view.into_inner();
    let fxaa_on = fxaa_on.is_some_and(|f| f.0);
    let Some(p) = pipelines.as_deref() else {
        return;
    };
    let Some(b) = PostBindings::new(
        p,
        &cache,
        &uniforms,
        &view_uniforms,
        depth,
        index,
        view_offset,
    ) else {
        return;
    };
    let get = |id| cache.get_render_pipeline(id);
    let (
        Some(extract),
        Some(blur_v),
        Some(blur_h),
        Some(finish),
        Some(finish_gamma),
        Some(downsample),
        Some(dof),
        Some(dof_gamma),
        Some(fxaa),
    ) = (
        get(p.extract),
        get(p.blur_v),
        get(p.blur_h),
        get(p.finish),
        get(p.finish_gamma),
        get(p.downsample),
        get(p.dof),
        get(p.dof_gamma),
        get(p.fxaa),
    )
    else {
        return;
    };
    let [hot, bv, bh, dof_frame, dof_blur, .., fxaa_frame] = &textures.0;
    let run = |ctx: &mut RenderContext, passes: &[Pass]| b.run(ctx, passes, false);
    let post = target.post_process_write();
    let (source, destination) = (post.source, post.destination);
    // ApplyFXAA: the frame copied into a format-2 temporary, fxaa.ssh drawn from it into the frame. Here the last pass
    // writes that temporary directly.
    let (last_dst, finish, dof) = if fxaa_on {
        (&fxaa_frame.default_view, finish_gamma, dof_gamma)
    } else {
        (destination, finish, dof)
    };
    let mut passes: Vec<Pass> = Vec::new();
    if settings.bloom.w > 0.5 {
        passes.push((extract, source, &bv.default_view, &hot.default_view));
        passes.push((
            blur_v,
            &hot.default_view,
            &bh.default_view,
            &bv.default_view,
        ));
        passes.push((
            blur_h,
            &bv.default_view,
            &hot.default_view,
            &bh.default_view,
        ));
    }
    if settings.dof.w > 0.5 {
        passes.push((
            finish_gamma,
            source,
            &bh.default_view,
            &dof_frame.default_view,
        ));
        passes.push((
            downsample,
            &dof_frame.default_view,
            &bh.default_view,
            &dof_blur.default_view,
        ));
        passes.push((
            dof,
            &dof_frame.default_view,
            &dof_blur.default_view,
            last_dst,
        ));
    } else {
        passes.push((finish, source, &bh.default_view, last_dst));
    }
    if fxaa_on {
        passes.push((
            fxaa,
            &fxaa_frame.default_view,
            &bh.default_view,
            destination,
        ));
    }
    run(&mut ctx, &passes);
}

/// The main-world entities with the glow composite mesh, rebuilt only when the marked set changed.
#[derive(Resource, Default)]
struct CompositeEntities(EntityHashSet);

fn extract_composite(
    mut set: ResMut<CompositeEntities>,
    parts: Extract<Query<(Entity, Ref<MeshMaterial3d<GlowComposite>>)>>,
) {
    if !super::marked_changed(set.0.len(), &parts) {
        return;
    }
    set.0.clear();
    set.0.extend(parts.iter().map(|(e, _)| e));
}

/// The glow composite items of each view, kept out of Bevy's transparent pass
/// between [`hold_composite`] and [`restore_composite`] so [`fog_and_glow`]
/// can draw them after the fog (the engine's `RenderSceneGlow` position).
#[derive(Resource, Default)]
pub(crate) struct HeldComposite(HashMap<RetainedViewEntity, SortedRenderPhase<Transparent3d>>);

fn hold_composite(
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut held: ResMut<HeldComposite>,
    entities: Res<CompositeEntities>,
) {
    for (view, phase) in phases.iter_mut() {
        let keys: Vec<_> = (phase.items.keys())
            .filter(|k| entities.0.contains(&k.1.id()))
            .copied()
            .collect();
        if keys.is_empty() {
            continue;
        }
        let mut composite = SortedRenderPhase::<Transparent3d>::default();
        for k in keys {
            if let Some(item) = phase.items.shift_remove(&k) {
                composite.items.insert(k, item);
            }
        }
        held.0.insert(*view, composite);
    }
}

fn restore_composite(
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut held: ResMut<HeldComposite>,
) {
    for (view, composite) in held.0.drain() {
        if let Some(p) = phases.get_mut(&view) {
            p.items.extend(composite.items);
            p.transient_items.extend(composite.transient_items);
        }
    }
}

/// The engine's fog and scene glow, in `RenderFinalScene`'s order (module doc):
/// the fog over the finished scene passes (particles, decals, blended meshes),
/// then the glow composite added over the fog — before the distortion
/// (`super::distort::draw_distortion`, the engine's `RenderPostObjects`) and the
/// rest of the post chain. The composite is held out of the transparent pass
/// ([`hold_composite`]) and drawn here.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fog_and_glow(
    world: &World,
    view: ViewQuery<(
        &ViewTarget,
        &PostTextures,
        &DynamicUniformIndex<GammaOut>,
        &GammaOut,
        &ViewUniformOffset,
        &ViewDepthStencilTexture,
        &ExtractedView,
    )>,
    pipelines: Option<Res<PostPipelines>>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<GammaOut>>,
    view_uniforms: Res<ViewUniforms>,
    held: Res<HeldComposite>,
    mut ctx: RenderContext,
) {
    let view_entity = view.entity();
    let (target, textures, index, settings, view_offset, depth, extracted) = view.into_inner();
    let Some(p) = pipelines.as_deref() else {
        return;
    };
    let Some(b) = PostBindings::new(
        p,
        &cache,
        &uniforms,
        &view_uniforms,
        depth,
        index,
        view_offset,
    ) else {
        return;
    };
    if settings.fog_color.w > 0.5
        && let Some(fog) = cache.get_render_pipeline(p.fog)
    {
        let post = target.post_process_write();
        b.run(
            &mut ctx,
            &[(
                fog,
                post.source,
                &textures.0[2].default_view,
                post.destination,
            )],
            false,
        );
    }
    let Some(phase) = held.0.get(&extracted.retained_view_entity) else {
        return;
    };
    if phase.items.is_empty() {
        return;
    }
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("gd_glow_composite"),
        color_attachments: &[Some(target.get_color_attachment())],
        depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Err(err) = phase.render(&mut pass, world, view_entity) {
        error!("glow composite pass: {err:?}");
    }
}

/// After Bevy's UI pass: the frame (gamma values, the UI blended over them as on the engine's back buffer) →
/// linear for the sRGB swapchain (module doc, "UI").
fn ui_to_linear(
    view: ViewQuery<(
        &ViewTarget,
        &DynamicUniformIndex<GammaOut>,
        &ViewUniformOffset,
        &ViewDepthStencilTexture,
    )>,
    pipelines: Option<Res<PostPipelines>>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<GammaOut>>,
    view_uniforms: Res<ViewUniforms>,
    mut ctx: RenderContext,
) {
    let (target, index, view_offset, depth) = view.into_inner();
    let Some(p) = pipelines.filter(|p| p.ready(&cache)) else {
        return;
    };
    let (Some(pipeline), Some(binding), Some(view_binding), Some(depth_view)) = (
        cache.get_render_pipeline(p.to_linear),
        uniforms.uniforms().binding(),
        view_uniforms.uniforms.binding(),
        depth.attachment.depth_stencil_views().depth_only_view(),
    ) else {
        return;
    };
    let post = target.post_process_write();
    let bg = ctx.render_device().create_bind_group(
        "gd_post_to_linear_bind_group",
        &cache.get_bind_group_layout(&p.layout),
        &BindGroupEntries::sequential((
            post.source,
            &p.sampler,
            binding,
            post.source,
            depth_view,
            view_binding,
        )),
    );
    let mut pass = ctx
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("gd_post_to_linear"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: post.destination,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Default::default()),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &bg, &[index.index(), view_offset.offset]);
    pass.draw(0..3, 0..1);
}

/// The Bevy UI shaders that draw into the frame (`bevy_ui_render`: nodes, images and text; gradients; box shadows;
/// sliced images), patched by [`gamma_ui_source`].
const UI_SHADERS: [&str; 4] = [
    "embedded://bevy_ui_render/ui.wesl",
    "embedded://bevy_ui_render/gradient.wesl",
    "embedded://bevy_ui_render/box_shadow.wesl",
    "embedded://bevy_ui_render/ui_texture_slice.wesl",
];

/// Marks a patched UI shader source.
const UI_GAMMA_MARK: &str = "// gd: gamma-space UI output";

/// sRGB encode of a linear colour (alpha kept): a UI colour / texel back to the gamma value it was given as.
const UI_GAMMA_FN: &str = "
fn gd_gamma(c: vec4<f32>) -> vec4<f32> {
    let x = max(c.rgb, vec3<f32>(0.0));
    let hi = 1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return vec4<f32>(select(hi, x * 12.92, x <= vec3<f32>(0.0031308)), c.a);
}
";

#[derive(Resource)]
struct UiShaders(Vec<Handle<Shader>>);

fn load_ui_shaders(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(UiShaders(
        UI_SHADERS.iter().map(|p| assets.load(*p)).collect(),
    ));
}

/// Swaps each loaded UI shader's source for its gamma-space version (again after a reload); the pipeline cache
/// recompiles on the `Modified` event.
fn patch_ui_shaders(handles: Res<UiShaders>, mut shaders: ResMut<Assets<Shader>>) {
    for h in &handles.0 {
        let Some(src) = shaders.get(h).and_then(|s| match &s.source {
            bevy::shader::Source::Wesl(src) | bevy::shader::Source::Wgsl(src) => Some(src),
            _ => None,
        }) else {
            continue;
        };
        if src.contains(UI_GAMMA_MARK) {
            continue;
        }
        let Some(new) = gamma_ui_source(src) else {
            bevy::log::warn_once!("UI shader {:?}: no fragment entry to patch", h.path());
            continue;
        };
        if let Some(mut s) = shaders.get_mut(h) {
            s.source = match &s.source {
                bevy::shader::Source::Wgsl(_) => bevy::shader::Source::Wgsl(new.into()),
                _ => bevy::shader::Source::Wesl(new.into()),
            };
        }
    }
}

/// A Bevy UI shader made to write gamma values (module doc, "UI"). `ui.wesl` (nodes, images, text) encodes the
/// node colour and the texel separately (`gd_gamma(colour) · gd_gamma(texel)`: the engine's colour × texture on
/// raw values); the others' `fragment` is wrapped, its linear result encoded. `None` without a `fragment` entry.
fn gamma_ui_source(src: &str) -> Option<String> {
    const UI_LINE: &str =
        "let color = select(in.color, in.color * texture_color, enabled(in.flags, TEXTURED));";
    let body = if src.contains(UI_LINE) {
        src.replacen(
            UI_LINE,
            "let color = select(gd_gamma(in.color), gd_gamma(in.color) * gd_gamma(texture_color), \
             enabled(in.flags, TEXTURED));",
            1,
        )
    } else {
        let f = src.find("fn fragment(")?;
        let at = src[..f].rfind("@fragment")?;
        let args = f + "fn fragment(".len();
        let close = args + src[args..].find(')')?;
        let open = close + src[close..].find('{')?;
        // `in: VertexType` (one argument, maybe on its own line with a trailing comma).
        let (name, ty) = src[args..close]
            .trim()
            .trim_end_matches(',')
            .split_once(':')?;
        let (name, ty) = (name.trim(), ty.trim());
        // The wrapped function is no entry point: its result loses `@location(0)`.
        let ret = src[close..open].replacen("@location(0) ", "", 1);
        format!(
            "{}fn gd_linear_fragment({}{ret}{}\n@fragment\nfn fragment({name}: {ty}) -> @location(0) vec4<f32> {{\n    \
             return gd_gamma(gd_linear_fragment({name}));\n}}\n",
            &src[..at],
            &src[args..close],
            &src[open..],
        )
    };
    Some(format!("{UI_GAMMA_MARK}\n{body}{UI_GAMMA_FN}"))
}

// ---------------------------------------------------------------------------
// Scene glow (GraphicsDeferredRenderer::RenderSceneGlow 0x1800ace20).
//
// The G-buffer glow target (glow.rgb·glow.a; renderer +253, format 2 like the
// other G-buffer targets, `CreateRenderTargets` @0x1800ad850, so at most 1;
// no glow scale in the material programs or glowPS) is added once unblurred (the
// deferred `glow` technique; done in the material shaders) and once through
// GraphicsCanvas::ApplyGlow(glow, 0.75, 4.0): a separable 8-tap blur
// (sub_1800A48A0) with weights 0.75·{.10,.11,.14,.15,.15,.14,.11,.10} at
// ±{0.5..3.5}·4 texels, vertical into a half-height then horizontal into a
// half-size target, composited additively (postprocessing/glow.ssh `normal`).
//
// Here a second camera renders the same view into a float image with the
// materials in "glow only" mode (marked by its exposure), and a fullscreen
// triangle on the main camera adds the blurred result before GammaOut.
//
// Hover outline, per GraphicsDeferredRenderer::RenderFinalScene
// @0x1800adeb0 (after RenderSceneGlow / RenderPostObjects): the outlined
// meshes' `deferredOutlineStyle` passes are drawn into a cleared full-size
// temporary target (format 2), depth-tested LessEqual against the scene
// depth without depth writes, cull none; their outlinePS (standard*.ssh)
// writes `outlineColor.rgb` flat with alpha 1 where the base texture's alpha
// is >= 0.01 (blend SrcAlpha / Zero). Then ApplyGlow(target, 1.25, 1.0)
// @0x1800a17c0 → sub_1800A48A0: the same 8-tap blur with weights
// 1.25·{.10 … .10} at ±{0.5..3.5}·1 source texel (vertical into w × h/2,
// horizontal into w/2 × h/2), added with postprocessing/glow.ssh `normal`
// (One / One, StencilFunc NotEqual, ref 255). Stencil 255 is written by the
// outlined meshes' own `deferredStencilStyle` pass (ref 255, Replace,
// LessEqual; drawn last among the scene passes, ExecutePrepareScene
// @0x1800adbc0 group +518, enabled by GraphicsMeshInstance::
// GetNumRenderPasses @0x1800c3bd0 only while outlined), so the blurred outline (and the
// scene glow's blur too) is added only around the object's visible pixels,
// never over them: a soft halo of about 4 px.
//
// Nothing else limits the halo: it is added to whatever lies next to those
// pixels, an occluder's edge included. Occluded parts of the object add
// nothing (the depth test), blended occluders write no depth and hide nothing.
//
// Here the glow view's alpha holds that mask: every depth-writing surface
// leaves 0 and the outlined parts' copies (`GdMaterial::outline_mask`, spawned
// by hover.rs, in the glow view only) draw their palette class (1 / 2 / 3)
// after them, with the passes' states; the composite blurs the texels'
// class colours (8 × 8 bilinear taps, 1 texel: 9 × 9 texels) × 1.25², both
// blurs skipped where the unblurred mask is set. PORT: the blurs
// run in one 64-tap pass at full resolution instead of two passes into
// half-size targets; the target format (2) is not decoded, so values are not
// clamped between the passes.
//
// PORT (performance, a deliberate difference): the glow view draws only the
// models that glow ([`sync_glow_layers`]), not the whole scene as black
// occluders, which cost a second draw of every mesh. So scene glow and the
// hover outline are not hidden by other objects (a glowing or outlined
// object behind a wall or tree shows its glow / halo through it); a model's
// own parts still occlude each other.
//
// The composite is drawn by [`fog_and_glow`] after the fog and before the
// distortion (the engine's RenderSceneGlow, between RenderFog and
// RenderPostObjects). PORT: scene glow and outline share one draw (the engine
// draws the outline after the post objects).
// ---------------------------------------------------------------------------

use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::camera::{Exposure, RenderTarget};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, CompareFunction,
    SpecializedMeshPipelineError, TextureFormat,
};
use bevy::window::PrimaryWindow;

use super::material::{FLAG_GLOW, GdMaterial};

/// Layer of the glow composite triangle (main camera only).
const COMPOSITE_LAYER: usize = 7;
/// Layer the glow camera renders: the models that glow ([`sync_glow_layers`]),
/// the decal glow layer and the hover outline's masks. Only materials that
/// know the glow-view marker may be on it: anything else (stock PBR, water)
/// would be lit with the glow camera's exposure and blow out.
pub const GLOW_LAYER: usize = 6;

/// Render layers for a mesh that is drawn in the main and the glow view.
pub fn glow_aware_layers() -> RenderLayers {
    RenderLayers::layer(0).with(GLOW_LAYER)
}

/// Puts a model on the [`GLOW_LAYER`] while one of its parts' materials glows
/// (`FLAG_GLOW`): all the main-view parts under the same parent, so the model
/// occludes its own glow. Glow flags only change with a new material handle (reglow,
/// icy, dissolve), hence `Changed`. Parts off layer 0 (paper
/// dolls, the outline masks) are left alone.
fn sync_glow_layers(
    mut commands: Commands,
    mats: Res<Assets<GdMaterial>>,
    changed: Query<(Entity, Option<&ChildOf>), Changed<MeshMaterial3d<GdMaterial>>>,
    parts: Query<(Entity, &MeshMaterial3d<GdMaterial>, Option<&RenderLayers>)>,
    children: Query<&Children>,
    mut done: Local<bevy::platform::collections::HashSet<Entity>>,
) {
    let main = RenderLayers::layer(0);
    for (e, parent) in &changed {
        let group = parent.map_or(e, ChildOf::parent);
        if !done.insert(group) {
            continue;
        }
        let members = || {
            let siblings = parent.and_then(|_| children.get(group).ok());
            let own = siblings.is_none().then_some(e);
            siblings
                .into_iter()
                .flat_map(|c| c.iter())
                .chain(own)
                .filter_map(|m| parts.get(m).ok())
                .filter(|(_, _, l)| l.is_none_or(|l| l.intersects(&main)))
        };
        let glows = members().any(|(_, m, _)| {
            mats.get(&m.0)
                .is_some_and(|m| m.params.flags.y & FLAG_GLOW != 0)
        });
        for (m, _, layers) in members() {
            let old = layers.cloned().unwrap_or_default();
            let new = if glows {
                old.clone().with(GLOW_LAYER)
            } else {
                old.clone().without(GLOW_LAYER)
            };
            if new != old {
                commands.entity(m).try_insert(new);
            }
        }
    }
    done.clear();
}

#[derive(Component)]
pub struct GlowCamera;

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct GlowComposite {
    /// x: total gain (0.75²), y: tap spacing in texels (4), z: outline blur
    /// gain (1.25²), w: 0 (reserved: 1 selects the shader's debug view, the unblurred glow
    /// and the outline mask in magenta).
    #[uniform(0)]
    pub params: Vec4,
    /// Outline palette ([`OutlineGlow`]): class 1's colour (rgb; w 1 = something is outlined), classes 2 and 3.
    #[uniform(0)]
    pub outline: Vec4,
    #[uniform(0)]
    pub outline2: Vec4,
    #[uniform(0)]
    pub outline3: Vec4,
    #[texture(1)]
    #[sampler(2)]
    pub glow: Handle<Image>,
}

impl GlowComposite {
    /// The scene glow's composite of the glow target `glow` (gain 0.75², taps 4 texels apart, outline gain 1.25²,
    /// no outline yet).
    pub fn scene(glow: Handle<Image>) -> Self {
        Self {
            params: Vec4::new(0.75 * 0.75, 4.0, 1.25 * 1.25, 0.0),
            outline: Vec4::ZERO,
            outline2: Vec4::ZERO,
            outline3: Vec4::ZERO,
            glow,
        }
    }
}

/// The composite's fullscreen triangle (clip-space positions; see gd_glow.wesl).
pub fn fullscreen_triangle() -> Mesh {
    Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![[-1.0f32, -1.0, 0.0], [3.0, -1.0, 0.0], [-1.0, 3.0, 0.0]],
    )
    .with_inserted_indices(bevy::mesh::Indices::U32(vec![0, 1, 2]))
}

/// The composite is held out of the transparent pass and drawn by
/// [`fog_and_glow`]; the bias keeps it last within that held phase (it sorts by
/// the world origin's view depth otherwise).
const COMPOSITE_SORT_BIAS: f32 = 1.0e9;

impl Material for GlowComposite {
    fn depth_bias(&self) -> f32 {
        COMPOSITE_SORT_BIAS
    }
    fn vertex_shader() -> ShaderRef {
        "embedded://gd_render/render/shaders/gd_glow.wesl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "embedded://gd_render/render/shaders/gd_glow.wesl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
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
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_write_enabled = Some(false);
            ds.depth_compare = Some(CompareFunction::Always);
        }
        if let Some(frag) = descriptor.fragment.as_mut() {
            let c = BlendComponent {
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            };
            for t in frag.targets.iter_mut().flatten() {
                t.blend = Some(BlendState { color: c, alpha: c });
            }
        }
        Ok(())
    }
}

pub struct GlowPlugin;

impl Plugin for GlowPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<GlowComposite>::default())
            .init_resource::<OutlineGlow>()
            .add_systems(PostStartup, setup_glow.after(setup_cameras))
            .add_systems(
                PostUpdate,
                (
                    sync_glow_camera.after(bevy::transform::TransformSystems::Propagate),
                    sync_outline,
                    sync_glow_layers
                        .after(crate::model::ModelSpawnSet)
                        .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
                ),
            );
    }
}

/// The outline pass's palette this frame (`hover.rs`): the colour of each mask class 1, 2, 3 (the outlined parts'
/// `GdParams::outline.w`; zero: no part of that class). Each part's mask holds its class, so objects of different
/// outline colours share the pass (`Actor::SetOutlineColor` per actor).
#[derive(Resource, Default, PartialEq)]
pub struct OutlineGlow(pub [Vec3; 3]);

fn sync_outline(
    outline: Res<OutlineGlow>,
    comp: Query<&MeshMaterial3d<GlowComposite>>,
    mut mats: ResMut<Assets<GlowComposite>>,
) {
    if !outline.is_changed() {
        return;
    }
    for m in &comp {
        if let Some(mut m) = mats.get_mut(&m.0) {
            let any = outline.0.iter().any(|c| *c != Vec3::ZERO);
            m.outline = outline.0[0].extend(any as u8 as f32);
            m.outline2 = outline.0[1].extend(0.0);
            m.outline3 = outline.0[2].extend(0.0);
        }
    }
}

fn setup_glow(
    mut commands: Commands,
    mains: Query<Entity, (With<Camera3d>, With<GammaOut>)>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<GlowComposite>>,
    hooks: Res<gd_game::hooks::Hooks>,
) {
    if hooks.render.no_glow {
        return;
    }
    let Ok(win) = window.single() else { return };
    let (w, h) = (win.physical_width().max(1), win.physical_height().max(1));
    let image = images.add(Image::new_target_texture(
        w,
        h,
        TextureFormat::Rgba16Float,
        None,
    ));
    commands.spawn((
        GlowCamera,
        Camera3d::default(),
        // Alpha 0: the outline mask starts empty.
        Camera {
            order: -1,
            clear_color: Color::NONE.into(),
            ..default()
        },
        RenderTarget::Image(image.clone().into()),
        Hdr,
        Tonemapping::None,
        DebandDither::Disabled,
        Msaa::Off,
        // ev100 0 → view.exposure 0.83: the materials' "glow only" marker.
        Exposure { ev100: 0.0 },
        RenderLayers::layer(GLOW_LAYER),
        Transform::default(),
        Projection::default(),
    ));
    let mat = mats.add(GlowComposite::scene(image));
    commands.spawn((
        Mesh3d(meshes.add(fullscreen_triangle())),
        MeshMaterial3d(mat),
        NoFrustumCulling,
        RenderLayers::layer(COMPOSITE_LAYER),
        Transform::default(),
    ));
    for e in &mains {
        commands
            .entity(e)
            .insert(RenderLayers::layer(0).with(COMPOSITE_LAYER));
    }
}

/// The glow view follows the main camera: same transform and projection,
/// and a target of the same size. (A target left at the start-up window size
/// gives the glow camera another aspect ratio once the window is resized, so
/// glow and outlines land away from their objects.)
fn sync_glow_camera(
    mains: Query<(&Camera, &GlobalTransform, &Projection), (With<GammaOut>, Without<GlowCamera>)>,
    mut glow: Query<
        (
            &mut Transform,
            &mut GlobalTransform,
            &mut Projection,
            &RenderTarget,
        ),
        With<GlowCamera>,
    >,
    mut images: ResMut<Assets<Image>>,
    comp: Query<&MeshMaterial3d<GlowComposite>>,
    mut mats: ResMut<Assets<GlowComposite>>,
) {
    let Some((cam, gt, proj)) = mains.iter().next() else {
        return;
    };
    for (mut t, mut g, mut p, target) in &mut glow {
        *t = gt.compute_transform();
        *g = *gt;
        *p = proj.clone();
        if let (Some(size), RenderTarget::Image(img)) = (cam.physical_target_size(), target)
            && images.get(&img.handle).is_some_and(|i| i.size() != size)
            && let Some(mut i) = images.get_mut(&img.handle)
        {
            i.resize(bevy::render::render_resource::Extent3d {
                width: size.x.max(1),
                height: size.y.max(1),
                depth_or_array_layers: 1,
            });
            // Rebind the new texture in the composite.
            for m in &comp {
                mats.get_mut(&m.0);
            }
        }
    }
}
