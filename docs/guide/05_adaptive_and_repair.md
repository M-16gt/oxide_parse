# Adaptive Selectors and LLM Repair

When site markup changes, plain CSS selectors silently return `None`. `oxide_parse` provides two layers of defense: first, fingerprint-based element relocation (`adaptive`), and second, compact zero-IO repair payloads you can pass to an LLM or lookup table when a layout changes beyond recognition.

```python
from oxide_parse import T, compile

old_html = '<div id="p1"><h3 class="title">ThinkPad X1</h3><span class="price">$1,299</span></div>'
new_html = '<div id="p1"><h3 class="name">ThinkPad X1</h3><span class="cost">$1,299</span></div>'

schema = {
    "title": T.css("h3.title", adaptive="title").text(),
    "price": T.css(".price", adaptive="price").text().float(),
}
plan = compile(schema, backend="auto")

print("engine:", plan.engine)
print("old:", plan.extract(old_html))
print("new:", plan.extract(new_html))
```

```text
engine: scraper
old: {'title': 'ThinkPad X1', 'price': 1299.0}
new: {'title': 'ThinkPad X1', 'price': 1299.0}
```

## How `adaptive` Works

1. Whenever the CSS selector matches, normal extraction runs with zero overhead.
2. On the first successful match under `adaptive="key"`, the engine saves a fingerprint of the matched element: tag name, attributes, normalized text, ancestor tag path, parent, sibling tags, children, and up to three extracted value `samples`.
3. If a later page produces no matches for that selector, candidates (prioritizing the same tag) are scored against the saved fingerprint.
4. When similarity is `>= threshold` (default `0.5`), the best match is returned and stored under `key#healed`. The baseline fingerprint is never overwritten, so subsequent runs compare against both.

`adaptive` requires a DOM engine (`scraper` or `lite`), so `backend="auto"` moves off `stream`.

For repeating card lists, put `adaptive` on the list selector right before `.map(...)`:

```python
old_list = '<ul><li class="card"><b>A</b></li><li class="card"><b>B</b></li></ul>'
new_list = '<ul><li class="row"><b>A</b></li><li class="row"><b>B</b></li></ul>'

list_plan = compile({"items": T.css("li.card", adaptive="cards").map(T.text())})
list_plan.extract(old_list)
print(list_plan.extract(new_list))
```

```text
{'items': ['A', 'B']}
```

When a list selector breaks, relocation finds the best matching item and collects matching siblings sharing the same tag under that parent.

## Persisting Fingerprints

By default fingerprints live in memory for the lifetime of `Plan`. For production jobs, persist them either on disk or in your own database/cache:

- File store: `compile(schema, adaptive_store="fingerprints.json")`.
- JSON string (for Redis, Postgres, S3, or serverless workers without writable disks):

```python
state = plan.export_adaptive_store()

# In a fresh worker process:
plan2 = compile(schema, adaptive_state=state)
# Or merge into an existing plan:
loaded = plan2.import_adaptive_store(state)
print("loaded keys:", loaded)
```

```text
loaded keys: 4
```

Those 4 keys are the two baselines (`title`, `price`) plus the two healed entries (`title#healed`, `price#healed`).

## Self-Healing via `extract_or_repair`

When markup changes too much to reach `threshold`, `adaptive` leaves the field as `None` and records an entry in `misses`. Hook up a `repair` or `repair_batch` callback — typically an LLM call — to heal broken selectors automatically.

`repair_batch` groups fields that broke inside the same parent container into a single request sharing one distilled `skeleton`:

```python
broken_html = '<div id="p1"><div class="card-hdr">Laptop X1 Pro</div><strong class="new-amount">1199</strong></div>'

def ask_llm_batch(batch):
    # Real code sends batch["skeleton"] and batch["fields"] to an LLM.
    keys = [f["key"] for f in batch["fields"]]
    print("repair keys:", keys, "scope:", batch["scope"])
    return {"title": ".card-hdr", "price": ".new-amount"}

res = plan.extract_or_repair(broken_html, repair_batch=ask_llm_batch)
print("data:", res.data)
print("repaired:", res.repaired)
print(res.plan.to_python(), end="")
```

```text
repair keys: ['title', 'price'] scope: container
data: {'title': 'Laptop X1 Pro', 'price': 1199.0}
repaired: True
from oxide_parse import T

schema = {
    'title': T.css('.card-hdr', adaptive='title').text(),
    'price': T.css('.new-amount', adaptive='price').text().float(),
}
```

Inside `extract_or_repair`:

1. Passes each batch's distilled `skeleton` (container or page fallback) and broken `fields` (old `css`, `fingerprint`, `samples`) to your callback.
2. Validates every returned CSS selector immediately with the engine parser. Invalid syntax (such as `.price::text`) does not crash the run — the engine sends back `previous_error` so the model can correct itself on a retry.
3. Re-runs extraction with the repaired selectors.
4. Returns `RepairResult(data, plan, repaired, errors, misses)` where `res.plan` is a patched copy ready to persist via `res.plan.to_python()` or `.to_json()`.

To patch a plan manually, call `plan.with_selectors({"price": ".new-amount"})`. Rewritten keys automatically clear their old baseline and `#healed` fingerprints (including from `adaptive_store` files) so the new selector learns a fresh baseline.

## Passive Monitoring Without Live LLM Calls

If you do not want synchronous LLM calls on production scraping workers, call `extract_detailed()` or `repair_request()`:

```python
det = plan2.extract_detailed(broken_html)
print("misses:", [m["key"] for m in det.misses])
```

```text
misses: ['title', 'price']
```

`plan.repair_request(broken_html)` builds a self-contained JSON payload with the schema, `misses`, and a distilled page skeleton (or `None` when nothing broke) ready to queue for offline repair. See [docs/ai/prompt_self_healing.md](../ai/prompt_self_healing.md) for a ready-to-use system prompt.
