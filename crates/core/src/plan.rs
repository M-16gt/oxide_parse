//! IR -> compiled plan. Selectors and regexes are compiled once (and de-duplicated),
//! then a small peephole optimiser rewrites pipelines.
use crate::adaptive::{self, Store};
use crate::dom::Dom;
use crate::error::{fmt_path, Error, Result};
use crate::ir::{Expr, OnError, Op};
use crate::value::Value;
use crate::xpath::{self, XPathExpr};
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;
use std::fmt::Write;

#[derive(Debug, Clone)]
pub(crate) enum CExpr {
    Pipe(Vec<COp>),
    Object(Vec<(String, CExpr)>),
    List(Vec<CExpr>),
    Lit(Value),
}

/// One step of a fused `parse_json()[...][...]` chain.
#[derive(Debug, Clone)]
pub(crate) enum JsonSeg {
    Key(String),
    At(usize),
}

#[derive(Debug, Clone)]
pub(crate) enum COp {
    Select(usize),
    XPath(usize),
    SelectFirst(usize),
    SelectExists(usize),
    /// `multi`: a `map` follows, so a relocation after a redesign must find the whole list of records
    Adaptive { sel: usize, all: usize, key: String, threshold: f64, multi: bool },
    Parent,
    Children,
    Root,
    Key(String),
    At(usize),
    Slice { start: Option<i64>, stop: Option<i64> },
    First,
    Last,
    Attr(String),
    HasAttr(String),
    Text,
    TextOpts { sep: String, strip: bool, without: Option<usize>, br: Option<String>, deep: bool },
    OwnTexts,
    Table { mode: u8, strip: bool, header_sep: String },
    Markdown { without: Option<usize> },
    Distill { keep: usize, max_text: usize, max_chars: usize },
    Html,
    InnerHtml,
    Meta(String),
    Index,
    Count,
    Exists,
    Map(CExpr),
    /// `map` with an error policy
    MapSafe(CExpr, OnError),
    ResolveUrl(Option<CExpr>),
    JsJson(Option<String>),
    Sections { header: usize, items: Option<usize> },
    /// only exists between compilation steps: `group_by | map` is fused into `GroupMap`
    GroupBy { header: usize, items: Option<usize> },
    GroupMap { header: usize, items: Option<usize>, body: CExpr, mode: crate::ir::OnError },
    JsonLd { sel: usize, schema_type: Option<String>, resolve_refs: bool },
    Microdata { sel: usize, schema_type: Option<String> },
    NextFlight { sel: Option<usize>, key: Option<String> },
    MainContent,
    /// `mode`: 0 = auto, 1 = first, 2 = list
    MetaTags { sel: usize, prefixes: Vec<String>, mode: u8 },
    FlatMap(CExpr),
    Filter(CExpr),
    Unique { by: Option<CExpr>, keep_last: bool },
    Default(CExpr),
    Required,
    NonEmpty,
    /// evaluate an expression against the context (internal: used to move ops behind `follow` into its schema)
    Run(CExpr),
    Coalesce(Vec<CExpr>),
    Strip,
    Lower,
    Upper,
    Replace(String, String),
    Split(String),
    Join(String),
    Regex(Regex, usize),
    RegexAll(Regex, usize),
    Int,
    Float,
    Bool,
    ParseSrcset { pick: Option<String>, min_width: Option<usize>, min_density: Option<f64> },
    Price { decimal: Option<String>, currency: Option<String> },
    ParseJson,
    /// `parse_json` followed by `key` / `at` steps: navigates the parsed document by reference and converts only
    /// the selected leaf (the parsed text is cached per run, see `Eval::json_doc`)
    JsonPath(Vec<JsonSeg>),
    /// Parse an HTML string fragment with a standalone compiled plan (`format="auto"`).
    ParseHtml(usize),
    /// Parse an HTTP `Link` header string (RFC 8288).
    LinkHeader(Option<String>),
    Eq(CExpr),
    Ne(CExpr),
    Contains(CExpr),
    StartsWith(CExpr),
    EndsWith(CExpr),
    Not,
    And(CExpr),
    Or(CExpr),
    If(CExpr, CExpr, CExpr),
    Follow {
        request: CExpr,
        meta: CExpr,
        sub: usize,
        /// operations that followed `follow(...)` in the pipeline: they moved into the sub-schema, but when no
        /// request is made (`request` is null) they still have to run, on `null` (`.default("-")`, `.required()`)
        on_null: Vec<COp>,
    },
    FollowForeign {
        request: CExpr,
        meta: CExpr,
        foreign: usize,
        on_null: Vec<COp>,
    },
    Paginate {
        pager: usize,
    },
}

pub(crate) struct ForeignSpec {
    pub(crate) engine: Arc<dyn crate::engine::Engine>,
    pub(crate) post: Vec<COp>,
}

#[derive(Debug, Clone)]
pub(crate) struct PaginateSpec {
    pub(crate) items: CExpr,
    pub(crate) next: CExpr,
    pub(crate) meta: CExpr,
    pub(crate) max_pages: Option<usize>,
    pub(crate) max_items: Option<usize>,
    pub(crate) unique_by: Option<CExpr>,
    pub(crate) keep_last: bool,
    pub(crate) post: Vec<COp>,
}

/// A compiled, immutable, thread-safe extraction plan. `S` is the backend's selector type.
pub struct Plan<S> {
    pub(crate) sels: Vec<S>,
    pub(crate) sel_src: Vec<String>,
    pub(crate) xpaths: Vec<XPathExpr>,
    pub(crate) root: CExpr,
    pub(crate) subs: Vec<CExpr>,
    pub(crate) html_subs: Vec<Arc<dyn crate::engine::Engine>>,
    pub(crate) foreign_specs: Vec<ForeignSpec>,
    pub(crate) pagers: Vec<PaginateSpec>,
    pub(crate) notes: Vec<String>,
    pub(crate) ir: Expr,
    pub(crate) backend: &'static str,
    pub(crate) store: Arc<dyn Store>,
}

impl Plan<()> {
    /// A plan with no program; used to run value-only operations (see the stream engine).
    pub(crate) fn empty() -> Plan<()> {
        Plan {
            sels: Vec::new(),
            sel_src: Vec::new(),
            xpaths: Vec::new(),
            root: CExpr::Lit(Value::Null),
            subs: Vec::new(),
            html_subs: Vec::new(),
            foreign_specs: Vec::new(),
            pagers: Vec::new(),
            notes: Vec::new(),
            ir: Expr::Lit(serde_json::Value::Null),
            backend: "none",
            store: adaptive::default_store(),
        }
    }
}

impl<S> Plan<S> {
    /// Compile `ir` for backend `D`. Fails on bad selectors/regexes or unsupported features.
    pub fn compile<D: Dom<Sel = S>>(ir: &Expr) -> Result<Self> {
        Self::compile_with::<D>(ir, None)
    }

    /// Like [`Plan::compile`] with a custom fingerprint store for `adaptive` selectors
    /// (default: an in-memory store owned by the plan).
    pub fn compile_with<D: Dom<Sel = S>>(ir: &Expr, store: Option<Arc<dyn Store>>) -> Result<Self> {
        Self::compile_with_backend::<D>(ir, store, D::NAME)
    }

    pub(crate) fn compile_with_backend<D: Dom<Sel = S>>(
        ir: &Expr,
        store: Option<Arc<dyn Store>>,
        backend_name: &'static str,
    ) -> Result<Self> {
        let mut c = Compiler::<D> {
            sels: Vec::new(),
            sel_src: Vec::new(),
            xpaths: Vec::new(),
            seen: HashMap::new(),
            subs: Vec::new(),
            html_subs: Vec::new(),
            foreign_specs: Vec::new(),
            pagers: Vec::new(),
            notes: Vec::new(),
            path: Vec::new(),
            backend_name,
        };
        let root = c.expr(ir)?;
        Ok(Plan {
            sels: c.sels,
            sel_src: c.sel_src,
            xpaths: c.xpaths,
            root,
            subs: c.subs,
            html_subs: c.html_subs,
            foreign_specs: c.foreign_specs,
            pagers: c.pagers,
            notes: c.notes,
            ir: ir.clone(),
            backend: backend_name,
            store: store.unwrap_or_else(adaptive::default_store),
        })
    }

    pub fn ir(&self) -> &Expr {
        &self.ir
    }

    pub fn backend(&self) -> &'static str {
        self.backend
    }

    /// Human-readable description of the compiled plan.
    pub fn explain(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "Plan (backend: {})", self.backend);
        let _ = writeln!(out, "selectors: {} unique", self.sel_src.len());
        for (i, s) in self.sel_src.iter().enumerate() {
            let _ = writeln!(out, "  #{i} css {s:?}");
        }
        if !self.xpaths.is_empty() {
            let _ = writeln!(out, "xpath expressions: {}", self.xpaths.len());
            for (i, x) in self.xpaths.iter().enumerate() {
                let _ = writeln!(out, "  x{i} {:?}", x.source());
            }
        }
        let _ = writeln!(out, "follow sub-plans: {}", self.subs.len());
        if !self.foreign_specs.is_empty() {
            let _ = writeln!(out, "foreign follow sub-plans: {}", self.foreign_specs.len());
        }
        if !self.pagers.is_empty() {
            let _ = writeln!(out, "paginate plans: {}", self.pagers.len());
        }
        let _ = writeln!(out, "optimisations applied: {}", self.notes.len());
        for n in &self.notes {
            let _ = writeln!(out, "  - {n}");
        }
        let _ = writeln!(out, "root:");
        self.fmt_expr(&self.root, 1, &mut out);
        for (i, s) in self.subs.iter().enumerate() {
            let _ = writeln!(out, "sub-plan {i}:");
            self.fmt_expr(s, 1, &mut out);
        }
        for (i, p) in self.pagers.iter().enumerate() {
            let _ = writeln!(out, "pager {i} (max_pages={:?}, max_items={:?}):", p.max_pages, p.max_items);
            let _ = writeln!(out, "  items:");
            self.fmt_expr(&p.items, 2, &mut out);
            let _ = writeln!(out, "  next:");
            self.fmt_expr(&p.next, 2, &mut out);
        }
        out
    }

    fn sel_label(&self, name: &str, i: usize) -> String {
        format!("{name}[css {:?} #{i}]", self.sel_src[i])
    }

    fn label(&self, op: &COp) -> String {
        match op {
            COp::Select(i) => self.sel_label("select", *i),
            COp::Adaptive { sel, key, .. } => format!("adaptive[css {:?} key={key:?}]", self.sel_src[*sel]),
            COp::XPath(i) => format!("xpath[{:?}]", self.xpaths[*i].source()),
            COp::SelectFirst(i) => self.sel_label("select_first", *i),
            COp::SelectExists(i) => self.sel_label("select_exists", *i),
            COp::Parent => "parent".into(),
            COp::Children => "children".into(),
            COp::Root => "root".into(),
            COp::Key(k) => format!("key({k:?})"),
            COp::At(i) => format!("at({i})"),
            COp::Slice { start, stop } => format!("slice({start:?}, {stop:?})"),
            COp::First => "first".into(),
            COp::Last => "last".into(),
            COp::Attr(a) => format!("attr({a:?})"),
            COp::HasAttr(a) => format!("has_attr({a:?})"),
            COp::TextOpts { sep, strip, without, br, deep } => {
                format!("text(sep={sep:?}, strip={strip}, without={without:?}, br={br:?}, deep={deep})")
            }
            COp::OwnTexts => "own_texts".into(),
            COp::Table { mode, strip, .. } => format!("table(mode={mode}, strip={strip})"),
            COp::NonEmpty => "non_empty".into(),
            COp::Run(_) => "run".into(),
            COp::ParseJson => "parse_json".into(),
            COp::JsonPath(segs) => format!("json_path({} steps)", segs.len()),
            COp::ParseHtml(i) => format!("parse_html(backend={})", self.html_subs[*i].backend()),
            COp::LinkHeader(rel) => format!("link_header({rel:?})"),
            COp::Text => "text".into(),
            COp::Markdown { without } => format!("markdown(without={without:?})"),
            COp::Distill { keep, max_text, max_chars } => format!("distill({keep}, {max_text}, {max_chars})"),
            COp::Html => "html".into(),
            COp::InnerHtml => "inner_html".into(),
            COp::Meta(k) => format!("meta({k:?})"),
            COp::Index => "index".into(),
            COp::Count => "count".into(),
            COp::Exists => "exists".into(),
            COp::Map(_) => "map".into(),
            COp::MapSafe(_, mode) => format!("map(on_error={mode:?})"),
            COp::ResolveUrl(_) => "resolve_url".into(),
            COp::JsJson(v) => format!("js_json({v:?})"),
            COp::Sections { header, .. } => format!("sections(header=#{header})"),
            COp::GroupBy { header, .. } => format!("group_by(header=#{header}) (needs map)"),
            COp::GroupMap { header, mode, .. } => format!("group_by(header=#{header}).map(on_error={mode:?})"),
            COp::JsonLd { schema_type, resolve_refs, .. } => format!("json_ld({schema_type:?}, resolve_refs={resolve_refs})"),
            COp::Microdata { schema_type, .. } => format!("microdata({schema_type:?})"),
            COp::NextFlight { key, .. } => format!("next_flight({key:?})"),
            COp::MainContent => "main_content".into(),
            COp::MetaTags { prefixes, mode, .. } => format!("meta_tags({prefixes:?}, mode={mode})"),
            COp::FlatMap(_) => "flat_map".into(),
            COp::Filter(_) => "filter".into(),
            COp::Unique { keep_last, .. } => format!("unique(keep={})", if *keep_last { "last" } else { "first" }),
            COp::Default(_) => "default".into(),
            COp::Required => "required".into(),
            COp::Coalesce(_) => "coalesce".into(),
            COp::Strip => "strip".into(),
            COp::Lower => "lower".into(),
            COp::Upper => "upper".into(),
            COp::Replace(a, b) => format!("replace({a:?} -> {b:?})"),
            COp::Split(s) => format!("split({s:?})"),
            COp::Join(s) => format!("join({s:?})"),
            COp::Regex(r, g) => format!("regex({:?}, group={g})", r.as_str()),
            COp::RegexAll(r, g) => format!("regex_all({:?}, group={g})", r.as_str()),
            COp::Int => "int".into(),
            COp::Float => "float".into(),
            COp::Bool => "bool".into(),
            COp::ParseSrcset { pick, .. } => format!("parse_srcset(pick={pick:?})"),
            COp::Price { decimal, currency } => format!("price(decimal={decimal:?}, currency={currency:?})"),
            COp::Eq(_) => "eq".into(),
            COp::Ne(_) => "ne".into(),
            COp::Contains(_) => "contains".into(),
            COp::StartsWith(_) => "starts_with".into(),
            COp::EndsWith(_) => "ends_with".into(),
            COp::Not => "not".into(),
            COp::And(_) => "and".into(),
            COp::Or(_) => "or".into(),
            COp::If(..) => "if".into(),
            COp::Follow { sub, .. } => format!("follow -> sub-plan {sub}"),
            COp::FollowForeign { foreign, .. } => format!("follow(format=foreign) -> sub-plan {foreign}"),
            COp::Paginate { pager } => format!("paginate -> pager {pager}"),
        }
    }

    fn nested<'a>(op: &'a COp) -> Vec<(&'static str, &'a CExpr)> {
        match op {
            COp::Map(e) | COp::FlatMap(e) => vec![("body", e)],
            COp::Filter(e) => vec![("predicate", e)],
            COp::Unique { by: Some(e), .. } => vec![("by", e)],
            COp::Default(e) => vec![("fallback", e)],
            COp::Run(e) => vec![("schema", e)],
            COp::MapSafe(e, _) => vec![("body", e)],
            COp::GroupMap { body, .. } => vec![("body", body)],
            COp::ResolveUrl(Some(e)) => vec![("base", e)],
            COp::Eq(e) | COp::Ne(e) | COp::Contains(e) | COp::StartsWith(e) | COp::EndsWith(e) => vec![("rhs", e)],
            COp::And(e) | COp::Or(e) => vec![("rhs", e)],
            COp::If(c, t, o) => vec![("cond", c), ("then", t), ("otherwise", o)],
            COp::Coalesce(es) => es.iter().map(|e| ("alt", e)).collect(),
            COp::Follow { request, meta, .. } | COp::FollowForeign { request, meta, .. } => {
                vec![("request", request), ("meta", meta)]
            }
            _ => vec![],
        }
    }

    fn fmt_expr(&self, e: &CExpr, ind: usize, out: &mut String) {
        let pad = "  ".repeat(ind);
        match e {
            CExpr::Lit(v) => {
                let _ = writeln!(out, "{pad}lit {v:?}");
            }
            CExpr::Object(fs) => {
                let _ = writeln!(out, "{pad}object");
                for (k, x) in fs {
                    let _ = writeln!(out, "{pad}  {k}:");
                    self.fmt_expr(x, ind + 2, out);
                }
            }
            CExpr::List(xs) => {
                let _ = writeln!(out, "{pad}list");
                for x in xs {
                    self.fmt_expr(x, ind + 1, out);
                }
            }
            CExpr::Pipe(ops) => {
                let line = ops.iter().map(|o| self.label(o)).collect::<Vec<_>>().join(" | ");
                let _ = writeln!(out, "{pad}{}", if line.is_empty() { "(identity)".to_string() } else { line });
                for o in ops {
                    for (name, sub) in Self::nested(o) {
                        let _ = writeln!(out, "{pad}  {name}:");
                        self.fmt_expr(sub, ind + 2, out);
                    }
                }
            }
        }
    }
}

struct Compiler<D: Dom> {
    sels: Vec<D::Sel>,
    sel_src: Vec<String>,
    xpaths: Vec<XPathExpr>,
    seen: HashMap<String, usize>,
    subs: Vec<CExpr>,
    html_subs: Vec<Arc<dyn crate::engine::Engine>>,
    foreign_specs: Vec<ForeignSpec>,
    pagers: Vec<PaginateSpec>,
    notes: Vec<String>,
    path: Vec<String>,
    backend_name: &'static str,
}

fn has_subqueries_or_adaptive(e: &Expr) -> bool {
    match e {
        Expr::Lit(_) => false,
        Expr::List(es) => es.iter().any(has_subqueries_or_adaptive),
        Expr::Object(m) => m.values().any(has_subqueries_or_adaptive),
        Expr::Pipe(ops) => ops.iter().any(|op| match op {
            Op::Follow { .. } | Op::Paginate { .. } | Op::Adaptive { .. } => true,
            Op::Map(b)
            | Op::FlatMap(b)
            | Op::Filter(b)
            | Op::Unique { by: Some(b), .. }
            | Op::Default(b)
            | Op::Eq(b)
            | Op::Ne(b)
            | Op::Contains(b)
            | Op::StartsWith(b)
            | Op::EndsWith(b)
            | Op::And(b)
            | Op::Or(b)
            | Op::ParseHtml(b) => has_subqueries_or_adaptive(b),
            Op::MapOpts { body, .. } => has_subqueries_or_adaptive(body),
            Op::ResolveUrl { base: Some(b) } => has_subqueries_or_adaptive(b),
            Op::Coalesce(es) => es.iter().any(has_subqueries_or_adaptive),
            Op::If { cond, then, otherwise } => {
                has_subqueries_or_adaptive(cond)
                    || has_subqueries_or_adaptive(then)
                    || has_subqueries_or_adaptive(otherwise)
            }
            _ => false,
        }),
    }
}

impl<D: Dom> Compiler<D> {
    fn err(&self, msg: impl Into<String>) -> Error {
        Error::Compile { path: fmt_path(&self.path), msg: msg.into() }
    }

    fn selector(&mut self, src: &str) -> Result<usize> {
        if let Some(&i) = self.seen.get(src) {
            return Ok(i);
        }
        if !D::SCOPE && src.to_ascii_lowercase().contains(":scope") {
            return Err(self.err(format!(
                "selector {src:?}: `:scope` is not supported by the `{}` engine (use the `lite` engine)",
                D::NAME
            )));
        }
        let s = D::compile_css(src).map_err(|m| self.err(format!("selector {src:?}: {m}")))?;
        self.sels.push(s);
        self.sel_src.push(src.to_string());
        let i = self.sels.len() - 1;
        self.seen.insert(src.to_string(), i);
        Ok(i)
    }

    fn xpath(&mut self, src: &str) -> Result<usize> {
        if !D::XPATH {
            return Err(self.err(format!("XPath is not supported by the `{}` engine (use scraper or lite)", D::NAME)));
        }
        let xp = xpath::parse(src).map_err(|m| self.err(format!("xpath {src:?}: {m}")))?;
        self.xpaths.push(xp);
        Ok(self.xpaths.len() - 1)
    }

    fn regex(&self, pattern: &str) -> Result<Regex> {
        Regex::new(pattern).map_err(|e| self.err(format!("invalid regex {pattern:?}: {e}")))
    }

    fn nested(&mut self, label: &str, e: &Expr) -> Result<CExpr> {
        self.path.push(label.to_string());
        let r = self.expr(e);
        self.path.pop();
        r
    }

    fn expr(&mut self, e: &Expr) -> Result<CExpr> {
        Ok(match e {
            Expr::Lit(j) => CExpr::Lit(Value::from_json(j.clone())),
            Expr::List(es) => {
                let mut out = Vec::with_capacity(es.len());
                for (i, x) in es.iter().enumerate() {
                    self.path.push(format!("[{i}]"));
                    out.push(self.expr(x)?);
                    self.path.pop();
                }
                CExpr::List(out)
            }
            Expr::Object(m) => {
                let mut out = Vec::with_capacity(m.len());
                for (k, x) in m {
                    self.path.push(k.clone());
                    out.push((k.clone(), self.expr(x)?));
                    self.path.pop();
                }
                CExpr::Object(out)
            }
            Expr::Pipe(ops) => {
                let mut out = Vec::with_capacity(ops.len());
                for op in ops {
                    out.push(self.op(op)?);
                }
                let out = self.peephole(out);
                CExpr::Pipe(self.fuse_follow_tail(out))
            }
        })
    }

    fn op(&mut self, op: &Op) -> Result<COp> {
        Ok(match op {
            Op::Css(s) => COp::Select(self.selector(s)?),
            Op::Xpath(s) => COp::XPath(self.xpath(s)?),
            Op::Adaptive { css, key, threshold } => {
                if !D::XPATH {
                    return Err(self.err(format!(
                        "adaptive matching needs an engine with node introspection (scraper or lite), not `{}`",
                        D::NAME
                    )));
                }
                COp::Adaptive { sel: self.selector(css)?, all: self.selector("*")?, key: key.clone(), threshold: threshold.unwrap_or(0.5), multi: false }
            }
            Op::Parent => COp::Parent,
            Op::Children => COp::Children,
            Op::Root => COp::Root,
            Op::Key(k) => COp::Key(k.clone()),
            Op::At(i) => COp::At(*i),
            Op::Slice { start, stop } => COp::Slice { start: *start, stop: *stop },
            Op::First => COp::First,
            Op::Last => COp::Last,
            Op::Attr(a) => COp::Attr(a.clone()),
            Op::HasAttr(a) => COp::HasAttr(a.clone()),
            Op::TextOpts { sep, strip, without, br, deep } => {
                let without = match without {
                    Some(sel) => Some(self.selector(sel)?),
                    None => None,
                };
                COp::TextOpts { sep: sep.clone(), strip: *strip, without, br: br.clone(), deep: deep.unwrap_or(true) }
            }
            Op::OwnTexts => COp::OwnTexts,
            Op::Table { mode, strip, header_sep } => COp::Table {
                mode: match mode.as_deref() {
                    None | Some("records") => 0,
                    Some("grid") => 1,
                    Some("split") => 2,
                    Some(other) => {
                        return Err(self.err(format!("table: mode must be \"records\", \"grid\", or \"split\", got {other:?}")))
                    }
                },
                strip: strip.unwrap_or(true),
                header_sep: header_sep.clone().unwrap_or_else(|| " / ".to_string()),
            },
            Op::Distill { keep, max_text, max_chars } => COp::Distill {
                keep: keep.unwrap_or(2),
                max_text: max_text.unwrap_or(80),
                max_chars: max_chars.unwrap_or(20_000),
            },
            Op::Markdown { without } => {
                let without = match without {
                    Some(sel) => Some(self.selector(sel)?),
                    None => None,
                };
                COp::Markdown { without }
            }
            Op::NonEmpty => COp::NonEmpty,
            Op::ParseJson => COp::ParseJson,
            Op::ParseHtml(sub) => {
                if has_subqueries_or_adaptive(sub) {
                    return Err(self.err("parse_html: sub-schema cannot contain follow, paginate, or adaptive"));
                }
                let eng = crate::engine::compile(sub, "auto")
                    .map_err(|e| self.err(format!("parse_html: {e}")))?;
                self.html_subs.push(Arc::from(eng));
                COp::ParseHtml(self.html_subs.len() - 1)
            }
            Op::LinkHeader { rel } => COp::LinkHeader(rel.clone()),
            Op::Text => COp::Text,
            Op::Html => COp::Html,
            Op::InnerHtml => COp::InnerHtml,
            Op::Meta(k) => COp::Meta(k.clone()),
            Op::Index => COp::Index,
            Op::Count => COp::Count,
            Op::Exists => COp::Exists,
            Op::Map(e) => COp::Map(self.nested("map", e)?),
            Op::FlatMap(e) => COp::FlatMap(self.nested("flat_map", e)?),
            Op::MapOpts { body, on_error } => COp::MapSafe(self.nested("map", body)?, *on_error),
            Op::ResolveUrl { base } => COp::ResolveUrl(match base {
                Some(b) => Some(self.nested("url.base", b)?),
                None => None,
            }),
            Op::JsJson { var } => COp::JsJson(var.clone()),
            Op::Sections { header, items } => {
                let header = self.selector(header)?;
                let items = match items {
                    Some(s) => Some(self.selector(s)?),
                    None => None,
                };
                COp::Sections { header, items }
            }
            Op::GroupBy { header, items } => {
                let header = self.selector(header)?;
                let items = match items {
                    Some(s) => Some(self.selector(s)?),
                    None => None,
                };
                COp::GroupBy { header, items }
            }
            Op::JsonLd { schema_type, resolve_refs } => {
                COp::JsonLd { sel: self.selector("script[type]")?, schema_type: schema_type.clone(), resolve_refs: resolve_refs.unwrap_or(false) }
            }
            Op::Microdata { schema_type } => {
                if !D::XPATH {
                    return Err(self.err(format!("microdata needs a DOM engine, not `{}`", D::NAME)));
                }
                COp::Microdata { sel: self.selector("[itemscope]")?, schema_type: schema_type.clone() }
            }
            Op::NextFlight { key } => {
                let sel = if D::XPATH { Some(self.selector("script")?) } else { None };
                COp::NextFlight { sel, key: key.clone() }
            }
            Op::MainContent => {
                if !D::XPATH {
                    return Err(self.err(format!("main_content needs a DOM engine, not `{}`", D::NAME)));
                }
                COp::MainContent
            }
            Op::MetaTags { prefixes, values } => COp::MetaTags {
                sel: self.selector("meta[content]")?,
                prefixes: prefixes.iter().map(|p| p.to_ascii_lowercase()).collect(),
                mode: match values.as_deref() {
                    None | Some("auto") => 0,
                    Some("first") => 1,
                    Some("list") => 2,
                    Some(other) => return Err(self.err(format!("meta_tags: values must be \"auto\", \"first\" or \"list\", got {other:?}"))),
                },
            },
            Op::Filter(e) => COp::Filter(self.nested("filter", e)?),
            Op::Unique { by, keep } => {
                let keep_last = match keep.as_deref() {
                    None | Some("first") => false,
                    Some("last") => true,
                    Some(other) => {
                        return Err(self.err(format!("unique: keep must be \"first\" or \"last\", got {other:?}")))
                    }
                };
                let by = match by {
                    Some(e) => Some(self.nested("unique.by", e)?),
                    None => None,
                };
                COp::Unique { by, keep_last }
            }
            Op::Default(e) => COp::Default(self.nested("default", e)?),
            Op::Required => COp::Required,
            Op::Coalesce(es) => {
                let mut v = Vec::with_capacity(es.len());
                for (i, e) in es.iter().enumerate() {
                    v.push(self.nested(&format!("coalesce[{i}]"), e)?);
                }
                COp::Coalesce(v)
            }
            Op::Strip => COp::Strip,
            Op::Lower => COp::Lower,
            Op::Upper => COp::Upper,
            Op::Replace { from, to } => COp::Replace(from.clone(), to.clone()),
            Op::Split(s) => COp::Split(s.clone()),
            Op::Join(s) => COp::Join(s.clone()),
            Op::Regex { pattern, group } => COp::Regex(self.regex(pattern)?, *group),
            Op::RegexAll { pattern, group } => COp::RegexAll(self.regex(pattern)?, *group),
            Op::Int => COp::Int,
            Op::Float => COp::Float,
            Op::Bool => COp::Bool,
            Op::ParseSrcset { pick, min_width, min_density } => {
                if let Some(p) = pick.as_deref() {
                    if p != "max" && p != "min" {
                        return Err(self.err(format!("parse_srcset: pick must be \"max\" or \"min\", got {p:?}")));
                    }
                }
                COp::ParseSrcset { pick: pick.clone(), min_width: *min_width, min_density: *min_density }
            }
            Op::Price { decimal, currency } => {
                if let Some(d) = decimal.as_deref() {
                    if d != "." && d != "," {
                        return Err(self.err(format!("price: decimal must be \".\" or \",\", got {d:?}")));
                    }
                }
                COp::Price { decimal: decimal.clone(), currency: currency.clone() }
            }
            Op::Eq(e) => COp::Eq(self.nested("eq", e)?),
            Op::Ne(e) => COp::Ne(self.nested("ne", e)?),
            Op::Contains(e) => COp::Contains(self.nested("contains", e)?),
            Op::StartsWith(e) => COp::StartsWith(self.nested("starts_with", e)?),
            Op::EndsWith(e) => COp::EndsWith(self.nested("ends_with", e)?),
            Op::Not => COp::Not,
            Op::And(e) => COp::And(self.nested("and", e)?),
            Op::Or(e) => COp::Or(self.nested("or", e)?),
            Op::If { cond, then, otherwise } => {
                COp::If(self.nested("if.cond", cond)?, self.nested("if.then", then)?, self.nested("if.otherwise", otherwise)?)
            }
            Op::Follow { request, meta, schema, format } => {
                if let Some(fmt) = format.as_deref() {
                    if fmt != "html" && fmt != "json" && fmt != "xml" {
                        return Err(self.err(format!("follow: format must be \"html\", \"json\", or \"xml\", got {fmt:?}")));
                    }
                }
                let request = self.nested("follow.request", request)?;
                let meta = match meta {
                    Some(m) => self.nested("follow.meta", m)?,
                    None => CExpr::Lit(Value::Null),
                };
                let switches = match (self.backend_name, format.as_deref()) {
                    (name, Some("html")) if name == "json" || name == "xml" => Some("auto"),
                    (name, Some("json")) if name != "json" => Some("json"),
                    (name, Some("xml")) if name != "xml" => Some("xml"),
                    _ => None,
                };
                if let Some(target) = switches {
                    let eng = crate::engine::compile(schema, target)
                        .map_err(|e| self.err(format!("follow(format={target}): {e}")))?;
                    self.foreign_specs.push(ForeignSpec {
                        engine: Arc::from(eng),
                        post: Vec::new(),
                    });
                    COp::FollowForeign {
                        request,
                        meta,
                        foreign: self.foreign_specs.len() - 1,
                        on_null: Vec::new(),
                    }
                } else {
                    let schema = self.nested("follow.schema", schema)?;
                    self.subs.push(schema);
                    COp::Follow { request, meta, sub: self.subs.len() - 1, on_null: Vec::new() }
                }
            }
            Op::Paginate { items, next, meta, max_pages, max_items, unique_by, keep } => {
                if *max_pages == Some(0) {
                    return Err(self.err("paginate: `max_pages` must be >= 1"));
                }
                if *max_items == Some(0) {
                    return Err(self.err("paginate: `max_items` must be >= 1"));
                }
                let keep_last = match keep.as_deref() {
                    None | Some("first") => false,
                    Some("last") => true,
                    Some(other) => {
                        return Err(self.err(format!("paginate: keep must be \"first\" or \"last\", got {other:?}")))
                    }
                };
                let items = self.nested("paginate.items", items)?;
                let next = self.nested("paginate.next", next)?;
                let meta = match meta {
                    Some(m) => self.nested("paginate.meta", m)?,
                    None => CExpr::Lit(Value::Null),
                };
                let unique_by = match unique_by {
                    Some(u) => Some(self.nested("paginate.unique_by", u)?),
                    None => None,
                };
                self.pagers.push(PaginateSpec {
                    items,
                    next,
                    meta,
                    max_pages: *max_pages,
                    max_items: *max_items,
                    unique_by,
                    keep_last,
                    post: Vec::new(),
                });
                COp::Paginate { pager: self.pagers.len() - 1 }
            }
        })
    }

    /// `follow(...).strip().lower()` / `paginate(...).count()`: the operations behind `follow` or `paginate` apply to
    /// the final result once fetching finishes, so they are moved into the sub-schema or `PaginateSpec::post`.
    fn fuse_follow_tail(&mut self, mut ops: Vec<COp>) -> Vec<COp> {
        // right to left: in `follow(A).follow(B).strip()` the `strip` moves into B's sub-schema first, then
        // the (already fused) second follow moves into A's, where it is evaluated against the page A fetched
        let mut upto = ops.len();
        while let Some(k) = ops[..upto].iter().rposition(|o| {
            matches!(o, COp::Follow { .. } | COp::FollowForeign { .. } | COp::Paginate { .. })
        }) {
            upto = k;
            if k + 1 == ops.len() {
                continue;
            }
            let tail: Vec<COp> = ops.split_off(k + 1);
            match ops.last_mut() {
                Some(COp::Paginate { pager }) => {
                    let pager = *pager;
                    self.pagers[pager].post.extend(tail);
                    self.notes.push("paginate|ops -> ops moved into paginate post-ops".into());
                }
                Some(COp::FollowForeign { foreign, on_null, .. }) => {
                    let foreign = *foreign;
                    let mut nulls: Vec<COp> = Vec::new();
                    for o in &tail {
                        match o {
                            COp::Follow { on_null: inner, .. }
                            | COp::FollowForeign { on_null: inner, .. } => nulls.extend(inner.iter().cloned()),
                            other => nulls.push(other.clone()),
                        }
                    }
                    *on_null = nulls;
                    self.foreign_specs[foreign].post.extend(tail);
                    self.notes.push("follow(foreign)|ops -> ops moved into foreign post-ops".into());
                }
                Some(COp::Follow { sub, on_null, .. }) => {
                    let sub = *sub;
                    // when no request is made the chain stops: later follows are skipped, but what follows
                    // *them* (their own `on_null`) still applies to null
                    let mut nulls: Vec<COp> = Vec::new();
                    for o in &tail {
                        match o {
                            COp::Follow { on_null: inner, .. }
                            | COp::FollowForeign { on_null: inner, .. } => nulls.extend(inner.iter().cloned()),
                            other => nulls.push(other.clone()),
                        }
                    }
                    *on_null = nulls;
                    let pag_target = match &self.subs[sub] {
                        CExpr::Pipe(inner_ops) => match inner_ops.last() {
                            Some(COp::Paginate { pager }) => Some(*pager),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(pager) = pag_target {
                        self.pagers[pager].post.extend(tail);
                        self.notes.push("follow(paginate)|ops -> ops moved into paginate post-ops".into());
                    } else {
                        let inner = std::mem::replace(&mut self.subs[sub], CExpr::Lit(Value::Null));
                        let mut pipe = vec![COp::Run(inner)];
                        pipe.extend(tail);
                        self.subs[sub] = CExpr::Pipe(pipe);
                        self.notes.push("follow|ops -> ops moved into the sub-schema".into());
                    }
                }
                _ => {}
            }
        }
        ops
    }

    /// Local rewrites. `select|first` stops at the first match instead of collecting everything.
    fn peephole(&mut self, ops: Vec<COp>) -> Vec<COp> {
        let mut out: Vec<COp> = Vec::with_capacity(ops.len());
        for op in ops {
            // `group_by(...) | map(body)` is one operation
            if matches!(op, COp::Map(_) | COp::MapSafe(..)) && matches!(out.last(), Some(COp::GroupBy { .. })) {
                if let Some(COp::GroupBy { header, items }) = out.pop() {
                    let (body, mode) = match op {
                        COp::Map(b) => (b, crate::ir::OnError::Raise),
                        COp::MapSafe(b, m) => (b, m),
                        _ => unreachable!(),
                    };
                    out.push(COp::GroupMap { header, items, body, mode });
                }
                continue;
            }
            // `adaptive | map`: a relocation has to bring back the whole list of records, not a single element
            if matches!(op, COp::Map(_) | COp::MapSafe(..)) {
                if let Some(COp::Adaptive { multi, .. }) = out.last_mut() {
                    *multi = true;
                }
            }
            // `css(..) | text | parse_json/js_json`: both read the text of a node themselves, and on the node they can
            // be served from the per-run cache without copying the text (`Eval::json_source`)
            if matches!(op, COp::ParseJson | COp::JsJson(_))
                && out.len() >= 2
                && matches!(out[out.len() - 1], COp::Text)
                && matches!(out[out.len() - 2], COp::SelectFirst(_))
            {
                out.pop();
                self.notes.push("select_first|text|parse_json -> select_first|parse_json (the node is parsed directly)".into());
            }
            // `parse_json()["a"]["b"][0]` -> one `json_path`: no deep copy of the parsed document per step
            let seg = match &op {
                COp::Key(k) => Some(JsonSeg::Key(k.clone())),
                COp::At(i) => Some(JsonSeg::At(*i)),
                _ => None,
            };
            if let Some(seg) = seg {
                if matches!(out.last(), Some(COp::ParseJson)) {
                    out.pop();
                    out.push(COp::JsonPath(vec![seg]));
                    self.notes.push("parse_json|key/at -> json_path (navigate by reference)".into());
                    continue;
                }
                if let Some(COp::JsonPath(segs)) = out.last_mut() {
                    segs.push(seg);
                    continue;
                }
            }
            // `text`, `attr`, `has_attr`, `html`, ... only ever look at the FIRST node, so a selector in front of
            // them can stop at the first match instead of collecting (and discarding) every matching node.
            // NOTE: `SelectFirst | First` is NOT redundant: `first` on a node means "first child".
            let action = match (out.last(), &op) {
                (Some(COp::Select(i)), COp::First | COp::At(0)) => Some((0u8, *i)),
                (Some(COp::Select(i) | COp::SelectFirst(i)), COp::Exists) => Some((1u8, *i)),
                (Some(COp::Strip), COp::Strip) => Some((2u8, 0)),
                (
                    Some(COp::Select(i)),
                    COp::Text
                    | COp::TextOpts { .. }
                    | COp::OwnTexts
                    | COp::Table { .. }
                    | COp::MainContent
                    | COp::Markdown { .. }
                    | COp::Distill { .. }
                    | COp::Attr(_)
                    | COp::HasAttr(_)
                    | COp::Html
                    | COp::InnerHtml,
                ) => Some((3u8, *i)),
                _ => None,
            };
            match action {
                Some((0, i)) => {
                    out.pop();
                    out.push(COp::SelectFirst(i));
                    self.notes.push("select|first -> select_first (early exit)".into());
                }
                Some((1, i)) => {
                    out.pop();
                    out.push(COp::SelectExists(i));
                    self.notes.push("select|exists -> select_exists (early exit, no allocation)".into());
                }
                Some((2, _)) => self.notes.push("strip|strip -> strip".into()),
                Some((_, i)) => {
                    out.pop();
                    out.push(COp::SelectFirst(i));
                    out.push(op);
                    self.notes.push("select|text/attr/html -> select_first|... (early exit)".into());
                }
                None => out.push(op),
            }
        }
        out
    }
}
