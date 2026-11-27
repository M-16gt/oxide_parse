"""Zero-IO breadth-first crawler helpers built on compiled `oxide_parse` schemas and user `fetch` callbacks.

Provides URL canonicalization, `max_depth` / `max_pages` bounds, same-domain and prefix filtering, and enqueue-time
deduplication so graph cycles and diamond links never fetch a canonical URL twice.

Examples:
    >>> from oxide_parse import T, crawl
    >>> pages = {
    ...     "https://ex.test/": "<h1>Root</h1><a href='/b'>b</a><a href='/c'>c</a>",
    ...     "https://ex.test/b": "<h1>B</h1><a href='/d'>d</a>",
    ...     "https://ex.test/c": "<h1>C</h1><a href='/d'>d</a>",
    ...     "https://ex.test/d": "<h1>D</h1><a href='/'>home</a>",
    ... }
    >>> res = crawl("https://ex.test/", T.css("h1").text(), fetch=lambda url: pages[url], max_depth=2)
    >>> [(p.data, p.depth) for p in res]
    [('Root', 0), ('B', 1), ('C', 1), ('D', 2)]
"""
from __future__ import annotations

import asyncio
import inspect
from collections import deque
from dataclasses import dataclass, field
from typing import Any, Callable, Deque, Dict, Iterable, List, Literal, Optional, Sequence, Set, Tuple, Union
from urllib.parse import parse_qsl, urlencode, urljoin, urlsplit, urlunsplit

from .expr import T
from .plan import Backend, Plan, _unpack_envelope


@dataclass(frozen=True)
class CrawlPage:
    """Result record produced for each visited URL during `crawl()` or `acrawl()`.

    Examples:
        >>> from oxide_parse import CrawlPage
        >>> CrawlPage(url="https://ex.test/", depth=0, data={"title": "Home"}, links=[])
        CrawlPage(url='https://ex.test/', depth=0, data={'title': 'Home'}, links=[], error=None)
    """

    url: str
    """Canonical URL visited for this page."""
    depth: int
    """Zero-based link distance from the starting seed URL(s)."""
    data: Any
    """Extracted value returned by `schema`, or `None` if an error was collected."""
    links: List[str] = field(default_factory=list)
    """Canonical outgoing URLs accepted by crawl scope rules from this page."""
    error: Optional[str] = None
    """Error message when `on_error="collect"` recorded a failure on this page."""


def canonicalize_url(
    url: str,
    *,
    base_url: Optional[str] = None,
    sort_query: bool = True,
    strip_fragment: bool = True,
) -> Optional[str]:
    """Normalize an HTTP(S) URL for frontier deduplication, returning `None` for non-navigable links.

    Resolves relative links against `base_url`, lowercases scheme and host, removes default ports (`:80` / `:443`),
    normalizes an empty path to `"/"`, drops `#fragments` when `strip_fragment=True`, and sorts query parameters when
    `sort_query=True`. Rejects blank strings, in-page anchors (`#...`), and `javascript:` / `mailto:` / `tel:` links.

    Args:
        url: Candidate URL or relative link reference.
        base_url: Optional base URL used to resolve relative references.
        sort_query: When `True` (default), sort query pairs alphabetically for canonical equality.
        strip_fragment: When `True` (default), remove URL `#fragment` identifiers.

    Returns:
        Canonical HTTP(S) URL string, or `None` if the URL should be ignored.

    Examples:
        >>> from oxide_parse import canonicalize_url
        >>> canonicalize_url("/items?b=2&a=1#top", base_url="https://Shop.Test:443")
        'https://shop.test/items?a=1&b=2'
        >>> canonicalize_url("javascript:void(0)") is None
        True
    """
    if not isinstance(url, str):
        return None
    raw = url.strip()
    if not raw or raw.startswith("#"):
        return None
    resolved = urljoin(base_url, raw) if base_url else raw
    try:
        parts = urlsplit(resolved)
    except ValueError:
        return None
    scheme = parts.scheme.lower()
    if scheme not in ("http", "https") or not parts.hostname:
        return None
    host = parts.hostname.lower()
    port = parts.port
    auth = ""
    if parts.username is not None:
        auth = parts.username
        if parts.password is not None:
            auth += f":{parts.password}"
        auth += "@"
    port_suffix = ""
    if port is not None and not ((scheme == "http" and port == 80) or (scheme == "https" and port == 443)):
        port_suffix = f":{port}"
    netloc = f"{auth}{host}{port_suffix}"
    path = parts.path or "/"
    query = urlencode(sorted(parse_qsl(parts.query, keep_blank_values=True)), doseq=True) if sort_query and parts.query else parts.query
    fragment = "" if strip_fragment else parts.fragment
    return urlunsplit((scheme, netloc, path, query, fragment))


def _coerce_plan(schema: Any, backend: Backend) -> Plan:
    return schema if isinstance(schema, Plan) else Plan(schema, backend)


def _normalize_links(raw: Any) -> List[str]:
    if raw is None:
        return []
    if isinstance(raw, str):
        return [raw]
    if isinstance(raw, list):
        return [item for item in raw if isinstance(item, str)]
    return []


def _accepted_url(
    raw: str,
    *,
    base_url: str,
    seed_hosts: Set[str],
    same_domain: bool,
    allow_prefixes: Optional[Tuple[str, ...]],
) -> Optional[str]:
    canon = canonicalize_url(raw, base_url=base_url)
    if canon is None:
        return None
    if same_domain:
        host = (urlsplit(canon).hostname or "").lower()
        if host not in seed_hosts:
            return None
    if allow_prefixes is not None and not canon.startswith(allow_prefixes):
        return None
    return canon


def _extract_page(
    data_plan: Plan,
    link_plan: Plan,
    raw_resp: Any,
    *,
    req_url: str,
    depth: int,
    seed_hosts: Set[str],
    same_domain: bool,
    allow_prefixes: Optional[Tuple[str, ...]],
) -> Tuple[Any, List[str]]:
    body, merged_meta = _unpack_envelope(raw_resp, meta={"base_url": req_url, "depth": depth}, is_json=data_plan.engine == "json")
    eff_url = (merged_meta or {}).get("base_url") or req_url
    data = data_plan.extract(body, meta=merged_meta)
    if data_plan.engine == "xml" and link_plan.engine != "xml":
        raw_links = []
    else:
        raw_links = _normalize_links(link_plan.extract(body, meta=merged_meta))
    accepted: List[str] = []
    for href in raw_links:
        canon = _accepted_url(
            href,
            base_url=eff_url,
            seed_hosts=seed_hosts,
            same_domain=same_domain,
            allow_prefixes=allow_prefixes,
        )
        if canon is not None:
            accepted.append(canon)
    return data, accepted


def crawl(
    seeds: Union[str, Iterable[str]],
    schema: Any,
    *,
    fetch: Callable[..., Any],
    links: Any = None,
    max_depth: int = 2,
    max_pages: int = 100,
    same_domain: bool = True,
    allow_prefixes: Optional[Sequence[str]] = None,
    backend: Backend = "auto",
    on_error: Literal["raise", "skip", "collect"] = "raise",
) -> List[CrawlPage]:
    """Run a breadth-first (BFS) crawl starting from `seeds` without owning network IO.

    Marks canonical URLs as seen at enqueue time so cycles and converging branches are visited in exact BFS order
    and fetched at most once. Enforces `max_depth`, `max_pages`, `same_domain`, and `allow_prefixes`.

    Args:
        seeds: Starting URL or iterable of starting URLs.
        schema: Extraction `Plan` or DSL schema (`dict`, `list`, `T` expression) evaluated per visited page.
        fetch: Sync callback `fetch(url)` (also receives keyword `url=` if needed? Called positionally `fetch(url)`).
        links: Optional schema extracting outgoing link strings per page (default `T.css("a[href]").map(T.attr("href"))`).
        max_depth: Maximum BFS depth (`0` fetches seeds only).
        max_pages: Hard cap on total visited pages (`>= 1`).
        same_domain: Keep only links sharing a hostname with `seeds` (default `True`).
        allow_prefixes: Optional allowed canonical URL prefix sequence.
        backend: Engine backend when compiling `schema` (default `"auto"`).
        on_error: Failure policy: `"raise"` (default), `"skip"` (drop failed page), or `"collect"` (emit `CrawlPage`
            with `error` populated and `data=None`).

    Returns:
        List of `CrawlPage` results in BFS visit order.

    Raises:
        ValueError: If `max_depth < 0`, `max_pages < 1`, `on_error` is invalid, or `seeds` yields no valid HTTP(S) URL.

    Examples:
        >>> from oxide_parse import T, crawl
        >>> graph = {"https://ex.test/": "<p>A</p><a href='/b'>b</a>", "https://ex.test/b": "<p>B</p>"}
        >>> [p.data for p in crawl("https://ex.test/", T.css("p").text(), fetch=lambda u: graph[u], max_depth=1)]
        ['A', 'B']
    """
    if max_depth < 0:
        raise ValueError("crawl(): max_depth must be >= 0")
    if max_pages < 1:
        raise ValueError("crawl(): max_pages must be >= 1")
    if on_error not in ("raise", "skip", "collect"):
        raise ValueError('crawl(): on_error must be "raise", "skip", or "collect"')

    seed_list = [seeds] if isinstance(seeds, str) else list(seeds)
    data_plan = _coerce_plan(schema, backend)
    link_expr = T.css("a[href]").map(T.attr("href")) if links is None else links
    link_plan = _coerce_plan(link_expr, "xml" if backend == "xml" else "auto")

    seen: Set[str] = set()
    queue: Deque[Tuple[str, int]] = deque()
    seed_hosts: Set[str] = set()
    for s in seed_list:
        c = canonicalize_url(s)
        if c is not None and c not in seen:
            seen.add(c)
            seed_hosts.add((urlsplit(c).hostname or "").lower())
            queue.append((c, 0))
    if not queue:
        raise ValueError("crawl(): no valid HTTP(S) seed URLs provided")

    prefixes = tuple(allow_prefixes) if allow_prefixes is not None else None
    out: List[CrawlPage] = []

    while queue and len(out) < max_pages:
        url, depth = queue.popleft()
        try:
            resp = fetch(url)
            data, accepted = _extract_page(
                data_plan,
                link_plan,
                resp,
                req_url=url,
                depth=depth,
                seed_hosts=seed_hosts,
                same_domain=same_domain,
                allow_prefixes=prefixes,
            )
        except Exception as exc:
            if on_error == "raise":
                raise
            if on_error == "collect":
                out.append(CrawlPage(url=url, depth=depth, data=None, links=[], error=str(exc)))
            continue

        out.append(CrawlPage(url=url, depth=depth, data=data, links=accepted))
        if depth < max_depth:
            for nxt in accepted:
                if nxt not in seen and len(out) + len(queue) < max_pages:
                    seen.add(nxt)
                    queue.append((nxt, depth + 1))

    return out


async def acrawl(
    seeds: Union[str, Iterable[str]],
    schema: Any,
    *,
    fetch: Callable[..., Any],
    links: Any = None,
    max_depth: int = 2,
    max_pages: int = 100,
    same_domain: bool = True,
    allow_prefixes: Optional[Sequence[str]] = None,
    backend: Backend = "auto",
    on_error: Literal["raise", "skip", "collect"] = "raise",
    concurrency: int = 4,
) -> List[CrawlPage]:
    """Async breadth-first (BFS) crawler processing each depth level concurrently via `asyncio.gather`.

    Supports both async (`async def fetch(url)`) and sync `fetch` callbacks while preserving deterministic level-by-level
    BFS discovery and canonical URL deduplication.

    Args:
        seeds: Starting URL or iterable of starting URLs.
        schema: Extraction `Plan` or DSL schema evaluated per visited page.
        fetch: Sync or async callback `fetch(url)` returning page HTML/XML/JSON or response envelope.
        links: Optional outgoing-link expression (default `T.css("a[href]").map(T.attr("href"))`).
        max_depth: Maximum BFS depth (`0` fetches seeds only).
        max_pages: Hard cap on total visited pages (`>= 1`).
        same_domain: Restrict links to seed hostname(s) (default `True`).
        allow_prefixes: Optional allowed canonical URL prefix sequence.
        backend: Engine backend when compiling `schema` (default `"auto"`).
        on_error: Failure policy: `"raise"`, `"skip"`, or `"collect"`.
        concurrency: Maximum concurrent `fetch` calls per BFS level (`>= 1`).

    Returns:
        List of `CrawlPage` records in BFS order.

    Raises:
        ValueError: If `max_depth < 0`, `max_pages < 1`, `concurrency < 1`, `on_error` is invalid, or no valid seeds.

    Examples:
        >>> pages = await acrawl("https://ex.test/", T.css("h1").text(), fetch=afetch)  # doctest: +SKIP
    """
    if max_depth < 0:
        raise ValueError("acrawl(): max_depth must be >= 0")
    if max_pages < 1:
        raise ValueError("acrawl(): max_pages must be >= 1")
    if concurrency < 1:
        raise ValueError("acrawl(): concurrency must be >= 1")
    if on_error not in ("raise", "skip", "collect"):
        raise ValueError('acrawl(): on_error must be "raise", "skip", or "collect"')

    seed_list = [seeds] if isinstance(seeds, str) else list(seeds)
    data_plan = _coerce_plan(schema, backend)
    link_expr = T.css("a[href]").map(T.attr("href")) if links is None else links
    link_plan = _coerce_plan(link_expr, "xml" if backend == "xml" else "auto")

    seen: Set[str] = set()
    frontier: List[str] = []
    seed_hosts: Set[str] = set()
    for s in seed_list:
        c = canonicalize_url(s)
        if c is not None and c not in seen and len(frontier) < max_pages:
            seen.add(c)
            seed_hosts.add((urlsplit(c).hostname or "").lower())
            frontier.append(c)
    if not frontier:
        raise ValueError("acrawl(): no valid HTTP(S) seed URLs provided")

    prefixes = tuple(allow_prefixes) if allow_prefixes is not None else None
    sem = asyncio.Semaphore(concurrency)
    out: List[CrawlPage] = []

    async def one(req_url: str, depth: int) -> Tuple[str, Optional[CrawlPage], Optional[Exception]]:
        async with sem:
            try:
                res = fetch(req_url)
                if inspect.isawaitable(res):
                    res = await res
                data, accepted = _extract_page(
                    data_plan,
                    link_plan,
                    res,
                    req_url=req_url,
                    depth=depth,
                    seed_hosts=seed_hosts,
                    same_domain=same_domain,
                    allow_prefixes=prefixes,
                )
                return req_url, CrawlPage(url=req_url, depth=depth, data=data, links=accepted), None
            except Exception as exc:
                return req_url, None, exc

    depth = 0
    while frontier and len(out) < max_pages and depth <= max_depth:
        remaining = max_pages - len(out)
        batch = frontier[:remaining]
        next_frontier: List[str] = []
        done = await asyncio.gather(*(one(u, depth) for u in batch))
        for req_url, page, exc in done:
            if exc is not None:
                if on_error == "raise":
                    raise exc
                if on_error == "collect":
                    out.append(CrawlPage(url=req_url, depth=depth, data=None, links=[], error=str(exc)))
                continue
            assert page is not None
            out.append(page)
            if depth < max_depth:
                for nxt in page.links:
                    if nxt not in seen and len(out) + len(next_frontier) < max_pages:
                        seen.add(nxt)
                        next_frontier.append(nxt)
        frontier = next_frontier
        depth += 1

    return out
