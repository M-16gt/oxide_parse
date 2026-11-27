"""CLI (`oxide-parse ...`), prompts, and the release hygiene checks (versions, stubs). Written with the standard library
only, so the structure tests run without the native module."""
import contextlib
import io
import json
import pathlib
import re
import tempfile

import pytest

import oxide_parse
from oxide_parse.__main__ import main

ROOT = pathlib.Path(__file__).resolve().parents[1]

HTML = '<html><body><h1> Hi </h1><div class="card"><h2>A</h2><a href="/a">x</a></div><div class="card"><h2>B</h2></div></body></html>'
IR = {"object": {"title": {"pipe": [{"css": "h1"}, "text", "strip"]}}}


def run_cli(*argv, stdin=None):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        old = None
        if stdin is not None:
            import sys

            old, sys.stdin = sys.stdin, io.StringIO(stdin)
        try:
            code = main(list(argv))
        except SystemExit as e:  # argparse errors
            code = int(e.code or 0)
        finally:
            if old is not None:
                import sys

                sys.stdin = old
    return code, out.getvalue(), err.getvalue()


def tmp(name, text):
    d = pathlib.Path(tempfile.mkdtemp())
    p = d / name
    p.write_text(text, encoding="utf-8")
    return str(p)


def test_versions_are_in_sync_everywhere():
    py = re.search(r'^version = "([^"]+)"', (ROOT / "pyproject.toml").read_text(encoding="utf-8"), re.M).group(1)
    assert oxide_parse.__version__ == py
    for crate in ("core", "py"):
        text = (ROOT / "crates" / crate / "Cargo.toml").read_text(encoding="utf-8")
        assert re.search(r'^version = "([^"]+)"', text, re.M).group(1) == py, crate
    lock = (ROOT / "Cargo.lock").read_text(encoding="utf-8")
    for name in ("oxide_parse-core", "oxide_parse-py"):
        assert f'name = "{name}"\nversion = "{py}"' in lock, name
    assert f"v{py}" in (ROOT / "docs" / "architecture" / "publishing.md").read_text(encoding="utf-8")


def test_stub_matches_the_native_module_when_it_is_built():
    stub = (ROOT / "python" / "oxide_parse" / "_core.pyi").read_text(encoding="utf-8")
    src = (ROOT / "crates" / "py" / "src" / "lib.rs").read_text(encoding="utf-8")
    # every Rust `fn` exposed to Python is in the stub (source-level check, no build needed)
    exposed = set(re.findall(r"#\[pyfunction\][^\n]*\n(?:#\[[^\n]*\n)*fn (\w+)", src))
    for name in exposed:
        assert re.search(rf"def {name}\(", stub), f"{name} missing in _core.pyi"
    for method in ("explain", "extract", "start", "extract_batch", "export_adaptive_store", "forget_adaptive_keys", "resume", "result", "requests_json"):
        assert re.search(rf"def {method}\(", stub), method
    core = pytest.importorskip("oxide_parse._core")
    assert core.__version__ == oxide_parse.__version__


def test_prompts_list_every_operation_and_every_source():
    from oxide_parse import ir_spec, prompts

    text = prompts.generate_prompt()
    assert "schema = ..." in text and "group_by" in text and "T.json_ld" in text
    for op in ir_spec.all_op_names():
        assert op in text, op
    for src in prompts.sources():
        assert src in {"bs4", "crawl4ai", "lxml", "scrapy", "selectolax"}
        assert "oxide_parse" in prompts.migrate_prompt(src)
    assert "BeautifulSoup" in prompts.migrate_prompt("bs4")
    with pytest.raises(ValueError):
        prompts.migrate_prompt("nope")


def test_cli_decompile_validate_migrate_prompt_and_errors():
    ir_file = tmp("s.json", json.dumps(IR))
    code, out, _ = run_cli("decompile", ir_file)
    assert code == 0 and "T.css('h1').text().strip()" in out
    code, out, err = run_cli("decompile", ir_file, "--name", "my_schema")
    assert "my_schema = " in out

    assert run_cli("validate", ir_file)[0] == 0
    bad = tmp("bad.json", json.dumps({"object": {"a": {"pipe": ["txt"]}}}))
    code, _, err = run_cli("validate", bad)
    assert code == 1 and "$.a[0]: unknown operation 'txt'" in err

    c4 = tmp("c4.json", json.dumps({"baseSelector": ".c", "fields": [{"name": "t", "selector": "h2", "type": "text"}]}))
    code, out, _ = run_cli("migrate", "--from", "crawl4ai", c4, "--format", "python")
    assert code == 0 and "T.css('.c').map({" in out
    code, out, _ = run_cli("migrate", "--from", "crawl4ai", c4, "--wrap", "items")
    assert code == 0 and "items" in json.loads(out)["object"]
    sm = tmp("m.json", json.dumps({"t": "h1::text"}))
    assert "T.css('h1').text().strip()" in run_cli("migrate", "--from", "selector-map", sm, "--format", "python")[1]
    # a Crawl4AI schema given to `run` / `decompile` gets a pointer to `migrate`, not a stack trace
    code, _, err = run_cli("decompile", c4)
    assert code == 1 and "migrate --from crawl4ai" in err
    broken = tmp("x.json", json.dumps({"fields": [{"name": "k", "type": "computed"}], "baseSelector": ".c"}))
    code, _, err = run_cli("migrate", "--from", "crawl4ai", broken)
    assert code == 1 and "computed" in err and "Traceback" not in err

    code, out, _ = run_cli("prompt", "generate")
    assert code == 0 and "oxide_parse" in out
    assert run_cli("prompt", "migrate", "--from", "scrapy")[0] == 0
    assert run_cli("prompt", "migrate", "--from", "nope")[0] == 1
    assert run_cli("ir-schema")[1].lstrip().startswith("{")
    assert run_cli("nope")[0] == 2  # argparse: unknown command
    assert run_cli("decompile", "/no/such/file.json")[0] == 1


def test_cli_python_schema_file():
    py = tmp("schema.py", "from oxide_parse import T\nschema = {'t': T.css('h1').text().strip()}\n")
    code, out, _ = run_cli("decompile", py)
    assert code == 0 and "'t': T.css('h1').text().strip()" in out
    code, _, err = run_cli("decompile", tmp("nosch.py", "x = 1\n"))
    assert code == 1 and "must define a variable `schema`" in err


def test_cli_commands_that_need_the_native_module():
    pytest.importorskip("oxide_parse._core")
    html_file = tmp("page.html", HTML)
    ir_file = tmp("s.json", json.dumps(IR))
    code, out, err = run_cli("run", ir_file, html_file, "--backend", "lite", "--explain")
    assert code == 0 and json.loads(out) == {"title": "Hi"}
    assert "engine: lite" in err
    code, out, _ = run_cli("markdown", html_file, "--selector", "body")
    assert code == 0 and out.startswith("# Hi")
    code, out, _ = run_cli("distill", html_file, "--max-chars", "500")
    assert code == 0 and "card" in out
    code, out, _ = run_cli("distill", "-", stdin=HTML)
    assert code == 0 and "h1" in out
    output = pathlib.Path(html_file).with_name("page.md")
    assert run_cli("markdown", html_file, "--out", str(output))[0] == 0
    assert output.read_text(encoding="utf-8").startswith("# Hi")
