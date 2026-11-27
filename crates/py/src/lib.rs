use oxide_parse_core::{available, compile_with, Engine, Error, Expr, FileStore, MemoryStore, Session as CoreSession, Store, Value};
use pyo3::conversion::IntoPyObject;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyByteArray, PyBytes, PyDict, PyFloat, PyList, PyString, PyTuple};
use rayon::prelude::*;
use std::borrow::Cow;
use std::sync::Arc;

pyo3::create_exception!(_core, OxideParseError, pyo3::exceptions::PyException);

fn err(e: Error) -> PyErr {
    OxideParseError::new_err(e.to_string())
}

/// Borrow a Python `str`, `bytes`, or copy a `bytearray` as UTF-8 without copying the source document on `str`/`bytes`.
fn borrow_source<'a>(obj: &'a Bound<'_, PyAny>) -> PyResult<Cow<'a, str>> {
    if let Ok(s) = obj.downcast::<PyString>() {
        return Ok(match s.to_cow() {
            Ok(cow) => cow,
            Err(_) => s.to_string_lossy(),
        });
    }
    if let Ok(b) = obj.downcast::<PyBytes>() {
        return Ok(match std::str::from_utf8(b.as_bytes()) {
            Ok(st) => Cow::Borrowed(st),
            Err(_) => Cow::Owned(String::from_utf8_lossy(b.as_bytes()).into_owned()),
        });
    }
    if let Ok(ba) = obj.downcast::<PyByteArray>() {
        let raw = ba.to_vec();
        return Ok(Cow::Owned(match String::from_utf8(raw) {
            Ok(st) => st,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        }));
    }
    Err(pyo3::exceptions::PyTypeError::new_err(format!(
        "expected str, bytes, or bytearray, got {}",
        obj.get_type().name()?
    )))
}

/// Cache of the (interned) key strings created while converting one result: 10 000 records share the same
/// handful of `PyString`s instead of interning "price" 10 000 times.
type KeyCache<'a, 'py> = std::collections::HashMap<&'a str, Bound<'py, PyString>>;

fn to_py<'py>(py: Python<'py>, v: &Value) -> PyResult<Bound<'py, PyAny>> {
    let mut keys: KeyCache<'_, 'py> = KeyCache::with_capacity(16);
    to_py_cached(py, v, &mut keys)
}

fn to_py_cached<'a, 'py>(py: Python<'py>, v: &'a Value, keys: &mut KeyCache<'a, 'py>) -> PyResult<Bound<'py, PyAny>> {
    Ok(match v {
        Value::Null | Value::Pending(_) => py.None().into_bound(py),
        Value::Bool(b) => PyBool::new(py, *b).to_owned().into_any(),
        Value::Int(i) => i.into_pyobject(py)?.into_any(),
        Value::Float(f) => PyFloat::new(py, *f).into_any(),
        Value::Str(s) => PyString::new(py, s).into_any(),
        Value::List(l) => {
            let mut items = Vec::with_capacity(l.len());
            for x in l {
                items.push(to_py_cached(py, x, keys)?);
            }
            PyList::new(py, items)?.into_any()
        }
        Value::Map(m) => {
            let d = PyDict::new(py);
            for (k, x) in m {
                let val = to_py_cached(py, x, keys)?;
                let key = keys.entry(k.as_str()).or_insert_with(|| PyString::intern(py, k));
                d.set_item(&*key, val)?;
            }
            d.into_any()
        }
    })
}

fn count_values(v: &Value) -> usize {
    match v {
        Value::List(l) => 1 + l.iter().map(count_values).sum::<usize>(),
        Value::Map(m) => 1 + m.values().map(count_values).sum::<usize>(),
        _ => 1,
    }
}

fn parse_meta(meta_json: Option<&str>) -> PyResult<Value> {
    match meta_json {
        None => Ok(Value::Null),
        Some(s) => serde_json::from_str::<serde_json::Value>(s)
            .map(Value::from_json)
            .map_err(|e| OxideParseError::new_err(format!("meta must be JSON-serialisable: {e}"))),
    }
}

/// A compiled extraction plan (immutable, thread-safe, reusable for any number of documents).
#[pyclass(name = "Plan", module = "oxide_parse._core", frozen)]
struct PyPlan {
    engine: Arc<dyn Engine>,
}

#[pymethods]
impl PyPlan {
    #[new]
    #[pyo3(signature = (ir_json, backend = "html", adaptive_store = None, adaptive_state = None))]
    fn new(ir_json: &str, backend: &str, adaptive_store: Option<&str>, adaptive_state: Option<&str>) -> PyResult<Self> {
        let ir = Expr::from_json(ir_json).map_err(err)?;
        let store: Option<Arc<dyn Store>> = match (adaptive_store, adaptive_state) {
            (Some(path), state) => {
                let fs = FileStore::open(path).map_err(err)?;
                if let Some(s) = state {
                    fs.import_json(s).map_err(err)?;
                }
                Some(Arc::new(fs))
            }
            // no file: fingerprints live in memory, optionally preloaded from an earlier `export_adaptive_store()`
            (None, Some(s)) => Some(Arc::new(MemoryStore::from_json(s).map_err(err)?)),
            (None, None) => None,
        };
        let engine = compile_with(&ir, backend, store).map_err(err)?;
        Ok(PyPlan { engine: Arc::from(engine) })
    }

    fn explain(&self) -> String {
        self.engine.explain()
    }

    /// All remembered fingerprints of the `adaptive` selectors as a JSON string.
    fn export_adaptive_store(&self) -> String {
        self.engine.adaptive_store().export_json()
    }

    /// Forget the fingerprints (and the `#healed` ones) of these keys, e.g. the keys whose selector was rewritten.
    /// Works on a store file as well as on the in-memory store.
    fn forget_adaptive_keys(&self, keys: Vec<String>) {
        let store = self.engine.adaptive_store();
        for k in keys {
            store.remove(&oxide_parse_core::adaptive::healed_key(&k));
            store.remove(&k);
        }
    }

    /// Merge fingerprints from `export_adaptive_store()` into this plan; returns how many were loaded.
    fn import_adaptive_store(&self, state: &str) -> PyResult<usize> {
        self.engine.adaptive_store().import_json(state).map_err(err)
    }

    #[getter]
    fn backend(&self) -> &'static str {
        self.engine.backend()
    }

    fn ir_json(&self) -> String {
        self.engine.ir().to_json()
    }

    /// Run the root schema; the GIL is released while parsing and executing.
    #[pyo3(signature = (source, meta_json = None))]
    fn start(&self, py: Python<'_>, source: &Bound<'_, PyAny>, meta_json: Option<&str>) -> PyResult<PySession> {
        let src = borrow_source(source)?;
        let meta = parse_meta(meta_json)?;
        let engine = &self.engine;
        let state = py.allow_threads(|| engine.start(src.as_ref(), meta)).map_err(err)?;
        Ok(PySession { engine: self.engine.clone(), state })
    }

    /// One-shot extraction for schemas without `follow`.
    #[pyo3(signature = (source, meta_json = None))]
    fn extract<'py>(&self, py: Python<'py>, source: &Bound<'_, PyAny>, meta_json: Option<&str>) -> PyResult<Bound<'py, PyAny>> {
        let src = borrow_source(source)?;
        let meta = parse_meta(meta_json)?;
        let engine = &self.engine;
        let v = py.allow_threads(|| engine.extract(src.as_ref(), meta)).map_err(err)?;
        to_py(py, &v)
    }

    /// Parse + extract, but do not build Python objects: returns the number of values in the result.
    /// For benchmarks: `extract` time minus `run_native` time = cost of creating the Python result.
    fn run_native(&self, py: Python<'_>, source: &Bound<'_, PyAny>) -> PyResult<usize> {
        let src = borrow_source(source)?;
        let engine = &self.engine;
        let v = py.allow_threads(|| engine.extract(src.as_ref(), Value::Null)).map_err(err)?;
        Ok(count_values(&v))
    }

    /// Extract many documents on a pool of `threads` threads (default: all cores), GIL released.
    /// `collect_errors=False`: the first failing document raises. `True`: failed documents give `None`
    /// and are listed as `(index, message)`.
    #[pyo3(signature = (sources, threads = None, collect_errors = false))]
    fn extract_batch<'py>(
        &self,
        py: Python<'py>,
        sources: &Bound<'py, PyAny>,
        threads: Option<usize>,
        collect_errors: bool,
    ) -> PyResult<(Bound<'py, PyList>, Vec<(usize, String)>)> {
        let kept: Vec<Bound<'py, PyAny>> = sources.try_iter()?.collect::<PyResult<_>>()?;
        let slices: Vec<Cow<'_, str>> = kept.iter().map(borrow_source).collect::<PyResult<_>>()?;
        let engine = &self.engine;
        let run = || slices.par_iter().map(|s| engine.extract(s.as_ref(), Value::Null)).collect::<Vec<Result<Value, Error>>>();
        let results: Vec<Result<Value, Error>> = py.allow_threads(|| match threads {
            Some(n) => match rayon::ThreadPoolBuilder::new().num_threads(n.max(1)).build() {
                Ok(pool) => pool.install(run),
                Err(_) => run(),
            },
            None => run(),
        });
        let mut items = Vec::with_capacity(results.len());
        let mut errors: Vec<(usize, String)> = Vec::new();
        let mut keys: KeyCache<'_, 'py> = KeyCache::with_capacity(16);
        for (i, r) in results.iter().enumerate() {
            match r {
                Ok(v) => items.push(to_py_cached(py, v, &mut keys)?),
                Err(e) => {
                    if !collect_errors {
                        return Err(err(e.clone()));
                    }
                    errors.push((i, e.to_string()));
                    items.push(py.None().into_bound(py));
                }
            }
        }
        Ok((PyList::new(py, items)?, errors))
    }

    /// Extract many documents in parallel (rayon), GIL released.
    fn extract_many<'py>(&self, py: Python<'py>, sources: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyList>> {
        let kept: Vec<Bound<'py, PyAny>> = sources.try_iter()?.collect::<PyResult<_>>()?;
        let slices: Vec<Cow<'_, str>> = kept.iter().map(borrow_source).collect::<PyResult<_>>()?;
        let engine = &self.engine;
        let results = py
            .allow_threads(|| slices.par_iter().map(|s| engine.extract(s.as_ref(), Value::Null)).collect::<Result<Vec<_>, Error>>())
            .map_err(err)?;
        let mut items = Vec::with_capacity(results.len());
        let mut keys: KeyCache<'_, 'py> = KeyCache::with_capacity(16);
        for v in &results {
            items.push(to_py_cached(py, v, &mut keys)?);
        }
        PyList::new(py, items)
    }
}

/// An in-progress extraction that may be waiting for `follow` documents.
#[pyclass(name = "Session", module = "oxide_parse._core")]
struct PySession {
    engine: Arc<dyn Engine>,
    state: CoreSession,
}

#[pymethods]
impl PySession {
    /// Outstanding requests as `(id, params)` tuples; `params` is the request dict (`url`, `headers`, ...).
    fn pending<'py>(&self, py: Python<'py>) -> PyResult<Vec<(u64, Bound<'py, PyAny>)>> {
        let mut out = Vec::new();
        for r in self.state.pending() {
            out.push((r.id, to_py(py, &Value::Map(r.params.clone()))?));
        }
        Ok(out)
    }

    /// Records quarantined by `map(..., on_error=...)`, as `(path, message)`.
    fn errors(&self) -> Vec<(String, String)> {
        self.state.errors().iter().map(|e| (e.path.clone(), e.message.clone())).collect()
    }

    /// Adaptive selectors that found nothing and could not be relocated, as a JSON array.
    fn misses_json(&self) -> PyResult<String> {
        serde_json::to_string(self.state.misses()).map_err(|e| OxideParseError::new_err(e.to_string()))
    }

    fn done(&self) -> bool {
        self.state.is_done()
    }

    /// Everything the session waits for, as a JSON array: `{"kind": "fetch", "id", "params", "meta", "url"}` for
    /// `follow` documents and `{"kind": "repair", "id", "doc", "url", "key", "css", "fingerprint", "container",
    /// "skeleton", "scope"}` for broken `adaptive` selectors. Answer both with `resume`.
    fn requests_json(&self) -> PyResult<String> {
        let mut out: Vec<serde_json::Value> = Vec::new();
        for r in self.state.pending() {
            out.push(serde_json::json!({
                "kind": "fetch",
                "id": r.id,
                "params": Value::Map(r.params.clone()).to_json(),
                "meta": r.meta.to_json(),
                "url": r.url(),
            }));
        }
        for r in self.state.repairs() {
            let mut v = serde_json::to_value(r).map_err(|e| OxideParseError::new_err(e.to_string()))?;
            if let Some(m) = v.as_object_mut() {
                m.insert("kind".to_string(), serde_json::Value::String("repair".to_string()));
            }
            out.push(v);
        }
        serde_json::to_string(&out).map_err(|e| OxideParseError::new_err(e.to_string()))
    }

    /// The selectors given by repairs so far, as `(key, css)`.
    fn overrides(&self) -> Vec<(String, String)> {
        self.state.overrides().iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Answer requests: `(id, text)`, `(id, (text, meta_json))` for a fetch, `(id, {key: css})` for a repair
    /// (`None` or `{}` declines it). Documents are processed first, then the repairs.
    fn resume(&mut self, py: Python<'_>, items: Vec<(u64, Bound<'_, PyAny>)>) -> PyResult<()> {
        let mut docs: Vec<(u64, String, Option<Value>)> = Vec::new();
        let mut answers: Vec<(u64, oxide_parse_core::IndexMap<String, String>)> = Vec::new();
        for (id, payload) in items {
            if payload.is_none() {
                answers.push((id, oxide_parse_core::IndexMap::new()));
            } else if let Ok(d) = payload.downcast::<PyDict>() {
                let mut m = oxide_parse_core::IndexMap::new();
                for (k, v) in d.iter() {
                    m.insert(k.extract::<String>()?, v.extract::<String>()?);
                }
                answers.push((id, m));
            } else if let Ok(tup) = payload.downcast::<PyTuple>() {
                if tup.len() != 2 {
                    return Err(pyo3::exceptions::PyTypeError::new_err(
                        "resume() fetch tuple must be (document, meta_json)",
                    ));
                }
                let body_obj = tup.get_item(0)?;
                let meta_obj = tup.get_item(1)?;
                let text = borrow_source(&body_obj)?.into_owned();
                let meta_str: Option<String> = meta_obj.extract()?;
                let meta = match meta_str {
                    Some(ref s) => Some(parse_meta(Some(s.as_str()))?),
                    None => None,
                };
                docs.push((id, text, meta));
            } else if let Ok(text) = borrow_source(&payload) {
                docs.push((id, text.into_owned(), None));
            } else {
                return Err(pyo3::exceptions::PyTypeError::new_err(
                    "resume() expects (id, str | bytes), (id, (str | bytes, meta_json)), or (id, dict | None) for a repair",
                ));
            }
        }
        let engine = &self.engine;
        let state = &mut self.state;
        py.allow_threads(|| {
            if !docs.is_empty() {
                engine.resume_with_meta(state, &docs)?;
            }
            if !answers.is_empty() {
                engine.repair(state, &answers)?;
            }
            Ok::<(), Error>(())
        })
        .map_err(err)
    }

    fn result<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let v = self.state.take_result().map_err(err)?;
        to_py(py, &v)
    }
}

/// Engines compiled into this build: `(name, kind, supports_xpath, description)`.
#[pyfunction]
fn engines() -> Vec<(String, String, bool, String)> {
    available().into_iter().map(|e| (e.name.to_string(), e.kind.to_string(), e.xpath, e.description.to_string())).collect()
}

/// Skeleton HTML of a page (scripts/styles/most attributes dropped, text clipped, runs of similar siblings folded).
#[cfg(feature = "lite")]
#[pyfunction]
#[pyo3(signature = (html, keep = 2, max_text = 80, max_chars = 20000))]
fn distill(py: Python<'_>, html: &Bound<'_, PyAny>, keep: usize, max_text: usize, max_chars: usize) -> PyResult<String> {
    let src = borrow_source(html)?;
    let opts = oxide_parse_core::DistillOptions { keep, max_text, max_chars };
    Ok(py.allow_threads(|| oxide_parse_core::distill(src.as_ref(), &opts)))
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    #[cfg(feature = "lite")]
    m.add_function(wrap_pyfunction!(distill, m)?)?;
    m.add_function(wrap_pyfunction!(engines, m)?)?;
    m.add_class::<PyPlan>()?;
    m.add_class::<PySession>()?;
    m.add("OxideParseError", m.py().get_type::<OxideParseError>())?;
    Ok(())
}
