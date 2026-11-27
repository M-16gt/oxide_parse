"""Extensible input/output serialization codec factory for `oxide_parse` schemas and results.

Ships with built-in `"json"`, `"yaml"`, `"toml"`, and `"python"` codecs via the global `FORMATS` registry, and
lets callers register custom formats (by canonical name, aliases, and file extensions) at runtime.

Examples:
    >>> from oxide_parse import FORMATS, T, compile
    >>> plan = compile({"h1": T.css("h1").text().strip()})
    >>> raw = plan.dumps("toml")
    >>> Plan_ir = FORMATS.loads(raw, "toml")
    >>> Plan_ir == plan.ir
    True
"""
from __future__ import annotations

import importlib
import json
import math
import os
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Sequence, Tuple, Union

_TOML_NULL_KEY = "__oxide_null__"
_TOML_ROOT_KEY = "__oxide_root__"
_TOML_LITERAL_KEY = "__oxide_literal__"
_TOML_RESERVED_KEYS = {_TOML_NULL_KEY, _TOML_ROOT_KEY, _TOML_LITERAL_KEY}
_IR_ROOT_KEYS = frozenset({"pipe", "object", "list", "lit"})
_BARE_TOML_KEY = re.compile(r"^[A-Za-z0-9_-]+$")


@dataclass(frozen=True)
class FormatCodec:
    """Descriptor holding serializer/deserializer callbacks and metadata for one format.

    Examples:
        >>> from oxide_parse.formats import get_format
        >>> codec = get_format("toml")
        >>> codec.name, codec.extensions
        ('toml', ('.toml',))
    """

    name: str
    """Canonical lowercase format identifier (e.g., `"json"`, `"yaml"`, `"toml"`, `"python"`)."""
    dumps: Optional[Callable[..., str]] = None
    """Optional serializer `(data, **options) -> str`."""
    loads: Optional[Callable[..., Any]] = None
    """Optional deserializer `(text, **options) -> Any`."""
    extensions: Tuple[str, ...] = ()
    """Normalized lowercase filename suffixes including leading dot (e.g., `('.toml',)`)."""
    aliases: Tuple[str, ...] = ()
    """Additional lowercase format names accepted when looking up this codec."""
    ir_only: bool = False
    """`True` when the format targets `oxide_parse` IR/DSL specifically rather than arbitrary values."""


def _normalize_ext(ext: str) -> str:
    s = ext.strip().lower()
    if not s:
        raise ValueError("file extension cannot be empty")
    return s if s.startswith(".") else f".{s}"


def _encode_toml_value(val: Any) -> Any:
    if val is None:
        return {_TOML_NULL_KEY: True}
    if isinstance(val, list):
        return [_encode_toml_value(item) for item in val]
    if isinstance(val, tuple):
        return [_encode_toml_value(item) for item in val]
    if isinstance(val, dict):
        mapping = {str(k): _encode_toml_value(v) for k, v in val.items()}
        if len(mapping) == 1 and mapping.keys() <= _TOML_RESERVED_KEYS:
            key, value = next(iter(mapping.items()))
            return {_TOML_LITERAL_KEY: [key, value]}
        return mapping
    return val


def _decode_toml_value(val: Any) -> Any:
    if isinstance(val, list):
        return [_decode_toml_value(item) for item in val]
    if isinstance(val, dict):
        if len(val) == 1 and val.get(_TOML_NULL_KEY) is True:
            return None
        literal = val.get(_TOML_LITERAL_KEY)
        if len(val) == 1 and isinstance(literal, list) and len(literal) == 2:
            key, value = literal
            if isinstance(key, str) and key in _TOML_RESERVED_KEYS:
                return {key: _decode_toml_value(value)}
        return {k: _decode_toml_value(v) for k, v in val.items()}
    return val


def _toml_key(key: str) -> str:
    if _BARE_TOML_KEY.fullmatch(key):
        return key
    return _toml_inline(key)


def _toml_inline(val: Any) -> str:
    if isinstance(val, bool):
        return "true" if val else "false"
    if isinstance(val, int):
        return str(val)
    if isinstance(val, float):
        if math.isnan(val):
            return "nan"
        if math.isinf(val):
            return "inf" if val > 0 else "-inf"
        return repr(val)
    if isinstance(val, str):
        return json.dumps(val, ensure_ascii=False).replace("\x7f", "\\u007f")
    if isinstance(val, (list, tuple)):
        return "[" + ", ".join(_toml_inline(item) for item in val) + "]"
    if isinstance(val, dict):
        parts = [f"{_toml_key(str(k))} = {_toml_inline(v)}" for k, v in val.items()]
        return "{" + ", ".join(parts) + "}"
    raise TypeError(f"unsupported TOML value type: {type(val).__name__}")


def _write_toml_table(
    mapping: Dict[str, Any],
    path: Tuple[str, ...],
    lines: List[str],
    *,
    emit_header: bool = True,
) -> None:
    scalars: List[Tuple[str, Any]] = []
    subtables: List[Tuple[str, Dict[str, Any]]] = []
    array_tables: List[Tuple[str, List[Dict[str, Any]]]] = []
    for raw_k, v in mapping.items():
        k = str(raw_k)
        if isinstance(v, dict):
            subtables.append((k, v))
        elif isinstance(v, list) and v and all(isinstance(item, dict) for item in v):
            array_tables.append((k, v))
        else:
            scalars.append((k, v))

    if path and emit_header:
        if lines and lines[-1] != "":
            lines.append("")
        lines.append("[" + ".".join(_toml_key(p) for p in path) + "]")
    for k, v in scalars:
        lines.append(f"{_toml_key(k)} = {_toml_inline(v)}")
    for k, sub in subtables:
        _write_toml_table(sub, (*path, k), lines)
    for k, items in array_tables:
        item_path = (*path, k)
        header = "[[" + ".".join(_toml_key(p) for p in item_path) + "]]"
        for item in items:
            if lines and lines[-1] != "":
                lines.append("")
            lines.append(header)
            _write_toml_table(item, item_path, lines, emit_header=False)


def _toml_dumps_fallback(mapping: Dict[str, Any]) -> str:
    lines: List[str] = []
    _write_toml_table(mapping, (), lines)
    return ("\n".join(lines) + "\n") if lines else ""


def _json_dumps(data: Any, *, indent: Optional[int] = None, ensure_ascii: bool = False, **kw: Any) -> str:
    return json.dumps(data, indent=indent, ensure_ascii=ensure_ascii, **kw)


def _json_loads(text: str, **kw: Any) -> Any:
    return json.loads(text, **kw)


def _yaml_dumps(data: Any, *, sort_keys: bool = False, allow_unicode: bool = True, **kw: Any) -> str:
    try:
        import yaml
    except ImportError as exc:  # pragma: no cover
        raise ImportError("YAML support requires PyYAML: pip install oxide-parse[yaml]") from exc
    return yaml.safe_dump(data, sort_keys=sort_keys, allow_unicode=allow_unicode, **kw)


def _yaml_loads(text: str, **kw: Any) -> Any:
    try:
        import yaml
    except ImportError as exc:  # pragma: no cover
        raise ImportError("YAML support requires PyYAML: pip install oxide-parse[yaml]") from exc
    return yaml.safe_load(text, **kw)


def _load_tomli_w() -> Any:
    try:
        return importlib.import_module("tomli_w")
    except ImportError:
        return None


def _toml_dumps(data: Any, **kw: Any) -> str:
    encoded = _encode_toml_value(data)
    root: Dict[str, Any] = encoded if isinstance(encoded, dict) else {_TOML_ROOT_KEY: encoded}
    tomli_w = _load_tomli_w()
    if tomli_w is not None:
        return str(tomli_w.dumps(root, **kw))
    return _toml_dumps_fallback(root)


def _toml_loads(text: str, **kw: Any) -> Any:
    reader: Any
    try:
        import tomllib

        reader = tomllib
    except ImportError:  # pragma: no cover - Python <3.11
        try:
            reader = importlib.import_module("tomli")
        except ImportError as exc:
            raise ImportError("TOML reading on Python <3.11 requires tomli: pip install oxide-parse[toml]") from exc
    raw = reader.loads(text, **kw)
    if isinstance(raw, dict) and set(raw.keys()) == {_TOML_ROOT_KEY}:
        return _decode_toml_value(raw[_TOML_ROOT_KEY])
    return _decode_toml_value(raw)


def _python_dumps(data: Any, *, name: str = "schema") -> str:
    from .decompile import decompile
    from .expr import to_ir

    is_ir = isinstance(data, dict) and len(data) == 1 and set(data.keys()) <= _IR_ROOT_KEYS
    ir = data if is_ir else to_ir(data)
    return decompile(ir, name=name)


def _python_loads(text: str, *, name: Optional[str] = None, filename: str = "<schema.py>") -> Any:
    from .expr import to_ir

    ns: Dict[str, Any] = {"__name__": "oxide_parse_schema", "__file__": filename}
    exec(compile(text, filename, "exec"), ns)
    candidates = (name,) if name else ("schema", "SCHEMA")
    for var in candidates:
        if var in ns:
            return to_ir(ns[var])
    expected = f"`{name}`" if name else "`schema`"
    raise ValueError(f"{filename} must define a variable {expected}")


class FormatRegistry:
    """Extensible registry mapping format names, aliases, and extensions to serializer/deserializer codecs.

    Examples:
        >>> from oxide_parse.formats import FormatRegistry
        >>> reg = FormatRegistry(with_defaults=False)
        >>> _ = reg.register("upper_json", dumps=lambda d: __import__("json").dumps(d).upper(), loads=lambda s: __import__("json").loads(s.lower()), extensions=(".ujson",))
        >>> reg.loads(reg.dumps({"k": "v"}, "upper_json"), "upper_json")
        {'k': 'v'}
    """

    def __init__(self, *, with_defaults: bool = True) -> None:
        """Initialize an empty or pre-populated codec registry.

        Args:
            with_defaults: When `True` (default), register built-in `"json"`, `"yaml"`, `"toml"`, and `"python"` codecs.

        Examples:
            >>> from oxide_parse.formats import FormatRegistry
            >>> "toml" in FormatRegistry().names()
            True
        """
        self._by_name: Dict[str, FormatCodec] = {}
        self._by_alias: Dict[str, str] = {}
        self._by_ext: Dict[str, str] = {}
        if with_defaults:
            self.register("json", dumps=_json_dumps, loads=_json_loads, extensions=(".json",))
            self.register("yaml", dumps=_yaml_dumps, loads=_yaml_loads, extensions=(".yaml", ".yml"), aliases=("yml",))
            self.register("toml", dumps=_toml_dumps, loads=_toml_loads, extensions=(".toml",))
            self.register(
                "python",
                dumps=_python_dumps,
                loads=_python_loads,
                extensions=(".py",),
                aliases=("py", "dsl"),
                ir_only=True,
            )

    def register(
        self,
        name: str,
        *,
        dumps: Optional[Callable[..., str]] = None,
        loads: Optional[Callable[..., Any]] = None,
        extensions: Sequence[str] = (),
        aliases: Sequence[str] = (),
        ir_only: bool = False,
        overwrite: bool = True,
    ) -> FormatCodec:
        """Register or replace a format codec in this registry.

        Args:
            name: Canonical format name (case-insensitive).
            dumps: Optional serializer callable returning `str`.
            loads: Optional deserializer callable returning parsed data.
            extensions: Filename extensions mapped to this format (with or without leading dot).
            aliases: Alternate format names mapped to `name`.
            ir_only: Mark format as IR/DSL-specific (`"python"`).
            overwrite: Replace an existing codec with the same canonical name (default `True`).

        Returns:
            Registered `FormatCodec` instance.

        Raises:
            ValueError: If the name or callbacks are invalid, an alias shadows a format name, or a collision occurs
                with `overwrite=False`.

        Examples:
            >>> from oxide_parse.formats import FormatRegistry
            >>> reg = FormatRegistry()
            >>> reg.register("ndjson", dumps=lambda d: __import__("json").dumps(d)).name
            'ndjson'
        """
        canon = name.strip().lower()
        if not canon:
            raise ValueError("format name cannot be empty")
        if dumps is None and loads is None:
            raise ValueError(f"format {canon!r} needs at least one of `dumps` or `loads`")
        if (dumps is not None and not callable(dumps)) or (loads is not None and not callable(loads)):
            raise ValueError(f"format {canon!r} requires callable codecs")

        norm_exts = tuple(dict.fromkeys(_normalize_ext(e) for e in extensions))
        norm_aliases = tuple(dict.fromkeys(a.strip().lower() for a in aliases if a.strip() and a.strip().lower() != canon))
        for alias in norm_aliases:
            if alias in self._by_name:
                raise ValueError(f"format alias {alias!r} shadows a registered format name")
        if not overwrite:
            for alias in (canon, *norm_aliases):
                if alias in self._by_alias:
                    raise ValueError(f"format name or alias {alias!r} is already registered")
            for ext in norm_exts:
                if ext in self._by_ext:
                    raise ValueError(f"format extension {ext!r} is already registered")
        if canon in self._by_name:
            self.unregister(canon)

        codec = FormatCodec(
            name=canon,
            dumps=dumps,
            loads=loads,
            extensions=norm_exts,
            aliases=norm_aliases,
            ir_only=ir_only,
        )
        self._by_name[canon] = codec
        self._by_alias[canon] = canon
        for alias in norm_aliases:
            self._by_alias[alias] = canon
        for ext in norm_exts:
            self._by_ext[ext] = canon
        return codec

    def unregister(self, name: str) -> None:
        """Remove a format codec and its aliases/extensions from the registry.

        Args:
            name: Canonical format name or alias to remove.

        Raises:
            KeyError: If `name` is not registered.

        Examples:
            >>> from oxide_parse.formats import FormatRegistry
            >>> reg = FormatRegistry()
            >>> _ = reg.register("tmpfmt", dumps=str)
            >>> reg.unregister("tmpfmt")
            >>> "tmpfmt" in reg.names()
            False
        """
        key = name.strip().lower()
        canon = self._by_alias.get(key)
        if canon is None or canon not in self._by_name:
            raise KeyError(f"unknown format {name!r}")
        self._by_name.pop(canon)
        for k in [a for a, target in self._by_alias.items() if target == canon]:
            del self._by_alias[k]
        for e in [ext for ext, target in self._by_ext.items() if target == canon]:
            del self._by_ext[e]

    def get(self, name: str) -> FormatCodec:
        """Look up a `FormatCodec` by canonical name, alias, or file suffix.

        Args:
            name: Format identifier (e.g., `"toml"`, `"yml"`, `".toml"`).

        Returns:
            Matching `FormatCodec`.

        Raises:
            ValueError: If no matching format is registered.

        Examples:
            >>> from oxide_parse.formats import FORMATS
            >>> FORMATS.get("yml").name
            'yaml'
        """
        key = name.strip().lower()
        canon = self._by_alias.get(key)
        if canon is None:
            ext = key if key.startswith(".") else f".{key}"
            canon = self._by_ext.get(ext)
        if canon is None or canon not in self._by_name:
            raise ValueError(f"unknown format {name!r}; available: {', '.join(self.names())}")
        return self._by_name[canon]

    def for_path(self, path: Union[str, os.PathLike[str]]) -> FormatCodec:
        """Resolve the format codec matching a file path's suffix.

        Args:
            path: File path or filename with extension.

        Returns:
            Matching `FormatCodec`.

        Raises:
            ValueError: If the path has no registered extension.

        Examples:
            >>> from oxide_parse.formats import FORMATS
            >>> FORMATS.for_path("schemas/books.toml").name
            'toml'
        """
        suffix = Path(path).suffix.lower()
        canon = self._by_ext.get(suffix)
        if canon is None or canon not in self._by_name:
            exts = ", ".join(sorted(self._by_ext))
            raise ValueError(f"no format registered for extension {suffix or '<none>'!r}; supported: {exts}")
        return self._by_name[canon]

    def names(self, *, readable: bool = False, writable: bool = False, data_only: bool = False) -> List[str]:
        """Return sorted canonical format names filtered by codec capabilities.

        Args:
            readable: Keep only formats supporting deserialization (`loads`).
            writable: Keep only formats supporting serialization (`dumps`).
            data_only: Exclude `ir_only` formats such as `"python"`.

        Returns:
            Sorted list of matching format names.

        Examples:
            >>> from oxide_parse.formats import FORMATS
            >>> "toml" in FORMATS.names(readable=True, writable=True)
            True
        """
        out: List[str] = []
        for canon, codec in self._by_name.items():
            if readable and codec.loads is None:
                continue
            if writable and codec.dumps is None:
                continue
            if data_only and codec.ir_only:
                continue
            out.append(canon)
        return sorted(out)

    def dumps(self, data: Any, format: str = "json", **options: Any) -> str:
        """Serialize `data` using the registered codec for `format`.

        Args:
            data: Python data or IR object to serialize.
            format: Registered format name, alias, or extension (default `"json"`).
            **options: Keyword options forwarded to the codec's `dumps` callback.

        Returns:
            Serialized text string.

        Raises:
            ValueError: If `format` is unknown or read-only.

        Examples:
            >>> from oxide_parse.formats import FORMATS
            >>> "a = 1" in FORMATS.dumps({"a": 1}, "toml")
            True
        """
        codec = self.get(format)
        if codec.dumps is None:
            raise ValueError(f"format {codec.name!r} does not support serialization")
        return codec.dumps(data, **options)

    def loads(self, text: Union[str, bytes, bytearray], format: str = "json", **options: Any) -> Any:
        """Deserialize `text` using the registered codec for `format`.

        Args:
            text: Encoded text or UTF-8 bytes.
            format: Registered format name, alias, or extension (default `"json"`).
            **options: Keyword options forwarded to the codec's `loads` callback.

        Returns:
            Deserialized Python value or IR dictionary.

        Raises:
            ValueError: If `format` is unknown or write-only.

        Examples:
            >>> from oxide_parse.formats import FORMATS
            >>> FORMATS.loads("a = 1\\n", "toml")
            {'a': 1}
        """
        codec = self.get(format)
        if codec.loads is None:
            raise ValueError(f"format {codec.name!r} does not support deserialization")
        raw = bytes(text).decode("utf-8") if isinstance(text, (bytes, bytearray)) else text
        return codec.loads(raw, **options)


FORMATS = FormatRegistry()


def register_format(
    name: str,
    *,
    dumps: Optional[Callable[..., str]] = None,
    loads: Optional[Callable[..., Any]] = None,
    extensions: Sequence[str] = (),
    aliases: Sequence[str] = (),
    ir_only: bool = False,
    overwrite: bool = True,
) -> FormatCodec:
    """Register a custom serialization/deserialization format on the global `FORMATS` factory.

    Args:
        name: Canonical format identifier.
        dumps: Serializer callback `(data, **options) -> str`.
        loads: Deserializer callback `(text, **options) -> Any`.
        extensions: File suffixes associated with this format.
        aliases: Alternate names resolving to `name`.
        ir_only: Mark codec as IR/DSL-only.
        overwrite: Replace existing codec of the same name (default `True`).

    Returns:
        Registered `FormatCodec`.

    Examples:
        >>> from oxide_parse import available_formats, register_format, unregister_format
        >>> _ = register_format("sample_fmt", dumps=str, loads=lambda s: {"raw": s})
        >>> "sample_fmt" in available_formats()
        True
        >>> unregister_format("sample_fmt")
    """
    return FORMATS.register(
        name,
        dumps=dumps,
        loads=loads,
        extensions=extensions,
        aliases=aliases,
        ir_only=ir_only,
        overwrite=overwrite,
    )


def unregister_format(name: str) -> None:
    """Remove a format codec from the global `FORMATS` factory.

    Args:
        name: Canonical format name or alias to remove.

    Examples:
        >>> from oxide_parse import available_formats, register_format, unregister_format
        >>> _ = register_format("tmp_doc", dumps=str)
        >>> unregister_format("tmp_doc")
        >>> "tmp_doc" in available_formats()
        False
    """
    FORMATS.unregister(name)


def get_format(name: str) -> FormatCodec:
    """Return the `FormatCodec` registered under `name` (or alias/extension) in `FORMATS`.

    Args:
        name: Format name, alias, or extension.

    Returns:
        Matching `FormatCodec`.

    Examples:
        >>> from oxide_parse import get_format
        >>> get_format("json").extensions
        ('.json',)
    """
    return FORMATS.get(name)


def available_formats(*, readable: bool = False, writable: bool = False) -> List[str]:
    """List sorted canonical format names currently registered in `FORMATS`.

    Args:
        readable: Only include formats with `loads`.
        writable: Only include formats with `dumps`.

    Returns:
        Sorted list of format names.

    Examples:
        >>> from oxide_parse import available_formats
        >>> all(f in available_formats() for f in ("json", "python", "toml", "yaml"))
        True
    """
    return FORMATS.names(readable=readable, writable=writable)


def dumps_format(data: Any, format: str = "json", **options: Any) -> str:
    """Serialize `data` using the global `FORMATS` factory.

    Args:
        data: Value or schema IR to serialize.
        format: Registered format name, alias, or suffix.
        **options: Extra keyword arguments forwarded to the codec.

    Returns:
        Serialized string.

    Examples:
        >>> from oxide_parse import dumps_format
        >>> "k = 2" in dumps_format({"k": 2}, "toml")
        True
    """
    return FORMATS.dumps(data, format, **options)


def loads_format(text: Union[str, bytes, bytearray], format: str = "json", **options: Any) -> Any:
    """Deserialize `text` using the global `FORMATS` factory.

    Args:
        text: Encoded string or UTF-8 bytes.
        format: Registered format name, alias, or suffix.
        **options: Extra keyword arguments forwarded to the codec.

    Returns:
        Parsed value or schema IR.

    Examples:
        >>> from oxide_parse import loads_format
        >>> loads_format("k = 2\\n", "toml")
        {'k': 2}
    """
    return FORMATS.loads(text, format, **options)
