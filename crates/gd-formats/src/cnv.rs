//! Conversation files (`Conversations.arc`, `*.cnv`).
//!
//! Per Game.dll: `Conversation::Build(BinaryReader&)` 0x180178a20,
//! `ConversationStep::Read` 0x18017a100, `EstablishLinks` 0x180178f60,
//! `ConversationStep::IsAvailable` 0x18017a640, `GetTypeTag` 0x18017a470.
//!
//! Layout: `"CNV "`, u32 version (>=5; stock 6), u32 (skipped), u32 id, the
//! root step (recursive), then the text block ([`crate::qst::Texts`]).
//! Step: u32 nChildren, u32 type, u32 flags, [type 7: u32 link], [v>=6: str VO
//! sound], conditions, actions, children. Steps are numbered in read order
//! (root = 0); that number is the text index and the link target.
use crate::qst::{Action, Conditions, Texts, read_actions};
use crate::util::Reader;
use anyhow::{Context, Result, bail};

pub const MAGIC: u32 = 0x2056_4E43; // "CNV "

/// `ConversationStep::Type`. Names from `GetTypeTag` (2 Continue, 3 Accept,
/// 4 Decline, 6 EndConversation, 7 link → tag of the target) and the
/// conversation logic (1 = NPC speech node, tracked as "used" per player).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepType {
    Root,
    Speech,
    Continue,
    Accept,
    Decline,
    Other(u32),
    End,
    Link,
}

impl StepType {
    fn from_u32(v: u32) -> Self {
        match v {
            0 => Self::Root,
            1 => Self::Speech,
            2 => Self::Continue,
            3 => Self::Accept,
            4 => Self::Decline,
            6 => Self::End,
            7 => Self::Link,
            o => Self::Other(o),
        }
    }
    pub fn tag(self) -> &'static str {
        match self {
            Self::Continue | Self::Link => "tagContinue",
            Self::Accept => "tagAccept",
            Self::Decline => "tagDecline",
            Self::End => "tagEndConversation",
            _ => "",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// Read-order number: text index and link target.
    pub index: u32,
    /// Raw type as stored (type 2 with no children is promoted to End).
    pub raw_type: u32,
    pub kind: StepType,
    /// Bit 0: only available while unused (`IsAvailable`); bit 1 ("merge"):
    /// when an earlier sibling speech is shown, this one's actions run too
    /// and its responses are listed after the earlier one's
    /// (`Grim Dawn.exe` 0x14016f7c0, see gd-sim `story/dialog.rs`). No stock
    /// conversation sets bit 0 or bit 1; speeches use 4, 8, 16 and 32;
    /// bit 2: an unused available speech with it clear makes
    /// `Conversation::GetPunctuation` return 6 instead of 5
    /// (`HasAvailableSpeech`); the plain NPC marker ignores it.
    pub flags: u32,
    /// Type 7: index of the step this one continues to.
    pub link: Option<u32>,
    pub voice: String,
    pub conditions: Option<Conditions>,
    pub actions: Vec<Action>,
    pub children: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Conversation {
    pub version: u32,
    pub header_unknown: u32,
    pub id: u32,
    pub root: Step,
    pub texts: Texts,
    /// MD5 of the file: the uid stored next to the id in `conversations.gdd`.
    pub hash: [u8; 16],
}

/// Deepest step nesting accepted; each level is a native stack frame.
const MAX_DEPTH: u32 = 256;

fn read_step(r: &mut Reader, version: u32, counter: &mut u32, depth: u32) -> Result<Step> {
    if depth > MAX_DEPTH {
        bail!("conversation steps nested deeper than {MAX_DEPTH}");
    }
    let index = *counter;
    *counter += 1;
    let n = r.u32()?;
    let raw_type = r.u32()?;
    let ty = if raw_type == 2 && n == 0 { 6 } else { raw_type };
    let flags = r.u32()?;
    let link = if ty == 7 { Some(r.u32()?) } else { None };
    let voice = if version >= 6 {
        r.string()?
    } else {
        String::new()
    };
    let conditions = Conditions::read(r)?;
    let actions = read_actions(r)?;
    let mut children = Vec::with_capacity(r.capacity(n as usize, 12));
    for _ in 0..n {
        children.push(read_step(r, version, counter, depth + 1)?);
    }
    Ok(Step {
        index,
        raw_type,
        kind: StepType::from_u32(ty),
        flags,
        link,
        voice,
        conditions,
        actions,
        children,
    })
}

impl Conversation {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let magic = r.u32()?;
        if magic != MAGIC {
            bail!("not a CNV file (magic {magic:#x})");
        }
        let version = r.u32()?;
        if version < 5 {
            bail!("conversation version {version} < 5");
        }
        let header_unknown = r.u32()?;
        let id = r.u32()?;
        let mut counter = 0;
        let root = read_step(&mut r, version, &mut counter, 0).context("steps")?;
        let texts = Texts::read(&mut r).context("text block")?;
        if r.remaining() != 0 {
            bail!("{} trailing bytes", r.remaining());
        }
        Ok(Self {
            version,
            header_unknown,
            id,
            root,
            texts,
            hash: crate::util::md5(data),
        })
    }

    /// All steps in read order (index == position).
    pub fn steps(&self) -> Vec<&Step> {
        fn walk<'a>(s: &'a Step, out: &mut Vec<&'a Step>) {
            out.push(s);
            for c in &s.children {
                walk(c, out);
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &mut out);
        out
    }

    /// Text of a step; a link with empty text shows its target's text
    /// (`Conversation::GetText`).
    pub fn text(&self, step: &Step) -> &str {
        let t = self.texts.get(step.index);
        if t.is_empty()
            && let Some(l) = step.link
        {
            return self.texts.get(l);
        }
        t
    }
}
