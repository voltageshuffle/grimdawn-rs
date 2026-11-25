//! Main menu state and input: pick a character from the port's save folder
//! (`save_root()/{main,user}/_<name>/player.gdc`; never the game's own saves),
//! create or delete one, choose the difficulty, then start the session
//! (`session::begin`). Drawn by `ui/mainmenu.rs` from [`MainMenu`]; clicks hit
//! the rects it leaves in [`MenuLayout`].
//!
//! Difficulties ([`DifficultyOption`]): Normal, Elite, Ultimate up to the
//! character's `greatest_difficulty` (3 counts as 0, `GameEngine::CompleteDifficulty`).
//! Beating Normal unlocks both Elite and Ultimate (`GameEngine::GameWon`;
//! Lua `Game.UnlockNextDifficulty`); new characters start at 0
//! (`Player::GetGreatestDifficultyCompleted`). Picks and enables
//! (`DifficultyWindow::ApplySelection` / `DifficultyWindow::UpdateEnabled`) are
//! in [`DifficultyOption::enabled`].
//!
//! Character list (`playmenu_characterlist.dbr`, `CharacterList::Update` /
//! `CharacterList::Draw`): at most `maxCharacters` rows from the first visible
//! one; an unset selection becomes the last entry. PORT: sorted by name ignoring
//! case (`CharacterList::Build` is not decompiled) and the most recently written
//! character is selected.
//!
//! Create Character (`CreateCharacterWindow::HandleEvent`): an empty name says
//! `tagMenuError07`, an existing name (ignoring case) `tagMenuError38`; Next
//! (`tagMenuButton12`) opens the difficulty window, whose Accept writes the save
//! (`gd_game::save::create_character`) and returns to the list with the new
//! character selected; a failed write stays in the window. Cancel goes back to
//! the name step. Names are at most `maxPlayerNameLength` (20) characters.
//! PORT: with no characters the Create window opens by itself; Cancel / Esc
//! still close it. Delete Character asks to type `tagMenuDeleteConfirm`
//! ("DELETE"; PORT: ignoring case) and removes the folder, only inside the
//! port's save folder.
//!
//! Keys (PORT: the main menu's key handling is not decompiled): Up/Down choose,
//! Enter play (or accept a window), Esc closes a window, Delete opens the delete
//! window, N the create window. Clicks activate on the left press
//! (`MenuButton::HandleMouseEvent`). While the menu is open no input reaches the
//! game. Multiplayer, Game Guide, Community, DLC are drawn disabled.
//!
//! The Crucible (`GameModeWindow::HandleEvent`, `GameModeWindow::Accept`): the
//! window sets the mode and arena (Random or one offered by the installed
//! packs); the difficulty window then lists Aspirant / Challenger / Gladiator and
//! Start mounts the Crucible data and opens the arena.
//! Custom (mode 2, `GameModeWindow::ShowCustomView` / `GameModeWindow::AcceptCustom`):
//! always offered; shows the user mods' maps ([`custom_maps`], read again each
//! time); Accept with an empty list falls back to the campaign, else mode 2 with
//! the selected mod and map. Start mounts the user mod over the base game
//! (`Engine::InitializeMod`), opens its map and begins the session; a character
//! created there is not in the main quest and is saved under `save/user/`, its
//! shared files under `save/<mod>/`. A mod that does not mount, or a map its
//! archives do not hold (`InitializeMod` mounts only `resources/*.arc`), refuses
//! Start. Start also refuses an existing save whose `map.dat` names a different
//! mod ([`start`]).
//!
//! Characters (`Game::ListCharacters`): `main/` and `user/`. Start in the
//! campaign with a character not in the main quest is refused
//! (`Game::CreatePlayer` / `Game::LoadCharacter`; the Crucible's `modName` is
//! `survivalmode`, `MainMenu::SetMapData`).
//!
//! Remembered menu (`playmenu.cpn`, `MainMenu::ReadPlayMenu` /
//! `MainMenu::WritePlayMenu`): selected character, game mode, difficulty option
//! and Veteran, Crucible arena. Read when the menu opens, written when one
//! changes. PORT: the file is `<save dir>/playmenu.cpn`; other fields of an
//! existing file are kept; Random arena is stored as "none"; mode 2 stores
//! `mod_name` / `level_name2`, and an unlisted custom map opens as the campaign.
//! A hook that sets the menu neither reads nor writes the file.
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
        // Before the game's systems: the menu swallows keyboard and mouse input.
        #[cfg(feature = "dev")]
        let setup = setup.run_if(|h: Res<gd_game_core::hooks::Hooks>| !h.session.skip_menu);
        app.init_resource::<MenuLayout>()
            .add_systems(Startup, setup)
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

#[derive(Clone, Debug, PartialEq)]
pub enum MenuWindow {
    None,
    Difficulty(DifficultyOption),
    Create(NewCharacter),
    /// Create Character's second step: the difficulty window for the validated new character.
    NewDifficulty(NewCharacter, DifficultyOption),
    /// The game-mode window: the pick being edited.
    GameType(GameType),
    Delete(String),
}

/// The game-mode window's edit (`gamemode_mastertable.dbr`, `GameModeWindow::HandleEvent`):
/// `arena` is the drop-down item (0 = Random, then offered [`gd_game_core::session::ARENAS`] order).
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
    /// `GameModeWindow::Open`: mode 2 shows the Custom view with the current mod and map selected.
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

    /// `GameModeWindow::ShowCustomView`: the list is read again, `current` selected.
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
/// `levels/world001.map` when it has no map at all. A map is a loose file or `<archive name>/<entry>`.
/// PORT: folders and maps are sorted by name (the engine uses `FindFirstFile` order).
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

/// The listed maps under `dir`, as resource paths relative to `base`.
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
    pub error: Option<&'static str>,
}

#[derive(Resource)]
pub struct MainMenu {
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub first: usize,
    pub option: DifficultyOption,
    /// The game mode (`GameInfo::SetMode`): arena 0 = Random.
    pub crucible: bool,
    pub arena: usize,
    pub custom: Option<CustomMap>,
    pub window: MenuWindow,
    /// Why the last Start was refused; cleared on any menu change.
    pub error: Option<String>,
    pub revision: u32,
}

impl MainMenu {
    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }

    /// A character is selected and it is not a dead hardcore one; the one rule for Start and Enter.
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

/// `Game::ListCharacters`: `Main/`, then `User/`.
const CHARACTER_FOLDERS: [&str; 2] = ["main", "user"];

/// The characters, by name, and the index of the most recently written one.
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

/// `playmenu.cpn` (the engine's shared save slot 2) of a save folder.
pub fn playmenu_path(save_dir: &gd_game_core::session::SaveDir) -> std::path::PathBuf {
    save_dir.0.join("playmenu.cpn")
}

impl MainMenu {
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

    /// Applies a remembered menu; a clamped option when the character may not pick it.
    pub fn restore(
        &mut self,
        pm: &gd_formats::save::cpn::PlayMenu,
        offered: &[usize],
        maps: &[CustomMap],
        rows: usize,
    ) {
        // No Crucible installed: a mode 1 file opens as the campaign.
        self.crucible = pm.mode == 1 && !offered.is_empty();
        // PORT: a mode 2 file whose map is no longer listed opens as the campaign (the engine keeps mode 2).
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

/// A missing or unreadable file is `None`.
fn read_playmenu(
    save_dir: &gd_game_core::session::SaveDir,
) -> Option<gd_formats::save::cpn::PlayMenu> {
    let b = std::fs::read(playmenu_path(save_dir)).ok()?;
    gd_formats::save::cpn::PlayMenu::parse(&b)
        .map_err(|e| warn!("menu: playmenu.cpn: {e:#}"))
        .ok()
}

fn write_playmenu(m: &MainMenu, root: &std::path::Path, save_dir: &gd_game_core::session::SaveDir) {
    let offered = gd_game_core::session::available_arenas(root);
    let pm = m.to_playmenu(read_playmenu(save_dir).unwrap_or_default(), &offered);
    match pm.encode() {
        Ok(b) => gd_game_core::session::write_user_file("menu", &playmenu_path(save_dir), b),
        Err(e) => error!("menu: playmenu.cpn: {e:#}"),
    }
}

pub(crate) fn setup(
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    save_dir: Res<gd_game_core::session::SaveDir>,
    data: Res<gd_game_core::data::GameData>,
) {
    let (entries, selected) = scan_characters(&save_dir);
    #[cfg(feature = "dev")]
    let (hooked, window, crucible) = (
        hooked(&hooks.ui),
        hook_window(&hooks.ui),
        hooks.ui.menu_crucible,
    );
    #[cfg(not(feature = "dev"))]
    let (hooked, window, crucible) = (false, MenuWindow::None, None);
    let mut m = MainMenu {
        entries,
        selected: 0,
        first: 0,
        option: DifficultyOption::Normal,
        crucible: crucible.is_some(),
        arena: crucible.unwrap_or(0),
        custom: None,
        window,
        error: None,
        revision: 0,
    };
    m.select(selected, 12);
    if !hooked && let Some(pm) = read_playmenu(&save_dir) {
        let offered = gd_game_core::session::available_arenas(data.root());
        m.restore(&pm, &offered, &custom_maps(data.root()), 12);
    }
    #[cfg(feature = "dev")]
    if hooks.ui.menu_custom
        && let MenuWindow::GameType(g) = &mut m.window
    {
        g.show_custom(data.root(), None);
    }
    if let MenuWindow::Difficulty(o) = &mut m.window {
        *o = m.option;
    }
    // PORT: no characters opens the Create window; a hook-set window wins.
    if m.entries.is_empty() && m.window == MenuWindow::None {
        m.window = MenuWindow::Create(NewCharacter::default());
    }
    commands.insert_resource(m);
}

/// Whether a hook sets the menu: the remembered one is then neither read nor written.
#[cfg(feature = "dev")]
fn hooked(ui: &gd_game_core::hooks::UiHooks) -> bool {
    ui.menu_crucible.is_some() || ui.menu_pick.is_some()
}

#[cfg(feature = "dev")]
fn hook_window(ui: &gd_game_core::hooks::UiHooks) -> MenuWindow {
    match ui.menu_open.as_deref() {
        Some("difficulty") => MenuWindow::Difficulty(DifficultyOption::Normal),
        Some("create") => MenuWindow::Create(NewCharacter {
            name: ui.menu_name.clone().unwrap_or_default(),
            ..default()
        }),
        Some("newdifficulty") => MenuWindow::NewDifficulty(
            NewCharacter {
                name: ui.menu_name.clone().unwrap_or_default(),
                ..default()
            },
            DifficultyOption::Normal,
        ),
        Some("delete") => MenuWindow::Delete(String::new()),
        Some("gametype") => MenuWindow::GameType(GameType {
            crucible: ui.menu_crucible.is_some(),
            arena: ui.menu_crucible.unwrap_or(0),
            ..default()
        }),
        _ => MenuWindow::None,
    }
}

#[allow(clippy::too_many_arguments)]
fn run_menu(
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
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
    #[cfg(feature = "dev")] (mut accepted, mut picked, time): (
        Local<bool>,
        Local<bool>,
        Res<Time<Real>>,
    ),
) {
    // Read before `swallow_input` resets this frame's `ButtonInput`.
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
    let cursor = window.single().ok().and_then(gd_ui_kit::cursor_px);
    let hit = cursor.filter(|_| pressed).and_then(|p| layout.at(p));
    // A press on a menu control must not reach the Options window, whose input runs next this frame.
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
    #[cfg(feature = "dev")]
    let hook = {
        let hook = !*accepted && m.window != MenuWindow::None && hooks.ui.menu_accept;
        *accepted |= hook;
        hook
    };
    #[cfg(not(feature = "dev"))]
    let hook = false;
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
            // `GameModeWindow::HandleToggle`: one of the three is down; Custom shows its view.
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
                // `GameModeWindow::AcceptCustom` / `GameModeWindow::Accept`: Custom with an empty list falls back
                // to the campaign; leaving a mode other than Custom re-applies the character, picking Custom does not.
                let was_custom = m.custom.is_some();
                let offered = gd_game_core::session::available_arenas(data.root());
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
            #[cfg(feature = "dev")]
            let pick = {
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
                pick.is_some()
            };
            #[cfg(not(feature = "dev"))]
            let pick = false;
            if (enter || hit == Some(Hit::Start) || pick)
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
        m.error = None;
        m.revision += 1;
        // Written at once (the engine writes on menu state changes).
        #[cfg(feature = "dev")]
        let hooked = hooked(&hooks.ui);
        #[cfg(not(feature = "dev"))]
        let hooked = false;
        if (now.0, now.2, &now.4) != (before.0, before.2, &before.4) && !hooked {
            write_playmenu(m, data.root(), &save_dir);
        }
    }
}

/// Leaves the menu into a session. A Crucible or custom game opens its mod's map first; if that
/// fails the menu stays open. A save whose `map.dat` names a different mod is refused (the Crucible
/// is exempt): the per-map folder is the map path alone.
/// PORT: the engine remounts the save's mod on load (`Game::LoadModMap`).
pub(crate) fn start(
    commands: &mut Commands,
    choice: Choice,
    gdc: Option<Gdc>,
    difficulty: usize,
    veteran: bool,
) {
    if let Some(msg) = choice.mod_mismatch(difficulty) {
        error!("menu: {} does not start: {msg}", choice.name);
        commands.queue(move |world: &mut World| {
            let mut m = world.resource_mut::<MainMenu>();
            m.error = Some(msg);
            m.revision += 1;
        });
        return;
    }
    if choice.game_mode() == 0 {
        commands.remove_resource::<MainMenu>();
        begin(commands, choice, gdc, difficulty, veteran);
        // The state change comes a frame later: cover the world now.
        gd_ui::loading::spawn_cover(commands);
        return;
    }
    commands.queue(move |world: &mut World| {
        if let Err(e) = gd_world::open_session_map(world, &choice) {
            let msg = format!("{e:#}");
            error!("menu: {} does not start: {msg}", choice.map_path());
            let mut m = world.resource_mut::<MainMenu>();
            m.error = Some(msg);
            m.revision += 1;
            return;
        }
        world.remove_resource::<MainMenu>();
        let mut commands = world.commands();
        begin(&mut commands, choice, gdc, difficulty, veteran);
        gd_ui::loading::spawn_cover(&mut commands);
        world.flush();
    });
}

/// The arena map of drop-down item `arena` (0 = Random: `MainMenu::SetMapData` picks `rand() % count`).
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

/// Removes a character folder, only inside the port's own save folder.
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

fn swallow_input(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
) {
    keys.reset_all();
    buttons.reset_all();
}
