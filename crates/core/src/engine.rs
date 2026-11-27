//! The engine registry: the backend-erased [`Engine`] trait and name-based compilation.
use crate::adaptive::{AdaptiveMiss, Store};
use crate::error::{Error, ErrorInfo, Result};
use std::sync::Arc;
use crate::exec::Session;
use crate::ir::Expr;
use crate::value::Value;

/// Result of [`Engine::extract_detailed`]: the data plus what did not make the extraction fail.
#[derive(Debug, Clone)]
pub struct Detailed {
    pub value: Value,
    /// records quarantined by `map(..., on_error="skip" | "null")`
    pub errors: Vec<ErrorInfo>,
    /// adaptive selectors that found nothing and could not be relocated
    pub misses: Vec<AdaptiveMiss>,
}

/// Backend-erased handle: parses source text and runs a plan. Use this from bindings / generic code.
pub trait Engine: Send + Sync {
    /// Parse `src` and run the root schema.
    fn start(&self, src: &str, meta: Value) -> Result<Session>;
    /// Parse the fetched documents and continue a session.
    fn resume(&self, sess: &mut Session, docs: &[(u64, String)]) -> Result<()>;
    /// Parse fetched documents with optional per-response metadata (`headers`, `status`, `base_url`) and continue a session.
    fn resume_with_meta(&self, sess: &mut Session, docs: &[(u64, String, Option<Value>)]) -> Result<()> {
        let plain: Vec<(u64, String)> = docs.iter().map(|(id, s, _)| (*id, s.clone())).collect();
        self.resume(sess, &plain)
    }
    fn explain(&self) -> String;
    fn ir(&self) -> &Expr;
    fn backend(&self) -> &'static str;
    /// The fingerprint store of this plan's `adaptive` selectors (an empty one for engines without them).
    /// Answer the session's repair requests: `(repair id, {key: css})`. The affected documents are re-run with the
    /// new selectors (follow requests that the new run repeats keep their ids); an empty map declines a repair.
    fn repair(&self, sess: &mut Session, answers: &[(u64, indexmap::IndexMap<String, String>)]) -> Result<()> {
        let _ = (sess, answers);
        Err(Error::Session(format!("the `{}` engine has no adaptive selectors to repair", self.backend())))
    }
    fn adaptive_store(&self) -> std::sync::Arc<dyn crate::adaptive::Store> {
        std::sync::Arc::new(crate::adaptive::MemoryStore::default())
    }

    /// One-shot extraction. Errors if the schema contains `follow` (use `start`/`resume` or `extract_with`).
    fn extract(&self, src: &str, meta: Value) -> Result<Value> {
        let mut s = self.start(src, meta)?;
        if !s.is_done() {
            return Err(Error::Session("schema contains `follow`; use extract_with / start+resume".into()));
        }
        s.take_result()
    }

    /// Like [`Engine::extract`], but also reports quarantined records and adaptive misses.
    fn extract_detailed(&self, src: &str, meta: Value) -> Result<Detailed> {
        let mut s = self.start(src, meta)?;
        if !s.is_done() {
            return Err(Error::Session("schema contains `follow`; use start/resume".into()));
        }
        let (errors, misses) = (s.errors().to_vec(), s.misses().to_vec());
        Ok(Detailed { value: s.take_result()?, errors, misses })
    }

    /// Synchronous driver: calls `fetch(&request)` for every pending request until done.
    fn extract_with<F>(&self, src: &str, meta: Value, mut fetch: F) -> Result<Value>
    where
        Self: Sized,
        F: FnMut(&crate::exec::Request) -> std::result::Result<String, String>,
    {
        let mut s = self.start(src, meta)?;
        while !s.is_done() {
            let mut docs = Vec::new();
            for r in s.pending() {
                let body = fetch(r).map_err(|e| Error::Session(format!("fetch {}: {e}", r.url().unwrap_or("<request>"))))?;
                docs.push((r.id, body));
            }
            self.resume(&mut s, &docs)?;
        }
        s.take_result()
    }
}

/// Description of an engine compiled into this build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineInfo {
    pub name: &'static str,
    /// `dom`, `stream` or `data`
    pub kind: &'static str,
    pub xpath: bool,
    pub description: &'static str,
}

/// Engines compiled into this build.
pub fn available() -> Vec<EngineInfo> {
    #[allow(unused_mut)]
    let mut v: Vec<EngineInfo> = Vec::new();
    #[cfg(feature = "engine-scraper")]
    v.push(EngineInfo { name: "scraper", kind: "dom", xpath: true, description: "html5ever DOM, spec-correct HTML5, full CSS" });
    #[cfg(feature = "engine-lite")]
    v.push(EngineInfo { name: "lite", kind: "dom", xpath: true, description: "zero-copy arena DOM, small and fast, common HTML rules" });
    #[cfg(feature = "engine-stream")]
    v.push(EngineInfo { name: "stream", kind: "stream", xpath: false, description: "single pass without a DOM for capture-style schemas" });
    #[cfg(feature = "engine-json")]
    v.push(EngineInfo { name: "json", kind: "data", xpath: false, description: "schemas over JSON documents" });
    #[cfg(feature = "engine-lite")]
    v.push(EngineInfo { name: "xml", kind: "dom", xpath: true, description: "zero-copy XML / Sitemap / RSS / Atom DOM with namespaces" });
    v
}

fn enabled() -> String {
    let names: Vec<&str> = available().iter().map(|e| e.name).collect();
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    }
}

fn missing(name: &str, feature: &str) -> Error {
    Error::Ir(format!(
        "engine `{name}` is not compiled into this build (enabled: {}). Rebuild with the cargo feature `{feature}`",
        enabled()
    ))
}

#[allow(unused_variables, unreachable_code)]
fn scraper(ir: &Expr, store: Option<Arc<dyn Store>>) -> Result<Box<dyn Engine>> {
    #[cfg(feature = "engine-scraper")]
    {
        return Ok(Box::new(crate::engines::scraper::compile_with(ir, store)?));
    }
    Err(missing("scraper", "engine-scraper"))
}

#[allow(unused_variables, unreachable_code)]
fn lite(ir: &Expr, store: Option<Arc<dyn Store>>) -> Result<Box<dyn Engine>> {
    #[cfg(feature = "engine-lite")]
    {
        return Ok(Box::new(crate::engines::lite::compile_with(ir, store)?));
    }
    Err(missing("lite", "engine-lite"))
}

#[allow(unused_variables, unreachable_code)]
fn stream(ir: &Expr, _store: Option<Arc<dyn Store>>) -> Result<Box<dyn Engine>> {
    #[cfg(feature = "engine-stream")]
    {
        return Ok(Box::new(crate::engines::stream::StreamPlan::compile(ir)?));
    }
    Err(missing("stream", "engine-stream"))
}

#[allow(unused_variables, unreachable_code)]
fn json(ir: &Expr, store: Option<Arc<dyn Store>>) -> Result<Box<dyn Engine>> {
    #[cfg(feature = "engine-json")]
    {
        return Ok(Box::new(crate::engines::json::compile_with(ir, store)?));
    }
    Err(missing("json", "engine-json"))
}

#[allow(unused_variables, unreachable_code)]
fn xml(ir: &Expr, store: Option<Arc<dyn Store>>) -> Result<Box<dyn Engine>> {
    #[cfg(feature = "engine-lite")]
    {
        return Ok(Box::new(crate::engines::lite::compile_xml_with(ir, store)?));
    }
    Err(missing("xml", "engine-lite"))
}

/// `html`: the first HTML DOM engine that is compiled in (scraper, then lite).
#[allow(unused_variables, unreachable_code)]
fn html(ir: &Expr, store: Option<Arc<dyn Store>>) -> Result<Box<dyn Engine>> {
    #[cfg(feature = "engine-scraper")]
    {
        return match scraper(ir, store.clone()) {
            Ok(e) => Ok(e),
            // `:scope` and selectors handled only by `lite` (`:contains`, `:disabled`, `:enabled`, `:checked`,
            // `:any-link`) fall back to `lite` on `html` / `auto`
            #[cfg(feature = "engine-lite")]
            Err(e)
                if e.to_string().contains(":scope")
                    || e.to_string().contains("UnsupportedPseudoClassOrElement") =>
            {
                lite(ir, store)
            }
            Err(e) => Err(e),
        };
    }
    #[cfg(all(not(feature = "engine-scraper"), feature = "engine-lite"))]
    {
        return lite(ir, store);
    }
    Err(missing("html", "engine-scraper or engine-lite"))
}

/// Compile `ir` for the engine called `name`:
/// `scraper`, `lite`, `stream`, `json`, plus `html` (best compiled-in DOM engine) and
/// `auto` (`stream` when the schema is streamable, otherwise `html`).
///
/// # Examples
///
/// ```rust
/// # #[cfg(feature = "engine-lite")]
/// # {
/// use oxide_parse_core::{compile, Expr, Value};
///
/// let ir = Expr::from_json(r#"{"pipe":[{"css":"b"},"text","int"]}"#).unwrap();
/// let plan = compile(&ir, "lite").unwrap();
/// assert_eq!(plan.extract("<b>42</b>", Value::Null).unwrap(), Value::Int(42));
/// # }
/// ```
pub fn compile(ir: &Expr, name: &str) -> Result<Box<dyn Engine>> {
    compile_with(ir, name, None)
}

/// [`compile`] with a fingerprint store for `adaptive` selectors (default: in memory, per plan).
pub fn compile_with(ir: &Expr, name: &str, store: Option<Arc<dyn Store>>) -> Result<Box<dyn Engine>> {
    match name {
        "scraper" => scraper(ir, store),
        "lite" => lite(ir, store),
        "stream" => stream(ir, store),
        "json" => json(ir, store),
        "xml" => xml(ir, store),
        "html" => html(ir, store),
        "auto" => {
            #[cfg(feature = "engine-stream")]
            {
                if let Ok(e) = stream(ir, store.clone()) {
                    return Ok(e);
                }
            }
            html(ir, store)
        }
        other => Err(Error::Ir(format!(
            "unknown engine {other:?}. Known: scraper, lite, stream, json, xml, html, auto (compiled in: {})",
            enabled()
        ))),
    }
}
