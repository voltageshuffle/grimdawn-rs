//! Weather: the rules of Engine.dll's `WeatherManager`: climate records, the region's climate
//! set, the weighted day / night pick with its 30 to 90 s re-roll, intensities, wind, the ambience adjustments and
//! the particle systems' spawn rules. Pure maths with a seedable generator; the renderer only draws
//! [`ParticleSystem::particles`] and flashes the [`Lightning`] light.
//!
//! Records (`records/game/weathermanager.dbr` `systems`, `weathersystem*.dbr`, `records/fx/climate/<set>.dbr`,
//! `records/fx/climate_weather/<n>.dbr`) load through [`Templates::load`], [`ClimateSet::load`]. All weather random
//! numbers come from one Park-Miller generator ([`Rng`] = `gd_rules::Rng`, `WeatherManager.rng`: state * 16807
//! mod 2^31-1, `rnd * 4.656613e-10` the unit float).
//!
//! PORT stand-ins: the intensity ramp (`WeatherSystem.intensityRamp`, set at once), the rain gust calm length and
//! noise, the fog burst line placement (`FogSystem::SpawnParticle`'s not transcribed), particle fades (engine: fog
//! 500 ms in, wind particles 500 ms in and out; here every type fades), the flat ground (the focus height) for
//! ground fog, the direction of the fog distance adjustment.
use crate::crt::CrtRand;
use gd_formats::vfs::Database;

use gd_rules::Rng;

/// The weather generator seeded from `seed` (`seed % (2^31-1)`, 0 → 1: a valid Park–Miller state).
fn seeded(seed: u32) -> Rng {
    Rng(match seed % 2_147_483_647 {
        0 => 1,
        s => s,
    })
}

/// Weather draws on the shared generator.
trait Draw {
    /// The next state, 1..2^31-2 (`rnd`).
    fn draw(&mut self) -> u32;
    /// Uniform in `lo..hi`.
    fn range(&mut self, lo: f32, hi: f32) -> f32;
    /// Uniform in `-r..r`.
    fn sym(&mut self, r: f32) -> f32;
}

impl Draw for Rng {
    fn draw(&mut self) -> u32 {
        self.next()
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.unit() * (hi - lo)
    }
    fn sym(&mut self, r: f32) -> f32 {
        self.range(-r, r)
    }
}

/// `weatherQuality` as the engine's index: the options' quality names 0 low .. 3 very high (ultra counts as 3).
pub fn quality_index(name: &str) -> u8 {
    match name {
        "low" => 0,
        "medium" => 1,
        "high" => 2,
        _ => 3,
    }
}

/// Spawn-delay multiplier `WeatherSystem::SetIntensity`: 3 / 2 / 1 for quality 0 / 1 / >= 2 (low weather has a third
/// of the particles).
pub fn quality_factor(quality: u8) -> i32 {
    match quality {
        0 => 3,
        1 => 2,
        _ => 1,
    }
}

/// `WeatherManager::maxParticles`, per system.
pub const MAX_PARTICLES: usize = 1024;

/// The particle system types (string compare in `WeatherManager::SetWeather`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Rain,
    Snow,
    Mist,
    Lightning,
    Fog,
    GroundFog,
    WindParticle,
    Leaves,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "Rain" => Self::Rain,
            "Snow" => Self::Snow,
            "Mist" => Self::Mist,
            "Lightning" => Self::Lightning,
            "Fog" => Self::Fog,
            "GroundFog" => Self::GroundFog,
            "WindParticle" => Self::WindParticle,
            "Leaves" => Self::Leaves,
            _ => return None,
        })
    }

    /// The spawn delay in ms at intensity `i` (0..1) `WeatherSystem::SetIntensity` (before the quality factor).
    pub fn delta_ms(self, i: f32) -> f32 {
        match self {
            Self::Rain => 20.0 - 18.0 * i,
            Self::Snow => 30.0 - 27.0 * i,
            Self::Mist => 100.0 - 90.0 * i,
            Self::GroundFog => 100.0 - 80.0 * i,
            Self::Fog | Self::WindParticle | Self::Leaves => 150.0 - 110.0 * i,
            Self::Lightning => 0.0,
        }
    }

    /// Loop-sound volume factor (`WeatherSystem::GetLoopVolume`): rain is louder.
    pub fn volume_factor(self) -> f32 {
        if self == Self::Rain { 1.2 } else { 1.0 }
    }
}

/// A `weathersystem*.dbr` (loader `WeatherSystem::Load`).
#[derive(Clone, Debug)]
pub struct Template {
    pub name: String,
    pub kind: Kind,
    pub texture: String,
    pub additive: bool,
    /// SoundPak record.
    pub sound: String,
    pub size: f32,
    pub speed: f32,
    /// Seconds (the engine stores ms).
    pub life: f32,
    pub radius: f32,
    pub height: f32,
}

impl Template {
    pub fn from_record(r: &gd_formats::arz::Record) -> Option<Self> {
        Some(Self {
            name: r.str("name")?.to_owned(),
            kind: Kind::parse(r.str("type")?)?,
            texture: r.str("texture").unwrap_or("").to_owned(),
            additive: r.str("blending") == Some("Additive"),
            sound: r.str("sound").unwrap_or("").to_owned(),
            size: r.f32("particleSize").unwrap_or(0.0),
            speed: r.f32("particleSpeed").unwrap_or(0.0),
            life: r.f32("particleLife").unwrap_or(0.0),
            radius: r.f32("spawnRadius").unwrap_or(0.0),
            height: r.f32("spawnHeight").unwrap_or(0.0),
        })
    }
}

/// The manager's templates in record order; two sharing a name: the first wins (`WeatherManager::Initialize`).
#[derive(Clone, Debug, Default)]
pub struct Templates {
    pub list: Vec<Template>,
    /// `windSound` SoundPak record.
    pub wind_sound: String,
}

impl Templates {
    pub fn load(db: &Database) -> Self {
        let Some(m) = db.get("records/game/weathermanager.dbr") else {
            return Self::default();
        };
        let list = m
            .strs("systems")
            .into_iter()
            .filter_map(|p| db.get(p))
            .filter_map(Template::from_record)
            .collect();
        Self {
            list,
            wind_sound: m.str("windSound").unwrap_or("").to_owned(),
        }
    }
    pub fn find(&self, name: &str) -> Option<&Template> {
        self.list.iter().find(|t| t.name == name)
    }
}

/// An entry's time filter (`ClimateSet::Load`): "" / "Always" always, "Day", anything else night.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    Always,
    Day,
    Night,
}

impl When {
    pub fn parse(s: &str) -> Self {
        match s {
            "" | "Always" => Self::Always,
            "Day" => Self::Day,
            _ => Self::Night,
        }
    }
}

/// A `climate_weather` record (loader `ClimateWeather::Load`): up to three systems with their intensity range (0..1)
/// and the ambience adjustments.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Climate {
    /// (system name, min, max intensity).
    pub systems: Vec<(String, f32, f32)>,
    pub darkness: f32,
    pub wind: f32,
    pub fog_density: f32,
    pub fog_distance: f32,
    pub fog_color: [f32; 3],
}

impl Climate {
    pub fn from_record(r: &gd_formats::arz::Record) -> Self {
        let mut systems = vec![];
        for n in 1..=3 {
            // The loader stops at the first empty system.
            let Some(name) = r.str(&format!("system{n}")) else {
                break;
            };
            let pct = |k: &str| r.f32(&format!("{k}Intensity{n}")).unwrap_or(0.0) * 0.01;
            systems.push((name.to_owned(), pct("min"), pct("max")));
        }
        let c = |ch: &str| r.f32(&format!("fogColorAdjustment.{ch}")).unwrap_or(0.0) * 0.01;
        Self {
            systems,
            darkness: r.f32("darknessAdjustment").unwrap_or(0.0),
            wind: r.f32("windIntensity").unwrap_or(0.0),
            fog_density: r.f32("fogDensityAdjustment").unwrap_or(0.0),
            fog_distance: r.f32("fogDistanceAdjustment").unwrap_or(0.0),
            fog_color: [c("R"), c("G"), c("B")],
        }
    }
}

/// One entry of a climate set with its climate.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub chance: i32,
    pub when: When,
    pub climate: Climate,
}

/// A `records/fx/climate/<set>.dbr`: up to 5 entries, loading stops at the first one with chance <= 0 or no record
/// (`ClimateSet::Load`), so a set can hold none.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClimateSet {
    pub entries: Vec<Entry>,
}

impl ClimateSet {
    pub fn load(db: &Database, record: &str) -> Option<Self> {
        let r = db.get(record)?;
        let mut entries = vec![];
        for n in 1..=5 {
            let chance = r.i32(&format!("climate{n}chance")).unwrap_or(0);
            let Some(path) = r.str(&format!("climate{n}dbr")) else {
                break;
            };
            if chance <= 0 {
                break;
            }
            let climate = db.get(path).map(Climate::from_record).unwrap_or_default();
            entries.push(Entry {
                chance,
                when: When::parse(r.str(&format!("climate{n}time")).unwrap_or("")),
                climate,
            });
        }
        Some(Self { entries })
    }
}

impl ClimateSet {
    /// A set of the one climate record `record`.
    pub fn single(db: &Database, record: &str) -> Option<Self> {
        let c = db.get(record)?;
        Some(Self {
            entries: vec![Entry {
                chance: 1,
                when: When::Always,
                climate: Climate::from_record(c),
            }],
        })
    }
}

/// The weighted pick `ClimateSet::PickWeather`: the eligible entries are those that are Always, Day while it is day,
/// Night while it is not. One eligible: it (no draw); none: `None`; else `r = rnd % (sum + 1)` and the first eligible
/// entry with `r <= running sum` wins.
pub fn pick(entries: &[Entry], is_day: bool, rng: &mut Rng) -> Option<usize> {
    let ok = |e: &Entry| match e.when {
        When::Always => true,
        When::Day => is_day,
        When::Night => !is_day,
    };
    let eligible: Vec<usize> = (0..entries.len()).filter(|&i| ok(&entries[i])).collect();
    match eligible.len() {
        0 => None,
        1 => Some(eligible[0]),
        _ => {
            let sum: i64 = eligible.iter().map(|&i| entries[i].chance as i64).sum();
            let r = rng.draw() as i64 % (sum + 1);
            let mut run = 0;
            for &i in &eligible {
                run += entries[i].chance as i64;
                if r <= run {
                    return Some(i);
                }
            }
            eligible.last().copied()
        }
    }
}

/// The re-roll timer after a pick `Update`: 30000 + rnd % 60001 ms.
pub fn reroll_ms(rng: &mut Rng) -> i32 {
    30_000 + (rng.draw() % 60_001) as i32
}

/// The ambience adjustments (`SetWeatherAdjustments`): darkness, fog density, fog distance, fog colour offsets.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Adjust {
    pub darkness: f32,
    pub fog_density: f32,
    pub fog_distance: f32,
    pub fog_color: [f32; 3],
}

impl Adjust {
    /// `WeatherManager::UpdateAmbience`: darkness / density / colour ease at 0.1 / s, the distance at 1.0 / s.
    pub fn ease_to(&mut self, target: &Adjust, dt: f32) {
        let step = |v: &mut f32, t: f32, rate: f32| {
            let d = rate * dt;
            *v += (t - *v).clamp(-d, d);
        };
        step(&mut self.darkness, target.darkness, 0.1);
        step(&mut self.fog_density, target.fog_density, 0.1);
        step(&mut self.fog_distance, target.fog_distance, 1.0);
        for i in 0..3 {
            step(&mut self.fog_color[i], target.fog_color[i], 0.1);
        }
    }
}

/// The wind (`WeatherManager.windDir` .. `WeatherManager.windTarget`). `WeatherManager::Update`: the
/// strength factor (`WeatherManager.windFactor`) eases at 0.1 / s to a target (`WeatherManager.windTarget`) re-rolled
/// on its own timer, every `10000 + rnd % 50001`
/// ms, to `1 + 2 * strength` (`strength` = the mean of the chosen climate's intensities that are > 0 times its
/// `windIntensity`, 0 without a choice); the direction turns towards the target direction `RandomizeWind` picks.
#[derive(Clone, Debug)]
pub struct Wind {
    pub factor: f32,
    target: f32,
    /// `RandomizeWind`'s timer (`WeatherManager.windDirTimer`) and the strength timer
    /// (`WeatherManager.windStrengthTimer`), ms.
    dir_timer_ms: i32,
    strength_timer_ms: i32,
    /// The current unit direction (x, z) (`WeatherManager.windDir`): what the particles and gusts follow.
    pub dir: [f32; 2],
    /// Where it turns to (`WeatherManager.windTargetDir`).
    pub target_dir: [f32; 2],
}

impl Wind {
    /// The constructor's `RandomizeWind(true)`: the direction is set at once.
    pub fn new(rng: &mut Rng) -> Self {
        let mut w = Self {
            factor: 1.0,
            target: 1.0,
            dir_timer_ms: 0,
            strength_timer_ms: 0,
            dir: [1.0, 0.0],
            target_dir: [1.0, 0.0],
        };
        w.randomize(rng, true);
        w
    }

    /// `WeatherManager::RandomizeWind`: the next pick in `10000 + rnd % 50001` ms; the target direction is
    /// `normalise(b, 0, a)` of two draws `a`, `b` uniform in -1..1 (a square, so the diagonals come up more often);
    /// `immediate` also turns the wind there at once.
    pub fn randomize(&mut self, rng: &mut Rng, immediate: bool) {
        self.dir_timer_ms = 10_000 + (rng.draw() % 50_001) as i32;
        let a = rng.unit() * 2.0 - 1.0;
        let b = rng.unit() * 2.0 - 1.0;
        let inv = 1.0 / (a * a + b * b).sqrt().max(1e-12);
        self.target_dir = [b * inv, a * inv];
        if immediate {
            self.dir = self.target_dir;
        }
    }

    /// The timers of `Update` (`dt` clamped to 100 ms): a new direction pick, a new strength target.
    pub fn tick(&mut self, dt_ms: i32, strength: f32, rng: &mut Rng) {
        self.dir_timer_ms -= dt_ms;
        if self.dir_timer_ms <= 0 {
            self.randomize(rng, false);
        }
        self.strength_timer_ms -= dt_ms;
        if self.strength_timer_ms <= 0 {
            self.strength_timer_ms = 10_000 + (rng.draw() % 50_001) as i32;
            self.target = 1.0 + 2.0 * strength;
        }
    }

    /// The easing at the end of `Update` (`dt` not clamped). While `dot(dir, target) < 0.9999` the direction
    /// turns about +Y by `((pi/128 - (factor - 1) * pi/256) * (1 - dot) + pi/64) * dt_s` towards the target (a stronger
    /// wind turns slower), or lands on it when that step reaches the angle left; the factor moves by at most
    /// `0.1 * dt_s` (only when more than 0.0001 away).
    pub fn ease(&mut self, dt_ms: f32) {
        use std::f32::consts::PI;
        let dt = dt_ms * 0.001;
        let [x, z] = self.dir;
        let [tx, tz] = self.target_dir;
        let d = x * tx + z * tz;
        if d < 0.9999 {
            let mut step =
                ((PI / 128.0 - (self.factor - 1.0) * (PI / 256.0)) * (1.0 - d) + PI / 64.0) * dt;
            if step <= d.clamp(-1.0, 1.0).acos().abs() {
                // The side: `dir x Y` = (-z, x); towards the target is a negative turn when it lies on that side.
                if -z * tx + x * tz >= 0.0 {
                    step = -step;
                }
                let (s, c) = step.sin_cos();
                self.dir = [x * c + z * s, -x * s + z * c];
            } else {
                self.dir = self.target_dir;
            }
        }
        let diff = self.target - self.factor;
        if diff.abs() > 0.0001 {
            self.factor += diff.signum() * (dt * 0.1).min(diff.abs());
        }
    }
}

/// One live particle.
#[derive(Clone, Copy, Debug, Default)]
pub struct Particle {
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    /// Remaining and total life, ms.
    pub life_ms: f32,
    pub total_ms: f32,
    /// Size in metres (template size x the cycle phase).
    pub size: f32,
    /// Spin angle, rad.
    pub spin: f32,
    pub spin_rate: f32,
}

impl Particle {
    /// Fade 0..1: in over the first and out over the last 500 ms (PORT for the types the engine does not fade).
    pub fn fade(&self) -> f32 {
        let age = self.total_ms - self.life_ms;
        let fin = (age / 500.0).clamp(0.0, 1.0);
        let fout = (self.life_ms / 500.0).clamp(0.0, 1.0);
        fin.min(fout)
    }
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l < 1e-6 {
        [0.0, -1.0, 0.0]
    } else {
        [v[0] / l, v[1] / l, v[2] / l]
    }
}

/// What the systems need each frame.
#[derive(Clone, Copy)]
pub struct Frame<'a> {
    /// The camera focus (world), the centre of every spawn area.
    pub cam: [f32; 3],
    pub wind_dir: [f32; 2],
    pub wind_factor: f32,
    pub dt_ms: f32,
    /// The world's floor: the quality 3 rain ray and the gusts' height (None: no world).
    pub ground: Option<&'a dyn Ground>,
}

/// The world queries weather needs.
pub trait Ground {
    /// The terrain height under (x, z).
    fn height(&self, x: f32, z: f32) -> Option<f32>;
    /// Distance along the unit `dir` from `origin` to the first physics surface (terrain, roofs, trees) within `len`
    /// (`World::GetIntersection`).
    fn ray(&self, origin: [f32; 3], dir: [f32; 3], len: f32) -> Option<f32>;
}

/// One running particle system (`WeatherSystem`, update `WeatherSystem::Update`).
#[derive(Clone, Debug)]
pub struct ParticleSystem {
    /// Unique per [`Weather`]: what the renderer keys its draw state by.
    pub id: u32,
    pub template: Template,
    pub particles: Vec<Particle>,
    pub intensity: f32,
    /// Spawning (`old > 0 || new > 0`); a stopped system lives until its particles are gone.
    pub active: bool,
    delta_ms: f32,
    timer_ms: f32,
    phase: f32,
    run_left: u32,
    cluster: Option<([f32; 3], f32)>,
    quality: u8,
    /// Rain gusts: ms left in the current calm / gust phase, whether a gust blows, and its tilt offset (x, z).
    gust_ms: f32,
    gusting: bool,
    gust_offset: [f32; 2],
    /// Rain drops that died this update (where they ended): each asks for a water ripple at quality > 1 (the
    /// death hook `RainSystem::OnParticleDeath`; [`Ripples`]).
    pub died: Vec<[f32; 3]>,
}

impl ParticleSystem {
    pub fn new(template: Template, quality: u8) -> Self {
        Self {
            id: 0,
            template,
            particles: Vec::new(),
            intensity: 0.0,
            active: false,
            delta_ms: 20.0,
            timer_ms: 0.0,
            phase: 0.75,
            run_left: 0,
            cluster: None,
            quality,
            gust_ms: 0.0,
            gusting: false,
            gust_offset: [0.0; 2],
            died: Vec::new(),
        }
    }

    /// The rain gust state, counted down once per update. Gusts exist only while
    /// `(windFactor - 1) / 2 >= 0.5`; a gust lasts `100 + rnd % 401` ms and tilts the base by `0.3 * (1 + noise)` on x / z.
    /// PORT: the calm phase is `1000 + rnd % 9001` ms, the noise a signed unit float per axis.
    fn update_gust(&mut self, f: &Frame<'_>, rng: &mut Rng) {
        if (f.wind_factor - 1.0) / 2.0 < 0.5 {
            self.gusting = false;
            return;
        }
        self.gust_ms -= f.dt_ms.min(100.0);
        if self.gust_ms <= 0.0 {
            self.gusting = !self.gusting;
            if self.gusting {
                self.gust_ms = 100.0 + (rng.draw() % 401) as f32;
                self.gust_offset = [
                    0.3 * (1.0 + rng.unit()) * if rng.unit() < 0.5 { -1.0 } else { 1.0 },
                    0.3 * (1.0 + rng.unit()) * if rng.unit() < 0.5 { -1.0 } else { 1.0 },
                ];
            } else {
                self.gust_ms = 1000.0 + (rng.draw() % 9001) as f32;
            }
        }
    }

    /// The rain tilt vector (`RainWeatherSystem::GetTilt`): base `(0.3 wx, -1, 0.3 wz)` normalised, plus the gust
    /// offset while [`Self::update_gust`] has a gust blowing.
    fn rain_tilt(&self, f: &Frame<'_>) -> [f32; 3] {
        let [wx, wz] = f.wind_dir;
        let [ox, oz] = if self.gusting {
            self.gust_offset
        } else {
            [0.0; 2]
        };
        norm([0.3 * wx + ox, -1.0, 0.3 * wz + oz])
    }

    /// `WeatherSystem::SetIntensity`: the spawn delay is `int(deltaFn(i) * Q)` ms.
    pub fn set_intensity(&mut self, i: f32, quality: u8) {
        self.active = self.intensity > 0.0 || i > 0.0;
        if i <= 0.0 {
            self.active = false;
        }
        self.intensity = i;
        self.quality = quality;
        self.delta_ms = ((self.template.kind.delta_ms(i) * quality_factor(quality) as f32) as i32)
            .max(1) as f32;
    }

    /// Stops spawning (a climate change fades the system out).
    pub fn stop(&mut self) {
        self.active = false;
        self.intensity = 0.0;
    }

    /// Finished (`WeatherSystem::IsFinished`): stopped and empty.
    pub fn finished(&self) -> bool {
        !self.active && self.particles.is_empty()
    }

    /// Loop volume (`WeatherSystem::GetLoopVolume`): `min(factor * count / max, 1)`.
    pub fn volume(&self) -> f32 {
        (self.template.kind.volume_factor() * self.particles.len() as f32 / MAX_PARTICLES as f32)
            .min(1.0)
    }

    /// Generic update: age and move the particles, then spawn (at most 500 a frame, up to [`MAX_PARTICLES`]).
    pub fn update(&mut self, f: &Frame<'_>, rng: &mut Rng) {
        let dt = f.dt_ms.min(100.0);
        self.died.clear();
        let t = &self.template;
        let kind = t.kind;
        let mut i = 0;
        while i < self.particles.len() {
            let p = &mut self.particles[i];
            p.life_ms -= dt;
            if p.life_ms <= 0.0 {
                if kind == Kind::Rain {
                    self.died.push(p.pos);
                }
                self.particles.swap_remove(i);
                continue;
            }
            // Movement scales with the particle's size cycle (`vel * dt * size / particleSize`).
            let k = if matches!(kind, Kind::Fog | Kind::GroundFog) || t.size <= 0.0 {
                1.0
            } else {
                p.size / t.size
            };
            for a in 0..3 {
                p.pos[a] += p.vel[a] * dt * 0.001 * k;
            }
            p.spin += p.spin_rate * dt * 0.001;
            i += 1;
        }
        if !self.active {
            return;
        }
        if kind == Kind::Rain {
            self.update_gust(f, rng);
        }
        self.timer_ms -= dt;
        let mut spawned = 0;
        while self.timer_ms <= 0.0 && spawned < 500 {
            self.timer_ms += self.delta_ms;
            spawned += 1;
            if self.particles.len() < MAX_PARTICLES {
                let p = self.spawn(f, rng);
                self.particles.push(p);
            }
        }
    }

    fn next_phase(&mut self) -> f32 {
        let p = self.phase;
        let (lo, hi) = if self.template.kind == Kind::Rain {
            (0.5, 0.5 * self.intensity + 1.0)
        } else {
            (0.75, 1.25)
        };
        self.phase += 0.05;
        if self.phase > hi || self.phase < lo {
            self.phase = lo;
        }
        p.clamp(lo, hi)
    }

    /// One new particle, by the type's spawn rules.
    fn spawn(&mut self, f: &Frame<'_>, rng: &mut Rng) -> Particle {
        let Template {
            kind,
            size,
            speed,
            life,
            radius: r,
            height,
            ..
        } = self.template;
        let [wx, wz] = f.wind_dir;
        let wf = f.wind_factor;
        let phase = self.next_phase();
        let mut p = Particle {
            life_ms: life * 1000.0,
            total_ms: life * 1000.0,
            size: size * phase,
            ..Default::default()
        };
        let around = |rng: &mut Rng| [f.cam[0] + rng.sym(r), f.cam[2] + rng.sym(r)];
        match kind {
            Kind::Snow | Kind::Mist => {
                let [x, z] = around(rng);
                p.pos = [x, f.cam[1] + 20.0, z];
                let d = norm([0.3 * wx, -1.0, 0.3 * wz]);
                p.vel = d.map(|c| c * speed);
            }
            Kind::Rain => {
                let [x, z] = around(rng);
                p.pos = [x, f.cam[1] + height, z];
                let d = self.rain_tilt(f);
                p.vel = [
                    d[0] * speed * rng.range(0.8, 1.2),
                    d[1] * speed * rng.range(0.8, 1.2),
                    d[2] * speed * rng.range(0.8, 1.2),
                ];
                // `RainSystem::SpawnParticle`: at weatherQuality 3 a ray of length 2 * spawnRadius from the spawn
                // point along the drop's direction cuts the life to `hit distance / particleSpeed` ms, so the drop
                // (and its ripple) ends at the ground, or a roof or tree.
                if self.quality >= 3
                    && let Some(dist) = f.ground.and_then(|g| g.ray(p.pos, d, 2.0 * r))
                {
                    p.life_ms = dist / speed * 1000.0;
                    p.total_ms = p.life_ms;
                }
            }
            Kind::Leaves => {
                let [x, z] = around(rng);
                p.pos = [x, f.cam[1] + height, z];
                let d = norm([0.5 * wf * wx, -0.25 - rng.unit() * 0.75, 0.5 * wf * wz]);
                p.vel = d.map(|c| c * speed * 0.5 * wf);
                p.spin_rate = rng.range(2.75, 3.53);
            }
            Kind::WindParticle => {
                let [x, z] = around(rng);
                p.pos = [x, f.cam[1] + 2.0 + rng.unit() * 3.0, z];
                let d = norm([0.5 * wf * wx, rng.sym(1.0), 0.5 * wf * wz]);
                p.vel = d.map(|c| c * speed * 0.5 * wf);
            }
            Kind::Fog => {
                if self.run_left == 0 {
                    self.run_left = rng.draw() % 41 + 10;
                    self.cluster = (rng.unit() < 0.25).then(|| {
                        let scale = rng.range(2.0, 4.0);
                        (
                            [
                                f.cam[0] + rng.sym(r) + wx * 10.0,
                                0.0,
                                f.cam[2] + rng.sym(r) + wz * 10.0,
                            ],
                            scale,
                        )
                    });
                }
                self.run_left -= 1;
                let (x, z) = match self.cluster {
                    // PORT: placed along a line across the wind.
                    Some((c, scale)) => {
                        let s = rng.sym(r * 0.25 * scale);
                        (c[0] - wz * s, c[2] + wx * s)
                    }
                    None => {
                        let [x, z] = around(rng);
                        (x, z)
                    }
                };
                p.pos = [x, f.cam[1] + 1.0 + rng.unit() * 4.0, z];
                let v = (wf * speed).max(0.01);
                p.vel = [wx * v, 0.0, wz * v];
                p.life_ms = 2.0 * r / v * 1000.0;
                p.total_ms = p.life_ms;
                p.spin_rate = rng.sym(0.349);
            }
            Kind::GroundFog => {
                let [x, z] = around(rng);
                // PORT: the floor is the focus height (`World::PutOnFloor` stand-in).
                p.pos = [x, f.cam[1], z];
                p.vel = [rng.sym(speed), 0.0, rng.sym(speed)];
                p.spin = rng.sym(std::f32::consts::TAU);
            }
            Kind::Lightning => {}
        }
        p
    }
}

/// The lightning flash (update `LightningWeatherSystem::Update`): a white point light (range 100) near the camera.
/// Strikes come after `100 + rnd % (14901 - 13000 * i)` ms, the first one too (counted from the frame lightning
/// becomes active); a strike is 2 or 3 flashes, each on `(1 + u) * 40` ms and off `(1 + u) * 35` ms. There is no
/// thunder and no bolt in the weather path.
#[derive(Clone, Debug, Default)]
pub struct Lightning {
    timer_ms: f32,
    flashes_left: u32,
    on: bool,
    /// The strike interval has been drawn (false after a reset: the next update starts the wait).
    started: bool,
    /// Offset of the light from the camera while on: x, z within +-30 m, y 30.
    pub offset: [f32; 3],
}

impl Lightning {
    pub const RANGE: f32 = 100.0;

    pub fn is_on(&self) -> bool {
        self.on
    }

    /// The wait before the next strike. The range shrinks with the intensity and stays at least 1 (an intensity
    /// above ~1.15 would take it below 0).
    fn interval_ms(intensity: f32, rng: &mut Rng) -> f32 {
        let range = 14_901u32.saturating_sub((13_000.0 * intensity.max(0.0)) as u32);
        100.0 + (rng.draw() % range.max(1)) as f32
    }

    pub fn update(&mut self, dt_ms: f32, intensity: f32, rng: &mut Rng) {
        if !self.started {
            self.started = true;
            self.timer_ms = Self::interval_ms(intensity, rng);
            return;
        }
        self.timer_ms -= dt_ms.min(100.0);
        if self.timer_ms > 0.0 {
            return;
        }
        if self.on {
            self.on = false;
            self.flashes_left = self.flashes_left.saturating_sub(1);
            self.timer_ms = if self.flashes_left > 0 {
                (1.0 + rng.unit()) * 35.0
            } else {
                Self::interval_ms(intensity, rng)
            };
        } else if self.flashes_left > 0 {
            self.on = true;
            self.timer_ms = (1.0 + rng.unit()) * 40.0;
        } else {
            self.flashes_left = 2 + rng.draw() % 2;
            self.offset = [rng.sym(30.0), 30.0, rng.sym(30.0)];
            self.on = true;
            self.timer_ms = (1.0 + rng.unit()) * 40.0;
        }
    }
}

/// One slot of a [`Ripples`] set.
#[derive(Clone, Copy, Debug, Default)]
pub struct Ripple {
    /// Centre on the water surface (+0.01 m).
    pub pos: [f32; 3],
    /// Half side of the quad (m).
    pub size: f32,
    pub max: f32,
    pub active: bool,
    /// Drawn this frame (set by [`Ripples::update`]) with this alpha.
    pub drawn: bool,
    pub alpha: f32,
}

/// A water's ripple set (`Water.ripples`; ctor `WaterRipples::Construct`, add `WaterRipples::Add`, update
/// `WaterRipples::Update`): [`Ripples::SLOTS`] slots, a new ripple takes the first free one (none free: dropped).
#[derive(Clone, Debug)]
pub struct Ripples {
    pub slots: [Ripple; Ripples::SLOTS],
    rand: CrtRand,
}

impl Default for Ripples {
    fn default() -> Self {
        Self {
            slots: [Ripple::default(); Self::SLOTS],
            rand: CrtRand::default(),
        }
    }
}

impl Ripples {
    pub const SLOTS: usize = 128;
    /// Growth in m/s (ctor: `WaterRipples.rate` = 1.0, times dt ms * 0.001).
    pub const RATE: f32 = 1.0;

    /// `Water::CreateRipple(pos, size, strength, 0)` with the water height already found: `water` = the
    /// nearest water surface an upward ray from `pos.y - 2` hits (`GetWaterPointInfo`), if any. The hit
    /// must lie below `pos.y + 2`; a ripple is added only when `strength >= 1`, its size parameter is `size * 4`.
    /// Returns whether the point was on water (the engine's return, also when no ripple was added).
    pub fn create(&mut self, pos: [f32; 3], water: Option<f32>, size: f32, strength: f32) -> bool {
        let Some(w) = water else {
            return false;
        };
        if w - (pos[1] - 2.0) >= 4.0 {
            return false;
        }
        if strength >= 1.0 {
            self.add([pos[0], w, pos[2]], size * 4.0);
        }
        true
    }

    /// `WaterRipples::Add`: the first free slot; size 0.3..0.7, max `s +- 0.2`, centre jittered +-0.2 m on x / z and
    /// 0.01 m above the surface (`rand() / 32767` per draw, in this order: size, max, x, z).
    pub fn add(&mut self, at: [f32; 3], s: f32) -> bool {
        let Some(i) = self.slots.iter().position(|r| !r.active) else {
            return false;
        };
        let mut u = || self.rand.rand() as f32 / 32767.0;
        let size = u() * 0.4 + 0.3;
        let max = u() * ((s + 0.2) - (s - 0.2)) + (s - 0.2);
        let x = (u() * 0.4 - 0.2) + at[0];
        let z = (u() * 0.4 - 0.2) + at[2];
        self.slots[i] = Ripple {
            pos: [x, at[1] + 0.01, z],
            size,
            max,
            active: true,
            drawn: false,
            alpha: 0.0,
        };
        true
    }

    /// `WaterRipples::Update` (`Water::Update`): a slot that reached its max goes; the others grow by `dt * RATE`
    /// (capped at the max) and are drawn with the triangle alpha of the NEW size: up to 1 at half the max, back to 0
    /// at the max.
    pub fn update(&mut self, dt_ms: f32) {
        let step = dt_ms * Self::RATE * 0.001;
        for r in &mut self.slots {
            r.drawn = false;
            if !r.active {
                continue;
            }
            if r.size >= r.max {
                r.active = false;
                continue;
            }
            r.size = (r.size + step).min(r.max);
            let half = r.max * 0.5;
            r.alpha = if r.size < half {
                r.size / half
            } else {
                1.0 - (r.size - half) / half
            };
            r.drawn = true;
        }
    }

    /// The fixed turn of slot `i`'s quad (`i * 16777620.0` rad as a float): the quad's corners are
    /// `c +- r +- q` with `r = (cos a, 0, sin a) * size`, `q = (-sin a, 0, cos a) * size`.
    pub fn angle(i: usize) -> f32 {
        i as f32 * 16_777_620.0
    }
}

/// A directional wind push (`World::CreateDirectionalWind`): bends the grass around `pos`
/// (`Terrain::CreateGrassWind`) and calls `Water::CreateRipple(pos, radius, strength, 0)` on the water there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirectionalWind {
    pub pos: [f32; 3],
    /// Unit direction (x, z).
    pub dir: [f32; 2],
    pub radius: f32,
    pub strength: f32,
}

/// One `advancedWind` gust (`WeatherManager.gusts` list, `WeatherManager::Update`): it moves downwind from 20 m
/// upwind of the camera and pushes the ground every 50 ms with an envelope over its life.
#[derive(Clone, Debug)]
pub struct Gust {
    pub pos: [f32; 3],
    pub dir: [f32; 2],
    life_ms: i32,
    total_ms: i32,
    /// `s^2` at the spawn (s = (factor - 1) / 2).
    strength: f32,
    /// m/s.
    speed: f32,
    radius: f32,
    tick_ms: i32,
    /// Envelope 1: four rises and falls (else one).
    pulse: bool,
}

impl Gust {
    /// One rise to the middle of the life and one fall, or (`pulse`) a triangle wave of period half the life,
    /// at least 0.25 (`life` = ms left; the quarter and half are integer divisions).
    fn envelope(&self) -> f32 {
        let life = self.life_ms;
        let e = if self.pulse {
            let q = (self.total_ms / 4).max(1);
            let e = (life % q) as f32 / q as f32;
            if (life / q) & 1 != 0 { 1.0 - e } else { e }
        } else {
            let half = (self.total_ms / 2).max(1);
            if life >= half {
                1.0 - (life - half) as f32 / half as f32
            } else {
                life as f32 / half as f32
            }
        };
        e.max(0.25)
    }
}

/// The whole weather: the manager's climate state plus the systems it drives.
#[derive(Clone, Debug)]
pub struct Weather {
    pub rng: Rng,
    pub templates: Templates,
    /// The set record in use (`""` = none: "no climate").
    pub set_name: String,
    set: ClimateSet,
    no_climate: bool,
    choice: Option<usize>,
    timer_ms: f32,
    /// Chosen climate's systems with their current intensity.
    pub used: Vec<(String, f32)>,
    pub wind: Wind,
    pub adjust: Adjust,
    pub systems: Vec<ParticleSystem>,
    pub lightning: Lightning,
    /// Counts picks (a pick restarts the systems).
    pub picks: u32,
    next_id: u32,
    /// `advancedWind` gusts (at most 3 spawn) and the spawn timer (`WeatherManager.gustTimer`).
    pub gusts: Vec<Gust>,
    gust_timer_ms: i32,
    /// This update's directional wind pushes (the gusts' 50 ms ticks).
    pub winds: Vec<DirectionalWind>,
}

impl Weather {
    pub fn new(seed: u32, templates: Templates) -> Self {
        let mut rng = seeded(seed);
        let wind = Wind::new(&mut rng);
        Self {
            rng,
            templates,
            set_name: String::new(),
            set: ClimateSet::default(),
            no_climate: true,
            choice: None,
            timer_ms: 90_000.0,
            used: vec![],
            wind,
            adjust: Adjust::default(),
            systems: vec![],
            lightning: Lightning::default(),
            picks: 0,
            next_id: 0,
            gusts: vec![],
            gust_timer_ms: 0,
            winds: vec![],
        }
    }

    /// `WeatherManager::RefreshIntensities`: the running systems take a changed `weatherQuality` at once.
    pub fn refresh_quality(&mut self, quality: u8) {
        for s in &mut self.systems {
            if s.active {
                s.set_intensity(s.intensity, quality);
            }
        }
    }

    /// The picked climate, if any.
    pub fn climate(&self) -> Option<&Climate> {
        self.choice
            .and_then(|i| self.set.entries.get(i))
            .map(|e| &e.climate)
    }

    /// `WeatherManager::ChangeClimate`: a different set name reloads and picks at once; the same name is a no-op;
    /// `None` is "no sector data": no choice, no re-roll until a set appears.
    pub fn change_climate(
        &mut self,
        name: &str,
        set: Option<ClimateSet>,
        is_day: bool,
        quality: u8,
    ) {
        if name == self.set_name {
            return;
        }
        self.set_name = name.to_owned();
        match set {
            Some(s) if !name.is_empty() => {
                self.set = s;
                self.no_climate = false;
                self.reroll(is_day, quality);
            }
            _ => {
                self.set = ClimateSet::default();
                self.no_climate = true;
                self.choice = None;
                self.timer_ms = 90_000.0;
                self.apply(quality);
            }
        }
    }

    /// Picks an entry and restarts the systems (`UpdateWeatherSystemsForCurrentClimate`).
    pub fn reroll(&mut self, is_day: bool, quality: u8) {
        self.choice = pick(&self.set.entries, is_day, &mut self.rng);
        self.timer_ms = reroll_ms(&mut self.rng) as f32;
        self.picks += 1;
        self.apply(quality);
    }

    /// Chosen systems get `cur = min + rnd * (max - min)` (`ClimateWeather::RollIntensity`); every other running
    /// system is stopped.
    fn apply(&mut self, quality: u8) {
        self.used.clear();
        let systems = self
            .climate()
            .map(|c| c.systems.clone())
            .unwrap_or_default();
        for (name, lo, hi) in systems {
            let v = lo + self.rng.unit() * (hi - lo);
            self.used.push((name, v.clamp(0.0, 1.0)));
        }
        for s in &mut self.systems {
            if !self.used.iter().any(|(n, _)| *n == s.template.name) {
                s.stop();
            }
        }
        for (name, v) in self.used.clone() {
            let Some(t) = self.templates.find(&name).cloned() else {
                continue; // "WeatherManager - No template for weather type"
            };
            if t.kind == Kind::Lightning {
                continue;
            }
            // Reuse a running system of the template, else create one.
            match self
                .systems
                .iter_mut()
                .find(|s| s.template.name == name && s.active)
            {
                Some(s) => s.set_intensity(v, quality),
                None => {
                    let mut s = ParticleSystem::new(t, quality);
                    self.next_id += 1;
                    s.id = self.next_id;
                    s.set_intensity(v, quality);
                    self.systems.push(s);
                }
            }
        }
    }

    /// The lightning system's intensity (0 when the climate has none).
    pub fn lightning_intensity(&self) -> f32 {
        self.used
            .iter()
            .filter(|(n, _)| {
                self.templates
                    .find(n)
                    .is_some_and(|t| t.kind == Kind::Lightning)
            })
            .map(|(_, v)| *v)
            .fold(0.0, f32::max)
    }

    /// `ClimateWeather::GetMeanIntensity`: the mean of the chosen climate's intensities that are > 0 (0 when none is).
    pub fn mean_intensity(&self) -> f32 {
        let (sum, n) = self
            .used
            .iter()
            .map(|u| u.1)
            .filter(|v| *v > 0.0)
            .fold((0.0f32, 0u32), |(s, n), v| (s + v, n + 1));
        if n == 0 { 0.0 } else { sum / n as f32 }
    }

    /// The target adjustments: [`Weather::mean_intensity`] times the climate's values.
    pub fn target_adjust(&self) -> Adjust {
        let Some(c) = self.climate() else {
            return Adjust::default();
        };
        let m = self.mean_intensity();
        Adjust {
            darkness: m * c.darkness,
            fog_density: m * c.fog_density,
            fog_distance: m * c.fog_distance,
            fog_color: c.fog_color.map(|v| m * v),
        }
    }

    /// `WeatherManager::Update`: re-roll on timer expiry (only with a climate), wind, ambience easing, systems.
    pub fn update(
        &mut self,
        cam: [f32; 3],
        dt_ms: f32,
        is_day: bool,
        quality: u8,
        ground: Option<&dyn Ground>,
    ) {
        if !self.no_climate {
            self.timer_ms -= dt_ms;
            if self.timer_ms < 0.0 {
                self.reroll(is_day, quality);
            }
        }
        // The systems move with the wind as it was; then the wind's timers and easing (`Update` order).
        let f = Frame {
            cam,
            wind_dir: self.wind.dir,
            wind_factor: self.wind.factor,
            dt_ms,
            ground,
        };
        for s in &mut self.systems {
            s.update(&f, &mut self.rng);
        }
        self.systems.retain(|s| !s.finished());
        let strength = self
            .climate()
            .map_or(0.0, |c| self.mean_intensity() * c.wind);
        let dt = dt_ms.min(100.0) as i32;
        self.wind.tick(dt, strength, &mut self.rng);
        self.winds.clear();
        if quality >= 2 {
            self.update_gusts(cam, dt, ground);
        }
        self.wind.ease(dt_ms);
        let target = self.target_adjust();
        self.adjust.ease_to(&target, dt_ms * 0.001);
        let li = self.lightning_intensity();
        if li > 0.0 {
            self.lightning.update(dt_ms, li, &mut self.rng);
        } else {
            self.lightning = Lightning::default();
        }
    }

    /// `advancedWind` (`weatherQuality` 2 or 3 `WeatherManager::RefreshIntensities`), `Update`'s gust block
    /// (`dt` clamped to 100 ms, s = (factor - 1) / 2): each gust ages; every 50 ms it pushes with
    /// `radius * e` and `s0^2 * e` (e = [`Gust::envelope`]) after moving `speed * 0.05` m downwind and onto the floor
    /// (PORT: the terrain height, `ground`). While the factor is above 1.25 and fewer than 3 blow, a new one comes after
    /// `lo + rnd % (hi - lo + 1)` ms (lo = int(4000 - 3000 s), hi = int(8000 - 6000 s)): 20 m upwind of the camera,
    /// `rnd * 40 - 20` m across, speed `5 + 6 rnd + 12 s` m/s, radius `3 + 4 rnd + 5 s` m, life
    /// `(rnd + 1) * 30000 / speed` ms, envelope `rnd & 1`. The particle modifier it registers is dead (type 1; the
    /// `ApplyModifier` handlers act on type 0 only), so weather particles are not pushed.
    fn update_gusts(&mut self, cam: [f32; 3], dt: i32, ground: Option<&dyn Ground>) {
        let winds = &mut self.winds;
        self.gusts.retain_mut(|g| {
            g.life_ms -= dt;
            if g.life_ms <= 0 {
                return false;
            }
            g.tick_ms -= dt;
            if g.tick_ms <= 0 {
                let e = g.envelope();
                g.tick_ms = 50;
                g.pos[0] += g.dir[0] * g.speed * 0.05;
                g.pos[2] += g.dir[1] * g.speed * 0.05;
                if let Some(y) = ground.and_then(|f| f.height(g.pos[0], g.pos[2])) {
                    g.pos[1] = y;
                }
                winds.push(DirectionalWind {
                    pos: g.pos,
                    dir: g.dir,
                    radius: e * g.radius,
                    strength: e * g.strength,
                });
            }
            true
        });
        if self.wind.factor <= 1.25 || self.gusts.len() >= 3 {
            return;
        }
        self.gust_timer_ms -= dt;
        if self.gust_timer_ms > 0 {
            return;
        }
        let s = (self.wind.factor - 1.0) * 0.5;
        let lo = (4000.0 - s * 3000.0) as i32;
        let m = match 1 - (s * 6000.0 - 8000.0) as i32 - lo {
            0 => 1,
            m => m,
        };
        let rng = &mut self.rng;
        self.gust_timer_ms = lo + (rng.draw() % m as u32) as i32;
        let across = rng.unit() * 40.0 - 20.0;
        let [x, z] = self.wind.dir;
        // `dir x Y` = (-z, x) across the wind; the gust starts 20 m upwind.
        let pos = [
            cam[0] - z * across - x * 20.0,
            cam[1],
            cam[2] + x * across - z * 20.0,
        ];
        let speed = rng.unit() * 6.0 + 5.0 + s * 12.0;
        let radius = rng.unit() * 4.0 + 3.0 + s * 5.0;
        let life = ((rng.unit() + 1.0) * 30_000.0 / speed) as i32;
        let pulse = rng.draw() & 1 != 0;
        self.gusts.push(Gust {
            pos,
            dir: self.wind.dir,
            life_ms: life,
            total_ms: life,
            strength: s * s,
            speed,
            radius,
            tick_ms: 50,
            pulse,
        });
    }

    /// `Update`: the `windSound` loop plays while the factor is above 1.25 (stopped, not faded, below).
    pub fn wind_sound_on(&self) -> bool {
        self.wind.factor > 1.25
    }

    /// The wind loop's volume `Update`: `min(gusts, 3) * 0.1 + (factor - 1) * 0.5 * 0.7`.
    pub fn wind_volume(&self) -> f32 {
        self.gusts.len().min(3) as f32 * 0.1 + (self.wind.factor - 1.0) * 0.5 * 0.7
    }

    /// `SetEnabled(false)`: all systems gone at once.
    pub fn clear(&mut self) {
        self.systems.clear();
        self.gusts.clear();
        self.winds.clear();
        self.lightning = Lightning::default();
        self.adjust = Adjust::default();
        self.used.clear();
        self.set_name.clear();
        self.set = ClimateSet::default();
        self.choice = None;
        self.no_climate = true;
    }
}
