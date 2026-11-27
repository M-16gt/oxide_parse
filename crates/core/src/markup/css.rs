//! A small CSS selector engine that works on any [`ElemView`] (arena DOM node or streaming stack frame).
//!
//! Supported: `tag`, `*`, `#id`, `.class`, `[attr]`, `[attr=v]` (`~= |= ^= $= *=`, `i`/`s` flags),
//! combinators ` ` `>` `+` `~`, lists `a, b`, and the pseudo-classes
//! `:first-child :last-child :only-child :first-of-type :last-of-type :only-of-type :empty :root :scope`,
//! `:nth-child :nth-last-child :nth-of-type :nth-last-of-type :not(...) :is(...) :where(...) :has(...) :contains(...)`,
//! `:any-link :checked :disabled :enabled`.
use super::entities;
use std::borrow::Cow;

/// What the matcher needs to know about an element.
pub trait ElemView: Sized + Clone {
    fn name(&self) -> &str;
    /// Decoded attribute value.
    fn attr(&self, key: &str) -> Option<Cow<'_, str>>;
    /// Parent element (the document node is not an element).
    fn parent(&self) -> Option<Self>;
    fn prev_sibling(&self) -> Option<Self>;
    /// 1-based position among the parent's element children.
    fn child_index(&self) -> usize;
    fn is_last_child(&self) -> bool;
    fn has_no_children(&self) -> bool;
    /// Element children in document order (needed by `:has`).
    fn children(&self, out: &mut Vec<Self>);
    /// Next element sibling (needed by `:has(+ x)` / `:has(~ x)`).
    fn next_sibling(&self) -> Option<Self>;
    /// Does the text of the element contain `needle`? (needed by `:contains`)
    fn contains_text(&self, needle: &str) -> bool;
    /// Is this the `:scope` element, i.e. the element a `select` started from (for a select over the whole
    /// document: the root element)? Engines that cannot say reject `:scope` selectors.
    fn is_scope(&self) -> bool {
        false
    }
    /// 1-based position from the end among the parent's element children.
    fn child_index_from_end(&self) -> usize {
        let mut n = 1;
        let mut cur = self.next_sibling();
        while let Some(s) = cur {
            n += 1;
            cur = s.next_sibling();
        }
        n
    }
    /// 1-based position among preceding element siblings of the same tag name.
    fn type_index(&self) -> usize {
        let me = self.name();
        let mut n = 1;
        let mut cur = self.prev_sibling();
        while let Some(s) = cur {
            if s.name().eq_ignore_ascii_case(me) {
                n += 1;
            }
            cur = s.prev_sibling();
        }
        n
    }
    /// 1-based position from the end among element siblings of the same tag name.
    fn type_index_from_end(&self) -> usize {
        let me = self.name();
        let mut n = 1;
        let mut cur = self.next_sibling();
        while let Some(s) = cur {
            if s.name().eq_ignore_ascii_case(me) {
                n += 1;
            }
            cur = s.next_sibling();
        }
        n
    }
    /// True if this element is a `<legend>` and has no preceding `<legend>` element siblings.
    fn is_first_legend_of_parent(&self) -> bool {
        if !self.name().eq_ignore_ascii_case("legend") {
            return false;
        }
        let mut cur = self.prev_sibling();
        while let Some(s) = cur {
            if s.name().eq_ignore_ascii_case("legend") {
                return false;
            }
            cur = s.prev_sibling();
        }
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Comb {
    Descendant,
    Child,
    Adjacent,
    General,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttrOp {
    Exists,
    Eq,
    Includes,
    Dash,
    Prefix,
    Suffix,
    Substr,
}

#[derive(Debug, Clone)]
struct AttrSel {
    name: String,
    op: AttrOp,
    value: String,
    ci: bool,
}

#[derive(Debug, Clone)]
enum Pseudo {
    FirstChild,
    LastChild,
    OnlyChild,
    FirstOfType,
    LastOfType,
    OnlyOfType,
    Empty,
    Root,
    /// `:scope`: the element the query started from (`T.css(":scope > .title")` inside a record)
    Scope,
    Nth(i32, i32),
    NthLastChild(i32, i32),
    NthOfType(i32, i32),
    NthLastOfType(i32, i32),
    Not(Vec<Complex>),
    Is(Vec<Complex>),
    /// `:has(> a, b + c, ...)`
    Has(Vec<Rel>),
    /// `:contains("text")` (case-sensitive substring of the element's text)
    Contains(String),
    AnyLink,
    Checked,
    Disabled,
    Enabled,
}

/// A relative selector of `:has`, left to right: how the first compound relates to the anchor, then the chain.
#[derive(Debug, Clone)]
struct Rel {
    lead: Comb,
    compounds: Vec<Compound>,
    /// `combs[k]` links `compounds[k]` to `compounds[k + 1]`
    combs: Vec<Comb>,
}

#[derive(Debug, Clone, Default)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<AttrSel>,
    pseudos: Vec<Pseudo>,
}

/// Parts are stored right-to-left: `parts[0]` is the rightmost compound; its `comb` links it to `parts[1]`.
#[derive(Debug, Clone)]
struct Complex {
    parts: Vec<Part>,
}

#[derive(Debug, Clone)]
struct Part {
    compound: Compound,
    comb: Comb,
}

/// A compiled selector list.
#[derive(Debug, Clone)]
pub struct Selector {
    alts: Vec<Complex>,
    src: String,
}

// ---------------------------------------------------------------------------------------------
// parsing
// ---------------------------------------------------------------------------------------------

/// `:not(:not(...))` nesting limit (see xpath::parser::MAX_DEPTH).
const MAX_NEST: usize = 32;

struct P<'s> {
    b: &'s [u8],
    s: &'s str,
    i: usize,
    depth: usize,
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'-' || c >= 0x80 || c == b'\\'
}

impl<'s> P<'s> {
    fn eof(&self) -> bool {
        self.i >= self.b.len()
    }
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }
    fn skip_ws(&mut self) -> bool {
        let start = self.i;
        loop {
            while matches!(self.peek(), Some(c) if c.is_ascii_whitespace()) {
                self.i += 1;
            }
            if self.b.get(self.i..self.i + 2) == Some(b"/*") {
                self.i += 2;
                while self.i < self.b.len() && self.b.get(self.i..self.i + 2) != Some(b"*/") {
                    self.i += 1;
                }
                if self.b.get(self.i..self.i + 2) == Some(b"*/") {
                    self.i += 2;
                }
                continue;
            }
            break;
        }
        self.i > start
    }
    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn err<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("{what} at position {} in {:?}", self.i, self.s))
    }

    fn ident(&mut self) -> Result<String, String> {
        let mut buf: Vec<u8> = Vec::new();
        loop {
            match self.peek() {
                Some(c) if c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c >= 0x80 => {
                    buf.push(c);
                    self.i += 1;
                }
                Some(b'\\') => {
                    self.i += 1;
                    match self.peek() {
                        Some(c) => {
                            buf.push(c);
                            self.i += 1;
                        }
                        None => return self.err("dangling escape"),
                    }
                }
                _ => break,
            }
        }
        if buf.is_empty() {
            return self.err("expected an identifier");
        }
        String::from_utf8(buf).map_err(|_| "invalid utf-8 in selector".to_string())
    }

    fn quoted(&mut self, q: u8) -> Result<String, String> {
        self.i += 1; // opening quote
        let mut buf: Vec<u8> = Vec::new();
        loop {
            match self.peek() {
                None => return self.err("unterminated string"),
                Some(c) if c == q => {
                    self.i += 1;
                    break;
                }
                Some(b'\\') => {
                    self.i += 1;
                    if let Some(c) = self.peek() {
                        buf.push(c);
                        self.i += 1;
                    }
                }
                Some(c) => {
                    buf.push(c);
                    self.i += 1;
                }
            }
        }
        String::from_utf8(buf).map_err(|_| "invalid utf-8 in selector".to_string())
    }

    fn attr(&mut self) -> Result<AttrSel, String> {
        self.skip_ws();
        let first = self.ident()?;
        let name = if self.peek() == Some(b'|') && self.b.get(self.i + 1) != Some(&b'=') {
            self.i += 1;
            let local = self.ident()?;
            format!("{first}:{local}").to_ascii_lowercase()
        } else if self.eat(b':') {
            let local = self.ident()?;
            format!("{first}:{local}").to_ascii_lowercase()
        } else {
            first.to_ascii_lowercase()
        };
        self.skip_ws();
        if self.eat(b']') {
            return Ok(AttrSel {
                name,
                op: AttrOp::Exists,
                value: String::new(),
                ci: false,
            });
        }
        let op = if self.eat(b'=') {
            AttrOp::Eq
        } else {
            let c = self.peek();
            self.i += 1;
            if !self.eat(b'=') {
                return self.err("expected '=' in attribute selector");
            }
            match c {
                Some(b'~') => AttrOp::Includes,
                Some(b'|') => AttrOp::Dash,
                Some(b'^') => AttrOp::Prefix,
                Some(b'$') => AttrOp::Suffix,
                Some(b'*') => AttrOp::Substr,
                _ => return self.err("unknown attribute operator"),
            }
        };
        self.skip_ws();
        let value = match self.peek() {
            Some(q) if q == b'"' || q == b'\'' => self.quoted(q)?,
            _ => {
                let start = self.i;
                while matches!(self.peek(), Some(c) if c != b']' && !c.is_ascii_whitespace()) {
                    self.i += 1;
                }
                self.s[start..self.i].to_string()
            }
        };
        self.skip_ws();
        let mut ci = false;
        if matches!(self.peek(), Some(b'i') | Some(b'I')) {
            ci = true;
            self.i += 1;
            self.skip_ws();
        } else if matches!(self.peek(), Some(b's') | Some(b'S')) {
            self.i += 1;
            self.skip_ws();
        }
        if !self.eat(b']') {
            return self.err("expected ']'");
        }
        Ok(AttrSel {
            name,
            op,
            value,
            ci,
        })
    }

    fn parse_nth_arg(&mut self, name: &str) -> Result<(i32, i32), String> {
        if !self.eat(b'(') {
            return self.err("expected '('");
        }
        let start = self.i;
        while matches!(self.peek(), Some(c) if c != b')') {
            self.i += 1;
        }
        let src = self.s;
        let arg = &src[start..self.i];
        if !self.eat(b')') {
            return self.err("expected ')'");
        }
        parse_nth(arg).ok_or_else(|| format!("bad :{name} argument {arg:?}"))
    }

    fn selector_list_arg(&mut self, name: &str) -> Result<Vec<Complex>, String> {
        if !self.eat(b'(') {
            return self.err("expected '('");
        }
        self.depth += 1;
        if self.depth > MAX_NEST {
            return Err(format!(":{name}(...) is nested too deeply"));
        }
        let mut list = Vec::new();
        loop {
            self.skip_ws();
            list.push(self.complex(true)?);
            self.skip_ws();
            if self.eat(b',') {
                continue;
            }
            if !self.eat(b')') {
                return self.err("expected ')'");
            }
            break;
        }
        self.depth -= 1;
        Ok(list)
    }

    fn pseudo(&mut self) -> Result<Pseudo, String> {
        if self.peek() == Some(b':') {
            return self.err("pseudo-elements (::x) are not supported");
        }
        let name = self.ident()?.to_ascii_lowercase();
        Ok(match name.as_str() {
            "first-child" => Pseudo::FirstChild,
            "last-child" => Pseudo::LastChild,
            "only-child" => Pseudo::OnlyChild,
            "first-of-type" => Pseudo::FirstOfType,
            "last-of-type" => Pseudo::LastOfType,
            "only-of-type" => Pseudo::OnlyOfType,
            "empty" => Pseudo::Empty,
            "root" => Pseudo::Root,
            "scope" => Pseudo::Scope,
            "any-link" => Pseudo::AnyLink,
            "checked" => Pseudo::Checked,
            "disabled" => Pseudo::Disabled,
            "enabled" => Pseudo::Enabled,
            "nth-child" => {
                let (a, b) = self.parse_nth_arg("nth-child")?;
                Pseudo::Nth(a, b)
            }
            "nth-last-child" => {
                let (a, b) = self.parse_nth_arg("nth-last-child")?;
                Pseudo::NthLastChild(a, b)
            }
            "nth-of-type" => {
                let (a, b) = self.parse_nth_arg("nth-of-type")?;
                Pseudo::NthOfType(a, b)
            }
            "nth-last-of-type" => {
                let (a, b) = self.parse_nth_arg("nth-last-of-type")?;
                Pseudo::NthLastOfType(a, b)
            }
            "not" => Pseudo::Not(self.selector_list_arg("not")?),
            "is" | "where" => Pseudo::Is(self.selector_list_arg(&name)?),
            "has" => {
                if !self.eat(b'(') {
                    return self.err("expected '('");
                }
                self.depth += 1;
                if self.depth > MAX_NEST {
                    return self.err(":has(...) is nested too deeply");
                }
                let mut rels = Vec::new();
                loop {
                    rels.push(self.relative()?);
                    self.skip_ws();
                    if self.eat(b',') {
                        continue;
                    }
                    if !self.eat(b')') {
                        return self.err("expected ')'");
                    }
                    break;
                }
                self.depth -= 1;
                Pseudo::Has(rels)
            }
            "contains" | "-soup-contains" => {
                if !self.eat(b'(') {
                    return self.err("expected '('");
                }
                self.skip_ws();
                let text = match self.peek() {
                    Some(q) if q == b'"' || q == b'\'' => self.quoted(q)?,
                    _ => {
                        let start = self.i;
                        while matches!(self.peek(), Some(c) if c != b')') {
                            self.i += 1;
                        }
                        self.s[start..self.i].trim().to_string()
                    }
                };
                self.skip_ws();
                if !self.eat(b')') {
                    return self.err("expected ')'");
                }
                Pseudo::Contains(text)
            }
            other => return Err(format!("unsupported pseudo-class :{other}")),
        })
    }

    fn compound(&mut self) -> Result<Compound, String> {
        let mut c = Compound::default();
        let mut any = false;
        if self.eat(b'*') {
            if self.eat(b'|') {
                if self.eat(b'*') {
                    // `* | *` matches any element
                } else if self.peek().map_or(false, is_ident_start) {
                    c.tag = Some(format!("*|{}", self.ident()?.to_ascii_lowercase()));
                } else {
                    return self.err("expected tag name or '*' after '*|'");
                }
            }
            any = true;
        } else if self.eat(b'|') {
            if self.eat(b'*') {
                c.tag = Some("|*".into());
            } else if self.peek().map_or(false, is_ident_start) {
                c.tag = Some(format!("|{}", self.ident()?.to_ascii_lowercase()));
            } else {
                return self.err("expected tag name after '|'");
            }
            any = true;
        } else if self.peek().map_or(false, is_ident_start) {
            let first = self.ident()?.to_ascii_lowercase();
            if self.eat(b'|') {
                if self.eat(b'*') {
                    c.tag = Some(format!("{first}:*"));
                } else if self.peek().map_or(false, is_ident_start) {
                    c.tag = Some(format!("{first}:{}", self.ident()?.to_ascii_lowercase()));
                } else {
                    return self.err("expected tag name or '*' after '|'");
                }
            } else {
                c.tag = Some(first);
            }
            any = true;
        }
        loop {
            match self.peek() {
                Some(b'#') => {
                    self.i += 1;
                    c.id = Some(self.ident()?);
                }
                Some(b'.') => {
                    self.i += 1;
                    c.classes.push(self.ident()?);
                }
                Some(b'[') => {
                    self.i += 1;
                    let a = self.attr()?;
                    c.attrs.push(a);
                }
                Some(b':') => {
                    self.i += 1;
                    let p = self.pseudo()?;
                    c.pseudos.push(p);
                }
                _ => break,
            }
            any = true;
        }
        if !any {
            return self.err("expected a selector");
        }
        Ok(c)
    }

    /// One relative selector inside `:has(...)`: optional leading combinator, then a chain of compounds.
    fn relative(&mut self) -> Result<Rel, String> {
        self.skip_ws();
        let lead = if self.eat(b'>') {
            Comb::Child
        } else if self.eat(b'+') {
            Comb::Adjacent
        } else if self.eat(b'~') {
            Comb::General
        } else {
            Comb::Descendant
        };
        self.skip_ws();
        let mut compounds = vec![self.compound()?];
        let mut combs: Vec<Comb> = Vec::new();
        loop {
            let ws = self.skip_ws();
            if matches!(self.peek(), None | Some(b',') | Some(b')')) {
                break;
            }
            let comb = if self.eat(b'>') {
                Comb::Child
            } else if self.eat(b'+') {
                Comb::Adjacent
            } else if self.eat(b'~') {
                Comb::General
            } else if ws {
                Comb::Descendant
            } else {
                return self.err("unexpected character");
            };
            self.skip_ws();
            combs.push(comb);
            compounds.push(self.compound()?);
        }
        Ok(Rel {
            lead,
            compounds,
            combs,
        })
    }

    fn complex(&mut self, in_parens: bool) -> Result<Complex, String> {
        let mut compounds = vec![self.compound()?];
        let mut combs: Vec<Comb> = Vec::new();
        loop {
            let ws = self.skip_ws();
            if matches!(self.peek(), None | Some(b',')) || (in_parens && self.peek() == Some(b')')) {
                break;
            }
            let comb = if self.eat(b'>') {
                Comb::Child
            } else if self.eat(b'+') {
                Comb::Adjacent
            } else if self.eat(b'~') {
                Comb::General
            } else if ws {
                Comb::Descendant
            } else {
                return self.err("unexpected character");
            };
            self.skip_ws();
            combs.push(comb);
            compounds.push(self.compound()?);
        }
        let n = compounds.len();
        let mut parts = Vec::with_capacity(n);
        for (k, compound) in compounds.into_iter().rev().enumerate() {
            let comb = if k + 1 < n {
                combs[n - 2 - k]
            } else {
                Comb::Descendant
            };
            parts.push(Part { compound, comb });
        }
        Ok(Complex { parts })
    }
}

/// `an+b`, `odd`, `even`, `n`, `3`
fn parse_nth(arg: &str) -> Option<(i32, i32)> {
    let t: String = arg
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    match t.as_str() {
        "odd" => return Some((2, 1)),
        "even" => return Some((2, 0)),
        _ => {}
    }
    if let Some(npos) = t.find('n') {
        let (a_s, rest) = (&t[..npos], &t[npos + 1..]);
        let a = match a_s {
            "" | "+" => 1,
            "-" => -1,
            s => s.parse::<i32>().ok()?,
        };
        let b = if rest.is_empty() {
            0
        } else {
            rest.parse::<i32>().ok()?
        };
        Some((a, b))
    } else {
        Some((0, t.parse::<i32>().ok()?))
    }
}

fn nth_matches(a: i32, b: i32, idx: usize) -> bool {
    // i64: `idx - b` and `d % a` cannot overflow for any i32 `a`/`b` (i32::MIN % -1 panics in i32)
    let Ok(idx) = i64::try_from(idx) else {
        return false;
    };
    let (a, b) = (i64::from(a), i64::from(b));
    if a == 0 {
        idx == b
    } else {
        let d = idx - b;
        d % a == 0 && d / a >= 0
    }
}

impl Selector {
    pub fn parse(src: &str) -> Result<Selector, String> {
        let mut p = P {
            b: src.as_bytes(),
            s: src,
            i: 0,
            depth: 0,
        };
        let mut alts = Vec::new();
        loop {
            p.skip_ws();
            alts.push(p.complex(false)?);
            p.skip_ws();
            if p.eat(b',') {
                continue;
            }
            if p.eof() {
                break;
            }
            return p.err("unexpected character");
        }
        Ok(Selector {
            alts,
            src: src.to_string(),
        })
    }

    pub fn source(&self) -> &str {
        &self.src
    }

    pub fn matches<E: ElemView>(&self, e: &E) -> bool {
        self.alts.iter().any(|c| match_complex(c, 0, e))
    }

    /// Cheap *necessary* condition, from the rightmost compound of each alternative, using only values the
    /// caller already has (tag name, raw `id` and `class` attribute). `false` => `matches` would be `false`.
    /// Lets a fused matcher skip most selectors without touching the ancestor chain or re-parsing attributes.
    pub fn may_match(&self, name: &str, id: Option<&str>, class: Option<&str>) -> bool {
        self.alts
            .iter()
            .any(|c| hint_ok(&c.parts[0].compound, name, id, class))
    }

    /// Does the selector use `:scope`?
    pub fn uses_scope(&self) -> bool {
        self.src.to_ascii_lowercase().contains(":scope")
    }

    /// Why this selector cannot run on a forward-only stream (`None` = it can).
    pub fn stream_unsupported(&self) -> Option<&'static str> {
        for c in &self.alts {
            if let Some(why) = complex_stream_unsupported(c) {
                return Some(why);
            }
        }
        None
    }

    /// True if matching this selector may call [`ElemView::type_index`] (`:first-of-type` / `:nth-of-type`).
    pub fn uses_type_index(&self) -> bool {
        self.alts.iter().any(complex_uses_type_index)
    }
}

fn tag_matches(want: &str, name: &str) -> bool {
    if let Some(local) = want.strip_prefix('|') {
        if local == "*" {
            return !name.contains(':');
        }
        return !name.contains(':') && name.eq_ignore_ascii_case(local);
    }
    if let Some(local) = want.strip_prefix("*|") {
        let actual = name.rsplit(':').next().unwrap_or(name);
        return actual.eq_ignore_ascii_case(local);
    }
    if let Some(pfx) = want.strip_suffix(":*") {
        return name
            .split_once(':')
            .map_or(false, |(p, _)| p.eq_ignore_ascii_case(pfx));
    }
    name.eq_ignore_ascii_case(want)
}

fn hint_ok(c: &Compound, name: &str, id: Option<&str>, class: Option<&str>) -> bool {
    if let Some(t) = &c.tag {
        if !tag_matches(t, name) {
            return false;
        }
    }
    if let Some(want) = &c.id {
        match id {
            None => return false,
            Some(v) if !v.contains('&') && v != want.as_str() => return false,
            _ => {}
        }
    }
    if !c.classes.is_empty() {
        match class {
            None => return false,
            Some(cls) if !cls.contains('&') => {
                for w in &c.classes {
                    if !cls.split_ascii_whitespace().any(|x| x == w.as_str()) {
                        return false;
                    }
                }
            }
            _ => {}
        }
    }
    true
}

fn complex_stream_unsupported(c: &Complex) -> Option<&'static str> {
    for p in &c.parts {
        if matches!(p.comb, Comb::Adjacent | Comb::General) {
            return Some("sibling combinators (+, ~)");
        }
        if let Some(why) = compound_stream_unsupported(&p.compound) {
            return Some(why);
        }
    }
    None
}

fn compound_stream_unsupported(c: &Compound) -> Option<&'static str> {
    for p in &c.pseudos {
        match p {
            Pseudo::LastChild
            | Pseudo::LastOfType
            | Pseudo::OnlyChild
            | Pseudo::OnlyOfType
            | Pseudo::NthLastChild(..)
            | Pseudo::NthLastOfType(..)
            | Pseudo::Empty
            | Pseudo::Has(_)
            | Pseudo::Contains(_) => {
                return Some(":has / :contains / :last-child / :only-child / :empty");
            }
            Pseudo::Not(list) | Pseudo::Is(list) => {
                for cx in list {
                    if let Some(why) = complex_stream_unsupported(cx) {
                        return Some(why);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn complex_uses_type_index(c: &Complex) -> bool {
    c.parts.iter().any(|p| compound_uses_type_index(&p.compound))
}

fn compound_uses_type_index(c: &Compound) -> bool {
    c.pseudos.iter().any(|p| match p {
        Pseudo::FirstOfType | Pseudo::NthOfType(..) => true,
        Pseudo::Not(list) | Pseudo::Is(list) => list.iter().any(complex_uses_type_index),
        Pseudo::Has(rels) => rels.iter().any(|r| r.compounds.iter().any(compound_uses_type_index)),
        _ => false,
    })
}

// ---------------------------------------------------------------------------------------------
// matching (right-to-left)
// ---------------------------------------------------------------------------------------------

fn match_complex<E: ElemView>(c: &Complex, k: usize, e: &E) -> bool {
    let part = &c.parts[k];
    if !match_compound(&part.compound, e) {
        return false;
    }
    if k + 1 == c.parts.len() {
        return true;
    }
    match part.comb {
        Comb::Child => e.parent().map_or(false, |p| match_complex(c, k + 1, &p)),
        Comb::Descendant => {
            let mut cur = e.parent();
            while let Some(p) = cur {
                if match_complex(c, k + 1, &p) {
                    return true;
                }
                cur = p.parent();
            }
            false
        }
        Comb::Adjacent => e
            .prev_sibling()
            .map_or(false, |s| match_complex(c, k + 1, &s)),
        Comb::General => {
            let mut cur = e.prev_sibling();
            while let Some(s) = cur {
                if match_complex(c, k + 1, &s) {
                    return true;
                }
                cur = s.prev_sibling();
            }
            false
        }
    }
}

/// The elements related to `e` by `comb` (children, descendants in document order, next sibling, later siblings).
fn relatives<E: ElemView>(e: &E, comb: Comb, out: &mut Vec<E>) {
    match comb {
        Comb::Child => e.children(out),
        Comb::Descendant => {
            let mut kids: Vec<E> = Vec::new();
            e.children(&mut kids);
            let mut stack: Vec<E> = kids.into_iter().rev().collect();
            while let Some(x) = stack.pop() {
                let mut ch: Vec<E> = Vec::new();
                x.children(&mut ch);
                out.push(x);
                stack.extend(ch.into_iter().rev());
            }
        }
        Comb::Adjacent => {
            if let Some(n) = e.next_sibling() {
                out.push(n);
            }
        }
        Comb::General => {
            let mut cur = e.next_sibling();
            while let Some(n) = cur {
                cur = n.next_sibling();
                out.push(n);
            }
        }
    }
}

/// Left-to-right matching of a `:has` chain starting at `cur` (which must match `rel.compounds[k]`).
fn match_fwd<E: ElemView>(rel: &Rel, k: usize, cur: &E) -> bool {
    if !match_compound(&rel.compounds[k], cur) {
        return false;
    }
    if k + 1 == rel.compounds.len() {
        return true;
    }
    let mut next: Vec<E> = Vec::new();
    relatives(cur, rel.combs[k], &mut next);
    next.iter().any(|n| match_fwd(rel, k + 1, n))
}

fn match_has<E: ElemView>(rel: &Rel, e: &E) -> bool {
    let mut first: Vec<E> = Vec::new();
    relatives(e, rel.lead, &mut first);
    first.iter().any(|c| match_fwd(rel, 0, c))
}

fn attr_matches(a: &AttrSel, v: &str) -> bool {
    let (v, want): (Cow<str>, Cow<str>) = if a.ci {
        (
            Cow::Owned(v.to_ascii_lowercase()),
            Cow::Owned(a.value.to_ascii_lowercase()),
        )
    } else {
        (Cow::Borrowed(v), Cow::Borrowed(a.value.as_str()))
    };
    let (v, want): (&str, &str) = (&v, &want);
    match a.op {
        AttrOp::Exists => true,
        AttrOp::Eq => v == want,
        AttrOp::Includes => v.split_ascii_whitespace().any(|x| x == want),
        AttrOp::Dash => {
            v == want || (v.starts_with(want) && v.as_bytes().get(want.len()) == Some(&b'-'))
        }
        AttrOp::Prefix => !want.is_empty() && v.starts_with(want),
        AttrOp::Suffix => !want.is_empty() && v.ends_with(want),
        AttrOp::Substr => !want.is_empty() && v.contains(want),
    }
}

fn is_disableable(name: &str) -> bool {
    name.eq_ignore_ascii_case("button")
        || name.eq_ignore_ascii_case("input")
        || name.eq_ignore_ascii_case("select")
        || name.eq_ignore_ascii_case("textarea")
        || name.eq_ignore_ascii_case("optgroup")
        || name.eq_ignore_ascii_case("option")
        || name.eq_ignore_ascii_case("fieldset")
}

/// `Some(true)` = `:disabled`, `Some(false)` = `:enabled`, `None` = element cannot be disabled/enabled.
fn elem_disabled<E: ElemView>(e: &E) -> Option<bool> {
    let name = e.name();
    if !is_disableable(name) {
        return None;
    }
    if e.attr("disabled").is_some() {
        return Some(true);
    }
    if name.eq_ignore_ascii_case("option") {
        let mut cur = e.parent();
        while let Some(p) = cur {
            if p.name().eq_ignore_ascii_case("optgroup") && p.attr("disabled").is_some() {
                return Some(true);
            }
            if p.name().eq_ignore_ascii_case("select") {
                break;
            }
            cur = p.parent();
        }
        return Some(false);
    }
    if name.eq_ignore_ascii_case("optgroup") || name.eq_ignore_ascii_case("fieldset") {
        return Some(false);
    }
    let mut child = e.clone();
    while let Some(p) = child.parent() {
        if p.name().eq_ignore_ascii_case("fieldset")
            && p.attr("disabled").is_some()
            && !child.is_first_legend_of_parent()
        {
            return Some(true);
        }
        child = p;
    }
    Some(false)
}

fn is_checked_elem<E: ElemView>(e: &E) -> bool {
    if e.name().eq_ignore_ascii_case("input") {
        e.attr("checked").is_some()
            && e.attr("type").map_or(false, |t| {
                t.eq_ignore_ascii_case("checkbox") || t.eq_ignore_ascii_case("radio")
            })
    } else if e.name().eq_ignore_ascii_case("option") {
        e.attr("selected").is_some()
    } else {
        false
    }
}

fn match_compound<E: ElemView>(c: &Compound, e: &E) -> bool {
    if let Some(t) = &c.tag {
        if !tag_matches(t, e.name()) {
            return false;
        }
    }
    if let Some(id) = &c.id {
        match e.attr("id") {
            Some(v) if &*v == id.as_str() => {}
            _ => return false,
        }
    }
    if !c.classes.is_empty() {
        let cls = match e.attr("class") {
            Some(v) => v,
            None => return false,
        };
        for want in &c.classes {
            if !cls.split_ascii_whitespace().any(|x| x == want.as_str()) {
                return false;
            }
        }
    }
    for a in &c.attrs {
        match e.attr(&a.name) {
            Some(v) => {
                if !attr_matches(a, &v) {
                    return false;
                }
            }
            None => return false,
        }
    }
    for p in &c.pseudos {
        let ok = match p {
            Pseudo::FirstChild => e.child_index() == 1,
            Pseudo::LastChild => e.is_last_child(),
            Pseudo::OnlyChild => e.child_index() == 1 && e.is_last_child(),
            Pseudo::FirstOfType => e.type_index() == 1,
            Pseudo::LastOfType => e.type_index_from_end() == 1,
            Pseudo::OnlyOfType => e.type_index() == 1 && e.type_index_from_end() == 1,
            Pseudo::Empty => e.has_no_children(),
            Pseudo::Root => e.parent().is_none(),
            Pseudo::Scope => e.is_scope(),
            Pseudo::Nth(a, b) => nth_matches(*a, *b, e.child_index()),
            Pseudo::NthLastChild(a, b) => nth_matches(*a, *b, e.child_index_from_end()),
            Pseudo::NthOfType(a, b) => nth_matches(*a, *b, e.type_index()),
            Pseudo::NthLastOfType(a, b) => nth_matches(*a, *b, e.type_index_from_end()),
            Pseudo::Not(list) => !list.iter().any(|cx| match_complex(cx, 0, e)),
            Pseudo::Is(list) => list.iter().any(|cx| match_complex(cx, 0, e)),
            Pseudo::Has(rels) => rels.iter().any(|r| match_has(r, e)),
            Pseudo::Contains(t) => e.contains_text(t),
            Pseudo::AnyLink => {
                (e.name().eq_ignore_ascii_case("a")
                    || e.name().eq_ignore_ascii_case("area")
                    || e.name().eq_ignore_ascii_case("link"))
                    && e.attr("href").is_some()
            }
            Pseudo::Checked => is_checked_elem(e),
            Pseudo::Disabled => elem_disabled(e) == Some(true),
            Pseudo::Enabled => elem_disabled(e) == Some(false),
        };
        if !ok {
            return false;
        }
    }
    true
}

/// Decode helper shared by element views.
pub fn decoded(raw: &str) -> Cow<'_, str> {
    entities::decode(raw)
}
