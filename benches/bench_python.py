"""Python-level comparison (add --noise to pad every product with irrelevant markup). Install extras: pip install beautifulsoup4 lxml parsel selectolax
Run: python benches/bench_python.py [n_products]"""
import argparse
import statistics
import sys
import time

from oxide_parse import T, compile, engines


def make_page(n, noise=False):
    junk = (
        '<div class="banner"><span>ad</span><span>ad</span><span>ad</span><ul><li>a</li><li>b</li><li>c</li></ul></div>'
        if noise
        else ""
    )
    rows = "".join(
        f'<div class="product" data-id="{i}"><h2 class="name">Product {i}</h2><span class="price">${i % 500}.99</span>'
        f'<a class="link" href="/p/{i}">more</a><p class="desc">Some description text for product {i}</p></div>' + junk
        for i in range(n)
    )
    return f"<html><body><h1>Catalog</h1><div id=list>{rows}</div></body></html>"


def timeit(fn, repeat=7, cooldown=0):
    fn()  # warm-up
    ts = []
    for _ in range(repeat):
        if cooldown:
            time.sleep(cooldown)
        t = time.perf_counter()
        fn()
        ts.append(time.perf_counter() - t)
    return statistics.median(ts)


def build_cases(html):
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
    plan = compile(schema)
    cases = {}
    for e in engines():
        if e.kind in ("dom", "stream"):
            p = compile(schema, backend=e.name)
            cases[f"oxide_parse [{e.name}]"] = (lambda p=p: p.extract(html))

    try:
        from bs4 import BeautifulSoup

        def bs4_run(parser):
            def run():
                s = BeautifulSoup(html, parser)
                items = [
                    {"id": int(p["data-id"]), "name": p.select_one(".name").get_text(),
                     "price": float(p.select_one(".price").get_text().lstrip("$")),
                     "url": p.select_one(".link")["href"], "desc": p.select_one(".desc").get_text()}
                    for p in s.select(".product")
                ]
                return {"title": s.select_one("h1").get_text(), "items": items}
            return run

        cases["bs4 + html.parser"] = bs4_run("html.parser")
        try:
            import lxml  # noqa: F401
            cases["bs4 + lxml"] = bs4_run("lxml")
        except ImportError:
            pass
    except ImportError:
        print("bs4 not installed, skipping")

    try:
        from parsel import Selector

        def parsel_run():
            s = Selector(text=html)
            items = [
                {"id": int(p.attrib["data-id"]), "name": p.css(".name::text").get(),
                 "price": float(p.css(".price::text").get().lstrip("$")),
                 "url": p.css(".link::attr(href)").get(), "desc": p.css(".desc::text").get()}
                for p in s.css(".product")
            ]
            return {"title": s.css("h1::text").get(), "items": items}

        cases["parsel"] = parsel_run
    except ImportError:
        print("parsel not installed, skipping")

    try:
        from selectolax.lexbor import LexborHTMLParser

        def slx_run():
            t = LexborHTMLParser(html)
            items = [
                {"id": int(p.attributes["data-id"]), "name": p.css_first(".name").text(),
                 "price": float(p.css_first(".price").text().lstrip("$")),
                 "url": p.css_first(".link").attributes["href"], "desc": p.css_first(".desc").text()}
                for p in t.css(".product")
            ]
            return {"title": t.css_first("h1").text(), "items": items}

        cases["selectolax"] = slx_run
    except ImportError:
        print("selectolax not installed, skipping")

    return plan, cases


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("n", nargs="?", type=int, default=1000)
    parser.add_argument("--noise", action="store_true")
    parser.add_argument("--fast-only", action="store_true")
    parser.add_argument("--repeat", type=int, default=7)
    parser.add_argument("--cooldown", type=float, default=0)
    args = parser.parse_args()
    if args.n < 1 or args.repeat < 1 or args.cooldown < 0:
        parser.error("n/repeat must be positive; cooldown must be nonnegative")
    n, noise = args.n, args.noise
    html = make_page(n, noise)
    plan, cases = build_cases(html)
    if args.fast_only:
        cases = {name: fn for name, fn in cases.items() if name.startswith("oxide_parse")}
    expected = plan.extract(html)
    for name, fn in cases.items():
        assert fn() == expected, f"Result mismatch: {name}"
    results = {name: timeit(fn, args.repeat, args.cooldown) for name, fn in cases.items()}
    base = min(t for n, t in results.items() if n.startswith("oxide_parse"))
    print(f"\n{n} products, {len(html) / 1024:.0f} KiB  (parse + extract, median of {args.repeat})\n")
    for name, t in sorted(results.items(), key=lambda kv: kv[1]):
        print(f"{name:<28}{t * 1000:9.2f} ms   x{t / base:5.2f} vs best oxide_parse")

    docs = [html] * 16
    t_many = timeit(lambda: plan.extract_many(docs), repeat=3)
    t_loop = timeit(lambda: [plan.extract(d) for d in docs], repeat=3)
    print(f"\n16 docs: extract_many {t_many * 1000:.1f} ms vs loop {t_loop * 1000:.1f} ms (rayon, GIL released)")


if __name__ == "__main__":
    main()
