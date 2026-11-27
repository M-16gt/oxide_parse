"""Deterministic converters from other declarative extraction formats to `oxide_parse` IR.

No LLM or network IO is performed. Feed the resulting IR straight to `Plan.from_ir(...)`, or turn it into readable
Python DSL code via `from_crawl4ai_code(...)` / `from_selector_map_code(...)`.

Examples:
    >>> from oxide_parse import Plan
    >>> from oxide_parse.migrate import from_crawl4ai_code, from_selector_map
    >>> ir = from_selector_map({"title": "h1::text", "href": "a::attr(href)"})
    >>> Plan.from_ir(ir).extract("<h1>Hi</h1><a href='/x'>x</a>")
    {'title': 'Hi', 'href': '/x'}
"""
from __future__ import annotations

import re
import warnings
from typing import Any, Dict, List, Literal, Optional, Set, Tuple

from . import ir_spec
from .decompile import decompile
from .expr import T, Expr, to_ir

__all__ = [
    "MigrationError",
    "Crawl4AIPromptPatch",
    "from_crawl4ai",
    "from_crawl4ai_code",
    "from_selector_map",
    "from_selector_map_code",
    "patch_crawl4ai_prompt",
]


class MigrationError(ValueError):
    """Raised when a source schema uses a construct with no faithful `oxide_parse` equivalent.

    Examples:
        >>> from oxide_parse.migrate import MigrationError, from_crawl4ai
        >>> try:
        ...     from_crawl4ai({"baseSelector": ".c", "fields": [{"name": "x", "type": "computed"}]})
        ... except MigrationError as exc:
        ...     "computed" in str(exc)
        True
    """


# -- Crawl4AI ---------------------------------------------------------------------------------------------------------
_TRANSFORMS = {"lowercase": "lower", "lower": "lower", "uppercase": "upper", "upper": "upper", "strip": "strip"}
# `sel/@href`, `sel/text()`: XPath already returns strings
_XPATH_STRINGS = re.compile(r"(/@[\w:.-]+|/text\(\)|\bstring\([^()]*\)|\bnormalize-space\([^()]*\))\s*$")
_XPATH_SCALAR = re.compile(r"^\s*\(?\s*(string|normalize-space|concat|substring[\w-]*|translate|count|sum)\s*\(")


def _decode_crawl4ai_dicts(value: Any) -> Any:
    if isinstance(value, list):
        return [_decode_crawl4ai_dicts(item) for item in value]
    if isinstance(value, dict):
        if set(value) == {"type", "value"} and value["type"] == "dict" and isinstance(value["value"], dict):
            value = value["value"]
        return {key: _decode_crawl4ai_dicts(item) for key, item in value.items()}
    return value


def _unwrap(strategy: Dict[str, Any]) -> Tuple[Dict[str, Any], str]:
    """Accept a plain schema, a serialized strategy, or a `CrawlerRunConfig` dump."""
    current = _decode_crawl4ai_dicts(strategy)
    hint = ""
    while isinstance(current, dict):
        name = current.get("type") or current.get("strategy")
        if isinstance(name, str) and ("css" in name.lower() or "xpath" in name.lower()):
            hint = name
        if "baseSelector" in current or "fields" in current:
            return current, hint
        current = next(
            (current[key] for key in ("params", "extraction_strategy", "schema") if isinstance(current.get(key), dict)),
            None,
        )
    raise MigrationError("not a Crawl4AI schema: expected `baseSelector` and `fields`")


def _kind_of(schema: Dict[str, Any], hint: str, kind: Optional[str]) -> str:
    if kind is not None:
        if kind not in ("css", "xpath"):
            raise ValueError('kind must be "css" or "xpath"')
        return kind
    if hint:
        return "xpath" if "xpath" in hint.lower() else "css"
    base = schema.get("baseSelector")
    return "xpath" if isinstance(base, str) and _is_xpath(base) else "css"


def _select(sel: Optional[str], kind: str) -> Expr:
    if not sel:
        return T
    return T.css(sel) if kind == "css" else T.xpath(sel)


def _is_string_xpath(sel: Optional[str], kind: str) -> bool:
    return kind == "xpath" and bool(sel) and bool(_XPATH_STRINGS.search(sel or "") or _XPATH_SCALAR.match(sel or ""))


def _scalar_xpath(sel: str) -> bool:
    return bool(_XPATH_SCALAR.match(sel))


def _crawl4ai_text(base: Expr, kind: str) -> Expr:
    if kind == "css":
        return base.text(strip=True)
    return base.text().strip()


_SIMPLE_CSS_TOKEN = re.compile(r"^(?P<tag>[a-zA-Z_*][a-zA-Z0-9_-]*)?(?:(?P<rest>(?:(?:\.[a-zA-Z0-9_-]+)|(?:#[a-zA-Z0-9_-]+))*))$")


def _sibling_source_to_xpath(source: str, path: str) -> str:
    s = source.strip()
    if not s.startswith("+"):
        raise MigrationError(f"{path}: `source` {source!r} is not a sibling selector (`+ ...`); use T.css(...).parent() / xpath by hand")
    sel = s[1:].strip()
    if not sel or any(c.isspace() for c in sel) or any(ch in sel for ch in ">~[,:"):
        raise MigrationError(f"{path}: `source` {source!r} is too complex; write explicit xpath by hand")
    m = _SIMPLE_CSS_TOKEN.match(sel)
    if not m or (not m.group("tag") and not m.group("rest")):
        raise MigrationError(f"{path}: unsupported sibling selector `source` {source!r}")
    tag = m.group("tag") or "*"
    preds: List[str] = []
    for part in re.findall(r"[.#][a-zA-Z0-9_-]+", m.group("rest") or ""):
        if part.startswith("."):
            cls = part[1:]
            preds.append(f"contains(concat(' ',normalize-space(@class),' '),' {cls} ')")
        else:
            preds.append(f"@id={part[1:]!r}")
    cond = ("[" + " and ".join(preds) + "]") if preds else ""
    return f"following-sibling::{tag}{cond}[1]"


def _field(f: Dict[str, Any], kind: str, path: str, *, allow_sibling_source: bool = False) -> Expr:
    if not isinstance(f, dict):
        raise MigrationError(f"{path}: a field is an object, got {type(f).__name__}")
    ftype = f.get("type", "text")
    sel = f.get("selector")
    if sel is not None and not isinstance(sel, str):
        raise MigrationError(f"{path}.selector: expected a string")
    sibling_xp: Optional[str] = None
    src = f.get("source")
    if src:
        if not allow_sibling_source:
            raise MigrationError(f"{path}: `source` (sibling lookup) has no equivalent; use T.css(...).parent() / xpath by hand")
        if not isinstance(src, str):
            raise MigrationError(f"{path}.source: expected a string")
        sibling_xp = _sibling_source_to_xpath(src, path)
    if ftype == "computed":
        raise MigrationError(f"{path}: `computed` fields run Python code; write them as a Python function over the result")
    base = T.xpath(sibling_xp) if sibling_xp and not sel else (
        T.xpath(sibling_xp).css(sel) if (sibling_xp and sel and kind == "css")
        else T.xpath(sibling_xp).xpath(sel) if (sibling_xp and sel)
        else _select(sel, kind)
    )
    eff_kind = "xpath" if (sibling_xp and not sel) else kind
    strings = _is_string_xpath(sel, eff_kind)
    pick = (lambda e: e) if (strings and _scalar_xpath(sel or "")) else (lambda e: e.at(0)) if strings else None

    if ftype == "text":
        e = pick(base).strip() if pick else _crawl4ai_text(base, eff_kind)
    elif ftype == "attribute":
        attr = f.get("attribute")
        if not attr:
            raise MigrationError(f"{path}: `attribute` field without `attribute`")
        if not isinstance(attr, str):
            raise MigrationError(f"{path}.attribute: expected an attribute name")
        e = pick(base) if pick else base.attr(attr)
    elif ftype == "html":
        e = pick(base) if pick else base.html()
    elif ftype == "regex":
        pattern = f.get("pattern")
        if not pattern:
            raise MigrationError(f"{path}: `regex` field without `pattern`")
        if not isinstance(pattern, str):
            raise MigrationError(f"{path}.pattern: expected a regex pattern")
        try:
            re.compile(pattern)
        except re.error as exc:
            raise MigrationError(f"{path}: invalid pattern {pattern!r}: {exc}") from exc
        if re.search(r"\(\?<?[=!]", pattern):
            warnings.warn(f"{path}: look-around is not supported by the Rust regex engine; rewrite {pattern!r}", stacklevel=3)
        group = f.get("group", 1)
        if type(group) is not int or group < 0:
            raise MigrationError(f"{path}.group: expected a non-negative integer")
        text = pick(base) if pick else _crawl4ai_text(base, eff_kind)
        e = text.regex(pattern, group)
    elif ftype in ("list", "nested_list"):
        e = base.map(_object(f.get("fields", []), eff_kind, path, allow_sibling_source=allow_sibling_source))
    elif ftype == "nested":
        body = _object(f.get("fields", []), eff_kind, path, allow_sibling_source=allow_sibling_source)
        if not sel and not sibling_xp:
            raise MigrationError(f"{path}.selector: nested fields require a selector")
        return base[:1].map(body).at(0).default({})
    else:
        raise MigrationError(f"{path}: unsupported field type {ftype!r}")

    transform = f.get("transform")
    if transform:
        op = _TRANSFORMS.get(str(transform).lower())
        if op is None:
            raise MigrationError(f"{path}: unsupported transform {transform!r} (lowercase, uppercase, strip)")
        e = getattr(e, op)()
    if "default" in f and f["default"] is not None:
        e = e.default(f["default"])
    return e


def _object(fields: List[Dict[str, Any]], kind: str, path: str, *, allow_sibling_source: bool = False) -> Dict[str, Expr]:
    if not isinstance(fields, list):
        raise MigrationError(f"{path}.fields: expected a list")
    out: Dict[str, Expr] = {}
    for i, f in enumerate(fields):
        name = f.get("name") if isinstance(f, dict) else None
        if not isinstance(name, str) or not name:
            raise MigrationError(f"{path}.fields[{i}]: a field needs a `name`")
        out[name] = _field(f, kind, f"{path}.{name}", allow_sibling_source=allow_sibling_source)
    return out


def from_crawl4ai(
    strategy: Dict[str, Any],
    *,
    kind: Optional[str] = None,
    wrap: Optional[str] = None,
    allow_sibling_source: bool = False,
) -> Dict[str, Any]:
    """Convert a Crawl4AI CSS/XPath extraction strategy into `oxide_parse` IR.

    Accepts a plain schema dict, a serialized `JsonCssExtractionStrategy`/`JsonXPathExtractionStrategy`, or a
    `CrawlerRunConfig.dump()`.

    Args:
        strategy: Crawl4AI schema or config dictionary.
        kind: Force `"css"` or `"xpath"` selector mode (default: inferred from strategy type / `baseSelector`).
        wrap: Optional key name wrapping the root list (`wrap="items"` produces `{"items": [...]}`).
        allow_sibling_source: Opt-in support for `source="+ tag.cls"` sibling row lookups translated to
            `T.xpath("following-sibling::...")` (default `False` keeps `MigrationError`).

    Returns:
        `oxide_parse` IR dictionary ready for `Plan.from_ir()`.

    Raises:
        MigrationError: On unsupported features (`computed`, `source` unless opted in, unknown types/transforms).

    Examples:
        >>> from oxide_parse import Plan
        >>> from oxide_parse.migrate import from_crawl4ai
        >>> ir = from_crawl4ai({"baseSelector": ".p", "fields": [{"name": "n", "selector": "b", "type": "text"}]})
        >>> Plan.from_ir(ir).extract("<div class='p'><b>A</b></div>")
        [{'n': 'A'}]
    """
    if (
        isinstance(strategy, dict)
        and len(strategy) == 1
        and set(strategy.keys()) <= {"pipe", "object", "list", "lit"}
        and not ir_spec.validate_structure(strategy)
    ):
        return {"object": {wrap: dict(strategy)}} if wrap else dict(strategy)
    schema, hint = _unwrap(strategy)
    kind = _kind_of(schema, hint, kind)
    body: Dict[str, Expr] = {}
    body.update(_object(schema.get("baseFields") or [], kind, "$.baseFields", allow_sibling_source=allow_sibling_source))
    body.update(_object(schema.get("fields", []), kind, "$", allow_sibling_source=allow_sibling_source))
    base = schema.get("baseSelector")
    if base is not None and not isinstance(base, str):
        raise MigrationError("$.baseSelector: expected a string")
    expr = _select(base, kind).map(body) if base else body
    return to_ir({wrap: expr}) if wrap else to_ir(expr)


def from_crawl4ai_code(
    strategy: Dict[str, Any],
    *,
    kind: Optional[str] = None,
    wrap: Optional[str] = None,
    name: str = "schema",
    allow_sibling_source: bool = False,
) -> str:
    """Convert a Crawl4AI strategy directly into runnable Python DSL source code.

    Args:
        strategy: Crawl4AI schema or config dictionary.
        kind: Force `"css"` or `"xpath"` selector mode.
        wrap: Optional wrapper key for the root list.
        name: Variable name assigned in generated code.
        allow_sibling_source: Opt-in support for `source="+ tag.cls"` sibling row lookups.

    Returns:
        Python source defining `name`.

    Examples:
        >>> from oxide_parse.migrate import from_crawl4ai_code
        >>> code = from_crawl4ai_code({"baseSelector": ".p", "fields": [{"name": "n", "selector": "b", "type": "text"}]})
        >>> "T.css('.p').map(" in code
        True
    """
    return decompile(from_crawl4ai(strategy, kind=kind, wrap=wrap, allow_sibling_source=allow_sibling_source), name)


# -- flat selector maps (Scrapy / Parsel / Frostwork style) ---------------------------------------------------------------
_SUFFIX = re.compile(r"^(?P<sel>.*?)::(?P<fn>text|attr\((?P<attr>[^)]*)\))\s*$", re.S)
_BASE_KEYS = ("_base", "$base")


def _is_xpath(sel: str) -> bool:
    s = sel.strip()
    return s.startswith(("/", "./", "(", "xpath:")) or s.startswith("..")


def _split(spec: str) -> tuple:
    spec = spec.strip()
    if spec.startswith("xpath:"):
        spec = spec[len("xpath:"):].strip()
    m = _SUFFIX.match(spec)
    if not m:
        return spec.strip(), None, None
    return m.group("sel").strip(), m.group("fn"), (m.group("attr") or "").strip().strip("'\"")


def _value(spec: str, strip: bool, path: str, *, own_text: bool = False) -> Expr:
    sel, fn, attr = _split(spec)
    xpath = _is_xpath(spec) or spec.strip().startswith("xpath:")
    base = _select(sel or None, "xpath" if xpath else "css")
    if xpath and _is_string_xpath(sel, "xpath"):
        e = base if _scalar_xpath(sel) else base.at(0)
        return e.strip() if strip else e
    if fn is None or fn == "text":
        e = base.own_text(strip=strip) if own_text else (base.text().strip() if strip else base.text())
        return e
    if not attr:
        raise MigrationError(f"{path}: `::attr()` needs a name")
    return base.attr(attr)


def _convert(value: Any, strip: bool, path: str, *, own_text: bool = False) -> Any:
    if isinstance(value, str):
        return _value(value, strip, path, own_text=own_text)
    if isinstance(value, (list, tuple)):
        if len(value) != 1:
            raise MigrationError(f"{path}: a list holds exactly one selector (all matches), got {len(value)}")
        spec = value[0]
        if isinstance(spec, dict):
            return _record(spec, strip, path, force_list=True, own_text=own_text)
        if not isinstance(spec, str):
            raise MigrationError(f"{path}: expected a selector string")
        sel, fn, attr = _split(spec)
        xpath = _is_xpath(spec)
        base = _select(sel, "xpath" if xpath else "css")
        if xpath and _is_string_xpath(spec, "xpath"):
            return base
        if fn and fn.startswith("attr"):
            if not attr:
                raise MigrationError(f"{path}: `::attr()` needs a name")
            return base.map(T.attr(attr))
        item = T.own_text(strip=strip) if own_text else (T.text().strip() if strip else T.text())
        return base.map(item)
    if isinstance(value, dict):
        return _record(value, strip, path, own_text=own_text)
    raise MigrationError(f"{path}: unsupported value {type(value).__name__}")


def _record(d: Dict[str, Any], strip: bool, path: str, force_list: bool = False, *, own_text: bool = False) -> Any:
    base = next((d[k] for k in _BASE_KEYS if k in d), None)
    if base is None:
        if force_list:
            raise MigrationError(f"{path}: a record inside a list needs `_base` (the selector of one record)")
        return {k: _convert(v, strip, f"{path}.{k}", own_text=own_text) for k, v in d.items() if k not in _BASE_KEYS}
    if not isinstance(base, str) or not base.strip():
        raise MigrationError(f"{path}._base: expected a non-empty selector string")
    sel, _fn, _attr = _split(base)
    items = {k: _convert(v, strip, f"{path}.{k}", own_text=own_text) for k, v in d.items() if k not in _BASE_KEYS}
    return _select(sel, "xpath" if _is_xpath(base) else "css").map(items)


def from_selector_map(mapping: Dict[str, Any], *, strip: bool = True, own_text: bool = False) -> Dict[str, Any]:
    """Convert a flat Scrapy/Parsel-style selector map into `oxide_parse` IR.

    Supports `"sel::text"`, `"sel::attr(name)"`, single-element list specs (`["li::text"]`), XPath selectors
    (starting with `/`, `./`, `(`, or `xpath:`), nested dicts, and `"_base"` repeating record maps.

    Args:
        mapping: Dictionary mapping field names to selector specs or nested dicts/lists.
        strip: Strip extracted text values (default `True`).
        own_text: Emit `.own_text()` instead of `.text()` for `::text` specs to read direct child text only.

    Returns:
        `oxide_parse` IR dictionary.

    Raises:
        MigrationError: If `mapping` is invalid or contains malformed `::attr()` / list specs.

    Examples:
        >>> from oxide_parse import Plan
        >>> from oxide_parse.migrate import from_selector_map
        >>> ir = from_selector_map({"title": "h1::text", "tags": ["li::text"]})
        >>> Plan.from_ir(ir).extract("<h1> Hi </h1><li>a</li><li>b</li>")
        {'title': 'Hi', 'tags': ['a', 'b']}
    """
    if not isinstance(mapping, dict):
        raise MigrationError("expected a dict of {name: selector}")
    return to_ir(_record(mapping, strip, "$", own_text=own_text))


def from_selector_map_code(
    mapping: Dict[str, Any],
    *,
    strip: bool = True,
    name: str = "schema",
    own_text: bool = False,
) -> str:
    """Convert a flat selector map directly into readable Python DSL source code.

    Args:
        mapping: Dictionary mapping field names to selector specs.
        strip: Strip extracted text values (default `True`).
        name: Variable name assigned in generated code.
        own_text: Emit `.own_text()` instead of `.text()` for `::text` specs.

    Returns:
        Python source defining `name`.

    Examples:
        >>> from oxide_parse.migrate import from_selector_map_code
        >>> "T.css('h1').text().strip()" in from_selector_map_code({"title": "h1::text"})
        True
    """
    return decompile(from_selector_map(mapping, strip=strip, own_text=own_text), name)


# -- Crawl4AI LLM prompt override --------------------------------------------------------------------------------------

_SENTINEL = object()


def _ir_sample_records(extracted: Any) -> Tuple[int, List[Dict[str, Any]]]:
    if extracted is None:
        return 0, []
    if isinstance(extracted, list):
        dicts = [x for x in extracted if isinstance(x, dict)]
        if dicts:
            return len(extracted), dicts
        return len(extracted), [{"value": x} for x in extracted]
    if isinstance(extracted, dict):
        return 1, [extracted]
    return 1, [{"value": extracted}]


def _collect_field_details(records: List[Dict[str, Any]], expected_fields: Optional[List[str]]) -> List[Dict[str, Any]]:
    keys: List[str] = []
    seen: Set[str] = set()
    for rec in records:
        for k in rec.keys():
            if k not in seen:
                seen.add(k)
                keys.append(str(k))
    for k in expected_fields or []:
        if k not in seen:
            seen.add(k)
            keys.append(k)
    details: List[Dict[str, Any]] = []
    total = len(records)
    for k in keys:
        populated = 0
        sample = None
        for rec in records:
            v = rec.get(k)
            if v not in (None, "", [], {}):
                populated += 1
                if sample is None:
                    sample = str(v)[:120]
        details.append({"name": k, "populated_count": populated, "total_count": total, "sample_value": sample})
    return details


def _safe_distill(html: str, max_chars: int = 2000) -> str:
    try:
        from .plan import distill

        return distill(html, max_chars=max_chars)
    except Exception:
        return html[:max_chars]


def _validate_ir_for_crawl4ai(
    schema: Any,
    html_content: str,
    schema_type: str = "CSS",
    expected_fields: Optional[List[str]] = None,
) -> Dict[str, Any]:
    from .plan import Plan, _core, validate_ir

    del schema_type
    result: Dict[str, Any] = {
        "success": False,
        "base_elements_found": 0,
        "total_fields": 0,
        "populated_fields": 0,
        "field_coverage": 0.0,
        "field_details": [],
        "issues": [],
        "sample_base_html": "",
        "top_level_structure": "",
    }
    if not isinstance(schema, dict):
        result["issues"].append(f"Expected oxide_parse IR JSON object, got {type(schema).__name__}")
        result["top_level_structure"] = _safe_distill(html_content)
        return result

    errors = validate_ir(schema, backend="lite")
    if errors:
        result["issues"].extend(errors)
        result["top_level_structure"] = _safe_distill(html_content)
        return result

    if _core is None:
        result["success"] = True
        return result

    try:
        extracted = Plan.from_ir(schema, "lite").extract(html_content)
    except Exception as exc:
        result["issues"].append(f"Extraction error: {exc}")
        result["top_level_structure"] = _safe_distill(html_content)
        return result

    count, records = _ir_sample_records(extracted)
    result["base_elements_found"] = count
    details = _collect_field_details(records, expected_fields)
    result["field_details"] = details
    result["total_fields"] = len(details)
    result["populated_fields"] = sum(1 for fd in details if fd["populated_count"] > 0)
    if result["total_fields"] > 0:
        result["field_coverage"] = result["populated_fields"] / result["total_fields"]

    if count == 0:
        result["issues"].append("IR schema matched 0 items in the HTML sample")
        result["top_level_structure"] = _safe_distill(html_content)
    elif result["populated_fields"] == 0:
        result["issues"].append("All extracted fields were empty/null")
        result["sample_base_html"] = _safe_distill(html_content)
    else:
        empty_fields = [fd["name"] for fd in details if fd["populated_count"] == 0]
        if empty_fields:
            result["issues"].append(f"Fields always empty: {', '.join(empty_fields)}")

    if expected_fields:
        populated_names = {fd["name"] for fd in details if fd["populated_count"] > 0}
        missing = [f for f in expected_fields if f not in populated_names]
        if missing:
            result["issues"].append(f"Expected fields missing or empty: {', '.join(missing)}")
        result["success"] = count > 0 and all(f in populated_names for f in expected_fields)
    else:
        result["success"] = count > 0 and result["populated_fields"] > 0
    return result


def _build_ir_feedback(
    validation_result: Dict[str, Any],
    schema: Any,
    attempt: int,
    is_repeated: bool,
) -> str:
    del schema
    parts = [f"## oxide_parse IR Validation — Attempt {attempt}"]
    if validation_result.get("base_elements_found", 0) == 0:
        parts.append("The IR schema returned no items or failed validation.")
        if validation_result.get("top_level_structure"):
            parts.append("HTML skeleton:\n```html\n" + str(validation_result["top_level_structure"]) + "\n```")
    details = validation_result.get("field_details") or []
    if details:
        parts.append(
            f"\nField coverage: {validation_result.get('populated_fields', 0)}/{validation_result.get('total_fields', 0)}"
        )
        for fd in details:
            parts.append(f"- {fd['name']}: {fd['populated_count']}/{fd['total_count']} (sample={fd.get('sample_value')!r})")
    if validation_result.get("issues"):
        parts.append("\nIssues:")
        for issue in validation_result["issues"]:
            parts.append(f"- {issue}")
    if validation_result.get("populated_fields", 0) == 0 and validation_result.get("sample_base_html"):
        parts.append("\nHTML snippet:\n```html\n" + str(validation_result["sample_base_html"]) + "\n```")
    if is_repeated:
        parts.append("\nWARNING: You returned the same IR as before. Fix the selectors/ops.")
    parts.append("\nReturn ONLY a corrected valid oxide_parse IR JSON object, nothing else.")
    return "\n".join(parts)


class Crawl4AIPromptPatch:
    """Reversible monkey-patch overriding Crawl4AI schema-generation prompts and validation for `oxide_parse`.

    Use as a context manager (`with migrate.patch_crawl4ai_prompt(): ...`) or manage explicitly via `.restore()`.

    Examples:
        >>> from oxide_parse import prompts
        >>> from oxide_parse.migrate import patch_crawl4ai_prompt
        >>> class DummyStrategy:
        ...     @staticmethod
        ...     def _build_schema_prompt(html, schema_type="CSS", query=None, target_json_example=None):
        ...         return "orig"
        >>> with patch_crawl4ai_prompt(DummyStrategy, output="ir"):
        ...     '"pipe"' in DummyStrategy._build_schema_prompt("<p>x</p>", query="text")
        True
        >>> DummyStrategy._build_schema_prompt("<p>x</p>")
        'orig'
    """

    def __init__(
        self,
        strategy_cls: Any,
        prompts_mod: Any = None,
        *,
        output: Literal["ir", "crawl4ai"] = "ir",
        distill_html: bool = False,
        max_chars: int = 40000,
    ) -> None:
        self._strategy_cls = strategy_cls
        self._prompts_mod = prompts_mod
        self.output = output
        self.distill_html = distill_html
        self.max_chars = max_chars
        self._saved: List[Tuple[Any, str, Any]] = []
        self._active = False

    def _set_attr(self, obj: Any, attr: str, value: Any) -> None:
        raw = obj.__dict__.get(attr, _SENTINEL) if hasattr(obj, "__dict__") else getattr(obj, attr, _SENTINEL)
        self._saved.append((obj, attr, raw))
        setattr(obj, attr, value)

    def install(self) -> "Crawl4AIPromptPatch":
        """Apply the configured prompt/validator overrides onto the target strategy class (idempotent).

        Returns:
            Self for fluent chaining or context management.

        Examples:
            >>> from oxide_parse.migrate import Crawl4AIPromptPatch
            >>> class Dummy:
            ...     pass
            >>> p = Crawl4AIPromptPatch(Dummy).install()
            >>> hasattr(Dummy, "_build_schema_prompt")
            True
            >>> p.restore()
        """
        if self._active:
            return self
        from . import prompts as ox_prompts

        output = self.output
        distill_html = self.distill_html
        max_chars = self.max_chars

        def _build_schema_prompt(
            html: str,
            schema_type: str = "CSS",
            query: Optional[str] = None,
            target_json_example: Optional[str] = None,
        ) -> str:
            return ox_prompts.build_crawl4ai_schema_prompt(
                html,
                schema_type=schema_type,
                query=query,
                target_json_example=target_json_example,
                output=output,
                distill_html=distill_html,
                max_chars=max_chars,
            )

        self._saved.clear()
        self._set_attr(self._strategy_cls, "_build_schema_prompt", staticmethod(_build_schema_prompt))

        orig_validate = getattr(self._strategy_cls, "_validate_schema", None)
        if output == "ir":
            self._set_attr(self._strategy_cls, "_validate_schema", staticmethod(_validate_ir_for_crawl4ai))
            self._set_attr(self._strategy_cls, "_build_feedback_message", staticmethod(_build_ir_feedback))
        else:
            def _validate_strict_crawl4ai(
                schema: Any,
                html_content: str,
                schema_type: str = "CSS",
                expected_fields: Optional[List[str]] = None,
            ) -> Dict[str, Any]:
                kind = "css" if str(schema_type).upper() == "CSS" else "xpath"
                try:
                    ir = from_crawl4ai(schema, kind=kind)
                    from .plan import validate_ir

                    ir_errors = validate_ir(ir, backend="lite")
                except MigrationError as exc:
                    return {
                        "success": False,
                        "base_elements_found": 0,
                        "total_fields": len(schema.get("fields", [])) if isinstance(schema, dict) else 0,
                        "populated_fields": 0,
                        "field_coverage": 0.0,
                        "field_details": [],
                        "issues": [f"oxide_parse compatibility error: {exc}"],
                        "sample_base_html": "",
                        "top_level_structure": _safe_distill(html_content),
                    }
                if ir_errors:
                    return {
                        "success": False,
                        "base_elements_found": 0,
                        "total_fields": len(schema.get("fields", [])) if isinstance(schema, dict) else 0,
                        "populated_fields": 0,
                        "field_coverage": 0.0,
                        "field_details": [],
                        "issues": ir_errors,
                        "sample_base_html": "",
                        "top_level_structure": _safe_distill(html_content),
                    }
                if orig_validate is not None:
                    return orig_validate(schema, html_content, schema_type, expected_fields=expected_fields)
                return _validate_ir_for_crawl4ai(ir, html_content, schema_type, expected_fields=expected_fields)

            self._set_attr(self._strategy_cls, "_validate_schema", staticmethod(_validate_strict_crawl4ai))

        if self._prompts_mod is not None:
            for attr, stype in (("JSON_SCHEMA_BUILDER", "CSS"), ("JSON_SCHEMA_BUILDER_XPATH", "XPATH")):
                if hasattr(self._prompts_mod, attr):
                    self._set_attr(self._prompts_mod, attr, ox_prompts.crawl4ai_schema_prompt(schema_type=stype, output=output))

        self._active = True
        return self

    def restore(self) -> None:
        """Restore all attributes modified by `install()` back to their original values.

        Examples:
            >>> from oxide_parse.migrate import patch_crawl4ai_prompt
            >>> class Dummy:
            ...     pass
            >>> patch = patch_crawl4ai_prompt(Dummy)
            >>> patch.restore()
            >>> hasattr(Dummy, "_build_schema_prompt")
            False
        """
        for obj, attr, prev in reversed(self._saved):
            if prev is _SENTINEL:
                if hasattr(obj, attr):
                    delattr(obj, attr)
            else:
                setattr(obj, attr, prev)
        self._saved.clear()
        self._active = False

    def __enter__(self) -> "Crawl4AIPromptPatch":
        return self.install()

    def __exit__(self, *exc_info: Any) -> None:
        self.restore()


def patch_crawl4ai_prompt(
    target: Any = None,
    *,
    output: Literal["ir", "crawl4ai"] = "ir",
    distill_html: bool = False,
    max_chars: int = 40000,
) -> Crawl4AIPromptPatch:
    """Override Crawl4AI's LLM schema prompt (and validation loop) so it generates `oxide_parse`-ready schemas.

    Installs the patch immediately and returns a `Crawl4AIPromptPatch` handle supporting both `with ...:` and
    explicit `.restore()`. When `output="ir"` (default), `JsonCssExtractionStrategy.generate_schema()` /
    `JsonXPathExtractionStrategy.generate_schema()` directly returns a validated `oxide_parse` IR schema usable with
    `Plan.from_ir(...)` (and accepted idempotently by `from_crawl4ai(...)`).

    Args:
        target: Optional strategy class or module exposing `JsonElementExtractionStrategy`. When `None`, imports
            `crawl4ai.extraction_strategy` and `crawl4ai.prompts`.
        output: `"ir"` (default) generates native `oxide_parse` JSON IR directly; `"crawl4ai"` keeps Crawl4AI JSON format
            while disallowing features unsupported by `from_crawl4ai()` (`computed`, `source`).
        distill_html: Run `oxide_parse.distill()` on HTML before sending it to the LLM (default `False`).
        max_chars: HTML character cap passed to prompt generation (default `40000`).

    Returns:
        Active `Crawl4AIPromptPatch` context/lifecycle guard.

    Raises:
        ImportError: If `target` is `None` and the `crawl4ai` package is not installed.
        TypeError: If `target` does not expose a strategy class or `_build_schema_prompt`.

    Examples:
        >>> from oxide_parse.migrate import patch_crawl4ai_prompt
        >>> class Strategy:
        ...     pass
        >>> with patch_crawl4ai_prompt(Strategy, output="ir"):
        ...     '"css"' in Strategy._build_schema_prompt("<b>hi</b>")
        True
    """
    if output not in ("ir", "crawl4ai"):
        raise ValueError('output must be "ir" or "crawl4ai"')

    prompts_mod: Any = None
    strategy_cls: Any
    if target is None:
        try:
            import crawl4ai.extraction_strategy as es_mod  # type: ignore[import-not-found]
        except ImportError as exc:
            raise ImportError(
                "crawl4ai is not installed; pass a strategy class/module explicitly or install crawl4ai"
            ) from exc
        strategy_cls = es_mod.JsonElementExtractionStrategy
        try:
            import crawl4ai.prompts as pr_mod  # type: ignore[import-not-found]

            prompts_mod = pr_mod
        except ImportError:
            prompts_mod = None
    elif hasattr(target, "JsonElementExtractionStrategy"):
        strategy_cls = target.JsonElementExtractionStrategy
        prompts_mod = getattr(target, "prompts", None)
    elif isinstance(target, type) or hasattr(target, "_build_schema_prompt") or hasattr(target, "generate_schema"):
        strategy_cls = target
    else:
        raise TypeError("target must be None, a Crawl4AI strategy class, or a module with JsonElementExtractionStrategy")

    return Crawl4AIPromptPatch(
        strategy_cls,
        prompts_mod,
        output=output,
        distill_html=distill_html,
        max_chars=max_chars,
    ).install()
