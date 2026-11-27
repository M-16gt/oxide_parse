# Quickstart

Everything you need to write your first schema and understand how the library fits together. Takes about five minutes.

## Installation

```bash
pip install oxide-parse
```

For YAML schema support (`Plan.from_yaml`, `--format yaml` in the CLI): `pip install "oxide-parse[yaml]"`.

From source (requires Rust 1.74+ and Python 3.9+):

```bash
uv sync
uv run maturin develop --release --features full
```

## First Parser

```python
from pprint import pprint
from oxide_parse import T, compile

html = """
<h1> Laptops </h1>
<div class="card">
  <a href="/p/1"><h2>ThinkPad X1</h2></a>
  <span class="price">$1,299.50</span>
</div>
<div class="card">
  <a href="/p/2"><h2>MacBook Air</h2></a>
  <span class="soldout">out of stock</span>
</div>
"""

schema = {
    "title": T.css("h1").text().strip(),
    "items": T.css(".card").map({
        "name": T.css("h2").text(),
        "url": T.css("a").attr("href").url(),
        "price": T.css(".price").text().float(),
    }),
}

plan = compile(schema, backend="auto")
data = plan.extract(html, meta={"base_url": "https://shop.example"})
pprint(data, sort_dicts=False)
```

```text
{'title': 'Laptops',
 'items': [{'name': 'ThinkPad X1',
            'url': 'https://shop.example/p/1',
            'price': 1299.5},
           {'name': 'MacBook Air',
            'url': 'https://shop.example/p/2',
            'price': None}]}
```

What happened here:

- `schema` is a plain Python dict. Its values describe *what* to extract, not imperative loops. `T` stands for the current node: the whole document at the top level, or each `.card` inside `.map(...)`.
- `compile()` validates the schema and turns it into a reusable plan. Compile once, then call `plan.extract()` across as many pages as you need.
- `.float()` parsed `$1,299.50` directly — no manual `.replace("$", "")` needed.
- The second card has no `.price`, so its price is `None` rather than an exception. Missing elements evaluate to `None` by default.
- `.url()` resolved `/p/1` into an absolute URL using `meta={"base_url": ...}` (or `<base href>` in the page). If neither provides a base URL, extraction fails explicitly instead of silently corrupting links.

## Which Engine Was Selected

```python
print(plan.engine)
```

```text
stream
```

`backend="auto"` inspected the schema and verified it can run in a single streaming pass without building a DOM tree. Once a schema needs a DOM (`xpath`, `parent()`, `adaptive`, etc.), `auto` switches to `lite` or `scraper`. See [03_choosing_engine.md](03_choosing_engine.md).

## Handling Malformed Data

Missing elements produce `None`. When an element exists but its content cannot be converted (for example, `.float()` on `"out of stock"`), extraction raises with the exact field path:

```python
bad_html = html.replace('class="soldout"', 'class="price"')

try:
    plan.extract(bad_html, meta={"base_url": "https://shop.example"})
except Exception as e:
    print(e)
```

```text
extraction error at `$.items[1].price`: float: no number in "out of stock"
```

For card lists you usually want to skip or null broken records instead of failing the whole page. Use `on_error`:

```python
schema = {
    "items": T.css(".card").map({
        "name": T.css("h2").text(),
        "price": T.css(".price").text().float(),
    }, on_error="skip"),
}
plan = compile(schema, backend="auto")

result = plan.extract_detailed(bad_html)
print(result.data)
print(result.errors)
```

```text
{'items': [{'name': 'ThinkPad X1', 'price': 1299.5}]}
[{'path': '$.items[1].price', 'message': 'float: no number in "out of stock"'}]
```

Use `on_error="null"` to put `None` in place of the broken record instead of dropping it.

## Saving and Decompiling Schemas

Plans serialize cleanly to JSON (or YAML) and back to runnable Python source — useful for storing schemas in a database or sending generated schemas for code review.

```python
from oxide_parse import Plan

text = plan.to_json()
plan2 = Plan.from_json(text, backend="auto")
print(plan2.to_python())
```

```text
from oxide_parse import T

schema = {
    'items': T.css('.card').map({
        'name': T.css('h2').text(),
        'price': T.css('.price').text().float(),
    }, on_error='skip'),
}
```

## Next Steps

- [02_dsl_cheatsheet.md](02_dsl_cheatsheet.md): every `T` operation on one page.
- [03_choosing_engine.md](03_choosing_engine.md): when engines differ and how to read `plan.explain()`.
- [04_hybrid_html_json.md](04_hybrid_html_json.md): extracting `__NEXT_DATA__`, JSON-LD, and meta tags.
