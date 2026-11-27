# Hybrid HTML and JSON

Modern sites often keep half the fields you want inside `<script id="__NEXT_DATA__">`, `window.__STATE__`, JSON-LD blocks, or `<meta>` tags instead of visible HTML elements. `oxide_parse` lets you extract all of them alongside CSS selectors in a single schema.

```python
from pprint import pprint
from oxide_parse import T, compile

html = """
<html>
<head>
  <meta property="og:title" content="ThinkPad X1">
  <meta property="og:image" content="/img/x1.jpg">
  <script type="application/ld+json">
    {"@context": "https://schema.org", "@type": "Product", "name": "ThinkPad X1", "offers": {"price": "1299.50"}}
  </script>
</head>
<body>
  <h1>ThinkPad X1</h1>
  <script id="__NEXT_DATA__" type="application/json">
    {"props": {"pageProps": {"sku": "X1-2024", "stock": 7}}}
  </script>
  <script id="state">window.__STATE__ = {"cart": {"count": 2}};</script>
</body>
</html>
"""

next_data = T.css("script#__NEXT_DATA__").text().parse_json()["props"]["pageProps"]

schema = {
    "title": T.css("h1").text(),
    "sku": next_data["sku"],
    "stock": next_data["stock"],
    "cart": T.css("script#state").text().js_json("window.__STATE__")["cart"]["count"],
    "og_image": T.meta_tags(values="first")["og:image"].url(),
    "ld_price": T.json_ld("Product")[0]["offers"]["price"].float(),
}

plan = compile(schema, backend="auto")
data = plan.extract(html, meta={"base_url": "https://shop.example"})
pprint(data, sort_dicts=False)
```

```text
{'title': 'ThinkPad X1',
 'sku': 'X1-2024',
 'stock': 7,
 'cart': 2,
 'og_image': 'https://shop.example/img/x1.jpg',
 'ld_price': 1299.5}
```

Removing only `ld_price` from that schema lets the entire remaining extraction run on the single-pass `stream` engine.

## `parse_json()` for Script Payload JSON

`T.css("script#__NEXT_DATA__").text().parse_json()` parses the element's text as JSON, after which `["key"]`, `[0]`, and `.map(...)` work normally.

Why it stays fast:

- The peephole optimizer fuses `.parse_json()["props"]["pageProps"]["sku"]` into a single `json_path` lookup: intermediate objects are walked by reference and only the final value is copied out.
- Parsed JSON strings (512+ bytes) are cached per document run: if three fields read separate branches of `__NEXT_DATA__`, it is parsed once.
- Fully supported in `stream`.

## `js_json()` for JavaScript Assignments

When data sits inside `window.__INITIAL_STATE__ = {...};` or `var data = JSON.parse("...");` rather than raw JSON, use `.js_json("var_name")`:

```python
html_js = '<script id="init">window.__STATE__ = {"user": {"id": 42}};</script>'
schema = {"uid": T.css("script#init").text().js_json("window.__STATE__")["user"]["id"]}
plan = compile(schema, backend="auto")
print(plan.engine, plan.extract(html_js))
```

```text
stream {'uid': 42}
```

Calling `.js_json()` with no argument extracts the first `{...}` or `[...]` literal in the script body. Results are cached per `(node, var_name)` within a document run and work in `stream`.

Common pitfall: writing `T.css("script").text().js_json(...)` on a page whose first `<script>` is analytics or JSON-LD. Always anchor the selector (`script#init`, `script:not([type])`, etc.). Note also that `js_json` only reads JSON literals and `JSON.parse("...")` strings — it does not execute arbitrary JS functions or expressions.

## `T.json_ld()`

Collects every `<script type="application/ld+json">` block on the page, unfolds top-level JSON arrays and `@graph` lists, strips `<!-- ... -->` wrappers, and ignores malformed JSON blocks:

```python
schema = {
    "all": T.json_ld(),
    "price": T.json_ld("Product")[0]["offers"]["price"].float(),
}
data = compile(schema, backend="auto").extract(html)
print(data["price"], len(data["all"]))
```

```text
1299.5 1
```

Filtering by `schema_type="Product"` matches both string and list `@type` values, as well as full URIs such as `https://schema.org/Product`. Requires a DOM engine (`auto` selects `lite` or `scraper`).

## `T.meta_tags()`

Collects `<meta property="..." content="...">` and `<meta name="..." content="...">` tags into a lowercased dictionary:

```python
meta_html = """
<html><head>
  <meta property="og:title" content="Product">
  <meta property="og:image" content="/1.jpg">
  <meta property="og:image" content="/2.jpg">
  <meta name="description" content="Summary">
</head><body></body></html>
"""

schema = {
    "og": T.meta_tags(),
    "first_img": T.meta_tags(values="first")["og:image"].url(),
    "all": T.meta_tags(prefixes=[], values="list"),
}
plan = compile(schema, backend="auto")
data = plan.extract(meta_html, meta={"base_url": "https://shop.example"})
print("engine:", plan.engine)
pprint(data, sort_dicts=False)
```

```text
engine: stream
{'og': {'og:title': 'Product', 'og:image': ['/1.jpg', '/2.jpg']},
 'first_img': 'https://shop.example/1.jpg',
 'all': {'og:title': ['Product'],
         'og:image': ['/1.jpg', '/2.jpg'],
         'description': ['Summary']}}
```

By default `prefixes` is `("og:", "twitter:")`. Pass `prefixes=[]` to keep every `<meta>` tag or pass your own tuple/list such as `("og:", "article:")`.

The `values` mode controls repeated tags like `og:image`:
- `"auto"` (default): single occurrence stays a `str`, multiple occurrences become a `list`.
- `"first"`: always a `str` (the first occurrence) — safe to chain directly into `.url()` or `.strip()`.
- `"list"`: always a `list`.

`T.meta_tags()` runs in `stream` and stops parsing once `<body>` opens. If a site puts `<meta>` tags inside `<body>`, compile with `backend="html"` instead.
