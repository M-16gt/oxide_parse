//! Plan executor, generic over [`Dom`]. Pure CPU: no IO, no async, no GIL.
//! `follow` never fetches anything: it yields a [`Request`] and the caller feeds the
//! response back with [`Plan::resume_dom`] (a small state machine; sync and async drivers are trivial).
use crate::adaptive::{self, AdaptiveMiss};
use crate::dom::{Dom, Walk};
use crate::error::{fmt_path, Error, ErrorInfo, Result};
use crate::ir::OnError;
use crate::plan::{CExpr, COp, JsonSeg, Plan};
use crate::value::Value;
use crate::xpath::{self, XValue};
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

/// The document id of the root document in a [`Session`] (sub-documents use the id of the request that fetched them).
const ROOT: u64 = u64::MAX;

/// Safety ceiling on pagination when `max_pages` is not set.
const DEFAULT_MAX_PAGES: usize = 10_000;

/// What kind of sub-request produced a [`Request`].
#[derive(Debug, Clone)]
pub(crate) enum ReqKind {
    Follow {
        sub: usize,
    },
    ForeignStart {
        foreign: usize,
    },
    ForeignStep {
        root_id: u64,
        sub_req_id: u64,
    },
    PaginatePage {
        pager: usize,
        acc: Vec<Value>,
        pages: usize,
        seen: HashSet<String>,
        path: Vec<String>,
    },
}

/// Which plan entry runs for a stored document in a [`Session`].
#[derive(Debug, Clone)]
enum DocKind {
    Root,
    Sub(usize),
    Foreign,
    Paginate {
        pager: usize,
        acc_before: Vec<Value>,
        pages_before: usize,
        seen_before: HashSet<String>,
        path: Vec<String>,
    },
}

/// A sub-document the caller must fetch for a `follow` or `paginate`.
///
/// `params` is the evaluated `request` dict (`url`, `headers`, `method`, ... whatever the schema put there).
/// The engine never interprets it: drivers pass it to the user's fetcher as `fetch(**params)`.
#[derive(Debug, Clone)]
pub struct Request {
    pub id: u64,
    pub params: IndexMap<String, Value>,
    pub meta: Value,
    pub(crate) kind: ReqKind,
    /// the document whose run issued this request ([`ROOT`] or another request's id)
    pub(crate) origin: u64,
}

impl Request {
    /// Convenience accessor for the conventional `url` key.
    pub fn url(&self) -> Option<&str> {
        match self.params.get("url") {
            Some(Value::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }
}

/// An `adaptive` selector that found nothing and could not be relocated, waiting for a new CSS selector: the second
/// kind of request of a [`Session`] (next to [`Request`]). The caller answers with `{key: css}` (an LLM, a human, a
/// lookup table) or declines with an empty map; the session then re-runs the document with the new selector.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RepairRequest {
    pub id: u64,
    /// the document the selector failed on: `None` = the root document, `Some(id)` = fetched by `follow` request `id`
    pub doc: Option<u64>,
    /// `params["url"]` of that `follow` request, if any
    pub url: Option<String>,
    pub key: String,
    /// the selector that no longer matches
    pub css: String,
    /// what the element used to look like (with `samples` of the values it produced); `None`: never learned
    pub fingerprint: Option<adaptive::Fingerprint>,
    /// the container the element used to live in, as found in the page now (`div#shop.catalog`)
    pub container: Option<String>,
    /// skeleton HTML: that container distilled on its own, or the whole page distilled when it was not found
    pub skeleton: Option<String>,
    /// `"container"` or `"page"` (what `skeleton` shows); `None` without the `lite` engine in the build
    pub scope: Option<String>,
    /// why the previous answer for this key was rejected (the CSS parser's message): the model can fix its syntax
    pub previous_error: Option<String>,
    /// how many answers for this key were rejected as invalid CSS (the second one declines the request)
    pub invalid_answers: u32,
}

/// What a session remembers about one document it has run.
#[derive(Debug)]
struct DocRec {
    which: DocKind,
    meta: Value,
    /// the document whose run issued the request for this one
    origin: u64,
    /// identity of the request (to recognise it again after a re-run of the document that issued it)
    key: String,
    url: Option<String>,
    /// kept only while the document has unresolved misses: a repair re-runs it
    source: Option<String>,
    errors: Vec<ErrorInfo>,
    misses: Vec<AdaptiveMiss>,
}

fn req_key(kind: &ReqKind, params: &IndexMap<String, Value>, meta: &Value) -> String {
    match kind {
        ReqKind::Follow { sub } => format!("{sub}|{params:?}|{meta:?}"),
        ReqKind::ForeignStart { foreign } => format!("foreign:{foreign}|{params:?}|{meta:?}"),
        ReqKind::ForeignStep { root_id, sub_req_id } => format!("fstep:{root_id}|{sub_req_id}|{params:?}|{meta:?}"),
        ReqKind::PaginatePage { pager, acc, pages, .. } => {
            format!("pag:{pager}|{pages}|{acc:?}|{params:?}|{meta:?}")
        }
    }
}

fn merge_response_meta(base: &Value, extra: Option<&Value>) -> Value {
    let Some(Value::Map(em)) = extra else {
        return base.clone();
    };
    let mut map = match base {
        Value::Map(m) => m.clone(),
        _ => IndexMap::new(),
    };
    for (k, v) in em {
        map.insert(k.clone(), v.clone());
    }
    Value::Map(map)
}

#[derive(Debug)]
struct ForeignRun {
    foreign: usize,
    sess: Session,
    meta: Value,
    pending_sub_ids: HashSet<u64>,
}

struct ValueDom;

impl Dom for ValueDom {
    type Node = ();
    type Sel = ();
    const NAME: &'static str = "value";
    fn compile_css(_src: &str) -> std::result::Result<(), String> {
        Err("css selectors are not supported in post-follow value pipelines".into())
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

fn strip_url_fragment(u: &str) -> &str {
    u.split('#').next().unwrap_or(u)
}

fn is_empty_or_noop_url(u: &str) -> bool {
    let t = u.trim();
    t.is_empty()
        || t == "#"
        || t.eq_ignore_ascii_case("javascript:void(0)")
        || t.eq_ignore_ascii_case("javascript:void(0);")
        || t.eq_ignore_ascii_case("javascript:;")
        || t.eq_ignore_ascii_case("javascript:")
}

fn canonical_paginate_key(params: &IndexMap<String, Value>) -> String {
    if params.len() == 1 {
        if let Some(Value::Str(u)) = params.get("url") {
            return format!("url:{}", strip_url_fragment(u.trim()));
        }
    }
    let mut norm = params.clone();
    if let Some(Value::Str(u)) = norm.get_mut("url") {
        *u = strip_url_fragment(u.trim()).to_string();
    }
    format!("params:{norm:?}")
}

/// State of an extraction that may need more documents (`follow` / `paginate`) or new selectors (`adaptive` repairs).
///
/// One loop drives both: `pending()` are documents to fetch, `repairs()` are selectors to fix; `resume` /
/// `repair` feed the answers back. Repairs are optional: `take_result` only needs the fetches.
///
/// # Examples
///
/// ```rust
/// # #[cfg(feature = "engine-lite")]
/// # {
/// use oxide_parse_core::{compile, Expr, Value};
///
/// let ir = Expr::from_json(
///     r#"{"pipe":[{"follow":{"request":{"pipe":[{"css":"a"},{"attr":"href"}]},"schema":{"pipe":[{"css":"b"},"text"]}}}]}"#,
/// ).unwrap();
/// let plan = compile(&ir, "lite").unwrap();
/// let mut sess = plan.start("<a href='https://ex.test/p'>go</a>", Value::Null).unwrap();
/// let req_id = sess.pending()[0].id;
/// plan.resume(&mut sess, &[(req_id, "<b>done</b>".to_string())]).unwrap();
/// assert_eq!(sess.take_result().unwrap(), Value::Str("done".into()));
/// # }
/// ```
#[derive(Debug)]
pub struct Session {
    /// the root document's value; `Value::Pending(id)` marks a document still to be inserted
    tree: Option<Value>,
    outstanding: IndexMap<u64, Request>,
    /// values of fetched documents, inserted into the tree by `take_result`
    results: HashMap<u64, Value>,
    docs: IndexMap<u64, DocRec>,
    foreign: HashMap<u64, ForeignRun>,
    repairs: IndexMap<u64, RepairRequest>,
    /// `"{doc}|{key}"` of repairs the caller declined
    declined: HashSet<String>,
    /// selectors given by repairs, applied to every document run by this session
    overrides: IndexMap<String, String>,
    next_id: u64,
    errors: Vec<ErrorInfo>,
    misses: Vec<AdaptiveMiss>,
}

type Checkpoint = (IndexMap<String, String>, IndexMap<u64, RepairRequest>, HashSet<String>);

impl Session {
    fn new(meta: Value, value: Value, reqs: Vec<Request>, errors: Vec<ErrorInfo>, misses: Vec<AdaptiveMiss>, next_id: u64) -> Session {
        let mut docs = IndexMap::new();
        docs.insert(
            ROOT,
            DocRec { which: DocKind::Root, meta, origin: ROOT, key: String::new(), url: None, source: None, errors: Vec::new(), misses: Vec::new() },
        );
        let mut s = Session {
            tree: Some(value),
            outstanding: IndexMap::new(),
            results: HashMap::new(),
            docs,
            foreign: HashMap::new(),
            repairs: IndexMap::new(),
            declined: HashSet::new(),
            overrides: IndexMap::new(),
            next_id,
            errors: Vec::new(),
            misses: Vec::new(),
        };
        for mut r in reqs {
            r.origin = ROOT;
            s.outstanding.insert(r.id, r);
        }
        if let Some(rec) = s.docs.get_mut(&ROOT) {
            rec.errors = errors;
            rec.misses = misses;
        }
        s.sync_repairs(ROOT);
        s.rebuild();
        s
    }

    /// A finished session with the records quarantined while it was computed (stream engine).
    pub(crate) fn from_parts(v: Value, errors: Vec<ErrorInfo>) -> Session {
        Session::new(Value::Null, v, Vec::new(), errors, Vec::new(), 0)
    }
    /// Records quarantined by `map(..., on_error="skip" | "null")` so far.
    pub fn errors(&self) -> &[ErrorInfo] {
        &self.errors
    }
    /// Adaptive selectors that found nothing and could not be relocated so far.
    pub fn misses(&self) -> &[AdaptiveMiss] {
        &self.misses
    }
    /// Documents to fetch.
    pub fn pending(&self) -> Vec<&Request> {
        self.outstanding.values().collect()
    }
    /// Selectors to repair (answer with `Engine::repair`; an empty answer declines).
    pub fn repairs(&self) -> Vec<&RepairRequest> {
        self.repairs.values().collect()
    }
    /// The selectors given by repairs so far (`key -> css`): feed them to `with_selectors` to keep them.
    pub fn overrides(&self) -> &IndexMap<String, String> {
        &self.overrides
    }
    /// No document left to fetch (open repairs do not count: they are optional).
    pub fn is_done(&self) -> bool {
        self.outstanding.is_empty()
    }
    pub fn take_result(&mut self) -> Result<Value> {
        if !self.outstanding.is_empty() {
            return Err(Error::Session(format!(
                "{} follow request(s) still pending",
                self.outstanding.len()
            )));
        }
        let mut tree = self
            .tree
            .take()
            .ok_or_else(|| Error::Session("result already taken".into()))?;
        fill(&mut tree, &mut self.results, 0)?;
        Ok(tree)
    }

    /// Keep the source of a document that has unresolved misses: a repair re-runs it, and its skeleton is built from it.
    pub(crate) fn attach_source(&mut self, doc: Option<u64>, src: &str) {
        let id = doc.unwrap_or(ROOT);
        if let Some(rec) = self.docs.get_mut(&id) {
            if rec.misses.is_empty() {
                return;
            }
            rec.source = Some(src.to_string());
        }
        self.fill_skeletons(id);
    }

    pub(crate) fn is_foreign_request(&self, id: u64) -> bool {
        matches!(
            self.outstanding.get(&id).map(|r| &r.kind),
            Some(ReqKind::ForeignStart { .. } | ReqKind::ForeignStep { .. })
        )
    }

    fn source_of(&self, id: u64) -> Result<String> {
        self.docs
            .get(&id)
            .and_then(|r| r.source.clone())
            .ok_or_else(|| Error::Session(format!("the source of document {id} was not kept: it has no unresolved selectors")))
    }

    fn checkpoint(&self) -> Checkpoint {
        (self.overrides.clone(), self.repairs.clone(), self.declined.clone())
    }

    fn restore(&mut self, c: Checkpoint) {
        self.overrides = c.0;
        self.repairs = c.1;
        self.declined = c.2;
    }

    /// Aggregates (`errors`, `misses`) from the per-document records.
    fn rebuild(&mut self) {
        let mut errors: Vec<ErrorInfo> = Vec::new();
        let mut misses: Vec<AdaptiveMiss> = Vec::new();
        for rec in self.docs.values() {
            errors.extend(rec.errors.iter().cloned());
            for m in &rec.misses {
                if !misses.iter().any(|x| x.key == m.key) {
                    misses.push(m.clone());
                }
            }
        }
        self.errors = errors;
        self.misses = misses;
    }

    /// One repair request per (document, broken key) that is neither answered nor declined.
    fn sync_repairs(&mut self, doc: u64) {
        let (misses, url) = match self.docs.get(&doc) {
            Some(r) => (r.misses.clone(), r.url.clone()),
            None => return,
        };
        let owner = if doc == ROOT { None } else { Some(doc) };
        self.repairs.retain(|_, r| r.doc != owner || misses.iter().any(|m| m.key == r.key));
        for m in misses {
            let declined = self.declined.contains(&format!("{doc}|{}", m.key));
            let exists = self.repairs.values().any(|r| r.doc == owner && r.key == m.key);
            if declined || exists {
                continue;
            }
            let id = self.next_id;
            self.next_id += 1;
            self.repairs.insert(
                id,
                RepairRequest {
                    id,
                    doc: owner,
                    url: url.clone(),
                    key: m.key,
                    css: m.css,
                    fingerprint: m.fingerprint,
                    container: None,
                    skeleton: None,
                    scope: None,
                    previous_error: None,
                    invalid_answers: 0,
                },
            );
        }
        self.fill_skeletons(doc);
    }

    /// Distil the broken selector's old container (or the page) into the repair requests of `doc`.
    fn fill_skeletons(&mut self, doc: u64) {
        let owner = if doc == ROOT { None } else { Some(doc) };
        let src = match self.docs.get(&doc).and_then(|r| r.source.clone()) {
            Some(s) => s,
            None => return,
        };
        // several fields of one redesigned card break together: distil that container once
        let mut cache: HashMap<String, (Option<String>, Option<String>, Option<String>)> = HashMap::new();
        for r in self.repairs.values_mut().filter(|r| r.doc == owner && r.skeleton.is_none()) {
            let ck = match r.fingerprint.as_ref().and_then(|f| f.parent.as_ref().map(|p| (p, &f.path))) {
                Some((p, path)) => format!("{p:?}|{path:?}"),
                None => "page".to_string(),
            };
            let entry = cache.entry(ck).or_insert_with(|| make_skeleton(&src, r.fingerprint.as_ref())).clone();
            r.container = entry.0;
            r.skeleton = entry.1;
            r.scope = entry.2;
        }
    }

    /// Remove a document and everything it issued.
    fn drop_doc(&mut self, id: u64) {
        let mut stack = vec![id];
        while let Some(d) = stack.pop() {
            self.outstanding.shift_remove(&d);
            self.results.remove(&d);
            self.docs.shift_remove(&d);
            self.foreign.remove(&d);
            self.repairs.retain(|_, r| r.doc != Some(d));
            let kids: Vec<u64> = self
                .outstanding
                .values()
                .filter(|r| r.origin == d)
                .map(|r| r.id)
                .chain(self.docs.iter().filter(|(_, r)| r.origin == d).map(|(i, _)| *i))
                .collect();
            stack.extend(kids);
        }
    }

    /// Store the outcome of (re-)running document `doc`. Requests that the new run repeats keep their ids (a fetch
    /// in flight stays valid, a fetched document is not fetched again); the others are dropped / new.
    fn install_run(&mut self, doc: u64, value: Value, reqs: Vec<Request>, errors: Vec<ErrorInfo>, misses: Vec<AdaptiveMiss>) {
        let mut old: HashMap<String, VecDeque<u64>> = HashMap::new();
        for (id, r) in &self.outstanding {
            if r.origin == doc {
                old.entry(req_key(&r.kind, &r.params, &r.meta)).or_default().push_back(*id);
            }
        }
        for (id, d) in &self.docs {
            if *id != ROOT && d.origin == doc && !self.outstanding.contains_key(id) {
                old.entry(d.key.clone()).or_default().push_back(*id);
            }
        }
        for ids in old.values_mut() {
            ids.make_contiguous().sort_unstable();
        }
        let mut remap: HashMap<u64, u64> = HashMap::new();
        let mut fresh: Vec<Request> = Vec::new();
        for mut r in reqs {
            let k = req_key(&r.kind, &r.params, &r.meta);
            if let Some(old_id) = old.get_mut(&k).and_then(VecDeque::pop_front) {
                remap.insert(r.id, old_id);
                continue;
            }
            r.origin = doc;
            fresh.push(r);
        }
        for id in old.into_values().flatten() {
            self.drop_doc(id);
        }
        let mut value = value;
        if !remap.is_empty() {
            remap_pending(&mut value, &remap);
        }
        for r in fresh {
            self.outstanding.insert(r.id, r);
        }
        if doc == ROOT {
            self.tree = Some(value);
        } else {
            self.results.insert(doc, value);
        }
        if let Some(rec) = self.docs.get_mut(&doc) {
            if misses.is_empty() {
                rec.source = None;
            }
            rec.errors = errors;
            rec.misses = misses;
        }
        self.sync_repairs(doc);
        self.rebuild();
    }
}

/// `(container label, skeleton, scope)` for a repair request.
#[allow(unused_variables)]
fn make_skeleton(src: &str, fp: Option<&adaptive::Fingerprint>) -> (Option<String>, Option<String>, Option<String>) {
    #[cfg(feature = "engine-lite")]
    {
        use crate::markup::distill::{distill, distill_container, DistillOptions};
        if let Some(f) = fp {
            if let Some(p) = &f.parent {
                let opts = DistillOptions { keep: 2, max_text: 80, max_chars: 8_000 };
                if let Some((label, sk)) = distill_container(src, &p.tag, p.id.as_deref(), &p.classes, &f.path, &opts) {
                    return (Some(label), Some(sk), Some("container".to_string()));
                }
            }
        }
        let opts = DistillOptions { keep: 2, max_text: 80, max_chars: 12_000 };
        return (None, Some(distill(src, &opts)), Some("page".to_string()));
    }
    #[cfg(not(feature = "engine-lite"))]
    {
        (None, None, None)
    }
}

/// Insert the fetched documents into the tree. `hops` counts nested `follow`s (a bug that reused an id could make a
/// document contain itself); a `Pending` without a result is an error instead of a silent hole.
fn fill(v: &mut Value, res: &mut HashMap<u64, Value>, hops: usize) -> Result<()> {
    if let Value::Pending(mut id) = *v {
        if hops >= 64 {
            return Err(Error::Session("follow nesting deeper than 64 levels".into()));
        }
        // Resolve linear `Pending -> Pending -> ...` chains iteratively (deep pagination stays O(1) stack).
        let mut steps = 0usize;
        let mut r = loop {
            steps += 1;
            if steps > 100_000 {
                return Err(Error::Session("pending chain exceeded 100000 steps".into()));
            }
            let next = res
                .remove(&id)
                .ok_or_else(|| Error::Session(format!("unresolved follow request id {id}")))?;
            match next {
                Value::Pending(next_id) => id = next_id,
                other => break other,
            }
        };
        fill(&mut r, res, hops + 1)?;
        *v = r;
        return Ok(());
    }
    match v {
        Value::List(l) => {
            for x in l.iter_mut() {
                fill(x, res, hops)?;
            }
        }
        Value::Map(m) => {
            for x in m.values_mut() {
                fill(x, res, hops)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn remap_pending(v: &mut Value, map: &HashMap<u64, u64>) {
    match v {
        Value::Pending(id) => {
            if let Some(to) = map.get(id) {
                *id = *to;
            }
        }
        Value::List(l) => l.iter_mut().for_each(|x| remap_pending(x, map)),
        Value::Map(m) => m.values_mut().for_each(|x| remap_pending(x, map)),
        _ => {}
    }
}

impl<S: Send + Sync> Plan<S> {
    /// Selectors given by repairs, compiled for the engine `D` (invalid CSS is an error).
    fn compile_overrides<D: Dom<Sel = S>>(&self, o: &IndexMap<String, String>) -> Result<HashMap<String, (String, S)>> {
        let mut out = HashMap::new();
        for (k, css) in o {
            let sel = D::compile_css(css).map_err(|m| Error::Session(format!("repair for {k:?}: invalid selector {css:?}: {m}")))?;
            out.insert(k.clone(), (css.clone(), sel));
        }
        Ok(out)
    }

    /// Run the root schema against `dom`.
    pub fn start_dom<D: Dom<Sel = S>>(&self, dom: &D, meta: Value) -> Result<Session> {
        let mut next_id = 0;
        let none: HashMap<String, (String, S)> = HashMap::new();
        let out = self.run(&DocKind::Root, dom, &meta, &mut next_id, &none)?;
        Ok(Session::new(meta, out.value, out.reqs, out.errors, out.misses, next_id))
    }

    /// Feed fetched documents (by request id) back in. New requests may appear in `sess.pending()`.
    pub fn resume_dom<D: Dom<Sel = S>>(
        &self,
        sess: &mut Session,
        docs: &[(u64, &D)],
    ) -> Result<()> {
        let with_meta: Vec<(u64, &D, Option<&Value>)> =
            docs.iter().map(|(id, dom)| (*id, *dom, None)).collect();
        self.resume_dom_with_meta(sess, &with_meta)
    }

    /// Feed fetched documents with optional per-response metadata (`headers`, `status`, `base_url`) back in.
    pub(crate) fn resume_dom_with_meta<D: Dom<Sel = S>>(
        &self,
        sess: &mut Session,
        docs: &[(u64, &D, Option<&Value>)],
    ) -> Result<()> {
        let overrides = self.compile_overrides::<D>(&sess.overrides)?;
        for (id, dom, extra_meta) in docs {
            let req = sess.outstanding.shift_remove(id).ok_or_else(|| {
                Error::Session(format!("unknown or already resolved request id {id}"))
            })?;
            let which = match &req.kind {
                ReqKind::Follow { sub } => DocKind::Sub(*sub),
                ReqKind::PaginatePage {
                    pager,
                    acc,
                    pages,
                    seen,
                    path,
                } => DocKind::Paginate {
                    pager: *pager,
                    acc_before: acc.clone(),
                    pages_before: *pages,
                    seen_before: seen.clone(),
                    path: path.clone(),
                },
                ReqKind::ForeignStart { .. } | ReqKind::ForeignStep { .. } => {
                    return Err(Error::Session(format!(
                        "internal error: foreign request {id} routed to local DOM resume"
                    )));
                }
            };
            let eff_meta = merge_response_meta(&req.meta, *extra_meta);
            let out = self.run(&which, *dom, &eff_meta, &mut sess.next_id, &overrides)?;
            let rec = DocRec {
                which,
                meta: eff_meta,
                origin: req.origin,
                key: req_key(&req.kind, &req.params, &req.meta),
                url: req.url().map(str::to_string),
                source: None,
                errors: Vec::new(),
                misses: Vec::new(),
            };
            sess.docs.insert(*id, rec);
            sess.install_run(*id, out.value, out.reqs, out.errors, out.misses);
        }
        Ok(())
    }

    pub(crate) fn resume_foreign(
        &self,
        sess: &mut Session,
        docs: &[(u64, String, Option<Value>)],
    ) -> Result<()> {
        for (id, body, extra_meta) in docs {
            let req = sess.outstanding.shift_remove(id).ok_or_else(|| {
                Error::Session(format!("unknown or already resolved request id {id}"))
            })?;
            match req.kind {
                ReqKind::ForeignStart { foreign } => {
                    let spec = &self.foreign_specs[foreign];
                    let eff_meta = merge_response_meta(&req.meta, extra_meta.as_ref());
                    let mut sub_sess = spec.engine.start(body, eff_meta.clone())?;
                    let rec = DocRec {
                        which: DocKind::Foreign,
                        meta: eff_meta.clone(),
                        origin: req.origin,
                        key: req_key(&req.kind, &req.params, &req.meta),
                        url: req.url().map(str::to_string),
                        source: None,
                        errors: Vec::new(),
                        misses: Vec::new(),
                    };
                    sess.docs.insert(*id, rec);
                    if sub_sess.is_done() {
                        let errors = sub_sess.errors().to_vec();
                        let misses = sub_sess.misses().to_vec();
                        let val = sub_sess.take_result()?;
                        let final_val = if spec.post.is_empty() {
                            val
                        } else {
                            let mut jc = None;
                            let mut tail_plan = Plan::empty();
                            tail_plan.html_subs = self.html_subs.clone();
                            tail_plan.run_tail::<ValueDom>(
                                &ValueDom,
                                &spec.post,
                                val,
                                &eff_meta,
                                None,
                                Vec::new(),
                                None,
                                &mut jc,
                            )?
                        };
                        sess.install_run(*id, final_val, Vec::new(), errors, misses);
                    } else {
                        let mut pending_sub_ids = HashSet::new();
                        for sub_r in sub_sess.pending() {
                            pending_sub_ids.insert(sub_r.id);
                            let pid = sess.next_id;
                            sess.next_id += 1;
                            sess.outstanding.insert(
                                pid,
                                Request {
                                    id: pid,
                                    params: sub_r.params.clone(),
                                    meta: sub_r.meta.clone(),
                                    kind: ReqKind::ForeignStep {
                                        root_id: *id,
                                        sub_req_id: sub_r.id,
                                    },
                                    origin: *id,
                                },
                            );
                        }
                        sess.foreign.insert(
                            *id,
                            ForeignRun {
                                foreign,
                                sess: sub_sess,
                                meta: eff_meta,
                                pending_sub_ids,
                            },
                        );
                    }
                }
                ReqKind::ForeignStep { root_id, sub_req_id } => {
                    let mut fr = sess.foreign.remove(&root_id).ok_or_else(|| {
                        Error::Session(format!("missing foreign session for request {root_id}"))
                    })?;
                    let spec = &self.foreign_specs[fr.foreign];
                    spec.engine
                        .resume_with_meta(&mut fr.sess, &[(sub_req_id, body.clone(), extra_meta.clone())])?;
                    for sub_r in fr.sess.pending() {
                        if fr.pending_sub_ids.insert(sub_r.id) {
                            let pid = sess.next_id;
                            sess.next_id += 1;
                            sess.outstanding.insert(
                                pid,
                                Request {
                                    id: pid,
                                    params: sub_r.params.clone(),
                                    meta: sub_r.meta.clone(),
                                    kind: ReqKind::ForeignStep {
                                        root_id,
                                        sub_req_id: sub_r.id,
                                    },
                                    origin: root_id,
                                },
                            );
                        }
                    }
                    if fr.sess.is_done() {
                        let errors = fr.sess.errors().to_vec();
                        let misses = fr.sess.misses().to_vec();
                        let val = fr.sess.take_result()?;
                        let final_val = if spec.post.is_empty() {
                            val
                        } else {
                            let mut jc = None;
                            let mut tail_plan = Plan::empty();
                            tail_plan.html_subs = self.html_subs.clone();
                            tail_plan.run_tail::<ValueDom>(
                                &ValueDom,
                                &spec.post,
                                val,
                                &fr.meta,
                                None,
                                Vec::new(),
                                None,
                                &mut jc,
                            )?
                        };
                        sess.install_run(root_id, final_val, Vec::new(), errors, misses);
                    } else {
                        sess.foreign.insert(root_id, fr);
                    }
                }
                _ => {
                    return Err(Error::Session(format!(
                        "internal error: non-foreign request {id} routed to foreign resume"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Apply repair answers: `(repair id, {key: css})`; an empty map declines. Fresh references are learned for the
    /// repaired keys. Returns the ids of the documents that have to be re-run (their sources are in the session).
    pub(crate) fn begin_repairs(&self, sess: &mut Session, answers: &[(u64, IndexMap<String, String>)]) -> Result<Vec<u64>> {
        let mut docs: Vec<u64> = Vec::new();
        for (rid, mapping) in answers {
            let rep = sess
                .repairs
                .shift_remove(rid)
                .ok_or_else(|| Error::Session(format!("unknown or already answered repair id {rid}")))?;
            let doc = rep.doc.unwrap_or(ROOT);
            if mapping.is_empty() {
                sess.declined.insert(format!("{doc}|{}", rep.key));
                continue;
            }
            for (k, css) in mapping {
                if css.trim().is_empty() {
                    return Err(Error::Session(format!("repair for {k:?}: empty selector")));
                }
                sess.overrides.insert(k.clone(), css.trim().to_string());
                // the relocated ("healed") reference belongs to the old selector; the baseline stays until the new
                // selector's element replaces it (if it fails again, the next repair still sees what the element was)
                self.store.remove(&adaptive::healed_key(k));
                // every document that is waiting for this key is re-run
                for (id, d) in &sess.docs {
                    if d.misses.iter().any(|m| m.key == *k) && !docs.contains(id) {
                        docs.push(*id);
                    }
                }
            }
            if !docs.contains(&doc) && mapping.keys().any(|k| *k == rep.key) {
                docs.push(doc);
            }
        }
        Ok(docs)
    }

    /// Re-run document `doc` (already parsed as `dom`) with the session's current overrides.
    pub(crate) fn rerun_dom<D: Dom<Sel = S>>(&self, sess: &mut Session, doc: u64, dom: &D) -> Result<()> {
        let (which, meta) = match sess.docs.get(&doc) {
            Some(r) => (r.which.clone(), r.meta.clone()),
            None => return Err(Error::Session(format!("unknown document {doc}"))),
        };
        let overrides = self.compile_overrides::<D>(&sess.overrides)?;
        let out = self.run(&which, dom, &meta, &mut sess.next_id, &overrides)?;
        sess.install_run(doc, out.value, out.reqs, out.errors, out.misses);
        Ok(())
    }

    /// Engine-independent part of `Engine::repair`: validate + record the answers, re-run the affected documents with
    /// `reparse(source, doc)`; on an error the session's repair state is restored.
    pub(crate) fn repair_with(
        &self,
        sess: &mut Session,
        answers: &[(u64, IndexMap<String, String>)],
        validate: &dyn Fn(&str) -> std::result::Result<(), String>,
        rerun: &mut dyn FnMut(&Plan<S>, &mut Session, u64, &str) -> Result<()>,
    ) -> Result<()> {
        // A model's selector that does not parse must not fail the extraction: the request stays open with the parser's
        // message (`previous_error`), and the second invalid answer declines it.
        let mut ids = HashSet::new();
        for (id, _) in answers {
            if !sess.repairs.contains_key(id) || !ids.insert(*id) {
                return Err(Error::Session(format!("unknown or already answered repair id {id}")));
            }
        }
        let snapshot = sess.checkpoint();
        let mut valid: Vec<(u64, IndexMap<String, String>)> = Vec::new();
        for (rid, mapping) in answers {
            let bad = mapping.iter().find_map(|(k, css)| validate(css.trim()).err().map(|m| (k.clone(), css.clone(), m)));
            match bad {
                None => valid.push((*rid, mapping.clone())),
                Some((k, css, msg)) => {
                    let rep = sess
                        .repairs
                        .get_mut(rid)
                        .ok_or_else(|| Error::Session(format!("unknown or already answered repair id {rid}")))?;
                    rep.invalid_answers += 1;
                    rep.previous_error = Some(format!("selector {css:?} for {k:?}: {msg}"));
                    if rep.invalid_answers >= 2 {
                        let doc = rep.doc.unwrap_or(ROOT);
                        let key = rep.key.clone();
                        sess.repairs.shift_remove(rid);
                        sess.declined.insert(format!("{doc}|{key}"));
                    }
                }
            }
        }
        let answers = &valid[..];
        let result = (|| -> Result<()> {
            let docs = self.begin_repairs(sess, answers)?;
            for d in docs {
                let src = sess.source_of(d)?;
                rerun(self, sess, d, &src)?;
            }
            Ok(())
        })();
        if result.is_err() {
            sess.restore(snapshot);
        }
        result
    }

    fn run<D: Dom<Sel = S>>(
        &self,
        which: &DocKind,
        dom: &D,
        meta: &Value,
        next_id: &mut u64,
        overrides: &HashMap<String, (String, S)>,
    ) -> Result<RunOut> {
        let mut ev = Eval {
            plan: self,
            dom,
            meta,
            overrides,
            root: dom.root(),
            path: Vec::new(),
            reqs: Vec::new(),
            next_id: *next_id,
            pending: Vec::new(),
            errors: Vec::new(),
            misses: Vec::new(),
            prefetch: None,
            json_cache: None,
            json_nodes: HashMap::new(),
            js_nodes: HashMap::new(),
            base_href: None,
            groups_mode: false,
            group_cur: None,
        };
        let start = Val::Node(dom.root());
        let v = match which {
            DocKind::Root => ev.eval(&self.root, &start, None)?,
            DocKind::Sub(i) => ev.eval(&self.subs[*i], &start, None)?,
            DocKind::Foreign => {
                return Err(Error::Session("internal error: cannot re-run foreign follow document locally".into()));
            }
            DocKind::Paginate {
                pager,
                acc_before,
                pages_before,
                seen_before,
                path,
            } => {
                ev.path = path.clone();
                ev.step_paginate(
                    *pager,
                    acc_before.clone(),
                    *pages_before,
                    seen_before.clone(),
                    &start,
                    None,
                )?
            }
        };
        let v = ev.finalize(v);
        // the extraction succeeded: now it is safe to remember what the adaptive selectors learned
        for p in std::mem::take(&mut ev.pending) {
            if p.heal || !self.store.contains(&p.key) {
                self.store.put(&p.key, &p.fp);
            }
        }
        *next_id = ev.next_id;
        Ok(RunOut {
            value: v,
            reqs: ev.reqs,
            errors: ev.errors,
            misses: ev.misses,
        })
    }
}

/// Cached parsed JSON string + AST for repeated `parse_json` / `json_path` lookups within one document.
#[cfg_attr(not(feature = "engine-stream"), allow(dead_code))]
pub(crate) type JsonCache = Option<(String, Rc<serde_json::Value>)>;

impl<S: Send + Sync> Plan<S> {
    /// Run value-only operations on an already computed value (used by the stream engine).
    pub(crate) fn run_tail<D: Dom<Sel = S>>(
        &self,
        dom: &D,
        ops: &[COp],
        v: Value,
        meta: &Value,
        idx: Option<usize>,
        path: Vec<String>,
        base_href: Option<&str>,
        json_cache: &mut JsonCache,
    ) -> Result<Value> {
        let no_overrides: HashMap<String, (String, S)> = HashMap::new();
        let mut ev = Eval {
            plan: self,
            dom,
            meta,
            overrides: &no_overrides,
            root: dom.root(),
            path,
            reqs: Vec::new(),
            next_id: 0,
            pending: Vec::new(),
            errors: Vec::new(),
            misses: Vec::new(),
            prefetch: None,
            json_cache: json_cache.take(),
            json_nodes: HashMap::new(),
            js_nodes: HashMap::new(),
            // a stream run has seen the document's `<base>` while it went by
            base_href: Some(base_href.map(str::to_string)),
            groups_mode: false,
            group_cur: None,
        };
        let ctx: Val<Nd<D>> = Val::V(Value::Null);
        let res = ev.run_ops(ops, Val::V(v), &ctx, idx);
        *json_cache = ev.json_cache.take();
        Ok(ev.finalize(res?))
    }
}

fn section_value(header: String, items: Vec<Value>) -> Value {
    let mut m = IndexMap::new();
    m.insert("header".to_string(), Value::Str(header));
    m.insert("items".to_string(), Value::List(items));
    Value::Map(m)
}

fn num_value(n: f64) -> Value {
    if n == 0.0 && n.is_sign_negative() {
        Value::Float(n)
    } else if n.is_finite() && n.fract() == 0.0 && n.abs() < 9e15 {
        Value::Int(n as i64)
    } else {
        Value::Float(n)
    }
}

/// Intermediate value. Nodes are kept as nodes until something needs a scalar.
enum Val<N> {
    Node(N),
    Nodes(Vec<N>),
    V(Value),
}

impl<N: Clone> Clone for Val<N> {
    fn clone(&self) -> Self {
        match self {
            Val::Node(n) => Val::Node(n.clone()),
            Val::Nodes(n) => Val::Nodes(n.clone()),
            Val::V(v) => Val::V(v.clone()),
        }
    }
}

fn kind<N>(v: &Val<N>) -> &'static str {
    match v {
        Val::Node(_) => "node",
        Val::Nodes(_) => "node list",
        Val::V(Value::Null) => "null",
        Val::V(Value::Bool(_)) => "bool",
        Val::V(Value::Int(_)) => "int",
        Val::V(Value::Float(_)) => "float",
        Val::V(Value::Str(_)) => "string",
        Val::V(Value::List(_)) => "list",
        Val::V(Value::Map(_)) => "map",
        Val::V(Value::Pending(_)) => "pending follow",
    }
}

fn null<N>() -> Val<N> {
    Val::V(Value::Null)
}

fn loose_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Float(y)) | (Value::Float(y), Value::Int(x)) => (*x as f64) == *y,
        _ => a == b,
    }
}

/// Find the first number in `s` and return it normalised (`-?digits[.digits]`).
///
/// Understands `1,299.50`, `1.299,50`, `19,99`, `12 990` (space / nbsp / narrow nbsp as thousands
/// separator, exactly three digits per group), `-.5`, and does not take the hyphen of `Model-3` for a sign.
/// Ambiguity rule: a single `,` followed by exactly three digits is a thousands separator (`1,299`) unless the
/// integer part is `0` or longer than three digits; a single `.` is always a decimal point.
pub(crate) fn normalize_number(s: &str) -> Option<String> {
    let c: Vec<char> = s.chars().collect();
    let n = c.len();
    let at = |k: usize| c.get(k).copied();
    let dig = |ch: Option<char>| matches!(ch, Some('0'..='9'));
    let mut i = 0;
    while i < n {
        let ch = c[i];
        let prev_alnum = i > 0 && c[i - 1].is_alphanumeric();
        if ch.is_ascii_digit() {
            break;
        }
        if ch == '-'
            && !prev_alnum
            && (dig(at(i + 1)) || (at(i + 1) == Some('.') && dig(at(i + 2))))
        {
            break;
        }
        if ch == '.' && dig(at(i + 1)) {
            break;
        }
        i += 1;
    }
    if i >= n {
        return None;
    }
    let neg = c[i] == '-';
    if neg {
        i += 1;
    }
    let mut raw = String::new();
    let mut group = 0usize;
    while i < n {
        let ch = c[i];
        if ch.is_ascii_digit() {
            raw.push(ch);
            group += 1;
            i += 1;
        } else if (ch == '.' || ch == ',') && dig(at(i + 1)) {
            raw.push(ch);
            group = 0;
            i += 1;
        } else if matches!(ch, ' ' | '\u{a0}' | '\u{202f}')
            && (1..=3).contains(&group)
            && !raw.contains('.')
            && !raw.contains(',')
            && dig(at(i + 1))
            && dig(at(i + 2))
            && dig(at(i + 3))
            && !dig(at(i + 4))
        {
            i += 1;
            group = 0;
        } else {
            break;
        }
    }
    let dots = raw.matches('.').count();
    let commas = raw.matches(',').count();
    let norm = if dots > 0 && commas > 0 {
        let (dec, thou) = if raw.rfind(',') > raw.rfind('.') {
            (',', '.')
        } else {
            ('.', ',')
        };
        let t: String = raw.chars().filter(|x| *x != thou).collect();
        if t.matches(dec).count() > 1 {
            return None;
        }
        t.replace(dec, ".")
    } else if commas > 1 {
        raw.replace(',', "")
    } else if commas == 1 {
        let (before, after) = raw.split_once(',').unwrap_or((&raw, ""));
        if after.len() == 3 && before.len() <= 3 && before.trim_start_matches('0') != "" {
            format!("{before}{after}")
        } else {
            format!("{before}.{after}")
        }
    } else if dots > 1 {
        raw.replace('.', "")
    } else {
        raw
    };
    Some(if neg { format!("-{norm}") } else { norm })
}

/// Lenient number extraction (see [`normalize_number`]): `"$1,299.50"` -> 1299.5, `"19,99 €"` -> 19.99.
pub(crate) fn parse_number(s: &str) -> Option<f64> {
    // fast path without allocations: plain `-12.5` / `$19.99` (the overwhelmingly common case);
    // anything this does not parse exactly the same way falls through to the full normaliser
    let t = s.trim().trim_start_matches('$').trim_start();
    if !t.is_empty()
        && t.as_bytes()
            .iter()
            .all(|&b| b.is_ascii_digit() || b == b'.' || b == b'-')
    {
        if let Ok(f) = t.parse::<f64>() {
            return Some(f);
        }
    }
    normalize_number(s)?.parse().ok()
}

/// Same, for integers: exact for the whole `i64` range (no detour through `f64`).
pub(crate) fn parse_int(s: &str) -> std::result::Result<i64, String> {
    let t = s.trim();
    if !t.is_empty()
        && t.as_bytes()
            .iter()
            .all(|&b| b.is_ascii_digit() || b == b'-')
    {
        if let Ok(i) = t.parse::<i64>() {
            return Ok(i);
        }
    }
    let norm = normalize_number(s).ok_or_else(|| format!("no number in {s:?}"))?;
    if let Ok(i) = norm.parse::<i64>() {
        return Ok(i);
    }
    match norm.parse::<f64>() {
        Ok(f) if f.fract() == 0.0 && f.abs() < 9.2e18 => Ok(f as i64),
        Ok(f) => Err(format!("{f} is not an integer (from {s:?})")),
        Err(_) => Err(format!("no number in {s:?}")),
    }
}

/// A fingerprint to remember, held back until the pipeline that produced it succeeded ("value guard"):
/// a captcha page or a "sold out" block must not become the reference just because a selector matched it.
struct PendingFp {
    key: String,
    fp: adaptive::Fingerprint,
    heal: bool,
}

/// Everything one run of a (sub-)schema produces.
struct RunOut {
    value: Value,
    reqs: Vec<Request>,
    errors: Vec<ErrorInfo>,
    misses: Vec<AdaptiveMiss>,
}

struct Eval<'a, D: Dom> {
    plan: &'a Plan<D::Sel>,
    dom: &'a D,
    meta: &'a Value,
    /// selectors given by repairs (`key -> (css, compiled)`): they replace the compiled selector of an `adaptive` key
    overrides: &'a HashMap<String, (String, D::Sel)>,
    root: D::Node,
    path: Vec<String>,
    reqs: Vec<Request>,
    next_id: u64,
    pending: Vec<PendingFp>,
    /// quarantined records (`map` with `on_error`)
    errors: Vec<ErrorInfo>,
    /// adaptive selectors without a result
    misses: Vec<AdaptiveMiss>,
    /// selector results of the record being evaluated, computed in one walk of its subtree
    prefetch: Option<Prefetch<D::Node>>,
    /// the last large JSON text parsed by `parse_json` / `json_path` and its parsed form
    json_cache: Option<(String, Rc<serde_json::Value>)>,
    /// parsed JSON per `node_key` (an `Eval` lives for one document, so keys never clash): a second field reading the
    /// same `<script id="__NEXT_DATA__">` neither copies its text nor hashes it
    json_nodes: HashMap<usize, Rc<serde_json::Value>>,
    /// `js_json(var)` results per node (and `var`): several fields reading branches of one
    /// `window.__INITIAL_STATE__` script parse the JavaScript object once
    js_nodes: HashMap<usize, Vec<(Option<String>, Rc<serde_json::Value>)>>,
    /// the document's `<base href>`: `None` = not looked up yet, `Some(None)` = there is none
    base_href: Option<Option<String>>,
    /// `group_by(..).map(..)`: the body's context is a group of sibling nodes (see `select_into`)
    groups_mode: bool,
    /// node keys of the group being mapped: selectors over exactly this group also match the group's own nodes
    group_cur: Option<Vec<usize>>,
}

/// Results of the leading selectors of a record's fields, found by one `Dom::select_multi` walk.
struct Prefetch<N> {
    /// `node_key` of the record element these results belong to
    key: usize,
    /// selector indices
    sels: Vec<usize>,
    /// `true`: all matches were collected; `false`: only the first one (the selector is used with `first` only)
    all: Vec<bool>,
    found: Vec<Vec<N>>,
}

/// Distil HTML text (engines that cannot distil a node directly): serialise, re-parse with `lite`, distil.
#[allow(unused_variables)]
fn distill_html(html: &str, keep: usize, max_text: usize, max_chars: usize) -> Option<String> {
    #[cfg(feature = "engine-lite")]
    {
        let opts = crate::markup::distill::DistillOptions { keep, max_text, max_chars };
        return Some(crate::markup::distill::distill(html, &opts));
    }
    #[cfg(not(feature = "engine-lite"))]
    {
        None
    }
}

/// `[start:stop]` over `len` items, Python rules: negative counts from the end, out-of-range is clamped, `lo <= hi`.
fn slice_bounds(len: usize, start: Option<i64>, stop: Option<i64>) -> (usize, usize) {
    let norm = |v: i64| -> usize {
        if v < 0 {
            (len as i64 + v).max(0) as usize
        } else {
            (v as usize).min(len)
        }
    };
    let lo = start.map_or(0, norm);
    let hi = stop.map_or(len, norm);
    (lo.min(hi), hi)
}

/// Add one `<meta>` to the `meta_tags` map. `mode`: 0 = auto (a repeated name becomes a list), 1 = first wins,
/// 2 = always a list. One hash lookup (entry API).
pub(crate) fn meta_insert(m: &mut IndexMap<String, Value>, key: String, content: String, mode: u8) {
    use indexmap::map::Entry;
    match m.entry(key) {
        Entry::Vacant(v) => {
            v.insert(if mode == 2 { Value::List(vec![Value::Str(content)]) } else { Value::Str(content) });
        }
        Entry::Occupied(mut o) => {
            if mode == 1 {
                return;
            }
            match o.get_mut() {
                Value::List(l) => l.push(Value::Str(content)),
                first => {
                    let prev = std::mem::replace(first, Value::Null);
                    *first = Value::List(vec![prev, Value::Str(content)]);
                }
            }
        }
    }
}

/// JSON-LD blocks hold an object, an array of objects, or an object with `@graph`: all become a flat list of objects.
fn flatten_ld(j: serde_json::Value, out: &mut Vec<serde_json::Value>) {
    match j {
        serde_json::Value::Array(a) => {
            for x in a {
                flatten_ld(x, out);
            }
        }
        serde_json::Value::Object(mut o) => {
            if let Some(serde_json::Value::Array(g)) = o.remove("@graph") {
                // the wrapper itself carries nothing but `@context` (and sometimes an id): keep it only if it does
                let rest_is_context = o.keys().all(|k| k == "@context" || k == "@id");
                for x in g {
                    flatten_ld(x, out);
                }
                if !rest_is_context {
                    out.push(serde_json::Value::Object(o));
                }
            } else {
                out.push(serde_json::Value::Object(o));
            }
        }
        _ => {}
    }
}

/// `@type` is a string or a list; `https://schema.org/Product` counts as `Product`.
fn ld_has_type(o: &serde_json::Value, want: &str) -> bool {
    let is = |t: &str| t == want || t.rsplit(|c| c == '/' || c == '#').next() == Some(want);
    match o.get("@type") {
        Some(serde_json::Value::String(t)) => is(t),
        Some(serde_json::Value::Array(a)) => a.iter().any(|t| t.as_str().map_or(false, is)),
        _ => false,
    }
}

/// Scalars (or the scalars of a list) of a value, as strings.
fn sample_strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::List(l) => {
            for x in l.iter().take(3) {
                sample_strings(x, out);
            }
        }
        // a record (`adaptive | map({...})`): up to three of its scalars (a badge like "Sale" alone says little)
        Value::Map(m) => {
            let before = out.len();
            for v in m.values() {
                if out.len() - before >= 3 {
                    break;
                }
                if !matches!(v, Value::Map(_) | Value::List(_) | Value::Null) {
                    if let Some(s) = v.scalar_string() {
                        out.push(s);
                    }
                }
            }
        }
        Value::Null => {}
        other => {
            if let Some(s) = other.scalar_string() {
                out.push(s);
            }
        }
    }
}

/// Join the pieces of `text(sep=..., strip=..., br=...)`: `sep` goes between text pieces, a `<br>` (`None`) is
/// written as `br` and stands in for the separator on both of its sides.
pub(crate) fn join_pieces(pieces: &[Option<String>], sep: &str, strip: bool, br: Option<&str>) -> String {
    let mut out = String::new();
    let mut prev_text = false;
    for p in pieces {
        match p {
            Some(t) => {
                let t = if strip { t.trim() } else { t.as_str() };
                if strip && t.is_empty() {
                    continue;
                }
                if prev_text {
                    out.push_str(sep);
                }
                out.push_str(t);
                prev_text = true;
            }
            None => {
                if let Some(b) = br {
                    out.push_str(b);
                    prev_text = false;
                }
            }
        }
    }
    out
}

fn json_kind(j: &serde_json::Value) -> &'static str {
    match j {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(n) => {
            if n.as_i64().is_some() {
                "int"
            } else {
                "float"
            }
        }
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "list",
        serde_json::Value::Object(_) => "map",
    }
}

fn expr_uses_ctx(e: &CExpr) -> bool {
    match e {
        CExpr::Lit(_) => false,
        CExpr::Object(fs) => fs.iter().any(|(_, x)| expr_uses_ctx(x)),
        CExpr::List(xs) => xs.iter().any(expr_uses_ctx),
        CExpr::Pipe(ops) => {
            let starts_ctx = !matches!(ops.first(), Some(COp::Root | COp::Meta(_) | COp::Index));
            starts_ctx || ops_use_ctx(ops)
        }
    }
}

fn ops_use_ctx(ops: &[COp]) -> bool {
    ops.iter().any(|op| match op {
        COp::Default(e)
        | COp::Eq(e)
        | COp::Ne(e)
        | COp::Contains(e)
        | COp::StartsWith(e)
        | COp::EndsWith(e)
        | COp::And(e)
        | COp::Or(e)
        | COp::Run(e)
        | COp::Unique { by: Some(e), .. } => expr_uses_ctx(e),
        COp::ResolveUrl(Some(e)) => expr_uses_ctx(e),
        COp::Coalesce(es) => es.iter().any(expr_uses_ctx),
        COp::If(c, t, o) => expr_uses_ctx(c) || expr_uses_ctx(t) || expr_uses_ctx(o),
        COp::Follow { request, meta, on_null, .. }
        | COp::FollowForeign { request, meta, on_null, .. } => {
            expr_uses_ctx(request) || expr_uses_ctx(meta) || ops_use_ctx(on_null)
        }
        COp::Paginate { .. } => true,
        _ => false,
    })
}

/// Fuse the leading selectors of a record's fields into one walk only from this many distinct selectors: with one
/// to three, a plain `select` per field (which stops at the first match) is cheaper. Heuristic, see `CHANGELOG.md`.
const FUSE_MIN_HEADS: usize = 4;

/// Texts shorter than this are parsed again every time (cheaper than keeping a copy around).
const JSON_CACHE_MIN: usize = 512;

type Nd<D> = <D as Dom>::Node;

impl<'a, D: Dom> Eval<'a, D> {
    fn err<T>(&self, op: &str, msg: impl Into<String>) -> Result<T> {
        Err(Error::Exec {
            path: fmt_path(&self.path),
            msg: format!("{op}: {}", msg.into()),
        })
    }

    fn finalize(&self, v: Val<Nd<D>>) -> Value {
        match v {
            Val::Node(n) => self.dom.to_value(&n),
            Val::Nodes(ns) => Value::List(ns.iter().map(|n| self.dom.to_value(n)).collect()),
            Val::V(v) => v,
        }
    }

    fn eval(&mut self, e: &CExpr, ctx: &Val<Nd<D>>, idx: Option<usize>) -> Result<Val<Nd<D>>> {
        match e {
            CExpr::Lit(v) => Ok(Val::V(v.clone())),
            CExpr::Object(fs) => {
                let mut m = IndexMap::with_capacity(fs.len());
                for (k, fe) in fs {
                    self.path.push(k.clone());
                    let v = self.eval(fe, ctx, idx)?;
                    m.insert(k.clone(), self.finalize(v));
                    self.path.pop();
                }
                Ok(Val::V(Value::Map(m)))
            }
            CExpr::List(es) => {
                let mut l = Vec::with_capacity(es.len());
                for (i, fe) in es.iter().enumerate() {
                    self.path.push(format!("[{i}]"));
                    let v = self.eval(fe, ctx, idx)?;
                    l.push(self.finalize(v));
                    self.path.pop();
                }
                Ok(Val::V(Value::List(l)))
            }
            CExpr::Pipe(ops) => {
                let mark = self.pending.len();
                let mut cur = ctx.clone();
                let mut start = 0usize;
                if let Some(first) = ops.first() {
                    if let Some(v) = self.prefetched(first, ctx) {
                        cur = v;
                        start = 1;
                    }
                }
                cur = self.run_ops(&ops[start..], cur, ctx, idx)?;
                if self.is_missing(&cur) {
                    // the pipeline produced nothing: do not let it teach the adaptive store anything
                    self.pending.truncate(mark);
                } else {
                    self.record_samples(ops, &cur);
                }
                Ok(cur)
            }
        }
    }

    fn run_ops(
        &mut self,
        ops: &[COp],
        mut cur: Val<Nd<D>>,
        ctx: &Val<Nd<D>>,
        idx: Option<usize>,
    ) -> Result<Val<Nd<D>>> {
        let mark = self.pending.len();
        let mut i = 0usize;
        while i < ops.len() {
            if matches!(ops[i], COp::ParseJson | COp::JsonPath(_)) {
                if let Some(map_op @ (COp::Map(_) | COp::MapSafe(..))) = ops.get(i + 1) {
                    let (body, mode) = match map_op {
                        COp::Map(b) => (b, OnError::Raise),
                        COp::MapSafe(b, m) => (b, *m),
                        _ => unreachable!(),
                    };
                    cur = match self.json_source(&cur)? {
                        None => null(),
                        Some(doc) => {
                            let target = match &ops[i] {
                                COp::ParseJson => Some(&*doc),
                                COp::JsonPath(segs) => self.nav_json(&doc, segs)?,
                                _ => unreachable!(),
                            };
                            match target {
                                None => null(),
                                Some(n) if n.is_null() => null(),
                                Some(serde_json::Value::Array(arr)) => {
                                    Val::V(Value::List(self.map_json_items(body, arr, mode)?))
                                }
                                Some(other) => {
                                    return self.err("map", format!("expected a collection, got {}", json_kind(other)))
                                }
                            }
                        }
                    };
                    i += 2;
                    continue;
                }
            }
            let op = &ops[i];
            if matches!(op, COp::Default(_)) && self.is_missing(&cur) {
                // the value was missing *before* `default` put a fallback in its place: whatever this
                // pipeline found for the adaptive store (a captcha block, an empty <span>) is no reference
                self.pending.truncate(mark);
            }
            cur = self.apply(op, cur, ctx, idx)?;
            i += 1;
        }
        Ok(cur)
    }

    fn nav_json<'j>(
        &self,
        mut node: &'j serde_json::Value,
        segs: &[JsonSeg],
    ) -> Result<Option<&'j serde_json::Value>> {
        for seg in segs {
            if node.is_null() {
                return Ok(Some(node));
            }
            match seg {
                JsonSeg::Key(k) => match node {
                    serde_json::Value::Object(m) => match m.get(k.as_str()) {
                        Some(c) => node = c,
                        None => return Ok(None),
                    },
                    other => return self.err("key", format!("expected a node, got {}", json_kind(other))),
                },
                JsonSeg::At(i) => match node {
                    serde_json::Value::Array(l) => match l.get(*i) {
                        Some(c) => node = c,
                        None => return Ok(None),
                    },
                    other => return self.err("at", format!("cannot index a {}", json_kind(other))),
                },
            }
        }
        Ok(Some(node))
    }

    fn map_json_items(
        &mut self,
        body: &CExpr,
        items: &[serde_json::Value],
        mode: OnError,
    ) -> Result<Vec<Value>> {
        let mut out = Vec::with_capacity(items.len());
        for (i, it) in items.iter().enumerate() {
            let (plen, pmark, rmark) = (self.path.len(), self.pending.len(), self.reqs.len());
            self.path.push(format!("[{i}]"));
            match self.eval_json_item(body, it, Some(i)) {
                Ok(r) => {
                    out.push(r);
                    self.path.pop();
                }
                Err(Error::Exec { path, msg }) if mode != OnError::Raise => {
                    self.errors.push(ErrorInfo { path, message: msg });
                    self.path.truncate(plen);
                    self.pending.truncate(pmark);
                    self.reqs.truncate(rmark);
                    if mode == OnError::Null {
                        out.push(Value::Null);
                    }
                }
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }

    fn eval_json_item(
        &mut self,
        e: &CExpr,
        item: &serde_json::Value,
        idx: Option<usize>,
    ) -> Result<Value> {
        match e {
            CExpr::Lit(v) => Ok(v.clone()),
            CExpr::Object(fs) => {
                let mut m = IndexMap::with_capacity(fs.len());
                for (k, fe) in fs {
                    self.path.push(k.clone());
                    let v = self.eval_json_item(fe, item, idx)?;
                    m.insert(k.clone(), v);
                    self.path.pop();
                }
                Ok(Value::Map(m))
            }
            CExpr::List(es) => {
                let mut l = Vec::with_capacity(es.len());
                for (i, fe) in es.iter().enumerate() {
                    self.path.push(format!("[{i}]"));
                    let v = self.eval_json_item(fe, item, idx)?;
                    l.push(v);
                    self.path.pop();
                }
                Ok(Value::List(l))
            }
            CExpr::Pipe(ops) => {
                let mut nav_len = 0usize;
                while nav_len < ops.len() && matches!(ops[nav_len], COp::Key(_) | COp::At(_)) {
                    nav_len += 1;
                }
                if (nav_len > 0 || matches!(ops.first(), Some(COp::Map(_) | COp::MapSafe(..))))
                    && !ops_use_ctx(&ops[nav_len..])
                {
                    let mut node_opt: Option<&serde_json::Value> = Some(item);
                    for op in &ops[..nav_len] {
                        let node = match node_opt {
                            Some(n) if !n.is_null() => n,
                            _ => {
                                node_opt = None;
                                break;
                            }
                        };
                        match op {
                            COp::Key(k) => match node {
                                serde_json::Value::Object(m) => node_opt = m.get(k.as_str()),
                                other => {
                                    return self.err("key", format!("expected a node, got {}", json_kind(other)))
                                }
                            },
                            COp::At(i) => match node {
                                serde_json::Value::Array(l) => node_opt = l.get(*i),
                                other => {
                                    return self.err("at", format!("cannot index a {}", json_kind(other)))
                                }
                            },
                            _ => unreachable!(),
                        }
                    }
                    if let Some(map_op @ (COp::Map(_) | COp::MapSafe(..))) = ops.get(nav_len) {
                        let mapped = match node_opt {
                            None => null(),
                            Some(n) if n.is_null() => null(),
                            Some(serde_json::Value::Array(arr)) => {
                                let (sub_body, sub_mode) = match map_op {
                                    COp::Map(b) => (b, OnError::Raise),
                                    COp::MapSafe(b, m) => (b, *m),
                                    _ => unreachable!(),
                                };
                                Val::V(Value::List(self.map_json_items(sub_body, arr, sub_mode)?))
                            }
                            Some(other) => {
                                return self.err("map", format!("expected a collection, got {}", json_kind(other)))
                            }
                        };
                        let res = self.run_ops(&ops[nav_len + 1..], mapped, &Val::V(Value::Null), idx)?;
                        return Ok(self.finalize(res));
                    }
                    let leaf_val = match node_opt {
                        Some(n) => Value::from_json_ref(n),
                        None => Value::Null,
                    };
                    let res = self.run_ops(&ops[nav_len..], Val::V(leaf_val), &Val::V(Value::Null), idx)?;
                    return Ok(self.finalize(res));
                }
                let it = Val::V(Value::from_json_ref(item));
                let v = self.eval(e, &it, idx)?;
                Ok(self.finalize(v))
            }
        }
    }

    // ---- helpers -------------------------------------------------------

    fn is_truthy(&self, v: &Val<Nd<D>>) -> bool {
        match v {
            Val::Node(n) => self.dom.node_truthy(n),
            Val::Nodes(n) => !n.is_empty(),
            Val::V(v) => v.truthy(),
        }
    }

    fn is_missing(&self, v: &Val<Nd<D>>) -> bool {
        match v {
            Val::Node(n) => self.dom.node_is_null(n),
            Val::Nodes(n) => n.is_empty(),
            Val::V(Value::Null) => true,
            _ => false,
        }
    }

    fn first_node(&self, op: &str, v: &Val<Nd<D>>) -> Result<Option<Nd<D>>> {
        match v {
            Val::Node(n) if self.dom.node_is_null(n) => Ok(None),
            Val::Node(n) => Ok(Some(n.clone())),
            Val::Nodes(ns) => Ok(ns.first().cloned()),
            Val::V(Value::Null) => Ok(None),
            other => self.err(op, format!("expected a node, got {}", kind(other))),
        }
    }

    fn to_str(&self, op: &str, v: &Val<Nd<D>>) -> Result<Option<String>> {
        match v {
            Val::Node(n) if self.dom.node_is_null(n) => Ok(None),
            Val::Node(n) => Ok(Some(self.dom.text(n))),
            Val::Nodes(ns) => Ok(ns.first().map(|n| self.dom.text(n))),
            Val::V(Value::Null) => Ok(None),
            Val::V(x @ (Value::Str(_) | Value::Int(_) | Value::Float(_) | Value::Bool(_))) => {
                Ok(x.scalar_string())
            }
            other => self.err(op, format!("expected a string, got {}", kind(other))),
        }
    }

    fn children_of(&self, n: &Nd<D>) -> Vec<Nd<D>> {
        let mut out = Vec::new();
        self.dom.children(n, &mut out);
        out
    }

    /// Items a collection-like value iterates over. A single node iterates its children.
    fn items(&self, op: &str, v: Val<Nd<D>>) -> Result<Option<Vec<Val<Nd<D>>>>> {
        match v {
            Val::Nodes(ns) => Ok(Some(ns.into_iter().map(Val::Node).collect())),
            Val::Node(n) if self.dom.node_is_null(&n) => Ok(None),
            Val::Node(n) => Ok(Some(
                self.children_of(&n).into_iter().map(Val::Node).collect(),
            )),
            Val::V(Value::List(l)) => Ok(Some(l.into_iter().map(|x| Val::V(x)).collect())),
            Val::V(Value::Null) => Ok(None),
            other => self.err(op, format!("expected a collection, got {}", kind(&other))),
        }
    }

    fn select_into(
        &self,
        op: &str,
        cur: &Val<Nd<D>>,
        i: usize,
        first_only: bool,
    ) -> Result<Option<Vec<Nd<D>>>> {
        let sel = &self.plan.sels[i];
        let mut out = Vec::new();
        match cur {
            Val::Node(n) => self.dom.select(n, sel, first_only, &mut out),
            Val::Nodes(ns) => {
                // the context is the group of a `group_by(..).map(..)`: its own nodes are candidates too
                let in_group = self.group_cur.as_ref().map_or(false, |g| {
                    g.len() == ns.len() && ns.iter().zip(g.iter()).all(|(n, k)| self.dom.node_key(n) == *k)
                });
                for n in ns {
                    if in_group && self.dom.matches_sel(n, sel) {
                        out.push(n.clone());
                        if first_only {
                            break;
                        }
                    }
                    self.dom.select(n, sel, first_only, &mut out);
                    if first_only && !out.is_empty() {
                        break;
                    }
                }
            }
            Val::V(Value::Null) => return Ok(None),
            other => return self.err(op, format!("selector expects a node, got {}", kind(other))),
        }
        Ok(Some(out))
    }

    /// Selectors that start a field pipeline of a record: `(selector, wants_all_matches)`.
    fn record_heads(body: &CExpr) -> Vec<(usize, bool)> {
        let mut heads: Vec<(usize, bool)> = Vec::new();
        if let CExpr::Object(fs) = body {
            for (_, e) in fs {
                if let CExpr::Pipe(ops) = e {
                    let (i, all) = match ops.first() {
                        Some(COp::Select(i)) => (*i, true),
                        Some(COp::SelectFirst(i)) | Some(COp::SelectExists(i)) => (*i, false),
                        _ => continue,
                    };
                    match heads.iter_mut().find(|h| h.0 == i) {
                        Some(h) => h.1 = h.1 || all,
                        None => heads.push((i, all)),
                    }
                }
            }
        }
        heads
    }

    /// The result of a pipeline's first operation if it was already computed by the fused walk (`map_items`) for this very node.
    fn prefetched(&self, op: &COp, ctx: &Val<Nd<D>>) -> Option<Val<Nd<D>>> {
        let pf = self.prefetch.as_ref()?;
        let n = match ctx {
            Val::Node(n) => n,
            _ => return None,
        };
        if self.dom.node_key(n) != pf.key {
            return None;
        }
        let (i, kind) = match op {
            COp::Select(i) => (*i, 0u8),
            COp::SelectFirst(i) => (*i, 1u8),
            COp::SelectExists(i) => (*i, 2u8),
            _ => return None,
        };
        let at = pf.sels.iter().position(|s| *s == i)?;
        let found = &pf.found[at];
        match kind {
            0 => {
                if !pf.all[at] {
                    return None; // only the first match was collected
                }
                Some(Val::Nodes(found.clone()))
            }
            1 => Some(match found.first() {
                Some(n) => Val::Node(n.clone()),
                None => null(),
            }),
            _ => Some(Val::V(Value::Bool(!found.is_empty()))),
        }
    }

    /// Parse JSON text, remembering the last large one: several fields reading branches of the same
    /// `__NEXT_DATA__` parse it once.
    fn json_source(&mut self, cur: &Val<Nd<D>>) -> Result<Option<Rc<serde_json::Value>>> {
        let node = match cur {
            Val::Node(n) => Some(n),
            Val::Nodes(ns) => ns.first(),
            _ => None,
        };
        if let Some(n) = node {
            let key = self.dom.node_key(n);
            if let Some(d) = self.json_nodes.get(&key) {
                return Ok(Some(Rc::clone(d)));
            }
            let text = self.dom.text(n);
            return match serde_json::from_str::<serde_json::Value>(text.trim()) {
                Ok(j) => {
                    let rc = Rc::new(j);
                    // only big documents are worth remembering, and only a handful of them
                    if text.len() >= JSON_CACHE_MIN && self.json_nodes.len() < 16 {
                        self.json_nodes.insert(key, Rc::clone(&rc));
                    }
                    Ok(Some(rc))
                }
                Err(e) => self.err("parse_json", e.to_string()),
            };
        }
        match self.to_str("parse_json", cur)? {
            None => Ok(None),
            Some(s) => match self.json_doc(&s) {
                Ok(d) => Ok(Some(d)),
                Err(m) => self.err("parse_json", m),
            },
        }
    }

    fn json_doc(&mut self, s: &str) -> std::result::Result<Rc<serde_json::Value>, String> {
        let t = s.trim();
        if let Some((src, doc)) = &self.json_cache {
            if src.as_str() == t {
                return Ok(Rc::clone(doc));
            }
        }
        let parsed: serde_json::Value = serde_json::from_str(t).map_err(|e| e.to_string())?;
        let rc = Rc::new(parsed);
        if t.len() >= JSON_CACHE_MIN {
            self.json_cache = Some((t.to_string(), Rc::clone(&rc)));
        }
        Ok(rc)
    }

    fn map_items(
        &mut self,
        body: &CExpr,
        items: &[Val<Nd<D>>],
        mode: OnError,
    ) -> Result<Vec<Value>> {
        let mut out = Vec::with_capacity(items.len());
        // fields that start with a selector: one walk of the record's subtree for all of them (engines that
        // profit from it, see `Dom::MULTI_SELECT`)
        let heads: Vec<(usize, bool)> = if D::MULTI_SELECT { Self::record_heads(body) } else { Vec::new() };
        let fuse = heads.len() >= FUSE_MIN_HEADS;
        let saved = if fuse { self.prefetch.take() } else { None };
        // built once per `map`, reused for every record: no allocation per record
        let plan = self.plan;
        let (head_sels, head_first): (Vec<&D::Sel>, Vec<bool>) = if fuse {
            (heads.iter().map(|h| &plan.sels[h.0]).collect(), heads.iter().map(|h| !h.1).collect())
        } else {
            (Vec::new(), Vec::new())
        };
        for (i, it) in items.iter().enumerate() {
            if self.groups_mode {
                if let Val::Nodes(ns) = it {
                    self.group_cur = Some(ns.iter().map(|n| self.dom.node_key(n)).collect());
                }
            }
            if fuse {
                if let Val::Node(n) = it {
                    // take/put back: a failed nested `map` may have dropped the buffers, then they are rebuilt
                    let mut pf = self.prefetch.take().unwrap_or_else(|| Prefetch {
                        key: 0,
                        sels: heads.iter().map(|h| h.0).collect(),
                        all: heads.iter().map(|h| h.1).collect(),
                        found: heads.iter().map(|_| Vec::new()).collect(),
                    });
                    for v in pf.found.iter_mut() {
                        v.clear();
                    }
                    pf.key = self.dom.node_key(n);
                    self.dom.select_multi(n, &head_sels, &head_first, &mut pf.found);
                    self.prefetch = Some(pf);
                }
            }
            let (plen, pmark, rmark) = (self.path.len(), self.pending.len(), self.reqs.len());
            self.path.push(format!("[{i}]"));
            match self.eval(body, it, Some(i)) {
                Ok(r) => {
                    out.push(self.finalize(r));
                    self.path.pop();
                }
                Err(Error::Exec { path, msg }) if mode != OnError::Raise => {
                    // quarantine this record: report it, forget what it started, carry on with the next one
                    self.errors.push(ErrorInfo { path, message: msg });
                    self.path.truncate(plen);
                    self.pending.truncate(pmark);
                    self.reqs.truncate(rmark);
                    if mode == OnError::Null {
                        out.push(Value::Null);
                    }
                }
                Err(e) => return Err(e),
            }
        }
        if fuse {
            self.prefetch = saved;
        }
        Ok(out)
    }

    /// Remember what an adaptive pipeline produced (up to three distinct values) next to its fingerprint.
    fn record_samples(&mut self, ops: &[COp], cur: &Val<Nd<D>>) {
        for op in ops {
            if let COp::Adaptive { key, .. } = op {
                let healed = adaptive::healed_key(key);
                let wants = |p: &PendingFp| (p.key == *key || p.key == healed) && p.fp.samples.len() < 3;
                if !self.pending.iter().any(|p| wants(p)) {
                    continue;
                }
                let mut fresh: Vec<String> = Vec::new();
                match cur {
                    Val::V(v) => sample_strings(v, &mut fresh),
                    Val::Node(n) => fresh.push(self.dom.text(n)),
                    Val::Nodes(ns) => {
                        for n in ns.iter().take(3) {
                            fresh.push(self.dom.text(n));
                        }
                    }
                }
                for p in self.pending.iter_mut().filter(|p| p.key == *key || p.key == healed) {
                    for s in &fresh {
                        let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect();
                        if !s.is_empty() && p.fp.samples.len() < 3 && !p.fp.samples.contains(&s) {
                            p.fp.samples.push(s);
                        }
                    }
                }
            }
        }
    }

    fn note_miss(&mut self, key: &str, css: &str, fingerprint: Option<adaptive::Fingerprint>) {
        if !self.misses.iter().any(|m| m.key == key) {
            self.misses.push(AdaptiveMiss {
                key: key.to_string(),
                css: css.to_string(),
                fingerprint,
            });
        }
    }

    fn str_op(
        &self,
        op: &str,
        cur: &Val<Nd<D>>,
        f: impl FnOnce(String) -> Value,
    ) -> Result<Val<Nd<D>>> {
        Ok(match self.to_str(op, cur)? {
            Some(s) => Val::V(f(s)),
            None => null(),
        })
    }

    fn dedup_values(
        &mut self,
        values: Vec<Value>,
        by: Option<&CExpr>,
        keep_last: bool,
    ) -> Result<Vec<Value>> {
        let Some(expr) = by else {
            return Ok(crate::structured::dedup_plain(values, keep_last));
        };
        let mut keys: Vec<String> = Vec::with_capacity(values.len());
        for (i, v) in values.iter().enumerate() {
            self.path.push(format!("[{i}]"));
            let kv = self.eval(expr, &Val::V(v.clone()), Some(i))?;
            self.path.pop();
            let fin = self.finalize(kv);
            keys.push(crate::structured::canonical_key(&fin));
        }
        Ok(crate::structured::dedup_with_keys(values, keys, keep_last))
    }

    fn sub_value(&mut self, e: &CExpr, ctx: &Val<Nd<D>>, idx: Option<usize>) -> Result<Value> {
        let v = self.eval(e, ctx, idx)?;
        Ok(self.finalize(v))
    }

    // ---- operations ----------------------------------------------------

    fn apply(
        &mut self,
        op: &COp,
        cur: Val<Nd<D>>,
        ctx: &Val<Nd<D>>,
        idx: Option<usize>,
    ) -> Result<Val<Nd<D>>> {
        Ok(match op {
            COp::Select(i) => match self.select_into("css", &cur, *i, false)? {
                Some(v) => Val::Nodes(v),
                None => null(),
            },
            COp::XPath(i) => {
                let xp = &self.plan.xpaths[*i];
                let ctxs: Vec<Nd<D>> = match &cur {
                    Val::Node(n) => vec![n.clone()],
                    Val::Nodes(ns) => ns.clone(),
                    Val::V(Value::Null) => return Ok(null()),
                    other => {
                        return self.err("xpath", format!("expected a node, got {}", kind(other)))
                    }
                };
                let mut nodes: Vec<Nd<D>> = Vec::new();
                let mut strings: Vec<Value> = Vec::new();
                let mut scalar: Option<Value> = None;
                let mut seen_strings = false;
                for c in &ctxs {
                    match xpath::evaluate(self.dom, c, xp) {
                        Ok(XValue::Nodes(v)) => nodes.extend(v),
                        Ok(XValue::Strings(v)) => {
                            seen_strings = true;
                            strings.extend(v.into_iter().map(Value::Str));
                        }
                        Ok(XValue::Num(n)) => scalar = Some(num_value(n)),
                        Ok(XValue::Str(t)) => scalar = Some(Value::Str(t)),
                        Ok(XValue::Bool(b)) => scalar = Some(Value::Bool(b)),
                        Err(m) => return self.err("xpath", m),
                    }
                }
                match scalar {
                    Some(v) => Val::V(v),
                    None if seen_strings => Val::V(Value::List(strings)),
                    None => Val::Nodes(nodes),
                }
            }
            COp::Adaptive {
                sel,
                all,
                key,
                threshold,
                multi,
            } => {
                let ctxs: Vec<Nd<D>> = match &cur {
                    Val::Node(n) => vec![n.clone()],
                    Val::Nodes(ns) => ns.clone(),
                    Val::V(Value::Null) => return Ok(null()),
                    other => {
                        return self.err(
                            "adaptive",
                            format!("selector expects a node, got {}", kind(other)),
                        )
                    }
                };
                let plan = self.plan;
                let ov = self.overrides;
                let (primary, css_now): (&D::Sel, &str) = match ov.get(key.as_str()) {
                    Some((css, s)) => (s, css.as_str()),
                    None => (&plan.sels[*sel], plan.sel_src[*sel].as_str()),
                };
                let overridden = ov.contains_key(key.as_str());
                let mut found: Vec<Nd<D>> = Vec::new();
                for c in &ctxs {
                    self.dom.select(c, primary, false, &mut found);
                }
                if !found.is_empty() {
                    if overridden && !self.pending.iter().any(|p| p.key == *key) {
                        // the selector comes from a repair and matches: its element is the new reference
                        let fp = adaptive::fingerprint(self.dom, &found[0], true);
                        self.pending.push(PendingFp {
                            key: key.clone(),
                            fp,
                            heal: true,
                        });
                    } else if !plan.store.contains(key)
                        && !self.pending.iter().any(|p| !p.heal && p.key == *key)
                    {
                        let fp = adaptive::fingerprint(self.dom, &found[0], true);
                        self.pending.push(PendingFp {
                            key: key.clone(),
                            fp,
                            heal: false,
                        });
                    }
                    Val::Nodes(found)
                } else if let Some(base) = plan.store.get(key) {
                    // the baseline (first learned fingerprint) is never overwritten; the last healed one is
                    // kept next to it, so repeated relocations on the new layout stay stable but one wrong
                    // relocation cannot erase what the element originally looked like
                    let healed_key = adaptive::healed_key(key);
                    let healed = plan.store.get(&healed_key);
                    let refs: Vec<&adaptive::Fingerprint> =
                        std::iter::once(&base).chain(healed.iter()).collect();
                    let relocated = if *multi {
                        adaptive::relocate_list(self.dom, &ctxs, &plan.sels[*all], &refs, *threshold)
                    } else {
                        adaptive::relocate(self.dom, &ctxs, &plan.sels[*all], &refs, *threshold).map(|(n, f)| (vec![n], f))
                    };
                    match relocated {
                        Some((nodes, mut fresh)) => {
                            // the healed fingerprint remembers what the old one produced, until new values arrive
                            fresh.samples = base.samples.clone();
                            self.pending.push(PendingFp {
                                key: healed_key,
                                fp: fresh,
                                heal: true,
                            });
                            Val::Nodes(nodes)
                        }
                        None => {
                            self.note_miss(key, css_now, Some(base));
                            Val::Nodes(Vec::new())
                        }
                    }
                } else {
                    self.note_miss(key, css_now, None);
                    Val::Nodes(Vec::new())
                }
            }
            COp::HasAttr(name) => Val::V(Value::Bool(match self.first_node("has_attr", &cur)? {
                Some(n) => self.dom.attr(&n, name).is_some(),
                None => false,
            })),
            COp::TextOpts {
                sep,
                strip,
                without,
                br,
                deep,
            } => match self.first_node("text", &cur)? {
                None => null(),
                Some(n) => {
                    if !*deep {
                        if br.is_some() {
                            let mut pieces: Vec<Option<String>> = Vec::new();
                            let mut depth = 0usize;
                            let empty: HashSet<usize> = HashSet::new();
                            if !self.dom.walk(&n, &empty, &mut |ev| match ev {
                                Walk::Open { name, .. } => {
                                    depth += 1;
                                    if depth == 2 && name.eq_ignore_ascii_case("br") {
                                        pieces.push(None);
                                    }
                                }
                                Walk::Close { .. } => {
                                    depth = depth.saturating_sub(1);
                                }
                                Walk::Text(t) => {
                                    if depth == 1 {
                                        pieces.push(Some(t.to_owned()));
                                    }
                                }
                            }) {
                                return self.err("text", format!("`br=` is not supported by the `{}` engine", D::NAME));
                            }
                            return Ok(Val::V(Value::Str(join_pieces(&pieces, sep, *strip, br.as_deref()))));
                        }
                        let mut parts = self.dom.direct_texts(&n);
                        if *strip {
                            parts = parts
                                .into_iter()
                                .map(|p| p.trim().to_string())
                                .filter(|p| !p.is_empty())
                                .collect();
                        }
                        return Ok(Val::V(Value::Str(parts.join(sep.as_str()))));
                    }
                    let mut skip: HashSet<usize> = HashSet::new();
                    if let Some(w) = without {
                        let mut gone: Vec<Nd<D>> = Vec::new();
                        self.dom.select(&n, &self.plan.sels[*w], false, &mut gone);
                        skip.extend(gone.iter().map(|g| self.dom.node_key(g)));
                    }
                    if br.is_some() {
                        let mut pieces: Vec<Option<String>> = Vec::new();
                        if !self.dom.text_pieces(&n, &skip, &mut pieces) {
                            return self.err("text", format!("`br=` is not supported by the `{}` engine", D::NAME));
                        }
                        return Ok(Val::V(Value::Str(join_pieces(&pieces, sep, *strip, br.as_deref()))));
                    }
                    let mut parts: Vec<String> = Vec::new();
                    self.dom.text_parts(&n, &skip, &mut parts);
                    if *strip {
                        parts = parts
                            .into_iter()
                            .map(|p| p.trim().to_string())
                            .filter(|p| !p.is_empty())
                            .collect();
                    }
                    Val::V(Value::Str(parts.join(sep.as_str())))
                }
            },
            COp::OwnTexts => match self.first_node("own_texts", &cur)? {
                None => null(),
                Some(n) => {
                    let parts = self.dom.direct_texts(&n);
                    Val::V(Value::List(parts.into_iter().map(Value::Str).collect()))
                }
            },
            COp::Table {
                mode,
                strip,
                header_sep,
            } => match self.first_node("table", &cur)? {
                None => null(),
                Some(n) => Val::V(crate::structured::extract_table(
                    self.dom,
                    &n,
                    *mode,
                    *strip,
                    header_sep,
                )),
            },
            COp::Distill { keep, max_text, max_chars } => match self.first_node("distill", &cur)? {
                None => null(),
                Some(n) => match self.dom.distill(&n, *keep, *max_text, *max_chars) {
                    Some(s) => Val::V(Value::Str(s)),
                    None => match distill_html(&self.dom.html(&n, false), *keep, *max_text, *max_chars) {
                        Some(s) => Val::V(Value::Str(s)),
                        None => return self.err("distill", "needs the `lite` engine in the build".to_string()),
                    },
                },
            },
            COp::Markdown { without } => match self.first_node("markdown", &cur)? {
                None => null(),
                Some(n) => {
                    let mut skip: HashSet<usize> = HashSet::new();
                    if let Some(w) = without {
                        let mut gone: Vec<Nd<D>> = Vec::new();
                        self.dom.select(&n, &self.plan.sels[*w], false, &mut gone);
                        skip.extend(gone.iter().map(|g| self.dom.node_key(g)));
                    }
                    let mut r = crate::markdown::Renderer::new();
                    let ok = self.dom.walk(&n, &skip, &mut |ev| r.feed(ev));
                    if !ok {
                        return self.err("markdown", format!("not supported by the `{}` engine", D::NAME));
                    }
                    Val::V(Value::Str(r.finish()))
                }
            },
            COp::NonEmpty => match &cur {
                Val::Node(n) if self.dom.node_is_empty(n) => null(),
                Val::V(Value::Str(s)) if s.trim().is_empty() => null(),
                Val::V(Value::List(l)) if l.is_empty() => null(),
                Val::V(Value::Map(m)) if m.is_empty() => null(),
                Val::Nodes(ns) if ns.is_empty() => null(),
                _ => cur.clone(),
            },
            COp::ParseJson => match self.json_source(&cur)? {
                None => null(),
                Some(doc) => Val::V(Value::from_json_ref(&doc)),
            },
            COp::JsonPath(segs) => match self.json_source(&cur)? {
                None => null(),
                Some(doc) => match self.nav_json(&doc, segs)? {
                    Some(node) => Val::V(Value::from_json_ref(node)),
                    None => null(),
                },
            },
            COp::ParseHtml(html_idx) => {
                if self.is_missing(&cur) {
                    return Ok(null());
                }
                let eng = &self.plan.html_subs[*html_idx];
                let html = match cur {
                    Val::V(Value::Str(s)) => s,
                    Val::Node(n) => {
                        if D::NAME == "json" {
                            match self.dom.to_value(&n) {
                                Value::Str(s) => s,
                                Value::Null => return Ok(null()),
                                other => {
                                    return self.err(
                                        "parse_html",
                                        format!("expected an HTML string, got {other:?}"),
                                    )
                                }
                            }
                        } else {
                            self.dom.text(&n)
                        }
                    }
                    Val::Nodes(ns) => match ns.first() {
                        None => return Ok(null()),
                        Some(n) => {
                            if D::NAME == "json" {
                                match self.dom.to_value(n) {
                                    Value::Str(s) => s,
                                    Value::Null => return Ok(null()),
                                    other => {
                                        return self.err(
                                            "parse_html",
                                            format!("expected an HTML string, got {other:?}"),
                                        )
                                    }
                                }
                            } else {
                                self.dom.text(n)
                            }
                        }
                    },
                    other => {
                        return self.err(
                            "parse_html",
                            format!("expected an HTML string, got {}", kind(&other)),
                        )
                    }
                };
                Val::V(eng.extract(&html, self.meta.clone())?)
            }
            COp::LinkHeader(rel) => match self.to_str("link_header", &cur)? {
                None => null(),
                Some(s) => {
                    let links = crate::url::parse_link_header(&s);
                    match rel {
                        Some(r) => links
                            .get(&r.to_ascii_lowercase())
                            .cloned()
                            .map(|u| Val::V(Value::Str(u)))
                            .unwrap_or_else(null),
                        None => Val::V(Value::Map(
                            links
                                .into_iter()
                                .map(|(k, v)| (k, Value::Str(v)))
                                .collect(),
                        )),
                    }
                }
            },
            COp::Run(e) => self.eval(e, ctx, idx)?,
            COp::ResolveUrl(base) => {
                let rel = match self.to_str("url", &cur)? {
                    Some(s) => s,
                    None => return Ok(null()),
                };
                let base_s: Option<String> = match base {
                    Some(e) => match self.sub_value(e, ctx, idx)? {
                        Value::Str(b) => Some(b),
                        Value::Null => None,
                        other => {
                            return self.err("url", format!("base must be a string, got {other:?}"))
                        }
                    },
                    // `meta["base_url"]` is the URL of the page itself; the page's `<base href>` is resolved against
                    // it (RFC 3986 / HTML), and links against the result
                    None => {
                        let page_url = match self.meta {
                            Value::Map(m) => match m.get("base_url") {
                                Some(Value::Str(b)) => Some(b.clone()),
                                _ => None,
                            },
                            _ => None,
                        };
                        if self.base_href.is_none() {
                            self.base_href = Some(self.dom.base_href());
                        }
                        match (self.base_href.clone().flatten(), page_url) {
                            (Some(h), Some(page)) => match crate::url::resolve(Some(&page), &h) {
                                Ok(b) => Some(b),
                                Err(m) => return self.err("url", format!("<base href={h:?}>: {m}")),
                            },
                            (Some(h), None) => Some(h),
                            (None, page) => page,
                        }
                    }
                };
                match crate::url::resolve(base_s.as_deref(), rel.trim()) {
                    Ok(u) => Val::V(Value::Str(u)),
                    Err(m) => return self.err("url", m),
                }
            }
            COp::JsJson(var) => {
                let node = match &cur {
                    Val::Node(n) => Some(n.clone()),
                    Val::Nodes(ns) => ns.first().cloned(),
                    _ => None,
                };
                if let Some(n) = node {
                    let key = self.dom.node_key(&n);
                    if let Some(doc) = self
                        .js_nodes
                        .get(&key)
                        .and_then(|v| v.iter().find(|(k, _)| k.as_deref() == var.as_deref()))
                        .map(|(_, d)| Rc::clone(d))
                    {
                        return Ok(Val::V(Value::from_json_ref(&doc)));
                    }
                    let text = self.dom.text(&n);
                    return match crate::jsjson::extract(&text, var.as_deref()) {
                        Ok(j) => {
                            let doc = Rc::new(j);
                            // big scripts only, and only a handful of them
                            if text.len() >= JSON_CACHE_MIN && self.js_nodes.len() < 16 {
                                self.js_nodes.entry(key).or_default().push((var.clone(), Rc::clone(&doc)));
                            }
                            Ok(Val::V(Value::from_json_ref(&doc)))
                        }
                        Err(m) => self.err("js_json", m),
                    };
                }
                match self.to_str("js_json", &cur)? {
                    None => null(),
                    Some(s) => match crate::jsjson::extract(&s, var.as_deref()) {
                        Ok(j) => Val::V(Value::from_json(j)),
                        Err(m) => return self.err("js_json", m),
                    },
                }
            }
            COp::Sections { header, items } => match self.first_node("sections", &cur)? {
                None => null(),
                Some(parent) => {
                    let plan = self.plan;
                    let keys_of = |this: &Self, sel: usize| -> HashSet<usize> {
                        let mut found: Vec<Nd<D>> = Vec::new();
                        this.dom.select(&parent, &plan.sels[sel], false, &mut found);
                        found.iter().map(|n| this.dom.node_key(n)).collect()
                    };
                    let headers = keys_of(self, *header);
                    let wanted: Option<HashSet<usize>> = items.map(|i| keys_of(self, i));
                    let mut kids: Vec<Nd<D>> = Vec::new();
                    self.dom.children(&parent, &mut kids);
                    let mut out: Vec<Value> = Vec::new();
                    let mut open: Option<(String, Vec<Value>)> = None;
                    for k in &kids {
                        let key = self.dom.node_key(k);
                        if headers.contains(&key) {
                            if let Some((h, its)) = open.take() {
                                out.push(section_value(h, its));
                            }
                            open = Some((self.dom.text(k).trim().to_string(), Vec::new()));
                        } else if let Some((_, its)) = open.as_mut() {
                            if wanted.as_ref().map_or(true, |w| w.contains(&key)) {
                                its.push(Value::Str(self.dom.text(k).trim().to_string()));
                            }
                        }
                    }
                    if let Some((h, its)) = open.take() {
                        out.push(section_value(h, its));
                    }
                    Val::V(Value::List(out))
                }
            },
            COp::GroupBy { .. } => {
                return self.err("group_by", "must be followed by `.map(...)`: the groups are only visible to its body".to_string())
            }
            COp::GroupMap { header, items, body, mode } => match self.first_node("group_by", &cur)? {
                None => null(),
                Some(parent) => {
                    let plan = self.plan;
                    let keys_of = |this: &Self, sel: usize| -> HashSet<usize> {
                        let mut found: Vec<Nd<D>> = Vec::new();
                        this.dom.select(&parent, &plan.sels[sel], false, &mut found);
                        found.iter().map(|n| this.dom.node_key(n)).collect()
                    };
                    let headers = keys_of(self, *header);
                    let wanted: Option<HashSet<usize>> = items.map(|i| keys_of(self, i));
                    let mut kids: Vec<Nd<D>> = Vec::new();
                    self.dom.children(&parent, &mut kids);
                    let mut groups: Vec<Vec<Nd<D>>> = Vec::new();
                    for k in kids {
                        let key = self.dom.node_key(&k);
                        if headers.contains(&key) {
                            groups.push(vec![k]);
                        } else if let Some(g) = groups.last_mut() {
                            if wanted.as_ref().map_or(true, |w| w.contains(&key)) {
                                g.push(k);
                            }
                        }
                    }
                    let items_v: Vec<Val<Nd<D>>> = groups.into_iter().map(Val::Nodes).collect();
                    let saved_mode = std::mem::replace(&mut self.groups_mode, true);
                    let saved_cur = self.group_cur.take();
                    let r = self.map_items(body, &items_v, *mode);
                    self.groups_mode = saved_mode;
                    self.group_cur = saved_cur;
                    Val::V(Value::List(r?))
                }
            },
            COp::JsonLd { sel, schema_type, resolve_refs } => match self.select_into("json_ld", &cur, *sel, false)? {
                None => null(),
                Some(scripts) => {
                    let mut flat: Vec<serde_json::Value> = Vec::new();
                    for s in &scripts {
                        let Some(media_type) = self.dom.attr(s, "type") else { continue };
                        let mime = media_type.split(';').next().unwrap_or("").trim();
                        if !mime.eq_ignore_ascii_case("application/ld+json") {
                            continue;
                        }
                        let text = self.dom.text(s);
                        let t = text.trim();
                        let t = t.strip_prefix("<!--").unwrap_or(t);
                        let t = t.strip_suffix("-->").unwrap_or(t).trim();
                        // JSON-LD in the wild is often broken: a bad block is skipped, not an error
                        if let Ok(j) = serde_json::from_str::<serde_json::Value>(t) {
                            flatten_ld(j, &mut flat);
                        }
                    }
                    if *resolve_refs {
                        crate::structured::resolve_json_ld_refs(&mut flat);
                    }
                    let out: Vec<Value> = flat
                        .iter()
                        .filter(|o| schema_type.as_deref().map_or(true, |want| ld_has_type(o, want)))
                        .map(Value::from_json_ref)
                        .collect();
                    Val::V(Value::List(out))
                }
            },
            COp::Microdata { sel, schema_type } => {
                let ctxs: Vec<Nd<D>> = match &cur {
                    Val::Node(n) => vec![n.clone()],
                    Val::Nodes(ns) => ns.clone(),
                    Val::V(Value::Null) => return Ok(null()),
                    other => return self.err("microdata", format!("expects a node, got {}", kind(other))),
                };
                if ctxs.is_empty() {
                    return Ok(null());
                }
                let mut scopes: Vec<Nd<D>> = Vec::new();
                let mut seen_keys: HashSet<usize> = HashSet::new();
                for c in &ctxs {
                    if self.dom.attr(c, "itemscope").is_some() && seen_keys.insert(self.dom.node_key(c)) {
                        scopes.push(c.clone());
                    }
                    let mut found: Vec<Nd<D>> = Vec::new();
                    self.dom.select(c, &self.plan.sels[*sel], false, &mut found);
                    for f in found {
                        if seen_keys.insert(self.dom.node_key(&f)) {
                            scopes.push(f);
                        }
                    }
                }
                Val::V(crate::structured::extract_microdata(
                    self.dom,
                    &scopes,
                    &self.dom.root(),
                    schema_type.as_deref(),
                ))
            }
            COp::NextFlight { sel, key } => match cur {
                Val::V(Value::Null) => null(),
                Val::V(Value::Str(s)) => Val::V(crate::structured::parse_next_flight(&[s], key.as_deref())),
                Val::V(Value::List(items)) => {
                    let scripts: Vec<String> = items
                        .into_iter()
                        .filter_map(|v| match v {
                            Value::Str(s) => Some(s),
                            _ => None,
                        })
                        .collect();
                    Val::V(crate::structured::parse_next_flight(&scripts, key.as_deref()))
                }
                Val::Node(n) => {
                    let mut scripts_nodes: Vec<Nd<D>> = Vec::new();
                    let mut seen_keys: HashSet<usize> = HashSet::new();
                    if self.dom.name(&n).map_or(false, |t| t.eq_ignore_ascii_case("script"))
                        && seen_keys.insert(self.dom.node_key(&n))
                    {
                        scripts_nodes.push(n.clone());
                    } else if let Some(si) = sel {
                        let mut found: Vec<Nd<D>> = Vec::new();
                        self.dom.select(&n, &self.plan.sels[*si], false, &mut found);
                        for f in found {
                            if seen_keys.insert(self.dom.node_key(&f)) {
                                scripts_nodes.push(f);
                            }
                        }
                    }
                    let texts: Vec<String> = scripts_nodes.iter().map(|s| self.dom.text(s)).collect();
                    Val::V(crate::structured::parse_next_flight(&texts, key.as_deref()))
                }
                Val::Nodes(ns) => {
                    if ns.is_empty() {
                        return Ok(null());
                    }
                    let mut scripts_nodes: Vec<Nd<D>> = Vec::new();
                    let mut seen_keys: HashSet<usize> = HashSet::new();
                    for n in &ns {
                        if self.dom.name(n).map_or(false, |t| t.eq_ignore_ascii_case("script")) {
                            if seen_keys.insert(self.dom.node_key(n)) {
                                scripts_nodes.push(n.clone());
                            }
                        } else if let Some(si) = sel {
                            let mut found: Vec<Nd<D>> = Vec::new();
                            self.dom.select(n, &self.plan.sels[*si], false, &mut found);
                            for f in found {
                                if seen_keys.insert(self.dom.node_key(&f)) {
                                    scripts_nodes.push(f);
                                }
                            }
                        }
                    }
                    let texts: Vec<String> = scripts_nodes.iter().map(|s| self.dom.text(s)).collect();
                    Val::V(crate::structured::parse_next_flight(&texts, key.as_deref()))
                }
                other => return self.err("next_flight", format!("expects a node or string, got {}", kind(&other))),
            },
            COp::MainContent => match self.first_node("main_content", &cur)? {
                None => null(),
                Some(n) => match crate::structured::find_main_content(self.dom, &n) {
                    Some(best) => Val::Node(best),
                    None => null(),
                },
            },
            COp::MetaTags { sel, prefixes, mode } => match self.select_into("meta_tags", &cur, *sel, false)? {
                None => null(),
                Some(metas) => {
                    let mut m: IndexMap<String, Value> = IndexMap::new();
                    for n in &metas {
                        let key = match self.dom.attr(n, "property").filter(|s| !s.trim().is_empty()) {
                            Some(k) => k,
                            None => match self.dom.attr(n, "name") {
                                Some(k) if !k.trim().is_empty() => k,
                                _ => continue,
                            },
                        };
                        let key = key.trim().to_ascii_lowercase();
                        if !prefixes.is_empty() && !prefixes.iter().any(|p| key.starts_with(p.as_str())) {
                            continue;
                        }
                        let content = match self.dom.attr(n, "content") {
                            Some(c) => c,
                            None => continue,
                        };
                        meta_insert(&mut m, key, content, *mode);
                    }
                    Val::V(Value::Map(m))
                }
            },
            COp::SelectFirst(i) => match self.select_into("css", &cur, *i, true)? {
                Some(mut v) if !v.is_empty() => Val::Node(v.swap_remove(0)),
                _ => null(),
            },
            COp::SelectExists(i) => {
                let found = self
                    .select_into("css", &cur, *i, true)?
                    .map(|v| !v.is_empty())
                    .unwrap_or(false);
                Val::V(Value::Bool(found))
            }
            COp::Parent => match &cur {
                Val::Node(n) => self.dom.parent(n).map(Val::Node).unwrap_or_else(null),
                Val::Nodes(ns) => {
                    Val::Nodes(ns.iter().filter_map(|n| self.dom.parent(n)).collect())
                }
                Val::V(Value::Null) => null(),
                other => {
                    return self.err("parent", format!("expected a node, got {}", kind(other)))
                }
            },
            COp::Children => match &cur {
                Val::Node(n) => Val::Nodes(self.children_of(n)),
                Val::Nodes(ns) => Val::Nodes(ns.iter().flat_map(|n| self.children_of(n)).collect()),
                Val::V(Value::Null) => null(),
                other => {
                    return self.err("children", format!("expected a node, got {}", kind(other)))
                }
            },
            COp::Root => Val::Node(self.root.clone()),
            // `cur` is owned here: move the child out instead of deep-cloning the whole subtree at every step
            // (`parse_json()["props"]["pageProps"]["products"]` used to clone the document three times)
            COp::Key(k) => match cur {
                Val::V(Value::Map(mut m)) => {
                    Val::V(m.swap_remove(k.as_str()).unwrap_or(Value::Null))
                }
                Val::V(Value::Null) => null(),
                other => match self.first_node("key", &other)? {
                    Some(n) => match self.dom.key(&n, k) {
                        Ok(Some(c)) => Val::Node(c),
                        Ok(None) => null(),
                        Err(m) => return self.err("key", m),
                    },
                    None => null(),
                },
            },
            COp::At(i) => match cur {
                Val::Nodes(mut ns) => {
                    if *i < ns.len() {
                        Val::Node(ns.swap_remove(*i))
                    } else {
                        null()
                    }
                }
                Val::V(Value::List(mut l)) => {
                    if *i < l.len() {
                        Val::V(l.swap_remove(*i))
                    } else {
                        null()
                    }
                }
                Val::V(Value::Null) => null(),
                Val::Node(n) => match self.dom.at(&n, *i) {
                    Ok(Some(c)) => Val::Node(c),
                    Ok(None) => null(),
                    Err(m) => return self.err("at", m),
                },
                other => return self.err("at", format!("cannot index a {}", kind(&other))),
            },
            COp::Slice { start, stop } => match cur {
                Val::Nodes(mut ns) => {
                    let (lo, hi) = slice_bounds(ns.len(), *start, *stop);
                    ns.truncate(hi);
                    ns.drain(..lo);
                    Val::Nodes(ns)
                }
                Val::V(Value::List(mut l)) => {
                    let (lo, hi) = slice_bounds(l.len(), *start, *stop);
                    l.truncate(hi);
                    l.drain(..lo);
                    Val::V(Value::List(l))
                }
                Val::V(Value::Str(s)) => {
                    // ASCII (the usual case): bytes are characters, no char walk
                    let ascii = s.is_ascii();
                    let n = if ascii { s.len() } else { s.chars().count() };
                    let (lo, hi) = slice_bounds(n, *start, *stop);
                    if lo == 0 && hi == n {
                        Val::V(Value::Str(s))
                    } else if ascii {
                        Val::V(Value::Str(s[lo..hi].to_string()))
                    } else {
                        Val::V(Value::Str(s.chars().skip(lo).take(hi - lo).collect()))
                    }
                }
                Val::V(Value::Null) => null(),
                // the children of one element, as in `children()[start:stop]`
                Val::Node(n) => {
                    let mut ns = self.children_of(&n);
                    let (lo, hi) = slice_bounds(ns.len(), *start, *stop);
                    ns.truncate(hi);
                    ns.drain(..lo);
                    Val::Nodes(ns)
                }
                other => return self.err("slice", format!("cannot slice a {}", kind(&other))),
            },
            COp::First | COp::Last => {
                let first = matches!(op, COp::First);
                let name = if first { "first" } else { "last" };
                match cur {
                    Val::Nodes(mut ns) => {
                        if ns.is_empty() {
                            null()
                        } else if first {
                            Val::Node(ns.swap_remove(0))
                        } else {
                            Val::Node(ns.pop().unwrap())
                        }
                    }
                    Val::Node(n) => {
                        let mut ch = self.children_of(&n);
                        if ch.is_empty() {
                            null()
                        } else if first {
                            Val::Node(ch.swap_remove(0))
                        } else {
                            Val::Node(ch.pop().unwrap())
                        }
                    }
                    Val::V(Value::List(mut l)) => {
                        if l.is_empty() {
                            null()
                        } else if first {
                            Val::V(l.swap_remove(0))
                        } else {
                            Val::V(l.pop().unwrap())
                        }
                    }
                    Val::V(Value::Null) => null(),
                    other => {
                        return self
                            .err(name, format!("expected a collection, got {}", kind(&other)))
                    }
                }
            }
            COp::Attr(name) => match self.first_node("attr", &cur)? {
                Some(n) => self
                    .dom
                    .attr(&n, name)
                    .map(|s| Val::V(Value::Str(s)))
                    .unwrap_or_else(null),
                None => null(),
            },
            COp::Text => match self.first_node("text", &cur)? {
                Some(n) => Val::V(Value::Str(self.dom.text(&n))),
                None => null(),
            },
            COp::Html | COp::InnerHtml => {
                let inner = matches!(op, COp::InnerHtml);
                match self.first_node("html", &cur)? {
                    Some(n) => Val::V(Value::Str(self.dom.html(&n, inner))),
                    None => null(),
                }
            }
            COp::Meta(k) => match self.meta {
                Value::Map(m) => Val::V(m.get(k).cloned().unwrap_or(Value::Null)),
                _ => null(),
            },
            COp::Index => idx
                .map(|i| Val::V(Value::Int(i as i64)))
                .unwrap_or_else(null),
            COp::Count => {
                let n = match &cur {
                    Val::Nodes(ns) => ns.len(),
                    Val::Node(n) => self.children_of(n).len(),
                    Val::V(Value::List(l)) => l.len(),
                    Val::V(Value::Map(m)) => m.len(),
                    Val::V(Value::Str(s)) => s.chars().count(),
                    Val::V(Value::Null) => 0,
                    other => return self.err("count", format!("cannot count a {}", kind(other))),
                };
                Val::V(Value::Int(n as i64))
            }
            COp::Exists => Val::V(Value::Bool(self.is_truthy(&cur))),
            COp::Map(body) => match self.items("map", cur)? {
                Some(items) => Val::V(Value::List(self.map_items(body, &items, OnError::Raise)?)),
                None => null(),
            },
            COp::MapSafe(body, mode) => match self.items("map", cur)? {
                Some(items) => Val::V(Value::List(self.map_items(body, &items, *mode)?)),
                None => null(),
            },
            COp::FlatMap(body) => match self.items("flat_map", cur)? {
                Some(items) => {
                    let mut out = Vec::new();
                    for v in self.map_items(body, &items, OnError::Raise)? {
                        match v {
                            Value::List(l) => out.extend(l),
                            Value::Null => {}
                            other => out.push(other),
                        }
                    }
                    Val::V(Value::List(out))
                }
                None => null(),
            },
            COp::Filter(pred) => {
                let was_nodes = matches!(cur, Val::Nodes(_) | Val::Node(_));
                match self.items("filter", cur)? {
                    Some(items) => {
                        let mut keep = Vec::new();
                        for (i, it) in items.into_iter().enumerate() {
                            self.path.push(format!("[{i}]"));
                            let r = self.eval(pred, &it, Some(i))?;
                            self.path.pop();
                            if self.is_truthy(&r) {
                                keep.push(it);
                            }
                        }
                        if was_nodes {
                            Val::Nodes(
                                keep.into_iter()
                                    .filter_map(
                                        |v| if let Val::Node(n) = v { Some(n) } else { None },
                                    )
                                    .collect(),
                            )
                        } else {
                            Val::V(Value::List(
                                keep.into_iter()
                                    .filter_map(|v| if let Val::V(x) = v { Some(x) } else { None })
                                    .collect(),
                            ))
                        }
                    }
                    None => null(),
                }
            }
            COp::Unique { by, keep_last } => match self.items("unique", cur)? {
                None => null(),
                Some(items) => {
                    let values: Vec<Value> = items.into_iter().map(|it| self.finalize(it)).collect();
                    Val::V(Value::List(self.dedup_values(values, by.as_ref(), *keep_last)?))
                }
            },
            COp::Default(fb) => {
                if self.is_missing(&cur) {
                    self.eval(fb, ctx, idx)?
                } else {
                    cur
                }
            }
            COp::Required => {
                if self.is_missing(&cur) {
                    return self.err("required", "value is missing");
                }
                cur
            }
            COp::Coalesce(alts) => {
                for a in alts {
                    let r = self.eval(a, ctx, idx)?;
                    if !self.is_missing(&r) {
                        return Ok(r);
                    }
                }
                null()
            }
            COp::Strip => self.str_op("strip", &cur, |s| Value::Str(s.trim().to_string()))?,
            COp::Lower => self.str_op("lower", &cur, |s| Value::Str(s.to_lowercase()))?,
            COp::Upper => self.str_op("upper", &cur, |s| Value::Str(s.to_uppercase()))?,
            COp::Replace(from, to) => self.str_op("replace", &cur, |s| {
                Value::Str(s.replace(from.as_str(), to))
            })?,
            COp::Split(sep) => self.str_op("split", &cur, |s| {
                Value::List(
                    s.split(sep.as_str())
                        .map(|p| Value::Str(p.to_string()))
                        .collect(),
                )
            })?,
            COp::Join(sep) => {
                let parts: Vec<String> = match &cur {
                    Val::Nodes(ns) => ns.iter().map(|n| self.dom.text(n)).collect(),
                    Val::V(Value::List(l)) => l.iter().filter_map(|x| x.scalar_string()).collect(),
                    Val::V(Value::Null) => return Ok(null()),
                    other => {
                        return self.err("join", format!("expected a list, got {}", kind(other)))
                    }
                };
                Val::V(Value::Str(parts.join(sep.as_str())))
            }
            COp::Regex(re, g) => self.str_op("regex", &cur, |s| {
                match re.captures(&s).and_then(|c| c.get(*g)) {
                    Some(m) => Value::Str(m.as_str().to_string()),
                    None => Value::Null,
                }
            })?,
            COp::RegexAll(re, g) => self.str_op("regex_all", &cur, |s| {
                Value::List(
                    re.captures_iter(&s)
                        .filter_map(|c| c.get(*g).map(|m| Value::Str(m.as_str().to_string())))
                        .collect(),
                )
            })?,
            COp::Int => match &cur {
                Val::V(Value::Int(_)) => cur.clone(),
                Val::V(Value::Float(f)) => {
                    if f.fract() == 0.0 {
                        Val::V(Value::Int(*f as i64))
                    } else {
                        return self.err("int", format!("{f} is not an integer"));
                    }
                }
                other => match self.to_str("int", other)? {
                    None => null(),
                    Some(s) => match parse_int(&s) {
                        Ok(i) => Val::V(Value::Int(i)),
                        Err(m) => return self.err("int", m),
                    },
                },
            },
            COp::Float => match &cur {
                Val::V(Value::Float(_)) => cur.clone(),
                Val::V(Value::Int(i)) => Val::V(Value::Float(*i as f64)),
                other => match self.to_str("float", other)? {
                    None => null(),
                    Some(s) => match parse_number(&s) {
                        Some(f) => Val::V(Value::Float(f)),
                        None => return self.err("float", format!("no number in {s:?}")),
                    },
                },
            },
            COp::Bool => match &cur {
                Val::V(Value::Bool(_)) => cur.clone(),
                Val::V(Value::Int(i)) => Val::V(Value::Bool(*i != 0)),
                Val::V(Value::Float(f)) => Val::V(Value::Bool(*f != 0.0)),
                other => match self.to_str("bool", other)? {
                    None => null(),
                    Some(s) => match s.trim().to_lowercase().as_str() {
                        "true" | "1" | "yes" | "y" | "on" => Val::V(Value::Bool(true)),
                        "false" | "0" | "no" | "n" | "off" | "" => Val::V(Value::Bool(false)),
                        _ => return self.err("bool", format!("cannot read {s:?} as bool")),
                    },
                },
            },
            COp::ParseSrcset {
                pick,
                min_width,
                min_density,
            } => match self.to_str("parse_srcset", &cur)? {
                None => null(),
                Some(s) => match crate::structured::parse_srcset(
                    &s,
                    pick.as_deref(),
                    *min_width,
                    *min_density,
                ) {
                    Ok(v) => Val::V(v),
                    Err(m) => return self.err("parse_srcset", m),
                },
            },
            COp::Price { decimal, currency } => match self.to_str("price", &cur)? {
                None => null(),
                Some(s) => match crate::structured::parse_price(
                    &s,
                    decimal.as_deref(),
                    currency.as_deref(),
                ) {
                    Some(v) => Val::V(v),
                    None => null(),
                },
            },
            COp::Eq(rhs) | COp::Ne(rhs) => {
                let r = self.sub_value(rhs, ctx, idx)?;
                let l = self.finalize(cur);
                let eq = loose_eq(&l, &r);
                Val::V(Value::Bool(if matches!(op, COp::Eq(_)) { eq } else { !eq }))
            }
            COp::Contains(rhs) => {
                let r = self.sub_value(rhs, ctx, idx)?;
                let l = self.finalize(cur);
                let res = match &l {
                    Value::Str(s) => r
                        .scalar_string()
                        .map(|n| s.contains(n.as_str()))
                        .unwrap_or(false),
                    Value::List(items) => items.iter().any(|x| loose_eq(x, &r)),
                    Value::Map(m) => r
                        .scalar_string()
                        .map(|k| m.contains_key(&k))
                        .unwrap_or(false),
                    _ => false,
                };
                Val::V(Value::Bool(res))
            }
            COp::StartsWith(rhs) | COp::EndsWith(rhs) => {
                let r = self.sub_value(rhs, ctx, idx)?;
                let l = self.finalize(cur);
                let res = match (&l, r.scalar_string()) {
                    (Value::Str(s), Some(p)) => {
                        if matches!(op, COp::StartsWith(_)) {
                            s.starts_with(p.as_str())
                        } else {
                            s.ends_with(p.as_str())
                        }
                    }
                    _ => false,
                };
                Val::V(Value::Bool(res))
            }
            COp::Not => Val::V(Value::Bool(!self.is_truthy(&cur))),
            COp::And(rhs) => {
                if !self.is_truthy(&cur) {
                    Val::V(Value::Bool(false))
                } else {
                    let r = self.eval(rhs, ctx, idx)?;
                    Val::V(Value::Bool(self.is_truthy(&r)))
                }
            }
            COp::Or(rhs) => {
                if self.is_truthy(&cur) {
                    Val::V(Value::Bool(true))
                } else {
                    let r = self.eval(rhs, ctx, idx)?;
                    Val::V(Value::Bool(self.is_truthy(&r)))
                }
            }
            COp::If(c, t, o) => {
                let cv = self.eval(c, ctx, idx)?;
                if self.is_truthy(&cv) {
                    self.eval(t, ctx, idx)?
                } else {
                    self.eval(o, ctx, idx)?
                }
            }
            COp::Follow {
                request,
                meta,
                sub,
                on_null,
            } => {
                let params = match self.sub_value(request, ctx, idx)? {
                    Value::Null => None,
                    Value::Str(u) => {
                        let mut m = IndexMap::new();
                        m.insert("url".to_string(), Value::Str(u));
                        Some(m)
                    }
                    Value::Map(m) => {
                        if matches!(m.get("url"), Some(Value::Null)) {
                            None
                        } else {
                            Some(m)
                        }
                    }
                    other => {
                        return self.err(
                            "follow",
                            format!("request must be a dict or a url string, got {other:?}"),
                        )
                    }
                };
                let params = match params {
                    Some(p) => p,
                    None => {
                        // no request is made: what followed `follow(...)` still applies, to null
                        let mut v: Val<Nd<D>> = null();
                        for op in on_null {
                            v = self.apply(op, v, ctx, idx)?;
                        }
                        return Ok(v);
                    }
                };
                let mut m = match self.sub_value(meta, ctx, idx)? {
                    Value::Null => Value::Map(IndexMap::new()),
                    m => m,
                };
                // the followed page's own URL is the base of its relative links, unless the schema says otherwise
                if let (Value::Map(mm), Some(Value::Str(u))) = (&mut m, params.get("url")) {
                    if !mm.contains_key("base_url") {
                        mm.insert("base_url".to_string(), Value::Str(u.clone()));
                    }
                }
                let id = self.next_id;
                self.next_id += 1;
                self.reqs.push(Request {
                    id,
                    params,
                    meta: m,
                    kind: ReqKind::Follow { sub: *sub },
                    origin: ROOT,
                });
                Val::V(Value::Pending(id))
            }
            COp::FollowForeign {
                request,
                meta,
                foreign,
                on_null,
            } => {
                let params = match self.sub_value(request, ctx, idx)? {
                    Value::Null => None,
                    Value::Str(u) => {
                        let mut m = IndexMap::new();
                        m.insert("url".to_string(), Value::Str(u));
                        Some(m)
                    }
                    Value::Map(m) => {
                        if matches!(m.get("url"), Some(Value::Null)) {
                            None
                        } else {
                            Some(m)
                        }
                    }
                    other => {
                        return self.err(
                            "follow",
                            format!("request must be a dict or a url string, got {other:?}"),
                        )
                    }
                };
                let params = match params {
                    Some(p) => p,
                    None => {
                        let mut v: Val<Nd<D>> = null();
                        for op in on_null {
                            v = self.apply(op, v, ctx, idx)?;
                        }
                        return Ok(v);
                    }
                };
                let mut m = match self.sub_value(meta, ctx, idx)? {
                    Value::Null => Value::Map(IndexMap::new()),
                    m => m,
                };
                if let (Value::Map(mm), Some(Value::Str(u))) = (&mut m, params.get("url")) {
                    if !mm.contains_key("base_url") {
                        mm.insert("base_url".to_string(), Value::Str(u.clone()));
                    }
                }
                let id = self.next_id;
                self.next_id += 1;
                self.reqs.push(Request {
                    id,
                    params,
                    meta: m,
                    kind: ReqKind::ForeignStart { foreign: *foreign },
                    origin: ROOT,
                });
                Val::V(Value::Pending(id))
            }
            COp::Paginate { pager } => {
                let mut seen = HashSet::new();
                if let Value::Map(mm) = self.meta {
                    if let Some(Value::Str(b)) = mm.get("base_url") {
                        if !is_empty_or_noop_url(b) {
                            seen.insert(format!("url:{}", strip_url_fragment(b.trim())));
                        }
                    }
                }
                return self.step_paginate(*pager, Vec::new(), 0, seen, ctx, idx);
            }
        })
    }

    fn finish_paginate(
        &mut self,
        post: &[COp],
        mut combined: Vec<Value>,
        max_items: Option<usize>,
        ctx: &Val<Nd<D>>,
        idx: Option<usize>,
    ) -> Result<Val<Nd<D>>> {
        if let Some(limit) = max_items {
            if combined.len() > limit {
                combined.truncate(limit);
            }
        }
        let v = Val::V(Value::List(combined));
        if post.is_empty() {
            Ok(v)
        } else {
            self.run_ops(post, v, ctx, idx)
        }
    }

    fn step_paginate(
        &mut self,
        pager: usize,
        acc_before: Vec<Value>,
        pages_before: usize,
        mut seen: HashSet<String>,
        ctx: &Val<Nd<D>>,
        idx: Option<usize>,
    ) -> Result<Val<Nd<D>>> {
        let spec = &self.plan.pagers[pager];
        let raw_items = self.eval(&spec.items, ctx, idx)?;
        let page_items: Vec<Value> = match raw_items {
            Val::V(Value::Null) => Vec::new(),
            Val::V(Value::List(v)) => v,
            Val::Node(ref n) if self.dom.node_is_null(n) => Vec::new(),
            Val::Node(ref n) => match self.dom.to_value(n) {
                Value::List(v) => v,
                Value::Null => Vec::new(),
                other => {
                    return self.err(
                        "paginate",
                        format!("`items` must evaluate to a list or null, got {other:?}"),
                    )
                }
            },
            Val::Nodes(ns) => ns.iter().map(|n| self.dom.to_value(n)).collect(),
            other => {
                return self.err(
                    "paginate",
                    format!("`items` must evaluate to a list or null, got {}", kind(&other)),
                )
            }
        };
        let mut combined = acc_before;
        combined.extend(page_items);
        if spec.unique_by.is_some() {
            combined = self.dedup_values(combined, spec.unique_by.as_ref(), spec.keep_last)?;
        }
        let pages_now = pages_before + 1;

        if spec.max_items.map_or(false, |lim| combined.len() >= lim)
            || spec.max_pages.map_or(false, |mp| pages_now >= mp)
        {
            return self.finish_paginate(&spec.post, combined, spec.max_items, ctx, idx);
        }

        let params = match self.sub_value(&spec.next, ctx, idx)? {
            Value::Null => None,
            Value::Str(u) if is_empty_or_noop_url(&u) => None,
            Value::Str(u) => {
                let mut m = IndexMap::new();
                m.insert("url".to_string(), Value::Str(u));
                Some(m)
            }
            Value::Map(m) => {
                let empty_or_null = match m.get("url") {
                    Some(Value::Null) => true,
                    Some(Value::Str(s)) => is_empty_or_noop_url(s),
                    _ => false,
                };
                if empty_or_null {
                    None
                } else {
                    Some(m)
                }
            }
            other => {
                return self.err(
                    "paginate",
                    format!("`next` must be a dict, url string, or null, got {other:?}"),
                )
            }
        };
        let params = match params {
            Some(p) => p,
            None => return self.finish_paginate(&spec.post, combined, spec.max_items, ctx, idx),
        };

        if !seen.insert(canonical_paginate_key(&params)) {
            return self.finish_paginate(&spec.post, combined, spec.max_items, ctx, idx);
        }

        if spec.max_pages.is_none() && pages_now >= DEFAULT_MAX_PAGES {
            return Err(Error::Session(format!(
                "paginate exceeded {DEFAULT_MAX_PAGES} pages without `max_pages`; pass `max_pages=...` if intended"
            )));
        }

        let mut base_map = match self.meta {
            Value::Map(m) => {
                let mut c = m.clone();
                c.shift_remove("base_url");
                c.shift_remove("headers");
                c.shift_remove("status");
                c
            }
            _ => IndexMap::new(),
        };
        let mut next_meta = match self.sub_value(&spec.meta, ctx, idx)? {
            Value::Null => Value::Map(base_map),
            Value::Map(em) => {
                for (k, v) in em {
                    base_map.insert(k, v);
                }
                Value::Map(base_map)
            }
            other => other,
        };
        if let (Value::Map(mm), Some(Value::Str(u))) = (&mut next_meta, params.get("url")) {
            if !mm.contains_key("base_url") {
                mm.insert("base_url".to_string(), Value::Str(u.clone()));
            }
        }

        let id = self.next_id;
        self.next_id += 1;
        self.reqs.push(Request {
            id,
            params,
            meta: next_meta,
            kind: ReqKind::PaginatePage {
                pager,
                acc: combined,
                pages: pages_now,
                seen,
                path: self.path.clone(),
            },
            origin: ROOT,
        });
        Ok(Val::V(Value::Pending(id)))
    }
}
