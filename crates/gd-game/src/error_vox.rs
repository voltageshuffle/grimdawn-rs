//! The player's spoken error lines ("I need more energy", "Not yet", "My bags are full"), the one thing the
//! `errorSpeech` option gates.
//!
//! Game.dll `Player::PlayNotEnoughManaVox`, `PlaySkillCooldownVox`,
//! `PlayItemCooldownVox`, `PlayInventoryFullSound` (the only `errorSpeech` reads
//! in Game.dll and Grim Dawn.exe): each plays its `CharacterSounds` entry (`notEnoughManaSound`,
//! `skillCooldownSound`, `itemCooldownSound`, `inventoryFullSound`: the form's override entry first, else the
//! character's own, [`crate::char_sounds::sound`]) with `SoundPak::Play2D` (unpositioned). The first three also need the
//! controlling (local) player and share two timers: nothing plays while the "any line" timer (`Player.anyVoxTimer`)
//! runs, and the same kind with the same id (skill or potion) is skipped while the "repeat" timer
//! (`Player.repeatVoxTimer`) runs; a played line sets them to 9000 / 15000 ms (`Player::Player`) and remembers its id
//! per kind. `Player::UpdateSelf`
//! counts both down by the frame's ms. The inventory-full line has no timer.
//!
//! Callers ([`ErrorVox`] writers): `ControllerPlayerState::DefaultRequestSkillAction` (a requested skill short of
//! energy, skill status 2: `casting::requests`), `HotSlotOptionSkill::PlayCooldownVox` (a pressed skill cooling down),
//! `HotSlotOptionPotion::Activate` (a potion that cannot be drunk while that bar is not full: id 1
//! health, 2 energy; `casting::potions`), `Player::GiveItemToCharacter` and the other `Give*ToCharacter` (an item
//! that does not fit is dropped by the player: `loot::story_items`) and the merchants' buy (`MarketMiniMart::Purchase`,
//! `MarketFaction::Purchase`; no room: `trade`). Picking up a ground item with full bags says nothing (no other
//! caller).
//! PORT: `Skill::UseDefaultWhenUnavailable` (a skill that falls back to the default attack stays silent when short of
//! energy) is not checked.
use bevy::prelude::*;
use gd_game_core::data::GameData;
use gd_game_core::options::GameOptions;
use gd_game_core::player::Player;

/// "Repeat" timer set by a played line (ms, `Player.repeatVoxTimer`).
pub const REPEAT_MS: i32 = 15_000;
/// "Any line" timer set by a played line (ms, `Player.anyVoxTimer`).
pub const ANY_MS: i32 = 9_000;

/// Which line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoxKind {
    NotEnoughEnergy,
    SkillCooldown,
    ItemCooldown,
    InventoryFull,
}

impl VoxKind {
    /// The `CharacterSounds` entry it plays.
    pub fn slot(self) -> &'static str {
        match self {
            VoxKind::NotEnoughEnergy => "notEnoughManaSound",
            VoxKind::SkillCooldown => "skillCooldownSound",
            VoxKind::ItemCooldown => "itemCooldownSound",
            VoxKind::InventoryFull => "inventoryFullSound",
        }
    }
}

/// A request for the player's error line `kind`; `id` tells repeats apart (the skill record, `"1"` / `"2"` for the
/// health / energy potion; unused for [`VoxKind::InventoryFull`]).
#[derive(Message, Clone, Debug)]
pub struct ErrorVox {
    pub kind: VoxKind,
    pub id: String,
}

impl ErrorVox {
    pub fn new(kind: VoxKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
        }
    }
}

/// The player's two timers and the last id of each throttled kind (`Player.repeatVoxTimer` / `Player.anyVoxTimer`,
/// `Player.lastManaVoxId` / `Player.lastSkillCooldownVoxId` / `Player.lastItemCooldownVoxId`). A component of the
/// player, so a new session starts clear.
#[derive(Component, Clone, Debug, Default)]
pub struct VoxTimers {
    pub repeat: i32,
    pub any: i32,
    last: [Option<String>; 3],
}

impl VoxTimers {
    /// Counts both timers down by `ms` (`Player::UpdateSelf`: only a running timer moves).
    pub fn tick(&mut self, ms: i32) {
        for t in [&mut self.repeat, &mut self.any] {
            if *t != 0 {
                *t -= ms;
            }
        }
    }

    /// Whether `kind` with `id` speaks now (and arms the timers if so).
    pub fn admit(&mut self, kind: VoxKind, id: &str) -> bool {
        let i = match kind {
            VoxKind::NotEnoughEnergy => 0,
            VoxKind::SkillCooldown => 1,
            VoxKind::ItemCooldown => 2,
            VoxKind::InventoryFull => return true,
        };
        if self.any > 0 || (self.repeat > 0 && self.last[i].as_deref() == Some(id)) {
            return false;
        }
        self.repeat = REPEAT_MS;
        self.any = ANY_MS;
        self.last[i] = Some(id.to_owned());
        true
    }
}

pub struct ErrorVoxPlugin;

impl Plugin for ErrorVoxPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ErrorVox>()
            .add_systems(Update, speak.run_if(resource_exists::<GameData>));
    }
}

#[allow(clippy::too_many_arguments)]
fn speak(
    mut commands: Commands,
    mut requests: MessageReader<ErrorVox>,
    data: Res<GameData>,
    options: Res<GameOptions>,
    time: Res<Time>,
    mut players: Query<
        (
            Entity,
            &gd_game_core::character::Character,
            Option<&mut VoxTimers>,
        ),
        With<Player>,
    >,
    mut out: MessageWriter<gd_game_core::visuals::PlaySound>,
    mut carry: Local<f32>,
) {
    let Ok((e, ch, timers)) = players.single_mut() else {
        requests.clear();
        return;
    };
    // A new player gets its timers; its requests wait a frame (messages live two updates).
    let Some(mut timers) = timers else {
        commands.entity(e).try_insert(VoxTimers::default());
        return;
    };
    timers.tick(gd_game_core::session::frame_ms(&mut carry, &time));
    let on = options.flag("errorSpeech");
    for r in requests.read() {
        if !on || !timers.admit(r.kind, &r.id) {
            continue;
        }
        if let Some(pak) = crate::char_sounds::sound(&data, ch, r.kind.slot()) {
            out.write(gd_game_core::visuals::PlaySound { pak, at: None });
        }
    }
}
