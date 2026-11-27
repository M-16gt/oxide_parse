# CLI Reference (`oxide-parse`)

Installing the package exposes the `oxide-parse` command (also runnable via `python -m oxide_parse`).

Global conventions:
- Pass `-` anywhere an input HTML file is expected to read from `stdin`.
- `--out FILE` (`-o FILE`) writes UTF-8 output to a file instead of `stdout`.
- Errors print a single concise line to `stderr` and exit with status `1`.

## Commands

### `distill` — shrink HTML into a compact skeleton for LLMs or inspection

```bash
oxide-parse distill page.html --max-chars 8000 -o skeleton.html
```

- `html` — path to HTML file or `-`.
- `--max-chars N` — maximum output characters (default `20000`).
- `--keep N` — similar sibling elements retained before folding (default `2`).
- `--max-text N` — maximum text characters kept per node (default `80`).
- Requires the `lite` engine in the native build.

### `markdown` — convert an HTML document or subtree to Markdown

```bash
oxide-parse markdown page.html --selector article --without ".ad, .share"
```

- `html` — path to HTML file or `-`.
- `--selector SEL` — CSS selector of the target root element (default `body`).
- `--without SEL` — CSS selector of subtrees to drop.
- `--backend NAME` — engine name (default `html`).

### `run` — execute a schema against a local HTML file

```bash
oxide-parse run schema.py page.html --backend auto --explain
oxide-parse run schema.json page.html --format yaml
```

- `schema` — path to `.json`, `.yaml`/`.yml` (IR), or `.py` (must define `schema` or `SCHEMA`). Crawl4AI JSON containing `baseSelector` prints a hint pointing to `migrate`.
- `html` — path to HTML file or `-`.
- `--backend NAME` — `auto` (default), `html`, `lite`, `scraper`, `stream`.
- `--explain` — print the compiled plan and rewrites to `stderr`.
- `--format json|yaml` — output format (default `json`; `yaml` requires `oxide-parse[yaml]`).
- Prints `engine: <name>` to `stderr`.

### `validate` — check a schema and list every error

```bash
oxide-parse validate schema.json --backend auto
```

- `schema` — `.json`, `.yaml`/`.yml`, or `.py`.
- `--backend NAME` — engine to compile-check against (default `auto`).
- Prints all issues to `stderr` (or `ok` if valid) and exits with `0` or `1`.

### `decompile` — convert IR back into readable Python DSL code

```bash
oxide-parse decompile schema.json --out schema.py --name schema
```

- `schema` — `.json`, `.yaml`/`.yml`, or `.py`.
- `--name IDENT` — target variable name in generated Python code (default `schema`).

### `migrate` — convert Crawl4AI or selector-map definitions to `oxide_parse`

```bash
oxide-parse migrate strategy.json --from crawl4ai --format python -o schema.py
oxide-parse migrate selectors.json --from selector-map --format ir
```

- `file` — input JSON or YAML file.
- `--from crawl4ai|selector-map` — source schema format (required).
- `--format ir|python|yaml` — output format (default `ir`).
- `--kind css|xpath` — override Crawl4AI selector kind (auto-detected by default).
- `--wrap NAME` — wrap Crawl4AI list root as `{"NAME": [...]}`.
- `--name IDENT` — variable name for `--format python` (default `schema`).

### `prompt` — print a system prompt for writing or migrating schemas

```bash
oxide-parse prompt generate
oxide-parse prompt migrate --from bs4
```

- `kind` — `generate` or `migrate`.
- `--from` — migration source: `bs4` (default), `crawl4ai`, `scrapy`, `selectolax`, `lxml`.

### `ir-schema` — print the JSON Schema of the IR format

```bash
oxide-parse ir-schema -o ir.schema.json
```

Outputs Draft 2020-12 JSON Schema describing all `Expr` and `Op` nodes.

### `--version`

Print the installed `oxide-parse` version.
