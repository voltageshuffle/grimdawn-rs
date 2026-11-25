//! Paper doll: the player's model in its current equipment, rendered into a
//! window's 3D view (the character window's `characterView`, the
//! Illusionist's). Its item list is read each frame from
//! `gd_game_core::inventory::Inventory::equipped` (each item drawn with its
//! illusion, `look_record`) and the sex from the player's `Loadout`; the body
//! is rebuilt when that list changes.
//!
//! `UISceneView::Load` / `UISceneView::Render`: the record gives the view rect
//! (`x y xSize ySize`, UI scaled), `cameraFov` (degrees, vertical, aspect 1 over
//! the view rect: `Camera::GetSubCameraToScreenMatrix`), `targetPointName` (an
//! attach point replacing `targetPoint`; empty in both stock views),
//! `cameraNearPlane`, `cameraFarPlane`, `targetPoint`, `targetOffset`,
//! `lightDirection` (towards the light, as `AmbianceManager::GetLightDirection`),
//! `lightColor`, `skyAmbientColor`, `groundAmbientColor`, `backgroundTexture`,
//! `backgroundColor`, `worldLighting` / `worldBackground` (off in both stock
//! doll views). Each frame: R = rotation about Y by the view's angle; eye =
//! targetPoint + R·targetOffset looking at targetPoint (up +Y); one
//! directional light along R·lightDirection with the record's colour, sky /
//! ground ambient from the record; post effects, depth pass, shadows, fog and
//! light rigs off; clear colour `backgroundColor` and the background texture
//! over the viewport (`GraphicsMTRenderer::RenderBackground`); the entity is a
//! `GAME::Paperdoll` in its own 20 m region (`__PaperDollRegion`) at the origin,
//! facing +Z. `Paperdoll::SetCharacter` / `SyncItems` copy the character's mesh
//! and its `EquipManager` items (the active weapon set), `UpdateIdleAnimation`
//! loops the long idle ([`long_idle`]); `SetItemAppearance` swaps one slot's look
//! (`UIIllusionist::PreviewItem` / `UIIllusionist::UpdatePreview`).
//!
//! Rotation (`UICharacterWindow::HandleRotateButtons` / `HandleViewMouse` /
//! `UpdateRotation`, reset to 0 on `InventoryWindow::OnHide`): while a rotate
//! button is held, every 25 ms the angle += 0.15 rad (left) / −= 0.15 (right),
//! mod 2π; dragging with the left button pressed in the view: angle −=
//! Δx · 0.01 per pixel; the wheel over the view: angle −= wheel · 0.1.
//! The main menu's character turns by [`menu_drag`].
//!
//! Port: a second camera per view renders the doll's render layer into an
//! image ([`DollView::layer`]) drawn by a UI material that shows its
//! gamma-space colours as they are (`doll.wesl`). The light rig rides in that
//! camera's `ColorGrading` (read by `gd_standard.wesl` `shade_doll`: gain =
//! light colour, lift = sky, gamma = ground, contrast = direction to the light,
//! exposure 1 marks the view).
//! PORT: the view clears to transparent and is drawn over the window (the
//! engine's background texture draw does not show; why is unchecked). Worn
//! items' mesh effects are drawn on the doll's layer (`fx::items`); a held item
//! clicked on the character window's doll goes to its slot
//! (`CharacterWindowScene::HandleItemClick`). While the player is shifted the
//! doll wears the form with its armour on ([`doll_forms`]).
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, RenderTarget};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::view::ColorGrading;
use bevy::window::PrimaryWindow;
use gd_render::character::{self, Body, CharAssets};
use gd_render::model::Skeleton;
use gd_ui::doll::{Dolls, SceneView};
use gd_ui_kit::contains;

pub struct DollPlugin;

impl Plugin for DollPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (rotate_input, menu_drag, rotate_looks, sync_dolls).chain(),
        )
        .add_systems(
            Update,
            long_idle.after(gd_game_core::character::CharacterStage::Drive),
        )
        .add_systems(
            Update,
            menu_idle.before(gd_game_core::character::CharacterStage::Drive),
        )
        .add_systems(
            Update,
            doll_forms
                .after(gd_game_core::shapeshift::ShapeshiftSet::Wear)
                .before(gd_render::character::CharacterRenderStage::WearForms),
        )
        .add_systems(
            PostUpdate,
            keep_layers.after(gd_render::model::ModelSpawnSet),
        );
    }
}

pub use gd_render::character::{DollBody, DollView};

const VIEWS: [DollView; 3] = [DollView::Character, DollView::Illusionist, DollView::Menu];

fn rotate_input(
    mut dolls: ResMut<Dolls>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    time: Res<Time<Real>>,
) {
    let cursor = window.single().ok().and_then(gd_ui_kit::cursor_px);
    let notches: f32 = wheel.read().map(|e| e.y.signum()).sum();
    let dt = time.delta_secs() * 1000.0;
    // The menu swallows `ButtonInput`: its character turns by `menu_drag`.
    for d in dolls
        .0
        .iter_mut()
        .filter(|(v, _)| **v != DollView::Menu)
        .map(|(_, d)| d)
    {
        if !d.shown {
            d.hold = None;
            d.drag = None;
            continue;
        }
        if buttons.just_pressed(MouseButton::Left)
            && let Some(p) = cursor
        {
            if let Some(i) = d
                .buttons
                .iter()
                .position(|b| b.as_ref().is_some_and(|b| contains(&b.rect, p)))
            {
                d.hold = Some((i == 1, 0.0));
            } else if d.rect.as_ref().is_some_and(|r| contains(r, p)) {
                d.drag = Some(p.x);
            }
        }
        if !buttons.pressed(MouseButton::Left) {
            d.hold = None;
            d.drag = None;
        }
        if let Some((right, next)) = d.hold.as_mut() {
            *next -= dt;
            while *next < 0.0 {
                d.rotation =
                    (d.rotation + if *right { -0.15 } else { 0.15 }) % std::f32::consts::TAU;
                *next += 25.0;
            }
        }
        if let (Some(x0), Some(p)) = (d.drag, cursor) {
            d.rotation -= (p.x - x0) * 0.01;
            d.drag = Some(p.x);
        }
        if notches != 0.0 && cursor.is_some_and(|p| d.rect.as_ref().is_some_and(|r| contains(r, p)))
        {
            d.rotation -= notches * 0.1;
        }
    }
}

/// Turning the main menu's character by dragging (`MainMenu::HandleMouseEvent` /
/// `MainMenu::SetCharacterRotation`): with no sub-window open and the press not taken by the menu's widgets,
/// a left-button press starts the drag; each mouse move turns the character by `(x − last x) · 0.01` rad
/// on the record's `mmPlayerRotation` Y; the release ends it, wherever it happens. Reads raw button messages.
/// PORT: "taken by a widget" is a press on a [`crate::menu::MenuLayout`] rect; the action-0 rotation
/// (`MainMenu::RotateCharacter`; its binding unchecked) is not ported.
#[allow(clippy::too_many_arguments)]
fn menu_drag(
    mut dolls: ResMut<Dolls>,
    mut clicks: MessageReader<bevy::input::mouse::MouseButtonInput>,
    window: Query<&Window, With<PrimaryWindow>>,
    menu: Option<Res<crate::menu::MainMenu>>,
    layout: Option<Res<crate::menu::MenuLayout>>,
    options: Res<gd_ui::state::options::OptionsWindow>,
    credits: Res<crate::ui::credits::Credits>,
) {
    use bevy::input::ButtonState;
    let cursor = window.single().ok().and_then(gd_ui_kit::cursor_px);
    let d = dolls.0.entry(DollView::Menu).or_default();
    let free = menu
        .as_ref()
        .is_some_and(|m| m.window == crate::menu::MenuWindow::None)
        && !options.open
        && !credits.open;
    for e in clicks.read().filter(|e| e.button == MouseButton::Left) {
        match e.state {
            ButtonState::Pressed if free && d.shown => {
                if let Some(p) = cursor
                    && layout.as_ref().is_none_or(|l| l.at(p).is_none())
                {
                    d.drag = Some(p.x);
                }
            }
            ButtonState::Released => d.drag = None,
            _ => {}
        }
    }
    if !d.shown || menu.is_none() {
        d.drag = None;
    }
    if let (Some(x0), Some(p)) = (d.drag, cursor) {
        d.rotation += (p.x - x0) * 0.01;
        d.drag = Some(p.x);
    }
}

/// The rotate buttons' `ButtonStatic` looks (`ButtonStatic::Render`): down
/// while held, in-focus while hovered; drawn over the window as an overlay node.
fn rotate_looks(
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    mut dolls: ResMut<Dolls>,
    window: Query<&Window, With<PrimaryWindow>>,
) {
    let Ok(win) = window.single() else { return };
    let cursor = gd_ui_kit::cursor_px(win);
    #[cfg(feature = "dev")]
    let held = hooks.render.doll_hold;
    #[cfg(not(feature = "dev"))]
    let held = None;

    let inv = 1.0 / win.scale_factor();
    for d in dolls.0.values_mut() {
        let want = if !d.shown {
            None
        } else if let Some(i) = d.hold.map(|(right, _)| right as usize).or(held) {
            Some((i, true))
        } else {
            d.buttons
                .iter()
                .position(|b| {
                    b.as_ref()
                        .is_some_and(|b| cursor.is_some_and(|p| contains(&b.rect, p)))
                })
                .map(|i| (i, false))
        };
        if d.button_look.map(|l| l.0) == want {
            continue;
        }
        if let Some((_, e)) = d.button_look.take() {
            commands.entity(e).try_despawn();
        }
        let Some((i, down)) = want else { continue };
        let Some(b) = d.buttons[i].as_ref() else {
            continue;
        };
        let Some(img) = (if down { b.down.clone() } else { b.over.clone() }) else {
            continue;
        };
        let r = b.rect;
        let e = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(r.x * inv),
                    top: Val::Px(r.y * inv),
                    width: Val::Px(r.w * inv),
                    height: Val::Px(r.h * inv),
                    ..default()
                },
                ImageNode::new(img).with_mode(bevy::ui::widget::NodeImageMode::Stretch),
                GlobalZIndex(5),
                Name::new("doll rotate button"),
            ))
            .id();
        d.button_look = Some(((i, down), e));
    }
}

/// The worn items (Illusionist: with the pending picks), each with its illusion.
fn looks(
    view: DollView,
    inv: &gd_game_core::inventory::Inventory,
    trade: &gd_ui::state::trade::Trade,
) -> Vec<String> {
    inv.equipped
        .iter()
        .enumerate()
        .filter_map(|(slot, it)| {
            let it = it.as_ref()?;
            if view == DollView::Illusionist
                && let Some((_, l)) = trade.illusion.picks.iter().find(|(s, _)| *s == slot)
            {
                return Some(l.clone());
            }
            Some(gd_sim::items::illusion::look_record(it).to_owned())
        })
        .collect()
}

/// Shows / hides each view, rebuilds its body when the look changes and
/// keeps its camera on the view's rect, angle and light rig.
#[allow(clippy::too_many_arguments)]
fn sync_dolls(
    mut commands: Commands,
    mut dolls: ResMut<Dolls>,
    panels: Res<gd_ui::UiPanels>,
    trade: Res<gd_ui::state::trade::Trade>,
    inv: Option<Res<gd_game_core::inventory::Inventory>>,
    players: Query<&gd_game_core::player::Loadout, With<gd_game_core::player::Player>>,
    bodies: Query<&Body>,
    skels: Query<&Skeleton>,
    mut cams: Query<(
        &mut Camera,
        &mut Transform,
        &mut Projection,
        &mut ColorGrading,
        &mut RenderTarget,
    )>,
    menu: Option<Res<crate::menu::MainMenu>>,
    scene: Option<Res<gd_render::render::backdrop::MenuBackdrop>>,
    mut menu_body: Query<&mut Transform, (With<DollBody>, Without<Camera>)>,
    mut a: CharAssets,
) {
    let female = players.single().ok().map(|l| l.female);
    for view in VIEWS {
        let d = dolls.0.entry(view).or_default();
        let open = match view {
            DollView::Character => panels.character,
            DollView::Illusionist => matches!(
                trade.open,
                Some(crate::trade::TradeOpen::Illusionist { .. })
            ),
            DollView::Menu => menu.is_some() && scene.is_some(),
        };
        if !open {
            d.shown = false;
        }
        let stale = match (d.shown, view, female, inv.as_deref()) {
            (false, ..) => d.look.is_some(),
            (true, DollView::Menu, ..) => d.preset != d.look,
            (true, _, Some(f), Some(_)) => {
                d.look.is_none()
                    || d.look.as_ref().is_some_and(|l| l.0 != f)
                    || inv.as_ref().is_some_and(|i| i.is_changed())
                    || trade.is_changed()
            }
            _ => d.look.is_some(),
        };
        if stale {
            let want = match (d.shown, view, female, inv.as_deref()) {
                (false, ..) => None,
                (true, DollView::Menu, ..) => d.preset.clone(),
                (true, _, Some(f), Some(inv)) => Some((f, looks(view, inv, &trade))),
                _ => None,
            };
            if want != d.look {
                if let Some(b) = d.body.take() {
                    if let Ok(body) = bodies.get(b) {
                        character::despawn_body(&mut commands, b, body, &skels);
                    }
                    commands.entity(b).try_despawn();
                }
                // `Actor::SetScale(mmPlayerScale)` replaces the record's scale.
                let place = match (view, &scene) {
                    (DollView::Menu, Some(s)) => Some(s.view.player),
                    _ => None,
                };
                if let Some((f, items)) = &want {
                    d.body = character::spawn_player(
                        &mut commands,
                        &mut a,
                        place.unwrap_or(Transform::from_translation(view.origin())),
                        *f,
                        items,
                    );
                    if let Some(b) = d.body {
                        commands
                            .entity(b)
                            .insert((Name::new(format!("doll {}", view.name())), DollBody(view)));
                        if let Some(tf) = place {
                            commands.entity(b).insert((tf, GlobalTransform::from(tf)));
                        }
                    }
                }
                d.look = want;
                // `MainMenu::SelectCharacter`: a (re)selected menu character restarts at angle 0.
                if view == DollView::Menu {
                    d.rotation = 0.0;
                    d.drag = None;
                }
            }
        }
        if view == DollView::Menu {
            // R(X, rx)·R(Y, ry + angle)·R(Z, rz).
            if let (Some(s), Some(mut t)) = (&scene, d.body.and_then(|b| menu_body.get_mut(b).ok()))
            {
                let (rz, ry, rx) = s.view.player.rotation.to_euler(EulerRot::ZYX);
                let r = Quat::from_euler(EulerRot::ZYX, rz, ry + d.rotation, rx);
                if t.rotation != r {
                    t.rotation = r;
                }
                let p = &s.view.player;
                if (t.translation, t.scale) != (p.translation, p.scale) {
                    (t.translation, t.scale) = (p.translation, p.scale);
                }
            }
            continue;
        }
        let Some(rec) = (d.shown).then(|| a.data.db.get(&d.record)).flatten() else {
            if let Some((mut cam, ..)) = d.camera.and_then(|c| cams.get_mut(c).ok()) {
                cam.is_active = false;
            }
            continue;
        };
        let sv = SceneView::from_record(rec);
        let Some((size, image, _)) = d.target.clone() else {
            continue;
        };
        let origin = view.origin();
        let tf = Transform::from_translation(origin + sv.eye(d.rotation))
            .looking_at(origin + sv.target_point, Vec3::Y);
        let proj = Projection::Perspective(PerspectiveProjection {
            fov: sv.fov,
            near: sv.near,
            far: sv.far,
            aspect_ratio: size.x as f32 / size.y as f32,
            ..default()
        });
        let grading = sv.grading();
        match d.camera.and_then(|c| cams.get_mut(c).ok()) {
            Some((mut cam, mut t, mut p, mut g, mut target)) => {
                cam.is_active = true;
                if !matches!(&*target, RenderTarget::Image(i) if i.handle == image) {
                    *target = RenderTarget::Image(image.clone().into());
                }
                *t = tf;
                *p = proj;
                if *g != grading {
                    *g = grading;
                }
            }
            None => {
                d.camera = Some(
                    commands
                        .spawn((
                            Camera3d::default(),
                            Camera {
                                order: -2,
                                clear_color: ClearColorConfig::Custom(Color::NONE),
                                ..default()
                            },
                            RenderTarget::Image(image.clone().into()),
                            Tonemapping::None,
                            DebandDither::Disabled,
                            Msaa::Off,
                            RenderLayers::layer(view.layer()),
                            proj,
                            grading,
                            tf,
                            Name::new(format!("doll camera {}", view.name())),
                        ))
                        .id(),
                );
            }
        }
    }
}

/// Puts every mesh of a doll on its view's layer only and out of the sun's shadow.
fn keep_layers(
    mut commands: Commands,
    dolls: Query<(Entity, &DollBody, &Body)>,
    children: Query<&Children>,
    layers: Query<Option<&RenderLayers>, With<Mesh3d>>,
) {
    for (root, view, body) in &dolls {
        let Some(want) = view.0.own_layer() else {
            continue;
        };
        let roots = std::iter::once(root).chain(body.weapons.iter().map(|w| w.root));
        for r in roots {
            for e in std::iter::once(r).chain(children.iter_descendants(r)) {
                if let Ok(l) = layers.get(e)
                    && l != Some(&want)
                {
                    commands
                        .entity(e)
                        .try_insert((want.clone(), NotShadowCaster));
                }
            }
        }
    }
}

/// The character window's and the Illusionist's dolls wear the player's
/// shapeshift form (`Paperdoll::SyncMesh`): the doll takes the override mesh's
/// file, scale and textures and plays its override animation set
/// (`Character.overrideAnimationSet`). The human head mesh goes; items and
/// default pieces (`SyncItems`, `SetupDefaultEquip`) stay attached and drawn:
/// - items are placed by attach point name (`character::slot_point`, missing =
///   identity); weapons go to the form's `R Hand` / `L Hand`; stock forms have
///   `Head` on the were forms and the beaver only; none has `Chest`, `Legs`...
/// - armour is skinned by bone name (`gd_sim::anim::pose_abs_attached_into`):
///   only the direwolf rig shares bones (25) with human armour; the others keep
///   the bind pose.
///
/// PORT: a shifted doll's worn effects on a head piece stay at the actor's
/// frame (`fx::items`), without the `Head` lift. The menu's character never shifts.
fn doll_forms(
    players: Query<
        &gd_render::character::Character,
        (With<gd_game_core::player::Player>, Without<DollBody>),
    >,
    mut dolls: Query<(&DollBody, &mut gd_render::character::Character)>,
) {
    let Ok(p) = players.single() else { return };
    for (view, mut c) in &mut dolls {
        if view.0 == DollView::Menu || c.form_mesh() == p.form_mesh() {
            continue;
        }
        match p.form_mesh() {
            Some(mesh) => c.wear_form(
                mesh,
                p.body.clone(),
                Some(p.anims.clone()),
                p.form_records().cloned().unwrap_or_default(),
            ),
            None => c.clear_form(),
        }
    }
}

/// The doll loops its hand state's long idle (`Paperdoll::UpdateIdleAnimation`:
/// the set's `LongIdle` slot, 16 + 8 · type), not the world's attack idle. One
/// variant is picked (`AnimationSlot::PickVariant`) when the hand state changes
/// and played looped. Port: started as a scripted animation and switched to
/// looping; its first wrap ends the scripted state.
fn long_idle(mut dolls: Query<(&DollBody, &mut gd_render::character::Character)>) {
    for (body, mut c) in &mut dolls {
        if body.0 == DollView::Menu {
            continue;
        }
        if c.playing == gd_sim::anim::AnimType::LongIdle {
            if let Some(cur) = c.channel.current.as_mut().filter(|cur| !cur.looped) {
                cur.looped = true;
            }
        } else if c.state != gd_render::character::CharState::Scripted {
            c.play_scripted(gd_sim::anim::AnimType::LongIdle);
        }
    }
}

/// `ControllerPlayerStateMenuIdle` / `MenuIdleAction::Execute`: the character idles in
/// [`gd_game_core::character::IdleMode::Menu`] (`MenuIdle` looped, a `MenuFidget` at a loop's end once the
/// transition timer is up).
fn menu_idle(mut dolls: Query<(&DollBody, &mut gd_render::character::Character)>) {
    use gd_game_core::character::{CharState, IdleMode};
    for (body, mut c) in &mut dolls {
        if body.0 == DollView::Menu && c.idle != IdleMode::Menu {
            c.idle = IdleMode::Menu;
            c.state = CharState::Idle;
        }
    }
}
