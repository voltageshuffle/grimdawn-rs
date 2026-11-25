use super::*;

/// Death and the no-action states end what a character's skills have running:
/// - `SkillManager::ImDead`: the current skill is cleared and every active-list and item-list
///   skill gets `Skill::StopSkill` (a channel stops, a charge is cancelled: no hit).
/// - `TakeStunAction::Execute` (sleep, knockdown, immobilize) → `SkillManager::StopCurrentSkill`:
///   a running current skill gets `StopSkill`; a charge cut short keeps what it paid.
///
/// PORT: a dead character's pending [`Casting`] / ally cast would go off on the death
/// animation's `End`, so it ends here, the frame after the character was marked.
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

#[derive(Component)]
pub struct Projectile {
    pub(crate) attacker: Entity,
    /// Its record (the impact sounds).
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
    /// Its caster's side, kept for when the caster is gone.
    pub(crate) side: Option<crate::combat::Side>,
    /// `projectileFragmentsName` with `...LaunchNumberMin/Max`: fragments fly off level with
    /// the ground, 360° / count apart plus ±30° jitter (`ProjectileBase::ProcessFragmentation`).
    pub(super) fragments: Vec<(String, u32, u32)>,
    /// Never hits this entity (a fragment's parent target).
    pub(super) exclude: Option<Entity>,
    /// Chance (%) to pierce a target and fly on (`projectilePiercing`).
    pub(crate) pierce: f32,
    pub(super) passed: Vec<Entity>,
    pub(crate) parent: Option<(String, usize)>,
    /// Where it was launched (range falloff).
    pub(crate) origin: Vec3,
    /// A `Skill_AttackProjectileDebuf`'s buff, landed on what it hits instead of an attack.
    pub(super) debuf: Option<ProjectileDebuf>,
    /// The ray radius it hits with: `actorRadius` × (1 + growth / 2); a target counts when the
    /// ray from the last position passes within its bounding box
    /// (`ProjectileBase::GetTargetsAlongPath`).
    pub(crate) touch: f32,
    /// `explodeOnMiss`: stopped by the world (or at the end of its flight) it still explodes.
    pub(crate) explode_on_miss: bool,
    /// Its skill is a `Skill_AttackProjectileSpawnPet`: its end summons.
    pub(crate) spawns_pet: bool,
    /// A `Skill_AttackProjectileChain`'s spark list, collected at launch: the impact attacks
    /// every entry (`Skill_AttackProjectileChain::TargetResult`).
    pub(crate) chain: Vec<Entity>,
}

impl Projectile {
    /// Attacker, hostile, record: what `ProjectileBase::NotifyMonsters` reads.
    pub fn owner(&self) -> (Entity, bool, &str) {
        (self.attacker, self.hostile, &self.record)
    }
}

pub(super) fn init_caster(
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    players: Query<Entity, (With<Player>, Without<Caster>)>,
    // Option: a new character has no `LoadedCharacter` (empty hotbar).
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    data: Res<GameData>,
) {
    for e in &players {
        let mut slots: [Option<String>; SLOTS] = Default::default();
        let page = |bar: &gd_formats::save::gdc::HotSlotBar| {
            let mut p: [Option<String>; SLOTS] = Default::default();
            for (i, s) in bar.slots.iter().take(SLOTS).enumerate() {
                // The potion / evade / scroll options as their records.
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
        // Each bar is the page of one skill set (`id`); the player starts in set 0.
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
            // A new character: the hotbar the game gives it.
            slots = page(&gd_formats::save::gdc::HotSlotBar {
                id: 0,
                slots: crate::save::new_hotbar(),
            });
        }
        #[cfg(feature = "dev")]
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
        // Weapon set II active: its mouse slots are the shown ones.
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

/// Keeps [`SpellAttacker`] matching the left-button skill.
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

/// Energy regen: the sheet's `energy_regen_per_sec` (`CharacterBio::GetManaRegenValue`).
pub(super) fn regen_and_cooldowns(time: Res<Time>, mut q: Query<(&mut Caster, &Combatant)>) {
    let dt = time.delta_secs();
    for (mut c, cb) in &mut q {
        // The timers are no change: `Changed<Caster>` is the hot slots'.
        let c = c.bypass_change_detection();
        // Energy is capped at max − switched-on toggles' reserve × (1 − reduction %)
        // (`CharacterBio::TakeBonus`).
        let max = cb.profile.sheet.usable_energy();
        c.energy = (c.energy + cb.profile.sheet.energy_regen_per_sec.max(0.0) * dt).min(max);
        c.cooldowns.tick(dt);
    }
}
