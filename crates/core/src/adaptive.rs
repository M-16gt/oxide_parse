//! Adaptive element relocation. When a selector stops matching after a redesign, find the element that is
//! *most similar* to the one it used to match, instead of failing.
//!
//! It is a recovery path, not the hot path:
//!
//! * selector matches  -> use the match (the first time, remember a fingerprint of it);
//! * selector is empty -> compare candidates with the remembered fingerprint, take the best one above the
//!   threshold and remember its (new) fingerprint ("heal").
//!
//! Everything is generic over [`Dom`] (needs the introspection group: `name`, `attrs`, `parent`, `children`).
use crate::dom::Dom;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// An adaptive selector that found nothing and could not be relocated: input for a (human or LLM) repair.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AdaptiveMiss {
    pub key: String,
    pub css: String,
    /// what the element used to look like (`None`: nothing was ever learned for this key)
    pub fingerprint: Option<Fingerprint>,
}

/// A structural description of one element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub tag: String,
    /// attributes sorted by name (values clipped)
    pub attrs: Vec<(String, String)>,
    /// normalised, clipped text (empty in the "light" fingerprint)
    pub text: String,
    /// tag names from the top-level element down to the parent
    pub path: Vec<String>,
    pub parent: Option<ParentFp>,
    pub prev: Option<String>,
    pub next: Option<String>,
    /// tag names of the element children (first 16)
    pub children: Vec<String>,
    /// up to three distinct values the selector produced when the fingerprint was taken (clipped): shown to a
    /// (human or LLM) repair next to the old element description
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub samples: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParentFp {
    pub tag: String,
    pub classes: Vec<String>,
    pub id: Option<String>,
}

fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

fn classes_of(attrs: &[(String, String)]) -> Vec<String> {
    attrs
        .iter()
        .find(|(k, _)| k == "class")
        .map(|(_, v)| v.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default()
}

fn tag_of<D: Dom>(dom: &D, n: &D::Node) -> String {
    dom.name(n).map(|c| c.to_ascii_lowercase()).unwrap_or_default()
}

/// Describe `n`. `with_text = false` skips the (potentially large) text: used for the cheap first pass.
pub fn fingerprint<D: Dom>(dom: &D, n: &D::Node, with_text: bool) -> Fingerprint {
    let mut attrs: Vec<(String, String)> = Vec::new();
    dom.attrs(n, &mut attrs);
    attrs.sort();
    for a in attrs.iter_mut() {
        a.1 = clip(&a.1, 120);
    }

    let text = if with_text { clip(&dom.text(n).split_whitespace().collect::<Vec<_>>().join(" "), 200) } else { String::new() };

    let mut path: Vec<String> = Vec::new();
    let mut cur = dom.parent(n);
    while let Some(p) = cur {
        path.push(tag_of(dom, &p));
        cur = dom.parent(&p);
    }
    path.reverse();

    let parent = dom.parent(n).map(|p| {
        let mut pa: Vec<(String, String)> = Vec::new();
        dom.attrs(&p, &mut pa);
        let id = pa.iter().find(|(k, _)| k == "id").map(|(_, v)| clip(v, 60));
        ParentFp { tag: tag_of(dom, &p), classes: classes_of(&pa), id }
    });

    let prev = dom.prev_sibling(n).map(|s| tag_of(dom, &s));
    let next = dom.next_sibling(n).map(|s| tag_of(dom, &s));

    let mut ch: Vec<D::Node> = Vec::new();
    dom.children(n, &mut ch);
    let children = ch.iter().take(16).map(|c| tag_of(dom, c)).collect();

    Fingerprint { tag: tag_of(dom, n), attrs, text, path, parent, prev, next, children, samples: Vec::new() }
}

// ---------------------------------------------------------------------------------------------
// similarity
// ---------------------------------------------------------------------------------------------

fn jaccard<T: Eq + std::hash::Hash>(a: &HashSet<T>, b: &HashSet<T>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let inter = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    inter / union
}

fn lcs(a: &[String], b: &[String]) -> usize {
    let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            dp[i][j] = if a[i - 1] == b[j - 1] { dp[i - 1][j - 1] + 1 } else { dp[i - 1][j].max(dp[i][j - 1]) };
        }
    }
    dp[a.len()][b.len()]
}

const W_TAG: f64 = 0.15;
const W_ATTRS: f64 = 0.25;
const W_TEXT: f64 = 0.20;
const W_PATH: f64 = 0.15;
const W_PARENT: f64 = 0.10;
const W_SIBLINGS: f64 = 0.05;
const W_CHILDREN: f64 = 0.10;

fn attrs_score(a: &Fingerprint, b: &Fingerprint) -> f64 {
    let pa: HashSet<String> = a.attrs.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let pb: HashSet<String> = b.attrs.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let pairs = jaccard(&pa, &pb);
    let ca: HashSet<String> = classes_of(&a.attrs).into_iter().collect();
    let cb: HashSet<String> = classes_of(&b.attrs).into_iter().collect();
    if ca.is_empty() && cb.is_empty() {
        pairs
    } else {
        0.5 * pairs + 0.5 * jaccard(&ca, &cb)
    }
}

fn text_score(a: &Fingerprint, b: &Fingerprint) -> f64 {
    let wa: HashSet<&str> = a.text.split_whitespace().collect();
    let wb: HashSet<&str> = b.text.split_whitespace().collect();
    if wa.is_empty() && wb.is_empty() {
        return 1.0;
    }
    if wa.is_empty() || wb.is_empty() {
        return 0.0;
    }
    2.0 * wa.intersection(&wb).count() as f64 / (wa.len() + wb.len()) as f64
}

fn path_score(a: &Fingerprint, b: &Fingerprint) -> f64 {
    let m = a.path.len().max(b.path.len());
    if m == 0 {
        1.0
    } else {
        lcs(&a.path, &b.path) as f64 / m as f64
    }
}

fn parent_score(a: &Fingerprint, b: &Fingerprint) -> f64 {
    match (&a.parent, &b.parent) {
        (None, None) => 1.0,
        (Some(x), Some(y)) => {
            let cx: HashSet<&String> = x.classes.iter().collect();
            let cy: HashSet<&String> = y.classes.iter().collect();
            0.5 * (x.tag == y.tag) as u8 as f64 + 0.5 * jaccard(&cx, &cy)
        }
        _ => 0.0,
    }
}

fn siblings_score(a: &Fingerprint, b: &Fingerprint) -> f64 {
    0.5 * (a.prev == b.prev) as u8 as f64 + 0.5 * (a.next == b.next) as u8 as f64
}

fn children_score(a: &Fingerprint, b: &Fingerprint) -> f64 {
    let ca: HashSet<&String> = a.children.iter().collect();
    let cb: HashSet<&String> = b.children.iter().collect();
    jaccard(&ca, &cb)
}

/// Everything except the text, rescaled to 0..1 (used to rank many candidates cheaply).
fn light_similarity(a: &Fingerprint, b: &Fingerprint) -> f64 {
    let sum = W_TAG * (a.tag == b.tag) as u8 as f64
        + W_ATTRS * attrs_score(a, b)
        + W_PATH * path_score(a, b)
        + W_PARENT * parent_score(a, b)
        + W_SIBLINGS * siblings_score(a, b)
        + W_CHILDREN * children_score(a, b);
    sum / (1.0 - W_TEXT)
}

fn light_best(fps: &[&Fingerprint], cand: &Fingerprint) -> f64 {
    fps.iter().map(|r| light_similarity(r, cand)).fold(0.0, f64::max)
}

/// Weighted similarity in 0..=1 (1 = identical description).
pub fn similarity(a: &Fingerprint, b: &Fingerprint) -> f64 {
    W_TAG * (a.tag == b.tag) as u8 as f64
        + W_ATTRS * attrs_score(a, b)
        + W_TEXT * text_score(a, b)
        + W_PATH * path_score(a, b)
        + W_PARENT * parent_score(a, b)
        + W_SIBLINGS * siblings_score(a, b)
        + W_CHILDREN * children_score(a, b)
}

/// Store key of the last healed fingerprint of `key` (the baseline stays under `key`).
pub fn healed_key(key: &str) -> String {
    format!("{key}#healed")
}

/// Find the element most similar to any of the reference fingerprints `fps` (the first one is the
/// baseline) below any of `roots`. `all_sel` must be a compiled `*` selector.
/// Returns the node and its fresh fingerprint if the best score reaches `threshold`.
pub fn relocate<D: Dom>(dom: &D, roots: &[D::Node], all_sel: &D::Sel, fps: &[&Fingerprint], threshold: f64) -> Option<(D::Node, Fingerprint)> {
    let mut cands: Vec<D::Node> = Vec::new();
    for r in roots {
        dom.select(r, all_sel, false, &mut cands);
    }
    let mut seen: HashSet<usize> = HashSet::new();
    cands.retain(|n| seen.insert(dom.node_key(n)));
    // an element rarely changes its tag: prefer same-tag candidates, fall back to everything
    let same: Vec<D::Node> = cands.iter().filter(|n| fps.iter().any(|f| tag_of(dom, n) == f.tag)).cloned().collect();
    let pool = if same.is_empty() { cands } else { same };

    let mut ranked: Vec<(f64, D::Node)> = pool.into_iter().map(|n| (light_best(fps, &fingerprint(dom, &n, false)), n)).collect();
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(8);

    let mut best: Option<(f64, D::Node, Fingerprint)> = None;
    for (_, n) in ranked {
        let f = fingerprint(dom, &n, true);
        let s = fps.iter().map(|r| similarity(r, &f)).fold(0.0, f64::max);
        if best.as_ref().map_or(true, |(bs, _, _)| s > *bs) {
            best = Some((s, n, f));
        }
    }
    match best {
        Some((s, n, f)) if s >= threshold => Some((n, f)),
        _ => None,
    }
}

/// Relocation of a *list* of records (`adaptive` right before `map`): the best match plus the elements next to it
/// (same parent, same tag) that look at least `0.85 * threshold` like the reference, in document order.
pub fn relocate_list<D: Dom>(dom: &D, roots: &[D::Node], all_sel: &D::Sel, fps: &[&Fingerprint], threshold: f64) -> Option<(Vec<D::Node>, Fingerprint)> {
    let (best, fresh) = relocate(dom, roots, all_sel, fps, threshold)?;
    let parent = match dom.parent(&best) {
        Some(p) => p,
        None => return Some((vec![best], fresh)),
    };
    let me = dom.node_key(&best);
    let tag = tag_of(dom, &best);
    let mut kids: Vec<D::Node> = Vec::new();
    dom.children(&parent, &mut kids);
    let floor = threshold * 0.85;
    let mut out: Vec<D::Node> = Vec::new();
    for k in kids {
        if dom.node_key(&k) == me {
            out.push(k);
            continue;
        }
        if tag_of(dom, &k) != tag {
            continue;
        }
        let f = fingerprint(dom, &k, true);
        if fps.iter().map(|r| similarity(r, &f)).fold(0.0, f64::max) >= floor {
            out.push(k);
        }
    }
    if out.is_empty() {
        return Some((vec![best], fresh));
    }
    Some((out, fresh))
}

// ---------------------------------------------------------------------------------------------
// stores
// ---------------------------------------------------------------------------------------------

/// Where fingerprints live between runs.
pub trait Store: Send + Sync {
    fn contains(&self, key: &str) -> bool;
    fn get(&self, key: &str) -> Option<Fingerprint>;
    fn put(&self, key: &str, fp: &Fingerprint);
    fn keys(&self) -> Vec<String>;
    /// Forget a fingerprint (a repaired selector starts a fresh reference). Stores that cannot may ignore it.
    fn remove(&self, key: &str) {
        let _ = key;
    }

    /// Every fingerprint as one JSON object (`{"key": {...}}`, the format of [`FileStore`] files): keep it in a
    /// database / Redis / a queue message where there is no disk (Lambda, Celery, containers).
    fn export_json(&self) -> String {
        let mut all: BTreeMap<String, Fingerprint> = BTreeMap::new();
        for k in self.keys() {
            if let Some(fp) = self.get(&k) {
                all.insert(k, fp);
            }
        }
        serde_json::to_string(&all).unwrap_or_else(|_| "{}".to_string())
    }

    /// Merge fingerprints exported by [`Store::export_json`] into this store (same keys are overwritten).
    /// Returns how many were loaded.
    fn import_json(&self, json: &str) -> crate::error::Result<usize> {
        let all = parse_store_json(json)?;
        let n = all.len();
        for (k, fp) in &all {
            self.put(k, fp);
        }
        Ok(n)
    }
}

fn parse_store_json(json: &str) -> crate::error::Result<BTreeMap<String, Fingerprint>> {
    if json.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    serde_json::from_str(json).map_err(|e| crate::error::Error::Doc(format!("adaptive store JSON: {e}")))
}

/// In-process store (the default: adaptive learns for the lifetime of the plan).
#[derive(Default)]
pub struct MemoryStore {
    map: Mutex<BTreeMap<String, Fingerprint>>,
}

impl MemoryStore {
    /// All fingerprints as JSON (same as [`Store::export_json`]).
    pub fn to_json(&self) -> String {
        self.export_json()
    }

    /// A store filled from JSON made by [`MemoryStore::to_json`] (or an adaptive store file).
    pub fn from_json(json: &str) -> crate::error::Result<MemoryStore> {
        Ok(MemoryStore { map: Mutex::new(parse_store_json(json)?) })
    }
}

impl Store for MemoryStore {
    fn contains(&self, key: &str) -> bool {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).contains_key(key)
    }
    fn get(&self, key: &str) -> Option<Fingerprint> {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).get(key).cloned()
    }
    fn put(&self, key: &str, fp: &Fingerprint) {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).insert(key.to_string(), fp.clone());
    }
    fn keys(&self) -> Vec<String> {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).keys().cloned().collect()
    }
    fn remove(&self, key: &str) {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).remove(key);
    }
}

/// JSON file store: survives restarts; written atomically (temp file + rename) on every change.
pub struct FileStore {
    path: PathBuf,
    map: Mutex<BTreeMap<String, Fingerprint>>,
}

impl FileStore {
    pub fn open(path: impl Into<PathBuf>) -> crate::error::Result<FileStore> {
        let path = path.into();
        let map = if path.exists() {
            let text = std::fs::read_to_string(&path).map_err(|e| crate::error::Error::Doc(format!("adaptive store {}: {e}", path.display())))?;
            if text.trim().is_empty() {
                BTreeMap::new()
            } else {
                serde_json::from_str(&text).map_err(|e| crate::error::Error::Doc(format!("adaptive store {}: {e}", path.display())))?
            }
        } else {
            BTreeMap::new()
        };
        Ok(FileStore { path, map: Mutex::new(map) })
    }

    fn flush(&self, map: &BTreeMap<String, Fingerprint>) {
        if let Ok(text) = serde_json::to_string_pretty(map) {
            let tmp = self.path.with_extension("tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, &self.path);
            }
        }
    }
}

impl Store for FileStore {
    fn contains(&self, key: &str) -> bool {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).contains_key(key)
    }
    fn get(&self, key: &str) -> Option<Fingerprint> {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).get(key).cloned()
    }
    fn put(&self, key: &str, fp: &Fingerprint) {
        let mut m = self.map.lock().unwrap_or_else(|e| e.into_inner());
        m.insert(key.to_string(), fp.clone());
        self.flush(&m);
    }
    fn keys(&self) -> Vec<String> {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).keys().cloned().collect()
    }
    fn remove(&self, key: &str) {
        let mut m = self.map.lock().unwrap_or_else(|e| e.into_inner());
        if m.remove(key).is_some() {
            self.flush(&m);
        }
    }
}

pub(crate) fn default_store() -> Arc<dyn Store> {
    Arc::new(MemoryStore::default())
}
