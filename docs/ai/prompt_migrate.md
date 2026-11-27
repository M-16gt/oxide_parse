# Prompt: Migrate Scrapers to `oxide_parse`

## When You Do Not Need an LLM

Declarative Crawl4AI JSON strategies (`JsonCssExtractionStrategy`, `JsonXPathExtractionStrategy`, `CrawlerRunConfig` dumps) and flat Scrapy/Parsel selector maps (`{"title": "h1::text", "tags": ["li::text"]}`) convert deterministically in milliseconds:

```bash
oxide-parse migrate --from crawl4ai strategy.json --format python -o schema.py
oxide-parse migrate --from selector-map selectors.json --format python -o schema.py
```

Or from Python: `oxide_parse.migrate.from_crawl4ai_code(strategy)` and `from_selector_map_code(mapping)`.

For imperative code written with BeautifulSoup, Scrapy, lxml, Selectolax, or Scrapling, use the mapping table and prompt below (or print the source-specific CLI prompt via `oxide-parse prompt migrate --from bs4`).

```bash
oxide-parse prompt migrate --from bs4   # choices: bs4, scrapy, selectolax, lxml, crawl4ai
```

## Primitive Mapping Table

| Source | Legacy Code | `oxide_parse` Equivalent |
|---|---|---|
| **Crawl4AI** | `JsonCssExtractionStrategy({"baseSelector": ".card", "fields": [...]})` | `T.css(".card").map({...})` |
| **Crawl4AI** | `{"name": "p", "selector": ".price", "type": "text"}` | `"p": T.css(".price").text(strip=True)` |
| **Crawl4AI** | `{"name": "u", "selector": "a", "type": "attribute", "attribute": "href"}` | `"u": T.css("a").attr("href")` |
| **Crawl4AI** | `result.markdown` | `T.css("article").markdown()` / `oxide_parse.markdown(html)` |
| **Scrapy / Parsel** | `sel.css("h1::text").get(default="")` | `T.css("h1").text().default("")` |
| **Scrapy / Parsel** | `sel.css("a::attr(href)").getall()` | `T.css("a").map(T.attr("href"))` |
| **Scrapy / Parsel** | `sel.css(".p::text").re_first(r"(\d+)")` | `T.css(".p").text().regex(r"(\d+)", 1)` |
| **Scrapy / Parsel** | `response.urljoin(href)` | `T.attr("href").url()` (+ `meta={"base_url": url}`) |
| **Scrapy / Parsel** | `response.follow(href, callback=self.parse_item, cb_kwargs=...)` | `follow(request=T.attr("href").url(), schema=..., meta=...)` |
| **Next-page loop** | `while next_url: ... next_url = sel.css("a.next::attr(href)").get()` | `paginate(items=..., next=T.css("a.next").attr("href").url())` |
| **BeautifulSoup** | `soup.select_one("h1").get_text(" ", strip=True)` | `T.css("h1").text(" ", strip=True)` |
| **BeautifulSoup** | `for x in el.select(".ad"): x.decompose(); el.get_text()` | `T.text(without=".ad")` |
| **BeautifulSoup** | `float(re.sub(r"[^\d.]", "", txt))` | `T.text().float()` |
| **Selectolax** | `tree.css_first(".name").text(strip=True)` | `T.css(".name").text().strip()` |
| **Selectolax** | `tree.strip_tags(["script", "style", ".ad"])` | `T.text(without=".ad")` (`script`/`style` are already omitted) |
| **Scrapling** | `page.css(".price", adaptive=True).first.text` | `T.css(".price", adaptive="price").text()` |
| **Scrapling** | `page.css(".card", adaptive=True)` (list of cards) | `T.css(".card", adaptive="cards").map({...})` |
| **Next.js / Nuxt** | `json.loads(soup.select_one("#__NEXT_DATA__").text)["props"]...` | `T.css("script#__NEXT_DATA__").text().parse_json()["props"]...` |

## System Prompt (Copy & Paste)

```markdown
You migrate scrapers to `oxide_parse` (Python DSL + Rust execution engine).
Convert the provided extraction code or JSON strategy (Crawl4AI, Scrapy/Parsel, BeautifulSoup, lxml, Selectolax, Scrapling) into a clean declarative `oxide_parse` schema.

## Migration Rules
1. Replace imperative `for item in ...` loops with `.map({...})`.
2. Remove `if el is not None else None` boilerplate — every chain is null-safe by default; use `.default(...)` for explicit fallbacks.
3. Replace manual price cleanup (`re.sub`, `.replace("$", "").replace(",", "")`) with `.float()` or `.int()` (built-in support for `"$1,299.50"`, `"19,99 €"`, `"12 990 ₽"`).
4. Replace `decompose()` / `strip_tags()` calls with `T.text(sep=" ", strip=True, without="...")` (`<script>` and `<style>` are skipped automatically).
5. Convert inline `json.loads(...)` calls on script tags to `.parse_json()[...]`, `.js_json("var_name")[...]`, or `T.json_ld("Type")`.
6. Convert link-following logic (`requests.get`, `response.follow`) to Zero-IO `follow(request={"url": ...}, schema={...}, meta={...})` and next-page loops to `paginate(items=..., next=...)`, with a minimal `plan.extract(html, fetch=...)` example.
7. Convert Scrapling `adaptive=True` lookups to `T.css(sel, adaptive="<field_key>")` and pass `adaptive_store="store.json"` to `compile()`.
8. Scrapy `::text` and `::attr(x)` pseudo-elements are invalid CSS in `oxide_parse` — use `.text()` and `.attr("x")` pipeline methods instead. Note that Parsel `::text` returns direct child text nodes whereas `T.text()` returns descendant text.
9. When direct children inside a card are needed (CSS `> .title`), write `:scope > .title`.

## Output
1. Complete runnable `oxide_parse` snippet (`schema = ...`, `plan = compile(schema, backend="auto")`).
2. **Migration Notes** (3-5 concise bullets: removed boilerplate, chosen engine, subtle semantic differences).
3. Keep non-extraction code (DB persistence, auth, custom session scheduling) outside `schema` around `plan.extract()` / `plan.start()`.
```
