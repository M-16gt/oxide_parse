//! Tree-construction rules on top of the tokenizer: void elements, implied end tags, scoped end tags.
//! Both engines consume this one event stream, so they always agree on the document structure.
//!
//! This follows the *common* parts of the HTML5 tree builder (it is not the full algorithm: no foster
//! parenting, no adoption agency for misnested formatting elements).
use super::tokenizer::{Token, Tokenizer};
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event<'a> {
    Open { name: &'a str, attrs: &'a str },
    Close { name: &'a str },
    Text { text: &'a str, decode: bool },
}

// ---- tag classification ---------------------------------------------------------------------

const F_CLOSES_P: u32 = 1;
const F_LI: u32 = 1 << 1;
const F_DT_DD: u32 = 1 << 2;
const F_TR: u32 = 1 << 3;
const F_TD_TH: u32 = 1 << 4;
const F_TSECTION: u32 = 1 << 5;
const F_OPTION: u32 = 1 << 6;
const F_OPTGROUP: u32 = 1 << 7;
const F_HEADING: u32 = 1 << 8;
const F_VOID: u32 = 1 << 9;
/// "special" elements of the HTML5 spec: a stray end tag never reaches past them
const F_SPECIAL: u32 = 1 << 10;
const F_FOREIGN: u32 = 1 << 11;
const F_TCAP_COLG: u32 = 1 << 12;
const F_COL: u32 = 1 << 13;

/// One lookup per tag instead of dozens of string comparisons.
fn classify(name: &str) -> u32 {
    let b = name.as_bytes();
    if b.is_empty() || b.len() > 12 {
        return 0;
    }
    let mut buf = [0u8; 12];
    for (i, c) in b.iter().enumerate() {
        buf[i] = c.to_ascii_lowercase();
    }
    let s = match std::str::from_utf8(&buf[..b.len()]) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    match s {
        "dd" | "dt" => F_CLOSES_P | F_SPECIAL | F_DT_DD,
        "li" => F_CLOSES_P | F_SPECIAL | F_LI,
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => F_CLOSES_P | F_SPECIAL | F_HEADING,
        "hr" => F_CLOSES_P | F_SPECIAL | F_VOID,
        "address" | "article" | "aside" | "blockquote" | "center" | "details" | "dialog"
        | "dir" | "div" | "dl" | "fieldset" | "figcaption" | "figure" | "footer" | "form"
        | "header" | "hgroup" | "listing" | "main" | "menu" | "nav" | "ol" | "p" | "plaintext"
        | "pre" | "section" | "summary" | "table" | "ul" | "xmp" => F_CLOSES_P | F_SPECIAL,
        "col" => F_VOID | F_SPECIAL | F_COL,
        "area" | "base" | "basefont" | "bgsound" | "br" | "embed" | "frame" | "img" | "input"
        | "keygen" | "link" | "meta" | "param" | "source" | "track" | "wbr" => F_VOID | F_SPECIAL,
        "tr" => F_SPECIAL | F_TR,
        "td" | "th" => F_SPECIAL | F_TD_TH,
        "thead" | "tbody" | "tfoot" => F_SPECIAL | F_TSECTION,
        "caption" | "colgroup" => F_SPECIAL | F_TCAP_COLG,
        "option" => F_OPTION,
        "optgroup" => F_OPTGROUP,
        "svg" | "math" => F_FOREIGN,
        "applet" | "body" | "button" | "frameset" | "head" | "html" | "iframe" | "marquee"
        | "noembed" | "noframes" | "noscript" | "object" | "script" | "select" | "style"
        | "template" | "textarea" | "title" => F_SPECIAL,
        _ => 0,
    }
}

pub fn is_void(name: &str) -> bool {
    classify(name) & F_VOID != 0
}

const HEADINGS: &[&str] = &["h1", "h2", "h3", "h4", "h5", "h6"];
/// default "has an element in scope" boundaries
const SCOPE: &[&str] = &[
    "applet", "caption", "html", "table", "td", "th", "marquee", "object", "template",
];
/// "button scope" (used for `p`)
const P_SCOPE: &[&str] = &[
    "applet", "caption", "html", "table", "td", "th", "marquee", "object", "template", "button",
];
const LI_SCOPE: &[&str] = &[
    "applet", "caption", "html", "table", "td", "th", "marquee", "object", "template", "ol", "ul",
];
const TABLE_SCOPE: &[&str] = &["html", "table", "template"];

fn is_one_of(name: &str, set: &[&str]) -> bool {
    set.iter().any(|s| name.eq_ignore_ascii_case(s))
}

/// An open element with its tag flags, classified once when it is opened (the end-tag walk used to classify
/// every stack entry again).
#[derive(Clone, Copy)]
struct OpenElem<'a> {
    name: &'a str,
    flags: u32,
}

pub struct Events<'a> {
    tok: Tokenizer<'a>,
    stack: Vec<OpenElem<'a>>,
    queue: VecDeque<Event<'a>>,
    /// number of open `<p>` elements: lets the (very common) `<div>` start tag skip the scope search
    open_p: usize,
    /// number of open `<svg>` / `<math>` elements: `in_foreign()` is a comparison instead of a stack scan
    open_foreign: usize,
    /// true until the first element (other than `<html>`) or non-whitespace text is seen
    before_content: bool,
    /// true once a `<frameset>` start tag has been seen in the document
    frameset_mode: bool,
    /// true right after `<pre>`, `<textarea>` or `<listing>` start tag (strips one leading newline)
    strip_leading_nl: bool,
}

impl<'a> Events<'a> {
    pub fn new(src: &'a str) -> Self {
        Events {
            tok: Tokenizer::new(src),
            stack: Vec::new(),
            queue: VecDeque::new(),
            open_p: 0,
            open_foreign: 0,
            before_content: true,
            frameset_mode: false,
            strip_leading_nl: false,
        }
    }

    fn push_open(&mut self, name: &'a str, flags: u32) {
        if name.eq_ignore_ascii_case("p") {
            self.open_p += 1;
        }
        if flags & F_FOREIGN != 0 {
            self.open_foreign += 1;
        }
        self.stack.push(OpenElem { name, flags });
    }

    /// Position of the nearest open element in `targets`, searching top-down and stopping at a barrier.
    fn find_open(&self, targets: &[&str], barriers: &[&str]) -> Option<usize> {
        for (i, el) in self.stack.iter().enumerate().rev() {
            if is_one_of(el.name, targets) {
                return Some(i);
            }
            if is_one_of(el.name, barriers) {
                return None;
            }
        }
        None
    }

    /// Close every open element from the top down to (and including) stack index `idx`.
    fn close_to(&mut self, idx: usize) {
        while self.stack.len() > idx {
            if let Some(el) = self.stack.pop() {
                if el.name.eq_ignore_ascii_case("p") {
                    self.open_p = self.open_p.saturating_sub(1);
                }
                if el.flags & F_FOREIGN != 0 {
                    self.open_foreign = self.open_foreign.saturating_sub(1);
                }
                self.queue.push_back(Event::Close { name: el.name });
            }
        }
    }

    fn close_all_in_scope(&mut self, targets: &[&str], barriers: &[&str]) {
        while let Some(i) = self.find_open(targets, barriers) {
            self.close_to(i);
        }
    }

    fn in_foreign(&self) -> bool {
        self.open_foreign > 0
    }

    fn implied_closes(&mut self, f: u32) {
        if f & F_CLOSES_P != 0 && self.open_p > 0 {
            if let Some(i) = self.find_open(&["p"], P_SCOPE) {
                self.close_to(i);
            }
        }
        if f & F_LI != 0 {
            if let Some(i) = self.find_open(&["li"], &["ul", "ol", "menu"]) {
                self.close_to(i);
            }
        } else if f & F_DT_DD != 0 {
            if let Some(i) = self.find_open(&["dt", "dd"], &["dl"]) {
                self.close_to(i);
            }
        } else if f & F_TR != 0 {
            self.close_all_in_scope(
                &["tr", "td", "th", "colgroup", "caption"],
                &["table", "thead", "tbody", "tfoot", "template", "html"],
            );
        } else if f & F_TD_TH != 0 {
            self.close_all_in_scope(
                &["td", "th", "colgroup", "caption"],
                &["tr", "table", "tbody", "thead", "tfoot", "template", "html"],
            );
        } else if f & (F_TSECTION | F_TCAP_COLG) != 0 {
            self.close_all_in_scope(
                &[
                    "td", "th", "tr", "tbody", "thead", "tfoot", "colgroup", "caption",
                ],
                &["table", "template", "html"],
            );
        } else if f & F_COL != 0 {
            self.close_all_in_scope(
                &["td", "th", "tr", "tbody", "thead", "tfoot", "caption"],
                &["colgroup", "table", "template", "html"],
            );
        } else if f & F_OPTION != 0 {
            if let Some(i) = self.find_open(&["option"], &["select", "optgroup"]) {
                self.close_to(i);
            }
        } else if f & F_OPTGROUP != 0 {
            if let Some(i) = self.find_open(&["option"], &["select", "optgroup"]) {
                self.close_to(i);
            }
            if let Some(i) = self.find_open(&["optgroup"], &["select"]) {
                self.close_to(i);
            }
        }
        if f & F_HEADING != 0 {
            if let Some(top) = self.stack.last() {
                if is_one_of(top.name, HEADINGS) {
                    let i = self.stack.len() - 1;
                    self.close_to(i);
                }
            }
        }
    }

    /// End tags, following the spec's scoping rules: they close the matching element only if no
    /// "special" element (or scope boundary) sits in between; otherwise they are ignored.
    fn end_tag(&mut self, name: &str) {
        if name.eq_ignore_ascii_case("body") || name.eq_ignore_ascii_case("html") {
            return; // the spec keeps them open until the end of the document
        }
        if name.eq_ignore_ascii_case("br") {
            self.before_content = false;
            self.queue.push_back(Event::Open {
                name: "br",
                attrs: "",
            });
            self.queue.push_back(Event::Close { name: "br" });
            return;
        }
        // In the head insertion mode a stray </p> is ignored, rather than
        // synthesizing the body-mode empty paragraph inside <head>.
        if name.eq_ignore_ascii_case("p")
            && self.find_open(&["head"], &["body", "template"]).is_some()
        {
            return;
        }
        if let Some(top) = self.stack.last() {
            if top.name.eq_ignore_ascii_case(name) {
                let i = self.stack.len() - 1;
                self.close_to(i);
                return;
            }
        }
        let f = classify(name);
        let target = if f & F_HEADING != 0 {
            self.find_open(HEADINGS, SCOPE)
        } else if name.eq_ignore_ascii_case("p") {
            if let Some(i) = self.find_open(&["p"], P_SCOPE) {
                Some(i)
            } else {
                self.before_content = false;
                self.queue.push_back(Event::Open {
                    name: "p",
                    attrs: "",
                });
                self.queue.push_back(Event::Close { name: "p" });
                None
            }
        } else if f & F_LI != 0 {
            self.find_open(&["li"], LI_SCOPE)
        } else if f & F_DT_DD != 0 {
            self.find_open(&[name], SCOPE)
        } else if f & (F_TR | F_TD_TH | F_TSECTION | F_TCAP_COLG) != 0
            || name.eq_ignore_ascii_case("table")
        {
            self.find_open(&[name], TABLE_SCOPE)
        } else if f & F_CLOSES_P != 0 {
            // `</div>`, `</ul>`, `</section>`, ...: closes the element if it is *in scope* (everything above it,
            // e.g. an unclosed <li>, is popped too); a table cell / caption / template boundary stops the search
            self.find_open(&[name], SCOPE)
        } else {
            // generic end tag: walk down the stack, a "special" element blocks it
            let mut found = None;
            for (i, el) in self.stack.iter().enumerate().rev() {
                if el.name.eq_ignore_ascii_case(name) {
                    found = Some(i);
                    break;
                }
                if el.flags & F_SPECIAL != 0 {
                    break;
                }
            }
            found
        };
        if let Some(i) = target {
            self.close_to(i);
        }
    }
}

impl<'a> Iterator for Events<'a> {
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Event<'a>> {
        loop {
            if let Some(e) = self.queue.pop_front() {
                return Some(e);
            }
            match self.tok.next_token() {
                None => {
                    // end of input: close everything still open
                    let el = self.stack.pop()?;
                    return Some(Event::Close { name: el.name });
                }
                Some(Token::Text { mut text, decode }) => {
                    if self.strip_leading_nl {
                        self.strip_leading_nl = false;
                        text = text
                            .strip_prefix("\r\n")
                            .or_else(|| text.strip_prefix('\n'))
                            .unwrap_or(text);
                        if text.is_empty() {
                            continue;
                        }
                    }
                    if self.before_content {
                        if text
                            .bytes()
                            .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0c))
                        {
                            continue;
                        }
                        self.before_content = false;
                    }
                    if self.frameset_mode && self.find_open(&["noframes"], &[]).is_none() {
                        let bytes = text.as_bytes();
                        let mut i = 0;
                        while i < bytes.len() {
                            while i < bytes.len()
                                && !matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
                            {
                                i += 1;
                            }
                            let start = i;
                            while i < bytes.len()
                                && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
                            {
                                i += 1;
                            }
                            if start < i {
                                self.queue.push_back(Event::Text {
                                    text: &text[start..i],
                                    decode,
                                });
                            }
                        }
                        continue;
                    }
                    return Some(Event::Text { text, decode });
                }
                Some(Token::Start {
                    name,
                    attrs,
                    self_closing,
                }) => {
                    self.strip_leading_nl = false;
                    let name = if name.eq_ignore_ascii_case("image") {
                        "img"
                    } else {
                        name
                    };
                    if name.eq_ignore_ascii_case("frame")
                        && self.find_open(&["frameset"], &[]).is_none()
                    {
                        continue;
                    }
                    if name.eq_ignore_ascii_case("frameset") {
                        self.frameset_mode = true;
                    }
                    if !name.eq_ignore_ascii_case("html") {
                        self.before_content = false;
                    }
                    if name.eq_ignore_ascii_case("select") {
                        if let Some(i) = self.find_open(&["select"], &[]) {
                            self.close_to(i);
                            continue;
                        }
                    }
                    let f = classify(name);
                    if f & (F_TR | F_TD_TH | F_TSECTION | F_TCAP_COLG | F_COL) != 0
                        && self.find_open(&["table"], TABLE_SCOPE).is_none()
                    {
                        continue;
                    }
                    self.implied_closes(f);
                    if f & F_COL != 0 {
                        if let Some(i) = self.find_open(&["colgroup", "table"], TABLE_SCOPE) {
                            if self.stack[i].name.eq_ignore_ascii_case("table") {
                                self.queue.push_back(Event::Open {
                                    name: "colgroup",
                                    attrs: "",
                                });
                                self.push_open("colgroup", classify("colgroup"));
                            }
                        }
                    } else if f & F_TR != 0 {
                        if let Some(i) = self.find_open(&["table", "tbody", "thead", "tfoot"], &[])
                        {
                            if self.stack[i].name.eq_ignore_ascii_case("table") {
                                self.queue.push_back(Event::Open {
                                    name: "tbody",
                                    attrs: "",
                                });
                                self.push_open("tbody", classify("tbody"));
                            }
                        }
                    } else if f & F_TD_TH != 0 {
                        if let Some(i) =
                            self.find_open(&["tr", "tbody", "thead", "tfoot", "table"], &[])
                        {
                            let context = self.stack[i].name;
                            if context.eq_ignore_ascii_case("table") {
                                self.queue.push_back(Event::Open {
                                    name: "tbody",
                                    attrs: "",
                                });
                                self.push_open("tbody", classify("tbody"));
                            }
                            if is_one_of(context, &["table", "tbody", "thead", "tfoot"]) {
                                self.queue.push_back(Event::Open {
                                    name: "tr",
                                    attrs: "",
                                });
                                self.push_open("tr", classify("tr"));
                            }
                        }
                    }
                    let ev_open = Event::Open { name, attrs };
                    // `<x/>` is honoured only for void and foreign (svg/math) elements; on ordinary HTML
                    // elements the spec ignores the slash, so `<div/>` stays open exactly like in a browser
                    let closes_itself = f & F_VOID != 0
                        || (self_closing && (f & F_FOREIGN != 0 || self.in_foreign()));
                    if closes_itself {
                        if self.queue.is_empty() {
                            self.queue.push_back(Event::Close { name });
                            return Some(ev_open);
                        }
                        self.queue.push_back(ev_open);
                        self.queue.push_back(Event::Close { name });
                    } else {
                        self.push_open(name, f);
                        if is_one_of(name, &["pre", "textarea", "listing"]) {
                            self.strip_leading_nl = true;
                        }
                        if self.queue.is_empty() {
                            return Some(ev_open);
                        }
                        self.queue.push_back(ev_open);
                    }
                }
                Some(Token::End { name }) => {
                    self.strip_leading_nl = false;
                    self.end_tag(name);
                }
            }
        }
    }
}
