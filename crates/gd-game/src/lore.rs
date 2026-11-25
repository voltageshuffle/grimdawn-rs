//! The lore codex: the lore notes (`ItemNote` records) the character has collected.
//!
//! `LoreJournal::RebuildTree`: the list is `Player::GetLoreCodex`, the note records in collection order, saved as
//! `player.gdc` block 12; nothing else about a note (read / unread) is kept. An entry whose record is not an
//! `ItemNote` is dropped at load (`Player::CleanLoreCodex`).
//!
//! Collecting a note:
//! - a note is an ordinary item: picked up into the bags and read by using it (`ItemNote::Use`);
//! - a note already in the codex stays in the bags and only shows its page (event 61, flag 0); else it is taken
//!   (stack of 1), `PlayStats::CollectedLoreNote` +1, appended to the codex, event 61 with flag 1, and a non-zero
//!   experience bonus goes through `Character::ReceiveExperience` with `ShowCinematicText` (`tagLoreBonus`, 5 s);
//! - the bonus (`ItemNote::Load`): `experienceBonus` indexed by the difficulty (the last value past the end), 0
//!   unless positive;
//! - event 61 (`QuestWindow::OnLoreNoteEvent`): flag 1 plays `loreCollectedSound`; then the page is shown and the
//!   tree rebuilt: [`LoreNoteRead`];
//! - monsters' `perPartyMemberDropItemName` (chances `perPartyMemberDropChance`, missing 100, clamped 0..=100) drop
//!   at death each with `rand() % 101 <= chance` ([`monster_note_drops`]);
//! - a chest's `perPartyMemberDropItemName` (`FixedItemController::LoadDropLoot`): one copy (fresh seed) after the
//!   loot table's items, no chance roll.
//!
//! Notes already read: `ItemNote::InitialUpdate` hides a note entity in the codex whenever it enters the world, so
//! it is never taken again (the drop code does not check the codex). The port does not spawn such a note
//! ([`LoreCodex::knows`]); a monster's or chest's note is still rolled and seeded, so the loot stream stays the
//! engine's. The codex exists from the session start ([`init_lore`]) and level objects wait for it. The engine
//! saves no per-level item state, so a note still in the bags unread reappears with its level, a read one never
//! does.
//! PORT: a taken note is also not re-placed while the session runs (`ChestStates::opened`), as a level streaming
//! back in would else duplicate it.

use bevy::prelude::*;

/// The collected lore notes (record paths, collection order).
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct LoreCodex {
    pub notes: Vec<String>,
}

impl LoreCodex {
    /// `Player::GetItemInLoreCodex`, by record name.
    pub fn knows(&self, record: &str) -> bool {
        self.notes.iter().any(|n| n.eq_ignore_ascii_case(record))
    }
}

/// `Class` `ItemNote`.
pub fn is_note(db: &gd_formats::vfs::Database, record: &str) -> bool {
    db.get(record)
        .is_some_and(|r| r.str("Class") == Some("ItemNote"))
}

/// At the session start; no save: empty.
pub fn init_lore(
    mut commands: Commands,
    // Option: `loaded` is absent for a new character.
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    data: Res<gd_game_core::data::GameData>,
) {
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

/// Event 61: `new` when the note entered the codex now (with `xp` experience), else it was already known.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct LoreNoteRead {
    pub record: String,
    pub new: bool,
    pub xp: u32,
}

#[derive(Message, Clone, Debug)]
pub struct UseNote(pub String);

/// A note placed in a level; taken ones are remembered in `ChestStates::opened` under its key.
#[derive(Component, Clone, Copy, Debug)]
pub struct PlacedNote(pub gd_game_core::world::ChestKey);

pub fn experience_bonus(rec: &gd_formats::arz::Record, difficulty: usize) -> u32 {
    let v = rec.f32s("experienceBonus");
    v.get(difficulty.min(v.len().saturating_sub(1)))
        .map_or(0, |&x| if x > 0.0 { x as u32 } else { 0 })
}

/// `roll` is `rand() % 101`.
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

#[allow(clippy::too_many_arguments)]
pub fn read_notes(
    mut used: MessageReader<UseNote>,
    mut codex: ResMut<LoreCodex>,
    mut inv: ResMut<gd_game_core::inventory::Inventory>,
    data: Res<gd_game_core::data::GameData>,
    mode: Res<gd_game_core::session::GameMode>,
    mut stats: ResMut<crate::play_stats::PlayCounts>,
    mut rewards: MessageWriter<gd_game_core::story::StoryEffect>,
    mut out: MessageWriter<LoreNoteRead>,
) {
    for UseNote(record) in used.read() {
        let Some(rec) = data.db.get(record).filter(|r| r.class == "ItemNote") else {
            continue;
        };
        if codex.knows(record) {
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
            rewards.write(gd_game_core::story::StoryEffect(
                gd_story::story::Effect::GiveExperience(xp),
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

pub fn spawn_placed_note(
    commands: &mut Commands,
    record: &str,
    at: Vec3,
    key: gd_game_core::world::ChestKey,
    taken: bool,
) -> Option<Entity> {
    if taken {
        return None;
    }
    let tf = Transform::from_translation(at);
    let item = gd_formats::save::item::Item {
        base_name: record.to_owned(),
        stack_count: 1,
        seed: gd_game_core::rng::u32(1..0x7FFF_FFFF),
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
    inv: Option<Res<gd_game_core::inventory::Inventory>>,
) -> bool {
    codex.is_some() && inv.is_some()
}

pub struct LorePlugin;

impl Plugin for LorePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<LoreNoteRead>()
            .add_message::<UseNote>()
            .add_systems(
                Update,
                (
                    init_lore
                        .run_if(resource_exists::<gd_game_core::session::SessionReady>)
                        .run_if(not(resource_exists::<LoreCodex>)),
                    read_notes
                        .after(crate::consumables::ConsumableStage::Use)
                        .run_if(has_lore_inputs),
                ),
            );
    }
}
