//! Grim Dawn's scripts are LuaJIT 2.0.4 (`x64/lua51.dll`) with a modified
//! lexer (`lex_scan` in lua51.dll):
//! `!` → `not`, `!=` → `~=`, `&&` → `and`, `||` → `or`, `//` line comments
//! (to `\r`/`\n`), `/* ... */` block comments. Everything else is Lua 5.1.
//! [`to_lua51`] rewrites those tokens outside strings/comments so a stock
//! Lua 5.1 compiles the game's sources; line numbers are preserved.

/// Rewrites Grim Dawn Lua into standard Lua 5.1.
pub fn to_lua51(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len() + src.len() / 16);
    let mut i = 0;
    // Copies b[start..end] (valid UTF-8 boundaries: all delimiters are ASCII).
    let push = |out: &mut String, s: usize, e: usize| out.push_str(&src[s..e]);
    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied().unwrap_or(0);
        match c {
            b'"' | b'\'' => {
                let s = i;
                i += 1;
                while i < b.len() && b[i] != c && b[i] != b'\n' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i = (i + 1).min(b.len());
                push(&mut out, s, i);
            }
            b'[' if long_bracket(b, i).is_some() => {
                let level = long_bracket(b, i).unwrap();
                let s = i;
                i = skip_long(b, i, level);
                push(&mut out, s, i);
            }
            b'-' if next == b'-' => {
                let s = i;
                if let Some(level) = long_bracket(b, i + 2) {
                    i = skip_long(b, i + 2, level);
                } else {
                    while i < b.len() && b[i] != b'\n' && b[i] != b'\r' {
                        i += 1;
                    }
                }
                push(&mut out, s, i);
            }
            b'/' if next == b'/' => {
                out.push_str("--");
                let s = i + 2;
                i = s;
                while i < b.len() && b[i] != b'\n' && b[i] != b'\r' {
                    i += 1;
                }
                // Keep the comment text but make sure it can't open a long comment.
                out.push(' ');
                push(&mut out, s, i);
            }
            b'/' if next == b'*' => {
                i += 2;
                out.push_str("  ");
                while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                    out.push(if b[i] == b'\n' || b[i] == b'\r' {
                        b[i] as char
                    } else {
                        ' '
                    });
                    i += 1;
                }
                i = (i + 2).min(b.len());
                out.push_str("  ");
            }
            b'!' if next == b'=' => {
                out.push_str("~=");
                i += 2;
            }
            b'!' => {
                out.push_str(" not ");
                i += 1;
            }
            b'&' if next == b'&' => {
                out.push_str(" and ");
                i += 2;
            }
            b'|' if next == b'|' => {
                out.push_str(" or ");
                i += 2;
            }
            _ => {
                let s = i;
                i += 1;
                while i < b.len()
                    && !matches!(b[i], b'"' | b'\'' | b'[' | b'-' | b'/' | b'!' | b'&' | b'|')
                {
                    i += 1;
                }
                push(&mut out, s, i);
            }
        }
    }
    out
}

/// `[[` / `[==[` at `i` → level.
fn long_bracket(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i) != Some(&b'[') {
        return None;
    }
    let mut j = i + 1;
    while b.get(j) == Some(&b'=') {
        j += 1;
    }
    (b.get(j) == Some(&b'[')).then_some(j - i - 1)
}

fn skip_long(b: &[u8], i: usize, level: usize) -> usize {
    let mut j = i + level + 2;
    while j < b.len() {
        if b[j] == b']'
            && b[j + 1..].iter().take(level).all(|&c| c == b'=')
            && b.get(j + 1 + level) == Some(&b']')
        {
            return j + level + 2;
        }
        j += 1;
    }
    b.len()
}
