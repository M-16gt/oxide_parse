# Conformance Testing for `lite` and `stream`

Conformance suite added during v7.3. Engine sources are tested against upstream expectations without altering test fixtures. Divergences remain explicit test failures and appear in the conformance report; a non-zero exit code when mismatches occur is expected.

## Running the Suite

```powershell
uv sync --extra test --extra bench

# One-time download of pinned upstream fixtures
uv run python scripts/fetch_conformance_suites.py

# Offline hash verification and JSON generation
uv run python scripts/fetch_conformance_suites.py --offline

$env:CONFORMANCE_REPORT_DIR = "$PWD/benchmarks/conformance-local"
cargo test -p oxide_parse-core --features full --test conformance -- --nocapture
uv run --extra test pytest tests/test_conformance.py -q --tb=short

# Real WHATWG HTML spec page. First run downloads and caches it
uv run --extra bench python benches/stress_whatwg_slickspeed.py --repeat 3 --output benchmarks/conformance-local/stress.json

# Cache-only run (offline)
uv run --extra bench python benches/stress_whatwg_slickspeed.py --offline --repeat 3 --output benchmarks/conformance-local/stress.json
```

Run `pytest` after the stress script to include stress timings in `REPORT.md`. To reproduce a single case, pass its parametrized ID from the report:

```powershell
uv run --extra test pytest 'tests/test_conformance.py::test_xpath_lxml[libxml2/functions:35-lite]' -vv
```

Without `CONFORMANCE_REPORT_DIR`, Rust and Python write reports under `target/conformance`. Use a dedicated directory per full run; partial pytest runs only update the Python entries for the executed tests.

## Fixture Sources

`tests/fixtures/conformance/external_suites.json` stores converted test cases alongside source URLs, revisions, and SHA-256 digests. Original files and licenses sit in adjacent subdirectories. The loader never executes external Go or TypeScript code, rejects invalid or empty downloads without overwriting existing fixtures, and verifies file hashes offline. Both `pytest` and `cargo` run completely offline.

| Source | Included subset | Cases |
|---|---|---:|
| html5lib-tests | `tree-construction/tests1`, `tests2`, `tables01`, `scriptdata01`, `entities01` | 295 |
| html5lib-tests | `tokenizer/test1`, `entities`, `numericEntities` | 485 |
| andybalholm/cascadia | Full `selectorTests` table from `selector_test.go` | 114 |
| fb55/css-select | Immutable Sizzle document-context tests | 15 |
| GNOME/libxml2 | `XPath/expr/base`, `floats`, `functions`, `strings` | 171 |
| Local cases | XPath paths, axes, unions, and functions with `lxml` oracle | 18 |

The `html5lib-tests` revision is pinned before the WPT `tree-construction` move. Dynamic Sizzle tests that mutate the DOM or require JS context are excluded, as are NWMatcher/Qwery and WPT `domxpath`. `libxml2/test/XPath/docs/simple` and `tests/simplebase` are kept for provenance; all XPath expressions run on a shared HTML document in both `lxml` and `oxide_parse`.

## What Is Verified

- **Rust (`tests/conformance.rs`)**: checks four embedded trees, CSS, XPath, external tokenizer fixtures, and tree construction (including any extra `.dat` files in `html5lib-tests/tree-construction`). `native_selector_capabilities.json` records `Selector::stream_unsupported()` output.
- **Python (`tests/test_conformance.py`)**: checks `lite == stream` across all 295 HTML inputs and 10 complex local documents (counts, text, attributes, lists, plus external CSS selectors), comparing canonical JSON bytes.
- **Upstream CSS checks**: compare `lite` against upstream expected match counts, plus differential comparison against `scraper` (`lite` vs `scraper` alone does not decide correctness; upstream expectations are recorded in JSON).
- **XPath checks**: compare both DOM engines (`lite` and `scraper`) against `lxml`. Non-zero finite floats allow `1e-14` relative tolerance; negative zero, strings, and types are checked explicitly, and `NaN`/`Infinity` are serialized via explicit `nonfinite` markers instead of invalid JSON.

## Scope & Caveats

`PASS`, `FAIL`, `ERROR`, `UNSUPPORTED`, and `KNOWN_DIFFERENCE` are tracked separately. Pass rate equals `PASS / (PASS + FAIL + ERROR)` — there is no aggregate threshold hiding individual failures.

Tree projections transparently strip `html`/`head`/`body` wrapper containers, comments, and doctypes while keeping all other elements, attributes, and text; adjacent arena text nodes are coalesced solely in the html5lib-format test dumper.

The tokenizer does not expose `Comment`/`DOCTYPE` tokens or parse error positions, so those are excluded. Non-ground initial tokenizer states and double-escaped UTF-16 cases require separate adapters and are marked `UNSUPPORTED`.

CSS exclusions are tracked in `UNSUPPORTED_CSS` (alongside cascadia extensions `!=` and `#=`). `stream` checks consult `Selector::stream_unsupported()`. Unexpected compile or runtime errors are never suppressed (e.g. CSS comments inside an otherwise supported selector remain a test error).

XPath excludes axes/functions documented as unsupported (`following::`, `preceding::`, `namespace::`, `id()`). Scientific-notation number literals are a libxml2 extension outside XPath 1.0.

`KNOWN_DIFFERENCE` lists explicit test IDs for reviewed `adoption agency`, `foster parenting`, and implicit empty `<head>` differences. Unreviewed divergences fail until investigated.

## Stress Test (WHATWG HTML Spec)

Runs Adapted SlickSpeed selectors against the real single-page WHATWG HTML specification. Each engine runs in an isolated subprocess with a timeout; non-deterministic output or mismatches fail with a non-zero exit code. Import and schema compilation times are excluded from timing; parsing, extraction, and Python object construction are included (1 warmup + 3 timed repeats).
