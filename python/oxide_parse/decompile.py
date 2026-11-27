"""IR to Python DSL source code converter (the inverse of `to_ir`).

Examples:
    >>> from oxide_parse import T, decompile, to_ir
    >>> print(decompile(to_ir({"title": T.css("h1").text().strip()})).strip())
    from oxide_parse import T
    <BLANKLINE>
    schema = {
        'title': T.css('h1').text().strip(),
    }
"""
from __future__ import annotations

from typing import Any

IND = 4
_RENAME = {"if": "if_", "not": "not_", "and": "and_", "or": "or_", "starts_with": "startswith", "ends_with": "endswith"}
_STR_ARG = {"css", "xpath", "attr", "has_attr", "meta", "key", "at", "split", "join"}
# Operations without arguments (whitelisted because output is executable Python source)
_UNIT_OPS = {
    "text", "own_texts", "main_content", "html", "inner_html", "parent", "children", "root", "index", "first", "last", "count", "exists",
    "required", "non_empty", "strip", "lower", "upper", "int", "float", "bool", "parse_json", "not",
}
_EXPR_ARG = {"map", "flat_map", "filter", "default", "eq", "ne", "contains", "starts_with", "ends_with", "and", "or", "parse_html"}


def expr_to_code(e: dict, lvl: int = 0) -> str:
    """Format a single IR expression node as a Python DSL expression string.

    Args:
        e: IR node dictionary (`{"pipe": ...}`, `{"object": ...}`, `{"list": ...}`, or `{"lit": ...}`).
        lvl: Current indentation nesting level (used internally for multi-line objects/lists).

    Returns:
        Python expression source code representing `e`.

    Raises:
        ValueError: If an unknown IR node kind or operation name is encountered.

    Examples:
        >>> from oxide_parse.decompile import expr_to_code
        >>> expr_to_code({"pipe": [{"css": "h1"}, "text", "strip"]})
        "T.css('h1').text().strip()"
    """
    ((kind, val),) = e.items()
    pad = " " * (IND * (lvl + 1))
    end = " " * (IND * lvl)
    if kind == "lit":
        return repr(val)
    if kind == "object":
        if not val:
            return "{}"
        body = ",\n".join(f"{pad}{k!r}: {expr_to_code(v, lvl + 1)}" for k, v in val.items())
        return "{\n" + body + ",\n" + end + "}"
    if kind == "list":
        if not val:
            return "[]"
        body = ",\n".join(f"{pad}{expr_to_code(v, lvl + 1)}" for v in val)
        return "[\n" + body + ",\n" + end + "]"
    if kind == "pipe":
        return "T" + "".join(_op(o, lvl) for o in val)
    raise ValueError(f"unknown IR node {kind!r}")


def _op(o: Any, lvl: int) -> str:
    if isinstance(o, str):
        if o not in _UNIT_OPS:
            raise ValueError(f"unknown operation {o!r}")
        return f".{_RENAME.get(o, o)}()"
    ((name, arg),) = o.items()
    m = _RENAME.get(name, name)
    if name in _STR_ARG:
        return f".{m}({arg!r})"
    if name in _EXPR_ARG:
        return f".{m}({expr_to_code(arg, lvl)})"
    if name == "text_opts":
        if arg.get("deep") is False:
            parts = []
            if arg.get("sep"):
                parts.append(repr(arg["sep"]))
            if arg.get("strip"):
                parts.append("strip=True")
            if arg.get("br") is not None:
                parts.append(f"br={arg['br']!r}")
            return ".own_text(" + ", ".join(parts) + ")"
        parts = []
        if arg.get("sep"):
            parts.append(repr(arg["sep"]))
        if arg.get("strip"):
            parts.append("strip=True")
        if arg.get("without") is not None:
            parts.append(f"without={arg['without']!r}")
        if arg.get("br") is not None:
            parts.append(f"br={arg['br']!r}")
        return ".text(" + ", ".join(parts) + ")"
    if name == "table":
        parts = []
        if arg.get("mode") is not None and arg.get("mode") != "records":
            parts.append(f"mode={arg['mode']!r}")
        if arg.get("strip") is False:
            parts.append("strip=False")
        if arg.get("header_sep") is not None and arg.get("header_sep") != " / ":
            parts.append(f"header_sep={arg['header_sep']!r}")
        return ".table(" + ", ".join(parts) + ")"
    if name == "unique":
        parts = []
        if arg.get("by") is not None:
            parts.append(expr_to_code(arg["by"], lvl))
        if arg.get("keep") is not None and arg.get("keep") != "first":
            parts.append(f"keep={arg['keep']!r}")
        return ".unique(" + ", ".join(parts) + ")"
    if name == "parse_srcset":
        parts = []
        if arg.get("pick") is not None:
            parts.append(f"pick={arg['pick']!r}")
        if arg.get("min_width") is not None:
            parts.append(f"min_width={arg['min_width']!r}")
        if arg.get("min_density") is not None:
            parts.append(f"min_density={arg['min_density']!r}")
        return ".parse_srcset(" + ", ".join(parts) + ")"
    if name == "price":
        parts = []
        if arg.get("decimal") is not None:
            parts.append(f"decimal={arg['decimal']!r}")
        if arg.get("currency") is not None:
            parts.append(f"currency={arg['currency']!r}")
        return ".price(" + ", ".join(parts) + ")"
    if name == "markdown":
        return ".markdown(" + (f"without={arg['without']!r}" if arg.get("without") is not None else "") + ")"
    if name == "map_opts":
        return f".map({expr_to_code(arg['body'], lvl)}, on_error={arg.get('on_error', 'raise')!r})"
    if name == "resolve_url":
        return f".url({expr_to_code(arg['base'], lvl)})" if arg.get("base") is not None else ".url()"
    if name == "js_json":
        return f".js_json({arg['var']!r})" if arg.get("var") is not None else ".js_json()"
    if name == "sections":
        extra = f", items={arg['items']!r}" if arg.get("items") is not None else ""
        return f".sections({arg['header']!r}{extra})"
    if name == "distill":
        return f".distill({arg.get('keep', 2)}, {arg.get('max_text', 80)}, {arg.get('max_chars', 20000)})"
    if name == "slice":
        a, b = arg.get("start"), arg.get("stop")
        if b is None and a == 1:
            return ".rest()"
        return f".slice({a!r}, {b!r})"
    if name == "group_by":
        extra = f", items={arg['items']!r}" if arg.get("items") is not None else ""
        return f".group_by({arg['header']!r}{extra})"
    if name == "json_ld":
        parts = []
        if arg.get("schema_type") is not None:
            parts.append(repr(arg["schema_type"]))
        if arg.get("resolve_refs"):
            parts.append("resolve_refs=True")
        return ".json_ld(" + ", ".join(parts) + ")"
    if name == "microdata":
        return f".microdata({arg['schema_type']!r})" if arg.get("schema_type") is not None else ".microdata()"
    if name == "next_flight":
        return f".next_flight({arg['key']!r})" if arg.get("key") is not None else ".next_flight()"
    if name == "meta_tags":
        extra = f", values={arg['values']!r}" if arg.get("values") else ""
        return f".meta_tags({arg.get('prefixes', [])!r}{extra})"
    if name == "adaptive":
        extra = f", threshold={arg['threshold']!r}" if arg.get("threshold") is not None else ""
        return f".css({arg['css']!r}, adaptive={arg['key']!r}{extra})"
    if name == "replace":
        return f".replace({arg['from']!r}, {arg['to']!r})"
    if name in ("regex", "regex_all"):
        g = arg.get("group", 0)
        return f".{name}({arg['pattern']!r})" if g == 0 else f".{name}({arg['pattern']!r}, {g})"
    if name == "coalesce":
        return ".coalesce(" + ", ".join(expr_to_code(a, lvl) for a in arg) + ")"
    if name == "if":
        return f".if_({expr_to_code(arg['cond'], lvl)}, {expr_to_code(arg['then'], lvl)}, {expr_to_code(arg['otherwise'], lvl)})"
    if name == "link_header":
        rel = arg.get("rel")
        return ".link_header()" if rel == "next" else f".link_header({rel!r})"
    if name == "follow":
        parts = [f"request={expr_to_code(arg['request'], lvl)}", f"schema={expr_to_code(arg['schema'], lvl)}"]
        if arg.get("meta") is not None:
            parts.append(f"meta={expr_to_code(arg['meta'], lvl)}")
        if arg.get("format") is not None:
            parts.append(f"format={arg['format']!r}")
        return ".follow(" + ", ".join(parts) + ")"
    if name == "paginate":
        parts = [f"items={expr_to_code(arg['items'], lvl)}", f"next={expr_to_code(arg['next'], lvl)}"]
        if arg.get("max_pages") is not None:
            parts.append(f"max_pages={arg['max_pages']!r}")
        if arg.get("max_items") is not None:
            parts.append(f"max_items={arg['max_items']!r}")
        if arg.get("meta") is not None:
            parts.append(f"meta={expr_to_code(arg['meta'], lvl)}")
        if arg.get("unique_by") is not None:
            parts.append(f"unique_by={expr_to_code(arg['unique_by'], lvl)}")
        if arg.get("keep") is not None and arg.get("keep") != "first":
            parts.append(f"keep={arg['keep']!r}")
        return ".paginate(" + ", ".join(parts) + ")"
    raise ValueError(f"unknown op {name!r}")


def decompile(ir: dict, name: str = "schema") -> str:
    """Return runnable Python source code that rebuilds `ir` using the `T` DSL.

    Args:
        ir: Serialized schema dictionary produced by `to_ir()` or `Plan.ir`.
        name: Variable name bound to the schema in generated code.

    Returns:
        Python module snippet defining `name` with `from oxide_parse import T`.

    Raises:
        ValueError: If `name` is not a valid Python identifier or `ir` contains unknown ops.

    Examples:
        >>> from oxide_parse import T, decompile, to_ir
        >>> code = decompile(to_ir({"n": T.css("b").text().int()}), name="my_schema")
        >>> "my_schema = " in code
        True
    """
    if not name.isidentifier():
        raise ValueError(f"target variable name must be a valid Python identifier, got {name!r}")
    return f"from oxide_parse import T\n\n{name} = {expr_to_code(ir)}\n"
