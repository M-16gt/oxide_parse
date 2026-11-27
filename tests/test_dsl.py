import json

from oxide_parse import T, decompile, follow, if_, to_ir
from oxide_parse.decompile import expr_to_code

SCHEMA = {
    "title": T.css("h1").text().strip(),
    "price": T.css(".price").text().replace("$", "").float().default(0),
    "tags": T.css(".tag").map(T.text().lower()),
    "items": T.css(".item").filter(T.attr("data-on") == "1").map(
        {"name": T.css(".n").first().text(), "i": T.index(), "cat": T.parent().css("h2").text()}
    ),
    "kind": if_(T.css(".sale").exists(), "sale", "regular"),
    "id": T.css("a").attr("href").regex(r"(\d+)", 1).int(),
    "extra": T.coalesce(T.css(".a").text(), T.css(".b").text()),
    "sub": T.css("a").map(
        follow(request={"url": T.attr("href"), "headers": {"Accept": "text/html"}}, schema={"t": T.meta("title"), "d": T.css("p").text()}, meta={"title": T.text()})
    ),
    "j": T["users"][0]["name"],
    "flags": [T.css("button").has_attr("disabled"), T.css(".p").text().non_empty().default("n/a")],
    "t": T.css("div").text(sep=" ", strip=True, without=".ad, script"),
    "json": T.css("script#d").text().parse_json()["props"]["price"],
    "heal": T.css("#p1 h3", adaptive="title", threshold=0.6).text(),
}


def test_ir_is_json_and_ordered():
    ir = to_ir(SCHEMA)
    assert json.loads(json.dumps(ir)) == ir
    assert list(ir["object"]) == list(SCHEMA)


def test_decompile_roundtrip():
    ir = to_ir(SCHEMA)
    ns = {}
    exec(decompile(ir), ns)
    assert to_ir(ns["schema"]) == ir


def test_immutable_chain():
    base = T.css("a")
    a = base.attr("href")
    b = base.text()
    assert base._steps() == [{"css": "a"}]
    assert a._steps()[-1] == {"attr": "href"} and b._steps()[-1] == "text"


def test_expr_has_no_truth_value():
    try:
        bool(T.css("x").text() == 1)
    except TypeError:
        return
    raise AssertionError("expected TypeError")


def test_repr_is_code():
    assert expr_to_code({"pipe": T.css("h1").text()._steps()}) == "T.css('h1').text()"


def test_follow_contract():
    spec = T.follow({"url": T.attr("href"), "method": "GET"}, {"x": T.text()})._steps()[0]["follow"]
    assert set(spec) == {"request", "schema"} and spec["request"]["object"]["method"] == {"lit": "GET"}
    short = T.follow(T.attr("href"), {"x": T.text()})._steps()[0]["follow"]
    assert short["request"] == {"pipe": [{"attr": "href"}]}
    for bad in (lambda: T.follow({"url": 1}, None), lambda: T.follow(url=T.attr("href"), schema={"x": T.text()})):  # type: ignore[call-arg]
        try:
            bad()
        except TypeError:
            continue
        raise AssertionError("`url=` is gone, `schema` is required")


def test_decompile_rejects_injection():
    for bad_ir in ({"pipe": ["text(); __import__('os')"]}, {"pipe": [{"__import__('os')": "x"}]}):
        try:
            decompile(bad_ir)
        except ValueError:
            continue
        raise AssertionError(f"accepted {bad_ir}")
    try:
        decompile({"lit": 1}, name="x = 1; import os #")
    except ValueError:
        return
    raise AssertionError("accepted a bad variable name")


def test_new_ops_in_ir():
    steps = T.css("div").text(sep=" ", strip=True, without=".ad")._steps()
    assert steps[-1] == {"text_opts": {"sep": " ", "strip": True, "without": ".ad"}}
    assert T.css("a").text()._steps()[-1] == "text"
    assert T.css("b").has_attr("disabled")._steps()[-1] == {"has_attr": "disabled"}


def test_text_with_empty_separator_is_plain_text():
    # sep="" joins with nothing: identical to text(), and it must survive an IR <-> code roundtrip
    ir = to_ir({"a": T.css("div").text(sep="")})
    assert ir == to_ir({"a": T.css("div").text()})
    ns = {}
    exec(decompile(ir), ns)
    assert to_ir(ns["schema"]) == ir


def test_stage2_ops_in_ir_and_roundtrip():
    schema = {
        "a": T.css(".p").map(T.css("b").text().int(), on_error="skip"),
        "b": T.css("a").attr("href").url(),
        "c": T.css("a").attr("href").url(T.meta("base")),
        "d": T.css("script").text().js_json("window.S")["x"],
        "e": T.css("script").text().js_json(),
        "f": T.css(".s").sections("h3", items="p"),
        "g": T.kv("Weight"),
    }
    ir = to_ir(schema)
    assert ir["object"]["a"]["pipe"][-1] == {"map_opts": {"body": to_ir(T.css("b").text().int()), "on_error": "skip"}}
    assert ir["object"]["b"]["pipe"][-1] == {"resolve_url": {}}
    ns = {}
    exec(decompile(ir), ns)
    assert to_ir(ns["schema"]) == ir
    try:
        T.css("x").map(T.text(), on_error="explode")
    except ValueError:
        return
    raise AssertionError("bad on_error accepted")


def test_kv_label_quoting():
    from oxide_parse.expr import _xpath_literal

    assert _xpath_literal("a") == "'a'" and _xpath_literal("it's") == '"it\'s"'
    assert _xpath_literal("a'b\"c").startswith("concat(")


def test_kv_searches_inside_the_current_element():
    xp = T.kv("Weight")._steps()[-1]["xpath"]
    assert xp.startswith(".//*") and not xp.startswith("//")


def test_v8_5_slice_rest_distill_and_meta_ir_shapes():
    from oxide_parse import T, decompile, to_ir

    assert T.css("li").slice(1, 4)._steps()[-1] == {"slice": {"start": 1, "stop": 4}}
    assert T[1:]._steps()[-1] == {"slice": {"start": 1}}
    assert T[:5]._steps()[-1] == {"slice": {"stop": 5}}
    assert T[-3:]._steps()[-1] == {"slice": {"start": -3}}
    assert T.rest()._steps()[-1] == {"slice": {"start": 1}}
    try:
        T[::2]
    except TypeError:
        pass
    else:
        raise AssertionError("a step is not supported")
    assert decompile(to_ir(T.css("a").rest())).endswith(".rest()\n")
    assert "slice(-2, None)" in decompile(to_ir(T.css("a")[-2:]))
    assert T.css("main").distill(max_chars=8000)._steps()[-1] == {"distill": {"keep": 2, "max_text": 80, "max_chars": 8000}}
    assert "distill(2, 80, 8000)" in decompile(to_ir(T.css("main").distill(max_chars=8000)))


def test_v8_5_validate_ir_reports_every_problem_with_paths():
    from oxide_parse import T, ir_schema, validate_ir
    from oxide_parse import ir_spec

    good = {"object": {"t": {"pipe": [{"css": "h1"}, "text"]}, "u": {"lit": 1}}}
    assert ir_spec.validate_structure(good) == []
    bad = {
        "object": {
            "a": {"pipe": [{"css": 5}, "txt", {"at": -1}]},
            "b": {"pipe": [{"text_opts": {"sepp": " "}}, {"map_opts": {"on_error": "ignore"}}, {"follow": {"request": {"lit": 1}}}]},
            "c": {"nope": 1},
            "d": {"pipe": [{"replace": {"from": "a"}}, "css"]},
        }
    }
    errors = ir_spec.validate_structure(bad)
    joined = "\n".join(errors)
    for needle in (
        "$.a[0].css: expected a string",
        "$.a[1]: unknown operation 'txt'",
        "$.a[2].at: expected a non-negative integer",
        "$.b[0].text_opts.sepp: unknown field",
        "$.b[1].map_opts.body: missing",
        "$.b[1].map_opts.on_error: must be one of",
        "$.b[2].follow.schema: missing",
        "$.c: unknown expression kind 'nope'",
        "$.d[0].replace.to: missing",
        "$.d[1]: unknown operation 'css' ('css' takes an argument",
    ):
        assert needle in joined, (needle, joined)
    assert len(errors) >= 10  # all of them in one pass, not just the first
    # JSON strings and T-expressions are accepted, garbage does not raise
    assert validate_ir("{not json")[0].startswith("$: not valid JSON")
    assert validate_ir(T.css("h1").text()) == []
    assert validate_ir({"x": T.css("h1").text(), "y": T.css("a").attr("href")}) == []
    assert validate_ir(T) == []
    assert validate_ir({"nested": [{"title": T.css("h1").text()}]}) == []
    assert validate_ir({"pipe": [{"distill": None}]})
    cycle = {"object": {}}
    cycle["object"]["self"] = cycle
    assert validate_ir(cycle)[0].startswith("$:")
    assert validate_ir(42)[0].startswith("$:")
    # the exported JSON Schema describes the same nodes
    schema = ir_schema()
    assert schema["$schema"].endswith("2020-12/schema")
    ops = schema["$defs"]["op"]["oneOf"]
    consts = {o["const"] for o in ops if "const" in o}
    named = {next(iter(o["properties"])) for o in ops if "properties" in o}
    assert consts | named == set(ir_spec.all_op_names())


def test_v8_5_ir_schema_file_is_up_to_date():
    import pathlib
    import sys

    sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "python" / "oxide_parse"))
    import ir_spec

    path = pathlib.Path(__file__).resolve().parents[1] / "python" / "oxide_parse" / "ir.schema.json"
    assert path.read_text(encoding="utf-8") == ir_spec.schema_json(), "run: python scripts/gen_ir_schema.py"


def test_paginate_ir_decompile_and_validation():
    from oxide_parse import paginate, validate_ir

    expr = paginate(
        items=T.css(".friend").map(T.text()),
        next=T.css("a.next").attr("href").url(),
        max_pages=5,
        max_items=50,
        meta={"tag": T.meta("tag")},
    )
    ir = to_ir({"friends": expr})
    step = ir["object"]["friends"]["pipe"][0]["paginate"]
    assert step["max_pages"] == 5 and step["max_items"] == 50 and "meta" in step
    ns = {}
    exec(decompile(ir), ns)
    assert to_ir(ns["schema"]) == ir

    for bad_kw in ({"max_pages": 0}, {"max_items": 0}, {"max_pages": True}, {"max_items": False}):
        try:
            paginate(T.css("a").map(T.text()), T.css("a.next").attr("href"), **bad_kw)  # type: ignore[arg-type]
        except ValueError:
            continue
        raise AssertionError(f"accepted {bad_kw}")

    bad_ir = {"object": {"p": {"pipe": [{"paginate": {"items": to_ir(T.css("li").map(T.text())), "max_pages": 0}}]}}}
    errs = validate_ir(bad_ir)
    joined = "\n".join(errs)
    assert "$.p[0].paginate.next: missing" in joined
    assert "$.p[0].paginate.max_pages: expected a positive integer" in joined
