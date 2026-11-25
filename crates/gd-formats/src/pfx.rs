//! `.pfx` particle effects (`GAME::EffectData`, Engine.dll).
//!
//! File = `u32 101`, then emitters until EOF: `{str name, "PFX1", u32 0,
//! u32 bodyBytes, u32 n × str (texture, shader, drop decal), u32 n × u32 bools,
//! u32 n × i32 ints, u32 n × f32 floats, u32 n × curve}` where curve =
//! `{f32 domain, f32 range, u32 n × (f32 x, f32 y)}`
//! (`EffectData::BufferLoad`, `EmitterData::InternalBinaryRead`,
//! `CurveData::BinaryRead`).
use crate::util::{Reader, byte_list};
use anyhow::{Result, bail, ensure};

pub const VERSION: u32 = 101;
pub const NUM_CURVES: usize = 26;
pub const NUM_BOOLS: usize = 14;

/// Piecewise-linear curve (`GAME::CurveData`). `x` runs over `[0, domain]`
/// (seconds of particle life or of emitter time), `y` over `[0, range]`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Curve {
    pub domain: f32,
    pub range: f32,
    pub points: Vec<[f32; 2]>,
}

impl Curve {
    /// `CurveData::GetYFast`, which is also what every inlined
    /// lookup in the emitter code does: the first segment whose end `x` is
    /// ≥ t is evaluated (so t below the first point extrapolates); past the
    /// last point, or with fewer than two points, the result is 0.
    pub fn eval(&self, t: f32) -> f32 {
        for w in self.points.windows(2) {
            let ([x0, y0], [x1, y1]) = (w[0], w[1]);
            if t <= x1 {
                // Zero-width segments would give NaN in the engine; hold y0.
                let slope = if x1 > x0 { (y1 - y0) / (x1 - x0) } else { 0.0 };
                return (t - x0) * slope + y0;
            }
        }
        0.0
    }

    /// Value of a "half graph" curve: centred on `range / 2` (rates that can be
    /// negative: rotation speed, gravity, offsets, spin).
    pub fn eval_centered(&self, t: f32) -> f32 {
        self.eval(t) - self.range * 0.5
    }

    /// `CurveData::GetMaxY`.
    pub fn max_y(&self) -> f32 {
        self.points
            .iter()
            .map(|p| p[1])
            .fold(f32::MIN, f32::max)
            .max(0.0)
    }
}

/// Spawn volume (`ints[0]`, `Emitter::EmitParticle`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// 0: uniform in a ball of the emission radius.
    Sphere,
    /// 1: uniform in a disc in the emitter's XZ plane.
    Disc,
    /// 2: on a circle in the XZ plane.
    Ring,
    /// 3: along +Y, `[0, radius]`.
    Line,
    /// 4: on the surface of a sphere.
    SphereSurface,
    /// anything else (stock: 5): at the emitter origin.
    Point,
}

impl Shape {
    pub fn from_int(i: i32) -> Self {
        match i {
            0 => Shape::Sphere,
            1 => Shape::Disc,
            2 => Shape::Ring,
            3 => Shape::Line,
            4 => Shape::SphereSurface,
            _ => Shape::Point,
        }
    }
}

/// Blend mode implied by the shader (`EmitterData::UpdateShader` and
/// the shaders' src/dst blend states).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Blend {
    /// `particleadditive*`: SrcAlpha, One (sort order 6100).
    Additive,
    /// `particlecombine`, `particlelit`: SrcAlpha, InvSrcAlpha (6000 / 5900).
    Alpha,
    /// `particledistort*`: screen-space refraction (not a colour blend).
    Distort,
}

/// Curve slots. "life" curves are sampled at the particle's age, "emitter"
/// curves at the emitter's time. Centered ones use [`Curve::eval_centered`].
pub mod curve {
    /// life: alpha.
    pub const ALPHA: usize = 0;
    /// life: colour (gamma space, 0..1).
    pub const RED: usize = 1;
    pub const GREEN: usize = 2;
    pub const BLUE: usize = 3;
    /// life, centered: rotation speed, degrees/s.
    pub const ROTATION_SPEED: usize = 4;
    /// life: billboard half-size (× emitter scale).
    pub const SIZE: usize = 5;
    /// emitter: particles per second.
    pub const RATE: usize = 6;
    /// emitter: initial speed.
    pub const SPEED: usize = 7;
    /// life, centered: gravity (subtracted from vel.y per second).
    pub const GRAVITY: usize = 8;
    /// emitter, centered: emitter offset X/Y/Z.
    pub const OFFSET_X: usize = 9;
    pub const OFFSET_Y: usize = 10;
    pub const OFFSET_Z: usize = 11;
    /// emitter: spread cone, degrees.
    pub const SPREAD: usize = 14;
    /// emitter: emission radius (× scale). Its domain is the emitter duration.
    pub const RADIUS: usize = 15;
    /// life, centered: orbit about the emitter's Y axis, rad/s (local emitters).
    pub const ORBIT: usize = 16;
    /// life: velocity stretch factor.
    pub const STRETCH: usize = 17;
    /// emitter, centered: emitter rotation rate about X/Y/Z, rad/s.
    pub const ROT_X: usize = 18;
    pub const ROT_Y: usize = 19;
    pub const ROT_Z: usize = 20;
    /// life: drag (velocity *= 1 - drag, per update).
    pub const DRAG: usize = 21;
    /// emitter: light colour and radius.
    pub const LIGHT_RED: usize = 22;
    pub const LIGHT_GREEN: usize = 23;
    pub const LIGHT_BLUE: usize = 24;
    pub const LIGHT_RADIUS: usize = 25;
}

/// One emitter (`GAME::EmitterData`).
#[derive(Clone, Debug)]
pub struct Emitter {
    pub name: String,
    pub texture: String,
    pub shader: String,
    /// Decal placed where each particle dies (`DecalType` record), usually empty.
    pub drop_decal: String,
    /// Read through the accessors below; unread slots: bools[8] scales the light by the night factor,
    /// bools[13] shows the light at every light-quality setting.
    pub bools: [bool; NUM_BOOLS],
    /// ints[0]: spawn shape.
    pub shape: Shape,
    pub shape_raw: i32,
    /// ints[1]: flipbook fps; 0 = frames spread over the particle's life.
    pub fps: i32,
    /// floats[0]: particle life, seconds.
    pub life: f32,
    /// floats[1..3]: initial emitter rotation about X, Y, Z, degrees (`Emitter::Reset`).
    pub rotation: [f32; 3],
    pub curves: Vec<Curve>,
    /// From `shader` at load ([`Self::blend`], [`Self::sort_order`]).
    blend: Blend,
    sort_order: u32,
}

impl Emitter {
    /// bools[0]: emitter time wraps (else emission stops after `duration`).
    pub fn looping(&self) -> bool {
        self.bools[0]
    }
    /// bools[1]: particles live in the emitter's space (move with it).
    pub fn local(&self) -> bool {
        self.bools[1]
    }
    /// bools[2]: billboards aligned to (and stretched along) velocity.
    pub fn velocity_aligned(&self) -> bool {
        self.bools[2]
    }
    /// bools[3]: particles fly towards the emitter origin and die there.
    pub fn converge(&self) -> bool {
        self.bools[3]
    }
    /// bools[4]: random initial rotation 0..359°.
    pub fn random_rotation(&self) -> bool {
        self.bools[4]
    }
    /// bools[5]: emission direction flattened to the XZ plane.
    pub fn horizontal(&self) -> bool {
        self.bools[5]
    }
    /// bools[6]: emitter carries a point light (curves 22–25).
    pub fn light(&self) -> bool {
        self.bools[6]
    }
    /// bools[7]: light casts shadows (`Emitter::UpdateLight`: the light's
    /// shadow priority is 1 when set, else 0; the bools sit in `EmitterData.bools`).
    pub fn light_shadows(&self) -> bool {
        self.bools[7]
    }
    /// bools[11]: the light is in simple mode (`Emitter::UpdateLight` copies it to the
    /// light's `Light.simpleMode`): `Light::AddToScene` gives it neither specular nor a shadow.
    pub fn light_simple(&self) -> bool {
        self.bools[11]
    }
    /// bools[10]: billboards lie in the emitter's XZ plane (`orientVertically`).
    pub fn orient_vertically(&self) -> bool {
        self.bools[10]
    }
    /// Emitter duration = domain of the emitter-time curves (`Emitter::Update`
    /// reads curve 15's domain).
    pub fn duration(&self) -> f32 {
        self.curves[curve::RADIUS].domain
    }
    pub fn blend(&self) -> Blend {
        self.blend
    }
    /// Render-queue order of the emitter's shader technique (`.ssh` sort
    /// order, `Emitter::GetRenderPassSortOrder`): particlelit 5900,
    /// particlecombine 6000, particleadditive* 6100, distort 7000. The queue is
    /// sorted by it before distance, so additive fire always draws over smoke.
    pub fn sort_order(&self) -> u32 {
        self.sort_order
    }
    pub fn curve(&self, i: usize) -> &Curve {
        &self.curves[i]
    }
}

#[derive(Clone, Debug)]
pub struct Pfx {
    pub emitters: Vec<Emitter>,
}

impl Pfx {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let ver = r.u32()?;
        if ver != VERSION {
            bail!("pfx version {ver}, expected {VERSION}");
        }
        let mut emitters = Vec::new();
        while r.remaining() > 0 {
            emitters.push(read_emitter(&mut r)?);
        }
        Ok(Pfx { emitters })
    }
}

fn read_emitter(r: &mut Reader) -> Result<Emitter> {
    let name = r.string()?;
    let magic = r.bytes(4)?;
    // Pre-PFX1 data goes through EmitterData::OldBinaryRead; no stock file uses it.
    ensure!(
        magic == b"PFX1",
        "emitter \"{name}\": magic {}, old format not supported",
        byte_list(magic, true)
    );
    let _zero = r.u32()?;
    let body = r.u32()? as usize;
    let start = r.pos;
    let n = r.u32()? as usize;
    ensure!(n <= 3, "emitter \"{name}\": {n} strings");
    let mut strs = [String::new(), String::new(), String::new()];
    for s in strs.iter_mut().take(n) {
        *s = r.string()?;
    }
    let n = r.u32()? as usize;
    ensure!(n <= NUM_BOOLS, "emitter \"{name}\": {n} bools");
    let mut bools = [false; NUM_BOOLS];
    for b in bools.iter_mut().take(n) {
        *b = r.u32()? != 0;
    }
    let n = r.u32()? as usize;
    ensure!(n <= 2, "emitter \"{name}\": {n} ints");
    let mut ints = [0i32; 2];
    for i in ints.iter_mut().take(n) {
        *i = r.i32()?;
    }
    let n = r.u32()? as usize;
    ensure!(n <= 4, "emitter \"{name}\": {n} floats");
    let mut floats = [10.0f32, 0.0, 0.0, 0.0];
    for f in floats.iter_mut().take(n) {
        *f = r.f32()?;
    }
    let n = r.u32()? as usize;
    ensure!(n <= NUM_CURVES, "emitter \"{name}\": {n} curves");
    let mut curves = vec![Curve::default(); NUM_CURVES];
    for c in curves.iter_mut().take(n) {
        c.domain = r.f32()?;
        c.range = r.f32()?;
        let k = r.u32()? as usize;
        ensure!(
            k <= r.remaining() / 8,
            "emitter \"{name}\": curve with {k} points"
        );
        c.points = (0..k)
            .map(|_| Ok([r.f32()?, r.f32()?]))
            .collect::<Result<_>>()?;
    }
    ensure!(
        r.pos - start == body,
        "emitter \"{name}\": body {body} bytes, read {}",
        r.pos - start
    );
    let [texture, shader, drop_decal] = strs;
    let s = shader.to_ascii_lowercase();
    let (blend, sort_order) = if s.contains("distort") {
        (Blend::Distort, 7000)
    } else if s.contains("additive") {
        (Blend::Additive, 6100)
    } else if s.contains("lit") {
        (Blend::Alpha, 5900)
    } else {
        (Blend::Alpha, 6000)
    };
    Ok(Emitter {
        name,
        texture,
        shader,
        drop_decal,
        bools,
        shape: Shape::from_int(ints[0]),
        shape_raw: ints[0],
        fps: ints[1],
        life: floats[0],
        rotation: [floats[1], floats[2], floats[3]],
        curves,
        blend,
        sort_order,
    })
}
