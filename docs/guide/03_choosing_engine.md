# Choosing an Engine

Short version: use `backend="auto"` unless you have a concrete reason not to. Below is how `auto` picks an engine and when specifying one explicitly makes sense.

## Engines

| `backend=` | Kind | When to use | XPath | `adaptive` | `:scope` |
|---|---|---|---|---|---|
| `auto` | automatic | default choice for most schemas | via DOM | via DOM | yes |
| `stream` | single pass, no DOM | catalogs, metadata, large pages | no | no | yes |
| `lite` | arena DOM | anything non-streamable, `:contains()` | yes | yes | yes |
| `scraper` | html5ever DOM | broken HTML needing full HTML5 tree rules | yes | yes | no |
| `xml` | XML arena DOM | Sitemaps, RSS/Atom feeds, CDATA, XML namespaces | yes | yes | yes |
| `json` | data | running schemas directly over JSON | no | no | no |

There is also `html`: the best compiled-in DOM engine (`scraper` first, then `lite`). That is `compile()`'s default when `backend` is omitted.

When `backend="auto"` compiles a schema, it tries `stream` first and falls back to `html` if anything requires a DOM (and `html` falls back to `lite` if the schema uses `:scope`, `:disabled`, `:enabled`, `:checked`, or `:any-link`, which `scraper` rejects).

## Inspecting the Plan

```python
from oxide_parse import T, compile

schema = {
    "title": T.css("h1").text().strip(),
    "links": T.css("a").map(T.attr("href")),
}
plan = compile(schema, backend="auto")
print(plan.engine)
print(plan.explain())
```

```text
stream
stream program (single pass, no DOM):
  title: css "h1" #0 -> text (+1 value ops)
  links: records of css "a" #1
      : item -> attr("href") (+0 value ops)

Plan (backend: lite)
selectors: 2 unique
  #0 css "h1"
  #1 css "a"
follow sub-plans: 0
optimisations applied: 1
  - select|text/attr/html -> select_first|... (early exit)
root:
  object
    title:
      select_first[css "h1" #0] | text | strip
    links:
      select[css "a" #1] | map
        body:
          attr("href")
```

The top block shows the streaming capture program: which selectors are tracked during the byte pass and what post-processing runs on each captured value. Below it sits the compiled plan representation with deduplicated selectors and peephole rewrites (e.g., `select | text` rewritten to `select_first` so matching stops at the first element).

## What Forces a Fallback Out of `stream`

`stream` only keeps open tag frames and collected values in memory, so anything requiring backward or upward DOM inspection cannot run there:

- `xpath()`, `parent()`, `root()`, `filter()`, `kv()`;
- `adaptive=` selectors;
- `follow(...)`, `paginate(...)`, nested object schemas inside fields;
- CSS features: sibling combinators `+` and `~`, `:last-child`, `:only-child`, `:nth-last-child()`, `:nth-last-of-type()`, `:last-of-type`, `:only-of-type`, `:empty`, `:has()`, `:contains()`;
- `T.json_ld()`, `T.sections()`, `T.group_by()`, `.markdown()`, `.distill()`.

What *does* stream: `css(...).text()/attr()/has_attr()/exists()/count()`, card lists `css(".card").map({...})` using those captures, `text(sep, strip, br, without)`, `:scope`, `.url()`, `.parse_json()`, `.js_json()`, `T.meta_tags()`, `on_error`, and all pure value operations (`strip`, `float`, `regex`, `slice`, `default`, etc.).

Passing `backend="stream"` explicitly when a schema cannot stream raises an error telling you which operation forced DOM:

```python
try:
    compile({"n": T.css("li:has(> a)").count()}, backend="stream")
except Exception as e:
    print(e)
```

```text
compile error at `$`: schema cannot be streamed: selector "li:has(> a)" uses :has / :contains / :last-child / :only-child / :empty
```

With `backend="auto"`, the same schema switches silently to a DOM engine:

```python
plan = compile({"title": T.xpath("//h1").text()}, backend="auto")
print(plan.engine)
```

```text
scraper
```

## Early Exit

`stream` stops scanning the HTML byte stream as soon as every single-value field has been resolved. The biggest win is `<head>` metadata: `T.meta_tags()` lives in `<head>`, so streaming stops as soon as `<body>` opens. On an 875 KiB page, reading `og:title` with `auto` finished in 1.4 ms vs 82 ms for a full DOM build (v8.5 measurement).

If you add a page-wide `.map(...)` list to the same schema, `stream` must read through the end of the document to collect every card.

## When to Pick an Engine Explicitly

- **`:contains()`**: implemented in `lite` only (`scraper` rejects it; pass `backend="lite"`). Avoid bare `*:contains("...")` because candidate text is scanned per match — narrow with a tag or class such as `li:contains("Sale")`.
- **`:has()`**: supported in both `lite` and `scraper`.
- **Malformed HTML**: broken tables or unclosed tags needing full HTML5 adoption agency and foster parenting rules. `scraper` runs full `html5ever`; `lite` uses a fast arena builder covering common HTML recovery rules.
- **Benchmarks**: pass the engine name explicitly so runs compare apples to apples.

## Build-Time Engine Features

Engines are compiled via Cargo feature flags; PyPI wheels enable `--features full`.

```python
import oxide_parse

for e in oxide_parse.engines():
    print(e.name, e.kind, e.xpath)
```

```text
scraper dom True
lite dom True
stream stream False
json data False
xml dom True
```

Building with a custom subset:

```bash
uv run maturin develop --release --features full                                      # all engines
uv run maturin develop --release --no-default-features --features "lite,stream,json"  # without html5ever
```

Requesting an excluded engine raises an error naming the Cargo feature to enable. Both `distill()` and `.distill()` require `lite`.

## Schemas Over JSON

`backend="json"` runs the same extraction DSL directly over JSON documents using key/index navigation instead of CSS:

```python
schema = {"users": T["users"].map({"name": T["name"], "age": T["age"].int()})}
plan = compile(schema, backend="json")
print(plan.extract({"users": [{"name": "Alice", "age": "31"}]}))
```

```text
{'users': [{'name': 'Alice', 'age': 31}]}
```

Inputs can be JSON strings, UTF-8 `bytes`, or parsed Python objects (`dict` / `list`).
