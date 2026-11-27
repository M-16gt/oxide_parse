"""Live Crawl4AI + Gemini schema-generation and extraction-parity tests.

Drop your Gemini API key into `.secrets/crawl4ai_gemini.json` (gitignored) or set
`GEMINI_API_KEY` / `GOOGLE_API_KEY`. Set `OXIDE_CRAWL4AI_GEMINI_MODEL` to override
the model in the local configuration, then run:

    pytest tests/test_crawl4ai_gemini_live.py -s -v
"""
from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

import pytest

from oxide_parse import Plan, migrate, prompts, to_ir, validate_ir

_ROOT = Path(__file__).resolve().parents[1]
_SECRETS_CONFIG = _ROOT / ".secrets" / "crawl4ai_gemini.json"
_EXAMPLE_CONFIG = _ROOT / "tests" / "crawl4ai_gemini.example.json"
_PLACEHOLDERS = {"", "PASTE_YOUR_GEMINI_API_KEY_HERE", "YOUR_API_KEY"}
_BRIDGE_MARKER = "__OXIDE_CRAWL4AI_BRIDGE_JSON__"

_DEFAULTS: Dict[str, Any] = {
    "api_key": "",
    "model": "gemini-3.5-flash-lite",
    "url": "https://books.toscrape.com/",
    "query": "Extract every book card on the page with its title, price, availability, and detail link href.",
    "schema_type": "CSS",
    "prompt_mode": "crawl4ai",
    "validate_schema": True,
    "timeout_sec": 60,
}

_BRIDGE_CODE = """
import json
import os
import sys

payload = json.load(sys.stdin)
os.environ["CRAWL4_AI_BASE_DIRECTORY"] = payload["workdir"]
os.environ["LITELLM_LOCAL_MODEL_COST_MAP"] = "True"

from crawl4ai import CrawlerRunConfig, JsonCssExtractionStrategy, JsonXPathExtractionStrategy, LLMConfig

patch = None
prompt_mode = payload.get("prompt_mode", "crawl4ai")
if payload.get("patch_prompt") or prompt_mode == "ir":
    oxide_py = payload.get("oxide_python")
    if oxide_py and oxide_py not in sys.path:
        sys.path.insert(0, oxide_py)
    from oxide_parse.migrate import patch_crawl4ai_prompt

    patch = patch_crawl4ai_prompt(output=prompt_mode)

schema_type = payload["schema_type"]
Strategy = JsonCssExtractionStrategy if schema_type == "CSS" else JsonXPathExtractionStrategy
llm_config = LLMConfig(provider=payload["provider"], api_token=payload["api_key"])
try:
    schema = Strategy.generate_schema(
        html=payload["html"],
        schema_type=schema_type,
        query=payload["query"],
        llm_config=llm_config,
        validate=bool(payload["validate_schema"]),
    )
finally:
    if patch is not None:
        patch.restore()

if prompt_mode == "ir":
    items = []
    exported = None
else:
    extractor = Strategy(schema)
    items = extractor.extract(payload["url"], payload["html"])
    exported = CrawlerRunConfig(extraction_strategy=extractor).dump()
print(payload["marker"])
print(json.dumps({"schema": schema, "items": items, "exported": exported}, ensure_ascii=False))
"""


def _compat_python_path() -> Optional[Path]:
    for rel in (
        Path(".migration/crawl4ai-compat/Scripts/python.exe"),
        Path(".migration/crawl4ai-compat/bin/python"),
    ):
        candidate = _ROOT / rel
        if candidate.is_file():
            return candidate
    return None


def load_gemini_config(config_path: Optional[Path] = None) -> Dict[str, Any]:
    """Load Gemini + Crawl4AI test configuration from JSON and environment variables."""
    if config_path is not None:
        resolved = Path(config_path)
    elif os.environ.get("OXIDE_CRAWL4AI_GEMINI_CONFIG"):
        resolved = Path(os.environ["OXIDE_CRAWL4AI_GEMINI_CONFIG"])
    elif _SECRETS_CONFIG.is_file():
        resolved = _SECRETS_CONFIG
    else:
        resolved = _EXAMPLE_CONFIG

    cfg = dict(_DEFAULTS)
    if resolved.is_file():
        raw = json.loads(resolved.read_text(encoding="utf-8"))
        if not isinstance(raw, dict):
            raise ValueError(f"Expected JSON object in {resolved}")
        for key, value in raw.items():
            if value is not None:
                cfg[key] = value

    api_key = str(cfg.get("api_key", "")).strip()
    if api_key in _PLACEHOLDERS:
        api_key = (os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY") or "").strip()
        if api_key in _PLACEHOLDERS:
            api_key = ""
    cfg["api_key"] = api_key

    schema_type = str(cfg.get("schema_type", "CSS")).strip().upper()
    if schema_type not in ("CSS", "XPATH"):
        raise ValueError(f"Unsupported schema_type {schema_type!r}; expected 'CSS' or 'XPATH'")
    cfg["schema_type"] = schema_type
    prompt_override = os.environ.get("OXIDE_CRAWL4AI_PROMPT_MODE", "").strip().lower()
    prompt_mode = prompt_override or str(cfg.get("prompt_mode") or _DEFAULTS["prompt_mode"]).strip().lower()
    if prompt_mode not in ("crawl4ai", "ir"):
        raise ValueError(f"Unsupported prompt_mode {prompt_mode!r}; expected 'crawl4ai' or 'ir'")
    cfg["prompt_mode"] = prompt_mode
    model_override = os.environ.get("OXIDE_CRAWL4AI_GEMINI_MODEL", "").strip()
    cfg["model"] = model_override or str(cfg.get("model") or _DEFAULTS["model"]).strip()
    cfg["url"] = str(cfg.get("url") or _DEFAULTS["url"]).strip()
    cfg["query"] = str(cfg.get("query") or _DEFAULTS["query"]).strip()
    cfg["validate_schema"] = bool(cfg.get("validate_schema", True))
    cfg["timeout_sec"] = int(cfg.get("timeout_sec", 60))
    return cfg


def fetch_live_html(url: str, timeout_sec: int = 60) -> str:
    """Fetch raw HTML over HTTP with a polite User-Agent."""
    req = urllib.request.Request(
        url,
        headers={
            "User-Agent": "oxide-parse-crawl4ai-test/0.9 (+https://github.com/oxide-parse)",
            "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        },
    )
    with urllib.request.urlopen(req, timeout=timeout_sec) as resp:
        charset = resp.headers.get_content_charset() or "utf-8"
        return resp.read().decode(charset, errors="replace")


def _get_local_crawl4ai_strategies() -> Tuple[type, type]:
    root_str = str(_ROOT)
    if root_str not in sys.path:
        sys.path.insert(0, root_str)
    from benches.suite_v7.competitors import get_crawl4ai_strategies

    strategies = get_crawl4ai_strategies()
    if strategies is None:
        raise RuntimeError("Crawl4AI strategies are unavailable (neither crawl4ai nor .migration/crawl4ai-strategy.py)")
    return strategies


def _strip_markdown_fences(text: str) -> str:
    cleaned = text.strip()
    return re.sub(r"^```(?:[a-zA-Z0-9_-]+)?\s*|```$", "", cleaned, flags=re.MULTILINE).strip()


def _generate_schema_via_gemini_rest(html: str, cfg: Dict[str, Any], bare_model: str) -> Dict[str, Any]:
    endpoint = f"https://generativelanguage.googleapis.com/v1beta/models/{bare_model}:generateContent"
    prompt_mode = cfg.get("prompt_mode", "crawl4ai")
    prompt = prompts.build_crawl4ai_schema_prompt(
        html=html[:80000],
        schema_type=cfg["schema_type"],
        query=cfg["query"],
        output="ir" if prompt_mode == "ir" else "crawl4ai",
    )
    body = json.dumps(
        {
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {"responseMimeType": "application/json"},
        }
    ).encode("utf-8")
    req = urllib.request.Request(
        endpoint,
        data=body,
        headers={
            "Content-Type": "application/json",
            "x-goog-api-key": cfg["api_key"],
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=cfg["timeout_sec"]) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        detail = exc.read().decode("utf-8", errors="replace")[:500]
        raise RuntimeError(f"Gemini REST request failed (HTTP {exc.code}): {detail}") from exc

    text = payload["candidates"][0]["content"]["parts"][0]["text"]
    return json.loads(_strip_markdown_fences(text))


def run_crawl4ai_gemini(
    html: str,
    cfg: Dict[str, Any],
    workdir: Path,
) -> Tuple[Dict[str, Any], List[Dict[str, Any]], Optional[Dict[str, Any]]]:
    """Generate a Crawl4AI schema with Gemini and run Crawl4AI extraction on `html`."""
    raw_model = cfg["model"]
    provider = raw_model if "/" in raw_model else f"gemini/{raw_model}"
    bare_model = raw_model.split("/", 1)[-1]

    # Tier 1: in-process crawl4ai package
    try:
        import crawl4ai  # type: ignore[import-not-found]
    except ImportError:
        crawl4ai = None

    if crawl4ai is not None:
        os.environ.setdefault("CRAWL4_AI_BASE_DIRECTORY", str(workdir))
        os.environ.setdefault("LITELLM_LOCAL_MODEL_COST_MAP", "True")
        strategy_cls = (
            crawl4ai.JsonCssExtractionStrategy
            if cfg["schema_type"] == "CSS"
            else crawl4ai.JsonXPathExtractionStrategy
        )
        llm_config = crawl4ai.LLMConfig(provider=provider, api_token=cfg["api_key"])
        if cfg.get("prompt_mode") == "ir":
            with migrate.patch_crawl4ai_prompt(output="ir"):
                schema = strategy_cls.generate_schema(
                    html=html,
                    schema_type=cfg["schema_type"],
                    query=cfg["query"],
                    llm_config=llm_config,
                    validate=cfg["validate_schema"],
                )
            return schema, [], None
        schema = strategy_cls.generate_schema(
            html=html,
            schema_type=cfg["schema_type"],
            query=cfg["query"],
            llm_config=llm_config,
            validate=cfg["validate_schema"],
        )
        extractor = strategy_cls(schema)
        items = extractor.extract(cfg["url"], html)
        exported = crawl4ai.CrawlerRunConfig(extraction_strategy=extractor).dump()
        return schema, items, exported

    # Tier 2: dedicated .migration/crawl4ai-compat environment (credentials passed via stdin only)
    compat_py = _compat_python_path()
    if compat_py is not None:
        bridge_payload = {
            "api_key": cfg["api_key"],
            "provider": provider,
            "schema_type": cfg["schema_type"],
            "prompt_mode": cfg.get("prompt_mode", "crawl4ai"),
            "oxide_python": str(_ROOT / "python"),
            "query": cfg["query"],
            "url": cfg["url"],
            "html": html,
            "validate_schema": cfg["validate_schema"],
            "workdir": str(workdir),
            "marker": _BRIDGE_MARKER,
        }
        proc = subprocess.run(
            [str(compat_py), "-c", _BRIDGE_CODE],
            input=json.dumps(bridge_payload, ensure_ascii=False),
            text=True,
            capture_output=True,
            timeout=max(120, cfg["timeout_sec"] * 3),
            encoding="utf-8",
            errors="replace",
        )
        if proc.returncode != 0:
            err_tail = (proc.stderr or proc.stdout or "").strip()[-1000:]
            raise RuntimeError(f"Crawl4AI compat bridge failed (exit {proc.returncode}): {err_tail}")
        stdout = proc.stdout or ""
        marker_idx = stdout.rfind(_BRIDGE_MARKER)
        if marker_idx == -1:
            raise RuntimeError(f"Crawl4AI bridge did not emit marker output: {stdout[-500:]}")
        json_tail = stdout[marker_idx + len(_BRIDGE_MARKER) :].strip()
        result = json.loads(json_tail)
        return result["schema"], result["items"], result.get("exported")

    # Tier 3: direct Gemini REST API + local Crawl4AI strategy snapshot
    schema = _generate_schema_via_gemini_rest(html, cfg, bare_model)
    if cfg.get("prompt_mode") == "ir":
        return schema, [], None
    css_cls, xpath_cls = _get_local_crawl4ai_strategies()
    strategy_cls = css_cls if cfg["schema_type"] == "CSS" else xpath_cls
    items = strategy_cls(schema).extract(cfg["url"], html)
    return schema, items, None


def _strip_none_fields(value: Any) -> Any:
    if isinstance(value, dict):
        return {k: _strip_none_fields(v) for k, v in value.items() if v is not None}
    if isinstance(value, list):
        return [_strip_none_fields(item) for item in value]
    return value


def _normalize_crawl4ai_ref(ref: Any, oxide_val: Any) -> Any:
    if isinstance(ref, dict) and isinstance(oxide_val, dict):
        return {k: _normalize_crawl4ai_ref(v, oxide_val.get(k)) for k, v in ref.items()}
    if isinstance(ref, list) and isinstance(oxide_val, str) and all(isinstance(x, str) for x in ref):
        # BeautifulSoup returns multi-valued attributes like `class` / `rel` as lists of tokens
        return " ".join(ref)
    if isinstance(ref, list) and isinstance(oxide_val, list):
        return [
            _normalize_crawl4ai_ref(r, oxide_val[i] if i < len(oxide_val) else None)
            for i, r in enumerate(ref)
        ]
    return ref


def _normalize_for_crawl4ai_comparison(
    oxide_records: List[Dict[str, Any]],
    crawl4ai_ref: List[Dict[str, Any]],
) -> Tuple[List[Dict[str, Any]], List[Dict[str, Any]]]:
    stripped = [_strip_none_fields(item) for item in oxide_records]
    non_empty_oxide = [item for item in stripped if item]
    normalized_ref = [
        _normalize_crawl4ai_ref(ref_item, non_empty_oxide[i] if i < len(non_empty_oxide) else None)
        for i, ref_item in enumerate(crawl4ai_ref)
    ]
    return non_empty_oxide, normalized_ref


def verify_crawl4ai_to_oxide_parity(
    html: str,
    url: str,
    schema_type: str,
    schema: Dict[str, Any],
    crawl4ai_items: List[Dict[str, Any]],
    exported_config: Optional[Dict[str, Any]] = None,
    *,
    prompt_mode: str = "crawl4ai",
) -> Dict[str, Any]:
    """Validate schema migration, IR validity, Python DSL roundtrip, and extraction parity."""
    assert isinstance(schema, dict)
    kind = "css" if schema_type.upper() == "CSS" else "xpath"
    if prompt_mode == "ir":
        ir = schema
        assert validate_ir(ir) == []
        assert migrate.from_crawl4ai(ir, kind=kind) == ir
        code = migrate.from_crawl4ai_code(schema, kind=kind)
        ns: Dict[str, Any] = {}
        exec(code, ns)
        assert to_ir(ns["schema"]) == ir

        lite_out = Plan.from_ir(ir, "lite").extract(html)
        scraper_out = Plan.from_ir(ir, "scraper").extract(html)
        auto_plan = Plan.from_ir(ir, "auto")
        assert lite_out == scraper_out == auto_plan.extract(html)
        records = lite_out if isinstance(lite_out, list) else ([lite_out] if isinstance(lite_out, dict) else [])
        assert len(records) > 0, f"Direct IR schema extracted 0 items: {lite_out!r}"
        return {
            "url": url,
            "engine": auto_plan.engine,
            "item_count": len(records),
            "exact_raw_match": True,
            "code": code,
            "sample": records[0],
        }

    assert schema.get("baseSelector"), f"Generated schema lacks baseSelector: {schema!r}"
    assert isinstance(schema.get("fields"), list) and len(schema["fields"]) > 0, f"Empty fields: {schema!r}"
    assert isinstance(crawl4ai_items, list) and len(crawl4ai_items) > 0, "Crawl4AI extracted 0 items"
    assert any(
        val not in (None, "", [], {})
        for item in crawl4ai_items
        for val in item.values()
    ), f"All extracted Crawl4AI fields were empty: {crawl4ai_items!r}"

    ir = migrate.from_crawl4ai(schema, kind=kind)
    if exported_config is not None:
        assert migrate.from_crawl4ai(exported_config) == ir
    assert validate_ir(ir) == []

    code = migrate.from_crawl4ai_code(schema, kind=kind)
    ns = {}
    exec(code, ns)
    assert to_ir(ns["schema"]) == ir

    lite_out = Plan.from_ir(ir, "lite").extract(html)
    scraper_out = Plan.from_ir(ir, "scraper").extract(html)
    auto_plan = Plan.from_ir(ir, "auto")
    auto_out = auto_plan.extract(html)
    assert lite_out == scraper_out == auto_out

    norm_oxide, norm_crawl4ai = _normalize_for_crawl4ai_comparison(lite_out, crawl4ai_items)
    assert norm_oxide == norm_crawl4ai

    return {
        "url": url,
        "engine": auto_plan.engine,
        "item_count": len(lite_out),
        "exact_raw_match": lite_out == crawl4ai_items,
        "code": code,
        "sample": lite_out[0],
    }


def test_gemini_config_loading_and_placeholders(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    example = json.loads(_EXAMPLE_CONFIG.read_text(encoding="utf-8"))
    for required_key in ("api_key", "model", "url", "query", "schema_type", "validate_schema", "timeout_sec"):
        assert required_key in example
    assert example["api_key"] == ""

    monkeypatch.delenv("GEMINI_API_KEY", raising=False)
    monkeypatch.delenv("GOOGLE_API_KEY", raising=False)
    monkeypatch.delenv("OXIDE_CRAWL4AI_GEMINI_CONFIG", raising=False)
    monkeypatch.delenv("OXIDE_CRAWL4AI_GEMINI_MODEL", raising=False)

    placeholder_cfg = tmp_path / "cfg.json"
    placeholder_cfg.write_text(
        json.dumps({"api_key": "PASTE_YOUR_GEMINI_API_KEY_HERE", "schema_type": "css"}),
        encoding="utf-8",
    )
    loaded = load_gemini_config(placeholder_cfg)
    assert loaded["api_key"] == ""
    assert loaded["schema_type"] == "CSS"
    assert loaded["model"] == example["model"]

    monkeypatch.setenv("GEMINI_API_KEY", "env-test-key")
    assert load_gemini_config(placeholder_cfg)["api_key"] == "env-test-key"

    placeholder_cfg.write_text(json.dumps({"api_key": "file-test-key"}), encoding="utf-8")
    assert load_gemini_config(placeholder_cfg)["api_key"] == "file-test-key"

    monkeypatch.setenv("OXIDE_CRAWL4AI_GEMINI_MODEL", "gemini-3.8-flash")
    placeholder_cfg.write_text(json.dumps({"model": "gemini-2.5-flash"}), encoding="utf-8")
    assert load_gemini_config(placeholder_cfg)["model"] == "gemini-3.8-flash"


def test_parity_pipeline_offline_mocked(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]) -> None:
    pytest.importorskip("oxide_parse._core")

    html = (
        '<section class="catalog">'
        '<article class="book-card" data-id="101">'
        '<h3>  Rust Parsing  </h3>'
        '<span class="price">£19.99</span>'
        '<a class="detail primary" href="/book/rust-parsing">Details</a>'
        '<ul class="tags"><li>rust</li><li>html</li></ul>'
        "</article>"
        '<article class="book-card" data-id="102">'
        '<h3>CSS Selectors</h3>'
        '<span class="price">£12.50</span>'
        '<ul class="tags"><li>css</li></ul>'
        "</article>"
        "</section>"
    )
    # Nullable/blank optional fields that LLM generators can emit
    nullable_schema: Dict[str, Any] = {
        "name": "Book Cards",
        "baseSelector": "article.book-card",
        "baseFields": None,
        "fields": [
            {"name": "title", "selector": "h3", "type": "text", "transform": ""},
            {"name": "price", "selector": ".price", "type": "regex", "pattern": r"£([\d.]+)"},
            {"name": "href", "selector": "a.detail", "type": "attribute", "attribute": "href", "source": None},
            {"name": "link_classes", "selector": "a.detail", "type": "attribute", "attribute": "class"},
            {"name": "tags", "selector": "ul.tags li", "type": "list", "fields": [{"name": "label", "type": "text"}]},
        ],
    }
    nullable_ir = migrate.from_crawl4ai(nullable_schema, kind="css")
    assert validate_ir(nullable_ir) == []

    llm_schema: Dict[str, Any] = {
        "name": "Book Cards",
        "baseSelector": "article.book-card",
        "baseFields": [],
        "fields": [
            {"name": "title", "selector": "h3", "type": "text", "transform": ""},
            {"name": "price", "selector": ".price", "type": "regex", "pattern": r"£([\d.]+)"},
            {"name": "href", "selector": "a.detail", "type": "attribute", "attribute": "href"},
            {"name": "link_classes", "selector": "a.detail", "type": "attribute", "attribute": "class"},
            {"name": "tags", "selector": "ul.tags li", "type": "list", "fields": [{"name": "label", "type": "text"}]},
        ],
    }

    # Crawl4AI omits missing fields and returns HTML class attributes as token lists.
    crawl4ai_items = [
        {
            "title": "Rust Parsing",
            "price": "19.99",
            "href": "/book/rust-parsing",
            "link_classes": ["detail", "primary"],
            "tags": [{"label": "rust"}, {"label": "html"}],
        },
        {
            "title": "CSS Selectors",
            "price": "12.50",
            "tags": [{"label": "css"}],
        },
    ]
    summary = verify_crawl4ai_to_oxide_parity(
        html=html,
        url="https://books.toscrape.com/",
        schema_type="CSS",
        schema=llm_schema,
        crawl4ai_items=crawl4ai_items,
    )
    assert summary["item_count"] == 2
    assert summary["sample"]["title"] == "Rust Parsing"
    assert summary["sample"]["price"] == "19.99"

    # Exercise the live entry point with mocked HTTP, Gemini, and Crawl4AI.
    monkeypatch.setattr(
        sys.modules[__name__],
        "load_gemini_config",
        lambda config_path=None: {**_DEFAULTS, "api_key": "mock-key"},
    )
    monkeypatch.setattr(sys.modules[__name__], "fetch_live_html", lambda url, timeout_sec=60: html)
    monkeypatch.setattr(
        sys.modules[__name__],
        "run_crawl4ai_gemini",
        lambda raw_html, cfg, workdir: (llm_schema, crawl4ai_items, None),
    )
    test_crawl4ai_live_request_and_gemini_schema(tmp_path)
    captured = capsys.readouterr().out
    assert "=== Crawl4AI + Gemini Live Parity Report ===" in captured
    assert "Items extracted: 2" in captured


def test_crawl4ai_live_request_and_gemini_schema(tmp_path: Path) -> None:
    pytest.importorskip("oxide_parse._core")
    cfg = load_gemini_config()
    if not cfg["api_key"]:
        pytest.skip("Paste your Gemini API key into .secrets/crawl4ai_gemini.json (or set GEMINI_API_KEY) to run live test.")

    html = fetch_live_html(cfg["url"], timeout_sec=cfg["timeout_sec"])
    schema, crawl4ai_items, exported = run_crawl4ai_gemini(html, cfg, tmp_path)
    summary = verify_crawl4ai_to_oxide_parity(
        html=html,
        url=cfg["url"],
        schema_type=cfg["schema_type"],
        schema=schema,
        crawl4ai_items=crawl4ai_items,
        exported_config=exported,
        prompt_mode=cfg.get("prompt_mode", "crawl4ai"),
    )

    print("\n=== Crawl4AI + Gemini Live Parity Report ===")
    print("URL:", cfg["url"])
    print("HTML bytes:", len(html.encode("utf-8")))
    print("Gemini model:", cfg["model"])
    print("Generated schema:", json.dumps(schema, indent=2, ensure_ascii=False))
    print("Generated oxide_parse Python DSL:\n" + summary["code"])
    print(f"Items extracted: {summary['item_count']} (auto engine: {summary['engine']}, exact_raw_match: {summary['exact_raw_match']})")
    print("Sample item:", json.dumps(summary["sample"], ensure_ascii=False))
