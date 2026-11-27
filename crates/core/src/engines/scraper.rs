//! `scraper` engine: html5ever (spec-compliant tree building) + `selectors` (full CSS).
//! XPath comes from the generic evaluator in [`crate::xpath`].
use crate::dom::{Dom, Walk};
use crate::engine::Engine;
use crate::error::Result;
use crate::exec::Session;
use crate::ir::Expr;
use crate::plan::Plan;
use crate::value::Value;
use ::scraper::{ElementRef, Html, Selector};
use std::borrow::Cow;
use std::collections::HashSet;

fn is_raw_text(name: &str) -> bool {
    name.eq_ignore_ascii_case("script") || name.eq_ignore_ascii_case("style")
}

/// Text nodes below `node` in document order; `<script>`/`<style>` content is skipped unless `node` is itself
/// one of them; subtrees whose element pointer is in `skip` are left out.
fn walk_text<'a, F: FnMut(&str)>(node: &ElementRef<'a>, skip: &HashSet<usize>, mut f: F) {
    let root_raw = is_raw_text(node.value().name());
    let mut stack = Vec::new();
    let kids: Vec<_> = node.children().collect();
    for k in kids.into_iter().rev() {
        stack.push((k, root_raw));
    }
    while let Some((n, raw_parent)) = stack.pop() {
        match n.value() {
            ::scraper::Node::Text(t) => {
                if !raw_parent || root_raw {
                    f(&**t);
                }
            }
            ::scraper::Node::Element(e) => {
                if !skip.is_empty() && skip.contains(&(e as *const _ as usize)) {
                    continue;
                }
                let raw = is_raw_text(e.name());
                let kids: Vec<_> = n.children().collect();
                for k in kids.into_iter().rev() {
                    stack.push((k, raw));
                }
            }
            _ => {}
        }
    }
}

pub struct HtmlDoc {
    html: Html,
}

impl HtmlDoc {
    pub fn parse(src: &str) -> Self {
        HtmlDoc {
            html: Html::parse_document(src),
        }
    }
    pub fn dom(&self) -> HtmlDom<'_> {
        HtmlDom { html: &self.html }
    }
}

pub struct HtmlDom<'a> {
    html: &'a Html,
}

impl<'a> Dom for HtmlDom<'a> {
    type Node = ElementRef<'a>;
    type Sel = Selector;
    const NAME: &'static str = "scraper";
    const XPATH: bool = true;

    fn compile_css(src: &str) -> std::result::Result<Selector, String> {
        Selector::parse(src).map_err(|e| e.to_string())
    }

    fn root(&self) -> ElementRef<'a> {
        self.html.root_element()
    }

    fn matches_sel(&self, node: &ElementRef<'a>, sel: &Selector) -> bool {
        sel.matches(node)
    }

    fn select(
        &self,
        node: &ElementRef<'a>,
        sel: &Selector,
        first_only: bool,
        out: &mut Vec<ElementRef<'a>>,
    ) {
        if *node == self.html.root_element() {
            let mut it = self.html.select(sel);
            if first_only {
                if let Some(x) = it.next() {
                    out.push(x);
                }
            } else {
                out.extend(it);
            }
            return;
        }
        let mut it = node.select(sel);
        if first_only {
            if let Some(x) = it.next() {
                out.push(x);
            }
        } else {
            out.extend(it);
        }
    }

    fn text(&self, node: &ElementRef<'a>) -> String {
        let mut out = String::new();
        walk_text(node, &HashSet::new(), |s| out.push_str(s));
        out
    }

    fn text_parts(&self, node: &ElementRef<'a>, skip: &HashSet<usize>, out: &mut Vec<String>) {
        walk_text(node, skip, |s| out.push(s.to_owned()));
    }

    fn walk(&self, node: &ElementRef<'a>, skip: &HashSet<usize>, f: &mut dyn FnMut(Walk<'_>)) -> bool {
        enum Step<N> {
            Enter(N),
            Exit(String),
        }
        let root = node.value();
        {
            let attr = |k: &str| root.attr(k).map(str::to_owned);
            f(Walk::Open { name: root.name(), attr: &attr });
        }
        let mut stack = vec![Step::Exit(root.name().to_owned())];
        let kids: Vec<_> = node.children().collect();
        for k in kids.into_iter().rev() {
            stack.push(Step::Enter(k));
        }
        while let Some(step) = stack.pop() {
            match step {
                Step::Exit(name) => f(Walk::Close { name: &name }),
                Step::Enter(n) => match n.value() {
                    ::scraper::Node::Text(t) => f(Walk::Text(&**t)),
                    ::scraper::Node::Element(e) => {
                        if !skip.is_empty() && skip.contains(&(e as *const _ as usize)) {
                            continue;
                        }
                        {
                            let attr = |k: &str| e.attr(k).map(str::to_owned);
                            f(Walk::Open { name: e.name(), attr: &attr });
                        }
                        stack.push(Step::Exit(e.name().to_owned()));
                        let kids: Vec<_> = n.children().collect();
                        for k in kids.into_iter().rev() {
                            stack.push(Step::Enter(k));
                        }
                    }
                    _ => {}
                },
            }
        }
        true
    }

    fn html(&self, node: &ElementRef<'a>, inner: bool) -> String {
        if inner {
            node.inner_html()
        } else {
            node.html()
        }
    }

    fn attr(&self, node: &ElementRef<'a>, name: &str) -> Option<String> {
        node.value().attr(name).map(str::to_owned)
    }

    fn to_value(&self, node: &ElementRef<'a>) -> Value {
        Value::Str(self.text(node))
    }

    fn node_key(&self, node: &ElementRef<'a>) -> usize {
        node.value() as *const _ as usize
    }

    fn parent(&self, node: &ElementRef<'a>) -> Option<ElementRef<'a>> {
        node.parent().and_then(ElementRef::wrap)
    }

    fn prev_sibling(&self, node: &ElementRef<'a>) -> Option<ElementRef<'a>> {
        let mut cur = node.prev_sibling();
        while let Some(s) = cur {
            if let Some(el) = ElementRef::wrap(s) {
                return Some(el);
            }
            cur = s.prev_sibling();
        }
        None
    }

    fn next_sibling(&self, node: &ElementRef<'a>) -> Option<ElementRef<'a>> {
        let mut cur = node.next_sibling();
        while let Some(s) = cur {
            if let Some(el) = ElementRef::wrap(s) {
                return Some(el);
            }
            cur = s.next_sibling();
        }
        None
    }

    fn children(&self, node: &ElementRef<'a>, out: &mut Vec<ElementRef<'a>>) {
        out.extend(node.children().filter_map(ElementRef::wrap));
    }

    fn name<'s>(&'s self, node: &ElementRef<'a>) -> Option<Cow<'s, str>> {
        Some(Cow::Borrowed(node.value().name()))
    }

    fn attrs(&self, node: &ElementRef<'a>, out: &mut Vec<(String, String)>) {
        out.extend(
            node.value()
                .attrs()
                .map(|(k, v)| (k.to_string(), v.to_string())),
        );
    }

    fn direct_texts(&self, node: &ElementRef<'a>) -> Vec<String> {
        node.children()
            .filter_map(|c| c.value().as_text().map(|t| (&**t).to_owned()))
            .collect()
    }
}

/// Compile an IR expression for this engine.
pub fn compile(ir: &Expr) -> Result<Plan<Selector>> {
    compile_with(ir, None)
}

/// Like [`compile`] with a custom fingerprint store for `adaptive` selectors.
pub fn compile_with(
    ir: &Expr,
    store: Option<std::sync::Arc<dyn crate::adaptive::Store>>,
) -> Result<Plan<Selector>> {
    Plan::compile_with::<HtmlDom<'static>>(ir, store)
}

impl Engine for Plan<Selector> {
    fn start(&self, src: &str, meta: Value) -> Result<Session> {
        let doc = HtmlDoc::parse(src);
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
                let parsed: Vec<(u64, HtmlDoc, Option<&Value>)> =
                    local.iter().map(|(i, h, m)| (*i, HtmlDoc::parse(h), *m)).collect();
                let doms: Vec<(u64, HtmlDom<'_>, Option<&Value>)> =
                    parsed.iter().map(|(i, d, m)| (*i, d.dom(), *m)).collect();
                let refs: Vec<(u64, &HtmlDom<'_>, Option<&Value>)> =
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
            let parsed = HtmlDoc::parse(src);
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
