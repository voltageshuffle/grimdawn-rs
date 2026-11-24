//! The casting data: attach-point geometry (the cast systems are gd-game's
//! `casting`).
use bevy::prelude::*;

/// Bind-pose position of a named mesh attach point in the character's space
/// (the parent bone's bind transform × the point's frame).
pub fn attach_point_bind(body: &gd_formats::msh::Mesh, name: &str) -> Option<bevy::math::Vec3> {
    let p = body.attach_point(name)?;
    let parent = p
        .parent_bone
        .map_or(gd_formats::msh::Coords::IDENTITY, |i| body.bones[i].abs);
    Some(bevy::math::Vec3::from(parent.mul(&p.coords).origin))
}

/// A character's damage-absorbing combat filters, in the order they were added: `CombatManager::AddCombatFilter`
/// appends a filter to the manager's list (`CombatManager.filters`) unless it is in it already, and
/// `CombatManager::ApplyDamage` runs every filter of the list in turn on each damage application (each type of a
/// hit and each DoT tick, `gd_sim::combat::Absorb`). Each skill is its own filter, so a recast keeps its place.
/// Every `gd_game::combat::Combatant` has one (empty: no absorb).
#[derive(Component, Default, Debug, Clone)]
pub struct Shields(pub Vec<Shield>);

/// One filter of [`Shields`]: its skill (or buff) record, absorb and seconds left.
///
/// - [`ShieldKind::Buff`]: a self buff's damage absorption (`damageAbsorptionPercent` %, then `damageAbsorption`
///   flat, a pool for `Skill_BuffSelfShield` that ends the shield when spent) for the types the skill's
///   `…DamageQualifier` flags list (none = all). `Skill_BuffSelfToggled::ModifyDamage`,
///   `Skill_BuffSelfShield::ModifyDamage`, `SkillProfile::LoadProfile`. Ends with `skillActiveDuration` when the
///   buff has one, or when the skill stops (`skill_buffs::stop_shields`).
/// - [`ShieldKind::Potion`]: a charge potion's absorb (`absorbShieldPercentHealth`, see `gd_game::casting::HEALTH_POTION`), the
///   potion skill's own filter, with the pool it started with. That makes the potion trackable while the pool
///   lasts (`Skill_ChargePotion::IsTrackable`: absorb left (`Skill_ChargePotion.absorbLeft`) > 0;
///   `TrackableRemainingTime` / `TrackableTotalTime` = absorb left / the amount of the last potion
///   (`Skill_ChargePotion.lastAmount`), both reset by each potion in
///   `TargetResult`).
/// - [`ShieldKind::Shrine`]: a Shattered Realm shrine buff's (`gd_game::endless::shrine_shield`).
#[derive(Debug, Clone)]
pub struct Shield {
    pub skill: String,
    pub absorb: gd_sim::combat::Absorb,
    pub left: Option<f32>,
    pub kind: ShieldKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShieldKind {
    Buff,
    /// The pool the last potion started with.
    Potion(f32),
    Shrine,
}

impl Shields {
    /// `AddCombatFilter`: the skill's filter takes the new values in its place, or joins at the end.
    pub fn add(&mut self, s: Shield) {
        match self
            .0
            .iter_mut()
            .find(|x| x.kind_matches(&s) && x.skill.eq_ignore_ascii_case(&s.skill))
        {
            Some(x) => *x = s,
            None => self.0.push(s),
        }
    }

    /// The potion's filter, if one is on.
    pub fn potion(&self) -> Option<&Shield> {
        self.0
            .iter()
            .find(|s| matches!(s.kind, ShieldKind::Potion(_)))
    }

    /// Each filter in turn on one damage application (`ApplyDamage`).
    pub fn filter(&mut self, ty: gd_sim::attributes::CombatType, v: f32) -> f32 {
        self.0.iter_mut().fold(v, |v, s| s.absorb.filter(ty, v))
    }

    /// Drops the spent pools.
    pub fn drop_spent(&mut self) {
        if self.0.iter().any(|s| s.absorb.spent()) {
            self.0.retain(|s| !s.absorb.spent());
        }
    }
}

impl Shield {
    fn kind_matches(&self, o: &Shield) -> bool {
        std::mem::discriminant(&self.kind) == std::mem::discriminant(&o.kind)
    }
}

use crate::data::GameData;
use crate::visuals::{BuffFx, EffectSpec};
use gd_formats::arz::Record;
use gd_formats::collections::HashMap;
use gd_sim::beam::SpellBeam;

/// Hotbar slots of a save bar (`slots_per_bar` 47): 0–9 keys 1–0, 10 left
/// mouse, 12 right mouse, 11 / 13 the left / right mouse of the alternate
/// weapon set, 24 / 25 the health / energy potions, 46 evade
/// (`PlayerHotSlotCtrl::GetPrimarySlot` / `GetSecondarySlot` /
/// `GetHealthPotionSlot` / `GetManaPotionSlot` / `GetEvadeSlot`);
/// 14–23 the second row's keys. Keys 1–0 (keybinding actions 9–18) fire
/// whichever row the HUD shows, flipped by ToggleHotSlotBar (action 43)
/// (`UIPlayerHud::HandleKey` / `UIPlayerHud::PressHotSlotButton`, names mapped by
/// `KeyBindings::GetActionByName`; grimdawn
/// `ui::hud::HotbarRow`).
pub const SLOT_LEFT: usize = 10;
pub const SLOT_RIGHT: usize = 12;
pub const SLOT_ROW2: usize = 14;
pub const SLOTS: usize = 24;
pub const DEFAULT_ATTACK: &str = "records/skills/default/defaultweaponattack.dbr";

/// The player's main hand holds a ranged weapon: weapon attacks fire its
/// `basicProjectileName` (a projectile record: `projectileVelocity`,
/// `projectileDistance`) at the target instead of hitting in melee reach.
#[derive(Component, Clone, Debug)]
pub struct RangedWeapon {
    pub projectile: String,
    /// Attack range: ranged weapons use the Long profile
    /// (`SkillManager::Update` via `GetWeaponRangeProfile`)
    /// — `longRange` + the 0.5 tolerance; the game adds both
    /// radii on top (`Character::GetTargetDistance`).
    pub reach: f32,
}

/// A skill hit landing on `target` (applied by `combat::apply_hits`).
#[derive(Message, Clone, Debug)]
pub struct SkillHit {
    pub attacker: Entity,
    pub target: Entity,
    pub skill: String,
    pub level_index: usize,
    /// The weapon-pool skill a pool attack runs under (`weapon_pool.rs`).
    pub parent: Option<(String, usize)>,
    /// Scale of the hit's damage (projectile range falloff,
    /// `gd_sim::actor::range_scale`); 1 otherwise.
    pub scale: f32,
    /// A projectile's hit (`TakeAttack` style 2: projectile block, deflect).
    pub projectile: bool,
}

/// A summon cast fired (spawned by `pets.rs`).
#[derive(Message, Clone, Debug)]
pub struct SpawnPet {
    pub owner: Entity,
    pub skill: String,
    pub level_index: usize,
    pub point: Vec3,
    /// The cast's target (`ActivateNow`'s target id): a `Skill_TargetedSpawnPet`
    /// packs its summons around it (`pets::placement`).
    pub target: Option<Entity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Weapon,
    SelfRadius,
    TargetRadius,
    Projectile,
    Drop,
    Buff,
    /// Summons (`Skill_TargetedSpawnPet`, `Skill_SpawnPet`): `pets.rs`.
    Pet,
    /// `Skill_AttackWave` / `Skill_WPAttack_AttackWave`: a travelling,
    /// widening band (`gd_game::waves`).
    Wave,
    /// `Skill_AttackLineFan`: a fan of travelling lines that burst at the
    /// end of their life (`gd_game::line_fan`).
    LineFan,
    /// `Skill_AttackLine`: a point travelling at the target that hits around it every period
    /// (`gd_game::casting::AttackLine`).
    Line,
    /// `Skill_BuffAttackRadiusDrop` (Devastation): `projectileLaunchNumber`
    /// drops every `skillTargetInterval` for `skillActiveDuration` within
    /// `skillTargetRadius` of the point.
    Storm,
    /// `Skill_AttackProjectileAreaEffect` (Sigil of Destruction): an area of
    /// `projectileExplosionRadius` at the point hitting every
    /// `targetInterval` ms of its projectile
    /// (`ProjectileAreaEffect::UpdateSelf`) for `skillActiveDuration`.
    Area,
    /// `Skill_CharonGeysers`: areas at the level's geyser markers around the
    /// caster (`gd_game::geysers`).
    Geysers,
    /// `Skill_AttackPattern`: delayed spots around a centre that run the
    /// skill's secondaries (`gd_game::patterns`).
    Pattern,
    /// `Skill_TurretFireControl`: the caster's turrets around it use a
    /// special (`gd_game::turrets`).
    TurretFire,
    /// `Skill_AttackSpellBeam` (Aether Ray): channelled while held ([`Channel`]).
    Beam,
    /// Toggled skills (`Skill_Buff{Self,Radius}Toggled`, …): a use flips
    /// them (`Loadout.skills.inactive`); the player's are auto toggles, on
    /// by themselves (`crate::skill_buffs` docs, auto toggles).
    Toggle,
    /// `Skill_AttackChain`: the target, then jumps (`sparkMaxNumber`,
    /// `sparkGap`, `sparkChance`) to the nearest unhit foes, with a
    /// `lightningName` bolt between each: each of the `sparkMaxNumber` jumps
    /// rolls `sparkChance` (a miss spends the jump) and picks a random unhit
    /// foe in the gap (`Skill_AttackChain::ActivateNow`,
    /// `GameEngine::PickObject`).
    Chain,
    /// `Skill_AttackPathCharge` (Vire's Might): a dash to the point, then a
    /// radius hit ([`PathCharge`]).
    PathCharge,
    /// `Skill_DispelMagic` (Nullification): around the point
    /// (`projectileExplosionRadius`), see [`Dispel`].
    Dispel,
    /// Instant effects on the caster: `Skill_GiveBonus` (life / energy),
    /// `Skill_RefreshCooldown`.
    Instant,
    /// `Skill_Shapeshift`: toggles the form (`shapeshift.rs`).
    Shapeshift,
    /// `Skill_AttackSpellReflectLink` (Mark of Torment): a link to the
    /// target (`gd_game::reflect_link`).
    ReflectLink,
    Unsupported,
}

pub fn kind_of(rec: &Record) -> Kind {
    let c = rec.class.as_str();
    match c {
        "Skill_WPAttack_BasicAttack" => Kind::Weapon,
        // The kick (`crate::hotbar::kicks_breakable`): a melee attack with the
        // record's own `weaponDamagePct` and flat offense, no weapon pool
        // (`Skill_Kick::TargetResult`).
        "Skill_Kick" => Kind::Weapon,
        // Weapon-pool skills (Fire Strike, Cadence…): weapon attacks that roll
        // the pool (`weapon_pool.rs`).
        _ if c.starts_with("Skill_WeaponPool") => Kind::Weapon,
        _ if c.starts_with("Skill_WPAttack") || c.starts_with("Skill_AttackWeapon") => Kind::Weapon,
        "Skill_AttackProjectileDrop" => Kind::Drop,
        "Skill_BuffAttackRadiusDrop" => Kind::Storm,
        "Skill_AttackProjectileAreaEffect" => Kind::Area,
        "Skill_CharonGeysers" => Kind::Geysers,
        "Skill_TurretFireControl" => Kind::TurretFire,
        "Skill_AttackPattern" => Kind::Pattern,
        // Channelled skills (`SkillChanneled` subclasses, RTTI).
        "Skill_AttackSpellBeam"
        | "Skill_AttackSpellCone"
        | "Skill_AttackSpellDrain"
        | "Skill_AttackRadiusGrow"
        | "Skill_AttackRadiusSpin" => Kind::Beam,
        // Leaps and teleports: a fast move to the point, then the radius.
        "Skill_AttackRadiusLeap"
        | "Skill_AttackRadiusDisengage"
        | "Skill_AttackRadiusTeleport"
        | "Skill_AttackSpellTeleportSelf" => Kind::PathCharge,
        // Spells on the target: the target foe alone, no radius
        // (`Skill_AttackSpellChaos::ActivateNow` needs a foe,
        // `TargetResult` hits it). See `single_target`.
        "Skill_AttackSpellChaos" | "Skill_AttackSpellChaosSpawnPet" => Kind::TargetRadius,
        "Skill_AttackLineFan" => Kind::LineFan,
        "Skill_AttackLine" => Kind::Line,
        // Self buffs (`Skill_BuffSelfDuration` / `…Toggled` subclasses).
        "Skill_BuffSelfImmobilize" | "Skill_BuffSelfShield" | "Skill_BuffAttackRadiusLightning" => {
            Kind::Buff
        }
        "Skill_GiveBonus" | "Skill_RefreshCooldown" | "SkillActivated_Suicide" => Kind::Instant,
        "Skill_Shapeshift" => Kind::Shapeshift,
        "Skill_AttackSpellReflectLink" => Kind::ReflectLink,
        "Skill_DispelMagic" => Kind::Dispel,
        "Skill_AttackPathCharge" => Kind::PathCharge,
        "Skill_AttackChain" => Kind::Chain,
        // A direct spell on the target (item skills such as Domination): the
        // target entity alone (`Skill_AttackSpell::ActivateNow`
        // lists just it). See `single_target`.
        "Skill_AttackSpell" => Kind::TargetRadius,
        // A `Skill_AttackSpell` that swaps places with its target first (see
        // `gd_game::casting::swap_places`).
        "Skill_AttackSpellTeleport" => Kind::TargetRadius,
        // A timed damaging aura: a buff plus [`DamageAura`]s for its duration.
        "Skill_BuffAttackRadiusDuration" => Kind::Buff,
        _ if gd_sim::skills::is_toggle(c) => Kind::Toggle,
        _ if c.starts_with("Skill_AttackRadiusLightning") => Kind::TargetRadius,
        // Point-blank attack buffs (Chilling Surge) hit around the caster.
        "Skill_AttackBuffRadius" if rec.bool("pointBlank") => Kind::SelfRadius,
        "Skill_AttackBuffRadius" => Kind::TargetRadius,
        // A debuff on the target (Bloody Pox): hits around the target point
        // (radius 1 m without `skillTargetRadius`).
        "Skill_AttackBuff" => Kind::TargetRadius,
        // Monster summons and generators spawn through `SpawnPet` too.
        // `Skill_SpawnMiniPet` is a `Skill_SpawnPet` with its own pet limit
        // (`pets.rs` module docs).
        // `Skill_AktaiosMirage` is a `Skill_SpawnPet` that also moves its
        // caster (`monster_skills::MIRAGE`).
        "Skill_TargetedSpawnPet"
        | "Skill_SpawnPet"
        | "Skill_SpawnMiniPet"
        | "Skill_SpawnPetMonster"
        | "Skill_MonsterGenerator"
        | "Skill_AktaiosMirage" => Kind::Pet,
        "Skill_AttackWave" => Kind::Wave,
        "Skill_AttackRadius" => Kind::SelfRadius,
        _ if c.starts_with("Skill_AttackProjectile") => Kind::Projectile,
        // `Skill_BuffSelfSick` is a `Skill_BuffSelfDuration` (its stuns:
        // `crate::afflictions` module docs).
        "Skill_BuffSelfDuration" | "Skill_BuffSelfSick" | "Skill_BuffRadius" => Kind::Buff,
        // A self buff that also grows the caster (`crate::colossus`).
        "Skill_BuffSelfColossus" => Kind::Buff,
        _ => Kind::Unsupported,
    }
}

/// Running cooldowns by skill (or scroll key): seconds left, the total
/// they started with and the skill's cooldown charges. The engine's `Skill`
/// keeps the timers (`Skill.cooldownLeft`, `Skill.cooldownTotal` ms; a refresh shortens only
/// the left time): the hotbar's wipe is `1 − left / total`
/// (`Skill::GetCooldownCompletion`, `GetCooldownRemaining`,
/// `GetCooldownTotal`).
///
/// Charges (`cooldownCharges`, modifier-summed;
/// `Skill::StartCooldown` / `EndCooldown`): a use
/// spends one charge and starts the cooldown only while none runs; each
/// finished cooldown gives a charge back and starts the next while charges
/// are still spent (`SetChargeCooldown`, `UpdateCooldownCharges`, the potion
/// rule generalized). A skill is usable while it has an unspent charge and no
/// disruption's cooldown runs on it ([`Self::replace`]: a disrupted skill
/// waits it out whatever charges it has left).
/// `StartCooldown` treats a cooldown of 0.1 s or less as none.
#[derive(Clone, Debug, Default)]
pub struct Cooldowns(HashMap<String, Cooldown>);

/// One skill's running cooldown and its charge state ([`Cooldowns`]).
#[derive(Clone, Debug)]
struct Cooldown {
    left: f32,
    total: f32,
    /// Charges spent: each running cooldown holds one.
    used: u32,
    /// The modifier-summed `cooldownCharges` (1: an ordinary skill).
    max: u32,
    /// A disruption's cooldown ([`Cooldowns::replace`]) runs: the skill is
    /// unusable until it ends, whatever charges are left.
    disrupted: bool,
}

impl Cooldowns {
    /// `Skill::StartCooldown` without charges (scrolls, an item
    /// toggle's re-arm): [`Self::start`] with one charge.
    pub fn insert(&mut self, skill: String, secs: f32) {
        self.start(skill, secs, 1);
    }
    /// `Skill::StartCooldown`: a cooldown of 0.1 s
    /// or less is none; else a charge is spent and the timer starts only
    /// while none runs (a running one keeps its time, the charge is held).
    pub fn start(&mut self, skill: String, secs: f32, charges: u32) {
        if secs <= 0.1 {
            return;
        }
        let max = charges.max(1);
        let c = self.0.entry(skill).or_insert(Cooldown {
            left: 0.0,
            total: 0.0,
            used: 0,
            max,
            disrupted: false,
        });
        c.max = max;
        c.used = (c.used + 1).min(max);
        if c.left <= 0.0 {
            c.left = secs;
            c.total = secs;
        }
    }
    /// Whether the skill can be used: no disruption runs, and no cooldown runs
    /// or a charge is left.
    pub fn usable(&self, skill: &str) -> bool {
        self.0
            .get(skill)
            .is_none_or(|c| !c.disrupted && c.used < c.max)
    }
    /// Seconds left.
    pub fn get(&self, skill: &str) -> Option<&f32> {
        self.0.get(skill).map(|c| &c.left)
    }
    /// Seconds left of `skill`'s cooldown, case-insensitive (the proc
    /// stores' keys).
    pub fn left_of(&self, skill: &str) -> Option<f32> {
        self.0
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(skill))
            .map(|(_, c)| c.left)
    }
    pub fn contains_key(&self, skill: &str) -> bool {
        self.0.contains_key(skill)
    }
    /// Elapsed part, 0..1 (`Skill::GetCooldownCompletion`).
    pub fn completion(&self, skill: &str) -> Option<f32> {
        self.0.get(skill).map(|c| {
            if c.total > 0.0 {
                (1.0 - c.left / c.total).clamp(0.0, 1.0)
            } else {
                1.0
            }
        })
    }
    /// `Skill::EndCooldown(ms)`: `secs` off the
    /// time left of `skill`'s cooldown (any of `names`), the total kept; at
    /// 0 a charge comes back and the next starts while charges are spent.
    /// False when none runs.
    pub fn shorten(&mut self, names: &[String], secs: f32) -> bool {
        let Some(k) = self
            .0
            .keys()
            .find(|k| names.iter().any(|n| n.eq_ignore_ascii_case(k)))
            .cloned()
        else {
            return false;
        };
        let c = self.0.get_mut(&k).expect("found above");
        c.left -= secs;
        Self::end(&mut self.0, &k);
        true
    }
    /// Whether any of `names` cools down.
    pub fn any_of(&self, names: &[String]) -> bool {
        self.0
            .keys()
            .any(|k| names.iter().any(|n| n.eq_ignore_ascii_case(k)))
    }
    /// Runs every cooldown down by `dt`; a finished one gives its charge
    /// back and starts the next while charges are spent (`EndCooldown`).
    pub fn tick(&mut self, dt: f32) {
        let mut ended = Vec::new();
        for (k, c) in &mut self.0 {
            c.left -= dt;
            if c.left <= 0.0 {
                ended.push(k.clone());
            }
        }
        for k in ended {
            Self::end(&mut self.0, &k);
        }
    }
    /// A cooldown reached 0: a charge comes back; while charges are still
    /// spent the next cooldown starts (`SetChargeCooldown`), else the entry
    /// ends. A disruption's cooldown ([`Self::replace`]) holds no charge of
    /// its own: it ends the block and returns only the charge of the cooldown
    /// it replaced, if one ran.
    fn end(map: &mut HashMap<String, Cooldown>, k: &str) {
        let Some(c) = map.get_mut(k) else { return };
        if c.left > 0.0 {
            return;
        }
        c.disrupted = false;
        c.used = c.used.saturating_sub(1);
        if c.used > 0 {
            c.left = c.total;
        } else {
            map.remove(k);
        }
    }
    /// `SkillManager::RefreshCooldown`: every
    /// running cooldown's time left shortens by `secs` (`EndCooldown`), the
    /// `except` keys skipped (the caster's own skill; the item list's
    /// potions); a finished one returns its charge as [`Self::shorten`] does.
    pub fn refresh_all(&mut self, secs: f32, except: &[String]) {
        let keys: Vec<String> = self
            .0
            .keys()
            .filter(|k| !except.iter().any(|e| e.eq_ignore_ascii_case(k)))
            .cloned()
            .collect();
        for k in keys {
            if let Some(c) = self.0.get_mut(&k) {
                c.left -= secs;
            }
            Self::end(&mut self.0, &k);
        }
    }
    /// `Skill::ReplaceCooldownTime(ms, 0)` (disruption's): `skill` cools down for `secs`
    /// unless its running cooldown has more left; the total becomes `secs` too. The charge state stays. PORT: a
    /// replaced cooldown blocks the skill until it ends, charges left or not (a disrupted skill can't be used; how
    /// the engine's charge check reads a replaced cooldown is not traced); a running cooldown with more left keeps
    /// its time and its charges.
    pub fn replace(&mut self, skill: &str, secs: f32) {
        if secs <= 0.0 {
            return;
        }
        match self.0.get_mut(skill) {
            Some(c) => {
                if c.left < secs {
                    c.left = secs;
                    c.total = secs;
                    c.disrupted = true;
                }
            }
            None => {
                self.0.insert(
                    skill.to_owned(),
                    Cooldown {
                        left: secs,
                        total: secs,
                        used: 0,
                        max: 1,
                        disrupted: true,
                    },
                );
            }
        }
    }
}

/// [`Cooldowns::replace`] on a monster's or pet's (skill, seconds left) list.
pub fn replace_cooldown(list: &mut Vec<(String, f32)>, skill: &str, secs: f32) {
    if secs <= 0.0 {
        return;
    }
    match list.iter_mut().find(|c| c.0 == skill) {
        Some(c) => c.1 = c.1.max(secs),
        None => list.push((skill.to_owned(), secs)),
    }
}

/// The player's hotbar, energy and cooldowns. The timed self buffs are
/// `gd_game::player::Loadout::buffs` (`crate::skill_buffs::TimedBuffs`, the
/// one store player / monster / pet share).
#[derive(Component, Clone)]
pub struct Caster {
    /// Save hotbar slots (see [`SLOT_LEFT`]).
    pub slots: [Option<String>; SLOTS],
    pub energy: f32,
    pub cooldowns: Cooldowns,
    /// The hotbar pages of the skill sets that are not displayed
    /// (`PlayerHotSlotCtrl`'s per-set `SkillSet` map, key = set id; the
    /// save's `bars`, `WriteProperties` writes every page).
    /// `slots` is the displayed page, of set [`Caster::shown`].
    pub sets: std::collections::BTreeMap<i32, [Option<String>; SLOTS]>,
    /// The active skill set (`PlayerHotSlotCtrl.activeSkillSet`; 0 = the default set,
    /// `kDefaultSkillSet`).
    pub set: i32,
    /// The displayed skill set, whose page `slots` is: [`Caster::set`] unless Show Alternate Hot Slot Set shows
    /// another page ([`Caster::next_displayed_set`]). `PlayerHotSlotCtrl::GetDisplayedSkillSetIndex`
    /// (`PlayerHotSlotCtrl.displayedSkillSet`, 0 = the active set's) and `GetDisplayedSkillSet`: every slot read, use
    /// and bind (`GetHotSlotOption`, `ActivateHotSlot`, `SetHotSlot`, the mouse slots) goes to the displayed page.
    pub shown: i32,
}

impl Caster {
    /// `PlayerHotSlotCtrl::SetActiveSkillSet`: shows the page
    /// of set `id`, creating an empty one on first use (returns true then).
    /// The displayed page goes back to the active set's (`PlayerHotSlotCtrl::Update`: a new
    /// active set clears the displayed one, `PlayerHotSlotCtrl.displayedSkillSet`).
    pub fn set_active_set(&mut self, id: i32) -> bool {
        if id == self.set {
            return false;
        }
        self.set = id;
        self.show_page(id)
    }

    /// `slots` becomes set `id`'s page (an empty one when it has none: returns true then).
    fn show_page(&mut self, id: i32) -> bool {
        if id == self.shown {
            return false;
        }
        let page = std::mem::take(&mut self.slots);
        self.sets.insert(self.shown, page);
        self.shown = id;
        match self.sets.remove(&id) {
            Some(page) => {
                self.slots = page;
                false
            }
            None => true,
        }
    }

    /// Shows the page of the next set that has one, after the last the active set's.
    /// `PlayerHotSlotCtrl::NextDisplayedSkillSet`: the set map's entry after the displayed
    /// id (`PlayerHotSlotCtrl.displayedSkillSet`), past the end 0 (= the active set's page); no entry for the
    /// displayed id, no change. The HUD calls
    /// it only while the active set is 0 (`ui::hud`).
    pub fn next_displayed_set(&mut self) {
        let next = self
            .sets
            .keys()
            .copied()
            .filter(|k| *k > self.shown)
            .min()
            .unwrap_or(self.set);
        self.show_page(next);
    }

    /// The active set's page again. `PlayerHotSlotCtrl::ResetDisplayedSkillSet` (`displayedSkillSet` = 0).
    pub fn reset_displayed_set(&mut self) {
        self.show_page(self.set);
    }

    /// Every page with its set id (the displayed one included), for saving.
    pub fn pages(&self) -> Vec<(i32, &[Option<String>; SLOTS])> {
        let mut v: Vec<_> = self.sets.iter().map(|(k, p)| (*k, p)).collect();
        v.push((self.shown, &self.slots));
        v.sort_by_key(|p| p.0);
        v
    }
}

/// A skill fired at once, without animation or energy (autocast procs).
#[derive(Message, Clone)]
pub struct ProcCast {
    pub caster: Entity,
    pub cast: Casting,
}

/// A cast in progress, fired on the animation's hit callback.
#[derive(Component, Clone)]
pub struct Casting {
    pub skill: String,
    pub level_index: usize,
    pub kind: Kind,
    pub point: Vec3,
    pub target: Option<Entity>,
    /// Cast by a monster: hits the player and their pets, not monsters.
    pub hostile: bool,
    /// The weapon-pool skill a pool attack runs under (`weapon_pool.rs`).
    pub parent: Option<(String, usize)>,
}

/// A weapon charge's warm-up paid its cost and started its cooldown
/// (`try_cast`'s `Use::Request` walk, `WarmUpStart`); the cast it arrives at
/// fires without paying again (`Skill_AttackWeaponCharge::ActivateNow`
/// bumps the hit iteration, only the first pays). It names the
/// skill's cast record ([`Casting::skill`]): only that cast, reached by the
/// charge's own arrival (`Use::ArrivedWarm`), skips the payment. `fire_casts`
/// takes it off as the cast fires; a cut-short charge keeps what it paid, and
/// whatever replaces or cancels its move (another walk or cast, a move
/// order, a stun, death) drops the marker.
#[derive(Component)]
pub struct WarmPaid(pub String);

/// The cast whose animation may still be playing after its hit fired it: a
/// later swipe callback of that animation still belongs to the skill
/// (`crate::combat_sounds::swipe_action`; 181 stock animations swipe after
/// their first hit).
#[derive(Component, Clone, Debug)]
pub struct LastCast {
    pub skill: String,
    pub weapon: bool,
    pub anim: gd_sim::anim::AnimType,
}

/// Marks a player whose left-click skill is a spell: clicked monsters are
/// attacked by casting it (`combat::player_attack` stays out of it).
#[derive(Component)]
pub struct SpellAttacker;

/// What learned modifiers add to a projectile skill: `projectilePiercing`
/// (Destabilization) and further fragment sets (`Skill_ProjectileModifier`
/// `projectileFragmentsName`, e.g. Replicating Missile's aether fragments);
/// and the cast's modifier visual overrides (`crate::skill_fx`).
#[derive(Clone, Default, Debug)]
pub struct ProjMods {
    pub pierce: f32,
    pub fragments: Vec<(String, u32, u32)>,
    /// The skill's fragments when a modifier overrides them (`crate::skill_fx::fragments_name`).
    pub fragments_name: Option<String>,
    /// Effects added to the projectile (`SkillActivated::AddProjectileEffects`, `crate::skill_fx::projectile_fx`).
    pub effects: Vec<String>,
}

impl ProjMods {
    pub fn of(
        db: &gd_formats::vfs::Database,
        parents: &HashMap<String, String>,
        book: &gd_sim::skills::SkillBook,
        skill: &str,
    ) -> Self {
        let mut m = ProjMods::default();
        for (s, l) in &book.skills {
            if *l == 0
                || !parents
                    .get(&s.to_ascii_lowercase())
                    .is_some_and(|p| p.eq_ignore_ascii_case(skill))
            {
                continue;
            }
            let Some(rec) = db.get(s) else { continue };
            let li = book.effective_level(db, s).max(1) as usize - 1;
            m.pierce += level_f(rec, "projectilePiercing", li);
            if let Some(name) = rec.str("projectileFragmentsName").filter(|n| !n.is_empty()) {
                let lo = level_f(rec, "projectileFragmentsLaunchNumberMin", li).max(0.0) as u32;
                let hi = (level_f(rec, "projectileFragmentsLaunchNumberMax", li) as u32).max(lo);
                m.fragments.push((name.to_owned(), lo, hi));
            }
        }
        m
    }
}

pub fn level_f(rec: &Record, k: &str, li: usize) -> f32 {
    gd_sim::stats::level_value(rec, k, li).unwrap_or(0.0)
}

/// Any skill but the default attack on the left button is cast on every
/// attack (spells, and weapon skills such as Fire Strike).
pub fn spell_left(data: &GameData, caster: &Caster) -> bool {
    caster.slots[SLOT_LEFT]
        .as_deref()
        .filter(|s| !s.eq_ignore_ascii_case(DEFAULT_ATTACK))
        .and_then(|s| data.db.get(s))
        .is_some_and(|r| kind_of(r) != Kind::Unsupported)
}

/// Use a hotbar slot (the game sends it for keys and mouse buttons): at
/// `target` (the character under the cursor: a foe, or one's own pet) or the
/// ground `point`; `force` is Shift + left button (attack in place);
/// `scripted` marks test-hook casts (their channels end after 2 s).
///
/// The aim by the skill's [`TargetType`](gd_sim::skills::TargetType),
/// `ControllerPlayerState::DefaultRequestSkillAction` (its
/// target is the combat enemy for enemy and location skills, the combat ally
/// for ally skills, the player for self skills; the controller's enemy / ally
/// are the foe / friend under the cursor, the player never one, `InGameUIActorCapture::CollectCandidates`):
/// - `Ally` (`Skill_GiveBonus`, `Skill_BuffOther`): the friend under the
///   cursor, else the request fails (`Skill_GiveBonus::GetValidTarget`:
///   a projectile skill takes any target; one in the world
///   passes; else `Skill::GetValidFriendTarget` with radius 0
///   (in `DefaultRequestSkillAction`) finds none): no cast, no cost, a heal key over the
///   ground does nothing;
/// - `Enemy` with a friend under the cursor and no foe: refused
///   (`Skill::CanTargetFriendAsEnemy`, false for every class), silently;
/// - otherwise the foe (if any) and the point;
/// - then the spell family's `SkillActivatedSpell::GetValidTarget`
///   ([`gd_sim::skills::request_aim`]): a `targetingMode` `Point` or
///   ground-only spell drops the foe, any other is refused without a foe.
///   PORT: the friend is one's own living pet.
#[derive(Message, Clone, Debug)]
pub struct CastRequest {
    pub slot: usize,
    pub target: Option<Entity>,
    pub point: Vec3,
    pub force: bool,
    pub scripted: bool,
}

/// A press on a pet portrait with a hot slot armed (`grimdawn` `ui/pets`):
/// the armed slot's skill is requested on the pet (`gd_game::hotbar` takes the
/// armed slot and turns it into a [`CastRequest`] at the pet).
///
/// `PlayerPortrait::HandleMouseEvent` (`Grim Dawn.exe`, the portrait's mouse handler)
/// and Game.dll: with `PlayerHotSlotCtrl::IsAnySlotActive` (a slot armed,
/// `SkillSet.activeSlot`) and the hot slots not disabled (`PlayerHotSlotCtrl.disableHotSlotsOverride`,
/// `SetDisableHotSlotsOverride`), a left or right press on the portrait's
/// frame rect sets the pet as the controller's combat ally, clears its
/// combat enemy and runs `ControllerPlayer::HandleActionFromMouse` at the
/// pet's coordinates with the pet's id; its first step,
/// `ControllerPlayerState::ActivateSuperSkill`, takes (and
/// clears) the armed slot and requests its skill
/// (`DefaultRequestSkillAction`), which aims it by the skill's
/// [`TargetType`](gd_sim::skills::TargetType) like any request
/// (`gd_game::casting::cast_requests`). PORT: the friend is the player's own pet (single
/// player has no party frames).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct CastOnAlly {
    pub ally: Entity,
}

/// The wrapper skill a hot slot cast ran as its `buffSkillName` record
/// (`Casting::skill` is `buff`): what `SkillActivated::ApplyBuffOnTarget`'s
/// `CastBuff` / `CastDebuf` triggers classify (`crate::autocast`). The last
/// use's, instant ones included; the pending animated cast's cooldown key is
/// [`CastCooldownKey`].
#[derive(Component, Clone, Debug)]
pub struct CastWrapper {
    pub buff: String,
    pub wrapper: String,
}

/// The wrapper a pending animated hot slot cast ([`Casting`] with `skill` =
/// `buff`) runs under: its cooldown's key and its item-skill check at the fire
/// (`fire_casts`; `gd_game::casting::try_cast` checks it, the hotbar shows it). Set with the
/// [`Casting`] only, so an instant cast during the animation does not change it.
#[derive(Component, Clone, Debug)]
pub struct CastCooldownKey(pub CastWrapper);

/// A hot slot skill was pressed while it cooled down (`Player::PlaySkillCooldownVox` and the UI's
/// `tagSkillNotReady` text).
#[derive(Message, Clone, Copy, Debug)]
pub struct SkillNotReady;

/// A channelled skill (`SkillChanneled` subclasses): it ticks
/// every `timeBetweenAttacks` ms (default 100 for every class), ÷ cast
/// speed (Spin: ÷ attack speed, `Skill_AttackRadiusSpin::Update`);
/// each tick pays the full `skillManaCost` and the channel
/// stops when the energy left is under it (`Skill_AttackSpellBeam::Update`);
/// releasing the key ends it (`SkillChanneled::Update`'s
/// timer, re-armed by every tick). The tick timer is the
/// engine's (the same in all five `Update`s: Spin,
/// Grow, Drain, Cone, Beam):
/// `timer -= frame ms; if timer <= 0 { timer =
/// (int)(timeBetweenAttacks / speed); tick }`, so a tick re-arms the whole
/// period (what the frame overshot is dropped) and there is one tick per
/// update at most: the rate follows the frame time by design, don't "fix"
/// it. PORT: the period is not cut to whole ms. Death and a stun end the
/// channel (`gd_game::casting::stop_skills`). A test cast channels for
/// 2 s. The beam turns to the cursor at once (the game's
/// `rotationSpeedMultiplier` turn rate is not done); its rays, strikes and
/// energy are `gd_game::casting::spell_beams`', the ribbons and effects grimdawn `beams.rs`'.
/// A cone's strikes, energy and target are its cone entry's (`class_fx::ConeFx`); each re-request retargets it
/// (`Skill_AttackSpellCone::GetValidTarget`: the request's target, [`Channel::target`]).
#[derive(Component)]
pub struct Channel {
    pub skill: String,
    pub level_index: usize,
    /// The hotbar slot it was started from.
    pub slot: usize,
    pub tick: f32,
    pub scripted: bool,
    /// The slot's key / button is still held (set by the game's input).
    pub input_held: bool,
    /// The cursor's ground point (set by the game's input).
    pub input_aim: Option<Vec3>,
    /// The point the channel aims at this frame (the cursor's, or the keyboard modes' `keyboard_aim`): a beam's
    /// slope (`gd_game::casting::spell_beams`).
    pub aim: Option<Vec3>,
    /// Seconds channelled (Winds of Asterkarn's growth).
    pub held: f32,
    /// The targeted foe: a drain's; a cone's, re-read from each re-request (the cursor's character; scripted: the
    /// nearest foe).
    pub target: Option<Entity>,
}

/// A grow channel's radius after `held` seconds (`Skill_AttackRadiusGrow::Update`): `skillTargetRadius`
/// plus `radiusIncrement` per `timeBetweenGrow` ms (default 100) ÷ cast speed, up to `explosionRadius`.
pub fn grow_radius(rec: &Record, li: usize, held: f32, spell_speed: f32) -> f32 {
    let r = level_f(rec, "skillTargetRadius", li).max(1.0);
    let steps = (held * 1000.0 * spell_speed.max(0.2)
        / rec.f32("timeBetweenGrow").unwrap_or(100.0).max(1.0))
    .floor();
    (r + steps * rec.f32("radiusIncrement").unwrap_or(0.0))
        .min(rec.f32("explosionRadius").unwrap_or(r).max(r))
}

/// The cone's frustum (`Skill_AttackSpellCone::GetTargets` → `Skill_AttackSpellCone::BuildFrustum`): the near face
/// at `near`, the far face at `far`, each `w0` / `w1` wide along the caster's `right` axis and `h` high along its
/// `up` axis; the near / far planes face the caster's forward `fwd`, the four sides join the faces' edges. True when
/// the box `b` meets it. PORT: `GetTargetsInFrustum` against the foe's world box (a plane test: the box's centre
/// within each plane's distance plus the box's projected half extent).
#[allow(clippy::too_many_arguments)]
pub fn cone_frustum_meets_box(
    near: Vec3,
    far: Vec3,
    fwd: Vec3,
    right: Vec3,
    up: Vec3,
    w0: f32,
    w1: f32,
    h: f32,
    b: &gd_sim::bounds::Aabb,
) -> bool {
    let c = Vec3::from(b.center);
    let half = Vec3::from(b.half);
    let reach = |n: Vec3| n.abs().dot(half);
    // Outward normal and a point of each plane.
    let side = |axis: Vec3, other: Vec3, a: f32, b: f32| {
        let (p, q) = (near + axis * a * 0.5, far + axis * b * 0.5);
        let n = (q - p).cross(other).normalize_or_zero();
        (if n.dot(axis) < 0.0 { -n } else { n }, p)
    };
    let planes = [
        (-fwd, near),
        (fwd, far),
        side(right, up, w0, w1),
        side(-right, up, w0, w1),
        side(up, right, h, h),
        side(-up, right, h, h),
    ];
    planes.iter().all(|(n, p)| (c - *p).dot(*n) <= reach(*n))
}

/// The `GD_CAST` script: `state` (a `Local` of the calling system) holds the start time and the entries left.
pub fn test_cast(
    state: &mut Option<(std::time::Instant, Vec<(f32, usize)>)>,
    hooks: &crate::hooks::Hooks,
) -> Option<usize> {
    let st = state.get_or_insert_with(|| (std::time::Instant::now(), hooks.gameplay.cast.clone()));
    let now = st.0.elapsed().as_secs_f32();
    let i = st.1.iter().position(|(t, _)| now >= *t)?;
    Some(st.1.remove(i).1)
}

/// A skill's own part of `Skill::GetTotalSkillAttribute` (see `gd_game::casting::cast_requests`): `k` over
/// its record and modifiers (each gated by its `{k}Chance`, a fresh roll per call), plus |that| ×
/// the same sum of `{k}Modifier` %.
pub fn own_skill_attribute<'a>(
    recs: impl Iterator<Item = (&'a Record, usize)> + Clone,
    k: &str,
) -> f32 {
    let rolled = |k: &str| {
        recs.clone()
            .map(|(r, l)| {
                let c = level_f(r, &format!("{k}Chance"), l);
                if c > 0.0 && c < 100.0 && crate::rng::f32() * 100.0 > c {
                    0.0
                } else {
                    level_f(r, k, l)
                }
            })
            .sum::<f32>()
    };
    let own = rolled(k);
    let own_pct = rolled(&format!("{k}Modifier"));
    own + own.abs() * own_pct * 0.01
}

/// The beam nodes of a caster's channelled beam skill, the held cast's last (`Skill_AttackSpellBeam.beamNodes` list).
#[derive(Component, Default)]
pub struct SpellBeams(pub Vec<BeamNode>);

/// One cast's rays.
pub struct BeamNode {
    /// Unique per caster (the drawing's key).
    pub id: u64,
    pub skill: String,
    pub level_index: usize,
    pub beam: SpellBeam,
    /// The fan's turn of each ray (radians).
    pub angles: Vec<f32>,
    /// The heading with its slope (kept while retracting): the start effect sits at `emit + heading * retract`.
    pub heading: Vec3,
    /// The emit point this frame.
    pub emit: Vec3,
    /// Each ray's direction this frame.
    pub dirs: Vec<Vec3>,
    /// Each ray meets a surface at its longest (its ground effect emits).
    pub surface: Vec<bool>,
    /// `beamName`, `startEffect`, `endEffect` as `ActivateNow` resolved them (`crate::skill_fx::beam_effects`).
    pub effects: [Option<String>; 3],
}

impl BeamNode {
    /// Ray `k`'s drawn segment: from the retract point to its far end.
    pub fn segment(&self, k: usize) -> (Vec3, Vec3) {
        let d = self.dirs[k];
        (
            self.emit + d * self.beam.retract,
            self.emit + d * self.beam.lens[k],
        )
    }
}

/// The skill's ray count and spread (degrees) at `li`, with the learned modifiers' (`GetSkillModifierData`).
pub fn fan(data: &GameData, rec: &Record, li: usize, mods: &[(String, usize)]) -> (usize, f32) {
    let sum = |k: &str| {
        level_f(rec, k, li)
            + mods
                .iter()
                .filter_map(|(m, ml)| data.db.get(m).map(|r| level_f(r, k, *ml)))
                .sum::<f32>()
    };
    ((sum("beamNumber") as usize).max(1), sum("beamAngle"))
}

/// One chaos skill cast in flight (`Skill_AttackSpellChaos`'s entry): its own entity.
#[derive(Component, Clone, Debug)]
pub struct ChaosBeam {
    pub caster: Entity,
    pub target: Entity,
    pub skill: String,
    pub level_index: usize,
    /// The weapon-pool parent the hit runs under ([`SkillHit::parent`]).
    pub parent: Option<(String, usize)>,
    /// The `ChaosBeam` record drawn.
    pub beam: String,
    /// The caster's `R Hand` when the beam was created (`None` before the first update).
    pub start: Option<Vec3>,
    /// The far end: the target's `Target` point while it is followed.
    pub end: Vec3,
    /// Metres the head / tail travelled from `start`.
    pub head: f32,
    pub tail: f32,
    /// The drawn part (`SetTopBottomPercent`): fractions of the way from `start`.
    pub top: f32,
    pub bottom: f32,
    /// The head arrived or the target went (state 2).
    pub tailing: bool,
}

impl ChaosBeam {
    pub fn new(
        caster: Entity,
        target: Entity,
        skill: String,
        level_index: usize,
        parent: Option<(String, usize)>,
        beam: String,
    ) -> Self {
        Self {
            caster,
            target,
            skill,
            level_index,
            parent,
            beam,
            start: None,
            end: Vec3::ZERO,
            head: 0.0,
            tail: 0.0,
            top: 0.0,
            bottom: 0.0,
            tailing: false,
        }
    }

    /// One `Update` step of `dt` s after the creation (states 1 and 2); `target`: its `Target` point and whether it
    /// is alive, if it still resolves. Returns `(arrived, done)`: the head arrived this step (the hit), the beam
    /// is done (state 3).
    pub fn step(
        &mut self,
        dt: f32,
        head_v: f32,
        tail_v: f32,
        target: Option<(Vec3, bool)>,
    ) -> (bool, bool) {
        let start = self.start.unwrap_or(self.end);
        if !self.tailing {
            let Some((end, _)) = target else {
                self.tailing = true;
                return (false, false);
            };
            self.end = end;
            let len = end.distance(start);
            self.head = (self.head + dt * head_v).max(0.0).min(len);
            self.top = if len > 0.0 {
                (self.head / len).clamp(0.0, 1.0)
            } else {
                1.0
            };
            self.tailing = self.top >= 1.0;
            return (self.tailing, false);
        }
        if let Some((end, true)) = target {
            self.end = end;
        }
        let len = self.end.distance(start);
        self.tail = (self.tail + dt * tail_v).max(0.0).min(len);
        self.bottom = if len > 0.0 {
            (self.tail / len).clamp(0.0, 1.0)
        } else {
            1.0
        };
        if self.bottom >= self.top {
            self.bottom = self.top;
            return (false, true);
        }
        (false, false)
    }
}

/// Nullification (`Skill_DispelMagic::TargetResult`): foes in
/// the radius take the skill (its debuff) and lose their
/// self buffs (`SkillManager::DispelSkillBuffs`), except skills
/// flagged `notDispelable`; everyone on the caster's side in the radius
/// (the caster too) loses their skill debuffs (`DispelSkillDeBuffs`)
/// and, with `dispelDamageOverTime`, their damage over time
/// (`crate::skill_buffs::DispelOn`). `skillActiveDuration` is unused.
#[derive(Message, Clone)]
pub struct Dispel {
    pub caster: Entity,
    pub at: Vec3,
    pub radius: f32,
    pub foes: Vec<Entity>,
    pub dots: bool,
}

/// `Skill_AttackPathCharge` in flight (Vire's Might): runs straight to the
/// point (`waveDistance` at most) and hits
/// every foe within `skillTargetRadius` × `endRadiusMultiplier` there. On
/// the way, every `timeBetweenAttacks` ms (default 100) it hits the foes
/// within `skillTargetRadius` of the caster not hit yet, each once
/// (`Skill_AttackPathCharge::Load` / `Update`). Walls: the move
/// follows a nav path, and fails (the skill lands where the caster stands)
/// when that path is longer than `maxMoveRatio` × the straight distance
/// (`Character::GetMoveToPoint` passes `SkillProfile`'s
/// `maxMoveRatio` to `CharacterMovementManager::FindPath`).
/// The run: the skill moves nothing itself (`ActivateNow`,
/// `Update`, `WarmUpStart`: no move); its move
/// is the controller's move type 2 one (ChargeToUseSkill, a monster's Pursue):
/// the Charge clip at speed factor 1, at the warmed-up run speed when the
/// warm-up started (`skillAllowsWarmUp`), `crate::combat::charge_move`. A set
/// without the Charge slot does not move. PORT: the run follows the cast.
#[derive(Component)]
pub struct PathCharge {
    /// Nav waypoints left to the end point.
    pub to: Vec<Vec3>,
    /// Fixed speed (m/s, a teleport's), or `None`: the Charge clip's run (`crate::combat::charge_move`).
    pub speed: Option<f32>,
    /// The warm-up started (`skillAllowsWarmUp`): the run is at the warmed-up run speed with this
    /// `characterRunSpeedModifier` %.
    pub warm: bool,
    pub bonus: f32,
    pub skill: String,
    pub level_index: usize,
    pub radius: f32,
    pub hostile: bool,
    pub left: f32,
    /// Where the caster stood when it started ([`ChargeStarts`]).
    pub start: Vec3,
    /// Hits along the way (`Skill_AttackPathCharge` only): radius (0 =
    /// none), seconds between, seconds to the next, foes already hit.
    pub path: (f32, f32, f32, Vec<Entity>),
    /// `Skill_AttackPathCharge` only: `secondarySkillDistance` (`Load`) and the spot the special secondaries
    /// last went off (`Skill_AttackPathCharge.lastSecondarySpot`, the start: `WarmUpStart`). `Update`: each frame
    /// of the run, once the caster is that far (flat) from the spot, the spot
    /// moves to it and `SpecialActivateSecondarySkills` runs there with no
    /// targets (a distance of 0: every frame).
    pub secondaries: Option<(f32, Vec3)>,
    /// The arrival's radius effect, resolved at the cast (`crate::skill_fx::radius_effect`: `fxChanges`
    /// over the level's `radiusEffectName`).
    pub radius_fx: Option<String>,
}

/// Leap classes: `Skill_AttackRadiusLeap` and `Skill_AttackRadiusDisengage`
/// (derived from it).
pub fn is_leap(class: &str) -> bool {
    matches!(
        class,
        "Skill_AttackRadiusLeap" | "Skill_AttackRadiusDisengage"
    )
}

/// A leap in progress (`Skill_AttackRadiusLeap` / `…Disengage`), per
/// Game.dll / Engine.dll: the skill's action is a `JumpAttackAction`
/// whose `Execute` calls `Character::JumpToUseSkill`.
/// That stores the start and the target (both put on the
/// floor) and sets the character's movement modifier to
/// `d / |z|`: `d` the horizontal start→target distance, `z` the jump
/// animation's total root-motion-bone translation (last key − first,
/// `GraphicsAnim::GetBoneCoords`), through the bone's bind matrix, forward
/// component (the frame-timing factor it also computes cancels out). The
/// animation's `StartJump` callback starts a `CharacterMovementManager::
/// MoveTo` to the target, `StopJump` stops it (`JumpAttackAction::
/// AnimationCallback`). While moving, the crowd agent's speed is
/// the actor's motion — the root-motion velocity of the frame
/// (`AnimChannel::Update` → `Actor::UpdateSelf`) — times that modifier
/// (`Character::CrowdAgentUpdate`), so the jump covers `d`
/// over the animation's root-motion curve, at its pace. The hit is the
/// animation's hit callback (`fire_casts`).
#[derive(Component)]
pub struct Leap {
    /// Nav waypoints left to the target.
    pub to: Vec<Vec3>,
    /// Horizontal start→target distance (`JumpToUseSkill`).
    pub dist: f32,
    /// The movement modifier, set at `StartJump`; `None` before it.
    pub scale: Option<f32>,
}

impl Leap {
    /// A jump along `to` covering `dist` (also used by the evade, which is
    /// the same `JumpToUseSkill` movement, `evade.rs`).
    pub fn new(to: Vec<Vec3>, dist: f32) -> Self {
        Self {
            to,
            dist,
            scale: None,
        }
    }
}

/// A pak's effects with their attach points (the record itself when it is
/// an effect, not a pak).
pub fn pak_effects(data: &GameData, pak: &str) -> Vec<(String, Option<String>)> {
    let Some(rec) = data.db.get(pak) else {
        return Vec::new();
    };
    if rec.get("particleEffectNames").is_none() {
        return vec![(pak.to_owned(), None)];
    }
    let points = rec.strs("particleEffectAttachPoints");
    let mut out: Vec<(String, Option<String>)> = rec
        .strs_iter("particleEffectNames")
        .enumerate()
        .map(|(i, n)| {
            (
                n.to_string(),
                points
                    .get(i)
                    .map(|p| p.to_string())
                    .filter(|p| !p.is_empty()),
            )
        })
        .collect();
    // `CharFxPak::LoadMesh`: its `meshEffectNames` (`FxMesh` records, `"mesh:"` marks them here) ride the character
    // too, at the record's `attachPoint`, else its origin (`PFxManager::AddMeshEffect`:
    // `Attach(effect, identity, attachPoint)`).
    out.extend(rec.strs_iter("meshEffectNames").map(|m| {
        let point = data
            .db
            .get(m)
            .and_then(|r| r.str("attachPoint").map(str::to_owned))
            .unwrap_or_default();
        (format!("{MESH_PREFIX}{m}"), Some(point))
    }));
    out
}

/// Marks an `FxMesh` record among a pak's effect names ([`pak_effects`]).
pub const MESH_PREFIX: &str = "mesh:";

/// The `CharFxPak` records' `startSound`, `activeSound` and `stopSound`.
pub fn char_pak_sounds(data: &GameData, paks: &[String]) -> Vec<[Option<String>; 3]> {
    let field = |r: &Record, k: &str| r.str(k).filter(|s| !s.is_empty()).map(str::to_owned);
    paks.iter()
        .filter_map(|p| data.db.get(p))
        .map(|r| {
            [
                field(r, "startSound"),
                field(r, "activeSound"),
                field(r, "stopSound"),
            ]
        })
        .collect()
}

/// `CharFxPak::Start` and what `CharFxPak::Stop` does when the
/// pak's first effect goes (`crate::combat_sounds` module doc): the pak's
/// sounds for a character effect started on `owner`. The loop and the stop
/// sound hang from that effect entity.
pub fn start_char_pak_sounds(
    w: &mut World,
    sounds: &[[Option<String>; 3]],
    owner: Entity,
    fx: &[Entity],
) {
    let at = w
        .get::<Transform>(owner)
        .map_or(Vec3::ZERO, |t| t.translation);
    for [start, active, stop] in sounds {
        if let Some(pak) = start.clone() {
            w.write_message(crate::visuals::PlaySound { pak, at: Some(at) });
        }
        let Some(first) = fx.first().copied() else {
            continue;
        };
        if let Some(pak) = active.clone() {
            w.spawn((
                crate::visuals::SoundLoop {
                    pak,
                    on: Some(owner),
                    at,
                    fade: 0.01,
                },
                ChildOf(first),
            ));
        }
        if let Some(pak) = stop.clone() {
            w.entity_mut(first)
                .insert(crate::visuals::StopSound { pak, on: owner });
        }
    }
}

/// `Skill_BuffSelfImmobilize` (Blade Barrier) roots its caster by refreshing a short hold while it runs.
/// Game.dll `Skill_BuffSelfImmobilize::Update`: after `Skill_BuffSelfDuration::Update`,
/// while the skill is active (`Skill.active`) the frame's ms come off its timer (`Skill_BuffSelfImmobilize.holdTimer`:
/// 0 from the constructor, never reset by an activation); at ≤ 0 the timer is re-armed with
/// `Skill_BuffSelfImmobilize.holdPeriod` (100 ms) and the holder gets `DurationDamageManager::AddFixedDamage(47 =
/// immobilize, 1.0, min(holdCap = 0.25 s, time left × 0.001))` (`Skill_BuffSelfImmobilize.holdCap`; the time left
/// in ms is `Skill_BuffSelfDuration.timeLeft`; a fixed entry: the duration is replaced,
/// `crate::afflictions::Afflictions::hold`), then
/// `EndAttack`. So the hold lasts while the buff runs and ends within 0.25 s of its stop, whatever stops it
/// (expiry, a dispel, death). Inserted at the first cast (`gd_game::casting::fire_casts`); the running buff is the caster's buff
/// list entry (`gd_game::player::Loadout::buffs`, `MonsterSkills::buffs`; no stock pet record has the class).
#[derive(Component, Debug)]
pub struct ImmobilizeRefresh {
    /// The skill under its buff list name.
    pub skill: String,
    /// `Skill_BuffSelfImmobilize.holdTimer`, seconds.
    pub timer: f32,
}

/// [`ImmobilizeRefresh`]'s re-arm (`Skill_BuffSelfImmobilize.holdPeriod` = 100 ms, the constructor).
pub const IMMOBILIZE_PERIOD_SECS: f32 = 0.1;

/// [`ImmobilizeRefresh`]'s hold cap (`Skill_BuffSelfImmobilize.holdCap` = 0.25 s, the constructor).
pub const IMMOBILIZE_HOLD_SECS: f32 = 0.25;

/// Damaging auras (`Skill_BuffAttackRadiusToggled`: blade swarm, blight
/// fumes, undeath aura …): every 1000 ms (`Update`, interval
/// set to 1000 by the constructor) the skill hits every foe within
/// `skillTargetRadius` of its owner. Pets and monsters that have the skill
/// keep it on: their AI casts it as a buffSelf skill
/// (`ControllerMonster::ChooseBestBuffSelfSkill`, skipped once
/// active) and a toggle never runs out; the port starts it on,
/// without that first cast.
pub struct DamageAura {
    pub skill: String,
    pub level_index: usize,
    pub radius: f32,
    pub hostile: bool,
    pub timer: f32,
    /// Seconds left for a timed aura (`Skill_BuffAttackRadiusDuration`).
    pub left: Option<f32>,
    /// Seconds between ticks (1, or `skillTargetInterval` for the lightning
    /// storm `Skill_BuffAttackRadiusLightning`).
    pub interval: f32,
    /// The storm hits only `skillTargetNumber` foes per tick (random ones:
    /// the radius list is shuffled and cut,
    /// `Skill_BuffAttackRadiusLightning::Update`), each with a
    /// `lightningName` bolt from the owner.
    pub max_targets: Option<usize>,
    pub bolt: Option<String>,
    /// The storm's `cameraShakeDurationSecs` / `cameraShakeAmplitude`, sent per bolt
    /// (`SkillActivated::CameraShake`, `Skill_BuffAttackRadiusLightning::Update`).
    pub shake: Option<(f32, f32)>,
}

impl DamageAura {
    /// The aura's skill record (read by the HUD, `buff_bar::pet_icons`).
    pub fn skill(&self) -> &str {
        &self.skill
    }

    pub fn new(rec: &Record, skill: &str, level_index: usize, hostile: bool) -> Self {
        let storm = rec.class == "Skill_BuffAttackRadiusLightning";
        let interval = if storm {
            level_f(rec, "skillTargetInterval", level_index)
        } else {
            1.0
        };
        Self {
            skill: skill.to_owned(),
            level_index,
            radius: level_f(rec, "skillTargetRadius", level_index),
            hostile,
            timer: interval,
            left: None,
            interval,
            max_targets: storm
                .then(|| level_f(rec, "skillTargetNumber", level_index).max(1.0) as usize),
            shake: storm
                .then(|| {
                    (
                        level_f(rec, "cameraShakeDurationSecs", level_index),
                        level_f(rec, "cameraShakeAmplitude", level_index),
                    )
                })
                .filter(|s| s.0 > 0.0001),
            bolt: if storm {
                rec.str("lightningName")
                    .filter(|l| !l.is_empty())
                    .map(str::to_owned)
            } else {
                None
            },
        }
    }
}

#[derive(Component)]
pub struct DamageAuras(pub Vec<DamageAura>);

impl DamageAuras {
    /// Dispelled (`crate::skill_buffs::DispelOn`): the damage auras stop
    /// (`Skill_BuffAttackRadiusToggled::StopSkill`, also the timed and
    /// lightning ones' class) but those `keep` names (`notDispelable`). The
    /// stopped ones.
    pub fn dispel(&mut self, keep: impl Fn(&str) -> bool) -> Vec<String> {
        let gone: Vec<String> = self
            .0
            .iter()
            .filter(|a| !keep(&a.skill))
            .map(|a| a.skill.clone())
            .collect();
        self.0.retain(|a| keep(&a.skill));
        gone
    }
}

/// Life from a bonus (`CharacterBio::TakeBonus` / `GetBonusLifeAmount`):
/// flat × (1 + `characterHealIncreasePercent` %) plus % of max
/// life × (1 + that + `characterPercentHealIncreaseModifier` %); energy is
/// not scaled. The over-time % uses only the first.
pub fn bonus_life(sheet: &gd_sim::stats::CharacterSheet, flat: f32, pct: f32) -> f32 {
    let h = sheet.heal_increase * 0.01;
    flat * (1.0 + h)
        + (sheet.life * pct * 0.01 * (1.0 + h + sheet.percent_heal_increase * 0.01)).max(0.0)
}

/// `Skill_BuffSelfToggled` and the classes deriving from it (RTTI: `…Colossus`, `…Duration`, `…Immobilize`,
/// `…Invulnerable`, `…Shield`, `…Sick`, `Skill_Shapeshift`), whose `Update` gives the self buff's
/// `gd_game::casting::Bonus::buff_duration` every second while it is on.
pub fn is_self_buff_class(class: &str) -> bool {
    matches!(
        class,
        "Skill_BuffSelfToggled"
            | "Skill_BuffSelfColossus"
            | "Skill_BuffSelfDuration"
            | "Skill_BuffSelfImmobilize"
            | "Skill_BuffSelfInvulnerable"
            | "Skill_BuffSelfShield"
            | "Skill_BuffSelfSick"
            | "Skill_Shapeshift"
    )
}

/// Seconds to each running self buff's next `gd_game::casting::Bonus::buff_duration` (`Skill_BuffSelfToggled.bonusTimer`).
#[derive(Component, Default, Debug)]
pub struct SelfBuffHeals(pub Vec<(String, f32)>);

/// The player's switched-on self-buff toggles from the book and worn items
/// (`gd_game::casting::self_buff_heals`), cached: reading them clones the skill book and scans
/// the items, so it runs only when the `gd_game::player::Loadout` changed. The timed buffs are
/// read fresh each frame (their remaining time ticks down).
#[derive(Component, Default)]
pub struct SelfBuffToggles(pub Vec<(String, usize)>);

/// `<type>DamageQualifier` flags of a skill record → damage types
/// (`SkillProfile::LoadProfile`: elemental = fire, cold,
/// lightning).
pub fn qualifying_types(rec: &Record) -> Vec<gd_sim::attributes::CombatType> {
    use gd_sim::attributes::CombatType as T;
    let mut v = Vec::new();
    for (f, tys) in [
        ("physicalDamageQualifier", &[T::Physical][..]),
        ("pierceDamageQualifier", &[T::Pierce]),
        ("lightningDamageQualifier", &[T::Lightning]),
        ("fireDamageQualifier", &[T::Fire]),
        ("coldDamageQualifier", &[T::Cold]),
        ("poisonDamageQualifier", &[T::Acid]),
        ("lifeDamageQualifier", &[T::Vitality]),
        ("bleedingDamageQualifier", &[T::Bleeding]),
        (
            "elementalDamageQualifier",
            &[T::Fire, T::Cold, T::Lightning],
        ),
        ("aetherDamageQualifier", &[T::Aether]),
        ("chaosDamageQualifier", &[T::Chaos]),
    ] {
        if rec.bool(f) {
            v.extend(
                tys.iter()
                    .filter(|t| !v.contains(*t))
                    .copied()
                    .collect::<Vec<_>>(),
            );
        }
    }
    v
}

/// The charge potions (`Skill_ChargePotion`): keys R (health) and E
/// (energy) — the default keybindings bind scan codes 19/18 to
/// actions 30/29 (`KeyBindings::GetActionByName` maps action 29 to
/// DrinkManaPotion and 30 DrinkHealthPotion).
///
/// `Skill_ChargePotion::TargetResult`: the potion's
/// own fields go to `Character::TakeBonus` as a slow bonus, so
/// (`CharacterBio::TakeBonus`) `skillLifePercent` % of max life (× the
/// healing bonuses) and `skillManaBonus` + `skillManaPercent` % of max energy
/// land at once, while `skillLifeBonus` and `skillLifePercentSlow` % (× the
/// healing increase) and `skillManaPercentSlow` % fill a restore pool. Each
/// learned `Skill_PotionModifier` adds its own fields the same way, scaled by
/// the potency 1 + (`modifierPotency` + `potionTransfusion`) % summed over the
/// potion and its containers / modifiers (`SkillProfile_Modifiers`,
/// `GetSkillModifierData`). `potionInfusion` % moves the potion's instant
/// parts into the pool, raised by that %; `potionTransfusion` drops the
/// potion's own restore. `absorbShieldPercentHealth` % × potency of max life
/// is a damage-absorbing shield (a combat filter for the potion's qualifying
/// damage types, none listed = every type: `Skill_ChargePotion::ModifyDamage`)
/// that lasts until used up (`Update` drops the filter once
/// spent), a new potion refilling it. The pools drain at max life /
/// `healthFeedRate` s and (max − reserved) energy / `manaFeedRate` s, held to
/// `…OverStorageLimit` × that max (`playerresourcebehavior.dbr`, read by
/// `CharacterBio::Load`, drained in `CharacterBio::Update`).
/// `cooldownCharges` uses are the shared [`Cooldowns`] charge rule
/// (`Skill::StartCooldown`, `SetChargeCooldown`, `UpdateCooldownCharges`):
/// a drink spends a charge, the cooldown starts only while none
/// runs, each finished one returns a charge and starts the next. Secondary potion modifiers
/// (explosive, electrified, buffs) fire on use (`secondary.rs`). Hook
/// `GD_POTION` (hooks.rs).
pub const HEALTH_POTION: &str = "records/skills/default/defaulthealthpotion.dbr";
pub const ENERGY_POTION: &str = "records/skills/default/defaultmanapotion.dbr";

/// The potion restore pools (life, energy) still to drain into the
/// character (`CharacterBio` slow life / energy).
#[derive(Component, Default)]
pub struct PotionOverTime {
    pub life: f32,
    pub energy: f32,
}

/// `playerresourcebehavior.dbr`: (`healthFeedRate`, `healthOverStorageLimit`,
/// `manaFeedRate`, `manaOverStorageLimit`).
pub fn resource_behavior(data: &GameData) -> (f32, f32, f32, f32) {
    let r = data.db.get("records/game/playerresourcebehavior.dbr");
    let f = |k: &str, d: f32| r.as_ref().and_then(|r| r.f32(k)).unwrap_or(d);
    (
        f("healthFeedRate", 10.0).max(0.01),
        f("healthOverStorageLimit", 1.0),
        f("manaFeedRate", 10.0).max(0.01),
        f("manaOverStorageLimit", 1.0),
    )
}

/// Drink a potion (the game sends it for the R / E keys).
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsePotion {
    Health,
    Energy,
}

/// `Skill::GetMoveTypeStateString` is "MoveToUseSkill" for these: the
/// player walks until within the skill's usable distance before casting.
/// `ControllerPlayerState::DefaultRequestSkillAction` (mouse
/// movement): a skill walks when its range profile is Melee, it
/// has `forceMovement`, or the `classicCasting` option is on; any other
/// ranged-profile skill casts at once and clamps in `try_cast`. Self-cast
/// kinds have no distance; weapon attacks are the combat system's.
/// `classic` is the `classicCasting` option (default off: `Options::Options`); `gate` is
/// the engine's extra clause of `DefaultRequestSkillAction`: with `movementType` 1 and no
/// target object, or `movementType` 2, only weapon charges walk into range (see [`Walk`]).
pub fn walks_into_range(rec: &Record, kind: Kind, walk: Walk) -> bool {
    if !walk.gate {
        return gd_sim::skills::is_weapon_charge(&rec.class);
    }
    (matches!(
        kind,
        Kind::TargetRadius
            | Kind::Projectile
            | Kind::Drop
            | Kind::Wave
            | Kind::LineFan
            | Kind::Line
            | Kind::Storm
            | Kind::Area
            | Kind::Chain
            | Kind::Dispel
    ) || charges_to_range(rec))
        && (matches!(rec.str("distanceProfile").unwrap_or("Melee"), "Melee")
            || rec.bool("forceMovement")
            || walk.classic)
}

/// The walking rule's inputs: the `classicCasting` option and the keyboard-movement gate
/// (`(InputMode != 0 || movementType != 1 || targetId != 0) && movementType != 2`; the port has no gamepad input mode).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Walk {
    pub classic: bool,
    pub gate: bool,
}

impl Walk {
    pub fn new(classic: bool, movement_type: i32, has_target: bool) -> Self {
        Walk {
            classic,
            gate: (movement_type != 1 || has_target) && movement_type != 2,
        }
    }
}

/// Move type 2 (`Skill_AttackPathCharge`): `Skill::GetMoveTypeStateString`
/// is "ChargeToUseSkill", so the out-of-range walk is a
/// charge (`gd_game::movement::MoveToUseSkill::charging`). Weapon charges (Blitz, Shadow
/// Strike: `gd_sim::skills::is_weapon_charge`) have move type 1, but
/// `DefaultRequestSkillAction` renames their "MoveToUseSkill"
/// to "ChargeToUseSkill", and being Melee-profile they walk into
/// range like any melee skill; see [`warms_up`].
pub fn charges_to_range(rec: &Record) -> bool {
    rec.class == "Skill_AttackPathCharge" || gd_sim::skills::is_weapon_charge(&rec.class)
}

/// How `gd_game::casting::try_cast` is reached.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Use {
    /// A fresh request (hotbar key, held left click).
    Request,
    /// The end of a `MoveToUseSkill` / ChargeToUseSkill move (no new walk).
    Arrived,
    /// The end of a charge whose warm-up already paid ([`warms_up`]).
    ArrivedWarm,
}

/// The weapon skills' warm-up (`Skill_AttackWeapon::WarmUpStart`,
/// run by `MoveToAction::Execute` as a ChargeToUseSkill move
/// starts): when the skill is enabled and its profile allows a
/// warm-up (`skillAllowsWarmUp`, every player weapon charge), it plays the
/// warm-up FX (`warmUpEffectName`), starts the cooldown, pays
/// `GetManaCost` and flags the warm-up as active (`Skill.warmUpActive` / `Skill.warmUpPaid`);
/// `Skill_AttackWeaponCharge::WarmUpStart` first sets the
/// push-through flag and stores the start (`MaxSkillDistance`), Blink's
/// also hides the caster. The move then runs at the warm-up
/// run speed (`gd_sim::skills::warmup_run_multiplier`). The move's end
/// (`MoveToAction::Finish`) calls `WarmUpEnd`: the FX stops,
/// and unless the target is a living character (or a breakable) the skill
/// is cancelled (`Cancel`). Reaching the target starts
/// UseSkill (`EndOfPathReached`) and the attack itself pays
/// nothing more: `Skill_AttackWeaponCharge::ActivateNow`
/// bumps the hit iteration when the warm-up was active, and only the first
/// iteration pays and starts the cooldown (`SkillManager::
/// SkillTargetResult`). It also runs the learned
/// `Skill_MeleeModifier`s only then (no stock record has one). A charge cut short (target dead:
/// `OnUpdate` → Idle; stun; a move order) keeps what it paid.
/// The warm-up's presentation is [`WarmUp`]. PORT: the push-through (the
/// port's player does not collide with characters) is not done.
pub fn warms_up(rec: &Record) -> bool {
    gd_sim::skills::is_weapon_charge(&rec.class) && rec.bool("skillAllowsWarmUp")
}

/// A weapon skill's warm-up running on its caster: from `WarmUpStart` to
/// `WarmUpEnd` (the ChargeToUseSkill move, [`warms_up`]). Game.dll:
/// - `Skill_AttackWeapon::WarmUpStart` plays `skillWarmUpSound`
///   at the caster (`Skill.warmUpSound`, `Skill::PlayWarmUpSound`), then
///   `Skill::CreateWarmUpFx` starts the level's
///   `warmupFxPakName` entry (level − 1 clamped to the list)
///   at the caster's coordinates, in the world (it stays where it started),
///   then `Skill::WarmUpEffect(caster, true)`: the first time,
///   `Character::AddParticleEffect(warmUpEffectName, warmUpEffectAttachPoint)`
///   (the handle is kept in `Skill.warmUpEffectHandle`), then
///   `StartParticleEffect`. It goes with the caster, at the attach point.
/// - `Skill_AttackWeapon::WarmUpEnd` (warm-up active only):
///   `WarmUpEffect(caster, false)` → `StopParticleEffect` →
///   `EffectEntity::StopEmitting` (`PFxManager::StopParticleEffect`): the
///   effect's live particles finish; the world pak plays on.
/// - `Skill_AttackWeaponBlink::WarmUpStart`: when the warm-up
///   became active, `SetVisibility(false)` on the caster
///   ([`crate::visuals::Hidden`]); its `WarmUpEnd` override
///   shows it again (`SetVisibility(true)`, unconditionally).
/// - Modifiers (`gd_game::casting::warm_up_modifiers`): `CreateWarmUpFx` then calls each
///   `Skill_Modifier` of the skill's learned list (`Skill.modifiers`, `AddModifier`) and
///   then of its gear list (`Skill.itemSkillModifiers`, `AddToItemSkillModifiers`),
///   the same `Skill::CreateWarmUpFx` on the modifier: its own
///   `warmupFxPakName` at its level starts too (no enabled check, so every
///   one adds to the skill's, an off gear modifier's too:
///   `gd_game::weapon_pool::SkillLinks::off_mods`). `WarmUpEffect` first asks the gear list, then
///   the learned one: the first modifier that `IsSkillEnabled`
///   and whose own `WarmUpEffect` returns true, i.e. has a
///   `warmUpEffectName` (with its own `warmUpEffectAttachPoint`), wears its
///   effect instead of the skill's; none: the skill's own. The end goes the
///   same way, so the modifier's effect is the one stopped. The sound is the
///   skill's only (`WarmUpStart` plays only its own `Skill.warmUpSound`). Stock: the Vire's
///   Might gear modifiers (`viremight_warmuploop_<type>fx01`, no attach
///   point).
/// - `Skill_AttackPathCharge::WarmUpStart` (Vire's Might) sets
///   its run up (push-through flag, start coordinates, `Skill_AttackPathCharge.pathActive`)
///   and ends in `Skill_AttackWeapon::WarmUpStart`, its `WarmUpEnd`
///   in `WarmUpEffect(caster, false)` while the warm-up was
///   active (`Skill.warmUpActive`).
///
/// The move that carries the warm-up: the player's ChargeToUseSkill
/// (`gd_game::movement::MoveToUseSkill::warm_up`), a monster's or pet's Pursue to its chosen
/// weapon charge (`gd_game::casting::begin_pursuit_charge`, `MoveToAction` with the skill, as
/// the engine), or a [`Charge`] / [`PathCharge`] run after the cast. PORT: the
/// run after the cast stands for the move before it (a use within the
/// skill's range out of weapon reach, an AI hit whose target stepped out of
/// reach, every path charge); a fresh effect entity per warm-up (the engine
/// restarts the kept one); an effect without an attach point sits where the
/// port's other unattached character effects do.
#[derive(Component)]
pub struct WarmUp {
    pub skill: String,
    /// The attached `warmUpEffectName` effect(s).
    pub fx: Vec<Entity>,
}

/// `Skill_AttackWeaponCharge` / `Skill_AttackWeaponBlink` warmed up: the
/// caster pushes through characters (`WarmUpStart` sets
/// `Character::SetPathingPushThru`'s flag) and runs at its warmed-up run
/// speed (`crate::combat::charge_move`). Two forms:
/// - a monster's Pursue to the weapon charge it chose (`gd_game::casting::ChargePursuit`):
///   its AI moves it, on the Run clip at the warmed-up rate, and its Attack
///   uses the skill at the end of the path (the engine's warm-up move, see
///   `gd_game::casting::begin_pursuit_charge`);
/// - a dash after the cast (the port's: a use within the skill's range out of
///   weapon reach, a monster's or pet's hit whose target stepped out of
///   reach): the caster paths to the target, the player on the Charge clip
///   (ChargeToUseSkill), an AI on the Run clip, at the warmed-up rate, stops
///   at reach and hits it on arrival.
///
/// The dash gives up once it has run
/// farther from its start than the target is plus `maxDistanceBuffer`
/// (`Skill_AttackWeaponCharge::MaxSkillDistance`, asked by the
/// controller's `MaxSkillDistance`; no check when the buffer is 0); no time
/// limit. A blink hides the caster meanwhile (`Skill_AttackWeaponBlink::
/// WarmUpStart`). It ends without its hit when the target dies (`ControllerPlayerStateChargeToUseSkill::
/// OnUpdate` → Idle, and `WarmUpEnd` cancels the skill unless the target is a living character or a
/// breakable: [`warms_up`]) and when the caster dies or is stunned (`gd_game::casting::stop_skills`). The skill's
/// `skillTargetNumber` / `skillTargetAngle` sweep along the way is not done.
#[derive(Component)]
pub struct Charge {
    pub target: Entity,
    /// `characterRunSpeedModifier` % of the skill.
    pub bonus: f32,
    pub skill: String,
    pub level_index: usize,
    pub start: Vec3,
    pub buffer: f32,
    pub path: Vec<Vec3>,
    pub repath: f32,
}

impl Charge {
    /// The warmed-up charge of `skill` (level index `li`, its `characterRunSpeedModifier` % `bonus`) from `at` at
    /// `target`.
    pub fn new(rec: &Record, skill: &str, li: usize, bonus: f32, at: Vec3, target: Entity) -> Self {
        Charge {
            target,
            bonus,
            skill: skill.to_owned(),
            level_index: li,
            start: at,
            buffer: rec.f32("maxDistanceBuffer").unwrap_or(0.0),
            path: Vec::new(),
            repath: 0.0,
        }
    }
}

/// Where each charge skill of a character last started (`Skill_AttackWeaponCharge`,
/// `...Blink`, `Skill_AttackPathCharge`): `WarmUpStart` stores the caster's
/// coordinates at `Skill_AttackWeaponCharge.chargeStart`, which `MaxSkillDistance` measures from.
/// Per skill (record name) as each skill object has its own copy.
/// PORT: a dash's when it starts to move, not at the warm-up start (the
/// caster stands still in between); a monster's Pursue warm-up's at its start.
#[derive(Component, Default)]
pub struct ChargeStarts(pub HashMap<String, Vec3>);

/// A Devastation-like rain of drops ([`Kind::Storm`]): every `skillTargetInterval`
/// around the cast point (`targetingMode` `Point`) or else around the
/// caster where it stands then (`Skill_BuffAttackRadiusDrop::Update`;
/// `gd_sim::skills::TargetingMode`): trap and
/// obelisk firestorms, Valaxteria's. Each tick drops `projectileLaunchNumber`
/// (0 → 1) projectiles: the landing radius is `1 + rand·(skillTargetRadius − 1)`
/// (an annulus, no drops within 1 m), the launch point is the landing spot at
/// `dropHeight ± dropVariation` plus a `dropRadius` disc offset (`Skill_BuffAttackRadiusDrop::DropLaunchPoint`),
/// and each flies back to its landing spot at its own
/// `projectileVelocity` (`ProjectileBase::MoveStraightLine`).
#[derive(Component)]
pub struct Storm {
    pub cast: Casting,
    pub around_caster: bool,
    pub projectile: String,
    pub left: f32,
    pub interval: f32,
    pub timer: f32,
    pub per_tick: usize,
    pub radius: f32,
    pub height: f32,
    /// `dropVariation`: the launch height is `height` ± this (`Skill_BuffAttackRadiusDrop::DropLaunchPoint`).
    pub drop_variation: f32,
    /// `dropRadius`: an extra uniform-disc sideways offset of the launch point.
    pub drop_radius: f32,
    pub explosion: f32,
    pub attacker: Entity,
}

/// The landing offset of one storm drop (`Skill_BuffAttackRadiusDrop::Update`):
/// the radius is `1 + u·(radius − 1)` — an annulus, no drop within
/// 1 m of the centre, densest near the inner edge — at the angle `a` (radians).
pub fn storm_landing(radius: f32, u: f32, a: f32) -> Vec3 {
    let r = 1.0 + u * (radius - 1.0).max(0.0);
    Vec3::new(a.sin() * r, 0.0, a.cos() * r)
}

/// A radius whose `expansionTime` (`SkillProfile.expansionTime`, s; `SkillProfile::GetRadiusTime`
/// gives it x 1000) is above 0: the radius grows from its centre
/// to its full size over that time and each foe is hit as it reaches it. Two
/// classes do this:
/// - `Skill_AttackRadius`: `ActivateNow` records {elapsed 0,
///   time = `expansionTime` x 1000 whole ms, radius, the caster's coords, no
///   one hit} (the Primary secondaries run at once on the full radius,
///   `fire.rs`). Each attack's `TargetResult` then runs the special pair; the
///   `Skill_AttackRadius.warmingUp` check in `ActivateNow` is the warm-up flag guarding
///   `SendSkillActiveUpdate` (no attack), so a radius that hits nothing runs
///   no special pair.
/// - `SkillSecondary_AttackRadius`: `ActivateNow` records the
///   same at the activation point (the first target's coords when it is
///   valid, `secondary.rs`); its `TargetResult` hits and runs the
///   ordinary pair only (`ActivateSecondarySkills`, no special pair).
///
/// `Update` (`Skill_AttackRadius::Update` / `SkillSecondary_AttackRadius::Update`, the same code), per frame: an entry
/// already past its time goes; else the frame's time is added and the radius
/// is `full x elapsed / time` (full once past it); the foes in it
/// (`GetTargetsInRadius`) not hit yet form one attack (the hits, the ordinary
/// secondaries on them), none: nothing. So the far edge is hit
/// `expansionTime` after the cast.
///
/// No other class grows its hits (every `GetRadiusTime` caller and
/// inline read of `SkillProfile.expansionTime`): `Skill_AttackRadiusLeap` / `Teleport` /
/// `Skill_AttackBuffRadius` / `Skill_AttackPathCharge` / `Skill_AttackRadius`
/// pass it only to `CreateVisualEffect` (the ring fx's time);
/// `SkillSecondary_OnKillAttackRadius::OnEnemyDeath` and
/// `Skill_OnHitAttackRadius::OnHitActivation` store it but take
/// the full-radius list at once, and their `Update`s attack every valid entry
/// on the next frame (the time only paces the debug sphere / list clear);
/// the lightning radii (`Skill_AttackRadiusLightning` /
/// `SkillSecondary_AttackRadiusLightning` / `Skill_AttackRadiusLightning2`)
/// store it beside their bolt, but hit when the bolt finishes or its 5000 ms
/// cap runs out. The debuff radii never read it.
#[derive(Component)]
pub struct ExpandingRadius {
    pub cast: Casting,
    pub caster: Entity,
    pub centre: Vec3,
    pub radius: f32,
    pub time: f32,
    pub elapsed: f32,
    pub hit: Vec<Entity>,
    /// Each attack runs the special pair (`Skill_AttackRadius`).
    pub special: bool,
}

/// The height of the cone's damage frustum (`Skill_AttackSpellCone.frustumHeight`, set to 5.0 by the constructor
/// and read by nothing else).
pub const CONE_HEIGHT: f32 = 5.0;

/// A cone entry (`Skill_AttackSpellCone`'s cone list, its own entity): created by the activation, it starts its
/// effect after the delay, grows, strikes and retracts (module doc). The one source of the cone's length.
#[derive(Component, Debug)]
pub struct ConeFx {
    pub owner: Entity,
    pub skill: String,
    pub level_index: usize,
    /// The cone effect record (`coneName` / `fxChanges`, resolved by the activation).
    pub record: String,
    /// The effect entity once emitting (state 2).
    pub effect: Option<Entity>,
    /// Seconds before it starts emitting (state 1).
    pub delay: f32,
    /// Seconds to the next strike.
    pub tick: f32,
    /// The grown length and the retract (m), `travelSpeed`.
    pub len: f32,
    pub retract: Option<f32>,
    pub speed: f32,
    /// The last placement (frozen once retracting): the emit point, the aim point (`Skill_AttackSpellCone.aimPoint`),
    /// the effect's rotation,
    /// the caster's rotation and `GetMaxConeLength`.
    pub emit: Vec3,
    pub aim: Vec3,
    pub rotation: Quat,
    pub caster_rot: Quat,
    pub max: f32,
}

impl ConeFx {
    /// One retracting update of `dt` s (state 4: `dt` ms × 0.002 × `travelSpeed`): true when the retract reached
    /// the length (state 5).
    pub fn retract(&mut self, dt: f32) -> bool {
        let r = self.retract.get_or_insert(0.0);
        *r += dt * 2.0 * self.speed;
        if *r >= self.len {
            *r = self.len;
            return true;
        }
        false
    }

    /// The damage frustum (`GetTargets`): from `emit + dir * retract` to `emit + dir * len` along `dir`
    /// = the normalised aim - emit, `startWidth` wide at the near face and the width the length reached at the far
    /// one (`startWidth + len / maxRange * (endWidth - startWidth)`), [`CONE_HEIGHT`] high, faced along the
    /// caster's axes. `foe_box` meets it.
    pub fn strikes(&self, rec: &Record, foe_box: &gd_sim::bounds::Aabb) -> bool {
        let li = self.level_index;
        let (w0, w1, range) = (
            level_f(rec, "startWidth", li),
            level_f(rec, "endWidth", li),
            level_f(rec, "maxRange", li),
        );
        let far_w = if range > 0.0 {
            w0 + self.len / range * (w1 - w0)
        } else {
            w0
        };
        let dir = (self.aim - self.emit).normalize_or_zero();
        cone_frustum_meets_box(
            self.emit + dir * self.retract.unwrap_or(0.0),
            self.emit + dir * self.len,
            self.caster_rot * Vec3::Z,
            self.caster_rot * Vec3::X,
            self.caster_rot * Vec3::Y,
            w0,
            far_w,
            CONE_HEIGHT,
            foe_box,
        )
    }

    /// One running update (state 2): the length grows `travelSpeed` m/s up to `max` (`GetMaxConeLength`).
    pub fn grow(&mut self, dt: f32, max: f32) {
        if self.len < max {
            self.len += dt * self.speed;
        }
        if max <= self.len {
            self.len = max;
        }
    }
}

/// The cone's slope: the y of the normalised `aim - emit` (`Update` / `GetMaxConeLength`).
pub fn cone_slope(emit: Vec3, aim: Vec3) -> f32 {
    (aim - emit).normalize_or_zero().y
}

/// The cone's rotation: the caster's with its forward turned up to `slope` (module doc, PORT).
pub fn cone_rotation(caster: Quat, slope: f32) -> Quat {
    caster * Quat::from_rotation_x(-slope.atan())
}

/// The running channel's base visual on its caster (module doc).
#[derive(Component, Debug)]
pub struct ChannelFx {
    pub skill: String,
    /// The effect entity, once created (a cone: its [`ConeFx`] entry).
    pub fx: Option<Entity>,
    /// Grow: the step shown.
    pub step: Option<usize>,
    /// Cone: the caster's emit point.
    pub point: String,
}

/// The charge effect each charged-scaling skill shows on its caster: (skill, charges, effect).
#[derive(Component, Default, Debug)]
pub struct ChargeFx(pub Vec<(String, u32, Entity)>);

pub fn spawn_on(
    commands: &mut Commands,
    owner: Entity,
    skill: &str,
    record: &str,
    at: Vec3,
) -> Entity {
    crate::visuals::notify(commands, record);
    let f = commands
        .spawn((
            EffectSpec {
                despawn_when_complete: true,
                ..EffectSpec::record(record)
            },
            Transform::from_translation(at),
            BuffFx {
                owner,
                skill: skill.to_owned(),
                toggled: false,
                point: None,
            },
        ))
        .id();
    commands.entity(owner).add_child(f);
    f
}

/// The [`RangedWeapon`] of a weapon record, if it is a ranged one.
pub fn ranged_weapon(data: &GameData, weapon: &str) -> Option<RangedWeapon> {
    let r = data.db.get(weapon).filter(|r| r.class.contains("Ranged"))?;
    let p = r.str("basicProjectileName")?.to_owned();
    let long = data.engine().long_range();
    Some(RangedWeapon {
        projectile: p,
        reach: long + 0.5,
    })
}
