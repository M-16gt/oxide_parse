//! `lite` engine: our own zero-copy tokenizer + arena DOM. No html5ever, no allocation per node
//! (names, attributes and text are slices of the input). CSS and XPath are supported.
//!
//! Trade-off: tree construction follows the common HTML rules (void elements, implied end tags for
//! `p/li/dt/dd/tr/td/th/option/...`) but is not the full HTML5 algorithm (no foster parenting, no adoption agency).
use crate::dom::{Dom, Walk};
use crate::engine::Engine;
use crate::error::Result;
use crate::exec::Session;
use crate::ir::Expr;
use crate::markup::arena::{Doc, Kind, NIL};
use crate::markup::css::{ElemView, Selector};
use crate::markup::events::is_void;
use crate::markup::{entities, tokenizer};
use crate::plan::Plan;
use crate::value::Value;
use std::borrow::Cow;
use std::collections::HashSet;

/// A parsed document borrowing the source text.
pub struct LiteDoc<'a> {
    doc: Doc<'a>,
}

impl<'a> LiteDoc<'a> {
    pub fn parse(src: &'a str) -> Self {
        LiteDoc { doc: Doc::parse(src) }
    }
    pub fn parse_xml(src: &'a str) -> Self {
        LiteDoc { doc: Doc::parse_xml(src) }
    }
    pub fn dom(&self) -> LiteDom<'_, 'a> {
        LiteDom { doc: &self.doc }
    }
}

pub struct LiteDom<'d, 'a> {
    doc: &'d Doc<'a>,
}

/// An element seen through the CSS matcher.
#[derive(Clone, Copy)]
pub struct Elem<'d, 'a> {
    doc: &'d Doc<'a>,
    id: u32,
    /// the node a `select` started from (`:scope`); the document node `0` for "no particular element"
    scope: u32,
}

impl<'d, 'a> ElemView for Elem<'d, 'a> {
    fn is_scope(&self) -> bool {
        if self.id == self.scope {
            return true;
        }
        // a query over the whole document: `:scope` is the root element, as in browsers
        matches!(self.doc.node(self.scope).kind, Kind::Document) && self.parent().is_none()
    }

    fn name(&self) -> &str {
        match self.doc.node(self.id).kind {
            Kind::Element { name, .. } => name,
            _ => "",
        }
    }

    fn attr(&self, key: &str) -> Option<Cow<'_, str>> {
        match self.doc.node(self.id).kind {
            Kind::Element { attrs, .. } => tokenizer::find_attr(attrs, key).map(entities::decode_attr),
            _ => None,
        }
    }

    fn parent(&self) -> Option<Self> {
        let p = self.doc.node(self.id).parent;
        if p == NIL || p == 0 {
            None
        } else {
            Some(Elem { doc: self.doc, id: p, scope: self.scope })
        }
    }

    fn prev_sibling(&self) -> Option<Self> {
        let mut p = self.doc.node(self.id).prev;
        while p != NIL {
            if self.doc.is_element(p) {
                return Some(Elem { doc: self.doc, id: p, scope: self.scope });
            }
            p = self.doc.node(p).prev;
        }
        None
    }

    fn child_index(&self) -> usize {
        let mut n = 1;
        let mut p = self.doc.node(self.id).prev;
        while p != NIL {
            if self.doc.is_element(p) {
                n += 1;
            }
            p = self.doc.node(p).prev;
        }
        n
    }

    fn child_index_from_end(&self) -> usize {
        let mut n = 1;
        let mut p = self.doc.node(self.id).next;
        while p != NIL {
            if self.doc.is_element(p) {
                n += 1;
            }
            p = self.doc.node(p).next;
        }
        n
    }

    fn type_index(&self) -> usize {
        let me = self.name();
        let mut n = 1;
        let mut p = self.doc.node(self.id).prev;
        while p != NIL {
            if let Kind::Element { name, .. } = self.doc.node(p).kind {
                if name.eq_ignore_ascii_case(me) {
                    n += 1;
                }
            }
            p = self.doc.node(p).prev;
        }
        n
    }

    fn type_index_from_end(&self) -> usize {
        let me = self.name();
        let mut n = 1;
        let mut p = self.doc.node(self.id).next;
        while p != NIL {
            if let Kind::Element { name, .. } = self.doc.node(p).kind {
                if name.eq_ignore_ascii_case(me) {
                    n += 1;
                }
            }
            p = self.doc.node(p).next;
        }
        n
    }

    fn is_first_legend_of_parent(&self) -> bool {
        if !self.name().eq_ignore_ascii_case("legend") {
            return false;
        }
        let mut p = self.doc.node(self.id).prev;
        while p != NIL {
            if let Kind::Element { name, .. } = self.doc.node(p).kind {
                if name.eq_ignore_ascii_case("legend") {
                    return false;
                }
            }
            p = self.doc.node(p).prev;
        }
        true
    }

    fn is_last_child(&self) -> bool {
        let mut p = self.doc.node(self.id).next;
        while p != NIL {
            if self.doc.is_element(p) {
                return false;
            }
            p = self.doc.node(p).next;
        }
        true
    }

    fn has_no_children(&self) -> bool {
        self.doc.node(self.id).first_child == NIL
    }

    fn children(&self, out: &mut Vec<Self>) {
        let mut c = self.doc.node(self.id).first_child;
        while c != NIL {
            if self.doc.is_element(c) {
                out.push(Elem { doc: self.doc, id: c, scope: self.scope });
            }
            c = self.doc.node(c).next;
        }
    }

    fn next_sibling(&self) -> Option<Self> {
        let mut p = self.doc.node(self.id).next;
        while p != NIL {
            if self.doc.is_element(p) {
                return Some(Elem { doc: self.doc, id: p, scope: self.scope });
            }
            p = self.doc.node(p).next;
        }
        None
    }

    fn contains_text(&self, needle: &str) -> bool {
        let mut buf = String::new();
        LiteDom { doc: self.doc }.walk_text(self.id, &HashSet::new(), |s| buf.push_str(s));
        buf.contains(needle)
    }
}

impl<'d, 'a> LiteDom<'d, 'a> {
    fn push_children(&self, id: u32, stack: &mut Vec<(u32, bool)>) {
        let mut ch: Vec<u32> = Vec::new();
        let mut c = self.doc.node(id).first_child;
        while c != NIL {
            ch.push(c);
            c = self.doc.node(c).next;
        }
        for c in ch.into_iter().rev() {
            stack.push((c, false));
        }
    }

    fn serialize(&self, id: u32, inner: bool, out: &mut String) {
        let mut stack: Vec<(u32, bool)> = Vec::new();
        if inner {
            self.push_children(id, &mut stack);
        } else {
            stack.push((id, false));
        }
        while let Some((n, closing)) = stack.pop() {
            let kind = self.doc.node(n).kind;
            if closing {
                if let Kind::Element { name, .. } = kind {
                    out.push_str("</");
                    out.push_str(name);
                    out.push('>');
                }
                continue;
            }
            match kind {
                Kind::Text { text, .. } => out.push_str(text),
                Kind::Element { name, attrs } => {
                    out.push('<');
                    out.push_str(name);
                    let a = attrs.trim();
                    if !a.is_empty() {
                        out.push(' ');
                        out.push_str(a);
                    }
                    out.push('>');
                    if is_void(name) {
                        continue;
                    }
                    stack.push((n, true));
                    self.push_children(n, &mut stack);
                }
                Kind::Document => {}
            }
        }
    }

    /// Text nodes below `root` in document order, skipping `<script>` and `<style>` descendant subtrees and emitting
    /// `decode: false` nodes (such as XML CDATA and direct rawtext children) verbatim.
    fn walk_text<F: FnMut(&str)>(&self, root: u32, skip: &HashSet<usize>, mut f: F) {
        let mut cur = self.doc.next_preorder(root, root);
        while cur != NIL {
            match self.doc.node(cur).kind {
                Kind::Text { text, decode } => {
                    if decode {
                        f(&entities::decode(text));
                    } else {
                        f(text);
                    }
                }
                Kind::Element { name, .. } => {
                    if name.eq_ignore_ascii_case("script")
                        || name.eq_ignore_ascii_case("style")
                        || (!skip.is_empty() && skip.contains(&(cur as usize)))
                    {
                        cur = self.doc.next_after(cur, root);
                        continue;
                    }
                }
                Kind::Document => {}
            }
            cur = self.doc.next_preorder(cur, root);
        }
    }

    fn elem_children(&self, id: u32, out: &mut Vec<u32>) {
        let mut c = self.doc.node(id).first_child;
        while c != NIL {
            if self.doc.is_element(c) {
                out.push(c);
            }
            c = self.doc.node(c).next;
        }
    }
}

impl<'d, 'a> Dom for LiteDom<'d, 'a> {
    type Node = u32;
    type Sel = Selector;
    const NAME: &'static str = "lite";
    const XPATH: bool = true;

    fn compile_css(src: &str) -> std::result::Result<Selector, String> {
        Selector::parse(src)
    }

    fn root(&self) -> u32 {
        0
    }

    fn select(&self, node: &u32, sel: &Selector, first_only: bool, out: &mut Vec<u32>) {
        let root = *node;
        let mut cur = self.doc.next_preorder(root, root);
        while cur != NIL {
            if self.doc.is_element(cur) && sel.matches(&Elem { doc: self.doc, id: cur, scope: root }) {
                out.push(cur);
                if first_only {
                    return;
                }
            }
            cur = self.doc.next_preorder(cur, root);
        }
    }

    const MULTI_SELECT: bool = true;

    fn distill(&self, node: &u32, keep: usize, max_text: usize, max_chars: usize) -> Option<String> {
        let opts = crate::markup::distill::DistillOptions { keep, max_text, max_chars };
        Some(crate::markup::distill::distill_node(self.doc, *node, &opts))
    }

    fn matches_sel(&self, node: &u32, sel: &Selector) -> bool {
        sel.matches(&Elem { doc: self.doc, id: *node, scope: *node })
    }
    const SCOPE: bool = true;

    /// One walk of the subtree for all selectors, no allocation. `id` / `class` of an element are read once and feed
    /// the cheap `may_match` pre-filter of every selector; the walk ends as soon as every selector that only wants
    /// its first match has found it.
    fn select_multi(&self, node: &u32, sels: &[&Selector], first_only: &[bool], out: &mut [Vec<u32>]) {
        let n = sels.len();
        if n > 64 {
            for k in 0..n {
                self.select(node, sels[k], first_only[k], &mut out[k]);
            }
            return;
        }
        // bit k set = selector k still collects
        let mut active: u64 = if n == 64 { u64::MAX } else { (1u64 << n) - 1 };
        let root = *node;
        let mut cur = self.doc.next_preorder(root, root);
        while cur != NIL && active != 0 {
            if let Kind::Element { name, attrs } = self.doc.node(cur).kind {
                let mut id: Option<&str> = None;
                let mut class: Option<&str> = None;
                if !attrs.is_empty() {
                    for (k, v) in tokenizer::attrs(attrs) {
                        // HTML5: the first occurrence of a duplicated attribute wins (as in `find_attr`)
                        if class.is_none() && k.eq_ignore_ascii_case("class") {
                            class = Some(v.unwrap_or(""));
                        } else if id.is_none() && k.eq_ignore_ascii_case("id") {
                            id = Some(v.unwrap_or(""));
                        }
                        if class.is_some() && id.is_some() {
                            break;
                        }
                    }
                }
                let view = Elem { doc: self.doc, id: cur, scope: root };
                let mut bits = active;
                while bits != 0 {
                    let k = bits.trailing_zeros() as usize;
                    bits &= bits - 1;
                    let sel = sels[k];
                    if sel.may_match(name, id, class) && sel.matches(&view) {
                        out[k].push(cur);
                        if first_only[k] {
                            active &= !(1u64 << k);
                        }
                    }
                }
            }
            cur = self.doc.next_preorder(cur, root);
        }
    }

    fn text(&self, node: &u32) -> String {
        let mut out = String::new();
        self.walk_text(*node, &HashSet::new(), |s| out.push_str(s));
        out
    }

    fn text_parts(&self, node: &u32, skip: &HashSet<usize>, out: &mut Vec<String>) {
        self.walk_text(*node, skip, |s| out.push(s.to_owned()));
    }

    fn walk(&self, node: &u32, skip: &HashSet<usize>, f: &mut dyn FnMut(Walk<'_>)) -> bool {
        let root = *node;
        let mut cur = root;
        loop {
            let mut descended = false;
            match self.doc.node(cur).kind {
                Kind::Element { name, .. } => {
                    if cur == root || skip.is_empty() || !skip.contains(&(cur as usize)) {
                        let view = Elem { doc: self.doc, id: cur, scope: root };
                        let attr = |k: &str| view.attr(k).map(Cow::into_owned);
                        f(Walk::Open { name, attr: &attr });
                        let fc = self.doc.node(cur).first_child;
                        if fc != NIL {
                            cur = fc;
                            descended = true;
                        } else {
                            f(Walk::Close { name });
                        }
                    }
                }
                Kind::Text { text, decode } => {
                    if decode {
                        f(Walk::Text(&entities::decode(text)));
                    } else {
                        f(Walk::Text(text));
                    }
                }
                Kind::Document => {
                    let fc = self.doc.node(cur).first_child;
                    if fc != NIL {
                        cur = fc;
                        descended = true;
                    }
                }
            }
            if descended {
                continue;
            }
            loop {
                if cur == root {
                    return true;
                }
                let nx = self.doc.node(cur).next;
                if nx != NIL {
                    cur = nx;
                    break;
                }
                cur = self.doc.node(cur).parent;
                if let Kind::Element { name, .. } = self.doc.node(cur).kind {
                    f(Walk::Close { name });
                }
            }
        }
    }

    fn html(&self, node: &u32, inner: bool) -> String {
        let mut out = String::new();
        self.serialize(*node, inner, &mut out);
        out
    }

    fn attr(&self, node: &u32, name: &str) -> Option<String> {
        Elem { doc: self.doc, id: *node, scope: 0 }.attr(name).map(Cow::into_owned)
    }

    fn to_value(&self, node: &u32) -> Value {
        Value::Str(self.text(node))
    }

    fn node_key(&self, node: &u32) -> usize {
        *node as usize
    }

    fn parent(&self, node: &u32) -> Option<u32> {
        Elem { doc: self.doc, id: *node, scope: 0 }.parent().map(|e| e.id)
    }

    fn children(&self, node: &u32, out: &mut Vec<u32>) {
        self.elem_children(*node, out);
    }

    fn prev_sibling(&self, node: &u32) -> Option<u32> {
        Elem { doc: self.doc, id: *node, scope: 0 }.prev_sibling().map(|e| e.id)
    }

    fn next_sibling(&self, node: &u32) -> Option<u32> {
        let mut p = self.doc.node(*node).next;
        while p != NIL {
            if self.doc.is_element(p) {
                return Some(p);
            }
            p = self.doc.node(p).next;
        }
        None
    }

    fn name<'s>(&'s self, node: &u32) -> Option<Cow<'s, str>> {
        match self.doc.node(*node).kind {
            Kind::Element { name, .. } => Some(Cow::Borrowed(name)),
            _ => None,
        }
    }

    fn attrs(&self, node: &u32, out: &mut Vec<(String, String)>) {
        if let Kind::Element { attrs, .. } = self.doc.node(*node).kind {
            for (k, v) in tokenizer::attrs(attrs) {
                out.push((k.to_ascii_lowercase(), entities::decode_attr(v.unwrap_or("")).into_owned()));
            }
        }
    }

    fn direct_texts(&self, node: &u32) -> Vec<String> {
        let mut out = Vec::new();
        let mut c = self.doc.node(*node).first_child;
        while c != NIL {
            if let Kind::Text { text, decode } = self.doc.node(c).kind {
                out.push(if decode { entities::decode(text).into_owned() } else { text.to_string() });
            }
            c = self.doc.node(c).next;
        }
        out
    }

    fn document_children(&self, out: &mut Vec<u32>) {
        self.elem_children(0, out);
    }
}

/// Compile an IR expression for the lite engine.
pub fn compile(ir: &Expr) -> Result<Plan<Selector>> {
    compile_with(ir, None)
}

/// Like [`compile`] with a custom fingerprint store for `adaptive` selectors.
pub fn compile_with(ir: &Expr, store: Option<std::sync::Arc<dyn crate::adaptive::Store>>) -> Result<Plan<Selector>> {
    Plan::compile_with::<LiteDom<'static, 'static>>(ir, store)
}

impl Engine for Plan<Selector> {
    fn start(&self, src: &str, meta: Value) -> Result<Session> {
        let doc = LiteDoc::parse(src);
        let mut sess = self.start_dom(&doc.dom(), meta)?;
        sess.attach_source(None, src);
        Ok(sess)
    }
    fn resume(&self, sess: &mut Session, docs: &[(u64, String)]) -> Result<()> {
        let with_meta: Vec<(u64, String, Option<Value>)> =
            docs.iter().map(|(id, body)| (*id, body.clone(), None)).collect();
        self.resume_with_meta(sess, &with_meta)
    }
    fn resume_with_meta(&self, sess: &mut Session, docs: &[(u64, String, Option<Value>)]) -> Result<()> {
        let mut local: Vec<(u64, &str, Option<&Value>)> = Vec::new();
        let mut foreign: Vec<(u64, String, Option<Value>)> = Vec::new();
        for (id, body, meta) in docs {
            if sess.is_foreign_request(*id) {
                foreign.push((*id, body.clone(), meta.clone()));
            } else {
                local.push((*id, body.as_str(), meta.as_ref()));
            }
        }
        if !foreign.is_empty() {
            self.resume_foreign(sess, &foreign)?;
        }
        if !local.is_empty() {
            {
                let parsed: Vec<(u64, LiteDoc<'_>, Option<&Value>)> =
                    local.iter().map(|(i, h, m)| (*i, LiteDoc::parse(h), *m)).collect();
                let doms: Vec<(u64, LiteDom<'_, '_>, Option<&Value>)> =
                    parsed.iter().map(|(i, d, m)| (*i, d.dom(), *m)).collect();
                let refs: Vec<(u64, &LiteDom<'_, '_>, Option<&Value>)> =
                    doms.iter().map(|(i, d, m)| (*i, d, *m)).collect();
                self.resume_dom_with_meta(sess, &refs)?;
            }
            for (i, h, _) in local {
                sess.attach_source(Some(i), h);
            }
        }
        Ok(())
    }
    fn repair(&self, sess: &mut Session, answers: &[(u64, indexmap::IndexMap<String, String>)]) -> Result<()> {
        self.repair_with(sess, answers, &|css| Selector::parse(css).map(|_| ()).map_err(|e| e.to_string()), &mut |plan, sess, doc, src| {
            let parsed = LiteDoc::parse(src);
            plan.rerun_dom(sess, doc, &parsed.dom())
        })
    }
    fn explain(&self) -> String {
        Plan::explain(self)
    }
    fn ir(&self) -> &Expr {
        Plan::ir(self)
    }
    fn backend(&self) -> &'static str {
        Plan::backend(self)
    }
    fn adaptive_store(&self) -> std::sync::Arc<dyn crate::adaptive::Store> {
        std::sync::Arc::clone(&self.store)
    }
}

/// Compiled extraction plan that parses source text with the native zero-copy XML parser.
pub struct XmlPlan(Plan<Selector>);

/// Compile an IR expression for the `xml` engine.
pub fn compile_xml_with(
    ir: &Expr,
    store: Option<std::sync::Arc<dyn crate::adaptive::Store>>,
) -> Result<XmlPlan> {
    Ok(XmlPlan(Plan::compile_with_backend::<LiteDom<'static, 'static>>(ir, store, "xml")?))
}

impl Engine for XmlPlan {
    fn start(&self, src: &str, meta: Value) -> Result<Session> {
        let doc = LiteDoc::parse_xml(src);
        let mut sess = self.0.start_dom(&doc.dom(), meta)?;
        sess.attach_source(None, src);
        Ok(sess)
    }
    fn resume(&self, sess: &mut Session, docs: &[(u64, String)]) -> Result<()> {
        let with_meta: Vec<(u64, String, Option<Value>)> =
            docs.iter().map(|(id, body)| (*id, body.clone(), None)).collect();
        self.resume_with_meta(sess, &with_meta)
    }
    fn resume_with_meta(&self, sess: &mut Session, docs: &[(u64, String, Option<Value>)]) -> Result<()> {
        let mut local: Vec<(u64, &str, Option<&Value>)> = Vec::new();
        let mut foreign: Vec<(u64, String, Option<Value>)> = Vec::new();
        for (id, body, meta) in docs {
            if sess.is_foreign_request(*id) {
                foreign.push((*id, body.clone(), meta.clone()));
            } else {
                local.push((*id, body.as_str(), meta.as_ref()));
            }
        }
        if !foreign.is_empty() {
            self.0.resume_foreign(sess, &foreign)?;
        }
        if !local.is_empty() {
            {
                let parsed: Vec<(u64, LiteDoc<'_>, Option<&Value>)> =
                    local.iter().map(|(i, h, m)| (*i, LiteDoc::parse_xml(h), *m)).collect();
                let doms: Vec<(u64, LiteDom<'_, '_>, Option<&Value>)> =
                    parsed.iter().map(|(i, d, m)| (*i, d.dom(), *m)).collect();
                let refs: Vec<(u64, &LiteDom<'_, '_>, Option<&Value>)> =
                    doms.iter().map(|(i, d, m)| (*i, d, *m)).collect();
                self.0.resume_dom_with_meta(sess, &refs)?;
            }
            for (i, h, _) in local {
                sess.attach_source(Some(i), h);
            }
        }
        Ok(())
    }
    fn repair(&self, sess: &mut Session, answers: &[(u64, indexmap::IndexMap<String, String>)]) -> Result<()> {
        self.0.repair_with(sess, answers, &|css| Selector::parse(css).map(|_| ()).map_err(|e| e.to_string()), &mut |plan, sess, doc, src| {
            let parsed = LiteDoc::parse_xml(src);
            plan.rerun_dom(sess, doc, &parsed.dom())
        })
    }
    fn explain(&self) -> String {
        self.0.explain()
    }
    fn ir(&self) -> &Expr {
        self.0.ir()
    }
    fn backend(&self) -> &'static str {
        self.0.backend()
    }
    fn adaptive_store(&self) -> std::sync::Arc<dyn crate::adaptive::Store> {
        std::sync::Arc::clone(&self.0.store)
    }
}
