# Prompt: Self-Healing Broken Selectors (`repair` / `repair_batch`)

When an `adaptive` selector stops matching and fingerprint relocation falls below `threshold`, `oxide_parse` emits a repair request to your callback — typically backed by an LLM.

See also [05_adaptive_and_repair.md](../guide/05_adaptive_and_repair.md).

## Input Payload (`repair_batch`)

`plan.extract(html, repair_batch=ask_llm)` and `plan.extract_or_repair(html, repair_batch=ask_llm)` group all fields that broke within the same HTML container into one payload:

- `doc`, `url`: document identifier and URL (if any);
- `scope`: `"container"` (distilled old parent container) or `"page"` (whole distilled page if the container was missing);
- `skeleton`: shrunk HTML skeleton of that region;
- `schema`: compiled schema IR;
- `fields`: list of `{id, key, css, fingerprint, previous_error?}` objects:
  - `fingerprint` holds old `tag`, `attrs`, `text`, `path`, `parent`, `prev`, `next`, `children`, and up to 3 previous extracted `samples`;
  - `previous_error` appears on a retry when the model's earlier selector failed CSS parsing.

Once the callback returns valid CSS selectors, extraction re-runs cleanly, and `extract_or_repair` returns an updated `Plan` ready to export via `res.plan.to_python()`.

## System Prompt for Your Model

```markdown
You repair broken CSS selectors for `oxide_parse`.
Input is a JSON object with:
- `skeleton`: distilled HTML of the target area (`scope`: `"container"` or `"page"`).
- `fields`: list of broken fields, each with `key`, old `css`, `fingerprint` (old tag, attrs, path, and `samples` of previous values), and optional `previous_error` if your last CSS had invalid syntax.

Rules:
1. Return ONLY a valid JSON object mapping `{key: new_css_selector}`. Omit a key if the data is genuinely absent from `skeleton`.
2. Use standard CSS Selectors only (tags, `#id`, `.class`, `[attr=val]`, `>`, ` `, `:nth-child()`, `:not()`, `:scope`).
3. NEVER use non-standard syntax like `::text`, `::attr(...)`, `:contains-text(...)`, or XPath.
4. Prefer stable semantic attributes (`data-testid`, `itemprop`, `aria-label`, semantic classes) over brittle hashed classes or deep `nth-child` chains.
5. If `previous_error` is present, fix the exact syntax issue reported there.
```

## Wiring in Python

```python
import json

SYSTEM_PROMPT = "..."  # system prompt above

def ask_llm(batch: dict) -> dict[str, str]:
    payload = {
        "scope": batch["scope"],
        "skeleton": batch["skeleton"],
        "fields": [
            {k: f[k] for k in ("key", "css", "fingerprint", "previous_error") if k in f}
            for f in batch["fields"]
        ],
    }
    raw = your_llm_call(system=SYSTEM_PROMPT, user=json.dumps(payload, ensure_ascii=False))
    return json.loads(raw)

res = plan.extract_or_repair(html, repair_batch=ask_llm)
if res.repaired:
    with open("schema.py", "w", encoding="utf-8") as f:
        f.write(res.plan.to_python())
```
