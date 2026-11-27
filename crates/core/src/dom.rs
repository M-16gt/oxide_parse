//! The engine abstraction. An engine = "how to navigate one parsed document".
//! The executor and the XPath evaluator are generic over this trait, so they are
//! written once and run on every engine (monomorphised, no `dyn` in the hot path).
use crate::value::Value;
use std::borrow::Cow;
use std::collections::HashSet;

/// One step of a document walk (see [`Dom::walk`]): what the Markdown renderer consumes.
pub enum Walk<'a> {
    /// An element opens; `attr` reads its (decoded) attributes.
    Open { name: &'a str, attr: &'a dyn Fn(&str) -> Option<String> },
    /// Decoded text.
    Text(&'a str),
    Close { name: &'a str },
}

/// Navigation primitives of a parsed document.
///
/// Only the first group is mandatory. The second group enables XPath (`XPATH = true`)
/// and, later, adaptive element matching (fingerprints need `name`, `attrs`, `parent`, `children`).
///
/// Conventions: `Node` is an *element* handle; the document node is never a `Node`.
/// `parent` returns `None` for top-level elements.
pub trait Dom {
    type Node: Clone;
    /// Compiled CSS selector. Must not depend on the document lifetime.
    type Sel: Send + Sync;
    const NAME: &'static str;
    /// Set to `true` once the "XPath group" below is implemented.
    const XPATH: bool = false;

    /// Compile a CSS selector. `Err` surfaces as a compile-time error.
    fn compile_css(src: &str) -> std::result::Result<Self::Sel, String>;

    fn root(&self) -> Self::Node;
    /// Does `sel` match this very element (not its descendants)? The default asks the parent; engines answer directly.
    fn matches_sel(&self, node: &Self::Node, sel: &Self::Sel) -> bool {
        match self.parent(node) {
            Some(p) => {
                let mut found: Vec<Self::Node> = Vec::new();
                self.select(&p, sel, false, &mut found);
                let key = self.node_key(node);
                found.iter().any(|f| self.node_key(f) == key)
            }
            None => false,
        }
    }
    /// The `href` of the document's first `<base href="...">` (trimmed; `None` if there is none or it is empty).
    /// `T.url()` resolves against it when `meta["base_url"]` is not given.
    fn base_href(&self) -> Option<String> {
        let sel = Self::compile_css("base[href]").ok()?;
        let mut out: Vec<Self::Node> = Vec::new();
        self.select(&self.root(), &sel, true, &mut out);
        let href = self.attr(out.first()?, "href")?;
        let href = href.trim();
        if href.is_empty() {
            None
        } else {
            Some(href.to_string())
        }
    }
    /// Descendants of `node` matching `sel` in document order; with `first_only` stop after the first match.
    fn select(&self, node: &Self::Node, sel: &Self::Sel, first_only: bool, out: &mut Vec<Self::Node>);
    /// `true` if CSS selectors of this engine understand `:scope` (the element a `select` started from). Other
    /// engines refuse such selectors at compile time instead of matching something different.
    const SCOPE: bool = false;
    /// `true` if [`Dom::select_multi`] is cheaper than calling `select` once per selector (one walk of the
    /// subtree instead of one per selector). The executor only fuses the fields of a record then.
    const MULTI_SELECT: bool = false;
    /// Several selectors over the same subtree. `out[k]` receives the matches of `sels[k]` in document order,
    /// with `first_only[k]` only the first one. Same result as one `select` per selector (the default).
    fn select_multi(&self, node: &Self::Node, sels: &[&Self::Sel], first_only: &[bool], out: &mut [Vec<Self::Node>]) {
        for k in 0..sels.len() {
            self.select(node, sels[k], first_only[k], &mut out[k]);
        }
    }
    /// Concatenated text of all descendant text nodes. Content of `<script>`/`<style>` below `node` is
    /// left out (the text of a script/style element itself is its content).
    fn text(&self, node: &Self::Node) -> String;
    /// The text nodes below `node` as separate strings (same script/style rule as `text`), leaving out
    /// the subtrees whose `node_key` is in `skip`.
    fn text_parts(&self, node: &Self::Node, skip: &HashSet<usize>, out: &mut Vec<String>) {
        let _ = skip;
        out.push(self.text(node));
    }
    /// Like `text_parts`, but `<br>` shows up as `None` between the pieces. Built on [`Dom::walk`]; `false` if the
    /// engine has no `walk`.
    fn text_pieces(&self, node: &Self::Node, skip: &HashSet<usize>, out: &mut Vec<Option<String>>) -> bool {
        let is_raw = |n: &str| n.eq_ignore_ascii_case("script") || n.eq_ignore_ascii_case("style");
        let mut seen_root = false;
        let mut root_raw = false;
        let mut raw = 0usize;
        self.walk(node, skip, &mut |ev| match ev {
            Walk::Open { name, .. } => {
                if !seen_root {
                    seen_root = true;
                    root_raw = is_raw(name);
                } else if is_raw(name) {
                    raw += 1;
                } else if raw == 0 && name.eq_ignore_ascii_case("br") {
                    out.push(None);
                }
            }
            Walk::Close { name } => {
                if is_raw(name) && raw > 0 {
                    raw -= 1;
                }
            }
            Walk::Text(t) => {
                if raw == 0 || root_raw {
                    out.push(Some(t.to_owned()));
                }
            }
        })
    }
    /// The element as a distilled skeleton (see `Op::Distill`); `None` = this engine cannot do it directly, the executor
    /// then distils the element's HTML.
    fn distill(&self, node: &Self::Node, keep: usize, max_text: usize, max_chars: usize) -> Option<String> {
        let _ = (node, keep, max_text, max_chars);
        None
    }
    /// Walk `node` (itself included) and everything below it in document order as open/text/close events,
    /// leaving out the subtrees whose `node_key` is in `skip`. `false` = this engine cannot do it.
    fn walk(&self, node: &Self::Node, skip: &HashSet<usize>, f: &mut dyn FnMut(Walk<'_>)) -> bool {
        let _ = (node, skip, f);
        false
    }
    fn html(&self, node: &Self::Node, inner: bool) -> String;
    fn attr(&self, node: &Self::Node, name: &str) -> Option<String>;
    /// `true` if `node` represents `null` / missing (`false` on HTML elements; `true` for JSON `null`).
    fn node_is_null(&self, _node: &Self::Node) -> bool {
        false
    }
    /// Truthiness of `node` in conditionals (`if_`, `filter`, `not`, `and`, `or`, `exists`).
    fn node_truthy(&self, _node: &Self::Node) -> bool {
        true
    }
    /// `true` if `node` is considered empty by `non_empty`.
    fn node_is_empty(&self, _node: &Self::Node) -> bool {
        false
    }
    /// How a node renders when a pipeline ends on it.
    fn to_value(&self, node: &Self::Node) -> Value;
    /// Stable identity of a node within one document (used for de-duplication).
    fn node_key(&self, node: &Self::Node) -> usize;

    fn parent(&self, _node: &Self::Node) -> Option<Self::Node> {
        None
    }
    /// Previous / next *element* sibling. The defaults scan the parent's children (O(siblings)); engines with
    /// sibling links override them with O(1) versions (adaptive matching asks for them once per candidate).
    fn prev_sibling(&self, node: &Self::Node) -> Option<Self::Node> {
        let mut sibs: Vec<Self::Node> = Vec::new();
        match self.parent(node) {
            Some(p) => self.children(&p, &mut sibs),
            None => self.document_children(&mut sibs),
        }
        let me = self.node_key(node);
        let i = sibs.iter().position(|s| self.node_key(s) == me)?;
        if i > 0 {
            Some(sibs[i - 1].clone())
        } else {
            None
        }
    }
    fn next_sibling(&self, node: &Self::Node) -> Option<Self::Node> {
        let mut sibs: Vec<Self::Node> = Vec::new();
        match self.parent(node) {
            Some(p) => self.children(&p, &mut sibs),
            None => self.document_children(&mut sibs),
        }
        let me = self.node_key(node);
        let i = sibs.iter().position(|s| self.node_key(s) == me)?;
        sibs.get(i + 1).cloned()
    }
    /// Element children in document order.
    fn children(&self, _node: &Self::Node, _out: &mut Vec<Self::Node>) {}
    fn key(&self, _node: &Self::Node, _key: &str) -> std::result::Result<Option<Self::Node>, String> {
        Err(format!("key access is not supported by the `{}` engine", Self::NAME))
    }
    fn at(&self, _node: &Self::Node, _i: usize) -> std::result::Result<Option<Self::Node>, String> {
        Err(format!("positional access is not supported by the `{}` engine", Self::NAME))
    }

    // ---- XPath / adaptive group ------------------------------------------------------------

    /// Tag name (as written; callers compare case-insensitively).
    fn name<'s>(&'s self, _node: &Self::Node) -> Option<Cow<'s, str>> {
        None
    }
    /// All attributes of an element (decoded values).
    fn attrs(&self, _node: &Self::Node, _out: &mut Vec<(String, String)>) {}
    /// Text of the element's *direct* text-node children (for `text()` steps).
    fn direct_texts(&self, _node: &Self::Node) -> Vec<String> {
        Vec::new()
    }
    /// Top-level elements (children of the virtual document node). Default: just the root.
    fn document_children(&self, out: &mut Vec<Self::Node>) {
        out.push(self.root());
    }
}
