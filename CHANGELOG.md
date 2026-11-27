# Changelog

## 0.9.1

- Added links to the source repository, documentation, issue tracker, and changelog in the PyPI metadata.
- Added a private vulnerability-reporting route and linked it from the README.

## 9.0 — migrators, CLI, release hygiene (package version 0.9.0)
* `oxide_parse.migrate`: deterministic converters, no model, milliseconds. `from_crawl4ai(strategy)` -> IR and
  `from_crawl4ai_code(strategy)` -> Python source (`JsonCssExtractionStrategy` and `JsonXPathExtractionStrategy`: `baseSelector`,
  `baseFields`, `text` / `attribute` / `html` / `regex` / `list` / `nested_list` / `nested`, `default`, `transform`), and
  `from_selector_map(mapping)` / `from_selector_map_code(...)` for flat Scrapy / Parsel style maps (`h1::text`, `a::attr(href)`,
  `["li::text"]`, XPath, nested dicts, `"_base"` records). What has no faithful equivalent (`computed`, `source`, unknown types
  and transforms) raises `MigrationError` with the path instead of converting wrongly. `Plan.from_ir(ir)` builds a plan from an IR dict.
* CLI `oxide-parse` / `python -m oxide_parse`: `distill`, `markdown`, `run` (prints the chosen engine; `--explain`), `decompile`,
  `validate`, `migrate`, `prompt generate|migrate`, `ir-schema`. Schemas: `.json` / `.yaml` IR or a `.py` file that defines `schema`.
  Errors are one line on stderr, exit code 1.
* `oxide_parse.prompts` (what `oxide-parse prompt` prints): a system prompt to generate a schema and one to migrate code from
  bs4 / Crawl4AI / Scrapy / selectolax / lxml; the DSL cheat sheet and the operation list come from the same table as `validate_ir`.
* Release: versions synchronised to 0.9.0 (`pyproject.toml`, both `Cargo.toml`, `Cargo.lock`, `__init__.py`; a test keeps them equal);
  the native module exports `__version__`; `_core.pyi` is fully annotated and documented; CI (`.github/workflows/ci.yml`):
  Rust tests, `cargo check` for `engine-scraper` / `engine-lite` / `fast` alone, the IR schema drift check, `maturin develop` + `pytest`
  on Python 3.9 / 3.12 / 3.13.

## 8.5 — polishing the core: slices, in-pipeline distill, streamed meta, IR validation
* `T.slice(start, stop)`, `T.rest()` (= `slice(1)`), `T[1:]`, `T[:5]`, `T[-3:]`: Python-style slices of a node list, a list
  value, a string (characters) or the children of one node; negative counts from the end, out-of-range is clamped, crossed
  bounds give an empty result. A pure value operation: it works in the tails of the `stream` engine. With `group_by`:
  `T.rest().map(T.text().strip())` is the group without its header.
* `T.distill(keep=2, max_text=80, max_chars=20000)`: one element as a prompt-sized skeleton next to the other fields.
  `lite` distils the parsed node directly, other engines go through the element's HTML.
* `stream`: `T.meta_tags(...)` (also with a tail such as `["og:title"]`) is streamed. It reads the `<head>` and lets the early
  exit fire once `<body>` has opened, so a metadata-only schema reads a few KB of a multi-MB page. (A `<meta>` inside the body is
  seen by the DOM engines only.)
* `with_selectors()` forgets the fingerprints of the rewritten keys in a store *file* too: `PyPlan.forget_adaptive_keys` and
  `Plan.with_selectors` use it for the in-memory store as well.
* `oxide_parse.validate_ir(ir, backend="auto") -> list[str]` returns every problem with a path (`$.items[1].map.price: ...`),
  not just the first; `oxide_parse.ir_schema()` and `python/oxide_parse/ir.schema.json` (draft 2020-12). One table
  (`ir_spec.py`) drives the validator and the schema; `scripts/gen_ir_schema.py --check` guards against drift.
* Benchmarks: `10_markdown_rag` (`oxide_parse` vs `markdownify` vs `html2text`); `06_text_entities` already runs `stream`.
* Performance: the Markdown renderer copies runs of text instead of pushing characters and lower-cases tag names without
  allocating; `meta_tags` inserts with a single hash lookup.

## 8.4.1 — review of 8.4
* **An invalid selector from a model no longer fails the extraction.** Answers are validated with the engine's CSS parser first;
  an invalid one leaves the request open with `previous_error` (the parser's message) and `invalid_answers`, the second invalid
  answer declines it. Before, `resume` raised.
* `take_result`: `fill` counts nested follows (limit 64) and an unresolved `Pending` is an error, not a silent hole.
* Distilling is done once per container: fields of one redesigned card that break together share one `skeleton`.
  Python: `sess.repair_batches()` and `repair_batch=` in `extract*` / `aextract*` (one call per fragment with a `fields` list;
  `sess.resume` is unchanged).
* `T.meta_tags(values="auto" | "first" | "list")`.

## 8.4 — one `Session` loop for `follow` and repairs
* A broken `adaptive` selector is now a *request* of the session, next to the `follow` fetch requests:
  `Session::repairs()` / `RepairRequest` in Rust, `sess.requests()` in Python (`{"kind": "fetch" | "repair", ...}`).
  `resume` answers both: `(id, document)` for a fetch, `(id, {key: css})` for a repair, `(id, None)` or `{}` declines.
  A repair re-runs the document it concerns (the root or a followed one) with the new selector; `follow` requests that the
  new run repeats keep their ids, so fetches already under way stay valid and fetched documents are not fetched again.
* A repair request carries what a model needs: `key`, the old `css`, the `fingerprint` of the old element with its
  `samples`, the **old container distilled on its own** (`container` label such as `div#shop.catalog`, `skeleton`,
  `scope: "container"`; if no element of the page resembles that container: the distilled page, `scope: "page"`), the
  `url` of the followed document and the schema. Needs the `lite` engine in the build (otherwise no skeleton).
* Repairs are optional: `take_result` / `result()` only need the fetches. Answers are applied as *overrides*
  (`sess.overrides`, `Detailed.repairs`): `plan.with_selectors(det.repairs)` makes them permanent. The old reference
  stays in the store until the element found by the new selector replaces it (a failing answer keeps the context).
* Python: `plan.start(...)` / `await plan.astart(...)` return a `Session` (`requests()`, `resume()`, `aresume()`, `result()`,
  `detailed()`, `overrides`); `extract` / `extract_detailed` / `aextract` / `aextract_detailed` take `repair=` (sync or
  async, concurrent with the fetches of the round; a key that is answered wrongly twice is declined). `extract_or_repair`
  runs on this loop and now also follows (`fetch=`).
* **Breaking:** the `repair` callback of `extract_or_repair` is called once per broken key with the new request shape (no
  `misses` list). `repair_request()` keeps the old page-level format.
* Internals: sub-document values are inserted into the tree at `take_result` (not while resuming), the session keeps per
  document records, and the source of a document is kept only while it has unresolved misses.

## 8.3 — streamed `without`, `group_by`, `json_ld`, `meta_tags`
* `stream`: `text(..., without="script, .ad, .badge")` is streamed (also per record and together with `br`). A collector
  starts skipping at the first element matching `without` below it and stops when that element closes. Any selector the
  stream engine can match works (ancestors included, no sibling combinators); other ones make `auto` fall back.
* `T.group_by(header, items=None).map(body)`: the general form of `sections`. A group is the header plus the siblings up to
  the next header; the body of `map` sees the group as its context: `T.first()` is the header, `T.css("a")` searches the group
  *including its own nodes* (flat markup has its links as siblings). `on_error` works. `sections` is unchanged. Without
  `.map(...)` it fails with a message saying so. Needs a DOM engine (`Dom::matches_sel` is new).
* `T.json_ld(schema_type=None)`: the JSON-LD objects as a flat list (arrays and `@graph` unfolded, `<!-- -->` wrappers
  stripped, broken blocks skipped, `@type` as string or list, `https://schema.org/Product` = `Product`).
* `T.meta_tags(prefixes=("og:", "twitter:"))`: meta tags as a dict (names lower-cased, `[]` = every tag, a repeated name becomes a list).

## 8.2.1 — review of 8.2
* **Behaviour change:** `T.url()` combines `meta["base_url"]` (the page URL) with the page's `<base href>`: the `<base href>`
  is resolved against the page URL first (`https://site.com/catalog/item` + `<base href="/shop/">` + `href="123"` ->
  `https://site.com/shop/123`). An absolute `<base href>` therefore wins over `meta["base_url"]` (as in a browser);
  `url(base)` in the schema still overrides everything. Before, `meta["base_url"]` replaced `<base href>`.
* `follow`: the child request gets `meta["base_url"]` = the followed URL unless the schema sets its own
  (`follow(...).css("a").attr("href").url()` works without passing meta by hand).
* `stream` + `url()`: early exit is held back only until the first `<base href>` or the opening `<body>`, instead of
  never; `url(base)` with an explicit base is not affected at all.
* `with_selectors()` drops the fingerprints (and `#healed` ones) of the keys it rewrites, keeps the others. A plan
  with a store *file* leaves the shared file alone.
* `samples` of `adaptive | map`: up to three scalars of the first record (`["Sale", "iPhone 16", "$999"]`).
* `is_none_or` (Rust 1.82) replaced by `map_or(true, ..)` in the tokenizer.

## 8.2 — engine parity and the leftovers of `AUDIT_RESPONSE`
* `:scope` in `stream`: `SView` knows the depth of the scope element (the open record while its fields are matched,
  the top-level element otherwise) and the selector cache is keyed by it, so nested records stay correct.
  `T.css(":scope > .title")` now runs on `lite` and `stream` (`auto` keeps the fast path); `scraper` still refuses it
  and `html` falls back to `lite`.
* `T.url()` uses the document's first `<base href>` when `meta["base_url"]` is not given (`Dom::base_href`; the stream
  engine records it while parsing and no longer stops early for schemas that call `url()`). Explicit `meta["base_url"]`
  and `url(base)` win. A relative `<base href>` without a meta base is still an error ("base URL is not absolute").
* `js_json(var)`: the parsed JavaScript object is cached per `(node_key, var)` (up to 16 scripts of 512+ bytes), so several
  fields reading branches of one `window.__INITIAL_STATE__` parse it once.
* Adaptive store without files: `Store::export_json` / `import_json`, `MemoryStore::to_json` / `from_json`,
  `Engine::adaptive_store()`; Python `plan.export_adaptive_store()`, `plan.import_adaptive_store(s)` and
  `Plan(..., adaptive_state=s)`. `with_selectors()` now keeps what the plan has learned.
* Records in `samples` of an `adaptive | map` fingerprint are represented by their first text value.

## 8.1 — performance fixes and the next four features (nothing here was compiled or benchmarked yet)
**Performance (see `AUDIT_RESPONSE.md` for what was changed with respect to the plan)**
* `find_end_tag` jumps with `memchr` / `memchr2` again (script, style, title, textarea).
* `parse_named_entity`: at most five binary searches for legacy names (2..=6 characters), one for `&name;`.
* `Events` keeps the tag flags on the stack (no `classify` per entry in the end-tag walk) and counts open `svg`/`math`.
* `lite`: the fused record walk has no per-record allocation (bitmask, buffers reused across the records of a `map`) and
  is used from four distinct leading selectors (`FUSE_MIN_HEADS`).
* JSON: `css | text | parse_json` becomes `css | parse_json`; the parsed document is cached by `node_key`
  (a second field reading the same `__NEXT_DATA__` neither copies nor hashes its text).

**Features**
* `T.markdown(without=...)` and `oxide_parse.markdown(html, selector="body", without=...)`: HTML -> Markdown
  (headings, paragraphs, lists, links, images, emphasis, code, quotes, GFM tables). Built on the new `Dom::walk`
  (open/text/close events), so it runs on `scraper` and `lite`.
* `:scope` in CSS (`lite`): the element a `select` started from, so `T.css(":scope > .title")` inside a record ignores
  page-level ancestors. `scraper` and `stream` refuse such selectors; `auto` / `html` fall back to `lite`.
* `T.text(..., br="\n")`: every `<br>` becomes that string and replaces the separator there. `text(sep=..., strip=...,
  br=...)` is streamed now (without `without=`).
* Adaptive v2: `adaptive` right before `map` relocates the whole list of records after a redesign (the best match
  plus its similar siblings), and fingerprints keep up to three `samples` of what the selector produced; they reach
  `repair_request()` inside `misses[].fingerprint`.

## 8.0 — v7.2 report, part 3 (and a cleaner `follow` contract)
* **Breaking:** `follow` has one contract, `request`. The `url=` argument of `T.follow(...)` / `follow(...)` and the
  `url` alias of the IR key are gone (`T.follow(T.attr("href"), schema)` or `request={"url": ...}`).
* `map(..., on_error="skip"|"null")` is streamed: the `stream` engine quarantines failing records itself and reports
  them in `extract_detailed().errors` (same paths and messages as the DOM engines); `auto` no longer falls back.
* `lite`: the fields of a record that start with a selector are evaluated in one walk of the record subtree
  (`Dom::select_multi`, `id`/`class` read once, `may_match` pre-filter). *Unmeasured: run `bench_suite.py`
  (`09b_selector_fanout_bottleneck`); the 2-2.5x of the report is a hypothesis.*
* `parse_json()[...][...]` navigates by reference (`json_path`) and the parsed text is cached per run: several fields
  of one `__NEXT_DATA__` parse it once, and `["category"]` no longer builds the 1 000 products it throws away.
* Not done on purpose: "card isolation" in `stream` (see `AUDIT_RESPONSE.md`); a test now pins the shared
  `querySelectorAll` scope in all three engines.

## 7.3 — audit of 7.2
* `Key` / `At` move the child out of the owned value instead of deep-cloning (JSON navigation was the one lost benchmark).
* `T.kv()` is scoped to the current element (`.//*`), fixed inside `.map(...)`.
* adaptive value guard: `default(...)` can no longer turn a failed pipeline into a learned reference.
* `distill`: `<script>`/`<template>`/... between cards no longer break the folding of repeated siblings.
* `js_json`: `//` and `/* */` comments and trailing commas in object literals.
* XPath `following-sibling` / `preceding-sibling` use O(1) sibling links (`Dom::next_sibling` / `prev_sibling`).
* `find_assignment` ignores `x.window.__S = ...` (adopted from the author's compiled tree).

## 7.2 — new features (stage 2)
* `map(..., on_error="skip"|"null")`, `extract_detailed` (quarantined records with their `$.path`).
* `url()` (RFC 3986), `js_json()`, `sections()`, `kv()`.
* CSS `:has()` (relative selectors with `>` `+` `~`, lists) and `:contains()` / `:-soup-contains()` in the `lite` engine.
* `extract_batch(threads=, errors="collect")`.
* `distill()` skeleton HTML; `repair_request` / `with_selectors` / `extract_or_repair` (zero IO).
* Adaptive misses are reported (`Detailed.misses`) with the old fingerprint.

## 7.1 — fixes and existing code (stage 1)
* `<script/>` tokenizer consistency, `select|text` early exit, stream `first().strip()`, `meta` in streamed tails,
  O(1) siblings, parser depth limits, stream early exit, adaptive value guard.
