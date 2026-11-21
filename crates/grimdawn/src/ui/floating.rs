//! Floating world texts ("+252 Iron"): game event 27
//! ([`gd_game::floating::FloatingText`]), shown by the game window's
//! floating-text list.
//!
//! Grim Dawn.exe: the game window registers a handler for event 27
//! (vtable 0x140309868, sub_14002E270) that adds an entry to its list at
//! +2200 (ctor sub_14000AC30, add sub_14000AD70, update sub_14000B400, draw
//! sub_14000BD30 from the window's draw sub_140027350). Adding:
//! - at most 20 entries (a 21st is dropped); the text must not be empty and
//!   the position must lie in a loaded region; an empty style means the
//!   engine's utility font style;
//! - duration 850 ms (1000 with flag 0x01 unless 0x10; × 2 with 0x20),
//!   alpha 1, scale = the event's scale, offset 0, direction (0, 1, 0);
//! - flag 0x10: the direction alternates per text between
//!   normalize(camera up + camera left) and normalize(up + right), × 1.75
//!   (the first goes up-left);
//! - flag 0x08: a delay of the longest pending delay among the same
//!   object's entries + 100 ms (with 0x10 or 0x01, else + 500).
//!
//! Each frame (dt in ms):
//! - while delayed, the delay counts down; when it ends the position snaps
//!   to the object's `HeadEffect` attach point;
//! - then the duration counts down (the entry goes at 0); the position
//!   follows the object's `HeadEffect` point while it has one;
//! - 0x02: offset += direction × 0.0025 × dt; unless 0x20 the scale grows by
//!   1.5 / s above 637.5 ms left and shrinks by 1 / s below 510 ms;
//! - else 0x10: the object is dropped (the text stays where it started),
//!   offset and scale as for 0x02, and the direction's y falls by 3 / s (an
//!   arc: up and sideways, then down);
//! - 0x04: alpha = time left / (base duration × (0.25 with 0x01, else
//!   0.5)) below that limit (425 ms for money);
//! - 0x01: the scale grows by 1.5 / s above 800 ms left and shrinks by
//!   2 / s below 200 ms.
//!
//! Draw (`GraphicsCanvas::RenderText3d`, Engine.dll @0x18009fb40): a
//! camera-facing text at position + offset, centred on it, in the style's
//! colour with the entry's alpha; the line is font size × scale × 0.009 m
//! tall and glyph widths are 1.33 × that height / the font's line height
//! per font pixel. PORT: rasterised at the style's size through the shared raster cache
//! ([`FontCache::rasterize_cached`], no texture per event) and scaled; the text is drawn over the
//! scene (no depth test).
//!
//! Combat numbers (`gd_game::floating` docs): flag 0x40 sets the direction to
//! normalize(3 × `rand` in [−1, 1]) (sub_14000AAF0); 0x80 keeps a list of
//! objects with a counter: the first text of an object adds it (counter 0, no
//! offset), each further one steps the counter (mod 7) and starts at the
//! offset a × camera right + b × camera up of dword_1402DDAC0 (0, 0),
//! (±0.6, ±0.5), (0, ±1); an object 3000 ms without a text leaves the list
//! (update); the text then drops the object (it does not follow it).
//! `GD_SHOT_ON` (hooks.rs) with the style record `moneypickup` shoots after a pickup.
use super::text::{FontCache, LoadedFont};
use crate::data::GameData;
use bevy::prelude::*;
use gd_game::floating::{FloatingText, TextArg};

const MAX_TEXTS: usize = 20;

pub struct FloatingPlugin;

impl Plugin for FloatingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FloatingTexts>().add_systems(
            PostUpdate,
            (add_texts, update_texts)
                .chain()
                .after(bevy::transform::TransformSystems::Propagate)
                .before(bevy::ui::UiSystems::Layout)
                .in_set(crate::ui::UiPlaySet),
        );
    }
}

struct Entry {
    entity: Option<Entity>,
    position: Vec3,
    offset: Vec3,
    direction: Vec3,
    delay: f32,
    duration: f32,
    flags: u32,
    alpha: f32,
    scale: f32,
    /// Style size (px), the raster's layout size and its draw offset.
    size: f32,
    layout: Vec2,
    raster_offset: Vec2,
    raster_size: Vec2,
    node: Entity,
}

/// The game window's floating-text list (+2200).
#[derive(Resource)]
pub struct FloatingTexts {
    entries: Vec<Entry>,
    /// The 0x10 side toggle (+124).
    left: bool,
    /// The 0x80 per-object offset counters (+128 list): object, counter,
    /// ms since its last text.
    slots: Vec<(Entity, usize, f32)>,
    /// The 0x40 combat-number scatter: a seeded engine RNG, not `fastrand`'s
    /// process generator (gameplay RNG stays untouched; the scatter is stable
    /// per session).
    scatter: gd_sim::combat::Rng,
}

/// The scatter's fixed seed (the draw is presentation, so any stable seed do).
const SCATTER_SEED: u32 = 0x5CA7_7E21;

impl Default for FloatingTexts {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            left: false,
            slots: Vec::new(),
            scatter: gd_sim::combat::Rng(SCATTER_SEED),
        }
    }
}

/// The 0x80 offsets (dword_1402DDAC0): (× camera right, × camera up).
const CRIT_OFFSETS: [(f32, f32); 7] = [
    (0.0, 0.0),
    (0.6, 0.5),
    (-0.6, 0.5),
    (0.6, -0.5),
    (-0.6, -0.5),
    (0.0, 1.0),
    (0.0, -1.0),
];

/// `text` with `{%d0}`, `{%d1}`… (or bare `%d0`) replaced by `args`
/// (colour codes kept).
pub fn format_tag(text: &str, args: &[i64]) -> String {
    let mut out = text.to_owned();
    for (i, a) in args.iter().enumerate() {
        out = out
            .replace(&format!("{{%d{i}}}"), &a.to_string())
            .replace(&format!("%d{i}"), &a.to_string());
    }
    out
}

/// The localised `tag` with its arguments (`{%d0}`, `{%.0f0}`, `{%s0}`…;
/// a [`TextArg::Tag`] is localised first, colour codes dropped).
pub fn format_text(fonts: &FontCache, tag: &str, args: &[TextArg]) -> String {
    use gd_sim::items::tooltip::{Arg, format_exact as format};
    let strs: Vec<String> = args
        .iter()
        .map(|a| {
            if let TextArg::Tag(t) = a {
                format(fonts.tag(t), &[])
            } else {
                String::new()
            }
        })
        .collect();
    let args: Vec<Arg> = args
        .iter()
        .zip(&strs)
        .map(|(a, s)| {
            if let TextArg::Num(v) = a {
                Arg::Num(*v)
            } else {
                Arg::Str(s.trim())
            }
        })
        .collect();
    format(fonts.tag(tag), &args).trim().to_owned()
}

/// The object's `HeadEffect` attach point (bind pose), else 1.6 m up.
fn head(
    chars: &Query<(&GlobalTransform, Option<&gd_render::character::Character>)>,
    e: Entity,
) -> Option<Vec3> {
    let (gt, ch) = chars.get(e).ok()?;
    Some(
        ch.and_then(|c| gd_game::casting::attach_point_bind(&c.body, "HeadEffect"))
            .map_or(gt.translation() + Vec3::Y * 1.6, |l| gt.transform_point(l)),
    )
}

#[allow(clippy::too_many_arguments)]
fn add_texts(
    mut fresh: gd_game::session::NewSession,
    mut commands: Commands,
    mut events: MessageReader<FloatingText>,
    mut list: ResMut<FloatingTexts>,
    data: Res<GameData>,
    fonts: ResMut<FontCache>,
    mut images: ResMut<Assets<Image>>,
    mut font_assets: ResMut<Assets<Font>>,
    cams: Query<&GlobalTransform, With<gd_render::camera::GameCamera>>,
    mut root: Local<Option<Entity>>,
) {
    if fresh.begun() {
        *root = Default::default();
        list.scatter = gd_sim::combat::Rng(SCATTER_SEED);
    }
    let mut fonts = fonts;
    let Ok(cam) = cams.single() else { return };
    for ev in events.read() {
        if list.entries.len() >= MAX_TEXTS {
            continue;
        }
        // `GD_SHOT_ON=moneypickup`: screenshots after an iron pickup.
        gd_game::visuals::notify(&mut commands, &ev.style);
        let text = format_text(&fonts, &ev.tag, &ev.args);
        if text.is_empty() {
            continue;
        }
        let Some(style) = fonts.style(&data, &ev.style) else {
            continue;
        };
        let Some(font) = fonts.font(&data, &style.font, &mut font_assets) else {
            continue;
        };
        let LoadedFont::Bitmap { font, atlas } = &*font else {
            continue;
        };
        let Some(ti) = fonts.rasterize_cached(&mut images, font, atlas, &style, style.size, &text)
        else {
            continue;
        };
        let flags = ev.flags;
        let mut duration = if flags & 0x10 == 0 && flags & 1 != 0 {
            1000.0
        } else {
            850.0
        };
        let mut direction = Vec3::Y;
        if flags & 0x10 != 0 {
            let (up, right) = (cam.up().as_vec3(), cam.right().as_vec3());
            direction = (if list.left { up + right } else { up - right }).normalize() * 1.75;
            list.left = !list.left;
        }
        if flags & 0x20 != 0 {
            duration *= 2.0;
        }
        let delay = if flags & 8 != 0 {
            let pending = list
                .entries
                .iter()
                .filter(|e| ev.entity.is_none() || e.entity == ev.entity)
                .map(|e| e.delay)
                .fold(0.0f32, f32::max);
            pending + if flags & 0x11 != 0 { 100.0 } else { 500.0 }
        } else {
            0.0
        };
        // 0x40: a random direction (each axis `rand` in [−1, 1], normalised).
        if flags & 0x40 != 0 {
            let r = |rng: &mut gd_sim::combat::Rng| rng.unit() * 2.0 - 1.0;
            direction = Vec3::new(
                r(&mut list.scatter),
                r(&mut list.scatter),
                r(&mut list.scatter),
            )
            .normalize_or(Vec3::Y);
        }
        // 0x80: the object's next offset (the first text of an object, or
        // one 3 s after its last, has none); the text does not follow it.
        let mut offset = Vec3::ZERO;
        let mut entity = ev.entity;
        if flags & 0x80 != 0 {
            if let Some(o) = ev.entity {
                match list.slots.iter_mut().find(|s| s.0 == o) {
                    Some(slot) => {
                        slot.1 = (slot.1 + 1) % CRIT_OFFSETS.len();
                        slot.2 = 0.0;
                        let (a, b) = CRIT_OFFSETS[slot.1];
                        offset = cam.right().as_vec3() * a + cam.up().as_vec3() * b;
                    }
                    None => list.slots.push((o, 0, 0.0)),
                }
            }
            entity = None;
        }
        let parent = *root.get_or_insert_with(|| {
            commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    GlobalZIndex(1),
                ))
                .id()
        });
        let node = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                ImageNode::new(ti.image.clone())
                    .with_mode(bevy::ui::widget::NodeImageMode::Stretch),
                Visibility::Hidden,
            ))
            .id();
        commands.entity(parent).add_child(node);
        list.entries.push(Entry {
            entity,
            position: ev.position,
            offset,
            direction,
            delay,
            duration,
            flags,
            alpha: 1.0,
            scale: ev.scale,
            size: style.size as f32,
            layout: Vec2::new(ti.layout.width as f32, ti.layout.height.max(1) as f32),
            raster_offset: ti.offset,
            raster_size: ti.size.as_vec2(),
            node,
        });
    }
}

fn update_texts(
    mut commands: Commands,
    time: Res<Time>,
    mut list: ResMut<FloatingTexts>,
    chars: Query<(&GlobalTransform, Option<&gd_render::character::Character>)>,
    cams: Query<(&Camera, &GlobalTransform), With<gd_render::camera::GameCamera>>,
    mut nodes: Query<(&mut Node, &mut ImageNode, &mut Visibility)>,
) {
    let dt = time.delta_secs() * 1000.0;
    let ds = dt * 0.001;
    let cam = cams.single().ok();
    list.slots.retain_mut(|s| {
        s.2 += dt;
        s.2 <= 3000.0
    });
    list.entries.retain_mut(|e| {
        if e.delay > 0.0 {
            e.delay -= dt;
            if e.delay <= 0.0
                && let Some(p) = e.entity.and_then(|o| head(&chars, o))
            {
                e.position = p;
            }
        } else {
            e.duration -= dt;
            if e.duration <= 0.0 {
                commands.entity(e.node).try_despawn();
                return false;
            }
            if let Some(p) = e.entity.and_then(|o| head(&chars, o)) {
                e.position = p;
            }
            let grow = |e: &mut Entry| {
                if e.duration < 510.0 {
                    e.scale = (e.scale - ds).max(0.0);
                } else if e.duration > 637.5 {
                    e.scale += ds * 1.5;
                }
            };
            if e.flags & 2 != 0 {
                e.offset += e.direction * 0.0025 * dt;
                if e.flags & 0x20 == 0 {
                    grow(e);
                }
            } else if e.flags & 0x10 != 0 {
                e.entity = None;
                e.offset += e.direction * 0.0025 * dt;
                grow(e);
                e.direction.y -= ds * 3.0;
            }
            if e.flags & 4 != 0 {
                let base = if e.flags & 0x10 == 0 && e.flags & 1 != 0 {
                    1000.0
                } else {
                    850.0
                };
                let limit = (base * if e.flags & 1 != 0 { 0.25 } else { 0.5 }) as i32 as f32;
                if e.duration < limit {
                    e.alpha = e.duration / limit;
                }
            }
            if e.flags & 1 != 0 {
                if e.duration < 200.0 {
                    e.scale = (e.scale - 2.0 * ds).max(0.0);
                } else if e.duration > 800.0 {
                    e.scale += ds * 1.5;
                }
            }
        }
        let Ok((mut node, mut img, mut vis)) = nodes.get_mut(e.node) else {
            return true;
        };
        let shown = e.delay <= 0.0
            && cam.is_some_and(|(cam, ct)| {
                // World height of the line and its projected size (px).
                let centre = e.position + e.offset;
                let h = e.size * e.scale * 0.009;
                let (Ok(c), Ok(top)) = (
                    cam.world_to_viewport(ct, centre),
                    cam.world_to_viewport(ct, centre + ct.up().as_vec3() * h),
                ) else {
                    return false;
                };
                let ky = (c - top).length() / e.layout.y;
                let kx = ky * 1.33;
                let origin = c - Vec2::new(e.layout.x * kx, e.layout.y * ky) * 0.5;
                node.left = Val::Px(origin.x + e.raster_offset.x * kx);
                node.top = Val::Px(origin.y + e.raster_offset.y * ky);
                node.width = Val::Px(e.raster_size.x * kx);
                node.height = Val::Px(e.raster_size.y * ky);
                img.color = Color::srgba(1.0, 1.0, 1.0, e.alpha);
                true
            });
        *vis = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        true
    });
}
