"""Compare both official extraction strategies when Crawl4AI is installed."""

import json

import pytest

from oxide_parse import Plan, migrate
from oxide_parse.__main__ import main


@pytest.mark.parametrize("kind", ["css", "xpath"])
def test_matches_crawl4ai_extraction(kind, tmp_path, capsys, monkeypatch):
    monkeypatch.setenv("CRAWL4_AI_BASE_DIRECTORY", str(tmp_path))
    monkeypatch.setenv("LITELLM_LOCAL_MODEL_COST_MAP", "True")
    crawl4ai = pytest.importorskip("crawl4ai")
    pytest.importorskip("oxide_parse._core")
    page = (
        '<article class="card" data-sku="A1"><h2> Cool <em>Phone</em> </h2>'
        '<a href="/p/a1">more</a><div class="detail"><b>OLED</b></div>'
        '<ul><li><span>red</span></li><li><span>blue</span></li></ul>'
        '<div class="price">USD 1299</div></article>'
        '<article class="card" data-sku="B2"><h2>Tablet</h2><a href="/p/b2">more</a></article>'
    )
    def select(tag, class_name=None):
        if kind == "css":
            return f"{tag}.{class_name}" if class_name else tag
        return f".//{tag}[@class='{class_name}']" if class_name else f".//{tag}"

    tags = [{"name": "label", "type": "text", "selector": select("span")}]
    schema = {
        "name": "Products",
        "baseSelector": "article.card" if kind == "css" else "//article[@class='card']",
        "baseFields": [{"name": "sku", "type": "attribute", "attribute": "data-sku"}],
        "fields": [
            {"name": "title", "type": "text", "selector": select("h2"), "transform": "uppercase"},
            {"name": "link", "type": "attribute", "selector": select("a"), "attribute": "href"},
            {"name": "price", "type": "regex", "selector": select("div", "price"), "pattern": r"USD (\d+)", "default": "N/A"},
            {"name": "currency", "type": "regex", "selector": select("div", "price"), "pattern": "USD", "group": 0, "default": "N/A"},
            {
                "name": "detail",
                "type": "nested",
                "selector": select("div", "detail"),
                "fields": [{"name": "kind", "type": "text", "selector": select("b")}],
            },
            {"name": "tags", "type": "list", "selector": select("li"), "fields": tags},
            {"name": "nested_tags", "type": "nested_list", "selector": select("li"), "fields": tags},
            {"name": "markup", "type": "html", "selector": select("h2")},
        ],
    }
    strategy = crawl4ai.JsonCssExtractionStrategy
    if kind == "xpath":
        strategy = crawl4ai.JsonXPathExtractionStrategy

    extractor = strategy(schema)
    expected = extractor.extract("https://example.test/products", page)
    assert len(expected) == 2
    assert expected[0]["currency"] == "USD"
    assert expected[1]["detail"] == {}
    assert expected[0]["title"] == ("COOLPHONE" if kind == "css" else "COOL PHONE")
    exported = crawl4ai.CrawlerRunConfig(extraction_strategy=extractor).dump()
    ir = migrate.from_crawl4ai(schema)
    assert migrate.from_crawl4ai(exported) == ir
    assert Plan.from_ir(ir, "lite").extract(page) == expected

    config_path = tmp_path / "crawl4ai.json"
    html_path = tmp_path / "page.html"
    schema_path = tmp_path / "schema.py"
    config_path.write_text(json.dumps(exported), encoding="utf-8")
    html_path.write_text(page, encoding="utf-8")
    assert main(["migrate", "--from", "crawl4ai", str(config_path), "--format", "python", "--out", str(schema_path)]) == 0
    assert main(["run", str(schema_path), str(html_path), "--backend", "lite"]) == 0
    assert json.loads(capsys.readouterr().out) == expected
