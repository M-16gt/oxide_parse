"""Synthetic, deterministic HTML & JSON document generators for all benchmark scenarios."""
from __future__ import annotations

import json


def make_catalog_page(n_products: int, noise: bool = False) -> str:
    """Scenario 01: E-commerce catalog with optional heavy DOM noise between cards."""
    junk = (
        '<div class="promo-banner" data-track="imp-99">'
        '<div class="promo-inner"><span>Sponsored</span><b>Sale</b><i>Hot</i>'
        '<ul class="breadcrumbs"><li><a href="/a">1</a></li><li><a href="/b">2</a></li>'
        '<li><a href="/c">3</a></li><li><a href="/d">4</a></li></ul>'
        '<div class="svg-wrap"><svg viewBox="0 0 10 10"><path d="M0 0L10 10"/><circle cx="5" cy="5" r="3"/></svg></div>'
        '</div></div>'
        if noise
        else ""
    )
    rows = "".join(
        f'<div class="product" data-id="{i}" data-avail="{"true" if i % 3 else "false"}">'
        f'<h2 class="name">Product {i}</h2>'
        f'<span class="price">${i % 500}.99</span>'
        f'<a class="link" href="/p/{i}">more</a>'
        f'<p class="desc">Some description text for product {i}</p>'
        f'<button class="buy" {"disabled" if i % 5 == 0 else ""}>Buy</button>'
        f'</div>' + junk
        for i in range(n_products)
    )
    return f"<!DOCTYPE html><html><head><title>Catalog</title></head><body><h1>Catalog</h1><div id=list>{rows}</div></body></html>"


def make_early_exit_page(n_tail_products: int = 8000) -> str:
    """Scenario 02: ~2.5 MiB page where target fields are in <head> and the very first card."""
    head = (
        "<!DOCTYPE html><html><head>"
        "<title>Flagship Smartphone X200 — Official Store</title>"
        '<meta property="og:title" content="Flagship Smartphone X200">'
        '<meta property="og:price:amount" content="899.00">'
        '<link rel="canonical" href="https://store.example/x200">'
        "</head><body>"
        '<header class="site-header"><h1 class="hero-title">Flagship Smartphone X200</h1></header>'
        '<div class="first-offer" data-sku="SKU-FIRST-001"><span class="offer-price">$899.00</span></div>'
    )
    tail = "".join(
        f'<div class="related-item" data-idx="{i}"><h3>Related {i}</h3>'
        f'<p>Long irrelevant review and specification text for item {i} with extra spans '
        f'<span>tag1</span><span>tag2</span><span>tag3</span></p></div>'
        for i in range(n_tail_products)
    )
    return head + f"<section class='related'>{tail}</section></body></html>"


def make_deep_combinator_page(n_blocks: int = 400, depth: int = 25) -> str:
    """Scenario 03: Deeply nested DOM tree with attribute & structural pseudo-class selectors."""
    open_wraps = "".join(f'<div class="lvl-{d}" data-depth="{d}">' for d in range(depth))
    close_wraps = "</div>" * depth
    items = []
    for i in range(n_blocks):
        items.append(
            f'<section class="card-wrap" data-group="{i % 4}">'
            f'<ul class="spec-list">'
            f'<li class="spec-item muted"><a data-kind="note-{i}" href="/n/{i}">Note {i}</a></li>'
            f'<li class="spec-item"><a data-kind="primary-{i}" href="/p/{i}">Primary {i}</a><span class="val">{i * 10}</span></li>'
            f'<li class="spec-item"><a data-kind="extra-{i}" href="/e/{i}">Extra {i}</a><span class="val">{i * 20}</span></li>'
            f'</ul></section>'
        )
    return f"<html><body>{open_wraps}<main id='app'>{''.join(items)}</main>{close_wraps}</body></html>"


def make_xpath_page(n_rows: int = 600) -> str:
    """Scenario 04: Table & sibling-heavy markup tailored for XPath 1.0 evaluation."""
    rows = []
    for i in range(n_rows):
        cat = "premium" if i % 4 == 0 else "standard"
        rows.append(
            f'<div class="row-item {cat}" data-id="{i}">'
            f'<span class="label">Weight</span><span class="value">{100 + (i % 50)}g</span>'
            f'<b class="title">Item #{i}</b>'
            f'<i class="cost">${(i % 300) + 10}</i>'
            f'<a class="action" href="/buy/{i}">Order</a>'
            f'</div>'
        )
    return f"<html><body><div id='table'>{''.join(rows)}</div></body></html>"


def make_hybrid_nextjs_page(n_products: int = 1000) -> str:
    """Scenario 05: SSR HTML page with a large __NEXT_DATA__ JSON script payload."""
    html_cards = "".join(
        f'<div class="ssr-card"><h2>SSR Item {i}</h2><p>Placeholder text for SEO {i}</p></div>'
        for i in range(n_products // 2)
    )
    payload = {
        "props": {
            "pageProps": {
                "category": "Electronics",
                "total": n_products,
                "products": [
                    {
                        "id": i,
                        "sku": f"NX-{i:05d}",
                        "name": f"NextJS Product {i}",
                        "price": round(19.99 + (i % 200) * 1.5, 2),
                        "inStock": (i % 3 != 0),
                    }
                    for i in range(n_products)
                ],
            }
        }
    }
    json_str = json.dumps(payload, separators=(",", ":"))
    return (
        f"<!DOCTYPE html><html><head><title>Next Store</title></head><body>"
        f"<h1 class='page-heading'>Electronics Catalog</h1>"
        f"<div id='ssr-grid'>{html_cards}</div>"
        f'<script id="__NEXT_DATA__" type="application/json">{json_str}</script>'
        f"</body></html>"
    )


def make_dirty_article_page(n_paragraphs: int = 600) -> str:
    """Scenario 06: Article with inline <script>/<style>, .ad blocks, <br> tags, and HTML entities."""
    paras = []
    for i in range(n_paragraphs):
        paras.append(
            f'<div class="para-block">'
            f'<p>Paragraph {i}: &laquo;Smart&nbsp;Tech &amp; Co.&raquo; &mdash; price is 19,99&nbsp;&euro; '
            f'<br>Second line of paragraph {i} with &#x27;hex&#x27; and &#169; entities.</p>'
            f'<div class="ad">ADVERTISEMENT BANNER #{i} DO NOT INCLUDE</div>'
            f'<script>window.__trk = "ignore_script_{i}";</script>'
            f'<style>.ad-{i} {{ display:none; }}</style>'
            f'</div>'
        )
    return f"<html><body><article id='main-article'><h1>Deep Review</h1>{''.join(paras)}</article></body></html>"


def make_adaptive_pages(n_noise_nodes: int = 400) -> tuple[str, str]:
    """Scenario 07: (v1_original_html, v2_redesigned_html) for adaptive selector benchmarking."""
    filler_before = "".join(
        f'<div class="filler-box" data-idx="{i}"><span>Filler {i}</span><p> Noise text {i} </p></div>'
        for i in range(n_noise_nodes // 2)
    )
    filler_after = "".join(
        f'<section class="widget" data-w="{i}"><h4>Widget {i}</h4><p>More text {i}</p></section>'
        for i in range(n_noise_nodes // 2)
    )
    v1 = (
        f"<html><body>{filler_before}"
        f'<section class="product-stage">'
        f'<article class="product-card" id="prod-main" data-sku="A100">'
        f'<h3 class="prod-title" data-role="heading">Ergonomic Desk Lamp Pro</h3>'
        f'<p class="prod-description" data-role="summary">Warm LED desk lamp with stepless dimming and USB-C port.</p>'
        f'<span class="prod-price" data-currency="USD">$49.90</span>'
        f'</article></section>'
        f"{filler_after}</body></html>"
    )
    # In V2: IDs and original classes (#prod-main, .prod-title, .prod-description, .prod-price) are renamed
    v2 = (
        f"<html><body>{filler_before}"
        f'<div class="v2-layout-shell">'
        f'<article class="item-surface redesigned" data-sku="A100">'
        f'<div class="item-header-group">'
        f'<h3 class="heading-xl" data-role="heading">Ergonomic Desk Lamp Pro</h3>'
        f'<p class="body-copy-muted" data-role="summary">Warm LED desk lamp with stepless dimming and USB-C port.</p>'
        f'<span class="price-badge-new" data-currency="USD">$49.90</span>'
        f'</div></article></div>'
        f"{filler_after}</body></html>"
    )
    return v1, v2


def make_localized_numbers_page(n_items: int = 2000) -> tuple[str, str]:
    """Scenario 09: Plain ASCII numbers vs localized European/Russian numbers."""
    ascii_rows = "".join(
        f'<div class="it" data-id="{i}"><span class="p">${(i % 900) + 10}.99</span></div>'
        for i in range(n_items)
    )
    local_rows = "".join(
        f'<div class="it" data-id="{i}"><span class="p">{(i % 90) + 1}\u00a0299,50 \u20bd</span></div>'
        for i in range(n_items)
    )
    return f"<html><body>{ascii_rows}</body></html>", f"<html><body>{local_rows}</body></html>"


def make_scrapling_nested_page(n_items: int = 5000, depth: int = 16) -> str:
    """Scenario 11a: Scrapling-style 5,000 elements under nested containers."""
    open_wrap = "".join(f'<div class="wrap-{d}">' for d in range(depth))
    close_wrap = "</div>" * depth
    items = "".join(
        f'<div class="item" data-idx="{i}"><span class="text">Item #{i} payload</span></div>'
        for i in range(n_items)
    )
    return f"<!DOCTYPE html><html><body>{open_wrap}<section id='items'>{items}</section>{close_wrap}</body></html>"


def make_scrapling_similar_pages(n_cards: int = 500) -> tuple[str, str]:
    """Scenario 11b: (v1_html, v2_redesigned_html) for Scrapling find_similar() vs adaptive list."""
    v1_cards = "".join(
        f'<div class="product" data-kind="card"><h3>Product {i}</h3><span class="p">${10 + (i % 90)}.99</span></div>'
        for i in range(n_cards)
    )
    v2_cards = "".join(
        f'<div class="item-card" data-kind="card"><h3>Product {i}</h3><span class="p">${10 + (i % 90)}.99</span></div>'
        for i in range(n_cards)
    )
    noise = "".join(f'<aside class="widget" data-w="{i}"><span>Ad {i}</span></aside>' for i in range(100))
    return (
        f"<html><body>{noise}<div id='grid'>{v1_cards}</div></body></html>",
        f"<html><body>{noise}<div id='grid'>{v2_cards}</div></body></html>",
    )


def make_selectolax_text_page(n_sections: int = 400) -> str:
    """Scenario 12: Selectolax-style page with scripts, styles, paragraphs, and links."""
    sections = "".join(
        f'<section class="sec" data-s="{i}">'
        f'<p>Section {i} body text with details.</p>'
        f'<script>window.__s{i}={i};</script>'
        f'<style>.sec-{i}{{margin:0;}}</style>'
        f'<a href="/a/{i}">Link A {i}</a><a href="/b/{i}">Link B {i}</a>'
        f"</section>"
        for i in range(n_sections)
    )
    return f"<!DOCTYPE html><html><head><title>Selectolax Benchmark Page</title></head><body>{sections}</body></html>"


def make_frostwork_offers_page(n_offers: int = 2000) -> str:
    """Scenario 13a: Frostwork grouped Page() extraction with flat header fields + 2000 offers."""
    offers = "".join(
        f'<div class="offer" data-sku="SKU-{i:04d}">'
        f'<h2>Offer #{i}</h2>'
        f'<span class="price">${10 + (i % 100)}.00</span>'
        f'<a href="/offer/{i}">View</a>'
        f"</div>"
        f'<div class="spacer"><span>promo</span></div>'
        for i in range(n_offers)
    )
    return (
        "<!DOCTYPE html><html><head>"
        '<title>Offers Catalog</title>'
        '<meta name="description" content="Best daily offers">'
        '<link rel="canonical" href="https://shop.example/offers">'
        "</head><body>"
        "<h1>Offers Catalog</h1>"
        f"<main>{offers}</main>"
        "</body></html>"
    )


def make_parsel_quotes_page(n_quotes: int = 500) -> str:
    """Scenario 14: Canonical Scrapy / Parsel quotes.toscrape.com benchmark page."""
    quotes = "".join(
        f'<div class="quote">'
        f'<span class="text">Quote number {i} for benchmark</span>'
        f'<small class="author">Author {i % 25}</small>'
        f'<a class="author-link" href="/author/{i % 25}">about</a>'
        f'<div class="tags">'
        f'<a class="tag" href="/tag/life">life</a>'
        f'<a class="tag" href="/tag/code">code</a>'
        f'<a class="tag" href="/tag/rust">rust</a>'
        f"</div></div>"
        for i in range(n_quotes)
    )
    return (
        "<!DOCTYPE html><html><body>"
        f"<div id='quotes'>{quotes}</div>"
        '<ul class="pager"><li class="next"><a href="/page/2/">Next</a></li></ul>'
        "</body></html>"
    )


def make_crawl4ai_nested_catalog_page(n_categories: int = 30, n_products: int = 4) -> str:
    """Scenario 15: Official Crawl4AI nested e-commerce catalog structure scaled up."""
    cats = []
    for c in range(n_categories):
        prods = []
        for p in range(n_products):
            idx = c * n_products + p
            prods.append(
                f'<div class="product">'
                f'<h3 class="product-name">Product {idx}</h3>'
                f'<p class="product-price">${50 + (idx % 500)}</p>'
                f'<div class="product-details"><span class="brand">Brand {idx % 10}</span><span class="model">M-{idx}</span></div>'
                f'<ul class="product-features"><li>5G</li><li>OLED</li><li>128GB</li></ul>'
                f'<div class="product-reviews">'
                f'<div class="review"><span class="reviewer">User A</span><span class="rating">5</span><p class="review-text">Great {idx}</p></div>'
                f'<div class="review"><span class="reviewer">User B</span><span class="rating">4</span><p class="review-text">Solid {idx}</p></div>'
                f"</div>"
                f'<ul class="related-products">'
                f'<li><span class="related-name">Case {idx}</span><span class="related-price">$19.99</span></li>'
                f'<li><span class="related-name">Cable {idx}</span><span class="related-price">$9.99</span></li>'
                f"</ul></div>"
            )
        cats.append(f'<div class="category"><h2 class="category-name">Category {c}</h2>{"".join(prods)}</div>')
    return f"<!DOCTYPE html><html><body><div id='catalog'>{''.join(cats)}</div></body></html>"
