//! Terrain grass: the blades a terrain layer's `TerrainType` scatters over
//! the ground (the Conclave's dry shrubs and grass tufts, meadows, ...).
//!
//! Engine.dll:
//! - `TerrainType::Load` (fields): `BladeTextureFileName` (the
//!   blade atlas; a layer without one has no grass), `BladeWidth`,
//!   `BladeHeight`, `BladeSpacing` (int), `BladeVariations` (int,
//!   default 1), `MinimumBladeOpacity` (int), `VariationProbability` (int
//!   list, its sum kept), `VariationClumpiness` (int list), `affectedByWind`.
//! - `GrassObject::CreateGrassBuffers`: with the terrain quality
//!   (`Terrain::GetQuality` = `GraphicsEngine.terrainQuality`, from the
//!   `detailLevel` option in `GraphicsEngine::UpdateFromOptions`: low 0, medium
//!   1, else ≥ 2) above 0 the spacing is `kGrassHighQualitySpacing` 0.58 m,
//!   else 1.16 m and only the layers whose blades are ≥ 1.65 m high and that
//!   have no `clutterMesh` grow; a terrain block holds at most 350 blades per
//!   layer. Read when a level's terrain is built (levels loaded later follow a
//!   changed option).
//! - `GrassObject::SetupGrassGeometry`, per 9 × 9-cell terrain
//!   block (`TerrainBase::ConstructBlocks`: vertices 9i ..= min(9i + 9, w − 1))
//!   and per grass layer, over the points x = x0, x0 + s, … ≤ x1 (z outer, x
//!   inner, f32 steps):
//!   1. a variation per point: `wang(int(x·12343 + z·7873)) % (variations +
//!      BladeSpacing) ≥ variations` → none, else `GetRandomTexture`
//!      of that int (`v % sum`, then the first i whose running sum
//!      of probabilities reaches it);
//!   2. with more than one variation, `PerformClumping`
//!      (seed `wang(13·(x0 + 871·z0))`; ≤ 256 Park–Miller-picked swaps of two
//!      points `ClumpFind` returns: from a start, the first point
//!      whose variation has fewer equal 8-neighbours (wrapping) than its
//!      clumpiness; only when there are as many clumpiness values as
//!      variations);
//!   3. per point, the layer's opacity o (`Terrain::GetLayerOpacityInterpolated`:
//!      bilinear over the cell weights, truncated) times
//!      Π (255 − o_above) / 255 over the layers above; o < int(MinimumBladeOpacity
//!      · 2.55) → no blade; else a vertical quad at the point on the ground
//!      (`GetHeightInterpolated`), turned by `wang(int(x·3219 + z·23113))`
//!      degrees, half width o/255 · BladeWidth, height o/255 · BladeHeight,
//!      normal +Y, u = variation / variations (+1/variations), v 0 at the top.
//! - grass.ssh, drawn twice (`TerrainRenderInterface::RenderGrass`, "TerrainGrass
//!   (Base)" then "TerrainGrass"): `deferredBase` keeps texels with alpha ≥ 0.65,
//!   opaque; `deferred` those with alpha ≥ 0.35, blended SrcAlpha / InvSrcAlpha
//!   with depth test `Less` (so only around the base pass's texels); cull none,
//!   albedo = the blade texel, normal +Y. The vertex program (every pass) bends
//!   the blade: h = 1 − v, a = sin(x + time)·h·windStrength,
//!   b = sin(z + time + 0.5)·h·windStrength, x += 0.075a + 0.025b,
//!   z += 0.025a + 0.075b; then the offset field d (characters pushing blades)
//!   moves xz by h·d and lowers y by H − √(H² − |h·d|²), H = `bladeHeight`.
//!   `TerrainType::SetGrassShaderParams`: `windStrength` = the
//!   `WeatherManager` wind strength (PORT: carried in the entity's `MeshTag`; `WeatherManager.windStrength`,
//!   `GetWindStrength`: 1 by default, the weather's eased 1..3 factor) when `affectedByWind`, else 0.
//! - The grass is part of the terrain, which casts no sun shadow.
//!
//! - The offset field (`GrassObject`, [`GrassField`]): per terrain
//!   block `int(w / s)` × `int(h / s)` points `s` (the spacing) apart from its
//!   corner (w, h its size); a blade reads the point `clamp(int((x − x0) / s))`,
//!   `clamp(int((z − z0) / s))` (`SetupGrassGeometry`), the same for its 4
//!   vertices. A point holds a disturbance offset / velocity and a wind offset /
//!   velocity. `Terrain::CreateGrassWind` (the gusts,
//!   `World::CreateDirectionalWind`) calls `GrassObject::AddWind`
//!   on each block whose box meets the cube of the radius about the point: every
//!   point closer than r gets wind velocity += (1 − d / r)·strength·(dir.x,
//!   dir.y) (dir.y of the gusts' (x, 0, z) is 0: they push along x only),
//!   clamped to ±0.3, and is held 0.1 s; the block animates the rectangle of
//!   pushed points for 5000 ms. `UpdateOffsetField` (each frame
//!   while animating) first writes the vertex offsets (disturbance + wind on a
//!   layer `affectedByWind`, else disturbance only), then steps each point with
//!   h = min(dt, 0.05 s): hold −= h; unheld: wind velocity = (v − 50·offset·h)·0.91;
//!   wind offset = clamp(v·h + offset, ±0.5); disturbance velocity = (v −
//!   50·offset·h)·0.95, offset likewise. The VS applies it (UV_1).
//! - Characters push the blades aside ([`character_disturbances`],
//!   Game.dll `CharacterMovementManager::Update`: each update of a
//!   character in action state Move (5), Walk (6), Jump (19), 20 or Evade (21)
//!   calls `World::CreatePointDisturbance` (Engine.dll) at its coords
//!   with radius `Character::GetExtents` (`pathingSize` Small 0.5,
//!   Medium 1, Large 1.75), strength 3, falloff on, splash type 1 for the player
//!   (0 else; water ripples only). Per region whose box grown by the radius holds
//!   the point, the terrain's `Terrain::CreateGrassDisturbance` walks the
//!   blocks like the wind (cube of the radius) into `GrassObject::AddDisturbance`:
//!   every point 0.0001 ≤ d < r from it gets disturbance velocity +=
//!   (dx, dz) / d · strength · (1 − d / r) (outwards), clamped to ±2, no hold; the
//!   same rectangle growth and 5000 ms timer as the wind.
//!   The message is [`gd_game_core::visuals::PointDisturbance`]; its water half is `Water::CreateRipple`. The other
//!   callers (all Game.dll, falloff on):
//!   - `FxPak::OnAddToLevel`: `disturbanceRadius` > 0.0001, strength 5, splash 0;
//!   - `Destructible::CollisionCallback`: a debris piece's contact
//!     at most every 250 ms, radius 1, strength √(|v|² + |ω|²), splash 2 on water
//!     (surface 7), else 0;
//!   - `Floater::UpdateSelf`: each bob turn, radius |the region box's
//!     half extents|, strength 1, splash 0;
//!   - `World::CreateLineDisturbance` (a wave's front each frame,
//!     strength 2, splash 0): radius-1 point disturbances 1 m
//!     apart along the line ([`line_disturbances`]);
//!   - `Skill_AttackLine::Update` (each `hitPeriod` at the travelling
//!     point, radius `attackRadius`, strength 5, splash 1);
//!   - not ported: `Character::CollisionCallback` (ragdolls: radius 1, strength
//!     the impact speed; the port has none), and `Fx::AnimationCallback` ("DisplaceGrass", radius
//!     the `FxMesh`'s `disturbanceRadius`, strength 5: no stock `FxMesh` animation
//!     has that callback, only creature ones, which `Fx` never plays).
//!
//! PORT: the block box's height is its ground's range; one
//! mesh per layer and 4 × 4 blocks ([`CHUNK`]); the blended pass is drawn before SSAO with the decals;
//! no alpha to coverage (no MSAA); `clutterMesh` (5 stock records) not placed;
//! the base layer's opacity is 255 everywhere. The offsets are not written into the
//! blade vertices: every loaded field lives in one storage buffer ([`GrassOffsets`]), a
//! blade vertex's `UV_1` holds its point's index there, and each frame only the animated
//! blocks' points are written to the GPU.
use super::material::MaterialAssets;
use crate::textures::Textures;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, MeshTag, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::Extract;
use bevy::render::render_asset::RenderAssets;
use bevy::render::renderer::RenderQueue;
use bevy::render::storage::{GpuShaderBuffer, ShaderBuffer};
use gd_formats::map::terrain::Terrain;
use gd_game_core::data::GameData;

/// `kGrassHighQualitySpacing`.
const SPACING: f32 = 0.58;
/// The low terrain quality's spacing.
const LOW_SPACING: f32 = 1.16;
/// The lowest blades that grow at the low terrain quality.
const LOW_MIN_HEIGHT: f32 = 1.65;

/// The terrain quality is above 0 (`detailLevel` not "low"). Like `textureQuality`, read by what is built from now
/// on.
#[derive(Resource)]
pub struct GrassQuality(pub bool);

impl Default for GrassQuality {
    fn default() -> Self {
        Self(true)
    }
}

/// Blades per block and layer (`CreateGrassBuffers`, terrain case).
const MAX_PER_BLOCK: usize = 350;
const BLOCK: u32 = 9;

/// A `TerrainType`'s blade parameters.
#[derive(Clone, Debug)]
pub struct GrassType {
    pub texture: String,
    pub width: f32,
    pub height: f32,
    pub spacing: u32,
    pub variations: u32,
    pub min_opacity: u32,
    pub probability: Vec<i32>,
    pub clumpiness: Vec<i32>,
    /// `affectedByWind`.
    pub wind: bool,
    /// A `clutterMesh` is set.
    pub clutter: bool,
}

impl GrassType {
    pub fn from_record(rec: &gd_formats::arz::Record) -> Option<Self> {
        let texture = rec.str("BladeTextureFileName").filter(|s| !s.is_empty())?;
        let ints = |k: &str| -> Vec<i32> {
            rec.str(k)
                .map(|s| {
                    s.split(';')
                        .filter_map(|v| v.trim().parse::<f32>().ok())
                        .map(|f| f as i32)
                        .collect()
                })
                .unwrap_or_default()
        };
        Some(Self {
            texture: texture.to_owned(),
            width: rec.f32("BladeWidth").unwrap_or(0.0),
            height: rec.f32("BladeHeight").unwrap_or(0.0),
            spacing: rec.i32("BladeSpacing").unwrap_or(0).max(0) as u32,
            variations: rec.i32("BladeVariations").unwrap_or(1).max(1) as u32,
            min_opacity: rec.i32("MinimumBladeOpacity").unwrap_or(0).max(0) as u32,
            probability: ints("VariationProbability"),
            clumpiness: ints("VariationClumpiness"),
            wind: rec.i32("affectedByWind").unwrap_or(0) != 0,
            clutter: rec.str("clutterMesh").is_some_and(|s| !s.is_empty()),
        })
    }

    /// `GetRandomTexture`.
    fn random_texture(&self, seed: i32) -> i32 {
        let total: i32 = self.probability.iter().sum();
        if total == 0 {
            return 0;
        }
        let v = seed.wrapping_rem(total);
        let mut sum = 0;
        for (i, p) in self.probability.iter().enumerate() {
            sum += p;
            if v <= sum {
                return i as i32;
            }
        }
        self.probability.len() as i32
    }

    /// `ClumpFind`.
    fn clump_find(&self, grid: &[i32], start: usize, w: i32, h: i32) -> Option<usize> {
        const N: [(i32, i32); 8] = [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ];
        let mut i = start;
        loop {
            let v = grid[i];
            let (ix, iz) = (i as i32 % w, i as i32 / w);
            let same = N
                .iter()
                .filter(|(dx, dz)| {
                    let x = (ix + dx).rem_euclid(w);
                    let z = (iz + dz).rem_euclid(h);
                    let n = (w * z + x) as usize;
                    n < grid.len() && grid[n] == v
                })
                .count() as i32;
            if v != -1 && same < self.clumpiness.get(v as usize).copied().unwrap_or(0) {
                return Some(i);
            }
            i = if i + 1 == grid.len() { 0 } else { i + 1 };
            if i == start {
                return None;
            }
        }
    }

    /// `PerformClumping`.
    fn clump(&self, seed: u32, grid: &mut [i32], w: i32, h: i32) {
        if self.clumpiness.len() != self.variations as usize || grid.is_empty() {
            return;
        }
        let n = grid.len() as u32;
        let mut v = park_miller(seed);
        for _ in 0..256 {
            let a = park_miller(v);
            let Some(i) = self.clump_find(grid, (a % n) as usize, w, h) else {
                break;
            };
            v = park_miller(a);
            let Some(j) = self.clump_find(grid, (v % n) as usize, w, h) else {
                break;
            };
            grid.swap(i, j);
        }
    }
}

/// Thomas Wang's 32-bit integer hash (the engine's inlined one).
pub fn wang(k: u32) -> u32 {
    let mut k = k.wrapping_add(!(k << 15));
    k ^= k >> 10;
    k = k.wrapping_mul(9);
    k ^= k >> 6;
    k = k.wrapping_add(!(k << 11));
    k ^ (k >> 16)
}

/// One minimal-standard step, 16807 · v mod (2³¹ − 1) (Schrage form).
fn park_miller(v: u32) -> u32 {
    let r = 16807u32
        .wrapping_mul(v)
        .wrapping_sub(0x7FFF_FFFFu32.wrapping_mul(v / 0x1F31D));
    if (r as i32) < 0 {
        r.wrapping_add(0x7FFF_FFFF)
    } else {
        r
    }
}

/// `GetLayerOpacityInterpolated` (0 ..= 255; the base layer is full).
fn layer_opacity(t: &Terrain, layer: usize, x: f32, z: f32) -> f32 {
    let Some(w) = t.layers.get(layer).and_then(|l| l.weights.as_ref()) else {
        return 255.0;
    };
    let (cw, ch) = (t.width - 1, t.height - 1);
    let (x0, z0) = (x as u32, z as u32);
    let x1 = if x0 + 1 >= cw { x0 } else { x0 + 1 };
    let z1 = if z0 + 1 >= ch { z0 } else { z0 + 1 };
    let at = |x: u32, z: u32| w.get((x + z * cw) as usize).copied().unwrap_or(0) as f32;
    let fx = x - x0 as f32;
    let a = (at(x1, z0) - at(x0, z0)) * fx + at(x0, z0);
    let b = (at(x1, z1) - at(x0, z1)) * fx + at(x0, z1);
    (((b - a) * (z - z0 as f32)) + a) as i32 as f32
}

/// The blades of one layer in one chunk of blocks: texture and mesh (terrain-local coordinates).
pub struct GrassBatch {
    pub texture: String,
    pub mesh: Mesh,
    /// `affectedByWind`.
    pub wind: bool,
    /// `BladeHeight` (the shader's `bladeHeight`).
    pub height: f32,
    /// Per vertex, its blade's point of the offset field ([`GrassField`]).
    field: Vec<u32>,
}

/// A terrain's grass: the batches and the blocks' offset fields.
#[derive(Default)]
pub struct TerrainGrass {
    pub batches: Vec<GrassBatch>,
    field: GrassField,
}

/// Blocks per side of a mesh chunk. PORT: the engine keeps buffers per terrain block.
const CHUNK: u32 = 4;

/// All grass of a terrain at the terrain quality `high`, one
/// batch per grass layer and chunk.
pub fn build(t: &Terrain, data: &GameData, high: bool) -> TerrainGrass {
    let spacing = if high { SPACING } else { LOW_SPACING };
    let types: Vec<Option<GrassType>> = t
        .layers
        .iter()
        .map(|l| data.db.get(&l.name).and_then(GrassType::from_record))
        .collect();
    let mut out = TerrainGrass::default();
    let bx = (t.width - 1).div_ceil(BLOCK);
    let bz = (t.height - 1).div_ceil(BLOCK);
    out.field = GrassField::layout(t, spacing, bx, bz);
    for (li, ty) in types.iter().enumerate() {
        let Some(ty) = ty else { continue };
        if !high && (ty.height < LOW_MIN_HEIGHT || ty.clutter) {
            continue;
        }
        for cj in (0..bz).step_by(CHUNK as usize) {
            for ci in (0..bx).step_by(CHUNK as usize) {
                let mut pos: Vec<[f32; 3]> = Vec::new();
                let mut uv: Vec<[f32; 2]> = Vec::new();
                let mut field = Vec::new();
                for j in cj..(cj + CHUNK).min(bz) {
                    for i in ci..(ci + CHUNK).min(bx) {
                        let fb = &out.field.blocks[(i + j * bx) as usize];
                        block(t, li, ty, spacing, i, j, fb, &mut pos, &mut uv, &mut field);
                    }
                }
                if pos.is_empty() {
                    continue;
                }
                let n = pos.len();
                let idx: Vec<u32> = (0..n as u32 / 4)
                    .flat_map(|q| {
                        let b = q * 4;
                        [b, b + 1, b + 2, b, b + 2, b + 3]
                    })
                    .collect();
                // UV_1 gets the blades' indices into `GrassOffsets` once the field has its range there.
                let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::all())
                    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
                    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n])
                    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
                    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.0f32; 2]; n])
                    .with_inserted_indices(Indices::U32(idx));
                out.batches.push(GrassBatch {
                    texture: ty.texture.clone(),
                    mesh,
                    wind: ty.wind,
                    height: ty.height,
                    field,
                });
            }
        }
    }
    out
}

/// A grass pass entity; `true` = its layer is `affectedByWind`.
#[derive(Component)]
pub struct GrassWind(pub bool);

/// Spawns each batch's two passes and the terrain's offset field under the level `root`.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    commands: &mut Commands,
    root: Entity,
    origin: Vec3,
    grass: TerrainGrass,
    data: &GameData,
    textures: &mut Textures,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    mats: &mut MaterialAssets,
) {
    let mut field = grass.field;
    field.origin = origin;
    // One material pair per layer texture, shared by its chunks and by every level.
    for g in grass.batches {
        let mesh = meshes.add(g.mesh);
        for fringe in [false, true] {
            let mat = mats.grass(data, &g.texture, fringe, g.height, textures, images);
            let e = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(mat),
                    Transform::from_translation(origin),
                    bevy::light::NotShadowCaster,
                    GrassWind(g.wind),
                    MeshTag::new(0),
                    Name::new("terrain grass"),
                ))
                .id();
            if fringe {
                commands
                    .entity(e)
                    .insert(super::gbuffer_blend::GBufferBlend::GrassFringe);
            }
            commands.entity(root).add_child(e);
        }
        field.meshes.push(FieldMesh {
            mesh,
            wind: g.wind,
            index: g.field,
        });
    }
    if !field.meshes.is_empty() {
        let e = commands
            .spawn((field, Name::new("terrain grass field")))
            .id();
        commands.entity(root).add_child(e);
    }
}

/// `SetGrassShaderParams`' `windStrength`: the weather's wind strength (1
/// without weather) on the layers `affectedByWind`, 0 on the others. PORT: the
/// f32 bits ride in the entity's `MeshTag`, not the material bind group.
pub fn grass_wind(
    weather: Option<Res<crate::render::weather::WeatherState>>,
    mut grass: ParamSet<(
        Query<(&GrassWind, &mut MeshTag)>,
        Query<(&GrassWind, &mut MeshTag), Added<GrassWind>>,
    )>,
    mut last: Local<Option<f32>>,
) {
    let strength = weather.map_or(1.0, |w| w.wind_strength());
    let set = |(wind, mut tag): (&GrassWind, Mut<MeshTag>)| {
        let want = if wind.0 { strength } else { 0.0 }.to_bits();
        if tag.value != want {
            tag.value = want;
        }
    };
    if last.replace(strength) == Some(strength) {
        grass.p1().iter_mut().for_each(set);
    } else {
        grass.p0().iter_mut().for_each(set);
    }
}

/// `Terrain::CreateGrassWind(pos, dir, radius, strength, falloff)` (world space).
#[derive(Clone, Copy, Debug)]
pub struct GrassWindPush {
    pub pos: Vec3,
    pub dir: Vec3,
    pub radius: f32,
    pub strength: f32,
    pub falloff: bool,
}

use gd_game_core::visuals::PointDisturbance;

/// Game.dll `CharacterMovementManager::Update`'s strength (`Game::MoveDisturbanceStrength`).
const MOVE_STRENGTH: f32 = 3.0;

/// A character's grass-disturbance facts: the `GetExtents` radius and a memo of the last cast skill's
/// leap-class verdict.
#[derive(Component)]
pub struct MoveDisturbance {
    radius: f32,
    leap: Option<(String, bool)>,
}

/// Game.dll `CharacterMovementManager::Update`: every update of a character in
/// action state Move, Walk, Jump, Evade (or the unnamed 20) disturbs the grass at its coords with its
/// `GetExtents` radius, strength 3, falloff on, splash 1 for the player, else 0 (its ms timer is reset to 1, so it
/// fires each update). Jump (19) is a leap's: `JumpAttackAction::Execute` →
/// `Character::JumpToUseSkill` sets it, and it holds until the next action's (the jump's `StopJump`
/// only stops the move), so the whole leap cast disturbs. PORT: the port's states Run, Walk, Charge (a move),
/// Evade, and Cast while its skill (the [`gd_game_core::casting::Casting`], after the hit the
/// [`gd_game_core::casting::LastCast`]) is a leap class.
pub fn character_disturbances(
    data: Res<GameData>,
    mut commands: Commands,
    mut chars: Query<(
        Entity,
        &gd_game_core::character::Character,
        &GlobalTransform,
        Option<&gd_game_core::casting::Casting>,
        Option<&gd_game_core::casting::LastCast>,
        Option<&mut MoveDisturbance>,
    )>,
    mut out: MessageWriter<PointDisturbance>,
) {
    use gd_game_core::character::CharState;
    for (e, c, tf, cast, last, mut dist) in &mut chars {
        let moving_state = matches!(
            c.state,
            CharState::Run | CharState::Walk | CharState::Charge | CharState::Evade
        );
        if !moving_state && !matches!(c.state, CharState::Cast) {
            continue;
        }
        let radius = match dist.as_mut() {
            Some(d) => d.radius,
            None => {
                let r = gd_game_core::pets::placement::extents(data.db.get(&c.record));
                commands.entity(e).insert(MoveDisturbance {
                    radius: r,
                    leap: None,
                });
                r
            }
        };
        let moving = moving_state
            || match cast.map(|k| &k.skill).or(last.map(|l| &l.skill)) {
                Some(skill) => {
                    let memo = dist
                        .as_ref()
                        .and_then(|d| d.leap.as_ref())
                        .filter(|(s, _)| s.as_str() == skill.as_str())
                        .map(|(_, leap)| *leap);
                    match memo {
                        Some(leap) => leap,
                        None => {
                            let leap = data
                                .db
                                .get(skill)
                                .is_some_and(|r| gd_game_core::casting::is_leap(&r.class));
                            if let Some(d) = dist.as_mut() {
                                d.leap = Some((skill.clone(), leap));
                            }
                            leap
                        }
                    }
                }
                None => false,
            };
        if !moving {
            continue;
        }
        out.write(PointDisturbance {
            pos: tf.translation(),
            radius,
            strength: MOVE_STRENGTH,
            falloff: true,
            splash: c.is_player as u8,
        });
    }
}

/// `World::CreateLineDisturbance`: its point disturbances, for the grass and the water.
pub fn line_disturbances(
    mut lines: MessageReader<gd_game_core::visuals::LineDisturbance>,
    mut out: MessageWriter<PointDisturbance>,
) {
    for l in lines.read() {
        out.write_batch(l.points());
    }
}

/// One point of a block's offset field (`GrassObject`, 36 bytes).
#[derive(Clone, Copy, Default, Debug, PartialEq)]
struct FieldPoint {
    /// `GrassPoint.offset`: the disturbance offset ([`PointDisturbance`]).
    off: Vec2,
    /// `GrassPoint.velocity`: its velocity.
    vel: Vec2,
    /// `GrassPoint.windOffset`: the wind offset.
    woff: Vec2,
    /// `GrassPoint.windVelocity`: its velocity.
    wvel: Vec2,
    /// `GrassPoint.windHold`: seconds the wind still holds it.
    hold: f32,
}

/// One terrain block's field: `int(w / s)` × `int(h / s)` points `s` apart from the block's corner.
#[derive(Clone, Debug)]
struct FieldBlock {
    start: usize,
    cols: i32,
    rows: i32,
    /// The block's corner (terrain-local x / z) and its ground's height range.
    corner: Vec2,
    size: Vec2,
    heights: (f32, f32),
    /// x0, x1, z0, z1 of the points being animated (`NONE` in x0: none).
    rect: [i32; 4],
    /// ms the field still animates (`GrassObject.animTimer`).
    timer_ms: i32,
}

const NONE: i32 = 1_000_000;

/// A mesh chunk's link to the field, until the field has its range in [`GrassOffsets`].
struct FieldMesh {
    mesh: Handle<Mesh>,
    wind: bool,
    /// Per vertex, its field point.
    index: Vec<u32>,
}

/// The offset fields of a terrain's blocks and the meshes that read them.
#[derive(Component, Default)]
pub struct GrassField {
    /// World position of the terrain's corner.
    origin: Vec3,
    spacing: f32,
    blocks: Vec<FieldBlock>,
    points: Vec<FieldPoint>,
    /// Its first point in [`GrassOffsets`] (`None` until `grass_offsets` places it).
    base: Option<u32>,
    /// The meshes whose `UV_1` gets the points' indices once placed.
    meshes: Vec<FieldMesh>,
    /// Blocks per row / column of the layout grid (the push's block-index walk).
    blocks_x: u32,
    blocks_z: u32,
    /// The terrain's max xz corner and the ground height range over all blocks, for
    /// rejecting a push that meets no block.
    extent: Vec2,
    heights: (f32, f32),
}

impl GrassField {
    fn layout(t: &Terrain, spacing: f32, bx: u32, bz: u32) -> Self {
        let inv = 1.0 / spacing;
        let mut f = GrassField {
            spacing,
            ..default()
        };
        let mut gh = (f32::INFINITY, f32::NEG_INFINITY);
        for j in 0..bz {
            for i in 0..bx {
                let (x0, z0) = (BLOCK * i, BLOCK * j);
                let (x1, z1) = (
                    (x0 + BLOCK).min(t.width - 1),
                    (z0 + BLOCK).min(t.height - 1),
                );
                let (w, h) = ((x1 - x0) as f32, (z1 - z0) as f32);
                // PORT: at least one point (a 1 m edge block at the low quality would have none).
                let (cols, rows) = (((w * inv) as i32).max(1), ((h * inv) as i32).max(1));
                let mut heights = (f32::INFINITY, f32::NEG_INFINITY);
                for z in z0..=z1 {
                    for x in x0..=x1 {
                        let y = t.height_at_vertex(x, z);
                        heights = (heights.0.min(y), heights.1.max(y));
                    }
                }
                gh = (gh.0.min(heights.0), gh.1.max(heights.1));
                f.blocks.push(FieldBlock {
                    start: f.points.len(),
                    cols,
                    rows,
                    corner: Vec2::new(x0 as f32, z0 as f32),
                    size: Vec2::new(w, h),
                    heights,
                    rect: [NONE, 0, NONE, 0],
                    timer_ms: 0,
                });
                f.points.resize(
                    f.points.len() + (cols * rows) as usize,
                    FieldPoint::default(),
                );
            }
        }
        f.blocks_x = bx;
        f.blocks_z = bz;
        f.extent = Vec2::new((t.width - 1) as f32, (t.height - 1) as f32);
        f.heights = gh;
        f
    }

    /// The field point of a blade at terrain-local (x, z) in block `b` (`SetupGrassGeometry`).
    fn point_of(b: &FieldBlock, spacing: f32, x: f32, z: f32) -> u32 {
        let inv = 1.0 / spacing;
        let c = (((x - b.corner.x) * inv) as i32).max(0).min(b.cols - 1);
        let r = (((z - b.corner.y) * inv) as i32).max(0).min(b.rows - 1);
        (b.start + (c + b.cols * r) as usize) as u32
    }

    /// The pushes' shared walk (`Terrain::CreateGrassWind` / `Terrain::CreateGrassDisturbance`, then
    /// `GrassObject::AddWind` / `AddDisturbance`): every block whose box (ground heights)
    /// meets the cube of `r` about `p` (terrain-local) grows its animated rectangle by the points
    /// `int((p − r) / s)` ..= `int((p + r) / s + 1)` (clamped), `push` runs on each of them closer than `r` with
    /// its offset from `p` (dx, dz) and distance, and the block animates for 5000 ms.
    fn push_points(&mut self, p: Vec3, r: f32, mut push: impl FnMut(&mut FieldPoint, Vec2, f32)) {
        let s = self.spacing;
        let inv = 1.0 / s;
        // The grid cells that can meet the push cube.
        // Block (i, j) spans x [9i, min(9i+9, w−1)] (z likewise); the −1 slackens the lower
        // bound for the clipped edge block — the exact box test below still gates each one.
        let bx = self.blocks_x as i32;
        let bz = self.blocks_z as i32;
        let cell = BLOCK as f32;
        let i0 = (((p.x - r) / cell).floor() as i32).saturating_sub(1).max(0);
        let i1 = (((p.x + r) / cell).floor() as i32).min(bx - 1);
        let j0 = (((p.z - r) / cell).floor() as i32).saturating_sub(1).max(0);
        let j1 = (((p.z + r) / cell).floor() as i32).min(bz - 1);
        for j in j0..=j1 {
            for i in i0..=i1 {
                let b = &mut self.blocks[(j * bx + i) as usize];
                let (lo, hi) = (b.corner, b.corner + b.size);
                let centre_y = (b.heights.0 + b.heights.1) * 0.5;
                let half_y = (b.heights.1 - b.heights.0) * 0.5;
                if p.x + r < lo.x
                    || p.x - r > hi.x
                    || p.z + r < lo.y
                    || p.z - r > hi.y
                    || (p.y - centre_y).abs() > half_y + r
                {
                    continue;
                }
                let (px, pz) = (p.x - b.corner.x, p.z - b.corner.y);
                let c0 = (((px - r) * inv) as i32).max(0);
                let c1 = (((px + r) * inv + 1.0) as i32).min(b.cols - 1);
                let r0 = (((pz - r) * inv) as i32).max(0);
                let r1 = (((pz + r) * inv + 1.0) as i32).min(b.rows - 1);
                b.rect = [
                    b.rect[0].min(c0),
                    b.rect[1].max(c1),
                    b.rect[2].min(r0),
                    b.rect[3].max(r1),
                ];
                for row in r0..=r1 {
                    for col in c0..=c1 {
                        let off = Vec2::new(col as f32 * s - px, row as f32 * s - pz);
                        let d = off.length();
                        if d < r {
                            push(
                                &mut self.points[b.start + (row * b.cols + col) as usize],
                                off,
                                d,
                            );
                        }
                    }
                }
                b.timer_ms = 5000;
            }
        }
    }

    /// Whether a push at terrain-local `p` with radius `r` can meet any block: the cube
    /// against the field's whole box. A superset of the per-block tests, so it never rejects a push that moves a
    /// point.
    fn touches(&self, p: Vec3, r: f32) -> bool {
        let centre_y = (self.heights.0 + self.heights.1) * 0.5;
        let half_y = (self.heights.1 - self.heights.0) * 0.5;
        p.x + r >= 0.0
            && p.x - r <= self.extent.x
            && p.z + r >= 0.0
            && p.z - r <= self.extent.y
            && (p.y - centre_y).abs() <= half_y + r
    }

    /// `Terrain::CreateGrassWind` → `GrassObject::AddWind`.
    fn add_wind(&mut self, p: Vec3, w: &GrassWindPush) {
        let r = w.radius;
        self.push_points(p, r, |pt, _, d| {
            let e = if w.falloff {
                (1.0 - d / r) * w.strength
            } else {
                w.strength
            };
            // The direction's x and y (not z): a gust's horizontal direction (x, 0, z) pushes
            // along x only.
            pt.wvel.x = (pt.wvel.x + e * w.dir.x).clamp(-0.3, 0.3);
            pt.wvel.y = (pt.wvel.y + e * w.dir.y).clamp(-0.3, 0.3);
            pt.hold = 0.1;
        });
    }

    /// `Terrain::CreateGrassDisturbance` → `GrassObject::AddDisturbance`:
    /// points 0.0001 ≤ d < r get disturbance velocity += (dx, dz) / d · strength (· (1 − d / r) with the falloff),
    /// clamped to ±2; no hold.
    fn add_disturbance(&mut self, p: Vec3, g: &PointDisturbance) {
        let r = g.radius;
        self.push_points(p, r, |pt, off, d| {
            if d < 0.0001 {
                return;
            }
            let mut e = off / d * g.strength;
            if g.falloff {
                e *= 1.0 - d / r;
            }
            pt.vel = (pt.vel + e).clamp(Vec2::splat(-2.0), Vec2::splat(2.0));
        });
    }

    /// `GrassObject::UpdateOffsetField`'s integration of the animated points.
    fn step(&mut self, dt_ms: i32) {
        let h = (dt_ms as f32 * 0.001).min(0.05);
        for b in &mut self.blocks {
            if b.rect[0] == NONE {
                continue;
            }
            for row in b.rect[2]..=b.rect[3] {
                for col in b.rect[0]..=b.rect[1] {
                    let Some(p) = self.points.get_mut(b.start + (row * b.cols + col) as usize)
                    else {
                        continue;
                    };
                    p.hold = (p.hold - h).max(0.0);
                    if p.hold <= 0.0001 {
                        p.wvel = (p.wvel + p.woff * -50.0 * h) * 0.91;
                    }
                    p.woff = (p.wvel * h + p.woff).clamp(Vec2::splat(-0.5), Vec2::splat(0.5));
                    p.vel = (p.off * -50.0 * h + p.vel) * 0.95;
                    p.off = (p.vel * h + p.off).clamp(Vec2::splat(-0.5), Vec2::splat(0.5));
                }
            }
            b.timer_ms -= dt_ms;
            if b.timer_ms <= 0 {
                b.rect = [NONE, 0, NONE, 0];
            }
        }
    }
}

/// Bytes per point in [`GrassOffsets`]' buffer: the disturbance offset, then the disturbance + wind offset.
const POINT_BYTES: u64 = 16;
/// The buffer's first size in points (1 MB); it doubles when the loaded fields outgrow it. Indices travel as
/// `f32` in `UV_1`, exact below 2^24 points.
const FIRST_CAPACITY: u32 = 1 << 16;

/// The offset fields of every loaded terrain in one storage buffer that every grass material reads (binding 14 of
/// [`super::GdMaterial`]): a blade vertex's `UV_1` is (its point's index in the buffer, 1 when its
/// layer is `affectedByWind`). Each field holds a range ([`GrassField::base`]); `writes` are the frame's changed
/// points, which the render world copies into the GPU buffer ([`write_offsets`]). A despawned field's range is
/// released by [`release_field`].
#[derive(Resource)]
pub struct GrassOffsets {
    pub buffer: Handle<ShaderBuffer>,
    capacity: u32,
    /// Free point ranges (start, len), sorted by start.
    free: Vec<(u32, u32)>,
    /// This frame's writes.
    writes: GrassWrites,
}

impl GrassOffsets {
    pub fn new(buffers: &mut Assets<ShaderBuffer>) -> Self {
        Self {
            buffer: buffers.add(Self::gpu(FIRST_CAPACITY)),
            capacity: FIRST_CAPACITY,
            free: vec![(0, FIRST_CAPACITY)],
            writes: GrassWrites::default(),
        }
    }

    /// A zeroed GPU buffer of `points`, written to only through [`write_offsets`].
    fn gpu(points: u32) -> ShaderBuffer {
        ShaderBuffer::with_size(points as u64 * POINT_BYTES, RenderAssetUsages::RENDER_WORLD)
    }

    /// First fit.
    fn alloc(&mut self, len: u32) -> Option<u32> {
        let i = self.free.iter().position(|&(_, l)| l >= len)?;
        let (start, l) = self.free[i];
        if l == len {
            self.free.remove(i);
        } else {
            self.free[i] = (start + len, l - len);
        }
        Some(start)
    }

    fn release(&mut self, start: u32, len: u32) {
        let mut i = self.free.partition_point(|&(s, _)| s < start);
        self.free.insert(i, (start, len));
        if i + 1 < self.free.len() && start + len == self.free[i + 1].0 {
            self.free[i].1 += self.free[i + 1].1;
            self.free.remove(i + 1);
        }
        if i > 0 && self.free[i - 1].0 + self.free[i - 1].1 == start {
            self.free[i - 1].1 += self.free[i].1;
            self.free.remove(i);
            i -= 1;
        }
        debug_assert!(self.free[i].0 + self.free[i].1 <= self.capacity);
    }

    /// Room for `len` more points: a bigger, zeroed GPU buffer (every placed field is written again).
    fn grow(&mut self, len: u32, buffers: &mut Assets<ShaderBuffer>) {
        let old = self.capacity;
        self.capacity = (old * 2).max(old + len);
        self.release(old, self.capacity - old);
        if let Some(mut b) = buffers.get_mut(&self.buffer) {
            *b = Self::gpu(self.capacity);
        }
    }

    fn write(&mut self, start: u32, points: &[FieldPoint]) {
        self.writes.ranges.push((start, points.len() as u32));
        self.writes.values.extend(points.iter().map(|p| {
            let w = p.off + p.woff;
            [p.off.x, p.off.y, w.x, w.y]
        }));
    }
}

/// A frame's offset writes: `ranges` (first point in the buffer, count) over the consecutive `values`.
#[derive(Default, Clone)]
pub struct GrassWrites {
    ranges: Vec<(u32, u32)>,
    values: Vec<[f32; 4]>,
}

impl GrassWrites {
    fn clear(&mut self) {
        self.ranges.clear();
        self.values.clear();
    }
}

/// Gives a despawned field's range back (the component is still readable in `Remove`).
pub fn release_field(
    ev: On<Remove<GrassField>>,
    fields: Query<&GrassField>,
    offsets: Option<ResMut<GrassOffsets>>,
) {
    if let Ok(f) = fields.get(ev.entity)
        && let Some(base) = f.base
        && let Some(mut offsets) = offsets
    {
        offsets.release(base, f.points.len() as u32);
    }
}

/// The gusts' and the disturbances' grass pushes, then each animated field: its offsets out to the GPU
/// (`UpdateOffsetField` writes the vertex offsets before it steps the points), then the step. A new field first gets
/// its range in [`GrassOffsets`] and its meshes the points' indices.
#[allow(clippy::too_many_arguments)]
pub fn grass_offsets(
    weather: Option<ResMut<crate::render::weather::WeatherState>>,
    mut disturbances: MessageReader<PointDisturbance>,
    time: Res<Time<Virtual>>,
    mut fields: Query<&mut GrassField>,
    mut offsets: ResMut<GrassOffsets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut carry: Local<f32>,
    mut pushes: Local<Vec<GrassWindPush>>,
    mut hits: Local<Vec<PointDisturbance>>,
) {
    let offsets = &mut *offsets;
    offsets.writes.clear();
    let mut grown = false;
    for mut f in &mut fields {
        if f.base.is_some() {
            continue;
        }
        let len = f.points.len() as u32;
        let base = match offsets.alloc(len) {
            Some(b) => b,
            None => {
                offsets.grow(len, &mut buffers);
                grown = true;
                offsets.alloc(len).expect("grown to fit")
            }
        };
        f.base = Some(base);
        for m in std::mem::take(&mut f.meshes) {
            if let Some(mut mesh) = meshes.get_mut(&m.mesh) {
                // Only a layer `affectedByWind` (`TerrainType.affectedByWind`) adds the wind offset.
                let wind = if m.wind { 1.0 } else { 0.0 };
                let uv: Vec<[f32; 2]> =
                    m.index.iter().map(|&i| [(base + i) as f32, wind]).collect();
                mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv);
            }
        }
        // A reused range still holds an old field's offsets.
        offsets.write(base, &f.points);
    }
    if grown {
        // Every placed field is written whole below, this frame's new ones included.
        offsets.writes.clear();
        for f in &fields {
            if let Some(base) = f.base {
                offsets.write(base, &f.points);
            }
        }
    }
    // Swap the weather's pushes out, leaving it this frame's cleared buffer.
    pushes.clear();
    if let Some(mut w) = weather {
        std::mem::swap(&mut w.grass_winds, &mut *pushes);
    }
    let pushes = &*pushes;
    hits.clear();
    hits.extend(disturbances.read().copied());
    let disturbances = &*hits;
    // Whole milliseconds with the remainder carried, so the wind is not slow at high refresh.
    let ms = time.delta_secs() * 1000.0 + *carry;
    let dt_ms = ms.floor();
    *carry = ms - dt_ms;
    let dt_ms = dt_ms as i32;
    for mut f in &mut fields {
        for w in pushes {
            let p = w.pos - f.origin;
            if f.touches(p, w.radius) {
                f.add_wind(p, w);
            }
        }
        for g in disturbances {
            let p = g.pos - f.origin;
            if f.touches(p, g.radius) {
                f.add_disturbance(p, g);
            }
        }
        if f.blocks.iter().all(|b| b.rect[0] == NONE) {
            continue;
        }
        if !pushes.is_empty() || !disturbances.is_empty() {
            let peak = |o: fn(&FieldPoint) -> f32| f.points.iter().map(o).fold(0.0, f32::max);
            debug!(
                "grass field: {} pushes, {} disturbances, {} blocks animated, peak wind offset {:.3}, \
                 disturbance offset {:.3}",
                pushes.len(),
                disturbances.len(),
                f.blocks.iter().filter(|b| b.rect[0] != NONE).count(),
                peak(|p| p.woff.length()),
                peak(|p| p.off.length()),
            );
        }
        let f = &mut *f;
        if let Some(base) = f.base {
            for b in f.blocks.iter().filter(|b| b.rect[0] != NONE) {
                let n = (b.cols * b.rows) as usize;
                offsets.write(base + b.start as u32, &f.points[b.start..b.start + n]);
            }
        }
        f.step(dt_ms);
    }
}

/// [`GrassOffsets`]' writes of the frame, in the render world.
#[derive(Resource, Default)]
pub struct ExtractedGrassWrites {
    buffer: Option<AssetId<ShaderBuffer>>,
    writes: GrassWrites,
}

pub fn extract_offsets(mut out: ResMut<ExtractedGrassWrites>, offsets: Extract<Res<GrassOffsets>>) {
    out.buffer = Some(offsets.buffer.id());
    out.writes.clone_from(&offsets.writes);
}

/// Copies the frame's writes into the GPU buffer, after `prepare_assets` made it (a grown buffer starts zeroed and
/// is written whole the frame it grows).
pub fn write_offsets(
    w: Res<ExtractedGrassWrites>,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
    queue: Res<RenderQueue>,
) {
    let Some(gpu) = w.buffer.and_then(|b| buffers.get(b)) else {
        return;
    };
    let mut at = 0;
    for &(start, len) in &w.writes.ranges {
        let values = &w.writes.values[at..at + len as usize];
        at += len as usize;
        // SAFETY: `[f32; 4]` is 16 plain bytes without padding; GPUs read little-endian like every target here.
        let bytes = unsafe {
            std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), std::mem::size_of_val(values))
        };
        queue.write_buffer(&gpu.buffer, start as u64 * POINT_BYTES, bytes);
    }
}

/// One block's blades of layer `li` (`SetupGrassGeometry`).
#[allow(clippy::too_many_arguments)]
fn block(
    t: &Terrain,
    li: usize,
    ty: &GrassType,
    s: f32,
    bi: u32,
    bj: u32,
    fb: &FieldBlock,
    pos: &mut Vec<[f32; 3]>,
    uv: &mut Vec<[f32; 2]>,
    field: &mut Vec<u32>,
) {
    let x0 = (BLOCK * bi) as f32;
    let z0 = (BLOCK * bj) as f32;
    let x1 = (BLOCK * bi + BLOCK).min(t.width - 1) as f32;
    let z1 = (BLOCK * bj + BLOCK).min(t.height - 1) as f32;
    let steps_x = ((x1 - x0) * (1.0 / s)) as i32;
    let steps_z = ((z1 - z0) * (1.0 / s)) as i32;
    let vars = ty.variations;
    let mut grid = Vec::new();
    let mut z = z0;
    while z <= z1 {
        let mut x = x0;
        while x <= x1 {
            let k = (x * 12343.0 + z * 7873.0) as i32;
            let v = if wang(k as u32) % (vars + ty.spacing) >= vars {
                -1
            } else {
                ty.random_texture(k)
            };
            grid.push(v);
            x += s;
        }
        z += s;
    }
    if vars > 1 {
        let seed = wang(13u32.wrapping_mul((x0 as i32 + 871 * z0 as i32) as u32));
        ty.clump(seed, &mut grid, steps_x + 1, steps_z + 1);
    }
    let min = (ty.min_opacity as f32 * 2.55) as u32 as f32;
    let du = 1.0 / vars as f32;
    let mut count = 0;
    let mut cell = 0;
    let mut z = z0;
    while z <= z1 {
        let mut x = x0;
        while x <= x1 {
            if count >= MAX_PER_BLOCK {
                return;
            }
            let mut o = layer_opacity(t, li, x, z);
            for above in li + 1..t.layers.len() {
                o *= (255.0 - layer_opacity(t, above, x, z)) * (1.0 / 255.0);
            }
            let v = grid.get(cell).copied().unwrap_or(-1);
            if o >= min && v != -1 {
                let h = t.sample(x, z);
                let k = (x * 3219.0 + z * 23113.0) as i64 as u32;
                // Degrees (a huge f32 angle: any direction).
                let angle = wang(k) as f32 * 0.017_453_292;
                let (sn, cs) = angle.sin_cos();
                let f = o * (1.0 / 255.0);
                let (w, ht) = (f * ty.width, f * ty.height);
                let u0 = v as f32 * du;
                pos.extend_from_slice(&[
                    [x - w * cs, h + ht, z - w * sn],
                    [x + w * cs, h + ht, z + w * sn],
                    [x + w * cs, h, z + w * sn],
                    [x - w * cs, h, z - w * sn],
                ]);
                uv.extend_from_slice(&[[u0, 0.0], [u0 + du, 0.0], [u0 + du, 1.0], [u0, 1.0]]);
                field.extend([GrassField::point_of(fb, s, x, z); 4]);
                count += 1;
            }
            cell += 1;
            x += s;
        }
        z += s;
    }
}
