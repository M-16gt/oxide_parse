# Python API Reference

Public classes, functions, and methods of `oxide_parse` (`v0.9.0`). Usage examples live in [guide/02_dsl_cheatsheet.md](../guide/02_dsl_cheatsheet.md).

## Top-Level Functions (`oxide_parse`)

- `compile(schema, backend="html", *, adaptive_store=None, adaptive_state=None) -> Plan` — compile a schema (`dict`, `list`, `Expr`, or literal) into a reusable `Plan`. `backend`: `"html"`, `"auto"`, `"stream"`, `"lite"`, `"scraper"`, `"json"`.
- `extract(source, schema, backend="html", *, meta=None, fetch=None, repair=None, repair_batch=None) -> Any` — one-shot extraction (compiles every call; prefer `compile` for repeated use).
- `to_ir(schema) -> dict` — convert a schema into JSON-compatible IR.
- `decompile(ir: dict, name: str = "schema") -> str` — turn IR back into Python DSL source code.
- `if_(cond, then, otherwise=None) -> Expr` — conditional branch evaluated against the current context.
- `coalesce(*alternatives) -> Expr` — first non-`None` expression from the arguments.
- `follow(request, schema, meta=None) -> Expr` — sub-query on a linked document (see Sub-queries below).
- `paginate(items, next, *, max_pages=None, max_items=None, meta=None) -> Expr` — collect `items` across pages linked by `next` requests (see Sub-queries below).
- `distill(html: str | bytes | bytearray, keep: int = 2, max_text: int = 80, max_chars: int = 20000) -> str` — compact HTML skeleton for humans or LLM prompts. Requires the `lite` engine.
- `markdown(html: str, selector: str = "body", *, without: str | None = None, backend: str = "html") -> str` — render the page or element to Markdown (`""` if `selector` matches nothing). Requires a DOM engine.
- `validate_ir(ir, backend: str = "auto") -> list[str]` — return every structural/compilation problem with its path (`$.items[0].price: ...`). Accepts an IR dict, JSON string, or DSL schema, and never raises.
- `ir_schema() -> dict` — Draft 2020-12 JSON Schema of the IR format.
- `engines() -> list[EngineInfo]` — engines compiled into the active build.

---

## DSL `T` (`Expr`)

`T` is the empty root `Expr`. Chains are immutable: every method returns a new `Expr`.

### Document and Value Navigation

> **Engines:** `stream` · `lite` · `scraper` · `json` (for `key`/`at`/`slice`/`first`/`last`) | **Fallback on `auto`:** `css` (without `+ ~ :has :contains :last-child :only-child :nth-last-* :last-of-type :only-of-type :empty` or `adaptive`), `:scope`, `key`, `at`, `slice`, `rest`, `first`, `last` run on `stream`; `xpath`, `parent`, `children`, `root`, `adaptive` fall back to DOM.

- `T.css(selector: str, *, adaptive: str | None = None, threshold: float | None = None)` — match CSS elements. With `adaptive="key"`, saves an element fingerprint on the first match and relocates the best candidate (`>= threshold`, default `0.5`) when selectors break. Passing `threshold` without `adaptive` raises `TypeError`.
- `T.xpath(selector: str)` — evaluate XPath 1.0 (DOM engines only).
- `T.parent()` — parent element.
- `T.children()` — child elements (on a single node, `map`/`count`/`first` also iterate its children).
- `T.root()` — document root element.
- `T.key(name: str)` / `T[name]` — object key (JSON, `parse_json()`, `js_json()`, `json_ld()`, `meta_tags()`).
- `T.at(i: int)` / `T[i]` — element at zero-based non-negative index (use `.last()` or `.slice(-1)` to count from the end).
- `T.first()` / `T.last()` — first / last element.
- `T.slice(start: int | None = None, stop: int | None = None)` / `T[start:stop]` — slice a node list, JSON list, string (by Unicode characters), or children of a single node. Stepped slicing (`T[::2]`) is unsupported.
- `T.rest()` — alias for `T.slice(1)` (skips header inside `group_by(...).map(...)`).

### Node Extraction

> **Engines:** `text`, `attr`, `has_attr`, `meta`, `index` work in `stream` · `lite` · `scraper`; `html`, `inner_html`, `markdown`, `distill` require a DOM engine.

- `T.text(sep: str | None = None, *, strip: bool = False, without: str | None = None, br: str | None = None)` — text of the first matched node (`<script>`/`<style>` descendants are excluded unless selected directly).
  - `sep`: string joined between text pieces;
  - `strip=True`: trims each piece and drops empty ones;
  - `without=".ad, .promo"`: CSS selector of subtrees to omit;
  - `br="\n"`: string emitted for `<br>` tags instead of `sep`.
- `T.attr(name: str)` — attribute value of the first node (`None` if absent).
- `T.has_attr(name: str)` — `True` if the first node has the attribute (including boolean flags such as `<button disabled>`).
- `T.html()` / `T.inner_html()` — outer / inner HTML of the first node.
- `T.markdown(*, without: str | None = None)` — convert the first node to Markdown.
- `T.distill(keep: int = 2, max_text: int = 80, max_chars: int = 20000)` — compact skeleton of the first node (direct arena walk on `lite`, HTML round-trip on `scraper`; requires feature `lite`).
- `T.meta(key: str)` — lookup `key` in the `meta` dictionary passed to `extract(..., meta=...)` or `follow`/`paginate(..., meta=...)`.
- `T.index()` — zero-based item index inside `.map(...)`.

### Strings, URLs, Numbers, and JSON

> **Engines:** `stream` · `lite` · `scraper` · `json` | **Fallback on `auto`:** stays on `stream`

All operations propagate `None` safely without raising.

- `T.strip()`, `T.lower()`, `T.upper()` — whitespace trimming and ASCII/Unicode case conversion.
- `T.replace(old: str, new: str)` — substring replacement.
- `T.split(sep: str)`, `T.join(sep: str)` — split a string or join a list of strings.
- `T.regex(pattern: str, group: int = 0)` — first regex match (or capture group; `None` if unmatched).
- `T.regex_all(pattern: str, group: int = 0)` — list of all regex matches.
- `T.int()`, `T.float()` — parse integers/floats including currency symbols and locale separators (`"$1,299.50"`, `"19,99 €"`, `"12 990 ₽"`, `"1.299,50"`). Raises an extraction error with path when no number is present.
- `T.bool()` — coerce to `bool`.
- `T.url(base: Any = None)` — RFC 3986 relative URL resolution against explicit `base`, `meta["base_url"]`, or `<base href>`.
- `T.parse_json()` — parse a JSON string; errors fail with field path.
- `T.js_json(var: str | None = None)` — extract JSON object/array from JS assignments or `JSON.parse("...")` strings.

### Hybrid HTML & Flat Markup Primitives

- `T.meta_tags(prefixes=("og:", "twitter:"), *, values="auto")` — `<meta property|name content>` dictionary with lowercased keys. Pass `prefixes=[]` for all tags. `values`: `"auto"`, `"first"`, or `"list"`.
  > **Engines:** `stream` · `lite` · `scraper` | **Fallback on `auto`:** stays on `stream` (stops at opening `<body>`)
- `T.json_ld(schema_type: str | None = None)` — list of JSON-LD objects from `<script type="application/ld+json">` (`@graph` and arrays unfolded; invalid blocks skipped; `schema_type` matches `@type` names or `schema.org` URIs).
  > **Engines:** `lite` · `scraper` | **Fallback on `auto`:** switches to DOM
- `T.sections(header: str, items: str | None = None)` — split flat child elements at `header` into `[{"header": str, "items": [str, ...]}]`.
  > **Engines:** `lite` · `scraper` | **Fallback on `auto`:** switches to DOM
- `T.group_by(header: str, items: str | None = None)` — group flat child siblings starting at each `header`; must be followed by `.map(...)` whose context is each group.
  > **Engines:** `lite` · `scraper` | **Fallback on `auto`:** switches to DOM
- `T.kv(label: str, *, exact: bool = True)` — sibling element (`following-sibling::*[1]`) next to a label in `th`/`dt`/`td`/`label`/`strong`/`b`/`span`/`div` (backed by XPath).
  > **Engines:** `lite` · `scraper` | **Fallback on `auto`:** switches to DOM

### Collections, Null Handling, and Logic

- `T.count()` — number of items (`stream` · `lite` · `scraper` · `json`).
- `T.exists()` — `True` if non-`None` and non-empty (`stream` · `lite` · `scraper` · `json`).
- `T.map(body, on_error: str | None = None)` — apply `body` to each item (`on_error`: `None`/`"raise"`, `"skip"`, or `"null"`).
  > **Engines:** top-level `css(...).map(...)` streams when its body uses streamable captures; nested `map` over DOM nodes switches `auto` to DOM.
- `T.flat_map(body)`, `T.filter(pred)` — flatten nested lists and filter collections (value lists work in `stream`; node lists require DOM).
- `T.default(value)` — fallback evaluated against current context when left-hand side is `None`.
- `T.required()` — raise extraction error if left-hand side is `None`.
- `T.non_empty()` — turn empty/whitespace string, empty list, or empty dict into `None`.
- `T.coalesce(*alternatives)` — first non-`None` expression.
- `T.eq(other)` (`==`), `T.ne(other)` (`!=`), `T.contains(other)`, `T.startswith(other)`, `T.endswith(other)`.
- `T.not_()` (`~`), `T.and_(other)` (`&`), `T.or_(other)` (`|`), `T.if_(cond, then, otherwise=None)`.

### Sub-queries & Pagination (`follow`, `paginate`)

> **Engines:** `lite` · `scraper` · `json` | **Fallback on `auto`:** switches to DOM

- `follow(request, schema, meta=None)` / `T.follow(...)` — emit a fetch request for another document. `request` is a dict expression (`{"url": ..., "headers": ...}`) or URL expression shorthand. Drivers call `fetch(**request)`. If `request` or `url` is `None`, no fetch runs and the value is `None`. Propagates `"url"` as `meta["base_url"]` to the child run.
- `paginate(items, next, *, max_pages=None, max_items=None, meta=None)` / `T.paginate(...)` — walk a chain of pages and concatenate `items`:
  - `items`: expression yielding a `list` (or `None` -> `[]`) on the initial page and each fetched page;
  - `next`: URL string or request dict expression for `fetch(**next)`; stops before fetching on `None`, empty string, `"#"`, `javascript:...`, `"url": None`, or cycle detection;
  - `max_pages >= 1`: page limit including initial page (default safety cap without it: 10,000 pages); `max_items >= 1`: item limit;
  - Inherits parent `meta`, updates `meta["base_url"]` per page, and applies trailing operations (`.count()`, `.slice(...)`, etc.) to the final combined list.

---

## Classes `Plan`, `Session`, `Detailed`, `RepairResult`, `EngineInfo`

### `Plan`

- Properties:
  - `plan.ir -> dict` — compiled IR dictionary;
  - `plan.engine -> str` — actual resolved backend (`"stream"`, `"lite"`, `"scraper"`, `"json"`).
- Persistence:
  - `plan.to_json(indent=None) -> str`, `plan.to_yaml() -> str` (requires `pyyaml`), `plan.to_python(name="schema") -> str`;
  - `Plan.from_ir(ir, backend="html", **kw)`, `Plan.from_json(text, ...)`, `Plan.from_yaml(text, ...)`, `Plan.from_file(path, ...)`.
- Debugging:
  - `plan.explain() -> str` — compiled execution plan, selector table, peephole rewrites, and streaming program;
  - `plan.run_native(source) -> int` — parse and extract without allocating Python objects (returns extracted value count).
- Synchronous extraction:
  - `plan.extract(source, *, meta=None, fetch=None, repair=None, repair_batch=None) -> Any`
  - `plan.extract_detailed(source, *, meta=None, fetch=None, repair=None, repair_batch=None) -> Detailed`
  - `plan.extract_or_repair(source, repair=None, *, meta=None, fetch=None, repair_batch=None) -> RepairResult`
  - `plan.extract_batch(sources: Iterable, *, threads: int | None = None, errors: str = "raise")` — Rayon batch execution with GIL released (`errors="collect"` returns `(results, failures)`).
  - `plan.extract_many(sources: Iterable) -> list` — shorthand for `extract_batch` with `errors="raise"`.
- Async extraction:
  - `await plan.aextract(source, *, meta=None, fetch=None, repair=None, repair_batch=None) -> Any`
  - `await plan.aextract_detailed(...) -> Detailed`
  - `await plan.astart(source, *, meta=None) -> Session`
- `Session` & selector repair:
  - `plan.start(source, *, meta=None) -> Session`
  - `plan.with_selectors(mapping: dict[str, str]) -> Plan` — copy with rewritten `adaptive` CSS selectors (clears old fingerprints for rewritten keys in memory and `adaptive_store`; raises `KeyError` on unknown keys).
  - `plan.repair_request(source, *, meta=None, max_chars=20000) -> dict | None` — offline repair payload (or `None` when nothing missed).
  - `plan.export_adaptive_store() -> str` — serialize adaptive fingerprints to JSON.
  - `plan.import_adaptive_store(state: str) -> int` — merge fingerprints from JSON; returns count loaded.

### `Session`

- `sess.requests() -> list[dict]` — pending `"fetch"` and `"repair"` requests.
- `sess.repair_batches() -> list[dict]` — repair requests grouped by shared container skeleton.
- `sess.pending() -> list[dict]` / `sess.repairs() -> list[dict]`.
- `sess.done -> bool` — `True` when no fetch requests remain (repairs are optional).
- `sess.overrides -> dict[str, str]` — selectors supplied by repairs in this session.
- `sess.resume(answers)` / `await sess.aresume(answers)` — submit `(id, document)`, `(id, {key: css})`, or `(id, None)`.
- `sess.result() -> Any` / `sess.detailed() -> Detailed`.

### `Detailed`, `RepairResult`, `EngineInfo`

- `Detailed(data, errors, misses, repairs)` — frozen dataclass returned by `extract_detailed`, `aextract_detailed`, and `Session.detailed()`.
- `RepairResult(data, plan, repaired, errors, misses)` — frozen dataclass returned by `extract_or_repair`.
- `EngineInfo(name, kind, xpath, description)` — frozen dataclass returned by `engines()`.

---

## Utilities: `oxide_parse.migrate` and `oxide_parse.prompts`

- `migrate.from_crawl4ai(strategy, *, kind=None, wrap=None) -> dict` — Crawl4AI `JsonCssExtractionStrategy` / `JsonXPathExtractionStrategy` -> IR.
- `migrate.from_crawl4ai_code(strategy, *, kind=None, wrap=None, name="schema") -> str` — same as Python source.
- `migrate.from_selector_map(mapping, *, strip=True) -> dict` — Scrapy/Parsel flat selector map -> IR.
- `migrate.from_selector_map_code(mapping, *, strip=True, name="schema") -> str` — same as Python source.
- `migrate.MigrationError(ValueError)` — unsupported construct error with field path.
- `prompts.generate_prompt() -> str` — system prompt for schema generation from HTML.
- `prompts.migrate_prompt(source="bs4") -> str` — system prompt for migrating from `"bs4"`, `"crawl4ai"`, `"scrapy"`, `"selectolax"`, or `"lxml"`.
- `prompts.sources() -> list[str]` — supported migration prompt sources.
