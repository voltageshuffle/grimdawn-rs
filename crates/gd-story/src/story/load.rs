//! Loaders for every quest/conversation file of the layered install (VFS).
//!
//! * Quests: PORT. [`quest_defs`] loads every `quests/*.qst` of the VFS up
//!   front; the engine loads the current world's quest-file list
//!   (`Quest2Repository::LoadQuests` via `World::GetQuestFile`, the `.map`
//!   header; see [`QuestDefs::repository_order`]).
//! * Conversations: PORT. [`dialogs`] loads every `conversations/*.cnv` up
//!   front; the engine loads a character's conversation from its record's
//!   `conversation` field (Game.dll `Character::Load` reads it).
//! * Text: a non-English session replaces each file's embedded text block with
//!   the line-ordered `text_<code>/<basename>.txt` override
//!   ([`gd_formats::text::load_text_override`], `PortableLocalization::Read`).
//!   PORT: a missing override keeps the embedded English (the engine fails the
//!   load; the port loads every file up front).
use super::dialog::Dialogs;
use super::quest::{QuestDefs, normalize_path};
use anyhow::{Context, Result};
use gd_formats::cnv::Conversation;
use gd_formats::collections::HashMap;
use gd_formats::qst::Texts;
use gd_formats::text::load_text_override;
use gd_formats::vfs::Vfs;
use std::sync::Arc;

/// Every `quests/*.qst` of the VFS (later layers already override); a
/// non-English session's text overrides replace the embedded text blocks
/// (module doc).
pub fn quest_defs(vfs: &Vfs, language: &str) -> Result<QuestDefs> {
    let mut paths: Vec<&str> = vfs
        .paths()
        .filter(|p| p.starts_with("quests/") && p.ends_with(".qst"))
        .collect();
    paths.sort();
    let mut defs = QuestDefs::from_files(
        paths
            .into_iter()
            .map(|p| Ok::<_, anyhow::Error>((p.to_string(), vfs.read(p)?)))
            .collect::<Result<Vec<_>>>()?,
    )?;
    if language != "en" {
        for def in &mut defs.list {
            if let Some(lines) = load_text_override(vfs, language, &def.path) {
                def.quest.texts = Texts::from_lines(lines);
            }
        }
    }
    Ok(defs)
}

/// Every `conversations/*.cnv` of the VFS, localized before wrapping in
/// `Arc` (module doc).
pub fn dialogs(vfs: &Vfs, language: &str) -> Result<Dialogs> {
    let mut paths: Vec<&str> = vfs
        .paths()
        .filter(|p| p.starts_with("conversations/") && p.ends_with(".cnv"))
        .collect();
    paths.sort();
    let mut defs = HashMap::default();
    for path in paths {
        let mut conv = Conversation::parse(&vfs.read(path)?).with_context(|| path.to_string())?;
        if language != "en"
            && let Some(lines) = load_text_override(vfs, language, path)
        {
            conv.texts = Texts::from_lines(lines);
        }
        defs.insert(normalize_path(path), Arc::new(conv));
    }
    Ok(Dialogs::new(defs))
}
