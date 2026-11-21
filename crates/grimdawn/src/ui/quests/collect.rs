use super::*;

/// A quest event's shown text (`Quest2Event::GetText` @0x180421f00, Game.dll): the authored
/// text when the event has no action; otherwise always the first action's `Localize(text)`, which only
/// the `Give*` amounts implement and only for the text `*` (`ScriptableAction_GiveTribute::Localize`
/// @0x18046d220, `_GiveMoney` @0x18046c520, `_GiveExperience` @0x18046b0f0, `_GiveLevel` @0x18046c320,
/// `_GiveSkillPoint` @0x18046cfb0, `_GiveAttribPoint` @0x18046aba0, `_GiveDevotion` @0x18046af10): their
/// `tagActionGive*` line with the amount (Tribute: `tagActionGiveTribute` for 1, else `…Tributes`; the
/// others one tag whatever the amount). Any other first action, or another text, gives an empty line
/// (so an authored text next to an action, e.g. `bq_wg08`'s "Slay Reynar, the Loathed", is not shown).
/// No stock quest authors `*` (nor gives Tribute: the stock `GiveTribute`s are conversation spends), so the
/// Tribute line shows only for mods. Tribute has no floating text: `GiveTribute::Execute` @0x18046d160 and
/// `Character::AddTribute` @0x180062290 send nothing but GameEvent 63 (the Crucible pane's glow and sound,
/// `ui/survival.rs`), and the exe uses no `tagActionGive*` tag.
pub(super) fn event_text(
    fonts: &FontCache,
    text: String,
    actions: &[gd_formats::qst::Action],
) -> String {
    use gd_formats::qst::Action as A;
    let Some(first) = actions.first() else {
        return text;
    };
    if text != "*" {
        return String::new();
    }
    let (tag, n) = match first {
        A::GiveTribute { amount: 1 } => ("tagActionGiveTribute", 1),
        A::GiveTribute { amount } => ("tagActionGiveTributes", *amount),
        A::GiveMoney { amount } => ("tagActionGiveMoney", *amount),
        A::GiveExperience { amount } => ("tagActionGiveExperience", *amount),
        A::GiveLevel { amount } => ("tagActionGiveLevel", *amount),
        A::GiveSkillPoint { amount } => ("tagActionGiveSkillPoint", *amount),
        A::GiveAttribPoint { amount } => ("tagActionGiveAttributePoint", *amount),
        A::GiveDevotion { amount } => ("tagActionGiveDevotionPoint", *amount),
        _ => return String::new(),
    };
    // `%d` of the stored dword: a spend (a wrapped u32) prints negative.
    crate::ui::floating::format_text(
        fonts,
        tag,
        &[gd_game::floating::TextArg::Num(n as i32 as f32)],
    )
}

/// Story text as shown: `{^n}` / `^n` become line breaks; the `^X` colour markup stays and is drawn by the text
/// drawing (`gd_formats::ui::layout_text`, carried over wraps by `Ui::wrap_lines`).
pub(super) fn plain(text: &str) -> String {
    text.replace("{^n}", "\n").replace("^n", "\n")
}

/// Builds [`QuestView`] from the story's quest log, in the repository's
/// order (the world map's quest list hashed as the engine does).
#[allow(clippy::too_many_arguments)]
pub fn collect(
    mut fresh: gd_game::session::NewSession,
    rt: Option<NonSend<StoryRuntime>>,
    fonts: Res<FontCache>,
    world: Option<Res<crate::world::StreamedMap>>,
    (inv, stash): (
        Option<Res<crate::inventory::Inventory>>,
        Option<Res<gd_game::stash::Stash>>,
    ),
    mut order: Local<Vec<usize>>,
    mut seen: Local<Option<u64>>,
    mut inputs: Local<Option<(u64, u64, Option<bevy::ecs::change_detection::Tick>, usize)>>,
    mut view: ResMut<QuestView>,
) {
    if fresh.begun() {
        *order = Default::default();
        *seen = Default::default();
        *inputs = Default::default();
    }
    let Some(rt) = rt else { return };
    let story = rt.story.borrow();
    let log = &story.quests;
    if order.len() != log.defs.list.len() {
        let Some(world) = world else { return };
        *order = log.defs.repository_order(&world.map.quests);
    }
    // The log's revision (`QuestLog::revision`) and the item counts' sources: nothing the view shows can change
    // without one of them, so the hash below (a walk of every task and objective) runs only when one moves.
    let key = (
        log.revision,
        inv.as_ref().map_or(0, |i| i.version),
        stash.as_ref().map(|s| s.last_changed()),
        story.difficulty(),
    );
    if *inputs == Some(key) {
        return;
    }
    *inputs = Some(key);
    let d = story.difficulty();
    // The view is rebuilt (many strings) only when what it shows changed: a hash of the log's task / objective states and
    // counters, the only inputs besides the fixed texts. The counts (`objective_progress`, an inventory and stash scan
    // per item objective) only of the quests the view lists: a task in progress or done.
    let mut h = std::collections::hash_map::DefaultHasher::new();
    d.hash(&mut h);
    for &qi in order.iter() {
        let tasks = &log.rt[qi].tasks;
        let shown = tasks
            .iter()
            .any(|t| t.state == STATE_IN_PROGRESS || t.state == STATE_COMPLETE);
        for (ti, t) in tasks.iter().enumerate() {
            (t.state, t.finished[d]).hash(&mut h);
            for (oi, o) in t.objectives.iter().enumerate() {
                o.satisfied.hash(&mut h);
                if shown {
                    story.objective_progress(qi, ti, oi).hash(&mut h);
                }
            }
        }
    }
    let fp = h.finish();
    if *seen == Some(fp) {
        return;
    }
    *seen = Some(fp);
    let mut quests = Vec::new();
    for &qi in order.iter() {
        let def = &log.defs.list[qi];
        let rts = &log.rt[qi].tasks;
        if !rts
            .iter()
            .any(|t| t.state == STATE_IN_PROGRESS || t.state == STATE_COMPLETE)
        {
            continue;
        }
        let q = &def.quest;
        let tasks = q
            .tasks
            .iter()
            .zip(rts)
            .enumerate()
            .map(|(ti, (t, r))| TaskView {
                name: plain(q.texts.get(t.name_text)),
                description: plain(q.texts.get(t.desc_text)),
                state: r.state,
                finished: r.finished[d],
                objectives: t
                    .objectives
                    .iter()
                    .zip(&r.objectives)
                    .enumerate()
                    .map(|(oi, (o, ort))| {
                        (
                            gd_game::story::objective_text(
                                Some(&*fonts.tags),
                                &story,
                                q.texts.get(o.text).to_owned(),
                                (qi, ti, oi),
                                o,
                            ),
                            ort.satisfied == 1,
                        )
                    })
                    .filter(|(t, _)| !t.is_empty())
                    .collect(),
                rewards: t
                    .rewards
                    .iter()
                    .map(|e| event_text(&fonts, plain(q.texts.get(e.text)), &e.actions))
                    .filter(|t| !t.is_empty())
                    .collect(),
                reward_items: t
                    .rewards
                    .iter()
                    .filter(|e| {
                        event_text(&fonts, plain(q.texts.get(e.text)), &e.actions).is_empty()
                    })
                    .flat_map(|e| e.actions.iter())
                    .filter_map(|a| match a {
                        gd_formats::qst::Action::GiveItem { item, count } if *count > 0 => {
                            Some(item.clone())
                        }
                        _ => None,
                    })
                    .collect(),
            })
            .collect();
        quests.push(QuestEntry {
            id: q.id,
            name: plain(q.name()),
            group: plain(q.group()),
            main: q.flags & 1 != 0,
            tasks,
        });
    }
    if view.quests != quests {
        view.quests = quests;
    }
}
