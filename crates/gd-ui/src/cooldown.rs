//! Cooldowns on the hotbar slots (the evade button, potions, skill slots).
//!
//! Grim Dawn.exe, hot slot update `HotSlotButton::Update` / draw
//! `HotSlotButton::Render`, its button's draw `ButtonStatic::Render` → `ButtonStatic::RenderCooldown` →
//! Engine.dll `GraphicsCanvas::RenderRadialWipeEffect`; Game.dll
//! `HotSlotOptionSkill::GetStatus` / `GetCooldownCompletion`:
//! - while the skill cools down (status 3) the icon is drawn as usual and a
//!   radial wipe in (0, 0, 0, 0.6) covers the part still to run: a triangle
//!   fan from the slot's centre through the point at angle completion × 2π
//!   (clockwise from 12 o'clock) and the points at every k·π/4 past it up
//!   to 2π, each at the half diagonal (`isCircular`: the smaller half
//!   extent) and clamped into the slot rect. Completion = 1 − left / total
//!   (`gd_game_core::casting::Cooldowns::completion`), so the dark part shrinks
//!   clockwise; nothing flashes when it ends;
//! - the seconds left, `ceil(ms / 1000)`, in the slot's own text widget
//!   (`HotSlotButton.cooldownText`, loaded from its record's `cooldownTextTable` by
//!   `HotSlotButton::Load` and drawn at the slot's position): the skill slots'
//!   `hud_hotslotcooldowntext.dbr` is centred on the slot, the potion
//!   buttons' `hud_hotslotcooldowntext_potion.dbr` is a smaller style in a
//!   box at (14, 18). The widget (loader `TextString::Load`) reads `style`, the
//!   text box and `bitmapName` / `additiveBitmapName`, so the potion
//!   record's `bitmapNameUp` glow is not drawn. Colours:
//!   `tagHUD_CooldownSecondsW` (^W) above 8 s, `…Y` (^Y) above 4 s, `…R`
//!   (^R) up to 4 s; `m:ss` above 59 s; `tagHUD_CooldownLong` above an hour;
//! - draw order: icon, wipe, the slot's overlay bitmap, key label, the
//!   seconds, so the label stays bright: the port redraws it over the wipe.
//!
//! A potion slot (`disableCooldownRadial`) with its combo mask draws a masked fill instead
//! (see `draw`, `ButtonStatic::RenderCooldown` / `ButtonStatic::RenderMaskedCooldown`).
//!
//! PORT: the wipe is a rasterised bitmap in [`STEPS`] steps (the engine
//! draws the fan), baked once per slot geometry into one tiled atlas image
//! (columns of steps, both dims kept within 4096 texels) whose sub-rect the
//! slot quad draws, so a running cooldown never mints a texture; the seconds
//! only with the `hotbarCooldownCounter`
//! option (off by default); the potion and evade slots use the skill rule
//! (their own status functions are not decompiled). The overlay is its own
//! node tree above the HUD (`GlobalZIndex(1)`).
use super::{FontCache, HudValues};
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;
use gd_formats::collections::HashMap;
use gd_formats::ui::Rect as URect;
use gd_game_core::casting::Caster;
use gd_game_core::data::GameData;

pub const STEPS: u16 = 512;

const WIPE: [f32; 4] = [0.0, 0.0, 0.0, 0.6];

#[derive(Clone, Debug, PartialEq)]
pub struct SlotCooldown {
    /// The HUD master field (`hudHotSlot3`, `hudEvadeButton`, …).
    pub field: &'static str,
    pub label: String,
    pub left: f32,
    /// Elapsed part, 0..1.
    pub completion: f32,
    pub skill: String,
}

const HOT_SLOTS: [&str; 10] = [
    "hudHotSlot1",
    "hudHotSlot2",
    "hudHotSlot3",
    "hudHotSlot4",
    "hudHotSlot5",
    "hudHotSlot6",
    "hudHotSlot7",
    "hudHotSlot8",
    "hudHotSlot9",
    "hudHotSlot10",
];

/// Writes the shown slots' running cooldowns into `out` (`row` = the hotbar row on the number slots).
pub fn slots(
    caster: &Caster,
    row: usize,
    labels: &std::collections::BTreeMap<&'static str, String>,
    out: &mut Vec<SlotCooldown>,
) {
    use gd_game_core::casting::{ENERGY_POTION, HEALTH_POTION, SLOT_LEFT, SLOT_RIGHT, SLOT_ROW2};
    let slot = |n: usize| caster.slots.get(n).and_then(|s| s.as_deref());
    let shown = [
        ("hudEvadeButton", Some(gd_game::evade::EVADE)),
        ("hudPriPotionButton", Some(HEALTH_POTION)),
        ("hudSecPotionButton", Some(ENERGY_POTION)),
    ]
    .into_iter()
    .chain(
        HOT_SLOTS
            .iter()
            .enumerate()
            .map(|(i, f)| (*f, slot(i + row * SLOT_ROW2))),
    )
    .chain([
        ("hudPriSkillButton", slot(SLOT_LEFT)),
        ("hudSecSkillButton", slot(SLOT_RIGHT)),
    ]);
    let mut n = 0;
    for (field, skill) in shown {
        let Some(skill) = skill else { continue };
        let Some(&left) = caster.cooldowns.get(skill).filter(|l| **l > 0.0) else {
            continue;
        };
        let completion = caster.cooldowns.completion(skill).unwrap_or(0.0);
        let label = labels.get(field).map(String::as_str).unwrap_or("");
        match out.get_mut(n) {
            Some(c) => {
                c.field = field;
                if c.label != label {
                    c.label.clear();
                    c.label.push_str(label);
                }
                (c.left, c.completion) = (left, completion);
                if c.skill != skill {
                    c.skill.clear();
                    c.skill.push_str(skill);
                }
            }
            None => out.push(SlotCooldown {
                field,
                label: label.to_owned(),
                left,
                completion,
                skill: skill.to_owned(),
            }),
        }
        n += 1;
    }
    out.truncate(n);
}

/// The seconds-left text (`HotSlotButton::Update`), with its colour code; `None` at 0.
pub fn text(fonts: &FontCache, left: f32) -> Option<String> {
    let ms = (left * 1000.0).round() as i64;
    let secs = (ms + 999).div_euclid(1000);
    if secs <= 0 {
        return None;
    }
    let tag = |t: &str, n: i64| super::floating::format_tag(fonts.tag(t), &[n]);
    Some(if secs > 3600 {
        fonts.tag("tagHUD_CooldownLong").to_owned()
    } else if secs > 59 {
        // GetTimeFormatA "m':'ss": minutes, seconds % (60 × minutes).
        let m = secs / 60;
        format!("{m}:{:02}", secs % (60 * m))
    } else if secs > 8 {
        tag("tagHUD_CooldownSecondsW", secs)
    } else if secs > 4 {
        tag("tagHUD_CooldownSecondsY", secs)
    } else {
        tag("tagHUD_CooldownSecondsR", secs)
    })
}

/// The wipe's fan (RenderRadialWipeEffect) in a `w`×`h` rect at the origin: the centre, the point at the
/// completion angle, then the π/4 points past it.
struct Fan {
    c: Vec2,
    r: f32,
    size: Vec2,
}

impl Fan {
    fn new(w: f32, h: f32, circular: bool) -> Self {
        let (hw, hh) = (w * 0.5, h * 0.5);
        let r = if circular {
            hw.min(hh)
        } else {
            (hw * hw + hh * hh).sqrt()
        };
        Self {
            c: Vec2::new(hw, hh),
            r,
            size: Vec2::new(w, h),
        }
    }

    /// The fan point at angle `a` (radians clockwise from 12 o'clock), clamped into the rect.
    fn at(&self, a: f32) -> Vec2 {
        let a = a - std::f32::consts::FRAC_PI_2;
        (self.c + Vec2::new(a.cos(), a.sin()) * self.r).clamp(Vec2::ZERO, self.size)
    }

    /// The angle (fraction of a turn) whose [`Fan::at`] point lies on the ray from the centre through `p`.
    fn turn(&self, p: Vec2, circular: bool) -> f32 {
        use std::f32::consts::{FRAC_PI_2, PI, TAU};
        let d = p - self.c;
        let a = if circular {
            d.y.atan2(d.x) + FRAC_PI_2
        } else if d.x.abs() * self.c.y > d.y.abs() * self.c.x {
            let cos = (-d.y * self.c.x / d.x.abs() / self.r).clamp(-1.0, 1.0);
            if d.x > 0.0 {
                cos.acos()
            } else {
                TAU - cos.acos()
            }
        } else {
            let sin = (d.x * self.c.y / d.y.abs() / self.r).clamp(-1.0, 1.0);
            if d.y < 0.0 {
                sin.asin()
            } else {
                PI - sin.asin()
            }
        };
        a.rem_euclid(TAU) / TAU
    }
}

fn in_tri(a: Vec2, b: Vec2, c: Vec2, p: Vec2) -> bool {
    let cross = |a: Vec2, b: Vec2, p: Vec2| (b - a).perp_dot(p - a);
    let (d1, d2, d3) = (cross(a, b, p), cross(b, c, p), cross(c, a, p));
    !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
}

/// A slot size's wipe samples (4×4 a pixel): each sample's span of steps `[first, last]` whose [`Fan`] covers it
/// (`[1, 0]`: none).
pub struct WipeMap(Vec<[u16; 2]>);

impl WipeMap {
    fn new(w: u32, h: u32, circular: bool) -> Self {
        use std::f32::consts::{FRAC_PI_4, TAU};
        let fan = Fan::new(w as f32, h as f32, circular);
        let c = fan.c;
        let pts: [Vec2; 9] = std::array::from_fn(|k| fan.at(k as f32 * FRAC_PI_4));
        // Each step's completion point and its first π/4 point past it (9: none, no triangle left).
        let steps: Vec<(Vec2, usize)> = (0..=STEPS)
            .map(|s| {
                let theta = s as f32 / STEPS as f32 * TAU;
                let next = (0..9).find(|&k| k as f32 * FRAC_PI_4 > theta);
                (fan.at(theta), next.unwrap_or(9))
            })
            .collect();
        let mut samples = Vec::with_capacity((w * h * 16) as usize);
        for y in 0..h {
            for x in 0..w {
                for i in 0..16 {
                    let p = Vec2::new(
                        x as f32 + (i % 4) as f32 * 0.25 + 0.125,
                        y as f32 + (i / 4) as f32 * 0.25 + 0.125,
                    );
                    // The last full-fan triangle holding `p`: the fan at a step keeps the triangles from its next
                    // π/4 point on, before them the completion triangle.
                    let full = (0..8).rev().find(|&j| in_tri(c, pts[j], pts[j + 1], p));
                    let covered = |s: u16| {
                        let (q, next) = steps[s as usize];
                        next < 9 && (full.is_some_and(|j| j >= next) || in_tri(c, q, pts[next], p))
                    };
                    let guess = ((fan.turn(p, circular) * STEPS as f32) as u16).min(STEPS);
                    let last = if covered(guess) {
                        let mut s = guess;
                        while s < STEPS && covered(s + 1) {
                            s += 1;
                        }
                        Some(s)
                    } else {
                        (guess.saturating_sub(4)..guess).rev().find(|&s| covered(s))
                    };
                    samples.push(match last {
                        Some(last) if full.is_some() => [0, last],
                        Some(last) => {
                            let (mut first, mut end) = (0, last);
                            while first < end {
                                let mid = (first + end) / 2;
                                if covered(mid) {
                                    end = mid;
                                } else {
                                    first = mid + 1;
                                }
                            }
                            [first, last]
                        }
                        None => [1, 0],
                    });
                }
            }
        }
        Self(samples)
    }
}

/// All [`STEPS`]+1 wipe bitmaps of a slot geometry in one image: `rows` steps per column,
/// column `c` row `r` holding step `c * rows + r` at (`c * w`, `r * h`); both dims fit 4096
/// texels for slot-sized rects. A sample's span `[first, last]` covers exactly those steps,
/// so one pass over the [`WipeMap`]'s samples fills every step's texel (4×4 samples a pixel).
/// Returns the image and `rows`.
fn wipe_atlas(map: &WipeMap, w: u32, h: u32) -> (Image, u32) {
    let steps = STEPS as u32 + 1;
    // Rows per column: fill the height limit, but enough rows that the columns fit the width.
    let min_rows = steps.div_ceil((4096 / w.max(1)).max(1));
    let rows = (4096 / h.max(1)).max(min_rows).clamp(1, steps);
    let cols = steps.div_ceil(rows);
    let (aw, ah) = (w * cols, h * rows);
    let texels: [[u8; 4]; 17] = std::array::from_fn(|n| wipe_texel(WIPE[3] * n as f32 / 16.0));
    let mut px = vec![0u8; (aw * ah * 4) as usize];
    // Per-pixel coverage count per step, as a difference array over the sample spans.
    let mut diff = vec![0i16; steps as usize + 1];
    for (i, samples) in map.0.as_chunks::<16>().0.iter().enumerate() {
        diff.fill(0);
        for &[first, last] in samples {
            if first <= last {
                diff[first as usize] += 1;
                diff[last as usize + 1] -= 1;
            }
        }
        let (x, y) = (i as u32 % w, i as u32 / w);
        let mut n = 0i16;
        for s in 0..steps {
            n += diff[s as usize];
            let o = (((y + s % rows * h) * aw + x + s / rows * w) * 4) as usize;
            px[o..o + 4].copy_from_slice(&texels[n as usize]);
        }
    }
    let image = Image::new(
        Extent3d {
            width: aw,
            height: ah,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        px,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    (image, rows)
}

/// Step `s`'s sub-rect (texture px) in its geometry's [`wipe_atlas`].
fn atlas_src(w: u32, h: u32, rows: u32, s: u16) -> URect {
    let s = s as u32;
    URect::new(
        (s / rows * w) as f32,
        (s % rows * h) as f32,
        w as f32,
        h as f32,
    )
}

fn wipe_texel(a: f32) -> [u8; 4] {
    [WIPE[0], WIPE[1], WIPE[2], a].map(|c| (c * 255.0).round() as u8)
}

fn solid_image() -> Image {
    let px = wipe_texel(WIPE[3]);
    Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        px.to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

#[derive(Component)]
pub struct CooldownRoot;

/// A drawn slot's wipe or masked fill.
#[derive(Component)]
pub struct SlotShade {
    field: &'static str,
    kind: Shade,
}

enum Shade {
    /// The slot's size in px and shape (the wipe bitmap's key).
    Wipe { w: u32, h: u32, circular: bool },
    /// The slot rect, the mask's top offset and the height between the offsets (px).
    Mask { r: URect, top: f32, region: f32 },
}

/// A drawn slot's seconds text and the whole seconds it shows.
#[derive(Component)]
pub struct SlotText {
    field: &'static str,
    secs: i64,
}

/// What the overlay was built for.
pub struct Layout {
    size: UVec2,
    scale: u32,
    generation: u64,
    counter: bool,
    slots: Vec<(&'static str, String, String)>,
    skills: gd_sim::skills::SkillBook,
}

fn step(c: &SlotCooldown) -> u16 {
    (c.completion * STEPS as f32).floor().min(STEPS as f32) as u16
}

/// The whole seconds [`text`] shows.
fn whole_secs(left: f32) -> i64 {
    ((left * 1000.0).round() as i64 + 999).div_euclid(1000)
}

/// `ButtonStatic::RenderMaskedCooldown`'s fill rect.
fn mask_rect(r: URect, top: f32, region: f32, completion: f32) -> URect {
    URect::new(r.x, r.y + top, r.w, (region * (1.0 - completion)).max(0.0))
}

/// Builds the overlay when the slots cooling down (or its inputs) change; every other frame updates it in place.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut fresh: gd_game_core::session::NewSession,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    (values, vitals): (Res<HudValues>, Res<super::HudVitals>),
    roots: Query<(Entity, &super::RootFrame), With<CooldownRoot>>,
    frame: Res<bevy::diagnostic::FrameCount>,
    mut last: Local<Option<Layout>>,
    mut classic: Local<bool>,
    mut wipes: Local<HashMap<(u32, u32, bool), (Handle<Image>, u32)>>,
    options: Res<gd_game_core::options::GameOptions>,
    mut assets: gd_ui_kit::UiAssets,
    (mut shades, mut texts, parents): (
        Query<(
            &SlotShade,
            &mut Node,
            Option<&mut ImageNode>,
            &mut Visibility,
        )>,
        Query<(Entity, &mut SlotText, &mut super::LiveText)>,
        Query<&ChildOf>,
    ),
) {
    if fresh.begun() {
        *last = None;
        wipes.clear();
    }
    // Bound the atlas cache: one texture per geometry; a resolution / UI-scale change mints new geometries.
    if wipes.len() > 32 {
        wipes.clear();
    }
    // No layout without a window: the next one rebuilds (the skill book compare below then sees
    // only `values` changes since this system's last run, which this return skips).
    let Some((win, size)) = window
        .single()
        .ok()
        .map(|w| (w, UVec2::new(w.physical_width(), w.physical_height())))
        .filter(|(_, s)| s.x > 0 && s.y > 0)
    else {
        *last = None;
        return;
    };
    if std::mem::replace(&mut *classic, assets.classic_hud()) != *classic {
        *last = None;
    }
    let counter = options.flag("hotbarCooldownCounter");
    let cooldowns = &vitals.cooldowns;
    let scale = assets.scale(size).to_bits();
    let generation = assets.fonts.generation;
    let same = last.as_ref().is_some_and(|l| {
        l.size == size
            && l.scale == scale
            && l.generation == generation
            && l.counter == counter
            && l.slots.len() == cooldowns.len()
            && l.slots
                .iter()
                .zip(cooldowns)
                .all(|(s, c)| s.0 == c.field && s.1 == c.skill && s.2 == c.label)
            && (!values.is_changed() || l.skills.same(&values.skills))
    });
    let kept = super::retire_roots(
        &mut commands,
        roots.iter().map(|(e, f)| (e, f.0)),
        frame.0,
        !same,
        cooldowns.is_empty(),
    );
    let inv = 1.0 / win.scale_factor();
    if same {
        let cooldown = |field: &str| cooldowns.iter().find(|c| c.field == field);
        for (shade, mut node, img, mut vis) in &mut shades {
            let Some(c) = cooldown(shade.field) else {
                continue;
            };
            let st = step(c);
            let shown = match shade.kind {
                Shade::Wipe { w, h, circular } => {
                    let (handle, rows) = wipes.entry((w, h, circular)).or_insert_with(|| {
                        let (image, rows) = wipe_atlas(&WipeMap::new(w, h, circular), w, h);
                        (assets.images.add(image), rows)
                    });
                    if let Some(mut img) = img {
                        if img.image != *handle {
                            img.image = handle.clone();
                        }
                        gd_ui_kit::set_image_src(&mut img, atlas_src(w, h, *rows, st));
                    }
                    st < STEPS
                }
                Shade::Mask { r, top, region } => {
                    super::set_node_rect(&mut node, mask_rect(r, top, region, c.completion), inv);
                    c.completion > 0.0 && st < STEPS
                }
            };
            vis.set_if_neq(if shown {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
        // Only the kept root's texts: a retired root's despawn is queued.
        for (e, mut t, mut live) in &mut texts {
            let Some(c) = cooldown(t.field) else { continue };
            if kept != Some(parents.root_ancestor(e)) {
                continue;
            }
            let secs = whole_secs(c.left);
            if t.secs != secs {
                t.secs = secs;
                let s = text(&assets.fonts, c.left).unwrap_or_default();
                live.set(e, &s, &mut commands, &data, &mut assets);
            }
        }
        return;
    }
    *last = Some(Layout {
        size,
        scale,
        generation,
        counter,
        slots: cooldowns
            .iter()
            .map(|c| (c.field, c.skill.clone(), c.label.clone()))
            .collect(),
        skills: values.skills.clone(),
    });
    if cooldowns.is_empty() {
        return;
    }
    let root = gd_ui_kit::spawn_frame_root(&mut commands, CooldownRoot, frame.0, 1);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        assets.scale(size),
        inv,
        Vec2::new(size.x as f32, size.y as f32),
    );
    let (Some(m), Some(to)) = (ui.record(ui.hud_master()), super::hud::toolbar_origin(&ui)) else {
        return;
    };
    let mut shade: Option<Handle<Image>> = None;
    for c in cooldowns {
        let st = step(c);
        let Some(rec) = m.str(c.field).map(str::to_owned) else {
            continue;
        };
        let Some(r) = super::hud::slot_rect(&ui, c.field) else {
            continue;
        };
        let circular = matches!(
            ui.widget(&rec),
            Some(gd_formats::ui::Widget::HotSlot { circular: true, .. })
        );
        let (w, h) = (r.w.max(1.0) as u32, r.h.max(1.0) as u32);
        // `ButtonStatic::RenderMaskedCooldown`: a `disableCooldownRadial` button with a mask draws the masked fill.
        let mask = matches!(
            ui.widget(&rec),
            Some(gd_formats::ui::Widget::HotSlot {
                disable_radial: true,
                ..
            })
        )
        .then(|| super::hud::potion_bitmap(&ui.data.db, &c.skill, &values.skills))
        .flatten()
        .filter(|b| !b.mask.is_empty())
        .and_then(|b| ui.image(&b.mask).map(|(h, size)| (b, h, size)));
        let (e, kind, shown) = if let Some((b, mh, msize)) = mask {
            let hh = msize.y.max(1) as f32;
            let top = b.mask_top as f32 / hh * r.h;
            let region = r.h - top - r.h * (b.mask_bottom as f32 / hh);
            let base = shade
                .get_or_insert_with(|| ui.images.add(solid_image()))
                .clone();
            let dst = mask_rect(r, top, region, c.completion);
            let e = super::masked::quad(&mut ui, base, mh, dst, r);
            let shown = c.completion > 0.0 && st < STEPS;
            (e, Shade::Mask { r, top, region }, shown)
        } else {
            let (handle, rows) = wipes.entry((w, h, circular)).or_insert_with(|| {
                let (image, rows) = wipe_atlas(&WipeMap::new(w, h, circular), w, h);
                (ui.images.add(image), rows)
            });
            let e = ui.quad(
                handle.clone(),
                r,
                Some(atlas_src(w, h, *rows, st)),
                Color::WHITE,
            );
            (e, Shade::Wipe { w, h, circular }, st < STEPS)
        };
        ui.commands.entity(e).insert((
            SlotShade {
                field: c.field,
                kind,
            },
            if shown {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
        ));
        super::hud::key_label(&mut ui, &rec, to, &c.label, None);
        let table = match ui.widget(&rec) {
            Some(gd_formats::ui::Widget::HotSlot { cooldown_text, .. }) => cooldown_text,
            _ => None,
        };
        if counter
            && let Some(gd_formats::ui::Widget::HotSlot { text: tb, .. }) =
                table.and_then(|t| ui.widget(&t))
        {
            let s = text(ui.fonts, c.left).unwrap_or_default();
            let at = super::TextAt::Box {
                tb,
                origin: Vec2::new(r.x, r.y),
            };
            let e = ui.live_text(at, &s);
            ui.commands.entity(e).insert(SlotText {
                field: c.field,
                secs: whole_secs(c.left),
            });
        }
    }
}
