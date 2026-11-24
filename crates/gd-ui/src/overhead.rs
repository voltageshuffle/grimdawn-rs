//! The player's overhead health ("(6,519/6,519)" over a small green bar),
//! the world actor description (`records/ui/inworld/
//! worldactordescription.dbr`, loader `WorldActorDescription::Load`).
//!
//! Grim Dawn.exe:
//! - shown for the main player when the `playerHealth` option is
//!   on, which it is by default (`Options::Options`, Engine.dll); with
//!   `healthBarsPercent` (off by default) the text would be a
//!   percentage (`WorldActorDescription::AddEntry`);
//! - text `tagHUDHealth_Head` "({%t0}/{%t1})" with the current and maximum
//!   life as integers with thousands separators (`UI::FormatThousands`), in the
//!   `healthValuesBox` text box (`actor_head_health.dbr`: x −2, y −40,
//!   `style_special_enemyhealth`);
//! - anchor (`WorldActorDescription::ComputeAnchor` / `AddEntry`): the actor's position raised by
//!   the top of its mesh's bounding box (`GraphicsMesh` bounds: centre.y +
//!   half.y = msh chunk 10's max.y) plus the record's `worldDescOffset`,
//!   projected and rounded, then moved up by (`interactIconPosition` + 90) ×
//!   the zoom scale, 1.2 − 0.4 × (camera distance − min) / (max − min)
//!   (`WorldActorDescription::Update`: `WorldActorDescription.iconRaise` = 90, `WorldActorDescription.zoomScale`);
//! - draw (`WorldActorDescription::Draw`): the text box as wide as the text, centred on the
//!   anchor; the main player's bar `playerHealthBar`
//!   (`actor_head_healthbar_player.dbr`; `largePlayerHealthBar` with `playerBarsLarge`,
//!   113 x 15 px) centred on the anchor x with its top at the anchor y, filled to
//!   clamp(life / max, 0.02, 1) of the upper / lower display range
//!   (`GetUpper/LowerHealthDisplayPercentage`, 100 / 0 here).
//!
//! Options (`gd_game_core::options`): the main player's entry exists only with
//! `playerLocalBar` (`WorldActorDescription::ShouldShow`); then the text shows with
//! `playerHealth` and the bar with `playerBars`.
//!
//! PORT: the bar's empty bitmap is drawn under the fill (`renderBackground`
//! is not set on this record; the real bar shows a dark track).
//!
//! Other characters (Grim Dawn.exe, same widget): an entry per
//! character entering view (`WorldActorDescription::OnCharacterEnterView`) unless
//! `WorldActorDescription::ShouldShow` skips it:
//! one that is not alive, visible, targetable (`IsTargetable`) or anger-causing
//! (`CausesAnger`: the training dummy, a dormant host, totems),
//! NPCs, friends of the main player (`FactionManager::IsFriend`), a player's
//! pets without `petBars`, the main player without
//! `playerLocalBar`. PORT: `IsTargetable` is not tested. The entry (`WorldActorDescription::AddEntry`) keeps the
//! classification (`ResolveMonsterClassificationEnum`: Common 0 … SuperBoss
//! 5), the "(cur/max)" text (`tagHUDHealthPercent_Head` with `healthBarsPercent`), the
//! fill and whether the character was ever hit (`PlayStats::
//! GetHitsReceived`). Draw (`WorldActorDescription::Draw`) for a monster:
//! - undamaged (never hit and full life) without `monsterBarsUndamaged`,
//!   or `monsterBars` off: only the classification icon
//!   with `monsterIcons`, centred on the anchor: Hero `heroIcon`,
//!   Boss / SuperBoss `nemesisIcon`, Quest `bossIcon` (the loader's order:
//!   hero, boss, nemesis);
//! - else: the text with `monsterHealth`, as for the player; the
//!   bar `monsterHealthBar` (`largeMonsterHealthBar` with `monsterBarsLarge`,
//!   for Common / Champion, or `bossBarsLarge`, for Hero and
//!   above) centred on the anchor x and vertically on the small icon's centre
//!   line (`heroIconSmall` y offset + half its height − half the bar's); for
//!   Hero / Boss / Quest / SuperBoss the small icon on both sides, its left
//!   edge at anchor x − icon w / 2 ± (12 × UI scale + bar w / 2).
//!
//! PORT: "hit received" is life below its maximum; the anchor uses the
//! record's mesh top × the creature's `scale`; bars sit under the windows
//! (the engine draws the world widgets before the UI). A widget is built per
//! layout (classification, bar or icon, large bar, window size; the player's:
//! the options) around its anchor; its root follows the anchor and its fill
//! and text change in place (a performance choice).
use super::Ui;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::{BarGraph, Rect as URect, TextBox, Widget};
use gd_game_core::data::GameData;
use gd_game_core::player::Player;

const HEALTH_TEXT: &str = "records/ui/inworld/actor_head_health.dbr";
const HEALTH_BAR: &str = "records/ui/inworld/actor_head_healthbar_player.dbr";
/// The player's bar with `playerBarsLarge`: 113 x 15 px instead of 75 x 5.
const HEALTH_BAR_LARGE: &str = "records/ui/inworld/actor_head_largehealthbar_player.dbr";
const PLAYER_RECORD: &str = "records/creatures/pc/malepc01.dbr";

pub struct OverheadPlugin;

impl Plugin for OverheadPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (player_health, monster_bars)
                .after(bevy::transform::TransformSystems::Propagate)
                .before(bevy::ui::UiSystems::Layout)
                .in_set(gd_ui_kit::UiPlaySet),
        );
    }
}

#[derive(Component)]
struct OverheadRoot;

/// One character's overhead widget (positioned at its anchor every frame).
#[derive(Component)]
struct MonsterBar {
    owner: Entity,
    /// Frame it was spawned (an older one is despawned once a newer has drawn).
    frame: u32,
}

/// A bar's fill quad ([`bar_at`]), sized in place: the bar's width (px), the full bitmap's size and the
/// window's inverse scale factor.
#[derive(Component)]
struct OverheadFill {
    w: f32,
    full: UVec2,
    inv: f32,
}

impl OverheadFill {
    /// Fills the quad to `ratio` (as [`bar_at`] draws it).
    fn set(&self, node: &mut Mut<Node>, img: &mut Mut<ImageNode>, ratio: f32) {
        let w = Val::Px((self.w * ratio).round() * self.inv);
        if node.width != w {
            node.width = w;
        }
        super::set_image_src(
            img,
            URect::new(0.0, 0.0, self.full.x as f32 * ratio, self.full.y as f32),
        );
    }
}

/// Moves a widget's root to `anchor` (physical px) when it moved.
fn place_root(node: &mut Mut<Node>, anchor: Vec2, inv: f32) {
    let (l, t) = (Val::Px(anchor.x * inv), Val::Px(anchor.y * inv));
    if node.left != l || node.top != t {
        node.left = l;
        node.top = t;
    }
}

/// A widget root at `anchor` (its parts are placed around (0, 0)), drawn under the windows.
fn spawn_widget_root(
    commands: &mut Commands,
    marker: impl Bundle,
    anchor: Vec2,
    inv: f32,
) -> Entity {
    commands
        .spawn((
            marker,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(anchor.x * inv),
                top: Val::Px(anchor.y * inv),
                ..default()
            },
            GlobalZIndex(-1),
        ))
        .id()
}

/// The health text's style and placement around the anchor (`healthValuesBox`).
fn health_text_at(ui: &mut Ui) -> Option<super::TextAt> {
    let tb = TextBox::from_record(&ui.record(HEALTH_TEXT)?);
    Some(super::TextAt::Line {
        style: tb.style.clone()?,
        at: Vec2::new(ui.sc(tb.rect.x), ui.sc(tb.rect.y)),
        centre: true,
        tint: Color::srgba(1.0, 1.0, 1.0, 1.0),
    })
}

/// `1234567` → "1,234,567" (`UI::FormatThousands`).
pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// Top of the player's mesh bounds (msh chunk 10 max.y) and the record's
/// `worldDescOffset` / `interactIconPosition`.
#[derive(Default)]
struct PlayerMetrics {
    top: Option<f32>,
    desc_offset: f32,
    icon_offset: f32,
}

/// The player's widget as built: its layout (window size, text / bar shown, large bar), root, fill and text, and
/// the (life, max) the text shows.
struct PlayerWidget {
    key: (UVec2, u32, (bool, bool), bool),
    root: Entity,
    fill: Option<Entity>,
    text: Option<Entity>,
    numbers: (i64, i64),
}

/// The player's health text: `tagHUDHealth_Head` with the numbers (the loaded tags may lack braces).
fn player_text(fonts: &super::text::FontCache, (cur, max): (i64, i64)) -> String {
    let fmt = fonts.tag("tagHUDHealth_Head");
    if fmt == "tagHUDHealth_Head" {
        format!("({}/{})", thousands(cur), thousands(max))
    } else {
        fmt.replace("{%t0}", "%t0")
            .replace("{%t1}", "%t1")
            .replace("%t0", &thousands(cur))
            .replace("%t1", &thousands(max))
    }
}

/// The player's overhead text and bar: built when the window size or the options change, moved to the anchor and
/// filled / numbered in place every frame.
#[allow(clippy::too_many_arguments)]
fn player_health(
    mut fresh: gd_game_core::session::NewSession,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    mut bodies: ResMut<gd_game_core::creature::BodyCache>,
    values: Res<super::HudVitals>,
    player: Query<&GlobalTransform, With<Player>>,
    cams: Query<(&Camera, &GlobalTransform, &gd_render::camera::GameCamera)>,
    roots: Query<(Entity, &super::RootFrame), With<OverheadRoot>>,
    frame: Res<bevy::diagnostic::FrameCount>,
    (mut metrics, mut shown): (Local<Option<PlayerMetrics>>, Local<Option<PlayerWidget>>),
    options: Res<gd_game_core::options::GameOptions>,
    mut assets: gd_ui_kit::UiAssets,
    (mut nodes, mut fills, mut texts): (
        Query<&mut Node, (With<OverheadRoot>, Without<OverheadFill>)>,
        Query<(&OverheadFill, &mut Node, &mut ImageNode), Without<OverheadRoot>>,
        Query<&mut super::LiveText>,
    ),
) {
    if fresh.begun() {
        *metrics = Default::default();
        *shown = Default::default();
    }
    let (Ok(win), Ok(ptf), Ok((cam, cam_tf, gc))) =
        (window.single(), player.single(), cams.single())
    else {
        return;
    };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if size.x == 0 || size.y == 0 {
        return;
    }
    let m = metrics.get_or_insert_with(|| {
        let rec = data.db.get(PLAYER_RECORD);
        let top = rec
            .and_then(|r| r.str("mesh"))
            .and_then(|p| bodies.get(&data, p))
            .and_then(|m| m.bounding_box)
            .map(|(_, max)| max[1]);
        let f = |k: &str| rec.and_then(|r| r.i32(k)).unwrap_or(0) as f32;
        PlayerMetrics {
            top,
            desc_offset: f("worldDescOffset"),
            icon_offset: f("interactIconPosition"),
        }
    });
    let head = ptf.translation() + Vec3::Y * (m.top.unwrap_or(2.0) + m.desc_offset);
    let Ok(p) = cam.world_to_viewport(cam_tf, head) else {
        return;
    };
    let span = (gc.distance_max - gc.distance_min).max(1e-3);
    let zoom = 1.2 - (gc.distance - gc.distance_min) / span * 0.4;
    let p = (p * win.scale_factor()).round();
    let anchor = Vec2::new(p.x, p.y - (m.icon_offset + 90.0) * zoom);
    let numbers = (
        values.health.max(0.0) as i64,
        values.health_max.max(1.0) as i64,
    );
    let ratio = (numbers.0 as f32 / numbers.1 as f32).clamp(0.02, 1.0);
    // `WorldActorDescription::ShouldShow`: the main player's entry (text and bar) exists only with
    // `playerLocalBar`.
    let local = options.flag("playerLocalBar");
    let show = (
        local && options.flag("playerHealth"),
        local && options.flag("playerBars"),
    );
    let large = options.flag("playerBarsLarge");
    // The UI scale too (`f32` bits): the bar is built at it.
    let key = (size, assets.scale(size).to_bits(), show, large);
    let inv = 1.0 / win.scale_factor();
    let same = shown.as_ref().is_some_and(|w| w.key == key);
    super::retire_roots(
        &mut commands,
        roots.iter().map(|(e, f)| (e, f.0)),
        frame.0,
        !same,
        show == (false, false),
    );
    if let Some(w) = shown.as_mut().filter(|_| same) {
        if let Ok(mut node) = nodes.get_mut(w.root) {
            place_root(&mut node, anchor, inv);
        }
        if let Some(Ok((f, mut node, mut img))) = w.fill.map(|e| fills.get_mut(e)) {
            f.set(&mut node, &mut img, ratio);
        }
        if w.numbers != numbers {
            w.numbers = numbers;
            if let Some(e) = w.text
                && let Ok(mut live) = texts.get_mut(e)
            {
                let text = player_text(&assets.fonts, numbers);
                live.set(e, &text, &mut commands, &data, &mut assets);
            }
        }
        return;
    }
    if show == (false, false) {
        *shown = None;
        return;
    }
    let root = spawn_widget_root(&mut commands, OverheadRoot, anchor, inv);
    commands.entity(root).insert(super::RootFrame(frame.0));
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        assets.scale(size),
        inv,
        Vec2::new(size.x as f32, size.y as f32),
    );
    let fill = if show.1
        && let Some(Widget::BarGraph(bar)) =
            ui.widget(if large { HEALTH_BAR_LARGE } else { HEALTH_BAR })
    {
        bar_at(&mut ui, &bar, Vec2::ZERO, ratio)
    } else {
        None
    };
    let text = if show.0
        && let Some(at) = health_text_at(&mut ui)
    {
        let text = player_text(ui.fonts, numbers);
        Some(ui.live_text(at, &text))
    } else {
        None
    };
    *shown = Some(PlayerWidget {
        key,
        root,
        fill,
        text,
        numbers,
    });
}

/// A creature record's anchor metrics, classification and foe flag. PORT (a performance choice): cached
/// per record path in [`monster_bars`]; the mesh top comes from the shared [`gd_game_core::creature::BodyCache`]
/// (the body's own loader, parsed once), not a second `.msh` parse. Only the foe flag depends on the
/// session, recomputed when the factions' version changes.
#[derive(Clone)]
struct RecordFacts {
    /// The mesh box's top; `top_known` false while the mesh still loads ([`mesh_top`]; the bar uses 2 m meanwhile).
    top: Option<f32>,
    top_known: bool,
    scale: f32,
    desc_offset: f32,
    icon_offset: f32,
    classification: u8,
    /// `monsters::is_foe` with the session's factions at `foe_version`.
    foe: bool,
    foe_version: u64,
}

/// The top of `record`'s mesh box: `None` while the mesh is parsed in the background (`BodyCache::ready`, so a new
/// creature record never reads its mesh on the game thread), `Some(None)` without a mesh or box.
fn mesh_top(
    data: &GameData,
    bodies: &mut gd_game_core::creature::BodyCache,
    record: &str,
) -> Option<Option<f32>> {
    let Some(p) = data.db.get(record).and_then(|r| r.str("mesh")) else {
        return Some(None);
    };
    if !bodies.ready(data, p) {
        return None;
    }
    Some(
        bodies
            .loaded(&gd_formats::vfs::normalized(p))
            .flatten()
            .and_then(|m| m.bounding_box)
            .map(|(_, max)| max[1]),
    )
}

fn record_facts(
    data: &GameData,
    bodies: &mut gd_game_core::creature::BodyCache,
    factions: Option<&gd_game::factions::PlayerFactions>,
    record: &str,
) -> RecordFacts {
    let rec = data.db.get(record);
    let top = mesh_top(data, bodies, record);
    let f = |k: &str| rec.and_then(|r| r.i32(k)).unwrap_or(0) as f32;
    let classification = match rec
        .and_then(|r| r.str("monsterClassification"))
        .unwrap_or("Common")
    {
        "Champion" => 1,
        "Hero" => 2,
        "Boss" => 3,
        "Quest" => 4,
        "SuperBoss" => 5,
        _ => 0,
    };
    RecordFacts {
        top: top.flatten(),
        top_known: top.is_some(),
        scale: rec
            .and_then(|r| r.f32("scale"))
            .filter(|s| *s > 0.0)
            .unwrap_or(1.0),
        desc_offset: f("worldDescOffset"),
        icon_offset: f("interactIconPosition"),
        classification,
        foe: gd_game::monsters::is_foe(&data.db, factions, record),
        foe_version: factions.map_or(0, |f| f.version),
    }
}

const DESC_WIDGET: &str = "records/ui/inworld/worldactordescription.dbr";

type BarSubjects<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static GlobalTransform,
        &'static gd_game_core::combat::Combatant,
        Option<&'static gd_game_core::monsters::Monster>,
        Option<&'static gd_game::pets::Pet>,
        Option<&'static gd_game::combat::MonsterAi>,
        Option<&'static InheritedVisibility>,
    ),
    (
        Without<gd_game_core::combat::Dead>,
        Without<Player>,
        Or<(
            With<gd_game_core::monsters::Monster>,
            With<gd_game::pets::Pet>,
        )>,
    ),
>;

/// What a monster's widget is built for (its layout): the classification, the bar mode's large bar (`None`: the
/// icon only), the window size and the UI scale (`f32` bits: the widget is built at it). The fill and the text
/// change in place.
#[derive(Clone, Copy, PartialEq)]
struct Look {
    classification: u8,
    bar: Option<bool>,
    size: UVec2,
    scale: u32,
}

/// What a widget's text shows: the truncated life and max life, `monsterHealth` and `healthBarsPercent`.
type Numbers = (i64, i64, bool, bool);

/// A character in view this frame: its anchor, layout, fill (‰) and text inputs.
struct Seen {
    anchor: Vec2,
    look: Look,
    fill: u32,
    numbers: Numbers,
}

/// One character's widget as drawn: its root, layout, fill and text, and what the text shows.
struct Drawn {
    root: Entity,
    look: Look,
    fill: Option<Entity>,
    text: Option<Entity>,
    numbers: Numbers,
}

/// A monster's bar text: "(cur/max)" with thousands separators, the rounded percentage with
/// `healthBarsPercent`, nothing without `monsterHealth`.
fn monster_text((cur, max, health, percent): Numbers) -> String {
    if !health {
        String::new()
    } else if percent {
        let ratio = (cur as f32 / max as f32).clamp(0.02, 1.0);
        format!("{}%", (ratio * 100.0 + 0.5) as i32)
    } else {
        format!("({}/{})", thousands(cur), thousands(max))
    }
}

/// Overhead bars of the monsters (and, with `petBars`, the pets): see the
/// module docs. A widget is built when its layout ([`Look`]) changes; its root follows the anchor and its fill and
/// text change in place.
#[allow(clippy::too_many_arguments)]
fn monster_bars(
    mut fresh: gd_game_core::session::NewSession,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    mut bodies: ResMut<gd_game_core::creature::BodyCache>,
    subjects: BarSubjects,
    cams: Query<(&Camera, &GlobalTransform, &gd_render::camera::GameCamera)>,
    mut bars: Query<(Entity, &MonsterBar, &mut Node), Without<OverheadFill>>,
    frame: Res<bevy::diagnostic::FrameCount>,
    options: Res<gd_game_core::options::GameOptions>,
    factions: Option<Res<gd_game::factions::PlayerFactions>>,
    mut facts: Local<gd_formats::collections::HashMap<String, RecordFacts>>,
    (mut shown, mut live): (
        Local<gd_formats::collections::HashMap<Entity, Drawn>>,
        Local<gd_formats::collections::HashMap<Entity, Seen>>,
    ),
    mut assets: gd_ui_kit::UiAssets,
    (mut fills, mut texts): (
        Query<(&OverheadFill, &mut Node, &mut ImageNode), Without<MonsterBar>>,
        Query<&mut super::LiveText>,
    ),
) {
    if fresh.begun() {
        shown.clear();
        // The foe flags are the ended session's; the records' own facts are static.
        for f in facts.values_mut() {
            f.foe_version = u64::MAX;
        }
    }
    let (Ok(win), Ok((cam, cam_tf, gc))) = (window.single(), cams.single()) else {
        return;
    };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if size.x == 0 || size.y == 0 {
        return;
    }
    let inv = 1.0 / win.scale_factor();
    let span = (gc.distance_max - gc.distance_min).max(1e-3);
    let zoom = 1.2 - (gc.distance - gc.distance_min) / span * 0.4;
    let o = |k: &str| options.flag(k);
    let (bars_on, undamaged, health, icons, large, boss_large, percent, pets) = (
        o("monsterBars"),
        o("monsterBarsUndamaged"),
        o("monsterHealth"),
        o("monsterIcons"),
        o("monsterBarsLarge"),
        o("bossBarsLarge"),
        o("healthBarsPercent"),
        o("petBars"),
    );
    // Per character: anchor, layout, fill and text inputs. The facts (and the foe flag) are cached per record:
    // no record clone, DB lookup or mesh parse per monster per frame.
    let faction_version = factions.as_deref().map_or(0, |f| f.version);
    live.clear();
    let scale = assets.scale(size).to_bits();
    for (e, gt, c, monster, pet, ai, vis) in &subjects {
        if vis.is_some_and(|v| !v.get()) || c.life <= 0.0 {
            continue;
        }
        // `Character::CausesAnger` false (`WorldActorDescription::ShouldShow`): no widget at all, whatever the
        // options (a training dummy, a dormant host, a totem).
        let causes_anger = match (ai, pet) {
            (Some(ai), _) => ai.as_target.causes_anger,
            (None, Some(p)) => p.causes_anger,
            (None, None) => true,
        };
        if !causes_anger {
            continue;
        }
        let (record, is_monster) = match (monster, pet) {
            (Some(m), None) => (m.record.as_str(), true),
            (_, Some(_)) if pets => (monster.map_or("", |m| m.record.as_str()), false),
            _ => continue,
        };
        let f = match facts.get_mut(record) {
            Some(f) => f,
            None => {
                facts.insert(
                    record.to_owned(),
                    record_facts(&data, &mut bodies, factions.as_deref(), record),
                );
                facts.get_mut(record).unwrap()
            }
        };
        if !f.top_known
            && let Some(top) = mesh_top(&data, &mut bodies, record)
        {
            (f.top, f.top_known) = (top, true);
        }
        if f.foe_version != faction_version {
            f.foe = gd_game::monsters::is_foe(&data.db, factions.as_deref(), record);
            f.foe_version = faction_version;
        }
        if is_monster && !f.foe {
            continue;
        }
        let head = gt.translation() + Vec3::Y * (f.top.unwrap_or(2.0) * f.scale + f.desc_offset);
        let Ok(p) = cam.world_to_viewport(cam_tf, head) else {
            continue;
        };
        let p = (p * win.scale_factor()).round();
        let anchor = Vec2::new(p.x, p.y - (f.icon_offset + 90.0) * zoom);
        if anchor.x < -200.0
            || anchor.y < -200.0
            || anchor.x > size.x as f32 + 200.0
            || anchor.y > size.y as f32 + 200.0
        {
            continue;
        }
        let max = c.profile.sheet.life.max(1.0).trunc();
        let cur = c.life.max(0.0).trunc();
        let ratio = (cur / max).clamp(0.02, 1.0);
        let bar_mode = bars_on && (undamaged || cur < max || pet.is_some());
        let look = if bar_mode {
            Look {
                classification: f.classification,
                bar: Some(if f.classification >= 2 {
                    boss_large
                } else {
                    large
                }),
                size,
                scale,
            }
        } else if icons && f.classification >= 2 {
            Look {
                classification: f.classification,
                bar: None,
                size,
                scale,
            }
        } else {
            continue;
        };
        live.insert(
            e,
            Seen {
                anchor,
                look,
                fill: (ratio * 1000.0) as u32,
                numbers: (cur as i64, max as i64, health, percent),
            },
        );
    }
    // Old widgets: gone characters, and replaced ones once the new one drew.
    for (e, b, _) in &bars {
        let current =
            shown.get(&b.owner).is_some_and(|d| d.root == e) && live.contains_key(&b.owner);
        if !current && (b.frame < frame.0 || !live.contains_key(&b.owner)) {
            commands.entity(e).try_despawn();
        }
    }
    shown.retain(|o, _| live.contains_key(o));
    let s = assets.scale(size);
    for (&owner, seen) in live.iter() {
        if let Some(d) = shown.get_mut(&owner)
            && d.look == seen.look
        {
            if let Ok((_, _, mut node)) = bars.get_mut(d.root) {
                place_root(&mut node, seen.anchor, inv);
            }
            if let Some(Ok((f, mut node, mut img))) = d.fill.map(|e| fills.get_mut(e)) {
                f.set(&mut node, &mut img, seen.fill as f32 / 1000.0);
            }
            if d.numbers != seen.numbers {
                d.numbers = seen.numbers;
                if let Some(e) = d.text
                    && let Ok(mut lt) = texts.get_mut(e)
                {
                    let text = monster_text(seen.numbers);
                    lt.set(e, &text, &mut commands, &data, &mut assets);
                }
            }
            continue;
        }
        let bar = MonsterBar {
            owner,
            frame: frame.0,
        };
        let root = spawn_widget_root(&mut commands, bar, seen.anchor, inv);
        let mut ui = assets.ui(
            &mut commands,
            &data,
            root,
            s,
            inv,
            Vec2::new(size.x as f32, size.y as f32),
        );
        let (fill, text) = draw_monster(&mut ui, seen);
        shown.insert(
            owner,
            Drawn {
                root,
                look: seen.look,
                fill,
                text,
                numbers: seen.numbers,
            },
        );
    }
}

/// One monster's widget around (0, 0) = its anchor (`WorldActorDescription::Draw`): its fill and text, when it has
/// a bar.
fn draw_monster(ui: &mut Ui, seen: &Seen) -> (Option<Entity>, Option<Entity>) {
    let classification = seen.look.classification;
    let Some(desc) = ui.record(DESC_WIDGET) else {
        return (None, None);
    };
    let field = |k: &str| desc.str(k).map(str::to_owned).unwrap_or_default();
    // Icon fields by classification: Hero, Boss / SuperBoss (nemesis slot),
    // Quest (boss slot).
    let icon_key = match classification {
        2 => Some("heroIcon"),
        3 | 5 => Some("nemesisIcon"),
        4 => Some("bossIcon"),
        _ => None,
    };
    let anchor = Vec2::ZERO;
    let Some(large) = seen.look.bar else {
        // Icon only, centred on the anchor (plus its own offset).
        if let Some(k) = icon_key
            && let Some(Widget::Bitmap { bitmap, x, y }) = ui.widget(&field(k))
            && let Some((h, sz)) = ui.image(&bitmap)
        {
            let (w, hh) = (ui.sc(sz.x as f32), ui.sc(sz.y as f32));
            let r = URect::new(
                (-w * 0.5).trunc() + ui.sc(x),
                (-hh * 0.5).trunc() + ui.sc(y),
                w,
                hh,
            );
            ui.quad(h, r, None, Color::WHITE);
        }
        return (None, None);
    };
    let text = health_text_at(ui).map(|at| ui.live_text(at, &monster_text(seen.numbers)));
    let Some(Widget::BarGraph(bar)) = ui.widget(&field(if large {
        "largeMonsterHealthBar"
    } else {
        "monsterHealthBar"
    })) else {
        return (None, text);
    };
    // The small hero icon's geometry places the bar (any classification).
    let small = |ui: &mut Ui, k: &str| match ui.widget(&field(k)) {
        Some(Widget::Bitmap { bitmap, x, y }) => ui.image(&bitmap).map(|(h, sz)| (h, x, y, sz)),
        _ => None,
    };
    let (iy, ih) = small(ui, "heroIconSmall")
        .map_or((0.0, 0.0), |(_, _, y, sz)| (ui.sc(y), ui.sc(sz.y as f32)));
    let empty = bar.empty.as_deref().and_then(|p| ui.image(p));
    let full_size = bar.full.as_deref().and_then(|p| ui.image(p)).map(|f| f.1);
    let (bw, bh) = bar.base_size(
        empty.as_ref().map(|e| (e.1.x, e.1.y)),
        full_size.map(|f| (f.x, f.y)),
    );
    let (w, h) = (ui.sc(bw), ui.sc(bh));
    let top = (anchor.y + iy + ih * 0.5 - h * 0.5).round();
    // Side icons for Hero and above.
    if let Some(k) = icon_key.map(|k| format!("{k}Small"))
        && let Some((hdl, x, y, sz)) = small(ui, &k)
    {
        let iw = ui.sc(sz.x as f32);
        let left = anchor.x - iw * 0.5;
        let d = ui.sc(12.0) + w * 0.5;
        for ix in [left + d, left - d] {
            let r = URect::new(
                ix.round() + ui.sc(x),
                (anchor.y + ui.sc(y)).round(),
                iw,
                ui.sc(sz.y as f32),
            );
            ui.quad(hdl.clone(), r, None, Color::WHITE);
        }
    }
    let ratio = seen.fill as f32 / 1000.0;
    (bar_at(ui, &bar, Vec2::new(anchor.x, top), ratio), text)
}

/// The bar centred on `anchor.x`, top at `anchor.y`, filled to `ratio`: its fill quad ([`OverheadFill`]).
fn bar_at(ui: &mut Ui, bar: &BarGraph, anchor: Vec2, ratio: f32) -> Option<Entity> {
    let empty = bar.empty.as_deref().and_then(|p| ui.image(p));
    let (full, fs) = bar.full.as_deref().and_then(|p| ui.image(p))?;
    let (bw, bh) = bar.base_size(empty.as_ref().map(|e| (e.1.x, e.1.y)), Some((fs.x, fs.y)));
    let (w, h) = (ui.sc(bw), ui.sc(bh));
    let r = URect::new((anchor.x - (w * 0.5).trunc()).round(), anchor.y, w, h);
    if let Some((e, _)) = empty {
        ui.quad(e, r, None, Color::WHITE);
    }
    let fill = URect::new(r.x, r.y, (r.w * ratio).round(), r.h);
    let e = ui.quad(
        full,
        fill,
        Some(URect::new(0.0, 0.0, fs.x as f32 * ratio, fs.y as f32)),
        Color::WHITE,
    );
    ui.commands.entity(e).insert(OverheadFill {
        w: r.w,
        full: fs,
        inv: ui.inv,
    });
    Some(e)
}
