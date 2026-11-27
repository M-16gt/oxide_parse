"""Execution drivers (`Plan`, `Session`), schema validation, and HTML utilities (`distill`, `markdown`).

`Plan` compiles an extraction schema once in Rust and exposes synchronous, async, and Rayon batch execution APIs
with the GIL released during native parsing and extraction.

Examples:
    >>> from oxide_parse import T, compile
    >>> plan = compile({"h1": T.css("h1").text().strip()}, backend="auto")
    >>> plan.engine, plan.extract("<h1> Hello </h1>")
    ('stream', {'h1': 'Hello'})
"""
from __future__ import annotations

import asyncio
import csv
import inspect
import io
import json
import os
import pathlib
import re
from dataclasses import dataclass, field
from typing import Any, Callable, Dict, Iterable, List, Literal, Optional, Sequence, Tuple, Union, overload

from .decompile import decompile
from .expr import to_ir
from .formats import FORMATS

Backend = Union[Literal["html", "auto", "stream", "lite", "scraper", "json", "xml"], str]

try:  # the Rust extension; the DSL/IR/decompiler work without it
    from . import _core
    OxideParseError = _core.OxideParseError
except ImportError:  # pragma: no cover
    _core = None  # type: ignore[assignment]  # Allow the DSL without the optional native module.

    class OxideParseError(Exception):  # type: ignore[no-redef]
        """Exception raised for compilation, extraction, and session errors."""

_OFFLOAD_BYTES = 64 * 1024

_META_CHARSET_RE = re.compile(
    rb"""<meta[^>]+(?:charset\s*=\s*["']?\s*([a-zA-Z0-9._-]+)|http-equiv\s*=\s*["']?content-type["']?[^>]+charset\s*=\s*([a-zA-Z0-9._-]+))""",
    re.IGNORECASE,
)
_CT_CHARSET_RE = re.compile(r"charset\s*=\s*[\"']?\s*([a-zA-Z0-9._-]+)", re.IGNORECASE)
_META_TAG_HINT_RE = re.compile(rb"<\s*meta\b", re.IGNORECASE)
_UTF8_NAMES = {"utf-8", "utf8", "utf_8"}


def _charset_from_content_type(content_type: Optional[str]) -> Optional[str]:
    if not content_type:
        return None
    m = _CT_CHARSET_RE.search(content_type)
    return m.group(1).strip() if m else None


def _detect_html_encoding(
    head: bytes,
    *,
    encoding: Optional[str] = None,
    content_type: Optional[str] = None,
) -> Optional[str]:
    if head.startswith(b"\xef\xbb\xbf"):
        return "utf-8-sig"
    if head.startswith((b"\xff\xfe", b"\xfe\xff")):
        return "utf-16"
    if encoding:
        return encoding.strip()
    ct_enc = _charset_from_content_type(content_type)
    if ct_enc:
        return ct_enc
    if _META_TAG_HINT_RE.search(head):
        m = _META_CHARSET_RE.search(head)
        if m:
            raw = m.group(1) or m.group(2)
            if raw:
                return raw.decode("ascii", errors="ignore").strip()
    return None


def _bytes_need_python_decode(raw: bytes, encoding: Optional[str] = None, content_type: Optional[str] = None) -> bool:
    if encoding is not None or _charset_from_content_type(content_type) is not None:
        return True
    head = raw[:2048]
    if head.startswith((b"\xef\xbb\xbf", b"\xff\xfe", b"\xfe\xff")):
        return True
    detected = _detect_html_encoding(head)
    return detected is not None and detected.lower() not in _UTF8_NAMES


def decode_html(
    data: "str | bytes | bytearray",
    encoding: Optional[str] = None,
    content_type: Optional[str] = None,
) -> str:
    """Decode HTML bytes using BOM, explicit `encoding`, HTTP `Content-Type`, or `<meta charset>`.

    Falls back to UTF-8 with replacement characters when no charset hint is declared or when a declared charset
    name is unknown to Python codecs.

    Args:
        data: HTML text or raw bytes (`str`, `bytes`, or `bytearray`).
        encoding: Optional explicit character encoding override (e.g., `"windows-1251"`).
        content_type: Optional HTTP `Content-Type` header string (e.g., `"text/html; charset=windows-1251"`).

    Returns:
        Decoded HTML string.

    Raises:
        TypeError: If `data` is not `str`, `bytes`, or `bytearray`.

    Examples:
        >>> from oxide_parse import decode_html
        >>> decode_html('<meta charset="windows-1251"><p>Привет</p>'.encode("windows-1251"))
        '<meta charset="windows-1251"><p>Привет</p>'
        >>> decode_html(b"<p>Hi</p>", content_type="text/html; charset=utf-8")
        '<p>Hi</p>'
    """
    if isinstance(data, str):
        return data
    if not isinstance(data, (bytes, bytearray)):
        raise TypeError(f"decode_html() expects str, bytes, or bytearray, got {type(data).__name__}")
    raw = bytes(data)
    enc = _detect_html_encoding(raw[:2048], encoding=encoding, content_type=content_type) or "utf-8"
    try:
        return raw.decode(enc, errors="replace")
    except LookupError:
        return raw.decode("utf-8", errors="replace")


def _is_response_like(obj: Any) -> bool:
    return (
        not isinstance(obj, (dict, list, tuple, str, bytes, bytearray))
        and hasattr(obj, "headers")
        and (hasattr(obj, "content") or hasattr(obj, "text"))
    )


def _normalize_headers(raw_headers: Any) -> Dict[str, str]:
    items = raw_headers.items() if hasattr(raw_headers, "items") else raw_headers
    out: Dict[str, str] = {}
    for k, v in items:
        key = str(k).lower()
        val = str(v)
        out[key] = f"{out[key]}, {val}" if key in out else val
    return out


def _unpack_envelope(
    payload: Any,
    *,
    meta: Optional[dict] = None,
    encoding: Optional[str] = None,
    is_json: bool = False,
) -> Tuple[Any, Optional[dict]]:
    body: Any = payload
    resp_meta: Dict[str, Any] = {}
    resp_enc: Optional[str] = encoding

    if isinstance(payload, tuple) and len(payload) == 2 and isinstance(payload[1], dict):
        body, raw_meta = payload
        resp_meta = dict(raw_meta)
        if isinstance(resp_meta.get("headers"), dict):
            resp_meta["headers"] = _normalize_headers(resp_meta["headers"])
    elif _is_response_like(payload):
        norm_headers = _normalize_headers(payload.headers)
        resp_meta = {"headers": norm_headers}
        status = getattr(payload, "status_code", getattr(payload, "status", None))
        if status is not None:
            resp_meta["status"] = status
        url = getattr(payload, "url", None)
        if isinstance(url, str) and url:
            resp_meta["base_url"] = url
        raw_content = getattr(payload, "content", None)
        if isinstance(raw_content, (bytes, bytearray)):
            body = bytes(raw_content)
            if resp_enc is None:
                obj_enc = getattr(payload, "encoding", None)
                if isinstance(obj_enc, str) and obj_enc:
                    resp_enc = obj_enc
        else:
            body = payload.text

    merged_meta: Optional[dict]
    if resp_meta and meta:
        merged_meta = {**resp_meta, **meta}
    elif resp_meta:
        merged_meta = resp_meta
    else:
        merged_meta = meta

    headers = merged_meta.get("headers") if isinstance(merged_meta, dict) else None
    content_type = headers.get("content-type") if isinstance(headers, dict) else None

    if isinstance(body, (bytes, bytearray)):
        if is_json:
            ct_enc = resp_enc or _charset_from_content_type(content_type)
            if ct_enc:
                body = bytes(body).decode(ct_enc, errors="replace")
        elif _bytes_need_python_decode(bytes(body), encoding=resp_enc, content_type=content_type):
            body = decode_html(body, encoding=resp_enc, content_type=content_type)

    return body, merged_meta


def _need_core():
    if _core is None:
        raise ImportError("oxide_parse native extension is not built; run `maturin develop --release`")


@dataclass(frozen=True)
class Detailed:
    """Extraction result bundled with quarantined record errors, unresolved adaptive misses, and repairs.

    Examples:
        >>> from oxide_parse import T, compile
        >>> plan = compile({"v": T.css("b").map(T.text().int(), on_error="skip")})
        >>> det = plan.extract_detailed("<b>1</b><b>oops</b><b>3</b>")
        >>> det.data, det.errors[0]["path"]
        ({'v': [1, 3]}, '$.v[1]')
    """

    data: Any
    """Extracted Python data."""
    errors: List[Dict[str, str]]
    """Records quarantined by `map(..., on_error="skip" | "null")`, each with `path` and `message`."""
    misses: List[Dict[str, Any]]
    """Adaptive selectors that matched nothing and could not relocate (`{"key", "css", "fingerprint"}`)."""
    repairs: Dict[str, str] = field(default_factory=dict)
    """Selectors supplied by `repair` callbacks or `Session.resume` (`{key: css}`)."""


_REPAIR_TASK = (
    "`css` is an adaptive selector (`key`) that no longer matches. `skeleton` shows the container it used to live in "
    "(`scope: container`) or, if that container was not found, the page (`scope: page`), as it looks now; "
    "`fingerprint` describes the old element and its `samples` are values the selector used to return. Answer with "
    "a new CSS selector for `key`, as {key: css}, or None / {} to give up."
)


_BATCH_TASK = (
    "Several adaptive selectors broke on the same page area. `skeleton` shows it as it looks now (`scope`: the container "
    "they used to live in, or the whole page). For every entry of `fields` (`key`, old `css`, `fingerprint` with "
    "`samples`, `previous_error` if your last selector did not parse) answer with a new CSS selector: {key: css}. "
    "Leave a key out to give up on it."
)


def _group_repairs(ir: Any, repairs: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    """Group repair requests by shared container skeleton so broken card fields go in one prompt."""
    groups: Dict[Tuple[Any, Any, Any], Dict[str, Any]] = {}
    for r in repairs:
        gk = (r.get("doc"), r.get("scope"), r.get("container") or hash(r.get("skeleton") or ""))
        g = groups.setdefault(
            gk,
            {
                "doc": r.get("doc"),
                "url": r.get("url"),
                "scope": r.get("scope"),
                "container": r.get("container"),
                "skeleton": r.get("skeleton"),
                "task": _BATCH_TASK,
                "schema": ir,
                "fields": [],
            },
        )
        g["fields"].append(
            {k: r.get(k) for k in ("id", "key", "css", "fingerprint", "previous_error") if r.get(k) is not None}
        )
    return list(groups.values())


class Session:
    """Zero-IO extraction state machine for crawlers mixing network fetches (`follow`/`paginate`) and selector repairs.

    Examples:
        >>> from oxide_parse import T, compile, follow
        >>> pages = {"https://ex.test/1": "<b>7</b>"}
        >>> plan = compile(follow(T.css("a").attr("href").url(), T.css("b").text().int()))
        >>> sess = plan.start("<a href='/1'>x</a>", meta={"base_url": "https://ex.test"})
        >>> while not sess.done:
        ...     sess.resume([(r["id"], pages[r["params"]["url"]]) for r in sess.pending()])
        >>> sess.result()
        7
    """

    def __init__(self, plan: "Plan", core: Any):
        """Wrap a native `_core.Session` handle produced by `Plan.start()`.

        Args:
            plan: Parent compiled `Plan` owning this session.
            core: Underlying native `_core.Session` object.

        Examples:
            >>> from oxide_parse import T, compile
            >>> sess = compile({"h": T.css("h1").text()}).start("<h1>Hi</h1>")
            >>> sess.done
            True
        """
        self._plan = plan
        self._core = core
        self._attempts: Dict[Tuple[Any, str], int] = {}

    def requests(self) -> List[Dict[str, Any]]:
        """Return all open requests: `"fetch"` for `follow`/`paginate` and `"repair"` for broken `adaptive` selectors.

        Returns:
            List of request dictionaries with `"kind"` set to `"fetch"` or `"repair"`.

        Examples:
            >>> from oxide_parse import T, compile, follow
            >>> sess = compile(follow(T.css("a").attr("href"), T.text())).start("<a href='/x'>x</a>")
            >>> sess.requests()[0]["kind"]
            'fetch'
        """
        out = json.loads(self._core.requests_json())
        for r in out:
            if r["kind"] == "repair":
                r["task"] = _REPAIR_TASK
                r["schema"] = self._plan._ir
        return out

    def repair_batches(self) -> List[Dict[str, Any]]:
        """Group open repair requests by shared container (`doc`, `scope`, `container`).

        Answer each field in `batch["fields"]` as `(field["id"], {field["key"]: css})` via `resume()`.

        Returns:
            List of grouped repair batch dictionaries sharing one distilled `skeleton`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> p = compile({"a": T.css(".a", adaptive="a", threshold=0.99).text()}, backend="lite")
            >>> _ = p.extract("<div><i class='a'>1</i></div>")
            >>> batches = p.start("<div><u>1</u></div>").repair_batches()
            >>> len(batches), batches[0]["fields"][0]["key"]
            (1, 'a')
        """
        return _group_repairs(self._plan._ir, self.repairs())

    def pending(self) -> List[Dict[str, Any]]:
        """Return only open network fetch requests (`kind == "fetch"`).

        Returns:
            List of fetch request dictionaries (`{"kind": "fetch", "id", "params", "meta", "url"}`).

        Examples:
            >>> from oxide_parse import T, compile, follow
            >>> sess = compile(follow(T.css("a").attr("href"), T.text())).start("<a href='/next'>n</a>")
            >>> sess.pending()[0]["url"]
            '/next'
        """
        return [r for r in self.requests() if r["kind"] == "fetch"]

    def repairs(self) -> List[Dict[str, Any]]:
        """Return only open adaptive selector repair requests (`kind == "repair"`).

        Returns:
            List of repair request dictionaries.

        Examples:
            >>> from oxide_parse import T, compile
            >>> p = compile({"a": T.css(".a", adaptive="a", threshold=0.99).text()}, backend="lite")
            >>> _ = p.extract("<i class='a'>1</i>")
            >>> p.start("<u>1</u>").repairs()[0]["key"]
            'a'
        """
        return [r for r in self.requests() if r["kind"] == "repair"]

    @property
    def done(self) -> bool:
        """Whether all fetch requests have been answered (open selector repairs are optional).

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("h1").text()).start("<h1>Hi</h1>").done
            True
        """
        return bool(self._core.done())

    @property
    def overrides(self) -> Dict[str, str]:
        """Selectors supplied by repairs so far (`{key: css}`).

        Examples:
            >>> from oxide_parse import T, compile
            >>> p = compile({"a": T.css(".a", adaptive="a", threshold=0.99).text()}, backend="lite")
            >>> _ = p.extract("<i class='a'>1</i>")
            >>> sess = p.start("<u>1</u>")
            >>> sess.resume([(sess.repairs()[0]["id"], {"a": "u"})])
            >>> sess.overrides
            {'a': 'u'}
        """
        return dict(self._core.overrides())

    def resume(self, answers: Any) -> None:
        """Submit answers for open requests.

        Args:
            answers: Iterable of `(request_id, document)` fetches (`str`, `bytes`, `bytearray`, `(doc, meta_dict)`, or
                duck-typed HTTP `Response`), `(request_id, {key: css})` repairs, or `(request_id, None)` declines.

        Examples:
            >>> from oxide_parse import T, compile, follow
            >>> sess = compile(follow(T.css("a").attr("href"), T.css("b").text())).start("<a href='/1'>x</a>")
            >>> req_id = sess.pending()[0]["id"]
            >>> sess.resume([(req_id, "<b>ok</b>")])
            >>> sess.result()
            'ok'
        """
        self._core.resume([(i, self._payload(p)) for i, p in answers])

    async def aresume(self, answers: Any) -> None:
        """Async variant of `resume()` that offloads large payloads (>= 64 KiB) to a worker thread.

        Args:
            answers: Iterable of `(request_id, payload)` tuples identical to `resume()`.

        Examples:
            >>> await sess.aresume([(req_id, html_bytes)])  # doctest: +SKIP
        """
        items = [(i, self._payload(p)) for i, p in answers]
        size = sum(len(p[0] if isinstance(p, tuple) else p) for _, p in items if isinstance(p[0] if isinstance(p, tuple) else p, (str, bytes, bytearray)))
        await _maybe_thread(self._core.resume, size, items)

    def _payload(self, p: Any, encoding: Optional[str] = None) -> Any:
        if p is None or isinstance(p, dict):
            return p
        body, extra_meta = _unpack_envelope(p, encoding=encoding, is_json=(self._plan._backend == "json"))
        if isinstance(body, (dict, list)):
            body = json.dumps(body)
        else:
            body = self._plan._src(body)
        if extra_meta:
            return (body, json.dumps(extra_meta))
        return body

    def result(self) -> Any:
        """Return the completed extraction result once all pending fetch requests are resolved.

        Returns:
            Extracted Python object/list/scalar.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("b").text()).start("<b>ok</b>").result()
            'ok'
        """
        return self._core.result()

    def detailed(self) -> Detailed:
        """Return `result()` together with quarantined errors, unresolved misses, and repair overrides.

        Returns:
            `Detailed` dataclass instance.

        Examples:
            >>> from oxide_parse import T, compile
            >>> det = compile(T.css("b").map(T.text().int(), on_error="skip")).start("<b>1</b><b>x</b>").detailed()
            >>> det.data, len(det.errors)
            ([1], 1)
        """
        errors = [{"path": p, "message": m} for p, m in self._core.errors()]
        misses = json.loads(self._core.misses_json())
        overrides = self.overrides
        return Detailed(self._core.result(), errors, misses, overrides)


@dataclass(frozen=True)
class RepairResult:
    """Result returned by `Plan.extract_or_repair()`.

    Examples:
        >>> from oxide_parse import T, compile
        >>> p = compile({"x": T.css(".old", adaptive="x", threshold=0.99).text()}, backend="lite")
        >>> _ = p.extract("<div><i class='old'>7</i></div>")
        >>> res = p.extract_or_repair("<div><u>7</u></div>", repair=lambda r: {r["key"]: "u"})
        >>> res.repaired, res.data
        (True, {'x': '7'})
    """

    data: Any
    """Extracted Python data after any applied repairs."""
    plan: "Plan"
    """Resulting `Plan` (a patched copy with rewritten selectors when `repaired` is `True`)."""
    repaired: bool
    """Whether one or more selectors were rewritten during this call."""
    errors: List[Dict[str, str]]
    """Records quarantined by `map(..., on_error="skip" | "null")`."""
    misses: List[Dict[str, Any]]
    """Adaptive selectors that remained unresolved."""


def distill(
    html: "str | bytes | bytearray",
    keep: int = 2,
    max_text: int = 80,
    max_chars: int = 20000,
    *,
    encoding: Optional[str] = None,
) -> str:
    """Shrink an HTML document into a compact structural skeleton for prompts or inspection.

    Strips `<script>`, `<style>`, `<svg>`, and noisy attributes, truncates long text nodes, and folds repeated similar
    sibling elements into `<!-- +N similar ... -->` notes. Requires the `lite` engine in the native build.

    Args:
        html: HTML source (`str`, `bytes`, or `bytearray`).
        keep: Number of similar consecutive siblings kept before folding.
        max_text: Maximum characters kept per text node.
        max_chars: Hard character cap on the returned skeleton.
        encoding: Optional explicit character encoding override for byte inputs.

    Returns:
        Compact HTML skeleton string.

    Raises:
        ImportError: If built without the `lite` feature.

    Examples:
        >>> from oxide_parse import distill
        >>> html = "<div><script>alert(1)</script>" + "<p class='i'>row</p>" * 5 + "</div>"
        >>> out = distill(html)
        >>> "alert" not in out and "+3 similar" in out
        True
    """
    _need_core()
    fn = getattr(_core, "distill", None)
    if fn is None:
        raise ImportError("distill() needs the `lite` engine: maturin develop --release --features lite")
    if isinstance(html, (bytes, bytearray)) and _bytes_need_python_decode(bytes(html), encoding=encoding):
        html = decode_html(html, encoding=encoding)
    return str(fn(html, keep, max_text, max_chars))


def ir_schema() -> Dict[str, Any]:
    """Return the bundled Draft 2020-12 JSON Schema for the `oxide_parse` IR format.

    Returns:
        JSON Schema dictionary.

    Examples:
        >>> from oxide_parse import ir_schema
        >>> ir_schema()["title"]
        'oxide_parse IR'
    """
    import pathlib

    return json.loads(pathlib.Path(__file__).with_name("ir.schema.json").read_text(encoding="utf-8"))


def validate_ir(ir: Any, backend: Backend = "auto") -> List[str]:
    """Validate a schema or IR without stopping on the first error, returning every issue with its path.

    Accepts a raw IR dict, a JSON string, or a DSL schema containing `T` expressions. Checks structural rules in Python
    and, when the native extension is loaded, compiles against `backend` to catch invalid CSS/XPath or unsupported ops.
    Never raises.

    Args:
        ir: Schema (`dict`, `list`, `Expr`), IR dict, or IR JSON string.
        backend: Target engine (`"auto"`, `"html"`, `"stream"`, `"lite"`, `"scraper"`, `"json"`).

    Returns:
        List of human-readable error strings (`$.items[1].map.price: ...`), or `[]` when valid.

    Examples:
        >>> from oxide_parse import T, validate_ir
        >>> validate_ir({"title": T.css("h1").text()})
        []
        >>> validate_ir({"object": {"x": {"pipe": ["bogus"]}}})
        ["$.x[0]: unknown operation 'bogus'"]
    """
    from . import ir_spec
    from .expr import Expr, to_ir

    def contains_expr(value: Any) -> bool:
        if isinstance(value, Expr):
            return True
        if isinstance(value, dict):
            return any(contains_expr(item) for item in value.values())
        if isinstance(value, (list, tuple)):
            return any(contains_expr(item) for item in value)
        return False

    value: Any = ir
    if isinstance(value, str):
        try:
            value = json.loads(value)
        except (ValueError, RecursionError) as e:
            return [f"$: not valid JSON ({e})"]
    try:
        if contains_expr(value):
            value = to_ir(value)
    except Exception as e:
        return [f"$: {e}"]
    errors = ir_spec.validate_structure(value)
    if errors:
        return errors
    try:
        _need_core()
    except Exception:
        return []  # No native module available: structural check is complete.
    try:
        Plan(value, backend, _ir=True)
    except Exception as e:
        return [f"$: {e}"]
    return []


def markdown(
    html: "str | bytes | bytearray",
    selector: str = "body",
    *,
    without: Optional[str] = None,
    main: bool = False,
    backend: Backend = "html",
    encoding: Optional[str] = None,
) -> str:
    """Convert an HTML document (or element matching `selector`) into Markdown for LLM/RAG pipelines.

    Requires a DOM engine (`html`, `lite`, or `scraper`). Returns `""` if `selector` matches nothing.

    Args:
        html: HTML source document (`str`, `bytes`, or `bytearray`).
        selector: CSS selector of the root element to render (default `"body"`). Ignored when `main=True`.
        without: Optional CSS selector of subtrees to exclude (e.g., `".ad, .share"`).
        main: When `True`, locate the primary article container automatically via `T.main_content()`.
        backend: Engine used for rendering (default `"html"`).
        encoding: Optional character encoding override when `html` is bytes.

    Returns:
        Rendered Markdown string.

    Examples:
        >>> from oxide_parse import markdown
        >>> markdown("<article><h1>Title</h1><div class='ad'>Ad</div><p>Body</p></article>", "article", without=".ad")
        '# Title\\n\\nBody'
    """
    from .expr import T

    target = T.main_content() if main else T.css(selector)
    out = Plan(target.markdown(without=without), backend).extract(html, encoding=encoding)
    return "" if out is None else str(out)


def _replace_adaptive(node: Any, mapping: Dict[str, str], seen: set) -> Any:
    if isinstance(node, dict):
        out: Dict[str, Any] = {}
        for k, v in node.items():
            if k == "adaptive" and isinstance(v, dict) and v.get("key") in mapping:
                seen.add(v["key"])
                v = {**v, "css": mapping[v["key"]]}
            out[k] = _replace_adaptive(v, mapping, seen)
        return out
    if isinstance(node, list):
        return [_replace_adaptive(x, mapping, seen) for x in node]
    return node


@dataclass(frozen=True)
class EngineInfo:
    """Metadata describing an extraction engine compiled into the native extension.

    Examples:
        >>> from oxide_parse import engines
        >>> any(e.name == "lite" and e.xpath for e in engines())
        True
    """

    name: str
    """Backend identifier (`"scraper"`, `"lite"`, `"stream"`, or `"json"`)."""
    kind: str
    """Execution model: `"dom"`, `"stream"`, or `"data"`."""
    xpath: bool
    """Whether XPath 1.0 queries are supported."""
    description: str
    """Short summary of the engine."""


def engines() -> List[EngineInfo]:
    """List all extraction engines compiled into the active native extension.

    Returns:
        List of `EngineInfo` records.

    Examples:
        >>> from oxide_parse import engines
        >>> "json" in {e.name for e in engines()}
        True
    """
    _need_core()
    return [EngineInfo(*e) for e in _core.engines()]


class Plan:
    """Compiled, immutable extraction plan built once from a schema and reused across documents.

    Examples:
        >>> from oxide_parse import Plan, T
        >>> plan = Plan({"title": T.css("h1").text().strip()}, backend="auto")
        >>> plan.extract("<h1> Shop </h1>")
        {'title': 'Shop'}
    """

    def __init__(
        self,
        schema: Any,
        backend: Backend = "html",
        *,
        adaptive_store: "Optional[str | os.PathLike[str]]" = None,
        adaptive_state: Optional[str] = None,
        _ir: bool = False,
    ):
        """Compile `schema` into an immutable extraction plan.

        Args:
            schema: DSL `T` expression, nested dict/list schema, or serialized IR when `_ir=True`.
            backend: Target backend (`"html"`, `"auto"`, `"stream"`, `"lite"`, `"scraper"`, or `"json"`).
            adaptive_store: Optional path to a JSON file persisting `adaptive` selector fingerprints across runs.
            adaptive_state: Optional JSON string from `export_adaptive_store()` for stateless environments (Redis, S3).
            _ir: Internal flag indicating `schema` is already IR (prefer `Plan.from_ir()`).

        Examples:
            >>> from oxide_parse import Plan, T
            >>> plan = Plan({"n": T.css("b").text().int()}, backend="auto")
            >>> plan.engine
            'stream'
        """
        _need_core()
        self._ir = schema if _ir else to_ir(schema)
        self._backend = backend
        store = None if adaptive_store is None else os.fspath(adaptive_store)
        self._adaptive_store = store
        self._core = _core.Plan(json.dumps(self._ir), backend, store, adaptive_state)

    # -- persistence -------------------------------------------------------
    @property
    def ir(self) -> dict:
        """Compiled schema represented as a JSON-serializable IR dictionary.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("h1").text()).ir
            {'pipe': [{'css': 'h1'}, 'text']}
        """
        return self._ir

    def dumps(self, format: str = "json", **options: Any) -> str:
        """Serialize this plan's compiled IR using any codec registered in `FORMATS`.

        Built-in formats are `"json"`, `"yaml"` (`"yml"`), `"toml"`, and `"python"` (`"py"`). Additional custom
        formats can be added with `oxide_parse.register_format()`.

        Args:
            format: Registered format name, alias, or filename extension (default `"json"`).
            **options: Keyword options forwarded to the codec's serializer (e.g., `indent=2`, `name="my_schema"`).

        Returns:
            Serialized schema representation string.

        Examples:
            >>> from oxide_parse import T, compile
            >>> p = compile({"title": T.css("h1").text()})
            >>> raw = p.dumps("toml")
            >>> Plan.loads(raw, "toml").ir == p.ir
            True
        """
        return FORMATS.dumps(self._ir, format, **options)

    def dump(
        self,
        target: Union[str, os.PathLike[str], Any],
        *,
        format: Optional[str] = None,
        encoding: str = "utf-8",
        **options: Any,
    ) -> None:
        """Serialize and write this plan's IR to a file path or writable stream via `FORMATS`.

        Args:
            target: Destination file path (`str`/`PathLike`) or writable text object implementing `.write(...)`.
            format: Format name override; when `None`, inferred from `target` suffix (or `"json"` for streams).
            encoding: Text encoding when writing to a filesystem path (default `"utf-8"`).
            **options: Keyword options forwarded to `Plan.dumps()`.

        Examples:
            >>> import io
            >>> from oxide_parse import T, compile
            >>> buf = io.StringIO()
            >>> compile({"h": T.css("h1").text()}).dump(buf, format="toml")
            >>> "css" in buf.getvalue() and "h1" in buf.getvalue()
            True
        """
        if hasattr(target, "write"):
            target.write(self.dumps(format or "json", **options))
            return
        fmt = format or FORMATS.for_path(target).name
        pathlib.Path(target).write_text(self.dumps(fmt, **options), encoding=encoding)

    def to_json(self, indent: Optional[int] = None) -> str:
        """Serialize the compiled schema IR as a JSON string.

        Args:
            indent: Optional indentation width passed to `json.dumps`.

        Returns:
            JSON-serialized IR string.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("h1").text()).to_json()
            '{"pipe": [{"css": "h1"}, "text"]}'
        """
        return self.dumps("json", indent=indent)

    def to_yaml(self) -> str:
        """Serialize the compiled schema IR as YAML (requires `pyyaml`).

        Returns:
            YAML-formatted IR string.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("h1").text()).to_yaml()  # doctest: +SKIP
            'pipe:\\n- css: h1\\n- text\\n'
        """
        return self.dumps("yaml")

    def to_toml(self) -> str:
        """Serialize the compiled schema IR as a TOML string.

        Uses `tomli-w` when installed or the built-in pure-Python TOML writer, encoding `None` leaves losslessly.

        Returns:
            TOML-formatted IR string.

        Examples:
            >>> from oxide_parse import T, compile
            >>> "css" in compile(T.css("h1").text()).to_toml()
            True
        """
        return self.dumps("toml")

    def to_python(self, name: str = "schema") -> str:
        """Decompile this plan back into runnable Python DSL source code.

        Args:
            name: Variable name assigned in generated code (default `"schema"`).

        Returns:
            Python code defining `name`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> "T.css('h1').text()" in compile({"t": T.css("h1").text()}).to_python()
            True
        """
        return self.dumps("python", name=name)

    @classmethod
    def from_ir(cls, ir: Dict[str, Any], backend: Backend = "html", **kw: Any) -> "Plan":
        """Compile a serialized IR dictionary directly without running `to_ir()`.

        Args:
            ir: IR dictionary produced by `to_ir()` or loaded from JSON/YAML/TOML.
            backend: Target backend engine name.
            **kw: Additional `Plan` keyword options (`adaptive_store`, `adaptive_state`).

        Returns:
            Compiled `Plan`.

        Examples:
            >>> from oxide_parse import Plan
            >>> Plan.from_ir({"pipe": [{"css": "h1"}, "text"]}).extract("<h1>OK</h1>")
            'OK'
        """
        return cls(ir, backend, _ir=True, **kw)

    @classmethod
    def loads(
        cls,
        text: Union[str, bytes, bytearray],
        format: str = "json",
        backend: Backend = "html",
        *,
        format_options: Optional[Dict[str, Any]] = None,
        **kw: Any,
    ) -> "Plan":
        """Deserialize and compile a plan from any codec registered in `FORMATS`.

        Args:
            text: Serialized IR (`str` or UTF-8 `bytes`) in `format`.
            format: Registered format name, alias, or filename extension (default `"json"`).
            backend: Target backend engine name.
            format_options: Optional keyword arguments forwarded to the codec's `loads` callback.
            **kw: Additional `Plan` keyword options (`adaptive_store`, `adaptive_state`).

        Returns:
            Compiled `Plan`.

        Raises:
            ValueError: If `format` is unknown or deserialized payload is not an IR dict.

        Examples:
            >>> from oxide_parse import Plan, T, compile
            >>> raw = compile({"t": T.css("h1").text()}).dumps("toml")
            >>> Plan.loads(raw, "toml").extract("<h1>OK</h1>")
            {'t': 'OK'}
        """
        ir = FORMATS.loads(text, format, **(format_options or {}))
        if not isinstance(ir, dict):
            raise ValueError(f"expected schema IR dict from {format!r}, got {type(ir).__name__}")
        return cls.from_ir(ir, backend, **kw)

    @classmethod
    def load(
        cls,
        source: Union[str, os.PathLike[str], Any],
        *,
        format: Optional[str] = None,
        backend: Backend = "html",
        encoding: str = "utf-8",
        format_options: Optional[Dict[str, Any]] = None,
        **kw: Any,
    ) -> "Plan":
        """Load and compile a serialized schema from a file path or readable stream via `FORMATS`.

        Args:
            source: Path (`str`/`PathLike`) to a schema file or readable file-like object implementing `.read()`.
            format: Format name override; when `None`, inferred from filename suffix (or `"json"` for streams).
            backend: Target backend engine name.
            encoding: Text encoding when reading from a path (default `"utf-8"`).
            format_options: Optional keyword arguments forwarded to the codec's `loads` callback.
            **kw: Additional `Plan` keyword options (`adaptive_store`, `adaptive_state`).

        Returns:
            Compiled `Plan`.

        Examples:
            >>> import io
            >>> from oxide_parse import Plan, T, compile
            >>> buf = io.StringIO(compile({"t": T.css("h1").text()}).dumps("toml"))
            >>> Plan.load(buf, format="toml").extract("<h1>OK</h1>")
            {'t': 'OK'}
        """
        opts = dict(format_options or {})
        if hasattr(source, "read"):
            raw = source.read()
            fmt = format or "json"
        else:
            path_obj = pathlib.Path(source)
            fmt = format or FORMATS.for_path(path_obj).name
            raw = path_obj.read_text(encoding=encoding)
            if FORMATS.get(fmt).name == "python" and "filename" not in opts:
                opts["filename"] = str(path_obj)
        return cls.loads(raw, fmt, backend, format_options=opts, **kw)

    @classmethod
    def from_json(cls, text: str, backend: Backend = "html", **kw: Any) -> "Plan":
        """Compile a plan from a JSON-encoded IR string.

        Args:
            text: JSON IR string produced by `Plan.to_json()`.
            backend: Target backend engine name.
            **kw: Additional `Plan` keyword options.

        Returns:
            Compiled `Plan`.

        Examples:
            >>> from oxide_parse import Plan
            >>> Plan.from_json('{"pipe": [{"css": "h1"}, "text"]}').extract("<h1>OK</h1>")
            'OK'
        """
        return cls.loads(text, "json", backend, **kw)

    @classmethod
    def from_yaml(cls, text: str, backend: Backend = "html", **kw: Any) -> "Plan":
        """Compile a plan from a YAML-encoded IR string (requires `pyyaml`).

        Args:
            text: YAML IR text.
            backend: Target backend engine name.
            **kw: Additional `Plan` keyword options.

        Returns:
            Compiled `Plan`.

        Examples:
            >>> Plan.from_yaml("pipe:\\n- css: h1\\n- text\\n")  # doctest: +SKIP
        """
        return cls.loads(text, "yaml", backend, **kw)

    @classmethod
    def from_toml(cls, text: str, backend: Backend = "html", **kw: Any) -> "Plan":
        """Compile a plan from a TOML-encoded IR string.

        Args:
            text: TOML IR text produced by `Plan.to_toml()` or `Plan.dumps("toml")`.
            backend: Target backend engine name.
            **kw: Additional `Plan` keyword options.

        Returns:
            Compiled `Plan`.

        Examples:
            >>> from oxide_parse import Plan, T, compile
            >>> toml_text = compile(T.css("h1").text()).to_toml()
            >>> Plan.from_toml(toml_text).extract("<h1>OK</h1>")
            'OK'
        """
        return cls.loads(text, "toml", backend, **kw)

    @classmethod
    def from_python(cls, code: str, backend: Backend = "html", *, name: Optional[str] = None, **kw: Any) -> "Plan":
        """Compile a plan directly from Python DSL source defining `schema` (or `name`).

        Args:
            code: Python source string defining `schema` (`to_python()` output or hand-written DSL).
            backend: Target backend engine name.
            name: Optional variable name to extract (defaults to `"schema"` / `"SCHEMA"`).
            **kw: Additional `Plan` keyword options.

        Returns:
            Compiled `Plan`.

        Examples:
            >>> from oxide_parse import Plan
            >>> Plan.from_python("from oxide_parse import T\\nschema = {'t': T.css('h1').text()}").extract("<h1>Hi</h1>")
            {'t': 'Hi'}
        """
        opts = {"name": name} if name is not None else None
        return cls.loads(code, "python", backend, format_options=opts, **kw)

    @classmethod
    def from_file(cls, path: Union[str, os.PathLike[str]], backend: Backend = "html", **kw: Any) -> "Plan":
        """Load and compile a serialized schema file (`.json`, `.yaml`/`.yml`, `.toml`, `.py`, or custom format).

        Args:
            path: Path to the schema file on disk.
            backend: Target backend engine name.
            **kw: Additional `Plan` keyword options.

        Returns:
            Compiled `Plan`.

        Examples:
            >>> plan = Plan.from_file("schema.toml", backend="auto")  # doctest: +SKIP
        """
        return cls.load(path, backend=backend, **kw)

    @property
    def engine(self) -> str:
        """Resolved execution engine name (e.g. `backend="auto"` resolves to `"stream"`, `"lite"`, or `"scraper"`).

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile({"h1": T.css("h1").text()}, backend="auto").engine
            'stream'
        """
        return str(self._core.backend)

    def explain(self) -> str:
        """Return a human-readable execution plan showing selectors, peephole optimizations, and streaming passes.

        Returns:
            Multi-line plan explanation string.

        Examples:
            >>> from oxide_parse import T, compile
            >>> "select_first" in compile(T.css("h1").text(), backend="lite").explain()
            True
        """
        return str(self._core.explain())

    # -- helpers -----------------------------------------------------------
    def _src(self, source: Any, encoding: Optional[str] = None) -> Any:
        if isinstance(source, str):
            return source
        if isinstance(source, (bytes, bytearray)):
            if self._backend != "json" and _bytes_need_python_decode(bytes(source), encoding=encoding):
                return decode_html(source, encoding=encoding)
            if self._backend == "json" and encoding is not None:
                return bytes(source).decode(encoding, errors="replace")
            return source
        if self._backend == "json":
            return json.dumps(source)
        raise TypeError(f"{self._backend} backend expects str/bytes, got {type(source).__name__}")

    def _prepare_source(
        self,
        source: Any,
        meta: Optional[dict],
        encoding: Optional[str],
    ) -> Tuple[Any, Optional[dict]]:
        body, merged_meta = _unpack_envelope(
            source,
            meta=meta,
            encoding=encoding,
            is_json=(self._backend == "json"),
        )
        return self._src(body, encoding=encoding), merged_meta

    @staticmethod
    def _meta(meta: Optional[dict]) -> Optional[str]:
        return None if meta is None else json.dumps(meta)

    # -- sync --------------------------------------------------------------
    def start(self, source: Any, *, meta: Optional[dict] = None, encoding: Optional[str] = None) -> Session:
        """Begin a stateful `Session` for schemas using `follow`, `paginate`, or manual `adaptive` repair.

        Args:
            source: Document (`str`, `bytes`, `bytearray`, `(doc, meta_dict)`, HTTP `Response`, or JSON object when
                `backend="json"`).
            meta: Optional metadata dictionary accessible via `T.meta(...)` and `.url()`.
            encoding: Optional character encoding override when `source` is bytes.

        Returns:
            Initialized `Session`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> sess = compile({"h1": T.css("h1").text()}).start("<h1>Go</h1>")
            >>> sess.done, sess.result()
            (True, {'h1': 'Go'})
        """
        src, merged_meta = self._prepare_source(source, meta, encoding)
        return Session(self, self._core.start(src, self._meta(merged_meta)))

    def extract(
        self,
        source: Any,
        *,
        meta: Optional[dict] = None,
        fetch: Optional[Callable[..., Any]] = None,
        repair: Optional[Callable[[Dict[str, Any]], Optional[Dict[str, str]]]] = None,
        repair_batch: Optional[Callable[[Dict[str, Any]], Optional[Dict[str, str]]]] = None,
        encoding: Optional[str] = None,
    ) -> Any:
        """Extract data from a single document (with the GIL released during native parsing/execution).

        Args:
            source: HTML `str`/`bytes`/`bytearray`, HTTP `Response`/`(body, meta)` envelope, or JSON data on
                `backend="json"`.
            meta: Optional metadata dictionary (e.g., `{"base_url": "https://shop.test"}`).
            fetch: Sync callback `fetch(**request) -> str | bytes | (body, meta) | Response` for `follow`/`paginate`.
            repair: Callback `repair(req) -> {key: css} | None` called once per unhealed `adaptive` selector.
            repair_batch: Callback `repair_batch(batch) -> {key: css}` grouping broken fields by container skeleton.
            encoding: Optional explicit character encoding override when `source` is bytes.

        Returns:
            Extracted Python value matching the compiled schema structure.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile({"h": T.css("h1").text().strip()}).extract("<h1> Hi </h1>")
            {'h': 'Hi'}
            >>> from oxide_parse import T, compile, follow
            >>> pages = {"https://ex.test/2": "<b>ok</b>"}
            >>> p = compile({"x": follow(T.css("a").attr("href").url(), T.css("b").text())})
            >>> p.extract("<a href='/2'>go</a>", meta={"base_url": "https://ex.test"}, fetch=lambda url, **_: pages[url])
            {'x': 'ok'}
        """
        if fetch is None and repair is None and repair_batch is None and encoding is None and meta is None:
            if type(source) is str:
                return self._core.extract(source)
            if type(source) is bytes and (self._backend == "json" or not _bytes_need_python_decode(source)):
                return self._core.extract(source)
        if fetch is None and repair is None and repair_batch is None:
            src, merged_meta = self._prepare_source(source, meta, encoding)
            return self._core.extract(src, self._meta(merged_meta))
        return self._drive(source, meta, fetch, repair, repair_batch=repair_batch, encoding=encoding).result()

    def _drive(
        self,
        source: Any,
        meta: Optional[dict],
        fetch: Optional[Callable[..., Any]],
        repair: Optional[Callable[[Dict[str, Any]], Optional[Dict[str, str]]]] = None,
        repair_attempts: int = 2,
        repair_batch: Optional[Callable[[Dict[str, Any]], Optional[Dict[str, str]]]] = None,
        legacy_repair_payload: bool = False,
        encoding: Optional[str] = None,
    ) -> Session:
        sess = self.start(source, meta=meta, encoding=encoding)
        tries: Dict[Tuple[Any, str], int] = {}
        wants_repairs = repair is not None or repair_batch is not None
        while True:
            reqs = sess.requests()
            fetches = [r for r in reqs if r["kind"] == "fetch"]
            repairs = [r for r in reqs if r["kind"] == "repair"] if wants_repairs else []
            if not fetches and not repairs:
                return sess
            if fetches and fetch is None:
                raise OxideParseError("schema contains follow/paginate(...) but no `fetch` was given")
            answers: List[Tuple[int, Any]] = [
                (r["id"], fetch(**r["params"])) for r in fetches  # type: ignore[misc]
            ]
            # Decline after `repair_attempts` so invalid answers from a model never loop forever.
            live: List[Dict[str, Any]] = []
            for r in repairs:
                slot = (r.get("doc"), r["key"])
                tries[slot] = tries.get(slot, 0) + 1
                if tries[slot] > repair_attempts:
                    answers.append((r["id"], None))
                else:
                    live.append(r)
            if repair_batch is not None:
                for batch in _group_repairs(self._ir, live):
                    mapping = repair_batch(batch) or {}
                    for f in batch["fields"]:
                        answers.append((f["id"], {f["key"]: mapping[f["key"]]} if f["key"] in mapping else None))
            elif repair is not None:
                for r in live:
                    if legacy_repair_payload:
                        miss = {k: r[k] for k in ("key", "css", "fingerprint")}
                        errors = [{"path": p, "message": m} for p, m in sess._core.errors()]
                        r = {**r, "misses": [miss], "errors": errors}
                    answers.append((r["id"], repair(r)))
            if encoding is not None:
                sess._core.resume([(i, sess._payload(p, encoding=encoding)) for i, p in answers])
            else:
                sess.resume(answers)

    @staticmethod
    def _detailed(sess: Session) -> Detailed:
        return sess.detailed()

    def extract_detailed(
        self,
        source: Any,
        *,
        meta: Optional[dict] = None,
        fetch: Optional[Callable[..., Any]] = None,
        repair: Optional[Callable[[Dict[str, Any]], Optional[Dict[str, str]]]] = None,
        repair_batch: Optional[Callable[[Dict[str, Any]], Optional[Dict[str, str]]]] = None,
        encoding: Optional[str] = None,
    ) -> Detailed:
        """Extract data and return `Detailed(data, errors, misses, repairs)`.

        Args:
            source: Input document.
            meta: Optional metadata dictionary.
            fetch: Fetch callback for `follow`/`paginate`.
            repair: Per-key selector repair callback.
            repair_batch: Container-grouped selector repair callback.
            encoding: Optional character encoding override for byte inputs.

        Returns:
            `Detailed` result object.

        Examples:
            >>> from oxide_parse import T, compile
            >>> det = compile(T.css("b").map(T.text().int(), on_error="skip")).extract_detailed("<b>1</b><b>x</b>")
            >>> det.data, det.errors[0]["path"]
            ([1], '$[1]')
        """
        return self._drive(source, meta, fetch, repair, repair_batch=repair_batch, encoding=encoding).detailed()

    # -- adaptive fingerprints without files ---------------------------------
    def export_adaptive_store(self) -> str:
        """Export all learned `adaptive` selector fingerprints as a JSON string for Redis/DB/queue storage.

        Returns:
            JSON string representing the fingerprint store.

        Examples:
            >>> from oxide_parse import T, compile
            >>> p = compile({"t": T.css("h1", adaptive="t").text()})
            >>> _ = p.extract("<h1>Hello</h1>")
            >>> '"t"' in p.export_adaptive_store()
            True
        """
        return str(self._core.export_adaptive_store())

    def import_adaptive_store(self, state: str) -> int:
        """Load or merge fingerprints previously produced by `export_adaptive_store()` into this plan.

        Args:
            state: JSON fingerprint store string.

        Returns:
            Number of fingerprint entries imported.

        Examples:
            >>> from oxide_parse import T, compile
            >>> schema = {"t": T.css("h1", adaptive="t").text()}
            >>> src = compile(schema)
            >>> _ = src.extract("<h1>Hello</h1>")
            >>> dst = compile(schema)
            >>> dst.import_adaptive_store(src.export_adaptive_store())
            1
        """
        return int(self._core.import_adaptive_store(state))

    # -- self-healing without IO ---------------------------------------------
    def with_selectors(self, mapping: Dict[str, str]) -> "Plan":
        """Return a new `Plan` with rewritten `adaptive` CSS selectors for the specified keys.

        Clears stale baseline and `#healed` fingerprints for rewritten keys (including on disk when `adaptive_store` is
        configured) while keeping learned fingerprints for untouched keys.

        Args:
            mapping: Dictionary mapping `{adaptive_key: new_css_selector}`.

        Returns:
            Freshly compiled `Plan` copy.

        Raises:
            KeyError: If any key in `mapping` does not match an `adaptive` selector in this schema.

        Examples:
            >>> from oxide_parse import T, compile
            >>> p = compile({"price": T.css(".old", adaptive="price").text()})
            >>> p2 = p.with_selectors({"price": ".new"})
            >>> p2.extract("<span class='new'>$5</span>")
            {'price': '$5'}
        """
        seen: set = set()
        ir = _replace_adaptive(self._ir, mapping, seen)
        unknown = set(mapping) - seen
        if unknown:
            raise KeyError(f"no adaptive selector with key(s): {sorted(unknown)}")
        state = self.export_adaptive_store() if self._adaptive_store is None else None
        plan = Plan(ir, self._backend, adaptive_store=self._adaptive_store, adaptive_state=state, _ir=True)
        plan._core.forget_adaptive_keys(list(mapping))
        return plan

    def repair_request(self, source: Any, *, meta: Optional[dict] = None, max_chars: int = 20000) -> Optional[Dict[str, Any]]:
        """Run extraction and build a ready-to-send repair payload dictionary if any `adaptive` selector missed.

        Args:
            source: Document to inspect.
            meta: Optional metadata dictionary.
            max_chars: Character cap passed to `distill()` for the returned page `skeleton`.

        Returns:
            Repair request dict (`misses`, `errors`, `skeleton`, `schema`), or `None` if everything matched.

        Examples:
            >>> from oxide_parse import T, compile
            >>> p = compile({"x": T.css(".missing", adaptive="x").text()}, backend="lite")
            >>> req = p.repair_request("<div><p>hello</p></div>")
            >>> req is not None and req["misses"][0]["key"] == "x"
            True
        """
        return self._request_from(self.extract_detailed(source, meta=meta), source, max_chars)

    def _request_from(self, det: Detailed, source: Any, max_chars: int = 20000) -> Optional[Dict[str, Any]]:
        if not det.misses:
            return None
        try:
            skeleton: Optional[str] = distill(source, max_chars=max_chars)
        except ImportError:
            skeleton = None
        except TypeError:
            skeleton = distill(self._src(source), max_chars=max_chars)
        return {
            "task": "Each entry in `misses` is a CSS selector that no longer matches. Using `skeleton` (the page as it "
            "looks now) and the old `fingerprint` of the element (its `samples` are values the selector used to return), "
            "answer with a new CSS selector per key.",
            "misses": det.misses,
            "errors": det.errors,
            "skeleton": skeleton,
            "schema": self._ir,
        }

    def extract_or_repair(
        self,
        source: Any,
        repair: Optional[Callable[[Dict[str, Any]], Optional[Dict[str, str]]]] = None,
        *,
        meta: Optional[dict] = None,
        fetch: Optional[Callable[..., Any]] = None,
        repair_batch: Optional[Callable[[Dict[str, Any]], Optional[Dict[str, str]]]] = None,
    ) -> RepairResult:
        """Extract `source`, invoke `repair` / `repair_batch` on broken selectors, re-run, and return a patched `Plan`.

        Args:
            source: Input document.
            repair: Single-field repair callback receiving a repair request dict and returning `{key: css}` or `None`.
            meta: Optional metadata dictionary.
            fetch: Fetch callback if the schema uses `follow` or `paginate`.
            repair_batch: Batch repair callback grouping fields by container skeleton.

        Returns:
            `RepairResult(data, plan, repaired, errors, misses)`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> p = compile({"price": T.css(".old", adaptive="price", threshold=0.99).text()}, backend="lite")
            >>> _ = p.extract("<div><span class='old'>$5</span></div>")
            >>> res = p.extract_or_repair("<div><em class='new'>$5</em></div>", repair=lambda r: {r["key"]: ".new"})
            >>> res.repaired, res.data, res.plan.extract("<div><em class='new'>$9</em></div>")
            (True, {'price': '$5'}, {'price': '$9'})
        """
        det = self._drive(
            source, meta, fetch, repair, repair_batch=repair_batch, legacy_repair_payload=True,
        ).detailed()
        if det.repairs:
            return RepairResult(det.data, self.with_selectors(det.repairs), True, det.errors, det.misses)
        return RepairResult(det.data, self, False, det.errors, det.misses)

    def run_native(self, source: Any) -> int:
        """Parse and extract in Rust without constructing Python objects, returning extracted value count.

        Useful in benchmarks to isolate native parsing/execution time from Python object allocation cost.

        Args:
            source: Input document.

        Returns:
            Count of leaf/container values produced in Rust.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile({"a": T.css("b").text(), "b": T.css("i").text()}).run_native("<b>1</b><i>2</i>")
            3
        """
        return int(self._core.run_native(self._src(source)))

    def _batch_sources(self, sources: Iterable[Any], encoding: Optional[str] = None) -> Any:
        """Keep `list` or `tuple` inputs zero-copy when valid; consume other iterables in one pass."""
        if encoding is None and type(sources) in (list, tuple):
            if self._backend != "json" and all(
                type(s) is str or (type(s) is bytes and not _bytes_need_python_decode(s))
                for s in sources
            ):
                return sources
            if self._backend == "json" and all(isinstance(s, (str, bytes, bytearray)) for s in sources):
                return sources
        return [self._src(s, encoding=encoding) for s in sources]

    @overload
    def extract_batch(self, sources: Iterable[Any], *, threads: Optional[int] = None, errors: Literal["raise"] = "raise", encoding: Optional[str] = None) -> List[Any]: ...
    @overload
    def extract_batch(self, sources: Iterable[Any], *, threads: Optional[int] = None, errors: Literal["collect"], encoding: Optional[str] = None) -> Tuple[List[Any], List[Dict[str, Any]]]: ...
    @overload
    def extract_batch(self, sources: Iterable[Any], *, threads: Optional[int] = None, errors: str = "raise", encoding: Optional[str] = None) -> Any: ...

    def extract_batch(
        self,
        sources: Iterable[Any],
        *,
        threads: Optional[int] = None,
        errors: str = "raise",
        encoding: Optional[str] = None,
    ) -> Any:
        """Extract many documents in parallel on a Rayon thread pool with the GIL released.

        Args:
            sources: Iterable of documents (`list` and `tuple` run zero-copy; generators consume in a single pass).
            threads: Worker thread count (default: all logical CPU cores).
            errors: `"raise"` (default) raises on the first failed document and returns `results`; `"collect"` returns
                `(results, failures)` with `None` at failed indices and `{"index", "message"}` failure entries.
            encoding: Optional character encoding override for byte inputs.

        Returns:
            Result list when `errors="raise"`, or `(results, failures)` tuple when `errors="collect"`.

        Raises:
            ValueError: If `errors` is not `"raise"` or `"collect"`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> plan = compile({"n": T.css("b").text().int()}, backend="auto")
            >>> plan.extract_batch(["<b>1</b>", "<b>2</b>"], threads=2)
            [{'n': 1}, {'n': 2}]
            >>> plan.extract_batch((s for s in ["<b>1</b>", "<b>bad</b>"]), errors="collect")[0]
            [{'n': 1}, None]
        """
        if errors not in ("raise", "collect"):
            raise ValueError('errors must be "raise" or "collect"')
        data, failed = self._core.extract_batch(self._batch_sources(sources, encoding=encoding), threads, errors == "collect")
        if errors == "raise":
            return data
        return data, [{"index": i, "message": m} for i, m in failed]

    def extract_many(self, sources: Iterable[Any], *, encoding: Optional[str] = None) -> list:
        """Extract many documents in parallel using Rayon's global thread pool (GIL released).

        Args:
            sources: Iterable of documents (`list`/`tuple` stay zero-copy; iterators consume in a single pass).
            encoding: Optional character encoding override for byte inputs.

        Returns:
            List of extracted results aligned with `sources`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile({"n": T.css("b").text().int()}).extract_many(["<b>1</b>", b"<b>2</b>"])
            [{'n': 1}, {'n': 2}]
        """
        return self._core.extract_many(self._batch_sources(sources, encoding=encoding))

    @overload
    def extract_jsonl(self, source: Union[str, bytes, bytearray, Iterable[Union[str, bytes]]], *, errors: Literal["raise"] = "raise", encoding: str = "utf-8") -> List[Any]: ...
    @overload
    def extract_jsonl(self, source: Union[str, bytes, bytearray, Iterable[Union[str, bytes]]], *, errors: Literal["collect"], encoding: str = "utf-8") -> Tuple[List[Any], List[Dict[str, Any]]]: ...
    @overload
    def extract_jsonl(self, source: Union[str, bytes, bytearray, Iterable[Union[str, bytes]]], *, errors: str = "raise", encoding: str = "utf-8") -> Any: ...

    def extract_jsonl(
        self,
        source: Union[str, bytes, bytearray, Iterable[Union[str, bytes]]],
        *,
        errors: str = "raise",
        encoding: str = "utf-8",
    ) -> Any:
        """Extract every non-blank line of a JSON Lines (NDJSON) document or stream with a `json` plan.

        Args:
            source: JSONL `str`, `bytes`, `bytearray`, or line iterable (such as an open text/binary file handle).
            errors: `"raise"` (default) raises on invalid JSON or extraction failure; `"collect"` returns
                `(results, failures)` with `None` at failed indices and `{"index", "line", "message"}` error dicts.
            encoding: Encoding used to decode byte inputs or byte lines (default `"utf-8"`).

        Returns:
            List of extracted records when `errors="raise"`, or `(results, failures)` when `errors="collect"`.

        Raises:
            OxideParseError: If the plan does not use `backend="json"` or an error occurs with `errors="raise"`.
            ValueError: If `errors` is not `"raise"` or `"collect"`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> p = compile({"id": T["id"].int()}, backend="json")
            >>> p.extract_jsonl('{"id": 1}\\n{"id": "2"}\\n')
            [{'id': 1}, {'id': 2}]
        """
        if errors not in ("raise", "collect"):
            raise ValueError('errors must be "raise" or "collect"')
        if self._backend != "json":
            raise OxideParseError('extract_jsonl() requires a plan compiled with backend="json"')
        lines: Iterable[Union[str, bytes, bytearray]]
        if isinstance(source, (bytes, bytearray)):
            lines = bytes(source).decode(encoding, errors="replace").splitlines()
        elif isinstance(source, str):
            lines = source.splitlines()
        else:
            lines = source

        parsed_rows: List[Tuple[int, int, str]] = []
        failures: List[Dict[str, Any]] = []
        bad_slots: set = set()
        results: List[Any] = []
        rec_idx = 0
        for line_no, raw in enumerate(lines, 1):
            text = bytes(raw).decode(encoding, errors="replace") if isinstance(raw, (bytes, bytearray)) else raw
            stripped = text.strip()
            if not stripped:
                continue
            try:
                json.loads(stripped)
            except ValueError as exc:
                if errors == "raise":
                    raise OxideParseError(f"jsonl line {line_no}: {exc}") from exc
                bad_slots.add(rec_idx)
                failures.append({"index": rec_idx, "line": line_no, "message": str(exc)})
                results.append(None)
                rec_idx += 1
                continue
            parsed_rows.append((rec_idx, line_no, stripped))
            results.append(None)
            rec_idx += 1

        if parsed_rows:
            batch_in = [payload for _, _, payload in parsed_rows]
            batch_out, batch_fail = self.extract_batch(batch_in, errors="collect")
            for k, (slot, _line_no, _) in enumerate(parsed_rows):
                results[slot] = batch_out[k]
            for f in batch_fail:
                slot, line_no, _ = parsed_rows[f["index"]]
                msg = f["message"]
                if errors == "raise":
                    raise OxideParseError(f"jsonl line {line_no}: {msg}")
                failures.append({"index": slot, "line": line_no, "message": msg})

        if errors == "raise":
            return results
        failures.sort(key=lambda item: item["index"])
        return results, failures

    @overload
    def extract_csv(self, source: Union[str, bytes, bytearray, Iterable[str]], *, delimiter: str = ",", quotechar: str = '"', fieldnames: Optional[Sequence[str]] = None, errors: Literal["raise"] = "raise", encoding: str = "utf-8") -> List[Any]: ...
    @overload
    def extract_csv(self, source: Union[str, bytes, bytearray, Iterable[str]], *, delimiter: str = ",", quotechar: str = '"', fieldnames: Optional[Sequence[str]] = None, errors: Literal["collect"], encoding: str = "utf-8") -> Tuple[List[Any], List[Dict[str, Any]]]: ...
    @overload
    def extract_csv(self, source: Union[str, bytes, bytearray, Iterable[str]], *, delimiter: str = ",", quotechar: str = '"', fieldnames: Optional[Sequence[str]] = None, errors: str = "raise", encoding: str = "utf-8") -> Any: ...

    def extract_csv(
        self,
        source: Union[str, bytes, bytearray, Iterable[str]],
        *,
        delimiter: str = ",",
        quotechar: str = '"',
        fieldnames: Optional[Sequence[str]] = None,
        errors: str = "raise",
        encoding: str = "utf-8",
    ) -> Any:
        """Extract rows of a CSV document (preserving quoted newlines and commas) using a `json` plan.

        Args:
            source: CSV `str`, `bytes`, `bytearray`, or text line iterable accepted by `csv.DictReader`.
            delimiter: Field separator character (default `","`).
            quotechar: Quoting character (default `'"'`).
            fieldnames: Optional column names; when `None`, the first CSV row supplies header keys.
            errors: `"raise"` (default) raises on extraction failure; `"collect"` returns `(results, failures)`.
            encoding: Encoding used when `source` is bytes (default `"utf-8"`).

        Returns:
            List of extracted row dicts/values when `errors="raise"`, or `(results, failures)` when `"collect"`.

        Raises:
            OxideParseError: If the plan does not use `backend="json"` or row extraction fails under `errors="raise"`.
            ValueError: If `errors` is not `"raise"` or `"collect"`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> csv_src = 'name,note\\n"Ada","line 1\\nline 2, ok"\\n'
            >>> compile({"name": T["name"], "note": T["note"]}, backend="json").extract_csv(csv_src)
            [{'name': 'Ada', 'note': 'line 1\\nline 2, ok'}]
        """
        if errors not in ("raise", "collect"):
            raise ValueError('errors must be "raise" or "collect"')
        if self._backend != "json":
            raise OxideParseError('extract_csv() requires a plan compiled with backend="json"')
        stream: Iterable[str]
        if isinstance(source, (bytes, bytearray)):
            stream = io.StringIO(bytes(source).decode(encoding, errors="replace"), newline="")
        elif isinstance(source, str):
            stream = io.StringIO(source, newline="")
        else:
            stream = source

        reader = csv.DictReader(stream, fieldnames=fieldnames, delimiter=delimiter, quotechar=quotechar)
        results: List[Any] = []
        failures: List[Dict[str, Any]] = []
        for idx, row in enumerate(reader):
            try:
                results.append(self.extract(dict(row)))
            except Exception as exc:
                if errors == "raise":
                    raise
                results.append(None)
                failures.append({"index": idx, "message": str(exc)})
        if errors == "raise":
            return results
        return results, failures

    # -- async -------------------------------------------------------------
    async def astart(self, source: Any, *, meta: Optional[dict] = None, encoding: Optional[str] = None) -> Session:
        """Async variant of `start()`, offloading parsing of large documents (>= 64 KiB) to a worker thread.

        Args:
            source: Document input.
            meta: Optional metadata dictionary.
            encoding: Optional character encoding override for byte inputs.

        Returns:
            Initialized `Session`.

        Examples:
            >>> sess = await plan.astart(html)  # doctest: +SKIP
        """
        text, merged_meta = self._prepare_source(source, meta, encoding)
        core = await _maybe_thread(self._core.start, len(text), text, self._meta(merged_meta))
        return Session(self, core)

    async def aextract(
        self,
        source: Any,
        *,
        meta: Optional[dict] = None,
        fetch: Optional[Callable[..., Any]] = None,
        repair: Optional[Callable[[Dict[str, Any]], Any]] = None,
        repair_batch: Optional[Callable[[Dict[str, Any]], Any]] = None,
        encoding: Optional[str] = None,
    ) -> Any:
        """Async extraction driver; runs pending fetches and repairs concurrently per round via `asyncio.gather`.

        Args:
            source: Document input.
            meta: Optional metadata dictionary.
            fetch: Sync or async `fetch(**request)` callback for `follow`/`paginate`.
            repair: Sync or async per-key repair callback.
            repair_batch: Sync or async container-grouped repair callback.
            encoding: Optional character encoding override for byte inputs.

        Returns:
            Extracted Python value.

        Examples:
            >>> data = await plan.aextract(html, fetch=afetch)  # doctest: +SKIP
        """
        sess = await self._adrive(source, meta, fetch, repair, repair_batch=repair_batch, encoding=encoding)
        return sess.result()

    async def aextract_detailed(
        self,
        source: Any,
        *,
        meta: Optional[dict] = None,
        fetch: Optional[Callable[..., Any]] = None,
        repair: Optional[Callable[[Dict[str, Any]], Any]] = None,
        repair_batch: Optional[Callable[[Dict[str, Any]], Any]] = None,
        encoding: Optional[str] = None,
    ) -> Detailed:
        """Async variant of `extract_detailed()`.

        Args:
            source: Document input.
            meta: Optional metadata dictionary.
            fetch: Sync or async fetch callback.
            repair: Sync or async repair callback.
            repair_batch: Sync or async batch repair callback.
            encoding: Optional character encoding override for byte inputs.

        Returns:
            `Detailed` result object.

        Examples:
            >>> det = await plan.aextract_detailed(html, fetch=afetch)  # doctest: +SKIP
        """
        return (await self._adrive(source, meta, fetch, repair, repair_batch=repair_batch, encoding=encoding)).detailed()

    async def _adrive(
        self,
        source: Any,
        meta: Optional[dict],
        fetch: Optional[Callable[..., Any]],
        repair: Optional[Callable[[Dict[str, Any]], Any]] = None,
        repair_attempts: int = 2,
        repair_batch: Optional[Callable[[Dict[str, Any]], Any]] = None,
        encoding: Optional[str] = None,
    ) -> Session:
        sess = await self.astart(source, meta=meta, encoding=encoding)
        tries: Dict[Tuple[Any, str], int] = {}

        async def call(fn: Callable[..., Any], *args: Any, **kw: Any) -> Any:
            r = fn(*args, **kw)
            if inspect.isawaitable(r):
                r = await r
            return r

        wants_repairs = repair is not None or repair_batch is not None
        while True:
            reqs = sess.requests()
            fetches = [r for r in reqs if r["kind"] == "fetch"]
            repairs = [r for r in reqs if r["kind"] == "repair"] if wants_repairs else []
            if not fetches and not repairs:
                return sess
            if fetches and fetch is None:
                raise OxideParseError("schema contains follow/paginate(...) but no `fetch` was given")

            live: List[Dict[str, Any]] = []
            declined: List[Tuple[int, Any]] = []
            for r in repairs:
                slot = (r.get("doc"), r["key"])
                tries[slot] = tries.get(slot, 0) + 1
                if tries[slot] > repair_attempts:
                    declined.append((r["id"], None))
                else:
                    live.append(r)

            async def one_fetch(r: Dict[str, Any]) -> Tuple[int, Any]:
                return r["id"], await call(fetch, **r["params"])  # type: ignore[arg-type]

            async def one_repair(r: Dict[str, Any]) -> List[Tuple[int, Any]]:
                return [(r["id"], await call(repair, r))]  # type: ignore[arg-type]

            async def one_batch(batch: Dict[str, Any]) -> List[Tuple[int, Any]]:
                mapping = (await call(repair_batch, batch)) or {}  # type: ignore[arg-type]
                return [(f["id"], {f["key"]: mapping[f["key"]]} if f["key"] in mapping else None) for f in batch["fields"]]

            jobs = [one_fetch(r) for r in fetches]
            if repair_batch is not None:
                repair_jobs = [one_batch(b) for b in _group_repairs(self._ir, live)]
            else:
                repair_jobs = [one_repair(r) for r in live]
            results = await asyncio.gather(*jobs, *repair_jobs)
            answers: List[Tuple[int, Any]] = list(declined)
            for res in results:
                if isinstance(res, list):
                    answers.extend(res)
                else:
                    answers.append(res)
            await sess.aresume(answers)


async def _maybe_thread(fn, size, *args):
    if size >= _OFFLOAD_BYTES:
        return await asyncio.to_thread(fn, *args)
    return fn(*args)


def compile(schema: Any, backend: Backend = "html", **kw: Any) -> Plan:  # noqa: A001
    """Compile a schema (`dict`, `list`, `T` expression, or literal) into a reusable `Plan`.

    Args:
        schema: Extraction schema.
        backend: Execution backend (`"html"`, `"auto"`, `"stream"`, `"lite"`, `"scraper"`, or `"json"`).
        **kw: Additional `Plan` keyword options (`adaptive_store`, `adaptive_state`).

    Returns:
        Compiled `Plan`.

    Examples:
        >>> from oxide_parse import T, compile
        >>> plan = compile({"title": T.css("h1").text()}, backend="auto")
        >>> plan.extract("<h1>Hello</h1>")
        {'title': 'Hello'}
    """
    return Plan(schema, backend, **kw)


def extract(source: Any, schema: Any, **kw: Any) -> Any:
    """Convenience one-shot extraction that compiles `schema` for a single document.

    For repeated calls, prefer compiling once with `compile(schema)` and calling `plan.extract(doc)`.

    Args:
        source: Input document.
        schema: Extraction schema.
        **kw: `backend` (default `"html"`) plus any keyword arguments accepted by `Plan.extract()`.

    Returns:
        Extracted Python value.

    Examples:
        >>> from oxide_parse import T, extract
        >>> extract("<h1>Hi</h1>", {"h1": T.css("h1").text()}, backend="auto")
        {'h1': 'Hi'}
    """
    backend = kw.pop("backend", "html")
    return Plan(schema, backend).extract(source, **kw)
