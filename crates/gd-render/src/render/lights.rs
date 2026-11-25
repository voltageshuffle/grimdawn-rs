//! The level lights: a placed point light (`LightCpu`) and its record loaders, the `FireLight` flicker
//! and `PulseLight` pulse, and the lights' shared CRT `rand()` stream.
use bevy::prelude::*;
use gd_sim::crt::CrtRand;

/// A point light placed in a level (`FireLight` flickers; `DayNightLight`
/// blends its day/night colours).
pub struct LightCpu {
    pub position: Vec3,
    pub color: Vec3,
    /// DayNightLight: (day, night) colours.
    pub day_night: Option<(Vec3, Vec3)>,
    pub radius: f32,
    pub flicker: Option<Flicker>,
    pub pulse: Option<Pulse>,
    /// The record's `shadowPriority` (`Light::Load`, `Light.shadowPriority`).
    pub shadow_priority: i32,
}

/// A `FireLight`'s on / off steps and radius pulse.
/// Engine.dll `FireLight::Load` (times are the record's seconds × 1000 as ints; a
/// missing field reads 0) and `UpdateSelf`, every update with `dt` ms:
/// - `timer -= dt`; at `timer <= 0` the light toggles. Leaving the offset phase it takes the record colour
///   back for `lerp(minOff, minOn, u)` ms; entering it, one `r = lerp(flickerMin, flickerMax, u)` is added to
///   each channel, `sat(colour + r)`, for `lerp(maxOff, maxOn, u)` ms (u = `rand() / 32767`, one draw for r
///   and one for the length; the duration is truncated to an int). The timer starts at 0 and the phase flag
///   is never initialised (heap memory, the port takes "base": the first update enters the offset phase).
/// - The radius moves by `expandSpeed × min(dt, 100) / 1000` m, growing until it reaches `maxRadius`, then
///   shrinking until `minRadius` (the flag flips once the bound is passed; it starts growing, ctor
///   `FireLight::FireLight`), from `lerp(minRadius, maxRadius, u)` drawn at `Load` after the unused `bobAmount` phase.
/// - `bobAmount` / `bobSpeed` are loaded, never read by the update.
///
/// `rand` is the CRT's (MSVC LCG, [`CrtRand`]), one stream shared with every other `rand()` caller of the
/// process. PORT: the lights draw from their own shared stream, [`LightRand`] (seed 1, the CRT default; the engine's
/// stream position depends on every other caller), and `Load`'s two draws happen at the light's first update.
/// `Light::AddToScene` hands colour × intensity (1) and the radius to the renderer every frame.
#[derive(Component, Clone, Copy, Debug)]
pub struct Flicker {
    base: Vec3,
    /// flickerMin, flickerMax.
    range: (f32, f32),
    /// The base phase's (minOff, minOn) and the offset phase's (maxOff, maxOn) lengths, ms.
    base_ms: (i32, i32),
    offset_ms: (i32, i32),
    /// minRadius, maxRadius, expandSpeed.
    radii: (f32, f32),
    expand: f32,
    timer: i32,
    offset: bool,
    growing: bool,
    /// The live colour and radius; `None` until the first update rolls the start radius.
    pub color: Vec3,
    pub radius: Option<f32>,
}

/// The lights' CRT `rand()` stream.
#[derive(Resource, Default)]
pub struct LightRand(pub CrtRand);

impl Flicker {
    pub fn from_record(rec: &gd_formats::arz::Record, base: Vec3) -> Self {
        let f = |k: &str| rec.f32(k).unwrap_or(0.0);
        let ms = |k: &str| (f(k) * 1000.0) as i32;
        Self {
            base,
            range: (f("flickerMin"), f("flickerMax")),
            base_ms: (ms("minOff"), ms("minOn")),
            offset_ms: (ms("maxOff"), ms("maxOn")),
            radii: (f("minRadius"), f("maxRadius")),
            expand: f("expandSpeed"),
            timer: 0,
            offset: false,
            growing: true,
            color: base,
            radius: None,
        }
    }

    /// One `FireLight::UpdateSelf` of `dt` ms; returns the radius.
    pub fn update(&mut self, dt: i32, rng: &mut CrtRand) -> f32 {
        let mut radius = match self.radius {
            Some(r) => r,
            None => {
                // `Load`: the bob phase, then the start radius.
                rng.rand();
                rng.unit() * (self.radii.1 - self.radii.0) + self.radii.0
            }
        };
        let step = dt.min(100) as f32 * 0.001;
        self.timer -= dt;
        if self.timer <= 0 {
            let leaving = self.offset;
            self.offset = !leaving;
            let (lo, hi) = if leaving {
                self.color = self.base;
                self.base_ms
            } else {
                let r = rng.unit() * (self.range.1 - self.range.0) + self.range.0;
                self.color = (self.base + Vec3::splat(r)).clamp(Vec3::ZERO, Vec3::ONE);
                self.offset_ms
            };
            self.timer =
                ((hi - lo) as f32 * rng.rand() as f32 * 0.000_030_518_51 + lo as f32) as i32;
        }
        let d = step * self.expand;
        if self.growing {
            radius += d;
            if radius >= self.radii.1 {
                self.growing = false;
            }
        } else {
            radius -= d;
            if radius <= self.radii.0 {
                self.growing = true;
            }
        }
        self.radius = Some(radius);
        radius
    }
}

/// A `PulseLight`'s colour swing and radius pulse (the cultist altars' red glow, the dust and red beams of
/// some caves). Engine.dll `PulseLight::Load` (a missing field reads 0; `Light::Load`
/// is not called, so `radius` / `color` are unused) and `UpdateSelf`, every update with `dt` ms:
/// - `s = min(dt, 100) / 1000`; the colour timer `t += s`, and once `t > colorPeriod` it restarts at 0 and the
///   direction flips. The colour is `lerp(firstColor, secondColor, sat(t))` going one way, `lerp(secondColor,
///   firstColor, sat(t))` the other (it starts first → second: ctor `PulseLight::PulseLight`): the clamp is on the
///   timer itself, not `t / colorPeriod`, so a swing takes 1 s and the colour then holds for the rest of the
///   period. `Load` starts the timer at `colorPeriod × rand() / 32767`.
/// - The radius starts at `minRadius` and moves by `expandSpeed × s` m, growing until it reaches
///   `maxRadius`, then shrinking until `minRadius` (the flag flips once the bound is passed), as a
///   `FireLight`'s.
///
/// PORT: the timer's `rand()` is the lights' [`LightRand`], drawn at the first update.
#[derive(Component, Clone, Copy, Debug)]
pub struct Pulse {
    first: Vec3,
    second: Vec3,
    period: f32,
    /// The colour timer (s); `None` until the first update draws it.
    timer: Option<f32>,
    /// Swinging first → second.
    forward: bool,
    /// minRadius, maxRadius.
    radii: (f32, f32),
    expand: f32,
    growing: bool,
    pub radius: f32,
    pub color: Vec3,
}

impl Pulse {
    pub fn from_record(rec: &gd_formats::arz::Record) -> Self {
        let f = |k: &str| rec.f32(k).unwrap_or(0.0);
        let c = |p: &str| {
            Vec3::new(
                f(&format!("{p}.r")),
                f(&format!("{p}.g")),
                f(&format!("{p}.b")),
            )
        };
        Self {
            first: c("firstColor"),
            second: c("secondColor"),
            period: f("colorPeriod"),
            timer: None,
            forward: true,
            radii: (f("minRadius"), f("maxRadius")),
            expand: f("expandSpeed"),
            growing: true,
            radius: f("minRadius"),
            color: c("firstColor"),
        }
    }

    /// One `PulseLight::UpdateSelf` of `dt` ms; returns the radius.
    pub fn update(&mut self, dt: i32, rng: &mut CrtRand) -> f32 {
        let t = *self.timer.get_or_insert_with(|| rng.unit() * self.period);
        let s = dt.min(100) as f32 * 0.001;
        let d = s * self.expand;
        if self.growing {
            self.radius += d;
            if self.radius >= self.radii.1 {
                self.growing = false;
            }
        } else {
            self.radius -= d;
            if self.radius <= self.radii.0 {
                self.growing = true;
            }
        }
        let mut t = t + s;
        if t > self.period {
            t = 0.0;
            self.forward = !self.forward;
        }
        self.timer = Some(t);
        let k = t.clamp(0.0, 1.0);
        self.color = if self.forward {
            self.first.lerp(self.second, k)
        } else {
            self.second.lerp(self.first, k)
        };
        self.radius
    }
}

/// Steps every `FireLight` ([`Flicker`]) and `PulseLight` ([`Pulse`]) and hands its colour and radius to its
/// `PointLight` (intensity = 40 000 × range keeps the brightness at 1).
pub fn update_fire_lights(
    time: Res<Time>,
    mut rng: ResMut<LightRand>,
    mut carry: Local<f32>,
    mut lights: Query<(&mut Flicker, &mut PointLight), Without<Pulse>>,
    mut pulses: Query<(&mut Pulse, &mut PointLight), Without<Flicker>>,
) {
    // The engine's frame time is whole milliseconds; the remainder carries.
    let ms = time.delta_secs() * 1000.0 + *carry;
    let dt = ms.floor();
    *carry = ms - dt;
    let set = |l: &mut PointLight, c: Vec3, r: f32| {
        let c = Color::linear_rgb(c.x, c.y, c.z);
        if l.color != c {
            l.color = c;
        }
        if l.range != r {
            l.range = r;
            l.intensity = 40_000.0 * r;
        }
    };
    for (mut f, mut l) in &mut lights {
        let r = f.update(dt as i32, &mut rng.0);
        set(&mut l, f.color, r);
    }
    for (mut p, mut l) in &mut pulses {
        let r = p.update(dt as i32, &mut rng.0);
        set(&mut l, p.color, r);
    }
}

/// Spawns a point light. Spawned hidden (`Visibility`): Bevy neither clusters nor extracts it (the materials
/// read `render::env`'s buffer).
pub fn spawn_light(commands: &mut Commands, l: &LightCpu) -> Entity {
    let color = Color::linear_rgb(l.color.x, l.color.y, l.color.z);
    let mut e = commands.spawn((
        PointLight {
            color,
            intensity: 40_000.0 * l.radius,
            range: l.radius,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_translation(l.position),
        Visibility::Hidden,
    ));
    if let Some(f) = l.flicker {
        e.insert(f);
    }
    if let Some(p) = l.pulse {
        e.insert(p);
    }
    if l.shadow_priority > 0 {
        e.insert(crate::render::point_shadow::ShadowPriority(
            l.shadow_priority,
        ));
    }
    if let Some((day, night)) = l.day_night {
        e.insert(crate::render::env::DayNightColors { day, night });
    }
    e.id()
}

pub fn light_from_record(rec: &gd_formats::arz::Record, position: Vec3) -> Option<LightCpu> {
    // `FireLight::Load` / `Light::Load` / `DayNightLight::Load`: a
    // missing field reads 0.
    let c = |p: &str| {
        Vec3::new(
            rec.f32(&format!("{p}.r")).unwrap_or(0.0),
            rec.f32(&format!("{p}.g")).unwrap_or(0.0),
            rec.f32(&format!("{p}.b")).unwrap_or(0.0),
        )
    };
    let f = |k: &str| rec.f32(k).unwrap_or(0.0);
    let shadow_priority = rec.i32("shadowPriority").unwrap_or(0);
    match rec.class.as_str() {
        "FireLight" => {
            let flicker = Flicker::from_record(rec, c("color"));
            Some(LightCpu {
                position,
                color: c("color"),
                day_night: None,
                radius: 0.5 * (f("minRadius") + f("maxRadius")),
                flicker: Some(flicker),
                pulse: None,
                shadow_priority,
            })
        }
        "DayNightLight" => Some(LightCpu {
            position,
            color: c("dayColor"),
            day_night: Some((c("dayColor"), c("nightColor"))),
            radius: rec.f32("radius").unwrap_or(0.0),
            flicker: None,
            pulse: None,
            shadow_priority,
        }),
        // `PulseLight::Load` ([`Pulse`]).
        "PulseLight" => {
            let p = Pulse::from_record(rec);
            Some(LightCpu {
                position,
                color: p.color,
                day_night: None,
                radius: p.radius,
                flicker: None,
                pulse: Some(p),
                shadow_priority,
            })
        }
        // `Light::Load` (`radius`, `color`, `shadowPriority`; no update of its own) and `NightLight`
        // (`NightLight::Load` = `Light::Load` + `useInterpolatedEnvironment`; `NightLight::UpdateSelf` sets
        // `Light.intensity` to the AmbianceManager's night-light scale). No stock level is a grid level, so
        // both read the current lighting's scale, which `DayNightColors` (day black, night the colour) applies.
        "Light" | "NightLight" => Some(LightCpu {
            position,
            color: c("color"),
            day_night: (rec.class == "NightLight").then(|| (Vec3::ZERO, c("color"))),
            radius: f("radius"),
            flicker: None,
            pulse: None,
            shadow_priority,
        }),
        _ => None,
    }
}

/// The light classes [`light_from_record`] makes (placed lights and mesh light attachments). Not here:
/// `FlickerLight` (Engine.dll `FlickerLight::Load` / `UpdateSelf` / `AddToScene`): its one
/// record `fx/lights/flickerlight.dbr` is neither placed in a stock map nor attached to a mesh.
pub fn is_light_class(class: &str) -> bool {
    matches!(
        class,
        "FireLight" | "DayNightLight" | "PulseLight" | "NightLight" | "Light"
    )
}
