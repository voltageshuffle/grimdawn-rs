//! Main menu state and input: pick a character from the port's save folder
//! (`save.rs` `save_root()/{main,user}/_<name>/player.gdc`; never the game's own
//! saves), create or delete one, choose the difficulty, then start the
//! session (`session::begin`).
//! The screens are drawn by `ui/mainmenu.rs` from [`MainMenu`]; clicks hit
//! the rects it leaves in [`MenuLayout`].
//!
//! Difficulties ([`DifficultyOption`]): Normal, Elite, Ultimate up to the
//! character's `greatest_difficulty` (3 counts as 0, as
//! `GameEngine::CompleteDifficulty` reads it). Beating the game
//! on Normal unlocks both Elite and Ultimate (`GameEngine::GameWon`;
//! Lua `Game.UnlockNextDifficulty`).
//! `greatest_difficulty` in the save is that unlocked level and new
//! characters start on Normal: the byte is
//! `Player::GetGreatestDifficultyCompleted` (`Player.greatestDifficultyCompleted`, raised by
//! `CompleteDifficulty`) and the exe's new-character path sets it to 0. The
//! difficulty window's picks (`DifficultyWindow::ApplySelection`) and enables (`DifficultyWindow::UpdateEnabled`)
//! are in [`DifficultyOption::enabled`].
//!
//! Character list (`playmenu_characterlist.dbr`, `Grim Dawn.exe`
//! `CharacterList::Update` / draw `CharacterList::Draw`): at most `maxCharacters` rows from the
//! first visible one; an unset selection becomes the last entry. PORT: the
//! list is sorted by name ignoring case (the reference shot's order; the
//! engine's list builder `CharacterList::Build` is not decompiled) and the most
//! recently written character is selected (the game remembers the last
//! played one).
//!
//! Create Character (`CreateCharacterWindow::HandleEvent`): an empty name says
//! `tagMenuError07`, a name an existing character has (ignoring case) says
//! `tagMenuError38`; otherwise the male / female radio and the hardcore box
//! set the new character; Next (`tagMenuButton12`) opens the difficulty
//! window (`difficultyWindow`, the same record as the play menu's) and its Accept writes the character's
//! save (`gd_game::save::create_character`) with the picked option, then returns to the list with the new
//! character selected — the player presses Start themselves; a failed write stays in the window. Cancel
//! goes back to the name step. Names are at most
//! `maxPlayerNameLength`
//! (mainmenu_constants.dbr, 20) characters. PORT: with no characters (at startup, or after deleting the
//! last one) the Create window opens by itself — the game shows an empty play menu; Cancel / Esc still
//! close it. Delete Character asks to type
//! `tagMenuDeleteConfirm` ("DELETE"; PORT: compared ignoring case) and removes
//! the folder, only inside the port's save folder.
//!
//! Keys (PORT: the port's; the main menu's key handling is not decompiled): Up/Down choose, Enter play (or accept a
//! window), Esc closes a window, Delete opens the delete window, N the create window. Clicks activate on the left
//! press, the `menubutton` class's rule as every button's (`ui::widgets::button_pressed`,
//! `MenuButton::HandleMouseEvent`: mouse event 1 dispatches the listeners' event 0; the release only sets the up
//! look). While the
//! menu is open no input reaches the game. Out of scope (single player
//! only): Multiplayer, Game Guide, Community, DLC are drawn disabled; Credits is the roll of `ui/credits.rs`.
//! The Crucible (`GameModeWindow::HandleEvent`, `GameModeWindow::Accept`): the window sets the
//! mode and the arena (Random or one offered by the installed packs), the difficulty window then lists
//! Aspirant / Challenger / Gladiator and Start mounts the Crucible data and opens the arena
//! (`world::open_session_map`).
//! Custom (mode 2, `GameModeWindow::Load` / toggles `GameModeWindow::HandleToggle` / view
//! `GameModeWindow::ShowCustomView` / Accept `GameModeWindow::AcceptCustom`): the toggle is always offered; it
//! shows the map list of
//! the user mods ([`custom_maps`], read again each time) with the selected map's file name above it; Accept
//! with an empty list falls back to the Main Campaign, else the mode is 2 with the selected mod and map
//! (`GameInfo::SetModName` / `SetLevelName`). The stock install has no user mod, so there Custom always ends
//! as the campaign. Start in mode 2 mounts the user mod over the base game (`Engine::InitializeMod`:
//! `gd_formats::vfs`), opens its map (`world::open_session_map`) and begins the session; a character created
//! there is not in the main quest and is saved under `save/user/` (`Choice::player_folder`), its shared files
//! under `save/<mod>/` (`Choice::shared_save_dir`). A mod that does not mount, or a listed map its archives do
//! not hold (a loose `.map`: `InitializeMod` mounts only `resources/*.arc`), refuses Start and the menu stays.
//!
//! Characters (`Game::ListCharacters`): the list holds the characters of `main/` and `user/`. Start in the
//! campaign (mode 0, no mod) with a character that is not in the main quest (`is_in_main_quest` 0: created in
//! the Crucible or a custom game) is refused. Start (`MainMenu::StartGame`) passes `GameInfo::GetModName`
//! to the save creation `Game::CreatePlayer`, which sets `Player::SetIsInMainQuest(modName empty)` on a new
//! character and fails for an existing one when `modName` is empty and `!IsInMainQuest`; the Crucible's mode 1
//! has `modName` `survivalmode` (`MainMenu::SetMapData`), so a Crucible-made character is not in the main quest. The
//! loader `Game::LoadCharacter` refuses it in the same way (`IsInMainQuest` required when no mod is set).
//!
//! What the menu remembers (`playmenu.cpn`, reader `MainMenu::ReadPlayMenu` / writer `MainMenu::WritePlayMenu`,
//! layout in
//! `gd_formats::save::cpn`): the selected character, the game mode, the difficulty option and Veteran, and the
//! Crucible arena. Read when the menu opens (a name that no longer exists leaves the default selection), written
//! when one of them changes. PORT: the file is the port's own `<save dir>/playmenu.cpn` (the engine's shared save
//! slot 2); only those fields are touched, the rest of an existing file (multiplayer fields, mod names) is kept;
//! the Random arena is stored as "none" (the engine stores the index its `rand()` resolved); mode 2 stores the mod
//! and map names (`mod_name`, `level_name2`), and a file whose custom map is no longer listed opens as the
//! campaign. A hook that sets the menu (`GD_MENU_PICK`, `GD_MENU_CRUCIBLE`, hooks.rs) neither reads nor writes
//! the file.
use crate::session::begin;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::save::gdc::Gdc;
use gd_formats::ui::Rect as URect;
use gd_game_core::session::Choice;
pub use gd_game_core::session::{CustomMap, DifficultyOption};

pub const CONSTANTS: &str = "records/ui/mainmenu/mainmenu_constants.dbr";

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        // A launch that skips the menu (a character or test session given) never shows it at startup, but the
        // systems stay: Exit to Main Menu (`exit.rs`) brings it up later.
        // Before the game's systems: the menu takes the keyboard and mouse
        // (`swallow_input`), so typing a name moves nothing and opens no
        // window. The options window opened from the menu reads its own
        // input in between.
        app.init_resource::<MenuLayout>()
            .add_systems(
                Startup,
                setup.run_if(|h: Res<gd_game_core::hooks::Hooks>| !h.session.skip_menu),
            )
            .add_systems(
                PreUpdate,
                (
                    run_menu
                        .after(bevy::input::InputSystems)
                        .before(gd_ui::state::options::OptionsInputSet),
                    swallow_input.after(gd_ui::state::options::OptionsInputSet),
                )
                    .run_if(resource_exists::<MainMenu>),
            );
    }
}

pub struct Entry {
    pub path: std::path::PathBuf,
    pub gdc: Gdc,
}

/// The open window over the main screen.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuWindow {
    None,
    /// The difficulty window with its highlighted option.
    Difficulty(DifficultyOption),
    Create(NewCharacter),
    /// Create Character's second step: the difficulty window for the validated new character.
    NewDifficulty(NewCharacter, DifficultyOption),
    /// The game-mode window (Campaign / Crucible, the arena drop-down): the pick being edited.
    GameType(GameType),
    /// Delete Character with the confirmation typed so far.
    Delete(String),
}

/// The game-mode window's edit (`gamemode_mastertable.dbr`, `GameModeWindow::HandleEvent`): the
/// Crucible toggle, the arena drop-down item (0 = Random, then [`gd_game_core::session::ARENAS`] order, as
/// offered) and whether the drop-down list is open; the Custom toggle, its map list (read when the
/// toggle shows it, [`custom_maps`]) and the list's selection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GameType {
    pub crucible: bool,
    pub arena: usize,
    pub open: bool,
    pub custom: bool,
    pub maps: Vec<CustomMap>,
    pub pick: usize,
}

impl GameType {
    /// The window as it opens on the menu's mode (`GameModeWindow::Open`: mode 2 shows the Custom view, its
    /// list read again and the entry of the current mod and map selected, else the first).
    fn open(m: &MainMenu, root: &std::path::Path) -> Self {
        let mut g = GameType {
            crucible: m.crucible,
            arena: m.arena,
            ..default()
        };
        if let Some(c) = &m.custom {
            g.show_custom(root, Some(c));
        }
        g
    }

    /// Shows the Custom view (`GameModeWindow::ShowCustomView`): the list is read again, the entry of `current`
    /// selected.
    fn show_custom(&mut self, root: &std::path::Path, current: Option<&CustomMap>) {
        (self.crucible, self.open, self.custom) = (false, false, true);
        self.maps = custom_maps(root);
        self.pick = current
            .and_then(|c| self.maps.iter().position(|e| e == c))
            .unwrap_or(0);
    }
}

/// The custom games the install offers (`MainMenu::ListCustomMods`): every folder of `<root>/mods/` but
/// `survivalmode` (case ignored), each with every `.map` of its `resources/` whose header is `MAP` and a
/// version byte above 5 (`World::GetTQ1MapFileVersionNumber`; a map that does not open is listed too), or
/// `levels/world001.map` when it has no map at all.
/// PORT: the engine lists the folders and maps in `FindFirstFile` / `FileSystem::GetMatchingFiles` order;
/// here both are sorted by name. A map is a loose file (its path under `resources/`) or an entry of one of
/// the folder's archives (`<archive name>/<entry>`, as the Crucible's `Maps.arc` holds `maps/…`).
pub fn custom_maps(root: &std::path::Path) -> Vec<CustomMap> {
    let mut dirs: Vec<String> = std::fs::read_dir(root.join("mods"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|d| d.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|d| d.file_name().into_string().ok())
        .filter(|n| !n.eq_ignore_ascii_case("survivalmode"))
        .collect();
    dirs.sort_by_key(|n| n.to_ascii_lowercase());
    let mut out = Vec::new();
    for module in dirs {
        let res = root.join("mods").join(&module).join("resources");
        let mut maps = Vec::new();
        mod_maps(&res, &res, &mut maps);
        maps.sort();
        if maps.is_empty() {
            maps.push("levels/world001.map".to_owned());
        }
        out.extend(maps.into_iter().map(|level| CustomMap {
            module: module.clone(),
            level,
        }));
    }
    out
}

/// The listed maps under `dir` (see [`custom_maps`]), as resource paths relative to `base`.
fn mod_maps(base: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
    let is_map =
        |h: Option<&[u8]>| h.is_none_or(|h| h.len() >= 4 && &h[..3] == b"MAP" && (h[3] as i8) > 5);
    for d in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = d.path();
        let ext = p
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase());
        if p.is_dir() {
            mod_maps(base, &p, out);
        } else if ext.as_deref() == Some("map") {
            let head = std::fs::File::open(&p).ok().and_then(|mut f| {
                let mut h = [0u8; 4];
                std::io::Read::read_exact(&mut f, &mut h).ok().map(|_| h)
            });
            if is_map(head.as_ref().map(|h| &h[..])) {
                let rel = p.strip_prefix(base).unwrap_or(&p);
                out.push(
                    rel.to_string_lossy()
                        .replace('\\', "/")
                        .to_ascii_lowercase(),
                );
            }
        } else if ext.as_deref() == Some("arc")
            && let Ok(arc) = gd_formats::arc::Arc::open(&p)
        {
            let stem = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            for e in arc
                .entries
                .iter()
                .filter(|e| e.name.to_ascii_lowercase().ends_with(".map"))
            {
                let head = arc.read(e).ok();
                if is_map(head.as_deref()) {
                    out.push(format!(
                        "{stem}/{}",
                        e.name.replace('\\', "/").to_ascii_lowercase()
                    ));
                }
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NewCharacter {
    pub name: String,
    pub female: bool,
    pub hardcore: bool,
    /// The error tag shown after a refused Create.
    pub error: Option<&'static str>,
}

#[derive(Resource)]
pub struct MainMenu {
    pub entries: Vec<Entry>,
    pub selected: usize,
    /// The first visible row.
    pub first: usize,
    pub option: DifficultyOption,
    /// The game mode (`GameInfo::SetMode`): the Crucible and its arena drop-down item (0 = Random), or a
    /// custom game (mode 2) and its map.
    pub crucible: bool,
    pub arena: usize,
    pub custom: Option<CustomMap>,
    pub window: MenuWindow,
    /// Bumped on every change (the screen rebuilds when it moves).
    pub revision: u32,
}

impl MainMenu {
    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }

    /// The Start button is enabled: a character is selected and it is not a dead hardcore one. The one rule
    /// for the button (`ui::mainmenu`) and for Enter / a pick, so Enter never starts what Start would not.
    pub fn can_start(&self) -> bool {
        self.current()
            .is_some_and(|e| !gd_game_core::session::is_dead(&e.gdc))
    }

    fn select(&mut self, i: usize, max_rows: usize) {
        self.selected = i.min(self.entries.len().saturating_sub(1));
        if self.selected < self.first {
            self.first = self.selected;
        } else if self.selected >= self.first + max_rows {
            self.first = self.selected + 1 - max_rows;
        }
        let crucible = self.crucible;
        self.option = self.current().map_or(DifficultyOption::Normal, |e| {
            DifficultyOption::of_save(&e.gdc).clamp(Some(&e.gdc), crucible)
        });
    }
}

/// A clickable part of the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Row(usize),
    ScrollUp,
    ScrollDown,
    Create,
    Delete,
    Difficulty,
    GameType,
    ModeCampaign,
    ModeCrucible,
    ModeCustom,
    /// A row of the custom maps' list.
    ModItem(usize),
    MapDrop,
    MapItem(usize),
    Start,
    Options,
    Credits,
    Exit,
    Pick(DifficultyOption),
    Accept,
    Cancel,
    Male,
    Female,
    Hardcore,
}

/// Hit rects of the built menu (physical px), written by `ui/mainmenu.rs`.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct MenuLayout {
    pub hits: Vec<(URect, Hit)>,
    /// Visible rows of the character list (`maxCharacters`).
    pub rows: usize,
}

impl MenuLayout {
    pub fn at(&self, p: Vec2) -> Option<Hit> {
        self.hits
            .iter()
            .rev()
            .find(|(r, _)| gd_ui_kit::contains(r, p))
            .map(|h| h.1)
    }
}

/// The folders of the port's characters (`Game::ListCharacters`: `Main/`, then `User/`).
const CHARACTER_FOLDERS: [&str; 2] = ["main", "user"];

/// The port's characters (of `main/` and `user/`), by name, and the most recently written one.
pub fn scan_characters(save_dir: &gd_game_core::session::SaveDir) -> (Vec<Entry>, usize) {
    let mut entries: Vec<(Entry, std::time::SystemTime)> = CHARACTER_FOLDERS
        .iter()
        .flat_map(|f| std::fs::read_dir(save_dir.join(f)))
        .flatten()
        .flatten()
        .map(|d| d.path().join("player.gdc"))
        .filter_map(|path| {
            let gdc = std::fs::read(&path)
                .ok()
                .and_then(|b| Gdc::parse(&b).ok())?;
            let t = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            Some((Entry { path, gdc }, t))
        })
        .collect();
    entries.sort_by_key(|(e, _)| e.gdc.header.name.to_lowercase());
    info!(
        "menu: {} characters in {}",
        entries.len(),
        save_dir.0.display()
    );
    let latest = entries
        .iter()
        .enumerate()
        .max_by_key(|(_, (_, t))| *t)
        .map_or(0, |(i, _)| i);
    (entries.into_iter().map(|(e, _)| e).collect(), latest)
}

/// The remembered-menu file (`playmenu.cpn`, the engine's shared save slot 2) of a save folder.
pub fn playmenu_path(save_dir: &gd_game_core::session::SaveDir) -> std::path::PathBuf {
    save_dir.0.join("playmenu.cpn")
}

impl MainMenu {
    /// `base` (the file as it was, or the defaults) with the menu's remembered fields set.
    /// `offered` = [`gd_game_core::session::available_arenas`].
    pub fn to_playmenu(
        &self,
        mut base: gd_formats::save::cpn::PlayMenu,
        offered: &[usize],
    ) -> gd_formats::save::cpn::PlayMenu {
        if let Some(e) = self.current() {
            base.character = e.gdc.header.name.clone();
        }
        let (d, vet) = self.option.mode();
        base.difficulty = d as u32;
        base.challenge = vet as u32;
        base.mode = match (self.crucible, &self.custom) {
            (true, _) => 1,
            (false, Some(c)) => {
                // The reader matches mode 2 by these two (`MainMenu::ReadPlayMenu`).
                (base.mod_name, base.level_name2) = (c.module.clone(), c.level.clone());
                2
            }
            _ => 0,
        };
        // PORT: Random is stored as no arena.
        base.survival_index = match self.arena {
            0 => u32::MAX,
            n => offered.get(n - 1).map_or(u32::MAX, |i| *i as u32),
        };
        base
    }

    /// Applies a remembered menu (reader: mode 1 re-selects the arena by index, mode 2 the custom
    /// map whose mod and map names match (`maps` = [`custom_maps`]; none listed: the campaign), the
    /// difficulty and the character come back as saved; a clamped option when the character may not pick it).
    pub fn restore(
        &mut self,
        pm: &gd_formats::save::cpn::PlayMenu,
        offered: &[usize],
        maps: &[CustomMap],
        rows: usize,
    ) {
        self.crucible = pm.mode == 1;
        // PORT: a mode 2 file whose map is no longer listed opens as the campaign (the engine keeps mode 2
        // with no map set when the list is not empty).
        self.custom = (pm.mode == 2)
            .then(|| {
                maps.iter()
                    .find(|c| c.module == pm.mod_name && c.level == pm.level_name2)
                    .cloned()
            })
            .flatten();
        self.arena = offered
            .iter()
            .position(|i| *i as u32 == pm.survival_index)
            .map_or(0, |p| p + 1);
        if let Some(i) = self
            .entries
            .iter()
            .position(|e| e.gdc.header.name == pm.character)
        {
            self.select(i, rows);
        }
        let g = self.current().map(|e| &e.gdc);
        let o = DifficultyOption::remembered(pm.difficulty, pm.challenge);
        if o.enabled_in(g, self.crucible) {
            self.option = o;
        }
    }
}

/// Reads the remembered menu (a missing or unreadable file is the defaults).
fn read_playmenu(
    save_dir: &gd_game_core::session::SaveDir,
) -> Option<gd_formats::save::cpn::PlayMenu> {
    let b = std::fs::read(playmenu_path(save_dir)).ok()?;
    gd_formats::save::cpn::PlayMenu::parse(&b)
        .map_err(|e| warn!("menu: playmenu.cpn: {e:#}"))
        .ok()
}

/// Writes the menu's remembered fields into `playmenu.cpn` (port-owned folder only, `write_user_file`).
fn write_playmenu(m: &MainMenu, root: &std::path::Path, save_dir: &gd_game_core::session::SaveDir) {
    let offered = gd_game_core::session::available_arenas(root);
    let pm = m.to_playmenu(read_playmenu(save_dir).unwrap_or_default(), &offered);
    match pm.encode() {
        Ok(b) => gd_game_core::session::write_user_file("menu", &playmenu_path(save_dir), b),
        Err(e) => error!("menu: playmenu.cpn: {e:#}"),
    }
}

pub(crate) fn setup(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    save_dir: Res<gd_game_core::session::SaveDir>,
    data: Res<gd_game_core::data::GameData>,
) {
    let (entries, selected) = scan_characters(&save_dir);
    // A hook that sets the menu leaves the remembered one alone.
    let hooked = hooks.ui.menu_crucible.is_some() || hooks.ui.menu_pick.is_some();
    let window = match hooks.ui.menu_open.clone().as_deref() {
        Some("difficulty") => MenuWindow::Difficulty(DifficultyOption::Normal),
        Some("create") => MenuWindow::Create(NewCharacter {
            name: hooks.ui.menu_name.clone().unwrap_or_default(),
            ..default()
        }),
        Some("newdifficulty") => MenuWindow::NewDifficulty(
            NewCharacter {
                name: hooks.ui.menu_name.clone().unwrap_or_default(),
                ..default()
            },
            DifficultyOption::Normal,
        ),
        Some("delete") => MenuWindow::Delete(String::new()),
        Some("gametype") => MenuWindow::GameType(GameType {
            crucible: hooks.ui.menu_crucible.is_some(),
            arena: hooks.ui.menu_crucible.unwrap_or(0),
            ..default()
        }),
        _ => MenuWindow::None,
    };
    let mut m = MainMenu {
        entries,
        selected: 0,
        first: 0,
        option: DifficultyOption::Normal,
        crucible: hooks.ui.menu_crucible.is_some(),
        arena: hooks.ui.menu_crucible.unwrap_or(0),
        custom: None,
        window,
        revision: 0,
    };
    m.select(selected, 12);
    if !hooked && let Some(pm) = read_playmenu(&save_dir) {
        let offered = gd_game_core::session::available_arenas(data.root());
        m.restore(&pm, &offered, &custom_maps(data.root()), 12);
    }
    // `GD_MENU_CUSTOM` (hooks.rs).
    if hooks.ui.menu_custom
        && let MenuWindow::GameType(g) = &mut m.window
    {
        g.show_custom(data.root(), None);
    }
    if let MenuWindow::Difficulty(o) = &mut m.window {
        *o = m.option;
    }
    // PORT: no characters opens the Create window (the game shows an empty play menu); a hook-set
    // window wins, and Cancel / Esc still close it.
    if m.entries.is_empty() && m.window == MenuWindow::None {
        m.window = MenuWindow::Create(NewCharacter::default());
    }
    commands.insert_resource(m);
}

#[allow(clippy::too_many_arguments)]
fn run_menu(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut typed: MessageReader<KeyboardInput>,
    mut menu: ResMut<MainMenu>,
    layout: Res<MenuLayout>,
    data: Res<gd_game_core::data::GameData>,
    (mut options, fonts, mut exit, mut buttons, mut credits): (
        ResMut<gd_ui::state::options::OptionsWindow>,
        Res<gd_ui_kit::text::FontCache>,
        MessageWriter<AppExit>,
        ResMut<ButtonInput<MouseButton>>,
        ResMut<crate::ui::credits::Credits>,
    ),
    save_dir: Res<gd_game_core::session::SaveDir>,
    (mut accepted, mut picked): (Local<bool>, Local<bool>),
    time: Res<Time<Real>>,
) {
    // The press (the shared rule): read before `swallow_input` resets this frame's `ButtonInput`.
    let pressed = gd_ui_kit::widgets::button_pressed(&buttons);
    if options.open || credits.open {
        typed.clear();
        return;
    }
    let before = (
        menu.selected,
        menu.first,
        menu.option,
        menu.window.clone(),
        (menu.crucible, menu.arena, menu.custom.clone()),
    );
    let cursor = window
        .single()
        .ok()
        .and_then(|w| gd_ui_kit::cursor_px(&hooks, w));
    let hit = cursor.filter(|_| pressed).and_then(|p| layout.at(p));
    // A press on a menu control is the menu's: the Options window, whose input runs next in this frame, must not
    // take it too (the press that opens it would land on whatever control lies under the cursor).
    if hit.is_some() {
        buttons.clear_just_pressed(MouseButton::Left);
    }
    let rows = layout.rows.max(1);
    let max_name = data
        .db
        .get(CONSTANTS)
        .and_then(|r| r.f32("maxPlayerNameLength"))
        .map_or(20, |v| v as usize);
    let m = &mut *menu;
    // `GD_MENU_ACCEPT` (hooks.rs).
    let hook = !*accepted && m.window != MenuWindow::None && hooks.ui.menu_accept;
    *accepted |= hook;
    let enter =
        keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) || hook;
    let esc = keys.just_pressed(KeyCode::Escape);
    match &mut m.window {
        MenuWindow::Create(new) => {
            for ev in typed.read().filter(|e| e.state.is_pressed()) {
                match &ev.logical_key {
                    Key::Character(c)
                        if new.name.chars().count() < max_name
                            && !c
                                .chars()
                                .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c)) =>
                    {
                        new.name.push_str(c.as_str())
                    }
                    Key::Space if !new.name.is_empty() && new.name.chars().count() < max_name => {
                        new.name.push(' ')
                    }
                    Key::Backspace => {
                        new.name.pop();
                    }
                    Key::Tab => new.female = !new.female,
                    _ => {}
                }
            }
            match hit {
                Some(Hit::Male) => new.female = false,
                Some(Hit::Female) => new.female = true,
                Some(Hit::Hardcore) => new.hardcore = !new.hardcore,
                _ => {}
            }
            if esc || hit == Some(Hit::Cancel) {
                m.window = MenuWindow::None;
            } else if (enter || hit == Some(Hit::Accept)) && !new.name.trim().is_empty() {
                let name = new.name.trim().to_owned();
                if m.entries
                    .iter()
                    .any(|e| e.gdc.header.name.eq_ignore_ascii_case(&name))
                {
                    new.error = Some("tagMenuError38");
                } else {
                    // Next: the difficulty step (`difficultyWindow`), opened on the menu's option.
                    let next = NewCharacter {
                        name,
                        error: None,
                        ..new.clone()
                    };
                    let pick = m.option.clamp(None, m.crucible);
                    m.window = MenuWindow::NewDifficulty(next, pick);
                }
            } else if enter {
                new.error = Some("tagMenuError07");
            }
        }
        MenuWindow::NewDifficulty(new, pick) => {
            typed.clear();
            if let Some(Hit::Pick(o)) = hit
                && o.enabled_in(None, m.crucible)
            {
                *pick = o;
            }
            if esc || hit == Some(Hit::Cancel) {
                // Back to the first step with what was typed.
                m.window = MenuWindow::Create(new.clone());
            } else if enter || hit == Some(Hit::Accept) {
                let (difficulty, veteran) = pick.mode();
                let choice = Choice {
                    save: None,
                    name: new.name.clone(),
                    female: new.female,
                    hardcore: new.hardcore,
                    survival: m.crucible.then(|| arena_map(data.root(), m.arena)),
                    custom: m.custom.clone().filter(|_| !m.crucible),
                };
                // Accept writes the save and returns to the list with the new character selected
                // (the player presses Start themselves); a failed write stays in the window.
                match gd_game::save::create_character(&save_dir, &choice, difficulty, veteran) {
                    Ok((path, gdc)) => {
                        m.entries.push(Entry { path, gdc });
                        m.entries.sort_by_key(|e| e.gdc.header.name.to_lowercase());
                        let i = m
                            .entries
                            .iter()
                            .position(|e| e.gdc.header.name.eq_ignore_ascii_case(&choice.name))
                            .unwrap_or(0);
                        m.select(i, rows);
                        m.window = MenuWindow::None;
                    }
                    Err(e) => error!("menu: could not create {:?}: {e:#}", choice.name),
                }
            }
        }
        MenuWindow::GameType(g) => {
            typed.clear();
            let offered = gd_game_core::session::available_arenas(data.root());
            // The toggles (`GameModeWindow::HandleToggle`): one of the three is down; Custom shows its view (the map
            // list read again, the current custom map selected), the others hide it.
            match hit {
                Some(Hit::ModeCampaign) => (g.crucible, g.open, g.custom) = (false, false, false),
                Some(Hit::ModeCrucible) => (g.crucible, g.custom) = (true, false),
                Some(Hit::ModeCustom) => g.show_custom(data.root(), m.custom.as_ref()),
                Some(Hit::ModItem(i)) if g.custom && i < g.maps.len() => g.pick = i,
                Some(Hit::MapDrop) => g.open = !g.open,
                Some(Hit::MapItem(i)) => (g.arena, g.open) = (i, false),
                _ => {}
            }
            if esc || hit == Some(Hit::Cancel) {
                if g.open {
                    g.open = false;
                } else {
                    m.window = MenuWindow::None;
                }
            } else if enter || hit == Some(Hit::Accept) {
                // `GameModeWindow::AcceptCustom` / `GameModeWindow::Accept`: the mode is set; Custom with an empty
                // list falls back to
                // the campaign; leaving a mode other than Custom re-applies the character (its difficulty
                // clamped), picking Custom does not.
                let was_custom = m.custom.is_some();
                (m.crucible, m.arena) = (g.crucible, g.arena.min(offered.len()));
                m.custom = g.custom.then(|| g.maps.get(g.pick).cloned()).flatten();
                if m.custom.is_none() && !was_custom {
                    let gdc = m.entries.get(m.selected).map(|e| &e.gdc);
                    m.option = m.option.clamp(gdc, m.crucible);
                }
                m.window = MenuWindow::None;
            }
        }
        MenuWindow::Delete(confirm) => {
            for ev in typed.read().filter(|e| e.state.is_pressed()) {
                match &ev.logical_key {
                    Key::Character(c) if confirm.chars().count() < 20 => {
                        confirm.push_str(c.as_str())
                    }
                    Key::Backspace => {
                        confirm.pop();
                    }
                    _ => {}
                }
            }
            let word = fonts.tag("tagMenuDeleteConfirm").to_owned();
            if esc || hit == Some(Hit::Cancel) {
                m.window = MenuWindow::None;
            } else if (enter || hit == Some(Hit::Accept))
                && confirm.trim().eq_ignore_ascii_case(&word)
                && m.selected < m.entries.len()
            {
                let path = m.entries[m.selected].path.clone();
                match delete_character(&save_dir, &path) {
                    Ok(()) => {
                        info!("menu: deleted {}", path.display());
                        m.entries.remove(m.selected);
                    }
                    Err(err) => error!("menu: could not delete {}: {err:#}", path.display()),
                }
                // No characters left: the Create window opens (the setup rule, mid-session).
                m.window = if m.entries.is_empty() {
                    MenuWindow::Create(NewCharacter::default())
                } else {
                    MenuWindow::None
                };
                let i = m.selected.min(m.entries.len().saturating_sub(1));
                m.first = 0;
                m.select(i, rows);
            }
        }
        MenuWindow::Difficulty(pick) => {
            typed.clear();
            let g = m.entries.get(m.selected).map(|e| &e.gdc);
            if let Some(Hit::Pick(o)) = hit
                && o.enabled_in(g, m.crucible)
            {
                *pick = o;
            }
            if esc || hit == Some(Hit::Cancel) {
                m.window = MenuWindow::None;
            } else if enter || hit == Some(Hit::Accept) {
                m.option = *pick;
                m.window = MenuWindow::None;
            }
        }
        MenuWindow::None => {
            typed.clear();
            let n = m.entries.len();
            match hit {
                Some(Hit::Row(i)) if i < n => m.select(i, rows),
                // The arrows step the selection (they're enabled by it,
                // `ui::mainmenu::character_list`); the list follows.
                Some(Hit::ScrollUp) if m.selected > 0 => m.select(m.selected - 1, rows),
                Some(Hit::ScrollDown) if m.selected + 1 < n => m.select(m.selected + 1, rows),
                Some(Hit::Create) => m.window = MenuWindow::Create(NewCharacter::default()),
                Some(Hit::Delete) if n > 0 => m.window = MenuWindow::Delete(String::new()),
                Some(Hit::Difficulty) => m.window = MenuWindow::Difficulty(m.option),
                Some(Hit::GameType) => {
                    m.window = MenuWindow::GameType(GameType::open(m, data.root()))
                }
                Some(Hit::Options) => options.open = true,
                Some(Hit::Credits) => credits.open = true,
                Some(Hit::Exit) => {
                    exit.write(AppExit::Success);
                }
                _ => {}
            }
            if keys.just_pressed(KeyCode::KeyN) {
                m.window = MenuWindow::Create(NewCharacter::default());
            } else if keys.just_pressed(KeyCode::Delete) && n > 0 {
                m.window = MenuWindow::Delete(String::new());
            } else if keys.just_pressed(KeyCode::ArrowDown) && n > 0 {
                m.select((m.selected + 1) % n, rows);
            } else if keys.just_pressed(KeyCode::ArrowUp) && n > 0 {
                m.select((m.selected + n - 1) % n, rows);
            }
            // `GD_MENU_PICK` / `GD_MENU_PICK_AFTER` (hooks.rs): once per process, so an Exit to Main Menu stays in the menu.
            let pick = hooks.ui.menu_pick.filter(|_| {
                !*picked
                    && hooks
                        .ui
                        .menu_pick_after
                        .is_none_or(|s| time.elapsed_secs() >= s)
            });
            *picked |= pick.is_some();

            if let Some((i, d)) = pick
                && i < n
            {
                m.select(i, rows);
                if let Some(d) = d.and_then(|d| DifficultyOption::ALL.get(d)) {
                    m.option = d.clamp(Some(&m.entries[i].gdc), false);
                }
            }
            if (enter || hit == Some(Hit::Start) || pick.is_some())
                && m.can_start()
                && let Some(e) = m.current()
                && (m.crucible || m.custom.is_some() || e.gdc.info.is_in_main_quest != 0)
            {
                let crucible = m.crucible;
                let choice = Choice {
                    save: Some(e.path.clone()),
                    name: e.gdc.header.name.clone(),
                    female: e.gdc.header.sex == 0,
                    hardcore: e.gdc.header.hardcore != 0,
                    survival: crucible.then(|| arena_map(data.root(), m.arena)),
                    custom: m.custom.clone().filter(|_| !crucible),
                };
                let (d, vet) = m.option.clamp(Some(&e.gdc), crucible).mode();
                // The entry stays listed: a mod session that does not start leaves the menu as it was.
                start(&mut commands, choice, Some(e.gdc.clone()), d, vet);
                return;
            }
        }
    }
    let now = (
        m.selected,
        m.first,
        m.option,
        m.window.clone(),
        (m.crucible, m.arena, m.custom.clone()),
    );
    if now != before {
        m.revision += 1;
        // A change of what the menu remembers is written at once (the engine writes on menu state changes).
        let hooked = hooks.ui.menu_crucible.is_some() || hooks.ui.menu_pick.is_some();
        if (now.0, now.2, &now.4) != (before.0, before.2, &before.4) && !hooked {
            write_playmenu(m, data.root(), &save_dir);
        }
    }
}

/// Leaves the menu into a session. A Crucible or custom game opens its mod's map first
/// (`world::open_session_map`); if that fails the menu stays open.
pub(crate) fn start(
    commands: &mut Commands,
    choice: Choice,
    gdc: Option<Gdc>,
    difficulty: usize,
    veteran: bool,
) {
    if choice.game_mode() == 0 {
        commands.remove_resource::<MainMenu>();
        begin(commands, choice, gdc, difficulty, veteran);
        // Cover the world in this frame (`ui::loading::spawn_cover`): the state change comes a frame later.
        gd_ui::loading::spawn_cover(commands);
        return;
    }
    commands.queue(move |world: &mut World| {
        if let Err(e) = crate::world::open_session_map(world, &choice) {
            error!("menu: {} does not start: {e:#}", choice.map_path());
            return;
        }
        world.remove_resource::<MainMenu>();
        let mut commands = world.commands();
        begin(&mut commands, choice, gdc, difficulty, veteran);
        gd_ui::loading::spawn_cover(&mut commands);
        world.flush();
    });
}

/// The arena map the drop-down item `arena` starts (0 = Random, else the offered arenas in drop-down
/// order; `MainMenu::SetMapData` picks `rand() % count` for Random).
pub fn arena_map(root: &std::path::Path, arena: usize) -> String {
    let offered = gd_game_core::session::available_arenas(root);
    let i = match arena {
        0 => offered
            .get(gd_game_core::rng::usize(..offered.len().max(1)))
            .copied(),
        n => offered.get(n - 1).copied(),
    };
    gd_game_core::session::arena_map(gd_game_core::session::ARENAS[i.unwrap_or(0)].1)
}

/// Removes a character folder (`.../{main,user}/_<name>/`), only inside the port's
/// own save folder.
fn delete_character(
    save_dir: &gd_game_core::session::SaveDir,
    gdc: &std::path::Path,
) -> anyhow::Result<()> {
    let dir = gdc.parent().ok_or_else(|| anyhow::anyhow!("no folder"))?;
    anyhow::ensure!(
        CHARACTER_FOLDERS
            .iter()
            .map(|f| save_dir.join(f))
            .any(|root| dir.starts_with(&root) && dir != root),
        "{} is outside {}",
        dir.display(),
        save_dir.0.display()
    );
    anyhow::ensure!(
        !dir.to_string_lossy()
            .contains(gd_game_core::session::LIVE_SAVES),
        "refusing to touch the game's saves"
    );
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

/// Clears this frame's keyboard and mouse input for the game's systems.
fn swallow_input(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
) {
    keys.reset_all();
    buttons.reset_all();
}
