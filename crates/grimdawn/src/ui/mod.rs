//! In-game UI drawn from the game's own UI records (`gd_formats::ui`).
//!
//! Layout is computed in physical pixels with the engine's rules
//! ([`gd_formats::ui`]): UI scale factor from the resolution, window
//! alignment, `round(pos × scale)` for child positions and `texture size ×
//! scale` for bitmaps. Every widget becomes an absolutely positioned Bevy UI
//! node under one root, spawned in the engine's draw order. The tree is
//! rebuilt when the window size, panel state or another layout input changes;
//! bar fills animate every frame. Overlays whose numbers change every few frames (cooldown wipes, buff sweeps,
//! overhead and hover bars) are built per layout and animate their fills, wipes and texts in place
//! ([`set_node_rect`], [`LiveText`]).
//!
//! Blending: the engine blends UI in gamma space (its UNORM back buffer); so
//! does the port: the UI is drawn into the frame while it holds gamma values
//! and every UI shader writes colours and texels as given (`render/post.rs`,
//! "UI"), so colours, tints and textures' own alpha need no correction. The
//! additive and masked materials (`additive.rs`, `masked.rs`) do the same in
//! their shaders.
pub mod achievements;
pub mod additive;
pub mod altar;
pub mod apply;
pub mod armor_window;
pub mod challenge;
pub mod character;
pub mod cinematic;
pub mod confirm;
pub mod cooldown;
pub mod credits;
pub mod cursor;
pub mod devotion;
pub mod dialog;
pub mod errormsg;
pub mod factions;
pub mod floating;
pub mod gamemenu;
pub mod hotbar_pick;
pub mod hud;
pub mod keys;
pub mod loading;
pub mod localmap;
pub mod lootfilter;
pub mod mainmenu;
pub mod masked;
pub mod menufolder;
pub mod newitems;
pub mod notify;
pub mod options;
pub mod overhead;
pub mod pets;
pub mod potions;
pub mod questpopup;
pub mod quests;
pub mod quicktip;
pub mod reclamation;
pub mod riftmap;
pub mod rollover;
pub mod shrine;
pub mod skills;
pub mod slot_select;
pub mod stack;
pub mod status;
pub mod survival;
pub mod tab_fields;
pub mod text;
pub mod tip;
pub mod trade;
pub mod view;
pub mod widgets;
pub mod window;

use crate::data::GameData;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::{Rect as URect, TextBox, Widget, WindowDef, round_px};
use gd_render::textures::Textures;
use std::sync::Arc;
use text::{FontCache, LoadedFont};

/// Whether the left mouse button was released this frame, from the raw
/// messages. Windows that swallow input (`reset_all` on `ButtonInput`) can't
/// use `just_released`: the reset drops the press from `pressed`, so the
/// release a frame later is never recorded.
pub fn left_released(clicks: &mut MessageReader<bevy::input::mouse::MouseButtonInput>) -> bool {
    // Reads every message (no short circuit), so none carries over a frame.
    clicks
        .read()
        .filter(|e| e.button == MouseButton::Left && e.state == bevy::input::ButtonState::Released)
        .count()
        > 0
}

/// The player state the HUD and windows show: a view assembled each frame by
/// [`view::collect`] from the gameplay state (empty until the player exists), published field by field
/// where it changed (so it is `Changed` only then). The values that change most frames are in [`HudVitals`].
#[derive(Resource, Clone, PartialEq)]
pub struct HudValues {
    /// Fraction of the current level's experience, 0..1.
    pub experience: f32,
    /// The Shattered Realm's floor progress of the player's region, 0..=1 (`Stat::EndlessProgress`).
    pub endless_progress: f32,
    /// Total experience and the total that leaves the level
    /// (`GetExperiencePoints` / `GetNextLevelExperience`), and whether the
    /// level is the last (`IsMaxLevel`): the experience bar's rollover.
    pub experience_points: u64,
    pub experience_next: u64,
    pub max_level: bool,
    pub level: u32,
    pub gold: u64,
    pub name: String,
    /// Character-window numbers by field name without the `tabN` prefix
    /// (e.g. `StrengthNumber`); missing ones show 0.
    pub stats: std::collections::BTreeMap<String, String>,
    /// The stats rows' rollover values by the same keys
    /// (`gd_sim::sheet::numbers_and_rollovers`; `rollover.rs`).
    pub rollovers: std::collections::BTreeMap<String, gd_sim::sheet::RollValues>,
    /// The armour row's breakdown window (`gd_sim::sheet::armor_breakdown`;
    /// `armor_window.rs`).
    pub armor: gd_sim::sheet::ArmorBreakdown,
    /// The left / right mouse skill's damage lines (`gd_game::sheet_dps`
    /// `SheetDps::breakdown`): the page II LMB / RMB rollovers.
    pub skill_breakdown: [Vec<(bool, gd_sim::attributes::CombatType, f32, f32)>; 2],
    pub attribute_points: u32,
    pub skill_points: u32,
    /// The skills window's undo button is enabled (`gd_game::points::SkillUndo`).
    pub skill_undo: bool,
    /// The skills window is in reclamation mode (`reclamation.rs`).
    pub reclaim: Option<reclamation::ReclaimView>,
    /// The devotion window's reclaim mode (`reclamation::DevotionReclaimView`).
    pub devotion_reclaim: Option<reclamation::DevotionReclaimView>,
    /// The devotion window's undo button is enabled (`gd_game::devotion::DevotionUndo`).
    pub devotion_undo: bool,
    /// The player's learned skills (skills window).
    pub skills: gd_sim::skills::SkillBook,
    /// Hotbar icons (bitmap paths) by save slot: 0–9 keys 1–0, 10/11 mouse.
    pub hotbar: Vec<Option<String>>,
    /// Hotbar row shown on the number slots (0, or 1 while Ctrl is held).
    pub hotbar_row: usize,
    /// The hot slots' key labels by HUD master field (the bound keys,
    /// `hud::slot_labels`; a rebind is a layout change).
    pub hotbar_labels: std::collections::BTreeMap<&'static str, String>,
    /// Each living pet's portrait buff icons (bitmaps,
    /// `gd_game::buff_bar::pet_icons`; `pets.rs`).
    pub pet_buffs: Vec<(Entity, Vec<String>)>,
    /// The hovered character's debuff icons (`view::target_strip`; `hover.rs`, `monsterBarsDebuffs`).
    pub target_debuffs: Vec<status::TargetDebuff>,
    /// The HUD teleport button shown (`gd_game::riftgates::portal_button`).
    pub teleport: Option<gd_game::riftgates::PortalButton>,
    /// The HUD weapon-swap button works (`gd_game::weapon_sets`: set II was
    /// chosen once in the character window).
    pub weapon_swap: bool,
    /// The game-mode label's tags (`gd_sim::mode::Mode::label_tags_in`; `""` = unused slot).
    pub mode_tags: [&'static str; 2],
    /// The area name tag at the player (the map's name sector; `hud.rs`).
    pub area_tag: String,
    /// The challenge area the player stands in, when it shows the HUD (`challenge.rs`).
    pub challenge_area: Option<challenge::ChallengeHud>,
    /// The player's faction reputation in save order (`PlayerFactions::list`; the factions window).
    pub factions: Vec<gd_formats::save::gdc::Faction>,
    /// The orb HUD's menu folder (`menufolder.rs`).
    pub menu_folder: menufolder::FolderView,
    /// The HUD's options button (`hud::OptionsButton`).
    pub options_button: hud::OptionsButton,
}

impl Default for HudValues {
    fn default() -> Self {
        Self {
            experience: 0.0,
            endless_progress: 0.0,
            experience_points: 0,
            experience_next: 0,
            max_level: false,
            level: 1,
            gold: 0,
            name: String::new(),
            stats: Default::default(),
            rollovers: Default::default(),
            armor: Default::default(),
            skill_breakdown: Default::default(),
            attribute_points: 0,
            skill_points: 0,
            skill_undo: false,
            reclaim: None,
            devotion_reclaim: None,
            devotion_undo: false,
            skills: Default::default(),
            hotbar: Vec::new(),
            hotbar_row: 0,
            hotbar_labels: Default::default(),
            pet_buffs: Vec::new(),
            target_debuffs: Vec::new(),
            teleport: None,
            weapon_swap: false,
            mode_tags: [""; 2],
            area_tag: String::new(),
            challenge_area: None,
            factions: Vec::new(),
            menu_folder: menufolder::FolderView::default(),
            options_button: hud::OptionsButton::default(),
        }
    }
}

/// The player values that change most frames (life, energy, the hot slots' cooldowns, the buff bar's timers),
/// written in place by [`view::collect`] every frame; their readers animate what they drive in place
/// (`hud::animate_bars`, `hud::orb_texts`, `cooldown::draw`, `status::draw`, the overhead health).
#[derive(Resource, Clone, PartialEq)]
pub struct HudVitals {
    pub health: f32,
    pub health_max: f32,
    pub energy: f32,
    pub energy_max: f32,
    /// Energy reserved by active auras (`Character::GetReserveMana`).
    pub energy_reserved: f32,
    /// Current constitution and its limit (`GetCurrentConstitution` / `GetConstitutionLimit`, the `hudConstitutionBar`).
    pub constitution: f32,
    pub constitution_max: f32,
    /// Status icons above the hotbar (`status.rs`).
    pub status: Vec<status::StatusIcon>,
    /// The shown hot slots' running cooldowns (`cooldown.rs`).
    pub cooldowns: Vec<cooldown::SlotCooldown>,
    /// The value of the player's fixed duration-damage entry of type 61 (`DamageMultiplier`, applied by enemy skills'
    /// `offensiveSlowDamageMult*`; 0: none); nonzero shows the "sundered" overlay (`survival.rs`).
    pub damage_taken_debuff: f32,
    /// The health-gain ("incoming heal") bar's ratio: `(life + PotionOverTime.life) / life_max`
    /// clamped to 0..1 (`Character::GetPotentialLife` @0x18006b1b0; `hud::Stat::HealthGain`).
    pub health_gain: f32,
    /// The shown hot slots' state overlays (`hud::hot_slot_states`; `view::collect` fills them).
    pub hot_slots: Vec<hud::HotSlotState>,
}

impl Default for HudVitals {
    fn default() -> Self {
        Self {
            health: 0.0,
            health_max: 0.0,
            energy: 0.0,
            energy_max: 0.0,
            energy_reserved: 0.0,
            constitution: 0.0,
            constitution_max: 0.0,
            status: Vec::new(),
            cooldowns: Vec::new(),
            damage_taken_debuff: 0.0,
            health_gain: 0.0,
            hot_slots: Vec::new(),
        }
    }
}

/// Which windows are open (`I` / `C` toggle the character window).
#[derive(Resource, Default, Clone, PartialEq)]
pub struct UiPanels {
    pub character: bool,
    /// Skills window (`S`) and its tab (1-based).
    pub skills: bool,
    pub skill_tab: u8,
    /// Character stats tab 1..=4 (tab pane shown inside the character window).
    pub tab: u8,
    /// Devotion window (`Y`).
    pub devotion: bool,
    /// Scroll offset (UI units) of the page II / III lists, by page.
    pub sheet_scroll: [f32; 4],
    /// Collapsed list sections (header field names).
    pub sheet_collapsed: Vec<String>,
}

impl UiPanels {
    /// Equal apart from the page II / III scroll (the lists scroll in place, `character::scroll_sheet`: a scroll
    /// is not a reason to rebuild the character window).
    fn same_but_scroll(&self, o: &Self) -> bool {
        let Self {
            character,
            skills,
            skill_tab,
            tab,
            devotion,
            sheet_scroll: _,
            sheet_collapsed,
        } = self;
        *character == o.character
            && *skills == o.skills
            && *skill_tab == o.skill_tab
            && *tab == o.tab
            && *devotion == o.devotion
            && *sheet_collapsed == o.sheet_collapsed
    }
}

/// The HUD's separately built root groups: each has its own signature, so a change rebuilds only the
/// groups that read it. Stacked in this order ([`ZIndex`] on the root), under the overlays that carry
/// a `GlobalZIndex`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Hud,
    Character,
    Trade,
    Skills,
    Devotion,
}

impl Group {
    const ALL: [Group; 5] = [
        Group::Hud,
        Group::Character,
        Group::Trade,
        Group::Skills,
        Group::Devotion,
    ];
}

#[derive(Component)]
pub struct UiRoot(pub Group);

/// A rebuilt UI tree waiting one frame (hidden) for layout and glyphs; then it
/// replaces the previous tree of its group, so a rebuild never shows a half-built frame.
#[derive(Component)]
pub struct UiFresh;

/// Shows each group's fresh root and despawns the root it replaces. `rebuild` runs after this system
/// and spawns at most one root per group per frame, so a group has at most one fresh root here.
fn swap_fresh(
    mut commands: Commands,
    fresh: Query<(Entity, &UiRoot), With<UiFresh>>,
    roots: Query<(Entity, &UiRoot), Without<UiFresh>>,
) {
    for (f, g) in &fresh {
        for (e, r) in &roots {
            if r.0 == g.0 {
                commands.entity(e).try_despawn();
            }
        }
        commands
            .entity(f)
            .remove::<UiFresh>()
            .insert(Visibility::Inherited);
    }
}

/// What each group's last build read (owned copies, compared against the live values without
/// cloning: [`Live`]). A group rebuilds when its own signature differs.
#[derive(Resource, Default)]
struct Built {
    hud: Option<HudSig>,
    character: Option<CharacterSig>,
    trade: Option<TradeSig>,
    skills: Option<SkillsSig>,
    devotion: Option<DevotionSig>,
}

/// The live inputs of the signatures.
struct Live<'a> {
    /// The window size and the UI scale's bits (`UiAssets::scale`): a scale change in Options
    /// rebuilds every group at the new scale, as a resize does (the HUD is never reopened).
    size: (UVec2, u32),
    panels: &'a UiPanels,
    values: &'a HudValues,
    inventory: u64,
    trade: u64,
    trade_open: bool,
    potions: bool,
    loot_button: lootfilter::HudButton,
    /// The factions and achievements windows are open (their classic HUD buttons are down).
    menu_down: [bool; 2],
    devotion: (u64, u64),
    classic_hud: bool,
}

impl Live<'_> {
    fn points(&self) -> (u32, u32) {
        (self.values.attribute_points, self.values.skill_points)
    }
    /// What decides where the character and NPC windows sit (`docked`).
    fn docking(&self) -> (bool, bool, bool, u64) {
        (
            self.panels.character,
            self.panels.skills,
            self.potions,
            self.trade,
        )
    }
}

#[derive(PartialEq)]
struct HudSig {
    size: (UVec2, u32),
    level: u32,
    points: (u32, u32),
    teleport: Option<gd_game::riftgates::PortalButton>,
    labels: ([&'static str; 2], String),
    row: usize,
    skills: gd_sim::skills::SkillBook,
    hotbar: Vec<Option<String>>,
    hotbar_labels: std::collections::BTreeMap<&'static str, String>,
    loot_button: lootfilter::HudButton,
    /// The factions and achievements windows are open (their classic HUD buttons are down).
    menu_down: [bool; 2],
    weapon_swap: bool,
    menu_folder: menufolder::FolderView,
    options_button: hud::OptionsButton,
    classic_hud: bool,
}

impl HudSig {
    fn of(l: &Live) -> Self {
        let v = l.values;
        Self {
            size: l.size,
            level: v.level,
            points: l.points(),
            teleport: v.teleport,
            labels: (v.mode_tags, v.area_tag.clone()),
            row: v.hotbar_row,
            skills: v.skills.clone(),
            hotbar: v.hotbar.clone(),
            hotbar_labels: v.hotbar_labels.clone(),
            loot_button: l.loot_button,
            menu_down: l.menu_down,
            weapon_swap: v.weapon_swap,
            menu_folder: v.menu_folder,
            options_button: v.options_button,
            classic_hud: l.classic_hud,
        }
    }
    fn same(&self, l: &Live) -> bool {
        let v = l.values;
        self.size == l.size
            && self.level == v.level
            && self.points == l.points()
            && self.teleport == v.teleport
            && self.labels.0 == v.mode_tags
            && self.labels.1 == v.area_tag
            && self.row == v.hotbar_row
            && self.loot_button == l.loot_button
            && self.menu_down == l.menu_down
            && self.weapon_swap == v.weapon_swap
            && self.menu_folder == v.menu_folder
            && self.options_button == v.options_button
            && self.classic_hud == l.classic_hud
            && self.hotbar == v.hotbar
            && self.hotbar_labels == v.hotbar_labels
            && self.skills == v.skills
    }
}

/// The character window's layout signature. Gold and the stats numbers are NOT here: they
/// change without changing the layout (every pickup / kill / stat tick) and are redrawn in
/// place by the window (`character.rs`), so a rebuild for them would be a rebuild storm.
#[derive(PartialEq)]
struct CharacterSig {
    size: (UVec2, u32),
    panels: UiPanels,
    docking: (bool, bool, bool, u64),
    level: u32,
    inventory: u64,
    points: (u32, u32),
    skills: gd_sim::skills::SkillBook,
    /// The mouse skills' names on page II (`list.SkillTag0/1`, the bound left / right hot slots):
    /// drawn only at build, so a rebind rebuilds.
    skill_tags: [Option<String>; 2],
}

impl CharacterSig {
    fn of(l: &Live) -> Self {
        let v = l.values;
        Self {
            size: l.size,
            panels: l.panels.clone(),
            docking: l.docking(),
            level: v.level,
            inventory: l.inventory,
            points: l.points(),
            skills: v.skills.clone(),
            skill_tags: Self::skill_tags(v).map(|t| t.cloned()),
        }
    }
    fn skill_tags(v: &HudValues) -> [Option<&String>; 2] {
        ["list.SkillTag0", "list.SkillTag1"].map(|k| v.stats.get(k))
    }
    fn same(&self, l: &Live) -> bool {
        let v = l.values;
        // Closed before and now: nothing is drawn, whatever the values do.
        if !self.panels.character && !l.panels.character && self.size == l.size {
            return true;
        }
        self.size == l.size
            && self.docking == l.docking()
            && self.level == v.level
            && self.inventory == l.inventory
            && self.points == l.points()
            && self.panels.same_but_scroll(l.panels)
            && self.skills == v.skills
            && self
                .skill_tags
                .iter()
                .map(Option::as_ref)
                .eq(Self::skill_tags(v))
    }
}

#[derive(PartialEq)]
struct TradeSig {
    size: (UVec2, u32),
    open: bool,
    docking: (bool, bool, bool, u64),
    level: u32,
    gold: u64,
    inventory: u64,
}

impl TradeSig {
    fn of(l: &Live) -> Self {
        Self {
            size: l.size,
            open: l.trade_open,
            docking: l.docking(),
            level: l.values.level,
            gold: l.values.gold,
            inventory: l.inventory,
        }
    }
    fn same(&self, l: &Live) -> bool {
        if !self.open && !l.trade_open && self.size == l.size {
            return true;
        }
        self.size == l.size
            && self.open == l.trade_open
            && self.docking == l.docking()
            && self.level == l.values.level
            && self.gold == l.values.gold
            && self.inventory == l.inventory
    }
}

#[derive(PartialEq)]
struct SkillsSig {
    size: (UVec2, u32),
    open: (bool, u8, bool),
    level: u32,
    points: u32,
    undo: bool,
    skills: gd_sim::skills::SkillBook,
    /// Reclamation mode and, in it, the money (its texts and the f8 flags).
    reclaim: Option<(reclamation::ReclaimView, u64)>,
}

impl SkillsSig {
    fn reclaim(v: &HudValues) -> Option<(&reclamation::ReclaimView, u64)> {
        v.reclaim.as_ref().map(|r| (r, v.gold))
    }
    fn open(l: &Live) -> (bool, u8, bool) {
        (l.panels.skills, l.panels.skill_tab, l.panels.devotion)
    }
    fn of(l: &Live) -> Self {
        let v = l.values;
        Self {
            size: l.size,
            open: Self::open(l),
            level: v.level,
            points: v.skill_points,
            undo: v.skill_undo,
            skills: v.skills.clone(),
            reclaim: Self::reclaim(v).map(|(r, g)| (r.clone(), g)),
        }
    }
    fn same(&self, l: &Live) -> bool {
        let v = l.values;
        if !self.open.0 && !l.panels.skills && self.size == l.size {
            return true;
        }
        self.size == l.size
            && self.open == Self::open(l)
            && self.level == v.level
            && self.points == v.skill_points
            && self.undo == v.skill_undo
            && self.skills == v.skills
            && self.reclaim.as_ref().map(|(r, g)| (r, *g)) == Self::reclaim(v)
    }
}

#[derive(PartialEq)]
struct DevotionSig {
    size: (UVec2, u32),
    open: (bool, bool, u8),
    version: (u64, u64),
    undo: bool,
    skills: gd_sim::skills::SkillBook,
    /// The reclaim mode and, in it, the money (the sidebar's numbers, the star looks).
    reclaim: Option<(reclamation::DevotionReclaimView, u64)>,
}

impl DevotionSig {
    fn reclaim(v: &HudValues) -> Option<(&reclamation::DevotionReclaimView, u64)> {
        v.devotion_reclaim.as_ref().map(|r| (r, v.gold))
    }
    fn open(l: &Live) -> (bool, bool, u8) {
        (l.panels.devotion, l.panels.skills, l.panels.skill_tab)
    }
    fn of(l: &Live) -> Self {
        Self {
            size: l.size,
            open: Self::open(l),
            version: l.devotion,
            undo: l.values.devotion_undo,
            skills: l.values.skills.clone(),
            reclaim: Self::reclaim(l.values).map(|(r, g)| (r.clone(), g)),
        }
    }
    fn same(&self, l: &Live) -> bool {
        if !self.open.0 && !l.panels.devotion && self.size == l.size {
            return true;
        }
        self.size == l.size
            && self.open == Self::open(l)
            && self.version == l.devotion
            && self.undo == l.values.devotion_undo
            && self.skills == l.values.skills
            && self.reclaim.as_ref().map(|(r, g)| (r, *g)) == Self::reclaim(l.values)
    }
}

pub struct UiPlugin;

/// Window input systems (clicks, hotkeys that consume the mouse): after the inventory is initialised
/// and synced, before the player's click-to-move and the casting / pet world clicks, so a click on a
/// window never reaches the world.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct UiInputSet;

/// Everything that shows or reads the playing session (`AppState::InGame`): the HUD, windows, tooltips, hover
/// and overlays. One run condition on the set (both `Update` and `PostUpdate`) instead of one per system;
/// [`UiInputSet`] and [`HudSet`] are inside it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct UiPlaySet;

/// Whether physical-px point `p` is inside `r` (half-open: the right and bottom edges are outside). The one hit test of
/// every window; do not write another.
pub fn contains(r: &gd_formats::ui::Rect, p: Vec2) -> bool {
    p.x >= r.x && p.y >= r.y && p.x < r.x + r.w && p.y < r.y + r.h
}

/// The pointer in physical px, `None` while it is outside the window: `GD_HOVER` (hooks.rs) stands in for the mouse.
/// The one pointer read of every window; do not read `Window::cursor_position` beside it.
pub fn cursor_px(hooks: &crate::hooks::Hooks, win: &Window) -> Option<Vec2> {
    hooks
        .ui
        .hover_px
        .or(win.cursor_position())
        .map(|c| c * win.scale_factor())
}

/// [`spawn_root`] for an overlay rebuilt every frame: stamped with the frame it was spawned ([`RootFrame`], [`retire_roots`]).
pub fn spawn_frame_root(
    commands: &mut Commands,
    marker: impl Component,
    frame: u32,
    z: i32,
) -> Entity {
    let root = spawn_root(commands, marker, z);
    commands.entity(root).insert(RootFrame(frame));
    root
}

/// Places an absolutely positioned node at `r` (screen px; `inv`: physical → logical), writing only what changed
/// (an unchanged node stays out of the UI layout's change detection).
pub fn set_node_rect(node: &mut Mut<Node>, r: URect, inv: f32) {
    let v = [r.x, r.y, r.w.max(0.0), r.h.max(0.0)].map(|p| Val::Px(p * inv));
    if [node.left, node.top, node.width, node.height] != v {
        let n = &mut **node;
        [n.left, n.top, n.width, n.height] = v;
    }
}

/// Sets an image node's source rect (texture px) when it changed.
pub fn set_image_src(img: &mut Mut<ImageNode>, src: URect) {
    let r = Some(Rect::new(
        src.x,
        src.y,
        src.x + src.w.max(0.0),
        src.y + src.h.max(0.0),
    ));
    if img.rect != r {
        img.rect = r;
    }
}

/// Where a [`LiveText`] draws: [`Ui::text_line_tinted`] with its top-left at `at` (with `centre`, `at.x` is the
/// centre: the line starts half its truncated width left of it, as the overhead health text), or [`Ui::text`] in a
/// record text box at `origin`.
#[derive(Clone)]
pub enum TextAt {
    Line {
        style: String,
        at: Vec2,
        centre: bool,
        tint: Color,
    },
    Box {
        tb: TextBox,
        origin: Vec2,
    },
}

/// A text that changes more often than the layout around it (a countdown, a bar's numbers), redrawn in place: a
/// node at its root's origin whose child is the text, respawned by [`LiveText::set`] when the text differs
/// (nothing while empty). Spawned by [`Ui::live_text`].
#[derive(Component)]
pub struct LiveText {
    at: TextAt,
    s: f32,
    inv: f32,
    screen: Vec2,
    shown: String,
}

impl LiveText {
    /// Draws `text` under `me` (this text's node) when it differs from the one shown.
    pub fn set(
        &mut self,
        me: Entity,
        text: &str,
        commands: &mut Commands,
        data: &GameData,
        assets: &mut UiAssets,
    ) {
        if self.shown == text {
            return;
        }
        self.shown.clear();
        self.shown.push_str(text);
        commands.entity(me).despawn_related::<Children>();
        let mut ui = assets.ui(commands, data, me, self.s, self.inv, self.screen);
        ui.draw_text(&self.at, text);
    }
}

impl Ui<'_, '_, '_> {
    /// A [`LiveText`] showing `text` at `at`: its node, under the root.
    pub fn live_text(&mut self, at: TextAt, text: &str) -> Entity {
        let node = self
            .commands
            .spawn(Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                ..default()
            })
            .id();
        self.commands.entity(self.root).add_child(node);
        let root = std::mem::replace(&mut self.root, node);
        self.draw_text(&at, text);
        self.root = root;
        self.commands.entity(node).insert(LiveText {
            at,
            s: self.s,
            inv: self.inv,
            screen: self.screen,
            shown: text.to_owned(),
        });
        node
    }

    fn draw_text(&mut self, at: &TextAt, text: &str) {
        match at {
            TextAt::Line {
                style,
                at,
                centre,
                tint,
            } => {
                let x = if *centre {
                    at.x - (self.text_width(style, text).trunc() * 0.5).trunc()
                } else {
                    at.x
                };
                self.text_line_tinted(style, Vec2::new(x, at.y), text, *tint);
            }
            TextAt::Box { tb, origin } => {
                self.text(tb, *origin, text);
            }
        }
    }
}

/// The HUD chain ([`view::collect`] → [`rebuild`] → tooltips, clock), after [`crate::inventory::InventorySet`]
/// so a click's result is applied before the rebuild reads it (the icon moves on the click frame).
/// What reads or extends the built HUD runs `.after(HudSet)`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HudSet;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            additive::AdditivePlugin,
            masked::MaskedPlugin,
            apply::ApplyUiPlugin,
            devotion::DevotionUiPlugin,
            dialog::DialogUiPlugin,
            quests::QuestUiPlugin,
            (survival::SurvivalUiPlugin, challenge::ChallengeUiPlugin),
            errormsg::ErrorMessagePlugin,
            newitems::NewItemsPlugin,
            shrine::ShrineUiPlugin,
            (
                potions::PotionsUiPlugin,
                altar::AltarUiPlugin,
                reclamation::ReclamationUiPlugin,
                factions::FactionsUiPlugin,
                achievements::AchievementsUiPlugin,
            ),
            hotbar_pick::HotbarPickPlugin,
            rollover::RolloverPlugin,
            overhead::OverheadPlugin,
            floating::FloatingPlugin,
        ))
        .add_plugins((
            quicktip::QuickTipPlugin,
            cinematic::CinematicPlugin,
            notify::NotifyPlugin,
            questpopup::QuestPopupPlugin,
            credits::CreditsPlugin,
            stack::StackUiPlugin,
            menufolder::MenuFolderPlugin,
        ))
        .add_plugins((
            gamemenu::GameMenuPlugin,
            options::OptionsUiPlugin,
            mainmenu::MainMenuUiPlugin,
            cursor::CursorPlugin,
            loading::LoadingPlugin,
        ))
        .configure_sets(
            Update,
            UiPlaySet.run_if(in_state(crate::session::AppState::InGame)),
        )
        .configure_sets(
            PostUpdate,
            UiPlaySet.run_if(in_state(crate::session::AppState::InGame)),
        )
        .configure_sets(
            Update,
            (
                UiInputSet
                    .after(crate::inventory::InventorySet)
                    .before(crate::player::PlayerSet),
                HudSet.after(crate::inventory::InventorySet),
            )
                .in_set(UiPlaySet),
        )
        .add_systems(
            PostUpdate,
            hud::toggle_ui
                .in_set(UiPlaySet)
                .before(bevy::ui::UiSystems::Prepare),
        )
        .init_resource::<HudValues>()
        .init_resource::<HudVitals>()
        .init_resource::<Built>()
        .init_resource::<skills::SkillLayout>()
        .init_resource::<UiPanels>()
        .init_resource::<hud::LevelUpIndicator>()
        .init_resource::<hud::HotbarRow>()
        .init_resource::<FontCache>()
        .add_systems(PreStartup, (keys::load, open_panels))
        .add_systems(
            PreUpdate,
            keys::sync_mode.run_if(resource_exists::<gd_game::options::GameOptions>),
        )
        // Before the windows that reset the mouse (game menu, options; the main menu's after them).
        .add_systems(
            PreUpdate,
            (widgets::play_button_sounds, keys::mouse_codes)
                .after(bevy::input::InputSystems)
                .before(gamemenu::GameMenuInputSet)
                .before(options::OptionsInputSet),
        )
        .add_systems(Update, load_tags)
        .add_systems(
            Update,
            (
                // After the window clicks (a modal box answers Esc first) and the pet input (Esc
                // clears the pet selection whatever the dispatcher then closes).
                esc_dispatcher
                    .after(window::WindowClicks)
                    .after(crate::pets::PetInputSet),
                (hud::capture, hud::buttons).chain(),
            )
                .in_set(crate::ui::UiInputSet),
        )
        .add_systems(
            Update,
            (
                (
                    view::collect,
                    swap_fresh,
                    toggle_panels,
                    rebuild,
                    hud::button_states,
                    hud::hot_slot_states,
                    character::scroll_sheet,
                    hud::animate_bars,
                    hud::flash_bars,
                )
                    .chain(),
                (
                    hud::orb_texts,
                    hud::pulse_hud,
                    character::sheet_vitals,
                    status::draw,
                    cooldown::draw,
                    status::hotbar_tooltip,
                    status::buff_tooltip,
                    skills::tooltip,
                    hud::clock,
                )
                    .chain(),
            )
                .chain()
                .in_set(HudSet),
        );
    }
}

/// `GD_UI_OPEN` (hooks.rs) opens the inventory, character, skills or devotion window at start
/// (`quests`, `potions`, `localmap`, `lootfilter`: their own modules).
fn open_panels(mut commands: Commands, hooks: Res<crate::hooks::Hooks>) {
    let Some(v) = hooks
        .ui
        .open
        .as_deref()
        .filter(|v| *v != "quests" && *v != "potions" && *v != "factions")
    else {
        return;
    };
    commands.insert_resource(match v {
        "skills" => UiPanels {
            skills: true,
            skill_tab: hooks.ui.tab.unwrap_or(1),
            ..default()
        },
        "devotion" => UiPanels {
            devotion: true,
            ..default()
        },
        _ => UiPanels {
            character: true,
            tab: hooks.ui.tab.unwrap_or(1),
            sheet_scroll: [hooks.ui.sheet_scroll; 4],
            sheet_collapsed: hooks.ui.sheet_collapse.clone(),
            ..default()
        },
    });
}

/// The font cache follows the data: tags loaded (and fonts and styles dropped) at startup and again whenever the
/// mounted mod changes (the base game, the Crucible, a custom game's mod; `GameData::tags` keeps one table per mod).
fn load_tags(
    data: Res<GameData>,
    mut fonts: ResMut<FontCache>,
    mut loaded: Local<Option<Option<String>>>,
) {
    let active = data.active_mod();
    if loaded.as_ref().map(Option::as_deref) != Some(active) {
        *loaded = Some(active.map(str::to_owned));
        fonts.load_tags(&data);
    }
}

fn toggle_panels(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<keys::KeyBindings>,
    mut windows: window::Windows,
) {
    if bindings.just_pressed(keys::CHARACTER, &keys) {
        windows.toggle(window::Window::Character);
        if windows.panels.character && windows.panels.tab == 0 {
            windows.panels.tab = 1;
        }
    }
    if bindings.just_pressed(keys::SKILLS, &keys) {
        windows.toggle(window::Window::Skills);
    }
    // Esc is the shared dispatcher's ([`esc_dispatcher`]): it closes the frontmost window.
}

/// The one Escape dispatcher (the engine routes a key to the focused window only: the factions
/// key `sub_140022830` case 52, `CursorHandlerEnchant::Escape` returning "consumed"; INFERENCE Esc follows
/// the same focus model). It walks the open modals front-to-back (a confirmation over its window, the
/// stack / pick / search boxes, the options window, the apply mode and the armed hot slot), then the focus
/// stack of open windows — a window moves to the front when it opens — closes the first one found and
/// consumes the press; with nothing open it opens the game menu. The PreUpdate input systems that swallow
/// the keyboard (game menu, options, stack, devotion search) leave Escape alive for it
/// ([`keys::reset_but_escape`]).
#[allow(clippy::too_many_arguments)]
fn esc_dispatcher(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut windows: window::Windows,
    mut options: ResMut<options::OptionsWindow>,
    mut pause: ResMut<gd_game::pause::GamePause>,
    inv: Option<Res<crate::inventory::Inventory>>,
    mut confirm: ResMut<apply::InvConfirm>,
    mut reclaim: ResMut<reclamation::NoPointsBox>,
    mut pick: ResMut<hotbar_pick::PickView>,
    mut stack: ResMut<stack::StackWindow>,
    mut devotion: ResMut<devotion::DevotionView>,
    mut potions: ResMut<potions::PotionsView>,
    mut pets: ResMut<pets::PetMenu>,
    (armed, over): (
        Res<gd_game::hotbar::ArmedSlot>,
        Res<crate::death::GameOverBox>,
    ),
    (mut apply_out, mut destroy_out, mut use_out, mut disarm_out, mut prompt_out, mut exit): (
        MessageWriter<gd_game::inventory::ApplyRequest>,
        MessageWriter<gd_game::item_drop::DestroyAnswer>,
        MessageWriter<gd_game::consumables::UseAnswer>,
        MessageWriter<gd_game::hotbar::DisarmHotSlot>,
        MessageWriter<crate::trade::DismissPrompt>,
        MessageWriter<crate::exit::ExitToMenu>,
    ),
    (mut fresh, mut focus, mut was): (
        gd_game::session::NewSession,
        Local<Vec<window::Window>>,
        Local<[bool; window::Window::COUNT]>,
    ),
) {
    if fresh.begun() {
        focus.clear();
        *was = Default::default();
    }
    // The focus stack: a window moves to the front when it opens, drops out when it closes.
    let now = windows.snapshot();
    for (i, w) in window::Window::ALL.into_iter().enumerate() {
        if now[i] && !was[i] {
            focus.retain(|f| *f != w);
            focus.push(w);
        } else if !now[i] {
            focus.retain(|f| *f != w);
        }
    }
    *was = now;
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    let closed = 'close: {
        // The hardcore Game Over box is over everything: Esc answers it and leaves the session (`death.rs`).
        if over.is_open() {
            exit.write(crate::exit::ExitToMenu);
            break 'close true;
        }
        // The game menu's own confirmation is over everything else.
        if windows.menu.confirm.is_some() {
            windows.menu.confirm = None;
            break 'close true;
        }
        // The options window (`Z_OPTIONS`) and the game menu (`Z_MENU`) are drawn over every
        // modal box below (`Z_MODAL` / `Z_WINDOW`), which stay open under them: Esc closes the top.
        if options.escape() {
            break 'close true;
        }
        if windows.menu.open {
            windows.close(window::Window::GameMenu);
            break 'close true;
        }
        // The trade window's dialog box (`crate::trade::Prompt`) or the Inventor's dismantle warning
        // (`Trade::confirm`): Esc answers No.
        if windows.trade.prompt.is_some() || windows.trade.confirm {
            prompt_out.write(crate::trade::DismissPrompt);
            break 'close true;
        }
        // The apply mode's questions (`apply.rs`): Esc answers No.
        if *confirm != apply::InvConfirm::None {
            *confirm = apply::InvConfirm::None;
            break 'close true;
        }
        if let Some(inv) = inv.as_deref() {
            if inv.destroying.is_some() {
                destroy_out.write(gd_game::item_drop::DestroyAnswer(false));
                break 'close true;
            }
            if gd_game::consumables::use_question(inv).is_some() {
                use_out.write(gd_game::consumables::UseAnswer(false));
                break 'close true;
            }
            if inv.applying.as_ref().is_some_and(|a| a.confirm.is_some()) {
                apply_out.write(gd_game::inventory::ApplyRequest::Answer(false));
                break 'close true;
            }
        }
        if reclaim.escape() {
            break 'close true;
        }
        if windows.loot.escape_confirm() {
            break 'close true;
        }
        if stack.open.is_some() {
            stack.hide();
            break 'close true;
        }
        if pick.slot.is_some() {
            *pick = hotbar_pick::PickView::default();
            break 'close true;
        }
        if devotion.pick.is_some() {
            devotion.pick = None;
            devotion.version += 1;
            break 'close true;
        }
        if devotion.typing {
            devotion.typing = false;
            devotion.version += 1;
            break 'close true;
        }
        if potions.list.is_some() {
            potions.list = None;
            break 'close true;
        }
        // The apply mode itself (no confirmation up): Esc ends it, the window stays.
        if inv.as_deref().is_some_and(|i| i.applying.is_some()) {
            apply_out.write(gd_game::inventory::ApplyRequest::Cancel);
            break 'close true;
        }
        // An armed hot slot drops before any window closes (the slot buttons take Esc,
        // sub_140211810, `hotbar_pick::disarm`).
        if armed.0.is_some() {
            disarm_out.write(gd_game::hotbar::DisarmHotSlot);
            break 'close true;
        }
        if pets.0.is_some() {
            pets.0 = None;
            break 'close true;
        }
        if let Some(w) = focus.last().copied() {
            windows.close(w);
            focus.pop();
            break 'close true;
        }
        false
    };
    if !closed {
        // Nothing open: the game menu, as its key (it replaces the pause key's pause).
        windows.menu.open = true;
        pause.key = false;
    }
    keys.clear_just_pressed(KeyCode::Escape);
}

/// Where the character window sits (`Ui::docked`: the x span of the window beside it): an NPC window
/// and the inventory side by side, centred; the potions window and the inventory; else a lone
/// character window centred. Only while the character window is open.
fn docked(
    panels: &UiPanels,
    trade: &crate::trade::Trade,
    potions_open: bool,
    data: &GameData,
    screen: Vec2,
    s: f32,
) -> Option<(f32, f32)> {
    if !panels.character {
        return None;
    }
    if let Some(rec) = trade::window_record(trade).and_then(|r| data.db.get(r))
        && let Some(cm) = data.db.get(character::MASTER)
    {
        let tw = WindowDef::from_record(rec).screen_rect(screen.x, screen.y, s);
        let cw = WindowDef::from_record(cm).screen_rect(screen.x, screen.y, s);
        let left = ((screen.x - tw.w - cw.w) * 0.5).max(0.0).round();
        Some((left, left + tw.w))
    } else if potions_open && let Some(pr) = potions::window_rect(data, screen, s) {
        // The potions window and the inventory side by side (`potions.rs`).
        Some((pr.x, pr.x + pr.w))
    } else if !panels.skills
        && let Some(cm) = data.db.get(character::MASTER)
    {
        // PORT: a lone character window opens centred (the record's
        // `windowScreenAlignmentX Right` is only used beside the skills window).
        let cw = WindowDef::from_record(cm).screen_rect(screen.x, screen.y, s);
        Some((0.0, ((screen.x - cw.w) * 0.5).max(0.0).round()))
    } else {
        None
    }
}

/// Builds the HUD's root groups ([`Group`]) whose signature changed; a closed window's group is
/// despawned and its layout cache reset instead of built.
#[allow(clippy::too_many_arguments)]
fn rebuild(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    panels: Res<UiPanels>,
    (values, vitals): (Res<HudValues>, Res<HudVitals>),
    mut built: ResMut<Built>,
    roots: Query<(Entity, &UiRoot)>,
    inv: Res<crate::inventory::Inventory>,
    reqs: Query<&gd_game::inventory::Requirements, With<crate::player::Player>>,
    trade: Res<crate::trade::Trade>,
    markets: Res<crate::trade::Markets>,
    stash: Option<Res<crate::trade::Stash>>,
    rules: Option<Res<crate::combat::CombatRules>>,
    (craft, devotion, mut view, potions_open, loot_view, items_hidden, factions): (
        Option<Res<crate::crafting::CraftList>>,
        Option<Res<crate::devotion::DevotionState>>,
        ResMut<devotion::DevotionView>,
        Res<gd_game::cauldron::PotionWindow>,
        Res<lootfilter::LootFilterView>,
        Res<crate::loot::ItemsHidden>,
        (
            Res<factions::FactionsView>,
            Res<achievements::AchievementsView>,
        ),
    ),
    mut assets: UiAssets,
) {
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if size.x == 0 || size.y == 0 {
        return;
    }
    let loot_button = lootfilter::HudButton::of(&loot_view, &items_hidden);
    let live = Live {
        size: (size, assets.scale(size).to_bits()),
        panels: &panels,
        values: &values,
        inventory: inv.version,
        trade: trade.version,
        trade_open: trade.open.is_some(),
        potions: potions_open.open,
        loot_button,
        menu_down: [factions.0.open, factions.1.open],
        devotion: (devotion.as_ref().map_or(0, |d| d.version), view.version),
        classic_hud: assets.classic_hud(),
    };
    let s = assets.scale(size);
    let screen = Vec2::new(size.x as f32, size.y as f32);
    let spawn = |commands: &mut Commands, g: Group| {
        // Even local z: the odd one between `Hud` and `Character` is the HUD overlays' ([`HUD_OVERLAY_Z`]).
        let z = 2 * Group::ALL.iter().position(|x| *x == g).unwrap_or(0) as i32;
        commands
            .spawn((
                UiRoot(g),
                UiFresh,
                Visibility::Hidden,
                ZIndex(z),
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
            ))
            .id()
    };
    let retire = |commands: &mut Commands, g: Group| {
        for (e, r) in &roots {
            if r.0 == g {
                commands.entity(e).try_despawn();
            }
        }
    };
    let req = reqs.single().copied().unwrap_or_default();
    macro_rules! ui {
        ($root:expr) => {{
            let mut ui = assets.ui(
                &mut commands,
                &data,
                $root,
                s,
                1.0 / win.scale_factor(),
                screen,
            );
            ui.req = req;
            ui
        }};
    }
    let mut rebuilt = Vec::new();
    if !built.hud.as_ref().is_some_and(|b| b.same(&live)) {
        built.hud = Some(HudSig::of(&live));
        let root = spawn(&mut commands, Group::Hud);
        hud::build(&mut ui!(root), &values, loot_button, live.menu_down);
        rebuilt.push("hud");
    }
    if !built.character.as_ref().is_some_and(|b| b.same(&live)) {
        built.character = Some(CharacterSig::of(&live));
        if panels.character {
            let root = spawn(&mut commands, Group::Character);
            let mut ui = ui!(root);
            ui.docked = docked(&panels, &trade, potions_open.open, &data, screen, s);
            character::build(&mut ui, &values, &vitals, &panels, Some(&inv));
        } else {
            retire(&mut commands, Group::Character);
            commands.insert_resource(crate::inventory::InvLayout::default());
        }
        rebuilt.push("character");
    }
    if !built.trade.as_ref().is_some_and(|b| b.same(&live)) {
        built.trade = Some(TradeSig::of(&live));
        if trade.open.is_some() {
            let root = spawn(&mut commands, Group::Trade);
            let mut ui = ui!(root);
            ui.docked = docked(&panels, &trade, potions_open.open, &data, screen, s);
            trade::build(
                &mut ui,
                &trade,
                &markets,
                stash.as_deref(),
                craft.as_deref(),
                (values.gold, values.level),
                Some(&inv),
            );
        } else {
            retire(&mut commands, Group::Trade);
            commands.insert_resource(crate::trade::TradeLayout::default());
        }
        rebuilt.push("trade");
    }
    if !built.skills.as_ref().is_some_and(|b| b.same(&live)) {
        built.skills = Some(SkillsSig::of(&live));
        if panels.skills {
            let root = spawn(&mut commands, Group::Skills);
            skills::build(&mut ui!(root), &values, panels.skill_tab);
        } else {
            retire(&mut commands, Group::Skills);
            commands.insert_resource(skills::SkillLayout::default());
        }
        rebuilt.push("skills");
    }
    if !built.devotion.as_ref().is_some_and(|b| b.same(&live)) {
        built.devotion = Some(DevotionSig::of(&live));
        match (&devotion, panels.devotion) {
            (Some(d), true) => {
                if view.built_scroll != view.scroll {
                    // Build bookkeeping, not a view change: devotion.rs readers key on `version`.
                    let v = view.bypass_change_detection();
                    v.built_scroll = v.scroll;
                }
                let root = spawn(&mut commands, Group::Devotion);
                let max_points = rules
                    .as_ref()
                    .map_or(0, |r| r.0.levels.max_devotion_points.max(0) as u32);
                devotion::build(&mut ui!(root), &values, d, &view, max_points);
            }
            _ => {
                retire(&mut commands, Group::Devotion);
                commands.insert_resource(devotion::DevotionLayout::default());
            }
        }
        rebuilt.push("devotion");
    }
    if !rebuilt.is_empty() {
        info!(
            "ui: built {} for {}x{} (scale {s:.3})",
            rebuilt.join(", "),
            size.x,
            size.y
        );
    }
}

/// The frame a rebuilt overlay's root was spawned. A freshly spawned UI
/// root is not drawn on its first frame, so an overlay rebuilt every frame
/// (the life text in a fight) would never show: [`retire_roots`] keeps the
/// root being replaced until its successor is drawn.
#[derive(Component)]
pub struct RootFrame(pub u32);

/// Despawns an overlay's old roots: all of them when `clear`; else every
/// root but the newest, once the newest has been drawn (spawned before
/// `frame`) or when a new one replaces it (`replacing`). Returns the root
/// that stays: an overlay updating its parts in place must only touch parts
/// under it, as the others' despawn is queued (a [`LiveText`] under one
/// would redraw into a despawned node).
pub fn retire_roots(
    commands: &mut Commands,
    roots: impl Iterator<Item = (Entity, u32)>,
    frame: u32,
    replacing: bool,
    clear: bool,
) -> Option<Entity> {
    let roots: Vec<(Entity, u32)> = roots.collect();
    let newest = roots.iter().max_by_key(|r| (r.1, r.0)).copied();
    for (e, f) in &roots {
        let keep = !clear && Some((*e, *f)) == newest;
        if !keep && (clear || replacing || newest.is_some_and(|n| n.1 < frame)) {
            commands.entity(*e).try_despawn();
        }
    }
    newest.filter(|_| !clear).map(|n| n.0)
}

/// The asset resources every [`Ui`] builder needs, as one system parameter: `assets.ui(..)` makes the build context (instead of a system
/// listing four `ResMut`s and spelling out the struct). Direct access to the caches (`assets.fonts.tags`, ...) is for what a system
/// does outside the `Ui`.
#[derive(bevy::ecs::system::SystemParam)]
pub struct UiAssets<'w> {
    pub tex: ResMut<'w, Textures>,
    pub images: ResMut<'w, Assets<Image>>,
    pub fonts: ResMut<'w, FontCache>,
    pub font_assets: ResMut<'w, Assets<Font>>,
    pub hooks: Res<'w, crate::hooks::Hooks>,
    options: Option<Res<'w, gd_game::options::GameOptions>>,
    scale: Res<'w, crate::settings::UiScaleOption>,
}

impl UiAssets<'_> {
    /// `standardHUD` (part of the signature of every layout that reads the HUD's table).
    pub fn classic_hud(&self) -> bool {
        hud::classic(self.options.as_deref())
    }

    /// The UI scale factor at a `size` (physical px) screen ([`crate::settings::UiScaleOption`]).
    pub fn scale(&self, size: UVec2) -> f32 {
        self.scale.factor(size.x, size.y)
    }

    /// The build context under `root`: UI scale `s`, physical → logical factor `inv`, the screen's physical size.
    pub fn ui<'a, 'w, 's>(
        &'a mut self,
        commands: &'a mut Commands<'w, 's>,
        data: &'a GameData,
        root: Entity,
        s: f32,
        inv: f32,
        screen: Vec2,
    ) -> Ui<'a, 'w, 's> {
        Ui {
            commands,
            root,
            data,
            tex: &mut self.tex,
            images: &mut self.images,
            fonts: &mut self.fonts,
            font_assets: &mut self.font_assets,
            hooks: &self.hooks,
            classic_hud: hud::classic(self.options.as_deref()),
            s,
            inv,
            screen,
            docked: None,
            req: Default::default(),
        }
    }
}

/// A full-screen, click-through root node for a window or overlay at `z` (`GlobalZIndex`), tagged with `marker`.
pub fn spawn_root(commands: &mut Commands, marker: impl Bundle, z: i32) -> Entity {
    commands
        .spawn((
            marker,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            GlobalZIndex(z),
        ))
        .id()
}

/// The HUD overlays' local [`ZIndex`] (minimap, quest tracker, clock, pet portraits, Crucible /
/// Shattered Realm panes, challenge banner): over the HUD group, under the Character / Trade / Skills /
/// Devotion windows (even local z, `rebuild`) and every `GlobalZIndex` window.
pub const HUD_OVERLAY_Z: i32 = 1;

/// [`spawn_root`] for a HUD overlay: no `GlobalZIndex`, stacked among the HUD groups at [`HUD_OVERLAY_Z`].
pub fn spawn_hud_overlay(commands: &mut Commands, marker: impl Bundle) -> Entity {
    commands
        .spawn((
            marker,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            ZIndex(HUD_OVERLAY_Z),
        ))
        .id()
}

/// Build context: spawns absolutely positioned nodes (physical px) under the root.
pub struct Ui<'a, 'w, 's> {
    pub commands: &'a mut Commands<'w, 's>,
    pub root: Entity,
    pub data: &'a GameData,
    pub tex: &'a mut Textures,
    pub images: &'a mut Assets<Image>,
    pub fonts: &'a mut FontCache,
    pub font_assets: &'a mut Assets<Font>,
    /// Hooks (`GD_REROLL` is read while building the reroll panel).
    pub hooks: &'a crate::hooks::Hooks,
    /// `standardHUD`: the HUD's layout comes from the classic master table ([`hud::master`]).
    pub classic_hud: bool,
    /// UI scale factor.
    pub s: f32,
    /// Physical → logical pixels.
    pub inv: f32,
    pub screen: Vec2,
    /// x of the open NPC window and of the inventory when docked together.
    pub docked: Option<(f32, f32)>,
    /// The player's equip requirements: the item cells' unusable marks (`character::item_cell`). Set by the
    /// builders that draw item cells; the default (level 1, no attributes) elsewhere.
    pub req: gd_game::inventory::Requirements,
}

impl Ui<'_, '_, '_> {
    /// The HUD master table of the chosen layout (`hud.rs` module doc).
    pub fn hud_master(&self) -> &'static str {
        hud::master(self.classic_hud)
    }

    pub fn record(&self, path: &str) -> Option<gd_formats::arz::Record> {
        if path.is_empty() {
            return None;
        }
        let r = self.data.db.get_owned(path);
        if r.is_none() {
            warn!("ui: missing record {path}");
        }
        r
    }

    pub fn widget(&self, path: &str) -> Option<Widget> {
        self.record(path).map(|r| Widget::from_record(&r))
    }

    /// Texture handle + pixel size (UI bitmaps are colour data shown as-is).
    pub fn image(&mut self, path: &str) -> Option<(Handle<Image>, UVec2)> {
        let h = self.tex.get(self.data, path, true, self.images)?;
        let size = self.images.get(&h).map(|i| i.size())?;
        Some((h, size))
    }

    /// [`Self::image`] desaturated.
    pub fn image_gray(&mut self, path: &str) -> Option<(Handle<Image>, UVec2)> {
        let h = self.tex.get_gray(self.data, path, self.images)?;
        let size = self.images.get(&h).map(|i| i.size())?;
        Some((h, size))
    }

    /// `round(v × s)`.
    pub fn sc(&self, v: f32) -> f32 {
        round_px(v * self.s)
    }

    pub fn node(&self, r: URect) -> Node {
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(r.x * self.inv),
            top: Val::Px(r.y * self.inv),
            width: Val::Px(r.w * self.inv),
            height: Val::Px(r.h * self.inv),
            ..default()
        }
    }

    /// Draws `image` (optionally a sub-rect in texture px) into `dst` (screen px).
    pub fn quad(
        &mut self,
        image: Handle<Image>,
        dst: URect,
        src: Option<URect>,
        color: Color,
    ) -> Entity {
        let mut img = ImageNode::new(image).with_color(color);
        img.rect = src.map(|r| Rect::new(r.x, r.y, r.x + r.w, r.y + r.h));
        let e = self.commands.spawn((self.node(dst), img)).id();
        self.commands.entity(self.root).add_child(e);
        e
    }

    /// Like [`Self::quad`] but the image fills `dst` whatever its aspect
    /// (`ImageNode` keeps the image's aspect by default).
    pub fn quad_stretch(&mut self, image: Handle<Image>, dst: URect, color: Color) -> Entity {
        let img = ImageNode::new(image)
            .with_color(color)
            .with_mode(bevy::ui::widget::NodeImageMode::Stretch);
        let e = self.commands.spawn((self.node(dst), img)).id();
        self.commands.entity(self.root).add_child(e);
        e
    }

    pub fn solid(&mut self, dst: URect, color: Color) -> Entity {
        let e = self
            .commands
            .spawn((self.node(dst), BackgroundColor(color)))
            .id();
        self.commands.entity(self.root).add_child(e);
        e
    }

    /// A bitmap at record position (`x`,`y`) relative to `origin`, sized by its texture.
    pub fn bitmap(&mut self, path: &str, x: f32, y: f32, origin: Vec2) -> Option<(Entity, URect)> {
        let (h, size) = self.image(path)?;
        let r = URect::new(
            origin.x + self.sc(x),
            origin.y + self.sc(y),
            self.sc(size.x as f32),
            self.sc(size.y as f32),
        );
        Some((self.quad(h, r, None, Color::WHITE), r))
    }

    /// A record's `Widget::Bitmap` field drawn at `origin` (`None` when the field is no bitmap or its texture
    /// is missing). The one copy of that pattern.
    pub fn bitmap_field(&mut self, rec: &str, origin: Vec2) -> Option<(Entity, URect)> {
        let Widget::Bitmap { bitmap, x, y } = self.widget(rec)? else {
            return None;
        };
        self.bitmap(&bitmap, x, y, origin)
    }

    /// Draws `text` (markup allowed) in a record text box relative to `origin`.
    pub fn text(&mut self, tb: &TextBox, origin: Vec2, text: &str) -> Option<Entity> {
        self.text_with_style(tb, tb.style.as_deref()?, origin, text)
    }

    /// Line height (px) of a font style (the height of one laid-out line).
    pub fn line_height(&mut self, style_path: &str) -> f32 {
        let Some(style) = self.fonts.style(self.data, style_path) else {
            return 0.0;
        };
        let size = style.scaled_size(self.s);
        match self
            .fonts
            .font(self.data, &style.font, self.font_assets)
            .as_deref()
        {
            Some(LoadedFont::Bitmap { font, .. }) => {
                gd_formats::ui::layout_text(
                    font,
                    size,
                    style.flags,
                    "A",
                    (style.color0, style.color1),
                )
                .height as f32
            }
            _ => (size as f32 * 1.2).round(),
        }
    }

    /// The style's pixel size under the UI scale (`StyleManager::LoadStyle`: the style object's +104, which the
    /// conversation layout 0x140170470 uses as the line height).
    pub fn font_px(&mut self, style_path: &str) -> f32 {
        self.fonts
            .style(self.data, style_path)
            .map_or(0.0, |st| st.scaled_size(self.s) as f32)
    }

    /// Width (px) of one line of text in a style.
    pub fn text_width(&mut self, style_path: &str, text: &str) -> f32 {
        self.fonts
            .text_width(self.data, self.font_assets, style_path, self.s, text)
    }

    /// The wrapped lines of `text` (markup kept; `\n` breaks) in a style at
    /// most `max_w` px wide.
    pub fn wrap_lines(&mut self, style_path: &str, max_w: f32, text: &str) -> Vec<String> {
        let Some(style) = self.fonts.style(self.data, style_path) else {
            return Vec::new();
        };
        let size = style.scaled_size(self.s);
        let font = self.fonts.font(self.data, &style.font, self.font_assets);
        let mut out = Vec::new();
        let mut carry = String::new();
        for part in text.split('\n') {
            let part_c = format!("{carry}{part}");
            carry = text::active_color(&part_c);
            match font.as_deref() {
                Some(LoadedFont::Bitmap { font, .. }) => {
                    out.extend(text::wrap_by(&part_c, max_w.trunc(), |t| {
                        text::bitmap_width(font, size, style.flags, t)
                    }))
                }
                _ => out.push(part_c),
            }
        }
        out
    }

    /// One line of text (markup allowed) with its top-left at `pos` (screen
    /// px); `alpha` scales its opacity.
    pub fn text_line(
        &mut self,
        style_path: &str,
        pos: Vec2,
        text: &str,
        alpha: f32,
    ) -> Option<Entity> {
        self.text_line_tinted(style_path, pos, text, Color::srgba(1.0, 1.0, 1.0, alpha))
    }

    /// [`Self::text_line`] multiplied by `tint` (a text box's colour).
    pub fn text_line_tinted(
        &mut self,
        style_path: &str,
        pos: Vec2,
        text: &str,
        tint: Color,
    ) -> Option<Entity> {
        self.text_line_colored(style_path, pos, text, tint, None)
    }

    /// [`Self::text_line_tinted`] with the text widget's colour override (`color`, as [`Self::text_colored`]).
    pub fn text_line_colored(
        &mut self,
        style_path: &str,
        pos: Vec2,
        text: &str,
        tint: Color,
        color: Option<[f32; 3]>,
    ) -> Option<Entity> {
        if text.is_empty() {
            return None;
        }
        let mut style = self.fonts.style(self.data, style_path)?;
        if let Some(c) = color {
            (style.color0, style.color1) = (c, c);
        }
        let font: Arc<LoadedFont> = self.fonts.font(self.data, &style.font, self.font_assets)?;
        let size = style.scaled_size(self.s);
        match &*font {
            LoadedFont::Bitmap { font, atlas } => {
                let ti =
                    self.fonts
                        .rasterize_cached(self.images, font, atlas, &style, size, text)?;
                let dst = URect::new(
                    (pos.x + ti.offset.x).round(),
                    (pos.y + ti.offset.y).round(),
                    ti.size.x as f32,
                    ti.size.y as f32,
                );
                let h = ti.image.clone();
                Some(self.quad(h, dst, None, tint))
            }
            LoadedFont::Dynamic(handle) => {
                let (c, t) = (style.color0, tint.to_srgba());
                let e = self
                    .commands
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(pos.x * self.inv),
                            top: Val::Px(pos.y * self.inv),
                            ..default()
                        },
                        Text::new(gd_formats::text::strip_markup(text)),
                        TextFont {
                            font: FontSource::Handle(handle.clone()),
                            font_size: FontSize::Px(size as f32 * self.inv),
                            ..default()
                        },
                        TextColor(Color::srgba(
                            c[0] * t.red,
                            c[1] * t.green,
                            c[2] * t.blue,
                            t.alpha,
                        )),
                    ))
                    .id();
                self.commands.entity(self.root).add_child(e);
                Some(e)
            }
        }
    }

    /// A window border of eight bitmaps (record fields `upperLeftTexture`,
    /// `upperRightTexture`, `lowerLeftTexture`, `lowerRightTexture`,
    /// `topTexture`, `bottomTexture`, `leftTexture`, `rightTexture`, with an
    /// optional `prefix` such as `obj`). `Grim Dawn.exe`
    /// 0x140259180: every piece lies outside `r`, offset by the upper-left
    /// corner's size: the corners at (x − ulw, y − ulh), (x + w, y − ulh),
    /// (x − ulw, y + h), (x + w, y + h); the top / bottom edges span `w` at
    /// y − ulh / y + h; the left / right edges span `h` at x − ulw / x + w.
    pub fn border(&mut self, rec: &gd_formats::arz::Record, prefix: &str, r: URect, alpha: f32) {
        let key = |k: &str| {
            let k = if prefix.is_empty() {
                k.to_string()
            } else {
                format!("{prefix}{}{}", k[..1].to_uppercase(), &k[1..])
            };
            rec.str(&k).unwrap_or("").to_owned()
        };
        let keys = [
            "upperLeftTexture",
            "upperRightTexture",
            "lowerLeftTexture",
            "lowerRightTexture",
            "topTexture",
            "bottomTexture",
            "leftTexture",
            "rightTexture",
        ];
        let paths = keys.map(key);
        self.border_paths(&paths.each_ref().map(String::as_str), r, alpha);
    }

    /// [`Self::border`] from the eight bitmaps' paths (upper left, upper
    /// right, lower left, lower right, top, bottom, left, right).
    pub fn border_paths(&mut self, paths: &[&str; 8], r: URect, alpha: f32) {
        let get = |ui: &mut Self, i: usize| {
            Some(paths[i])
                .filter(|p| !p.is_empty())
                .and_then(|p| ui.image(p))
        };
        let Some((ul, uls)) = get(self, 0) else {
            return;
        };
        let (cw, ch) = (self.sc(uls.x as f32), self.sc(uls.y as f32));
        let tint = Color::srgba(1.0, 1.0, 1.0, alpha);
        self.quad_stretch(ul, URect::new(r.x - cw, r.y - ch, cw, ch), tint);
        let pieces: [(f32, f32, Option<f32>, Option<f32>); 7] = [
            (r.x + r.w, r.y - ch, None, None),
            (r.x - cw, r.y + r.h, None, None),
            (r.x + r.w, r.y + r.h, None, None),
            (r.x, r.y - ch, Some(r.w), None),
            (r.x, r.y + r.h, Some(r.w), None),
            (r.x - cw, r.y, None, Some(r.h)),
            (r.x + r.w, r.y, None, Some(r.h)),
        ];
        for (i, (x, y, w, h)) in pieces.into_iter().enumerate() {
            let Some((img, size)) = get(self, i + 1) else {
                continue;
            };
            let (iw, ih) = (self.sc(size.x as f32), self.sc(size.y as f32));
            self.quad_stretch(
                img,
                URect::new(x, y, w.unwrap_or(iw), h.unwrap_or(ih)),
                tint,
            );
        }
    }

    pub fn text_with_style(
        &mut self,
        tb: &TextBox,
        style_path: &str,
        origin: Vec2,
        text: &str,
    ) -> Option<Entity> {
        self.text_colored(tb, style_path, origin, text, None)
    }

    /// [`Self::text_with_style`] with the text widget's colour override
    /// (`color`: both glyph colours, as a text widget's +112 flag with its
    /// colour at +96 replaces the style's).
    pub fn text_colored(
        &mut self,
        tb: &TextBox,
        style_path: &str,
        origin: Vec2,
        text: &str,
        color: Option<[f32; 3]>,
    ) -> Option<Entity> {
        if text.is_empty() {
            return None;
        }
        let mut style = self.fonts.style(self.data, style_path)?;
        if let Some(c) = color {
            (style.color0, style.color1) = (c, c);
        }
        let font: Arc<LoadedFont> = self.fonts.font(self.data, &style.font, self.font_assets)?;
        let size = style.scaled_size(self.s);
        let bx = URect::new(
            origin.x + self.sc(tb.rect.x),
            origin.y + self.sc(tb.rect.y),
            self.sc(tb.rect.w),
            self.sc(tb.rect.h),
        );
        match &*font {
            LoadedFont::Bitmap { font, atlas } => {
                let ti =
                    self.fonts
                        .rasterize_cached(self.images, font, atlas, &style, size, text)?;
                let x = tb.align_x.place(bx.x, bx.w, ti.layout.width);
                let y = tb.align_y.place(bx.y, bx.h, ti.layout.height);
                let dst = URect::new(
                    x + ti.offset.x,
                    y + ti.offset.y,
                    ti.size.x as f32,
                    ti.size.y as f32,
                );
                let h = ti.image.clone();
                Some(self.quad(h, dst, None, Color::WHITE))
            }
            LoadedFont::Dynamic(handle) => {
                let plain = gd_formats::text::strip_markup(text);
                let c = style.color0;
                let justify = match tb.align_x {
                    gd_formats::ui::TextAlign::Start => Justify::Left,
                    gd_formats::ui::TextAlign::End => Justify::Right,
                    gd_formats::ui::TextAlign::Center => Justify::Center,
                };
                let e = self
                    .commands
                    .spawn((
                        self.node(bx),
                        Text::new(plain),
                        TextFont {
                            font: FontSource::Handle(handle.clone()),
                            font_size: FontSize::Px(size as f32 * self.inv),
                            ..default()
                        },
                        TextColor(Color::srgb(c[0], c[1], c[2])),
                        TextLayout::justify(justify),
                    ))
                    .id();
                self.commands.entity(self.root).add_child(e);
                Some(e)
            }
        }
    }
}

/// Resets the HUD and window state a session leaves (`exit/mod.rs`).
pub fn reset_session(world: &mut World) {
    world.insert_resource(hud::HotbarRow::default());
    world.insert_resource(UiPanels::default());
    world.insert_resource(Built::default());
    world.insert_resource(localmap::MapWindow::default());
    world.insert_resource(localmap::MapTiles::default());
    world.insert_resource(survival::SurvivalView::default());
    world.insert_resource(survival::EndlessView::default());
    world.insert_resource(survival::GlowState::default());
    world.insert_resource(survival::PaneRollovers::default());
    world.insert_resource(survival::GlowOverlay::default());
    world.insert_resource(survival::SunderedOverlay::default());
    world.insert_resource(errormsg::ErrorMessage::default());
    // Keeps the once-per-process main-menu tip flag (`QuickTip::menu_tip_done`).
    if let Some(mut tip) = world.get_resource_mut::<quicktip::QuickTip>() {
        tip.end_session();
    } else {
        world.insert_resource(quicktip::QuickTip::default());
    }
    world.insert_resource(cinematic::CinematicQueue::default());
    world.insert_resource(survival::MutatorView::default());
    world.insert_resource(quests::QuestJournal::default());
    world.insert_resource(quests::QuestView::default());
    world.insert_resource(quests::TrackerFade::default());
    world.insert_resource(quests::JournalLayout::default());
    world.insert_resource(dialog::Portrait::default());
    world.insert_resource(floating::FloatingTexts::default());
    world.insert_resource(hotbar_pick::PickView::default());
    world.insert_resource(hotbar_pick::PickLayout::default());
    world.insert_resource(hotbar_pick::OverHotSlot::default());
    world.insert_resource(pets::PortraitLayout::default());
    world.insert_resource(pets::PetMenu::default());
    world.insert_resource(gamemenu::GameMenu::default());
    world.insert_resource(gamemenu::GameMenuLayout::default());
    world.insert_resource(potions::PotionsLayout::default());
    world.insert_resource(potions::PotionsView::default());
    world.insert_resource(factions::FactionsView::default());
    world.insert_resource(factions::FactionsLayout::default());
    world.insert_resource(achievements::AchievementsView::default());
    world.insert_resource(achievements::AchievementsLayout::default());
    // Kept: gd-game's `Achievements` (the engine's AchievementManager lives for the process, `gd_game::achievements`).
    world.insert_resource(hud::LevelUpIndicator::default());
    world.insert_resource(menufolder::MenuFolder::default());
    world.insert_resource(riftmap::RiftMapView::default());
    world.insert_resource(lootfilter::LootFilterView::default());
    world.insert_resource(crate::inventory::UiCapture::default());
    world.insert_resource(crate::hover::HoverPoint::default());
    world.insert_resource(cursor::GameCursor::default());
    world.insert_resource(apply::ApplyView::default());
    world.insert_resource(apply::InvConfirm::default());
    world.insert_resource(devotion::DevotionView::default());
    world.insert_resource(devotion::StarPulses::default());
    world.insert_resource(dialog::DialogLayout::default());
    world.insert_resource(shrine::ShrineLayout::default());
    world.insert_resource(gamemenu::Dim::default());
    world.insert_resource(options::OptionsWindow::default());
    world.insert_resource(options::OptionsLayout::default());
    world.insert_resource(crate::doll::Dolls::default());
    world.insert_resource(HudValues::default());
    world.insert_resource(HudVitals::default());
    world.insert_resource(questpopup::QuestPopup::default());
    world.insert_resource(crate::trade::TradeLayout::default());
    world.insert_resource(altar::AltarLayout::default());
    world.insert_resource(reclamation::NoPointsBox::default());
    world.insert_resource(hud::HudLayout::default());
    world.insert_resource(status::StatusLayout::default());
    world.insert_resource(credits::Credits::default());
    world.insert_resource(stack::StackWindow::default());
}
