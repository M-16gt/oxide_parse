"""System prompts for LLMs generating or migrating `oxide_parse` schemas.

`oxide_parse` never calls language models directly. Print these prompts via `oxide-parse prompt ...` or pass them to
your own client.

Examples:
    >>> from oxide_parse import prompts
    >>> "bs4" in prompts.sources()
    True
    >>> "schema = ..." in prompts.generate_prompt()
    True
"""
from __future__ import annotations

from typing import Dict, List, Literal, Optional

from . import ir_spec

_COMMON = """\
You write extraction schemas for the Python library `oxide_parse`. A schema is a Python value built from `T`
(`from oxide_parse import T`): a dict of named fields, a list, or a T-expression. T-expressions are chains that start at
the current node (the document at the top level, the record inside `.map(...)`).

SELECT
  T.css("a.item")            all matches (CSS)         T.xpath("//h1")           XPath 1.0
  .first()  .last()  [n]  [a:b]  .rest()               position / slice (rest = all but the first)
  .parent()  .children()  .root()
  .css(...)  chained: searches below the current nodes; `:scope > li` is the current element itself
EXTRACT
  .text()  .own_text()  .own_texts()  .html()  .inner_html()  .attr("href")  .has_attr("x")  .meta("key")  .count()  .exists()
  .text(" ", strip=True, without=".ad, script", br="\\n")   joined text pieces, skipped subtrees, <br> as newline
  .table("records" | "grid" | "split")  .main_content().markdown(without=".ad")  .distill(max_chars=8000)
  T.json_ld("Product", resolve_refs=True)  T.microdata("Product")  T.next_flight("key")  T.meta_tags()
CLEAN AND CONVERT
  .strip() .lower() .upper() .replace("a", "b") .regex(r"(\\d+)", 1) .regex_all(...) .split(",") .join(", ")
  .int() .float() .price() .parse_srcset("max") .bool() .parse_json()["a"]["b"]  .parse_html({...})  .js_json()  .url()
  .link_header("next")                                      extract RFC 8288 Link header URL from T.meta("headers")["link"]
STRUCTURE
  {"title": T.css("h1").text(), "tags": T.css(".tag").map(T.text()).unique()}
  T.css(".card").map({"name": T.css("h2").text(), "price": T.css(".price").text().float()}, on_error="skip")
  T.css(".specs").group_by("h3").map({"title": T.first().text(), "items": T.rest().map(T.text().strip())})   flat markup
  T.coalesce(a, b)  .default("N/A")  .required()  .non_empty()  T.if_(cond, then, otherwise)  .eq() .ne() .contains()
  T.css("a.next").follow(T.attr("href"), {...}, format="json")   fetch another page ("html" | "json" | "xml")
  T.paginate(items=T.css(".item").map({...}), next=T.css("a.next").attr("href").url(), unique_by=T["sku"])   dedup & paginate
  T.css(".price", adaptive="price")   a selector that heals itself after a redesign (use for fragile selectors)

RULES
  * Missing things are `None`, never an exception: a chain on nothing stays nothing. Use `.default(...)` for fallbacks.
  * `.text()` of a node list reads the first node. For every match use `.map(...)`.
  * Prefer short, stable CSS selectors (ids, semantic classes, data-* attributes) over long positional paths.
  * Put values that repeat inside records: `T.css(".card").map({...})`, selectors inside are relative to the card.
  * Numbers: `.text().float()` understands "$1,299.50" and "1 299,50 ₽".
  * Answer with ONE Python code block that defines `schema = ...` and nothing else.
"""

_OPS = "Operations (IR names): " + ", ".join(ir_spec.all_op_names()) + "."

_GENERATE_TAIL = """\

TASK
You get a description of the data to extract and an HTML sample (possibly distilled with `oxide-parse distill`).
Write `schema` so that it extracts exactly that data from pages built like the sample. Check every selector against the
sample; do not invent classes that are not there. After writing the code, `oxide-parse validate schema.py` and
`oxide-parse run schema.py page.html` show problems and the result.
"""

_MIGRATE_TAIL = """\

TASK
You get existing extraction code@@SRC@@. Translate it into an `oxide_parse` `schema` that returns the same data in the same
shape. Keep the semantics: first-match vs all matches, stripping, defaults, type conversions. Replace loops that build
records with `.map({...})`. Mention in a comment above the code anything you could not translate.

@@MAPPING@@
"""

_MAPPINGS: Dict[str, str] = {
    "bs4": """\
BeautifulSoup -> oxide_parse
  soup.select_one("h1").get_text(strip=True)       T.css("h1").text().strip()
  [a["href"] for a in soup.select("a")]            T.css("a").map(T.attr("href"))
  soup.select_one("a")["href"]                     T.css("a").attr("href")
  for card in soup.select(".card"): {...}          T.css(".card").map({...})
  card.select_one(".price").text                   T.css(".price").text()            (inside the map)
  float(re.sub(r"[^\\d.]", "", s))                  .float()  or  .regex(r"([\\d.]+)", 1).float()
  x.find("h2", class_="t")                         T.css("h2.t")
  x.find_next_sibling("p")                         T.xpath("following-sibling::p[1]")
  x.get("data-id")                                 T.attr("data-id")
  if el else None                                  nothing to write: a missing element is already None""",
    "crawl4ai": """\
Crawl4AI schemas are converted deterministically: `oxide-parse migrate --from crawl4ai strategy.json --format python`.
Use that output; only write by hand what it reports as unsupported (`computed` fields: do them in Python afterwards).""",
    "scrapy": """\
Scrapy / Parsel -> oxide_parse
  response.css("h1::text").get()                   T.css("h1").text().strip()
  response.css("a::attr(href)").getall()           T.css("a").map(T.attr("href"))
  response.xpath("//h1/text()").get()              T.xpath("//h1/text()").at(0)       (xpath strings come as a list)
  for row in response.css(".row"): yield {...}     T.css(".row").map({...})
  response.urljoin(href)                           .url()
  response.follow(href, callback=...)              T.css("a").follow(T.attr("href"), {...})""",
    "selectolax": """\
selectolax -> oxide_parse
  tree.css_first("h1").text(strip=True)            T.css("h1").text().strip()
  [n.attributes["href"] for n in tree.css("a")]    T.css("a").map(T.attr("href"))
  node.css_first(".p").text()                      T.css(".p").text()                  (inside .map)""",
    "lxml": """\
lxml -> oxide_parse
  tree.xpath("//h1/text()")[0]                     T.xpath("//h1/text()").at(0)
  tree.cssselect("a")                              T.css("a")
  el.get("href")                                   T.attr("href")
  el.text_content().strip()                        T.text().strip()""",
}


def sources() -> List[str]:
    """Return sorted source library names supported by `migrate_prompt()`.

    Returns:
        Sorted list of valid `source` identifiers.

    Examples:
        >>> from oxide_parse import prompts
        >>> prompts.sources()
        ['bs4', 'crawl4ai', 'lxml', 'scrapy', 'selectolax']
    """
    return sorted(_MAPPINGS)


def generate_prompt() -> str:
    """Return a system prompt instructing an LLM how to write an `oxide_parse` schema.

    Returns:
        Prompt text covering `T` syntax, rules, and every IR operation name.

    Examples:
        >>> from oxide_parse import prompts
        >>> p = prompts.generate_prompt()
        >>> "T.css(" in p and "schema = ..." in p
        True
    """
    return _COMMON + "\n" + _OPS + "\n" + _GENERATE_TAIL


def migrate_prompt(source: str = "bs4") -> str:
    """Return a system prompt for translating scraper code from another library.

    Args:
        source: Source ecosystem identifier (`"bs4"`, `"crawl4ai"`, `"scrapy"`, `"selectolax"`, or `"lxml"`).

    Returns:
        System prompt tailored with migration mappings for `source`.

    Raises:
        ValueError: If `source` is not in `sources()`.

    Examples:
        >>> from oxide_parse import prompts
        >>> "BeautifulSoup" in prompts.migrate_prompt("bs4")
        True
    """
    if source not in _MAPPINGS:
        raise ValueError(f"unknown source {source!r}; choose from {', '.join(sources())}")
    label = "" if source == "crawl4ai" else f" written for {source}"
    tail = _MIGRATE_TAIL.replace("@@SRC@@", label).replace("@@MAPPING@@", _MAPPINGS[source])
    return _COMMON + "\n" + _OPS + "\n" + tail


_IR_GENERATE = """\
You generate `oxide_parse` JSON IR schemas directly from HTML samples. Output ONLY a single valid JSON object (no markdown fences, no commentary).

ROOT SHAPES
  Every IR expression is one of 4 JSON node kinds:
    {"object": {"field": <expr>, ...}}         top-level dict of fields (streamable when fields use css+text/attr)
    {"pipe": [<op>, ...]}                       sequential pipeline starting at the current node
    {"list": [<expr>, ...]}                     fixed JSON array of expressions
    {"lit": <json_value>}                       literal scalar/null value

SELECTOR AND EXTRACTION OPS
  {"css": ".card"}                              match elements via CSS (add "adaptive": {"css": "...", "key": "...", "threshold": 0.5} for fragile nodes)
  {"xpath": "//h1"}                             XPath 1.0 (use only when selector_kind="XPATH" or CSS cannot express it)
  "first" | "last" | {"at": 0}                  pick one item
  {"slice": {"start": 1, "stop": null}}         slice list or node set
  "text" | {"text_opts": {"sep": " ", "strip": true, "without": ".ad", "br": "\\n", "deep": true}}
  "own_texts" | {"table": {"mode": "records", "strip": true}} | {"attr": "href"} | {"has_attr": "disabled"}
  "html" | "inner_html" | {"markdown": {"without": ".ad"}} | "main_content"
  {"map": <expr>} | {"map_opts": {"body": <expr>, "on_error": "skip"}}
  {"filter": <pred_expr>} | {"unique": {"by": <expr>, "keep": "first"}}
  {"default": <expr>} | "non_empty" | "required" | {"coalesce": [<expr>, ...]}
  "strip" | "lower" | "upper" | {"replace": {"from": "a", "to": "b"}} | {"split": ","} | {"join": ", "}
  {"regex": {"pattern": "([0-9.]+)", "group": 1}} | "int" | "float" | "bool"
  {"price": {"decimal": "."}} | {"parse_srcset": {"pick": "max"}} | {"resolve_url": {}}
  "parse_json" | {"json_ld": {"schema_type": "Product", "resolve_refs": true}} | {"microdata": {"schema_type": "Product"}} | {"next_flight": {}}

CANONICAL EXAMPLE (repeating cards)
  {
    "pipe": [
      {"css": "article.product"},
      {"map": {
        "object": {
          "title": {"pipe": [{"css": "h3 a"}, {"attr": "title"}]},
          "price": {"pipe": [{"css": ".price"}, "text", "strip", "float"]},
          "href": {"pipe": [{"css": "h3 a"}, {"attr": "href"}, {"resolve_url": {}}]}
        }
      }}
    ]
  }

RULES
  * Every selector MUST match elements actually present in the HTML sample; do not invent class names.
  * "text", {"attr": "..."}, and "html" read the FIRST matched node. Inside {"map": {"object": {...}}}, selectors are relative to each card.
  * Missing elements evaluate to null automatically (no exceptions); use {"default": ...} or {"coalesce": [...]} for fallbacks.
  * Omit dynamic/generated classes; prefer stable semantic tags, attributes, and short selectors.
"""

_CRAWL4AI_STRICT = """\
You generate a valid Crawl4AI JSON extraction schema compatible with `oxide_parse.migrate.from_crawl4ai()`.
Output ONLY a JSON object with keys: `name` (str), `baseSelector` (str), optional `baseFields` (list), and `fields` (list).
Supported field `type` values ONLY: `"text"`, `"attribute"` (requires `"attribute"`), `"html"`, `"regex"` (requires `"pattern"`, optional `"group"`), `"nested"` (requires `"fields"`), `"list"`, `"nested_list"`.
Optional supported keys per field: `"name"`, `"selector"`, `"default"`, `"transform"` (`"lowercase"`, `"uppercase"`, `"strip"` ONLY).
FORBIDDEN features (never emit): `"computed"`, `"source"`, custom Python transforms, or volatile hash classes.
"""


def ir_generate_prompt(*, kind: str = "css") -> str:
    """Return a system prompt instructing an LLM to generate `oxide_parse` JSON IR directly.

    Args:
        kind: Preferred selector style (`"css"` or `"xpath"`).

    Returns:
        Prompt string containing IR structure grammar, canonical examples, and all supported op names.

    Raises:
        ValueError: If `kind` is not `"css"` or `"xpath"`.

    Examples:
        >>> from oxide_parse import prompts
        >>> p = prompts.ir_generate_prompt(kind="css")
        >>> '"pipe"' in p and '"object"' in p
        True
    """
    norm = kind.strip().lower()
    if norm not in ("css", "xpath"):
        raise ValueError('ir_generate_prompt(): kind must be "css" or "xpath"')
    pref = (
        f"Preferred selector mode: {norm.upper()} "
        + ('(use {"css": "..."}; reserve {"xpath": "..."} only if CSS cannot express it).' if norm == "css" else '(use {"xpath": "..."}).')
    )
    return _IR_GENERATE + "\n" + pref + "\n" + _OPS


def crawl4ai_schema_prompt(*, schema_type: str = "CSS", output: Literal["ir", "crawl4ai"] = "ir") -> str:
    """Return system instructions suitable for dropping into Crawl4AI's schema prompt templates.

    Args:
        schema_type: Target selector family (`"CSS"` or `"XPATH"`).
        output: `"ir"` (default) requests native `oxide_parse` JSON IR directly; `"crawl4ai"` requests a strict
            Crawl4AI schema restricted to deterministically migratable types.

    Returns:
        System prompt text.

    Raises:
        ValueError: If `schema_type` or `output` is invalid.

    Examples:
        >>> from oxide_parse import prompts
        >>> "oxide_parse" in prompts.crawl4ai_schema_prompt(output="ir")
        True
    """
    st = schema_type.strip().upper()
    if st not in ("CSS", "XPATH"):
        raise ValueError('schema_type must be "CSS" or "XPATH"')
    if output == "ir":
        return ir_generate_prompt(kind=st.lower())
    if output == "crawl4ai":
        return _CRAWL4AI_STRICT + f"\nSelector syntax mode: {st}."
    raise ValueError('output must be "ir" or "crawl4ai"')


def build_crawl4ai_schema_prompt(
    html: str,
    schema_type: str = "CSS",
    query: Optional[str] = None,
    target_json_example: Optional[str] = None,
    *,
    output: Literal["ir", "crawl4ai"] = "ir",
    distill_html: bool = False,
    max_chars: int = 40000,
) -> str:
    """Build a complete Crawl4AI-compatible prompt overriding schema generation to target `oxide_parse`.

    Matches Crawl4AI's `JsonElementExtractionStrategy._build_schema_prompt(html, schema_type, query, target_json_example)`
    signature so it can drop cleanly into Crawl4AI's `generate_schema` / LLM refinement loop.

    Args:
        html: Sample HTML document (or snippet) to inspect.
        schema_type: Selector mode (`"CSS"` or `"XPATH"`).
        query: Optional natural-language description of fields to extract.
        target_json_example: Optional JSON string demonstrating desired output shape.
        output: `"ir"` (default) instructs the LLM to output validated `oxide_parse` IR directly; `"crawl4ai"`
            outputs a constrained Crawl4AI schema guaranteed to migrate cleanly via `migrate.from_crawl4ai()`.
        distill_html: Shrink large HTML into a structural skeleton via `oxide_parse.distill()` before embedding.
        max_chars: Character budget applied to the embedded HTML sample (default `40000`).

    Returns:
        Combined prompt string ready to send to an LLM (e.g., Gemini, OpenAI, Anthropic, LiteLLM).

    Examples:
        >>> from oxide_parse import prompts
        >>> p = prompts.build_crawl4ai_schema_prompt("<h1>Book</h1>", query="Extract title", output="ir")
        >>> "Extract title" in p and '"pipe"' in p
        True
    """
    system_part = crawl4ai_schema_prompt(schema_type=schema_type, output=output)
    snippet = html
    if distill_html and html:
        try:
            from .plan import distill

            snippet = distill(html, max_chars=max_chars)
        except Exception:
            snippet = html[:max_chars]
    elif max_chars > 0 and len(snippet) > max_chars:
        snippet = snippet[:max_chars]

    parts = [system_part, f"HTML to analyze:\n```html\n{snippet}\n```"]
    if query:
        parts.append(f"## Target extraction goal\n{query}")
    if target_json_example:
        parts.append(f"## Target JSON output example\n```json\n{target_json_example}\n```")
    tail = (
        "Return ONLY valid oxide_parse JSON IR matching the selectors present in the HTML, nothing else."
        if output == "ir"
        else "Return ONLY valid JSON following the Crawl4AI schema contract above, nothing else."
    )
    parts.append(tail)
    return "\n\n".join(parts)
