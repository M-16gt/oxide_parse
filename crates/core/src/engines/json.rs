//! JSON engine: the same schemas over `serde_json` data (`key`, `at`, `children`, `map`, ...).
use crate::dom::Dom;
use crate::engine::Engine;
use crate::exec::Session;
use crate::error::{Error, Result};
use crate::ir::Expr;
use crate::plan::Plan;
use crate::value::Value;
use serde_json::Value as J;

pub struct JsonDoc {
    v: J,
}

impl JsonDoc {
    pub fn parse(src: &str) -> Result<Self> {
        serde_json::from_str(src).map(|v| JsonDoc { v }).map_err(|e| Error::Doc(e.to_string()))
    }
    pub fn from_value(v: J) -> Self {
        JsonDoc { v }
    }
    pub fn dom(&self) -> JsonDom<'_> {
        JsonDom { v: &self.v }
    }
}

pub struct JsonDom<'a> {
    v: &'a J,
}

fn j_text(n: &J) -> String {
    match n {
        J::String(s) => s.clone(),
        J::Null => String::new(),
        other => other.to_string(),
    }
}

impl<'a> Dom for JsonDom<'a> {
    type Node = &'a J;
    type Sel = ();
    const NAME: &'static str = "json";

    fn compile_css(_src: &str) -> std::result::Result<(), String> {
        Err("css selectors are not available on the `json` engine; use key / at / children".into())
    }
    fn root(&self) -> &'a J {
        self.v
    }
    fn select(&self, _n: &&'a J, _s: &(), _first: bool, _out: &mut Vec<&'a J>) {}
    fn text(&self, n: &&'a J) -> String {
        j_text(n)
    }
    fn html(&self, n: &&'a J, _inner: bool) -> String {
        n.to_string()
    }
    fn attr(&self, n: &&'a J, name: &str) -> Option<String> {
        n.get(name).filter(|v| !v.is_null()).map(j_text)
    }
    fn node_is_null(&self, n: &&'a J) -> bool {
        n.is_null()
    }
    fn node_truthy(&self, n: &&'a J) -> bool {
        match *n {
            J::Null => false,
            J::Bool(b) => *b,
            J::Number(num) => num
                .as_i64()
                .map_or_else(|| num.as_f64().map_or(false, |f| f != 0.0), |i| i != 0),
            J::String(s) => !s.is_empty(),
            J::Array(a) => !a.is_empty(),
            J::Object(o) => !o.is_empty(),
        }
    }
    fn node_is_empty(&self, n: &&'a J) -> bool {
        match *n {
            J::Null => true,
            J::String(s) => s.trim().is_empty(),
            J::Array(a) => a.is_empty(),
            J::Object(o) => o.is_empty(),
            _ => false,
        }
    }
    fn to_value(&self, n: &&'a J) -> Value {
        Value::from_json_ref(*n)
    }
    fn node_key(&self, n: &&'a J) -> usize {
        (*n) as *const J as usize
    }
    fn children(&self, n: &&'a J, out: &mut Vec<&'a J>) {
        match *n {
            J::Array(a) => out.extend(a.iter()),
            J::Object(o) => out.extend(o.values()),
            _ => {}
        }
    }
    fn key(&self, n: &&'a J, key: &str) -> std::result::Result<Option<&'a J>, String> {
        Ok(n.get(key).filter(|v| !v.is_null()))
    }
    fn at(&self, n: &&'a J, i: usize) -> std::result::Result<Option<&'a J>, String> {
        Ok(n.get(i).filter(|v| !v.is_null()))
    }
}

pub fn compile(ir: &Expr) -> Result<Plan<()>> {
    compile_with(ir, None)
}

/// Like [`compile`] with a custom fingerprint store for `adaptive` selectors.
pub fn compile_with(ir: &Expr, store: Option<std::sync::Arc<dyn crate::adaptive::Store>>) -> Result<Plan<()>> {
    Plan::compile_with::<JsonDom<'static>>(ir, store)
}

impl Engine for Plan<()> {
    fn start(&self, src: &str, meta: Value) -> Result<Session> {
        let doc = JsonDoc::parse(src)?;
        self.start_dom(&doc.dom(), meta)
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
            let parsed = local
                .iter()
                .map(|(i, h, m)| JsonDoc::parse(h).map(|d| (*i, d, *m)))
                .collect::<Result<Vec<_>>>()?;
            let doms: Vec<(u64, JsonDom<'_>, Option<&Value>)> =
                parsed.iter().map(|(i, d, m)| (*i, d.dom(), *m)).collect();
            let refs: Vec<(u64, &JsonDom<'_>, Option<&Value>)> =
                doms.iter().map(|(i, d, m)| (*i, d, *m)).collect();
            self.resume_dom_with_meta(sess, &refs)?;
        }
        Ok(())
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
}
