# Conformance report

Generated UTC 2026-11-17T11:39:17.360016+00:00.

Strict comparisons retain upstream expectations. Failures and reviewed architecture differences are recorded below.

PASS percentages below count executed strict comparisons only. Unsupported cases and known architecture differences are separate.

| Group | PASS | FAIL | ERROR | UNSUPPORTED | KNOWN_DIFFERENCE | Strict pass % |
|---|---:|---:|---:|---:|---:|---:|
| embedded_invariant | 10 | 0 | 0 | 0 | 0 | 100.0 |
| html5lib_invariant | 295 | 0 | 0 | 0 | 0 | 100.0 |
| html5lib_tree_oracle | 288 | 0 | 0 | 0 | 7 | 100.0 |
| css_upstream | 87 | 0 | 0 | 41 | 1 | 100.0 |
| css_stream_invariant | 76 | 0 | 0 | 53 | 0 | 100.0 |
| css_scraper_oracle | 82 | 0 | 0 | 42 | 5 | 100.0 |
| xpath_lxml | 318 | 0 | 0 | 60 | 0 | 100.0 |
| native_embedded_css | 74 | 0 | 0 | 0 | 0 | 100.0 |
| native_embedded_tree | 4 | 0 | 0 | 0 | 0 | 100.0 |
| native_embedded_xpath | 36 | 0 | 0 | 0 | 0 | 100.0 |
| native_external_tokenizer | 472 | 0 | 0 | 13 | 0 | 100.0 |
| native_external_tree | 256 | 0 | 0 | 0 | 39 | 100.0 |
| native_selector_capabilities | 0 | 0 | 0 | 0 | 0 | n/a |

## Scope and limitations

- HTML tree checks project away html/head/body containers, comments and doctypes. Other structure is compared; this is not full HTML5 certification.
- Fragment-context and scripting-flag fixtures are separately labelled. lite/stream invariants still run their input as standalone documents.
- Tokenizer checks project away Comment/DOCTYPE tokens and merge adjacent Character tokens. Parse-error positions are not exposed by the library.
- CSS uses the entire cascadia selectorTests table and 15 initial immutable Sizzle assertions. Dynamic JS tests, NWMatcher/Qwery and WPT domxpath are not imported.
- XPath uses pinned libxml2 expression files plus explicit shared-HTML path tests against lxml. XML document-context tests are retained as source data only.
- Missing upstream features are explicit UNSUPPORTED results. Unexpected compile/extract failures are ERROR and fail pytest.
- Scalar XPath numbers use relative tolerance 1e-14 (except signed zero); lite/stream comparisons use canonical JSON byte equality.
- See python_results.json for provenance and result-file index. python_<group>.json and native_*.json contain every input, selector/expression, expected value, actual value and error.

## Reproduce

```powershell
$env:CONFORMANCE_REPORT_DIR = './benchmarks/2026-11-17-v8_1-conformance'
cargo test -p oxide_parse-core --features full --test conformance -- --nocapture
uv run --extra bench pytest tests/test_conformance.py -q --tb=short
```
