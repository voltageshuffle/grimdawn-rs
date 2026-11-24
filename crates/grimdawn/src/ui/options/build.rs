use super::*;
use gd_ui::state::options::{OptionsWindow, Tab};

#[derive(Component)]
pub(super) struct OptionsRoot;

/// The shown rollover: its tag (a control's rollover tag, or an open dropdown row's full label), root and text size
/// (physical px).
#[derive(Default)]
pub(super) struct Tip {
    tag: Option<String>,
    root: Option<Entity>,
    size: Vec2,
}

/// The controls' rollovers: the menu manager's shared rollover box
/// ([`gd_ui::rollover::menu_tip`]). Grim Dawn.exe: a hovered check box
/// (`MenuButton::HandleMouseEvent`), slider (knob or rect, `MenuSlider::HandleMouseEvent`) or dropdown
/// (`MenuDropBox::HandleDropMouse`) hands its rollover tag to the
/// menu manager (`MenuManager::SetRollover`), which localises it and splits the lines at `^n`
/// (`MenuRolloverBox::SetText`,
/// no wrapping); the box draws (`MenuRolloverBox::Draw`) once 500 ms have passed since the first text was ever set (the
/// timer `MenuRolloverBox.firstTextTime` is set once and only the constructor clears it). `first` keeps that one-shot
/// start for the whole process (never reset): a new tag spawns a fresh root whose [`crate::ui::tip::TipDelay`] is
/// seeded with the elapsed time, and `tip::delay_tips` drives its visibility (instant while paused). The box moves with
/// the cursor in place (the cursor is not in its identity: no rebuild while moving).
/// PORT: while a dropdown's list is open only its rows show a rollover — the row's full label (the drawn text is
/// ellipsised), drawn without localisation ([`gd_ui::rollover::menu_tip_text`]); hidden while a binding waits for
/// a key; in the main menu (no session, `delay_tips` is in `UiPlaySet`) the one-shot timer shows it here itself.
#[allow(clippy::too_many_arguments)]
pub(super) fn rollover(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    win: Res<OptionsWindow>,
    layout: Res<OptionsLayout>,
    options: Res<GameOptions>,
    state: Res<State<gd_ui::state::session::AppState>>,
    mut roots: Query<(&mut Node, &mut Visibility), With<gd_ui::rollover::MenuTipRoot>>,
    mut tip: Local<Tip>,
    mut first: Local<Option<f32>>,
    time: Res<Time<Real>>,
    mut assets: gd_ui_kit::UiAssets,
) {
    let Ok(w) = window.single() else { return };
    let p = gd_ui_kit::cursor_px(&assets.hooks, w);
    let tag = p
        .filter(|_| win.open && win.rebind.is_none())
        .and_then(|p| {
            if win.dropdown.is_some() {
                // The open list's rows show their full label (the drawn rows are ellipsised).
                match hit_at(
                    &layout,
                    win.draft.as_ref().unwrap_or(&options),
                    win.scroll,
                    win.drop_top,
                    p,
                ) {
                    Some(Hit::DropRow(k)) => layout.drop_rows.get(k).cloned(),
                    _ => None,
                }
            } else {
                layout
                    .tips
                    .iter()
                    .find(|(r, _)| contains(r, p))
                    .map(|t| t.1.clone())
            }
        });
    if tag != tip.tag {
        if let Some(e) = tip.root.take() {
            commands.entity(e).try_despawn();
        }
        tip.tag = tag.clone();
        if let (Some(tag), Some(p)) = (tag, p) {
            let now = time.elapsed_secs();
            let first = first.get_or_insert(now);
            let screen = Vec2::new(w.physical_width() as f32, w.physical_height() as f32);
            let root = gd_ui_kit::spawn_root(
                &mut commands,
                (
                    gd_ui::rollover::MenuTipRoot,
                    gd_ui::hud::KeepsShown,
                    Visibility::Hidden,
                    gd_ui_kit::tip::TipDelay::menu(
                        tag.clone(),
                        gd_ui::rollover::MENU_TIP_DELAY,
                    )
                    .with_ms(((now - *first) * 1000.0).max(0.0)),
                ),
                gd_ui::window::Z_OPTIONS + 5,
            );
            // Drawn in screen pixels: the style's font size and the paddings are not UI-scaled.
            let mut ui = assets.ui(
                &mut commands,
                &data,
                root,
                1.0,
                1.0 / w.scale_factor(),
                screen,
            );
            let size = if win.dropdown.is_some() {
                gd_ui::rollover::menu_tip_text(&mut ui, &tag)
            } else {
                gd_ui::rollover::menu_tip(&mut ui, &tag)
            };
            let o = gd_ui::rollover::menu_tip_origin(p, size, screen) / w.scale_factor();
            commands.entity(root).insert(Node {
                position_type: PositionType::Absolute,
                left: Val::Px(o.x),
                top: Val::Px(o.y),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            });
            tip.root = Some(root);
            tip.size = size;
        }
    }
    let (Some(e), Some(p)) = (tip.root, p) else {
        return;
    };
    let Ok((mut node, mut vis)) = roots.get_mut(e) else {
        return;
    };
    let screen = Vec2::new(w.physical_width() as f32, w.physical_height() as f32);
    let o = gd_ui::rollover::menu_tip_origin(p, tip.size, screen) / w.scale_factor();
    node.left = Val::Px(o.x);
    node.top = Val::Px(o.y);
    if *state.get() != gd_ui::state::session::AppState::InGame {
        let shown = first.is_some_and(|t| {
            time.elapsed_secs() - t >= gd_ui::rollover::MENU_TIP_DELAY / 1000.0
        });
        vis.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// A state-dependent image node: `build` draws its initial state and [`live`]
/// swaps the bitmap (and moves the knob) in place — a hover, drag or enabled
/// change never rebuilds the window.
#[derive(Component)]
pub(super) enum Look {
    /// A tab quad: the open tab keeps its down bitmap (the tab is in the signature).
    Tab {
        tab: Tab,
        up: Handle<Image>,
        over: Handle<Image>,
    },
    /// A check box quad, its states indexed by [`check_state`].
    Check {
        k: &'static str,
        states: [(Handle<Image>, Color); 5],
    },
    /// A slider knob quad: `x0..x1` the knob centre's travel, `w` its width
    /// (physical px). The hit rect stays at the build position.
    Knob {
        k: &'static str,
        bitmaps: [Handle<Image>; 3],
        x0: f32,
        x1: f32,
        w: f32,
        inv: f32,
    },
    /// A bottom button quad (Default / Apply / Close); its label is a child.
    Bottom {
        hit: Hit,
        up: Handle<Image>,
        over: Option<Handle<Image>>,
        disabled: Option<Handle<Image>>,
    },
    /// A bottom button's label, dimmed while its button is disabled.
    Label(Hit),
}

/// A dropdown's value text ([`Ui::live_text`]): [`live`] sets it from the draft, ellipsised
/// to `max_w` (the box's inner width, physical px) in `style`.
#[derive(Component)]
pub(super) struct DropText {
    pub(super) shown: Shown,
    pub(super) style: String,
    pub(super) max_w: f32,
}

/// A dropdown row's highlight (the select fill and its two border lines):
/// [`live`] shows the hovered row's, else the current choice's.
#[derive(Component)]
pub(super) struct DropRowHl(pub(super) usize);

/// Apply's button-sound gate: [`live`] shows it while Apply is enabled
/// (`play_button_sounds` ignores hidden nodes). It carries the window's z so
/// the sounds registered under it rank with the window's own buttons.
#[derive(Component)]
pub(super) struct ApplyGate;

/// A node [`live`] moves with a list's scroll (the clipped pane's inner node
/// or the scroll bar's thumb), so a scroll never rebuilds the window.
#[derive(Component)]
pub(super) enum Scroll {
    /// The dropdown list's pane ([`gd_ui::window::pane`]): `max` its last first row.
    DropPane {
        clip_y: f32,
        row_h: f32,
        inv: f32,
        max: usize,
    },
    /// The dropdown list's grey thumb: `base_y + first * step`.
    DropThumb {
        base_y: f32,
        step: f32,
        inv: f32,
        max: usize,
    },
    /// The key-binding list's pane.
    KeyPane { clip_y: f32, row_h: f32, inv: f32 },
    /// The key-binding list's scroll bar slider: `track0 + travel * scroll / span`.
    KeyThumb {
        track0: f32,
        travel: f32,
        span: f32,
        inv: f32,
    },
}

/// A 3 × 3 border set (`borderthin_*.tex`: lt ct rt / lm rm / lb cb rb)
/// around `r` (inside it), with a fill.
pub(super) fn thin_box(ui: &mut Ui, pattern: &str, r: URect, fill: Color) {
    ui.solid(r, fill);
    let get = |ui: &mut Ui, k: &str| ui.image(&pattern.replace('*', k));
    let Some((lt, sz)) = get(ui, "lt") else {
        return;
    };
    let (cw, ch) = (ui.sc(sz.x as f32), ui.sc(sz.y as f32));
    ui.quad_stretch(lt, URect::new(r.x, r.y, cw, ch), Color::WHITE);
    let parts: [(&str, f32, f32, f32, f32); 7] = [
        ("rt", r.x + r.w - cw, r.y, cw, ch),
        ("lb", r.x, r.y + r.h - ch, cw, ch),
        ("rb", r.x + r.w - cw, r.y + r.h - ch, cw, ch),
        ("ct", r.x + cw, r.y, r.w - 2.0 * cw, ch),
        ("cb", r.x + cw, r.y + r.h - ch, r.w - 2.0 * cw, ch),
        ("lm", r.x, r.y + ch, cw, r.h - 2.0 * ch),
        ("rm", r.x + r.w - cw, r.y + ch, cw, r.h - 2.0 * ch),
    ];
    for (k, x, y, w, h) in parts {
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        if let Some((img, _)) = get(ui, k) {
            ui.quad_stretch(img, URect::new(x, y, w, h), Color::WHITE);
        }
    }
}

pub(super) fn color4(rec: &Record, k: &str) -> Color {
    let f = |c: &str| rec.f32(&format!("{k}{c}")).unwrap_or(1.0);
    Color::srgba(f("R"), f("G"), f("B"), f("A"))
}

/// A thin button (Default / Apply / Close): bitmap and centred label, drawn in
/// its enabled / disabled up look; [`live`] swaps the bitmap on hover and dims
/// the label (a child of the quad, so it hides with it).
pub(super) fn bottom_button(
    ui: &mut Ui,
    rec: &Record,
    centre_x: f32,
    bottom: f32,
    hit: Hit,
    enabled: bool,
) -> Option<URect> {
    let (up, size) = rec
        .str("bitmapNameUp")
        .map(str::to_owned)
        .and_then(|p| ui.image(&p))?;
    let over = rec
        .str("bitmapNameInFocus")
        .map(str::to_owned)
        .and_then(|p| ui.image(&p))
        .map(|(h, _)| h);
    let disabled = rec
        .str("bitmapNameDisabled")
        .map(str::to_owned)
        .and_then(|p| ui.image(&p))
        .map(|(h, _)| h);
    let r = URect::new(
        (centre_x + ui.sc(rec.f32("bitmapPositionX").unwrap_or(0.0))).round(),
        bottom - ui.sc(rec.f32("bitmapPositionY").unwrap_or(0.0)),
        ui.sc(size.x as f32),
        ui.sc(size.y as f32),
    );
    let img = if enabled {
        up.clone()
    } else {
        disabled.clone().unwrap_or_else(|| up.clone())
    };
    let e = ui.quad(img, r, None, Color::WHITE);
    // A disabled button without a disabled bitmap is hidden, as the old
    // None-return would ([`live`] keeps it hidden while disabled).
    if !enabled && disabled.is_none() {
        ui.commands.entity(e).insert(Visibility::Hidden);
    }
    let root = ui.root;
    ui.root = e;
    if let (Some(style), Some(tag)) = (rec.str("textFont"), rec.str("textTag")) {
        let t = ui.fonts.tag(tag).to_string();
        if let Some(le) = gd_ui_kit::widgets::label(
            ui,
            URect::new(0.0, 0.0, r.w, r.h),
            style,
            TextAlign::Center,
            &t,
        ) {
            ui.commands.entity(le).insert(Look::Label(hit));
        }
    }
    ui.root = root;
    ui.commands.entity(e).insert(Look::Bottom {
        hit,
        up,
        over,
        disabled,
    });
    Some(r)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    win: Res<OptionsWindow>,
    options: Res<GameOptions>,
    bindings: Res<KeyBindings>,
    mut layout: ResMut<OptionsLayout>,
    roots: Query<Entity, With<OptionsRoot>>,
    devices: Res<crate::audio::devices::AudioDevices>,
    adapters: Res<crate::render_devices::RenderAdapters>,
    monitors: Query<(
        Entity,
        &bevy::window::Monitor,
        Option<&bevy::window::PrimaryMonitor>,
    )>,
    mut fresh: gd_game_core::session::NewSession,
    mut built: Local<
        Rebuild<(
            UVec2,
            bool,
            Tab,
            Option<usize>,
            Option<(u32, usize)>,
            i32,
            String,
            i32,
            u64,
            u64,
            u32,
        )>,
    >,
    mut assets: gd_ui_kit::UiAssets,
) {
    if fresh.begun() {
        built.reset();
    }
    let Ok(w) = window.single() else { return };
    let size = UVec2::new(w.physical_width(), w.physical_height());
    // Only layout state plus the tags generation: hover, drags, scrolls and the draft's values are live
    // ([`live`]); the movement type changes the layout (the key list's WASD rows),
    // and the draft's language / device rebuild the closed dropdown box on a pick.
    // No draft / bindings clone before the early-return.
    let movement = win
        .draft
        .as_ref()
        .map_or(options.int("movementType"), |d| d.int("movementType"));
    let language = win
        .draft
        .as_ref()
        .map_or(language_option(&options), |d| language_option(d))
        .to_string();
    let device = win
        .draft
        .as_ref()
        .map_or(options.int("device"), |d| d.int("device"));
    let sig = (
        size,
        win.open,
        win.tab,
        win.dropdown,
        win.rebind,
        movement,
        language,
        device,
        win.keys_ver,
        // A language / mod swap reloads the tags (`FontCache::generation`): rebuild the tag-resolved row
        // labels and dropdown values, as the other UI groups' signatures do.
        assets.fonts.generation,
        // An applied UI Scale rebuilds the open window at the new size.
        assets.scale(size).to_bits(),
    );
    if size.x == 0 || !built.changed(sig) {
        return;
    }
    gd_ui::window::despawn_all(&mut commands, &roots);
    *layout = OptionsLayout::default();
    if !win.open {
        return;
    }
    let draft = win.draft.clone().unwrap_or_else(|| options.clone());
    let keys_draft = win.keys.clone().unwrap_or_else(|| bindings.clone());
    let (Some(rec), Some(consts)) = (data.db.get(RECORD), data.db.get(CONSTANTS)) else {
        return;
    };
    let s = assets.scale(size);
    let root = gd_ui_kit::spawn_root(&mut commands, OptionsRoot, gd_ui::window::Z_OPTIONS);
    commands.entity(root).insert(gd_ui::hud::KeepsShown);
    // The dropdown values' tag table ([`ChoiceCtx`]): an Arc clone, so the `Ui`'s mutable borrow of the
    // fonts cache below does not conflict with the context's borrow.
    let tags = assets.fonts.tags.clone();
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        s,
        1.0 / w.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let languages = data.available_languages();
    let ctx = ChoiceCtx::new(
        &devices,
        crate::settings::primary_monitor(&monitors),
        ui.screen,
        &adapters,
        &languages,
        &tags,
    );
    // The menu layer's window: `windowDefaultX/Y` is the top-left corner's
    // offset from the aligned point (the screen centre here), not centred
    // on it.
    let def = WindowDef::from_record(rec);
    let wr = URect::new(
        (ui.screen.x * 0.5 + def.x * s).round(),
        (ui.screen.y * 0.5 + def.y * s).round(),
        ui.sc(def.w),
        ui.sc(def.h),
    );
    let o = Vec2::new(wr.x, wr.y);
    // The window takes the whole screen's mouse (`ui/widgets.rs`, button sounds): the HUD and the windows behind
    // it neither sound nor press; its own buttons, registered after this, are in front.
    ui.sound_blocker(URect::new(0.0, 0.0, ui.screen.x, ui.screen.y));
    // Reference-shot coordinates → screen.
    let at =
        |ui: &Ui, x: f32, y: f32| Vec2::new(o.x + ui.sc(x - ORIGIN.x), o.y + ui.sc(y - ORIGIN.y));
    let field = |k: &str| rec.str(k).unwrap_or("").to_string();
    if let Some(bg) = ui.record(&field("backgroundImage"))
        && let Some(path) = bg.str("bitmapName")
    {
        ui.bitmap(path, 0.0, 0.0, o);
    }
    // Title, centred on its `bitmapPositionY`.
    if let Some(t) = ui.record(&field("windowTitle"))
        && let (Some(style), Some(tag)) = (t.str("textFont").map(str::to_owned), t.str("textTag"))
    {
        let text = ui.fonts.tag(tag).to_string();
        let h = ui.sc(t.f32("maxHeight").unwrap_or(30.0));
        let y = o.y + ui.sc(t.f32("bitmapPositionY").unwrap_or(0.0)) - h * 0.5;
        gd_ui_kit::widgets::label(
            &mut ui,
            URect::new(o.x, y.round(), wr.w, h),
            &style,
            TextAlign::Center,
            &text,
        );
    }
    // Tabs.
    for (i, k) in TAB_FIELDS.iter().enumerate() {
        let Some(b) = ui.record(&field(k)) else {
            continue;
        };
        let tab = Tab::ALL[i];
        let Some((up, sz)) = b
            .str("bitmapNameUp")
            .map(str::to_owned)
            .and_then(|p| ui.image(&p))
        else {
            continue;
        };
        let over = b
            .str("bitmapNameInFocus")
            .map(str::to_owned)
            .and_then(|p| ui.image(&p))
            .map_or_else(|| up.clone(), |(h, _)| h);
        let r = URect::new(
            o.x + ui.sc(b.f32("bitmapPositionX").unwrap_or(0.0)),
            o.y + ui.sc(b.f32("bitmapPositionY").unwrap_or(0.0)),
            ui.sc(sz.x as f32),
            ui.sc(sz.y as f32),
        );
        // The open tab shows its down bitmap (the tab is in the signature).
        let img = if win.tab == tab {
            b.str("bitmapNameDown")
                .map(str::to_owned)
                .and_then(|p| ui.image(&p))
                .map_or_else(|| up.clone(), |(h, _)| h)
        } else {
            up.clone()
        };
        let e = ui.quad(img, r, None, Color::WHITE);
        ui.commands.entity(e).insert(Look::Tab { tab, up, over });
        ui.record_button_sound(r, &b);
        layout.hits.push((r, Hit::Tab(tab)));
    }
    let check = consts
        .str("optionsCheckboxBitmap")
        .unwrap_or("ui/mainmenu/buttoncheckbox*.tex")
        .to_string();
    // The code-built check boxes' sounds (`MenuCheckBox::MenuCheckBox`: the constants' gem button sounds).
    let check_over = consts.str("gemButtonRolloverSound").map(str::to_owned);
    let check_down = consts.str("gemButtonClickSound").map(str::to_owned);
    let label_style = consts
        .str("optionsFontName")
        .unwrap_or(LABEL_STYLE)
        .to_string();
    let value_style = consts
        .str("dropboxListFont")
        .unwrap_or(VALUE_STYLE)
        .to_string();
    let drop_border = consts
        .str("dropboxBorder")
        .unwrap_or("ui/mainmenu/borderthin_*.tex")
        .to_string();
    let drop_back = color4(consts, "dropboxBackColor");
    let drop_arrow = consts
        .str("dropboxButtonUpBitmap")
        .unwrap_or("ui/generic/buttoncarrowlg_downup.tex")
        .to_string();
    let label = |ui: &mut Ui, tag: &str, x: f32, cy: f32| {
        if tag.is_empty() {
            return;
        }
        let text = ui.fonts.tag(tag).trim().to_string();
        let lh = ui.line_height(&label_style);
        ui.text_line(
            &label_style,
            Vec2::new(x, (cy - lh * 0.5).round()),
            &text,
            1.0,
        );
    };
    let mut open_list = None;
    for (ci, c) in page(win.tab).into_iter().enumerate() {
        match c {
            Ctl::Check(k, tag, x, y) => {
                let enabled = check_enabled(k, &draft);
                let on = enabled && draft.flag(k);
                // `Grim Dawn.exe` `MenuCheckBox::Render` (the check box's draw; its bitmaps are the pattern with
                // `Up` / `Down` / `Over` / `Disabled`, `MenuCheckBox::LoadBitmaps`, and `DownOver`,
                // `MenuCheckBox::MenuCheckBox`): a disabled box (`MenuCheckBox.enabled` clear), on or off, draws the
                // `Disabled` bitmap (else `Up` tinted 0.6 grey when that one is
                // missing). All five states are resolved once; [`live`] swaps them.
                let p = at(&ui, x, y);
                if let Some((up_h, up_sz)) = ui.image(&check.replace('*', "up")) {
                    let grey = (up_h.clone(), Color::srgb(0.6, 0.6, 0.6));
                    let states = CHECK_STATES.map(|s| {
                        ui.image(&check.replace('*', s))
                            .map(|(h, _)| (h, Color::WHITE))
                            .unwrap_or_else(|| grey.clone())
                    });
                    let r = URect::new(p.x, p.y, ui.sc(up_sz.x as f32), ui.sc(up_sz.y as f32));
                    let (img, tint) = states[check_state(on, enabled, false)].clone();
                    let e = ui.quad(img, r, None, tint);
                    ui.commands.entity(e).insert(Look::Check { k, states });
                    // A disabled box takes no mouse, so shows no rollover either (`MenuButton::HandleMouseEvent`).
                    if enabled {
                        let r = URect::new(r.x, r.y, ui.sc(24.0), r.h);
                        layout.hits.push((r, Hit::Check(k)));
                        ui.button_sound(r, check_over.as_deref(), check_down.as_deref());
                        layout.tips.push((r, rollover_tag(tag)));
                    }
                }
                let (lx, ly) = (p.x + ui.sc(34.0), p.y + ui.sc(12.0));
                label(&mut ui, tag, lx, ly);
            }
            Ctl::Slider(k, tag, lx, cy, x0, x1) => {
                let l = at(&ui, lx, cy);
                label(&mut ui, tag, l.x, l.y);
                let (a, b) = (at(&ui, x0, cy), at(&ui, x1, cy));
                let track_y = (a.y - ui.sc(6.0)).round();
                let left = ui.image("ui/mainmenu/sliderleft.tex");
                let right = ui.image("ui/mainmenu/sliderright.tex");
                let centre = ui.image("ui/mainmenu/slidercenter.tex");
                if let (Some((li, ls)), Some((ri, rs)), Some((ci, cs))) = (left, right, centre) {
                    let (lw, rw, th) = (ui.sc(ls.x as f32), ui.sc(rs.x as f32), ui.sc(cs.y as f32));
                    ui.quad_stretch(li, URect::new(a.x, track_y, lw, th), Color::WHITE);
                    ui.quad_stretch(
                        ci,
                        URect::new(a.x + lw, track_y, (b.x - a.x - lw - rw).max(0.0), th),
                        Color::WHITE,
                    );
                    ui.quad_stretch(ri, URect::new(b.x - rw, track_y, rw, th), Color::WHITE);
                }
                let v = slider_get(&draft, k);
                let kx = a.x + (b.x - a.x) * v;
                // The knob's look ([`knob_bitmap`]): [`live`] swaps it and moves the knob.
                if let Some((up_h, sz)) = ui.image(KNOB[0]) {
                    let (kw, kh) = (ui.sc(sz.x as f32), ui.sc(sz.y as f32));
                    let held = ui.image(KNOB[1]).map_or_else(|| up_h.clone(), |(h, _)| h);
                    let over = ui.image(KNOB[2]).map_or_else(|| up_h.clone(), |(h, _)| h);
                    let bitmaps = [up_h, held, over];
                    let r = URect::new(
                        (kx - kw * 0.5).round(),
                        (track_y + ui.sc(4.0) - kh * 0.5).round(),
                        kw,
                        kh,
                    );
                    let e = ui.quad(
                        bitmaps[knob_bitmap(win.drag == Some(k), false)].clone(),
                        r,
                        None,
                        Color::WHITE,
                    );
                    ui.commands.entity(e).insert(Look::Knob {
                        k,
                        bitmaps,
                        x0: a.x,
                        x1: b.x,
                        w: kw,
                        inv: ui.inv,
                    });
                    // Before the slider's rect: the first hit under the cursor wins. Stored at value 0 ([`hit_at`]).
                    let at0 = URect::new(a.x - kw * 0.5, r.y, kw, kh);
                    layout.hits.push((at0, Hit::Knob(k, b.x - a.x)));
                }
                let r = URect::new(
                    a.x - ui.sc(6.0),
                    track_y - ui.sc(8.0),
                    b.x - a.x + ui.sc(12.0),
                    ui.sc(24.0),
                );
                layout.hits.push((r, Hit::Slider(k, a.x, b.x)));
                layout.tips.push((r, rollover_tag(tag)));
            }
            Ctl::Drop(shown, tag, lx, cy, x0, x1) => {
                let l = at(&ui, lx, cy);
                label(&mut ui, tag, l.x, l.y);
                let (a, b) = (at(&ui, x0, cy), at(&ui, x1, cy));
                let h = ui.sc(20.0);
                let r = URect::new(a.x, (a.y - h * 0.5).round(), b.x - a.x, h);
                thin_box(&mut ui, &drop_border, r, drop_back);
                let text = shown_text(shown, &draft, &ctx);
                // PORT: the value is ellipsised to the box's inner width (`UITextBox::TruncateLine`);
                // [`live`] re-truncates it on a pick. The full string drives the pick matching.
                let max_w = r.w - 2.0 * ui.sc(4.0);
                let text = ui.truncate_line(&value_style, max_w, &text);
                let lh = ui.line_height(&value_style);
                let e = ui.live_text(
                    TextAt::Line {
                        style: value_style.clone(),
                        at: Vec2::new(r.x + ui.sc(4.0), (r.y + (h - lh) * 0.5).round()),
                        centre: false,
                        tint: Color::WHITE,
                    },
                    &text,
                );
                ui.commands.entity(e).insert(DropText {
                    shown,
                    style: value_style.clone(),
                    max_w,
                });
                let mut hit_r = r;
                let (mut arrow_bottom, mut arrow_w) = (r.y + r.h, 0.0);
                if let Some((img, sz)) = ui.image(&drop_arrow) {
                    let (aw, ah) = (ui.sc(sz.x as f32), ui.sc(sz.y as f32));
                    let ar = URect::new(b.x + ui.sc(3.0), (a.y - ah * 0.5).round(), aw, ah);
                    ui.quad(img, ar, None, Color::WHITE);
                    hit_r.w = ar.x + ar.w - r.x;
                    (arrow_bottom, arrow_w) = (ar.y + ar.h, aw);
                }
                layout.hits.push((hit_r, Hit::Drop(ci)));
                layout.tips.push((hit_r, rollover_tag(tag)));
                if win.dropdown == Some(ci) {
                    open_list = Some((r, shown, arrow_bottom, arrow_w));
                }
            }
        }
    }
    for (tag, x, cy) in loose_labels(win.tab) {
        let p = at(&ui, *x, *cy);
        label(&mut ui, tag, p.x, p.y);
    }
    if win.tab == Tab::KeyBindings {
        let (tl, br) = (at(&ui, 545.0, 362.0), at(&ui, 1372.0, 717.0));
        key_list(
            &mut ui,
            consts,
            &draft,
            (&keys_draft, win.rebind),
            win.scroll,
            tl,
            br,
            &mut layout,
        );
        if let Some((img, sz)) = ui.image("ui/mainmenu/buttonsmallcontoller01_up.tex") {
            let p = at(&ui, 546.0, 726.0);
            ui.quad(
                img,
                URect::new(p.x, p.y, ui.sc(sz.x as f32), ui.sc(sz.y as f32)),
                None,
                Color::WHITE,
            );
        }
    }
    // Default / Apply / Close.
    let (cx, bottom) = (o.x + wr.w * 0.5, o.y + wr.h);
    let changed = !draft.same_values(&options) || keys_draft != *bindings;
    // Apply's sounds under a gate [`live`] shows while Apply is enabled.
    let gate = ui
        .commands
        .spawn((
            Node::default(),
            GlobalZIndex(gd_ui::window::Z_OPTIONS),
            ApplyGate,
            if changed {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
        ))
        .id();
    ui.commands.entity(root).add_child(gate);
    for (k, hit, enabled) in [
        ("defaultsButton", Hit::Default, true),
        ("applyButton", Hit::Apply, changed),
        ("closeButton", Hit::Close, true),
    ] {
        let Some(b) = ui.record(&field(k)) else {
            continue;
        };
        if let Some(r) = bottom_button(&mut ui, &b, cx, bottom, hit, enabled) {
            if hit == Hit::Apply {
                let save = ui.root;
                ui.root = gate;
                ui.record_button_sound(r, &b);
                ui.root = save;
            } else {
                ui.record_button_sound(r, &b);
            }
            layout.hits.push((r, hit));
        }
    }
    // The open dropdown's list (`Grim Dawn.exe`: the options window's dropdowns are made by
    // `MenuDropBox::MenuDropBox`,
    // drawn by `MenuDropBox::Draw` and their rows by `MenuDropBox::DrawRow`): rows `itemHeight` 19 px tall from
    // `borderTop` 2 px under the box, at most 16 shown; a black (0, 0, 0, 1) back under them; the `dropboxBorder` frame
    // drawn round the box and the list together; the highlighted row (the current choice when the list opens, then the
    // row under the cursor, `MenuDropBox::SetSelection` / `MenuDropBox::SetHighlight`) inset `highlightBoxReduction`
    // 1 px, filled `dropboxSelectColor` with a `dropboxSelectBorderColor` line along its top and bottom; the row text
    // in `dropboxListFont`, `borderSide` 4 px in, centred in the row. Over 16 rows (`MenuDropBox::Draw` / input
    // `MenuDropBox::HandleMouseEvent`): rows `drop_top..` shown, a wheel step scrolls a row, a press in the list's
    // right 20 px sets the first row ([`drop_bar_row`]); the thumb (grey 0.5) is the arrow button's width less 12 px
    // from 5 px into it, at `top + 4 + first * (height - 11) / rows`, `(height - 11) * 16 / rows` tall. PORT: the arrow
    // button is drawn outside the box here, so the thumb takes its width at the list's right edge; all rows are drawn
    // in a clipped pane ([`gd_ui::window::pane`]) at scroll 0 and [`live`] scrolls it. The drawn row text is
    // ellipsised to the inner width, less the scroll column when it shows (`UITextBox::TruncateLine`); the layout keeps
    // the full labels (`drop_rows`) for the rows' rollovers ([`rollover`]). The open list spawns
    // last — after the key list and the Default / Apply / Close buttons — so it draws over the window chrome
    // (`MenuDropBox::Draw` draws the open list over the window's buttons): the window's nodes all share one Z and
    // Bevy draws equal-Z siblings in insertion (`Children`) order, later sibling on top, so spawn order is draw
    // order. Its row / bar hits are prepended last to match, taking the mouse before the chrome's hits.
    if let Some((r, shown, arrow_bottom, arrow_w)) = open_list {
        let rows = choices(shown, &draft, &ctx);
        let current = shown_text(shown, &draft, &ctx);
        let row_h = ui.sc(19.0);
        let top = r.y + r.h + ui.sc(2.0);
        let frame_h = ui
            .image(&drop_border.replace('*', "lt"))
            .map_or(0.0, |(_, s)| ui.sc(s.y as f32));
        let max = rows.len().saturating_sub(DROP_ROWS);
        let first = win.drop_top.min(max);
        let shown_rows = rows.len().min(DROP_ROWS);
        let bottom = top + row_h * shown_rows as f32 + frame_h;
        // The back starts under the arrow button (`MenuDropBox::Draw`: its rect's bottom), over the box's lower
        // edge.
        ui.solid(
            URect::new(r.x, arrow_bottom, r.w, bottom - arrow_bottom),
            Color::srgba(0.0, 0.0, 0.0, 1.0),
        );
        thin_box(
            &mut ui,
            &drop_border,
            URect::new(r.x, r.y, r.w, bottom - r.y),
            Color::NONE,
        );
        let select = color4(consts, "dropboxSelectColor");
        let select_border = color4(consts, "dropboxSelectBorderColor");
        let lh = ui.line_height(&value_style);
        let text_max_w = r.w
            - 2.0 * ui.sc(4.0)
            - if rows.len() > DROP_ROWS {
                ui.sc(20.0)
            } else {
                0.0
            };
        let mut hits = Vec::new();
        let (s, h, n) = (ui.s, bottom - top, rows.len() as f32);
        let track = h - 11.0 * s;
        if rows.len() > DROP_ROWS {
            hits.push((
                URect::new(r.x + r.w - 20.0 * s, top, 20.0 * s, h),
                Hit::DropBar(top, h, rows.len()),
            ));
        }
        let clip = URect::new(r.x, top, r.w, row_h * shown_rows as f32);
        let inner = gd_ui::window::pane(&mut ui, clip, first as f32 * row_h);
        ui.commands.entity(inner).insert(Scroll::DropPane {
            clip_y: top,
            row_h,
            inv: ui.inv,
            max,
        });
        let root = ui.root;
        ui.root = inner;
        let highlighted = rows.iter().position(|(t, _)| *t == current);
        for (k, (text, _)) in rows.iter().enumerate() {
            let rr = URect::new(r.x, top + row_h * k as f32, r.w, row_h);
            let inset = ui.sc(1.0);
            let hr = URect::new(rr.x + inset, rr.y, rr.w - 2.0 * inset, rr.h);
            let hl = ui.solid(hr, select);
            ui.commands.entity(hl).insert((
                DropRowHl(k),
                if highlighted == Some(k) {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
            ));
            let hl_root = ui.root;
            ui.root = hl;
            ui.solid(URect::new(0.0, 0.0, hr.w, 1.0), select_border);
            ui.solid(URect::new(0.0, hr.h, hr.w, 1.0), select_border);
            ui.root = hl_root;
            let drawn = ui.truncate_line(&value_style, text_max_w, text);
            ui.text_line(
                &value_style,
                Vec2::new(rr.x + ui.sc(4.0), (rr.y + (row_h - lh) * 0.5).round()),
                &drawn,
                1.0,
            );
            hits.push((rr, Hit::DropRow(k)));
        }
        ui.root = root;
        if rows.len() > DROP_ROWS {
            let base_y = top + 4.0 * s;
            let step = track / n;
            let e = ui.solid(
                URect::new(
                    r.x + r.w - arrow_w + 5.0 * s,
                    base_y + first as f32 * step,
                    (arrow_w - 12.0 * s).max(1.0),
                    track * DROP_ROWS as f32 / n,
                ),
                Color::srgba(0.5, 0.5, 0.5, 1.0),
            );
            ui.commands.entity(e).insert(Scroll::DropThumb {
                base_y,
                step,
                inv: ui.inv,
                max,
            });
        }
        layout.drop_row_h = row_h;
        layout.drop_clip = clip;
        layout.drop_rows = rows.iter().map(|(t, _)| t.clone()).collect();
        hits.append(&mut layout.hits);
        layout.hits = hits;
    }
}

/// The key bindings list: header row, all rows in a clipped pane at scroll 0
/// ([`live`] scrolls it), scroll bar.
#[allow(clippy::too_many_arguments)]
pub(super) fn key_list(
    ui: &mut Ui,
    consts: &Record,
    o: &GameOptions,
    (b, rebind): (&KeyBindings, Option<(u32, usize)>),
    scroll: usize,
    tl: Vec2,
    br: Vec2,
    layout: &mut OptionsLayout,
) {
    let r = URect::new(tl.x, tl.y, br.x - tl.x, br.y - tl.y);
    let border = consts
        .str("menuListOuterBorder")
        .unwrap_or("ui/mainmenu/borderthick_*.tex")
        .to_string();
    thin_box(ui, &border, r, color4(consts, "menuListDefaultColColor"));
    layout.hits.push((r, Hit::List));
    let header_style = consts
        .str("optionsKeyBindingHeaderFontName")
        .unwrap_or(LABEL_STYLE)
        .to_string();
    let row_style = consts
        .str("defaultControlFontName")
        .unwrap_or(VALUE_STYLE)
        .to_string();
    // Measured: header bar 4..26, rows from 30, 20 px each.
    let (pad, head_y, head_h) = (ui.sc(6.0), ui.sc(4.0), ui.sc(22.0));
    ui.solid(
        URect::new(r.x + ui.sc(4.0), r.y + head_y, r.w - ui.sc(8.0), head_h),
        color4(consts, "optionsKeyBindingHeaderColor"),
    );
    let cols = [0.0, ui.sc(370.0), ui.sc(540.0)];
    let hl = ui.line_height(&header_style);
    for (i, tag) in [
        "tagKeybindingTitle02",
        "tagKeybindingTitle03",
        "tagKeybindingTitle04",
    ]
    .iter()
    .enumerate()
    {
        let t = ui.fonts.tag(tag).to_string();
        ui.text_line(
            &header_style,
            Vec2::new(
                r.x + pad + cols[i],
                (r.y + head_y + (head_h - hl) * 0.5).round(),
            ),
            &t,
            1.0,
        );
    }
    let rows = binding_rows(ui.fonts, o, b);
    let row_h = ui.sc(20.0);
    let top = r.y + ui.sc(30.0);
    let visible = (((r.y + r.h) - top) / row_h).floor().max(1.0) as usize;
    layout.max_scroll = rows.len().saturating_sub(visible);
    layout.key_row_h = row_h;
    layout.key_clip = URect::new(r.x, top, r.w, (r.y + r.h) - top);
    let scroll = scroll.min(layout.max_scroll);
    let rl = ui.line_height(&row_style);
    // The selected row: the one being bound, else the first (static — rebind
    // is in the signature).
    let selected = match rebind {
        Some((a, _)) => rows.iter().position(|(ra, ..)| *ra == a),
        None => Some(0),
    };
    let inner = gd_ui::window::pane(ui, layout.key_clip, scroll as f32 * row_h);
    ui.commands.entity(inner).insert(Scroll::KeyPane {
        clip_y: top,
        row_h,
        inv: ui.inv,
    });
    let root = ui.root;
    ui.root = inner;
    let mut cells = Vec::new();
    for (k, (a, f, p, s)) in rows.iter().enumerate() {
        let y = top + k as f32 * row_h;
        let waiting = rebind.filter(|r| r.0 == *a);
        if selected == Some(k) {
            // The selected row (the first, or the one being bound).
            ui.solid(
                URect::new(r.x + ui.sc(4.0), y, r.w - ui.sc(24.0), row_h),
                color4(consts, "menuListSelectColor"),
            );
        }
        for (i, t) in [f, p, s].into_iter().enumerate() {
            // PORT: a cell waiting for its key shows "...".
            let t = if i > 0 && waiting.is_some_and(|w| w.1 == i - 1) {
                "..."
            } else {
                t.as_str()
            };
            ui.text_line(
                &row_style,
                Vec2::new(r.x + pad + cols[i], (y + (row_h - rl) * 0.5).round()),
                t,
                1.0,
            );
        }
        if !f.is_empty() {
            for slot in 0..2 {
                let x0 = r.x + pad + cols[slot + 1];
                let w = if slot == 0 {
                    cols[2] - cols[1]
                } else {
                    r.w - pad - cols[2] - ui.sc(24.0)
                };
                cells.push((URect::new(x0, y, w, row_h), Hit::Key(*a, slot)));
            }
        }
    }
    ui.root = root;
    // The cells before the list's own hit.
    cells.append(&mut layout.hits);
    layout.hits = cells;
    // Scroll bar: the slider's place in the list ([`live`] moves it); its thumb takes a press before the cells and
    // the list ([`Hit::KeyThumb`]).
    if let Some((img, sz)) = ui.image(
        consts
            .str("scrollbarSliderBitmap")
            .unwrap_or("ui/mainmenu/scrollbar01.tex"),
    ) {
        let track = (r.y + ui.sc(30.0), r.h - ui.sc(32.0));
        let (bw, bh) = (ui.sc(sz.x as f32), ui.sc(sz.y as f32));
        let span = rows.len().saturating_sub(visible).max(1) as f32;
        let y = track.0 + (track.1 - bh) * (scroll as f32 / span);
        let x = r.x + r.w - pad - bw;
        let e = ui.quad(img, URect::new(x, y.round(), bw, bh), None, Color::WHITE);
        layout
            .hits
            .insert(0, (URect::new(x, track.0, bw, bh), Hit::KeyThumb));
        layout.key_bar = (track.0, track.1 - bh);
        ui.commands.entity(e).insert(Scroll::KeyThumb {
            track0: track.0,
            travel: track.1 - bh,
            span,
            inv: ui.inv,
        });
    }
}
