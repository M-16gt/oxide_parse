# oxide_parse

[![PyPI](https://img.shields.io/pypi/v/oxide-parse)](https://pypi.org/project/oxide-parse/)
[![CI](https://github.com/M-16gt/oxide_parse/actions/workflows/ci.yml/badge.svg)](https://github.com/M-16gt/oxide_parse/actions/workflows/ci.yml)
[![MIT license](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

Declarative HTML and JSON extraction engine in Rust with a Python API: you describe a schema with `T`, compile it to a JSON-serializable IR, and run it in a single streaming pass or over a DOM.

Three things that set it apart:
- **Streaming engine (`stream`) without a DOM:** common schemas (catalogs, metadata, embedded JSON) run in a single pass over bytes, keeping only the open tag stack and extracted values in memory.
- **Schema as serializable IR:** save any plan to JSON or YAML (`to_json()`) and turn it back into plain Python (`to_python()`).
- **Zero-IO `Session`:** the core never performs network requests or LLM calls — it yields URLs (`follow`, `paginate`) and broken selectors (`adaptive` + `repair_batch`) to your sync or async code.

## Installation

```bash
pip install oxide-parse
```

From source (Rust 1.74+ and Python 3.9+):

```bash
uv sync && uv run maturin develop --release --features full
```

## Quick Example

```python
from oxide_parse import T, compile

html = """
<html><body>
  <h1>Laptops</h1>
  <script id="__NEXT_DATA__" type="application/json">{"currency": "USD"}</script>
  <div class="card"><h2>ThinkPad X1</h2><span class="price">$1,299.50</span></div>
  <div class="card"><h2>Broken</h2><span class="price">out of stock</span></div>
</body></html>
"""

schema = {
    "title": T.css("h1").text(),
    "currency": T.css("script#__NEXT_DATA__").text().parse_json()["currency"],
    "items": T.css(".card").map({
        "name": T.css("h2").text(),
        "price": T.css(".price").text().float(),
    }, on_error="skip"),
}

plan = compile(schema, backend="auto")
data = plan.extract_detailed(html)
print(plan.engine, data.data, data.errors, sep="\n")
```

```text
stream
{'title': 'Laptops', 'currency': 'USD', 'items': [{'name': 'ThinkPad X1', 'price': 1299.5}]}
[{'path': '$.items[1].price', 'message': 'float: no number in "out of stock"'}]
```

## Choosing an Engine

| `backend=` | Kind | When to use | DOM | XPath | `:scope` | `adaptive` |
|---|---|---|---|---|---|---|
| `auto` | automatic | almost always (`stream`, falls back to DOM when needed) | as needed | yes | yes | yes |
| `stream` | streaming | catalogs, `<head>` metadata, large pages | no | no | yes | no |
| `lite` | arena DOM | complex CSS, `:contains()`, XPath, `distill()` | yes | yes | yes | yes |
| `scraper` | html5ever DOM | malformed HTML needing exact HTML5 tree rules | yes | yes | no | yes |
| `json` | data | same schemas over JSON documents | no | no | no | no |
| `xml` | XML DOM | XML documents and XPath selectors | yes | yes | no | no |

## Highlights

**Hybrid HTML + JSON** (`__NEXT_DATA__`, JS state, JSON-LD, OpenGraph):

```python
hy_html = '<head><meta property="og:title" content="X1"></head><script id="s">window.__S__={"n":3}</script>'
hy_schema = {"og": T.meta_tags(values="first")["og:title"], "n": T.css("script#s").text().js_json("window.__S__")["n"]}
print(compile(hy_schema, backend="auto").extract(hy_html))
```

```text
{'og': 'X1', 'n': 3}
```

**Self-healing selectors & Python code patching** (`adaptive` + `extract_or_repair` + `to_python()`):

```python
p_adapt = compile({"price": T.css(".price", adaptive="price").text().float()})
p_adapt.extract('<div id="p1"><span class="price">$100</span></div>')
res = p_adapt.extract_or_repair('<section><div><strong class="cost">$95</strong></div></section>', repair=lambda _: {"price": ".cost"})
print(res.data, res.plan.to_python().splitlines()[-2].strip())
```

```text
{'price': 95.0} 'price': T.css('.cost', adaptive='price').text().float(),
```

**Zero-IO `follow` / `paginate` and Rayon `extract_batch` (GIL released):**

```python
from oxide_parse import follow, paginate

pages = {"https://ex.test/1": "<b>42</b><a class='next' href='/2'>n</a>", "https://ex.test/2": "<b>9</b>"}
f_plan = compile({"vals": follow(T.css("a").attr("href").url(), paginate(T.css("b").map(T.text().int()), T.css("a.next").attr("href").url()))})
print(f_plan.extract('<a href="/1">x</a>', meta={"base_url": "https://ex.test"}, fetch=lambda url, **_: pages[url]))
print(compile({"n": T.css("b").text().int()}, backend="auto").extract_batch(["<b>1</b>", "<b>x</b>"], errors="collect"))
```

```text
{'vals': [42, 9]}
([{'n': 1}, None], [{'index': 1, 'message': 'extraction error at `$.n`: int: no number in "x"'}])
```

**LLM preparation (`distill`, `markdown`) and deterministic migration without an LLM:**

```python
from oxide_parse import distill, markdown, migrate

print(markdown("<article><h1>Title</h1><p class='ad'>Ad</p><p>Text</p></article>", "article", without=".ad"))
print(migrate.from_selector_map_code({"title": "h1::text", "tags": ["li::text"]}).splitlines()[-3].strip())
```

```text
# Title

Text
'title': T.css('h1').text().strip(),
```

## Benchmark Summary

Medians on Windows x64, Python 3.13 (details, versions, and methodology in [docs/architecture/benchmarks.md](docs/architecture/benchmarks.md) and `benchmarks/`):

| Scenario | `oxide_parse` | Nearest competitors | Run |
|---|---|---|---|
| Catalog 10,000 items + noise (6.2 MiB) | `stream` **71.9 ms**, `lite` **103.1 ms** | `frostwork` 127.5 ms, `selectolax` 445.1 ms, `bs4+lxml` 8659.6 ms | v7.2 full |
| Early Exit (`og:title` from `<head>`, 875 KiB) | `auto` (`stream`) **1.43 ms** | `scraper` (v8.4.1 pre-stream meta) 81.89 ms | v8.5 paired |
| Hybrid HTML + `__NEXT_DATA__` (1,000 records) | `lite` **4.34 ms**, `stream` **6.05 ms** | `selectolax + json.loads` 2.45 ms, `lxml + json.loads` 5.17 ms | v8.0 full |
| Batch 32 docs x 500 items (9.8 MiB) | `stream` (Rayon) **51.6 ms** | `stream` (loop) 121.7 ms, `selectolax` (loop) 487.7 ms | v8.0 full |
| HTML to Markdown (300 blocks) | `lite` **1.51 ms**, `scraper` **7.26 ms** | `html2text` 140.5 ms, `markdownify` 197.4 ms | v8.5 paired |

## Documentation

| Section | Links |
|---|---|
| Docs map | [docs/index.md](docs/index.md) |
| Guides | [Quickstart](docs/guide/01_quickstart.md) · [DSL Cheatsheet](docs/guide/02_dsl_cheatsheet.md) · [Choosing Engine](docs/guide/03_choosing_engine.md) · [HTML + JSON](docs/guide/04_hybrid_html_json.md) · [Adaptive & Repair](docs/guide/05_adaptive_and_repair.md) · [Follow & Pagination](docs/guide/06_follow_and_crawling.md) · [RAG & Migration](docs/guide/07_llm_rag_markdown.md) |
| AI prompts | [Generate Schema](docs/ai/prompt_generate_schema.md) · [Migrate Parser](docs/ai/prompt_migrate.md) · [Self-Healing Repair](docs/ai/prompt_self_healing.md) |
| Reference | [Python API](docs/reference/python_api.md) · [IR Specification](docs/reference/ir_spec.md) · [CSS & XPath](docs/reference/selectors_and_xpath.md) · [CLI](docs/reference/cli.md) |
| Core & tests | [Architecture](docs/architecture/overview.md) · [Conformance](docs/architecture/conformance.md) · [Benchmarks](docs/architecture/benchmarks.md) · [Publishing](docs/architecture/publishing.md) · [CHANGELOG](CHANGELOG.md) |
| LLM indexes | [llms.txt](llms.txt) · [llms-full.txt](llms-full.txt) · [CLAUDE.md](CLAUDE.md) |

Report bugs in [GitHub Issues](https://github.com/M-16gt/oxide_parse/issues). Use the [security policy](SECURITY.md) to report vulnerabilities privately.
