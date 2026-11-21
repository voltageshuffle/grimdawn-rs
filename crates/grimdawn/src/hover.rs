//! Hovering a usable object (chest, door, devotion shrine, riftgate) or a
//! character (monster, NPC, pet): the hover target gets an outline and the
//! `UIActorDescription` widget at the top centre of the screen describes it.
//!
//! Target (Grim Dawn.exe frame update sub_14002BC00 → sub_14010F2B0
//! → sub_14010F470), in this order:
//! 1. an unopened container under the cursor (`FixedItemContainer`,
//!    `IsOfInterest`: state 1, closed: `Chest::is_of_interest`);
//! 2. an enemy under the cursor: of the enemies the cursor touches, the one
//!    nearest to the player (sub_140032100); it also becomes the combat
//!    enemy. The list (sub_1400318C0) takes `Destructible`s that are
//!    `IsTargetable` (record `targetable`, not broken) too, so a targetable
//!    breakable is hovered (and clicked) like an enemy. One pick feeds the hover and the attack
//!    (`combat::pick_enemy`: the posed body box under the ray, else near it, the touched enemy nearest to the
//!    player; the candidate margin widens while a held click is pathing / repeated, sub_140021000);
//! 3. a non-enemy character under the cursor: an `Npc` (record `Class`
//!    `Npc*`) only while it is available for conversations
//!    (`Npc::IsAvailableForConversations`: the story object's flag; level
//!    NPCs without story hooks count as available, PORT) and visible;
//!    friendly monsters always; the player's own pets only while the Target
//!    Pet key is held; with them a described decoration (a direction sign:
//!    `gd_game::world::Described`, its parts' mesh boxes under the ray), named
//!    by its `description` and outlined in the object colour;
//! 4. another fixed item of interest: doors (`gd_game::doors` module docs),
//!    devotion shrines / totems (`Shrine::is_of_interest`: not during their
//!    fight, a totem not once cleared), riftgates (`Waypoint::is_of_interest`:
//!    not locked), personal portals and the Shattered Realm's exit gates (both named
//!    `tagRiftgatePersonal`), its unused buff shrines.
//!
//! The list above is the gamepad / WASD path; in Keyboard Only (`movementType` 2) the only thing it touches is the
//! controller targeting's pick (`crate::controller_target`: not a ground item, whose label is selected instead).
//! PORT: the mouse path (sub_140021000) keeps its filters but is not ported
//! as such: it collects what is near the cursor (sub_14002FF80) and takes the
//! mesh under the ray, else the one nearest to it (sub_140030D80), with no
//! fixed order of kinds.
//! What it drops, sub_14002FF80: the main player (never outlined:
//! sub_14010F350 only clears the outline), characters failing its virtual
//! +1120 / +1152 / +1160 checks, and a `Pet` that is no foe of the player
//! unless its argument is 0, which is byte +2121 of the input
//! object, set while key action 38 Target Pet is held (sub_140026910;
//! `tagKeybind35`, "Target Pet (Hold Key and Click)"). So a summon is neither
//! named nor outlined until that key is down, and then it is `friendOutline`.
//! All of this needs a main player, so nothing is hovered on the main menu,
//! whose character is a paper doll (`doll::DollBody`).
//!
//! Outline (sub_14010F470, with the `targetOutline` option, bool 22, on by
//! default in `Options::Options`): fixed items get `objectOutlineColor`;
//! characters `friendOutlineColor` when `FactionManager::IsFriend(player
//! pack, GetOriginalFaction, pack)`, else `mindControlledOutlineColor` when
//! their current faction is a friend, else `enemyOutlineColor`; pets are
//! friends of their owner. Here foes (`gd_game::combat::Side::Player`'s foes: a `Monster` without `Allied`
//! (the friendly packs, `gd_game::monsters`, and faction changes, `gd_game::factions`), a pet a monster's
//! convert joined to its pack) are enemies and every other character (pets, NPCs,
//! `Allied` monsters such as guards) a friend (no mind control). An NPC is outlined only while it is "of interest"
//! (`Npc::IsOfInterest`: it has a conversation and is available, or it is a
//! boat master; every `Npc` subclass — merchant, crafter, … — overrides it to
//! true); a `Destructible` only while `IsTargetable`, in the object
//! colour with the mouse (the enemy colour with a gamepad, input mode ≠ 0). Colours: `records/game/gameengine.dbr`
//! (`GameEngine::LoadFromDatabase`): object (0.5, 0.5, 1), friend (0, 1, 0),
//! enemy (1, 0, 0).
//!
//! The outline itself (Engine.dll `GraphicsDeferredRenderer::
//! RenderFinalScene` @0x1800adeb0; details in `render/post.rs`): the
//! outlined meshes are drawn flat in `outlineColor` into a separate target,
//! blurred by `ApplyGlow(target, 1.25, 1.0)` and added over the scene except
//! on the object's own visible pixels: a soft halo of about 4 px around it.
//! The halo is added to whatever lies beside those pixels, so it falls on the
//! edge of something in front of the object too: the engine does that
//! (stencil 255 is the only exclusion; `shots/real/trainingdummy.png`).
//! Here each outlined part gets a copy in the glow view only ([`OutlineMask`],
//! `GdMaterial::outline_mask`: its palette class where it passes the depth
//! test) and [`OutlineGlow`] carries the classes' colours to the composite;
//! with nothing outlined no copy exists. Besides the hover target the label
//! manager outlines the ground items whose label is listed and, brighter,
//! the selected one ([`ITEM_OUTLINE`], [`SELECTED_ITEM_OUTLINE`]): an
//! actor has one outline colour, so each class is one palette entry.
//!
//! The widget (`records/ui/inworld/actordescription.dbr`, loader
//! sub_14010DA60, update sub_14010E9B0, draw sub_14010E2B0 from the game
//! window's draw sub_140027350). The anchor is the top centre of the
//! viewport: x = viewport x + width / 2, y = 1 px (not UI-scaled); each part
//! is centred on the anchor x (texts by their text width, bitmaps by theirs)
//! plus its own record offset (scaled). Drawn in order:
//! - name (`actor_name.dbr`, y 0, `style_special_enemyname`): a fixed
//!   item's `description` (a personal portal: `tagRiftgatePersonal`,
//!   `FixedItemTeleport::GetGameDescription` @0x1802b5df0: the tag localized
//!   with the owner's name as its parameter, which stock text does not use); an NPC's `Npc::GetRolloverDescription`
//!   (`description` + " - " + `professionTag` when set); other characters'
//!   `Monster::GetGameDescription`: the classification's colour code
//!   (`GameEngine::GetMonsterColorText`, gameengine.dbr: Common `{^w}`,
//!   Champion `{^y}`, Hero `{^o}`, Boss / SuperBoss `{^r}`, Quest `{^p}`)
//!   before the `description`;
//! - for objects and NPCs (+2120 set: `Npc` or `Decoration`) nothing else,
//!   except a `Destructible` (its `GetGameDescription`: the `description`):
//!   `healthBar` at `GetLife` / `GetMaxLife` and the tier-1 border (no race,
//!   no level; PORT: the fill clamp is the characters' 0.02…1);
//! - for other characters: race (`actor_race.dbr`, y 75,
//!   `style_special_enemyrace`; `Monster::GetRaceText`: the
//!   `characterRacialProfile` entries' `tag<Race>` joined by " - ",
//!   `noTagUnknownRace` for an unknown one), drawn when not empty;
//! - for a `Monster` (pets too) or a player: the health bar, filled to
//!   clamp(life / max, 0.02, 1) of the display range: `healthBarBoss` when
//!   the classification is Boss or above (≥ 3) and the canvas is at least
//!   1920 px wide, else `healthBar` (animated, 30 fps, y 58); then the tier
//!   border by classification: Common tier 1, Champion 2, Hero 3, Quest 4,
//!   Boss / SuperBoss 5 (Quest and Boss fall back to tier 3 below 1920 px;
//!   y 31; a player gets tier 1); status icons only with
//!   `monsterBarsDebuffs` (bool 58, off: the strip, `debuff_strip`);
//! - the level (`actor_level.dbr`, y 27, `style_special_enemylvl`): the
//!   character level as a bare number, coloured by its difference to the
//!   player's level d: d < −2 `levelDiffTier6`, −2…−1 tier 5, 0…1 tier 4,
//!   2…3 tier 3, 4…5 tier 2, ≥ 6 tier 1; drawn for players always and for
//!   other characters with `showMonsterLevelOnRollover` (bool 5, on by
//!   default: `Options::Options` leaves the bit at the array's `true`).
//!
//! The widget's fade (+2121…+2128: alpha 0.3 → 1 over 250 ms) never runs:
//! only its constructor (sub_14010D810) writes the fade flags (to 0), so the
//! widget always draws at full alpha (the writes to +2121/+2122/
//! +2123 in Grim Dawn.exe).
//!
//! PORT (a performance choice): the widget is built when what it shows changes (target, level, strip, window), not
//! for the target's life: the bar's fill follows it in place (`HoverFill`, `animate_fill`).
//!
//! Test hook: `GD_HOVER=x,y` (logical px; `@dx,dy` from the window centre)
//! stands in for the mouse, as for item tooltips; `GD_HOVER=monster` /
//! `GD_HOVER=npc` / `GD_HOVER=breakable` / `GD_HOVER=decoration` points it at
//! the nearest foe / NPC / targetable breakable (1 m up) / described
//! decoration (2 m up).
use crate::data::GameData;
use crate::record_facts::{RecordFacts, icon_override};
use crate::ui::{Ui, text::FontCache};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::{BarGraph, Rect as URect, TextBox, Widget};
use gd_render::hover::HoverGlow;
use gd_render::render::material::GdMaterial;
use gd_render::render::post::{GLOW_LAYER, OutlineGlow};
use gd_sim::bounds;
use std::collections::HashMap;
use std::sync::Arc;

const DESC: &str = "records/ui/inworld/actordescription.dbr";
const NAME_BOX: &str = "records/ui/inworld/actor_name.dbr";

pub struct HoverPlugin;

impl Plugin for HoverPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Hovered>()
            .init_resource::<HoverPoint>()
            .add_systems(
                Update,
                // `pick` runs in every state (the menu's cursor reads `Hovered`); the overlays are in-game only.
                (
                    (deco_spheres, pick).chain(),
                    (outline, describe, animate_fill)
                        .chain()
                        .in_set(crate::ui::UiPlaySet),
                )
                    .chain()
                    .after(crate::player::PlayerSet),
            );
        #[cfg(feature = "dev")]
        app.add_systems(
            Update,
            hook_debuff
                .after(pick)
                .before(outline)
                .in_set(crate::ui::UiPlaySet),
        );
    }
}

/// What the hover target is (decides the widget's parts).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Kind {
    /// A fixed item: name only.
    #[default]
    Object,
    /// An `Npc`: name only.
    Npc,
    /// Another character: name, race, health bar (when it has a life),
    /// tier border, level.
    Character,
    /// A `Destructible`: name, health bar, tier-1 border.
    Breakable,
}

/// What the world mouse handler (Grim Dawn.exe sub_140021000) marks for the
/// cursor about the hover target (`ui::cursor` reads it).
/// sub_140021000: `Attack` (game window +2106) when the object under the
/// cursor has an id and the controller's combat enemy is set (an enemy, a
/// targetable breakable); for a visible `Npc` that `IsOfInterest` (a plain
/// `Npc`: `Npc::IsOfInterest`; every `Npc` subclass's slot returns true),
/// `Merchant` (+2105) when its class is `NpcMerchant` (or derives from it:
/// no stock class does), else `Dialog` (+2104); then the actor's record
/// `interactIconOverride` (`Actor::Load`, `GetInteractIconOverride`):
/// "Dialog" clears the flags and sets `Dialog`, "Attack" sets `Attack`.
/// The combat enemy is not the hover target: sub_140021000 clears
/// it each event, picks the object under the cursor from the whole list
/// (sub_140030D80 over the frame's candidates) and, when there is one, sets
/// the combat enemy from the enemy list's pick (the touched enemy /
/// targetable breakable nearest to the player); so a container, door or
/// non-interest NPC that wins the hover over an enemy still gets `Attack`,
/// while an NPC's merchant / dialog marks outrank it in `ui::cursor::select`
/// ([`marks`]). While an enemy-target skill runs (`IsRunningSkill`, the running skill's `GetTargetType` 2),
/// an ally under the cursor drops the object: no hover target, no mark. With `targetLock` the held button keeps
/// the object ([`held_pick`]).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Interact {
    #[default]
    None,
    Attack,
    Dialog,
    Merchant,
}

/// What the world mouse handler remembers between events (sub_140021000): the object of the last event
/// (+2472: set to the picked object, cleared when none), whether the last left press issued a primary command
/// (+465: `HandleActionFromMouse` with the left button, not through the Move key; cleared once no button is held)
/// and the combat enemy of the last event (restored with a locked object).
#[derive(Default)]
struct HeldLock {
    remembered: Option<Entity>,
    primary: bool,
    enemy: bool,
}

/// The inputs of [`held_pick`].
struct HeldInput {
    /// Option `targetLock` ("Classic Targeting", bool 27).
    target_lock: bool,
    /// `SkillManager::IsRunningSkill`. PORT: a channel (`gd_game::casting::Channel`) is the running skill.
    running: bool,
    /// `IsCommandRepeated`: a world button held after its press, or the Move key held.
    repeated: bool,
    remembered: Option<Entity>,
    primary: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HeldPick {
    /// The usual pick under the cursor.
    Fresh,
    /// The remembered object stays the target, the combat enemy too (when it is still there).
    Locked(Entity),
    /// No object at all (no hover target, no mark).
    Nothing,
}

/// sub_140021000 (mouse input mode): with `targetLock`, no skill running, an object remembered and the
/// command repeated, the object stays the target (unless it is gone or a broken breakable) and the last event's
/// combat enemy is restored; else, with `targetLock`, no skill running and the last left press a primary command,
/// nothing is picked while the button is held. Otherwise the cursor picks as usual.
fn held_pick(i: &HeldInput) -> HeldPick {
    if !i.target_lock || i.running {
        return HeldPick::Fresh;
    }
    match i.remembered {
        Some(e) if i.repeated => HeldPick::Locked(e),
        _ if i.primary => HeldPick::Nothing,
        _ => HeldPick::Fresh,
    }
}

/// The pointer position the pick used this frame (logical px; the
/// `GD_HOVER` stand-in when set), `None` outside the window. Written by
/// [`pick`] only.
#[derive(Resource, Default, PartialEq, Clone, Copy, Debug)]
pub struct HoverPoint(pub Option<Vec2>);

/// Test hook `GD_DEBUFF_TARGET=<skill>`: the skill's debuff (as `casting::fire` lands a curse: the wrapper's
/// buff record, level 1, its `skillActiveDuration`) lands once on each character the pointer hovers, cast by
/// the player.
#[cfg(feature = "dev")]
fn hook_debuff(
    hooks: Res<crate::hooks::Hooks>,
    data: Res<GameData>,
    hovered: Res<Hovered>,
    player: Query<Entity, With<crate::player::Player>>,
    mut debufs: MessageWriter<gd_game::skill_buffs::DebufTarget>,
    (mut done, mut fresh): (Local<Vec<Entity>>, gd_game::session::NewSession),
) {
    if fresh.begun() {
        done.clear();
    }
    let (Some(skill), Some(target), Ok(caster)) = (
        hooks.ui.debuff_target.as_deref(),
        hovered.entity.filter(|_| hovered.kind == Kind::Character),
        player.single(),
    ) else {
        return;
    };
    if done.contains(&target) {
        return;
    }
    let Some(rec) = data.db.get(skill) else {
        return;
    };
    let buff = gd_sim::skills::profile_name(rec)
        .unwrap_or(skill)
        .to_owned();
    let Some(brec) = data.db.get(&buff) else {
        return;
    };
    done.push(target);
    debufs.write(gd_game::skill_buffs::DebufTarget {
        caster,
        target,
        buff,
        skill: skill.to_owned(),
        level_index: 0,
        mods: Vec::new(),
        devotion: 0,
        secs: (gd_game::casting::level_f(brec, "skillActiveDuration", 0) * 1000.0) as i32 as f32
            / 1000.0,
    });
}

/// The hover target and what the widget shows of it.
#[derive(Resource, Default, PartialEq, Clone, Debug)]
pub struct Hovered {
    pub entity: Option<Entity>,
    pub name: Arc<str>,
    pub kind: Kind,
    /// The cursor's mark for it.
    pub interact: Interact,
    /// The outline colour (`None`: not outlined).
    pub outline: Option<Vec3>,
    /// Race line (characters).
    pub race: Arc<str>,
    /// `monsterClassification` index (Common 0 … SuperBoss 5).
    pub classification: u8,
}

/// The hover target's [`Interact`]: its own mark (`base`), `Attack` added
/// when a combat enemy is under the cursor (`enemy`, whatever won the
/// hover), then the record's override ("Dialog" clears every flag, the
/// attack one too; "Attack" only adds the attack mark). Dialog / merchant
/// outrank attack in `ui::cursor::select`, so one value carries the flags.
fn marks(base: Interact, enemy: bool, icon: Option<Interact>) -> Interact {
    let base = if enemy && base == Interact::None {
        Interact::Attack
    } else {
        base
    };
    match (icon, base) {
        (Some(Interact::Dialog), _) => Interact::Dialog,
        (Some(Interact::Attack), Interact::None) => Interact::Attack,
        _ => base,
    }
}

#[derive(Component)]
struct NameRoot;

/// Distance along the ray to where it enters a box placed by `gt` (`b` in that frame: a mesh's box, a ground item's
/// model box): [`bounds::Aabb::ray_entry`] of the ray taken into the frame.
pub(crate) fn ray_box(ray: Ray3d, gt: &GlobalTransform, b: &bounds::Aabb) -> Option<f32> {
    let inv = gt.affine().inverse();
    // Not normalised: t stays the world distance along the ray.
    let t = b.ray_entry(
        inv.transform_point3(ray.origin).to_array(),
        inv.transform_vector3(*ray.direction).to_array(),
    );
    (t < f32::INFINITY).then_some(t)
}

/// What decides the hover target: the cursor ray, or in Keyboard Only the controller targeting's pick
/// (`crate::controller_target`), the only thing it touches.
#[derive(Clone, Copy)]
enum Probe {
    Ray(Ray3d),
    Pick(Entity),
}

impl Probe {
    /// [`bounds::ray_sphere`] of the ray; 0 for the picked entity.
    fn hit(self, e: Entity, centre: Vec3, r: f32) -> Option<f32> {
        match self {
            Probe::Ray(ray) => bounds::ray_sphere(
                ray.origin.to_array(),
                ray.direction.to_array(),
                centre.to_array(),
                r,
            ),
            Probe::Pick(p) => (p == e).then_some(0.0),
        }
    }
}

/// Nearest hit along the ray among `(entity, sphere centre, radius, record)`.
fn nearest<'a>(
    probe: Probe,
    items: impl Iterator<Item = (Entity, Vec3, f32, &'a str)>,
) -> Option<(Entity, &'a str)> {
    let mut best: Option<(f32, Entity, &str)> = None;
    for (e, centre, r, rec) in items {
        if let Some(t) = probe.hit(e, centre, r)
            && best.is_none_or(|b| t < b.0)
        {
            best = Some((t, e, rec));
        }
    }
    best.map(|b| (b.1, b.2))
}

/// Characters the player's side may attack are those [`gd_game::combat::Side::Player`] calls foes
/// (`FactionManager::IsFoe`): monsters without `Allied`, a pet a monster's convert joined to its pack.
type Foes<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static GlobalTransform,
        &'static crate::combat::Combatant,
        &'static gd_render::character::Character,
        gd_game::combat::SideData,
    ),
    (Without<crate::combat::Dead>, Without<crate::player::Player>),
>;
type Friends<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static GlobalTransform,
        &'static gd_render::character::Character,
        Option<&'static gd_game::story::StoryActor>,
        Option<&'static Visibility>,
        Option<&'static gd_game::pets::Pet>,
        gd_game::combat::SideData,
    ),
    (
        Without<crate::player::Player>,
        Without<crate::combat::Dead>,
        Without<crate::doll::DollBody>,
    ),
>;

/// Whether the player's side calls this character a foe (`Side::foe`, the one rule).
fn player_foe(sd: &gd_game::combat::SideDataItem) -> bool {
    gd_game::combat::Side::Player.foe(sd.side_ref())
}

/// A described decoration's parts as one world sphere: the ray is tested against its parts only when it passes
/// within it. PORT (a performance choice): the parts are spawned with the decoration (`world.rs`), so the sphere is
/// taken once they have their boxes; 1 m of margin covers a floating part's bob.
#[derive(Component)]
struct DecoSphere {
    centre: Vec3,
    radius: f32,
}

fn deco_spheres(
    mut commands: Commands,
    described: Query<Entity, (With<gd_game::world::Described>, Without<DecoSphere>)>,
    children: Query<&Children>,
    boxes: Query<(&GlobalTransform, &Aabb)>,
) {
    for e in &described {
        let (mut lo, mut hi) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
        for (gt, b) in children
            .iter_descendants(e)
            .filter_map(|c| boxes.get(c).ok())
        {
            for i in 0..8 {
                let s = Vec3::new(
                    if i & 1 == 0 { -1.0 } else { 1.0 },
                    if i & 2 == 0 { -1.0 } else { 1.0 },
                    if i & 4 == 0 { -1.0 } else { 1.0 },
                );
                let w = gt.transform_point(Vec3::from(b.center) + Vec3::from(b.half_extents) * s);
                (lo, hi) = (lo.min(w), hi.max(w));
            }
        }
        if lo.x <= hi.x {
            commands.entity(e).try_insert(DecoSphere {
                centre: (lo + hi) * 0.5,
                radius: (hi - lo).length() * 0.5 + 1.0,
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn pick(
    window: Query<&Window, With<PrimaryWindow>>,
    cams: Query<(&Camera, &GlobalTransform), With<gd_render::camera::GameCamera>>,
    capture: Res<crate::inventory::UiCapture>,
    hooks: Res<crate::hooks::Hooks>,
    data: Res<GameData>,
    fonts: Res<FontCache>,
    objects: (
        Query<(Entity, &GlobalTransform, &crate::chests::Chest)>,
        Query<(Entity, &GlobalTransform, &gd_game::doors::Door)>,
        Query<(Entity, &GlobalTransform, &gd_game::devotion::Shrine)>,
        Query<(Entity, &GlobalTransform, &gd_game::riftgates::Waypoint)>,
        Query<
            (
                Entity,
                &GlobalTransform,
                &crate::combat::Combatant,
                &gd_game::world::Breakable,
            ),
            Without<crate::combat::Dead>,
        >,
        Query<(Entity, &GlobalTransform, &gd_game::riftgates::Portal)>,
        Query<(Entity, &GlobalTransform, &gd_game::ascendant::Altar)>,
        (
            Query<(Entity, &GlobalTransform), With<gd_game::endless::EndlessGate>>,
            Query<(Entity, &GlobalTransform, &gd_game::endless::EndlessShrine)>,
        ),
        (
            Query<(
                Entity,
                &GlobalTransform,
                &gd_game::world::Described,
                Option<&DecoSphere>,
            )>,
            Query<&Children>,
            Query<(&GlobalTransform, &Aabb)>,
        ),
    ),
    devotion: Option<Res<gd_game::devotion::DevotionState>>,
    (foes, friends, player): (
        Foes,
        Friends,
        Query<
            (
                Entity,
                &GlobalTransform,
                &gd_game::movement::MovePath,
                &crate::combat::Combatant,
            ),
            With<crate::player::Player>,
        >,
    ),
    (keys, bindings): (Res<ButtonInput<KeyCode>>, Res<crate::ui::keys::KeyBindings>),
    rt: Option<NonSend<gd_game::story::StoryRuntime>>,
    (mut facts, mut lock, mut fresh): (
        ResMut<RecordFacts>,
        Local<HeldLock>,
        gd_game::session::NewSession,
    ),
    held: (
        Res<ButtonInput<MouseButton>>,
        Res<crate::casting::MoveKeyHeld>,
        Query<&gd_game::casting::Channel, With<crate::player::Player>>,
    ),
    (mut hovered, mut commands, glow): (ResMut<Hovered>, Commands, Query<Entity, With<HoverGlow>>),
    mut point: ResMut<HoverPoint>,
    (options, controller, pause): (
        Res<gd_game::options::GameOptions>,
        Res<crate::controller_target::ControllerPick>,
        Res<gd_game::pause::GamePause>,
    ),
) {
    let (
        chests,
        doors,
        shrines,
        gates,
        breakables,
        portals,
        altars,
        (exit_gates, endless_shrines),
        (described, children, boxes),
    ) = objects;
    if fresh.begun() {
        // The remembered object was the ended session's.
        *lock = HeldLock::default();
    }
    // A described decoration's hit: the nearest of its parts' mesh boxes along the ray.
    let deco_hit = |probe: Probe, e: Entity, sphere: Option<&DecoSphere>| match probe {
        Probe::Pick(p) => (p == e).then_some(0.0),
        Probe::Ray(ray) => {
            let s = sphere?;
            let v = s.centre - ray.origin;
            if (v - *ray.direction * v.dot(*ray.direction)).length() > s.radius {
                return None;
            }
            children
                .iter_descendants(e)
                .filter_map(|c| boxes.get(c).ok())
                .filter_map(|(gt, b)| {
                    let b = bounds::Aabb {
                        center: b.center.to_array(),
                        half: b.half_extents.to_array(),
                    };
                    ray_box(ray, gt, &b)
                })
                .min_by(f32::total_cmp)
        }
    };
    let Ok(win) = window.single() else { return };
    let facts = &mut *facts;
    let fonts = &*fonts;
    let [object_c, friend_c, enemy_c] = facts.colours(&data);
    let ppos = player.single().map_or(Vec3::ZERO, |p| p.1.translation());
    let target_pet_key = bindings.pressed(crate::ui::keys::TARGET_PET, &keys);
    let story = rt.as_ref().map(|rt| rt.story.borrow());
    // `Npc::IsAvailableForConversations` / `IsOfInterest` of a story NPC.
    let npc_state = |a: Option<&gd_game::story::StoryActor>| -> (bool, bool) {
        let Some(o) = a.and_then(|a| story.as_ref()?.entities.get(a.0)) else {
            return (true, false);
        };
        let available = o.in_world && o.available_for_conversation;
        (available, available && o.conversation.is_some())
    };
    let cursor = hooks
        .ui
        .hover
        .as_deref()
        .and_then(|v| {
            let cam = cams.single().ok()?;
            let project = |p: Vec3| cam.0.world_to_viewport(cam.1, p + Vec3::Y).ok();
            let near = |it: &mut dyn Iterator<Item = Vec3>| {
                it.min_by(|a, b| a.distance(ppos).total_cmp(&b.distance(ppos)))
            };
            match v {
                "monster" => near(
                    &mut foes
                        .iter()
                        .filter(|f| player_foe(&f.4))
                        .map(|f| f.1.translation()),
                )
                .and_then(project),
                "breakable" => near(
                    &mut breakables
                        .iter()
                        .filter(|b| b.3.targetable)
                        .map(|b| b.1.translation()),
                )
                .and_then(project),
                "decoration" => near(&mut described.iter().map(|d| d.1.translation()))
                    .and_then(|p| cam.0.world_to_viewport(cam.1, p + Vec3::Y * 2.0).ok()),
                "npc" => near(
                    &mut friends
                        .iter()
                        .filter(|f| {
                            !player_foe(&f.6)
                                && facts.of(&data, fonts, &f.2.record).npc
                                && npc_state(f.3).0
                        })
                        .map(|f| f.1.translation()),
                )
                .and_then(project),
                _ => {
                    // `@dx,dy`: from the window centre (the player), whatever the window size.
                    let (rel, v) = v.strip_prefix('@').map_or((false, v), |r| (true, r));
                    let (x, y) = v.split_once(',')?;
                    let p = Vec2::new(x.parse().ok()?, y.parse().ok()?);
                    Some(if rel { p + win.size() / 2.0 } else { p })
                }
            }
        })
        .or(win.cursor_position());
    point.set_if_neq(HoverPoint(cursor));
    // No main player (the main menu): nothing is hovered, but the pointer
    // stays known (the game cursor sprite follows it there).
    let Ok((me, _, ppath, pcb)) = player.single() else {
        hovered.set_if_neq(Hovered::default());
        return;
    };
    // Keyboard Only: the controller targeting's pick (a ground item is the label selection, no hover).
    let probe = if options.int("movementType") == 2 {
        match controller.0 {
            gd_game::controller_target::Pick::Item(_) => None,
            p => p.entity().map(Probe::Pick),
        }
    } else {
        cursor
            // The game menu / its confirm / Options take the pointer: nothing behind them is hovered.
            .filter(|_| !capture.0 && !pause.menu)
            .zip(cams.single().ok())
            .and_then(|(c, (cam, tf))| cam.viewport_to_world(tf, c).ok())
            .map(Probe::Ray)
    };
    // The held button (option `targetLock`, sub_140021000, [`held_pick`]).
    let (mouse, move_held, channel) = held;
    let running = channel.single().ok();
    let pressed = mouse.any_just_pressed([MouseButton::Left, MouseButton::Right]);
    let any_held = mouse.any_pressed([MouseButton::Left, MouseButton::Right]);
    if !any_held {
        lock.primary = false;
    }
    let rule = match probe {
        Some(Probe::Ray(_)) => held_pick(&HeldInput {
            target_lock: options.flag("targetLock"),
            running: running.is_some(),
            repeated: (any_held && !pressed) || move_held.0,
            remembered: lock.remembered,
            primary: lock.primary,
        }),
        _ => HeldPick::Fresh,
    };
    if mouse.just_pressed(MouseButton::Left) && !capture.0 && !move_held.0 {
        lock.primary = true;
    }
    let last_enemy = lock.enemy;
    // The held-click pick frustum widens while pathing to an enemy or repeated (sub_140021000
    // `IsPathing || IsCommandRepeated`): the stickiness of a held attack.
    let wide =
        (any_held && !pressed) || move_held.0 || (pcb.target.is_some() && !ppath.points.is_empty());
    let resolve = |facts: &mut RecordFacts,
                   probe: Option<Probe>,
                   enemy_override: Option<bool>|
     -> (Option<Hovered>, bool) {
        // 2. (before the rest: the combat enemy marks whatever wins) Enemies
        // (targetable breakables among them): the one nearest to the player.
        let foe = probe
            .and_then(|probe| match probe {
                Probe::Ray(ray) => gd_game::combat::pick_enemy(
                    ray,
                    foes.iter()
                        .filter(|f| player_foe(&f.4))
                        .map(|(e, gt, cb, _, _)| (e, gt, cb))
                        .chain(
                            breakables
                                .iter()
                                .filter(|b| b.3.targetable)
                                .map(|(e, gt, cb, _)| (e, gt, cb)),
                        ),
                    ppos,
                    wide,
                ),
                Probe::Pick(p) => (foes.get(p).is_ok_and(|f| player_foe(&f.4))
                    || breakables.get(p).is_ok_and(|b| b.3.targetable))
                .then_some(p),
            })
            .and_then(|e| {
                if let Ok((_, gt, _, ch, sd)) = foes.get(e) {
                    Some((e, gt.translation(), Ok((ch, sd.joined.is_some()))))
                } else {
                    breakables
                        .get(e)
                        .ok()
                        .map(|(_, gt, _, b)| (e, gt.translation(), Err(b)))
                }
            });
        let enemy = enemy_override.unwrap_or(foe.is_some());
        let object = |e: Entity, rec: &str| {
            let r = data.db.get(rec);
            let tag = r.and_then(|r| r.str("description")).unwrap_or_default();
            let name: Arc<str> = fonts.tag(tag).into();
            let icon = r.and_then(|r| r.str("interactIconOverride").and_then(icon_override));
            Hovered {
                entity: Some(e),
                name,
                outline: Some(object_c),
                interact: marks(Interact::None, enemy, icon),
                ..default()
            }
        };
        let next = probe.and_then(|probe| {
            // 1. Containers.
            if let Some((e, rec)) = nearest(
                probe,
                chests
                    .iter()
                    .filter(|c| c.2.is_of_interest())
                    .map(|(e, gt, c)| {
                        (
                            e,
                            gt.translation() + Vec3::Y * 0.5,
                            gd_game::combat::body_radius(c.radius) + 0.5,
                            c.record.as_str(),
                        )
                    }),
            ) {
                return Some(object(e, rec));
            }
            // 2. Enemies (picked above).
            match foe {
                Some((e, _, Ok((m, converted)))) => {
                    let f = facts.of(&data, fonts, &m.record);
                    let interact = marks(Interact::Attack, true, f.icon);
                    return Some(Hovered {
                        entity: Some(e),
                        name: f.name.clone(),
                        kind: Kind::Character,
                        interact,
                        // sub_14010F470 asks `IsFriend` of the original faction: a converted pet's is the
                        // player's, so it keeps the friend colour while it fights him.
                        outline: Some(if converted { friend_c } else { enemy_c }),
                        race: f.race.clone(),
                        classification: f.classification,
                    });
                }
                // A breakable: the object colour with the mouse (sub_14010F470).
                Some((e, _, Err(b))) => {
                    let o = object(e, &b.record);
                    let interact = if o.interact == Interact::None {
                        Interact::Attack
                    } else {
                        o.interact
                    };
                    return Some(Hovered {
                        kind: Kind::Breakable,
                        interact,
                        ..o
                    });
                }
                None => {}
            }
            // 3. Other characters.
            let mut best: Option<(f32, Hovered)> = None;
            for (e, gt, ch, actor, vis, pet, sd) in &friends {
                if player_foe(&sd) {
                    continue;
                }
                if pet.is_some_and(|p| p.owner == me) && !target_pet_key {
                    continue;
                }
                let f = facts.of(&data, fonts, &ch.record);
                let radius = gd_game::combat::body_radius(f.radius);
                let Some(t) = probe.hit(e, gt.translation() + Vec3::Y, radius + 0.4) else {
                    continue;
                };
                if best.as_ref().is_some_and(|b| b.0 <= t) {
                    continue;
                }
                let h = if f.npc {
                    let (available, conversation) = npc_state(actor);
                    if !available || vis == Some(&Visibility::Hidden) {
                        continue;
                    }
                    // `IsOfInterest`: a subclass's always (module docs).
                    let interest = conversation || f.npc_subclass;
                    let mark = match (interest, f.merchant) {
                        (false, _) => Interact::None,
                        (true, true) => Interact::Merchant,
                        (true, false) => Interact::Dialog,
                    };
                    Hovered {
                        entity: Some(e),
                        name: f.name.clone(),
                        kind: Kind::Npc,
                        interact: marks(mark, enemy, f.icon),
                        outline: interest.then_some(friend_c),
                        ..default()
                    }
                } else {
                    Hovered {
                        entity: Some(e),
                        name: f.name.clone(),
                        kind: Kind::Character,
                        interact: marks(Interact::None, enemy, f.icon),
                        outline: Some(friend_c),
                        race: f.race.clone(),
                        classification: f.classification,
                    }
                };
                best = Some((t, h));
            }
            // Described decorations (`gd_game::world::Described`) are with the characters
            // (sub_14002FF80 lists them with them; sub_14002BC00 takes a `Decoration`
            // from that pick): the nearest along the ray. Name only, object outline.
            for (e, _, d, sphere) in &described {
                let Some(t) = deco_hit(probe, e, sphere) else {
                    continue;
                };
                if best.as_ref().is_some_and(|b| b.0 <= t) {
                    continue;
                }
                let name = fonts.tag(&d.tag).into();
                best = Some((
                    t,
                    Hovered {
                        entity: Some(e),
                        name,
                        outline: Some(object_c),
                        interact: marks(Interact::None, enemy, None),
                        ..default()
                    },
                ));
            }
            if let Some((_, h)) = best {
                return Some(h);
            }
            // 4. Other fixed items.
            // Doors only while of interest (`gd_game::doors::Door::is_of_interest`).
            let doors = doors
                .iter()
                .filter(|(_, _, d)| data.db.get(&d.record).is_some_and(|r| d.is_of_interest(r)))
                .map(|(e, gt, d)| {
                    (
                        e,
                        gt.translation() + Vec3::Y,
                        gd_game::combat::body_radius(d.radius) + 0.8,
                        d.record.as_str(),
                    )
                });
            let shrines = shrines
                .iter()
                .filter(|s| s.2.is_of_interest(devotion.as_deref()))
                .map(|(e, gt, s)| {
                    (
                        e,
                        gt.translation() + Vec3::Y,
                        gd_game::combat::body_radius(s.radius).max(1.0),
                        s.record.as_str(),
                    )
                });
            let gates = gates
                .iter()
                .filter(|g| {
                    g.2.kind == gd_game::riftgates::WaypointKind::Riftgate && g.2.is_of_interest()
                })
                .map(|(e, gt, w)| {
                    (
                        e,
                        gt.translation() + Vec3::Y,
                        w.radius.max(1.0),
                        w.record.as_str(),
                    )
                });
            // A personal portal (PORT: hit within 1 m of a point 1 m up, as its clicks do): its
            // name is `tagRiftgatePersonal`, not the record's `description`.
            let portals =
                portals
                    .iter()
                    .filter(|(_, _, p)| p.closing.is_none())
                    .map(|(e, gt, _)| {
                        (
                            e,
                            gt.translation() + Vec3::Y,
                            1.0,
                            gd_game::riftgates::PORTAL_RECORD,
                        )
                    });
            // The ascendant altar (a fixed actor, always of interest).
            let altars = altars.iter().map(|(e, gt, a)| {
                (
                    e,
                    gt.translation() + Vec3::Y,
                    gd_game::combat::body_radius(a.radius).max(1.0),
                    a.record.as_str(),
                )
            });
            // The Shattered Realm's exit gates (`FixedItemDungeonTeleport`: a `FixedItemTeleport`, always of
            // interest; PORT: hit within its mesh's 1.45 m orb, centred 1.8 m up) and its unused buff shrines
            // (`EndlessShrine::is_of_interest`).
            let exits = exit_gates.iter().map(|(e, gt)| {
                (
                    e,
                    gt.translation() + Vec3::Y * 1.8,
                    1.45,
                    gd_game::endless::EXIT_GATE_RECORD,
                )
            });
            let endless_shrines =
                endless_shrines
                    .iter()
                    .filter(|s| s.2.is_of_interest())
                    .map(|(e, gt, s)| {
                        (
                            e,
                            gt.translation() + Vec3::Y,
                            gd_game::combat::body_radius(s.radius).max(1.0),
                            s.record.as_str(),
                        )
                    });
            nearest(
                probe,
                doors
                    .chain(shrines)
                    .chain(gates)
                    .chain(portals)
                    .chain(altars)
                    .chain(exits)
                    .chain(endless_shrines),
            )
            .map(|(e, rec)| {
                let mut h = object(e, rec);
                // `FixedItemTeleport::GetGameDescription` @0x1802b5df0 (the exit gate's class inherits
                // it): `tagRiftgatePersonal`, not the record's description.
                if rec == gd_game::riftgates::PORTAL_RECORD
                    || rec == gd_game::endless::EXIT_GATE_RECORD
                {
                    h.name = fonts.tag("tagRiftgatePersonal").into();
                }
                h
            })
        });
        (next, enemy)
    };
    // An ally under the cursor while an enemy-target skill runs (`GetTargetType` 2 and `IsRunningSkill`): the
    // object is dropped (sub_140021000: the ally pick is set, then the object cleared).
    let enemy_skill_running = running.is_some_and(|c| {
        data.db.get(&c.skill).is_some_and(|r| {
            gd_sim::skills::target_type(&r.class) == gd_sim::skills::TargetType::Enemy
        })
    });
    let ally_touched = |probe: Option<Probe>, facts: &mut RecordFacts| {
        probe.is_some_and(|probe| {
            friends.iter().any(|(e, gt, ch, actor, vis, pet, sd)| {
                if player_foe(&sd) || (pet.is_some_and(|p| p.owner == me) && !target_pet_key) {
                    return false;
                }
                let f = facts.of(&data, fonts, &ch.record);
                if f.npc && (!npc_state(actor).0 || vis == Some(&Visibility::Hidden)) {
                    return false;
                }
                probe
                    .hit(
                        e,
                        gt.translation() + Vec3::Y,
                        gd_game::combat::body_radius(f.radius) + 0.4,
                    )
                    .is_some()
            })
        })
    };
    let (next, enemy) = match rule {
        HeldPick::Locked(e) => match resolve(facts, Some(Probe::Pick(e)), Some(last_enemy)) {
            (Some(h), enemy) => (Some(h), enemy),
            // Gone (dead, broken, despawned): as if nothing were remembered.
            _ if lock.primary => (None, false),
            _ => resolve(facts, probe, None),
        },
        HeldPick::Nothing => (None, false),
        HeldPick::Fresh => {
            let (next, enemy) = resolve(facts, probe, None);
            // The combat enemy is set before the object is dropped.
            if next.is_some() && enemy_skill_running && ally_touched(probe, facts) {
                (None, enemy)
            } else {
                (next, enemy)
            }
        }
    };
    // +2472: the object of this event (none: cleared), and the combat enemy for the next one.
    lock.remembered = next.as_ref().and_then(|h| h.entity);
    lock.enemy = enemy;
    let next = next.unwrap_or_default();
    if *hovered != next {
        debug!(
            "hover: {:?} {:?} {:?} outline {:?}",
            next.entity, next.kind, next.name, next.outline
        );
        *hovered = next;
    }
    // The light rig's hover highlight (`gd_render::hover`): the marker follows the hover target that took the outline
    // path (sub_140021000: an `Npc` goes through sub_14010F2B0, whose sub_14010F470 outlines and ends in
    // `SetHighlight(true)`, only when `IsOfInterest`; else it is only named), whether or not `targetOutline` draws it.
    let want = hovered.entity.filter(|_| hovered.outline.is_some());
    let cur = glow.iter().next();
    if cur != want {
        if let Some(e) = cur {
            commands.entity(e).remove::<HoverGlow>();
        }
        if let Some(e) = want {
            commands.entity(e).try_insert(HoverGlow);
        }
    }
}

/// An outlined part's copy in the outline pass (module docs).
#[derive(Component)]
pub struct OutlineMask;

/// The ground item outline colours of the label manager (`Grim Dawn.exe` `UIWorldDescManager` +356 / +372,
/// ctor sub_14028A4F0): every item whose label is listed (sub_14028BAD0, from the frustum scan sub_14028C670:
/// `EnableOutline(true)`, `SetOutlineColor(+356)`) and the selected label's item (update sub_14028AE50, when the
/// command is not repeated: `SetOutlineColor(+372)`). Not gated by `targetOutline`.
const ITEM_OUTLINE: Vec3 = Vec3::new(0.5, 0.4, 0.25);
const SELECTED_ITEM_OUTLINE: Vec3 = Vec3::new(1.0, 0.8, 0.5);

/// The outlined roots (root, palette class: 1 the hover target, 2 a listed ground item, 3 the selected one) and
/// their masks by part: (root, the mesh and material the mask was made from, mask entity).
#[derive(Default)]
struct Outlined(
    Vec<(Entity, u8)>,
    HashMap<Entity, (Entity, AssetId<Mesh>, AssetId<GdMaterial>, Entity)>,
);

/// The outline pass's objects (module docs): the hover target in its colour (palette class 1, with `targetOutline`)
/// and the ground items with a listed label ([`ITEM_OUTLINE`] / [`SELECTED_ITEM_OUTLINE`], classes 2 / 3).
#[allow(clippy::too_many_arguments)]
fn outline(
    mut commands: Commands,
    hovered: Res<Hovered>,
    listed: Res<crate::loot::ListedLabels>,
    options: Res<gd_game::options::GameOptions>,
    children: Query<&Children>,
    parts: Query<
        (&Mesh3d, &MeshMaterial3d<GdMaterial>, Option<&SkinnedMesh>),
        Without<OutlineMask>,
    >,
    mut materials: ResMut<Assets<GdMaterial>>,
    (mut state, mut fresh): (Local<Outlined>, gd_game::session::NewSession),
    mut glow: ResMut<OutlineGlow>,
) {
    if fresh.begun() {
        // The masks went with the ended session's entities.
        *state = Outlined::default();
    }
    let hover = hovered
        .entity
        .zip(hovered.outline)
        .filter(|_| options.flag("targetOutline"));
    let mut want: Vec<(Entity, u8)> = hover.map(|h| (h.0, 1)).into_iter().collect();
    for e in &listed.0 {
        if hover.is_none_or(|h| h.0 != *e) {
            want.push((*e, if listed.1 == Some(*e) { 3 } else { 2 }));
        }
    }
    if state.0 != want {
        // Masks of roots no longer outlined, or outlined in another class.
        let keep = |root: Entity| state.0.iter().find(|w| w.0 == root).map(|w| w.1);
        let gone: Vec<Entity> = state
            .1
            .values()
            .filter(|m| keep(m.0) != want.iter().find(|w| w.0 == m.0).map(|w| w.1))
            .map(|m| m.3)
            .collect();
        for mask in &gone {
            commands.entity(*mask).try_despawn();
        }
        state.1.retain(|_, m| !gone.contains(&m.3));
        state.0 = want.clone();
    }
    let mut palette = [Vec3::ZERO; 3];
    if let Some(h) = hover {
        palette[0] = h.1;
    }
    for (_, class) in &want {
        match class {
            2 => palette[1] = ITEM_OUTLINE,
            3 => palette[2] = SELECTED_ITEM_OUTLINE,
            _ => {}
        }
    }
    glow.set_if_neq(OutlineGlow(palette));
    // Every frame: parts that appeared since (bodies and attachments load
    // over several frames) or changed mesh or material.
    for &(root, class) in &want {
        for e in children.iter_descendants(root) {
            let Ok((mesh, material, skin)) = parts.get(e) else {
                continue;
            };
            let have = state.1.get(&e).copied();
            if have.is_some_and(|p| p.1 == mesh.0.id() && p.2 == material.0.id()) {
                continue;
            }
            let Some(mask) = materials
                .get(&material.0)
                .and_then(|m| m.outline_mask(class))
            else {
                continue;
            };
            if let Some(old) = have {
                commands.entity(old.3).try_despawn();
            }
            let mask = materials.add(mask);
            let mut m = commands.spawn((
                OutlineMask,
                Mesh3d(mesh.0.clone()),
                MeshMaterial3d(mask),
                RenderLayers::layer(GLOW_LAYER),
                NoFrustumCulling,
                bevy::light::NotShadowCaster,
                ChildOf(e),
            ));
            if let Some(skin) = skin {
                m.insert(skin.clone());
            }
            let id = m.id();
            state.1.insert(e, (root, mesh.0.id(), material.0.id(), id));
        }
    }
}

/// The health bar's fill, animated every frame ([`animate_fill`]): its bar, the full bitmap's size, the widget's
/// rect (screen px), the window's inverse scale factor and the fill ratio ([`describe`] sets it in place).
#[derive(Component)]
struct HoverFill {
    bar: BarGraph,
    full_size: UVec2,
    rect: URect,
    inv: f32,
    ratio: f32,
}

/// What the widget was built for: the target, whether it has a bar, its level and the player's level, the window
/// size, the debuff strip. The bar's fill is not here: it changes in place.
type Shown = (
    Hovered,
    bool,
    Option<i32>,
    i32,
    UVec2,
    Vec<crate::ui::status::TargetDebuff>,
);

#[allow(clippy::too_many_arguments)]
fn describe(
    mut commands: Commands,
    hovered: Res<Hovered>,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    roots: Query<(Entity, &crate::ui::RootFrame), With<NameRoot>>,
    frame: Res<bevy::diagnostic::FrameCount>,
    lives: Query<(
        &crate::combat::Combatant,
        Has<gd_game::monsters::Monster>,
        Has<gd_game::pets::Pet>,
    )>,
    player: Query<&crate::combat::Combatant, With<crate::player::Player>>,
    mut assets: crate::ui::UiAssets,
    (mut shown, mut fresh): (Local<Option<Shown>>, gd_game::session::NewSession),
    options: Res<gd_game::options::GameOptions>,
    values: Res<crate::ui::HudValues>,
    mut fills: Query<&mut HoverFill>,
) {
    if fresh.begun() {
        *shown = None;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let live = hovered
        .entity
        .filter(|_| matches!(hovered.kind, Kind::Character | Kind::Breakable))
        .and_then(|e| lives.get(e).ok());
    // A `Monster` (pets too) or a `Destructible` has the health bar.
    let ratio = live
        .filter(|l| l.1 || l.2 || hovered.kind == Kind::Breakable)
        .map(|l| (l.0.life / l.0.profile.sheet.life.max(1.0)).clamp(0.02, 1.0));
    let level = live
        .filter(|_| hovered.kind == Kind::Character && options.flag("showMonsterLevelOnRollover"))
        .map(|l| l.0.profile.level);
    let plevel = player.single().map(|p| p.profile.level).unwrap_or(1);
    let strip: &[crate::ui::status::TargetDebuff] = if options.flag("monsterBarsDebuffs") {
        &values.target_debuffs
    } else {
        &[]
    };
    let same = shown.as_ref().is_some_and(|s| {
        s.0 == *hovered
            && s.1 == ratio.is_some()
            && s.2 == level
            && s.3 == plevel
            && s.4 == size
            && s.5 == strip
    });
    let clear = hovered.entity.is_none() || hovered.name.is_empty() || size.x == 0;
    crate::ui::retire_roots(
        &mut commands,
        roots.iter().map(|(e, f)| (e, f.0)),
        frame.0,
        !same,
        clear,
    );
    if same {
        // The life changes the fill only (`animate_fill` draws it).
        if let Some(ratio) = ratio {
            for mut f in &mut fills {
                if f.ratio != ratio {
                    f.ratio = ratio;
                }
            }
        }
        return;
    }
    *shown = Some((
        hovered.clone(),
        ratio.is_some(),
        level,
        plevel,
        size,
        strip.to_vec(),
    ));
    if clear {
        return;
    }
    let (Some(desc), Some(name_rec)) = (data.db.get(DESC), data.db.get(NAME_BOX)) else {
        return;
    };
    let root = crate::ui::spawn_frame_root(&mut commands, NameRoot, frame.0, 4);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        assets.scale(size),
        1.0 / win.scale_factor(),
        size.as_vec2(),
    );
    // Anchor: the viewport's top centre, 1 px down (sub_140027350).
    let anchor = Vec2::new((ui.screen.x * 0.5).trunc(), 1.0);
    // A text box as wide as its text, centred on the anchor (sub_14010E2B0).
    let text = |ui: &mut Ui, tb: &TextBox, s: &str, tint: Color| {
        let Some(style) = tb.style.as_deref() else {
            return;
        };
        let w = ui.text_width(style, s).trunc();
        let at = Vec2::new(
            anchor.x - w * 0.5 + ui.sc(tb.rect.x),
            anchor.y + ui.sc(tb.rect.y),
        );
        ui.text_line_tinted(style, at, s, tint);
    };
    text(
        &mut ui,
        &TextBox::from_record(name_rec),
        &hovered.name,
        Color::WHITE,
    );
    let sub = |k: &str| desc.str(k).map(str::to_owned).unwrap_or_default();
    // A `Destructible` (sub_14010E2B0's +2120 branch): `healthBar` at
    // life / max life and the tier-1 border, nothing else.
    if hovered.kind == Kind::Breakable {
        if let Some(ratio) = ratio
            && let Some(Widget::BarGraph(bar)) = ui.widget(&sub("healthBar"))
        {
            health_bar(&mut ui, bar, anchor, ratio);
        }
        tier_border(&mut ui, &sub("healthBarTier1Bitmap"), anchor);
        return;
    }
    if hovered.kind != Kind::Character {
        return;
    }
    if !hovered.race.is_empty()
        && let Some(r) = data.db.get(&sub("raceTextBox"))
    {
        text(
            &mut ui,
            &TextBox::from_record(r),
            &hovered.race,
            Color::WHITE,
        );
    }
    if let Some(ratio) = ratio {
        let wide = size.x >= 1920;
        let boss = hovered.classification >= 3 && wide;
        if let Some(Widget::BarGraph(bar)) =
            ui.widget(&sub(if boss { "healthBarBoss" } else { "healthBar" }))
        {
            health_bar(&mut ui, bar, anchor, ratio);
        }
        let tier = match hovered.classification {
            0 => 1,
            1 => 2,
            3 | 5 if wide => 5,
            4 if wide => 4,
            _ => 3,
        };
        tier_border(&mut ui, &sub(&format!("healthBarTier{tier}Bitmap")), anchor);
        if !strip.is_empty() {
            debuff_strip(&mut ui, &data, &sub("statusIcons"), anchor, strip);
        }
    }
    if let Some(level) = level
        && let Some(r) = data.db.get(&sub("levelTextBox"))
    {
        let d = level - plevel;
        let tier = match d {
            ..-2 => 6,
            -2..=-1 => 5,
            0..=1 => 4,
            2..=3 => 3,
            4..=5 => 2,
            _ => 1,
        };
        let c = |ch: &str| {
            desc.f32(&format!("levelDiffTier{tier}.{ch}"))
                .unwrap_or(1.0)
        };
        text(
            &mut ui,
            &TextBox::from_record(r),
            &level.to_string(),
            Color::srgb(c("R"), c("G"), c("B")),
        );
    }
}

/// The debuff icon strip under the bar (`monsterBarsDebuffs`; sub_14010E2B0, sub_14024E2E0): the widget
/// `actor_statusicons.dbr` is centred on the anchor by its width, its debuff area
/// (area 2: `activeHudArea2X/Y` = (0, 120), width 226) starts there and the icons run left to right at their
/// texture's own size (32 px) plus `iconHorizSpace` / `iconVertSpace` (6); a row ends once the running sum
/// reaches the area width (6 icons), the next goes down. Countdown text (red) at the cooldown text location
/// (16, 18) of the icon. PORT: the widget width centred on is taken as the area's width (not decompiled).
fn debuff_strip(
    ui: &mut Ui,
    data: &GameData,
    record: &str,
    anchor: Vec2,
    icons: &[crate::ui::status::TargetDebuff],
) {
    let Some(rec) = data.db.get(record) else {
        return;
    };
    let f = |k: &str, d: f32| rec.f32(k).unwrap_or(d);
    let (hs, vs) = (
        ui.sc(f("iconHorizSpace", 6.0)),
        ui.sc(f("iconVertSpace", 6.0)),
    );
    let aw = ui.sc(f("activeHudArea2SizeX", 226.0));
    let left = anchor.x - (aw * 0.5).trunc() + ui.sc(f("activeHudArea2X", 0.0));
    let top = anchor.y + ui.sc(f("activeHudArea2Y", 120.0));
    let style = rec
        .str("cooldownFontStyle")
        .unwrap_or("records/ui/styles/text/style_special_cooldown.dbr")
        .to_owned();
    let (tx, ty) = (
        ui.sc(f("cooldownTextLocationX", 16.0)),
        ui.sc(f("cooldownTextLocationY", 18.0)),
    );
    let (mut x, mut y, mut sum) = (left, top, 0.0f32);
    for icon in icons {
        let Some((img, size)) = ui.image(&icon.icon) else {
            continue;
        };
        let (w, h) = (ui.sc(size.x as f32), ui.sc(size.y as f32));
        ui.quad_stretch(img, URect::new(x, y, w, h), Color::WHITE);
        if let Some(text) = &icon.text {
            let lh = ui.line_height(&style);
            ui.text_line_tinted(
                &style,
                Vec2::new(x + tx, y + ty - lh),
                text,
                Color::srgb(1.0, 0.0, 0.0),
            );
        }
        sum += w + hs;
        x += w + hs;
        if sum >= aw {
            x = left;
            sum = 0.0;
            y += h + vs;
        }
    }
}

/// A tier border bitmap centred on the anchor at its record offset.
fn tier_border(ui: &mut Ui, widget: &str, anchor: Vec2) {
    if let Some(Widget::Bitmap { bitmap, x, y }) = ui.widget(widget)
        && let Some((h, s)) = ui.image(&bitmap)
    {
        let (w, hgt) = (ui.sc(s.x as f32), ui.sc(s.y as f32));
        ui.quad(
            h,
            URect::new(
                anchor.x - (w * 0.5).trunc() + ui.sc(x),
                anchor.y + ui.sc(y),
                w,
                hgt,
            ),
            None,
            Color::WHITE,
        );
    }
}

/// The bar centred on the anchor at its record offset; the empty bitmap
/// under the fill when `renderBackground` is set (animated bars).
fn health_bar(ui: &mut Ui, bar: BarGraph, anchor: Vec2, ratio: f32) {
    let empty = bar.empty.as_deref().and_then(|p| ui.image(p));
    let Some((full, fs)) = bar.full.as_deref().and_then(|p| ui.image(p)) else {
        return;
    };
    let (bw, bh) = bar.base_size(empty.as_ref().map(|e| (e.1.x, e.1.y)), Some((fs.x, fs.y)));
    let (w, h) = (ui.sc(bw), ui.sc(bh));
    let r = URect::new(
        anchor.x - (w * 0.5).trunc() + ui.sc(bar.x),
        anchor.y + ui.sc(bar.y),
        w,
        h,
    );
    if let Some((e, _)) = empty
        && (bar.render_background || bar.anim.is_none())
    {
        ui.quad(e, r, None, Color::WHITE);
    }
    let e = ui.quad(
        full,
        bar.fill_dst(r, ratio),
        Some(bar.fill_src(fs.x, fs.y, ratio, 0)),
        Color::WHITE,
    );
    ui.commands.entity(e).insert(HoverFill {
        bar,
        full_size: fs,
        rect: r,
        inv: ui.inv,
        ratio,
    });
}

/// The fill at its ratio; `UIBarGraphAnimated`: the fill cycles its frames at `framesPerSecond`. Writes only what
/// changed.
fn animate_fill(time: Res<Time>, mut fills: Query<(&HoverFill, &mut Node, &mut ImageNode)>) {
    for (f, mut node, mut img) in &mut fills {
        let frame = f
            .bar
            .anim
            .map_or(0, |a| (time.elapsed_secs() * a.fps) as u32);
        let src = f.bar.fill_src(f.full_size.x, f.full_size.y, f.ratio, frame);
        crate::ui::set_node_rect(&mut node, f.bar.fill_dst(f.rect, f.ratio), f.inv);
        crate::ui::set_image_src(&mut img, src);
    }
}
