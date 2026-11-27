//! `stream` engine (Frostwork-style): **no DOM**. The schema is analysed at compile time; if every field is a
//! "capture" (first match text/attr, exists, count, or a list of records built from such captures) the document
//! is processed in one forward pass over the shared tokenizer, with all selectors matched against the
//! stack of open elements. Memory is O(depth + result), not O(document).
//!
//! Whatever cannot be streamed (xpath, parent/root, filter, nested objects, follow, ...) is reported by
//! [`StreamPlan::compile`] with a reason; the `auto` engine then falls back to a DOM engine.
//!
//! Semantics are identical to the DOM engines for streamable schemas (same tokenizer, same tree rules),
//! which the test-suite checks by running both and comparing.
use crate::dom::Dom;
use crate::engine::Engine;
use crate::engines::lite;
use crate::error::{Error, ErrorInfo, Result};
use crate::exec::{JsonCache, Session};
use crate::ir::{Expr, OnError};
use crate::markup::css::{ElemView, Selector};
use crate::markup::events::{Event, Events};
use crate::markup::{entities, tokenizer};
use crate::plan::{CExpr, COp, Plan};
use crate::value::Value;
use indexmap::IndexMap;
use std::borrow::Cow;

// ---------------------------------------------------------------------------------------------
// the streamable subset: analysis
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum Source {
    /// the item element itself (only inside records)
    Own,
    /// descendants matching selector `#i`
    Sel(usize),
}

/// Options for collecting text from an element.
#[derive(Debug, Clone)]
struct TextSpec {
    sep: String,
    strip: bool,
    br: Option<String>,
    deep: bool,
    /// selector of the subtrees to leave out (`text(without=".ad, script")`): the collector skips their text
    without: Option<usize>,
}

fn text_spec(
    sep: &str,
    strip: bool,
    without: &Option<usize>,
    br: &Option<String>,
    deep: bool,
    sels: &[Selector],
) -> std::result::Result<TextSpec, String> {
    if let Some(w) = without {
        check_sel(sels, *w)?;
    }
    Ok(TextSpec { sep: sep.to_string(), strip, br: br.clone(), deep, without: *without })
}

#[derive(Debug, Clone)]
enum Take {
    TextOpts(TextSpec),
    OwnTexts,
    Text,
    Attr(String),
    HasAttr(String),
    Exists,
    Count,
}

#[derive(Debug, Clone)]
struct Cap {
    src: Source,
    take: Take,
}

#[derive(Debug, Clone)]
enum FieldKind {
    Lit(Value),
    Index,
    /// a pipeline that starts from `meta` (or a literal) and needs no document at all
    Pure,
    Cap(Cap),
}

#[derive(Debug, Clone)]
struct Field {
    name: String,
    kind: FieldKind,
    /// value-only operations applied after the capture (`strip`, `float`, `default`, ...)
    tail: Vec<COp>,
}

enum ListBody {
    Record(Vec<Field>),
    Single(Field),
}

enum Top {
    Field(Field),
    /// `T.meta_tags(...)` over the whole document (+ a value-only tail such as `["og:title"]`). Read from the `<head>`:
    /// the document is not read past the opening `<body>`.
    Meta { name: String, prefixes: Vec<String>, mode: u8, tail: Vec<COp> },
    /// `mode`: what a failing record does (`map(..., on_error=...)`), same semantics as in the DOM engines
    List { name: String, item: usize, body: ListBody, tail: Vec<COp>, mode: OnError },
}

struct Program {
    tops: Vec<Top>,
    /// distinct selector indices that must be evaluated on every element
    #[allow(dead_code)]
    used: Vec<usize>,
    /// every top-level field is a single capture that finishes by itself (no list, no `count`):
    /// the document does not have to be read to the end once they are all done
    stoppable: bool,
    /// some top is `meta_tags`: its `<meta>`s live in the head, so an early exit waits for `<body>`
    has_meta: bool,
    /// some field calls `url()` without an explicit base: `<base href>` matters, so the head must be read first
    needs_base: bool,
    /// some selector uses `:first-of-type` or `:nth-of-type`: maintain `sib_tags`
    needs_type_index: bool,
    /// number of captures that must finish before an early exit is possible
    singles: usize,
}

/// Operations that treat a *node* exactly like its text. After `select_first` the capture may stream the text and
/// run these on it. (`count`, `first`, `exists`, `not`, `key`, ... mean something else on a node: not allowed.)
fn text_like_op(op: &COp) -> bool {
    matches!(
        op,
        COp::Strip
            | COp::Lower
            | COp::Upper
            | COp::Replace(..)
            | COp::Split(_)
            | COp::Regex(..)
            | COp::RegexAll(..)
            | COp::Int
            | COp::Float
            | COp::Bool
            | COp::ParseSrcset { .. }
            | COp::Price { .. }
            | COp::ParseJson
            | COp::JsonPath(_)
            | COp::JsJson(_)
            | COp::NextFlight { .. }
            | COp::ParseHtml(_)
            | COp::LinkHeader(_)
            | COp::Eq(_)
            | COp::Ne(_)
            | COp::Contains(_)
            | COp::StartsWith(_)
            | COp::EndsWith(_)
            | COp::Default(_)
    )
}

fn is_value_expr(e: &CExpr) -> bool {
    match e {
        CExpr::Lit(_) => true,
        CExpr::List(es) => es.iter().all(is_value_expr),
        CExpr::Object(fs) => fs.iter().all(|(_, v)| is_value_expr(v)),
        CExpr::Pipe(ops) => ops.iter().all(is_value_op),
    }
}

/// Operations that need no document node: they can run on a captured value (with `NoDom`),
/// including `map` / `filter` / `if` / ... whose nested expressions are value-only too.
fn is_value_op(op: &COp) -> bool {
    match op {
        COp::Strip
        | COp::Lower
        | COp::Upper
        | COp::Replace(..)
        | COp::Split(_)
        | COp::Join(_)
        | COp::Regex(..)
        | COp::RegexAll(..)
        | COp::Int
        | COp::Float
        | COp::Bool
        | COp::ParseSrcset { .. }
        | COp::Price { .. }
        | COp::Required
        | COp::Not
        | COp::Count
        | COp::First
        | COp::Last
        | COp::At(_)
        | COp::Slice { .. }
        | COp::Key(_)
        | COp::NonEmpty
        | COp::ParseJson
        | COp::JsonPath(_)
        | COp::Index
        | COp::Meta(_)
        | COp::JsJson(_)
        | COp::NextFlight { .. }
        | COp::ParseHtml(_)
        | COp::LinkHeader(_)
        | COp::ResolveUrl(None)
        | COp::Exists => true,
        COp::ResolveUrl(Some(e)) => is_value_expr(e),
        COp::Map(e)
        | COp::FlatMap(e)
        | COp::Filter(e)
        | COp::Unique { by: Some(e), .. }
        | COp::Default(e)
        | COp::Eq(e)
        | COp::Ne(e)
        | COp::Contains(e)
        | COp::StartsWith(e)
        | COp::EndsWith(e)
        | COp::And(e)
        | COp::Or(e) => is_value_expr(e),
        COp::If(c, t, o) => is_value_expr(c) && is_value_expr(t) && is_value_expr(o),
        COp::Coalesce(es) => es.iter().all(is_value_expr),
        COp::Unique { by: None, .. } => true,
        _ => false,
    }
}

fn check_sel(sels: &[Selector], i: usize) -> std::result::Result<(), String> {
    match sels[i].stream_unsupported() {
        Some(why) => Err(format!("selector {:?} uses {why}", sels[i].source())),
        None => Ok(()),
    }
}

fn check_tail(name: &str, rest: &[COp]) -> std::result::Result<(), String> {
    if rest.iter().all(is_value_op) {
        Ok(())
    } else {
        Err(format!("field `{name}`: an operation after the capture needs the DOM"))
    }
}

fn analyze_field(name: &str, e: &CExpr, in_item: bool, sels: &[Selector]) -> std::result::Result<Field, String> {
    let ops = match e {
        CExpr::Lit(v) => {
            return Ok(Field { name: name.to_string(), kind: FieldKind::Lit(v.clone()), tail: Vec::new() });
        }
        CExpr::Pipe(ops) => ops,
        _ => return Err(format!("field `{name}`: nested objects/lists need the DOM")),
    };
    if matches!(ops.first(), Some(COp::Meta(_))) {
        check_tail(name, ops)?;
        return Ok(Field { name: name.to_string(), kind: FieldKind::Pure, tail: ops.to_vec() });
    }
    let (kind, rest): (FieldKind, &[COp]) = match ops.first() {
        Some(COp::Index) if in_item => (FieldKind::Index, &ops[1..]),
        Some(COp::Select(i)) | Some(COp::SelectFirst(i)) => {
            let first_only = matches!(ops[0], COp::SelectFirst(_));
            check_sel(sels, *i)?;
            let cap = |take: Take| FieldKind::Cap(Cap { src: Source::Sel(*i), take });
            match ops.get(1) {
                Some(COp::Text) => (cap(Take::Text), &ops[2..]),
                Some(COp::OwnTexts) => (cap(Take::OwnTexts), &ops[2..]),
                Some(COp::TextOpts { sep, strip, without, br, deep }) => {
                    (cap(Take::TextOpts(text_spec(sep, *strip, without, br, *deep, sels)?)), &ops[2..])
                }
                Some(COp::Attr(a)) => (cap(Take::Attr(a.clone())), &ops[2..]),
                Some(COp::HasAttr(a)) => (cap(Take::HasAttr(a.clone())), &ops[2..]),
                Some(COp::Count) if !first_only => (cap(Take::Count), &ops[2..]),
                None if first_only => (cap(Take::Text), &ops[1..]),
                Some(op) if first_only && text_like_op(op) => (cap(Take::Text), &ops[1..]),
                _ => return Err(format!("field `{name}`: after a selector only text/attr/count/exists can be streamed")),
            }
        }
        Some(COp::SelectExists(i)) => {
            check_sel(sels, *i)?;
            (FieldKind::Cap(Cap { src: Source::Sel(*i), take: Take::Exists }), &ops[1..])
        }
        Some(COp::Text) if in_item => (FieldKind::Cap(Cap { src: Source::Own, take: Take::Text }), &ops[1..]),
        Some(COp::OwnTexts) if in_item => (FieldKind::Cap(Cap { src: Source::Own, take: Take::OwnTexts }), &ops[1..]),
        Some(COp::TextOpts { sep, strip, without, br, deep }) if in_item => (
            FieldKind::Cap(Cap { src: Source::Own, take: Take::TextOpts(text_spec(sep, *strip, without, br, *deep, sels)?) }),
            &ops[1..],
        ),
        Some(COp::Attr(a)) if in_item => (FieldKind::Cap(Cap { src: Source::Own, take: Take::Attr(a.clone()) }), &ops[1..]),
        Some(COp::HasAttr(a)) if in_item => (FieldKind::Cap(Cap { src: Source::Own, take: Take::HasAttr(a.clone()) }), &ops[1..]),
        _ => return Err(format!("field `{name}`: this pipeline shape needs the DOM")),
    };
    check_tail(name, rest)?;
    Ok(Field { name: name.to_string(), kind, tail: rest.to_vec() })
}

fn analyze_top(name: &str, e: &CExpr, sels: &[Selector]) -> std::result::Result<Top, String> {
    if let CExpr::Pipe(ops) = e {
        if let Some(COp::MetaTags { prefixes, mode, .. }) = ops.first() {
            let tail = &ops[1..];
            check_tail(name, tail)?;
            return Ok(Top::Meta { name: name.to_string(), prefixes: prefixes.clone(), mode: *mode, tail: tail.to_vec() });
        }
        let map: Option<(&CExpr, OnError)> = match ops.get(1) {
            Some(COp::Map(body)) => Some((body, OnError::Raise)),
            Some(COp::MapSafe(body, mode)) => Some((body, *mode)),
            _ => None,
        };
        if let (Some(COp::Select(i)), Some((body, mode))) = (ops.first(), map) {
            check_sel(sels, *i)?;
            let body = match body {
                CExpr::Object(fs) => {
                    let mut v = Vec::with_capacity(fs.len());
                    for (k, fe) in fs {
                        v.push(analyze_field(k, fe, true, sels)?);
                    }
                    ListBody::Record(v)
                }
                CExpr::Pipe(_) => ListBody::Single(analyze_field("", body, true, sels)?),
                _ => return Err(format!("field `{name}`: map body must be an object or a pipeline")),
            };
            let tail = &ops[2..];
            check_tail(name, tail)?;
            return Ok(Top::List { name: name.to_string(), item: *i, body, tail: tail.to_vec(), mode });
        }
    }
    Ok(Top::Field(analyze_field(name, e, false, sels)?))
}

fn analyze(base: &Plan<Selector>) -> std::result::Result<Program, String> {
    let fields = match &base.root {
        CExpr::Object(fs) => fs,
        _ => return Err("the root of the schema must be an object".into()),
    };
    if !base.subs.is_empty() || !base.foreign_specs.is_empty() || !base.pagers.is_empty() {
        return Err("`follow` / `paginate` needs a DOM engine for the parent page".into());
    }
    let mut tops = Vec::with_capacity(fields.len());
    for (name, e) in fields {
        tops.push(analyze_top(name, e, &base.sels)?);
    }
    let mut used: Vec<usize> = Vec::new();
    let mut note = |c: &Cap| {
        if let Source::Sel(i) = c.src {
            used.push(i);
        }
        if let Take::TextOpts(TextSpec { without: Some(w), .. }) = &c.take {
            used.push(*w);
        }
    };
    for t in &tops {
        match t {
            Top::Field(f) => {
                if let FieldKind::Cap(c) = &f.kind {
                    note(c);
                }
            }
            Top::Meta { .. } => {}
            Top::List { item, body, .. } => {
                note(&Cap { src: Source::Sel(*item), take: Take::Exists });
                match body {
                    ListBody::Record(fs) => fs.iter().for_each(|f| {
                        if let FieldKind::Cap(c) = &f.kind {
                            note(c)
                        }
                    }),
                    ListBody::Single(f) => {
                        if let FieldKind::Cap(c) = &f.kind {
                            note(c);
                        }
                    }
                }
            }
        }
    }
    used.sort_unstable();
    used.dedup();
    let is_count = |f: &Field| matches!(&f.kind, FieldKind::Cap(c) if matches!(c.take, Take::Count));
    let stoppable = tops.iter().all(|t| match t {
        Top::Field(f) => !is_count(f),
        Top::Meta { .. } => true,
        Top::List { .. } => false,
    });
    let has_meta = tops.iter().any(|t| matches!(t, Top::Meta { .. }));
    let singles = tops
        .iter()
        .filter(|t| matches!(t, Top::Field(f) if matches!(f.kind, FieldKind::Cap(_)) && !is_count(f)))
        .count();
    let needs_base = tops.iter().any(|t| match t {
        Top::Field(f) => f.tail.iter().any(|o| matches!(o, COp::ResolveUrl(None))),
        Top::Meta { tail, .. } => tail.iter().any(|o| matches!(o, COp::ResolveUrl(None))),
        Top::List { .. } => false,
    });
    let needs_type_index = used.iter().any(|&si| base.sels[si].uses_type_index());
    Ok(Program { tops, used, stoppable, needs_base, has_meta, needs_type_index, singles })
}

// ---------------------------------------------------------------------------------------------
// a Dom with no document, used to run the value-only "tail" operations
// ---------------------------------------------------------------------------------------------

struct NoDom;

impl Dom for NoDom {
    type Node = ();
    type Sel = ();
    const NAME: &'static str = "none";
    fn compile_css(_src: &str) -> std::result::Result<(), String> {
        Err("no document".into())
    }
    fn root(&self) {}
    fn select(&self, _n: &(), _s: &(), _f: bool, _o: &mut Vec<()>) {}
    fn text(&self, _n: &()) -> String {
        String::new()
    }
    fn html(&self, _n: &(), _i: bool) -> String {
        String::new()
    }
    fn attr(&self, _n: &(), _k: &str) -> Option<String> {
        None
    }
    fn to_value(&self, _n: &()) -> Value {
        Value::Null
    }
    fn node_key(&self, _n: &()) -> usize {
        0
    }
}

// ---------------------------------------------------------------------------------------------
// the plan
// ---------------------------------------------------------------------------------------------

/// A schema compiled for single-pass streaming execution.
pub struct StreamPlan {
    base: Plan<Selector>,
    prog: Program,
    tails: Plan<()>,
}

impl StreamPlan {
    /// Compile `ir`. Fails (with a reason) if the schema is not streamable.
    pub fn compile(ir: &Expr) -> Result<StreamPlan> {
        let base = lite::compile(ir)?;
        let prog = analyze(&base)
            .map_err(|why| Error::Compile { path: "$".to_string(), msg: format!("schema cannot be streamed: {why}") })?;
        let mut tails = Plan::empty();
        tails.html_subs = base.html_subs.clone();
        Ok(StreamPlan { base, prog, tails })
    }

    /// Run the schema over `src` in one pass.
    pub fn run(&self, src: &str, meta: &Value) -> Result<Value> {
        Ok(self.run_detailed(src, meta)?.0)
    }

    /// Like [`StreamPlan::run`], plus the records quarantined by `map(..., on_error="skip" | "null")`.
    pub fn run_detailed(&self, src: &str, meta: &Value) -> Result<(Value, Vec<ErrorInfo>)> {
        let mut r = Run {
            plan: self,
            stack: Vec::new(),
            root_children: 0,
            root_legends: 0,
            sib_tags: Vec::new(),
            matcher: Matcher {
                epoch: 0,
                seen_at: vec![0; self.base.sels.len()],
                scope_at: vec![0; self.base.sels.len()],
                matched: vec![false; self.base.sels.len()],
            },
            state: self
                .prog
                .tops
                .iter()
                .map(|t| match t {
                    Top::Field(_) => TopState::Field(CapState::default()),
                    Top::Meta { .. } => TopState::Meta(IndexMap::new()),
                    Top::List { body, .. } => {
                        let stride = match body {
                            ListBody::Record(fs) => fs.len(),
                            ListBody::Single(_) => 1,
                        };
                        TopState::List(ListState::new(stride))
                    }
                })
                .collect(),
            collectors: Vec::new(),
            remaining: self.prog.singles,
            base_href: None,
            base_seen: false,
            seen_body: false,
        };
        for ev in Events::new(src) {
            match ev {
                Event::Open { name, attrs } => r.on_open(name, attrs),
                Event::Close { .. } => {
                    r.on_close();
                    // every single-value capture is done and closed: the rest of the document cannot change the
                    // result (a title / og:image / first price on a 3 MB page is found in the first few KB)
                    if r.can_stop() {
                        break;
                    }
                }
                Event::Text { text, decode } => r.on_text(text, decode),
            }
        }
        r.finish(meta)
    }

    fn describe(&self) -> String {
        let mut out = String::from("stream program (single pass, no DOM):\n");
        let sel = |i: usize| format!("css {:?} #{i}", self.base.sel_src[i]);
        let cap = |c: &Cap| {
            let src = match c.src {
                Source::Own => "item".to_string(),
                Source::Sel(i) => sel(i),
            };
            let take = match &c.take {
                Take::Text => "text".to_string(),
                Take::OwnTexts => "own_texts".to_string(),
                Take::TextOpts(s) => format!("text(sep={:?}, strip={}, br={:?}, without={:?}, deep={})", s.sep, s.strip, s.br, s.without, s.deep),
                Take::Attr(a) => format!("attr({a:?})"),
                Take::HasAttr(a) => format!("has_attr({a:?})"),
                Take::Exists => "exists".to_string(),
                Take::Count => "count".to_string(),
            };
            format!("{src} -> {take}")
        };
        let field = |f: &Field| match &f.kind {
            FieldKind::Lit(v) => format!("{}: lit {v:?}", f.name),
            FieldKind::Index => format!("{}: index", f.name),
            FieldKind::Pure => format!("{}: value-only pipeline ({} ops, from meta)", f.name, f.tail.len()),
            FieldKind::Cap(c) => format!("{}: {} (+{} value ops)", f.name, cap(c), f.tail.len()),
        };
        for t in &self.prog.tops {
            match t {
                Top::Field(f) => out.push_str(&format!("  {}\n", field(f))),
                Top::Meta { name, prefixes, mode, tail } => out.push_str(&format!(
                    "  {name}: meta_tags({prefixes:?}, mode={mode}) read from the head (+{} value ops)\n",
                    tail.len()
                )),
                Top::List { name, item, body, mode, .. } => {
                    let policy = if *mode == OnError::Raise { String::new() } else { format!(" (on_error={mode:?})") };
                    out.push_str(&format!("  {name}: records of {}{policy}\n", sel(*item)));
                    match body {
                        ListBody::Record(fs) => fs.iter().for_each(|f| out.push_str(&format!("      {}\n", field(f)))),
                        ListBody::Single(f) => out.push_str(&format!("      {}\n", field(f))),
                    }
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------
// execution
// ---------------------------------------------------------------------------------------------

#[derive(Clone)]
struct CapState {
    done: bool,
    val: Value,
    count: i64,
}

impl Default for CapState {
    fn default() -> Self {
        CapState { done: false, val: Value::Null, count: 0 }
    }
}

struct ListState {
    /// Flat row-major storage: record `ri`, field `fi` lives at `ri * stride + fi`.
    vals: Vec<CapState>,
    stride: usize,
    len: usize,
    /// open records: (record index, depth of the item element)
    open: Vec<(usize, usize)>,
}

impl ListState {
    fn new(stride: usize) -> Self {
        ListState { vals: Vec::new(), stride, len: 0, open: Vec::new() }
    }
}

enum TopState {
    Field(CapState),
    Meta(IndexMap<String, Value>),
    List(ListState),
}

#[derive(Clone, Copy)]
enum Target {
    Top(usize),
    Rec { top: usize, rec: usize, field: usize },
}

struct Collector {
    depth: usize,
    target: Target,
    buf: String,
    /// `text(sep=..., br=...)`: the text nodes are kept apart (`None` = a `<br>`) and joined when the element closes
    spec: Option<TextSpec>,
    own_texts: bool,
    pieces: Vec<Option<String>>,
    /// depth of the `without` element whose subtree is being skipped
    skip: Option<usize>,
}

impl Collector {
    fn new(depth: usize, target: Target, take: &Take) -> Collector {
        let (spec, own_texts) = match take {
            Take::TextOpts(s) => (Some(s.clone()), false),
            Take::OwnTexts => (None, true),
            _ => (None, false),
        };
        Collector { depth, target, buf: String::new(), spec, own_texts, pieces: Vec::new(), skip: None }
    }

    fn without(&self) -> Option<usize> {
        self.spec.as_ref().and_then(|s| s.without)
    }
}

struct Frame<'a> {
    name: &'a str,
    attrs: &'a str,
    index: usize,
    children: usize,
    legends: u32,
    first_legend: bool,
    sib_idx: usize,
}

/// An open element seen through the CSS matcher (ancestors only: a stream cannot look back at siblings).
#[derive(Clone, Copy)]
struct SView<'s, 'a> {
    stack: &'s [Frame<'a>],
    sib_tags: &'s [(usize, &'a str)],
    i: usize,
    /// stack depth of the `:scope` element: the open record while its fields are matched, `1` (the top-level
    /// element, i.e. the document's root) everywhere else; this is what `Elem::scope` is in the `lite` engine
    scope_depth: usize,
}

impl<'s, 'a> ElemView for SView<'s, 'a> {
    fn name(&self) -> &str {
        self.stack[self.i].name
    }
    fn attr(&self, key: &str) -> Option<Cow<'_, str>> {
        tokenizer::find_attr(self.stack[self.i].attrs, key).map(entities::decode_attr)
    }
    fn parent(&self) -> Option<Self> {
        if self.i > 0 {
            Some(SView { stack: self.stack, sib_tags: self.sib_tags, i: self.i - 1, scope_depth: self.scope_depth })
        } else {
            None
        }
    }
    fn is_scope(&self) -> bool {
        self.i + 1 == self.scope_depth
    }
    fn prev_sibling(&self) -> Option<Self> {
        None // rejected at compile time (Selector::stream_unsupported)
    }
    fn child_index(&self) -> usize {
        self.stack[self.i].index
    }
    fn type_index(&self) -> usize {
        let me = self.stack[self.i].name;
        let mut idx = self.stack[self.i].sib_idx;
        let mut count = 1usize;
        while idx > 0 {
            idx -= 1;
            let (d, tag) = self.sib_tags[idx];
            if d < self.i {
                break;
            }
            if d == self.i && tag.eq_ignore_ascii_case(me) {
                count += 1;
            }
        }
        count
    }
    fn is_first_legend_of_parent(&self) -> bool {
        self.stack[self.i].first_legend
    }
    fn is_last_child(&self) -> bool {
        false
    }
    fn has_no_children(&self) -> bool {
        false
    }
    // `:has` / `:contains` need the tree: such selectors are rejected for the stream engine at compile time
    fn children(&self, _out: &mut Vec<Self>) {}
    fn next_sibling(&self) -> Option<Self> {
        None
    }
    fn contains_text(&self, _needle: &str) -> bool {
        false
    }
}

/// Apply a capture to a freshly opened element. Returns `true` if a text collector must start.
fn apply_take(cap: &Cap, st: &mut CapState, attrs: &str) -> bool {
    match &cap.take {
        Take::Attr(a) => {
            if !st.done {
                st.done = true;
                st.val = match tokenizer::find_attr(attrs, a) {
                    Some(v) => Value::Str(entities::decode_attr(v).into_owned()),
                    None => Value::Null,
                };
            }
            false
        }
        Take::HasAttr(a) => {
            if !st.done {
                st.done = true;
                st.val = Value::Bool(tokenizer::find_attr(attrs, a).is_some());
            }
            false
        }
        Take::Exists => {
            st.done = true;
            false
        }
        Take::Count => {
            st.count += 1;
            false
        }
        Take::Text | Take::TextOpts(_) | Take::OwnTexts => {
            if st.done {
                false
            } else {
                st.done = true;
                true
            }
        }
    }
}

/// `id` / `class` of the element being opened, read lazily: only if some selector actually has to be checked.
struct Lazy<'a> {
    attrs: &'a str,
    parsed: bool,
    id: Option<&'a str>,
    class: Option<&'a str>,
}

impl<'a> Lazy<'a> {
    fn ensure(&mut self) {
        if self.parsed {
            return;
        }
        self.parsed = true;
        for (k, v) in tokenizer::attrs(self.attrs) {
            // HTML5: the FIRST occurrence of a duplicated attribute wins (same as `find_attr`)
            if self.class.is_none() && k.eq_ignore_ascii_case("class") {
                self.class = Some(v.unwrap_or(""));
            } else if self.id.is_none() && k.eq_ignore_ascii_case("id") {
                self.id = Some(v.unwrap_or(""));
            }
            if self.class.is_some() && self.id.is_some() {
                break;
            }
        }
    }
}

/// Memo of selector results per element: a selector is evaluated at most once per element, and only when an
/// unfinished capture asks for it (outside records, and once a field is done, most selectors are never evaluated).
struct Matcher {
    epoch: u32,
    seen_at: Vec<u32>,
    /// the scope a cached result was computed for (it matters for `:scope` selectors only, but is cheap to compare)
    scope_at: Vec<usize>,
    matched: Vec<bool>,
}

impl Matcher {
    fn begin(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }

    fn check<'f>(&mut self, sels: &[Selector], si: usize, stack: &[Frame<'f>], sib_tags: &[(usize, &'f str)], name: &str, lazy: &mut Lazy<'f>, scope: usize) -> bool {
        if self.seen_at[si] != self.epoch || self.scope_at[si] != scope {
            self.seen_at[si] = self.epoch;
            self.scope_at[si] = scope;
            lazy.ensure();
            let sel = &sels[si];
            let view = SView { stack, sib_tags, i: stack.len() - 1, scope_depth: scope };
            self.matched[si] = sel.may_match(name, lazy.id, lazy.class) && sel.matches(&view);
        }
        self.matched[si]
    }
}

/// Does this capture still want to see matches? (`count` always does; the others only until they are done.)
#[inline]
fn needs_match(cap: &Cap, st: &CapState) -> bool {
    matches!(cap.take, Take::Count) || !st.done
}

struct Run<'p, 'a> {
    plan: &'p StreamPlan,
    stack: Vec<Frame<'a>>,
    root_children: usize,
    root_legends: u32,
    sib_tags: Vec<(usize, &'a str)>,
    matcher: Matcher,
    state: Vec<TopState>,
    collectors: Vec<Collector>,
    /// captures that still have to find their (first) match before an early exit is possible
    remaining: usize,
    /// the first `<base href>` seen (`T.url()` resolves against it unless `meta["base_url"]` is given)
    base_href: Option<String>,
    base_seen: bool,
    /// `<body>` has opened: the `<head>` (and its `<base>`) is behind us
    seen_body: bool,
}

impl<'p, 'a> Run<'p, 'a> {
    fn on_open(&mut self, name: &'a str, attrs: &'a str) {
        if !self.seen_body && name.len() == 4 && name.eq_ignore_ascii_case("body") {
            self.seen_body = true;
        }
        if self.plan.prog.has_meta && !self.seen_body && name.len() == 4 && name.eq_ignore_ascii_case("meta") {
            self.on_meta(attrs);
        }
        if !self.base_seen && name.len() == 4 && name.eq_ignore_ascii_case("base") {
            if let Some(h) = tokenizer::find_attr(attrs, "href") {
                self.base_seen = true;
                let h = entities::decode_attr(h);
                let h = h.trim();
                if !h.is_empty() {
                    self.base_href = Some(h.to_string());
                }
            }
        }
        let is_legend = name.len() == 6 && name.eq_ignore_ascii_case("legend");
        let (index, first_legend) = match self.stack.last_mut() {
            Some(p) => {
                p.children += 1;
                let fl = if is_legend {
                    p.legends += 1;
                    p.legends == 1
                } else {
                    false
                };
                (p.children, fl)
            }
            None => {
                self.root_children += 1;
                let fl = if is_legend {
                    self.root_legends += 1;
                    self.root_legends == 1
                } else {
                    false
                };
                (self.root_children, fl)
            }
        };
        let sib_idx = if self.plan.prog.needs_type_index {
            let i = self.sib_tags.len();
            self.sib_tags.push((self.stack.len(), name));
            i
        } else {
            0
        };
        self.stack.push(Frame { name, attrs, index, children: 0, legends: 0, first_legend, sib_idx });
        let depth = self.stack.len();
        self.matcher.begin();
        let plan = self.plan;
        let sels = &plan.base.sels;
        let mut lazy = Lazy { attrs, parsed: attrs.is_empty(), id: None, class: None };
        // `text(without=...)`: an element matching it starts a subtree the asking collectors leave out
        for ci in 0..self.collectors.len() {
            let (w, cdepth, skipping) = {
                let c = &self.collectors[ci];
                (c.without(), c.depth, c.skip.is_some())
            };
            if let Some(si) = w {
                if !skipping && cdepth < depth && self.matcher.check(sels, si, &self.stack, &self.sib_tags, name, &mut lazy, cdepth) {
                    self.collectors[ci].skip = Some(depth);
                }
            }
        }
        if name.eq_ignore_ascii_case("br") {
            for c in &mut self.collectors {
                if c.depth < depth && c.skip.is_none() && c.spec.as_ref().map_or(false, |s| (s.deep || depth == c.depth + 1) && s.br.is_some()) {
                    c.pieces.push(None);
                }
            }
        }
        for (ti, top) in plan.prog.tops.iter().enumerate() {
            match top {
                Top::Meta { .. } => {}
                Top::Field(f) => {
                    if let FieldKind::Cap(cap) = &f.kind {
                        if let Source::Sel(si) = cap.src {
                            if let TopState::Field(st) = &mut self.state[ti] {
                                if needs_match(cap, st) && self.matcher.check(sels, si, &self.stack, &self.sib_tags, name, &mut lazy, 1) {
                                    let was_done = st.done;
                                    if apply_take(cap, st, attrs) {
                                        self.collectors.push(Collector::new(depth, Target::Top(ti), &cap.take));
                                    }
                                    if !was_done && st.done && self.remaining > 0 {
                                        self.remaining -= 1;
                                    }
                                }
                            }
                        }
                    }
                }
                Top::List { item, body, .. } => {
                    let fields: &[Field] = match body {
                        ListBody::Record(fs) => fs.as_slice(),
                        ListBody::Single(f) => std::slice::from_ref(f),
                    };
                    if let TopState::List(ls) = &mut self.state[ti] {
                        let stride = ls.stride;
                        // 1) selectors relative to records that are already open
                        for &(ri, rd) in &ls.open {
                            if rd >= depth {
                                continue;
                            }
                            let base = ri * stride;
                            for (fi, f) in fields.iter().enumerate() {
                                if let FieldKind::Cap(cap) = &f.kind {
                                    if let Source::Sel(si) = cap.src {
                                        if needs_match(cap, &ls.vals[base + fi])
                                            && self.matcher.check(sels, si, &self.stack, &self.sib_tags, name, &mut lazy, rd)
                                        {
                                            let st = &mut ls.vals[base + fi];
                                            if apply_take(cap, st, attrs) {
                                                self.collectors.push(Collector::new(depth, Target::Rec { top: ti, rec: ri, field: fi }, &cap.take));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // 2) this element starts a new record
                        if self.matcher.check(sels, *item, &self.stack, &self.sib_tags, name, &mut lazy, 1) {
                            let ri = ls.len;
                            ls.len += 1;
                            if stride > 0 {
                                ls.vals.resize(ls.len * stride, CapState::default());
                            }
                            ls.open.push((ri, depth));
                            let base = ri * stride;
                            for (fi, f) in fields.iter().enumerate() {
                                if let FieldKind::Cap(cap) = &f.kind {
                                    if let Source::Own = cap.src {
                                        let st = &mut ls.vals[base + fi];
                                        if apply_take(cap, st, attrs) {
                                            self.collectors.push(Collector::new(depth, Target::Rec { top: ti, rec: ri, field: fi }, &cap.take));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[inline]
    fn can_stop(&self) -> bool {
        self.plan.prog.stoppable
            && self.remaining == 0
            && self.collectors.is_empty()
            // `url()` without an explicit base: wait for the `<base href>` or the start of the body
            && (!self.plan.prog.needs_base || self.base_seen || self.seen_body)
            // `meta_tags` reads the head: stop only once the body has started
            && (!self.plan.prog.has_meta || self.seen_body)
    }

    /// A `<meta property|name content>` in the head: add it to every `meta_tags` top (same key rules as the DOM engines).
    fn on_meta(&mut self, attrs: &'a str) {
        let content = match tokenizer::find_attr(attrs, "content") {
            Some(c) => c,
            None => return,
        };
        let pick = |k: &str| tokenizer::find_attr(attrs, k).map(|v| entities::decode_attr(v)).filter(|v| !v.trim().is_empty());
        let key = match pick("property").or_else(|| pick("name")) {
            Some(k) => k.trim().to_ascii_lowercase(),
            None => return,
        };
        let content = entities::decode_attr(content).into_owned();
        let plan = self.plan;
        for (top, st) in plan.prog.tops.iter().zip(self.state.iter_mut()) {
            if let (Top::Meta { prefixes, mode, .. }, TopState::Meta(m)) = (top, st) {
                if prefixes.is_empty() || prefixes.iter().any(|p| key.starts_with(p.as_str())) {
                    crate::exec::meta_insert(m, key.clone(), content.clone(), *mode);
                }
            }
        }
    }

    fn on_text(&mut self, text: &'a str, decode: bool) {
        if self.collectors.is_empty() {
            return;
        }
        // same rule as the DOM engines: <script>/<style> content is not part of an ancestor's text
        let depth = self.stack.len();
        let in_raw = !decode && self.stack.last().map_or(false, |f| f.name.eq_ignore_ascii_case("script") || f.name.eq_ignore_ascii_case("style"));
        let t: Cow<str> = if decode { entities::decode(text) } else { Cow::Borrowed(text) };
        for c in &mut self.collectors {
            if (in_raw && c.depth < depth) || c.skip.is_some() {
                continue;
            }
            if (c.own_texts || c.spec.as_ref().map_or(false, |s| !s.deep)) && c.depth != depth {
                continue;
            }
            if c.own_texts || c.spec.is_some() {
                c.pieces.push(Some(t.to_string()));
            } else {
                c.buf.push_str(&t);
            }
        }
    }

    fn on_close(&mut self) {
        let depth = self.stack.len();
        for c in &mut self.collectors {
            if c.skip == Some(depth) {
                c.skip = None;
            }
        }
        while let Some(pos) = self.collectors.iter().position(|c| c.depth == depth) {
            let c = self.collectors.swap_remove(pos);
            let val = if c.own_texts {
                Value::List(c.pieces.into_iter().flatten().map(Value::Str).collect())
            } else {
                Value::Str(match &c.spec {
                    Some(s) => crate::exec::join_pieces(&c.pieces, &s.sep, s.strip, s.br.as_deref()),
                    None => c.buf,
                })
            };
            match c.target {
                Target::Top(ti) => {
                    if let TopState::Field(st) = &mut self.state[ti] {
                        st.val = val;
                    }
                }
                Target::Rec { top, rec, field } => {
                    if let TopState::List(ls) = &mut self.state[top] {
                        let stride = ls.stride;
                        ls.vals[rec * stride + field].val = val;
                    }
                }
            }
        }
        for st in &mut self.state {
            if let TopState::List(ls) = st {
                while let Some(&(_, d)) = ls.open.last() {
                    if d == depth {
                        ls.open.pop();
                    } else {
                        break;
                    }
                }
            }
        }
        while let Some(&(d, _)) = self.sib_tags.last() {
            if d >= depth {
                self.sib_tags.pop();
            } else {
                break;
            }
        }
        self.stack.pop();
    }

    /// `idx` is the record index inside a list (`None` outside lists): `T.index()` anywhere in the field,
    /// also inside `default(...)` / `if_(...)` of the tail, sees it like in the DOM engines.
    fn field_value(
        &self,
        f: &Field,
        st: CapState,
        idx: Option<usize>,
        meta: &Value,
        json_cache: &mut JsonCache,
    ) -> Result<Value> {
        let raw = match &f.kind {
            FieldKind::Lit(v) => v.clone(),
            FieldKind::Index => idx.map(|i| Value::Int(i as i64)).unwrap_or(Value::Null),
            FieldKind::Pure => Value::Null,
            FieldKind::Cap(c) => match c.take {
                Take::Count => Value::Int(st.count),
                Take::Exists => Value::Bool(st.done),
                Take::Text | Take::TextOpts(_) | Take::OwnTexts | Take::Attr(_) => st.val,
                // no matching element => `false` (same as the DOM engines), not null
                Take::HasAttr(_) => {
                    if st.done {
                        st.val
                    } else {
                        Value::Bool(false)
                    }
                }
            },
        };
        if f.tail.is_empty() {
            return Ok(raw);
        }
        // the error path is only built when something fails (no allocations on the success path)
        self.plan
            .tails
            .run_tail(&NoDom, &f.tail, raw, meta, idx, Vec::new(), self.base_href.as_deref(), json_cache)
    }

    fn finish(mut self, meta: &Value) -> Result<(Value, Vec<ErrorInfo>)> {
        let plan = self.plan;
        let states = std::mem::take(&mut self.state);
        let mut out: IndexMap<String, Value> = IndexMap::new();
        let mut errors: Vec<ErrorInfo> = Vec::new();
        let mut json_cache: JsonCache = None;
        for (top, st) in plan.prog.tops.iter().zip(states) {
            match (top, st) {
                (Top::Field(f), TopState::Field(cs)) => {
                    let v = self
                        .field_value(f, cs, None, meta, &mut json_cache)
                        .map_err(|e| e.prefixed(&[f.name.clone()]))?;
                    out.insert(f.name.clone(), v);
                }
                (Top::Meta { name, tail, .. }, TopState::Meta(m)) => {
                    let raw = Value::Map(m);
                    let v = if tail.is_empty() {
                        raw
                    } else {
                        plan.tails
                            .run_tail(&NoDom, tail, raw, meta, None, Vec::new(), self.base_href.as_deref(), &mut json_cache)
                            .map_err(|e| e.prefixed(&[name.clone()]))?
                    };
                    out.insert(name.clone(), v);
                }
                (Top::List { name, body, tail, mode, .. }, TopState::List(ls)) => {
                    let mut items = Vec::with_capacity(ls.len);
                    let mut vals_iter = ls.vals.into_iter();
                    for ri in 0..ls.len {
                        let at = |field: Option<&str>| {
                            let mut p = vec![name.clone(), format!("[{ri}]")];
                            if let Some(f) = field {
                                p.push(f.to_string());
                            }
                            p
                        };
                        let built: Result<Value> = match body {
                            ListBody::Record(fs) => {
                                let mut m: IndexMap<String, Value> = IndexMap::with_capacity(fs.len());
                                let mut failed: Option<Error> = None;
                                for f in fs {
                                    let cs = vals_iter.next().unwrap_or_default();
                                    if failed.is_none() {
                                        match self.field_value(f, cs, Some(ri), meta, &mut json_cache) {
                                            Ok(v) => {
                                                m.insert(f.name.clone(), v);
                                            }
                                            Err(e) => {
                                                failed = Some(e.prefixed(&at(Some(&f.name))));
                                            }
                                        }
                                    }
                                }
                                match failed {
                                    Some(e) => Err(e),
                                    None => Ok(Value::Map(m)),
                                }
                            }
                            ListBody::Single(f) => {
                                let cs = vals_iter.next().unwrap_or_default();
                                self.field_value(f, cs, Some(ri), meta, &mut json_cache).map_err(|e| e.prefixed(&at(None)))
                            }
                        };
                        match built {
                            Ok(v) => items.push(v),
                            // quarantine the record: report it and carry on (same as `map` in the DOM engines)
                            Err(Error::Exec { path, msg }) if *mode != OnError::Raise => {
                                errors.push(ErrorInfo { path, message: msg });
                                if *mode == OnError::Null {
                                    items.push(Value::Null);
                                }
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    let list = Value::List(items);
                    let v = if tail.is_empty() {
                        list
                    } else {
                        plan.tails
                            .run_tail(&NoDom, tail, list, meta, None, Vec::new(), self.base_href.as_deref(), &mut json_cache)
                            .map_err(|e| e.prefixed(&[name.clone()]))?
                    };
                    out.insert(name.clone(), v);
                }
                _ => {}
            }
        }
        Ok((Value::Map(out), errors))
    }
}

impl Engine for StreamPlan {
    fn start(&self, src: &str, meta: Value) -> Result<Session> {
        let (value, errors) = self.run_detailed(src, &meta)?;
        Ok(Session::from_parts(value, errors))
    }
    fn resume(&self, _sess: &mut Session, _docs: &[(u64, String)]) -> Result<()> {
        Err(Error::Session("the stream engine has no follow requests".into()))
    }
    fn explain(&self) -> String {
        format!("{}\n{}", self.describe(), self.base.explain())
    }
    fn ir(&self) -> &Expr {
        self.base.ir()
    }
    fn backend(&self) -> &'static str {
        "stream"
    }
}
