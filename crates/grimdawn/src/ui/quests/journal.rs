use super::*;

/// A journal pane's scrolled inner node ([`crate::ui::window::pane`]): [`journal_live`] moves it by
/// `QuestJournal::scroll[pane]`, so scrolling does not rebuild the window.
#[derive(Component)]
pub(super) struct JournalScrolled {
    pub(super) pane: usize,
    pub(super) clip_y: f32,
    pub(super) inv: f32,
}

/// A tree row's hover shade: [`journal_live`] shows the one whose key is `QuestJournal::hover`, so
/// hovering does not rebuild the window.
#[derive(Component)]
pub(super) struct RowHover(pub(super) String);

/// Scrolls the journal's panes and scroll bar thumbs and shows the hovered tree row in place (the
/// pattern of `character::scroll_sheet`): `QuestJournal::scroll` / `hover` are not in `build_journal`'s
/// signature, so a scroll or hover change moves these nodes instead of rebuilding the window.
pub(super) fn journal_live(
    journal: Res<QuestJournal>,
    mut panes: Query<(&JournalScrolled, &mut Node), Without<ScrollThumb>>,
    mut thumbs: Query<(&ScrollThumb, &mut Node), Without<JournalScrolled>>,
    mut hovers: Query<(&RowHover, &mut Visibility)>,
) {
    for (s, mut node) in &mut panes {
        let top = Val::Px((-s.clip_y - journal.scroll[s.pane]) * s.inv);
        if node.top != top {
            node.top = top;
        }
    }
    for (t, mut node) in &mut thumbs {
        if t.owner != ScrollOwner::Journal {
            continue;
        }
        let off = t.travel * (journal.scroll[t.pane] / t.max).clamp(0.0, 1.0);
        let top = Val::Px(((t.base_y + off).round() + t.round_add) * t.inv);
        if node.top != top {
            node.top = top;
        }
    }
    for (h, mut vis) in &mut hovers {
        vis.set_if_neq(if journal.hover.as_deref() == Some(h.0.as_str()) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// The reward items' places (sub_1401F0D70 / sub_1401F09D0, a
/// binary-tree packer): sizes plus `spacing` go into rows of `width` from
/// the first item's row down; returns each item's top-left offset (by input
/// index) and the packed height. PORT: sorted tallest first (the sort's
/// comparator, sub_1401F1620, is not read).
pub fn pack_items(sizes: &[Vec2], width: f32, spacing: f32) -> (Vec<Vec2>, f32) {
    #[derive(Clone)]
    struct Node {
        r: [f32; 4],
        kids: Option<(usize, usize)>,
        used: bool,
    }
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by(|a, b| sizes[*b].y.total_cmp(&sizes[*a].y));
    let mut out = vec![Vec2::ZERO; sizes.len()];
    let Some(&first) = order.first() else {
        return (out, 0.0);
    };
    let inf = f32::INFINITY;
    let h0 = sizes[first].y;
    let mut nodes = vec![
        Node {
            r: [0.0, 0.0, width, inf],
            kids: Some((1, 2)),
            used: false,
        },
        Node {
            r: [0.0, 0.0, width, h0 + spacing],
            kids: None,
            used: false,
        },
        Node {
            r: [0.0, h0 + spacing, width, inf],
            kids: None,
            used: false,
        },
    ];
    let mut height = h0;
    fn insert(
        nodes: &mut Vec<Node>,
        mut n: usize,
        w: f32,
        h: f32,
        height: &mut f32,
    ) -> Option<Vec2> {
        loop {
            if let Some((a, b)) = nodes[n].kids {
                if let Some(p) = insert(nodes, a, w, h, height) {
                    return Some(p);
                }
                n = b;
                continue;
            }
            let [x, y, nw, nh] = nodes[n].r;
            if nodes[n].used || nw < w || nh < h {
                return None;
            }
            if nw == w && nh == h {
                nodes[n].used = true;
                return Some(Vec2::new(x, y));
            }
            let k = nodes.len();
            if nh == f32::INFINITY {
                // A new row: (row, below), the row split into (item, rest).
                *height = y + h;
                nodes.push(Node {
                    r: [x, y, nw, h],
                    kids: Some((k + 2, k + 3)),
                    used: false,
                });
                nodes.push(Node {
                    r: [x, y + h, nw, nh],
                    kids: None,
                    used: false,
                });
                nodes.push(Node {
                    r: [x, y, w, h],
                    kids: None,
                    used: true,
                });
                nodes.push(Node {
                    r: [x + w, y, nw - w, h],
                    kids: None,
                    used: false,
                });
            } else if w <= h {
                nodes.push(Node {
                    r: [x, y, w, nh],
                    kids: None,
                    used: true,
                });
                nodes.push(Node {
                    r: [x + w, y, nw - w, nh],
                    kids: None,
                    used: false,
                });
            } else {
                nodes.push(Node {
                    r: [x, y, nw, h],
                    kids: None,
                    used: true,
                });
                nodes.push(Node {
                    r: [x, y + h, nw, nh - h],
                    kids: None,
                    used: false,
                });
            }
            nodes[n].kids = Some((k, k + 1));
            return Some(Vec2::new(x, y));
        }
    }
    for i in order {
        if let Some(p) = insert(
            &mut nodes,
            0,
            sizes[i].x + spacing,
            sizes[i].y + spacing,
            &mut height,
        ) {
            out[i] = p;
        }
    }
    (out, height)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_journal(
    mut fresh: gd_game::session::NewSession,
    hooks: Res<crate::hooks::Hooks>,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    view: Res<QuestView>,
    mut journal: ResMut<QuestJournal>,
    roots: Query<Entity, With<JournalRoot>>,
    mut layout: ResMut<JournalLayout>,
    (inv, reqs): (
        Res<crate::inventory::Inventory>,
        Query<&gd_game::inventory::Requirements, With<crate::player::Player>>,
    ),
    lore: Option<Res<gd_game::lore::LoreCodex>>,
    (devotion, index, rules): (
        Option<Res<gd_game::devotion::DevotionState>>,
        Option<Res<gd_game::riftgates::WaypointIndex>>,
        Option<Res<crate::combat::CombatRules>>,
    ),
    bindings: Res<crate::ui::keys::KeyBindings>,
    (mut built, mut picked): (
        Local<
            Option<(
                UVec2,
                QuestView,
                QuestJournal,
                usize,
                u64,
                crate::ui::keys::KeyBindings,
            )>,
        >,
        Local<bool>,
    ),
    mut assets: crate::ui::UiAssets,
) {
    if fresh.begun() {
        *built = Default::default();
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    // The tab's quests in the repository's order (no main-first sort).
    let list: Vec<&QuestEntry> = view
        .quests
        .iter()
        .filter(|q| {
            if journal.completed {
                q.completed()
            } else {
                q.in_progress()
            }
        })
        .collect();
    // `GD_QUEST_SELECT`: selected once it is listed.
    if !*picked
        && let Some(name) = hooks.ui.quest_select.clone()
        && let Some(q) = list.iter().find(|q| q.name == name)
    {
        *picked = true;
        journal.selected = Some(q.id);
    }
    // Keep a valid selection (the first quest of the tab).
    let sel = journal
        .selected
        .filter(|id| list.iter().any(|q| q.id == *id))
        .or(list.first().map(|q| q.id));
    if journal.selected != sel {
        journal.selected = sel;
    }
    let notes = lore.as_deref().map_or(&[][..], |l| &l.notes[..]);
    let dsig = devotion.as_ref().map_or(0, |d| {
        d.version
            ^ ((d.restored.len() as u64) << 32)
            ^ ((d.total as u64) << 48)
            ^ index.as_ref().map_or(0, |i| i.0.len() as u64) << 20
    });
    // `scroll` and `hover` are not in the signature: `journal_live` moves the panes, the scroll bar
    // thumbs and the hover shade in place, so a scroll or hover change does not rebuild the window.
    let same_journal = |j: &QuestJournal| {
        j.open == journal.open
            && j.completed == journal.completed
            && j.lore == journal.lore
            && j.note == journal.note
            && j.devotion == journal.devotion
            && j.help == journal.help
            && j.help_page == journal.help_page
            && j.selected == journal.selected
            && j.collapsed == journal.collapsed
            && j.untracked == journal.untracked
    };
    // The Help tab's text names the bound keys: a rebind rebuilds it.
    if size.x == 0
        || built.as_ref().is_some_and(|(s, v, j, n, d, b)| {
            *s == size
                && v == &*view
                && same_journal(j)
                && *n == notes.len()
                && *d == dsig
                && (!journal.help || b == &*bindings)
        })
    {
        return;
    }
    *built = Some((
        size,
        view.clone(),
        journal.clone(),
        notes.len(),
        dsig,
        bindings.clone(),
    ));
    for e in &roots {
        commands.entity(e).try_despawn();
    }
    *layout = JournalLayout::default();
    let Some(rec) = data.db.get(JOURNAL).filter(|_| journal.open) else {
        return;
    };
    let root = crate::ui::spawn_root(&mut commands, JournalRoot, crate::ui::window::Z_WINDOW - 1);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        assets.scale(size),
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    ui.req = reqs.single().copied().unwrap_or_default();
    let field = |r: &gd_formats::arz::Record, k: &str| r.str(k).unwrap_or("").to_string();
    let wr = WindowDef::from_record(rec).screen_rect(ui.screen.x, ui.screen.y, ui.s);
    let o = Vec2::new(wr.x, wr.y);
    let page = |ui: &Ui, k: &str| ui.record(&field(rec, k));
    let (jk, lk) = if journal.help {
        ("helpPage", "helpIndex")
    } else if journal.lore {
        ("codexJournal", "codexLog")
    } else {
        ("questJournal", "questLog")
    };
    let (Some(jr), Some(lr)) = (
        page(
            &ui,
            if journal.devotion {
                "devotionJournal"
            } else {
                jk
            },
        ),
        page(&ui, lk),
    ) else {
        return;
    };
    let num = |r: &gd_formats::arz::Record, k: &str| r.f32(k).unwrap_or(0.0);
    let (jx, jy) = (ui.sc(num(&jr, "positionX")), ui.sc(num(&jr, "positionY")));
    let (lx, ly) = (ui.sc(num(&lr, "positionX")), ui.sc(num(&lr, "positionY")));
    // Page backgrounds (the devotion page is one full-height page).
    for (pr, px, py) in [(&jr, jx, jy), (&lr, lx, ly)]
        .into_iter()
        .take(if journal.devotion { 1 } else { 2 })
    {
        if let Some(b) = ui.record(&field(pr, "background")) {
            let p = b.str("bitmapName").unwrap_or("").to_string();
            ui.bitmap(
                &p,
                num(&b, "bitmapPositionX"),
                num(&b, "bitmapPositionY"),
                o + Vec2::new(px, py),
            );
        }
    }
    // Tabs (help, active, completed, lore, devotion) and close button.
    for (i, k) in [
        "helpButton",
        "questButton",
        "questCompleteButton",
        "codexButton",
        "devotionButton",
    ]
    .into_iter()
    .enumerate()
    {
        if let Some(b) = ui.record(&field(rec, k)) {
            let down = if journal.help {
                i == 0
            } else if journal.devotion {
                i == 4
            } else if journal.lore {
                i == 3
            } else {
                (i == 1 && !journal.completed) || (i == 2 && journal.completed)
            };
            let img = if down {
                "bitmapNameDown"
            } else {
                "bitmapNameUp"
            };
            let r = ui
                .bitmap(
                    &field(&b, img),
                    num(&b, "bitmapPositionX"),
                    num(&b, "bitmapPositionY"),
                    o,
                )
                .map(|(_, r)| r);
            if let Some(r) = r {
                ui.record_button_sound(r, &b);
            }
            layout.tabs[i] = r;
        }
    }
    if let Some(b) = ui.record(&field(rec, "closeButton")) {
        layout.close = ui
            .bitmap(
                &field(&b, "bitmapNameUp"),
                num(&b, "bitmapPositionX"),
                num(&b, "bitmapPositionY"),
                o,
            )
            .map(|(_, r)| r);
        if let Some(r) = layout.close {
            ui.record_button_sound(r, &b);
        }
    }
    let sb_rec = |r: &gd_formats::arz::Record| field(r, "verticalScrollbar");
    if journal.devotion {
        devotion_page(
            &mut ui,
            &journal,
            &mut layout,
            devotion.as_deref(),
            index.as_deref(),
            rules
                .as_ref()
                .map_or(0, |r| r.0.levels.max_devotion_points.max(0) as u32),
            Vec2::new(o.x + jx, o.y + jy),
            &jr,
        );
        layout.window = Some(URect::new(wr.x, wr.y, wr.w, wr.h));
        return;
    }
    if journal.help {
        help_pages(&mut ui, &journal, &mut layout, &bindings, o, (&jr, &lr));
        layout.window = Some(URect::new(wr.x, wr.y, wr.w, wr.h));
        return;
    }
    if journal.lore {
        codex_pages(&mut ui, &journal, &mut layout, notes, o, (&jr, &lr));
        layout.window = Some(URect::new(wr.x, wr.y, wr.w, wr.h));
        return;
    }

    // Upper page (sub_14021FC80): the quest's title (`questTitle`),
    // then on the active tab per task in progress its description
    // (`taskText`), `objectiveSectionSpacing`, the "Objectives:" header
    // (`tagQuestObjectives` in `objectiveTitleStyle`, a row high) and the
    // objectives with text (checked when satisfied or the task complete;
    // header and spacing dropped without objectives); on the completed tab
    // the descriptions of the tasks completed after being in progress,
    // joined by blank lines. Then per task `rewardSectionSpacing`, a row for
    // the "Rewards:" header (`tagQuestRewards`, `rewardTitleStyle`; drawn only
    // with rewards), each reward event's text (`rewardStyle`, a
    // `minorRowHeight` row each) and, after `itemSectionSpacing`, the reward
    // items packed (`pack_items`) from `itemBorderAdjust` in over the page
    // width less twice it, `itemSpacing` apart (sub_14021F5E0 /
    // sub_14021F960), each with its rarity plate. Incentives are not shown
    // (the page asks for them but draws nothing).
    let jpane = URect::new(
        o.x + jx,
        o.y + jy + ui.sc(num(&jr, "topGuardBand")),
        ui.sc(num(&jr, "width")),
        ui.sc(num(&jr, "height") - num(&jr, "topGuardBand")),
    );
    let content_x = o.x + jx + ui.sc(num(&jr, "contentX"));
    let outer = ui.root;
    let inner = pane(&mut ui, jpane, journal.scroll[0]);
    ui.commands.entity(inner).insert(JournalScrolled {
        pane: 0,
        clip_y: jpane.y,
        inv: ui.inv,
    });
    ui.root = inner;
    let mut y = jpane.y;
    let text_rec = ui.record(&field(&jr, "taskText"));
    let desc_style = text_rec
        .as_ref()
        .map(|r| field(r, "fontStyle"))
        .unwrap_or_default();
    let desc_w = ui.sc(text_rec.as_ref().map_or(315.0, |r| num(r, "textBoxWidth")));
    let (obj_style, obj_title_style) = (
        field(&jr, "objectiveStyle"),
        field(&jr, "objectiveTitleStyle"),
    );
    let checks: Vec<_> = ui
        .record(&field(&jr, "completeBitmap"))
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
    let major = ui.sc(num(&jr, "majorRowHeight"));
    let minor = ui.sc(num(&jr, "minorRowHeight"));
    // Description lines step by the style's font size
    // (sub_14021FC80: its height is lines × the style's size).
    let desc_lh = ui
        .fonts
        .style(ui.data, &desc_style)
        .map_or(0.0, |st| st.scaled_size(ui.s) as f32);
    let obj_lh = ui.line_height(&obj_style);
    if let Some(q) = list.iter().find(|q| Some(q.id) == journal.selected) {
        if let Some(t) = ui.record(&field(&jr, "questTitle")) {
            let tb = TextBox::from_record(&t);
            ui.text(&tb, Vec2::new(o.x + jx, y), &q.name);
            y += ui.sc(tb.rect.y + tb.rect.h);
        }
        let desc = |ui: &mut Ui, y: &mut f32, text: &str| {
            for l in ui.wrap_lines(&desc_style, desc_w, text) {
                ui.text_line(&desc_style, Vec2::new(content_x, *y), &l, 1.0);
                *y += desc_lh;
            }
        };
        if journal.completed {
            let text = q
                .tasks
                .iter()
                .filter(|t| t.state == STATE_COMPLETE && t.finished)
                .map(|t| t.description.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            desc(&mut ui, &mut y, &text);
        } else {
            for t in q.tasks.iter().filter(|t| t.state == STATE_IN_PROGRESS) {
                desc(&mut ui, &mut y, &t.description);
                if !t.objectives.is_empty() {
                    y += ui.sc(num(&jr, "objectiveSectionSpacing"));
                    let title = ui.fonts.tag("tagQuestObjectives").to_string();
                    let lh = ui.line_height(&obj_title_style);
                    ui.text_line(
                        &obj_title_style,
                        Vec2::new(content_x, y + ((major - lh) * 0.5).round()),
                        &title,
                        1.0,
                    );
                    y += major;
                    for (text, done) in &t.objectives {
                        let done = *done || t.state == STATE_COMPLETE;
                        let (cw, ch) = match checks.get(done as usize).cloned().flatten() {
                            Some((img, s)) => {
                                let (w, h) = (ui.sc(s.x as f32), ui.sc(s.y as f32));
                                ui.quad(
                                    img,
                                    URect::new(content_x, y + ((obj_lh - h) * 0.5).round(), w, h),
                                    None,
                                    Color::WHITE,
                                );
                                (w + ui.sc(4.0), h)
                            }
                            None => (0.0, 0.0),
                        };
                        let lines = ui.wrap_lines(&obj_style, desc_w - cw, text);
                        let y0 = y;
                        for l in &lines {
                            ui.text_line(&obj_style, Vec2::new(content_x + cw, y), l, 1.0);
                            y += obj_lh;
                        }
                        y = y.max(y0 + ch);
                    }
                }
                // Rewards.
                y += ui.sc(num(&jr, "rewardSectionSpacing"));
                let header_y = y;
                y += major;
                let reward_style = field(&jr, "rewardStyle");
                let rlh = ui.line_height(&reward_style);
                for r in &t.rewards {
                    ui.text_line(
                        &reward_style,
                        Vec2::new(content_x, y + ((minor - rlh) * 0.5).round()),
                        r,
                        1.0,
                    );
                    y += minor;
                }
                let items: Vec<(Item, Vec2)> = match Some(&*inv) {
                    Some(inv) => t
                        .reward_items
                        .iter()
                        .filter_map(|rec| {
                            let item = Item {
                                base_name: rec.clone(),
                                stack_count: 1,
                                var1: 1,
                                ..Default::default()
                            };
                            let info = inv.info(&item)?;
                            let (_, sz) = ui.image(&info.resolved.bitmap)?;
                            Some((item, Vec2::new(ui.sc(sz.x as f32), ui.sc(sz.y as f32))))
                        })
                        .collect(),
                    None => Vec::new(),
                };
                if !t.rewards.is_empty() || !items.is_empty() {
                    let title = ui.fonts.tag("tagQuestRewards").to_string();
                    let rtitle = field(&jr, "rewardTitleStyle");
                    let lh = ui.line_height(&rtitle);
                    ui.text_line(
                        &rtitle,
                        Vec2::new(content_x, header_y + ((major - lh) * 0.5).round()),
                        &title,
                        1.0,
                    );
                }
                if let (false, inv) = (items.is_empty(), &*inv) {
                    y += ui.sc(num(&jr, "itemSectionSpacing"));
                    let border = ui.sc(num(&jr, "itemBorderAdjust"));
                    let spacing = ui.sc(num(&jr, "itemSpacing"));
                    let sizes: Vec<Vec2> = items.iter().map(|(_, s)| *s).collect();
                    let (places, height) =
                        pack_items(&sizes, ui.sc(num(&jr, "width")) - 2.0 * border, spacing);
                    for ((item, sz), at) in items.iter().zip(places) {
                        crate::ui::character::item_cell(
                            &mut ui,
                            inv,
                            item,
                            URect::new(
                                (content_x + border + at.x).round(),
                                (y + at.y).round(),
                                sz.x,
                                sz.y,
                            ),
                        );
                    }
                    y += height - spacing * 0.5;
                }
            }
        }
    }
    let content = y - jpane.y;
    ui.root = outer;
    let top = o.y + jy + ui.sc(num(&jr, "scrollbarPositionAdjust"));
    let len = ui.sc(num(&jr, "height") + num(&jr, "scrollbarSizeAdjust"));
    layout.panes[0] = (jpane, (content - jpane.h).max(0.0));
    if content > jpane.h || jr.bool("alwaysShowScroll") {
        layout.bars[0] = scrollbar_live(
            &mut ui,
            &sb_rec(&jr),
            jpane.x + jpane.w,
            top,
            len,
            (jpane.h, content, journal.scroll[0]),
            ScrollOwner::Journal,
            0,
        );
    }

    // Lower page: the quest tree (sub_1402235A0 layout,
    // sub_140223920 draw): a group row (`majorRowHeight`) has its expand
    // bitmap (`expandButton`: frame 1 "—" while expanded) at the row's x,
    // vertically centred, the name 1.5 bitmap widths after it; a quest row
    // (`minorRowHeight`, indented `indentWidth`) has the track bitmap
    // (`trackBitmap`, frame 1 while tracked), the name 1.5 widths after it
    // and the main-quest icon at the tree's x; the selected quest's row gets
    // `nodeBitmap` tinted `selectedColor` from `leftPaddingNode` over the tree
    // width less `rightPaddingNode`. Groups in the order of their first quest.
    let Some(tree) = ui.record(&field(&lr, "tree")) else {
        return;
    };
    let lpane = URect::new(
        o.x + lx,
        o.y + ly + ui.sc(num(&lr, "topGuardBand")),
        ui.sc(num(&lr, "width")),
        ui.sc(num(&lr, "height") - num(&lr, "topGuardBand") - num(&lr, "bottomGuardBand")),
    );
    let inner = pane(&mut ui, lpane, journal.scroll[1]);
    ui.commands.entity(inner).insert(JournalScrolled {
        pane: 1,
        clip_y: lpane.y,
        inv: ui.inv,
    });
    ui.root = inner;
    let tx = o.x + lx + ui.sc(num(&tree, "position.X"));
    let mut y = lpane.y + ui.sc(num(&tree, "position.Y"));
    let (gmajor, minor, indent, tree_w) = (
        ui.sc(num(&tree, "majorRowHeight")),
        ui.sc(num(&tree, "minorRowHeight")),
        ui.sc(num(&tree, "indentWidth")),
        ui.sc(num(&tree, "treeWidth")),
    );
    let group_style = ui
        .record(&field(&tree, "groupNameText"))
        .and_then(|r| r.str("style").map(str::to_owned))
        .unwrap_or_default();
    let (primary, secondary, selected_style) = (
        field(&tree, "primaryStyle"),
        field(&tree, "secondaryStyle"),
        field(&tree, "selectedStyle"),
    );
    let node = ui.image(&field(&tree, "nodeBitmap"));
    let frames = |ui: &mut Ui, k: &str| -> Vec<Option<(Handle<Image>, UVec2)>> {
        ui.record(&field(&tree, k))
            .map(|r| {
                r.strs("bitmaps")
                    .into_iter()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
            .into_iter()
            .map(|p| ui.image(&p))
            .collect()
    };
    let (expand, track) = (
        frames(&mut ui, "expandButton"),
        frames(&mut ui, "trackBitmap"),
    );
    let star = ui
        .record(&field(&tree, "mainQuestIcon"))
        .and_then(|r| r.str("bitmapName").map(str::to_owned))
        .and_then(|p| ui.image(&p));
    let sel_color = Color::srgba(
        num(&tree, "selectedColor.R") / 255.0,
        num(&tree, "selectedColor.G") / 255.0,
        num(&tree, "selectedColor.B") / 255.0,
        num(&tree, "selectedColor.A") / 255.0,
    );
    let (lpad, rpad) = (
        ui.sc(num(&tree, "leftPaddingNode")),
        ui.sc(num(&tree, "rightPaddingNode")),
    );
    // The row under the mouse: `nodeBitmap` in `hoverColor` (sub_140223920;
    // the record spells its blue `hoverCover.B`, so the loaded blue is 0).
    let hover_color = Color::srgba(
        num(&tree, "hoverColor.R") / 255.0,
        num(&tree, "hoverColor.G") / 255.0,
        num(&tree, "hoverColor.B") / 255.0,
        num(&tree, "hoverColor.A") / 255.0,
    );
    let hover_node = |ui: &mut Ui, key: String, y: f32, h: f32| {
        if let Some((img, _)) = node.clone() {
            let e = ui.quad_stretch(img, URect::new(tx + lpad, y, tree_w - rpad, h), hover_color);
            let shown = journal.hover.as_deref() == Some(key.as_str());
            ui.commands.entity(e).insert((
                RowHover(key),
                if shown {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
            ));
        }
    };
    if list.is_empty() {
        let tag = if journal.completed {
            "tagQuestJournalNoComplete"
        } else {
            "tagQuestJournalNoQuests"
        };
        let t = ui.fonts.tag(tag).to_string();
        ui.text_line(&secondary, Vec2::new(tx, y), &t, 1.0);
    }
    let mut groups: Vec<&str> = Vec::new();
    for q in &list {
        if !groups.contains(&q.group.as_str()) {
            groups.push(&q.group);
        }
    }
    // A bitmap centred in a row at `x`; returns its rect.
    let centred = |ui: &mut Ui, img: &Option<(Handle<Image>, UVec2)>, x: f32, y: f32, h: f32| {
        let (img, s) = img.clone()?;
        let (w, bh) = (ui.sc(s.x as f32), ui.sc(s.y as f32));
        let r = URect::new(x, y + (h * 0.5 - bh * 0.5).round(), w, bh);
        ui.quad(img, r, None, Color::WHITE);
        Some(r)
    };
    for g in groups {
        let open = !journal.collapsed.iter().any(|c| c == g);
        hover_node(&mut ui, format!("g{g}"), y, gmajor);
        layout
            .group_rows
            .push((URect::new(tx, y, tree_w, gmajor), g.to_owned()));
        let mut x = tx;
        if let Some(r) = centred(
            &mut ui,
            &expand.get(open as usize).cloned().flatten(),
            x,
            y,
            gmajor,
        ) {
            x += (r.w * 1.5).round();
            layout.groups.push((r, g.to_owned()));
        }
        let glh = ui.line_height(&group_style);
        ui.text_line(
            &group_style,
            Vec2::new(x, y + ((gmajor - glh) * 0.5).round()),
            g,
            1.0,
        );
        y += gmajor;
        if !open {
            continue;
        }
        for q in list.iter().filter(|q| q.group == g) {
            let is_sel = journal.selected == Some(q.id);
            if is_sel && let Some((img, _)) = node.clone() {
                ui.quad_stretch(
                    img,
                    URect::new(tx + lpad, y, tree_w - rpad, minor),
                    sel_color,
                );
            }
            hover_node(&mut ui, format!("q{}", q.id), y, minor);
            let mut x = tx + indent;
            let tracked = !journal.untracked.contains(&q.id);
            if !journal.completed
                && let Some(r) = centred(
                    &mut ui,
                    &track.get(tracked as usize).cloned().flatten(),
                    x,
                    y,
                    minor,
                )
            {
                x += (r.w * 1.5).round();
                layout.tracks.push((r, q.id));
            }
            if q.main {
                centred(&mut ui, &star, tx, y, minor);
            }
            let style = if is_sel {
                &selected_style
            } else if q.main {
                &primary
            } else {
                &secondary
            };
            let lh = ui.line_height(style);
            ui.text_line(
                style,
                Vec2::new(x, y + ((minor - lh) * 0.5).round()),
                &q.name,
                1.0,
            );
            layout.rows.push((URect::new(tx, y, tree_w, minor), q.id));
            y += minor;
        }
    }
    let content = y - lpane.y;
    ui.root = outer;
    layout.panes[1] = (lpane, (content - lpane.h).max(0.0));
    if content > lpane.h || lr.bool("alwaysShowScroll") {
        let top = o.y + ly + ui.sc(num(&lr, "scrollbarPositionAdjust"));
        let len = ui.sc(num(&lr, "height") + num(&lr, "scrollbarSizeAdjust"));
        layout.bars[1] = scrollbar_live(
            &mut ui,
            &sb_rec(&lr),
            lpane.x + lpane.w,
            top,
            len,
            (lpane.h, content, journal.scroll[1]),
            ScrollOwner::Journal,
            1,
        );
    }
    layout.window = Some(URect::new(wr.x, wr.y, wr.w, wr.h));
}
