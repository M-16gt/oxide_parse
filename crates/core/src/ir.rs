//! The serialisable intermediate representation (IR). One definition gives you
//! JSON / YAML loading and saving; the Python side builds exactly this structure.
use crate::error::{Error, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// What `map` does when the body fails on one record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnError {
    /// the whole extraction fails (default, same as plain `map`)
    #[default]
    Raise,
    /// the failed record becomes `null`
    Null,
    /// the failed record is left out of the list
    Skip,
}

/// An expression: a pipeline of operations, an object, a list or a literal.
///
/// # Examples
///
/// ```rust
/// use oxide_parse_core::Expr;
///
/// let ir = Expr::from_json(r#"{"pipe":[{"css":"h1"},"text"]}"#).unwrap();
/// assert!(ir.to_json().contains("h1"));
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Expr {
    Pipe(Vec<Op>),
    Object(IndexMap<String, Expr>),
    List(Vec<Expr>),
    Lit(serde_json::Value),
}

/// One pipeline step. Operations that take an `Expr` evaluate it either per item
/// (`map`, `flat_map`, `filter`) or against the pipeline's *context* (`default`, `eq`, `and`, `if`, `follow`, ...).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    // navigation
    Css(String),
    Xpath(String),
    Parent,
    Children,
    Root,
    Key(String),
    At(usize),
    /// Python-style slice `[start:stop]` (negative counts from the end, out-of-range is clamped) of a node list, a list
    /// value, a string (characters) or the children of one node. `T.rest()` is `slice(1)`: the group minus its header.
    Slice {
        #[serde(default)]
        start: Option<i64>,
        #[serde(default)]
        stop: Option<i64>,
    },
    First,
    Last,
    // extraction
    Attr(String),
    /// `true` if the element has the attribute at all (also valueless ones: `<button disabled>`).
    HasAttr(String),
    Text,
    /// Text with options. `sep` joins the text nodes (like BeautifulSoup `get_text(separator)`),
    /// `strip` trims every piece and drops empty ones, `without` is a CSS selector of subtrees to leave out.
    TextOpts {
        #[serde(default)]
        sep: String,
        #[serde(default)]
        strip: bool,
        #[serde(default)]
        without: Option<String>,
        /// What a `<br>` becomes (e.g. `"\n"`): it replaces the separator at that place. `None`: `<br>` is ignored.
        #[serde(default)]
        br: Option<String>,
        /// Whether to descend into nested child elements (`Some(false)` reads only direct child text nodes).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deep: Option<bool>,
    },
    /// Direct child text nodes of the first matched element as a list of strings.
    OwnTexts,
    /// Expand an HTML `<table>` into `records`, `grid`, or `split` with full `rowspan`/`colspan` resolution.
    Table {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        strip: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        header_sep: Option<String>,
    },
    /// The element as Markdown (headings, lists, links, emphasis, code, quotes, tables); `without` is a CSS selector
    /// of subtrees to leave out. DOM engines only.
    Markdown {
        #[serde(default)]
        without: Option<String>,
    },
    /// The element as a skeleton for prompts / humans (same as `oxide_parse.distill`, for one element): scripts, styles
    /// and most attributes dropped, text clipped to `max_text`, runs of similar siblings folded to `keep`, output cut at
    /// `max_chars`. Defaults 2 / 80 / 20000. Needs the `lite` engine in the build.
    Distill {
        #[serde(default)]
        keep: Option<usize>,
        #[serde(default)]
        max_text: Option<usize>,
        #[serde(default)]
        max_chars: Option<usize>,
    },
    Html,
    InnerHtml,
    Meta(String),
    Index,
    /// `css` with recovery: if `css` matches nothing, relocate the element most similar to the one
    /// remembered under `key` (see [`crate::adaptive`]). Meant for single elements.
    Adaptive {
        css: String,
        key: String,
        #[serde(default)]
        threshold: Option<f64>,
    },
    // collections
    Count,
    Exists,
    Map(Box<Expr>),
    /// `map` with an error policy: with `skip` / `null` a failing record is quarantined (reported by
    /// `extract_detailed`) instead of failing the page.
    MapOpts {
        body: Box<Expr>,
        #[serde(default)]
        on_error: OnError,
    },
    /// Resolve a relative URL (RFC 3986) against `base` or, without it, against `meta["base_url"]`.
    ResolveUrl {
        #[serde(default)]
        base: Option<Box<Expr>>,
    },
    /// Take the JSON out of JavaScript source: `var` is the assignment target (`window.__STATE__`),
    /// without it the first `{...}` / `[...]`. Also understands `X = JSON.parse("...")`.
    JsJson {
        #[serde(default)]
        var: Option<String>,
    },
    /// Flat markup: split the children of an element into sections, each starting at a `header` element and
    /// running up to the next one: `[{"header": text, "items": [text, ...]}, ...]`.
    Sections {
        header: String,
        #[serde(default)]
        items: Option<String>,
    },
    /// Flat markup, general form: group the children of an element at every `header` element (the header comes first in
    /// its group, then the siblings up to the next header, narrowed by `items`). Must be followed by `map`, whose body
    /// sees the group as its context: `T.first()` is the header, `T.css("a")` searches the whole group.
    GroupBy {
        header: String,
        #[serde(default)]
        items: Option<String>,
    },
    /// The JSON-LD objects of the page (`<script type="application/ld+json">`): arrays and `@graph` are unfolded, invalid
    /// blocks skipped, `schema_type` keeps the objects of that `@type`. `resolve_refs` expands `{"@id": ...}` references.
    JsonLd {
        #[serde(default)]
        schema_type: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolve_refs: Option<bool>,
    },
    /// HTML Microdata (`[itemscope]`, `itemprop`, `itemtype`, `itemref`, `itemid`) as a list of dicts (DOM engines only).
    Microdata {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schema_type: Option<String>,
    },
    /// Decode Next.js App Router `self.__next_f.push(...)` RSC Flight payloads into JSON values.
    NextFlight {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<String>,
    },
    /// Heuristic Readability-style main content element selector (DOM engines only).
    MainContent,
    /// OpenGraph / Twitter / any `<meta property|name content>` as one map; `prefixes` limits the names (`[]` = all).
    /// A name that repeats becomes a list.
    MetaTags {
        #[serde(default)]
        prefixes: Vec<String>,
        /// `"auto"` (default): the content, or a list if the name repeats; `"first"`: always the first content (a
        /// string); `"list"`: always a list.
        #[serde(default)]
        values: Option<String>,
    },
    FlatMap(Box<Expr>),
    Filter(Box<Expr>),
    /// Order-preserving list deduplication (`by`: key expr evaluated per item; `keep`: `"first"` or `"last"`).
    Unique {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        by: Option<Box<Expr>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keep: Option<String>,
    },
    // null handling
    Default(Box<Expr>),
    Required,
    /// empty / whitespace-only string, empty list or map => `null` (so `default` can kick in)
    NonEmpty,
    Coalesce(Vec<Expr>),
    // strings
    Strip,
    Lower,
    Upper,
    Replace { from: String, to: String },
    Split(String),
    Join(String),
    Regex { pattern: String, #[serde(default)] group: usize },
    RegexAll { pattern: String, #[serde(default)] group: usize },
    // conversion
    Int,
    Float,
    Bool,
    /// Parse an HTML `srcset` string (WHATWG rules) into candidate dicts or pick one by `pick`/`min_width`/`min_density`.
    ParseSrcset {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pick: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_width: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_density: Option<f64>,
    },
    /// Parse a locale-aware price string into `{"amount": float, "currency": str | null}` (`null` when no number).
    Price {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        decimal: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        currency: Option<String>,
    },
    /// Parse a string as JSON; `key`/`at`/`map` then navigate the result (`__NEXT_DATA__`, JSON-LD).
    ParseJson,
    /// Parse an HTML string fragment (e.g., inside JSON or `iframe[srcdoc]`) with a sub-schema.
    ParseHtml(Box<Expr>),
    /// Parse an HTTP `Link` header string (RFC 8288): return URL for `rel` or `{rel: url}` map when `rel` is `None`.
    LinkHeader {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rel: Option<String>,
    },
    // logic
    Eq(Box<Expr>),
    Ne(Box<Expr>),
    Contains(Box<Expr>),
    StartsWith(Box<Expr>),
    EndsWith(Box<Expr>),
    Not,
    And(Box<Expr>),
    Or(Box<Expr>),
    If { cond: Box<Expr>, then: Box<Expr>, otherwise: Box<Expr> },
    // sub-queries (no IO here: the engine hands requests back to the caller)
    Follow {
        /// What the caller needs to fetch the page: a dict (`{"url": ..., "headers": ..., ...}`) that the
        /// driver passes on as `fetch(**request)`, or a plain string (shorthand for `{"url": s}`).
        /// Evaluates to `null`, or to a dict whose `url` is `null`, => no request, the field is `null`.
        request: Box<Expr>,
        #[serde(default)]
        meta: Option<Box<Expr>>,
        schema: Box<Expr>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
    },
    /// Collect `items` across a chain of pages linked by `next` requests (`fetch(**next)`), stopping on `null` / empty /
    /// cycle, or once `max_pages` (initial page included) or `max_items` is reached.
    Paginate {
        items: Box<Expr>,
        next: Box<Expr>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        meta: Option<Box<Expr>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_pages: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_items: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unique_by: Option<Box<Expr>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keep: Option<String>,
    },
}

impl Expr {
    pub fn from_json(s: &str) -> Result<Expr> {
        serde_json::from_str(s).map_err(|e| Error::Ir(e.to_string()))
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("IR is always serialisable")
    }
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("IR is always serialisable")
    }
    #[cfg(feature = "yaml")]
    pub fn from_yaml(s: &str) -> Result<Expr> {
        serde_yaml::from_str(s).map_err(|e| Error::Ir(e.to_string()))
    }
    #[cfg(feature = "yaml")]
    pub fn to_yaml(&self) -> String {
        serde_yaml::to_string(self).expect("IR is always serialisable")
    }
}
