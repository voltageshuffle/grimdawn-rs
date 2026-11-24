//! The riftgate map window (`records/ui/riftgatemap/riftgate_mastertable.dbr`,
//! `worldmapwindow.tpl`): the world map with the zones' gate icons and the
//! player's devotion shrines. It shows [`RiftgateMap`] (opened by using a
//! riftgate) with [`TravelState`] / [`DevotionState`] and sends
//! [`RiftgateRequest`]s; the travel rules are in `gd_game::riftgates`.
//!
//! `Grim Dawn.exe` (widget `RiftgateMap`: loader `RiftgateMap::Load`,
//! icons `RiftgateMap::BuildDestinations`, map sections `RiftgateMap::LoadSections`, draw `RiftgateMap::Draw`, mouse
//! `RiftgateMap::HandleMouseEvent`, wheel `RiftgateMap::HandleWheel`, update `RiftgateMap::Update`):
//! - the window rect is the record's (1001 × 792, centred); the map view is
//!   `mapPosition` (scaled, from the window's corner) × `mapSize`, clipped;
//! - the map is the `mapSections` bitmaps (`texture` at `x/yPosition`, all
//!   scaled); their bounds give the map's centre. Map point `p` (record
//!   units) is drawn at `view centre + s·p − bounds centre − pan`; the pan is
//!   clamped to ±(bounds − view) / 2 per axis (0 when the map is smaller);
//! - on top of the sections: the `labelTag<i>` texts (i from 0 until a tag is
//!   empty, at most 150) at `labelX/Y<i>` × s in `labelPrimaryStyle` or
//!   `labelSecondaryStyle` (`labelStyle<i>` = "Primary"), each `^n`-split
//!   line centred on the point, lines 0.75 line heights apart;
//! - the zones are `Region001ZoneList` (`ZoneManager` page 0; zone record:
//!   `WindowLocationX/Y`, `ZoneNameTag`, `TeleportNameTag`,
//!   `hideUndiscovered`). Each discovered gate (in the player's list order)
//!   goes to its region's zone (`Region::GetZoneRecord`); the zone's n-th
//!   gate icon is at its location + n × `iconSpacing` in x (minus for
//!   `zoneNameLocation` Right), named by the zone's `TeleportNameTag`. Its
//!   bitmaps: `staticPortal`, `staticPortalCurrent` (the gate in use while
//!   the player is within 8 m of it), `staticPortalHover`. A zone without a
//!   discovered gate and without `hideUndiscovered` gets one `undiscovered`
//!   icon (not hoverable or clickable). Icons are centred on their point;
//! - shrines: `Region001ShrineList` records (`WindowLocationX/Y`,
//!   `corruptedShrine`); a restored shrine of the player shows
//!   `cleansedShrine` / `cleansedShrineHover` ("tagShrineRestored"), a
//!   discovered one `corruptedShrine*` ("tagShrineDesecrated") or
//!   `ruinedShrine*` ("tagShrineRuined") by the record's flag;
//! - the `backgroundImage` frame is drawn over the map, then the
//!   `destinationTitle` text (the hovered icon's name; empty otherwise, as
//!   every mouse move inside the window clears it) and the close button;
//! - mouse: hovering a gate / shrine icon shows its name and the hover
//!   bitmap; pressing and releasing the left button on the same gate icon
//!   asks to travel; pressing elsewhere on the map (left or right button)
//!   drags it; the wheel pans 12.5 px per step (x, and y inverted);
//! - the player's personal portal (`GameEngine::GetTeleportInfoList`,
//!   `RiftgateMap::AddPersonalPortal`) is an icon of its region's zone, after the zone's gates
//!   (the zone counts as discovered): `personalPortal`,
//!   `personalPortalCurrent` (the portal opened the map), `personalPortalHover`,
//!   named "<player name> - <area name>"; it travels back to the portal;
//! - on open the map centres on the zone of the discovered gate nearest to
//!   the player, else the last used gate (`RiftgateMap::CenterOnStart`);
//! - the map lives in the map window ([`WINDOW`], `UIWorldMap`: loader
//!   `UIWorldMap::Load`, draw `UIWorldMap::Draw`, open `UIWorldMap::Open`, clicks
//!   `UIWorldMap::OnButtonClick`), which draws over it its round `centerButton` (a click
//!   centres the map as on opening) and the `switchMapButton` tab labelled
//!   `tagWorldMapWindowName` ("World Map"; "tagMapWindowName" over the
//!   aerial map); the switch toggles to the local map (`localmap.rs`,
//!   `UIWorldMap::OnButtonClick`). `M` opens the same window without a gate (no travel);
//! - the window's buttons draw `bitmapNameInFocus` while hovered (`ButtonStatic`
//!   draw `ButtonStatic::Render`), and the centre button shows its `rolloverTag`
//!   ("Center") as the generic button tooltip (`ui::rollover::button_tip`);
//!   the close and switch buttons have no `rolloverTag`; every button plays its
//!   record's `soundNameDown` on press (the shared `ButtonSound`, `ui/widgets.rs`);
//! - there is no zoom: the wheel only pans (`RiftgateMap::HandleWheel`). The sections are
//!   drawn at their textures' loaded size × the UI scale (`RiftgateMap::LoadSections`), so a
//!   texture still streaming at a lower mip shows the map smaller (as in the
//!   real game).
//!
//! The HUD teleport button's clicks are handled here too ([`teleport_button`]).
//!
//! The window is built when what it shows changes; a drag moves the content node (`pan_content`) and the
//! hover (icon / button hover bitmaps, the title) is `live`'s: hidden bitmaps shown in place, never a rebuild.
//!
//! PORT: the portal's region is the map region whose bounds hold it (the
//! engine asks the portal's `WorldVec3`); the area name joins the strings
//! (the engine formats it with `SimpleStringFormat`); zone names are not drawn (`zoneNameLocation` is
//! Hidden in stock data); the nearest gate ignores
//! `World::AreRegionsConnected`; the pan arrow buttons (`pan*Image`, empty in
//! stock data) are not drawn; the map opened with `M` waits for the player before centring; label lines drop markup; the wheel step is per
//! Bevy scroll line; Esc closes.
use super::contains;
use gd_ui::state::riftmap::{RiftMapView, RiftLayout, Icon, IconKind};
use gd_ui::window::Rebuild;
use super::Ui;
use super::text::FontCache;
use gd_ui::state::inventory::UiCapture;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::map::Uid;
use gd_formats::ui::{Rect as URect, TextAlign, TextBox, WindowDef};
use gd_game::devotion::DevotionState;
use gd_game::riftgates::{Rift, RiftgateMap, RiftgateRequest, TravelState, WaypointIndex};
use gd_game_core::data::GameData;

pub const RECORD: &str = "records/ui/riftgatemap/riftgate_mastertable.dbr";

/// The map window holding the riftgate map (`UIWorldMap`, `mapwindow.tpl`).
pub const WINDOW: &str = "records/ui/mapaerial/combinedmapconfiguration.dbr";

pub struct RiftMapUiPlugin;

impl Plugin for RiftMapUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(super::localmap::LocalMapPlugin)
            .init_resource::<RiftMapView>()
            .add_systems(
                Update,
                teleport_button.in_set(gd_ui_kit::UiInputSet).after(clicks),
            );
        // Live: the world map page is shown, or its root is still up (the closing frame despawns it).
        gd_ui::window::add_window(
            app,
            build,
            (pan_content, live, clicks).chain(),
            |map: Res<RiftgateMap>,
             win: Res<gd_ui::state::map::MapWindow>,
             roots: Query<(), With<RiftRoot>>| {
                win.world_shown(&map) || !roots.is_empty()
            },
        );
    }
}




/// The personal portal as the map shows it: its object id, zone record and name.
pub struct PortalIcon {
    pub id: u32,
    pub zone: String,
    pub name: String,
}




#[derive(Component)]
struct RiftRoot;

#[derive(Component)]
struct RiftContent;

fn at(r: &gd_formats::arz::Record) -> Vec2 {
    Vec2::new(
        r.f32("WindowLocationX").unwrap_or(0.0),
        r.f32("WindowLocationY").unwrap_or(0.0),
    )
}

/// The map's icons (module doc), zones first in list order, then shrines.
pub fn icons(
    data: &GameData,
    fonts: &FontCache,
    map: &RiftgateMap,
    travel: Option<&TravelState>,
    devotion: Option<&DevotionState>,
    index: Option<&WaypointIndex>,
    portal: Option<&PortalIcon>,
) -> Vec<Icon> {
    let Some(rec) = data.db.get(RECORD) else {
        return Vec::new();
    };
    let field = |k: &str| rec.str(k).unwrap_or("").to_owned();
    let spacing = rec.f32("iconSpacing").unwrap_or(18.0);
    let sign = if rec.str("zoneNameLocation") == Some("Right") {
        -1.0
    } else {
        1.0
    };
    let name = |tag: &str| gd_formats::text::strip_markup(fonts.tag(tag));
    let mut out = Vec::new();
    // Zones: (record, its icons).
    let zones: Vec<String> = rec
        .strs("Region001ZoneList")
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let mut found = vec![false; zones.len()];
    let mut per_zone: Vec<Vec<Icon>> = vec![Vec::new(); zones.len()];
    let gate = [
        field("staticPortal"),
        field("staticPortalCurrent"),
        field("staticPortalHover"),
    ];
    for uid in travel.map_or(&[][..], |t| &t.teleports) {
        let Some(p) = index.and_then(|i| i.0.get(uid)) else {
            continue;
        };
        let Some(z) = zones.iter().position(|z| *z == p.map_record) else {
            continue;
        };
        let Some(zr) = data.db.get(&zones[z]) else {
            continue;
        };
        found[z] = true;
        let n = per_zone[z].len() as f32;
        let current = map.gate == Some(Rift::Gate(*uid)) && map.at_gate;
        per_zone[z].push(Icon {
            at: at(zr) + Vec2::X * sign * n * spacing,
            images: gate.clone(),
            name: name(zr.str("TeleportNameTag").unwrap_or("")),
            kind: IconKind::Gate(*uid),
            zone: at(zr),
            current,
        });
    }
    if let Some(p) = portal
        && let Some(z) = zones.iter().position(|z| *z == p.zone)
        && let Some(zr) = data.db.get(&zones[z])
    {
        found[z] = true;
        let n = per_zone[z].len() as f32;
        let images = [
            field("personalPortal"),
            field("personalPortalCurrent"),
            field("personalPortalHover"),
        ];
        let current = map.gate == Some(Rift::Portal(p.id));
        per_zone[z].push(Icon {
            at: at(zr) + Vec2::X * sign * n * spacing,
            images,
            name: p.name.clone(),
            kind: IconKind::Portal(p.id),
            zone: at(zr),
            current,
        });
    }
    let undiscovered = field("undiscovered");
    for (z, icons) in per_zone.into_iter().enumerate() {
        if !icons.is_empty() {
            out.extend(icons);
        } else if let Some(zr) = data.db.get(&zones[z])
            && !found[z]
            && !zr.bool("hideUndiscovered")
        {
            out.push(Icon {
                at: at(zr),
                images: std::array::from_fn(|_| undiscovered.clone()),
                name: name(zr.str("ZoneNameTag").unwrap_or("")),
                kind: IconKind::Undiscovered,
                zone: at(zr),
                current: false,
            });
        }
    }
    // Shrines.
    let shrines: Vec<String> = rec
        .strs("Region001ShrineList")
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let mut shown = vec![false; shrines.len()];
    let mut add = |uids: &[Uid], restored: bool, out: &mut Vec<Icon>| {
        for uid in uids {
            let Some(p) = index.and_then(|i| i.0.get(uid)) else {
                continue;
            };
            let Some(k) = shrines
                .iter()
                .position(|s| *s == p.map_record)
                .filter(|k| !shown[*k])
            else {
                continue;
            };
            let Some(sr) = data.db.get(&shrines[k]) else {
                continue;
            };
            shown[k] = true;
            let (img, hover, tag) = if restored {
                ("cleansedShrine", "cleansedShrineHover", "tagShrineRestored")
            } else if sr.bool("corruptedShrine") {
                (
                    "corruptedShrine",
                    "corruptedShrineHover",
                    "tagShrineDesecrated",
                )
            } else {
                ("ruinedShrine", "ruinedShrineHover", "tagShrineRuined")
            };
            out.push(Icon {
                at: at(sr),
                images: [field(img), field(img), field(hover)],
                name: name(tag),
                kind: IconKind::Shrine,
                zone: at(sr),
                current: false,
            });
        }
    };
    if let Some(d) = devotion {
        add(&d.restored, true, &mut out);
        add(&d.discovered, false, &mut out);
    }
    out
}

/// A label's lines: the text split at `^n` / `^N`, its `^X` markup drawn by `ui/text.rs` (a colour carries over a
/// line break, [`super::text::active_color`]); `{` / `}` grouping is not drawn.
fn label_lines(text: &str) -> Vec<String> {
    let mut carry = String::new();
    text.replace("^N", "^n")
        .split("^n")
        .map(|l| {
            let line = format!("{carry}{}", l.replace(['{', '}'], ""));
            carry = super::text::active_color(&line);
            line
        })
        .collect()
}

/// Draws `text` centred on `centre` (both axes).
fn centred(ui: &mut Ui, style: &str, centre: Vec2, text: &str) {
    let tb = TextBox {
        style: None,
        rect: URect::new(-2000.0, 0.0, 4000.0, 0.0),
        align_x: TextAlign::Center,
        align_y: TextAlign::Center,
        tag: None,
        glow: None,
    };
    ui.text_with_style(&tb, style, centre, text);
}

#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    map: Res<RiftgateMap>,
    travel: Option<Res<TravelState>>,
    devotion: Option<Res<DevotionState>>,
    index: Option<Res<WaypointIndex>>,
    portal: (
        Res<gd_game::riftgates::PersonalPortal>,
        Option<Res<crate::world::StreamedMap>>,
        Res<gd_ui::HudValues>,
    ),
    players: Query<&Transform, With<gd_game_core::player::Player>>,
    roots: Query<Entity, With<RiftRoot>>,
    mapwin: Res<gd_ui::state::map::MapWindow>,
    mut view: ResMut<RiftMapView>,
    mut assets: gd_ui_kit::UiAssets,
    mut fresh: gd_game_core::session::NewSession,
    mut built: Local<
        Rebuild<(
            RiftgateMap,
            [usize; 3],
            bool,
            UVec2,
            u32,
            Option<(u32, Option<String>)>,
            (bool, bool),
        )>,
    >,
) {
    if fresh.begun() {
        built.reset();
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let shown = mapwin.world_shown(&map);
    if !shown {
        view.centred = false;
        view.drag = None;
        view.pressed = None;
    }
    let known = [
        travel.as_ref().map_or(0, |t| t.teleports.len()),
        devotion.as_ref().map_or(0, |d| d.restored.len()),
        devotion.as_ref().map_or(0, |d| d.discovered.len()),
    ];
    let portal_sig = portal.0.portal.as_ref().map(|p| (p.id, p.area_tag.clone()));
    let sig = (
        map.clone(),
        known,
        index.is_some() && players.single().is_ok(),
        size,
        assets.scale(size).to_bits(),
        portal_sig,
        (view.centred, shown),
    );
    if size.x == 0 || !built.changed(sig) {
        return;
    }
    gd_ui::window::despawn_all(&mut commands, &roots);
    view.layout = RiftLayout::default();
    // An icon index does not outlive its layout.
    view.pressed = None;
    let Some(rec) = data.db.get(RECORD).filter(|_| shown) else {
        return;
    };
    let s = assets.scale(size);
    let root = gd_ui_kit::spawn_root(&mut commands, RiftRoot, gd_ui::window::Z_WINDOW);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        s,
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let field = |k: &str| rec.str(k).unwrap_or("").to_owned();
    let wr = WindowDef::from_record(rec).screen_rect(ui.screen.x, ui.screen.y, s);
    let num = |k: &str| rec.f32(k).unwrap_or(0.0);
    let m = URect::new(
        wr.x + ui.sc(num("mapPositionX")),
        wr.y + ui.sc(num("mapPositionY")),
        ui.sc(num("mapSizeX")),
        ui.sc(num("mapSizeY")),
    );
    // Sections and their bounds.
    let mut sections = Vec::new();
    let (mut min, mut max) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
    for path in rec.strs("mapSections") {
        let Some(sr) = data.db.get(path) else {
            continue;
        };
        let Some((img, sz)) = sr.str("texture").and_then(|t| ui.image(t)) else {
            continue;
        };
        let p = Vec2::new(
            sr.f32("xPosition").unwrap_or(0.0),
            sr.f32("yPosition").unwrap_or(0.0),
        ) * s;
        let wh = sz.as_vec2() * s;
        min = min.min(p);
        max = max.max(p + wh);
        sections.push((img, p, wh));
    }
    if sections.is_empty() {
        return;
    }
    view.layout.map = m;
    view.layout.bounds = (min, max - min);
    let portal_icon = portal.0.portal.as_ref().and_then(|p| {
        let zone = gd_game_core::levels::region_zone(&portal.1.as_deref()?.map, p.pos)?;
        let area = p
            .area_tag
            .as_deref()
            .map(|t| gd_formats::text::strip_markup(ui.fonts.tag(t)))
            .unwrap_or_default();
        Some(PortalIcon {
            id: p.id,
            zone,
            name: format!("{} - {area}", portal.2.name),
        })
    });
    let icons = icons(
        &data,
        ui.fonts,
        &map,
        travel.as_deref(),
        devotion.as_deref(),
        index.as_deref(),
        portal_icon.as_ref(),
    );
    if !view.centred {
        // Centre on the zone of the discovered gate nearest to the player, else the last used one.
        let player = players.single().ok().map(|t| t.translation);
        let gate_pos = |u: &Uid| index.as_ref().and_then(|i| i.0.get(u)).map(|p| p.pos);
        let nearest = icons
            .iter()
            .filter_map(|i| {
                if let IconKind::Gate(u) = i.kind {
                    Some((i, gate_pos(&u)?))
                } else {
                    None
                }
            })
            .min_by(|a, b| {
                player
                    .map_or(0.0, |p| a.1.distance_squared(p))
                    .total_cmp(&player.map_or(0.0, |p| b.1.distance_squared(p)))
            })
            .map(|(i, _)| i);
        let last = travel
            .as_ref()
            .and_then(|t| t.last_used)
            .and_then(|u| icons.iter().find(|i| i.kind == IconKind::Gate(u)));
        let target = nearest.or(last).map(|i| i.zone);
        view.pan = target.map_or(Vec2::ZERO, |t| t * s - (min + (max - min) * 0.5));
        // Before the player exists (the map opened at start) try again later.
        view.centred = target.is_some() || player.is_some();
        view.clamp();
    }
    // The map view (clipped), the content inside it.
    let clip = ui
        .commands
        .spawn(Node {
            overflow: Overflow::clip(),
            ..ui.node(m)
        })
        .id();
    ui.commands.entity(root).add_child(clip);
    let o = view.content_origin();
    let content = ui
        .commands
        .spawn((
            RiftContent,
            ui.node(URect::new(o.x, o.y, max.x - min.x, max.y - min.y)),
        ))
        .id();
    ui.commands.entity(clip).add_child(content);
    ui.root = content;
    for (img, p, wh) in sections {
        let p = p - min;
        ui.quad_stretch(img, URect::new(p.x, p.y, wh.x, wh.y), Color::WHITE);
    }
    let (primary, secondary) = (field("labelPrimaryStyle"), field("labelSecondaryStyle"));
    for i in 0..150 {
        let Some(tag) = rec.str(&format!("labelTag{i}")).filter(|t| !t.is_empty()) else {
            break;
        };
        let style = if rec.str(&format!("labelStyle{i}")) == Some("Primary") {
            &primary
        } else {
            &secondary
        };
        let p = Vec2::new(num(&format!("labelX{i}")), num(&format!("labelY{i}"))) * s - min;
        let step = (ui.line_height(style) * 0.75).trunc();
        let text = ui.fonts.tag(tag).to_owned();
        for (k, line) in label_lines(&text).iter().enumerate() {
            centred(
                &mut ui,
                style,
                Vec2::new(p.x.trunc(), p.y.trunc() + k as f32 * step),
                line,
            );
        }
    }
    for icon in icons {
        let Some((img, sz)) = ui.image(&icon.images[icon.current as usize]) else {
            continue;
        };
        let (w, h) = (ui.sc(sz.x as f32), ui.sc(sz.y as f32));
        let c = icon.at * s - min;
        let r = URect::new((c.x - w * 0.5).round(), (c.y - h * 0.5).round(), w, h);
        let base = ui.quad(img, r, None, Color::WHITE);
        // The hover bitmap, shown instead by `live` (the index is the icon's place in this layout).
        let i = view.layout.icons.len();
        if icon.kind != IconKind::Undiscovered
            && let Some((img, sz)) = ui.image(&icon.images[2])
        {
            let (w, h) = (ui.sc(sz.x as f32), ui.sc(sz.y as f32));
            let hr = URect::new((c.x - w * 0.5).round(), (c.y - h * 0.5).round(), w, h);
            let over = ui.quad(img, hr, None, Color::WHITE);
            swap(&mut ui, base, over, Hover::Icon(i));
        }
        view.layout.icons.push((r, icon));
    }
    // Frame, title, close button.
    ui.root = root;
    if let Some((img, sz)) = ui.image(&field("backgroundImage")) {
        ui.quad_stretch(
            img,
            URect::new(wr.x, wr.y, ui.sc(sz.x as f32), ui.sc(sz.y as f32)),
            Color::WHITE,
        );
    }
    // The `destinationTitle` text is `live`'s (the hovered icon's name).
    if let Some(b) = ui.record(&field("closeButton"))
        && let Some(up) = b.str("bitmapNameUp")
        && let Some((base, r)) = ui.bitmap(
            up,
            b.f32("bitmapPositionX").unwrap_or(0.0),
            b.f32("bitmapPositionY").unwrap_or(0.0),
            Vec2::new(wr.x, wr.y),
        )
    {
        // `ButtonStatic` draw (`ButtonStatic::Render`): `bitmapNameInFocus` while hovered.
        if let Some((img, _)) = b.str("bitmapNameInFocus").and_then(|p| ui.image(p)) {
            let over = ui.quad(img, r, None, Color::WHITE);
            swap(&mut ui, base, over, Hover::Button(0));
        }
        view.layout.close = Some(r);
        ui.record_button_sound(r, &b);
    }
    // The map window's own buttons over the riftgate map (`UIWorldMap::Draw`):
    // `centerButton`, then `switchMapButton` labelled "World Map" while the
    // riftgate map shows (`UIWorldMap::Open`), then its close button (the same
    // record as the map's, drawn above).
    if let Some(w) = data.db.get(WINDOW) {
        let o = Vec2::new(wr.x, wr.y);
        for (k, label) in [("centerButton", false), ("switchMapButton", true)] {
            let Some(b) = w.str(k).and_then(|p| data.db.get(p)) else {
                continue;
            };
            let Some((img, sz)) = b.str("bitmapNameUp").and_then(|p| ui.image(p)) else {
                continue;
            };
            let r = URect::new(
                o.x + ui.sc(b.f32("bitmapPositionX").unwrap_or(0.0)),
                o.y + ui.sc(b.f32("bitmapPositionY").unwrap_or(0.0)),
                ui.sc(sz.x as f32),
                ui.sc(sz.y as f32),
            );
            let base = ui.quad(img, r, None, Color::WHITE);
            if let Some((img, _)) = b.str("bitmapNameInFocus").and_then(|p| ui.image(p)) {
                let over = ui.quad(img, r, None, Color::WHITE);
                swap(&mut ui, base, over, Hover::Button(1 + label as usize));
            }
            ui.record_button_sound(r, b);
            if label && let Some(style) = b.str("style") {
                let text = ui.fonts.tag("tagWorldMapWindowName").to_owned();
                super::widgets::label(&mut ui, r, style, TextAlign::Center, &text);
            } else if !label {
                view.layout.center = Some(r);
            }
            if label {
                view.layout.switch = Some(r);
            }
        }
    }
    view.layout.window = Some(wr);
}

/// What the mouse can be over: an icon (its index in the layout) or a window button ([`RiftMapView::button_at`]).
#[derive(Clone, Copy, PartialEq)]
enum Hover {
    Icon(usize),
    Button(usize),
}

/// A bitmap shown while its [`Hover`] is under the mouse (`true`) or while it is not (`false`).
#[derive(Component)]
struct Swap(Hover, bool);

/// The hovered icon's name (`destinationTitle`).
#[derive(Component)]
struct RiftTitle;

/// `base` shows while `h` is not hovered, `over` (hidden until then) while it is.
fn swap(ui: &mut Ui, base: Entity, over: Entity, h: Hover) {
    ui.commands.entity(base).insert(Swap(h, false));
    ui.commands
        .entity(over)
        .insert((Swap(h, true), Visibility::Hidden));
}

/// Per frame, without a rebuild: the hover bitmap of the icon or button under the mouse and the hovered icon's
/// name (one text, replaced when the hovered icon changes).
#[allow(clippy::too_many_arguments)]
fn live(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    view: Res<RiftMapView>,
    roots: Query<Entity, With<RiftRoot>>,
    titles: Query<Entity, With<RiftTitle>>,
    mut swaps: Query<(&Swap, &mut Visibility)>,
    mut assets: gd_ui_kit::UiAssets,
    mut titled: Local<Option<(Entity, usize)>>,
) {
    let (Ok(win), Some(wr), Ok(root)) = (window.single(), view.layout.window, roots.single())
    else {
        *titled = None;
        return;
    };
    let cursor = super::cursor_px(&assets.hooks, win);
    let hover = cursor.and_then(|p| view.icon_at(p));
    let bhover = cursor.and_then(|p| view.button_at(p));
    for (s, mut vis) in &mut swaps {
        let on = match s.0 {
            Hover::Icon(i) => hover == Some(i),
            Hover::Button(b) => bhover == Some(b),
        };
        vis.set_if_neq(if on == s.1 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    let key = hover.map(|i| (root, i));
    if *titled == key {
        return;
    }
    *titled = key;
    for e in &titles {
        commands.entity(e).try_despawn();
    }
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if let Some((_, icon)) = hover.and_then(|i| view.layout.icons.get(i))
        && let Some(t) = data
            .db
            .get(RECORD)
            .and_then(|r| data.db.get(r.str("destinationTitle")?))
    {
        let mut ui = assets.ui(
            &mut commands,
            &data,
            root,
            assets.scale(size),
            1.0 / win.scale_factor(),
            size.as_vec2(),
        );
        if let Some(e) = ui.text(&TextBox::from_record(t), Vec2::new(wr.x, wr.y), &icon.name) {
            ui.commands.entity(e).insert(RiftTitle);
        }
    }
}


/// Keeps the content node at the current pan (no rebuild while dragging).
fn pan_content(
    view: Res<RiftMapView>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut q: Query<&mut Node, With<RiftContent>>,
) {
    let Ok(win) = window.single() else { return };
    let o = view.content_origin() / win.scale_factor();
    for mut n in &mut q {
        let (l, t) = (Val::Px(o.x), Val::Px(o.y));
        if n.left != l || n.top != t {
            n.left = l;
            n.top = t;
        }
    }
}

/// Mouse and keys on the window (module doc); the window takes the mouse
/// (no walking). `GD_RIFT_PICK` (hooks.rs) once asks to travel to the
/// n-th gate icon (1-based, the gate in use left out).
#[allow(clippy::too_many_arguments)]
pub fn clicks(
    hooks: Res<gd_game_core::hooks::Hooks>,
    buttons: Res<ButtonInput<MouseButton>>,
    (keys, bindings): (Res<ButtonInput<KeyCode>>, Res<super::keys::KeyBindings>),
    mut wheel: MessageReader<MouseWheel>,
    window: Query<&Window, With<PrimaryWindow>>,
    map: Res<RiftgateMap>,
    mut view: ResMut<RiftMapView>,
    mut mapwin: ResMut<gd_ui::state::map::MapWindow>,
    mut capture: ResMut<UiCapture>,
    mut out: MessageWriter<RiftgateRequest>,
    mut picked: Local<bool>,
    pause: Res<gd_game_core::pause::GamePause>,
) {
    let Some(wr) = view.layout.window.filter(|_| mapwin.world_shown(&map)) else {
        if mapwin.world_shown(&map) {
            wheel.clear();
        }
        return;
    };
    // Closing: the riftgate's map through the rules, the `M` window here.
    let close = |mapwin: &mut gd_ui::state::map::MapWindow,
                 out: &mut MessageWriter<RiftgateRequest>| {
        mapwin.open = false;
        if map.open {
            out.write(RiftgateRequest::Close);
        }
    };
    if !*picked && let Some(n) = hooks.ui.rift_pick {
        let gates: Vec<Rift> = view
            .layout
            .icons
            .iter()
            .filter_map(|(_, i)| {
                if let IconKind::Gate(u) = i.kind {
                    Some(Rift::Gate(u))
                } else {
                    None
                }
            })
            .filter(|u| map.gate != Some(*u))
            .collect();
        if let Some(u) = n.checked_sub(1).and_then(|i| gates.get(i)) {
            info!("riftmap test: travelling to gate {n}");
            out.write(RiftgateRequest::Travel(*u));
            *picked = true;
        }
    }
    // Esc closes the map (and the riftgate map with it) through the shared dispatcher
    // (`ui::esc_dispatcher`).
    if pause.menu {
        // Modal: the game menu / Options takes the wheel, keys and presses.
        wheel.clear();
        view.drag = None;
        view.pressed = None;
        return;
    }
    // Center Map presses the centre button.
    if super::localmap::center_map_key(&keys, &bindings) {
        view.centred = false;
    }
    let p = window
        .single()
        .ok()
        .and_then(|win| super::cursor_px(&hooks, win));
    if let (Some(p), Some(last)) = (p, view.drag) {
        view.pan -= p - last;
        view.clamp();
        view.drag = Some(p);
    }
    if buttons.just_released(MouseButton::Left) {
        if let (Some(i), Some(h)) = (view.pressed, p.and_then(|p| view.icon_at(p)))
            && i == h
            && let Some(r) = view.layout.icons[i].1.rift()
            && map.open
        {
            out.write(RiftgateRequest::Travel(r));
        }
        view.pressed = None;
        view.drag = None;
    }
    if buttons.just_released(MouseButton::Right) {
        view.drag = None;
    }
    // A release missed outside the game window still ends the drag / press.
    if !buttons.pressed(MouseButton::Left) {
        view.pressed = None;
        if !buttons.pressed(MouseButton::Right) {
            view.drag = None;
        }
    }
    // The wheel pans only over the window.
    let Some(p) = p.filter(|p| contains(&wr, *p)) else {
        wheel.clear();
        return;
    };
    capture.0 = true;
    for w in wheel.read() {
        let k = match w.unit {
            MouseScrollUnit::Line => 12.5,
            MouseScrollUnit::Pixel => 12.5 / 20.0,
        };
        view.pan.x += w.x * k;
        view.pan.y -= w.y * k;
        view.clamp();
    }
    if buttons.just_pressed(MouseButton::Left) {
        if view.layout.close.is_some_and(|c| contains(&c, p)) {
            close(&mut mapwin, &mut out);
        } else if view.layout.switch.is_some_and(|c| contains(&c, p)) {
            mapwin.switch(&map);
        } else if view.layout.center.is_some_and(|c| contains(&c, p)) {
            // Centres as on opening (`UIWorldMap::OnButtonClick` → `RiftgateMap::CenterOnStart`).
            view.centred = false;
        } else if let Some(i) = view
            .icon_at(p)
            .filter(|i| view.layout.icons[*i].1.rift().is_some())
        {
            view.pressed = Some(i);
        } else if contains(&view.layout.map, p) {
            view.drag = Some(p);
        }
    } else if buttons.just_pressed(MouseButton::Right) && contains(&view.layout.map, p) {
        view.drag = Some(p);
    }
}

/// A left click on the HUD teleport button asks for a personal portal (`gd_game::riftgates::OpenPortal`; the rules
/// decide). PORT: on press. Nothing while the game menu / options window is up: the button stays drawn then but is
/// disabled (`ui::view`, `UIPlayerHud::Update`). The Personal Riftgate key (action 33, default L) presses the button
/// shown: `Grim Dawn.exe` HUD key action `UIPlayerHud::HandleKeyAction` case 33 (reached when no window takes the key):
/// the shown one of `hudTeleportButton` / `hudActiveTeleportButton` gets a press through the HUD's button manager,
/// nothing when neither shows. The button's rect is looked up when the HUD's layout inputs change (button shown, window
/// size, UI scale, HUD layout), not per frame.
#[allow(clippy::too_many_arguments)]
fn teleport_button(
    mut commands: Commands,
    (keys, bindings, dialog, rift_map): (
        Res<ButtonInput<KeyCode>>,
        Res<super::keys::KeyBindings>,
        Res<gd_game::conversation::Conversation>,
        Res<gd_game::riftgates::RiftgateMap>,
    ),
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    values: Res<gd_ui::HudValues>,
    mut capture: ResMut<UiCapture>,
    mut assets: gd_ui_kit::UiAssets,
    mut out: MessageWriter<gd_game::riftgates::OpenPortal>,
    mut rect: Local<(
        Rebuild<(gd_game::riftgates::PortalButton, UVec2, u32, u32, bool)>,
        Option<URect>,
    )>,
    pause: Res<gd_game_core::pause::GamePause>,
) {
    if pause.menu {
        return;
    }
    if values.teleport.is_some()
        && bindings.just_pressed(super::keys::PERSONAL_RIFTGATE, &keys)
        && dialog.view.is_none()
        && !rift_map.open
    {
        info!("riftmap: personal riftgate key");
        out.write(gd_game::riftgates::OpenPortal);
        return;
    }
    let (Some(t), Ok(win)) = (values.teleport, window.single()) else {
        return;
    };
    let Some(p) = super::cursor_px(&assets.hooks, win) else {
        return;
    };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if size.x == 0 || size.y == 0 {
        return;
    }
    let s = assets.scale(size);
    let inv = 1.0 / win.scale_factor();
    if rect
        .0
        .changed((t, size, s.to_bits(), inv.to_bits(), assets.classic_hud()))
    {
        let mut probe = assets.ui(
            &mut commands,
            &data,
            Entity::PLACEHOLDER,
            s,
            inv,
            size.as_vec2(),
        );
        rect.1 = gd_ui::hud::teleport_rect(&mut probe, t);
        // Nothing found (a record or bitmap is missing): asked again the next frame.
        if rect.1.is_none() {
            rect.0.reset();
        }
    }
    if !rect.1.is_some_and(|r| contains(&r, p)) {
        return;
    }
    capture.0 = true;
    if buttons.just_pressed(MouseButton::Left) {
        info!("riftmap: teleport button pressed");
        out.write(gd_game::riftgates::OpenPortal);
    }
}
