# Benchmark analysis (Python API, parse + extract, median)

Source: `benches/bench_suite.py` runs on the author's machine (100 / 1000 / 10000 products, with and without `--noise`).
"Noise" adds exactly **110 bytes per product** (305.08 − 197.66 KiB at 1000), so the +100 KiB seen at 1000 products
comes from the noise option; the base page (197.66 KiB) did not change between v2 and v3.

## What the numbers say (facts)

| 10000 products | lite | stream | scraper | selectolax | parsel | bs4 |
|---|---|---|---|---|---|---|
| ms | 77.5 | 99.9 | 165.5 | 306.5 | 1571.6 | 4099 (html.parser) |
| vs lite | 1.0x | 1.29x | 2.14x | 3.95x | 20.3x | 52.9x |
| MiB/s (incl. Python result) | 25.4 | 19.7 | 11.9 | 6.4 | 1.3 | 0.5 |

* `lite` is first at every size: 3.5–4.5x faster than selectolax, 16–22x than parsel, 51–74x than bs4.
* `scraper` (same executor, html5ever parser) is 2.0–3.0x slower than `lite` => the parser, not the extraction layer,
  is what separates them.
* **`stream` is not faster than `lite`** on this workload: 1.01x–1.38x slower; the gap grows with size
  (1.01x at 1000 -> 1.29x at 10000) and with noise (1.38x). 1000 -> 10000 scaling is 10.7x for stream vs 8.4x for lite.
* Cost of irrelevant markup (ms per extra MiB at 10000): lite 17, stream 30, scraper 114, selectolax 115, parsel 158.
  Both own engines are 4–9x cheaper per irrelevant byte than the DOM libraries, but stream pays ~1.8x more than lite.
* `extract_many` over 16 docs: 1.71x (1000) and 1.89x (10000) faster than a loop.

## Interpretation (hypotheses, NOT yet measured)

1. 21–32 MiB/s is far below what a zero-copy tokenizer can do, so most of the time is *not* tokenizing. Suspects:
   result construction (Rust `Value` -> Python objects, serial under the GIL), attribute re-scanning in selector matching,
   per-field allocations. `Plan.run_native()` and `benches/stages.rs` exist to split these.
2. Why `stream` lags: this workload touches almost every element, so "skip what you do not need" has nothing to skip;
   per element it pays a frame push, an attribute scan and 6 selector pre-filters. Frostwork-style wins come from
   many selectors over big pages with few matches, not from "extract 5 fields of every card".
3. Super-linear stream scaling pointed at per-record allocations: `finish()` built an error-path `Vec<String>` per field
   (about ten small allocations per field per record) even on success. Fixed in this version (paths are built only on error).
4. `extract_many`: only parse+extract run in parallel; copying the sources and building Python objects are serial.
   With 4 cores a 1.89x speed-up implies roughly a third of the time is serial (Amdahl), consistent with (1).

## Changes made because of it (all unmeasured until you re-run)

* tag classification once per start tag instead of ~50 string comparisons (affects lite and stream);
* open-`<p>` counter: a `<div>` no longer searches the stack when no `<p>` is open;
* stream: no allocations on the success path of `finish()`, captured strings are moved instead of cloned;
* measurement tools: `cargo bench --bench stages` (events / arena / exec-only / total), `Plan.run_native()` and the
  "where the time goes" block of `bench_python.py` (total vs native vs Python objects).

## Next, in order of expected value (decide after the new measurements)

1. cache `class`/`id` per node (arena) or fuse the selectors of one record into a single subtree traversal:
   today each field selector re-parses the attributes of every descendant of the card;
2. cheaper Python result construction (shared key strings, fewer temporaries) and a `raw="json"` / columnar output;
3. only then revisit `stream`: tag/class index so an element is tested against the few selectors that can match.

## Update (v5): lazy selector evaluation in `stream`

The stage analysis of the "v4 review" confirmed hypothesis 2: `stream` evaluated every selector of the schema on every
element, while `lite` runs the field selectors only inside the cards. v5 evaluates a selector only when an unfinished
capture needs it: outside cards only the card selector is checked, and a field stops asking once it is filled.
Expected effect: the cost of irrelevant markup should move towards `lite`'s 17 ms/MiB. **Verify** with `--noise`.

## v6 measurements (author's report) and how they were read

* `stream` is first in every scenario, 1.40–1.49x faster than `lite` and ~1.2x than the 10000+noise `lite` figure:
  the lazy selector evaluation of v5 confirmed hypothesis 2 above. Against the popular stacks (10000 products + noise):
  ~6x selectolax, ~28x parsel, 105–120x bs4.
* **Caution about the machine.** On the Intel N97 the *same* code is much slower in the long runs than in the short ones
  (lite 10000 without noise: min 62 ms, median 110 ms, max 186 ms), and 10000+noise is *faster* than 10000 without noise,
  which cannot be a property of the library. The report normalises by selectolax (a code path that did not change) and
  gets "real" speed-ups of 1.2–1.7x for `lite`; that is a reasonable estimate, but it rests on the assumption that
  the throttling hits all libraries equally, which is not measured.
* Use the new options of `benches/bench_python.py` to get cleaner numbers: `--repeat 21` (more samples),
  `--cooldown 2` (pause between cases so the CPU can recover), `--fast-only` (only oxide_parse engines, much shorter run).
  Compare minima as well as medians; compare engines *within the same run*, never across runs.

## v7.2 report (author's `BENCHMARK_REPORT.md`, new suite `benches/suite_v7`) and the v7.3 reading

* 8 of 9 scenarios are won; the only loss is `05_hybrid_html_json` (JSON inside `<script id="__NEXT_DATA__">`, 0.83x of
  selectolax + `json.loads`).
* **Cause of the loss, confirmed in the code:** `COp::Key` / `COp::At` deep-cloned the child value at every step, so
  `parse_json()["props"]["pageProps"]["products"]` copied (nearly) the whole document three times per field. v7.3
  moves the child out of the owned value instead (`swap_remove`). What is left: the JSON text is still parsed once per
  field (a cache of the last parsed script is a v8.0 idea) and converted from `serde_json::Value` to our `Value`.
* **Fan-out:** in `09b_selector_fanout_bottleneck` `lite` grows from 6.4 ms (2 selectors per card) to 11.7 ms (11), because
  every field selector walks the card's subtree again. A single walk per card (like `stream` does) is the v8.0 item.
* **Caveat about cross-version comparisons.** The v7.2 pages and scenarios are not the v6 ones (the page is 2532 KiB
  instead of 2015 KiB, other generators), so "stream 77 ms -> 42 ms" cannot be attributed to a single change
  (`select|text` early exit, lazy selectors, tag classification and the machine's thermal state all contribute).
  Compare engines within one run; compare versions only with the same suite on the same page.
