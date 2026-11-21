//! Hit resolution: to-hit, critical tiers, per-damage-type attacker
//! processing, resistances, armor, shield block, DoTs and leech, following
//! `CombatManager::TakeAttack` @0x18011d950 and the `CombatAttribute*`
//! classes (Game.dll).
//!
//! All arithmetic is f32 in the engine's operation order; `PCT` is the f32
//! `0.0099999998` the engine uses for "/100".
use crate::attributes::CombatType;
use crate::stats::{CombatFormulas, CombatVars, PCT, Region};

/// `GAME::RandomUniform`: Park–Miller minimal standard generator
/// (a = 16807, m = 2^31 - 1, Schrage with q = 127773, r = 2836), used in
/// every roll site of `CombatManager`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rng(pub u32);

impl Rng {
    // The engine's name for the step; `Rng` is no iterator.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u32 {
        let s = self.0;
        let t = 16807u32
            .wrapping_mul(s % 127_773)
            .wrapping_sub(2836u32.wrapping_mul(s / 127_773)) as i32;
        let t = if t < 0 {
            t.wrapping_add(0x7FFF_FFFF)
        } else {
            t
        };
        self.0 = t as u32;
        self.0
    }
    /// `(float)(int)next * 4.6566129e-10` — [0, 1).
    pub fn unit(&mut self) -> f32 {
        (self.next() as i32) as f32 * 4.656_613e-10
    }
}

/// Probability to hit (`DesignerCalculateProbabilityToHit` @0x180121790):
/// the `probabilityToHitEquation`, raised to `pthMinimum`.
pub fn probability_to_hit(f: &CombatFormulas, oa: f32, da: f32) -> f32 {
    let v = CombatVars {
        offensive_ability: oa,
        defensive_ability: da,
        ..Default::default()
    };
    let pth = f.probability_to_hit.as_ref().map_or(0.0, |e| v.eval(e));
    // (bad data: OA = DA = 0 makes the stock equation 0/0; NaN would fail
    // every later comparison and turn each hit into 0 damage.)
    if !pth.is_finite() || pth <= f.pth_minimum {
        f.pth_minimum
    } else {
        pth
    }
}

/// Result of [`damage_modifier`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitRoll {
    /// 0 = miss; <1 glancing (normalPTHEquation); 1 = normal; >1 critical.
    pub multiplier: f32,
    /// Critical tier 0..=5 (`pthDamageModifier1..6`); 0 also for non-crits.
    pub tier: u32,
}

/// `CombatManager::CalculateDamageModifier(pth, rand)` @0x180120b10:
/// `roll = max(100, pth) * rand`; a miss when `pth < 100 && roll > pth`.
/// Above `pthThreshold1` the roll picks the highest tier whose threshold it
/// exceeds (thresholds 2..6); otherwise the hit is scaled by
/// `normalPTHEquation` (0.75 if absent).
pub fn damage_modifier(f: &CombatFormulas, pth: f32, rand: f32) -> HitRoll {
    let roll = 100f32.max(pth) * rand;
    if pth < 100.0 && roll > pth {
        return HitRoll {
            multiplier: 0.0,
            tier: 0,
        };
    }
    let t = &f.pth_tiers;
    if pth > t[0].0 {
        for tier in (1..6).rev() {
            if roll > t[tier].0 {
                return HitRoll {
                    multiplier: t[tier].1,
                    tier: tier as u32,
                };
            }
        }
        HitRoll {
            multiplier: t[0].1,
            tier: 0,
        }
    } else {
        let v = CombatVars {
            probability_to_hit: pth,
            ..Default::default()
        };
        HitRoll {
            multiplier: f.normal_pth.as_ref().map_or(0.75, |e| v.eval(e)),
            tier: 0,
        }
    }
}

/// Critical chance shown in the UI (`DesignerCalculateCriticalChance`
/// @0x180120ae0): `max((pth - pthThreshold2) / max(100, pth), 0)`.
pub fn critical_chance(f: &CombatFormulas, pth: f32) -> f32 {
    ((pth - f.pth_tiers[1].0) / 100f32.max(pth)).max(0.0)
}

/// `CombatManager::PickRegion` @0x180120d90: `next % total + 1` against
/// the cumulative region chances in load order (bad data: negative chances
/// count as 0 and the sum is u64, so it cannot wrap).
pub fn pick_region(f: &CombatFormulas, rng: &mut Rng) -> Region {
    let chance = |c: i32| c.max(0) as u64;
    let total = f.regions.iter().map(|r| chance(r.1)).sum::<u64>().max(1);
    let target = u64::from(rng.next()) % total + 1;
    let mut cum = 0u64;
    for (r, c) in &f.regions {
        cum += chance(*c);
        if target <= cum {
            return *r;
        }
    }
    Region::FullyProtected
}

/// `GAME::ReductionInfo` (6 f32). Attacker side (`CollectCombatParameters`):
/// damage-reduction debuffs on the attacker (types 29, 28, -, 30, -, 31).
/// Defender side (`TakeAttack`): resistance reduction on the defender
/// (types 33, 32, 35, 34, 37, 36).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ReductionInfo {
    pub total_abs: f32,
    pub total_pct: f32,
    pub physical_abs: f32,
    pub physical_pct: f32,
    pub elemental_abs: f32,
    pub elemental_pct: f32,
}

/// `GAME::DamageScaleInfo`: two percentage multipliers (0 = unused).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DamageScale(pub f32, pub f32);

/// What a damage-side attribute evaluation needs from the attacker.
pub struct AttackerScaling<'a> {
    pub formulas: &'a CombatFormulas,
    /// Attacker's equation variables (Cunning = `dexterityDV`, Spirit =
    /// `intelligenceDV`, ...).
    pub vars: CombatVars,
}

impl AttackerScaling<'_> {
    fn eval(
        &self,
        eq: &Option<crate::expr::Expr>,
        set: impl Fn(&mut CombatVars, f32),
        x: f32,
    ) -> f32 {
        match eq {
            Some(e) => {
                let mut v = self.vars;
                set(&mut v, x);
                v.eval(e)
            }
            None => x,
        }
    }
    fn physical(&self, x: f32) -> f32 {
        self.eval(
            &self.formulas.physical_damage,
            |v, x| v.physical_damage = x,
            x,
        )
    }
    fn pierce(&self, x: f32) -> f32 {
        self.eval(&self.formulas.pierce_damage, |v, x| v.pierce_damage = x, x)
    }
    fn magical(&self, x: f32) -> f32 {
        self.eval(
            &self.formulas.magical_damage,
            |v, x| v.magical_damage = x,
            x,
        )
    }
    fn physical_duration(&self, x: f32) -> f32 {
        self.eval(
            &self.formulas.physical_duration_damage,
            |v, x| v.physical_damage = x,
            x,
        )
    }
    fn magical_duration(&self, x: f32) -> f32 {
        self.eval(
            &self.formulas.magical_duration_damage,
            |v, x| v.magical_damage = x,
            x,
        )
    }
}

fn reduce(x: f32, pct01: f32, abs: f32) -> f32 {
    x - ((x.abs() * pct01) + abs)
}

/// Instant damage of one type (`CombatAttributeAbsDamage`, or the
/// `...Elemental` subclass for Spirit-scaled types).
#[derive(Debug, Clone, PartialEq)]
pub struct HitDamage {
    pub ty: CombatType,
    /// Rolled value; `min`/`max` are kept for display like the engine.
    pub value: f32,
    pub min: f32,
    pub max: f32,
    /// Summed `offensiveXModifier` (%), applied at processing then cleared.
    pub modifier: f32,
    /// `this+16`: apply the Cunning/Spirit damage equations.
    pub scale_by_attributes: bool,
}

impl HitDamage {
    /// Engine roll (`DamageAttributeAbs::AddDamageToAccumulator` @0x18018da60):
    /// `chanceMult * (((max - min) * (float)r) * 4.6566129e-10 + min) * (a7 + a8)`.
    pub fn roll(ty: CombatType, min: f32, max: f32, scale: f32, rng: &mut Rng) -> Self {
        let r = rng.next() as i32 as f32;
        let value = ((((max - min) * r) * 4.656_613e-10) + min) * scale;
        Self {
            ty,
            value,
            min: min * scale,
            max: max * scale,
            modifier: 0.0,
            scale_by_attributes: true,
        }
    }

    /// `offensiveElementalMin/Max`: one roll split 1/3 each into Cold, Fire,
    /// Lightning (`DamageAttributeAbs_ElementalDamage::AddDamageToAccumulator`
    /// @0x18019aaa0; note its different f32 order).
    pub fn roll_elemental(min: f32, max: f32, scale: f32, rng: &mut Rng) -> [Self; 3] {
        let r = rng.next() as i32 as f32;
        let v = (((r * 4.656_613e-10) * (max - min)) + min) * scale * 0.333_333_34;
        let (lo, hi) = (min * scale * 0.333_333_34, max * scale * 0.333_333_34);
        CombatType::ELEMENTAL.map(|ty| Self {
            ty,
            value: v,
            min: lo,
            max: hi,
            modifier: 0.0,
            scale_by_attributes: true,
        })
    }

    /// Attacker-side processing: `CombatAttributeAbsDamage::Process`
    /// @0x180113690 / `CombatAttributeAbsDamageElemental::Process` @0x180113d50.
    /// The % modifier is taken from the *unscaled* value, so
    /// attribute bonuses and +% damage add.
    pub fn process(&mut self, atk: &AttackerScaling, red: &ReductionInfo, scale: DamageScale) {
        let elemental_class = self.ty.scales_with_spirit();
        let m = self.modifier;
        let add = [
            (self.value.abs() * m) * PCT,
            (self.min.abs() * m) * PCT,
            (self.max.abs() * m) * PCT,
        ];
        let mut v = [self.value, self.min, self.max];
        if self.scale_by_attributes {
            for x in &mut v {
                if *x > 0.0 {
                    if elemental_class {
                        *x = atk.magical(*x);
                    } else if self.ty == CombatType::Pierce {
                        *x = atk.pierce(*x);
                    }
                }
            }
        }
        for (x, a) in v.iter_mut().zip(add) {
            *x += a;
        }
        if scale.0 != 0.0 {
            v.iter_mut().for_each(|x| *x *= scale.0 * PCT);
        }
        if scale.1 != 0.0 {
            v.iter_mut().for_each(|x| *x *= scale.1 * PCT);
        }
        let mut pct = red.total_pct;
        if !elemental_class && self.ty == CombatType::Physical {
            pct = red.physical_pct.max(pct);
        } else if self.ty.is_elemental() {
            pct = red.elemental_pct.max(pct);
        }
        let p = pct * PCT;
        for x in &mut v {
            *x = reduce(*x, p, red.total_abs).max(0.0);
        }
        [self.value, self.min, self.max] = v;
        self.modifier = 0.0;
    }
}

/// Weapon base damage (`CombatAttributeDamage_BasePhysical`, from
/// `offensivePhysicalMin/Max` + `offensivePierceRatioMin` of a weapon).
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponDamage {
    /// Rolled, min, max — physical part after [`WeaponDamage::process`].
    pub physical: [f32; 3],
    /// Pierce part (filled by `process`).
    pub pierce: [f32; 3],
    /// % of the damage that is pierce.
    pub pierce_ratio: f32,
    /// `offensivePierceRatioModifier` (%, of the ratio).
    pub pierce_ratio_modifier: f32,
    /// `offensivePhysicalModifier` (%).
    pub physical_modifier: f32,
    /// `offensivePierceModifier` (%).
    pub pierce_modifier: f32,
    /// `this+84`: skill weapon-damage multiplier (1.0 = 100%).
    pub weapon_multiplier: f32,
    pub scale_by_attributes: bool,
    /// Armor gathered by `ExecuteDefense`: sum of absorption% * 0.01 * protection.
    pub protection_absorption: f32,
    /// Sum of protection of matching armor pieces.
    pub protection: f32,
    /// `this+68` armor bonus (0 in every path traced).
    pub protection_bonus: f32,
}

impl WeaponDamage {
    /// `DamageAttribute_BasePhysical::AddDamageToAccumulator` @0x1801a0370.
    pub fn roll(min: f32, max: f32, pierce_ratio: f32, scale: f32, rng: &mut Rng) -> Self {
        let range = (max - min).max(0.0);
        let hi = min + range;
        let r = rng.next() as i32 as f32;
        let v = ((((hi - min) * r) * 4.656_613e-10) + min) * scale;
        Self {
            physical: [v, min * scale, (min + range) * scale],
            pierce: [0.0; 3],
            pierce_ratio,
            pierce_ratio_modifier: 0.0,
            physical_modifier: 0.0,
            pierce_modifier: 0.0,
            weapon_multiplier: 1.0,
            scale_by_attributes: true,
            protection_absorption: 0.0,
            protection: 0.0,
            protection_bonus: 0.0,
        }
    }

    /// `CombatAttributeDamage_BasePhysical::Process` @0x180116c30.
    // `max().min()`, not `clamp`: a NaN ratio becomes 0.
    #[allow(clippy::manual_clamp)]
    pub fn process(
        &mut self,
        atk: &AttackerScaling,
        red: &ReductionInfo,
        scale: DamageScale,
        flat: f32,
    ) {
        let ratio = (((self.pierce_ratio.abs() * self.pierce_ratio_modifier) * PCT)
            + self.pierce_ratio)
            .max(0.0)
            .min(100.0);
        self.pierce_ratio = ratio;
        let r = ratio * PCT;
        let mut ph = self.physical;
        let mut pi = [0.0f32; 3];
        for i in 0..3 {
            pi[i] = ph[i] * r;
            ph[i] -= ph[i] * r;
        }
        let pm = self.physical_modifier;
        let ph_add = ph.map(|x| (x.abs() * pm) * PCT);
        if self.scale_by_attributes {
            ph.iter_mut()
                .filter(|x| **x > 0.0)
                .for_each(|x| *x = atk.physical(*x));
        }
        for i in 0..3 {
            ph[i] = (ph_add[i] + ph[i]).max(0.0);
        }
        let qm = self.pierce_modifier;
        let pi_add = pi.map(|x| (x.abs() * qm) * PCT);
        if self.scale_by_attributes {
            pi.iter_mut()
                .filter(|x| **x > 0.0)
                .for_each(|x| *x = atk.pierce(*x));
        }
        for i in 0..3 {
            pi[i] = (pi_add[i] + pi[i]).max(0.0);
        }
        let all = |ph: &mut [f32; 3], pi: &mut [f32; 3], f: &dyn Fn(f32) -> f32| {
            ph.iter_mut().chain(pi.iter_mut()).for_each(|x| *x = f(*x));
        };
        if scale.0 != 0.0 {
            all(&mut ph, &mut pi, &|x| (scale.0 * PCT) * x);
        }
        let wm = self.weapon_multiplier;
        all(&mut ph, &mut pi, &|x| wm * x);
        if scale.1 != 0.0 {
            all(&mut ph, &mut pi, &|x| (scale.1 * PCT) * x);
        }
        all(&mut ph, &mut pi, &|x| x - flat);
        let p = red.physical_pct.max(red.total_pct) * PCT;
        for x in &mut ph {
            *x = reduce(*x, p, red.total_abs).max(0.0);
        }
        for x in &mut pi {
            *x = (*x - (((x.abs() * red.total_pct) * PCT) + red.total_abs)).max(0.0);
        }
        self.physical = ph;
        self.pierce = pi;
        self.physical_modifier = 0.0;
        self.pierce_ratio_modifier = 0.0;
        self.pierce_modifier = 0.0;
    }
}

/// A damage-over-time or debuff (`CombatAttributeDurDamage` /
/// `...DurDamageElemental`): `offensiveSlowX{Min,Max,Duration*}`.
#[derive(Debug, Clone, PartialEq)]
pub struct DotDamage {
    pub ty: CombatType,
    /// Damage per second (rolled), min, max.
    pub dps: [f32; 3],
    /// Seconds.
    pub duration: f32,
    /// The duration as loaded, before its modifier (the attribute's +40,
    /// which `Process` leaves alone: `CombatAttributeDurDamage::
    /// GetMinMaxDamage` @0x1801144f0 matches the sheet's row duration
    /// against it).
    pub base_duration: f32,
    /// `offensiveSlowXModifier` (%).
    pub modifier: f32,
    /// `offensiveSlowXDurationModifier` (%).
    pub duration_modifier: f32,
    pub scale_by_attributes: bool,
    /// Hit multiplier recorded by `ProcessBluntDamageModifier`.
    pub hit_multiplier: f32,
    /// The owning source's stable key (`actor::dot_source_key`: the item
    /// instance / skill record / passive that owns the attribute, hashed with
    /// the attribute's field). `DamageAttributeDur::
    /// AddDamageToAccumulator` @0x180190560: the slot key is
    /// `(GetObjectId(owner), attribute_index)` — the owner object, not the
    /// attacker; distinct owners' same-type DoTs stack, only the same owner's
    /// re-application keeps max. 0: unkeyed (display-only values).
    pub source: u64,
}

impl DotDamage {
    /// `CombatAttributeDurDamage::Process` @0x180114170 and
    /// `CombatAttributeDurDamageElemental::Process` @0x1801148e0 (
    /// equation slot = `magicalDurationDamageEquation`; its reduction:
    /// total %, or the larger elemental % for cold / fire / lightning, then
    /// the flat total).
    pub fn process(&mut self, atk: &AttackerScaling, red: &ReductionInfo, scale: DamageScale) {
        let elemental_class = self.ty.scales_with_spirit();
        let m = self.modifier;
        let add = self.dps.map(|x| (x.abs() * m) * PCT);
        if self.scale_by_attributes {
            for x in &mut self.dps {
                if *x > 0.0 {
                    if elemental_class {
                        *x = atk.magical_duration(*x);
                    } else if matches!(self.ty, CombatType::Bleeding | CombatType::Physical) {
                        *x = atk.physical_duration(*x);
                    }
                }
            }
        }
        for (x, a) in self.dps.iter_mut().zip(add) {
            *x += a;
        }
        if scale.0 != 0.0 {
            self.dps.iter_mut().for_each(|x| *x *= scale.0 * PCT);
        }
        if scale.1 != 0.0 {
            self.dps.iter_mut().for_each(|x| *x *= scale.1 * PCT);
        }
        let mut pct = red.total_pct;
        if self.ty == CombatType::Physical && !elemental_class {
            pct = red.physical_pct.max(pct);
        } else if elemental_class && self.ty.is_elemental() {
            pct = red.elemental_pct.max(pct);
        }
        let p = pct * PCT;
        for x in &mut self.dps {
            *x = reduce(*x, p, red.total_abs).max(0.0);
        }
        let d = self.duration;
        self.duration = (((d.abs() * self.duration_modifier) * PCT) + d).max(0.0);
        self.modifier = 0.0;
        self.duration_modifier = 0.0;
    }
}

/// Damage instances an attack carries to the defender.
#[derive(Debug, Clone, PartialEq)]
pub enum Damage {
    Hit(HitDamage),
    Weapon(WeaponDamage),
    Dot(DotDamage),
    /// `offensiveLifeLeechMin/Max`: % of the life the hit removes, returned
    /// to the attacker (`CombatAttributeAbsDamage_LifeLeech::Execute`
    /// @0x180118900 accumulates the percent; `TakeAttack`'s tail heals the
    /// attacker by it × the life actually lost). `value` is the rolled
    /// percent, `min`/`max` its bounds (the sheet's Damage to Health row).
    LifeLeech {
        value: f32,
        min: f32,
        max: f32,
    },
    /// `offensivePercentCurrentLifeMin/Max`: % of the defender's current life
    /// (`CombatAttributeAbsDamage_PercentLife::Execute` @0x180118510).
    PercentCurrentLife {
        value: f32,
        min: f32,
        max: f32,
    },
    /// Crowd control (`CombatAttributeReflexDamage`: stun, freeze, trap,
    /// petrify, knockdown, sleep …): its duration in seconds, reduced by the
    /// defender's resistance of the same type like any damage value. Fear,
    /// confusion, convert (seconds), taunt (anger) and disruption (seconds)
    /// are handled alike; the last three become the hit's [`Influence`].
    Reflex {
        ty: CombatType,
        secs: f32,
    },
}

impl Damage {
    pub fn ty(&self) -> CombatType {
        match self {
            Damage::Hit(h) => h.ty,
            Damage::Weapon(_) => CombatType::PhysicalPierce,
            Damage::Dot(d) => d.ty,
            Damage::LifeLeech { .. } => CombatType::LifeLeech,
            Damage::PercentCurrentLife { .. } => CombatType::PercentCurrentLife,
            Damage::Reflex { ty, .. } => *ty,
        }
    }

    /// `ProcessBluntDamageModifier(mult)` (per class): hits, weapon
    /// parts and DoT dps are multiplied by the to-hit multiplier.
    fn blunt(&mut self, m: f32) {
        match self {
            Damage::Hit(h) => h.value = (m * h.value).max(0.0),
            Damage::Weapon(w) => {
                w.pierce[0] = (m * w.pierce[0]).max(0.0);
                w.physical[0] = (m * w.physical[0]).max(0.0);
            }
            // `CombatAttributeDurFixedDamage` (debuffs) ignores it.
            Damage::Dot(d) if is_dot_damage(d.ty) => {
                d.hit_multiplier = m;
                d.dps[0] = (m * d.dps[0]).max(0.0);
            }
            Damage::Dot(_) => {}
            Damage::LifeLeech { .. }
            | Damage::PercentCurrentLife { .. }
            | Damage::Reflex { .. } => {}
        }
    }

    /// `ScaleDamage(pct)` (per-class).
    /// Scales every value by `pct` %.
    pub fn scale(&mut self, pct: f32) {
        if pct < 0.0 {
            return;
        }
        let k = pct * PCT;
        match self {
            Damage::Hit(h) => {
                h.value *= k;
                h.min *= k;
                h.max *= k;
            }
            Damage::Weapon(w) => w
                .physical
                .iter_mut()
                .chain(w.pierce.iter_mut())
                .for_each(|x| *x *= k),
            // `CombatManager::TakeAttack` @0x18011d950: it calls `ScaleDamage` @0x18011af00 with
            // `includeType61 = false`, which skips type-61 attributes, so a 30 % sunder stays 30 %, not 45 %,
            // on an already sundered target.
            Damage::Dot(d) if d.ty == CombatType::DamageMultiplier => {}
            Damage::Dot(d) => d.dps.iter_mut().for_each(|x| *x *= k),
            // base `ScaleDamage` @0x180113b70: leech and % current
            // life scale like any value (damage-taken multiplier, reflect %).
            Damage::LifeLeech { value, min, max }
            | Damage::PercentCurrentLife { value, min, max } => {
                for x in [value, min, max] {
                    *x *= k;
                }
            }
            Damage::Reflex { .. } => {}
        }
    }

    /// `ReduceDamage(type, pct)`: `x = max((1 - pct * 0.01) * x, 0)` when the
    /// type matches (@0x180113af0 / BasePhysical @0x1801175f0).
    pub fn reduce_damage(&mut self, ty: CombatType, pct: f32) {
        let k = |x: f32| ((1.0 - pct * PCT) * x).max(0.0);
        match self {
            Damage::Hit(h) if h.ty == ty => h.value = k(h.value),
            Damage::Hit(h) => h.value = h.value.max(0.0),
            Damage::Dot(d) if d.ty == ty => d.dps[0] = k(d.dps[0]),
            Damage::Dot(d) => d.dps[0] = d.dps[0].max(0.0),
            Damage::Weapon(w) => match ty {
                CombatType::Physical => w.physical[0] = k(w.physical[0]),
                CombatType::Pierce => w.pierce[0] = k(w.pierce[0]),
                CombatType::PhysicalPierce => {
                    let f = 1.0 - pct * PCT;
                    let p = (f * w.physical[0]).max(0.0);
                    w.pierce[0] = (f * w.pierce[0]).max(0.0);
                    w.physical[0] = p;
                }
                CombatType::AbsorptionProtection => w.protection_absorption += pct,
                CombatType::Protection => w.protection += pct,
                _ => {}
            },
            Damage::LifeLeech { value, min, max } if ty == CombatType::LifeLeech => {
                for x in [value, min, max] {
                    *x = k(*x);
                }
            }
            // `..._PercentLife::ReduceDamage` @0x180118620: vitality
            // resistance resists it too.
            Damage::PercentCurrentLife { value, min, max }
                if ty == CombatType::PercentCurrentLife || ty == CombatType::Vitality =>
            {
                for x in [value, min, max] {
                    *x = k(*x);
                }
            }
            Damage::Reflex { ty: t, secs } if *t == ty => *secs = k(*secs),
            _ => {}
        }
    }

    /// `ReduceDurationDamage(type, pct)` @0x180114560.
    pub fn reduce_duration(&mut self, ty: CombatType, pct: f32) {
        if let Damage::Dot(d) = self {
            if pct > 0.0 && d.ty == ty {
                d.duration *= 1.0 - pct * PCT;
            }
            d.duration = d.duration.max(0.0);
        }
    }

    /// `DamageReductionByShield` -> `DesignerCalculateShieldBlockDamageReduction`
    /// per damage value (each instance is compared with the full block amount).
    fn shield(&mut self, f: &CombatFormulas, block: f32, absorption: f32) {
        let r = |x: f32| shield_block_reduction(f, x, block, absorption).max(0.0);
        match self {
            Damage::Hit(h) => h.value = r(h.value),
            Damage::Dot(d) => d.dps[0] = r(d.dps[0]),
            Damage::Weapon(w) => {
                w.physical[0] = r(w.physical[0]);
                w.pierce[0] = r(w.pierce[0]);
            }
            // base `DamageReductionByShield` @0x180113b40: a block
            // reduces leech, % current life and crowd control values too.
            Damage::LifeLeech { value, min, max }
            | Damage::PercentCurrentLife { value, min, max } => {
                for x in [value, min, max] {
                    *x = r(*x);
                }
            }
            Damage::Reflex { secs, .. } => *secs = r(*secs),
        }
    }
}

/// `DesignerCalculatePhysicalDamageDefense(dmg, protection, protAbs, bonus)`
/// @0x180121430: absorption = protAbs / protection; both scaled
/// by `(bonus + 1)`; `DLEP` when protection >= damage else `DGP`.
pub fn physical_damage_defense(
    f: &CombatFormulas,
    dmg: f32,
    protection: f32,
    prot_abs: f32,
    bonus: f32,
) -> f32 {
    if protection <= 0.0 {
        return dmg;
    }
    let absorption = prot_abs / protection;
    let v = CombatVars {
        physical_damage: dmg,
        sum_protection: (bonus + 1.0) * protection,
        sum_absorption: (bonus + 1.0) * absorption,
        ..Default::default()
    };
    let eq = if v.sum_protection >= dmg {
        &f.physical_defense_dlep
    } else {
        &f.physical_defense_dgp
    };
    let r = match eq {
        Some(e) => v.eval(e),
        None => dmg,
    };
    r.max(0.0)
}

/// `DesignerCalculateShieldBlockDamageReduction(dmg, block, absorption)`
/// @0x180121cc0: `DLEB` when dmg <= block, else `DGB`.
pub fn shield_block_reduction(f: &CombatFormulas, dmg: f32, block: f32, absorption: f32) -> f32 {
    let v = CombatVars {
        damage_dv: dmg,
        shield_defense: block,
        shield_absorption: absorption,
        ..Default::default()
    };
    let eq = if dmg <= block {
        &f.shield_dleb
    } else {
        &f.shield_dgb
    };
    match eq {
        Some(e) => v.eval(e).max(0.0),
        None => dmg,
    }
}

/// Restricted resistances (`DefenseAttributeAbsRestricted`: crowd control,
/// influence, leech, mana burn, disruption and % current life): their
/// `Process` @0x180115290 runs with a zeroed `ReductionInfo`, so resistance
/// reduction debuffs do not lower them.
pub fn restricted_resistance(ty: CombatType) -> bool {
    use CombatType as T;
    matches!(
        ty,
        T::Disruption
            | T::PercentCurrentLife
            | T::LifeLeech
            | T::ManaLeech
            | T::ManaBurn
            | T::ManaBurnRatio
            | T::Stun
            | T::Sleep
            | T::Trap
            | T::Freeze
            | T::Petrify
            | T::Knockdown
            | T::Taunt
            | T::Convert
            | T::Fear
            | T::Confusion
    )
}

/// Damage types a duration attribute deals over time (the rest are
/// debuffs: slows, reductions, fumble …).
pub fn is_dot_damage(ty: CombatType) -> bool {
    use CombatType as T;
    matches!(
        ty,
        T::Physical
            | T::Pierce
            | T::Cold
            | T::Fire
            | T::Acid
            | T::Lightning
            | T::Vitality
            | T::Chaos
            | T::Aether
            | T::Bleeding
    )
}

/// Defender-side combat attributes (`GetAllDefenseAttributes` result).
#[derive(Debug, Clone, PartialEq)]
pub enum Defense {
    /// `CombatAttributeAbsDefense`: `defensiveX` resistance; `restricted`:
    /// [`restricted_resistance`].
    Resistance {
        ty: CombatType,
        value: f32,
        modifier: f32,
        cap_bonus: f32,
        restricted: bool,
    },
    /// `CombatAttributeDurDefense`: `defensiveXDuration` (shortens DoTs, capped too).
    DurationResistance {
        ty: CombatType,
        value: f32,
        modifier: f32,
        cap_bonus: f32,
    },
    /// `CombatAttributeDefense_AbsorptionProtection`: one armor piece.
    Armor {
        region: Option<Region>,
        protection: f32,
        absorption: f32,
        protection_modifier: f32,
        absorption_modifier: f32,
    },
}

impl Defense {
    /// `Process` (`CombatAttributeAbsDefense::Process` @0x180115370,
    /// `CombatAttributeDurDefense::Process`, `..._AbsorptionProtection::Process`
    /// @0x180118370). `cap` is `GetDefenseAttributeCap()` (80 for players,
    /// 100 for monsters, per difficulty).
    pub fn process(
        &mut self,
        cap: f32,
        red: &ReductionInfo,
        scale: DamageScale,
        armor_reduction: f32,
    ) {
        match self {
            Defense::Resistance {
                ty,
                value,
                modifier,
                cap_bonus,
                restricted,
            } => {
                let mut v = ((value.abs() * *modifier) * PCT) + *value;
                if scale.0 > 0.0 {
                    v *= scale.0 * PCT;
                }
                let (abs, pct) = if *restricted {
                    (0.0, 0.0)
                } else {
                    let (mut abs, mut pct) = (red.total_abs, red.total_pct);
                    if *ty == CombatType::Physical {
                        abs = red.physical_abs.max(abs);
                        pct = red.physical_pct.max(pct);
                    } else if ty.is_elemental() {
                        abs = red.elemental_abs.max(abs);
                        pct = red.elemental_pct.max(pct);
                    }
                    (abs, pct)
                };
                v -= (v.abs() * (pct * PCT)) + abs;
                let c = cap + *cap_bonus;
                if c > 0.0 {
                    v = c.min(v);
                }
                *value = v;
                *modifier = 0.0;
            }
            Defense::DurationResistance {
                value,
                modifier,
                cap_bonus,
                ..
            } => {
                let mut v = ((value.abs() * *modifier) * PCT) + *value;
                if scale.0 > 0.0 {
                    v *= scale.0 * PCT;
                }
                v = v.max(0.0);
                let c = cap + *cap_bonus;
                if c > 0.0 {
                    v = c.min(v);
                }
                *value = v;
                *modifier = 0.0;
            }
            Defense::Armor {
                protection,
                absorption,
                protection_modifier,
                absorption_modifier,
                ..
            } => {
                let p_add = (protection.abs() * *protection_modifier) * PCT;
                *absorption =
                    (((absorption.abs() * *absorption_modifier) * PCT) + *absorption).min(100.0);
                let mut p = p_add + *protection;
                if scale.0 > 0.0 {
                    p *= scale.0 * PCT;
                }
                *protection = p - armor_reduction;
                *protection_modifier = 0.0;
                *absorption_modifier = 0.0;
            }
        }
    }

    /// `Execute(damage, region)`.
    pub fn execute(&self, d: &mut Damage, region: Region) {
        match self {
            Defense::Resistance { ty, value, .. } => d.reduce_damage(*ty, *value),
            Defense::DurationResistance { ty, value, .. } => d.reduce_duration(*ty, *value),
            Defense::Armor {
                region: r,
                protection,
                absorption,
                ..
            } => {
                let ok = r.is_none_or(|r| {
                    r == region || r == Region::FullyProtected || region == Region::FullyProtected
                });
                if ok {
                    d.reduce_damage(
                        CombatType::AbsorptionProtection,
                        (*absorption * PCT) * *protection,
                    );
                    d.reduce_damage(CombatType::Protection, *protection);
                }
            }
        }
    }
}

/// `TakeAttack` combat styles (`ParametersCombat+16`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AttackKind {
    /// 1: fumble, dodge, to-hit.
    #[default]
    Melee,
    /// 2: projectile fumble, deflect, to-hit.
    Ranged,
    /// 3: always hits (multiplier 1), no retaliation.
    Retaliation,
    /// 4/5: multiplier precomputed by the caster.
    Debuff(f32),
    /// 6: `max(1 - reflectResist%, 0)`.
    Reflection,
    /// Anything else: to-hit only.
    Direct,
}

/// Attacker-side parameters of one attack (`GAME::ParametersCombat`).
#[derive(Debug, Clone, PartialEq)]
pub struct Attack {
    pub kind: AttackKind,
    /// `+20`: attacker OA.
    pub offensive_ability: f32,
    /// `+148`: fumble chance % (melee).
    pub fumble_chance: f32,
    /// `+180`: projectile fumbled.
    pub projectile_fumbled: bool,
    /// `+176 > 0`: cannot be deflected/blocked (projectile).
    pub unblockable: bool,
    /// `+160`: crit damage bonus added to the tier multiplier when > 1.
    pub crit_damage_bonus: f32,
    /// `+152`: skill energy cost (for energy absorption).
    pub energy_cost: f32,
    /// Mana burn: % of the burned energy dealt as damage
    /// (`offensiveManaBurnDamageRatio` + `…RatioAdder`).
    pub mana_burn_ratio: f32,
    /// Damage after attacker-side processing.
    pub damage: Vec<Damage>,
}

/// A damage-absorbing buff on the defender: a `CombatFilter` that
/// `CombatManager::ApplyDamage` runs on every damage application (each type
/// of a hit, each DoT tick) `Skill_BuffSelfToggled::ModifyDamage` /
/// `Skill_BuffSelfShield::ModifyDamage`: only for the qualifying types (none
/// listed = all), the damage is cut by `pct` %, then `flat` soaks what is
/// left — per application, or as a pool that runs down (`pool`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Absorb {
    pub pct: f32,
    pub flat: f32,
    pub pool: bool,
    /// Qualifying types (`<type>DamageQualifier`); empty = all.
    pub types: Vec<CombatType>,
}

impl Absorb {
    /// The damage left after this filter.
    pub fn filter(&mut self, ty: CombatType, v: f32) -> f32 {
        if v <= 0.0 || (!self.types.is_empty() && !self.types.contains(&ty)) {
            return v;
        }
        let mut d = v;
        if self.pct > 0.0 {
            d -= self.pct * PCT * d;
        }
        if self.flat > 0.0 {
            let a = self.flat.min(d);
            d -= a;
            if self.pool {
                self.flat -= a;
            }
        }
        d
    }

    /// A pool shield with nothing left to give.
    pub fn spent(&self) -> bool {
        self.pool && self.flat <= 0.0 && self.pct <= 0.0
    }
}

/// Defender state (`CombatManager`/`Character` fields read by `TakeAttack`).
#[derive(Debug, Clone, PartialEq)]
pub struct Defender<'a> {
    /// Final DA (`DesignerCalculateDefensiveAbility(0)`, debuffs included).
    pub defensive_ability: f32,
    pub dodge: f32,
    pub deflect: f32,
    /// The defenses before `ProcessDefense` ([`take_attack`] processes a copy
    /// of each per hit, so a profile's list is shared, not cloned).
    pub defenses: &'a [Defense],
    /// `GetDefenseAttributeCap()`.
    pub resistance_cap: f32,
    /// Resistance reduction debuffs on the defender.
    pub resistance_reduction: ReductionInfo,
    /// `DefensiveReduction` (25) debuff: flat armor reduction.
    pub armor_reduction: f32,
    /// `100 + DDM(61) + negativeDamageMult` (% damage taken; scaled only when > 100).
    pub damage_taken_pct: f32,
    /// `defensivePercentReflectionResistance`, capped
    /// (`actor::Profile::reflection_resist`).
    pub reflection_resist: f32,
    /// `defensiveReflect` total (%).
    pub reflect: f32,
    pub shield: Option<Shield>,
    /// Damage-absorbing buffs ([`Absorb`]).
    pub absorb: Vec<Absorb>,
    /// `SkillManager::GetReflectLinkDamageReduction`: the summed
    /// `damageAbsorptionPercent` of the defender's reflect links (0: none),
    /// see [`HitResult::link_copy`].
    pub link_absorb: f32,
    /// `+720`: ms until the shield can block again.
    pub block_cooldown_ms: i32,
    /// Character attr 25 (%).
    pub block_recovery_reduction: f32,
    /// `defensiveBlockModifier` total (flat, added by the block equation).
    pub block_modifier: f32,
    /// `defensiveBlockAmountModifier` total (%).
    pub block_amount_modifier: f32,
    /// Racial bonus % damage reduction (`+712`).
    pub racial_reduction: f32,
    /// Character attr 26 (%).
    pub energy_absorption: f32,
    /// Current and max energy (mana burn).
    pub energy: f32,
    pub energy_max: f32,
    pub life: f32,
}

/// Equipped shield (`WeaponArmor_Shield` +5936..+5948).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shield {
    /// `defensiveBlockChance` (%).
    pub chance: f32,
    /// `defensiveBlock`: block amount.
    pub amount: f32,
    /// `blockAbsorption` (%).
    pub absorption: f32,
    /// `blockRecoveryTime` (s).
    pub recovery: f32,
}

impl Shield {
    /// A shield record's fields (`WeaponArmor_Shield::Load` @0x18058cd70,
    /// The record's own values, `GetShieldBlockChance` /
    /// `GetShieldDefense` / `GetShieldAbsorption` read them back,
    /// `GetShieldRecoveryTime` = `(int)(blockRecoveryTime × 1000)` ms).
    pub fn from_record(rec: &gd_formats::arz::Record) -> Self {
        use crate::items::db::Fields;
        Self {
            chance: rec.float("defensiveBlockChance"),
            amount: rec.float("defensiveBlock"),
            absorption: rec.float("blockAbsorption"),
            recovery: rec.float("blockRecoveryTime"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissReason {
    Fumble,
    Dodge,
    Deflect,
    Missed,
}

/// What an attack's influence and disruption attributes do to the defender, beside damage. They travel as
/// [`Damage::Reflex`] (no blunt, crit or damage-taken scaling, reduced by the defender's restricted resistance of
/// the type and by a block, never reflected nor linked) and are executed here, not as crowd control:
/// - taunt (50): `CombatAttributeInfluenceDamage_Taunt::Execute` @0x180118c30: a value above 0 goes to
///   `Character::CombatExertInfluenceTaunt` @0x180068f70 (the value passed on in xmm1) → the defender's controller
///   `ControllerMonster::TauntMe` @0x180139730: within `InPursuitRange`, `AngerManager::AddAnger(last attacker,
///   value)`. One call per instance, so the instances add up.
/// - convert (51): `CombatAttributeInfluenceDamage_Convert::Execute` @0x180118d00: a value of at least 1
///   calls `Monster::JoinMe(attacker, value × 1000 ms)` when the attacker's level + 5 reaches the defender's (the
///   caller's check: it needs both characters). A second instance meets a monster that already joined
///   (`JoinMe`'s refusal), so the first one counts.
/// - disruption (13): `CombatAttributeAbsDamage_Disruption::Execute` @0x180118770: a value above 0 calls
///   `Character::CombatAddCooldownDamage` @0x180069000 with `(int)(value × 1000)` ms → `SkillManager::
///   ApplyCooldownDamage` @0x18053d380, which puts each of the defender's skills on that cooldown unless it already
///   runs longer (`Skill::ReplaceCooldownTime`), so the longest instance counts.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Influence {
    /// Anger for the attacker (sum of the taunt instances).
    pub taunt: f32,
    /// Seconds the defender joins the attacker's side (0: none).
    pub convert: f32,
    /// Seconds of cooldown on the defender's skills (0: none).
    pub disruption: f32,
}

/// Outcome of [`take_attack`].
#[derive(Debug, Clone, PartialEq)]
pub struct HitResult {
    pub miss: Option<MissReason>,
    pub pth: f32,
    pub roll: HitRoll,
    pub region: Region,
    pub blocked: bool,
    /// Instant damage applied, per type (weapon: Physical after armor, Pierce).
    pub applied: Vec<(CombatType, f32)>,
    /// DoTs and debuffs handed to the defender's [`DotManager`].
    pub dots: Vec<DotDamage>,
    /// Crowd control started on the defender: (type, seconds).
    pub reflex: Vec<(CombatType, f32)>,
    /// Taunt, convert and disruption ([`Influence`]).
    pub influence: Influence,
    /// Life removed by this hit (capped by remaining life).
    pub life_lost: f32,
    /// The hit's absolute damage: every instant damage after the defenses
    /// (`TakeAttack` sums the accumulator's attributes into "Total Damage:
    /// Absolute" before `ExecuteDamage`, the floating combat number).
    /// PORT: taken before the damage-absorbing buffs' filter.
    pub total: f32,
    /// Life returned to the attacker.
    pub leeched: f32,
    /// Energy gained by the defender.
    pub energy_absorbed: f32,
    /// Energy the defender lost to mana burn.
    pub energy_burned: f32,
    /// Total reflect % (`defensiveReflect`).
    pub reflect_pct: f32,
    /// The incoming damage scaled by the reflect % (to send back with
    /// `actor::reflect`); empty without reflect.
    pub reflected: Vec<Damage>,
    /// A reflect link's share of the hit ([`Defender::link_absorb`]): the
    /// damage after the defenses, scaled by `link_absorb` % (to scale by the
    /// link's reflect %, `actor::link_reflection`); empty without a link.
    pub link_copy: Vec<Damage>,
}

/// `CombatManager::TakeAttack` @0x18011d950, single-player path, in
/// order. The engine draws from two generators (the
/// accumulator's for region, the attack seed for rolls); here one stream
/// is used in the same per-roll order.
pub fn take_attack(
    f: &CombatFormulas,
    atk: &mut Attack,
    def: &mut Defender,
    rng: &mut Rng,
) -> HitResult {
    let region = pick_region(f, rng);
    let mut res = HitResult {
        miss: None,
        pth: 0.0,
        roll: HitRoll {
            multiplier: 1.0,
            tier: 0,
        },
        region,
        blocked: false,
        applied: Vec::new(),
        dots: Vec::new(),
        reflex: Vec::new(),
        influence: Influence::default(),
        life_lost: 0.0,
        total: 0.0,
        leeched: 0.0,
        energy_absorbed: 0.0,
        energy_burned: 0.0,
        reflect_pct: 0.0,
        reflected: Vec::new(),
        link_copy: Vec::new(),
    };
    let to_hit = |res: &mut HitResult, rng: &mut Rng| {
        res.pth = probability_to_hit(f, atk.offensive_ability, def.defensive_ability);
        res.roll = damage_modifier(f, res.pth, rng.unit());
        if res.roll.multiplier == 0.0 {
            res.miss = Some(MissReason::Missed);
        }
    };
    match atk.kind {
        AttackKind::Melee => {
            if atk.fumble_chance > 0.0
                && (rng.next() as i32 as f32 * 4.656_613e-8) < atk.fumble_chance
            {
                res.miss = Some(MissReason::Fumble);
            } else if def.dodge > 0.0 && rng.unit() * 100.0 < def.dodge {
                res.miss = Some(MissReason::Dodge);
            } else {
                to_hit(&mut res, rng);
            }
        }
        AttackKind::Ranged => {
            if atk.projectile_fumbled {
                res.miss = Some(MissReason::Fumble);
            } else if def.deflect > 0.0 && rng.unit() * 100.0 < def.deflect && !atk.unblockable {
                res.miss = Some(MissReason::Deflect);
            } else {
                to_hit(&mut res, rng);
            }
        }
        AttackKind::Retaliation => {}
        AttackKind::Debuff(m) => res.roll.multiplier = m,
        AttackKind::Reflection => {
            res.roll.multiplier = (1.0 - def.reflection_resist * PCT).max(0.0)
        }
        AttackKind::Direct => to_hit(&mut res, rng),
    }
    // Energy absorption (`TakeAttack` tail `LABEL_315`): character
    // attribute 26 % of the attack's energy cost, gained on a miss too (the
    // miss path falls through to the same block).
    if def.energy_absorption > 0.0 {
        res.energy_absorbed = (def.energy_absorption * PCT) * atk.energy_cost;
    }
    if res.miss.is_some() || res.roll.multiplier <= 0.0 {
        return res;
    }
    // Damage-taken multiplier, crit bonus, blunt modifier.
    if def.damage_taken_pct > 100.0 {
        atk.damage
            .iter_mut()
            .for_each(|d| d.scale(def.damage_taken_pct));
    }
    // Reflection (`TakeAttack` @0x18011d950): not for retaliation
    // or reflected attacks. The copy is the incoming damage as it arrives,
    // scaled by the damage-taken multiplier like the real one but without
    // the hit roll / crit multiplier; `FilterReflectDamage` drops the
    // attributes flagged non-reflectable (crowd control `Reflex`, and
    // taunt/fear/convert), so hits, weapon damage, DoTs, debuffs, leech and
    // % current life are reflected, then `ScaleDamage(reflect %)` scales
    // every part. A player's `playerReflectCap` limits only the debuffs
    // (`CombatAttributeDurFixedDamage` uses the reflect cap), applied in
    // `actor::attack_linked`.
    if def.reflect > 0.0 && !matches!(atk.kind, AttackKind::Retaliation | AttackKind::Reflection) {
        res.reflected = atk
            .damage
            .iter()
            .filter(|d| !matches!(d, Damage::Reflex { .. }))
            .cloned()
            .map(|mut d| {
                d.scale(def.reflect);
                d
            })
            .collect();
        res.reflect_pct = def.reflect;
    }
    let mut m = res.roll.multiplier;
    if m > 1.0 {
        m += atk.crit_damage_bonus;
    }
    res.roll.multiplier = m;
    atk.damage.iter_mut().for_each(|d| d.blunt(m));
    // Shield block.
    if def.block_cooldown_ms <= 0
        && let Some(sh) = def.shield
    {
        let mut v = CombatVars {
            block_chance: sh.chance,
            block_chance_modifier: def.block_modifier,
            probability_to_hit: res.pth,
            ..Default::default()
        };
        let blocked = match atk.kind {
            AttackKind::Melee => {
                let chance = f.melee_block.as_ref().map_or(0.0, |e| v.eval(e)).min(100.0);
                (rng.unit() * 99.0) + 1.0 <= chance
            }
            AttackKind::Ranged if !atk.unblockable => {
                v.block_chance = sh.chance;
                let chance = f.projectile_block.as_ref().map_or(0.0, |e| v.eval(e));
                (rng.unit() * 99.0) + 1.0 <= chance
            }
            _ => false,
        };
        if blocked {
            let amount = sh.amount * ((def.block_amount_modifier * PCT) + 1.0);
            atk.damage
                .iter_mut()
                .for_each(|d| d.shield(f, amount, sh.absorption));
            res.blocked = true;
            let rec = (sh.recovery * 1000.0) as i32 as f32;
            def.block_cooldown_ms = (rec - (def.block_recovery_reduction * PCT) * rec) as i32;
        }
    }
    if def.racial_reduction > 0.0 {
        let p = def.racial_reduction;
        atk.damage.iter_mut().for_each(|d| {
            let t = d.ty();
            d.reduce_damage(t, p)
        });
    }
    // ProcessDefense, then ExecuteDefense: every defense against every
    // damage. Each defense is processed on its own from the defender's cap
    // and debuffs (unchanged since the hit began), so the processed copy is
    // made here and the defender's list stays as built.
    for dfn in def.defenses {
        let mut dfn = dfn.clone();
        dfn.process(
            def.resistance_cap,
            &def.resistance_reduction,
            DamageScale::default(),
            def.armor_reduction,
        );
        for d in &mut atk.damage {
            dfn.execute(d, region);
        }
    }
    // Reflect links (`TakeAttack` @0x18011d950): after
    // `ExecuteDefense` the attack is copied and, with
    // `GetReflectLinkDamageReduction` > 0, every attribute of the copy is
    // scaled by it; for attacks other than retaliation (style 3) and
    // reflection (6) the copy is `FilterReflectDamage`d (as for reflection:
    // no crowd control) and goes to `SkillManager::ReflectLinkDamage`.
    if def.link_absorb > 0.0
        && !matches!(atk.kind, AttackKind::Retaliation | AttackKind::Reflection)
    {
        res.link_copy = atk
            .damage
            .iter()
            .filter(|d| !matches!(d, Damage::Reflex { .. }))
            .cloned()
            .map(|mut d| {
                d.scale(def.link_absorb);
                d
            })
            .collect();
    }
    // ExecuteDamage.
    let life0 = def.life.max(0.0);
    let mut leech = 0.0f32;
    for d in &atk.damage {
        match d {
            // `CombatAttributeAbsDamage_ManaBurn::Execute` @0x1801189c0:
            // burns value % of max energy (at most what is
            // left) and deals ratio % of the burned energy.
            Damage::Hit(h) if h.ty == CombatType::ManaBurn => {
                let burned = (h.value * PCT * def.energy_max).min(def.energy.max(0.0));
                def.energy -= burned;
                res.energy_burned += burned;
                let v = atk.mana_burn_ratio * PCT * burned;
                apply(def, &mut res, CombatType::ManaBurn, v);
            }
            Damage::Hit(h) => apply(def, &mut res, h.ty, h.value),
            Damage::Weapon(w) => {
                if w.physical[0] > 0.0 {
                    let v = physical_damage_defense(
                        f,
                        w.physical[0],
                        w.protection,
                        w.protection_absorption,
                        w.protection_bonus,
                    );
                    apply(def, &mut res, CombatType::Physical, v);
                }
                if w.pierce[0] > 0.0 {
                    apply(def, &mut res, CombatType::Pierce, w.pierce[0]);
                }
            }
            Damage::Dot(dot) => res.dots.push(dot.clone()),
            Damage::LifeLeech { value, .. } => {
                if *value > 0.0 {
                    leech += *value
                }
            }
            Damage::Reflex { ty, secs } => match ty {
                // `Influence` (Execute thresholds there).
                CombatType::Taunt if *secs > 0.0 => res.influence.taunt += *secs,
                CombatType::Convert if *secs >= 1.0 && res.influence.convert == 0.0 => {
                    res.influence.convert = *secs
                }
                CombatType::Disruption => {
                    // `(int)(value × 1000)` ms.
                    let secs = (*secs * 1000.0) as i32 as f32 * 0.001;
                    res.influence.disruption = res.influence.disruption.max(secs);
                }
                CombatType::Taunt | CombatType::Convert => {}
                // `..._Fear::Execute` @0x180118e40 (fear and
                // confusion): at least a full second; other crowd control
                // (`CombatAttributeReflexDamage::Execute` @0x180114ee0) any
                // positive duration.
                _ => {
                    let ok = *secs > 0.0
                        && (!matches!(ty, CombatType::Fear | CombatType::Confusion)
                            || *secs >= 1.0);
                    if ok {
                        res.reflex.push((*ty, *secs));
                    }
                }
            },
            Damage::PercentCurrentLife { value, .. } => {
                let v = (def.life.max(0.0) * *value) * PCT;
                apply(def, &mut res, CombatType::PercentCurrentLife, v)
            }
        }
    }
    res.life_lost = life0 - def.life.max(0.0);
    if leech > 0.0 && res.life_lost > 0.0 {
        res.leeched = (leech * PCT) * res.life_lost;
    }
    res
}

fn apply(def: &mut Defender, res: &mut HitResult, ty: CombatType, v: f32) {
    res.total += v.max(0.0);
    let v = def.absorb.iter_mut().fold(v, |v, a| a.filter(ty, v));
    if v > 0.0 {
        def.life -= v;
        res.applied.push((ty, v));
    }
}

/// One DoT application in the duration-damage manager: `per` damage in each
/// 100 ms slot from the timeline's head at the time it was added up to `end`
/// (absolute slot numbers).
#[derive(Debug, Clone, PartialEq)]
struct DotApp {
    /// The slot key: the owning object + attribute ([`DotDamage::source`]).
    source: u64,
    /// The attacker's entity bits (the hit's `ParametersCombat` +8): the kill
    /// credit and the experience share, not the slot key.
    attacker: u64,
    per: f32,
    /// The hit's to-hit multiplier (`AddDamage`'s last argument).
    multiplier: f32,
    /// The attack's skill (the entry's skills: the kill's `OnEnemyDeath`).
    skill: Option<std::sync::Arc<str>>,
    /// First slot past the application.
    end: u64,
}

/// One source's entry in one slot, as the engine's slot holds it: the
/// largest `per` of the source's applications covering the slot (with the
/// skill of the first application reaching it), and the attacker and
/// multiplier of the first covering application.
#[derive(Debug, Clone, Copy)]
struct SlotEntry {
    source: u64,
    attacker: u64,
    damage: f32,
    multiplier: f32,
    /// The application whose skill the entry carries (index into `apps`).
    skill: usize,
}

/// One damage type's queue.
#[derive(Debug, Clone)]
struct Timeline {
    ty: CombatType,
    /// The next slot to tick.
    head: u64,
    /// Live applications (`end > head`) in insertion order.
    apps: Vec<DotApp>,
}

impl Timeline {
    /// One past the last queued slot.
    fn end(&self) -> u64 {
        self.apps.iter().map(|a| a.end).max().unwrap_or(self.head)
    }

    /// The entries of slot `s` (≥ `head`) in insertion order: a source joins
    /// a slot when its first application covering it was added.
    fn slot(&self, s: u64, out: &mut Vec<SlotEntry>) {
        out.clear();
        for (i, a) in self.apps.iter().enumerate() {
            if a.end <= s {
                continue;
            }
            match out.iter_mut().find(|e| e.source == a.source) {
                Some(e) => {
                    if a.per > e.damage {
                        e.damage = a.per;
                        e.skill = i;
                    }
                }
                None => out.push(SlotEntry {
                    source: a.source,
                    attacker: a.attacker,
                    damage: a.per,
                    multiplier: a.multiplier,
                    skill: i,
                }),
            }
        }
    }
}

/// Longest DoT in 100 ms slots (bad data: an infinite or huge duration would
/// queue billions of ticks; stock durations are seconds).
const MAX_DOT_SLOTS: usize = 6_000;

/// One [`DotManager::update`]: damage per type, the part from one attacker,
/// the largest multiplier among the ticked entries (any source) and the
/// attacker of the last entry that dealt damage.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DotTick {
    pub per_type: Vec<(CombatType, f32)>,
    pub from_source: f32,
    pub multiplier: f32,
    /// `DurationDamageManager::ExecuteDamage` @0x1802796b0: before each entry's `ApplyDamage` the
    /// defender's `CombatManager` attacker (+364, the killer `Character::CharacterIsDying` sends) is set to the
    /// entry's attacker, so after an update it is the last entry that dealt damage. PORT: the engine's entries
    /// are one per (type, attack) in insertion order; here it is the last entry of the last timeline (types in
    /// first-application order) that ticked.
    pub last_source: Option<u64>,
    /// The skill of that entry (`ApplyDamage` @0x18011d340: the killing entry's skills run the
    /// attacker's `OnEnemyDeath`).
    pub last_skill: Option<std::sync::Arc<str>>,
    /// The damage per attacker this update (before the defender's filters; each entry's `ApplyDamage`
    /// registers its own, `GameEngine::RegisterDamage`, the experience share).
    pub by_source: Vec<(u64, f32)>,
}

/// Per-type 100 ms timeline (`DurationDamageManager`:
/// `AddDamage` spreads `dps * 0.1` over `(int)(duration * 10)` slots; in a
/// slot the same source keeps `max(old, new)`, different sources stack —
/// the source is the owning object + attribute, [`DotDamage::source`], not
/// the attacker; `Update(ms)` consumes whole 100 ms ticks, carrying the
/// remainder).
/// `DurationDamageManager::ExecuteDamage` @0x1802796b0 also returns the
/// damage of the entries whose attacker is the main player and the largest
/// multiplier of every ticked slot entry (`sub_1802800A0`): the
/// DoT combat numbers ([`DotTick`]). A slot's entries, sorted, are scaled by
/// gameengine.dbr `damageMagnitude` % by rank (the last value for the rest;
/// `sub_18027FC30`); stock it is just 100, so stacking is full.
///
/// PORT: the manager stores one entry per application ([`DotApp`]) instead
/// of writing it into every slot, and derives a slot's entries when it ticks
/// (same max / stack rules, same entry order).
#[derive(Debug, Clone, Default)]
pub struct DotManager {
    timelines: Vec<Timeline>,
    carry_ms: i32,
    /// Reused slot entries of [`Self::update_from`].
    scratch: Vec<SlotEntry>,
}

impl DotManager {
    /// A DoT of `dps` for `duration`; `source` is the slot key
    /// ([`DotDamage::source`]), `attacker` the entity bits the kill credits.
    pub fn add(&mut self, ty: CombatType, dps: f32, duration: f32, source: u64, attacker: u64) {
        self.add_hit(ty, dps, duration, source, attacker, 1.0, None);
    }

    /// [`Self::add`] with the hit's to-hit multiplier.
    // The engine's `AddDamage` parameters, kept flat (type, value, duration,
    // slot key, attacker, multiplier, skill).
    #[allow(clippy::too_many_arguments)]
    pub fn add_hit(
        &mut self,
        ty: CombatType,
        dps: f32,
        duration: f32,
        source: u64,
        attacker: u64,
        multiplier: f32,
        skill: Option<&str>,
    ) {
        if dps <= 0.0 || duration <= 0.0 {
            return;
        }
        let n = ((duration * 10.0) as i32 as usize).min(MAX_DOT_SLOTS);
        if n == 0 {
            return;
        }
        let per = dps * 0.1;
        let tl = match self.timelines.iter().position(|t| t.ty == ty) {
            Some(i) => &mut self.timelines[i],
            None => {
                self.timelines.push(Timeline {
                    ty,
                    head: 0,
                    apps: Vec::new(),
                });
                self.timelines.last_mut().unwrap()
            }
        };
        let end = tl.head + n as u64;
        // Covered at least as high in every slot already: no slot changes.
        if tl
            .apps
            .iter()
            .any(|a| a.source == source && a.end >= end && a.per >= per)
        {
            return;
        }
        // The same application as the last one, reaching further: the slots
        // past its end get the entry the new one would add, at the same place.
        if let Some(last) = tl.apps.last_mut()
            && last.source == source
            && last.attacker == attacker
            && last.per == per
            && last.multiplier == multiplier
            && last.skill.as_deref() == skill
        {
            last.end = last.end.max(end);
            return;
        }
        tl.apps.push(DotApp {
            source,
            attacker,
            per,
            multiplier,
            skill: skill.map(Into::into),
            end,
        });
    }

    /// Advances `ms`; returns damage per type dealt this update.
    pub fn update(&mut self, ms: i32) -> Vec<(CombatType, f32)> {
        self.update_from(ms, None).per_type
    }

    /// [`Self::update`], also summing what the attacker `source` (entity bits) dealt.
    pub fn update_from(&mut self, ms: i32, source: Option<u64>) -> DotTick {
        let mut t = self.carry_ms + ms;
        let ticks = if t <= 100 { 0 } else { t / 100 };
        if t > 100 {
            t %= 100;
        }
        self.carry_ms = t;
        let mut out = DotTick::default();
        let mut entries = std::mem::take(&mut self.scratch);
        for tl in &mut self.timelines {
            let mut sum = 0.0f32;
            let end = tl.end();
            for _ in 0..ticks {
                if tl.head >= end {
                    break;
                }
                tl.slot(tl.head, &mut entries);
                tl.head += 1;
                for e in &entries {
                    sum += e.damage;
                    if e.damage > 0.0 {
                        out.last_source = Some(e.attacker);
                        out.last_skill = tl.apps[e.skill].skill.clone();
                    }
                    out.multiplier = out.multiplier.max(e.multiplier);
                    if Some(e.attacker) == source {
                        out.from_source += e.damage;
                    }
                    if e.damage > 0.0 {
                        match out.by_source.iter_mut().find(|b| b.0 == e.attacker) {
                            Some(b) => b.1 += e.damage,
                            None => out.by_source.push((e.attacker, e.damage)),
                        }
                    }
                }
            }
            let head = tl.head;
            tl.apps.retain(|a| a.end > head);
            if sum > 0.0 {
                out.per_type.push((tl.ty, sum));
            }
        }
        self.scratch = entries;
        self.timelines.retain(|t| !t.apps.is_empty());
        out
    }

    /// The types with damage queued, in the order their timelines were
    /// created (PORT: the engine's normal duration-damage list keeps one entry
    /// per (type, attack) in insertion order, `DurationDamageManager::AddDamage`
    /// @0x180279e70; the port merges attacks of one type into one timeline,
    /// kept at the first one's place).
    pub fn types(&self) -> impl Iterator<Item = CombatType> + '_ {
        self.timelines.iter().map(|t| t.ty)
    }

    /// Seconds until the last queued damage of `ty` has ticked.
    pub fn remaining(&self, ty: CombatType) -> f32 {
        self.timelines
            .iter()
            .find(|t| t.ty == ty)
            .map_or(0.0, |t| (t.end() - t.head) as f32 * 0.1)
    }

    pub fn is_active(&self, ty: CombatType) -> bool {
        self.timelines.iter().any(|t| t.ty == ty)
    }

    /// Damage of `ty` still to tick in the next second (10 slots of 100 ms): the buff bar's "Damage Every Second"
    /// number (`DurationDamageManager::GetDamageData`'s magnitude).
    pub fn per_second(&self, ty: CombatType) -> f32 {
        self.timelines.iter().find(|t| t.ty == ty).map_or(0.0, |t| {
            let mut entries = Vec::new();
            let mut sum = 0.0;
            for s in t.head..(t.head + 10).min(t.end()) {
                t.slot(s, &mut entries);
                for e in &entries {
                    sum += e.damage;
                }
            }
            sum
        })
    }
}

/// One conversion: (in type, out type, %).
pub type Conversion = (CombatType, CombatType, f32);

/// Damage conversion (`ConversionAttributeAccumulator::Weigh` @0x18017d520 +
/// `CombatAttributeAccumulator::ConvertDamage` @0x180119ed0): each
/// stage's conversions are grouped by input type; a group over 100 % is
/// scaled down to 100 %. The groups run one after the other: every
/// not-yet-converted damage of the input type (Physical also takes the
/// weapon's base damage, Elemental takes Cold, Fire and Lightning) gives each
/// output its % as a new, not-again-convertible damage; the original keeps
/// the rest (so a later group converts from what is left), or goes when
/// nothing is left. Damage over time never converts to Pierce, Chaos or
/// Aether. An Elemental output splits in thirds (cold, fire, lightning, each
/// at % / 3); a converted DoT keeps its duration (the new attribute copies
/// the source); hit damage stays hit damage and DoTs stay DoTs
/// (`ConvertCombatAttributes` @0x1801199f0).
///
/// The % modifier a damage already holds goes along: the piece copies it
/// and both piece and original scale damage and modifier by their share
/// (`ScaleDamage(pct)` then `ScaleModifiers(pct)`,
/// `CombatAttributeAbsDamage::ScaleModifiers` / `CombatAttributeDurDamage::
/// ScaleModifiers` / `CombatAttributeDamage_BasePhysical::ScaleModifiers`:
/// modifier × pct / 100). `Skill::CollectCombatParameters` applies the
/// skill's own modifiers before its conversions, so those are the modifiers
/// that scale here; the character's come after (see `actor::collect_damage`).
/// `stages` run in order (the skill's conversions, then the character's);
/// what one stage converted stays out of the next.
pub fn convert_damage(damage: &mut Vec<Damage>, stages: &[&[Conversion]]) {
    let mut converted = Vec::new();
    for convs in stages {
        for (in_t, outs) in weigh(convs) {
            convert_group(damage, in_t, &outs, &mut converted);
        }
    }
    damage.extend(converted);
}

/// `ConversionAttributeAccumulator::Weigh`: conversions summed per input and
/// output type, each input's group scaled down to 100 % (fractions of 1).
fn weigh(convs: &[Conversion]) -> Vec<(CombatType, Vec<(CombatType, f32)>)> {
    let mut groups: Vec<(CombatType, Vec<(CombatType, f32)>)> = Vec::new();
    for &(i, o, pct) in convs {
        match groups.iter_mut().find(|g| g.0 == i) {
            Some(g) => match g.1.iter_mut().find(|x| x.0 == o) {
                Some(x) => x.1 += pct * PCT,
                None => g.1.push((o, pct * PCT)),
            },
            None => groups.push((i, vec![(o, pct * PCT)])),
        }
    }
    for g in &mut groups {
        let total: f32 = g.1.iter().map(|x| x.1).sum();
        if total > 1.0 {
            g.1.iter_mut().for_each(|x| x.1 *= 1.0 / total);
        }
    }
    groups
}

/// One input type's `ConvertDamage` over the not-yet-converted `damage`.
fn convert_group(
    damage: &mut Vec<Damage>,
    in_t: CombatType,
    outs: &[(CombatType, f32)],
    converted: &mut Vec<Damage>,
) {
    use CombatType as T;
    let matches = |ty: T| {
        ty == in_t
            || (in_t == T::Physical && ty == T::PhysicalPierce)
            || (in_t == T::Elemental && matches!(ty, T::Cold | T::Fire | T::Lightning))
    };
    damage.retain_mut(|d| {
        let ty = match &*d {
            Damage::Hit(h) => h.ty,
            Damage::Weapon(_) => T::PhysicalPierce,
            Damage::Dot(x) => x.ty,
            _ => return true,
        };
        if !matches(ty) {
            return true;
        }
        let is_dot = matches!(d, Damage::Dot(_));
        let mut left = 1.0f32;
        for &(o, f) in outs {
            if (is_dot && matches!(o, T::Pierce | T::Chaos | T::Aether)) || f <= 0.0 {
                continue;
            }
            left -= f;
            let elemental = [
                (T::Cold, f / 3.0),
                (T::Fire, f / 3.0),
                (T::Lightning, f / 3.0),
            ];
            let single = [(o, f)];
            let split: &[(T, f32)] = if o == T::Elemental {
                &elemental
            } else {
                &single
            };
            for &(o, f) in split {
                match &*d {
                    Damage::Hit(h) => converted.push(Damage::Hit(HitDamage {
                        ty: o,
                        value: h.value * f,
                        min: h.min * f,
                        max: h.max * f,
                        modifier: h.modifier * f,
                        scale_by_attributes: h.scale_by_attributes,
                    })),
                    Damage::Weapon(w) => converted.push(Damage::Hit(HitDamage {
                        ty: o,
                        value: w.physical[0] * f,
                        min: w.physical[1] * f,
                        max: w.physical[2] * f,
                        modifier: w.physical_modifier * f,
                        scale_by_attributes: w.scale_by_attributes,
                    })),
                    Damage::Dot(x) => {
                        let mut n = x.clone();
                        n.ty = o;
                        n.dps = x.dps.map(|v| v * f);
                        n.modifier = x.modifier * f;
                        converted.push(Damage::Dot(n));
                    }
                    _ => {}
                }
            }
        }
        if left > 1e-4 {
            match d {
                Damage::Hit(h) => {
                    h.value *= left;
                    h.min *= left;
                    h.max *= left;
                    h.modifier *= left;
                }
                Damage::Weapon(w) => {
                    w.physical.iter_mut().for_each(|v| *v *= left);
                    w.physical_modifier *= left;
                }
                Damage::Dot(x) => {
                    x.dps.iter_mut().for_each(|v| *v *= left);
                    x.modifier *= left;
                }
                _ => {}
            }
            true
        } else {
            false
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f() -> CombatFormulas {
        CombatFormulas::stock()
    }

    #[test]
    fn pth_formula() {
        // OA == DA: (3.5/4.5*300*0.3) + (100*0.7) - 50 = 90.
        let p = probability_to_hit(&f(), 1000.0, 1000.0);
        assert!((p - 90.0).abs() < 1e-3, "{p}");
        // Large DA clamps to pthMinimum.
        assert_eq!(probability_to_hit(&f(), 100.0, 3000.0), 55.0);
        // Level-1 vs level-1 (155/155).
        assert!((probability_to_hit(&f(), 155.0, 155.0) - 90.0).abs() < 1e-3);
    }

    #[test]
    fn crit_tiers() {
        let f = f();
        // pth 90: roll = 100*rand; miss above 90.
        assert_eq!(damage_modifier(&f, 90.0, 0.95).multiplier, 0.0);
        assert_eq!(
            damage_modifier(&f, 90.0, 0.85),
            HitRoll {
                multiplier: 1.0,
                tier: 0
            }
        );
        // pth 140 (never misses): roll = 140*rand.
        assert_eq!(
            damage_modifier(&f, 140.0, 0.99),
            HitRoll {
                multiplier: 1.5,
                tier: 5
            }
        ); // 138.6 > 135
        assert_eq!(
            damage_modifier(&f, 140.0, 0.9),
            HitRoll {
                multiplier: 1.3,
                tier: 3
            }
        ); // 126 > 120
        assert_eq!(
            damage_modifier(&f, 140.0, 0.5),
            HitRoll {
                multiplier: 1.0,
                tier: 0
            }
        ); // 70 <= 90
        // pth <= 70: scaled by pth/70.
        assert_eq!(damage_modifier(&f, 60.0, 0.1).multiplier, 60.0 / 70.0);
        assert_eq!(critical_chance(&f, 100.0), 0.1);
    }

    #[test]
    fn resistance_reduction_and_cap() {
        let mut d = Defense::Resistance {
            ty: CombatType::Fire,
            value: 90.0,
            modifier: 0.0,
            cap_bonus: 0.0,
            restricted: false,
        };
        d.process(80.0, &ReductionInfo::default(), DamageScale::default(), 0.0);
        assert_eq!(
            d,
            Defense::Resistance {
                ty: CombatType::Fire,
                value: 80.0,
                modifier: 0.0,
                cap_bonus: 0.0,
                restricted: false
            }
        );
        // RR: 50% of |res| plus elemental flat 20 (elemental beats total).
        let mut d = Defense::Resistance {
            ty: CombatType::Fire,
            value: 40.0,
            modifier: 0.0,
            cap_bonus: 0.0,
            restricted: false,
        };
        let rr = ReductionInfo {
            total_pct: 50.0,
            elemental_abs: 20.0,
            total_abs: 5.0,
            ..Default::default()
        };
        d.process(80.0, &rr, DamageScale::default(), 0.0);
        let Defense::Resistance { value, .. } = d else {
            unreachable!()
        };
        assert_eq!(value, 0.0); // 40 - (20 + 20)
        // A restricted resistance (stun) ignores the reduction debuffs.
        let mut d = Defense::Resistance {
            ty: CombatType::Stun,
            value: 40.0,
            modifier: 0.0,
            cap_bonus: 0.0,
            restricted: true,
        };
        d.process(80.0, &rr, DamageScale::default(), 0.0);
        let Defense::Resistance { value, .. } = d else {
            unreachable!()
        };
        assert_eq!(value, 40.0);
        // Negative resistance amplifies damage.
        let mut dm = Damage::Hit(HitDamage {
            ty: CombatType::Fire,
            value: 100.0,
            min: 0.0,
            max: 0.0,
            modifier: 0.0,
            scale_by_attributes: false,
        });
        dm.reduce_damage(CombatType::Fire, -25.0);
        let Damage::Hit(h) = dm else { unreachable!() };
        assert_eq!(h.value, 125.0);
    }

    #[test]
    fn armor_formula() {
        let fm = f();
        // 100 armor at 70% absorption: protection >= damage: 50 * 0.3.
        let pa = 70.0 * PCT * 100.0;
        assert!((physical_damage_defense(&fm, 50.0, 100.0, pa, 0.0) - 15.0).abs() < 1e-4);
        // damage 300 > 100 protection: 100*0.3 + 200.
        assert!((physical_damage_defense(&fm, 300.0, 100.0, pa, 0.0) - 230.0).abs() < 1e-3);
        assert_eq!(physical_damage_defense(&fm, 300.0, 0.0, 0.0, 0.0), 300.0);
    }

    #[test]
    fn dot_timeline() {
        let mut m = DotManager::default();
        m.add(CombatType::Fire, 100.0, 1.0, 1, 1);
        m.add(CombatType::Fire, 50.0, 1.0, 1, 1); // same source: max per slot
        m.add(CombatType::Fire, 20.0, 0.5, 2, 2); // other source stacks
        assert_eq!(m.update(100), vec![]); // exactly 100 ms is not yet a tick
        let d = m.update(150); // 250 ms -> 2 ticks, 50 carried
        assert_eq!(d, vec![(CombatType::Fire, 2.0 * (10.0 + 2.0))]);
        let d = m.update(1000);
        assert!((d[0].1 - (8.0 * 10.0 + 3.0 * 2.0)).abs() < 1e-3, "{d:?}");
        assert!(!m.is_active(CombatType::Fire));
    }

    #[test]
    fn full_hit() {
        let fm = f();
        let mut atk = Attack {
            kind: AttackKind::Direct,
            offensive_ability: 2000.0,
            fumble_chance: 0.0,
            projectile_fumbled: false,
            unblockable: false,
            crit_damage_bonus: 0.0,
            energy_cost: 0.0,
            mana_burn_ratio: 0.0,
            damage: vec![
                Damage::Hit(HitDamage {
                    ty: CombatType::Fire,
                    value: 100.0,
                    min: 0.0,
                    max: 0.0,
                    modifier: 0.0,
                    scale_by_attributes: false,
                }),
                Damage::LifeLeech {
                    value: 10.0,
                    min: 10.0,
                    max: 10.0,
                },
            ],
        };
        let mut def = Defender {
            defensive_ability: 100.0,
            dodge: 0.0,
            deflect: 0.0,
            defenses: &[Defense::Resistance {
                ty: CombatType::Fire,
                value: 25.0,
                modifier: 0.0,
                cap_bonus: 0.0,
                restricted: false,
            }],
            resistance_cap: 80.0,
            resistance_reduction: ReductionInfo::default(),
            armor_reduction: 0.0,
            damage_taken_pct: 100.0,
            reflection_resist: 0.0,
            reflect: 0.0,
            shield: None,
            absorb: Vec::new(),
            link_absorb: 0.0,
            block_cooldown_ms: 0,
            block_recovery_reduction: 0.0,
            block_modifier: 0.0,
            block_amount_modifier: 0.0,
            racial_reduction: 0.0,
            energy_absorption: 0.0,
            energy: 0.0,
            energy_max: 0.0,
            life: 1000.0,
        };
        let r = take_attack(&fm, &mut atk, &mut def, &mut Rng(12345));
        assert_eq!(r.miss, None);
        assert!(r.pth >= 135.0);
        let m = r.roll.multiplier;
        let expect = (m * 100.0) * 0.75;
        assert!((r.applied[0].1 - expect).abs() < 1e-3, "{r:?}");
        assert!((def.life - (1000.0 - expect)).abs() < 1e-3);
        assert!((r.leeched - expect * 0.1).abs() < 1e-3);
    }
}
