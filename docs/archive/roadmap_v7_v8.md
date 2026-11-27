# Roadmap

## Plan by stages

* **v7.3 (released):** the six fixes of the v7.2 audit (see `CHANGELOG.md`).
* **v9.0 (this release):** `oxide_parse.migrate` (Crawl4AI, selector maps), CLI `oxide-parse`, prompts, version 0.9.0, CI, full `_core.pyi`.
* **v8.5 (released):** `T.slice` / `T.rest`, `T.distill`, streamed `T.meta_tags`, `validate_ir` + `ir.schema.json`, `forget_adaptive_keys`, benchmark `10_markdown_rag`.
* **v8.4 (released, tests pass):** repairs as requests of the `Session` (one loop with `follow`), local distill.
* **v8.3 (released, tests pass):** streamed `text(without=)`, `group_by`, `json_ld`, `meta_tags`.
* **v8.2 (released, tests pass):** `:scope` in stream, `<base href>`, `js_json` cache, store export/import.
* **v8.1 (released, tests pass):** the five performance fixes of the v8.1 plan, `T.markdown`, `:scope` (lite),
  `text(br=)` + streamed `text(sep, strip)`, adaptive lists + `samples`. Next: compile, run the suites, measure.
* **v8.0 (released):** `on_error` in `stream`, fused record walk in `lite`, JSON parse cache + `json_path`,
  `follow(request=...)` only. Next: measure (`bench_suite.py`), then decide on an index of selectors by rightmost tag/class.
* **v8.0 candidates (from the v7.2 report), with the verdict of the review:** `on_error` in the stream engine (yes);
  one walk per card instead of one per field selector in `lite` (yes, 2-2.5x claimed, measure); parse each
  `__NEXT_DATA__` once per document (yes); "card isolation" in `stream` (no: `querySelectorAll` semantics are intended).

* **Stage 1 — fixes and existing code (this release):** tokenizer/tree-rule consistency, `select|text` early exit,
  stream `first().strip()` + `meta`, O(1) siblings, parser depth limits, stream early exit, adaptive value guard.
* **Stage 2 — new features (v7.2, DONE except where noted):**
  1. DONE `on_error="skip"|"null"` for `map` (+ `extract_detailed`);
  2. DONE `:has()` / `:contains()` in the CSS engine (lite) and `T.kv()`;
  3. DONE `T.url()`, `js_json(...)`;
  4. DONE `T.sections(header, items)`;
  5. DONE `extract_batch` (own thread pool, per-document errors); NOT done: raw/columnar output;
  6. DONE `distill()` and the zero-IO repair loop (`repair_request` / `with_selectors` / `extract_or_repair`); NOT done: Markdown output of distill.
* **Stage 3 — measurements first:** per-node `class`/`id` cache or fused traversal in `lite` (decide with `benches/stages.rs`).

## 1. Adaptive mode — v1 done (`crates/core/src/adaptive.rs`)

`T.css(sel, adaptive="key", threshold=...)` -> IR `Op::Adaptive`. Generic over `Dom` (scraper, lite), fingerprints in a
`Store` (`MemoryStore`, `FileStore` = JSON file, atomic writes). Scoring weights: tag .15, attributes+classes .25,
text .20, tag path (LCS) .15, parent .10, siblings .05, children .10; candidates are ranked cheaply without text first,
then the top 8 are scored in full.

Next:

* `find_similar`-style op that returns *all* similar elements (lists of cards), not just the best one;
* tune/expose weights, report "relocated with score 0.81" (a `plan.adaptive_log()`), optional `on_relocate` callback;
* refresh fingerprints periodically (not only on first success / after healing);
* a labelled corpus of real redesigns to tune the default threshold (0.5 is a starting point, not a measurement).

## 2. Faster paths (driven by `docs/architecture/benchmarks.md`)

* measure first: `cargo bench --bench stages`, `Plan.run_native()`, `bench_python.py` "where the time goes";
* per-node `class`/`id` cache or one fused traversal per record (field selectors re-scan attributes today);
* cheaper Python result construction; `raw="json"` / columnar output for `extract_many`;

* done: stream pre-filter (tag/id/class from one attribute scan, `Selector::may_match`) before any ancestor matching;
* selector index by rightmost tag/class so each element is tested against a few selectors, not all of them;
* skip relative selectors while no record is open; intern attribute names; `Dom::attr` returning `Cow`;
* fuse selectors of one schema into a single traversal on DOM engines;
* `extract_many`: only parsing+extraction run in parallel, conversion to Python objects is serial under the GIL —
  a `raw="json"` mode or columnar output would lift that ceiling.

## 3. More engines (each is one `impl Dom` behind a feature)

* `tl`: very fast lenient DOM (needs parent links computed on top);
* `lol_html`: Cloudflare's streaming rewriter as an alternative stream backend;
* lexbor (C) for maximum DOM speed;
* XML / feeds (`engine-xml`), JSONPath on the `json` engine.

## 4. Distribution

* build several wheel flavours (`oxide-parse` = scraper+json, `oxide-parse-fast` = lite+stream+json) and publish them
  as extras via a thin meta-package;
* `python -m oxide_parse engines` command printing what the installed wheel contains.
