//! Pets: summoned by `Skill_TargetedSpawnPet` / `Skill_SpawnPet` casts
//! (`casting::SpawnPet`). The pet record is `spawnObjects[skill level − 1]`
//! (one record per summon level), at the owner's character level
//! (`charLevel` of its bio equations; `Skill_SpawnPet::SpawnPet`
//! copies the caster's character level to the pet); at most `petLimit` per
//! summon skill, the oldest goes first.
//!
//! Mini pets (`Skill_SpawnMiniPet`: a `Skill_SpawnPet` subclass
//! whose vtable differs only in `PostPetSpawned` @0x180509cd0 and
//! `KillExtraPets` @0x180509b00): the 10 stock users are consumables
//! (`ItemUsableSkill`: Will-o-Wisp, Crate, the Hargate / Kasparov quest
//! crystals, the reward minis). The pet is an ordinary `Pet` (its record's
//! controller, follow walk, skills, portrait flag); `PostPetSpawned` passes
//! "mini" (true) to `Player::PostPetSpawn`, which stores it in the `PetPen`
//! entry (+36, `sub_1803C9A70`). `KillExtraPets` (after the new pet joined
//! the pen) first kills the skill's pets beyond `petLimit` (1 for every
//! stock one, so a re-use replaces it), then the owner's mini pets of any
//! skill beyond `GameEngine::GetMiniPetLimit` (`gameengine.dbr`
//! `miniPetLimit` = 3, `LoadFromDatabase`, default 0 = no limit), oldest
//! first ([`Pet::mini`]). `Skill_SpawnMiniPet::KillExtraPets`
//! @0x180509b00: both passes walk the ordinary pen (`GetPetPen`, vfunc
//! +1824), count unreleased entries (`> limit`; the second pass those with
//! the +36 mini byte, any skill) and kill the first matching entries through
//! pen helper `sub_1803C9B30(mode 0)` = `KillMe(overkillOnDismiss)` (the
//! dying skill runs), as the base `KillExtraPets`. The stock mini pet records are `invincible` or
//! `hiddenFromCombat` with `causesAnger` 0 / `angerMultiplier` 0: monsters
//! never get angry at them and no skill reaches them
//! (`combat::HiddenFromCombat`). Their skills set no
//! `spawnObjectsTimeToLive`: they stay until replaced, disbanded or killed.
//!
//! A pet's skills are its record's `skillName{n}` at `skillLevel{n}`
//! (equations of `charLevel`); skills unlocked by the owner's pet modifiers
//! (`SkillSecondary_PetModifier.petSkillName`) take the modifier's level.
//! Its primary attack is the first active attack skill with a level (e.g.
//! the Raven's Storm Orb), else a melee swing; `specialAttack{,2,3}SkillName`
//! fire with `specialAttack*Chance` % (rolled whenever `ChooseBestSkill` runs: on finding a foe, at an attack's end, every 200 ms while pursuing; a special that passes its roll and band but is out of reach is held: the pet pursues for it, `Pet::held`, until the next choice, as monsters do) once `specialAttack*Delay` s have passed since the last use, after
//! their `specialAttack*Timeout` (below), while the foe is inside the
//! `specialAttack*Range` band (`IsSkillInProperRange`, [`PetAttack::band`]).
//! Pet bonuses (`GameEngine::SetPetBonus` list, applied by
//! `ContributePetBonus*Attributes` to the owner's pets like a skill record
//! at its level): the `petBonusName` of the owner's learned
//! skills (devotion stars included) at their current level while installed
//! ([`pet_bonuses`]), of the player's running buffs while they run (a
//! `Skill_BuffSelfDuration` cast, a buff record on the player, a switched-on
//! radius toggle's buff: `InitializeBuff` / `SkillBuff::Install`, removed
//! when the buff ends; every pet of the owner, those summoned during it
//! too) and of the records of
//! every worn item (base, affixes, component, augment) at level 1. Filter
//! (`ContributePetBonus*` → `sub_180592080`): a bonus with
//! `petTypeNames` reaches only pets whose `petTypeNames` share a name
//! (either list empty: it applies). Item bonuses at level 1 = index 0
//! (`ItemEquipment::AttachItem`, `ItemRelic::InstallOnCharacter`
//! and `ItemSet::PetBonusInstall` install their pet bonus with level 1, a
//! learned skill with its own level, `Skill::InstallPetBonus`). Bonuses are
//! live (`GameEngine::SetPetBonus` / `RemovePetBonus` only edit
//! the list; the pet reads it in `Pet::ContributeMisc*Attributes`, which
//! `CharacterBio::UpdateResources` runs every 250 ms): [`refresh_profiles`]
//! rebuilds the pets' profiles when the owner's loadout changes.
//! `isPetBonusScaling` (set on the pets' own skills, 167 records) takes no part
//! in the bonuses or any rule (`SkillProfile` +0x6C1 is read only by
//! the tooltip builders, `DamageAttributeStore::ProcessScale` / `ProcessText`,
//! `RetaliationAttributeStore::ProcessText` (`SetProcess(false)`: the pet
//! skill's damage shown unprocessed, without the player's scaling), `Skill::
//! CreateUIPetText` (the `SkillPetDescriptionHeading` block) and the summary
//! texts; `GetPetBonusScaling` has no caller, Grim Dawn.exe imports none).
//!
//! `PetPlayerScaling` pets (1245 stock records: the player classes' totems,
//! turrets, ...; `Skill::CollectCombatParameters` @0x180499e40): when the
//! attacker's class is `PetPlayerScaling` and its
//! leader (vtable +2256) is a `Player`, the attacker is replaced by that
//! player for the whole collection: its attribute accumulator (offence,
//! damage modifiers), racial bonuses (`ContributeRacialBonusDamage`) and
//! skill manager (its retaliation and conversion attributes). Port:
//! [`Pet::player_scaling`] makes `combat::apply_hits` attack with the owner's
//! profile (attributes, racial bonuses, conversions, retaliation, which the
//! profile holds); what the pet takes back (reflection, retaliation) is
//! defended by its own profile. The skill's modifiers are the skill's own
//! (`a1`, not the swapped manager), so nothing else is swapped.
//!
//! Pet bonus effects ([`PetBonusFx`]): a pet bonus record's
//! `charFxPakPetNames` (per level, `PetBonus` loader sub_1803C8AF0, +1480)
//! is a character effect pak on every pet it reaches. `Skill::
//! InstallPetBonus` @0x18049f2d0 (a learned skill or modifier, a running
//! buff, a `SkillBuff` on the owner) takes the entry at the skill's current
//! level (level − 1, clamped; empty: none) and, for each `Pet` led by the
//! skill's owner whose `petTypeNames` pass the bonus's (as the stats),
//! calls `GameEngine::AddPetBonusFxPak(pet, skill id, [pak])` → the pet's
//! `SkillManager` `AddCharFxPak` (vtable +248, keyed by the skill id, its
//! attach points from the pak); `Skill::RemovePetBonus` @0x18049f860 sends
//! `RemovePetBonusFxPak(pet, skill id)` to every pet of the owner. A pet
//! summoned later gets them from `Player::PostPetSpawn` →
//! `GameEngine::GetPetBonusCharFxPakNames` @0x1802f7650 (the owner's list
//! entries that pass the type filter and have a skill id). Item bonuses
//! (`ItemEquipment::AttachItem`, relics, components, augments, sets) are
//! installed with skill id 0, so they never show effects. Stock data: Call
//! of the Grave and Blasphemy. Port: [`sync_pet_bonus_fx`] derives the list
//! from [`pet_bonuses`]' sources when the owner's loadout changes or a pet
//! is new, and spawns the paks as `BuffFx` under [`pet_bonus_fx_key`].
//! PORT: a dying pet drops them at once (as its other buff effects).
//!
//! Self buffs (`ControllerPet::RegisterStates` is the monster's,
//! so `ControllerMonster::ChooseBestBuffSelfSkill` @0x180137bb0 picks them,
//! called from `Update` (the 4 s `WheneverPossible` check),
//! `DefaultEnemyFoundResponse`, `DefaultAttackedResponse`, the ally
//! responses and the Attack state's "End"; `Player::PostPetSpawn` and
//! `Skill_TargetedSpawnPet::PostPetSpawned` start none): the first of
//! `buffSelfSkillName`, `buffSelf2…`, `buffSelf3…` that the pet has (a level
//! from its record or the owner's pet modifier, e.g. the Hellhound's
//! Hellfire Aura from `summon_hellhound3_petmodifier` at that skill's
//! level) and that is not on yet (`IsTrackable`, +512) is used, per the
//! controller's `BuffSelfBehavior` (the stock pet controllers:
//! WheneverPossible, so ~4 s after the summon or at once on the first foe).
//! A toggle stays on (`Pet::toggled`); a radius aura then buffs the pet and
//! its allies every second (`auras.rs`), other toggles add their records to
//! the pet's profile; a timed self buff is cast on the pet and lasts its
//! `skillActiveDuration` (`Pet::buffs`; a `Skill_BuffRadius` one reaches
//! its allies through the aura passes). Initial skills
//! (`initialSkillName`, `initial2SkillName`) that are toggles are on from
//! the summon (`ControllerMonsterStateStartup::UseInitialSkillIfSet`, the
//! startup state the pet's controller registers). Buff other: as a
//! monster's (`monster_skills` module docs: only `buffOtherSkillName`, by
//! `BuffAllyBehavior` / `BuffAllyTargeting`), the candidates being the live
//! friends of the pet's current side within `ViewDistance` (and
//! `MaxYViewDistance` in height when set) other than the pet itself that
//! have no leader or are quest pets (`ChooseBestAllyToBuff` →
//! `GetCharactersInSphere` @0x18013afe0 with both flags false: the
//! player, an allied monster that follows nobody, a quest pet; the owner's
//! other pets are led by him and left out) that lack the buff; the cast lands through
//! `monster_skills::ally_casts_land` (the Rift Hound guardians' Guardian
//! Roar; the Ognapesh revenant's `Skill_BuffRadius` shield runs on itself).
//! Energy: pets have a mana pool like every `Character` (the bio
//! equations' `characterMana`, e.g. `bio_raven_01.dbr`; the portrait's bar
//! is `CurrentMana / GetAttribute(5)`, sub_140254DB0), full at the
//! summon and regenerating ([`Pet::energy`]); no stock pet skill has a cost.
//! Self / ally buffs and initial skills start their `skillCooldownTime` when
//! used and wait it out (the `Skill` ready check of the choosers, as for
//! monsters, `monster_skills`); initial skills that are not toggles are cast
//! on the pet itself once at the start (`UseInitialSkillIfSet`).
//! PORT: the defend / attacked responses are folded into "found a target";
//! `instantCast` toggles skip the cast animation.
//!
//! AI: pets run the monster AI (`ControllerPet` is a
//! `ControllerMonster` whose `RegisterStates` are the monster's) with the
//! pet record's `controller` and the owner as leader:
//! - targets: the angriest foe (`ControllerMonster::FindEnemy` →
//!   `AngerManager::GetNewTarget`); anger from sight within `ViewDistance`
//!   (`ProcessAngerTarget`, same rates as `combat::sight_anger`), from hits
//!   taken (`UnderAttack`: `AttackedAnger`, at most every 500 ms), decaying
//!   by `ForgiveRate`; a foe counts while within `MaxPursuitDistance` of the
//!   home point, which for a follower is its leader (`InPursuitRange` /
//!   `GetHomePosition`) and within `MaxYViewDistance` in height;
//! - skills fire once the centre distance ≤ both radii + the skill's
//!   `Skill::GetRange` (`distanceProfile` → `gameengine.dbr` ranges) + 0.5
//!   (`CloseEnoughToUseSkill`, `GetSkillUseTolerance`);
//! - the swing timer re-arms from the controller's `min/maxSwingPause` as
//!   an attack starts (`ResetSwingTimer` @0x18013d030,
//!   `combat::monster_ai`), a uniform draw in the range;
//! - without a target (`FollowLeader` / `DefendLeader`): every 300–700 ms
//!   (`DefendLeader::GetNewTestDistanceInterval`) it checks its spot next to
//!   the owner and walks there; the spot is a slot on the owner's defence
//!   ring at both radii + 0.5 (`Character::RequestDefenseSlot`, the owner's
//!   defence `SlotManager`, melee.rs: each follower gets its own spot, more
//!   rings when full); farther than `TeleportToLeaderDistance` (0: never) it
//!   is put on its slot (`ControllerMonster::TeleportToLeader`). Teleports
//!   happen only in those two states, never while fighting (their
//!   `OnUpdate`). A pet goes for its target as any monster does
//!   (`Character::GetMoveToPoint`): a melee skill asks the target's attack
//!   `SlotManager` for a spot, other skills run to the point `Skill::GetRange` +
//!   the target's radius short of it. The fight is the monster's state
//!   machine (melee.rs: Pursue, NavigateObstacle, WaitToAttack, Attack,
//!   RepositionForAttack with `PursuitTime` / `randomRepositionChance` of the
//!   pet's controller record): a melee pet whose target has no free attack
//!   slot waits to attack (roaming near the target) instead of crowding it.
//!   A follower that loses its defence slot (`FollowLeader::LostSlot`) stands
//!   as `DefendLeader` until its next check.
//! - the controller's `Class` decides its states as for monsters
//!   (`combat::ControllerClass`; `Monster::UseController` @0x18037e700 puts
//!   the pet on the stance's controller object, made from its record): a
//!   `ControllerStationaryMonster` / `ControllerTotem` pet (traps, mines,
//!   totems, the Living Plant, the Ice Wall) never moves: no follow walk or
//!   teleport (FollowLeader / DefendLeader are its stub to "Idle"), a move
//!   order only clears its anger (its "Move" is the stub), and it attacks
//!   what is in reach of its skill (its Pursue); a totem picks the nearest
//!   foe on its anger list.
//!
//! Commands ([`PetCommand`]): vanilla has two ways to order pets.
//! The "Pet Attack" skill (`records/skills/default/defaultpetattack.dbr`,
//! class `Skill_PetAttack`, granted to every player by
//! `SkillManager::LoadDefaultPlayerSkills`, put on the hotbar like any skill)
//! commands every pet of the caster (`Skill_PetAttack::GetValidTarget`
//! @0x180503810: each `PetPen` entry that is a `Monster` led by the caster):
//! a target under the cursor → `RequestAllyAttackConfigCmd`, no target → a
//! `RequestAllyMoveConfigCmd` to the ground point (no foe test: a friendly
//! target is refused by the pet's `IsEnemyValid`). `SetAvailability` greys
//! it out (status 6) without such a pet. Selected pets (the selection is UI
//! state, `grimdawn::pets` module docs) get one order each on the next world
//! click (GrimDawn.exe sub_1402056D0): the attack request when a foe is
//! under the cursor, else the move request; `ControllerPlayer::
//! HandlePetAction` @0x18015fd70 does the same for one pet (foe by
//! `FactionManager::IsFoe`; clicking the pet itself does nothing). The pet's
//! state handles them (`Character::RequestAttack` / `RequestMove` → state
//! vtable slots 17 / 18):
//! - attack (`DefaultRequestAttackResponse` @0x18014db90): from the pet's
//!   leader, when `IsEnemyValid` (a live foe within `MaxPursuitDistance` of
//!   the leader) and not already its enemy, the target becomes the most hated
//!   enemy (`SetMostHatedEnemy`, which `AngerManager::GetNewTarget` returns
//!   before any anger) and the pet pursues it. It stays the target until it
//!   is no longer valid; `FollowLeader` / `DefendLeader` `OnBegin` clear it.
//! - move (`DefaultRequestMoveResponse` @0x18014dd20): anger list and most
//!   hated cleared, state "Move": run (`GetRunSpeed`) to the point
//!   (`ControllerMonsterStateMove::OnBegin`; unreachable → "Idle"), "Idle"
//!   at the end of the path or when it fails. While moving the pet does not
//!   fight and ignores attack requests (Move's slot 17 is empty); it still
//!   gains anger, and fights or goes back to its owner from "Idle".
//! - Stunned, knocked down, fleeing, dying and the other passive states
//!   ignore both (empty slots); the Startup state ignores requests from a
//!   player (`ControllerMonsterStateStartup::RequestAttack` /
//!   `RequestMove`: the default response only when the sender is not a
//!   `Player`): [`Pet::startup`]. Startup (`OnBegin` @0x18013ead0,
//!   `HandleEvent` @0x18013edc0; `Skill_SpawnPet::SpawnPet` @0x180509760 enables the
//!   spawn animation on every pet): the pet plays animation type 19 (Spawn)
//!   once and leaves for Idle at its "End" event, at once when its set has
//!   none; `UseInitialSkillIfSet` follows the exit. PORT: "End" is the scripted
//!   animation's state ending (`Character::play_scripted`, as the hidden monster).
//! - Sounds: the attack command plays the pet's `petAttackSound` and Move
//!   its `petAcknowledgeSound` (`Monster::Load`); no stock record sets them.
//!
//! Stances ([`Stance`]): a pet record names three controllers,
//! `controller` (Normal, `Monster::ControllerType` 0), `controllerAggressive`
//! (1) and `controllerDefensive` (2) (`Monster::Load`; the `controllers/pets`
//! `_aggressive` / `_defensive` records differ in view distance, anger rates,
//! pursuit distance, roaming, ...). The portrait's right-click menu
//! (`tagPortraitNormal` / `Aggressive` / `Defensive`, GrimDawn.exe
//! sub_140252600; two portrait buttons cycle Normal → Aggressive → Defensive
//! and back, sub_140254B40) calls `Monster::UseController` @0x18037e700 on
//! that pet (a missing record: no change) and stores the type per summon
//! skill (`Player::SetPetControllerType`, keyed by `PetPen::GetPetOwner`, the
//! summoning skill); `Player::PostPetSpawn` puts each new pet of that skill
//! on the stored controller. The map is session state (not saved: only the
//! player's constructor / destructor and those calls touch it). A pet starts
//! on Normal; the portrait shows Aggressive for a skill with no stored type
//! (`GetPetControllerType` defaults to 1), a vanilla UI quirk. Each
//! controller is its own object with its own anger list, so a switch starts
//! the AI afresh ([`PetStances`] on the owner, [`SetPetStance`]).
//!
//! Portraits ([`portrait_pets`]): `Monster::JoinMe` registers a pet
//! joining the main player in the local pet list (`GameEngine::
//! RegisterLocalPet`, once, appended) only when its record sets
//! `showStatusWidgetWhenPet` (`Monster::Load`, default false: Raven,
//! Hellhound, Briarthorn, Primal Spirit, Blight Fiend and item pets; not
//! skeletons, totems, mines, ...); `Monster::CharacterIsDying` unregisters
//! it. The HUD (GrimDawn.exe sub_140256D20) keeps one portrait per listed
//! pet in list order (new ones appended), so the order is the summon order
//! of the live portrait pets. A portrait's stance icon is
//! [`PetStances::portrait`]. "Disband Pet" ([`DisbandPet`], the portrait
//! menu's `tagPortraitDisband`, sub_140252600 → `ControllerPlayer::ReleasePet`
//! → `ReleasePetConfigCmd::Execute` @0x1800e5080 → the pet's
//! `ControllerCombat::KillMe` @0x18012ce30): the pet dies.
//!
//! Lifetime ([`Pet::time_to_live`]): `Skill_SpawnPet::SpawnPet`
//! (and every other summon: `Skill_TargetedSpawnPet`, the projectile /
//! lightning / chaos spawners, `SkillSecondary_PetSpawn`,
//! `Skill_MonsterGenerator`) asks `Skill::GetSpawnObjectTimeToLive`
//! @0x180493660 at the skill's level ([`spawn_time_to_live`]: the
//! `spawnObjectsTimeToLive` of the skill and its learned and gear
//! modifiers summed, e.g. the b011 medals' +12 s on Thermite Mines; a
//! learned `Skill_SpawnPetTransmuter`'s `modSpawnObjectsTimeToLive` replaces
//! it, the Rotgheist's 12 s) and, when > 0, `Monster::SetLifetime` puts it on
//! the pet's controller (+0x624, ms). No stat or "pet duration" attribute
//! takes part. `ControllerMonster::Update` @0x180134ec0 counts it down by
//! the frame's game time, whatever the state (stunned, fighting, moving), so
//! it runs from the summon; `Monster::UseController` copies it to the new
//! controller, so a stance switch keeps it. At 0: `ControllerCombat::KillMe
//! (overkillOnDismiss)`, an ordinary death (`DieAction`; the flag only picks
//! the overkill death), so the dying skill goes off ([`pet_dying_skills`]).
//! Stock timed player summons: Thermite Mines 18 s, Mortar Trap 24 s, Wind
//! Devil, totems, runes, Reap Spirit's spawns, the devotion summons
//! (Elemental Seeker 3 s, the `tier2_10f` skeletons 6–20 s, ...).
//! The HUD shows no timer (no getter of +0x624 exists; Grim Dawn.exe's
//! portraits read none). The expiry passes the record's `overkillOnDismiss`
//! (`Character::GetDismissBehavior`): set, the
//! pet dies as an overkill ([`crate::combat::Overkill`], gibs with gore on);
//! a Disband is no overkill. Monster summons (`crate::monster_skills::Minion`)
//! pass it too, on expiry and on the pet limit.
//!
//! Release (`ReleasePetConfigCmd::Execute` @0x1800e5080, [`ReleasePet`]
//! / [`DisbandPet`]): `overkill = max(life, 0) >= maxLife x gibThreshold`
//! ([`Pet::gib_threshold`], the record's percent; absent: never) and, when
//! the command's flag (+20) is set, `Character::SuppressDyingSkill`
//! (+0x2834, tested by `StartDyingSkill`; [`SuppressDyingSkill`]); then
//! `KillMe(overkill)`. The portrait's Disband (sub_140252600) passes false:
//! a disbanded pet's dying skill goes off, as an expiring one's.
//!
//! Release on a lost skill bonus (`sub_18034D680`; [`AugmentsRemoved`] from
//! `crate::inventory::sync_equipment`, [`removed_augments`],
//! [`released_by_augments`]): when a skill-bonus container comes off the main
//! player (`ItemEquipment::DetachItem` / `ItemArtifact::DetachItem`: the item's
//! records at level index 0 (count 1); `ItemRelic::RemoveFromCharacter` /
//! `ItemEnchantment::RemoveFromCharacter`: a component / augment, also one
//! replaced on a worn item; `ItemSet::SkillAugmentInstall` /
//! `SkillAugmentRemove`: at every set-count change, the previous count's
//! values), each bonus whose value is not 0 releases pets through the pen
//! (`PetPen` at `Player` +8272; [`ReleasePet`] with the suppress flag; a quest
//! pet's separate pen is untouched):
//! - `augmentSkillName{n}`: the player's skill of that name
//!   (`SkillManager::FindSkillId`, the learned list) gets `Skill::ReleasePets`
//!   @0x1804820d0 (Skill vtable +0x80; only for the main player): its pets
//!   (`sub_1803C9E50`, by skill reference) and those of each of its base skills
//!   (`Skill` +440, filled by `SkillManager::CreateAndLoadSkill` @0x18052f300:
//!   the skill a modifier or secondary hangs on; the port's
//!   `weapon_pool::SkillLinks`);
//! - `augmentMasteryName{n}`: `sub_1803C9F00`, the pets of every skill whose
//!   mastery (`Skill` +316, `AddMastery`) is that one;
//! - `augmentAllLevel`: `sub_1803C9CC0`, the pets of every skill that is not
//!   an item skill (`Skill` +761, `SetAsItemSkill`), i.e. of the learned list.
//!
//! So taking off gear with "+N to Summon Hellhound" kills the hound (as a
//! gib when healthy, no dying skill). A weapon swap is such a take-off
//! (`EquipmentCtrl::SetAlternateEquipment` @0x180289c10 →
//! `WeaponSet__Sift` @0x18028d690 → `ControllerCharacter::
//! SendEquipDetachAction` → `DetachItemConfigCmd` → `Player::DetachItemAction`
//! → `EquipManager::DetachItemAction` → `Weapon::DetachItem` →
//! `ItemEquipment::DetachItem`): leaving a set whose items carry such a bonus
//! releases those pets, e.g. Twilight's Veil (+1 Occultist): Raven and
//! Hellhound die on the swap away from it, not on the swap back (an attach
//! releases nothing). A swap between sets without such bonuses keeps every
//! pet. PORT: several set pieces changing in
//! one equipment change count as one move (the counts in between all
//! release; the engine detaches and attaches one by one, so a piece swapped
//! for another of the same set would release too); a pet's skill counts as
//! "the player's" when the skill book lists it (consumable summons such as
//! the mini pets are not, nor is a skill reached only through item grants).
//!
//! `Skill_SpawnPet::StopSkill` @0x180509a00 ([`StopSummonSkill`]; the owner's death
//! runs it on every skill, `SkillManager::ImDead` @0x1805351b0, and taking off an
//! item on each skill it granted, `RemoveItemSkills` @0x180539360:
//! `crate::player::unequipped_toggles`) and
//! `KillExtraPets` @0x180509890 (the pet limit in `spawn_pets`) kill
//! with `KillMe(overkillOnDismiss)` ([`dismiss`]; no suppress flag, the dying
//! skill goes off): a stop kills all the skill's live pets, `KillExtraPets`
//! the oldest beyond `petLimit` (0 = unlimited). `Skill_SpawnQuestPet`
//! (pen at `Character` +8296, apart from the normal pen: other summons'
//! limits do not count its pets; [`Pet::quest`]) stops with mode 2 while the
//! owner lives (`vfunc +1832`; owner dead: mode 0, a kill): pen helper
//! `sub_1803C9B30` runs `Monster::UnJoinLeader(false)`: the pet
//! stays alive and friendly, loses banner, leader and anger and leaves the
//! pen ([`release_quest_pet`], [`ReleasedQuestPet`]); a new quest pet's
//! `SpawnPet` @0x18050a480 first releases all of the old ones the same way.
//! PORT: no stock skill uses the class (only the base template exists), so
//! the spawn replacement is untested; the +1472 profile vector (unknown) is
//! taken as empty.
//!
//! Special attack timeouts ([`PetAttack::timeout`],
//! `Monster::InitSkillsInController` @0x18037a310, the monster rule in
//! `monster_skills`): each special slot keeps its `specialAttack*Timeout`
//! (s → ms; the time left starts at it) beside its delay (left starts at 0);
//! `ControllerMonster::Update` counts the timeout down only once the
//! controller has found an enemy, and `ChooseBestSkill` takes a special
//! only when both are out and `rand() % 100 < chance`. So a pet's special
//! first comes `Timeout` s into its first fight: the Hellhound's
//! `petskill_hellhound_breathefire` 4 s, the Elemental Seeker's suicide 3 s.
//!
//! Submodules: `types` (commands, stances, `Pet`, its controller), `spawn` (`spawn_pets`, profiles),
//! `commands` (orders, expiry, `impl Pet`), `ai` (`pet_ai`), `bonuses` (pet bonus sources and their FX).
use crate::casting::{Casting, Kind, SpawnPet, kind_of, spawn_fx};
use crate::character::{CharState, Character};
use crate::combat::{CombatRules, Combatant, Dead, MonsterAi, advance_steered, run_speed};
use crate::data::GameData;
use crate::nav::Nav;
use crate::player::{Loadout, Player};
use bevy::prelude::*;
use std::sync::Arc;

pub struct PetsPlugin;

impl Plugin for PetsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PetCommand>()
            .add_message::<SetPetStance>()
            .add_message::<DisbandPet>()
            .add_message::<ReleasePet>()
            .add_message::<AugmentsRemoved>()
            .add_message::<StopSummonSkill>()
            .add_systems(
                Update,
                (
                    crate::melee::sync_slots,
                    spawn_pets.run_if(resource_exists::<crate::rules::CombatRules>),
                    pet_commands,
                    expire_pets,
                    stop_summon_skills,
                    release_augment_pets,
                    disband_pets,
                    pet_dying_skills,
                    pet_ai,
                    crate::auras::pulse_auras.in_set(crate::auras::AurasSet),
                    refresh_profiles.run_if(resource_exists::<crate::rules::CombatRules>),
                    sync_pet_bonus_fx,
                )
                    .chain()
                    .after(crate::casting::CastingSet)
                    .before(crate::combat::CombatSet),
            );
    }
}

mod types;
pub use self::types::*;
pub mod placement;
mod spawn;
use self::spawn::*;
mod commands;
pub use self::commands::*;
mod ai;
use self::ai::*;
mod bonuses;
pub use self::bonuses::*;
