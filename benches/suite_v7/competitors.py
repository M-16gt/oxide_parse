"""Unified adapters and version discovery for oxide_parse and competing libraries."""
from __future__ import annotations

import ast
import importlib.metadata
from pathlib import Path
import sys
from typing import Dict, Optional, Tuple
import warnings

AVAILABLE_LIBS: Dict[str, str] = {}
SKIPPED: Dict[str, str] = {}

_ROOT = Path(__file__).resolve().parents[2]
_MIGRATION_DEPS = _ROOT / ".migration" / "v8_5-bench-deps"
_CRAWL4AI_STRATEGY_SRC = _ROOT / ".migration" / "crawl4ai-strategy.py"
_CRAWL4AI_CACHE: Optional[Tuple[type, type]] = None


def ensure_local_bench_deps() -> None:
    """Include local vendored bench dependencies (markdownify, html2text) if present."""
    if _MIGRATION_DEPS.is_dir():
        path_str = str(_MIGRATION_DEPS)
        if path_str not in sys.path:
            sys.path.append(path_str)


def get_crawl4ai_strategies() -> Optional[Tuple[type, type]]:
    """Return (JsonCssExtractionStrategy, JsonXPathExtractionStrategy) from installed crawl4ai or local snapshot."""
    global _CRAWL4AI_CACHE
    if _CRAWL4AI_CACHE is not None:
        return _CRAWL4AI_CACHE
    try:
        from crawl4ai import JsonCssExtractionStrategy, JsonXPathExtractionStrategy

        _CRAWL4AI_CACHE = (JsonCssExtractionStrategy, JsonXPathExtractionStrategy)
        return _CRAWL4AI_CACHE
    except Exception:
        pass
    if not _CRAWL4AI_STRATEGY_SRC.is_file():
        return None
    try:
        from bs4 import BeautifulSoup  # noqa: F401
        from lxml import etree, html  # noqa: F401

        module = ast.parse(_CRAWL4AI_STRATEGY_SRC.read_text(encoding="utf-8"))
        names = {
            "ExtractionStrategy",
            "JsonElementExtractionStrategy",
            "JsonCssExtractionStrategy",
            "JsonXPathExtractionStrategy",
        }
        classes = [node for node in module.body if isinstance(node, ast.ClassDef) and node.name in names]
        for cls in classes:
            if cls.name == "JsonElementExtractionStrategy":
                cls.body = [
                    node
                    for node in cls.body
                    if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) or node.lineno < 1320
                ]
        ns: Dict[str, object] = {}
        preamble = (
            "from abc import ABC, abstractmethod\n"
            "import json\n"
            "import re\n"
            "from typing import Any, Dict, List, Optional, Tuple, Union\n"
            "from bs4 import BeautifulSoup\n"
            "from lxml import etree, html\n"
        )
        exec(compile(preamble, str(_CRAWL4AI_STRATEGY_SRC), "exec"), ns)
        exec(compile(ast.Module(body=classes, type_ignores=[]), str(_CRAWL4AI_STRATEGY_SRC), "exec"), ns)
        warnings.filterwarnings("ignore", category=FutureWarning, message=".*Truth-testing of elements.*")
        _CRAWL4AI_CACHE = (ns["JsonCssExtractionStrategy"], ns["JsonXPathExtractionStrategy"])  # type: ignore[assignment]
        return _CRAWL4AI_CACHE
    except Exception:
        return None


def detect_libraries() -> Dict[str, str]:
    """Detect installed parsing libraries and their exact versions."""
    ensure_local_bench_deps()
    SKIPPED.clear()
    pkgs = [
        ("oxide_parse", "oxide-parse"),
        ("frostwork", "frostwork"),
        ("selectolax", "selectolax"),
        ("lxml", "lxml"),
        ("parsel", "parsel"),
        ("scrapling", "scrapling"),
        ("bs4", "beautifulsoup4"),
        ("markdownify", "markdownify"),
        ("html2text", "html2text"),
    ]
    found: Dict[str, str] = {}
    for mod_name, dist_name in pkgs:
        try:
            mod = __import__(mod_name)
            try:
                ver = importlib.metadata.version(dist_name)
            except importlib.metadata.PackageNotFoundError:
                ver = getattr(mod, "__version__", "installed")
            found[mod_name] = ver
        except Exception as exc:
            record_skip(mod_name, exc)
    try:
        strategies = get_crawl4ai_strategies()
        if strategies is not None:
            try:
                found["crawl4ai"] = importlib.metadata.version("crawl4ai")
            except importlib.metadata.PackageNotFoundError:
                found["crawl4ai"] = "local-strategy"
        else:
            record_skip("crawl4ai", "crawl4ai not installed and local snapshot unavailable")
    except Exception as exc:
        record_skip("crawl4ai", exc)
    AVAILABLE_LIBS.clear()
    AVAILABLE_LIBS.update(found)
    return found


def has_lib(name: str) -> bool:
    return name in AVAILABLE_LIBS


def record_skip(name: str, reason: object) -> None:
    SKIPPED[name] = str(reason)
