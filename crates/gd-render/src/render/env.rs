//! Lighting environment: the `AmbianceManager` outdoor slot around the
//! camera target ([`Ambiance`]: day/night cycles, environment colour
//! modifiers, bloom and adjustment sector data, the `lightingTransformation`
//! sun turn; Engine.dll `DayNightCycle::Read`, `GetLighting`,
//! `GetLightDirection`, `AmbianceManager::Update`) plus the level's point lights, packed into the storage
//! buffer every Grim Dawn material reads; bloom / adjustment go to the
//! camera's [`super::post::GammaOut`].
//!
//! PORT: the point lights are Bevy `PointLight`s spawned hidden (`Visibility`), so Bevy never clusters or
//! extracts them; the materials read only this buffer. It holds the [`MAX_LIGHTS`] nearest the camera
//! target among those whose sphere meets the game camera's frustum (`Light::AddToScene`'s test); the engine has
//! no cap.
use super::GdDefaults;
use bevy::prelude::*;
use bevy::render::storage::ShaderBuffer;
use gd_game_core::save::MapLayout;
use std::f32::consts::PI;

/// The system that writes the environment buffer and turns the sun (the sun's shadow map is built after it).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnvSystems;

/// At most this many point lights (in view, nearest to the camera target) are shaded.
const MAX_LIGHTS: usize = 64;

pub struct EnvPlugin;

impl Plugin for EnvPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TimeOfDay>();
        // `GD_TIME`: a fixed hour from the first frame on.
        #[cfg(feature = "dev")]
        if let Some(t) = app
            .world()
            .get_resource::<gd_game_core::hooks::Hooks>()
            .and_then(|h| h.render.time)
        {
            app.insert_resource(TimeOfDay(t));
        }
        app.add_systems(
            Update,
            (
                advance_time_of_day,
                player_light,
                player_light_radius,
                player_light_shadow,
            )
                .chain(),
        )
        // Before Bevy builds the sun's shadow cascades: a sun whose shadows `update_env` switches on
        // after them has no cascade for a view (`prepare_lights` panics).
        .add_systems(
            PostUpdate,
            update_env
                .after(TransformSystems::Propagate)
                .before(bevy::light::SimulationLightSystems::UpdateDirectionalLightCascades)
                .in_set(EnvSystems),
        );
    }
}

/// `DayNightLight` colours: `lerp(day, night, nightLightScale)`
/// (`DayNightLight::UpdateSelf`).
#[derive(Component, Clone, Copy)]
pub struct DayNightColors {
    pub day: Vec3,
    pub night: Vec3,
}

/// `AmbianceManager` night-light scale (`AmbianceManager::ComputeNightLightScale`):
/// `1 - clamp((lum(light) / 2 + lum(sky) / 2 - 0.3) * 10, 0, 1)` with
/// lum = 0.3 r + 0.59 g + 0.11 b. It is computed for the
/// environment slot of the camera region (underground or not,
/// `Region::IsUnderground` = the level has a grid) and `DayNightLight::UpdateSelf`
/// reads the outdoor slot's; no stock level is a grid level (all TerrainRT),
/// so that is always the current lighting and caves (dark "Cave" day/night
/// data) get the night colours, as here.
pub fn night_light_scale(light: Vec3, sky: Vec3) -> f32 {
    let lum = |c: Vec3| c.x * 0.3 + c.y * 0.59 + c.z * 0.11;
    1.0 - ((lum(light) * 0.5 + lum(sky) * 0.5 - 0.3) * 10.0).clamp(0.0, 1.0)
}

/// The player's own light (the player record's `playerLight`, read by
/// `Player::Load` (Game.dll): `records/creatures/pc/playerlight.dbr`,
/// a DayNightLight of day colour 0.18, night 0.75 / 0.7, radius 12, carried
/// by the player), attached to the player mesh's "Light" attach
/// point (`Player::InitialUpdate`, Game.dll; the point is 3.5 m up,
/// unparented, in `hero_00.msh` and `heroine_00_head.msh`).
///
/// Its radius follows the character's `characterLightRadius` total every
/// update (`Player::UpdateSelf`, formula in `gd_sim::stats::light_radius`). The same
/// update sets its shadow priority to 4 when `Options::GetQuality(6)`
/// (`lightingQuality`) is 3 ("very high"), else 0
/// (`Light::SetShadowPriority`): [`player_light_shadow`].
///
/// The character `lightRig` (playerlightrig.dbr) is not this light: it lights
/// only its own character.
#[derive(Component)]
struct PlayerLight {
    /// The record's `radius` (`Light::GetRadius`, kept by `Player::Load`).
    base: f32,
}

#[derive(Component)]
struct HasPlayerLight;

fn player_light(
    mut commands: Commands,
    players: Query<
        (Entity, Option<&gd_game_core::player::Loadout>),
        (With<gd_game_core::player::Player>, Without<HasPlayerLight>),
    >,
    data: Res<gd_game_core::data::GameData>,
) {
    for (p, loadout) in &players {
        commands.entity(p).insert(HasPlayerLight);
        let record = gd_game_core::player::player_record(loadout.is_some_and(|l| l.female));
        let Some(rec) = data
            .db
            .get(record)
            .and_then(|g| g.str("playerLight").map(str::to_owned))
            .and_then(|l| data.db.get(&l))
        else {
            continue;
        };
        let c = |k: &str| {
            Vec3::new(
                rec.f32(&format!("{k}.r")).unwrap_or(0.0),
                rec.f32(&format!("{k}.g")).unwrap_or(0.0),
                rec.f32(&format!("{k}.b")).unwrap_or(0.0),
            )
        };
        let (day, night) = (c("dayColor"), c("nightColor"));
        let radius = rec.f32("radius").unwrap_or(12.0);
        let light = commands
            .spawn((
                PointLight {
                    color: Color::linear_rgb(day.x, day.y, day.z),
                    intensity: 40_000.0 * radius,
                    range: radius,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_xyz(0.0, 3.5, 0.0),
                // Read from this module's buffer, not Bevy's clusters.
                Visibility::Hidden,
                DayNightColors { day, night },
                PlayerLight { base: radius },
                Name::new("player light"),
            ))
            .id();
        commands.entity(p).add_child(light);
    }
}

/// [`PlayerLight`]'s radius from its owner's `characterLightRadius` total.
fn player_light_radius(
    mut lights: Query<(&PlayerLight, &ChildOf, &mut PointLight)>,
    owners: Query<&gd_game_core::combat::Combatant>,
) {
    for (light, parent, mut point) in &mut lights {
        let Ok(c) = owners.get(parent.parent()) else {
            continue;
        };
        let radius = gd_sim::stats::light_radius(light.base, c.profile.sheet.light_radius).max(0.0);
        if point.range != radius {
            point.range = radius;
            point.intensity = 40_000.0 * radius;
        }
    }
}

/// `Player::UpdateSelf`: the player light's shadow
/// priority is 4 at `lightingQuality` 3, else none.
fn player_light_shadow(
    options: Res<gd_game_core::options::GameOptions>,
    lights: Query<(Entity, Option<&super::point_shadow::ShadowPriority>), With<PlayerLight>>,
    mut commands: Commands,
) {
    let want = matches!(
        options.get("lightingQuality"),
        Some(gd_formats::options::Value::Quality(q)) if super::shadow::quality_index(q) == 3
    );
    for (e, cur) in &lights {
        match (want, cur) {
            (true, None) => {
                commands
                    .entity(e)
                    .try_insert(super::point_shadow::ShadowPriority(4));
            }
            (false, Some(_)) => {
                // The map it last got goes with the priority: no stale `shadowBluriness` in the env buffer.
                commands.entity(e).try_remove::<(
                    super::point_shadow::ShadowPriority,
                    super::point_shadow::PointShadowMap,
                )>();
            }
            _ => {}
        }
    }
}

/// Hour of the day (0..24) shown: `AmbianceManager::GetTime`: with the game's
/// `dayNightCycle` option on, the running clock (gd-game's [`gd_game_core::day_clock::DayClock`]: from 9:00, a new
/// character from 2:00, at 30 times real time); off, the fixed 12.0 (`AmbianceManager` ctor). `GD_TIME`
/// overrides both.
#[derive(Resource)]
pub struct TimeOfDay(pub f32);

impl Default for TimeOfDay {
    fn default() -> Self {
        Self(12.0)
    }
}

/// `GetTime` each frame (the option is read every frame).
fn advance_time_of_day(
    mut tod: ResMut<TimeOfDay>,
    options: Res<gd_game_core::options::GameOptions>,
    clock: Res<gd_game_core::day_clock::DayClock>,
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
) {
    let cycle = options.flag("dayNightCycle");
    // Screenshot runs keep noon (`GD_TIME` picks another hour).
    #[cfg(feature = "dev")]
    let cycle = cycle && hooks.shot.path.is_none();
    let hour = if cycle { clock.0 } else { 12.0 };
    // `GD_TIME` wins every frame.
    #[cfg(feature = "dev")]
    let hour = hooks.render.time.unwrap_or(hour);
    if (hour - tod.0).abs() > f32::EPSILON {
        tod.0 = hour;
    }
}

/// One key of a day/night cycle (52 bytes in the engine; alpha dropped).
#[derive(Clone, Copy, Debug)]
pub struct LightSetup {
    pub time: f32,
    pub light: Vec3,
    pub sky: Vec3,
    pub ground: Vec3,
}

/// `DayNightCycle` (`sunset`, `sunrise`, `dayTime` (`GetDayTime`: the day begins), `nightTime` (`GetNightTime`: the
/// night begins), v2 files: sunrise / sunset, `DayNightCycle::Read`), `sunAzimuth`, `sunMinAngle`, `moonAzimuth`,
/// `moonMinAngle`).
#[derive(Clone, Debug)]
pub struct DayNightCycle {
    pub sunset: f32,
    pub sunrise: f32,
    pub day_time: f32,
    pub night_time: f32,
    pub sun_azimuth: f32,
    pub sun_min_angle: f32,
    pub moon_azimuth: f32,
    pub moon_min_angle: f32,
    pub setups: Vec<LightSetup>,
}

fn hm(h: f32, m: f32) -> f32 {
    h + m / 60.0
}

impl Default for DayNightCycle {
    /// `DayNightCycle::SetDefault` (colour tables `Engine::DayNightDefaultColors`).
    fn default() -> Self {
        let s = |t: f32, l: [f32; 3], sk: [f32; 3], g: [f32; 3]| LightSetup {
            time: t,
            light: l.into(),
            sky: sk.into(),
            ground: g.into(),
        };
        let night = ([0.165, 0.198, 0.3], [0.12, 0.26, 0.39], [0.11, 0.15, 0.18]);
        let day = ([0.94, 0.82, 0.65], [0.52, 0.56, 0.64], [0.45, 0.29, 0.18]);
        Self {
            sunrise: hm(2.0, 1.0),
            sunset: hm(22.0, 59.0),
            day_time: hm(2.0, 1.0),
            night_time: hm(22.0, 59.0),
            sun_azimuth: f32::from_bits(1066627591),
            sun_min_angle: f32::from_bits(1054828275),
            moon_azimuth: f32::from_bits(1066627591),
            moon_min_angle: f32::from_bits(1060288707),
            setups: vec![
                s(1.0, night.0, night.1, night.2),
                s(
                    hm(3.0, 10.0),
                    [1.0, 0.81, 0.59],
                    [0.26, 0.15, 0.15],
                    [0.65, 0.38, 0.11],
                ),
                s(9.0, day.0, day.1, day.2),
                s(17.0, day.0, day.1, day.2),
                s(
                    18.0,
                    [0.94, 0.78, 0.65],
                    [0.5, 0.45, 0.38],
                    [0.45, 0.29, 0.18],
                ),
                s(22.0, [1.0, 0.6, 0.35], [0.2, 0.32, 0.5], [0.5, 0.27, 0.21]),
                s(hm(23.0, 10.0), night.0, night.1, night.2),
            ],
        }
    }
}

impl DayNightCycle {
    /// The stored cycle (`gd_formats::map::sector::DayNightData`) over the
    /// defaults, as `DayNightCycle::Read` does: v2 files take the night times
    /// from sunset / sunrise, zero azimuths keep the default ones.
    // Filled field by field like the engine reader.
    #[allow(clippy::field_reassign_with_default)]
    pub fn from_data(d: &gd_formats::map::sector::DayNightData) -> Self {
        let mut c = Self::default();
        c.sunset = d.sunset;
        c.sunrise = d.sunrise;
        (c.day_time, c.night_time) = d.night.unwrap_or((d.sunrise, d.sunset));
        if d.sun_azimuth != 0.0 {
            c.sun_azimuth = d.sun_azimuth;
        }
        c.sun_min_angle = d.sun_min_angle;
        if d.moon_azimuth != 0.0 {
            c.moon_azimuth = d.moon_azimuth;
        }
        c.moon_min_angle = d.moon_min_angle;
        c.setups = d
            .keys
            .iter()
            .map(|(t, v)| LightSetup {
                time: *t,
                light: Vec3::new(v[0], v[1], v[2]),
                sky: Vec3::new(v[3], v[4], v[5]),
                ground: Vec3::new(v[6], v[7], v[8]),
            })
            .collect();
        c
    }

    /// (light, sky, ground) at `time`: Catmull-Rom over the cyclic keys
    /// (`DayNightCycle::InterpolateKeys`), light faded to black around sunrise/sunset.
    pub fn lighting(&self, time: f32) -> (Vec3, Vec3, Vec3) {
        let s = &self.setups;
        let n = s.len();
        if n == 0 {
            return (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO);
        }
        let mut i = s.iter().position(|k| time < k.time).unwrap_or(0);
        if i >= n {
            i = 0;
        }
        let p = |k: usize| &s[k % n];
        let (i2, i1, i3) = ((i + 2 * (n - 1)) % n, (i + n - 1) % n, (i + 1) % n);
        let mut span = s[i].time - s[i1].time;
        if span < 0.0 {
            span += 24.0;
        }
        let t = if span == 0.0 {
            0.0
        } else {
            let tt = if time < s[i1].time { time + 24.0 } else { time };
            (tt - s[i1].time) / span
        };
        let cr = |a: Vec3, b: Vec3, c: Vec3, d: Vec3| {
            0.5 * (2.0 * b
                + (c - a) * t
                + (2.0 * a - 5.0 * b + 4.0 * c - d) * t * t
                + (3.0 * b - a - 3.0 * c + d) * t * t * t)
        };
        let f = |g: fn(&LightSetup) -> Vec3| cr(g(p(i2)), g(p(i1)), g(p(i)), g(p(i3)));
        let mut light = f(|k| k.light);
        let sky = f(|k| k.sky);
        let ground = f(|k| k.ground);
        if self.sunrise != self.sunset {
            let fade = |e: f32| {
                let c = ((1.0 - (time - e).abs().min(1.0)) * PI).cos();
                (c + 1.0) * 0.5
            };
            let a = fade(self.sunrise);
            let b = fade(self.sunset);
            light *= (a * a) * (b * b);
        }
        let sat = |v: Vec3| v.clamp(Vec3::ZERO, Vec3::ONE);
        (sat(light), sat(sky), sat(ground))
    }

    /// Unit vector towards the sun/moon (world, Y-up).
    pub fn light_direction(&self, time: f32) -> Vec3 {
        let (mut elev, mut az) = (self.sun_min_angle, self.sun_azimuth);
        if self.sunrise != self.sunset {
            let (rise, set) = (self.sunrise, self.sunset);
            let set_w = if set < rise { set + 24.0 } else { set };
            let tw = if time < rise { time + 24.0 } else { time };
            let t = (tw - rise) / (set_w - rise);
            if (0.0..=1.0).contains(&t) {
                elev = (PI - 2.0 * self.sun_min_angle) * t + self.sun_min_angle;
            } else {
                let mut rise_w = rise + 24.0;
                if rise_w < set {
                    rise_w += 24.0;
                }
                let tw = if time < set { time + 24.0 } else { time };
                let t2 = (tw - set) / (rise_w - set);
                if (0.0..=1.0).contains(&t2) {
                    az = self.moon_azimuth;
                    elev = (PI - 2.0 * self.moon_min_angle) * t2 + self.moon_min_angle;
                }
            }
        }
        Vec3::new(elev.cos(), az.sin() * elev.sin(), az.cos() * elev.sin()).normalize_or_zero()
    }

    /// `DayNightCycle::GetIsDay`: `time` in the day window
    /// [`Self::day_time`] (`DayNightCycle.dayTime`) .. [`Self::night_time`] (`DayNightCycle.nightTime`), mod 24.
    pub fn is_day(&self, time: f32) -> bool {
        gd_sim::jukebox::is_day(time, self.day_time, self.night_time)
    }

    /// `AmbianceManager::ComputeShadowIntensity`: 0 → 1 over 0.1 h after sunrise, back before sunset. It reaches
    /// no renderer: only the "Shadow Intensity" statistics line reads `AmbianceManager.shadowIntensity`.
    pub fn shadow_intensity(&self, time: f32) -> f32 {
        let (v6, v8) = (self.sunset, self.sunrise);
        let (v9, v10) = (v6 + 0.1, v8 - 0.1);
        let v11 = if v9 < v6 { v9 + 24.0 } else { v9 };
        let v7 = if time < v6 { time + 24.0 } else { time };
        let v12 = (v7 - v6) / (v11 - v6);
        if (0.0..=1.0).contains(&v12) {
            return v12;
        }
        let v8w = if v8 < v10 { v8 + 24.0 } else { v8 };
        let v13 = if time < v10 { time + 24.0 } else { time };
        let v14 = (v13 - v10) / (v8w - v10);
        if (0.0..=1.0).contains(&v14) {
            return 1.0 - v14;
        }
        1.0
    }
}

/// CPU copy of the environment buffer.
#[derive(Clone, Debug, Default)]
pub struct EnvGpu {
    pub sun_dir: Vec3,
    pub sun_on: bool,
    pub sun_color: Vec3,
    pub shadow_intensity: f32,
    pub sky: Vec3,
    pub ground: Vec3,
    /// The point lights, nearest the camera target first ([`MAX_LIGHTS`]).
    pub lights: Vec<GpuLight>,
    /// `hdrLighting`: the lights add into the float target, unclamped (the shaders' `env[4].x` limit).
    pub hdr: bool,
    /// The point lights' specular term unless `lightingQuality` is low: `Light::AddToScene`
    /// sets the scene light's `GraphicsLight.specular` (`RenderDeferredPointLight`: `pointSpecular`, else
    /// `point`) to !`GraphicsEngine::UseLowQualityLighting` (`lightingQuality` low), and
    /// only for a light not in simple mode ([`SimpleLight`]); [`GpuLight::specular`] is the result per light.
    /// The term itself (`pointSpecularPS`) is the sun's: `sat(V·R)^power · specular`, power =
    /// 2^(10.5 · material.y), specular = the specular target's rgb, same G-buffer, same falloff and shadow.
    pub point_specular: bool,
}

/// One point light of the environment buffer.
#[derive(Clone, Debug, Default)]
pub struct GpuLight {
    pub position: Vec3,
    pub radius: f32,
    pub color: Vec3,
    /// The point shadow's `shadowBluriness`, 0 = no map.
    pub blur: f32,
    /// `pointSpecular` rather than `point` (the scene light's `GraphicsLight.specular`, [`EnvGpu::point_specular`]).
    pub specular: bool,
}

/// A light in simple mode (the engine's `Light.simpleMode`): `Light::AddToScene` clears the
/// scene light's specular flag (`GraphicsLight.specular`) and sets it, and the shadow priority, only when
/// `simpleMode` is 0, so such a light is drawn with `point` (no specular) and never shadowed. Only effect lights set
/// it: `Emitter::UpdateLight` copies the emitter's bools[11] (EmitterData +115,
/// `gd_formats::pfx::Emitter::light_simple`) every update; `Light::SetSimpleMode` has no
/// other caller. Effect lights never get a point shadow in the port, so only the specular is withheld here.
#[derive(Component)]
pub struct SimpleLight;

/// `GraphicsEngine::GetHdrLightingFactor` (`GraphicsEngine.hdrLightingFactor`; 1.15 from the constructor,
/// Engine.dll `GraphicsEngine` ctor; `SetHdrLightingFactor` has no caller in Engine.dll). With
/// `hdrLighting` the deferred renderer scales the sun's and the point lights' colour by it
/// (`RenderDeferredDirectionalLight`, `RenderDeferredPointLight`:
/// `lightColor *= factor` when `GraphicsDeferredRenderer.hdrLighting` is set); the ambient pass is not scaled.
pub const HDR_LIGHTING_FACTOR: f32 = 1.15;

/// The limit the shaders clamp each light term to: the UNORM back buffer's 1, or none in
/// the `hdrLighting` float target (`GraphicsDeferredRenderer::CreateRenderTargets`:
/// `GraphicsDeferredRenderer.hdrTarget` is made in format 13 = DXGI R16G16B16A16_FLOAT, Direct3D11.dll
/// `RenderDevice::GetDxgiFormat`; the G-buffer and back buffer are format 2 = B8G8R8A8_UNORM).
const HDR_LIMIT: f32 = 65504.0;

impl EnvGpu {
    pub fn to_vec4s(&self) -> Vec<[f32; 4]> {
        let mut v = Vec::new();
        self.write_vec4s(&mut v);
        v
    }

    /// [`Self::to_vec4s`] into `v`.
    fn write_vec4s(&self, v: &mut Vec<[f32; 4]>) {
        v.clear();
        v.extend([
            self.sun_dir
                .extend(if self.sun_on { 1.0 } else { 0.0 })
                .to_array(),
            self.sun_color.extend(self.shadow_intensity).to_array(),
            self.sky.extend(0.0).to_array(),
            self.ground.extend(self.lights.len() as f32).to_array(),
            [if self.hdr { HDR_LIMIT } else { 1.0 }, 0.0, 0.0, 0.0],
        ]);
        for l in &self.lights {
            v.push(l.position.extend(l.radius).to_array());
            v.push(l.color.extend(0.0).to_array());
        }
        // After the lights, one entry each: x = the point shadow's `shadowBluriness`, y = 1 for
        // `pointSpecular` (the light's specular term).
        for l in &self.lights {
            let specular = self.point_specular && l.specular;
            v.push([l.blur, if specular { 1.0 } else { 0.0 }, 0.0, 0.0]);
        }
        // A fixed size: a rewrite then updates the GPU buffer in place.
        v.resize(5 + 3 * MAX_LIGHTS, [0.0; 4]);
    }
}

/// One layer-0 entry (`FogSectorData`, [`gd_formats::map::sector::EnvironmentFields`]).
#[derive(Clone, Copy, Debug, Default)]
struct EnvModifier {
    light: Vec3,
    sky: Vec3,
    ground: Vec3,
    fog_color: Vec3,
    /// +152 / +156.
    depth_density: f32,
    height_density: f32,
    /// (start, end) metres: +160 / +164 depth, +168 / +172 height.
    depth: Vec2,
    height: Vec2,
}

/// The map's lighting sector data, keyed by GUID.
#[derive(Default)]
struct Tables {
    daynight: Vec<([i32; 4], String, DayNightCycle)>,
    /// Layer 0 (`FogSectorData`).
    modifiers: Vec<([i32; 4], EnvModifier)>,
    bloom: Vec<([i32; 4], Vec3)>,
    /// (GUID, brightness, contrast, saturation, tint).
    adjustment: Vec<([i32; 4], Vec3, Vec3)>,
}

impl Tables {
    fn read(set: &gd_formats::map::sector::SectorDataSet) -> anyhow::Result<Self> {
        use gd_formats::map::sector::{EnvironmentFields, SectorEntry};
        let tail = set.tail_layers()?;
        let rgb = |c: [f32; 4]| Vec3::new(c[0], c[1], c[2]);
        let modifiers = set
            .layers
            .first()
            .map(|(_, v)| {
                v.iter()
                    .filter_map(|e| match e {
                        SectorEntry::Fog(b, d) => EnvironmentFields::from_dwords(d).map(|f| {
                            let m = EnvModifier {
                                light: rgb(f.light_mod),
                                sky: rgb(f.sky_mod),
                                ground: rgb(f.ground_mod),
                                fog_color: rgb(f.fog_color),
                                depth_density: f.fog_floats[0],
                                height_density: f.fog_floats[1],
                                depth: Vec2::new(f.fog_ints[0] as f32, f.fog_ints[1] as f32),
                                height: Vec2::new(f.fog_ints[2] as f32, f.fog_ints[3] as f32),
                            };
                            (b.ints, m)
                        }),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            daynight: tail
                .daynight
                .iter()
                .map(|(b, d)| (b.ints, b.name.clone(), DayNightCycle::from_data(d)))
                .collect(),
            modifiers,
            bloom: tail
                .later
                .bloom
                .iter()
                .map(|b| (b.base.ints, Vec3::from(b.params)))
                .collect(),
            adjustment: tail
                .later
                .adjustment
                .iter()
                .map(|a| {
                    (
                        a.base.ints,
                        Vec3::new(a.brightness, a.contrast, a.saturation),
                        Vec3::from(a.tint),
                    )
                })
                .collect(),
        })
    }
}

/// The outdoor environment slot of `AmbianceManager` (Engine.dll;
/// `AmbianceManager.outdoorSlot`, stock levels are never underground, see
/// [`night_light_scale`]): each part blends the sector-data entries around
/// the target ([`gd_game_core::levels::Levels::sector_weights`]) and fills the
/// remaining weight with its default.
/// - day/night (`AmbianceManager::UpdateDayNightSlot`, layer 4, ±20 m): Σ w·`GetLighting` and
///   Σ w·`GetLightDirection`, rest from the default cycle (`AmbianceManager.defaultCycle`); the
///   direction is normalised, then turned by the lighting transform.
/// - modifiers (`AmbianceManager::UpdateModifierSlot`, layer 0, ±20 m): Σ w·light / sky / ground
///   modifier + rest × 1; `GetLightColor` / `GetSkyAmbientColor` /
///   `GetGroundAmbientColor` multiply them into the colours (the weather
///   darkness is subtracted from sky and ground). The same pass fills
///   the fog: colour Σ w·colour and densities Σ w·density (the rest adds
///   nothing; depth density clamped to [0, 1], `AmbianceManager.environmentEffects`
///   on), the depth and height (start, end) distances Σ w·d / Σ w; the weather
///   increments (`AmbianceManager.weather*`, 0 without weather) are added. With depth-fog clamping
///   (`AmbianceManager.depthFogClamping`, on from the constructor) the depth start becomes
///   max(clamp, start) and the end keeps its distance to the start; the clamp
///   is [`depth_fog_clamp`].
/// - bloom (`AmbianceManager_UpdateBloomSlot`, layer 5, ±5 m): Σ w·params, clamped to
///   [0,1] × [0,4] × [0,1] (+ weather increments, 0).
/// - adjustment (`AmbianceManager::UpdateAdjustmentSlot`, layer 8, ±5 m): Σ w·(brightness,
///   contrast, saturation, tint) + rest × (0, 0, 0, 1, 1, 1), clamped to
///   [-1,1]³ × [0,1]³.
///
/// The slots are recomputed from the sector data on every update, without
/// easing: `AmbianceManager::UpdateOutdoorFactor` (from `AmbianceManager::Update`) only eases the
/// outdoor / underground factor `AmbianceManager.outdoorFactor` (target 1 outdoors, 0.1 × dt per
/// step), which stays 1 on stock levels (none is underground).
/// Also the resource of the current frame's values.
#[derive(Clone, Debug, PartialEq, Resource)]
pub struct Ambiance {
    pub light: Vec3,
    pub sky: Vec3,
    pub ground: Vec3,
    pub light_dir: Vec3,
    pub shadow_intensity: f32,
    /// x threshold scale, y blur gain, z saturation (`HotBlurFrameBuffer`).
    pub bloom: Vec3,
    /// brightness, contrast, saturation.
    pub adjust: Vec3,
    pub tint: Vec3,
    /// Fog colour (`fogColor`).
    pub fog_color: Vec3,
    /// Depth fog (`depthFog`): start, end (view depth, m), density.
    pub depth_fog: Vec3,
    /// Height fog (`heightFog`): start, end (world height, m), density.
    pub height_fog: Vec3,
    /// `AmbianceManager::GetIsDay` (`AmbianceManager.isDay`): `DayNightCycle::GetIsDay(t)` of the area's cycle,
    /// read by the weather's Day / Night climate filter and the ambient sound's day / night set.
    /// `AmbianceManager::Update`: the area's cycle is the layer-4 entry of the cell under the target in
    /// the manager's region (the same id array that picks the ambient layer-2 session), else the default cycle
    /// (`AmbianceManager.defaultCycle`); `GetIsDay` tests the day window [`DayNightCycle::day_time`] ..
    /// [`DayNightCycle::night_time`]. The same cycle feeds the shadow
    /// intensity (`AmbianceManager::ComputeShadowIntensity`).
    pub is_day: bool,
    /// Name of the heaviest day/night entry (log only).
    pub daynight_name: String,
}

impl Ambiance {
    /// `weights(layer, radius)` = [`gd_game_core::levels::Levels::sector_weights`];
    /// `transform_deg` = `gameengine.dbr` `lightingTransformation`;
    /// `fog_clamp` = [`depth_fog_clamp`]; `cell` = the layer-4 (day / night) id of the cell under the target
    /// ([`Self::is_day`]); `weather` = the weather increments
    /// (`AmbianceManager::SetWeatherAdjustments`: `weatherDarkness`
    /// ×3, `weatherDensity`, `weatherDistance`, `weatherColor`): the darkness
    /// comes off the sky and ground ambient (`GetSkyAmbientColor` /
    /// `GetGroundAmbientColor`: c − min(d, c); the sun and shadows keep theirs),
    /// `AmbianceManager::UpdateModifierSlot` adds the density (then clamps to 0..1), the distance to
    /// the depth fog's start before the depth-fog clamp (the end stays) and the
    /// colour (clamped 0..1).
    fn compute(
        tables: &Tables,
        weights: impl Fn(usize, f32) -> Vec<([i32; 4], f32)>,
        time: f32,
        transform_deg: f32,
        fog_clamp: f32,
        cell: Option<[i32; 4]>,
        weather: &gd_sim::weather::Adjust,
    ) -> Self {
        let default_cycle = DayNightCycle::default();
        let (mut light, mut sky, mut ground, mut dir) =
            (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO);
        let mut rest = 1.0;
        let mut name = ("<default>".to_owned(), 0.0f32);
        // The target cell's cycle, else the default (`Self::is_day`).
        let area_cycle = cell
            .and_then(|g| tables.daynight.iter().find(|(id, ..)| *id == g))
            .map_or(&default_cycle, |(.., c)| c);
        for (g, w) in weights(4, 20.0) {
            if let Some((_, n, c)) = tables.daynight.iter().find(|(id, _, _)| *id == g) {
                let (l, s, gr) = c.lighting(time);
                light += w * l;
                sky += w * s;
                ground += w * gr;
                dir += w * c.light_direction(time);
                rest -= w;
                if w > name.1 {
                    name = (n.clone(), w);
                }
            }
        }
        let (l, s, gr) = default_cycle.lighting(time);
        light += rest * l;
        sky += rest * s;
        ground += rest * gr;
        dir += rest * default_cycle.light_direction(time);
        let dir = dir.normalize_or_zero();
        // SetLightingTransformAngle: rotation about +Y by the angle (degrees,
        // `Math::AxisAngleMatrix`), applied as v · M.
        let (sn, cs) = transform_deg.to_radians().sin_cos();
        let light_dir = Vec3::new(dir.x * cs + dir.z * sn, dir.y, -dir.x * sn + dir.z * cs);

        let (mut lm, mut sm, mut gm) = (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO);
        let (mut fog_color, mut depth_density, mut height_density) = (Vec3::ZERO, 0.0, 0.0);
        let (mut depth, mut height) = (Vec2::ZERO, Vec2::ZERO);
        let mut rest = 1.0;
        for (g, w) in weights(0, 20.0) {
            if let Some((_, m)) = tables.modifiers.iter().find(|(id, ..)| *id == g) {
                lm += w * m.light;
                sm += w * m.sky;
                gm += w * m.ground;
                fog_color += w * m.fog_color;
                depth_density += w * m.depth_density;
                height_density += w * m.height_density;
                depth += w * m.depth;
                height += w * m.height;
                rest -= w;
            }
        }
        if rest < 1.0 {
            // The distances are weighted means; colour and densities fade
            // out with the rest weight.
            depth /= 1.0 - rest;
            height /= 1.0 - rest;
        }
        lm += Vec3::splat(rest);
        sm += Vec3::splat(rest);
        gm += Vec3::splat(rest);
        // SetDepthFogClamp: the start moves out to at least the clamp
        // distance, keeping the ramp length (`AmbianceManager.depthFogClamping`, on).
        let depth = Vec2::new(depth.x + weather.fog_distance, depth.y);
        let start = depth.x.max(fog_clamp);
        let depth_fog = Vec3::new(
            start,
            depth.y - depth.x + start,
            (depth_density + weather.fog_density).clamp(0.0, 1.0),
        );

        let mut bloom = Vec3::ZERO;
        for (g, w) in weights(5, 5.0) {
            if let Some((_, b)) = tables.bloom.iter().find(|(id, _)| *id == g) {
                bloom += w * *b;
            }
        }
        let bloom = bloom.clamp(Vec3::ZERO, Vec3::new(1.0, 4.0, 1.0));

        let (mut adjust, mut tint) = (Vec3::ZERO, Vec3::ZERO);
        let mut rest = 1.0;
        for (g, w) in weights(8, 5.0) {
            if let Some((_, a, t)) = tables.adjustment.iter().find(|(id, ..)| *id == g) {
                adjust += w * *a;
                tint += w * *t;
                rest -= w;
            }
        }
        tint += Vec3::splat(rest);

        Self {
            light: light.clamp(Vec3::ZERO, Vec3::ONE) * lm,
            sky: (sky.clamp(Vec3::ZERO, Vec3::ONE) * sm - Vec3::splat(weather.darkness))
                .max(Vec3::ZERO),
            ground: (ground.clamp(Vec3::ZERO, Vec3::ONE) * gm - Vec3::splat(weather.darkness))
                .max(Vec3::ZERO),
            light_dir,
            shadow_intensity: area_cycle.shadow_intensity(time),
            is_day: area_cycle.is_day(time),
            bloom,
            adjust: adjust.clamp(Vec3::splat(-1.0), Vec3::ONE),
            tint: tint.clamp(Vec3::ZERO, Vec3::ONE),
            fog_color: (fog_color + Vec3::from(weather.fog_color)).clamp(Vec3::ZERO, Vec3::ONE),
            depth_fog,
            height_fog: height.extend(height_density),
            daynight_name: name.0,
        }
    }
}

/// `AmbianceManager::SetDepthFogClamp` from the controlling `GameCamera`
/// (Game.dll `GameCamera::ResetToDefaults` / `GameCamera::UpdateDepthFogClamp`, set on
/// zoom): d + (d − min) / (max − min) × `CameraDepthFogClamp` with d the
/// camera distance and min / max `CameraDistanceMin` / `Max`
/// (`gameengine.dbr`: −1, 20, 48: 35.4 m at the default 36 m), so the depth
/// fog never starts in front of the player.
pub fn depth_fog_clamp(distance: f32, min: f32, max: f32, k: f32) -> f32 {
    if max == min {
        distance
    } else {
        distance + (distance - min) / (max - min) * k
    }
}

#[derive(Default)]
struct Cache {
    tables: Option<Tables>,
    /// The tables' map: the world's (false) or the main menu scene's (true).
    menu: bool,
    transform_deg: f32,
    fog_clamp_k: f32,
    /// The last logged day/night entry and its bloom / adjust / tint at 2 decimals.
    current: Option<(String, [i32; 9])>,
    /// The frame's point lights with their squared distance to the target.
    lights: Vec<(f32, GpuLight)>,
    gpu_lights: Vec<GpuLight>,
    vec4s: Vec<[f32; 4]>,
}

#[allow(clippy::too_many_arguments)]
fn update_env(
    world: Option<Res<MapLayout>>,
    levels: Res<gd_game_core::levels::Levels>,
    defaults: Option<Res<GdDefaults>>,
    data: Res<gd_game_core::data::GameData>,
    time: Res<TimeOfDay>,
    cams: Query<(
        &crate::camera::GameCamera,
        Option<&bevy::camera::primitives::Frustum>,
    )>,
    lights: Query<
        (
            &GlobalTransform,
            &PointLight,
            Option<&DayNightColors>,
            Option<&super::point_shadow::PointShadowMap>,
            Has<SimpleLight>,
        ),
        Without<super::light_rig::RigLight>,
    >,
    mut suns: Query<(&mut Transform, &mut DirectionalLight)>,
    mut post: Query<&mut super::post::GammaOut>,
    options: Option<Res<gd_game_core::options::GameOptions>>,
    weather: Option<Res<super::weather::WeatherState>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut cache: Local<Cache>,
    menu: Option<Res<super::backdrop::MenuBackdrop>>,
    (mut commands, current, story): (
        Commands,
        Option<Res<Ambiance>>,
        Option<NonSend<gd_game_core::story::StoryRuntime>>,
    ),
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
) {
    let Some(defaults) = defaults else { return };
    let cam = cams.iter().next();
    let target = cam.map_or(Vec3::ZERO, |c| c.0.target);
    let frustum = cam.and_then(|c| c.1);
    // The main menu's scene lights itself from its own map.
    let sector_data = match &menu {
        Some(m) => m.map.as_ref().map(|m| &m.sector_data),
        None => world.as_ref().map(|w| &w.0.sector_data),
    };
    // The tables are the map's: rebuilt when the menu comes or goes, or the world's map is replaced (a Crucible
    // or custom map installed over the campaign world).
    if cache.menu != menu.is_some()
        || (menu.is_none() && world.as_ref().is_some_and(|w| w.is_changed()))
    {
        cache.menu = menu.is_some();
        cache.tables = None;
    }
    if cache.tables.is_none() {
        cache.tables = Some(match sector_data {
            Some(sd) => Tables::read(sd).unwrap_or_else(|e| {
                warn!("lighting sector data: {e:#}");
                Tables::default()
            }),
            None => Tables::default(),
        });
        // GameEngine::LoadFromDatabase → SetLightingTransformAngle.
        let eng = data.db.get("records/game/gameengine.dbr");
        cache.transform_deg = eng
            .as_ref()
            .and_then(|r| r.f32("lightingTransformation"))
            .unwrap_or(0.0);
        cache.fog_clamp_k = eng
            .as_ref()
            .and_then(|r| r.f32("CameraDepthFogClamp"))
            .unwrap_or(0.0);
    }
    let t = menu.as_ref().map_or(time.0, |m| m.time);
    // The menu: `SetDepthFogClamp(0)`.
    let fog_clamp = match (&menu, cam) {
        (None, Some((c, _))) => depth_fog_clamp(
            c.distance,
            c.distance_min,
            c.distance_max,
            cache.fog_clamp_k,
        ),
        _ => 0.0,
    };
    let sector_levels = match &menu {
        Some(m) => Some(&m.levels),
        None => Some(&*levels),
    };
    // The day / night cell (`Ambiance::is_day`): in the game from the player's region's level (the manager's
    // region; the Shattered Realm's regions overlap main-world ground).
    let cell = match (
        &menu,
        gd_game_core::riftgates::player_region(story.as_deref()),
    ) {
        (Some(m), _) => m.levels.sector_guid(target, 4),
        (None, Some(r)) => levels.sector_guid_in(r, target, 4),
        (None, None) => levels.sector_guid(target, 4),
    };
    let amb = Ambiance::compute(
        cache.tables.as_ref().unwrap(),
        |layer, r| {
            sector_levels
                .map(|l| l.sector_weights(target, layer, r))
                .unwrap_or_default()
        },
        t,
        cache.transform_deg,
        fog_clamp,
        cell,
        &weather.map(|w| w.adjust()).unwrap_or_default(),
    );
    if sector_data.is_none() {
        cache.tables = None;
    }
    if current.as_deref() != Some(&amb) {
        commands.insert_resource(amb.clone());
    }
    let (light, sky, ground) = (amb.light, amb.sky, amb.ground);
    let night = night_light_scale(light, sky);
    let r2 = |v: Vec3| (v * 100.0).round().as_ivec3().to_array();
    let [b, a, tn] = [r2(amb.bloom), r2(amb.adjust), r2(amb.tint)];
    let key = [b[0], b[1], b[2], a[0], a[1], a[2], tn[0], tn[1], tn[2]];
    if cache
        .current
        .as_ref()
        .is_none_or(|(name, k)| *name != amb.daynight_name || *k != key)
    {
        info!(
            "environment: day/night {}: light {light} sky {sky} ground {ground}, night-light scale {night:.2}, bloom {:.2}, adjustment {:.2} tint {:.2}",
            amb.daynight_name, amb.bloom, amb.adjust, amb.tint
        );
        cache.current = Some((amb.daynight_name.clone(), key));
    }
    let cam_distance = match &menu {
        Some(b) => b.cam_distance(),
        None => cam.map_or(0.0, |c| c.0.distance),
    };
    if !post.is_empty() {
        let video = options
            .as_ref()
            .map(|o| super::post::VideoOptions::from_options(o))
            .unwrap_or_default();
        // `GD_FOG=1` forces the fog on, `GD_DOF=0` the depth of field off.
        #[cfg(feature = "dev")]
        let video = super::post::VideoOptions {
            fog: video.fog || hooks.render.fog,
            depth_of_field: video.depth_of_field && !hooks.render.no_dof,
            ..video
        };
        let v = super::post::GammaOut::from_ambiance(&amb, cam_distance, video)
            .with_post_effects(options.as_ref().is_none_or(|o| o.flag("advancedEffects")))
            .with_colorblind(options.as_ref().map_or(0, |o| o.int("colorblind")));
        let v = match options.as_ref() {
            Some(o) => v.with_gamma_ramp(
                o.int("screenMode") == 0,
                o.float("brightness"),
                o.float("contrast"),
                o.float("gamma"),
            ),
            None => v,
        };
        for mut p in &mut post {
            p.set_if_neq(v);
        }
    }
    let dir = amb.light_dir;
    let hdr = options.as_ref().is_some_and(|o| o.flag("hdrLighting"));
    let light_gain = if hdr { HDR_LIGHTING_FACTOR } else { 1.0 };
    let mut env = EnvGpu {
        sun_dir: dir,
        // GraphicsMTRenderer::AddSceneLight skips a near-black sun.
        sun_on: light.max_element() > 0.03 && dir != Vec3::ZERO,
        sun_color: light * light_gain,
        shadow_intensity: amb.shadow_intensity,
        sky,
        ground,
        lights: std::mem::take(&mut cache.gpu_lights),
        hdr,
        point_specular: options
            .as_ref()
            .is_none_or(|o| super::light_rig::lighting_quality(o) != 0),
    };
    env.lights.clear();
    // Kept in a stable (position) order. The shaders' falloff max(1 - (d / radius)², 0) · sat(N·L) ·
    // colour · albedo is deferred.ssh `pointPS` (radius = `lightMaxDistance`,
    // `RenderDeferredPointLight`), times the light's point shadow; every light in the scene is
    // drawn (`SetupLights` has no cap).
    // `lightColor` is the scene light's `GraphicsLight.color`, the record colour × intensity
    // (× the HDR factor), not the pow(2.2) copy `GraphicsLight.linearColor`
    // (`Light::AddToScene`, `RenderDeferredPointLight`).
    let ls = &mut cache.lights;
    ls.clear();
    // `Light::AddToScene`'s frustum test first: a light whose sphere misses the
    // view lights nothing on screen, and the shaders loop over every listed light.
    ls.extend(
        lights
            .iter()
            .filter(|(gt, l, ..)| {
                frustum.is_none_or(|f| super::point_shadow::in_view(f, gt.translation(), l.range))
            })
            .map(|(gt, l, dn, shadow, simple)| {
                let p = gt.translation();
                let c = match dn {
                    Some(dn) => {
                        let v = dn.day.lerp(dn.night, night);
                        bevy::color::LinearRgba::rgb(v.x, v.y, v.z)
                    }
                    None => l.color.to_linear(),
                };
                // Brightness: `intensity` relative to the level lights' 40 000 ×
                // range (1 for those; effect lights fade with their emitter and
                // are 0 once it is done or hidden).
                let k = if l.range > 0.0 {
                    (l.intensity / (40_000.0 * l.range)).clamp(0.0, 4.0)
                } else {
                    0.0
                };
                (
                    p.distance_squared(target),
                    GpuLight {
                        position: p,
                        radius: l.range,
                        color: Vec3::new(c.red, c.green, c.blue) * k * light_gain,
                        blur: shadow.map_or(0.0, |s| s.bluriness()),
                        specular: !simple,
                    },
                )
            }),
    );
    // The nearest MAX_LIGHTS without sorting all of them, then in position order.
    if ls.len() > MAX_LIGHTS {
        ls.select_nth_unstable_by(MAX_LIGHTS, |a, b| a.0.total_cmp(&b.0));
        ls.truncate(MAX_LIGHTS);
    }
    ls.sort_by(|a, b| {
        let (a, b) = (a.1.position, b.1.position);
        a.x.total_cmp(&b.x).then(a.z.total_cmp(&b.z))
    });
    env.lights.extend(ls.drain(..).map(|(_, l)| l));
    env.write_vec4s(&mut cache.vec4s);
    let same = buffers
        .get(&defaults.env)
        .and_then(|b| {
            b.cast_slice::<[f32; 4]>()
                .map(|old| old == cache.vec4s.as_slice())
        })
        .unwrap_or(false);
    if !same && let Some(mut buf) = buffers.get_mut(&defaults.env) {
        buf.clear();
        buf.extend(cache.vec4s.iter().copied());
    }
    cache.gpu_lights = env.lights;
    for (mut tf, mut sun) in &mut suns {
        if sun.shadow_maps_enabled != env.sun_on {
            sun.shadow_maps_enabled = env.sun_on;
        }
        if dir != Vec3::ZERO {
            let want = Transform::from_translation(target)
                .looking_to(-dir, if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y });
            tf.set_if_neq(want);
        }
    }
}
