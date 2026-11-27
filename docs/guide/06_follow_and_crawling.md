# Following Links (`follow`), Pagination (`paginate`), `Session`, and Batches

The `oxide_parse` core never performs network or disk IO on its own. Instead, sub-queries (`follow`, `paginate`) run as a deterministic state machine: the engine yields request dicts, your own HTTP client (`requests`, `httpx`, `aiohttp`, Playwright) fetches the pages, and you pass HTML/JSON responses back in.

## `follow` (Sync)

```python
from pprint import pprint
from oxide_parse import T, compile, follow

html = """
<div class="book"><h2>Dune</h2><a href="/b/1">Details</a></div>
<div class="book"><h2>Solaris</h2><a href="/b/2">Details</a></div>
"""
pages = {
    "https://books.example/b/1": '<span class="pages">412 pp</span>',
    "https://books.example/b/2": '<span class="pages">256 pp</span>',
}

schema = {
    "books": T.css(".book").map(follow(
        request={"url": T.css("a").attr("href").url()},
        meta={"title": T.css("h2").text()},
        schema={
            "title": T.meta("title"),
            "pages": T.css(".pages").text().int(),
        },
    )),
}

plan = compile(schema)

def my_fetch(url, **rest):
    return pages[url]

data = plan.extract(html, meta={"base_url": "https://books.example/"}, fetch=my_fetch)
pprint(data, sort_dicts=False)
```

```text
{'books': [{'title': 'Dune', 'pages': 412}, {'title': 'Solaris', 'pages': 256}]}
```

Key contract details for `follow`:

- There is no `url=` keyword argument — pass `request=`. Values inside `request` can be any `T` expressions, literals, or nested dicts (`headers`, `method`, `params`). Everything is evaluated and forwarded to `fetch(**request)`.
- Passing a bare expression or string (`follow(T.css("a").attr("href").url(), schema)`) is shorthand for `request={"url": ...}`.
- When `request` or its `"url"` evaluates to `None`, no request is emitted and the field evaluates to `None`.
- The followed document inherits `meta["base_url"] = params["url"]` automatically unless overridden in `follow(..., meta=...)`, so relative URLs inside sub-pages resolve against the sub-page URL.
- Nesting can be arbitrary (`follow` inside `follow` or `paginate` inside `follow`).
- Pipeline steps chained after `follow(...)` (such as `.default(...)` or `["price"]`) move onto the fetched sub-page's result. Chaining `follow(A).follow(B)` evaluates `B` against the document fetched by `A` (discarding `A`'s sub-schema result).

## Pagination (`paginate`)

When results span `next` links or cursor tokens (e.g., user friends, comments, search pages), declare pagination directly in the schema with `paginate(items, next, ...)` instead of writing manual `Session` loops:

```python
from pprint import pprint
from oxide_parse import T, compile, follow, paginate

social_pages = {
    "https://social.example/u/42": '<a class="friends" href="/u/42/friends">Friends</a>',
    "https://social.example/u/42/friends": (
        '<div class="friend">Ann</div><div class="friend">Bob</div>'
        '<a class="next" href="?page=2">Next</a>'
    ),
    "https://social.example/u/42/friends?page=2": (
        '<div class="friend">Vera</div><div class="friend">Dan</div>'
        '<a class="next" href="?page=3">Next</a>'
    ),
    "https://social.example/u/42/friends?page=3": '<div class="friend">Eve</div>',
}

friends_schema = {
    "friends": follow(
        T.css("a.friends").attr("href").url(),
        paginate(
            items=T.css(".friend").map(T.text()),
            next=T.css("a.next").attr("href").url(),
            max_items=3,
        ),
    )
}

def fetch_site(url, **rest):
    return social_pages[url]

out = compile(friends_schema).extract(
    social_pages["https://social.example/u/42"],
    meta={"base_url": "https://social.example/u/42"},
    fetch=fetch_site,
)
pprint(out, sort_dicts=False)
```

```text
{'friends': ['Ann', 'Bob', 'Vera']}
```

`paginate` drives the same `fetch` / `aextract` / `Session` loop and stops *before* issuing the next request whenever any stopping rule triggers:

- `max_items`: target item count; once reached, the list is truncated and no further page is fetched.
- `max_pages`: maximum pages including the initial page (`max_pages=1` returns only the current page). Without `max_pages`, a safety ceiling of 10,000 pages applies.
- `next` returns `None`, an empty/whitespace string, `"#"`, `javascript:...`, or a dict whose `"url"` is `None`/empty.
- Cycle detection: the canonicalized request (`url` with `#fragment` stripped, or full `params` when extra keys like cursor/body exist) was already visited in this pagination chain.
- Parent `meta` keys propagate across pages, with `meta["base_url"]` updated automatically from each page's `"url"`.

## Async Crawling (`aextract`)

Use `await plan.aextract(...)` to fetch one level of requests concurrently. Your `fetch` function may be sync or `async`:

```python
# skip-doctest
import aiohttp

async def afetch(url, headers=None, **rest):
    async with http_session.get(url, headers=headers) as resp:
        return await resp.text()

data = await plan.aextract(html, meta={"base_url": "https://books.example/"}, fetch=afetch)
```

All pending requests in a round run concurrently via `asyncio.gather`. Documents of 64 KiB or more are parsed in `asyncio.to_thread` with the Rust GIL released so the event loop stays responsive.

## Manual Crawler Loop with `Session`

When your crawler manages custom priority queues, retries, rate limits, proxies, or mixes network fetches with LLM repairs in one scheduler, drive `plan.start()` directly:

```python
sess = plan.start(html, meta={"base_url": "https://books.example/"})
while not sess.done:
    answers = []
    for r in sess.requests():
        if r["kind"] == "fetch":
            answers.append((r["id"], my_fetch(**r["params"])))
        else:  # kind == "repair" for broken adaptive selectors
            answers.append((r["id"], None))
    sess.resume(answers)

print(sess.result())
```

```text
{'books': [{'title': 'Dune', 'pages': 412}, {'title': 'Solaris', 'pages': 256}]}
```

Notes on `Session`:
- `sess.pending()` filters `sess.requests()` to `kind == "fetch"`; `sess.repairs()` filters to `kind == "repair"`; `sess.repair_batches()` groups broken selectors by container skeleton.
- `sess.done` becomes `True` as soon as no fetch requests remain (repairs are optional; declined repairs stay `None`).
- Call `sess.detailed()` instead of `sess.result()` to get quarantined `errors`, unresolved `misses`, and applied `repairs`.
- Async equivalents: `await plan.astart(...)` and `await sess.aresume(answers)`.

## Parallel Batch Extraction (`extract_batch`)

When documents are already in memory (archive, Kafka batch, response cache), extract them in parallel on a Rayon thread pool with the GIL released:

```python
docs = ["<h1>10</h1>", "<h1>error</h1>", "<h1>30</h1>"]
batch_plan = compile({"count": T.css("h1").text().int()}, backend="auto")

results, failures = batch_plan.extract_batch(docs, threads=4, errors="collect")
print("results:", results)
print("failures:", failures)
```

```text
results: [{'count': 10}, None, {'count': 30}]
failures: [{'index': 1, 'message': 'extraction error at `$.count`: int: no number in "error"'}]
```

With `errors="raise"` (default), the first failing document raises an exception and `extract_batch` returns the result list directly. With `errors="collect"`, it returns `(results, failures)` with `None` at failed indices. `plan.extract_many(docs)` is a fast shorthand when you only need `errors="raise"`. Both accept any `Iterable` (`list`/`tuple` take a zero-copy path; generators are consumed in a single pass).
