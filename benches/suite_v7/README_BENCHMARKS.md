# `oxide_parse` Extended Benchmark Suite (v7.2+)

This suite serves two goals:
1. **Competitor comparison** against `frostwork`, `selectolax` (lexbor), `lxml`, `parsel`, `scrapling` (standard + adaptive), `beautifulsoup4`, `crawl4ai`, `markdownify`, and `html2text`.
2. **Internal bottleneck profiling** across specialized workloads: Early Exit, deep CSS combinators, XPath 1.0, hybrid HTML+JSON (`__NEXT_DATA__`), dirty HTML text/entity decoding, adaptive selector relocation, Rayon batch extraction, HTML-to-Markdown, and Python object construction overhead.

---

## Quick Start

### 1. Files (`benches/suite_v7/`)
- `README_BENCHMARKS.md`
- `competitors.py`
- `generators.py`
- `suite_scenarios.py`
- `run_all_benchmarks.py`

### 2. Installing Competitors
Competitor libraries live in the `bench` optional extra (Scrapling and Frostwork require Python 3.10+). The runner automatically detects installed packages and notes missing ones in the report.

```bash
uv sync --extra bench
```

### 3. Running Benchmarks

```bash
# Standard run
uv run --extra bench python -u benches/suite_v7/run_all_benchmarks.py --output benchmarks/v7_run

# Quick smoke run (catalog sizes 100 / 1000, repeat=3)
uv run --extra bench python -u benches/suite_v7/run_all_benchmarks.py --quick --output benchmarks/v7_quick

# Full run with CPU cooldown pauses
uv run --extra bench python -u benches/suite_v7/run_all_benchmarks.py --full --cooldown 2.0 --output benchmarks/v7_full

# Run selected scenarios by substring
uv run --extra bench python -u benches/suite_v7/run_all_benchmarks.py --filter adaptive,xpath,bottleneck
```

The output directory receives two files:
- `results.json` — raw timings, package versions, machine/extension metadata, skips, and errors.
- `BENCHMARK_REPORT.md` — formatted Markdown summary with comparison tables.

---

## Scenarios

| # | Scenario | What it tests | Competitors |
|---|---|---|---|
| **01** | `catalog_scaling` | Catalog scaling (100 / 1,000 / 10,000 cards) with `noise=True` and `False`, plus `extract()` vs `run_native()`. | `lite`, `stream`, `scraper`, `frostwork`, `selectolax`, `lxml`, `parsel`, `scrapling`, `bs4` |
| **02** | `early_exit_head` | Extracting `<title>`, OpenGraph meta tags, and the first item near the top of a ~2.5 MiB page. | All HTML engines |
| **03** | `deep_css_combinators` | Deep nesting (25 levels) + complex selectors (`> + :not() :nth-child() [attr^=]`). | `lite`, `stream`, `scraper`, `selectolax`, `parsel` |
| **04** | `xpath_torture` | XPath 1.0 queries: predicates, `contains()`, `following-sibling::`, `parent::`, `count()`. | `lite`, `scraper`, `lxml`, `parsel` |
| **05** | `hybrid_html_json` | SSR Next.js page: HTML + JSON inside `<script id="__NEXT_DATA__">` (`.parse_json()` vs parser + `json.loads()`). | `lite`, `stream`, `scraper`, `selectolax + json`, `lxml + json` |
| **06** | `text_and_entities` | Cluttered article markup: stripping ads/scripts (`without=".ad"`) + entity decoding. | `lite`, `scraper`, `selectolax`, `bs4 + lxml` |
| **07** | `adaptive_redesign` | vs `scrapling(adaptive=True)`: hot path (match) and cold relocate path (redesigned DOM on 400 and 1,600 nodes). | `oxide_parse [lite]`, `oxide_parse [scraper]`, `scrapling` |
| **08** | `batch_parallel` | 32 documents in parallel (`plan.extract_many(docs)`, Rayon, GIL released) vs single-threaded loops. | `lite`, `stream`, `scraper`, `selectolax` |
| **09** | `internal_bottlenecks` | Internal profiler: fast US price vs localized price, 2 selectors vs 11 selectors (including misses). | `lite`, `stream`, `scraper` |
| **10** | `markdown_rag` | HTML -> Markdown for RAG (`T.css("body").markdown()` vs `markdownify` and `html2text`, 300 blocks). | `lite`, `scraper` |
| **11** | `scrapling_bench` | Scrapling benchmark workloads: 5,000 nested elements and `find_similar()` vs `adaptive` list on 500 cards. | `lite`, `stream`, `scraper`, `scrapling`, `frostwork`, `selectolax`, `parsel`, `lxml`, `bs4` |
| **12** | `selectolax_bench` | Selectolax `examples/benchmark.py` workload: strip `script`/`style`, full `body` text + `title` + 800 links. | `lite`, `stream`, `scraper`, `selectolax`, `lxml`, `parsel`, `bs4` |
| **13** | `frostwork_bench` | `frostwork.Page` workloads: grouped offers (~2,000 items) and `first_only` Early Exit on ~1.6 MiB HTML. | `lite`, `stream`, `scraper`, `frostwork`, `selectolax`, `parsel`, `scrapling` |
| **14** | `parsel_quotes_bench` | Scrapy/Parsel quotes catalog pattern (500 quote cards with nested tags). | `lite`, `scraper`, `frostwork`, `selectolax`, `lxml`, `parsel`, `scrapling`, `bs4` |
| **15** | `crawl4ai_nested_bench` | Crawl4AI nested e-commerce catalog (30 categories x 4 products with nested details/features/reviews/related). | `lite`, `scraper`, `crawl4ai`, `selectolax`, `parsel` |

## Correctness & Timing Rules

- Every extracted output is verified outside the timed section.
- Before each `adaptive` relocation run, a fresh plan learns the reference page outside the timer; the timer covers parsing the redesigned HTML and relocating elements.
- `Modest` is no longer available in `selectolax 1.x`; missing optional packages are recorded cleanly in the report.
- `--repeat` sets sample count per case, `--cooldown` sets sleep seconds between scenario groups, and `--quick` caps catalog scaling at 1,000 items while running all specialized workloads at full size.
