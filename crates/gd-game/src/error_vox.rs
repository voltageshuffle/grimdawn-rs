//! The player's spoken error lines ("I need more energy", "Not yet", "My bags are full"), the one
//! thing the `errorSpeech` option gates.
//!
//! `Player::PlayNotEnoughManaVox`, `PlaySkillCooldownVox`, `PlayItemCooldownVox`,
//! `PlayInventoryFullSound`: each plays its `CharacterSounds` entry with `SoundPak::Play2D`. The
//! first three need the local player and share two timers: nothing plays while
//! `Player.anyVoxTimer` runs, and the same kind with the same id is skipped while
//! `Player.repeatVoxTimer` runs; a played line sets them to 9000 / 15000 ms. The inventory-full
//! line has no timer.
//!
//! Callers ([`ErrorVox`] writers): a requested skill short of energy, a pressed skill cooling
//! down, a potion that cannot be drunk (id 1 health, 2 energy), a given item that does not fit,
//! a merchant buy without room. Picking up a ground item with full bags says nothing.
//! PORT: `Skill::UseDefaultWhenUnavailable` (silent when short of energy) is not checked.
use bevy::prelude::*;
use gd_game_core::data::GameData;
use gd_game_core::options::GameOptions;
use gd_game_core::player::Player;

/// `Player.repeatVoxTimer` set by a played line (ms).
pub const REPEAT_MS: i32 = 15_000;
/// `Player.anyVoxTimer` set by a played line (ms).
pub const ANY_MS: i32 = 9_000;

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

/// A request for the player's error line `kind`; `id` tells repeats apart (the skill record,
/// `"1"` / `"2"` for the health / energy potion).
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

/// The player's two timers and the last id of each throttled kind. A component of the player, so
/// a new session starts clear.
#[derive(Component, Clone, Debug, Default)]
pub struct VoxTimers {
    pub repeat: i32,
    pub any: i32,
    last: [Option<String>; 3],
}

impl VoxTimers {
    /// Counts both timers down by `ms` (only a running timer moves).
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
