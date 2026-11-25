//! The casting data (the cast systems are gd-game's `casting`).
use bevy::prelude::*;

/// Bind-pose position of a named mesh attach point in the character's space.
pub fn attach_point_bind(body: &gd_formats::msh::Mesh, name: &str) -> Option<bevy::math::Vec3> {
    let p = body.attach_point(name)?;
    let parent = p
        .parent_bone
        .map_or(gd_formats::msh::Coords::IDENTITY, |i| body.bones[i].abs);
    Some(bevy::math::Vec3::from(parent.mul(&p.coords).origin))
}

/// A character's damage-absorbing combat filters, in the order they were added (`CombatManager::AddCombatFilter`);
/// `CombatManager::ApplyDamage` runs each in turn on each damage application. Each skill is its own filter, so a
/// recast keeps its place.
#[derive(Component, Default, Debug, Clone)]
pub struct Shields(pub Vec<Shield>);

/// One filter of [`Shields`]: its skill (or buff) record, absorb and seconds left.
///
/// - [`ShieldKind::Buff`]: a self buff's damage absorption (`damageAbsorptionPercent` %, then `damageAbsorption`
///   flat, a pool for `Skill_BuffSelfShield` that ends the shield when spent) for the types its
///   `…DamageQualifier` flags list (none = all). Ends with `skillActiveDuration` or when the skill stops.
/// - [`ShieldKind::Potion`]: a charge potion's absorb (`absorbShieldPercentHealth`), trackable while the pool
///   lasts (`Skill_ChargePotion::IsTrackable`: absorb left / the last potion's amount).
/// - [`ShieldKind::Shrine`]: a Shattered Realm shrine buff's.
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

    pub fn potion(&self) -> Option<&Shield> {
        self.0
            .iter()
            .find(|s| matches!(s.kind, ShieldKind::Potion(_)))
    }

    /// `ApplyDamage`.
    pub fn filter(&mut self, ty: gd_sim::attributes::CombatType, v: f32) -> f32 {
        self.0.iter_mut().fold(v, |v, s| s.absorb.filter(ty, v))
    }

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

/// Hotbar slots of a save bar (47): 0–9 keys 1–0, 10 left mouse, 12 right mouse, 11 / 13 the left / right mouse
/// of the alternate weapon set, 24 / 25 the health / energy potions, 46 evade; 14–23 the second row's keys.
/// Keys 1–0 fire whichever row the HUD shows, flipped by ToggleHotSlotBar (`UIPlayerHud::PressHotSlotButton`).
pub const SLOT_LEFT: usize = 10;
pub const SLOT_RIGHT: usize = 12;
pub const SLOT_ROW2: usize = 14;
pub const SLOTS: usize = 24;
pub const DEFAULT_ATTACK: &str = "records/skills/default/defaultweaponattack.dbr";

/// The player's main hand holds a ranged weapon: weapon attacks fire its `basicProjectileName`.
#[derive(Component, Clone, Debug)]
pub struct RangedWeapon {
    pub projectile: String,
    /// The Long range profile (`GetWeaponRangeProfile`): `longRange` + the 0.5 tolerance; the game adds both
    /// radii on top (`Character::GetTargetDistance`).
    pub reach: f32,
}

/// Applied by `combat::apply_hits`.
#[derive(Message, Clone, Debug)]
pub struct SkillHit {
    pub attacker: Entity,
    pub target: Entity,
    pub skill: String,
    pub level_index: usize,
    /// The weapon-pool skill a pool attack runs under.
    pub parent: Option<(String, usize)>,
    /// Projectile range falloff (`gd_sim::actor::range_scale`); 1 otherwise.
    pub scale: f32,
    /// `TakeAttack` style 2: projectile block, deflect.
    pub projectile: bool,
}

/// A summon cast fired.
#[derive(Message, Clone, Debug)]
pub struct SpawnPet {
    pub owner: Entity,
    pub skill: String,
    pub level_index: usize,
    pub point: Vec3,
    /// `ActivateNow`'s target id: a `Skill_TargetedSpawnPet` packs its summons around it.
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
    /// `Skill_TargetedSpawnPet`, `Skill_SpawnPet`, ….
    Pet,
    /// `Skill_AttackWave`: a travelling, widening band.
    Wave,
    /// `Skill_AttackLineFan`: a fan of travelling lines that burst at the end of their life.
    LineFan,
    /// `Skill_AttackLine`: a point travelling at the target that hits around it every period.
    Line,
    /// `Skill_BuffAttackRadiusDrop` (Devastation): `projectileLaunchNumber` drops every `skillTargetInterval`
    /// for `skillActiveDuration` within `skillTargetRadius` of the point.
    Storm,
    /// `Skill_AttackProjectileAreaEffect` (Sigil of Destruction): an area of `projectileExplosionRadius` at the
    /// point hitting every `targetInterval` ms (`ProjectileAreaEffect::UpdateSelf`) for `skillActiveDuration`.
    Area,
    /// `Skill_CharonGeysers`: areas at the level's geyser markers around the caster.
    Geysers,
    /// `Skill_AttackPattern`: delayed spots around a centre that run the skill's secondaries.
    Pattern,
    /// `Skill_TurretFireControl`: the caster's turrets around it use a special.
    TurretFire,
    /// `SkillChanneled` subclasses: channelled while held ([`Channel`]).
    Beam,
    /// Toggled skills: a use flips them; the player's are auto toggles.
    Toggle,
    /// `Skill_AttackChain::ActivateNow`: the target, then each of the `sparkMaxNumber` jumps rolls
    /// `sparkChance` (a miss spends the jump) and picks a random unhit foe within `sparkGap`, with a
    /// `lightningName` bolt between each.
    Chain,
    /// `Skill_AttackPathCharge` and leaps / teleports: a dash to the point, then a radius hit ([`PathCharge`]).
    PathCharge,
    /// `Skill_DispelMagic` (Nullification): around the point (`projectileExplosionRadius`).
    Dispel,
    /// Instant effects on the caster: `Skill_GiveBonus`, `Skill_RefreshCooldown`.
    Instant,
    /// `Skill_Shapeshift`: toggles the form.
    Shapeshift,
    /// `Skill_AttackSpellReflectLink` (Mark of Torment): a link to the target.
    ReflectLink,
    Unsupported,
}

pub fn kind_of(rec: &Record) -> Kind {
    let c = rec.class.as_str();
    match c {
        "Skill_WPAttack_BasicAttack" => Kind::Weapon,
        // `Skill_Kick::TargetResult`: the record's own `weaponDamagePct` and flat offense, no weapon pool.
        "Skill_Kick" => Kind::Weapon,
        _ if c.starts_with("Skill_WeaponPool") => Kind::Weapon,
        _ if c.starts_with("Skill_WPAttack") || c.starts_with("Skill_AttackWeapon") => Kind::Weapon,
        "Skill_AttackProjectileDrop" => Kind::Drop,
        "Skill_BuffAttackRadiusDrop" => Kind::Storm,
        "Skill_AttackProjectileAreaEffect" => Kind::Area,
        "Skill_CharonGeysers" => Kind::Geysers,
        "Skill_TurretFireControl" => Kind::TurretFire,
        "Skill_AttackPattern" => Kind::Pattern,
        "Skill_AttackSpellBeam"
        | "Skill_AttackSpellCone"
        | "Skill_AttackSpellDrain"
        | "Skill_AttackRadiusGrow"
        | "Skill_AttackRadiusSpin" => Kind::Beam,
        "Skill_AttackRadiusLeap"
        | "Skill_AttackRadiusDisengage"
        | "Skill_AttackRadiusTeleport"
        | "Skill_AttackSpellTeleportSelf" => Kind::PathCharge,
        // The target foe alone, no radius (`Skill_AttackSpellChaos::ActivateNow` needs a foe).
        "Skill_AttackSpellChaos" | "Skill_AttackSpellChaosSpawnPet" => Kind::TargetRadius,
        "Skill_AttackLineFan" => Kind::LineFan,
        "Skill_AttackLine" => Kind::Line,
        "Skill_BuffSelfImmobilize" | "Skill_BuffSelfShield" | "Skill_BuffAttackRadiusLightning" => {
            Kind::Buff
        }
        "Skill_GiveBonus" | "Skill_RefreshCooldown" | "SkillActivated_Suicide" => Kind::Instant,
        "Skill_Shapeshift" => Kind::Shapeshift,
        "Skill_AttackSpellReflectLink" => Kind::ReflectLink,
        "Skill_DispelMagic" => Kind::Dispel,
        "Skill_AttackPathCharge" => Kind::PathCharge,
        "Skill_AttackChain" => Kind::Chain,
        // The target entity alone (`Skill_AttackSpell::ActivateNow` lists just it).
        "Skill_AttackSpell" => Kind::TargetRadius,
        // Swaps places with its target first.
        "Skill_AttackSpellTeleport" => Kind::TargetRadius,
        // A timed damaging aura: a buff plus [`DamageAura`]s for its duration.
        "Skill_BuffAttackRadiusDuration" => Kind::Buff,
        _ if gd_sim::skills::is_toggle(c) => Kind::Toggle,
        _ if c.starts_with("Skill_AttackRadiusLightning") => Kind::TargetRadius,
        "Skill_AttackBuffRadius" if rec.bool("pointBlank") => Kind::SelfRadius,
        "Skill_AttackBuffRadius" => Kind::TargetRadius,
        // Hits around the target point (1 m without `skillTargetRadius`).
        "Skill_AttackBuff" => Kind::TargetRadius,
        "Skill_TargetedSpawnPet"
        | "Skill_SpawnPet"
        | "Skill_SpawnMiniPet"
        | "Skill_SpawnPetMonster"
        | "Skill_MonsterGenerator"
        | "Skill_AktaiosMirage" => Kind::Pet,
        "Skill_AttackWave" => Kind::Wave,
        "Skill_AttackRadius" => Kind::SelfRadius,
        _ if c.starts_with("Skill_AttackProjectile") => Kind::Projectile,
        "Skill_BuffSelfDuration" | "Skill_BuffSelfSick" | "Skill_BuffRadius" => Kind::Buff,
        "Skill_BuffSelfColossus" => Kind::Buff,
        _ => Kind::Unsupported,
    }
}

/// Running cooldowns by skill (or scroll key): seconds left, the total they started with and the skill's
/// cooldown charges (`Skill.cooldownLeft` / `cooldownTotal`; a refresh shortens only the left time).
///
/// Charges (`cooldownCharges`, modifier-summed; `Skill::StartCooldown` / `EndCooldown`): a use spends one charge
/// and starts the cooldown only while none runs; each finished cooldown gives a charge back and starts the next
/// while charges are still spent (`SetChargeCooldown`). A skill is usable while it has an unspent charge and no
/// disruption's cooldown runs on it ([`Self::replace`]). `StartCooldown` treats 0.1 s or less as none.
#[derive(Clone, Debug, Default)]
pub struct Cooldowns(HashMap<String, Cooldown>);

#[derive(Clone, Debug)]
struct Cooldown {
    left: f32,
    total: f32,
    /// Charges spent: each running cooldown holds one.
    used: u32,
    /// The modifier-summed `cooldownCharges`.
    max: u32,
    /// A disruption's cooldown runs: the skill is unusable until it ends, whatever charges are left.
    disrupted: bool,
}

impl Cooldowns {
    /// [`Self::start`] with one charge (scrolls, an item toggle's re-arm).
    pub fn insert(&mut self, skill: String, secs: f32) {
        self.start(skill, secs, 1);
    }
    /// `Skill::StartCooldown`: a charge is spent and the timer starts only while none runs.
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
    /// No disruption runs, and no cooldown runs or a charge is left.
    pub fn usable(&self, skill: &str) -> bool {
        self.0
            .get(skill)
            .is_none_or(|c| !c.disrupted && c.used < c.max)
    }
    pub fn get(&self, skill: &str) -> Option<&f32> {
        self.0.get(skill).map(|c| &c.left)
    }
    /// Case-insensitive (the proc stores' keys).
    pub fn left_of(&self, skill: &str) -> Option<f32> {
        self.0
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(skill))
            .map(|(_, c)| c.left)
    }
    pub fn contains_key(&self, skill: &str) -> bool {
        self.0.contains_key(skill)
    }
    /// 0..1 (`Skill::GetCooldownCompletion`).
    pub fn completion(&self, skill: &str) -> Option<f32> {
        self.0.get(skill).map(|c| {
            if c.total > 0.0 {
                (1.0 - c.left / c.total).clamp(0.0, 1.0)
            } else {
                1.0
            }
        })
    }
    /// `Skill::EndCooldown(ms)`: `secs` off the time left of the cooldown of any of `names`, the total kept.
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
    pub fn any_of(&self, names: &[String]) -> bool {
        self.0
            .keys()
            .any(|k| names.iter().any(|n| n.eq_ignore_ascii_case(k)))
    }
    /// A finished cooldown gives its charge back (`EndCooldown`).
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
    /// A cooldown reached 0: a charge comes back; while charges are still spent the next cooldown starts
    /// (`SetChargeCooldown`), else the entry ends. A disruption's cooldown holds no charge of its own.
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
    /// `SkillManager::RefreshCooldown`: every running cooldown shortens by `secs` (`EndCooldown`), the `except`
    /// keys skipped (the caster's own skill; the item list's potions).
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
    /// `Skill::ReplaceCooldownTime(ms, 0)` (disruption's): `skill` cools down for `secs` unless its running
    /// cooldown has more left. PORT: a replaced cooldown blocks the skill until it ends, charges left or not (how
    /// the engine's charge check reads a replaced cooldown is not traced).
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

/// The player's hotbar, energy and cooldowns.
#[derive(Component, Clone)]
pub struct Caster {
    /// See [`SLOT_LEFT`].
    pub slots: [Option<String>; SLOTS],
    pub energy: f32,
    pub cooldowns: Cooldowns,
    /// The hotbar pages of the skill sets that are not displayed (`PlayerHotSlotCtrl`'s per-set `SkillSet` map,
    /// key = set id); `slots` is the displayed page, of set [`Caster::shown`].
    pub sets: std::collections::BTreeMap<i32, [Option<String>; SLOTS]>,
    /// `PlayerHotSlotCtrl.activeSkillSet` (0 = `kDefaultSkillSet`).
    pub set: i32,
    /// `PlayerHotSlotCtrl.displayedSkillSet`: [`Caster::set`] unless Show Alternate Hot Slot Set shows another
    /// page. Every slot read, use and bind goes to the displayed page.
    pub shown: i32,
}

impl Caster {
    /// `PlayerHotSlotCtrl::SetActiveSkillSet`: shows the page of set `id` (true when it was created); the
    /// displayed page goes back to the active set's.
    pub fn set_active_set(&mut self, id: i32) -> bool {
        if id == self.set {
            return false;
        }
        self.set = id;
        self.show_page(id)
    }

    /// True when set `id` had no page.
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

    /// `PlayerHotSlotCtrl::NextDisplayedSkillSet`: the set map's entry after the displayed id, past the end the
    /// active set's page.
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

    /// `PlayerHotSlotCtrl::ResetDisplayedSkillSet`.
    pub fn reset_displayed_set(&mut self) {
        self.show_page(self.set);
    }

    /// The displayed one included.
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

#[derive(Component, Clone)]
pub struct Casting {
    pub skill: String,
    pub level_index: usize,
    pub kind: Kind,
    pub point: Vec3,
    pub target: Option<Entity>,
    /// Cast by a monster: hits the player and their pets, not monsters.
    pub hostile: bool,
    /// The weapon-pool skill a pool attack runs under.
    pub parent: Option<(String, usize)>,
}

/// A weapon charge's warm-up paid its cost and started its cooldown; the cast it arrives at
/// (`Use::ArrivedWarm`, naming [`Casting::skill`]) fires without paying again
/// (`Skill_AttackWeaponCharge::ActivateNow`: only the first hit iteration pays). Whatever replaces or cancels
/// its move drops the marker.
#[derive(Component)]
pub struct WarmPaid(pub String);

/// The cast whose animation may still be playing after its hit fired it: a later swipe callback of that
/// animation still belongs to the skill.
#[derive(Component, Clone, Debug)]
pub struct LastCast {
    pub skill: String,
    pub weapon: bool,
    pub anim: gd_sim::anim::AnimType,
}

/// A player whose left-click skill is a spell: clicked monsters are attacked by casting it.
#[derive(Component)]
pub struct SpellAttacker;

/// What learned modifiers add to a projectile skill: `projectilePiercing`, further fragment sets
/// (`Skill_ProjectileModifier` `projectileFragmentsName`) and visual overrides.
#[derive(Clone, Default, Debug)]
pub struct ProjMods {
    pub pierce: f32,
    pub fragments: Vec<(String, u32, u32)>,
    /// The skill's fragments when a modifier overrides them.
    pub fragments_name: Option<String>,
    /// `SkillActivated::AddProjectileEffects`.
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

/// Any skill but the default attack on the left button is cast on every attack.
pub fn spell_left(data: &GameData, caster: &Caster) -> bool {
    caster.slots[SLOT_LEFT]
        .as_deref()
        .filter(|s| !s.eq_ignore_ascii_case(DEFAULT_ATTACK))
        .and_then(|s| data.db.get(s))
        .is_some_and(|r| kind_of(r) != Kind::Unsupported)
}

/// Use a hotbar slot: at `target` (the character under the cursor: a foe, or one's own pet) or the ground
/// `point`; `force` is Shift + left button (attack in place); `scripted` marks test-hook casts (their channels
/// end after 2 s).
///
/// The aim by the skill's [`TargetType`](gd_sim::skills::TargetType)
/// (`ControllerPlayerState::DefaultRequestSkillAction`):
/// - `Ally`: the friend under the cursor, else the request fails (`Skill_GiveBonus::GetValidTarget`): no
///   cast, no cost;
/// - `Enemy` with a friend under the cursor and no foe: refused silently (`Skill::CanTargetFriendAsEnemy`);
/// - otherwise the foe (if any) and the point;
/// - then `SkillActivatedSpell::GetValidTarget` ([`gd_sim::skills::request_aim`]): a `targetingMode` `Point`
///   or ground-only spell drops the foe, any other is refused without a foe.
///
/// PORT: the friend is one's own living pet.
#[derive(Message, Clone, Debug)]
pub struct CastRequest {
    pub slot: usize,
    pub target: Option<Entity>,
    pub point: Vec3,
    pub force: bool,
    pub scripted: bool,
}

/// A press on a pet portrait with a hot slot armed: the armed slot's skill is requested on the pet.
///
/// `PlayerPortrait::HandleMouseEvent`: with a slot armed and the hot slots not disabled, a left or right press
/// sets the pet as the controller's combat ally, clears its combat enemy and runs
/// `ControllerPlayer::HandleActionFromMouse` at the pet; `ActivateSuperSkill` takes (and clears) the armed slot
/// and requests its skill, aimed like any request. PORT: the friend is the player's own pet (no party frames).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct CastOnAlly {
    pub ally: Entity,
}

/// The wrapper skill a hot slot cast ran as its `buffSkillName` record (`Casting::skill` is `buff`): what the
/// `CastBuff` / `CastDebuf` triggers classify. The last use's, instant ones included.
#[derive(Component, Clone, Debug)]
pub struct CastWrapper {
    pub buff: String,
    pub wrapper: String,
}

/// The wrapper a pending animated hot slot cast runs under: its cooldown's key and its item-skill check at the
/// fire. Set with the [`Casting`] only, so an instant cast during the animation does not change it.
#[derive(Component, Clone, Debug)]
pub struct CastCooldownKey(pub CastWrapper);

/// `Player::PlaySkillCooldownVox` and the UI's `tagSkillNotReady` text.
#[derive(Message, Clone, Copy, Debug)]
pub struct SkillNotReady;

/// A channelled skill (`SkillChanneled` subclasses): it ticks every `timeBetweenAttacks` ms (default 100) ÷ cast
/// speed (Spin: ÷ attack speed); each tick pays the full `skillManaCost` and the channel stops when the energy
/// left is under it; releasing the key ends it. The engine's timer (all five `Update`s): `timer -= frame ms; if
/// timer <= 0 { timer = (int)(timeBetweenAttacks / speed); tick }`: the overshoot is dropped and there is one
/// tick per update at most, so the rate follows the frame time; don't "fix" it. PORT: the period is not cut to
/// whole ms; the beam turns to the cursor at once. Death and a stun end the channel. A cone's strikes, energy
/// and target are its cone entry's; each re-request retargets it (`Skill_AttackSpellCone::GetValidTarget`).
#[derive(Component)]
pub struct Channel {
    pub skill: String,
    pub level_index: usize,
    pub slot: usize,
    pub tick: f32,
    pub scripted: bool,
    /// Set by the game's input.
    pub input_held: bool,
    /// The cursor's ground point (set by the game's input).
    pub input_aim: Option<Vec3>,
    /// The point the channel aims at this frame: a beam's slope.
    pub aim: Option<Vec3>,
    /// Seconds channelled (a grow's radius).
    pub held: f32,
    /// A drain's foe; a cone's, re-read from each re-request.
    pub target: Option<Entity>,
}

/// `Skill_AttackRadiusGrow::Update`: `skillTargetRadius` plus `radiusIncrement` per `timeBetweenGrow` ms
/// (default 100) ÷ cast speed, up to `explosionRadius`.
pub fn grow_radius(rec: &Record, li: usize, held: f32, spell_speed: f32) -> f32 {
    let r = level_f(rec, "skillTargetRadius", li).max(1.0);
    let steps = (held * 1000.0 * spell_speed.max(0.2)
        / rec.f32("timeBetweenGrow").unwrap_or(100.0).max(1.0))
    .floor();
    (r + steps * rec.f32("radiusIncrement").unwrap_or(0.0))
        .min(rec.f32("explosionRadius").unwrap_or(r).max(r))
}

/// The cone's frustum (`Skill_AttackSpellCone::BuildFrustum`): the near face at `near`, the far face at `far`,
/// each `w0` / `w1` wide along `right` and `h` high along `up`, facing `fwd`. True when the box `b` meets it.
/// PORT: a plane test against the foe's world box.
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

/// `state` (a `Local` of the calling system) holds the start time and the entries left.
#[cfg(feature = "dev")]
pub fn test_cast(
    state: &mut Option<(std::time::Instant, Vec<(f32, usize)>)>,
    hooks: &crate::hooks::Hooks,
) -> Option<usize> {
    let st = state.get_or_insert_with(|| (std::time::Instant::now(), hooks.gameplay.cast.clone()));
    let now = st.0.elapsed().as_secs_f32();
    let i = st.1.iter().position(|(t, _)| now >= *t)?;
    Some(st.1.remove(i).1)
}

/// A skill's own part of `Skill::GetTotalSkillAttribute`: `k` over its record and modifiers (each gated by its
/// `{k}Chance`, a fresh roll per call), plus |that| × the same sum of `{k}Modifier` %.
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

/// `Skill_AttackSpellBeam.beamNodes`: the held cast's last.
#[derive(Component, Default)]
pub struct SpellBeams(pub Vec<BeamNode>);

pub struct BeamNode {
    /// Unique per caster (the drawing's key).
    pub id: u64,
    pub skill: String,
    pub level_index: usize,
    pub beam: SpellBeam,
    /// Radians.
    pub angles: Vec<f32>,
    /// The heading with its slope (kept while retracting): the start effect sits at `emit + heading * retract`.
    pub heading: Vec3,
    pub emit: Vec3,
    pub dirs: Vec<Vec3>,
    /// Each ray meets a surface at its longest (its ground effect emits).
    pub surface: Vec<bool>,
    /// `beamName`, `startEffect`, `endEffect` as `ActivateNow` resolved them.
    pub effects: [Option<String>; 3],
}

impl BeamNode {
    /// From the retract point to the ray's far end.
    pub fn segment(&self, k: usize) -> (Vec3, Vec3) {
        let d = self.dirs[k];
        (
            self.emit + d * self.beam.retract,
            self.emit + d * self.beam.lens[k],
        )
    }
}

/// Ray count and spread (degrees), with the learned modifiers' (`GetSkillModifierData`).
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

/// One `Skill_AttackSpellChaos` cast in flight: its own entity.
#[derive(Component, Clone, Debug)]
pub struct ChaosBeam {
    pub caster: Entity,
    pub target: Entity,
    pub skill: String,
    pub level_index: usize,
    pub parent: Option<(String, usize)>,
    pub beam: String,
    /// The caster's `R Hand` when the beam was created (`None` before the first update).
    pub start: Option<Vec3>,
    /// The target's `Target` point while it is followed.
    pub end: Vec3,
    pub head: f32,
    pub tail: f32,
    /// `SetTopBottomPercent`: fractions of the way from `start`.
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

    /// One `Update` step (states 1 and 2); `target`: its `Target` point and whether it is alive, if it still
    /// resolves. Returns `(arrived, done)`: the head arrived this step (the hit), the beam is done (state 3).
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

/// `Skill_DispelMagic::TargetResult`: foes in the radius take the skill and lose their self buffs except
/// `notDispelable` ones; the caster's side in the radius loses its skill debuffs and, with `dots`, its damage
/// over time. `skillActiveDuration` is unused.
#[derive(Message, Clone)]
pub struct Dispel {
    pub caster: Entity,
    pub at: Vec3,
    pub radius: f32,
    pub foes: Vec<Entity>,
    pub dots: bool,
}

/// `Skill_AttackPathCharge` in flight (Vire's Might): runs straight to the point (`waveDistance` at most) and hits
/// every foe within `skillTargetRadius` × `endRadiusMultiplier` there; on the way, every `timeBetweenAttacks` ms
/// (default 100) it hits each foe within `skillTargetRadius` once. The move follows a nav path and fails when
/// that path is longer than `maxMoveRatio` × the straight distance (`CharacterMovementManager::FindPath`). The
/// run is the controller's ChargeToUseSkill move: the Charge clip at speed factor 1 (a set without it does not
/// move), warmed-up with `skillAllowsWarmUp`. PORT: the run follows the cast.
#[derive(Component)]
pub struct PathCharge {
    /// Nav waypoints left.
    pub to: Vec<Vec3>,
    /// Fixed speed (m/s, a teleport's), or `None`: the Charge clip's run.
    pub speed: Option<f32>,
    /// `skillAllowsWarmUp`: the run is at the warmed-up run speed with `bonus` `characterRunSpeedModifier` %.
    pub warm: bool,
    pub bonus: f32,
    pub skill: String,
    pub level_index: usize,
    pub radius: f32,
    pub hostile: bool,
    pub left: f32,
    /// [`ChargeStarts`].
    pub start: Vec3,
    /// `Skill_AttackPathCharge` only: radius (0 = none), seconds between, seconds to the next, foes already hit.
    pub path: (f32, f32, f32, Vec<Entity>),
    /// `Skill_AttackPathCharge` only: `secondarySkillDistance` and the spot the special secondaries last went off
    /// (`lastSecondarySpot`). Once the caster is that far (flat) from the spot, the spot moves to it and
    /// `SpecialActivateSecondarySkills` runs there (a distance of 0: every frame).
    pub secondaries: Option<(f32, Vec3)>,
    /// Resolved at the cast (`fxChanges` over the level's `radiusEffectName`).
    pub radius_fx: Option<String>,
}

/// `Skill_AttackRadiusLeap` and `Skill_AttackRadiusDisengage` (derived from it).
pub fn is_leap(class: &str) -> bool {
    matches!(
        class,
        "Skill_AttackRadiusLeap" | "Skill_AttackRadiusDisengage"
    )
}

/// A leap in progress: `JumpAttackAction::Execute` → `Character::JumpToUseSkill` sets the movement modifier to
/// `d / |z|`: `d` the horizontal start→target distance, `z` the jump animation's total root-motion-bone forward
/// translation (`GraphicsAnim::GetBoneCoords`, through the bone's bind matrix). `StartJump` starts the move to the
/// target, `StopJump` stops it; while moving the speed is the frame's root-motion velocity times the modifier
/// (`Character::CrowdAgentUpdate`), so the jump covers `d` over the animation's curve. The hit is the
/// animation's hit callback.
#[derive(Component)]
pub struct Leap {
    pub to: Vec<Vec3>,
    pub dist: f32,
    /// Set at `StartJump`.
    pub scale: Option<f32>,
}

impl Leap {
    /// Also the evade's (the same `JumpToUseSkill` movement).
    pub fn new(to: Vec<Vec3>, dist: f32) -> Self {
        Self {
            to,
            dist,
            scale: None,
        }
    }
}

/// A pak's effects with their attach points (the record itself when it is an effect, not a pak).
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
    // `CharFxPak::LoadMesh`: its `meshEffectNames` ride the character too, at the record's `attachPoint`, else its
    // origin (`PFxManager::AddMeshEffect`).
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

/// Marks an `FxMesh` record among a pak's effect names.
pub const MESH_PREFIX: &str = "mesh:";

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

/// `CharFxPak::Start` / `Stop`: the pak's sounds for a character effect started on `owner`; the loop and the stop
/// sound hang from the pak's first effect entity.
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

/// `Skill_BuffSelfImmobilize::Update` (Blade Barrier): while the skill is active the frame's ms come off
/// `holdTimer` (0 from the constructor, never reset by an activation); at ≤ 0 it re-arms with `holdPeriod` and the
/// holder gets `AddFixedDamage(immobilize, 1.0, min(holdCap, time left))`, so the hold ends within 0.25 s of the
/// buff's stop, whatever stops it. Inserted at the first cast.
#[derive(Component, Debug)]
pub struct ImmobilizeRefresh {
    /// The skill under its buff list name.
    pub skill: String,
    /// `holdTimer`, seconds.
    pub timer: f32,
}

/// `Skill_BuffSelfImmobilize.holdPeriod`.
pub const IMMOBILIZE_PERIOD_SECS: f32 = 0.1;

/// `Skill_BuffSelfImmobilize.holdCap`.
pub const IMMOBILIZE_HOLD_SECS: f32 = 0.25;

/// Damaging auras (`Skill_BuffAttackRadiusToggled`): every 1000 ms the skill hits every foe within
/// `skillTargetRadius` of its owner. Pets and monsters keep it on (their AI casts it as a buffSelf skill and a
/// toggle never runs out); PORT: the port starts it on, without that first cast.
pub struct DamageAura {
    pub skill: String,
    pub level_index: usize,
    pub radius: f32,
    pub hostile: bool,
    pub timer: f32,
    /// A timed aura's (`Skill_BuffAttackRadiusDuration`).
    pub left: Option<f32>,
    /// 1, or `skillTargetInterval` for the lightning storm (`Skill_BuffAttackRadiusLightning`).
    pub interval: f32,
    /// The storm hits `skillTargetNumber` random foes per tick (`Skill_BuffAttackRadiusLightning::Update`), each
    /// with a `lightningName` bolt from the owner.
    pub max_targets: Option<usize>,
    pub bolt: Option<String>,
    /// `cameraShakeDurationSecs` / `cameraShakeAmplitude`, sent per bolt.
    pub shake: Option<(f32, f32)>,
}

impl DamageAura {
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
    /// `Skill_BuffAttackRadiusToggled::StopSkill`: all but those `keep` names (`notDispelable`) stop; returns
    /// the stopped ones.
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

/// `CharacterBio::GetBonusLifeAmount`: flat × (1 + `characterHealIncreasePercent` %) plus % of max life × (1 +
/// that + `characterPercentHealIncreaseModifier` %). The over-time % uses only the first.
pub fn bonus_life(sheet: &gd_sim::stats::CharacterSheet, flat: f32, pct: f32) -> f32 {
    let h = sheet.heal_increase * 0.01;
    flat * (1.0 + h)
        + (sheet.life * pct * 0.01 * (1.0 + h + sheet.percent_heal_increase * 0.01)).max(0.0)
}

/// `Skill_BuffSelfToggled` and its subclasses, whose `Update` gives the self buff's bonus every second while on.
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

/// Seconds to each running self buff's next bonus (`Skill_BuffSelfToggled.bonusTimer`).
#[derive(Component, Default, Debug)]
pub struct SelfBuffHeals(pub Vec<(String, f32)>);

/// The player's switched-on self-buff toggles from the book and worn items, recomputed when the loadout changes.
#[derive(Component, Default)]
pub struct SelfBuffToggles(pub Vec<(String, usize)>);

/// `SkillProfile::LoadProfile`: elemental = fire, cold, lightning.
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
            for t in tys {
                if !v.contains(t) {
                    v.push(*t);
                }
            }
        }
    }
    v
}

/// The charge potions (`Skill_ChargePotion`), keys R (health) and E (energy).
///
/// `Skill_ChargePotion::TargetResult` → `CharacterBio::TakeBonus`: `skillLifePercent` % of max life (× the
/// healing bonuses) and `skillManaBonus` + `skillManaPercent` % of max energy land at once, while
/// `skillLifeBonus`, `skillLifePercentSlow` % and `skillManaPercentSlow` % fill a restore pool. Each learned
/// `Skill_PotionModifier` adds its own fields the same way, scaled by the potency 1 + (`modifierPotency` +
/// `potionTransfusion`) %. `potionInfusion` % moves the instant parts into the pool, raised by that %;
/// `potionTransfusion` drops the potion's own restore. `absorbShieldPercentHealth` % × potency of max life is an
/// absorb shield for the qualifying types until used up. The pools drain at max / `healthFeedRate` s and
/// (max − reserved) / `manaFeedRate` s, held to `…OverStorageLimit` × that max. `cooldownCharges` follow the
/// [`Cooldowns`] charge rule.
pub const HEALTH_POTION: &str = "records/skills/default/defaulthealthpotion.dbr";
pub const ENERGY_POTION: &str = "records/skills/default/defaultmanapotion.dbr";

/// The potion restore pools (`CharacterBio` slow life / energy).
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

/// Drink a potion (R / E).
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsePotion {
    Health,
    Energy,
}

/// `ControllerPlayerState::DefaultRequestSkillAction` ("MoveToUseSkill"): a skill walks within its usable
/// distance before casting when its range profile is Melee, it has `forceMovement`, or `classicCasting` is on;
/// any other ranged-profile skill casts at once and clamps in `try_cast`. With the [`Walk`] gate closed, only
/// weapon charges walk.
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

/// `gate`: `(InputMode != 0 || movementType != 1 || targetId != 0) && movementType != 2` (no gamepad input mode).
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

/// "ChargeToUseSkill": `Skill_AttackPathCharge` (move type 2) and the weapon charges (renamed by
/// `DefaultRequestSkillAction`; see [`warms_up`]).
pub fn charges_to_range(rec: &Record) -> bool {
    rec.class == "Skill_AttackPathCharge" || gd_sim::skills::is_weapon_charge(&rec.class)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Use {
    /// A fresh request (hotbar key, held left click).
    Request,
    /// The end of a `MoveToUseSkill` / ChargeToUseSkill move.
    Arrived,
    /// The end of a charge whose warm-up already paid.
    ArrivedWarm,
}

/// The weapon skills' warm-up (`Skill_AttackWeapon::WarmUpStart` as a ChargeToUseSkill move starts): with
/// `skillAllowsWarmUp` it plays the warm-up FX, starts the cooldown, pays `GetManaCost` and flags the warm-up
/// active; the move runs at the warm-up run speed (`gd_sim::skills::warmup_run_multiplier`). The move's end
/// (`WarmUpEnd`) stops the FX and cancels the skill unless the target is a living character (or a breakable).
/// The attack itself pays nothing more: `Skill_AttackWeaponCharge::ActivateNow` bumps the hit iteration and only
/// the first pays. A charge cut short keeps what it paid. PORT: the push-through is not done (the port's player
/// does not collide with characters).
pub fn warms_up(rec: &Record) -> bool {
    gd_sim::skills::is_weapon_charge(&rec.class) && rec.bool("skillAllowsWarmUp")
}

/// A weapon skill's warm-up running on its caster, from `WarmUpStart` to `WarmUpEnd`:
/// - `Skill_AttackWeapon::WarmUpStart` plays `skillWarmUpSound`, starts the level's `warmupFxPakName` entry in
///   the world at the caster (`Skill::CreateWarmUpFx`), then `Skill::WarmUpEffect(caster, true)`: the
///   `warmUpEffectName` effect on the caster at `warmUpEffectAttachPoint`.
/// - `WarmUpEnd` (warm-up active only): `StopParticleEffect`: the effect's live particles finish; the world pak
///   plays on.
/// - `Skill_AttackWeaponBlink`: hides the caster while warm ([`crate::visuals::Hidden`]); its `WarmUpEnd` shows
///   it again, unconditionally.
/// - Modifiers: `CreateWarmUpFx` also starts each learned then gear `Skill_Modifier`'s own `warmupFxPakName`
///   (no enabled check: an off gear modifier's too). `WarmUpEffect` asks the gear list, then the learned one:
///   the first enabled modifier with a `warmUpEffectName` wears its effect instead of the skill's. The sound is
///   the skill's only.
/// - `Skill_AttackPathCharge::WarmUpStart` sets its run up and ends in `Skill_AttackWeapon::WarmUpStart`.
///
/// The move that carries it: the player's ChargeToUseSkill, a monster's or pet's Pursue to its chosen weapon
/// charge, or a [`Charge`] / [`PathCharge`] run after the cast. PORT: the run after the cast stands for the move
/// before it (a use within range out of weapon reach, an AI hit whose target stepped out of reach, every path
/// charge); a fresh effect entity per warm-up (the engine restarts the kept one).
#[derive(Component)]
pub struct WarmUp {
    pub skill: String,
    /// The attached `warmUpEffectName` effect(s).
    pub fx: Vec<Entity>,
}

/// `Skill_AttackWeaponCharge` / `Skill_AttackWeaponBlink` warmed up: the caster pushes through characters
/// (`Character::SetPathingPushThru`) and runs at its warmed-up run speed. Two forms:
/// - a monster's Pursue to the weapon charge it chose: its AI moves it on the Run clip and its Attack uses the
///   skill at the end of the path;
/// - a dash after the cast (PORT, see [`WarmUp`]): the caster paths to the target (the player on the Charge
///   clip, an AI on the Run clip), stops at reach and hits it on arrival.
///
/// The dash gives up once it has run farther from its start than the target is plus `maxDistanceBuffer`
/// (`Skill_AttackWeaponCharge::MaxSkillDistance`; no check when 0); no time limit. It ends without its hit when
/// the target dies (`WarmUpEnd` cancels, [`warms_up`]) or the caster dies or is stunned. PORT: the skill's
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
    /// `bonus`: its `characterRunSpeedModifier` %.
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

/// Where each charge skill of a character last started (`Skill_AttackWeaponCharge.chargeStart`, set by
/// `WarmUpStart`), which `MaxSkillDistance` measures from. PORT: a dash's when it starts to move, not at the
/// warm-up start; a monster's Pursue warm-up's at its start.
#[derive(Component, Default)]
pub struct ChargeStarts(pub HashMap<String, Vec3>);

/// A Devastation-like rain of drops (`Skill_BuffAttackRadiusDrop::Update`): every `skillTargetInterval` around
/// the cast point (`targetingMode` `Point`) or else around the caster where it stands then. Each tick drops
/// `projectileLaunchNumber` (0 → 1) projectiles at [`storm_landing`] spots, launched from `dropHeight ±
/// dropVariation` plus a `dropRadius` disc offset (`DropLaunchPoint`), each flying straight to its spot at its own
/// `projectileVelocity`.
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
    /// `dropVariation`: the launch height is `height` ± this.
    pub drop_variation: f32,
    /// `dropRadius`: a uniform-disc sideways offset of the launch point.
    pub drop_radius: f32,
    pub explosion: f32,
    pub attacker: Entity,
}

/// `Skill_BuffAttackRadiusDrop::Update`: the radius is `1 + u·(radius − 1)` (an annulus, no drop within 1 m) at
/// the angle `a` (radians).
pub fn storm_landing(radius: f32, u: f32, a: f32) -> Vec3 {
    let r = 1.0 + u * (radius - 1.0).max(0.0);
    Vec3::new(a.sin() * r, 0.0, a.cos() * r)
}

/// A radius whose `expansionTime` (s) is above 0 grows from its centre to its full size over that time, hitting
/// each foe as it reaches it:
/// - `Skill_AttackRadius`: the Primary secondaries run at once on the full radius; each attack runs the special
///   pair, so a radius that hits nothing runs no special pair.
/// - `SkillSecondary_AttackRadius`: at the activation point; its attacks run the ordinary pair only.
///
/// `Update`, per frame: an entry past its time goes; else the radius is `full × elapsed / time` and the foes in
/// it not hit yet form one attack. No other class grows its hits (the others pass `expansionTime` only to the
/// ring fx or pace a debug sphere).
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

/// `Skill_AttackSpellCone.frustumHeight` (set by the constructor, never changed).
pub const CONE_HEIGHT: f32 = 5.0;

/// A `Skill_AttackSpellCone` cone list entry, its own entity: it starts its effect after the delay, grows, strikes
/// and retracts. The one source of the cone's length.
#[derive(Component, Debug)]
pub struct ConeFx {
    pub owner: Entity,
    pub skill: String,
    pub level_index: usize,
    /// `coneName` / `fxChanges`, resolved by the activation.
    pub record: String,
    /// Once emitting (state 2).
    pub effect: Option<Entity>,
    /// Seconds before it starts emitting (state 1).
    pub delay: f32,
    pub tick: f32,
    /// The grown length and the retract (m), `travelSpeed`.
    pub len: f32,
    pub retract: Option<f32>,
    pub speed: f32,
    /// The last placement (frozen once retracting): the emit point, `aimPoint`, the effect's rotation, the caster's
    /// rotation and `GetMaxConeLength`.
    pub emit: Vec3,
    pub aim: Vec3,
    pub rotation: Quat,
    pub caster_rot: Quat,
    pub max: f32,
}

impl ConeFx {
    /// State 4 (`dt` ms × 0.002 × `travelSpeed`): true when the retract reached the length (state 5).
    pub fn retract(&mut self, dt: f32) -> bool {
        let r = self.retract.get_or_insert(0.0);
        *r += dt * 2.0 * self.speed;
        if *r >= self.len {
            *r = self.len;
            return true;
        }
        false
    }

    /// `GetTargets`: from `emit + dir * retract` to `emit + dir * len`, `startWidth` wide at the near face and
    /// `startWidth + len / maxRange * (endWidth - startWidth)` at the far one, [`CONE_HEIGHT`] high, faced along
    /// the caster's axes.
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

    /// State 2: the length grows `travelSpeed` m/s up to `max` (`GetMaxConeLength`).
    pub fn grow(&mut self, dt: f32, max: f32) {
        if self.len < max {
            self.len += dt * self.speed;
        }
        if max <= self.len {
            self.len = max;
        }
    }
}

/// The y of the normalised `aim - emit` (`Update` / `GetMaxConeLength`).
pub fn cone_slope(emit: Vec3, aim: Vec3) -> f32 {
    (aim - emit).normalize_or_zero().y
}

/// The caster's rotation with its forward turned up to `slope`.
pub fn cone_rotation(caster: Quat, slope: f32) -> Quat {
    caster * Quat::from_rotation_x(-slope.atan())
}

/// The running channel's base visual on its caster.
#[derive(Component, Debug)]
pub struct ChannelFx {
    pub skill: String,
    /// Once created (a cone: its [`ConeFx`] entry).
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

pub fn ranged_weapon(data: &GameData, weapon: &str) -> Option<RangedWeapon> {
    let r = data.db.get(weapon).filter(|r| r.class.contains("Ranged"))?;
    let p = r.str("basicProjectileName")?.to_owned();
    let long = data.engine().long_range();
    Some(RangedWeapon {
        projectile: p,
        reach: long + 0.5,
    })
}
