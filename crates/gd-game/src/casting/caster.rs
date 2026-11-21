use super::*;

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
    /// widening band ([`crate::waves`]).
    Wave,
    /// `Skill_AttackLineFan`: a fan of travelling lines that burst at the
    /// end of their life ([`crate::line_fan`]).
    LineFan,
    /// `Skill_AttackLine`: a point travelling at the target that hits around it every period
    /// ([`AttackLine`]).
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
    /// caster ([`crate::geysers`]).
    Geysers,
    /// `Skill_AttackPattern`: delayed spots around a centre that run the
    /// skill's secondaries ([`crate::patterns`]).
    Pattern,
    /// `Skill_TurretFireControl`: the caster's turrets around it use a
    /// special ([`crate::turrets`]).
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
    /// target ([`crate::reflect_link`]).
    ReflectLink,
    Unsupported,
}

pub fn kind_of(rec: &Record) -> Kind {
    let c = rec.class.as_str();
    match c {
        "Skill_WPAttack_BasicAttack" => Kind::Weapon,
        // The kick (`crate::hotbar::kicks_breakable`): a melee attack with the
        // record's own `weaponDamagePct` and flat offense, no weapon pool
        // (`Skill_Kick::TargetResult` @0x1804f8cd0).
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
        // (`Skill_AttackSpellChaos::ActivateNow` @0x1804dce10 needs a foe,
        // `TargetResult` @0x1804dcff0 hits it). See `single_target`.
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
        // target entity alone (`Skill_AttackSpell::ActivateNow` @0x1804d82f0
        // lists just it). See `single_target`.
        "Skill_AttackSpell" => Kind::TargetRadius,
        // A `Skill_AttackSpell` that swaps places with its target first (see
        // [`swap_places`]).
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
/// keeps the timers (`+484` left, `+488` total ms; a refresh shortens only
/// the left time): the hotbar's wipe is `1 − left / total`
/// (`Skill::GetCooldownCompletion`, `GetCooldownRemaining`,
/// `GetCooldownTotal`).
///
/// Charges (`cooldownCharges`, modifier-summed;
/// `Skill::StartCooldown` @0x1804916b0 / `EndCooldown` @0x1804919d0): a use
/// spends one charge and starts the cooldown only while none runs; each
/// finished cooldown gives a charge back and starts the next while charges
/// are still spent (`SetChargeCooldown`, `UpdateCooldownCharges`, the potion
/// rule generalized). A skill is usable while it has an unspent charge and no
/// disruption's cooldown runs on it ([`Self::replace`]: a disrupted skill
/// waits it out whatever charges it has left).
/// `StartCooldown` treats a cooldown of 0.1 s or less as none.
#[derive(Clone, Debug, Default)]
pub struct Cooldowns(pub(super) HashMap<String, Cooldown>);

/// One skill's running cooldown and its charge state ([`Cooldowns`]).
#[derive(Clone, Debug)]
pub(super) struct Cooldown {
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
    /// `Skill::StartCooldown` without charges (scrolls, evade, an item
    /// toggle's re-arm): [`Self::start`] with one charge.
    pub fn insert(&mut self, skill: String, secs: f32) {
        self.start(skill, secs, 1);
    }
    /// `Skill::StartCooldown` @0x1804916b0: a cooldown of 0.1 s
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
    /// `Skill::EndCooldown(ms)` @0x1804919d0: `secs` off the
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
    /// `SkillManager::RefreshCooldown` @0x18053cb30: every
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
    /// `Skill::ReplaceCooldownTime(ms, 0)` @0x180491a80 (disruption's): `skill` cools down for `secs`
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
pub(crate) fn replace_cooldown(list: &mut Vec<(String, f32)>, skill: &str, secs: f32) {
    if secs <= 0.0 {
        return;
    }
    match list.iter_mut().find(|c| c.0 == skill) {
        Some(c) => c.1 = c.1.max(secs),
        None => list.push((skill.to_owned(), secs)),
    }
}

/// The player's hotbar, energy and cooldowns. The timed self buffs are
/// [`crate::player::Loadout::buffs`] (`crate::skill_buffs::TimedBuffs`, the
/// one store player / monster / pet share).
#[derive(Component, Clone)]
pub struct Caster {
    /// Save hotbar slots (see [`SLOT_LEFT`]).
    pub slots: [Option<String>; SLOTS],
    pub energy: f32,
    pub cooldowns: Cooldowns,
    /// The hotbar pages of the skill sets that are not displayed
    /// (`PlayerHotSlotCtrl`'s per-set `SkillSet` map, key = set id; the
    /// save's `bars`, `WriteProperties` @0x1803edc70 writes every page).
    /// `slots` is the displayed page, of set [`Caster::shown`].
    pub sets: std::collections::BTreeMap<i32, [Option<String>; SLOTS]>,
    /// The active skill set (`PlayerHotSlotCtrl` +152; 0 = the default set,
    /// `kDefaultSkillSet`).
    pub set: i32,
    /// The displayed skill set, whose page `slots` is: [`Caster::set`] unless Show Alternate Hot Slot Set shows
    /// another page ([`Caster::next_displayed_set`]). `PlayerHotSlotCtrl::GetDisplayedSkillSetIndex`
    /// @0x1803ee150 (+156, 0 = the active set's) and `GetDisplayedSkillSet` @0x1803eae40: every slot read, use and
    /// bind (`GetHotSlotOption`, `ActivateHotSlot`, `SetHotSlot`, the mouse slots) goes to the displayed page.
    pub shown: i32,
}

impl Caster {
    /// `PlayerHotSlotCtrl::SetActiveSkillSet` @0x1803edf30: shows the page
    /// of set `id`, creating an empty one on first use (returns true then).
    /// The displayed page goes back to the active set's (`PlayerHotSlotCtrl::Update` @0x1803eb2c0: a new
    /// active set clears the displayed one, +156).
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
    /// `PlayerHotSlotCtrl::NextDisplayedSkillSet` @0x1803ee070: the set map's entry after the displayed
    /// id (+156), past the end 0 (= the active set's page); no entry for the displayed id, no change. The HUD calls
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

    /// The active set's page again. `PlayerHotSlotCtrl::ResetDisplayedSkillSet` @0x1803ee140 (+156 = 0).
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

/// Death and the no-action states end what a character's skills have running. Game.dll:
/// - `Character::ImDying` @0x18005c0a0 → `SkillManager::ImDead` @0x180535160: the current skill is cleared (+1060)
///   and every active-list and item-list skill gets `StopSkill` (vtable +1176). A channel stops
///   (`Skill_AttackSpellCone` / `…Beam::StopSkill` @0x1804cda60 → `StopEmitting`; `Skill_AttackSpellDrain` /
///   `…RadiusSpin` / `…RadiusGrow::StopSkill` @0x1804c70e0), a charge is cancelled
///   (`Skill_AttackWeaponCharge::StopSkill` @0x1804c8880, the path charge's too → `Cancel` @0x1804e49a0:
///   push-through, trails and warm-up off, no hit). A dying skill is cast after that, by the Dying state
///   ([`ProcCast`], `crate::monster_skills`).
/// - `TakeStunAction::Execute` @0x180086180 (the sleep, knockdown and immobilize actions alike) →
///   `SkillManager::StopCurrentSkill` @0x180534540: the current skill, when `IsRunning` (the channelled classes),
///   gets `StopSkill`; the action takes the place of the character's own, the cast's (`crate::afflictions` drops
///   [`Casting`]) or the charge's move ([`warms_up`]: a charge cut short by a stun keeps what it paid).
///
/// PORT: a cast goes off on its animation's callback, which a dead character's cast no longer gets; the port's
/// pending [`Casting`] / ally cast would go off on the death animation's `End`, so it ends here too. Done at the
/// head of the casting chain, the frame after `combat::kill` / `crate::afflictions` marked the character (the
/// animation callbacks that could fire it come later than that), not inside them.
#[allow(clippy::type_complexity)]
pub(super) fn stop_skills(
    mut commands: Commands,
    dead: Query<
        Entity,
        (
            With<Dead>,
            Or<(
                With<Casting>,
                With<crate::monster_skills::AllyCast>,
                With<Channel>,
                With<WarmPaid>,
                With<Charge>,
                With<PathCharge>,
            )>,
        ),
    >,
    stunned: Query<
        Entity,
        (
            With<crate::movement::Stunned>,
            Without<Dead>,
            Or<(
                With<Channel>,
                With<Charge>,
                With<PathCharge>,
                With<WarmPaid>,
            )>,
        ),
    >,
) {
    for e in &dead {
        commands.entity(e).try_remove::<(
            Casting,
            crate::monster_skills::AllyCast,
            Channel,
            Charge,
            PathCharge,
            WarmPaid,
        )>();
    }
    for e in &stunned {
        commands
            .entity(e)
            .try_remove::<(Channel, Charge, PathCharge, WarmPaid)>();
    }
}

/// Marks a player whose left-click skill is a spell: clicked monsters are
/// attacked by casting it (`combat::player_attack` stays out of it).
#[derive(Component)]
pub struct SpellAttacker;

#[derive(Component)]
pub struct Projectile {
    pub(crate) attacker: Entity,
    /// Its record (the impact sounds, `crate::combat_sounds`).
    pub(crate) record: String,
    pub(crate) skill: String,
    pub(crate) level_index: usize,
    pub(super) vel: Vec3,
    pub(super) left: f32,
    pub(crate) explosion: f32,
    pub(crate) impact_fx: Option<String>,
    pub(crate) flight_fx: Option<Entity>,
    /// Drops explode on reaching the ground.
    pub(super) drop: bool,
    pub(crate) hostile: bool,
    /// Its caster's side, kept for when the caster is gone
    /// ([`crate::combat::Side::lookup_kept`]).
    pub(crate) side: Option<crate::combat::Side>,
    /// `projectileFragmentsName` with `...LaunchNumberMin/Max`: split on
    /// impact into fragments flying off level with the ground, each
    /// 360° / count further round plus ±30° jitter
    /// (`ProjectileBase::ProcessFragmentation`).
    pub(super) fragments: Vec<(String, u32, u32)>,
    /// Never hits this entity (a fragment's parent target).
    pub(super) exclude: Option<Entity>,
    /// Chance (%) to pierce a target and fly on (`projectilePiercing`).
    pub(crate) pierce: f32,
    /// Targets already pierced.
    pub(super) passed: Vec<Entity>,
    pub(crate) parent: Option<(String, usize)>,
    /// Where it was launched (range falloff).
    pub(crate) origin: Vec3,
    /// A `Skill_AttackProjectileDebuf`'s buff, landed on what it hits
    /// instead of an attack.
    pub(super) debuf: Option<ProjectileDebuf>,
    /// The ray radius it hits with: `actorRadius` (× 1 + growth / 2, growth
    /// 0 at launch); a target's bounding box counts when the ray from the
    /// last position to the new one passes within it
    /// (`ProjectileBase::GetTargetsAlongPath` @0x1804082d0, Engine.dll
    /// `Actor::Load`).
    pub(crate) touch: f32,
    /// `explodeOnMiss`: stopped by the world (or at the end of its flight) it
    /// still explodes (`ProjectileFireballLike::UpdateSelf` state 3).
    pub(crate) explode_on_miss: bool,
    /// Its skill is a `Skill_AttackProjectileSpawnPet`: its end summons
    /// (`TargetInformation`, [`crate::pets::placement::hit_spots`]).
    pub(crate) spawns_pet: bool,
    /// A `Skill_AttackProjectileChain`'s spark list, collected at launch
    /// (the primary target and its jumps, `sparkMaxNumber` / `sparkChance` /
    /// `sparkGap`, LOS-tested): the impact attacks every entry
    /// (`Skill_AttackProjectileChain::TargetResult` @0x1804c0150).
    pub(crate) chain: Vec<Entity>,
}

impl Projectile {
    /// Who launched it, whether it seeks the player's side (a monster's), its
    /// record: what `ProjectileBase::NotifyMonsters` reads (`crate::dodge`).
    pub fn owner(&self) -> (Entity, bool, &str) {
        (self.attacker, self.hostile, &self.record)
    }
}

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

pub(super) fn init_caster(
    hooks: Res<crate::hooks::Hooks>,
    mut commands: Commands,
    players: Query<Entity, (With<Player>, Without<Caster>)>,
    // Option: a new character has no `LoadedCharacter` (empty hotbar).
    loaded: Option<Res<crate::session::LoadedCharacter>>,
    data: Res<GameData>,
) {
    for e in &players {
        let mut slots: [Option<String>; SLOTS] = Default::default();
        let page = |bar: &gd_formats::save::gdc::HotSlotBar| {
            let mut p: [Option<String>; SLOTS] = Default::default();
            for (i, s) in bar.slots.iter().take(SLOTS).enumerate() {
                // The potion / evade / scroll options as their records
                // (`hotbar`).
                use gd_formats::save::gdc::HotSlot;
                p[i] = match s {
                    HotSlot::Skill { skill, .. } if !skill.is_empty() => Some(skill.clone()),
                    HotSlot::Plain(2) => Some(HEALTH_POTION.into()),
                    HotSlot::Plain(3) => Some(ENERGY_POTION.into()),
                    HotSlot::Plain(5) => Some(crate::evade::EVADE.into()),
                    HotSlot::Scroll { a, .. } if !a.is_empty() => Some(a.clone()),
                    _ => None,
                };
            }
            p
        };
        // Each bar is the page of one skill set (`id`); the player starts in
        // the default set 0 (a form's page comes back when it is entered).
        let mut sets = std::collections::BTreeMap::new();
        if let Some(c) = loaded.as_ref() {
            for bar in &c.0.ui.bars {
                sets.insert(bar.id as i32, page(bar));
            }
            if !sets.contains_key(&0)
                && let Some(b) = c.0.ui.bars.first()
            {
                sets.insert(0, page(b));
            }
        }
        if let Some(p) = sets.remove(&0) {
            slots = p;
        } else if loaded.is_none() {
            // A new character: the hotbar the game gives it (both sets' left
            // mouse slots hold the default attack, `crate::save::new_character`).
            slots = page(&gd_formats::save::gdc::HotSlotBar {
                id: 0,
                slots: crate::save::new_hotbar(),
            });
        }
        // `GD_SLOT` (hooks.rs).
        for (n, rec) in &hooks.gameplay.slot {
            if *n < SLOTS {
                slots[*n] = Some(rec.clone());
            }
        }

        let mut caster = Caster {
            slots,
            energy: f32::MAX,
            cooldowns: Cooldowns::default(),
            sets,
            set: 0,
            shown: 0,
        };
        // Weapon set II active: its mouse slots are the shown ones
        // (`crate::weapon_sets`).
        if loaded
            .as_ref()
            .and_then(|c| c.0.inventory.data.as_ref())
            .is_some_and(|d| d.use_alternate != 0)
        {
            crate::weapon_sets::swap_mouse_slots(&mut caster);
        }
        if caster.slots[SLOT_LEFT].is_none() {
            caster.slots[SLOT_LEFT] = Some(DEFAULT_ATTACK.into());
        }
        info!(
            "hotbar: {:?}",
            caster
                .slots
                .iter()
                .map(|s| s.as_deref().map(|p| p.rsplit('/').next().unwrap_or(p)))
                .collect::<Vec<_>>()
        );
        let spell = spell_left(&data, &caster);
        let mut ec = commands.entity(e);
        ec.insert(caster);
        if spell {
            ec.insert(SpellAttacker);
        }
    }
}

/// Any skill but the default attack on the left button is cast on every
/// attack (spells, and weapon skills such as Fire Strike).
pub(super) fn spell_left(data: &GameData, caster: &Caster) -> bool {
    caster.slots[SLOT_LEFT]
        .as_deref()
        .filter(|s| !s.eq_ignore_ascii_case(DEFAULT_ATTACK))
        .and_then(|s| data.db.get(s))
        .is_some_and(|r| kind_of(r) != Kind::Unsupported)
}

/// Keeps [`SpellAttacker`] matching the left-button skill (it changes with
/// the hotbar and the weapon set; [`regen_and_cooldowns`]' timers leave the change flag alone).
pub(super) fn sync_spell_attacker(
    mut commands: Commands,
    data: Res<GameData>,
    players: Query<(Entity, &Caster, Has<SpellAttacker>), (With<Player>, Changed<Caster>)>,
) {
    for (e, caster, has) in &players {
        match (spell_left(&data, caster), has) {
            (true, false) => {
                commands.entity(e).try_insert(SpellAttacker);
            }
            (false, true) => {
                commands.entity(e).remove::<SpellAttacker>();
            }
            _ => {}
        }
    }
}

/// Energy regen per second is `manaRegenEquation` of `playerresourcebehavior.dbr`
/// (`CharacterBio::GetManaRegenValue`), in the sheet's
/// `energy_regen_per_sec`; life regen likewise in `player_regen`.
pub(super) fn regen_and_cooldowns(time: Res<Time>, mut q: Query<(&mut Caster, &Combatant)>) {
    let dt = time.delta_secs();
    for (mut c, cb) in &mut q {
        // The timers are no change: `Changed<Caster>` is the hot slots'.
        let c = c.bypass_change_detection();
        // Toggled skills hold their reserve: energy is capped at max −
        // reserve × (1 − reduction %) (the cap `CharacterBio::TakeBonus`
        // applies); only switched-on toggles reserve
        // (`Skill_BuffSelfToggled::CollectPassiveCharAttributes` @0x1804ec1e0
        // checks the active flag; the port's `SkillBook.inactive`).
        let max = (cb.profile.sheet.energy - cb.profile.sheet.energy_reserve).max(0.0);
        c.energy = (c.energy + cb.profile.sheet.energy_regen_per_sec.max(0.0) * dt).min(max);
        c.cooldowns.tick(dt);
    }
}
