#!/usr/bin/env python3
"""Master runner for the oxide_parse comprehensive benchmark suite.

Generates both `results.json` and a formatted `BENCHMARK_REPORT.md` with bottleneck analysis.
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import platform
import sys
import time
from pathlib import Path
from typing import Any, Dict, List

from competitors import SKIPPED, detect_libraries
import suite_scenarios as sc


def format_markdown_report(meta: Dict[str, Any], sections: List[Dict[str, Any]]) -> str:
    lines: List[str] = []
    lines.append(f"# `oxide_parse` Full Benchmark Report — {meta['started_utc'][:10]}")
    lines.append("")
    lines.append("## Environment & Library Versions")
    lines.append(f"- **OS / Platform:** `{meta['platform']}`")
    lines.append(f"- **CPU / Cores:** `{meta['processor']}` ({meta['cpu_count']} logical cores)")
    lines.append(f"- **Python:** `{meta['python']}`")
    lines.append(f"- **Run parameters:** `repeat={meta['repeat']}`, `cooldown={meta['cooldown_sec']}s`")
    lines.append("- **Detected libraries:**")
    for k, v in sorted(meta["libraries"].items()):
        lines.append(f"  - `{k}`: **{v}**")
    lines.append("")

    # Summary Table for Scenario 01 if present
    cat_secs = [s for s in sections if s["scenario"] == "01_catalog_scaling"]
    if cat_secs:
        lines.append("## 1. Summary Table: Catalog Scaling (Medians, ms)")
        all_impls: List[str] = []
        for s in cat_secs:
            for k in s["timings"]:
                if k not in all_impls:
                    all_impls.append(k)
        header = "| Items | Noise | KiB | " + " | ".join(all_impls) + " |"
        sep = "|---:|:---:|---:|" + "|".join(["---:"] * len(all_impls)) + "|"
        lines.append(header)
        lines.append(sep)
        for s in cat_secs:
            row = [str(s["n_products"]), str(s["noise"]), f"{s['bytes']/1024:.1f}"]
            best = min((v["median_ms"] for v in s["timings"].values()), default=1.0)
            for impl in all_impls:
                if impl in s["timings"]:
                    m = s["timings"][impl]["median_ms"]
                    cell = f"**{m:.3f}**" if m == best else f"{m:.3f}"
                    row.append(cell)
                else:
                    row.append("—")
            lines.append("| " + " | ".join(row) + " |")
        lines.append("")

        lines.append("### `extract` vs `run_native` Comparison")
        lines.append("")
        lines.append("Median deltas estimate Python object construction overhead. `run_native` also counts output values and frees the Rust `Value`, so negative deltas can appear on noisy runs.")
        lines.append("")
        lines.append("| Items | Noise | Engine | extract (ms) | run_native (ms) | Delta (ms) |")
        lines.append("|---:|:---:|---|---:|---:|---:|")
        for s in cat_secs:
            for eng, b in s.get("native_breakdown", {}).items():
                lines.append(
                    f"| {s['n_products']} | {s['noise']} | `{eng}` | {b['total_ms']:.3f} | "
                    f"{b['rust_native_ms']:.3f} | {b['difference_ms']:.3f} |"
                )
        lines.append("")

    # Detailed Tables for all scenarios
    lines.append("## 2. Scenario Results")
    for s in sections:
        if s["scenario"] == "01_catalog_scaling":
            continue
        lines.append(f"### {s['label']}")
        lines.append("| Implementation | Median (ms) | Min (ms) | IQR (ms) | Max (ms) | Ratio vs Leader |")
        lines.append("|---|---:|---:|---:|---:|---:|")
        timings = s["timings"]
        if not timings:
            lines.append("| *(no available libraries)* | — | — | — | — | — |")
            continue
        best = min(v["median_ms"] for v in timings.values())
        for name, st in sorted(timings.items(), key=lambda kv: kv[1]["median_ms"]):
            ratio = st["median_ms"] / best if best > 0 else 1.0
            lines.append(
                f"| `{name}` | **{st['median_ms']:.3f}** | {st['min_ms']:.3f} | "
                f"{st['iqr_ms']:.3f} | {st['max_ms']:.3f} | x{ratio:.2f} |"
            )
        lines.append("")

    lines.extend(["", "## Skipped Implementations", ""])
    for name, reason in meta.get("skipped", {}).items():
        lines.append(f"- `{name}` — {reason}")
    if meta.get("errors"):
        lines.extend(["", "## Scenario Errors", ""])
        for name, reason in meta["errors"].items():
            lines.append(f"- `{name}` — {reason}")
    lines.extend(["", "One warmup run, randomized order per round, GC run before timing, and all outputs verified. Cooldown pauses run between scenario groups. Sizes are in UTF-8 bytes.",
                  "", "OS background load, thermals, and CPU clocks were not pinned. `--quick` checks functionality on smaller catalogs.", ""])
    return "\n".join(lines)


def main() -> None:
    sys.stdout.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser(description="Run comprehensive oxide_parse benchmark suite")
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--quick", action="store_true", help="Smoke run (catalog sizes 100/1000, repeat=3)")
    mode.add_argument("--full", action="store_true", help="Full run (100, 1000, 10000 items, repeat=7)")
    ap.add_argument("--repeat", type=int, default=0, help="Override repeat count per scenario")
    ap.add_argument("--cooldown", type=float, default=1.0, help="Seconds to sleep between scenarios to prevent CPU thermal throttling")
    ap.add_argument("--filter", type=str, default="", help="Comma-separated scenario names to run (e.g. adaptive,xpath,bottleneck)")
    ap.add_argument("--output", type=str, default="benchmarks/latest_suite", help="Directory to save results.json and BENCHMARK_REPORT.md")
    args = ap.parse_args()
    if args.repeat < 0 or args.cooldown < 0:
        ap.error("repeat and cooldown must be nonnegative")

    repeat = args.repeat or (3 if args.quick else 7)
    sizes = [100, 1000] if args.quick else [100, 1000, 10000]
    filters = [f.strip().lower() for f in args.filter.split(",") if f.strip()]

    def should_run(name: str) -> bool:
        return not filters or any(f in name.lower() for f in filters)

    libs = detect_libraries()
    print("Detected libraries:", json.dumps(libs, indent=2))

    meta = {
        "started_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "platform": platform.platform(),
        "processor": platform.processor(),
        "cpu_count": os.cpu_count(),
        "python": sys.version,
        "repeat": repeat,
        "cooldown_sec": args.cooldown,
        "libraries": libs,
        "mode": "quick" if args.quick else "full",
        "skipped": SKIPPED,
        "errors": {},
    }
    from oxide_parse import _core, engines
    native = Path(_core.__file__)
    meta["native_sha256"] = hashlib.sha256(native.read_bytes()).hexdigest()
    meta["engines"] = [e.name for e in engines()]
    meta["suite_sha256"] = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                            for p in Path(__file__).parent.glob("*.py")}
    root = Path(__file__).resolve().parents[2]
    meta["build_config_sha256"] = {
        name: hashlib.sha256((root / name).read_bytes()).hexdigest()
        for name in ("pyproject.toml", "Cargo.toml", "Cargo.lock", "crates/py/Cargo.toml")
    }

    sections: List[Dict[str, Any]] = []
    tasks = [
        ("01_catalog", lambda: sc.run_01_catalog_scaling(sizes, repeat)),
        ("02_early_exit", lambda: [sc.run_02_early_exit(repeat)]),
        ("03_deep_css", lambda: [sc.run_03_deep_combinators(repeat)]),
        ("04_xpath", lambda: [sc.run_04_xpath_torture(repeat)]),
        ("05_hybrid_json", lambda: [sc.run_05_hybrid_html_json(repeat)]),
        ("06_text_entities", lambda: [sc.run_06_text_and_entities(repeat)]),
        ("07_adaptive", lambda: sc.run_07_adaptive_redesign(repeat)),
        ("08_batch_parallel", lambda: [sc.run_08_batch_parallel(repeat)]),
        ("09_bottlenecks", lambda: sc.run_09_internal_bottlenecks(repeat)),
        ("10_markdown_rag", lambda: [sc.run_10_markdown_rag(repeat)]),
        ("11_scrapling_bench", lambda: sc.run_11_scrapling_benchmark(repeat)),
        ("12_selectolax_bench", lambda: [sc.run_12_selectolax_benchmark(repeat)]),
        ("13_frostwork_bench", lambda: sc.run_13_frostwork_benchmark(repeat)),
        ("14_parsel_quotes_bench", lambda: [sc.run_14_parsel_scrapy_quotes(repeat)]),
        ("15_crawl4ai_nested_bench", lambda: [sc.run_15_crawl4ai_nested_ecommerce(repeat)]),
    ]

    if not any(should_run(tag) for tag, _ in tasks):
        ap.error("filter did not match any scenario")

    out_dir = Path(args.output)
    out_dir.mkdir(parents=True, exist_ok=True)
    results_path = out_dir / "results.json"
    report_path = out_dir / "BENCHMARK_REPORT.md"

    def save():
        meta["updated_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        results_path.write_text(json.dumps({"meta": meta, "sections": sections}, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        report_path.write_text(format_markdown_report(meta, sections), encoding="utf-8")

    for tag, fn in tasks:
        if not should_run(tag):
            continue
        print(f"\n>>> Running [{tag}] ...", flush=True)
        t0 = time.perf_counter()
        try:
            out = fn()
        except Exception as exc:
            meta["errors"][tag] = f"{type(exc).__name__}: {exc}"
            print(f"FAILED [{tag}]: {exc}", flush=True)
            save()
            continue
        elapsed = time.perf_counter() - t0
        sections.extend(out)
        save()
        for item in out:
            print(f"  {item['label']} ({elapsed:.1f}s):")
            for k, v in sorted(item["timings"].items(), key=lambda kv: kv[1]["median_ms"]):
                print(f"    {k:<38} median={v['median_ms']:8.3f} ms  (min={v['min_ms']:.3f}, iqr={v['iqr_ms']:.3f})")
        if args.cooldown > 0:
            time.sleep(args.cooldown)

    save()

    print(f"\nSaved JSON results to: {results_path}")
    print(f"Saved Markdown report to: {report_path}")
    if meta["errors"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
