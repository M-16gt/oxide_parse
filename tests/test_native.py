"""Integration tests; skipped when the native extension is not built."""
import asyncio

import pytest

pytest.importorskip("oxide_parse._core")
from oxide_parse import OxideParseError, T, compile, engines, follow  # noqa: E402

HTML = '<h1> Shop </h1><div class="p"><b>A</b><i>$1,299.50</i></div><div class="p"><b>B</b><i>5</i></div>'


def test_extract():
    plan = compile({"title": T.css("h1").text().strip(), "items": T.css(".p").map({"n": T.css("b").text(), "p": T.css("i").text().float()})})
    assert plan.extract(HTML) == {"title": "Shop", "items": [{"n": "A", "p": 1299.5}, {"n": "B", "p": 5.0}]}
    assert plan.extract_many([HTML, HTML])[1]["items"][0]["n"] == "A"


def _follow_plan():
    return compile({"x": T.css(".p").map(follow(T.css("b").text(), {"t": T.meta("k"), "h": T.css("h2").text()}, meta={"k": T.css("i").text()}))})


def test_follow_sync_and_async():
    site = {"A": "<h2>pa</h2>", "B": "<h2>pb</h2>"}
    expected = {"x": [{"t": "$1,299.50", "h": "pa"}, {"t": "5", "h": "pb"}]}
    assert _follow_plan().extract(HTML, fetch=lambda url: site[url]) == expected

    async def afetch(url):
        await asyncio.sleep(0)
        return site[url]

    assert asyncio.run(_follow_plan().aextract(HTML, fetch=afetch)) == expected


def test_json_backend_and_roundtrip():
    plan = compile({"n": T["users"].map(T["name"])}, backend="json")
    assert plan.extract({"users": [{"name": "a"}, {"name": "b"}]}) == {"n": ["a", "b"]}
    from oxide_parse import Plan

    assert Plan.from_json(plan.to_json(), "json").extract('{"users":[{"name":"z"}]}') == {"n": ["z"]}


def test_errors_have_paths():
    plan = compile({"items": T.css(".p").map({"p": T.css("b").text().float()})})
    with pytest.raises(Exception, match=r"\$\.items\[0\]\.p"):
        plan.extract(HTML)


def test_engines_listing():
    names = {e.name for e in engines()}
    assert "json" in names and ({"scraper", "lite"} & names)


def test_unknown_engine_is_an_error():
    with pytest.raises(OxideParseError, match="unknown engine"):
        compile({"x": T.css("a").text()}, backend="nope")


def test_auto_engine_resolves_and_matches():
    plan = compile({"t": T.css("h1").text(), "n": T.css(".p").count()}, backend="auto")
    assert plan.engine in {"stream", "scraper", "lite"}
    assert plan.extract(HTML) == {"t": " Shop ", "n": 2}


def test_xpath_on_dom_engines():
    if not any(e.xpath for e in engines()):
        pytest.skip("no XPath-capable engine compiled in")
    plan = compile({"names": T.xpath("//div[@class='p']/b/text()"), "n": T.xpath("count(//div)")})
    assert plan.extract(HTML) == {"names": ["A", "B"], "n": 2}


def test_adaptive_selector_heals(tmp_path):
    v1 = '<section><article class="product" id="p1"><h3>Product 1</h3><p class="description">Nice lamp</p></article></section>'
    v2 = '<div class="w"><article class="product new" data-id="p1"><div class="info"><h3>Product 1</h3><p class="new-desc">Nice lamp</p></div></article></div>'
    schema = {"title": T.css("#p1 h3", adaptive="title").text(), "desc": T.css("#p1 .description", adaptive="desc").text()}
    store = tmp_path / "fp.json"
    first = compile(schema, adaptive_store=store)
    assert first.extract(v1) == {"title": "Product 1", "desc": "Nice lamp"}
    # a new process / plan reading the same store file
    second = compile(schema, adaptive_store=store)
    assert second.extract(v2) == {"title": "Product 1", "desc": "Nice lamp"}
    # without a store that saw v1 there is nothing to relocate
    assert compile(schema).extract(v2) == {"title": None, "desc": None}


def test_audit_regressions_python_level():
    page = (
        '<div><button class="buy" disabled>Buy</button><span class="price">   </span>'
        '<script id="d" type="application/json">{"props": {"price": 1990}}</script>'
        '<p>one<br>two</p><article>A <div class="ad">AD</div>B<script>alert(1)</script></article>'
        '<i class="n">19,99 \u20ac</i><i class="m">12 990 \u20bd</i><i class="id">9007199254740993</i></div>'
    )
    plan = compile({
        "disabled": T.css("button.buy").has_attr("disabled"),
        "missing": T.css("button.buy").has_attr("hidden"),
        "price": T.css(".price").text().non_empty().default("on request"),
        "json_price": T.css("script#d").text().parse_json()["props"]["price"],
        "joined": T.css("p").text(sep=" ", strip=True),
        "clean": T.css("article").text(sep=" ", strip=True, without=".ad"),
        "plain": T.css("article").text(),
        "eur": T.css("i.n").text().float(),
        "rub": T.css("i.m").text().int(),
        "big": T.css("i.id").text().int(),
    })
    assert plan.extract(page) == {
        "disabled": True, "missing": False, "price": "on request", "json_price": 1990,
        "joined": "one two", "clean": "A B", "plain": "A ADB", "eur": 19.99, "rub": 12990, "big": 9007199254740993,
    }


def test_ops_after_follow_run_on_the_fetched_result():
    plan = compile({"t": T.follow(request=T.css("a").attr("href"), schema=T.css("h1").text()).strip().lower()})
    assert plan.extract('<a href="/x">x</a>', fetch=lambda url: "<h1>  HELLO </h1>") == {"t": "hello"}


def _engine_names():
    return {e.name for e in engines()}


def test_stage2_on_error_detailed_url_js_json_sections_kv():
    html = (
        '<div class="p"><b>1</b></div><div class="p"><b>oops</b></div><div class="p"><b>3</b></div>'
        '<a class="lnk" href="../img/a.png">i</a>'
        '<script>window.__STATE__ = {"price": 1990};</script>'
        '<div class="specs"><h3>Size</h3><p>10</p><h3>Color</h3><p>red</p></div>'
        '<table><tr><th>Weight:</th><td> 5 kg </td></tr><tr><th>Color</th><td>red</td></tr></table>'
    )
    plan = compile({
        "v": T.css(".p").map(T.css("b").text().int(), on_error="skip"),
        "link": T.css("a.lnk").attr("href").url(),
        "price": T.css("script").text().js_json("window.__STATE__")["price"],
        "specs": T.css(".specs").sections("h3"),
        "weight": T.kv("Weight").text().strip(),
    })
    det = plan.extract_detailed(html, meta={"base_url": "https://shop.example/cat/x.html"})
    assert det.data == {
        "v": [1, 3],
        "link": "https://shop.example/img/a.png",
        "price": 1990,
        "specs": [{"header": "Size", "items": ["10"]}, {"header": "Color", "items": ["red"]}],
        "weight": "5 kg",
    }
    assert [e["path"] for e in det.errors] == ["$.v[1]"] and "int" in det.errors[0]["message"]
    assert det.misses == []


def test_v8_on_error_is_streamed_and_follow_has_no_url_argument():
    html = '<div class="p"><b>1</b></div><div class="p"><b>oops</b></div><div class="p"><b>3</b></div>'
    schema = {"v": T.css(".p").map(T.css("b").text().int(), on_error="skip")}
    plan = compile(schema, backend="auto")
    if "stream" in _engine_names():
        assert plan.engine == "stream"
    det = plan.extract_detailed(html)
    assert det.data == {"v": [1, 3]}
    assert [e["path"] for e in det.errors] == ["$.v[1]"]
    with pytest.raises(TypeError):
        T.follow(url=T.attr("href"), schema={"x": T.text()})  # type: ignore[call-arg]


def test_stage2_has_contains_and_distill_need_lite():
    if "lite" not in _engine_names():
        pytest.skip("build with --features lite")
    from oxide_parse import distill

    html = "<ul><li><b>x</b></li><li>hello world</li></ul>"
    plan = compile({"a": T.css("li:has(b)").count(), "b": T.css("li:contains('hello')").count()}, backend="lite")
    assert plan.extract(html) == {"a": 1, "b": 1}
    out = distill("<div><script>x</script>" + "<p class='i'>t</p>" * 6 + "</div>")
    assert "script" not in out and "+4 similar" in out


def test_stage2_batch_and_repair():
    plan = compile({"n": T.css("b").text().int()})
    assert plan.extract_batch(["<b>1</b>", "<b>2</b>"], threads=2) == [{"n": 1}, {"n": 2}]
    data, failed = plan.extract_batch(["<b>1</b>", "<b>x</b>"], errors="collect")
    assert data == [{"n": 1}, None] and failed[0]["index"] == 1
    with pytest.raises(OxideParseError):
        plan.extract_batch(["<b>x</b>"])

    html = '<div><span class="cost">$5</span></div>'
    broken = compile({"price": T.css(".gone", adaptive="price").text()})
    asked = []

    def llm(request):
        asked.append(request)
        return {"price": ".cost"}

    res = broken.extract_or_repair(html, llm)
    assert res.repaired and res.data == {"price": "$5"} and res.plan is not broken
    assert asked[0]["misses"][0]["key"] == "price" and asked[0]["misses"][0]["css"] == ".gone"
    with pytest.raises(KeyError):
        broken.with_selectors({"nope": ".x"})
    assert compile({"p": T.css(".cost").text()}).extract_or_repair(html, llm).repaired is False


def test_follow_request_kwargs_and_meta():
    plan = compile(follow(
        request={"url": T.css("a").attr("href"), "headers": {"Accept": "text/html"}, "method": "GET"},
        schema={"title": T.css("h1").text(), "source": T.meta("source")},
        meta={"source": "catalog"},
    ))
    received = []

    def fetch(**request):
        received.append(request)
        return "<h1>Product</h1>"

    async def afetch(**request):
        return fetch(**request)

    expected = {"title": "Product", "source": "catalog"}
    html = '<a href="/product">Product</a>'
    assert plan.extract(html, fetch=fetch) == expected
    assert asyncio.run(plan.aextract(html, fetch=afetch)) == expected
    assert received == [{"url": "/product", "headers": {"Accept": "text/html"}, "method": "GET"}] * 2
    assert plan.extract("<p>No link</p>", fetch=fetch) is None
    assert len(received) == 2


def test_kv_is_scoped_to_the_current_element():
    html = (
        '<div class="card"><table><tr><th>Weight</th><td>1 kg</td></tr></table></div>'
        '<div class="card"><table><tr><th>Weight</th><td>2 kg</td></tr></table></div>'
    )
    plan = compile({"w": T.css(".card").map(T.kv("Weight").text().strip())})
    assert plan.extract(html) == {"w": ["1 kg", "2 kg"]}


def test_json_navigation_keeps_working_after_the_move_optimisation():
    html = '<script id="d">{"props": {"page": {"items": [{"sku": "a"}, {"sku": "b"}], "cat": "E"}}}</script>'
    plan = compile({
        "cat": T.css("#d").text().parse_json()["props"]["page"]["cat"],
        "skus": T.css("#d").text().parse_json()["props"]["page"]["items"].map(T["sku"]),
        "again": T.css("#d").text().parse_json()["props"]["page"]["items"][1]["sku"],
    })
    assert plan.extract(html) == {"cat": "E", "skus": ["a", "b"], "again": "b"}


def test_v8_1_markdown_and_br_and_scope():
    from oxide_parse import markdown

    html = '<body><article><h1>Hi</h1><p>One <b>two</b><br>three</p><div class="ad">BUY</div></article></body>'
    md = markdown(html, "article", without=".ad")
    assert md == "# Hi\n\nOne **two**\nthree"
    assert compile({"m": T.css("article").markdown()}).extract(html)["m"].startswith("# Hi")

    spec = compile({"t": T.css("p").text(" ", strip=True, br="\n")})
    assert spec.extract(html) == {"t": "One two\nthree"}

    if "lite" in _engine_names():
        card = '<div class="h"><div class="c"><span class="t">A</span></div></div>'
        plan = compile({"v": T.css(".c").map({"outer": T.css(".h .t").text(), "own": T.css(":scope > .t").text()})}, backend="lite")
        assert plan.extract(card) == {"v": [{"outer": "A", "own": "A"}]}
        # `auto` falls back to lite for :scope, scraper refuses it
        assert compile({"v": T.css(":scope .t").text()}, backend="auto").extract(card)["v"] == "A"


def test_v8_1_adaptive_list_relocation_and_samples():
    before = '<div id="s"><div class="product"><h2>A</h2><i>1</i></div><div class="product"><h2>B</h2><i>2</i></div></div>'
    after = '<div id="s"><div class="item-x"><h2>A</h2><i>1</i></div><div class="item-x"><h2>B</h2><i>2</i></div></div>'
    plan = compile({"c": T.css(".product", adaptive="cards").map({"n": T.css("h2").text(), "p": T.css("i").text()})})
    first = plan.extract(before)
    assert len(first["c"]) == 2
    assert plan.extract(after) == first


def test_v8_2_store_export_scope_in_stream_and_base_href():
    before = '<div id="s"><div class="product"><h2>A</h2></div><div class="product"><h2>B</h2></div></div>'
    after = '<div id="s"><div class="item-x"><h2>A</h2></div><div class="item-x"><h2>B</h2></div></div>'
    schema = {"c": T.css(".product", adaptive="cards").map({"n": T.css("h2").text()})}
    first = compile(schema, backend="lite")
    expected = first.extract(before)
    state = first.export_adaptive_store()
    assert '"cards"' in state
    # a new plan (another process) starts from the exported state: no file needed
    again = compile(schema, backend="lite", adaptive_state=state)
    assert again.extract(after) == expected
    other = compile(schema, backend="lite")
    assert other.import_adaptive_store(state) == 1

    base = '<head><base href="https://s.example/c/"></head><a href="x">1</a>'
    assert compile({"u": T.css("a").attr("href").url()}).extract(base) == {"u": "https://s.example/c/x"}
    # an absolute <base href> wins over the page URL; a relative one is resolved against it
    assert compile({"u": T.css("a").attr("href").url()}).extract(base, meta={"base_url": "https://o.example/"}) == {"u": "https://s.example/c/x"}
    rel = '<head><base href="/shop/"></head><a href="123">x</a>'
    assert compile({"u": T.css("a").attr("href").url()}).extract(rel, meta={"base_url": "https://site.com/catalog/item"}) == {"u": "https://site.com/shop/123"}

    if "stream" in _engine_names():
        card = '<div class="c"><span class="t">A</span><b><span class="t">deep</span></b></div>'
        plan = compile({"v": T.css(".c").map({"t": T.css(":scope > .t").text()})}, backend="auto")
        assert plan.engine == "stream"
        assert plan.extract(card) == {"v": [{"t": "A"}]}


def test_v8_2_with_selectors_forgets_only_the_rewritten_keys():
    schema = {"a": T.css(".a", adaptive="a").text(), "b": T.css(".b", adaptive="b").text()}
    plan = compile(schema, backend="lite")
    plan.extract('<i class="a">1</i><i class="b">2</i>')
    fixed = plan.with_selectors({"a": ".a2"})
    import json

    keys = set(json.loads(fixed.export_adaptive_store()))
    assert keys == {"b"}


def test_v8_3_group_by_json_ld_meta_tags_and_streamed_without():
    html = (
        '<div class="s"><h3>A</h3><a href="/1">1</a><a href="/2">2</a><h3>B</h3><p><a href="/3">3</a></p></div>'
        '<script type="application/ld+json">{"@graph":[{"@type":"Product","name":"P"}]}</script>'
        '<meta property="og:title" content="T"><meta name="twitter:card" content="c"><meta name="x" content="y">'
    )
    plan = compile(
        {
            "groups": T.css(".s").group_by("h3").map({"title": T.first().text(), "links": T.css("a").map(T.attr("href"))}),
            "name": T.json_ld("Product")[0]["name"],
            "meta": T.meta_tags(),
            "all_meta": T.meta_tags([]),
        }
    )
    out = plan.extract(html)
    assert out["groups"] == [{"title": "A", "links": ["/1", "/2"]}, {"title": "B", "links": ["/3"]}]
    assert out["name"] == "P"
    assert out["meta"] == {"og:title": "T", "twitter:card": "c"}
    assert out["all_meta"]["x"] == "y"

    article = '<article>Hi <span class="ad">BUY</span><b class="badge">NEW</b> there<script>x</script></article>'
    cleaned = compile({"t": T.css("article").text(" ", strip=True, without=".ad, .badge")}, backend="auto")
    assert cleaned.extract(article) == {"t": "Hi there"}
    if "stream" in _engine_names():
        assert cleaned.engine == "stream"


GOOD_PAGE = '<aside class="side">ads</aside><div id="shop" class="catalog"><span class="price">$5</span></div>'
BROKEN_PAGE = '<aside class="side">ads</aside><div id="shop" class="catalog"><section class="cost-box"><em>5 USD</em></section></div>'


def _price_plan():
    plan = compile({"price": T.css(".price", adaptive="price", threshold=0.99).text()}, backend="lite")
    plan.extract(GOOD_PAGE)  # learns the reference
    return plan


def test_v8_4_one_session_loop_for_fetch_and_repair():
    plan = _price_plan()
    sess = plan.start(BROKEN_PAGE)
    (req,) = sess.requests()
    assert req["kind"] == "repair" and req["key"] == "price" and req["scope"] == "container"
    assert "cost-box" in req["skeleton"] and "side" not in req["skeleton"]
    assert req["fingerprint"]["samples"] == ["$5"]
    assert sess.done  # repairs are optional
    sess.resume([(req["id"], {"price": "em"})])
    assert sess.requests() == []
    assert sess.overrides == {"price": "em"}
    assert sess.result() == {"price": "5 USD"}


def test_v8_4_driver_callbacks_attempt_limit_and_extract_or_repair():
    plan = _price_plan()
    det = plan.extract_detailed(BROKEN_PAGE, repair=lambda r: {r["key"]: "em"})
    assert det.data == {"price": "5 USD"} and det.repairs == {"price": "em"} and not det.misses

    stubborn = _price_plan()
    calls = []
    assert stubborn.extract(BROKEN_PAGE, repair=lambda r: calls.append(1) or {"price": ".nope"}) == {"price": None}
    assert len(calls) == 2  # then the repair is declined instead of looping

    result = _price_plan().extract_or_repair(BROKEN_PAGE, lambda r: {r["key"]: "em"})
    assert result.repaired and result.data == {"price": "5 USD"}
    assert result.plan.extract(BROKEN_PAGE) == {"price": "5 USD"}


def test_v8_4_followed_documents_and_async_loop():
    import asyncio

    sub = {"price": T.css(".price", adaptive="price", threshold=0.99).text()}
    plan = compile({"items": T.css("a").map(T.follow(T.attr("href"), sub))}, backend="lite")
    pages = {"/p1": '<div class="price">$5</div>', "/p2": '<div class="cost">7</div>'}
    seen = []

    async def fetch(url, **_):
        return pages[url]

    async def repair(req):
        seen.append((req["url"], req["key"]))
        return {req["key"]: ".cost"}

    out = asyncio.run(plan.aextract('<a href="/p1">1</a><a href="/p2">2</a>', fetch=fetch, repair=repair))
    assert out == {"items": [{"price": "$5"}, {"price": "7"}]}
    assert seen == [("/p2", "price")]


def test_v8_4_invalid_css_batches_and_meta_values():
    # a selector the parser rejects must not raise out of the driver: it is retried, then declined
    plan = _price_plan()
    seen = []

    def bad(r):
        seen.append(r.get("previous_error"))
        return {"price": "[[["}

    assert plan.extract(BROKEN_PAGE, repair=bad) == {"price": None}
    assert seen[0] is None and seen[1] and "price" in seen[1]

    # two fields that broke together are one batch
    schema = {"a": T.css(".a", adaptive="a", threshold=0.99).text(), "b": T.css(".b", adaptive="b", threshold=0.99).text()}
    p2 = compile(schema, backend="lite")
    p2.extract('<div id="c" class="x"><i class="a">1</i><i class="b">2</i></div>')
    batches = []

    def batch(b):
        batches.append(b)
        return {f["key"]: {"a": "u", "b": "s"}[f["key"]] for f in b["fields"]}

    out = p2.extract('<div id="c" class="x"><u>1</u><s>2</s></div>', repair_batch=batch)
    assert out == {"a": "1", "b": "2"}
    assert len(batches) == 1 and {f["key"] for f in batches[0]["fields"]} == {"a", "b"}

    html = '<meta property="og:image" content="a.png"><meta property="og:image" content="b.png">'
    assert compile({"m": T.meta_tags(values="first")}).extract(html) == {"m": {"og:image": "a.png"}}
    assert compile({"m": T.meta_tags()}).extract(html)["m"]["og:image"] == ["a.png", "b.png"]


def test_v8_4_repair_reuses_duplicate_follow_requests():
    plan = compile({
        "price": T.css(".price", adaptive="price", threshold=0.99).text(),
        "items": T.css("a").map(T.follow(T.attr("href"), T.css("h1").text())),
    }, backend="lite")
    plan.extract(GOOD_PAGE)
    sess = plan.start(BROKEN_PAGE + '<a href="/same">one</a><a href="/same">two</a>')
    first, second = sess.pending()
    sess.resume([(first["id"], "<h1>first result</h1>")])
    repair, = sess.repairs()
    sess.resume([(repair["id"], {"price": "em"})])
    assert [r["id"] for r in sess.pending()] == [second["id"]]
    sess.resume([(second["id"], "<h1>second result</h1>")])
    assert sess.result() == {"price": "5 USD", "items": ["first result", "second result"]}


def test_v8_5_with_selectors_forgets_rewritten_keys_on_disk(tmp_path):
    import json

    store = str(tmp_path / "fingerprints.json")
    schema = {
        "a": T.css(".a", adaptive="a").text(),
        "b": T.css(".b", adaptive="b").text(),
    }
    plan = compile(schema, backend="lite", adaptive_store=store)
    plan.extract('<i class="a">old</i><i class="b">keep</i>')
    saved = json.loads(plan.export_adaptive_store())
    plan.import_adaptive_store(json.dumps({"a#healed": saved["a"]}))
    patched = plan.with_selectors({"a": ".new"})
    assert json.loads(compile(schema, backend="lite", adaptive_store=store).export_adaptive_store()) == {
        "b": saved["b"],
    }
    assert patched.extract('<i class="new">new</i><i class="b">keep</i>') == {"a": "new", "b": "keep"}
    assert set(json.loads(compile(schema, backend="lite", adaptive_store=store).export_adaptive_store())) == {"a", "b"}


def test_batch_and_many_single_pass_iterators():
    plan = compile({"n": T.css("b").text().int()})
    docs = ["<b>1</b>", b"<b>2</b>", bytearray(b"<b>3</b>")]
    expected = [{"n": 1}, {"n": 2}, {"n": 3}]

    assert plan.extract_many(d for d in docs) == expected
    assert plan.extract_many(iter(docs)) == expected
    assert plan.extract_batch((d for d in docs), threads=2) == expected
    data, failed = plan.extract_batch(iter(["<b>1</b>", "<b>x</b>"]), errors="collect")
    assert data == [{"n": 1}, None] and [f["index"] for f in failed] == [1]

    class OneShotIterable:
        def __init__(self, items):
            self._items = items
            self._used = False

        def __iter__(self):
            assert not self._used, "__iter__ called more than once"
            self._used = True
            return iter(self._items)

    class OneShotListSubclass(list):
        def __init__(self, items):
            super().__init__(items)
            self._used = False

        def __iter__(self):
            assert not self._used, "list subclass __iter__ called more than once"
            self._used = True
            return super().__iter__()

    assert plan.extract_many(OneShotIterable(docs)) == expected
    assert plan.extract_batch(OneShotIterable(docs)) == expected
    assert plan.extract_many(OneShotListSubclass(docs)) == expected
    assert plan.extract_batch(OneShotListSubclass(docs)) == expected

    with pytest.raises(TypeError):
        plan.extract_many(iter(["<b>1</b>", None]))
    with pytest.raises(TypeError):
        plan.extract_batch(iter(["<b>1</b>", None]))
    with pytest.raises(TypeError):
        plan.extract_many(["<b>1</b>", None])
    with pytest.raises(TypeError):
        plan.extract_batch(["<b>1</b>", None])

    assert plan.extract_many(("<b>1</b>", b"<b>2</b>")) == [{"n": 1}, {"n": 2}]
    assert plan.extract_batch(("<b>1</b>", b"<b>2</b>")) == [{"n": 1}, {"n": 2}]

    jplan = compile({"n": T["n"]}, backend="json")
    jexpected = [{"n": 1}, {"n": 2}]
    assert jplan.extract_many(d for d in [{"n": 1}, {"n": 2}]) == jexpected
    assert jplan.extract_batch(d for d in [{"n": 1}, {"n": 2}]) == jexpected
    assert jplan.extract_many(d for d in ['{"n": 1}', '{"n": 2}']) == jexpected
    assert jplan.extract_batch(d for d in ['{"n": 1}', {"n": 2}]) == jexpected
    assert jplan.extract_many(['{"n": 1}', {"n": 2}]) == jexpected
    assert jplan.extract_batch(['{"n": 1}', {"n": 2}]) == jexpected


def test_paginate_friends_limits_cycles_and_json():
    from oxide_parse import paginate

    site = {
        "https://social.test/u/42": '<a class="friends" href="/u/42/friends">Friends</a>',
        "https://social.test/u/42/friends": (
            '<div class="friend">Ann</div><div class="friend">Bob</div>'
            '<a class="next" href="?page=2">Next</a>'
        ),
        "https://social.test/u/42/friends?page=2": (
            '<div class="friend">Cid</div><div class="friend">Dan</div>'
            '<a class="next" href="?page=3">Next</a>'
        ),
        "https://social.test/u/42/friends?page=3": '<div class="friend">Eve</div>',
    }
    schema = {
        "friends": follow(
            T.css("a.friends").attr("href").url(),
            paginate(T.css(".friend").map(T.text()), T.css("a.next").attr("href").url()),
        )
    }
    fetched = []

    def fetch(url, **_):
        fetched.append(url)
        return site[url]

    plan = compile(schema, backend="auto")
    assert plan.engine in {"lite", "scraper"}
    assert plan.extract(
        site["https://social.test/u/42"],
        meta={"base_url": "https://social.test/u/42"},
        fetch=fetch,
    ) == {"friends": ["Ann", "Bob", "Cid", "Dan", "Eve"]}
    assert fetched == [
        "https://social.test/u/42/friends",
        "https://social.test/u/42/friends?page=2",
        "https://social.test/u/42/friends?page=3",
    ]

    # max_items stops without requesting page 3
    fetched.clear()
    lim_plan = compile(
        paginate(T.css(".friend").map(T.text()), T.css("a.next").attr("href").url(), max_items=3)
    )
    assert lim_plan.extract(
        site["https://social.test/u/42/friends"],
        meta={"base_url": "https://social.test/u/42/friends"},
        fetch=fetch,
    ) == ["Ann", "Bob", "Cid"]
    assert fetched == ["https://social.test/u/42/friends?page=2"]

    # max_pages=1 fetches nothing
    fetched.clear()
    assert compile(
        paginate(T.css(".friend").map(T.text()), T.css("a.next").attr("href").url(), max_pages=1)
    ).extract(
        site["https://social.test/u/42/friends"],
        meta={"base_url": "https://social.test/u/42/friends"},
        fetch=fetch,
    ) == ["Ann", "Bob"]
    assert fetched == []

    # cycle detection and `#` stop
    cyc_pages = {
        "https://social.test/c1": '<div class="f">A</div><a class="next" href="/c2">n</a>',
        "https://social.test/c2": '<div class="f">B</div><a class="next" href="/c1">loop</a>',
    }
    cyc_fetched = []
    assert compile(paginate(T.css(".f").map(T.text()), T.css("a.next").attr("href").url())).extract(
        cyc_pages["https://social.test/c1"],
        meta={"base_url": "https://social.test/c1"},
        fetch=lambda url, **_: cyc_fetched.append(url) or cyc_pages[url],
    ) == ["A", "B"]
    assert cyc_fetched == ["https://social.test/c2"]

    # cursor pagination on backend="json"
    api_pages = {
        ("https://api.test/friends", "c1"): '{"users": [{"name": "U1"}, {"name": "U2"}], "next_cursor": "c2"}',
        ("https://api.test/friends", "c2"): '{"users": [{"name": "U3"}], "next_cursor": null}',
    }
    jplan = compile(
        paginate(
            items=T["users"].map(T["name"]),
            next=T.if_(
                T["next_cursor"] != None,  # noqa: E711
                {"url": "https://api.test/friends", "params": {"cursor": T["next_cursor"]}},
                None,
            ),
        ),
        backend="json",
    )
    assert jplan.extract(
        api_pages[("https://api.test/friends", "c1")],
        fetch=lambda url, params, **_: api_pages[(url, params["cursor"])],
    ) == ["U1", "U2", "U3"]

    # extract_detailed with on_error="skip" across paginated pages
    det_pages = {
        "https://e.test/1": '<b>1</b><a class="n" href="/2">n</a>',
        "https://e.test/2": '<b>oops</b><b>3</b>',
    }
    det_plan = compile({
        "nums": paginate(T.css("b").map(T.text().int(), on_error="skip"), T.css("a.n").attr("href").url()),
    })
    det = det_plan.extract_detailed(
        det_pages["https://e.test/1"],
        meta={"base_url": "https://e.test/1"},
        fetch=lambda url, **_: det_pages[url],
    )
    assert det.data == {"nums": [1, 3]}
    assert len(det.errors) == 1 and det.errors[0]["message"].startswith("int:")
