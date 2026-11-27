"""Implementation of the 9 benchmark scenarios for oxide_parse v7.2."""
from __future__ import annotations

import gc
import json
import logging
import random
import statistics
import time
from typing import Any, Callable, Dict, List, Optional, Tuple

from competitors import get_crawl4ai_strategies, has_lib, record_skip
import generators as gen
from oxide_parse import Plan, T, compile, engines, migrate


def measure_interleaved(
    cases: Dict[str, Callable[[], Any]],
    repeat: int = 7,
    seed: int = 20261110,
    validator: Optional[Callable[[str, Any], None]] = None,
    prepare: Optional[Dict[str, Callable[[], None]]] = None,
) -> Dict[str, Dict[str, Any]]:
    """Run all implementations in randomized order per round with gc.collect() outside the timer."""
    if not cases:
        return {}
    rng = random.Random(seed)
    names = list(cases.keys())
    samples: Dict[str, List[float]] = {k: [] for k in names}
    expected = None
    have_expected = False

    def validate(name, out):
        nonlocal expected, have_expected
        if validator is not None:
            validator(name, out)
        elif not have_expected:
            expected = out
            have_expected = True
        elif out != expected:
            raise ValueError(f"Result mismatch for {name}")

    # Warm-up + validation
    for name in names:
        if prepare and name in prepare:
            prepare[name]()
        gc.collect()
        out = cases[name]()
        validate(name, out)
        del out

    for r in range(repeat):
        order = list(names)
        rng.shuffle(order)
        for name in order:
            if prepare and name in prepare:
                prepare[name]()
            gc.collect()
            t0 = time.perf_counter()
            out = cases[name]()
            dt = time.perf_counter() - t0
            samples[name].append(dt * 1000.0)  # milliseconds
            validate(name, out)
            del out

    summary: Dict[str, Dict[str, Any]] = {}
    for name, ts in samples.items():
        s = sorted(ts)
        q1 = s[len(s) // 4]
        q3 = s[(len(s) * 3) // 4]
        summary[name] = {
            "min_ms": round(s[0], 4),
            "median_ms": round(statistics.median(s), 4),
            "mean_ms": round(statistics.mean(s), 4),
            "iqr_ms": round(q3 - q1, 4),
            "max_ms": round(s[-1], 4),
            "samples_ms": [round(x, 4) for x in ts],
            "repeat": repeat,
            "validated": True,
        }
    return summary


def available_oxide_engines(kinds: Tuple[str, ...] = ("dom", "stream")) -> List[str]:
    return [e.name for e in engines() if e.kind in kinds]


def _get_scrapling_cls():
    """Find the installed Scrapling selector class."""
    import scrapling
    if hasattr(scrapling, "Selector"):
        return scrapling.Selector
    if hasattr(scrapling, "Adaptor"):
        return scrapling.Adaptor
    from scrapling.parser import Selector
    return Selector


def _scrapling_first(page, selector, **kwargs):
    return page.css(selector, **kwargs).first


# =============================================================================================
# SCENARIO 01: E-commerce Catalog Scaling (with & without noise)
# =============================================================================================
def run_01_catalog_scaling(sizes: List[int], repeat: int) -> List[Dict[str, Any]]:
    results = []
    schema = {
        "title": T.css("h1").text(),
        "items": T.css(".product").map({
            "id": T.attr("data-id").int(),
            "name": T.css(".name").text(),
            "price": T.css(".price").text().float(),
            "url": T.css(".link").attr("href"),
            "desc": T.css(".desc").text(),
        }),
    }

    for n in sizes:
        for noise in (False, True):
            html = gen.make_catalog_page(n, noise=noise)
            cases: Dict[str, Callable[[], Any]] = {}
            native_breakdown: Dict[str, Dict[str, float]] = {}

            for eng in available_oxide_engines(("dom", "stream")):
                plan = compile(schema, backend=eng)
                cases[f"oxide_parse [{eng}]"] = (lambda p=plan, h=html: p.extract(h))

            if has_lib("selectolax"):
                from selectolax.lexbor import LexborHTMLParser
                try:
                    from selectolax.parser import HTMLParser as ModestHTMLParser
                except ImportError as exc:
                    ModestHTMLParser = None
                    record_skip("selectolax [modest]", exc)

                def slx_lexbor(h=html):
                    t = LexborHTMLParser(h)
                    return {
                        "title": t.css_first("h1").text(),
                        "items": [
                            {
                                "id": int(p.attributes["data-id"]),
                                "name": p.css_first(".name").text(),
                                "price": float(p.css_first(".price").text().lstrip("$")),
                                "url": p.css_first(".link").attributes["href"],
                                "desc": p.css_first(".desc").text(),
                            }
                            for p in t.css(".product")
                        ],
                    }

                def slx_modest(h=html):
                    t = ModestHTMLParser(h)
                    return {
                        "title": t.css_first("h1").text(),
                        "items": [
                            {
                                "id": int(p.attributes["data-id"]),
                                "name": p.css_first(".name").text(),
                                "price": float(p.css_first(".price").text().lstrip("$")),
                                "url": p.css_first(".link").attributes["href"],
                                "desc": p.css_first(".desc").text(),
                            }
                            for p in t.css(".product")
                        ],
                    }

                cases["selectolax [lexbor]"] = slx_lexbor
                if ModestHTMLParser is not None:
                    cases["selectolax [modest]"] = slx_modest

            if has_lib("frostwork"):
                try:
                    import frostwork as fw
                    if hasattr(fw, "Page"):
                        page = fw.Page().field("title", "h1::text").many("items", ".product", {
                            "id": "::attr(data-id)", "name": ".name::text", "price": ".price::text",
                            "url": ".link::attr(href)", "desc": ".desc::text",
                        })

                        def fw_run(h=html, page=page):
                            result = page.extract(h).to_dict()
                            for item in result["items"]:
                                item["id"] = int(item["id"])
                                item["price"] = float(item["price"].lstrip("$"))
                            return result
                        sample = fw_run()
                        expected_items = [
                            {"id": i, "name": f"Product {i}", "price": float(f"{i % 500}.99"),
                             "url": f"/p/{i}", "desc": f"Some description text for product {i}"}
                            for i in range(n)
                        ]
                        if sample != {"title": "Catalog", "items": expected_items}:
                            raise ValueError("Installed frostwork adapter returned incompatible data")
                        cases["frostwork"] = fw_run
                    else:
                        record_skip("01_catalog: frostwork", "Installed API has no Page class")
                except Exception as exc:
                    record_skip("01_catalog: frostwork", exc)

            if has_lib("lxml"):
                from lxml import html as lhtml

                def lxml_xpath_run(h=html):
                    tree = lhtml.fromstring(h)
                    return {
                        "title": tree.xpath("string(//h1)"),
                        "items": [
                            {
                                "id": int(p.attrib["data-id"]),
                                "name": p.findtext(".//*[@class='name']"),
                                "price": float(p.findtext(".//*[@class='price']").lstrip("$")),
                                "url": p.find(".//*[@class='link']").attrib["href"],
                                "desc": p.findtext(".//*[@class='desc']"),
                            }
                            for p in tree.xpath("//div[@class='product']")
                        ],
                    }

                cases["lxml [direct]"] = lxml_xpath_run

            if has_lib("parsel"):
                from parsel import Selector

                def parsel_run(h=html):
                    s = Selector(text=h)
                    return {
                        "title": s.css("h1::text").get(),
                        "items": [
                            {
                                "id": int(p.attrib["data-id"]),
                                "name": p.css(".name::text").get(),
                                "price": float(p.css(".price::text").get().lstrip("$")),
                                "url": p.css(".link::attr(href)").get(),
                                "desc": p.css(".desc::text").get(),
                            }
                            for p in s.css(".product")
                        ],
                    }

                cases["parsel"] = parsel_run

            if has_lib("scrapling"):
                try:
                    SCls = _get_scrapling_cls()

                    def scrapling_run(h=html, cls=SCls):
                        page = cls(h, adaptive=False)
                        return {
                            "title": _scrapling_first(page, "h1").text,
                            "items": [
                                {
                                    "id": int(p.attrib["data-id"]),
                                    "name": _scrapling_first(p, ".name").text,
                                    "price": float(_scrapling_first(p, ".price").text.lstrip("$")),
                                    "url": _scrapling_first(p, ".link").attrib["href"],
                                    "desc": _scrapling_first(p, ".desc").text,
                                }
                                for p in page.css(".product")
                            ],
                        }

                    scrapling_run()
                    cases["scrapling"] = scrapling_run
                except Exception as exc:
                    record_skip("01_catalog: scrapling", exc)

            if has_lib("bs4"):
                from bs4 import BeautifulSoup

                def bs4_lxml_run(h=html):
                    s = BeautifulSoup(h, "lxml")
                    return {
                        "title": s.select_one("h1").get_text(),
                        "items": [
                            {
                                "id": int(p["data-id"]),
                                "name": p.select_one(".name").get_text(),
                                "price": float(p.select_one(".price").get_text().lstrip("$")),
                                "url": p.select_one(".link")["href"],
                                "desc": p.select_one(".desc").get_text(),
                            }
                            for p in s.select(".product")
                        ],
                    }

                if has_lib("lxml"):
                    cases["bs4 + lxml"] = bs4_lxml_run

            expected = {"title": "Catalog", "items": [
                {"id": i, "name": f"Product {i}", "price": float(f"{i % 500}.99"),
                 "url": f"/p/{i}", "desc": f"Some description text for product {i}"}
                for i in range(n)
            ]}

            def validate(name: str, out: Any) -> None:
                if out != expected:
                    raise ValueError(f"{name}: catalog extraction mismatch")

            timings = measure_interleaved(cases, repeat=repeat, validator=validate)

            # run_native also counts result values and releases its Rust result.
            # The difference between independently timed paths is only an estimate.
            for eng in available_oxide_engines(("dom", "stream")):
                plan = compile(schema, backend=eng)
                native_expected = plan.run_native(html)

                def validate_breakdown(name, out, count=native_expected):
                    if out != (expected if name == "extract" else count):
                        raise ValueError(f"Invalid {name} result")

                measured = measure_interleaved({"extract": lambda p=plan: p.extract(html),
                                                "run_native": lambda p=plan: p.run_native(html)},
                                               repeat=repeat, validator=validate_breakdown)
                tot_ms = measured["extract"]["median_ms"]
                nat_ms = measured["run_native"]["median_ms"]
                native_breakdown[eng] = {
                    "total_ms": round(tot_ms, 4),
                    "rust_native_ms": round(nat_ms, 4),
                    "difference_ms": round(tot_ms - nat_ms, 4),
                    "timings": measured,
                }

            results.append({
                "scenario": "01_catalog_scaling",
                "label": f"Catalog n={n}, noise={noise} ({len(html.encode('utf-8'))/1024:.1f} KiB)",
                "n_products": n,
                "noise": noise,
                "bytes": len(html.encode("utf-8")),
                "timings": timings,
                "native_breakdown": native_breakdown,
            })
    return results


# =============================================================================================
# SCENARIO 02: Early Exit / Head Extraction on a ~2.5 MiB Page
# =============================================================================================
def run_02_early_exit(repeat: int) -> Dict[str, Any]:
    html = gen.make_early_exit_page(n_tail_products=8000)
    schema = {
        "title": T.css("title").first().text(),
        "og_title": T.css('meta[property="og:title"]').first().attr("content"),
        "canonical": T.css('link[rel="canonical"]').first().attr("href"),
        "h1": T.css("h1.hero-title").first().text(),
        "first_sku": T.css(".first-offer").first().attr("data-sku"),
        "first_price": T.css(".first-offer .offer-price").first().text().float(),
    }
    cases: Dict[str, Callable[[], Any]] = {}
    for eng in available_oxide_engines(("dom", "stream")):
        p = compile(schema, backend=eng)
        cases[f"oxide_parse [{eng}]"] = (lambda p=p: p.extract(html))

    if has_lib("selectolax"):
        from selectolax.lexbor import LexborHTMLParser

        def slx_head():
            t = LexborHTMLParser(html)
            fo = t.css_first(".first-offer")
            return {
                "title": t.css_first("title").text(),
                "og_title": t.css_first('meta[property="og:title"]').attributes["content"],
                "canonical": t.css_first('link[rel="canonical"]').attributes["href"],
                "h1": t.css_first("h1.hero-title").text(),
                "first_sku": fo.attributes["data-sku"],
                "first_price": float(fo.css_first(".offer-price").text().lstrip("$")),
            }

        cases["selectolax [lexbor]"] = slx_head

    if has_lib("lxml"):
        from lxml import html as lhtml

        def lxml_head():
            t = lhtml.fromstring(html)
            return {
                "title": t.findtext(".//title"),
                "og_title": t.xpath("string(//meta[@property='og:title']/@content)"),
                "canonical": t.xpath("string(//link[@rel='canonical']/@href)"),
                "h1": t.xpath("string(//h1[@class='hero-title'])"),
                "first_sku": t.xpath("string(//div[@class='first-offer'][1]/@data-sku)"),
                "first_price": float(t.xpath("string(//div[@class='first-offer'][1]/span[@class='offer-price'])").lstrip("$")),
            }

        cases["lxml [direct]"] = lxml_head

    if has_lib("parsel"):
        from parsel import Selector

        def parsel_head():
            s = Selector(text=html)
            return {
                "title": s.css("title::text").get(),
                "og_title": s.css('meta[property="og:title"]::attr(content)').get(),
                "canonical": s.css('link[rel="canonical"]::attr(href)').get(),
                "h1": s.css("h1.hero-title::text").get(),
                "first_sku": s.css(".first-offer::attr(data-sku)").get(),
                "first_price": float(s.css(".first-offer .offer-price::text").get().lstrip("$")),
            }

        cases["parsel"] = parsel_head

    return {
        "scenario": "02_early_exit_head",
        "label": f"Early-Exit Head + 1st Offer on {len(html.encode('utf-8'))/1024:.0f} KiB page",
        "bytes": len(html.encode("utf-8")),
        "timings": measure_interleaved(cases, repeat=repeat),
    }


# =============================================================================================
# SCENARIO 03: Deep DOM Nesting & Complex CSS Combinators
# =============================================================================================
def run_03_deep_combinators(repeat: int) -> Dict[str, Any]:
    html = gen.make_deep_combinator_page(n_blocks=400, depth=25)
    sel_str = 'div.lvl-0 main#app > section.card-wrap ul.spec-list > li.spec-item:not(.muted) a[data-kind^="primary-"]'
    schema = {
        "links": T.css(sel_str).map(T.attr("href")),
        "second_vals": T.css("section.card-wrap ul.spec-list > li.spec-item:nth-child(2) span.val").map(T.text().int()),
    }
    cases: Dict[str, Callable[[], Any]] = {}
    for eng in available_oxide_engines(("dom", "stream")):
        try:
            p = compile(schema, backend=eng)
            cases[f"oxide_parse [{eng}]"] = (lambda p=p: p.extract(html))
        except Exception as exc:
            record_skip(f"03_deep_css: oxide_parse [{eng}]", exc)

    if has_lib("selectolax"):
        from selectolax.lexbor import LexborHTMLParser

        def slx_deep():
            t = LexborHTMLParser(html)
            return {
                "links": [a.attributes["href"] for a in t.css(sel_str)],
                "second_vals": [
                    int(s.text())
                    for s in t.css("section.card-wrap ul.spec-list > li.spec-item:nth-child(2) span.val")
                ],
            }

        cases["selectolax [lexbor]"] = slx_deep

    if has_lib("parsel"):
        from parsel import Selector

        def parsel_deep():
            s = Selector(text=html)
            return {
                "links": s.css(sel_str + "::attr(href)").getall(),
                "second_vals": [
                    int(x)
                    for x in s.css("section.card-wrap ul.spec-list > li.spec-item:nth-child(2) span.val::text").getall()
                ],
            }

        cases["parsel"] = parsel_deep

    return {
        "scenario": "03_deep_css_combinators",
        "label": f"Deep DOM (depth=25, 400 blocks, :not/:nth-child/[attr^=]) ({len(html.encode('utf-8'))/1024:.1f} KiB)",
        "bytes": len(html.encode("utf-8")),
        "timings": measure_interleaved(cases, repeat=repeat),
    }


# =============================================================================================
# SCENARIO 04: XPath 1.0 Torture Benchmark (Axes, Predicates, Sibling Lookups)
# =============================================================================================
def run_04_xpath_torture(repeat: int) -> Dict[str, Any]:
    html = gen.make_xpath_page(n_rows=600)
    schema = {
        "premium_ids": T.xpath("//div[contains(@class, 'premium')]/@data-id"),
        "weights": T.xpath("//div[@class='row-item standard']/span[@class='label']/following-sibling::span[1]/text()"),
        "parent_ids": T.xpath("//a[@class='action']/parent::div/@data-id"),
        "total_rows": T.xpath("count(//div[@data-id])"),
    }
    cases: Dict[str, Callable[[], Any]] = {}
    for eng in ("lite", "scraper"):
        if eng in available_oxide_engines(("dom",)):
            p = compile(schema, backend=eng)
            cases[f"oxide_parse [{eng}]"] = (lambda p=p: p.extract(html))

    if has_lib("lxml"):
        from lxml import html as lhtml

        def lxml_xp():
            t = lhtml.fromstring(html)
            return {
                "premium_ids": t.xpath("//div[contains(@class, 'premium')]/@data-id"),
                "weights": t.xpath("//div[@class='row-item standard']/span[@class='label']/following-sibling::span[1]/text()"),
                "parent_ids": t.xpath("//a[@class='action']/parent::div/@data-id"),
                "total_rows": int(t.xpath("count(//div[@data-id])")),
            }

        cases["lxml [xpath]"] = lxml_xp

    if has_lib("parsel"):
        from parsel import Selector

        def parsel_xp():
            s = Selector(text=html)
            return {
                "premium_ids": s.xpath("//div[contains(@class, 'premium')]/@data-id").getall(),
                "weights": s.xpath("//div[@class='row-item standard']/span[@class='label']/following-sibling::span[1]/text()").getall(),
                "parent_ids": s.xpath("//a[@class='action']/parent::div/@data-id").getall(),
                "total_rows": int(float(s.xpath("count(//div[@data-id])").get())),
            }

        cases["parsel [xpath]"] = parsel_xp

    return {
        "scenario": "04_xpath_torture",
        "label": f"XPath 1.0 (contains, following-sibling, parent, count on 600 rows) ({len(html.encode('utf-8'))/1024:.1f} KiB)",
        "bytes": len(html.encode("utf-8")),
        "timings": measure_interleaved(cases, repeat=repeat),
    }


# =============================================================================================
# SCENARIO 05: Hybrid HTML -> Embedded JSON (__NEXT_DATA__)
# =============================================================================================
def run_05_hybrid_html_json(repeat: int) -> Dict[str, Any]:
    html = gen.make_hybrid_nextjs_page(n_products=1000)
    schema = {
        "h1": T.css("h1.page-heading").text(),
        "category": T.css("script#__NEXT_DATA__").text().parse_json()["props"]["pageProps"]["category"],
        "skus": T.css("script#__NEXT_DATA__").text().parse_json()["props"]["pageProps"]["products"].map(T["sku"]),
    }
    cases: Dict[str, Callable[[], Any]] = {}
    for eng in available_oxide_engines(("dom", "stream")):
        p = compile(schema, backend=eng)
        cases[f"oxide_parse [{eng}]"] = (lambda p=p: p.extract(html))

    if has_lib("selectolax"):
        from selectolax.lexbor import LexborHTMLParser

        def slx_json():
            t = LexborHTMLParser(html)
            data = json.loads(t.css_first("script#__NEXT_DATA__").text())
            pp = data["props"]["pageProps"]
            return {
                "h1": t.css_first("h1.page-heading").text(),
                "category": pp["category"],
                "skus": [item["sku"] for item in pp["products"]],
            }

        cases["selectolax + json.loads"] = slx_json

    if has_lib("lxml"):
        from lxml import html as lhtml

        def lxml_json():
            t = lhtml.fromstring(html)
            raw = t.xpath("string(//script[@id='__NEXT_DATA__'])")
            pp = json.loads(raw)["props"]["pageProps"]
            return {
                "h1": t.xpath("string(//h1[@class='page-heading'])"),
                "category": pp["category"],
                "skus": [item["sku"] for item in pp["products"]],
            }

        cases["lxml + json.loads"] = lxml_json

    return {
        "scenario": "05_hybrid_html_json",
        "label": f"Hybrid HTML + __NEXT_DATA__ JSON (1000 items, {len(html.encode('utf-8'))/1024:.1f} KiB)",
        "bytes": len(html.encode("utf-8")),
        "timings": measure_interleaved(cases, repeat=repeat),
    }


# =============================================================================================
# SCENARIO 06: Dirty Article Text Extraction (without=".ad", sep=" ", HTML entities)
# =============================================================================================
def run_06_text_and_entities(repeat: int) -> Dict[str, Any]:
    html = gen.make_dirty_article_page(n_paragraphs=600)
    schema = {
        "title": T.css("article h1").text(),
        "clean_blocks": T.css(".para-block").map(T.text(sep=" ", strip=True, without=".ad")),
    }
    cases: Dict[str, Callable[[], Any]] = {}
    expected = None
    for eng in available_oxide_engines(("dom", "stream")):
        p = compile(schema, backend=eng)
        result = p.extract(html)
        if expected is None:
            expected = result
        elif result != expected:
            raise AssertionError(f"{eng}: text exclusion differs from the first oxide_parse engine")
        cases[f"oxide_parse [{eng}]"] = (lambda p=p: p.extract(html))

    if has_lib("selectolax"):
        from selectolax.lexbor import LexborHTMLParser

        def slx_clean():
            t = LexborHTMLParser(html)
            for bad in t.css("script, style, .ad"):
                bad.decompose()
            return {
                "title": t.css_first("article h1").text(),
                "clean_blocks": [b.text(separator=" ", strip=True) for b in t.css(".para-block")],
            }

        cases["selectolax [strip_tags]"] = slx_clean

    if has_lib("bs4") and has_lib("lxml"):
        from bs4 import BeautifulSoup

        def bs4_clean():
            s = BeautifulSoup(html, "lxml")
            for bad in s.select("script, style, .ad"):
                bad.decompose()
            return {
                "title": s.select_one("article h1").get_text(),
                "clean_blocks": [b.get_text(separator=" ", strip=True) for b in s.select(".para-block")],
            }

        cases["bs4 + lxml [decompose]"] = bs4_clean

    return {
        "scenario": "06_text_and_entities",
        "label": f"Dirty Text + Entities + Subtree Exclusion (600 blocks, {len(html.encode('utf-8'))/1024:.1f} KiB)",
        "bytes": len(html.encode("utf-8")),
        "timings": measure_interleaved(cases, repeat=repeat),
    }


# =============================================================================================
# SCENARIO 07: Adaptive Self-Healing After Redesign (oxide_parse vs Scrapling)
# =============================================================================================
def run_07_adaptive_redesign(repeat: int) -> List[Dict[str, Any]]:
    out_list = []
    for n_nodes in (400, 1600):
        v1_html, v2_html = gen.make_adaptive_pages(n_noise_nodes=n_nodes)
        schema = {
            "title": T.css("#prod-main .prod-title", adaptive="title").text(),
            "desc": T.css("#prod-main .prod-description", adaptive="desc").text(),
            "price": T.css("#prod-main .prod-price", adaptive="price").text(),
        }
        hot_cases: Dict[str, Callable[[], Any]] = {}
        cold_cases: Dict[str, Callable[[], Any]] = {}
        cold_prepare = {}
        expected = {"title": "Ergonomic Desk Lamp Pro",
                    "desc": "Warm LED desk lamp with stepless dimming and USB-C port.", "price": "$49.90"}

        def validate_adaptive(name, result):
            if result != expected:
                raise ValueError(f"{name}: adaptive extraction mismatch")

        for eng in ("lite", "scraper"):
            if eng in available_oxide_engines(("dom",)):
                p = compile(schema, backend=eng)
                v1_res = p.extract(v1_html)
                validate_adaptive(eng, v1_res)
                hot_cases[f"oxide_parse [{eng}] (v1 match)"] = (lambda p=p, h=v1_html: p.extract(h))
                holder = [p]
                name = f"oxide_parse [{eng}] (v2 relocate)"

                def reset(engine=eng, holder=holder):
                    holder[0] = compile(schema, backend=engine)
                    holder[0].extract(v1_html)

                cold_prepare[name] = reset
                cold_cases[name] = (lambda holder=holder, h=v2_html: holder[0].extract(h))

        if has_lib("scrapling"):
            try:
                SCls = _get_scrapling_cls()
                logging.getLogger("scrapling").setLevel(logging.WARNING)
                selectors = {"title": "#prod-main .prod-title", "desc": "#prod-main .prod-description",
                             "price": "#prod-main .prod-price"}
                hot_args = {"storage_file": ":memory:", "url": f"https://hot-{n_nodes}.example"}
                cold_state = {"generation": 0, "args": None}

                def scrapling_extract(html, args, save=False, relocate=False):
                    doc = SCls(html, adaptive=True, url=args["url"], storage_args=args)
                    return {field: _scrapling_first(doc, selector, auto_save=save, adaptive=relocate).text
                            for field, selector in selectors.items()}

                def reset_scrapling():
                    cold_state["generation"] += 1
                    cold_state["args"] = {
                        "storage_file": ":memory:",
                        "url": f"https://cold-{n_nodes}-{cold_state['generation']}.example",
                    }
                    result = scrapling_extract(v1_html, cold_state["args"], save=True)
                    validate_adaptive("scrapling seed", result)

                validate_adaptive("scrapling seed", scrapling_extract(v1_html, hot_args, save=True))
                hot_cases["scrapling [SQLite memory] (v1 match)"] = lambda: scrapling_extract(v1_html, hot_args)
                name = "scrapling [SQLite memory] (v2 relocate)"
                cold_cases[name] = lambda: scrapling_extract(v2_html, cold_state["args"], relocate=True)
                cold_prepare[name] = reset_scrapling
            except Exception as exc:
                record_skip("07_adaptive: scrapling", exc)

        out_list.append({
            "scenario": f"07a_adaptive_hot_{n_nodes}",
            "label": f"Adaptive Hot Path (Selector Matches, {n_nodes} filler nodes)",
            "bytes": len(v1_html.encode("utf-8")),
            "timings": measure_interleaved(hot_cases, repeat=repeat, validator=validate_adaptive),
        })
        out_list.append({
            "scenario": f"07b_adaptive_relocate_{n_nodes}",
            "label": f"Adaptive Cold Path (Broken Selector Relocation across {n_nodes} nodes)",
            "bytes": len(v2_html.encode("utf-8")),
            "timings": measure_interleaved(cold_cases, repeat=repeat, prepare=cold_prepare,
                                            validator=validate_adaptive),
        })
    return out_list


# =============================================================================================
# SCENARIO 08: Multi-Document Batch Parallel Processing (32 pages x 500 products)
# =============================================================================================
def run_08_batch_parallel(repeat: int) -> Dict[str, Any]:
    docs = [gen.make_catalog_page(500, noise=True) for _ in range(32)]
    schema = {
        "title": T.css("h1").text(),
        "items": T.css(".product").map({
            "id": T.attr("data-id").int(),
            "name": T.css(".name").text(),
            "price": T.css(".price").text().float(),
        }),
    }
    cases: Dict[str, Callable[[], Any]] = {}
    for eng in available_oxide_engines(("dom", "stream")):
        p = compile(schema, backend=eng)
        cases[f"oxide_parse [{eng}] extract_many (Rayon)"] = (lambda p=p: p.extract_many(docs))
        cases[f"oxide_parse [{eng}] sequential loop"] = (lambda p=p: [p.extract(d) for d in docs])

    if has_lib("selectolax"):
        from selectolax.lexbor import LexborHTMLParser

        def slx_batch():
            out = []
            for d in docs:
                t = LexborHTMLParser(d)
                out.append({
                    "title": t.css_first("h1").text(),
                    "items": [
                        {
                            "id": int(p.attributes["data-id"]),
                            "name": p.css_first(".name").text(),
                            "price": float(p.css_first(".price").text().lstrip("$")),
                        }
                        for p in t.css(".product")
                    ],
                })
            return out

        cases["selectolax [lexbor] sequential loop"] = slx_batch

    return {
        "scenario": "08_batch_parallel",
        "label": f"Batch 32 docs x 500 noisy products ({sum(len(d.encode("utf-8")) for d in docs)/1024:.0f} KiB total)",
        "bytes": sum(len(d.encode("utf-8")) for d in docs),
        "timings": measure_interleaved(cases, repeat=repeat),
    }


# =============================================================================================
# SCENARIO 09: Internal Bottleneck Profiler (Fast vs Slow Numbers, Selector Scaling)
# =============================================================================================
def run_09_internal_bottlenecks(repeat: int) -> List[Dict[str, Any]]:
    ascii_html, local_html = gen.make_localized_numbers_page(n_items=2000)
    num_schema = {
        "items": T.css(".it").map({
            "id": T.attr("data-id").int(),
            "p": T.css(".p").text().float(),
        })
    }
    num_cases: Dict[str, Callable[[], Any]] = {}
    num_counts = {}
    for eng in available_oxide_engines(("dom", "stream")):
        p = compile(num_schema, backend=eng)
        for label, html in (("fast ASCII ('$19.99')", ascii_html), ("localized ('1 299,50 ₽')", local_html)):
            expected = {"items": [
                {"id": i, "p": float(f"{(i % 900) + 10}.99") if html is ascii_html else ((i % 90) + 1) * 1000 + 299.5}
                for i in range(2000)
            ]}
            if p.extract(html) != expected:
                raise ValueError(f"{eng}: number parsing mismatch")
            name = f"{eng}: {label}"
            num_counts[name] = p.run_native(html)
            num_cases[name] = (lambda p=p, h=html: p.run_native(h))

    # Sub-test B: 2 selectors vs 11 selectors on the same page
    cat_html = gen.make_catalog_page(1000, noise=True)
    s_few = {"items": T.css(".product").map({"n": T.css(".name").text()})}
    s_many = {
        "items": T.css(".product").map({
            "f1": T.css(".name").text(),
            "f2": T.css(".price").text(),
            "f3": T.css(".link").attr("href"),
            "f4": T.css(".desc").text(),
            "f5": T.css("button.buy").has_attr("disabled"),
            "f6": T.css(".missing-1").text(),
            "f7": T.css(".missing-2").text(),
            "f8": T.css(".missing-3").text(),
            "f9": T.css("div.promo-inner span.nope").text(),
            "f10": T.css("ul.breadcrumbs li.nope a").attr("href"),
        }),
    }
    sel_cases: Dict[str, Callable[[], Any]] = {}
    sel_counts = {}
    for eng in available_oxide_engines(("dom", "stream")):
        p_few = compile(s_few, backend=eng)
        p_many = compile(s_many, backend=eng)
        expected_few = {"items": [{"n": f"Product {i}"} for i in range(1000)]}
        expected_many = {"items": [
            {"f1": f"Product {i}", "f2": f"${i % 500}.99", "f3": f"/p/{i}",
             "f4": f"Some description text for product {i}", "f5": i % 5 == 0,
             "f6": None, "f7": None, "f8": None, "f9": None, "f10": None}
            for i in range(1000)
        ]}
        if p_few.extract(cat_html) != expected_few or p_many.extract(cat_html) != expected_many:
            raise ValueError(f"{eng}: selector fanout result mismatch")
        sel_cases[f"{eng}: 2 selectors (run_native)"] = (lambda p=p_few: p.run_native(cat_html))
        sel_cases[f"{eng}: 11 selectors incl. 5 misses (run_native)"] = (lambda p=p_many: p.run_native(cat_html))

    for name, fn in sel_cases.items():
        sel_counts[name] = fn()

    def validate_count(name, out, counts):
        if out != counts[name]:
            raise ValueError(f"{name}: inconsistent native result")

    return [
        {
            "scenario": "09a_number_parser_bottleneck",
            "label": "Internal Profiler: Fast ASCII vs Localized Number Parsing (2000 items, run_native)",
            "bytes": len(local_html.encode("utf-8")),
            "timings": measure_interleaved(num_cases, repeat=repeat,
                                            validator=lambda name, out: validate_count(name, out, num_counts)),
        },
        {
            "scenario": "09b_selector_fanout_bottleneck",
            "label": "Internal Profiler: 2 Selectors vs 11 Selectors on 1000 noisy cards (run_native)",
            "bytes": len(cat_html.encode("utf-8")),
            "timings": measure_interleaved(sel_cases, repeat=repeat,
                                            validator=lambda name, out: validate_count(name, out, sel_counts)),
        },
    ]


# =============================================================================================
# SCENARIO 10: HTML -> Markdown for RAG / LLM input (oxide_parse vs markdownify vs html2text)
# =============================================================================================
def run_10_markdown_rag(repeat: int) -> Dict[str, Any]:
    """The same page converted to Markdown by every library (whole body; oxide_parse drops script/style/svg itself)."""
    html = gen.make_dirty_article_page(n_paragraphs=300)
    cases: Dict[str, Callable[[], Any]] = {}
    expected: Optional[str] = None
    for eng in available_oxide_engines(("dom",)):
        p = compile(T.css("body").markdown(), backend=eng)
        out = p.extract(html)
        if not isinstance(out, str) or not out.strip():
            raise AssertionError(f"{eng}: markdown() returned {out!r}")
        if expected is None:
            expected = out
        elif out != expected:
            raise AssertionError(f"{eng}: markdown differs from the first oxide_parse engine")
        cases[f"oxide_parse [{eng}]"] = (lambda p=p: p.extract(html))

    if has_lib("markdownify"):
        from markdownify import markdownify

        cases["markdownify"] = lambda: markdownify(html)

    if has_lib("html2text"):
        import html2text

        def h2t() -> str:
            conv = html2text.HTML2Text()
            conv.body_width = 0
            return conv.handle(html)

        cases["html2text"] = h2t

    def validate_markdown(name: str, output: Any) -> None:
        if not isinstance(output, str) or "Deep Review" not in output or "Paragraph 299" not in output:
            raise AssertionError(f"{name}: markdown is missing expected article content")

    return {
        "scenario": "10_markdown_rag",
        "label": f"HTML -> Markdown for RAG (300 blocks, {len(html.encode('utf-8'))/1024:.1f} KiB)",
        "bytes": len(html.encode("utf-8")),
        "timings": measure_interleaved(cases, repeat=repeat, validator=validate_markdown),
    }


# =============================================================================================
# SCENARIO 11: Scrapling Official Benchmarks (5000 Nested Elements & find_similar Relocation)
# =============================================================================================
def run_11_scrapling_benchmark(repeat: int) -> List[Dict[str, Any]]:
    # 11a: Text Extraction Speed Test (5,000 nested elements)
    html_5k = gen.make_scrapling_nested_page(n_items=5000, depth=16)
    schema_5k = {"items": T.css(".item .text").map(T.text())}
    cases_5k: Dict[str, Callable[[], Any]] = {}
    for eng in available_oxide_engines(("dom", "stream")):
        p = compile(schema_5k, backend=eng)
        cases_5k[f"oxide_parse [{eng}]"] = (lambda p=p: p.extract(html_5k))

    if has_lib("scrapling"):
        try:
            SCls = _get_scrapling_cls()
            cases_5k["scrapling"] = (lambda cls=SCls: {"items": list(cls(html_5k, adaptive=False).css(".item .text::text").getall())})
        except Exception as exc:
            record_skip("11a_scrapling: scrapling", exc)

    if has_lib("selectolax"):
        from selectolax.lexbor import LexborHTMLParser

        cases_5k["selectolax [lexbor]"] = lambda: {"items": [n.text() for n in LexborHTMLParser(html_5k).css(".item .text")]}

    if has_lib("frostwork"):
        try:
            import frostwork as fw

            fw_page = fw.Page().field_all("items", ".item .text::text")
            cases_5k["frostwork"] = (lambda page=fw_page: page.extract(html_5k).to_dict())
        except Exception as exc:
            record_skip("11a_scrapling: frostwork", exc)

    if has_lib("parsel"):
        from parsel import Selector

        cases_5k["parsel"] = lambda: {"items": Selector(text=html_5k).css(".item .text::text").getall()}

    if has_lib("lxml"):
        from lxml import html as lhtml

        cases_5k["lxml [direct]"] = lambda: {"items": lhtml.fromstring(html_5k).xpath("//div[@class='item']/span[@class='text']/text()")}

    if has_lib("bs4") and has_lib("lxml"):
        from bs4 import BeautifulSoup

        cases_5k["bs4 + lxml"] = lambda: {"items": [el.get_text() for el in BeautifulSoup(html_5k, "lxml").select(".item .text")]}

    # 11b: Scrapling find_similar() vs oxide_parse adaptive list (500 redesigned cards)
    v1_html, v2_html = gen.make_scrapling_similar_pages(n_cards=500)
    expected_titles = [f"Product {i}" for i in range(500)]
    sim_schema = {"items": T.css(".product", adaptive="cards").map(T.css("h3").text())}
    cases_sim: Dict[str, Callable[[], Any]] = {}
    prepare_sim: Dict[str, Callable[[], None]] = {}

    for eng in ("lite", "scraper"):
        if eng in available_oxide_engines(("dom",)):
            holder = [compile(sim_schema, backend=eng)]
            name = f"oxide_parse [{eng}] (adaptive list)"

            def reset_op(engine=eng, holder=holder):
                holder[0] = compile(sim_schema, backend=engine)
                holder[0].extract(v1_html)

            prepare_sim[name] = reset_op
            cases_sim[name] = (lambda holder=holder: holder[0].extract(v2_html))

    if has_lib("scrapling"):
        try:
            SCls = _get_scrapling_cls()

            def sc_find_similar(cls=SCls):
                doc = cls(v2_html, adaptive=False)
                first = doc.css(".item-card").first
                return {"items": [str(first.css("h3::text").get()), *[str(x.css("h3::text").get()) for x in first.find_similar()]]}

            if sc_find_similar()["items"] != expected_titles:
                raise ValueError("scrapling find_similar returned unexpected titles")
            cases_sim["scrapling [find_similar]"] = sc_find_similar
        except Exception as exc:
            record_skip("11b_scrapling_similar: scrapling", exc)

    return [
        {
            "scenario": "11a_scrapling_nested_5000",
            "label": f"Scrapling Bench: 5000 Nested Elements ({len(html_5k.encode('utf-8'))/1024:.1f} KiB)",
            "bytes": len(html_5k.encode("utf-8")),
            "timings": measure_interleaved(cases_5k, repeat=repeat),
        },
        {
            "scenario": "11b_scrapling_find_similar_500",
            "label": f"Scrapling Bench: find_similar vs adaptive list on 500 cards ({len(v2_html.encode('utf-8'))/1024:.1f} KiB)",
            "bytes": len(v2_html.encode("utf-8")),
            "timings": measure_interleaved(cases_sim, repeat=repeat, prepare=prepare_sim),
        },
    ]


# =============================================================================================
# SCENARIO 12: Selectolax Official Benchmark (Clean Text + Title + Links)
# =============================================================================================
def run_12_selectolax_benchmark(repeat: int) -> Dict[str, Any]:
    html = gen.make_selectolax_text_page(n_sections=400)
    schema = {
        "title": T.css("title").text(),
        "text": T.css("body").text("\n", strip=True),
        "links": T.css("a[href]").map(T.attr("href")),
    }
    cases: Dict[str, Callable[[], Any]] = {}
    for eng in available_oxide_engines(("dom", "stream")):
        p = compile(schema, backend=eng)
        cases[f"oxide_parse [{eng}]"] = (lambda p=p: p.extract(html))

    if has_lib("selectolax"):
        from selectolax.lexbor import LexborHTMLParser

        def slx_run():
            t = LexborHTMLParser(html)
            for bad in t.css("script, style"):
                bad.decompose()
            return {
                "title": t.css_first("title").text(),
                "text": t.body.text(separator="\n", strip=True),
                "links": [a.attributes["href"] for a in t.css("a[href]")],
            }

        cases["selectolax [lexbor]"] = slx_run

    if has_lib("lxml"):
        from lxml import html as lhtml

        def lxml_run():
            t = lhtml.fromstring(html)
            return {
                "title": t.findtext(".//title"),
                "text": "\n".join(s.strip() for s in t.xpath("//body//text()[not(ancestor::script or ancestor::style)]") if s.strip()),
                "links": t.xpath("//a[@href]/@href"),
            }

        cases["lxml [direct]"] = lxml_run

    if has_lib("parsel"):
        from parsel import Selector

        def parsel_run():
            s = Selector(text=html)
            return {
                "title": s.css("title::text").get(),
                "text": "\n".join(x.strip() for x in s.xpath("//body//text()[not(ancestor::script or ancestor::style)]").getall() if x.strip()),
                "links": s.css("a[href]::attr(href)").getall(),
            }

        cases["parsel"] = parsel_run

    if has_lib("bs4") and has_lib("lxml"):
        from bs4 import BeautifulSoup

        def bs4_run():
            s = BeautifulSoup(html, "lxml")
            for bad in s.select("script, style"):
                bad.decompose()
            return {
                "title": s.select_one("title").get_text(),
                "text": s.body.get_text("\n", strip=True),
                "links": [a["href"] for a in s.select("a[href]")],
            }

        cases["bs4 + lxml"] = bs4_run

    return {
        "scenario": "12_selectolax_bench",
        "label": f"Selectolax Bench: Strip script/style + body text + 800 links ({len(html.encode('utf-8'))/1024:.1f} KiB)",
        "bytes": len(html.encode("utf-8")),
        "timings": measure_interleaved(cases, repeat=repeat),
    }


# =============================================================================================
# SCENARIO 13: Frostwork Official Benchmarks (Grouped Page + First-Only Early Exit)
# =============================================================================================
def run_13_frostwork_benchmark(repeat: int) -> List[Dict[str, Any]]:
    # 13a: Grouped Page (flat header fields + 2000 offers)
    html_g = gen.make_frostwork_offers_page(n_offers=2000)
    selector_map = {
        "h1": "h1::text",
        "desc": 'meta[name="description"]::attr(content)',
        "canonical": 'link[rel="canonical"]::attr(href)',
        "offers": [{
            "_base": ".offer",
            "sku": "::attr(data-sku)",
            "title": "h2::text",
            "price": ".price::text",
            "href": "a::attr(href)",
        }],
    }
    ir_g = migrate.from_selector_map(selector_map, strip=False)
    cases_g: Dict[str, Callable[[], Any]] = {}
    for eng in available_oxide_engines(("dom", "stream")):
        p = Plan.from_ir(ir_g, backend=eng)
        cases_g[f"oxide_parse [{eng}]"] = (lambda p=p: p.extract(html_g))

    if has_lib("frostwork"):
        try:
            import frostwork as fw

            fw_g = (
                fw.Page()
                .field("h1", "h1::text")
                .field("desc", 'meta[name="description"]::attr(content)')
                .field("canonical", 'link[rel="canonical"]::attr(href)')
                .many("offers", ".offer", {
                    "sku": "::attr(data-sku)",
                    "title": "h2::text",
                    "price": ".price::text",
                    "href": "a::attr(href)",
                })
            )
            cases_g["frostwork"] = (lambda page=fw_g: page.extract(html_g).to_dict())
        except Exception as exc:
            record_skip("13a_frostwork: frostwork", exc)

    if has_lib("selectolax"):
        from selectolax.lexbor import LexborHTMLParser

        def slx_g():
            t = LexborHTMLParser(html_g)
            return {
                "h1": t.css_first("h1").text(),
                "desc": t.css_first('meta[name="description"]').attributes["content"],
                "canonical": t.css_first('link[rel="canonical"]').attributes["href"],
                "offers": [
                    {
                        "sku": o.attributes["data-sku"],
                        "title": o.css_first("h2").text(),
                        "price": o.css_first(".price").text(),
                        "href": o.css_first("a").attributes["href"],
                    }
                    for o in t.css(".offer")
                ],
            }

        cases_g["selectolax [lexbor]"] = slx_g

    if has_lib("parsel"):
        from parsel import Selector

        def parsel_g():
            s = Selector(text=html_g)
            return {
                "h1": s.css("h1::text").get(),
                "desc": s.css('meta[name="description"]::attr(content)').get(),
                "canonical": s.css('link[rel="canonical"]::attr(href)').get(),
                "offers": [
                    {
                        "sku": o.attrib["data-sku"],
                        "title": o.css("h2::text").get(),
                        "price": o.css(".price::text").get(),
                        "href": o.css("a::attr(href)").get(),
                    }
                    for o in s.css(".offer")
                ],
            }

        cases_g["parsel"] = parsel_g

    if has_lib("scrapling"):
        try:
            SCls = _get_scrapling_cls()

            def sc_g(cls=SCls):
                doc = cls(html_g, adaptive=False)
                return {
                    "h1": _scrapling_first(doc, "h1").text,
                    "desc": _scrapling_first(doc, 'meta[name="description"]').attrib["content"],
                    "canonical": _scrapling_first(doc, 'link[rel="canonical"]').attrib["href"],
                    "offers": [
                        {
                            "sku": o.attrib["data-sku"],
                            "title": _scrapling_first(o, "h2").text,
                            "price": _scrapling_first(o, ".price").text,
                            "href": _scrapling_first(o, "a").attrib["href"],
                        }
                        for o in doc.css(".offer")
                    ],
                }

            cases_g["scrapling"] = sc_g
        except Exception as exc:
            record_skip("13a_frostwork: scrapling", exc)

    # 13b: Frostwork first_only Early Exit vs oxide_parse stream on a ~1.6 MiB page
    html_ee = gen.make_early_exit_page(n_tail_products=8000)
    ee_map = {
        "title": "title::text",
        "og_title": 'meta[property="og:title"]::attr(content)',
        "canonical": 'link[rel="canonical"]::attr(href)',
        "h1": "h1.hero-title::text",
        "first_sku": ".first-offer::attr(data-sku)",
        "first_price": ".first-offer .offer-price::text",
    }
    ir_ee = migrate.from_selector_map(ee_map, strip=False)
    cases_ee: Dict[str, Callable[[], Any]] = {}
    for eng in available_oxide_engines(("dom", "stream")):
        p = Plan.from_ir(ir_ee, backend=eng)
        cases_ee[f"oxide_parse [{eng}]"] = (lambda p=p: p.extract(html_ee))

    if has_lib("frostwork"):
        try:
            import frostwork as fw

            fw_ee = fw.Page()
            for fname, fsel in ee_map.items():
                fw_ee = fw_ee.field(fname, fsel)
            cases_ee["frostwork [first_only]"] = (lambda page=fw_ee: page.extract(html_ee).to_dict())
        except Exception as exc:
            record_skip("13b_frostwork_ee: frostwork", exc)

    if has_lib("selectolax"):
        from selectolax.lexbor import LexborHTMLParser

        def slx_ee():
            t = LexborHTMLParser(html_ee)
            fo = t.css_first(".first-offer")
            return {
                "title": t.css_first("title").text(),
                "og_title": t.css_first('meta[property="og:title"]').attributes["content"],
                "canonical": t.css_first('link[rel="canonical"]').attributes["href"],
                "h1": t.css_first("h1.hero-title").text(),
                "first_sku": fo.attributes["data-sku"],
                "first_price": fo.css_first(".offer-price").text(),
            }

        cases_ee["selectolax [lexbor]"] = slx_ee

    if has_lib("parsel"):
        from parsel import Selector

        def parsel_ee():
            s = Selector(text=html_ee)
            return {k: s.css(v).get() for k, v in ee_map.items()}

        cases_ee["parsel"] = parsel_ee

    return [
        {
            "scenario": "13a_frostwork_grouped_page",
            "label": f"Frostwork Bench: Grouped Page (header + 2000 offers, {len(html_g.encode('utf-8'))/1024:.1f} KiB)",
            "bytes": len(html_g.encode("utf-8")),
            "timings": measure_interleaved(cases_g, repeat=repeat),
        },
        {
            "scenario": "13b_frostwork_first_only_early_exit",
            "label": f"Frostwork Bench: first_only Early Exit ({len(html_ee.encode('utf-8'))/1024:.0f} KiB)",
            "bytes": len(html_ee.encode("utf-8")),
            "timings": measure_interleaved(cases_ee, repeat=repeat),
        },
    ]


# =============================================================================================
# SCENARIO 14: Scrapy / Parsel Canonical Quotes Benchmark (500 quotes with nested tag lists)
# =============================================================================================
def run_14_parsel_scrapy_quotes(repeat: int) -> Dict[str, Any]:
    html = gen.make_parsel_quotes_page(n_quotes=500)
    schema = {
        "quotes": T.css("div.quote").map({
            "quote": T.css("span.text").text(),
            "author": T.css("small.author").text(),
            "author_url": T.css("a.author-link").attr("href"),
            "tags": T.css("a.tag").map(T.text()),
        }),
        "next": T.css("li.next a").attr("href"),
    }
    cases: Dict[str, Callable[[], Any]] = {}
    for eng in ("lite", "scraper"):
        if eng in available_oxide_engines(("dom",)):
            p = compile(schema, backend=eng)
            cases[f"oxide_parse [{eng}]"] = (lambda p=p: p.extract(html))

    if has_lib("frostwork"):
        try:
            import frostwork as fw

            fw_q = (
                fw.Page()
                .many("quotes", "div.quote", {
                    "quote": "span.text::text",
                    "author": "small.author::text",
                    "author_url": "a.author-link::attr(href)",
                    "tags": ("a.tag::text", "all"),
                })
                .field("next", "li.next a::attr(href)")
            )
            cases["frostwork"] = (lambda page=fw_q: page.extract(html).to_dict())
        except Exception as exc:
            record_skip("14_parsel_quotes: frostwork", exc)

    if has_lib("selectolax"):
        from selectolax.lexbor import LexborHTMLParser

        def slx_q():
            t = LexborHTMLParser(html)
            return {
                "quotes": [
                    {
                        "quote": q.css_first("span.text").text(),
                        "author": q.css_first("small.author").text(),
                        "author_url": q.css_first("a.author-link").attributes["href"],
                        "tags": [a.text() for a in q.css("a.tag")],
                    }
                    for q in t.css("div.quote")
                ],
                "next": t.css_first("li.next a").attributes["href"],
            }

        cases["selectolax [lexbor]"] = slx_q

    if has_lib("lxml"):
        from lxml import html as lhtml

        def lxml_q():
            t = lhtml.fromstring(html)
            return {
                "quotes": [
                    {
                        "quote": q.findtext(".//span[@class='text']"),
                        "author": q.findtext(".//small[@class='author']"),
                        "author_url": q.find(".//a[@class='author-link']").attrib["href"],
                        "tags": q.xpath(".//a[@class='tag']/text()"),
                    }
                    for q in t.xpath("//div[@class='quote']")
                ],
                "next": t.xpath("string(//li[@class='next']/a/@href)"),
            }

        cases["lxml [direct]"] = lxml_q

    if has_lib("parsel"):
        from parsel import Selector

        def parsel_q():
            s = Selector(text=html)
            return {
                "quotes": [
                    {
                        "quote": q.css("span.text::text").get(),
                        "author": q.css("small.author::text").get(),
                        "author_url": q.css("a.author-link::attr(href)").get(),
                        "tags": q.css("a.tag::text").getall(),
                    }
                    for q in s.css("div.quote")
                ],
                "next": s.css("li.next a::attr(href)").get(),
            }

        cases["parsel"] = parsel_q

    if has_lib("scrapling"):
        try:
            SCls = _get_scrapling_cls()

            def sc_q(cls=SCls):
                doc = cls(html, adaptive=False)
                return {
                    "quotes": [
                        {
                            "quote": _scrapling_first(q, "span.text").text,
                            "author": _scrapling_first(q, "small.author").text,
                            "author_url": _scrapling_first(q, "a.author-link").attrib["href"],
                            "tags": [str(x) for x in q.css("a.tag::text").getall()],
                        }
                        for q in doc.css("div.quote")
                    ],
                    "next": doc.css("li.next a::attr(href)").get(),
                }

            cases["scrapling"] = sc_q
        except Exception as exc:
            record_skip("14_parsel_quotes: scrapling", exc)

    if has_lib("bs4") and has_lib("lxml"):
        from bs4 import BeautifulSoup

        def bs4_q():
            s = BeautifulSoup(html, "lxml")
            return {
                "quotes": [
                    {
                        "quote": q.select_one("span.text").get_text(),
                        "author": q.select_one("small.author").get_text(),
                        "author_url": q.select_one("a.author-link")["href"],
                        "tags": [a.get_text() for a in q.select("a.tag")],
                    }
                    for q in s.select("div.quote")
                ],
                "next": s.select_one("li.next a")["href"],
            }

        cases["bs4 + lxml"] = bs4_q

    return {
        "scenario": "14_parsel_quotes_bench",
        "label": f"Parsel/Scrapy Quotes Bench (500 quotes with nested tags, {len(html.encode('utf-8'))/1024:.1f} KiB)",
        "bytes": len(html.encode("utf-8")),
        "timings": measure_interleaved(cases, repeat=repeat),
    }


# =============================================================================================
# SCENARIO 15: Crawl4AI Official Nested E-commerce Catalog (120 complex nested products)
# =============================================================================================
def run_15_crawl4ai_nested_ecommerce(repeat: int) -> Dict[str, Any]:
    html = gen.make_crawl4ai_nested_catalog_page(n_categories=30, n_products=4)

    def make_strategy(kind: str) -> Dict[str, Any]:
        def sel(tag: str, cls: Optional[str] = None, base: bool = False) -> str:
            if kind == "css":
                return f"{tag}.{cls}" if cls else tag
            prefix = "//" if base else ".//"
            return f"{prefix}{tag}[@class='{cls}']" if cls else prefix + tag

        return {
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

    css_strategy = make_strategy("css")
    ir = migrate.from_crawl4ai(css_strategy)
    cases: Dict[str, Callable[[], Any]] = {}
    for eng in ("lite", "scraper"):
        if eng in available_oxide_engines(("dom",)):
            p = Plan.from_ir(ir, backend=eng)
            cases[f"oxide_parse [{eng}]"] = (lambda p=p: p.extract(html))

    strategies = get_crawl4ai_strategies()
    if strategies is not None:
        JsonCss, JsonXPath = strategies
        css_ext = JsonCss(css_strategy)
        xpath_ext = JsonXPath(make_strategy("xpath"))
        cases["crawl4ai [JsonCss]"] = (lambda e=css_ext: e.extract("https://example.test/catalog", html))
        cases["crawl4ai [JsonXPath]"] = (lambda e=xpath_ext: e.extract("https://example.test/catalog", html))

    if has_lib("selectolax"):
        from selectolax.lexbor import LexborHTMLParser

        def slx_c4a():
            t = LexborHTMLParser(html)
            out = []
            for c in t.css("div.category"):
                prods = []
                for p in c.css("div.product"):
                    d = p.css_first("div.product-details")
                    prods.append({
                        "name": p.css_first("h3.product-name").text(strip=True),
                        "price": p.css_first("p.product-price").text(strip=True),
                        "details": {
                            "brand": d.css_first("span.brand").text(strip=True),
                            "model": d.css_first("span.model").text(strip=True),
                        } if d is not None else {},
                        "features": [{"feature": li.text(strip=True)} for li in p.css("ul.product-features li")],
                        "reviews": [
                            {
                                "reviewer": r.css_first("span.reviewer").text(strip=True),
                                "rating": r.css_first("span.rating").text(strip=True),
                                "comment": r.css_first("p.review-text").text(strip=True),
                            }
                            for r in p.css("div.review")
                        ],
                        "related_products": [
                            {
                                "name": rp.css_first("span.related-name").text(strip=True),
                                "price": rp.css_first("span.related-price").text(strip=True),
                            }
                            for rp in p.css("ul.related-products li")
                        ],
                    })
                out.append({"category_name": c.css_first("h2.category-name").text(strip=True), "products": prods})
            return out

        cases["selectolax [lexbor]"] = slx_c4a

    if has_lib("parsel"):
        from parsel import Selector

        def parsel_c4a():
            s = Selector(text=html)
            out = []
            for c in s.css("div.category"):
                prods = []
                for p in c.css("div.product"):
                    d = p.css("div.product-details")
                    prods.append({
                        "name": p.css("h3.product-name::text").get().strip(),
                        "price": p.css("p.product-price::text").get().strip(),
                        "details": {
                            "brand": d.css("span.brand::text").get().strip(),
                            "model": d.css("span.model::text").get().strip(),
                        } if d else {},
                        "features": [{"feature": x.strip()} for x in p.css("ul.product-features li::text").getall()],
                        "reviews": [
                            {
                                "reviewer": r.css("span.reviewer::text").get().strip(),
                                "rating": r.css("span.rating::text").get().strip(),
                                "comment": r.css("p.review-text::text").get().strip(),
                            }
                            for r in p.css("div.review")
                        ],
                        "related_products": [
                            {
                                "name": rp.css("span.related-name::text").get().strip(),
                                "price": rp.css("span.related-price::text").get().strip(),
                            }
                            for rp in p.css("ul.related-products li")
                        ],
                    })
                out.append({"category_name": c.css("h2.category-name::text").get().strip(), "products": prods})
            return out

        cases["parsel"] = parsel_c4a

    return {
        "scenario": "15_crawl4ai_nested_bench",
        "label": f"Crawl4AI Nested E-commerce Catalog (30x4 products, {len(html.encode('utf-8'))/1024:.1f} KiB)",
        "bytes": len(html.encode("utf-8")),
        "timings": measure_interleaved(cases, repeat=repeat),
    }
