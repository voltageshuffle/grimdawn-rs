use super::*;

#[derive(Component)]
pub(super) struct TrackerButtonRoot;

/// The toggle button at the fade alpha (`bitmapNameInFocus` under the mouse). Built per (size, shown);
/// a drag moves the root node and the button's sound rect in place, and the hover bitmap and the fade alpha
/// change the image node in place, not by a rebuild.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn tracker_button(
    mut fresh: gd_game_core::session::NewSession,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    fade: Res<TrackerFade>,
    roots: Query<Entity, With<TrackerButtonRoot>>,
    mut root_nodes: Query<&mut Node, With<TrackerButtonRoot>>,
    mut sounds: Query<(&mut gd_ui_kit::widgets::ButtonSound, &ChildOf)>,
    mut built: Local<crate::window::Rebuild<(Option<Vec2>, bool)>>,
    // Where the built button sits (physical px): a drag offsets the root from it.
    mut built_at: Local<Vec2>,
    // The built image node and its up / in-focus bitmaps.
    mut node: Local<Option<(Entity, Handle<Image>, Option<Handle<Image>>)>>,
    mut images: Query<&mut ImageNode>,
    mut assets: gd_ui_kit::UiAssets,
) {
    if fresh.begun() {
        built.reset();
        *node = None;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let sig = (fade.button.map(|r| Vec2::new(r.w, r.h)), fade.alpha > 0.0);
    if size.x != 0 && built.changed(sig) {
        crate::window::despawn_all(&mut commands, &roots);
        *node = None;
        let b = data
            .db
            .get(TRACKER)
            .and_then(|t| t.str("toggleButton").map(str::to_owned))
            .and_then(|b| data.db.get(&b));
        if let (Some(r), Some(b)) = (fade.button.filter(|_| fade.alpha > 0.0), b) {
            *built_at = Vec2::new(r.x, r.y);
            let root = gd_ui_kit::spawn_hud_overlay(&mut commands, TrackerButtonRoot);
            let mut ui = assets.ui(
                &mut commands,
                &data,
                root,
                assets.scale(size),
                1.0 / win.scale_factor(),
                Vec2::new(size.x as f32, size.y as f32),
            );
            let mut img = |key: &str| b.str(key).map(str::to_owned).and_then(|p| ui.image(&p));
            let up = img("bitmapNameUp");
            let over = img("bitmapNameInFocus").map(|(h, _)| h);
            if let Some((up, _)) = up {
                let shown = if fade.hover { over.clone() } else { None };
                let e = ui.quad(
                    shown.unwrap_or_else(|| up.clone()),
                    r,
                    None,
                    Color::srgba(1.0, 1.0, 1.0, fade.alpha),
                );
                *node = Some((e, up, over));
            }
            ui.record_button_sound(r, b);
        }
        return;
    }
    // In place: a drag moves the root and the sound rect with the button.
    if let Some(r) = fade.button.filter(|_| fade.alpha > 0.0) {
        let off = (Vec2::new(r.x, r.y) - *built_at) / win.scale_factor();
        let (left, top) = (Val::Px(off.x), Val::Px(off.y));
        for mut n in &mut root_nodes {
            if n.left != left || n.top != top {
                n.left = left;
                n.top = top;
            }
        }
        for (mut s, parent) in &mut sounds {
            if roots.contains(parent.parent()) && s.rect != r {
                s.rect = r;
            }
        }
    }
    // In place: the hover bitmap and the fade alpha.
    let Some((e, up, over)) = node.as_ref() else {
        return;
    };
    let Ok(mut img) = images.get_mut(*e) else {
        return;
    };
    let want = over.as_ref().filter(|_| fade.hover).unwrap_or(up);
    if img.image != *want {
        img.image = want.clone();
    }
    if img.color.alpha() != fade.alpha {
        img.color.set_alpha(fade.alpha);
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn build_tracker(
    mut fresh: gd_game_core::session::NewSession,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    view: Res<QuestView>,
    journal: Res<QuestJournal>,
    panels: Res<crate::UiPanels>,
    survival: Res<crate::state::survival::SurvivalView>,
    roots: Query<Entity, With<TrackerRoot>>,
    mut root_nodes: Query<&mut Node, With<TrackerRoot>>,
    (options, mut fade): (Res<gd_game_core::options::GameOptions>, ResMut<TrackerFade>),
    mut built: Local<Option<(UVec2, Vec<u32>, bool, u64)>>,
    // Where the built tracker sits and its title / button rects there (physical px): a drag (`fade.pos`)
    // moves the root and the rects in place.
    mut placed: Local<Option<(Vec2, Option<URect>, Option<URect>)>>,
    mut assets: gd_ui_kit::UiAssets,
) {
    if fresh.begun() {
        *built = Default::default();
        *placed = None;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    // Hidden while the devotion window is open (as in the real game;
    // PORT: the engine's rule is not decompiled).
    // The Crucible hides the tracker for the whole session (`UIPlayerHud::Load`: mode 1 clears
    // its visible byte; its objective window shows instead, `survival.rs`).
    // The tracked quests, borrowed: only a rebuild clones them. The ids say which are shown; the view's
    // change detection covers their content (`collect` writes it only when its hash changed).
    let shown: Vec<&QuestEntry> = if panels.devotion || survival.objective_window {
        Vec::new()
    } else {
        tracked(&view)
            .into_iter()
            .filter(|q| !journal.untracked.contains(&q.id))
            .collect()
    };
    let open = tracker_open(&options);
    let pos = fade.pos;
    let generation = assets.fonts.generation;
    let same = built.as_ref().is_some_and(|(s, ids, o, g)| {
        *s == size
            && *o == open
            && !view.is_changed()
            && ids.len() == shown.len()
            && ids.iter().zip(&shown).all(|(id, q)| *id == q.id)
            && *g == generation
    });
    if size.x == 0 {
        return;
    }
    if same {
        // In place: the drag's offset from where the tracker was built.
        if let Some((at, title, button)) = *placed {
            let off = pos.map_or(Vec2::ZERO, |p| p - at);
            let shift =
                |r: Option<URect>| r.map(|r| URect::new(r.x + off.x, r.y + off.y, r.w, r.h));
            let (title, button) = (shift(title), shift(button));
            if fade.title != title || fade.button != button {
                (fade.title, fade.button) = (title, button);
            }
            let (left, top) = (
                Val::Px(off.x / win.scale_factor()),
                Val::Px(off.y / win.scale_factor()),
            );
            for mut n in &mut root_nodes {
                if n.left != left || n.top != top {
                    n.left = left;
                    n.top = top;
                }
            }
        }
        return;
    }
    let quests: Vec<QuestEntry> = shown.into_iter().cloned().collect();
    *built = Some((
        size,
        quests.iter().map(|q| q.id).collect(),
        open,
        generation,
    ));
    for e in &roots {
        commands.entity(e).try_despawn();
    }
    (fade.title, fade.button) = (None, None);
    *placed = None;
    let Some(rec) = data.db.get(TRACKER) else {
        return;
    };
    if quests.is_empty() {
        return;
    }
    let root = gd_ui_kit::spawn_hud_overlay(&mut commands, TrackerRoot);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        assets.scale(size),
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let field = |k: &str| rec.str(k).unwrap_or("").to_string();
    let mut wr = WindowDef::from_record(rec).screen_rect(ui.screen.x, ui.screen.y, ui.s);
    if let Some(p) = pos {
        (wr.x, wr.y) = (p.x, p.y);
    }
    let (title_style, task_style, obj_style) = (
        field("titleFontStyle"),
        field("taskFontStyle"),
        field("objectiveFontStyle"),
    );
    let (title_lh, task_lh, obj_lh) = (
        ui.line_height(&title_style),
        ui.line_height(&task_style),
        ui.line_height(&obj_style),
    );
    let icon = ui
        .record(&field("mainQuestIcon"))
        .and_then(|r| r.str("bitmapName").map(str::to_owned))
        .and_then(|p| ui.image(&p));
    let check = ui.record(&field("objectiveBitmap"));
    let check_y = check
        .as_ref()
        .and_then(|r| r.f32("bitmapPositionY"))
        .unwrap_or(0.0);
    let checks: Vec<_> = check
        .as_ref()
        .map(|r| {
            r.strs("bitmaps")
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into_iter()
        .map(|p| ui.image(&p))
        .collect();
    let check_w = checks
        .iter()
        .flatten()
        .map(|(_, s)| ui.sc(s.x as f32))
        .next()
        .unwrap_or(0.0);
    let icon_w = icon.as_ref().map_or(0.0, |(_, s)| ui.sc(s.x as f32));
    // The toggle button and the title line (`tracker_input`).
    fade.title = Some(URect::new(wr.x, wr.y, wr.w, title_lh));
    if let Some((_, bs)) = ui
        .record(&field("toggleButton"))
        .and_then(|b| b.str("bitmapNameUp").map(str::to_owned))
        .and_then(|p| ui.image(&p))
    {
        let (bw, bh) = (ui.sc(bs.x as f32), ui.sc(bs.y as f32));
        fade.button = Some(URect::new(
            wr.x + wr.w - bw - 1.0,
            wr.y + ((title_lh - bh) * 0.5).round(),
            bw,
            bh,
        ));
    }
    *placed = Some((Vec2::new(wr.x, wr.y), fade.title, fade.button));
    if !open {
        return;
    }
    // Lay out first (the height goes into the background).
    struct Line {
        text: String,
        x: f32,
        style: usize,
        mark: Option<usize>,
    }
    let mut lines: Vec<(f32, Line)> = Vec::new();
    let mut height = title_lh;
    let mut y = title_lh + 3.0;
    for q in &quests {
        let name = ui.truncate_line(&task_style, wr.w - icon_w, &q.name);
        let mark = q.main.then_some(usize::MAX);
        lines.push((
            if q.main { y - 1.0 } else { y },
            Line {
                text: name,
                x: if q.main { icon_w + 4.0 } else { 0.0 },
                style: 1,
                mark,
            },
        ));
        y += task_lh;
        height += task_lh;
        for t in q.tasks.iter().filter(|t| t.state == STATE_IN_PROGRESS) {
            for (o, done) in &t.objectives {
                let ol = ui.wrap_lines(&obj_style, wr.w - check_w, o);
                for (i, l) in ol.iter().enumerate() {
                    lines.push((
                        y,
                        Line {
                            text: l.clone(),
                            x: check_w,
                            style: 2,
                            mark: (i == 0).then_some(*done as usize),
                        },
                    ));
                    y += obj_lh;
                }
                height += ol.len() as f32 * obj_lh;
            }
        }
    }
    let r = URect::new(wr.x, wr.y, wr.w, height.round());
    fade.size = Vec2::new(r.w, r.h);
    let c = |k: &str| rec.f32(k).unwrap_or(0.0);
    ui.solid(
        r,
        Color::srgba(
            c("backgroundColor.R"),
            c("backgroundColor.G"),
            c("backgroundColor.B"),
            c("backgroundColor.A"),
        ),
    );
    ui.border(rec, "", r, 1.0);
    let title = ui.fonts.tag(&field("titleTag")).to_string();
    ui.text_line(&title_style, Vec2::new(r.x, r.y), &title, 1.0);
    for (ly, l) in lines {
        let p = Vec2::new(r.x, r.y + ly);
        match l.mark {
            Some(usize::MAX) => {
                if let Some((img, s)) = icon.clone() {
                    let (w, h) = (ui.sc(s.x as f32), ui.sc(s.y as f32));
                    ui.quad(img, URect::new(p.x, p.y, w, h), None, Color::WHITE);
                }
            }
            Some(frame) => {
                if let Some(Some((img, s))) = checks.get(frame).cloned() {
                    let (w, h) = (ui.sc(s.x as f32), ui.sc(s.y as f32));
                    ui.quad(
                        img,
                        URect::new(p.x, p.y + ui.sc(check_y), w, h),
                        None,
                        Color::WHITE,
                    );
                }
            }
            None => {}
        }
        let style = if l.style == 1 {
            &task_style
        } else {
            &obj_style
        };
        ui.text_line(style, Vec2::new(p.x + l.x, p.y), &l.text, 1.0);
    }
}
