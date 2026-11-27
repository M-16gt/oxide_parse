"""Tests for the extensible `oxide_parse.formats` factory (including TOML) and Crawl4AI prompt overrides."""
from __future__ import annotations

import base64
import io
import json
import subprocess
import sys
from pathlib import Path
from typing import Any, Dict, List

import pytest

if sys.version_info >= (3, 11):
    import tomllib
else:
    import tomli as tomllib

from oxide_parse import (
    FORMATS,
    Plan,
    T,
    available_formats,
    compile,
    dumps_format,
    get_format,
    if_,
    loads_format,
    migrate,
    prompts,
    register_format,
    to_ir,
    unregister_format,
    validate_ir,
)
from oxide_parse import formats as formats_mod
from tests.test_cli_and_release import HTML, run_cli, tmp as write_tmp_file
from tests.test_dsl import SCHEMA


def test_builtin_format_roundtrips_json_yaml_toml_python(tmp_path: Path) -> None:
    pytest.importorskip("oxide_parse._core")
    # Pure-HTML subset + nullables for end-to-end extraction parity; also check full `SCHEMA` IR (with JSON nav)
    html_schema = {k: v for k, v in SCHEMA.items() if k != "j"}
    rich_schema = {
        **html_schema,
        "none_guard": if_(T.css(".flag").exists(), T.css(".flag").text(), None),
        "link_all": T.meta("hdr")["link"].link_header(None),
        "tail_only": T.css("li").slice(1, None).map(T.text()),
    }
    plan = compile(rich_schema, backend="lite")
    full_ir = to_ir(SCHEMA)

    sample_html = (
        '<html><head><base href="https://shop.test/"></head><body>'
        "<h1> Shop </h1>"
        "<div class='item' data-on='1'><span class='n'>A</span><h2>Cat</h2></div>"
        "<div class='p'><b>Item</b><span class='price'>$12.50</span></div>"
        "<span class='tag'>SALE</span>"
        "<a href='/p/12'>go</a>"
        "<span class='a'>x</span><span class='b'>y</span>"
        "<script id='d'>{\"props\":{\"price\":99}}</script>"
        "<h3 id='p1'>T</h3><ul><li>1</li><li>2</li></ul>"
        "</body></html>"
    )
    fetch_cb = lambda url, **_: "<p>Followed</p>"  # noqa: E731
    expected = plan.extract(sample_html, meta={"hdr": {"link": '<https://ex.test/2>; rel="next"'}}, fetch=fetch_cb)

    for fmt in ("json", "yaml", "toml", "python"):
        serialized = plan.dumps(fmt)
        reloaded = Plan.loads(serialized, fmt, backend="lite")
        assert reloaded.ir == plan.ir
        assert reloaded.extract(sample_html, meta={"hdr": {"link": '<https://ex.test/2>; rel="next"'}}, fetch=fetch_cb) == expected
        assert loads_format(dumps_format(full_ir, fmt), fmt) == full_ir

    # File paths and binary/stream load/dump
    toml_file = tmp_path / "schema.toml"
    plan.dump(toml_file)
    assert tomllib.loads(toml_file.read_text(encoding="utf-8"))
    assert Plan.load(toml_file, backend="lite").ir == plan.ir
    assert Plan.from_file(toml_file, backend="lite").ir == plan.ir
    assert Plan.from_toml(plan.to_toml(), backend="lite").ir == plan.ir

    py_file = tmp_path / "schema.py"
    plan.dump(py_file)
    assert Plan.load(py_file, backend="lite").ir == plan.ir
    assert Plan.from_python(plan.to_python(), backend="lite").ir == plan.ir

    stream = io.StringIO()
    plan.dump(stream, format="toml")
    stream.seek(0)
    assert Plan.load(stream, format="toml", backend="lite").ir == plan.ir


def test_toml_fallback_writer_and_non_dict_root(monkeypatch: pytest.MonkeyPatch) -> None:
    pytest.importorskip("oxide_parse._core")
    monkeypatch.setattr(formats_mod, "_load_tomli_w", lambda: None)

    plan = compile({**SCHEMA, "nullable": if_(T.css("b").exists(), "yes", None)}, backend="lite")
    toml_text = plan.dumps("toml")
    parsed = tomllib.loads(toml_text)
    assert isinstance(parsed, dict)
    assert Plan.loads(toml_text, "toml", backend="lite").ir == plan.ir

    # Arbitrarily shaped non-dict outputs (list / None / scalar) round-trip losslessly via TOML factory
    for data in (
        [1, None, {"k": "v", "nested": [True, None]}],
        None,
        "scalar",
        {"__oxide_root__": 1},
        {"__oxide_literal__": ["__oxide_root__", 1]},
        {"nested": {"__oxide_null__": True}},
        {"line\n": "delete\x7f"},
    ):
        raw = dumps_format(data, "toml")
        assert tomllib.loads(raw)
        assert loads_format(raw, "toml") == data


def test_custom_format_registration_and_error_guards(tmp_path: Path) -> None:
    pytest.importorskip("oxide_parse._core")
    assert all(name in available_formats(readable=True, writable=True) for name in ("json", "yaml", "toml", "python"))
    assert get_format("yml").name == "yaml"
    assert get_format(".py").name == "python"

    register_format(
        "b64json",
        dumps=lambda d: base64.b64encode(json.dumps(d).encode("utf-8")).decode("ascii"),
        loads=lambda s: json.loads(base64.b64decode(s.encode("ascii")).decode("utf-8")),
        extensions=(".b64",),
        aliases=("base64_json",),
    )
    try:
        plan = compile({"title": T.css("h1").text().strip()}, backend="auto")
        encoded = plan.dumps("base64_json")
        assert Plan.loads(encoded, "b64json", backend="auto").extract("<h1> Hi </h1>") == {"title": "Hi"}

        path = tmp_path / "custom.b64"
        plan.dump(path)
        assert Plan.load(path, backend="auto").extract("<h1> Yo </h1>") == {"title": "Yo"}

        with pytest.raises(ValueError, match="already registered"):
            register_format("b64json", dumps=str, overwrite=False)
        with pytest.raises(ValueError, match="shadows"):
            register_format("conflicting", dumps=str, aliases=("json",))
        with pytest.raises(ValueError, match="already registered"):
            register_format("conflicting", dumps=str, extensions=(".b64",), overwrite=False)
        with pytest.raises(ValueError, match="extension"):
            register_format("b64json", dumps=str, extensions=(" ",))
        assert get_format("b64json").extensions == (".b64",)
    finally:
        unregister_format("b64json")

    assert "b64json" not in FORMATS.names()
    with pytest.raises(ValueError, match="unknown format"):
        Plan.loads("{}", "nope_fmt")
    with pytest.raises(ValueError, match="no format registered"):
        Plan.load(tmp_path / "weird.unknown")


def test_cli_supports_toml_and_ir_prompt(tmp_path: Path) -> None:
    pytest.importorskip("oxide_parse._core")
    toml_schema = write_tmp_file("s.toml", dumps_format({"object": {"title": {"pipe": [{"css": "h1"}, "text", "strip"]}}}, "toml"))
    html_file = write_tmp_file("p.html", HTML)

    code, out, err = run_cli("validate", toml_schema)
    assert code == 0 and "ok" in err

    code, out, _ = run_cli("decompile", toml_schema)
    assert code == 0 and "T.css('h1').text().strip()" in out

    code, out, _ = run_cli("run", toml_schema, html_file, "--format", "toml")
    assert code == 0 and tomllib.loads(out) == {"title": "Hi"}

    c4 = write_tmp_file("c4.json", json.dumps({"baseSelector": ".c", "fields": [{"name": "t", "selector": "h2", "type": "text"}]}))
    code, out, _ = run_cli("migrate", "--from", "crawl4ai", c4, "--format", "toml")
    assert code == 0 and loads_format(out, "toml")["pipe"][0] == {"css": ".c"}

    code, out, _ = run_cli("migrate", "--from", "selector-map", "-", stdin='title = "h1::text"\n')
    assert code == 0 and json.loads(out)["object"]["title"]["pipe"][0] == {"css": "h1"}

    code, out, _ = run_cli("prompt", "generate", "--format", "ir")
    assert code == 0 and '"pipe"' in out and "oxide_parse" in out

    code, out, _ = run_cli("prompt", "generate", "--format", "crawl4ai")
    assert code == 0 and "baseSelector" in out and "computed" in out


def test_crawl4ai_prompt_override_direct_ir_and_validation_loop() -> None:
    pytest.importorskip("oxide_parse._core")

    class FakePromptMod:
        JSON_SCHEMA_BUILDER = "OLD_CSS"
        JSON_SCHEMA_BUILDER_XPATH = "OLD_XPATH"

    class FakeStrategy:
        @staticmethod
        def _build_schema_prompt(html: str, schema_type: str = "CSS", query: str | None = None, target_json_example: str | None = None) -> str:
            return f"OLD_PROMPT:{schema_type}:{query}"

        @staticmethod
        def _validate_schema(schema: dict, html_content: str, schema_type: str = "CSS", expected_fields: list[str] | None = None) -> dict:
            return {"success": False, "old": True}

        @staticmethod
        def _build_feedback_message(vr: dict, schema: dict, attempt: int, is_repeated: bool) -> str:
            return "OLD_FEEDBACK"

    class FakeModule:
        JsonElementExtractionStrategy = FakeStrategy
        prompts = FakePromptMod

    html = '<div class="card"><h2>Widget</h2><span class="price">$19.50</span></div>'
    with migrate.patch_crawl4ai_prompt(FakeModule, output="ir", distill_html=True):
        built = FakeStrategy._build_schema_prompt(html, "CSS", query="Extract card title and price")
        assert '"pipe"' in built and "Extract card title and price" in built
        assert '"object"' in FakePromptMod.JSON_SCHEMA_BUILDER
        assert '"xpath"' in FakePromptMod.JSON_SCHEMA_BUILDER_XPATH

        # Invalid selector returns diagnostic failure + distilled feedback
        bad_ir = {"pipe": [{"css": ".missing"}, {"map": {"object": {"title": {"pipe": [{"css": "h2"}, "text"]}}}}]}
        vr_bad = FakeStrategy._validate_schema(bad_ir, html, "CSS", expected_fields=["title", "price"])
        assert vr_bad["success"] is False
        assert vr_bad["base_elements_found"] == 0
        fb = FakeStrategy._build_feedback_message(vr_bad, bad_ir, 1, False)
        assert "matched 0 items" in fb and "oxide_parse IR" in fb

        # Valid IR passes validation and works idempotently through migrate.from_crawl4ai()
        good_ir = {
            "pipe": [
                {"css": ".card"},
                {"map": {"object": {"title": {"pipe": [{"css": "h2"}, "text"]}, "price": {"pipe": [{"css": ".price"}, "text", "float"]}}}},
            ]
        }
        vr_good = FakeStrategy._validate_schema(good_ir, html, "CSS", expected_fields=["title", "price"])
        assert vr_good["success"] is True
        assert vr_good["populated_fields"] == 2
        assert migrate.from_crawl4ai(good_ir) == good_ir
        assert Plan.from_ir(good_ir, "lite").extract(html) == [{"title": "Widget", "price": 19.5}]
        assert validate_ir(good_ir) == []

    # Everything restored cleanly on exit
    assert FakePromptMod.JSON_SCHEMA_BUILDER == "OLD_CSS"
    assert FakePromptMod.JSON_SCHEMA_BUILDER_XPATH == "OLD_XPATH"
    assert FakeStrategy._build_schema_prompt(html, "CSS", query="q") == "OLD_PROMPT:CSS:q"
    assert FakeStrategy._validate_schema({}, html)["old"] is True
    assert FakeStrategy._build_feedback_message({}, {}, 1, False) == "OLD_FEEDBACK"


def test_patch_real_crawl4ai_compat_module_if_present() -> None:
    compat_py = Path(__file__).resolve().parents[1] / ".migration" / "crawl4ai-compat" / "Scripts" / "python.exe"
    if not compat_py.is_file():
        pytest.skip(".migration/crawl4ai-compat environment not installed")

    oxide_python = str(Path(__file__).resolve().parents[1] / "python")
    code = f"""
import sys
sys.path.insert(0, {oxide_python!r})
import crawl4ai.extraction_strategy as es
import crawl4ai.prompts as pr
from oxide_parse.migrate import patch_crawl4ai_prompt

orig_builder = pr.JSON_SCHEMA_BUILDER
orig_prompt = es.JsonElementExtractionStrategy._build_schema_prompt("<b>hi</b>", "CSS")
with patch_crawl4ai_prompt(output="ir", distill_html=True):
    assert '"pipe"' in pr.JSON_SCHEMA_BUILDER
    patched = es.JsonElementExtractionStrategy._build_schema_prompt("<article><p>Hello world</p></article>", "CSS", query="Extract paragraph")
    assert '"pipe"' in patched and "Extract paragraph" in patched
    vr = es.JsonElementExtractionStrategy._validate_schema({{"pipe": [{{"css": "p"}}, "text"]}}, "<p>Hello</p>", "CSS")
    assert isinstance(vr, dict) and vr["success"] is True

assert pr.JSON_SCHEMA_BUILDER == orig_builder
assert es.JsonElementExtractionStrategy._build_schema_prompt("<b>hi</b>", "CSS") == orig_prompt
print("CRAWL4AI_PATCH_OK")
"""
    proc = subprocess.run([str(compat_py), "-c", code], capture_output=True, text=True, timeout=60, encoding="utf-8")
    assert proc.returncode == 0, proc.stderr or proc.stdout
    assert "CRAWL4AI_PATCH_OK" in proc.stdout
