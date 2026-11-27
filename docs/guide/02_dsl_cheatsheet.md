# DSL Cheatsheet (`T`)

Everything you can write in a schema on one page. Full signatures live in [reference/python_api.md](../reference/python_api.md).

```python
from oxide_parse import T, compile, follow, paginate, if_, coalesce
```

## Reading Pipelines

`T` represents the current node or value. `T.css(".card").css("h2").text()` reads left to right: find cards, find headings inside them, read the text. Before `compile()`, a `T` chain only records operations without touching any document.

Four core rules:

- `.text()`, `.attr()`, and `.html()` on a node list read the **first** node. To collect every match, use `.map(...)`.
- When a step finds nothing, `None` flows down the rest of the pipeline without raising. `.default(x)` supplies a fallback; `.required()` turns `None` into an error.
- Arguments to `default`, `if_`, `coalesce`, `eq`, etc. are evaluated against the **current context** (the card inside `map`), not against the left-hand value.
- Never use Python `and`/`or`/`not`/`if` inside schemas (`bool(expr)` raises `TypeError`). Use `&`, `|`, `~`, and `if_()`.

## Navigation

| Task | Expression |
|---|---|
| All CSS matches | `T.css("a.item")` |
| XPath 1.0 | `T.xpath("//h1")` (DOM engines only) |
| First / last match | `.first()`, `.last()` |
| By index | `T.css("li")[0]`, `.at(1)` (or `.last()` for the final item) |
| Slices | `T[1:]`, `T[:5]`, `.slice(1, 4)`, `.rest()` (everything after first) |
| Tree traversal | `.parent()`, `.children()`, `.root()` |
| JSON key | `T["props"]["items"]`, `.key("props")` |
| Direct children of card | `T.css(":scope > .title")` |

```python
html = "<ul><li>1</li><li>2</li><li>3</li><li>4</li></ul>"
schema = {
    "middle": T.css("li").slice(1, 3).map(T.text()),
    "last": T.css("li")[-1:].map(T.text()),
}
print(compile(schema, backend="auto").extract(html))
```

```text
{'middle': ['2', '3'], 'last': ['4']}
```

## Extraction

| Task | Expression |
|---|---|
| Text | `.text()` (`<script>`/`<style>` descendants skipped automatically) |
| Joined & trimmed text | `.text(" ", strip=True)` |
| Excluding subtrees | `.text(" ", strip=True, without=".ad, .share")` |
| `<br>` as newline | `.text(" ", strip=True, br="\n")` |
| Attribute | `.attr("href")` |
| Attribute presence | `.has_attr("disabled")` (handles valueless flags like `<button disabled>`) |
| HTML | `.html()`, `.inner_html()` |
| Markdown | `.markdown(without=".ad")` (DOM engines) |
| Prompt-sized skeleton | `.distill(max_chars=8000)` |
| Item index in `map` | `T.index()` |
| Count / presence | `.count()`, `.exists()` |
| Metadata value | `T.meta("title")` |

```python
html = '<div class="d">Screen: OLED<br>Refresh: 120 Hz<span class="ad">promo</span></div>'
schema = {"spec": T.css(".d").text(" ", strip=True, br="\n", without=".ad")}
print(compile(schema, backend="auto").extract(html))
```

```text
{'spec': 'Screen: OLED\nRefresh: 120 Hz'}
```

## Strings & Types

| Task | Expression |
|---|---|
| Trim & case | `.strip()`, `.lower()`, `.upper()` |
| Replace | `.replace("old", "new")` |
| Split / join | `.split(",")`, `.join(", ")` |
| Regex | `.regex(r"(\d+)", 1)`, `.regex_all(r"\d+")` |
| Numbers | `.int()`, `.float()` |
| Booleans | `.bool()` |
| Resolve relative URL | `.url()` or `.url("https://base/")` |

`.int()` and `.float()` handle currency symbols and localized separators out of the box: `"$1,299.50"` -> `1299.5`, `"19,99 €"` -> `19.99`, `"12 990 ₽"` -> `12990`, `"1.299,50"` -> `1299.5`. A single comma followed by three digits (`1,299`) is treated as a thousands separator unless the integer part is `0` or longer than three digits, and `"Model-3"` does not read `-3`. If no number appears in the text, conversion raises an extraction error rather than returning `None`.

## Nulls & Fallbacks

| Task | Expression |
|---|---|
| Default value | `.default("—")` |
| Required value | `.required()` |
| Treat blanks/empty as missing | `.non_empty().default("—")` |
| First non-missing alternative | `coalesce(T.attr("title"), T.text())` |

```python
html = "<p></p>"
schema = {
    "price": T.css(".price").text().float(),
    "label": T.css(".label").text().default("—"),
}
print(compile(schema).extract(html))
```

```text
{'price': None, 'label': '—'}
```

## Collections & Logic

| Task | Expression |
|---|---|
| Map items | `.map({...})`, `.map(T.text())` |
| Quarantine bad records | `.map({...}, on_error="skip")` or `"null"` |
| Flatten nested lists | `.flat_map(...)` |
| Filter | `.filter(T.attr("data-on") == "1")` |
| Comparisons | `==`, `!=`, `.contains()`, `.startswith()`, `.endswith()` |
| Boolean logic | `&`, `\|`, `~` |
| Conditional | `if_(cond, then, otherwise)` |

```python
html = '''<ul>
  <li data-on="1" data-hot>A</li>
  <li data-on="0">B</li>
  <li data-on="1" title="Featured">C</li>
</ul>'''
schema = {
    "items": T.css("li").filter(T.attr("data-on") == "1").map({
        "name": T.text(),
        "hot": if_(T.has_attr("data-hot"), "yes", "no"),
        "label": coalesce(T.attr("title"), T.text()),
    }),
}
print(compile(schema).extract(html))
```

```text
{'items': [{'name': 'A', 'hot': 'yes', 'label': 'A'}, {'name': 'C', 'hot': 'no', 'label': 'Featured'}]}
```

## Flat Markup

Pages often dump sections sequentially inside one container: a heading, a few paragraphs, another heading. Without wrapper elements per section, use `group_by`:

```python
html = '''<div class="specs">
  <h3>Display</h3><p>14"</p><p>OLED</p>
  <h3>Memory</h3><p>32 GB</p>
</div>'''
schema = {
    "specs": T.css(".specs").group_by("h3").map({
        "section": T.first().text(),
        "items": T.rest().map(T.text()),
    }),
}
print(compile(schema, backend="auto").extract(html))
```

```text
{'specs': [{'section': 'Display', 'items': ['14"', 'OLED']}, {'section': 'Memory', 'items': ['32 GB']}]}
```

Inside `group_by(...).map(...)`, the current node list is the group itself: `T.first()` is the header and `T.rest()` holds everything after it. If you only need `{"header": ..., "items": [...]}` pairs, `T.css(".specs").sections("h3", items="p")` is even shorter.

For label/value tables, `T.kv("Weight")` finds a `th`/`dt`/`td`/`label`/`strong`/`b`/`span`/`div` matching that label and returns its following sibling element:

```python
html = "<table><tr><th>Weight</th><td>1.1 kg</td></tr></table>"
print(compile({"weight": T.kv("Weight").text().float()}).extract(html))
```

```text
{'weight': 1.1}
```

## Hybrid Data & Sub-queries

More details live in dedicated guides:

| Pattern | Expression | Guide |
|---|---|---|
| JSON inside `<script>` | `T.css("script#__NEXT_DATA__").text().parse_json()["props"]` | [04](04_hybrid_html_json.md) |
| JS variable JSON | `.js_json("window.__STATE__")["cart"]` | [04](04_hybrid_html_json.md) |
| JSON-LD | `T.json_ld("Product")[0]["name"]` | [04](04_hybrid_html_json.md) |
| OpenGraph / Twitter | `T.meta_tags()`, `T.meta_tags(values="first")["og:image"]` | [04](04_hybrid_html_json.md) |
| Self-healing selector | `T.css(".price", adaptive="price")` | [05](05_adaptive_and_repair.md) |
| Linked page sub-query | `follow(request=..., schema=..., meta=...)` | [06](06_follow_and_crawling.md) |
| `next`-page pagination | `paginate(items=..., next=..., max_pages=..., max_items=...)` | [06](06_follow_and_crawling.md) |
