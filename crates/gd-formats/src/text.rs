//! Localisation tag files (`Text_XX.arc` → `tags*.txt`), as loaded by
//! `LocalizationManager::LoadTags` (Engine.dll) and
//! `LocalizationManager::AddTagToMap`:
//! - only `.txt` files whose name starts with `tags` (case-insensitive) are
//!   loaded, except `tags_console.txt` (`IsConsoleTagsFilename`);
//! - optional UTF-8 BOM is skipped; text is UTF-8 (converted to UTF-16);
//! - per line: empty → skip, first char `#` → comment, split at the first
//!   `=` (no `=` → warning, skipped); key and value are not trimmed;
//! - a duplicate key is reported and the *first* definition is kept;
//! - every `{` and `}` is removed from the value (`{%s0}` → `%s0`).
//!
//! Inline markup is interpreted at draw time (`GraphicsFont2::ProcessCharacter`):
//! `^X` sets colour X (see [`color_code`]), `^^` is a literal
//! `^`, `^-` restores the previous colour; `&b`/`&i`/`&s` switch to bold /
//! italic / normal style, `&&` is a literal `&`.
//!
//! The language list ([`LANGUAGES`]) is the engine's (`LocalizationManager::Load`,
//! `GetLanguageName` / `GetLanguageTag`; Grim Dawn.exe `OptionsWindow::Load` builds the
//! options dropdown from it): the `language` option's stored string, the archive code
//! (`Text_<CODE>.arc`, mounted as `text_<code>/`) and the display name. Which languages
//! an install offers follows its mounted `Text_*.arc`, not this static list.
//!
//! Quest / conversation text and the language's paging settings live here too:
//! non-English quest / dialog text is a line-ordered override file
//! `text_<code>/<basename>.txt` of the `.qst` / `.cnv` source
//! ([`load_text_override`], Engine.dll `PortableLocalization::Read`, invoked by
//! `Quest2::Load` / `Conversation::Build`); expansion overrides take precedence
//! foa > fg > aom > root. `text_<code>/language.def` holds the language's
//! settings ([`LanguageDef`], `LocalizationManager::InitializeLanguage`).
use crate::collections::HashMap;
use crate::vfs::Vfs;

/// One language of the engine's list: the `language` option's stored string, the archive
/// code (`Text_<CODE>.arc` → `text_<code>/` in the VFS) and the dropdown's display name
/// (`LocalizationManager::Load` / `GetLanguageName`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Language {
    pub option: &'static str,
    pub code: &'static str,
    pub name: &'static str,
}

/// The engine's 13 languages in table order (`tagLanguage01`…`tagLanguage14`, 02 absent).
/// Availability follows the install's mounted `Text_*.arc`, not this list.
pub const LANGUAGES: &[Language] = &[
    Language {
        option: "English",
        code: "en",
        name: "English (US)",
    },
    Language {
        option: "Spanish",
        code: "es",
        name: "Spanish",
    },
    Language {
        option: "French",
        code: "fr",
        name: "French",
    },
    Language {
        option: "German",
        code: "de",
        name: "German",
    },
    Language {
        option: "Italian",
        code: "it",
        name: "Italian",
    },
    Language {
        option: "Czech",
        code: "cs",
        name: "Czech",
    },
    Language {
        option: "Japanese",
        code: "ja",
        name: "Japanese",
    },
    Language {
        option: "Korean",
        code: "ko",
        name: "Korean",
    },
    Language {
        option: "Polish",
        code: "pl",
        name: "Polish",
    },
    Language {
        option: "Portuguese",
        code: "pt",
        name: "Portuguese",
    },
    Language {
        option: "Russian",
        code: "ru",
        name: "Russian",
    },
    Language {
        option: "Vietnamese",
        code: "vi",
        name: "Vietnamese",
    },
    Language {
        option: "Chinese",
        code: "zh",
        name: "Chinese",
    },
];

/// The language's settings from `text_<code>/language.def`
/// (`LocalizationManager::InitializeLanguage`): keys match case-insensitively,
/// a non-empty `separators` replaces the default `.?!` wholesale (zh ships
/// `。` only; ja/ko ship none, so the default applies), `wordmode=false`
/// clears [`Self::word_mode`]. PORT: `word_mode` (CJK font word-wrap) is not
/// applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageDef {
    pub separators: String,
    pub word_mode: bool,
    pub fonts: String,
    pub locale: String,
}

impl Default for LanguageDef {
    fn default() -> Self {
        Self {
            separators: ".?!".to_owned(),
            word_mode: true,
            fonts: String::new(),
            locale: String::new(),
        }
    }
}

impl LanguageDef {
    /// Parse `language.def` bytes: per line, strip `\r`, split at the first
    /// `=` (no `=` → skipped), match the key case-insensitively; values are
    /// taken as-is (no trim).
    pub fn parse(data: &[u8]) -> Self {
        let mut def = Self::default();
        let text = String::from_utf8_lossy(data);
        for line in text.split('\n') {
            let line = line.strip_suffix('\r').unwrap_or(line);
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key.eq_ignore_ascii_case("separators") {
                if !value.is_empty() {
                    def.separators = value.to_owned();
                }
            } else if key.eq_ignore_ascii_case("wordmode") {
                def.word_mode = value != "false";
            } else if key.eq_ignore_ascii_case("fonts") {
                def.fonts = value.to_owned();
            } else if key.eq_ignore_ascii_case("locale") {
                def.locale = value.to_owned();
            }
        }
        def
    }

    /// Read `text_<code>/language.def` from the VFS; the default when absent
    /// (the English archive ships none).
    pub fn load(vfs: &Vfs, code: &str) -> Self {
        match vfs.read(&format!("text_{code}/language.def")) {
            Ok(data) => Self::parse(&data),
            Err(_) => Self::default(),
        }
    }
}

/// The quest / conversation text override of `source_path` in language
/// `code`: `text_<code>/<basename>.txt`, the basename between the last `/`
/// and last `.` of the `.qst` / `.cnv` source path. Line i ↔ text index i of
/// the embedded enUS table; UTF-8 (not the embedded table's latin1), empty
/// lines significant (kept, no trim, no BOM strip). Engine.dll
/// `PortableLocalization::Read` replaces the whole text table; expansion
/// overrides take precedence foa > fg > aom > root (first existing wins).
/// `None` when no override exists — PORT: the caller then keeps the embedded
/// English (the engine fails the load instead).
pub fn load_text_override(vfs: &Vfs, code: &str, source_path: &str) -> Option<Vec<String>> {
    let base = &source_path[source_path.rfind('/').map_or(0, |i| i + 1)..];
    let stem = &base[..base.rfind('.').unwrap_or(base.len())];
    let dir = format!("text_{code}");
    for candidate in [
        format!("{dir}/foa/{stem}.txt"),
        format!("{dir}/fg/{stem}.txt"),
        format!("{dir}/aom/{stem}.txt"),
        format!("{dir}/{stem}.txt"),
    ] {
        if let Ok(data) = vfs.read(&candidate) {
            return Some(
                String::from_utf8_lossy(&data)
                    .split('\n')
                    .map(|l| l.strip_suffix('\r').unwrap_or(l).to_owned())
                    .collect(),
            );
        }
    }
    None
}

#[derive(Debug, Clone, Default)]
pub struct TagTable {
    pub tags: HashMap<String, String>,
    /// Count of keys seen again after their first definition (first one wins).
    pub duplicates: usize,
    /// Count of non-comment lines without `=`.
    pub malformed: usize,
}

impl TagTable {
    /// Should a file with this (base) name be loaded as a tag file?
    /// Per `LoadTags`: the `txt` files of the localisation path whose
    /// name starts with `tags`, skipping `tags_console.txt`.
    pub fn is_tag_file(file_name: &str) -> bool {
        let base = file_name.rsplit('/').next().unwrap_or(file_name);
        base.len() >= 4
            && base.as_bytes()[..4].eq_ignore_ascii_case(b"tags")
            && base.to_ascii_lowercase().ends_with(".txt")
            && !base.eq_ignore_ascii_case("tags_console.txt")
    }

    /// Load order for tag files: newest expansion first (`tagsgdx3_*` …
    /// `tagsgdx1_*`, then the base `tags_*`), and earlier files win (the
    /// base game ships "?" placeholders for the expansion classes' names).
    /// `LoadTags` takes the files in `FileSystem::GetMatchingFiles`
    /// order (Engine.dll: each source in the order it was added,
    /// unsorted), and the game adds the sources as gdx3, gdx2, gdx1, base
    /// (`Grim Dawn.exe` `Game::Initialize`, `AddSource` appends). Within one layer
    /// the archive's entry order applies; the port sorts by name, which is
    /// equivalent as no base tag file repeats another's key.
    pub fn sort_files(paths: &mut [&str]) {
        let layer = |p: &str| {
            let base = p.rsplit('/').next().unwrap_or(p).to_ascii_lowercase();
            base.strip_prefix("tagsgdx")
                .and_then(|r| r.get(..1).and_then(|d| d.parse::<u32>().ok()))
                .unwrap_or(0)
        };
        paths.sort_by(|a, b| layer(b).cmp(&layer(a)).then(a.cmp(b)));
    }

    /// Add one file's contents (engine order: later files don't override).
    pub fn add_file(&mut self, bytes: &[u8]) {
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        let text = String::from_utf8_lossy(bytes);
        for line in text.split('\n') {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                self.malformed += 1;
                continue;
            };
            if self.tags.contains_key(k) {
                self.duplicates += 1;
                continue;
            }
            let v: String = v.chars().filter(|&c| c != '{' && c != '}').collect();
            self.tags.insert(k.to_string(), v);
        }
    }

    pub fn get(&self, tag: &str) -> Option<&str> {
        self.tags.get(tag).map(|s| s.as_str())
    }
}

/// Default RGBA for a `^X` colour code (case-insensitive), from the engine's
/// letter→index table (`Ui::ColorCodeIndices`) and colour table (`Ui::ColorCodeColors`).
/// Letters J, N, U, V map to no colour (`None`).
pub fn color_code(c: char) -> Option<[f32; 4]> {
    const IDX: [u8; 26] = [
        11, 0, 9, 14, 17, 8, 1, 18, 10, 22, 13, 16, 12, 22, 6, 5, 19, 2, 7, 15, 22, 22, 3, 20, 4,
        21,
    ];
    const COLORS: [[f32; 3]; 22] = [
        [0.224, 0.667, 0.808], // 0  ^b blue
        [0.063, 0.918, 0.365], // 1  ^g green
        [1.0, 0.258, 0.0],     // 2  ^r red
        [1.0, 1.0, 1.0],       // 3  ^w white
        [1.0, 0.871, 0.271],   // 4  ^y yellow
        [0.738, 0.579, 0.776], // 5  ^p purple
        [0.95, 0.64, 0.3],     // 6  ^o orange
        [0.6, 0.6, 0.6],       // 7  ^s silver/grey
        [1.0, 0.411, 0.705],   // 8  ^f fuchsia
        [0.0, 1.0, 1.0],       // 9  ^c cyan
        [0.65, 0.22, 1.0],     // 10 ^i indigo
        [0.5, 1.0, 0.831],     // 11 ^a aqua
        [0.5, 0.0, 0.0],       // 12 ^m maroon
        [0.941, 0.901, 0.549], // 13 ^k khaki
        [0.1, 0.1, 0.1],       // 14 ^d dark
        [0.0, 1.0, 0.82],      // 15 ^t teal
        [0.57, 0.797, 0.0],    // 16 ^l lime
        [0.592, 0.506, 0.373], // 17 ^e (tan; common in item text)
        [0.83, 0.8, 0.675],    // 18 ^h
        [0.761, 0.69, 0.777],  // 19 ^q
        [0.216, 0.369, 0.161], // 20 ^x
        [0.4, 0.6, 1.0],       // 21 ^z
    ];
    let u = c.to_ascii_uppercase();
    if !u.is_ascii_uppercase() {
        return None;
    }
    let i = IDX[(u as u8 - b'A') as usize] as usize;
    COLORS.get(i).map(|c| [c[0], c[1], c[2], 1.0])
}

/// Remove `^X` colour codes and `&b/&i/&s` style codes, unescaping `^^`/`&&`.
pub fn strip_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if (c == '^' || c == '&') && it.peek().is_some() {
            let n = *it.peek().unwrap();
            if n == c {
                out.push(c);
                it.next();
                continue;
            }
            let code = n.to_ascii_lowercase();
            let is_code = if c == '^' {
                code.is_ascii_alphabetic() || code == '-'
            } else {
                matches!(code, 'b' | 'i' | 's')
            };
            if is_code {
                it.next();
                continue;
            }
        }
        out.push(c);
    }
    out
}
