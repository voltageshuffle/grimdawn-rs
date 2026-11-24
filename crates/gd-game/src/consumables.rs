//! Using consumables from the bags (right click, `inventory.rs` queues them
//! in `Inventory::used`).
//!
//! Game.dll `…::Use`: `OneShot_Scroll` (elixirs, oils, potions)
//! activates its `skillName` on the user (`Character::ActivateSkill` at its
//! coords, after `SkillManager::AddToScrollSkillList`).
//!
//! `ItemDevotionReset` (Tonic of Clarity):
//! `CreateSecondaryCursorHandler` (requirements met, [`use_gated`]) -> `CursorHandlerDevotionReset`;
//! `AllowUse` = some devotion points spent (`GetDevotionPoints < GetTotalDevotionPoints`).
//! `SecondaryInitialize`: allowed -> the yes / no dialog `tagDevotionResetConfirmation`
//! ([`Inventory::use_ask`] up, answered with [`UseAnswer`]; `Update`: yes ->
//! `UseItem`, no -> cancelled, kept); not allowed -> `UseItem`, refused by `AllowUse`: the "cannot" sound
//! (`GameEngine.genericErrorSound`, `PlayErrorSound`'s), kept, no dialog ([`ask_use`]).
//! `Use` -> `DevotionWindow_ResetAll` (exe): every star removed (powers and bindings
//! cleared), unspent = total (shrines stay); skipped entirely while the devotion pane is in its reclaim mode
//! (`DevotionWindow.reclaimMode`, [`crate::reclamation::SkillReclamation::devotion_mode`]), the tonic still used
//! up; then `useSound`.
//! Game.dll `PlayerInventoryCtrl::UseItem` → `UsePotionOfType`
//! (`OneShot_Scroll` derives from `OneShot_Potion`): a scroll is refused
//! (and kept) while its cooldown runs: the cooldown of its scroll `type`
//! (Elixir / Oil / Tincture, shared by every scroll of that type,
//! `GetCurrentScrollCooldown`) or, untyped, of the item record itself; using
//! one starts it at the item's `useDelayTime`. `AddToScrollSkillList` stops
//! the skill of the previous scroll of the same type
//! (`Skill_BuffSelfToggled::StopSkill`). Every item skill record
//! (`skillName` of all 64 scroll / usable skill items, and their buff
//! records) has single-valued fields, so the cast level is moot (level
//! index 0). `ItemUsableSkill` (binding stones, pet and mini pet summons,
//! `pets.rs` module docs; a class
//! without symbols in Game.dll, loader `ItemUsableSkill_Load`): `Use` activates its `skillName` like a
//! scroll, with no item cooldown (it is not an `OneShot_Potion`) and no
//! cooldown test in the activation (`Skill_SpawnPet::ActivateNow`); its
//! `AllowUse` always allows and `Character::UseInventoryItem`
//! takes one of the stack, so the item is used up unless it is a single
//! `infiniteUse` item. `ItemUsableSkill_Use`: it plays `useSound` at the user's coords, then
//! `Character::ActivateSkill(skill, "End", user, user coords)`: target the user, at its own position (no
//! cursor target, no confirmation). PORT: the game looks the skill up in the user's skill manager
//! (`FindSkillId`, none -> nothing; where the item's skill is added there is not traced); the port casts the
//! record. `OneShot_Sack` is not used from the bags: the game adds
//! the bag when the item enters the inventory (`Inventory::add_to_bags`).
//! `OneShot_SkillUnlock` unlocks potion containers / modifiers
//! (`gd_sim::potions` rules), applied to every character on load.
//! PORT: kept account-wide in `potionunlocks.txt` of the shared folder (`Choice::shared_save_dir`) (one item
//! record per line), the port's own file; a hardcore character's is `potionunlocks_hardcore.txt` (the engine's
//! `potions.gsh`, `gd_formats::save::Shared::Potions`); the game keeps them in the shared
//! save `potions.gst` (`GameEngine::GetSharedSavePath` type 6,
//! `Load/SavePotionUnlocks`, an encrypted `CheckedReader` stream; no real
//! file to test against). Choosing containers / modifiers: the potions
//! window (`cauldron`), hook `GD_POTION_SELECT` (applied
//! on load, `gd_sim::potions::select`).
//! `OneShot_Food` (the three Potions of Constitution): a one-shot, so like a sack it never stays in the
//! bags: `Inventory::add_to_bags` uses it at once (`PlayerInventoryCtrl::OneShotFilter`, notes
//! `shattered_realm2`), picked up within 1 m (`gd_sim::items::loot::auto_pickup_radius`). Its life / mana fields are the
//! `OneShot_Potion` ones (tooltip `OneShot_Potion::GetUIDisplayText`): `Character::TakeBonus` like the charge
//! potion's own fields (`casting::take_bonus`: `bonusLifePercent`, `bonusLifePercentSlow` into the restore pool,
//! `bonusManaPercent`); `bonusConstitutionPercent` / `bonusConstitutionPoints` fill
//! [`crate::constitution::Constitution`] (`CharacterBio::TakeBonus`). The other `OneShot_Potion`
//! bonus fields (experience, gold, attribute / skill points) are 0 in every stock food and not applied.
//! `ItemAttributeReset` (Tonic of Reshaping): `CreateSecondaryCursorHandler` (requirements
//! met) -> `CursorHandlerAttributeReset`, the same shape as the devotion one (`SecondaryInitialize`:
//! allowed -> `tagAttributeResetConfirmation`, dialog 27; `Update`: yes -> `UseItem`); `AllowUse`
//! = `IsExpansion1Loaded` and `HasModifierPointsInUse` (some attribute point spent; the port always
//! has the expansion); `Use`: the refund (`points` module doc), then `useSound`. PORT: `Use` also calls
//! a `GameEngine.attributeResetListener` object's `AttributeResetListener::OnReset` (not traced, taken as a UI
//! refresh; the port's UI reads the state).
//! `ItemFactionBooster` / `ItemFactionWarrant` (writs / bounties): `CreateSecondaryCursorHandler`
//! (requirements met) -> `CursorHandlerFactionBooster` (both classes; the bounty derives from the
//! writ): `SecondaryInitialize` asks `tagFactionWarrantConfirmation` (a bounty allowed while the
//! faction has a positive boost) or `tagFactionMandateConfirmation` (allowed while it has a negative boost), dialog
//! 24, else `UseItem` at once; `AllowUse` (both classes') and `Use`: `factions` module doc.
//! Item classes `ItemUpgrade` / `MasteryReset` / `SkillReset` / `Respec` / `InventorBlueprint` are not in stock
//! data.
//! Hook `GD_USE_ITEM` (hooks.rs).
use crate::constitution::Constitution;
use bevy::prelude::*;
use gd_game_core::casting::{Caster, Casting, Kind, ProcCast};
use gd_game_core::consumables::UseAsk;
use gd_game_core::data::GameData;
use gd_game_core::inventory::Inventory;
use gd_game_core::player::{Loadout, Player, ProfileDirty};

pub struct ConsumablesPlugin;

impl Plugin for ConsumablesPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<DifficultyUnlockUsed>()
            .add_message::<UseAnswer>()
            .add_systems(
                Update,
                (
                    test_use,
                    ask_use,
                    use_items.in_set(ConsumableStage::Use),
                    apply_difficulty_unlocks,
                )
                    .chain()
                    .after(gd_game_core::inventory::InventorySet)
                    .before(gd_game_core::player::PlayerSet)
                    .run_if(resource_exists::<gd_game_core::inventory::Inventory>),
            );
    }
}

/// A campaign difficulty unlock item was used (`ItemDifficultyUnlock::Use`): its difficulty and `statQuests`.
#[derive(Message, Clone, Debug)]
pub struct DifficultyUnlockUsed {
    pub unlocked: usize,
    pub quests: Vec<String>,
}

/// The classes whose secondary cursor handler may ask first: `CursorHandlerDevotionReset`,
/// `CursorHandlerAttributeReset`, `CursorHandlerFactionBooster` (the writ's and the bounty's, module doc).
pub fn asks(class: &str) -> bool {
    matches!(
        class,
        "ItemDevotionReset" | "ItemAttributeReset" | "ItemFactionBooster" | "ItemFactionWarrant"
    )
}

/// The devotion reset question's text tag.
pub const DEVOTION_RESET_TAG: &str = "tagDevotionResetConfirmation";
/// The attribute reset question's text tag (`CursorHandlerAttributeReset::SecondaryInitialize`).
pub const ATTRIBUTE_RESET_TAG: &str = "tagAttributeResetConfirmation";
/// A bounty replacing a writ (`CursorHandlerFactionBooster::SecondaryInitialize`).
pub const FACTION_WARRANT_TAG: &str = "tagFactionWarrantConfirmation";
/// A writ / bounty while the faction has a negative boost.
pub const FACTION_MANDATE_TAG: &str = "tagFactionMandateConfirmation";

/// The use question's tag while it is up.
pub fn use_question(inv: &Inventory) -> Option<&'static str> {
    inv.use_ask.as_ref().and_then(|a| a.question)
}

/// The answer to the use question (yes: use the item).
#[derive(Message, Clone, Copy, Debug)]
pub struct UseAnswer(pub bool);

/// The item's `AllowUse` (module doc).
fn allow_use(
    data: &GameData,
    a: &UseAsk,
    devotion: Option<&crate::devotion::DevotionState>,
    factions: Option<&crate::factions::PlayerFactions>,
    loadout: Option<&Loadout>,
) -> bool {
    match a.class.as_str() {
        "ItemDevotionReset" => devotion.is_some_and(|d| d.points < d.total),
        "ItemAttributeReset" => loadout.is_some_and(crate::points::attributes_spent),
        _ => booster(data, &a.record)
            .zip(factions)
            .is_some_and(|(b, f)| f.booster_allowed(data, &b)),
    }
}

fn booster(data: &GameData, record: &str) -> Option<gd_rules::faction::Booster> {
    data.db
        .get(record)
        .and_then(gd_rules::faction::Booster::from_record)
}

/// The secondary cursor handlers that ask first (module doc): `SecondaryInitialize` decides between the
/// question and `UseItem` at once; a yes `UseItem`s one from its cell. `UseItem` tests `AllowUse`: refused ->
/// the "cannot" sound, kept.
fn ask_use(
    data: Res<GameData>,
    mut inv: ResMut<Inventory>,
    // Option: no devotion state, nothing is spent (refused).
    devotion: Option<Res<crate::devotion::DevotionState>>,
    // Option: no factions yet, a writ / bounty is refused.
    factions: Option<Res<crate::factions::PlayerFactions>>,
    players: Query<&Loadout, With<Player>>,
    mut answers: MessageReader<UseAnswer>,
) {
    let (devotion, factions, loadout) = (
        devotion.as_deref(),
        factions.as_deref(),
        players.single().ok(),
    );
    let allowed = |a: &UseAsk| allow_use(&data, a, devotion, factions, loadout);
    let mut decided = Vec::new();
    let mut at_once = None;
    if let Some(a) = inv.use_ask.as_mut().filter(|a| a.question.is_none()) {
        let ok = allowed(a);
        a.question = match a.class.as_str() {
            "ItemDevotionReset" => ok.then_some(DEVOTION_RESET_TAG),
            "ItemAttributeReset" => ok.then_some(ATTRIBUTE_RESET_TAG),
            // `CursorHandlerFactionBooster::SecondaryInitialize`: a bounty allowed while the
            // faction is positive boosted asks `tagFactionWarrantConfirmation`; else any allowed while it is
            // negative boosted asks `tagFactionMandateConfirmation`; else `UseItem` at once.
            _ => booster(&data, &a.record).and_then(|b| {
                let f = factions?;
                if b.negative && ok && f.boosted(b.faction, false) {
                    Some(FACTION_WARRANT_TAG)
                } else if ok && f.boosted(b.faction, true) {
                    Some(FACTION_MANDATE_TAG)
                } else {
                    None
                }
            }),
        };
        if a.question.is_none() {
            at_once = Some(ok);
        }
    }
    if let Some(ok) = at_once {
        decided.extend(inv.use_ask.take().map(|a| (a, ok)));
    }
    for &UseAnswer(yes) in answers.read() {
        let Some(a) = inv.use_ask.take_if(|a| a.question.is_some()) else {
            continue;
        };
        if yes {
            let ok = allowed(&a);
            decided.push((a, ok));
        }
    }
    for (a, ok) in decided {
        if !ok {
            info!(
                "{} refused (AllowUse)",
                a.record.rsplit('/').next().unwrap_or("")
            );
            inv.sounds.extend(gd_game_core::stash::error_sound(&data));
            continue;
        }
        let Some(i) = inv.bags.get(a.bag).and_then(|b| {
            b.items
                .iter()
                .position(|q| q.x == a.x && q.y == a.y && q.item.base_name == a.record)
        }) else {
            continue;
        };
        let items = &mut inv.bags[a.bag].items;
        let mut one = items[i].item.clone();
        one.stack_count = 1;
        if items[i].item.stack_count > 1 {
            items[i].item.stack_count -= 1;
        } else {
            items.remove(i);
        }
        inv.used.push((one, Some((a.bag, a.x, a.y))));
        inv.changed();
    }
}

/// The inventory bags a difficulty unlock item gives.
const UNLOCK_BAG: &str = "records/items/misc/inventorybag.dbr";

/// The campaign's difficulty unlock item, `ItemDifficultyUnlock::Use`:
/// `GameEngine::CompleteDifficulty` (above the greatest difficulty, 3 reading 0: the cinematic text
/// `tagDifficultyUnlocked` / `tagDifficultyLevelUnlocked` with `tagGameDifficultySetting0<d+1>`, type 7, and the
/// greatest becomes it); the difficulties it skips ([`gd_story::story::Story::skip_difficulties`]) get every riftgate
/// of the world's `Riftgates` instance groups ([`crate::riftgates::RiftgateGroup`], `Player::AddTeleportUID`) and
/// the skill / attribute points of the `statQuests` tasks not done there (events 49 / 50); then five inventory bags
/// (`GiveItemToCharacter`). The quests' own catch-up scaling (`Quest2Task::AdjustRewards`) runs when a task
/// completes, in gd-sim, and leaves skipped difficulties out.
#[allow(clippy::too_many_arguments)]
pub fn apply_difficulty_unlocks(
    mut used: MessageReader<DifficultyUnlockUsed>,
    unlock: Option<ResMut<gd_game_core::session::DifficultyUnlock>>,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    travel: Option<ResMut<crate::riftgates::TravelState>>,
    group: Option<Res<crate::riftgates::RiftgateGroup>>,
    mut effects: MessageWriter<gd_game_core::story::StoryEffect>,
    mut cinematic: MessageWriter<gd_game_core::story::CinematicText>,
    mut gained: MessageWriter<gd_game_core::progress::PointsGained>,
) {
    let (mut unlock, mut travel) = (unlock, travel);
    for u in used.read() {
        if let Some(cur) = unlock.as_mut()
            && u.unlocked > cur.0
        {
            cur.0 = u.unlocked;
            cinematic.write(gd_game_core::story::CinematicText {
                head: "tagDifficultyUnlocked".to_owned(),
                sub: "tagDifficultyLevelUnlocked".to_owned(),
                sub_args: vec![gd_game_core::floating::TextArg::Tag(format!(
                    "tagGameDifficultySetting0{}",
                    u.unlocked + 1
                ))],
                kind: 7,
                front: false,
            });
        }
        let Some(rt) = &rt else { continue };
        let quests: Vec<&str> = u.quests.iter().map(String::as_str).collect();
        let (skipped, skill, attrib) = rt.story.borrow_mut().skip_difficulties(u.unlocked, &quests);
        info!(
            "difficulty unlock: skipped {skipped:?}, {skill} skill and {attrib} attribute points"
        );
        if let (Some(t), Some(g)) = (travel.as_mut(), &group) {
            for &d in &skipped {
                for uid in &g.0 {
                    if d == t.difficulty {
                        if !t.teleports.contains(uid) {
                            t.teleports.push(*uid);
                        }
                    } else if !t.unlocked.contains(&(d, *uid)) {
                        t.unlocked.push((d, *uid));
                    }
                }
            }
        }
        use gd_story::story::Effect;
        if skill > 0 {
            effects.write(gd_game_core::story::StoryEffect(Effect::GiveSkillPoints(
                skill,
            )));
            // Event 49 (the effect itself sends none: `progress` module doc); event 50 comes with the attribute effect.
            gained.write(gd_game_core::progress::PointsGained {
                skill: true,
                ..default()
            });
        }
        if attrib > 0 {
            effects.write(gd_game_core::story::StoryEffect(
                Effect::GiveAttributePoints(attrib),
            ));
        }
        effects.write(gd_game_core::story::StoryEffect(Effect::GiveItem {
            record: UNLOCK_BAG.to_owned(),
            count: 5,
            complete_relic: false,
        }));
    }
}

/// Item classes a right click uses.
pub fn is_usable(class: &str) -> bool {
    matches!(
        class,
        "OneShot_Scroll"
            | "ItemUsableSkill"
            | "ItemDevotionReset"
            | "ItemAttributeReset"
            | "ItemFactionBooster"
            | "ItemFactionWarrant"
            | "OneShot_SkillUnlock"
            | "ItemDifficultyUnlock"
            | "ItemNote"
            | "ItemTransmuterSet"
    ) || is_potion(class)
        || class == "OneShot_Dye"
}

/// `OneShot_Potion` and its subclasses (classInfo parents; `OneShot_Scroll` is one too, with its own `Use`):
/// `PlayerInventoryCtrl::UseItem` sends them through `UsePotionOfType` (the cooldown).
/// `OneShot_Dye` derives from `OneShot`, not from them.
pub fn is_potion(class: &str) -> bool {
    matches!(
        class,
        "OneShot_Potion" | "OneShot_PotionHealth" | "OneShot_PotionMana"
    )
}

/// The blueprint classes: all share `ItemArtifactFormula::Use` (learn into `formulas.gst`) and
/// `AllowUse` (refused when known) in their vtables.
pub fn is_formula(class: &str) -> bool {
    matches!(
        class,
        "ItemArtifactFormula"
            | "ItemSetFormula"
            | "ItemRandomSetFormula"
            | "ItemRerollFormula"
            | "ItemAscensionFormula"
            | "AscendantAltarFormula"
    )
}

/// Whether a right click's `CursorHandlerItemUse` (`PlayerInventoryCtrl::UseItem(id, 1)` at once) is behind
/// the item's requirements. Game.dll `CreateSecondaryCursorHandler` (an `Item` virtual):
/// `OneShot_Potion::…` (also `OneShot_PotionHealth` / `_PotionMana` / `OneShot_Dye`) always
/// makes it; `OneShot_SkillUnlock::…` (folded: `OneShot_Scroll`, the formulas, `ItemNote`,
/// `ItemDifficultyUnlock`, `ItemTransmuterSet`) only when `AreRequirementsMet` passes,
/// else none (the click then goes to `SmartAutoInsert`, which refuses them: nothing happens).
/// `ItemUsableSkill` shares the folded `OneShot_SkillUnlock` handler;
/// `ItemDevotionReset::CreateSecondaryCursorHandler` makes its
/// `CursorHandlerDevotionReset` only when `AreRequirementsMet` passes.
pub fn use_gated(class: &str) -> bool {
    is_formula(class)
        || matches!(
            class,
            "OneShot_Scroll"
                | "ItemUsableSkill"
                | "ItemDevotionReset"
                | "ItemAttributeReset"
                | "ItemFactionBooster"
                | "ItemFactionWarrant"
                | "OneShot_SkillUnlock"
                | "ItemNote"
                | "ItemDifficultyUnlock"
                | "ItemTransmuterSet"
        )
}

/// Whether using it (one of a stack of `stack`) takes it from the bags: all
/// but a single `infiniteUse` item (see the module docs).
pub fn consumed(rec: &gd_formats::arz::Record, stack: u32) -> bool {
    // A note is taken by `lore::read_notes` only when it is new.
    rec.class != "ItemNote"
        && !(rec.class == "ItemUsableSkill" && rec.bool("infiniteUse") && stack <= 1)
}

/// The scroll cooldown key of an `OneShot_Scroll` (`OneShot_Scroll::GetCooldownKey`: the
/// three scroll types share one cooldown each; untyped scrolls have their
/// own, by record).
pub(crate) fn scroll_key(rec: &gd_formats::arz::Record, item: &str) -> String {
    match rec.str("type") {
        Some(t @ ("Elixir" | "Oil" | "Tincture")) => format!("scrolltype:{t}"),
        _ => format!("scroll:{}", item.to_ascii_lowercase()),
    }
}

/// In the session's shared folder (`Choice::shared_save_dir`, as the engine's `potions.gst`); a hardcore
/// character's is `potionunlocks_hardcore.txt` (the engine's `potions.gsh`, `gd_formats::save::Shared::Potions`).
fn unlocks_file(
    choice: &gd_game_core::session::Choice,
    dir: &gd_game_core::session::SaveDir,
) -> std::path::PathBuf {
    choice.shared_save_dir(dir).join(if choice.hardcore {
        "potionunlocks_hardcore.txt"
    } else {
        "potionunlocks.txt"
    })
}

/// The account's potion unlock items.
pub fn account_unlocks(
    choice: &gd_game_core::session::Choice,
    dir: &gd_game_core::session::SaveDir,
) -> Vec<String> {
    std::fs::read_to_string(unlocks_file(choice, dir))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Applies the account's unlocks to a new / loaded character, then the
/// `GD_POTION_SELECT` choices.
pub fn apply_account_unlocks(
    choice: &gd_game_core::session::Choice,
    dir: &gd_game_core::session::SaveDir,
    db: &gd_formats::vfs::Database,
    book: &mut gd_sim::skills::SkillBook,
    hooks: &gd_game_core::hooks::Hooks,
) {
    for u in account_unlocks(choice, dir) {
        if let Some(r) = db.get(&u) {
            gd_sim::potions::apply_unlock(db, book, r);
        }
    }
    for s in &hooks.gameplay.potion_select {
        match gd_sim::potions::select(db, book, s) {
            Ok(()) => info!("potion: chose {}", s.rsplit('/').next().unwrap_or(s)),
            Err(e) => info!("potion: cannot choose {s}: {e}"),
        }
    }
}

fn add_account_unlock(
    choice: &gd_game_core::session::Choice,
    dir: &gd_game_core::session::SaveDir,
    record: &str,
) {
    let mut list = account_unlocks(choice, dir);
    if list.iter().any(|l| l.eq_ignore_ascii_case(record)) {
        return;
    }
    list.push(record.to_owned());
    let path = unlocks_file(choice, dir);
    if path
        .to_string_lossy()
        .contains(gd_game_core::session::LIVE_SAVES)
    {
        return;
    }
    let _created = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
    if let Err(e) = std::fs::write(&path, list.join("\n") + "\n") {
        warn!("potion unlocks: {}: {e}", path.display());
    }
}

fn test_use(
    hooks: Res<gd_game_core::hooks::Hooks>,
    time: Res<Time>,
    mut inv: ResMut<Inventory>,
    players: Query<(), With<Player>>,
    mut done: Local<Option<f32>>,
) {
    if players.is_empty() || done.is_some_and(|t| t < 0.0) {
        return;
    }
    let t = done.get_or_insert(2.0);
    *t -= time.delta_secs();
    if *t > 0.0 {
        return;
    }
    *t = -1.0;
    for r in &hooks.gameplay.use_item {
        inv.used.push((
            gd_formats::save::item::Item {
                base_name: r.to_owned(),
                stack_count: 1,
                ..Default::default()
            },
            None,
        ));
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn use_items(
    mut fresh: gd_game_core::session::NewSession,
    mut commands: Commands,
    data: Res<GameData>,
    mut inv: ResMut<Inventory>,
    // Option: devotion is optional input (its effects are skipped without it).
    devotion: Option<ResMut<crate::devotion::DevotionState>>,
    mut players: Query<
        (
            Entity,
            &Transform,
            &mut Caster,
            &mut Loadout,
            &gd_game_core::inventory::Requirements,
        ),
        With<Player>,
    >,
    mut bodies: Query<
        (
            &mut gd_game_core::combat::Combatant,
            Option<&gd_game_core::casting::PotionOverTime>,
            Option<&mut Constitution>,
        ),
        With<Player>,
    >,
    mut procs: MessageWriter<ProcCast>,
    save_dir: Res<gd_game_core::session::SaveDir>,
    mut active: Local<gd_formats::collections::HashMap<String, String>>,
    (mut notes, mut sets, mut sound, reclaim): (
        MessageWriter<crate::lore::UseNote>,
        MessageWriter<crate::illusion::UseTransmuteSet>,
        MessageWriter<gd_game_core::visuals::PlaySound>,
        // Option: the reclaim mode is only possible with it.
        Option<Res<crate::reclamation::SkillReclamation>>,
    ),
    (choice, rt, unlock, mut unlocks, mut devotion_undo): (
        Res<gd_game_core::session::Choice>,
        Option<NonSend<gd_game_core::story::StoryRuntime>>,
        Option<Res<gd_game_core::session::DifficultyUnlock>>,
        MessageWriter<DifficultyUnlockUsed>,
        // Option: present with the devotion plugin; the tonic's reset drops its pending steps.
        Option<ResMut<crate::devotion::DevotionUndo>>,
    ),
    (mut resets, mut boosts, factions, mut stats): (
        MessageWriter<crate::points::ResetAttributes>,
        MessageWriter<crate::factions::BoosterUsed>,
        // Option: no factions yet, a writ / bounty is refused.
        Option<Res<crate::factions::PlayerFactions>>,
        MessageWriter<crate::play_stats::PlayStat>,
    ),
) {
    if fresh.begun() {
        *active = Default::default();
    }
    if inv.used.is_empty() {
        return;
    }
    // No player yet: the uses stay queued.
    let Ok((e, tf, mut caster, mut loadout, req)) = players.single_mut() else {
        return;
    };
    let used = std::mem::take(&mut inv.used);
    let at = crate::loot::Receiver {
        center: tf.translation,
        req,
    };
    let mut devotion = devotion;
    for (item, from) in used {
        let Some(rec) = data.db.get(&item.base_name) else {
            give_back(&mut commands, &mut inv, &data, at, item, from);
            continue;
        };
        match rec.class.as_str() {
            "OneShot_Scroll" | "ItemUsableSkill" => {
                let Some(skill) = rec
                    .str("skillName")
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                else {
                    give_back(&mut commands, &mut inv, &data, at, item, from);
                    continue;
                };
                let Some(srec) = data.db.get(&skill) else {
                    give_back(&mut commands, &mut inv, &data, at, item, from);
                    continue;
                };
                let scroll = rec.class == "OneShot_Scroll";
                let key = scroll_key(rec, &item.base_name);
                if scroll && caster.cooldowns.contains_key(&key) {
                    // Refused: back to its source cell (`inventory.rs` took one).
                    info!(
                        "{} is cooling down",
                        item.base_name.rsplit('/').next().unwrap_or("")
                    );
                    give_back(&mut commands, &mut inv, &data, at, item, from);
                    continue;
                }
                let kind = gd_game_core::casting::kind_of(srec);
                if kind == Kind::Unsupported {
                    warn!("consumable skill {skill} ({}) not supported", srec.class);
                    give_back(&mut commands, &mut inv, &data, at, item, from);
                    continue;
                }
                // The skill object is cast: a buff wrapper's profile (its
                // `buffSkillName` record) is resolved where it fires
                // (`casting::fire_casts`, `gd_sim::skills::profile_name`), so
                // a radius buff keeps its aura passes.
                let cast = skill;
                if scroll {
                    let cd = rec.f32("useDelayTime").unwrap_or(0.0);
                    if cd > 0.0 {
                        caster.cooldowns.insert(key.clone(), cd);
                    }
                    // The previous scroll of this type stops.
                    if key.starts_with("scrolltype:")
                        && let Some(prev) = active.insert(key, cast.clone())
                        && prev != cast
                    {
                        loadout
                            .buffs
                            .list
                            .iter_mut()
                            .filter(|b| b.0 == prev)
                            .for_each(|b| b.2 = 0.0);
                    }
                }
                info!(
                    "used {} → {}",
                    item.base_name.rsplit('/').next().unwrap_or(""),
                    cast.rsplit('/').next().unwrap_or("")
                );
                // `OneShot_Scroll` / `ItemUsableSkill_Use`: the skill on the user at its own position
                // (`Character::ActivateSkill` at its coords; the port's self casts take the caster as their
                // target: `target` stays None). `ItemUsableSkill` plays its `useSound` at the user.
                if !scroll && let Some(pak) = rec.str("useSound").filter(|p| !p.is_empty()) {
                    sound.write(gd_game_core::visuals::PlaySound {
                        pak: pak.to_owned(),
                        at: Some(tf.translation),
                    });
                }
                let point = tf.translation;
                procs.write(ProcCast {
                    caster: e,
                    cast: Casting {
                        skill: cast,
                        level_index: 0,
                        kind,
                        point,
                        target: None,
                        hostile: false,
                        parent: None,
                    },
                });
            }
            // Read by `lore::read_notes` (it takes the note when new).
            "ItemNote" => {
                notes.write(crate::lore::UseNote(item.base_name.clone()));
            }
            // Unlocked by `illusion::use_sets` (`ItemTransmuterSet::Use`).
            "ItemTransmuterSet" => {
                sets.write(crate::illusion::UseTransmuteSet(item.base_name.clone()));
            }
            "OneShot_SkillUnlock" => {
                if !gd_sim::potions::can_unlock(&data.db, &loadout.skills, rec) {
                    info!(
                        "nothing to unlock with {}",
                        item.base_name.rsplit('/').next().unwrap_or("")
                    );
                    give_back(&mut commands, &mut inv, &data, at, item, from);
                    continue;
                }
                gd_sim::potions::apply_unlock(&data.db, &mut loadout.skills, rec);
                add_account_unlock(&choice, &save_dir, &item.base_name);
                let tag = if rec.i32("skillLevel").unwrap_or(0) <= 1 {
                    "potion modifier unlocked"
                } else {
                    "potion container upgraded"
                };
                info!("{tag}: {}", item.base_name.rsplit('/').next().unwrap_or(""));
            }
            // `ItemDifficultyUnlock::AllowUse` / `Use` (`gameMode` Survival, the
            // Merit items): in the Crucible the Merit items are usable when their difficulty
            // is above the character's greatest survival difficulty; using one stores it
            // (`CompleteSurvivalDifficulty(GameDifficulty)`). The campaign's (`gameMode` Campaign,
            // `difficultyunlock_*`): outside the Crucible (game mode 0 or 2), with Forgotten Gods loaded (the items are
            // its records), when its difficulty is above the greatest campaign difficulty ([`apply_difficulty_unlocks`]).
            // An item refused stays.
            "ItemDifficultyUnlock" => {
                let unlocked = match rec.str("difficultyUnlocked") {
                    Some("Elite") => 1,
                    Some("Ultimate") => 2,
                    _ => 0,
                };
                let done = if rec.str("gameMode") == Some("Survival") {
                    choice.survival.is_some()
                        && rt.as_ref().is_some_and(|rt| {
                            rt.story
                                .borrow_mut()
                                .game
                                .survival
                                .unlock_difficulty(unlocked)
                        })
                } else {
                    choice.survival.is_none()
                        && unlock.as_ref().is_some_and(|u| unlocked as usize > u.0)
                };
                if done {
                    info!("difficulty {unlocked} unlocked by an item");
                    if rec.str("gameMode") != Some("Survival") {
                        unlocks.write(DifficultyUnlockUsed {
                            unlocked: unlocked as usize,
                            quests: rec
                                .strs("statQuests")
                                .into_iter()
                                .map(str::to_owned)
                                .collect(),
                        });
                    }
                } else {
                    give_back(&mut commands, &mut inv, &data, at, item, from);
                    continue;
                }
            }
            // `UseItem` → `UsePotionOfType` (the record's cooldown, refused and kept while it
            // runs; started at `useDelayTime`), then `OneShot_Potion::Use`: `OneShot::Use`
            // (use sound, play stats) and `Character::TakeBonus` of its bonus fields.
            c if is_potion(c) => {
                let key = scroll_key(rec, &item.base_name);
                if caster.cooldowns.contains_key(&key) {
                    info!(
                        "{} is cooling down",
                        item.base_name.rsplit('/').next().unwrap_or("")
                    );
                    give_back(&mut commands, &mut inv, &data, at, item, from);
                    continue;
                }
                let Ok((mut cb, pools, _)) = bodies.single_mut() else {
                    give_back(&mut commands, &mut inv, &data, at, item, from);
                    continue;
                };
                let cd = rec.f32("useDelayTime").unwrap_or(0.0);
                if cd > 0.0 {
                    caster.cooldowns.insert(key, cd);
                }
                let f = |k: &str| rec.f32(k).unwrap_or(0.0);
                crate::casting::take_bonus(
                    &data,
                    &mut commands,
                    e,
                    &mut cb,
                    &mut caster,
                    pools,
                    (
                        f("bonusLifePoints"),
                        f("bonusLifePercent"),
                        f("bonusLifePercentSlow"),
                        f("bonusManaPercent"),
                    ),
                );
                info!("drank {}", item.base_name.rsplit('/').next().unwrap_or(""));
                stats.write(crate::play_stats::PlayStat::PotionUsed(c.to_owned()));
            }
            // `OneShot_Dye::Use` (a `OneShot`, no potion cooldown): `OneShot::Use`, then the
            // player's base texture becomes `maleTexture` / `femaleTexture` (`OneShot_Dye::Load`) by gender.
            // PORT: the texture is not swapped (no stock dye record); the dye is used up.
            "OneShot_Dye" => {
                info!(
                    "dyed with {}",
                    item.base_name.rsplit('/').next().unwrap_or("")
                );
            }
            "OneShot_Food" => {
                let Ok((mut cb, pools, constitution)) = bodies.single_mut() else {
                    give_back(&mut commands, &mut inv, &data, at, item, from);
                    continue;
                };
                let f = |k: &str| rec.f32(k).unwrap_or(0.0);
                crate::casting::take_bonus(
                    &data,
                    &mut commands,
                    e,
                    &mut cb,
                    &mut caster,
                    pools,
                    (
                        f("bonusLifePoints"),
                        f("bonusLifePercent"),
                        f("bonusLifePercentSlow"),
                        f("bonusManaPercent"),
                    ),
                );
                let limit = cb.profile.sheet.constitution;
                let (pct, pts) = (f("bonusConstitutionPercent"), f("bonusConstitutionPoints"));
                match constitution {
                    Some(mut c) => c.take_bonus(limit, pct, pts),
                    None => {
                        let mut c = Constitution::full(limit, cb.life);
                        c.current = 0.0;
                        c.take_bonus(limit, pct, pts);
                        commands.entity(e).try_insert(c);
                    }
                }
                info!("ate {}", item.base_name.rsplit('/').next().unwrap_or(""));
            }
            "ItemDevotionReset" => {
                let Some(d) = devotion.as_mut() else {
                    give_back(&mut commands, &mut inv, &data, at, item, from);
                    continue;
                };
                // `Use` re-tests some-spent (reached here only by the `GD_USE_ITEM` hook, which skips `AllowUse`).
                if d.points >= d.total {
                    info!("no devotion points to reset");
                    give_back(&mut commands, &mut inv, &data, at, item, from);
                    continue;
                }
                // `DevotionWindow_ResetAll` does nothing in the reclaim mode; the tonic is used all the same.
                if reclaim.as_ref().is_some_and(|r| r.devotion_mode()) {
                    info!("devotion reset skipped: the devotion window is in its reclaim mode");
                } else {
                    reset_devotion(d, &mut loadout, devotion_undo.as_deref_mut());
                    commands.entity(e).try_insert(ProfileDirty);
                }
                if let Some(pak) = rec.str("useSound").filter(|p| !p.is_empty()) {
                    sound.write(gd_game_core::visuals::PlaySound {
                        pak: pak.to_owned(),
                        at: Some(tf.translation),
                    });
                }
            }
            // `ItemAttributeReset::Use` (`HasModifierPointsInUse` re-tested; reached refused
            // only by the `GD_USE_ITEM` hook, which skips `AllowUse`): the refund (`points::ResetAttributes`),
            // then `useSound`.
            "ItemAttributeReset" => {
                if !crate::points::attributes_spent(&loadout) {
                    info!("no attribute points to reset");
                    give_back(&mut commands, &mut inv, &data, at, item, from);
                    continue;
                }
                resets.write(crate::points::ResetAttributes);
                if let Some(pak) = rec.str("useSound").filter(|p| !p.is_empty()) {
                    sound.write(gd_game_core::visuals::PlaySound {
                        pak: pak.to_owned(),
                        at: Some(tf.translation),
                    });
                }
            }
            // The boost itself: `factions::use_boosters` (`ItemFactionBooster::Use` / `ItemFactionWarrant::Use`).
            "ItemFactionBooster" | "ItemFactionWarrant" => {
                let ok = gd_rules::faction::Booster::from_record(rec)
                    .zip(factions.as_deref())
                    .is_some_and(|(b, f)| f.booster_allowed(&data, &b));
                if !ok {
                    give_back(&mut commands, &mut inv, &data, at, item, from);
                    continue;
                }
                boosts.write(crate::factions::BoosterUsed(item.base_name.clone()));
            }
            _ => {}
        }
    }
    inv.changed();
}

/// A refused use goes back to its cell ([`Inventory::return_used`]), else is thrown down at the player.
fn give_back(
    commands: &mut Commands,
    inv: &mut Inventory,
    data: &GameData,
    at: crate::loot::Receiver,
    item: gd_formats::save::item::Item,
    from: Option<(usize, u32, u32)>,
) {
    if let Err(item) = inv.return_used(item, from) {
        crate::loot::give_item(commands, inv, data, at, item, false);
    }
}

/// `DevotionWindow_ResetAll` (module doc): every star skill removed, powers and bindings cleared, the spent
/// points back. The window's pending undo steps go too (undoing a step taken before the reset would refund
/// points past the total).
fn reset_devotion(
    d: &mut crate::devotion::DevotionState,
    loadout: &mut Loadout,
    undo: Option<&mut crate::devotion::DevotionUndo>,
) {
    if let Some(u) = undo {
        u.0.clear();
    }
    let stars: Vec<String> = loadout
        .skills
        .skills
        .iter()
        .filter(|(s, _)| d.data.locate(s).is_some())
        .map(|(s, _)| s.clone())
        .collect();
    loadout
        .skills
        .skills
        .retain(|(s, _)| d.data.locate(s).is_none());
    d.points = d.total;
    d.powers.clear();
    d.bindings.clear();
    d.version += 1;
    info!(
        "devotion reset: {} stars removed, {} points",
        stars.len(),
        d.points
    );
}

/// Items used: what reacts to a used item orders after `Use`.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConsumableStage {
    Use,
}
