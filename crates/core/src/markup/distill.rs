//! Skeleton HTML: what a page *looks like* in a few KB, for humans and for LLM prompts.
//!
//! Scripts, styles, svg and most attributes are dropped, text is clipped, and runs of similar siblings
//! (the cards of a list) are folded into one comment, so a 3 MB product page becomes something that fits
//! in a prompt and still shows the structure a selector has to match.
use super::arena::{Doc, Kind, NIL};
use super::events::is_void;
use super::tokenizer;

#[derive(Debug, Clone)]
pub struct DistillOptions {
    /// how many members of a run of similar siblings are kept (the rest becomes `<!-- +N similar ... -->`)
    pub keep: usize,
    /// text nodes are clipped to this many characters
    pub max_text: usize,
    /// hard cap on the size of the output
    pub max_chars: usize,
}

impl Default for DistillOptions {
    fn default() -> Self {
        DistillOptions { keep: 2, max_text: 80, max_chars: 20_000 }
    }
}

const SKIP: &[&str] = &["script", "style", "svg", "noscript", "template", "iframe", "canvas", "link", "meta", "base"];
const KEEP_ATTRS: &[&str] = &[
    "id", "class", "href", "src", "alt", "title", "name", "type", "value", "role", "aria-label", "itemprop", "itemtype",
    "property", "content", "for", "placeholder", "data-testid", "data-test", "data-qa",
];

enum Item<'a> {
    Node(u32, usize),
    Close(&'a str),
    Note(String),
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max).collect();
        t.push('…');
        t
    }
}

/// Signature (tag + sorted classes) and a printable label of an element.
fn signature(doc: &Doc<'_>, id: u32) -> (String, String) {
    if let Kind::Element { name, attrs } = doc.node(id).kind {
        let name = name.to_ascii_lowercase();
        let mut classes: Vec<&str> = tokenizer::find_attr(attrs, "class").unwrap_or("").split_whitespace().collect();
        classes.sort_unstable();
        let sig = format!("{name}.{}", classes.join("."));
        let label = if classes.is_empty() { format!("<{name}>") } else { format!("<{name} class=\"{}\">", classes.join(" ")) };
        (sig, label)
    } else {
        (String::new(), String::new())
    }
}

fn write_open(doc: &Doc<'_>, id: u32, depth: usize, opts: &DistillOptions, out: &mut String) {
    if let Kind::Element { name, attrs } = doc.node(id).kind {
        out.push('\n');
        for _ in 0..depth {
            out.push_str("  ");
        }
        out.push('<');
        out.push_str(&name.to_ascii_lowercase());
        for (k, v) in tokenizer::attrs(attrs) {
            let k = k.to_ascii_lowercase();
            if KEEP_ATTRS.contains(&k.as_str()) {
                let v = clip(v.unwrap_or(""), if k == "href" || k == "src" { 80 } else { opts.max_text }).replace('"', "&quot;");
                out.push_str(&format!(" {k}=\"{v}\""));
            }
        }
        out.push('>');
    }
}

fn flush(run: &mut Option<(String, usize, String)>, depth: usize, keep: usize, items: &mut Vec<Item<'_>>) {
    if let Some((_, count, label)) = run.take() {
        if count > keep {
            items.push(Item::Note(format!("\n{}<!-- +{} similar {} -->", "  ".repeat(depth), count - keep, label)));
        }
    }
}

fn push_children<'a>(doc: &Doc<'a>, id: u32, depth: usize, keep: usize, stack: &mut Vec<Item<'a>>) {
    let mut items: Vec<Item<'a>> = Vec::new();
    let mut run: Option<(String, usize, String)> = None;
    let mut c = doc.node(id).first_child;
    while c != NIL {
        match doc.node(c).kind {
            Kind::Element { name, .. } => {
                // `<script>` / `<template>` between cards are dropped anyway: they must not end the run either
                if SKIP.iter().any(|s| name.eq_ignore_ascii_case(s)) {
                    c = doc.node(c).next;
                    continue;
                }
                let (sig, label) = signature(doc, c);
                let same = matches!(&run, Some((rs, _, _)) if *rs == sig);
                if same {
                    if let Some((_, count, _)) = run.as_mut() {
                        *count += 1;
                        if *count <= keep {
                            items.push(Item::Node(c, depth));
                        }
                    }
                } else {
                    flush(&mut run, depth, keep, &mut items);
                    run = Some((sig, 1, label));
                    items.push(Item::Node(c, depth));
                }
            }
            Kind::Text { text, .. } => {
                if !text.trim().is_empty() {
                    flush(&mut run, depth, keep, &mut items);
                    items.push(Item::Node(c, depth));
                }
            }
            Kind::Document => {}
        }
        c = doc.node(c).next;
    }
    flush(&mut run, depth, keep, &mut items);
    for it in items.into_iter().rev() {
        stack.push(it);
    }
}

/// Distil `src` (see the module docs).
pub fn distill(src: &str, opts: &DistillOptions) -> String {
    let doc = Doc::parse(src);
    let mut stack: Vec<Item<'_>> = Vec::new();
    push_children(&doc, 0, 0, opts.keep, &mut stack);
    render(&doc, stack, opts)
}

/// Where a broken selector used to match: the container (`parent`, as remembered in a fingerprint) distilled on
/// its own, so a prompt shows the few KB that matter instead of the whole page. Returns `(label, skeleton)`, or
/// `None` if no element of `src` looks enough like that container (then distil the whole page).
pub fn distill_container(
    src: &str,
    tag: &str,
    id: Option<&str>,
    classes: &[String],
    path: &[String],
    opts: &DistillOptions,
) -> Option<(String, String)> {
    let doc = Doc::parse(src);
    let mut best: Option<(f64, u32)> = None;
    let mut cur = doc.node(0).first_child;
    // pre-order walk over every element
    let mut visited = 0usize;
    while cur != NIL {
        visited += 1;
        if visited > 2_000_000 {
            break;
        }
        if let Kind::Element { name, attrs } = doc.node(cur).kind {
            if name.eq_ignore_ascii_case(tag) {
                let mut score = 0.0f64;
                let cid = tokenizer::find_attr(attrs, "id").map(|v| v.trim()).filter(|v| !v.is_empty());
                if let (Some(a), Some(b)) = (id, cid) {
                    if a == b {
                        score += 10.0;
                    }
                }
                let cclasses: Vec<&str> = tokenizer::find_attr(attrs, "class").unwrap_or("").split_whitespace().collect();
                if !classes.is_empty() && !cclasses.is_empty() {
                    let common = classes.iter().filter(|c| cclasses.contains(&c.as_str())).count();
                    let union = classes.len() + cclasses.len() - common;
                    score += 5.0 * common as f64 / union.max(1) as f64;
                }
                // ancestors, nearest first: the same chain of tag names as in the fingerprint's path
                if !path.is_empty() {
                    let mut chain: Vec<String> = Vec::new();
                    let mut p = doc.node(cur).parent;
                    while p != NIL && p != 0 {
                        if let Kind::Element { name, .. } = doc.node(p).kind {
                            chain.push(name.to_ascii_lowercase());
                        }
                        p = doc.node(p).parent;
                    }
                    chain.reverse();
                    // path in a fingerprint ends with the *parent of the element*, i.e. includes this container's tag
                    let want: Vec<String> = path.iter().map(|s| s.to_ascii_lowercase()).collect();
                    if chain.ends_with(&want[..want.len() - 1]) {
                        score += 1.0;
                    }
                }
                if score > 0.0 && best.map_or(true, |(b, _)| score > b) {
                    best = Some((score, cur));
                }
            }
        }
        cur = doc.next_preorder(cur, 0);
    }
    // an id or at least half of the classes must agree, otherwise "the same container" is a guess
    let (score, node) = best?;
    if score < 2.5 {
        return None;
    }
    let stack: Vec<Item<'_>> = vec![Item::Node(node, 0)];
    let label = match doc.node(node).kind {
        Kind::Element { name, attrs } => {
            let mut l = name.to_ascii_lowercase();
            if let Some(i) = tokenizer::find_attr(attrs, "id") {
                l.push('#');
                l.push_str(i.trim());
            }
            for c in tokenizer::find_attr(attrs, "class").unwrap_or("").split_whitespace() {
                l.push('.');
                l.push_str(c);
            }
            l
        }
        _ => String::new(),
    };
    Some((label, render(&doc, stack, opts)))
}

/// Distil one element (itself included) of an already parsed document: no serialisation, no re-parse.
pub fn distill_node(doc: &Doc<'_>, id: u32, opts: &DistillOptions) -> String {
    let mut stack: Vec<Item<'_>> = Vec::new();
    match doc.node(id).kind {
        Kind::Element { .. } => stack.push(Item::Node(id, 0)),
        _ => push_children(doc, id, 0, opts.keep, &mut stack),
    }
    render(doc, stack, opts)
}

fn render<'a>(doc: &Doc<'a>, mut stack: Vec<Item<'a>>, opts: &DistillOptions) -> String {
    let mut out = String::new();
    while let Some(it) = stack.pop() {
        if out.len() > opts.max_chars {
            out.push_str("\n<!-- truncated -->");
            break;
        }
        match it {
            Item::Close(name) => {
                out.push_str("</");
                out.push_str(&name.to_ascii_lowercase());
                out.push('>');
            }
            Item::Note(s) => out.push_str(&s),
            Item::Node(id, depth) => match doc.node(id).kind {
                Kind::Text { text, .. } => {
                    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !t.is_empty() {
                        out.push_str(&clip(&t, opts.max_text));
                    }
                }
                Kind::Element { name, .. } => {
                    if SKIP.iter().any(|s| name.eq_ignore_ascii_case(s)) {
                        continue;
                    }
                    write_open(doc, id, depth, opts, &mut out);
                    if is_void(name) {
                        continue;
                    }
                    stack.push(Item::Close(name));
                    push_children(doc, id, depth + 1, opts.keep, &mut stack);
                }
                Kind::Document => {}
            },
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_similar_siblings_and_drops_noise() {
        let mut html = String::from("<html><head><title>T</title><script>x()</script><style>a{}</style></head><body><ul class=list>");
        for i in 0..5 {
            html.push_str(&format!("<li class=item data-x=1><a href=/{i}>A{i}</a></li>"));
        }
        html.push_str("</ul><div style='color:red' onclick='x()'>some text</div><svg><path d=x /></svg></body></html>");
        let out = distill(&html, &DistillOptions::default());
        assert!(out.contains("<title>T</title>"), "{out}");
        assert!(out.contains("+3 similar"), "{out}");
        assert!(out.contains("A0") && out.contains("A1") && !out.contains("A2"), "{out}");
        assert!(!out.contains("script") && !out.contains("style") && !out.contains("svg") && !out.contains("onclick"), "{out}");
        assert!(out.contains("some text"), "{out}");
    }

    #[test]
    fn dropped_tags_between_cards_do_not_break_the_fold() {
        let html = format!("<ul>{}</ul>", "<li class=i>x</li><script>1</script><template><b>t</b></template>".repeat(6));
        let out = distill(&html, &DistillOptions::default());
        assert!(out.contains("+4 similar"), "{out}");
        assert!(!out.contains("script") && !out.contains("template"), "{out}");
    }

    #[test]
    fn caps_the_size() {
        let html = format!("<div>{}</div>", "<p>x</p><b>y</b>".repeat(5000));
        let out = distill(&html, &DistillOptions { keep: 100000, max_text: 10, max_chars: 500 });
        assert!(out.len() < 700 && out.ends_with("<!-- truncated -->"), "{}", out.len());
    }
}
