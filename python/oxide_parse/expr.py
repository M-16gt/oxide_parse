"""Python extraction DSL (`T`) and IR serialization (`to_ir`).

Build declarative pipelines starting from the root expression `T` (or standalone helpers `if_()`, `coalesce()`,
`follow()`, `paginate()`), then compile them once with `compile()` and reuse the resulting `Plan` across any number
of documents.

Examples:
    >>> from oxide_parse import T, compile
    >>> html = "<h1>Catalog</h1><ul><li class='p'>A</li><li class='p'>B</li></ul>"
    >>> compile({"title": T.css("h1").text(), "items": T.css("li.p").map(T.text())}).extract(html)
    {'title': 'Catalog', 'items': ['A', 'B']}
"""
from __future__ import annotations

import builtins
from typing import Any, Dict, List, Literal, Optional, Sequence


class Expr:
    """Immutable pipeline of extraction operations starting from the current node or value.

    `Expr` never reads a document directly; each method appends one step and returns a new `Expr`. At the schema root,
    `T` refers to the document root element (HTML) or root JSON value; inside `.map(...)`, it refers to each item.

    Operator overloads:
        * `expr[k]` dispatches to `.key(k)` for strings, `.at(k)` for ints, and `.slice(k.start, k.stop)` for slices.
        * `==`, `!=`, `&`, `|`, and `~` build schema-level comparisons and boolean logic.
        * Evaluating truthiness (`bool(expr)`, Python `and`/`or`/`not`/`if`) raises `TypeError`; use `if_()` instead.

    Examples:
        >>> from oxide_parse import T, compile
        >>> base = T.css(".card")
        >>> schema = base.map({"title": T.css("h2").text(), "sale": T.has_attr("data-sale")})
        >>> compile(schema).extract("<div class='card' data-sale><h2>Phone</h2></div>")
        [{'title': 'Phone', 'sale': True}]
    """

    __slots__ = ("_prev", "_op")

    def __init__(self, prev: "Expr | None" = None, op: Any = None):
        """Initialize an `Expr` node wrapping an optional predecessor and operation.

        Args:
            prev: Preceding `Expr` step in the immutable pipeline.
            op: Serialized IR operation appended by this step.

        Examples:
            >>> from oxide_parse import Expr
            >>> isinstance(Expr(), Expr)
            True
        """
        self._prev = prev
        self._op = op

    # -- internals ---------------------------------------------------------
    def _then(self, op: Any) -> "Expr":
        return Expr(self, op)

    def _steps(self) -> List[Any]:
        out: List[Any] = []
        node: Expr | None = self
        while node is not None and node._op is not None:
            out.append(node._op)
            node = node._prev
        out.reverse()
        return out

    def __repr__(self) -> str:
        from .decompile import expr_to_code

        return expr_to_code({"pipe": self._steps()})

    # -- navigation --------------------------------------------------------
    def css(self, selector: str, *, adaptive: Optional[str] = None, threshold: Optional[builtins.float] = None) -> "Expr":
        """Match descendant elements using a CSS selector.

        When `adaptive="key"` is set, the first successful match stores an element fingerprint under `key`. On later
        documents where `selector` matches nothing, the DOM engine scores candidates and relocates the closest element
        at or above `threshold`. Adaptive matching disables `stream` on `backend="auto"`.

        Args:
            selector: CSS selector string (without Scrapy/Parsel `::text` or `::attr(...)` pseudo-elements).
            adaptive: Unique fingerprint store key enabling structural self-healing.
            threshold: Minimum similarity score in `[0.0, 1.0]` for adaptive relocation (default `0.5`).

        Returns:
            New `Expr` holding the matched element(s).

        Raises:
            TypeError: If `threshold` is given without `adaptive`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("h1").text()).extract("<h1>Hello</h1>")
            'Hello'
            >>> p = compile(T.css(".price", adaptive="price").text())
            >>> _ = p.extract('<div id="card"><span class="price">$50</span></div>')
            >>> p.extract('<div id="card"><span class="cost">$50</span></div>')
            '$50'
        """
        if adaptive is None:
            if threshold is not None:
                raise TypeError("`threshold` only applies to adaptive selectors")
            return self._then({"css": selector})
        spec: Dict[str, Any] = {"css": selector, "key": adaptive}
        if threshold is not None:
            spec["threshold"] = threshold
        return self._then({"adaptive": spec})

    def xpath(self, selector: str) -> "Expr":
        """Match nodes or evaluate an XPath 1.0 expression against the current context (DOM engines only).

        Element paths return node sets; terminal `/text()` or `/@attr` steps return lists of strings; scalar XPath
        functions (`count(...)`, `string(...)`) return numbers, strings, or booleans.

        Args:
            selector: XPath 1.0 expression string.

        Returns:
            New `Expr` with XPath results.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.xpath("//li/text()")).extract("<ul><li>A</li><li>B</li></ul>")
            ['A', 'B']
        """
        return self._then({"xpath": selector})

    def parent(self) -> "Expr":
        """Navigate to the parent element of the current node (DOM engines only).

        Returns:
            New `Expr` pointing to the parent element, or `None` at the root.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("b").parent().attr("id")).extract("<div id='box'><b>x</b></div>")
            'box'
        """
        return self._then("parent")

    def children(self) -> "Expr":
        """Select direct child elements (or child entries on a JSON container).

        Returns:
            New `Expr` holding the child nodes or values.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("ul").children().map(T.text())).extract("<ul><li>1</li><li>2</li></ul>")
            ['1', '2']
        """
        return self._then("children")

    def root(self) -> "Expr":
        """Reset navigation back to the document root node (DOM engines only).

        Returns:
            New `Expr` referring to the document root.

        Examples:
            >>> from oxide_parse import T, compile
            >>> schema = T.css(".card").map({"title": T.root().css("h1").text(), "item": T.text()})
            >>> compile(schema).extract("<h1>Shop</h1><div class='card'>A</div>")
            [{'title': 'Shop', 'item': 'A'}]
        """
        return self._then("root")

    def key(self, name: str) -> "Expr":
        """Read key `name` from a dict/JSON object or parsed `parse_json()`/`js_json()`/`json_ld()`/`meta_tags()` map.

        Args:
            name: Mapping key name.

        Returns:
            New `Expr` yielding the key value, or `None` if the key is absent.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.key("user").key("name"), backend="json").extract({"user": {"name": "Ada"}})
            'Ada'
        """
        return self._then({"key": name})

    def at(self, i: int) -> "Expr":
        """Select element at zero-based index `i` from a list or node list (non-negative; use `.last()` or `.slice(-1)` for the end).

        Args:
            i: Zero-based non-negative element index (`i >= 0`).

        Returns:
            New `Expr` yielding the element, or `None` if out of range.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("li").at(1).text()).extract("<ul><li>A</li><li>B</li></ul>")
            'B'
        """
        return self._then({"at": i})

    def first(self) -> "Expr":
        """Select the first node from a node set, first item of a list, or first child of one node.

        Returns:
            New `Expr` holding the first item, or `None` when empty.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("li").first().text()).extract("<ul><li>A</li><li>B</li></ul>")
            'A'
        """
        return self._then("first")

    def last(self) -> "Expr":
        """Select the last node from a node set, last item of a list, or last child of one node.

        Returns:
            New `Expr` holding the last item, or `None` when empty.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("li").last().text()).extract("<ul><li>A</li><li>B</li></ul>")
            'B'
        """
        return self._then("last")

    def __getitem__(self, k: Any) -> "Expr":
        """Index a map by key, a sequence by position, or slice `[start:stop]`.

        Args:
            k: `str` key (delegates to `.key()`), `int` position (delegates to `.at()`), or `slice` without step.

        Returns:
            New `Expr` applying key lookup, indexing, or slicing.

        Raises:
            TypeError: If `k` is a stepped slice (`[::2]`) or not a `str`, `int`, or `slice`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T["items"][1]["sku"], backend="json").extract({"items": [{"sku": "A"}, {"sku": "B"}]})
            'B'
            >>> compile(T.css("li")[1:].map(T.text())).extract("<ul><li>1</li><li>2</li><li>3</li></ul>")
            ['2', '3']
        """
        if isinstance(k, str):
            return self.key(k)
        if isinstance(k, int):
            return self.at(k)
        if isinstance(k, slice):
            if k.step not in (None, 1):
                raise TypeError("T[a:b] does not support a step")
            return self.slice(k.start, k.stop)
        raise TypeError("T[...] accepts a str (key), an int (position) or a slice")

    def slice(self, start: Optional[builtins.int] = None, stop: Optional[builtins.int] = None) -> "Expr":
        """Slice `[start:stop]` over a node list, JSON list, string (by characters), or children of a single node.

        Negative bounds count from the end and out-of-range bounds are clamped following Python slicing rules.

        Args:
            start: Inclusive start index (default `0`).
            stop: Exclusive end index (default length of collection).

        Returns:
            New `Expr` with the sliced subsequence.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("li").slice(1, 3).map(T.text())).extract("<ul><li>a</li><li>b</li><li>c</li><li>d</li></ul>")
            ['b', 'c']
            >>> compile(T.css("p").text().slice(-3, None)).extract("<p>abcdef</p>")
            'def'
        """
        spec: Dict[str, Any] = {}
        if start is not None:
            spec["start"] = builtins.int(start)
        if stop is not None:
            spec["stop"] = builtins.int(stop)
        return self._then({"slice": spec})

    def rest(self) -> "Expr":
        """Return all elements except the first (`slice(1)`), ideal for stripping section headers inside `group_by`.

        Returns:
            New `Expr` holding items from index 1 onward.

        Examples:
            >>> from oxide_parse import T, compile
            >>> html = "<div><h3>Specs</h3><p>RAM</p><p>SSD</p></div>"
            >>> schema = T.css("div").group_by("h3").map({"h": T.first().text(), "v": T.rest().map(T.text())})
            >>> compile(schema).extract(html)
            [{'h': 'Specs', 'v': ['RAM', 'SSD']}]
        """
        return self.slice(1)

    # -- extraction --------------------------------------------------------
    def attr(self, name: str) -> "Expr":
        """Read attribute `name` from the first matched node (or JSON key).

        Args:
            name: Case-insensitive HTML attribute name (e.g., `"href"`, `"data-id"`).

        Returns:
            New `Expr` yielding the attribute value string, or `None` if missing.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("a").attr("href")).extract('<a href="/home">Home</a>')
            '/home'
        """
        return self._then({"attr": name})

    def text(
        self,
        sep: Optional[str] = None,
        *,
        strip: builtins.bool = False,
        without: Optional[str] = None,
        br: Optional[str] = None,
        deep: builtins.bool = True,
    ) -> "Expr":
        """Extract descendant (or direct-child when `deep=False`) text of the first matched element.

        Omits nested `<script>` and `<style>` text automatically.

        Args:
            sep: String joined between text fragments (matches BeautifulSoup's `get_text(separator)`).
            strip: Trim whitespace around each fragment and drop empty pieces before joining.
            without: CSS selector of descendant subtrees whose text should be skipped (e.g., `".ad, .badge"`).
            br: Replacement emitted at `<br>` tags instead of `sep` (e.g., `br="\\n"`).
            deep: When `False`, read only direct child text nodes (ignoring nested elements, like CSS `::own_text`).

        Returns:
            New `Expr` yielding the assembled text string, or `None` if no node matched.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("p").text()).extract("<p>Hello <b>world</b><script>x()</script></p>")
            'Hello world'
            >>> html = '<p>OLED<br>120 Hz<span class="ad">buy</span></p>'
            >>> compile(T.css("p").text(" ", strip=True, without=".ad", br="\\n")).extract(html)
            'OLED\\n120 Hz'
        """
        if deep and not sep and not strip and without is None and br is None:  # `sep=""` joins directly: identical to plain text()
            return self._then("text")
        spec: Dict[str, Any] = {"sep": sep or "", "strip": strip}
        if without is not None:
            spec["without"] = without
        if br is not None:
            spec["br"] = br
        if not deep:
            spec["deep"] = False
        return self._then({"text_opts": spec})

    def own_text(
        self,
        sep: Optional[str] = None,
        *,
        strip: builtins.bool = False,
        br: Optional[str] = None,
    ) -> "Expr":
        """Extract only direct child text nodes of the first matched element, ignoring nested element text.

        Equivalent to `.text(sep, strip=strip, br=br, deep=False)`.

        Args:
            sep: String joined between direct child text fragments.
            strip: Trim whitespace around each piece and omit blank fragments before joining.
            br: Replacement string emitted at direct `<br>` children instead of `sep`.

        Returns:
            New `Expr` yielding the joined direct-child text, or `None` if no element matched.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("div").own_text(strip=True)).extract("<div>100<span>USD</span></div>")
            '100'
        """
        return self.text(sep, strip=strip, br=br, deep=False)

    def own_texts(self) -> "Expr":
        """Extract non-empty direct child text nodes of the first matched element as a list of strings.

        Ignores text inside nested child elements while keeping each direct text fragment separated.

        Returns:
            New `Expr` yielding a `list[str]`, or `None` if no element matched.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("div").own_texts()).extract("<div>A<b>1</b>B</div>")
            ['A', 'B']
        """
        return self._then("own_texts")

    def table(
        self,
        mode: Literal["records", "grid", "split"] = "records",
        *,
        strip: builtins.bool = True,
        header_sep: Optional[str] = None,
    ) -> "Expr":
        """Extract an HTML `<table>` into structured rows with full `rowspan`/`colspan` expansion.

        Ignores nested sub-tables inside cells and treats `rowspan="0"` as extending to the end of its row group.

        Args:
            mode: Output format: `"records"` (list of dicts keyed by normalized headers), `"grid"` (2D `list[list[str]]`),
                or `"split"` (`{"headers": list[list[str]], "rows": list[list[str]]}`).
            strip: Trim cell text and normalize empty padding (default `True`).
            header_sep: Separator joining multi-row column headers (default `" / "`).

        Returns:
            New `Expr` yielding the structured table representation, or `None` if no element matched.

        Raises:
            ValueError: If `mode` is not `"records"`, `"grid"`, or `"split"`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> html = "<table><thead><tr><th>Item</th><th>Qty</th></tr></thead><tbody><tr><td>A</td><td>2</td></tr></tbody></table>"
            >>> compile(T.css("table").table()).extract(html)
            [{'Item': 'A', 'Qty': '2'}]
        """
        if mode not in ("records", "grid", "split"):
            raise ValueError('table(): mode must be "records", "grid", or "split"')
        spec: Dict[str, Any] = {}
        if mode != "records":
            spec["mode"] = mode
        if not strip:
            spec["strip"] = False
        if header_sep is not None:
            spec["header_sep"] = header_sep
        return self._then({"table": spec})

    def markdown(self, *, without: Optional[str] = None) -> "Expr":
        """Render the first matched element as Markdown for LLM/RAG pipelines (DOM engines only).

        Converts headings, paragraphs, lists, links, images, emphasis, code, blockquotes, and tables while dropping
        `<script>`, `<style>`, `<svg>`, and subtrees matched by `without`.

        Args:
            without: Optional CSS selector of subtrees to omit before rendering.

        Returns:
            New `Expr` yielding a Markdown string, or `None` if no element matched.

        Examples:
            >>> from oxide_parse import T, compile
            >>> html = "<article><h1>Title</h1><div class='ad'>Ad</div><p>Body</p></article>"
            >>> compile(T.css("article").markdown(without=".ad")).extract(html)
            '# Title\\n\\nBody'
        """
        return self._then({"markdown": {} if without is None else {"without": without}})

    def distill(self, keep: builtins.int = 2, max_text: builtins.int = 80, max_chars: builtins.int = 20000) -> "Expr":
        """Shrink the first matched element into a compact HTML skeleton (requires the `lite` feature).

        Drops scripts, styles, SVGs, and most attributes; clips text nodes; folds repeated similar siblings into
        `<!-- +N similar ... -->` comments.

        Args:
            keep: Number of similar sibling elements kept before folding the remainder.
            max_text: Maximum characters retained per text node.
            max_chars: Hard character cap on the returned HTML skeleton.

        Returns:
            New `Expr` yielding the skeleton HTML string, or `None` if no node matched.

        Examples:
            >>> from oxide_parse import T, compile
            >>> html = "<main><ul>" + "".join(f"<li class='i'><a href='/{i}'>Item {i}</a></li>" for i in range(5)) + "</ul></main>"
            >>> "<!-- +3 similar" in compile(T.css("main").distill(keep=2, max_chars=2000)).extract(html)
            True
        """
        return self._then({"distill": {"keep": keep, "max_text": max_text, "max_chars": max_chars}})

    def url(self, base: Any = None) -> "Expr":
        """Resolve a relative URL according to RFC 3986.

        Resolution priority matches browsers: `base` (if passed) -> `base` resolved vs nothing; otherwise `<base href>`
        resolved against `meta["base_url"]`, falling back directly to `meta["base_url"]`.

        Args:
            base: Explicit base URL string or `Expr` overriding `meta["base_url"]` and `<base href>`.

        Returns:
            New `Expr` yielding the resolved absolute URL string.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("a").attr("href").url()).extract('<a href="../p/1">x</a>', meta={"base_url": "https://shop.test/cat/list"})
            'https://shop.test/p/1'
            >>> compile(T.css("a").attr("href").url("https://cdn.test/img/")).extract('<a href="a.png">i</a>')
            'https://cdn.test/img/a.png'
        """
        return self._then({"resolve_url": {} if base is None else {"base": to_ir(base)}})

    def js_json(self, var: Optional[str] = None) -> "Expr":
        """Extract JSON from JavaScript source code without running a JS engine.

        Understands variable assignments (`window.__STATE__ = {...};`), `JSON.parse("...")`, and JSON literals. Parsed
        objects are cached per `(node, var)` within a document run.

        Args:
            var: Optional JS variable/property assignment target; when `None`, uses the first `{...}` or `[...]` literal.

        Returns:
            New `Expr` yielding the parsed JSON structure.

        Examples:
            >>> from oxide_parse import T, compile
            >>> html = '<script id="s">window.__STATE__ = {"cart": {"count": 3}};</script>'
            >>> compile(T.css("#s").text().js_json("window.__STATE__")["cart"]["count"]).extract(html)
            3
        """
        return self._then({"js_json": {} if var is None else {"var": var}})

    def sections(self, header: str, items: Optional[str] = None) -> "Expr":
        """Split flat sibling children at each `header` into `[{"header": str, "items": [str, ...]}]` dicts.

        Args:
            header: CSS selector matching section heading elements among direct children.
            items: Optional CSS selector filtering sibling content elements belonging to each section.

        Returns:
            New `Expr` producing a list of section dictionaries (DOM engines only).

        Examples:
            >>> from oxide_parse import T, compile
            >>> html = "<div class='s'><h3>CPU</h3><p>M4</p><h3>RAM</h3><p>16GB</p></div>"
            >>> compile(T.css(".s").sections("h3", items="p")).extract(html)
            [{'header': 'CPU', 'items': ['M4']}, {'header': 'RAM', 'items': ['16GB']}]
        """
        spec: Dict[str, Any] = {"header": header}
        if items is not None:
            spec["items"] = items
        return self._then({"sections": spec})

    def group_by(self, header: str, items: Optional[str] = None) -> "Expr":
        """Group flat sibling elements starting at each `header`; must be followed directly by `.map(...)`.

        Inside `.map(...)`, the context is the node list of one group: `T.first()` selects the header element and
        `T.rest()` returns all subsequent members of the group.

        Args:
            header: CSS selector starting a new group whenever a direct child matches.
            items: Optional CSS selector restricting non-header members included in each group.

        Returns:
            New `Expr` yielding groups of nodes ready for `.map(...)` (DOM engines only).

        Examples:
            >>> from oxide_parse import T, compile
            >>> html = "<div class='s'><h3>A</h3><a href='/1'>one</a><h3>B</h3><p><a href='/2'>two</a></p></div>"
            >>> schema = T.css(".s").group_by("h3").map({"title": T.first().text(), "links": T.css("a").map(T.attr("href"))})
            >>> compile(schema).extract(html)
            [{'title': 'A', 'links': ['/1']}, {'title': 'B', 'links': ['/2']}]
        """
        spec: Dict[str, Any] = {"header": header}
        if items is not None:
            spec["items"] = items
        return self._then({"group_by": spec})

    def json_ld(self, schema_type: Optional[str] = None, *, resolve_refs: builtins.bool = False) -> "Expr":
        """Extract JSON-LD objects from `<script type="application/ld+json">` tags (DOM engines only).

        Unfolds top-level arrays and `@graph` containers, strips HTML comment wrappers, and ignores invalid JSON blocks.

        Args:
            schema_type: Optional `@type` filter (matches both short names like `"Product"` and `schema.org` URIs).
            resolve_refs: When `True`, expand `{"@id": ...}` references against matching nodes in the same document.

        Returns:
            New `Expr` producing a list of parsed JSON-LD dictionaries.

        Examples:
            >>> from oxide_parse import T, compile
            >>> html = '<script type="application/ld+json">{"@type":"Product","name":"Chair"}</script>'
            >>> compile(T.json_ld("Product")[0]["name"]).extract(html)
            'Chair'
        """
        spec: Dict[str, Any] = {}
        if schema_type is not None:
            spec["schema_type"] = schema_type
        if resolve_refs:
            spec["resolve_refs"] = True
        return self._then({"json_ld": spec})

    def microdata(self, schema_type: Optional[str] = None) -> "Expr":
        """Extract HTML Microdata (`itemscope`, `itemprop`, `itemtype`, `itemref`, `itemid`) as dicts (DOM engines only).

        Nested `itemscope` values become nested dictionaries, repeated `itemprop` properties become lists, and
        tag-specific value attributes (`content`, `src`, `href`, `data`, `value`, `datetime`) are honored per WHATWG.

        Args:
            schema_type: Optional `itemtype` filter (matches short names like `"Product"` or full schema URLs).

        Returns:
            New `Expr` yielding a list of extracted Microdata dictionaries.

        Examples:
            >>> from oxide_parse import T, compile
            >>> html = '<div itemscope itemtype="https://schema.org/Product"><span itemprop="name">Tea</span></div>'
            >>> compile(T.microdata("Product")[0]["name"], backend="lite").extract(html)
            'Tea'
        """
        return self._then({"microdata": {} if schema_type is None else {"schema_type": schema_type}})

    def next_flight(self, key: Optional[str] = None) -> "Expr":
        """Decode Next.js App Router `self.__next_f.push(...)` RSC Flight payloads into JSON data.

        Combines multi-chunk `[1, "..."]` script streams, resolves `$<id>` / `$L<id>` / `$@<id>` row references,
        unescapes `$$`, and either returns decoded top-level objects or collects every matching `key` subtree.

        Args:
            key: Optional object key name to search for recursively across decoded Flight payloads.

        Returns:
            New `Expr` yielding a `list` of decoded objects (or matching field values when `key` is set).

        Examples:
            >>> import json
            >>> from oxide_parse import T, compile
            >>> row = '1:{"products":[{"id":1}]}'
            >>> html = "<script>self.__next_f.push([1," + json.dumps(row) + "])</script>"
            >>> compile(T.next_flight("products")[0][0]["id"], backend="lite").extract(html)
            1
        """
        return self._then({"next_flight": {} if key is None else {"key": key}})

    def main_content(self) -> "Expr":
        """Heuristically locate the primary article/content element, ignoring navbars, sidebars, and footers.

        Useful before `.markdown()` on unfamiliar page layouts whose main container selector varies (DOM engines only).

        Returns:
            New `Expr` pointing to the highest-scoring main content element, or `None` when empty.

        Examples:
            >>> from oxide_parse import T, compile
            >>> html = '<nav>Menu</nav><article><p>Long article body with details, examples, and prose.</p></article>'
            >>> compile(T.main_content().markdown(), backend="lite").extract(html)
            'Long article body with details, examples, and prose.'
        """
        return self._then("main_content")

    def meta_tags(self, prefixes: Optional[Sequence[str]] = ("og:", "twitter:"), *, values: Literal["auto", "first", "list"] = "auto") -> "Expr":
        """Collect `<meta property|name content>` tags into a dictionary with lowercased keys.

        Supported on `stream` (reads up to opening `<body>`) and DOM engines.

        Args:
            prefixes: Key prefixes to retain (default `("og:", "twitter:")`). Pass `[]` to capture every `<meta>` tag.
            values: Duplicate-key policy: `"auto"` (single string, repeated list), `"first"` (always first string),
                or `"list"` (always a list).

        Returns:
            New `Expr` yielding a dictionary of extracted meta tag values.

        Raises:
            ValueError: If `values` is not `"auto"`, `"first"`, or `"list"`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> html = '<head><meta property="og:title" content="Book"><meta property="og:image" content="1.jpg"><meta property="og:image" content="2.jpg"></head>'
            >>> compile(T.meta_tags()).extract(html)
            {'og:title': 'Book', 'og:image': ['1.jpg', '2.jpg']}
            >>> compile(T.meta_tags(values="first")["og:image"]).extract(html)
            '1.jpg'
        """
        if values not in ("auto", "first", "list"):
            raise ValueError('values must be "auto", "first" or "list"')
        spec: Dict[str, Any] = {"prefixes": list(prefixes or [])}
        if values != "auto":
            spec["values"] = values
        return self._then({"meta_tags": spec})

    def kv(self, label: str, *, exact: builtins.bool = True) -> "Expr":
        """Select the adjacent value element next to a label in spec tables, `<dl>` lists, or inline rows.

        Compiles to a scoped XPath (`th`/`dt`/`td`/`label`/`strong`/`b`/`span`/`div` + `following-sibling::*[1]`), so
        when used inside `.map(...)` it stays scoped to each card (DOM engines only).

        Args:
            label: Text label to match.
            exact: If `True` (default), allow at most 3 trailing punctuation/colon characters; if `False`, substring match.

        Returns:
            New `Expr` pointing to the following sibling value element(s).

        Examples:
            >>> from oxide_parse import T, compile
            >>> html = "<table><tr><th>Weight:</th><td> 1.2 kg </td></tr></table>"
            >>> compile(T.kv("Weight").text().strip()).extract(html)
            '1.2 kg'
        """
        lit = _xpath_literal(label)
        cond = (
            f"starts-with(normalize-space(.), {lit}) and string-length(normalize-space(.)) <= string-length({lit}) + 3"
            if exact
            else f"contains(normalize-space(.), {lit})"
        )
        tags = " or ".join(f"self::{t}" for t in ("th", "dt", "td", "label", "strong", "b", "span", "div"))
        return self._then({"xpath": f".//*[{tags}][{cond}]/following-sibling::*[1]"})

    def has_attr(self, name: str) -> "Expr":
        """Check whether the first matched element defines attribute `name` (including boolean attributes).

        Args:
            name: Case-insensitive HTML attribute name.

        Returns:
            New `Expr` evaluating to `True` or `False` (`False` when empty/missing).

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("button").has_attr("disabled")).extract("<button disabled>Save</button>")
            True
        """
        return self._then({"has_attr": name})

    def html(self) -> "Expr":
        """Return outer HTML markup of the first matched element (DOM engines only).

        Returns:
            New `Expr` yielding an outer HTML string, or `None` if no node matched.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("b").html()).extract("<div><b>hi</b></div>")
            '<b>hi</b>'
        """
        return self._then("html")

    def inner_html(self) -> "Expr":
        """Return inner HTML markup inside the first matched element (DOM engines only).

        Returns:
            New `Expr` yielding an inner HTML string, or `None` if no node matched.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("div").inner_html()).extract("<div><b>hi</b></div>")
            '<b>hi</b>'
        """
        return self._then("inner_html")

    def meta(self, key: str) -> "Expr":
        """Read `key` from the `meta` dictionary passed to `extract(..., meta=...)` or `follow`/`paginate(..., meta=...)`.

        Args:
            key: Metadata dictionary key.

        Returns:
            New `Expr` yielding the metadata value, or `None` if absent.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile({"src": T.meta("source")}).extract("<p/>", meta={"source": "catalog"})
            {'src': 'catalog'}
        """
        return self._then({"meta": key})

    def index(self) -> "Expr":
        """Return the zero-based position of the current item inside `.map(...)`.

        When `on_error="skip"` drops earlier items, surviving items retain their original zero-based source index.

        Returns:
            New `Expr` yielding an `int` inside `.map(...)` (`None` at the root).

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("li").map({"i": T.index(), "v": T.text()})).extract("<ul><li>A</li><li>B</li></ul>")
            [{'i': 0, 'v': 'A'}, {'i': 1, 'v': 'B'}]
        """
        return self._then("index")

    # -- collections -------------------------------------------------------
    def count(self) -> "Expr":
        """Count elements in a node set, JSON list/map, or children of a single element.

        Returns:
            New `Expr` yielding an `int` (`0` when `None` or empty).

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("li").count()).extract("<ul><li>a</li><li>b</li></ul>")
            2
        """
        return self._then("count")

    def exists(self) -> "Expr":
        """Check whether the current value or node set is present and non-empty.

        Returns:
            New `Expr` yielding `True` or `False`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile({"ok": T.css(".badge").exists()}).extract("<span class='badge'>New</span>")
            {'ok': True}
        """
        return self._then("exists")

    def map(self, body: Any, on_error: Optional[Literal["raise", "skip", "null"]] = None) -> "Expr":
        """Evaluate `body` for every element in a node list, JSON array, or child list of one element.

        Args:
            body: Schema expression, dict, list, or literal evaluated with `T` bound to each item.
            on_error: Record error policy: `"raise"` / `None` (fail the page), `"skip"` (omit failing records and
                report them in `Detailed.errors`), or `"null"` (insert `None` and report).

        Returns:
            New `Expr` producing a list of mapped results (`None` when input is `None`).

        Raises:
            ValueError: If `on_error` is not `"raise"`, `"skip"`, `"null"`, or `None`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> html = "<div><b>1</b></div><div><b>bad</b></div><div><b>3</b></div>"
            >>> compile(T.css("div").map(T.css("b").text().int(), on_error="skip")).extract(html)
            [1, 3]
        """
        if on_error is None or on_error == "raise":
            return self._then({"map": to_ir(body)})
        if on_error not in ("skip", "null"):
            raise ValueError('on_error must be "raise", "skip" or "null"')
        return self._then({"map_opts": {"body": to_ir(body), "on_error": on_error}})

    def flat_map(self, body: Any) -> "Expr":
        """Map `body` over each item and flatten nested lists/node sets one level.

        Args:
            body: Expression yielding a list (or scalar appended directly) per input item.

        Returns:
            New `Expr` yielding a flat list.

        Examples:
            >>> from oxide_parse import T, compile
            >>> html = "<ul><li>a,b</li><li>c</li></ul>"
            >>> compile(T.css("li").flat_map(T.text().split(","))).extract(html)
            ['a', 'b', 'c']
        """
        return self._then({"flat_map": to_ir(body)})

    def filter(self, pred: Any) -> "Expr":
        """Keep items in a node set or list where `pred` evaluates truthy.

        Args:
            pred: Predicate expression evaluated per item (usually using `==`, `!=`, `&`, `|`, `~`, `.contains()`).

        Returns:
            New `Expr` containing only matching items (DOM engines for node lists; all engines on JSON/value lists).

        Examples:
            >>> from oxide_parse import T, compile
            >>> html = "<ul><li data-on='1'>A</li><li data-on='0'>B</li></ul>"
            >>> compile(T.css("li").filter(T.attr("data-on") == "1").map(T.text())).extract(html)
            ['A']
        """
        return self._then({"filter": to_ir(pred)})

    def unique(self, by: Any = None, *, keep: Literal["first", "last"] = "first") -> "Expr":
        """Deduplicate items in a list while preserving original order (null-safe).

        Args:
            by: Optional key expression evaluated per item (`T` bound to each item). Without it, items themselves
                are compared by structural canonical equality.
            keep: Duplicate resolution policy: `"first"` (default, keeps first occurrence position and value) or
                `"last"` (keeps first-seen slot updated with the last duplicate value).

        Returns:
            New `Expr` yielding a deduplicated `list`, or `None` if the input was `None`.

        Raises:
            ValueError: If `keep` is not `"first"` or `"last"`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("li").map(T.text()).unique()).extract("<ul><li>a</li><li>b</li><li>a</li></ul>")
            ['a', 'b']
        """
        if keep not in ("first", "last"):
            raise ValueError('unique(): keep must be "first" or "last"')
        spec: Dict[str, Any] = {}
        if by is not None:
            spec["by"] = to_ir(by)
        if keep != "first":
            spec["keep"] = keep
        return self._then({"unique": spec})

    # -- null handling -----------------------------------------------------
    def default(self, value: Any) -> "Expr":
        """Substitute `value` when the incoming value or node set is missing (`None` or empty node set).

        Note that `value` is evaluated against the enclosing **context** (e.g., the current card in `.map(...)`),
        allowing fallbacks like `T.css(".sale").text().default(T.css(".regular").text())`.

        Args:
            value: Literal or `Expr` fallback evaluated when left-hand side is missing.

        Returns:
            New `Expr` returning either the present left-hand value or evaluated `value`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css(".price").text().default("N/A")).extract("<p>No price</p>")
            'N/A'
        """
        return self._then({"default": to_ir(value)})

    def required(self) -> "Expr":
        """Fail extraction with a field-path error if the incoming value is missing (`None` or empty node set).

        Returns:
            New `Expr` passing non-missing values through unchanged.

        Examples:
            >>> from oxide_parse import T, compile
            >>> try:
            ...     compile({"sku": T.css(".sku").text().required()}).extract("<p/>")
            ... except Exception as exc:
            ...     "$.sku" in str(exc)
            True
        """
        return self._then("required")

    def non_empty(self) -> "Expr":
        """Turn whitespace-only strings, empty lists, empty maps, or empty node sets into `None`.

        Useful before `.default(...)` so blank HTML elements fall back cleanly.

        Returns:
            New `Expr` yielding `None` on empty values or the original value otherwise.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("span").text().non_empty().default("unknown")).extract("<span>   </span>")
            'unknown'
        """
        return self._then("non_empty")

    def coalesce(self, *alternatives: Any) -> "Expr":
        """Evaluate `alternatives` in order against the current context and return the first non-missing result.

        Args:
            *alternatives: Literal or `Expr` expressions tried left-to-right.

        Returns:
            New `Expr` returning the first non-missing alternative, or `None` if all miss.

        Examples:
            >>> from oxide_parse import T, compile, coalesce
            >>> html = "<a title='Full Title'>Short</a>"
            >>> compile(T.css("a").map(coalesce(T.attr("data-name"), T.attr("title"), T.text()))).extract(html)
            ['Full Title']
        """
        return self._then({"coalesce": [to_ir(a) for a in alternatives]})

    # -- strings -----------------------------------------------------------
    def strip(self) -> "Expr":
        """Trim leading and trailing Unicode whitespace from a string (null-safe).

        Returns:
            New `Expr` yielding the trimmed string, or `None`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("h1").text().strip()).extract("<h1>  Hi  </h1>")
            'Hi'
        """
        return self._then("strip")

    def lower(self) -> "Expr":
        """Convert a string value to lowercase (null-safe).

        Returns:
            New `Expr` yielding the lowercased string, or `None`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("h1").text().lower()).extract("<h1>HELLO</h1>")
            'hello'
        """
        return self._then("lower")

    def upper(self) -> "Expr":
        """Convert a string value to uppercase (null-safe).

        Returns:
            New `Expr` yielding the uppercased string, or `None`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("h1").text().upper()).extract("<h1>hello</h1>")
            'HELLO'
        """
        return self._then("upper")

    def replace(self, old: str, new: str) -> "Expr":
        """Replace all literal occurrences of `old` with `new` in a string (null-safe).

        Args:
            old: Substring to find.
            new: Replacement string.

        Returns:
            New `Expr` yielding the updated string, or `None`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("p").text().replace("foo", "bar")).extract("<p>foo 1</p>")
            'bar 1'
        """
        return self._then({"replace": {"from": old, "to": new}})

    def split(self, sep: str) -> "Expr":
        """Split a string on separator `sep` into a list of strings (null-safe).

        Args:
            sep: Non-empty delimiter string (empty string yields character strings).

        Returns:
            New `Expr` yielding a `list[str]`, or `None`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("p").text().split(",")).extract("<p>a,b,c</p>")
            ['a', 'b', 'c']
        """
        return self._then({"split": sep})

    def join(self, sep: str) -> "Expr":
        """Join elements of a list (converting scalars to strings) with separator `sep` (null-safe).

        Args:
            sep: Separator inserted between items.

        Returns:
            New `Expr` yielding the joined string, or `None`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("li").map(T.text()).join(" | ")).extract("<ul><li>A</li><li>B</li></ul>")
            'A | B'
        """
        return self._then({"join": sep})

    def regex(self, pattern: str, group: int = 0) -> "Expr":
        """Return the first Rust regex match (or capture group) from a string (null-safe).

        Args:
            pattern: Regular expression pattern (`regex` crate syntax; look-around unsupported).
            group: Capture group index (`0` = full match, `1+` = parenthesized group).

        Returns:
            New `Expr` yielding the matched string, or `None` when input is `None` or unmatched.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("p").text().regex(r"ID:(\\d+)", 1)).extract("<p>User ID:42</p>")
            '42'
        """
        return self._then({"regex": {"pattern": pattern, "group": group}})

    def regex_all(self, pattern: str, group: int = 0) -> "Expr":
        """Return all non-overlapping Rust regex matches (or capture group `group`) as a list.

        Args:
            pattern: Regular expression pattern.
            group: Capture group index (`0` = full match).

        Returns:
            New `Expr` yielding a `list[str]`, or `None` when input is `None`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("p").text().regex_all(r"\\d+")).extract("<p>a1 b22 c3</p>")
            ['1', '22', '3']
        """
        return self._then({"regex_all": {"pattern": pattern, "group": group}})

    # -- conversion --------------------------------------------------------
    def int(self) -> "Expr":
        """Parse an integer from strings or numbers, ignoring surrounding currency labels/separators.

        Understands `"$1,299.50"` (`1299`), `"12 990 ₽"` (`12990`), and rejects strings without any number. Null-safe.

        Returns:
            New `Expr` yielding an `int`, or `None` when input is `None`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css(".qty").text().int()).extract("<span class='qty'>12 990 pcs</span>")
            12990
        """
        return self._then("int")

    def float(self) -> "Expr":
        """Parse a floating-point number including currency symbols and locale separators (null-safe).

        Handles `"$1,299.50"` (`1299.5`), `"1.299,50"` (`1299.5`), `"19,99 €"` (`19.99`), and avoids taking hyphens
        in tokens like `"Model-3"` as a minus sign. Fails with an extraction error when a non-null text has no number.

        Returns:
            New `Expr` yielding a `float`, or `None` when input is `None`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("i").text().float()).extract("<i>$1,299.50</i>")
            1299.5
            >>> compile(T.css("i").text().float()).extract("<i>19,99 EUR</i>")
            19.99
        """
        return self._then("float")

    def bool(self) -> "Expr":
        """Parse boolean-like strings (`"true"`, `"1"`, `"yes"`, `"false"`, `"0"`, `""`) or Python scalars (null-safe).

        Returns:
            New `Expr` yielding a `bool`, or `None` when input is `None`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("span").text().bool()).extract("<span>yes</span>")
            True
        """
        return self._then("bool")

    def parse_srcset(
        self,
        pick: Optional[Literal["max", "min"]] = None,
        *,
        min_width: Optional[builtins.int] = None,
        min_density: Optional[builtins.float] = None,
    ) -> "Expr":
        """Parse an HTML `srcset` attribute string according to WHATWG rules (null-safe).

        Correctly splits image candidates without breaking URLs that contain commas (such as CDN transformation paths).

        Args:
            pick: Optional selection policy: `None` returns a list of candidate dicts (`{"url", "width", "density"}`),
                `"max"` selects the largest candidate dict, and `"min"` selects the smallest.
            min_width: Optional minimum `w` descriptor (`>= 1`); selects the smallest candidate meeting it (or largest
                available fallback).
            min_density: Optional minimum `x` pixel-density descriptor (`> 0`); selects the tightest match (or highest
                available fallback).

        Returns:
            New `Expr` yielding candidate list or a single selected candidate dict, or `None` on empty/missing input.

        Raises:
            ValueError: If `pick`, `min_width`, or `min_density` is invalid.

        Examples:
            >>> from oxide_parse import T, compile
            >>> img = '<img srcset="/cdn-cgi/image/w=400,f=auto/a.jpg 400w, /cdn-cgi/image/w=800,f=auto/a.jpg 800w">'
            >>> compile(T.css("img").attr("srcset").parse_srcset("max")["url"]).extract(img)
            '/cdn-cgi/image/w=800,f=auto/a.jpg'
        """
        if pick not in (None, "max", "min"):
            raise ValueError('parse_srcset(): pick must be None, "max", or "min"')
        if min_width is not None and (isinstance(min_width, builtins.bool) or not isinstance(min_width, builtins.int) or min_width < 1):
            raise ValueError("parse_srcset(): min_width must be a positive integer (>= 1)")
        if min_density is not None and (isinstance(min_density, builtins.bool) or not isinstance(min_density, (builtins.int, builtins.float)) or min_density <= 0):
            raise ValueError("parse_srcset(): min_density must be positive")
        spec: Dict[str, Any] = {}
        if pick is not None:
            spec["pick"] = pick
        if min_width is not None:
            spec["min_width"] = min_width
        if min_density is not None:
            spec["min_density"] = builtins.float(min_density)
        return self._then({"parse_srcset": spec})

    def price(self, *, decimal: Optional[str] = None, currency: Optional[str] = None) -> "Expr":
        """Parse a locale-aware price string into `{"amount": float, "currency": str | None}` (null-safe).

        Recognizes ISO 4217 currency codes, prefixes/symbols (`$`, `US$`, `€`, `£`, `₽`, `₺`, `₴`, `₸`, etc.),
        and thousand/decimal formats, returning `None` when the string contains no numeric price.

        Args:
            decimal: Optional explicit decimal separator (`"."` or `","`) when ambiguous; auto-detected by default.
            currency: Optional fallback currency code when only an ambiguous symbol or no symbol is present.

        Returns:
            New `Expr` yielding `{"amount": float, "currency": str | None}`, or `None` if absent/non-numeric.

        Raises:
            ValueError: If `decimal` is provided and not `"."` or `","`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("b").text().price()).extract("<b>1 299,50 EUR</b>")
            {'amount': 1299.5, 'currency': 'EUR'}
        """
        if decimal is not None and decimal not in (".", ","):
            raise ValueError('price(): decimal must be ".", ",", or None')
        spec: Dict[str, Any] = {}
        if decimal is not None:
            spec["decimal"] = decimal
        if currency is not None:
            spec["currency"] = currency
        return self._then({"price": spec})

    def parse_json(self) -> "Expr":
        """Parse a string or element text as JSON so keys/indices can navigate inside it.

        Adjacent `[key]`/`[idx]` steps fuse into zero-copy `json_path` lookups, and large blocks (512+ bytes) are cached
        once per document run across fields.

        Returns:
            New `Expr` yielding the parsed JSON value, or `None` when input is `None`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> html = '<script id="d">{"props":{"price":199}}</script>'
            >>> compile(T.css("script#d").text().parse_json()["props"]["price"]).extract(html)
            199
        """
        return self._then("parse_json")

    def parse_html(self, schema: Any) -> "Expr":
        """Parse a string value (e.g., HTML inside JSON or `iframe[srcdoc]`) with a sub-schema.

        Compiles `schema` on the `"auto"` HTML engine and executes it against the string value at this step, returning
        `None` cleanly when the input is `None`.

        Args:
            schema: Sub-schema (`dict`, `list`, or `Expr`) applied to the parsed HTML fragment.

        Returns:
            New `Expr` yielding the extracted result from the HTML fragment, or `None` on missing input.

        Examples:
            >>> from oxide_parse import T, compile
            >>> plan = compile(T["html"].parse_html(T.css("li").map(T.text())), backend="json")
            >>> plan.extract({"html": "<ul><li>A</li><li>B</li></ul>"})
            ['A', 'B']
        """
        return self._then({"parse_html": to_ir(schema)})

    def link_header(self, rel: Optional[str] = "next") -> "Expr":
        """Parse an HTTP `Link` header string (RFC 8288) into a URL or `{rel: url}` map.

        Args:
            rel: Link relation name to select (default `"next"`, case-insensitive), or `None` to return the full
                `{rel: url}` dictionary.

        Returns:
            New `Expr` yielding the matched URL string, `None` when absent, or a dict of all relations when `rel=None`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> hdr = '<https://api.ex/items?page=2>; rel="next", <https://api.ex/items?page=9>; rel="last"'
            >>> compile(T.meta("headers")["link"].link_header("next"), backend="json").extract({}, meta={"headers": {"link": hdr}})
            'https://api.ex/items?page=2'
        """
        return self._then({"link_header": {"rel": rel}})

    # -- logic -------------------------------------------------------------
    def eq(self, other: Any) -> "Expr":
        """Compare equality against `other` (also exposed as `expr == other`; `int` and `float` compare numerically).

        Args:
            other: Literal value or `Expr` evaluated against the current context.

        Returns:
            New `Expr` yielding a `bool`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("span").text().eq("OK")).extract("<span>OK</span>")
            True
        """
        return self._then({"eq": to_ir(other)})

    def ne(self, other: Any) -> "Expr":
        """Compare inequality against `other` (also exposed as `expr != other`).

        Args:
            other: Literal value or `Expr` evaluated against the current context.

        Returns:
            New `Expr` yielding a `bool`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("span").text().ne("Sold out")).extract("<span>In stock</span>")
            True
        """
        return self._then({"ne": to_ir(other)})

    def contains(self, other: Any) -> "Expr":
        """Check substring, list membership, or dict key containment against `other`.

        Args:
            other: Substring / element / key literal or `Expr` evaluated against current context.

        Returns:
            New `Expr` yielding a `bool`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("p").text().contains("Sale")).extract("<p>Winter Sale</p>")
            True
        """
        return self._then({"contains": to_ir(other)})

    def startswith(self, other: Any) -> "Expr":
        """Check whether a string starts with `other`.

        Args:
            other: Prefix string literal or `Expr`.

        Returns:
            New `Expr` yielding a `bool`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("a").attr("href").startswith("https://")).extract('<a href="https://ex.test">x</a>')
            True
        """
        return self._then({"starts_with": to_ir(other)})

    def endswith(self, other: Any) -> "Expr":
        """Check whether a string ends with `other`.

        Args:
            other: Suffix string literal or `Expr`.

        Returns:
            New `Expr` yielding a `bool`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("a").attr("href").endswith(".pdf")).extract('<a href="/doc.pdf">x</a>')
            True
        """
        return self._then({"ends_with": to_ir(other)})

    def not_(self) -> "Expr":
        """Logical negation of the current value (also written `~expr`).

        Returns:
            New `Expr` yielding `not truthy(value)`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css("button").has_attr("disabled").not_()).extract("<button>Buy</button>")
            True
        """
        return self._then("not")

    def and_(self, other: Any) -> "Expr":
        """Short-circuiting logical AND (also written `expr & other`), with `other` evaluated against current context.

        Args:
            other: Right-hand expression or literal.

        Returns:
            New `Expr` yielding a `bool`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css(".in-stock").exists().and_(T.css(".price").exists())).extract('<i class="in-stock"></i><i class="price">5</i>')
            True
        """
        return self._then({"and": to_ir(other)})

    def or_(self, other: Any) -> "Expr":
        """Short-circuiting logical OR (also written `expr | other`), with `other` evaluated against current context.

        Args:
            other: Right-hand expression or literal.

        Returns:
            New `Expr` yielding a `bool`.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.css(".a").exists().or_(T.css(".b").exists())).extract('<i class="b">1</i>')
            True
        """
        return self._then({"or": to_ir(other)})

    def if_(self, cond: Any, then: Any, otherwise: Any = None) -> "Expr":
        """Branch conditionally inside a schema without Python `if/else`.

        Args:
            cond: Condition `Expr` evaluated against the current context.
            then: Expression returned when `cond` is truthy.
            otherwise: Expression returned otherwise (defaults to `None`).

        Returns:
            New `Expr` yielding the chosen branch.

        Examples:
            >>> from oxide_parse import T, compile
            >>> compile(T.if_(T.css(".sale").exists(), "sale", "regular")).extract("<b class='sale'>!</b>")
            'sale'
        """
        return self._then({"if": {"cond": to_ir(cond), "then": to_ir(then), "otherwise": to_ir(otherwise)}})

    def follow(
        self,
        request: Any,
        schema: Any,
        meta: Any = None,
        *,
        format: Optional[str] = None,
    ) -> "Expr":
        """Emit a zero-IO fetch request for a linked page and extract `schema` from its response.

        `request` can be a URL expression (`T.attr("href").url()`) or a dict whose evaluated entries become keyword
        arguments to your `fetch(**request)` callback (e.g., `{"url": ..., "headers": ..., "method": "POST"}`). If
        `request` or `"url"` evaluates to `None`, no request is emitted and the field yields `None`. The child document
        automatically inherits `"base_url"` from `request["url"]` unless overridden in `meta`.

        Args:
            request: URL string/`Expr` or request dict mapping evaluated for `fetch(**request)`.
            schema: Sub-schema extracted from the fetched page.
            meta: Optional dict/`Expr` passed as `meta` (`T.meta(...)`) into the sub-schema.
            format: Optional target response format (`"html"`, `"json"`, or `"xml"`) when crossing document formats.

        Returns:
            New `Expr` representing the linked sub-query.

        Raises:
            TypeError: If `request` or `schema` is `None`.
            ValueError: If `format` is provided and not `"html"`, `"json"`, or `"xml"`.

        Examples:
            >>> from oxide_parse import T, compile, follow
            >>> pages = {"https://ex.test/p1": "<span class='price'>42</span>"}
            >>> schema = follow(T.css("a").attr("href").url(), {"price": T.css(".price").text().int()})
            >>> compile(schema).extract("<a href='/p1'>go</a>", meta={"base_url": "https://ex.test"}, fetch=lambda url, **_: pages[url])
            {'price': 42}
        """
        if request is None or schema is None:
            raise TypeError("follow() needs `request` and `schema`")
        if format not in (None, "html", "json", "xml"):
            raise ValueError('follow(): format must be "html", "json", or "xml"')
        spec: Dict[str, Any] = {"request": to_ir(request), "schema": to_ir(schema)}
        if meta is not None:
            spec["meta"] = to_ir(meta)
        if format is not None:
            spec["format"] = format
        return self._then({"follow": spec})

    def paginate(
        self,
        items: Any,
        next: Any,
        *,
        max_pages: Optional[builtins.int] = None,
        max_items: Optional[builtins.int] = None,
        meta: Any = None,
        unique_by: Any = None,
        keep: Literal["first", "last"] = "first",
    ) -> "Expr":
        """Collect `items` in order across pages linked by `next` requests (`fetch(**next)`).

        Evaluates `items` on the current page and each fetched page, stopping *before* issuing the next fetch when:
        `max_items` items have been collected, `max_pages` pages (initial page included) have been visited, `next`
        yields `None` / empty string / `"#"` / `javascript:...` / `"url": None`, or a repeated canonical request
        (cycle) is detected. When `unique_by` is set, items are deduplicated across pages before applying `max_items`.

        Args:
            items: Expression yielding a list (or `None` treated as `[]`) per page.
            next: URL expression or request dict expression for `fetch(**next)` targeting the next page.
            max_pages: Optional maximum total pages to visit (`>= 1`). Without it, a safety limit of 10,000 applies.
            max_items: Optional maximum items to collect (`>= 1`); excess items on the final page are truncated.
            meta: Optional dict/`Expr` merged over parent `meta` on subsequent pages.
            unique_by: Optional key expression evaluated on each collected item to deduplicate across pages.
            keep: Duplicate handling when `unique_by` is set: `"first"` (default) or `"last"`.

        Returns:
            New `Expr` accumulating the flattened list across pages.

        Raises:
            TypeError: If `items` or `next` is `None`.
            ValueError: If `max_pages`/`max_items` is invalid or `keep` is not `"first"` or `"last"`.

        Examples:
            >>> from oxide_parse import T, compile, paginate
            >>> pages = {
            ...     "https://ex.test/1": "<li>A</li><li>B</li><a class='next' href='?p=2'>next</a>",
            ...     "https://ex.test/1?p=2": "<li>C</li><li>D</li><a class='next' href='?p=3'>next</a>",
            ... }
            >>> schema = paginate(T.css("li").map(T.text()), T.css("a.next").attr("href").url(), max_items=3)
            >>> compile(schema).extract(pages["https://ex.test/1"], meta={"base_url": "https://ex.test/1"}, fetch=lambda url, **_: pages[url])
            ['A', 'B', 'C']
        """
        if items is None or next is None:
            raise TypeError("paginate() needs `items` and `next`")
        if keep not in ("first", "last"):
            raise ValueError('paginate(): keep must be "first" or "last"')
        spec: Dict[str, Any] = {"items": to_ir(items), "next": to_ir(next)}
        if meta is not None:
            spec["meta"] = to_ir(meta)
        if max_pages is not None:
            if isinstance(max_pages, builtins.bool) or not isinstance(max_pages, builtins.int) or max_pages < 1:
                raise ValueError("max_pages must be a positive integer (>= 1)")
            spec["max_pages"] = max_pages
        if max_items is not None:
            if isinstance(max_items, builtins.bool) or not isinstance(max_items, builtins.int) or max_items < 1:
                raise ValueError("max_items must be a positive integer (>= 1)")
            spec["max_items"] = max_items
        if unique_by is not None:
            spec["unique_by"] = to_ir(unique_by)
        if keep != "first":
            spec["keep"] = keep
        return self._then({"paginate": spec})

    # -- operators (sugar) -------------------------------------------------
    def __eq__(self, other: Any) -> "Expr":  # type: ignore[override]
        return self.eq(other)

    def __ne__(self, other: Any) -> "Expr":  # type: ignore[override]
        return self.ne(other)

    def __and__(self, other: Any) -> "Expr":
        return self.and_(other)

    def __or__(self, other: Any) -> "Expr":
        return self.or_(other)

    def __invert__(self) -> "Expr":
        return self.not_()

    def __hash__(self) -> builtins.int:
        return id(self)

    def __bool__(self) -> builtins.bool:
        raise TypeError("an Expr has no truth value; use .exists()/.bool() inside the schema, and `&`/`|`/`~` instead of and/or/not")


def _xpath_literal(s: str) -> str:
    """Quote `s` as an XPath 1.0 string literal."""
    if "'" not in s:
        return f"'{s}'"
    if '"' not in s:
        return f'"{s}"'
    parts = s.split("'")
    return "concat(" + ", \"'\", ".join(f"'{x}'" for x in parts) + ")"


def to_ir(x: Any) -> dict:
    """Convert a Python DSL schema (`Expr`, `dict`, `list`, `tuple`, or scalar literal) into IR.

    Args:
        x: Schema fragment built from `T`, plain containers, and JSON-compatible literals.

    Returns:
        IR dictionary ready for JSON/YAML serialization, `validate_ir()`, `decompile()`, or `Plan.from_ir()`.

    Raises:
        TypeError: If `x` contains an unsupported object type.

    Examples:
        >>> from oxide_parse import T, to_ir
        >>> to_ir({"title": T.css("h1").text()})
        {'object': {'title': {'pipe': [{'css': 'h1'}, 'text']}}}
    """
    if isinstance(x, Expr):
        return {"pipe": x._steps()}
    if isinstance(x, dict):
        return {"object": {str(k): to_ir(v) for k, v in x.items()}}
    if isinstance(x, (list, tuple)):
        return {"list": [to_ir(v) for v in x]}
    if x is None or isinstance(x, (bool, int, float, str)):
        return {"lit": x}
    raise TypeError(f"unsupported schema element of type {type(x).__name__}")


#: Root pipeline builder representing the current node (document root or active `.map()` item).
T = Expr()


def if_(cond: Any, then: Any, otherwise: Any = None) -> Expr:
    """Create a schema conditional expression evaluated against the current context.

    Args:
        cond: Condition `Expr` or value.
        then: Result expression when `cond` evaluates truthy.
        otherwise: Result expression otherwise (defaults to `None`).

    Returns:
        Conditional `Expr` (`T.if_(...)`).

    Examples:
        >>> from oxide_parse import T, compile, if_
        >>> compile(if_(T.css(".vip").exists(), "vip", "std")).extract("<i class='vip'/>")
        'vip'
    """
    return T.if_(cond, then, otherwise)


def coalesce(*alternatives: Any) -> Expr:
    """Return the first non-missing (`None` / empty node set) expression from `alternatives`.

    Args:
        *alternatives: Literal values or `Expr` chains evaluated left-to-right against the current context.

    Returns:
        `Expr` yielding the first present alternative, or `None`.

    Examples:
        >>> from oxide_parse import T, coalesce, compile
        >>> compile(coalesce(T.css(".sale").text(), T.css(".price").text())).extract("<span class='price'>$10</span>")
        '$10'
    """
    return T.coalesce(*alternatives)


def follow(
    request: Any,
    schema: Any,
    meta: Any = None,
    *,
    format: Optional[str] = None,
) -> Expr:
    """Emit a sub-query request (`fetch(**request)`) and apply `schema` to the fetched document.

    Top-level shortcut for `T.follow(request, schema, meta, format=format)`.

    Args:
        request: URL string/`Expr` or request dict expression for `fetch(**request)`.
        schema: Extraction schema run against the fetched document.
        meta: Optional dict/`Expr` passed into the sub-schema as `meta`.
        format: Optional response format switch (`"html"`, `"json"`, or `"xml"`).

    Returns:
        Sub-query `Expr`.

    Examples:
        >>> from oxide_parse import T, compile, follow
        >>> pages = {"https://ex.test/2": "<b>Hi</b>"}
        >>> compile(follow(T.css("a").attr("href").url(), T.css("b").text())).extract("<a href='/2'>2</a>", meta={"base_url": "https://ex.test"}, fetch=lambda url, **_: pages[url])
        'Hi'
    """
    return T.follow(request, schema, meta, format=format)


def paginate(
    items: Any,
    next: Any,
    *,
    max_pages: Optional[builtins.int] = None,
    max_items: Optional[builtins.int] = None,
    meta: Any = None,
    unique_by: Any = None,
    keep: Literal["first", "last"] = "first",
) -> Expr:
    """Collect `items` in order across pages linked by `next` requests (`fetch(**next)`).

    Top-level shortcut for `T.paginate(...)`.

    Args:
        items: Expression producing a list (or `None` treated as `[]`) per page.
        next: URL string/`Expr` or request dict expression pointing to the next page.
        max_pages: Optional limit on total pages visited, including the starting page (`>= 1`).
        max_items: Optional limit on total items returned (`>= 1`).
        meta: Optional dict/`Expr` merged with parent `meta` for subsequent pages.
        unique_by: Optional key expression evaluated per collected item to deduplicate across pages.
        keep: Duplicate policy when `unique_by` is set (`"first"` or `"last"`).

    Returns:
        Pagination `Expr` yielding a combined list.

    Examples:
        >>> from oxide_parse import T, compile, paginate
        >>> pages = {"https://ex.test/p1": "<b>1</b><a href='/p2'>n</a>", "https://ex.test/p2": "<b>2</b>"}
        >>> compile(paginate(T.css("b").map(T.text()), T.css("a").attr("href").url())).extract(pages["https://ex.test/p1"], meta={"base_url": "https://ex.test/p1"}, fetch=lambda url, **_: pages[url])
        ['1', '2']
    """
    return T.paginate(items, next, max_pages=max_pages, max_items=max_items, meta=meta, unique_by=unique_by, keep=keep)
