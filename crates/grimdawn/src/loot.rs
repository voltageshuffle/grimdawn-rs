//! Ground items, presentation side: the lying model and the name label of
//! each `gd_game::loot::GroundItem` (drops and pickups are gameplay,
//! `gd_game::loot`), labels placed over their items, cursor picking.
//!
//! Labels as `UIWorldDescManager` (GrimDawn.exe, ctor `UIWorldDescManager::UIWorldDescManager`, update
//! `UIWorldDescManager::Update`, placement `UIWorldDescManager::FindWorldPosition` /
//! `UIWorldDescManager::StackLabel`, draw `UIWorldDescManager::Draw`): a box in the `uiGroundItemRolloverStyle`
//! style (background,
//! border, the name in its font tinted by rarity; [`label_box`]), centred 40 px above the item, stacked upwards
//! over earlier labels with 4 px gaps ([`stack_label`]) and eased into place ([`animate_offset`]). The cursor
//! selects the label whose drawn (stacked, eased) rect it is on, the last placed first ([`label_at`],
//! [`HoveredLabel`]): a click picks that item up (`player.rs`), its tooltip shows (`tooltip.rs`).
//!
//! The loot filter (`gd_game::loot_filter`, rules
//! `gd_sim::items::loot_filter`) hides the labels of the items it rejects
//! ([`FilteredOut`]: no label). The label mode ([`LabelMode`],
//! `gd_game::loot_filter::label_mode`, GrimDawn.exe `UIWorldDescManager::ApplyLootFilter`
//! and the key actions 35 / 37, held flags set by `UIWorldDescManager::HandleHeldKeys`): "Show
//! Items (No Filter)" (Left Alt) shows every label while held, "Show Items
//! (Filter Common)" (Z) every label but Common ones; with both held, Z wins.
//! The cursor pick and the Pickup key (action 55, unbound by default) use the
//! pick mode ([`PickMode`], `gd_game::loot_filter::pick_mode`,
//! `InGameUI::GetPickFilterMode`, adds `lockItemPickup`): [`Unpickable`] marks what the
//! cursor cannot pick (`player.rs`); the Pickup key sends
//! `gd_game::loot_filter::PickUpNearest`.
//!
//! "Toggle Hide All Items" (key action 57, no key in the engine's default
//! table `KeyMap::SetDefaultBindings`, so unbound until the player binds one;
//! GrimDawn.exe HUD key handler `UIPlayerHud::HandleKey`) flips the HUD's
//! items-shown flag ([`ItemsHidden`]) on the key press and shows
//! `tagLootFilterToggleOff` / `tagLootFilterToggleOn` with
//! `Engine::ShowCinematicText` (type 9, empty subline, not in front:
//! [`gd_game_core::story::CinematicText`]); while hidden the label mode is
//! `ItemIgnore` mode 3 ([`Mode::HideAll`](gd_sim::items::loot_filter::Mode),
//! `UIWorldDescManager::ApplyLootFilter`: below the two held keys) and the HUD loot filter button
//! is disabled (`ui/lootfilter.rs`). The loot filter window: `ui/lootfilter.rs`.
//!
//! Loot beams, Game.dll `Item::CreateBeamLoop` /
//! `CreateBeamEnd`, `GameEngine::LoadFromDatabase`: the
//! `records/game/gameiteminfo.dbr` `<class>ItemBeamLoop` / `<class>ItemBeamEnd`
//! effects of the item's display class (the classification; both affixes
//! rare: doubleRare 16, or monsterDoubleRare 17; `ui::character::rarity_key`,
//! stock data has them for epic, legendary, doubleRare and
//! monsterDoubleRare). The loop is attached to the item once it lands
//! (`Item::PhysicsUpdate`, retried in `UpdateSelf`; the throw:
//! `gd_sim::loot_toss`, [`gd_game::loot::Tossed`]) and destroyed on pickup
//! (`Item::OnPickup`), which spawns the end effect at the item. It sits at
//! the item's centre of mass (`Actor::GetCenterOfMass`, Engine.dll:
//! the centre of the mesh's chunk-10 bounding box, `GraphicsMesh::Initialize`)
//! in the item's frame ([`CentreOfMass`]). The beam shows exactly when the
//! label does (GrimDawn.exe `UIWorldDescManager::ApplyLootFilter` → `Item::ToggleLootBeam`):
//! hidden while [`FilteredOut`]. A thrown item is hidden until its release
//! (`GameEngineInboundInterface::CreateItem` adds it to the world then).
use bevy::prelude::*;
pub use gd_game::loot::*;
use gd_game_core::player::Player;
pub use gd_game_core::progress::PlayerProgress;
use gd_render::character::CharAssets;
use gd_render::hover::LabelGlow;
use gd_render::model::Ready;
use gd_ui::state::loot::{FilteredOut, HoveredLabel, ItemsHidden};

pub struct LootPlugin;

impl Plugin for LootPlugin {
    fn build(&self, app: &mut App) {
        app.register_required_components::<GroundItem, Visibility>()
            .init_resource::<LabelMode>()
            .init_resource::<PickMode>()
            .init_resource::<ItemsHidden>()
            .init_resource::<HoveredLabel>()
            .init_resource::<ListedLabels>()
            .add_plugins(gd_ui::lootfilter::LootFilterWindowPlugin)
            .add_systems(
                Update,
                (
                    #[cfg(feature = "dev")]
                    test_drops,
                    dress_ground_items,
                    drop_orphan_labels,
                    label_mode,
                    filter_labels,
                    toss_visibility,
                    start_beams,
                    beam_visibility,
                    beam_ends,
                )
                    .chain()
                    .after(gd_game::loot_filter::LootFilterStage::Apply),
            )
            .add_systems(
                PostUpdate,
                place_labels
                    .after(TransformSystems::Propagate)
                    .before(bevy::ui::UiSystems::Layout),
            );
    }
}

/// The ground items whose label is listed this frame (in view, not filtered out; drawn or pushed off by the
/// stacking) and the selected one when its outline shows (no button held on), set by [`place_labels`]: the label
/// manager outlines them (`hover.rs`).
#[derive(Resource, Default)]
pub struct ListedLabels(pub Vec<Entity>, pub Option<Entity>);

/// A ground item the cursor cannot pick in the current [`PickMode`].
#[derive(Component)]
pub struct Unpickable;

/// Which ground labels show (`gd_sim::items::loot_filter::Mode`), from the
/// held keys.
#[derive(Resource, Default, PartialEq)]
pub struct LabelMode(pub gd_sim::items::loot_filter::Mode);

/// Which ground items the cursor and the Pickup key may pick (module doc).
#[derive(Resource, Default, PartialEq)]
pub struct PickMode(pub gd_sim::items::loot_filter::Mode);

#[allow(clippy::too_many_arguments)]
fn label_mode(
    hooks: Res<gd_game_core::hooks::Hooks>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<gd_ui_kit::keys::KeyBindings>,
    options: Res<gd_game_core::options::GameOptions>,
    mut hidden: ResMut<ItemsHidden>,
    mut cinematic: MessageWriter<gd_game_core::story::CinematicText>,
    mut pickup: MessageWriter<gd_game::loot_filter::PickUpNearest>,
    (mut mode, mut pick): (ResMut<LabelMode>, ResMut<PickMode>),
) {
    use gd_game::loot_filter::{label_mode, pick_mode};
    if bindings.just_pressed(gd_ui_kit::keys::HIDE_ALL_ITEMS, &keys) {
        hidden.0 = !hidden.0;
        cinematic.write(gd_game_core::story::CinematicText {
            head: if hidden.0 {
                "tagLootFilterToggleOff"
            } else {
                "tagLootFilterToggleOn"
            }
            .into(),
            sub: String::new(),
            sub_args: Vec::new(),
            kind: 9,
            front: false,
        });
    }
    // Test hook `GD_LABEL_MODE=all|common` holds the key.
    let hook = hooks.items.label_mode.as_deref();
    let show_all = bindings.pressed(gd_ui_kit::keys::SHOW_ALL_ITEMS, &keys) || hook == Some("all");
    let hide_common =
        bindings.pressed(gd_ui_kit::keys::HIDE_COMMON_ITEMS, &keys) || hook == Some("common");
    mode.set_if_neq(LabelMode(label_mode(show_all, hide_common, hidden.0)));
    let p = pick_mode(
        options.flag("lockItemPickup"),
        show_all,
        hide_common,
        hidden.0,
    );
    pick.set_if_neq(PickMode(p));
    if bindings.just_pressed(gd_ui_kit::keys::PICKUP, &keys) {
        pickup.write(gd_game::loot_filter::PickUpNearest(p));
    }
}

/// A ground item the loot filter has not checked yet: hidden ([`FilteredOut`]) until [`filter_labels`] does.
#[derive(Component)]
struct Unfiltered;

/// How long [`filter_labels`] checks new drops a frame (at least one item): a boss or chest drop of dozens of
/// items is filtered over a few frames instead of one long one (they are in the air for the first ones).
const FILTER_BUDGET: std::time::Duration = std::time::Duration::from_millis(2);

/// Marks the ground items the filter hides: every item when the options or the mode change; else a new drop is
/// hidden at once ([`Unfiltered`]) and checked within [`FILTER_BUDGET`] a frame, so a fight's drops do not
/// re-filter everything already on the ground nor stall a frame.
#[allow(clippy::too_many_arguments)]
fn filter_labels(
    mut commands: Commands,
    filter: Option<Res<gd_game::loot_filter::LootFilter>>,
    (mode, pick): (Res<LabelMode>, Res<PickMode>),
    inv: Option<Res<gd_game_core::inventory::Inventory>>,
    data: Res<gd_game_core::data::GameData>,
    players: Query<&gd_game_core::player::Loadout, With<Player>>,
    items: Query<(Entity, &GroundItem, Has<FilteredOut>, Has<Unpickable>)>,
    pending: Query<Entity, With<Unfiltered>>,
    new: Query<Entity, Added<GroundItem>>,
) {
    let (Some(filter), Some(inv)) = (filter, inv) else {
        return;
    };
    for e in &new {
        commands.entity(e).try_insert((FilteredOut, Unfiltered));
    }
    let all = filter.is_changed() || mode.is_changed() || pick.is_changed();
    if !all && pending.is_empty() {
        return;
    }
    let loadout = players.single().ok();
    let mut mark = |(e, gi, hidden, unpickable): (Entity, &GroundItem, bool, bool)| {
        let passes = |m| {
            gi.item
                .as_ref()
                .is_none_or(|it| filter.passes(&inv, &data.db, it, m, loadout))
        };
        let show = passes(mode.0);
        let can_pick = if pick.0 == mode.0 {
            show
        } else {
            passes(pick.0)
        };
        let mut ec = commands.entity(e);
        ec.try_remove::<Unfiltered>();
        match (show, hidden) {
            (false, false) => {
                ec.try_insert(FilteredOut);
            }
            (true, true) => {
                ec.try_remove::<FilteredOut>();
            }
            _ => {}
        }
        match (can_pick, unpickable) {
            (false, false) => {
                ec.try_insert(Unpickable);
            }
            (true, true) => {
                ec.try_remove::<Unpickable>();
            }
            _ => {}
        }
    };
    if all {
        // This frame's drops get their hidden marker after this pass: checked from the next frame.
        for it in items.iter().filter(|it| !new.contains(it.0)) {
            mark(it);
        }
        return;
    }
    let start = std::time::Instant::now();
    for (n, e) in pending.iter().enumerate() {
        if n > 0 && start.elapsed() >= FILTER_BUDGET {
            break;
        }
        if let Ok(it) = items.get(e) {
            mark(it);
        }
    }
}

/// A ground item's loot beam loop effect (module doc).
#[derive(Component)]
struct LootBeam(Entity);

/// The loop effect record a ground item starts once it lands (at its [`CentreOfMass`]).
#[derive(Component)]
struct PendingBeam(String);

/// A ground item's centre of mass in its frame (`Actor::GetCenterOfMass`: the centre of the mesh's
/// bounding box, module doc), where its beam sits and its label is projected from ([`label_anchor`]); the origin
/// without a model.
#[derive(Component)]
struct CentreOfMass(Vec3);

/// A ground item [`dress_ground_items`] is done with.
#[derive(Component)]
struct ItemDressed;

/// A ground item's model box in its frame, what the cursor ray picks the item by ([`model_under`]).
#[derive(Component)]
struct ModelBounds(gd_sim::bounds::Aabb);

/// The pickable ground item (of `drawn`: its label is shown) whose model the cursor ray meets, the nearest along the
/// ray. `Grim Dawn.exe` world mouse handler `InGameUI::HandleMouseEvent`: the object under the cursor
/// (`InGameUIActorCapture::PickObjectUnderCursor`: the candidates' mesh hit (`Actor::HitTestMesh`), nearest; the
/// lists of `InGameUIActorCapture::CollectCandidates` keep items that are of interest and pass
/// the pick mode's loot filter), when it is a visible `Item` and the command is not repeated (no button held on
/// from an earlier event), is outlined and becomes the label manager's world-hovered item
/// (`UIWorldDescManager.worldHoveredItem`, cleared after each label update `UIWorldDescManager::Update`). PORT: the
/// model's bounding box in the item's frame stands in for its triangles.
fn model_under(
    ray: Ray3d,
    drawn: &[(Entity, LabelRect)],
    items: &Query<(&GlobalTransform, &ModelBounds), (With<GroundItem>, Without<Unpickable>)>,
) -> Option<Entity> {
    let mut best: Option<(f32, Entity)> = None;
    for (e, _) in drawn {
        let Ok((gt, b)) = items.get(*e) else { continue };
        if let Some(t) = crate::hover::ray_box(ray, gt, &b.0)
            && best.is_none_or(|x| t < x.0)
        {
            best = Some((t, *e));
        }
    }
    best.map(|b| b.1)
}

/// Thrown items show from their release on (module doc).
fn toss_visibility(mut items: Query<(&gd_game::loot::Tossed, &mut Visibility)>) {
    for (t, mut vis) in &mut items {
        vis.set_if_neq(if t.wait > 0.0 {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
}

/// Landed items start their loot beam (`Item::CreateBeamLoop`); one that left the simulation without landing
/// never does (`gd_game::loot::NoBeam`).
#[allow(clippy::type_complexity)]
fn start_beams(
    mut commands: Commands,
    items: Query<
        (Entity, &PendingBeam, &CentreOfMass),
        (
            Without<gd_game::loot::Tossed>,
            Without<gd_game::loot::NoBeam>,
        ),
    >,
) {
    for (e, p, com) in &items {
        let fx = gd_render::fx::spawn_effect_record(
            &mut commands,
            &p.0,
            Transform::from_translation(com.0),
        );
        gd_render::model::try_add_child(&mut commands, e, fx);
        commands
            .entity(e)
            .try_insert(LootBeam(fx))
            .try_remove::<PendingBeam>();
    }
}

/// The item's `gameiteminfo.dbr` beam effect record (`Loop` / `End`).
fn beam_record(
    db: &gd_formats::vfs::Database,
    r: &gd_sim::items::ResolvedItem,
    which: &str,
) -> Option<String> {
    let key = gd_ui::character::rarity_key(r.display_color_index())?;
    db.get("records/game/gameiteminfo.dbr")?
        .str(&format!("{key}ItemBeam{which}"))
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// The beam shows with the label (`Item::ToggleLootBeam`).
fn beam_visibility(
    beams: Query<(&LootBeam, Has<FilteredOut>), Or<(Changed<LootBeam>, Added<FilteredOut>)>>,
    all: Query<(&LootBeam, Has<FilteredOut>)>,
    removed: RemovedComponents<FilteredOut>,
    mut vis: Query<&mut Visibility>,
) {
    let refresh = |(b, hidden): (&LootBeam, bool), vis: &mut Query<&mut Visibility>| {
        if let Ok(mut v) = vis.get_mut(b.0) {
            v.set_if_neq(if hidden {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            });
        }
    };
    if removed.is_empty() {
        beams.iter().for_each(|b| refresh(b, &mut vis));
    } else {
        all.iter().for_each(|b| refresh(b, &mut vis));
    }
}

/// A picked up item's beam end effect (`Item::CreateBeamEnd`).
fn beam_ends(
    mut commands: Commands,
    mut picked: MessageReader<gd_game::loot::ItemPickedUp>,
    data: Res<gd_game_core::data::GameData>,
) {
    for p in picked.read() {
        let idb = &data.item_db();
        if let Some(rec) =
            gd_sim::items::resolve(idb, &p.item).and_then(|r| beam_record(&data.db, &r, "End"))
        {
            // `GD_SHOT_ON=lootbeamend` shoots it.
            gd_game_core::visuals::notify(&mut commands, &rec);
            gd_render::fx::spawn_effect_record(
                &mut commands,
                &rec,
                Transform::from_translation(p.at),
            );
        }
    }
}

/// A ground item's name label: a box in the ground-item rollover style around the name (`label_box`), placed
/// over the item by [`place_labels`].
#[derive(Component)]
pub struct ItemLabel {
    pub owner: Entity,
    /// The stacked position's offset from the natural one (y, physical px), smoothed, and the shown offset moving
    /// towards it; `None` before the first placement.
    offsets: Option<(f32, f32)>,
    /// The style's background colour and `backgroundHighlightColor` (RGBA, [`label_background`]).
    backgrounds: [[f32; 4]; 2],
    /// Drawn with the highlight colour (the selected label, [`HoveredLabel`]).
    lit: bool,
}

/// `records/ui/styles/rollover/style_itemsground.dbr` → `uiGroundItemRolloverStyle` (GrimDawn.exe
/// `InGameUI::LoadWorldDescStyle` → `UIWorldDescManager` load `UIWorldDescManager::Load`).
const LABEL_STYLE_HOLDER: &str = "records/ui/styles/rollover/style_itemsground.dbr";
/// `UIWorldDescManager.labelLift` (ctor `UIWorldDescManager::UIWorldDescManager`: 40.0): the label's centre sits this
/// far above the projected item.
const LABEL_LIFT: f32 = 40.0;
/// Text size + 6 (`UIWorldDescManager::FindWorldPosition`).
const LABEL_PAD: f32 = 3.0;
/// Gap between stacked labels and the snap distance (`UIWorldDescManager::FindWorldPosition` /
/// `UIWorldDescManager::StackLabel`).
const LABEL_GAP: f32 = 4.0;

/// New ground items get their model and their label: the item's name in its
/// display class's colour (`ui::character::name_color`), or the gold amount. The model is the record's `mesh`
/// scaled by the record's `scale` at the item's coordinates, turned as they
/// are (`GameEngineInboundInterface::CreateItem`, no lay-flat turn:
/// a thrown item rests in the pose its rigid body touched down in,
/// `gd_sim::drop_body`). The model loads in the background (`CharAssets::model_ready`): an item is dressed once
/// it is built (until then one record lookup a frame); [`ItemDressed`] marks a dressed one.
#[allow(clippy::too_many_arguments)]
fn dress_ground_items(
    mut commands: Commands,
    new: Query<(Entity, &GroundItem), Without<ItemDressed>>,
    mut fonts: ResMut<gd_ui_kit::text::FontCache>,
    mut font_assets: ResMut<Assets<Font>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    ui_scale: Res<gd_ui_kit::state::settings::UiScaleOption>,
    mut a: CharAssets,
) {
    let Ok(win) = windows.single() else { return };
    let idb = &a.data.item_db();
    for (e, gi) in &new {
        let record = gi
            .item
            .as_ref()
            .map_or(gd_game::loot::GOLD_RECORD, |i| i.base_name.as_str());
        let db = a.data.db.clone();
        let model = match db
            .get(record)
            .and_then(|r| r.str("mesh"))
            .map(|m| a.model_ready(m))
        {
            Some(Ready::Loading) => continue,
            Some(Ready::Done(model)) => Some(model),
            Some(Ready::Failed) | None => None,
        };
        commands.entity(e).try_insert(ItemDressed);
        let mut beam = None;
        let (label, color) = match &gi.item {
            Some(item) => {
                let Some(r) = gd_sim::items::resolve(idb, item) else {
                    continue;
                };
                beam = beam_record(&a.data.db, &r, "Loop");
                let name = gd_sim::items::naming::item_name(&fonts.tags, &r);
                (
                    name,
                    gd_ui::character::name_color(&a.data.db, r.display_color_index()),
                )
            }
            None => {
                // The gold pile's name (`money_label`). PORT: the name tag is the one stock `OneShot_Gold`
                // record's `description` (how the engine picks the record is not decompiled); without a colour
                // code in the format the label takes a common item's colour.
                let name_tag = a
                    .data
                    .db
                    .get(gd_game::loot::GOLD_RECORD)
                    .and_then(|r| r.str("description").map(str::to_owned))
                    .unwrap_or_default();
                let (label, color) =
                    gd_sim::items::naming::money_label(&fonts.tags, &name_tag, gi.gold);
                (
                    label,
                    color.map_or(gd_sim::items::Classification::Common.rgb(), |c| {
                        [c[0], c[1], c[2]]
                    }),
                )
            }
        };
        let scale = db
            .get(record)
            .and_then(|r| r.f32("scale"))
            .filter(|s| *s > 0.0)
            .unwrap_or(1.0);
        // The model at the item's coordinates (their rotation is the rigid body's, `gd_game::loot::toss_flight`),
        // scaled by the record's `scale`.
        let body_tf = Transform::from_scale(Vec3::splat(scale));
        // The mesh's bounding box centre (`Actor::GetCenterOfMass`) in the item's frame, unscaled: the beam turns
        // with the item (`Item::CreateBeamLoop` attaches it there without the mesh scale).
        let com = model.as_ref().map_or(Vec3::ZERO, |m| {
            m.msh.bounding_box.map_or(Vec3::ZERO, |(lo, hi)| {
                (Vec3::from(lo) + Vec3::from(hi)) * 0.5
            })
        });
        commands.entity(e).try_insert(CentreOfMass(com));
        if let Some(m) = &model {
            // The mesh's own AABB (chunk 10) under the uniform scale; the vertex scan only for a mesh
            // without one.
            let bounds = match m.msh.bounding_box {
                Some((lo, hi)) => Some((Vec3::from(lo) * scale, Vec3::from(hi) * scale)),
                None if !m.msh.vertices.positions.is_empty() => {
                    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                    for v in &m.msh.vertices.positions {
                        let q = body_tf.transform_point(Vec3::from(*v));
                        lo = lo.min(q);
                        hi = hi.max(q);
                    }
                    Some((lo, hi))
                }
                None => None,
            };
            if let Some((lo, hi)) = bounds {
                commands
                    .entity(e)
                    .try_insert(ModelBounds(gd_sim::bounds::Aabb {
                        center: ((lo + hi) * 0.5).to_array(),
                        half: ((hi - lo) * 0.5).to_array(),
                    }));
            }
        }
        if let Some(record) = beam {
            commands.entity(e).try_insert(PendingBeam(record));
        }
        if let Some(model) = model {
            let body = commands.spawn((body_tf, Visibility::default())).id();
            gd_render::model::try_add_child(&mut commands, e, body);
            gd_render::model::spawn_model(&mut commands, body, &model);
            // The mesh's own `CreateEntity` effects, as every actor's (`Actor::CreateAttachmentsFromMesh`,
            // `fx::spawn_mesh_attachments`): a lore note's glow, gone with the item when it is picked up.
            gd_render::fx::spawn_mesh_attachments(&mut commands, &a.data, &model, body);
        }
        let (root, backgrounds) = label_box(
            &mut commands,
            &mut a,
            &mut fonts,
            &mut font_assets,
            (
                ui_scale.factor(win.physical_width(), win.physical_height()),
                1.0 / win.scale_factor(),
            ),
            &label,
            Color::srgb(color[0], color[1], color[2]),
        );
        commands.entity(root).insert(ItemLabel {
            owner: e,
            offsets: None,
            backgrounds,
            lit: false,
        });
    }
}

/// The label box (`UIWorldDescManager::Draw` / `UIWorldDescManager::FindWorldPosition`): the
/// rollover style's background ([`label_background`]; `backgroundHighlightColor` for the selected label,
/// `LabelEntry.highlighted` → `UIRolloverWidget.useHighlightColor`) and border bitmaps around the name in the style's
/// font (`style`, centred), tinted with the rarity
/// colour; text size + 6 (sized by the 3 px padding). PORT: the border pieces lie inside the box as the tooltip's.
fn label_box(
    commands: &mut Commands,
    a: &mut CharAssets,
    fonts: &mut gd_ui_kit::text::FontCache,
    font_assets: &mut Assets<Font>,
    (s, inv): (f32, f32),
    text: &str,
    tint: Color,
) -> (Entity, [[f32; 4]; 2]) {
    use gd_ui_kit::text::LoadedFont;
    let rec = a
        .data
        .db
        .get(LABEL_STYLE_HOLDER)
        .and_then(|r| r.str("uiGroundItemRolloverStyle").map(str::to_owned))
        .and_then(|p| a.data.db.get(&p));
    let f = |k: &str, d: f32| rec.as_ref().and_then(|r| r.f32(k)).unwrap_or(d);
    let backgrounds = [
        [
            f("backgroundColorRed", 0.03),
            f("backgroundColorGreen", 0.03),
            f("backgroundColorBlue", 0.04),
            f("backgroundColorAlpha", 0.75),
        ],
        [
            f("backgroundHighlightColorRed", 0.4),
            f("backgroundHighlightColorGreen", 0.4),
            f("backgroundHighlightColorBlue", 0.5),
            f("backgroundHighlightColorAlpha", 0.7),
        ],
    ];
    let px = |v: f32| Val::Px(v * inv);
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::all(px(LABEL_PAD)),
                ..default()
            },
            label_background(backgrounds[0]),
            Visibility::Hidden,
        ))
        .id();
    let style_path = rec
        .as_ref()
        .and_then(|r| r.str("style").map(str::to_owned))
        .unwrap_or_else(|| "records/ui/styles/text/style_textwhite_sizen_bold.dbr".to_owned());
    let style = fonts.style(&a.data, &style_path);
    let font = style
        .as_ref()
        .and_then(|st| fonts.font(&a.data, &st.font, font_assets));
    let text_node = match (style, font.as_deref()) {
        (Some(style), Some(LoadedFont::Bitmap { font, atlas })) => {
            let size = style.scaled_size(s);
            let ti = gd_ui_kit::text::rasterize(font, atlas, &style, size, text);
            let (w, h) = ti.as_ref().map_or((0.0, 0.0), |t| {
                (t.layout.width as f32, t.layout.height as f32)
            });
            let n = commands
                .spawn(Node {
                    width: px(w),
                    height: px(h),
                    ..default()
                })
                .id();
            if let Some(ti) = ti {
                let img = a.images.add(ti.image);
                let g = commands
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(ti.offset.x.round()),
                            top: px(ti.offset.y.round()),
                            width: px(ti.size.x as f32),
                            height: px(ti.size.y as f32),
                            ..default()
                        },
                        ImageNode::new(img)
                            .with_color(tint)
                            .with_mode(bevy::ui::widget::NodeImageMode::Stretch),
                    ))
                    .id();
                commands.entity(n).add_child(g);
            }
            n
        }
        (Some(st), Some(LoadedFont::Dynamic(h, _))) => commands
            .spawn((
                Text::new(gd_formats::text::strip_markup(text)),
                TextFont {
                    font: FontSource::Handle(h.clone()),
                    font_size: FontSize::Px(st.scaled_size(s) as f32 * inv),
                    ..default()
                },
                TextColor(tint),
            ))
            .id(),
        // Stock styles always load; broken data gets an empty label.
        _ => {
            warn!("loot label: style {style_path} or its font did not load");
            commands.spawn(Node::default()).id()
        }
    };
    commands.entity(root).add_child(text_node);
    // Border bitmaps, stretched along the edges, corners at their size.
    let mut tex = |key: &str| -> Option<(Handle<Image>, Vec2)> {
        let path = rec.as_ref()?.str(key)?.to_owned();
        let h = a.textures.get(&a.data, &path, true, &mut a.images)?;
        let sz = a.images.get(&h)?.size().as_vec2() * s;
        Some((h, sz.round()))
    };
    let piece = |commands: &mut Commands, t: Option<(Handle<Image>, Vec2)>, node: Node| {
        if let Some((h, _)) = t {
            let e = commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        ..node
                    },
                    ImageNode::new(h).with_mode(bevy::ui::widget::NodeImageMode::Stretch),
                ))
                .id();
            commands.entity(root).add_child(e);
        }
    };
    let (top, bottom, left, right) = (
        tex("topTexture"),
        tex("bottomTexture"),
        tex("leftTexture"),
        tex("rightTexture"),
    );
    let th = |t: &Option<(Handle<Image>, Vec2)>, w: bool| {
        t.as_ref().map_or(0.0, |t| if w { t.1.x } else { t.1.y })
    };
    let (bt, bb, bl, br) = (
        th(&top, false),
        th(&bottom, false),
        th(&left, true),
        th(&right, true),
    );
    piece(
        commands,
        top,
        Node {
            left: px(bl),
            right: px(br),
            top: px(0.0),
            height: px(bt),
            ..default()
        },
    );
    piece(
        commands,
        bottom,
        Node {
            left: px(bl),
            right: px(br),
            bottom: px(0.0),
            height: px(bb),
            ..default()
        },
    );
    piece(
        commands,
        left,
        Node {
            top: px(bt),
            bottom: px(bb),
            left: px(0.0),
            width: px(bl),
            ..default()
        },
    );
    piece(
        commands,
        right,
        Node {
            top: px(bt),
            bottom: px(bb),
            right: px(0.0),
            width: px(br),
            ..default()
        },
    );
    for (k, l, t) in [
        ("upperLeftTexture", true, true),
        ("upperRightTexture", false, true),
        ("lowerLeftTexture", true, false),
        ("lowerRightTexture", false, false),
    ] {
        let p = tex(k);
        let sz = p.as_ref().map_or(Vec2::ZERO, |p| p.1);
        let mut n = Node {
            width: px(sz.x),
            height: px(sz.y),
            ..default()
        };
        if l {
            n.left = px(0.0)
        } else {
            n.right = px(0.0)
        }
        if t {
            n.top = px(0.0)
        } else {
            n.bottom = px(0.0)
        }
        piece(commands, p, n);
    }
    (root, backgrounds)
}

/// The label's background (rollover widget draw `UIRolloverWidget::Draw`, colours loaded by
/// `UIRolloverWidget::LoadColors`): a vertical
/// gradient over the box, from the colour at the top to it + 0.2 per channel (at most 1) at the bottom, same alpha
/// (`GraphicsCanvas::RenderVerticalGradient`, Engine.dll: top vertices the first colour). The alpha is scaled by the
/// widget's colour alpha (`UIRolloverWidget.colorAlpha`: 1 with the mouse, 0.55 for an out-of-reach label with a
/// gamepad).
fn label_background(c: [f32; 4]) -> BackgroundGradient {
    let top = Color::srgba(c[0], c[1], c[2], c[3]);
    let bottom = Color::srgba(
        (c[0] + 0.2).min(1.0),
        (c[1] + 0.2).min(1.0),
        (c[2] + 0.2).min(1.0),
        c[3],
    );
    BackgroundGradient::from(LinearGradient {
        color_space: InterpolationColorSpace::Srgba,
        angle: LinearGradient::TO_BOTTOM,
        stops: vec![ColorStop::auto(top), ColorStop::auto(bottom)],
    })
}

/// Labels go with their item (picked up or culled).
fn drop_orphan_labels(
    mut commands: Commands,
    labels: Query<(Entity, &ItemLabel)>,
    items: Query<(), With<GroundItem>>,
) {
    for (e, l) in &labels {
        if !items.contains(l.owner) {
            commands.entity(e).try_despawn();
        }
    }
}

/// The point an item's label is projected from, `UIWorldDescManager::FindWorldPosition`: the item's
/// `Actor::GetCenterOfMass` ([`CentreOfMass`]) through its coords (region rotation + translation).
fn label_anchor(gt: &GlobalTransform, com: Option<&CentreOfMass>) -> Vec3 {
    gt.transform_point(com.map_or(Vec3::ZERO, |c| c.0))
}

/// A label rect in physical px: x, y, w, h.
type LabelRect = [f32; 4];

fn overlaps(a: &LabelRect, b: &LabelRect) -> bool {
    b[0] + b[2] > a[0] && b[0] < a[0] + a[2] && b[1] + b[3] > a[1] && b[1] < a[1] + a[3]
}

/// `UIWorldDescManager::FindWorldPosition` / `UIWorldDescManager::StackLabel`: a label that overlaps one placed
/// before it goes above it (`y = other.y − 4 − h`), again while it overlaps any (`placed` are the earlier labels'
/// stacked rects); off the top of the screen it is not shown (`None`). Within 4 px of its natural place it stays there.
fn stack_label(natural: LabelRect, placed: &[LabelRect]) -> Option<LabelRect> {
    let mut r = natural;
    while let Some(o) = placed.iter().find(|o| overlaps(&r, o)) {
        r[1] = o[1] - LABEL_GAP - r[3];
        if r[1] + r[3] < 0.0 {
            return None;
        }
    }
    if (r[1] - natural[1]).abs() <= LABEL_GAP {
        r = natural;
    }
    Some(r)
}

/// `UIWorldDescManager::Update`: per label id, last frame's offsets from the natural y (the
/// smoothed stacked offset `t`, the shown one `c`); the new stacked offset is approached by min(5·dt, 1) of the gap,
/// and the shown one moves towards that by min((d²·0.2 + 50)·dt, 400·dt, d). A new label shows at its natural y
/// with `t` = its stacked offset.
fn animate_offset(prev: Option<(f32, f32)>, stacked: f32, dt: f32) -> (f32, f32) {
    let (t, c) = match prev {
        Some((t, c)) => (t + (stacked - t) * (dt * 5.0).min(1.0), c),
        None => (stacked, 0.0),
    };
    let d = (t - c).abs();
    let step = ((d * d * 0.2 + 50.0) * dt).min(dt * 400.0).min(d);
    (t, if t <= c { c - step } else { c + step })
}

/// Places the labels in the engine's order (object id ascending, `UIWorldDescManager::SortLabels`; PORT: entity
/// index): natural
/// rect centred `LABEL_LIFT` px above the projected item (floored), stacked ([`stack_label`]) and animated
/// ([`animate_offset`]); then the label under the cursor ([`label_at`], [`HoveredLabel`]), else the world-hovered
/// item's (in Keyboard Only the controller targeting's item, `crate::controller_target`; else the model under the
/// cursor, [`model_under`]).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn place_labels(
    hooks: Res<gd_game_core::hooks::Hooks>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    (mut hovered, mut listed): (ResMut<HoveredLabel>, ResMut<ListedLabels>),
    cams: Query<(&Camera, &GlobalTransform), With<gd_render::camera::GameCamera>>,
    items: Query<
        (
            &GlobalTransform,
            Has<FilteredOut>,
            Option<&gd_game::loot::Tossed>,
            Option<&CentreOfMass>,
        ),
        With<GroundItem>,
    >,
    time: Res<Time>,
    mut labels: Query<(
        Entity,
        &mut ItemLabel,
        &mut Node,
        &mut Visibility,
        &ComputedNode,
        &mut BackgroundGradient,
    )>,
    (options, controller): (
        Res<gd_game_core::options::GameOptions>,
        Res<crate::controller_target::ControllerPick>,
    ),
    (models, buttons, capture): (
        Query<(&GlobalTransform, &ModelBounds), (With<GroundItem>, Without<Unpickable>)>,
        Res<ButtonInput<MouseButton>>,
        Res<gd_ui::state::inventory::UiCapture>,
    ),
    (mut commands, glow): (Commands, Query<Entity, With<LabelGlow>>),
    mut scratch: Local<LabelScratch>,
) {
    let LabelScratch {
        shown,
        placed,
        drawn,
        want,
        cur,
    } = &mut *scratch;
    let Ok((cam, cam_gt)) = cams.single() else {
        hovered.0 = None;
        listed.0.clear();
        listed.1 = None;
        sync_label_glow(&mut commands, &glow, &listed, want, cur);
        return;
    };
    let sf = cam.target_scaling_factor().unwrap_or(1.0);
    shown.clear();
    for (le, mut l, _, mut vis, computed, _) in &mut labels {
        let p = items
            .get(l.owner)
            .ok()
            .and_then(|(gt, filtered, toss, com)| {
                let filtered = filtered || toss.is_some_and(|t| t.wait > 0.0);
                let near = cam_gt.translation().distance(gt.translation()) < 80.0;
                (!filtered && near)
                    .then(|| cam.world_to_viewport(cam_gt, label_anchor(gt, com)).ok())
                    .flatten()
            });
        match p {
            Some(p) => {
                let size = computed.size();
                let x = (p.x * sf - size.x * 0.5).floor();
                let y = (p.y * sf - size.y * 0.5 - LABEL_LIFT).floor();
                shown.push((l.owner, le, [x, y, size.x, size.y]));
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
                l.offsets = None;
            }
        }
    }
    shown.sort_by_key(|(owner, _, _)| owner.index());
    listed.0.clear();
    listed.0.extend(shown.iter().map(|s| s.0));
    let dt = time.delta_secs();
    placed.clear();
    drawn.clear();
    for &(owner, le, natural) in shown.iter() {
        let Ok((_, mut l, mut node, mut vis, _, _)) = labels.get_mut(le) else {
            continue;
        };
        let Some(r) = stack_label(natural, placed) else {
            vis.set_if_neq(Visibility::Hidden);
            l.offsets = None;
            continue;
        };
        placed.push(r);
        let (t, c) = animate_offset(l.offsets, r[1] - natural[1], dt);
        l.offsets = Some((t, c));
        node.left = Val::Px(natural[0] / sf);
        node.top = Val::Px((natural[1] + c) / sf);
        vis.set_if_neq(Visibility::Visible);
        drawn.push((owner, [natural[0], natural[1] + c, natural[2], natural[3]]));
    }
    let cursor = windows
        .single()
        .ok()
        .and_then(|w| gd_ui_kit::cursor_px(&hooks, w).map(|p| p / w.scale_factor()));
    // No label under the cursor: the world-hovered item (`UIWorldDescManager.worldHoveredItem`) when its label is
    // drawn: in Keyboard Only the controller targeting's item (`InGameUI::Update`, after the mouse events), else
    // the item model under the cursor
    // (`model_under`; only when the event is no repeat: no button held on, the world got the mouse).
    let ko_item = match controller.0 {
        gd_game::controller_target::Pick::Item(e) if options.int("movementType") == 2 => Some(e),
        _ => None,
    };
    let fresh = !buttons.any_pressed([MouseButton::Left, MouseButton::Right])
        || buttons.any_just_pressed([MouseButton::Left, MouseButton::Right]);
    let model = || {
        cursor
            .filter(|_| fresh && !capture.0)
            .and_then(|c| cam.viewport_to_world(cam_gt, c).ok())
            .and_then(|ray| model_under(ray, drawn, &models))
    };
    let world = ko_item
        .filter(|e| drawn.iter().any(|(o, _)| o == e))
        .or_else(model);
    hovered.0 = cursor.and_then(|c| label_at(c * sf, drawn)).or(world);
    listed.1 = hovered.0.filter(|_| fresh);
    // The selected label is drawn in the highlight colour (`UIWorldDescManager::Draw`: `LabelEntry.highlighted` →
    // `UIRolloverWidget.useHighlightColor`).
    for (_, mut l, _, _, _, mut bg) in &mut labels {
        let lit = hovered.0 == Some(l.owner);
        if l.lit != lit {
            l.lit = lit;
            *bg = label_background(l.backgrounds[lit as usize]);
        }
    }
    sync_label_glow(&mut commands, &glow, &listed, want, cur);
}

/// [`place_labels`]' per-frame lists, kept for their capacity.
#[derive(Default)]
struct LabelScratch {
    shown: Vec<(Entity, Entity, LabelRect)>,
    placed: Vec<LabelRect>,
    drawn: Vec<(Entity, LabelRect)>,
    want: Vec<Entity>,
    cur: Vec<Entity>,
}

/// The light rig's label highlight (`gd_render::hover`): the markers follow the listed items, cleared
/// the frame an item stops being listed.
fn sync_label_glow(
    commands: &mut Commands,
    glow: &Query<Entity, With<LabelGlow>>,
    listed: &ListedLabels,
    want: &mut Vec<Entity>,
    cur: &mut Vec<Entity>,
) {
    want.clear();
    want.extend(listed.0.iter().chain(listed.1.iter()).copied());
    want.sort_unstable();
    want.dedup();
    cur.clear();
    cur.extend(glow.iter());
    cur.sort_unstable();
    for e in cur.iter().filter(|e| want.binary_search(e).is_err()) {
        commands.entity(*e).remove::<LabelGlow>();
    }
    for e in want.iter().filter(|e| cur.binary_search(e).is_err()) {
        commands.entity(*e).try_insert(LabelGlow);
    }
}

/// The ground item whose label is under the cursor (physical px), `UIWorldDescManager::Update` (mouse path,
/// `UIWorldDescManager.mousePath`): the labels are tested from the last placed to the first against their
/// drawn rect (the shown, eased position: `LabelEntry.shownRect`), half-open `[x, x + w) × [y, y + h)`; the first
/// hit is the selected label (`UIWorldDescManager.selectedLabel`), which a click uses
/// (`UIWorldDescManager::HandleClick` → `ControllerPlayer::HandleActionFromMouse`) and whose tooltip shows. With no
/// label hit the label of the
/// world-hovered item (`UIWorldDescManager.worldHoveredItem`, [`model_under`]; in Keyboard Only the
/// controller targeting's item) is selected, when it is in the list.
fn label_at(cursor: Vec2, drawn: &[(Entity, LabelRect)]) -> Option<Entity> {
    drawn
        .iter()
        .rev()
        .find(|(_, r)| {
            cursor.x >= r[0] && cursor.x < r[0] + r[2] && cursor.y >= r[1] && cursor.y < r[1] + r[3]
        })
        .map(|(e, _)| *e)
}

/// Test hook: `GD_DROP_ITEMS=record;record` drops those items (and 25 iron)
/// around the player once it exists (checks how ground items rest), or
/// `GD_DROP_AFTER` seconds after it does.
#[allow(clippy::too_many_arguments)]
#[cfg(feature = "dev")]
fn test_drops(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    mut done: Local<bool>,
    mut since: Local<f32>,
    time: Res<Time>,
    players: Query<&Transform, With<Player>>,
    a: CharAssets,
) {
    if *done {
        return;
    }
    if players.is_empty() {
        return;
    }
    *since += time.delta_secs();
    if hooks.items.drop_after.is_some_and(|d| *since < d) {
        return;
    }
    let Some(v) = hooks.items.drop_items.clone() else {
        *done = true;
        return;
    };
    let Ok(tf) = players.single() else { return };
    *done = true;
    let idb = a.data.item_db();
    let at = tf.translation + Vec3::new(2.5, 0.0, 0.0);
    let items: Vec<_> = v
        .split(';')
        .filter(|s| !s.is_empty())
        .map(|rec| gd_formats::save::item::Item {
            base_name: rec.to_string(),
            stack_count: 1,
            ..Default::default()
        })
        .collect();
    drop_loot(&mut commands, &idb, at, items, &[25]);
}
