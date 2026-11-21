use super::*;

/// The controller record's `Class` (the RTTI class `Monster::Load`
/// creates; Game.dll classInfo chain). Every class reads
/// `ControllerMonster::Load`'s fields; they differ in their states.
///
/// * `ControllerStationaryMonster` (classInfo 0x180A58610, vtable
///   0x1807472D0): `CanMove` (vtable slot 63) returns false, and its
///   `RegisterStates` @0x180170ad0 has no Patrol, Roam, Wander,
///   WaitToAttack, RepositionForAttack, NavigateObstacle, DodgeAttack or
///   QuestMove state; Flee, Return, FollowLeader, DefendLeader and Move are a
///   stub whose `OnBegin` @0x1801709d0 goes "Idle" (`SetState` of a missing
///   state only logs "State %s is trying to be set, but not found." and
///   stays). Its Pursue `OnBegin` @0x1801706d0: a valid enemy that is
///   `CloseEnoughToUseSkill` → "Attack", else "Idle". Idle's roaming
///   (`ControllerMonsterStateIdle::StartedRoaming`) and Attack's
///   `randomRepositionChance` need `CanMove`, so the monster never moves: it
///   turns to and attacks what comes in reach (the record's run speed is
///   not used by its AI).
/// * `ControllerTotem` (classInfo 0x180A58670, parent Stationary; ctor
///   0x1801752c0): a stationary monster whose `AngerManager` comparator is
///   the distance one (`SetDistanceAngerComparator`, sub_18001F0D0: the
///   entry's distance, set by `AngerManager::Update` @0x18001dbb0): it goes
///   for the nearest foe on its anger list, not the angriest.
/// * `ControllerMonsterHidden` (create sub_180103CB0): `appearDistance`
///   (+0x870, default 5, `Load` sub_18013DD90) and the monster's states with
///   "Startup" replaced by `ControllerMonsterStateHidden` (`RegisterStates`
///   sub_18013DDD0), see [`Hidden`].
/// * `ControllerGraeae` and `ControllerMonsterSynergy`: the monster's states
///   plus their own (`crate::graeae`, `crate::synergy`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ControllerClass {
    #[default]
    Monster,
    Stationary,
    Totem,
    Hidden,
    /// `ControllerGraeae` (`crate::graeae`).
    Graeae,
    /// `ControllerMonsterSynergy` (`crate::synergy`).
    Synergy,
    /// `ControllerSpirit` (`crate::spirit`).
    Spirit,
    /// `ControllerSpiritHost` (`crate::spirit`).
    SpiritHost,
}

impl ControllerClass {
    /// PORT: the other classes run as `ControllerMonster`.
    pub fn of(class: Option<&str>) -> Self {
        match class {
            Some("ControllerStationaryMonster") => Self::Stationary,
            Some("ControllerTotem") => Self::Totem,
            Some("ControllerMonsterHidden") => Self::Hidden,
            Some("ControllerGraeae") => Self::Graeae,
            Some("ControllerMonsterSynergy") => Self::Synergy,
            Some("ControllerSpirit") => Self::Spirit,
            Some("ControllerSpiritHost") => Self::SpiritHost,
            _ => Self::Monster,
        }
    }

    /// `ControllerMonster::CanMove` as far as the class decides it (the
    /// stationary classes: never; the port's `movement::Rooted` stands in for
    /// the rest of `ControllerMonster::CanMove` @0x18013b6e0).
    pub fn can_move(self) -> bool {
        !matches!(self, Self::Stationary | Self::Totem)
    }
}

/// `ControllerMonsterStateHidden` (the hidden monster's "Startup"):
/// `OnBegin` @0x18013dea0 plays a `LongIdleAction` (a monster that has
/// popped out, `Monster` +20889 `GetPoppedOut`, goes "Idle"). Every
/// `OnUpdate` @0x18013df90 until it has a target gathers its foes within
/// `ViewDistance` (a sphere), takes the closest, and when that one is nearer
/// than `appearDistance` (3D) makes it the target, plays the Spawn animation
/// (type 19, speed 1), marks the monster popped out and, when the monster's
/// `ambushDissolveTime` is not 0, un-dissolves its mesh and every attached
/// actor ([`AmbushDissolve`]). `HandleEvent` "End" (the animation's end) with a
/// target: "Pursue" it with `ChooseBestSkill`. Every other event (enemy
/// found, attacked, ally attacked, projectiles, low health) is the empty
/// base handler (`HandleEvent` @0x18013e5f0 reacts to "End" only): until
/// then the monster ignores hits and calls.
#[derive(Clone, Copy, Debug, Default)]
pub struct Hidden {
    pub appear: f32,
    /// The foe it appears for (its Spawn animation is playing).
    pub target: Option<Entity>,
    /// The monster record's `ambushDissolveTime` (`Monster::Load` @0x180373810: an int read as ms,
    /// default 0; the texture `ambushDissolveTexture`, default `System/Textures/Cloud.tex`).
    pub dissolve_ms: i32,
}

/// The mesh un-dissolve a hidden monster starts when it appears ([`Hidden`]); the renderer draws it
/// (grimdawn `monsters.rs`, the `standarddissolve*` shaders) and the component goes when it is done.
/// Engine.dll `GraphicsMeshInstance::BeginUnDissolve` @0x1800bfce0 (called with colour 0, 0, 0,
/// `ambushDissolveTime × 0.001` s, the ambush texture, override shader on): value 1, duration
/// `(int)(s × 1000)` ms, the instance's shader replaced by `standarddissolve{skinned,static}.ssh`
/// (`GraphicsMeshInstance::Update` @0x1800bffd0). Every mesh update with `dt` ms (sub_1800C67F0):
/// `value -= dt / duration`; at `<= 0` the value is 0, the un-dissolve ends and the override shader is
/// unloaded. Stock times are 0, 2 or 3 (ms: the designers' "seconds" read as an int), so the mesh is
/// whole after the first update.
/// PORT: the first step is taken on the frame after the appearance (the engine's order of the
/// controller and the mesh update within a frame is unchecked).
#[derive(Component, Clone, Copy, Debug)]
pub struct AmbushDissolve {
    /// `dissolveValue`: 1 = gone, 0 = whole.
    pub value: f32,
    pub duration_ms: i32,
}

impl AmbushDissolve {
    /// `BeginUnDissolve` for `ambushDissolveTime` (as `Hidden::OnUpdate` hands it: × 0.001, then × 1000).
    pub fn begin(dissolve_ms: i32) -> Self {
        Self {
            value: 1.0,
            duration_ms: ((dissolve_ms as f32 * 0.001) * 1000.0) as i32,
        }
    }

    /// One mesh update of `dt` ms; true when the un-dissolve has ended.
    pub fn step(&mut self, dt: i32) -> bool {
        self.value -= dt as f32 / self.duration_ms as f32;
        if self.value <= 0.0 {
            self.value = 0.0;
            return true;
        }
        false
    }
}

/// Advances every [`AmbushDissolve`] (whole milliseconds, the remainder carries) and removes the finished.
pub fn ambush_dissolve(
    mut commands: Commands,
    time: Res<Time>,
    mut carry: Local<f32>,
    mut q: Query<(Entity, &mut AmbushDissolve)>,
) {
    let dt = crate::session::frame_ms(&mut carry, &time) as f32;
    for (e, mut d) in &mut q {
        if d.is_added() {
            continue;
        }
        if d.step(dt as i32) {
            commands.entity(e).try_remove::<AmbushDissolve>();
        }
    }
}

/// `ControllerMonsterStateStartup` for a monster whose Spawn animation was enabled
/// (`Monster::EnableSpawnAnimation` @0x18037e820 sets the controller's +1542; who: [`MonsterAi::enable_spawn_animation`],
/// the player's pets: `pets::Pet::startup`; `ProxyMenu`'s main-menu scene creatures are not ported). `OnBegin` @0x18013ead0: when the current animation set has a
/// Spawn (type 19) animation it plays it once (speed 1) and stays; `HandleEvent` @0x18013edc0 reacts to "End"
/// only (then "Idle"); without one it goes "Idle" at once. Meanwhile the monster takes no enemy, ignores
/// hits and calls and does not move (`RequestAttack` / `RequestMove` are overridden).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpawnStartup {
    /// "Idle" and on (every monster not given the Spawn animation).
    #[default]
    Done,
    /// Spawn animation enabled, `OnBegin` not run yet.
    Pending,
    /// The Spawn animation is playing.
    Playing,
}

/// A monster that is not the player's foe (a Crucible defense turret, a town guard: `factions` Survivors; who gets it:
/// `monsters::is_fighter`; a reputation change turns it, `factions::turn_creatures`): it cannot be targeted by the
/// player and does not fight him. Every monster, allied or not, fights the monsters that are its foes by faction
/// (`FactionPack::is_foe`; `ControllerMonster::AngerUpdate` @0x18013bd20 asks `FactionManager::IsFoe`
/// @0x18028f870 of every character in its view sphere: there is no monster-vs-monster table nor ally flag, two
/// monster factions are foes by the same pack rule as the player), seeing the ones with `causesAnger` and taking as
/// enemies the `targetable` ones ([`MonsterAi::as_target`]: a guard; a turret has neither), see [`FoeLists`].
/// PORT: a monster's relation to the player is this mark, `IsFoe(player, it)`; the engine's monster asks
/// `IsFoe(it, player)` (`FactionManager::IsFoe(Character*, Character*, bool)` @0x18028f870 and the pack
/// rule @0x18028f960: its own `factionPlayer` first, then the player's value for its faction), which differs only
/// when the pack's `factionPlayer` is non-zero and the player's value for its faction has the other sign. Stock
/// data never gets there for a fighter: every faction the player gains or loses
/// standing with (Survivors, `User0`/`4`/`5`/`7`-`11`/`13`-`15`/`17`/`21`) has `factionPlayer` 0; the packs with a
/// non-zero one hold -100 / -1000 / -24000 for factions the player's pack starts below 0 and no kill bonus, quest
/// or conversation raises (`noRepGainFactions`, the kill bonuses go to the factions above), except Homestead
/// (`User2`, +1000, one quest gives -250) whose creatures are all `Npc`s, and the Crucible's Survivors (+1000),
/// which only gain. Skill victims follow the same relation ([`Side`]).
#[derive(Component)]
pub struct Allied;

/// Whose foe a skill effect's caster is: the victim rule of every skill hit (radius, cone, ray, projectile, aura,
/// area, debuff). `GameEngine::FilterInvalidTargets` @0x1802eca00 (behind `GetTargetsInRadius` /
/// `AlongRay` / `AroundRay` / `InCone` / `InFrustum` / `GetAllTargetsInRadius`): a non-character entity that is
/// alive or intact always passes (breakables); a character passes when it is alive (`IsAlive`), not hiding (unless
/// the caller allows it), not an `Npc`, not the excluded id, and `FactionManager::IsFoe(caster, it)` (the friend
/// filter: `IsFriend`) with the caster looked up by its object id; when the caster is gone, `IsFoe` of the pack the
/// skill passed against the victim's faction and pack. No side table: two monsters of foe factions hit each other
/// with every skill as with weapon swings, a monster's area hurts no monster of its own faction, an `Allied` guard's
/// skills hit its faction foes and never the player.
/// A projectile or lasting area keeps its caster's side ([`Side::lookup_kept`]: the pack `FilterInvalidTargets`
/// is given), so after its caster despawned it still hits that pack's foes; breakables take hits from every caster.
/// PORT: the player's relation to a monster is the [`Allied`] mark both ways (see there); pets stand with the
/// player unless a monster's convert joined them to its pack ([`JoinedPack`]); the side is kept at the effect's
/// first update (an effect whose caster despawned before it falls back to its `hostile` flag,
/// [`Side::from_hostile`]); other effects (auras, beams, leaps) end with their caster.
#[derive(Clone, Debug, Default)]
pub enum Side {
    /// The player and their pets (no `MonsterAi`, no [`JoinedPack`]).
    #[default]
    Player,
    Monster {
        pack: Option<std::sync::Arc<crate::monsters::FactionPack>>,
        allied: bool,
    },
}

/// A [`Side`] borrowed from a character's components ([`SideData`]), for the victim end of [`Side::foe`].
#[derive(Clone, Copy, Debug)]
pub enum SideRef<'a> {
    Player,
    Monster {
        pack: &'a Option<std::sync::Arc<crate::monsters::FactionPack>>,
        allied: bool,
    },
}

/// A player's pet a monster's convert joined to its pack (`Monster::JoinMe`, `combat::influence`): the pack and
/// the attacker's `Allied` mark, its side while the join lasts, and the attacker, its leader meanwhile (the
/// pet's AI follows it; the owner is [`crate::combat::Converted::leader`], its leader again at the end).
#[derive(Component, Debug, Clone)]
pub struct JoinedPack {
    pub pack: Option<std::sync::Arc<crate::monsters::FactionPack>>,
    pub allied: bool,
    pub leader: Entity,
}

/// What decides a character's [`Side`]: its AI (faction pack), `Allied` mark, and a pet's [`JoinedPack`].
#[derive(bevy::ecs::query::QueryData)]
pub struct SideData {
    pub ai: Option<&'static MonsterAi>,
    pub allied: Has<Allied>,
    pub joined: Option<&'static JoinedPack>,
}

impl SideDataItem<'_, '_> {
    /// The character's side, borrowed.
    pub fn side_ref(&self) -> SideRef<'_> {
        match (self.ai, self.joined) {
            (Some(ai), _) => SideRef::Monster {
                pack: &ai.faction,
                allied: self.allied,
            },
            (None, Some(j)) => SideRef::Monster {
                pack: &j.pack,
                allied: j.allied,
            },
            (None, None) => SideRef::Player,
        }
    }

    /// The character's side.
    pub fn side(&self) -> Side {
        match self.side_ref() {
            SideRef::Player => Side::Player,
            SideRef::Monster { pack, allied } => Side::Monster {
                pack: pack.clone(),
                allied,
            },
        }
    }
}

/// What a victim query reads for [`Side::hits`]: its [`SideData`], whether it is a breakable or a physics prop
/// (`crate::physics_decoration`: both are intact non-characters), and whether it is hiding ([`Self::hiding`]).
#[derive(bevy::ecs::query::QueryData)]
pub struct VictimSide {
    pub side: SideData,
    pub breakable: Has<crate::world::Breakable>,
    prop: Has<crate::physics_decoration::PhysicsDecoration>,
    hidden_from_combat: Has<HiddenFromCombat>,
    invisible: Has<crate::visuals::Hidden>,
}

impl VictimSideItem<'_, '_> {
    /// `Character::IsHiding` @0x18005bba0 (vtable +0x468; no override in `Monster` / `Player`):
    /// `!Entity::IsVisible()` (Engine.dll @0x18007f7c0: the visibility +392 is 1 or 3, so an actor
    /// `SetVisibility(false)` hid, [`crate::visuals::Hidden`]) or `hiddenFromCombat` (+0x24B8,
    /// [`HiddenFromCombat`]) or the life state +9392 `Unknown` (0, before `Character` set-up: no port
    /// counterpart).
    pub fn hiding(&self) -> bool {
        self.hidden_from_combat || self.invisible
    }

    /// A character, what an AI takes as its target (`IsEnemyValid`, `AttackingEnemy`'s character): not a
    /// breakable nor a physics prop. A prop has no `MonsterAi` / `Player` / `Pet`, so its [`SideData`] would read
    /// as the player's side and a monster's pick would take it for a foe.
    pub fn character(&self) -> bool {
        !self.breakable && !self.prop
    }
}

/// The skill target queries' filter for the player's own foe queries that take no [`VictimSide`]
/// (`FilterInvalidTargets` drops hiding characters, [`Side::hits`]).
pub type NotHiding = (Without<HiddenFromCombat>, Without<crate::visuals::Hidden>);

impl Side {
    /// A cast's side when its caster is gone (`hostile`: a monster's).
    pub fn from_hostile(hostile: bool) -> Self {
        if hostile {
            Self::Monster {
                pack: None,
                allied: false,
            }
        } else {
            Self::Player
        }
    }

    /// The caster `e`'s side, else ([`Self::from_hostile`]) the cast's.
    pub fn lookup<F: bevy::ecs::query::QueryFilter>(
        sides: &Query<SideData, F>,
        e: Entity,
        hostile: bool,
    ) -> Self {
        sides
            .get(e)
            .map_or_else(|_| Self::from_hostile(hostile), |s| s.side())
    }

    /// The caster `e`'s side, kept in `kept` while it lives; once it is gone, the kept side (the skill's stored
    /// faction pack, `FilterInvalidTargets`' fallback), else [`Self::from_hostile`].
    pub fn lookup_kept<F: bevy::ecs::query::QueryFilter>(
        sides: &Query<SideData, F>,
        e: Entity,
        hostile: bool,
        kept: &mut Option<Side>,
    ) -> Self {
        match sides.get(e) {
            Ok(s) => {
                let side = s.side();
                *kept = Some(side.clone());
                side
            }
            Err(_) => kept.clone().unwrap_or_else(|| Self::from_hostile(hostile)),
        }
    }

    /// Whether it is a monster's (it seeks the player's side).
    pub fn hostile(&self) -> bool {
        matches!(self, Self::Monster { allied: false, .. })
    }

    /// This side borrowed, for the victim end of [`Self::foe`].
    pub fn as_ref(&self) -> SideRef<'_> {
        match self {
            Self::Player => SideRef::Player,
            Self::Monster { pack, allied } => SideRef::Monster {
                pack,
                allied: *allied,
            },
        }
    }

    /// `IsFoe(caster, character)` for a character on side `victim`.
    pub fn foe(&self, victim: SideRef) -> bool {
        match (self, victim) {
            (Self::Player, SideRef::Player) => false,
            (Self::Player, SideRef::Monster { allied, .. }) => !allied,
            (Self::Monster { allied: a, .. }, SideRef::Player) => !a,
            (Self::Monster { pack, .. }, SideRef::Monster { pack: v, .. }) => {
                crate::monsters::FactionPack::foes(pack, v)
            }
        }
    }

    /// Whether a skill of this side hits an entity ([`VictimSide`]): every breakable and physics prop and its
    /// foes that are not hiding. `GameEngine::FilterInvalidTargets` @0x1802eca00, run by every skill
    /// target query (`GetTargetsInRadius` @0x1802ebe50, `…AroundRay` @0x1802ec0b0, `…AlongRay`, `…InCone`
    /// @0x1802ec2a0, `…InFrustum`, `GetAllTargetsInRadius`): a non-character that is alive / intact always
    /// passes (`PhysicsDecoration::TakeAttack` @0x1803cb850 takes an attack from anyone); a
    /// character must be alive, not an `Npc`, not hiding ([`VictimSideItem::hiding`]; only
    /// `Skill_TurretFireControl::ActivateNow` @0x18050c370 asks for hiding ones: the caster's own turrets,
    /// `casting::Kind::TurretFire`) and a foe. The test is at query time: the attack itself (`Character::AttackTarget` @0x180067460,
    /// `CombatManager::TakeAttack` @0x18011d950) does not ask, nor does a melee swing at its chosen target
    /// (`SkillActivated::ExecuteMeleeAttack` @0x180518320 without a cone only checks `IsTargetInRange`).
    /// A pick of one target among these (an autocast's nearest foe) takes [`VictimSideItem::character`]s only.
    pub fn hits(&self, v: &VictimSideItem) -> bool {
        !v.character() || (!v.hiding() && self.foe(v.side.side_ref()))
    }
}

/// What the player may aim a click or the controller at: a targetable breakable, else a character
/// [`Side::Player`] calls a foe (a monster without `Allied`, a pet a monster's convert joined).
pub fn player_target(breakable: Option<&crate::world::Breakable>, side: &SideDataItem) -> bool {
    breakable.map_or_else(|| Side::Player.foe(side.side_ref()), |b| b.targetable)
}

/// The characters each monster may take as its enemy this frame (`ControllerMonster::IsEnemyValid` @0x18013b270:
/// alive, `targetable` (Character +8026), `FactionManager::IsFoe(me, it)`; the pursuit range is checked by
/// the caller): the player and their pets for a hostile monster, and the living `targetable` monsters whose pack is a
/// foe of its own (`IsFoe(FactionPack, FactionType, FactionPack)` @0x18028f960: its value for their faction when
/// non-zero, else theirs for its faction; `FactionPack::foes`). The answer only depends on the monster's pack and
/// whether it is `Allied`, so the list is built once per side present (a handful of packs), not per monster. Each
/// side's list is indexed ([`Foes`]): a monster's per-frame lookups of its enemy do not walk it.
pub(super) struct FoeLists<'a> {
    players: &'a [(Entity, Vec3, f32)],
    /// The distinct packs of the targetable monsters (only those are cloned).
    packs: Vec<Option<std::sync::Arc<crate::monsters::FactionPack>>>,
    /// (entity, position, radius, index into `packs`) of the targetable monsters.
    rivals: Vec<(Entity, Vec3, f32, Option<usize>)>,
    sides: Vec<(
        Option<std::sync::Arc<crate::monsters::FactionPack>>,
        bool,
        Foes,
    )>,
}

impl<'a> FoeLists<'a> {
    pub(super) fn new(
        players: &'a [(Entity, Vec3, f32)],
        packs: Vec<Option<std::sync::Arc<crate::monsters::FactionPack>>>,
        rivals: Vec<(Entity, Vec3, f32, Option<usize>)>,
    ) -> Self {
        Self {
            players,
            packs,
            rivals,
            sides: Vec::new(),
        }
    }

    /// The index of the side of a monster of `pack` (`allied`: `Allied`), built on first use.
    pub(super) fn side(
        &mut self,
        pack: &Option<std::sync::Arc<crate::monsters::FactionPack>>,
        allied: bool,
    ) -> usize {
        if let Some(i) = self
            .sides
            .iter()
            .position(|s| s.1 == allied && crate::monsters::FactionPack::same(&s.0, pack))
        {
            return i;
        }
        let mut foes: Vec<(Entity, Vec3, f32)> = if allied {
            Vec::new()
        } else {
            self.players.to_vec()
        };
        if pack.is_some() {
            foes.extend(
                self.rivals
                    .iter()
                    .filter(|r| {
                        r.3.is_some_and(|i| {
                            crate::monsters::FactionPack::foes(pack, &self.packs[i])
                        })
                    })
                    .map(|r| (r.0, r.1, r.2)),
            );
        }
        self.sides.push((pack.clone(), allied, Foes::new(foes)));
        self.sides.len() - 1
    }

    pub(super) fn get(&self, side: usize) -> &Foes {
        &self.sides[side].2
    }
}

/// A side's foes ([`FoeLists`]): (entity, position, radius) in the list's order, indexed by entity (an entity is
/// listed once: the player's side and the monsters are disjoint queries) and, on the first [`Foes::nearest`], by
/// position (`combat::grid`).
pub(super) struct Foes {
    list: Vec<(Entity, Vec3, f32)>,
    by_entity: bevy::ecs::entity::EntityHashMap<usize>,
    grid: std::cell::OnceCell<gd_sim::grid::Grid>,
}

impl Foes {
    pub(super) fn new(list: Vec<(Entity, Vec3, f32)>) -> Self {
        let mut by_entity = bevy::ecs::entity::EntityHashMap::default();
        by_entity.reserve(list.len());
        for (i, f) in list.iter().enumerate() {
            // The first entry, as a scan's `find` would.
            by_entity.entry(f.0).or_insert(i);
        }
        Self {
            list,
            by_entity,
            grid: std::cell::OnceCell::new(),
        }
    }

    /// The whole list, in order.
    pub(super) fn all(&self) -> &[(Entity, Vec3, f32)] {
        &self.list
    }

    /// The foe `e`, if listed.
    pub(super) fn get(&self, e: Entity) -> Option<(Entity, Vec3, f32)> {
        self.by_entity.get(&e).map(|&i| self.list[i])
    }

    /// The nearest foe within `range` of `me` (3D distance ≤ `range`) and its distance, the first listed on a tie
    /// (a scan's `min_by`). `near` is scratch.
    pub(super) fn nearest(
        &self,
        me: Vec3,
        range: f32,
        near: &mut Vec<usize>,
    ) -> Option<(Entity, f32)> {
        self.grid
            .get_or_init(|| {
                gd_sim::grid::Grid::new(grid::CELL, self.list.iter().map(|f| grid::xz(f.1)))
            })
            .near(grid::xz(me), grid::pad(range), near);
        near.iter()
            .map(|&i| (self.list[i].0, self.list[i].1.distance(me)))
            .filter(|f| f.1 <= range)
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
}

/// The index of `faction` in a table of the distinct packs, adding it (one `Arc` clone) when new. The rivals
/// of [`FoeLists`] name their pack by index, so only the distinct packs are cloned, not one per monster.
pub(super) fn pack_index(
    packs: &mut Vec<Option<std::sync::Arc<crate::monsters::FactionPack>>>,
    faction: &Option<std::sync::Arc<crate::monsters::FactionPack>>,
) -> Option<usize> {
    faction.is_some().then(|| {
        packs
            .iter()
            .position(|p| crate::monsters::FactionPack::same(p, faction))
            .unwrap_or_else(|| {
                packs.push(faction.clone());
                packs.len() - 1
            })
    })
}

/// A creature's record fields its foes' sight and targeting read ([`MonsterAi::as_target`]).
#[derive(Clone, Copy, Debug)]
pub struct AsTarget {
    pub causes_anger: bool,
    pub targetable: bool,
    pub anger_multiplier: f32,
}

/// Monster behaviour state and its controller record's settings.
///
/// By design, no Emote state: the controller's `randomEmoteChance` / `randomEmoteMinTime` / `randomEmoteMaxTime`
/// (+0x64C / +0x650 / +0x654) drive `ControllerMonster::Update` @0x180134ec0 (in any state but Dying / Dead):
/// every `IGenerate(min, max)` ms a `rand() % 100 < chance` adds the temporary state "Emote"
/// (`ControllerMonsterStateEmote`: `OnBegin` @0x18014bb70 plays animation type 36 `Emote`, `HandleEvent`
/// @0x18014bbb0 on "End" idles and pops it; `AddTemporaryState` @0x180123fb0 suspends the current state and
/// releases its attack slot) only when `Character::CanPlayAnimation(Emote)` holds (the inlined test: the
/// animation set's slot 36, `AnimSlotRandom_IsEmpty` @0x180021920, is not empty). No stock record
/// sets a `*EmoteAnim1..3` file (`gd_sim::anim`), so the state is never entered.
#[derive(Component)]
pub struct MonsterAi {
    pub home: Vec3,
    /// The controller record's class.
    pub class: ControllerClass,
    /// The hidden monster's "Startup" state while it lasts (see [`Hidden`]).
    pub hidden: Option<Hidden>,
    /// The Spawn-animation "Startup" ([`SpawnStartup`]).
    pub startup: SpawnStartup,
    /// The dodge fields, timer and the DodgeAttack state (dodge.rs).
    pub dodge: crate::dodge::Dodge,
    /// The `RandomAnger*` / `ignorePets*` rolls (anger_rolls.rs).
    pub rolls: crate::anger_rolls::AngerRolls,
    /// `LeadChance` per difficulty (lead.rs).
    pub lead: [f32; 3],
    /// `RepositionChance` (%, melee.rs `ProjectileCollided`).
    pub reposition_chance: i32,
    pub view: f32,
    pub view_y: f32,
    pub pursuit: f32,
    pub swing_pause: (f32, f32),
    pub aggro: bool,
    pub(super) path: Vec<Vec3>,
    /// Anger per foe (`AngerManager`), see [`anger_from_hits`].
    pub anger: Vec<(Entity, f32)>,
    /// Controller `AttackedAnger` and `petAngerTransference` / 100.
    pub(super) anger_cfg: (f32, f32),
    /// `InnerViewDistance`, `SightAngerRate`, `InnerSightAngerRate`.
    pub(super) sight: (f32, f32, f32),
    /// `ForgiveRate`: anger lost per second (`AngerManager::Update`).
    pub(super) forgive: f32,
    /// `UnderAttack` re-arm timer (500 ms).
    pub(super) anger_timer: f32,
    /// Each anger entry as `AngerManager::Update` @0x18001dbb0 left it at the last `AngerUpdate` tick, before
    /// forgiving (the entry's +36): [`MonsterAi::anger_diff`].
    pub(super) anger_base: Vec<(Entity, f32)>,
    /// The alert before a pursuit ([`Alert`]).
    pub(super) alert: Alert,
    /// `AngerTolerance`: least anger a target needs (`GetNewTarget`).
    pub(super) tolerance: f32,
    /// Controller `ignoreSleepingEnemies` (+0x605, default 0; `ControllerMonster::Load`): a foe that is
    /// not a player and sleeps ([`crate::afflictions::Afflictions::asleep`], action state 11) is skipped by
    /// `GetNewTarget` @0x18001e1c0 ([`find_enemy`]), by `FindEnemyUpdate` @0x18013c0f0's too-close (flee) check, and
    /// dropped from the anger list by `AngerManager::Update` @0x18001dbb0 (`ShouldRemoveEnemy` @0x18001eec0) at each
    /// `AngerUpdate` tick (`sight_anger`).
    pub(super) ignore_sleeping: bool,
    /// `AllyAttackedAnger`, `ChanceToRespondToDistressCall` and
    /// `DistressResponseBehavior` (0 same group, 1 same race, 2 all).
    pub(super) ally_anger: f32,
    pub(super) respond: f32,
    pub(super) respond_behavior: u8,
    /// The record's `characterRacialProfile` entries.
    pub(super) race: Vec<String>,
    /// `pathingViewDistance` (0: off).
    pub(super) pathing_view: f32,
    /// This monster's own distress calls.
    pub(super) distress: Distress,
    /// Seconds to the next `AngerUpdate` tick (250 ms period).
    pub(super) anger_tick: f32,
    /// Seconds to the next `FindEnemyUpdate` (200 ms period).
    pub(super) find_timer: f32,
    /// The enemy `FindEnemy` picked.
    pub enemy: Option<Entity>,
    /// The controller's patrol points and "Patrol" state.
    pub patrol: crate::patrol::PatrolState,
    /// The record's faction pack (`FactionManager::IsFriend`).
    pub faction: Option<std::sync::Arc<crate::monsters::FactionPack>>,
    /// How its foes see it (`Character::Load` @0x180054e70): `causesAnger` (Character +8376, default 1:
    /// it enters a foe's sight sphere, `GetCharactersInSphere`), `targetable` (+8026, default 1: `IsEnemyValid` /
    /// `GetNewTarget` accept it) and `angerMultiplier` (+8388: `AngerManager::AddAnger` scales anger for it).
    /// Read by the monsters whose faction is its foe ([`FoeLists`], `sight_anger`).
    pub as_target: AsTarget,
    /// The Flee state's settings and bookkeeping (flee.rs).
    pub flee: crate::flee::Flee,
    /// A line-of-sight spot to run to (monster_skills.rs) and whether its
    /// path was computed; seconds until another skill may be tried.
    pub reposition: Option<Vec3>,
    pub(super) repos_pathed: bool,
    /// `UseSkill` found no line-of-sight spot: the "RepositionForAttack" /
    /// "Return" give-up (monster_skills.rs), taken by `monster_ai`.
    pub give_up: bool,
    /// In that "Return": the enemy is taken up again only at a
    /// `FindEnemyUpdate` tick.
    pub(super) returning: bool,
    /// The fight states (melee.rs).
    pub melee: crate::melee::MeleeAi,
    /// `walkDistance`, `walkSpeed`, `walkUsesRun` (`Character::Load`).
    pub walk: crate::melee::Walk,
}

/// The alert before a pursuit. The `Monster` record's `alertAnimChance` (+18636, int %, default 0),
/// `alertSoundChance` (+18632, %) and `alertSound` (+2328), `Monster::Load` @0x180373810; `once` is the
/// controller's +920 (`ShouldPlayRallyOrAlert` @0x180139790: 1 from the constructor, cleared by the first alert
/// sound, set by nothing else). `ControllerMonsterState::DefaultEnemyFoundResponse` @0x18014cab0, once
/// the response made "Pursue" the state and the executing state is "Pursue" (no self buff pushed over it):
/// `rand() % 100 < alertAnimChance`, the animation set has the Alert slot (33), the enemy is farther than
/// `gameengine.dbr` `alertDistance` ([`crate::rules::GameEngineRules::alert_distance`]) and `AngerManager::GetAngerDiff` @0x18001db60 of the
/// enemy is below 15 (spotted rather than just hit, [`MonsterAi::anger_diff`]): the temporary state
/// "AlertBeforePursue" (`OnBegin` @0x18014b940 plays Alert once at speed 1, `OnUpdate` @0x18014b980 turns to the
/// enemy, `HandleEvent` @0x18014bab0 pops itself on "End", the Pursue under it goes on; its event handlers are all
/// empty: hits and calls are ignored meanwhile) and `alertSound` plays. Else `rand() % 100 < alertSoundChance`
/// and `once` play `alertSound` (and clear `once`).
#[derive(Clone, Debug, Default)]
pub struct Alert {
    anim_chance: i32,
    sound_chance: i32,
    sound: Option<String>,
    once: bool,
    /// "AlertBeforePursue" is the executing state, on this enemy.
    pub(super) on: Option<Entity>,
}

impl Alert {
    fn of(rec: Option<&gd_formats::arz::Record>) -> Self {
        let i = |k: &str| rec.and_then(|r| r.f32(k)).unwrap_or(0.0) as i32;
        Self {
            anim_chance: i("alertAnimChance"),
            sound_chance: i("alertSoundChance"),
            sound: rec
                .and_then(|r| r.str("alertSound"))
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            once: true,
            on: None,
        }
    }

    /// `DefaultEnemyFoundResponse`'s alert for `enemy` (type docs): `far` (beyond `alertDistance`), `diff` its
    /// `GetAngerDiff`, `can_play` the Alert slot. True when "AlertBeforePursue" starts; the sound to play, if any.
    pub(super) fn enemy_found(
        &mut self,
        enemy: Entity,
        can_play: bool,
        far: bool,
        diff: f32,
    ) -> (bool, Option<String>) {
        if (crate::rng::u32(..32768) % 100) < self.anim_chance.max(0) as u32
            && can_play
            && far
            && diff < 15.0
        {
            self.on = Some(enemy);
            return (true, self.sound.clone());
        }
        if (crate::rng::u32(..32768) % 100) < self.sound_chance.max(0) as u32 && self.once {
            self.once = false;
            return (false, self.sound.clone());
        }
        (false, None)
    }
}

/// A script's `MoveAction`: the character walks the path at its run speed
/// (`ControllerMonsterStateQuestMove::OnBegin` → `ControllerAI::MoveTo` with
/// `Character::GetRunSpeed`) ignoring enemies and attacks, and on
/// reaching the end (`EndOfPathReached`) records the spot as its spawn point
/// and goes idle. `quest_walk` runs it for every story character (friendly
/// ones have no `MonsterAi`); the monster AI and skill choice skip it.
///
/// `interruptible` is `MoveAction`'s bool, the controller's
/// `AllowQuestInterruptions` (`ControllerAI::QuestCommandMove` stores it at
/// +812): the state looks for enemies (`ShouldFindEnemy`) and
/// answers `EnemyFound` / `Attacked` / `LowHealth` / ... with the default
/// responses (the monster leaves the move to fight) only when it is set; else
/// it ignores them all. [`quest_interrupt`] is that.
#[derive(Component)]
pub struct QuestMove {
    pub path: Vec<Vec3>,
    pub interruptible: bool,
}

/// The enemy part of `ControllerMonster::FindEnemyUpdate` for a monster.
///
/// A `ControllerTotem` ranks the foes that pass the tolerance by distance,
/// the nearest first (see [`ControllerClass`]).
///
/// `pets` are the foes that are pets: while the monster ignores pets
/// (anger_rolls.rs) the too-close check and the target pick leave them out.
/// `asleep` tells the foes that are asleep and not players: an
/// `ignoreSleepingEnemies` monster leaves them out of both too
/// ([`MonsterAi::ignore_sleeping`]).
pub(super) fn find_enemy(
    ai: &mut MonsterAi,
    me: Vec3,
    foes: &Foes,
    pets: &[Entity],
    asleep: impl Fn(Entity) -> bool,
    dt: f32,
) {
    let before = ai.enemy;
    ai.find_timer -= dt;
    let ticked = ai.find_timer <= 0.0;
    if ticked {
        ai.find_timer += 0.2 * (1.0 - ai.find_timer / 0.2).floor().max(1.0);
        let ignore_pets = crate::anger_rolls::ignores_pets(ai, |e| pets.contains(&e));
        let ignore_sleeping = ai.ignore_sleeping;
        let skip = |e: Entity| (ignore_pets && pets.contains(&e)) || (ignore_sleeping && asleep(e));
        let kept: Vec<(Entity, Vec3, f32)>;
        let candidates = if ignore_pets || ignore_sleeping {
            kept = foes.all().iter().filter(|f| !skip(f.0)).copied().collect();
            &kept[..]
        } else {
            foes.all()
        };
        crate::flee::too_close_check(ai, me, candidates);
        // `new_target` only reads the candidates on the anger list: those, looked up (the same entries a scan of
        // `candidates` finds).
        let on_list: Vec<(Entity, Vec3, f32)> = ai
            .anger
            .iter()
            .filter_map(|a| foes.get(a.0))
            .filter(|f| !skip(f.0))
            .collect();
        ai.enemy = new_target(
            &ai.anger,
            &on_list,
            me,
            ai.enemy,
            ai.tolerance,
            ai.class == ControllerClass::Totem,
        );
    }
    if let Some(t) = ai.enemy
        && foes.get(t).is_none_or(|f| !ai.in_pursuit(me, f.1))
    {
        ai.enemy = None;
    }
    if ai.enemy.is_some() && (ticked || !ai.returning) {
        ai.aggro = true;
        ai.returning = false;
    }
    // `DefaultEnemyFoundResponse`: a new enemy makes a
    // `LeadWhenEnemyIsSeen` monster call its followers.
    if ai.enemy.is_some()
        && ai.enemy != before
        && ai.patrol.settings.leader == crate::patrol::LeaderBehavior::WhenEnemyIsSeen
    {
        ai.patrol.call = true;
    }
}

/// `AngerManager::GetNewTarget`'s pick: the foes on the anger list whose
/// anger (× 1.3 for the current target) reaches the tolerance, the angriest
/// first, or with `nearest` (the totem's distance comparator) the nearest.
pub(super) fn new_target(
    anger: &[(Entity, f32)],
    foes: &[(Entity, Vec3, f32)],
    me: Vec3,
    current: Option<Entity>,
    tolerance: f32,
    nearest: bool,
) -> Option<Entity> {
    let valid = anger
        .iter()
        .filter_map(|(e, v)| {
            foes.iter().find(|f| f.0 == *e).map(|f| {
                (
                    *e,
                    v * if Some(*e) == current { 1.3 } else { 1.0 },
                    f.1.distance(me),
                )
            })
        })
        .filter(|(_, v, _)| *v >= tolerance);
    if nearest {
        valid.min_by(|a, b| a.2.total_cmp(&b.2))
    } else {
        valid.max_by(|a, b| a.1.total_cmp(&b.1))
    }
    .map(|(e, _, _)| e)
}

/// The characters' sleep, for [`MonsterAi::ignore_sleeping`]: a `Player` is never skipped (the `Player` class
/// test in `GetNewTarget` / `ShouldRemoveEnemy` / `FindEnemyUpdate`); pets and monsters are.
pub(super) type Sleepers<'w, 's> =
    Query<'w, 's, &'static crate::afflictions::Afflictions, Without<Player>>;

/// `e` is asleep and not a player ([`Sleepers`]).
pub(super) fn asleep(q: &Sleepers, e: Entity) -> bool {
    q.get(e).is_ok_and(|a| a.asleep())
}

/// An interruptible `MoveAction` ends when its monster gets an enemy; like
/// every end of the "QuestMove" state (`OnEnd` @0x18014be50) the spot becomes
/// its spawn point. Its enemies are the ones `FindEnemyUpdate` /
/// `IsEnemyValid` takes in every state ([`FoeLists`]: the player's side for a
/// hostile monster, the `targetable` monsters of foe factions for all).
#[allow(clippy::type_complexity)]
pub fn quest_interrupt(
    mut commands: Commands,
    time: Res<Time>,
    players: Query<
        (Entity, &Transform, &Combatant, Has<crate::pets::Pet>),
        (
            Or<(With<Player>, With<crate::pets::Pet>)>,
            Without<Dead>,
            Without<MonsterAi>,
            Without<JoinedPack>,
        ),
    >,
    joined: Query<
        (Entity, &Transform, &Combatant, &JoinedPack),
        (Without<Dead>, Without<MonsterAi>),
    >,
    mut q: Query<
        (
            Entity,
            &Transform,
            &Combatant,
            Option<&QuestMove>,
            &mut MonsterAi,
            Has<Allied>,
        ),
        Without<Dead>,
    >,
    sleepers: Sleepers,
) {
    if !q
        .iter()
        .any(|(.., mv, _, _)| mv.is_some_and(|m| m.interruptible))
    {
        return;
    }
    let foes: Vec<(Entity, Vec3, f32)> = players
        .iter()
        .map(|(e, t, c, _)| (e, t.translation, c.radius))
        .collect();
    let pets: Vec<Entity> = players.iter().filter(|p| p.3).map(|p| p.0).collect();
    let mut packs: Vec<Option<std::sync::Arc<crate::monsters::FactionPack>>> = Vec::new();
    let mut rivals: Vec<_> = q
        .iter()
        .filter(|(.., ai, _)| ai.as_target.targetable)
        .map(|(e, tf, cb, _, ai, _)| {
            (
                e,
                tf.translation,
                cb.radius,
                pack_index(&mut packs, &ai.faction),
            )
        })
        .collect();
    rivals.extend(joined.iter().map(|(e, tf, cb, j)| {
        (
            e,
            tf.translation,
            cb.radius,
            pack_index(&mut packs, &j.pack),
        )
    }));
    let mut lists = FoeLists::new(&foes, packs, rivals);
    for (e, tf, _, mv, mut ai, allied) in &mut q {
        if !mv.is_some_and(|m| m.interruptible) {
            continue;
        }
        let side = lists.side(&ai.faction, allied);
        find_enemy(
            &mut ai,
            tf.translation,
            lists.get(side),
            &pets,
            |e| asleep(&sleepers, e),
            time.delta_secs(),
        );
        ai.flee.request = None;
        if ai.enemy.is_some() {
            ai.home = tf.translation;
            ai.path.clear();
            commands.entity(e).remove::<QuestMove>();
        }
    }
}

/// Walks the `QuestMove` paths of story characters. The move ends at the end of its path, never because a
/// frame did not move it (a paused clock, a frozen walker).
pub fn quest_walk(
    mut commands: Commands,
    time: Res<Time>,
    nav: Res<Nav>,
    mut q: Query<
        (
            Entity,
            &mut Transform,
            &mut QuestMove,
            &mut Character,
            Option<&mut MonsterAi>,
            Option<&mut crate::movement::CrowdAgent>,
        ),
        (Without<Dead>, Without<crate::casting::Casting>),
    >,
) {
    let dt = time.delta_secs();
    for (e, mut tf, mut mv, mut ch, ai, mut agent) in &mut q {
        // A swing or cast in progress finishes first, and a knockdown's get-up.
        if matches!(
            ch.state,
            CharState::Attack | CharState::Cast | CharState::KnockedDown | CharState::GettingUp
        ) {
            continue;
        }
        let speed = run_speed(&ch);
        let moving = advance_steered(
            &mut tf,
            &mut mv.path,
            speed,
            dt,
            &mut ch.turn,
            agent.as_deref_mut(),
        );
        if let Some(g) = nav
            .0
            .ground([tf.translation.x, tf.translation.y + 0.5, tf.translation.z])
        {
            tf.translation.y = g;
        }
        let arrived = mv.path.is_empty();
        let want = if moving && !arrived {
            CharState::Run
        } else {
            CharState::Idle
        };
        if ch.state != want {
            ch.state = want;
        }
        if arrived {
            if let Some(mut ai) = ai {
                ai.home = tf.translation;
                ai.path.clear();
            }
            commands.entity(e).remove::<QuestMove>();
        }
    }
}

impl MonsterAi {
    /// `Monster::EnableSpawnAnimation` @0x18037e820 (Monster vtable +0x630) on a monster about to be added to
    /// the world ([`SpawnStartup`]). Callers, each right before `World::AddEntity` / `FastSpawnEntity`
    /// and on every spawned monster: `ProxyAmbush::PlaceNextObject`, `ProxyMenu::PlaceNextObject`,
    /// `Skill_SpawnPet::SpawnPet` @0x180509450 and the `Skill_TargetedSpawnPet` / `SkillSecondary_TargetedSpawnPet`
    /// / `Skill_AttackRadiusLightningSpawnPet` / `Skill_AttackSpellChaosSpawnPet` overrides,
    /// `Skill_MonsterGenerator::SkillSpawnObject` @0x1804fd560, `FactionPack::IncrementNemesisSpawnCounter`
    /// @0x1802a5630 (a nemesis) and `GameEngine::OnCreatureDeath` @0x1802eff40 (the Dread). A hidden monster's
    /// Startup is its Hidden state instead ([`Hidden`]), so it keeps that.
    pub fn enable_spawn_animation(&mut self) {
        if self.hidden.is_none() {
            self.startup = SpawnStartup::Pending;
        }
    }

    /// `MaxYViewDistance`.
    pub fn view_y(&self) -> f32 {
        self.view_y
    }

    pub fn clear_path(&mut self) {
        self.path.clear();
    }

    /// `Character::TeleportHome` (sub_140051960): the controller's
    /// anger list is cleared and it returns to the idle state.
    pub fn go_idle(&mut self) {
        self.anger.clear();
        self.enemy = None;
        self.aggro = false;
        self.path.clear();
        self.patrol.interrupt();
    }
}

/// `Character` distress-call settings and state (see [`anger_from_hits`]).
#[derive(Clone, Debug, Default)]
pub struct Distress {
    pub(super) on: bool,
    pub(super) range: f32,
    pub(super) interval: f32,
    pub(super) calls_left: i32,
    pub(super) timer: f32,
}

impl MonsterAi {
    /// The end of the move under way (the crowd agent's target, `crate::movement::crowd_update`).
    pub fn path_target(&self) -> Option<Vec3> {
        self.path.last().copied()
    }

    /// `AngerManager::GetAngerDiff` @0x18001db60: `e`'s anger now less its value at the last
    /// `AngerManager::Update` before forgiving ([`MonsterAi::anger_base`]; an entry added since: all of it), i.e.
    /// the anger gained since the last `AngerUpdate` tick less that tick's forgiving.
    pub(super) fn anger_diff(&self, e: Entity) -> f32 {
        let of = |l: &[(Entity, f32)]| l.iter().find(|a| a.0 == e).map_or(0.0, |a| a.1);
        of(&self.anger) - of(&self.anger_base)
    }

    /// The `AngerUpdate` tick's `AngerManager::Update` left the entries (`forgive`: what it forgave): the
    /// [`MonsterAi::anger_diff`] base.
    pub(super) fn anger_ticked(&mut self, forgive: f32) {
        self.anger_base.clear();
        self.anger_base
            .extend(self.anger.iter().map(|a| (a.0, a.1 + forgive)));
    }

    /// In the temporary state "AlertBeforePursue" ([`Alert`]): its event handlers are empty.
    pub fn alerting(&self) -> bool {
        self.alert.on.is_some()
    }

    /// `AngerManager::AddAnger` @0x18001dfa0: capped at 100; the
    /// other entries lose 70% of the amount split among the list. The
    /// amount already includes the foe's `angerMultiplier` (its record; 1
    /// for players, pets keep theirs in [`crate::pets::Pet`]).
    pub fn add_anger(&mut self, who: Entity, amount: f32) {
        add_anger(&mut self.anger, who, amount);
    }

    /// `ControllerMonster::InPursuitRange` @0x18013b910: |Δy| ≤
    /// `MaxYViewDistance` from the monster (`me`) when set, and closer than
    /// `MaxPursuitDistance` to the monster's home when that is set.
    pub fn in_pursuit(&self, me: Vec3, p: Vec3) -> bool {
        // `GetHomePosition` @0x180137da0: the Patrol state's
        // `UsePathPositionAsHome` makes the monster's own position its home;
        // a monster with a leader (`JoinMe`) has the leader's spot.
        let home = if self.patrol.active() && !self.aggro {
            me
        } else {
            self.patrol.leader_pos.unwrap_or(self.home)
        };
        (self.view_y <= 0.0 || (p.y - me.y).abs() <= self.view_y)
            && (self.pursuit <= 0.0 || home.distance(p) < self.pursuit)
    }

    pub fn from_record(data: &GameData, record: &str, home: Vec3) -> Self {
        let rec = data.db.get(record);
        let ctl = rec
            .as_ref()
            .and_then(|r| r.str("controller").and_then(|c| data.db.get(c)));
        let f = |k: &str, d: f32| ctl.as_ref().and_then(|c| c.f32(k)).unwrap_or(d);
        let rf = |k: &str, d: f32| rec.as_ref().and_then(|r| r.f32(k)).unwrap_or(d);
        // A record bool defaulting to 1.
        let rb = |k: &str| rec.as_ref().is_none_or(|r| r.get(k).is_none() || r.bool(k));
        // `ControllerMonster::Load` @0x180135e60 and `Character::Load`
        // @0x180054e70 defaults (the defaults the loader passes the
        // record getters, e.g. `SightAngerRate` / `InnerSightAngerRate` /
        // `ProjectileAnger` 20, `InnerViewDistance` 5, `AttackedAnger` 100,
        // `AllyAttackedAnger` 1, `ForgiveRate` 50, `AngerTolerance` 10,
        // `MaxYViewDistance` / `MaxPursuitDistance` / swing pauses 0; stock
        // controllers nearly all set them).
        let respond_behavior = match ctl
            .as_ref()
            .and_then(|c| c.str("DistressResponseBehavior"))
            .unwrap_or("RespondToAll")
        {
            "RespondToSameGroup" => 0,
            "RespondToSameRace" => 1,
            _ => 2,
        };
        let class = ControllerClass::of(ctl.as_ref().and_then(|c| c.str("Class")));
        Self {
            home,
            class,
            hidden: (class == ControllerClass::Hidden).then(|| Hidden {
                appear: f("appearDistance", 5.0),
                target: None,
                dissolve_ms: rec
                    .as_ref()
                    .and_then(|r| r.i32("ambushDissolveTime"))
                    .unwrap_or(0),
            }),
            startup: SpawnStartup::Done,
            dodge: crate::dodge::Dodge::from_controller(ctl),
            rolls: crate::anger_rolls::AngerRolls::from_controller(ctl),
            lead: crate::lead::chances(ctl),
            reposition_chance: ctl
                .as_ref()
                .and_then(|c| c.i32("RepositionChance"))
                .unwrap_or(0),
            view: f("ViewDistance", 15.0),
            view_y: f("MaxYViewDistance", 0.0),
            pursuit: f("MaxPursuitDistance", 0.0),
            swing_pause: (f("minSwingPause", 0.0), f("maxSwingPause", 0.0)),
            aggro: false,
            path: Vec::new(),
            anger: Vec::new(),
            anger_cfg: (
                f("AttackedAnger", 100.0),
                f("petAngerTransference", 0.0) * 0.01,
            ),
            sight: (
                f("InnerViewDistance", 5.0),
                f("SightAngerRate", 20.0),
                f("InnerSightAngerRate", 20.0),
            ),
            forgive: f("ForgiveRate", 50.0),
            anger_timer: 0.0,
            anger_base: Vec::new(),
            alert: Alert::of(rec),
            tolerance: f("AngerTolerance", 10.0),
            ignore_sleeping: ctl
                .as_ref()
                .is_some_and(|c| c.bool("ignoreSleepingEnemies")),
            ally_anger: f("AllyAttackedAnger", 1.0),
            respond: f("ChanceToRespondToDistressCall", 100.0),
            respond_behavior,
            race: rec
                .map(|r| {
                    r.strs_iter("characterRacialProfile")
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            pathing_view: f("pathingViewDistance", 0.0),
            distress: Distress {
                on: rec
                    .as_ref()
                    .and_then(|r| {
                        r.get("distressCallWhenDying")
                            .map(|_| r.bool("distressCallWhenDying"))
                    })
                    .unwrap_or(true),
                range: rf("distressCallRange", 0.0),
                interval: rf("distressCallTime", 5000.0) / 1000.0,
                calls_left: rec
                    .as_ref()
                    .and_then(|r| r.i32("maxDistressCalls"))
                    .unwrap_or(1),
                timer: 0.0,
            },
            // `ControllerMonster::ControllerMonster`: `rand() % 251` ms.
            anger_tick: crate::rng::u32(..251) as f32 / 1000.0,
            find_timer: 0.0,
            enemy: None,
            faction: crate::monsters::FactionPack::shared(data, record),
            as_target: AsTarget {
                causes_anger: rb("causesAnger"),
                targetable: rb("targetable"),
                // PORT: absent, 1 (the LoadTable float default is not checked; stock creature records carry it).
                anger_multiplier: rf("angerMultiplier", 1.0),
            },
            flee: crate::flee::Flee::from_controller(ctl, rec),
            reposition: None,
            repos_pathed: false,
            give_up: false,
            returning: false,
            walk: crate::melee::Walk::of(rec),
            melee: crate::melee::MeleeAi::new(
                ctl.as_ref()
                    .and_then(|c| c.i32("PursuitTime"))
                    .unwrap_or(10000),
                ctl.as_ref()
                    .and_then(|c| c.i32("randomRepositionChance"))
                    .unwrap_or(0),
            ),
            patrol: crate::patrol::PatrolState::new(crate::patrol::Settings {
                roam: ctl.as_ref().and_then(|c| c.str("RoamBehavior")) == Some("Roam"),
                idle_chance: ctl
                    .as_ref()
                    .and_then(|c| c.i32("ChanceToIdleOnPatrol"))
                    .unwrap_or(20)
                    .max(0) as u32,
                idle_ms: (
                    ctl.as_ref()
                        .and_then(|c| c.i32("MinPatrolIdleTime"))
                        .unwrap_or(2000),
                    ctl.as_ref()
                        .and_then(|c| c.i32("MaxPatrolIdleTime"))
                        .unwrap_or(5000),
                ),
                roam_wait_ms: (
                    ctl.as_ref()
                        .and_then(|c| c.i32("MinTimeBeforeRoam"))
                        .unwrap_or(3000),
                    ctl.as_ref()
                        .and_then(|c| c.i32("MaxTimeBeforeRoam"))
                        .unwrap_or(10000),
                ),
                roam_dist: f("RoamDistance", 10.0),
                min_roam: f("MinRoamDistance", 2.0),
                wander_dist: f("WanderDistance", 5.0),
                min_wander: f("MinWanderDistance", 2.0),
                teleport: f("TeleportToLeaderDistance", 20.0),
                leader: crate::patrol::LeaderBehavior::parse(
                    ctl.as_ref()
                        .and_then(|c| c.str("LeaderBehavior"))
                        .unwrap_or("NeverLead"),
                ),
                leader_dist: f("LeaderDistance", 5.0),
                max_followers: ctl
                    .as_ref()
                    .and_then(|c| c.i32("MaxFollowers"))
                    .unwrap_or(4)
                    .max(0) as u32,
            }),
        }
    }
}

/// `ControllerMonsterStateFlee::DoFlee` (flee.rs): picks the point and
/// runs there, or ends the state at once.
#[allow(clippy::too_many_arguments)]
pub(super) fn do_flee(
    ai: &mut MonsterAi,
    me: Vec3,
    radius: f32,
    enemy: Entity,
    foes: &[(Entity, Vec3, f32)],
    friends: &[(Vec3, f32)],
    nav: &Nav,
    scratch: &mut gd_sim::nav::PathScratch,
) {
    let home = if ai.patrol.active() && !ai.aggro {
        me
    } else {
        ai.patrol.leader_pos.unwrap_or(ai.home)
    };
    let dest = crate::flee::enemy_pos(foes, enemy).map(|ep| {
        ai.flee
            .pick_point(me, radius, ep, home, ai.patrol.settings.roam_dist, friends)
    });
    let path = dest
        .filter(|d| (*d - me).length_squared() > 0.000_625)
        .and_then(|d| {
            nav.0.find_path(
                scratch,
                me.to_array(),
                d.to_array(),
                crate::nav::PATH_BUDGET,
            )
        });
    match path {
        Some(p) => {
            ai.path = p.into_iter().skip(1).map(Vec3::from).collect();
            ai.flee.arm();
        }
        None => end_flee(ai, me),
    }
}

/// `ControllerMonsterStateFlee::OnEnd` (flee.rs): back to "Idle".
pub(super) fn end_flee(ai: &mut MonsterAi, me: Vec3) {
    let (clear, reset) = ai.flee.end();
    ai.path.clear();
    if clear {
        ai.anger.clear();
        ai.enemy = None;
    }
    if reset {
        ai.home = me;
    }
}

/// Moves `tf` along `path` at `speed` m/s for `dt` s; true while moving. The character turns toward where it
/// goes (`turn`: `CrowdAgentMoved`, `crate::turning`).
fn advance(
    tf: &mut Transform,
    path: &mut Vec<Vec3>,
    speed: f32,
    dt: f32,
    turn: &mut crate::turning::Turn,
) -> bool {
    let mut moved = false;
    let mut budget = speed * dt;
    while budget > 0.0 && !path.is_empty() {
        let to = Vec3::new(
            path[0].x - tf.translation.x,
            0.0,
            path[0].z - tf.translation.z,
        );
        let d = to.length();
        if d <= budget {
            tf.translation.x = path[0].x;
            tf.translation.z = path[0].z;
            path.remove(0);
            budget -= d;
        } else {
            tf.translation += to / d * budget;
            budget = 0.0;
        }
        if d > 1e-3 {
            turn.moved(to / d * speed);
        }
        moved = true;
    }
    moved
}

/// A crowd agent's move along `path` this frame (Engine.dll crowd update sub_18025C440): the desired
/// velocity (toward the path's next corner × `speed`, the agent's top speed, params +8) plus the separation
/// [`crate::movement::crowd_begin`] computed for it from where its neighbours stood, clamped to `speed`
/// (`gd_sim::crowd::clamp_len`), integrated over `dt`: the AI and the crowd agree on one displacement. Without a
/// separation, or once the corner is within this frame's step (the arrival), it follows the path exactly
/// ([`advance`]); the crowd puts a steered agent back on the mesh (`crowd_update`). True while moving; the
/// character turns toward its velocity (`CrowdAgentMoved`, `crate::turning`). `agent`: none for a character
/// without one (it moves unsteered).
pub fn advance_steered(
    tf: &mut Transform,
    path: &mut Vec<Vec3>,
    speed: f32,
    dt: f32,
    turn: &mut crate::turning::Turn,
    agent: Option<&mut crate::movement::CrowdAgent>,
) -> bool {
    if let Some(a) = agent {
        a.speed = speed;
        let sep = a.separation;
        if sep != Vec3::ZERO && speed > 0.0 && dt > 0.0 && !path.is_empty() {
            let to = (path[0] - tf.translation).with_y(0.0);
            let d = to.length();
            if d > speed * dt {
                let dv = to / d * speed;
                let v = gd_sim::crowd::clamp_len([dv.x + sep.x, dv.z + sep.z], speed);
                let v = Vec3::new(v[0], 0.0, v[1]);
                tf.translation += v * dt;
                a.steered = true;
                if v.length_squared() > 1e-6 {
                    turn.moved(v);
                }
                return true;
            }
        }
    }
    advance(tf, path, speed, dt, turn)
}

/// The one run speed of every mover (the player's path, the monsters', pets' and spirits' AI moves): the
/// character's root motion while its Run clip carries it (`Character::CrowdAgentUpdate` @0x180066d60:
/// the crowd's top speed is `Entity::GetMotion`, the playing animation's root velocity, blended as the clip fades
/// in), and before it does (the state just set, the clip not advanced yet) the Run slot's own root-motion speed
/// ([`Character::move_motion`]) at the run speed and run slow it plays at; 0 while frozen. No fixed base speed.
pub fn run_speed(ch: &Character) -> f32 {
    move_speed(ch, CharState::Run, ch.run_speed)
}

/// A `WalkAction`'s speed: [`run_speed`]'s rule on the Walk slot (`Character::WalkTo` plays it at the run
/// speed).
pub fn walk_speed(ch: &Character) -> f32 {
    move_speed(ch, CharState::Walk, ch.run_speed)
}

/// A charge's move: its speed, state and clip caller speed ([`Character::move_rate`]). The move starts its clip
/// (`state`) at speed factor 1 (`Character::MoveTo` @0x18005f870 with factor 1.0 from
/// `ControllerPlayerStateChargeToUseSkill::OnBegin` @0x180167ef0 and from `ControllerMonsterStatePursue::OnBegin`
/// @0x18013f6d0 for move type 2: the Charge animation 7). A warmed-up one (`warm`: the skill's
/// `characterRunSpeedModifier` % and `absoluteRunSpeedCapMax` %) moves at the warmed-up run speed: the skill's
/// `WarmUpStart` on the move (`MoveToAction::Execute` @0x180082c00, the move carrying the skill: vtable 214) sets
/// the warm-up flag (Character +9907) and `Character::ForceSpeedUpdate` @0x180068ca0 re-adjusts the moving clip to
/// `GetRunSpeed`, which now counts the skill's bonus (`Skill_AttackWeapon::CollectPassiveCharAttributes` adds the
/// profile's stats while the flag is on) under `absoluteRunSpeedCapMax` instead of the player cap
/// (`CapAbsoluteRunSpeed`; `gd_sim::skills::warmup_run_multiplier`). The clip is the one the move
/// started: the player's ChargeToUseSkill and a path charge the Charge slot, an AI's Pursue to a weapon charge
/// (move type 1, animation 5 at `GetRunSpeed`) its
/// pace's Run or Walk slot. Its root motion at that rate carries the character ([`move_speed`]). A set without the
/// slot does not move (`Character::MoveTo`: `PlayAnimationIfAvailable` fails, the move stops; Idle). PORT: Run and
/// Walk stand in for each other, as everywhere the port plays them (`Character::enter_state`, [`move_speed`]): only
/// a set with neither does not move. The warm-up
/// ends with the move (`MoveToAction::Finish` @0x180082ef0: the skill's `WarmUpEnd`, the flag off,
/// `ForceSpeedUpdate`).
pub fn charge_move(
    ch: &Character,
    state: CharState,
    warm: Option<(f32, f32)>,
) -> (f32, CharState, f32) {
    use gd_sim::anim::AnimType;
    let rate = warm.map_or(1.0, |(bonus, cap)| {
        gd_sim::skills::warmup_run_multiplier(ch.run_speed, bonus, cap)
    });
    let playable = match state {
        CharState::Charge => ch.anims.has(AnimType::Charge),
        _ => ch.anims.has(AnimType::Run) || ch.anims.has(AnimType::Walk),
    };
    if !playable {
        return (0.0, CharState::Idle, rate);
    }
    (move_speed(ch, state, rate), state, rate)
}

/// The ground speed of the move `state`'s clip at caller speed `rate`: the playing clip's root motion, scaled
/// from the caller speed it plays at to `rate`; before it plays, its slot's own motion at `rate` and the run slow.
fn move_speed(ch: &Character, state: CharState, rate: f32) -> f32 {
    if ch.frozen {
        return 0.0;
    }
    let rm = Vec3::new(ch.root_velocity.x, 0.0, ch.root_velocity.z).length();
    match ch.move_clip_rate() {
        Some(playing) if ch.state == state && rm > 0.0 && playing > 0.0 => rm * rate / playing,
        _ => ch.move_motion(state) * rate * ch.slow[0],
    }
}

/// The speed, state and clip caller speed ([`Character::move_rate`], `None`: the run speed) of a
/// `ControllerAI::MoveTo` at a [`Pace`] (`Pursue::OnBegin`): the Run animation at the run speed
/// (`GetRunSpeed`) or at unit speed (the speed factor 1), the Charge animation at 1, or the Walk animation
/// (the Run one with `walkUsesRun`, animation type 6 - flag) at `walkSpeed` × the run speed. `Character::MoveTo`
/// @0x18005f870 plays the clip at that factor and the clip's root motion carries the character, so the
/// speed is that clip's motion at that factor ([`move_speed`]): the feet keep pace with the ground.
pub fn pace_speed(
    ch: &Character,
    pace: crate::melee::Pace,
    walk: &crate::melee::Walk,
) -> (f32, CharState, Option<f32>) {
    use crate::melee::Pace;
    let walk_rate = walk.speed * ch.run_speed;
    match pace {
        Pace::Run => (run_speed(ch), CharState::Run, None),
        Pace::Unit => (
            move_speed(ch, CharState::Run, 1.0),
            CharState::Run,
            Some(1.0),
        ),
        Pace::Charge => {
            let (speed, state, rate) = charge_move(ch, CharState::Charge, None);
            (speed, state, (state != CharState::Idle).then_some(rate))
        }
        Pace::Walk if walk.uses_run => (
            move_speed(ch, CharState::Run, walk_rate),
            CharState::Run,
            Some(walk_rate),
        ),
        Pace::Walk => (
            move_speed(ch, CharState::Walk, walk_rate),
            CharState::Walk,
            Some(walk_rate),
        ),
    }
}

/// `AngerManager::AddAnger` @0x18001dfa0 on an anger list: capped at 100; the other entries lose 70%
/// of the amount split among the list (monsters, [`MonsterAi::add_anger`], and pets).
pub fn add_anger(list: &mut Vec<(Entity, f32)>, who: Entity, amount: f32) {
    if amount <= 0.0 {
        return;
    }
    match list.iter_mut().find(|a| a.0 == who) {
        Some(a) => a.1 = (a.1 + amount).min(100.0),
        None => list.push((who, amount.min(100.0))),
    }
    let n = list.len();
    if n > 1 {
        let cut = amount / n as f32 * 0.7;
        for a in list.iter_mut().filter(|a| a.0 != who) {
            a.1 -= cut;
        }
    }
    list.retain(|a| a.1 >= 0.0);
}
