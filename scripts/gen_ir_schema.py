"""Regenerate python/oxide_parse/ir.schema.json from the IR table (python/oxide_parse/ir_spec.py).

    python scripts/gen_ir_schema.py          # write the file
    python scripts/gen_ir_schema.py --check  # exit 1 if the committed file is stale (CI)
"""
import importlib.util
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
SPEC = ROOT / "python" / "oxide_parse" / "ir_spec.py"
OUT = ROOT / "python" / "oxide_parse" / "ir.schema.json"

spec = importlib.util.spec_from_file_location("ir_spec", SPEC)
mod = importlib.util.module_from_spec(spec)
sys.modules["ir_spec"] = mod  # dataclass-free, but typing needs the module registered
spec.loader.exec_module(mod)
text = mod.schema_json()

if "--check" in sys.argv:
    if not OUT.exists() or OUT.read_text(encoding="utf-8") != text:
        print(f"{OUT} is stale: run python scripts/gen_ir_schema.py", file=sys.stderr)
        sys.exit(1)
    print("ir.schema.json is up to date")
else:
    OUT.write_bytes(text.encode("utf-8"))
    print(f"wrote {OUT}")
