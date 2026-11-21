//! Per-world/per-difficulty state: `quests.gdd` and `conversations.gdd`
//! (`main/_<name>/levels_world001.map/<Difficulty>/`).
//!
//! Quests — Game.dll `sub_1803FBBE0` (file body), `Quest2Repository::
//! WriteProperties` (0x1804238c0), `Quest2::WriteProperties` (0x180420fe0),
//! `Quest2Task::WriteProperties` (0x180426370). Conversations — writer
//! `sub_1803FC800`, reader `sub_1803FC3C0`, payload `sub_18017B780`.
//!
//! Quest task payloads contain the task's objective/condition state, written by
//! virtual `WriteProperties` of objects created from the quest definitions
//! (`.qst`). Those bytes are kept as `conditions` — decrypted with u32
//! granularity (+ trailing bytes). The cipher key only depends on ciphertext
//! bytes, so this re-encrypts exactly regardless of the real field widths.
use super::cipher::{Decoder, Encoder};
use super::gdc::Uid;
use anyhow::{Result, bail};

pub const QUEST_MAGIC: u32 = 0x5854_5351; // "QSTX"
pub const CONVERSATION_MAGIC: u32 = 0x5856_4E43; // "CNVX"

#[derive(Debug, Clone, PartialEq)]
pub struct QuestTask {
    pub id: u32,
    pub uid: Uid,
    pub block_id: u32,
    /// Task state for this difficulty.
    pub state: u32,
    pub in_progress: u8,
    /// Opaque objective/condition state, see module docs.
    pub conditions: Vec<u8>,
    pub complete: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Quest {
    pub id: u32,
    pub uid: Uid,
    pub block_id: u32,
    pub tasks: Vec<QuestTask>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Quests {
    pub seed: u32,
    pub version: u32,
    pub player_uid: Uid,
    pub tokens_version: u32,
    pub tokens: Vec<String>,
    pub repository_version: u32,
    pub quests: Vec<Quest>,
}

fn read_opaque(d: &mut Decoder, n: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n / 4 {
        out.extend_from_slice(&d.u32()?.to_le_bytes());
    }
    for _ in 0..n % 4 {
        out.push(d.u8()?);
    }
    Ok(out)
}

fn write_opaque(e: &mut Encoder, b: &[u8]) {
    let (words, rest) = b.as_chunks::<4>();
    for w in words {
        e.u32(u32::from_le_bytes(*w));
    }
    for &x in rest {
        e.u8(x);
    }
}

fn read_magic(d: &mut Decoder, magic: u32, what: &str) -> Result<(u32, Uid)> {
    let m = d.u32()?;
    if m != magic {
        bail!("not a {what} file (magic {m:#x})");
    }
    let version = d.u32()?;
    if version != 0 {
        bail!("{what} version {version} not supported");
    }
    Ok((version, d.uid()?))
}

impl Quests {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(data)?;
        let (version, player_uid) = read_magic(&mut d, QUEST_MAGIC, "quests.gdd")?;

        let (_, end) = d.begin_block(Some(10))?;
        let tokens_version = d.u32()?;
        if tokens_version != 2 {
            bail!("quest token block version {tokens_version} not supported");
        }
        let n = d.u32()?;
        let tokens = (0..n).map(|_| d.string()).collect::<Result<Vec<_>>>()?;
        d.end_block(10, end)?;

        let (_, end) = d.begin_block(Some(11))?;
        let repository_version = d.u32()?;
        if repository_version != 4 {
            bail!("quest repository version {repository_version} not supported");
        }
        let nq = d.u32()?;
        let mut quests = Vec::new();
        for _ in 0..nq {
            let id = d.u32()?;
            let uid = d.uid()?;
            let (block_id, qend) = d.begin_block(None)?;
            let nt = d.u32()?;
            let mut tasks = Vec::new();
            for _ in 0..nt {
                let id = d.u32()?;
                let uid = d.uid()?;
                let (tblock, tend) = d.begin_block(None)?;
                let state = d.u32()?;
                let in_progress = d.u8()?;
                let Some(n) = tend.checked_sub(d.pos + 1) else {
                    bail!("quest task block too short")
                };
                let conditions = read_opaque(&mut d, n)?;
                let complete = d.u8()?;
                d.end_block(tblock, tend)?;
                tasks.push(QuestTask {
                    id,
                    uid,
                    block_id: tblock,
                    state,
                    in_progress,
                    conditions,
                    complete,
                });
            }
            d.end_block(block_id, qend)?;
            quests.push(Quest {
                id,
                uid,
                block_id,
                tasks,
            });
        }
        d.end_block(11, end)?;
        if !d.at_end() {
            bail!("{} trailing bytes", d.len() - d.pos);
        }
        Ok(Self {
            seed: d.seed,
            version,
            player_uid,
            tokens_version,
            tokens,
            repository_version,
            quests,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut e = Encoder::new(self.seed);
        e.u32(QUEST_MAGIC);
        e.u32(self.version);
        e.uid(&self.player_uid);
        e.begin_block(10);
        e.u32(self.tokens_version);
        e.u32(self.tokens.len() as u32);
        for t in &self.tokens {
            e.string(t)?;
        }
        e.end_block()?;
        e.begin_block(11);
        e.u32(self.repository_version);
        e.u32(self.quests.len() as u32);
        for q in &self.quests {
            e.u32(q.id);
            e.uid(&q.uid);
            e.begin_block(q.block_id);
            e.u32(q.tasks.len() as u32);
            for t in &q.tasks {
                e.u32(t.id);
                e.uid(&t.uid);
                e.begin_block(t.block_id);
                e.u32(t.state);
                e.u8(t.in_progress);
                write_opaque(&mut e, &t.conditions);
                e.u8(t.complete);
                e.end_block()?;
            }
            e.end_block()?;
        }
        e.end_block()?;
        e.finish()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Conversation {
    pub id: u32,
    pub uid: Uid,
    /// Localization (= read-order) indices of the speech steps shown, in
    /// order. The exe's dialog code calls
    /// `ConversationStore::AddSpeechToConversations(id,
    /// ConversationStep::GetLocalizationIndex)` for an unused speech.
    pub values: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Conversations {
    pub seed: u32,
    pub version: u32,
    pub player_uid: Uid,
    pub block_version: u32,
    pub conversations: Vec<Conversation>,
}

impl Conversations {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(data)?;
        let (version, player_uid) = read_magic(&mut d, CONVERSATION_MAGIC, "conversations.gdd")?;
        let (_, end) = d.begin_block(Some(9))?;
        let block_version = d.u32()?;
        if block_version != 1 {
            bail!("conversation block version {block_version} not supported");
        }
        let n = d.u32()?;
        let mut conversations = Vec::new();
        for _ in 0..n {
            let id = d.u32()?;
            let uid = d.uid()?;
            let m = d.u32()?;
            let values = (0..m).map(|_| d.u16()).collect::<Result<Vec<_>>>()?;
            conversations.push(Conversation { id, uid, values });
        }
        d.end_block(9, end)?;
        if !d.at_end() {
            bail!("{} trailing bytes", d.len() - d.pos);
        }
        Ok(Self {
            seed: d.seed,
            version,
            player_uid,
            block_version,
            conversations,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut e = Encoder::new(self.seed);
        e.u32(CONVERSATION_MAGIC);
        e.u32(self.version);
        e.uid(&self.player_uid);
        e.begin_block(9);
        e.u32(self.block_version);
        e.u32(self.conversations.len() as u32);
        for c in &self.conversations {
            e.u32(c.id);
            e.uid(&c.uid);
            e.u32(c.values.len() as u32);
            for &v in &c.values {
                e.u16(v);
            }
        }
        e.end_block()?;
        e.finish()
    }
}
