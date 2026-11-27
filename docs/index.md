# `oxide_parse` Documentation

Organized by task: guides with runnable examples, ready-to-use LLM prompts, concise references, and core internals.

## Where to Start

- **New to the library:** [guide/01_quickstart.md](guide/01_quickstart.md) (five minutes from install to working parser).
- **Looking up a `T` method:** [guide/02_dsl_cheatsheet.md](guide/02_dsl_cheatsheet.md).
- **Generating or migrating schemas with an LLM:** [ai/prompt_generate_schema.md](ai/prompt_generate_schema.md) and [ai/prompt_migrate.md](ai/prompt_migrate.md).
- **Exact signatures, IR format, or XPath functions:** [reference/python_api.md](reference/python_api.md).

## Documentation Map

### Guides (`guide/`)

| File | Covers |
|---|---|
| [01_quickstart.md](guide/01_quickstart.md) | Installation, first schema, compiling `Plan`, `on_error`, JSON/Python round-trip |
| [02_dsl_cheatsheet.md](guide/02_dsl_cheatsheet.md) | `T` cheatsheet: navigation, types, null-safety, filters, `group_by`, `kv` |
| [03_choosing_engine.md](guide/03_choosing_engine.md) | `stream` vs `lite` vs `scraper` vs `json` vs `auto`, reading `plan.explain()` |
| [04_hybrid_html_json.md](guide/04_hybrid_html_json.md) | `__NEXT_DATA__`, `window.__STATE__`, JSON-LD, and `<meta>` in one schema |
| [05_adaptive_and_repair.md](guide/05_adaptive_and_repair.md) | Adaptive selectors, fingerprint stores, LLM repair, `to_python()` |
| [06_follow_and_crawling.md](guide/06_follow_and_crawling.md) | `follow`, `paginate`, sync/async drivers, `Session` loop, `extract_batch` |
| [07_llm_rag_markdown.md](guide/07_llm_rag_markdown.md) | `distill()`, `markdown()` for RAG, Crawl4AI and selector-map migration |

### AI Prompts & Ecosystem (`ai/`)

| File | Covers |
|---|---|
| [prompt_generate_schema.md](ai/prompt_generate_schema.md) | System prompt: HTML / `distill()` to an `oxide_parse` schema |
| [prompt_migrate.md](ai/prompt_migrate.md) | Migrating from Crawl4AI, Scrapy/Parsel, BS4, Selectolax, Scrapling |
| [prompt_self_healing.md](ai/prompt_self_healing.md) | System prompt and batch payload for `repair` / `repair_batch` callbacks |

### Reference (`reference/`)

| File | Covers |
|---|---|
| [python_api.md](reference/python_api.md) | `T` methods, `Plan`, `Session`, `Detailed`, `RepairResult`, `migrate`, `prompts` |
| [ir_spec.md](reference/ir_spec.md) | JSON/YAML IR specification, `validate_ir()`, and [ir.schema.json](../python/oxide_parse/ir.schema.json) |
| [selectors_and_xpath.md](reference/selectors_and_xpath.md) | CSS selector matrix per engine and XPath 1.0 specification |
| [cli.md](reference/cli.md) | `oxide-parse` CLI: `distill`, `markdown`, `run`, `validate`, `decompile`, `migrate`, `prompt` |

### Core Internals & History (`architecture/`, `archive/`)

| File | Covers |
|---|---|
| [architecture/overview.md](architecture/overview.md) | Core layers: Tokenizer -> Events -> Arena/Stream -> Executor -> PyO3 |
| [architecture/conformance.md](architecture/conformance.md) | Conformance suites: html5lib, cascadia, Sizzle, libxml2 |
| [architecture/benchmarks.md](architecture/benchmarks.md) | Benchmark methodology and bottleneck analysis |
| [architecture/publishing.md](architecture/publishing.md) | Cross-platform wheel builds and PyPI publishing |
| [CHANGELOG.md](../CHANGELOG.md) | Release history |
| [archive/audit_v7_v8.md](archive/audit_v7_v8.md) | Archived v7-v8 audit notes |
| [archive/roadmap_v7_v8.md](archive/roadmap_v7_v8.md) | Archived v7-v9 roadmap notes |

### LLM Indexes

- [llms.txt](../llms.txt) — compact `<= 4 KiB` index following llmstxt.org.
- [llms-full.txt](../llms-full.txt) — concatenated context bundle (`README.md`, DSL cheatsheet, IR spec, prompts).

## Executable Examples in Docs

Python examples in `README.md` and `docs/guide/*.md` are executed by `pytest` via [test_docs_examples.py](../tests/test_docs_examples.py):
- every ```` ```python ```` block without `# skip-doctest` runs in order within a file namespace;
- if a ```` ```text ```` block immediately follows a Python block, stdout must match it line for line.
