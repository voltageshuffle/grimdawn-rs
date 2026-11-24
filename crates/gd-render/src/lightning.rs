//! `Lightning` records (a skill's `lightningName`, e.g. Doom Bolt's
//! `deathbolt1`): `numberOfBolts` beams to the target, from the sky with
//! `fromSky`, else from the caster; an additive ribbon (`Texture`,
//! `trailadditive.ssh`) `Width` wide, the texture repeated every
//! `TextureStretch` metres, coloured `Red/Green/Blue/Alpha256`, living
//! `timeMin..timeMax` seconds.
//!
//! Game.dll `Lightning` (`Load`, `SetTarget`, `Generate`, `RecursiveGenerate`,
//! `UpdateSelf`) and Engine.dll `LineEffect::RenderPass`:
//! - with `fromSky` a bolt starts 20 m straight above the target;
//! - the path halves every segment of 2 m or more, 4 levels deep: the
//!   midpoint moves perpendicular to the segment, in a random direction
//!   around it, by (noise × 0.25 + uniform(−0.5, 0.5)) × 0.2 × the segment's
//!   length, the noise being a 1D lattice noise of game time × 0.02 plus a
//!   small random offset (see [`subdivide`] and [`noise1`]);
//! - the path is generated once, when the bolt is made (`SetTarget` →
//!   `Generate`), not per frame (`UpdateSelf` only calls `Generate` for new
//!   tendril bolts).
//! - the ribbon faces the camera (the scene's camera axes);
//! - over its life (`timeMin..timeMax`) the drawn part grows from the start
//!   during the first quarter and is eaten from the start during the last,
//!   and its colour is scaled by the drawn fraction (top − bottom).
//!
//! Lightning2: `lightningName` records of class `Lightning2` (Engine.dll; `lightning2*.dbr`: devotion and item
//! procs, Storm Caller, `Skill_AttackRadiusLightning2` / `Skill_BuffAttackRadiusLightning`) are another
//! object ([`Bolt2`]). `Lightning2::Load`: `fromSky`, `color.r/g/b/a`, `texture`
//! (default `FX/Textures/lightningbolt128_02.tex`), `shader` (`TrailAdditive.ssh`), `lineWidth`,
//! `textureLength`, `strikeEffect` (default `records/fx/ambient/fx_lightningblast.dbr`), `strikeDecal`
//! (default `records/fx/decals/decal_blackchar.dbr`); the record's `life` is not read: every bolt lives
//! `lightningLife` = 200 ms. `SetEnd` with `fromSky` starts it 20 m above the end (Game.dll
//! `Skill_AttackRadiusLightning2::CreateVisualEffect` calls `SetEnd` then `SetFromSky(true)`,
//! so it is the record's flag that counts; every stock record has it). The path is regenerated every
//! 50–100 ms ([`update_bolts2`]) as a main tendril and sub-tendrils ([`generate`], [`generate_tendril`],
//! [`generate_sub_tendril`]) and drawn as camera-facing ribbons ([`bolt2_mesh`]). The strike decal is dynamic
//! (`Decal.dynamic`) and its `Decal.faceAngle` (π/2 − `maxAngle`) is set to −π/2, so it keeps every mesh face
//! whatever its slope (`Lightning2::Generate`; grimdawn `casting.rs` `fx_decals`). PORT: the `rand()`
//! calls use the port's stream; the scene's camera axis is the camera's forward.
use bevy::prelude::*;

pub struct LightningPlugin;

impl Plugin for LightningPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (spawn_bolts, fade_bolts, update_bolts2));
    }
}

pub use gd_game_core::visuals::PendingBolt;

#[derive(Component)]
struct Bolt {
    age: f32,
    life: f32,
    color: Vec3,
    mesh: Handle<Mesh>,
    points: u32,
    /// The point range `fade_bolts` last drew.
    drawn: (u32, u32),
}

/// `RecursiveGenerate`: `from`..`to` split `depth` more times; `game_ms` is
/// the game clock in ms (Engine.dll `Engine::GetGameTime`: the
/// pausable, scaled game time as an int of milliseconds).
///
/// `Lightning::RecursiveGenerate`, per split, three `rand()` calls in this order:
/// angle = rand × 2π/32767 (0.00019175345) around the segment, the axes being
/// `p` = normalised dir × Y (dir × X when |dir.y| > 0.9999, `Game::PerpendicularAxis`) and
/// normalised dir × p, direction = cos·p + sin·(dir × p); u = rand / 32767 −
/// 0.5; t = (double)game_ms × 0.02 + rand × 100/32767 (0.0030518509);
/// offset = (noise1(t) × 0.25 + u) × 0.2 × length.
fn subdivide(from: Vec3, to: Vec3, depth: u32, game_ms: i32, out: &mut Vec<Vec3>) {
    let len = from.distance(to);
    if len == 0.0 || len < 2.0 || depth == 0 {
        out.push(to);
        return;
    }
    let dir = (to - from) / len;
    let a = if dir.y.abs() <= 0.9999 {
        dir.cross(Vec3::Y)
    } else {
        dir.cross(Vec3::X)
    }
    .normalize();
    let b = dir.cross(a).normalize();
    let angle = rand() as f32 * 0.000_191_753_45;
    let side = a * angle.cos() + b * angle.sin();
    let u = rand() as f32 * 0.000_030_518_51 - 0.5;
    let jitter = rand() as f32 * 0.003_051_851;
    let t = (game_ms as f64 * 0.02 + jitter as f64) as f32;
    let k = noise1(t) * 0.25 + u;
    let mid = (from + to) * 0.5 + side * (k * (len * 0.2));
    subdivide(from, mid, depth - 1, game_ms, out);
    subdivide(mid, to, depth - 1, game_ms, out);
}

/// The CRT `rand()` (0..=32767) the game draws from. PORT: the game's is the
/// process-wide CRT stream shared with everything else, so its values cannot
/// be reproduced; the port's shared RNG stands in, with the same range.
fn rand() -> i32 {
    gd_game_core::rng::i32(0..=32767)
}

/// MSVC CRT `rand()` from its per-thread start state 1 (used to rebuild the
/// noise table exactly).
struct MsvcRand(u32);

impl MsvcRand {
    fn next(&mut self) -> i32 {
        self.0 = self.0.wrapping_mul(214_013).wrapping_add(2_531_011);
        ((self.0 >> 16) & 0x7fff) as i32
    }
}

/// Game.dll's 1D gradient noise table (`Game::Noise1Table`: 514 permutation entries,
/// then 514 gradients).
///
/// `Game::InitNoise1`, run by the static initialiser `Game::StaticInitNoise1` with
/// the CRT `rand`: for i in 0..256, perm[i] = i and grad[i] = (rand % 512 −
/// 256) / 256, normalised to ±1 when its square exceeds 1e-4; then for i =
/// 255 down to 1 perm[i] swaps with perm[rand as u8]; then entries 256..514
/// repeat 0..258. That initialiser and the 2D one after it are the only
/// static initialisers of Game.dll (or Engine.dll, which loads first) that
/// reach `rand`, and GrimDawn.exe imports Game.dll statically, so they run on
/// the main thread during process start before any `srand`: the 1D table is
/// built from the CRT's initial state (1). ASSUMED: no other module loaded
/// before it (CrashReport, StatReport, lua51) calls `rand` on that thread.
struct Noise1 {
    perm: [usize; 514],
    grad: [f32; 514],
}

static NOISE1: std::sync::LazyLock<Noise1> = std::sync::LazyLock::new(|| {
    let mut r = MsvcRand(1);
    let mut perm = [0usize; 514];
    let mut grad = [0f32; 514];
    for i in 0..256 {
        perm[i] = i;
        let v = ((r.next() % 512) - 256) as f32 * 0.003_906_25;
        grad[i] = if (v * v).abs() > 0.000_1 {
            v * (1.0 / (v * v).sqrt())
        } else {
            v
        };
    }
    for i in (1..256).rev() {
        let j = r.next() as u8 as usize;
        perm.swap(i, j);
    }
    for j in 0..258 {
        perm[256 + j] = perm[j];
        grad[256 + j] = grad[j];
    }
    Noise1 { perm, grad }
});

/// `Game::Noise1` (Perlin's `noise1`): i = floor(t) & 255, f = t −
/// floor(t), s = (3 − 2f)·f²; lerp(grad[perm[i]]·f, grad[perm[i+1]]·(f − 1), s).
fn noise1(t: f32) -> f32 {
    let n = &*NOISE1;
    let fl = t.floor();
    let i = (fl as i32 & 255) as usize;
    let f = t - fl;
    let s = (3.0 - (f + f)) * (f * f);
    let a = n.grad[n.perm[i]] * f;
    let b = n.grad[n.perm[i + 1]] * (f - 1.0);
    (b - a) * s + a
}

fn spawn_bolts(
    mut commands: Commands,
    new: Query<(Entity, &PendingBolt)>,
    cams: Query<&GlobalTransform, With<crate::camera::GameCamera>>,
    time: Res<Time>,
    mut assets: crate::character::CharAssets,
) {
    let cam = cams.iter().next().map(|c| c.translation());
    for (e, b) in &new {
        commands.entity(e).try_despawn();
        gd_game_core::visuals::notify(&mut commands, &b.record);
        // The database by its own handle: the record stays borrowed while `assets` spawns.
        let db = assets.data.db.clone();
        let Some(rec) = db.get(&b.record) else {
            continue;
        };
        if rec.class == "Lightning2" {
            spawn_bolt2(&mut commands, &mut assets, rec, b);
            continue;
        }
        let img = rec.str("Texture").and_then(|p| {
            assets
                .textures
                .get(&assets.data, p, true, &mut assets.images)
        });
        // `Load`: colours default to 0.
        let c = |k: &str| rec.i32(k).unwrap_or(0) as f32 / 255.0;
        let color = Vec3::new(c("Red256"), c("Green256"), c("Blue256")) * c("Alpha256");
        let width = rec.f32("Width").unwrap_or(1.0).max(0.05);
        let stretch = rec.f32("TextureStretch").unwrap_or(5.0).max(0.1);
        let (lo, hi) = (
            rec.f32("timeMin").unwrap_or(0.3),
            rec.f32("timeMax").unwrap_or(0.5),
        );
        for _ in 0..rec.i32("numberOfBolts").unwrap_or(1).max(1) {
            let from = if rec.bool("fromSky") {
                b.to + Vec3::Y * 20.0
            } else {
                b.from
            };
            let dir = (b.to - from).normalize_or(Vec3::NEG_Y);
            let mid = (from + b.to) * 0.5;
            let view = cam.map_or(Vec3::Z, |c| (c - mid).normalize_or(Vec3::Z));
            let side = dir.cross(view).normalize_or(Vec3::X);
            let mut pts = vec![from];
            subdivide(
                from,
                b.to,
                4,
                (time.elapsed_secs_f64() * 1000.0) as i32,
                &mut pts,
            );
            let mut pos = Vec::new();
            let mut uv = Vec::new();
            let mut along = 0.0;
            for i in 0..pts.len() {
                if i > 0 {
                    along += pts[i].distance(pts[i - 1]);
                }
                let d =
                    (pts[(i + 1).min(pts.len() - 1)] - pts[i.saturating_sub(1)]).normalize_or(dir);
                let s = d
                    .cross((cam.unwrap_or(pts[i] + Vec3::Z) - pts[i]).normalize_or(Vec3::Z))
                    .normalize_or(side)
                    * width
                    * 0.5;
                pos.push((pts[i] + s - b.to).to_array());
                pos.push((pts[i] - s - b.to).to_array());
                // Beam textures run along V (`beam_deathbolt_01`: a vertical bolt).
                uv.push([0.0, along / stretch]);
                uv.push([1.0, along / stretch]);
            }
            let n = pts.len() as u32;
            let mut m = Mesh::new(
                bevy::mesh::PrimitiveTopology::TriangleList,
                bevy::asset::RenderAssetUsages::default(),
            );
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
            m.insert_attribute(
                Mesh::ATTRIBUTE_NORMAL,
                vec![[0.0, 1.0, 0.0]; (n * 2) as usize],
            );
            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
            m.insert_indices(bevy::mesh::Indices::U32(vec![
                0;
                (n.saturating_sub(1) * 6)
                    as usize
            ]));
            let mesh = assets.meshes.add(m);
            let mat = assets.materials.add(StandardMaterial {
                base_color_texture: img.clone(),
                base_color: Color::linear_rgb(color.x, color.y, color.z),
                unlit: true,
                alpha_mode: AlphaMode::Add,
                cull_mode: None,
                ..default()
            });
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat),
                Transform::from_translation(b.to),
                bevy::light::NotShadowCaster,
                bevy::camera::visibility::NoFrustumCulling,
                Bolt {
                    age: 0.0,
                    life: lo + gd_game_core::rng::f32() * (hi - lo).max(0.0),
                    color,
                    mesh,
                    points: n,
                    drawn: (u32::MAX, u32::MAX),
                },
                Name::new("lightning"),
            ));
        }
    }
}

fn fade_bolts(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Bolt, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for (e, mut b, m) in &mut q {
        b.age += time.delta_secs();
        if b.age >= b.life {
            commands.entity(e).try_despawn();
            continue;
        }
        // `Lightning::UpdateSelf` → `LineEffect::SetTopBottomPercent`.
        let rest = 1.0 - (b.age / b.life.max(1e-3)).min(1.0);
        let top = if rest > 0.75 {
            1.0 - (rest - 0.75) * 4.0
        } else {
            1.0
        };
        let bottom = if rest < 0.25 { 1.0 - rest * 4.0 } else { 0.0 };
        let k = top - bottom;
        if let Some(mut mat) = materials.get_mut(&m.0) {
            mat.base_color = Color::linear_rgb(b.color.x * k, b.color.y * k, b.color.z * k);
        }
        // Points drawn: count × bottom ..= count × top, when at least 2 apart.
        let (lo, hi) = (
            ((b.points as f32) * bottom) as u32,
            (((b.points as f32) * top) as u32).min(b.points - 1),
        );
        if (lo, hi) == b.drawn {
            continue;
        }
        b.drawn = (lo, hi);
        if let Some(mut mesh) = meshes.get_mut(&b.mesh)
            && let Some(bevy::mesh::Indices::U32(idx)) = mesh.indices_mut()
        {
            // The full-size index buffer from the spawn, rewritten in place: the drawn quads, the rest degenerate.
            idx.fill(0);
            if hi >= lo + 2 {
                for (q, i) in idx.as_chunks_mut::<6>().0.iter_mut().zip(lo..hi) {
                    let a = i * 2;
                    q.copy_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]);
                }
            }
        }
    }
}

/// A `Lightning2` bolt (module doc, "Lightning2").
#[derive(Component)]
struct Bolt2 {
    start: Vec3,
    /// End − start (`SetEnd`, `Lightning2.delta`).
    delta: Vec3,
    color: Vec3,
    line_width: f32,
    texture_length: f32,
    strike_effect: String,
    strike_decal: String,
    /// `lightningLife` left, ms (`Lightning2.lifeLeft`).
    life_ms: i32,
    /// Until the next `Generate`, ms (`Lightning2.regenTimer`).
    regen_ms: i32,
    /// The texture's v offset (`Lightning2.textureOffset`).
    tex_offset: f32,
    /// The tendrils: points relative to `start` and their width factor.
    tendrils: Vec<(Vec<Vec3>, f32)>,
    /// Made on the first update ([`update_bolts2`]).
    mesh: Option<Handle<Mesh>>,
    /// The frame-time remainder below 1 ms.
    carry: f32,
}

/// Engine.dll `Lightning2::lightningLife` (ms).
const LIGHTNING2_LIFE_MS: i32 = 200;

fn spawn_bolt2(
    commands: &mut Commands,
    assets: &mut crate::character::CharAssets,
    rec: &gd_formats::arz::Record,
    b: &PendingBolt,
) {
    // `Load`: a missing float reads 0, the strings fall back to the engine's defaults.
    let f = |k: &str| rec.f32(k).unwrap_or(0.0);
    let img = assets.textures.get(
        &assets.data,
        rec.str("texture")
            .unwrap_or("FX/Textures/lightningbolt128_02.tex"),
        true,
        &mut assets.images,
    );
    // `SetEnd` with `fromSky`: the bolt starts 20 m above its end.
    let start = if rec.bool("fromSky") {
        b.to + Vec3::Y * 20.0
    } else {
        b.from
    };
    // The mesh is made on the first update, once the bolt is generated (no empty mesh asset).
    // `trailadditive.ssh`: texture × vertex colour, added (as `trails.rs`).
    let mat = assets.materials.add(StandardMaterial {
        base_color_texture: img,
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        ..default()
    });
    commands.spawn((
        MeshMaterial3d(mat),
        Transform::from_translation(start),
        bevy::light::NotShadowCaster,
        bevy::camera::visibility::NoFrustumCulling,
        Bolt2 {
            start,
            delta: b.to - start,
            color: Vec3::new(f("color.r"), f("color.g"), f("color.b")),
            line_width: f("lineWidth"),
            texture_length: f("textureLength"),
            strike_effect: rec
                .str("strikeEffect")
                .unwrap_or("records/fx/ambient/fx_lightningblast.dbr")
                .to_owned(),
            strike_decal: rec
                .str("strikeDecal")
                .unwrap_or("records/fx/decals/decal_blackchar.dbr")
                .to_owned(),
            life_ms: LIGHTNING2_LIFE_MS,
            regen_ms: 0,
            tex_offset: 0.0,
            tendrils: Vec::new(),
            mesh: None,
            carry: 0.0,
        },
        Name::new("lightning2"),
    ));
}

/// `rand() / 32767`.
fn unit(rand: &mut impl FnMut() -> i32) -> f32 {
    rand() as f32 * 0.000_030_518_51
}

/// The two axes a tendril's points swing along, for a unit direction (`GenerateTendril`'s frame): a vertical
/// direction gets a random horizontal pair.
fn tendril_axes(dir: Vec3, rand: &mut impl FnMut() -> i32) -> (Vec3, Vec3) {
    if dir.y.abs() == 1.0 {
        let r1 = unit(rand) * 2.0 - 1.0;
        let r2 = unit(rand) * 2.0 - 1.0;
        let n = 1.0 / (r2 * r2 + r1 * r1).sqrt();
        let (a, b) = (r1 * n, r2 * n);
        (Vec3::new(b, 0.0, a), Vec3::new(a, 0.0, -b))
    } else {
        let (x, y, z) = (dir.x, dir.y, dir.z);
        (
            Vec3::new(z, 0.0, -x),
            Vec3::new(x * y, -(x * x + z * z), z * y),
        )
    }
}

/// `Lightning2::GenerateTendril`: the points from `from` towards `to`, width factor `a`.
fn generate_tendril(from: Vec3, to: Vec3, a: f32, rand: &mut impl FnMut() -> i32) -> Vec<Vec3> {
    let d = to - from;
    let len2 = d.length_squared();
    let dir = d / len2.sqrt();
    let total = if len2 == 0.0 { 0.0 } else { len2.sqrt() };
    let mut pts = vec![from];
    let (axis_a, axis_b) = tendril_axes(dir, rand);
    if total > 0.0001 {
        let inv = 1.0 / total;
        let (step_lo, step_span) = (a + a, a * 4.0 - (a + a));
        let (amp_lo, amp_span) = (a * 0.25, a * 0.75 - a * 0.25);
        let mut left = total;
        let mut along = from;
        while left > 0.0001 {
            let phase = 1.0 - inv * left;
            let step = (unit(rand) * step_span + step_lo).min(left);
            left -= step;
            along += dir * step;
            let kb = unit(rand) * amp_span + amp_lo;
            let s = (phase * 12.252_212).sin();
            let ka = unit(rand) * amp_span + amp_lo;
            pts.push(along + axis_a * (s * ka) + axis_b * (s * kb));
        }
    }
    pts
}

/// `Lightning2::GenerateSubTendril` (`Generate` passes `a = 0.5` and no random length): a
/// tendril from `at` half the bolt's length along `dir`, offset sideways by `4a` along the tendril axes.
fn generate_sub_tendril(
    at: Vec3,
    dir: Vec3,
    len: f32,
    a: f32,
    rand: &mut impl FnMut() -> i32,
) -> Vec<Vec3> {
    let side = if dir.y.abs() == 1.0 {
        let r1 = unit(rand) * 2.0 - 1.0;
        let r2 = unit(rand) * 2.0 - 1.0;
        let n = 1.0 / (r1 * r1 + r2 * r2).sqrt();
        let (p, q) = (r1 * n, r2 * n);
        Vec3::new(q + p, 0.0, p - q)
    } else {
        let (x, y, z) = (dir.x, dir.y, dir.z);
        Vec3::new(z + x * y, -(x * x + z * z), -x + y * z)
    };
    let end = at + dir * (len * a) + side * (a * 4.0);
    generate_tendril(at, end, a, rand)
}

/// `Lightning2::Generate`: the main tendril from the start to the end (width 1), then from
/// every 1st to 3rd of its points (`i = floor(i + 1 + 2u)`) within the first half of the bolt a sub-tendril
/// (width 0.5); the texture offset is drawn anew. Returns the strike point relative to the start (the main
/// tendril's last point).
fn generate(
    delta: Vec3,
    tendrils: &mut Vec<(Vec<Vec3>, f32)>,
    tex_offset: &mut f32,
    rand: &mut impl FnMut() -> i32,
) -> Vec3 {
    tendrils.clear();
    let main = generate_tendril(Vec3::ZERO, delta, 1.0, rand);
    let len2 = delta.length_squared();
    let dir = delta / len2.sqrt();
    let len = if len2 == 0.0 { 0.0 } else { len2.sqrt() };
    let mut subs = Vec::new();
    let mut i = 0usize;
    while i < main.len() {
        if main[i].length() / len <= 0.5 {
            subs.push((generate_sub_tendril(main[i], dir, len, 0.5, rand), 0.5));
        }
        i = (unit(rand) * 2.0 + 1.0 + i as f32) as usize;
    }
    *tex_offset = unit(rand);
    let strike = *main.last().unwrap();
    tendrils.push((main, 1.0));
    tendrils.extend(subs);
    strike
}

/// `Lightning2::RenderPass`: each tendril a ribbon facing the camera (side = view axis × the
/// segment direction, the first point using the tendril's first-to-last direction), `lineWidth / 2 × width`
/// to each side; u 0 / 1 across, v = the offset plus the straight distance from the tendril's first point /
/// `textureLength`; vertex colour the record's rgb (`color.a` is unused) with alpha 0 at both ends; at most
/// 500 segments.
fn bolt2_mesh(b: &Bolt2, view: Vec3) -> Mesh {
    let (mut pos, mut uv, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut segments = 0usize;
    for (pts, w) in &b.tendrils {
        let n = pts.len();
        if n == 0 || segments + n - 1 > 500 {
            break;
        }
        segments += n - 1;
        let base = pos.len() as u32;
        for (i, p) in pts.iter().enumerate() {
            let d = if i == 0 {
                pts[n - 1] - pts[0]
            } else {
                *p - pts[i - 1]
            }
            .normalize_or_zero();
            let side = view.cross(d) * (b.line_width * 0.5 * w);
            let v = p.distance(pts[0]) / b.texture_length + b.tex_offset;
            let alpha = if i == 0 || i == n - 1 { 0.0 } else { 1.0 };
            let c = [b.color.x, b.color.y, b.color.z, alpha];
            pos.push((*p + side).to_array());
            uv.push([0.0, v]);
            pos.push((*p - side).to_array());
            uv.push([1.0, v]);
            col.push(c);
            col.push(c);
        }
        for k in 1..n as u32 {
            let v = base + 2 * k;
            idx.extend_from_slice(&[v, v + 1, v - 1, v - 2, v, v - 1]);
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; pos.len()];
    let mut m = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    );
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    m.insert_indices(bevy::mesh::Indices::U32(idx));
    m
}

/// `Lightning2::UpdateSelf` every update of `dt` ms: the life counts down (gone at 0); the
/// regeneration timer too, and at 0 the texture offset moves on by 0.25 + u / 4 (wrapping past 1), the
/// timer restarts at 50 + 50u ms and the bolt is generated again (so the first update generates it),
/// each time with its strike effect and decal at the strike point (`strikeEffect`, default
/// `fx_lightningblast`, an `EffectEntity` added at the point; `strikeDecal`, default `decal_blackchar`,
/// put on the floor there).
fn update_bolts2(
    mut commands: Commands,
    time: Res<Time>,
    cams: Query<&GlobalTransform, With<crate::camera::GameCamera>>,
    mut bolts: Query<(Entity, &mut Bolt2)>,
    mut meshes: ResMut<Assets<Mesh>>,
    data: Res<gd_game_core::data::GameData>,
) {
    let view = cams.iter().next().map_or(Vec3::NEG_Y, |c| *c.forward());
    let mut rng = rand;
    for (e, mut b) in &mut bolts {
        let ms = time.delta_secs() * 1000.0 + b.carry;
        let dt = ms.floor();
        b.carry = ms - dt;
        let dt = dt as i32;
        b.life_ms -= dt;
        if b.life_ms <= 0 {
            commands.entity(e).try_despawn();
            continue;
        }
        b.regen_ms -= dt;
        if b.regen_ms <= 0 {
            let mut o = unit(&mut rng) * 0.25 + b.tex_offset + 0.25;
            if o > 1.0 {
                o -= 1.0;
            }
            b.tex_offset = o;
            b.regen_ms = (unit(&mut rng) * 50.0 + 50.0) as i32;
            let b = &mut *b;
            let strike = b.start + generate(b.delta, &mut b.tendrils, &mut b.tex_offset, &mut rng);
            gd_game_core::visuals::spawn_fx(
                &mut commands,
                &data,
                &b.strike_effect,
                Transform::from_translation(strike),
            );
            commands.spawn((
                gd_game_core::visuals::PendingDecal(b.strike_decal.clone()),
                Transform::from_translation(strike),
            ));
        }
        let m = bolt2_mesh(&b, view);
        match &b.mesh {
            Some(h) => {
                if let Some(mut x) = meshes.get_mut(h) {
                    *x = m;
                }
            }
            None => {
                let h = meshes.add(m);
                commands.entity(e).insert(Mesh3d(h.clone()));
                b.mesh = Some(h);
            }
        }
    }
}
