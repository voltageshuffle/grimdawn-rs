use super::*;

/// The devotion page (Devotion tab; `devotionJournal`),
/// `Grim Dawn.exe` `DevotionJournal::Rebuild` (rebuild): one scroll pane (`topGuardBand`,
/// `height`, `verticalScrollbar`) holding
/// - the title (`devotionTitle`): `tagDevotionJournalTitle` with
///   `Character::GetTotalDevotionPoints` / `GetMaxDevotionPoints`;
/// - a row per shrine the player restored (`Player::GetShrineUIDs`) that is
///   in the map's "Devotion Shrines" instance group: its record's
///   localized `journalTag`, rows sorted by that text (ordinal, std::sort
///   `DevotionJournal::AddRows`); a row is the `devotionBitmap` icon at its position plus
///   the running height, the `devotionText` box right of the icon (its
///   `textBoxX` / `textBoxY` from the icon's top-right), the running height
///   growing by the icon's height + 2.
///
/// PORT: the restored shrines are `DevotionState::restored` resolved through
/// the waypoint index (shrine records with a `journalTag`), standing for the
/// instance-group test.
#[allow(clippy::too_many_arguments)]
pub(super) fn devotion_page(
    ui: &mut Ui,
    journal: &QuestJournal,
    layout: &mut JournalLayout,
    devotion: Option<&gd_game::devotion::DevotionState>,
    index: Option<&gd_game::riftgates::WaypointIndex>,
    max_points: u32,
    o: Vec2,
    jr: &gd_formats::arz::Record,
) {
    let field = |r: &gd_formats::arz::Record, k: &str| r.str(k).unwrap_or("").to_string();
    let num = |r: &gd_formats::arz::Record, k: &str| r.f32(k).unwrap_or(0.0);
    let mut names: Vec<String> = devotion
        .into_iter()
        .flat_map(|d| d.restored.iter())
        .filter_map(|uid| index?.0.get(uid))
        .filter_map(|p| {
            ui.record(&p.record)?
                .str("journalTag")
                .filter(|t| !t.is_empty())
                .map(|t| plain(ui.fonts.tag(t)))
        })
        .collect();
    names.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    let pane_r = URect::new(
        o.x,
        o.y + ui.sc(num(jr, "topGuardBand")),
        ui.sc(num(jr, "width")),
        ui.sc(num(jr, "height") - num(jr, "topGuardBand") - num(jr, "bottomGuardBand")),
    );
    let outer = ui.root;
    let inner = pane(ui, pane_r, journal.scroll[0]);
    ui.commands.entity(inner).insert(JournalScrolled {
        pane: 0,
        clip_y: pane_r.y,
        inv: ui.inv,
    });
    ui.root = inner;
    let mut bottom = pane_r.y;
    if let Some(t) = ui.record(&field(jr, "devotionTitle")) {
        let tb = TextBox::from_record(&t);
        let (total, max) = devotion.map_or((0, 0), |d| (d.total, max_points));
        let text = crate::floating::format_tag(
            ui.fonts.tag("tagDevotionJournalTitle"),
            &[total as i64, max as i64],
        );
        ui.text(&tb, Vec2::new(o.x, pane_r.y), &text);
        bottom = bottom.max(pane_r.y + ui.sc(tb.rect.y + tb.rect.h));
    }
    let icon = ui.record(&field(jr, "devotionBitmap"));
    let text_tb = ui
        .record(&field(jr, "devotionText"))
        .map(|r| TextBox::from_record(&r));
    let mut y = 0.0;
    for n in &names {
        let Some(b) = &icon else { break };
        let Some((_, r)) = ui.bitmap(
            &field(b, "bitmapName"),
            num(b, "bitmapPositionX"),
            num(b, "bitmapPositionY") + y,
            Vec2::new(o.x, pane_r.y),
        ) else {
            break;
        };
        if let Some(tb) = &text_tb {
            ui.text(tb, Vec2::new(r.x + r.w, r.y), n);
        }
        bottom = bottom.max(r.y + r.h);
        y += r.h / ui.s + 2.0;
    }
    let content = bottom - pane_r.y;
    ui.root = outer;
    layout.panes[0] = (pane_r, (content - pane_r.h).max(0.0));
    if content > pane_r.h || jr.bool("alwaysShowScroll") {
        let top = o.y + ui.sc(num(jr, "scrollbarPositionAdjust"));
        let len = ui.sc(num(jr, "height") + num(jr, "scrollbarSizeAdjust"));
        layout.bars[0] = scrollbar_live(
            ui,
            &field(jr, "verticalScrollbar"),
            pane_r.x + pane_r.w,
            top,
            len,
            (pane_r.h, content, journal.scroll[0]),
            ScrollOwner::Journal,
            0,
        );
    }
}

/// The lore page (Codex tab; `codexJournal` / `codexLog`),
/// `Grim Dawn.exe` `LoreJournal::RebuildTree` (tree rebuild), `LoreJournal::AddNote` (adding a
/// note), `LoreJournal::CompareNotes` (note order), `LoreJournal::ShowPage` (the upper page):
/// - the notes are [`gd_game::lore::LoreCodex`], sorted by their localized
///   `description` (else `codexTitle`; ordinal compare), grouped under their
///   localized `subHeading` (a group per heading, groups sorted by name);
///   a note's row shows its `description` (`noteNameText`);
/// - the selected note's page: its title (`noteTitle`: `codexTitle`, else
///   `description`) and below it its `itemText` (`noteText`, wrapped);
///   selecting a note scrolls the page to the top.
///
/// PORT: groups start expanded; the note
/// row's indent is the tree's `indentWidth` (the row layout is the quest
/// tree's, not decompiled for the codex); `GenderizeText` is skipped (no
/// gender markup in the stock note texts).
pub(super) fn codex_pages(
    ui: &mut Ui,
    journal: &QuestJournal,
    layout: &mut JournalLayout,
    notes: &[String],
    o: Vec2,
    (jr, lr): (&gd_formats::arz::Record, &gd_formats::arz::Record),
) {
    let field = |r: &gd_formats::arz::Record, k: &str| r.str(k).unwrap_or("").to_string();
    let num = |r: &gd_formats::arz::Record, k: &str| r.f32(k).unwrap_or(0.0);
    struct Note {
        rec: String,
        name: String,
        title: String,
        text: String,
        group: String,
    }
    let mut list: Vec<Note> = notes
        .iter()
        .filter_map(|n| {
            let r = ui.record(n)?;
            let tag = |k: &str| {
                r.str(k)
                    .filter(|t| !t.is_empty())
                    .map(|t| ui.fonts.tag(t).to_string())
            };
            let name = tag("description")
                .or_else(|| tag("codexTitle"))
                .unwrap_or_default();
            let title = tag("codexTitle")
                .or_else(|| tag("description"))
                .unwrap_or_default();
            Some(Note {
                rec: n.clone(),
                title,
                text: plain(&tag("itemText").unwrap_or_default()),
                group: tag("subHeading").unwrap_or_default(),
                name,
            })
        })
        .collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    let mut groups: Vec<&str> = Vec::new();
    for n in &list {
        if !groups.contains(&n.group.as_str()) {
            groups.push(&n.group);
        }
    }
    groups.sort();
    // No note chosen yet: the first note of the first group (`LoreJournal::SelectFirstNote`).
    let selected: Option<String> = journal.note.clone().or_else(|| {
        groups
            .iter()
            .find_map(|g| list.iter().find(|n| n.group == *g))
            .map(|n| n.rec.clone())
    });
    let sb_rec = |r: &gd_formats::arz::Record| field(r, "verticalScrollbar");
    let (jx, jy) = (ui.sc(num(jr, "positionX")), ui.sc(num(jr, "positionY")));
    let (lx, ly) = (ui.sc(num(lr, "positionX")), ui.sc(num(lr, "positionY")));
    let outer = ui.root;

    let jpane = URect::new(
        o.x + jx,
        o.y + jy + ui.sc(num(jr, "topGuardBand")),
        ui.sc(num(jr, "width")),
        ui.sc(num(jr, "height") - num(jr, "topGuardBand")),
    );
    let inner = pane(ui, jpane, journal.scroll[0]);
    ui.commands.entity(inner).insert(JournalScrolled {
        pane: 0,
        clip_y: jpane.y,
        inv: ui.inv,
    });
    ui.root = inner;
    let mut y = jpane.y;
    if let Some(n) = list.iter().find(|n| selected.as_ref() == Some(&n.rec)) {
        if let Some(t) = ui.record(&field(jr, "noteTitle")) {
            let tb = TextBox::from_record(&t);
            ui.text(&tb, Vec2::new(o.x + jx, y), &n.title);
            y += ui.sc(tb.rect.y + tb.rect.h);
        }
        if let Some(t) = ui.record(&field(jr, "noteText")) {
            let style = field(&t, "fontStyle");
            let (x, w) = (
                o.x + jx + ui.sc(num(&t, "positionX")),
                ui.sc(num(&t, "textBoxWidth")),
            );
            y += ui.sc(num(&t, "positionY"));
            let lh = ui
                .fonts
                .style(ui.data, &style)
                .map_or(0.0, |st| st.scaled_size(ui.s) as f32);
            for l in ui.wrap_lines(&style, w, &n.text) {
                ui.text_line(&style, Vec2::new(x, y), &l, 1.0);
                y += lh;
            }
        }
    }
    let content = y - jpane.y;
    ui.root = outer;
    layout.panes[0] = (jpane, (content - jpane.h).max(0.0));
    if content > jpane.h || jr.bool("alwaysShowScroll") {
        let top = o.y + jy + ui.sc(num(jr, "scrollbarPositionAdjust"));
        let len = ui.sc(num(jr, "height") + num(jr, "scrollbarSizeAdjust"));
        layout.bars[0] = scrollbar_live(
            ui,
            &sb_rec(jr),
            jpane.x + jpane.w,
            top,
            len,
            (jpane.h, content, journal.scroll[0]),
            ScrollOwner::Journal,
            0,
        );
    }

    let Some(tree) = ui.record(&field(lr, "tree")) else {
        return;
    };
    let lpane = URect::new(
        o.x + lx,
        o.y + ly + ui.sc(num(lr, "topGuardBand")),
        ui.sc(num(lr, "width")),
        ui.sc(num(lr, "height") - num(lr, "topGuardBand") - num(lr, "bottomGuardBand")),
    );
    let inner = pane(ui, lpane, journal.scroll[1]);
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
    let text_style = |ui: &Ui, k: &str| {
        ui.record(&field(&tree, k))
            .and_then(|r| r.str("style").map(str::to_owned))
            .unwrap_or_default()
    };
    let (group_style, note_style) = (
        text_style(ui, "groupNameText"),
        text_style(ui, "noteNameText"),
    );
    let selected_style = field(&tree, "selectedStyle");
    let node = ui.image(&field(&tree, "nodeBitmap"));
    let expand: Vec<_> = ui
        .record(&field(&tree, "expandButton"))
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
    let color = |k: &str, b: &str| {
        Color::srgba(
            num(&tree, &format!("{k}.R")) / 255.0,
            num(&tree, &format!("{k}.G")) / 255.0,
            num(&tree, b) / 255.0,
            num(&tree, &format!("{k}.A")) / 255.0,
        )
    };
    // The record spells the hover blue `hoverCover.B` (loaded as 0).
    let (sel_color, hover_color) = (
        color("selectedColor", "selectedColor.B"),
        color("hoverColor", "hoverColor.B"),
    );
    let (lpad, rpad) = (
        ui.sc(num(&tree, "leftPaddingNode")),
        ui.sc(num(&tree, "rightPaddingNode")),
    );
    let shade = |ui: &mut Ui, on: bool, c: Color, y: f32, h: f32| {
        if on && let Some((img, _)) = node.clone() {
            ui.quad_stretch(img, URect::new(tx + lpad, y, tree_w - rpad, h), c);
        }
    };
    let hover_node = |ui: &mut Ui, layout: &mut JournalLayout, key: String, y: f32, h: f32| {
        if let Some((img, _)) = node.clone() {
            let e = ui.quad_stretch(img, URect::new(tx + lpad, y, tree_w - rpad, h), hover_color);
            let shown = journal.hover.as_deref() == Some(key.as_str());
            ui.commands.entity(e).insert((
                RowHover(key.clone()),
                if shown {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
            ));
        }
        layout.hovers.push((URect::new(tx, y, tree_w, h), key));
    };
    for g in groups {
        let key = format!("lore:{g}");
        let open = !journal.collapsed.contains(&key);
        hover_node(ui, &mut *layout, format!("g{key}"), y, gmajor);
        let mut x = tx;
        if let Some((img, s)) = expand.get(open as usize).cloned().flatten() {
            let (w, h) = (ui.sc(s.x as f32), ui.sc(s.y as f32));
            let r = URect::new(x, y + (gmajor * 0.5 - h * 0.5).round(), w, h);
            ui.quad(img, r, None, Color::WHITE);
            x += (w * 1.5).round();
            layout.groups.push((r, key));
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
        for n in list.iter().filter(|n| n.group == g) {
            let sel = selected.as_ref() == Some(&n.rec);
            shade(ui, sel, sel_color, y, minor);
            hover_node(ui, &mut *layout, format!("n{}", n.rec), y, minor);
            let style = if sel { &selected_style } else { &note_style };
            let lh = ui.line_height(style);
            ui.text_line(
                style,
                Vec2::new(tx + indent, y + ((minor - lh) * 0.5).round()),
                &n.name,
                1.0,
            );
            layout
                .notes
                .push((URect::new(tx, y, tree_w, minor), n.rec.clone()));
            y += minor;
        }
    }
    let content = y - lpane.y;
    ui.root = outer;
    layout.panes[1] = (lpane, (content - lpane.h).max(0.0));
    if content > lpane.h || lr.bool("alwaysShowScroll") {
        let top = o.y + ly + ui.sc(num(lr, "scrollbarPositionAdjust"));
        let len = ui.sc(num(lr, "height") + num(lr, "scrollbarSizeAdjust"));
        layout.bars[1] = scrollbar_live(
            ui,
            &sb_rec(lr),
            lpane.x + lpane.w,
            top,
            len,
            (lpane.h, content, journal.scroll[1]),
            ScrollOwner::Journal,
            1,
        );
    }
}

/// The help page (Help tab; `helpPage` upper, `helpIndex` lower),
/// `Grim Dawn.exe` `HelpIndex::Load` / `HelpIndex::Rebuild` (index), `HelpPage::Load` /
/// `HelpPage::SetContent` (page):
/// - the index lists every distinct record of the record's `tutorialPages`,
///   named by the localized `TextTag` of the page's first `NuggetArray`
///   nugget, sorted (ordinal); a flat list of `entryNameText` rows; with
///   nothing selected the first entry is;
/// - the page: its `altPage` (when set) replaces it; its nuggets stack from
///   `contentY`, each (after the first) after its `TopSpacing`: the text
///   (`TextTag` localized in `TextStyle`, wrapped to `contentWidth`, centered
///   when `TextCentering`; with a `keyOperation` the tag takes the name of
///   that action's primary key, `KeyBindings::GetActionByName`) at `contentX`, the `Image`s in
///   one row centered in the content width, 10 px apart; the nugget is as
///   high as its images plus its text.
///
/// - the index rows (node layout `HelpIndex::LayoutNodes`, draw `HelpIndex::DrawNode`):
///   `minorRowHeight` high, the name at the tree's x (no indent, no left
///   padding) centred vertically, the selection / hover shade
///   (`nodeBitmap`, `selectedColor` / `hoverColor`) from `leftPaddingNode`
///   to `treeWidth - rightPaddingNode` (`yPaddingNode`: absent, 0).
///
/// PORT: text lines step by the style's font size as the quest page's.
pub(super) fn help_pages(
    ui: &mut Ui,
    journal: &QuestJournal,
    layout: &mut JournalLayout,
    bindings: &gd_ui_kit::keys::KeyBindings,
    o: Vec2,
    (jr, lr): (&gd_formats::arz::Record, &gd_formats::arz::Record),
) {
    let field = |r: &gd_formats::arz::Record, k: &str| r.str(k).unwrap_or("").to_string();
    let num = |r: &gd_formats::arz::Record, k: &str| r.f32(k).unwrap_or(0.0);
    let mut pages: Vec<(String, String)> = Vec::new();
    for p in lr.strs("tutorialPages") {
        if pages.iter().any(|(r, _)| r == p) {
            continue;
        }
        let Some(first) = ui
            .record(p)
            .and_then(|r| r.strs("NuggetArray").first().map(|s| s.to_string()))
        else {
            continue;
        };
        let name = ui
            .record(&first)
            .and_then(|n| n.str("TextTag").map(|t| ui.fonts.tag(t).to_string()))
            .unwrap_or_default();
        pages.push((p.to_string(), name));
    }
    pages.sort_by(|a, b| a.1.cmp(&b.1));
    let sel = journal
        .help_page
        .clone()
        .filter(|s| pages.iter().any(|(r, _)| r == s))
        .or_else(|| pages.first().map(|p| p.0.clone()));
    let sb_rec = |r: &gd_formats::arz::Record| field(r, "verticalScrollbar");
    let (jx, jy) = (ui.sc(num(jr, "positionX")), ui.sc(num(jr, "positionY")));
    let (lx, ly) = (ui.sc(num(lr, "positionX")), ui.sc(num(lr, "positionY")));
    let outer = ui.root;

    let jpane = URect::new(
        o.x + jx,
        o.y + jy + ui.sc(num(jr, "topGuardBand")),
        ui.sc(num(jr, "width")),
        ui.sc(num(jr, "height") - num(jr, "topGuardBand")),
    );
    let inner = pane(ui, jpane, journal.scroll[0]);
    ui.commands.entity(inner).insert(JournalScrolled {
        pane: 0,
        clip_y: jpane.y,
        inv: ui.inv,
    });
    ui.root = inner;
    let mut y = jpane.y + ui.sc(num(jr, "contentY"));
    if let Some(page) = sel.as_ref().and_then(|s| ui.record(s)) {
        let page = page
            .str("altPage")
            .filter(|a| !a.is_empty())
            .and_then(|a| ui.record(a))
            .unwrap_or(page);
        let (cx, cw) = (
            jpane.x + ui.sc(num(jr, "contentX")),
            ui.sc(num(jr, "contentWidth")),
        );
        for (i, n) in page.strs("NuggetArray").into_iter().enumerate() {
            let Some(n) = ui.record(n) else { continue };
            if i > 0 {
                y += ui.sc(n.i32("TopSpacing").unwrap_or(0) as f32);
            }
            let style = field(&n, "TextStyle");
            let mut text_h = 0.0;
            if let Some(tag) = n.str("TextTag").filter(|t| !t.is_empty()) {
                let op = n.str("keyOperation").and_then(key_operation);
                let text = match op {
                    Some(a) => {
                        let key = gd_ui_kit::keys::key_name(bindings.get(a)[0]);
                        gd_sim::items::tooltip::format_exact(
                            ui.fonts.tag(tag),
                            &[gd_sim::items::tooltip::Arg::Str(&key)],
                        )
                        .trim()
                        .to_owned()
                    }
                    None => ui.fonts.tag(tag).trim().to_owned(),
                };
                let lh = ui
                    .fonts
                    .style(ui.data, &style)
                    .map_or(0.0, |st| st.scaled_size(ui.s) as f32);
                for l in ui.wrap_lines(&style, cw, &text) {
                    let x = if n.bool("TextCentering") {
                        cx + ((cw - ui.text_width(&style, &l)) * 0.5).max(0.0)
                    } else {
                        cx
                    };
                    ui.text_line(&style, Vec2::new(x, y + text_h), &l, 1.0);
                    text_h += lh;
                }
            }
            let imgs: Vec<_> = n
                .strs("Image")
                .into_iter()
                .filter_map(|p| ui.image(p))
                .collect();
            let sizes: Vec<Vec2> = imgs
                .iter()
                .map(|(_, s)| Vec2::new(ui.sc(s.x as f32), ui.sc(s.y as f32)))
                .collect();
            let gap = ui.sc(10.0);
            let total =
                sizes.iter().map(|s| s.x).sum::<f32>() + gap * sizes.len().saturating_sub(1) as f32;
            let mut x = cx + (cw - total) * 0.5;
            for ((img, _), s) in imgs.iter().zip(&sizes) {
                ui.quad(img.clone(), URect::new(x, y, s.x, s.y), None, Color::WHITE);
                x += s.x + gap;
            }
            y += sizes.iter().map(|s| s.y).fold(0.0, f32::max) + text_h;
        }
    }
    let content = y - jpane.y;
    ui.root = outer;
    layout.panes[0] = (jpane, (content - jpane.h).max(0.0));
    if content > jpane.h || jr.bool("alwaysShowScroll") {
        let top = o.y + jy + ui.sc(num(jr, "scrollbarPositionAdjust"));
        let len = ui.sc(num(jr, "height") + num(jr, "scrollbarSizeAdjust"));
        layout.bars[0] = scrollbar_live(
            ui,
            &sb_rec(jr),
            jpane.x + jpane.w,
            top,
            len,
            (jpane.h, content, journal.scroll[0]),
            ScrollOwner::Journal,
            0,
        );
    }

    let Some(tree) = ui.record(&field(lr, "tree")) else {
        return;
    };
    let lpane = URect::new(
        o.x + lx,
        o.y + ly + ui.sc(num(lr, "topGuardBand")),
        ui.sc(num(lr, "width")),
        ui.sc(num(lr, "height") - num(lr, "topGuardBand") - num(lr, "bottomGuardBand")),
    );
    let inner = pane(ui, lpane, journal.scroll[1]);
    ui.commands.entity(inner).insert(JournalScrolled {
        pane: 1,
        clip_y: lpane.y,
        inv: ui.inv,
    });
    ui.root = inner;
    let tx = o.x + lx + ui.sc(num(&tree, "position.X"));
    let mut y = lpane.y + ui.sc(num(&tree, "position.Y"));
    let (minor, tree_w) = (
        ui.sc(num(&tree, "minorRowHeight")),
        ui.sc(num(&tree, "treeWidth")),
    );
    let entry_style = ui
        .record(&field(&tree, "entryNameText"))
        .and_then(|r| r.str("style").map(str::to_owned))
        .unwrap_or_default();
    let selected_style = field(&tree, "selectedStyle");
    let node = ui.image(&field(&tree, "nodeBitmap"));
    let color = |k: &str, b: &str| {
        Color::srgba(
            num(&tree, &format!("{k}.R")) / 255.0,
            num(&tree, &format!("{k}.G")) / 255.0,
            num(&tree, b) / 255.0,
            num(&tree, &format!("{k}.A")) / 255.0,
        )
    };
    let (sel_color, hover_color) = (
        color("selectedColor", "selectedColor.B"),
        color("hoverColor", "hoverColor.B"),
    );
    let (lpad, rpad) = (
        ui.sc(num(&tree, "leftPaddingNode")),
        ui.sc(num(&tree, "rightPaddingNode")),
    );
    for (rec, name) in &pages {
        let on = sel.as_ref() == Some(rec);
        if on && let Some((img, _)) = node.clone() {
            ui.quad_stretch(
                img,
                URect::new(tx + lpad, y, tree_w - rpad, minor),
                sel_color,
            );
        }
        let key = format!("n{rec}");
        if let Some((img, _)) = node.clone() {
            let e = ui.quad_stretch(
                img,
                URect::new(tx + lpad, y, tree_w - rpad, minor),
                hover_color,
            );
            let shown = journal.hover.as_deref() == Some(key.as_str());
            ui.commands.entity(e).insert((
                RowHover(key.clone()),
                if shown {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
            ));
        }
        layout.hovers.push((URect::new(tx, y, tree_w, minor), key));
        let style = if on { &selected_style } else { &entry_style };
        let lh = ui.line_height(style);
        ui.text_line(
            style,
            Vec2::new(tx, y + ((minor - lh) * 0.5).round()),
            name,
            1.0,
        );
        layout
            .notes
            .push((URect::new(tx, y, tree_w, minor), rec.clone()));
        y += minor;
    }
    let content = y - lpane.y;
    ui.root = outer;
    layout.panes[1] = (lpane, (content - lpane.h).max(0.0));
    if content > lpane.h || lr.bool("alwaysShowScroll") {
        let top = o.y + ly + ui.sc(num(lr, "scrollbarPositionAdjust"));
        let len = ui.sc(num(lr, "height") + num(lr, "scrollbarSizeAdjust"));
        layout.bars[1] = scrollbar_live(
            ui,
            &sb_rec(lr),
            lpane.x + lpane.w,
            top,
            len,
            (lpane.h, content, journal.scroll[1]),
            ScrollOwner::Journal,
            1,
        );
    }
}

/// A `keyOperation` name's key action (`KeyBindings::GetActionByName`; unknown names map to 0).
pub fn key_operation(name: &str) -> Option<u32> {
    Some(match name {
        "CharacterWindow" => 1,
        "SkillWindow" => 2,
        "QuestWindow" => 3,
        "MapWindow" => 4,
        "LootFilterWindow" => 56,
        "ChatWindow" => 5,
        "PartyWindow" => 6,
        "OptionsWindow" => 7,
        "HelpWindow" => 8,
        "FactionsWindow" => 52,
        "AchievementWindow" => 53,
        "Use" => 54,
        "Pickup" => 55,
        "HotSlotSelect1" => 9,
        "HotSlotSelect2" => 10,
        "HotSlotSelect3" => 11,
        "HotSlotSelect4" => 12,
        "HotSlotSelect5" => 13,
        "HotSlotSelect6" => 14,
        "HotSlotSelect7" => 15,
        "HotSlotSelect8" => 16,
        "HotSlotSelect9" => 17,
        "HotSlotSelect0" => 18,
        "Camera_Zoom_In" => 21,
        "Camera_Zoom_Out" => 22,
        "Camera_Max_Zoom_In" => 23,
        "Camera_Max_Zoom_Out" => 24,
        "Camera_Default_Zoom" => 25,
        "Camera_Rotate_Left" => 27,
        "Camera_Rotate_Right" => 28,
        "Camera_Rotate" => 26,
        "DrinkManaPotion" => 29,
        "DrinkHealthPotion" => 30,
        "MapWindow_CenterMap" => 31,
        "DropItem" => 32,
        "PortalStone_Activate" => 33,
        "WeaponSwap" => 34,
        "ToggleHotSlotBar" => 43,
        "SelectPet1" => 44,
        "SelectPet2" => 45,
        "SelectPet3" => 46,
        "SelectPet4" => 47,
        "SelectPet5" => 48,
        "SelectPetAll" => 49,
        "ForceMove" => 50,
        "DisplayGroundItemPopupsNoFilter" => 35,
        "DisplayWorldItemTooltips" => 36,
        "DisplayGroundItemPopupsFilterCommonItems" => 37,
        "TargetPet" => 38,
        "StationaryAttack" => 39,
        "PushToTalk" => 58,
        "Evade" => 59,
        "ToggleUI" => 60,
        "AltarWindow" => 61,
        "ToggleHotSlotSet" => 62,
        "Move_Forward" => 63,
        "Move_Backward" => 64,
        "Move_Left" => 65,
        "Move_Right" => 66,
        _ => return None,
    })
}
