"""Declarative HTML and JSON extraction compiler backed by Rust.

Build schemas using the `T` expression builder, serialize or inspect them as JSON-compatible IR, and execute them on
single-pass streaming (`stream`), arena DOM (`lite`), html5ever DOM (`scraper`), or `json` backends. Network IO and
LLM calls stay outside the core via `Session`, `follow`, `paginate`, and `adaptive` repair callbacks.

Examples:
    Extract structured fields from HTML:

    >>> from oxide_parse import T, compile
    >>> html = "<h1>Shop</h1><div class='p'><b>Item</b><i>$12.50</i></div>"
    >>> schema = {"title": T.css("h1").text(), "items": T.css(".p").map({"name": T.css("b").text(), "price": T.css("i").text().float()})}
    >>> compile(schema, backend="auto").extract(html)
    {'title': 'Shop', 'items': [{'name': 'Item', 'price': 12.5}]}

    Round-trip a schema through JSON IR and back to Python code:

    >>> from oxide_parse import decompile, to_ir
    >>> ir = to_ir({"title": T.css("h1").text().strip()})
    >>> print(decompile(ir).strip())
    from oxide_parse import T
    <BLANKLINE>
    schema = {
        'title': T.css('h1').text().strip(),
    }
"""
from .crawl import CrawlPage, acrawl, canonicalize_url, crawl
from .decompile import decompile
from .expr import Expr, T, coalesce, follow, if_, paginate, to_ir
from .formats import (
    FORMATS,
    FormatCodec,
    FormatRegistry,
    available_formats,
    dumps_format,
    get_format,
    loads_format,
    register_format,
    unregister_format,
)
from .plan import (
    Detailed,
    EngineInfo,
    OxideParseError,
    Plan,
    RepairResult,
    Session,
    compile,
    decode_html,
    distill,
    engines,
    extract,
    ir_schema,
    markdown,
    validate_ir,
)

__all__ = [
    "T",
    "Expr",
    "to_ir",
    "if_",
    "coalesce",
    "follow",
    "paginate",
    "FORMATS",
    "FormatCodec",
    "FormatRegistry",
    "register_format",
    "unregister_format",
    "get_format",
    "available_formats",
    "dumps_format",
    "loads_format",
    "CrawlPage",
    "canonicalize_url",
    "crawl",
    "acrawl",
    "Plan",
    "compile",
    "extract",
    "decompile",
    "OxideParseError",
    "engines",
    "EngineInfo",
    "Detailed",
    "RepairResult",
    "Session",
    "decode_html",
    "distill",
    "markdown",
    "validate_ir",
    "ir_schema",
]
__version__ = "0.9.1"
