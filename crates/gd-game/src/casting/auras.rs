use super::*;

/// The player's damage toggles (`Skill_BuffAttackRadiusToggled`, learned or
/// a worn item's, `crate::player` docs: item toggles) as its
/// [`DamageAuras`] while they are on (`Loadout::skill_active`); a timed aura
/// keeps running. An aura that stays on keeps its tick timer.
pub(super) fn player_damage_toggles(
    mut commands: Commands,
    data: Res<GameData>,
    mut q: Query<
        (
            Entity,
            &Loadout,
            &gd_game_core::player::EffectiveSkills,
            Option<&mut DamageAuras>,
        ),
        (
            With<Player>,
            Or<(
                Changed<Loadout>,
                Changed<gd_game_core::player::EffectiveSkills>,
            )>,
        ),
    >,
) {
    for (e, l, eff, auras) in &mut q {
        let want: Vec<(String, usize)> = l
            .skills_and_item_toggles(&data.db)
            .into_iter()
            .filter(|(s, lv)| {
                *lv > 0
                    && data
                        .db
                        .get(s)
                        .is_some_and(|r| r.class == "Skill_BuffAttackRadiusToggled")
                    && l.skill_active(&data.db, s)
            })
            .map(|(s, _)| {
                let li = l.skill_level(&data.db, eff, &s).max(1) as usize - 1;
                (s, li)
            })
            .collect();
        let toggle = |a: &DamageAura| {
            a.left.is_none()
                && data
                    .db
                    .get(&a.skill)
                    .is_some_and(|r| r.class == "Skill_BuffAttackRadiusToggled")
        };
        let have: Vec<(String, usize)> = auras
            .as_ref()
            .map(|a| {
                a.0.iter()
                    .filter(|a| toggle(a))
                    .map(|a| (a.skill.clone(), a.level_index))
                    .collect()
            })
            .unwrap_or_default();
        if have == want {
            continue;
        }
        let new: Vec<DamageAura> = want
            .iter()
            .filter(|w| !have.contains(w))
            .filter_map(|(s, li)| data.db.get(s).map(|r| DamageAura::new(r, s, *li, false)))
            .collect();
        match auras {
            Some(mut a) => {
                a.0.retain(|x| {
                    !toggle(x) || want.iter().any(|w| w.0 == x.skill && w.1 == x.level_index)
                });
                a.0.extend(new);
            }
            None => {
                commands.entity(e).try_insert(DamageAuras(new));
            }
        }
    }
}

pub(super) fn damage_auras(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &GlobalTransform, &mut DamageAuras), Without<Dead>>,
    all: Query<
        (
            Entity,
            &GlobalTransform,
            &Combatant,
            crate::combat::VictimSide,
        ),
        Without<Dead>,
    >,
    sides: Query<crate::combat::SideData>,
    players: Query<(), With<Player>>,
    mut hits: MessageWriter<SkillHit>,
) {
    let dt = time.delta_secs();
    // The bodies, indexed at the first pass (`combat::grid`: candidates in `all`'s order, so the target list and
    // the shuffle's draws over it are the full scan's).
    let mut bodies = None;
    let mut near = Vec::new();
    for (e, gt, mut auras) in &mut q {
        auras.0.retain_mut(|a| {
            a.left.is_none_or(|l| {
                a.left = Some(l - dt);
                l - dt > 0.0
            })
        });
        for a in &mut auras.0 {
            a.timer -= dt;
            if a.timer > 0.0 {
                continue;
            }
            a.timer += a.interval;
            let side = crate::combat::Side::lookup(&sides, e, a.hostile);
            let (list, grid) = &*bodies.get_or_insert_with(|| {
                let list: Vec<_> = all.iter().collect();
                let grid = crate::combat::grid::BodyGrid::new(
                    list.iter()
                        .map(|(_, t, c, _)| c.body_box(&t.affine()))
                        .collect(),
                );
                (list, grid)
            });
            let at = gt.translation();
            grid.near(at, a.radius, &mut near);
            // `Combatant::in_radius` on the indexed box.
            let mut targets: Vec<(Entity, Vec3)> = near
                .iter()
                .filter(|&&i| {
                    let (t, _, _, v) = &list[i];
                    *t != e && side.hits(v) && grid.body(i).meets_sphere(at.to_array(), a.radius)
                })
                .map(|&i| (list[i].0, list[i].1.translation()))
                .collect();
            if let Some(n) = a.max_targets {
                gd_game_core::rng::shuffle(&mut targets);
                targets.truncate(n);
            }
            for (t, at) in targets {
                hits.write(SkillHit {
                    attacker: e,
                    target: t,
                    skill: a.skill.clone(),
                    level_index: a.level_index,
                    parent: None,
                    scale: 1.0,
                    projectile: false,
                });
                // `SkillActivated::CameraShake` per bolt: `Player::SendCameraShakeEvent` is the player's
                // own shake, a monster's / pet's `Actor` event shakes only a camera that sees it.
                if let Some((secs, amp)) = a.shake {
                    commands.write_message(gd_game_core::visuals::CameraShake {
                        ms: (secs * 1000.0).trunc(),
                        amp,
                        at: gt.translation(),
                        own: players.contains(e),
                    });
                }
                if let Some(l) = &a.bolt {
                    commands.spawn(gd_game_core::visuals::PendingBolt {
                        record: l.clone(),
                        from: gt.translation() + Vec3::Y * 1.5,
                        to: at + Vec3::Y,
                    });
                }
            }
        }
    }
}

/// The life / energy part of a skill's `GAME::Bonus`, as the skill bonuses fill it from the profile
/// (`GetSkillModifierData`: the record at the level plus its modifiers') and give it with
/// `Character::TakeBonus(bonus, slow = false)`: `Skill_BuffRadius::TargetResult` (the
/// activation's pass), `Skill_BuffOther::TargetResult`, `Skill_GiveBonus::TargetResult`
/// and `Skill_BuffSelfToggled::ReceiveActiveUpdate` (a self buff going on) copy
/// `skillLifeBonus`, `skillLifePercent`, `skillLifePercentSlow`, `skillManaBonus`,
/// `skillManaPercent`, `skillManaPercentSlow` (as loaded by `SkillProfile_Modifiers::Load`)
/// into `Bonus.life` … `Bonus.manaPercentSlow`. A held buff's periodic one is [`Bonus::buff_duration`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bonus {
    pub life: f32,
    pub life_pct: f32,
    pub life_slow: f32,
    pub mana: f32,
    pub mana_pct: f32,
    pub mana_slow: f32,
}

impl Bonus {
    /// The skill bonus from the profile's fields (`f`: the record's value at the level plus its modifiers').
    pub fn of(f: impl Fn(&str) -> f32) -> Self {
        Self {
            life: f("skillLifeBonus"),
            life_pct: f("skillLifePercent"),
            life_slow: f("skillLifePercentSlow"),
            mana: f("skillManaBonus"),
            mana_pct: f("skillManaPercent"),
            mana_slow: f("skillManaPercentSlow"),
        }
    }

    /// [`Bonus::of`] for record `rec` at `li` with the modifier records `mods`.
    pub fn of_record(
        db: &gd_formats::vfs::Database,
        rec: &Record,
        li: usize,
        mods: &[(String, usize)],
    ) -> Self {
        let mods: Vec<(Record, usize)> = mods
            .iter()
            .filter_map(|(m, l)| db.get_owned(m).map(|r| (r, *l)))
            .collect();
        Self::of(|k| level_f(rec, k, li) + mods.iter().map(|(r, l)| level_f(r, k, *l)).sum::<f32>())
    }

    /// A held buff's bonus every 1000 ms: `SkillBuff::Update` (a buff another character put
    /// here, `SkillBuff.bonusTimer`, 0 at creation so the first comes at once) and `Skill_BuffSelfToggled::Update`
    /// (a self buff while on, `Skill_BuffSelfToggled.bonusTimer`, zeroed by `ReceiveActiveUpdate`) give the profile's
    /// `skillLifeBonusBuffDuration` and `skillLifePercentBuffDuration` as `Bonus.life` / `Bonus.lifePercent` (flat,
    /// %), when either is non-zero.
    pub fn buff_duration(f: impl Fn(&str) -> f32) -> Self {
        Self {
            life: f("skillLifeBonusBuffDuration"),
            life_pct: f("skillLifePercentBuffDuration"),
            ..default()
        }
    }

    pub fn is_zero(&self) -> bool {
        *self == Self::default()
    }

    /// `CharacterBio::TakeBonus` with `slow = false`: life + % of max life × (1 +
    /// healing increase + % healing increase), energy + % of max energy, + the flat energy, life + flat × (1 +
    /// healing increase), + the slow % of max life × (1 + healing increase) and energy + the slow % of max
    /// energy (at once: only `slow = true`, the charge potions', fills the restore pools); each capped at the
    /// maximum. `energy`: the holder's current energy (monsters keep none here).
    pub fn apply(
        &self,
        sheet: &gd_sim::stats::CharacterSheet,
        life: &mut f32,
        energy: Option<&mut f32>,
    ) {
        let (lmax, emax) = (sheet.life, sheet.energy);
        let h = sheet.heal_increase * 0.01;
        *life = (*life + bonus_life(sheet, 0.0, self.life_pct)).min(lmax);
        *life = (*life + self.life * (1.0 + h)).min(lmax);
        *life = (*life + (self.life_slow * 0.01 * lmax * (1.0 + h)).max(0.0)).min(lmax);
        if let Some(e) = energy {
            *e = (*e + self.mana_pct * 0.01 * emax).min(emax);
            *e = (*e + self.mana).min(emax);
            *e = (*e + self.mana_slow * 0.01 * emax).min(emax);
        }
    }

    /// [`Bonus::apply`] on `who` (its `Combatant` life, its `Caster` / `Pet` energy); nothing on the dead.
    pub fn apply_to(&self, w: &mut World, who: Entity) {
        if self.is_zero() || w.get::<Dead>(who).is_some() {
            return;
        }
        let mut energy = w
            .get::<Caster>(who)
            .map(|k| k.energy)
            .or_else(|| w.get::<crate::pets::Pet>(who).map(|p| p.energy));
        let Some(mut c) = w.get_mut::<Combatant>(who) else {
            return;
        };
        let sheet = c.profile.sheet.clone();
        self.apply(&sheet, &mut c.life, energy.as_mut());
        debug!("bonus on {who}: {self:?} -> life {:.0}", c.life);
        if let Some(e) = energy {
            if let Some(mut k) = w.get_mut::<Caster>(who) {
                k.energy = e;
            } else if let Some(mut p) = w.get_mut::<crate::pets::Pet>(who) {
                p.energy = e;
            }
        }
    }

    /// [`Bonus::apply_to`] once the frame's commands run.
    pub fn queue(self, commands: &mut Commands, who: Entity) {
        if !self.is_zero() {
            commands.queue(move |w: &mut World| self.apply_to(w, who));
        }
    }
}

/// The player's switched-on toggles at their effective level ([`self_buff_heals`]; `effective` the
/// player's [`gd_game_core::player::EffectiveSkills`]).
fn loadout_toggles(
    db: &gd_formats::vfs::Database,
    l: &Loadout,
    effective: &gd_sim::skills::SkillBook,
) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for (s, lv) in l.skills_and_item_toggles(db) {
        if lv > 0 && l.skill_active(db, &s) {
            let li = l.skill_level(db, effective, &s).max(1) as usize - 1;
            out.push((s, li));
        }
    }
    out
}

/// `Skill_BuffSelfToggled::Update`: while a self buff is on, every 1000 ms (the
/// first in the update after it went on, `ReceiveActiveUpdate` zeroes the timer) its holder takes the
/// profile's [`Bonus::buff_duration`] (the record with the caster's modifiers of it). The running self buffs:
/// the player's timed buffs and switched-on self toggles, a monster's / pet's timed buffs and toggles.
#[allow(clippy::type_complexity)]
pub(super) fn self_buff_heals(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut q: Query<
        (
            Entity,
            Option<Ref<Loadout>>,
            Option<Ref<gd_game_core::player::EffectiveSkills>>,
            Option<&gd_game_core::weapon_pool::SkillLinks>,
            Option<&crate::monster_skills::MonsterSkills>,
            Option<&crate::pets::Pet>,
            Option<&mut SelfBuffHeals>,
            Option<&mut SelfBuffToggles>,
        ),
        (
            Without<Dead>,
            With<Combatant>,
            Or<(
                With<Caster>,
                With<Loadout>,
                With<crate::monster_skills::MonsterSkills>,
                With<crate::pets::Pet>,
            )>,
        ),
    >,
) {
    let dt = time.delta_secs();
    let db = &data.db;
    for (e, loadout, effective, links, ms, pet, heals, mut toggle_cache) in &mut q {
        // The player's switched-on toggles are cached (the book clone and item
        // scan run only when the loadout changed); the timed buffs and a
        // monster's / pet's toggles are read fresh.
        let fresh_toggles: Vec<(String, usize)>;
        let loadout_on: &[(String, usize)] = match (loadout.as_deref(), effective.as_deref()) {
            (Some(l), Some(eff)) => {
                let stale = toggle_cache.as_deref().is_none_or(|_| {
                    loadout.as_ref().is_some_and(|r| r.is_changed())
                        || effective.as_ref().is_some_and(|r| r.is_changed())
                });
                if stale {
                    fresh_toggles = loadout_toggles(db, l, eff);
                    match toggle_cache.as_mut() {
                        Some(c) => c.0 = fresh_toggles.clone(),
                        None => {
                            commands
                                .entity(e)
                                .try_insert(SelfBuffToggles(fresh_toggles.clone()));
                        }
                    }
                    &fresh_toggles
                } else {
                    &toggle_cache
                        .as_deref()
                        .expect("a present cache when fresh")
                        .0
                }
            }
            _ => &[],
        };
        let mut on: Vec<(String, usize)> = Vec::new();
        let timed = loadout
            .as_deref()
            .map(|l| l.buffs.list.as_slice())
            .or(ms.map(|m| m.buffs.list.as_slice()))
            .or(pet.map(|p| p.buffs()))
            .unwrap_or_default();
        on.extend(
            timed
                .iter()
                .filter(|b| b.2 > 0.0)
                .map(|b| (b.0.clone(), b.1)),
        );
        on.extend(loadout_on.iter().cloned());
        on.extend(
            ms.map(|m| m.toggled())
                .or(pet.map(|p| p.toggled()))
                .unwrap_or_default()
                .iter()
                .cloned(),
        );
        // A buff record another character put in the timed list (a monster's buff-other on the player) is a
        // `SkillBuff` (`SkillBuff::Update`, the same bonus); so is the player's own radius buff, which the
        // engine's pass puts on the player too (`crate::auras`: the port keeps it here).
        on.retain(|(s, _)| {
            db.get(s).is_some_and(|r| {
                is_self_buff_class(&r.class)
                    || r.class.starts_with("SkillBuff")
                    || (loadout.is_some() && r.class.starts_with("Skill_BuffRadius"))
            })
        });
        let Some(mut heals) = heals else {
            if !on.is_empty() {
                commands.entity(e).try_insert(SelfBuffHeals::default());
            }
            continue;
        };
        heals.0.retain(|(s, _)| on.iter().any(|o| &o.0 == s));
        for (s, li) in on {
            let t = match heals.0.iter_mut().find(|h| h.0 == s) {
                Some((_, t)) => {
                    *t -= dt;
                    t
                }
                None => {
                    heals.0.push((s.clone(), 0.0));
                    &mut heals.0.last_mut().expect("just pushed").1
                }
            };
            if *t > 0.0 {
                continue;
            }
            *t = 1.0;
            let Some(rec) = db.get(&s).map(|r| {
                gd_sim::skills::profile_name(r)
                    .and_then(|b| db.get(b))
                    .unwrap_or(r)
            }) else {
                continue;
            };
            let mods = links.map_or(&[][..], |l| l.mods_of(&s));
            let mods: Vec<(Record, usize)> = mods
                .iter()
                .filter_map(|(m, l)| db.get_owned(m).map(|r| (r, *l)))
                .collect();
            Bonus::buff_duration(|k| {
                level_f(rec, k, li) + mods.iter().map(|(r, l)| level_f(r, k, *l)).sum::<f32>()
            })
            .queue(&mut commands, e);
        }
    }
}

/// Timed filters run out ([`Shield::left`]).
pub(super) fn shields(time: Res<Time>, mut q: Query<&mut Shields>) {
    let dt = time.delta_secs();
    for mut s in &mut q {
        if s.0.iter().all(|s| s.left.is_none()) {
            continue;
        }
        for s in &mut s.0 {
            if let Some(l) = s.left.as_mut() {
                *l -= dt;
            }
        }
        s.0.retain(|s| s.left.is_none_or(|l| l > 0.0));
    }
}
