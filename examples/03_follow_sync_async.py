"""`follow` never does IO inside the engine: you supply `fetch` (requests, aiohttp, cache, Playwright...).

The `request` dict of `follow` is passed to it as keyword arguments: `fetch(**request)`.
"""
import asyncio

from oxide_parse import T, compile, follow

SITE = {
    "/rust": '<p class="pages">552 pages</p><p class="author">Klabnik</p>',
    "/py": '<p class="pages">1,200 pages</p><p class="author">Ramalho</p>',
}
CATALOG = """
<div class="book"><h2>Rust</h2><span class="price">$40</span><a href="/rust">d</a></div>
<div class="book"><h2>Python</h2><span class="price">$55</span><a href="/py">d</a></div>
"""

plan = compile({
    "books": T.css(".book").map(follow(
        request={
            "url": T.css("a").attr("href"),
            "headers": {"Referer": "https://shop.example"},   # any extra fields you want your fetcher to see
        },
        meta={"title": T.css("h2").text(), "price": T.css(".price").text().float()},
        schema={
            "title": T.meta("title"),
            "price": T.meta("price"),
            "pages": T.css(".pages").text().int(),
            "author": T.css(".author").text(),
        },
    ))
})

# sync
def fetch(url, headers=None):
    print("  fetch", url, headers)
    return SITE[url]


print(plan.extract(CATALOG, fetch=fetch))


# async (fetch may be sync or async; all pending requests of a level are fetched concurrently)
async def afetch(url, headers=None):
    await asyncio.sleep(0.01)
    return SITE[url]


print(asyncio.run(plan.aextract(CATALOG, fetch=afetch)))

# or drive the state machine yourself
sess = plan._core.start(CATALOG)
print(sess.pending())
