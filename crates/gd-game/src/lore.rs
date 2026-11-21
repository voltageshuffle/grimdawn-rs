//! The lore codex: the lore notes (`ItemNote` records) the character has
//! collected, shown on the Codex window's lore page (`grimdawn` `ui/quests.rs`).
//!
//! `Grim Dawn.exe` codex log rebuild sub_14016A180:
//! - the list is `Player::GetLoreCodex`, the note records' object ids in
//!   collection order, saved as `player.gdc` block 12 (v1, a string list of the
//!   record paths, `gd_formats::save::gdc::Notes`); nothing else about a note
//!   (read / unread) is kept;
//! - an entry whose record is not an `ItemNote` is dropped from the player's
//!   list (`Player::CleanLoreCodex` with the kept ones), done here at load.
//!
//! Collecting a note, Game.dll:
//! - a note is an ordinary item: picked up into the bags (it is no
//!   `OneShot_*`, so `PlayerInventoryCtrl::AddItem` does not use it) and read
//!   by using it (right click: `PlayerInventoryCtrl::UseItem` @0x1803f42c0 →
//!   `Character::UseInventoryItem` → `ItemNote::Use`);
//! - `UseItem`: a note already in the codex (`Player::GetItemInLoreCodex` by
//!   record) stays in the bags and only shows its codex page (event 61 with
//!   flag 0, then the journal opens); else `ItemNote::AllowUse` @0x180346d00
//!   (allowed when not in the codex; takes the item when the stack is 1) and
//!   `ItemNote::Use` @0x180346a70: `PlayStats::CollectedLoreNote` (+1,
//!   inlined), `Player::AddItemToLoreCodex` (appended), event 61 with flag 1,
//!   then when its experience bonus is not 0 `Character::ReceiveExperience`
//!   (with the character's experience bonus %) and `ShowCinematicText`
//!   (`tagLoreBonus` over `ExperienceFormat` with the amount, 5 s), then the
//!   journal opens;
//! - the bonus (`ItemNote::Load` @0x1803462d0): `experienceBonus` indexed by
//!   the difficulty (the last value past the end), 0 unless positive;
//! - event 61 (GrimDawn.exe sub_14022FB30): flag 1 plays the quest window's
//!   `loreCollectedSound`; then the note's page is shown (sub_140168860) and
//!   the tree rebuilt: [`LoreNoteRead`], handled by the game (`ui/quests.rs`);
//! - monsters' `perPartyMemberDropItemName` (`Monster::Load`, chances
//!   `perPartyMemberDropChance`, a missing one 100, clamped 0..=100) drop at
//!   death each with `rand() % 101 <= chance` (`ControllerMonster::DropLoot`
//!   @0x18013a3d0): [`monster_note_drops`].
//!
//! A chest's `perPartyMemberDropItemName` (`FixedItem::Load` into +3336, kept
//! as the `ItemReplicaInfo` at +3328):
//! `FixedItemController::LoadDropLoot` @0x1802adb90: after the chest's loot
//! table is generated, when `GameEngine::IsServerOrSingle` one copy of that
//! note (fresh seed) is appended to the loot per player of the party (the
//! single player: one note, after the table's items, no chance roll); the
//! port drops it with the loot when the chest opens (`chests.rs`).
//!
//! Level-placed notes: `ItemNote::InitialUpdate` @0x1803464c0: when the
//! note entity appears, `Actor::SetVisibility(!Player::GetItemInLoreCodex)`
//! for the main player. The engine saves no per-level item state (the save's
//! `map.dat` holds only the map path and spawn coordinates), so the only
//! memory of a taken placed note is the codex (block 12): a note still in the
//! bags unread reappears with its level, a read one never does
//! ([`LoreCodex::knows`], `monsters::spawn_level_objects`).
//! PORT: a taken note is also not re-placed while the session runs
//! (`ChestStates::opened`), as a level streaming back in would else duplicate
//! it; stack counts of notes are 1 in the stock data, so a used note is always
//! taken.

use crate::session::MessageDrain;
use bevy::prelude::*;

/// The collected lore notes (record paths, collection order).
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct LoreCodex {
    pub notes: Vec<String>,
}

impl LoreCodex {
    /// Whether `record` is in the codex (`Player::GetItemInLoreCodex`, by record name).
    pub fn knows(&self, record: &str) -> bool {
        self.notes.iter().any(|n| n.eq_ignore_ascii_case(record))
    }
}

/// Whether `record` is a lore note (`Class` `ItemNote`).
pub fn is_note(db: &gd_formats::vfs::Database, record: &str) -> bool {
    db.get(record)
        .is_some_and(|r| r.str("Class") == Some("ItemNote"))
}

/// The codex from the loaded character, once the player exists (no save: empty).
pub fn init_lore(
    mut commands: Commands,
    // Option: `codex` is the existence check (this system creates it); `loaded` is absent for a new character.
    codex: Option<Res<LoreCodex>>,
    loaded: Option<Res<crate::session::LoadedCharacter>>,
    data: Res<crate::data::GameData>,
    players: Query<(), With<crate::player::Player>>,
) {
    if codex.is_some() || players.is_empty() {
        return;
    }
    let notes = loaded
        .map(|c| {
            c.0.notes
                .notes
                .iter()
                .filter(|n| is_note(&data.db, n))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    commands.insert_resource(LoreCodex { notes });
}

/// A note was read (the engine's event 61): `new` when it entered the codex
/// now (with `xp` experience), else it was already known. The game plays the
/// sound, shows the notice and opens the journal at it.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct LoreNoteRead {
    pub record: String,
    pub new: bool,
    pub xp: u32,
}

/// A note used from the bags (queued by `consumables`).
#[derive(Message, Clone, Debug)]
pub struct UseNote(pub String);

/// A note placed in a level (lying as a ground item); taken ones are
/// remembered in `ChestStates::opened` under its key.
#[derive(Component, Clone, Copy, Debug)]
pub struct PlacedNote(pub crate::world::ChestKey);

/// The note's experience bonus at `difficulty` (module doc).
pub fn experience_bonus(rec: &gd_formats::arz::Record, difficulty: usize) -> u32 {
    let v = rec.f32s("experienceBonus");
    v.get(difficulty.min(v.len().saturating_sub(1)))
        .map_or(0, |&x| if x > 0.0 { x as u32 } else { 0 })
}

/// The notes a dying monster drops (module doc); `roll` is `rand() % 101`.
pub fn monster_note_drops(
    rec: &gd_formats::arz::Record,
    mut roll: impl FnMut() -> u32,
) -> Vec<String> {
    let chances = rec.f32s("perPartyMemberDropChance");
    rec.strs("perPartyMemberDropItemName")
        .into_iter()
        .enumerate()
        .filter(|(_, n)| !n.is_empty())
        .filter_map(|(i, n)| {
            let chance = chances.get(i).map_or(100, |&c| (c as i32).clamp(0, 100)) as u32;
            (roll() <= chance).then(|| n.to_owned())
        })
        .collect()
}

/// Reads used notes (module doc): a new one is taken from the bags, added
/// to the codex and the play stats, and gives its experience bonus.
#[allow(clippy::too_many_arguments)]
pub fn read_notes(
    mut used: MessageReader<UseNote>,
    mut codex: ResMut<LoreCodex>,
    mut inv: ResMut<crate::inventory::Inventory>,
    data: Res<crate::data::GameData>,
    mode: Res<crate::session::GameMode>,
    mut stats: ResMut<crate::play_stats::PlayCounts>,
    mut rewards: MessageWriter<crate::story::StoryEffect>,
    mut out: MessageWriter<LoreNoteRead>,
) {
    for UseNote(record) in used.read() {
        let Some(rec) = data.db.get(record).filter(|r| r.class == "ItemNote") else {
            continue;
        };
        if codex.notes.iter().any(|n| n.eq_ignore_ascii_case(record)) {
            out.write(LoreNoteRead {
                record: record.clone(),
                new: false,
                xp: 0,
            });
            continue;
        }
        inv.remove_by_record(record, 1);
        codex.notes.push(record.clone());
        stats.lore_notes += 1;
        let difficulty = mode.0.difficulty;
        let xp = experience_bonus(rec, difficulty);
        if xp > 0 {
            rewards.write(crate::story::StoryEffect(
                gd_sim::story::Effect::GiveExperience(xp),
            ));
        }
        info!(
            "lore note collected: {} (+{xp} xp)",
            record.rsplit('/').next().unwrap_or("")
        );
        out.write(LoreNoteRead {
            record: record.clone(),
            new: true,
            xp,
        });
    }
}

/// Places a level's note as a ground item (module doc), unless taken.
pub fn spawn_placed_note(
    commands: &mut Commands,
    record: &str,
    at: Vec3,
    key: crate::world::ChestKey,
    taken: bool,
) -> Option<Entity> {
    if taken {
        return None;
    }
    let tf = Transform::from_translation(at);
    let item = gd_formats::save::item::Item {
        base_name: record.to_owned(),
        stack_count: 1,
        seed: crate::rng::u32(1..0x7FFF_FFFF),
        ..Default::default()
    };
    Some(
        commands
            .spawn((
                tf,
                GlobalTransform::from(tf),
                crate::loot::GroundItem {
                    item: Some(item),
                    gold: 0,
                    yaw: 0.0,
                },
                PlacedNote(key),
                Name::new("lore note"),
            ))
            .id(),
    )
}

/// The codex and the bags exist: a used note can be read.
fn has_lore_inputs(
    codex: Option<Res<LoreCodex>>,
    inv: Option<Res<crate::inventory::Inventory>>,
) -> bool {
    codex.is_some() && inv.is_some()
}

pub struct LorePlugin;

impl Plugin for LorePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<LoreNoteRead>()
            .add_message::<UseNote>()
            .drain_messages_unless::<UseNote, _>(has_lore_inputs)
            .add_systems(
                Update,
                (
                    init_lore
                        .after(crate::player::PlayerSet)
                        .run_if(not(resource_exists::<LoreCodex>)),
                    read_notes
                        .after(crate::consumables::ConsumableStage::Use)
                        .run_if(has_lore_inputs),
                ),
            );
    }
}
