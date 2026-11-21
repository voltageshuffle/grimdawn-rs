//! CPU particle simulation, a port of `GAME::Emitter` (Engine.dll):
//! `Update` 0x18006d420, `EmitParticles` 0x18006bd90, `EmitParticle`
//! 0x180069a00, particle init 0x180185200, `UpdateParticles` 0x18006c6c0,
//! `UpdateLight` 0x18006b670, `Reset` 0x18006a510.
//!
//! Matrices: the engine uses row vectors (`p' = p·M`); here everything is in
//! glam's column form, so an engine product `R·M` becomes `M * R`.
use bevy::math::{Mat3, Vec3, Vec4};
use gd_formats::pfx::{Emitter as Data, Shape, curve as c};

/// MSVC `rand()`: 0..=32767.
fn rand() -> u32 {
    gd_game::rng::u32(0..=32767)
}

/// `rand() * (1/32767)`.
fn rand01() -> f32 {
    rand() as f32 * 0.000_030_518_51
}

const DEG: f32 = 0.017_453_292;
/// The `fxQuality` option's particle density (`Engine::SetFxQualityModifier` @0x180079480:
/// low 0.33, medium 0.66, high and above 1.0), read by each emitter when it is created. PORT: a display
/// setting kept here; emitters already running keep theirs.
static DENSITY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f80_0000);

/// The density now set (the emitter spawn path passes it to [`EmitterState::new`]).
pub fn fx_density() -> f32 {
    f32::from_bits(DENSITY.load(std::sync::atomic::Ordering::Relaxed))
}

/// Sets the density for the `fxQuality` value `quality` (`low`, `medium`, `high`, ...).
pub fn set_fx_quality(quality: &str) {
    let d: f32 = match quality {
        "low" => 0.33,
        "medium" => 0.66,
        _ => 1.0,
    };
    DENSITY.store(d.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Particle cap per emitter (`AllocateParticleArray`).
const MAX_PARTICLES: usize = 10_000;

/// `world = rot * local + pos` (GAME::Coords).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coords {
    pub rot: Mat3,
    pub pos: Vec3,
}

impl Coords {
    pub const IDENTITY: Coords = Coords {
        rot: Mat3::IDENTITY,
        pos: Vec3::ZERO,
    };
    fn point(&self, p: Vec3) -> Vec3 {
        self.rot * p + self.pos
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Particle {
    /// +0 seconds.
    pub life: f32,
    /// +4 seconds.
    pub age: f32,
    /// +8 degrees.
    pub rotation: f32,
    /// +12 billboard half-size.
    pub size: f32,
    /// +16 velocity stretch.
    pub stretch: f32,
    /// +20 flipbook frame.
    pub frame: u32,
    /// +24 position in the emitter's simulation space (world, or emitter-local
    /// for local emitters).
    pub pos: Vec3,
    /// +36
    pub vel: Vec3,
    /// +48 rgba (gamma space).
    pub color: Vec4,
}

/// Runtime state of one emitter.
#[derive(Clone, Debug)]
pub struct EmitterState {
    pub particles: Vec<Particle>,
    capacity: usize,
    /// Emitter time (+52), wraps for looping emitters.
    pub time: f32,
    /// Fractional particle budget (+56) and particles emitted so far (+60).
    accum: f32,
    emitted: i32,
    /// Non-looping emitter reached its duration (+38).
    pub done: bool,
    /// `StopEmitting` (+37).
    pub stopped: bool,
    /// Cleared by `DisableLooping` (+747).
    pub loop_allowed: bool,
    /// Emitter orientation (+400), spun by curves 18–20.
    pub rot: Mat3,
    /// Emitter offset (+64), curves 9–11.
    pub offset: Vec3,
    /// `SetScale` (+748).
    pub scale: f32,
    /// Coords of the previous update, for spawn interpolation.
    prev_pos: Option<Vec3>,
    /// Highest particle alpha of the last update (+752), drives the light.
    pub max_alpha: f32,
    /// Texture frames (flipbook length).
    pub frames: u32,
    /// Particle density setting (gEngine+3096; 1.0 = default).
    pub density: f32,
    /// Particles of an emitter with a drop decal that died since the caller
    /// last drained this: (sim-space position, yaw in degrees).
    /// `Emitter::UpdateParticles` @0x18006c6c0: a dying particle of an emitter
    /// whose `DecalType` (+592) is set places that decal on the ground at its
    /// world position (`World::PlaceDecalOnGround`), yawed `rand() % 360`°.
    pub dropped: Vec<(Vec3, f32)>,
    /// `SetAnchored` emitters: the anchor history (anchor 1, anchor 2),
    /// oldest first, at most 4 (`Emitter::SetAnchor1/2`), and whether a
    /// fifth entry has pushed one out.
    anchors: Vec<(Vec3, Vec3)>,
    anchors_full: bool,
}

/// The emitter's light this frame (bools[6]).
#[derive(Clone, Copy, Debug)]
pub struct LightState {
    pub position: Vec3,
    pub color: Vec3,
    pub radius: f32,
    /// clamp(max particle alpha, 0, 1) (`Emitter::AddToScene`).
    pub intensity: f32,
}

impl EmitterState {
    /// `density` is the fx-quality setting ([`fx_density`]) the caller passes in: an explicit input, so
    /// the emitter tests do not read the process global the settings test writes.
    pub fn new(d: &Data, frames: u32, density: f32) -> Self {
        // Emitter::Reset: identity, then X, Y, Z increments of floats[1..3] degrees.
        let rot = Mat3::from_rotation_x(d.rotation[0] * DEG)
            * Mat3::from_rotation_y(d.rotation[1] * DEG)
            * Mat3::from_rotation_z(d.rotation[2] * DEG);
        // AllocateParticleArray: max rate · density · life · 1.25, at least 10,
        // at most 10000, plus 100 slack.
        let want = (d.curve(c::RATE).max_y() * density * d.life * 1.25) as usize;
        let capacity = want.clamp(10, MAX_PARTICLES) + 100;
        EmitterState {
            particles: Vec::new(),
            capacity,
            time: 0.0,
            accum: 0.0,
            emitted: 0,
            done: false,
            stopped: false,
            loop_allowed: true,
            rot,
            offset: Vec3::ZERO,
            scale: 1.0,
            prev_pos: None,
            max_alpha: 0.0,
            frames: frames.max(1),
            density,
            dropped: Vec::new(),
            anchors: Vec::new(),
            anchors_full: false,
        }
    }

    /// `Emitter::SetAnchor1` / `SetAnchor2` (@0x180069040): the
    /// anchors (world) join the history; from then on the emitter is
    /// anchored ([`Self::update`]).
    pub fn set_anchors(&mut self, a: Vec3, b: Vec3) {
        self.anchors.push((a, b));
        if self.anchors.len() > 4 {
            self.anchors.remove(0);
            self.anchors_full = true;
        }
    }

    /// The four spline points of each anchor: the history once it is full
    /// (the engine fills it with a fifth entry and drops the oldest), until
    /// then four copies of the first entry.
    fn anchor_points(&self) -> Option<[[Vec3; 4]; 2]> {
        let first = *self.anchors.first()?;
        let full = self.anchors_full;
        Some(std::array::from_fn(|k| {
            std::array::from_fn(|i| {
                let e = if full { self.anchors[i] } else { first };
                if k == 0 { e.0 } else { e.1 }
            })
        }))
    }

    /// `Emitter::IsComplete`.
    pub fn is_complete(&self) -> bool {
        self.particles.is_empty() && (self.done || self.stopped)
    }

    fn emitting(&self, d: &Data) -> bool {
        !self.stopped && (!self.done || (self.loop_allowed && d.looping()))
    }

    /// One `Emitter::Update`. `coords` is the owning entity's frame; `dt` the
    /// real time since the last update.
    pub fn update(&mut self, d: &Data, coords: Coords, dt: f32) {
        let dtc = dt.min(0.1);
        if self.emitting(d) {
            let dur = d.duration();
            self.time += dtc;
            if self.loop_allowed && d.looping() {
                if dur > 0.0 {
                    self.time -= (self.time / dur).trunc() * dur;
                }
            } else if self.time >= dur {
                self.time = dur;
                self.done = true;
            }
        }
        let t = self.time;
        self.offset = Vec3::new(
            d.curve(c::OFFSET_X).eval_centered(t),
            d.curve(c::OFFSET_Y).eval_centered(t),
            d.curve(c::OFFSET_Z).eval_centered(t),
        );
        self.emit_particles(d, coords, dtc);
        if !self.particles.is_empty() {
            self.update_particles(d, dt, dtc);
        } else {
            self.max_alpha = 0.0;
        }
        self.prev_pos = Some(coords.pos);
    }

    fn emit_particles(&mut self, d: &Data, coords: Coords, dtc: f32) {
        if !self.emitting(d) {
            return;
        }
        let t = self.time;
        let mut rate = d.curve(c::RATE).eval(t) * dtc;
        if self.accum > 0.0 {
            rate *= self.density;
        }
        self.accum += rate;
        let n = self.accum as i32 - self.emitted;
        // Emitter spin, rad/s about its own axes (IncrementX/Y/ZRot).
        for (i, axis) in [
            (c::ROT_X, Vec3::X),
            (c::ROT_Y, Vec3::Y),
            (c::ROT_Z, Vec3::Z),
        ] {
            let a = d.curve(i).eval_centered(t) * dtc;
            if a.abs() > 0.0001 {
                self.rot *= Mat3::from_axis_angle(axis, a);
            }
        }
        // `Emitter::EmitParticles` @0x18006bd90: an anchored emitter
        // emits particle i of n between the two anchors' Catmull-Rom points
        // at i / n (sub_180070CB0: from the history's second entry to its
        // third). PORT: the engine skips this while gEngine +0x293 is set
        // (unidentified).
        if let Some([p1, p2]) = self.anchor_points() {
            for i in 0..n.max(0) {
                let t = i as f32 / n as f32;
                self.emit_anchored(d, catmull_rom(p1, t), catmull_rom(p2, t));
                self.emitted += 1;
            }
            return;
        }
        let local = d.local();
        let prev = self.prev_pos.unwrap_or(coords.pos);
        for i in 0..n.max(0) {
            let w = if local {
                Coords::IDENTITY
            } else {
                // Spawn points are spread along the path since the last update.
                let f = i as f32 / n as f32;
                Coords {
                    rot: coords.rot,
                    pos: prev * (1.0 - f) + coords.pos * f,
                }
            };
            self.emit_particle(d, w);
            self.emitted += 1;
        }
    }

    /// `Emitter::EmitAnchoredParticle` @0x18006acf0: at a random
    /// point between `a` and `b` (in steps of 0.001; equal anchors: `b` 1 cm
    /// along x), moving in a random direction (a normalised cube sample) at
    /// the speed curve's value; the emitter's shape, spread, offset and
    /// rotation play no part. The anchors are world positions whatever the
    /// emitter's space, as in the engine.
    fn emit_anchored(&mut self, d: &Data, a: Vec3, mut b: Vec3) {
        if self.particles.len() >= self.capacity {
            return;
        }
        if a == b {
            b.x += 0.01;
        }
        let r = || (rand() % 1000) as f32 * 0.001;
        let pos = a + (b - a) * r();
        let dir = Vec3::new(r() - 0.5, r() - 0.5, r() - 0.5);
        let mut vel = dir.normalize_or_zero() * d.curve(c::SPEED).eval(self.time);
        if vel.length_squared() < 0.001 {
            vel = Vec3::new(0.0, 0.001, 0.0);
        }
        let mut part = Particle {
            life: d.life,
            age: 0.0,
            rotation: 1.0,
            pos,
            vel,
            ..Default::default()
        };
        if d.random_rotation() {
            part.rotation = (rand() % 360) as f32;
        }
        self.particles.push(part);
    }

    fn emit_particle(&mut self, d: &Data, w: Coords) {
        if self.particles.len() >= self.capacity {
            return;
        }
        let t = self.time;
        let spread = d.curve(c::SPREAD).eval(t) * DEG;
        let polar = if spread == 0.0 {
            0.0
        } else {
            rand01() * spread - spread * 0.5
        } + 1.57;
        let azimuth = rand() as f32 * 0.000_191_753_45 - spread * 0.5;
        let mut dir = Vec3::new(
            polar.cos() * azimuth.cos(),
            polar.sin(),
            polar.cos() * azimuth.sin(),
        );
        if d.horizontal() {
            dir.y = 0.0;
        }
        let vel_e = dir.normalize_or_zero() * d.curve(c::SPEED).eval(t);
        let mut p = self.offset * self.scale;
        let r = (self.scale * d.curve(c::RADIUS).eval(t)).max(0.01);
        let in_ball = |dims: usize| -> Vec3 {
            // Rejection sampling in the cube [-r, r]; gives up after 10 tries.
            let mut tries = 0;
            loop {
                let mut v = Vec3::new(rand01() * 2.0 * r - r, 0.0, 0.0);
                v.y = rand01() * 2.0 * r - r;
                if dims == 3 {
                    v.z = rand01() * 2.0 * r - r;
                } else {
                    // 2D: the second sample is z.
                    v = Vec3::new(v.x, 0.0, v.y);
                }
                tries += 1;
                if tries > 10 {
                    v = Vec3::ZERO;
                }
                if v.length() <= r {
                    return v;
                }
            }
        };
        match d.shape {
            Shape::Sphere => p += in_ball(3),
            Shape::Disc => p += in_ball(2),
            Shape::Ring => {
                let a = rand01() * std::f32::consts::TAU;
                p += Vec3::new(a.cos() * r, 0.0, a.sin() * r);
            }
            // y += rand(0, r) (r is never 0 after the 0.01 clamp).
            Shape::Line => p.y += rand01() * r,
            Shape::SphereSurface => p += in_ball(3).normalize_or_zero() * r,
            Shape::Point => {}
        }
        let mut vel = w.rot * (self.rot * vel_e);
        if vel == Vec3::ZERO {
            vel = Vec3::splat(0.001);
        }
        let origin = w.point(self.rot * self.offset);
        let pos = w.point(self.rot * p);
        let mut part = Particle {
            life: d.life,
            age: 0.0,
            rotation: 1.0,
            pos,
            vel,
            ..Default::default()
        };
        if d.converge() {
            let speed = part.vel.length();
            let to = origin - pos;
            let dist = to.length();
            if dist == 0.0 {
                part.vel = Vec3::ZERO;
                part.life = 0.0;
            } else {
                part.vel = to / dist * speed;
                part.life = (dist / speed).min(part.life);
            }
        }
        if d.random_rotation() {
            part.rotation = (rand() % 360) as f32;
        }
        self.particles.push(part);
    }

    fn update_particles(&mut self, d: &Data, dt: f32, dtc: f32) {
        let (gravity_dt, drag_k) = if d.converge() { (0.0, 0.0) } else { (dtc, 1.0) };
        let rot_half = d.curve(c::ROTATION_SPEED).range * 0.5;
        let grav_half = d.curve(c::GRAVITY).range * 0.5;
        let orbit_half = d.curve(c::ORBIT).range * 0.5;
        let local = d.local();
        let frames = self.frames as i32;
        // Drag is applied once per engine update (no dt): the engine
        // has no fixed simulation rate: `Engine::Update` (Engine.dll
        // 0x1800752f0) runs once per frame with the frame's elapsed ms
        // (`GAME::UpdateTime`, 0x1801e96b0) and effects update in it
        // (`EffectEntity::UpdateSelf` → `Effect::Update` → `Emitter::Update`).
        // The port also updates once per rendered frame, so once per call.
        let drag_steps = 1.0;
        self.max_alpha = 0.0;
        let mut i = 0;
        while i < self.particles.len() {
            let p = &mut self.particles[i];
            let age = p.age + dt;
            p.age = age;
            if age >= p.life {
                let pos = p.pos;
                self.particles.swap_remove(i);
                if !d.drop_decal.is_empty() {
                    self.dropped.push((pos, (rand() % 360) as f32));
                }
                continue;
            }
            p.size = d.curve(c::SIZE).eval(age) * self.scale;
            p.stretch = d.curve(c::STRETCH).eval(age);
            p.pos += p.vel * dt;
            let orbit = d.curve(c::ORBIT).eval(age);
            if (orbit - orbit_half).abs() >= 0.01 && local {
                p.pos = Mat3::from_rotation_y((orbit - orbit_half) * dt) * p.pos;
            }
            p.rotation += (d.curve(c::ROTATION_SPEED).eval(age) - rot_half) * dt;
            p.vel.y -= (d.curve(c::GRAVITY).eval(age) - grav_half) * gravity_dt;
            let drag = d.curve(c::DRAG).eval(age) * drag_k;
            if drag != 0.0 {
                p.vel *= (1.0 - drag).max(0.0).powf(drag_steps);
            }
            p.color = Vec4::new(
                d.curve(c::RED).eval(age),
                d.curve(c::GREEN).eval(age),
                d.curve(c::BLUE).eval(age),
                d.curve(c::ALPHA).eval(age),
            );
            self.max_alpha = self.max_alpha.max(p.color.w);
            p.frame = if d.fps != 0 {
                ((d.fps as f32 * age) as i32).rem_euclid(frames) as u32
            } else {
                (((age / p.life) * frames as f32) as i32).rem_euclid(frames) as u32
            };
            i += 1;
        }
    }

    /// `Emitter::UpdateLight`: at the emitter origin, colour/radius from curves
    /// 22–25 at emitter time, intensity = max particle alpha.
    pub fn light(&self, d: &Data, coords: Coords) -> Option<LightState> {
        if !d.light() {
            return None;
        }
        let t = self.time;
        Some(LightState {
            position: self.rot * self.offset + coords.pos,
            color: Vec3::new(
                d.curve(c::LIGHT_RED).eval(t),
                d.curve(c::LIGHT_GREEN).eval(t),
                d.curve(c::LIGHT_BLUE).eval(t),
            ),
            radius: d.curve(c::LIGHT_RADIUS).eval(t),
            intensity: self.max_alpha.clamp(0.0, 1.0),
        })
    }
}

/// sub_180070CB0: the Catmull-Rom point at `t` between `p[1]` and `p[2]`.
fn catmull_rom(p: [Vec3; 4], t: f32) -> Vec3 {
    let [p0, p1, p2, p3] = p;
    (p1 * 2.0
        + (p2 - p0) * t
        + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * t * t
        + (p1 * 3.0 - p0 - p2 * 3.0 + p3) * t * t * t)
        * 0.5
}
