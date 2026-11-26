//! In-game UI drawn from the game's own UI records (`gd_formats::ui`).
//!
//! Layout is computed in physical pixels with the engine's rules
//! ([`gd_formats::ui`]): UI scale factor from the resolution, window
//! alignment, `round(pos × scale)` for child positions and `texture size ×
//! scale` for bitmaps. Every widget becomes an absolutely positioned Bevy UI
//! node under one root, spawned in the engine's draw order.
//!
//! Blending: the engine blends UI in gamma space (its UNORM back buffer); so
//! does the port, and every UI shader writes colours and texels as given.
//!
//! The UI only reads gameplay state and sends request messages (AGENTS.md). Exception: presentation
//! options (the local map's zoom, the quest tracker's toggle) are written directly, as the options window does.
//!
//! The windows coupled to the binary's audio mixer, world streaming or menu scene stay in `grimdawn::ui`;
//! the presentation state they share with the input systems is [`state`].
#![allow(clippy::type_complexity)]
pub mod achievements;
pub mod altar;
pub mod apply;
pub mod armor_window;
pub mod challenge;
pub mod character;
pub mod cooldown;
pub mod cursor;
pub mod devotion;
pub mod dialog;
pub mod doll;
pub mod errormsg;
pub mod factions;
pub mod fade;
pub mod floating;
pub mod gamemenu;
pub mod hotbar_pick;
pub mod hud;
pub mod item_styles;
pub mod loading;
pub mod lootfilter;
pub mod menufolder;
pub mod newitems;
pub mod notify;
pub mod overhead;
pub mod pets;
pub mod potions;
pub mod questpopup;
pub mod quests;
pub mod reclamation;
pub mod rollover;
pub mod shrine;
pub mod skills;
pub mod slot_select;
pub mod stack;
pub mod state;
pub mod status;
pub mod tab_fields;
pub mod tooltip;
pub mod trade;
pub mod window;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::{Widget, WindowDef};
use gd_game_core::data::GameData;
use gd_ui_kit::{
    HudSet, LiveText, RootFrame, TextAt, Ui, UiAssets, UiInputSet, UiPlaySet, contains, cursor_px,
    retire_roots, set_image_src, set_node_rect, spawn_frame_root, spawn_hud_overlay, spawn_root,
};
use gd_ui_kit::{additive, confirm, keys, masked, text, tip, widgets};
use text::FontCache;

/// The player state the HUD and windows show, assembled each frame by [`view::collect`] (empty until the
/// player exists), published field by field where it changed. The values that change most frames are in
/// [`HudVitals`].
#[derive(Resource, Clone, PartialEq)]
pub struct HudValues {
    /// Fraction of the current level's experience, 0..1.
    pub experience: f32,
    /// The Shattered Realm's floor progress of the player's region, 0..=1 (`Stat::EndlessProgress`).
    pub endless_progress: f32,
    /// `GetExperiencePoints` / `GetNextLevelExperience` / `IsMaxLevel`: the experience bar's rollover.
    pub experience_points: u64,
    pub experience_next: u64,
    pub max_level: bool,
    pub level: u32,
    pub gold: u64,
    pub name: String,
    /// Character-window numbers by field name without the `tabN` prefix
    /// (e.g. `StrengthNumber`); missing ones show 0.
    pub stats: std::collections::BTreeMap<String, String>,
    /// The stats rows' rollover values by the same keys.
    pub rollovers: std::collections::BTreeMap<String, gd_sim::sheet::RollValues>,
    /// The armour row's breakdown window.
    pub armor: gd_sim::sheet::ArmorBreakdown,
    /// The left / right mouse skill's damage lines: the page II LMB / RMB rollovers.
    pub skill_breakdown: [Vec<(bool, gd_sim::attributes::CombatType, f32, f32)>; 2],
    pub attribute_points: u32,
    pub skill_points: u32,
    /// The class trees whose skills-window pane has its undo button enabled.
    pub skill_undo: Vec<String>,
    pub reclaim: Option<reclamation::ReclaimView>,
    pub devotion_reclaim: Option<reclamation::DevotionReclaimView>,
    pub devotion_undo: bool,
    pub skills: gd_sim::skills::SkillBook,
    /// Hotbar icons (bitmap paths) by save slot: 0–9 keys 1–0, 10/11 mouse.
    pub hotbar: Vec<Option<String>>,
    /// Hotbar row shown on the number slots (0, or 1 while Ctrl is held).
    pub hotbar_row: usize,
    /// The hot slots' key labels by HUD master field; a rebind is a layout change.
    pub hotbar_labels: std::collections::BTreeMap<&'static str, String>,
    /// Each living pet's portrait buff icons.
    pub pet_buffs: Vec<(Entity, Vec<String>)>,
    /// The hovered character's debuff icons (`monsterBarsDebuffs`).
    pub target_debuffs: Vec<status::TargetDebuff>,
    pub teleport: Option<gd_game::riftgates::PortalButton>,
    /// Set II was chosen once in the character window.
    pub weapon_swap: bool,
    /// `""` = unused slot.
    pub mode_tags: [&'static str; 2],
    /// The map's name sector at the player.
    pub area_tag: String,
    /// The challenge area the player stands in, when it shows the HUD.
    pub challenge_area: Option<challenge::ChallengeHud>,
    /// Save order.
    pub factions: Vec<gd_formats::save::gdc::Faction>,
    pub menu_folder: menufolder::FolderView,
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
            skill_undo: Vec::new(),
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

/// The player values that change most frames, written in place by [`view::collect`]; their readers
/// animate in place.
#[derive(Resource, Clone, PartialEq)]
pub struct HudVitals {
    pub health: f32,
    pub health_max: f32,
    pub energy: f32,
    pub energy_max: f32,
    /// `Character::GetReserveMana`.
    pub energy_reserved: f32,
    /// `GetCurrentConstitution` / `GetConstitutionLimit`.
    pub constitution: f32,
    pub constitution_max: f32,
    pub status: Vec<status::StatusIcon>,
    pub cooldowns: Vec<cooldown::SlotCooldown>,
    /// The player's fixed duration-damage entry of type 61 (`DamageMultiplier`; 0: none); nonzero shows the
    /// "sundered" overlay.
    pub damage_taken_debuff: f32,
    /// `(life + PotionOverTime.life) / life_max` clamped to 0..1 (`Character::GetPotentialLife`).
    pub health_gain: f32,
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

#[derive(Resource, Default, Clone, PartialEq)]
pub struct UiPanels {
    pub character: bool,
    /// Skills window and its tab (1-based).
    pub skills: bool,
    pub skill_tab: u8,
    /// The mastery picked on each slot's selection pane: its pane shows in that slot until undone there.
    pub mastery_picks: [Option<String>; 2],
    /// The mastery (`masteryMasteryButtons` index) the free slot's selection pane describes: the last hovered.
    pub mastery_hover: Option<u8>,
    /// Character stats tab 1..=4.
    pub tab: u8,
    pub devotion: bool,
    /// Scroll offset (UI units) of the page II / III lists, by page.
    pub sheet_scroll: [f32; 4],
    /// Header field names.
    pub sheet_collapsed: Vec<String>,
}

impl UiPanels {
    /// Equal apart from the page II / III scroll (the lists scroll in place).
    fn same_but_scroll(&self, o: &Self) -> bool {
        let Self {
            character,
            skills,
            skill_tab,
            mastery_picks: _,
            mastery_hover: _,
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

/// The HUD's separately built root groups, each with its own signature. Stacked in this order
/// ([`ZIndex`] on the root), under the overlays that carry a `GlobalZIndex`.
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

/// A rebuilt UI tree waiting one frame (hidden) for layout and glyphs before replacing its group's tree.
#[derive(Component)]
pub struct UiFresh;

/// `rebuild` runs after this and spawns at most one root per group per frame.
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

/// What each group's last build read; a group rebuilds when its own signature differs.
#[derive(Resource, Default)]
struct Built {
    hud: Option<HudSig>,
    character: Option<CharacterSig>,
    trade: Option<TradeSig>,
    skills: Option<SkillsSig>,
    devotion: Option<DevotionSig>,
}

struct Live<'a> {
    /// The window size and the UI scale's bits.
    size: (UVec2, u32),
    /// `FontCache::generation`: tag-resolved labels are baked at build time.
    generation: u64,
    panels: &'a UiPanels,
    values: &'a HudValues,
    inventory: u64,
    trade: u64,
    trade_open: bool,
    /// The open trade window's gold-derived looks (`trade::afford`); gold itself is not a signature input.
    trade_afford: (bool, bool),
    potions: bool,
    loot_button: lootfilter::HudButton,
    menu_down: [bool; 2],
    devotion: (u64, u64),
    classic_hud: bool,
}

impl Live<'_> {
    fn points(&self) -> (u32, u32) {
        (self.values.attribute_points, self.values.skill_points)
    }
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
    generation: u64,
    level: u32,
    points: (u32, u32),
    teleport: Option<gd_game::riftgates::PortalButton>,
    labels: ([&'static str; 2], String),
    row: usize,
    skills: gd_sim::skills::SkillBook,
    hotbar: Vec<Option<String>>,
    hotbar_labels: std::collections::BTreeMap<&'static str, String>,
    loot_button: lootfilter::HudButton,
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
            generation: l.generation,
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
            && self.generation == l.generation
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
            && self.skills.same(&v.skills)
    }
}

/// Gold and the stats numbers are not here: the window redraws them in place.
#[derive(PartialEq)]
struct CharacterSig {
    size: (UVec2, u32),
    generation: u64,
    panels: UiPanels,
    docking: (bool, bool, bool, u64),
    level: u32,
    inventory: u64,
    points: (u32, u32),
    skills: gd_sim::skills::SkillBook,
    /// The mouse skills' names on page II (`list.SkillTag0/1`): drawn only at build.
    skill_tags: [Option<String>; 2],
}

impl CharacterSig {
    fn of(l: &Live) -> Self {
        let v = l.values;
        Self {
            size: l.size,
            generation: l.generation,
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
        if !self.panels.character && !l.panels.character && self.size == l.size {
            return true;
        }
        self.size == l.size
            && self.generation == l.generation
            && self.docking == l.docking()
            && self.level == v.level
            && self.inventory == l.inventory
            && self.points == l.points()
            && self.panels.same_but_scroll(l.panels)
            && self.skills.same(&v.skills)
            && self
                .skill_tags
                .iter()
                .map(Option::as_ref)
                .eq(Self::skill_tags(v))
    }
}

/// Gold is not here; the gold-derived looks baked at build are keyed as the `afford` pair.
#[derive(PartialEq)]
struct TradeSig {
    size: (UVec2, u32),
    generation: u64,
    open: bool,
    docking: (bool, bool, bool, u64),
    level: u32,
    afford: (bool, bool),
    inventory: u64,
}

impl TradeSig {
    fn of(l: &Live) -> Self {
        Self {
            size: l.size,
            generation: l.generation,
            open: l.trade_open,
            docking: l.docking(),
            level: l.values.level,
            afford: l.trade_afford,
            inventory: l.inventory,
        }
    }
    fn same(&self, l: &Live) -> bool {
        if !self.open && !l.trade_open && self.size == l.size {
            return true;
        }
        self.size == l.size
            && self.generation == l.generation
            && self.open == l.trade_open
            && self.docking == l.docking()
            && self.level == l.values.level
            && self.afford == l.trade_afford
            && self.inventory == l.inventory
    }
}

#[derive(PartialEq)]
struct SkillsSig {
    size: (UVec2, u32),
    generation: u64,
    open: (bool, u8, bool),
    level: u32,
    points: u32,
    picks: [Option<String>; 2],
    hover: Option<u8>,
    undo: Vec<String>,
    skills: gd_sim::skills::SkillBook,
    /// Reclamation mode and, in it, the money.
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
            generation: l.generation,
            open: Self::open(l),
            level: v.level,
            points: v.skill_points,
            picks: l.panels.mastery_picks.clone(),
            hover: l.panels.mastery_hover,
            undo: v.skill_undo.clone(),
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
            && self.generation == l.generation
            && self.open == Self::open(l)
            && self.level == v.level
            && self.points == v.skill_points
            && self.picks == l.panels.mastery_picks
            && self.hover == l.panels.mastery_hover
            && self.undo == v.skill_undo
            && self.skills.same(&v.skills)
            && self.reclaim.as_ref().map(|(r, g)| (r, *g)) == Self::reclaim(v)
    }
}

#[derive(PartialEq)]
struct DevotionSig {
    size: (UVec2, u32),
    generation: u64,
    open: bool,
    version: (u64, u64),
    undo: bool,
    skills: gd_sim::skills::SkillBook,
    /// The reclaim mode and, in it, the money.
    reclaim: Option<(reclamation::DevotionReclaimView, u64)>,
}

impl DevotionSig {
    fn reclaim(v: &HudValues) -> Option<(&reclamation::DevotionReclaimView, u64)> {
        v.devotion_reclaim.as_ref().map(|r| (r, v.gold))
    }
    fn open(l: &Live) -> bool {
        l.panels.devotion
    }
    fn of(l: &Live) -> Self {
        Self {
            size: l.size,
            generation: l.generation,
            open: Self::open(l),
            version: l.devotion,
            undo: l.values.devotion_undo,
            skills: l.values.skills.clone(),
            reclaim: Self::reclaim(l.values).map(|(r, g)| (r.clone(), g)),
        }
    }
    fn same(&self, l: &Live) -> bool {
        if !self.open && !l.panels.devotion && self.size == l.size {
            return true;
        }
        self.size == l.size
            && self.generation == l.generation
            && self.open == Self::open(l)
            && self.version == l.devotion
            && self.undo == l.values.devotion_undo
            && self.skills.same(&l.values.skills)
            && self.reclaim.as_ref().map(|(r, g)| (r, *g)) == Self::reclaim(l.values)
    }
}

/// The HUD rebuild chain, inside [`HudSet`]; the binary's `view::collect` runs before it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HudBuildSet;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            additive::AdditivePlugin,
            masked::MaskedPlugin,
            apply::ApplyUiPlugin,
            devotion::DevotionUiPlugin,
            dialog::DialogUiPlugin,
            quests::QuestUiPlugin,
            challenge::ChallengeUiPlugin,
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
            notify::NotifyPlugin,
            questpopup::QuestPopupPlugin,
            stack::StackUiPlugin,
            menufolder::MenuFolderPlugin,
        ))
        .add_plugins((
            gamemenu::GameMenuPlugin,
            cursor::CursorPlugin,
            loading::LoadingPlugin,
            doll::DollUiPlugin,
        ))
        .configure_sets(
            Update,
            UiPlaySet.run_if(in_state(crate::state::session::AppState::InGame)),
        )
        .configure_sets(
            PostUpdate,
            UiPlaySet.run_if(in_state(crate::state::session::AppState::InGame)),
        )
        .configure_sets(
            Update,
            (
                (
                    UiInputSet
                        .after(gd_game_core::inventory::InventorySet)
                        .before(gd_game_core::player::PlayerSet),
                    HudSet.after(gd_game_core::inventory::InventorySet),
                )
                    .in_set(UiPlaySet),
                HudBuildSet.in_set(HudSet),
            ),
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
        .add_systems(PreStartup, keys::load)
        .add_systems(
            PreUpdate,
            keys::sync_mode.run_if(resource_exists::<gd_game_core::options::GameOptions>),
        );
        #[cfg(feature = "dev")]
        app.add_systems(PreStartup, open_panels);
        app
            // Before the windows that reset the mouse.
            .add_systems(
                PreUpdate,
                (widgets::play_button_sounds, keys::mouse_codes)
                    .after(bevy::input::InputSystems)
                    .before(gamemenu::GameMenuInputSet)
                    .before(crate::state::options::OptionsInputSet),
            )
            .add_systems(Update, load_tags.before(HudSet))
            .add_systems(
                Update,
                (
                    // After the window clicks (a modal box answers Esc first) and the pet input.
                    esc_dispatcher
                        .after(window::WindowClicks)
                        .after(crate::state::pets::PetInputSet),
                    (hud::capture, hud::buttons).chain(),
                )
                    .in_set(gd_ui_kit::UiInputSet),
            )
            .add_systems(
                Update,
                gd_game::pause::apply_pause_key
                    .after(UiInputSet)
                    .before(gd_game_core::player::PlayerSet),
            )
            .add_systems(
                Update,
                trade::live_gold
                    .after(HudSet)
                    .after(crate::state::trade::TradeInputSet)
                    .in_set(UiPlaySet),
            )
            .add_systems(
                Update,
                (
                    (
                        swap_fresh,
                        toggle_panels,
                        rebuild,
                        hud::button_states,
                        hud::hot_slot_states,
                        character::scroll_sheet,
                        hud::animate_bars,
                        hud::flash_bars,
                    )
                        .chain()
                        .in_set(HudBuildSet),
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
                        hud::fps_overlay,
                    )
                        .chain()
                        .in_set(HudSet),
                )
                    .chain(),
            );
    }
}

#[cfg(feature = "dev")]
fn open_panels(mut commands: Commands, hooks: Res<gd_game_core::hooks::Hooks>) {
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

/// Reloads the tags (dropping fonts and styles) at startup and whenever the mounted mod or language
/// changes; bumps `FontCache::generation` and clears the hot slots' key labels for `view::collect` to refill.
pub fn load_tags(
    data: Res<GameData>,
    mut fonts: ResMut<FontCache>,
    mut values: ResMut<HudValues>,
    mut loaded: Local<Option<(Option<String>, String)>>,
) {
    let active = data.active_mod();
    let lang = data.language();
    let same = loaded
        .as_ref()
        .is_some_and(|(m, l)| m.as_deref() == active && l == &lang);
    if !same {
        *loaded = Some((active.map(str::to_owned), lang));
        fonts.load_tags(&data);
        values.hotbar_labels.clear();
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
}

/// The one Escape dispatcher (the engine routes a key to the focused window only: `InGameUI::HandleKeyAction`
/// case 52, `CursorHandlerEnchant::Escape`; INFERENCE Esc follows the same focus model): the open modals
/// front-to-back, then the focus stack of open windows; with nothing open it opens the game menu. The
/// PreUpdate input systems that swallow the keyboard leave Escape alive for it ([`keys::reset_but_escape`]).
#[allow(clippy::too_many_arguments)]
fn esc_dispatcher(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut windows: window::Windows,
    mut options: ResMut<crate::state::options::OptionsWindow>,
    inv: Res<gd_game_core::inventory::Inventory>,
    mut confirm: ResMut<apply::InvConfirm>,
    mut reclaim: ResMut<reclamation::NoPointsBox>,
    mut pick: ResMut<hotbar_pick::PickView>,
    mut stack: ResMut<stack::StackWindow>,
    mut devotion: ResMut<devotion::DevotionView>,
    mut potions: ResMut<potions::PotionsView>,
    mut pets: ResMut<pets::PetMenu>,
    (armed, over): (
        Res<gd_game::hotbar::ArmedSlot>,
        Res<crate::state::death::GameOverBox>,
    ),
    (
        mut apply_out,
        mut destroy_out,
        mut use_out,
        mut disarm_out,
        mut prompt_out,
        mut exit,
        mut pause_out,
    ): (
        MessageWriter<gd_game_core::inventory::ApplyRequest>,
        MessageWriter<gd_game::item_drop::DestroyAnswer>,
        MessageWriter<gd_game::consumables::UseAnswer>,
        MessageWriter<gd_game::hotbar::DisarmHotSlot>,
        MessageWriter<crate::state::trade::DismissPrompt>,
        MessageWriter<crate::state::exit::ExitToMenu>,
        MessageWriter<gd_game_core::pause::PauseKey>,
    ),
    (mut fresh, mut focus, mut was): (
        gd_game_core::session::NewSession,
        Local<Vec<window::Window>>,
        Local<[bool; window::Window::COUNT]>,
    ),
) {
    if fresh.begun() {
        focus.clear();
        *was = Default::default();
    }
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
        // The hardcore Game Over box: Esc leaves the session.
        if over.is_open() {
            exit.write(crate::state::exit::ExitToMenu);
            break 'close true;
        }
        if windows.menu.confirm.is_some() {
            windows.menu.confirm = None;
            break 'close true;
        }
        // The options window and the game menu are drawn over every modal box below.
        if options.escape() {
            break 'close true;
        }
        if windows.menu.open {
            windows.close(window::Window::GameMenu);
            break 'close true;
        }
        // The trade window's dialog box or the Inventor's dismantle warning: Esc answers No.
        if windows.trade.prompt.is_some() || windows.trade.confirm {
            prompt_out.write(crate::state::trade::DismissPrompt);
            break 'close true;
        }
        if *confirm != apply::InvConfirm::None {
            *confirm = apply::InvConfirm::None;
            break 'close true;
        }
        if inv.destroying.is_some() {
            destroy_out.write(gd_game::item_drop::DestroyAnswer(false));
            break 'close true;
        }
        if gd_game::consumables::use_question(&inv).is_some() {
            use_out.write(gd_game::consumables::UseAnswer(false));
            break 'close true;
        }
        if inv.applying.as_ref().is_some_and(|a| a.confirm.is_some()) {
            apply_out.write(gd_game_core::inventory::ApplyRequest::Answer(false));
            break 'close true;
        }
        if reclaim.escape() {
            break 'close true;
        }
        if windows.loot.escape() {
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
        // The apply mode itself: Esc ends it, the window stays.
        if inv.applying.is_some() {
            apply_out.write(gd_game_core::inventory::ApplyRequest::Cancel);
            break 'close true;
        }
        // An armed hot slot drops before any window closes (`UIPlayerHud::HandleEscape`).
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
        // Nothing open: the game menu (it replaces the pause key's pause).
        windows.menu.open = true;
        pause_out.write(gd_game_core::pause::PauseKey(Some(false)));
    }
    keys.clear_just_pressed(KeyCode::Escape);
}

/// The x span of the window beside the character window, only while it is open.
fn docked(
    panels: &UiPanels,
    trade: &crate::state::trade::Trade,
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
        Some((pr.x, pr.x + pr.w))
    } else if !panels.skills
        && let Some(cm) = data.db.get(character::MASTER)
    {
        // PORT: a lone character window opens centred (the record's `windowScreenAlignmentX Right` is only
        // used beside the skills window).
        let cw = WindowDef::from_record(cm).screen_rect(screen.x, screen.y, s);
        Some((0.0, ((screen.x - cw.w) * 0.5).max(0.0).round()))
    } else {
        None
    }
}

/// Builds the groups whose signature changed; a closed window's group is despawned and its layout cache reset.
#[allow(clippy::too_many_arguments)]
fn rebuild(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    panels: Res<UiPanels>,
    (values, vitals): (Res<HudValues>, Res<HudVitals>),
    mut built: ResMut<Built>,
    roots: Query<(Entity, &UiRoot)>,
    inv: Res<gd_game_core::inventory::Inventory>,
    reqs: Query<&gd_game_core::inventory::Requirements, With<gd_game_core::player::Player>>,
    trade: Res<crate::state::trade::Trade>,
    markets: Res<gd_game::trade::Markets>,
    stash: Option<Res<gd_game_core::stash::Stash>>,
    rules: Option<Res<gd_game_core::rules::CombatRules>>,
    (craft, devotion, mut view, potions_open, loot_view, items_hidden, factions): (
        Option<Res<crate::state::crafting::CraftList>>,
        Option<Res<gd_game::devotion::DevotionState>>,
        ResMut<devotion::DevotionView>,
        Res<gd_game::cauldron::PotionWindow>,
        Res<lootfilter::LootFilterView>,
        Res<crate::state::loot::ItemsHidden>,
        (
            Res<factions::FactionsView>,
            Res<achievements::AchievementsView>,
        ),
    ),
    mut assets: UiAssets,
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
) {
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if size.x == 0 || size.y == 0 {
        return;
    }
    let loot_button = lootfilter::HudButton::of(&loot_view, &items_hidden);
    let live = Live {
        size: (size, assets.scale(size).to_bits()),
        generation: assets.fonts.generation,
        panels: &panels,
        values: &values,
        inventory: inv.version,
        trade: trade.version,
        trade_open: trade.open.is_some(),
        trade_afford: trade::afford(
            &data,
            &trade,
            stash.as_deref(),
            craft.as_deref(),
            (values.gold, values.level),
            &inv,
        ),
        potions: potions_open.open,
        loot_button,
        menu_down: [factions.0.open, factions.1.open],
        devotion: (devotion.as_ref().map_or(0, |d| d.version), view.version),
        classic_hud: assets.classic_hud(),
    };
    let s = assets.scale(size);
    let screen = Vec2::new(size.x as f32, size.y as f32);
    let spawn = |commands: &mut Commands, g: Group| {
        // Even local z: the odd one between `Hud` and `Character` is [`HUD_OVERLAY_Z`].
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
            commands.insert_resource(crate::state::inventory::InvLayout::default());
        }
        rebuilt.push("character");
    }
    if !built.trade.as_ref().is_some_and(|b| b.same(&live)) {
        built.trade = Some(TradeSig::of(&live));
        if trade.open.is_some() {
            let root = spawn(&mut commands, Group::Trade);
            let mut ui = ui!(root);
            ui.docked = docked(&panels, &trade, potions_open.open, &data, screen, s);
            #[cfg(feature = "dev")]
            let reroll_test = hooks.items.reroll.is_some();
            #[cfg(not(feature = "dev"))]
            let reroll_test = false;
            trade::build(
                &mut ui,
                &trade,
                &markets,
                stash.as_deref(),
                craft.as_deref(),
                (values.gold, values.level),
                Some(&inv),
                live.trade_afford,
                reroll_test,
            );
        } else {
            retire(&mut commands, Group::Trade);
            commands.insert_resource(crate::state::trade::TradeLayout::default());
        }
        rebuilt.push("trade");
    }
    if !built.skills.as_ref().is_some_and(|b| b.same(&live)) {
        built.skills = Some(SkillsSig::of(&live));
        if panels.skills {
            let root = spawn(&mut commands, Group::Skills);
            skills::build(
                &mut ui!(root),
                &values,
                panels.skill_tab,
                &panels.mastery_picks,
                panels.mastery_hover,
            );
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
                    // Not a view change: devotion.rs readers key on `version`.
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

/// Resets the HUD and window state a session leaves.
pub fn reset_session(world: &mut World) {
    world.insert_resource(hud::HotbarRow::default());
    world.insert_resource(UiPanels::default());
    world.insert_resource(Built::default());
    world.insert_resource(crate::state::map::MapWindow::default());
    world.insert_resource(errormsg::ErrorMessage::default());
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
    // Kept: gd-game's `Achievements` (the engine's AchievementManager lives for the process).
    world.insert_resource(hud::LevelUpIndicator::default());
    world.insert_resource(menufolder::MenuFolder::default());
    world.insert_resource(crate::state::riftmap::RiftMapView::default());
    world.insert_resource(lootfilter::LootFilterView::default());
    world.insert_resource(crate::state::inventory::UiCapture::default());
    world.insert_resource(crate::state::hover::HoverPoint::default());
    world.insert_resource(cursor::GameCursor::default());
    world.insert_resource(apply::ApplyView::default());
    world.insert_resource(apply::InvConfirm::default());
    world.insert_resource(devotion::DevotionView::default());
    world.insert_resource(devotion::StarPulses::default());
    world.insert_resource(dialog::DialogLayout::default());
    world.insert_resource(shrine::ShrineLayout::default());
    world.insert_resource(gamemenu::Dim::default());
    world.insert_resource(crate::state::options::OptionsWindow::default());
    world.insert_resource(crate::doll::Dolls::default());
    world.insert_resource(HudValues::default());
    world.insert_resource(HudVitals::default());
    world.insert_resource(questpopup::QuestPopup::default());
    world.insert_resource(crate::state::trade::TradeLayout::default());
    world.insert_resource(altar::AltarLayout::default());
    world.insert_resource(reclamation::NoPointsBox::default());
    world.insert_resource(hud::HudLayout::default());
    world.insert_resource(status::StatusLayout::default());
    world.insert_resource(stack::StackWindow::default());
}
