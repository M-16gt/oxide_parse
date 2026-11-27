# IR Specification (`Expr` and `Op`)

The IR is the JSON/YAML intermediate representation of a schema. `to_ir(schema)` compiles the Python DSL (`T`) into IR, `decompile(ir)` reconstructs Python code from IR, and Rust `Plan::compile` executes IR directly.

Machine-readable JSON Schema (Draft 2020-12): [ir.schema.json](../../python/oxide_parse/ir.schema.json) (also returned by `oxide_parse.ir_schema()`). The single source of truth is [ir_spec.py](../../python/oxide_parse/ir_spec.py), verified against the JSON Schema and Rust AST by `python scripts/gen_ir_schema.py --check`.

## `Expr` Nodes

Every `Expr` node is a JSON object with exactly one of these four keys:

| Kind | Shape | Meaning |
|---|---|---|
| `pipe` | `{"pipe": [op, ...]}` | Sequence of operations evaluated over the current context |
| `object` | `{"object": {"field": expr, ...}}` | Dictionary of named field expressions |
| `list` | `{"list": [expr, ...]}` | List of sub-expressions |
| `lit` | `{"lit": <json_value>}` | Any JSON-compatible literal value |

## `Op` Operations

### Unit Operations (`UNIT_OPS`)

Serialized inside `"pipe"` as plain strings:

- Navigation: `"parent"`, `"children"`, `"root"`, `"first"`, `"last"`.
- Extraction: `"text"`, `"html"`, `"inner_html"`, `"index"`.
- Collections & nulls: `"count"`, `"exists"`, `"required"`, `"non_empty"`.
- Strings: `"strip"`, `"lower"`, `"upper"`.
- Conversions & logic: `"int"`, `"float"`, `"bool"`, `"parse_json"`, `"not"`.

### Single-Argument Scalar Operations

Serialized as `{"<name>": arg}`:

| Op | Argument Type | Purpose |
|---|---|---|
| `css` | `str` | CSS selector |
| `xpath` | `str` | XPath 1.0 expression |
| `key` | `str` | Object key lookup |
| `attr` | `str` | Attribute lookup |
| `has_attr` | `str` | Attribute presence check |
| `meta` | `str` | Metadata dictionary key lookup |
| `split` | `str` | Split string by delimiter |
| `join` | `str` | Join list by delimiter |
| `at` | `uint` (`int >= 0`) | Zero-based element index (Python `at` also handles negative indices; JSON Schema validates `uint`; use `slice` for general slices) |

*Note:* `python/oxide_parse/ir_spec.py` defines `at` as `uint`.

### Expression Operations

Serialized as `{"<name>": <Expr>}` (or `{"coalesce": [<Expr>, ...]}`):

- `map`, `flat_map`, `filter`
- `default`
- `eq`, `ne`, `contains`, `starts_with`, `ends_with`
- `and`, `or`
- `coalesce` (`list[Expr]`)

### Structured Operations (`STRUCT_OPS`)

Serialized as `{"<name>": {field: value, ...}}`:

| Op | Fields (`*` = required) |
|---|---|
| `slice` | `start?: int \| null`, `stop?: int \| null` |
| `text_opts` | `sep?: str`, `strip?: bool`, `without?: str \| null`, `br?: str \| null` |
| `markdown` | `without?: str \| null` |
| `distill` | `keep?: uint \| null`, `max_text?: uint \| null`, `max_chars?: uint \| null` |
| `adaptive` | `css*: str`, `key*: str`, `threshold?: float \| null` |
| `map_opts` | `body*: Expr`, `on_error?: "raise" \| "null" \| "skip"` |
| `resolve_url` | `base?: Expr \| null` |
| `js_json` | `var?: str \| null` |
| `sections` | `header*: str`, `items?: str \| null` |
| `group_by` | `header*: str`, `items?: str \| null` |
| `json_ld` | `schema_type?: str \| null` |
| `meta_tags` | `prefixes?: list[str]`, `values?: "auto" \| "first" \| "list" \| null` |
| `replace` | `from*: str`, `to*: str` |
| `regex` | `pattern*: str`, `group?: uint` |
| `regex_all` | `pattern*: str`, `group?: uint` |
| `if` | `cond*: Expr`, `then*: Expr`, `otherwise*: Expr` |
| `follow` | `request*: Expr`, `meta?: Expr \| null`, `schema*: Expr` |
| `paginate` | `items*: Expr`, `next*: Expr`, `meta?: Expr \| null`, `max_pages?: posint \| null`, `max_items?: posint \| null` |

## IR Validation

`oxide_parse.validate_ir(ir, backend="auto") -> list[str]` walks the entire IR tree and collects every issue at once with paths (`$.field[0].op: message`) instead of stopping at the first error. When the native module is loaded, it also checks compilation against `backend` (CSS/XPath syntax and engine compatibility).

CLI equivalents:

```bash
oxide-parse validate schema.json --backend auto
oxide-parse ir-schema
```
