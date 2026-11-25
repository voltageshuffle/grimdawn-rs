//! Animated scenery: a `Decoration` / `WeatherDecoration` record's `animation`
//! (windmills, swinging lanterns, carts, ...).
//!
//! Game.dll:
//! - `Decoration::Load`: `animation` (the base), `playOnce`, and
//!   `animation1`..`animation5` each kept with its `animationNChance` when that
//!   is > 0.
//! - `Decoration::InitialUpdate` plays the base animation once
//!   (speed 1); `Decoration::AnimationCallback`, on its "End",
//!   unless `playOnce`, plays `GetRandomAnimation`: r = rand() /
//!   32767 · 100, walk the list subtracting chances until r ≤ a chance (that
//!   one), else the base. So a decoration loops through its animations; with
//!   `playOnce` it stops on the last frame.
//! - `WeatherDecoration` (`WeatherDecoration::vftable`; `WeatherDecoration::Load`,
//!   `WeatherDecoration::InitialUpdate`, `WeatherDecoration::UpdateSelf`): plays one
//!   `GetRandomAnimation` looped; speed 1, or with `animateAtWindSpeed`
//!   `min + (max − min) · normalised wind` (`min/maxAnimateWindMultiplier`).
//!
//! - `Floater` (a `Decoration` that bobs on water, 9 stock records: barrels,
//!   boats, a coffin, a corpse; class "Floater" `Floater::RegisterClass`, vtable
//!   `Floater::vftable`; [`Floater`]): `Floater::Load` reads `bobTime` (s → ms),
//!   `bobMax`, `alwaysBob`, then `Decoration::Load`, and draws its first target
//!   t = `FGenerate(bobMax / 2, bobMax)`, from −t, timer 0.
//!   `Floater::OnAddToLevel` keeps its placed coords. `Floater::UpdateSelf` (in a
//!   region, no parent): timer += dt; past `bobTime` the target becomes the
//!   start and a new one is drawn on the other side (`FGenerate(bobMax / 2,
//!   bobMax)` after a target ≤ 0, else `FGenerate(−bobMax / 2, −bobMax)`), the
//!   timer restarts and `World::CreatePointDisturbance` runs at its coords with
//!   radius |the half extents of its region box| (`Entity.regionBoundingBox`,
//!   `SetRegionBoundingBox`), strength 1, falloff, splash 0; then its coords are
//!   the placed ones raised by from + (to − from) · (sin(timer / bobTime · π −
//!   π/2) + 1) / 2.
//!
//! PORT: `animateAtWindSpeed` (no stock record sets it) and the
//! `windDirectionBone` turn toward the wind (`WeatherDecoration::TweakPose`) /
//! its weather particle funnel are not ported; `rand()` and `FGenerate` are
//! `fastrand`; `alwaysBob` is read but its consumer is not traced; a floater's
//! region box is its mesh box through its placement (world axes).
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_render::model::{Animator, ModelCache};

pub struct DecorationAnimPlugin;

impl Plugin for DecorationAnimPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            ((start_animations, next_animation).chain(), bob_floaters),
        );
    }
}

/// On a scenery root: the record's animations (paths), played by this module.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct DecorationAnims {
    pub base: String,
    /// `animation1..5` with their chance in percent (> 0).
    pub choices: Vec<(String, f32)>,
    pub play_once: bool,
    /// `WeatherDecoration`: one random pick, looped.
    pub weather: bool,
}

impl DecorationAnims {
    pub fn from_record(rec: &Record) -> Option<Self> {
        let weather = match rec.class.as_str() {
            "Decoration" | "Floater" => false,
            "WeatherDecoration" => true,
            _ => return None,
        };
        let base = rec.str("animation").filter(|s| !s.is_empty())?.to_owned();
        let choices = (1..=5)
            .filter_map(|i| {
                let chance = rec.f32(&format!("animation{i}Chance")).unwrap_or(0.0);
                let path = rec
                    .str(&format!("animation{i}"))
                    .filter(|s| !s.is_empty())?;
                (chance > 0.0).then(|| (path.to_owned(), chance))
            })
            .collect();
        Some(Self {
            base,
            choices,
            play_once: rec.bool("playOnce"),
            weather,
        })
    }

    /// `GetRandomAnimation` for a roll `r` in [0, 100].
    pub fn pick(&self, r: f32) -> &str {
        let mut sum = 0.0;
        for (path, chance) in &self.choices {
            if r - sum <= *chance {
                return path;
            }
            sum += chance;
        }
        &self.base
    }
}

/// `rand() · 0.003051851` (rand in 0..=32767).
fn roll() -> f32 {
    fastrand::i32(0..=32767) as f32 * 0.003_051_851
}

fn start_animations(
    mut commands: Commands,
    new: Query<(Entity, &DecorationAnims), Added<DecorationAnims>>,
    data: Res<gd_game_core::data::GameData>,
    mut cache: ResMut<ModelCache>,
) {
    for (e, d) in &new {
        let path = if d.weather { d.pick(roll()) } else { &d.base };
        if let Some(anim) = cache.anim(&data, path) {
            commands.entity(e).try_insert(Animator {
                anim,
                time: 0.0,
                speed: 1.0,
                looped: d.weather,
            });
        }
    }
}

/// A decoration's animation ended: the next random one (`AnimationCallback`).
fn next_animation(
    mut q: Query<(&DecorationAnims, &mut Animator)>,
    data: Res<gd_game_core::data::GameData>,
    mut cache: ResMut<ModelCache>,
) {
    for (d, mut a) in &mut q {
        if a.looped || d.play_once {
            continue;
        }
        let len = a.anim.num_frames.saturating_sub(1) as f32 / a.anim.frame_rate.max(1) as f32;
        if a.time < len {
            continue;
        }
        let over = a.time - len;
        if let Some(anim) = cache.anim(&data, d.pick(roll())) {
            a.anim = anim;
        }
        a.time = over;
    }
}

/// On a placed `Floater` (module doc): its bob.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Floater {
    /// `bobTime` in ms.
    period_ms: i32,
    max: f32,
    pub always: bool,
    /// The start and target heights of the current bob, ms into it.
    from: f32,
    to: f32,
    timer_ms: i32,
    /// The placed height (local), and the disturbance radius.
    base_y: f32,
    pub radius: f32,
}

impl Floater {
    /// `Floater::Load` (then [`Floater::place`]).
    pub fn from_record(rec: &Record) -> Option<Self> {
        if rec.class != "Floater" {
            return None;
        }
        let max = rec.f32("bobMax").unwrap_or(0.0);
        let to = generate(max * 0.5, max);
        Some(Self {
            period_ms: (rec.f32("bobTime").unwrap_or(0.0) * 1000.0) as i32,
            max,
            always: rec.bool("alwaysBob"),
            from: -to,
            to,
            timer_ms: 0,
            base_y: 0.0,
            radius: 0.0,
        })
    }

    /// `OnAddToLevel`: its placed local height and the half extents of its region box (`box_min`, `box_max`).
    /// Each placed one draws its own first target (`Load` runs per object; the record's parse is shared here).
    pub fn place(&mut self, base_y: f32, box_min: Vec3, box_max: Vec3) {
        self.to = generate(self.max * 0.5, self.max);
        self.from = -self.to;
        self.base_y = base_y;
        self.radius = ((box_max - box_min) * 0.5).length();
    }

    /// `UpdateSelf`'s step: whether the bob turned (the disturbance), and the height offset.
    /// PORT: a record without a positive `bobTime` stays still (the bob's phase divides by it).
    fn step(&mut self, dt_ms: i32) -> (bool, f32) {
        if self.period_ms <= 0 {
            return (false, 0.0);
        }
        self.timer_ms += dt_ms;
        let turned = self.timer_ms > self.period_ms;
        if turned {
            self.from = self.to;
            self.to = if self.to <= 0.0 {
                generate(self.max * 0.5, self.max)
            } else {
                generate(self.max * -0.5, -self.max)
            };
            self.timer_ms = 0;
        }
        let k = ((self.timer_ms as f32 / self.period_ms as f32 * std::f32::consts::PI
            - std::f32::consts::FRAC_PI_2)
            .sin()
            + 1.0)
            * 0.5;
        (turned, k * (self.to - self.from) + self.from)
    }
}

/// `RandomUniformLocked::FGenerate(lo, hi)` (PORT: `fastrand`).
fn generate(lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * fastrand::f32()
}

/// `Floater::UpdateSelf`: bob, and disturb the grass and water at each turn (module doc).
fn bob_floaters(
    time: Res<Time>,
    mut q: Query<(&mut Floater, &mut Transform, &GlobalTransform)>,
    mut out: MessageWriter<gd_game_core::visuals::PointDisturbance>,
    mut carry: Local<f32>,
) {
    let ms = time.delta_secs() * 1000.0 + *carry;
    let dt_ms = ms.floor();
    *carry = ms - dt_ms;
    let dt_ms = dt_ms as i32;
    for (mut f, mut tf, gt) in &mut q {
        let (turned, y) = f.step(dt_ms);
        if turned {
            out.write(gd_game_core::visuals::PointDisturbance {
                pos: gt.translation(),
                radius: f.radius,
                strength: 1.0,
                falloff: true,
                splash: 0,
            });
        }
        tf.translation.y = f.base_y + y;
    }
}
