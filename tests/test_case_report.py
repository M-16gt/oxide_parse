"""Executable regression and capability suite covering scenarios from `oxide-parse-case-report.md`.

Includes:
- Wave 0 baseline fixtures across all documented core patterns (including the priority architectural series:
  cases 05, 07, 08, 09, 14, 23, 24, 29, 33).
- Wave 1 features and fixes:
  - Case #15 / #27 (`T.parse_html(...)` for HTML inside JSON and `iframe[srcdoc]`)
  - Case #20 (`decode_html` + automatic BOM / `<meta charset>` / `Content-Type` / `encoding=` handling)
  - Case #22 (cross-format `follow(..., format="json", schema=paginate(...))` inside a single HTML plan)
  - Case #23 (`JsonDom` truthiness / null / missingness fix for `if_`, `default`, `exists`, `non_empty`, `paginate`)
  - Case #24 (RFC 8288 `.link_header("next")` + response `(body, meta)` and duck-typed `Response` envelopes)
- Wave 2 structured, text, media, XML, and deduplication features:
  - Case #07 (`.table("records"|"grid"|"split")` with `colspan`/`rowspan`/`rowspan="0"` and nested table isolation)
  - Case #08 (`.own_text()`, `.text(..., deep=False)`, `.own_texts()` parity across `stream`, `lite`, `scraper`)
  - Case #16 (`.price()` locale & currency normalization)
  - Case #17 (`.parse_srcset()` WHATWG parser preserving CDN commas)
  - Case #18 (`backend="xml"` & `follow(..., format="xml")` for Sitemap, RSS w/ CDATA, Atom, CSS/XPath namespaces)
  - Case #32 (opt-in `own_text=True` in selector maps & `allow_sibling_source=True` in `from_crawl4ai`)
  - Case #33 (`.unique(by=..., keep=...)` & `paginate(..., unique_by=..., max_items=...)`)
- Wave 3 ecosystem & semantic extractors:
  - Case #11 (`json_ld(..., resolve_refs=True)` expanding `@id` references inside `@graph`)
  - Case #12 (`microdata()` for `itemscope`/`itemprop`/`itemref` and nested items across `lite`/`scraper`)
  - Case #14 (`next_flight()` for Next.js App Router `self.__next_f.push(...)` RSC Flight chunks & refs)
  - Case #19 (`Plan.extract_jsonl()` & `Plan.extract_csv()` for multi-line quoted CSV & bad NDJSON lines)
  - Case #25 (`crawl()` Zero-IO BFS link-graph crawler with `canonicalize_url`, depth/domain limits)
  - Case #26 / #27 / #34 (load-more endpoint parity, open Shadow DOM snapshot, normalized PDF/OCR rows)
  - Case #31 (`main_content()` & `markdown(..., main=True)` layout-independent article extraction)
"""
from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Any, Dict, List

import pytest

from oxide_parse import (
    Plan,
    T,
    canonicalize_url,
    compile,
    crawl,
    decode_html,
    follow,
    markdown,
    migrate,
    paginate,
    to_ir,
    validate_ir,
)


@dataclass
class FakeResponse:
    """Minimal duck-typed HTTP response object for testing response envelopes."""

    content: bytes
    headers: Dict[str, str]
    status_code: int = 200
    url: str = ""


# -- 1. HTML structure ------------------------------------------------------------------------------------------------


def test_case_01_quotes_authors_and_nested_tags() -> None:
    html = (
        '<div class="quote"><span class="text">Hello</span>'
        '<small class="author">Ada</small><a class="tag">science</a><a class="tag">life</a></div>'
        '<div class="quote"><span class="text">World</span><small class="author">Grace</small></div>'
    )
    schema = T.css(".quote").map(
        {
            "text": T.css(".text").text(),
            "author": T.css(".author").text(),
            "tags": T.css(".tag").map(T.text()),
        }
    )
    expected = [
        {"text": "Hello", "author": "Ada", "tags": ["science", "life"]},
        {"text": "World", "author": "Grace", "tags": []},
    ]
    for backend in ("lite", "scraper", "auto"):
        assert compile(schema, backend=backend).extract(html) == expected


def test_case_02_nested_catalog_and_crawl4ai_migration() -> None:
    html = (
        '<section class="category"><h2 class="cat-name">Audio</h2>'
        '<div class="product"><span class="name">Headphones</span><span class="brand">Acme</span>'
        '<div class="review">Great</div><ul><li class="feature">ANC</li><li class="feature">BT</li></ul></div>'
        '<div class="product"><span class="name">Earbuds</span><ul><li class="feature">Compact</li></ul></div>'
        "</section>"
    )
    c4_schema = {
        "name": "Catalog",
        "baseSelector": ".category",
        "fields": [
            {"name": "category", "selector": ".cat-name", "type": "text"},
            {
                "name": "products",
                "selector": ".product",
                "type": "nested_list",
                "fields": [
                    {"name": "name", "selector": ".name", "type": "text"},
                    {"name": "brand", "selector": ".brand", "type": "text"},
                    {"name": "reviews", "selector": ".review", "type": "list", "fields": [{"name": "body", "type": "text"}]},
                    {"name": "features", "selector": "li.feature", "type": "list", "fields": [{"name": "label", "type": "text"}]},
                ],
            },
        ],
    }
    ir = migrate.from_crawl4ai(c4_schema)
    assert validate_ir(ir) == []
    out = Plan.from_ir(ir, "lite").extract(html)
    assert out == [
        {
            "category": "Audio",
            "products": [
                {
                    "name": "Headphones",
                    "brand": "Acme",
                    "reviews": [{"body": "Great"}],
                    "features": [{"label": "ANC"}, {"label": "BT"}],
                },
                {
                    "name": "Earbuds",
                    "brand": None,
                    "reviews": [],
                    "features": [{"label": "Compact"}],
                },
            ],
        }
    ]


def test_case_03_first_offer_early_exit_vs_count() -> None:
    tail = '<div class="offer">Other</div>' * 1000
    html = f"<title>Shop</title><div class='offer'>First</div>{tail}"
    early_plan = compile({"title": T.css("title").text(), "first": T.css(".offer").text()}, backend="auto")
    count_plan = compile(
        {"title": T.css("title").text(), "first": T.css(".offer").text(), "n": T.css(".offer").count()},
        backend="auto",
    )
    assert early_plan.engine == "stream"
    assert count_plan.engine == "stream"
    assert early_plan.extract(html) == {"title": "Shop", "first": "First"}
    assert count_plan.extract(html) == {"title": "Shop", "first": "First", "n": 1001}


def test_case_04_filter_by_descendant_class_and_text() -> None:
    html = (
        '<div class="product"><span class="in-stock">В наличии</span><b>Item A</b></div>'
        '<div class="product archived"><span class="in-stock">В наличии</span><b>Item B</b></div>'
        '<div class="product"><span>Нет</span><b>Item C</b></div>'
    )
    for backend in ("lite", "scraper"):
        plan = compile(T.css(".product:has(> .in-stock):not(.archived)").map(T.css("b").text()), backend=backend)
        assert plan.extract(html) == ["Item A"]

    text_match = compile(T.css(".product:contains('В наличии'):not(.archived)").map(T.css("b").text()), backend="lite")
    assert text_match.extract(html) == ["Item A"]


def test_case_05_sections_between_headings() -> None:
    flat_html = "<article><h2>A</h2><p>one</p><p>two</p><h2>B</h2><p>three</p></article>"
    sec_out = compile(T.css("article").sections("h2", "p"), backend="lite").extract(flat_html)
    grp_out = compile(
        T.css("article").group_by("h2", "p").map({"h": T.first().text(), "p": T.rest().map(T.text())}),
        backend="lite",
    ).extract(flat_html)
    expected = [{"header": "A", "items": ["one", "two"]}, {"header": "B", "items": ["three"]}]
    assert sec_out == expected
    assert [{"h": x["header"], "p": x["items"]} for x in expected] == grp_out


def test_case_06_kv_characteristic_lookup() -> None:
    html_a = "<dl><dt>Цвет</dt><dd>Белый</dd><dt>Вес</dt><dd>2 кг</dd></dl>"
    html_b = "<dl><dt>Вес</dt><dd>2 кг</dd><dt>Цвет</dt><dd>Белый</dd></dl>"
    plan = compile({"weight": T.kv("Вес").text(), "missing": T.kv("Размер").text()}, backend="lite")
    assert plan.extract(html_a) == {"weight": "2 кг", "missing": None}
    assert plan.extract(html_b) == {"weight": "2 кг", "missing": None}


def test_case_07_table_rowspan_colspan_cell_extraction_baseline() -> None:
    html = (
        '<table><tr><th rowspan="2">Region</th><th colspan="2">Sales</th></tr>'
        "<tr><th>Q1</th><th>Q2</th></tr><tr><td>EU</td><td>10</td><td>20</td></tr></table>"
    )
    cells = T.css("th, td").map(
        {"text": T.text(), "rows": T.attr("rowspan").int().default(1), "cols": T.attr("colspan").int().default(1)}
    )
    rows = compile(T.css("table tr").map(cells), backend="scraper").extract(html)
    assert rows[0] == [{"text": "Region", "rows": 2, "cols": 1}, {"text": "Sales", "rows": 1, "cols": 2}]
    assert [c["text"] for c in rows[2]] == ["EU", "10", "20"]


def test_case_08_mixed_text_vs_direct_text_nodes() -> None:
    html = "<p>До <b>важного</b> после <i>ещё</i> конец</p>"
    plan = compile({"all": T.css("p").text(), "own": T.xpath("//p/text()")}, backend="lite")
    assert plan.extract(html) == {"all": "До важного после ещё конец", "own": ["До ", " после ", " конец"]}


def test_case_09_broken_html_table_and_foster_parented_text() -> None:
    unclosed_html = "<table><tr><td>A<td>1<tr><td>B<td>2</table>"
    schema = T.css("table tr").map(T.css("td").map(T.text()))
    for backend in ("scraper", "lite"):
        assert compile(schema, backend=backend).extract(unclosed_html) == [["A", "1"], ["B", "2"]]

    foster_html = "<table>hello<tr><td>A</td></tr></table>"
    # html5ever (`scraper`) foster-parents "hello" out of <table>
    scraper_table_text = compile(T.css("table").text(), backend="scraper").extract(foster_html)
    assert scraper_table_text == "A"


# -- 2. Metadata, embedded data & formats -----------------------------------------------------------------------------


def test_case_10_and_11_and_13_and_14_metadata_and_embedded_json() -> None:
    og_html = (
        '<head><meta property="og:image" content="a.jpg"><meta property="og:image:width" content="640">'
        '<meta property="og:image" content="b.jpg"></head>'
    )
    assert compile(T.meta_tags(["og:image"], values="list"), backend="auto").extract(og_html) == {
        "og:image": ["a.jpg", "b.jpg"],
        "og:image:width": ["640"],
    }

    ld_html = (
        '<script type="application/ld+json">not json</script>'
        '<script type="application/ld+json">'
        '{"@graph":[{"@type":"Organization"},{"@type":"Product","name":"Tea","offers":{"price":"2.50"}}]}'
        "</script>"
    )
    assert compile(T.json_ld("Product").map({"name": T["name"], "price": T["offers"]["price"]}), backend="lite").extract(
        ld_html
    ) == [{"name": "Tea", "price": "2.50"}]

    next_html = '<script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"title":"Tea"}}}</script>'
    assert (
        compile(T.css("script#__NEXT_DATA__").text().parse_json()["props"]["pageProps"]["title"], backend="auto").extract(
            next_html
        )
        == "Tea"
    )

    flight_html = (
        "<script>self.__next_f.push([1,\"chunk-a\"])</script>"
        "<script>self.__next_f.push([1,\"chunk-b\"])</script>"
    )
    scripts = compile(T.css("script").map(T.text()), backend="lite").extract(flight_html)
    assert len(scripts) == 2 and all("__next_f.push" in s for s in scripts)


def test_case_15_html_fragment_inside_json_single_plan() -> None:
    plan = compile(
        {
            "items": T["html"].parse_html(T.css("li").map(T.text())),
            "empty": T["empty"].parse_html(T.css("li").map(T.text())),
            "missing": T["missing"].parse_html(T.css("li").map(T.text())),
        },
        backend="json",
    )
    data = {"html": "<ul><li>A</li><li>B</li></ul>", "empty": "", "missing": None}
    assert plan.extract(data) == {"items": ["A", "B"], "empty": [], "missing": None}

    # Verify decompile & validate_ir roundtrip for parse_html
    assert validate_ir(plan.ir) == []
    ns: Dict[str, Any] = {}
    exec(plan.to_python(), ns)
    assert to_ir(ns["schema"]) == plan.ir


def test_case_16_locale_prices_and_quarantine() -> None:
    html = '<p class="p">1.299,50 €</p><p class="p">$1,299.50</p><p class="p">N/A</p>'
    det = compile(T.css("p.p").map(T.text().float(), on_error="skip"), backend="auto").extract_detailed(html)
    assert det.data == [1299.5, 1299.5]
    assert len(det.errors) == 1 and det.errors[0]["path"] == "$[2]"


def test_case_20_windows_1251_charset_decoding() -> None:
    body_text = "<p>Привет</p>"
    with_meta = '<meta charset="windows-1251"><p>Привет</p>'.encode("windows-1251")
    raw_cp1251 = body_text.encode("windows-1251")

    plan = compile(T.css("p").text(), backend="lite")
    # 1. Explicit <meta charset="windows-1251"> detected automatically
    assert plan.extract(with_meta) == "Привет"
    # 2. Explicit encoding= parameter on unlabelled bytes
    assert plan.extract(raw_cp1251, encoding="windows-1251") == "Привет"
    # 3. HTTP Content-Type header in response envelope / FakeResponse
    resp = FakeResponse(content=raw_cp1251, headers={"Content-Type": "text/html; charset=windows-1251"})
    assert plan.extract(resp) == "Привет"
    # 4. BOM (UTF-8 BOM and UTF-16 LE BOM)
    assert plan.extract(b"\xef\xbb\xbf" + body_text.encode("utf-8")) == "Привет"
    assert plan.extract(body_text.encode("utf-16")) == "Привет"
    # 5. Direct helper decode_html()
    assert decode_html(raw_cp1251, content_type="text/html; charset=windows-1251") == body_text
    # 6. Control: unlabelled non-UTF-8 bytes produce UTF-8 replacement characters
    assert "\ufffd" in plan.extract(raw_cp1251)


# -- 3. Navigation, pagination, response headers & cross-format follow ------------------------------------------------


def test_case_21_html_pagination_and_cycle_guard() -> None:
    root = "https://example.test/items"
    pages = {
        root: '<li>A</li><a class="next" href="?page=2">Next</a>',
        root + "?page=2": '<li>B</li><a class="next" href="?page=3">Next</a>',
        root + "?page=3": "<li>C</li>",
    }
    calls: List[str] = []

    def fetch(url: str) -> str:
        calls.append(url)
        return pages[url]

    schema = paginate(items=T.css("li").map(T.text()), next=T.css("a.next").attr("href").url())
    plan = compile(schema, backend="lite")
    assert plan.extract(pages[root], meta={"base_url": root}, fetch=fetch) == ["A", "B", "C"]
    assert calls == [root + "?page=2", root + "?page=3"]

    calls.clear()
    limited = compile(paginate(items=T.css("li").map(T.text()), next=T.css("a.next").attr("href").url(), max_pages=2), backend="lite")
    assert limited.extract(pages[root], meta={"base_url": root}, fetch=fetch) == ["A", "B"]
    assert calls == [root + "?page=2"]

    cycle_pages = {
        root: '<li>A</li><a class="next" href="?page=2">Next</a>',
        root + "?page=2": f'<li>B</li><a class="next" href="{root}">Back</a>',
    }
    assert plan.extract(cycle_pages[root], meta={"base_url": root}, fetch=lambda url: cycle_pages[url]) == ["A", "B"]


def test_case_22_cards_follow_detail_and_cross_format_json_paginate() -> None:
    catalog_url = "https://shop.test/books"
    catalog_html = (
        '<article class="book"><a href="/book/1">Book 1</a></article>'
        '<article class="book"><a href="/book/2">Book 2</a></article>'
    )
    pages = {
        "https://shop.test/book/1": '<h1>Book 1</h1><span class="isbn">111</span><div class="api" data-url="/api/1/reviews?p=1"></div>',
        "https://shop.test/book/2": '<h1>Book 2</h1><span class="isbn">222</span><div class="api" data-url="/api/2/reviews?p=1"></div>',
        "https://shop.test/api/1/reviews?p=1": json.dumps({"reviews": ["good"], "next": "/api/1/reviews?p=2"}),
        "https://shop.test/api/1/reviews?p=2": json.dumps({"reviews": ["great"], "next": None}),
        "https://shop.test/api/2/reviews?p=1": json.dumps({"reviews": [], "next": None}),
    }

    schema = T.css(".book").map(
        {
            "link": T.css("a").attr("href").url(),
            "detail": follow(
                T.css("a").attr("href").url(),
                {
                    "isbn": T.css(".isbn").text(),
                    "reviews": follow(
                        T.css(".api").attr("data-url").url(),
                        paginate(items=T["reviews"], next=T["next"].url()),
                        format="json",
                    ),
                },
            ),
        }
    )
    plan = compile(schema, backend="lite")
    out = plan.extract(catalog_html, meta={"base_url": catalog_url}, fetch=lambda url: pages[url])
    assert out == [
        {"link": "https://shop.test/book/1", "detail": {"isbn": "111", "reviews": ["good", "great"]}},
        {"link": "https://shop.test/book/2", "detail": {"isbn": "222", "reviews": []}},
    ]


def test_case_23_json_truthiness_nullness_and_graphql_cursor_pagination() -> None:
    # Regression matrix for JsonDom truthiness and JSON null handling (Case #23 bug fix)
    bool_plan = compile(
        {
            "t_if": T.if_(T["t"], "yes", "no"),
            "f_if": T.if_(T["f"], "yes", "no"),
            "n_if": T.if_(T["n"], "yes", "no"),
            "z_if": T.if_(T["z"], "yes", "no"),
            "e_if": T.if_(T["e"], "yes", "no"),
            "f_exists": T["f"].exists(),
            "n_exists": T["n"].exists(),
            "n_def": T["n"].default("fb"),
            "miss_def": T["missing"].default("fb"),
            "blank_ne": T["blank"].non_empty().default("fb"),
            "n_int": T["n"].int(),
        },
        backend="json",
    )
    sample = {"t": True, "f": False, "n": None, "z": 0, "e": [], "blank": "   "}
    assert bool_plan.extract(sample) == {
        "t_if": "yes",
        "f_if": "no",
        "n_if": "no",
        "z_if": "no",
        "e_if": "no",
        "f_exists": False,
        "n_exists": False,
        "n_def": "fb",
        "miss_def": "fb",
        "blank_ne": "fb",
        "n_int": None,
    }

    # GraphQL cursor pagination WITHOUT explicit `.eq(True)`
    initial = {"data": {"items": {"nodes": [{"name": "A"}, {"name": "B"}], "pageInfo": {"hasNextPage": True, "endCursor": "c1"}}}}
    second = {"data": {"items": {"nodes": [{"name": "C"}], "pageInfo": {"hasNextPage": False, "endCursor": None}}}}
    calls: List[Any] = []

    def fetch(url: str, variables: Dict[str, Any]) -> str:
        calls.append((url, variables))
        return json.dumps(second)

    items = T["data"]["items"]
    info = items["pageInfo"]
    schema = paginate(
        items=items["nodes"].map(T["name"]),
        next=T.if_(info["hasNextPage"], {"url": "https://example.test/graphql", "variables": {"after": info["endCursor"]}}, None),
    )
    assert compile(schema, backend="json").extract(initial, fetch=fetch) == ["A", "B", "C"]
    assert calls == [("https://example.test/graphql", {"after": "c1"})]


def test_case_24_rest_link_header_pagination_and_envelopes() -> None:
    p1_url = "https://api.example/items?page=1"
    p2_url = "https://api.example/items?page=2"
    responses = {
        p1_url: FakeResponse(
            content=json.dumps([{"id": 1}]).encode("utf-8"),
            headers={"Link": f'<{p1_url}&x=1,2>; rel="first", <{p2_url}>; rel="next", <https://api.example/items?page=9>; rel="last"'},
            url=p1_url,
        ),
        p2_url: (
            json.dumps([{"id": 2}]),
            {"headers": {"content-type": "application/json"}, "status": 200},
        ),
    }
    schema = paginate(
        items=T.map(T["id"]),
        next=T.meta("headers")["link"].link_header("next"),
    )
    plan = compile(schema, backend="json")
    assert plan.extract(responses[p1_url], fetch=lambda url: responses[url]) == [1, 2]
    assert validate_ir(plan.ir) == []
    ns: Dict[str, Any] = {}
    exec(plan.to_python(), ns)
    assert to_ir(ns["schema"]) == plan.ir


def test_case_27_iframe_src_and_srcdoc() -> None:
    html = (
        '<iframe id="ext" src="/frame.html"></iframe>'
        '<iframe id="inline" srcdoc="<ul><li>In1</li><li>In2</li></ul>"></iframe>'
    )
    pages = {"https://example.test/frame.html": "<ul><li>Ext1</li><li>Ext2</li></ul>"}
    schema = {
        "ext": follow(T.css("#ext").attr("src").url(), T.css("li").map(T.text())),
        "inline": T.css("#inline").attr("srcdoc").parse_html(T.css("li").map(T.text())),
    }
    for backend in ("lite", "scraper"):
        out = compile(schema, backend=backend).extract(html, meta={"base_url": "https://example.test"}, fetch=lambda url: pages[url])
        assert out == {"ext": ["Ext1", "Ext2"], "inline": ["In1", "In2"]}


def test_case_28_retry_transport_wrapper_over_fetch() -> None:
    attempts: Dict[str, int] = {}
    html = "<a href='/next'>Next</a>"

    def flaky_fetch(url: str) -> str:
        for _ in range(3):
            attempts[url] = attempts.get(url, 0) + 1
            if attempts[url] >= 2:
                return "<b>Recovered</b>"
        raise RuntimeError("unreachable")

    out = compile(follow(T.css("a").attr("href").url(), T.css("b").text()), backend="lite").extract(
        html,
        meta={"base_url": "https://example.test"},
        fetch=flaky_fetch,
    )
    assert out == "Recovered"
    assert attempts["https://example.test/next"] == 2


# -- 4. Resilience, cleanup & migration -------------------------------------------------------------------------------


def test_case_29_adaptive_redesign_and_negative_neighbor_check() -> None:
    v1 = '<section><div class="product" data-sku="A1"><h3 class="name">Tea</h3><span class="price">$10</span></div></section>'
    v2_renamed = '<section><div class="product" data-sku="A1"><h3 class="name">Tea</h3><span class="cost">$10</span></div></section>'
    v2_missing_target = (
        '<section><div class="other" data-sku="B2"><h3 class="name">Coffee</h3><span class="cost">$99</span></div></section>'
    )
    schema = {"price": T.css(".product .price", adaptive="price").text()}
    plan = compile(schema, backend="lite")
    assert plan.extract(v1) == {"price": "$10"}
    assert plan.extract(v2_renamed) == {"price": "$10"}
    # Negative check with stricter threshold catches when target disappeared cleanly via detailed misses
    strict = compile({"price": T.css(".product .price", adaptive="price", threshold=0.8).text()}, backend="lite")
    assert strict.extract(v1) == {"price": "$10"}
    det = strict.extract_detailed(v2_missing_target)
    assert det.data == {"price": None} and len(det.misses) == 1


def test_case_30_record_quarantine_and_batch_errors() -> None:
    html = "<b>10</b><b>N/A</b><b>20</b>"
    assert compile(T.css("b").map(T.text().int(), on_error="skip"), backend="auto").extract(html) == [10, 20]
    assert compile(T.css("b").map(T.text().int(), on_error="null"), backend="auto").extract(html) == [10, None, 20]
    with pytest.raises(Exception, match="int:"):
        compile(T.css("b").map(T.text().int()), backend="auto").extract(html)

    results, failures = compile(T.css("b").text().int(), backend="auto").extract_batch(["<b>1</b>", "<b>bad</b>"], errors="collect")
    assert results == [1, None] and len(failures) == 1 and failures[0]["index"] == 1


def test_case_31_article_markdown_without_ads() -> None:
    html = '<article><h1>Guide</h1><div class="ad">Buy!</div><p>Read <a href="/x">docs</a>.</p><pre><code>x = 1</code></pre></article>'
    md = compile(T.css("article").markdown(without=".ad"), backend="lite").extract(html)
    assert "# Guide" in md and "[docs](/x)" in md and "x = 1" in md and "Buy!" not in md


def test_case_32_migration_and_computed_error() -> None:
    html = '<div class="card"><h2>Item</h2><a href="/1">go</a></div><div class="card"><h2>Two</h2></div>'
    c4 = {
        "baseSelector": ".card",
        "fields": [
            {"name": "title", "selector": "h2", "type": "text"},
            {"name": "link", "selector": "a", "type": "attribute", "attribute": "href", "default": "/none"},
        ],
    }
    pm = {"cards": {"_base": ".card", "title": "h2::text", "link": "a::attr(href)"}}
    assert Plan.from_ir(migrate.from_crawl4ai(c4), "lite").extract(html) == [
        {"title": "Item", "link": "/1"},
        {"title": "Two", "link": "/none"},
    ]
    assert Plan.from_ir(migrate.from_selector_map(pm), "lite").extract(html) == {
        "cards": [{"title": "Item", "link": "/1"}, {"title": "Two", "link": None}]
    }
    with pytest.raises(migrate.MigrationError, match="computed"):
        migrate.from_crawl4ai({"baseSelector": ".card", "fields": [{"name": "x", "type": "computed", "expression": "a+b"}]})


def test_case_33_unique_skus_across_pages() -> None:
    p1 = "https://shop.test/list?p=1"
    pages = {
        p1: '<i data-sku="A">10</i><i data-sku="B">20</i><a class="next" href="?p=2">next</a>',
        "https://shop.test/list?p=2": '<i data-sku="B">18</i><i data-sku="C">30</i>',
    }
    schema = paginate(
        items=T.css("i").map({"sku": T.attr("data-sku"), "price": T.text().int()}),
        next=T.css("a.next").attr("href").url(),
    )
    rows = compile(schema, backend="lite").extract(pages[p1], meta={"base_url": p1}, fetch=lambda url: pages[url])
    dedup_last = {r["sku"]: r for r in rows}
    assert list(dedup_last.keys()) == ["A", "B", "C"]
    assert dedup_last["B"]["price"] == 18


# -- Wave 2 coverage ---------------------------------------------------------------------------------------------------


def test_wave2_case_07_table_modes_spans_and_nested_isolation() -> None:
    html = """
    <table id="t">
      <thead>
        <tr><th rowspan="2">Metric</th><th colspan="2">2025</th></tr>
        <tr><th>Q1</th><th>Q2</th></tr>
      </thead>
      <tbody>
        <tr><td>Rev<table><tr><td>nested</td></tr></table></td><td>100</td><td rowspan="2">200</td></tr>
        <tr><td>Cost</td><td>40</td></tr>
      </tbody>
      <tfoot>
        <tr><td rowspan="0">Foot</td><td colspan="2">Total</td></tr>
      </tfoot>
    </table>
    """
    for backend in ("lite", "scraper"):
        records = compile(T.css("#t").table("records"), backend=backend).extract(html)
        assert records == [
            {"Metric": "Rev", "2025 / Q1": "100", "2025 / Q2": "200"},
            {"Metric": "Cost", "2025 / Q1": "40", "2025 / Q2": "200"},
            {"Metric": "Foot", "2025 / Q1": "Total", "2025 / Q2": "Total"},
        ]
        records_pipe = compile(T.css("#t").table("records", header_sep="|"), backend=backend).extract(html)
        assert records_pipe[0] == {"Metric": "Rev", "2025|Q1": "100", "2025|Q2": "200"}
        split = compile(T.css("#t").table("split"), backend=backend).extract(html)
        assert split == {
            "headers": [["Metric", "2025", "2025"], ["Metric", "Q1", "Q2"]],
            "rows": [
                ["Rev", "100", "200"],
                ["Cost", "40", "200"],
                ["Foot", "Total", "Total"],
            ],
        }
        grid = compile(T.css("#t").table("grid"), backend=backend).extract(html)
        assert grid[0] == ["Metric", "2025", "2025"]
        assert grid[1] == ["Metric", "Q1", "Q2"]
        assert grid[2] == ["Rev", "100", "200"]
        assert grid[3] == ["Cost", "40", "200"]
        assert len(grid) == 5


def test_wave2_case_08_own_text_and_own_texts_engine_parity() -> None:
    html = (
        '<div class="box">'
        '  <h1>1299 <span class="cur">EUR</span><script>bad()</script></h1>'
        '  <p class="lines">Alpha<span>skip <b>deep</b></span><br>Beta<b>2</b>Gamma</p>'
        "</div>"
    )
    schema = {
        "price_own": T.css(".box h1").own_text(strip=True),
        "price_deep_false": T.css(".box h1").text(strip=True, deep=False),
        "lines_joined": T.css(".lines").own_text(" ", strip=True, br="\n"),
        "lines_parts": T.css(".lines").own_texts(),
        "missing": T.css(".missing").own_text(),
    }
    expected = {
        "price_own": "1299",
        "price_deep_false": "1299",
        "lines_joined": "Alpha\nBeta Gamma",
        "lines_parts": ["Alpha", "Beta", "Gamma"],
        "missing": None,
    }
    for backend in ("stream", "lite", "scraper"):
        assert compile(schema, backend=backend).extract(html) == expected


def test_wave2_case_16_price_locale_and_currency() -> None:
    html = (
        '<i class="a">$1,299.50</i>'
        '<i class="b">1.299,50 €</i>'
        '<i class="c">12 990 ₽</i>'
        '<i class="d">US$ 49.90</i>'
        '<i class="e">19,95</i>'
        '<i class="f">N/A</i>'
    )
    schema = {
        "a": T.css(".a").text().price(),
        "b": T.css(".b").text().price(),
        "c": T.css(".c").text().price(),
        "d": T.css(".d").text().price(),
        "e": T.css(".e").text().price(decimal=",", currency="CHF"),
        "f": T.css(".f").text().price(),
    }
    assert compile(schema, backend="stream").extract(html) == {
        "a": {"amount": 1299.5, "currency": "USD"},
        "b": {"amount": 1299.5, "currency": "EUR"},
        "c": {"amount": 12990.0, "currency": "RUB"},
        "d": {"amount": 49.9, "currency": "USD"},
        "e": {"amount": 19.95, "currency": "CHF"},
        "f": None,
    }


def test_wave2_case_17_parse_srcset_whatwg_cdn_commas() -> None:
    html = (
        '<img id="cdn" srcset="'
        "https://cdn.ex/cdn-cgi/image/w=400,f=auto,q=80/shoe.jpg 400w, "
        "https://cdn.ex/cdn-cgi/image/w=1200,f=auto,q=80/shoe.jpg 1200w, "
        'https://cdn.ex/cdn-cgi/image/w=800,f=auto,q=80/shoe.jpg 800w">'
        '<img id="ret" srcset="a.png, b@2x.png 2x, c@3x.png 3x">'
        '<img id="empty" srcset="   ">'
    )
    schema = {
        "all": T.css("#cdn").attr("srcset").parse_srcset(),
        "max": T.css("#cdn").attr("srcset").parse_srcset("max")["url"],
        "min": T.css("#cdn").attr("srcset").parse_srcset("min")["url"],
        "mw": T.css("#cdn").attr("srcset").parse_srcset(min_width=700)["url"],
        "ret": T.css("#ret").attr("srcset").parse_srcset(min_density=1.5)["url"],
        "empty": T.css("#empty").attr("srcset").parse_srcset("max"),
    }
    res = compile(schema, backend="stream").extract(html)
    assert len(res["all"]) == 3
    assert res["all"][0] == {"url": "https://cdn.ex/cdn-cgi/image/w=400,f=auto,q=80/shoe.jpg", "width": 400, "density": None}
    assert res["max"] == "https://cdn.ex/cdn-cgi/image/w=1200,f=auto,q=80/shoe.jpg"
    assert res["min"] == "https://cdn.ex/cdn-cgi/image/w=400,f=auto,q=80/shoe.jpg"
    assert res["mw"] == "https://cdn.ex/cdn-cgi/image/w=800,f=auto,q=80/shoe.jpg"
    assert res["ret"] == "b@2x.png"
    assert res["empty"] is None


def test_wave2_case_18_xml_sitemap_rss_atom_namespaces_and_follow() -> None:
    sitemap = """<?xml version="1.0" encoding="UTF-8"?>
    <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
      <url><loc>https://ex.test/a</loc><priority>0.8</priority></url>
      <url><loc>https://ex.test/b</loc><priority>0.5</priority></url>
    </urlset>
    """
    sm_schema = T.css("url").map({"loc": T.css("loc").text(), "prio": T.css("priority").text().float()})
    assert compile(sm_schema, backend="xml").extract(sitemap) == [
        {"loc": "https://ex.test/a", "prio": 0.8},
        {"loc": "https://ex.test/b", "prio": 0.5},
    ]

    rss = """<?xml version="1.0"?>
    <rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom" xmlns:dc="http://purl.org/dc/elements/1.1/">
      <channel>
        <atom:link href="https://ex.test/feed.xml" rel="self"/>
        <item>
          <title><![CDATA[Hello <World> & Co]]></title>
          <link/>
          <dc:creator>Ada</dc:creator>
        </item>
      </channel>
    </rss>
    """
    rss_schema = {
        "self_css": T.css("atom|link").attr("href"),
        "self_any": T.css("*|link").attr("href"),
        "self_xpath": T.xpath("//atom:link/@href").at(0),
        "title": T.css("item title").text(),
        "creator_local": T.xpath("//*[local-name()='creator']").text(),
        "creator_ns": T.xpath("namespace-uri(//dc:creator)"),
        "all_ns_wc": T.xpath("//atom:*/@rel").at(0),
    }
    assert compile(rss_schema, backend="xml").extract(rss) == {
        "self_css": "https://ex.test/feed.xml",
        "self_any": "https://ex.test/feed.xml",
        "self_xpath": "https://ex.test/feed.xml",
        "title": "Hello <World> & Co",
        "creator_local": "Ada",
        "creator_ns": "http://purl.org/dc/elements/1.1/",
        "all_ns_wc": "self",
    }

    atom = """<?xml version="1.0" encoding="utf-8"?>
    <feed xmlns="http://www.w3.org/2005/Atom">
      <entry>
        <title>Entry 1</title>
        <link rel="alternate" href="https://ex.test/post-1"/>
        <summary>Summary text</summary>
      </entry>
    </feed>
    """
    atom_schema = T.css("entry").map(
        {
            "title": T.css("title").text(),
            "href": T.css("link").attr("href"),
            "summary": T.css("summary").text(),
        }
    )
    assert compile(atom_schema, backend="xml").extract(atom) == [
        {"title": "Entry 1", "href": "https://ex.test/post-1", "summary": "Summary text"}
    ]

    html_page = '<head><link rel="sitemap" href="https://ex.test/sitemap.xml"></head>'
    cross = follow(
        T.css('link[rel="sitemap"]').attr("href"),
        T.css("url").map(T.css("loc").text()),
        format="xml",
    )
    assert compile(cross, backend="auto").extract(html_page, fetch=lambda url: sitemap) == [
        "https://ex.test/a",
        "https://ex.test/b",
    ]


def test_wave2_case_32_opt_in_own_text_and_sibling_source() -> None:
    html_own = '<div class="price">499 <span>RUB</span></div>'
    ir_own = migrate.from_selector_map({"amount": ".price::text"}, own_text=True)
    assert Plan.from_ir(ir_own, "stream").extract(html_own) == {"amount": "499"}

    html_sib = (
        "<table><tbody>"
        '<tr class="row"><td class="title">Rust Guide</td></tr>'
        '<tr class="sub"><td class="pts">142 points</td></tr>'
        '<tr class="row"><td class="title">No Score</td></tr>'
        "</tbody></table>"
    )
    c4_sib = {
        "baseSelector": "tr.row",
        "fields": [
            {"name": "title", "selector": "td.title", "type": "text"},
            {"name": "points", "source": "+ tr.sub", "selector": "td.pts", "type": "text"},
        ],
    }
    with pytest.raises(migrate.MigrationError, match="sibling"):
        migrate.from_crawl4ai(c4_sib)
    ir_sib = migrate.from_crawl4ai(c4_sib, allow_sibling_source=True)
    assert validate_ir(ir_sib) == []
    assert Plan.from_ir(ir_sib, "lite").extract(html_sib) == [
        {"title": "Rust Guide", "points": "142 points"},
        {"title": "No Score", "points": None},
    ]


def test_wave2_case_33_unique_and_paginate_unique_by() -> None:
    html = "<ul><li>A</li><li>B</li><li>A</li><li>C</li></ul>"
    assert compile(T.css("li").map(T.text()).unique(), backend="auto").extract(html) == ["A", "B", "C"]
    rows_html = '<i data-k="1">v1</i><i data-k="2">v2</i><i data-k="1">v1b</i>'
    rows = T.css("i").map({"k": T.attr("data-k"), "v": T.text()})
    assert compile(rows.unique(T["k"], keep="first"), backend="lite").extract(rows_html) == [
        {"k": "1", "v": "v1"},
        {"k": "2", "v": "v2"},
    ]
    assert compile(rows.unique(T["k"], keep="last"), backend="lite").extract(rows_html) == [
        {"k": "1", "v": "v1b"},
        {"k": "2", "v": "v2"},
    ]

    p1 = "https://shop.test/list?p=1"
    pages = {
        p1: '<i data-sku="A">10</i><i data-sku="B">20</i><a class="next" href="?p=2">n</a>',
        "https://shop.test/list?p=2": '<i data-sku="B">18</i><i data-sku="C">30</i><a class="next" href="?p=3">n</a>',
        "https://shop.test/list?p=3": '<i data-sku="D">40</i>',
    }
    base_items = T.css("i").map({"sku": T.attr("data-sku"), "price": T.text().int()})
    next_link = T.css("a.next").attr("href").url()

    first_res = compile(
        paginate(base_items, next_link, unique_by=T["sku"], keep="first", max_items=3),
        backend="lite",
    ).extract(pages[p1], meta={"base_url": p1}, fetch=lambda url: pages[url])
    assert first_res == [{"sku": "A", "price": 10}, {"sku": "B", "price": 20}, {"sku": "C", "price": 30}]

    last_res = compile(
        paginate(base_items, next_link, unique_by=T["sku"], keep="last"),
        backend="lite",
    ).extract(pages[p1], meta={"base_url": p1}, fetch=lambda url: pages[url])
    assert last_res == [
        {"sku": "A", "price": 10},
        {"sku": "B", "price": 18},
        {"sku": "C", "price": 30},
        {"sku": "D", "price": 40},
    ]


# -- Wave 3 coverage ---------------------------------------------------------------------------------------------------


def test_wave3_case_11_json_ld_id_ref_resolution() -> None:
    html = """
    <script type="application/ld+json">
    {
      "@context": "https://schema.org",
      "@graph": [
        {"@type": "Organization", "@id": "#org", "name": "TeaCo"},
        {"@type": "Offer", "@id": "#offer", "price": "2.50", "priceCurrency": "USD", "seller": {"@id": "#org"}},
        {"@type": "Product", "name": "Matcha", "offers": {"@id": "#offer"}, "brand": {"@id": "#missing"}},
        {"@type": "Offer", "@id": "#loop", "price": "9.00", "next": {"@id": "#loop"}}
      ]
    }
    </script>
    """
    for backend in ("lite", "scraper"):
        products = compile(T.json_ld("Product", resolve_refs=True), backend=backend).extract(html)
        assert len(products) == 1
        assert products[0]["name"] == "Matcha"
        assert products[0]["offers"]["price"] == "2.50"
        assert products[0]["offers"]["seller"]["name"] == "TeaCo"
        assert products[0]["brand"] == {"@id": "#missing"}

        raw = compile(T.json_ld("Product"), backend=backend).extract(html)
        assert raw[0]["offers"] == {"@id": "#offer"}

        loop_offers = compile(T.json_ld("Offer", resolve_refs=True), backend=backend).extract(html)
        assert len(loop_offers) == 2 and loop_offers[1]["price"] == "9.00"


def test_wave3_case_12_microdata_itemscope_itemref_and_nested() -> None:
    html = """
    <div id="shared-brand">
      <span itemprop="brand">Acme</span>
      <meta itemprop="sku" content="SKU-99">
    </div>
    <div itemscope itemtype="https://schema.org/Product" itemid="urn:p:1" itemref="shared-brand">
      <h1 itemprop="name">  Tea Kettle  </h1>
      <img itemprop="image" src="https://cdn.ex/1.jpg">
      <img itemprop="image" src="https://cdn.ex/2.jpg">
      <a itemprop="url" href="/p/1">View</a>
      <time itemprop="releaseDate" datetime="2025-02-01">Feb 2025</time>
      <div itemprop="offers" itemscope itemtype="https://schema.org/Offer">
        <meta itemprop="price" content="49.90">
        <span itemprop="priceCurrency">USD</span>
      </div>
    </div>
    """
    for backend in ("lite", "scraper"):
        items = compile(T.microdata("Product"), backend=backend).extract(html)
        assert len(items) == 1
        p = items[0]
        assert p["@type"] == "https://schema.org/Product"
        assert p["@id"] == "urn:p:1"
        assert p["name"] == "Tea Kettle"
        assert p["brand"] == "Acme"
        assert p["sku"] == "SKU-99"
        assert p["image"] == ["https://cdn.ex/1.jpg", "https://cdn.ex/2.jpg"]
        assert p["url"] == "/p/1"
        assert p["releaseDate"] == "2025-02-01"
        assert p["offers"] == {"@type": "https://schema.org/Offer", "price": "49.90", "priceCurrency": "USD"}


def test_wave3_case_14_nextjs_app_router_flight_segments() -> None:
    html = """
    <html><body>
      <script>self.__next_f.push([1,"2:{\\"badge\\":\\"Top Seller\\",\\"cost\\":\\"$$19.99\\"}\\n1:[\\"$\\",\\"div\\",null,{\\"title\\":\\"Shop\\",\\"products\\":[{\\"id\\":10,\\"tag\\":\\"$2\\"}]"])</script>
      <script>self.__next_f.push([1,"}]\\n3:T9,Hello RSC\\n"])</script>
    </body></html>
    """
    schema = {
        "all": T.next_flight(),
        "products": T.next_flight("products")[0],
        "stream_first": T.css("script").text().next_flight("badge"),
    }
    for backend in ("lite", "scraper"):
        res = compile(schema, backend=backend).extract(html)
        assert res["products"] == [{"id": 10, "tag": {"badge": "Top Seller", "cost": "$19.99"}}]
        assert "Hello RSC" in res["all"]
        assert res["stream_first"] == ["Top Seller"]

    assert compile({"b": T.css("script").at(0).text().next_flight("badge")}, backend="stream").extract(html) == {
        "b": ["Top Seller"]
    }


def test_wave3_case_19_jsonl_and_csv_multiline_quotes() -> None:
    jsonl = '{"id": 1, "name": "Alpha"}\n\n{"id": "2", "name": "Beta"}\n'
    p_json = compile({"id": T["id"].int(), "name": T["name"]}, backend="json")
    assert p_json.extract_jsonl(jsonl) == [{"id": 1, "name": "Alpha"}, {"id": 2, "name": "Beta"}]

    bad_jsonl = '{"id": 1, "name": "Ok"}\n{broken\n{"id": "bad_int", "name": "No"}\n{"id": 4, "name": "Good"}\n'
    rows, failures = p_json.extract_jsonl(bad_jsonl, errors="collect")
    assert rows == [{"id": 1, "name": "Ok"}, None, None, {"id": 4, "name": "Good"}]
    assert [f["index"] for f in failures] == [1, 2]
    assert [f["line"] for f in failures] == [2, 3]

    csv_text = 'id,title,note\n1,"Tea","line 1\nline 2, extra"\n2,"Cup","ok"\n'
    assert p_json.extract_csv(csv_text) == [{"id": 1, "name": None}, {"id": 2, "name": None}]
    csv_plan = compile({"id": T["id"].int(), "title": T["title"], "note": T["note"]}, backend="json")
    assert csv_plan.extract_csv(csv_text) == [
        {"id": 1, "title": "Tea", "note": "line 1\nline 2, extra"},
        {"id": 2, "title": "Cup", "note": "ok"},
    ]


def test_wave3_case_25_bfs_link_graph_crawl_canonicalization_and_scope() -> None:
    assert canonicalize_url("/x?b=2&a=1#frag", base_url="https://Ex.Test:443") == "https://ex.test/x?a=1&b=2"
    graph = {
        "https://ex.test/": "<h1>A</h1><a href='/b#sec'>b</a><a href='/c?x=1&y=2'>c</a><a href='https://other.test/z'>ext</a>",
        "https://ex.test/b": "<h1>B</h1><a href='/d'>d</a>",
        "https://ex.test/c?x=1&y=2": "<h1>C</h1><a href='/d#top'>d</a>",
        "https://ex.test/d": "<h1>D</h1><a href='/'>home</a><a href='/e'>too-deep</a>",
        "https://ex.test/e": "<h1>E</h1>",
    }
    fetches: List[str] = []

    def fetch(url: str) -> str:
        fetches.append(url)
        return graph[url]

    pages = crawl("https://ex.test/", T.css("h1").text(), fetch=fetch, max_depth=2, max_pages=10, same_domain=True)
    assert [(p.data, p.depth) for p in pages] == [("A", 0), ("B", 1), ("C", 1), ("D", 2)]
    assert fetches == ["https://ex.test/", "https://ex.test/b", "https://ex.test/c?x=1&y=2", "https://ex.test/d"]


def test_wave3_case_26_and_27_and_34_dynamic_shadow_and_pdf_contracts() -> None:
    # Case #26: Load-more rendered HTML vs JSON endpoint parity & termination when no new items/cursor remain
    rendered_after_click = '<ul><li class="rev">Good</li><li class="rev">Great</li><li class="rev">Fast</li><li class="rev">Solid</li></ul>'
    api_pages = {
        "https://shop.test/reviews?c=0": json.dumps({"reviews": ["Good", "Great"], "next": "c2"}),
        "https://shop.test/reviews?c=c2": json.dumps({"reviews": ["Fast", "Solid"], "next": None}),
    }
    html_reviews = compile({"items": T.css("li.rev").map(T.text())}, backend="stream").extract(rendered_after_click)["items"]
    cursor_schema = paginate(
        items=T["reviews"],
        next=T.if_(T["next"].eq(None), None, T["next"]),
    )
    fetched = compile(cursor_schema, backend="json").extract(
        api_pages["https://shop.test/reviews?c=0"],
        fetch=lambda url: api_pages[f"https://shop.test/reviews?c={url}"],
    )
    assert html_reviews == fetched == ["Good", "Great", "Fast", "Solid"]

    # Case #27: Open Shadow DOM serialized fragment & iframe[srcdoc] parity
    shadow_fragment = "<ul><li class='item'>One</li><li class='item'>Two</li></ul>"
    doc = {"shadow_html": shadow_fragment}
    assert compile(T["shadow_html"].parse_html(T.css(".item").map(T.text())), backend="json").extract(doc) == ["One", "Two"]

    # Case #34: Normalized PDF text-layer / OCR records with quarantine on bad scan row
    ocr_rows = [
        {"name": "Widget A", "price": "$12.50"},
        {"name": "Blurry OCR Line", "price": "N/A"},
        {"name": "Widget B", "price": "19,90 €"},
    ]
    ocr_plan = compile(T.map({"name": T["name"], "price": T["price"].float()}, on_error="skip"), backend="json")
    det = ocr_plan.extract_detailed(ocr_rows)
    assert det.data == [{"name": "Widget A", "price": 12.5}, {"name": "Widget B", "price": 19.9}]
    assert len(det.errors) == 1 and det.errors[0]["path"] == "$[1].price"


def test_wave3_case_31_main_content_discovery_and_markdown() -> None:
    page_a = """
    <html><body>
      <header class="site-header"><nav><a>Home</a><a>Docs</a></nav></header>
      <div class="layout">
        <aside class="sidebar"><a>Popular 1</a><a>Popular 2</a><a>Related link</a></aside>
        <article class="post">
          <h1>Guide One</h1>
          <div class="ad">Sponsored link!</div>
          <p>Oxide Parse extracts structured data reliably, cleanly, and quickly from varied HTML documents.</p>
          <p>Multiple paragraphs with prose, commas, and explanations give this article clear readability density.</p>
        </article>
      </div>
      <footer>Copyright 2026</footer>
    </body></html>
    """
    page_b = """
    <html><body>
      <nav><a>Menu</a></nav>
      <div id="cookie-banner">Accept cookies</div>
      <div class="story-body">
        <h1>Guide Two</h1>
        <p>Unfamiliar websites use generic wrapper classes instead of semantic article elements for long prose.</p>
        <p>Scoring density, comma counts, and negative tokens finds the true story body reliably across themes.</p>
      </div>
      <div class="comments"><p>Short</p></div>
    </body></html>
    """
    for backend in ("lite", "scraper"):
        md_a = markdown(page_a, main=True, without=".ad", backend=backend)
        assert "# Guide One" in md_a and "Oxide Parse extracts" in md_a and "Sponsored" not in md_a and "Popular" not in md_a

        md_b = compile(T.main_content().markdown(without=".ad"), backend=backend).extract(page_b)
        assert "# Guide Two" in md_b and "Unfamiliar websites" in md_b and "Accept cookies" not in md_b
