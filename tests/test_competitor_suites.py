"""Differential tests comparing oxide_parse against upstream competitor libraries on their own test patterns."""
from __future__ import annotations

import json
import sys
from pathlib import Path
from urllib.parse import urljoin

import pytest

pytest.importorskip("oxide_parse._core")
from oxide_parse import Plan, T, compile, engines, markdown, migrate  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
BENCHES_V7 = ROOT / "benches" / "suite_v7"
if str(BENCHES_V7) not in sys.path:
    sys.path.insert(0, str(BENCHES_V7))

import competitors as _comp  # noqa: E402

_comp.ensure_local_bench_deps()

AVAILABLE = {e.name for e in engines()}
HTML_ENGINES = [e for e in ("stream", "lite", "scraper") if e in AVAILABLE]
DOM_ENGINES = [e for e in ("lite", "scraper") if e in AVAILABLE]


# ---------------------------------------------------------------------------
# 1. Frostwork (frostwork.Page, frostwork.navigation)
# ---------------------------------------------------------------------------
def test_frostwork_flat_page_parity():
    fw = pytest.importorskip("frostwork")
    html = (
        '<div class="product">'
        '<meta itemprop="brand" content="Acme">'
        "<h1>Widget</h1>"
        '<span class="price">$9</span>'
        '<img src="/a.png"><img src="/b.png">'
        '<span class="spec">100g</span><span class="spec">USB-C</span>'
        "</div>"
    )
    fw_css = (
        fw.Page()
        .field("name", "h1::text")
        .field("price", ".price::text")
        .field_all("images", "img::attr(src)")
        .field_join("specs", ".spec::text", " ")
    )
    expected_css = fw_css.extract(html).to_dict()
    schema_css = {
        "name": T.css("h1").text(),
        "price": T.css(".price").text(),
        "images": T.css("img").map(T.attr("src")),
        "specs": T.css(".spec").map(T.text()).join(" "),
    }
    for eng in HTML_ENGINES:
        assert compile(schema_css, backend=eng).extract(html) == expected_css

    # Same flat selectors via deterministic selector-map migrator
    migrated = migrate.from_selector_map(
        {"name": "h1::text", "price": ".price::text", "images": ["img::attr(src)"]},
        strip=False,
    )
    for eng in HTML_ENGINES:
        got = Plan.from_ir(migrated, backend=eng).extract(html)
        assert got == {k: expected_css[k] for k in ("name", "price", "images")}

    # XPath field from frostwork.webpoet docs (`//meta[@itemprop='brand']/@content`)
    fw_xp = fw.Page().field("brand", "//meta[@itemprop='brand']/@content")
    expected_xp = fw_xp.extract(html).to_dict()
    for eng in DOM_ENGINES:
        assert compile({"brand": T.xpath("//meta[@itemprop='brand']/@content").first()}, backend=eng).extract(html) == expected_xp


def test_frostwork_grouped_many_and_one_parity():
    fw = pytest.importorskip("frostwork")
    html = """
    <html><body>
      <h1>Catalog</h1>
      <div class="featured"><h2>Hero Deal</h2><a href="/hero">Go</a></div>
      <article class="product" data-id="101">
        <h2 class="name">Keyboard</h2><span class="price">129.90</span>
        <a class="detail" href="/p/101">Details</a>
        <span class="tag">mechanical</span><span class="tag">rgb</span>
      </article>
      <article class="product" data-id="102">
        <h2 class="name">Mouse</h2><span class="price">39.50</span>
        <a class="detail" href="/p/102">Details</a>
        <span class="tag">wireless</span>
      </article>
    </body></html>
    """
    # Flat grouped schema (streamable in oxide_parse)
    fw_flat = (
        fw.Page()
        .field("heading", "h1::text")
        .many("products", ".product", {
            "id": "::attr(data-id)",
            "name": ".name::text",
            "price": ".price::text",
            "url": "a.detail::attr(href)",
        })
    )
    expected_flat = fw_flat.extract(html).to_dict()
    schema_flat = {
        "heading": T.css("h1").text(),
        "products": T.css(".product").map({
            "id": T.attr("data-id"),
            "name": T.css(".name").text(),
            "price": T.css(".price").text(),
            "url": T.css("a.detail").attr("href"),
        }),
    }
    for eng in HTML_ENGINES:
        assert compile(schema_flat, backend=eng).extract(html) == expected_flat

    # Nested `one()` + `many()` with `("...", "all")` sub-field cardinality (DOM engines)
    fw_rich = (
        fw.Page()
        .field("heading", "h1::text")
        .one("featured", ".featured", {"title": "h2::text", "url": "a::attr(href)"})
        .many("products", ".product", {
            "id": "::attr(data-id)",
            "name": ".name::text",
            "tags": (".tag::text", "all"),
        })
    )
    expected_rich = fw_rich.extract(html).to_dict()
    schema_rich = {
        "heading": T.css("h1").text(),
        "featured": T.css(".featured")[:1].map({"title": T.css("h2").text(), "url": T.css("a").attr("href")}).at(0),
        "products": T.css(".product").map({
            "id": T.attr("data-id"),
            "name": T.css(".name").text(),
            "tags": T.css(".tag").map(T.text()),
        }),
    }
    for eng in DOM_ENGINES:
        assert compile(schema_rich, backend=eng).extract(html) == expected_rich


@pytest.mark.parametrize(
    ("doc_url", "html"),
    [
        ("https://site.com/catalog/item", '<head><base href="/shop/"></head><a href="123">x</a>'),
        ("https://site.com/catalog/item", '<head><base href="https://cdn.example/dir/"></head><a href="123">x</a>'),
        ("https://site.com/catalog/item", '<a href="123">x</a>'),
    ],
)
def test_frostwork_base_url_navigation_parity(doc_url, html):
    fw = pytest.importorskip("frostwork")
    nav = fw.navigation(html, doc_url)
    expected = urljoin(nav.base_url or doc_url, "123")
    plan_ir = {"u": T.css("a").attr("href").url()}
    for eng in HTML_ENGINES:
        assert compile(plan_ir, backend=eng).extract(html, meta={"base_url": doc_url}) == {"u": expected}


# ---------------------------------------------------------------------------
# 2. Scrapling (Selector, regex, text cleaning, adaptive, find_similar)
# ---------------------------------------------------------------------------
def test_scrapling_css_xpath_regex_and_text_parity():
    scrapling = pytest.importorskip("scrapling")
    html = """
    <html><body>
      <ul class="menu">
        <li><a href="/home">Home</a><span class="price">$10.99</span></li>
        <li><a href="/about">About</a><span class="price">N/A</span></li>
      </ul>
      <div class="article">
        Hello <b>world</b><script>bad()</script><style>.x{}</style> from <i>Scrapling</i>.
      </div>
      <script id="payload">{"ok": true, "n": 7}</script>
    </body></html>
    """
    page = scrapling.Selector(html, url="https://example.com/dir/", adaptive=False)
    expected = {
        "first_link": page.css("ul.menu li a::text").get(),
        "all_texts": list(page.css("ul.menu li a::text").getall()),
        "all_hrefs": [page.urljoin(u) for u in page.css("ul.menu li a::attr(href)").getall()],
        "xp_texts": list(page.xpath("//ul/li/a/text()").getall()),
        "xp_count": int(float(page.xpath("count(//ul/li)").get())),
        "re_prices": list(page.css(".price").re(r"\$(\d+\.\d+)")),
        "re_first": page.css(".price").re_first(r"\$(\d+\.\d+)", default="0.00"),
        "article": str(page.css(".article").first.get_all_text(separator=" ", strip=True, ignore_tags=("script", "style"))),
        "payload": scrapling.Selector(page.css("#payload::text").get()).json(),
    }
    schema_dom = {
        "first_link": T.css("ul.menu li a").text(),
        "all_texts": T.css("ul.menu li a").map(T.text()),
        "all_hrefs": T.css("ul.menu li a").map(T.attr("href").url()),
        "xp_texts": T.xpath("//ul/li/a/text()"),
        "xp_count": T.xpath("count(//ul/li)"),
        "re_prices": T.css(".price").map(T.text().regex(r"\$(\d+\.\d+)", 1)).filter(T.exists()),
        "re_first": T.css(".price").text().regex(r"\$(\d+\.\d+)", 1).default("0.00"),
        "article": T.css(".article").text(" ", strip=True),
        "payload": T.css("#payload").text().parse_json(),
    }
    for eng in DOM_ENGINES:
        assert compile(schema_dom, backend=eng).extract(html, meta={"base_url": "https://example.com/dir/"}) == expected


def test_scrapling_adaptive_single_element_relocation_parity():
    scrapling = pytest.importorskip("scrapling")
    v1_html = (
        '<div id="container"><h1 class="heading">Main</h1>'
        '<button class="old-btn" data-role="cta">Submit Order</button></div>'
    )
    v2_html = (
        '<div id="container"><h1 class="heading">Main</h1>'
        '<button class="brand-new-btn" data-role="cta">Submit Order</button></div>'
    )
    s_args = {"storage_file": ":memory:", "url": "https://scrapling-parity-single.example"}
    s1 = scrapling.Selector(v1_html, url=s_args["url"], adaptive=True, storage_args=s_args)
    expected_v1 = str(s1.css("button.old-btn", auto_save=True).first.text)

    s2 = scrapling.Selector(v2_html, url=s_args["url"], adaptive=True, storage_args=s_args)
    expected_v2 = str(s2.css("button.old-btn", adaptive=True).first.text)
    assert expected_v1 == expected_v2 == "Submit Order"

    for eng in DOM_ENGINES:
        plan = compile({"btn": T.css("button.old-btn", adaptive="cta").text()}, backend=eng)
        assert plan.extract(v1_html) == {"btn": expected_v1}
        assert plan.extract(v2_html) == {"btn": expected_v2}


def test_scrapling_find_similar_list_parity():
    scrapling = pytest.importorskip("scrapling")
    v1_html = (
        '<div id="grid">'
        '<div class="product" data-kind="card"><h3>Item 1</h3></div>'
        '<div class="product" data-kind="card"><h3>Item 2</h3></div>'
        '<div class="product" data-kind="card"><h3>Item 3</h3></div>'
        "</div>"
    )
    v2_html = (
        '<div id="grid">'
        '<div class="item-card" data-kind="card"><h3>Item 1</h3></div>'
        '<div class="item-card" data-kind="card"><h3>Item 2</h3></div>'
        '<div class="item-card" data-kind="card"><h3>Item 3</h3></div>'
        "</div>"
    )
    s_v2 = scrapling.Selector(v2_html, adaptive=False)
    first = s_v2.css(".item-card").first
    expected = [str(first.css("h3::text").get()), *[str(x.css("h3::text").get()) for x in first.find_similar()]]
    assert expected == ["Item 1", "Item 2", "Item 3"]

    for eng in DOM_ENGINES:
        plan = compile({"titles": T.css(".product", adaptive="cards").map(T.css("h3").text())}, backend=eng)
        assert plan.extract(v1_html) == {"titles": expected}
        assert plan.extract(v2_html) == {"titles": expected}


# ---------------------------------------------------------------------------
# 3. Parsel / Scrapy (Selector, regex, XPath axes & regex, JMESPath)
# ---------------------------------------------------------------------------
def test_parsel_selector_css_xpath_re_parity():
    parsel = pytest.importorskip("parsel")
    html = """
    <div class="quotes">
      <div class="quote">
        <span class="text">Quote One</span>
        <small class="author">Alice</small>
        <a class="tag" href="/t/1">life</a><a class="tag" href="/t/2">code</a>
      </div>
      <div class="quote">
        <span class="text">Quote Two</span>
        <small class="author">Bob</small>
        <a class="tag" href="/t/3">rust</a>
      </div>
      <li class="next"><a href="/page/2/">Next</a></li>
    </div>
    <div class="item"><p><a href="http://example.com/item/123">Name 1: $10.99</a></p></div>
    <div class="item"><p><a href="http://example.com/item/456">Name 2: $25.50</a></p></div>
    """
    sel = parsel.Selector(text=html)
    expected = {
        "quotes": [
            {
                "quote": q.css(".text::text").get(),
                "author": q.css(".author::text").get(),
                "tags": q.css(".tag::text").getall(),
            }
            for q in sel.css("div.quote")
        ],
        "next": sel.css("li.next a::attr(href)").get(),
        "name_num": sel.css(".item a::text").re_first(r"Name (\d+)"),
        "prices": sel.xpath('//div[@class="item"]/p/a/text()').re(r"\$(\d+\.\d+)"),
    }
    schema = {
        "quotes": T.css("div.quote").map({
            "quote": T.css(".text").text(),
            "author": T.css(".author").text(),
            "tags": T.css(".tag").map(T.text()),
        }),
        "next": T.css("li.next a").attr("href"),
        "name_num": T.css(".item a").text().regex(r"Name (\d+)", 1),
        "prices": T.xpath('//div[@class="item"]/p/a/text()').map(T.regex(r"\$(\d+\.\d+)", 1)),
    }
    for eng in DOM_ENGINES:
        assert compile(schema, backend=eng).extract(html) == expected


def test_parsel_xpath_axes_and_regex_matches_parity():
    parsel = pytest.importorskip("parsel")
    html = """
    <div id="box">
      <ul class="list">
        <li data-n="10">Item 10</li>
        <li data-n="20" class="mid">Other</li>
        <li data-n="30">Item 25</li>
      </ul>
      <p>  Hello   Parsel  </p>
    </div>
    """
    sel = parsel.Selector(text=html)
    expected = {
        "next_sib": sel.xpath("//li[@class='mid']/following-sibling::li[1]/text()").get(),
        "prev_sib": sel.xpath("//li[@class='mid']/preceding-sibling::li[1]/text()").get(),
        "anc_id": sel.xpath("//li[@class='mid']/ancestor::div[1]/@id").get(),
        "norm": sel.xpath("normalize-space(//p)").get(),
        "total": float(sel.xpath("sum(//li/@data-n)").get()),
        "joined": sel.xpath("concat(//li[1]/text(), '|', //li[3]/text())").get(),
        "re_matched": sel.xpath("//li[re:test(text(), '^Item \\d+$')]/text()").getall(),
    }
    schema = {
        "next_sib": T.xpath("//li[@class='mid']/following-sibling::li[1]/text()").first(),
        "prev_sib": T.xpath("//li[@class='mid']/preceding-sibling::li[1]/text()").first(),
        "anc_id": T.xpath("//li[@class='mid']/ancestor::div[1]/@id").first(),
        "norm": T.xpath("normalize-space(//p)"),
        "total": T.xpath("sum(//li/@data-n)"),
        "joined": T.xpath("concat(//li[1]/text(), '|', //li[3]/text())"),
        "re_matched": T.xpath("//li[matches(text(), '^Item \\d+$')]/text()"),
    }
    for eng in DOM_ENGINES:
        assert compile(schema, backend=eng).extract(html) == expected


def test_parsel_json_jmespath_parity():
    parsel = pytest.importorskip("parsel")
    payload = {"store": {"books": [{"title": "Dune", "price": 15}, {"title": "Solaris", "price": 12}]}}
    raw = json.dumps(payload)
    sel = parsel.Selector(text=raw)
    expected = {"titles": sel.jmespath("store.books[*].title").getall()}
    schema = {"titles": T["store"]["books"].map(T["title"])}
    assert compile(schema, backend="json").extract(raw) == expected


# ---------------------------------------------------------------------------
# 4. Selectolax (LexborHTMLParser)
# ---------------------------------------------------------------------------
def test_selectolax_lexbor_css_attributes_and_scope_parity():
    sl = pytest.importorskip("selectolax.lexbor")
    html = """
    <html><body>
      <div class="card">
        <span class="title">Direct</span>
        <div class="nested"><span class="title">Nested</span></div>
        <button disabled data-sku="A-100">Buy &amp; Save</button>
      </div>
      <ul>
        <li class="row muted">Skip</li>
        <li class="row">Second</li>
        <li class="row">Third</li>
      </ul>
    </body></html>
    """
    tree = sl.LexborHTMLParser(html)
    card = tree.css_first(".card")
    direct_titles = [
        n.text(strip=True)
        for n in card.iter(include_text=False)
        if n.tag == "span" and "title" in (n.attributes.get("class") or "").split()
    ]
    btn = tree.css_first('button[data-sku^="A-"]')
    expected = {
        "direct": direct_titles,
        "btn_text": btn.text(strip=True),
        "disabled": "disabled" in btn.attributes,
        "rows": [n.text(strip=True) for n in tree.css("ul > li:not(.muted):nth-child(2n+1)")],
    }
    schema = {
        "direct": T.css(".card").map(T.css(":scope > .title").text().strip()),
        "btn_text": T.css('button[data-sku^="A-"]').text().strip(),
        "disabled": T.css('button[data-sku^="A-"]').has_attr("disabled"),
        "rows": T.css("ul > li:not(.muted):nth-child(2n+1)").map(T.text().strip()),
    }
    # `:scope` works in stream and lite (scraper refuses `:scope`)
    for eng in [e for e in ("stream", "lite") if e in AVAILABLE]:
        assert compile(schema, backend=eng).extract(html) == expected


def test_selectolax_strip_tags_vs_without_parity():
    sl = pytest.importorskip("selectolax.lexbor")
    html = (
        '<article><h1>News &mdash; Today</h1>'
        '<div class="p">Keep <b>this</b><span class="ad">BUY</span><i class="promo">OFF</i>'
        "<script>x=1</script><style>.a{}</style> text.</div>"
        "</article>"
    )
    tree = sl.LexborHTMLParser(html)
    for bad in tree.css("script, style, .ad, .promo"):
        bad.decompose()
    expected = {
        "title": tree.css_first("h1").text(strip=True),
        "body": tree.css_first(".p").text(separator=" ", strip=True),
    }
    schema = {
        "title": T.css("h1").text().strip(),
        "body": T.css(".p").text(" ", strip=True, without=".ad, .promo"),
    }
    for eng in HTML_ENGINES:
        assert compile(schema, backend=eng).extract(html) == expected


# ---------------------------------------------------------------------------
# 5. BeautifulSoup4 + SoupSieve
# ---------------------------------------------------------------------------
def test_bs4_soupsieve_has_contains_and_hybrid_json_parity():
    bs4 = pytest.importorskip("bs4")
    html = """
    <html><body>
      <article class="product" data-id="101">
        <h2 class="name">Mechanical keyboard</h2>
        <span class="price">129.90</span>
        <a class="detail featured" href="/products/101">Special Offer</a>
      </article>
      <article class="product" data-id="102">
        <h2 class="name">Wireless mouse</h2>
        <span class="price">39.50</span>
        <a class="detail" href="/products/102">Details</a>
      </article>
      <div class="note">One <span class="ad">AD</span> Two</div>
      <script id="__NEXT_DATA__" type="application/json">
        {"props":{"pageProps":{"category":"Accessories","total":2}}}
      </script>
    </body></html>
    """
    soup = bs4.BeautifulSoup(html, "html.parser")
    for ad in soup.select(".note .ad"):
        ad.decompose()
    expected = {
        "featured_names": [el.select_one(".name").get_text(strip=True) for el in soup.select("article:has(> a.featured)")],
        "special_links": [el.get("href") for el in soup.select('a:-soup-contains("Special Offer")')],
        "note": soup.select_one(".note").get_text(" ", strip=True),
        "category": json.loads(soup.select_one("script#__NEXT_DATA__").string)["props"]["pageProps"]["category"],
    }
    schema = {
        "featured_names": T.css("article:has(> a.featured)").map(T.css(".name").text().strip()),
        "special_links": T.css('a:contains("Special Offer")').map(T.attr("href")),
        "note": T.css(".note").text(" ", strip=True, without=".ad"),
        "category": T.css("script#__NEXT_DATA__").text().parse_json()["props"]["pageProps"]["category"],
    }
    # `:contains` is supported by `lite`
    if "lite" in AVAILABLE:
        assert compile(schema, backend="lite").extract(html) == expected
    # Without `:contains`, `scraper` agrees on `:has`, `without`, and `parse_json`
    schema_no_contains = {k: v for k, v in schema.items() if k != "special_links"}
    expected_no_contains = {k: v for k, v in expected.items() if k != "special_links"}
    for eng in DOM_ENGINES:
        assert compile(schema_no_contains, backend=eng).extract(html) == expected_no_contains


# ---------------------------------------------------------------------------
# 6. Crawl4AI official nested E-commerce catalog (CSS & XPath)
# ---------------------------------------------------------------------------
CRAWL4AI_CATALOG_HTML = """
<div class="category">
  <h2 class="category-name">Electronics</h2>
  <div class="product">
    <h3 class="product-name">Smartphone X</h3>
    <p class="product-price">$999</p>
    <div class="product-details"><span class="brand">TechCorp</span><span class="model">X-2000</span></div>
    <ul class="product-features"><li>5G capable</li><li>6.5&quot; OLED screen</li></ul>
    <div class="product-reviews">
      <div class="review"><span class="reviewer">John D.</span><span class="rating">4.5</span><p class="review-text">Great phone!</p></div>
    </div>
    <ul class="related-products">
      <li><span class="related-name">Phone Case</span><span class="related-price">$29.99</span></li>
    </ul>
  </div>
</div>
<div class="category">
  <h2 class="category-name">Books</h2>
  <div class="product">
    <h3 class="product-name">Rust Guide</h3>
    <p class="product-price">$49</p>
    <div class="product-details"><span class="brand">O'Reilly</span><span class="model">2ed</span></div>
    <ul class="product-features"><li>Paperback</li></ul>
    <div class="product-reviews"></div>
    <ul class="related-products"></ul>
  </div>
</div>
"""


@pytest.mark.filterwarnings("ignore:Truth-testing of elements:FutureWarning")
@pytest.mark.parametrize("kind", ["css", "xpath"])
def test_crawl4ai_official_nested_ecommerce_catalog_parity(kind):
    strategies = _comp.get_crawl4ai_strategies()
    if strategies is None:
        pytest.skip("crawl4ai strategy classes not available")
    JsonCss, JsonXPath = strategies

    def sel(tag, cls=None, base=False):
        if kind == "css":
            return f"{tag}.{cls}" if cls else tag
        prefix = "//" if base else ".//"
        return f"{prefix}{tag}[@class='{cls}']" if cls else prefix + tag

    strategy = {
        "name": "E-commerce Product Catalog",
        "baseSelector": sel("div", "category", base=True),
        "fields": [
            {"name": "category_name", "type": "text", "selector": sel("h2", "category-name")},
            {
                "name": "products",
                "type": "nested_list",
                "selector": sel("div", "product"),
                "fields": [
                    {"name": "name", "type": "text", "selector": sel("h3", "product-name")},
                    {"name": "price", "type": "text", "selector": sel("p", "product-price")},
                    {
                        "name": "details",
                        "type": "nested",
                        "selector": sel("div", "product-details"),
                        "fields": [
                            {"name": "brand", "type": "text", "selector": sel("span", "brand")},
                            {"name": "model", "type": "text", "selector": sel("span", "model")},
                        ],
                    },
                    {
                        "name": "features",
                        "type": "list",
                        "selector": "ul.product-features li" if kind == "css" else ".//ul[@class='product-features']//li",
                        "fields": [{"name": "feature", "type": "text"}],
                    },
                    {
                        "name": "reviews",
                        "type": "nested_list",
                        "selector": sel("div", "review"),
                        "fields": [
                            {"name": "reviewer", "type": "text", "selector": sel("span", "reviewer")},
                            {"name": "rating", "type": "text", "selector": sel("span", "rating")},
                            {"name": "comment", "type": "text", "selector": sel("p", "review-text")},
                        ],
                    },
                    {
                        "name": "related_products",
                        "type": "list",
                        "selector": "ul.related-products li" if kind == "css" else ".//ul[@class='related-products']//li",
                        "fields": [
                            {"name": "name", "type": "text", "selector": sel("span", "related-name")},
                            {"name": "price", "type": "text", "selector": sel("span", "related-price")},
                        ],
                    },
                ],
            },
        ],
    }
    extractor = (JsonCss if kind == "css" else JsonXPath)(strategy)
    expected = extractor.extract("https://example.test/catalog", CRAWL4AI_CATALOG_HTML)
    ir = migrate.from_crawl4ai(strategy)
    for eng in DOM_ENGINES:
        assert Plan.from_ir(ir, backend=eng).extract(CRAWL4AI_CATALOG_HTML) == expected


# ---------------------------------------------------------------------------
# 7. Markdown (markdownify, html2text)
# ---------------------------------------------------------------------------
def test_markdown_competitors_preserve_all_headings_links_and_lists():
    markdownify = pytest.importorskip("markdownify")
    html2text = pytest.importorskip("html2text")
    html = """
    <article>
      <h1>Main Guide</h1>
      <p>Read the <a href="https://example.org/docs">documentation</a> and run <code>cargo test</code>.</p>
      <h2>Checklist</h2>
      <ul><li>First item</li><li>Second item</li></ul>
      <table>
        <thead><tr><th>Engine</th><th>Mode</th></tr></thead>
        <tbody><tr><td>stream</td><td>no-dom</td></tr></tbody>
      </table>
    </article>
    """
    md_lite = markdown(html, "article", backend="lite")
    md_scraper = markdown(html, "article", backend="scraper")
    assert md_lite == md_scraper

    md_markdownify = markdownify.markdownify(html)
    h2t = html2text.HTML2Text()
    h2t.body_width = 0
    md_html2text = h2t.handle(html)

    must_contain = [
        "Main Guide",
        "https://example.org/docs",
        "cargo test",
        "Checklist",
        "First item",
        "Second item",
        "Engine",
        "stream",
        "no-dom",
    ]
    for output in (md_lite, md_markdownify, md_html2text):
        for token in must_contain:
            assert token in output


# ---------------------------------------------------------------------------
# 8. Extended CSS Level 4, XPath substring/lang, and zero-copy bytes/bytearray
# ---------------------------------------------------------------------------
def test_css4_and_state_selectors_parity_across_engines():
    bs4 = pytest.importorskip("bs4")
    html = """
    <html><head><link href="/style.css"></head><body>
      <div id="wrap">
        <p class="Lead" lang="en-US">One</p>
        <span>Mid</span>
        <p class="body">Two</p>
        <p class="tail">Three</p>
      </div>
      <form>
        <fieldset disabled>
          <legend><input id="i1" type="checkbox" checked></legend>
          <legend><input id="i2" type="radio" checked></legend>
          <button id="b1">Send</button>
        </fieldset>
        <fieldset>
          <input id="i3" type="text">
          <select>
            <optgroup disabled><option id="o1" selected>A</option></optgroup>
            <option id="o2">B</option>
          </select>
        </fieldset>
      </form>
      <a id="a1" href="/home">Home</a><a id="a2">NoHref</a>
    </body></html>
    """
    soup = bs4.BeautifulSoup(html, "html.parser")

    shared_selectors = [
        (":is(#wrap > p.body, #wrap > span)", None),
        (":where(p.Lead, p.tail)", None),
        ("#wrap > p:first-of-type", None),
        ("#wrap > p:nth-of-type(2)", None),
        ("#wrap > p:not(#wrap p.Lead)", None),
        ('p[class="Lead" s]', None),
        ('p[class="lead" s]', None),
    ]
    dom_selectors = [
        "#wrap > p:last-of-type",
        "#wrap > p:nth-last-of-type(2)",
        "#wrap > span:only-of-type",
        "#wrap > :nth-last-child(2)",
    ]
    stream_and_lite_state_selectors = [
        (":any-link", 2),
        (":checked", 3),
        ("input:disabled, button:disabled, option:disabled, fieldset:disabled", 4),
        ("input:enabled, option:enabled", 3),
    ]

    for sel, _ in shared_selectors:
        want = [el.get_text() for el in soup.select(sel)]
        for eng in HTML_ENGINES:
            assert compile({"v": T.css(sel).map(T.text())}, backend=eng).extract(html)["v"] == want, (eng, sel)

    for sel in dom_selectors:
        want = [el.get_text() for el in soup.select(sel)]
        for eng in DOM_ENGINES:
            assert compile({"v": T.css(sel).map(T.text())}, backend=eng).extract(html)["v"] == want, (eng, sel)

    for sel, want_count in stream_and_lite_state_selectors:
        for eng in [e for e in ("lite", "stream", "auto") if e in AVAILABLE or e == "auto"]:
            assert compile({"n": T.css(sel).count()}, backend=eng).extract(html)["n"] == want_count, (eng, sel)


def test_xpath_substring_and_lang_parity_and_zero_copy_bytes():
    etree = pytest.importorskip("lxml.etree")
    html_str = (
        '<html lang="en-US"><body>'
        '<div id="main"><p class="lead">Hello &mdash; World</p>'
        '<span xml:lang="de-AT">Servus</span></div>'
        "</body></html>"
    )
    html_bytes = html_str.encode("utf-8")
    html_bytearray = bytearray(html_bytes)

    schema = {
        "title": T.css("p.lead").text(),
        "lang_en": T.xpath("boolean(//p[lang('en')])"),
        "lang_de": T.xpath("boolean(//span[lang('de')])"),
        "sub": T.xpath("substring('abcdef', 1.5, 2.6)"),
    }
    tree = etree.HTML(html_str)
    for eng in DOM_ENGINES:
        p = compile(schema, backend=eng)
        r_str = p.extract(html_str)
        r_bytes = p.extract(html_bytes)
        r_ba = p.extract(html_bytearray)
        assert r_str == r_bytes == r_ba
        assert r_str["title"] == "Hello \u2014 World"
        assert r_str["lang_en"] is True
        assert r_str["lang_de"] is True
        assert r_str["sub"] == etree.XPath("substring('abcdef', 1.5, 2.6)")(tree)

    if "stream" in AVAILABLE:
        sp = compile({"title": T.css("p.lead").text()}, backend="stream")
        assert sp.extract(html_str) == sp.extract(html_bytes) == sp.extract(html_bytearray)
        assert sp.extract_many([html_str, html_bytes, html_bytearray]) == [{"title": "Hello \u2014 World"}] * 3
