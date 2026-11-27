# Response to the code audit

Every item was checked against the code **and the HTML5 specification** before changing anything.
Legend: ✅ accepted, ✏️ accepted with changes, ❌ rejected (reason given).

## Part 1 — bugs

| # | Item | Verdict |
|---|---|---|
| 1.1 | `nth_matches` overflow / `i32::MIN % -1` panic | ✅ computed in `i64`; regression test with extreme values |
| 1.2a | `<!-->` / `<!--->` swallow the document | ✅ fixed in `skip_declaration` |
| 1.2b | `<div data-id= class="x">` "loses" `class` | ❌ HTML5 "before attribute value state": whitespace after `=` is skipped and `class="x"` *becomes the value* of `data-id` — browsers and html5ever (`scraper`) do exactly this. The suggested patch would make `lite` disagree with `scraper`. |
| 1.2c | `<div class=card/>` keeps the `/` | ❌ in the unquoted value state `/` is part of the value (`class="card/"`), and the tag is *not* self-closing; this is browser behaviour (the well-known `<a href=x/>` quirk). Kept. |
| 1.3a | `self_closing` ignored (`<svg><path/>`) | ✏️ honoured for **void and foreign (svg/math) elements only**. For ordinary HTML elements the spec ignores the slash (`<div/>` opens a div), so honouring it everywhere would diverge from browsers/`scraper`. |
| 1.3b | stray `</div>` in a table cell closes the page div | ✅ end tags now follow the spec's scoping: block elements use the scope boundaries (`td`, `th`, `caption`, `table`, `template`, ...), table elements use table scope, generic end tags are stopped by "special" elements; `p`/`li`/`dd`/`dt`/headings have their own scope rules |
| 1.4a | duplicate attribute: last wins in the stream pre-filter, first in `find_attr` | ✅ first wins everywhere (HTML5); test across all three engines |
| 1.4b | `<script>`/`<style>` text leaks into `.text()` | ✏️ ancestors' `.text()` now leaves script/style content out **in all engines** (they used to agree on including it); the text of a script/style element *itself* is kept, so `T.css("script#__NEXT_DATA__").text()` works |
| 1.4c | field selectors "leak" above the record root | ❌ by design: it is `querySelectorAll` semantics (also `scraper`'s): a selector is matched against the whole document, the element only scopes the candidates. Documented. |
| 1.5a | `parse_number` (EU decimals, space thousands, `Model-3`, `-.5`) | ✏️ rewritten (`normalize_number`), also handles `1.299.500`, nbsp/narrow nbsp; verified on a table of cases |
| 1.5b | `int` loses precision above 2^53 | ✅ exact `i64` parsing (`parse_int`) |
| 1.5c | boolean attributes | ✏️ new op `has_attr` (see case 1); `exists`/`bool` semantics unchanged |
| 1.5d | adaptive overwrites the reference on first relocation | ✏️ the baseline fingerprint is never overwritten; the last healed one is stored next to it (`key#healed`) and the best of the two is used. Returning a single element stays a documented limitation (list relocation is on the roadmap). |
| 1.6 | code injection in `decompile` | ✅ unit ops are whitelisted, `name` must be an identifier |

## Part 2 — user cases

| # | Case | Result |
|---|---|---|
| 1 | boolean attributes | ✅ `T.has_attr("disabled")` |
| 2 | `default` on empty text | ✅ `T.non_empty()` (empty / whitespace string, empty list or map -> `None`) |
| 3 | text glued across `<br>`/blocks | ✅ `T.text(sep=" ", strip=True)` (DOM engines; not streamable, `auto` falls back) |
| 4 | exclude noisy subtrees | ✅ `T.text(without=".ad, .promo")` + script/style rule above |
| 5 | JSON inside HTML | ✅ `T.parse_json()`, then `["props"]["pageProps"]["price"]`; allowed in streamed tails |
| 6 | ops after `follow(...)` | ✅ moved into the follow's sub-schema at compile time; they run on the *fetched* page's result |

## Part 3 — tests

The suggested tests could not be used as written (`extract` needs the `meta` argument, `Value` is not indexable,
the first test only parsed the selector, and two expectations contradicted the HTML spec). Corrected versions are in
`crates/core/tests/basic.rs` (`audit_*`) and `crates/core/tests/engines.rs` (`audit_*`), plus Python tests.

---

# Review of "Аудит и оптимизация v4"

| # | Item | Verdict |
|---|---|---|
| 1.1 | `audit_numbers` reads attributes at the document root | ✅ real bug in my test (the root is `<html>`). ✏️ The proposed replacement would not work either: `map` over a *single node* iterates its **children**, not the node. Fixed by starting every pipeline with `css("p")`; the test now also covers the fast numeric paths. |
| 2.1 | `follow(...).default(..)` loses the default when no request is made | ✅ real bug. ✏️ Re-adding `Default`/`Required` copies on the outer pipeline would drop everything after them (`.default("x").upper()` gives `"x"`, not `"X"`). Instead `Follow` now carries `on_null`: the whole tail that moved into the sub-schema, executed on `null` when no request is made. |
| 2.2a | `has_attr` not streamable | ✅ `Take::HasAttr`. ✏️ the proposed `field_value` would return `null` when nothing matches, the DOM engines return `false`; both now agree. |
| 2.2b | `map`/`filter`/`if`/... after `parse_json` not streamable | ✅ `is_value_expr`: nested expressions made only of value operations are allowed in streamed tails. |
| 3.1 | `stream` evaluates all selectors on every element | ✅ matches the stage analysis. Implemented as a per-element `Matcher` (memo + epoch) and lazy `id`/`class` extraction; a selector is evaluated only if an unfinished capture asks for it. Written as a separate struct instead of the proposed `macro_rules!` so the borrow checker sees disjoint fields. |
| 3.2 | `normalize_number` allocates for plain numbers | ✅ allocation-free fast path in `parse_number`/`parse_int`; it only accepts strings that parse identically, everything else goes to the full normaliser. |
| 3.3 | intern dict keys in `to_py` | ✏️ adopted, but the claim is only partly right: `PyString::intern` still creates a temporary string per call; what it buys is *shared* key objects (memory, deallocation, identity comparisons). Measure with `Plan.run_native()`; a per-call key cache is the next step if it matters. |

All of these are unmeasured: re-run `benches/bench_python.py` and `cargo bench --bench stages`.

---

# Review of "Ревью и доработки v5"

| # | Item | Verdict |
|---|---|---|
| 1 | "compiles and passes on the first try" | ⚠️ a static opinion, not a build: there is still no compiler in the authoring environment. Please run `cargo test --features full`. |
| 2.1 | cache interned `PyString` keys per call | ✅ as proposed; ✏️ `extract_many` shares one cache across all its results |
| 2.2 | `T.index()` inside a tail (`default(T.index())`) is `null` in `stream` | ✅ real mismatch with the DOM engines. `run_tail` now takes the record index; test `stream_preserves_item_index_in_tail_ops` runs scraper / lite / stream |
| 2.3 | chained `follow(A).follow(B).strip()` | ✅ real problem (the second follow was never fused). ❌ the proposed loop does not fix it: it stops at the last follow (no tail), so the first one is never fused, and its test passes only because the first page's result is discarded and the URLs are never checked. Implemented right-to-left fusion instead: the later follow moves into the earlier one's sub-schema, so its `request` is evaluated against the page the earlier one fetched, and when the first request is null the chain stops while what follows the last follow still applies to null. The new test asserts the two requested URLs. |

Semantics worth knowing: `follow(...)` ignores the value before it and evaluates its `request` against the *context*
(the current element / document). In a chain the context of the second follow is the first fetched page; the first
follow's own schema value is discarded. For "fetch A, read a field of A, then fetch B built from it" nest the second
`follow` inside the first one's `schema` instead.

---

# Review of "Отчет по бенчмарку v6 / аудит кода / план v7" — stage 1 (fixes and work on existing code)

| # | Item | Verdict |
|---|---|---|
| 2.1 | `<script src=x />`: tokenizer is not raw, tree rules keep the element open | ✅ real (HTML ignores the slash on `<script>`). ✏️ a blanket "always raw" would break `<svg><title/>`: the tokenizer now counts open `<svg>`/`<math>` and honours the slash only there, matching events.rs |
| 2.2 | `Select -> SelectFirst` before `text/attr/html` | ✅ semantics are identical (all of them read the first node); also `text_opts`, `has_attr`, `inner_html`. ❌ the suggested removal of `SelectFirst \| First` is wrong: on a node `first` means "first child", the rule would change `css(a).first().first()`. Test added. Speed-ups of "15–30%" are unmeasured. |
| 2.3 | stream rejects `css(..).first().strip()`; `meta` ignored | ✅ both. ✏️ the proposed "after `select_first` everything is text" is too broad: `count`, `first`, `exists`, `not`, `key` mean something else on a node (e.g. `count` = number of children). A whitelist of operations that treat a node like its text is used instead. `meta` works in streamed tails and as a field (`Pure`). |
| 2.4 | O(N²) sibling scan in `adaptive::fingerprint` | ✅ `Dom::prev_sibling/next_sibling` (default O(siblings), O(1) in `lite` and `scraper`) |
| 2.5 | unbounded recursion in XPath / CSS parsers | ✅ depth limits (64 for expressions, 32 for `:not` nesting); relevant because the release profile uses `panic = "abort"` |
| 2.6 | `text(sep="")` roundtrip | ✅ `sep=""` is the plain `text()` (the same result) |
| step 1 | stream early exit | ✅ when every top-level field is a single capture (no list, no `count`) and all are done and closed, reading stops. |
| step 2 | `may_match` pre-filter in `lite` | ⏸ postponed, not done: `matches` already checks tag first and reads the attributes once per selector, a pre-filter would read them twice on hits; the claimed 1.5–2x is a guess. A per-node `class`/`id` cache in the arena is the real candidate, to be decided with `benches/stages.rs`. |
| step 4 | adaptive "Value Guard" | ✅ fingerprints are written only after the whole extraction succeeded and only from pipelines that ended with a value. |
| 3.2 (concepts) | "in `lite`/`scraper` a selector is scoped to the card" | ❌ not the case: `element.select(sel)` matches the *whole* selector against the document (`querySelectorAll` semantics, also `scraper`), the element only limits the candidates; stream behaves the same. Intentional. |

Not started (stage 2, new features): `on_error` for lists, `:has()` / `:contains()`, sibling grouping, `T.kv()`, `T.url()`,
`js_json`, `distill`, zero-IO schema repair, `extract_batch`.

---

# Review of "Аудит oxide_parse v7.2 и план работ v7.3–v8.0" (released as v7.3)

The author's code dump was diffed against this tree first: it contains only the small fixes the author had to make
(`find_assignment` must also reject a preceding `.`, test imports, the `full` feature in `pyproject.toml`, a safer example);
they are adopted. (`markup/mod.rs` differs only because of the markdown fence of the dump itself.)

| # | Item | Verdict |
|---|---|---|
| 2.1 | `Key` / `At` clone the subtree at every step | ✅ real, and the dominant cost of `05_hybrid`. `swap_remove` on the owned value. |
| 2.2 | `T.kv()` searches the whole document (`//*`) | ✅ real: an absolute path ignores the current card. Now `.//*`; regression test inside `.map()`. |
| 2.3 | `default(...)` launders a failed adaptive pipeline | ✅ real: an empty `<span>` became the reference because `default` made the pipeline "succeed". The pipeline's pending fingerprints are dropped when the value is missing *before* `default`. |
| 2.4 | `<script>`/`<template>` between cards break the fold in `distill` | ✅ they are skipped while grouping. |
| 2.5 | JS comments in `jsjson::balanced` | ✅ and ✏️ extended: the proposed patch only fixed the bracket scan, but the text was then still not JSON. Comments and trailing commas are now stripped (outside strings) before parsing; JS-only syntax (single-quoted strings, unquoted keys, `undefined`) still reports an error. |
| 2.6 | XPath sibling axes build a vector of siblings | ✅ `Dom::next_sibling` / `prev_sibling` (O(1) in `lite` and `scraper`); same nearest-first order. |
| 3.1 (v8.0) | `on_error` in the stream engine | ✔ good v8.0 item (the per-record failure point is in `finish()`). |
| 3.2 (v8.0) | stream "leaks" ancestors outside the card, `lite`/`scraper` do not | ❌ premise is wrong: `element.select(sel)` matches the whole selector against the document in `lite` and in `scraper` too. Cutting the ancestor chain at the card would make `stream` *differ* from the DOM engines. The test `v73_selectors_inside_a_card_still_see_ancestors_outside_it` pins the behaviour of all three. |
| 3.3 / 3.4 (v8.0) | fused per-card walk, JSON parse cache | ✔ both are supported by the benchmark (fan-out 6.4 -> 11.7 ms; repeated `serde_json` parses). Not in v7.3. |

The "3-4x for `05_hybrid`" in the checklist is an estimate; please measure.

---

# Review of "Технический отчет v7.2: разбор бенчмарков, аудит кода и план v8.0" — part 3 (plan for v8.0)

| # | Item | Verdict |
|---|---|---|
| 3.1 | `on_error` inside `stream.rs` | ✅ as proposed: `Top::List` carries the policy, `analyze_top` takes `Map` and `MapSafe`, failing records are quarantined in `finish()` and reach `Session::errors()`. Cross-engine test (`tests/v8.rs`) compares data and `errors` with scraper and lite. |
| 3.2 | `min_depth` / card isolation in `stream` | ❌ the premise is wrong: in `lite` and `scraper` the selector of a field is matched against the *whole* document (`Selector::matches` walks `parent()` up to the root); the record only scopes which elements are candidates. `stream` already does the same. Isolating only `stream` would make it disagree with the other two engines. Pinned by `field_selectors_use_query_selector_all_scope_in_every_engine`. |
| 3.3 | fused record extraction in `lite` | ✏️ implemented in the executor, not as a `lite`-only rewrite: `Dom::select_multi` + `Eval::prefetch`, so any engine can opt in (`MULTI_SELECT`). The gain comes mainly from reading `id`/`class` once and pre-filtering with `may_match`; the tree walk itself was never the dominant cost. The "2-2.5x" claim is unmeasured. |
| 3.4 | cache of the parsed `__NEXT_DATA__` | ✏️ keyed by the text (not `(node_key, op)`, which breaks across `follow` documents and `run_tail`), 512+ bytes only; plus `json_path`, which avoids converting the whole document to `Value` when a field needs one leaf. |
| user | `follow(url=...)` | ✅ removed on request: only `request` remains (Python and IR). |

---

# Review of "План оптимизации v8.1 и дорожная карта v8.1-v9.0"

| # | Item | Verdict |
|---|---|---|
| P1 | `memchr` in `find_end_tag` | ✅ as proposed. Equivalent to the byte loop: in the normal state (and for every raw-text element but `script`) only `<` starts a transition, in the escaped states `<` or `-`. Unit tests added. |
| P2 | flags + counters in `events.rs` | ✏️ partly. Flags on the stack and `open_foreign` are in. The `open_p` counter already existed, `open_table` / `open_select` would never be read (dead code), and the "hot" walks are rare: an end tag that matches the top of the stack returns before any scan, `in_foreign()` runs only for `<x/>` on a non-foreign tag. Expect a small gain; the conformance tests must stay green. |
| P3 | `select_multi` overhead | ✏️ the allocation point was right, the others not: the walk already stops when every first-only selector has its match (`live` counter) and the attributes are parsed once per element, not once per selector. Fixed: no allocation per record (bitmask in `lite`, buffers reused across the records of a `map`), fusion from four leading selectors. Selectors that want *all* matches stay supported: a plain `select` would walk the whole subtree anyway. |
| P4 | JSON cache by `node_key` | ✅ with adaptations: there is no `Val::Json`; the cache is `HashMap<usize, Rc<serde_json::Value>>` (at most 16 big documents), and the `Text` before `parse_json` is dropped only right after a `select_first` (elsewhere `text` errors on a string while `parse_json` would accept it). |
| P5 | `parse_named_entity` | ✅ the premises were checked against the table: 106 legacy names, 2..=6 characters, no one-character names. |
| F1 | Markdown | ✅ implemented on a `Dom::walk` event stream instead of the arena, so `scraper` (the `html` default) works too. |
| F2 | `:scope` | ✏️ `lite` only. In `scraper` `:scope` means the root when no scope element is given, i.e. something else; the plan's `stream` change (frame index) was not done. Both refuse the selector at compile time and `auto` / `html` fall back to `lite`, so no engine answers differently silently. |
| F3 | Adaptive v2 | ✅ `relocate_list` (best match + same-parent, same-tag siblings above `0.85 * threshold`) and `samples`. Samples are filled from the first values seen (three distinct, 80 characters). |
| F4 | `br` and streamed `text(sep, strip)` | ✅ `br` replaces the separator around it. `without=` is still not streamed. Stream joins the text *events*, the DOM engines the text *nodes*; they agree on the tested shapes. |
| - | not done | `:has` tag pre-filter, `<base href>` in `T.url`, nested `body` in `sections`, cache by `(node_key, var)` for `js_json`. |

---

# v8.2: closing the leftovers

| Item | Result |
|---|---|
| `:scope` in `stream` | ✅ as proposed (`scope_depth`, `is_scope`: `i + 1 == scope_depth`). One addition: the matcher caches a selector's result per element, which is wrong once the same selector is checked for two open records, so the cache also stores the scope it was computed for (test with nested records). |
| `<base href>` in `T.url()` | ✅ `Dom::base_href` (first `base[href]`, default method, so `scraper` and `lite` share it) and a capture in `stream`. Two consequences handled: schemas with `url()` are not stoppable in `stream` (the head must be read in full), and the stream tail evaluator receives the captured href. Priority: `url(base)` > `meta["base_url"]` > `<base href>`. A `<base href>` given as a path (`/shop/`) is not combined with the document URL, because the engine does not know it; give `meta["base_url"]` for that. |
| `js_json` cache | ✅ keyed by node and `var`, at most 16 scripts, 512+ bytes only (the `Text` before `js_json` is already dropped after `select_first` since 8.1). |
| `MemoryStore` serialization | ✅ also for `FileStore` (default trait methods), plus `Plan(..., adaptive_state=...)` because a plan's store is fixed at compile time. |

---

# Review of the v8.2 side effects (8.2.1)

| Point | Result |
|---|---|
| `with_selectors` keeps stale fingerprint of the rewritten key | ✅ confirmed (`store.contains(key)` stays true, no new reference is ever learned). Rewritten keys and their `#healed` entries are dropped for in-memory stores; a file store is shared with the original plan, so it is not edited. |
| early exit with `url()` | ✅ held back only until `<base href>` / `<body>`. |
| `<base href>` relative to `meta["base_url"]` | ✅ implemented, which reverses the "meta wins" rule of the 8.2 request: with an absolute `<base href>` the page URL no longer changes the result. Say so if you want the old override back (it would be a `meta["base_url_override"]`). |
| `follow` hands `base_url` down | ✅ only when the schema did not set `base_url` itself. A relative followed URL becomes a relative base and `url()` then reports "not absolute". |
| samples | ✅ up to three scalars of the first record. |
| `is_none_or` | ✅ replaced. |

---

# v8.3 design notes

| Item | Notes |
|---|---|
| streamed `without` | `skip_until_depth` per collector (not global): two fields may skip different things. The `without` selector is checked with the matcher cache of the element, so a selector shared with a capture costs one match. Raw `<script>`/`<style>` text was already excluded. Difference to remember: the stream joins text *events*, the DOM engines text *nodes*; they agree on the tested shapes. |
| `group_by` | Implemented as one fused operation (`group_by | map`), not as a new value kind: a bare `group_by` is an error. For the body, selectors over *exactly the current group* also match the group's own nodes (otherwise `T.css("a")` would miss links that are siblings of the header); anything nested inside keeps the normal descendant-only rule. `sections` keeps its output shape. Not streamable. |
| `json_ld`, `meta_tags` | Operations in the core (not Python sugar over `parse_json`), because skipping broken blocks, `@graph`, `@type` lists and repeated meta names are awkward to express with the existing operations. They compile their own selectors (`script[type*="ld+json"]`, `meta[content]`). Not streamable. |

---

# v8.4 design notes: repairs inside the session

| Question | Decision |
|---|---|
| Who re-runs a document after a repair? | The session, from the source it kept (only for documents with unresolved misses). The plan stays immutable: the new selectors are *overrides* compiled per run for the engine in use. |
| What happens to `follow` requests of a re-run document? | A request with the same `(sub-schema, params, meta)` as an old one keeps its id (in flight or already fetched); the others are dropped with everything they issued. So a model can answer while fetches run. |
| Why are repairs optional for `result()`? | A crawler without a model (or one that gives up) must still finish. `resume(id, None)` declines; the Python drivers decline a key after two wrong answers, so the loop always ends. |
| What is "the local distill"? | The element's old *parent* (tag, id, classes, path from its fingerprint) is looked up in the page as it is now (id match, class overlap >= half) and distilled on its own, max 8 000 characters. No match: the whole page, 12 000 characters. |
| Why does a failed answer keep the old reference? | Replacing the baseline only when the new selector actually matches means a second repair still sees what the element originally looked like. |
| Not done | Re-using fetched sources of a root re-run's *changed* requests, a `fetch` cache keyed by request, and repair requests for the `stream` engine (it has no adaptive selectors). |

---

# Review of the v8.4 risks (8.4.1)

| # | Point | Verdict |
|---|---|---|
| 1 | `PendingFp` write mode (`FpWrite`) | ❎ not needed: the premise (`heal: false` for an override) does not match the code. An overridden selector's match is pushed with `heal: true`, which overwrites the baseline, and `begin_repairs` already removes the stale `{key}#healed`. A three-state enum would be tidier than a bool, but changes nothing in behaviour; left as is. |
| 2 | errors per document | ✅ already so: `DocRec.errors` is replaced when a document is re-run and `errors()` is rebuilt from the records. |
| 3A | orphaned requests | ✅ already so: `install_run` drops the old requests of the re-run document that the new run does not repeat (and everything they issued); `origin` plays the role of `owner`. |
| 3B | `fill` depth / unresolved `Pending` | ✅ done, with one change: the limit counts nested *follows*, not structural depth (a deep JSON value is not a nesting error). |
| 4 | invalid CSS from a model | ✅ a real gap (the driver would raise). Done as proposed, with the engine's parser as validator. |
| 5 | tokens | ✅ distil once per container (cache by parent fingerprint) and batches in Python. The `html` key of the proposal is the existing `skeleton`. |
| 6 | Rust < 1.82, `engine-lite` isolation | ✅ `is_none_or` was already replaced in 8.2.1; the `engine-lite` dependency is already confined to one function (`make_skeleton`). `cargo check` for the two feature sets still has to be run by you. |
| bonus | stream `without` + `br`, `with_selectors`, `meta_tags(values=)` | the first two exist since 8.3 / 8.2.1 (`Collector.skip`, reset on close; rewritten keys are dropped with their `#healed`); `values=` added. |

---

# v8.5 design notes

| Item | Decision |
|---|---|
| `slice` on a single node | the children of that element (as in the plan), so `T.css("ul").first().slice(2)` works; use `T.css("li")[2:]` for the matching items. |
| `distill` for `scraper` | the plan allowed `Dom::walk`; the distiller works on the arena, so `scraper` serialises the element and re-parses it with `lite` (needs the `lite` engine in the build, otherwise a clear error). `lite` itself skips that round trip (`distill_node`). |
| streamed `meta_tags` | exit at `<body>`, as planned; this is a deliberate difference to the DOM engines for `<meta>` inside the body, covered by a test. A schema with other single captures keeps reading until they are found *and* the body has started. |
| `validate_ir` | structural check in pure Python (works without the native module) + a compile for the chosen backend when it is present. Unknown fields are reported, although serde would ignore them: for a model's output a typo like `sepp` should not pass silently. |
| IR table drift | `ir_spec.py` lists every operation; the test suite compares it with `ir.rs` by regenerating the schema, and `scripts/gen_ir_schema.py --check` is meant for CI. |

---

# v9.0 design notes (stage 2)

| Item | Decision |
|---|---|
| Output shape of `from_crawl4ai` | the bare list, as Crawl4AI returns it; `wrap="items"` gives `{"items": [...]}`. A schema without `baseSelector` reads the fields from the document. |
| `nested` | `T.css(sel)[:1].map({...}).at(0)` returns one object or `None`. `list` and `nested_list` use `.map({...})`. |
| `regex` | Uses the schema's `group` (default 1, with 0 for the whole match), matching Crawl4AI 0.9.6. Look-around in the pattern triggers a migration warning because the Rust regex engine cannot compile it. |
| XPath strategy | selectors that end in `/@attr`, `/text()`, `string(...)` or `normalize-space(...)` already yield strings: they become `.at(0)` (or stay a scalar), others `.text().strip()` / `.attr(...)`. |
| `html` | Uses `html()` to include the selected tag. Serialization details depend on the parser. |
| Missing scalar fields | Crawl4AI omits the key. Migrated records keep it with `None` unless the field supplies a default. |
| selector maps | Parsel's `::text` takes an element's own text nodes, `T.text()` all text below it: documented as an approximation. `_base` (or `$base`) marks a record list, which Parsel has no syntax for. |
| strictness | `computed` and `source` fields raise `MigrationError` rather than being dropped: a silently different result is worse than a clear refusal. |
| CLI `run` on a Crawl4AI file | refuses with a pointer to `migrate`. |
| package version | 0.9.0: "9.0" in this log is the feature level, the wheel stays below 1.0 until the documentation stage is done. Change it in the four places (a test checks them). |
| not in this stage | the documentation tree (`docs/guide`, `docs/ai`, `llms.txt`) from part 3 of the plan. |
