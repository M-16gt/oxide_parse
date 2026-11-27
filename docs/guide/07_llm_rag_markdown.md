# Page Distillation, Markdown for RAG, and Schema Migration

`oxide_parse` bundles utilities for preparing HTML for language models and converting existing scraper definitions.

## Skeleton HTML for LLM Prompts (`distill`)

Raw pages often exceed hundreds of kilobytes due to `<script>`, `<style>`, `<svg>`, tracking attributes, and long lists of nearly identical cards. `distill()` shrinks HTML down to a compact structural skeleton by stripping scripts/styles/SVGs and noisy attributes, clipping long text, and folding runs of similar sibling elements:

```python
from oxide_parse import T, compile, distill

cards = "".join(f'<li class="item" data-track="{i}"><a href="/i/{i}">Item {i}</a></li>' for i in range(10))
html = f"""
<html><head><style>.x{{}}</style></head>
<body>
  <nav>Menu</nav>
  <main class="content">
    <h1>Overview</h1>
    <p>First <b>paragraph</b> of text.</p>
    <ul class="list">{cards}</ul>
  </main>
  <script>var x = 1;</script>
</body></html>
"""

print(distill(html, max_chars=2000))
```

```text

<html>
  <head></head>
  <body>
    <nav>Menu</nav>
    <main class="content">
      <h1>Overview</h1>
      <p>First
        <b>paragraph</b>of text.</p>
      <ul class="list">
        <li class="item">
          <a href="/i/0">Item 0</a></li>
        <li class="item">
          <a href="/i/1">Item 1</a></li>
        <!-- +8 similar <li class="item"> --></ul></main></body></html>
```

You can also invoke `.distill(...)` inside a schema to shrink one page section alongside structured fields:

```python
schema = {
    "title": T.css("h1").text(),
    "main_skeleton": T.css("main").distill(keep=2, max_text=80, max_chars=4000),
}
data = compile(schema, backend="auto").extract(html)
print(data["title"])
print("<!-- +8 similar" in data["main_skeleton"])
```

```text
Overview
True
```

Both `distill()` and `.distill()` require the `lite` engine (included in default PyPI wheels).

## HTML to Markdown (`markdown`)

For RAG ingestion, `oxide_parse` includes a Rust HTML-to-Markdown converter (`oxide_parse.markdown()` and `T.markdown()`). It formats headings, paragraphs, lists, links, images, emphasis, code blocks, blockquotes, and tables while skipping `<script>`, `<style>`, `<svg>`, and any elements matched by `without=`:

```python
from oxide_parse import markdown

article = """
<article>
  <h1>Overview</h1>
  <p>First <b>paragraph</b> of text.</p>
  <div class="ad">Advertisement</div>
  <ul><li>Item A</li><li>Item B</li></ul>
</article>
"""

md = markdown(article, selector="article", without=".ad")
print(md)
```

```text
# Overview

First **paragraph** of text.

- Item A
- Item B
```

Inside a schema, write `T.css("article").markdown(without=".ad")`. Requires a DOM engine (`lite` or `scraper`); in v8.5 benchmarks over a 300-block article, `lite` ran in 1.5 ms and `scraper` in 7.3 ms vs 140 ms for `html2text` and 197 ms for `markdownify`.

## Deterministic Migration Without an LLM

Existing declarative Crawl4AI JSON schemas and flat Scrapy/Parsel selector maps convert deterministically in milliseconds via `oxide_parse.migrate` and can be decompiled straight back to Python code:

```python
from oxide_parse import Plan, migrate

crawl4ai_strategy = {
    "name": "Catalog",
    "baseSelector": ".card",
    "fields": [
        {"name": "title", "selector": "h2", "type": "text"},
        {"name": "price", "selector": ".price", "type": "regex", "pattern": r"(\d+)", "default": "0"},
        {"name": "url", "selector": "a", "type": "attribute", "attribute": "href"},
    ],
}

ir = migrate.from_crawl4ai(crawl4ai_strategy)
print(migrate.from_crawl4ai_code(crawl4ai_strategy), end="")
```

```text
from oxide_parse import T

schema = T.css('.card').map({
    'title': T.css('h2').text(strip=True),
    'price': T.css('.price').text(strip=True).regex('(\\d+)', 1).default('0'),
    'url': T.css('a').attr('href'),
})
```

The converter handles `JsonCssExtractionStrategy`, `JsonXPathExtractionStrategy` (including `CrawlerRunConfig.dump()` wrappers), `list` / `nested_list` / `nested`, `baseFields`, `transform`, and `default`. Unsupported constructs (`computed`, `source`) raise `MigrationError` with the exact field path.

For flat selector maps in Scrapy / Parsel style (`::text`, `::attr(href)`, `["li::text"]`, `"_base": ".card"`), use `from_selector_map` and `from_selector_map_code`:

```python
print(migrate.from_selector_map_code({
    "title": "h1::text",
    "tags": ["li.tag::text"],
    "next": "a.next::attr(href)",
}), end="")
```

```text
from oxide_parse import T

schema = {
    'title': T.css('h1').text().strip(),
    'tags': T.css('li.tag').map(T.text().strip()),
    'next': T.css('a.next').attr('href'),
}
```

Both converters are exposed via the CLI (`oxide-parse migrate --from crawl4ai strategy.json --format python`). For translating imperative BeautifulSoup, Scrapy, or selectolax code with an LLM, see [docs/ai/prompt_migrate.md](../ai/prompt_migrate.md).
