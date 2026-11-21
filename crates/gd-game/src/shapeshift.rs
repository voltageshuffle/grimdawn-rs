//! Shapeshifting (`Skill_Shapeshift`: the Berserker's Wereraven form, the
//! Fangs relic's beast form).
//!
//! Game.dll: the class is a `Skill_BuffSelfToggled` (its stats
//! count while it is on); `Skill_Shapeshift::SetShapeshifted` @0x180505a70
//! swaps the mesh (`replacementMeshMale/Female`), animations
//! (`replacementAnims`), sounds, footsteps and unarmed trail, sets the
//! character's active skill set to `activeSkillSet` (skills of another set
//! are unavailable, `Skill::SetAvailability`) and copies `allowedSkillTypes`
//! to the character.
//! The port: casting the skill toggles the form ([`Shapeshifted`]); while on,
//! the whole hotbar swaps to the form's skill set page
//! (`PlayerHotSlotCtrl::Update` sees the character's active skill set change
//! and calls `SetActiveSkillSet`: the page is created on first use, see
//! [`new_page`], kept for later and saved as its own bar; leaving the form
//! shows the default set's page again); `grantedSkills` come at
//! the form skill's level. Their level follows the form skill
//! (`Skill_Shapeshift::SkillLevelChange` → `SkillManager::UpdateSubSkillLevels`),
//! other skills cast only when their class is in
//! `allowedSkillTypes` (none listed = all, class names compared without
//! case: `SkillManager::IsGlobalSkillTypeAndAllowed`),
//! `skillActiveManaCost` drains per second and the form ends without energy
//! (`SkillActivatedBuffSelf::ApplyActiveCosts`).
//!
//! The look ([`wear_form`], `SetShapeshifted` @0x180505a70, called by
//! `ReceiveActiveUpdate` @0x180506f80 as the toggle's state changes, i.e. when
//! the cast fires):
//! - going in: `Character::SetOverrideMesh` with [`replacement_mesh`] (scaled
//!   like the character: `Actor::SetScale` with its scale; the old mesh's
//!   attachments removed, the new one's made), `SetOverrideAnimations` with
//!   `replacementAnims` (only the table's `unarmed*` keys are read,
//!   `sub_180090460`, whatever the weapons), `SetHideEquipment(true)` (+9744:
//!   `Character::AddAttachedEntitiesToScene` @0x180072130 skips every attached
//!   `ItemEquipment`, so armour and weapons are not drawn; other attachments,
//!   e.g. skill effects, stay), `ClearAnimationCallbacks`, then the form's
//!   Transform animation (type 49) at the spell-cast speed. The cast itself
//!   played the Transform of the set worn then (`StartAction`, see
//!   `gd_sim::anim::skill_animation`).
//! - coming out: the override mesh / animations cleared (the current run or
//!   idle restarts, no Transform), equipment shown again.
//! - no collision or radius change (nothing else is touched); hover, targeting,
//!   pets and effect anchors keep the character (the attach points are the
//!   form mesh's: `GetMeshInstance` returns the override).
//! - the form ends only by the toggle (cast again) or running out of energy;
//!   `Character::CharacterIsDying` @0x180068db0 stops no skill, so a form dies
//!   with its own Die animation. `skillActiveDuration` forms (monsters' timed
//!   `ActivateNow` branch) are not ported: no player form has one.
//!
//! - also going in: `SetOverrideSounds(replacementSounds)`,
//!   `SetOverrideFootsteps(replacementFootsteps)` and
//!   `SetUnarmedWeaponTrail(attackTrail)` (kept on the [`Character`] with the
//!   mesh, `FormRecords`; read by `crate::char_sounds` and the renderer's
//!   trails); coming out they are cleared (empty paths).
//!
//! Timed monster forms (`skillActiveDuration` > 0; Kurn berserker, Chthonic
//! Kurn shaman, Scorv, Mogdrogen's nemesis form).
//! `Skill_Shapeshift::ActivateNow` @0x180506ce0: the form is "toggled"
//! (+1520) only when the duration is 0; otherwise a recast while it runs
//! zeroes the remaining time (+1540), a first cast sets it to
//! `skillActiveDuration` × 1000 ms (+1540 and the total +1776) and sends the
//! active update (`ReceiveActiveUpdate` @0x180506f80 → `SetShapeshifted`
//! @0x180505a70, the same for any character, then `StartCooldown` on ending and
//! `ControllerMonster::SkillFailed` @0x18013cae0 so the controller chooses
//! again). `Update` @0x180506a10 counts it down; at 0 the form waits for the
//! skill the monster is using ([`expire_monster_forms`]): the first
//! time it reads the controller's current state skill (state data +8, kept at
//! +1856): none, or a `SkillChanneled` (flagged +1562 = `SkillChanneled::
//! ForceEnd` @0x180529420, its `GetValidTarget` then fails and the channel
//! stops), and the form is ready to end (+1860); another skill makes it wait,
//! and on later frames it is ready unless the character's action state
//! (`Character::GetActionState`, +9396) is Attack (7) or 20, or Move (5) /
//! Jump (19) / Evade (21) with the state still holding a skill. Ready and not
//! frozen / petrified, the form ends (+1536 off, the inactive update,
//! `PrimaryStopSecondarySkills`); else the time stays 0 and it tries again
//! next frame. The port: [`crate::monster_skills::MonsterSkills::has_pick`]
//! is the state's skill, `CharState::Attack` / `Cast` the attack action
//! states, `Run` / `Charge` / `Evade` the moving ones; monsters channel
//! nothing here (no monster beam), so the `ForceEnd` has no target.
//! The form's granted skills are the monster's own: a skill with
//! a `skillSet` other than 0 is usable only in that active set
//! (`Skill::SetAvailability` @0x1804924f0) and `SetShapeshifted` copies the
//! form's `allowedSkillTypes` to the character, which `ChooseBestSkill` reads:
//! in the form the set 0 skills of a class not listed are not chosen, the
//! plain swing (`Skill_WPAttack_BasicAttack`) neither when not listed (no
//! stock form lists it) ([`crate::monster_skills::MonsterSkills::set_active_set`]).
//!
//! Refresh-duration modifiers: `ActivateNow` fills the timed form's budget
//! (`Skill::GetRefreshDurationModifierData` into +1544) and
//! `Skill_Shapeshift::AddTimeToLive` @0x180507d40 (a toggled form: no-op)
//! adds the update's time, capped by the source's budget, to the time left
//! (the total follows when exceeded). Its only caller is a
//! `RefreshSkillDurationController`'s update (`sub_1805258f0`, vtable +1584),
//! which only a player registers (`crate::refresh`), and no player form is
//! timed: never reached in stock, not ported.
//!
//! Deferred swap (`SetShapeshifted` @0x180505a70 / `Update`): while
//! the character is in a physics simulation (`Entity::GetPhysicsSimulation`
//! not 0: `Character::ExecuteKnockdown` @0x18005d5bc's ragdoll, 3) the swap
//! is kept (+1888, the state +1889) and done by `Update` once the simulation
//! ends and the character is alive (or respawning). The port defers the
//! ending of a timed form ([`DeferredSwap`]): the form's stats, sound and
//! timer stop at once, the look and the skill set come back once its throw
//! ([`crate::ragdoll::Ragdoll`]) is over. A form cannot start knocked down
//! (the knockdown stops the cast, `afflictions`), nor does a player form end
//! thrown (a form drained of energy swaps at once).
use crate::casting::Caster;
use crate::character::{Character, FormRecords};
use crate::creature::BodyCache;
use crate::data::GameData;
use crate::monster_skills::MonsterSkills;
use crate::monsters::Monster;
use crate::player::{Loadout, Player, ProfileDirty};
use crate::weapon_pool::SkillLinks;
use bevy::prelude::*;
use gd_formats::arz::Record;

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
                .before(crate::character::CharacterStage::Drive),
        );
    }
}

/// The player's active form.
#[derive(Component, Debug)]
pub struct Shapeshifted {
    pub skill: String,
    pub granted: Vec<String>,
    /// The form's mesh ([`replacement_mesh`], picked as it starts) and
    /// animation table (`replacementAnims`).
    pub mesh: String,
    pub anims: String,
    /// `replacementSounds`, `replacementFootsteps`, `attackTrail`.
    pub records: FormRecords,
    drain: f32,
}

/// A shapeshift cast fired (`casting::fire_casts`): toggles the form.
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
        // The character's active skill set becomes the form's
        // (`activeSkillSet`); the hotbar shows that set's page, created on
        // its first use (`PlayerHotSlotCtrl::Update` @0x1803eb2c0).
        let set = rec.i32("activeSkillSet").unwrap_or(0);
        if caster.set_active_set(set) {
            new_page(&data, &mut caster, set, &granted, &allowed_classes);
        }
        let level = m.level_index as u32 + 1;
        loadout
            .granted
            .extend(granted.iter().map(|g| (g.clone(), level)));
        loadout.form = Some((m.skill.clone(), allowed_classes));
        // The form runs as a timed-buff entry with no end: it leaves with
        // the form (below), not by expiry.
        loadout.buffs.add(&m.skill, m.level_index, f32::INFINITY);
        let drain = crate::casting::level_f(rec, "skillActiveManaCost", m.level_index);
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

/// A timed form's time left on a monster (`Skill_Shapeshift` +1540) and,
/// once it ran out, the end wait (module doc): the state's skill was seen
/// (+1856) and the form is ready to end (+1860).
#[derive(Component, Debug)]
pub struct TimedForm {
    pub left: f32,
    seen_skill: bool,
    ready: bool,
}

impl TimedForm {
    /// The end wait once the time is out (module doc): `skill` = the
    /// controller's state holds a skill, `state` the action state.
    fn end_ready(&mut self, skill: bool, state: crate::character::CharState) -> bool {
        use crate::character::CharState as S;
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

/// A form's end waiting for the knockdown to finish (`Skill_Shapeshift`
/// +1888, module doc): the look and skill set still the form's.
#[derive(Component, Debug)]
pub struct DeferredSwap;

/// A monster casts a shapeshift (module doc, timed forms).
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
            // ActivateNow with the form running: the time is zeroed.
            t.left = 0.0;
            continue;
        }
        let secs = crate::casting::level_f(rec, "skillActiveDuration", m.level_index);
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
        // A pending swap out of the look is dropped (a recast before it was
        // done: `SetShapeshifted(true)` finds the set still on).
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

/// `Skill_Shapeshift::Update`'s countdown and end wait (module doc): the
/// form ends once ready and not frozen or petrified; its stats and sound stop,
/// the monster's own skill set and look come back (after its throw:
/// [`DeferredSwap`]).
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
            commands.write_message(crate::visuals::PlaySound {
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

/// `Skill_Shapeshift::Update`'s deferred swap (+1888): done once the
/// physics simulation is over and the monster lives.
#[allow(clippy::type_complexity)]
fn apply_deferred_swaps(
    mut commands: Commands,
    data: Res<GameData>,
    mut q: Query<
        (Entity, &mut MonsterSkills),
        (
            With<DeferredSwap>,
            Without<crate::combat::Dead>,
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

/// A new set's page (`PlayerHotSlotCtrl::Update` @0x1803eb2c0): the first
/// two skills of the set (`SkillManager::GetDefaultSkillIds` @0x18052edb0:
/// skills whose `skillSet` is the set, in order) fill the left button and its
/// alternate-weapon twin (the first) and the right button and its twin (the
/// second); every key slot of the first and second row copies the default
/// page's skill when it may be used in the form (`CloneDefaultSlot`
/// @0x1803eb030: `SkillManager::IsGlobalSkillTypeAndAllowed` @0x18053e580,
/// every skill class passes the global-type list, then `allowedSkillTypes`
/// none listed = all, else the class by name).
fn new_page(
    data: &GameData,
    caster: &mut Caster,
    set: i32,
    granted: &[String],
    allowed: &[String],
) {
    use crate::casting::{SLOT_LEFT, SLOT_RIGHT, SLOT_ROW2};
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

/// The form ends (`Skill_Shapeshift::ReceiveActiveUpdate` going inactive:
/// `skillDeactivatedSound` at the character, `crate::combat_sounds::deactivate`).
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
        commands.write_message(crate::visuals::PlaySound { pak, at: Some(at) });
    }
    // The character's set goes back to the default one: its page is shown
    // again as it was (the form's page stays for the next time).
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

/// The modifiers of `skill`: the learned ones (`Skill` +320), then the gear's
/// (`Skill::GetItemSkillModifiers`), from [`SkillLinks`].
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

/// The form's mesh (`Skill_Shapeshift::GetReplacementMesh` @0x1805057f0):
/// `replacementMeshFemale` for a female character (gender 2), else
/// `replacementMeshMale`, each falling back to `replacementMesh`
/// (`Skill_Shapeshift::Load` @0x180506650); then every enabled modifier of the
/// skill in `mods` order (learned, then gear) with a
/// `shapeshiftMeshOverrideFemale` / `…Male` replaces it, so the last one wins
/// (`Skill_Modifier::GetShapeshiftMeshOverride` @0x1804fccc0; e.g. the
/// Berserker's transmuters and the `b30x` gear modifiers' wolf / raven skins).
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

/// Puts a character's (player or monster) [`Shapeshifted`] look on its [`Character`] and takes it
/// off again (module doc): the single place the override slot follows the
/// form. A character rebuilt while shifted (new equipment: the game makes a
/// new `Character`) gets the look back without the Transform animation.
/// PORT: a form whose mesh does not load keeps the character's own look.
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
            gd_sim::anim::AnimSet::load(
                &gd_sim::anim::RecordIndex::new(t),
                gd_sim::equip::HandState::Unarmed,
                |p| data.vfs.exists(p),
            )
        });
        c.set_form(mesh, body, set, s.records.clone());
        if s.is_added() && c.state != crate::character::CharState::Death {
            c.play_transform();
        }
    }
}

/// The body swap of a shape change (`wear_form`): what draws the character orders after it.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShapeshiftSet {
    Wear,
}
