//! Loaders for every quest/conversation file of the layered install (VFS).
//!
//! * Quests: PORT. [`quest_defs`] loads every `quests/*.qst` of the VFS up
//!   front; the engine loads the current world's quest-file list
//!   (`Quest2Repository::LoadQuests` via `World::GetQuestFile`; see
//!   [`QuestDefs::repository_order`]).
//! * Conversations: PORT. [`dialogs`] loads every `conversations/*.cnv` up
//!   front; the engine loads a character's conversation from its record's
//!   `conversation` field (`Character::Load`).
//! * Resilience: PORT. A file the VFS cannot read or parse is skipped with a
//!   warning, not propagated.
//! * Text: a non-English session replaces each file's embedded text block with
//!   the line-ordered `text_<code>/<basename>.txt` override
//!   ([`gd_formats::text::load_text_override`], `PortableLocalization::Read`).
//!   PORT: a missing override keeps the embedded English (the engine fails the
//!   load).
use super::dialog::Dialogs;
use super::quest::{QuestDef, QuestDefs, normalize_path};
use anyhow::Context;
use gd_formats::cnv::Conversation;
use gd_formats::collections::HashMap;
use gd_formats::qst::{Quest, Texts};
use gd_formats::text::load_text_override;
use gd_formats::vfs::Vfs;
use std::sync::Arc;

/// Every `quests/*.qst` of the VFS, localized.
pub fn quest_defs(vfs: &Vfs, language: &str) -> QuestDefs {
    let mut paths: Vec<&str> = vfs
        .paths()
        .filter(|p| p.starts_with("quests/") && p.ends_with(".qst"))
        .collect();
    paths.sort();
    let mut defs = QuestDefs::default();
    for p in paths {
        let quest = vfs
            .read(p)
            .and_then(|data| Quest::parse(&data))
            .with_context(|| p.to_string());
        match quest {
            Ok(quest) => defs.add(QuestDef {
                path: normalize_path(p),
                quest,
            }),
            Err(e) => log::warn!("story: skipping quest file {p}: {e:#}"),
        }
    }
    if language != "en" {
        for def in &mut defs.list {
            if let Some(lines) = load_text_override(vfs, language, &def.path) {
                def.quest.texts = Texts::from_lines(lines);
            }
        }
    }
    defs
}

/// Every `conversations/*.cnv` of the VFS, localized.
pub fn dialogs(vfs: &Vfs, language: &str) -> Dialogs {
    let mut paths: Vec<&str> = vfs
        .paths()
        .filter(|p| p.starts_with("conversations/") && p.ends_with(".cnv"))
        .collect();
    paths.sort();
    let mut defs = HashMap::default();
    for path in paths {
        let conv = vfs
            .read(path)
            .and_then(|data| Conversation::parse(&data))
            .with_context(|| path.to_string());
        let mut conv = match conv {
            Ok(conv) => conv,
            Err(e) => {
                log::warn!("story: skipping conversation file {path}: {e:#}");
                continue;
            }
        };
        if language != "en"
            && let Some(lines) = load_text_override(vfs, language, path)
        {
            conv.texts = Texts::from_lines(lines);
        }
        defs.insert(normalize_path(path), Arc::new(conv));
    }
    Dialogs::new(defs)
}
