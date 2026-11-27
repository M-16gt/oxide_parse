"""Converters (`oxide_parse.migrate`): the structure needs no native module, the behaviour checks use it when present."""
import json

import pytest

from oxide_parse import Plan, T, decompile, validate_ir
from oxide_parse import migrate
from oxide_parse.migrate import MigrationError

CARDS = """
<div class="card" data-sku="A1"><h2> Phone </h2><a href="/p/a1">more</a><span class="price">$1,299 total</span>
  <div class="d"><b>OLED</b></div><i class="tag">x</i><i class="tag">y</i></div>
<div class="card" data-sku="B2"><h2>Tablet</h2><a href="/p/b2">more</a><div class="d"><b>LCD</b></div></div>
"""

SCHEMA = {
    "name": "Shop",
    "baseSelector": ".card",
    "baseFields": [{"name": "sku", "type": "attribute", "attribute": "data-sku"}],
    "fields": [
        {"name": "title", "selector": "h2", "type": "text", "transform": "uppercase"},
        {"name": "link", "selector": "a", "type": "attribute", "attribute": "href"},
        {"name": "price", "selector": ".price", "type": "regex", "pattern": r"(\d[\d,]*)", "default": "N/A"},
        {"name": "detail", "selector": ".d", "type": "nested", "fields": [{"name": "kind", "selector": "b", "type": "text"}]},
        {"name": "tags", "selector": ".tag", "type": "list", "fields": [{"name": "t", "type": "text"}]},
    ],
}


def test_crawl4ai_ir_is_valid_and_decompiles_to_readable_python():
    ir = migrate.from_crawl4ai(SCHEMA)
    assert validate_ir(ir) == []
    code = migrate.from_crawl4ai_code(SCHEMA)
    assert code.startswith("from oxide_parse import T\n")
    assert "T.css('.card').map({" in code and ".text(strip=True).upper()" in code and ".default('N/A')" in code
    ns: dict = {}
    exec(code, ns)  # the generated code runs and rebuilds the same IR
    from oxide_parse import to_ir

    assert to_ir(ns["schema"]) == ir
    wrapped = migrate.from_crawl4ai(SCHEMA, wrap="items")
    assert set(wrapped["object"]) == {"items"}


def test_crawl4ai_accepts_the_wrappers_other_tools_write():
    assert migrate.from_crawl4ai({"schema": SCHEMA}) == migrate.from_crawl4ai(SCHEMA)
    assert migrate.from_crawl4ai({"type": "JsonCssExtractionStrategy", "params": {"schema": SCHEMA}}) == migrate.from_crawl4ai(SCHEMA)
    wrapped = {"type": "dict", "value": {"baseSelector": "p", "fields": [{"type": "dict", "value": {"name": "title", "type": "text"}}]}}
    strategy = {"type": "JsonCssExtractionStrategy", "params": {"schema": wrapped}}
    config = {"type": "CrawlerRunConfig", "params": {"extraction_strategy": strategy}}
    assert migrate.from_crawl4ai(config) == migrate.from_crawl4ai({"baseSelector": "p", "fields": [{"name": "title", "type": "text"}]})


def test_crawl4ai_reports_what_it_cannot_convert():
    base = {"baseSelector": ".c"}
    for field, needle in (
        ({"name": "x", "type": "computed", "expression": "a+b"}, "computed"),
        ({"name": "x", "selector": "a", "type": "magic"}, "unsupported field type"),
        ({"name": "x", "selector": "a", "type": "text", "transform": "title"}, "unsupported transform"),
        ({"name": "x", "selector": "a", "type": "attribute"}, "without `attribute`"),
        ({"name": "x", "selector": "a", "type": "text", "source": "+ .y"}, "source"),
        ({"selector": "a", "type": "text"}, "needs a `name`"),
    ):
        with pytest.raises(MigrationError, match=needle):
            migrate.from_crawl4ai({**base, "fields": [field]})
    with pytest.raises(MigrationError):
        migrate.from_crawl4ai({"nothing": 1})


def test_crawl4ai_xpath_strategy_handles_string_results():
    x = {
        "type": "JsonXPathExtractionStrategy",
        "baseSelector": "//div[@class='c']",
        "fields": [
            {"name": "t", "selector": "./h2", "type": "text"},
            {"name": "h", "selector": "./a/@href", "type": "attribute", "attribute": "href"},
            {"name": "n", "selector": "normalize-space(./h2)", "type": "text"},
        ],
    }
    ir = migrate.from_crawl4ai(x)
    assert validate_ir(ir) == []
    text = json.dumps(ir)
    assert '"xpath": "//div[@class=\'c\']"' in text and '"at": 0' in text
    assert migrate.from_crawl4ai(x, kind="css")["pipe"][0] == {"css": "//div[@class='c']"}


def test_selector_map_covers_text_attr_lists_xpath_and_records():
    ir = migrate.from_selector_map(
        {
            "title": "h1::text",
            "href": "a::attr(href)",
            "plain": "p.lead",
            "tags": ["li.tag::text"],
            "links": ["a.l::attr(href)"],
            "x": "//h2/text()",
            "cards": {"_base": ".card", "n": "h2::text", "u": "a::attr(href)", "self": "::attr(data-id)"},
        }
    )
    assert validate_ir(ir) == []
    code = decompile(ir)
    for needle in (
        "'title': T.css('h1').text().strip()",
        "'href': T.css('a').attr('href')",
        "'tags': T.css('li.tag').map(T.text().strip())",
        "'links': T.css('a.l').map(T.attr('href'))",
        "'x': T.xpath('//h2/text()').at(0).strip()",
        "'self': T.attr('data-id')",
    ):
        assert needle in code, (needle, code)
    assert "migrate" not in code
    raw = migrate.from_selector_map({"a": "p::text"}, strip=False)
    assert "strip" not in json.dumps(raw)
    for bad in ({"a": ["x", "y"]}, {"a": 3}, {"a": ["::text"] and [{"k": "p"}]}):
        with pytest.raises(MigrationError):
            migrate.from_selector_map(bad)


def test_migrated_schemas_give_the_expected_data():
    pytest.importorskip("oxide_parse._core")
    plan = Plan.from_ir(migrate.from_crawl4ai(SCHEMA), "lite")
    out = plan.extract(CARDS)
    assert out[0] == {
        "sku": "A1",
        "title": "PHONE",
        "link": "/p/a1",
        "price": "1,299",
        "detail": {"kind": "OLED"},
        "tags": [{"t": "x"}, {"t": "y"}],
    }
    assert out[1]["price"] == "N/A" and out[1]["tags"] == [] and out[1]["detail"] == {"kind": "LCD"}
    flat = Plan.from_ir(
        migrate.from_selector_map({"cards": {"_base": ".card", "n": "h2::text", "sku": "::attr(data-sku)"}, "first": "h2::text"}),
        "lite",
    )
    assert flat.extract(CARDS) == {"cards": [{"n": "Phone", "sku": "A1"}, {"n": "Tablet", "sku": "B2"}], "first": "Phone"}

    root = migrate.from_crawl4ai({"fields": [{"name": "title", "selector": "h1"}]})
    assert Plan.from_ir(root, "lite").extract("<h1>Title</h1>") == {"title": "Title"}
    xpath = migrate.from_selector_map({"links": ["xpath://a/@href"]})
    assert Plan.from_ir(xpath, "lite").extract('<a href="/x">X</a>') == {"links": ["/x"]}
