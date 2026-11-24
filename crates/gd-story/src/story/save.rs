//! Story state ⇄ `quests.gdd` / `conversations.gdd` (one pair per
//! difficulty). Layout per `Quest2Repository/Quest2/Quest2Task::
//! Read/WriteProperties`:
//! quest = `u32 id, MD5(file), block{u32 nTasks, tasks}`; task = `u32 uid,
//! MD5(task bytes), block{u32 state, u8 finished, u32 per counter condition
//! (types 10/15/16/28/29, objective order), u8 rewarded}`. A task whose hash
//! differs from the data (`Quest2Task::ReadProperties`)
//! keeps a saved "complete" state, any other saved state becomes the task's
//! current runtime state (`Quest2Task.state`); its counters are skipped, but the
//! finished / rewarded bytes are always imported.
//!
//! Every difficulty's `quests.gdd` is loaded (the per-difficulty state, finished and rewarded arrays,
//! `Quest2Task.difficultyStates` / `Quest2Task.finished` / `Quest2Task.rewarded`, are what reward catch-up
//! and difficulty skips read), the current one last; only
//! the current one sets the runtime state, the counters, the tokens and the active list. PORT: the counters
//! are kept for the current difficulty only, so another difficulty's file is written back
//! ([`Story::save_quests`]) by patching the state / finished / rewarded of the tasks it has and keeping the
//! rest of it (hashes, counters, tokens) as it is. Quests and tasks the data defines but a file lacks (added
//! by an expansion or mod after the save) are appended, as [`Story::save_conversations`] does.
use super::Story;
use super::quest::{STATE_AVAILABLE, STATE_COMPLETE, STATE_IN_PROGRESS};
use gd_formats::collections::HashSet;
use gd_formats::save::gdd;

impl Story {
    /// Loads the quest state of `difficulty` (and, if it is the current one,
    /// the runtime state, the counters, the token list and the active task list — in-progress tasks are
    /// re-begun with force, which runs no incentives). Load the current difficulty last (module doc).
    pub fn load_quests(&mut self, save: &gdd::Quests, difficulty: usize) {
        let defs = self.quests.defs.clone();
        let current = difficulty == self.difficulty();
        for sq in &save.quests {
            let Some(qi) = defs.by_id(sq.id) else {
                self.log(format!("quests.gdd: unknown quest {:#x}", sq.id));
                continue;
            };
            let quest = &defs.list[qi].quest;
            for st in &sq.tasks {
                let Some(ti) = quest.tasks.iter().position(|t| t.uid == st.id) else {
                    continue;
                };
                let tdef = &quest.tasks[ti];
                let changed = tdef.hash != st.uid;
                let rt = &mut self.quests.rt[qi].tasks[ti];
                let mut state = if st.state > 3 {
                    STATE_AVAILABLE
                } else {
                    st.state
                };
                if changed && state != STATE_COMPLETE {
                    state = rt.state;
                }
                rt.per_difficulty[difficulty] = state;
                rt.finished[difficulty] = st.in_progress == 1;
                rt.rewarded[difficulty] = st.complete == 1;
                if !changed && current {
                    let mut words = st
                        .conditions
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|w| u32::from_le_bytes(*w));
                    for (oi, o) in tdef.objectives.iter().enumerate() {
                        for (ci, c) in o.conditions.iter().flat_map(|c| c.list.iter().enumerate()) {
                            if c.saved_words() == 1 {
                                rt.objectives[oi].counters[ci] = words.next().unwrap_or(0);
                            }
                        }
                    }
                }
                if current {
                    rt.state = state;
                }
            }
        }
        if current {
            self.player.tokens = save.tokens.iter().cloned().collect();
            self.quests.active.clear();
            for (qi, q) in self.quests.rt.iter().enumerate() {
                for (ti, t) in q.tasks.iter().enumerate() {
                    if t.state == STATE_IN_PROGRESS {
                        self.quests.active.push((qi, ti));
                    }
                }
            }
        }
    }

    /// Writes the quest state of `difficulty`. With a `template` (the file
    /// that was loaded) its quest/task order, seed and header are kept, so an
    /// unchanged state re-encodes byte-identically; for another difficulty than the current one only its
    /// tasks' state bytes change (module doc). Quests and tasks the template lacks are appended.
    pub fn save_quests(&self, difficulty: usize, template: Option<&gdd::Quests>) -> gdd::Quests {
        let defs = &self.quests.defs;
        let current = difficulty == self.difficulty();
        let task_block = |qi: usize, ti: usize| -> gdd::QuestTask {
            let tdef = &defs.list[qi].quest.tasks[ti];
            let rt = &self.quests.rt[qi].tasks[ti];
            let mut conditions = Vec::new();
            for (oi, o) in tdef.objectives.iter().enumerate() {
                for (ci, c) in o.conditions.iter().flat_map(|c| c.list.iter().enumerate()) {
                    if c.saved_words() == 1 {
                        // The counters are the current difficulty's (module doc).
                        let n = if current {
                            rt.objectives[oi].counters[ci]
                        } else {
                            0
                        };
                        conditions.extend_from_slice(&n.to_le_bytes());
                    }
                }
            }
            gdd::QuestTask {
                id: tdef.uid,
                uid: tdef.hash,
                block_id: 0,
                state: if current {
                    rt.state
                } else {
                    rt.per_difficulty[difficulty]
                },
                in_progress: rt.finished[difficulty] as u8,
                conditions,
                complete: rt.rewarded[difficulty] as u8,
            }
        };
        let new_quest = |qi: usize| {
            let quest = &defs.list[qi].quest;
            gdd::Quest {
                id: quest.id,
                uid: quest.hash,
                block_id: 0,
                tasks: (0..quest.tasks.len())
                    .map(|ti| task_block(qi, ti))
                    .collect(),
            }
        };
        let quests = match template {
            Some(t) => {
                let mut quests: Vec<gdd::Quest> = t
                    .quests
                    .iter()
                    .map(|sq| match defs.by_id(sq.id) {
                        Some(qi) => {
                            let quest = &defs.list[qi].quest;
                            let mut tasks: Vec<gdd::QuestTask> = sq
                                .tasks
                                .iter()
                                .map(|st| match quest.tasks.iter().position(|t| t.uid == st.id) {
                                    Some(ti) if current => gdd::QuestTask {
                                        block_id: st.block_id,
                                        ..task_block(qi, ti)
                                    },
                                    Some(ti) => {
                                        let b = task_block(qi, ti);
                                        gdd::QuestTask {
                                            state: b.state,
                                            in_progress: b.in_progress,
                                            complete: b.complete,
                                            ..st.clone()
                                        }
                                    }
                                    None => st.clone(),
                                })
                                .collect();
                            tasks.extend(
                                quest
                                    .tasks
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, td)| !sq.tasks.iter().any(|st| st.id == td.uid))
                                    .map(|(ti, _)| task_block(qi, ti)),
                            );
                            gdd::Quest {
                                id: sq.id,
                                uid: quest.hash,
                                block_id: sq.block_id,
                                tasks,
                            }
                        }
                        None => sq.clone(),
                    })
                    .collect();
                let saved: HashSet<u32> = t.quests.iter().map(|sq| sq.id).collect();
                quests.extend(
                    (0..defs.list.len())
                        .filter(|&qi| !saved.contains(&defs.list[qi].quest.id))
                        .map(new_quest),
                );
                quests
            }
            None => (0..defs.list.len()).map(new_quest).collect(),
        };
        let tokens = if current {
            self.player.tokens.iter().cloned().collect()
        } else {
            template.map(|t| t.tokens.clone()).unwrap_or_default()
        };
        gdd::Quests {
            seed: template.map_or(0x1234_5678, |t| t.seed),
            version: 0,
            player_uid: template.map_or([0; 16], |t| t.player_uid),
            tokens_version: 2,
            tokens: match template {
                // Keep the file's token order when the set is unchanged.
                Some(t)
                    if t.tokens
                        .iter()
                        .cloned()
                        .collect::<std::collections::BTreeSet<_>>()
                        == self.player.tokens =>
                {
                    t.tokens.clone()
                }
                _ => tokens,
            },
            repository_version: 4,
            quests,
        }
    }

    /// Whether any task holds quest state of `difficulty` (not available, finished or rewarded): a difficulty
    /// without a file needs one only then.
    pub fn has_quest_state(&self, difficulty: usize) -> bool {
        self.quests.rt.iter().flat_map(|q| &q.tasks).any(|t| {
            t.per_difficulty[difficulty] != STATE_AVAILABLE
                || t.finished[difficulty]
                || t.rewarded[difficulty]
        })
    }

    pub fn load_conversations(&mut self, save: &gdd::Conversations) {
        self.dialogs.used.clear();
        for c in &save.conversations {
            self.dialogs.used.insert(c.id, c.values.clone());
        }
    }

    pub fn save_conversations(&self, template: Option<&gdd::Conversations>) -> gdd::Conversations {
        let hash_of = |id: u32| self.dialogs.by_id(id).map(|c| c.hash);
        let mut conversations: Vec<gdd::Conversation> = Vec::new();
        if let Some(t) = template {
            for c in &t.conversations {
                if let Some(v) = self.dialogs.used.get(&c.id) {
                    conversations.push(gdd::Conversation {
                        id: c.id,
                        uid: c.uid,
                        values: v.clone(),
                    });
                }
            }
        }
        let mut rest: Vec<_> = self
            .dialogs
            .used
            .iter()
            .filter(|(id, _)| !conversations.iter().any(|c| c.id == **id))
            .collect();
        rest.sort_by_key(|(id, _)| **id);
        for (id, v) in rest {
            conversations.push(gdd::Conversation {
                id: *id,
                uid: hash_of(*id).unwrap_or([0; 16]),
                values: v.clone(),
            });
        }
        gdd::Conversations {
            seed: template.map_or(0x1234_5679, |t| t.seed),
            version: 0,
            player_uid: template.map_or([0; 16], |t| t.player_uid),
            block_version: 1,
            conversations,
        }
    }
}
