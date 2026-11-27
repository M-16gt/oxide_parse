//! Zero-copy HTML tokenizer. Not a spec state machine: it is lenient and built for speed.
use memchr::{memchr, memchr3, memmem};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token<'a> {
    /// `attrs` is the raw text between the tag name and `>` (parse it lazily with [`attrs`]).
    Start {
        name: &'a str,
        attrs: &'a str,
        self_closing: bool,
    },
    End {
        name: &'a str,
    },
    /// `decode`: whether character references should be decoded (false inside `<script>`/`<style>`).
    Text {
        text: &'a str,
        decode: bool,
    },
}

pub struct Tokenizer<'a> {
    src: &'a str,
    pos: usize,
    raw_until: Option<&'a str>,
    /// open `<svg>` / `<math>` elements: only there does `<x/>` really close itself (HTML5)
    foreign: usize,
    xml_mode: bool,
}

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

#[inline]
fn is_raw_text(name: &str) -> bool {
    match name.len() {
        5 => name.eq_ignore_ascii_case("style") || name.eq_ignore_ascii_case("title"),
        6 => name.eq_ignore_ascii_case("script"),
        8 => name.eq_ignore_ascii_case("textarea"),
        9 => name.eq_ignore_ascii_case("plaintext"),
        _ => false,
    }
}

#[inline]
fn is_foreign_name(name: &str) -> bool {
    (name.len() == 3 && name.eq_ignore_ascii_case("svg"))
        || (name.len() == 4 && name.eq_ignore_ascii_case("math"))
}

fn is_rcdata(name: &str) -> bool {
    name.eq_ignore_ascii_case("textarea") || name.eq_ignore_ascii_case("title")
}

/// Index of the `<` of the next `</name` (case-insensitive), or `b.len()`.
fn find_end_tag(b: &[u8], from: usize, name: &[u8]) -> usize {
    if name.eq_ignore_ascii_case(b"plaintext") {
        return b.len();
    }
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ScriptEscapeState {
        Normal,
        Escaped,
        DoubleEscaped,
    }
    let mut i = from;
    let is_script = name.eq_ignore_ascii_case(b"script");
    let mut state = ScriptEscapeState::Normal;
    while i < b.len() {
        // Jump with SIMD to the next byte that can change anything: in the normal state (and for every raw-text
        // element but `<script>`) only `<` starts a transition; in the escaped states `-->` starts with `-`.
        let hop = if is_script && state != ScriptEscapeState::Normal {
            memchr::memchr2(b'<', b'-', &b[i..])
        } else {
            memchr(b'<', &b[i..])
        };
        match hop {
            Some(off) => i += off,
            None => return b.len(),
        }
        if is_script {
            if state == ScriptEscapeState::Normal && b[i..].starts_with(b"<!--") {
                state = ScriptEscapeState::Escaped;
                i += 4;
                continue;
            }
            if state != ScriptEscapeState::Normal && b[i..].starts_with(b"-->") {
                state = ScriptEscapeState::Normal;
                i += 3;
                continue;
            }
            if state == ScriptEscapeState::Escaped
                && b[i] == b'<'
                && b.len() >= i + 7
                && b[i + 1..i + 7].eq_ignore_ascii_case(b"script")
                && b.get(i + 7)
                    .map_or(true, |c| is_ws(*c) || *c == b'/' || *c == b'>')
            {
                state = ScriptEscapeState::DoubleEscaped;
                i += 7;
                continue;
            }
        }
        let j = i;
        if b[j] == b'<' && b.get(j + 1) == Some(&b'/') {
            let k = j + 2;
            if b.len() >= k + name.len() && b[k..k + name.len()].eq_ignore_ascii_case(name) {
                match b.get(k + name.len()) {
                    Some(&c) if is_ws(c) || c == b'>' || c == b'/' => {
                        if is_script && state == ScriptEscapeState::DoubleEscaped {
                            state = ScriptEscapeState::Escaped;
                            i = k + name.len();
                            continue;
                        }
                        return j;
                    }
                    _ => {}
                }
            }
        }
        i = j + 1;
    }
    b.len()
}

impl<'a> Tokenizer<'a> {
    pub fn new(src: &'a str) -> Self {
        Tokenizer {
            src,
            pos: 0,
            raw_until: None,
            foreign: 0,
            xml_mode: false,
        }
    }

    pub fn new_xml(src: &'a str) -> Self {
        Tokenizer {
            src,
            pos: 0,
            raw_until: None,
            foreign: 0,
            xml_mode: true,
        }
    }

    pub fn next_token(&mut self) -> Option<Token<'a>> {
        loop {
            let b = self.src.as_bytes();
            let len = b.len();
            if self.pos >= len {
                return None;
            }
            if !self.xml_mode {
                if let Some(name) = self.raw_until.take() {
                    let end = find_end_tag(b, self.pos, name.as_bytes());
                    if end > self.pos {
                        let text = &self.src[self.pos..end];
                        self.pos = end;
                        return Some(Token::Text {
                            text,
                            decode: is_rcdata(name),
                        });
                    }
                }
            }
            if b[self.pos] == b'<' && self.pos + 1 < len {
                let c = b[self.pos + 1];
                if self.xml_mode && b[self.pos..].starts_with(b"<![CDATA[") {
                    let start = self.pos + 9;
                    let end = match memmem::find(&b[start..], b"]]>") {
                        Some(off) => {
                            self.pos = start + off + 3;
                            start + off
                        }
                        None => {
                            self.pos = len;
                            len
                        }
                    };
                    if end > start {
                        return Some(Token::Text {
                            text: &self.src[start..end],
                            decode: false,
                        });
                    }
                    continue;
                }
                let is_tag_start = c.is_ascii_alphabetic() || (self.xml_mode && c == b'_');
                let is_end_start = self.pos + 2 < len
                    && (b[self.pos + 2].is_ascii_alphabetic()
                        || (self.xml_mode && b[self.pos + 2] == b'_'));
                if c == b'/' && is_end_start {
                    return Some(self.end_tag());
                }
                if c == b'/' && self.pos + 2 < len {
                    self.pos = memchr(b'>', &b[self.pos + 2..])
                        .map(|o| self.pos + 2 + o + 1)
                        .unwrap_or(len);
                    continue;
                }
                if is_tag_start {
                    return Some(self.start_tag());
                }
                if c == b'!' || c == b'?' {
                    self.skip_declaration();
                    continue;
                }
            }
            return Some(self.text());
        }
    }

    fn text(&mut self) -> Token<'a> {
        let b = self.src.as_bytes();
        let start = self.pos;
        let mut i = start;
        if b[i] == b'<' {
            i += 1; // a literal '<' that does not start markup
        }
        loop {
            match memchr(b'<', &b[i..]) {
                None => {
                    i = b.len();
                    break;
                }
                Some(off) => {
                    let j = i + off;
                    let starts_markup = matches!(b.get(j + 1), Some(&c) if c.is_ascii_alphabetic() || c == b'/' || c == b'!' || c == b'?');
                    if starts_markup {
                        i = j;
                        break;
                    }
                    i = j + 1;
                }
            }
        }
        self.pos = i;
        Token::Text {
            text: &self.src[start..i],
            decode: true,
        }
    }

    fn skip_declaration(&mut self) {
        let b = self.src.as_bytes();
        let p = self.pos;
        if b[p..].starts_with(b"<!--") {
            let rest = &b[p + 4..];
            // `<!-->` and `<!--->` are complete (abruptly closed) comments in HTML5
            if rest.starts_with(b">") {
                self.pos = p + 5;
            } else if rest.starts_with(b"->") {
                self.pos = p + 6;
            } else {
                self.pos = match memmem::find(rest, b"-->") {
                    Some(o) => p + 4 + o + 3,
                    None => b.len(),
                };
            }
        } else if self.xml_mode && b[p..].starts_with(b"<?") {
            let rest = &b[p + 2..];
            self.pos = match memmem::find(rest, b"?>") {
                Some(o) => p + 2 + o + 2,
                None => b.len(),
            };
        } else {
            self.pos = match memchr(b'>', &b[p..]) {
                Some(o) => p + o + 1,
                None => b.len(),
            };
        }
    }

    fn start_tag(&mut self) -> Token<'a> {
        let b = self.src.as_bytes();
        let len = b.len();
        let name_start = self.pos + 1;
        let mut i = name_start;
        while i < len && !is_ws(b[i]) && b[i] != b'>' && b[i] != b'/' {
            i += 1;
        }
        let name = &self.src[name_start..i];
        let attrs_start = i;
        if i < len && b[i] != b'>' {
            let mut prev_sig = 0u8;
            loop {
                match memchr3(b'>', b'"', b'\'', &b[i..]) {
                    None => {
                        i = len;
                        break;
                    }
                    Some(off) => {
                        let hit = i + off;
                        let mut k = hit;
                        while k > i {
                            k -= 1;
                            if !is_ws(b[k]) {
                                prev_sig = b[k];
                                break;
                            }
                        }
                        let c = b[hit];
                        if c == b'>' {
                            i = hit;
                            break;
                        }
                        if prev_sig == b'=' {
                            match memchr(c, &b[hit + 1..]) {
                                Some(qoff) => {
                                    prev_sig = c;
                                    i = hit + 1 + qoff + 1;
                                }
                                None => {
                                    i = len;
                                    break;
                                }
                            }
                        } else {
                            prev_sig = c;
                            i = hit + 1;
                        }
                    }
                }
            }
        }
        let attrs = &self.src[attrs_start..i];
        self.pos = if i < len { i + 1 } else { len };
        let trimmed = attrs.trim_end();
        let self_closing = if self.xml_mode {
            trimmed.ends_with('/')
        } else {
            trimmed.ends_with('/')
                && (trimmed.len() == 1 || {
                    let before = trimmed.as_bytes()[trimmed.len() - 2];
                    is_ws(before) || before == b'"' || before == b'\''
                })
        };
        if !self.xml_mode {
            if !self_closing && is_foreign_name(name) {
                self.foreign += 1;
            }
            // In HTML the slash of `<script src=x />` is ignored: the element stays open and its content is raw
            // text up to `</script>` (the tree rules in events.rs agree). Only inside svg/math is it self-closing.
            if is_raw_text(name) && !(self_closing && self.foreign > 0) {
                self.raw_until = Some(name);
            }
        }
        Token::Start {
            name,
            attrs,
            self_closing,
        }
    }

    fn end_tag(&mut self) -> Token<'a> {
        let b = self.src.as_bytes();
        let name_start = self.pos + 2;
        let mut i = name_start;
        while i < b.len() && !is_ws(b[i]) && b[i] != b'>' && b[i] != b'/' {
            i += 1;
        }
        let name = &self.src[name_start..i];
        if self.foreign > 0 && is_foreign_name(name) {
            self.foreign -= 1;
        }
        if i < b.len() && b[i] == b'>' {
            self.pos = i + 1;
            return Token::End { name };
        }
        let mut quote = None;
        while i < b.len() {
            let c = b[i];
            i += 1;
            match quote {
                Some(q) if c == q => quote = None,
                Some(_) => {}
                None if c == b'\'' || c == b'"' => quote = Some(c),
                None if c == b'>' => break,
                None => {}
            }
        }
        self.pos = i;
        Token::End { name }
    }
}

/// Iterator over the attributes of a raw attribute string: `(name, raw value)`.
/// A valueless attribute yields `None`. Values are *not* entity-decoded.
pub struct Attrs<'a> {
    src: &'a str,
    i: usize,
}

pub fn attrs(raw: &str) -> Attrs<'_> {
    Attrs { src: raw, i: 0 }
}

impl<'a> Iterator for Attrs<'a> {
    type Item = (&'a str, Option<&'a str>);

    fn next(&mut self) -> Option<Self::Item> {
        let b = self.src.as_bytes();
        let len = b.len();
        loop {
            while self.i < len && (is_ws(b[self.i]) || b[self.i] == b'/') {
                self.i += 1;
            }
            if self.i >= len {
                return None;
            }
            let start = self.i;
            while self.i < len && !is_ws(b[self.i]) && b[self.i] != b'=' && b[self.i] != b'/' {
                self.i += 1;
            }
            if self.i == start {
                self.i += 1; // stray '=' or similar: skip it
                continue;
            }
            let name = &self.src[start..self.i];
            let mut j = self.i;
            while j < len && is_ws(b[j]) {
                j += 1;
            }
            if j < len && b[j] == b'=' {
                j += 1;
                while j < len && is_ws(b[j]) {
                    j += 1;
                }
                let value;
                if j < len && (b[j] == b'"' || b[j] == b'\'') {
                    let q = b[j];
                    let vs = j + 1;
                    let ve = memchr(q, &b[vs..]).map(|o| vs + o).unwrap_or(len);
                    value = &self.src[vs..ve];
                    self.i = if ve < len { ve + 1 } else { len };
                } else {
                    let vs = j;
                    let mut ve = vs;
                    while ve < len && !is_ws(b[ve]) {
                        ve += 1;
                    }
                    value = &self.src[vs..ve];
                    self.i = ve;
                }
                return Some((name, Some(value)));
            }
            return Some((name, None));
        }
    }
}

/// Raw value of attribute `key` (case-insensitive). A valueless attribute is `Some("")`.
pub fn find_attr<'a>(raw: &'a str, key: &str) -> Option<&'a str> {
    attrs(raw)
        .find(|(n, _)| n.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.unwrap_or(""))
}

#[cfg(test)]
mod find_end_tag_tests {
    use super::find_end_tag;

    fn last(s: &str, pat: &str) -> usize {
        s.rfind(pat).unwrap()
    }

    #[test]
    fn script_escape_states_with_memchr_jumps() {
        let s = "var a = 1 < 2; <!-- <script> </script> --> x </script>tail";
        assert_eq!(find_end_tag(s.as_bytes(), 0, b"script"), last(s, "</script>"));
        let plain = "if (a<b && c-->d) {} </SCRIPT >";
        assert_eq!(find_end_tag(plain.as_bytes(), 0, b"script"), last(plain, "</SCRIPT"));
        let none = "no end tag here <b> -->";
        assert_eq!(find_end_tag(none.as_bytes(), 0, b"script"), none.len());
    }

    #[test]
    fn style_and_title_only_look_for_their_end_tag() {
        let s = "a { b: '<!--' } </style><p>";
        assert_eq!(find_end_tag(s.as_bytes(), 0, b"style"), s.find("</style>").unwrap());
        assert_eq!(find_end_tag(b"x </ title> </title>", 0, b"title"), 12 + 0);
    }
}
