//! Conversation runtime. Step availability is `ConversationStep::IsAvailable`:
//! a step whose flag bit 0 is set is
//! unavailable once used; its conditions (all/any) must hold; a link step (7)
//! also requires its target to be available. It decides whether an NPC has
//! something to say (`HasAvailableSpeech`).
//!
//! The window's step update (`ConversationWindow::UpdateStep` with the speech
//! collection `ConversationWindow::CollectSpeeches`; `Story::advance`), from the current step
//! (the root, or the response last picked; a link is replaced by its target):
//! * the candidates are the children (`GetSteps`, link-transparent) whose own
//!   conditions hold, in file order (no used check, no link-target conditions, and no step-kind
//!   filter: `CollectSpeeches` keeps every condition-holding child, verified in the decompile);
//! * the first candidate is shown, and so is every later one with flag bit 1
//!   ("merge"): for each, in order, its actions run (`GetActions`, on the NPC)
//!   and its responses are appended. No stock conversation sets bit 0 or 1;
//! * only the first supplies the text (`GetText`: own, else a link target's)
//!   and the VO ([`DialogView::voice`]), and is recorded as used after its
//!   actions ran (`AddSpeechToConversations` with its own index);
//! * responses are the children of types 2, 3, 4, 6 and 7 whose own
//!   conditions hold; with none the window adds "End Conversation"
//!   ([`DialogView::END`]); no candidate at all closes the window;
//! * long texts are paged (`split_pages`); the responses show on the last page.
//!
//! A type 8 step ("random speech", 4 stock steps) takes the text index of a
//! random direct `IsAvailable` speech child, keeping its own when there is none
//! (`ConversationStep::GetLocalizationIndex`): CRT `rand() / (32768 / n)`,
//! redrawn while `>= n`. `GetText` and `AddSpeechToConversations` draw
//! separately; the record is guarded by the step's `IsUsed` byte
//! (`Dialogs::shown`). PORT: the 15 bits come from `Story::rng`.
//!
//! "Used" speech steps are saved per conversation in `conversations.gdd` as
//! the list of step indices in the order they were shown.
use super::Story;
use super::quest::{LuaCond, collect_lua_conds};
use gd_formats::cnv::{Conversation, Step, StepType};
use gd_formats::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Default)]
pub struct Dialogs {
    pub defs: HashMap<String, Arc<Conversation>>,
    /// Conversation id → its definition, built from `defs` in [`Dialogs::new`]
    /// only. PORT: on an id collision the smallest path wins (no stock
    /// collision; the engine's store is not ported).
    by_id: HashMap<u32, Arc<Conversation>>,
    /// Conversation id → used speech step indices (save order).
    pub used: HashMap<u32, Vec<u16>>,
    /// Non-speech steps (type 8, or a link in speech position) already shown:
    /// the step's in-memory used byte (`ConversationStep::IsUsed`), which
    /// `ConversationStep::Read` initialises from the
    /// stored list for type 1 speeches only; for other types it starts clear
    /// and is set when the window shows the step. Not saved.
    pub shown: HashSet<(u32, u32)>,
    pub session: Option<Session>,
    /// Normalised path → the `LuaScript` conditions of the conversation's steps (unique keys; conversations
    /// without any are absent). Built in [`Dialogs::new`].
    lua: HashMap<String, Vec<Arc<LuaCond>>>,
}

impl Dialogs {
    /// The conversations by normalised path, indexed by id.
    pub fn new(defs: HashMap<String, Arc<Conversation>>) -> Self {
        let mut paths: Vec<&String> = defs.keys().collect();
        paths.sort_unstable();
        let mut by_id = HashMap::with_capacity_and_hasher(defs.len(), Default::default());
        let mut lua = HashMap::default();
        let mut seen = HashSet::default();
        for p in paths {
            let c = &defs[p];
            by_id.entry(c.id).or_insert_with(|| c.clone());
            seen.clear();
            let mut list = Vec::new();
            for step in c.steps() {
                collect_lua_conds(step.conditions.as_ref(), &mut seen, &mut list);
            }
            if !list.is_empty() {
                lua.insert(p.clone(), list);
            }
        }
        Self {
            defs,
            by_id,
            lua,
            ..Self::default()
        }
    }
    /// The conversation with id `id` (see `by_id`).
    pub fn by_id(&self, id: u32) -> Option<&Arc<Conversation>> {
        self.by_id.get(&id)
    }
    pub fn is_used(&self, conv: u32, step: u32) -> bool {
        self.used
            .get(&conv)
            .is_some_and(|u| u.contains(&(step as u16)))
    }
    /// The `LuaScript` conditions of the conversation at `path` (any spelling), if it has any.
    pub fn lua_conds(&self, path: &str) -> Option<&[Arc<LuaCond>]> {
        if self.lua.is_empty() {
            return None;
        }
        self.lua
            .get(&super::quest::normalize_path(path))
            .map(Vec::as_slice)
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub conv: Arc<Conversation>,
    pub npc: u32,
    /// Speech step currently shown.
    pub speech: u32,
}

/// What the dialog UI shows.
#[derive(Debug, Clone, PartialEq)]
pub struct DialogView {
    pub npc: u32,
    pub speaker: String,
    /// The whole speech text.
    pub text: String,
    /// `text` split into the window's pages (`split_pages`); never empty.
    pub pages: Vec<String>,
    /// Page shown. While it is not the last, the window offers only a
    /// "Continue" button (`tagContinue`, see `next_page`); the responses are
    /// listed on the last page.
    pub page: usize,
    /// (step index, text, type tag) of each available response. When the
    /// speech has none, the window's own "End Conversation" button
    /// ([`DialogView::END`]) is the only entry.
    pub responses: Vec<(u32, String, &'static str)>,
    /// The voice-over file of the speech shown (empty: none).
    /// `ConversationStep::PlayVO`: the shown step's
    /// own VO, a link's (type 7) that of its first non-link target; a type 8
    /// step plays its own, not the drawn child's. Stock: 167 files, all
    /// `.wav` under `sound/dialog/quest/` (and three ghost vocalisations).
    pub voice: String,
}

impl DialogView {
    /// Response index of the "End Conversation" button the window adds when
    /// a speech has no available response. `ConversationWindow::AddEndButton`
    /// (button labelled `tagEndConversation`, no step attached)
    /// and `ConversationWindow::OnButtonClicked` (clicking a button without a step runs no actions and
    /// leaves no next step, so the window closes). Its text is empty: the UI
    /// shows the localised tag.
    pub const END: u32 = u32::MAX;

    pub fn page_text(&self) -> &str {
        self.pages.get(self.page).map_or("", |s| s.as_str())
    }
    pub fn on_last_page(&self) -> bool {
        self.page + 1 >= self.pages.len()
    }
    /// The page "Continue" button: `ConversationWindow::AddContinueButton` (the
    /// button carries the current step) and `ConversationWindow::OnButtonClicked` (clicking a button
    /// whose step is the current one only requests the next page; no
    /// actions run). Returns false on the last page.
    pub fn next_page(&mut self) -> bool {
        if self.on_last_page() {
            return false;
        }
        self.page += 1;
        true
    }
}

/// Characters per page: the `autosplit` field of the conversation window
/// record (`records/ingameui/conversationwindow/conversationwindow.dbr`,
/// value 500; read by `ConversationWindow::Load`).
pub const PAGE_CHARS: usize = 500;
/// `LocalizationManager::GetSentenceSeperators`: the `separators` entry of
/// the language's `text_XX/language.def` ([`gd_formats::text::LanguageDef`]),
/// default `.?!` (`LocalizationManager::InitializeLanguage`). The English text
/// archive ships no `language.def`, so the default applies. The session's
/// value is [`Story::separators`] (`gd_game::story::setup_story` reads the
/// language's def); this is its initial value.
pub const SENTENCE_SEPARATORS: &str = ".?!";

/// The window's text paging. `ConversationWindow::NextPageEnd` (one call
/// per page) and `ConversationWindow::UpdateStep` (the paging loop), counting UTF-16 units:
/// - from the current offset, take `size` units; if the offset plus `size`
///   reaches the end, the rest is the last page;
/// - otherwise scan that window backwards for a sentence separator; one found
///   past the window's first unit ends the page just after it;
/// - with none (or only at the first unit) the page ends after the last
///   space of the window (the whole window when it has no space) and gets
///   " ..." appended;
/// - the offset advances by the page's length (without the " ..."), then
///   past any spaces; once it reaches the end there is no further page.
pub fn split_pages(text: &str, size: usize, separators: &str) -> Vec<String> {
    let u: Vec<u16> = text.encode_utf16().collect();
    let seps: Vec<u16> = separators.encode_utf16().collect();
    let mut pages = Vec::new();
    let mut off = 0;
    loop {
        if size == 0 || off + size >= u.len() {
            pages.push(String::from_utf16_lossy(&u[off.min(u.len())..]));
            break;
        }
        let window = &u[off..off + size];
        let mut space = None;
        let mut sep = None;
        for i in (0..size).rev() {
            if seps.contains(&window[i]) {
                sep = Some(i);
                break;
            }
            if space.is_none() && window[i] == u16::from(b' ') {
                space = Some(i);
            }
        }
        let take = match sep {
            Some(i) if i > 0 => {
                pages.push(String::from_utf16_lossy(&window[..=i]));
                i + 1
            }
            _ => {
                let take = space.map_or(size, |s| s + 1);
                pages.push(String::from_utf16_lossy(&window[..take]) + " ...");
                take
            }
        };
        off += take;
        while off < u.len() && u[off] == u16::from(b' ') {
            off += 1;
        }
        if off >= u.len() {
            break;
        }
    }
    pages
}

fn find(step: &Step, index: u32) -> Option<&Step> {
    if step.index == index {
        return Some(step);
    }
    step.children.iter().find_map(|c| find(c, index))
}

/// `ConversationStep::GetSteps`: a link
/// without children stands for its target's (following link chains).
fn steps_of<'a>(conv: &'a Conversation, step: &'a Step) -> &'a [Step] {
    let mut s = step;
    for _ in 0..64 {
        match (
            s.kind,
            s.children.is_empty(),
            s.link.and_then(|l| find(&conv.root, l)),
        ) {
            (StepType::Link, true, Some(t)) => s = t,
            _ => break,
        }
    }
    &s.children
}

/// `ConversationStep::GetActions`: a link
/// without actions runs its target's (following link chains).
fn actions_of<'a>(conv: &'a Conversation, step: &'a Step) -> &'a [gd_formats::qst::Action] {
    let mut s = step;
    for _ in 0..64 {
        match (
            s.kind,
            s.actions.is_empty(),
            s.link.and_then(|l| find(&conv.root, l)),
        ) {
            (StepType::Link, true, Some(t)) => s = t,
            _ => break,
        }
    }
    &s.actions
}

/// `ConversationStep::PlayVO` / `StopVO`: a link stands for its first
/// non-link target (`ConversationStep.linkTarget`, the resolved link); an unresolved link has none.
fn voice_of<'a>(conv: &'a Conversation, step: &'a Step) -> &'a str {
    let mut s = step;
    for _ in 0..64 {
        if s.kind != StepType::Link {
            return &s.voice;
        }
        match s.link.and_then(|l| find(&conv.root, l)) {
            Some(t) => s = t,
            None => break,
        }
    }
    ""
}

impl Story {
    /// `ConversationStep::IsAvailable`.
    pub fn step_available(&self, conv: &Conversation, step: &Step) -> bool {
        let mut s = step;
        for _ in 0..64 {
            if s.flags & 1 != 0 && self.dialogs.is_used(conv.id, s.index) {
                return false;
            }
            if !self.eval_conditions(s.conditions.as_ref(), &[]) {
                return false;
            }
            match (s.kind, s.link.and_then(|l| find(&conv.root, l))) {
                (StepType::Link, Some(t)) => s = t,
                _ => return true,
            }
        }
        true
    }

    fn first_speech<'a>(&self, conv: &'a Conversation, parent: &'a Step) -> Option<&'a Step> {
        // A childless link stands for its target's children (HasAvailableSpeech).
        steps_of(conv, parent)
            .iter()
            .find(|c| c.kind == StepType::Speech && self.step_available(conv, c))
    }

    /// The faction lock of an NPC's conversation `ConversationWindow::Open`
    /// (window open) over `Conversation::CollectQuestReferences`:
    /// every step with a `BeginQuestTask` action
    /// (link steps skipped, a childless link's children are its target's) is
    /// checked together with its ancestors (`GetParent`): each condition that
    /// does not hold, is a `HasFaction` of the NPC's own faction (`npc_faction`,
    /// compared by name) and whose requirement exceeds the player's value
    /// raises the result to (faction, requirement - value) when the
    /// requirement is above the deficit kept so far (the engine's compare, 0
    /// at first). Any other unmet condition ends that step's scan of the
    /// condition list. `None`: nothing locked (the window's -1000).
    pub fn locked_quest_faction(&self, npc: u32, npc_faction: &str) -> Option<(String, i32)> {
        use gd_formats::qst::{Action, Condition};
        let path = self.entities.get(npc)?.conversation.as_deref()?;
        let conv = self.dialogs.defs.get(&super::quest::normalize_path(path))?;
        let mut best: Option<(String, i32)> = None;
        let mut deficit = 0;
        // Depth-first over the begin-quest steps, with each one's ancestor chain.
        fn walk<'a>(
            conv: &'a Conversation,
            step: &'a Step,
            chain: &mut Vec<&'a Step>,
            out: &mut Vec<Vec<&'a Step>>,
        ) {
            if step.kind == StepType::Link {
                return;
            }
            chain.push(step);
            if step
                .actions
                .iter()
                .any(|a| matches!(a, Action::BeginQuestTask { .. }))
            {
                out.push(chain.clone());
            }
            for c in steps_of(conv, step) {
                walk(conv, c, chain, out);
            }
            chain.pop();
        }
        let mut found = Vec::new();
        walk(conv, &conv.root, &mut Vec::new(), &mut found);
        for chain in found {
            for step in chain.iter().rev() {
                for e in step.conditions.iter().flat_map(|c| c.list.iter()) {
                    if self.eval_condition(e, 0) {
                        continue;
                    }
                    let Condition::HasFaction { faction, value } = &e.cond else {
                        break;
                    };
                    if !faction.trim().eq_ignore_ascii_case(npc_faction) {
                        break;
                    }
                    let missing = (*value as f32 - self.player.faction(faction)) as i32;
                    if missing > 0 && *value > deficit {
                        deficit = missing;
                        best = Some((faction.clone(), missing));
                    }
                }
            }
        }
        best
    }

    /// Whether talking to an NPC with this conversation would show something (`HasAvailableSpeech`).
    pub fn has_speech(&self, conversation: &str) -> bool {
        let Some(conv) = self
            .dialogs
            .defs
            .get(&super::quest::normalize_path(conversation))
        else {
            return false;
        };
        self.first_speech(conv, &conv.root).is_some()
    }

    /// Start talking to an NPC. `ConversationWindow::BeginConversation` (the
    /// conversation window opening): nothing happens unless the root step's
    /// conditions hold; then the root's speech is shown (`advance`), and only
    /// after that do the root step's actions run (also when no speech was
    /// available and the window closes again).
    pub fn talk(&mut self, npc: u32) -> Option<DialogView> {
        let path = self.entities.get(npc)?.conversation.clone()?;
        let conv = self
            .dialogs
            .defs
            .get(&super::quest::normalize_path(&path))?
            .clone();
        if !self.eval_conditions(conv.root.conditions.as_ref(), &[]) {
            return None;
        }
        let view = self.advance(conv.clone(), npc, 0);
        if view.is_none() {
            self.log(format!(
                "No available speech found for conversation '{path}'!"
            ));
        }
        self.exec_actions(&conv.root.actions, Some(npc));
        view
    }

    /// The window's step update from step `current` (module doc): runs the
    /// shown speeches' actions, records the first as used and returns its
    /// view; `None` when no speech is available (the window closes).
    fn advance(&mut self, conv: Arc<Conversation>, npc: u32, current: u32) -> Option<DialogView> {
        let mut step = find(&conv.root, current)?;
        if step.kind == StepType::Link {
            step = find(&conv.root, step.link?)?;
        }
        let candidates: Vec<&Step> = steps_of(&conv, step)
            .iter()
            .filter(|c| self.eval_conditions(c.conditions.as_ref(), &[]))
            .collect();
        let mut shown: Option<(u32, String, String)> = None;
        let mut responses = Vec::new();
        for speech in candidates {
            let index = speech.index;
            if shown.is_some() && speech.flags & 2 == 0 {
                continue;
            }
            self.exec_actions(actions_of(&conv, speech), Some(npc));
            for c in steps_of(&conv, speech) {
                let response = matches!(
                    c.kind,
                    StepType::Continue
                        | StepType::Accept
                        | StepType::Decline
                        | StepType::End
                        | StepType::Link
                );
                if response && self.eval_conditions(c.conditions.as_ref(), &[]) {
                    let tag = match (c.kind, c.link.and_then(|l| find(&conv.root, l))) {
                        (StepType::Link, Some(t)) => t.kind.tag(),
                        (k, _) => k.tag(),
                    };
                    responses.push((c.index, conv.text(c).to_string(), tag));
                }
            }
            if shown.is_none() {
                // `Conversation::GetText`: the text at the localisation
                // index; an empty one falls back to a link's target.
                let text_index = self.localization_index(&conv, speech);
                let mut text = conv.texts.get(text_index);
                if text.is_empty() && speech.kind == StepType::Link {
                    text = conv.text(speech);
                }
                shown = Some((index, text.to_string(), voice_of(&conv, speech).to_string()));
                let is_speech = speech.kind == StepType::Speech;
                let is_used = if is_speech {
                    self.dialogs.is_used(conv.id, index)
                } else {
                    self.dialogs.shown.contains(&(conv.id, index))
                };
                if !is_used {
                    let recorded = self.localization_index(&conv, speech);
                    self.dialogs
                        .used
                        .entry(conv.id)
                        .or_default()
                        .push(recorded as u16);
                }
                if !is_speech {
                    self.dialogs.shown.insert((conv.id, index));
                }
            }
        }
        let (shown, text, voice) = shown?;
        if responses.is_empty() {
            responses.push((DialogView::END, String::new(), StepType::End.tag()));
        }
        let view = DialogView {
            npc,
            speaker: conv.texts.get(0).to_string(),
            pages: split_pages(&text, PAGE_CHARS, &self.separators),
            page: 0,
            text,
            responses,
            voice,
        };
        self.dialogs.session = Some(Session {
            conv,
            npc,
            speech: shown,
        });
        Some(view)
    }

    /// `ConversationStep::GetLocalizationIndex` (module doc).
    fn localization_index(&mut self, conv: &Conversation, step: &Step) -> u32 {
        if step.raw_type != 8 {
            return step.index;
        }
        let pool: Vec<&Step> = step
            .children
            .iter()
            .filter(|c| c.kind == StepType::Speech && self.step_available(conv, c))
            .collect();
        let n = pool.len() as u32;
        if n == 0 {
            return step.index;
        }
        let pick = loop {
            // (bad data: a pool over 0x8000 would divide by zero.)
            let r = (self.rng.next() & 0x7fff) / (0x8000 / n).max(1);
            if r < n {
                break r;
            }
        };
        self.localization_index(conv, pool[pick as usize])
    }

    /// Pick a response (by step index). Returns the next view, or `None` when
    /// the conversation ends. `ConversationWindow::OnButtonClicked` (response
    /// click) and `ConversationWindow::UpdateStep`: the response's actions run
    /// (`GetActions`: an actionless link runs its target's), it becomes the
    /// current step, and an Accept, Decline or End response closes the window;
    /// any other continues with its speeches (`advance`). [`DialogView::END`]
    /// just closes the window.
    pub fn choose(&mut self, response: u32) -> Option<DialogView> {
        let Session { conv, npc, .. } = self.dialogs.session.take()?;
        if response == DialogView::END {
            return None;
        }
        let step = find(&conv.root, response)?;
        let kind = step.kind;
        self.exec_actions(actions_of(&conv, step), Some(npc));
        if matches!(kind, StepType::Accept | StepType::Decline | StepType::End) {
            return None;
        }
        self.advance(conv, npc, response)
    }
}

/// The mark floating over an NPC (`Conversation::Punctuation`, the values
/// `Conversation::GetPunctuation` returns; the FX record each one attaches is
/// `gd_game::npc_marks::punctuation_record`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Punctuation {
    None = 0,
    /// A quest can be started here (or a turn-in step is reachable).
    Exclamation = 1,
    /// A quest of this conversation is in progress.
    QuestionGray = 2,
    /// A quest of this conversation can be turned in.
    QuestionGold = 4,
    /// Something to say that was heard before.
    Speech = 5,
    /// An unheard speech (flag bit 2 clear).
    NewSpeech = 6,
}

impl Story {
    /// `Conversation::GetPunctuation` for the
    /// NPC's conversation file:
    /// 1. `Conversation::GetQuestPunctuation` over the step tree, if the
    ///    conversation references quests: an available step's
    ///    `BeginQuestTask` whose task is Available gives 1; its
    ///    `CompleteQuestTask` whose task is in progress gives 4 when the
    ///    task's objectives are met ([`Story::task_objectives_met`]) and 1
    ///    otherwise; children are visited after (a later hit overwrites,
    ///    except that a 4 returns from its own step's action list at once;
    ///    a speech child ends the child scan);
    /// 2. else 2 when a quest completed by one of the conversation's steps
    ///    (`Conversation::CollectQuestReferences`: non-link steps' actions,
    ///    depth first) has any task in progress (`Quest2Task::InProgress`:
    ///    state 2; its `Quest2Task.multiplayerBlocked` flag is the multiplayer block, never set here);
    /// 3. else `HasAvailableSpeech` (the first available speech of the root,
    ///    link-transparent): 6 when its flag bit 2 is clear and it was not
    ///    used yet, else 5; none: 0.
    pub fn conversation_punctuation(&self, conversation: &str) -> Punctuation {
        let Some(conv) = self
            .dialogs
            .defs
            .get(&super::quest::normalize_path(conversation))
        else {
            return Punctuation::None;
        };
        let mut p = Punctuation::None;
        self.quest_punctuation(conv, &conv.root, &mut p);
        if p != Punctuation::None {
            return p;
        }
        let mut refs = Vec::new();
        collect_complete_refs(&conv.root, &mut refs);
        for quest in refs {
            let Some(qi) = self.quests.defs.by_path(quest) else {
                continue;
            };
            if self.quests.rt[qi]
                .tasks
                .iter()
                .any(|t| t.state == super::quest::STATE_IN_PROGRESS)
            {
                return Punctuation::QuestionGray;
            }
        }
        match self.first_speech(conv, &conv.root) {
            None => Punctuation::None,
            Some(s) if s.flags & 4 == 0 && !self.dialogs.is_used(conv.id, s.index) => {
                Punctuation::NewSpeech
            }
            Some(_) => Punctuation::Speech,
        }
    }

    /// `Conversation::GetQuestPunctuation`.
    fn quest_punctuation(&self, conv: &Conversation, step: &Step, out: &mut Punctuation) {
        use super::quest::{STATE_AVAILABLE, STATE_IN_PROGRESS};
        use gd_formats::qst::Action;
        if !self.step_available(conv, step) {
            return;
        }
        let actions = if step.kind == StepType::Link {
            actions_of(conv, step)
        } else {
            &step.actions[..]
        };
        for a in actions {
            match a {
                Action::BeginQuestTask { quest, task } => {
                    let Some(qi) = self.quests.defs.by_path(quest) else {
                        continue;
                    };
                    let Some(ti) = self.quests.task_index(qi, *task) else {
                        continue;
                    };
                    if self.quests.task_state(qi, ti) == STATE_AVAILABLE {
                        *out = Punctuation::Exclamation;
                    }
                }
                Action::CompleteQuestTask { quest, task } => {
                    let Some(qi) = self.quests.defs.by_path(quest) else {
                        continue;
                    };
                    let Some(ti) = self.quests.task_index(qi, *task) else {
                        continue;
                    };
                    if self.quests.task_state(qi, ti) != STATE_IN_PROGRESS {
                        continue;
                    }
                    if self.task_objectives_met(qi, ti) {
                        *out = Punctuation::QuestionGold;
                        return;
                    }
                    *out = Punctuation::Exclamation;
                }
                _ => {}
            }
        }
        if step.kind != StepType::Link {
            for c in &step.children {
                self.quest_punctuation(conv, c, out);
                if c.kind == StepType::Speech {
                    break;
                }
            }
        }
    }
}

/// The quests of the `CompleteQuestTask` actions of a step tree, in
/// `CollectQuestReferences` order (link steps are not entered).
fn collect_complete_refs<'a>(step: &'a Step, out: &mut Vec<&'a str>) {
    if step.kind == StepType::Link {
        return;
    }
    for a in &step.actions {
        if let gd_formats::qst::Action::CompleteQuestTask { quest, .. } = a {
            out.push(quest);
        }
    }
    for c in &step.children {
        collect_complete_refs(c, out);
    }
}
