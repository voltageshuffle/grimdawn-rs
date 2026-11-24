//! The HUD minimap and the local (aerial) map of the map window, one engine
//! widget (`Grim Dawn.exe` detail-map widget: constructor `DetailMapWidget::DetailMapWidget`, update
//! `DetailMapWidget::Update`, map pass `DetailMapWidget::DrawMapPass`, draw `DetailMapWidget::Render`, minimap icons
//! `DetailMapWidget::RenderMinimapIcons`, local-map icons `DetailMapWidget::RenderLocalMapIcons`, icon names
//! `DetailMapWidget::GetIconName`; window `records/ui/mapaerial/aerialmapconfiguration.dbr`, loader
//! `LocalMap::Load`, input `LocalMap::HandleInput` / `LocalMap::OnLegendButton`):
//! - the map is a top-down orthographic view of the regions' minimap images
//!   (`gd_formats::map::minimap`): centred on the camera's target (+ the
//!   local map's pan), screen up = the camera's horizontal forward (fixed to
//!   world (−1, 0, −1)/√2 on both maps by the option `lockMapRotation`), view height
//!   `zoom × 3` m over the widget's height. Minimap zoom 20 (30 in HUD
//!   mode 4); local map zoom the option `mapZoom` (factory default 0.5, clamped
//!   to 40..135 on read), ±30 per
//!   zoom button, ±10 per `+` / `−` key, eased toward the target at 0.06 m per
//!   ms. Regions drawn: those in the view connected to the player's (stock
//!   regions never overlap in x / z, so PORT: every region in the view);
//! - fog of war ([`gd_sim::minimap_raster`] module doc): the fog pass
//!   (`DetailMapWidget::DrawFogPass`) darkens unexplored ground (saved fog; a region never
//!   visited is black) on both, except in the running Shattered Realm
//!   dungeon's regions; the local map's map pass also greys the ground not
//!   explored this session (session fog; the minimap's mask is
//!   `invisible.tex`), so in the dungeon the local map shows unexplored
//!   ground greyed and the minimap shows it whole. Local-map icons hide in fog there too (`World::IsInFog`,
//!   no dungeon test), and the fog keeps being revealed and saved there
//!   (`Player::UpdateSelf` calls `World::AddFOWVisibility` unconditionally);
//! - the widget keeps one render texture (`DetailMapWidget.renderTexture`), created once at the widget's
//!   size and redrawn by the map pass on every draw (`DetailMapWidget::Render`): moving
//!   the player rebuilds nothing;
//! - the minimap's render is composited through
//!   `system/textures/circlemask.tex` (`shaders/basictexturemasked.ssh`
//!   style `normal`: colour = map rgb, alpha = mask alpha × the widget's
//!   alpha `DetailMapWidget.mapAlpha`, blended src-alpha over the scene; `DetailMapWidget::Render`). That alpha
//!   is 1 (constructor) except in the floating compass modes 3 / 4, where
//!   `UIPlayerHud::ToggleCompass` sets it to the HUD table's
//!   `floatingMapTransparency` (stock 0.65) and `DetailMapWidget.markerAlpha` to
//!   `floatingMarkerTransparency` (stock 1); the minimap's icons
//!   (`DetailMapWidget::RenderMinimapIcons`) are drawn in the map's alpha, its north marker in the
//!   marker's ([`map_alphas`]); the local map's mouse (`LocalMap::HandleMouseEvent`):
//!   the wheel zooms (target − delta / 30), a left drag in the pane pans
//!   (px × zoom × 3 / the pane's width / height, along the camera's axes,
//!   added on release or leaving the window), the centre button resets the
//!   pan (the gamepad's sticks pan 12.5 px per unit and its triggers zoom
//!   ±10, `LocalMap::HandleInput`);
//! - icons (`MinimapGameNugget`s from `GameEngine::GetDetailMapData`, the
//!   entities' `AppendDetailMapData`): the main player (type 0,
//!   `mapsymbol_herodirectionalover.tex` on the minimap, `..._herodirectional`
//!   on the local map) oriented toward its facing, NPCs whose record has
//!   `IncludeInMap` (2, `mapsymbol_npc`), riftgates (3,
//!   `ui/riftgatemap/icon_riftgatedown.tex`), named respawn points (4), merchants and
//!   the Runekeeper (7, vendor), the spirit guide (10, `NpcSkillReallocator`,
//!   witch), the inventor (11, `NpcEnchanter`), caravan drivers (13), smiths
//!   (15, `NpcCrafter`), devotion shrines (16, `StaticShrine`), illusionists
//!   (18, `NpcTransmuter`); a quest marker (8, `mapsymbol_mission.tex`, the
//!   star) instead of the entity's own icon while one of the record's
//!   `questFile<n>` / `taskUid<n>` tasks is in progress (within `markerRange`
//!   of the player when set; Game.dll `QuestMarkerArea::Test`). Minimap icons are
//!   16 × s px (boss 22) centred on their point; outside the circle (radius
//!   − 2) they are pinned to its edge, except NPCs (2), food (5) and Custom
//!   (14), which are dropped. Local-map icons are 24 × s px and hidden in fog;
//!   the hero, party members, NPCs and Custom quads turn with their facing,
//!   the rest stand upright (`DetailMapWidget::RenderMinimapIcons` / `DetailMapWidget::RenderLocalMapIcons`);
//! - more nuggets (Game.dll `AppendDetailMapData`s; each tries the quest
//!   marker first): any Actor's `mapNuggetType` / `mapNuggetCustom` (Engine.dll
//!   `Actor::Load` / `Actor::AppendDetailMapData`: "Boss" 17 on
//!   nemeses and the Dread, "Custom" 14 = the bitmap at 0.75 as a quad turned
//!   with the entity, "Regular" 19 = the bitmap as an icon), used by
//!   monsters (`Monster::AppendDetailMapData`, campaign: alive, not hiding; in the running Shattered Realm
//!   dungeon, once its active floor is the boss floor or 80 % done, a star (8) / boss icon (17) on each live
//!   foe instead),
//!   unbroken `Destructible`s, closed chests and static scenery (dynamic
//!   obstacles); `AreaOfInterest` (6, `AreaDescription`; `questOnly` ones
//!   only as quest markers); ground items (19 with gameiteminfo's
//!   `<rarity>ItemMapNugget` when `Item::PassLootFilter` passes); monster
//!   totems (`MonsterShrine`, 19 with their bitmap until cleared; it and
//!   `StaticShrine` skip `hideMapIcon`); the main player's grave (12,
//!   `tagGrave01`, `GameEngine::GetDetailMapData`, which adds party
//!   members (1) only for other players: multiplayer). Pets have no
//!   nugget of their own (no `Pet::AppendDetailMapData`: `Monster`'s, i.e.
//!   the record's `mapNuggetType`, which none of the 1413 stock `Pet`
//!   records sets), so the player's pets don't show;
//! - a quest marker (`QuestMarkerArea::Test`) counts a task in progress when
//!   `Quest2Task::InProgress` (state 2, not MP-blocked); with the record's
//!   `areaBitmap` it pushes a Custom quad first (under the star): scale 1,
//!   turned by `areaRotation` degrees when `areaRotate` is set, else with the
//!   entity, moved `areaOffsetX` px along its facing and `areaOffsetY`
//!   across it (× widget width / 120 on the minimap, × 0.3125 / zoom × 460
//!   on the local map); no stock record sets `areaBitmap`. Done tasks drop
//!   both (the test runs every frame). A Custom quad is
//!   texture px × scale × widget width / 120 on the minimap (under the other
//!   icons) and texture px × scale × 460 × s × 0.3125 / zoom on the local map;
//! - the minimap's north marker (`ui/mapaerial/mapsymbol_north.tex`) sits on
//!   the circle in the direction of world (−1, 0, −1)/√2, turned that way;
//! - the icon under the mouse (within half its width of its centre, the
//!   last drawn first) shows its name (`style_defaultwhite_sizen`,
//!   `DetailMapWidget::GetIconName`): on the minimap left of the icon, on the local map 3 px
//!   right of it, 5 px below its top; no background (the widget's background
//!   pointer, `DetailMapWidget.background`, is never set);
//! - legend buttons (`ButtonStatic`, round): `bitmapNameInFocus` under the
//!   mouse, their `iconbutton_<kind>rollover` (`ui/rollover.rs`); a click
//!   makes that type's icons blink for 1000 ms on the local map
//!   (`LocalMap::OnLegendButton`: their `...over` bitmap, grown by ms left / 1000 × 20 × s);
//! - the local map's window: `backgroundImage` over the map pane
//!   (`MapPaneX/Y/Width/Height`), the area name
//!   (`map_regionnamedisplayaerial.dbr`), the legend buttons and labels
//!   (`iconbutton_<kind>.dbr` / `iconbutton_<kind>text.dbr`), the zoom buttons
//!   (`map_buttonzoomin/out.dbr`), then the map window's centre, switch
//!   ("Local Map", `tagMapWindowName`) and close buttons. The switch toggles
//!   to the world map (`riftmap.rs`) and back (`UIWorldMap::OnButtonClick`); `M` (port key)
//!   opens the window on the page shown last. Every button plays its record's
//!   `soundNameDown` on press (the shared `ButtonSound`, `ui/widgets.rs`).
//!
//! PORT: the minimap is composited on the CPU ([`gd_sim::minimap_raster`])
//! with an analytic disc for `circlemask.tex`; NPCs without a story object count as visible; party members'
//! nuggets are not drawn (multiplayer); the gamepad's pan / zoom is not
//! ported (no gamepad input in the port); a Custom
//! quad on the minimap shows whole while its centre is inside the circle
//! (the engine masks it); story-scripted scenery has no nugget; the wheel's
//! delta is taken as 120 per notch; it hides while the
//! devotion window is open (reference shot; the rule is not decompiled); the
//! zoom ease and the legend blink run on real time (they keep animating while
//! the game is paused).
mod gather;
mod input;
mod local;
mod minimap;

use super::Ui;
use gd_ui::state::map::MapWindow;
use super::text::FontCache;
use gd_ui::state::inventory::UiCapture;
use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;
use gather::*;
pub use gather::{MapIcon, MapPoint, MapSources};
use gd_formats::map::minimap::MinimapImage;
use gd_formats::ui::{Rect as URect, TextBox, WindowDef};
use gd_game::riftgates::{RiftgateMap, RiftgateRequest, WaypointKind};
use gd_game_core::data::GameData;
use gd_sim::minimap_raster::{FogMask, RegionDraw, RegionFog, View, draw_regions};
pub(super) use input::center_map_key;
use input::*;
use local::*;
use minimap::*;
use std::sync::{Arc, Weak};

pub const AERIAL: &str = "records/ui/mapaerial/aerialmapconfiguration.dbr";
const AREA_TITLE: &str = "records/ui/mapaerial/map_regionnamedisplayaerial.dbr";
const NAME_STYLE: &str = "records/ui/styles/text/style_defaultwhite_sizen.dbr";

/// The minimap's (map and icons, north marker) alphas in HUD modes 3 / 4: the HUD table's `floatingMapTransparency` /
/// `floatingMarkerTransparency` (`UIPlayerHud::ToggleCompass`, loaded by `UIPlayerHud::Load`; module
/// doc), 1 when the field is missing.
fn map_alphas(data: &GameData, options: &gd_game_core::options::GameOptions) -> (f32, f32) {
    let rec = data
        .db
        .get(gd_ui_kit::master(gd_ui_kit::classic(Some(options))));
    let f = |k: &str| rec.as_ref().and_then(|r| r.f32(k)).unwrap_or(1.0);
    (
        f("floatingMapTransparency"),
        f("floatingMarkerTransparency"),
    )
}
/// PORT: how far past the minimap's view icons are gathered (the engine
/// inflates the view frustum by `Engine::GetLevelLoadInflation`).
const GATHER_MARGIN: f32 = 10.0;
/// The nugget type each legend button blinks (`LocalMap::OnLegendButton`).
const LEGEND_KIND: [u8; 12] = [0, 8, 3, 2, 16, 18, 6, 13, 11, 7, 10, 15];

pub struct LocalMapPlugin;

impl Plugin for LocalMapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MapWindow>()
            .init_resource::<MapTiles>()
            .add_systems(
                Update,
                (
                    keys.in_set(gd_ui_kit::UiPlaySet),
                    minimap.in_set(gd_ui_kit::UiPlaySet),
                    local_map.in_set(gd_ui_kit::UiPlaySet),
                    clicks.in_set(gd_ui_kit::UiInputSet),
                )
                    .chain(),
            )
            .add_systems(PreStartup, start_window)
            .add_systems(Update, persist_zoom);
    }
}

/// `GD_UI_OPEN=localmap` / `worldmap` (hooks.rs): the map window at start; `GD_MAP_ZOOM` is the local map's
/// zoom (the option `mapZoom`).
fn start_window(mut commands: Commands, hooks: Res<gd_game_core::hooks::Hooks>) {
    let zoom = hooks.ui.map_zoom.map_or(65.0, |z| z.clamp(40.0, 135.0));

    let (open, local) = match hooks.ui.open.as_deref() {
        Some("localmap") => (true, true),
        Some("worldmap") => (true, false),
        _ => (false, true),
    };
    commands.insert_resource(MapWindow {
        open,
        local,
        zoom,
        zoom_to: zoom,
        ..default()
    });
}

/// The local map's zoom is the option `mapZoom`: the window starts at the stored value (unless
/// `GD_MAP_ZOOM` is set), again in every session (Exit to Main Menu puts `MapWindow::default()` back, whose 65 is
/// not a zoom the player chose), and a change is written back to the port's options file once the zoom has stopped
/// changing for a second (PORT: the engine saves the options on exit; the port has no exit hook for that).
fn persist_zoom(
    mut win: ResMut<MapWindow>,
    mut options: ResMut<gd_game_core::options::GameOptions>,
    hooks: Res<gd_game_core::hooks::Hooks>,
    time: Res<Time<Real>>,
    mut fresh: gd_game_core::session::NewSession,
    // (the stored zoom is loaded into this session's window, seconds the zoom has differed from the stored one)
    mut state: Local<(bool, f32)>,
) {
    if fresh.begun() {
        *state = default();
    }
    if !state.0 {
        state.0 = true;
        if hooks.ui.map_zoom.is_none() {
            let z = options.float("mapZoom").clamp(40.0, 135.0);
            win.zoom = z;
            win.zoom_to = z;
        }
        return;
    }
    if (win.zoom_to - options.float("mapZoom")).abs() < 0.01 {
        state.1 = 0.0;
        return;
    }
    state.1 += time.delta_secs();
    if state.1 >= 1.0 && hooks.ui.map_zoom.is_none() {
        state.1 = 0.0;
        options.set("mapZoom", gd_formats::options::Value::Float(win.zoom_to));
        gd_ui_kit::state::settings::store(&options);
    }
}





/// Decoded region minimap images and the two render targets. A region's image decodes in the background the first time
/// a map needs it; the maps draw it once it lands ([`MapTiles::landed`]), so entering a new area does not stall a frame
/// on its map tiles. The last [`TILES_KEPT`] images drawn stay decoded ([`MapTiles::kept`], 256 KB each): closing and
/// opening the local map, panning back over ground or zooming does not decode them again, while a long trip or a
/// teleport lets the old ones go. The session's end drops them all (`ui::reset_session`).
#[derive(Resource)]
pub struct MapTiles {
    /// The map the tiles are of (its address): another map starts over.
    map: usize,
    /// Per map region: `None` never decoded, `Some(None)` no image (or unreadable), else the decoded image.
    regions: Vec<Option<Option<Weak<MinimapImage>>>>,
    /// Regions decoding in the background.
    loading: gd_formats::collections::HashMap<usize, bevy::tasks::Task<Option<MinimapImage>>>,
    /// The images drawn (or landed) last, held.
    kept: gd_game_core::weak_cache::Recent<Arc<MinimapImage>>,
    /// The regions whose image landed, the last [`LANDED_LOG`] in order: the `landed - log.len()`th landing onward.
    landed_log: std::collections::VecDeque<usize>,
    /// Landings so far: a map drawn before redraws the landed regions' boxes ([`MapCache::update`]).
    landed: u64,
    mini: Option<(Handle<Image>, UVec2)>,
    local: Option<(Handle<Image>, UVec2)>,
}

impl Default for MapTiles {
    fn default() -> Self {
        Self {
            map: 0,
            regions: Vec::new(),
            loading: Default::default(),
            kept: gd_game_core::weak_cache::Recent::new(TILES_KEPT),
            landed_log: Default::default(),
            landed: 0,
            mini: None,
            local: None,
        }
    }
}

/// Region images [`MapTiles`] keeps decoded (256 KB each: 64 MB).
const TILES_KEPT: usize = 256;
/// Landings [`MapTiles`] remembers by region; a map further behind redraws whole.
const LANDED_LOG: usize = 64;

impl MapTiles {
    /// Files the images decoded in the background.
    fn poll(&mut self) {
        use bevy::tasks::block_on;
        use bevy::tasks::futures_lite::future;
        if self.loading.is_empty() {
            return;
        }
        let done: Vec<(usize, Option<MinimapImage>)> = self
            .loading
            .iter_mut()
            .filter_map(|(i, t)| Some((*i, block_on(future::poll_once(t))?)))
            .collect();
        for (i, img) in done {
            self.loading.remove(&i);
            let Some(slot) = self.regions.get_mut(i) else {
                continue;
            };
            match img {
                Some(img) => {
                    let img = Arc::new(img);
                    *slot = Some(Some(Arc::downgrade(&img)));
                    self.kept.keep(&img);
                }
                None => *slot = Some(None),
            }
            self.landed += 1;
            if self.landed_log.len() == LANDED_LOG {
                self.landed_log.pop_front();
            }
            self.landed_log.push_back(i);
        }
    }

    /// The regions that landed after the `since`th landing, `None` when the log no longer reaches back that far.
    fn landed_since(&self, since: u64) -> Option<impl Iterator<Item = usize> + '_> {
        let first = self.landed - self.landed_log.len() as u64;
        (since >= first).then(|| {
            self.landed_log
                .iter()
                .skip((since - first) as usize)
                .copied()
        })
    }

    /// Region `i`'s image, if decoded; else its decode starts in the background (drawn when it lands).
    fn tile(&mut self, map: &Arc<gd_formats::map::MapFile>, i: usize) -> Option<Arc<MinimapImage>> {
        let id = Arc::as_ptr(map) as usize;
        if self.map != id || self.regions.len() != map.regions.len() {
            self.map = id;
            self.regions = vec![None; map.regions.len()];
            self.loading.clear();
            self.kept.clear();
            self.landed_log.clear();
        }
        match &self.regions[i] {
            Some(None) => return None,
            Some(Some(w)) => {
                if let Some(img) = w.upgrade() {
                    self.kept.keep(&img);
                    return Some(img);
                }
            }
            None => {}
        }
        self.loading.entry(i).or_insert_with(|| {
            let map = map.clone();
            bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                map.minimap_image(i)
                    .inspect_err(|e| warn!("minimap {}: {e:#}", map.regions[i].name))
                    .ok()
                    .flatten()
            })
        });
        None
    }

    fn target(
        slot: &mut Option<(Handle<Image>, UVec2)>,
        images: &mut Assets<Image>,
        size: UVec2,
    ) -> Handle<Image> {
        if let Some((h, s)) = slot
            && *s == size
        {
            return h.clone();
        }
        let img = Image::new_fill(
            Extent3d {
                width: size.x.max(1),
                height: size.y.max(1),
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[0, 0, 0, 0],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        let h = images.add(img);
        *slot = Some((h.clone(), size));
        h
    }
}

/// The view's screen axes in world x / z: right and up (the camera's
/// horizontal right / forward).
#[derive(Clone, Copy, PartialEq)]
struct Axes {
    right: Vec2,
    up: Vec2,
}

fn axes(cam: &Transform) -> Axes {
    let (f, r) = (cam.forward(), cam.right());
    let up = Vec2::new(f.x, f.z)
        .try_normalize()
        .unwrap_or(Vec2::new(-1.0, -1.0).normalize());
    let right = Vec2::new(r.x, r.z)
        .try_normalize()
        .unwrap_or(Vec2::new(-up.y, up.x));
    Axes { right, up }
}

/// The axes of both maps: `lockMapRotation` (an `Options` bool, "Force the map to always point north") fixes them to
/// the default 45 degree camera's basis, else they follow the camera's yaw. `DetailMapWidget::DrawMapPass` (the map
/// pass, run by the widget's draw `DetailMapWidget::Render` for the minimap and the local map alike: it builds the map
/// camera `DetailMapWidget.mapCamera` from the fixed vectors or the live camera's axes) and the icons projected through
/// that camera (`DetailMapWidget::RenderLocalMapIcons`, local map) or drawn with a fixed (0, −1) up
/// (`DetailMapWidget::RenderMinimapIcons`, minimap).
fn map_axes(cam: &Transform, options: &gd_game_core::options::GameOptions) -> Axes {
    if options.flag("lockMapRotation") {
        let up = Vec2::new(-1.0, -1.0).normalize();
        Axes {
            right: Vec2::new(-up.y, up.x),
            up,
        }
    } else {
        axes(cam)
    }
}

/// The ground view of a `w × h` px widget centred on `centre` (x, z) at
/// `mpp` metres per px.
fn view_of(centre: Vec2, a: Axes, mpp: f32, w: f32, h: f32) -> View {
    let dx = a.right * mpp;
    let dy = -a.up * mpp;
    let o = centre - dx * (w * 0.5) - dy * (h * 0.5);
    View {
        origin: [o.x, o.y],
        dx: [dx.x, dx.y],
        dy: [dy.x, dy.y],
    }
}

/// What a map image is drawn from. The fog passes ([`gd_sim::minimap_raster`] module doc,
/// `DetailMapWidget::DrawMapPass` /
/// `DetailMapWidget::DrawFogPass`): the local map greys unexplored ground and the minimap does not; both darken it to
/// black except in regions of the running Shattered Realm dungeon (`in_dungeon`, by map region index); a region without
/// a fog grid is fully fogged (the engine creates it on first use).
struct MapSource<'a> {
    tiles: &'a mut MapTiles,
    map: &'a Arc<gd_formats::map::MapFile>,
    fog: Option<&'a gd_game::fog::FogOfWar>,
    /// The fog resource was replaced since the last frame (its revision counts from 0 again).
    fog_new: bool,
    minimap: bool,
    in_dungeon: &'a dyn Fn(usize) -> bool,
    /// The running Shattered Realm dungeon.
    dungeon: Option<u32>,
}

/// Half the box around the fog's last reveal that a reveal can change on a map (world m): the 30 m reveal range plus
/// the bilinear fog cells' reach and the reveal's cell offset (`gd_formats::save::fow`).
const REVEAL_REACH: f32 = gd_formats::save::fow::VISIBLE_RANGE + 3.0 * gd_formats::save::fow::CELL;
/// The margin a map image is drawn with around its widget's view, per side, in widget sizes.
const MAP_MARGIN: f32 = 0.5;

/// A widget's map image (RGBA8), drawn with a margin around the view (PORT: the engine draws its map pass on the GPU
/// every frame; the CPU raster here draws it whole only when its scale, turn or dungeon changes; a view leaving it
/// moves the image, copying what both cover and drawing only the uncovered strips; a fog reveal or a tile landing
/// redraws only its box). The view scrolls across it in whole pixels.
struct MapCache {
    view: View,
    size: UVec2,
    rgba: Vec<u8>,
    /// The [`gd_game::fog::FogOfWar::revision`] drawn (`None`: no fog).
    fog: Option<u64>,
    dungeon: Option<u32>,
    /// The [`MapTiles::landed`] count it drew with: a tile landing since redraws its box.
    landed: u64,
}

impl MapCache {
    /// Brings `cache` up to date for `view` (`size` px; drawn whole, it gets a margin of `margin` × `size` on each
    /// side): the view's whole-pixel offset in it and whether its pixels changed.
    fn update(
        cache: &mut Option<Self>,
        src: &mut MapSource,
        view: &View,
        size: UVec2,
        margin: f32,
    ) -> (UVec2, bool) {
        src.tiles.poll();
        let landed = src.tiles.landed;
        // The minimap shows only the saved fog: the session fog's reveals (every metre walked) leave it alone.
        let revision = src.fog.map(|f| {
            if src.minimap {
                f.saved_revision
            } else {
                f.revision
            }
        });
        let m = (size.as_vec2() * margin).round().as_uvec2();
        let full = size + 2 * m;
        if let Some(c) = cache.as_mut().filter(|c| {
            !src.fog_new && c.dungeon == src.dungeon && c.view.dx == view.dx && c.view.dy == view.dy
        }) {
            let at = match c.offset(view, size) {
                Some(o) => Some((o, false)),
                None if c.size == full => {
                    c.shift(src, view, m);
                    c.offset(view, size).map(|o| (o, true))
                }
                None => None,
            };
            if let Some((o, shifted)) = at {
                let refreshed = c.refresh(src, revision, landed);
                return (o, shifted || refreshed);
            }
        }
        let mut c = MapCache {
            view: View {
                origin: [
                    view.origin[0] - view.dx[0] * m.x as f32 - view.dy[0] * m.y as f32,
                    view.origin[1] - view.dx[1] * m.x as f32 - view.dy[1] * m.y as f32,
                ],
                ..*view
            },
            size: full,
            rgba: vec![0; (full.x * full.y * 4) as usize],
            fog: revision,
            dungeon: src.dungeon,
            landed,
        };
        c.draw(src, [0, 0, full.x, full.y]);
        *cache = Some(c);
        (m, true)
    }

    /// Redraws what changed since the last draw: one fog reveal's box and the landed tiles' boxes; anything else (more
    /// reveals, a landing log that no longer reaches back) all of it. Whether anything was drawn.
    fn refresh(&mut self, src: &mut MapSource, revision: Option<u64>, landed: u64) -> bool {
        if self.fog == revision && self.landed == landed {
            return false;
        }
        let mut clips = Vec::new();
        let mut all = false;
        if self.fog != revision {
            match (self.fog, revision, src.fog.and_then(|f| f.last)) {
                (Some(a), Some(b), Some(p)) if b == a + 1 => {
                    let r = Vec2::splat(REVEAL_REACH);
                    clips.push(self.clip_box(p - r, p + r));
                }
                _ => all = true,
            }
        }
        if !all && self.landed != landed {
            match src.tiles.landed_since(self.landed) {
                Some(regions) => {
                    for i in regions.collect::<Vec<_>>() {
                        let (a, b) = gd_game::fog::bounds(&src.map.regions[i]);
                        clips.push(self.clip_box(a.xz(), b.xz()));
                    }
                }
                None => all = true,
            }
        }
        self.fog = revision;
        self.landed = landed;
        if all {
            self.draw(src, [0, 0, self.size.x, self.size.y]);
        } else {
            for clip in clips {
                self.draw(src, clip);
            }
        }
        true
    }

    /// Moves the image (same scale and size) so `view` sits `m` px in from its corner: the pixels the old and new
    /// images both cover are copied, the uncovered strips drawn.
    fn shift(&mut self, src: &mut MapSource, view: &View, m: UVec2) {
        let s = self.view.screen(view.origin).map(f32::round);
        let k = IVec2::new(s[0] as i32 - m.x as i32, s[1] as i32 - m.y as i32);
        self.view.origin = self.view.world(k.x as f32, k.y as f32);
        let (w, h) = (self.size.x as i32, self.size.y as i32);
        if k.x.abs() >= w || k.y.abs() >= h {
            self.draw(src, [0, 0, self.size.x, self.size.y]);
            return;
        }
        // New pixel (x, y) is old pixel (x + k.x, y + k.y): rows copied in the order that reads before it writes.
        let stride = (w * 4) as usize;
        let (sx0, sx1) = (k.x.max(0), w.min(w + k.x));
        let (span, dx0) = (((sx1 - sx0) * 4) as usize, ((sx0 - k.x) * 4) as usize);
        let copy = |rgba: &mut Vec<u8>, y: i32| {
            let from = (y + k.y) as usize * stride + (sx0 * 4) as usize;
            rgba.copy_within(from..from + span, y as usize * stride + dx0);
        };
        let (ry0, ry1) = (0.max(-k.y), h.min(h - k.y));
        if k.y >= 0 {
            (ry0..ry1).for_each(|y| copy(&mut self.rgba, y));
        } else {
            (ry0..ry1).rev().for_each(|y| copy(&mut self.rgba, y));
        }
        let (w, h) = (w as u32, h as u32);
        let (ry0, ry1) = (ry0 as u32, ry1 as u32);
        if ry0 > 0 {
            self.draw(src, [0, 0, w, ry0]);
        }
        if ry1 < h {
            self.draw(src, [0, ry1, w, h]);
        }
        if k.x > 0 {
            self.draw(src, [w - k.x as u32, ry0, w, ry1]);
        } else if k.x < 0 {
            self.draw(src, [0, ry0, (-k.x) as u32, ry1]);
        }
    }

    /// The view the widget actually shows when cut at whole-pixel offset `o`: icons placed with it move in the same
    /// whole-pixel steps as the image under them (placed with the exact view they drift up to a pixel against it
    /// as the camera moves).
    fn shown(&self, o: UVec2) -> View {
        View {
            origin: self.view.world(o.x as f32, o.y as f32),
            ..self.view
        }
    }

    /// The whole-pixel offset of `view` (`size` px) in the image, if it lies inside.
    fn offset(&self, view: &View, size: UVec2) -> Option<UVec2> {
        let [x, y] = self.view.screen(view.origin).map(f32::round);
        (x >= 0.0
            && y >= 0.0
            && x + size.x as f32 <= self.size.x as f32
            && y + size.y as f32 <= self.size.y as f32)
            .then(|| UVec2::new(x as u32, y as u32))
    }

    /// The image's pixels (x0, y0, x1, y1) covering the world box `lo`..`hi` (x / z).
    fn clip_box(&self, lo: Vec2, hi: Vec2) -> [u32; 4] {
        let pts = [(lo.x, lo.y), (hi.x, lo.y), (lo.x, hi.y), (hi.x, hi.y)]
            .map(|(x, z)| self.view.screen([x, z]));
        let (mut a, mut b) = (Vec2::INFINITY, Vec2::NEG_INFINITY);
        for [x, y] in pts {
            a = a.min(Vec2::new(x, y));
            b = b.max(Vec2::new(x, y));
        }
        let a = a.floor().max(Vec2::ZERO).min(self.size.as_vec2());
        let b = b.ceil().max(a).min(self.size.as_vec2());
        [a.x as u32, a.y as u32, b.x as u32, b.y as u32]
    }

    /// Draws the regions inside `clip` (px: x0, y0, x1, y1), black where none.
    fn draw(&mut self, src: &mut MapSource, clip: [u32; 4]) {
        let [x0, y0, x1, y1] = clip;
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let corners = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)]
            .map(|(x, y)| self.view.world(x as f32, y as f32));
        let (mut lo, mut hi) = (Vec2::INFINITY, Vec2::NEG_INFINITY);
        for [x, z] in corners {
            lo = lo.min(Vec2::new(x, z));
            hi = hi.max(Vec2::new(x, z));
        }
        let mut found = Vec::new();
        for i in 0..src.map.regions.len() {
            let r = &src.map.regions[i];
            let (a, b) = gd_game::fog::bounds(r);
            if a.x > hi.x || b.x < lo.x || a.z > hi.y || b.z < lo.y || r.minimap_size == 0 {
                continue;
            }
            if let Some(img) = src.tiles.tile(src.map, i) {
                found.push((i, ([a.x, a.z], [b.x, b.z]), img));
            }
        }
        // The saved fog darkens (a region without one is fully fogged: black), the session fog greys the local map
        // (module doc of `gd_sim::minimap_raster`).
        fn mask(g: Option<&gd_formats::save::fow::FogGrid>) -> FogMask<'_> {
            g.map_or(FogMask::FOGGED, |g| FogMask {
                w: g.w,
                h: g.h,
                cells: &g.cells,
            })
        }
        let draws: Vec<RegionDraw> = found
            .iter()
            .map(|(i, bounds, img)| RegionDraw {
                bounds: *bounds,
                img,
                fog: src.fog.map(|f| {
                    let key = src.map.regions[*i].name.to_ascii_lowercase();
                    RegionFog {
                        grey: (!src.minimap).then(|| mask(f.session.get(&key))),
                        darken: (!(src.in_dungeon)(*i)).then(|| mask(f.grids.get(&key))),
                    }
                }),
            })
            .collect();
        draw_regions(
            &mut self.rgba,
            (self.size.x, self.size.y),
            clip,
            &self.view,
            &draws,
        );
    }

    /// The minimap's image: the `side`² px at `o`, its alpha the analytic disc standing in for `circlemask.tex`.
    fn disc(&self, o: UVec2, side: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity((side * side * 4) as usize);
        for y in 0..side {
            let s = (((o.y + y) * self.size.x + o.x) * 4) as usize;
            out.extend_from_slice(&self.rgba[s..s + (side * 4) as usize]);
        }
        let r = side as f32 * 0.5;
        for y in 0..side {
            for x in 0..side {
                let d = Vec2::new(x as f32 + 0.5 - r, y as f32 + 0.5 - r).length();
                out[((y * side + x) * 4 + 3) as usize] = ((r - d).clamp(0.0, 1.0) * 255.0) as u8;
            }
        }
        out
    }
}

/// Draws one icon centred at `c` (px), turned by `angle` (clockwise).
fn icon_node(ui: &mut Ui, img: Handle<Image>, c: Vec2, size: Vec2, angle: f32, alpha: f32) {
    let r = URect::new(
        (c.x - size.x * 0.5).round(),
        (c.y - size.y * 0.5).round(),
        size.x,
        size.y,
    );
    let e = ui.quad_stretch(img, r, Color::WHITE.with_alpha(alpha));
    if angle != 0.0 {
        ui.commands
            .entity(e)
            .insert(UiTransform::from_rotation(Rot2::radians(angle)));
    }
}

/// An icon's bitmap: the nugget's own (Custom, Regular) or its type's
/// (`blink`: its `...over` variant, `DetailMapWidget::RenderLocalMapIcons`).
fn icon_image(
    ui: &mut Ui,
    i: &MapIcon,
    local: bool,
    blink: bool,
) -> Option<(Handle<Image>, UVec2)> {
    match (i.kind, &i.bitmap) {
        (14 | 19, Some(b)) => ui.image(b),
        (14 | 19, None) => None,
        (k, _) => {
            let t = icon_texture(k, local)?;
            // PORT: the riftgate's blink bitmap is assumed to be `icon_riftgateover`.
            let over = if blink {
                Some(
                    t.replace("herodirectional.tex", "herodirectionalover.tex")
                        .replace("icon_riftgatedown", "icon_riftgateover")
                        .replace(".tex", "over.tex")
                        .replace("overover", "over"),
                )
            } else {
                None
            };
            over.and_then(|o| ui.image(&o)).or_else(|| ui.image(t))
        }
    }
}

/// Icons turned by their facing (`DetailMapWidget::RenderMinimapIcons`: the hero, party members,
/// NPCs, and Custom quads); the rest stand upright.
fn turns(kind: u8) -> bool {
    matches!(kind, 0 | 1 | 2 | 14)
}

/// The clockwise angle turning a bitmap's "up" toward screen direction `d`.
fn angle_of(d: Vec2) -> f32 {
    if d.length_squared() < 1e-8 {
        0.0
    } else {
        d.x.atan2(-d.y)
    }
}

type Scene<'w, 's> = (
    Query<'w, 's, (&'static Transform, &'static gd_render::camera::GameCamera)>,
    Query<
        'w,
        's,
        &'static Transform,
        (
            With<gd_game_core::player::Player>,
            Without<gd_render::camera::GameCamera>,
        ),
    >,
    MapSources<'w, 's>,
);
