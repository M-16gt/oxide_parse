"""Runs the Python examples of README.md and docs/guide/*.md, so that the documentation cannot drift from the API.

Rules (also in docs/index.md):
* every ```python block of a file runs in order, in one namespace per file (a later block may use `html`, `plan`...);
* a block that contains `# skip-doctest` is not run (async code, real network, pseudo-code);
* if a ```text block follows a python block directly (only blank lines between them), it is the expected stdout of
  that block and is compared line by line (trailing spaces ignored).
"""
from __future__ import annotations

import contextlib
import io
import pathlib
import re
import subprocess
import sys

import pytest

ROOT = pathlib.Path(__file__).resolve().parents[1]
FILES = [ROOT / "README.md", *sorted((ROOT / "docs" / "guide").glob("*.md"))]
FENCE = re.compile(r"^```([\w-]*)[^\n]*\n(.*?)^```[ \t]*$", re.S | re.M)
MD_LINK = re.compile(r"(?<!!)\[[^\]]+\]\(([^)]+)\)")

EXPECTED_DOC_FILES = [
    "README.md",
    "CHANGELOG.md",
    "CLAUDE.md",
    "llms.txt",
    "llms-full.txt",
    "docs/index.md",
    "docs/guide/01_quickstart.md",
    "docs/guide/02_dsl_cheatsheet.md",
    "docs/guide/03_choosing_engine.md",
    "docs/guide/04_hybrid_html_json.md",
    "docs/guide/05_adaptive_and_repair.md",
    "docs/guide/06_follow_and_crawling.md",
    "docs/guide/07_llm_rag_markdown.md",
    "docs/ai/prompt_generate_schema.md",
    "docs/ai/prompt_migrate.md",
    "docs/ai/prompt_self_healing.md",
    "docs/reference/python_api.md",
    "docs/reference/ir_spec.md",
    "docs/reference/selectors_and_xpath.md",
    "docs/reference/cli.md",
    "docs/architecture/overview.md",
    "docs/architecture/conformance.md",
    "docs/architecture/benchmarks.md",
    "docs/architecture/publishing.md",
    "docs/archive/audit_v7_v8.md",
]


def blocks(text: str):
    """(lang, code, gap_before) for every fenced block; `gap_before` is the text since the previous block."""
    out, pos = [], 0
    for m in FENCE.finditer(text):
        out.append((m.group(1).lower(), m.group(2), text[pos:m.start()]))
        pos = m.end()
    return out


def norm(s: str) -> list:
    return [line.rstrip() for line in s.strip("\n").splitlines()]


@pytest.mark.parametrize("path", FILES, ids=lambda p: str(p.relative_to(ROOT)).replace("\\", "/"))
def test_examples_run(path: pathlib.Path):
    pytest.importorskip("oxide_parse._core")
    items = blocks(path.read_text(encoding="utf-8"))
    ns: dict = {"__name__": "__docs__"}
    ran = 0
    for i, (lang, code, _gap) in enumerate(items):
        if lang != "python" or "# skip-doctest" in code:
            continue
        out = io.StringIO()
        try:
            with contextlib.redirect_stdout(out):
                exec(compile(code, f"{path.name}[block {i}]", "exec"), ns)
        except Exception as e:  # pragma: no cover - the message is the point
            pytest.fail(f"{path.name}, block {i} failed: {type(e).__name__}: {e}\n---\n{code}")
        ran += 1
        if i + 1 < len(items):
            nlang, ncode, ngap = items[i + 1]
            if nlang == "text" and not ngap.strip():
                assert norm(out.getvalue()) == norm(ncode), f"{path.name}, block {i}: output differs from the text block"
    assert ran, f"{path.name}: no runnable python blocks"


def test_readme_length_limit():
    lines = (ROOT / "README.md").read_text(encoding="utf-8").splitlines()
    assert len(lines) <= 280, f"README.md has {len(lines)} lines (limit: 280)"


def test_docs_tree_complete():
    missing = [rel for rel in EXPECTED_DOC_FILES if not (ROOT / rel).is_file()]
    assert not missing, f"missing documentation files: {missing}"


def test_relative_markdown_links():
    doc_files = [ROOT / "README.md", *sorted((ROOT / "docs").rglob("*.md"))]
    broken = []
    for doc in doc_files:
        text = FENCE.sub("", doc.read_text(encoding="utf-8"))
        text = re.sub(r"`[^`\n]+`", "", text)
        for target in MD_LINK.findall(text):
            target = target.strip().split("#", 1)[0]
            if not target or target.startswith(("http://", "https://", "mailto:")):
                continue
            resolved = (doc.parent / target).resolve()
            if not resolved.exists():
                broken.append(f"{doc.relative_to(ROOT)} -> {target}")
    assert not broken, f"broken relative markdown links: {broken}"


def test_llms_txt_up_to_date():
    llms = ROOT / "llms.txt"
    assert llms.exists()
    assert len(llms.read_bytes()) <= 4096, "llms.txt exceeds 4096 bytes"
    res = subprocess.run(
        [sys.executable, str(ROOT / "scripts" / "build_llms_txt.py"), "--check"],
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    assert res.returncode == 0, res.stderr or res.stdout


def test_docs_and_llms_are_english_only():
    cyrillic = re.compile(r"[\u0400-\u04FF]")
    targets = [
        ROOT / "README.md",
        ROOT / "CLAUDE.md",
        ROOT / "llms.txt",
        ROOT / "llms-full.txt",
        ROOT / "benches" / "suite_v7" / "README_BENCHMARKS.md",
        *sorted(p for p in (ROOT / "docs").rglob("*.md") if "archive" not in p.parts),
    ]
    hits = []
    for path in targets:
        for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            if cyrillic.search(line):
                hits.append(f"{path.relative_to(ROOT)}:{lineno}: {line.strip()}")
                break
    assert not hits, f"non-English Cyrillic text found in docs: {hits}"


def test_public_docstrings_have_examples_and_doctests_pass():
    import doctest
    import importlib
    import inspect

    from oxide_parse import Expr, Plan, Session

    modules = [
        "oxide_parse",
        "oxide_parse.expr",
        "oxide_parse.formats",
        "oxide_parse.plan",
        "oxide_parse.crawl",
        "oxide_parse.decompile",
        "oxide_parse.migrate",
        "oxide_parse.prompts",
        "oxide_parse.ir_spec",
        "oxide_parse.__main__",
    ]
    missing_doc = []
    missing_example = []
    for mod_name in modules:
        mod = importlib.import_module(mod_name)
        if not (mod.__doc__ and mod.__doc__.strip()):
            missing_doc.append(mod_name)
        for name, obj in inspect.getmembers(mod):
            if name.startswith("_") or getattr(obj, "__module__", None) != mod_name:
                continue
            doc = getattr(obj, "__doc__", None) or ""
            if not doc.strip():
                missing_doc.append(f"{mod_name}.{name}")
            elif inspect.isfunction(obj) and ("Examples:" not in doc or ">>>" not in doc):
                missing_example.append(f"{mod_name}.{name}")
    for cls in (Expr, Plan, Session):
        for name, member in vars(cls).items():
            if name.startswith("_") and name not in ("__init__", "__getitem__"):
                continue
            if isinstance(member, property):
                doc = member.__doc__ or ""
                target: object = member.fget
            elif isinstance(member, (classmethod, staticmethod)):
                fn = member.__func__
                doc = fn.__doc__ or ""
                target = fn
            elif callable(member):
                doc = member.__doc__ or ""
                target = member
            else:
                continue
            if not doc.strip():
                missing_doc.append(f"{cls.__name__}.{name}")
            elif callable(target) and not isinstance(member, property) and ("Examples:" not in doc or ">>>" not in doc):
                missing_example.append(f"{cls.__name__}.{name}")

    assert not missing_doc, f"missing docstrings: {missing_doc}"
    assert not missing_example, f"missing `Examples:` / `>>>` in docstrings: {missing_example}"

    pytest.importorskip("oxide_parse._core")
    for mod_name in modules[:-1]:
        mod = importlib.import_module(mod_name)
        res = doctest.testmod(mod, optionflags=doctest.ELLIPSIS | doctest.NORMALIZE_WHITESPACE)
        assert res.attempted > 0, f"{mod_name}: no doctests executed"
        assert res.failed == 0, f"{mod_name}: {res.failed} doctest(s) failed"
