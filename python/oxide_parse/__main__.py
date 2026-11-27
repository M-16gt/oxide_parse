"""Command-line interface (`python -m oxide_parse ...` or `oxide-parse ...`).

Examples:
    Run from a shell:

        oxide-parse distill page.html --max-chars 8000
        oxide-parse markdown page.html --selector article --without ".ad, .share"
        oxide-parse run schema.json page.html --backend auto --explain
        oxide-parse decompile schema.json --out schema.py
        oxide-parse validate schema.json
        oxide-parse migrate --from crawl4ai strategy.json --format python
        oxide-parse prompt generate
"""
from __future__ import annotations

import argparse
import json
import pathlib
import sys
from typing import Any, List, Optional

from . import __version__


class CliError(Exception):
    """Expected CLI error printed as a single concise line (exit status 1)."""


def _read_text(path: str) -> str:
    if path == "-":
        return sys.stdin.read()
    try:
        return pathlib.Path(path).read_text(encoding="utf-8", errors="replace")
    except OSError as e:
        raise CliError(f"cannot read {path}: {e.strerror or e}") from e


def _write(text: str, out: Optional[str]) -> None:
    if out:
        pathlib.Path(out).write_bytes(text.encode("utf-8"))
        print(f"wrote {out}", file=sys.stderr)
    else:
        sys.stdout.write(text if text.endswith("\n") else text + "\n")


def _load_schema(path: str) -> Any:
    """Return `(ir_dict, None)` for serialized IR files or `(None, schema)` for a `.py` module."""
    from .formats import FORMATS

    text = _read_text(path)
    suffix = pathlib.Path(path).suffix.lower() if path != "-" else ""
    if suffix == ".py":
        ns: dict = {"__name__": "oxide_parse_schema", "__file__": path}
        try:
            exec(compile(text, path, "exec"), ns)
        except Exception as e:
            raise CliError(f"{path}: {type(e).__name__}: {e}") from e
        for name in ("schema", "SCHEMA"):
            if name in ns:
                return None, ns[name]
        raise CliError(f"{path} must define a variable `schema`")

    fmt = "json"
    if suffix:
        try:
            fmt = FORMATS.for_path(path).name
        except ValueError:
            fmt = "json"
    try:
        data = FORMATS.loads(text, fmt)
    except ImportError as e:
        raise CliError(str(e)) from e
    except Exception as e:
        raise CliError(f"{path}: invalid {fmt.upper()} ({e})") from e
    if isinstance(data, dict) and "baseSelector" in data:
        raise CliError(f"{path} looks like a Crawl4AI schema: convert it first, `oxide-parse migrate --from crawl4ai {path}`")
    return data, None


def _plan(path: str, backend: str) -> Any:
    from .plan import Plan

    ir, schema = _load_schema(path)
    return Plan.from_ir(ir, backend) if schema is None else Plan(schema, backend)


def _ir_of(path: str) -> dict:
    from .expr import to_ir

    ir, schema = _load_schema(path)
    return ir if schema is None else to_ir(schema)


# -- commands ---------------------------------------------------------------------------------------------------------------
def _cmd_distill(a: argparse.Namespace) -> int:
    from .plan import distill

    _write(distill(_read_text(a.html), keep=a.keep, max_text=a.max_text, max_chars=a.max_chars), a.out)
    return 0


def _cmd_markdown(a: argparse.Namespace) -> int:
    from .plan import markdown

    _write(markdown(_read_text(a.html), a.selector, without=a.without, backend=a.backend), a.out)
    return 0


def _cmd_run(a: argparse.Namespace) -> int:
    from .formats import FORMATS

    plan = _plan(a.schema, a.backend)
    result = plan.extract(_read_text(a.html))
    print(f"engine: {plan.engine}", file=sys.stderr)
    if a.explain:
        print(plan.explain(), file=sys.stderr)
    try:
        opts = {"indent": 2} if FORMATS.get(a.format).name == "json" else {}
        rendered = FORMATS.dumps(result, a.format, **opts)
    except (ImportError, ValueError) as e:
        raise CliError(str(e)) from e
    _write(rendered, a.out)
    return 0


def _cmd_decompile(a: argparse.Namespace) -> int:
    from .decompile import decompile

    _write(decompile(_ir_of(a.schema), a.name), a.out)
    return 0


def _cmd_validate(a: argparse.Namespace) -> int:
    from .plan import validate_ir

    errors = validate_ir(_ir_of(a.schema), a.backend)
    for e in errors:
        print(e, file=sys.stderr)
    if not errors:
        print("ok", file=sys.stderr)
    return 1 if errors else 0


def _cmd_migrate(a: argparse.Namespace) -> int:
    from . import migrate as m
    from .formats import FORMATS

    text = _read_text(a.file)
    data: Any = None
    loaded = False
    if a.file != "-":
        try:
            codec = FORMATS.for_path(a.file)
            if not codec.ir_only:
                data = FORMATS.loads(text, codec.name)
                loaded = True
        except Exception:
            loaded = False
    if not loaded:
        last_err: Optional[Exception] = None
        for candidate in ("json", "yaml", "toml"):
            try:
                data = FORMATS.loads(text, candidate)
                if not isinstance(data, dict):
                    raise ValueError(f"{candidate.upper()} schema must be an object")
                loaded = True
                break
            except Exception as exc:
                last_err = exc
        if not loaded:
            raise CliError(f"{a.file}: neither JSON, YAML, nor TOML ({last_err})")
    try:
        ir = m.from_crawl4ai(data, kind=a.kind, wrap=a.wrap) if a.source == "crawl4ai" else m.from_selector_map(data)
    except m.MigrationError as e:
        raise CliError(str(e)) from e
    target_fmt = "json" if a.format == "ir" else a.format
    try:
        norm = FORMATS.get(target_fmt).name
        if norm == "python":
            rendered = FORMATS.dumps(ir, "python", name=a.name)
        elif norm == "json":
            rendered = FORMATS.dumps(ir, "json", indent=2)
        else:
            rendered = FORMATS.dumps(ir, norm)
    except (ImportError, ValueError) as e:
        raise CliError(str(e)) from e
    _write(rendered, a.out)
    return 0


def _cmd_prompt(a: argparse.Namespace) -> int:
    from . import prompts

    if a.kind == "generate":
        fmt = getattr(a, "format", "python")
        if fmt == "ir":
            _write(prompts.ir_generate_prompt(kind=getattr(a, "kind_sel", "css")), a.out)
        elif fmt == "crawl4ai":
            _write(prompts.crawl4ai_schema_prompt(schema_type=getattr(a, "kind_sel", "css").upper(), output="crawl4ai"), a.out)
        else:
            _write(prompts.generate_prompt(), a.out)
    else:
        try:
            _write(prompts.migrate_prompt(a.source), a.out)
        except ValueError as e:
            raise CliError(str(e)) from e
    return 0


def _cmd_ir_schema(a: argparse.Namespace) -> int:
    from .plan import ir_schema

    _write(json.dumps(ir_schema(), ensure_ascii=False, indent=2), a.out)
    return 0


def build_parser() -> argparse.ArgumentParser:
    """Build the top-level CLI `ArgumentParser` with all `oxide-parse` subcommands.

    Returns:
        Configured parser instance.

    Examples:
        >>> from oxide_parse.__main__ import build_parser
        >>> build_parser().parse_args(["validate", "schema.json"]).command
        'validate'
    """
    p = argparse.ArgumentParser(prog="oxide-parse", description="oxide_parse: declarative HTML extraction, from the shell.")
    p.add_argument("--version", action="version", version=f"oxide-parse {__version__}")
    sub = p.add_subparsers(dest="command", metavar="<command>")
    sub.required = True

    def add(name: str, help: str, fn: Any) -> argparse.ArgumentParser:
        sp = sub.add_parser(name, help=help, description=help)
        sp.set_defaults(fn=fn)
        return sp

    def out(sp: argparse.ArgumentParser) -> None:
        sp.add_argument("--out", "-o", metavar="FILE", help="write here instead of stdout")

    s = add("distill", "shrink an HTML file to a skeleton for a human or an LLM prompt", _cmd_distill)
    s.add_argument("html", help="HTML file (- for stdin)")
    s.add_argument("--max-chars", type=int, default=20000)
    s.add_argument("--keep", type=int, default=2, help="similar siblings kept before folding (default 2)")
    s.add_argument("--max-text", type=int, default=80, help="characters of text kept per node (default 80)")
    out(s)

    s = add("markdown", "convert an HTML file (or one element of it) to Markdown", _cmd_markdown)
    s.add_argument("html", help="HTML file (- for stdin)")
    s.add_argument("--selector", default="body", help="CSS selector of the element (default body)")
    s.add_argument("--without", help="CSS selector of subtrees to leave out, e.g. '.ad, .share'")
    s.add_argument("--backend", default="html")
    out(s)

    s = add("run", "run a schema (.json / .yaml / .toml IR, or a .py file defining `schema`) on an HTML file", _cmd_run)
    s.add_argument("schema")
    s.add_argument("html", help="HTML file (- for stdin)")
    s.add_argument("--backend", default="auto", help="auto, html, lite, scraper, stream, xml (default auto)")
    s.add_argument("--explain", action="store_true", help="print the execution plan to stderr")
    s.add_argument("--format", choices=("json", "yaml", "toml"), default="json")
    out(s)

    s = add("decompile", "turn a JSON/YAML/TOML IR (or a .py schema) into readable Python code", _cmd_decompile)
    s.add_argument("schema")
    s.add_argument("--name", default="schema", help="variable name in the generated code")
    out(s)

    s = add("validate", "check a schema (.json/.yaml/.toml/.py) and report every problem with its path (exit 1 if any)", _cmd_validate)
    s.add_argument("schema")
    s.add_argument("--backend", default="auto")

    s = add("migrate", "convert another tool's schema (Crawl4AI, flat selector map) to oxide_parse", _cmd_migrate)
    s.add_argument("file", help="JSON/YAML/TOML file")
    s.add_argument("--from", dest="source", choices=("crawl4ai", "selector-map"), required=True)
    s.add_argument("--format", choices=("ir", "json", "python", "yaml", "toml"), default="ir")
    s.add_argument("--kind", choices=("css", "xpath"), help="Crawl4AI strategy kind (default: guessed)")
    s.add_argument("--wrap", metavar="NAME", help='return {"NAME": [...]} instead of the bare list')
    s.add_argument("--name", default="schema", help="variable name for --format python")
    out(s)

    s = add("prompt", "print a system prompt for an LLM that writes or migrates schemas", _cmd_prompt)
    s.add_argument("kind", choices=("generate", "migrate"))
    s.add_argument("--from", dest="source", default="bs4", help="migrate: bs4, crawl4ai, scrapy, selectolax, lxml")
    s.add_argument("--format", choices=("python", "ir", "crawl4ai"), default="python", help="generate target format")
    s.add_argument("--selector-kind", dest="kind_sel", choices=("css", "xpath"), default="css", help="selector mode for --format ir")
    out(s)

    s = add("ir-schema", "print the JSON Schema of the IR", _cmd_ir_schema)
    out(s)
    return p


def main(argv: Optional[List[str]] = None) -> int:
    """CLI entry point returning an integer process exit status.

    Args:
        argv: Argument vector excluding program name (defaults to `sys.argv[1:]`).

    Returns:
        `0` on success, `1` on handled CLI/schema/runtime errors.

    Examples:
        >>> from oxide_parse.__main__ import main
        >>> main(["prompt", "generate", "--out", ""]) >= 0
        True
    """
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8")  # type: ignore[union-attr]  # Windows consoles default to cp125x
        except (AttributeError, ValueError):
            pass
    args = build_parser().parse_args(argv)
    try:
        return int(args.fn(args))
    except CliError as e:
        print(f"oxide-parse: {e}", file=sys.stderr)
        return 1
    except BrokenPipeError:
        return 0
    except Exception as e:
        print(f"oxide-parse: {type(e).__name__}: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
