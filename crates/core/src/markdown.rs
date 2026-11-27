//! HTML -> Markdown over a stream of [`Walk`] events (any engine can produce them, see `Dom::walk`).
//!
//! Meant for LLM / RAG input: headings, paragraphs, lists, links, images, emphasis, code, quotes and tables
//! become Markdown; `<script>`, `<style>`, `<svg>`, `<template>`, `<head>` and friends are dropped.
use crate::dom::Walk;

/// Elements whose whole subtree is left out.
const SKIP: &[&str] = &[
    "script", "style", "svg", "noscript", "template", "head", "title", "iframe", "canvas", "object", "embed",
];

/// Block-level containers: a blank line before and after.
const BLOCKS: &[&str] = &[
    "p", "div", "section", "article", "header", "footer", "main", "nav", "aside", "figure", "figcaption", "form",
    "fieldset", "details", "summary", "address", "dl", "dt", "dd", "caption", "body", "html",
];

enum Kind {
    Root,
    Wrap(&'static str),
    Link(Option<String>),
    Heading(usize),
    Pre,
    Quote,
    Cell,
}

struct Frame {
    kind: Kind,
    buf: String,
}

#[derive(Default)]
struct Table {
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    th_in_row: bool,
    first_has_th: bool,
}

/// Feed it events, then call [`Renderer::finish`].
pub struct Renderer {
    frames: Vec<Frame>,
    tables: Vec<Table>,
    /// (ordered, last number)
    lists: Vec<(bool, usize)>,
    skip: usize,
    pre: usize,
    pending_space: bool,
    /// a list marker was just written: a block opening right after it must not break the line
    marker: bool,
}

impl Default for Renderer {
    fn default() -> Self {
        Renderer::new()
    }
}

/// Lower-case tag name without an allocation (no HTML element name is longer than 16 bytes; a longer one is no
/// tag we render, it becomes `""`: transparent).
fn lower_tag<'b>(name: &str, buf: &'b mut [u8; 16]) -> &'b str {
    let b = name.as_bytes();
    if b.len() > buf.len() {
        return "";
    }
    for (i, c) in b.iter().enumerate() {
        buf[i] = c.to_ascii_lowercase();
    }
    // ASCII lower-casing keeps UTF-8 valid
    std::str::from_utf8(&buf[..b.len()]).unwrap_or("")
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Renderer {
    pub fn new() -> Self {
        Renderer {
            frames: vec![Frame { kind: Kind::Root, buf: String::new() }],
            tables: Vec::new(),
            lists: Vec::new(),
            skip: 0,
            pre: 0,
            pending_space: false,
            marker: false,
        }
    }

    fn top(&mut self) -> &mut String {
        // `frames` always holds the root frame
        &mut self.frames.last_mut().expect("root frame").buf
    }

    fn put(&mut self, s: &str) {
        self.marker = false;
        self.top().push_str(s);
    }

    /// Make the output end with `n` newlines (nothing at the very start).
    fn ensure_break(&mut self, n: usize) {
        if self.marker {
            return;
        }
        let buf = self.top();
        if buf.is_empty() {
            return;
        }
        while buf.ends_with(' ') {
            buf.pop();
        }
        let have = buf.bytes().rev().take_while(|b| *b == b'\n').count();
        for _ in have..n {
            buf.push('\n');
        }
    }

    fn flush_space(&mut self) {
        if self.pending_space {
            self.pending_space = false;
            let buf = self.top();
            if !buf.is_empty() && !buf.ends_with('\n') && !buf.ends_with(' ') {
                buf.push(' ');
            }
        }
    }

    fn text(&mut self, s: &str) {
        if self.skip > 0 {
            return;
        }
        if self.pre > 0 {
            self.put(s);
            return;
        }
        // whole runs of non-whitespace are copied at once (a char-by-char push was the hot spot on long paragraphs)
        let is_ws = |c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}');
        let mut rest = s;
        loop {
            let t = rest.trim_start_matches(is_ws);
            if t.len() != rest.len() {
                self.pending_space = true;
            }
            if t.is_empty() {
                break;
            }
            let end = t.find(is_ws).unwrap_or(t.len());
            self.flush_space();
            self.marker = false;
            self.top().push_str(&t[..end]);
            rest = &t[end..];
        }
    }

    fn open(&mut self, name: &str, attr: &dyn Fn(&str) -> Option<String>) {
        let mut buf = [0u8; 16];
        let n = lower_tag(name, &mut buf);
        if self.skip > 0 || SKIP.contains(&n) {
            self.skip += 1;
            return;
        }
        match n {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.ensure_break(2);
                self.pending_space = false;
                let level = (n.as_bytes()[1] - b'0') as usize;
                self.frames.push(Frame { kind: Kind::Heading(level), buf: String::new() });
            }
            "br" => {
                self.pending_space = false;
                let buf = self.top();
                while buf.ends_with(' ') {
                    buf.pop();
                }
                self.put("\n");
            }
            "hr" => {
                self.ensure_break(2);
                self.pending_space = false;
                self.put("---");
                self.ensure_break(2);
            }
            "ul" | "ol" => {
                self.ensure_break(if self.lists.is_empty() { 2 } else { 1 });
                self.pending_space = false;
                let start = if n == "ol" {
                    attr("start").and_then(|s| s.trim().parse::<usize>().ok()).map(|s| s.saturating_sub(1)).unwrap_or(0)
                } else {
                    0
                };
                self.lists.push((n == "ol", start));
            }
            "li" => {
                self.marker = false;
                self.ensure_break(1);
                self.pending_space = false;
                let indent = "  ".repeat(self.lists.len().saturating_sub(1));
                let mark = match self.lists.last_mut() {
                    Some((true, k)) => {
                        *k += 1;
                        format!("{k}. ")
                    }
                    _ => "- ".to_string(),
                };
                self.put(&indent);
                self.put(&mark);
                self.marker = true;
            }
            "blockquote" => {
                self.ensure_break(2);
                self.pending_space = false;
                self.frames.push(Frame { kind: Kind::Quote, buf: String::new() });
            }
            "pre" => {
                self.ensure_break(2);
                self.pending_space = false;
                self.pre += 1;
                self.frames.push(Frame { kind: Kind::Pre, buf: String::new() });
            }
            "code" if self.pre == 0 => {
                self.flush_space();
                self.frames.push(Frame { kind: Kind::Wrap("`"), buf: String::new() });
            }
            "strong" | "b" => {
                self.flush_space();
                self.frames.push(Frame { kind: Kind::Wrap("**"), buf: String::new() });
            }
            "em" | "i" => {
                self.flush_space();
                self.frames.push(Frame { kind: Kind::Wrap("*"), buf: String::new() });
            }
            "del" | "s" | "strike" => {
                self.flush_space();
                self.frames.push(Frame { kind: Kind::Wrap("~~"), buf: String::new() });
            }
            "a" => {
                self.flush_space();
                self.frames.push(Frame { kind: Kind::Link(attr("href")), buf: String::new() });
            }
            "img" => {
                if let Some(src) = attr("src").filter(|s| !s.is_empty()) {
                    self.flush_space();
                    let alt = squash(&attr("alt").unwrap_or_default()).replace('[', "(").replace(']', ")");
                    self.put(&format!("![{alt}]({src})"));
                }
            }
            "table" => {
                self.ensure_break(2);
                self.pending_space = false;
                self.tables.push(Table::default());
            }
            "tr" => {
                if let Some(t) = self.tables.last_mut() {
                    t.row.clear();
                    t.th_in_row = false;
                }
            }
            "td" | "th" => {
                self.pending_space = false;
                if n == "th" {
                    if let Some(t) = self.tables.last_mut() {
                        t.th_in_row = true;
                    }
                }
                self.frames.push(Frame { kind: Kind::Cell, buf: String::new() });
            }
            _ if BLOCKS.contains(&n) => {
                self.ensure_break(2);
                self.pending_space = false;
            }
            _ => {}
        }
    }

    /// Close the innermost `Wrap(mark)` frame and write `mark inner mark` to its parent.
    fn close_wrap(&mut self, mark: &str) {
        if !matches!(self.frames.last().map(|f| &f.kind), Some(Kind::Wrap(m)) if *m == mark) {
            return;
        }
        let f = self.frames.pop().expect("frame");
        let inner = f.buf.trim();
        if !inner.is_empty() {
            self.put(&format!("{mark}{inner}{mark}"));
        }
    }

    fn close(&mut self, name: &str) {
        if self.skip > 0 {
            self.skip -= 1;
            return;
        }
        let mut buf = [0u8; 16];
        let n = lower_tag(name, &mut buf);
        match n {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if matches!(self.frames.last().map(|f| &f.kind), Some(Kind::Heading(_))) {
                    let f = self.frames.pop().expect("frame");
                    let level = if let Kind::Heading(l) = f.kind { l } else { 1 };
                    let text = squash(&f.buf);
                    if !text.is_empty() {
                        self.marker = false;
                        self.ensure_break(2);
                        self.put(&format!("{} {}", "#".repeat(level), text));
                    }
                    self.pending_space = false;
                    self.ensure_break(2);
                }
            }
            "ul" | "ol" => {
                self.lists.pop();
                self.marker = false;
                self.ensure_break(if self.lists.is_empty() { 2 } else { 1 });
                self.pending_space = false;
            }
            "li" => {
                self.marker = false;
                self.ensure_break(1);
                self.pending_space = false;
            }
            "blockquote" => {
                if matches!(self.frames.last().map(|f| &f.kind), Some(Kind::Quote)) {
                    let f = self.frames.pop().expect("frame");
                    let content = f.buf.trim_matches('\n').to_string();
                    if !content.is_empty() {
                        self.marker = false;
                        self.ensure_break(2);
                        for line in content.lines() {
                            if line.trim().is_empty() {
                                self.put(">\n");
                            } else {
                                self.put(&format!("> {line}\n"));
                            }
                        }
                    }
                    self.pending_space = false;
                    self.ensure_break(2);
                }
            }
            "pre" => {
                self.pre = self.pre.saturating_sub(1);
                if matches!(self.frames.last().map(|f| &f.kind), Some(Kind::Pre)) {
                    let f = self.frames.pop().expect("frame");
                    let code = f.buf.trim_end_matches('\n').to_string();
                    self.marker = false;
                    self.ensure_break(2);
                    self.put(&format!("```\n{code}\n```"));
                    self.pending_space = false;
                    self.ensure_break(2);
                }
            }
            "code" => self.close_wrap("`"),
            "strong" | "b" => self.close_wrap("**"),
            "em" | "i" => self.close_wrap("*"),
            "del" | "s" | "strike" => self.close_wrap("~~"),
            "a" => {
                if matches!(self.frames.last().map(|f| &f.kind), Some(Kind::Link(_))) {
                    let f = self.frames.pop().expect("frame");
                    let inner = f.buf.trim().to_string();
                    let href = match f.kind {
                        Kind::Link(h) => h.map(|h| h.trim().to_string()).filter(|h| !h.is_empty()),
                        _ => None,
                    };
                    if !inner.is_empty() {
                        match href {
                            Some(h) => self.put(&format!("[{inner}]({h})")),
                            None => self.put(&inner),
                        }
                    }
                }
            }
            "td" | "th" => {
                if matches!(self.frames.last().map(|f| &f.kind), Some(Kind::Cell)) {
                    let f = self.frames.pop().expect("frame");
                    let cell = squash(&f.buf).replace('|', "\\|");
                    self.pending_space = false;
                    match self.tables.last_mut() {
                        Some(t) => t.row.push(cell),
                        None => self.put(&cell),
                    }
                }
            }
            "tr" => {
                if let Some(t) = self.tables.last_mut() {
                    if !t.row.is_empty() {
                        if t.rows.is_empty() {
                            t.first_has_th = t.th_in_row;
                        }
                        let row = std::mem::take(&mut t.row);
                        t.rows.push(row);
                    }
                    t.th_in_row = false;
                }
            }
            "table" => {
                if let Some(t) = self.tables.pop() {
                    let md = render_table(&t);
                    if !md.is_empty() {
                        self.marker = false;
                        self.ensure_break(2);
                        self.put(&md);
                    }
                    self.pending_space = false;
                    self.ensure_break(2);
                }
            }
            _ if BLOCKS.contains(&n) => {
                self.ensure_break(2);
                self.pending_space = false;
            }
            _ => {}
        }
    }

    pub fn feed(&mut self, ev: Walk<'_>) {
        match ev {
            Walk::Open { name, attr } => self.open(name, attr),
            Walk::Close { name } => self.close(name),
            Walk::Text(t) => self.text(t),
        }
    }

    pub fn finish(mut self) -> String {
        // frames left open by a truncated event stream: fold them into their parents
        while self.frames.len() > 1 {
            let f = self.frames.pop().expect("frame");
            self.top().push_str(&f.buf);
        }
        let raw = self.frames.pop().map(|f| f.buf).unwrap_or_default();
        tidy(&raw)
    }
}

fn render_table(t: &Table) -> String {
    let cols = t.rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if cols == 0 {
        return String::new();
    }
    let mut out = String::new();
    for (i, row) in t.rows.iter().enumerate() {
        out.push('|');
        for c in 0..cols {
            out.push(' ');
            out.push_str(row.get(c).map(String::as_str).unwrap_or(""));
            out.push_str(" |");
        }
        out.push('\n');
        if i == 0 {
            out.push('|');
            for _ in 0..cols {
                out.push_str(" --- |");
            }
            out.push('\n');
        }
    }
    out.truncate(out.trim_end_matches('\n').len());
    out
}

/// Trim line ends and squeeze runs of blank lines (code fences are left alone).
fn tidy(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_fence = false;
    let mut blank = 0usize;
    for line in raw.split('\n') {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        let l = if in_fence { line } else { line.trim_end() };
        if !in_fence && l.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(l);
        out.push('\n');
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(events: &[(&str, Option<&str>)]) -> String {
        let mut r = Renderer::new();
        let none = |_: &str| -> Option<String> { None };
        for (name, text) in events {
            match text {
                Some(t) => r.feed(Walk::Text(t)),
                None if name.starts_with('/') => r.feed(Walk::Close { name: &name[1..] }),
                None => r.feed(Walk::Open { name, attr: &none }),
            }
        }
        r.finish()
    }

    #[test]
    fn headings_paragraphs_lists_and_emphasis() {
        let md = run(&[
            ("h1", None), ("", Some(" Title ")), ("/h1", None),
            ("p", None), ("", Some("Some ")), ("b", None), ("", Some("bold")), ("/b", None), ("", Some(" text")), ("/p", None),
            ("ul", None), ("li", None), ("", Some("one")), ("/li", None), ("li", None), ("p", None), ("", Some("two")), ("/p", None), ("/li", None), ("/ul", None),
        ]);
        assert_eq!(md, "# Title\n\nSome **bold** text\n\n- one\n- two");
    }

    #[test]
    fn scripts_are_dropped() {
        let md = run(&[("p", None), ("", Some("a")), ("script", None), ("", Some("var x;")), ("/script", None), ("", Some(" b")), ("/p", None)]);
        assert_eq!(md, "a b");
    }
}
