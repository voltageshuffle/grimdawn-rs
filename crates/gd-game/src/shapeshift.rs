//! Shapeshifting (`Skill_Shapeshift`: the Berserker's Wereraven form, the Fangs relic's beast form).
//!
//! The class is a `Skill_BuffSelfToggled` (its stats count while it is on). Casting the skill toggles the form
//! ([`Shapeshifted`]); while on, the character's active skill set is `activeSkillSet` (skills of another set
//! are unavailable, `Skill::SetAvailability`) and the hotbar shows that set's page (`PlayerHotSlotCtrl::Update`:
//! created on first use by [`new_page`], kept and saved as its own bar). `grantedSkills` come at the form
//! skill's level (`SkillManager::UpdateSubSkillLevels`); other skills cast only when their class is in
//! `allowedSkillTypes` (none listed = all, compared without case: `SkillManager::IsGlobalSkillTypeAndAllowed`);
//! `skillActiveManaCost` drains per second and the form ends without energy
//! (`SkillActivatedBuffSelf::ApplyActiveCosts`).
//!
//! The look ([`wear_form`], `SetShapeshifted`, when the cast fires):
//! - going in: `Character::SetOverrideMesh` with [`replacement_mesh`] (scaled like the character),
//!   `SetOverrideAnimations` with `replacementAnims` (only its `unarmed*` keys,
//!   `Character::LoadUnarmedAnimations`), `SetHideEquipment(true)` (attached `ItemEquipment` is not drawn;
//!   other attachments stay), `ClearAnimationCallbacks`, then the form's Transform animation (type 49) at the
//!   spell-cast speed; `SetOverrideSounds(replacementSounds)`, `SetOverrideFootsteps(replacementFootsteps)` and
//!   `SetUnarmedWeaponTrail(attackTrail)` ([`FormRecords`]).
//! - coming out: all cleared (the current run or idle restarts, no Transform), equipment shown again.
//! - no collision or radius change; attach points are the form mesh's (`GetMeshInstance` returns the override).
//! - a player form ends only by the toggle or running out of energy; `Character::CharacterIsDying` stops no
//!   skill, so a form dies with its own Die animation.
//!
//! Timed monster forms (`skillActiveDuration` > 0), `Skill_Shapeshift::ActivateNow`: a recast while it runs
//! zeroes `timeLeft`, a first cast sets it to `skillActiveDuration` and sends the active update. `Update`
//! counts it down; at 0 the form waits ([`expire_monster_forms`]): the first time it reads the controller's
//! state skill (`stateSkill`): none or a `SkillChanneled` (force-ended) makes it ready (`readyToEnd`), another
//! skill makes it wait; on later frames it is ready unless the action state (`Character::GetActionState`) is
//! Attack (7) or 20, or Move (5) / Jump (19) / Evade (21) with the state still holding a skill. Ready and not
//! frozen / petrified, the form ends (the inactive update, `PrimaryStopSecondarySkills`); else it retries next
//! frame. PORT: [`crate::monster_skills::MonsterSkills::has_pick`] is the state's skill, `Attack` / `Cast` the
//! attack action states, `Run` / `Charge` / `Evade` the moving ones; no monster channels. In the form,
//! `ChooseBestSkill` skips set 0 skills whose class is not in `allowedSkillTypes`, the plain swing included
//! ([`crate::monster_skills::MonsterSkills::set_active_set`]).
//!
//! Not ported: `Skill_Shapeshift::AddTimeToLive` (only a player's `RefreshSkillDurationController` calls it,
//! and no player form is timed).
//!
//! Deferred swap (`Skill_Shapeshift.deferredSwap`): while the character is in a physics simulation
//! (`Character::ExecuteKnockdown`'s ragdoll) the swap waits until it ends and the character is alive. PORT:
//! only a timed form's ending is deferred ([`DeferredSwap`]): its stats, sound and timer stop at once, the look
//! and the skill set come back after the throw ([`crate::ragdoll::Ragdoll`]).
use crate::monster_skills::MonsterSkills;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_game_core::casting::Caster;
use gd_game_core::character::{Character, FormRecords};
use gd_game_core::creature::BodyCache;
use gd_game_core::data::GameData;
use gd_game_core::monsters::Monster;
use gd_game_core::player::{Loadout, Player, ProfileDirty};
use gd_game_core::shapeshift::ShapeshiftSet;
use gd_game_core::weapon_pool::SkillLinks;

pub struct ShapeshiftPlugin;

impl Plugin for ShapeshiftPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Toggle>().add_systems(
            Update,
            (
                toggle,
                toggle_monster,
                drain,
                expire_monster_forms,
                apply_deferred_swaps,
                wear_form.in_set(ShapeshiftSet::Wear),
            )
                .chain()
                .before(gd_game_core::character::CharacterStage::Drive),
        );
    }
}

#[derive(Component, Debug)]
pub struct Shapeshifted {
    pub skill: String,
    pub granted: Vec<String>,
    /// [`replacement_mesh`], picked as the form starts.
    pub mesh: String,
    pub anims: String,
    pub records: FormRecords,
    drain: f32,
}

/// A shapeshift cast fired: toggles the form.
#[derive(Message, Clone, Debug)]
pub struct Toggle {
    pub caster: Entity,
    pub skill: String,
    pub level_index: usize,
}

fn toggle(
    mut commands: Commands,
    data: Res<GameData>,
    mut msgs: MessageReader<Toggle>,
    mut q: Query<
        (
            &mut Caster,
            &mut Loadout,
            Option<&Shapeshifted>,
            Option<&SkillLinks>,
            &Transform,
        ),
        With<Player>,
    >,
) {
    for m in msgs.read() {
        let Ok((mut caster, mut loadout, cur, links, tf)) = q.get_mut(m.caster) else {
            continue;
        };
        if let Some(s) = cur {
            let same = s.skill.eq_ignore_ascii_case(&m.skill);
            revert(
                &mut commands,
                &data,
                m.caster,
                tf.translation,
                s,
                &mut caster,
                &mut loadout,
            );
            if same {
                continue;
            }
        }
        let Some(rec) = data.db.get(&m.skill) else {
            continue;
        };
        let granted: Vec<String> = rec
            .strs_iter("grantedSkills")
            .map(|s| s.to_string())
            .collect();
        let allowed_classes: Vec<String> = rec
            .strs_iter("allowedSkillTypes")
            .map(|s| s.to_string())
            .collect();
        // The hotbar shows the set's page, created on its first use (`PlayerHotSlotCtrl::Update`).
        let set = rec.i32("activeSkillSet").unwrap_or(0);
        if caster.set_active_set(set) {
            new_page(&data, &mut caster, set, &granted, &allowed_classes);
        }
        let level = m.level_index as u32 + 1;
        loadout
            .granted
            .extend(granted.iter().map(|g| (g.clone(), level)));
        loadout.form = Some((m.skill.clone(), allowed_classes));
        // A timed-buff entry with no end: it leaves with the form.
        loadout.buffs.add(&m.skill, m.level_index, f32::INFINITY);
        let drain = gd_game_core::casting::level_f(rec, "skillActiveManaCost", m.level_index);
        let mods = modifiers(&loadout, links, &m.skill);
        let mesh = replacement_mesh(&data, rec, loadout.female, &mods);
        let anims = rec.str("replacementAnims").unwrap_or_default().to_owned();
        let field = |k: &str| rec.str(k).unwrap_or_default().to_owned();
        let records = FormRecords {
            sounds: field("replacementSounds"),
            footsteps: field("replacementFootsteps"),
            trail: field("attackTrail"),
        };
        info!(
            "shapeshift: {} on ({} granted skills, mesh {mesh})",
            m.skill.rsplit('/').next().unwrap_or(""),
            granted.len()
        );
        commands.entity(m.caster).try_insert((
            Shapeshifted {
                skill: m.skill.clone(),
                granted,
                mesh,
                anims,
                records,
                drain,
            },
            ProfileDirty,
        ));
    }
}

/// A timed form's time left on a monster (`Skill_Shapeshift.timeLeft`) and, once it ran out, the end wait
/// (`stateSkill` seen, `readyToEnd`).
#[derive(Component, Debug)]
pub struct TimedForm {
    pub left: f32,
    seen_skill: bool,
    ready: bool,
}

impl TimedForm {
    /// `skill`: the controller's state holds a skill; `state`: the action state.
    fn end_ready(&mut self, skill: bool, state: gd_game_core::character::CharState) -> bool {
        use gd_game_core::character::CharState as S;
        if !self.ready {
            if !self.seen_skill {
                self.seen_skill = skill;
                self.ready = !skill;
            } else {
                self.ready = match state {
                    S::Attack | S::Cast => false,
                    S::Run | S::Charge | S::Evade => !skill,
                    _ => true,
                };
            }
        }
        self.ready
    }
}

/// A form's end waiting for the knockdown to finish: the look and skill set still the form's.
#[derive(Component, Debug)]
pub struct DeferredSwap;

#[allow(clippy::type_complexity)]
fn toggle_monster(
    mut commands: Commands,
    data: Res<GameData>,
    mut msgs: MessageReader<Toggle>,
    mut q: Query<(&mut MonsterSkills, Option<&mut TimedForm>), (With<Monster>, Without<Player>)>,
) {
    for m in msgs.read() {
        let Ok((mut ms, timed)) = q.get_mut(m.caster) else {
            continue;
        };
        let Some(rec) = data.db.get(&m.skill) else {
            continue;
        };
        if let Some(mut t) = timed {
            t.left = 0.0;
            continue;
        }
        let secs = gd_game_core::casting::level_f(rec, "skillActiveDuration", m.level_index);
        if secs <= 0.0 {
            continue;
        }
        let field = |k: &str| rec.str(k).unwrap_or_default().to_owned();
        let granted = rec
            .strs_iter("grantedSkills")
            .map(|s| s.to_string())
            .collect();
        let mesh = replacement_mesh(&data, rec, false, &[]);
        let records = FormRecords {
            sounds: field("replacementSounds"),
            footsteps: field("replacementFootsteps"),
            trail: field("attackTrail"),
        };
        let allowed: Vec<String> = rec
            .strs_iter("allowedSkillTypes")
            .map(|s| s.to_string())
            .collect();
        ms.set_active_set(&data, rec.i32("activeSkillSet").unwrap_or(0), &allowed);
        ms.set_form_stats(Some((m.skill.clone(), m.level_index)));
        // A recast before a pending swap out was done: `SetShapeshifted(true)` finds the set still on.
        commands.entity(m.caster).try_remove::<DeferredSwap>();
        info!(
            "monster shapeshift: {} for {secs} s",
            m.skill.rsplit('/').next().unwrap_or("")
        );
        commands.entity(m.caster).try_insert((
            Shapeshifted {
                skill: m.skill.clone(),
                granted,
                mesh,
                anims: field("replacementAnims"),
                records,
                drain: 0.0,
            },
            TimedForm {
                left: secs,
                seen_skill: false,
                ready: false,
            },
        ));
    }
}

/// `Skill_Shapeshift::Update`'s countdown and end wait.
#[allow(clippy::type_complexity)]
fn expire_monster_forms(
    mut commands: Commands,
    data: Res<GameData>,
    time: Res<Time>,
    mut q: Query<(
        Entity,
        &mut TimedForm,
        &mut MonsterSkills,
        &Character,
        &Shapeshifted,
        &Transform,
        Has<crate::ragdoll::Ragdoll>,
    )>,
) {
    for (e, mut t, mut ms, ch, s, tf, thrown) in &mut q {
        t.left -= time.delta_secs();
        if t.left > 0.0 {
            continue;
        }
        if !t.end_ready(ms.has_pick(), ch.state) || ch.frozen {
            t.left = 0.0;
            continue;
        }
        if let Some(pak) = crate::combat_sounds::deactivate(&data, &s.skill) {
            commands.write_message(gd_game_core::visuals::PlaySound {
                pak,
                at: Some(tf.translation),
            });
        }
        ms.set_form_stats(None);
        info!(
            "monster shapeshift: {} off",
            s.skill.rsplit('/').next().unwrap_or("")
        );
        commands.entity(e).try_remove::<TimedForm>();
        if thrown {
            commands.entity(e).try_insert(DeferredSwap);
        } else {
            ms.set_active_set(&data, 0, &[]);
            commands.entity(e).try_remove::<Shapeshifted>();
        }
    }
}

/// `Skill_Shapeshift::Update`'s deferred swap: done once the physics simulation is over and the monster lives.
#[allow(clippy::type_complexity)]
fn apply_deferred_swaps(
    mut commands: Commands,
    data: Res<GameData>,
    mut q: Query<
        (Entity, &mut MonsterSkills),
        (
            With<DeferredSwap>,
            Without<gd_game_core::combat::Dead>,
            Without<crate::ragdoll::Ragdoll>,
        ),
    >,
) {
    for (e, mut ms) in &mut q {
        ms.set_active_set(&data, 0, &[]);
        commands
            .entity(e)
            .try_remove::<(Shapeshifted, DeferredSwap)>();
    }
}

/// A new set's page (`PlayerHotSlotCtrl::Update`): the set's first two skills (`SkillManager::GetDefaultSkillIds`)
/// fill the left and right buttons and their alternate-weapon twins; every key slot of the first and second row
/// copies the default page's skill when it may be used in the form (`CloneDefaultSlot`).
fn new_page(
    data: &GameData,
    caster: &mut Caster,
    set: i32,
    granted: &[String],
    allowed: &[String],
) {
    use gd_game_core::casting::{SLOT_LEFT, SLOT_RIGHT, SLOT_ROW2};
    let mut ids = granted
        .iter()
        .filter(|g| data.db.get(g).and_then(|r| r.i32("skillSet")) == Some(set));
    if let Some(a) = ids.next() {
        caster.slots[SLOT_LEFT] = Some(a.clone());
        caster.slots[SLOT_LEFT + 1] = Some(a.clone());
    }
    if let Some(b) = ids.next() {
        caster.slots[SLOT_RIGHT] = Some(b.clone());
        caster.slots[SLOT_RIGHT + 1] = Some(b.clone());
    }
    let Some(default) = caster.sets.get(&0).cloned() else {
        return;
    };
    for i in (0..10).chain(SLOT_ROW2..SLOT_ROW2 + 10) {
        let Some(skill) = &default[i] else { continue };
        let ok = allowed.is_empty()
            || data
                .db
                .get(skill)
                .is_some_and(|r| allowed.iter().any(|a| a.eq_ignore_ascii_case(&r.class)));
        if ok {
            caster.slots[i] = Some(skill.clone());
        }
    }
}

/// The form ends (`Skill_Shapeshift::ReceiveActiveUpdate` going inactive: `skillDeactivatedSound`).
fn revert(
    commands: &mut Commands,
    data: &GameData,
    e: Entity,
    at: Vec3,
    s: &Shapeshifted,
    caster: &mut Caster,
    loadout: &mut Loadout,
) {
    if let Some(pak) = crate::combat_sounds::deactivate(data, &s.skill) {
        commands.write_message(gd_game_core::visuals::PlaySound { pak, at: Some(at) });
    }
    // The default set's page is shown again; the form's page stays for the next time.
    caster.set_active_set(0);
    loadout
        .granted
        .retain(|(g, _)| !s.granted.iter().any(|x| x.eq_ignore_ascii_case(g)));
    loadout.form = None;
    loadout
        .buffs
        .list
        .retain(|b| !b.0.eq_ignore_ascii_case(&s.skill));
    info!(
        "shapeshift: {} off",
        s.skill.rsplit('/').next().unwrap_or("")
    );
    commands
        .entity(e)
        .try_remove::<Shapeshifted>()
        .try_insert(ProfileDirty);
}

fn drain(
    mut commands: Commands,
    data: Res<GameData>,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Caster, &mut Loadout, &Shapeshifted, &Transform), With<Player>>,
) {
    for (e, mut caster, mut loadout, s, tf) in &mut q {
        if s.drain <= 0.0 {
            continue;
        }
        let cost = s.drain * time.delta_secs();
        if caster.energy < cost {
            revert(
                &mut commands,
                &data,
                e,
                tf.translation,
                s,
                &mut caster,
                &mut loadout,
            );
        } else {
            caster.energy -= cost;
        }
    }
}

/// The modifiers of `skill`: the learned ones, then the gear's (`Skill::GetItemSkillModifiers`).
fn modifiers(loadout: &Loadout, links: Option<&SkillLinks>, skill: &str) -> Vec<String> {
    let Some(links) = links else {
        return Vec::new();
    };
    let learned = |m: &str| {
        loadout
            .skills
            .skills
            .iter()
            .any(|(s, lv)| *lv > 0 && s.eq_ignore_ascii_case(m))
    };
    let (mine, gear): (Vec<_>, Vec<_>) = links
        .mods_of(skill)
        .iter()
        .map(|(m, _)| m.clone())
        .partition(|m| learned(m));
    mine.into_iter().chain(gear).collect()
}

/// The form's mesh (`Skill_Shapeshift::GetReplacementMesh`): `replacementMeshFemale` / `…Male`, falling back to
/// `replacementMesh`; then every modifier in `mods` with a `shapeshiftMeshOverrideFemale` / `…Male` replaces it,
/// the last one winning (`Skill_Modifier::GetShapeshiftMeshOverride`).
pub fn replacement_mesh(data: &GameData, rec: &Record, female: bool, mods: &[String]) -> String {
    let g = if female { "Female" } else { "Male" };
    let own = |r: &Record, k: &str| r.str(k).filter(|s| !s.is_empty()).map(str::to_owned);
    let mut mesh = own(rec, &format!("replacementMesh{g}"))
        .or_else(|| own(rec, "replacementMesh"))
        .unwrap_or_default();
    for m in mods {
        if let Some(o) = data
            .db
            .get(m)
            .and_then(|r| own(r, &format!("shapeshiftMeshOverride{g}")))
        {
            mesh = o;
        }
    }
    mesh
}

/// Puts a character's [`Shapeshifted`] look on its [`Character`] and takes it off again. A character rebuilt
/// while shifted gets the look back without the Transform animation. PORT: a form whose mesh does not load
/// keeps the character's own look.
pub fn wear_form(
    data: Res<GameData>,
    mut bodies: ResMut<BodyCache>,
    mut q: Query<(&mut Character, Option<Ref<Shapeshifted>>)>,
) {
    for (mut c, shift) in &mut q {
        let wanted = shift
            .as_ref()
            .map(|s| s.mesh.as_str())
            .filter(|m| !m.is_empty());
        if c.form_mesh() == wanted {
            continue;
        }
        let (Some(s), Some(mesh)) = (shift.as_ref(), wanted) else {
            c.clear_form();
            continue;
        };
        let Some(body) = bodies.get(&data, mesh) else {
            continue;
        };
        let set = data.db.get(&s.anims).map(|t| {
            gd_sim::anim::AnimSet::load(t, gd_sim::equip::HandState::Unarmed, |p| {
                data.vfs.exists(p)
            })
        });
        c.set_form(mesh, body, set, s.records.clone());
        if s.is_added() && c.state != gd_game_core::character::CharState::Death {
            c.play_transform();
        }
    }
}
