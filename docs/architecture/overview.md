# Architecture

```
                 Python DSL (T)                       JSON / YAML file
                      │ to_ir                               │
                      └───────────────► Expr (IR, serde) ◄──┘
                                           │  Plan::compile::<D>
                                           ▼
                   Plan<D::Sel>  (CSS compiled once, deduplicated; XPath parsed; peephole optimiser)
                                           │
          ┌────────────────────────────────┴───────────────────────────────┐
          ▼ DOM engines                                                    ▼ stream engine
   Eval<D: Dom> (generic, monomorphised)                       analyse schema -> capture program
   scraper │ lite │ json                                        one pass over markup::events
                                                                 (tails run on a NoDom via Eval)
```

## Layers

| Layer | Where | Knows about |
|---|---|---|
| IR | `ir.rs` | nothing but data |
| Compiler / optimiser | `plan.rs` | `Dom::compile_css`, `Dom::XPATH` |
| Executor | `exec.rs` | `Dom` (never a concrete engine) |
| XPath | `xpath/` | `Dom` navigation primitives |
| Adaptive | `adaptive.rs` | `Dom` introspection (`name attrs parent children`) |
| Markup runtime | `markup/` | bytes -> events -> arena / stack frames |
| Engines | `engines/` | one `impl Dom` (or a stream program) each |
| Registry | `engine.rs` | cargo features -> names |

## Adding a DOM engine

1. Add a cargo feature in `crates/core/Cargo.toml` (`engine-foo = ["dep:foo"]`) and forward it in `crates/py/Cargo.toml`.
2. Create `engines/foo.rs`, implement [`Dom`](../../crates/core/src/dom.rs):
   * mandatory: `compile_css`, `root`, `select`, `text`, `html`, `attr`, `to_value`, `node_key`;
   * for XPath set `const XPATH = true` and implement `name`, `attrs`, `direct_texts`, `parent`, `children`
     (and `document_children` if the document has several top-level elements);
   * `Node` must be a cheap `Clone` handle; `Sel` must not borrow the document.
3. Add `impl Engine for Plan<FooSel>` (copy the 20 lines from `engines/lite.rs`) and a `compile()` function.
4. Register the name in `engine.rs` (`available()` and `compile()`), add it to `tests/engines.rs`.

The executor, the optimiser, XPath, `follow`, the Python bindings and `explain()` need no changes.

## Stream engine rules

`engines/stream.rs::analyze` accepts per root field:

* `css(s) | text|attr(a)|exists|count` (+ value-only tail), `css(s)` + `first` is implied;
* `css(item) | map({field: capture | index | lit})` or `map(capture)` (+ value-only tail),
  where `capture` may also start at the item itself (`text`, `attr(a)`); `map(..., on_error="skip"|"null")` is
  streamed too (a failing record is quarantined while the records are built in `Run::finish`, errors reach
  `Session::errors()` through `Session::from_parts`);
* literals.

Value-only tail ops are those that need no node: `strip lower upper replace split join regex regex_all int float bool
required not count first last at exists` and `default/eq/ne/contains/starts_with/ends_with/and/or` with literal arguments.
`json_path` (the fused `parse_json()[..][..]`) is a value op as well.
Selectors may not use `+`, `~`, `:last-child`, `:only-child`, `:empty`.

## Session: fetches and repairs (v8.4)

* `Session` = root value + `outstanding` fetch `Request`s + `results` of fetched documents (inserted by `take_result`) + one
  `DocRec` per document (sub-schema, meta, origin, url, errors, misses, source while it has misses) + open `RepairRequest`s
  + `overrides`.
* `Plan::run` takes the overrides (compiled per run, `Eval::overrides`); the `Adaptive` op uses the override instead of the
  compiled selector, and a match of an overridden key replaces the stored reference (`heal: true`).
* `Engine::repair` -> `Plan::repair_with` -> `begin_repairs` (validate, record, drop the stale healed reference) ->
  `rerun_dom` per affected document -> `Session::install_run` (keeps ids of repeated requests, remaps `Pending` ids,
  rebuilds `errors` / `misses`, syncs the repair requests, fills skeletons). A failure restores the repair state.
* `distill_container` finds the old parent (`ParentFp` + path) in the current page and distils only that subtree.

## Streamed `without`, groups (v8.3)

* `stream`: `TextSpec.without` is a selector index; a `Collector` has `skip: Option<usize>` (depth of the matching element), set in
  `on_open` (checked with the matcher), cleared in `on_close`; `on_text` and `<br>` ignore a skipping collector.
* `group_by | map` -> `COp::GroupMap`: `map_items` runs with `groups_mode`, the item is `Val::Nodes(group)` and `group_cur`
  holds its node keys; `select_into` over exactly that `Val::Nodes` also tries `Dom::matches_sel` on the group's own nodes.
* `COp::JsonLd` / `COp::MetaTags` select `script[type*="ld+json"]` / `meta[content]` from the context node.

## Walk, Markdown, `:scope` (v8.1)

* `Dom::walk` emits open / text / close events of a subtree (skipping `node_key`s); `markdown::Renderer` turns them
  into Markdown, `Dom::text_pieces` into text pieces (`None` = `<br>`). `lite` and `scraper` implement it.
* `:scope`: `ElemView::is_scope`; in `lite` an `Elem` carries the node a `select` started from, in `stream` an `SView`
  carries the stack depth of the scope (`1` = the document, `rd` = the open record). `Dom::SCOPE` says whether an engine
  understands it; `Plan::selector` refuses the selector otherwise and `engine::html` retries with `lite`.
* `<base href>`: `Dom::base_href` (default: first `base[href]`), cached per `Eval`; `stream` records it in `on_open` and passes it
  to `Plan::run_tail`.
* `Store::export_json` / `import_json` (default methods), `Engine::adaptive_store()`.
* `adaptive | map` sets `multi` on the `Adaptive` op; `adaptive::relocate_list` then returns the best match and its
  similar siblings.

## Executor notes (v8)

* **Fused record fields (`lite`).** `Eval::map_items` looks at the leading selectors of the fields of a `map` body
  (`Select` / `SelectFirst` / `SelectExists`). With two or more and `Dom::MULTI_SELECT`, it calls
  `Dom::select_multi` once per record: one walk of the record subtree, `id`/`class` read once and shared by the
  `Selector::may_match` pre-filter of every selector. The results sit in `Eval::prefetch`, keyed by the record's
  `node_key`; a pipeline whose first op is one of those selectors and whose context is that record takes the
  stored result (also inside `default(...)`/`if_(...)`). Engines without `MULTI_SELECT` (scraper) run unchanged.
* **JSON.** The peephole fuses `parse_json | key/at ...` into `json_path`: navigation by reference, only the
  selected leaf becomes a `Value`. The last parsed text of 512+ bytes is cached in `Eval::json_cache`, so several
  fields reading branches of one `__NEXT_DATA__` parse it once per run.
Anything else returns `Err(reason)`; `auto` then falls back to a DOM engine.

## Invariants worth keeping

* null propagates through value operations; `default`/`required` are the only places that decide about missing values;
* every error carries a path (`$.items[3].price`);
* nothing in `core` does IO or touches Python; the GIL is released around every native call;
* all engines must produce the same result for the same (streamable) schema: add a case to `tests/engines.rs`.
